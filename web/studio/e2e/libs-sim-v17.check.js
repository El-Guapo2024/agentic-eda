// Libraries and the simulator, round 3 (UI-LEFT-3 L3-1 .. L3-6, M3-1 .. M3-3, C3-11; lane libs-sim, task-v17-libssim):
//   L3-1  Revert of a renamed library symbol goes back to its old name, and the tree selects it (`RevertSymbol`, `UpdateSymbolAfterRename( &original, .. )`);
//   L3-2  a renamed derived symbol is saved derived from its parent;
//   L3-5  the library editors' trees get every library's descriptions and fields with the tree (`GET /api/library/entries_all`), no library opened;
//   L3-6  a footprint file only KiCad's parser refuses (kicad-cli's verdict) is not in the Footprint Editor's tree, nor in the chooser's items;
//   M3-1  the K device's l1 / l2 list the design's inductors (`GET /api/sim/model/inductors`), no kicad-cli netlist asked for, an inductor excluded from the
//         simulation left out;
//   M3-2  the probe and tune cursors carry KiCad's 32- and 64-pixel pictures as one image set;
//   M3-3  a measurement row cleared stays, empty, until the next run; the points field takes digits only and is normalized on leaving it;
//   C3-11 the Symbol Chooser's search takes PCRE2's back references and look-arounds (`/(\d)\1/`).
// The libraries it makes stay in the board's folder (their rows are taken out of the table); every design edit is undone or harmless.
// It answers through `window.__libsSimV17Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=sim-v10 minutes=12
(async () => {
  const results = [];
  const check = (window.__libsSimV17Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const click = (el) => el?.click();
  // the page's requests (the sleeps poll /api/version: room for all of them)
  performance.setResourceTimingBufferSize(1_000_000);
  const requested = (path) => performance.getEntriesByType("resource").some((e) => new URL(e.name).pathname === path);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close|No)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(800); };
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const libRow = (lib) => document.querySelector(`.library-tree-group[data-library="${CSS.escape(lib)}"]`);
  const openGroup = async (lib) => { const r = await until(() => libRow(lib), 8000); if (r && !r.textContent.trim().startsWith("▾")) { r.click(); await sleep(400); } return !!r; };

  const s = Date.now().toString(36);
  const SYM = `Lv17S${s}`, FP = `Lv17F${s}`;

  // ------------------------------------------------------------------------------------------------------------------------ the Symbol Editor
  await scenario("L3-5: the Symbol Editor's tree gets every library's descriptions and fields with the tree", async (t) => {
    performance.clearResourceTimings();
    await clickTab("Symbol");
    t(await until(() => requested("/api/library/entries_all"), 8000), "GET /api/library/entries_all asked when the tree is built");
    const all = await until(async () => { const v = await json("/api/library/entries_all?kind=symbol"); return v.status === "ready" ? v : null; }, 120000);
    t(!!all && Object.keys(all.libraries).length > 0, `every library's items: ${all ? Object.keys(all.libraries).length : "none"} libraries`);
    const device = all?.libraries?.Device?.find((i) => i.name === "R");
    t(!!device && device.description !== "" && device.fields?.Datasheet !== undefined, `Device:R comes with its description and fields: ${JSON.stringify(device ?? {}).slice(0, 160)}`);
  });

  await scenario("L3-1, L3-2: Revert of a renamed symbol goes back to its name; a renamed derived symbol is saved derived", async (t) => {
    const add = await post("/api/library_tables/add", { kind: "symbol", create: true, path: `libs/${SYM}` });
    if (!t(add.ok === true, `a library of the project table: ${JSON.stringify(add).slice(0, 160)}`)) return;
    let r = await cmd({ op: "batch", cmds: [{ op: "new_symbol", lib_id: `${SYM}:Base` }, { op: "new_symbol", lib_id: `${SYM}:Orig` }] });
    t(r.ok !== false, `two new symbols: ${JSON.stringify(r).slice(0, 120)}`);
    // Orig derived from Base, the library saved
    const orig = await json(`/api/symbol?lib_id=${encodeURIComponent(`${SYM}:Orig`)}`);
    r = await cmd({ op: "put_library_symbol", symbol: { ...(orig.symbol ?? orig), extends: `${SYM}:Base` }, overwrite: true });
    t(r.ok !== false, `Orig derives from Base: ${JSON.stringify(r).slice(0, 120)}`);
    const saved = await post("/api/library/save", { kind: "symbol", lib: SYM });
    t(saved.ok === true, `saved: ${JSON.stringify(saved).slice(0, 160)}`);
    // the library dropped from the design (a reload), then Orig renamed: the parent is not in the design
    await post("/api/library/revert", { kind: "symbol", lib: SYM });
    r = await cmd({ op: "rename_library_symbol", lib_id: `${SYM}:Orig`, new_lib_id: `${SYM}:Renamed`, overwrite: false });
    t(r.ok !== false, `renamed: ${JSON.stringify(r).slice(0, 120)}`);
    await sleep(600);
    // Revert from the tree
    t(await openGroup(SYM), "the library in the tree");
    const row = await until(() => treeRow(`${SYM}:Renamed`), 8000);
    if (!t(!!row, "the renamed symbol in the tree")) return;
    row.click();
    await sleep(300);
    await E.run("common.Control.revert");
    const ask = await until(() => [...document.querySelectorAll("[role=dialog] button")].find((b) => b.textContent.trim() === "Revert"), 5000);
    t(!!ask, "Revert asks first");
    click(ask);
    t(await until(() => treeRow(`${SYM}:Orig`) && !treeRow(`${SYM}:Renamed`), 8000), "the tree has the old name again, the new one gone");
    t(await until(() => treeRow(`${SYM}:Orig`)?.getAttribute("aria-selected") === "true", 4000), "the tree selects it (`SelectLibId`)");
    // renamed again and saved: written derived from Base
    r = await cmd({ op: "rename_library_symbol", lib_id: `${SYM}:Orig`, new_lib_id: `${SYM}:Again`, overwrite: false });
    const again = await post("/api/library/save", { kind: "symbol", lib: SYM });
    t(again.ok === true, `saved again: ${JSON.stringify(again).slice(0, 120)}`);
    await post("/api/library/revert", { kind: "symbol", lib: SYM });
    const details = await json(`/api/library/derived?id=${encodeURIComponent(`${SYM}:Base`)}`);
    t((details.names ?? []).includes("Again") && !(details.names ?? []).includes("Orig"), `the file has Again derived from Base: ${JSON.stringify(details)}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ the Footprint Editor
  await scenario("L3-6: a footprint file only KiCad's parser refuses is not in the tree nor the chooser's items, and the library says which", async (t) => {
    const add = await post("/api/library_tables/add", { kind: "footprint", create: true, path: `libs/${FP}` });
    if (!t(add.ok === true, `a footprint library: ${JSON.stringify(add).slice(0, 160)}`)) return;
    const dir = (await json("/api/library_tables")).tables.footprint.project.rows.find((x) => x.nickname === FP)?.uri ?? "";
    const folder = (await json("/api/library/writable?kind=footprint")).libraries.find((l) => l.name === FP)?.path;
    const text = (name, extra) => `(footprint "${name}" (version 20240108) (generator "check") (layer "F.Cu")\n${extra}\t(attr smd)\n\t(pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))\n)\n`;
    const w1 = await window.__runCheckWriteFile?.(`${folder}/Good.kicad_mod`, text("Good", ""));
    const w2 = await window.__runCheckWriteFile?.(`${folder}/Strict.kicad_mod`, text("Strict", "\t(frobnicate 1)\n"));
    if (!t(w1 === "ok" && w2 === "ok", `two files written (${dir}): ${w1} ${w2}`)) return;
    const items = await json(`/api/library/items?kind=footprint&lib=${FP}`);
    t(JSON.stringify(items.names) === JSON.stringify(["Good"]), `the tree's items: ${JSON.stringify(items.names)}`);
    t(/Unable to read file '.*Strict\.kicad_mod'/.test(items.load_error ?? ""), `the library says which: ${(items.load_error ?? "").slice(0, 160)}`);
    const entries = await json(`/api/library/entries?kind=footprint&lib=${FP}`);
    t(JSON.stringify((entries.entries ?? []).map((e) => e.name)) === JSON.stringify(["Good"]), `the chooser's items: ${JSON.stringify((entries.entries ?? []).map((e) => e.name))}`);
    await clickTab("Footprint");
    t(await openGroup(FP), "the library in the Footprint Editor's tree");
    t(await until(() => treeRow(`${FP}:Good`), 6000) && !treeRow(`${FP}:Strict`), "Good is listed, Strict is not");
  });

  // ------------------------------------------------------------------------------------------------------------------------ the chooser's search
  await scenario("C3-11: the Symbol Chooser's search takes PCRE2's back references and look-aheads", async (t) => {
    const hit = async (q) => {
      const v = await until(async () => { const r = await json(`/api/library/search?kind=symbol&q=${encodeURIComponent(q)}`); return r.status === "ready" ? r : null; }, 180000);
      return (v?.libraries ?? []).flatMap((l) => l.items.map((i) => `${l.name}:${i.name}`));
    };
    const back = await hit("/^LM(\\d)\\1/");
    t(back.some((n) => /:LM(\d)\1/.test(n)) && !back.some((n) => /:LM358$/.test(n)), `/^LM(\\d)\\1/ finds LM339 and the like, not LM358: ${back.slice(0, 5).join(", ")}`);
    const ahead = await hit("/^R(?=_)/");
    t(ahead.some((n) => /:R_/.test(n)) && !ahead.some((n) => /^Device:R$/.test(n)), `/^R(?=_)/ finds R_..., not R: ${ahead.slice(0, 5).join(", ")}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ the simulator
  await clickTab("Schematic");
  const dlg = () => document.querySelector('[data-testid="sim-model-dialog"]');
  const $ = (id) => dlg()?.querySelector(`[data-testid="${id}"]`) ?? null;
  const cell = (name) => dlg()?.querySelector(`[data-testid="sim-model-param-grid"] tr[data-param="${name}"]`)?.querySelector("input, select") ?? null;
  const propsDialog = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector('[data-testid="symbol-sim-model"]')) ?? null;
  const openModel = async (ref) => {
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [ref]);
    await sleep(300);
    await E.run("eeschema.InteractiveEdit.properties");
    await sleep(500);
    click(document.querySelector('[data-testid="symbol-sim-model"]'));
    return !!(await until(() => $("sim-model-device"), 10000));
  };
  const cancelBoth = async () => {
    for (let i = 0; i < 2; i++) {
      const top = dlg() ?? propsDialog();
      click([...(top?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.textContent.trim() === "Cancel"));
      await sleep(300);
    }
  };
  const inductorChoices = async () => {
    setValue($("sim-model-device"), "Mutual Inductance Statement");
    const l1 = await until(() => (cell("l1") instanceof HTMLSelectElement && cell("l1").options.length > 1 ? cell("l1") : null), 20000);
    return l1 ? [...l1.options].map((o) => o.value) : [];
  };

  await scenario("M3-1: the K device's l1 / l2 are the design's inductors, read without kicad-cli's netlist", async (t) => {
    performance.clearResourceTimings();
    t(await openModel("R2"), "the Simulation Model dialog");
    const opts = await inductorChoices();
    t(opts.join(",") === ",L1,L2", `l1's choices: ${JSON.stringify(opts)}`);
    t(requested("/api/sim/model/inductors") && !requested("/api/sim/circuit"), `GET /api/sim/model/inductors (${requested("/api/sim/model/inductors")}), no POST /api/sim/circuit (${requested("/api/sim/circuit")})`);
    await cancelBoth();
    // L2 excluded from the simulation: out of the list (`ResolveExcludedFromSim`)
    const ex = await cmd({ op: "set_symbol_attrs", ids: ["L2"], exclude_from_sim: true });
    t(ex.ok !== false, `L2 excluded: ${JSON.stringify(ex).slice(0, 100)}`);
    await sleep(800);
    t(await openModel("R2"), "the dialog again");
    const after = await inductorChoices();
    t(after.join(",") === ",L1", `L2 left out: ${JSON.stringify(after)}`);
    await cancelBoth();
    await cmd({ op: "set_symbol_attrs", ids: ["L2"], exclude_from_sim: false });
    await sleep(500);
  });

  const win = () => document.querySelector('[role=dialog][aria-label="SPICE Simulator"]');
  const q = (sel) => win()?.querySelector(sel) ?? null;
  const simDialog = (title) => document.querySelector(`.sim-dialog [role=dialog][aria-label="${title}"]`);
  const focusSim = () => { const w = q(".sim-titlebar"); w?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); w?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })); };
  const run = async (id, arg) => { focusSim(); await sleep(60); return E.run(id, arg); };
  const runButton = () => q('button[aria-label="Run Simulation"]');
  const settle = async (ms = 90000) => { await sleep(300); const t0 = Date.now(); while (runButton()?.disabled && Date.now() - t0 < ms) await sleep(200); await sleep(400); };
  const measurementRows = () => [...(q('table[aria-label="Measurements"]')?.querySelectorAll("tbody tr") ?? [])].map((r) => r.querySelector("input")?.value ?? "");
  const commit = async (el, v) => { setValue(el, v); await sleep(80); el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await sleep(120); };

  await scenario("M3-3: the points field takes digits only and is normalized on leaving it, with no message", async (t) => {
    await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2000);
    if (!t(!!win(), "the simulator")) return;
    await run("eeschema.Simulation.newAnalysisTab");
    const d = await until(() => simDialog("New Simulation Tab"), 5000);
    if (!t(!!d, "New Simulation Tab")) return;
    setValue(d.querySelector('select[aria-label="Analysis type"]'), "AC");
    await sleep(200);
    const pts = d.querySelector('[aria-label="Number of points per decade:"]');
    if (!t(!!pts, "the points field")) return;
    setValue(pts, "");
    pts.focus();
    const typed = await window.__runCheckInput?.([{ t: "type", text: "1e2x0" }]);
    await sleep(200);
    t(typed === "ok" && pts.value === "120", `typed 1e2x0, the field holds ${JSON.stringify(pts.value)} (IsCharOk: digits only)`);
    setValue(pts, "007");
    pts.focus();
    pts.blur();
    await sleep(200);
    t(pts.value === "7", `left with 007: ${JSON.stringify(pts.value)} (NormalizeString)`);
    click([...d.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel"));
    await sleep(300);
  });

  await scenario("M3-2, M3-3: the probe and tune cursors' 64-pixel pictures; a measurement row cleared stays until the next run", async (t) => {
    if (!t(!!win(), "the simulator")) return;
    await run("eeschema.Simulation.newAnalysisTab");
    const d = await until(() => simDialog("New Simulation Tab"), 5000);
    if (!t(!!d, "New Simulation Tab")) return;
    setValue(d.querySelector('select[aria-label="Analysis type"]'), "TRAN");
    await sleep(200);
    setValue(d.querySelector('[aria-label="Time step:"]'), "10u");
    setValue(d.querySelector('[aria-label="Final time:"]'), "1m");
    click([...d.querySelectorAll("button")].find((b) => b.textContent.trim() === "OK"));
    await sleep(800);
    await run("eeschema.Simulation.runSimulation");
    await settle();
    for (const [k, m] of [["Measurement 1", "MAX V(/MID)"], ["Measurement 2", "MIN V(/MID)"], ["Measurement 3", "AVG V(/MID)"]]) {
      const el = q(`input[aria-label="${k}"]`);
      if (el) await commit(el, m);
      await settle();
    }
    t(JSON.stringify(measurementRows()) === JSON.stringify(["MAX V(/MID)", "MIN V(/MID)", "AVG V(/MID)", ""]), `three measurements and the type-in row: ${JSON.stringify(measurementRows())}`);
    await commit(q('input[aria-label="Measurement 2"]'), "");
    await settle();
    t(JSON.stringify(measurementRows()) === JSON.stringify(["MAX V(/MID)", "", "AVG V(/MID)", ""]), `the cleared row stays, empty: ${JSON.stringify(measurementRows())}`);
    await run("eeschema.Simulation.runSimulation");
    await settle();
    t(JSON.stringify(measurementRows()) === JSON.stringify(["MAX V(/MID)", "AVG V(/MID)", ""]), `the next run builds the grid again: ${JSON.stringify(measurementRows())}`);
    // the cursors
    const box = document.querySelector(".pcb-canvas-container");
    await run("eeschema.Simulation.probe");
    await sleep(400);
    const probe = decodeURIComponent(box?.style.cursor ?? "");
    t(/image-set\(/.test(probe) && probe.includes('width="32" height="32"') && probe.includes('width="64" height="64"') && /\) 1 31, crosshair$/.test(probe), `the voltage probe: ${probe.slice(0, 40)}..${probe.slice(-40)}`);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    await run("eeschema.Simulation.tune");
    await sleep(400);
    const tune = decodeURIComponent(box?.style.cursor ?? "");
    t(/image-set\(/.test(tune) && tune.includes('width="64" height="64"') && /\) 1 30, crosshair$/.test(tune), `the tune cursor (KICURSOR::TUNE): ${tune.slice(-40)}`);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    click(q('button[aria-label="Close"]'));
    await sleep(500);
  });

  // ------------------------------------------------------------------------------------------------------------------------ clean up: the check's rows out of the board's tables
  for (const kind of ["symbol", "footprint"]) {
    const v = await json("/api/library_tables");
    const rows = v.tables[kind].project.rows.filter((r) => !/^Lv17[SF]/.test(r.nickname));
    await post("/api/library_tables", { kind, rows });
  }

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
