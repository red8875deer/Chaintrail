import { describe, expect, it } from "vitest";
import type { TraceView } from "../api/types";
import forward from "../../fixtures/btc_trace_forward.json";
import backward from "../../fixtures/btc_trace_backward.json";
import { elementsFor, leadPath, smallEdges } from "./graph";
import { leadsCsv } from "./export";

const views: [string, TraceView][] = [
  ["forward", forward as unknown as TraceView],
  ["backward", backward as unknown as TraceView],
];

describe.each(views)("graph elements (%s fixture)", (_, view) => {
  it("every edge connects existing nodes", () => {
    const els = elementsFor(view);
    const ids = new Set(els.filter((e) => e.group === "nodes").map((e) => e.data.id));
    for (const e of els.filter((e) => e.group === "edges")) {
      expect(ids.has(e.data.source as string)).toBe(true);
      expect(ids.has(e.data.target as string)).toBe(true);
    }
  });
  it("every lead is on a node and has a path back to the seed", () => {
    view.report.leads.forEach((lead, i) => {
      const p = leadPath(view, i);
      expect(p.nodes.size).toBeGreaterThan(0);
      if (lead.hops > 0) expect(p.edges.size).toBeGreaterThan(0);
      const seeds = view.graph.nodes.filter((n) => n.seed).map((n) => n.id);
      expect(seeds.some((s) => p.nodes.has(s))).toBe(true);
    });
  });
  it("no node shows more than the traced amount", () => {
    for (const n of view.graph.nodes) {
      expect(n.amount_in).toBeLessThanOrEqual(view.report.seed_amount);
      expect(n.amount_out).toBeLessThanOrEqual(view.report.seed_amount);
    }
  });
  it("accounting balances", () => {
    const r = view.report;
    const leads = r.leads.reduce((s, l) => s + l.amount, 0);
    expect(leads + r.stats.fees + r.stats.below_threshold).toBe(r.seed_amount);
  });
  it("exports one CSV row per lead", () => {
    expect(leadsCsv(view).trim().split("\r\n")).toHaveLength(view.report.leads.length + 1);
  });
});

describe("path highlighting", () => {
  const view = forward as unknown as TraceView;
  it("does not light up sibling branches of the same transaction", () => {
    // The lead at bc1qpx7v… (seizure transfer) shares the 85/15 split transaction with the
    // 11.25 BTC branch to bc1qu57h…, which must not be on its path.
    const i = view.report.leads.findIndex((l) => l.address?.value === "bc1qpx7vyv5tp7dm0g475ev527krg764t73dh77gls");
    expect(i).toBeGreaterThanOrEqual(0);
    const p = leadPath(view, i);
    expect(p.nodes.has("bc1qxu83k5qkj8kcqdqqenwzn7khcw4llfykeqwg45")).toBe(true);
    expect(p.nodes.has("bc1qu57hnxf0c65fsdd5kewcsfeag6sljgfhz99zwt")).toBe(false);
  });
  it("hides small flows by share", () => {
    expect(smallEdges(view, 0).size).toBe(0);
    expect(smallEdges(view, 10_000).size).toBe(view.graph.edges.length);
  });
});
