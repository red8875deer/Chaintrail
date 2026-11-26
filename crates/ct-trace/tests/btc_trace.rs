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
    assert_eq!(lead.title, "Example Exchange");
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
    assert_eq!(service.address, Some(hot));
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
