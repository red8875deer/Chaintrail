import { describe, expect, it } from "vitest";
import { btcParts, formatBtc, formatBtcShort, formatShare, looksLikeAddress, parseBtc, shortId, shortSeed, csvCell } from "./format";

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

describe("parseBtc with commas", () => {
  it("reads a decimal comma and thousands separators, and rejects ambiguous input", async () => {
    const { parseBtc } = await import("./format");
    expect(parseBtc("0,5")).toBe(50_000_000n);
    expect(parseBtc("1,000")).toBe(100_000_000_000n);
    expect(parseBtc("1,000.5")).toBe(100_050_000_000n);
    expect(parseBtc("0,500,1")).toBeNull();
    expect(parseBtc("1,0.5")).toBeNull();
  });
});

describe("extractId", () => {
  it("finds a txid or address in an explorer link", async () => {
    const { extractId } = await import("./format");
    const t = "fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264";
    expect(extractId(`https://mempool.space/tx/${t}#vout=1`)).toBe(t);
    expect(extractId(`  ${t} `)).toBe(t);
    expect(extractId("https://mempool.space/address/bc1qnhx6u3af9qyyy5rrtfldlhjxuayp4axscqqf3r")).toBe(
      "bc1qnhx6u3af9qyyy5rrtfldlhjxuayp4axscqqf3r",
    );
    expect(extractId("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")).toBe("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
  });
});

describe("csvCell", () => {
  it("neutralises spreadsheet formulas in text", async () => {
    const { csvCell } = await import("./format");
    expect(csvCell('=HYPERLINK("http://x")')).toBe(`"'=HYPERLINK(""http://x"")"`);
    expect(csvCell("@cmd")).toBe("'@cmd");
    expect(csvCell("Zürich, CH")).toBe('"Zürich, CH"');
    expect(csvCell(-5)).toBe("-5");
  });
});

describe("looksLikeAddress", () => {
  it("tells addresses from transaction ids", () => {
    expect(looksLikeAddress("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2")).toBe(true);
    expect(looksLikeAddress("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy")).toBe(true);
    expect(looksLikeAddress("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq")).toBe(true);
    expect(looksLikeAddress("3f036ff88bb851b57a1e28780dbce35a6457a8b57995c095b55b3b0cf48ba9fd")).toBe(false);
    expect(looksLikeAddress("1f036ff88bb851b57a1e28780dbce35a6457a8b57995c095b55b3b0cf48ba9fd")).toBe(false);
    expect(looksLikeAddress("not an id")).toBe(false);
  });
});

describe("shortSeed", () => {
  it("shortens the ids in a trace seed", () => {
    const tx = "fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264";
    expect(shortSeed(`${tx}:1`)).toBe("fc78327d…f1b264:1");
    expect(shortSeed(`${tx} → bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq`)).toBe("fc78327d…f1b264 → bc1qar0s…zwf5mdq".replace("zwf5mdq", "wf5mdq"));
  });
});
