// What an investigator needs to ask a custodian (or a court) to freeze funds still at rest:
// each unspent output (txid:vout) with its on-chain value and the traced share, the total at
// the address, when "unspent" was checked, and who may hold the keys if a label says so.

import { useEffect, useState } from "react";
import { call } from "../api/client";
import type { GraphNode, Lead, TraceView } from "../api/types";
import { formatBtc, formatShare, formatUtc } from "../lib/format";
import { categoryName } from "../lib/meaning";
import { Btc, Id } from "./bits";

interface Coin {
  txid: string;
  vout: number;
  traced: number;
  /** On-chain value and whether it is still unspent, as checked now. */
  value?: number;
  spent?: boolean;
}

export function FreezeBlock({ view, node, leads }: { view: TraceView; node: GraphNode; leads: Lead[] }) {
  const r = view.report;
  const base: Coin[] = leads
    .filter((l) => l.kind.kind === "at_rest" && l.txid && l.vout !== null && l.vout !== undefined)
    .map((l) => ({ txid: l.txid!, vout: l.vout!, traced: l.amount }));
  const key = base.map((c) => `${c.txid}:${c.vout}`).join(",");
  const [coins, setCoins] = useState<Coin[]>(base);
  const [checkedAt, setCheckedAt] = useState<number | null>(null);
  const [failed, setFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    let live = true;
    setCoins(base);
    setCheckedAt(null);
    setFailed(false);
    setCopied(false);
    // Check each output again now: funds at rest can move at any time.
    void Promise.all(
      base.map((c) =>
        call("btc_tx", { txid: c.txid }).then((t) => {
          const o = t.outputs.find((x) => x.index === c.vout);
          return { ...c, value: o?.value, spent: o?.spent };
        }),
      ),
    )
      .then((checked) => {
        if (!live) return;
        setCoins(checked);
        setCheckedAt(Math.floor(Date.now() / 1000));
      })
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
    // `key` stands for the list of coins.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  if (base.length === 0) return null;

  const total = coins.reduce((s, c) => s + c.traced, 0);
  const moved = coins.filter((c) => c.spent);
  const label = r.labels_seen.find((l) => l.address.value === node.address);
  const custodian = node.entity ?? label?.entity ?? null;
  const text = [
    "ChainTrail summary for a freeze request. This is a lead, not proof: check it before acting on it.",
    `Generated: ${formatUtc(Date.now() / 1000)}`,
    `Trace: ${r.direction} from ${r.seed} (${formatBtc(r.seed_amount, 8)} BTC)${view.generated_at ? `, run ${formatUtc(view.generated_at)}` : ""}`,
    ...(node.address ? [`Address: ${node.address}`] : []),
    ...(custodian ? [`Possible custodian (label or inference, not proof): ${custodian}`] : ["Custodian: unknown (no label)"]),
    `Unspent outputs holding traced funds (${coins.length}):`,
    ...coins.map(
      (c) =>
        `  - ${c.txid}:${c.vout}: ${c.value !== undefined ? `${formatBtc(c.value, 8)} BTC on-chain, ` : ""}traced share ${formatBtc(c.traced, 8)} BTC${c.spent ? " (MOVED since the trace)" : ""}`,
    ),
    `Traced total at rest here: ${formatBtc(total, 8)} BTC, ${formatShare(total, r.seed_amount)} of the traced amount`,
    checkedAt
      ? `Checked unspent at ${formatUtc(checkedAt)} (public data, may be up to 10 minutes old)`
      : "Not re-checked: confirm the outputs are still unspent before acting",
    `Amounts follow the pro-rata rule: where funds were mixed, each output gets a share in proportion to value.`,
  ].join("\n");

  return (
    <section className="request" aria-label="For a freeze request">
      <h4>For a freeze request</h4>
      <div className="request-actions">
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
      </div>
      {moved.length > 0 && (
        <p className="callout small" role="alert">
          {moved.length === coins.length ? "These funds have" : `${moved.length} of these outputs have`} moved since the
          trace: trace again before asking for a freeze.
        </p>
      )}
      <dl className="request-list">
        <dt>Custodian</dt>
        <dd>{custodian ? `${custodian}${label ? ` (${categoryName(label.category)}, label)` : ""}` : "Unknown: no label"}</dd>
        <dt>Traced total here</dt>
        <dd>
          <Btc sats={total} /> <span className="muted small">{formatShare(total, r.seed_amount)}</span>
        </dd>
        <dt>Checked unspent</dt>
        <dd>{checkedAt ? formatUtc(checkedAt) : failed ? "could not be checked" : "checking…"}</dd>
      </dl>
      <ol className="request-deposits" aria-label="Unspent outputs">
        {coins.map((c) => (
          <li key={`${c.txid}:${c.vout}`}>
            <div className="request-deposit-head">
              <Id value={`${c.txid}:${c.vout}`} short />
              {c.spent && <strong className="small">moved</strong>}
            </div>
            <div className="small">
              {c.value !== undefined && (
                <>
                  on-chain <Btc sats={c.value} />
                  {" · "}
                </>
              )}
              traced <Btc sats={c.traced} />
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}
