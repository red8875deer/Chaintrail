import { describe, expect, it } from "vitest";
import { href, parseRoute, sameTrace } from "./route";

const T = "fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264";

describe("trace links", () => {
  it("round-trip through href and parseRoute, including run", () => {
    const params = { txid: T, vout: 1, direction: "forward" as const, maxHops: 8, run: true };
    const r = parseRoute(href({ page: "trace", params }));
    expect(r.page).toBe("trace");
    if (r.page === "trace") expect(r.params).toMatchObject(params);
  });

  it("treat links that differ only in order, case, defaults or run as the same trace", () => {
    const a = parseRoute(`#/trace?dir=forward&txid=${T.toUpperCase()}&vout=1&run=1`);
    const b = parseRoute(`#/trace?txid=${T}&vout=1&dir=forward&hops=8&txs=100`);
    expect(a.page === "trace" && b.page === "trace" && sameTrace(a.params!, b.params!)).toBe(true);
  });

  it("tell different traces apart", () => {
    const base = { txid: T, direction: "forward" as const };
    expect(sameTrace(base, { ...base, vout: 1 })).toBe(false);
    expect(sameTrace(base, { ...base, direction: "backward" })).toBe(false);
    expect(sameTrace(base, { ...base, maxHops: 12 })).toBe(false);
  });
});
