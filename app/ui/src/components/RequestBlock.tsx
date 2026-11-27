// What an investigator needs to ask a service for records about an endpoint: who, which
// address, which transaction and when, how much was paid there on-chain and how much of
// it is the traced share, and why we think it is that service. Copyable as plain text.

import { useEffect, useState } from "react";
import { call } from "../api/client";
import type { GraphNode, Lead, TraceView, TxView } from "../api/types";
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

export function RequestBlock({ view, node, lead }: { view: TraceView; node: GraphNode; lead: Lead }) {
  const r = view.report;
  const forward = r.direction === "forward";
  // The transfer that reached the endpoint (forward) or left the source (backward).
  const edge = view.graph.edges
    .filter((e) => e.source !== e.target && (forward ? e.target === node.id : e.source === node.id))
    .sort((a, b) => b.amount - a.amount)[0];
  const [tx, setTx] = useState<TxView | null>(null);
  const [failed, setFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  const txid = edge?.txid;
  useEffect(() => {
    let live = true;
    setTx(null);
    setFailed(false);
    setCopied(false);
    if (txid)
      call("btc_tx", { txid })
        .then((t) => live && setTx(t))
        .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [txid]);
  if (!edge) return null;

  // A service found by a transaction (e.g. a withdrawal payout traced back) has no single
  // address: the request names the transaction.
  const address = node.address;
  const onChain = !address
    ? null
    : tx
    ? forward
      ? tx.outputs.filter((o) => o.address === address).reduce((s, o) => s + o.value, 0)
      : tx.inputs.filter((i) => i.address === address).reduce((s, i) => s + i.value, 0)
    : null;
  const claim = describeClaim(lead.claim);
  const later = forward && lead.txid && lead.txid !== edge.txid ? lead.txid : null;

  const text = [
    "ChainTrail trace summary. This is a lead, not proof: check it before acting on it.",
    `Generated: ${formatUtc(Date.now() / 1000)}`,
    `Trace: ${forward ? "forward" : "backward"} from ${r.seed} (${formatBtc(r.seed_amount, 8)} BTC)`,
    `Lead: ${lead.title}${lead.category ? ` (${categoryName(lead.category)})` : ""}`,
    `Basis: ${claim.kind}. ${claim.headline}`,
    ...claim.evidence.map((e) => `  - ${e}`),
    ...(claim.url ? [`Source: ${claim.url}${claim.asOf ? ` (as of ${claim.asOf})` : ""}`] : []),
    ...(address ? [`${forward ? "Deposit address" : "Sending address"}: ${address}`] : []),
    `Transaction: ${edge.txid} (${formatUtc(edge.time)})`,
    ...(onChain !== null
      ? [`${forward ? "Paid to" : "Paid from"} this address in that transaction: ${formatBtc(onChain, 8)} BTC`]
      : []),
    `Traced share: ${formatBtc(edge.amount, 8)} BTC${onChain ? ` (${formatShare(edge.amount, onChain)} of that payment)` : ""}, ${formatShare(edge.amount, r.seed_amount)} of the traced amount`,
    ...(later ? [`The service then moved the funds in transaction ${later}`] : []),
    `Path (transactions from the start): ${lead.path.join(" -> ")}`,
    `Amounts follow the pro-rata rule: where funds were mixed, each output gets a share in proportion to value.`,
  ].join("\n");

  return (
    <section className="request" aria-label="For a records request">
      <h4>For a records request</h4>
      <dl className="request-list">
        {address && (
          <>
            <dt>{forward ? "Deposit address" : "Sending address"}</dt>
            <dd>
              <Id value={address} />
            </dd>
          </>
        )}
        <dt>Transaction</dt>
        <dd>
          <Id value={edge.txid} short href={href({ page: "tx", txid: edge.txid })} />
        </dd>
        <dt>Time</dt>
        <dd>{formatUtc(edge.time)}</dd>
        {address && (
          <>
            <dt>{forward ? "Paid to this address" : "Paid from this address"}</dt>
            <dd>{onChain !== null ? <Btc sats={onChain} /> : failed ? "could not be loaded" : "loading…"}</dd>
          </>
        )}
        <dt>Traced share</dt>
        <dd>
          <Btc sats={edge.amount} />
          {onChain ? <span className="muted small"> {formatShare(edge.amount, onChain)} of that payment</span> : null}
        </dd>
        {later && (
          <>
            <dt>Then moved in</dt>
            <dd>
              <Id value={later} short href={href({ page: "tx", txid: later })} />
            </dd>
          </>
        )}
      </dl>
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
        {copied ? "Copied" : "Copy summary"}
      </button>
    </section>
  );
}
