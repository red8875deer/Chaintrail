// Exports: the full report (JSON), leads (CSV) and the graph (PNG).

import { call } from "../api/client";
import type { StatusView, TraceView } from "../api/types";
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

/** Who made an export, when, from what data and with which settings. */
export interface Provenance {
  generated_at: string;
  app: string;
  app_version: string | null;
  data_source: string | null;
  label_data: { packs: number; labelled_addresses: number } | null;
  settings: string;
  notes: string[];
}

async function status(): Promise<StatusView | null> {
  try {
    return await call("status", {});
  } catch {
    return null;
  }
}

/** The trace's settings in words, e.g. "forward from …, max hops 8, …". */
export function settingsText(view: TraceView): string {
  const r = view.report;
  const c = r.config;
  const parts = [
    `${r.direction} from ${r.seed}`,
    `traced amount ${formatBtc(r.seed_amount, 8)} BTC`,
    `max hops ${c.max_hops}`,
    `max transactions ${c.max_txs}`,
    `branches under ${formatBtc(r.min_amount, 8)} BTC not followed`,
    `address checks ${c.check_addresses ? "on" : "off"}`,
    `allocation ${r.allocation}`,
  ];
  if (c.seed_share !== null && c.seed_share !== undefined)
    parts.push(
      c.continued_from
        ? `continued from the trace of ${c.continued_from}, following only its share of the seed`
        : `continued from an earlier trace, following only its share of the seed`,
    );
  else if (c.continued_from) parts.push(`continued from the trace of ${c.continued_from}`);
  if (c.mixed_in) parts.push(`funds already mixed in ${c.mixed_in}`);
  return parts.join("; ");
}

export function provenance(view: TraceView, s: StatusView | null, generated = new Date()): Provenance {
  return {
    generated_at: formatUtc(generated.getTime() / 1000),
    app: "ChainTrail",
    app_version: s?.version ?? null,
    data_source: s?.provider ?? null,
    label_data: s ? { packs: s.packs, labelled_addresses: s.labelled_addresses } : null,
    settings: settingsText(view),
    notes: [
      "Leads are leads, not proof: anything inferred or taken from a label is marked as such.",
      "Where traced funds were mixed with other funds, amounts are pro-rata shares (see mixed_in on each lead).",
    ],
  };
}

export async function exportJson(view: TraceView) {
  const meta = provenance(view, await status());
  download(`${baseName(view)}.json`, JSON.stringify({ chaintrail_export: meta, ...view }, null, 2), "application/json");
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
    "coin",
    "path",
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
      l.txid && l.vout !== null && l.vout !== undefined ? `${l.txid}:${l.vout}` : "",
      l.path.join(" > "),
      [...c.evidence, c.url ?? ""].filter(Boolean).join(" | "),
    ];
  });
  return [header, ...rows].map((row) => row.map(csvCell).join(",")).join("\r\n") + "\r\n";
}

/** The CSV with a few "#" lines of provenance before the header row. */
export function leadsCsvWithProvenance(view: TraceView, meta: Provenance): string {
  const lines = [
    `# ChainTrail leads export, generated ${meta.generated_at}${meta.app_version ? `, app ${meta.app_version}` : ""}`,
    `# Trace: ${meta.settings}`,
    `# Data: ${meta.data_source ?? "unknown source"}${meta.label_data ? `; labels: ${meta.label_data.packs} packs, ${meta.label_data.labelled_addresses} addresses` : ""}`,
    ...meta.notes.map((n) => `# ${n}`),
  ];
  return lines.map((l) => csvCell(l)).join("\r\n") + "\r\n" + leadsCsv(view);
}

export function exportCsv(view: TraceView) {
  void status().then((s) => {
    // A byte-order mark so spreadsheet programs read the file as UTF-8 ("Zürich").
    const csv = leadsCsvWithProvenance(view, provenance(view, s));
    download(`${baseName(view)}-leads.csv`, "\uFEFF" + csv, "text/csv;charset=utf-8");
  });
}

/** Saves the graph image with a header (what was traced, when, what is hidden); returns
 * false if there is nothing to save. */
export function exportPng(view: TraceView, dataUrl: string, drawnNote: string): boolean {
  const data = dataUrl.split(",")[1] ?? "";
  if (!data) return false;
  void (async () => {
    const meta = provenance(view, await status());
    const img = new Image();
    img.src = dataUrl;
    await img.decode();
    const lines = [
      `ChainTrail trace: ${view.report.direction} from ${view.report.seed}`,
      `Traced amount ${formatBtc(view.report.seed_amount, 8)} BTC. Generated ${meta.generated_at}${meta.app_version ? ` with ChainTrail ${meta.app_version}` : ""}${meta.data_source ? ` from ${meta.data_source}` : ""}.`,
      drawnNote,
      "Leads are leads, not proof. Amounts after mixing are pro-rata shares. Dashed outlines: identified by inference.",
    ].filter(Boolean);
    const scale = Math.max(1, Math.min(2, img.width / 1200));
    const lineH = 22 * scale;
    const width = Math.max(img.width, Math.round(1200 * scale));
    const font = (bold: boolean) => `${bold ? "bold " : ""}${Math.round(15 * scale)}px "Atkinson Hyperlegible Next", sans-serif`;
    // Wrap each line to the image width.
    const measure = document.createElement("canvas").getContext("2d");
    if (!measure) return;
    const wrapped: { text: string; bold: boolean }[] = [];
    lines.forEach((t, k) => {
      measure.font = font(k === 0);
      let row = "";
      for (const word of t.split(" ")) {
        const next = row ? `${row} ${word}` : word;
        if (row && measure.measureText(next).width > width - 32 * scale) {
          wrapped.push({ text: row, bold: k === 0 });
          row = word;
        } else row = next;
      }
      wrapped.push({ text: row, bold: k === 0 });
    });
    const head = Math.round((wrapped.length + 1) * lineH);
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = img.height + head;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const css = getComputedStyle(document.documentElement);
    ctx.fillStyle = css.getPropertyValue("--surface").trim() || "#ffffff";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = css.getPropertyValue("--ink").trim() || "#000000";
    wrapped.forEach((l, k) => {
      ctx.font = font(l.bold);
      ctx.fillText(l.text, 16 * scale, (k + 1) * lineH);
    });
    ctx.drawImage(img, 0, head);
    canvas.toBlob((blob) => {
      if (blob) download(`${baseName(view)}.png`, blob, "image/png");
    }, "image/png");
  })();
  return true;
}
