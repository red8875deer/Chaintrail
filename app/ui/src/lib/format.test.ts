import { describe, expect, it } from "vitest";
import { btcParts, formatBtc, formatBtcShort, formatShare, parseBtc, shortId, csvCell } from "./format";

describe("BTC formatting (integer maths only)", () => {
  it("formats sats exactly", () => {
    expect(formatBtc(7_499_998_307)).toBe("74.99998307");
    expect(formatBtc(100_000_000)).toBe("1.00");
    expect(formatBtc(1)).toBe("0.00000001");
    expect(formatBtc(-150_000)).toBe("-0.0015");
    expect(formatBtc("2100000000000000")).toBe("21,000,000.00");
  });
  it("splits faint trailing zeros", () => {
    expect(btcParts(7_500_000_000)).toEqual({ sign: "", whole: "75", decimals: "00", zeros: "000000" });
  });
  it("rounds compact labels half up", () => {
    expect(formatBtcShort(6_369_998_561)).toBe("63.70");
    expect(formatBtcShort(4_976_631)).toBe("0.0498");
    expect(formatBtcShort(1_693)).toBe("0.00001693");
  });
  it("parses typed amounts", () => {
    expect(parseBtc("0.5")).toBe(50_000_000n);
    expect(parseBtc("1,000.25")).toBe(100_025_000_000n);
    expect(parseBtc(".00000001")).toBe(1n);
    expect(parseBtc("0.000000001")).toBeNull();
    expect(parseBtc("abc")).toBeNull();
  });
  it("computes shares from integers", () => {
    expect(formatShare(5_828_827_793, 7_499_998_307)).toBe("77.72%");
    expect(formatShare(1, 3)).toBe("33.33%");
    expect(formatShare(1, 1_000_000)).toBe("<0.01%");
    expect(formatShare(1, 0)).toBe("–");
  });
  it("shortens ids and quotes CSV", () => {
    expect(shortId("fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264")).toBe("fc78327d…f1b264");
    expect(csvCell('a "b", c')).toBe('"a ""b"", c"');
  });
});
