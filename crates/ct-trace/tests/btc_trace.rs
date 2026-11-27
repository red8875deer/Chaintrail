//! Engine tests on small synthetic transaction graphs.
//! Every test also checks conservation: leads + fees + dust = seed amount.

use ct_core::{Claim, Confidence, EntityCategory, Label};
use ct_trace::testing::{MemoryBitcoin, addr, coinbase, tx, txid};
use ct_trace::{
    Direction, FlowRole, LeadKind, MemoryLabels, NoLabels, Seed, StopReason, TraceConfig,
    TraceReport, trace_bitcoin,
};

const BTC: u64 = 100_000_000;

async fn run(p: &MemoryBitcoin, seed: Seed, config: TraceConfig) -> TraceReport {
    run_with_labels(p, &NoLabels, seed, config).await
}

async fn run_with_labels(
    p: &MemoryBitcoin,
    labels: &dyn ct_trace::LabelLookup,
    seed: Seed,
    config: TraceConfig,
) -> TraceReport {
    let r = trace_bitcoin(p, labels, seed, config, &mut |_| {})
        .await
        .unwrap();
    assert_eq!(r.accounted(), r.seed_amount, "conservation: {r:#?}");
    r
}

fn forward() -> TraceConfig {
    TraceConfig {
        min_amount: Some(1),
        ..Default::default()
    }
}

/// Victim (addr 1) pays scammer (addr 2) 1 BTC in tx 1.
fn victim_payment() -> ct_core::UtxoTx {
    tx(
        &txid(1),
        100,
        &[(&txid(900), 0, &addr(1), BTC + 10_000)],
        &[(&addr(2), BTC)],
    )
}

#[tokio::test]
async fn simple_chain_ends_at_rest() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 10_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let r = run(
        &p,
        Seed::Payment {
            txid: txid(1),
            address: addr(2),
        },
        forward(),
    )
    .await;

    assert_eq!(r.seed_amount, BTC);
    assert_eq!(r.leads.len(), 1);
    let lead = &r.leads[0];
    assert_eq!(lead.kind, LeadKind::AtRest);
    assert_eq!(lead.address, Some(addr(3)));
    assert_eq!(lead.amount, BTC - 10_000);
    assert_eq!(lead.hops, 1);
    assert_eq!(lead.confidence, Confidence::Certain);
    assert_eq!(lead.path, vec![txid(1), txid(2)]);
    assert_eq!(r.stats.fees, 10_000);
}

#[tokio::test]
async fn mixed_funds_split_pro_rata() {
    // Scammer spends the 1 BTC together with 3 BTC of other funds into 2 + 1.999 BTC.
    let t2 = tx(
        &txid(2),
        101,
        &[
            (&txid(1), 0, &addr(2), BTC),
            (&txid(800), 0, &addr(4), 3 * BTC),
        ],
        &[(&addr(5), 2 * BTC), (&addr(6), 1_999 * BTC / 1000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    let at = |n: u8| {
        r.leads
            .iter()
            .find(|l| l.address == Some(addr(n)))
            .unwrap()
            .amount
    };
    assert_eq!(at(5), BTC / 2); // 1 × 2/4
    assert_eq!(at(6), 49_975_000); // 1 × 1.999/4
    assert_eq!(r.stats.fees, 25_000);
}

#[tokio::test]
async fn coinjoin_is_a_mixer_endpoint() {
    let cj_inputs: Vec<(String, u32, ct_core::Address, u64)> = (0..5)
        .map(|i| {
            if i == 0 {
                (txid(1), 0, addr(2), BTC)
            } else {
                (txid(700 + i), 0, addr(50 + i as u8), BTC)
            }
        })
        .collect();
    let ins: Vec<(&str, u32, &ct_core::Address, u64)> = cj_inputs
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let outs_addrs: Vec<_> = (0..5).map(|i| addr(60 + i)).collect();
    let outs: Vec<(&ct_core::Address, u64)> = outs_addrs.iter().map(|a| (a, BTC / 2)).collect();
    let cj = tx(&txid(2), 101, &ins, &outs);
    let p = MemoryBitcoin::new([victim_payment(), cj]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    assert_eq!(r.leads.len(), 1);
    let lead = &r.leads[0];
    assert_eq!(lead.kind, LeadKind::Endpoint);
    assert_eq!(lead.category, Some(EntityCategory::Mixer));
    assert_eq!(lead.amount, BTC);
    assert_eq!(lead.confidence, Confidence::High);
    assert!(matches!(lead.claim, Claim::Inference { .. }));
}

#[tokio::test]
async fn busy_wallet_stops_the_trace() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(7), BTC - 1_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 0, &addr(7), BTC - 1_000)],
        &[(&addr(8), BTC - 2_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]).with_summary(&addr(7), 20_000, 5_000);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    let lead = &r.leads[0];
    assert_eq!(lead.kind, LeadKind::Endpoint);
    assert_eq!(lead.category, Some(EntityCategory::UnknownService));
    assert_eq!(lead.address, Some(addr(7)));
    assert_eq!(lead.confidence, Confidence::High);
    // tx 3 is never fetched: the trace stopped at the service.
    assert_eq!(p.call_count("tx"), 2);
}

#[tokio::test]
async fn deposit_consolidation_is_a_service_endpoint() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(9), BTC - 1_000)],
    );
    // addr 9 (deposit address) is swept with 59 other deposits.
    let mut ins: Vec<(String, u32, ct_core::Address, u64)> =
        vec![(txid(2), 0, addr(9), BTC - 1_000)];
    for i in 0..59u8 {
        ins.push((txid(1000 + i as u32), 0, addr(100 + i), 1_000_000));
    }
    let ins_ref: Vec<(&str, u32, &ct_core::Address, u64)> = ins
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let sweep = tx(&txid(3), 102, &ins_ref, &[(&addr(250), 150 * BTC / 100)]);
    let p = MemoryBitcoin::new([victim_payment(), t2, sweep]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    let lead = &r.leads[0];
    assert_eq!(lead.kind, LeadKind::Endpoint);
    assert_eq!(lead.address, Some(addr(9)));
    assert_eq!(lead.txid, Some(txid(3)));
    assert!(
        lead.title
            .contains("deposit swept together with 59 other addresses"),
        "{}",
        lead.title
    );
    assert_eq!(lead.confidence, Confidence::Medium);
}

#[tokio::test]
async fn labels_take_priority() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(10), BTC - 1_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let labels = MemoryLabels::new([Label {
        address: addr(10),
        entity: "Example Exchange".into(),
        category: EntityCategory::Exchange,
        claim: Claim::Attribution {
            source: "test".into(),
            confidence: Confidence::High,
            as_of: None,
            url: None,
        },
        note: None,
    }]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let lead = &r.leads[0];
    assert_eq!(lead.title, "Example Exchange (exchange)");
    assert_eq!(lead.category, Some(EntityCategory::Exchange));
    assert!(matches!(lead.claim, Claim::Attribution { .. }));
}

#[tokio::test]
async fn hop_limit_and_budget() {
    // Chain: 1 → 2 → 3 → 4.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(11), BTC)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 0, &addr(11), BTC)],
        &[(&addr(12), BTC)],
    );
    let t4 = tx(
        &txid(4),
        103,
        &[(&txid(3), 0, &addr(12), BTC)],
        &[(&addr(13), BTC)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3, t4]);

    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        TraceConfig {
            max_hops: 2,
            ..forward()
        },
    )
    .await;
    let lead = &r.leads[0];
    assert_eq!(
        lead.kind,
        LeadKind::Unexplored {
            stop: StopReason::HopLimit
        }
    );
    assert_eq!(lead.address, Some(addr(12)));
    assert_eq!(lead.hops, 2);

    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        TraceConfig {
            max_txs: 2,
            ..forward()
        },
    )
    .await;
    assert_eq!(
        r.leads[0].kind,
        LeadKind::Unexplored {
            stop: StopReason::Budget
        }
    );
    assert!(r.stats.budget_exhausted);
    assert_eq!(r.stats.txs_examined, 2);
}

#[tokio::test]
async fn small_branches_are_accounted_as_dust() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(14), BTC - 500_000), (&addr(15), 400_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        TraceConfig {
            min_amount: Some(1_000_000),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(r.leads.len(), 1);
    assert_eq!(r.stats.below_threshold, 400_000);
}

#[tokio::test]
async fn converging_paths_add_up_without_refetching() {
    // Seed pays the scammer twice in one tx; both outputs are spent together.
    let t1 = tx(
        &txid(1),
        100,
        &[(&txid(900), 0, &addr(1), BTC + 10_000)],
        &[(&addr(2), BTC / 2), (&addr(3), BTC / 2)],
    );
    let t2 = tx(
        &txid(2),
        101,
        &[
            (&txid(1), 0, &addr(2), BTC / 2),
            (&txid(1), 1, &addr(3), BTC / 2),
        ],
        &[(&addr(16), BTC - 2_000)],
    );
    let p = MemoryBitcoin::new([t1, t2]);
    let r = run(&p, Seed::Tx { txid: txid(1) }, forward()).await;
    assert_eq!(r.leads.len(), 1);
    assert_eq!(r.leads[0].amount, BTC - 2_000);
    assert_eq!(p.call_count("tx"), 2);
}

#[tokio::test]
async fn peel_chain_is_followed_along_change() {
    // 10 BTC peels 0.1 BTC (round payment) twice; change is non-round and 5x+ larger.
    let t1 = tx(
        &txid(1),
        100,
        &[(&txid(900), 0, &addr(1), 10 * BTC + 1_000)],
        &[(&addr(2), 10 * BTC)],
    );
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), 10 * BTC)],
        &[(&addr(20), 10_000_000), (&addr(21), 989_987_651)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 1, &addr(21), 989_987_651)],
        &[(&addr(22), 10_000_000), (&addr(23), 979_975_303)],
    );
    let p = MemoryBitcoin::new([t1, t2, t3]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    assert_eq!(r.stats.peel_steps, 2);
    let change_flows: Vec<_> = r
        .flows
        .iter()
        .filter(|f| matches!(f.role, FlowRole::Change { .. }))
        .collect();
    assert_eq!(change_flows.len(), 2);
    assert!(change_flows.iter().all(|f| f.peel));
    // Peeled payments and the remaining balance are all leads.
    assert_eq!(r.leads.len(), 3);
}

#[tokio::test]
async fn backward_finds_mined_coins_and_service_withdrawals() {
    // addr 30 mined 6.25 BTC; a service paid addr 31 1 BTC in a 60-output batch.
    let cb = coinbase(&txid(10), 50, &addr(30), 625_000_000);
    let batch_addrs: Vec<_> = (0..59u8).map(|i| addr(120 + i)).collect();
    let a31 = addr(31);
    let mut batch_outs: Vec<(&ct_core::Address, u64)> = vec![(&a31, BTC)];
    batch_outs.extend(batch_addrs.iter().map(|a| (a, 1_000_000)));
    let hot = addr(200);
    let batch = tx(
        &txid(11),
        60,
        &[(&txid(999), 0, &hot, 200 * BTC)],
        &batch_outs,
    );
    // Seed: addr 30 and addr 31 fund a payment to addr 32.
    let seed = tx(
        &txid(12),
        70,
        &[
            (&txid(10), 0, &addr(30), 625_000_000),
            (&txid(11), 0, &addr(31), BTC),
        ],
        &[(&addr(32), 725_000_000 - 5_000)],
    );
    let p = MemoryBitcoin::new([cb, batch, seed]);
    let r = run(
        &p,
        Seed::Tx { txid: txid(12) },
        TraceConfig {
            direction: Direction::Backward,
            min_amount: Some(1),
            ..Default::default()
        },
    )
    .await;

    let mined = r.leads.iter().find(|l| l.kind == LeadKind::Mined).unwrap();
    let service = r
        .leads
        .iter()
        .find(|l| l.kind == LeadKind::Endpoint)
        .unwrap();
    assert_eq!(mined.txid, Some(txid(10)));
    assert_eq!(service.category, Some(EntityCategory::UnknownService));
    // The service is the payout transaction as a whole, not one of its input addresses.
    assert_eq!(service.address, None);
    assert!(service.txid.is_some());
    let _ = hot;
    // Pro-rata by input value: 6.25 : 1.
    assert_eq!(mined.amount + service.amount, r.seed_amount);
    assert!(mined.amount > 6 * service.amount);
}

#[tokio::test]
async fn bad_seed_is_an_error() {
    let p = MemoryBitcoin::new([victim_payment()]);
    let err = trace_bitcoin(
        &p,
        &NoLabels,
        Seed::Payment {
            txid: txid(1),
            address: addr(99),
        },
        forward(),
        &mut |_| {},
    )
    .await;
    assert!(err.is_err());
    assert!(
        trace_bitcoin(
            &p,
            &NoLabels,
            Seed::Tx { txid: txid(77) },
            forward(),
            &mut |_| {}
        )
        .await
        .is_err()
    );
}

fn label(n: u8, entity: &str, category: EntityCategory, confidence: Confidence) -> Label {
    Label {
        address: addr(n),
        entity: entity.into(),
        category,
        claim: Claim::Attribution {
            source: "test-pack".into(),
            confidence,
            as_of: None,
            url: Some("https://example.org/list".into()),
        },
        note: None,
    }
}

/// Deposit address 9 is swept with 59 others; one co-spent address (100) is a labelled
/// exchange wallet, so the deposit is that exchange's.
fn sweep_graph() -> MemoryBitcoin {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(9), BTC - 1_000)],
    );
    let mut ins: Vec<(String, u32, ct_core::Address, u64)> =
        vec![(txid(2), 0, addr(9), BTC - 1_000)];
    for i in 0..59u8 {
        ins.push((txid(1000 + i as u32), 0, addr(100 + i), 1_000_000));
    }
    let ins_ref: Vec<(&str, u32, &ct_core::Address, u64)> = ins
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let sweep = tx(&txid(3), 102, &ins_ref, &[(&addr(250), 150 * BTC / 100)]);
    MemoryBitcoin::new([victim_payment(), t2, sweep])
}

#[tokio::test]
async fn consolidation_is_named_by_a_co_spent_label() {
    let p = sweep_graph();
    let labels = MemoryLabels::new([label(
        100,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let lead = &r.leads[0];
    assert_eq!(lead.category, Some(EntityCategory::Exchange));
    assert!(
        lead.title
            .starts_with("Likely Example Exchange (exchange): deposit swept"),
        "{}",
        lead.title
    );
    assert_eq!(lead.address, Some(addr(9)));
    // Shape (Medium, 60 inputs) and label (High) must both hold: Medium.
    assert_eq!(lead.confidence, Confidence::Medium);
    match &lead.claim {
        Claim::Inference {
            heuristic,
            evidence,
            ..
        } => {
            assert_eq!(heuristic, "label.same_wallet");
            assert!(
                evidence
                    .iter()
                    .any(|e| e.contains("https://example.org/list"))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn consolidation_named_by_swept_destination_is_capped_at_medium() {
    let p = sweep_graph();
    let labels = MemoryLabels::new([label(
        250,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let lead = &r.leads[0];
    assert_eq!(lead.category, Some(EntityCategory::Exchange));
    assert_eq!(lead.confidence, Confidence::Medium);
}

#[tokio::test]
async fn weak_labels_are_reported_but_do_not_stop() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(10), BTC - 1_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 0, &addr(10), BTC - 1_000)],
        &[(&addr(11), BTC - 2_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let labels = MemoryLabels::new([label(
        10,
        "Rumoured Exchange",
        EntityCategory::Exchange,
        Confidence::Low,
    )]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    assert_eq!(r.leads[0].kind, LeadKind::AtRest);
    assert_eq!(r.leads[0].address, Some(addr(11)));
    assert_eq!(r.labels_seen.len(), 1);
    assert_eq!(r.labels_seen[0].entity, "Rumoured Exchange");
}

#[tokio::test]
async fn backward_batch_payout_is_named_by_sender_label() {
    let hot = addr(200);
    let batch_addrs: Vec<_> = (0..59u8).map(|i| addr(120 + i)).collect();
    let a31 = addr(31);
    let mut outs: Vec<(&ct_core::Address, u64)> = vec![(&a31, BTC)];
    outs.extend(batch_addrs.iter().map(|a| (a, 1_000_000)));
    let batch = tx(&txid(11), 60, &[(&txid(999), 0, &hot, 200 * BTC)], &outs);
    let p = MemoryBitcoin::new([batch]);
    let labels = MemoryLabels::new([label(
        200,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Output {
            txid: txid(11),
            vout: 0,
        },
        TraceConfig {
            direction: Direction::Backward,
            min_amount: Some(1),
            ..Default::default()
        },
    )
    .await;
    let lead = &r.leads[0];
    assert_eq!(lead.category, Some(EntityCategory::Exchange));
    assert!(
        lead.title
            .starts_with("Likely Example Exchange (exchange): funds sent in a batch payout"),
        "{}",
        lead.title
    );
    // Drawn as the payout transaction; the labelled sender is in the evidence.
    assert_eq!(lead.address, None);
}

#[tokio::test]
async fn recorded_traces_replay_identically() {
    use ct_trace::testing::{Recorder, Recording, Replay};
    let p = sweep_graph();
    let seed = || Seed::Output {
        txid: txid(1),
        vout: 0,
    };
    let recorder = Recorder::new(&p);
    let live = trace_bitcoin(&recorder, &NoLabels, seed(), forward(), &mut |_| {})
        .await
        .unwrap();
    // Round-trip through JSON, as a fixture file would.
    let json = serde_json::to_string(&recorder.into_recording()).unwrap();
    let replay = Replay::new(serde_json::from_str::<Recording>(&json).unwrap());
    let again = trace_bitcoin(&replay, &NoLabels, seed(), forward(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&live.leads).unwrap(),
        serde_json::to_value(&again.leads).unwrap()
    );
    assert_eq!(live.flows.len(), again.flows.len());
}

#[tokio::test]
async fn consolidation_of_long_dormant_coins_is_followed_not_called_a_service() {
    // The traced coin and 59 others were created at height 100 and swept ~139 days later
    // (20,000 blocks): an owner consolidating old coins, as the 2016 Bitfinex thief did.
    let t1 = tx(
        &txid(1),
        100,
        &[(&txid(900), 0, &addr(1), BTC + 10_000)],
        &[(&addr(2), BTC)],
    );
    let mut prevs = vec![t1];
    let mut ins: Vec<(String, u32, ct_core::Address, u64)> = vec![(txid(1), 0, addr(2), BTC)];
    for i in 0..59u8 {
        let id = txid(1000 + i as u32);
        prevs.push(tx(
            &id,
            100,
            &[(&txid(5000 + i as u32), 0, &addr(1), 2_000_000)],
            &[(&addr(100 + i), 1_000_000)],
        ));
        ins.push((id, 0, addr(100 + i), 1_000_000));
    }
    let ins_ref: Vec<(&str, u32, &ct_core::Address, u64)> = ins
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    prevs.push(tx(
        &txid(3),
        20_100,
        &ins_ref,
        &[(&addr(250), 159 * BTC / 100)],
    ));
    let p = MemoryBitcoin::new(prevs);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;

    assert!(
        r.leads
            .iter()
            .all(|l| l.category != Some(EntityCategory::UnknownService)),
        "{r:#?}"
    );
    let lead = &r.leads[0];
    assert_eq!(lead.kind, LeadKind::AtRest);
    assert_eq!(lead.address, Some(addr(250)));
    let note = r.flows.iter().find_map(|f| f.note.clone()).unwrap();
    assert!(note.contains("not a service sweep"), "{note}");
}

#[tokio::test]
async fn graph_of_a_trace() {
    use ct_trace::graph::{NodeKind, build};
    // Victim → scammer (seed) → split; one branch into a CoinJoin, one at rest.
    let cj_ins: Vec<(String, u32, ct_core::Address, u64)> = (0..5)
        .map(|i| {
            if i == 0 {
                (txid(2), 0, addr(5), BTC / 2)
            } else {
                (txid(700 + i), 0, addr(50 + i as u8), BTC / 2)
            }
        })
        .collect();
    let ins: Vec<(&str, u32, &ct_core::Address, u64)> = cj_ins
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let outs_addrs: Vec<_> = (0..5).map(|i| addr(60 + i)).collect();
    let outs: Vec<(&ct_core::Address, u64)> = outs_addrs.iter().map(|a| (a, BTC / 4)).collect();
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(5), BTC / 2), (&addr(6), BTC / 2 - 1_000)],
    );
    let cj = tx(&txid(3), 102, &ins, &outs);
    let p = MemoryBitcoin::new([victim_payment(), t2, cj]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let g = build(&r);
    // The coin entering the CoinJoin has its own edge into the transaction.
    assert!(
        r.flows
            .iter()
            .any(|f| f.to_coin.as_deref() == Some(txid(3).as_str()) && f.from == Some(addr(5))),
        "{:#?}",
        r.flows
    );

    let node = |id: &str| g.nodes.iter().find(|n| n.id == id).unwrap();
    let seed = node(addr(2).as_str());
    assert!(seed.seed);
    assert_eq!(seed.hop, 0);
    assert_eq!(seed.amount_out, BTC - 1_000);
    let rest = node(addr(6).as_str());
    assert!(rest.lead.is_some());
    assert_eq!(rest.hop, 1);
    // The CoinJoin is a transaction node reached from addr 5.
    let mixer = g
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Transaction)
        .unwrap();
    assert_eq!(mixer.category, Some(EntityCategory::Mixer));
    assert!(
        g.edges
            .iter()
            .any(|e| e.source == addr(5).as_str() && e.target == mixer.id)
    );
    // Every edge connects existing nodes.
    for e in &g.edges {
        assert!(g.nodes.iter().any(|n| n.id == e.source));
        assert!(g.nodes.iter().any(|n| n.id == e.target));
    }
    // Every lead appears on some node.
    for i in 0..r.leads.len() {
        assert!(
            g.nodes
                .iter()
                .any(|n| n.lead.as_ref().is_some_and(|l| l.index == i)),
            "lead {i}"
        );
    }
}

#[tokio::test]
async fn graph_node_totals_never_exceed_the_traced_amount() {
    use ct_trace::graph::build;
    // addr 2 pays itself twice before moving on: the self-transfers must not be counted.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(2), BTC - 1_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 0, &addr(2), BTC - 1_000)],
        &[(&addr(2), BTC - 2_000)],
    );
    let t4 = tx(
        &txid(4),
        103,
        &[(&txid(3), 0, &addr(2), BTC - 2_000)],
        &[(&addr(7), BTC - 3_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3, t4]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let g = build(&r);
    for n in &g.nodes {
        assert!(
            n.amount_in <= r.seed_amount && n.amount_out <= r.seed_amount,
            "{n:?}"
        );
    }
}

#[tokio::test]
async fn graph_keeps_every_lead_that_ends_at_the_same_address() {
    use ct_trace::graph::build;
    // addr 3 gets part of the funds directly (left unspent) and the rest via addr 4, which
    // it spends again after the hop limit: two different leads at one address.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC / 4), (&addr(4), BTC / 2 + BTC / 4 - 10_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 1, &addr(4), BTC / 2 + BTC / 4 - 10_000)],
        &[(&addr(3), BTC / 2 + BTC / 4 - 20_000)],
    );
    let t4 = tx(
        &txid(4),
        103,
        &[(&txid(3), 0, &addr(3), BTC / 2 + BTC / 4 - 20_000)],
        &[(&addr(5), BTC / 2 + BTC / 4 - 30_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3, t4]);
    let r = run(
        &p,
        Seed::Payment {
            txid: txid(1),
            address: addr(2),
        },
        TraceConfig {
            max_hops: 2,
            ..forward()
        },
    )
    .await;
    let at3: Vec<usize> = (0..r.leads.len())
        .filter(|&i| r.leads[i].address == Some(addr(3)))
        .collect();
    assert_eq!(at3.len(), 2, "{:#?}", r.leads);
    let g = build(&r);
    let node = g
        .nodes
        .iter()
        .find(|n| n.id == addr(3).to_string())
        .unwrap();
    let lead = node.lead.as_ref().unwrap();
    let mut all: Vec<usize> = std::iter::once(lead.index)
        .chain(lead.others.iter().copied())
        .collect();
    all.sort_unstable();
    assert_eq!(all, at3);
    assert_eq!(
        lead.amount,
        at3.iter().map(|&i| r.leads[i].amount).sum::<u64>()
    );
    // The main lead is the larger one.
    assert_eq!(
        lead.index,
        *at3.iter().max_by_key(|&&i| r.leads[i].amount).unwrap()
    );
}

#[tokio::test]
async fn graph_of_a_whole_transaction_has_one_root_paying_each_output() {
    use ct_trace::NodeKind;
    use ct_trace::graph::build;
    // tx 2 pays a labelled exchange (an endpoint at once), an address that keeps the funds,
    // and an address that moves them on.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[
            (&addr(10), BTC / 2),
            (&addr(3), BTC / 4),
            (&addr(4), BTC / 4 - 10_000),
        ],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 2, &addr(4), BTC / 4 - 10_000)],
        &[(&addr(5), BTC / 4 - 20_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let labels = MemoryLabels::new([label(
        10,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(&p, &labels, Seed::Tx { txid: txid(2) }, forward()).await;
    assert_eq!(r.seed_outputs.len(), 3);
    let g = build(&r);

    let seeds: Vec<_> = g.nodes.iter().filter(|n| n.seed).collect();
    assert_eq!(seeds.len(), 1, "{seeds:#?}");
    assert_eq!(seeds[0].kind, NodeKind::Transaction);
    let root = &seeds[0].id;
    for (a, amount) in [(10, BTC / 2), (3, BTC / 4), (4, BTC / 4 - 10_000)] {
        let e = g
            .edges
            .iter()
            .find(|e| &e.source == root && e.target == addr(a).to_string())
            .unwrap_or_else(|| panic!("no edge to addr {a}: {:#?}", g.edges));
        assert_eq!(e.amount, amount);
    }
    // Every node is reachable from the root.
    let mut seen = std::collections::HashSet::from([root.clone()]);
    let mut queue = vec![root.clone()];
    while let Some(id) = queue.pop() {
        for e in g.edges.iter().filter(|e| e.source == id) {
            if seen.insert(e.target.clone()) {
                queue.push(e.target.clone());
            }
        }
    }
    assert_eq!(seen.len(), g.nodes.len(), "unreachable: {:#?}", g.nodes);
}

#[tokio::test]
async fn graph_of_one_output_has_no_extra_root() {
    use ct_trace::graph::build;
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 10_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let g = build(&r);
    let seeds: Vec<_> = g.nodes.iter().filter(|n| n.seed).collect();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].id, addr(2).to_string());
}

#[tokio::test]
async fn graph_follows_funds_through_an_output_without_an_address() {
    use ct_core::ScriptType;
    use ct_trace::graph::build;
    // tx 2 pays a P2PK output (no address), which tx 3 spends to addr 4.
    let mut t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 10_000)],
    );
    t2.outputs[0].address = None;
    t2.outputs[0].script_type = ScriptType::P2pk;
    let mut t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 0, &addr(3), BTC - 10_000)],
        &[(&addr(4), BTC - 20_000)],
    );
    t3.inputs[0].address = None;
    t3.inputs[0].script_type = Some(ScriptType::P2pk);
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let r = run(
        &p,
        Seed::Payment {
            txid: txid(1),
            address: addr(2),
        },
        forward(),
    )
    .await;
    let g = build(&r);
    // One node for the addressless coin, with the funds going in and out of it.
    let coin = format!("noaddr:{}:0", txid(2));
    assert!(g.edges.iter().any(|e| e.target == coin), "{:#?}", g.edges);
    assert!(
        g.edges
            .iter()
            .any(|e| e.source == coin && e.target == addr(4).to_string())
    );
    assert_eq!(
        g.nodes
            .iter()
            .filter(|n| n.id.starts_with("noaddr:"))
            .count(),
        1
    );
}

#[tokio::test]
async fn small_test_payment_to_an_endpoint_counts_toward_it() {
    // The scammer first sends a 20,000-sat test payment to the exchange, then the rest.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(10), 20_000), (&addr(3), BTC - 30_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 1, &addr(3), BTC - 30_000)],
        &[(&addr(10), BTC - 40_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let labels = MemoryLabels::new([label(
        10,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(
        &p,
        &labels,
        Seed::Payment {
            txid: txid(1),
            address: addr(2),
        },
        TraceConfig {
            min_amount: Some(100_000),
            ..Default::default()
        },
    )
    .await;
    let ex: Vec<_> = r
        .leads
        .iter()
        .filter(|l| l.address == Some(addr(10)))
        .collect();
    assert_eq!(ex.len(), 1, "{:#?}", r.leads);
    assert_eq!(ex[0].kind, LeadKind::Endpoint);
    assert_eq!(ex[0].amount, 20_000 + BTC - 40_000);
    assert_eq!(r.stats.below_threshold, 0);
}

#[tokio::test]
async fn backward_credits_each_traced_output_with_its_own_share() {
    // tx 20 pays addr 30 (1 BTC) and addr 31 (2 BTC); the seed tx 21 spends both.
    let t19 = tx(
        &txid(19),
        90,
        &[(&txid(900), 0, &addr(28), 3 * BTC + 10_000)],
        &[(&addr(29), 3 * BTC)],
    );
    let t20 = tx(
        &txid(20),
        100,
        &[(&txid(19), 0, &addr(29), 3 * BTC)],
        &[(&addr(30), BTC), (&addr(31), 2 * BTC - 10_000)],
    );
    let t21 = tx(
        &txid(21),
        101,
        &[
            (&txid(20), 0, &addr(30), BTC),
            (&txid(20), 1, &addr(31), 2 * BTC - 10_000),
        ],
        &[(&addr(40), 3 * BTC - 20_000)],
    );
    let p = MemoryBitcoin::new([t19, t20, t21]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(21),
            vout: 0,
        },
        TraceConfig {
            direction: Direction::Backward,
            max_hops: 3,
            min_amount: Some(1),
            ..Default::default()
        },
    )
    .await;
    // In tx 20, each output is credited only with what the seed drew from it.
    let into = |a: u8| -> u64 {
        r.flows
            .iter()
            .filter(|f| f.txid == txid(20) && f.to == Some(addr(a)))
            .map(|f| f.amount)
            .sum()
    };
    let (a30, a31) = (into(30), into(31));
    assert!(a30 > 0 && a31 > 0, "{:#?}", r.flows);
    assert!(a31 > a30 * 19 / 10 && a31 < a30 * 21 / 10, "{a30} vs {a31}");
    assert!(a30 <= BTC && a31 <= 2 * BTC);
    // Leads are never further back than the hop limit.
    assert!(r.leads.iter().all(|l| l.hops <= 3), "{:#?}", r.leads);
}

#[tokio::test]
async fn backward_leads_respect_the_hop_limit() {
    let t20 = tx(
        &txid(20),
        100,
        &[(&txid(19), 0, &addr(29), BTC)],
        &[(&addr(30), BTC - 10_000)],
    );
    let t21 = tx(
        &txid(21),
        101,
        &[(&txid(20), 0, &addr(30), BTC - 10_000)],
        &[(&addr(40), BTC - 20_000)],
    );
    let p = MemoryBitcoin::new([t20, t21]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(21),
            vout: 0,
        },
        TraceConfig {
            direction: Direction::Backward,
            max_hops: 1,
            min_amount: Some(1),
            ..Default::default()
        },
    )
    .await;
    assert!(r.leads.iter().all(|l| l.hops <= 1), "{:#?}", r.leads);
    // The branch not followed names the coin exactly.
    let nf = r
        .leads
        .iter()
        .find(|l| matches!(l.kind, LeadKind::Unexplored { .. }))
        .unwrap();
    assert_eq!(nf.txid, Some(txid(20)));
    assert_eq!(nf.vout, Some(0));
}
