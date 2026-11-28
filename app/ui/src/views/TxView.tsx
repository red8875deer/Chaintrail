import { useState } from "react";
import { call } from "../api/client";
import type { LabelView } from "../api/types";
import { Btc, CategoryTag, ErrorBox, Id, Loading, Meter, useLoad } from "../components/bits";
import { formatInt, formatUtc, shortId } from "../lib/format";
import { href } from "../lib/route";

function Who({ address, label, script }: { address: string | null; label: LabelView | null; script: string | null }) {
  return (
    <div style={{ display: "grid", gap: 3 }}>
      {address ? (
        <a className="mono small" href={href({ page: "address", address })} title={address}>
          {address}
        </a>
      ) : (
        <span className="muted small">No address ({script ?? "unknown"} script)</span>
      )}
      {label && (
        <span className="lead-meta" title={`Third-party label from ${label.source}: a lead, not proof`}>
          <span className="basis attribution">Label</span>
          <strong style={{ color: "var(--ink)" }}>{label.entity}</strong>
          <CategoryTag category={label.category} />
          <Meter confidence={label.confidence} />
        </span>
      )}
    </div>
  );
}

const SHOW = 25;

function ShowAll({ total, shown, onClick }: { total: number; shown: number; onClick: () => void }) {
  if (total <= shown) return null;
  return (
    <button type="button" className="btn subtle" onClick={onClick} style={{ marginTop: 8 }}>
      Show all {total}
    </button>
  );
}

export function TxView({ txid }: { txid: string }) {
  const { data: tx, error, loading } = useLoad(() => call("btc_tx", { txid }), [txid]);
  const [allIn, setAllIn] = useState(false);
  const [allOut, setAllOut] = useState(false);
  return (
    <div className="page">
      <div className="page-inner">
        <header style={{ display: "grid", gap: 6 }}>
          <p className="muted small">Bitcoin transaction</p>
          <h1 className="mono" style={{ fontSize: "1.1rem" }}>
            {txid}
          </h1>
        </header>
        {loading && <Loading what="Loading transaction…" />}
        <ErrorBox error={error} />
        {tx && (
          <>
            <div className="actions">
              <a className="btn primary" href={href({ page: "trace", params: { txid: tx.txid, direction: "forward", run: true } })}>
                Trace where it went
              </a>
              <a className="btn" href={href({ page: "trace", params: { txid: tx.txid, direction: "backward", run: true } })}>
                Trace where it came from
              </a>
              <Id value={tx.txid} short />
            </div>
            <section className="panel">
              <dl className="facts">
                <div className="fact">
                  <dt>Confirmed</dt>
                  <dd>{tx.block ? `${formatUtc(tx.block.time)}, block ${formatInt(tx.block.height)}` : "Not yet"}</dd>
                </div>
                <div className="fact">
                  <dt>Moved</dt>
                  <dd>
                    <Btc sats={tx.total_out} />
                  </dd>
                </div>
                <div className="fact">
                  <dt>Fee</dt>
                  <dd>
                    <Btc sats={tx.fee} /> <span className="muted small">{tx.vsize} vB</span>
                  </dd>
                </div>
              </dl>
            </section>
            <section className="panel" style={{ display: "grid", gap: 10 }}>
              <h2>What this transaction looks like</h2>
              <p>
                <strong>{tx.shape.description.charAt(0).toUpperCase() + tx.shape.description.slice(1)}</strong>{" "}
                {tx.shape.coinjoin_confidence && <Meter confidence={tx.shape.coinjoin_confidence} />}
                {tx.shape.service_confidence && (
                  <span className="small muted"> suggests a service wallet <Meter confidence={tx.shape.service_confidence} /></span>
                )}
              </p>
              {tx.shape.evidence.length > 0 && (
                <ul className="evidence">
                  {tx.shape.evidence.map((e, i) => (
                    <li key={i}>{e}</li>
                  ))}
                </ul>
              )}
              {tx.change ? (
                <div>
                  <p>
                    Output #{tx.change.output} is likely change returned to the sender{" "}
                    <Meter confidence={tx.change.confidence} /> <span className="basis inference">Inferred</span>
                  </p>
                  <ul className="evidence">
                    {tx.change.evidence.map((e, i) => (
                      <li key={i}>{e}</li>
                    ))}
                  </ul>
                </div>
              ) : (
                <p className="muted small">No output stands out as change.</p>
              )}
              {tx.peel && <p className="small">Looks like a step in a peel chain: a small payment split off a large balance.</p>}
            </section>
            <div className="io">
              <section className="panel">
                <div className="panel-head">
                  <h2>From</h2>
                  <Btc sats={tx.total_in} />
                </div>
                <table className="ledger">
                  <tbody>
                    {(allIn ? tx.inputs : tx.inputs.slice(0, SHOW)).map((i, n) => (
                      <tr key={n}>
                        <td>
                          {i.coinbase ? (
                            <span>Newly mined coins</span>
                          ) : (
                            <Who address={i.address} label={i.label} script={i.script_type} />
                          )}
                        </td>
                        <td className="num">
                          <Btc sats={i.value} unit={false} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {!allIn && <ShowAll total={tx.inputs.length} shown={SHOW} onClick={() => setAllIn(true)} />}
              </section>
              <section className="panel">
                <div className="panel-head">
                  <h2>To</h2>
                  <Btc sats={tx.total_out} />
                </div>
                <table className="ledger">
                  <tbody>
                    {(allOut ? tx.outputs : tx.outputs.slice(0, SHOW)).map((o) => (
                      <tr key={o.index}>
                        <td>
                          <span className="muted small">#{o.index} </span>
                          {tx.change?.output === o.index && <span className="basis inference">Likely change</span>}
                          <Who address={o.address} label={o.label} script={o.script_type} />
                          <div className="row-actions small">
                            {!o.spendable ? (
                              <span className="muted">Data, cannot be spent</span>
                            ) : o.spent && o.spent_by ? (
                              <span className="muted">
                                Spent in{" "}
                                <a className="mono" href={href({ page: "tx", txid: o.spent_by })}>
                                  {shortId(o.spent_by)}
                                </a>
                                {o.spent_height ? ` (block ${formatInt(o.spent_height)})` : " (unconfirmed)"}
                              </span>
                            ) : (
                              <span>Unspent: funds at rest</span>
                            )}
                            {o.spendable && o.spent && (
                              <a
                                className="btn subtle"
                                href={href({ page: "trace", params: { txid: tx.txid, vout: o.index, direction: "forward", run: true } })}
                              >
                                Trace this output
                              </a>
                            )}
                          </div>
                        </td>
                        <td className="num">
                          <Btc sats={o.value} unit={false} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {!allOut && <ShowAll total={tx.outputs.length} shown={SHOW} onClick={() => setAllOut(true)} />}
              </section>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
