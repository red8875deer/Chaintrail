// Details of the selected node, edge or lead, with every claim explained in plain words.

import type { Claim, GraphNode, Lead, TraceView } from "../api/types";
import { formatShare, formatUtc } from "../lib/format";
import { edgeById, nodeById } from "../lib/graph";
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

export function DetailsPanel({
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
        ? view.graph.nodes.find((n) => n.lead?.index === selectedLead)
        : undefined;
  if (!node) return null;
  const lead = node.lead ? r.leads[node.lead.index] : undefined;
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
