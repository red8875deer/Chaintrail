#!/usr/bin/env node
// Checks that every installed npm package (direct and transitive, dev included) uses a
// permissive licence compatible with ChainTrail's Apache-2.0 licence (see ADR-0007).
// Reads the `license` / `licenses` field of each node_modules/**/package.json, plus every
// entry of package-lock.json (which also covers platform-specific packages that are not
// installed on this machine, e.g. macOS/Windows binaries), and evaluates simple SPDX
// expressions ("A OR B", "A AND B", parentheses).
//
// Usage: npm run licences            (exit code 1 if anything is not allowed)
//        npm run licences -- --list  (also print every package and its licence)

import { readdirSync, readFileSync, existsSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ALLOWED = new Set([
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "0BSD",
  "ISC",
  "CC0-1.0",
  "Unlicense",
  "Zlib",
  "BlueOak-1.0.0",
  // Font files only (ADR-0007): the SIL Open Font Licence allows bundling fonts with any
  // software and places no conditions on the software itself.
  "OFL-1.1",
]);

// Explicitly forbidden families (for a clearer message).
const COPYLEFT = /^(A?GPL|LGPL|MPL|EPL|CDDL|EUPL|OSL|CPL)/i;

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const nodeModules = join(root, "node_modules");
const list = process.argv.includes("--list");

if (!existsSync(nodeModules)) {
  console.error("node_modules not found: run `npm install` first.");
  process.exit(2);
}

/** Collect package directories, including nested node_modules and @scopes. */
function packageDirs(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name.startsWith(".")) continue;
    const full = join(dir, name);
    if (!statSync(full).isDirectory()) continue;
    if (name.startsWith("@")) {
      packageDirs(full, out);
      continue;
    }
    if (existsSync(join(full, "package.json"))) out.push(full);
    const nested = join(full, "node_modules");
    if (existsSync(nested)) packageDirs(nested, out);
  }
  return out;
}

function licenceOf(pkg) {
  const l = pkg.license ?? pkg.licenses;
  if (!l) return null;
  if (typeof l === "string") return l;
  if (Array.isArray(l)) {
    const parts = l.map((x) => (typeof x === "string" ? x : x?.type)).filter(Boolean);
    return parts.length ? `(${parts.join(" OR ")})` : null;
  }
  if (typeof l === "object" && l.type) return l.type;
  return null;
}

/** Evaluate an SPDX expression: true if it can be used under allowed licences only. */
function allowed(expr) {
  const tokens = expr.replace(/\(/g, " ( ").replace(/\)/g, " ) ").trim().split(/\s+/);
  let i = 0;
  function primary() {
    const t = tokens[i++];
    if (t === "(") {
      const v = orExpr();
      i++; // ")"
      return v;
    }
    // "Apache-2.0 WITH LLVM-exception" style: the base licence decides.
    if (tokens[i] === "WITH") i += 2;
    return ALLOWED.has(t.replace(/\+$/, ""));
  }
  function andExpr() {
    let v = primary();
    while (tokens[i] === "AND") {
      i++;
      v = primary() && v;
    }
    return v;
  }
  function orExpr() {
    let v = andExpr();
    while (tokens[i] === "OR") {
      i++;
      v = andExpr() || v;
    }
    return v;
  }
  try {
    return orExpr();
  } catch {
    return false;
  }
}

const seen = new Map();
for (const dir of packageDirs(nodeModules)) {
  let pkg;
  try {
    pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  } catch {
    continue;
  }
  // Some packages ship inner package.json files (e.g. {"type":"module"}) without a name.
  if (!pkg.name || !pkg.version) continue;
  const key = `${pkg.name}@${pkg.version}`;
  if (!seen.has(key)) seen.set(key, licenceOf(pkg));
}

// package-lock.json: every package npm could install, on any platform.
const lockPath = join(root, "package-lock.json");
if (existsSync(lockPath)) {
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  for (const [path, entry] of Object.entries(lock.packages ?? {})) {
    if (!path || entry.link) continue; // "" is the app itself
    const name = entry.name ?? path.slice(path.lastIndexOf("node_modules/") + 13);
    const key = `${name}@${entry.version}`;
    if (!seen.has(key) || seen.get(key) === null) seen.set(key, licenceOf(entry));
  }
} else {
  console.warn("package-lock.json not found: only installed packages were checked.");
}

const bad = [];
for (const [key, licence] of [...seen].sort()) {
  const ok = licence !== null && allowed(licence);
  if (list) console.log(`${ok ? "ok " : "BAD"} ${key}  ${licence ?? "(no licence field)"}`);
  if (!ok) bad.push([key, licence]);
}

if (bad.length) {
  console.error(`\n${bad.length} package(s) with a licence that is not allowed:`);
  for (const [key, licence] of bad) {
    const why =
      licence === null
        ? "no licence field; check by hand"
        : COPYLEFT.test(licence)
          ? "copyleft"
          : "not on the allowlist";
    console.error(`  ${key}: ${licence ?? "-"} (${why})`);
  }
  console.error(`\nAllowed: ${[...ALLOWED].join(", ")}`);
  process.exit(1);
}
console.log(`Licences OK: ${seen.size} packages (installed and lockfile), all on the allowlist.`);
