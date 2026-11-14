// The Symbol Editor's nine-scenario click-through (UI-LEFT-2 YY8; PARITY-symedit "Click-through needed"), walked with real input: every click, double
// click, drag, wheel turn and key goes through the browser's own input pipeline (`window.__runCheckInput`, Playwright's mouse and keyboard from
// e2e/run-check.mjs), and each step is compared with what KiCad's code does:
//   1 open         the library tree's search and a double click on Device:R open it (`SYMBOL_EDITOR_CONTROL::EditSymbol`): its two pins are drawn;
//   2 Ctrl+Shift+E a placed resistor clicked on the Schematic tab, Ctrl+Shift+E (`SCH_EDITOR_CONTROL::EditWithSymbolEditor`, "Edit Library Symbol...")
//                  opens its library symbol on the Symbol tab, at its unit;
//   3 pins         Ctrl+N (New Symbol, the name asked first), P: a click asks Pin Properties first and leaves the number empty (`CreatePin`), OK puts the
//                  pin in the hand and a click places it (`TwoClickPlace`); Insert repeats it one step on, its number and name incremented (`RepeatPin`); a
//                  double click on a pin opens Pin Properties, whose type, orientation and length are written back;
//   4 Pin Table    a cell edited inline goes back to the pin on OK (`DIALOG_LIB_EDIT_PIN_TABLE::TransferDataFromWindow`);
//   5 drawing      a rectangle and a circle (two clicks each), a polyline (clicks, a double click), a text (a click, the dialog, OK); the rectangle dragged,
//                  the circle deleted with Del;
//   6 units        Symbol Properties' Number of units 2: the unit selector offers two units; on unit 2 the unit-1 pins are not drawn and not picked, and a
//                  pin "Common to all units in symbol" is on both;
//   7 on board     File > Update Symbol on Board (the studio's push, see kicad/menuExtras.ts): the placed instance's library copy takes the new pins;
//   8 export       File > Export > Symbol... saves `<name>.kicad_sym`: a whole `(kicad_symbol_lib ..)` with the symbol (kicad-cli then loads it: the
//                  check's `file` names where it was written);
//   9 undo         Ctrl+Z on the Symbol tab undoes the symbol's last edit and nothing of the schematic; on the Schematic tab it undoes the schematic's.
//
// Run it through e2e/run-check.mjs (it needs `__runCheckInput`) on a COPY of a board (`eda board new examples/mcu_board_30plus.yaml -o <dir>`), with KiCad's
// libraries (`KICAD_PATH`). It makes a symbol of its own in the project library, places it on the root sheet and takes both away at the end.
// It answers through `window.__symClickCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms), file }.
// run-all-checks: board=mcu30 minutes=12
(async () => {
  const results = [];
  const check = (window.__symClickCheck = { results, done: false, ok: false, wait: null, file: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name), file: check.file };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const state = () => E.state();
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const symNames = async () => (await json("/api/symbol_editor/names")).project ?? [];
  const sch = () => json("/api/schematic");
  const near = (a, b) => Math.abs(a - b) < 1e-6;

  const input = async (ops) => {
    if (typeof window.__runCheckInput !== "function") throw new Error("no real input: run this check through e2e/run-check.mjs");
    const r = await window.__runCheckInput(ops);
    if (r !== "ok") throw new Error(`input: ${r}`);
  };
  const press = async (key, wait = 400) => { await input([{ t: "press", key }]); await sleep(wait); };
  const canvasEl = () => [...document.querySelectorAll(".pcb-canvas-container")].find((c) => c.getBoundingClientRect().width > 0);
  const toScreen = (x, y) => { const v = state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + x * v.scale, r.top + v.y + y * v.scale]; };
  const moveTo = async (x, y, steps = 5) => { const [px, py] = toScreen(x, y); await input([{ t: "move", x: px, y: py, steps }]); await sleep(150); };
  const clickAt = async (x, y, wait = 600, mods = []) => {
    const [px, py] = toScreen(x, y);
    await input([{ t: "move", x: px, y: py, steps: 4 }, { t: "pause", ms: 100 }, ...mods.map((k) => ({ t: "keydown", key: k })), { t: "down" }, { t: "pause", ms: 40 }, { t: "up" }, ...mods.map((k) => ({ t: "keyup", key: k }))]);
    await sleep(wait);
  };
  const dblAt = async (x, y, wait = 900) => { const [px, py] = toScreen(x, y); await input([{ t: "move", x: px, y: py, steps: 3 }, { t: "pause", ms: 100 }, { t: "dblclick", x: px, y: py }]); await sleep(wait); };
  const dragFromTo = async (a, b, wait = 1000) => {
    const [ax, ay] = toScreen(a[0], a[1]);
    const [bx, by] = toScreen(b[0], b[1]);
    await input([{ t: "move", x: ax, y: ay, steps: 3 }, { t: "pause", ms: 150 }, { t: "down" }, { t: "pause", ms: 60 }, { t: "move", x: bx, y: by, steps: 14 }, { t: "pause", ms: 250 }, { t: "up" }]);
    await sleep(wait);
  };
  /** The middle of what can be seen of `el`: its box cut by every ancestor that clips its overflow (a library tree row is as wide as its columns, LL4,
   * wider than the panel that shows it, so the middle of its box can be under the canvas). */
  const visibleMiddle = (el) => {
    let { left, top, right, bottom } = el.getBoundingClientRect();
    for (let a = el.parentElement; a; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.overflowX === "visible" && cs.overflowY === "visible") continue;
      const b = a.getBoundingClientRect();
      left = Math.max(left, b.left); top = Math.max(top, b.top); right = Math.min(right, b.right); bottom = Math.min(bottom, b.bottom);
    }
    return { x: (left + right) / 2, y: (top + bottom) / 2 };
  };
  const clickEl = async (el, wait = 300) => { if (!el) throw new Error("no element to click"); el.scrollIntoView({ block: "nearest" }); await input([{ t: "click", ...visibleMiddle(el) }]); await sleep(wait); };
  const dblEl = async (el, wait = 900) => { el.scrollIntoView({ block: "nearest" }); await input([{ t: "dblclick", ...visibleMiddle(el) }]); await sleep(wait); };
  const typeInto = async (el, text) => { await clickEl(el, 100); await input([{ t: "press", key: "Control+a" }, { t: "press", key: "Delete" }, ...(String(text) === "" ? [] : [{ t: "type", text: String(text) }])]); await sleep(250); };
  /** A <select> by the keyboard: focused, then the arrows to the option (a closed select changes, and fires change, on each). */
  const pick = async (sel, value) => { sel.focus(); const i = [...sel.options].findIndex((o) => o.value === value); const n = i - sel.selectedIndex; if (i >= 0 && n !== 0) await input(Array.from({ length: Math.abs(n) }, () => ({ t: "press", key: n > 0 ? "ArrowDown" : "ArrowUp" }))); await sleep(300); return i >= 0; };
  const clickTab = async (label) => { const b = [...document.querySelectorAll("button, [role=tab]")].find((x) => x.textContent.trim() === label); if (b) await clickEl(b, 1200); };
  const tool = async (action) => { const b = [...document.querySelectorAll(`button.toolbar-button[data-action="${action}"]`)].find((x) => x.getBoundingClientRect().width > 0); if (!b) throw new Error(`no toolbar button ${action}`); await clickEl(b, 400); };
  const menu = async (top, path) => {
    const m = [...document.querySelectorAll(".menubar-item")].find((x) => x.textContent.trim().startsWith(top));
    if (!m) throw new Error(`no menu ${top}`);
    await clickEl(m, 300);
    for (const re of path) {
      const it = [...m.querySelectorAll(".menu-node-item, .menu-node-label")].find((x) => x.getBoundingClientRect().width > 0 && re.test(x.textContent.trim()));
      if (!it) throw new Error(`no item ${re} in ${top}: ${[...m.querySelectorAll(".menu-node-item, .menu-node-label")].map((x) => x.textContent.trim()).join(" | ")}`);
      if (it.classList.contains("menu-node-label")) { const r = it.getBoundingClientRect(); await input([{ t: "move", x: r.left + r.width / 2, y: r.top + r.height / 2, steps: 3 }]); await sleep(300); } else await clickEl(it, 800);
    }
  };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const dialogTitle = () => dialog()?.querySelector(".dialog-header")?.textContent?.trim() ?? dialog()?.getAttribute("aria-label") ?? "";
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const fieldOf = (label) => { const s = [...(dialog()?.querySelectorAll(".kv-grid > span") ?? [])].find((e) => e.textContent.trim().replace(/:$/, "") === label); const n = s?.nextElementSibling; return n?.matches("input,select") ? n : n?.querySelector("input,select") ?? null; };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry || document.querySelector(".dialog")); i++) await press("Escape", 300); };
  const undo = async () => { await press("Control+z", 900); };
  const frameView = async (target = 3e-5) => {
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
    for (let i = 0; i < 25 && state().view.scale < target; i++) { await moveTo(0, 0, 1); await input([{ t: "wheel", dy: -120 }]); await sleep(200); }
    for (let i = 0; i < 25 && state().view.scale > target * 1.8; i++) { await moveTo(0, 0, 1); await input([{ t: "wheel", dy: 120 }]); await sleep(200); }
  };
  /** How much of a small square of the canvas around a sheet point is not background (the painter drew something there). */
  /** How many pixels of the square are in a pin's red (the theme's pin colour), not the blue of the axis through the origin. */
  const pinInk = (x, y, r = 3) => {
    const cv = canvasEl().querySelector("canvas");
    const rect = cv.getBoundingClientRect();
    const k = cv.width / rect.width;
    const [sx, sy] = toScreen(x, y);
    const d = cv.getContext("2d").getImageData(Math.round((sx - rect.left) * k) - r, Math.round((sy - rect.top) * k) - r, 2 * r + 1, 2 * r + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i] > 100 && d[i] > d[i + 1] + 50 && d[i] > d[i + 2] + 50) n++;
    return n;
  };
  const inked = (x, y, r = 3) => {
    const cv = canvasEl().querySelector("canvas");
    const rect = cv.getBoundingClientRect();
    const k = cv.width / rect.width;
    const [sx, sy] = toScreen(x, y);
    const ctx = cv.getContext("2d");
    const bg = ctx.getImageData(2, 2, 1, 1).data;
    const d = ctx.getImageData(Math.round((sx - rect.left) * k) - r, Math.round((sy - rect.top) * k) - r, 2 * r + 1, 2 * r + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - bg[0]) + Math.abs(d[i + 1] - bg[1]) + Math.abs(d[i + 2] - bg[2]) > 60) n++;
    return n;
  };

  const scenario = async (name, fn) => {
    const only = new URLSearchParams(location.hash.slice(1)).get("only");
    if (only && !new RegExp(only).test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await toSelect(); } catch { /* ignore */ }
    entry.notes.push(`note: the editor ends on ${state().symbolEditor?.libId ?? "?"} (${state().tab})`);
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const MM = 1_000_000;
  const G = 1_270_000;
  const before = await symNames();
  const NAME = `Click${Date.now() % 100000}`;
  let SYM = null;
  let placedId = null;
  const RPLACED = "R981";
  // a resistor on the root sheet for Ctrl+Shift+E
  await cmd({ op: "add_symbol", id: RPLACED, lib_id: "Device:R", at: { x: 70 * G, y: 104 * G }, rot_millideg: 0, value: "1k", footprint: "", unit: 1 });
  // what the page saves as a file
  const saved = [];
  if (!window.__symClickBlob) {
    const real = URL.createObjectURL.bind(URL);
    window.__symClickBlob = real;
    URL.createObjectURL = (b) => { if (b instanceof Blob) b.text().then((text) => saved.push({ text, at: Date.now() })); return real(b); };
    const click = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () { if (this.download) saved.push({ name: this.download, at: Date.now() }); return click.call(this); };
  }

  (async () => {
    // ----- 1
    await scenario("1 open: the library tree's search and a double click on Device:R open it; its two pins are drawn", async (t) => {
      await clickTab("Symbol");
      t(state().tab === "symbol", `the Symbol tab (${state().tab})`);
      const search = document.querySelector('[data-library-tree-search="symbol"]');
      await typeInto(search, "Device:R");
      const row = await until(() => document.querySelector('[role="treeitem"][data-name="Device:R"]'), 8000);
      if (!t(!!row, "the search lists Device:R")) return;
      await sleep(800); // the list settles (the libraries load as the search narrows them)
      await dblEl(document.querySelector('[role="treeitem"][data-name="Device:R"]') ?? row, 1500);
      if (state().symbolEditor?.libId !== "Device:R") await dblEl(document.querySelector('[role="treeitem"][data-name="Device:R"]') ?? row, 1500);
      const opened = await until(() => (state().symbolEditor?.libId === "Device:R" ? state().symbolEditor : null), 8000);
      t(!!opened, `the double click opens it (${JSON.stringify(state().symbolEditor)})`);
      const s = await symbol("Device:R");
      t((s.pins ?? []).length === 2, `two pins (${(s.pins ?? []).map((p) => p.number)})`);
      await frameView();
      // each pin's end, where the painter draws its line (a library point is y up, the editor's y down)
      const drawn = (s.pins ?? []).map((p) => inked(p.at.x * MM, -p.at.y * MM));
      t(drawn.every((n) => n > 0), `the canvas draws both pins (${drawn} pixels inked at their ends)`);
      await typeInto(search, "");
    });

    // ----- 2
    await scenario("2 Ctrl+Shift+E: a resistor clicked on the Schematic tab opens its library symbol on the Symbol tab, at its unit", async (t) => {
      await clickTab("Schematic");
      while (state().sheetPath.length > 0) { await E.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
      await E.run("common.Control.zoomFitScreen");
      await sleep(500);
      const d = await sch();
      const r = d.symbols.find((s) => s.id === RPLACED);
      if (!t(!!r, `${RPLACED} is on the sheet`)) return;
      const lib = d.lib_symbols[r.lib_id];
      // the middle of its pins, the body
      const ys = lib.pins.map((p) => -p.at[1] * MM), xs = lib.pins.map((p) => p.at[0] * MM);
      const body = [r.at[0] + xs.reduce((a, b) => a + b, 0) / xs.length, r.at[1] + ys.reduce((a, b) => a + b, 0) / ys.length];
      for (let i = 0; i < 25 && state().view.scale < 1.2e-5; i++) { await moveTo(body[0], body[1], 1); await input([{ t: "wheel", dy: -120 }]); await sleep(200); }
      await clickAt(body[0], body[1]);
      t(state().selection.some((s) => (s.id ?? s) === RPLACED), `the click selects ${RPLACED} (${JSON.stringify(state().selection)})`);
      await press("Control+Shift+E", 1500);
      t(state().tab === "symbol", `the Symbol tab comes up (${state().tab})`);
      t(state().symbolEditor?.libId === r.lib_id && state().symbolEditor?.unit === 1, `with ${r.lib_id}, unit 1 (${JSON.stringify(state().symbolEditor)})`);
    });

    // ----- 3
    await scenario("3 pins: Ctrl+N asks the name; P and a click ask Pin Properties first (no number), a click places the pin; Insert repeats it numbered and named one up; a double click edits a pin", async (t) => {
      await clickTab("Symbol");
      await moveTo(0, 0);
      await press("Control+n", 800);
      t(/New Symbol/.test(dialogTitle()), `Ctrl+N opens New Symbol (${dialogTitle()})`);
      await typeInto(fieldOf("Symbol name"), NAME);
      await clickEl(buttonText(dialog(), "OK"), 1200);
      SYM = await until(async () => (await symNames()).find((n) => n.endsWith(`:${NAME}`)), 8000);
      // into the library of the tree's target (`CreateNewSymbol`: `getTargetLib()`), Device here, whose R the tree has selected; the tree then selects the new one
      t(SYM === `Device:${NAME}`, `in the library of the tree's selection, Device (${SYM})`);
      t(state().symbolEditor?.libId === SYM, `and it is loaded (${state().symbolEditor?.libId})`);
      if (!t(!!SYM && state().symbolEditor?.libId === SYM, `${SYM} is made and open (${state().symbolEditor?.libId})`)) return;
      await frameView();
      await moveTo(-5 * G, 0);
      await press("p", 400);
      t(state().tool === "pin", `P arms the pin tool (${state().tool})`);
      await clickAt(-5 * G, 0, 700);
      t(/Pin Properties/.test(dialogTitle()), `a click opens Pin Properties first (${dialogTitle()})`);
      t(fieldOf("Pin number")?.value === "", `the new pin has no number (${fieldOf("Pin number")?.value})`);
      await typeInto(fieldOf("Pin number"), "1");
      await typeInto(fieldOf("Pin name"), "A1");
      await clickEl(buttonText(dialog(), "OK"), 600);
      t(((await symbol(SYM)).pins ?? []).length === 0, "OK puts it in the hand; nothing is placed yet");
      await clickAt(-5 * G, 0, 900);
      await toSelect();
      let pins = (await symbol(SYM)).pins ?? [];
      t(pins.length === 1 && pins[0].number === "1" && near(pins[0].at.x, -6.35) && near(pins[0].at.y, 0), `the click places pin 1 there (${JSON.stringify(pins.map((p) => [p.number, p.at]))})`);
      await press("Insert", 900);
      pins = (await symbol(SYM)).pins ?? [];
      const rep = pins.find((p) => p.number === "2");
      t(!!rep && rep.name === "A2" && near(rep.at.y, -2.54), `Insert repeats it one step down as pin 2, A2 (${JSON.stringify(rep && [rep.number, rep.name, rep.at])})`);
      // a double click on pin 1's end: Pin Properties; type, orientation and length written back
      await toSelect();
      await dblAt(-5 * G, 0, 900);
      t(/Pin Properties/.test(dialogTitle()), `a double click on pin 1 opens Pin Properties (${dialogTitle()})`);
      if (/Pin Properties/.test(dialogTitle())) {
        t(fieldOf("Pin number")?.value === "1", `of pin 1 (${fieldOf("Pin number")?.value})`);
        await pick(fieldOf("Electrical type"), "input");
        await typeInto(fieldOf("Pin length"), "5.08");
        await clickEl(buttonText(dialog(), "OK"), 1000);
        const p1 = ((await symbol(SYM)).pins ?? []).find((p) => p.number === "1");
        t(p1?.electrical_type === "input" && near(p1?.length_mm ?? 0, 5.08), `pin 1 is an input 5.08 mm long (${JSON.stringify(p1 && [p1.electrical_type, p1.length_mm])})`);
      }
    });

    // ----- 4
    await scenario("4 Pin Table: a pin's name edited in a cell goes back to the pin on OK; one undo", async (t) => {
      if (!SYM) return t(false, "no symbol");
      await tool("eeschema.InteractiveEdit.pinTable");
      const dlg = await until(() => document.querySelector('[data-testid="pin-table"]'), 5000);
      if (!t(!!dlg, `the toolbar's Pin Table opens it (${dialogTitle()})`)) return;
      const rows = [...dlg.querySelectorAll('[data-testid="pin-table-row"]')];
      const row2 = rows.find((r) => [...r.querySelectorAll("input")].some((i) => i.getAttribute("aria-label") === "Number" && i.value === "2"));
      const nameCell = row2?.querySelector('input[aria-label="Name"]');
      if (!t(!!nameCell, `pin 2's row with its Name cell (${rows.length} rows)`)) return;
      await typeInto(nameCell, "B2");
      await press("Tab", 200);
      await clickEl(dlg.querySelector('[data-testid="pin-table-ok"]'), 1000);
      const p2 = ((await symbol(SYM)).pins ?? []).find((p) => p.number === "2");
      t(p2?.name === "B2", `pin 2 is named B2 now (${p2?.name})`);
      await undo();
      t(((await symbol(SYM)).pins ?? []).find((p) => p.number === "2")?.name === "A2", "one Ctrl+Z gives it back A2");
      await press("Control+y", 900);
    });

    // ----- 5
    await scenario("5 drawing: a rectangle, a circle, a polyline and a text drawn; the rectangle dragged, the circle deleted; one undo each", async (t) => {
      if (!SYM) return t(false, "no symbol");
      const g0 = ((await symbol(SYM)).graphics ?? []).length;
      await tool("eeschema.InteractiveDrawing.drawRectangle");
      await clickAt(-2 * G, -4 * G, 400);
      await clickAt(4 * G, 2 * G, 900);
      await tool("eeschema.InteractiveDrawing.drawCircle");
      await clickAt(8 * G, 0, 400);
      await clickAt(9 * G, 0, 900);
      await tool("eeschema.SymbolDrawing.drawSymbolLines");
      await clickAt(-2 * G, 5 * G, 300);
      await clickAt(2 * G, 5 * G, 300);
      await dblAt(2 * G, 7 * G, 900);
      await toSelect();
      let g = (await symbol(SYM)).graphics ?? [];
      const kinds = g.slice(g0).map((x) => x.kind);
      t(g.length === g0 + 3 && kinds.includes("rectangle") && kinds.includes("circle") && kinds.some((k) => /poly/.test(k)), `a rectangle, a circle and a polyline (${JSON.stringify(g.slice(g0).map((x) => [x.kind, x.start ?? x.center ?? x.points?.length]))})`);
      const rect = g.slice(g0).find((x) => x.kind === "rectangle");
      t(!!rect && near(rect.start.x, -2.54) && near(rect.start.y, 5.08) && near(rect.end.x, 5.08) && near(rect.end.y, -2.54), `the rectangle from the first click to the second, y up (${JSON.stringify(rect && [rect.start, rect.end])})`);
      const t0 = ((await symbol(SYM)).texts ?? []).length;
      await tool("eeschema.SymbolDrawing.placeSymbolText");
      await clickAt(0, 9 * G, 900);
      if (t(/Text Properties/.test(dialogTitle()), `a click with the text tool opens Text Properties (${dialogTitle()})`)) {
        const box = dialog().querySelector("textarea") ?? dialog().querySelector('input:not([type]), input[type="text"]');
        t(document.activeElement === box, `the text box has the focus (${document.activeElement?.tagName})`);
        await typeInto(box, "NOTE");
        await clickEl(buttonText(dialog(), "OK"), 900);
        if (state().carry || state().tool !== "select") await clickAt(0, 9 * G, 900);
      }
      await toSelect();
      const texts = [...((await symbol(SYM)).texts ?? []), ...((await symbol(SYM)).graphics ?? []).filter((x) => x.kind === "text")];
      t(texts.length === t0 + 1 && texts.some((x) => /NOTE/.test(x.text ?? x.content ?? "")), `the text is there (${JSON.stringify(texts.map((x) => x.text ?? x.content))})`);
      // the rectangle by its left edge (clear of the pins and the fields), dragged two steps right
      await clickAt(-2 * G, -2 * G);
      t(state().selection.length === 1 && state().selection[0].kind !== "unknown" && !/^fld:/.test(state().selection[0].id), `a click on the rectangle's edge picks it (${JSON.stringify(state().selection)})`);
      await dragFromTo([-2 * G, -2 * G], [0, -2 * G]);
      g = (await symbol(SYM)).graphics ?? [];
      const moved = g.find((x) => x.kind === "rectangle");
      // (to a few nm: a shape has no anchor, so the drag is held where the pointer pressed, `BestDragOrigin`'s mouse, which the screen's pixels round)
      const nearNm = (a, b) => Math.abs(a - b) < 1e-5;
      t(!!moved && nearNm(moved.start.x, 0) && nearNm(moved.end.x, 7.62), `the drag moved it 2 grid right (${JSON.stringify(moved && [moved.start, moved.end])})`);
      // the circle by its edge, Del
      await clickAt(9 * G, 0);
      await press("Delete", 900);
      g = (await symbol(SYM)).graphics ?? [];
      t(!g.some((x) => x.kind === "circle"), `Del deletes the circle (${g.map((x) => x.kind)})`);
      for (let i = 0; i < 6; i++) await undo();
      g = (await symbol(SYM)).graphics ?? [];
      t(g.length === g0 && ((await symbol(SYM)).texts ?? []).length === t0, `six Ctrl+Z take the six edits back (${g.length - g0} graphics left)`);
    });

    // ----- 6
    await scenario("6 units: Number of units 2 grows the unit selector and copies unit 1 into unit 2; on unit 2 a unit-1 pin is neither drawn nor picked; a pin common to all units is on both", async (t) => {
      if (!SYM) return t(false, "no symbol");
      await tool("eeschema.InteractiveEdit.symbolProperties");
      const dlg = await until(() => document.querySelector('[data-testid="symbol-properties"]'), 5000);
      if (!t(!!dlg, `Symbol Properties (${dialogTitle()})`)) return;
      await typeInto(dlg.querySelector('[data-testid="props-unit-count"]'), "2");
      await clickEl(dlg.querySelector('[data-testid="props-ok"]'), 1200);
      const s2 = await symbol(SYM);
      t(s2.unit_count === 2, `two units (${s2.unit_count}; ${Object.keys(s2).slice(0, 12)})`);
      const unitSel = [...document.querySelectorAll(".toolbar-control select")].find((s) => /unit/i.test(s.closest(".toolbar-control")?.getAttribute("title") ?? ""));
      t(!!unitSel && unitSel.options.length === 2, `the unit selector offers two units (${unitSel && [...unitSel.options].map((o) => o.textContent)})`);
      if (!unitSel) return;
      // `SetUnitCount( 2, true )`: unit 2 starts as a copy of unit 1's items
      const pinsNow = (await symbol(SYM)).pins ?? [];
      const copies = pinsNow.filter((p) => p.unit === 2);
      t(copies.length > 0 && copies.length === pinsNow.filter((p) => (p.unit ?? 1) === 1).length, `unit 2 starts as a copy of unit 1's pins (${JSON.stringify(pinsNow.map((p) => [p.number, p.unit]))})`);
      await pick(unitSel, unitSel.options[1].value);
      t(state().symbolEditor?.unit === 2, `unit 2 is edited (${JSON.stringify(state().symbolEditor)})`);
      await sleep(500);
      // unit 2's copy of pin 2 is the one drawn and picked there; once it is deleted, unit 1's pin 2 is neither (pin 2: off the axis through the origin,
      // whose line would hide a pin's on the canvas)
      await clickAt(-4 * G, 2 * G);
      const picked = state().selection.map((x) => x.id ?? x);
      const copy2 = copies.find((p) => p.number === "2");
      t(picked.length === 1 && picked[0] === copy2?.id, `a click on pin 2's place picks unit 2's copy (${JSON.stringify(picked)} vs ${copy2?.id})`);
      await press("Delete", 900);
      t(!((await symbol(SYM)).pins ?? []).some((p) => p.id === copy2?.id), "Del deletes unit 2's copy");
      await moveTo(12 * G, 12 * G); // the crosshair away from where the pixels are read
      await sleep(400);
      t(pinInk(-4 * G, 2 * G, 2) === 0, `unit 1's pin 2 is not drawn on unit 2 (${pinInk(-4 * G, 2 * G, 2)} pin-red pixels on its line)`);
      await clickAt(-4 * G, 2 * G);
      t(state().selection.length === 0, `nor picked there (${JSON.stringify(state().selection)})`);
      // a pin common to all units, on unit 2
      await moveTo(-5 * G, -4 * G);
      await press("p", 400);
      await clickAt(-5 * G, -4 * G, 700);
      if (t(/Pin Properties/.test(dialogTitle()), "Pin Properties for the new pin")) {
        await typeInto(fieldOf("Pin number"), "3");
        await typeInto(fieldOf("Pin name"), "C");
        const common = dialog().querySelector('[data-testid="pin-common-units"]');
        if (common && !common.checked) await clickEl(common, 200);
        t(!!common?.checked, "Common to all units in symbol is ticked");
        await clickEl(buttonText(dialog(), "OK"), 600);
        await clickAt(-5 * G, -4 * G, 900);
      }
      await toSelect();
      const p3 = ((await symbol(SYM)).pins ?? []).find((p) => p.number === "3");
      t(p3?.unit === 0, `pin 3 belongs to every unit (unit ${p3?.unit})`);
      t(pinInk(-4 * G, -4 * G, 2) > 0, `the common pin 3 is drawn on unit 2 (${pinInk(-4 * G, -4 * G, 2)})`);
      await pick(unitSel, unitSel.options[0].value);
      await moveTo(12 * G, 12 * G);
      await sleep(500);
      t(state().symbolEditor?.unit === 1 && pinInk(-4 * G, -4 * G, 2) > 0 && pinInk(-4 * G, 2 * G, 2) > 0, `back on unit 1 both pin 2 and the common pin 3 are drawn (${pinInk(-4 * G, 2 * G, 2)}, ${pinInk(-4 * G, -4 * G, 2)})`);
    });

    // ----- 7
    await scenario("7 Update Symbol on Board: a placed instance's library copy takes the symbol's pins", async (t) => {
      if (!SYM) return t(false, "no symbol");
      // an instance placed from the Symbol Editor (`addSymbolToSchematic`: the symbol rides the cursor on the Schematic tab, a click places it)
      const ids0 = (await sch()).symbols.map((x) => x.id);
      await tool("eeschema.SymbolLibraryControl.addSymbolToSchematic");
      await sleep(800);
      t(state().tab === "schematic" && !!state().carry, `Add Symbol to Schematic: the Schematic tab with the symbol in the hand (${state().tab}, ${JSON.stringify(state().carry)?.slice(0, 80)})`);
      await clickAt(90 * G, 104 * G, 1200);
      await toSelect();
      const d0 = await sch();
      const placed = d0.symbols.find((x) => !ids0.includes(x.id) && x.lib_id === SYM);
      if (!t(!!placed, `a click places it (${d0.symbols.filter((x) => !ids0.includes(x.id)).map((x) => [x.id, x.lib_id])})`)) return;
      placedId = placed.id;
      const placedLib = placed.lib_id;
      await clickTab("Symbol");
      await frameView();
      const n0 = (d0.lib_symbols[placedLib]?.pins ?? []).length;
      // a fourth pin, then the push
      await moveTo(8 * G, 0);
      await press("p", 400);
      await clickAt(8 * G, 0, 700);
      if (/Pin Properties/.test(dialogTitle())) {
        await typeInto(fieldOf("Pin number"), "4");
        await typeInto(fieldOf("Pin name"), "D");
        await clickEl(buttonText(dialog(), "OK"), 600);
        await clickAt(8 * G, 0, 900);
      }
      await toSelect();
      const want = ((await symbol(SYM)).pins ?? []).length;
      await menu("File", [/^Update Symbol on Board/]);
      await sleep(1500);
      const got = ((await sch()).lib_symbols[placedLib]?.pins ?? []).length;
      const d1 = await sch();
      t(got === want && got > n0, `the schematic's copy has the symbol's ${want} pins (${n0} -> ${got}; keys ${Object.keys(d1.lib_symbols).filter((k) => k.includes(NAME))}; placed ${JSON.stringify(d1.symbols.find((x) => x.id === placedId)?.pins?.length)})`);
      t((d1.symbols.find((x) => x.id === placedId)?.pins ?? []).length === want, "and the placed symbol has them");
    });

    // ----- 8
    await scenario("8 File > Export > Symbol... saves <name>.kicad_sym, a whole symbol library with the symbol", async (t) => {
      if (!SYM) return t(false, "no symbol");
      if (state().tab !== "symbol") await clickTab("Symbol");
      const from = saved.length;
      await menu("File", [/^Export/, /^Symbol\.\.\./]);
      const got = await until(() => (saved.slice(from).some((s) => s.text) ? saved.slice(from) : null), 8000);
      const file = got?.find((s) => s.text)?.text ?? "";
      const name = got?.find((s) => s.name)?.name ?? "";
      t(name === `${NAME.toLowerCase()}.kicad_sym`, `the file is named ${NAME.toLowerCase()}.kicad_sym (${name})`);
      let depth = 0, balanced = true;
      for (const ch of file.replace(/"(?:[^"\\]|\\.)*"/g, "")) { if (ch === "(") depth++; else if (ch === ")") { depth--; if (depth < 0) balanced = false; } }
      t(/^\(kicad_symbol_lib\b/.test(file.trim()) && balanced && depth === 0, `a whole (kicad_symbol_lib ..) s-expression (${file.length} chars, depth ${depth})`);
      const nPins = ((await symbol(SYM)).pins ?? []).length;
      t(new RegExp(`\\(symbol "${NAME}"`).test(file) && (file.match(/\(pin /g) ?? []).length === nPins, `with the symbol and its ${nPins} pins, every unit's (${(file.match(/\(pin /g) ?? []).length})`);
      if (window.__runCheckWriteFile && file) {
        const path = `/tmp/symedit-clickthrough-${NAME}.kicad_sym`;
        if ((await window.__runCheckWriteFile(path, file)) === "ok") check.file = path;
      }
    });

    // ----- 9
    await scenario("9 undo per tab: Ctrl+Z on the Symbol tab undoes the symbol's edit only; on the Schematic tab the schematic's", async (t) => {
      if (!SYM) return t(false, "no symbol");
      if (state().tab !== "symbol") await clickTab("Symbol");
      const pins0 = ((await symbol(SYM)).pins ?? []).length;
      const placed0 = (await sch()).symbols.some((s) => s.id === placedId);
      // a schematic edit: the placed resistor moved (on the Schematic tab, by the keyboard: select it, M is not needed -- a cmd is the schematic's own step)
      await cmd({ op: "sch_move", verb: "move", ids: [RPLACED], dx: 2 * G, dy: 0 });
      await sleep(500);
      const r0 = (await sch()).symbols.find((s) => s.id === RPLACED)?.at;
      // a symbol edit: pin 4 deleted on the Symbol tab
      await clickAt(8 * G, 0);
      await press("Delete", 900);
      t(((await symbol(SYM)).pins ?? []).length === pins0 - 1, "Del takes pin 4 away");
      await clickTab("Schematic");
      await undo();
      t(((await symbol(SYM)).pins ?? []).length === pins0 - 1, "Ctrl+Z on the Schematic tab leaves the symbol alone");
      const r1 = (await sch()).symbols.find((s) => s.id === RPLACED)?.at;
      t(!!r0 && !!r1 && r1[0] === r0[0] - 2 * G, `and undoes the schematic's move (${JSON.stringify([r0, r1])})`);
      t((await sch()).symbols.some((s) => s.id === placedId) === placed0, "and nothing else of the schematic");
      await clickTab("Symbol");
      await undo();
      t(((await symbol(SYM)).pins ?? []).length === pins0, "Ctrl+Z on the Symbol tab brings pin 4 back");
    });

    // ----------------------------------------------------------------------------------------------------------------------- clean-up
    await toSelect();
    if (placedId) await cmd({ op: "delete_symbol", id: placedId });
    await cmd({ op: "delete_symbol", id: RPLACED });
    if (SYM) await cmd({ op: "delete_library_symbol", lib_id: SYM });
    // an opened Device:R made a project copy; leave the library as it was
    for (const n of await symNames()) if (!before.includes(n) && n !== SYM) await cmd({ op: "delete_library_symbol", lib_id: n });
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
