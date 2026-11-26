// Trace graph → Cytoscape elements and styles. Pure functions, unit-tested.
//
// Meaning is encoded by shape AND colour AND border (never colour alone):
// what a node is (category), what happened there (lead kind), and how sure we are
// (dashed border = inferred, solid = fact or label).

import type { ElementDefinition, StylesheetJsonBlock } from "cytoscape";
import type { GraphEdge, GraphNode, TraceView } from "../api/types";
import { formatBtcShort, shareBasisPoints } from "./format";
import { type NodeStyleKey, nodeStyleKey } from "./meaning";

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

/** How a node's identification was reached, from its lead's claim. */
function basis(view: TraceView, n: GraphNode): "fact" | "inference" | "attribution" | "none" {
  if (!n.lead) return n.entity ? "attribution" : "none";
  const lead = view.report.leads[n.lead.index];
  return lead ? lead.claim.kind : "none";
}

export function nodeSize(n: GraphNode, seedAmount: number): number {
  const amount = Math.max(n.amount_in, n.amount_out, n.lead?.amount ?? 0);
  const share = seedAmount > 0 ? Math.min(1, amount / seedAmount) : 0;
  return Math.round(26 + 34 * Math.sqrt(share));
}

export function edgeWidth(amount: number, seedAmount: number): number {
  const share = seedAmount > 0 ? Math.min(1, amount / seedAmount) : 0;
  return Math.round((1.25 + 9 * Math.sqrt(share)) * 10) / 10;
}

export function nodeAmount(n: GraphNode): number {
  return n.lead?.amount ?? Math.max(n.amount_in, n.amount_out);
}

export function elementsFor(view: TraceView): ElementDefinition[] {
  const seed = view.report.seed_amount;
  const nodes: ElementDefinition[] = view.graph.nodes.map((n) => {
    const styleKey = nodeStyleKey(n);
    return {
      group: "nodes",
      data: {
        id: n.id,
        label: `${n.label}\n${formatBtcShort(nodeAmount(n))} BTC`,
        styleKey,
        basis: basis(view, n),
        size: nodeSize(n, seed),
        isLead: n.lead ? 1 : 0,
      },
      classes: [styleKey, n.lead ? "lead" : "", basis(view, n)].filter(Boolean).join(" "),
    };
  });
  const edges: ElementDefinition[] = view.graph.edges.map((e) => ({
    group: "edges",
    data: {
      id: e.id,
      source: e.source,
      target: e.target,
      label: formatBtcShort(e.amount) + (e.peel ? " (peel)" : ""),
      width: edgeWidth(e.amount, seed),
      shareBp: Number(shareBasisPoints(e.amount, seed)),
      role: e.role.role,
    },
    // An address paying itself (e.g. change to the same address) is drawn as a loop
    // without a label, to keep the picture readable.
    classes: [e.role.role, e.peel ? "peel" : "", e.source === e.target ? "loop" : ""]
      .filter(Boolean)
      .join(" "),
  }));
  return [...nodes, ...edges];
}

/**
 * Edges and nodes on a lead's path. Walks from the lead node towards the seed using only
 * edges whose transaction is on the lead's path, so sibling branches of the same
 * transaction are not included.
 */
export function leadPath(view: TraceView, leadIndex: number): { nodes: Set<string>; edges: Set<string> } {
  const nodes = new Set<string>();
  const edges = new Set<string>();
  const lead = view.report.leads[leadIndex];
  const start = view.graph.nodes.find((n) => n.lead?.index === leadIndex);
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

/** Edges hidden by the "hide small flows" control, by share of the traced amount. */
export function smallEdges(view: TraceView, minBp: number): Set<string> {
  const out = new Set<string>();
  if (minBp <= 0) return out;
  for (const e of view.graph.edges) {
    if (shareBasisPoints(e.amount, view.report.seed_amount) < BigInt(minBp)) out.add(e.id);
  }
  return out;
}

export function edgeById(view: TraceView, id: string): GraphEdge | undefined {
  return view.graph.edges.find((e) => e.id === id);
}

export function nodeById(view: TraceView, id: string): GraphNode | undefined {
  return view.graph.nodes.find((n) => n.id === id);
}

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
        "font-size": 11,
        color: p.ink,
        "text-wrap": "wrap",
        "text-max-width": "150px",
        "text-valign": "bottom",
        "text-margin-y": 6,
        "text-background-color": p.surface,
        "text-background-opacity": 0.85,
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
    { selector: "node.at_rest", style: { "border-style": "double", "border-width": 5, "border-color": p.nodes.at_rest, "background-opacity": 0.35 } },
    { selector: "node.unexplored", style: { "border-style": "dotted", "background-opacity": 0.35, "border-color": p.muted } },
    { selector: "node.seed", style: { "border-width": 3, "border-color": p.amber } },
    {
      selector: "edge",
      style: {
        width: "data(width)",
        "line-color": p.trail,
        "target-arrow-color": p.trail,
        "target-arrow-shape": "triangle",
        "arrow-scale": 0.9,
        "curve-style": "bezier",
        opacity: 0.75,
        label: "data(label)",
        "font-family": "Atkinson Hyperlegible Mono, ui-monospace, monospace",
        "font-size": 10,
        color: p.muted,
        "text-rotation": "autorotate",
        "text-margin-y": -8,
        "text-background-color": p.surface,
        "text-background-opacity": 0.8,
        "text-background-padding": "1px",
        "transition-property": "opacity, line-color, width",
        "transition-duration": 150,
      },
    },
    { selector: "edge.change", style: { "line-style": "dashed", "line-dash-pattern": [7, 4] } },
    { selector: "edge.loop", style: { label: "", "loop-direction": "-45deg", "loop-sweep": "-60deg" } },
    { selector: ".dim", style: { opacity: 0.12 } },
    { selector: ".hidden", style: { display: "none" } },
    {
      selector: "edge.path",
      style: { "line-color": p.amber, "target-arrow-color": p.amber, opacity: 1, color: p.ink, "z-index": 10 },
    },
    { selector: "node.path", style: { "border-color": p.amber, "border-width": 4, opacity: 1, "z-index": 10 } },
    {
      selector: ":selected",
      style: { "overlay-color": p.amber, "overlay-opacity": 0.18, "overlay-padding": 6 },
    },
  ];
}
