// UI-LEFT-2 SS6 and SS7 (PARITY-sch §75): Symbol Properties' Fields grid has `FIELDS_GRID_TABLE`'s schematic columns, Name .. Bold shown at open and the others from
// the column labels' right-click menu (`GRID_TRICKS::onGridLabelRightClick`); X / Y Position, Orientation and Allow Autoplacement set a field in the symbol's own
// frame, and OK places an autoplaced symbol's fields again as `SCH_EDIT_TOOL::Properties` does; Move Up / Move Down order the user fields; Angle and Mirror
// (`SetOrientation`); a power symbol's dialog has Unit, Body style, Angle and Mirror, and its shown user field is drawn and picked on the sheet; a sheet read from a
// KiCad file has its symbols' fields as items -- V opens Field Properties -- and Symbol Properties offers the project's field-name templates.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with KiCad's libraries (`KICAD_PATH`), on a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schSymPropsV3Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__schSymPropsV3Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schSymPropsV3Sched) {
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
    window.__schSymPropsV3Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = () => json("/api/schematic");
  const run = async (id, args) => { const r = await __eda.run(id, args); await sleep(250); return r; };
  const state = () => __eda.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(150); } };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 8000);
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(900); };
  const fitTo = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(600); };
  /** Symbol Properties of `id`, opened with E. */
  const props = async (id, title = /^Symbol Properties/) => { await select([id]); await run("eeschema.InteractiveEdit.properties"); return opened(title); };
  /** The column labels' right-click menu of the grid in `dlg` (`data-testid` `grid`), and the click on the column `label`'s item. */
  const columnMenuItems = async (dlg, grid) => {
    dlg.querySelector(`[data-testid=${grid}] thead`)?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 200, button: 2 }));
    await sleep(250);
    return [...document.querySelectorAll(".menu-node-item")];
  };
  const label = (item) => item.textContent.replace(/[✓ ]/g, "").trim();
  const showColumns = async (dlg, grid, labels) => {
    for (const l of labels) {
      const items = await columnMenuItems(dlg, grid);
      items.find((i) => label(i) === l)?.click();
      await sleep(200);
    }
  };
  const heads = (dlg, grid) => [...(dlg.querySelector(`[data-testid=${grid}] thead`)?.querySelectorAll("th") ?? [])].map((h) => h.textContent.trim());
  const fieldOf = async (ref, name) => (await sch()).symbols.find((s) => s.id === ref)?.fields?.find((f) => f.name === name);

  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror, (x.fields ?? []).map((f) => [f.name, f.at, f.visible, f.local ?? null])]), power: s.power_symbols.map((p) => [p.id, p.rot, p.props ?? null, (p.fields ?? []).map((f) => f.name)]), sheets: s.sheets.length });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
      try { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 100 * G;
  const R = "Device:R";
  const placeR = async (id = "R90", x = X0, y = Y0) => {
    const put = await cmd({ op: "add_symbol", id, lib_id: R, at: { x, y }, rot_millideg: 0, value: "", footprint: "", unit: 1 });
    await sleep(1200);
    return put;
  };

  // A KiCad schematic with a resistor whose Value was placed by hand and a ground symbol with a shown user field.
  const KICAD_FILE = `(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0")
	(uuid "11111111-1111-1111-1111-111111111111")
	(paper "A4")
	(lib_symbols
		(symbol "Device:R" (pin_numbers (hide yes)) (pin_names (offset 0)) (exclude_from_sim no) (in_bom yes) (on_board yes)
			(property "Reference" "R" (at 2.032 0 90) (effects (font (size 1.27 1.27))))
			(property "Value" "R" (at 0 0 90) (effects (font (size 1.27 1.27))))
			(property "Footprint" "" (at -1.778 0 90) (effects (font (size 1.27 1.27)) (hide yes)))
			(property "Datasheet" "~" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))
			(symbol "R_0_1" (rectangle (start -1.016 -2.54) (end 1.016 2.54) (stroke (width 0.254) (type default)) (fill (type none))))
			(symbol "R_1_1"
				(pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
				(pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))
			)
		)
	)
	(symbol (lib_id "Device:R") (at 100 100 0) (unit 1) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no)
		(uuid "21111111-1111-1111-1111-111111111111")
		(property "Reference" "R7" (at 102 99 0) (effects (font (size 1.27 1.27)) (justify left)))
		(property "Value" "47k" (at 102 101 0) (effects (font (size 1.27 1.27)) (justify left)))
		(property "Footprint" "" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(property "Datasheet" "~" (at 100 100 0) (effects (font (size 1.27 1.27)) (hide yes)))
		(pin "1" (uuid "31111111-1111-1111-1111-111111111111"))
		(pin "2" (uuid "31111111-1111-1111-1111-111111111112"))
		(instances (project "x" (path "/11111111-1111-1111-1111-111111111111" (reference "R7") (unit 1))))
	)
)
`;

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
  while (state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);

  (async () => {
    await toSelect();
    await select([]);
    base = await snapshot();

    await scenario("The Fields grid: KiCad's schematic columns, Name .. Bold shown at open, the others from the column labels' menu; X / Y Position, Orientation and Allow Autoplacement; OK places an autoplaced symbol's fields again; one undo each", async (t) => {
      const put = await placeR();
      if (!t(put.ok, `a resistor is placed (${put.message})`)) return;
      const v0 = await fieldOf("R90", "Value");
      t(!!v0?.local && typeof v0.local_vertical === "boolean", `the state sends the Value's place in the symbol's own frame (${JSON.stringify([v0?.local, v0?.local_vertical])})`);
      let dlg = await props("R90");
      if (!t(!!dlg, "E opens Symbol Properties")) return;
      t(same(heads(dlg, "symbol-fields-grid"), ["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold"]), `the first eight columns are shown (${heads(dlg, "symbol-fields-grid")})`);
      const items = await columnMenuItems(dlg, "symbol-fields-grid");
      t(same(items.map(label), ["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold", "Text Size", "Orientation", "X Position", "Y Position", "Font", "Color", "Allow Autoplacement"]), `the menu lists every column, no Private (${items.map(label)})`);
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await sleep(200);
      await showColumns(dlg, "symbol-fields-grid", ["Orientation", "X Position", "Y Position", "Allow Autoplacement"]);
      t(heads(dlg, "symbol-fields-grid").includes("X Position") && heads(dlg, "symbol-fields-grid").includes("Allow Autoplacement"), `the menu shows them (${heads(dlg, "symbol-fields-grid")})`);
      const x = dlg.querySelector('input[aria-label="Value x position"]');
      t(!!x && Math.abs(Number(x.value) - v0.local[0] / 1e6) < 1e-3, `X Position is the Value's offset from the symbol (${x?.value} for ${v0?.local})`);
      // fields placed by hand (a symbol just placed keeps no flag): the place typed is where the field goes
      t(!(await sch()).symbols.find((s) => s.id === "R90")?.fields_autoplaced, "the symbol's fields are not marked autoplaced");
      setValue(x, (v0.local[0] / 1e6 + 10).toFixed(2));
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      let v = await fieldOf("R90", "Value");
      t(v?.local?.[0] === v0.local[0] + 10_000_000 && v?.local?.[1] === v0.local[1], `the Value is where it was typed (${JSON.stringify(v?.local)})`);
      await undo();
      t(same((await fieldOf("R90", "Value"))?.local, v0.local), "one undo puts it back");
      // autoplaced (Autoplace Fields, O): OK places the fields again (`AutoplaceFields( screen, fieldsAutoplaced )`), so the place typed does not stay ...
      const o = await cmd({ op: "sch_edit", verb: "autoplace_fields", ids: ["R90"] });
      await sleep(800);
      const v1 = await fieldOf("R90", "Value");
      t((await sch()).symbols.find((s) => s.id === "R90")?.fields_autoplaced === "manual", `Autoplace Fields marks them autoplaced (${o.ok})`);
      dlg = await props("R90");
      await showColumns(dlg, "symbol-fields-grid", ["Orientation", "X Position", "Allow Autoplacement"]);
      setValue(dlg.querySelector('input[aria-label="Value x position"]'), (v1.local[0] / 1e6 + 10).toFixed(2));
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      v = await fieldOf("R90", "Value");
      t(same(v?.local, v1.local), `an autoplaced symbol: OK places the Value again (${JSON.stringify(v?.local)} for ${JSON.stringify(v1.local)})`);
      // ... unless the field's Allow Autoplacement is off; and the Orientation
      dlg = await props("R90");
      await showColumns(dlg, "symbol-fields-grid", ["Orientation", "X Position", "Allow Autoplacement"]);
      dlg.querySelector('input[aria-label="Value allow autoplacement"]').click();
      setValue(dlg.querySelector('input[aria-label="Value x position"]'), (v1.local[0] / 1e6 + 10).toFixed(2));
      setValue(dlg.querySelector('select[aria-label="Value orientation"]'), v1.local_vertical ? "Horizontal" : "Vertical");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      v = await fieldOf("R90", "Value");
      t(v?.local?.[0] === v1.local[0] + 10_000_000 && v?.local_vertical === !v1.local_vertical && v?.allow_autoplace === false, `Allow Autoplacement off: the Value stays where it was typed, turned (${JSON.stringify([v?.local, v?.local_vertical, v?.allow_autoplace])})`);
      t(v?.vertical === !v1.vertical, `its text turns on the sheet (${v1.vertical} -> ${v?.vertical})`);
      await undo();
      v = await fieldOf("R90", "Value");
      t(same(v?.local, v1.local) && v?.local_vertical === v1.local_vertical && v?.allow_autoplace !== false, `one undo puts the field back (${JSON.stringify([v?.local, v?.local_vertical, v?.allow_autoplace])})`);
    });

    await scenario("Move Up and Move Down order the user fields after the mandatory ones; OK sends the order; one undo", async (t) => {
      const put = await placeR();
      if (!t(put.ok, "a resistor is placed")) return;
      for (const [name, text] of [["MPN", "ERJ-3"], ["Tol", "1%"]]) await cmd({ op: "sch_edit", verb: "add_user_field", id: "R90", name, text });
      await sleep(800);
      const order = async () => ((await sch()).symbols.find((s) => s.id === "R90")?.fields ?? []).map((f) => f.name);
      t(same((await order()).slice(4), ["MPN", "Tol"]), `the user fields are listed (${await order()})`);
      const dlg = await props("R90");
      const row = (rowName) => [...dlg.querySelectorAll("[data-testid=symbol-fields-grid] tbody tr")].find((tr) => tr.querySelector("td")?.textContent.trim() === rowName);
      row("Value")?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      await sleep(150);
      t(dlg.querySelector("[data-testid=symbol-field-up]")?.disabled && dlg.querySelector("[data-testid=symbol-field-down]")?.disabled, "a mandatory row moves neither up nor down");
      dlg.querySelector("[data-testid=symbol-field-delete]")?.click();
      await sleep(150);
      t(/The first 4 fields are mandatory\./.test(dlg.textContent), "Delete on a mandatory row: KiCad's message");
      row("Tol")?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      await sleep(150);
      t(dlg.querySelector("[data-testid=symbol-field-down]")?.disabled === true, "the last row goes no further down");
      dlg.querySelector("[data-testid=symbol-field-up]")?.click();
      await sleep(150);
      const rows = [...dlg.querySelectorAll("[data-testid=symbol-fields-grid] tbody tr")].map((tr) => tr.querySelector("td")?.textContent.trim());
      t(same(rows.slice(4), ["Tol", "MPN"]), `Move Up swaps it with the row above (${rows})`);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      t(same((await order()).slice(4), ["Tol", "MPN"]), `OK gives the symbol that order (${await order()})`);
      await undo();
      t(same((await order()).slice(4), ["MPN", "Tol"]), "one undo puts the order back");
    });

    await scenario("Angle and Mirror set the symbol's orientation whatever it was; the dialog reads them back; one undo", async (t) => {
      const put = await placeR();
      if (!t(put.ok, "a resistor is placed")) return;
      let dlg = await props("R90");
      const angle = dlg.querySelector("[data-testid=symbol-angle]");
      const mirror = dlg.querySelector("[data-testid=symbol-mirror]");
      t(same([...angle.options].map((o) => o.textContent), ["0", "+90", "-90", "180"]) && same([...mirror.options].map((o) => o.textContent), ["Not mirrored", "Around X axis", "Around Y axis"]), "KiCad's choices");
      setValue(angle, "270");
      setValue(mirror, "x");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      let s = (await sch()).symbols.find((x) => x.id === "R90");
      t(s?.rot === 270 && s?.mirror === "x", `the symbol is at -90, mirrored around the X axis (${JSON.stringify([s?.rot, s?.mirror])})`);
      dlg = await props("R90");
      t(dlg.querySelector("[data-testid=symbol-angle]")?.value === "270" && dlg.querySelector("[data-testid=symbol-mirror]")?.value === "x", "the dialog reads them back");
      // -90 around the Y axis is +90 around the X axis: `GetOrientation` gives the first of its forms that makes the matrix, and the dialog shows that
      setValue(dlg.querySelector("[data-testid=symbol-mirror]"), "y");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      s = (await sch()).symbols.find((x) => x.id === "R90");
      t(s?.rot === 90 && s?.mirror === "x", `-90 around the Y axis reads back as +90 around the X axis, as GetOrientation reads it (${JSON.stringify([s?.rot, s?.mirror])})`);
      await undo();
      await undo();
      const back = (await sch()).symbols.find((x) => x.id === "R90");
      t(back?.rot === 0 && !back?.mirror, "one undo turns it back");
    });

    await scenario("A power symbol's Properties: Unit and Body style (disabled), Angle and Mirror; its shown user field is drawn on the sheet and picked; one undo each", async (t) => {
      const put = await cmd({ op: "add_power_symbol", lib_id: "power:GND", at: { x: X0 + 30 * G, y: Y0 }, rot_millideg: 0, net: "GND", pin: "" });
      if (!t(put.ok, `a ground symbol is placed (${put.message})`)) return;
      await sleep(1200);
      let ps = (await sch()).power_symbols.at(-1);
      const id = ps?.id;
      const withField = await cmd({ op: "sch_edit", verb: "set_power_props", id, fields: [{ name: "Rating", text: "1A", visible: true }] });
      t(withField.ok, "a shown user field is given to it");
      await sleep(800);
      ps = (await sch()).power_symbols.find((p) => p.id === id);
      const rating = ps?.fields?.find((f) => f.name === "Rating");
      t(!!rating && rating.id === `fld:${id}:Rating` && rating.visible && !same(rating.at, ps.at), `the state lists it as a field of the sheet, where Autoplace Fields puts it (${JSON.stringify(rating)})`);
      if (rating) {
        await fitTo([id]);
        await select([]);
        await clickAt(rating.at[0], rating.at[1]);
        t(same(state().selection.map((x) => x.id ?? x), [rating.id]), `a click on its text picks the field (${JSON.stringify(state().selection)})`);
      }
      let dlg = await props(id);
      if (!t(!!dlg && !!dlg.querySelector("[data-testid=power-general]"), "E opens its Symbol Properties with the General box")) return;
      t(dlg.querySelector("[data-testid=power-unit]")?.disabled && dlg.querySelector("[data-testid=power-body-style]")?.disabled, "Unit and Body style are there, disabled (one of each)");
      t(same(heads(dlg, "power-fields-grid"), ["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold"]), `the grid's columns are the symbol dialog's (${heads(dlg, "power-fields-grid")})`);
      setValue(dlg.querySelector("[data-testid=power-mirror]"), "x");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      t((await sch()).power_symbols.find((p) => p.id === id)?.rot === 180, "Mirror around the X axis turns a ground symbol upside down");
      await undo();
      t((await sch()).power_symbols.find((p) => p.id === id)?.rot === 0, "one undo turns it back");
    });

    await scenario("A sheet read from a KiCad file: its symbol's fields are items, V opens Field Properties on the Value, and Symbol Properties offers the project's field-name templates", async (t) => {
      const drop = await json("/api/sch/dropped_sheets", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ files: [{ name: "kicad_in.kicad_sch", text: KICAD_FILE }] }) });
      if (!t(drop.ok, `the file is put where Import Sheet reads it (${drop.message ?? drop.path})`)) return;
      const before = (await sch()).symbols.map((s) => s.id);
      const put = await cmd({ op: "import_sheet", source: drop.path, as_sheet: false, at: { x: X0 + 60 * G, y: Y0 }, keep_annotations: true });
      if (!t(put.ok, `Import Sheet brings its contents onto the sheet (${put.message})`)) return;
      await sleep(1500);
      const s = (await sch()).symbols.find((x) => !before.includes(x.id) && x.lib_id === "Device:R");
      t(!!s && (s.fields ?? []).some((f) => f.name === "Value" && f.id === `fld:${s.id}:Value`), `its symbol's fields are items (${JSON.stringify(s?.fields?.map((f) => f.id))})`);
      if (!s) return;
      await select([s.id]);
      await run("eeschema.InteractiveEdit.editValue");
      const fp = await opened(/^Edit Value Field/);
      t(!!fp && !document.querySelector("[data-testid=symbol-properties-title]"), `V opens Field Properties, not Symbol Properties (${dialog()?.querySelector(".dialog-header")?.textContent})`);
      await closeDialogs();
      const tpl = await cmd({ op: "set_field_name_templates", fields: [{ name: "Manufacturer", visible: true, url: false }] });
      t(tpl.ok, "the project gets a field-name template");
      const dlg = await props(s.id);
      await sleep(800);
      const names = [...(dlg?.querySelectorAll('input[aria-label="New field name"]') ?? [])].map((i) => i.value);
      t(names.includes("Manufacturer"), `Symbol Properties offers it (${names})`);
      await closeDialogs();
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
