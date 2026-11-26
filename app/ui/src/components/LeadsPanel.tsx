// Leads: where the traced funds ended up (or came from), largest first.

import { useEffect, useState } from "react";
import type { Lead, TraceView } from "../api/types";
import { formatShare, formatUtcDate, shareBasisPoints, shortId } from "../lib/format";
import { categoryName, leadKindName } from "../lib/meaning";
import { Basis, Btc, Meter, categoryColor } from "./bits";

function LeadRow({
  lead,
  index,
  total,
  selected,
  onSelect,
}: {
  lead: Lead;
  index: number;
  total: number;
  selected: boolean;
  onSelect: (i: number) => void;
}) {
  const pct = Number(shareBasisPoints(lead.amount, total)) / 100;
  const colour =
    lead.kind.kind === "at_rest"
      ? "var(--n-at_rest)"
      : lead.kind.kind === "unexplored"
        ? "var(--n-unexplored)"
        : categoryColor(lead.category);
  return (
    <li
      className="lead"
      role="option"
      aria-selected={selected}
      tabIndex={0}
      onClick={() => onSelect(index)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(index);
        }
      }}
    >
      <div className="lead-top">
        <span className="lead-title">
          <span className="swatch" style={{ background: colour }} />
          {lead.title}
        </span>
      </div>
      {(lead.address || lead.txid) && (
        <div className="lead-where" title={lead.address?.value ?? lead.txid ?? ""}>
          {lead.address ? lead.address.value : `transaction ${shortId(lead.txid ?? "")}`}
        </div>
      )}
      <div className="lead-top">
        <Btc sats={lead.amount} />
        <span className="muted small">{formatShare(lead.amount, total)}</span>
      </div>
      <div className="share" aria-hidden="true">
        <b style={{ width: `${Math.max(pct, 0.5)}%` }} />
      </div>
      <div className="lead-meta">
        <span>{leadKindName(lead.kind)}</span>
        {lead.category && <span>{categoryName(lead.category)}</span>}
        <Meter confidence={lead.confidence} />
        <Basis claim={lead.claim} />
        <span>
          {lead.hops} hop{lead.hops === 1 ? "" : "s"}
        </span>
        {lead.first_time !== null && <span>{formatUtcDate(lead.first_time)}</span>}
      </div>
    </li>
  );
}

export function LeadsPanel({
  view,
  selectedLead,
  onSelectLead,
}: {
  view: TraceView;
  selectedLead: number | null;
  onSelectLead: (i: number) => void;
}) {
  const r = view.report;
  const [showOpen, setShowOpen] = useState(false);
  const indexed = r.leads.map((lead, index) => ({ lead, index }));
  const found = indexed.filter((x) => x.lead.kind.kind !== "unexplored");
  const open = indexed.filter((x) => x.lead.kind.kind === "unexplored");
  const openTotal = open.reduce((s, x) => s + x.lead.amount, 0);
  // With no endpoint reached, the branches not followed are the result: show them.
  useEffect(() => setShowOpen(found.length === 0), [view, found.length]);
  // Keep a selected branch visible.
  useEffect(() => {
    if (selectedLead !== null && r.leads[selectedLead]?.kind.kind === "unexplored") setShowOpen(true);
  }, [selectedLead, r.leads]);
  const leadsTotal = r.leads.reduce((s, l) => s + l.amount, 0);
  const forward = r.direction === "forward";

  return (
    <>
      <section aria-label="Traced amount">
        <p className="muted small">{forward ? "Traced forward from" : "Traced back from"}</p>
        <p className="summary-amount">
          <Btc sats={r.seed_amount} />
        </p>
        <p className="muted small mono" title={r.seed}>
          {r.seed.length > 40 ? `${r.seed.slice(0, 18)}…${r.seed.slice(-18)}` : r.seed}
        </p>
      </section>

      <section aria-label="Leads">
        <h2 style={{ marginBottom: 8 }}>{forward ? "Where the funds ended up" : "Where the funds came from"}</h2>
        {found.length === 0 && (
          <p className="muted small">
            No endpoint was reached within the limits. Raise the hop or transaction limits, or trace one of
            the branches below on its own.
          </p>
        )}
        <ul className="leads" role="listbox" aria-label="Leads">
          {found.map(({ lead, index }) => (
            <LeadRow
              key={index}
              lead={lead}
              index={index}
              total={r.seed_amount}
              selected={selectedLead === index}
              onSelect={onSelectLead}
            />
          ))}
        </ul>
      </section>

      {open.length > 0 && (
        <section aria-label="Not followed">
          <button
            type="button"
            className="btn group-toggle"
            aria-expanded={showOpen}
            onClick={() => setShowOpen((v) => !v)}
          >
            <span>
              Not followed: {open.length} branch{open.length === 1 ? "" : "es"}
            </span>
            <span>
              <Btc sats={openTotal} /> <span className="muted small">{formatShare(openTotal, r.seed_amount)}</span>
            </span>
          </button>
          {showOpen && (
            <ul className="leads" role="listbox" aria-label="Branches not followed" style={{ marginTop: 6 }}>
              {open.map(({ lead, index }) => (
                <LeadRow
                  key={index}
                  lead={lead}
                  index={index}
                  total={r.seed_amount}
                  selected={selectedLead === index}
                  onSelect={onSelectLead}
                />
              ))}
            </ul>
          )}
        </section>
      )}

      <section className="accounting" aria-label="Accounting">
        <div className="row">
          <span>Leads</span>
          <Btc sats={leadsTotal} />
        </div>
        <div className="row">
          <span>Fees</span>
          <Btc sats={r.stats.fees} />
        </div>
        <div className="row">
          <span>Small branches not followed</span>
          <Btc sats={r.stats.below_threshold} />
        </div>
        <div className="row total">
          <span>Traced amount</span>
          <Btc sats={leadsTotal + r.stats.fees + r.stats.below_threshold} />
        </div>
        <p className="muted small">
          Every satoshi is accounted for. Mixed funds are split in proportion to value (pro-rata).
        </p>
      </section>

      {r.labels_seen.length > 0 && (
        <section aria-label="Labelled addresses passed through">
          <h3 style={{ marginBottom: 6 }}>Labelled addresses passed through</h3>
          <p className="muted small" style={{ marginBottom: 6 }}>
            These labels were not strong enough to stop the trace.
          </p>
          <ul className="leads">
            {r.labels_seen.map((l) => (
              <li key={l.address.value} className="small">
                <span className="mono">{l.address.value}</span>
                <br />
                {l.entity} ({categoryName(l.category)}) <Basis claim={l.claim} />
              </li>
            ))}
          </ul>
        </section>
      )}

      <p className="muted small">
        Examined {r.stats.txs_examined} transactions and checked {r.stats.addresses_checked} addresses.
        {r.stats.budget_exhausted && " The transaction budget ran out; raise it to go further."}
      </p>
    </>
  );
}
