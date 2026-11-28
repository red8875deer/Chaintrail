// One way to call the app, wherever it runs (ADR-0010):
// - desktop (Tauri): invoke(<command>, { req }) and `trace_progress` events;
// - browser (`chaintrail serve`): POST /api/<command> with the per-run token;
// - mock (`?mock=1`): answers from recorded fixtures, for demos and UI tests.

import type { Commands, ServiceError, TraceProgress } from "./types";

type Req<K extends keyof Commands> = Commands[K][0];
type Res<K extends keyof Commands> = Commands[K][1];

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export const isDesktop = typeof window !== "undefined" && !!window.__TAURI_INTERNALS__;
export const isMock =
  typeof window !== "undefined" && new URLSearchParams(window.location.search).has("mock");

const TOKEN_KEY = "chaintrail-token";

/** Read `#token=…` once (the link `chaintrail serve` prints), keep it for the session. */
export function captureToken(): void {
  const m = /^#token=([0-9a-zA-Z_-]+)/.exec(window.location.hash);
  if (m?.[1]) {
    try {
      sessionStorage.setItem(TOKEN_KEY, m[1]);
    } catch {
      /* storage unavailable: token stays in memory below */
    }
    memoryToken = m[1];
    history.replaceState(null, "", window.location.pathname + window.location.search + "#/");
  }
}

let memoryToken: string | null = null;

function token(): string | null {
  if (memoryToken) return memoryToken;
  try {
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export class ApiError extends Error implements ServiceError {
  kind: ServiceError["kind"];
  constructor(e: ServiceError) {
    super(e.message);
    this.kind = e.kind;
  }
}

function asServiceError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  if (e && typeof e === "object" && "kind" in e && "message" in e) {
    return new ApiError(e as ServiceError);
  }
  const message = e instanceof Error ? e.message : String(e);
  return new ApiError({ kind: "internal", message });
}

export async function call<K extends keyof Commands>(name: K, req: Req<K>): Promise<Res<K>> {
  try {
    if (isMock) {
      const { mockCall } = await import("./mock");
      return (await mockCall(name, req)) as Res<K>;
    }
    if (isDesktop) {
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<Res<K>>(name, { req });
    }
    const t = token();
    const res = await fetch(`api/${name}`, {
      method: "POST",
      headers: { "content-type": "application/json", ...(t ? { "x-chaintrail-token": t } : {}) },
      body: JSON.stringify(req),
    });
    if (res.status === 401) {
      throw new ApiError({
        kind: "invalid_input",
        message:
          "This page is not connected to ChainTrail. Open the link that `chaintrail serve` printed (it ends with #token=…).",
      });
    }
    if (!res.ok) {
      const body = await res.json().catch(() => null);
      throw asServiceError(body ?? { kind: "internal", message: `HTTP ${res.status}` });
    }
    return (await res.json()) as Res<K>;
  } catch (e) {
    if (e instanceof TypeError && !isDesktop && !isMock) {
      throw new ApiError({
        kind: "unavailable",
        message: "Cannot reach ChainTrail. Is `chaintrail serve` still running?",
      });
    }
    throw asServiceError(e);
  }
}

/** A trace that can report progress and be stopped (it then ends early with what it found). */
export interface RunningTrace {
  result: Promise<Res<"btc_trace">>;
  stop: () => void;
}

/** Start a trace with progress and Stop, in the desktop app and in the browser alike. */
export function startTrace(req: Req<"btc_trace">, onProgress: (p: TraceProgress) => void): RunningTrace {
  const id = `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
  if (isMock) return { result: call("btc_trace", req), stop: () => {} };
  if (isDesktop) {
    let unlisten: (() => void) | null = null;
    const result = (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<TraceProgress>("trace_progress", (e) => {
        if (e.payload.id === id) onProgress(e.payload);
      });
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        return await invoke<Res<"btc_trace">>("btc_trace", { req, id });
      } catch (e) {
        throw asServiceError(e);
      } finally {
        unlisten?.();
      }
    })();
    const stop = () => {
      void import("@tauri-apps/api/core").then(({ invoke }) => invoke("trace_stop", { req: { id } }).catch(() => {}));
    };
    return { result, stop };
  }
  // Browser: the trace runs in one request; progress is asked for alongside it.
  const t = token();
  const headers = { "content-type": "application/json", ...(t ? { "x-chaintrail-token": t } : {}) };
  let done = false;
  const poll = async () => {
    while (!done) {
      await new Promise((r) => setTimeout(r, 700));
      if (done) break;
      try {
        const res = await fetch("api/trace_progress", { method: "POST", headers, body: JSON.stringify({ id }) });
        const p = res.ok ? ((await res.json()) as TraceProgress | null) : null;
        if (p && !done) onProgress(p);
      } catch {
        /* progress is best effort */
      }
    }
  };
  void poll();
  const result = (async () => {
    try {
      const res = await fetch("api/btc_trace", {
        method: "POST",
        headers: { ...headers, "x-chaintrail-trace-id": id },
        body: JSON.stringify(req),
      });
      if (res.status === 401) {
        throw new ApiError({
          kind: "invalid_input",
          message:
            "This page is not connected to ChainTrail. Open the link that `chaintrail serve` printed (it ends with #token=…).",
        });
      }
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        throw asServiceError(body ?? { kind: "internal", message: `HTTP ${res.status}` });
      }
      return (await res.json()) as Res<"btc_trace">;
    } catch (e) {
      if (e instanceof TypeError) {
        throw new ApiError({ kind: "unavailable", message: "Cannot reach ChainTrail. Is `chaintrail serve` still running?" });
      }
      throw asServiceError(e);
    } finally {
      done = true;
    }
  })();
  // keepalive: the stop still reaches the app if the page is being closed or reloaded.
  const stop = () => {
    void fetch("api/trace_stop", { method: "POST", headers, body: JSON.stringify({ id }), keepalive: true }).catch(() => {});
  };
  return { result, stop };
}

/** Listen to trace progress (desktop only). Returns an unsubscribe function. */
export async function onTraceProgress(cb: (p: TraceProgress) => void): Promise<() => void> {
  if (!isDesktop) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<TraceProgress>("trace_progress", (e) => cb(e.payload));
}
