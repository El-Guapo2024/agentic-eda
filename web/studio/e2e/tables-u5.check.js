// Tables, round u5 (UI-LEFT P16): in both editors, Copy and Paste of a table and of a block of cells (the clipboard of `CLIPBOARD_IO::SaveSelection` / `SCH_IO_KICAD_SEXPR::Format`),
// Paste into selected cells ("Paste Cells", `pasteCellsIntoSelection`) and KiCad's refusal of a selection that is no block, a drag across cells that selects the block under it
// (`selectTableCells`, `SelectCellsBetween`), `@{..}` expressions in cells shown evaluated; on the board Flip (KiCad 25ec2335 `PCB_TABLE::Flip`); on the sheet Duplicate of a table,
// the Table Cell Properties dialog's Text color and Background fill, and a `${REF:FIELD}` in a cell that follows the symbol through a rename.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), on a COPY
// of a board: every scenario undoes what it did, and the last check says the board and the sheet have the tables they began with.
// `window.__tablesU5Only = /regexp/` runs only the scenarios whose names match.
//
// It answers through `window.__tablesU5Check`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with `await __tablesU5Check.wait(30000)`
// until `.done`. It works through `window.__eda` (actions by id, `common.InteractiveSelection.selectItems`, Undo), the dialogs' controls, pointer events on the canvas, and
// `/api/cmd` for the tables it starts from; what the server holds is read back from `GET /api/state` (`drawings.tables`) and `GET /api/schematic` (`tables`).
// run-all-checks: board=mcu30 minutes=12   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__tablesU5Check = { results, done: false, ok: false, wait: null });
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
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const selectedIds = () => __eda.state().selection.map((x) => x.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const run = async (id, arg) => { const r = await __eda.run(id, arg); await sleep(900); return r; };
  const cmd = async (c) => { const r = await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((x) => x.json()); await sleep(900); return r; };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(200); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  /** A press at one point of the editor, a drag through the screen to another, and the release there (`selectTableCells` needs a real drag: more than 8 px). */
  const dragFromTo = async (a, b, extra = {}) => {
    const [ax, ay] = screenOf(a[0], a[1]);
    const [bx, by] = screenOf(b[0], b[1]);
    fire("pointermove", ax, ay, extra); await sleep(120);
    fire("pointerdown", ax, ay, { buttons: 1, ...extra }); await sleep(60);
    for (let k = 1; k <= 6; k++) { fire("pointermove", ax + ((bx - ax) * k) / 6, ay + ((by - ay) * k) / 6, { buttons: 1, ...extra }); await sleep(60); }
    fire("pointerup", bx, by, extra); await sleep(500);
  };
  const center = (cell) => [cell.corners.reduce((a, p) => a + p[0], 0) / 4, cell.corners.reduce((a, p) => a + p[1], 0) / 4];
  const cellOf = (t, row, col) => t.cells.find((c) => c.row === row && c.col === col);
  const shownOf = (c) => c.shown ?? c.text;
  const goTab = async (name) => {
    if (__eda.state().tab === name.toLowerCase()) return;
    buttonText(document, name)?.click();
    await sleep(1500);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
  };
  const escape = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(900); };
  const tableOf = (tables, id) => tables.find((t) => t.id === id);
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const only = window.__tablesU5Only;
  const setValue = (el, v) => { const proto = Object.getPrototypeOf(el); Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };

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
  if (__eda.state().tab !== "pcb") await goTab("PCB");
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);
  const board0 = (await boardTables()).length;
  const sheet0 = (await sheetTables()).length;
  const errors0 = Date.now();
  /** The messages KiCad shows for a refused paste into cells: the scenarios that ask for one expect it as an error toast. */
  const expected = [/Selected cells must form a contiguous block/];

  /** A board table of `rows` x `cols` 12 x 4 mm cells at (x, y) mm on `layer`, each cell's text its address in the grid (`r0c1`), drawn by the verb Draw Table's OK sends. */
  const boardTable = async (x, y, rows, cols, layer = "F.SilkS") => {
    const before = (await boardTables()).map((t) => t.id);
    const texts = Array.from({ length: rows }, (_, r) => Array.from({ length: cols }, (_, c) => `r${r}c${c}`));
    await cmd({ op: "table", verb: "draw", layer, at: { x: x * 1e6, y: y * 1e6 }, cols, rows, cell_w_nm: 12_000_000, cell_h_nm: 4_000_000, line_width_nm: 120_000, patch: { texts } });
    return (await boardTables()).find((t) => !before.includes(t.id));
  };
  const sheetTable = async (x, y, rows, cols, texts) => {
    const before = (await sheetTables()).map((t) => t.id);
    texts = texts ?? Array.from({ length: rows }, (_, r) => Array.from({ length: cols }, (_, c) => `r${r}c${c}`));
    await cmd({ op: "sch_table", verb: "draw", layer: "", at: { x: x * 1e6, y: y * 1e6 }, cols, rows, cell_w_nm: 20_320_000, cell_h_nm: 5_080_000, line_width_nm: 0, patch: { texts } });
    return (await sheetTables()).find((t) => !before.includes(t.id));
  };
  const undoUntil = async (count, want) => { for (let i = 0; i < 16 && (await count()) !== want; i++) await undo(); };

  // ---------------------------------------------------------------------------------------------------------------------- the board
  await scenario("board: Copy and Paste of a table, and of a block of its cells", async (t) => {
    const tb = await boardTable(40, 30, 3, 3);
    t(!!tb, "a 3 x 3 table to copy");
    if (!tb) return;
    const ids0 = (await boardTables()).map((x) => x.id);
    await select([tb.id]);
    await run("common.Interactive.copy");
    await run("common.Interactive.paste");
    let tables = await boardTables();
    const copy = tables.find((x) => !ids0.includes(x.id));
    t(tables.length === board0 + 2 && copy && same(copy.cells.map((c) => c.text), tb.cells.map((c) => c.text)), `Paste adds a copy of the table with its cells (${tables.length - board0} tables)`);
    t(copy && same(selectedIds(), [copy.id]) && __eda.state().tool === "move", `the copy is selected and carried (${selectedIds()}, ${__eda.state().tool})`);
    await escape();
    t((await boardTables()).length === board0 + 1, "Escape takes the carried copy away");
    // two cells side by side: a table of those two (pasted with no cell selected: with cells selected a Paste pastes into them, below)
    await select([`${tb.id}:1:1`, `${tb.id}:1:2`]);
    await run("common.Interactive.copy");
    await select([]);
    await run("common.Interactive.paste");
    tables = await boardTables();
    const block = tables[tables.length - 1];
    t(tables.length === board0 + 2 && block.rows === 1 && block.cols === 2 && same(block.cells.map((c) => c.text), ["r1c1", "r1c2"]), `the copied cells paste as a 1 x 2 table of them (${block.rows} x ${block.cols}: ${block.cells.map((c) => c.text)})`);
    await escape();
    await undoUntil(async () => (await boardTables()).length, board0);
    t((await boardTables()).length === board0, "undone");
  });

  await scenario("board: Paste with cells selected pastes into them, and KiCad refuses cells that are no block", async (t) => {
    const a = await boardTable(40, 30, 2, 2);
    const b = await boardTable(40, 60, 3, 3);
    if (!a || !b) return t(false, "two tables");
    await select([`${a.id}:0:0`, `${a.id}:0:1`, `${a.id}:1:0`, `${a.id}:1:1`]);
    await run("common.Interactive.copy");
    await select([`${b.id}:1:1`]);
    await run("common.Interactive.paste");
    let now = tableOf(await boardTables(), b.id);
    t(same([0, 1, 2].map((r) => [0, 1, 2].map((c) => cellOf(now, r, c).text)), [["r0c0", "r0c1", "r0c2"], ["r1c0", "r0c0", "r0c1"], ["r2c0", "r1c0", "r1c1"]]), `the 2 x 2 block lands from the selected cell on (${now.cells.map((c) => c.text)})`);
    t((await boardTables()).length === board0 + 2 && __eda.state().tool !== "move", "it adds no table and carries nothing");
    // a selection that runs past the table grows it
    await select([`${b.id}:2:2`]);
    await run("common.Interactive.paste");
    now = tableOf(await boardTables(), b.id);
    t(now.rows === 4 && now.cols === 4, `past the last row and column the table grows (${now.rows} x ${now.cols})`);
    // two cells that are no block: KiCad's words, and nothing pasted
    await select([`${b.id}:0:0`, `${b.id}:2:2`]);
    const r = await run("common.Interactive.paste");
    t(/Selected cells must form a contiguous block/.test(r.toast ?? "") || __eda.errors(errors0).some((e) => /contiguous block/.test(e.message)), `refused with KiCad's message (${r.toast})`);
    t(same(tableOf(await boardTables(), b.id).cells.map((c) => c.text), now.cells.map((c) => c.text)), "and the table is as it was");
    await undoUntil(async () => (await boardTables()).length, board0);
    t((await boardTables()).length === board0, "undone");
  });

  await scenario("board: a drag across cells selects the block under it", async (t) => {
    const tb = await boardTable(40, 30, 3, 3);
    if (!tb) return t(false, "a table");
    await select([]);
    await dragFromTo(center(cellOf(tb, 0, 0)), center(cellOf(tb, 1, 1)));
    t(same([...selectedIds()].sort(), [`${tb.id}:0:0`, `${tb.id}:0:1`, `${tb.id}:1:0`, `${tb.id}:1:1`].sort()), `the four cells the drag touched (${selectedIds()})`);
    t((await boardTables()).find((x) => x.id === tb.id).x === tb.x, "and the table did not move");
    // Shift adds the cells of another drag to them
    await dragFromTo(center(cellOf(tb, 2, 2)), center(cellOf(tb, 2, 2)).map((v, i) => v + (i === 0 ? 1_000_000 : 500_000)), { shiftKey: true });
    t(selectedIds().length === 5 && selectedIds().includes(`${tb.id}:2:2`), `Shift adds (${selectedIds()})`);
    // a press and release on a cell is still the click that picks it
    const [px, py] = screenOf(...center(cellOf(tb, 2, 0)));
    fire("pointermove", px, py); await sleep(100); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(400);
    t(same(selectedIds(), [`${tb.id}:2:0`]), `a click picks one cell (${selectedIds()})`);
    await undoUntil(async () => (await boardTables()).length, board0);
  });

  await scenario("board: Flip turns a table over as KiCad 25ec2335 does", async (t) => {
    const tb = await boardTable(40, 30, 2, 3);
    if (!tb) return t(false, "a table");
    await select([tb.id]);
    await run("pcbnew.InteractiveEdit.flip");
    let now = tableOf(await boardTables(), tb.id);
    t(now.layer === "B.SilkS" && same(now.cells.filter((c) => c.row === 0).map((c) => c.text), ["r0c2", "r0c1", "r0c0"]) && now.cells.every((c) => c.mirror), `F: on the back, its columns the other way and its text mirrored (${now.layer}, ${now.cells.filter((c) => c.row === 0).map((c) => c.text)})`);
    const w = tb.bbox[2] - tb.bbox[0];
    t(Math.abs(now.bbox[2] - now.bbox[0] - w) < 2, "the same width, nothing moved beside the mirror image");
    await undo();
    // top to bottom: half round, the box where the mirror image's is
    const mid = (tb.bbox[1] + tb.bbox[3]) / 2;
    await cmd({ op: "flip_items", ids: [tb.id], pivot: { x: Math.round((tb.bbox[0] + tb.bbox[2]) / 2), y: Math.round(mid) }, direction: "top_bottom" });
    now = tableOf(await boardTables(), tb.id);
    t(now.angle === 180_000 && now.layer === "B.SilkS", `top to bottom it is at 180 degrees on the back (${now.angle}, ${now.layer})`);
    t(same(now.bbox.map((v) => Math.round(v / 1000)), tb.bbox.map((v) => Math.round(v / 1000))), `about its own middle its box stays (${now.bbox} against ${tb.bbox})`);
    await undoUntil(async () => (await boardTables()).length, board0);
    t((await boardTables()).length === board0, "undone");
  });

  await scenario("board: an expression in a cell is shown evaluated", async (t) => {
    const tb = await boardTable(40, 30, 2, 2);
    if (!tb) return t(false, "a table");
    await cmd({ op: "table", verb: "edit_cells", id: tb.id, cells: [[1, 1]], edit: { text: "@{${ROW}*10 + 1} @{upper(\"ok\")}" } });
    const c = cellOf(tableOf(await boardTables(), tb.id), 1, 1);
    t(shownOf(c) === "21 OK", `the cell shows ${JSON.stringify(shownOf(c))} for ${JSON.stringify(c.text)}`);
    await undoUntil(async () => (await boardTables()).length, board0);
  });

  // ---------------------------------------------------------------------------------------------------------------------- the sheet
  await goTab("Schematic");
  await scenario("sheet: Copy, Paste and Duplicate of a table", async (t) => {
    const tb = await sheetTable(40, 150, 2, 2);
    if (!tb) return t(false, "a table");
    await __eda.run("common.Control.zoomFitScreen"); await sleep(400);
    const ids0 = (await sheetTables()).map((x) => x.id);
    await select([tb.id]);
    await run("common.Interactive.copy");
    await run("common.Interactive.paste");
    await sleep(800);
    const [qx, qy] = screenOf(tb.x + 60_000_000, tb.y);
    fire("pointermove", qx, qy); await sleep(900);
    fire("pointerdown", qx, qy, { buttons: 1 }); await sleep(80); fire("pointerup", qx, qy); await sleep(1800);
    let tables = await sheetTables();
    const copy = tables.find((x) => !ids0.includes(x.id));
    t(tables.length === sheet0 + 2 && copy && same(copy.cells.map((c) => c.text), tb.cells.map((c) => c.text)), `Paste puts a copy of the table on the sheet (${tables.length - sheet0} tables)`);
    t(copy && selectedIds().includes(copy.id), `and selects it (${selectedIds()})`);
    // Duplicate (Ctrl+D): copied and carried at once
    await select([tb.id]);
    await run("common.Interactive.duplicate");
    await sleep(800);
    const [rx, ry] = screenOf(tb.x, tb.y + 30_000_000);
    fire("pointermove", rx, ry); await sleep(900);
    fire("pointerdown", rx, ry, { buttons: 1 }); await sleep(80); fire("pointerup", rx, ry); await sleep(1800);
    tables = await sheetTables();
    t(tables.length === sheet0 + 3, `Duplicate makes another (${tables.length - sheet0} tables)`);
    await undoUntil(async () => (await sheetTables()).length, sheet0);
    t((await sheetTables()).length === sheet0, "undone");
  });

  await scenario("sheet: copied cells pasted into selected cells, and a drag across cells selects them", async (t) => {
    const tb = await sheetTable(40, 150, 3, 3);
    if (!tb) return t(false, "a table");
    await __eda.run("common.Control.zoomFitScreen"); await sleep(400);
    await select([`${tb.id}:0:0`, `${tb.id}:0:1`]);
    await run("common.Interactive.copy");
    await select([`${tb.id}:2:1`]);
    await run("common.Interactive.paste");
    let now = tableOf(await sheetTables(), tb.id);
    t(cellOf(now, 2, 1).text === "r0c0" && cellOf(now, 2, 2).text === "r0c1" && (await sheetTables()).length === sheet0 + 1, `Paste Cells: (2,1) and (2,2) take the copied texts (${cellOf(now, 2, 1).text}, ${cellOf(now, 2, 2).text})`);
    // the drag
    await select([]);
    await dragFromTo(center(cellOf(now, 0, 1)), center(cellOf(now, 1, 2)));
    t(same([...selectedIds()].sort(), [`${tb.id}:0:1`, `${tb.id}:0:2`, `${tb.id}:1:1`, `${tb.id}:1:2`].sort()), `a drag selects the four cells under it (${selectedIds()})`);
    t(tableOf(await sheetTables(), tb.id).x === tb.x, "and does not move the table");
    await undoUntil(async () => (await sheetTables()).length, sheet0);
  });

  await scenario("sheet: Table Cell Properties sets the text colour and the background fill", async (t) => {
    const tb = await sheetTable(40, 150, 2, 2);
    if (!tb) return t(false, "a table");
    await select([`${tb.id}:0:0`]);
    await run("eeschema.InteractiveEdit.properties");
    const dlg = document.querySelector("[data-testid=table-cell-properties]");
    t(!!dlg && /Text color/.test(dlg.textContent) && /Background fill/.test(dlg.textContent) && !!dlg.querySelector("[data-testid=cell-margin-left]"), "the sheet's dialog has Text color, Background fill and the margins");
    if (!dlg) return;
    const textDefault = dlg.querySelector('[data-testid=cell-text-color] input[type=checkbox]');
    textDefault.click(); await sleep(150);
    setValue(dlg.querySelector('[data-testid=cell-text-color] input[type=color]'), "#ff0000"); await sleep(150);
    dlg.querySelector('[data-testid=cell-fill-color] input[type=checkbox]').click(); await sleep(150);
    setValue(dlg.querySelector('[data-testid=cell-fill-color] input[type=color]'), "#00ff00"); await sleep(150);
    dlg.querySelector("[data-testid=cell-ok]").click();
    await sleep(1200);
    const c = cellOf(tableOf(await sheetTables(), tb.id), 0, 0);
    t(same(c.color, [255, 0, 0, 255]), `the text is red (${JSON.stringify(c.color)})`);
    t(c.fill === "color" && same(c.fill_color, [0, 255, 0, 255]), `the cell is filled green (${c.fill} ${JSON.stringify(c.fill_color)})`);
    // two cells that differ: mixed values, left as they are
    await select([`${tb.id}:0:0`, `${tb.id}:0:1`]);
    await run("eeschema.InteractiveEdit.properties");
    const dlg2 = document.querySelector("[data-testid=table-cell-properties]");
    t(!!dlg2?.querySelector("[data-testid=cell-text-color-popup]"), "for two cells that differ the colour reads -- mixed values --");
    dlg2?.querySelector("[data-testid=cell-ok]")?.click();
    await sleep(1200);
    t(same(cellOf(tableOf(await sheetTables(), tb.id), 0, 0).color, [255, 0, 0, 255]), "and OK leaves each cell's colour");
    await undoUntil(async () => (await sheetTables()).length, sheet0);
  });

  await scenario("sheet: an expression is shown evaluated, and a ${REF:FIELD} follows its symbol through a rename", async (t) => {
    const tb = await sheetTable(40, 150, 1, 2, [["@{${COL} + 10}", "${J1:VALUE}"]]);
    if (!tb) return t(false, "a table");
    let now = tableOf(await sheetTables(), tb.id);
    t(shownOf(cellOf(now, 0, 0)) === "10", `@{\${COL} + 10} shows ${JSON.stringify(shownOf(cellOf(now, 0, 0)))}`);
    t(shownOf(cellOf(now, 0, 1)) === "HDR", `\${J1:VALUE} shows the header's value (${shownOf(cellOf(now, 0, 1))})`);
    const header = (await json("/api/schematic")).sheets.find((s) => /Header/.test(s.name));
    const res = await cmd({ op: "on_sheet", sheet: header.id, cmd: { op: "rename_symbol", id: "J1", new_id: "J7" } });
    now = tableOf(await sheetTables(), tb.id);
    t(res.ok && cellOf(now, 0, 1).text === "${J7:VALUE}" && shownOf(cellOf(now, 0, 1)) === "HDR", `after J1 is renamed J7 the cell reads ${cellOf(now, 0, 1).text} and still shows ${shownOf(cellOf(now, 0, 1))}`);
    await undo();
    now = tableOf(await sheetTables(), tb.id);
    t(cellOf(now, 0, 1).text === "${J1:VALUE}", `Undo gives J1 back to the cell (${cellOf(now, 0, 1).text})`);
    await undoUntil(async () => (await sheetTables()).length, sheet0);
  });

  // ---------------------------------------------------------------------------------------------------------------------- the end
  await closeDialogs();
  await __eda.run("common.Interactive.cancel");
  await sleep(300);
  const errs = __eda.errors(errors0).map((e) => e.message).filter((m) => !expected.some((re) => re.test(m)));
  const end = { board: (await boardTables()).length, sheet: (await sheetTables()).length };
  results.push({ name: "end: the board and the sheet have the tables they began with, and no other error came up", ok: end.board === board0 && end.sheet === sheet0 && errs.length === 0, notes: [`${end.board === board0 && end.sheet === sheet0 ? "ok" : "FAIL"}: board ${board0} -> ${end.board}, sheet ${sheet0} -> ${end.sheet}`, `${errs.length === 0 ? "ok" : "FAIL"}: errors ${JSON.stringify(errs).slice(0, 300)}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
  return "started";
})();
