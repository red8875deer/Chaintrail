// Mock mode (`?mock=1`): answers from responses recorded from the real service.
// Lets the app be demoed and its UI tested without network access.

import type { Commands } from "./types";
import status from "../../fixtures/status.json";
import resolveAddress from "../../fixtures/resolve_address.json";
import address from "../../fixtures/btc_address.json";
import txCoinjoin from "../../fixtures/btc_tx_coinjoin.json";
import txPayment from "../../fixtures/btc_tx_payment.json";
import traceForward from "../../fixtures/btc_trace_forward.json";
import traceBackward from "../../fixtures/btc_trace_backward.json";
import labelLookup from "../../fixtures/label_lookup.json";
import labelPacks from "../../fixtures/label_packs.json";

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function mockCall<K extends keyof Commands>(
  name: K,
  req: Commands[K][0],
): Promise<unknown> {
  await delay(name === "btc_trace" ? 600 : 120);
  switch (name) {
    case "status":
      return status;
    case "resolve": {
      const q = String((req as { query: string }).query).trim();
      if (/^[0-9a-fA-F]{64}$/.test(q)) return { kind: "transaction", txid: q.toLowerCase() };
      if (q.length > 25) return { ...resolveAddress, address: q };
      throw { kind: "invalid_input", message: "Not a transaction id or a supported address." };
    }
    case "btc_address":
      return address;
    case "btc_tx":
      return (req as { txid: string }).txid.startsWith("272e8b2d") ? txCoinjoin : txPayment;
    case "btc_trace": {
      // Larger recorded traces, loaded only when asked for (demos and browser tests).
      const r = req as { txid: string; direction: string };
      const big: Record<string, () => Promise<{ default: unknown }>> = {
        f4184fc5: () => import("../../fixtures/btc_trace_large.json"),
        "3f036ff8": () => import("../../fixtures/btc_trace_fanin.json"),
        a82d6098: () => import("../../fixtures/btc_trace_chain.json"),
        "628fb3f0": () => import("../../fixtures/btc_trace_alloutputs.json"),
      };
      const load = big[r.txid.slice(0, 8)];
      if (load) return (await load()).default;
      return r.direction === "backward" ? traceBackward : traceForward;
    }
    case "label_lookup":
      return labelLookup;
    case "label_packs":
      return labelPacks;
    case "label_add":
      return labelLookup;
    case "update_ofac":
      return {
        as_of: "2026-11-23",
        addresses: 1027,
        entries: 113,
        unsupported: {},
        reassigned: [],
        invalid: [],
        path: "(mock)",
      };
  }
  throw { kind: "not_found", message: `unknown command ${String(name)}` };
}
