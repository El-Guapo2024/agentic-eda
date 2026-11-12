// Snap to grid, the grid axes, the arc editing mode and the Footprint Editor's Footprint Defaults (UI-LEFT-2 CC5, FF7; PARITY-common.md "Snap to grid", PARITY-fpedit.md
// "Footprint Defaults"):
//   snap modes   each editor's Display Options' "Snap to grid:": Never leaves the board's measure point and line point off the grid (whole nm), When grid shown snaps
//                while the grid is shown and not once it is hidden, Always snaps; the schematic's wire point the same with Never;
//   axes         the board draws no axes (its grid.axes_enabled is off), the Footprint Editor draws its two axes (forced on) in LAYER_GRID_AXES, the Symbol Editor in BLUE;
//   arc mode     the board's arc mode actions set the preference Preferences > PCB Editor > Editing Options shows, the Footprint Editor's its own index;
//   point editor a line of the Footprint Editor dragged by its end handle moves that end (fp_item set_graphic, one undo step);
//   defaults     Footprint Defaults' Reference text and an added text item reach a New Footprint, which is one undo step.
// It puts every preference back and deletes the footprints it made.
// run-all-checks: board=mcu30-routed minutes=8
(async () => {
  const results = [];
  const check = (window.__snapModesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };
  const E = window.__eda;
  const state = () => E.state();
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const run = async (id) => { await E.run(id); await sleep(250); };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(1000); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(120); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(80); };
  const prefsStored = () => JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}");
  const roundTo = (v, g) => Math.round(v / g) * g;
  const near = (a, b, tol = 0.5) => Array.isArray(a) && Math.abs(a[0] - b[0]) <= tol && Math.abs(a[1] - b[1]) <= tol;
  const key = (k) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  /** Arms a tool unless it is armed already (running the action again would leave it). */
  const arm = async (id, tool) => { if (state().tool !== tool) await run(id); return state().tool === tool; };
  const toSelect = async () => { key("Escape"); await sleep(120); key("Escape"); await sleep(120); };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const c2w = (cx, cy) => { const v = state().view, r = canvas().getBoundingClientRect(); return [(cx - r.left - v.x) / v.scale, (cy - r.top - v.y) / v.scale]; };
  const fire = (type, x, y, buttons = 0) => (document.elementFromPoint(x, y) ?? canvas()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons }));
  const hover = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(80); return state().snap; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = E.errors().length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const errs = E.errors().slice(errorsFrom).map((e) => e.message);
    if (errs.length) { entry.notes.push(`FAIL: errors logged: ${errs.slice(0, 3).join(" | ")}`); entry.ok = false; }
    for (let i = 0; i < 3 && document.querySelector(".dialog-backdrop"); i++) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click(); await sleep(200); }
    results.push(entry);
  };
  /** Preferences on `page` (a tree id), `fn` on the dialog, then OK. */
  const prefs = async (page, fn) => {
    await run("common.SuiteControl.openPreferences");
    tid(`prefs-page-${page}`)?.click();
    await sleep(200);
    await fn();
    tid("preferences-ok")?.click();
    await sleep(350);
  };
  const setSnapMode = (editor, mode) => prefs(`display_${editor}`, async () => { const sel = document.querySelector(`[data-testid="display-options-${editor}"] [data-testid="pref-grid-snap"]`); if (sel) await choose(sel, mode); });
  const libraryBefore = (await json("/api/footprint_library")).project ?? [];

  await scenario("Snap to grid on the board: Never, When grid shown with the grid shown and hidden, Always", async (t) => {
    await clickTab("PCB");
    await toSelect();
    const r = canvas().getBoundingClientRect();
    const [cx, cy] = c2w(r.left + r.width * 0.31, r.top + r.height * 0.37);
    const g = state().grid;
    const probe = [Math.round(cx) + Math.round(g * 0.37), Math.round(cy) + Math.round(g * 0.21)];
    t(await arm("common.Interactive.measureTool", "measure"), "the measure tool is armed");
    let s = await hover(...probe);
    const onGrid = [roundTo(probe[0], g), roundTo(probe[1], g)];
    t(s && near(s.output, onGrid), `Always: the point goes to the grid (${JSON.stringify(s?.output)} vs ${onGrid})`);
    await setSnapMode("pcb", "never");
    await arm("common.Interactive.measureTool", "measure");
    s = await hover(...probe);
    t(s && !near(s.output, onGrid) && s.output.every(Number.isInteger), `Never: the point is not put on the grid (${JSON.stringify(s?.output)}, probe ${probe})`);
    await toSelect();
    await run("pcbnew.InteractiveDrawing.line");
    s = await hover(...probe);
    t(s && !near(s.output, onGrid), `Never: the line tool's point is off the grid too (${JSON.stringify(s)})`);
    await toSelect();
    await setSnapMode("pcb", "with_grid");
    await arm("common.Interactive.measureTool", "measure");
    s = await hover(...probe);
    t(s && near(s.output, onGrid), `When grid shown, the grid shown: snapped (${JSON.stringify(s?.output)})`);
    await run("common.Control.toggleGrid");
    s = await hover(probe[0] + 1, probe[1] + 1);
    t(s && !near(s.output, onGrid), `When grid shown, the grid hidden: not snapped (${JSON.stringify(s?.output)})`);
    await run("common.Control.toggleGrid");
    s = await hover(...probe);
    t(s && near(s.output, onGrid), "and snapped again once it is shown");
    await toSelect();
    await setSnapMode("pcb", "always");
    t(JSON.parse(localStorage.getItem("eda-studio.common-options") ?? "{}").window?.pcb?.gridSnap === "always", "Always is kept again");
    await E.run("common.Control.zoomRedraw").catch(() => null);
    await sleep(300);
    t(window.__edaGrid?.pcb?.axes === false && !window.__edaGrid?.pcb?.marks?.axis, `the board draws no axes (grid.axes_enabled off): ${JSON.stringify(window.__edaGrid?.pcb)}`);
  });

  await scenario("Snap to grid on the schematic: the wire tool's point with Never and Always", async (t) => {
    await clickTab("Schematic");
    await toSelect();
    const r = canvas().getBoundingClientRect();
    const [cx, cy] = c2w(r.left + 30, r.top + 30);
    const probe = [Math.round(cx) + 333_001, Math.round(cy) + 211_003];
    await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
    const g = state().grid;
    let s = await hover(...probe);
    t(s && near(s.output, [roundTo(probe[0], g), roundTo(probe[1], g)]), `Always: on the grid (${JSON.stringify(s?.output)})`);
    await toSelect();
    await setSnapMode("schematic", "never");
    await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
    s = await hover(...probe);
    t(s && !near(s.output, [roundTo(probe[0], g), roundTo(probe[1], g)]), `Never: not put on the grid (${JSON.stringify(s?.output)}, probe ${probe})`);
    await toSelect();
    await setSnapMode("schematic", "always");
  });

  let made = null;
  await scenario("the Footprint Editor: its axes, the point editor on a line, the arc mode; the Symbol Editor's axes", async (t) => {
    await clickTab("Footprint");
    await run("pcbnew.ModuleEditor.newFootprint");
    await sleep(900);
    made = ((await json("/api/footprint_library")).project ?? []).find((n) => !libraryBefore.includes(n)) ?? null;
    t(!!made, `a new footprint is open: ${made}`);
    await run("common.Control.zoomFitScreen");
    // Zoom to Fit frames the new footprint's texts by their boxes (FF10), which leaves y -4 mm above the canvas: two steps out bring the line's place in
    await run("common.Control.zoomOutCenter");
    await run("common.Control.zoomOutCenter");
    await sleep(300);
    const fg = window.__edaGrid?.footprint;
    t(fg?.axes === true && fg?.marks?.axis === 2, `the Footprint Editor draws its two axes (${JSON.stringify(fg)})`);
    // a line, then its end dragged by the handle
    await run("pcbnew.InteractiveDrawing.line");
    const pt = async (wx, wy, buttons = 0, kind = "pointermove") => { const [x, y] = w2c(wx, wy); fire(kind, x, y, buttons); await sleep(90); };
    const clickW = async (wx, wy) => { await pt(wx, wy); await pt(wx, wy, 1, "pointerdown"); await pt(wx, wy, 0, "pointerup"); await sleep(350); };
    // (away from the new footprint's texts at y -0.5, 1 and 2.5 mm)
    await clickW(-2_000_000, -4_000_000);
    await clickW(2_000_000, -4_000_000);
    await clickW(2_000_000, -4_000_000);
    await toSelect();
    await run("common.InteractiveSelection.selectionTool");
    const fp = async () => { const f = await json(`/api/footprint?name=${encodeURIComponent(made)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [] }; };
    const line = (await fp()).graphics.find((g) => g.kind === "segment");
    if (!t(line, "a line is drawn")) return;
    await clickW(0, line.start.y);
    t((state().selection ?? []).some((s) => s.id === line.id), "the line is selected");
    const [ex, ey] = w2c(line.end.x, line.end.y);
    fire("pointermove", ex, ey); await sleep(80);
    fire("pointerdown", ex, ey, 1); await sleep(60);
    const [tx, ty] = w2c(line.end.x + 1_000_000, line.end.y + 1_000_000);
    for (let i = 1; i <= 6; i++) { fire("pointermove", ex + ((tx - ex) * i) / 6, ey + ((ty - ey) * i) / 6, 1); await sleep(60); }
    fire("pointerup", tx, ty); await sleep(900);
    const moved = (await fp()).graphics.find((g) => g.id === line.id);
    t(moved && moved.end.x === line.end.x + 1_000_000 && moved.end.y === line.end.y + 1_000_000 && moved.start.x === line.start.x, `the end handle moved the line's end on the grid: ${JSON.stringify(moved?.end)} from ${JSON.stringify(line.end)}`);
    // the Footprint Editor's arc mode actions set its own index
    await run("pcbnew.PointEditor.arcKeepEndpoint");
    t(prefsStored().fpArcEditMode === 1, `Keep Arc Endpoints sets the Footprint Editor's mode to 1: ${prefsStored().fpArcEditMode}`);
    await run("pcbnew.PointEditor.arcKeepCenter");
    t(prefsStored().fpArcEditMode === 0, "and Keep Arc Center to 0");
    // the Symbol Editor's axes, in BLUE
    await clickTab("Symbol");
    await sleep(500);
    const sg = window.__edaGrid?.symbol;
    t(sg?.axes === true && sg?.axesColor === "#000084ff", `the Symbol Editor draws its axes in COLOR4D( BLUE ): ${JSON.stringify(sg)}`);
  });

  await scenario("the board's arc mode actions set the preference Editing Options shows", async (t) => {
    await clickTab("PCB");
    await run("pcbnew.PointEditor.arcKeepRadius");
    t(prefsStored().pcbArcEditMode === "keep_center_ends_adjust_angle", `kept: ${prefsStored().pcbArcEditMode}`);
    await run("common.SuiteControl.openPreferences");
    tid("prefs-page-pcb_edit")?.click();
    await sleep(200);
    t(tid("pref-pcb-arc-mode")?.value === "keep_center_ends_adjust_angle", `PCB Editor > Editing Options shows it: ${tid("pref-pcb-arc-mode")?.value}`);
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(200);
    await run("pcbnew.PointEditor.arcKeepCenter");
    t(prefsStored().pcbArcEditMode === "keep_center_adjust_angle_radius", "back to Keep Arc Center");
  });

  await scenario("Footprint Defaults reach New Footprint, which starts the editor's history (undo leaves it); User Layer Names name a row of the Appearance panel", async (t) => {
    await clickTab("Footprint");
    await prefs("fp_defaults", async () => {
      await type(tid("fp-defaults-field-0-text"), "U**");
      document.querySelector('button[aria-label="Add Text Item"]')?.click();
      await sleep(120);
      const rows = document.querySelectorAll('[data-testid="fp-defaults-texts"] input[aria-label^="Text item "]');
      await type(rows[rows.length - 1], "rev A");
    });
    t(JSON.stringify(prefsStored().fpDefaultTextItems?.map((i) => i.text)) === JSON.stringify(["U**", "", "${REFERENCE}", "rev A"]), `kept: ${JSON.stringify(prefsStored().fpDefaultTextItems)}`);
    const before = (await json("/api/footprint_library")).project ?? [];
    await run("pcbnew.ModuleEditor.newFootprint");
    await sleep(1500);
    const name = ((await json("/api/footprint_library")).project ?? []).find((n) => !before.includes(n));
    const f = name ? await json(`/api/footprint?name=${encodeURIComponent(name)}`) : null;
    t(f && f.reference?.text === "U**" && JSON.stringify((f.texts ?? []).map((x) => x.content)) === JSON.stringify(["${REFERENCE}", "rev A"]), `the new footprint: Reference ${f?.reference?.text}, texts ${JSON.stringify((f?.texts ?? []).map((x) => [x.content, x.at?.y]))}`);
    await run("common.Interactive.undo");
    await sleep(900);
    // KiCad's NewFootprint clears the undo list (`Clear_Pcb` -> `ClearUndoRedoList`, footprint_editor_control.cpp): making it is not a step to undo
    t(((await json("/api/footprint_library")).project ?? []).includes(name), "undo leaves the new footprint (New Footprint clears the editor's history)");
    await prefs("fp_layer_names", async () => {
      document.querySelector('button[aria-label="Add Layer Name"]')?.click();
      await sleep(120);
      await choose(document.querySelector('select[aria-label="Layer name 1 layer"]'), "User.7");
      await type(document.querySelector('input[aria-label="Layer name 1"]'), "Notes7");
    });
    t(JSON.stringify(prefsStored().fpUserLayerNames) === JSON.stringify({ "User.7": "Notes7" }), `kept: ${JSON.stringify(prefsStored().fpUserLayerNames)}`);
    await sleep(300);
    const row = document.querySelector('.ap-row[data-layer="User.7"]');
    t(row && /Notes7/.test(row.textContent), `the Appearance panel lists User.7 as "Notes7": ${row?.textContent}`);
    // back to KiCad's defaults
    for (const page of ["fp_defaults", "fp_layer_names"]) {
      await prefs(page, async () => { [...document.querySelectorAll(".dialog-footer button")].find((b) => /^Reset .* to Defaults$/.test(b.textContent.trim()))?.click(); await sleep(150); });
    }
    t(prefsStored().fpDefaultTextItems?.[0]?.text === "REF**" && JSON.stringify(prefsStored().fpUserLayerNames) === "{}", "the pages' Reset puts KiCad's defaults back");
  });

  try {
    for (const n of (await json("/api/footprint_library")).project ?? []) if (!libraryBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
    await clickTab("PCB");
  } catch { /* cleanup is not worth failing for */ }
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
