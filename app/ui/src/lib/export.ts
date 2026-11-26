// Exports: the full report (JSON), leads (CSV) and the graph (PNG).

import type { TraceView } from "../api/types";
import { csvCell, formatBtc, formatShare, formatUtc } from "./format";
import { categoryName, describeClaim, leadKindName } from "./meaning";

function download(name: string, data: BlobPart, type: string) {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function baseName(view: TraceView): string {
  const seed = view.report.seed.replace(/[^0-9a-zA-Z]+/g, "-").slice(0, 24);
  return `chaintrail-${view.report.direction}-${seed}`;
}

export function exportJson(view: TraceView) {
  download(`${baseName(view)}.json`, JSON.stringify(view, null, 2), "application/json");
}

export function leadsCsv(view: TraceView): string {
  const r = view.report;
  const header = [
    "lead",
    "kind",
    "category",
    "address",
    "transaction",
    "amount_btc",
    "share",
    "hops",
    "first_seen_utc",
    "confidence",
    "basis",
    "evidence_or_source",
  ];
  const rows = r.leads.map((l) => {
    const c = describeClaim(l.claim);
    return [
      l.title,
      leadKindName(l.kind),
      l.category ? categoryName(l.category) : "",
      l.address?.value ?? "",
      l.txid ?? "",
      formatBtc(l.amount, 8),
      formatShare(l.amount, r.seed_amount),
      l.hops,
      l.first_time !== null ? formatUtc(l.first_time) : "",
      l.confidence,
      c.kind,
      [...c.evidence, c.url ?? ""].filter(Boolean).join(" | "),
    ];
  });
  return [header, ...rows].map((row) => row.map(csvCell).join(",")).join("\r\n") + "\r\n";
}

export function exportCsv(view: TraceView) {
  download(`${baseName(view)}-leads.csv`, leadsCsv(view), "text/csv");
}

export function exportPng(view: TraceView, dataUrl: string) {
  const bin = atob(dataUrl.split(",")[1] ?? "");
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  download(`${baseName(view)}.png`, bytes, "image/png");
}
