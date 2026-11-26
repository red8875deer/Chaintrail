import { call } from "../api/client";
import type { LabelView } from "../api/types";
import { Btc, CategoryTag, ErrorBox, Id, Loading, Meter, useLoad } from "../components/bits";
import { formatInt, formatUtc, shortId } from "../lib/format";
import { href } from "../lib/route";

export function LabelList({ labels }: { labels: LabelView[] }) {
  if (labels.length === 0) {
    return <p className="muted">No labels for this address. That means unknown, not innocent.</p>;
  }
  return (
    <ul className="leads">
      {labels.map((l, i) => (
        <li key={i} style={{ display: "grid", gap: 4, padding: "8px 0" }}>
          <div className="lead-meta" style={{ color: "var(--ink)", fontSize: "0.95rem" }}>
            <strong>{l.entity}</strong>
            <CategoryTag category={l.category} />
            <Meter confidence={l.confidence} />
            <span className="basis attribution">Label</span>
          </div>
          <p className="small muted">
            From the “{l.pack}” pack{l.as_of ? `, as of ${l.as_of}` : ""}.{" "}
            {/^https?:\/\//.test(l.source) ? (
              <a href={l.source} target="_blank" rel="noreferrer noopener">
                Check the source
              </a>
            ) : (
              <span>Source: {l.source}</span>
            )}
          </p>
          {l.note && <p className="small">{l.note}</p>}
        </li>
      ))}
    </ul>
  );
}

export function AddressView({ address }: { address: string }) {
  const { data, error, loading } = useLoad(() => call("btc_address", { address, pages: 2 }), [address]);
  return (
    <div className="page">
      <div className="page-inner">
        <header style={{ display: "grid", gap: 6 }}>
          <p className="muted small">Bitcoin address</p>
          <h1 className="mono" style={{ fontSize: "1.3rem" }}>
            {address}
          </h1>
        </header>
        {loading && <Loading what="Loading address…" />}
        <ErrorBox error={error} />
        {data && (
          <>
            <section className="panel">
              <div className="panel-head">
                <h2>Who owns it</h2>
                <a className="btn subtle" href={href({ page: "labels", address })}>
                  Add a label
                </a>
              </div>
              <LabelList labels={data.labels} />
            </section>
            <section className="panel">
              <dl className="facts">
                <div className="fact">
                  <dt>Balance</dt>
                  <dd>
                    <Btc sats={data.balance} />
                  </dd>
                </div>
                <div className="fact">
                  <dt>Received</dt>
                  <dd>
                    <Btc sats={data.summary.received} />
                  </dd>
                </div>
                <div className="fact">
                  <dt>Sent</dt>
                  <dd>
                    <Btc sats={data.summary.spent} />
                  </dd>
                </div>
                <div className="fact">
                  <dt>Transactions</dt>
                  <dd>{formatInt(data.summary.tx_count)}</dd>
                </div>
              </dl>
            </section>
            <section className="panel">
              <div className="panel-head">
                <h2>Transactions</h2>
                {data.more && <span className="muted small">Showing the {data.transactions.length} most recent</span>}
              </div>
              <table className="ledger">
                <thead>
                  <tr>
                    <th>Date</th>
                    <th>Transaction</th>
                    <th className="num">Change for this address</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {data.transactions.map((t) => (
                    <tr key={t.txid}>
                      <td className="small">{formatUtc(t.time, "unconfirmed")}</td>
                      <td>
                        <a className="mono small" href={href({ page: "tx", txid: t.txid })} title={t.txid}>
                          {shortId(t.txid, 12, 8)}
                        </a>
                      </td>
                      <td className="num">
                        <Btc sats={t.net} signed />
                      </td>
                      <td className="num">
                        {t.net > 0 ? (
                          <a
                            className="btn subtle"
                            href={href({ page: "trace", params: { txid: t.txid, to: address, direction: "forward", run: true } })}
                          >
                            Trace where it went
                          </a>
                        ) : (
                          <a
                            className="btn subtle"
                            href={href({ page: "trace", params: { txid: t.txid, direction: "backward", run: true } })}
                          >
                            Trace where it came from
                          </a>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
            <p className="small muted">
              <Id value={address} copy />
            </p>
          </>
        )}
      </div>
    </div>
  );
}
