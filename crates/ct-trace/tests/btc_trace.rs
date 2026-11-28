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
    // Nothing mixed on the way: the amount is an on-chain fact.
    assert_eq!(lead.mixed_in, None);
    assert_eq!(lead.claim, Claim::Fact);
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
    // Every transfer out of the mixing transaction is marked too.
    assert!(
        r.flows
            .iter()
            .filter(|f| f.txid == txid(2))
            .all(|f| f.mixed_in == Some(txid(2)))
    );
    // The amounts are a pro-rata allocation after mixing, not on-chain facts.
    for l in &r.leads {
        assert_eq!(l.mixed_in, Some(txid(2)), "{l:#?}");
        assert_eq!(l.confidence, Confidence::Medium);
        match &l.claim {
            Claim::Inference {
                heuristic,
                evidence,
                ..
            } => {
                assert_eq!(heuristic, "allocation.pro_rata");
                assert!(evidence[0].contains(&txid(2)), "{evidence:?}");
            }
            other => panic!("{other:?}"),
        }
    }
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
async fn labelled_scam_wallet_is_followed_however_busy() {
    // Address 7 collected many victim payments (busy), but it is labelled as a scam: the
    // trace follows it on instead of calling it a service.
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
    let labels = MemoryLabels::new([label(
        7,
        "Example scam",
        EntityCategory::Scam,
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
    assert!(
        r.leads
            .iter()
            .all(|l| l.category != Some(EntityCategory::UnknownService)),
        "{:#?}",
        r.leads
    );
    assert_eq!(r.leads[0].address, Some(addr(8)));
    assert_eq!(r.leads[0].kind, LeadKind::AtRest);
    assert!(r.labels_seen.iter().any(|l| l.address == addr(7)));
}

#[tokio::test]
async fn whirlpool_tx0_premix_is_a_mixer_and_change_is_followed() {
    // The scammer's coin goes into a Tx0: 6 premix outputs of 5,010,000 sats (0.05 BTC
    // pool), an OP_RETURN, the coordinator fee and change.
    const PREMIX: u64 = 5_010_000;
    let coin = 6 * PREMIX + 250_000 + 2_000_000 + 1_000;
    let pay = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), coin), (&addr(4), BTC - coin - 1_000)],
    );
    let mut outs: Vec<(ct_core::Address, u64)> = (0..6).map(|i| (addr(40 + i), PREMIX)).collect();
    outs.push((addr(50), 250_000));
    outs.push((addr(51), 2_000_000));
    let outs: Vec<(&ct_core::Address, u64)> = outs.iter().map(|(a, v)| (a, *v)).collect();
    let mut tx0 = tx(&txid(3), 102, &[(&txid(2), 0, &addr(3), coin)], &outs);
    tx0.outputs.push(ct_core::TxOutput {
        index: tx0.outputs.len() as u32,
        address: None,
        value: 0,
        script_type: ct_core::ScriptType::OpReturn,
        script_pubkey: String::new(),
    });
    let p = MemoryBitcoin::new([victim_payment(), pay, tx0]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let mixer = r
        .leads
        .iter()
        .find(|l| l.category == Some(EntityCategory::Mixer))
        .expect("a Whirlpool lead");
    assert_eq!(mixer.txid.as_deref(), Some(txid(3).as_str()));
    assert_eq!(mixer.amount, 6 * PREMIX);
    assert_eq!(mixer.confidence, Confidence::High);
    assert!(mixer.title.contains("Whirlpool"), "{}", mixer.title);
    // Change and the coordinator fee are followed as ordinary outputs (here: at rest).
    for (a, v) in [(addr(51), 2_000_000), (addr(50), 250_000)] {
        let l = r
            .leads
            .iter()
            .find(|l| l.address == Some(a.clone()))
            .expect("lead");
        assert_eq!(l.kind, LeadKind::AtRest);
        assert_eq!(l.amount, v);
    }
    // No service: the Tx0's 9 outputs are not a batch payout.
    assert!(
        r.leads
            .iter()
            .all(|l| l.category != Some(EntityCategory::UnknownService))
    );
}

#[tokio::test]
async fn oversized_share_or_minimum_is_refused() {
    let p = MemoryBitcoin::new([victim_payment()]);
    let seed = || Seed::Output {
        txid: txid(1),
        vout: 0,
    };
    for config in [
        TraceConfig {
            seed_share: Some(BTC + 1),
            ..forward()
        },
        TraceConfig {
            min_amount: Some(BTC + 1),
            ..forward()
        },
    ] {
        let e = trace_bitcoin(&p, &NoLabels, seed(), config, &mut |_| {})
            .await
            .unwrap_err();
        assert!(e.to_string().contains("larger than"), "{e}");
    }
    assert_eq!(ct_trace::btc::default_min_amount(540), 5);
    assert_eq!(ct_trace::btc::default_min_amount(50_000_000), 50_000);
    assert_eq!(ct_trace::btc::default_min_amount(200_000), 1_000);
}

#[tokio::test]
async fn whole_tx_seed_outputs_are_checked_as_endpoints() {
    // Tracing a whole transaction: its output to a labelled exchange is an endpoint at once
    // (hop 0), not followed into the exchange's next payout.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(10), BTC / 2), (&addr(11), BTC / 2 - 1_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2]);
    let labels = MemoryLabels::new([label(
        10,
        "Example Exchange",
        EntityCategory::Exchange,
        Confidence::High,
    )]);
    let r = run_with_labels(&p, &labels, Seed::Tx { txid: txid(2) }, forward()).await;
    let ex = r
        .leads
        .iter()
        .find(|l| l.address == Some(addr(10)))
        .expect("exchange lead");
    assert_eq!(ex.kind, LeadKind::Endpoint);
    assert_eq!(ex.category, Some(EntityCategory::Exchange));
    assert_eq!(ex.hops, 0);
    assert_eq!(ex.amount, BTC / 2);
    assert!(r.labels_seen.iter().all(|l| l.address != addr(10)));
}

#[tokio::test]
async fn small_branches_are_listed_with_their_coins() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 101_000), (&addr(4), 100_000)],
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
            ..forward()
        },
    )
    .await;
    assert_eq!(r.stats.below_threshold, 100_000);
    let b = &r.small_branches[0];
    assert_eq!(
        (b.txid.as_str(), b.vout, b.amount, b.hop),
        (txid(2).as_str(), Some(1), 100_000, 1)
    );
    assert_eq!(b.address, Some(addr(4)));
}

#[tokio::test]
async fn unconfirmed_funds_at_rest_are_not_a_fact() {
    let mut t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 1_000)],
    );
    t2.block = None;
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
    let l = &r.leads[0];
    assert_eq!(l.kind, LeadKind::AtRest);
    assert_eq!(l.title, "Funds at rest (unconfirmed)");
    assert_eq!(l.confidence, Confidence::Medium);
    assert!(matches!(l.claim, Claim::Inference { .. }));

    // An endpoint reached through the unconfirmed transaction is at most Medium too.
    let mut t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC - 1_000)],
    );
    t2.block = None;
    let p = MemoryBitcoin::new([victim_payment(), t2]).with_summary(&addr(3), 20_000, 5_000);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let l = &r.leads[0];
    assert_eq!(l.kind, LeadKind::Endpoint);
    assert_eq!(l.confidence, Confidence::Medium);
    let Claim::Inference { evidence, .. } = &l.claim else {
        panic!("{:?}", l.claim)
    };
    assert!(evidence.iter().any(|e| e.contains("not confirmed")));
}

#[tokio::test]
async fn a_coin_reached_again_after_it_was_followed_is_not_split() {
    // The seed splits into A (0.6) and B (0.4); tx 3 spends both into X (0.9) and Y (0.1).
    // A is followed first, so X is reached twice: 0.54 from A, then 0.36 from B. The 0.36
    // part is under the minimum (0.39 BTC) but X was already followed, so it goes the same way
    // and the lead at X gets both parts; only Y is a small branch.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), 60_000_000), (&addr(4), 40_000_000 - 1_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[
            (&txid(2), 0, &addr(3), 60_000_000),
            (&txid(2), 1, &addr(4), 40_000_000 - 1_000),
        ],
        &[(&addr(5), 90_000_000), (&addr(6), 10_000_000 - 2_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        TraceConfig {
            min_amount: Some(39_000_000),
            ..forward()
        },
    )
    .await;
    let x = r
        .leads
        .iter()
        .find(|l| l.address == Some(addr(5)))
        .expect("lead at X");
    assert!(x.amount > 89_000_000, "{}", x.amount);
    assert!(
        r.small_branches.iter().all(|b| b.address == Some(addr(6))),
        "{:#?}",
        r.small_branches
    );
    // One flow per transfer, not one per part.
    assert_eq!(
        r.flows
            .iter()
            .filter(|f| f.txid == txid(3) && f.to == Some(addr(5)))
            .count(),
        2,
        "one from each input coin"
    );
}

#[tokio::test]
async fn change_back_to_the_start_address_does_not_make_it_an_endpoint() {
    // The start address (2, busy) pays 0.1 BTC on and sends the rest back to itself; the
    // trace continues from it instead of calling it a service one hop later.
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(6), 10_000_000), (&addr(2), BTC - 10_001_000)],
    );
    let t3 = tx(
        &txid(3),
        102,
        &[(&txid(2), 1, &addr(2), BTC - 10_001_000)],
        &[(&addr(7), BTC - 10_002_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]).with_summary(&addr(2), 20_000, 5_000);
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
        r.leads.iter().all(|l| l.address != Some(addr(2))),
        "{:#?}",
        r.leads
    );
    assert!(r.leads.iter().any(|l| l.address == Some(addr(7))));
}

#[tokio::test]
async fn sanctioned_individuals_are_followed_and_entities_stop() {
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
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]);
    let seed = || Seed::Output {
        txid: txid(1),
        vout: 0,
    };
    let sdn = |kind: &str| {
        let mut l = label(
            7,
            "EXAMPLE PERSON",
            EntityCategory::Sanctioned,
            Confidence::High,
        );
        l.note = Some(format!("SDN uid 1; {kind}; programs: CYBER2"));
        l
    };
    // A person on the list: followed through, listed as passed.
    let r = run_with_labels(
        &p,
        &MemoryLabels::new([sdn("Individual")]),
        seed(),
        forward(),
    )
    .await;
    assert_eq!(r.leads[0].address, Some(addr(8)));
    assert!(r.labels_seen.iter().any(|l| l.address == addr(7)));
    // An entity on the list (an exchange, a mixer): the trace stops there.
    let r = run_with_labels(&p, &MemoryLabels::new([sdn("Entity")]), seed(), forward()).await;
    assert_eq!(r.leads[0].address, Some(addr(7)));
    assert_eq!(r.leads[0].category, Some(EntityCategory::Sanctioned));
}

#[tokio::test]
async fn coinjoin_change_matched_by_amount_is_followed() {
    // A JoinMarket-style round: 4 participants each put in one coin and get 0.1 BTC; the
    // traced participant (1 BTC in) gets 0.9 BTC + 400 sats fee back as change. The change
    // is followed; only 0.1 BTC (minus the fee received) counts as mixed.
    const E: u64 = 10_000_000;
    let mut cj_in: Vec<(String, u32, ct_core::Address, u64)> = vec![(txid(1), 0, addr(2), BTC)];
    for i in 0..3u8 {
        cj_in.push((
            txid(50 + u32::from(i)),
            0,
            addr(50 + i),
            30_000_000 + u64::from(i) * 7_000_000,
        ));
    }
    let cj_in_ref: Vec<(&str, u32, &ct_core::Address, u64)> = cj_in
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let mut outs: Vec<(ct_core::Address, u64)> = (0..4u8).map(|i| (addr(60 + i), E)).collect();
    outs.push((addr(70), BTC - E + 400));
    for i in 0..3u64 {
        outs.push((addr(71 + i as u8), 30_000_000 + i * 7_000_000 - E - 900));
    }
    let outs_ref: Vec<(&ct_core::Address, u64)> = outs.iter().map(|(a, v)| (a, *v)).collect();
    let cj = tx(&txid(2), 101, &cj_in_ref, &outs_ref);
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
    let mixer = r
        .leads
        .iter()
        .find(|l| l.category == Some(EntityCategory::Mixer))
        .expect("mixer lead");
    let change = r
        .leads
        .iter()
        .find(|l| l.address == Some(addr(70)))
        .expect("change followed");
    assert_eq!(change.amount, BTC - E + 400);
    assert_eq!(mixer.amount, E - 400);
    // Amounts after the CoinJoin rest on the change match: not on-chain facts.
    assert_eq!(change.mixed_in.as_deref(), Some(txid(2).as_str()));
}

#[tokio::test]
async fn a_failed_address_check_is_reported_not_skipped() {
    // Address 7 cannot be checked (data source down): the branch stops there as "not
    // followed: data error", instead of being treated as a plain address and followed on.
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
    let p = MemoryBitcoin::new([victim_payment(), t2, t3]).with_failing_summary(&addr(7));
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        forward(),
    )
    .await;
    let l = &r.leads[0];
    assert_eq!(l.address, Some(addr(7)));
    assert!(
        matches!(&l.kind, LeadKind::Unexplored { stop: StopReason::Error(e) } if e.contains("address check failed")),
        "{:?}",
        l.kind
    );
    assert_eq!(r.stats.address_checks_failed, 1);
}

#[tokio::test]
async fn the_budget_also_limits_lookups() {
    // tx 2 fans out to 40 addresses; with a budget of 2 transactions nothing more is
    // fetched or looked up for those 40 coins.
    let outs: Vec<(ct_core::Address, u64)> =
        (0..40u8).map(|i| (addr(100 + i), 2_000_000)).collect();
    let outs: Vec<(&ct_core::Address, u64)> = outs.iter().map(|(a, v)| (a, *v)).collect();
    let t2 = tx(&txid(2), 101, &[(&txid(1), 0, &addr(2), BTC)], &outs);
    let p = MemoryBitcoin::new([victim_payment(), t2]);
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
    assert!(r.stats.budget_exhausted);
    assert_eq!(p.call_count("address_summary"), 0);
    assert!(
        p.call_count("outspends") <= 2,
        "{}",
        p.call_count("outspends")
    );
    assert_eq!(r.leads.len(), 40);
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

#[tokio::test]
async fn backward_fees_are_not_mixing() {
    // One input, one output at every step: only fees leave. Amounts stay facts.
    let t20 = tx(
        &txid(20),
        100,
        &[(&txid(19), 0, &addr(29), BTC)],
        &[(&addr(30), BTC - 1_000)],
    );
    let t21 = tx(
        &txid(21),
        101,
        &[(&txid(20), 0, &addr(30), BTC - 1_000)],
        &[(&addr(40), BTC - 2_000)],
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
            max_hops: 2,
            min_amount: Some(1),
            ..Default::default()
        },
    )
    .await;
    assert!(
        r.leads.iter().all(|l| l.mixed_in.is_none()),
        "{:#?}",
        r.leads
    );
    assert!(r.flows.iter().all(|f| f.mixed_in.is_none()));
}

#[tokio::test]
async fn following_a_share_of_an_output_is_an_allocation() {
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
        TraceConfig {
            seed_share: Some(BTC / 2),
            ..forward()
        },
    )
    .await;
    assert_eq!(r.seed_amount, BTC / 2);
    // The rest of the output is other funds: the seed transaction is where they mix.
    assert!(
        r.leads.iter().all(|l| l.mixed_in == Some(txid(1))),
        "{:#?}",
        r.leads
    );
    assert!(r.leads.iter().all(|l| l.claim != Claim::Fact));
}

#[tokio::test]
async fn funds_at_rest_are_one_lead_per_coin() {
    let t2 = tx(
        &txid(2),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(3), BTC / 2), (&addr(3), BTC / 2 - 10_000)],
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
    let rest: Vec<_> = r
        .leads
        .iter()
        .filter(|l| l.kind == LeadKind::AtRest)
        .collect();
    assert_eq!(rest.len(), 2, "{rest:#?}");
    let mut vouts: Vec<_> = rest.iter().map(|l| l.vout).collect();
    vouts.sort();
    assert_eq!(vouts, vec![Some(0), Some(1)]);
    assert_eq!(rest.iter().map(|l| l.amount).sum::<u64>(), BTC - 10_000);
}

#[tokio::test]
async fn a_coin_reached_by_a_shorter_route_is_not_stopped_by_the_hop_limit() {
    // T1 splits 90/10 to A and B. A goes on through T2 to C. T3 spends C (hop 2) and B
    // (hop 1) together into O, which T4 spends to P. The big part reaches O by the longer
    // route first; O is still within 3 hops by the shorter one.
    let t1 = tx(
        &txid(31),
        101,
        &[(&txid(1), 0, &addr(2), BTC)],
        &[(&addr(50), 9 * BTC / 10), (&addr(51), BTC / 10 - 1_000)],
    );
    let t2 = tx(
        &txid(32),
        102,
        &[(&txid(31), 0, &addr(50), 9 * BTC / 10)],
        &[(&addr(52), 9 * BTC / 10 - 1_000)],
    );
    let t3 = tx(
        &txid(33),
        103,
        &[
            (&txid(32), 0, &addr(52), 9 * BTC / 10 - 1_000),
            (&txid(31), 1, &addr(51), BTC / 10 - 1_000),
        ],
        &[(&addr(53), BTC - 3_000)],
    );
    let t4 = tx(
        &txid(34),
        104,
        &[(&txid(33), 0, &addr(53), BTC - 3_000)],
        &[(&addr(54), BTC - 4_000)],
    );
    let p = MemoryBitcoin::new([victim_payment(), t1, t2, t3, t4]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(1),
            vout: 0,
        },
        TraceConfig {
            max_hops: 3,
            ..forward()
        },
    )
    .await;
    // Nothing is left "not followed" at O: all of it went on to P.
    assert!(
        r.leads
            .iter()
            .all(|l| !(matches!(l.kind, LeadKind::Unexplored { .. }) && l.txid == Some(txid(33)))),
        "{:#?}",
        r.leads
    );
    let at_p: u64 = r
        .leads
        .iter()
        .filter(|l| l.address == Some(addr(54)))
        .map(|l| l.amount)
        .sum();
    // All of it (pro-rata shares are rounded down to the sat; rounding goes to fees).
    assert!((BTC - 4_000 - 5..=BTC - 4_000).contains(&at_p), "{at_p}");
}

#[tokio::test]
async fn backward_from_a_consolidation_follows_its_inputs() {
    let mut ins: Vec<(String, u32, ct_core::Address, u64)> = Vec::new();
    for i in 0..60u8 {
        ins.push((txid(1000 + i as u32), 0, addr(100 + i), 1_000_000));
    }
    let ins_ref: Vec<(&str, u32, &ct_core::Address, u64)> = ins
        .iter()
        .map(|(t, v, a, x)| (t.as_str(), *v, a, *x))
        .collect();
    let sweep = tx(&txid(3), 102, &ins_ref, &[(&addr(250), 59_000_000)]);
    let p = MemoryBitcoin::new([sweep]);
    let r = run(
        &p,
        Seed::Output {
            txid: txid(3),
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
    // Not one "service" lead at the start: the 60 inputs are where the funds came from.
    assert!(r.leads.iter().all(|l| l.hops >= 1), "{:#?}", r.leads);
    assert_eq!(r.flows.iter().filter(|f| f.txid == txid(3)).count(), 60);
}
