import { describe, expect, it } from "vitest";
import type { StatusView, TraceView } from "../api/types";
import forward from "../../fixtures/btc_trace_forward.json";
import status from "../../fixtures/status.json";
import { leadsCsv, leadsCsvWithProvenance, plainBtc, plainShare, provenance } from "./export";

const view = forward as unknown as TraceView;

describe("exports carry provenance", () => {
  const meta = provenance(view, status as unknown as StatusView, new Date(Date.UTC(2026, 10, 27, 3, 0)));

  it("says when, with what and how the trace was made", () => {
    expect(meta.generated_at).toBe("2026-11-27 03:00 UTC");
    expect(meta.app_version).toBe((status as StatusView).version);
    expect(meta.settings).toContain(view.report.seed);
    expect(meta.settings).toContain(`max hops ${view.report.config.max_hops}`);
  });

  it("has the header on the first row, plain numbers, and provenance on every row", () => {
    const csv = leadsCsvWithProvenance(view, meta);
    const lines = csv.trim().split("\r\n");
    expect(lines[0]).toMatch(/^lead,kind,category,/);
    expect(lines[0]).toContain("generated_utc");
    const extra = (view.report.stats.fees > 0 ? 1 : 0) + (view.report.stats.below_threshold > 0 ? 1 : 0);
    expect(lines.length).toBe(view.report.leads.length + 1 + extra);
    // Every lead row names its full path, the time and the trace it came from.
    for (const l of view.report.leads) expect(csv).toContain(l.path.join(" > "));
    for (const row of lines.slice(1)) expect(row).toContain("2026-11-27 03:00 UTC");
    // Without provenance the rows are the same, minus its columns.
    expect(leadsCsv(view).trim().split("\r\n").length).toBe(lines.length);
  });

  it("adds up to the traced amount", () => {
    const rows = leadsCsv(view).trim().split("\r\n").slice(1);
    const sats = rows.reduce((s, row) => {
      const m = /,(\d+)\.(\d{8}),/.exec(row);
      return s + (m ? Number(m[1]) * 100_000_000 + Number(m[2]) : 0);
    }, 0);
    expect(sats).toBe(view.report.seed_amount);
  });

  it("writes amounts and shares as plain numbers", () => {
    expect(plainBtc(6_936_902_357_000)).toBe("69369.02357000");
    expect(plainBtc(1)).toBe("0.00000001");
    expect(plainShare(1, 3)).toBe("0.333333");
    expect(plainShare(5, 5)).toBe("1.000000");
    expect(plainShare(0, 0)).toBe("0.000000");
  });
});
