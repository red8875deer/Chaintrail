// Graph legend: shape + colour per meaning, and how edges are drawn.

import type { TraceView } from "../api/types";
import { NODE_SHAPES } from "../lib/graph";
import { NODE_STYLES, type NodeStyleKey, nodeStyleKey } from "../lib/meaning";

function points(n: number, r: number, rot = -Math.PI / 2): string {
  return Array.from({ length: n }, (_, i) => {
    const a = rot + (i * 2 * Math.PI) / n;
    return `${8 + r * Math.cos(a)},${8 + r * Math.sin(a)}`;
  }).join(" ");
}

export function ShapeIcon({ k }: { k: NodeStyleKey }) {
  const fill = `var(--n-${k})`;
  const shape = NODE_SHAPES[k];
  const common = { fill, stroke: "var(--surface)", strokeWidth: 1 };
  const dashed = k === "unidentified" ? { stroke: "var(--ink)", strokeDasharray: "2 1.5" } : {};
  const rest =
    k === "at_rest" ? { fillOpacity: 0.35, stroke: fill, strokeWidth: 2.5 } : k === "unexplored" ? { fillOpacity: 0.35, stroke: "var(--muted)", strokeDasharray: "1 1.5" } : {};
  let el;
  switch (shape) {
    case "diamond":
      el = <polygon points="8,1 15,8 8,15 1,8" {...common} />;
      break;
    case "octagon":
      el = <polygon points={points(8, 7, Math.PI / 8)} {...common} />;
      break;
    case "triangle":
      el = <polygon points="8,1.5 15,14.5 1,14.5" {...common} />;
      break;
    case "hexagon":
      el = <polygon points={points(6, 7, 0)} {...common} />;
      break;
    case "round-pentagon":
      el = <polygon points={points(5, 7)} {...common} />;
      break;
    case "barrel":
      el = <rect x="2" y="1.5" width="12" height="13" rx="5" {...common} />;
      break;
    case "round-tag":
      el = <polygon points="1,3 11,3 15,8 11,13 1,13" {...common} />;
      break;
    case "round-rectangle":
      el = <rect x="1.5" y="3" width="13" height="10" rx="2.5" {...common} {...dashed} />;
      break;
    default:
      el = <circle cx="8" cy="8" r="6" {...common} {...rest} />;
  }
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      {el}
    </svg>
  );
}

// Closed by default so it does not cover the graph; the choice is remembered.
const LEGEND_KEY = "chaintrail.legend.open";
function legendOpen(): boolean {
  try {
    return window.localStorage.getItem(LEGEND_KEY) === "1";
  } catch {
    return false;
  }
}
function rememberLegend(open: boolean) {
  try {
    window.localStorage.setItem(LEGEND_KEY, open ? "1" : "0");
  } catch {
    // Storage blocked: the legend just opens closed next time.
  }
}

export function Legend({ view }: { view: TraceView }) {
  const present = new Set(view.graph.nodes.map(nodeStyleKey));
  const keys = (Object.keys(NODE_STYLES) as NodeStyleKey[]).filter((k) => present.has(k));
  const hasChange = view.graph.edges.some((e) => e.role.role === "change");
  return (
    <details
      className="legend"
      open={legendOpen()}
      onToggle={(e) => rememberLegend((e.currentTarget as HTMLDetailsElement).open)}
    >
      <summary>Legend</summary>
      <ul>
        {keys.map((k) => (
          <li key={k} title={NODE_STYLES[k].description}>
            <ShapeIcon k={k} />
            {NODE_STYLES[k].name}
          </li>
        ))}
        <li>
          <svg width="16" height="16" aria-hidden="true">
            <circle cx="8" cy="8" r="5.5" fill="var(--n-address)" stroke="var(--muted)" strokeWidth="2.5" strokeDasharray="0" />
            <circle cx="8" cy="8" r="7.2" fill="none" stroke="var(--muted)" strokeWidth="1" />
          </svg>
          Group of smaller branches (click to open)
        </li>
        <li>
          <svg width="16" height="16" aria-hidden="true">
            <rect x="1" y="5" width="14" height="6" rx="3" fill="var(--surface)" stroke="var(--trail)" strokeWidth="1.5" strokeDasharray="2.5 1.5" />
          </svg>
          Addresses in a row (click to open)
        </li>
        <li>
          <svg width="16" height="16" aria-hidden="true">
            <line x1="1" y1="8" x2="15" y2="8" stroke="var(--trail)" strokeWidth="2.5" />
          </svg>
          Funds moved (thicker = more; "(3 tx)" = 3 transfers combined)
        </li>
        {hasChange && (
          <li>
            <svg width="16" height="16" aria-hidden="true">
              <line x1="1" y1="8" x2="15" y2="8" stroke="var(--trail)" strokeWidth="2.5" strokeDasharray="4 2" />
            </svg>
            Likely change back to the sender (inferred)
          </li>
        )}
        <li>
          <svg width="16" height="16" aria-hidden="true">
            <rect x="2" y="3" width="12" height="10" rx="2.5" fill="none" stroke="var(--ink)" strokeWidth="1.5" strokeDasharray="2 1.5" />
          </svg>
          Dashed outline: identified by inference
        </li>
        <li>
          <svg width="16" height="16" aria-hidden="true">
            <line x1="1" y1="8" x2="15" y2="8" stroke="var(--amber)" strokeWidth="3" />
          </svg>
          Path of the selected lead
        </li>
      </ul>
    </details>
  );
}
