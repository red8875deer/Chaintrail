import { describe, expect, it } from "vitest";
import type { TraceView } from "../api/types";
import forward from "../../fixtures/btc_trace_forward.json";
import backward from "../../fixtures/btc_trace_backward.json";
import large from "../../fixtures/btc_trace_large.json";
import fanin from "../../fixtures/btc_trace_fanin.json";
import chain from "../../fixtures/btc_trace_chain.json";
import { type DisplayOptions, buildDisplay, defaultMinShareBp, displayPath, leadPath, nodeForLead } from "./graph";
import { leadsCsv } from "./export";

const views: [string, TraceView][] = [
  ["forward", forward as unknown as TraceView],
  ["backward", backward as unknown as TraceView],
  ["large fan-out (270 nodes)", large as unknown as TraceView],
  ["Silk Road fan-in (58 nodes)", fanin as unknown as TraceView],
  ["long chain (125 nodes)", chain as unknown as TraceView],
];

const defaults = (v: TraceView, keep = new Set<string>()): DisplayOptions => ({
  minShareBp: defaultMinShareBp(v),
  simplify: true,
  expanded: new Set(),
  keep,
});

const ids = (d: ReturnType<typeof buildDisplay>, group: "nodes" | "edges") =>
  d.elements.filter((e) => e.group === group).map((e) => String(e.data.id));

describe.each(views)("display graph: %s", (_, view) => {
  const d = buildDisplay(view, defaults(view));

  it("every drawn edge connects drawn nodes", () => {
    const nodes = new Set(ids(d, "nodes"));
    for (const e of d.elements.filter((e) => e.group === "edges")) {
      expect(nodes.has(String(e.data.source))).toBe(true);
      expect(nodes.has(String(e.data.target))).toBe(true);
    }
  });

  it("accounts for every report node (drawn, grouped or hidden)", () => {
    expect(d.nodeOf.size + d.hidden).toBe(d.total);
  });

  it("stays small enough to read", () => {
    expect(ids(d, "nodes").length).toBeLessThanOrEqual(45);
  });

  it("every lead, when selected, is drawn on its own with its path", () => {
    view.report.leads.forEach((lead, i) => {
      const path = leadPath(view, i);
      const dd = buildDisplay(view, defaults(view, path.nodes));
      const node = nodeForLead(view, i)!;
      expect(node, `lead ${i} has a node`).toBeDefined();
      const drawn = dd.nodeOf.get(node.id);
      expect(drawn, `lead ${i} ${lead.title}`).toBeDefined();
      const p = displayPath(dd, path);
      expect(p.nodes.size).toBeGreaterThan(0);
      if (lead.hops > 0) expect(p.edges.size).toBeGreaterThan(0);
      const drawnEdges = new Set(ids(dd, "edges"));
      p.edges.forEach((e) => expect(drawnEdges.has(e)).toBe(true));
    });
  });

  it("showing everything draws every node", () => {
    const all = buildDisplay(view, { minShareBp: 0, simplify: false, expanded: new Set(), keep: new Set() });
    expect(ids(all, "nodes").length).toBe(view.graph.nodes.length);
    expect(all.groups.size).toBe(0);
  });

  it("accounting balances and exports one CSV row per lead", () => {
    const r = view.report;
    const leads = r.leads.reduce((s, l) => s + l.amount, 0);
    expect(leads + r.stats.fees + r.stats.below_threshold).toBe(r.seed_amount);
    expect(leadsCsv(view).trim().split("\r\n")).toHaveLength(r.leads.length + 1);
  });
});

describe("grouping and chains", () => {
  it("groups the Silk Road wallet sweeps and opens a group on request", () => {
    const view = fanin as unknown as TraceView;
    const d = buildDisplay(view, defaults(view));
    const groups = [...d.groups.values()].filter((g) => g.kind === "group");
    expect(groups.length).toBeGreaterThan(0);
    const g = groups.sort((a, b) => b.members.length - a.members.length)[0]!;
    const opened = buildDisplay(view, { ...defaults(view), expanded: new Set([g.id]) });
    for (const m of g.members) expect(opened.nodeOf.get(m)).toBe(m);
  });

  it("collapses the long single-address chain", () => {
    const view = chain as unknown as TraceView;
    const d = buildDisplay(view, defaults(view));
    const chains = [...d.groups.values()].filter((g) => g.kind === "chain");
    expect(chains.length).toBeGreaterThan(0);
    expect(Math.max(...chains.map((c) => c.members.length))).toBeGreaterThanOrEqual(10);
  });

  it("does not light up sibling branches of the same transaction", () => {
    const view = forward as unknown as TraceView;
    const i = view.report.leads.findIndex((l) => l.address?.value === "bc1qpx7vyv5tp7dm0g475ev527krg764t73dh77gls");
    expect(i).toBeGreaterThanOrEqual(0);
    const p = leadPath(view, i);
    expect(p.nodes.has("bc1qxu83k5qkj8kcqdqqenwzn7khcw4llfykeqwg45")).toBe(true);
    expect(p.nodes.has("bc1qu57hnxf0c65fsdd5kewcsfeag6sljgfhz99zwt")).toBe(false);
  });
});
