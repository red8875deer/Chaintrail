# ChainTrail

**Free, open-source cryptocurrency tracing.** Follow funds forwards and backwards across
Bitcoin, Ethereum and Tron, see where they end up (exchanges, mixers, bridges, DeFi
protocols, sanctioned wallets), and get a list of leads instead of a wall of wallet IDs.

> Status: early development. Bitcoin tracing with address labels, an interactive graph and a
> desktop app (Phases 1–3). Ethereum and Tron come next. Not yet ready to rely on for real
> investigations: verify every lead.
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

## Use it

**Desktop app.** Installers for Windows, macOS and Linux are built by the
[release workflow](.github/workflows/release.yml) and attached to the repository's
releases. Early builds are not code-signed, so your system will warn before opening them.

**In your browser, on your own computer.** Build once, then run:

```sh
(cd app/ui && npm ci && npm run build)
cargo run --release -p ct-cli -- serve
# open the link it prints (http://127.0.0.1:7878/#token=…)
```

Only your computer can connect, and the link contains a one-time access token.

**Command line.**

```sh
# Where did the 10 BTC of the first Bitcoin payment go?
cargo run -p ct-cli -- btc trace f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16 --vout 0
# Where did the funds of a transaction come from?
cargo run -p ct-cli -- btc trace <txid> --backward
# Who owns this address? (sanctions lists, exchanges, mixers, ransomware, ...)
cargo run -p ct-cli -- labels lookup 1Kr6QSydW9bFQG1mXiPNNu6WpJGmUa9i1g
# Keep the sanctions list current
cargo run -p ct-cli -- labels update-ofac
```

About 48,000 labelled addresses ship with the app, each with a link to its source (see
[data/labels/SOURCES.md](data/labels/SOURCES.md)). Add your own from the Labels page or
with `labels add`. Fetched data is cached in your user data folder
(`chaintrail/cache.sqlite`), so repeat traces are instant.

### Build the desktop app yourself

Needs Rust, Node.js 22 and, on Linux, `libwebkit2gtk-4.1-dev` and friends
(see `.github/workflows/ci.yml`).

```sh
(cd app/ui && npm ci)
cd app/src-tauri && npx @tauri-apps/cli@2 build
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
