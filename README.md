# ChainTrail

**Free, open-source cryptocurrency tracing.** Follow funds forwards and backwards across
Bitcoin, Ethereum and Tron, see where they end up (exchanges, mixers, bridges, DeFi
protocols, sanctioned wallets), and get a list of leads instead of a wall of wallet IDs.

> Status: early development (Phase 0 – foundations). Not ready for real investigations yet.
> See [docs/ROADMAP.md](docs/ROADMAP.md).

## Goals

- **Answer "where did the money go?"** Automatic tracing stops at real-world endpoints and
  produces leads: *"4.2 BTC reached a deposit address at Exchange X on 2026-03-02"*.
- **Accurate and honest.** Every claim is marked as an on-chain fact, an inference, or an
  attribution, with its source and confidence. Uncertain links are never shown as certain.
- **Costs nothing to run.** A standalone desktop app that uses free public data sources.
  No servers, no subscriptions. Optionally point it at your own node later.
- **Open data.** Address labels live in this repository with a source for every entry,
  so anyone can review, correct and extend them.

## Coverage (first release)

| Chain    | Assets traced                         |
|----------|---------------------------------------|
| Bitcoin  | BTC                                   |
| Ethereum | ETH, USDT, USDC (and any ERC-20)      |
| Tron     | TRX, USDT (and any TRC-20)            |

Why these: they carry most of the value in fraud, scam and ransomware cases, and each one
has a free data source. See [docs/decisions](docs/decisions) for the reasoning.

## Try it (developer preview)

Requires a recent Rust toolchain.

```sh
cargo run -p ct-cli -- btc address 12cbQLTFMXRnSzktFkuoG3eHoMeFtpTu3S
cargo run -p ct-cli -- btc tx f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16
cargo run -p ct-cli -- check TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md): how the app is put together
- [Tracing methodology](docs/TRACING.md): heuristics, endpoints, leads, confidence
- [Data sources](docs/DATA_SOURCES.md): which free APIs are used and their limits
- [Roadmap](docs/ROADMAP.md): phases and progress
- [Decision records](docs/decisions): why things are the way they are

## Privacy note

Public APIs can see which addresses you look up. For sensitive work, run your own node
(support planned) or use a VPN.

## License

Apache License 2.0. See [LICENSE](LICENSE).
