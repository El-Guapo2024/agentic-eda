// Symbol Editor leftovers of UI-LEFT Y6, Y7, Y8 and X11 (task-u5-dialogs; PARITY-symedit.md): the Pin Table's summary, KiCad's columns and names, its grouping
// by name with a grouped row's renumbering, and the columns menu (`DIALOG_LIB_EDIT_PIN_TABLE`); Pin Properties with KiCad's words, no Delete Pin and no clamps
// (r7 E7-S4); a power symbol's Exclude boxes enabled when Symbol Properties opens (E7-S9); Find, Find and Replace, Replace and Replace All over the open symbol
// (`SCH_FIND_REPLACE_TOOL` in a `SYMBOL_EDIT_FRAME`); Duplicate opening the copy at the unit being edited (E7-S21); a derived symbol in italics in the tree
// (`SetItalic( !m_IsRoot )`); the Symbol Checker's toolbar button live; and Export Symbol as SVG asking kicad-cli for the editor's theme and the hidden items.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/symedit-u5.check.js 8`. It makes a library of its own (`U5<time>Lib`) in the project's table.
// It answers through `window.__symeditU5Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__symeditU5Check = { results, done: false, ok: false, wait: null });
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
  const enter = async (el) => { el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await sleep(120); };
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
  const selected = () => E.state().selection.map((s) => s.id ?? s);
  const pinsOf = async () => (await symbol(ID)).pins ?? [];

  const s = Date.now().toString(36);
  const LIB = `U${s}Lib`;
  const ID = `${LIB}:U5`;
  const ids = {};

  // ------------------------------------------------------------------------------------------------------------------------ the symbol
  await scenario("a library and a two-unit symbol: VCC (1) and GND (2) on unit A, VCC (3) on unit B, a text", async (t) => {
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
    await type(tid("new-symbol-name"), "U5");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    const cur = await symbol(ID);
    const pin = (number, name, unit, y) => ({ number, name, electrical_type: "power_in", shape: "line", at: { x: -7.62, y }, angle_deg: 0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const c of [
      { op: "edit_symbol_properties", lib_id: ID, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 2, has_alternate_body_style: false, footprint_filters: [] },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("1", "VCC", 1, 2.54) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("2", "GND", 1, 0) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("3", "VCC", 2, -2.54) },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text", unit: 0, body_style: 1, text: "VCC rail", at: { x: 0, y: 3.81 }, angle_deg: 0, size_mm: 1.27 } },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const sym = await symbol(ID);
    for (const p of sym.pins) ids[`p${p.number}`] = p.id;
    ids.text = sym.graphics.find((g) => g.kind === "text")?.id;
    t(ids.p1 && ids.p2 && ids.p3 && ids.text, `ids: ${JSON.stringify(ids)}`);
    await (await itemRow(ID))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    t(E.state().symbolEditor?.libId === ID, `the symbol is open (${JSON.stringify(E.state().symbolEditor)})`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y6: the Pin Table
  await scenario("the Pin Table: KiCad's summary, its eight first columns, the type names; Group by name and a grouped row's renumbering; the columns menu", async (t) => {
    await run("eeschema.InteractiveEdit.pinTable");
    if (!t(await until(() => tid("pin-table"), 4000), "the Pin Table opens")) return;
    t(tid("pin-table-numbers")?.textContent === "1-3" && tid("pin-table-count")?.textContent === "3" && tid("pin-table-duplicates")?.textContent === "none", `Pin numbers: ${tid("pin-table-numbers")?.textContent}, Pin count: ${tid("pin-table-count")?.textContent}, Duplicate pins: ${tid("pin-table-duplicates")?.textContent}`);
    const headers = () => [...document.querySelectorAll('[data-testid="pin-table-header"] th[data-col]')].map((h) => h.dataset.col);
    t(headers().join(",") === "Count,Number,Name,Electrical Type,Graphic Style,Orientation,X Position,Y Position", `ShowHideColumns( "0 1 2 3 4 5 9 10" ): ${headers().join(",")}`);
    const rows = () => [...document.querySelectorAll('[data-testid="pin-table-row"]')];
    const cell = (row, label) => row.querySelector(`[aria-label="${label}"]`);
    t(rows().length === 3 && cell(rows()[0], "Number").value === "1" && cell(rows()[2], "Number").value === "3", `a row a pin, by Count then Number: ${rows().map((r) => cell(r, "Number").value)}`);
    t(cell(rows()[0], "Electrical Type").value === "Power input" && cell(rows()[0], "Graphic Style").value === "Line" && cell(rows()[0], "Orientation").value === "Right", `KiCad's names: ${cell(rows()[0], "Electrical Type").value} / ${cell(rows()[0], "Graphic Style").value}`);
    t(cell(rows()[0], "Count").textContent === "1", "Count 1");
    t(/mm$/.test(cell(rows()[0], "X Position").value), `a position in the frame's units: ${cell(rows()[0], "X Position").value}`);
    tid("pin-table-group").click();
    await sleep(300);
    const vcc = rows().find((r) => cell(r, "Name").value === "VCC");
    t(rows().length === 2 && vcc && cell(vcc, "Number").value === "1, 3" && cell(vcc, "Count").textContent === "2", `Group by name: ${rows().map((r) => `${cell(r, "Name").value}=${cell(r, "Number").value}`)}`);
    if (vcc) {
      await type(cell(vcc, "Number"), "1, 3, 4");
      await enter(cell(vcc, "Number"));
      t(tid("pin-table-count")?.textContent === "4" && tid("pin-table-numbers")?.textContent === "1-4", `a fourth number adds a pin: ${tid("pin-table-count")?.textContent} (${tid("pin-table-numbers")?.textContent})`);
    }
    tid("pin-table-header").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 300, clientY: 200 }));
    await sleep(150);
    [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].find((b) => b.textContent.replace(/[✓ ]/g, "").trim() === "Length")?.click();
    await sleep(150);
    t(headers().includes("Length"), `the header's menu shows Length: ${headers().join(",")}`);
    await (async () => { tid("pin-table-ok").click(); await sleep(1500); })();
    const pins = await pinsOf();
    const four = pins.find((p) => p.number === "4");
    t(pins.length === 4 && four && four.name === "VCC" && four.unit === 2, `OK wrote the new pin, the row's last pin's name and unit: ${JSON.stringify(four)}`);
    await undo();
    t((await pinsOf()).length === 3, "one undo takes the table back");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y7: Pin Properties
  await scenario("Pin Properties: KiCad's labels and rows, no Delete Pin, a text size taken as typed (r7 E7-S4)", async (t) => {
    await select([ids.p1]);
    await run("eeschema.InteractiveEdit.properties");
    const header = await until(() => [...document.querySelectorAll(".dialog-header")].find((h) => /Pin Properties/.test(h.textContent)), 4000);
    if (!t(header, "Pin Properties opens")) return;
    const d = header.closest(".dialog");
    const labels = [...d.querySelectorAll(".kv-grid > span")].map((e) => e.textContent.trim()).filter((l) => l.endsWith(":"));
    t(["Pin name:", "Pin number:", "Electrical type:", "Graphic style:", "X position:", "Y position:", "Orientation:", "Pin length:", "Name text size:", "Number text size:"].every((l, i) => labels[i] === l), `the rows: ${labels.slice(0, 10).join(" | ")}`);
    t(!button(d, "Delete Pin"), "no Delete Pin button");
    const sizeInput = [...d.querySelectorAll(".kv-grid > span")].find((e) => e.textContent.trim() === "Name text size:")?.nextElementSibling?.querySelector("input");
    await type(sizeInput, "0.05");
    button(d, "OK").click();
    await sleep(1200);
    const p1 = (await pinsOf()).find((p) => p.id === ids.p1);
    t(p1 && Math.abs((p1.name_size_mm ?? 0) - 0.05) < 1e-9, `0.05 mm kept, not clamped to 0.1: ${p1?.name_size_mm}`);
    await undo();
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y7: a power symbol's Exclude boxes
  await scenario("Symbol Properties of a power symbol: the Exclude boxes open enabled; ticking Define as power symbol checks and disables them (onPowerCheckBox, r7 E7-S9)", async (t) => {
    const cur = await symbol(ID);
    const r = await cmd({ op: "edit_symbol_properties", lib_id: ID, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: true, in_bom: false, on_board: false, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 2, has_alternate_body_style: false, footprint_filters: [] });
    t(r.ok !== false, "the symbol is a power symbol");
    await sleep(1200);
    await run("eeschema.InteractiveEdit.symbolProperties");
    if (!t(await until(() => tid("props-power"), 4000), "Symbol Properties opens")) return;
    t(tid("props-power").checked && !tid("props-exclude_from_sim").disabled && !tid("props-in_bom").disabled, "opened on a power symbol, the boxes are enabled (TransferDataToWindow sets their values only)");
    tid("props-power").click(); await sleep(80);
    tid("props-power").click(); await sleep(80);
    t(tid("props-exclude_from_sim").disabled && tid("props-exclude_from_sim").checked && tid("props-in_bom").disabled, "ticking the box again checks and disables them");
    await closeDialogs();
    await undo();
    t((await symbol(ID)).power !== true, "one undo: not a power symbol again");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y7: Find and Replace
  await scenario("Find in the Symbol Editor: the matches by x then y, the end of the symbol, the wrap; Replace and Replace All, one undo step each", async (t) => {
    await select([]);
    await run("common.Interactive.find");
    if (!t(await until(() => tid("sym-find-dialog"), 3000), "Ctrl+F opens Find on the Symbol tab")) return;
    t(!document.querySelector('[data-testid="sym-find-dialog"]')?.textContent.includes("Search pin names and numbers"), "no Search pin names box (it is on, and hidden, in the Symbol Editor)");
    await type(tid("sym-find-text"), "VCC");
    const next = async () => { tid("sym-find-next").click(); await sleep(500); return selected(); };
    t((await next()).join() === ids.p1, `the first match: pin 1 (${selected()})`);
    t((await next()).join() === ids.p3, `then pin 3, the next by y (${selected()})`);
    t((await next()).join() === ids.text, `then the text (${selected()})`);
    await next();
    t(/Reached end of symbol\. Find again to wrap around to the start\./.test(tid("sym-find-status")?.textContent ?? ""), `the end: ${tid("sym-find-status")?.textContent}`);
    t((await next()).join() === ids.p1, "a Find within 4 s wraps to the start");
    await closeDialogs();
    await run("common.Interactive.findAndReplace");
    if (!t(await until(() => tid("sym-replace-text"), 3000), "Find and Replace opens with Replace with:")) return;
    await type(tid("sym-find-text"), "VCC");
    await type(tid("sym-replace-text"), "VDD");
    tid("sym-replace-all").click();
    await sleep(1500);
    let sym = await symbol(ID);
    const names = sym.pins.map((p) => `${p.number}:${p.name}`).sort().join(" ");
    t(names === "1:VDD 2:GND 3:VDD" && sym.graphics.find((g) => g.id === ids.text)?.text === "VDD rail", `Replace All: ${names}, ${sym.graphics.find((g) => g.id === ids.text)?.text}`);
    await closeDialogs();
    await undo();
    sym = await symbol(ID);
    t(sym.pins.filter((p) => p.name === "VCC").length === 2 && sym.graphics.find((g) => g.id === ids.text)?.text === "VCC rail", "one undo takes every replacement back");
    // Replace: the selected match, then Find Next
    await select([]);
    await run("common.Interactive.findAndReplace");
    await until(() => tid("sym-replace-text"), 3000);
    await type(tid("sym-find-text"), "VCC");
    await type(tid("sym-replace-text"), "VDD");
    tid("sym-find-next").click();
    await sleep(500);
    tid("sym-replace").click();
    await sleep(1500);
    sym = await symbol(ID);
    t(sym.pins.find((p) => p.id === ids.p1)?.name === "VDD" && sym.pins.find((p) => p.id === ids.p3)?.name === "VCC", "Replace changes the current match only");
    t(selected().join() === ids.p3, `and finds the next one (${selected()})`);
    await closeDialogs();
    await undo();
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y7: Duplicate at the unit being edited
  await scenario("Duplicate opens the copy at the unit being edited (LoadOneLibrarySymbolAux( .., GetUnit(), GetBodyStyle() ), r7 E7-S21)", async (t) => {
    await run("eeschema.EditorControl.nextUnit");
    t(E.state().symbolEditor?.unit === 2, `unit B is being edited (${JSON.stringify(E.state().symbolEditor)})`);
    await run("eeschema.SymbolLibraryControl.duplicateSymbol");
    const landed = await until(() => E.state().symbolEditor?.libId === `${ID}_1` && E.state().symbolEditor, 6000);
    t(landed && landed.unit === 2, `the copy is open at unit B: ${JSON.stringify(E.state().symbolEditor)}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y8: the tree and the Symbol Checker
  await scenario("a derived symbol is drawn in italics in the tree, at its library's level; the Symbol Checker's button is live", async (t) => {
    const r = await cmd({ op: "derive_symbol", parent: ID, lib_id: `${LIB}:U5D` });
    t(r.ok !== false, `derived: ${r.message ?? "ok"}`);
    const row = await until(() => treeRow(`${LIB}:U5D`), 6000);
    t(row && getComputedStyle(row).fontStyle === "italic" && row.dataset.derived === "true", `the derived symbol's row is in italics (${row && getComputedStyle(row).fontStyle})`);
    t(treeRow(ID) && getComputedStyle(treeRow(ID)).fontStyle === "normal", "its parent's is not");
    t(row && row.parentElement === treeRow(ID)?.parentElement, "both rows are the library's (no nesting)");
    const btn = [...document.querySelectorAll('[data-editor="symbol"] .toolbar-button')].find((b) => (b.title ?? "").startsWith("Symbol Checker"));
    t(btn && !btn.disabled, `the Symbol Checker button is live (${btn?.title})`);
    btn?.click();
    t(await until(() => [...document.querySelectorAll(".dialog-header")].some((h) => /Symbol Checker|Symbol Warnings/.test(h.textContent)), 4000), "it opens the Symbol Checker");
  });

  // ------------------------------------------------------------------------------------------------------------------------ X11: Export Symbol as SVG
  await scenario("Export Symbol as SVG asks kicad-cli for the editor's theme and the hidden pins and fields (SVGPlotSymbol's render settings)", async (t) => {
    const sent = [];
    const real = window.fetch;
    window.fetch = (url, init) => { if (String(url).includes("/api/sym/svg")) sent.push(JSON.parse(init?.body ?? "{}")); return real(url, init); };
    try {
      await run("eeschema.SymbolLibraryControl.exportSymbolAsSVG");
      await until(() => sent.length > 0, 8000);
      await sleep(1500);
    } finally {
      window.fetch = real;
    }
    const b = sent[0] ?? {};
    t(b.theme === "_builtin_default" && b.include_hidden_pins === true && b.include_hidden_fields === true, `the request: ${JSON.stringify(b)}`);
  }, [/kicad-cli/i]);

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
