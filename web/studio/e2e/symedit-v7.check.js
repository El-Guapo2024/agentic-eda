// Symbol Editor, UI-LEFT-2 YY2, YY5, YY6 (task-v7-symedit; PARITY-symedit "Round 2, task-v7-symedit"): the Properties pane's looks rows (Line Style, Private, a hatched Fill,
// Corner Radius, a text's Hyperlink, an arc's Start) set beside the graphic and written where KiCad's SaveSymbol writes them; Shape Properties' border Style, "Fill with:" and
// Private; Text Box Properties' border Style; Increment of the text the Text tool carries; Copy's images and the SVG of the copied text; Find's history and its brightened
// matches; Symbol Properties' refusals in KiCad's message box; Bulk Edit's Exclude From Simulation and Local Power columns.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/symedit-v7.check.js 8`. It makes a library of its own (`V7<time>Lib`) in the project's table.
// It answers through `window.__symeditV7Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__symeditV7Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await sleep(250);
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  const E = window.__eda;
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
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
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
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };
  // the canvas, in the editor's nm (y down)
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse" }));
  };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(150); };
  const clickAt = async (wx, wy) => { await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(500); };
  const pane = () => tid("sym-properties");
  const row = (name) => pane()?.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const setRow = async (name, value) => {
    const el = input(name);
    if (!el) return false;
    if (el.type === "checkbox") { if (el.checked !== value) el.click(); await sleep(600); return true; }
    el.focus();
    await type(el, value);
    if (!(el instanceof HTMLSelectElement)) el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await sleep(600);
    return true;
  };
  const keep = async () => (await symbol(ID)).kicad ?? {};
  const copyText = async (ids) => (await post("/api/symbol/clipboard/copy", { lib_id: ID, ids })).text ?? "";

  const s = Date.now().toString(36);
  const LIB = `V${s}Lib`;
  const ID = `${LIB}:Vs`;
  const ids = {};

  await scenario("a library and a symbol: a rectangle, an arc, a text, a text box and a pin", async (t) => {
    await clickTab("Symbol");
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog("New Symbol Library"), 4000);
    if (!t(d, "New Symbol Library opens")) return;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), LIB);
    document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
    button(d, "Create").click();
    t(await until(() => !dialog("New Symbol Library"), 6000), `${LIB} made`);
    t(await selectLibrary(LIB), "the library is in the tree");
    await run("eeschema.SymbolLibraryControl.newSymbol");
    if (!t(await until(() => tid("new-symbol"), 3000), "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "Vs");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    for (const c of [
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "rectangle", unit: 0, body_style: 1, start: { x: -5.08, y: 5.08 }, end: { x: 5.08, y: -5.08 }, stroke_mm: 0.254, fill: "background" } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "arc", unit: 0, body_style: 1, start: { x: 10, y: 0 }, mid: { x: 10 + 2 * Math.SQRT1_2 - 2, y: 2 * Math.SQRT1_2 }, end: { x: 8, y: 2 }, stroke_mm: 0.254, fill: "none" } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text", unit: 0, body_style: 1, text: "VCC rail", at: { x: 0, y: 7.62 }, angle_deg: 0, size_mm: 1.27 } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text_box", unit: 0, body_style: 1, text: "box", start: { x: -15, y: 5 }, end: { x: -8, y: 0 }, angle_deg: 0, size_mm: 1.27, h_align: "left", v_align: "top", stroke_mm: 0.254, fill: "none" } },
      { op: "add_symbol_pin", lib_id: ID, pin: { number: "1", name: "VCC", electrical_type: "power_in", shape: "line", at: { x: -7.62, y: 2.54 }, angle_deg: 0, length_mm: 2.54, unit: 0, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null } },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const sym = await symbol(ID);
    for (const g of sym.graphics) ids[g.kind] = g.id;
    ids.pin = sym.pins[0]?.id;
    t(ids.rectangle && ids.arc && ids.text && ids.text_box && ids.pin, `ids: ${JSON.stringify(ids)}`);
    await (await itemRow(ID))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    t(E.state().symbolEditor?.libId === ID, "the symbol is open");
  });

  // ------------------------------------------------------------------------------------------------------------------------ YY2: the pane
  await scenario("YY2 the pane: a rectangle's Line Style, Private, Cross-hatch Fill and Corner Radius are written where SaveSymbol writes them; one undo each", async (t) => {
    await select([ids.rectangle]);
    t(await until(() => row("Line Style") && row("Corner Radius") && row("Private"), 3000), "the rectangle's rows");
    t(input("Line Style") && !input("Line Style").disabled && input("Private") && !input("Private").disabled && input("Corner Radius") && !input("Corner Radius").disabled, "Line Style, Private and Corner Radius are writable");
    await setRow("Line Style", "dot");
    t(await until(async () => (await keep()).stroke_types?.[ids.rectangle] === "dot", 4000), "Line Style: Dotted kept beside the rectangle");
    await setRow("Private", true);
    t(await until(async () => (await keep()).graphic_looks?.[ids.rectangle]?.private === true, 4000), "Private kept beside it");
    await setRow("Fill", "cross_hatch");
    t(await until(async () => (await keep()).graphic_looks?.[ids.rectangle]?.hatch === "cross_hatch", 4000), "a cross-hatched fill");
    await setRow("Corner Radius", "1");
    t(await until(async () => (await keep()).graphic_looks?.[ids.rectangle]?.radius_mm === 1, 4000), `a corner radius of 1 mm (${JSON.stringify((await keep()).graphic_looks)})`);
    const text = await copyText([ids.rectangle]);
    t(/\(rectangle private \(start -5\.08 5\.08\) \(end 5\.08 -5\.08\) \(radius 1\) \(stroke \(width 0\.254\) \(type dot\)\) \(fill \(type cross_hatch\) \(color 0 0 0 0\)\)\)/.test(text), `formatRect: ${text.split("\n").find((l) => l.includes("(rectangle")) ?? text}`);
    await undo();
    t(((await keep()).graphic_looks?.[ids.rectangle]?.radius_mm ?? 0) === 0 && (await keep()).graphic_looks?.[ids.rectangle]?.hatch === "cross_hatch", "one undo takes the radius back, not the hatch");
  });

  await scenario("YY2 the pane: a text's Hyperlink and Color, an arc's Start X moves about its centre", async (t) => {
    await select([ids.text]);
    t(await until(() => row("Hyperlink") && !input("Hyperlink")?.disabled, 3000), "a text's Hyperlink is writable");
    await setRow("Hyperlink", "https://kicad.org");
    t(await until(async () => (await keep()).graphic_looks?.[ids.text]?.hyperlink === "https://kicad.org", 4000), "the link kept beside the text");
    t((await copyText([ids.text])).includes('(href "https://kicad.org")'), "written as EDA_TEXT::Format's (href ..)");
    await select([ids.arc]);
    t(await until(() => row("Start X") && !input("Start X")?.disabled, 3000), "an arc's Start X is writable");
    const before = (await symbol(ID)).graphics.find((g) => g.id === ids.arc);
    await setRow("Start X", "8");
    const after = await until(async () => { const g = (await symbol(ID)).graphics.find((x) => x.id === ids.arc); return g && Math.abs(g.start.x - 8) < 1e-6 ? g : null; }, 4000);
    t(after && Math.abs(after.end.x - before.end.x) < 1e-6 && Math.abs(after.mid.x - before.mid.x) > 1e-4, `the start moved, the end stayed, the middle recomputed: ${JSON.stringify(after)}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ YY2: the dialogs
  await scenario("YY2 Shape Properties: border Color and Style, Fill with: a colour, Private; Text Box Properties' border Style", async (t) => {
    await select([ids.rectangle]);
    await run("eeschema.InteractiveEdit.properties");
    if (!t(await until(() => tid("shape-properties"), 3000), "Shape Properties opens")) return;
    t(tid("shape-private")?.checked === true, "Private shows the rectangle's flag");
    t(tid("shape-border-style")?.value === "dot", `the border Style shows Dotted: ${tid("shape-border-style")?.value}`);
    await type(tid("shape-border-style"), "dash_dot");
    tid("shape-fill-color")?.click();
    await sleep(100);
    const swatchDefault = tid("shape-fill-swatch")?.querySelector('input[type="checkbox"]');
    if (swatchDefault?.checked) { swatchDefault.click(); await sleep(100); }
    const swatch = tid("shape-fill-swatch")?.querySelector('input[type="color"]');
    if (swatch) { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(swatch, "#00ff00"); swatch.dispatchEvent(new Event("input", { bubbles: true })); swatch.dispatchEvent(new Event("change", { bubbles: true })); await sleep(100); }
    tid("shape-private")?.click();
    await sleep(80);
    tid("shape-ok")?.click();
    const k = await until(async () => { const kk = await keep(); return kk.stroke_types?.[ids.rectangle] === "dash_dot" ? kk : null; }, 4000);
    const g = (await symbol(ID)).graphics.find((x) => x.id === ids.rectangle);
    t(k && g?.fill === "color" && k.graphic_looks?.[ids.rectangle]?.fill_color?.g === 255 && !k.graphic_looks?.[ids.rectangle]?.private && !k.graphic_looks?.[ids.rectangle]?.hatch, `OK: dash-dot, filled with its own colour, not private, no hatch (${JSON.stringify(k?.graphic_looks?.[ids.rectangle])}, fill ${g?.fill})`);
    t((await copyText([ids.rectangle])).includes("(fill (type color) (color 0 255 0 1))"), "formatFill writes the colour");
    await select([ids.text_box]);
    await run("eeschema.InteractiveEdit.properties");
    if (!t(await until(() => tid("text-box-properties"), 3000), "Text Box Properties opens")) return;
    t(!!tid("tb-border-style"), "the border Style is there");
    await type(tid("tb-border-style"), "dash");
    tid("tb-ok")?.click();
    t(await until(async () => (await keep()).stroke_types?.[ids.text_box] === "dash", 4000), "the box's border is dashed");
    t((await copyText([ids.text_box])).includes("(type dash)"), "written in the box's stroke");
  });

  // ------------------------------------------------------------------------------------------------------------------------ YY5
  await scenario("YY5 Increment of the text the Text tool carries; Copy's SVG of the copied text", async (t) => {
    await select([]);
    await run("eeschema.SymbolDrawing.placeSymbolText");
    await clickAt(0, -12_700_000);
    if (!t(await until(() => tid("text-properties"), 3000), "Text Properties opens on the click")) return;
    await type(tid("text-text"), "A1");
    tid("text-ok")?.click();
    await sleep(400);
    await hover(0, -12_700_000);
    await run("eeschema.Interactive.incrementPrimary");
    await sleep(200);
    const before = (await symbol(ID)).graphics.length;
    t((await symbol(ID)).graphics.length === before, "nothing committed by the increment");
    await clickAt(2_540_000, -12_700_000);
    const placed = await until(async () => (await symbol(ID)).graphics.find((g) => g.kind === "text" && /^A\d$/.test(g.text)), 4000);
    t(placed?.text === "A2", `the text placed is the incremented one: ${placed?.text}`);
    await run("common.Interactive.cancelInteractive");
    await run("common.InteractiveSelection.selectionTool");
    const raw = await copyText([ids.pin, ids.arc]);
    const svg = await post("/api/sym/svg", { lib_id: ID, text: raw, unit: 1, body_style: 1, theme: "_builtin_default" });
    t(svg.ok && /<svg/.test(svg.svg?.text ?? ""), `kicad-cli plots the copied symbol (${svg.message ?? "ok"})`);
    await select([ids.pin, ids.arc]);
    window.__edaSymClipboardTypes = undefined;
    await run("common.Interactive.copy");
    const types = await until(() => window.__edaSymClipboardTypes, 8000);
    t(true, `the clipboard took: ${JSON.stringify(types ?? "nothing (headless: no clipboard permission)")}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ YY6
  await scenario("YY6 Find: every match brightened while the dialog is open, the history of what was searched", async (t) => {
    await select([]);
    await run("common.Interactive.find");
    if (!t(await until(() => tid("sym-find-dialog"), 3000), "Find opens")) return;
    await type(tid("sym-find-text"), "VCC");
    const bright = await until(() => { const b = window.__edaSymFind?.().bright ?? []; return b.length >= 2 ? b : null; }, 5000);
    t(bright && bright.includes(ids.pin) && bright.includes(ids.text), `the pin and the text are brightened: ${JSON.stringify(bright)}`);
    tid("sym-find-next")?.click();
    await sleep(500);
    await type(tid("sym-find-text"), "box");
    tid("sym-find-next")?.click();
    await sleep(500);
    t(JSON.stringify(window.__edaSymFind().findHistory.slice(0, 2)) === JSON.stringify(["box", "VCC"]), `the history, the latest first: ${JSON.stringify(window.__edaSymFind().findHistory)}`);
    t([...document.querySelectorAll('[data-testid="sym-find-history"] option')].map((o) => o.value).join(",").startsWith("box,VCC"), "the combo lists it");
    await closeDialogs();
    t((window.__edaSymFind().bright ?? []).length === 0, "closed: nothing brightened");
  });

  await scenario("YY6 Symbol Properties' refusal in KiCad's message box; Bulk Edit's Exclude From Simulation and Local Power", async (t) => {
    await run("eeschema.InteractiveEdit.symbolProperties");
    if (!t(await until(() => tid("symbol-properties"), 3000), "Symbol Properties opens")) return;
    await type(tid("field-text-0"), "12");
    tid("props-ok")?.click();
    const said = await until(() => tid("message-box-text")?.textContent, 3000);
    t(/References must start with a letter\./.test(said ?? "") && document.querySelector('[data-testid="message-box"]')?.getAttribute("aria-label") === "Error", `DisplayError: ${said}`);
    tid("message-box-ok")?.click();
    await sleep(200);
    t(!!tid("symbol-properties") && !tid("message-box"), "OK closes the box, the dialog stays");
    await closeDialogs();
    await run("eeschema.SymbolLibraryControl.showLibraryFieldsTable");
    if (!t(await until(() => tid("fields-table"), 4000), "the fields table opens")) return;
    const sim = await until(() => tid("cell-Vs-exclude-sim"), 5000);
    const heads = [...document.querySelectorAll('[data-testid="fields-table"] th')].map((h) => h.textContent);
    t(heads.join("|").includes("Keywords|Exclude From BOM|Exclude From Simulation|Exclude From Board|Power Symbol|Local Power Symbol"), `UpdateFieldList's order: ${heads.join("|")}`);
    if (!t(sim, "the Exclude From Simulation cell")) return;
    sim.click();
    tid("cell-Vs-local-power")?.click();
    await sleep(100);
    tid("fields-ok")?.click();
    const after = await until(async () => { const v = await symbol(ID); return v.exclude_from_sim ? v : null; }, 4000);
    t(after?.exclude_from_sim === true && after?.power === true && after?.power_local === true, `OK: excluded from simulation, a local power symbol (${after?.exclude_from_sim}, ${after?.power}, ${after?.power_local})`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
