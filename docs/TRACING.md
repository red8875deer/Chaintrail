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
   "below value threshold".
4. **The graph:** every hop, for visual review.

## 2. Endpoints (where tracing stops)

| Category | How it's recognised |
|---|---|
| Exchange / custodial service | Label match; or behaviour (deposit sweeps, huge consolidation txs, very high counterparty count) |
| Mixer / tumbler | Label match (e.g. Tornado Cash pools); CoinJoin transaction patterns |
| Bridge | Known bridge contracts / deposit addresses; trace continues on the other chain when the bridge event can be matched |
| DeFi protocol / DEX | Known contracts; for swaps, value continues in the output asset |
| Sanctioned | OFAC SDN list match |
| Swap service (instant exchanger) | Label match; behaviour |
| Funds at rest | Unspent / unmoved balance |

An endpoint found by **label** has the label's confidence. An endpoint found by
**behaviour** is reported as e.g. "likely exchange (unidentified)" with medium/low
confidence and the evidence.

## 3. Bitcoin (UTXO) heuristics

Each heuristic returns a score in `[0,1]` and evidence text. Scores combine into a
confidence level: Certain / High / Medium / Low.

### 3.1 Common-input ownership (clustering)
Inputs spent together in one transaction are usually controlled by one wallet.
**Disabled** for transactions that look like CoinJoins (3.3), because applying it there
merges unrelated people (the "supercluster" failure).
Reference: Meiklejohn et al., *A Fistful of Bitcoins*, IMC 2013.

### 3.2 Change detection
For a payment with two or more outputs, which output returns to the sender? Signals:
- **Address type match:** change usually has the same script type as the inputs.
- **Round amounts:** the payment is often round (in BTC or fiat); change is not.
- **Unnecessary input:** if a smaller input set could have paid an output, that output is
  probably the payment, not the change.
- **Fresh address:** change goes to a never-before-seen address.
- **Precision:** change typically has more significant digits.
- **Peel-chain shape:** one large output continuing, one small output peeled off.

Reference: Kappos et al., *How to Peel a Million*, USENIX Security 2022.

### 3.3 CoinJoin detection
Many inputs, several outputs of exactly equal value, plus change outputs. Known
coordinator patterns (Wasabi, Whirlpool, JoinMarket) are recognised specifically.
A CoinJoin is an endpoint of type *mixer* unless demixing (later phase) gives a
high-confidence link.

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
The rule used is recorded with every trace.

## 6. Confidence

| Level | Meaning |
|---|---|
| Certain | On-chain fact |
| High | Deterministic protocol match or multiple strong signals agree |
| Medium | One strong signal, or several weak ones agree |
| Low | Weak or single weak signal; shown but never followed automatically |

A path's confidence is the **lowest** confidence of any hop on it.

## 7. Known limits

- **Monero, shielded Zcash:** not traceable. The trace records the entry point.
- **Off-chain exchanges / instant swappers:** the link between deposit and withdrawal is
  inside the service. Matching by amount and time is reported as *probable* at best.
- **On-demand analysis:** clusters are built from what the trace touched, not from the
  whole chain, so they can be smaller than the true wallet. This under-merges (safe)
  rather than over-merges (dangerous).
