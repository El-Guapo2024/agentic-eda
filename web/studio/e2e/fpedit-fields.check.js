// The Footprint Editor's fields and the tools that were dimmed (UI-LEFT F1 and F2; PARITY-fpedit.md section 20): the Reference, the Value and the user fields are drawn
// and picked on the canvas, a drag moves the Reference, its double click opens "Footprint Reference Properties" (`DIALOG_TEXT_PROPERTIES` on a `PCB_FIELD`) and OK edits it;
// a double click on a graphic opens its Shape Properties (`DIALOG_SHAPE_PROPERTIES`); R turns any item, Mirror Horizontally / Vertically mirror the selection on its layers
// (`EDIT_TOOL::Mirror`), Group and Ungroup make and break a group (`PCB_GROUP_TOOL`) and a click on a member selects the group; the 45-degree line mode bends a line; a dimension
// and a text box are drawn with their tools; every edit is one undo step of the Footprint Editor; the footprint saved into a library of the tables (Save As) and opened again
// from its file keeps its fields, its groups, its dimension and its text box (that kicad-cli loads such a file, `fp upgrade` keeps every item and `fp export svg`
// plots it, is the slow test `kicad_cli_loads_and_keeps_every_item_our_writer_writes` of `crates/kicad/tests/fp_items_io.rs`).
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes footprints and a library of its own (nickname `FeLib`),
// deletes the footprints of the project library it made and takes the library's row out of the project table at the end. It answers through `window.__fpeditFieldsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__fpeditFieldsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(120); };
  const until = async (fn, ms = 4000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(100); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => { const f = await json(`/api/footprint?name=${encodeURIComponent(name)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [], pads: f.pads ?? [] }; };
  const dialog = (label) => document.querySelector(`[role="dialog"][aria-label="${label}"]`);
  const button = (root, label) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** `mods`: Ctrl+Shift by default -- neither the grid nor an anchor moves the point (a click lands where it is sent); `{ shift: true }` alone keeps the grid. */
  const fire = (type, wx, wy, buttons = 0, mods = { ctrl: true, shift: true }, detail = 1) => {
    const [x, y] = toScreen(wx, wy);
    const mac = /Mac/.test(navigator.platform);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !mac && !!mods.ctrl, metaKey: mac && !!mods.ctrl, shiftKey: !!mods.shift, detail }));
  };
  const clickAt = async (wx, wy, mods) => { fire("pointermove", wx, wy, 0, mods); await sleep(70); fire("pointerdown", wx, wy, 1, mods); await sleep(40); fire("pointerup", wx, wy, 0, mods); await sleep(400); };
  /** A plain click of the select tool (no modifier: Shift would add to the selection). */
  const selectAt = async (wx, wy, mods = {}) => clickAt(wx, wy, mods);
  const dblClickAt = async (wx, wy) => {
    const [x, y] = toScreen(wx, wy);
    await selectAt(wx, wy);
    canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, detail: 2 }));
    await sleep(400);
  };
  /** A drag of the select tool from one point to another (past the 8 px drag threshold). */
  const dragFrom = async (wx, wy, tx, ty) => {
    fire("pointermove", wx, wy, 0, {});
    await sleep(60);
    fire("pointerdown", wx, wy, 1, {});
    await sleep(60);
    for (let i = 1; i <= 6; i++) {
      fire("pointermove", wx + ((tx - wx) * i) / 6, wy + ((ty - wy) * i) / 6, 1, {});
      await sleep(60);
    }
    fire("pointerup", tx, ty, 0, {});
    await sleep(600);
  };
  const key = async (k, opts = {}) => { canvasEl().focus(); canvasEl().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...opts })); await sleep(250); };
  const arm = async (action, tool) => { if (__eda.state().tool !== tool) { await __eda.run(action); await sleep(250); } };
  const selection = () => (__eda.state().selection ?? []).map((s) => s.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(700); };
  const made = [];
  const newFootprint = async () => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !before.includes(n));
    made.push(name);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(400);
    return name;
  };
  const near = (a, b, tol = 1) => Math.abs(a - b) <= tol;
  // the library tree and the table dialogs (as e2e/library-save.check.js drives them)
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  // the library's row by the name it carries: its text ends in the count (and " *" when modified), and a name that ends in a digit -- the base-36 time in these
  // checks' names does, 10 times in 36 -- read as part of the count once the digits were stripped from the text
  const libRow = (lib) => document.querySelector(`.library-tree-group[data-library="${CSS.escape(lib)}"]`);
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  const itemRow = async (id) => {
    const lib = id.split(":")[0];
    if (await until(() => libRow(lib), 6000)) if (!isOpen(lib)) { libRow(lib).click(); await sleep(250); }
    return until(() => treeRow(id), 6000);
  };
  const selectItem = async (id) => { const r = await itemRow(id); r?.click(); await sleep(300); return !!r; };
  const savedFootprint = async (id) => (await json(`/api/library/saved?kind=footprint&id=${encodeURIComponent(id)}`)).footprint;
  const tables = () => json("/api/library_tables");
  const LIB = `FeLib${Date.now().toString(36)}`;

  // --------------------------------------------------------------------------------------------------------------------------- run
  const libraryBefore = await fpNames();
  let name = null;

  await scenario("F1: the Reference is drawn and picked; a drag moves it, one undo step", async (t) => {
    name = await newFootprint();
    if (!t(!!name, `a new footprint: ${name}`)) return;
    // `CreateNewFootprint`: REF** on the silkscreen half a text height above the origin, the Value on the fabrication layer at (0, 1 mm)
    await selectAt(0, -500_000);
    t(selection().join() === "field:Reference", `a click on REF** selects the Reference: ${JSON.stringify(__eda.state().selection)}`);
    await selectAt(0, 1_000_000);
    t(selection().join() === "field:Value", `a click on the Value selects it: ${selection()}`);
    // Left button drag's default is "Drag selected objects" (`MOUSE_DRAG_ACTION::DRAG_SELECTED`, UI-LEFT C1): a drag from an item not selected draws the selection
    // rectangle, so the Reference is selected first
    await selectAt(0, -500_000);
    await dragFrom(0, -500_000, 0, -2_500_000);
    const fp = await footprint(name);
    t(fp.reference && near(fp.reference.at.y, -2_500_000, 300_000) && fp.reference.layer === "F.SilkS", `the Reference moved: ${JSON.stringify(fp.reference?.at)}`);
    await undo();
    const back = await footprint(name);
    t(!back.reference || back.reference.at.y === -500_000, `one undo puts it back: ${JSON.stringify(back.reference?.at)}`);
    await redo();
    t((await footprint(name)).reference?.at.y !== -500_000, "redo moves it again");
  });

  await scenario("F1: a double click on the Reference opens Footprint Reference Properties; OK edits it, one undo step; the Value's and E", async (t) => {
    if (!name) return t(false, "no footprint");
    const at = (await footprint(name)).reference?.at ?? { x: 0, y: -500_000 };
    await dblClickAt(at.x, at.y);
    const d = await until(() => dialog("Footprint Reference Properties"), 3000);
    if (!t(d, "the dialog has KiCad's title")) return;
    t(d.textContent.includes("Reference:") && d.textContent.includes("Show") && d.textContent.includes("Keep upright"), "its labels: Reference:, Show, Keep upright");
    await type(tid("fp-text-edit-content"), "U**");
    await choose(tid("fp-text-edit-layer"), "F.Fab");
    await type(tid("fp-text-edit-height"), "1.5");
    await type(tid("fp-text-edit-angle"), "90");
    tid("fp-text-edit-ok").click();
    t(await until(() => !dialog("Footprint Reference Properties"), 3000), "OK closes it");
    const r = (await footprint(name)).reference;
    t(r && r.text === "U**" && r.layer === "F.Fab" && r.size[1] === 1_500_000 && r.angle === 90_000, `the Reference is edited: ${JSON.stringify(r && [r.text, r.layer, r.size, r.angle])}`);
    await undo();
    const u = (await footprint(name)).reference;
    t(u && u.text !== "U**", `one undo takes the edit back: ${u?.text}`);
    await redo();
    // the Value: E on it
    await selectAt(0, 1_000_000);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    const v = await until(() => dialog("Footprint Value Properties"), 3000);
    t(v && v.textContent.includes("Value:"), "E on the Value opens Footprint Value Properties");
    await closeDialogs();
  });

  let line = null;
  await scenario("F1: a double click on a graphic opens its Shape Properties; Validate's words; OK edits it", async (t) => {
    if (!name) return t(false, "no footprint");
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    // below the `${REFERENCE}` text CreateNewFootprint puts at (0, 2.5 mm)
    await clickAt(-3_000_000, 5_000_000);
    await clickAt(3_000_000, 5_000_000);
    await key("Escape");
    await key("Escape");
    const fp = await footprint(name);
    line = fp.graphics.find((g) => g.kind === "segment");
    if (!t(line, "a line is drawn")) return;
    await dblClickAt(0, 5_000_000);
    const d = await until(() => dialog("Segment Properties"), 3000);
    if (!t(d, `"Segment Properties" opens (EDA_SHAPE::getFriendlyName)`)) return;
    await type(tid("fp-shape-width"), "0");
    tid("fp-shape-ok").click();
    await sleep(300);
    t(tid("fp-shape-errors")?.textContent.includes("Line width must be greater than zero."), `a line of no width is refused: ${tid("fp-shape-errors")?.textContent}`);
    await type(tid("fp-shape-width"), "0.25");
    await type(tid("fp-shape-end-y"), "6");
    tid("fp-shape-ok").click();
    t(await until(() => !dialog("Segment Properties"), 3000), "OK closes it");
    const g = (await footprint(name)).graphics.find((x) => x.id === line.id);
    t(g && g.stroke_width === 250_000 && g.end.y === 6_000_000, `the line is edited: ${JSON.stringify(g && [g.stroke_width, g.end])}`);
    line = g;
  });

  await scenario("F2: R turns a graphic about its position; Mirror Horizontally mirrors on its layer; one undo step each", async (t) => {
    if (!name || !line) return t(false, "no line");
    await selectAt(0, (line.start.y + line.end.y) / 2);
    t(selection().join() === line.id, `the line is selected: ${selection()}`);
    await __eda.run("pcbnew.InteractiveEdit.rotateCcw");
    await sleep(600);
    let g = (await footprint(name)).graphics.find((x) => x.id === line.id);
    // about its start (-3, 5): the end (3, 6) goes to (-2, -1) -- `RotatePoint` counter-clockwise on a y-down screen
    t(g && g.start.x === -3_000_000 && g.start.y === 5_000_000 && g.end.x === -2_000_000 && g.end.y === -1_000_000, `turned: ${JSON.stringify(g && [g.start, g.end])}`);
    await undo();
    g = (await footprint(name)).graphics.find((x) => x.id === line.id);
    t(g && g.end.x === 3_000_000, "undo turns it back");
    // Mirror the line and the Value about the centre of the line's box (the Value is a text: left out of the box)
    await selectAt(0, (line.start.y + line.end.y) / 2);
    await selectAt(0, 1_000_000, { shift: true });
    t(selection().length === 2, `the line and the Value: ${selection()}`);
    await __eda.run("pcbnew.InteractiveEdit.mirrorHoriontally");
    await sleep(600);
    const fp = await footprint(name);
    g = fp.graphics.find((x) => x.id === line.id);
    t(g && g.start.x === 3_000_000 && g.end.x === -3_000_000 && g.layer === line.layer, `the line mirrors left-right on ${g?.layer}: ${JSON.stringify(g && [g.start, g.end])}`);
    t(fp.value && fp.value.at.x === 0 && fp.value.layer === "F.Fab", `the Value mirrors about x = 0 and keeps its layer: ${JSON.stringify(fp.value && [fp.value.at, fp.value.layer])}`);
    await undo();
    g = (await footprint(name)).graphics.find((x) => x.id === line.id);
    t(g && g.start.x === -3_000_000, "one undo takes the mirror back");
  });

  await scenario("F2: Group makes a group of the selection; a click on a member selects it, a drag moves it; Group Properties; Ungroup", async (t) => {
    if (!name || !line) return t(false, "no line");
    await __eda.run("common.Interactive.unselectAll").catch(() => {});
    await selectAt(0, (line.start.y + line.end.y) / 2);
    await selectAt(0, 1_000_000, { shift: true });
    await __eda.run("common.Interactive.group");
    await sleep(700);
    let fp = await footprint(name);
    const g = (fp.items?.groups ?? [])[0];
    t(g && g.member_ids.includes(line.id) && g.member_ids.includes("field:Value"), `a group of the line and the Value: ${JSON.stringify(g)}`);
    t(selection().join() === g?.id, `the new group is selected: ${selection()}`);
    await __eda.run("common.Interactive.unselectAll").catch(() => {});
    await selectAt(0, 1_000_000);
    t(selection().join() === g?.id, `a click on the Value selects its group: ${selection()}`);
    await dragFrom(0, 1_000_000, 1_000_000, 1_000_000);
    fp = await footprint(name);
    const moved = fp.graphics.find((x) => x.id === line.id);
    t(moved && near(moved.start.x, line.start.x + 1_000_000, 300_000) && fp.value && near(fp.value.at.x, 1_000_000, 300_000), `the drag moves both: ${JSON.stringify([moved?.start, fp.value?.at])}`);
    // E on the group: Group Properties (`case PCB_GROUP_T: groupProperties`), its name one undo step
    await __eda.run("pcbnew.InteractiveEdit.properties");
    const gd = await until(() => dialog("Group Properties"), 3000);
    if (t(gd, "E on the group opens Group Properties")) {
      await type(tid("fp-group-name"), "body");
      tid("fp-group-ok").click();
      t(await until(async () => ((await footprint(name)).items?.groups ?? [])[0]?.name === "body", 3000), "OK names the group");
    }
    await __eda.run("common.Interactive.ungroup");
    await sleep(600);
    t(((await footprint(name)).items?.groups ?? []).length === 0, "Ungroup dissolves it");
    await undo();
    t(((await footprint(name)).items?.groups ?? []).length === 1, "undo brings the group back");
  });

  await scenario("F2: the 45-degree line mode bends a line, the free mode does not", async (t) => {
    if (!name) return t(false, "no footprint");
    const before = (await footprint(name)).graphics.length;
    await __eda.run("pcbnew.EditorControl.lineMode45");
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    await clickAt(-2_000_000, -4_000_000, { shift: true });
    await clickAt(2_000_000, -3_500_000, { shift: true });
    await key("Escape");
    await key("Escape");
    let gs = (await footprint(name)).graphics.slice(before);
    t(gs.length === 1 && gs[0].start.y === gs[0].end.y, `in 45-degree mode a nearly horizontal line is horizontal: ${JSON.stringify(gs.map((g) => [g.start, g.end]))}`);
    await __eda.run("pcbnew.EditorControl.lineModeFree");
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    await clickAt(-2_000_000, -5_000_000, { shift: true });
    await clickAt(2_000_000, -4_500_000, { shift: true });
    await key("Escape");
    await key("Escape");
    gs = (await footprint(name)).graphics.slice(before);
    t(gs.length === 2 && gs[1].end.y === -4_500_000, `in the free mode the line goes where it is clicked: ${JSON.stringify(gs.map((g) => [g.start, g.end]))}`);
    await __eda.run("pcbnew.EditorControl.lineMode45");
  });

  await scenario("F2: a dimension and a text box drawn with their tools, each one undo step", async (t) => {
    if (!name) return t(false, "no footprint");
    const MM = 1_000_000;
    const items = async (kind) => ((await footprint(name)).items?.[kind] ?? []);
    // Draw Aligned Dimension: origin, end, then the height
    await arm("pcbnew.InteractiveDrawing.alignedDimension", "dim_aligned");
    await clickAt(-4 * MM, -8 * MM);
    await clickAt(4 * MM, -8 * MM);
    await clickAt(0, -10 * MM);
    const dims = await until(async () => { const d = await items("dimensions"); return d.length > 0 ? d : null; });
    t(dims && dims[0].kind?.kind === "aligned" && dims[0].start.x === -4 * MM && dims[0].end.x === 4 * MM, `an aligned dimension: ${JSON.stringify(dims && [dims[0].kind, dims[0].start, dims[0].end])}`);
    await key("Escape");
    await key("Escape");
    // Draw Text Box: two corners, then Text Box Properties
    await arm("pcbnew.InteractiveDrawing.textbox", "textbox");
    await clickAt(2 * MM, 10 * MM, { shift: true });
    await clickAt(10 * MM, 13 * MM, { shift: true });
    const dlg = await until(() => tid("pcb-text-box-properties"));
    if (t(dlg, "Text Box Properties opens on the box")) {
      const el = tid("ptb-text");
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, "SAVED");
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(100);
      tid("ptb-ok").click();
    }
    const boxes = await until(async () => { const b = await items("text_boxes"); return b.length > 0 ? b : null; });
    t(boxes && boxes[0].text === "SAVED", `the text box: ${JSON.stringify(boxes && [boxes[0].text, boxes[0].start, boxes[0].end])}`);
    await key("Escape");
    await key("Escape");
    await undo();
    t((await items("text_boxes")).length === 0, "one undo takes the text box away");
    await redo();
    t((await items("text_boxes")).length === 1, "redo brings it back");
  });

  await scenario("the footprint saved into a library of the tables and opened again from its file keeps its fields, its group and its items", async (t) => {
    if (!name) return t(false, "no footprint");
    const before = await footprint(name);
    await __eda.run("common.Control.newLibrary");
    const nd = await until(() => dialog("New Footprint Library"), 4000);
    if (!t(nd, "New Footprint Library opens")) return;
    await until(() => nd.querySelector("#lt-folder")?.value, 4000);
    await type(nd.querySelector("#lt-name"), LIB);
    document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
    button(nd, "Create").click();
    if (!t(await until(() => !dialog("New Footprint Library"), 6000), `${LIB} made`)) return;
    await selectItem(name);
    await __eda.run("common.Control.saveAs");
    const d = await until(() => dialog("Save Footprint As"), 3000);
    if (!t(d, "Save Footprint As opens")) return;
    await until(() => d.querySelector(`[data-lib="${LIB}"]`), 4000);
    await type(d.querySelector('[aria-label="Name"]'), "FeFp1");
    d.querySelector(`[data-lib="${LIB}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    const id = `${LIB}:FeFp1`;
    // Save As is over when its dialog is: after the file it opens the copy (`SetFPID`) and closes. Forgetting the design's copy before that let the dialog's
    // own open land after the delete, which (before the open verb read the library itself) started a blank footprint under the name.
    t(await until(() => !dialog("Save Footprint As"), 8000), "Save As closes");
    const saved = await until(async () => savedFootprint(id), 6000);
    if (!t(saved, `Save As wrote ${id} into the library's folder`)) return;
    // forget the design's copy, open it from the tree: it comes from the .kicad_mod
    await cmd({ op: "delete_library_footprint", name: id });
    await until(async () => !(await fpNames()).includes(id), 4000);
    const r = await itemRow(id);
    if (!t(r, "the tree lists it from the library's folder")) return;
    r.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    // (the editor copies the library's footprint into the design, then opens it: wait for its content, not for the first answer)
    const back = await until(async () => { const v = await footprint(id); return v && !v.error && v.graphics.length > 0 ? v : null; }, 12000);
    if (!back) {
      const v = await footprint(id);
      const fpState = __eda.state();
      return t(false, `reopened from the file: ${JSON.stringify({ error: v.error, graphics: v.graphics.length, pads: v.pads.length, tab: fpState.tab, lists: Object.fromEntries(Object.entries(fpState.lists ?? {}).map(([k, l]) => [k, l.length])) })}; names ${JSON.stringify((await fpNames()).filter((n) => n.startsWith("FeLib")))}`);
    }
    t(true, "reopened from the file");
    t(back.reference?.text === before.reference?.text && back.reference?.layer === before.reference?.layer && back.reference?.angle === before.reference?.angle, `the Reference as it was edited: ${JSON.stringify(back.reference && [back.reference.text, back.reference.layer, back.reference.angle])}`);
    const groups = back.items?.groups ?? [];
    t(groups.length === (before.items?.groups ?? []).length && groups.every((g) => g.member_ids.length === 2), `the groups and their members: ${JSON.stringify(groups.map((g) => g.member_ids))}`);
    t(groups.some((g) => g.member_ids.includes("field:Value")), "a field is still a member of its group");
    t(back.graphics.length === before.graphics.length, `the graphics: ${back.graphics.length} of ${before.graphics.length}`);
    for (const kind of ["dimensions", "text_boxes"]) t((before.items?.[kind] ?? []).length > 0 && (back.items?.[kind] ?? []).length === (before.items?.[kind] ?? []).length, `the ${kind}: ${(back.items?.[kind] ?? []).length} of ${(before.items?.[kind] ?? []).length}`);
    t(back.items?.text_boxes?.[0]?.text === "SAVED", `the text box's text: ${back.items?.text_boxes?.[0]?.text}`);
    made.push(id);
  });

  // ------------------------------------------------------------------------------------------------------------------------ the end
  for (const n of await fpNames()) if (!libraryBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
  // the check's library out of the project table (its folder stays in the board's directory)
  const v = await tables();
  if (v?.tables?.footprint) await post("/api/library_tables", { kind: "footprint", rows: v.tables.footprint.project.rows.filter((r) => !r.nickname.startsWith("FeLib")) });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
