// The parts cache (`crate::parts`, docs/PARTS-CACHE.md, task-v21-parts): `eda board serve` keeps one SQLite index of every installed symbol and footprint
// up to date on a thread of its own, and `GET /api/parts/*` answers from it on the connection's own thread -- never on the request loop, never running kicad-cli.
// The check waits for the indexer to have read the board's libraries, then: LM358 is found with its 8 pins and their electrical types; Device:R's fits are the
// footprints the Symbol Chooser's selector lists (`/api/library/fp_select`, pins=2, R_*), as a set; the SOIC-8 is an 8-pad SMD footprint of 1.27 mm pitch
// whose 3D model is found; `/api/version` answers in under 200 ms while the indexer looks at every file again (`POST /api/parts/refresh`); and a footprint
// saved into a project library of the board is in the cache within one poll.
// Returns { ok, results: [{ name, ok, detail? }, ...] }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  const until = async (test, ms = 30000, every = 250) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await test(); if (v) return v; await sleep(every); } return null; };
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });

  // the indexer has read the board's libraries (a fresh cache is built in a few seconds)
  const ready = await until(async () => { const s = await json("/api/parts/status"); return s.state === "ready" ? s : null; }, 180000, 500);
  const st = await json("/api/parts/status");
  check("the cache is ready", ready, { state: st.state, indexer: st.indexer, stale: (st.stale ?? []).slice(0, 5), libraries: st.libraries });

  // LM358: 8 pins, with their types
  const found = await json("/api/parts/search?kind=symbol&q=lm358&limit=10");
  const lm = (found.hits ?? []).find((h) => h.id === "Amplifier_Operational:LM358");
  check("lm358 finds Amplifier_Operational:LM358, best first", lm && found.hits[0].id === lm.id && lm.pins === 8 && lm.units === 3, (found.hits ?? []).slice(0, 3).map((h) => [h.id, h.score, h.pins]));
  const show = await json("/api/parts/show?kind=symbol&id=Amplifier_Operational:LM358");
  const types = {};
  for (const p of show.pin_list ?? []) types[p.number] = p.type;
  check("its pins and their electrical types", (show.pin_list ?? []).length === 8 && types["3"] === "input" && types["1"] === "output" && types["8"] === "power_in" && types["4"] === "power_in", types);

  // Device:R's fits are the footprint selector's list
  const fits = await json("/api/parts/fits?symbol=Device:R");
  const sel = await until(async () => { const r = await json("/api/library/fp_select?zero=0&pins=2&fp_filters=R_*"); return r.status === "ready" ? r : null; }, 120000, 500);
  const a = new Set(fits.footprints ?? []), b = new Set(sel?.footprints ?? []);
  const same = a.size > 0 && a.size === b.size && [...a].every((x) => b.has(x));
  check("fits Device:R == the selector's pins=2 R_* list (as a set)", same, { fits: a.size, selector: b.size, onlyFits: [...a].filter((x) => !b.has(x)).slice(0, 3), onlySelector: [...b].filter((x) => !a.has(x)).slice(0, 3) });

  // SOIC-8: SMD, 1.27 mm, 8 pads, its model found
  const soic = await json("/api/parts/show?kind=footprint&id=Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
  check("SOIC-8 is SMD, 1.27 mm pitch, 8 pads, its 3D model found", soic.attr === "smd" && soic.pitch_mm === 1.27 && soic.pads === 8 && soic.model_found === true, { attr: soic.attr, pitch: soic.pitch_mm, pads: soic.pads, model: soic.model_found, models: soic.models, error: soic.error });

  // the request loop is not held while the indexer reads every file again
  const before = await json("/api/parts/status");
  const queued = await post("/api/parts/refresh");
  const times = [];
  let sawBusy = false;
  const t0 = Date.now();
  while (Date.now() - t0 < 4000) {
    const s0 = performance.now();
    await fetch("/api/version", { cache: "no-store" }).then((r) => r.text());
    times.push(performance.now() - s0);
    if (times.length % 10 === 0) sawBusy ||= (await json("/api/parts/status")).indexer?.phase !== "ready";
  }
  times.sort((x, y) => x - y);
  const p95 = times[Math.floor(times.length * 0.95)] ?? 0, max = times[times.length - 1] ?? 0;
  check("/api/version answers in under 200 ms while the indexer refreshes", queued.queued > 0 && max < 200, { queued: queued.queued, requests: times.length, p95: Math.round(p95), max: Math.round(max), sawBusy, generation: before.generation });

  // a footprint saved into a project library is in the cache within one poll
  const lib = `PcLib${Date.now() % 100000}`;
  const added = await post("/api/library_tables/add", { kind: "footprint", path: `\${KIPRJMOD}/${lib}`, create: true });
  check("a project footprint library is made", added.ok !== false && !added.error, added);
  const src = await json("/api/library/details?kind=footprint&id=Resistor_SMD:R_0603_1608Metric");
  const id = `${lib}:PcPart`;
  const put = await cmd({ op: "put_library_footprint", footprint: { ...src.footprint, name: id, description: "Saved by the parts-cache check" }, overwrite: true });
  const saved = await post("/api/library/save", { kind: "footprint", id });
  check("the footprint is saved into it", put.ok !== false && saved.ok, { put: put.message ?? put.error, saved: saved.message ?? saved.error });
  const s0 = Date.now();
  const hit = await until(async () => {
    const r = await json(`/api/parts/search?kind=footprint&q=PcPart&lib=${lib}`);
    return (r.hits ?? []).find((h) => h.id === id) ?? null;
  }, 10000, 200);
  const took = Date.now() - s0;
  check("the cache has it within one poll (2 s)", hit && took < 2000 && hit.description === "Saved by the parts-cache check" && hit.pads === 2, { took, hit });

  check("no error was logged", window.__eda.errors().length === 0, window.__eda.errors().slice(0, 3));
  const ok = results.every((r) => r.ok);
  return { ok, results };
})();
