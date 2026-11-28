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
import { extractId, formatBtc, formatElapsed, formatInt, looksLikeAddress, parseBtc, shortSeed } from "../lib/format";
import {
  buildDisplay,
  defaultMinShareBp,
  displayPath,
  findNodes,
  isGroupId,
  leadPath,
  leadSpine,
  nodeForLead,
  routeToNode,
} from "../lib/graph";
import { DEFAULT_MAX_HOPS, DEFAULT_MAX_TXS, type TraceParams, href, parseRoute, sameTrace } from "../lib/route";

/** Traces opened in this window, newest last: Back and Forward reopen them at once. */
const recent = new Map<string, View>();
const RECENT_MAX = 6;

/** What the investigator had on screen for a trace, kept in its history entry. */
interface ViewState {
  lead: number | null;
  hideBp: number;
  simplify: boolean;
  expanded: string[];
}

function savedViewState(): ViewState | null {
  return (history.state as { ui?: ViewState } | null)?.ui ?? null;
}

/** Graphs with more addresses than this show "Arranging…" before a redraw. */
const BIG_VIEW = 300;

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
  /** The earlier trace's seed, for the record. */
  from?: string;
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
    from: p?.from,
  };
}

function toRequest(f: FormState): TraceRequest | string {
  // Explorer links are accepted: the transaction id is taken from them.
  const txid = extractId(f.txid);
  if (!/^[0-9a-fA-F]{64}$/.test(txid))
    return looksLikeAddress(txid)
      ? "That is an address, not a transaction id. Look it up with the search at the top to see its transactions, then trace one of them (or put the address in “or paid to address”)."
      : "Enter a transaction id: 64 characters, 0–9 and a–f.";
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
    continued_from: f.from ?? null,
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
  // Redrawing a big graph blocks the page for a moment: say so first, then do it.
  const [busy, setBusy] = useState(false);
  const bigRef = useRef(false);
  bigRef.current = (view?.graph.nodes.length ?? 0) > BIG_VIEW;
  const heavy = useCallback((apply: () => void) => {
    if (!bigRef.current) return apply();
    setBusy(true);
    requestAnimationFrame(() =>
      setTimeout(() => {
        apply();
        setBusy(false);
      }, 0),
    );
  }, []);
  useEffect(() => setSliderBp(hideBp), [hideBp]);
  useEffect(() => {
    if (sliderBp === hideBp) return;
    const t = setTimeout(() => heavy(() => setHideBp(sliderBp)), 250);
    return () => clearTimeout(t);
  }, [sliderBp, hideBp, heavy]);
  const [simplify, setSimplify] = useState(true);
  /** The checkbox follows the click at once; the graph follows `simplify`. */
  const [simplifyBox, setSimplifyBox] = useState(true);
  useEffect(() => setSimplifyBox(simplify), [simplify]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [noteClosed, setNoteClosed] = useState(false);
  /** Nodes to bring into view after the graph changes (e.g. an opened group). */
  const [viewTarget, setViewTarget] = useState<{ ids: string[] } | null>(null);
  /** Nodes found with "Find in graph", drawn with their route from the start. */
  const [found, setFound] = useState<{ nodes: ReadonlySet<string>; edges: ReadonlySet<string> }>({
    nodes: new Set(),
    edges: new Set(),
  });
  const [findText, setFindText] = useState("");
  /** Said to screen readers when a trace finishes or fails. */
  const [announce, setAnnounce] = useState("");
  const [findNote, setFindNote] = useState("");
  const findIndex = useRef<{ q: string; i: number }>({ q: "", i: 0 });
  const controls = useRef<GraphControls | null>(null);
  const runId = useRef(0);

  /** `fresh`: asked for with the Trace button, so never answered from this window's memory. */
  const run = useCallback(async (f: FormState, fresh = false) => {
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
      from: f.from,
      run: true,
    };
    const current = parseRoute(window.location.hash);
    const url = href({ page: "trace", params: target });
    // Reopening the same trace (Back, Forward, reload) keeps what was on screen.
    let restore: ViewState | null = null;
    if (current.page === "trace" && current.params && sameTrace(current.params, target)) {
      restore = savedViewState();
      history.replaceState(history.state, "", url);
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
      const key = JSON.stringify(req);
      const v = (!fresh && recent.get(key)) || (await call("btc_trace", req));
      if (id !== runId.current) return;
      recent.delete(key);
      recent.set(key, v);
      if (recent.size > RECENT_MAX) recent.delete(recent.keys().next().value!);
      setView(v);
      setNoteClosed(false);
      setViewTarget(null);
      setFound({ nodes: new Set(), edges: new Set() });
      setFindNote("");
      const ends = v.report.leads.filter((l) => l.kind.kind === "endpoint").length;
      setAnnounce(
        `Trace finished: ${v.report.leads.length} leads, ${ends} endpoint${ends === 1 ? "" : "s"}. Results are in the panel after the graph.`,
      );
      if (restore) {
        setHideBp(restore.hideBp);
        setSimplify(restore.simplify);
        setExpanded(new Set(restore.expanded));
        setSelectedLead(restore.lead !== null && restore.lead < v.report.leads.length ? restore.lead : null);
      } else {
        setHideBp(defaultMinShareBp(v));
        setSimplify(true);
        setExpanded(new Set());
        // Show the strongest result at once: the largest endpoint, else the largest branch.
        const first = v.report.leads.findIndex((l) => l.kind.kind !== "unexplored");
        setSelectedLead(first >= 0 ? first : v.report.leads.length > 0 ? 0 : null);
      }
    } catch (e) {
      if (id === runId.current) {
        setError(e);
        setAnnounce("The trace failed; the reason is shown under the form.");
      }
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

  // Keep what is on screen in this trace's history entry, for Back and Forward.
  useEffect(() => {
    if (!view || running) return;
    const t = setTimeout(() => {
      const ui: ViewState = { lead: selectedLead, hideBp, simplify, expanded: [...expanded] };
      history.replaceState({ ...((history.state as object | null) ?? {}), ui }, "", window.location.href);
    }, 300);
    return () => clearTimeout(t);
  }, [view, running, selectedLead, hideBp, simplify, expanded]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void run(form, true);
  };
  // Changing what is traced ends a continuation: its share belongs to that seed only.
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) =>
    setForm((f) => {
      const seedChanged = k === "txid" || k === "vout" || k === "to" || k === "direction";
      return { ...f, [k]: v, ...(seedChanged ? { share: undefined, mixed: undefined, from: undefined } : {}) };
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
            keep: found.nodes.size ? new Set([...revealed.nodes, ...found.nodes]) : revealed.nodes,
            keepEdges: found.edges.size ? new Set([...revealed.edges, ...found.edges]) : revealed.edges,
          })
        : null,
    [view, hideBp, simplify, expanded, revealed, found],
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

  // Find an address or transaction in this trace: draw it with its route and select it.
  // Enter again moves to the next match.
  const find = () => {
    if (!view) return;
    const q = findText.trim();
    const matches = findNodes(view, q);
    if (matches.length === 0) {
      setFindNote(q.length < 4 ? "Type at least 4 characters." : "Not in this trace.");
      return;
    }
    const i = findIndex.current.q === q ? (findIndex.current.i + 1) % matches.length : 0;
    findIndex.current = { q, i };
    const n = matches[i]!;
    const route = routeToNode(view, n.id);
    const next = { nodes: new Set([...found.nodes, ...route.nodes]), edges: new Set([...found.edges, ...route.edges]) };
    // Open any group or run of addresses that still stands in for it.
    const open = new Set(expanded);
    for (let k = 0; k < 4; k++) {
      const d = buildDisplay(view, {
        minShareBp: hideBp,
        simplify,
        expanded: open,
        keep: new Set([...revealed.nodes, ...next.nodes]),
        keepEdges: new Set([...revealed.edges, ...next.edges]),
      });
      const at = d.nodeOf.get(n.id);
      if (!at || !isGroupId(at)) break;
      open.add(at);
    }
    if (open.size !== expanded.size) setExpanded(open);
    setFound(next);
    setSelection({ type: "node", id: n.id });
    setViewTarget({ ids: [n.id] });
    setFindNote(matches.length > 1 ? `${i + 1} of ${matches.length}: Enter for the next` : "");
  };

  // Back to where the trace starts.
  const goToStart = () => {
    if (!view || !display) return;
    const seed = view.graph.nodes.find((n) => n.seed);
    const id = seed ? (display.nodeOf.get(seed.id) ?? seed.id) : null;
    if (!id) return;
    setSelection({ type: "node", id });
    controls.current?.focus(id);
  };

  // Many small branches were not followed: trace again with a tenth of the minimum.
  const lowerMinimum = () => {
    if (!view) return;
    const min = Math.max(1, Math.floor(view.report.min_amount / 10));
    const current = parseRoute(window.location.hash);
    void run({ ...fromParams(current.page === "trace" ? current.params : null), minBtc: formatBtc(min, 8) });
  };

  // Selecting never toggles off (arrow keys and clicks stay predictable); selecting the
  // chosen lead again brings it back into view. Close or Esc clears it.
  const selectLead = (i: number) => {
    if (i === selectedLead && view && display) {
      const n = nodeForLead(view, i);
      const id = n ? display.nodeOf.get(n.id) : undefined;
      if (id) setViewTarget({ ids: [id] });
    }
    setSelectedLead(i);
    setSelection(null);
  };

  return (
    <div className="workspace">
      <button
        type="button"
        className="skip-link"
        onClick={() => document.querySelector<HTMLElement>(".side-scroll")?.focus()}
      >
        Skip to results
      </button>
      <p className="sr-only" role="status" aria-live="polite">
        {announce}
      </p>
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
            Continuing {form.from ? <>the trace from <span className="mono">{shortSeed(form.from)}</span></> : "an earlier trace"}:
            following only <Btc sats={form.share} /> of this output, the share that came from it
            {form.mixed ? " (already mixed with other funds, so amounts are pro-rata shares)" : ""}.{" "}
            <button
              type="button"
              className="btn subtle"
              disabled={running}
              onClick={() => void run({ ...form, share: undefined, mixed: undefined })}
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
              {display && !noteClosed && (display.hidden > 0 || display.unlinked > 0 || display.groups.size > 0) && (
                <div className="view-note" role="status">
                  <p>
                    Showing {formatInt(display.shown)} of {formatInt(display.total)} addresses
                    {display.total - display.shown - display.hidden - display.unlinked > 0 &&
                      ` (and ${formatInt(display.total - display.shown - display.hidden - display.unlinked)} more inside groups and runs)`}
                    {display.hidden > 0 &&
                      `; ${formatInt(display.hidden)} with smaller flows ${display.hidden === 1 ? "is" : "are"} hidden (slider below)`}
                    {display.unlinked > 0 && `; ${formatInt(display.unlinked)} not linked to the start`}.
                    {display.groups.size > 0 && " Groups and runs of addresses open on click."} Every lead is listed with the results.
                  </p>
                  <button type="button" className="note-close" aria-label="Close this note" onClick={() => setNoteClosed(true)}>
                    ×
                  </button>
                </div>
              )}
              <div className="canvas-stage">
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
                  busy={busy}
                  onOpen={expand}
                />
              )}
              <Legend view={view} />
              </div>
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
                  <input
                    type="checkbox"
                    checked={simplifyBox}
                    onChange={(e) => {
                      const on = e.target.checked;
                      setSimplifyBox(on);
                      heavy(() => setSimplify(on));
                    }}
                  />
                  Group small branches
                </label>
                <form
                  className="find"
                  role="search"
                  onSubmit={(e) => {
                    e.preventDefault();
                    find();
                  }}
                >
                  <input
                    aria-label="Find an address or transaction in this graph"
                    placeholder="Find address or tx"
                    value={findText}
                    onChange={(e) => {
                      setFindText(e.target.value);
                      setFindNote("");
                    }}
                    spellCheck={false}
                  />
                  {findNote && (
                    <span className="small muted" role="status">
                      {findNote}
                    </span>
                  )}
                </form>
                <button type="button" className="btn subtle" onClick={goToStart} title="Show where the trace starts">
                  Start
                </button>
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
          {view && !running && display && display.elements.length === 0 && (
            <div className="canvas-empty" role="status">
              <p>
                Nothing to draw at this setting: every flow is smaller than the “Hide flows under” share. Move the slider
                left to show them.
              </p>
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
          <div className="side-scroll" tabIndex={-1}>
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
                <LeadsPanel
                  view={view}
                  selectedLead={selectedLead}
                  onSelectLead={selectLead}
                  onGoToStart={goToStart}
                  onClearLead={() => {
                    setSelectedLead(null);
                    setSelection(null);
                  }}
                  onLowerMinimum={running || view.report.min_amount <= 1 ? undefined : lowerMinimum}
                />
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
                      const note = display
                        ? `Drawn: ${display.shown} of ${display.total} addresses${display.hidden > 0 ? `; ${display.hidden} with flows under ${(hideBp / 100).toFixed(hideBp % 100 ? 1 : 0)}% of the traced amount hidden` : ""}${display.groups.size > 0 ? "; smaller branches grouped and runs of addresses collapsed" : ""}. Amounts on lines are BTC.${selectedLead !== null && view.report.leads[selectedLead] ? ` Highlighted: the route to "${view.report.leads[selectedLead]!.title}".` : ""} Every lead is in the CSV and JSON exports.`
                        : "";
                      if (!exportPng(view, png, note)) window.alert("There is no graph to save at the current setting.");
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
