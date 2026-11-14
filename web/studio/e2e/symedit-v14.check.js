// Symbol Editor, UI-LEFT-3 Y3-1 .. Y3-8 (task-v14-symedit; PARITY-symedit "Round 3, task-v14-symedit"): the canvas in the editor's theme; Symbol Properties of a tree symbol
// that is not loaded; a renamed field's looks; the Text Box tool's last line style for the next box and shape; Shape Properties' fill swatch; Bulk Edit's view controls; the Pin
// Table's cross-probe out from behind its dialog; Copy's SVG on the selection's page; KiCad's words and boxes (the info bar, New Symbol's Overwrite); Draw Arc's five modes.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/symedit-v14.check.js 8`. It makes a library of its own (`V14<time>Lib`) in the project's table.
// It answers through `window.__symeditV14Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__symeditV14Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await sleep(250);
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  const E = window.__eda;
  const MM = 1_000_000;
  const G = 1.27;
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (el, value) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(80);
  };
  const until = async (cond, ms = 8000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(600); };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const libRow = (lib) => [...document.querySelectorAll(".library-tree-group")].find((g) => g.textContent.replace(/[▾▸]/g, "").trim().replace(/\s*\*?\s*[\d…]*$/, "").replace(/^☆ /, "") === lib);
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      tid("message-box-ok")?.click();
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close|No)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
      const discard = tid("choice-alt");
      if (discard) { discard.click(); await sleep(250); }
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    await E.run("common.Interactive.cancelInteractive");
    results.push(entry);
  };
  const selectLibrary = async (lib) => {
    const r = await until(() => libRow(lib), 6000);
    if (!r) return false;
    r.click();
    await sleep(250);
    if (!isOpen(lib)) libRow(lib)?.click();
    await sleep(250);
    return true;
  };
  const itemRow = async (id) => {
    const lib = id.split(":")[0];
    if (await until(() => libRow(lib), 6000)) if (!isOpen(lib)) { libRow(lib).click(); await sleep(250); }
    return until(() => treeRow(id), 6000);
  };
  const select = async (ids) => { await E.run("common.Interactive.unselectAll"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };
  // the canvas, in the editor's nm (y down)
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse" }));
  };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(150); };
  const clickAt = async (wx, wy) => { await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(400); };
  const keep = async (id) => (await symbol(id)).kicad ?? {};
  const near = (a, b, tol = 1e-3) => Math.abs(a - b) <= tol;
  const msgBox = () => ({ text: tid("message-box-text")?.textContent ?? null, title: tid("message-box")?.getAttribute("aria-label") ?? null });

  const s = Date.now().toString(36);
  const LIB = `V14${s}Lib`;
  const ID = `${LIB}:Va`;
  const ID2 = `${LIB}:Vb`;

  await scenario("a library of its own and a symbol made with New Symbol", async (t) => {
    await clickTab("Symbol");
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog("New Symbol Library"), 4000);
    if (!t(d, "New Symbol Library opens")) return;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), LIB);
    document.querySelector('[data-library-file-table] input[value="project"]')?.click();
    button(d, "Create").click();
    t(await until(() => !dialog("New Symbol Library"), 6000), `${LIB} made`);
    t(await selectLibrary(LIB), "the library is in the tree");
    await run("eeschema.SymbolLibraryControl.newSymbol");
    if (!t(await until(() => tid("new-symbol"), 3000), "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "Va");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    t(await until(() => E.state().symbolEditor?.libId === ID, 4000), "and it is open");
  });

  await scenario("Y3-1 the canvas is the editor's theme's background (LAYER_SCHEMATIC_BACKGROUND), not the board's dark one", async (t) => {
    const canvas = canvasEl()?.querySelector("canvas");
    if (!t(canvas, "the canvas")) return;
    await sleep(400);
    const px = canvas.getContext("2d").getImageData(3, 3, 1, 1).data;
    t(Math.abs(px[0] - 245) <= 2 && Math.abs(px[1] - 244) <= 2 && Math.abs(px[2] - 239) <= 2, `KiCad Default's #f5f4ef: rgb(${px[0]}, ${px[1]}, ${px[2]})`);
  });

  await scenario("Y3-7 New Symbol's refusals in KiCad's boxes: no name is a message box, a name in use asks Overwrite, and Overwrite replaces the symbol", async (t) => {
    t((await cmd({ op: "add_symbol_pin", lib_id: ID, pin: { number: "9", name: "OLD", electrical_type: "passive", shape: "line", at: { x: 0, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 0, body_style: 0, hidden: false, name_size_mm: null, number_size_mm: null } })).ok !== false, "a pin on the symbol");
    await run("eeschema.SymbolLibraryControl.newSymbol");
    if (!t(await until(() => tid("new-symbol"), 3000), "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "   ");
    tid("new-symbol-ok").click();
    const box = await until(() => msgBox().text, 3000);
    t(/Symbol must have a name\./.test(box ?? "") && msgBox().title === "Message", `wxMessageBox: ${box} (${msgBox().title})`);
    tid("message-box-ok")?.click();
    await sleep(200);
    t(!!tid("new-symbol") && !document.querySelector('[data-testid="new-symbol-error"]'), "the dialog stays, with no error line");
    await type(tid("new-symbol-name"), "Va");
    tid("new-symbol-ok").click();
    const asked = await until(() => dialog("Confirmation"), 3000);
    t(asked && new RegExp(`Symbol 'Va' already exists in library '${LIB}'\\.`).test(asked.textContent) && button(asked, "Overwrite"), `KIDIALOG with Overwrite: ${asked?.textContent}`);
    button(asked, "Overwrite")?.click();
    t(await until(async () => ((await symbol(ID)).pins ?? []).length === 0, 5000), "Overwrite: the symbol is a new, empty one");
    t(await until(() => !tid("new-symbol"), 3000), "the dialog is gone");
  });

  await scenario("Y3-7 a refusal shows in the frame's info bar: Show Datasheet with none (`ShowInfoBarError`)", async (t) => {
    await run("common.Control.showDatasheet");
    t(await until(() => tid("sym-infobar-text")?.textContent === "No datasheet defined.", 3000), `the info bar: ${tid("sym-infobar-text")?.textContent}`);
    t(E.state().symbolEditor?.infoBar === "No datasheet defined.", "the editor's info bar holds it");
  });

  const setup = [
    { op: "add_symbol_pin", lib_id: ID, pin: { number: "1", name: "A", electrical_type: "input", shape: "line", at: { x: 0, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 0, body_style: 0, hidden: false, name_size_mm: null, number_size_mm: null } },
    { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "polyline", unit: 0, body_style: 0, pts: [{ x: -10.16, y: 10.16 }, { x: 0, y: 10.16 }], stroke_mm: 0, fill: "none" } },
  ];

  await scenario("Y3-8 Draw Arc's five modes in the Symbol Editor: Start, End, Midpoint; Tangent from a drawn line and chained; Backspace; Alt+/ cycles the mode", async (t) => {
    for (const c of setup) t((await cmd(c)).ok !== false, `${c.op}`);
    await sleep(600);
    const arcs = async () => (await symbol(ID)).graphics.filter((g) => g.kind === "arc");
    await run("eeschema.InteractiveDrawing.drawArcStartEndMid");
    t(E.state().tool === "draw_arc" && E.state().symbolEditor?.arcDrawMode === "start_end_mid", `the tool in the mode: ${E.state().tool}, ${E.state().symbolEditor?.arcDrawMode}`);
    // (the clicks on the 50 mil grid: G = 1.27 mm)
    await clickAt(8 * G * MM, 0);
    t(E.state().symbolEditor?.drawing === "arc", "the first click starts an arc");
    await clickAt(16 * G * MM, 0);
    await clickAt(12 * G * MM, -4 * G * MM);
    const a1 = await until(async () => (await arcs()).find((g) => (near(g.start.x, 8 * G) && near(g.end.x, 16 * G)) || (near(g.start.x, 16 * G) && near(g.end.x, 8 * G))), 4000);
    t(a1 && near(a1.start.y, 0) && near(a1.end.y, 0) && near(a1.mid.y, 4 * G, 1e-2), `the arc through the three clicks (mm, +y up): ${JSON.stringify(a1)}`);
    t(a1 && (await until(() => E.state().selection.some((x) => (x.id ?? x) === a1.id), 3000)), `left selected: ${JSON.stringify(E.state().selection)}`);
    // Tangent: a click on the drawn line's end starts from it, the next click ends the arc, and the arc after starts where it ended
    await run("eeschema.InteractiveDrawing.drawArcTangent");
    t(E.state().symbolEditor?.arcDrawMode === "tangent", "Tangent mode");
    await clickAt(0, -8 * G * MM);
    t(E.state().symbolEditor?.drawing === "arc", "seeded on the line's end");
    await clickAt(4 * G * MM, -12 * G * MM);
    const a2 = await until(async () => (await arcs()).find((g) => (near(g.start.x, 0) && near(g.start.y, 8 * G)) || (near(g.end.x, 0) && near(g.end.y, 8 * G))), 4000);
    t(a2, `the tangent arc leaves the line's end: ${JSON.stringify((await arcs()).map((g) => [g.start, g.end]))}`);
    t(E.state().symbolEditor?.drawing === "arc", "the next arc waits, seeded at this one's end");
    await E.run("common.Interactive.cancelInteractive");
    document.querySelector(".editor-canvas-col .pcb-canvas-container")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(200);
    // the centre mode: two clicks, Backspace takes the start back; Alt+/ cycles the mode
    await run("eeschema.InteractiveDrawing.drawArcCenter");
    const n = (await arcs()).length;
    await clickAt(-16 * G * MM, 0);
    await clickAt(-12 * G * MM, 0);
    await run("eeschema.InteractiveDrawing.deleteLastPoint");
    await run("eeschema.InteractiveDrawing.deleteLastPoint");
    t(E.state().symbolEditor?.drawing === null, `Backspace twice: nothing left of the arc (${E.state().symbolEditor?.drawing})`);
    await run("common.InteractiveDrawing.cycleArcDrawMode");
    t(E.state().symbolEditor?.arcDrawMode === "start_end_mid", `Alt+/: the next mode (${E.state().symbolEditor?.arcDrawMode})`);
    t((await arcs()).length === n, "nothing drawn by the cancelled arc");
    const checked = document.querySelector('[data-action="eeschema.InteractiveDrawing.drawArcStartEndMid"]');
    if (checked) t(/active|checked|pressed/.test(checked.className + (checked.getAttribute("aria-pressed") ?? "")), `the toolbar's mode button is pressed (${checked.className})`);
    await run("common.InteractiveSelection.selectionTool");
  });

  await scenario("Y3-4 the Text Box tool's border style starts the next box and the next shape (`m_lastStroke`)", async (t) => {
    await run("eeschema.SymbolDrawing.drawSymbolTextBox");
    await clickAt(-30 * MM, -30 * MM);
    await clickAt(-20 * MM, -25 * MM);
    if (!t(await until(() => tid("text-box-properties"), 3000), "Text Box Properties opens")) return;
    await type(tid("tb-text"), "NOTE");
    await type(tid("tb-width"), "0.3");
    await type(tid("tb-border-style"), "dash");
    tid("tb-ok").click();
    t(await until(async () => (await symbol(ID)).graphics.some((g) => g.kind === "text_box" && g.text === "NOTE"), 4000), "the box is in");
    await until(() => !tid("text-box-properties"), 3000);
    await sleep(400);
    t(E.state().tool === "draw_textbox", `the tool stays armed (${E.state().tool})`);
    await clickAt(-30 * MM, -40 * MM);
    await clickAt(-20 * MM, -35 * MM);
    t(await until(() => tid("tb-border-style")?.value === "dash", 3000), `the next box starts dashed (${tid("tb-border-style")?.value})`);
    tid("text-box-properties") && button(tid("text-box-properties"), "Cancel")?.click();
    await sleep(200);
    await run("eeschema.InteractiveDrawing.drawRectangle");
    await clickAt(24 * G * MM, 24 * G * MM);
    await clickAt(32 * G * MM, 32 * G * MM);
    const rect = await until(async () => (await symbol(ID)).graphics.find((g) => g.kind === "rectangle" && near(g.start.x, 24 * G)), 4000);
    t(rect && near(rect.stroke_mm, 0.3), `the rectangle takes the box's width (${rect?.stroke_mm})`);
    t(rect && (await keep(ID)).stroke_types?.[rect.id] === "dash", `and its dashes (${JSON.stringify((await keep(ID)).stroke_types)})`);
    await run("common.InteractiveSelection.selectionTool");
  });

  await scenario("Y3-5 Shape Properties' fill swatch shows the layer colour of the fill chosen", async (t) => {
    const rect = (await symbol(ID)).graphics.find((g) => g.kind === "rectangle" && near(g.start.x, 24 * G));
    if (!t(rect, "the rectangle")) return;
    await cmd({ op: "edit_symbol_graphic", lib_id: ID, id: rect.id, stroke_mm: 0.3, fill: "outline", unit: 0, body_style: 0 });
    await sleep(500);
    await select([rect.id]);
    await run("eeschema.InteractiveEdit.properties");
    if (!t(await until(() => tid("shape-properties"), 3000), "Shape Properties opens")) return;
    const swatch = () => tid("shape-fill-swatch")?.querySelector('input[type="color"]')?.value;
    t(swatch() === "#840000", `the outline fill shows LAYER_DEVICE (${swatch()})`);
    tid("shape-fill-background").click();
    await sleep(150);
    t(swatch() === "#ffffc2", `the background fill shows LAYER_DEVICE_BACKGROUND (${swatch()})`);
  });

  await scenario("Y3-3 a field renamed in Symbol Properties keeps its colour and face", async (t) => {
    const sym = await symbol(ID);
    // (the stored Reference row is left out: its text is the symbol's prefix, which `set_fields` would take from it)
    const fields = (sym.fields ?? []).filter((f) => f.name !== "Reference").concat([{ name: "MPN", text: "X1", at: { x: 0, y: -5 } }]);
    t((await cmd({ op: "edit_library_symbol_fields", lib_id: ID, fields })).ok !== false, "a user field MPN");
    t((await cmd({ op: "set_symbol_field_looks", lib_id: ID, name: "MPN", looks: { color: { r: 10, g: 20, b: 30, a: 255 } } })).ok !== false, "with a colour");
    await sleep(500);
    await run("common.Interactive.unselectAll");
    await run("eeschema.InteractiveEdit.symbolProperties");
    if (!t(await until(() => tid("symbol-properties"), 3000), "Symbol Properties opens")) return;
    const i = [...document.querySelectorAll('[data-testid^="field-name-"]')].findIndex((el) => el.value === "MPN");
    if (!t(i >= 0, "the MPN row")) return;
    await type(document.querySelectorAll('[data-testid^="field-name-"]')[i], "PartNo");
    t(document.querySelectorAll('[data-testid^="field-name-"]')[i].value === "PartNo", "the name is typed");
    tid("props-ok").click();
    await sleep(800);
    t(!msgBox().text, `no refusal (${msgBox().text})`);
    const k = await until(async () => { const kk = await keep(ID); return kk.field_looks?.PartNo ? kk : null; }, 4000);
    t(k && k.field_looks.PartNo.color?.b === 30 && !k.field_looks.MPN, `the looks went with the name: ${JSON.stringify(k?.field_looks)}`);
  });

  await scenario("Y3-2 Symbol Properties of a tree symbol that is not loaded edits it without loading it", async (t) => {
    t((await cmd({ op: "new_symbol", lib_id: ID2 })).ok !== false, `${ID2} made`);
    await sleep(500);
    const row = await itemRow(ID2);
    if (!t(row, "its tree row")) return;
    row.click();
    await sleep(300);
    await run("eeschema.InteractiveEdit.symbolProperties");
    const d = await until(() => tid("symbol-properties"), 3000);
    if (!t(d, "Symbol Properties opens")) return;
    t(d.textContent.includes(ID2), "about the tree's symbol");
    t(E.state().symbolEditor?.libId === ID, `the canvas keeps ${ID} (${E.state().symbolEditor?.libId})`);
    await type(tid("props-keywords"), "kw-v14");
    tid("props-ok").click();
    t(await until(async () => (await symbol(ID2)).keywords === "kw-v14", 4000), "OK writes the tree's symbol");
    t(E.state().symbolEditor?.libId === ID, "still not loaded");
  });

  await scenario("Y3-5 Bulk Edit: Reference and Description start hidden; Show, Group By and Rename Field in the view controls", async (t) => {
    await run("eeschema.SymbolLibraryControl.showLibraryFieldsTable");
    if (!t(await until(() => tid("fields-table") && tid("fields-row-Va"), 5000), "the table opens with its rows")) return;
    const heads = () => [...document.querySelectorAll('[data-testid="fields-table"] th[data-testid^="fields-head-"]')].map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
    t(!heads().includes("Reference") && !heads().includes("Description") && heads().includes("Value"), `UpdateFieldList's shown columns: ${heads().join(", ")}`);
    t(!!tid("fields-view-controls") && !!tid("view-show-Reference"), "the view controls");
    tid("view-show-Reference").click();
    await sleep(150);
    t(heads().includes("Reference"), "Show puts the column in");
    tid("view-group-Footprint").click();
    await sleep(200);
    t(!!document.querySelector('[data-testid^="fields-group-"]'), "Group By the (empty) Footprint gathers the two symbols in a group");
    tid("view-group-Footprint").click();
    await sleep(200);
    if (!t(tid("view-row-PartNo"), "the PartNo row of the view controls")) return;
    tid("view-row-PartNo").click();
    await sleep(100);
    tid("fields-rename-field").click();
    const prompt = await until(() => dialog("Rename Field"), 3000);
    if (!t(prompt, "Rename Field asks the new name")) return;
    await type(prompt.querySelector("input"), "PN");
    button(prompt, "OK")?.click();
    await sleep(300);
    t(heads().includes("PN") && !heads().includes("PartNo"), `the column is renamed: ${heads().join(", ")}`);
    tid("fields-ok").click();
    await sleep(800);
    t(!msgBox().text && !tid("fields-table"), `OK closes the table (${msgBox().text})`);
    const k = await until(async () => { const sym = await symbol(ID); return (sym.fields ?? []).some((f) => f.name === "PN") ? sym : null; }, 5000);
    t(k && !(k.fields ?? []).some((f) => f.name === "PartNo"), "OK renames the field");
    t(k && k.kicad?.field_looks?.PN?.color?.b === 30, `and its looks follow (${JSON.stringify(k?.kicad?.field_looks)})`);
  });

  await scenario("Y3-5 Bulk Edit's grid menu: Create Derived Symbol makes a row of the table without loading it; Clear cell", async (t) => {
    await run("eeschema.SymbolLibraryControl.showLibraryFieldsTable");
    if (!t(await until(() => tid("td-Vb-Value"), 5000), "the table opens")) return;
    tid("td-Vb-Value").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
    const item = (label) => [...document.querySelectorAll('[role="menuitem"]')].find((m) => m.textContent.trim() === label);
    t(await until(() => item("Create Derived Symbol") && item("Clear cell") && item("Revert symbol"), 2000), "the menu has KiCad's entries");
    item("Create Derived Symbol")?.click();
    const d = await until(() => tid("derive-symbol"), 3000);
    if (!t(d, "the Derive dialog opens on the row's symbol")) return;
    button(d, "OK")?.click();
    t(await until(() => tid("fields-row-Vb_1"), 5000), "the derived symbol is a row of the table");
    t(E.state().symbolEditor?.libId === ID, `and is not loaded (${E.state().symbolEditor?.libId})`);
    // Vb's Keywords are "kw-v14" (the Y3-2 scenario's)
    t(tid("cell-Vb-Keywords")?.value === "kw-v14", `Vb's keywords (${tid("cell-Vb-Keywords")?.value})`);
    tid("td-Vb-Keywords").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
    await until(() => item("Clear cell"), 2000);
    item("Clear cell")?.click();
    await sleep(150);
    t(tid("cell-Vb-Keywords")?.value === "", `Clear cell empties it (${tid("cell-Vb-Keywords")?.value})`);
    tid("fields-cancel").click();
    const asked = await until(() => tid("choice-ok") && tid("choice-alt"), 3000);
    t(asked && /Save changes\?/.test(document.querySelector('[role="dialog"][aria-label="Save Changes?"]')?.textContent ?? ""), "Cancel with an edit asks Save changes?");
    tid("choice-alt")?.click();
    await sleep(300);
    t(!tid("fields-table"), "Discard Changes closes the table");
    t((await symbol(`${LIB}:Vb_1`)).extends != null, "the derived symbol stays");
  });

  await scenario("Y3-6 Copy's SVG is on the selection's page", async (t) => {
    const rect = (await symbol(ID)).graphics.find((g) => g.kind === "rectangle" && near(g.start.x, 24 * G));
    if (!t(rect, "the rectangle")) return;
    window.__edaSymClipboardSvg = undefined;
    await select([rect.id]);
    await run("common.Interactive.copy");
    const svg = await until(() => window.__edaSymClipboardSvg, 15000, 250);
    if (!t(svg, `kicad-cli plotted the copy (${svg === null ? "null" : "pending"})`)) return;
    const vb = /viewBox="([^"]+)"/.exec(svg)?.[1]?.split(/\s+/).map(Number);
    const w = /width="([\d.]+)mm"/.exec(svg)?.[1];
    // the rectangle was clicked at 30.48..40.64 mm on both axes of the editor (+y down, as the plot's mm are): the box grown 2% a side and by the pen
    t(vb && vb[0] > 29.5 && vb[0] < 30.5 && vb[1] > 29.5 && vb[1] < 30.5 && vb[2] > 10.16 && vb[2] < 11.5, `viewBox ${vb}`);
    t(w && Number(w) === vb[2], `the page's width is the box's (${w} mm)`);
  });

  await scenario("Y3-5 the Pin Table's cross-probe brings a pin out from behind the dialog (`FocusOnLocation`'s `findDialogs()`)", async (t) => {
    const pin = (await symbol(ID)).pins.find((p) => p.number === "1");
    if (!t(pin, "pin 1")) return;
    await E.run("common.Control.zoomFitScreen");
    await sleep(400);
    await run("eeschema.InteractiveEdit.pinTable");
    const d = await until(() => tid("pin-table"), 4000);
    if (!t(d, "the Pin Table opens")) return;
    await sleep(300);
    const cr = canvasEl().getBoundingClientRect();
    const where = () => {
      const [px, py] = toScreen(pin.at.x * MM, -pin.at.y * MM);
      const r = d.getBoundingClientRect();
      const g = r.width / 10;
      return { px, py, behind: px >= r.left - g && px <= r.right + g && py >= r.top - g && py <= r.bottom + g, under: px >= r.left && px <= r.right && py >= r.top && py <= r.bottom, r };
    };
    const before = where();
    const v0 = JSON.stringify(E.state().view);
    const row = [...document.querySelectorAll('[data-testid="pin-table-row"]')].find((r) => r.querySelector('[aria-label="Number"]')?.value === "1");
    if (!t(row, "the pin's row")) return;
    row.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    t(await until(() => E.state().symbolEditor?.brightened === pin.id, 2000), "the pin is brightened");
    await sleep(300);
    const after = where();
    if (before.behind) {
      t(JSON.stringify(E.state().view) !== v0, "behind the dialog at first: the view moves");
      // `VIEW::SetCenter( aPos, dialogScreenRects )`: the middle of what the deflated free area leaves (kicad-port/pinTable.ts `unobscuredCentre`, unit-tested on its own; a
      // dialog in the middle of the canvas leaves a ring whose box's middle is the canvas's)
      t(Math.abs(after.px - before.px) + Math.abs(after.py - before.py) > 1, `the pin moved: (${Math.round(before.px)}, ${Math.round(before.py)}) -> (${Math.round(after.px)}, ${Math.round(after.py)})`);
    } else {
      t(JSON.stringify(E.state().view) === v0, `clear of the dialog and in view: the view stays (pin at ${Math.round(before.px)}, ${Math.round(before.py)})`);
    }
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
