// Trace graph → what is drawn. Pure functions, unit-tested.
//
// Two layers:
// 1. The trace report's graph: every address and transfer the trace touched (complete).
// 2. The *display* graph built here, which keeps large traces readable:
//    - flows below a share of the traced amount are hidden (with a notice);
//    - many small leaf branches of one parent are grouped into one node;
//    - runs of plain pass-through addresses are collapsed into one node.
//    Groups and chains expand on click. The main route to every lead the user selects is
//    revealed and stays drawn, so the graph does not jump around: its addresses are never
//    hidden or grouped (plain pass-through runs on it stay collapsed, highlighted). The
//    leads panel always lists every lead.
//
// Meaning is encoded by shape AND colour AND border (never colour alone):
// what a node is (category), what happened there (lead kind), and how sure we are
// (dashed border = inferred, solid = fact or label).

import type { ElementDefinition, StylesheetJsonBlock } from "cytoscape";
import type { GraphEdge, GraphNode, TraceView } from "../api/types";
import { formatBtcShort, shareBasisPoints, shortId } from "./format";
import { NODE_STYLES, type NodeStyleKey, nodeStyleKey } from "./meaning";

export interface Palette {
  ink: string;
  muted: string;
  surface: string;
  trail: string;
  amber: string;
  nodes: Record<NodeStyleKey, string>;
}

export const NODE_SHAPES: Record<NodeStyleKey, string> = {
  seed: "diamond",
  sanctioned: "octagon",
  crime: "triangle",
  mixer: "hexagon",
  exchange: "round-rectangle",
  mining: "barrel",
  defi: "round-pentagon",
  service: "round-rectangle",
  unidentified: "round-rectangle",
  at_rest: "ellipse",
  unexplored: "ellipse",
  transaction: "round-tag",
  address: "ellipse",
};

/** Graphs with more original nodes than this start with small flows hidden. */
export const LARGE_GRAPH_NODES = 40;
/** Default "hide flows under" steps for large graphs (basis points of the traced amount):
 * the smallest one that brings the drawn graph down to READABLE_NODES is used. */
export const DEFAULT_MIN_BP_STEPS = [100, 200, 300, 500, 750, 1000];
/** About as many nodes as fit on one screen at a readable zoom. */
export const READABLE_NODES = 40;
/** Leaf siblings of one kind under one parent before they are grouped. */
const GROUP_OVER = 4;
/** Largest leaves kept visible when a group is formed. */
const GROUP_KEEP = 2;
/** Shortest run of pass-through addresses that is collapsed. */
const CHAIN_MIN = 3;
/** Edges below this share get no permanent label in busy graphs (shown on hover). */
const LABEL_MIN_BP = 100;
const BUSY_EDGES = 20;
/** Edges meeting at one node before their labels move to the unshared end. */
const FAN = 3;

function basis(view: TraceView, n: GraphNode): "fact" | "inference" | "attribution" | "none" {
  if (!n.lead) return n.entity ? "attribution" : "none";
  const lead = view.report.leads[n.lead.index];
  return lead ? lead.claim.kind : "none";
}

export function nodeSize(amount: number, seedAmount: number): number {
  const share = seedAmount > 0 ? Math.min(1, amount / seedAmount) : 0;
  return Math.round(24 + 34 * Math.sqrt(share));
}

export function edgeWidth(amount: number, seedAmount: number): number {
  const share = seedAmount > 0 ? Math.min(1, amount / seedAmount) : 0;
  return Math.round((1.25 + 9 * Math.sqrt(share)) * 10) / 10;
}

/** The traced amount at a node: what passed through it, or what ended there if more. A node
 * where part of the funds stopped (e.g. not followed) but more moved on shows the larger. */
export function nodeAmount(n: GraphNode): number {
  return Math.max(n.lead?.amount ?? 0, n.amount_in, n.amount_out);
}

/** Every route by which the traced funds reached a lead from the start: the lead and all
 * its ancestors (forward: what sent funds to it, transitively; backward: what it funded on
 * the way to the start). Siblings of the same transactions are not on it. */
export function leadPath(view: TraceView, leadIndex: number): { nodes: Set<string>; edges: Set<string> } {
  const nodes = new Set<string>();
  const edges = new Set<string>();
  const start = nodeForLead(view, leadIndex);
  if (!view.report.leads[leadIndex] || !start) return { nodes, edges };
  const forward = view.report.direction === "forward";
  // Edges by the node they lead back from.
  const back = new Map<string, GraphEdge[]>();
  for (const e of view.graph.edges) {
    if (e.source === e.target) continue;
    const from = forward ? e.target : e.source;
    (back.get(from) ?? back.set(from, []).get(from)!).push(e);
  }
  const queue = [start.id];
  nodes.add(start.id);
  while (queue.length) {
    const id = queue.pop()!;
    for (const e of back.get(id) ?? []) {
      edges.add(e.id);
      const next = forward ? e.source : e.target;
      if (!nodes.has(next)) {
        nodes.add(next);
        queue.push(next);
      }
    }
  }
  return { nodes, edges };
}

/** The main route from a lead back to the start: at each step the largest transfer on the
 * lead's path. This is what must be drawn to show the lead; other routes are drawn if large
 * enough (tiny side transfers of the same transactions would only add clutter). */
export function leadSpine(view: TraceView, leadIndex: number): { nodes: Set<string>; edges: Set<string> } {
  const path = leadPath(view, leadIndex);
  const nodes = new Set<string>();
  const edges = new Set<string>();
  const start = nodeForLead(view, leadIndex);
  if (!start) return { nodes, edges };
  const forward = view.report.direction === "forward";
  let cur = start.id;
  nodes.add(cur);
  for (;;) {
    const back = view.graph.edges.filter(
      (e) => path.edges.has(e.id) && e.source !== e.target && (forward ? e.target === cur : e.source === cur),
    );
    if (back.length === 0) break;
    const e = back.reduce((a, b) => (b.amount > a.amount ? b : a));
    const next = forward ? e.source : e.target;
    edges.add(e.id);
    if (nodes.has(next)) break;
    nodes.add(next);
    cur = next;
  }
  return { nodes, edges };
}

/** The graph node where a lead ends (several leads can end at one address). */
export function nodeForLead(view: TraceView, leadIndex: number): GraphNode | undefined {
  return view.graph.nodes.find((n) => n.lead && (n.lead.index === leadIndex || n.lead.others.includes(leadIndex)));
}

export function edgeById(view: TraceView, id: string): GraphEdge | undefined {
  return view.graph.edges.find((e) => e.id === id);
}

export function nodeById(view: TraceView, id: string): GraphNode | undefined {
  return view.graph.nodes.find((n) => n.id === id);
}

// ---- display graph ----------------------------------------------------------------------

export interface DisplayOptions {
  /** Hide flows below this share of the traced amount (basis points). */
  minShareBp: number;
  /** Group small leaf branches and collapse plain chains. */
  simplify: boolean;
  /** Group and chain ids the user expanded. */
  expanded: ReadonlySet<string>;
  /** Report node ids that must stay visible and ungrouped (revealed lead routes). */
  keep: ReadonlySet<string>;
  /** Report edge ids drawn whatever their size (revealed lead routes). */
  keepEdges: ReadonlySet<string>;
}

export interface DisplayGroup {
  id: string;
  kind: "group" | "chain";
  /** Report node ids it stands for, largest first. */
  members: string[];
  /** Traced amount entering the group. */
  amount: number;
  /** Chains: traced amount leaving the last address (the rest left in hidden branches or
   * went to fees). Groups: same as `amount`. */
  amountOut: number;
  styleKey: NodeStyleKey;
}

export interface Display {
  elements: ElementDefinition[];
  /** Report node id → displayed node id (itself, a group or a chain). */
  nodeOf: Map<string, string>;
  /** Report edge id → displayed edge id (edges inside a collapsed run map to its line). */
  edgeOf: Map<string, string>;
  /** Displayed edge id → the report edges it actually draws (its own transfers only). */
  edgeMembers: Map<string, string[]>;
  groups: Map<string, DisplayGroup>;
  /** Report nodes not drawn because their flows are below the threshold. */
  hidden: number;
  /** Report nodes not linked to the start by any flow (cannot be drawn at any setting). */
  unlinked: number;
  /** Report nodes drawn individually. */
  shown: number;
  total: number;
}

const isGroupId = (id: string) => id.startsWith("group:") || id.startsWith("chain:");
export { isGroupId };

export function buildDisplay(view: TraceView, o: DisplayOptions): Display {
  const fwd = view.report.direction === "forward";
  const seedAmount = view.report.seed_amount;
  const byId = new Map(view.graph.nodes.map((n) => [n.id, n]));
  const away = (e: GraphEdge) => (fwd ? e.target : e.source);
  const toward = (e: GraphEdge) => (fwd ? e.source : e.target);
  const share = (amount: number) => Number(shareBasisPoints(amount, seedAmount));

  // 1. Hide small flows, then keep what is still connected to the seed.
  const kept = view.graph.edges.filter((e) => share(e.amount) >= o.minShareBp || o.keepEdges.has(e.id));
  const visible = new Set<string>();
  // Start points too: a trace of every output of a large batch has many small ones. Keep
  // those with a large enough flow or amount, and always at least the largest.
  const touched = new Set(kept.flatMap((e) => [e.source, e.target]));
  const seeds = view.graph.nodes.filter((n) => n.seed).sort((a, b) => nodeAmount(b) - nodeAmount(a));
  const queue = seeds
    .filter((n, i) => i === 0 || touched.has(n.id) || share(nodeAmount(n)) >= o.minShareBp || o.keep.has(n.id))
    .map((n) => n.id);
  queue.forEach((id) => visible.add(id));
  o.keep.forEach((id) => visible.add(id));
  queue.push(...o.keep);
  while (queue.length) {
    const id = queue.shift()!;
    for (const e of kept) {
      if (e.source === e.target || toward(e) !== id) continue;
      const next = away(e);
      if (!visible.has(next)) {
        visible.add(next);
        queue.push(next);
      }
    }
  }
  const edges = kept.filter((e) => visible.has(e.source) && visible.has(e.target));
  // Nodes no flow links to the start, whatever the threshold (should not happen; counted
  // apart so the notice never blames the slider for them).
  const linked = new Set(view.graph.nodes.filter((n) => n.seed).map((n) => n.id));
  for (let grew = true; grew; ) {
    grew = false;
    for (const e of view.graph.edges) {
      if (linked.has(toward(e)) && !linked.has(away(e))) {
        linked.add(away(e));
        grew = true;
      }
    }
  }

  // Children (away from the seed) and parents, ignoring self-transfers.
  const children = new Map<string, GraphEdge[]>();
  const parents = new Map<string, GraphEdge[]>();
  for (const e of edges) {
    if (e.source === e.target) continue;
    (children.get(toward(e)) ?? children.set(toward(e), []).get(toward(e))!).push(e);
    (parents.get(away(e)) ?? parents.set(away(e), []).get(away(e))!).push(e);
  }

  const nodeOf = new Map<string, string>();
  const groups = new Map<string, DisplayGroup>();

  if (o.simplify) {
    // 2. Group leaf branches: same parent, same kind, many of them.
    for (const [parent, out] of children) {
      const leaves = new Map<NodeStyleKey, GraphEdge[]>();
      for (const e of out) {
        const c = away(e);
        const n = byId.get(c);
        if (!n || n.seed || o.keep.has(c) || children.has(c) || (parents.get(c)?.length ?? 0) !== 1) continue;
        const key = nodeStyleKey(n);
        (leaves.get(key) ?? leaves.set(key, []).get(key)!).push(e);
      }
      for (const [key, list] of leaves) {
        if (list.length <= GROUP_OVER) continue;
        const id = `group:${parent}:${key}`;
        if (o.expanded.has(id)) continue;
        list.sort((a, b) => b.amount - a.amount);
        const members = list.slice(GROUP_KEEP);
        groups.set(id, {
          id,
          kind: "group",
          members: members.map(away),
          amount: members.reduce((s, e) => s + e.amount, 0),
          amountOut: members.reduce((s, e) => s + e.amount, 0),
          styleKey: key,
        });
        members.forEach((e) => nodeOf.set(away(e), id));
      }
    }

    // 3. Collapse runs of plain pass-through addresses.
    const plain = (id: string) => {
      const n = byId.get(id);
      return (
        !!n &&
        !n.seed &&
        !n.lead &&
        !n.entity &&
        n.kind === "address" &&
        !nodeOf.has(id) &&
        (parents.get(id)?.length ?? 0) === 1 &&
        (children.get(id)?.length ?? 0) === 1
      );
    };
    for (const id of visible) {
      if (!plain(id)) continue;
      const up = toward(parents.get(id)![0]!);
      if (plain(up)) continue; // not the start of a run
      const run = [id];
      let cur = id;
      for (;;) {
        const next = away(children.get(cur)![0]!);
        if (!plain(next)) break;
        run.push(next);
        cur = next;
      }
      if (run.length < CHAIN_MIN) continue;
      const cid = `chain:${run[0]}`;
      if (o.expanded.has(cid)) continue;
      groups.set(cid, {
        id: cid,
        kind: "chain",
        members: run,
        amount: parents.get(run[0]!)![0]!.amount,
        amountOut: children.get(run[run.length - 1]!)![0]!.amount,
        styleKey: "address",
      });
      run.forEach((m) => nodeOf.set(m, cid));
    }
  }

  // 4. Emit nodes.
  const elements: ElementDefinition[] = [];
  let shown = 0;
  for (const id of visible) {
    if (nodeOf.has(id)) continue;
    nodeOf.set(id, id);
    const n = byId.get(id)!;
    const styleKey = nodeStyleKey(n);
    const b = basis(view, n);
    // The traced transaction (all-outputs traces) stands for the whole traced amount,
    // including outputs too small to follow.
    const amount = n.seed && n.kind === "transaction" ? seedAmount : nodeAmount(n);
    shown++;
    elements.push({
      group: "nodes",
      data: {
        id,
        // Endpoints often share a name ("Likely service"): a short id tells them apart.
        label: n.entity
          ? `${n.label}\n${shortId(n.address ?? n.txid ?? id, 6, 4)}\n${formatBtcShort(amount)} BTC`
          : `${n.label}\n${formatBtcShort(amount)} BTC`,
        size: nodeSize(amount, seedAmount),
        isLead: n.lead ? 1 : 0,
      },
      classes: [styleKey, n.lead ? "lead" : "", b].filter(Boolean).join(" "),
    });
  }
  for (const g of groups.values()) {
    const label =
      g.kind === "chain"
        ? `${g.members.length} addresses in a row`
        : `${g.members.length} more: ${NODE_STYLES[g.styleKey].name.toLowerCase()}`;
    elements.push({
      group: "nodes",
      data: {
        id: g.id,
        label: `${label}\n${formatBtcShort(g.amount)} BTC`,
        size: nodeSize(g.amount, seedAmount),
        isLead: 0,
      },
      classes: [g.styleKey, g.kind].join(" "),
    });
  }

  // 5. Emit edges. Transfers between the same two drawn nodes (several transactions, or
  //    members of a group or chain) are drawn as one combined edge with their total.
  const edgeOf = new Map<string, string>();
  const merged = new Map<string, { source: string; target: string; amount: number; members: GraphEdge[] }>();
  for (const e of edges) {
    const s = nodeOf.get(e.source)!;
    const t = nodeOf.get(e.target)!;
    if (s === t && isGroupId(s)) {
      // Inside a chain: highlight maps to the chain node's incoming edge (set below).
      continue;
    }
    const key = `${s}|${t}`;
    const m = merged.get(key) ?? merged.set(key, { source: s, target: t, amount: 0, members: [] }).get(key)!;
    // An address paying itself again and again moves the same coins each time: the total
    // is the largest single move, not the sum.
    m.amount = s === t ? Math.max(m.amount, e.amount) : m.amount + e.amount;
    m.members.push(e);
  }
  const busy = merged.size > BUSY_EDGES;
  const edgeMembers = new Map<string, string[]>();
  // Where several edges meet at one node, their amounts would pile up there: put the label
  // at the other, unshared end (by each source for a fan-in, by each target for a fan-out).
  const inDeg = new Map<string, number>();
  const outDeg = new Map<string, number>();
  for (const m of merged.values()) {
    if (m.source === m.target) continue;
    inDeg.set(m.target, (inDeg.get(m.target) ?? 0) + 1);
    outDeg.set(m.source, (outDeg.get(m.source) ?? 0) + 1);
  }
  for (const [key, m] of merged) {
    const first = m.members[0]!;
    const single = m.members.length === 1 && !isGroupId(m.source) && !isGroupId(m.target);
    const id = single ? first.id : `agg|${key}`;
    m.members.forEach((e) => edgeOf.set(e.id, id));
    edgeMembers.set(
      id,
      m.members.map((e) => e.id),
    );
    const bp = share(m.amount);
    const roles = new Set(m.members.map((e) => e.role.role));
    const peel = m.members.every((e) => e.peel);
    const loop = m.source === m.target;
    const minor = busy && bp < LABEL_MIN_BP;
    const fanIn = (inDeg.get(m.target) ?? 0) >= FAN && (outDeg.get(m.source) ?? 0) === 1;
    const fanOut = (outDeg.get(m.source) ?? 0) >= FAN && (inDeg.get(m.target) ?? 0) === 1;
    const width = loop ? Math.min(edgeWidth(m.amount, seedAmount), 3) : edgeWidth(m.amount, seedAmount);
    elements.push({
      group: "edges",
      data: {
        id,
        source: m.source,
        target: m.target,
        label:
          formatBtcShort(m.amount) +

          (m.members.length > 1 ? (loop ? `, to itself ${m.members.length} times` : ` ×${m.members.length}`) : ""),
        width,
        // Keep the amount clear of the line and its arrow, however thick the line is.
        labelDy: -Math.round(6 + 1.6 * width),
        weight: 1 + Math.round(bp / 100),
        amount: m.amount,
        count: m.members.length,
      },
      classes: [
        roles.size === 1 ? first.role.role : "payment",
        peel ? "peel" : "",
        loop ? "loop" : "",
        minor ? "minor" : "",
        !loop && fanIn ? "fan-in" : "",
        !loop && fanOut ? "fan-out" : "",
        !minor && !loop && fanIn ? "lbl-src" : "",
        !minor && !loop && fanOut ? "lbl-tgt" : "",
      ]
        .filter(Boolean)
        .join(" "),
    });
  }
  // Edges inside a chain highlight as the chain's incoming edge.
  for (const e of edges) {
    if (edgeOf.has(e.id)) continue;
    const c = nodeOf.get(e.source)!;
    const incoming = [...merged.values()].find((m) => m.target === c || m.source === c);
    if (incoming) edgeOf.set(e.id, edgeOf.get(incoming.members[0]!.id)!);
  }

  return {
    elements,
    nodeOf,
    edgeOf,
    edgeMembers,
    groups,
    hidden: view.graph.nodes.filter((n) => !visible.has(n.id) && linked.has(n.id)).length,
    unlinked: view.graph.nodes.filter((n) => !visible.has(n.id) && !linked.has(n.id)).length,
    shown,
    total: view.graph.nodes.length,
  };
}

/** Report path → displayed ids. */
export function displayPath(d: Display, path: { nodes: Set<string>; edges: Set<string> }) {
  const nodes = new Set<string>();
  const edges = new Set<string>();
  path.nodes.forEach((id) => {
    const x = d.nodeOf.get(id);
    if (x) nodes.add(x);
  });
  path.edges.forEach((id) => {
    const x = d.edgeOf.get(id);
    if (x) edges.add(x);
  });
  return { nodes, edges };
}

/** Default display threshold for a trace: nothing hidden in small graphs; in large ones the
 * smallest step that keeps the simplified graph readable (the user can lower it). */
export function defaultMinShareBp(view: TraceView): number {
  if (view.graph.nodes.length <= LARGE_GRAPH_NODES) return 0;
  const drawn = (bp: number) =>
    buildDisplay(view, { minShareBp: bp, simplify: true, expanded: new Set(), keep: new Set(), keepEdges: new Set() }).elements.filter(
      (e) => e.group === "nodes",
    ).length;
  return DEFAULT_MIN_BP_STEPS.find((bp) => drawn(bp) <= READABLE_NODES) ?? DEFAULT_MIN_BP_STEPS.at(-1)!;
}

// ---- style ------------------------------------------------------------------------------

export function graphStyle(p: Palette): StylesheetJsonBlock[] {
  const nodeColor = (k: NodeStyleKey) => ({
    selector: `node.${k}`,
    style: { "background-color": p.nodes[k], shape: NODE_SHAPES[k] as never },
  });
  return [
    {
      selector: "node",
      style: {
        width: "data(size)",
        height: "data(size)",
        label: "data(label)",
        "font-family": "Atkinson Hyperlegible Next, system-ui, sans-serif",
        "font-size": 13,
        "min-zoomed-font-size": 7,
        color: p.ink,
        "text-wrap": "wrap",
        // Narrow labels: wide ones reach under neighbouring edges.
        "text-max-width": "112px",
        "text-valign": "bottom",
        "text-margin-y": 6,
        "text-background-color": p.surface,
        "text-background-opacity": 0.9,
        "text-background-padding": "2px",
        "text-background-shape": "roundrectangle",
        "border-width": 2,
        "border-color": p.surface,
        "transition-property": "opacity, border-color, border-width",
        "transition-duration": 150,
      },
    },
    ...(Object.keys(NODE_SHAPES) as NodeStyleKey[]).map(nodeColor),
    // Inferred identifications get a dashed outline; facts and labels a solid one.
    { selector: "node.lead", style: { "border-width": 3, "border-color": p.ink } },
    { selector: "node.inference", style: { "border-style": "dashed" } },
    {
      selector: "node.at_rest",
      style: { "border-style": "double", "border-width": 5, "border-color": p.nodes.at_rest, "background-opacity": 0.35 },
    },
    { selector: "node.unexplored", style: { "border-style": "dotted", "background-opacity": 0.35, "border-color": p.muted } },
    { selector: "node.seed", style: { "border-width": 3, "border-color": p.amber } },
    // Long names (chains, transactions) may use a wider label.
    { selector: "node.chain, node.transaction, node.group", style: { "text-max-width": "190px" } },
    // Groups look stacked (double outline); chains are pills. Both expand on click.
    { selector: "node.group", style: { "border-style": "double", "border-width": 6, "border-color": p.muted } },
    {
      selector: "node.chain",
      style: {
        shape: "round-rectangle",
        width: 54,
        height: 20,
        "background-color": p.surface,
        "border-width": 2,
        "border-style": "dashed",
        "border-color": p.trail,
      },
    },
    {
      selector: "edge",
      style: {
        width: "data(width)",
        "line-color": p.trail,
        // Arrows mid-edge: they never hide under a node's label or pile up where many
        // edges meet, and the flow direction (left to right) stays clear.
        "target-arrow-shape": "none",
        "mid-target-arrow-shape": "triangle",
        "mid-target-arrow-color": p.trail,
        "arrow-scale": 0.9,
        "curve-style": "bezier",
        "control-point-step-size": 36,
        opacity: 0.8,
        label: "data(label)",
        "font-family": "Atkinson Hyperlegible Mono, ui-monospace, monospace",
        "font-size": 12,
        "min-zoomed-font-size": 7,
        color: p.ink,
        "text-rotation": "autorotate",
        "text-margin-y": "data(labelDy)" as never,
        "source-text-rotation": "autorotate",
        "source-text-offset": 84,
        "source-text-margin-y": "data(labelDy)" as never,
        "target-text-rotation": "autorotate",
        "target-text-offset": 84,
        "target-text-margin-y": "data(labelDy)" as never,
        "text-background-color": p.surface,
        "text-background-opacity": 0.92,
        "text-background-padding": "2px",
        "text-background-shape": "roundrectangle",
        "transition-property": "opacity, line-color, width",
        "transition-duration": 150,
      },
    },
    // Fans (3+ edges meeting at one node) are drawn as a comb: one trunk and a straight stub
    // per branch, so no line runs diagonally through the other branches' labels. The amount
    // sits on each branch's own stub (by each source of a fan-in, each target of a fan-out).
    {
      selector: "edge.fan-in, edge.fan-out",
      style: {
        "curve-style": "taxi",
        "taxi-direction": "horizontal",
        "taxi-turn-min-distance": 4,
        "target-arrow-shape": "triangle",
        "target-arrow-color": p.trail,
        "mid-target-arrow-shape": "none",
      },
    },
    { selector: "edge.fan-in", style: { "taxi-turn": "112px" } },
    { selector: "edge.fan-out", style: { "taxi-turn": "-112px" } },
    {
      selector: "edge.lbl-src",
      style: { label: "", "source-label": "data(label)", "source-text-offset": 50, "source-text-rotation": "none" },
    },
    {
      selector: "edge.lbl-tgt",
      style: { label: "", "target-label": "data(label)", "target-text-offset": 56, "target-text-rotation": "none" },
    },
    { selector: "edge.change", style: { "line-style": "dashed", "line-dash-pattern": [7, 4] } },
    // Self-transfers loop over the top: arrows arrive from the left, labels sit below.
    { selector: "edge.loop", style: { label: "", "loop-direction": "0deg", "loop-sweep": "-50deg", "text-margin-y": -14 } },
    { selector: "edge.loop.hover, edge.loop:selected", style: { label: "data(label)" } },
    { selector: "edge.minor", style: { label: "" } },
    // Amounts on the selected lead's route are always shown.
    { selector: "edge.minor.hover, edge.minor.path, edge.minor:selected", style: { label: "data(label)" } },
    {
      selector: "edge.minor.fan-in.hover, edge.minor.fan-in.path, edge.minor.fan-in:selected",
      style: { label: "", "source-label": "data(label)", "source-text-offset": 50, "source-text-rotation": "none" },
    },
    {
      selector: "edge.minor.fan-out.hover, edge.minor.fan-out.path, edge.minor.fan-out:selected",
      style: { label: "", "target-label": "data(label)", "target-text-offset": 56, "target-text-rotation": "none" },
    },
    { selector: ".dim", style: { opacity: 0.22 } },
    {
      selector: "edge.path",
      style: {
        "line-color": p.amber,
        "mid-target-arrow-color": p.amber,
        "target-arrow-color": p.amber,
        opacity: 1,
        "z-index": 10,
      },
    },
    { selector: "node.path", style: { "border-color": p.amber, "border-width": 4, opacity: 1, "z-index": 10 } },
    { selector: "edge.hover", style: { opacity: 1, "z-index": 20 } },
    {
      selector: ":selected",
      style: { "overlay-color": p.amber, "overlay-opacity": 0.18, "overlay-padding": 6 },
    },
  ];
}
