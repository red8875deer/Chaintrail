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
  const parts = view.report.seed.split(/[^0-9a-zA-Z]+/).filter(Boolean);
  const seed = [parts[0]?.slice(0, 16), ...parts.slice(1).map((p) => p.slice(0, 12))].filter(Boolean).join("-");
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
    "amount_basis",
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
      l.mixed_in ? `pro-rata share (funds mixed in ${l.mixed_in})` : "on-chain (unmixed)",
      [...c.evidence, c.url ?? ""].filter(Boolean).join(" | "),
    ];
  });
  return [header, ...rows].map((row) => row.map(csvCell).join(",")).join("\r\n") + "\r\n";
}

export function exportCsv(view: TraceView) {
  download(`${baseName(view)}-leads.csv`, leadsCsv(view), "text/csv");
}

/** Saves the graph image; returns false if there is nothing to save. */
export function exportPng(view: TraceView, dataUrl: string): boolean {
  const data = dataUrl.split(",")[1] ?? "";
  if (!data) return false;
  const bin = atob(data);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  download(`${baseName(view)}.png`, bytes, "image/png");
  return true;
}
