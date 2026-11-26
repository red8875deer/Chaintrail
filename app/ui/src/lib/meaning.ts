// Plain-language meaning of the API's enums: categories, confidence levels, lead kinds
// and claims. Wording follows docs/TRACING.md: an inference or label is never a fact.

import type {
  Claim,
  Confidence,
  EntityCategory,
  FlowRole,
  GraphNode,
  LeadKind,
  StopReason,
} from "../api/types";

export const CATEGORY_NAMES: Record<EntityCategory, string> = {
  exchange: "Exchange",
  mixer: "Mixer",
  bridge: "Bridge",
  dex: "Decentralised exchange (DEX)",
  defi: "DeFi protocol",
  swap_service: "Swap service",
  payment_processor: "Payment processor",
  wallet_service: "Wallet service",
  mining_pool: "Mining pool",
  gambling: "Gambling",
  darknet: "Darknet market",
  scam: "Scam",
  ransomware: "Ransomware",
  hack: "Hack / theft",
  terrorist_financing: "Terrorist financing",
  sanctioned: "Sanctioned",
  individual: "Individual",
  unknown_service: "Unidentified service",
};

/** Categories in the order the label form lists them. */
export const CATEGORY_ORDER: EntityCategory[] = [
  "exchange",
  "swap_service",
  "mixer",
  "bridge",
  "dex",
  "defi",
  "payment_processor",
  "wallet_service",
  "mining_pool",
  "gambling",
  "darknet",
  "scam",
  "ransomware",
  "hack",
  "terrorist_financing",
  "sanctioned",
  "individual",
  "unknown_service",
];

export const CRIME_CATEGORIES: ReadonlySet<EntityCategory> = new Set([
  "scam",
  "ransomware",
  "hack",
  "darknet",
  "terrorist_financing",
]);

export function categoryName(c: EntityCategory | null | undefined): string {
  return c ? CATEGORY_NAMES[c] : "";
}

// ---- confidence -------------------------------------------------------------------------

export const CONFIDENCE_RANK: Record<Confidence, number> = {
  low: 1,
  medium: 2,
  high: 3,
  certain: 4,
};

export const CONFIDENCE_NAMES: Record<Confidence, string> = {
  certain: "Certain",
  high: "High",
  medium: "Medium",
  low: "Low",
};

/** docs/TRACING.md §6. */
export const CONFIDENCE_MEANING: Record<Confidence, string> = {
  certain: "On-chain fact",
  high: "Deterministic protocol match, or several strong signals agree",
  medium: "One strong signal, or several weak ones agree",
  low: "Weak or single weak signal; never followed automatically",
};

// ---- lead kinds -------------------------------------------------------------------------

export function stopReasonText(stop: StopReason): string {
  switch (stop.reason) {
    case "hop_limit":
      return "hop limit reached";
    case "budget":
      return "transaction budget used up";
    case "error":
      return `data error: ${stop.detail}`;
  }
}

export function leadKindName(kind: LeadKind): string {
  switch (kind.kind) {
    case "endpoint":
      return "Endpoint";
    case "at_rest":
      return "Funds at rest";
    case "mined":
      return "Newly mined";
    case "unexplored":
      return "Not followed";
  }
}

export function leadKindExplanation(kind: LeadKind): string {
  switch (kind.kind) {
    case "endpoint":
      return "The trace stopped here because the funds reached a known or detected service.";
    case "at_rest":
      return "The funds have not moved since: they are still in this unspent output.";
    case "mined":
      return "The funds were created by mining (a coinbase transaction).";
    case "unexplored":
      return `The trace did not follow these funds further (${stopReasonText(kind.stop)}). This is a limit of the trace, not an endpoint.`;
  }
}

// ---- flow roles -------------------------------------------------------------------------

export function roleText(role: FlowRole): string {
  switch (role.role) {
    case "payment":
      return "Payment (funds moved to another address)";
    case "source":
      return "Source (funds came from this input)";
    case "change":
      return `Likely change returned to the sender (inferred, ${CONFIDENCE_NAMES[role.confidence].toLowerCase()} confidence)`;
  }
}

// ---- claims -----------------------------------------------------------------------------

const HEURISTIC_NAMES: Record<string, string> = {
  coinjoin: "CoinJoin detection",
  "service.busy_address": "address activity (busy service wallet)",
  "service.tx_shape": "transaction shape (service behaviour)",
  "label.same_wallet": "a label on the same wallet",
};

export function heuristicName(h: string): string {
  if (HEURISTIC_NAMES[h]) return HEURISTIC_NAMES[h];
  if (h.startsWith("change.")) return "change detection";
  return h.replace(/[._]/g, " ");
}

export interface ClaimText {
  /** Short badge text: "On-chain fact", "Inferred", "Label". */
  kind: string;
  /** One sentence in plain words. */
  headline: string;
  evidence: string[];
  url: string | null;
  asOf: string | null;
}

export function describeClaim(claim: Claim): ClaimText {
  switch (claim.kind) {
    case "fact":
      return {
        kind: "On-chain fact",
        headline: "Read directly from the blockchain.",
        evidence: [],
        url: null,
        asOf: null,
      };
    case "inference":
      return {
        kind: "Inferred",
        headline: `Inferred by ${heuristicName(claim.heuristic)}, ${CONFIDENCE_NAMES[claim.confidence].toLowerCase()} confidence. Not proof.`,
        evidence: claim.evidence,
        url: null,
        asOf: null,
      };
    case "attribution":
      return {
        kind: "Label",
        headline: `Label from the “${claim.source}” pack, ${CONFIDENCE_NAMES[claim.confidence].toLowerCase()} confidence. A label is a lead, not proof of ownership.`,
        evidence: [],
        url: claim.url ?? null,
        asOf: claim.as_of,
      };
  }
}

export function claimConfidence(claim: Claim): Confidence {
  return claim.kind === "fact" ? "certain" : claim.confidence;
}

// ---- node meaning (graph styling and legend) --------------------------------------------

export type NodeStyleKey =
  | "seed"
  | "sanctioned"
  | "crime"
  | "mixer"
  | "exchange"
  | "mining"
  | "defi"
  | "service"
  | "unidentified"
  | "at_rest"
  | "unexplored"
  | "transaction"
  | "address";

export interface NodeStyleInfo {
  name: string;
  description: string;
}

export const NODE_STYLES: Record<NodeStyleKey, NodeStyleInfo> = {
  seed: { name: "Start of the trace", description: "The address or output you traced from" },
  sanctioned: { name: "Sanctioned", description: "On a sanctions list (OFAC SDN)" },
  crime: {
    name: "Crime-related",
    description: "Scam, ransomware, hack, darknet market or terrorist financing",
  },
  mixer: { name: "Mixer / CoinJoin", description: "Funds were mixed with other people's" },
  exchange: { name: "Exchange", description: "Custodial exchange" },
  mining: { name: "Mining", description: "Mining pool or newly mined coins" },
  defi: { name: "DeFi, DEX, bridge or swap", description: "Swap or cross-chain service" },
  service: { name: "Other known service", description: "Payment processor, wallet, gambling" },
  unidentified: {
    name: "Unidentified service",
    description: "Detected by behaviour; owner unknown",
  },
  at_rest: { name: "Funds at rest", description: "Unspent: the funds are still here" },
  unexplored: { name: "Not followed", description: "The trace stopped at a limit" },
  transaction: { name: "Transaction", description: "A transaction shown as one point" },
  address: { name: "Address", description: "Unlabelled address the funds passed through" },
};

export function categoryStyle(c: EntityCategory): NodeStyleKey | null {
  if (c === "sanctioned") return "sanctioned";
  if (CRIME_CATEGORIES.has(c)) return "crime";
  if (c === "mixer") return "mixer";
  if (c === "exchange") return "exchange";
  if (c === "mining_pool") return "mining";
  if (c === "defi" || c === "dex" || c === "bridge" || c === "swap_service") return "defi";
  if (c === "payment_processor" || c === "wallet_service" || c === "gambling") return "service";
  if (c === "unknown_service") return "unidentified";
  return null; // individual: shown as a plain address with its label text
}

/** How a graph node is drawn. Seed first, then what it is, then what happened there. */
export function nodeStyleKey(n: GraphNode): NodeStyleKey {
  if (n.seed) return "seed";
  if (n.category) {
    const s = categoryStyle(n.category);
    if (s) return s;
  }
  if (n.lead) {
    switch (n.lead.kind.kind) {
      case "at_rest":
        return "at_rest";
      case "unexplored":
        return "unexplored";
      case "mined":
        return "mining";
      case "endpoint":
        break;
    }
  }
  if (n.kind === "transaction") return "transaction";
  return "address";
}
