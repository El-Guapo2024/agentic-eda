// Import Sheet (GAPS.md item 33, package P4's hierarchy row, where Import Sheet and the Hierarchy Navigator were the two checks it
// lacked): the schematic editor's Import Sheet... (`eeschema.InteractiveDrawing.importSheet`, `SCH_DRAWING_TOOLS::ImportSheet` at
// 8303b2ad) opens KiCad's "Choose Schematic" chooser on the project's `.kicad_sch` files (or a path typed in), reads the file and says
// what it would bring and what `LoadSheetFromFile` finds (a missing library, a loop, symbols with no link, references the design has);
// "Place as sheet" then takes two clicks (the corners), EditSheetProperties names the sheet after the file (`fn.GetName()`,
// `<name>.kicad_sch`) and Import puts a copy of the file in as ONE undo step (`DrawSheet`'s single commit) -- its screen's symbols
// numbered anew, its hierarchical labels,
// and its own nested sheets with their pins. Undo takes the placement and every screen it brought away.
//
// The file is KiCad's QA `qa/data/eeschema/issue10926_1_subsheet_1.kicad_sch` (one symbol on the nets of the hierarchical labels IN and
// OUT, two nested sheets both showing `issue10926_1_subsheet_1_1.kicad_sch`, pins OUT_A and IN_A on each), copied under `fixtures/` of
// the check's scratch board by tools/run-all-checks.sh (`board=mcu30-sheet`). It is under `fixtures/` so the copy the import writes
// (`issue10926_1_subsheet_1.kicad_sch` in the project folder) does not write over the file it reads.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml with those two files under `fixtures/` (see
// tools/run-all-checks.sh), with the schematic tab open -- `node web/studio/e2e/run-check.mjs <url> web/studio/e2e/import-sheet.check.js`
// -- on a COPY of a board: every scenario undoes what it did (and, if it left anything behind, the run undoes until the design is as it
// was). It works through `window.__eda` (the action that opens the chooser, undo, cancel), the dialog's controls and the two pointer
// clicks a user makes on the canvas.
//
// It answers through `window.__importSheetCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-sheet minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__importSheetCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__importSheetSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 3e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__importSheetSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const SOURCE = "fixtures/issue10926_1_subsheet_1.kicad_sch";
  const NESTED = "issue10926_1_subsheet_1_1.kicad_sch";
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  /** The schematic of a sheet path: the placement ids from the root down, joined with "/" (the server splits the query on "/", so they are not escaped). */
  const schOf = (ids) => json(`/api/schematic${ids.length > 0 ? `?sheet=${ids.map(encodeURIComponent).join("/")}` : ""}`);
  const hierarchy = () => json("/api/sch/hierarchy").then((r) => r.sheets);
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(500); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(500); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1200); };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(900); await select([]); await fit(); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const dialog = () => document.querySelector(".dialog");
  const fileRows = () => [...(document.querySelector('[data-testid="import-sheet-files"]')?.querySelectorAll('[role="option"]') ?? [])];
  const labelled = (text) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.includes(text))?.querySelector("input");
  /** Wait until the chooser has read the file (it asks the server what the file would bring after a pause). */
  const waitCheck = async () => {
    for (let i = 0; i < 25; i++) {
      if (/symbols?,/.test(dialog()?.innerText ?? "") && !/Reading /.test(dialog()?.innerText ?? "")) return true;
      await sleep(200);
    }
    return false;
  };

  /** The design as the scenarios touch it: the root's sheet placements and the whole hierarchy, to compare with how it was. */
  const snapshot = async () => {
    const root = await sch("");
    return JSON.stringify({
      sheets: root.sheets.map((s) => [s.id, s.name, s.file, s.at, s.size, s.pins.map((p) => [p.name, p.shape, p.at])]),
      symbols: root.symbols.map((x) => [x.id, x.reference, x.at]),
      labels: root.labels.map((l) => [l.id, l.net, l.at, l.kind]),
      wires: root.wires.map((w) => [w.id, w.pts]),
      hierarchy: (await hierarchy()).map((h) => [h.path, h.name, h.file, h.page]),
    });
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 16; i++) {
      if ((await snapshot()) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the design could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  await leave();
  await fit();
  const view0 = __eda.state().view;
  if (!view0 || !Number.isFinite(view0.scale) || view0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)})`] }); check.done = true; return "no canvas size"; }
  const root0 = await sch("");
  if (!root0.sheets.some((s) => s.name.startsWith("MCU"))) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the module sheets of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no sheets"; }
  const base = await snapshot();
  const baseTree = await hierarchy();
  // The import is placed in the empty corner of the A4 page, clear of the module sheets at the top (30..70 mm, 150..180 mm).
  const A = [30_000_000, 150_000_000];
  const B = [70_000_000, 180_000_000];
  const openChooser = async (t) => {
    await __eda.run("eeschema.InteractiveDrawing.importSheet");
    await sleep(800);
    t(/Choose Schematic/.test(dialog()?.innerText ?? ""), `Import Sheet opens KiCad's "Choose Schematic" (${dialog()?.innerText?.split("\n")[0]})`);
    const rows = fileRows();
    t(rows.length === 2 && rows.some((r) => r.textContent.includes(SOURCE)) && rows.some((r) => r.textContent.includes(NESTED)), `the chooser lists the project's two schematic files (${rows.map((r) => r.textContent.split("\n")[0])})`);
    const row = rows.find((r) => r.textContent.includes(SOURCE));
    row?.click();
    await sleep(300);
    t(await waitCheck(), `choosing ${SOURCE} reads it (${dialog()?.innerText?.match(/\d+ symbols?[^\n]*/)?.[0]})`);
    t(/2 symbols, 2 sheets \(Subsheet_1_1, Subsheet_1_2\), 1 library \(Test-Library\)/.test(dialog()?.innerText ?? ""), "and says what it would bring, as LoadSheetFromFile's check does");
    t(/missing from the current project: Test-Library/.test(dialog()?.innerText ?? ""), "the library the project lacks is a warning, not an error: the file's own definitions are used");
    t(labelled("Place as sheet")?.checked === true, "Place as sheet is on (the studio's starting choice; KiCad's is off)");
    t(labelled("Place as group")?.checked === true && labelled("Place repeated copies")?.checked === false && labelled("Keep annotations")?.checked === false, "Place as group on, repeated copies and Keep annotations off, as the chooser starts");
    const open = buttonText(dialog(), "Open");
    t(!!open && open.disabled === false, "Open is offered for a file whose checks found no error");
    open?.click();
    await sleep(600);
    t(!dialog(), "the chooser closes");
    t(__eda.state().tool === "sch_import_sheet", `and arms the placement tool (${__eda.state().tool})`);
  };

  (async () => {
    // ----- the chooser
    await scenario("Choose Schematic: the project's files, what the file brings, LoadSheetFromFile's checks; Open arms the tool", async (t) => {
      await openChooser(t);
      await escape();
      t(__eda.state().tool === "select", "Esc puts the tool down without placing anything");
    });

    // ----- the import
    await scenario("Import: two clicks, Sheet Properties named after the file, Import -- one undo takes the sheet and its screens away", async (t) => {
      await openChooser(t);
      await clickAt(A[0], A[1]);
      await clickAt(B[0], B[1]);
      t(/Hierarchical Sheet Properties/.test(dialog()?.innerText ?? ""), "the second click sizes it and opens Sheet Properties");
      const inputs = [...(dialog()?.querySelectorAll("input") ?? [])];
      t(inputs.length === 2 && inputs[0].value === "issue10926_1_subsheet_1" && inputs[1].value === "issue10926_1_subsheet_1.kicad_sch", `named after the file, as DrawSheet names it: fn.GetName(), <name>.kicad_sch (${inputs.map((i) => i.value)})`);
      t(/goes in the project as issue10926_1_subsheet_1.kicad_sch/.test(dialog()?.innerText ?? ""), "the dialog says the copy's file, not the file read");
      const importButton = buttonText(dialog(), "Import");
      t(!!importButton && importButton.disabled === false, "Import is offered");
      importButton?.click();
      await sleep(2500);
      t(!dialog(), "Import closes the dialog");
      const root = await sch("");
      const made = root.sheets.filter((s) => !root0.sheets.some((o) => o.id === s.id));
      t(made.length === 1, `one sheet placement came in (${made.map((s) => s.name)})`);
      const sheet = made[0];
      if (!sheet) return;
      t(sheet.name === "issue10926_1_subsheet_1" && sheet.file === "issue10926_1_subsheet_1.kicad_sch", `it shows the copy under the file the dialog named (${sheet.name}: ${sheet.file})`);
      t(sheet.file !== SOURCE, "which is not the fixture it read");
      t(Math.abs(sheet.at[0] - A[0]) < 2_000_000 && Math.abs(sheet.at[1] - A[1]) < 2_000_000, `at the first click (${sheet.at})`);
      t(sheet.size[0] > 20_000_000 && sheet.size[1] > 10_000_000, `sized by the second (${sheet.size})`);
      // SCH_EDIT_FRAME::LoadSheetFromFile / DrawSheet: the sheet symbol has no pins; the file's own hierarchical labels are not turned into them
      t(sheet.pins.length === 0, `the imported sheet symbol has no pins of its own -- LoadSheetFromFile creates none (${sheet.pins.length})`);
      const tree = await hierarchy();
      const entry = tree.find((h) => h.path.length === 1 && h.path[0] === sheet.id);
      const below = tree.filter((h) => h.path.length === 2 && h.path[0] === sheet.id);
      t(!!entry && entry.file === sheet.file, "the hierarchy lists it");
      t(below.length === 2 && below.every((h) => h.file === NESTED) && same(below.map((h) => h.name), ["Subsheet_1_1", "Subsheet_1_2"]), `and the two sheets its screen places, both showing ${NESTED} (${below.map((h) => h.name)})`);
      // what its screen holds
      await enterSheet(sheet.id);
      t(same(__eda.state().sheetPath, [sheet.id]), "the imported sheet opens in the editor");
      const inside = await sch(sheet.id);
      t(inside.file === "issue10926_1_subsheet_1.kicad_sch", `the sheet shows the copy (${inside.file})`);
      t(inside.symbols.length === 1 && /^U\d+$/.test(inside.symbols[0].reference ?? ""), `its symbol came in, numbered anew (${inside.symbols[0]?.reference})`);
      const pinNets = inside.symbols[0]?.pins.map((p) => p.net) ?? [];
      t(pinNets.length === 2 && pinNets.every(Boolean) && new Set(pinNets).size === 2, `with its two pins on the nets of the screen's hierarchical labels (${pinNets})`);
      const labels = inside.labels.map((l) => l.net).sort();
      t(same(labels, ["IN", "OUT"]) && inside.labels.every((l) => l.scope === "hierarchical"), `the hierarchical labels IN and OUT came in (${labels})`);
      t(inside.sheets.length === 2 && inside.sheets.every((s) => s.file === NESTED && same(s.pins.map((p) => [p.name, p.shape]).sort(), [["IN_A", "input"], ["OUT_A", "output"]])), `the two nested sheets' pins OUT_A and IN_A came in with them (${inside.sheets.map((s) => s.pins.map((p) => p.name))})`);
      // a screen of the imported tree, one level down: its symbol and its own hierarchical labels
      const nested = inside.sheets[0];
      const deep = await schOf([sheet.id, nested.id]);
      t(deep.file === NESTED && deep.symbols.length === 1 && same(deep.labels.map((l) => l.net).sort(), ["IN_A", "OUT_A"]), `one level down, ${NESTED} shows its symbol and the hierarchical labels IN_A and OUT_A (${deep.labels.map((l) => l.net)})`);
      // one undo takes the placement and every screen the import brought
      await leave();
      await select([]);
      await undo();
      const after = await sch("");
      t(after.sheets.length === root0.sheets.length && !after.sheets.some((s) => s.id === sheet.id), "one Undo takes the placement away");
      t(same(await hierarchy(), baseTree), "and the screens it brought leave the hierarchy with it");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await leave();
    const end = await snapshot();
    results.push({ name: "end: the design is as it was", ok: end === base, notes: [`${end === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
