// The trace graph: Cytoscape with a left-to-right dagre layout (time flows to the right).
// Draws the *display* graph (lib/graph.ts buildDisplay), not the raw report graph.
//
// One Cytoscape instance lives for the whole trace. When the drawn graph changes, it is
// updated in place: if only edges or labels change nothing moves; if nodes are added or
// removed, the layout is recomputed and nodes glide to their new places (so the reader can
// follow what moved) instead of the picture being replaced.

import { type KeyboardEvent, type RefObject, useEffect, useRef, useState } from "react";
import cytoscape, { type Core, type EdgeSingular, type Position } from "cytoscape";
import dagre from "cytoscape-dagre";
import { type Display, type Palette, graphStyle, isGroupId, labelWidth } from "../lib/graph";
import type { NodeStyleKey } from "../lib/meaning";
import { NODE_STYLES } from "../lib/meaning";

cytoscape.use(dagre);

export type Selection = { type: "node" | "edge"; id: string } | null;

export interface GraphControls {
  fit: () => void;
  relayout: () => void;
  png: () => string;
  focus: (id: string) => boolean;
}

/** Below this zoom the labels (12–13px) render under ~10px; never open smaller than this.
 * "Fit" can still show the whole graph as an overview. */
const MIN_READABLE_ZOOM = 0.8;
/** Small graphs are not blown up: more context, steadier text size between traces. */
const MAX_FIT_ZOOM = 1.15;
/** Room around the graph; more at the top (notice) and bottom (toolbar, legend). */
const PAD = { top: 28, bottom: 60, left: 48, right: 48 };
const MOVE_MS = 350;
/** Edge amounts on lines steeper than this (30°) are drawn level. */
const STEEP_LABEL_RAD = Math.PI / 6;
/** `?debug=1`: the graph is exposed for tests and layout decisions are logged. */
const DEBUG = typeof window !== "undefined" && new URLSearchParams(window.location.search).has("debug");

const LAYOUT = {
  name: "dagre",
  rankDir: "LR",
  // Node sizes include their labels, so a tall label never touches the node below it.
  nodeDimensionsIncludeLabels: true,
  nodeSep: 28,
  // Room for a fan's trunk between two columns of labels.
  rankSep: 110,
  edgeSep: 18,
  padding: 40,
  fit: false,
  animate: false,
  // Heavier flows pull straighter, so the main trail reads as a line.
  edgeWeight: (e: EdgeSingular) => Number(e.data("weight") ?? 1),
};

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

export function readPalette(): Palette {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim() || "#888";
  const nodes = Object.fromEntries(
    (Object.keys(NODE_STYLES) as NodeStyleKey[]).map((k) => [k, v(`--n-${k}`)]),
  ) as Record<NodeStyleKey, string>;
  return {
    ink: v("--ink"),
    muted: v("--muted"),
    surface: v("--surface"),
    trail: v("--trail"),
    amber: v("--amber"),
    nodes,
  };
}

type Eles = Pick<cytoscape.Collection, "boundingBox" | "empty" | "nonempty">;

/**
 * Show `eles` as large as fits, but never smaller than readable (unless `overview`). If they
 * do not fit readably, show as much as fits while keeping `focus` (e.g. the selected lead)
 * on screen.
 */
function view(cy: Core, eles: Eles, focus: Eles | null, animate: boolean, overview = false) {
  if (eles.empty()) return;
  const bb = eles.boundingBox();
  const w = Math.max(cy.width() - PAD.left - PAD.right, 100);
  const h = Math.max(cy.height() - PAD.top - PAD.bottom, 100);
  const fitZoom = Math.min(w / Math.max(bb.w, 1), h / Math.max(bb.h, 1));
  const zoom = Math.max(overview ? 0.1 : MIN_READABLE_ZOOM, Math.min(MAX_FIT_ZOOM, fitZoom));
  let cx = bb.x1 + bb.w / 2;
  let cy_ = bb.y1 + bb.h / 2;
  if (zoom > fitZoom && focus && focus.nonempty()) {
    const f = focus.boundingBox();
    const halfW = w / 2 / zoom;
    const halfH = h / 2 / zoom;
    const m = 24 / zoom;
    const clamp = (v: number, lo: number, hi: number) => (lo > hi ? (lo + hi) / 2 : Math.min(hi, Math.max(lo, v)));
    cx = clamp(cx, f.x2 + m - halfW, f.x1 - m + halfW);
    cy_ = clamp(cy_, f.y2 + m - halfH, f.y1 - m + halfH);
  }
  // Never show the start of the trace cut in half (its amount would read wrong): bring it
  // fully into view if the focus stays in view, else move it fully out.
  if (!overview) {
    const halfW = w / 2 / zoom;
    const halfH = h / 2 / zoom;
    const m = 12 / zoom;
    const f = focus && focus.nonempty() ? focus.boundingBox() : null;
    const keeps = (x: number, y: number) =>
      !f || (f.x1 >= x - halfW && f.x2 <= x + halfW && f.y1 >= y - halfH && f.y2 <= y + halfH);
    cy.nodes(".seed").forEach((n) => {
      const b = n.boundingBox({ includeLabels: true });
      // Cut: it crosses the canvas edge. Brought in, it clears the padding; moved out, it
      // clears the canvas edge.
      const fix = (lo: number, hi: number, c: number, half: number, padLo: number, padHi: number, other: number, horizontal: boolean) => {
        const edgeLo = c - half - padLo / zoom;
        const edgeHi = c + half + padHi / zoom;
        const cutLow = lo < edgeLo && hi > edgeLo;
        const cutHigh = hi > edgeHi && lo < edgeHi;
        if (!cutLow && !cutHigh) return c;
        const inside = cutLow ? lo - m + half : hi + m - half;
        const outside = cutLow ? hi + m + half + padLo / zoom : lo - m - half - padHi / zoom;
        const ok = (x: number) => (horizontal ? keeps(x, other) : keeps(other, x));
        return ok(inside) ? inside : ok(outside) ? outside : c;
      };
      cx = fix(b.x1, b.x2, cx, halfW, PAD.left, PAD.right, cy_, true);
      cy_ = fix(b.y1, b.y2, cy_, halfH, PAD.top, PAD.bottom, cx, false);
    });
  }
  const pan = { x: PAD.left + w / 2 - zoom * cx, y: PAD.top + h / 2 - zoom * cy_ };
  cy.stop(true);
  if (animate && !reducedMotion()) cy.animate({ zoom, pan }, { duration: MOVE_MS });
  else cy.viewport({ zoom, pan });
}

/**
 * Edges that would cross another node or label, or whose amount would land on another
 * amount, are bent: try curves of growing depth on either side and keep the first that is
 * clear. Larger flows are placed first, so they stay straight. Fans (combs) and loops keep
 * their own shape; their amounts count as already placed.
 */
function routeAroundNodes(cy: Core) {
  type Box = { x1: number; y1: number; x2: number; y2: number };
  const grow = (b: Box, d: number): Box => ({ x1: b.x1 - d, y1: b.y1 - d, x2: b.x2 + d, y2: b.y2 + d });
  const inside = (p: Position, b: Box) => p.x > b.x1 && p.x < b.x2 && p.y > b.y1 && p.y < b.y2;
  const overlaps = (a: Box, b: Box) => a.x1 < b.x2 && b.x1 < a.x2 && a.y1 < b.y2 && b.y1 < a.y2;

  // Spatial grid: each check looks only at boxes near it (large graphs froze for seconds).
  const CELL = 160;
  class Grid<T> {
    private cells = new Map<string, { box: Box; item: T }[]>();
    add(box: Box, item: T) {
      for (let x = Math.floor(box.x1 / CELL); x <= Math.floor(box.x2 / CELL); x++)
        for (let y = Math.floor(box.y1 / CELL); y <= Math.floor(box.y2 / CELL); y++) {
          const k = `${x},${y}`;
          (this.cells.get(k) ?? this.cells.set(k, []).get(k)!).push({ box, item });
        }
    }
    at(p: Position) {
      return this.cells.get(`${Math.floor(p.x / CELL)},${Math.floor(p.y / CELL)}`) ?? [];
    }
    within(box: Box) {
      const out: { box: Box; item: T }[] = [];
      for (let x = Math.floor(box.x1 / CELL); x <= Math.floor(box.x2 / CELL); x++)
        for (let y = Math.floor(box.y1 / CELL); y <= Math.floor(box.y2 / CELL); y++)
          for (const e of this.cells.get(`${x},${y}`) ?? []) if (overlaps(e.box, box)) out.push(e);
      return out;
    }
  }
  const bodies = new Grid<string>();
  const labels = new Map<string, Box>();
  cy.nodes().forEach((n) => {
    bodies.add(grow(n.boundingBox({ includeLabels: true, includeOverlays: false }), 8), n.id());
    labels.set(n.id(), grow(n.boundingBox({ includeNodes: false, includeLabels: true, includeOverlays: false }), 4));
  });
  const placed = new Grid<null>();

  /** Where the edge's amount would be drawn (rotated with the line, lifted above it). */
  const labelBox = (e: EdgeSingular, ctrl: Position | null): Box | null => {
    const label = String(e.data("label") ?? "");
    if (!label || !e.style("label")) return null;
    const s = e.source().position();
    const t = e.target().position();
    const mid = ctrl ? { x: (s.x + 2 * ctrl.x + t.x) / 4, y: (s.y + 2 * ctrl.y + t.y) / 4 } : { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
    const a = e.hasClass("flat") ? 0 : Math.atan2(t.y - s.y, t.x - s.x);
    const w = labelWidth(label);
    const h = 18;
    const hw = (w * Math.abs(Math.cos(a)) + h * Math.abs(Math.sin(a))) / 2;
    const hh = (w * Math.abs(Math.sin(a)) + h * Math.abs(Math.cos(a))) / 2;
    const cy_ = mid.y + Number(e.data("labelDy") ?? -8);
    return { x1: mid.x - hw, x2: mid.x + hw, y1: cy_ - hh, y2: cy_ + hh };
  };

  /** How badly a route collides: the amount over a node or another amount counts most, then
   * each point of the line inside a node or label. Stops counting at `limit`. */
  const cost = (e: EdgeSingular, ctrl: Position | null, limit = Infinity) => {
    const s = e.source().position();
    const t = e.target().position();
    const own = [e.source().id(), e.target().id()];
    const ownLabels = own.map((id) => labels.get(id)!).filter(Boolean);
    const lb = labelBox(e, ctrl);
    let c = 0;
    if (lb) {
      c += 20 * bodies.within(lb).filter((b) => !own.includes(b.item)).length;
      c += 20 * placed.within(lb).length;
    }
    for (let k = 3; k <= 57 && c < limit; k++) {
      const u = k / 60;
      const p = ctrl
        ? {
            x: (1 - u) * (1 - u) * s.x + 2 * (1 - u) * u * ctrl.x + u * u * t.x,
            y: (1 - u) * (1 - u) * s.y + 2 * (1 - u) * u * ctrl.y + u * u * t.y,
          }
        : { x: s.x + (t.x - s.x) * u, y: s.y + (t.y - s.y) * u };
      // An edge may touch its own nodes, but not run through their labels.
      if (ownLabels.some((b) => inside(p, b))) c += 1;
      if (bodies.at(p).some((b) => !own.includes(b.item) && inside(p, b.box))) c += 1;
    }
    return c;
  };

  const edges = cy.edges().sort((a, b) => Number(b.data("amount") ?? 0) - Number(a.data("amount") ?? 0));
  edges.forEach((e) => {
    e.removeStyle("curve-style control-point-distances control-point-weights");
    // Amounts follow the line when it is nearly level; on steeper lines they stay level,
    // so they are never read at a slant.
    const s = e.source().position();
    const t = e.target().position();
    const steep = Math.abs(Math.atan2(t.y - s.y, Math.abs(t.x - s.x))) > STEEP_LABEL_RAD;
    if (steep !== e.hasClass("flat")) e.toggleClass("flat", steep);
  });
  // Combs and loops keep their shape; their amounts are fixed obstacles.
  edges.forEach((e) => {
    if (e.isLoop() || e.hasClass("fan-in") || e.hasClass("fan-out")) {
      const b = e.boundingBox({ includeNodes: false, includeEdges: false, includeLabels: true, includeOverlays: false });
      if (b.w > 1 && b.h > 1) placed.add(b, null);
    }
  });
  edges.forEach((e) => {
    if (e.isLoop() || e.hasClass("fan-in") || e.hasClass("fan-out")) return;
    let ctrl: Position | null = null;
    let best = cost(e, null);
    if (best > 0) {
      const s = e.source().position();
      const t = e.target().position();
      const len = Math.hypot(t.x - s.x, t.y - s.y) || 1;
      const nx = -(t.y - s.y) / len;
      const ny = (t.x - s.x) / len;
      const mid = { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
      // The first clear curve, else the one that collides least (gentler curves win ties).
      let bend = 0;
      for (const c of [50, -50, 90, -90, 140, -140, 200, -200, 270, -270]) {
        const p = { x: mid.x + nx * c, y: mid.y + ny * c };
        const k = cost(e, p, best);
        if (k < best) {
          best = k;
          bend = c;
          ctrl = p;
          if (k === 0) break;
        }
      }
      // Same sign convention as Cytoscape: distance along the left-hand normal of
      // source→target (checked in the browser).
      if (ctrl) e.style({ "curve-style": "unbundled-bezier", "control-point-distances": bend, "control-point-weights": 0.5 });
    }
    const lb = labelBox(e, ctrl);
    if (lb) placed.add(lb, null);
  });
}

/** Small changes (e.g. revealing a lead's route, which may also dissolve a small group) are
 * placed into the existing layout; larger ones (slider, grouping, "Show all") re-run it. */
const MAX_PLACED = 24;
const MAX_REMOVED = 6;

/**
 * Place new nodes into an existing left-to-right layout: each goes one column after what
 * pays it (or one before what it pays), in the nearest free slot above or below. When a new
 * route needs more columns than there are before the drawn node it reaches, everything from
 * that node rightwards moves right as a block (nothing else moves). Returns false if money
 * would still flow right to left; the caller then lays the whole graph out again.
 */
function placeNear(cy: Core, added: cytoscape.NodeCollection, pinned: ReadonlySet<string> = new Set()): boolean {
  type Box = { x1: number; y1: number; x2: number; y2: number };
  type N = cytoscape.NodeSingular;
  const box = (n: N, at?: Position): Box => {
    const b = n.boundingBox({ includeLabels: true, includeOverlays: false });
    if (!at) return b;
    const p = n.position();
    return { x1: b.x1 + at.x - p.x, x2: b.x2 + at.x - p.x, y1: b.y1 + at.y - p.y, y2: b.y2 + at.y - p.y };
  };
  const overlaps = (a: Box, b: Box, m = 14) => a.x1 < b.x2 + m && b.x1 < a.x2 + m && a.y1 < b.y2 + m && b.y1 < a.y2 + m;
  const pending = new Set(added.map((n) => n.id()));
  const isNew = (id: string) => pending.has(id);
  const old: N[] = [];
  cy.nodes().forEach((n) => {
    if (!isNew(n.id())) old.push(n);
  });
  const node = (id: string) => cy.getElementById(id) as N;
  const others = (c: cytoscape.CollectionReturnValue, self: string) => {
    const out: N[] = [];
    c.forEach((m) => {
      if (m.isNode() && m.id() !== self) out.push(m);
    });
    return out;
  };
  // Column spacing as the layout made it.
  let xs = [...new Set(old.map((n) => Math.round(n.position().x)))].sort((a, b) => a - b);
  const gaps = xs.slice(1).map((x, i) => x - xs[i]!).filter((g) => g > 40).sort((a, b) => a - b);
  const colGap = gaps.length ? gaps[Math.floor(gaps.length / 2)]! : 260;
  const snap = (x: number) => xs.find((c) => Math.abs(c - x) < colGap / 3) ?? x;

  // Flow order among the new nodes: senders first.
  const indeg = new Map<string, number>([...pending].map((id) => [id, 0]));
  added.forEach((n) => {
    for (const m of others(n.outgoers("node"), n.id())) if (indeg.has(m.id())) indeg.set(m.id(), indeg.get(m.id())! + 1);
  });
  const order: string[] = [];
  const queue = [...pending].filter((id) => indeg.get(id) === 0);
  while (queue.length) {
    const id = queue.shift()!;
    order.push(id);
    for (const m of others(node(id).outgoers("node"), id)) {
      if (!indeg.has(m.id())) continue;
      indeg.set(m.id(), indeg.get(m.id())! - 1);
      if (indeg.get(m.id()) === 0) queue.push(m.id());
    }
  }
  for (const id of pending) if (!order.includes(id)) order.push(id);

  // 1. Columns: one right of the rightmost sender, else one left of the leftmost receiver.
  const colX = new Map<string, number>();
  const columns = () => {
    colX.clear();
    for (const id of order) {
      const n = node(id);
      const from = others(n.incomers("node"), id)
        .map((m) => (isNew(m.id()) ? colX.get(m.id()) : m.position().x))
        .filter((x): x is number => x !== undefined);
      if (from.length) {
        colX.set(id, snap(Math.max(...from) + colGap));
        continue;
      }
      const to = others(n.outgoers("node"), id)
        .filter((m) => !isNew(m.id()))
        .map((m) => m.position().x);
      if (to.length) colX.set(id, snap(Math.min(...to) - colGap));
    }
  };
  // 2. Room: a drawn node paid by a new one must be at least half a column to its right.
  for (let round = 0; round < 8; round++) {
    columns();
    let need: { at: number; dx: number } | null = null;
    for (const id of order) {
      const x = colX.get(id);
      if (x === undefined) continue;
      for (const m of others(node(id).outgoers("node"), id)) {
        if (isNew(m.id())) continue;
        const mx = m.position().x;
        if (mx - x < colGap / 2 && (!need || mx < need.at)) need = { at: mx, dx: x + colGap - mx };
      }
    }
    if (!need) break;
    const { at, dx } = need;
    for (const m of old) {
      if (pinned.has(m.id())) continue;
      const p = m.position();
      if (p.x >= at - 1) m.position({ x: p.x + dx, y: p.y });
    }
    xs = xs.map((c) => (c >= at - 1 ? c + dx : c));
  }
  columns();

  // 3. Rows: the nearest free slot by the node it is attached to.
  const boxes: Box[] = old.map((n) => box(n));
  const done = new Set<string>();
  for (const id of order) {
    const x = colX.get(id);
    if (x === undefined) continue;
    const n = node(id);
    const placedNow = (m: N) => !isNew(m.id()) || done.has(m.id());
    const anchor = others(n.incomers("node"), id).find(placedNow) ?? others(n.outgoers("node"), id).find(placedNow);
    const y0 = anchor ? anchor.position().y : 0;
    const step = Math.max(40, box(n).y2 - box(n).y1 + 16);
    let y = y0;
    for (let k = 0; k < 60; k++) {
      const cand = y0 + (k % 2 === 0 ? 1 : -1) * Math.ceil(k / 2) * step;
      if (!boxes.some((b) => overlaps(box(n, { x, y: cand }), b))) {
        y = cand;
        break;
      }
    }
    n.position({ x, y });
    boxes.push(box(n));
    done.add(id);
  }
  // Anything not connected to a drawn node: below everything.
  let bottom = Math.max(...boxes.map((b) => b.y2), 0);
  for (const id of pending) {
    if (done.has(id)) continue;
    bottom += 80;
    node(id).position({ x: xs[0] ?? 0, y: bottom });
  }
  // Money always flows left to right.
  let ok = true;
  added.connectedEdges().forEach((e) => {
    if (e.isEdge() && !e.isLoop() && e.source().position().x >= e.target().position().x - 1) {
      ok = false;
      if (DEBUG) console.debug("placeNear: right to left", e.source().id(), "→", e.target().id());
    }
  });
  return ok;
}

/** Graphs with more nodes than this are laid out in a worker (dagre takes 1–2 s at 1,500+). */
const BIG_GRAPH = 300;
let layoutWorker: Worker | null = null;
let layoutSeq = 0;

/** The same layout as LAYOUT, computed off the main thread. Resolves to null when a newer
 * layout was requested meanwhile (its result would be stale). */
function layoutOffThread(cy: Core): Promise<Map<string, Position> | null> {
  layoutWorker ??= new Worker(new URL("./layout.worker.ts", import.meta.url), { type: "module" });
  const worker = layoutWorker;
  const id = ++layoutSeq;
  const nodes = cy.nodes().map((n) => {
    const d = n.layoutDimensions({ nodeDimensionsIncludeLabels: true });
    return { id: n.id(), w: d.w, h: d.h };
  });
  const edges = cy.edges().map((e) => ({
    id: e.id(),
    source: e.source().id(),
    target: e.target().id(),
    weight: Number(e.data("weight") ?? 1),
  }));
  return new Promise((resolve) => {
    const on = (ev: MessageEvent<{ id: number; positions: [string, number, number][] }>) => {
      if (ev.data.id !== id) return;
      worker.removeEventListener("message", on);
      resolve(id === layoutSeq ? new Map(ev.data.positions.map(([k, x, y]) => [k, { x, y }])) : null);
    };
    worker.addEventListener("message", on);
    worker.postMessage({
      id,
      options: { rankDir: LAYOUT.rankDir, nodeSep: LAYOUT.nodeSep, rankSep: LAYOUT.rankSep, edgeSep: LAYOUT.edgeSep },
      nodes,
      edges,
    });
  });
}

/** Re-enables Cytoscape's warnings after the last glide (glides can overlap). */
let warningsTimer: ReturnType<typeof setTimeout> | undefined;

/** Move nodes from `from` to where they are now, smoothly (or not at all if reduced motion). */
function glide(cy: Core, from: Map<string, Position>) {
  if (reducedMotion()) return;
  // While nodes pass each other, Cytoscape warns about edges it cannot draw for a frame.
  cytoscape.warnings(false);
  clearTimeout(warningsTimer);
  warningsTimer = setTimeout(() => cytoscape.warnings(true), MOVE_MS + 150);
  cy.nodes().forEach((n) => {
    const to = { ...n.position() };
    const start = from.get(n.id());
    if (!start || (Math.abs(start.x - to.x) < 0.5 && Math.abs(start.y - to.y) < 0.5)) return;
    n.position(start);
    n.animate({ position: to }, { duration: MOVE_MS, easing: "ease-in-out-cubic" });
  });
}

export function GraphCanvas({
  display,
  path,
  focusId,
  viewTarget,
  selected,
  onSelect,
  controls,
  busy = false,
  onOpen,
}: {
  display: Display;
  /** Displayed ids on the selected lead's path. */
  path: { nodes: Set<string>; edges: Set<string> } | null;
  /** Displayed node to keep in view (the selected lead). */
  focusId: string | null;
  /** Displayed nodes to bring into view once (e.g. the members of an opened group). */
  viewTarget: { ids: string[] } | null;
  selected: Selection;
  onSelect: (s: Selection) => void;
  controls: RefObject<GraphControls | null>;
  /** Work on the graph is about to block the page: show that it is being arranged. */
  busy?: boolean;
  /** Open a group or run of addresses (Enter on it). */
  onOpen?: (id: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);
  const nodeSet = useRef("");
  /** Positions to glide from after a layout (set by the update, used by the last effect). */
  const glideFrom = useRef<Map<string, Position> | null>(null);
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const focusRef = useRef(focusId);
  focusRef.current = focusId;
  const [arranging, setArranging] = useState(false);
  /** Positions of nodes the investigator dragged. */
  const pinned = useRef(new Map<string, Position>());

  // Create the graph once.
  useEffect(() => {
    if (!host.current || cyRef.current) return;
    const cy = cytoscape({
      container: host.current,
      elements: [],
      style: graphStyle(readPalette()),
      minZoom: 0.1,
      maxZoom: 3,
      boxSelectionEnabled: false,
    });
    cy.on("tap", "node", (e) => onSelectRef.current({ type: "node", id: e.target.id() }));
    cy.on("tap", "edge", (e) => onSelectRef.current({ type: "edge", id: e.target.id() }));
    cy.on("tap", (e) => {
      if (e.target === cy) onSelectRef.current(null);
    });
    cy.on("mouseover", "edge", (e) => e.target.addClass("hover"));
    cy.on("mouseout", "edge", (e) => e.target.removeClass("hover"));
    // A node the investigator moved stays where it was put (until "Reset layout").
    cy.on("dragfree", "node", (e) => {
      pinned.current.set(e.target.id(), { ...e.target.position() });
      routeAroundNodes(cy);
    });
    // The canvas changes size with the strips around it (notice, tools), not only the window.
    const ro = new ResizeObserver(() => cy.resize());
    ro.observe(host.current);
    cy.on("destroy", () => ro.disconnect());
    cyRef.current = cy;
    if (new URLSearchParams(window.location.search).has("debug")) {
      (window as unknown as { __chaintrailCy?: Core }).__chaintrailCy = cy;
    }
    const focus = () => (focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed"));
    controls.current = {
      // The whole graph, however small: labels too small to read are left out.
      fit: () => view(cy, cy.elements(), null, true, true),
      relayout: () => {
        pinned.current.clear();
        cy.nodes().stop(true, true);
        const before = new Map(cy.nodes().map((n) => [n.id(), { ...n.position() }]));
        const finish = () => {
          routeAroundNodes(cy);
          const path = cy.elements(".path");
          view(cy, path.nonempty() ? path : cy.elements(), focus(), true);
          glide(cy, before);
        };
        if (cy.nodes().length > BIG_GRAPH) {
          setArranging(true);
          void layoutOffThread(cy).then((pos) => {
            if (!pos || cyRef.current !== cy) return;
            cy.batch(() => {
              cy.nodes().forEach((n) => {
                const p = pos.get(n.id());
                if (p) n.position(p);
              });
            });
            finish();
            setArranging(false);
          });
          return;
        }
        cy.layout(LAYOUT as unknown as cytoscape.LayoutOptions).run();
        finish();
      },
      // Sharp but within the browser's canvas size limit (a too-large canvas silently
      // yields an empty image).
      png: () => {
        if (cy.elements().empty()) return "";
        const bb = cy.elements().boundingBox();
        const scale = Math.min(2, 16000 / Math.max(bb.w, bb.h, 1), Math.sqrt(120e6 / Math.max(bb.w * bb.h, 1)));
        return cy.png({ full: true, scale, bg: readPalette().surface });
      },
      focus: (id: string) => {
        const el = cy.getElementById(id);
        if (el.empty()) return false;
        view(cy, el, el, true);
        return true;
      },
    };
  }, [controls]);

  // Apply the drawn graph. Only a change in the set of nodes re-runs the layout.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.nodes().stop(true, true);
    const ids = display.elements
      .filter((e) => e.group === "nodes")
      .map((e) => String(e.data.id))
      .sort()
      .join("|");
    const first = nodeSet.current === "";
    const nodesChanged = ids !== nodeSet.current;
    nodeSet.current = ids;
    const before = new Map(cy.nodes().map((n) => [n.id(), { ...n.position() }]));
    const want = new Map(display.elements.map((e) => [String(e.data.id), e]));
    let structure = false;
    cy.batch(() => {
      cy.elements().forEach((el) => {
        if (!want.has(el.id())) {
          cy.remove(el);
          structure = true;
        }
      });
      for (const [id, def] of want) {
        const el = cy.getElementById(id);
        if (el.empty()) {
          cy.add(def);
          structure = true;
        } else {
          const { id: _id, source: _s, target: _t, ...rest } = def.data;
          el.data(rest);
          el.classes(def.classes ?? "");
        }
      }
    });
    const added = cy.nodes().filter((n) => !before.has(n.id()));
    const removed = [...before.keys()].filter((id) => cy.getElementById(id).empty()).length;
    const pins = () =>
      cy.nodes().forEach((n) => {
        const p = pinned.current.get(n.id());
        if (p) n.position(p);
      });
    if (
      nodesChanged &&
      !first &&
      removed <= MAX_REMOVED &&
      added.length <= MAX_PLACED &&
      placeNear(cy, added, new Set(pinned.current.keys()))
    ) {
      // A few nodes revealed (e.g. the route to a selected lead): place them next to what is
      // already drawn and move nothing else. "Reset layout" gives the full layout again.
      added.forEach((n) => {
        const old = n.neighborhood("node").filter((m) => before.has(m.id())).first();
        if (old.nonempty()) before.set(n.id(), before.get(old.id())!);
      });
      glideFrom.current = before;
    } else if (nodesChanged && cy.nodes().length > BIG_GRAPH) {
      // Large graph: lay out in a worker; meanwhile new nodes wait next to a neighbour.
      cy.nodes().forEach((n) => {
        if (before.has(n.id())) return;
        const old = n.neighborhood("node").filter((m) => before.has(m.id())).first();
        if (old.nonempty()) n.position(before.get(old.id())!);
      });
      setArranging(true);
      void layoutOffThread(cy).then((pos) => {
        if (!pos || cyRef.current !== cy) return;
        cy.nodes().stop(true, true);
        const from = new Map(cy.nodes().map((n) => [n.id(), { ...n.position() }]));
        cy.batch(() => {
          cy.nodes().forEach((n) => {
            const p = pos.get(n.id());
            if (p) n.position(p);
          });
          pins();
        });
        routeAroundNodes(cy);
        const onPath = cy.elements(".path");
        const focus = focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed");
        view(cy, onPath.nonempty() ? onPath : cy.elements(), focus, !first);
        if (!first) glide(cy, from);
        setArranging(false);
      });
      return;
    } else if (nodesChanged) {
      cy.layout(LAYOUT as unknown as cytoscape.LayoutOptions).run();
      pins();
      // New nodes appear from a neighbour that was already drawn.
      cy.nodes().forEach((n) => {
        if (before.has(n.id())) return;
        const old = n.neighborhood("node").filter((m) => before.has(m.id())).first();
        if (old.nonempty()) before.set(n.id(), before.get(old.id())!);
      });
      if (!first) glideFrom.current = before;
      const focus = focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed");
      view(cy, cy.elements(), focus, !first);
    }
    // Re-route only when lines were added or removed (labels changing alone keep their place).
    if (structure) routeAroundNodes(cy);
  }, [display]);

  // Touch screens: the graph starts locked so a swipe scrolls the page; "Move graph" unlocks
  // panning, zooming and dragging.
  const coarse = typeof window !== "undefined" && (window.matchMedia?.("(pointer: coarse)").matches ?? false);
  const [locked, setLocked] = useState(coarse);
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.userPanningEnabled(!locked);
    cy.userZoomingEnabled(!locked);
    cy.autoungrabify(locked);
  }, [locked]);

  // Follow the system theme.
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const restyle = () => cyRef.current?.style(graphStyle(readPalette()));
    mq?.addEventListener("change", restyle);
    return () => mq?.removeEventListener("change", restyle);
  }, []);

  // Clean up on unmount.
  useEffect(
    () => () => {
      cyRef.current?.destroy();
      cyRef.current = null;
      nodeSet.current = "";
      controls.current = null;
    },
    [controls],
  );

  // Highlight the selected lead's path and dim everything else.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.batch(() => {
      cy.elements().removeClass("path dim");
      if (!path) return;
      cy.elements().forEach((el) => {
        const id: string = el.id();
        const on = el.group() === "nodes" ? path.nodes.has(id) : path.edges.has(id);
        el.addClass(on ? "path" : "dim");
      });
    });
    if (path) {
      const eles = cy.elements(".path");
      const focus = focusId ? cy.getElementById(focusId) : null;
      view(cy, eles.nonempty() ? eles : cy.elements(), focus, true);
    }
  }, [display, path, focusId]);

  // Bring newly opened nodes into view (once per request).
  const shownTarget = useRef<{ ids: string[] } | null>(null);
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy || !viewTarget || shownTarget.current === viewTarget) return;
    shownTarget.current = viewTarget;
    let eles = cy.collection();
    for (const id of viewTarget.ids) eles = eles.union(cy.getElementById(id));
    if (eles.nonempty()) view(cy, eles, eles.first(), true);
  }, [display, viewTarget]);

  // Reflect the selection.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.elements().unselect();
    if (selected) cy.getElementById(selected.id).select();
  }, [selected, display]);

  // Last: once the view has been worked out on the new layout, let nodes glide there.
  useEffect(() => {
    const cy = cyRef.current;
    const from = glideFrom.current;
    glideFrom.current = null;
    if (cy && from) glide(cy, from);
  }, [display]);

  // Keyboard: arrows move along the money (right: where it went, left: where it came from,
  // up/down: neighbours in the same column), Escape clears. Each move is announced.
  const [said, setSaid] = useState("");
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const cy = cyRef.current;
    if (!cy) return;
    if (e.key === "Escape") {
      onSelectRef.current(null);
      setSaid("Selection cleared");
      return;
    }
    // Enter opens a selected group or run of addresses.
    if (e.key === "Enter" && selected?.type === "node" && isGroupId(selected.id) && onOpen) {
      e.preventDefault();
      onOpen(selected.id);
      setSaid("Opened");
      return;
    }
    if (!["ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown"].includes(e.key)) return;
    e.preventDefault();
    const current = selected?.type === "node" ? cy.getElementById(selected.id) : cy.collection();
    let next: cytoscape.NodeSingular | undefined;
    if (current.empty() || !current.isNode()) {
      const start = focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed");
      next = (start.nonempty() ? start : cy.nodes()).first() as cytoscape.NodeSingular;
    } else {
      const n = current as cytoscape.NodeSingular;
      const p = n.position();
      const byAmount = (edges: cytoscape.EdgeCollection, end: "source" | "target") => {
        let best: cytoscape.EdgeSingular | undefined;
        edges.forEach((x) => {
          if (!x.isLoop() && (!best || Number(x.data("amount") ?? 0) > Number(best.data("amount") ?? 0))) best = x;
        });
        return best ? (end === "source" ? best.source() : best.target()) : undefined;
      };
      if (e.key === "ArrowRight") next = byAmount(n.outgoers("edge"), "target");
      else if (e.key === "ArrowLeft") next = byAmount(n.incomers("edge"), "source");
      else {
        const up = e.key === "ArrowUp";
        let gap = Infinity;
        cy.nodes().forEach((m) => {
          const q = m.position();
          if (m.id() === n.id() || Math.abs(q.x - p.x) >= 80 || (up ? q.y >= p.y : q.y <= p.y)) return;
          if (Math.abs(q.y - p.y) < gap) {
            gap = Math.abs(q.y - p.y);
            next = m;
          }
        });
      }
    }
    if (!next) {
      setSaid(e.key === "ArrowRight" ? "No further step" : e.key === "ArrowLeft" ? "This is where the funds start" : "No more in this column");
      return;
    }
    onSelectRef.current({ type: "node", id: next.id() });
    controls.current?.focus(next.id());
    setSaid(String(next.data("label") ?? next.id()).replace(/\n/g, ", "));
  };

  return (
    <>
      <div
        ref={host}
        className={locked ? "canvas locked" : "canvas"}
        role="application"
        tabIndex={0}
        aria-roledescription="trace graph"
        aria-label="Trace graph. Arrow keys move between addresses: right follows the funds onward, left goes back towards the start, up and down move within a column. Escape clears the selection."
        onKeyDown={onKeyDown}
      />
      <div className="sr-only" aria-live="polite">
        {said}
      </div>
      {coarse && (
        <button type="button" className="btn graph-lock" aria-pressed={!locked} onClick={() => setLocked((l) => !l)}>
          {locked ? "Move graph" : "Done moving"}
        </button>
      )}
      {(arranging || busy) && (
        <div className="arranging" role="status">
          Arranging the graph…
        </div>
      )}
    </>
  );
}
