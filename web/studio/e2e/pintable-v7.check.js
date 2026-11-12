// The Pin Table of UI-LEFT-2 YY4 (task-v7-symedit; PARITY-symedit.md, "Round 2, task-v7-symedit"): Group Selected Pins (`OnGroupSelected`), the options
// button's "Highlight on Cross-probe" and the cross-probe (`OnCellSelected` -> `SYMBOL_EDIT_FRAME::FocusOnItem`: the pin brightened, its unit shown), Import From
// File... (`OnImportButtonClick`, replace and append, the "Errors" question), a length worked out by `NUMERIC_EVALUATOR`, the trash button deleting the selected
// rows (no per-row Delete), and the cells' icons -- then OK as one undo step.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/pintable-v7.check.js 6`. It makes a library of its own (`PT<time>Lib`) in the project's table.
// It answers through `window.__pintableV7Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (window.__pintableV7Check = { results, done: false, ok: false, wait: null });
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
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
    el.dispatchEvent(new Event("input", { bubbles: true }));
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
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
      const discard = tid("choice-alt");
      if (discard) { discard.click(); await sleep(250); }
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };
  const rows = () => [...document.querySelectorAll('[data-testid="pin-table-row"]')];
  const cellOf = (row, label) => row.querySelector(`[aria-label="${label}"]`);
  const numbersOf = () => rows().map((r) => cellOf(r, "Number")?.value);
  const mouseDown = async (el, shift = false) => { el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, shiftKey: shift })); await sleep(250); };
  /** The next browser file chooser answers with `text` as `name` (`pickTextFile`'s `<input type=file>`). */
  const answerFileChooser = (name, text) => {
    const original = HTMLInputElement.prototype.click;
    HTMLInputElement.prototype.click = function () {
      if (this.type !== "file") return original.call(this);
      HTMLInputElement.prototype.click = original;
      const dt = new DataTransfer();
      dt.items.add(new File([text], name, { type: "text/csv" }));
      this.files = dt.files;
      this.dispatchEvent(new Event("change"));
    };
  };
  const openTable = async () => {
    await run("eeschema.InteractiveEdit.pinTable");
    return until(() => tid("pin-table"), 4000);
  };

  const s = Date.now().toString(36);
  const LIB = `PT${s}Lib`;
  const ID = `${LIB}:PT`;

  await scenario("a library and a two-unit symbol: 1 A and 2 B on unit A, 3 C on unit B", async (t) => {
    await clickTab("Symbol");
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog("New Symbol Library"), 4000);
    if (!t(d, "New Symbol Library opens")) return;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), LIB);
    button(d, "Create").click();
    t(await until(() => !dialog("New Symbol Library"), 6000), `${LIB} made`);
    const r = await until(() => libRow(LIB), 6000);
    r?.click();
    await sleep(250);
    if (!isOpen(LIB)) libRow(LIB)?.click();
    await sleep(250);
    await run("eeschema.SymbolLibraryControl.newSymbol");
    if (!t(await until(() => tid("new-symbol"), 3000), "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "PT");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    const cur = await symbol(ID);
    const pin = (number, name, unit, x, y) => ({ number, name, electrical_type: "input", shape: "line", at: { x, y }, angle_deg: 0, length_mm: 2.54, unit, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const c of [
      { op: "edit_symbol_properties", lib_id: ID, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 2, has_alternate_body_style: false, footprint_filters: [] },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("1", "A", 1, -7.62, 2.54) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("2", "B", 1, -7.62, 0) },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("3", "C", 2, 200, 200) },
    ]) {
      const res = await cmd(c);
      t(res.ok !== false, `${c.op}: ${res.message ?? "ok"}`);
    }
    if (!isOpen(LIB)) libRow(LIB)?.click();
    (await until(() => treeRow(ID), 6000))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    t(await until(() => E.state().symbolEditor?.libId === ID, 4000), "the symbol is open");
  });

  await scenario("no per-row Delete; the type, style and orientation cells carry KiCad's icons", async (t) => {
    if (!t(await openTable(), "the Pin Table opens")) return;
    t(rows().length === 3, `three rows (${rows().length})`);
    t(!rows().some((r) => [...r.querySelectorAll("button")].some((b) => b.textContent.trim() === "Delete")), "no Delete button in a row");
    const icons = [...rows()[0].querySelectorAll("img[data-icon]")].map((i) => i.dataset.icon);
    t(JSON.stringify(icons) === JSON.stringify(["pintype_input", "pinshape_normal", "pinorient_right"]), `row 1's icons: ${icons}`);
    const img = rows()[0].querySelector("img[data-icon]");
    t(await until(() => img.complete && img.naturalWidth > 0, 3000), `the icon file loads (${img.src})`);
  });

  await scenario("cross-probe: a row's pin is brightened and its unit shown (FocusOnItem); off from the options menu; cleared when the table closes", async (t) => {
    if (!t(await openTable(), "the Pin Table opens")) return;
    const row3 = rows().find((r) => cellOf(r, "Number")?.value === "3");
    await mouseDown(row3);
    const pin3 = (await symbol(ID)).pins.find((p) => p.number === "3");
    t(await until(() => E.state().symbolEditor?.brightened === pin3.id, 2000), `pin 3 is brightened (${E.state().symbolEditor?.brightened})`);
    t(E.state().symbolEditor?.unit === 2, `unit B is shown (${E.state().symbolEditor?.unit})`);
    // the pin at (200, 200) mm is far off the view: the view moves to it
    const v = E.state().view;
    const sx = 200e6 * v.scale + v.x;
    const w = Math.max(0, ...[...document.querySelectorAll("canvas")].map((c) => c.clientWidth));
    t(sx > 0 && sx < w, `the view is moved to the pin (screen x ${Math.round(sx)} of ${w})`);
    tid("pin-table-options").click();
    await sleep(200);
    const item = [...document.querySelectorAll(".context-menu *, [role=menuitem]")].find((e) => /Highlight on Cross-probe/.test(e.textContent) && e.children.length === 0) ?? [...document.querySelectorAll("*")].find((e) => e.children.length === 0 && /Highlight on Cross-probe/.test(e.textContent));
    if (!t(item, "the options menu has Highlight on Cross-probe")) return;
    item.click();
    await sleep(250);
    t(E.state().symbolEditor?.brightened === null, "turning it off clears the highlight");
    await mouseDown(rows().find((r) => cellOf(r, "Number")?.value === "1"));
    t(E.state().symbolEditor?.brightened === null, "a row selected with it off brightens nothing");
    tid("pin-table-options").click();
    await sleep(200);
    ([...document.querySelectorAll("*")].find((e) => e.children.length === 0 && /Highlight on Cross-probe/.test(e.textContent)))?.click();
    await sleep(200);
    await mouseDown(rows().find((r) => cellOf(r, "Number")?.value === "1"));
    t(E.state().symbolEditor?.brightened != null, "on again: the row's pin is brightened");
    await closeDialogs();
    t(E.state().symbolEditor?.brightened === null, "closing the table clears the focus (ClearFocus)");
  });

  await scenario("Group Selected Pins: the block's pins in the first row, Count shown; a length worked out from 2.54*2; the trash deletes the selected rows; OK is one undo step", async (t) => {
    if (!t(await openTable(), "the Pin Table opens")) return;
    const byNum = (n) => rows().find((r) => cellOf(r, "Number")?.value === n);
    await mouseDown(byNum("1"));
    await mouseDown(byNum("3"), true);
    t(rows().filter((r) => r.dataset.selected === "1").length === 3, "Shift+click selects the rows between");
    tid("pin-table-group-selected").click();
    await sleep(300);
    t(cellOf(rows()[0], "Number")?.value === "1, 2, 3" && rows().length === 1, `one row of the three: ${numbersOf()}`);
    t([...document.querySelectorAll('[data-testid="pin-table-header"] th[data-col]')].some((h) => h.dataset.col === "Count"), "the Count column is shown");
    tid("pin-table-refresh").click();
    await sleep(300);
    t(rows().length === 3, "Refresh splits them again");
    // the length column, from the columns menu, worked out
    tid("pin-table-header").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 300, clientY: 200 }));
    await sleep(200);
    ([...document.querySelectorAll("*")].find((e) => e.children.length === 0 && /^\W*Length$/.test(e.textContent.trim())))?.click();
    await sleep(250);
    const len = cellOf(byNum("2"), "Length");
    if (!t(len, "the Length column is shown")) return;
    len.focus();
    await type(len, "2.54*2");
    len.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    len.blur();
    await sleep(250);
    t(/^5\.08/.test(cellOf(byNum("2"), "Length").value), `2.54*2 is ${cellOf(byNum("2"), "Length").value}`);
    cellOf(byNum("2"), "Length").focus();
    await sleep(150);
    t(cellOf(byNum("2"), "Length").value === "2.54*2", `editing it again shows the expression (${cellOf(byNum("2"), "Length").value})`);
    cellOf(byNum("2"), "Length").blur();
    await sleep(150);
    await mouseDown(byNum("1"));
    tid("pin-table-delete").click();
    await sleep(250);
    t(JSON.stringify(numbersOf().sort()) === JSON.stringify(["2", "3"]), `the trash deletes pin 1: ${numbersOf()}`);
    tid("pin-table-ok").click();
    await sleep(1500);
    const sym = await symbol(ID);
    const p2 = sym.pins.find((p) => p.number === "2");
    t(sym.pins.length === 2 && Math.abs(p2.length_mm - 5.08) < 1e-6, `saved: ${sym.pins.length} pins, pin 2 ${p2?.length_mm} mm`);
    await E.run("common.Interactive.undo");
    await sleep(900);
    t((await symbol(ID)).pins.length === 3, "one Undo brings the table's edit back");
  });

  await scenario("Import From File...: unknown columns asked about (Errors), Append adds, Replace all replaces", async (t) => {
    if (!t(await openTable(), "the Pin Table opens")) return;
    tid("pin-table-import-append").click();
    answerFileChooser("pins.csv", "Number,Name,Electrical Type,Graphic Style,Orientation,Length,X Position,Y Position,Visible,Colour\n10,CLK,Output,Clock,Left,100mil,5.08,-2.54,True,red\n11,DO,Bidirectional,Line,Left,2.54,5.08,0,False,blue\n");
    tid("pin-table-import-file").click();
    const ask = await until(() => [...document.querySelectorAll("[role=dialog]")].find((d) => /Unknown columns in data: Colour\. These columns will be ignored\./.test(d.textContent)), 3000);
    if (!t(ask, "KiCad's Errors message")) return;
    button(ask, "OK").click();
    await sleep(400);
    t(JSON.stringify(numbersOf().sort()) === JSON.stringify(["1", "10", "11", "2", "3"]), `appended: ${numbersOf()}`);
    const clk = rows().find((r) => cellOf(r, "Number")?.value === "10");
    t(cellOf(clk, "Electrical Type")?.value === "Output" && cellOf(clk, "Graphic Style")?.value === "Clock" && cellOf(clk, "Orientation")?.value === "Left", "the imported cells");
    tid("pin-table-import-replace").click();
    answerFileChooser("pins.csv", "Number,Name\n20,X\n");
    tid("pin-table-import-file").click();
    await sleep(500);
    t(JSON.stringify(numbersOf()) === JSON.stringify(["20"]), `replaced: ${numbersOf()}`);
    tid("pin-table-ok").click();
    await sleep(1500);
    const sym = await symbol(ID);
    t(sym.pins.length === 1 && sym.pins[0].number === "20" && sym.pins[0].name === "X", `saved: ${sym.pins.map((p) => p.number)}`);
  });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
