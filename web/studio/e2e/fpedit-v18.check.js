// The Footprint Editor's round-3 leftovers of lane fpedit-2 (UI-LEFT-3 F3-5, F3-6, F3-7, F3-8, F3-9; task-v18-fpedit; PARITY-fpedit section 30). F3-11 and F3-12 are the
// server's (crates/cli/src/fp_library_record_tests.rs, crates/kicad/tests/fp_items_io.rs).
//   F3-6   Footprint Defaults' text item rows are picked as a wxGridSelectRows grid's (a click on a cell selects the row, Ctrl adds one, Delete takes the selected rows,
//          else the cursor's); New Footprint's extra text takes its layer class's italic and keep-upright from Graphics Defaults;
//   F3-7   Sketch Graphic Items is kept in the preferences; the Renumber Pads prompt is a popup at the cursor; Mirror of a point says so in a message box ("PCB_POINT");
//   F3-8   with the PCB Editor's Display Origin on the grid origin and the Footprint Editor's X axis increasing left, the status bar, the graphic dialog, the text
//          dialog's offset, the Properties pane and Move Corner To show the footprint's coordinates through them; Space sets the local origin of dx / dy;
//   F3-9   View > Flip Board View mirrors the canvas (a click picks what is drawn under it), its check mark and the Appearance panel's box follow, R turns the other way;
//   F3-5   the read-only info bar's "Save as editable copy": cancelled, the editor's modified flag is cleared all the same; saved, the copy opens unmodified.
//
// Run it on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`), with KiCad's footprint libraries
// installed read-only (/usr/share/kicad-nightly, or KICAD_PATH): it makes a footprint of its own and puts the preferences it changes back. It answers through
// `window.__fpV18Check`: { results, done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (window.__fpV18Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results };
  };
  const E = window.__eda;
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const until = async (cond, ms = 10000, step = 150) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(80); };
  const prefsStored = () => JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}");
  const openPrefs = async (page) => {
    await E.run("common.SuiteControl.openPreferences");
    await sleep(300);
    tid(`prefs-page-${page}`)?.click();
    await sleep(250);
  };
  const prefsOk = async () => { tid("preferences-ok")?.click(); await sleep(400); };
  const prefs = async (page, fn) => { await openPrefs(page); await fn(); await prefsOk(); };
  const resetPage = async (page) => prefs(page, async () => { [...document.querySelectorAll(".dialog-footer button")].find((b) => /^Reset .* to Defaults$/.test(b.textContent.trim()))?.click(); await sleep(150); });
  const closeDialogs = async () => { for (let i = 0; i < 4 && document.querySelector(".dialog-backdrop"); i++) { const b = [...document.querySelectorAll(".dialog-backdrop button")].reverse().find((x) => /^(Cancel|OK|Close)$/.test(x.textContent.trim())); if (b) b.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(250); } };
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
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container") ?? document.querySelector(".pcb-canvas-container");
  // the check's own record of Flip Board View (it is the one that turns it on and off)
  let isFlipped = false;
  const flipped = () => isFlipped;
  /** A world point on the screen, through the view and, when the view is flipped, its mirror about the canvas middle. */
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); const sx = v.x + wx * v.scale; return [r.left + (flipped() ? r.width - sx : sx), r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0, mods = {}) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", isPrimary: true, ...mods })); };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(200); };
  const clickAt = async (wx, wy) => { await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(700); };
  const statusText = () => [...document.querySelectorAll(".status-bar .field, .statusbar .field, footer .field")].map((e) => e.textContent).join(" | ");
  const mm = (v) => Math.round(v * 1e6);

  const before = await fpNames();
  let name = null;
  const ids = {};
  const POLY = [[10, 0], [14, 0], [14, 1], [10, 3]].map(([x, y]) => ({ x: mm(x), y: mm(y) }));

  await scenario("setup: a footprint with a polygon, a text, a point and a pad", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Footprint")?.click();
    await until(() => E.state().tab === "footprint", 4000);
    await sleep(500);
    await E.run("pcbnew.ModuleEditor.newFootprint");
    name = await until(async () => (await fpNames()).find((n) => !before.includes(n)), 8000);
    if (!t(!!name, `a new footprint (${name})`)) return;
    const r = [
      await cmd({ op: "add_footprint_graphic", footprint: name, shape: { kind: "polygon", layer: "F.SilkS", stroke_width: 120_000, filled: false, pts: POLY } }),
      await cmd({ op: "add_footprint_text", footprint: name, text: { content: "HELLO", at: { x: mm(-4), y: mm(9) }, angle: 0, layer: "F.Fab", size_nm: mm(1), stroke_width: mm(0.15), justify: "center", mirror: false } }),
      await cmd({ op: "fp_item", verb: "add", footprint: name, item: { point: { layer: "User.1", at: { x: mm(-6), y: mm(-6) }, size_nm: mm(1) } } }),
      await cmd({ op: "add_pad", footprint: name, pad: { number: "1", at: { x: mm(-8), y: 0 }, size: [mm(1), mm(1)], offset: { x: 0, y: 0 }, shape: "rect", kind: "smd", drill: null, drill_slot: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"] } }),
    ];
    t(r.every((x) => x.ok !== false), `added (${r.map((x) => x.message ?? "ok").join(", ")})`);
    const fp = await footprint(name);
    ids.poly = fp.graphics.find((g) => g.kind === "polygon")?.id;
    ids.text = fp.texts.find((x) => x.content === "HELLO")?.id;
    ids.point = (fp.items?.points ?? [])[0]?.id;
    t(ids.poly && ids.text && ids.point, `ids ${JSON.stringify(ids)}`);
    await E.run("common.Control.zoomFitScreen");
    await sleep(700);
  });

  await scenario("F3-6: Footprint Defaults' text rows are picked by the row (a click, Ctrl+click), Delete takes the selected rows, else the cursor's", async (t) => {
    await openPrefs("fp_defaults");
    const table = await until(() => tid("fp-defaults-texts"), 3000);
    if (!t(table, "the Footprint Defaults page")) return;
    t(!table.querySelector("input[type=checkbox]"), "no check boxes to pick a row");
    const rows = () => [...table.querySelectorAll("tbody tr")];
    const n0 = rows().length;
    document.querySelector('button[aria-label="Add Text Item"]').click();
    await sleep(250);
    document.querySelector('button[aria-label="Add Text Item"]').click();
    await sleep(250);
    t(rows().length === n0 + 2, `two rows added (${rows().length})`);
    t(document.activeElement === rows()[n0 + 1]?.querySelector("input"), "Add puts the cursor on the new row and edits its text (`OnAddRow`)");
    rows()[n0].querySelector("td").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await sleep(100);
    t(rows()[n0].getAttribute("aria-selected") === "true" && rows()[n0 + 1].getAttribute("aria-selected") === "false", "a click on a cell selects its row alone");
    rows()[n0 + 1].querySelector("td").dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true }));
    await sleep(100);
    t(rows()[n0].getAttribute("aria-selected") === "true" && rows()[n0 + 1].getAttribute("aria-selected") === "true", "Ctrl+click adds a row");
    document.querySelector('button[aria-label="Delete Text Item"]').click();
    await sleep(200);
    t(rows().length === n0, `Delete takes both (${rows().length})`);
    document.querySelector('button[aria-label="Add Text Item"]').click();
    await sleep(250);
    document.querySelector('button[aria-label="Delete Text Item"]').click();
    await sleep(200);
    t(rows().length === n0, "nothing selected: Delete takes the cursor's row (the one just added)");
    await closeDialogs();
  });

  await scenario("F3-6: New Footprint's other text takes its layer class's italic and keep-upright (Graphics Defaults' Fab Layers)", async (t) => {
    await prefs("fp_graphics", async () => {
      const it = document.querySelector('input[aria-label="Fab Layers Italic"]');
      const up = document.querySelector('input[aria-label="Fab Layers Keep Upright"]');
      if (it && !it.checked) it.click();
      if (up && up.checked) up.click();
      await sleep(100);
    });
    const had = await fpNames();
    await E.run("pcbnew.ModuleEditor.newFootprint");
    const made = await until(async () => (await fpNames()).find((n) => !had.includes(n)), 8000);
    if (t(made, `a new footprint (${made})`)) {
      const fp = await footprint(made);
      const ref = fp.texts.find((x) => x.content === "${REFERENCE}");
      const extra = (fp.kicad?.text_extras ?? []).find((e) => e.id === ref?.id);
      t(ref && extra?.italic === true, `the \${REFERENCE} text on F.Fab is italic (${JSON.stringify(extra)})`);
      t(ref && (fp.kicad?.unlocked_texts ?? []).includes(ref.id), "and not kept upright");
      await cmd({ op: "delete_library_footprint", name: made });
    }
    await resetPage("fp_graphics");
  });
  // (open the check's footprint again: New Footprint opened the other)
  if (name && E.state().tab === "footprint") {
    const row = document.querySelector(`[role=treeitem][data-name="${CSS.escape(name)}"]`);
    row?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await sleep(800);
    await sleep(600);
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
  }

  await scenario("F3-7: Sketch Graphic Items is kept with the preferences", async (t) => {
    const was = prefsStored().fpGraphicsFill;
    await E.run("pcbnew.Control.graphicOutlines");
    await sleep(300);
    t(prefsStored().fpGraphicsFill === false, `outlines: pcb_display.graphics_fill off (${prefsStored().fpGraphicsFill})`);
    await E.run("pcbnew.Control.graphicOutlines");
    await sleep(300);
    t(prefsStored().fpGraphicsFill !== false, `filled again (${prefsStored().fpGraphicsFill}, was ${was})`);
  });

  await scenario("F3-7: Mirror of a point says so in KiCad's message box (titled PCB_POINT) and leaves it where it is", async (t) => {
    await select([ids.point]);
    E.run("pcbnew.InteractiveEdit.mirrorHoriontally");
    const box = await until(() => [...document.querySelectorAll('[data-testid="message-box"]')].find((d) => d.getAttribute("aria-label") === "PCB_POINT"), 4000);
    t(box && /virtual BOARD_ITEM::Mirror used, should not occur/.test(box.textContent), `the box: "${box?.textContent}"`);
    button(box, "OK")?.click();
    await sleep(500);
    const p = ((await footprint(name)).items?.points ?? [])[0];
    t(p && p.at.x === mm(-6) && p.at.y === mm(-6), `the point stays (${JSON.stringify(p?.at)})`);
  });

  await scenario("F3-7: the Renumber Pads prompt is a popup at the cursor (`statusPopup.Move( mouse + ( 20, 20 ) )`), not a banner", async (t) => {
    E.run("pcbnew.PadTool.enumeratePads");
    const d = await until(() => [...document.querySelectorAll(".dialog")].find((x) => /Pad Enumeration Settings/.test(x.querySelector(".dialog-header")?.textContent ?? "")), 3000);
    if (!t(d, "the Pad Enumeration Settings dialog")) return;
    button(d, "OK")?.click();
    await sleep(400);
    fire("pointermove", mm(0), mm(0));
    await sleep(300);
    const popup = tid("fp-enumerate-popup");
    const [sx, sy] = toScreen(mm(0), mm(0));
    const r = popup?.getBoundingClientRect();
    t(popup && /Click on pad/.test(popup.textContent), `the popup (${popup?.textContent})`);
    t(r && Math.abs(r.left - (sx + 20)) <= 2 && Math.abs(r.top - (sy + 20)) <= 2, `at the cursor plus (20, 20): ${r && [r.left, r.top]} vs ${[sx + 20, sy + 20]}`);
    canvasEl().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(500);
    t(!tid("fp-enumerate-popup"), "Esc ends it");
  });

  await scenario("F3-8: Display Origin on the grid origin and X increasing left -- the status bar, the shape dialog, the text dialog, the Properties pane, Move Corner To", async (t) => {
    // the Footprint Editor's grid origin at (2, 1) mm, the PCB Editor's Display Origin on it, the Footprint Editor's X axis increasing left
    await E.run("common.Control.editGridOrigin");
    const g = await until(() => dialog("Grid Origin"), 3000);
    if (!t(g, "the Grid Origin dialog")) return;
    await type(g.querySelector('input[aria-label="X"]'), "2");
    await type(g.querySelector('input[aria-label="Y"]'), "1");
    button(g, "OK")?.click();
    await sleep(400);
    await prefs("pcb_axes", async () => { tid("pcb-prefs-origin-grid")?.click(); await sleep(100); });
    await prefs("fp_axes", async () => { tid("fp-prefs-x-left")?.click(); await sleep(100); });
    t(prefsStored().pcbDisplayOrigin === "grid" && prefsStored().fpInvertXAxis === true, `set (${prefsStored().pcbDisplayOrigin}, ${prefsStored().fpInvertXAxis})`);
    // the status bar: the cursor at (10, 0) mm reads X -8 (from 2, leftward) and Y -1
    await hover(mm(10), mm(0));
    await sleep(300);
    const s = statusText();
    t(/X -8(\.0+)? *(mm)? +Y -1(\.0+)?/.test(s) || /-8\.0+.*-1\.0+/.test(s), `the status bar (${s})`);
    // Space: the local origin here, dx / dy from it
    E.run("common.Control.resetLocalCoords");
    await hover(mm(12), mm(0));
    await sleep(300);
    t(/dx -2(\.0+)?/.test(statusText()), `dx through ToDisplayRelX from the local origin (${statusText()})`);
    // the shape dialog: the polygon's points are not in it -- a line is; use the Properties pane's Position X of the text (ABS) and the text dialog's offset (REL)
    await select([ids.text]);
    const pane = await until(() => tid("fp-properties"), 2000);
    const posX = pane?.querySelector('[data-prop="Position X"] input');
    t(posX && Number.parseFloat(posX.value) === 6, `Properties: the text's Position X is -4 from 2, leftward: 6 (${posX?.value})`);
    const opened = await E.run("pcbnew.InteractiveEdit.properties");
    const tx = await until(() => tid("fp-text-edit-x"), 3000);
    t(tx && Number(tx.value) === 4, `the text dialog's Offset X is -(-4) (${tx?.value}; ${JSON.stringify(opened)}; dialogs ${JSON.stringify(E.state().dialogs)})`);
    await closeDialogs();
    // Move Corner To on the polygon's corner at (14, 1): the dialog shows (-12, 0) and (-13, 0) moves it to (15, 1)
    await select([ids.poly]);
    await hover(mm(14), mm(1));
    const mc = await E.run("pcbnew.InteractiveEdit.moveCorner");
    void mc;
    const pe = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Move Corner to Location/.test(d.querySelector(".dialog-header")?.textContent ?? "")), 3000);
    const inputs = pe ? [...pe.querySelectorAll("input")] : [];
    t(inputs.length >= 2 && Number.parseFloat(inputs[0].value) === -12 && Number.parseFloat(inputs[1].value) === 0, `the Point Entry dialog: ${inputs.map((i) => i.value)}`);
    if (inputs.length >= 2) {
      await type(inputs[0], "-13");
      button(pe, "OK")?.click();
      await sleep(800);
      const poly = (await footprint(name)).graphics.find((g2) => g2.id === ids.poly);
      t(poly?.pts.some((p) => p.x === mm(15) && p.y === mm(1)), `moved to (15, 1) mm (${JSON.stringify(poly?.pts)})`);
      await undo();
    }
    // back: the page origin, X increasing right, the grid origin at (0, 0)
    await resetPage("pcb_axes");
    await resetPage("fp_axes");
    await E.run("common.Control.editGridOrigin");
    const g2 = await until(() => dialog("Grid Origin"), 3000);
    if (g2) { await type(g2.querySelector('input[aria-label="X"]'), "0"); await type(g2.querySelector('input[aria-label="Y"]'), "0"); button(g2, "OK")?.click(); await sleep(300); }
    t(prefsStored().pcbDisplayOrigin !== "grid" && !prefsStored().fpInvertXAxis, "put back");
  });

  await scenario("F3-9: View > Flip Board View mirrors the canvas; a click picks what is drawn under it; the check marks follow; R turns the other way", async (t) => {
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
    const r = await E.run("pcbnew.Control.flipBoard");
    t(r?.ok !== false, `the action runs on the Footprint tab (${r?.error ?? "ok"})`);
    isFlipped = true;
    await sleep(400);
    t(document.querySelector('[data-testid="fp-flip-board"]')?.checked === true, "the Appearance panel's Flip board view is checked");
    // a click where the text is drawn (mirrored about the canvas middle) picks it
    await select([]);
    await clickAt(mm(-4), mm(9));
    const sel = E.state().selection.map((x) => x.id ?? x);
    t(sel.includes(ids.text), `the text picked where it is drawn (${JSON.stringify(sel)}; cursor ${JSON.stringify(E.state().cursor ?? null)}; view ${JSON.stringify(E.state().view)}; text ${JSON.stringify((await footprint(name)).texts.find((x) => x.id === ids.text)?.at)})`);
    // R: a quarter turn the other way (`rotateAngle = -rotateAngle`)
    await select([ids.text]);
    await E.run("pcbnew.InteractiveEdit.rotateCcw");
    await sleep(700);
    const txt = (await footprint(name)).texts.find((x) => x.id === ids.text);
    t(txt && (txt.angle === 270_000 || txt.angle === -90_000), `R turned it clockwise in the footprint's frame (${txt?.angle})`);
    await undo();
    await E.run("pcbnew.Control.flipBoard");
    isFlipped = false;
    await sleep(300);
    t(document.querySelector('[data-testid="fp-flip-board"]')?.checked === false, "flipped back");
  });

  await scenario("F3-5: the read-only info bar's Save as editable copy -- cancelled, the modified flag cleared; saved, the copy opens unmodified", async (t) => {
    const ro = "Resistor_SMD:R_0603_1608Metric";
    const TABLE_LIB = `V18Lib${Date.now().toString(36)}`;
    const add = await fetch("/api/library_tables/add", { method: "POST", body: JSON.stringify({ kind: "footprint", path: `${TABLE_LIB}.pretty`, create: true }) }).then((r) => r.json());
    if (!t(!add.error, `a footprint library of the project's table to save into (${add.error ?? TABLE_LIB})`)) return;
    const writable = await json("/api/library/writable?kind=footprint");
    const lib = (writable.libraries ?? []).find((l) => l.name === "Resistor_SMD");
    if (!lib || lib.writable) return t(true, `no read-only Resistor_SMD here (${JSON.stringify(lib)}): skipped`);
    const search = document.querySelector('[data-library-tree-search="footprint"]');
    await type(search, "R_0603_1608Metric");
    const row = await until(() => document.querySelector(`[role=treeitem][data-name="${CSS.escape(ro)}"]`), 8000);
    if (!t(row, "the tree finds the installed footprint")) return;
    row.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    const bar = await until(() => tid("fp-read-only-banner"), 8000);
    t(bar && /Editing footprint from read-only library Resistor_SMD/.test(bar.textContent), `the info bar (${bar?.textContent})`);
    const watch = async (id) => json(`/api/library/watch?kind=footprint&id=${encodeURIComponent(id)}`);
    const fp = await footprint(ro);
    await cmd({ op: "add_footprint_text", footprint: ro, text: { content: "EDITED", at: { x: 0, y: mm(3) }, angle: 0, layer: "F.Fab", size_nm: mm(1), stroke_width: mm(0.15), justify: "center", mirror: false } });
    t((await until(async () => (await watch(ro)).dirty, 4000)) === true, "edited: modified");
    // cancelled
    tid("fp-read-only-save-as")?.click();
    const d = await until(() => dialog("Save Footprint As"), 3000);
    if (!t(d, "Save Footprint As opens")) return;
    button(d, "Cancel")?.click();
    t(await until(async () => (await watch(ro)).dirty === false, 4000), "Cancel: ClearModify() all the same");
    t(!tid("fp-read-only-banner"), "the info bar is dismissed");
    t((await footprint(ro)).texts.some((x) => x.content === "EDITED"), "the copy as it was");
    // saved into the project library: the copy opens, unmodified, and the tree shows it
    await cmd({ op: "forget_library_footprints", names: [ro] });
    await sleep(500);
    const ro2 = "Resistor_SMD:R_0805_2012Metric";
    await type(search, "R_0805_2012Metric");
    const row2 = await until(() => document.querySelector(`[role=treeitem][data-name="${CSS.escape(ro2)}"]`), 8000);
    row2?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await until(() => tid("fp-read-only-banner"), 8000);
    await cmd({ op: "add_footprint_text", footprint: ro2, text: { content: "EDITED", at: { x: 0, y: mm(3) }, angle: 0, layer: "F.Fab", size_nm: mm(1), stroke_width: mm(0.15), justify: "center", mirror: false } });
    tid("fp-read-only-save-as")?.click();
    const d2 = await until(() => dialog("Save Footprint As"), 3000);
    if (!t(d2, "Save Footprint As opens again")) return;
    const copyName = `V18Copy${Date.now().toString(36)}`;
    await type(d2.querySelector('input[aria-label="Name"]'), copyName);
    const libOpt = await until(() => d2.querySelector(`[role=option][data-lib="${CSS.escape(TABLE_LIB)}"]`), 4000);
    t(libOpt, `the library made for the check is listed (${TABLE_LIB})`);
    libOpt?.click();
    await sleep(200);
    button(d2, "Save")?.click();
    const opened = `${TABLE_LIB}:${copyName}`;
    t(await until(async () => (await footprint(opened))?.texts?.some((x) => x.content === "EDITED"), 8000), `the copy, with the edit, is ${opened}`);
    t(await until(async () => (await watch(opened)).dirty === false, 4000), `the editor holds it unmodified (ClearModify): ${JSON.stringify(await watch(opened))}`);
    await type(search, "");
    await sleep(300);
    t(await until(() => document.querySelector(`[role=treeitem][data-name="${CSS.escape(opened)}"]`) ?? [...document.querySelectorAll(".library-tree-group")].find((g) => g.getAttribute("data-library") === TABLE_LIB && /\b1\b/.test(g.textContent)), 6000), "the tree has it (SyncLibraryTree)");
    await cmd({ op: "delete_library_footprint", name: opened });
    await cmd({ op: "forget_library_footprints", names: [ro2] });
    await type(search, "");
    void fp;
  });

  if (name) await cmd({ op: "delete_library_footprint", name });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
