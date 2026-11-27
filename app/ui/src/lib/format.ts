// Formatting helpers. Amounts are integer satoshis; all maths here is integer (BigInt) so
// nothing is lost to floating-point rounding. Times are Unix seconds, always shown in UTC.

const SATS_PER_BTC = 100_000_000n;

/** Accepts a sats amount as number (must be an integer), bigint or decimal string. */
export function toSats(value: number | bigint | string): bigint {
  if (typeof value === "bigint") return value;
  if (typeof value === "number") {
    if (!Number.isInteger(value)) throw new RangeError(`not an integer amount: ${value}`);
    return BigInt(value);
  }
  if (!/^-?\d+$/.test(value)) throw new RangeError(`not an integer amount: ${value}`);
  return BigInt(value);
}

function groupThousands(digits: string): string {
  return digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

export interface BtcParts {
  sign: "" | "-";
  /** Integer part, grouped with commas. */
  whole: string;
  /** Decimals up to the last significant digit (at least `minDecimals`). */
  decimals: string;
  /** Trailing zeros that complete the 8 decimals (shown faint in the UI). */
  zeros: string;
}

/** Split an amount into display parts. `75.00000000` → whole 75, decimals "00", zeros "000000". */
export function btcParts(value: number | bigint | string, minDecimals = 2): BtcParts {
  const sats = toSats(value);
  const sign = sats < 0n ? "-" : "";
  const abs = sats < 0n ? -sats : sats;
  const whole = groupThousands((abs / SATS_PER_BTC).toString());
  const frac = (abs % SATS_PER_BTC).toString().padStart(8, "0");
  let end = frac.length;
  while (end > minDecimals && frac[end - 1] === "0") end--;
  return { sign, whole, decimals: frac.slice(0, end), zeros: frac.slice(end) };
}

/**
 * Sats → BTC string with up to 8 decimals, trailing zeros trimmed (keeping `minDecimals`).
 * 7499998307 → "74.99998307", 100000000 → "1.00", 1 → "0.00000001".
 */
export function formatBtc(value: number | bigint | string, minDecimals = 2): string {
  const p = btcParts(value, minDecimals);
  return `${p.sign}${p.whole}${p.decimals ? "." + p.decimals : ""}`;
}

/** Same as `formatBtc` but with an explicit sign for non-zero amounts ("+0.5", "-1.2"). */
export function formatBtcSigned(value: number | bigint | string): string {
  const s = toSats(value);
  const text = formatBtc(s);
  return s > 0n ? `+${text}` : text;
}

/**
 * Parse a BTC amount typed by a person ("0.5", "1,000.25", ".001") into sats.
 * Returns null if it is not a non-negative number with at most 8 decimals.
 */
export function parseBtc(input: string): bigint | null {
  let s = input.trim().replace(/\s/g, "");
  // Commas: thousands separators ("1,000.5", "12,345") or a decimal comma ("0,5").
  // Anything else with a comma is ambiguous and rejected rather than guessed.
  if (s.includes(",")) {
    if (/^[1-9]\d{0,2}(,\d{3})+(\.\d*)?$/.test(s)) s = s.replace(/,/g, "");
    else if (!s.includes(".") && /^\d+,\d{1,8}$/.test(s)) s = s.replace(",", ".");
    else return null;
  }
  const m = /^(\d*)(?:\.(\d{0,8}))?$/.exec(s);
  if (!m || (m[1] === "" && (m[2] ?? "") === "")) return null;
  const whole = BigInt(m[1] || "0");
  const frac = BigInt((m[2] ?? "").padEnd(8, "0") || "0");
  return whole * SATS_PER_BTC + frac;
}

/** Share of `part` in `total` in basis points (1/100 of a percent), rounded half up. */
export function shareBasisPoints(part: number | bigint, total: number | bigint): bigint {
  const p = toSats(part);
  const t = toSats(total);
  if (t <= 0n) return 0n;
  return (p * 20_000n + t) / (2n * t);
}

/**
 * Share as a percentage string with two decimals, from integers only.
 * 5828827793 of 7499998307 → "77.72%". Non-zero shares under 0.005% → "<0.01%".
 */
export function formatShare(part: number | bigint, total: number | bigint): string {
  const t = toSats(total);
  if (t <= 0n) return "–";
  const p = toSats(part);
  const bp = shareBasisPoints(p, t);
  if (bp === 0n && p > 0n) return "<0.01%";
  const whole = bp / 100n;
  const frac = (bp % 100n).toString().padStart(2, "0");
  return `${whole}.${frac}%`;
}

/** Group an integer with commas: 48581 → "48,581". */
export function formatInt(n: number | bigint): string {
  const s = typeof n === "bigint" ? n.toString() : Math.trunc(n).toString();
  return s.startsWith("-") ? "-" + groupThousands(s.slice(1)) : groupThousands(s);
}

function pad2(n: number): string {
  return n.toString().padStart(2, "0");
}

/** Unix seconds → "2021-05-08 18:21 UTC". */
export function formatUtc(seconds: number | null | undefined, fallback = "–"): string {
  if (seconds === null || seconds === undefined) return fallback;
  const d = new Date(seconds * 1000);
  return (
    `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())} ` +
    `${pad2(d.getUTCHours())}:${pad2(d.getUTCMinutes())} UTC`
  );
}

/** Unix seconds → "2021-05-08" (UTC). */
export function formatUtcDate(seconds: number | null | undefined, fallback = "–"): string {
  if (seconds === null || seconds === undefined) return fallback;
  const d = new Date(seconds * 1000);
  return `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())}`;
}

/** Shorten a long id: "fc78327d…f1b264". Short ids are returned unchanged. */
export function shortId(id: string, head = 8, tail = 6): string {
  if (id.length <= head + tail + 2) return id;
  return `${id.slice(0, head)}…${id.slice(-tail)}`;
}

/** Elapsed milliseconds → "0:07", "1:05". */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${pad2(s % 60)}`;
}

/** Quote a value for CSV (RFC 4180). */
export function csvCell(v: string | number | bigint | null | undefined): string {
  if (v === null || v === undefined) return "";
  let s = String(v);
  // Text starting like a formula (=, +, -, @) would run in a spreadsheet: label data comes
  // from third parties, so it is made plain text.
  if (typeof v === "string" && /^[=+\-@\t\r]/.test(s)) s = `'${s}`;
  return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/**
 * Compact BTC for graph labels: 2 decimals at ≥ 1 BTC, 4 at ≥ 0.0001 BTC, otherwise every
 * significant digit. Rounds half up with integers only. 6369998561 → "63.70".
 */
export function formatBtcShort(value: number | bigint | string): string {
  const sats = toSats(value);
  const abs = sats < 0n ? -sats : sats;
  const sign = sats < 0n ? "-" : "";
  const decimals = abs >= SATS_PER_BTC ? 2 : abs >= 10_000n ? 4 : 8;
  const unit = 10n ** BigInt(8 - decimals);
  const rounded = ((abs + unit / 2n) / unit) * unit;
  const whole = groupThousands((rounded / SATS_PER_BTC).toString());
  let frac = (rounded % SATS_PER_BTC).toString().padStart(8, "0").slice(0, decimals);
  // Small amounts keep all 8 decimals (exact sats), so "0.00000540" and "0.00000546" align.
  return `${sign}${whole}${frac ? "." + frac : ""}`;
}

/** A transaction id or address inside pasted text, e.g. an explorer link
 * ("https://mempool.space/tx/<txid>", ".../address/<addr>"). Plain ids pass through. */
export function extractId(input: string): string {
  const s = input.trim();
  const tx = /(?:^|[^0-9a-fA-F])([0-9a-fA-F]{64})(?:$|[^0-9a-fA-F])/.exec(s);
  if (tx) return tx[1]!;
  const addr = /\/address\/([A-Za-z0-9]{25,90})/.exec(s);
  if (addr) return addr[1]!;
  return s;
}
