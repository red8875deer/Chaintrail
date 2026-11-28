import { type FormEvent, useEffect, useState } from "react";
import { call } from "../api/client";
import type { AddLabelRequest, EntityCategory, LabelView, OfacUpdateView } from "../api/types";
import { ErrorBox, Loading, useLoad } from "../components/bits";
import { formatInt } from "../lib/format";
import { CATEGORY_NAMES, CATEGORY_ORDER } from "../lib/meaning";
import { LabelList } from "./AddressView";

export function LabelsView({ address: initial }: { address?: string }) {
  const [query, setQuery] = useState(initial ?? "");
  const [looked, setLooked] = useState<{ address: string; labels: LabelView[] } | null>(null);
  const [lookupError, setLookupError] = useState<unknown>(null);
  const packs = useLoad(() => call("label_packs", {}), []);

  const lookup = async (a: string) => {
    setLookupError(null);
    try {
      setLooked({ address: a, labels: await call("label_lookup", { address: a }) });
    } catch (e) {
      setLooked(null);
      setLookupError(e);
    }
  };
  useEffect(() => {
    if (initial) void lookup(initial);
  }, [initial]);

  // No defaults for category and confidence: a service label at Medium or High stops every
  // later trace at that address, so it must be a choice, not a slip.
  const [form, setForm] = useState<
    Omit<AddLabelRequest, "category" | "confidence"> & {
      category: AddLabelRequest["category"] | "";
      confidence: AddLabelRequest["confidence"] | "";
    }
  >({
    address: initial ?? "",
    entity: "",
    category: "",
    confidence: "",
    note: "",
    source: "",
  });
  const [added, setAdded] = useState<string | null>(null);
  const [addError, setAddError] = useState<unknown>(null);
  const add = async (e: FormEvent) => {
    e.preventDefault();
    setAddError(null);
    setAdded(null);
    if (!form.category || !form.confidence) {
      setAddError(new Error("Choose a category and how sure you are."));
      return;
    }
    try {
      const labels = await call("label_add", {
        ...form,
        category: form.category,
        confidence: form.confidence,
        note: form.note?.trim() || null,
        source: form.source?.trim() || null,
      });
      setAdded(`Saved. ${form.address} now has ${labels.length} label${labels.length === 1 ? "" : "s"}.`);
      if (looked?.address === form.address) setLooked({ address: form.address, labels });
    } catch (err) {
      setAddError(err);
    }
  };

  const [ofac, setOfac] = useState<OfacUpdateView | null>(null);
  const [ofacBusy, setOfacBusy] = useState(false);
  const [ofacError, setOfacError] = useState<unknown>(null);
  const updateOfac = async () => {
    setOfacBusy(true);
    setOfacError(null);
    try {
      setOfac(await call("update_ofac", {}));
    } catch (e) {
      setOfacError(e);
    } finally {
      setOfacBusy(false);
    }
  };

  return (
    <div className="page">
      <div className="page-inner">
        <header style={{ display: "grid", gap: 8 }}>
          <h1>Labels</h1>
          <p className="muted" style={{ maxWidth: "70ch" }}>
            Labels say who owns an address and where that claim comes from. Every label links to its source; none
            is treated as proof.
          </p>
        </header>

        <section className="panel" style={{ display: "grid", gap: 12 }}>
          <h2>Look up an address</h2>
          <form
            className="search"
            style={{ maxWidth: "none" }}
            onSubmit={(e) => {
              e.preventDefault();
              if (query.trim()) void lookup(query.trim());
            }}
          >
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Bitcoin, Ethereum or Tron address" spellCheck={false} />
            <button className="btn primary" type="submit">
              Look up
            </button>
          </form>
          <ErrorBox error={lookupError} />
          {looked && (
            <div>
              <p className="mono small" style={{ marginBottom: 4 }}>
                {looked.address}
              </p>
              <LabelList labels={looked.labels} />
            </div>
          )}
        </section>

        <section className="panel" style={{ display: "grid", gap: 12 }}>
          <h2>Add your own label</h2>
          <p className="muted small">
            Saved on this computer only, in your labels folder. Your labels win ties with built-in ones. A service label
            (exchange, mixer, payment service…) at Medium or High stops every later trace at this address; perpetrators'
            and individuals' wallets are followed through.
          </p>
          <form onSubmit={add} style={{ display: "grid", gap: 12 }}>
            <div className="form-grid">
              <label className="field" style={{ gridColumn: "1 / -1" }}>
                <span>Address</span>
                <input className="mono" value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} spellCheck={false} required />
              </label>
              <label className="field">
                <span>Who owns it</span>
                <input value={form.entity} onChange={(e) => setForm({ ...form, entity: e.target.value })} placeholder="e.g. Example Exchange" required />
              </label>
              <label className="field">
                <span>Category</span>
                <select
                  required
                  value={form.category}
                  onChange={(e) => setForm({ ...form, category: e.target.value as EntityCategory })}
                >
                  <option value="" disabled>
                    Choose…
                  </option>
                  {CATEGORY_ORDER.map((c) => (
                    <option key={c} value={c}>
                      {CATEGORY_NAMES[c]}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>How sure are you?</span>
                <select
                  required
                  value={form.confidence}
                  onChange={(e) => setForm({ ...form, confidence: e.target.value as AddLabelRequest["confidence"] })}
                >
                  <option value="" disabled>
                    Choose…
                  </option>
                  <option value="high">High: confirmed by the owner or an official source</option>
                  <option value="medium">Medium: strong evidence</option>
                  <option value="low">Low: a hint only (never stops a trace)</option>
                </select>
              </label>
              <label className="field">
                <span>Source (link or reference)</span>
                <input value={form.source ?? ""} onChange={(e) => setForm({ ...form, source: e.target.value })} placeholder="optional" />
              </label>
              <label className="field" style={{ gridColumn: "1 / -1" }}>
                <span>Note</span>
                <input value={form.note ?? ""} onChange={(e) => setForm({ ...form, note: e.target.value })} placeholder="optional" />
              </label>
            </div>
            <div className="actions">
              <button className="btn primary" type="submit">
                Save label
              </button>
            </div>
            {added && <p className="small">{added}</p>}
            <ErrorBox error={addError} />
          </form>
        </section>

        <section className="panel" style={{ display: "grid", gap: 12 }}>
          <div className="panel-head" style={{ marginBottom: 0 }}>
            <h2>Sanctions list</h2>
            <button className="btn" type="button" onClick={updateOfac} disabled={ofacBusy}>
              {ofacBusy ? "Updating…" : "Update the OFAC list now"}
            </button>
          </div>
          <p className="muted small">Downloads the current US Treasury OFAC SDN list and replaces the built-in copy.</p>
          {ofac && (
            <p className="small">
              Updated: {formatInt(ofac.addresses)} addresses from {ofac.entries} sanctioned entries, list dated {ofac.as_of}.
            </p>
          )}
          <ErrorBox error={ofacError} />
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Label sources</h2>
            {packs.data && (
              <span className="muted small">
                {formatInt(packs.data.reduce((s, p) => s + p.labels, 0))} labels in {packs.data.length} packs
              </span>
            )}
          </div>
          {packs.loading && <Loading what="Loading packs…" />}
          <ErrorBox error={packs.error} />
          <div className="pack-list">
            {packs.data?.map((p) => (
              <div key={p.id} className="pack">
                <strong>{p.title}</strong>
                <span className="muted small" style={{ textAlign: "right" }}>
                  {formatInt(p.labels)} labels
                </span>
                <span className="small muted">
                  {p.as_of ? `As of ${p.as_of}. ` : ""}
                  {p.licence}.{" "}
                  {/^https?:\/\//.test(p.source) ? (
                    <a href={p.source} target="_blank" rel="noreferrer noopener">
                      Source
                    </a>
                  ) : (
                    p.source
                  )}
                </span>
                <span className="small muted mono" style={{ textAlign: "right" }}>
                  {p.id}
                </span>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
