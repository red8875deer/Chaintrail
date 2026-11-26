//! Golden cases: fund flows documented in public court or government documents,
//! reproduced offline from recorded chain data (`tests/golden/*.toml` + `*.json`).
//!
//! Each case states what the document says (with a link and quote) and what the trace must
//! find. A failure means the engine no longer reproduces a documented real-world flow.

use std::path::Path;

use ct_core::{Address, Chain};
use ct_trace::testing::{Recording, Replay};
use ct_trace::{Direction, NoLabels, Seed, TraceConfig, trace_bitcoin};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    case: Meta,
    trace: TraceSpec,
    #[serde(default)]
    expect: Vec<Expect>,
}

#[derive(Deserialize)]
struct Meta {
    id: String,
    #[allow(dead_code)]
    title: String,
    document: String,
    #[allow(dead_code)]
    quote: String,
    /// What is verified and what is not (shown on failure).
    status: String,
    recording: String,
}

#[derive(Deserialize)]
struct TraceSpec {
    direction: Direction,
    txid: String,
    vout: Option<u32>,
    to: Option<String>,
    max_hops: u32,
    max_txs: u32,
    min_amount_sats: Option<u64>,
    #[serde(default)]
    check_addresses: bool,
}

#[derive(Deserialize)]
struct Expect {
    /// The traced funds must flow into this address...
    address: String,
    /// ...with at least this many sats in total.
    min_sats: u64,
    why: String,
    /// The part of the address the document prints (documents often redact the start).
    document_suffix: Option<String>,
}

#[tokio::test]
async fn golden_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut cases: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    cases.sort();
    let mut failures = Vec::new();
    for path in &cases {
        let case: Case = toml::from_str(&std::fs::read_to_string(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let rec: Recording =
            serde_json::from_str(&std::fs::read_to_string(dir.join(&case.case.recording)).unwrap())
                .unwrap();
        let t = &case.trace;
        let seed = match (t.vout, &t.to) {
            (Some(vout), _) => Seed::Output {
                txid: t.txid.clone(),
                vout,
            },
            (None, Some(to)) => Seed::Payment {
                txid: t.txid.clone(),
                address: Address::parse(Chain::Bitcoin, to).unwrap(),
            },
            (None, None) => Seed::Tx {
                txid: t.txid.clone(),
            },
        };
        let config = TraceConfig {
            direction: t.direction,
            max_hops: t.max_hops,
            max_txs: t.max_txs,
            min_amount: t.min_amount_sats,
            check_addresses: t.check_addresses,
            ..Default::default()
        };
        let report = trace_bitcoin(&Replay::new(rec), &NoLabels, seed, config, &mut |_| {})
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", case.case.id));
        assert_eq!(
            report.accounted(),
            report.seed_amount,
            "{}: conservation",
            case.case.id
        );
        for e in &case.expect {
            if let Some(sfx) = &e.document_suffix {
                assert!(
                    e.address.ends_with(sfx.as_str()),
                    "{}: {} does not match the document's {sfx}",
                    case.case.id,
                    e.address
                );
            }
            let target = Address::parse(Chain::Bitcoin, &e.address).unwrap();
            let into: u64 = report
                .flows
                .iter()
                .filter(|f| match report.direction {
                    Direction::Forward => f.to.as_ref() == Some(&target),
                    Direction::Backward => f.from.as_ref() == Some(&target),
                })
                .map(|f| f.amount)
                .sum();
            if into < e.min_sats {
                failures.push(format!(
                    "{}: expected ≥ {} sats at {} ({}), found {} (document: {}; status: {})",
                    case.case.id,
                    e.min_sats,
                    e.address,
                    e.why,
                    into,
                    case.case.document,
                    case.case.status
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "golden cases failed:\n{}",
        failures.join("\n")
    );
}

/// Guard against the cases silently disappearing.
#[test]
fn golden_cases_exist() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let n = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .count();
    assert!(n >= 3, "expected at least 3 golden cases, found {n}");
}
