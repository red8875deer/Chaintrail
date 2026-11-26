// The trace graph: Cytoscape with a left-to-right dagre layout (time flows to the right).

import { type RefObject, useEffect, useRef } from "react";
import cytoscape, { type Core } from "cytoscape";
import dagre from "cytoscape-dagre";
import type { TraceView } from "../api/types";
import { type Palette, elementsFor, graphStyle, leadPath, smallEdges } from "../lib/graph";
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

const LAYOUT = {
  name: "dagre",
  rankDir: "LR",
  nodeSep: 34,
  rankSep: 130,
  edgeSep: 14,
  padding: 40,
  fit: true,
  animate: false,
} as const;

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

export function GraphCanvas({
  view,
  selectedLead,
  selected,
  hideBelowBp,
  onSelect,
  controls,
}: {
  view: TraceView;
  selectedLead: number | null;
  selected: Selection;
  hideBelowBp: number;
  onSelect: (s: Selection) => void;
  controls: RefObject<GraphControls | null>;
}) {
  const host = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;

  // Build the graph when the trace changes.
  useEffect(() => {
    if (!host.current) return;
    const cy = cytoscape({
      container: host.current,
      elements: elementsFor(view),
      style: graphStyle(readPalette()),
      layout: LAYOUT as cytoscape.LayoutOptions,
      minZoom: 0.15,
      maxZoom: 3,
      boxSelectionEnabled: false,
    });
    cy.on("tap", "node", (e) => onSelectRef.current({ type: "node", id: e.target.id() }));
    cy.on("tap", "edge", (e) => onSelectRef.current({ type: "edge", id: e.target.id() }));
    cy.on("tap", (e) => {
      if (e.target === cy) onSelectRef.current(null);
    });
    cyRef.current = cy;
    controls.current = {
      fit: () => cy.fit(cy.elements(":visible"), 40),
      relayout: () => cy.layout(LAYOUT as cytoscape.LayoutOptions).run(),
      png: () => cy.png({ full: true, scale: 2, bg: readPalette().surface }),
      focus: (id: string) => {
        const el = cy.getElementById(id);
        if (el.empty()) return false;
        cy.animate({ center: { eles: el }, zoom: Math.max(cy.zoom(), 1) }, { duration: reducedMotion() ? 0 : 250 });
        return true;
      },
    };
    // Follow the system theme.
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const restyle = () => cy.style(graphStyle(readPalette()));
    mq?.addEventListener("change", restyle);
    return () => {
      mq?.removeEventListener("change", restyle);
      controls.current = null;
      cy.destroy();
      cyRef.current = null;
    };
  }, [view, controls]);

  // Highlight the selected lead's path and dim everything else.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.batch(() => {
      cy.elements().removeClass("path dim");
      if (selectedLead === null) return;
      const p = leadPath(view, selectedLead);
      cy.elements().forEach((el) => {
        const id: string = el.id();
        const on = el.group() === "nodes" ? p.nodes.has(id) : p.edges.has(id);
        el.addClass(on ? "path" : "dim");
      });
    });
    if (selectedLead !== null) {
      const eles = cy.elements(".path");
      if (eles.nonempty()) {
        cy.animate({ fit: { eles, padding: 80 } }, { duration: reducedMotion() ? 0 : 300 });
      }
    }
  }, [view, selectedLead]);

  // Hide small flows (and nodes left with nothing visible, unless they are leads).
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    const hide = smallEdges(view, hideBelowBp);
    cy.batch(() => {
      cy.elements().removeClass("hidden");
      cy.edges().forEach((e) => {
        if (hide.has(e.id())) e.addClass("hidden");
      });
      cy.nodes().forEach((n) => {
        if (n.data("isLead") || n.hasClass("seed")) return;
        const visible = n.connectedEdges().filter((e) => !e.hasClass("hidden"));
        if (visible.empty()) n.addClass("hidden");
      });
    });
  }, [view, hideBelowBp]);

  // Reflect the selection.
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.elements().unselect();
    if (selected) cy.getElementById(selected.id).select();
  }, [selected, view]);

  return <div ref={host} className="canvas" aria-label="Trace graph" role="img" />;
}
