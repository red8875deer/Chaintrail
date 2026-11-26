import { describe, expect, it } from "vitest";
import type { TraceView } from "../api/types";
import forward from "../../fixtures/btc_trace_forward.json";
import backward from "../../fixtures/btc_trace_backward.json";
import large from "../../fixtures/btc_trace_large.json";
import fanin from "../../fixtures/btc_trace_fanin.json";
import chain from "../../fixtures/btc_trace_chain.json";
import alloutputs from "../../fixtures/btc_trace_alloutputs.json";
import { type DisplayOptions, buildDisplay, defaultMinShareBp, displayPath, leadPath, leadSpine, nodeForLead } from "./graph";
import { leadsCsv } from "./export";

const views: [string, TraceView][] = [
  ["forward", forward as unknown as TraceView],
  ["backward", backward as unknown as TraceView],
  ["large fan-out (270 nodes)", large as unknown as TraceView],
  ["Silk Road fan-in (58 nodes)", fanin as unknown as TraceView],
  ["long chain (125 nodes)", chain as unknown as TraceView],
  ["all outputs of a batch payout", alloutputs as unknown as TraceView],
];

const defaults = (v: TraceView, keep = new Set<string>(), keepEdges = new Set<string>()): DisplayOptions => ({
  minShareBp: defaultMinShareBp(v),
  simplify: true,
  expanded: new Set(),
  keep,
  keepEdges,
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
    expect(d.nodeOf.size + d.hidden + d.unlinked).toBe(d.total);
  });

  it("links every report node to the start", () => {
    expect(d.unlinked).toBe(0);
  });

  it("stays small enough to read", () => {
    expect(ids(d, "nodes").length).toBeLessThanOrEqual(45);
  });

  it("every lead, when selected, is drawn on its own with its main route", () => {
    view.report.leads.forEach((lead, i) => {
      const spine = leadSpine(view, i);
      const dd = buildDisplay(view, defaults(view, spine.nodes, spine.edges));
      const node = nodeForLead(view, i)!;
      expect(node, `lead ${i} has a node`).toBeDefined();
      // The lead is drawn on its own; every address on its main route is drawn (plain
      // pass-through runs may stay collapsed)…
      expect(dd.nodeOf.get(node.id)).toBe(node.id);
      spine.nodes.forEach((id) => {
        const shown = dd.nodeOf.get(id);
        expect(shown, `lead ${i} ${lead.title}: ${id}`).toBeDefined();
        if (shown !== id) expect(dd.groups.get(shown!)?.kind).toBe("chain");
      });
      // …joined by drawn edges, back to the start.
      const drawnEdges = new Set(ids(dd, "edges"));
      spine.edges.forEach((e) => expect(drawnEdges.has(dd.edgeOf.get(e)!)).toBe(true));
      if (lead.hops > 0) expect(spine.edges.size).toBeGreaterThan(0);
      const p = displayPath(dd, leadPath(view, i));
      p.edges.forEach((e) => expect(drawnEdges.has(e)).toBe(true));
    });
  });

  it("revealing a route adds no other small transfers", () => {
    const i = view.report.leads.length - 1;
    const spine = leadSpine(view, i);
    const base = buildDisplay(view, defaults(view));
    const dd = buildDisplay(view, defaults(view, spine.nodes, spine.edges));
    const minBp = defaultMinShareBp(view);
    for (const e of view.graph.edges) {
      const small = (e.amount * 10000) / view.report.seed_amount < minBp;
      if (small && !spine.edges.has(e.id)) {
        expect(dd.edgeOf.has(e.id), `small edge ${e.id}`).toBe(base.edgeOf.has(e.id));
      }
    }
  });

  it("showing everything draws every node", () => {
    const all = buildDisplay(view, { minShareBp: 0, simplify: false, expanded: new Set(), keep: new Set(), keepEdges: new Set() });
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

describe("all-outputs traces", () => {
  const view = alloutputs as unknown as TraceView;
  it("start from the traced transaction, and draw the largest leads by default", () => {
    const seeds = view.graph.nodes.filter((n) => n.seed);
    expect(seeds).toHaveLength(1);
    expect(seeds[0]!.kind).toBe("transaction");
    const d = buildDisplay(view, defaults(view));
    for (let i = 0; i < 4; i++) {
      const n = nodeForLead(view, i)!;
      expect(d.nodeOf.get(n.id), `lead ${i} ${view.report.leads[i]!.title}`).toBeDefined();
    }
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
