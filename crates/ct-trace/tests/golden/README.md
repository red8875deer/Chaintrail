# Golden cases

Each `<id>.toml` describes a fund flow documented in a public court or government document
(link and quote), how to trace it, and what the trace must find. `<id>.json` is the chain
data the trace needs, recorded with:

```sh
chaintrail btc trace <txid> [--vout N | --to ADDR] [--backward] --record <id>.json
```

`tests/golden.rs` replays every case offline. A failure means the engine no longer
reproduces a documented real-world flow.
