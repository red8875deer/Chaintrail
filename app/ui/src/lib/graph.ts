// Trace graph → what is drawn. Pure functions, unit-tested.
//
// Two layers:
// 1. The trace report's graph: every address and transfer the trace touched (complete).
// 2. The *display* graph built here, which keeps large traces readable:
//    - flows below a share of the traced amount are hidden (with a notice);
//    - many small leaf branches of one parent are grouped into one node;
//    - runs of plain pass-through addresses are collapsed into one node.
//    Groups and chains expand on click. Nothing on the selected lead's path is ever hidden
//    or grouped, and the leads panel always lists every lead.
//
// Meaning is encoded by shape AND colour AND border (never colour alone):
// what a node is (category), what happened there (lead kind), and how sure we are
// (dashed border = inferred, solid = fact or label).

import type { ElementDefinition, StylesheetJsonBlock } from "cytoscape";
import type { GraphEdge, GraphNode, TraceView } from "../api/types";
import { formatBtcShort, shareBasisPoints } from "./format";
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

export function nodeAmount(n: GraphNode): number {
  return n.lead?.amount ?? Math.max(n.amount_in, n.amount_out);
}

/** Edges and nodes on a lead's path, as ids of the report's graph. Walks from the lead
 * towards the seed using only edges whose transaction is on the lead's path, so sibling
 * branches of the same transaction are not included. */
export function leadPath(view: TraceView, leadIndex: number): { nodes: Set<string>; edges: Set<string> } {
  const nodes = new Set<string>();
  const edges = new Set<string>();
  const lead = view.report.leads[leadIndex];
  const start = nodeForLead(view, leadIndex);
  if (!lead || !start) return { nodes, edges };
  const pathTx = new Set(lead.path);
  const forward = view.report.direction === "forward";
  const queue = [start.id];
  nodes.add(start.id);
  while (queue.length) {
    const id = queue.shift()!;
    for (const e of view.graph.edges) {
      if (!pathTx.has(e.txid) || edges.has(e.id)) continue;
      // Forward: walk back along incoming edges. Backward: walk along outgoing edges.
      const next = forward ? (e.target === id ? e.source : null) : e.source === id ? e.target : null;
      if (next === null) continue;
      edges.add(e.id);
      if (!nodes.has(next)) {
        nodes.add(next);
        queue.push(next);
      }
    }
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
  /** Report node ids that must stay visible and ungrouped (the selected lead's path). */
  keep: ReadonlySet<string>;
}

export interface DisplayGroup {
  id: string;
  kind: "group" | "chain";
  /** Report node ids it stands for, largest first. */
  members: string[];
  /** Traced amount entering the group. */
  amount: number;
  styleKey: NodeStyleKey;
}

export interface Display {
  elements: ElementDefinition[];
  /** Report node id → displayed node id (itself, a group or a chain). */
  nodeOf: Map<string, string>;
  /** Report edge id → displayed edge id. */
  edgeOf: Map<string, string>;
  groups: Map<string, DisplayGroup>;
  /** Report nodes not drawn because their flows are below the threshold. */
  hidden: number;
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
  const kept = view.graph.edges.filter(
    (e) => share(e.amount) >= o.minShareBp || (o.keep.has(e.source) && o.keep.has(e.target)),
  );
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
    shown++;
    elements.push({
      group: "nodes",
      data: {
        id,
        label: `${n.label}\n${formatBtcShort(nodeAmount(n))} BTC`,
        size: nodeSize(nodeAmount(n), seedAmount),
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
    m.amount += e.amount;
    m.members.push(e);
  }
  const busy = merged.size > BUSY_EDGES;
  for (const [key, m] of merged) {
    const first = m.members[0]!;
    const single = m.members.length === 1 && !isGroupId(m.source) && !isGroupId(m.target);
    const id = single ? first.id : `agg|${key}`;
    m.members.forEach((e) => edgeOf.set(e.id, id));
    const bp = share(m.amount);
    const roles = new Set(m.members.map((e) => e.role.role));
    const peel = m.members.every((e) => e.peel);
    elements.push({
      group: "edges",
      data: {
        id,
        source: m.source,
        target: m.target,
        label: formatBtcShort(m.amount) + (peel ? " (peel)" : "") + (m.members.length > 1 ? ` ×${m.members.length}` : ""),
        width: edgeWidth(m.amount, seedAmount),
        weight: 1 + Math.round(bp / 100),
        amount: m.amount,
        count: m.members.length,
      },
      classes: [
        roles.size === 1 ? first.role.role : "payment",
        peel ? "peel" : "",
        m.source === m.target ? "loop" : "",
        busy && bp < LABEL_MIN_BP ? "minor" : "",
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
    groups,
    hidden: view.graph.nodes.length - visible.size,
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
    buildDisplay(view, { minShareBp: bp, simplify: true, expanded: new Set(), keep: new Set() }).elements.filter(
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
        "font-size": 12,
        "min-zoomed-font-size": 7,
        color: p.ink,
        "text-wrap": "wrap",
        "text-max-width": "140px",
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
        "target-arrow-color": p.trail,
        "target-arrow-shape": "triangle",
        "arrow-scale": 0.8,
        "curve-style": "bezier",
        "control-point-step-size": 36,
        opacity: 0.8,
        label: "data(label)",
        "font-family": "Atkinson Hyperlegible Mono, ui-monospace, monospace",
        "font-size": 11,
        "min-zoomed-font-size": 7,
        color: p.ink,
        "text-rotation": "autorotate",
        "text-margin-y": -9,
        "text-background-color": p.surface,
        "text-background-opacity": 0.92,
        "text-background-padding": "2px",
        "text-background-shape": "roundrectangle",
        "transition-property": "opacity, line-color, width",
        "transition-duration": 150,
      },
    },
    { selector: "edge.change", style: { "line-style": "dashed", "line-dash-pattern": [7, 4] } },
    // Self-transfers loop over the top: arrows arrive from the left, labels sit below.
    { selector: "edge.loop", style: { label: "", "loop-direction": "0deg", "loop-sweep": "-50deg" } },
    { selector: "edge.minor", style: { label: "" } },
    { selector: "edge.minor.hover, edge.minor:selected", style: { label: "data(label)" } },
    { selector: ".dim", style: { opacity: 0.12 } },
    {
      selector: "edge.path",
      style: { "line-color": p.amber, "target-arrow-color": p.amber, opacity: 1, "z-index": 10 },
    },
    { selector: "node.path", style: { "border-color": p.amber, "border-width": 4, opacity: 1, "z-index": 10 } },
    { selector: "edge.hover", style: { opacity: 1, "z-index": 20 } },
    {
      selector: ":selected",
      style: { "overlay-color": p.amber, "overlay-opacity": 0.18, "overlay-padding": 6 },
    },
  ];
}
