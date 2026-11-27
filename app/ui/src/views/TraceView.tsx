// The trace workspace: form, graph, leads and details.

import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { TraceProgress, TraceRequest, TraceView as View } from "../api/types";
import { call, isDesktop, onTraceProgress } from "../api/client";
import { Btc, ErrorBox } from "../components/bits";
import { DetailsPanel } from "../components/DetailsPanel";
import { type GraphControls, GraphCanvas, type Selection } from "../components/GraphCanvas";
import { LeadsPanel } from "../components/LeadsPanel";
import { Legend } from "../components/Legend";
import { exportCsv, exportJson, exportPng } from "../lib/export";
import { extractId, formatElapsed, formatInt, parseBtc } from "../lib/format";
import { buildDisplay, defaultMinShareBp, displayPath, isGroupId, leadPath, leadSpine, nodeForLead } from "../lib/graph";
import { DEFAULT_MAX_HOPS, DEFAULT_MAX_TXS, type TraceParams, href, parseRoute, sameTrace } from "../lib/route";

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

interface FormState {
  txid: string;
  vout: string;
  to: string;
  direction: "forward" | "backward";
  maxHops: string;
  maxTxs: string;
  minBtc: string;
  checkAddresses: boolean;
  /** Continuing an earlier trace: its share of the seed (sats) and earlier mixing. */
  share?: number;
  mixed?: string;
}

function fromParams(p: TraceParams | null): FormState {
  return {
    txid: p?.txid ?? "",
    vout: p?.vout !== undefined ? String(p.vout) : "",
    to: p?.to ?? "",
    direction: p?.direction ?? "forward",
    maxHops: String(p?.maxHops ?? DEFAULT_MAX_HOPS),
    maxTxs: String(p?.maxTxs ?? DEFAULT_MAX_TXS),
    minBtc: p?.minBtc ?? "",
    checkAddresses: p?.checkAddresses ?? true,
    share: p?.share,
    mixed: p?.mixed,
  };
}

function toRequest(f: FormState): TraceRequest | string {
  // Explorer links are accepted: the transaction id is taken from them.
  const txid = extractId(f.txid);
  if (!/^[0-9a-fA-F]{64}$/.test(txid)) return "Enter a transaction id: 64 characters, 0–9 and a–f.";
  if (f.vout.trim() && f.to.trim()) return "Fill in either the output number or the recipient address, not both.";
  if (f.vout.trim() && !/^\d+$/.test(f.vout.trim())) return "The output number must be a whole number (0, 1, 2…).";
  if (f.vout.trim() && Number(f.vout.trim()) > 4_294_967_295) return "That output number is too large.";
  const min = f.minBtc.trim() ? parseBtc(f.minBtc) : null;
  if (f.minBtc.trim() && min === null) return "The minimum must be an amount in BTC, e.g. 0.01 (or 0,01).";
  if (min !== null && min > 21_000_000n * 100_000_000n) return "The minimum is larger than all bitcoin in existence.";
  // Limits are checked, never changed silently.
  const limit = (s: string, name: string, max: number, d: number): number | string => {
    if (!s.trim()) return d;
    if (!/^\d+$/.test(s.trim())) return `${name} must be a whole number.`;
    const n = Number(s.trim());
    return n < 1 || n > max ? `${name} must be between 1 and ${max}.` : n;
  };
  const hops = limit(f.maxHops, "Max hops", 100, DEFAULT_MAX_HOPS);
  if (typeof hops === "string") return hops;
  const txs = limit(f.maxTxs, "Max transactions", 5000, DEFAULT_MAX_TXS);
  if (typeof txs === "string") return txs;
  return {
    txid,
    vout: f.vout.trim() ? Number(f.vout.trim()) : null,
    to: f.to.trim() || null,
    direction: f.direction,
    max_hops: hops,
    max_txs: txs,
    min_sats: min !== null ? Number(min) : null,
    check_addresses: f.checkAddresses,
    share_sats: f.share ?? null,
    mixed_in: f.mixed ?? null,
  };
}

export function TraceView({ params }: { params: TraceParams | null }) {
  const [form, setForm] = useState<FormState>(() => fromParams(params));
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<TraceProgress | null>(null);
  const [started, setStarted] = useState(0);
  const [now, setNow] = useState(0);
  const [selectedLead, setSelectedLead] = useState<number | null>(null);
  const [selection, setSelection] = useState<Selection>(null);
  const [hideBp, setHideBp] = useState(0);
  // The slider moves freely; the graph follows once it settles (each change re-lays out).
  const [sliderBp, setSliderBp] = useState(0);
  useEffect(() => setSliderBp(hideBp), [hideBp]);
  useEffect(() => {
    if (sliderBp === hideBp) return;
    const t = setTimeout(() => setHideBp(sliderBp), 250);
    return () => clearTimeout(t);
  }, [sliderBp, hideBp]);
  const [simplify, setSimplify] = useState(true);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [noteClosed, setNoteClosed] = useState(false);
  /** Nodes to bring into view after the graph changes (e.g. an opened group). */
  const [viewTarget, setViewTarget] = useState<{ ids: string[] } | null>(null);
  const controls = useRef<GraphControls | null>(null);
  const runId = useRef(0);

  const run = useCallback(async (f: FormState) => {
    const req = toRequest(f);
    if (typeof req === "string") {
      setError(new Error(req));
      return;
    }
    // Every trace gets its own history entry and a link that reproduces it: Back and
    // Forward re-open earlier traces (instantly, from the local cache), and reloading or
    // sharing the link shows the same results.
    const target: TraceParams = {
      txid: req.txid,
      vout: req.vout ?? undefined,
      to: req.to ?? undefined,
      direction: req.direction,
      maxHops: req.max_hops ?? undefined,
      maxTxs: req.max_txs ?? undefined,
      minBtc: f.minBtc.trim() || undefined,
      checkAddresses: req.check_addresses ?? undefined,
      share: f.share,
      mixed: f.mixed,
      run: true,
    };
    const current = parseRoute(window.location.hash);
    const url = href({ page: "trace", params: target });
    if (current.page === "trace" && current.params && sameTrace(current.params, target)) {
      history.replaceState(null, "", url);
    } else {
      history.pushState(null, "", url);
    }
    const id = ++runId.current;
    setRunning(true);
    setError(null);
    // Old results never stay on screen under a new request (it may fail).
    setView(null);
    setProgress(null);
    setSelectedLead(null);
    setSelection(null);
    setStarted(Date.now());
    const stop = await onTraceProgress((p) => id === runId.current && setProgress(p));
    try {
      const v = await call("btc_trace", req);
      if (id !== runId.current) return;
      setView(v);
      setHideBp(defaultMinShareBp(v));
      setExpanded(new Set());
      setNoteClosed(false);
      setViewTarget(null);
      // Show the strongest result at once: the largest endpoint, else the largest branch.
      const first = v.report.leads.findIndex((l) => l.kind.kind !== "unexplored");
      setSelectedLead(first >= 0 ? first : v.report.leads.length > 0 ? 0 : null);
    } catch (e) {
      if (id === runId.current) setError(e);
    } finally {
      stop();
      if (id === runId.current) setRunning(false);
    }
  }, []);

  // Start immediately when opened from a "Trace…" link.
  useEffect(() => {
    const f = fromParams(params);
    setForm(f);
    if (params?.run) {
      void run(f);
    } else if (!params) {
      // A fresh trace page: drop the previous result and any trace still running.
      runId.current += 1;
      setRunning(false);
      setView(null);
      setError(null);
    }
  }, [params, run]);

  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(t);
  }, [running]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void run(form);
  };
  // Changing what is traced ends a continuation: its share belongs to that seed only.
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) =>
    setForm((f) => {
      const seedChanged = k === "txid" || k === "vout" || k === "to" || k === "direction";
      return { ...f, [k]: v, ...(seedChanged ? { share: undefined, mixed: undefined } : {}) };
    });

  // What is drawn: small flows hidden, small branches grouped. The main route to every lead
  // selected so far is revealed and stays revealed, so selecting leads does not reshuffle
  // the graph (it only changes when a route that was hidden has to be drawn).
  const reportPath = useMemo(
    () => (view && selectedLead !== null ? leadPath(view, selectedLead) : null),
    [view, selectedLead],
  );
  const revealedRef = useRef<{ view: View | null; nodes: Set<string>; edges: Set<string> }>({
    view: null,
    nodes: new Set(),
    edges: new Set(),
  });
  const revealed = useMemo(() => {
    let r = revealedRef.current;
    if (r.view !== view) r = { view, nodes: new Set(), edges: new Set() };
    if (view && selectedLead !== null) {
      const s = leadSpine(view, selectedLead);
      const missing = [...s.nodes].some((n) => !r.nodes.has(n)) || [...s.edges].some((e) => !r.edges.has(e));
      if (missing) r = { view, nodes: new Set([...r.nodes, ...s.nodes]), edges: new Set([...r.edges, ...s.edges]) };
    }
    revealedRef.current = r;
    return r;
  }, [view, selectedLead]);
  const display = useMemo(
    () =>
      view
        ? buildDisplay(view, {
            minShareBp: hideBp,
            simplify,
            expanded,
            keep: revealed.nodes,
            keepEdges: revealed.edges,
          })
        : null,
    [view, hideBp, simplify, expanded, revealed],
  );
  const path = useMemo(() => (display && reportPath ? displayPath(display, reportPath) : null), [display, reportPath]);
  const focusId = useMemo(() => {
    if (!view || !display || selectedLead === null) return null;
    const n = nodeForLead(view, selectedLead);
    return n ? (display.nodeOf.get(n.id) ?? null) : null;
  }, [view, display, selectedLead]);
  const expand = (id: string) => {
    const g = display?.groups.get(id);
    setExpanded((cur) => new Set([...cur, id]));
    setSelection(null);
    if (g) setViewTarget({ ids: g.members });
  };

  const selectLead = (i: number) => {
    setSelectedLead((cur) => (cur === i ? null : i));
    setSelection(null);
  };

  return (
    <div className="workspace">
      <form className="trace-form" onSubmit={submit} aria-label="Trace settings">
        <label className="field">
          <span>Transaction id</span>
          <input
            name="txid"
            value={form.txid}
            onChange={(e) => set("txid", e.target.value)}
            placeholder="64-character transaction id"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="field">
          <span>Output #</span>
          <input className="narrow" value={form.vout} onChange={(e) => set("vout", e.target.value)} inputMode="numeric" placeholder="all" />
        </label>
        <label className="field">
          <span>or paid to address</span>
          <input
            value={form.to}
            onChange={(e) => set("to", e.target.value)}
            placeholder="optional"
            spellCheck={false}
            style={{ width: "22ch", fontFamily: "var(--mono)", fontSize: "0.85rem" }}
          />
        </label>
        <div className="field">
          <span>Direction</span>
          <div className="segmented" role="group" aria-label="Direction">
            <button type="button" aria-pressed={form.direction === "forward"} onClick={() => set("direction", "forward")}>
              Where did it go?
            </button>
            <button type="button" aria-pressed={form.direction === "backward"} onClick={() => set("direction", "backward")}>
              Where did it come from?
            </button>
          </div>
        </div>
        <button type="submit" className="btn primary" disabled={running}>
          {running ? "Tracing…" : "Trace"}
        </button>
        {form.share !== undefined && (
          <p className="continuation" role="note">
            Continuing an earlier trace: following only <Btc sats={form.share} /> of this output, the share that came
            from it{form.mixed ? " (already mixed with other funds, so amounts are pro-rata shares)" : ""}.{" "}
            <button
              type="button"
              className="btn subtle"
              onClick={() => setForm((f) => ({ ...f, share: undefined, mixed: undefined }))}
            >
              Trace the whole output instead
            </button>
          </p>
        )}
        <details>
          <summary>Limits and options</summary>
          <div className="more">
            <label className="field">
              <span>Max hops</span>
              <input className="narrow" value={form.maxHops} onChange={(e) => set("maxHops", e.target.value)} inputMode="numeric" />
            </label>
            <label className="field">
              <span>Max transactions to fetch</span>
              <input className="narrow" value={form.maxTxs} onChange={(e) => set("maxTxs", e.target.value)} inputMode="numeric" />
            </label>
            <label className="field">
              <span>Ignore branches under (BTC)</span>
              <input value={form.minBtc} onChange={(e) => set("minBtc", e.target.value)} placeholder="0.1% of the amount" style={{ width: "16ch" }} />
            </label>
            <label className="check">
              <input type="checkbox" checked={form.checkAddresses} onChange={(e) => set("checkAddresses", e.target.checked)} />
              <span>
                Check address activity
                <br />
                <span className="muted small">Detects busy exchange and service wallets. Slower on first run.</span>
              </span>
            </label>
          </div>
        </details>
      </form>

      <div className="board">
        <div className="canvas-wrap">
          {view && !running && (
            <>
              {display && (
                <GraphCanvas
                  display={display}
                  path={path}
                  focusId={focusId}
                  viewTarget={viewTarget}
                  selected={selection}
                  onSelect={(s) => {
                    setSelection(s);
                    if (s?.type === "node" && !isGroupId(s.id)) {
                      const n = view.graph.nodes.find((x) => x.id === s.id);
                      // Keep the chosen lead if it ends here too (several can share an address).
                      setSelectedLead((cur) =>
                        n?.lead ? (cur !== null && n.lead.others.includes(cur) ? cur : n.lead.index) : null,
                      );
                    }
                  }}
                  controls={controls}
                />
              )}
              <Legend view={view} />
              {display && !noteClosed && (display.hidden > 0 || display.unlinked > 0 || display.groups.size > 0) && (
                <div className="view-note" role="status">
                  <p>
                    Showing {formatInt(display.shown)} of {formatInt(display.total)} addresses
                    {display.total - display.shown - display.hidden - display.unlinked > 0 &&
                      ` (and ${formatInt(display.total - display.shown - display.hidden - display.unlinked)} more inside groups and runs)`}
                    {display.hidden > 0 &&
                      `; ${formatInt(display.hidden)} with smaller flows ${display.hidden === 1 ? "is" : "are"} hidden (slider below)`}
                    {display.unlinked > 0 && `; ${formatInt(display.unlinked)} not linked to the start`}.
                    {display.groups.size > 0 && " Groups and runs of addresses open on click."} Every lead is listed on the right.
                  </p>
                  <button type="button" className="note-close" aria-label="Close this note" onClick={() => setNoteClosed(true)}>
                    ×
                  </button>
                </div>
              )}
              <div className="canvas-tools" role="toolbar" aria-label="Graph tools">
                <label title="Hide flows smaller than this share of the traced amount">
                  Hide flows under {(sliderBp / 100).toFixed(sliderBp % 100 ? 1 : 0)}%
                  <input
                    type="range"
                    min={0}
                    max={1000}
                    step={10}
                    value={sliderBp}
                    aria-valuetext={`${(sliderBp / 100).toFixed(sliderBp % 100 ? 1 : 0)} percent of the traced amount`}
                    onChange={(e) => setSliderBp(Number(e.target.value))}
                  />
                </label>
                <label title="Group many small branches and collapse long runs of plain addresses">
                  <input type="checkbox" checked={simplify} onChange={(e) => setSimplify(e.target.checked)} />
                  Group small branches
                </label>
                <button type="button" className="btn subtle" onClick={() => controls.current?.fit()}>
                  Fit
                </button>
                <button
                  type="button"
                  className="btn subtle"
                  title="Undo any nodes you dragged and lay the graph out again"
                  onClick={() => controls.current?.relayout()}
                >
                  Reset layout
                </button>
              </div>
            </>
          )}
          {!view && !running && (
            <div className="canvas-empty">
              <div>
                <h2>Follow a payment</h2>
                <p>
                  Enter a transaction id above. To follow one payment, add the output number or the address it paid.
                  ChainTrail follows the money until it reaches an exchange, a mixer or another service, or until the
                  limits you set.
                </p>
                <ErrorBox error={error} />
              </div>
            </div>
          )}
          {running && (
            <div className="canvas-empty">
              <div className="progress" role="status" aria-live="polite">
                <div className="track" />
                <strong>Following the funds… {formatElapsed(now - started)}</strong>
                <span className="muted small">
                  {progress
                    ? `${plural(progress.txs_examined, "transaction")} examined, ${plural(progress.pending, "branch", "branches")} queued, ${plural(progress.leads, "lead")} so far`
                    : isDesktop
                      ? "Starting…"
                      : "The first trace of a transaction can take a minute; repeat traces are instant."}
                </span>
              </div>
            </div>
          )}
        </div>

        <aside className="side" aria-label="Results">
          <div className="side-scroll">
            {view && !running ? (
              <>
                {error ? <ErrorBox error={error} /> : null}
                <DetailsPanel
                  view={view}
                  display={display}
                  onExpand={expand}
                  selection={selection}
                  selectedLead={selection ? null : selectedLead}
                  onClose={() => {
                    setSelection(null);
                    setSelectedLead(null);
                    // Keep keyboard users in place: back to the graph.
                    document.querySelector<HTMLElement>(".canvas")?.focus();
                  }}
                />
                <LeadsPanel view={view} selectedLead={selectedLead} onSelectLead={selectLead} />
                <div className="callout">
                  Leads are leads, not proof. Anything inferred or taken from a label is marked as such; check it
                  before acting on it.
                </div>
                <div className="actions">
                  <button type="button" className="btn" onClick={() => exportJson(view)}>
                    Save report (JSON)
                  </button>
                  <button type="button" className="btn" onClick={() => exportCsv(view)}>
                    Save leads (CSV)
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() => {
                      const png = controls.current?.png() ?? "";
                      if (!exportPng(view, png)) window.alert("There is no graph to save at the current setting.");
                    }}
                  >
                    Save graph (PNG)
                  </button>
                </div>
              </>
            ) : (
              <p className="muted small">Results appear here: where the funds went, how much, and how sure we are.</p>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
