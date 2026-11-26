// Details of the selected node, edge or lead, with every claim explained in plain words.

import type { Claim, GraphNode, Lead, TraceView } from "../api/types";
import { formatBtcShort, formatShare, formatUtc } from "../lib/format";
import { type Display, edgeById, isGroupId, nodeById, nodeForLead } from "../lib/graph";
import { categoryName, describeClaim, leadKindExplanation, leadKindName, roleText } from "../lib/meaning";
import { type TraceParams, href } from "../lib/route";
import { Basis, Btc, CategoryTag, Id, Meter } from "./bits";
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

/** Where to trace next from a node. */
function nextTrace(view: TraceView, node: GraphNode, lead: Lead | undefined): TraceParams | null {
  const forward = view.report.direction === "forward";
  if (lead?.txid) {
    return forward
      ? { txid: lead.txid, to: lead.address?.value, direction: "forward", run: true }
      : { txid: lead.txid, direction: "backward", run: true };
  }
  if (forward && node.address) {
    const incoming = view.graph.edges
      .filter((e) => e.target === node.id)
      .sort((a, b) => b.amount - a.amount)[0];
    if (incoming) return { txid: incoming.txid, to: node.address, direction: "forward", run: true };
  }
  return null;
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
        {chain
          ? "The funds passed straight through these unlabelled addresses, one after another. They are drawn as one step to keep the graph readable."
          : "These branches are drawn as one point to keep the graph readable. Each is still listed with the leads."}
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

export function DetailsPanel({
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
    const members = view.graph.edges
      .filter((e) => display.edgeOf.get(e.id) === selection.id)
      .sort((a, b) => b.amount - a.amount);
    return (
      <section className="details" aria-label="Selected flow">
        <div className="details-head">
          <h3>Funds moved (combined)</h3>
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
          {members.length} transfer{members.length === 1 ? "" : "s"} drawn as one line, with their total.
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
    const from = nodeById(view, e.source);
    const to = nodeById(view, e.target);
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
        <p className="small">
          From <span className="mono">{from?.label ?? e.source}</span> to{" "}
          <span className="mono">{to?.label ?? e.target}</span>
        </p>
        <p className="small">{roleText(e.role)}</p>
        {e.note && <p className="small muted">{e.note}</p>}
        <p className="small">
          {formatUtc(e.time)}
          {e.peel && ". Part of a peel chain."}
        </p>
        <p className="small">
          Transaction <Id value={e.txid} short href={href({ page: "tx", txid: e.txid })} />
        </p>
        <p className="muted small">
          The amount is the traced share of this transfer, not necessarily the whole transfer.
        </p>
      </section>
    );
  }

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
  const seen = r.labels_seen.find((l) => l.address.value === node.address);
  const next = nextTrace(view, node, lead);
  const title = node.entity ?? (node.kind === "transaction" ? "Transaction" : node.seed ? "Start of the trace" : "Address");

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
      {lead && (
        <div style={{ display: "grid", gap: 6 }}>
          <p>
            <strong>{leadKindName(lead.kind)}:</strong> <Btc sats={lead.amount} />{" "}
            <span className="muted small">{formatShare(lead.amount, r.seed_amount)}</span>
          </p>
          <p className="small muted">{leadKindExplanation(lead.kind)}</p>
          {lead.category && <p className="small">Category: {categoryName(lead.category)}</p>}
          <ClaimExplained claim={lead.claim} />
          <p className="small muted">
            Reached after {lead.hops} hop{lead.hops === 1 ? "" : "s"}
            {lead.first_time !== null ? `, first on ${formatUtc(lead.first_time)}` : ""}.
          </p>
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
      <dl className="facts">
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
      <div className="actions">
        {node.address && (
          <a className="btn" href={href({ page: "address", address: node.address })}>
            Open address
          </a>
        )}
        {next && (
          <a className="btn" href={href({ page: "trace", params: next })}>
            {view.report.direction === "forward" ? "Trace onward from here" : "Trace further back"}
          </a>
        )}
        {node.address && (
          <a className="btn subtle" href={href({ page: "labels", address: node.address })}>
            Labels
          </a>
        )}
      </div>
    </section>
  );
}
