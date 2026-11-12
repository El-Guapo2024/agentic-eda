// Symbol Editor, UI-LEFT Y4 (task-u6-last; PARITY-symedit.md "Symbol Properties' units, body styles and pin connections; a text box's colours, font and Private flag"):
// Symbol Properties' "Unit display names (optional):" grid, "Custom body styles:" with its grid, and the Pin Connections tab (KiCad 25ec2335
// `DIALOG_LIB_SYMBOL_PROPERTIES`) -- one undo step with the rest of the dialog, the toolbar's unit and body style boxes naming them
// (`RebuildSymbolUnitAndBodyStyleLists`), the exported `.kicad_sym` saying `(body_styles ..)`, `(unit_name ..)` and the jumper groups; Validate's refusals;
// and Text Box / Text Properties' Font, colours, Background fill and "Private to symbol editor" (`DIALOG_TEXT_PROPERTIES`), drawn by the canvas and written by
// `saveTextBox` / `saveText`.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/symedit-props-u6.check.js 6`. It makes a library of its own (`Y<time>Lib`) in the project's table.
// It answers through `window.__symeditPropsU6Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__symeditPropsU6Check = { results, done: false, ok: false, wait: null });
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
  // a refusal of Symbol Properties is KiCad's message box (`DisplayError`, UI-LEFT-2 YY6): its text, and OK closes it
  const msgBox = () => {
    const text = tid("message-box-text")?.textContent ?? "";
    tid("message-box-ok")?.click();
    return text;
  };
  const type = async (el, value) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(80);
  };
  const click = async (el, ms = 150) => { el?.click(); await sleep(ms); };
  const until = async (cond, ms = 8000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(600); };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const libRow = (lib) => [...document.querySelectorAll(".library-tree-group")].find((g) => g.textContent.replace(/[▾▸]/g, "").trim().replace(/\s*\*?\s*[\d…]*$/, "").replace(/^☆ /, "") === lib);
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const exported = async (id) => (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`, { cache: "no-store" })).text();
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
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
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };
  const openProps = async () => { await run("eeschema.InteractiveEdit.symbolProperties"); return until(() => tid("symbol-properties"), 4000); };
  const canvas = () => document.querySelector(".editor-canvas-col canvas");
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** The canvas's colour at the editor's point (nm, +y down). */
  const pixel = (wx, wy) => {
    const c = canvas(); const ctx = c.getContext("2d"); const dpr = c.width / parseFloat(c.style.width || c.width); const rect = c.getBoundingClientRect();
    const [x, y] = toScreen(wx, wy);
    const d = ctx.getImageData(Math.round((x - rect.left) * dpr), Math.round((y - rect.top) * dpr), 1, 1).data;
    return [d[0], d[1], d[2]];
  };

  const s = Date.now().toString(36);
  const LIB = `Y${s}Lib`;
  const ID = `${LIB}:Y4`;
  const ids = {};

  // ------------------------------------------------------------------------------------------------------------------------ the symbol
  await scenario("a library and a two-unit symbol with pins 1-4, a text box and a text", async (t) => {
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
    const nd = await until(() => tid("new-symbol"), 3000);
    if (!t(nd, "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "Y4");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    const cur = await symbol(ID);
    const pin = (number, unit, y) => ({ number, name: `P${number}`, electrical_type: "passive", shape: "line", at: { x: -7.62, y }, angle_deg: 0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const c of [
      { op: "edit_symbol_properties", lib_id: ID, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 2, has_alternate_body_style: false, footprint_filters: [] },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("1", 1, 2.54) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("2", 1, 0) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("3", 2, -2.54) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("4", 2, -5.08) },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text_box", unit: 0, body_style: 1, text: "NOTE", start: { x: -5.08, y: 12.7 }, end: { x: 5.08, y: 7.62 }, angle_deg: 0, size_mm: 1.27, h_align: "left", v_align: "top", stroke_mm: 0.254, fill: "none" } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text", unit: 0, body_style: 1, text: "LBL", at: { x: 0, y: -10.16 }, angle_deg: 0, size_mm: 1.27 } },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const sym = await symbol(ID);
    ids.box = sym.graphics.find((g) => g.kind === "text_box")?.id;
    ids.text = sym.graphics.find((g) => g.kind === "text")?.id;
    t(ids.box && ids.text, `ids: ${JSON.stringify(ids)}`);
    await (await itemRow(ID))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    t(E.state().symbolEditor?.libId === ID, `the symbol is open (${JSON.stringify(E.state().symbolEditor)})`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ Units & Body Styles, Pin Connections
  await scenario("Symbol Properties: unit display names, custom body styles and jumper groups in one step; the toolbar names them; the file says them", async (t) => {
    if (!t(await openProps(), "Symbol Properties opens")) return;
    t(!!tid("props-unit-names") && !!tid("props-unit-name-1") && !!tid("props-unit-name-2") && !tid("props-unit-name-3"), "a names row per unit");
    t(tid("props-single-style")?.checked === true && tid("props-body-style-names-add")?.disabled === true, "single body style; the custom grid is disabled (syncBodyStyleControls)");
    t(!!tid("props-pin-connections") && tid("props-duplicate-jumpers")?.checked === false, "the Pin Connections group");
    // `updateUnitCount`: the names grid follows the count
    await type(tid("props-unit-count"), "3");
    t(!!tid("props-unit-name-3"), "three units: three rows");
    await type(tid("props-unit-count"), "2");
    t(!tid("props-unit-name-3"), "back to two");
    // Validate (25ec2335): a name in use, case aside
    await type(tid("props-unit-name-1"), "Gate");
    await type(tid("props-unit-name-2"), "gate");
    await click(tid("props-ok"), 500);
    { const said = msgBox(); t(/The unit name 'gate' is already in use\./.test(said) && !!tid("symbol-properties"), `refused: ${said}`); }
    await type(tid("props-unit-name-2"), "Power");
    // custom styles: fewer than two names is refused
    await click(tid("props-custom-styles"));
    t(tid("props-body-style-names-add")?.disabled === false, "Custom body styles enables the grid");
    await click(tid("props-body-style-names-add"));
    await type(tid("props-body-style-names-row-0"), "Front");
    await click(tid("props-ok"), 500);
    { const said = msgBox(); t(/Custom body styles must have at least 2 entries/.test(said), `refused: ${said}`); }
    await click(tid("props-body-style-names-add"));
    await type(tid("props-body-style-names-row-1"), "Top");
    await click(tid("props-body-style-names-add"));
    await type(tid("props-body-style-names-row-2"), "Side");
    // Move up: Side above Top
    tid("props-body-style-names-row-2").focus(); await sleep(80);
    await click(tid("props-body-style-names-up"));
    t(tid("props-body-style-names-row-1")?.value === "Side" && tid("props-body-style-names-row-2")?.value === "Top", "Move up swaps the rows");
    // Pin Connections
    await click(tid("props-duplicate-jumpers"));
    await click(tid("props-jumper-groups-add"));
    await type(tid("props-jumper-groups-row-0"), "3, 1");
    await click(tid("props-jumper-groups-add"));
    await type(tid("props-jumper-groups-row-1"), "4 2");
    await click(tid("props-ok"), 1500);
    t(!tid("symbol-properties"), `OK closes the dialog ${tid("props-error")?.textContent ?? ""}`);
    const sym = await symbol(ID);
    t(JSON.stringify(sym.body_style_names) === JSON.stringify(["Front", "Side", "Top"]) && !sym.has_alternate_body_style, `body styles: ${JSON.stringify(sym.body_style_names)}`);
    t(sym.kicad?.unit_names?.["1"] === "Gate" && sym.kicad?.unit_names?.["2"] === "Power", `unit names: ${JSON.stringify(sym.kicad?.unit_names)}`);
    t(sym.kicad?.duplicate_pin_numbers_are_jumpers === true && JSON.stringify(sym.kicad?.jumper_pin_groups) === JSON.stringify([["1", "3"], ["2", "4"]]), `jumpers: ${JSON.stringify(sym.kicad?.jumper_pin_groups)}`);
    // (the server leaves out a body style of 1)
    const styles = sym.pins.map((p) => p.body_style ?? 1).sort().join("");
    t(styles === "111122223333", `SetBodyStyleCount( 3, true, true ): every style-1 pin copied into styles 2 and 3 (${styles})`);
    const boxStyles = sym.graphics.filter((g) => g.kind === "text_box").map((g) => g.body_style ?? 1).sort().join("");
    t(boxStyles === "123", `and every style-1 graphic (aDuplicateDrawItems): ${boxStyles}`);
    // the toolbar's boxes
    await sleep(400);
    const bodySel = tid("sym-body-style-select");
    t(bodySel && !bodySel.disabled && [...bodySel.options].map((o) => o.textContent).join(",") === "Front,Side,Top", `body style box: ${bodySel && [...bodySel.options].map((o) => o.textContent)}`);
    const unitSel = document.querySelector('.toolbar-control select:not([data-testid])');
    const unitTexts = [...document.querySelectorAll(".toolbar-control select")].map((sel) => [...sel.options].map((o) => o.textContent).join(","));
    t(unitTexts.includes("Gate,Power"), `unit box: ${JSON.stringify(unitTexts)} ${unitSel ? "" : ""}`);
    // the file
    const text = await exported(ID);
    t(text.includes('(body_styles "Front" "Side" "Top")'), "the .kicad_sym says (body_styles ..)");
    t(text.includes('(unit_name "Gate")') && text.includes('(unit_name "Power")'), "and each unit's (unit_name ..)");
    t(text.includes('(duplicate_pin_numbers_are_jumpers yes)') && text.includes('(jumper_pin_groups ("1" "3") ("2" "4"))'), "and the jumpers");
    t(text.includes('_1_3"') && text.includes('_2_3"'), "and the third style's sub-symbols");
    // one undo step
    await undo();
    const back = await symbol(ID);
    t(!back.body_style_names?.length && !back.kicad?.unit_names && back.pins.length === 4, `one undo step takes it all back: ${JSON.stringify([back.body_style_names, back.kicad?.unit_names, back.pins.length])}`);
    await E.run("common.Interactive.redo"); await sleep(900);
    t((await symbol(ID)).body_style_names?.length === 3, "redo brings it back");
  });

  await scenario("Symbol Properties: a jumper pin the symbol has not is refused with KiCad's message; De Morgan from custom keeps two styles", async (t) => {
    if (!t(await openProps(), "Symbol Properties opens")) return;
    t(tid("props-custom-styles")?.checked === true && tid("props-body-style-names-row-2")?.value === "Top", "it opens on the custom styles");
    t(tid("props-unit-name-1")?.value === "Gate" && tid("props-jumper-groups-row-0")?.value === "1, 3", "the names and the groups as rows");
    await type(tid("props-jumper-groups-row-1"), "4, 9");
    await click(tid("props-ok"), 200);
    const said = await until(() => document.body.textContent.includes("Pin '9' in jumper pin group 2 does not exist in this symbol."), 4000);
    t(!!said, "KiCad's message is shown");
    const refused = await symbol(ID);
    t(JSON.stringify(refused.kicad?.jumper_pin_groups) === JSON.stringify([["1", "3"], ["2", "4"]]) && refused.body_style_names?.length === 3, `nothing changed: ${JSON.stringify(refused.kicad?.jumper_pin_groups)}`);
    await closeDialogs();
    if (!t(await openProps(), "Symbol Properties opens again")) return;
    await click(tid("props-demorgan"));
    await click(tid("props-ok"), 600);
    // three to two: "Delete extra body styles from symbol?"
    const yes = await until(() => [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Yes"), 3000);
    t(!!yes, "Validate asks: Delete extra body styles from symbol?");
    await click(yes, 1500);
    const dm = await symbol(ID);
    const dmStyles = dm.pins.map((p) => p.body_style ?? 1).sort().join("");
    t(dm.has_alternate_body_style && !dm.body_style_names?.length && dmStyles === "11112222" && dm.graphics.length === 4, `De Morgan: style 3 pruned, style 2 kept as it was (${dmStyles}, ${dm.graphics.length} graphics)`);
    const bodySel = tid("sym-body-style-select");
    t([...(bodySel?.options ?? [])].map((o) => o.textContent).join(",") === "Standard,Alternate", "the body style box: Standard, Alternate");
  }, [/does not exist in this symbol/]);

  // ------------------------------------------------------------------------------------------------------------------------ the text box and the text
  await scenario("Text Box Properties: font, text and border colours, background fill with its colour, Private; drawn and written", async (t) => {
    await E.run("common.Control.zoomFitScreen"); await sleep(400);
    await select([ids.box]);
    await run("eeschema.InteractiveEdit.properties");
    if (!t(await until(() => tid("text-box-properties"), 3000), "Text Box Properties opens")) return;
    t(!!tid("tb-font")?.querySelector("select") && tid("tb-font").querySelector("select").value === "Default Font", "the Font choice starts at Default Font");
    t(tid("tb-private")?.checked === false && tid("tb-filled")?.checked === false, "not private, no background fill");
    await type(tid("tb-font").querySelector("select"), "KiCad Font");
    // the text colour, the border colour, the fill and its colour
    const swatch = async (id, hex) => { const box = tid(id); const def = box.querySelector('input[type=checkbox]'); if (def.checked) await click(def); await type(box.querySelector('input[type=color]'), hex); };
    await swatch("tb-color", "#00ff00");
    await swatch("tb-border-color", "#ff0000");
    await click(tid("tb-filled"));
    await swatch("tb-fill-color", "#0000ff");
    await click(tid("tb-private"));
    await click(tid("tb-ok"), 1200);
    t(!tid("text-box-properties"), "OK closes the dialog");
    const g = (await symbol(ID)).graphics.find((x) => x.id === ids.box);
    t(g?.fill === "color" && g?.fill_color?.b === 255 && g?.fill_color?.r === 0, `FILLED_WITH_COLOR in blue: ${JSON.stringify([g?.fill, g?.fill_color])}`);
    t(g?.stroke_color?.r === 255 && g?.text_color?.g === 255 && g?.private === true && g?.font === "KiCad Font", `border red, text green, private, KiCad Font: ${JSON.stringify([g?.stroke_color, g?.text_color, g?.private, g?.font])}`);
    await select([]); await sleep(500);
    // inside the box, off its text: the fill colour (Y up mm 12.7..7.62 is -12.7e6..-7.62e6 nm down); the border's top edge
    const fill = pixel(3_500_000, -8_300_000);
    t(fill[2] > 200 && fill[0] < 60 && fill[1] < 60, `the box is filled blue: ${fill}`);
    const edge = pixel(0, -12_700_000);
    t(edge[0] > 200 && edge[1] < 80 && edge[2] < 80, `its border is red: ${edge}`);
    const text = await exported(ID);
    // the border Style (UI-LEFT-2 YY2) shows the default style as Solid and OK writes it: `stroke.SetLineStyle( it->first )`
    t(/\(text_box private "NOTE" .*\(stroke \(width 0\.254\) \(type solid\) \(color 255 0 0 1\)\) \(fill \(type color\) \(color 0 0 255 1\)\) \(effects \(font \(face "KiCad Font"\) \(size 1\.27 1\.27\) \(color 0 255 0 1\)\)/.test(text), "saveTextBox writes private, the colours and the face");
    await undo();
    const before = (await symbol(ID)).graphics.find((x) => x.id === ids.box);
    t(before?.fill === "none" && !before?.private && !before?.stroke_color, "one undo step");
    await E.run("common.Interactive.redo"); await sleep(900);
  });

  await scenario("Text Properties: Private, the font and the colour of a symbol's text", async (t) => {
    await select([ids.text]);
    await run("eeschema.InteractiveEdit.properties");
    if (!t(await until(() => tid("text-properties"), 3000), "Text Properties opens")) return;
    t(tid("text-private")?.checked === false && tid("text-font")?.querySelector("select")?.value === "Default Font", "not private, Default Font");
    await click(tid("text-private"));
    const box = tid("text-color"); await click(box.querySelector('input[type=checkbox]')); await type(box.querySelector('input[type=color]'), "#ff00ff");
    await click(tid("text-ok"), 1200);
    const g = (await symbol(ID)).graphics.find((x) => x.id === ids.text);
    t(g?.private === true && g?.text_color?.r === 255 && g?.text_color?.b === 255 && g?.text_color?.g === 0, `private, magenta: ${JSON.stringify([g?.private, g?.text_color])}`);
    const text = await exported(ID);
    t(text.includes('(text private "LBL" (at 0 -10.16 0) (effects (font (size 1.27 1.27) (color 255 0 255 1))))'), "saveText writes `private` before the text and the colour");
    // reopening shows what it holds; turning Private off takes the word out
    await select([ids.text]);
    await run("eeschema.InteractiveEdit.properties");
    await until(() => tid("text-properties"), 3000);
    t(tid("text-private")?.checked === true, "it reopens private");
    await click(tid("text-private"));
    await click(tid("text-ok"), 1200);
    t(!(await symbol(ID)).graphics.find((x) => x.id === ids.text)?.private, "unticked: not private");
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
