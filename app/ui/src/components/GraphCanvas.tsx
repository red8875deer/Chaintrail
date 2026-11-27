// The trace graph: Cytoscape with a left-to-right dagre layout (time flows to the right).
// Draws the *display* graph (lib/graph.ts buildDisplay), not the raw report graph.
//
// One Cytoscape instance lives for the whole trace. When the drawn graph changes, it is
// updated in place: if only edges or labels change nothing moves; if nodes are added or
// removed, the layout is recomputed and nodes glide to their new places (so the reader can
// follow what moved) instead of the picture being replaced.

import { type RefObject, useEffect, useRef } from "react";
import cytoscape, { type Core, type EdgeSingular, type Position } from "cytoscape";
import dagre from "cytoscape-dagre";
import { type Display, type Palette, graphStyle } from "../lib/graph";
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
const MAX_FIT_ZOOM = 1.4;
/** Room around the graph; more at the top (notice) and bottom (toolbar, legend). */
const PAD = { top: 104, bottom: 96, left: 48, right: 48 };
const MOVE_MS = 350;

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
  const bodies = new Map<string, Box>();
  const labels = new Map<string, Box>();
  cy.nodes().forEach((n) => {
    bodies.set(n.id(), grow(n.boundingBox({ includeLabels: true, includeOverlays: false }), 8));
    labels.set(n.id(), grow(n.boundingBox({ includeNodes: false, includeLabels: true, includeOverlays: false }), 4));
  });
  const inside = (p: Position, b: Box) => p.x > b.x1 && p.x < b.x2 && p.y > b.y1 && p.y < b.y2;
  const overlaps = (a: Box, b: Box) => a.x1 < b.x2 && b.x1 < a.x2 && a.y1 < b.y2 && b.y1 < a.y2;
  const placed: Box[] = [];

  /** Where the edge's amount would be drawn (rotated with the line, lifted above it). */
  const labelBox = (e: EdgeSingular, ctrl: Position | null): Box | null => {
    const label = String(e.data("label") ?? "");
    if (!label || !e.style("label")) return null;
    const s = e.source().position();
    const t = e.target().position();
    const mid = ctrl ? { x: (s.x + 2 * ctrl.x + t.x) / 4, y: (s.y + 2 * ctrl.y + t.y) / 4 } : { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
    const a = Math.atan2(t.y - s.y, t.x - s.x);
    const w = label.length * 7.4 + 6;
    const h = 18;
    const hw = (w * Math.abs(Math.cos(a)) + h * Math.abs(Math.sin(a))) / 2;
    const hh = (w * Math.abs(Math.sin(a)) + h * Math.abs(Math.cos(a))) / 2;
    const cy_ = mid.y + Number(e.data("labelDy") ?? -8);
    return { x1: mid.x - hw, x2: mid.x + hw, y1: cy_ - hh, y2: cy_ + hh };
  };

  const blocked = (e: EdgeSingular, ctrl: Position | null) => {
    const s = e.source().position();
    const t = e.target().position();
    const own = [e.source().id(), e.target().id()];
    const lb = labelBox(e, ctrl);
    if (lb) {
      for (const [id, b] of bodies) if (!own.includes(id) && overlaps(lb, b)) return true;
      for (const b of placed) if (overlaps(lb, b)) return true;
    }
    for (let k = 3; k <= 57; k++) {
      const u = k / 60;
      const p = ctrl
        ? {
            x: (1 - u) * (1 - u) * s.x + 2 * (1 - u) * u * ctrl.x + u * u * t.x,
            y: (1 - u) * (1 - u) * s.y + 2 * (1 - u) * u * ctrl.y + u * u * t.y,
          }
        : { x: s.x + (t.x - s.x) * u, y: s.y + (t.y - s.y) * u };
      for (const [id, b] of bodies) {
        if (own.includes(id)) {
          // An edge may touch its own nodes, but not run through their labels.
          if (inside(p, labels.get(id)!)) return true;
        } else if (inside(p, b)) {
          return true;
        }
      }
    }
    return false;
  };

  const edges = cy.edges().sort((a, b) => Number(b.data("amount") ?? 0) - Number(a.data("amount") ?? 0));
  edges.forEach((e) => {
    e.removeStyle("curve-style control-point-distances control-point-weights");
  });
  // Combs and loops keep their shape; their amounts are fixed obstacles.
  edges.forEach((e) => {
    if (e.isLoop() || e.hasClass("fan-in") || e.hasClass("fan-out")) {
      const b = e.boundingBox({ includeNodes: false, includeEdges: false, includeLabels: true, includeOverlays: false });
      if (b.w > 1 && b.h > 1) placed.push(b);
    }
  });
  edges.forEach((e) => {
    if (e.isLoop() || e.hasClass("fan-in") || e.hasClass("fan-out")) return;
    let ctrl: Position | null = null;
    let d = 0;
    if (blocked(e, null)) {
      const s = e.source().position();
      const t = e.target().position();
      const len = Math.hypot(t.x - s.x, t.y - s.y) || 1;
      const nx = -(t.y - s.y) / len;
      const ny = (t.x - s.x) / len;
      const mid = { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
      for (const c of [50, -50, 90, -90, 140, -140, 200, -200, 270, -270]) {
        const p = { x: mid.x + nx * c, y: mid.y + ny * c };
        if (!blocked(e, p)) {
          ctrl = p;
          d = c;
          break;
        }
      }
      if (ctrl) {
        // Same sign convention as Cytoscape: distance along the left-hand normal of
        // source→target (checked in the browser).
        e.style({ "curve-style": "unbundled-bezier", "control-point-distances": d, "control-point-weights": 0.5 });
      }
    }
    const lb = labelBox(e, ctrl);
    if (lb) placed.push(lb);
  });
}

/** Small changes (e.g. revealing a lead's route, which may also dissolve a small group) are
 * placed into the existing layout; larger ones (slider, grouping, "Show all") re-run it. */
const MAX_PLACED = 24;
const MAX_REMOVED = 6;

/**
 * Place new nodes into an existing left-to-right layout without moving anything else: each
 * goes one column after the drawn node that sends it funds (or before the one it sends
 * funds to), in the nearest free slot above or below.
 */
function placeNear(cy: Core, added: cytoscape.NodeCollection) {
  type Box = { x1: number; y1: number; x2: number; y2: number };
  const box = (n: cytoscape.NodeSingular, at?: Position): Box => {
    const b = n.boundingBox({ includeLabels: true, includeOverlays: false });
    if (!at) return b;
    const p = n.position();
    return { x1: b.x1 + at.x - p.x, x2: b.x2 + at.x - p.x, y1: b.y1 + at.y - p.y, y2: b.y2 + at.y - p.y };
  };
  const overlaps = (a: Box, b: Box, m = 14) => a.x1 < b.x2 + m && b.x1 < a.x2 + m && a.y1 < b.y2 + m && b.y1 < a.y2 + m;
  const pending = new Set(added.map((n) => n.id()));
  const placed = cy.nodes().filter((n) => !pending.has(n.id())) as unknown as cytoscape.NodeCollection;
  const boxes: Box[] = placed.map((n) => box(n));
  // Column spacing and step as the layout made them.
  const xs = [...new Set(placed.map((n) => Math.round(n.position().x)))].sort((a, b) => a - b);
  const gaps = xs.slice(1).map((x, i) => x - xs[i]!).filter((g) => g > 40).sort((a, b) => a - b);
  const colGap = gaps.length ? gaps[Math.floor(gaps.length / 2)]! : 260;
  const snap = (x: number) => {
    const near = xs.find((c) => Math.abs(c - x) < colGap / 3);
    return near ?? x;
  };
  for (let guard = 0; pending.size > 0 && guard < 200; guard++) {
    let progress = false;
    for (const id of [...pending]) {
      const n = cy.getElementById(id) as cytoscape.NodeSingular;
      const from = n.incomers("node").filter((m) => !pending.has(m.id()));
      const to = n.outgoers("node").filter((m) => !pending.has(m.id()));
      if (from.empty() && to.empty()) continue;
      const anchor = (from.nonempty() ? from[0] : to[0]) as cytoscape.NodeSingular;
      const a = anchor.position();
      const x = snap(from.nonempty() ? a.x + colGap : a.x - colGap);
      const step = Math.max(40, box(n).y2 - box(n).y1 + 16);
      let y = a.y;
      for (let k = 0; k < 60; k++) {
        const cand = a.y + (k % 2 === 0 ? 1 : -1) * Math.ceil(k / 2) * step;
        if (!boxes.some((b) => overlaps(box(n, { x, y: cand }), b))) {
          y = cand;
          break;
        }
      }
      n.position({ x, y });
      boxes.push(box(n));
      pending.delete(id);
      progress = true;
    }
    if (!progress) break;
  }
  // Anything not connected to a drawn node: below everything.
  let bottom = Math.max(...boxes.map((b) => b.y2), 0);
  for (const id of pending) {
    const n = cy.getElementById(id) as cytoscape.NodeSingular;
    bottom += 80;
    n.position({ x: xs[0] ?? 0, y: bottom });
  }
}

/** Move nodes from `from` to where they are now, smoothly (or not at all if reduced motion). */
function glide(cy: Core, from: Map<string, Position>) {
  if (reducedMotion()) return;
  // While nodes pass each other, Cytoscape warns about edges it cannot draw for a frame.
  cytoscape.warnings(false);
  setTimeout(() => cytoscape.warnings(true), MOVE_MS + 100);
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
    cy.on("dragfree", "node", () => routeAroundNodes(cy));
    cyRef.current = cy;
    if (new URLSearchParams(window.location.search).has("debug")) {
      (window as unknown as { __chaintrailCy?: Core }).__chaintrailCy = cy;
    }
    const focus = () => (focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed"));
    controls.current = {
      // The whole graph, however small: labels too small to read are left out.
      fit: () => view(cy, cy.elements(), null, true, true),
      relayout: () => {
        cy.nodes().stop(true, true);
        const before = new Map(cy.nodes().map((n) => [n.id(), { ...n.position() }]));
        cy.layout(LAYOUT as unknown as cytoscape.LayoutOptions).run();
        routeAroundNodes(cy);
        const path = cy.elements(".path");
        view(cy, path.nonempty() ? path : cy.elements(), focus(), true);
        glide(cy, before);
      },
      png: () => cy.png({ full: true, scale: 2, bg: readPalette().surface }),
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
    cy.batch(() => {
      cy.elements().forEach((el) => {
        if (!want.has(el.id())) cy.remove(el);
      });
      for (const [id, def] of want) {
        const el = cy.getElementById(id);
        if (el.empty()) {
          cy.add(def);
        } else {
          const { id: _id, source: _s, target: _t, ...rest } = def.data;
          el.data(rest);
          el.classes(def.classes ?? "");
        }
      }
    });
    const added = cy.nodes().filter((n) => !before.has(n.id()));
    const removed = [...before.keys()].filter((id) => cy.getElementById(id).empty()).length;
    if (nodesChanged && !first && removed <= MAX_REMOVED && added.length <= MAX_PLACED) {
      // A few nodes revealed (e.g. the route to a selected lead): place them next to what is
      // already drawn and move nothing else. "Reset layout" gives the full layout again.
      placeNear(cy, added);
      added.forEach((n) => {
        const old = n.neighborhood("node").filter((m) => before.has(m.id())).first();
        if (old.nonempty()) before.set(n.id(), before.get(old.id())!);
      });
      glideFrom.current = before;
    } else if (nodesChanged) {
      cy.layout(LAYOUT as unknown as cytoscape.LayoutOptions).run();
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
    routeAroundNodes(cy);
  }, [display]);

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

  return <div ref={host} className="canvas" aria-label="Trace graph" role="img" />;
}
