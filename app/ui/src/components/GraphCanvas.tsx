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
  nodeSep: 46,
  // Room for a fan's trunk between two columns without touching either column's labels.
  rankSep: 170,
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
 * Straight edges that would cross another node or a label are bent around it: try curves
 * of growing depth on either side and keep the first that is clear. Fans (combs) and loops
 * are left alone.
 */
function routeAroundNodes(cy: Core) {
  type Box = { x1: number; y1: number; x2: number; y2: number };
  const grow = (b: Box, d: number): Box => ({ x1: b.x1 - d, y1: b.y1 - d, x2: b.x2 + d, y2: b.y2 + d });
  const bodies = new Map<string, Box>();
  const labels = new Map<string, Box>();
  cy.nodes().forEach((n) => {
    bodies.set(n.id(), grow(n.boundingBox({ includeLabels: true, includeOverlays: false }), 4));
    labels.set(n.id(), grow(n.boundingBox({ includeNodes: false, includeLabels: true, includeOverlays: false }), 2));
  });
  const inside = (p: Position, b: Box) => p.x > b.x1 && p.x < b.x2 && p.y > b.y1 && p.y < b.y2;
  const blocked = (e: EdgeSingular, ctrl: Position | null) => {
    const s = e.source().position();
    const t = e.target().position();
    const own = [e.source().id(), e.target().id()];
    for (let k = 2; k <= 28; k++) {
      const u = k / 30;
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
  cy.edges().forEach((e) => {
    e.removeStyle("curve-style control-point-distances control-point-weights");
    if (e.isLoop() || e.hasClass("fan-in") || e.hasClass("fan-out")) return;
    if (!blocked(e, null)) return;
    const s = e.source().position();
    const t = e.target().position();
    const len = Math.hypot(t.x - s.x, t.y - s.y) || 1;
    const nx = -(t.y - s.y) / len;
    const ny = (t.x - s.x) / len;
    const mid = { x: (s.x + t.x) / 2, y: (s.y + t.y) / 2 };
    for (const d of [50, -50, 90, -90, 140, -140, 200, -200, 270, -270]) {
      const ctrl = { x: mid.x + nx * d, y: mid.y + ny * d };
      if (!blocked(e, ctrl)) {
        // Cytoscape measures the distance perpendicular to source→target; check which sign
        // matches by reading the control point back.
        e.style({ "curve-style": "unbundled-bezier", "control-point-distances": d, "control-point-weights": 0.5 });
        const cp = (e as unknown as { controlPoints(): Position[] | undefined }).controlPoints()?.[0];
        if (cp && Math.hypot(cp.x - ctrl.x, cp.y - ctrl.y) > 1) e.style("control-point-distances", -d);
        return;
      }
    }
  });
}

/** Move nodes from `from` to where they are now, smoothly (or not at all if reduced motion). */
function glide(cy: Core, from: Map<string, Position>) {
  if (reducedMotion()) return;
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
    if (nodesChanged) {
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
