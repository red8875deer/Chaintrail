# Tracing methodology

What the engine does, how it decides, and how it reports uncertainty.
This document is the reference; code comments link back to the section that justifies them.

## 1. What a trace produces

A trace starts from a **seed** (an address, a transaction, or a specific output/transfer)
and a **direction**:

- **Forward:** where did these funds go?
- **Backward:** where did these funds come from?

It produces:

1. **Leads:** endpoints the funds reached, each with entity, category, amount, share of
   the traced funds, time, the path taken, and a confidence.
2. **Funds at rest:** amounts still sitting in unidentified wallets (possible freeze or
   watch targets).
3. **Dead ends with reasons:** e.g. "entered Monero via swap service", "hop limit reached",
   "below value threshold". An amount below the threshold that reaches an address already
   known as an endpoint in the same trace (a label, or an address check already made) still
   counts toward that endpoint, e.g. a small test payment before the main deposit. No extra
   lookups are made for small amounts.
4. **The graph:** every hop, for visual review.

## 2. Endpoints (where tracing stops)

| Category | How it's recognised |
|---|---|
| Exchange / custodial service | Label match; or behaviour (deposit sweeps, huge consolidation txs, very high counterparty count) |
| Mixer / tumbler | Label match (e.g. Tornado Cash pools); CoinJoin transaction patterns |
| Bridge | Known bridge contracts / deposit addresses; trace continues on the other chain when the bridge event can be matched |
| DeFi protocol / DEX | Known contracts; for swaps, value continues in the output asset |
| Sanctioned | OFAC SDN list match |
| Darknet market, gambling, payment processor, wallet service | Label match |
| Swap service (instant exchanger) | Label match; behaviour |
| Funds at rest | Unspent / unmoved balance |

Wallets of **perpetrators** (labels of category ransomware, hack, scam, terrorist financing)
and of individuals are **not** endpoints: the funds are followed through them and the label
is listed as passed through. (A ransomware label on the first hop would otherwise end the
trace before the funds reach an exchange or a seizure.)

An endpoint found by **label** has the label's confidence. An endpoint found by
**behaviour** is reported as e.g. "likely exchange (unidentified)" with medium/low
confidence and the evidence.

### How tracing proceeds (implemented for Bitcoin)

- Branches are followed **largest amount first**, so a limited budget goes to the biggest
  flows. Limits: `--max-hops` (default 8), `--max-txs` (default 100), and a minimum branch
  size (default 0.1% of the traced amount, at least 1,000 sats; for traces under
  100,000 sats the floor is 1% of the amount). A minimum larger than the traced amount, or a
  continuation share larger than the start, is refused rather than clipped.
- The **seed address is never treated as an endpoint**, also when funds come back to it later
  (change to itself): you chose to trace from it, even if
  it is busy or labelled (scam collection addresses often are), and forward its sweep
  together with many other addresses is not a service stop either (a scammer gathering
  victims' payments looks the same; the note on the edge says both readings). Its label,
  if any, is listed. Backward, a start transaction that is a service payout does stop: that
  is where the funds came from; a consolidation at the start does not (its inputs are).
  Tracing forward from a **whole transaction**, its outputs are where the funds went: each is
  checked like any other address (an output to a labelled exchange is an endpoint at hop 0).
- **Hops** are counted the same way in both directions: a lead is at most `max-hops`
  transactions away from the seed. Each coin keeps the shortest route by which the funds
  reached it, and its hop count and path always agree. Branches are followed largest
  first, so a coin can first arrive by a long route: one stopped at the hop limit is
  checked again at the end and followed if a shorter route reached it meanwhile.
- **Backward**, each traced output of a transaction is credited only with what it received,
  and a whole transaction traced back is drawn as the transaction itself. Funds from a
  CoinJoin or a service payout are drawn as coming from that transaction (not from one of
  its input addresses); funds entering a CoinJoin are drawn coin by coin.
- Small amounts that reach an address already known as an endpoint count toward it.
- Transactions fetched only to look at (the age of a consolidation's inputs, whether they
  came out of CoinJoins) do not use the trace's transaction budget.
- **Accounting always balances:** leads + fees + small branches = the traced amount.
  Forward, fees and rounding go to "fees". Backward, rounding goes to the largest input.
- Leads that stop because of limits or data errors are reported as **not followed**,
  separately from real endpoints. They and funds at rest are one lead per coin
  (`txid:vout`), so each can be continued, or frozen, exactly.
- Branches under the minimum are counted as "small branches"; the 200 largest are listed with
  their coin (`small_branches`), so they can be traced on their own.
- Funds at rest in an **unconfirmed** transaction are an inference (Medium), not a fact: the
  transaction can still be replaced or dropped. Any lead whose route passes through an
  unconfirmed transaction is at most Medium and says so.
- A transaction with several traced inputs is processed once per input, so a coin can be
  reached again after it was followed: that later part goes the same way whatever its size
  (it is never filed as a separate small branch), and the parts of one transfer are reported
  as one flow.

### Labels during a trace

- A labelled address **stops** the trace if its category is an endpoint (see §2: not
  individuals or perpetrators) and its confidence is at least the stop level (Medium by
  default).
  Weaker labels are listed as "labelled addresses passed through" and tracing continues.
- An address on a sanctions list as an **individual** (OFAC "Individual" entries) is a
  person's wallet: it is followed through like a perpetrator's and shown as sanctions
  exposure. Sanctioned **entities** (exchanges, mixers, markets) stop the trace.
- An address labelled as a perpetrator or an individual is **followed through** whatever its
  activity: the busy-wallet check is not applied to it (the 2020 Twitter-hack scam address
  collected hundreds of victim payments and later dust, so it looks "busy").
- A service detected by **transaction shape** (deposit consolidation, batch payout) is
  **named** from labels on addresses of the same wallet: any labelled input of the
  transaction (common-input ownership), or, for consolidations, the address that received
  the swept funds (capped at Medium). The lead's confidence is the lower of the shape's and
  the label's, and its evidence includes the label's source link.
- A CoinJoin is named when a labelled mixer address (e.g. a coordinator fee address)
  appears in it.
- When several labels exist for an address, the strongest wins; at equal confidence a
  sanctions label is shown first, then the user's own labels.

## 3. Bitcoin (UTXO) heuristics

Each heuristic returns a score in `[0,1]` and evidence text. Scores combine into a
confidence level: Certain / High / Medium / Low.

### 3.1 Common-input ownership (clustering)
Inputs spent together in one transaction are usually controlled by one wallet.
**Disabled** for transactions that look like CoinJoins (3.3), because applying it there
merges unrelated people (the "supercluster" failure).
Reference: Meiklejohn et al., *A Fistful of Bitcoins*, IMC 2013.

### 3.2 Change detection
For a payment with two or more outputs, which output returns to the sender? Each signal
votes for one output; votes for the same output combine (noisy-OR), votes for different
outputs cancel (`best × (1 − runner-up)`), so conflicts end up Low.

| Signal | Weight | Rule |
|---|---|---|
| Address reuse | 0.95 | Exactly one output pays an input address |
| Script type | 0.70 | All inputs share a type and exactly one output has it |
| Round payment | 0.65 | Other outputs are round (≥ 4 trailing zeros in sats), this one is not (≤ 2) |
| Optimal change | 0.55 | Exactly one output is smaller than every input (2+ inputs) |

Score → confidence: ≥ 0.85 High, ≥ 0.6 Medium, else Low (not followed as "same owner").
Planned: fresh-address and fiat-round signals. Weights are initial values to be calibrated.

Reference: Kappos et al., *How to Peel a Million*, USENIX Security 2022.

### 3.3 CoinJoin detection
| Kind | Rule | Confidence |
|---|---|---|
| Whirlpool-style | 5–8 inputs, same number of outputs, all outputs equal | High |
| WabiSabi (Wasabi 2.x) | ≥ 50 inputs and outputs, ≥ 50% of outputs are standard denominations (2ⁿ, 3ⁿ, 2·3ⁿ, 10ⁿ, 2·10ⁿ, 5·10ⁿ sats ≥ 5,000) | High |
| Generic (JoinMarket, Wasabi 1.x etc.) | ≥ 3 equal outputs, at least as many inputs but at most 6 per equal output, inputs from 2+ addresses, ≤ one change per mixed output (outputs at any repeated value count as mixed: Wasabi 1.x pays several levels v, ~2v, ~4v…) | Medium (High with ≥ 5 equal) |
| Chip split (ChipMixer-style) | ≥ 5 outputs of one value 2ᵏ × 0.001 BTC, nothing else but at most one small remainder, inputs from 2+ addresses | Medium |

**Change in a CoinJoin.** In rounds with a single equal amount *e* (JoinMarket style), a
participant's change is identified by arithmetic: for an input of value *v*, exactly one
output of value *v − e* give or take the fees (at most 1% of *e*, or 10,000 sats) that no
other input also matches. Forward, that part of the traced coin is followed as change and only
the rest is the mixer lead; backward, a traced change output is traced back through its own
input. Amounts after that are inferences (Medium), since identifying the change is. Rounds
with several mixing levels (Wasabi 1.x) are not matched.

A **Whirlpool Tx0** (one wallet preparing coins: ≥ 2 equal "premix" outputs at a pool
amount — 0.001, 0.01, 0.05 or 0.5 BTC — plus at most 5% for fees, an OP_RETURN, and at most
3 other outputs for the coordinator fee and change) is not a CoinJoin and not a batch
payout. Forward, its premix outputs become one mixer lead "Likely entering Whirlpool"
(High); its change and fee outputs are followed as usual. Backward it is an ordinary
transaction (one owner).

In every kind, the equal outputs must go to separate addresses (at least 90% distinct):
equal outputs paid to one address are one owner splitting or consolidating funds (seen in
the WannaCry cash-out: 727 inputs into 6 × 500 BTC to one address). Exchange batch payouts
(few inputs, many outputs) and consolidations are explicitly not CoinJoins. Titles of
CoinJoin leads say "Likely mixed in …": the pattern is strong evidence, not proof. A CoinJoin is an endpoint of type *mixer* unless demixing (later phase) gives a
high-confidence link.

### 3.3a Service detection by behaviour
| Signal | Low (noted, trace continues) | Medium (stops) | High (stops) |
|---|---|---|---|
| Consolidation: ≤ 2 recipients (distinct output addresses), many distinct input addresses | ≥ 20 | ≥ 50 | — |
| Batch payout: many distinct output addresses, fewer inputs | ≥ 20 | ≥ 50 | ≥ 100 |
| Busy address: transactions and spent outputs | — | ≥ 1,000 and ≥ 100 | ≥ 10,000 and ≥ 1,000 |

The busy-address counts are the address's whole history to date (the free API gives no
counts as of a date); the evidence says so. An address that became busy only after the
traced transfer can therefore be over-reported: check the dates for older cases.

**Dormancy rule:** before a consolidation is treated as a service, the five largest inputs
are checked. If most had been unspent for more than 90 days, it is an owner consolidating
old coins (for example a thief moving stolen funds years later), not a service sweeping
fresh deposits, and the trace continues through it. Measured on real transactions: exchange
and darknet-market sweeps had input ages of 0–40 days; the 2016 Bitfinex thief's 2022
consolidations about 2,009 days. The `bitfinex-2016-hack` golden case fails without this rule.

**Post-mix rule:** a consolidation whose inputs are at least 80% standard CoinJoin amounts
*and* most of whose sampled inputs were created by CoinJoin transactions is one user merging
mixed coins (where the trail continues), not a service. Round amounts alone are not enough:
ordinary deposits are often round too.

Small fan-outs are not treated as services because launderers also spread funds over a few
dozen mule addresses. Addresses that only receive (donation addresses, dust targets) are
not treated as services.

### 3.4 Peel chains
Repeated pattern: large input → one small output + one large change → repeat.
Followed automatically along the change side; each peel is a candidate lead.

## 4. Ethereum and Tron (account) heuristics

### 4.1 Deposit-address detection
Exchanges give each customer a deposit address that forwards (sweeps) everything to a
hot wallet shortly after receiving it. An address that receives from outside and forwards
its full balance to the same high-volume wallet is a deposit address; the destination
is the exchange's hot wallet.
Reference: Victor, *Address Clustering Heuristics for Ethereum*, FC 2020.

### 4.2 Other links
- Gas funding: the address that paid a fresh wallet's first gas fee.
- Contract deployer relationships.
- Token approvals granted to the same spender.

### 4.3 Value through DeFi
Swaps on DEXs are decoded so the trace continues in the output token instead of stopping
at the router contract.

## 5. Allocation in mixed wallets

When traced funds land in a wallet that also holds other money, which outgoing payments
carry the traced funds? Selectable rules:

| Rule | Meaning |
|---|---|
| **FIFO** | First in, first out |
| **LIFO** | Last in, first out |
| **Pro-rata (haircut)** | Each outgoing payment carries a proportional share |
| **Poison** | Every outgoing payment counts as fully tainted (upper bound) |
| **LIBR** | Lowest intermediate balance rule |

The default for automatic tracing is **pro-rata** (no single payment is overstated).
Shares are rounded down to the satoshi (within one split, the remainder goes to the largest
output); what rounding loses is counted as fees, so amounts can be a few satoshis below a
coin's value when several traced coins meet in one transaction.
The rule used is recorded with every trace.

## 6. Confidence

| Level | Meaning |
|---|---|
| Certain | On-chain fact |
| High | Deterministic protocol match or multiple strong signals agree |
| Medium | One strong signal, or several weak ones agree |
| Low | Weak or single weak signal; shown but never followed automatically |

A path's confidence is the **lowest** confidence of any hop on it.

**Amounts after mixing are allocations, not facts.** A lead's amount is an on-chain fact
only if the traced funds were never mixed with other funds on the way (forward: every input
of each transaction was traced funds; backward: every output with value was traced; fees
are not other funds). A trace continued with only part of an output starts as an
allocation. After mixing, amounts are pro-rata shares: the lead records the transaction where the
funds were first mixed (`mixed_in`), shows "pro-rata share", and a lead that would be a
fact (funds at rest, not followed) becomes an inference about the amount (Medium) whose
evidence names that transaction. Identifications (labels, service and CoinJoin detection)
keep their own confidence; the amount basis is shown next to them.

**Continuing a trace.** A continuation follows only the share the earlier trace carried
(`seed_share`), keeps its earlier mixing (`mixed_in`) and records the earlier trace's seed
(`continued_from`) so exports can be followed back to it; the record does not change what
is traced. Continuing from an address on the way uses the largest transfer into it
(forward) or out of it (backward); the UI says when other transfers would be separate traces.

## 7. Golden cases

`crates/ct-trace/tests/golden/` holds fund flows documented by US government sources,
replayed offline from recorded chain data on every test run:

| Case | Document | What the trace must reproduce | Verified against the document |
|---|---|---|---|
| Colonial Pipeline (2021) | Seizure affidavit 3:21-mj-70945-LB, ¶¶ 28–33 | every hop of the affidavit's trail to the FBI's "Subject Address", amounts to within fees | Yes: each address ends with the suffix the (redacted) affidavit prints |
| Silk Road "Individual X" (2020) | Forfeiture complaint 3:20-cv-07811-VC, ¶¶ 15–17 | 69,370 BTC → the two 2012 recipient addresses (47,458 / 22,013 BTC) | Yes: full addresses and amounts verbatim |
| Bitfinex hack (2016/2022) | Statement of facts 1:22-mj-00022-RMM, ¶¶ 4–6 | stolen coins → the 2022 seizure wallet, through the thief's own consolidations | Partly: hack and seizure confirmed, addresses not named (regression test for the dormancy rule) |

Each case file quotes the document and states exactly what is and is not verified.

## 8. Known limits

- **Monero, shielded Zcash:** not traceable. The trace records the entry point.
- **Off-chain exchanges / instant swappers:** the link between deposit and withdrawal is
  inside the service. Matching by amount and time is reported as *probable* at best.
- **On-demand analysis:** clusters are built from what the trace touched, not from the
  whole chain, so they can be smaller than the true wallet. This under-merges (safe)
  rather than over-merges (dangerous).
