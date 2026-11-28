// What an investigator needs to ask a service for records about an endpoint: who, which
// address, which transaction and when, how much was paid there on-chain and how much of
// it is the traced share, and why we think it is that service. Copyable as plain text.

import { Fragment, useEffect, useMemo, useState } from "react";
import { call } from "../api/client";
import type { GraphNode, Lead, TraceView } from "../api/types";
import { formatBtc, formatShare, formatUtc } from "../lib/format";
import { categoryName, describeClaim } from "../lib/meaning";
import { href } from "../lib/route";
import { Btc, Id } from "./bits";

/** What a transaction paid to `address` (outputs) or from it (inputs), loaded on demand. */
export function useOnChainAmount(txid: string | undefined, address: string | null | undefined, side: "to" | "from") {
  const [amount, setAmount] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    setAmount(null);
    setFailed(false);
    if (txid && address)
      call("btc_tx", { txid })
        .then((t) => {
          if (!live) return;
          const sum =
            side === "to"
              ? t.outputs.filter((o) => o.address === address).reduce((s, o) => s + o.value, 0)
              : t.inputs.filter((i) => i.address === address).reduce((s, i) => s + i.value, 0);
          setAmount(sum);
        })
        .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [txid, address, side]);
  return { amount, failed };
}

/** One transfer into the endpoint (forward) or out of the source (backward). */
interface Deposit {
  txid: string;
  /** Block time; null while unconfirmed. */
  time: number | null;
  /** Traced funds in this transfer (sats). */
  traced: number;
}

/** Most deposits whose on-chain amounts are looked up (each is one cached transaction). */
const MAX_LOOKUPS = 60;
/** Deposits listed before "Show all". */
const SHOWN = 6;

export function RequestBlock({ view, node, lead }: { view: TraceView; node: GraphNode; lead: Lead }) {
  const r = view.report;
  const forward = r.direction === "forward";
  // Every transfer that reached the endpoint (forward) or left the source (backward), oldest
  // first: a records request should name all of them, not just the largest.
  const deposits = useMemo(() => {
    const by = new Map<string, Deposit>();
    for (const e of view.graph.edges) {
      if (e.source === e.target || (forward ? e.target !== node.id : e.source !== node.id)) continue;
      const d = by.get(e.txid) ?? { txid: e.txid, time: e.time, traced: 0 };
      d.traced += e.amount;
      by.set(e.txid, d);
    }
    // Backward from a service payout: the start transaction itself is the withdrawal.
    if (by.size === 0 && lead.txid) by.set(lead.txid, { txid: lead.txid, time: lead.first_time, traced: lead.amount });
    const t = (d: Deposit) => d.time ?? Number.MAX_SAFE_INTEGER;
    return [...by.values()].sort((a, b) => t(a) - t(b) || b.traced - a.traced);
  }, [view, node.id, forward, lead]);
  /** Transfers into the endpoint that were sweeps (several inputs, all paid to it). */
  const [sweeps, setSweeps] = useState<Set<string>>(new Set());
  const [onChain, setOnChain] = useState<Map<string, number>>(new Map());
  const [failed, setFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  const [all, setAll] = useState(false);
  // A service found by a transaction (e.g. a withdrawal payout traced back) has no single
  // address: the request names the transactions.
  const address = node.address;
  useEffect(() => {
    let live = true;
    setOnChain(new Map());
    setSweeps(new Set());
    setFailed(false);
    setCopied(false);
    setAll(false);
    if (address)
      for (const d of deposits.slice(0, MAX_LOOKUPS))
        call("btc_tx", { txid: d.txid })
          .then((t) => {
            if (!live) return;
            const sum = forward
              ? t.outputs.filter((o) => o.address === address).reduce((s, o) => s + o.value, 0)
              : t.inputs.filter((i) => i.address === address).reduce((s, i) => s + i.value, 0);
            setOnChain((m) => new Map(m).set(d.txid, sum));
            // A sweep: an exchange gathering deposits into this wallet (several inputs, at
            // least 90% of the value paid here).
            const out = t.outputs.filter((o) => o.spendable).reduce((s, o) => s + o.value, 0);
            if (forward && t.inputs.length >= 2 && sum * 10 >= out * 9) setSweeps((x) => new Set(x).add(d.txid));
          })
          .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [deposits, address, forward]);
  if (deposits.length === 0) return null;

  // Swept in: the customer's deposit was the address it was swept from, paid by the
  // transaction before (or the start of the trace).
  const depositAddresses = [...sweeps].flatMap((sweep) =>
    view.graph.edges
      .filter((e) => e.txid === sweep && e.target === node.id && e.source !== node.id)
      .map((e) => {
        const from = view.graph.nodes.find((n) => n.id === e.source);
        const into = view.graph.edges
          .filter((x) => x.target === e.source && x.source !== e.source)
          .sort((a, b) => b.amount - a.amount)[0];
        const seed = r.seed_outputs.find((o) => o.address?.value === from?.address);
        return {
          sweep,
          address: from?.address ?? null,
          txid: into?.txid ?? seed?.txid ?? null,
          time: into?.time ?? seed?.time ?? null,
        };
      })
      .filter((x) => x.address),
  );
  const claim = describeClaim(lead.claim);
  const unknownOwner = lead.category === "unknown_service" || lead.category === null;
  const tracedTotal = deposits.reduce((s, d) => s + d.traced, 0);
  const single = deposits.length === 1 ? deposits[0]! : null;
  const later = forward && lead.txid && !deposits.some((d) => d.txid === lead.txid) ? lead.txid : null;
  const paid = forward ? "Paid to" : "Paid from";
  const amountText = (d: Deposit) => {
    const c = onChain.get(d.txid);
    return c !== undefined
      ? `${paid.toLowerCase()} this address ${formatBtc(c, 8)} BTC, traced share ${formatBtc(d.traced, 8)} BTC (${formatShare(d.traced, c)})`
      : `traced share ${formatBtc(d.traced, 8)} BTC`;
  };

  const text = [
    "ChainTrail trace summary. This is a lead, not proof: check it before acting on it.",
    `Generated: ${formatUtc(Date.now() / 1000)}`,
    `Trace: ${forward ? "forward" : "backward"} from ${r.seed} (${formatBtc(r.seed_amount, 8)} BTC)`,
    `Lead: ${lead.title}${lead.category ? ` (${categoryName(lead.category)})` : ""}`,
    ...(unknownOwner ? ["Owner: unknown (service detected by behaviour, not named): identify it before sending a request"] : []),
    `Basis: ${claim.kind}. ${claim.headline}`,
    ...claim.evidence.map((e) => `  - ${e}`),
    ...(claim.url ? [`Source: ${claim.url}${claim.asOf ? ` (as of ${claim.asOf})` : ""}`] : []),
    ...depositAddresses.map(
      (d) =>
        `Likely deposit address (swept into this wallet in ${d.sweep}): ${d.address}${d.txid ? `; paid in transaction ${d.txid}${d.time ? ` (${formatUtc(d.time)})` : ""}` : ""}`,
    ),
    ...(address
      ? [`${forward ? (depositAddresses.length ? "Wallet the deposit was swept into" : "Deposit address") : "Sending address"}: ${address}`]
      : []),
    deposits.length === 1
      ? `Transaction: ${single!.txid} (${formatUtc(single!.time)}): ${amountText(single!)}`
      : `Transactions (${deposits.length}, oldest first):`,
    ...(deposits.length === 1 ? [] : deposits.map((d) => `  - ${d.txid} (${formatUtc(d.time)}): ${amountText(d)}`)),
    `Traced total: ${formatBtc(tracedTotal, 8)} BTC, ${formatShare(tracedTotal, r.seed_amount)} of the traced amount`,
    ...(later ? [`The service then moved the funds in transaction ${later}`] : []),
    `Shortest route (transactions from the start): ${lead.path.join(" -> ")}`,
    `Amounts follow the pro-rata rule: where funds were mixed, each output gets a share in proportion to value.`,
  ].join("\n");

  const paidCell = (d: Deposit) => {
    const c = onChain.get(d.txid);
    if (c !== undefined) return <Btc sats={c} />;
    return failed || deposits.indexOf(d) >= MAX_LOOKUPS ? "not loaded" : "loading…";
  };

  return (
    <section className="request" aria-label="For a records request">
      <h4>For a records request</h4>
      {unknownOwner && (
        <p className="callout small">
          Owner unknown: this service was detected by its behaviour, not named. Identify who runs it (labels, its other
          activity, a commercial or law-enforcement source) before sending a request.
        </p>
      )}
      <div className="request-actions">
        {!single && deposits.length > SHOWN && (
          <button type="button" className="btn subtle" onClick={() => setAll((a) => !a)}>
            {all ? "Show fewer" : `Show all ${deposits.length}`}
          </button>
        )}
        <button
          type="button"
          className="btn"
          onClick={() =>
            navigator.clipboard?.writeText(text).then(
              () => setCopied(true),
              () => setCopied(false),
            )
          }
        >
          {copied ? "Copied" : deposits.length > 1 ? `Copy summary (all ${deposits.length})` : "Copy summary"}
        </button>
      </div>
      {depositAddresses.length > 0 && (
        <>
          <p className="callout small">
            The funds reached this wallet in a sweep (several deposit addresses gathered into it). The address the
            service can look up is most likely the one they were swept from:
          </p>
          <dl className="request-list">
            {depositAddresses.map((d) => (
              <Fragment key={`${d.sweep}-${d.address}`}>
                <dt>Likely deposit address</dt>
                <dd>
                  <Id value={d.address!} />
                </dd>
                {d.txid && (
                  <>
                    <dt>Paid in</dt>
                    <dd>
                      <Id value={d.txid} short href={href({ page: "tx", txid: d.txid })} /> {formatUtc(d.time)}
                    </dd>
                  </>
                )}
              </Fragment>
            ))}
          </dl>
        </>
      )}
      <dl className="request-list">
        {address && (
          <>
            <dt>{forward ? (depositAddresses.length ? "Swept into" : "Deposit address") : "Sending address"}</dt>
            <dd>
              <Id value={address} />
            </dd>
          </>
        )}
        {single ? (
          <>
            <dt>Transaction</dt>
            <dd>
              <Id value={single.txid} short href={href({ page: "tx", txid: single.txid })} />
            </dd>
            <dt>Time</dt>
            <dd>{formatUtc(single.time)}</dd>
            {address && (
              <>
                <dt>{paid} this address</dt>
                <dd>{paidCell(single)}</dd>
              </>
            )}
            <dt>Traced share</dt>
            <dd>
              <Btc sats={single.traced} />
              {onChain.get(single.txid) ? (
                <span className="muted small"> {formatShare(single.traced, onChain.get(single.txid)!)} of that payment</span>
              ) : null}
            </dd>
          </>
        ) : (
          <>
            <dt>Transactions</dt>
            <dd>{deposits.length}, oldest first</dd>
            <dt>Traced total</dt>
            <dd>
              <Btc sats={tracedTotal} />
            </dd>
          </>
        )}
        {later && (
          <>
            <dt>Then moved in</dt>
            <dd>
              <Id value={later} short href={href({ page: "tx", txid: later })} />
            </dd>
          </>
        )}
      </dl>
      {!single && (
        <ol className="request-deposits" aria-label="Transactions">
          {(all ? deposits : deposits.slice(0, SHOWN)).map((d) => (
            <li key={d.txid}>
              <div className="request-deposit-head">
                <span>{formatUtc(d.time)}</span>
                <Id value={d.txid} short href={href({ page: "tx", txid: d.txid })} />
              </div>
              <div className="small">
                {address && (
                  <>
                    {paid} address {paidCell(d)}
                    {" · "}
                  </>
                )}
                traced <Btc sats={d.traced} />
              </div>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
