import { describe, expect, it } from "vitest";
import type { StatusView, TraceView } from "../api/types";
import forward from "../../fixtures/btc_trace_forward.json";
import status from "../../fixtures/status.json";
import { csvComment, leadsCsv, leadsCsvWithProvenance, provenance } from "./export";

const view = forward as unknown as TraceView;

describe("exports carry provenance", () => {
  const meta = provenance(view, status as unknown as StatusView, new Date(Date.UTC(2026, 10, 27, 3, 0)));

  it("says when, with what and how the trace was made", () => {
    expect(meta.generated_at).toBe("2026-11-27 03:00 UTC");
    expect(meta.app_version).toBe((status as StatusView).version);
    expect(meta.settings).toContain(view.report.seed);
    expect(meta.settings).toContain(`max hops ${view.report.config.max_hops}`);
  });

  it("puts it above the CSV header without changing the rows", () => {
    const csv = leadsCsvWithProvenance(view, meta);
    const lines = csv.trim().split("\r\n");
    const comments = lines.filter((l) => l.startsWith("#"));
    expect(comments.length).toBeGreaterThanOrEqual(3);
    // One plain cell each: never quoted, no commas.
    for (const c of comments) expect(c).not.toMatch(/[",]/);
    expect(lines.slice(comments.length).join("\r\n")).toBe(leadsCsv(view).trim());
    // Every lead row names its full path of transactions.
    for (const l of view.report.leads) expect(csv).toContain(l.path.join(" > "));
  });

  it("writes comment lines as one unquoted cell", () => {
    expect(csvComment('traced amount 1,234,567.5 BTC, max hops 8, "x"\nnext')).toBe(
      "# traced amount 1234567.5 BTC; max hops 8; 'x' next",
    );
  });
});
