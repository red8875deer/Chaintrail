// Leads: where the traced funds ended up (or came from), largest first.

import { type KeyboardEvent, useEffect, useState } from "react";
import type { Lead, TraceView } from "../api/types";
import { formatBtc, formatShare, formatUtcDate, shareBasisPoints, shortId } from "../lib/format";
import { categoryName, leadKindName } from "../lib/meaning";
import { href } from "../lib/route";
import { Basis, Btc, Meter, categoryColor } from "./bits";

function LeadRow({
  lead,
  index,
  total,
  selected,
  tabbable,
  onSelect,
}: {
  lead: Lead;
  index: number;
  total: number;
  selected: boolean;
  /** The one row of the list reachable with Tab (arrow keys move within the list). */
  tabbable: boolean;
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
      aria-label={`${lead.title}: ${formatBtc(lead.amount, 8)} BTC, ${formatShare(lead.amount, total)}${lead.address ? `, ${shortId(lead.address.value)}` : ""}`}
      data-lead={index}
      tabIndex={tabbable ? 0 : -1}
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
        {lead.mixed_in && (
          <span
            className="tag-allocated"
            title={`The traced funds were mixed with other funds in transaction ${lead.mixed_in}: this amount is their pro-rata share, not an on-chain fact.`}
          >
            pro-rata share
          </span>
        )}
        <span title="Transactions between the start of the trace and this lead">
          {lead.hops === 0 ? "at the start" : `${lead.hops} hop${lead.hops === 1 ? "" : "s"}`}
        </span>
        {lead.first_time !== null && <span>{formatUtcDate(lead.first_time)}</span>}
      </div>
    </li>
  );
}

/** One slice of the result: an endpoint category, funds at rest, not followed, or fees. */
interface Bucket {
  key: string;
  name: string;
  colour: string;
  amount: number;
  leads: number;
}

export function bucketOf(lead: Lead): string {
  return lead.kind.kind === "endpoint" ? `cat:${lead.category ?? "unknown_service"}` : lead.kind.kind;
}

function buckets(view: TraceView): Bucket[] {
  const r = view.report;
  const map = new Map<string, Bucket>();
  for (const l of r.leads) {
    const key = bucketOf(l);
    const b =
      map.get(key) ??
      map
        .set(key, {
          key,
          name:
            l.kind.kind === "endpoint"
              ? categoryName(l.category ?? "unknown_service")
              : l.kind.kind === "at_rest"
                ? "At rest (unspent)"
                : l.kind.kind === "mined"
                  ? "Mined"
                  : "Not followed",
          colour:
            l.kind.kind === "at_rest"
              ? "var(--n-at_rest)"
              : l.kind.kind === "unexplored"
                ? "var(--n-unexplored)"
                : categoryColor(l.category),
          amount: 0,
          leads: 0,
        })
        .get(key)!;
    b.amount += l.amount;
    b.leads += 1;
  }
  const order = (b: Bucket) => (b.key.startsWith("cat:") ? 0 : b.key === "mined" ? 1 : b.key === "at_rest" ? 2 : 3);
  const list = [...map.values()].sort((a, b) => order(a) - order(b) || b.amount - a.amount);
  if (r.stats.below_threshold > 0)
    list.push({
      key: "small",
      name: "Small branches not followed",
      colour: "repeating-linear-gradient(135deg, var(--rule) 0 3px, var(--surface-2) 3px 6px)",
      amount: r.stats.below_threshold,
      leads: 0,
    });
  if (r.stats.fees > 0) list.push({ key: "fees", name: "Fees", colour: "var(--rule)", amount: r.stats.fees, leads: 0 });
  return list;
}

/** Where the traced amount went, by kind of endpoint, at a glance; rows filter the leads. */
function Exposure({
  view,
  filter,
  onFilter,
  onLowerMinimum,
}: {
  view: TraceView;
  filter: string | null;
  onFilter: (key: string | null) => void;
  onLowerMinimum?: () => void;
}) {
  const total = view.report.seed_amount;
  const list = buckets(view);
  return (
    <section aria-label="Summary" className="exposure">
      <div className="exposure-bar" aria-hidden="true">
        {list.map((b) => (
          <span
            key={b.key}
            title={`${b.name}: ${formatShare(b.amount, total)}`}
            style={{
              // Tiny parts stay visible; the bar as a whole never exceeds its width.
              flex: `${Math.max(Number(shareBasisPoints(b.amount, total)), 60)} 1 0`,
              background: b.colour,
            }}
          />
        ))}
      </div>
      <ul className="exposure-list">
        {list.map((b) => (
          <li key={b.key}>
            <button
              type="button"
              className="exposure-row"
              aria-pressed={filter === b.key}
              disabled={b.key === "fees" || b.key === "small"}
              title={
                b.key === "fees"
                  ? "Not a lead: paid as transaction fees"
                  : b.key === "small"
                    ? `Not a lead: branches under the minimum of ${formatBtc(view.report.min_amount, 8)} BTC were not followed`
                    : "Show only these leads"
              }
              onClick={() => onFilter(filter === b.key ? null : b.key)}
            >
              <span className="swatch" style={{ background: b.colour }} />
              <span className="exposure-name">
                {b.name}
                {b.leads > 0 && <span className="muted small"> · {b.leads} lead{b.leads === 1 ? "" : "s"}</span>}
              </span>
              <span className="exposure-amount">
                <Btc sats={b.amount} />
              </span>
              <span className="muted small exposure-share">{formatShare(b.amount, total)}</span>
            </button>
          </li>
        ))}
      </ul>
      <PassedThrough view={view} />
      <SmallBranches view={view} />
      {onLowerMinimum && Number(shareBasisPoints(view.report.stats.below_threshold, total)) >= SMALL_BRANCHES_WARN_BP && (
        <p className="callout small">
          {formatShare(view.report.stats.below_threshold, total)} of the traced amount went into branches smaller than{" "}
          <Btc sats={view.report.min_amount} />, which were not followed.{" "}
          <button type="button" className="btn subtle" onClick={onLowerMinimum}>
            Follow smaller branches
          </button>
        </p>
      )}
    </section>
  );
}

/** Labelled addresses the funds went through (the trace went on): indirect exposure, with
 * the traced amount that passed there. Names come from third-party labels. */
function PassedThrough({ view }: { view: TraceView }) {
  const r = view.report;
  const rows = r.labels_seen
    .map((l) => {
      const n = view.graph.nodes.find((x) => x.address === l.address.value);
      return { l, amount: n ? Math.max(n.amount_in, n.amount_out) : 0 };
    })
    .filter((x) => x.amount > 0)
    .sort((a, b) => b.amount - a.amount);
  if (rows.length === 0) return null;
  const sanctioned = rows.filter((x) => x.l.category === "sanctioned");
  return (
    <div className="passed-through">
      {sanctioned.length > 0 && (
        <p className="callout small" role="note">
          Sanctions exposure: the funds {r.direction === "forward" ? "passed through" : "came through"}{" "}
          {sanctioned.length === 1 ? "an address" : `${sanctioned.length} addresses`} on a sanctions list (
          {sanctioned
            .slice(0, 2)
            .map((x) => x.l.entity)
            .join(", ")}
          ). The trace followed the funds on through them; dealing with these addresses may be prohibited.
        </p>
      )}
      <p className="small muted">{r.direction === "forward" ? "Passed through" : "Came through"} labelled addresses:</p>
      <ul className="exposure-list">
        {rows.slice(0, 5).map(({ l, amount }) => (
          <li key={l.address.value} className="exposure-row static" title={l.address.value}>
            <span className="swatch" style={{ background: categoryColor(l.category) }} />
            <span className="exposure-name">
              {l.entity || categoryName(l.category)}
              <span className="muted small"> · {categoryName(l.category)}</span> <Basis claim={l.claim} />
            </span>
            <span className="exposure-amount">
              <Btc sats={amount} />
            </span>
            <span className="muted small exposure-share">{formatShare(amount, r.seed_amount)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The largest branches under the minimum, each with a link to trace it on its own. */
function SmallBranches({ view }: { view: TraceView }) {
  const r = view.report;
  const list = (r.small_branches ?? []).filter((b) => b.vout !== null);
  if (list.length === 0) return null;
  const listed = list.reduce((s, b) => s + b.amount, 0);
  return (
    <details className="small-branches">
      <summary className="small">
        {listed < r.stats.below_threshold
          ? `The ${list.length} largest small branches (${formatShare(listed, r.stats.below_threshold)} of their amount)`
          : `Small branches (${list.length})`}
      </summary>
      <ul className="evidence">
        {list.map((b) => (
          <li key={`${b.txid}:${b.vout}`}>
            <Btc sats={b.amount} />{" "}
            <span className="muted small">
              {b.address ? shortId(b.address.value) : `${shortId(b.txid)}:${b.vout}`} · hop {b.hop}
            </span>{" "}
            <a
              className="small"
              href={href({
                page: "trace",
                params: {
                  txid: b.txid,
                  vout: b.vout!,
                  direction: r.direction,
                  share: b.amount,
                  mixed: b.mixed_in ?? undefined,
                  from: r.seed,
                  run: true,
                },
              })}
            >
              Trace
            </a>
          </li>
        ))}
      </ul>
    </details>
  );
}

/** Above this share of the seed in unfollowed small branches, offer a re-run with a lower minimum. */
const SMALL_BRANCHES_WARN_BP = 500;

/** Arrow keys, Home and End move through a list of leads and select (one Tab stop). */
function listKeys(onSelect: (i: number) => void, onClear?: () => void) {
  return (e: KeyboardEvent<HTMLUListElement>) => {
    if (e.key === "Escape" && onClear) {
      e.preventDefault();
      onClear();
      return;
    }
    const items = [...e.currentTarget.querySelectorAll<HTMLLIElement>("li[data-lead]")];
    const at = items.indexOf(document.activeElement as HTMLLIElement);
    const to =
      e.key === "ArrowDown" ? at + 1 : e.key === "ArrowUp" ? at - 1 : e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : null;
    if (to === null || items.length === 0) return;
    e.preventDefault();
    const next = items[Math.max(0, Math.min(items.length - 1, to))]!;
    next.focus();
    onSelect(Number(next.dataset.lead));
  };
}

export function LeadsPanel({
  view,
  selectedLead,
  onSelectLead,
  onClearLead,
  onGoToStart,
  onLowerMinimum,
}: {
  view: TraceView;
  selectedLead: number | null;
  onSelectLead: (i: number) => void;
  /** Esc in the list: no lead selected. */
  onClearLead?: () => void;
  /** Show the start of the trace on the graph. */
  onGoToStart?: () => void;
  /** Trace again with a lower minimum branch size. */
  onLowerMinimum?: () => void;
}) {
  const r = view.report;
  const [showOpen, setShowOpen] = useState(false);
  const [filter, setFilter] = useState<string | null>(null);
  useEffect(() => setFilter(null), [view]);
  const indexed = r.leads
    .map((lead, index) => ({ lead, index }))
    .filter((x) => filter === null || bucketOf(x.lead) === filter);
  const found = indexed.filter((x) => x.lead.kind.kind !== "unexplored");
  const open = indexed.filter((x) => x.lead.kind.kind === "unexplored");
  const openTotal = open.reduce((s, x) => s + x.lead.amount, 0);
  // With no endpoint reached, the branches not followed are the result: show them.
  useEffect(() => setShowOpen(found.length === 0), [view, found.length]);
  useEffect(() => {
    if (filter === "unexplored") setShowOpen(true);
  }, [filter]);
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
        {r.config.seed_share !== null && r.config.seed_share !== undefined && (
          <p className="small">
            of <Btc sats={r.seed_outputs.reduce((s, o) => s + o.amount, 0)} /> in this output: continued from an
            earlier trace, following only its share.
          </p>
        )}
        {onGoToStart ? (
          <button type="button" className="linklike small mono" title={`${r.seed}: show on the graph`} onClick={onGoToStart}>
            {r.seed.length > 40 ? `${r.seed.slice(0, 18)}…${r.seed.slice(-18)}` : r.seed}
          </button>
        ) : (
          <p className="muted small mono" title={r.seed}>
            {r.seed.length > 40 ? `${r.seed.slice(0, 18)}…${r.seed.slice(-18)}` : r.seed}
          </p>
        )}
      </section>

      <Exposure view={view} filter={filter} onFilter={setFilter} onLowerMinimum={onLowerMinimum} />

      <section aria-label="Leads">
        <h2 style={{ marginBottom: 8 }}>{forward ? "Where the funds ended up" : "Where the funds came from"}</h2>
        {filter !== null && (
          <p className="small">
            Showing one kind only.{" "}
            <button type="button" className="btn subtle" onClick={() => setFilter(null)}>
              Show all leads
            </button>
          </p>
        )}
        {found.length === 0 && filter === null && open.length > 0 && (
          <p className="muted small">
            No endpoint was reached within the limits. Raise the hop or transaction limits, or continue one of
            the branches below.
          </p>
        )}
        {r.leads.length === 0 && (
          <p className="muted small">
            Nothing to follow: all of the traced amount went to fees or to branches smaller than the minimum
            of <Btc sats={r.min_amount} /> (see the accounting below). Lower the minimum under "Limits and options"
            to follow small amounts.
          </p>
        )}
        <ul className="leads" role="listbox" aria-label="Leads" onKeyDown={listKeys(onSelectLead, onClearLead)}>
          {found.map(({ lead, index }, k) => (
            <LeadRow
              key={index}
              lead={lead}
              index={index}
              total={r.seed_amount}
              selected={selectedLead === index}
              tabbable={found.some((x) => x.index === selectedLead) ? selectedLead === index : k === 0}
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
            <ul
              className="leads"
              role="listbox"
              aria-label="Branches not followed"
              style={{ marginTop: 6 }}
              onKeyDown={listKeys(onSelectLead, onClearLead)}
            >
              {open.map(({ lead, index }, k) => (
                <LeadRow
                  key={index}
                  lead={lead}
                  index={index}
                  total={r.seed_amount}
                  selected={selectedLead === index}
                  tabbable={open.some((x) => x.index === selectedLead) ? selectedLead === index : k === 0}
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
            The trace went on past these: wallets of perpetrators and individuals are followed through, and other
            labels here were too weak to stop it.
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
