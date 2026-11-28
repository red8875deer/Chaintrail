import { type FormEvent, useEffect, useState } from "react";
import { call, isMock } from "./api/client";
import type { StatusView } from "./api/types";
import { ErrorBox } from "./components/bits";
import { extractId, formatInt } from "./lib/format";
import { go, href, useRoute } from "./lib/route";
import { AddressView } from "./views/AddressView";
import { Home } from "./views/Home";
import { LabelsView } from "./views/LabelsView";
import { TraceView } from "./views/TraceView";
import { TxView } from "./views/TxView";

function Logo() {
  return (
    <svg viewBox="0 0 32 32" aria-hidden="true">
      <rect width="32" height="32" rx="7" fill="var(--ink)" />
      <path d="M9 21 L16 11 L23 18.5" fill="none" stroke="var(--surface)" strokeWidth="2.2" strokeLinecap="round" />
      <path d="M16 11 L19.5 24" stroke="var(--muted)" strokeWidth="1.4" strokeLinecap="round" />
      <circle cx="9" cy="21" r="3" fill="var(--surface)" />
      <circle cx="16" cy="11" r="2.5" fill="var(--surface)" />
      <circle cx="23" cy="18.5" r="3.4" fill="var(--amber)" />
      <circle cx="23" cy="18.5" r="1.4" fill="var(--ink)" />
    </svg>
  );
}

export function App() {
  const route = useRoute();
  const [query, setQuery] = useState("");
  const [searchError, setSearchError] = useState<unknown>(null);
  const [status, setStatus] = useState<StatusView | null>(null);

  useEffect(() => {
    call("status", {}).then(setStatus, () => setStatus(null));
  }, []);

  useEffect(() => setSearchError(null), [route]);

  const search = async (e: FormEvent) => {
    e.preventDefault();
    // Explorer links are accepted: the transaction id or address is taken from them.
    const q = extractId(query);
    if (!q) return;
    setSearchError(null);
    try {
      const r = await call("resolve", { query: q });
      if (r.kind === "transaction") go({ page: "tx", txid: r.txid });
      else if (r.supported) go({ page: "address", address: r.address });
      else go({ page: "labels", address: r.address });
    } catch (err) {
      setSearchError(err);
    }
  };

  let page;
  switch (route.page) {
    case "home":
      page = <Home />;
      break;
    case "address":
      page = <AddressView key={route.address} address={route.address} />;
      break;
    case "tx":
      page = <TxView key={route.txid} txid={route.txid} />;
      break;
    case "trace":
      page = <TraceView params={route.params} />;
      break;
    case "labels":
      page = <LabelsView key={route.address ?? ""} address={route.address} />;
      break;
  }

  return (
    <div className="app">
      {isMock && (
        <p className="mock-banner" role="note">
          Demo data: recorded traces, no network. Any transaction id shows a recorded trace.
        </p>
      )}
      <header className="topbar">
        <a className="brand" href={href({ page: "home" })}>
          <Logo />
          ChainTrail
        </a>
        <form className="search" onSubmit={search} role="search">
          <input
            aria-label="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Paste a Bitcoin address or transaction id"
            spellCheck={false}
            autoComplete="off"
          />
          <button className="btn primary" type="submit">
            Look up
          </button>
        </form>
        <nav className="nav" aria-label="Main">
          <a href={href({ page: "trace", params: null })} aria-current={route.page === "trace" ? "page" : undefined}>
            Trace
          </a>
          <a href={href({ page: "labels" })} aria-current={route.page === "labels" ? "page" : undefined}>
            Labels
          </a>
        </nav>
        {status && (
          <span className="status" title={`Data from ${status.provider}`}>
            {formatInt(status.labelled_addresses)} labelled addresses
          </span>
        )}
      </header>
      {searchError ? (
        <div style={{ padding: "12px 20px" }}>
          <ErrorBox error={searchError} />
        </div>
      ) : null}
      {page}
    </div>
  );
}
