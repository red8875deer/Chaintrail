// The trace graph: Cytoscape with a left-to-right dagre layout (time flows to the right).
// Draws the *display* graph (lib/graph.ts buildDisplay), not the raw report graph.

import { type RefObject, useEffect, useRef } from "react";
import cytoscape, { type Core, type EdgeSingular } from "cytoscape";
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

const LAYOUT = {
  name: "dagre",
  rankDir: "LR",
  nodeSep: 46,
  rankSep: 140,
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
 * do not fit readably, centre on `focus` (e.g. the selected lead) at a readable zoom instead.
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
    // Too large to show readably: show as much of it as fits while keeping the focus (with
    // its label) on screen, e.g. the lead at the far end of a long route.
    const f = focus.boundingBox();
    const halfW = w / 2 / zoom;
    const halfH = h / 2 / zoom;
    const m = 24 / zoom;
    const clamp = (v: number, lo: number, hi: number) => (lo > hi ? (lo + hi) / 2 : Math.min(hi, Math.max(lo, v)));
    cx = clamp(cx, f.x2 + m - halfW, f.x1 - m + halfW);
    cy_ = clamp(cy_, f.y2 + m - halfH, f.y1 - m + halfH);
  }
  const pan = {
    x: PAD.left + w / 2 - zoom * cx,
    y: PAD.top + h / 2 - zoom * cy_,
  };
  if (animate && !reducedMotion()) cy.animate({ zoom, pan }, { duration: 300 });
  else cy.viewport({ zoom, pan });
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
  const signature = useRef("");
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const focusRef = useRef(focusId);
  focusRef.current = focusId;

  // Lay out again only when the drawn nodes change. If only edges or labels change, update
  // them in place so nothing moves.
  useEffect(() => {
    if (!host.current) return;
    const sig = display.elements
      .filter((e) => e.group === "nodes")
      .map((e) => e.data.id)
      .join("|");
    const current = cyRef.current;
    if (current && sig === signature.current) {
      const want = new Map(display.elements.map((e) => [String(e.data.id), e]));
      current.batch(() => {
        current.elements().forEach((el) => {
          if (!want.has(el.id())) current.remove(el);
        });
        for (const [id, def] of want) {
          const el = current.getElementById(id);
          if (el.empty()) {
            current.add(def);
          } else {
            const { id: _id, source: _s, target: _t, ...rest } = def.data;
            el.data(rest);
            el.classes(def.classes ?? "");
          }
        }
      });
      return;
    }
    signature.current = sig;
    cyRef.current?.destroy();
    const cy = cytoscape({
      container: host.current,
      elements: display.elements,
      style: graphStyle(readPalette()),
      layout: LAYOUT as unknown as cytoscape.LayoutOptions,
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
    cyRef.current = cy;
    if (new URLSearchParams(window.location.search).has("debug")) {
      (window as unknown as { __chaintrailCy?: Core }).__chaintrailCy = cy;
    }
    const focus = () => (focusRef.current ? cy.getElementById(focusRef.current) : cy.nodes(".seed"));
    view(cy, cy.elements(), focus(), false);
    controls.current = {
      // The whole graph, however small: labels too small to read are left out.
      fit: () => view(cy, cy.elements(), null, true, true),
      relayout: () => {
        cy.layout(LAYOUT as unknown as cytoscape.LayoutOptions).run();
        view(cy, cy.elements(), focus(), false);
      },
      png: () => cy.png({ full: true, scale: 2, bg: readPalette().surface }),
      focus: (id: string) => {
        const el = cy.getElementById(id);
        if (el.empty()) return false;
        view(cy, el, el, true);
        return true;
      },
    };
  }, [display, controls]);

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
      signature.current = "";
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

  return <div ref={host} className="canvas" aria-label="Trace graph" role="img" />;
}
