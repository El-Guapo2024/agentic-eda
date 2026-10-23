// Runs one web/studio/e2e/*.check.js in headless Chromium against a served board and prints its results.
// usage: PLAYWRIGHT_MODULE=<playwright>/index.mjs node web/studio/e2e/run-check.mjs <url> <check.js> [minutes]
// Exit 0 when the check is done and ok.
//
// Two kinds of check script are run:
//   - the ones with a global (`window.__xxxCheck = { results, done, ok, wait(ms) }`, found by name in the script): polled through `wait()`;
//   - the older ones that are one expression resolving to their results (a list of { name, ok, detail }, an object { ok, results }, or that as a
//     JSON string): the expression's promise is awaited, and a global with `.results` (if the script has one) shows the progress.
// The before-run screenshot goes to $RUN_CHECK_SHOT (default /tmp/run-check-start.png).
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";

const [url, file, minutes = "10"] = process.argv.slice(2);
if (!url || !file) {
  console.error("usage: run-check.mjs <url> <check.js> [minutes]");
  process.exit(2);
}
const src = readFileSync(file, "utf8");
const global = (src.match(/window\.(__\w+Check)\s*=/) || [])[1] ?? null;
const { open } = await import(resolve(dirname(file), "browser.mjs"));
const { b, p, errors, shot } = await open();
await p.goto(url, { waitUntil: "networkidle" });
await p.waitForFunction(() => !!window.__eda, null, { timeout: 60000 });
await shot(process.env.RUN_CHECK_SHOT || "/tmp/run-check-start.png"); // a hidden canvas learns its size only after a screenshot
// The script's own value (a promise for the expression-style checks), kept where the loop below can read it.
await p.evaluate((s) => {
  window.__runCheckOut = null;
  Promise.resolve((0, eval)(s)).then(
    (r) => { window.__runCheckOut = { r }; },
    (e) => { window.__runCheckOut = { err: String((e && e.stack) || e) }; }
  );
}, src);

/** One shape for every kind of result: [{ name, ok, notes }]. */
const parse = (r) => { if (typeof r !== "string") return r; try { return JSON.parse(r); } catch { return { results: [{ name: "the script's value", ok: false, detail: r }] }; } };
const normalise = (r) => {
  r = parse(r);
  const list = Array.isArray(r) ? r : Array.isArray(r?.results) ? r.results : [];
  return list.map((s) => {
    const detail = typeof s.detail === "string" ? s.detail : JSON.stringify(s.detail ?? s);
    const notes = s.notes ? [...s.notes] : s.ok ? [] : [`FAIL: ${detail}`.slice(0, 600)];
    return { name: s.name ?? s.kind ?? "?", ok: !!s.ok, notes };
  });
};

const deadline = Date.now() + Number(minutes) * 60000;
let done = false;
let ok = false;
let results = [];
let failure = null;
while (Date.now() < deadline) {
  const r = await p.evaluate(async (g) => {
    const c = g ? window[g] : null;
    if (c && typeof c.wait === "function") { const w = await c.wait(20000); return { kind: "wait", w, done: !!(w?.done ?? c.done), ok: !!(w?.ok ?? c.ok) }; }
    const t0 = Date.now();
    while (!window.__runCheckOut && Date.now() - t0 < 20000) await fetch("/api/version").then((x) => x.text());
    return { kind: "expr", out: window.__runCheckOut, n: c?.results?.length ?? null };
  }, global);
  if (r.kind === "wait") {
    if (r.done) {
      done = true;
      ok = r.ok;
      results = normalise(r.w.results || (await p.evaluate((g) => window[g].results || [], global)));
      break;
    }
    console.log(`... ${r.w?.n ?? r.w?.results?.length ?? "?"} scenario(s) so far`);
  } else if (r.out) {
    done = true;
    if (r.out.err) { failure = r.out.err; break; }
    results = normalise(r.out.r);
    const v = parse(r.out.r);
    ok = results.length > 0 && results.every((s) => s.ok) && (Array.isArray(v) || v?.ok !== false);
    break;
  } else if (r.n != null) console.log(`... ${r.n} result(s) so far`);
}
for (const s of results) {
  console.log(`${s.ok ? "ok  " : "FAIL"} ${s.name}`);
  for (const n of s.notes || []) if (!n.startsWith("ok")) console.log(`       ${n}`);
}
if (failure) console.log(`the script threw: ${failure}`);
const failed = results.filter((s) => !s.ok).map((s) => s.name);
console.log(`${global ?? file}: done=${done} ok=${done && ok && !failure} scenarios=${results.length} failed=${JSON.stringify(failed)}`);
if (errors.length) console.log("console errors:\n  " + errors.slice(0, 20).join("\n  "));
await b.close();
process.exit(done && ok && !failure ? 0 : 1);
