// The UI contract. Mirrors crates/ct-service/src/views.rs and the serde shapes of
// ct-core / ct-trace types exactly. Amounts are base units (sats) as numbers; times are
// Unix seconds. Change this file only together with the Rust side.

export type Chain = "bitcoin" | "ethereum" | "tron";
export type Confidence = "low" | "medium" | "high" | "certain";
export type Direction = "forward" | "backward";
export type EntityCategory =
  | "exchange"
  | "mixer"
  | "bridge"
  | "dex"
  | "defi"
  | "swap_service"
  | "payment_processor"
  | "wallet_service"
  | "mining_pool"
  | "gambling"
  | "darknet"
  | "scam"
  | "ransomware"
  | "hack"
  | "terrorist_financing"
  | "sanctioned"
  | "individual"
  | "unknown_service";

/** A validated address as serialised by ct-core. */
export interface Address {
  chain: Chain;
  value: string;
}

/** How we know something (ADR-0005). Never show an inference or attribution as a fact. */
export type Claim =
  | { kind: "fact" }
  | { kind: "inference"; heuristic: string; confidence: Confidence; evidence: string[] }
  | {
      kind: "attribution";
      source: string;
      confidence: Confidence;
      as_of: string | null;
      url?: string;
    };

export interface BlockRef {
  height: number;
  hash: string;
  time: number;
}

/** Errors come back with a kind (for styling) and a plain-language message. */
export interface ServiceError {
  kind: "invalid_input" | "not_found" | "unavailable" | "internal";
  message: string;
}

// ---- status / resolve -----------------------------------------------------------------

export interface StatusView {
  version: string;
  provider: string;
  cache: string | null;
  labelled_addresses: number;
  packs: number;
  user_labels_dir: string | null;
}

export type ResolveView =
  | { kind: "address"; chain: Chain; address: string; supported: boolean }
  | { kind: "transaction"; txid: string };

// ---- labels ---------------------------------------------------------------------------

export interface LabelView {
  entity: string;
  category: EntityCategory;
  /** Human-readable category, e.g. "swap service". */
  category_name: string;
  confidence: Confidence;
  pack: string;
  /** Where the label can be checked (URL or reference). */
  source: string;
  as_of: string | null;
  note: string | null;
}

export interface PackView {
  id: string;
  title: string;
  source: string;
  licence: string;
  as_of: string | null;
  labels: number;
  origin: string;
  category: EntityCategory;
  confidence: Confidence;
}

export interface AddLabelRequest {
  address: string;
  entity: string;
  category: EntityCategory;
  confidence: Exclude<Confidence, "certain">;
  note?: string | null;
  source?: string | null;
}

export interface OfacUpdateView {
  as_of: string | null;
  addresses: number;
  entries: number;
  unsupported: Record<string, number>;
  reassigned: string[];
  invalid: string[];
  path: string;
}

// ---- address --------------------------------------------------------------------------

export interface AddressSummary {
  address: Address;
  tx_count: number;
  received_outputs: number;
  received: number;
  spent_outputs: number;
  spent: number;
  unconfirmed_tx_count: number;
  unconfirmed_delta: number;
}

export interface TxBrief {
  txid: string;
  height: number | null;
  time: number | null;
  /** Net change for the address, sats (negative = sent). */
  net: number;
  fee: number;
  inputs: number;
  outputs: number;
}

export interface AddressView {
  address: string;
  chain: Chain;
  summary: AddressSummary;
  balance: number;
  labels: LabelView[];
  transactions: TxBrief[];
  more: boolean;
}

// ---- transaction ----------------------------------------------------------------------

export interface ShapeView {
  kind: "simple" | "coinbase" | "coinjoin" | "consolidation" | "batch_payout";
  description: string;
  service_confidence: Confidence | null;
  coinjoin_confidence: Confidence | null;
  evidence: string[];
}

export interface ChangeView {
  output: number;
  confidence: Confidence;
  evidence: string[];
}

export interface InputView {
  address: string | null;
  value: number;
  script_type: string | null;
  prev_txid: string | null;
  prev_vout: number;
  coinbase: boolean;
  label: LabelView | null;
}

export interface OutputView {
  index: number;
  address: string | null;
  value: number;
  script_type: string;
  label: LabelView | null;
  spendable: boolean;
  spent: boolean;
  spent_by: string | null;
  spent_height: number | null;
}

export interface TxView {
  txid: string;
  block: BlockRef | null;
  fee: number;
  vsize: number;
  total_in: number;
  total_out: number;
  shape: ShapeView;
  change: ChangeView | null;
  peel: boolean;
  inputs: InputView[];
  outputs: OutputView[];
}

// ---- trace ----------------------------------------------------------------------------

export interface TraceRequest {
  txid: string;
  vout?: number | null;
  to?: string | null;
  direction: Direction;
  max_hops?: number | null;
  max_txs?: number | null;
  min_sats?: number | null;
  check_addresses?: boolean | null;
}

export type StopReason =
  | { reason: "hop_limit" }
  | { reason: "budget" }
  | { reason: "error"; detail: string };

export type LeadKind =
  | { kind: "endpoint" }
  | { kind: "at_rest" }
  | { kind: "mined" }
  | { kind: "unexplored"; stop: StopReason };

export interface Lead {
  kind: LeadKind;
  title: string;
  category: EntityCategory | null;
  address: Address | null;
  /** Traced address next to the lead on the path. */
  via: Address | null;
  txid: string | null;
  amount: number;
  first_time: number | null;
  hops: number;
  confidence: Confidence;
  claim: Claim;
  /** Txids from the seed to the lead. */
  path: string[];
}

export type FlowRole =
  | { role: "change"; confidence: Confidence }
  | { role: "payment" }
  | { role: "source" };

export interface Flow {
  txid: string;
  hop: number;
  time: number | null;
  from: Address | null;
  to: Address | null;
  amount: number;
  role: FlowRole;
  peel: boolean;
  note: string | null;
}

export interface LabelSeen {
  address: Address;
  entity: string;
  category: EntityCategory;
  claim: Claim;
}

export interface TraceStats {
  txs_examined: number;
  addresses_checked: number;
  fees: number;
  below_threshold: number;
  peel_steps: number;
  budget_exhausted: boolean;
}

export interface TraceConfig {
  direction: Direction;
  max_hops: number;
  max_txs: number;
  min_amount: number | null;
  check_addresses: boolean;
  stop_confidence: Confidence;
}

/** One output of the seed transaction included in the trace. */
export interface SeedOutput {
  txid: string;
  vout: number;
  address: Address | null;
  amount: number;
  time: number | null;
}

export interface TraceReport {
  direction: Direction;
  seed: string;
  seed_amount: number;
  /** The outputs the trace started from (forward) or whose funding it traced (backward). */
  seed_outputs: SeedOutput[];
  allocation: string;
  min_amount: number;
  config: TraceConfig;
  /** Largest first. leads + stats.fees + stats.below_threshold = seed_amount. */
  leads: Lead[];
  flows: Flow[];
  labels_seen: LabelSeen[];
  stats: TraceStats;
}

export type NodeKind = "address" | "transaction" | "no_address";

export interface NodeLead {
  /** Index into TraceReport.leads (the main lead here: an endpoint, else the largest). */
  index: number;
  kind: LeadKind;
  title: string;
  /** Total of every lead at this node. */
  amount: number;
  confidence: Confidence;
  /** Other leads that end at the same node (the same address reached more than once). */
  others: number[];
}

export interface GraphNode {
  id: string;
  kind: NodeKind;
  address: string | null;
  txid: string | null;
  label: string;
  seed: boolean;
  hop: number;
  entity: string | null;
  category: EntityCategory | null;
  confidence: Confidence | null;
  lead: NodeLead | null;
  amount_in: number;
  amount_out: number;
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  txid: string;
  hop: number;
  time: number | null;
  amount: number;
  role: FlowRole;
  peel: boolean;
  note: string | null;
}

export interface GraphView {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface TraceView {
  report: TraceReport;
  graph: GraphView;
}

/** Progress events emitted by the desktop app during a trace ("trace_progress"). */
export interface TraceProgress {
  txs_examined: number;
  pending: number;
  leads: number;
}

// ---- commands -------------------------------------------------------------------------

/** Every command: name → [request, response]. Same names over Tauri and HTTP. */
export interface Commands {
  status: [Record<string, never>, StatusView];
  resolve: [{ query: string }, ResolveView];
  btc_address: [{ address: string; pages?: number }, AddressView];
  btc_tx: [{ txid: string }, TxView];
  btc_trace: [TraceRequest, TraceView];
  label_lookup: [{ address: string }, LabelView[]];
  label_packs: [Record<string, never>, PackView[]];
  label_add: [AddLabelRequest, LabelView[]];
  update_ofac: [Record<string, never>, OfacUpdateView];
}
