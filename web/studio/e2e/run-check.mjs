// Runs one web/studio/e2e/*.check.js in headless Chromium against a served board and prints its results.
// usage: PLAYWRIGHT_MODULE=<playwright>/index.mjs node web/studio/e2e/run-check.mjs <url> <check.js> [minutes]
// The check's global (`window.__xxxCheck`) is found by name in the script. Exit 0 when the check is done and ok.
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";

const [url, file, minutes = "10"] = process.argv.slice(2);
const src = readFileSync(file, "utf8");
const global = (src.match(/window\.(__\w+Check)\s*=/) || [])[1];
if (!url || !global) {
  console.error("usage: run-check.mjs <url> <check.js> [minutes]  (no window.__xxxCheck found)");
  process.exit(2);
}
const { open } = await import(resolve(dirname(file), "browser.mjs"));
const { b, p, errors, shot } = await open();
await p.goto(url, { waitUntil: "networkidle" });
await p.waitForFunction(() => !!window.__eda, null, { timeout: 60000 });
await shot("/tmp/run-check-start.png"); // a hidden canvas learns its size only after a screenshot
await p.evaluate(src);
const deadline = Date.now() + Number(minutes) * 60000;
let r;
while (Date.now() < deadline) {
  r = await p.evaluate(async (g) => window[g].wait(20000), global);
  if (r.done) break;
  console.log(`... ${r.n} scenario(s) so far`);
}
// `wait()` answers with the counts; the notes are on the check's own `results`
const results = r.results || (await p.evaluate((g) => window[g].results || [], global));
for (const s of results) {
  console.log(`${s.ok ? "ok  " : "FAIL"} ${s.name}`);
  for (const n of s.notes || []) if (!n.startsWith("ok")) console.log(`       ${n}`);
}
console.log(`${global}: done=${r.done} ok=${r.ok} scenarios=${r.n} failed=${JSON.stringify(r.failed)}`);
if (errors.length) console.log("console errors:\n  " + errors.slice(0, 20).join("\n  "));
await b.close();
process.exit(r.done && r.ok ? 0 : 1);
