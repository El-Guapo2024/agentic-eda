// The Footprint Editor against KiCad, UI-LEFT F7, F8, F9 and Y3 (PARITY-fpedit.md section 21): Set Anchor moves the zones, the dimensions, the courtyard and the items
// kept as the text of their forms (`FOOTPRINT::MoveAnchorPosition`); File > Import > Graphics... puts a file's shapes into the footprint on the layer chosen, one undo step;
// Preferences > Footprint Editor (Editing Options' rotation step, Graphics Defaults) set what R turns by and what a line starts with; the Footprint Checker puts a short's
// marker where the pads meet; the Properties pane lists a pad's, a graphic's and the footprint's rows and edits them, one undo step each.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`). It makes footprints of its own in the project library and deletes them at the end; it puts the preferences back.
// It answers through `window.__fpeditToolsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__fpeditToolsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const tick = async (el, on) => { if (el && el.checked !== on) { el.click(); await sleep(120); } };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(150); };
  const until = async (fn, ms = 5000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(120); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => { const f = await json(`/api/footprint?name=${encodeURIComponent(name)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [], pads: f.pads ?? [] }; };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button, button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, wx, wy, buttons = 0, mods = {}) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const clickAt = async (wx, wy, mods = { ctrl: true, shift: true }) => { fire("pointermove", wx, wy, 0, mods); await sleep(70); fire("pointerdown", wx, wy, 1, mods); await sleep(40); fire("pointerup", wx, wy, 0, mods); await sleep(450); };
  const selectAt = async (wx, wy) => clickAt(wx, wy, {});
  const selection = () => (__eda.state().selection ?? []).map((s) => s.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(800); };
  const libraryBefore = await fpNames();
  /** A new footprint (File > New Footprint, opened in the editor) made what a `.kicad_mod` body says: the file read by the studio's reader and put in its place. */
  const putFootprint = async (body) => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !before.includes(n));
    if (!name) return null;
    const parsed = (await post("/api/footprint/parse", `(footprint "X" (version 20240108) (generator "pcbnew") (layer "F.Cu")\n${body}\n)`)).footprint;
    if (!parsed) return null;
    const r = await cmd({ op: "put_library_footprint", footprint: { ...parsed, name }, overwrite: true });
    if (!r.ok) return null;
    await until(async () => (await footprint(name)).pads.length === parsed.pads.length || null, 4000);
    await sleep(900);
    return name;
  };
  const okButton = () => { const all = [...document.querySelectorAll(".dialog-footer button")].filter((b) => b.textContent.trim() === "OK"); return all[all.length - 1]; };

  // --------------------------------------------------------------------------------------------------------------------------- F7
  await scenario("F7: Set Anchor moves the zone, the dimension, the courtyard and a zone kept as text; the point stays", async (t) => {
    const name = await putFootprint(
      `(attr smd)
 (fp_rect (start -2 -1) (end 2 1) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
 (pad "1" smd rect (at -1 0) (size 0.8 0.8) (layers "F.Cu" "F.Paste" "F.Mask"))
 (point (at 2 2) (size 1) (layer "F.Fab"))
 (dimension (type aligned) (layer "F.Fab") (pts (xy 0 2) (xy 2 2)) (height 1) (format (prefix "") (suffix "") (units 3) (units_format 1) (precision 4)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes)) (gr_text "2 mm" (at 1 0.6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)))))
 (zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
 (zone (net 0) (net_name "") (layer "B.Cu") (locked yes) (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))`
    );
    if (!t(name, "the footprint is in the project library")) return;
    const before = await footprint(name);
    t((before.items?.zones ?? []).length + (before.kicad?.tokens ?? []).filter((x) => x.startsWith("(zone")).length === 2 && (before.items?.dimensions ?? []).length === 1 && (before.items?.points ?? []).length === 1, `a zone, a dimension and a point are items: ${JSON.stringify(Object.keys(before.items ?? {}))}`);
    const kept = (before.kicad?.tokens ?? []).filter((x) => x.startsWith("(zone"));
    await __eda.run("common.Control.zoomFitScreen");
    await __eda.run("pcbnew.InteractiveDrawing.setAnchor");
    await sleep(300);
    await clickAt(1_000_000, 500_000);
    const after = await until(async () => { const f = await footprint(name); return f.pads[0]?.at.x !== before.pads[0]?.at.x ? f : null; }, 5000);
    if (!t(after, "the anchor tool moved the footprint's items")) return;
    t(after.pads[0].at.x === -2_000_000 && after.pads[0].at.y === -500_000, `the pad: ${JSON.stringify(after.pads[0].at)}`);
    const z = after.items.zones.find((q) => q.layer === "F.Cu") ?? after.items.zones[0];
    const zp = z.outline?.[0] ?? z.polygon?.[0];
    t(zp && zp.x === -1_000_000 && zp.y === -500_000, `the zone's first corner: ${JSON.stringify(zp)}`);
    const d = after.items.dimensions[0];
    t(d.start?.x === -1_000_000 && d.start?.y === 1_500_000, `the dimension: ${JSON.stringify(d.start)}`);
    t(after.items.points[0].at.x === 2_000_000, `the point stays (MoveAnchorPosition does not move m_points): ${JSON.stringify(after.items.points[0].at)}`);
    const k = (after.kicad?.tokens ?? []).find((x) => x.startsWith("(zone"));
    if (kept.length > 0) t(k && k.includes("(xy -1 -0.5) (xy 0 -0.5) (xy 0 0.5)"), `the zone kept as text moved: ${k}`);
    else t(after.items.zones.length === 2, `both zones are items: ${after.items.zones.length}`);
    t(JSON.stringify(after.courtyard) === JSON.stringify([3_000_000, 1_500_000]), `the courtyard is the box of its graphics, moved: ${JSON.stringify(after.courtyard)}`);
    await undo();
    t((await footprint(name)).pads[0].at.x === -1_000_000, "one undo takes the anchor back");
  });

  // --------------------------------------------------------------------------------------------------------------------------- F8
  let lineFp = null;
  await scenario("F8: Import Graphics puts the file's shapes into the footprint on the layer chosen, one undo step", async (t) => {
    const had = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    lineFp = (await fpNames()).find((n) => !had.includes(n));
    if (!t(lineFp, "a new footprint")) return;
    const before = (await footprint(lineFp)).graphics.length;
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><rect x="1" y="1" width="6" height="4" fill="none" stroke="black" stroke-width="0.2"/><line x1="0" y1="8" x2="8" y2="8" stroke="black" stroke-width="0.2"/></svg>';
    await __eda.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "logo.svg", text: svg });
    const dlg = await until(() => tid("import-graphics"), 3000);
    if (!t(dlg, "the Import Graphics dialog opens in the Footprint Editor")) return;
    await tick(document.querySelector('[aria-label="Place at"]'), true);
    await type(document.querySelector('[aria-label="Place at X"]'), "0");
    await type(document.querySelector('[aria-label="Place at Y"]'), "0");
    await tick(document.querySelector('[aria-label="Set layer"]'), true);
    await choose(document.querySelector('[aria-label="Layer"]'), "F.SilkS");
    await sleep(700);
    okButton()?.click();
    const after = await until(async () => { const f = await footprint(lineFp); return f.graphics.length > before ? f : null; }, 6000);
    if (!t(after, "the shapes are the footprint's")) return;
    const added = after.graphics.slice(before);
    t(added.length >= 2 && added.every((g) => g.layer === "F.SilkS"), `on F.SilkS: ${added.map((g) => `${g.kind}@${g.layer}`).join(", ")}`);
    await undo();
    t((await footprint(lineFp)).graphics.length === before, "one undo takes the import back");
  });

  await scenario("F8: Preferences > Footprint Editor: the rotation step R turns by, the Graphics Defaults a line starts with, the Origins & Axes page", async (t) => {
    if (!lineFp) return t(false, "no footprint");
    await __eda.run("common.SuiteControl.openPreferences");
    if (!t(await until(() => tid("preferences-dialog"), 3000), "Preferences opens")) return;
    for (const id of ["fp_display", "fp_axes", "fp_edit", "fp_graphics"]) t(tid(`prefs-page-${id}`), `the tree has Footprint Editor > ${tid(`prefs-page-${id}`)?.textContent}`);
    tid("prefs-page-fp_edit").click();
    await sleep(200);
    await type(tid("fp-prefs-rotation"), "45");
    tid("prefs-page-fp_graphics").click();
    await sleep(200);
    await type(document.querySelector('[aria-label="Silk Layers Line Thickness"]'), "0.2");
    tid("prefs-page-fp_axes").click();
    await sleep(200);
    tid("fp-prefs-y-up").click();
    await sleep(100);
    tid("preferences-ok").click();
    await sleep(400);
    // a line drawn now on the silkscreen (the editor's first active layer) starts with 0.2 mm
    await __eda.run("common.Control.zoomFitScreen");
    await __eda.run("pcbnew.InteractiveDrawing.line");
    await sleep(300);
    await clickAt(-2_000_000, 5_000_000);
    await clickAt(2_000_000, 5_000_000);
    await clickAt(2_000_000, 5_000_000);
    await __eda.run("common.InteractiveSelection.selectionTool");
    await sleep(600);
    const f = await footprint(lineFp);
    const line = f.graphics.find((g) => g.kind === "segment" && g.start.y === 5_000_000);
    if (!t(line, `a line is drawn: ${JSON.stringify(f.graphics.map((g) => `${g.kind}@${g.layer}`))}`)) return;
    t(line.layer === "F.SilkS" && line.stroke_width === 200_000, `the line on ${line.layer} starts with the Graphics Defaults' 0.2 mm: ${line.stroke_width}`);
    // R turns it by 45 degrees
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(-1_500_000, 5_000_000);
    t(selection().join() === line.id, `the line is selected: ${selection()}`);
    await __eda.run("pcbnew.InteractiveEdit.rotateCcw");
    await sleep(800);
    const r = (await footprint(lineFp)).graphics.find((g) => g.id === line.id);
    const ang = Math.round((Math.atan2(-(r.end.y - r.start.y), r.end.x - r.start.x) * 180) / Math.PI);
    t(Math.abs(Math.abs(ang) - 45) <= 1, `R turned the line by the step, 45 degrees: ${ang}`);
    // the preferences back
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => tid("preferences-dialog"), 3000);
    tid("prefs-page-fp_edit").click();
    await sleep(150);
    await type(tid("fp-prefs-rotation"), "90");
    tid("prefs-page-fp_graphics").click();
    await sleep(150);
    tid("fp-prefs-graphics-reset")?.click();
    tid("prefs-page-fp_axes").click();
    await sleep(150);
    tid("fp-prefs-y-down").click();
    tid("preferences-ok").click();
    await sleep(300);
  });

  // --------------------------------------------------------------------------------------------------------------------------- F9
  await scenario("F9: the Footprint Checker's short is marked where the pads meet, as KiCad's Collide places it", async (t) => {
    const name = await putFootprint(
      `(attr smd)
 (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
 (pad "2" smd rect (at 0.6 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))`
    );
    if (!t(name, "the footprint is in the project library")) return;
    const report = await post("/api/footprint/check", { name });
    const short = (report.markers ?? []).find((m) => m.type === "shorting_items");
    if (!t(short, `a short is found: ${JSON.stringify(report.markers?.map((m) => m.type))}`)) return;
    t(short.at[0] === 300_000 && short.at[1] === -500_000, `at the middle of the edges that meet, (0.3, -0.5) mm: ${JSON.stringify(short.at)}`);
    t(short.items.length === 2 && short.items.every((i) => /^Pad [12]/.test(i.description)), `the two pads: ${short.items.map((i) => i.description).join(" | ")}`);
    // the dialog runs the same checks and lists the short
    await __eda.run("pcbnew.ModuleEditor.checkFootprint");
    const dlg = await until(() => tid("footprint-checker"), 3000);
    if (!t(dlg, "the checker dialog opens")) return;
    [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Run Checks")?.click();
    t(await until(() => tid("fpc-list")?.textContent.includes("[shorting_items]"), 20000), `the dialog lists the short: ${tid("fpc-list")?.textContent.slice(0, 200)}`);
  });

  // --------------------------------------------------------------------------------------------------------------------------- Y3
  await scenario("Y3: the Properties pane shows the footprint with nothing selected and a pad's rows when one is; an edit is one undo step", async (t) => {
    const name = await putFootprint(`(attr smd)\n (pad "1" smd rect (at 0 0) (size 1 0.8) (layers "F.Cu" "F.Paste" "F.Mask"))`);
    if (!t(name, "the footprint is in the project library")) return;
    await __eda.run("common.InteractiveSelection.clear");
    await sleep(400);
    const pane = await until(() => tid("fp-properties"), 3000);
    if (!t(pane, "the pane is docked in the Footprint Editor")) return;
    const row = (n) => pane.querySelector(`.prop-row[data-prop="${n}"]`);
    t(pane.textContent.includes("Footprint") && row("Library Description") && row("Clearance Override") && !row("Component Class"), "with nothing selected: the footprint's rows, no Component Class");
    await __eda.run("common.Control.zoomFitScreen");
    await selectAt(0, 0);
    t(await until(() => row("Size X"), 3000), "a pad's rows");
    t(!row("Net") && !row("Pin Name"), "no Net or Pin Name in a library editor");
    const input = row("Size X").querySelector("input");
    input.focus();
    await type(input, "1.5");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const edited = await until(async () => { const f = await footprint(name); return f.pads[0].size[0] === 1_500_000 ? f : null; }, 4000);
    t(edited, "Size X sets the pad's width");
    await undo();
    t((await footprint(name)).pads[0].size[0] === 1_000_000, "one undo takes it back");
  });

  // ------------------------------------------------------------------------------------------------------------------------ the end
  for (const n of await fpNames()) if (!libraryBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
