// Hash routes, so links work the same in the desktop app and in the browser:
// #/  #/address/<addr>  #/tx/<txid>  #/trace?txid=…&vout=…&to=…&dir=forward&run=1  #/labels

import { useEffect, useState } from "react";
import type { Direction } from "../api/types";

export interface TraceParams {
  txid: string;
  vout?: number;
  to?: string;
  direction: Direction;
  maxHops?: number;
  maxTxs?: number;
  minBtc?: string;
  checkAddresses?: boolean;
  /** Continuing an earlier trace: follow only this many sats (the share it traced here). */
  share?: number;
  /** Continuing an earlier trace whose funds were mixed in this transaction. */
  mixed?: string;
  /** Continuing an earlier trace: its seed, for the record. */
  from?: string;
  /** Start the trace immediately. */
  run?: boolean;
}

export type Route =
  | { page: "home" }
  | { page: "address"; address: string }
  | { page: "tx"; txid: string }
  | { page: "trace"; params: TraceParams | null }
  | { page: "labels"; address?: string };

export function parseRoute(hash: string): Route {
  const h = hash.replace(/^#/, "");
  const [path = "", query = ""] = h.split("?");
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  const q = new URLSearchParams(query);
  switch (parts[0]) {
    case "address":
      return parts[1] ? { page: "address", address: parts[1] } : { page: "home" };
    case "tx":
      return parts[1] ? { page: "tx", txid: parts[1] } : { page: "home" };
    case "labels":
      return { page: "labels", address: q.get("address") ?? undefined };
    case "trace": {
      const txid = q.get("txid");
      if (!txid) return { page: "trace", params: null };
      const num = (k: string) => {
        const v = q.get(k);
        return v !== null && v !== "" && /^\d+$/.test(v) ? Number(v) : undefined;
      };
      return {
        page: "trace",
        params: {
          txid,
          vout: num("vout"),
          to: q.get("to") ?? undefined,
          direction: q.get("dir") === "backward" ? "backward" : "forward",
          maxHops: num("hops"),
          maxTxs: num("txs"),
          minBtc: q.get("min") ?? undefined,
          checkAddresses: q.get("check") === "0" ? false : undefined,
          share: num("share"),
          mixed: q.get("mixed") ?? undefined,
          from: q.get("from") ?? undefined,
          run: q.get("run") === "1",
        },
      };
    }
    default:
      return { page: "home" };
  }
}

export function href(route: Route): string {
  switch (route.page) {
    case "home":
      return "#/";
    case "address":
      return `#/address/${encodeURIComponent(route.address)}`;
    case "tx":
      return `#/tx/${encodeURIComponent(route.txid)}`;
    case "labels":
      return route.address ? `#/labels?address=${encodeURIComponent(route.address)}` : "#/labels";
    case "trace": {
      const p = route.params;
      if (!p) return "#/trace";
      const q = new URLSearchParams({ txid: p.txid, dir: p.direction });
      if (p.vout !== undefined) q.set("vout", String(p.vout));
      if (p.to) q.set("to", p.to);
      if (p.maxHops !== undefined) q.set("hops", String(p.maxHops));
      if (p.maxTxs !== undefined) q.set("txs", String(p.maxTxs));
      if (p.minBtc) q.set("min", p.minBtc);
      if (p.checkAddresses === false) q.set("check", "0");
      if (p.share !== undefined) q.set("share", String(p.share));
      if (p.mixed) q.set("mixed", p.mixed);
      if (p.from) q.set("from", p.from);
      if (p.run) q.set("run", "1");
      return `#/trace?${q.toString()}`;
    }
  }
}

/** Trace limits used when a link does not give them. */
export const DEFAULT_MAX_HOPS = 8;
export const DEFAULT_MAX_TXS = 100;

/** Two trace links describe the same trace (ignoring parameter order, defaults and `run`). */
export function sameTrace(a: TraceParams, b: TraceParams): boolean {
  const key = (p: TraceParams) =>
    [
      p.txid.toLowerCase(),
      p.vout ?? "",
      p.to ?? "",
      p.direction,
      p.maxHops ?? DEFAULT_MAX_HOPS,
      p.maxTxs ?? DEFAULT_MAX_TXS,
      p.minBtc ?? "",
      p.checkAddresses === false ? "0" : "",
      p.share ?? "",
      p.mixed ?? "",
    ].join("|");
  return key(a) === key(b);
}

export function go(route: Route): void {
  window.location.hash = href(route);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(window.location.hash));
  useEffect(() => {
    const on = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}
