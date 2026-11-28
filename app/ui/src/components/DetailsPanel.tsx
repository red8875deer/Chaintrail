// Details of the selected node, edge or lead, with every claim explained in plain words.

import { useEffect, useRef } from "react";
import type { Claim, GraphNode, Lead, TraceView } from "../api/types";
import { formatBtcShort, formatShare, formatUtc, shortId } from "../lib/format";
import { type Display, edgeById, isGroupId, leadPath, nodeById, nodeForLead } from "../lib/graph";
import { categoryName, describeClaim, leadKindExplanation, leadKindName, roleText } from "../lib/meaning";
import { type TraceParams, href } from "../lib/route";
import { Basis, Btc, CategoryTag, Id, Meter } from "./bits";
import { FreezeBlock } from "./FreezeBlock";
import { RequestBlock, useOnChainAmount } from "./RequestBlock";
import type { Selection } from "./GraphCanvas";

function ClaimExplained({ claim }: { claim: Claim }) {
  const t = describeClaim(claim);
  return (
    <div>
      <p className="small">
        <Basis claim={claim} /> {t.headline}
      </p>
      {t.evidence.length > 0 && (
        <ul className="evidence">
          {t.evidence.map((e, i) => (
            <li key={i}>{e}</li>
          ))}
        </ul>
      )}
      {t.url && (
        <p className="small" style={{ marginTop: 4 }}>
          Source:{" "}
          {/^https?:\/\//.test(t.url) ? (
            <a href={t.url} target="_blank" rel="noreferrer noopener">
              {t.url}
            </a>
          ) : (
            t.url
          )}
          {t.asOf && <span className="muted"> (as of {t.asOf})</span>}
        </p>
      )}
    </div>
  );
}

/** Where to continue the trace from a node. The new trace follows only the share of the
 * funds this trace followed there (not the whole output) and keeps any earlier mixing, so
 * its amounts stay comparable with this one. */
function nextTrace(
  view: TraceView,
  node: GraphNode,
  lead: Lead | undefined,
): { params: TraceParams; label: string; caution?: string } | null {
  const r = view.report;
  const forward = r.direction === "forward";
  const again = forward ? "Trace onward from here" : "Trace further back";
  if (lead) {
    const k = lead.kind.kind;
    // At rest: nothing moved. Mined: the origin. Mixer: following through is unreliable.
    if (k === "at_rest" || k === "mined" || lead.category === "mixer") return null;
    if (k === "unexplored" && lead.txid && lead.vout !== null && lead.vout !== undefined) {
      return {
        label: again,
        params: {
          txid: lead.txid,
          vout: lead.vout,
          direction: r.direction,
          share: lead.amount,
          mixed: lead.mixed_in ?? undefined,
          from: r.seed,
          run: true,
        },
      };
    }
    if (k === "endpoint") {
      // Backward, an endpoint is where the funds came from: the end of the trail.
      if (!forward || !node.address) return null;
      // One deposit transaction: the traced funds it paid to the address (all routes in
      // that transaction), with that deposit's own mixing, not the lead's total.
      const deposits = new Map<string, number>();
      for (const f of r.flows)
        if (f.to?.value === node.address) deposits.set(f.txid, (deposits.get(f.txid) ?? 0) + f.amount);
      const [txid, share] = [...deposits].sort((a, b) => b[1] - a[1])[0] ?? [];
      if (!txid || !share) return null;
      const flows = r.flows.filter((f) => f.txid === txid && f.to?.value === node.address);
      const others = deposits.size - 1;
      return {
        label: "Trace onward through this service",
        caution:
          "A service pools many customers' funds: after a deposit, which payments carry these funds is an allocation, not a link. Records from the service are more reliable." +
          (others > 0
            ? ` This follows the largest deposit (transaction ${shortId(txid)}); the ${others} other${others === 1 ? "" : "s"} would be separate traces.`
            : ""),
        params: {
          txid,
          to: node.address,
          direction: "forward",
          share,
          // That deposit's own mixing; the service's own spends then mix (the engine marks it).
          mixed: flows.find((f) => f.mixed_in)?.mixed_in ?? undefined,
          from: r.seed,
          run: true,
        },
      };
    }
  }
  if (!node.address) return null;
  // An address on the way: continue from the largest transfer into it (forward) or out of
  // it (backward), with that transfer's traced amount.
  const edges = view.graph.edges
    .filter((e) => e.source !== e.target && (forward ? e.target === node.id : e.source === node.id))
    .sort((a, b) => b.amount - a.amount);
  const edge = edges[0];
  if (!edge) return null;
  const others = new Set(edges.map((e) => e.txid)).size - 1;
  const caution =
    others > 0
      ? `Continues from the largest transfer ${forward ? "into" : "out of"} this address (transaction ${shortId(edge.txid)}) only; the ${others} other${others === 1 ? "" : "s"} would be separate traces.`
      : undefined;
  const flows = r.flows.filter(
    (f) => f.txid === edge.txid && (forward ? f.to?.value === node.address : f.from?.value === node.address),
  );
  if (forward) {
    // Everything this transaction's traced funds paid to the address (all its outputs
    // there), so an output traced in full is continued in full.
    return {
      label: again,
      caution,
      params: {
        txid: edge.txid,
        to: node.address,
        direction: "forward",
        share: flows.reduce((sum, f) => sum + f.amount, 0) || edge.amount,
        mixed: flows.find((f) => f.mixed_in)?.mixed_in ?? undefined,
        from: r.seed,
        run: true,
      },
    };
  }
  // Backward: one coin the address spent in that transaction, with that coin's own share
  // (the address may have spent several coins there).
  const coins = new Map<string, number>();
  for (const f of flows) if (f.from_coin) coins.set(f.from_coin, (coins.get(f.from_coin) ?? 0) + f.amount);
  const [coinRef, coinShare] = [...coins].sort((a, b) => b[1] - a[1])[0] ?? [];
  const coin = coinRef?.split(":");
  if (!coin || coin.length !== 2 || !coinShare) return null;
  const moreCoins = coins.size - 1;
  return {
    label: again,
    caution:
      [caution, moreCoins > 0 ? `Follows the largest of the ${coins.size} coins this address spent there.` : null]
        .filter(Boolean)
        .join(" ") || undefined,
    params: {
      txid: coin[0]!,
      vout: Number(coin[1]),
      direction: "backward",
      share: coinShare,
      mixed: flows.find((f) => f.from_coin === coinRef && f.mixed_in)?.mixed_in ?? undefined,
      from: r.seed,
      run: true,
    },
  };
}

function GroupDetails({
  view,
  display,
  id,
  onExpand,
  onClose,
}: {
  view: TraceView;
  display: Display;
  id: string;
  onExpand: (id: string) => void;
  onClose: () => void;
}) {
  const g = display.groups.get(id);
  if (!g) return null;
  const r = view.report;
  const chain = g.kind === "chain";
  const members = g.members.map((m) => nodeById(view, m)).filter((n): n is GraphNode => !!n);
  return (
    <section className="details" aria-label="Selected group">
      <div className="details-head">
        <h3>{chain ? `${members.length} addresses in a row` : `${members.length} smaller branches`}</h3>
        <button type="button" className="btn subtle" onClick={onClose}>
          Close
        </button>
      </div>
      <p>
        <Btc sats={g.amount} /> <span className="muted small">{formatShare(g.amount, r.seed_amount)} of the traced amount</span>
      </p>
      <p className="small muted">
        {chain ? (
          <>
            The funds passed through these unlabelled addresses one after another; they are drawn as one step to keep the
            graph readable. {formatBtcShort(g.amount)} BTC entered the first and {formatBtcShort(g.amountOut)} BTC
            continued from the last.
            {g.amountOut < g.amount &&
              ` The other ${formatBtcShort(g.amount - g.amountOut)} BTC left in smaller branches (hidden at the current setting) or went to transaction fees.`}
          </>
        ) : (
          "These branches are drawn as one point to keep the graph readable. Each is still listed with the leads."
        )}
      </p>
      <ul className="evidence" style={{ maxHeight: 220, overflow: "auto" }}>
        {members.slice(0, 50).map((n) => (
          <li key={n.id}>
            <span className="mono">{n.label}</span> {formatBtcShort(n.lead?.amount ?? Math.max(n.amount_in, n.amount_out))} BTC
            {n.lead ? ` (${n.lead.title})` : ""}
          </li>
        ))}
        {members.length > 50 && <li>and {members.length - 50} more</li>}
      </ul>
      <div className="actions">
        <button type="button" className="btn primary" onClick={() => onExpand(id)}>
          {chain ? "Show each address" : `Show all ${members.length}`}
        </button>
      </div>
    </section>
  );
}

/** Keeps the details in sight when something on the graph is clicked. */
export function DetailsPanel(props: Parameters<typeof DetailsBody>[0]) {
  const ref = useRef<HTMLDivElement>(null);
  const { selection } = props;
  useEffect(() => {
    if (!selection) return;
    ref.current?.scrollIntoView?.({ block: "start", behavior: "auto" });
  }, [selection]);
  return (
    <div ref={ref} className="details-anchor">
      <DetailsBody {...props} />
    </div>
  );
}

function DetailsBody({
  view,
  display,
  onExpand,
  selection,
  selectedLead,
  onClose,
}: {
  view: TraceView;
  display: Display | null;
  onExpand: (id: string) => void;
  selection: Selection;
  selectedLead: number | null;
  onClose: () => void;
}) {
  const r = view.report;

  if (selection?.type === "node" && isGroupId(selection.id) && display) {
    return <GroupDetails view={view} display={display} id={selection.id} onExpand={onExpand} onClose={onClose} />;
  }

  if (selection?.type === "edge" && selection.id.startsWith("agg|") && display) {
    const el = display.elements.find((e) => e.data.id === selection.id);
    if (!el) return null;
    const label = (id: unknown) => {
      const s = String(id);
      const g = display.groups.get(s);
      if (g) return g.kind === "chain" ? `${g.members.length} addresses in a row` : `${g.members.length} smaller branches`;
      return nodeById(view, s)?.label ?? s;
    };
    const ids = new Set(display.edgeMembers.get(selection.id) ?? []);
    const members = view.graph.edges.filter((e) => ids.has(e.id)).sort((a, b) => (a.time ?? 0) - (b.time ?? 0));
    const loop = el.data.source === el.data.target;
    return (
      <section className="details" aria-label="Selected flow">
        <div className="details-head">
          <h3>{loop ? "Funds moved to the same address" : "Funds moved (combined)"}</h3>
          <button type="button" className="btn subtle" onClick={onClose}>
            Close
          </button>
        </div>
        <p>
          <Btc sats={Number(el.data.amount)} />{" "}
          <span className="muted small">{formatShare(Number(el.data.amount), r.seed_amount)} of the traced amount</span>
        </p>
        <p className="small">
          From <span className="mono">{label(el.data.source)}</span> to <span className="mono">{label(el.data.target)}</span>
        </p>
        <p className="small muted">
          {loop
            ? `The address paid itself ${members.length} time${members.length === 1 ? "" : "s"}. Each time the same coins moved again, so the amount is the largest single move, not a total.`
            : members.length === 1
              ? "One transfer."
              : `${members.length} transfers drawn as one line; the amount is their total.`}
        </p>
        <ul className="evidence" style={{ maxHeight: 220, overflow: "auto" }}>
          {members.slice(0, 30).map((e) => (
            <li key={e.id}>
              {formatBtcShort(e.amount)} BTC, {formatUtc(e.time)}{" "}
              <Id value={e.txid} short href={href({ page: "tx", txid: e.txid })} />
            </li>
          ))}
          {members.length > 30 && <li>and {members.length - 30} more</li>}
        </ul>
      </section>
    );
  }

  if (selection?.type === "edge") {
    const e = edgeById(view, selection.id);
    if (!e) return null;
    return <EdgeDetails view={view} edgeId={e.id} onClose={onClose} />;
  }

  return <NodeDetails view={view} selection={selection} selectedLead={selectedLead} onClose={onClose} />;
}

function EdgeDetails({ view, edgeId, onClose }: { view: TraceView; edgeId: string; onClose: () => void }) {
  const r = view.report;
  const e = edgeById(view, edgeId)!;
  const from = nodeById(view, e.source);
  const to = nodeById(view, e.target);
  // The whole on-chain payment this traced share is part of.
  const whole = useOnChainAmount(e.txid, to?.address ?? null, "to");
  // Traced funds can reach the payment from several input addresses (one line each).
  const siblings = view.graph.edges.filter((x) => x.txid === e.txid && x.target === e.target && x.source !== x.target);
  const tracedInPayment = siblings.reduce((s, x) => s + x.amount, 0);
  const mixed = r.flows.some((f) => f.txid === e.txid && f.to?.value === to?.address && f.mixed_in);
  return (
      <section className="details" aria-label="Selected flow">
        <div className="details-head">
          <h3>Funds moved</h3>
          <button type="button" className="btn subtle" onClick={onClose}>
            Close
          </button>
        </div>
        <p>
          <Btc sats={e.amount} /> <span className="muted small">{formatShare(e.amount, r.seed_amount)} of the traced amount</span>
        </p>
        {whole.amount !== null && whole.amount > 0 && (
          <p className="small">
            The transaction paid <Btc sats={whole.amount} /> to this address.{" "}
            {siblings.length > 1 && (
              <>
                Traced funds reached it from {siblings.length} input addresses, <Btc sats={tracedInPayment} /> in all; this
                line is the part from {from?.label ?? e.source}.{" "}
              </>
            )}
            {tracedInPayment >= whole.amount ? (
              "All of it is traced funds."
            ) : (
              <>
                The other <Btc sats={whole.amount - tracedInPayment} />{" "}
                {mixed
                  ? "came from other funds mixed in this transaction."
                  : "is not part of the funds traced here."}
              </>
            )}
          </p>
        )}
        <p className="small">
          From <span className="mono">{from?.label ?? e.source}</span> to{" "}
          <span className="mono">{to?.label ?? e.target}</span>
        </p>
        <p className="small">{roleText(e.role)}</p>
        {e.note && <p className="small muted">{e.note}</p>}
        <p className="small">
          {formatUtc(e.time)}
          {e.peel &&
            ". Part of a peel chain: a large amount repeatedly splits off a smaller payment and moves the rest on to a new address."}
        </p>
        <p className="small">
          Transaction <Id value={e.txid} short href={href({ page: "tx", txid: e.txid })} />
        </p>
      </section>
    );
}

const DAY = 86_400;
/** Gaps at least this long on a route are pointed out (funds sat still). */
const LONG_GAP_DAYS = 30;

function days(seconds: number): string {
  const d = Math.round(seconds / DAY);
  return d < 1 ? "less than a day" : d === 1 ? "1 day" : d < 60 ? `${d} days` : `${Math.round(d / 30.4)} months`;
}

/** When traced funds reached this address and when they left it. */
function NodeTimes({ view, node }: { view: TraceView; node: GraphNode }) {
  if (!node.address) return null;
  const a = node.address;
  const ins = view.report.flows.filter((f) => f.to?.value === a && f.from?.value !== a && f.time !== null).map((f) => f.time!);
  const outs = view.report.flows.filter((f) => f.from?.value === a && f.to?.value !== a && f.time !== null).map((f) => f.time!);
  if (ins.length === 0 && outs.length === 0) return null;
  const first = ins.length ? Math.min(...ins) : null;
  const last = outs.length ? Math.max(...outs) : null;
  const left = outs.length ? Math.min(...outs) : null;
  return (
    <p className="small">
      {first !== null && <>Traced funds first arrived {formatUtc(first)}. </>}
      {left !== null && (
        <>
          They left {formatUtc(left)}
          {last !== null && last !== left ? ` (last ${formatUtc(last)})` : ""}
          {first !== null && left >= first ? `, ${days(left - first)} later` : ""}.
        </>
      )}
    </p>
  );
}

/** The route to a lead as dated steps, with long pauses pointed out. */
function RouteTimeline({ view, lead }: { view: TraceView; lead: Lead }) {
  const r = view.report;
  const timeOf = (txid: string) => {
    const t = r.flows.filter((f) => f.txid === txid && f.time !== null).map((f) => f.time!);
    if (t.length) return Math.min(...t);
    return r.seed_outputs.find((o) => o.txid === txid)?.time ?? null;
  };
  const steps = lead.path.map((txid) => ({ txid, time: timeOf(txid) }));
  return (
    <details className="route-timeline" open={steps.length <= 6}>
      <summary className="small">
        Route in time ({steps.length} transactions{steps[0]?.time && steps.at(-1)?.time ? `, ${days(Math.abs(steps.at(-1)!.time! - steps[0]!.time!))}` : ""})
      </summary>
      <ol className="evidence">
        {steps.map((st, i) => {
          const prev = i > 0 ? steps[i - 1]!.time : null;
          const gap = st.time !== null && prev !== null ? Math.abs(st.time - prev) : null;
          return (
            <li key={`${st.txid}-${i}`}>
              {formatUtc(st.time, "unconfirmed")} <Id value={st.txid} short href={href({ page: "tx", txid: st.txid })} />
              {gap !== null && gap >= LONG_GAP_DAYS * DAY && (
                <strong className="small"> · funds sat {days(gap)} before this</strong>
              )}
            </li>
          );
        })}
      </ol>
    </details>
  );
}

function NodeDetails({
  view,
  selection,
  selectedLead,
  onClose,
}: {
  view: TraceView;
  selection: Selection;
  selectedLead: number | null;
  onClose: () => void;
}) {
  const r = view.report;
  const node =
    selection?.type === "node"
      ? nodeById(view, selection.id)
      : selectedLead !== null
        ? nodeForLead(view, selectedLead)
        : undefined;
  if (!node) return null;
  // Several leads can end at one address: show the chosen one, and list the others.
  const here = node.lead ? [node.lead.index, ...node.lead.others] : [];
  const leadIndex = selectedLead !== null && here.includes(selectedLead) ? selectedLead : here[0];
  const lead = leadIndex !== undefined ? r.leads[leadIndex] : undefined;
  const otherLeads = here.filter((i) => i !== leadIndex);
  // The highlighted route is one route; say so when it carries only part of the amount.
  const routeAmount =
    lead && leadIndex !== undefined
      ? (() => {
          const p = leadPath(view, leadIndex);
          const forward = r.direction === "forward";
          return view.graph.edges
            .filter((e) => p.edges.has(e.id) && e.source !== e.target && (forward ? e.target : e.source) === node.id)
            .reduce((s, e) => s + e.amount, 0);
        })()
      : 0;
  const partRoute = lead !== undefined && lead.hops > 0 && routeAmount > 0 && routeAmount * 100 < lead.amount * 99;
  const seen = r.labels_seen.find((l) => l.address.value === node.address);
  const next = nextTrace(view, node, lead);
  const title =
    node.entity ??
    (node.seed ? "Start of the trace" : lead ? lead.title : node.kind === "transaction" ? "Transaction" : "Address");

  return (
    <section className="details" aria-label="Selected item">
      <div className="details-head">
        <h3>{title}</h3>
        <button type="button" className="btn subtle" onClick={onClose}>
          Close
        </button>
      </div>
      {node.address && <Id value={node.address} />}
      {node.txid && <Id value={node.txid} short href={href({ page: "tx", txid: node.txid })} />}
      {(node.category || node.confidence) && (
        <div className="lead-meta">
          {node.category && <CategoryTag category={node.category} />}
          {node.confidence && <Meter confidence={node.confidence} />}
        </div>
      )}
      {/* Actions first: the card scrolls, and these are what an investigator does next. */}
      <div className="actions">
        {next && (
          <a className="btn" href={href({ page: "trace", params: next.params })} title={next.caution}>
            {next.label}
          </a>
        )}
        {node.address && (
          <a className="btn" href={href({ page: "address", address: node.address })}>
            Open address
          </a>
        )}
        {node.address && (
          <a className="btn subtle" href={href({ page: "labels", address: node.address })}>
            Labels
          </a>
        )}
      </div>
      {next?.caution && <p className="small muted">{next.caution}</p>}
      {/* What the investigator sends out comes next, before the explanations. */}
      {lead && lead.kind.kind === "endpoint" && lead.category !== "mixer" && (
        <RequestBlock view={view} node={node} lead={lead} />
      )}
      {here.some((i) => r.leads[i]?.kind.kind === "at_rest") && (
        <FreezeBlock view={view} node={node} leads={here.map((i) => r.leads[i]!).filter(Boolean)} />
      )}
      {lead && (
        <div style={{ display: "grid", gap: 6 }}>
          <p>
            <strong>{leadKindName(lead.kind)}:</strong> <Btc sats={lead.amount} />{" "}
            <span className="muted small">{formatShare(lead.amount, r.seed_amount)}</span>
          </p>
          <p className="small muted">{leadKindExplanation(lead.kind, lead.category, r.direction)}</p>
          {lead.category && <p className="small">Category: {categoryName(lead.category)}</p>}
          <ClaimExplained claim={lead.claim} />
          <p className="small muted">
            {lead.hops === 0 ? "At the start of the trace" : `Reached after ${lead.hops} hop${lead.hops === 1 ? "" : "s"}`}
            {lead.first_time !== null ? `, first on ${formatUtc(lead.first_time)}` : ""}.
          </p>
          {partRoute && (
            <p className="small muted">
              The highlighted route carries {formatBtcShort(routeAmount)} of these {formatBtcShort(lead.amount)} BTC; the rest
              moved by other routes (smaller ones may be hidden on the graph).
            </p>
          )}
          {otherLeads.length > 0 && (
            <div className="small">
              Funds also reached this address by {otherLeads.length === 1 ? "another route" : `${otherLeads.length} other routes`}:
              <ul className="evidence">
                {otherLeads.map((i) => (
                  <li key={i}>
                    {r.leads[i]!.title}: <Btc sats={r.leads[i]!.amount} />
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
      {!lead && seen && <ClaimExplained claim={seen.claim} />}
      <NodeTimes view={view} node={node} />
      {lead && lead.path.length > 1 && <RouteTimeline view={view} lead={lead} />}
      <dl className="facts" hidden={node.amount_in === 0 && node.amount_out === 0}>
        <div className="fact">
          <dt>Traced funds in</dt>
          <dd>
            <Btc sats={node.amount_in} />
          </dd>
        </div>
        <div className="fact">
          <dt>Traced funds out</dt>
          <dd>
            <Btc sats={node.amount_out} />
          </dd>
        </div>
      </dl>
    </section>
  );
}
