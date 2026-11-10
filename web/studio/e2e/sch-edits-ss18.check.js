// UI-LEFT-2 SS18 (PARITY-sch §81): the schematic editor's small edits, the visible ones --
//   Unfold from Bus lists a differential pair in a submenu of its own (`BUS_UNFOLD_MENU::update`'s `diff_busses`) and R turns the member's label while unfolding
//   (`doDrawSegments`: `m_busUnfold.label->Rotate90`);
//   Import Sheet's contents turn with R while they ride the cursor (`placeSheetContents` hands them to the move tool), in the import's one undo step;
//   Align takes a selected group as one item and moves it whole (`SCH_ALIGN_TOOL::GetSelections`, `SCH_GROUP::Move`);
//   Shift+click on a second cell selects the cells between it and the first (`SCH_SELECTION_TOOL::Main`, `m_previous_first_cell`);
//   Table Cell Properties' Font is KiCad's choice ("Default Font", "KiCad Font", the machine's fonts);
//   a label's `${REF:FIELD}` follows its symbol through a rename (`ConvertRefsToKIIDs` in the label dialog).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`),
// on a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schEditsSs18Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schEditsSs18Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schEditsSs18Sched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 7e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__schEditsSs18Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const sch = () => json("/api/schematic");
  const state = () => E.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 3 && state().tool !== "select"; i++) await escape(); };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const selectedIds = () => state().selection.map((s) => s.id);
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); };
  const clickAt = async (x, y, extra = {}) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py, extra); await sleep(250); fire("pointerdown", px, py, { buttons: 1, ...extra }); await sleep(60); fire("pointerup", px, py, extra); await sleep(900); };
  const dblClickAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    fire("pointermove", px, py); await sleep(200);
    fire("pointerdown", px, py, { buttons: 1 }); await sleep(40); fire("pointerup", px, py); await sleep(40);
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, button: 0, detail: 2 }));
    await sleep(1200);
  };
  const setValue = (el, v) => { const proto = Object.getPrototypeOf(el); Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; return cond; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    results.push({ name, ok, notes });
    for (const d of document.querySelectorAll(".dialog-backdrop")) d.click();
    await toSelect();
  };
  const mm = (v) => Math.round(v * 1e6);
  // A free part of the root sheet, far from the module sheets, on the 50 mil grid the tools snap to
  const X = mm(25.4), Y = mm(160.02);

  // the Schematic tab
  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);
  const start = await sch();
  const aliases0 = (await json("/api/sch/setup")).bus_aliases ?? [];

  // ------------------------------------------------------------------------------------------------------------------------ unfold
  await scenario("Unfold from Bus: a differential pair is a submenu of its own, and R turns the member's label while unfolding", async (t) => {
    let made = 0;
    try {
      if (!t((await cmd({ op: "set_bus_aliases", aliases: [...aliases0, { name: "E2E_PAIR", members: ["DP", "DN"] }] })).ok !== false, "an alias E2E_PAIR = {DP DN}")) return;
      made++;
      const w = await cmd({ op: "add_wire", pts: [{ x: X, y: Y }, { x: X + mm(25.4), y: Y }], bus: true });
      made++;
      const l = await cmd({ op: "add_label", net: "{E2E_PAIR CLK}", at: { x: X, y: Y }, kind: { scope: "local" } });
      made++;
      if (!t(w.ok !== false && l.ok !== false, "a bus labelled {E2E_PAIR CLK}")) return;
      await until(async () => (await sch()).wires.some((v) => v.bus && v.member_tree?.length === 2), 5000);
      await sleep(800);
      await hoverAt(X + mm(12.7), Y);
      await run("eeschema.InteractiveDrawingLineWireBus.unfoldBus");
      const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Unfold from Bus/.test(d.textContent ?? "")), 4000);
      if (!t(!!dlg, "the Unfold from Bus menu opens")) return;
      const pair = dlg.querySelector('details[data-unfold-submenu="E2E_PAIR"]');
      const diff = pair?.querySelector('details[data-unfold-submenu="D"]');
      const inDiff = diff ? [...diff.querySelectorAll("button[data-unfold-member]")].map((b) => b.dataset.unfoldMember) : [];
      t(!!pair && !!diff && same(inDiff, ["DP", "DN"]), `E2E_PAIR's members DP and DN sit in the submenu D (${JSON.stringify(inDiff)})`);
      t(!!dlg.querySelector('button[data-unfold-member="CLK"]') && !dlg.querySelector('details[data-unfold-submenu="CLK"]'), "CLK, a net of the group, is a plain item");
      dlg.querySelector('button[data-unfold-member="DP"]').click();
      await sleep(400);
      t(state().tool === "wire", `the wire tool is drawing from the bus entry (${state().tool})`);
      // R: the member's label turns a quarter (SPIN_STYLE::RIGHT -> UP)
      await run("eeschema.InteractiveEdit.rotateCCW");
      const before = await sch();
      await clickAt(X + mm(12.7) + mm(5.08), Y + mm(7.62));
      await dblClickAt(X + mm(12.7) + mm(5.08), Y + mm(17.78));
      const after = await until(async () => { const s = await sch(); return s.labels.length > before.labels.length ? s : null; }, 6000);
      const label = after?.labels.find((x) => x.net === "DP");
      t(!!label, `the wire, the entry and the label DP are on the sheet (${after?.labels.length ?? 0} labels)`);
      t(label?.spin === "up", `the label is turned up by R (${label?.spin})`);
      if (label) made++;
      await toSelect();
    } finally {
      for (let i = 0; i < made; i++) await undo();
    }
    const back = await sch();
    t(back.wires.length === start.wires.length && back.labels.length === start.labels.length && same((await json("/api/sch/setup")).bus_aliases ?? [], aliases0), "undone: the bus, its label, the unfold and the alias");
  });

  // ------------------------------------------------------------------------------------------------------------------------ import sheet
  await scenario("Import Sheet: R turns the contents while they ride the cursor, and the click places them turned, one undo step", async (t) => {
    await run("eeschema.InteractiveDrawing.importSheet");
    const chooser = await until(() => [...document.querySelectorAll(".dialog, [role=dialog]")].find((d) => /Choose Schematic/.test(d.textContent ?? "")), 4000);
    const asSheet = chooser && [...chooser.querySelectorAll("label")].find((l) => /Place as sheet/.test(l.textContent ?? ""))?.querySelector("input");
    if (asSheet?.checked) asSheet.click();
    await sleep(200);
    const close = chooser && [...chooser.querySelectorAll("button")].find((x) => /Cancel|Close|×/.test(x.textContent.trim()));
    if (close) close.click(); else await escape();
    await sleep(300);
    // a horizontal wire with a label, as KiCad writes one
    const file =
      `(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "9e1f5a10-0000-4000-8000-00000000f001") (paper "A4") (lib_symbols) ` +
      `(wire (pts (xy 20.32 20.32) (xy 40.64 20.32)) (stroke (width 0) (type default)) (uuid "9e1f5a10-0000-4000-8000-00000000f002")) ` +
      `(label "TURNED" (at 40.64 20.32 0) (fields_autoplaced yes) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "9e1f5a10-0000-4000-8000-00000000f003")) ` +
      `(sheet_instances (path "/" (page "1"))))`;
    const before = await sch();
    const dt = new DataTransfer();
    dt.items.add(new File([file], "turned_copy.kicad_sch", { type: "text/plain" }));
    const mk = (type) => new DragEvent(type, { bubbles: true, cancelable: true, composed: true, dataTransfer: dt });
    document.body.dispatchEvent(mk("dragenter"));
    document.body.dispatchEvent(mk("dragover"));
    await sleep(150);
    document.body.dispatchEvent(mk("drop"));
    await until(() => state().tool === "sch_import_sheet", 8000);
    if (!t(state().tool === "sch_import_sheet", `the file is in the hand (${state().tool})`)) return;
    await hoverAt(X + mm(150), Y);
    t(!!(await until(() => Number(container().dataset.importGhost ?? 0) > 0, 15000)), "the contents ride the cursor");
    await run("eeschema.InteractiveEdit.rotateCCW");
    await sleep(1500);
    t(Number(container().dataset.importGhost ?? 0) > 0, "and still ride after R");
    await clickAt(X + mm(150), Y);
    const placed = await until(async () => { const s = await sch(); return s.labels.some((l) => l.net === "TURNED") ? s : null; }, 15000);
    const wire = placed?.wires.find((w) => !before.wires.some((b) => b.id === w.id) && !w.bus);
    t(!!wire, "the click places the wire and the label");
    t(!!wire && wire.pts.length === 2 && wire.pts[0][0] === wire.pts[1][0] && Math.abs(wire.pts[0][1] - wire.pts[1][1]) === mm(20.32), `the wire comes in turned a quarter: vertical (${JSON.stringify(wire?.pts)})`);
    await toSelect();
    await undo();
    const back = await sch();
    t(back.wires.length === before.wires.length && !back.labels.some((l) => l.net === "TURNED"), "one undo takes the import away");
  });

  // ------------------------------------------------------------------------------------------------------------------------ align
  await scenario("Align: a selected group is one item and moves whole", async (t) => {
    const at = (s, id) => s.texts.find((x) => x.id === id)?.at;
    const texts0 = (await sch()).texts.map((x) => x.id);
    let made = 0;
    try {
      for (const [dx, dy, s] of [[mm(30.48), mm(20.32), "E2E_G1"], [mm(40.64), mm(25.4), "E2E_G2"], [0, mm(30.48), "E2E_LONE"]]) {
        await cmd({ op: "add_sch_text", content: s, at: { x: X + dx, y: Y + dy }, angle_millideg: 0, size_nm: 1_270_000 });
        made++;
      }
      let s = await sch();
      const ids = Object.fromEntries(s.texts.filter((x) => !texts0.includes(x.id)).map((x) => [x.content, x.id]));
      await cmd({ op: "sch_edit", verb: "group", ids: [ids.E2E_G1, ids.E2E_G2] });
      made++;
      s = await sch();
      const group = (s.groups ?? []).find((g) => g.member_ids.includes(ids.E2E_G1));
      if (!t(!!group && group.member_ids.length === 2, "the two texts are a group")) return;
      await hoverAt(X + mm(200), Y + mm(200));
      await select([group.id, ids.E2E_LONE]);
      t(same([...selectedIds()].sort(), [group.id, ids.E2E_LONE].sort()), `the group and the lone text are selected (${selectedIds()})`);
      const g1 = at(s, ids.E2E_G1), g2 = at(s, ids.E2E_G2), lone = at(s, ids.E2E_LONE);
      await run("eeschema.Align.alignLeft");
      const after = await until(async () => { const n = await sch(); return !same(at(n, ids.E2E_G1), g1) ? n : null; }, 5000);
      const n1 = after && at(after, ids.E2E_G1), n2 = after && at(after, ids.E2E_G2);
      t(!!after && n1[0] - g1[0] === n2[0] - g2[0] && n1[1] - g1[1] === n2[1] - g2[1] && n1[0] < g1[0], `both members moved left by one offset (${n1?.[0] - g1[0]}, ${n2?.[0] - g2[0]})`);
      t(!!after && same(at(after, ids.E2E_LONE), lone), "the lone text, the leftmost, stays");
      if (after) made++;
    } finally {
      for (let i = 0; i < made; i++) await undo();
    }
    t((await sch()).texts.length === start.texts.length, "undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ tables
  const sheetTables = async () => (await sch()).tables ?? [];
  const sheetTable = async (x, y, rows, cols) => {
    const before = (await sheetTables()).map((t) => t.id);
    const texts = Array.from({ length: rows }, (_, r) => Array.from({ length: cols }, (_, c) => `r${r}c${c}`));
    await cmd({ op: "sch_table", verb: "draw", layer: "", at: { x: x * 1e6, y: y * 1e6 }, cols, rows, cell_w_nm: 20_320_000, cell_h_nm: 5_080_000, line_width_nm: 0, patch: { texts } });
    return (await sheetTables()).find((t) => !before.includes(t.id));
  };
  const center = (cell) => [cell.corners.reduce((a, p) => a + p[0], 0) / 4, cell.corners.reduce((a, p) => a + p[1], 0) / 4];
  const cellOf = (tb, row, col) => tb.cells.find((c) => c.row === row && c.col === col);
  const tables0 = (await sheetTables()).length;

  await scenario("Shift+click on a second cell selects the cells between it and the first", async (t) => {
    const tb = await sheetTable(40, 150, 3, 3);
    if (!t(!!tb, "a 3 x 3 table")) return;
    try {
      // the page has the table once its schematic is read again
      await sleep(1500);
      await E.run("common.Control.zoomFitScreen");
      await sleep(600);
      await select([]);
      await hoverAt(...center(cellOf(tb, 0, 0)));
      await sleep(600);
      await clickAt(...center(cellOf(tb, 0, 0)));
      await until(() => selectedIds().length > 0, 3000);
      t(same(selectedIds(), [`${tb.id}:0:0`]), `a click selects one cell (${selectedIds()})`);
      await clickAt(...center(cellOf(tb, 2, 1)), { shiftKey: true });
      const want = ["0:0", "0:1", "1:0", "1:1", "2:0", "2:1"].map((a) => `${tb.id}:${a}`);
      t(same([...selectedIds()].sort(), want.sort()), `Shift+click on (2,1) selects the six cells between (${selectedIds()})`);
      await clickAt(...center(cellOf(tb, 0, 2)), { shiftKey: true });
      t(same([...selectedIds()].sort(), ["0:0", "0:1", "0:2"].map((a) => `${tb.id}:${a}`).sort()), `another Shift+click starts from the same first cell (${selectedIds()})`);
    } finally {
      for (let i = 0; i < 4 && (await sheetTables()).length !== tables0; i++) await undo();
    }
  });

  await scenario("Table Cell Properties: the Font is KiCad's choice, and OK sets it", async (t) => {
    const tb = await sheetTable(40, 150, 1, 2);
    if (!t(!!tb, "a table")) return;
    try {
      await sleep(1500);
      await select([`${tb.id}:0:0`]);
      await run("eeschema.InteractiveEdit.properties");
      const dlg = await until(() => document.querySelector("[data-testid=table-cell-properties]"), 4000);
      const font = dlg?.querySelector("[data-testid=cell-face]");
      const options = font ? [...font.querySelectorAll("option")].map((o) => o.value) : [];
      t(font?.tagName === "SELECT" && options[0] === "Default Font" && options[1] === "KiCad Font", `a choice of Default Font, KiCad Font, ... (${options.slice(0, 3).join(", ")})`);
      t(font?.value === "Default Font", `the cell's font selected (${font?.value})`);
      if (!font) return;
      setValue(font, "KiCad Font");
      await sleep(200);
      dlg.querySelector("[data-testid=cell-ok]").click();
      await sleep(1200);
      const c = cellOf((await sheetTables()).find((x) => x.id === tb.id), 0, 0);
      t(c.face === "KiCad Font", `the cell's font is KiCad Font (${c.face})`);
    } finally {
      for (let i = 0; i < 4 && (await sheetTables()).length !== tables0; i++) await undo();
    }
  });

  // ------------------------------------------------------------------------------------------------------------------------ cross-references
  await scenario("A label's ${REF:FIELD} follows its symbol through a rename", async (t) => {
    const labels0 = (await sch()).labels.map((l) => l.id);
    await cmd({ op: "add_label", net: "N_${J1:VALUE}", at: { x: X, y: Y + mm(50.8) }, kind: { scope: "local" } });
    const label = (await sch()).labels.find((l) => !labels0.includes(l.id));
    if (!t(!!label, "a label N_${J1:VALUE}")) return;
    const header = (await sch()).sheets.find((s) => /Header/.test(s.name));
    const res = await cmd({ op: "on_sheet", sheet: header.id, cmd: { op: "rename_symbol", id: "J1", new_id: "J77" } });
    const now = (await sch()).labels.find((l) => l.id === label.id || (l.at[0] === label.at[0] && l.at[1] === label.at[1]));
    t(res.ok !== false && now?.net === "N_${J77:VALUE}", `after J1 is renamed J77 the label reads ${now?.net}`);
    await undo();
    const back = (await sch()).labels.find((l) => l.at[0] === label.at[0] && l.at[1] === label.at[1]);
    t(back?.net === "N_${J1:VALUE}", `Undo gives J1 back (${back?.net})`);
    await undo();
    t((await sch()).labels.length === start.labels.length, "undone");
  });

  // the sheet is as it began
  const end = await sch();
  results.push({ name: "the sheet is as it began", ok: end.symbols.length === start.symbols.length && end.wires.length === start.wires.length && end.labels.length === start.labels.length && end.texts.length === start.texts.length && (end.tables ?? []).length === (start.tables ?? []).length && (end.groups ?? []).length === (start.groups ?? []).length, notes: [`wires ${start.wires.length} -> ${end.wires.length}, labels ${start.labels.length} -> ${end.labels.length}, texts ${start.texts.length} -> ${end.texts.length}, tables ${(start.tables ?? []).length} -> ${(end.tables ?? []).length}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
