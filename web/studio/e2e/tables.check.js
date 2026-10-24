// Tables in the board and the schematic check (GAPS.md item 24, package P16t): the board's Place > Add Stackup Table, Add Board Characteristics Table and Draw Table, the table
// and cell selections (a click picks a cell, Select Row(s) / Column(s) / Table), Add Row / Column, Delete Row(s) / Column(s), Merge and Unmerge Cells, Edit Table, Delete and Undo;
// the sheet's Draw Table, the same structure edits, the Properties panel's rows for a table cell, a table and its cells held, turned and locked as the table they are in, and the
// context menu's table entries.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), in a
// VISIBLE pane (a hidden one stops the timers it waits on) -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it
// edits and undoes, and it makes nothing that stays: every scenario undoes what it did, and the last check says the board and the sheet have the tables they began with.
// `window.__tablesOnly = /regexp/` runs only the scenarios whose names match.
//
// It answers through `window.__tablesCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a few minutes (each edit waits for the server's answer), longer than a tool call may last, so it runs on its own: read it with `await __tablesCheck.wait(30000)` (it
// returns when the run is done or the time is up) until `.done`.
//
// It works through `window.__eda` (every action by its id, `common.InteractiveSelection.selectItems` to select, `common.Interactive.undo`), the dialog's controls and the pointer events
// a user makes on the canvas; what the server holds is read back from `GET /api/state` (the board's `drawings.tables`) and `GET /api/schematic` (`tables`).
// run-all-checks: board=mcu30 minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__tablesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await sleep(500);
    return { done: check.done, ok: check.ok, n: results.length, passed: results.filter((r) => r.ok).map((r) => r.name.slice(0, 40)), failed: results.filter((r) => !r.ok) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const boardTables = async () => (await json("/api/state")).drawings?.tables ?? [];
  const sheetTables = async () => (await json("/api/schematic")).tables ?? [];
  const sheet = () => json("/api/schematic");
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const selectedIds = () => __eda.state().selection.map((x) => x.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const run = async (id, arg) => { await __eda.run(id, arg); await sleep(900); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(200); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const mouse = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: type === "contextmenu" ? 2 : 0, ...extra }));
  };
  /** Where a point of the editor (um) is on the canvas (px), from the view the hook reports. */
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(150); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(400); };
  const rclickAt = async (x, y) => { const [px, py] = screenOf(x, y); mouse("contextmenu", px, py); await sleep(600); };
  const center = (cell) => [cell.corners.reduce((a, p) => a + p[0], 0) / 4, cell.corners.reduce((a, p) => a + p[1], 0) / 4];
  const goTab = async (name) => {
    if (__eda.state().tab === name.toLowerCase()) return;
    buttonText(document, name)?.click();
    await sleep(1500);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
  };
  const menuLabels = () => { const m = document.querySelector(".menubar-dropdown, .context-menu"); return m ? [...m.querySelectorAll("*")].filter((e) => e.children.length === 0).map((e) => (e.textContent || "").trim()).filter(Boolean) : []; };
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(250); };
  const tableOf = (tables, id) => tables.find((t) => t.id === id);
  const only = window.__tablesOnly;
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  /** Run one scenario: `t(ok, note)` records a check; a scenario that throws fails. */
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const r = { name, ok: true, notes: [] };
    results.push(r);
    const t = (ok, note) => { r.notes.push(`${ok ? "ok" : "FAIL"}: ${note}`); if (!ok) r.ok = false; };
    try { await fn(t); } catch (e) { t(false, `threw ${e && e.message ? e.message : e}`); }
    await closeDialogs();
    await __eda.run("common.Interactive.cancel");
    await sleep(300);
  };

  (async () => {
  // ---------------------------------------------------------------------------------------------------------------------- the start
  if (__eda.state().tab !== "pcb") await goTab("PCB");
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);
  const board0 = (await boardTables()).length;
  const sheet0 = (await sheetTables()).length;
  const errors0 = __eda.errors().length;
  /** The carried tables (Place > Add Stackup Table / Add Board Characteristics) follow the cursor and are put down by a click. */
  const placeCarried = async (action) => {
    await run(action);
    await sleep(2500); // the characteristics table asks kicad-cli for the board's statistics first
    const at = screenOf(60_000, 20_000);
    fire("pointermove", at[0], at[1]); await sleep(200);
    fire("pointermove", at[0] + 4, at[1] + 4); await sleep(200);
    await clickAt(60_000_000 + 4 / __eda.state().view.scale, 20_000_000 + 4 / __eda.state().view.scale);
    await sleep(900);
  };

  // ---------------------------------------------------------------------------------------------------------------------- the board
  await scenario("board: Add Stackup Table is carried and put down; one Undo takes it away", async (t) => {
    await placeCarried("pcbnew.InteractiveDrawing.placeStackup");
    const tables = await boardTables();
    t(tables.length === board0 + 1, `a table was added (${board0} -> ${tables.length})`);
    const tb = tables[tables.length - 1];
    t(tb && tb.cols === 7 && tb.rows >= 6, `it has the stackup's seven columns (${tb && `${tb.rows} x ${tb.cols}`})`);
    t(tb && tb.cells.some((c) => c.text === "F.Cu" || /copper/i.test(c.text)), "and the board's layers in its cells");
    await undo();
    t((await boardTables()).length === board0, "one Undo gives the board the tables it had");
  });

  await scenario("board: Add Board Characteristics Table puts kicad-cli's numbers in a four column table", async (t) => {
    await placeCarried("pcbnew.InteractiveDrawing.placeCharacteristics");
    const tables = await boardTables();
    t(tables.length === board0 + 1, `a table was added (${board0} -> ${tables.length})`);
    const tb = tables[tables.length - 1];
    t(tb && tb.cols === 4, `four columns (${tb && tb.cols})`);
    const texts = tb ? tb.cells.map((c) => c.text) : [];
    t(texts.includes("BOARD CHARACTERISTICS"), "titled BOARD CHARACTERISTICS");
    t(texts.some((s) => /^Board thickness/.test(s)) && texts.some((s) => /mm$/.test(s)), "with the board's thickness and size in mm");
    await undo();
    t((await boardTables()).length === board0, "one Undo takes it away");
  });

  await scenario("board: Draw Table (two clicks, Table Properties) and the structure edits", async (t) => {
    await run("pcbnew.InteractiveDrawing.drawTable");
    await clickAt(50_000_000, 30_000_000);
    const [qx, qy] = screenOf(110_000, 52_000);
    fire("pointermove", qx, qy); await sleep(200);
    await clickAt(110_000_000, 52_000_000);
    const ok = buttonText(document, "OK");
    t(!!ok, "the second click opens Table Properties");
    ok?.click();
    await sleep(1300);
    let tables = await boardTables();
    t(tables.length === board0 + 1, `OK drew the table (${board0} -> ${tables.length})`);
    const tb = tables[tables.length - 1];
    if (!tb) return;
    t(tb.cols >= 2 && tb.rows >= 2, `a grid of ${tb.rows} x ${tb.cols} cells`);
    t(selectedIds().includes(tb.id), "the new table is selected");
    await __eda.run("common.Interactive.cancel");
    await sleep(300);

    // a click picks a cell, and the cell is a selectable item with its own kind
    const c00 = tb.cells.find((c) => c.row === 0 && c.col === 0);
    const [cx, cy] = center(c00);
    await clickAt(cx, cy);
    t(same(selectedIds(), [c00.id]) && __eda.state().selection[0].kind === "tablecell", `a click picks the cell (${selectedIds()})`);

    // Select Row(s), Select Column(s), Select Table
    await run("common.InteractiveSelection.Rows");
    t(selectedIds().length === tb.cols && selectedIds().every((id) => id.startsWith(`${tb.id}:0:`)), `Select Row(s) takes the ${tb.cols} cells of the row (${selectedIds().length})`);
    await select([c00.id]);
    await run("common.InteractiveSelection.SelectColumns");
    t(selectedIds().length === tb.rows && selectedIds().every((id) => id.endsWith(":0")), `Select Column(s) takes the ${tb.rows} cells of the column (${selectedIds().length})`);
    await run("common.InteractiveSelection.SelectTable");
    t(same(selectedIds(), [tb.id]), "Select Table takes the table");

    // Add Row Below / Column After, Delete Row(s) / Column(s)
    await select([c00.id]);
    await run("common.TableEditor.addRowBelow");
    tables = await boardTables();
    t(tableOf(tables, tb.id).rows === tb.rows + 1, `Add Row Below: ${tb.rows} -> ${tableOf(tables, tb.id).rows} rows`);
    await select([c00.id]);
    await run("common.TableEditor.addColAfter");
    tables = await boardTables();
    t(tableOf(tables, tb.id).cols === tb.cols + 1, `Add Column After: ${tb.cols} -> ${tableOf(tables, tb.id).cols} columns`);
    await select([`${tb.id}:1:1`]);
    await run("common.TableEditor.deleteRows");
    await select([`${tb.id}:0:1`]);
    await run("common.TableEditor.deleteColumns");
    tables = await boardTables();
    t(tableOf(tables, tb.id).rows === tb.rows && tableOf(tables, tb.id).cols === tb.cols, `Delete Row(s) and Column(s) bring it back to ${tb.rows} x ${tb.cols}`);

    // Merge / Unmerge Cells over a block of two cells, then Edit Table opens the dialog
    await select([`${tb.id}:0:0`, `${tb.id}:0:1`]);
    await run("common.TableEditor.mergeCells");
    tables = await boardTables();
    t(tableOf(tables, tb.id).cells.find((c) => c.row === 0 && c.col === 0).col_span === 2, "Merge Cells spans the first cell over two columns");
    await select([`${tb.id}:0:0`]);
    await run("common.TableEditor.unmergeCell");
    tables = await boardTables();
    t(tableOf(tables, tb.id).cells.find((c) => c.row === 0 && c.col === 0).col_span === 1, "Unmerge Cells gives them back");
    await select([`${tb.id}:0:0`]);
    await run("pcbnew.TableEditor.editTable");
    t(!!buttonText(document, "OK"), "Edit Table opens Table Properties");
    await closeDialogs();

    // Delete takes the whole table; Undo gives it back; and everything the scenario did is one undo step each
    await select([tb.id]);
    await run("common.Interactive.delete");
    t((await boardTables()).length === board0, "Delete takes the table away");
    await undo();
    t((await boardTables()).length === board0 + 1, "Undo gives it back");
    for (let i = 0; i < 14 && (await boardTables()).length > board0; i++) await undo();
    t((await boardTables()).length === board0, "and the Undos that follow take the drawing of it away");
  });

  // ---------------------------------------------------------------------------------------------------------------------- the sheet
  await goTab("Schematic");
  await scenario("sheet: Draw Table, the structure edits and the table's panel rows", async (t) => {
    await run("eeschema.InteractiveDrawing.drawTable");
    t(__eda.state().tool === "sch_table", "the Draw Table tool is armed");
    await clickAt(40_000_000, 150_000_000);
    const [qx, qy] = screenOf(100_000, 165_000);
    fire("pointermove", qx, qy); await sleep(250);
    await clickAt(100_000_000, 165_000_000);
    const ok = buttonText(document, "OK");
    t(!!ok, "the second click opens Table Properties");
    ok?.click();
    await sleep(1300);
    let tables = await sheetTables();
    t(tables.length === sheet0 + 1, `OK drew the table (${sheet0} -> ${tables.length})`);
    const tb = tables[tables.length - 1];
    if (!tb) return;
    t(tb.layer === "" && tb.cols >= 2 && tb.rows >= 2, `a grid of ${tb.rows} x ${tb.cols} cells on the sheet`);
    await __eda.run("common.Interactive.cancel"); // leave the tool
    await sleep(300);
    await select([`${tb.id}:1:1`]);
    await run("common.TableEditor.addRowAbove");
    tables = await sheetTables();
    t(tableOf(tables, tb.id).rows === tb.rows + 1, `Add Row Above: ${tb.rows} -> ${tableOf(tables, tb.id).rows} rows`);
    t(same(selectedIds(), [`${tb.id}:2:1`]), `the selected cell is still the one it was, a row lower (${selectedIds()})`);

    // the Properties panel: a table cell has the text box's rows and the table's; an edit is one command
    const panel = () => [...document.querySelectorAll(".properties-panel, [class*=properties]")].map((e) => e.textContent).join(" ");
    t(/Table Cell/.test(panel()) && /Column Width/.test(panel()) && /Background Fill/.test(panel()), "the Properties panel names a Table Cell and lists Column Width and Background Fill");
    await select([tb.id]);
    await sleep(300);
    t(/Border Width/.test(panel()) && /Row Separators/.test(panel()), "and for a table the Table Properties group");

    // a selected cell stands for its table in Move and Rotate (RequestSelection with aPromoteCellSelections)
    await select([`${tb.id}:0:0`]);
    await run("eeschema.InteractiveEdit.rotateCCW");
    let now = tableOf(await sheetTables(), tb.id);
    t(now.angle === 90_000, `R turns the table a quarter (angle ${now.angle})`);
    t(same(selectedIds(), [`${tb.id}:0:0`]), "and the cell stays selected");
    await run("eeschema.InteractiveEdit.rotateCCW");
    now = tableOf(await sheetTables(), tb.id);
    t(now.angle === 0, `R again lays it down (angle ${now.angle})`);

    // Lock is the table's
    await select([`${tb.id}:0:0`]);
    await run("eeschema.InteractiveEdit.lock");
    t((await sheet()).locked.includes(tb.id), "Lock locks the table the cell is in");
    await run("eeschema.InteractiveEdit.unlock");
    t(!(await sheet()).locked.includes(tb.id), "Unlock gives it back");

    // Delete on a cell clears its text; on a table it takes the table away
    await select([`${tb.id}:0:0`]);
    await run("eeschema.InteractiveEdit.properties");
    await closeDialogs();
    await select([tb.id]);
    await run("common.Interactive.delete");
    t((await sheetTables()).length === sheet0, "Delete takes the table away");
    for (let i = 0; i < 14 && (await sheetTables()).length > sheet0; i++) await undo();
    t((await sheetTables()).length === sheet0 && (await sheet()).tables.every((x) => x.id !== tb.id), "Undo takes the table out of the sheet again");
  });

  await scenario("sheet: the context menu of a cell offers the table edits, and Merge Cells only for a block", async (t) => {
    // a table to ask the menu on (made by the verb, as the dialog would)
    const draw = { op: "sch_table", verb: "draw", layer: "", at: { x: 40_000_000, y: 150_000_000 }, cols: 3, rows: 3, cell_w_nm: 20_320_000, cell_h_nm: 5_080_000, line_width_nm: 0 };
    const res = await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: draw, strict: false }) }).then((r) => r.json());
    await sleep(900);
    const tables = await sheetTables();
    const tb = tables[tables.length - 1];
    t(tables.length === sheet0 + 1 && tb, `a 3 x 3 table is on the sheet (${JSON.stringify(res).slice(0, 80)})`);
    if (!tb) return;
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    await select([`${tb.id}:1:1`]);
    const cell = tb.cells.find((c) => c.row === 1 && c.col === 1);
    const [x, y] = center(cell);
    await rclickAt(x, y);
    const labels = menuLabels();
    for (const want of ["Select Column(s)", "Select Row(s)", "Select Table", "Add Row Above", "Add Row Below", "Add Column Before", "Add Column After", "Delete Row(s)", "Delete Column(s)", "Edit Table...", "Export Table to CSV..."])
      t(labels.some((l) => l.startsWith(want.replace("...", ""))), `the menu offers ${want}`);
    t(!labels.some((l) => /^Merge Cells/.test(l)) && !labels.some((l) => /^Unmerge/.test(l)), "but neither Merge Cells nor Unmerge Cells for one plain cell");
    await closeMenu();
    await select([`${tb.id}:0:0`, `${tb.id}:0:1`]);
    // (a right click on a cell that is not selected selects it: ask the menu on one of the two)
    const [ax, ay] = center(tb.cells.find((c) => c.row === 0 && c.col === 0));
    await rclickAt(ax, ay);
    t(same(selectedIds().sort(), [`${tb.id}:0:0`, `${tb.id}:0:1`]), `the right click keeps the two cells selected (${selectedIds()})`);
    t(menuLabels().some((l) => /^Merge Cells/.test(l)), "two cells side by side are a block: Merge Cells is offered");
    await closeMenu();
    await select([tb.id]);
    await run("common.Interactive.delete");
    for (let i = 0; i < 6 && (await sheetTables()).length > sheet0; i++) await undo();
    t((await sheetTables()).length === sheet0, "the table is undone");
  });

  // ---------------------------------------------------------------------------------------------------------------------- the end
  await closeDialogs();
  await __eda.run("common.Interactive.cancel");
  await sleep(300);
  const end = { board: (await boardTables()).length, sheet: (await sheetTables()).length, errors: __eda.errors().slice(errors0).map((e) => e.message) };
  results.push({ name: "end: the board and the sheet have the tables they began with, and no console error came up", ok: end.board === board0 && end.sheet === sheet0 && end.errors.length === 0, notes: [`${end.board === board0 && end.sheet === sheet0 ? "ok" : "FAIL"}: board ${board0} -> ${end.board}, sheet ${sheet0} -> ${end.sheet}`, `${end.errors.length === 0 ? "ok" : "FAIL"}: console errors ${JSON.stringify(end.errors).slice(0, 200)}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
  return "started";
})();
