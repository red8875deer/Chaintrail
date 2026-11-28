import { href } from "../lib/route";

/** Cases documented in US court filings; the traces reproduce them (docs/TRACING.md §7). */
const EXAMPLES = [
  {
    title: "Colonial Pipeline ransom, May 2021",
    text: "Follow the ransom to the address the FBI seized 63.7 BTC from (seizure affidavit, ¶¶ 30–33).",
    params: {
      txid: "fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264",
      vout: 1,
      direction: "forward" as const,
      maxHops: 3,
      run: true,
    },
  },
  {
    title: "Silk Road “Individual X”, seized November 2020",
    text: "Trace 69,370 BTC back to the two addresses that took it from Silk Road in 2012 (forfeiture complaint, ¶¶ 15–17).",
    params: {
      txid: "3f036ff88bb851b57a1e28780dbce35a6457a8b57995c095b55b3b0cf48ba9fd",
      vout: 0,
      direction: "backward" as const,
      // As the golden case: seizure → two self-transfers → the 2013 transfer (4 hops).
      maxHops: 4,
      maxTxs: 25,
      run: true,
    },
  },
];

export function Home() {
  return (
    <div className="page">
      <div className="home">
        <header style={{ display: "grid", gap: 14 }}>
          <h1>Where did the money go?</h1>
          <p className="lede">
            Paste a Bitcoin transaction or address in the search box. ChainTrail follows the funds, stops where
            they reach an exchange, a mixer or another service, and tells you how sure it is about every step.
          </p>
        </header>

        <div className="paths">
          <div className="path-card">
            <h3>Follow a payment</h3>
            <p className="muted small">
              Start from the transaction where the money left, and see where it ended up: exchanges to contact,
              mixers, or wallets where it still sits.
            </p>
            <a className="btn" href={href({ page: "trace", params: null })}>
              Open the trace workspace
            </a>
          </div>
          <div className="path-card">
            <h3>Look into an address</h3>
            <p className="muted small">
              See an address's balance, its history and any labels: sanctions lists, exchanges, known scams.
            </p>
            <p className="small">Paste it in the search box at the top.</p>
          </div>
          <div className="path-card">
            <h3>Check who owns it</h3>
            <p className="muted small">
              About 48,000 labelled addresses, each linked to its source. Add your own as you learn more.
            </p>
            <a className="btn" href={href({ page: "labels" })}>
              Open labels
            </a>
          </div>
        </div>

        <section style={{ display: "grid", gap: 10 }}>
          <h2>Try a documented case</h2>
          <div className="examples">
            {EXAMPLES.map((e) => (
              <a key={e.title} className="example" href={href({ page: "trace", params: e.params })} style={{ color: "inherit", textDecoration: "none" }}>
                <span style={{ display: "grid", gap: 2 }}>
                  <strong>{e.title}</strong>
                  <span className="muted small">{e.text}</span>
                </span>
                <span className="btn">Run trace</span>
              </a>
            ))}
          </div>
        </section>

        <p className="muted small">
          Free and open source. Uses public blockchain data; the services you query can see which addresses you
          look up.
        </p>
      </div>
    </div>
  );
}
