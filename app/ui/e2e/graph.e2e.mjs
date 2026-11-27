#!/usr/bin/env node
// Browser checks of the trace graph on recorded real traces (mock mode, no network).
// They guard what an investigator needs to read a graph: nothing overlaps, text is readable
// when a trace opens, the selected lead is on screen, and selecting leads never moves what
// is already drawn. Run after `npm run build`:  npm run e2e
//
// Uses the Playwright library with Chromium (CI installs it with
// `npx playwright install --with-deps chromium`).

import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 4179;
const BASE = `http://127.0.0.1:${PORT}`;

/** Readable: labels are 12–13px, so the opening zoom must keep them near 10px. */
const MIN_ZOOM = 0.79;
/** Overlapping labels tolerated per trace (rotated-label boxes are approximate). */
const MAX_LABEL_OVERLAPS = 1;

const CASES = [
  ["Colonial Pipeline ransom, forward", "txid=fc78327d4e46dac01dc313067b1ac7f274cdb3a07ea9f28f6f71473145f1b264&vout=1&dir=forward"],
  ["Bitfinex withdrawal, backward", "txid=91af018f49660816a65de0107cc3ca1935c6eddcaf9cf90f46cc56ef424370f4&to=bc1q4k73rpas0qz93xrg2rjeush6vwaejkp2nranlqsajmwm5qgsfj4sd5u4jd&dir=backward"],
  ["2009 payment, 15 hops (270 addresses)", "txid=f4184fc596403b9d638783cf57adfe4c75c605f6356fbc91338530e9831e9e16&vout=0&dir=forward"],
  ["Silk Road wallet, backward fan-in", "txid=3f036ff88bb851b57a1e28780dbce35a6457a8b57995c095b55b3b0cf48ba9fd&vout=0&dir=backward"],
  ["40-hop chain, backward", "txid=a82d60986dda7b3cde8a480a73d0be6a3b07b4554bfe7a93e87637f5b5ae2c08&dir=backward"],
  ["Batch payout, all outputs", "txid=628fb3f0cac80a37f14d54ffc7ab9e6c4e531cd58f87b8f8cb32b65d8e92f06a&dir=forward"],
];

/** Runs in the page: edges drawn right to left (funds flow left to right). */
function reversedEdges() {
  const cy = window.__chaintrailCy;
  let n = 0;
  cy.edges().forEach((e) => {
    if (!e.isLoop() && e.target().position().x < e.source().position().x - 20) n++;
  });
  return n;
}

/** Runs in the page: overlaps and view facts of the graph as drawn. */
function measure() {
  const cy = window.__chaintrailCy;
  const hit = (a, b) => a.x1 < b.x2 && b.x1 < a.x2 && a.y1 < b.y2 && b.y1 < a.y2;
  const nodes = cy.nodes().map((n) => ({ id: n.id(), b: n.boundingBox({ includeLabels: true, includeOverlays: false }) }));
  const bodies = cy.nodes().map((n) => ({ id: n.id(), b: n.boundingBox({ includeLabels: false, includeOverlays: false }) }));
  const labels = [];
  cy.elements().forEach((el) => {
    const shown = ["label", "source-label", "target-label"].some((k) => {
      try {
        return el.style(k) !== "";
      } catch {
        return false;
      }
    });
    if (!shown) return;
    const b = el.boundingBox({ includeNodes: false, includeEdges: false, includeLabels: true, includeOverlays: false });
    if (b.w > 1 && b.h > 1) labels.push({ id: el.id(), text: String(el.data("label") ?? "").replace(/\n/g, " ").slice(0, 30), b });
  });
  const nodeOverlaps = [];
  for (let i = 0; i < nodes.length; i++)
    for (let j = i + 1; j < nodes.length; j++) if (hit(nodes[i].b, nodes[j].b)) nodeOverlaps.push(`${nodes[i].id} / ${nodes[j].id}`);
  const labelOverlaps = [];
  for (let i = 0; i < labels.length; i++) {
    for (let j = i + 1; j < labels.length; j++) if (hit(labels[i].b, labels[j].b)) labelOverlaps.push(`"${labels[i].text}" / "${labels[j].text}"`);
    for (const n of bodies) if (n.id !== labels[i].id && hit(labels[i].b, n.b)) labelOverlaps.push(`"${labels[i].text}" on node ${n.id.slice(0, 16)}`);
  }
  const ext = cy.extent();
  const lead = cy.nodes(".path.lead").first();
  const leadInView = lead.empty() ? null : hit(lead.boundingBox({ includeLabels: true }), ext);
  const pos = {};
  cy.nodes().forEach((n) => {
    pos[n.id()] = n.position();
  });
  return { zoom: cy.zoom(), nodes: nodes.length, nodeOverlaps, labelOverlaps, leadInView, pos };
}

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(BASE)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("vite preview did not start");
}

const server = spawn("npx", ["vite", "preview", "--port", String(PORT), "--strictPort", "--host", "127.0.0.1"], {
  cwd: root,
  stdio: "ignore",
});
const failures = [];
try {
  await waitForServer();
  const browser = await chromium.launch();
  for (const [name, query] of CASES) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    const errors = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    page.on("console", (m) => {
      if (m.type() === "error") errors.push(m.text());
    });
    const fail = (what) => failures.push(`${name}: ${what}`);
    await page.goto(`${BASE}/?mock=1&debug=1#/trace?${query}&run=1`);
    await page.waitForSelector(".summary-amount", { timeout: 30000 });
    await page.waitForTimeout(900);
    const m = await page.evaluate(measure);
    if (m.zoom < MIN_ZOOM) fail(`opens at zoom ${m.zoom.toFixed(2)} (labels too small to read)`);
    if (m.nodeOverlaps.length) fail(`overlapping nodes: ${m.nodeOverlaps.slice(0, 3).join("; ")}`);
    if (m.labelOverlaps.length > MAX_LABEL_OVERLAPS) fail(`${m.labelOverlaps.length} overlapping labels: ${m.labelOverlaps.slice(0, 3).join("; ")}`);
    if (m.leadInView === false) fail("the selected lead is off screen");

    // Selecting leads must not rearrange what is drawn: a revealed route may push the part
    // of the graph to its right further right (as a block); nothing else moves, and money
    // keeps flowing left to right.
    for (const b of await page.$$(".group-toggle[aria-expanded='false']")) await b.click();
    const leads = await page.$$(".lead");
    let prev = m.pos;
    let worst = 0;
    let shifted = 0;
    const reversed0 = await page.evaluate(reversedEdges);
    for (let i = 0; i < Math.min(leads.length, 6); i++) {
      await (await page.$$(".lead"))[i].click();
      await page.waitForTimeout(700);
      const cur = await page.evaluate(measure);
      const ids = Object.keys(cur.pos).filter((id) => prev[id]);
      for (const id of ids) {
        const p = cur.pos[id];
        const o = prev[id];
        worst = Math.max(worst, Math.abs(p.y - o.y), o.x - p.x);
        shifted = Math.max(shifted, p.x - o.x);
      }
      for (const a of ids)
        for (const b of ids)
          if (prev[a].x < prev[b].x - 1 && cur.pos[a].x > cur.pos[b].x + 1) fail(`after selecting lead ${i + 1}: ${a} and ${b} swapped sides`);
      const reversed = await page.evaluate(reversedEdges);
      if (reversed > reversed0) fail(`after selecting lead ${i + 1}: ${reversed - reversed0} more edge(s) run right to left`);
      if (cur.nodeOverlaps.length) fail(`after selecting lead ${i + 1}: overlapping nodes ${cur.nodeOverlaps.slice(0, 2).join("; ")}`);
      prev = cur.pos;
    }
    if (worst > 2) fail(`selecting leads moved drawn nodes up, down or left by up to ${Math.round(worst)}px`);
    if (errors.length) fail(`console errors: ${errors.slice(0, 2).join(" | ")}`);
    console.log(
      `${failures.some((f) => f.startsWith(name)) ? "FAIL" : "ok  "} ${name}: ${m.nodes} nodes, zoom ${m.zoom.toFixed(2)}, ` +
        `${m.labelOverlaps.length} label overlap(s), ${leads.length} leads, max move ${Math.round(worst)}px` +
        (shifted > 2 ? `, block shifted right ${Math.round(shifted)}px` : ""),
    );
    await page.close();
  }
  await browser.close();
} finally {
  server.kill();
}
if (failures.length) {
  console.error(`\n${failures.length} problem(s):\n- ${failures.join("\n- ")}`);
  process.exit(1);
}
console.log("\nGraph checks passed.");
