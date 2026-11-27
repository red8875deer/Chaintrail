// Lays out large trace graphs off the main thread, so the page stays responsive while dagre
// works (about 1–2 s for 1,500+ addresses). Same options as the on-thread layout.

import { Graph, layout } from "@dagrejs/dagre";

export interface LayoutRequest {
  id: number;
  options: { rankDir: string; nodeSep: number; rankSep: number; edgeSep: number };
  nodes: { id: string; w: number; h: number }[];
  edges: { id: string; source: string; target: string; weight: number }[];
}

export interface LayoutResult {
  id: number;
  positions: [string, number, number][];
}

self.onmessage = (e: MessageEvent<LayoutRequest>) => {
  const { id, options, nodes, edges } = e.data;
  const g = new Graph({ multigraph: true });
  g.setGraph({
    rankdir: options.rankDir,
    nodesep: options.nodeSep,
    ranksep: options.rankSep,
    edgesep: options.edgeSep,
  });
  g.setDefaultEdgeLabel(() => ({}));
  for (const n of nodes) g.setNode(n.id, { width: n.w, height: n.h });
  for (const x of edges) {
    if (x.source === x.target) continue;
    g.setEdge(x.source, x.target, { weight: x.weight, minlen: 1 }, x.id);
  }
  layout(g);
  const positions: [string, number, number][] = nodes.map((n) => {
    const v = g.node(n.id) as { x: number; y: number };
    return [n.id, v.x, v.y];
  });
  (self as unknown as Worker).postMessage({ id, positions } satisfies LayoutResult);
};
