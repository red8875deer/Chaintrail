// Small building blocks used everywhere: amounts, trust markers, ids, errors.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Claim, Confidence, EntityCategory } from "../api/types";
import { ApiError } from "../api/client";
import { btcParts, shortId } from "../lib/format";
import {
  CONFIDENCE_MEANING,
  CONFIDENCE_NAMES,
  CONFIDENCE_RANK,
  categoryName,
  categoryStyle,
  describeClaim,
} from "../lib/meaning";

/** A BTC amount from sats; trailing zeros to 8 decimals shown faint. */
export function Btc({ sats, unit = true, signed = false }: { sats: number; unit?: boolean; signed?: boolean }) {
  const p = btcParts(sats);
  const cls = signed ? (sats > 0 ? "btc pos" : sats < 0 ? "btc neg" : "btc") : "btc";
  return (
    <span className={cls}>
      {signed && sats > 0 ? "+" : ""}
      {p.sign}
      {p.whole}.{p.decimals}
      <span className="zeros">{p.zeros}</span>
      {unit && <span className="unit">BTC</span>}
    </span>
  );
}

export function Meter({ confidence }: { confidence: Confidence }) {
  const rank = CONFIDENCE_RANK[confidence];
  return (
    <span className="meter" title={CONFIDENCE_MEANING[confidence]}>
      <i aria-hidden="true">
        {[1, 2, 3, 4].map((i) => (
          <b key={i} className={i <= rank ? "on" : ""} />
        ))}
      </i>
      {CONFIDENCE_NAMES[confidence]}
    </span>
  );
}

/** Fact (solid), inferred (dashed) or label (tag-shaped): how we know something. */
export function Basis({ claim }: { claim: Claim }) {
  const t = describeClaim(claim);
  return (
    <span className={`basis ${claim.kind}`} title={t.headline}>
      {t.kind}
    </span>
  );
}

export function categoryColor(c: EntityCategory | null | undefined): string {
  const key = c ? categoryStyle(c) : null;
  return key ? `var(--n-${key})` : "var(--n-address)";
}

export function CategoryTag({ category }: { category: EntityCategory }) {
  return (
    <span className="tag">
      <span className="swatch" style={{ background: categoryColor(category) }} />
      {categoryName(category)}
    </span>
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  const copy = useCallback(() => {
    navigator.clipboard?.writeText(text).then(
      () => {
        setDone(true);
        setTimeout(() => setDone(false), 1200);
      },
      () => {},
    );
  }, [text]);
  return (
    <button type="button" className="copy" onClick={copy} aria-label={`${label} ${text}`}>
      {done ? "Copied" : label}
    </button>
  );
}

/** A long id (address or txid): full in mono, or shortened with the full value on hover. */
export function Id({
  value,
  short = false,
  href,
  copy = true,
}: {
  value: string;
  short?: boolean;
  href?: string;
  copy?: boolean;
}) {
  const text = short ? shortId(value) : value;
  return (
    <span>
      {href ? (
        <a className="mono" href={href} title={value}>
          {text}
        </a>
      ) : (
        <span className="mono" title={value}>
          {text}
        </span>
      )}
      {copy && <CopyButton text={value} />}
    </span>
  );
}

export function ErrorBox({ error }: { error: unknown }) {
  if (!error) return null;
  const message = error instanceof ApiError || error instanceof Error ? error.message : String(error);
  return (
    <div className="error" role="alert">
      {message}
    </div>
  );
}

/** Run an async loader when `deps` change; returns data, error and loading state. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]) {
  const [state, setState] = useState<{ data: T | null; error: unknown; loading: boolean }>({
    data: null,
    error: null,
    loading: true,
  });
  const seq = useRef(0);
  useEffect(() => {
    const n = ++seq.current;
    setState({ data: null, error: null, loading: true });
    load().then(
      (data) => n === seq.current && setState({ data, error: null, loading: false }),
      (error) => n === seq.current && setState({ data: null, error, loading: false }),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}

export function Loading({ what }: { what: string }) {
  return (
    <div className="progress" role="status">
      <div className="track" />
      <span className="muted small">{what}</span>
    </div>
  );
}
