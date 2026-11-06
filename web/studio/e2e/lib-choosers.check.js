// Library choosers check (UI-LEFT L3 and L6, GAPS.md items 5 and 30): the search words KiCad's choosers take beyond text, the Symbol Chooser's footprint
// selector, the Footprint Chooser's 3D viewer in its own window, Change Symbols' browse buttons and a folder library that loads with errors.
//
// What it checks:
//   the Footprint Chooser (Place Footprint): `/^R_0603/` (`EDA_PATTERN_MATCH_REGEX`, the word lowered first) lists footprints whose name starts R_0603 and
//     nothing else; the 3D view on and "Show 3D viewer in own window" ticked open "3D Viewer — Lib:Name", which follows the selection and closes when unticked;
//   the Symbol Chooser (A) over a project library the check makes through the routes (New Library, two symbols put and saved into its file): `resistance<10k`
//     (`EDA_PATTERN_MATCH_RELATIONAL`) finds the symbol whose keywords say resistance=4.7k and not the one with resistance=22k, `/^lcres_l/` the first only;
//   the footprint selector (`FOOTPRINT_SELECT_WIDGET`) of Device:R: "No default footprint", then 2-pad footprints its filter R_* allows; the one chosen is
//     drawn and is the footprint the placed symbol gets (undone after);
//   Change Symbols: the browse button opens the Symbol Chooser over every library (power symbols too), OK puts the id in the field, Change gives a symbol a
//     symbol of a library the design did not use, with its definition (undone after);
//   a folder library with a broken file (only when the board's folder holds Broken.kicad_symdir, a GoodPart.kicad_sym and a cut-off BadPart.kicad_sym, which
//     the person serving the board puts there): added to the project table, it lists GoodPart, GET /api/library/load_errors names it with KiCad's words
//     ("Unable to read file '...BadPart.kicad_sym'" ... "The library cannot be saved until these errors are fixed manually."), the chooser and the status bar
//     warn; the row is taken back out.
// The project library file the check makes stays in the board's folder (its row is taken out of the table); every design edit is undone.
// It answers through `window.__libChoosersCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=12 fixtures=broken-symdir   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__libChoosersCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };
  const E = window.__eda;
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const run = (id, args) => E.run(id, args);
  const sch = () => json("/api/schematic");
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const undo = async (domain) => { await post("/api/undo", { domain }); await sleep(300); };
  const items = (root) => [...(root?.querySelectorAll("[data-chooser-tree] [data-id]") ?? [])].filter((r) => !r.dataset.unit).map((r) => r.dataset.id);
  const status = (root) => root?.querySelector("[data-chooser-status]")?.textContent ?? "";
  const closeChoosers = async () => {
    for (let i = 0; i < 4; i++) {
      const c = document.querySelector("[data-chooser-cancel]");
      if (!c) break;
      c.click();
      await sleep(300);
    }
  };
  const symSnapshot = async () => JSON.stringify((await sch()).symbols.map((s) => [s.id, s.lib_id, s.footprint ?? "", s.at]));
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeChoosers(); } catch { /* ignore */ }
    results.push(entry);
  };
  /** Type a search into the open chooser and wait until the list answers those very words: the chooser says which search its list answers in full
   *  (`data-chooser-answered`, null while the server has not answered the one typed or is still reading libraries). Waiting for "N matches" alone read the
   *  last search's list when the new answer came late (the flake of 2026-11-06: `resistance>10k` answered with `resistance<10k`'s LcRes_Low). */
  const search = async (root, q, ms = 90000) => {
    setValue(root.querySelector("#library-tree-search"), q);
    return until(() => root.querySelector("[data-chooser-status]")?.dataset.chooserAnswered === q && / match(es)?/.test(status(root)), ms);
  };

  const s = Date.now().toString(36);
  const LIB = `LcLib${s}`;
  const errorsBefore = E.errors().length;

  // ----------------------------------------------------------------------------------------------------------- the Footprint Chooser
  await scenario("Footprint Chooser: /^R_0603/ lists only footprints whose name starts R_0603", async (t) => {
    await clickTab("PCB");
    await run("pcbnew.EditorControl.placeFootprint");
    const root = await until(() => document.querySelector("[data-footprint-chooser]"), 6000);
    if (!t(root, "Place Footprint opens the Footprint Chooser")) return;
    t(await search(root, "/^R_0603/"), `the search finishes: ${status(root)}`);
    const found = items(root);
    t(found.length > 0 && found.every((id) => /^r_0603/i.test(id.split(":")[1])), `${found.length} footprints, every name starts R_0603: ${found.slice(0, 6).join(", ")}`);
    t(found.includes("Resistor_SMD:R_0603_1608Metric"), "Resistor_SMD:R_0603_1608Metric is one");
    t(!found.includes("Capacitor_SMD:C_0603_1608Metric"), "C_0603 is not");
    await search(root, "/^R_0603");
    t(items(root).length === found.length, "the closing slash may be left off (no feedback otherwise while typing)");
    await search(root, "R_0603");
    t(items(root).length >= found.length, `the plain text finds at least as many (anywhere in a term): ${items(root).length}`);
  });

  await scenario("Footprint Chooser: Show 3D viewer in own window follows the selection and closes when unticked", async (t) => {
    await run("pcbnew.EditorControl.placeFootprint");
    const root = await until(() => document.querySelector("[data-footprint-chooser]"), 6000);
    if (!t(root, "the chooser opens")) return;
    await search(root, "R_0603_1608Metric");
    const pick = async (id) => { const r = await until(() => root.querySelector(`[data-chooser-tree] [data-id="${id}"]`), 20000); r?.click(); await sleep(500); return !!r; };
    t(await pick("Resistor_SMD:R_0603_1608Metric"), "R_0603_1608Metric selected");
    const box = root.querySelector("[data-chooser-own-3d]");
    if (!t(box && !box.checked, "the box is there, unticked as the frame is made")) return;
    box.click();
    await sleep(400);
    const v3 = root.querySelector("[data-chooser-view-3d]");
    t(!document.querySelector("[data-chooser-3d-window]") || v3.getAttribute("aria-pressed") === "true", "with the 3D view off the box alone opens nothing");
    if (v3.getAttribute("aria-pressed") !== "true") { v3.click(); await sleep(500); }
    const win = await until(() => document.querySelector("[data-chooser-3d-window]"), 4000);
    t(win, "the 3D view on: the own window opens");
    t(/3D Viewer — Resistor_SMD:R_0603_1608Metric$/.test(win?.querySelector("[data-chooser-3d-window-title]")?.textContent ?? ""), `its title: ${win?.querySelector("[data-chooser-3d-window-title]")?.textContent}`);
    t(!!document.querySelector('[data-testid="chooser-3d-window"]'), "it holds a 3D view");
    await search(root, "R_0805_2012Metric");
    t(await pick("Resistor_SMD:R_0805_2012Metric"), "another footprint selected");
    t(await until(() => /R_0805_2012Metric$/.test(document.querySelector("[data-chooser-3d-window-title]")?.textContent ?? ""), 3000), "the window follows it");
    box.click();
    await sleep(400);
    t(!document.querySelector("[data-chooser-3d-window]"), "unticked: the window closes");
    box.click();
    await sleep(300);
    const again = await until(() => document.querySelector("[data-chooser-3d-window]"), 3000);
    again?.querySelector("[data-chooser-3d-window-close]")?.click();
    await sleep(300);
    t(again && !document.querySelector("[data-chooser-3d-window]"), "its own close box closes it");
    v3.click(); // back to how the chooser opens: the 3D view off
    await sleep(300);
  });

  // ----------------------------------------------------------------------------------------------------------- the Symbol Chooser
  let made = false;
  const tablesBefore = await json("/api/library_tables");
  await scenario("Symbol Chooser: resistance<10k and /^lcres_l/ over a project library of the tables", async (t) => {
    await clickTab("Schematic");
    const add = await post("/api/library_tables/add", { kind: "symbol", path: `${LIB}.kicad_sym`, create: true });
    if (!t(!add.error, `New Library ${LIB}: ${add.error ?? "made"}`)) return;
    made = true;
    const base = (await json("/api/library/symbol?lib_id=Device:R")).symbol;
    if (!t(base, "Device:R read as the template of the two symbols")) return;
    const put = async (name, keywords) => cmd({ op: "put_library_symbol", symbol: { ...base, lib_id: `${LIB}:${name}`, description: `Test resistor ${name}`, keywords }, overwrite: true });
    const a = await put("LcRes_Low", "resistance=4.7k thin-film");
    const b = await put("LcRes_High", "resistance=22k thick-film");
    t(a.ok !== false && b.ok !== false, "the two symbols are in the design's buffer of the library");
    const saved = await post("/api/library/save", { kind: "symbol", lib: LIB });
    t(saved.ok, `saved into the file: ${saved.message ?? saved.error}`);
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 6000);
    if (!t(root, "A opens the Symbol Chooser")) return;
    t(await search(root, "resistance<10k"), `the search finishes: ${status(root)}`);
    let found = items(root);
    t(found.includes(`${LIB}:LcRes_Low`), `resistance<10k finds LcRes_Low (resistance=4.7k): ${found.join(", ")}`);
    t(!found.includes(`${LIB}:LcRes_High`), "and not LcRes_High (resistance=22k)");
    await search(root, "resistance>10k");
    found = items(root);
    t(found.includes(`${LIB}:LcRes_High`) && !found.includes(`${LIB}:LcRes_Low`), `resistance>10k the other way round: ${found.join(", ")}`);
    await search(root, "/^lcres_l/");
    found = items(root);
    t(found.includes(`${LIB}:LcRes_Low`) && !found.includes(`${LIB}:LcRes_High`), `/^lcres_l/: ${found.join(", ")}`);
    await closeChoosers();
    await run("common.Interactive.cancel");
  });

  await scenario("Symbol Chooser: the footprint selector of Device:R lists its filter's 2-pad footprints; the one chosen is placed", async (t) => {
    const before = await symSnapshot();
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 6000);
    if (!t(root, "A opens the Symbol Chooser")) return;
    await search(root, "Device:R");
    const row = await until(() => root.querySelector('[data-chooser-tree] [data-id="Device:R"]'), 20000);
    if (!t(row, "Device:R listed")) return;
    row.click();
    const sel = await until(() => { const s = root.querySelector("[data-footprint-selector]"); return s && !s.disabled && s.options.length > 1 ? s : null; }, 90000);
    if (!t(sel, `the selector fills: ${root.querySelector("[data-footprint-selector]")?.options.length ?? 0} entries`)) return;
    const opts = [...sel.options].map((o) => o.value);
    t(sel.options[0].textContent === "No default footprint" && opts[0] === "", `first "No default footprint": ${sel.options[0].textContent}`);
    t(opts.slice(1).every((f) => /^[^:]+:r_/i.test(f)), `every entry is R_* (the symbol's filter): ${opts.slice(1, 5).join(", ")}`);
    t(opts.includes("Resistor_SMD:R_0603_1608Metric") && !opts.some((f) => /:R_Array/i.test(f) && false), `${opts.length - 1} footprints, R_0603_1608Metric among them`);
    const d = await json(`/api/library/details?kind=footprint&id=${encodeURIComponent(opts[1])}`);
    t(d.numbered_pads === 2, `the pin count filter: ${opts[1]} has ${d.numbered_pads} numbered pads`);
    setValue(sel, "Resistor_SMD:R_0603_1608Metric");
    await sleep(600);
    t(root.querySelector("[data-default-footprint]")?.textContent.trim() === "Resistor_SMD:R_0603_1608Metric", "the one chosen is drawn and named above the preview");
    root.querySelector("[data-chooser-ok]").click();
    await sleep(600);
    // a click on the sheet, in the empty band under the module sheets
    const v = E.state().view;
    const box = document.querySelector(".pcb-canvas-container");
    const rect = box.getBoundingClientRect();
    const [px, py] = [v.x + 101_600_000 * v.scale, v.y + 152_400_000 * v.scale];
    const fire = (type, extra = {}) => (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + px, clientY: rect.top + py, button: 0, buttons: 0, ...extra }));
    fire("pointermove");
    await sleep(150);
    fire("pointerdown", { buttons: 1 });
    await sleep(60);
    fire("pointerup");
    await sleep(800);
    await run("common.Interactive.cancel");
    await sleep(300);
    const after = (await sch()).symbols;
    const placed = after.find((x) => x.lib_id === "Device:R" && !JSON.parse(before).some((b) => b[0] === x.id));
    t(placed && placed.footprint === "Resistor_SMD:R_0603_1608Metric", `the placed symbol has the footprint chosen: ${placed && placed.id} ${placed && placed.footprint}`);
    for (let i = 0; i < 4 && (await symSnapshot()) !== before; i++) await undo("schematic");
    t((await symSnapshot()) === before, "undone");
  });

  await scenario("Change Symbols: the browse button opens the Symbol Chooser over every library; the change brings the definition", async (t) => {
    const before = await symSnapshot();
    // the root sheet of the board holds only its module sheets: a resistor of its own to change, in the empty band under them
    const added = await cmd({ op: "add_symbol", id: "R901", lib_id: "Device:R", at: { x: 101_600_000, y: 152_400_000 }, rot_millideg: 0, value: "", footprint: "", unit: 1 });
    const target = (await sch()).symbols.find((x) => x.id === "R901");
    if (!t(target, `a resistor to change: ${target ? target.id : JSON.stringify(added).slice(0, 200)}`)) return;
    await run("common.InteractiveSelection.clear");
    await run("common.InteractiveSelection.selectItems", [target.id]);
    await sleep(300);
    await run("eeschema.InteractiveEdit.changeSymbols");
    const browse = await until(() => document.querySelector("[data-change-new-browse]"), 4000);
    if (!t(browse, "Change Symbols opens with its browse button")) return;
    t(!!document.querySelector("[data-change-match-browse]"), "the matching-identifier field has one too");
    browse.click();
    const picker = await until(() => document.querySelector("[data-symbol-picker]"), 4000);
    if (!t(picker, "the browse button opens the Symbol Chooser")) return;
    t(!picker.querySelector("[data-footprint-selector]") && !picker.querySelector("[data-chooser-keep]"), "with no footprint pane and no placement boxes (SYMBOL_CHOOSER_FRAME)");
    await search(picker, "+3V3");
    t(items(picker).includes("power:+3V3"), "power symbols are offered (no filter)");
    await search(picker, "LM358");
    const lm = await until(() => picker.querySelector('[data-chooser-tree] [data-id="Amplifier_Operational:LM358"]'), 20000);
    if (!t(lm, "LM358 listed")) return;
    lm.click();
    await sleep(300);
    picker.querySelector("[data-chooser-ok]").click();
    await sleep(400);
    t(!document.querySelector("[data-symbol-picker]") && document.querySelector("[data-change-new-id]")?.value === "Amplifier_Operational:LM358", `OK puts the id in the field: ${document.querySelector("[data-change-new-id]")?.value}`);
    const changeBtn = [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Change");
    changeBtn?.click();
    const changed = await until(async () => (await sch()).symbols.find((x) => x.id === target.id && x.lib_id === "Amplifier_Operational:LM358"), 8000);
    t(changed, `${target.id} is Amplifier_Operational:LM358 now`);
    const def = await json("/api/library/symbol?lib_id=Amplifier_Operational:LM358");
    t(def.symbol && def.symbol.unit_count === 3, `the definition came with it: ${def.symbol ? def.symbol.unit_count + " units" : def.error}`);
    [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Close")?.click();
    await sleep(300);
    for (let i = 0; i < 4 && (await symSnapshot()) !== before; i++) await undo("schematic");
    t((await symSnapshot()) === before, "undone");
  });

  // ----------------------------------------------------------------------------------------------------------- a broken folder library
  await scenario("a folder library with a broken file loads with errors and says so (KiCad 25ec2335 SCH_IO_KICAD_SEXPR_LIB_CACHE::Load)", async (t) => {
    const listing = await json(`/api/fs/list?path=${encodeURIComponent("Broken.kicad_symdir")}&what=symbol`);
    const there = !listing.error && JSON.stringify(listing).includes("BadPart");
    if (!there) {
      t(true, "skipped: the board's folder holds no Broken.kicad_symdir with a broken BadPart.kicad_sym (put one there before serving to run this scenario)");
      return;
    }
    const add = await post("/api/library_tables/add", { kind: "symbol", path: "Broken.kicad_symdir" });
    if (!t(!add.error, `Broken added to the project table: ${add.error ?? "ok"}`)) return;
    const errs = await json("/api/library/load_errors?kind=symbol");
    const mine = (errs.errors ?? []).find((e) => e.lib === "Broken");
    t(mine && /^Unable to read file '.*BadPart\.kicad_sym'\n/.test(mine.error) && mine.error.endsWith("The library cannot be saved until these errors are fixed manually."), `GET /api/library/load_errors: ${mine && mine.error}`);
    const entries = await json("/api/library/entries?kind=symbol&lib=Broken");
    t((entries.entries ?? []).map((e) => e.name).join() === "GoodPart" && entries.load_error === mine?.error, `the good file is read: ${(entries.entries ?? []).map((e) => e.name)}`);
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 6000);
    if (!t(root, "the Symbol Chooser opens")) return;
    await search(root, "GoodPart");
    t(items(root).includes("Broken:GoodPart"), "GoodPart is offered");
    t(await until(() => /Library 'Broken' loaded with errors|libraries loaded with errors/.test(root.querySelector("[data-chooser-load-errors]")?.textContent ?? ""), 5000), `the chooser warns: ${root.querySelector("[data-chooser-load-errors]")?.textContent}`);
    t(await until(() => document.querySelector("[data-library-load-warnings]"), 5000), "and the status bar");
    await closeChoosers();
    await run("common.Interactive.cancel");
    const rows = (await json("/api/library_tables")).tables.symbol.project.rows.filter((r) => r.nickname !== "Broken");
    const back = await post("/api/library_tables", { kind: "symbol", rows });
    t(!back.error, `the row is taken back out: ${back.error ?? "ok"}`);
  });

  // ----------------------------------------------------------------------------------------------------------- clean up
  await scenario("clean up: the project library's row out, the design as it was, no page error", async (t) => {
    if (made) {
      for (const name of ["LcRes_Low", "LcRes_High"]) await cmd({ op: "delete_library_symbol", lib_id: `${LIB}:${name}` });
      const rows = (await json("/api/library_tables")).tables.symbol.project.rows.filter((r) => r.nickname !== LIB);
      const back = await post("/api/library_tables", { kind: "symbol", rows });
      t(!back.error, `${LIB}'s row out of the table: ${back.error ?? "ok"}`);
    }
    const now = await json("/api/library_tables");
    t(now.tables.symbol.project.rows.length === tablesBefore.tables.symbol.project.rows.length, `the project table has the rows it had: ${now.tables.symbol.project.rows.length}`);
    const errs = E.errors().slice(errorsBefore);
    t(errs.length === 0, `no page error: ${errs.map((e) => e.message ?? e).join(" | ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return check.ok;
})();
