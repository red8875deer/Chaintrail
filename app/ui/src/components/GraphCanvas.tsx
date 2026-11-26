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

/** Below this zoom the labels (11–12px) render under ~9px; never open smaller than this.
 * "Fit" can still show the whole graph as an overview. */
const MIN_READABLE_ZOOM = 0.75;
const MAX_FIT_ZOOM = 1.4;
/** Room around the graph; more at the top (notice) and bottom (toolbar, legend). */
const PAD = { top: 104, bottom: 96, left: 48, right: 48 };

const LAYOUT = {
  name: "dagre",
  rankDir: "LR",
  nodeSep: 46,
  rankSep: 130,
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
  const target = zoom > fitZoom && focus && focus.nonempty() ? focus : eles;
  const tb = target.boundingBox();
  const pan = {
    x: PAD.left + w / 2 - zoom * (tb.x1 + tb.w / 2),
    y: PAD.top + h / 2 - zoom * (tb.y1 + tb.h / 2),
  };
  if (animate && !reducedMotion()) cy.animate({ zoom, pan }, { duration: 300 });
  else cy.viewport({ zoom, pan });
}

export function GraphCanvas({
  display,
  path,
  focusId,
  selected,
  onSelect,
  controls,
}: {
  display: Display;
  /** Displayed ids on the selected lead's path. */
  path: { nodes: Set<string>; edges: Set<string> } | null;
  /** Displayed node to keep in view (the selected lead). */
  focusId: string | null;
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

  // Rebuild only when the drawn elements change (not on every selection).
  useEffect(() => {
    if (!host.current) return;
    const sig = display.elements.map((e) => e.data.id).join("|");
    if (sig === signature.current && cyRef.current) return;
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

  // Reflect the selection.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.elements().unselect();
    if (selected) cy.getElementById(selected.id).select();
  }, [selected, display]);

  return <div ref={host} className="canvas" aria-label="Trace graph" role="img" />;
}
