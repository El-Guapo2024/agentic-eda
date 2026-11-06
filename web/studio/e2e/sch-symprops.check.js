// UI-LEFT S1b, S4 and S5 (PARITY-sch §59): Place Symbol (A) carries the symbol -- the chooser at the first press, the symbol rides the cursor numbered, R / X turn
// it while it rides, the click places it with them, "Place all units" brings the next unit, "Place repeated copies" a copy with the next reference, Esc with it
// opens the chooser again (`SCH_DRAWING_TOOLS::PlaceSymbol`); Symbol Properties' Unit, Pin Functions tab and the fields grid's Font and Color
// (`DIALOG_SYMBOL_PROPERTIES`); U / V / F open Field Properties on the one field (`SCH_EDIT_TOOL::EditField`); a pin's Pin Function submenu
// (`ALT_PIN_FUNCTION_MENU`), the Symbol Unit submenu (`SYMBOL_UNIT_MENU`) and View > Show Pin Alternate Icons.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with KiCad's libraries (`KICAD_PATH`: Amplifier_Operational:LM358 has three units, MCU_ST_STM32F0:STM32F030F4Px pins with alternates), on a COPY of a board: every
// scenario undoes what it did (and, if it left anything behind, the run undoes until the root sheet is as it was).
//
// It answers through `window.__schSymPropsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }. It works through `window.__eda` (the tools' actions, R, X,
// Esc, `__eda.state().carry`), the dialogs' controls and the pointer events a user makes on the canvas.
// run-all-checks: board=mcu30 minutes=12
(async () => {
  const results = [];
  const check = (window.__schSymPropsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schSymPropsSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 6e6;
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
    window.__schSymPropsSched = true;
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
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(900); };
  const contextAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    fire("pointermove", px, py);
    await sleep(200);
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, button: 2 }));
    await sleep(500);
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const newOf = (before, list) => list.filter((x) => !before.some((b) => b.id === x.id && b.unit === x.unit));
  /** The item in the hand, once its drawing has arrived. */
  const carried = (pred = () => true) => until(() => (state().carry?.drawn && pred(state().carry) ? state().carry : null), 10000);
  /** The dialog open now, by its title. */
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 8000);
  /** The chooser's search for `query`, then its row `id` chosen. */
  const choose = async (dlg, query, id) => {
    setValue(dlg.querySelector("[data-chooser-search]"), query);
    await until(() => dlg.querySelector(`[role=treeitem][data-id="${id}"]`), 40000);
    dlg.querySelector(`[role=treeitem][data-id="${id}"]`)?.click();
    await sleep(500);
    return !!dlg.querySelector(`[role=treeitem][data-id="${id}"]`);
  };
  /** Where a library pin of a placed, unturned symbol ends on the sheet: the symbol's place plus the pin's end (mm, y up, in the drawn symbol's frame). */
  const pinEnd = (s, sym, number) => {
    const lp = s.lib_symbols[sym.lib_id]?.pins.find((p) => p.number === number);
    return lp ? [Math.round(sym.at[0] + lp.at[0] * 1e6), Math.round(sym.at[1] - lp.at[1] * 1e6)] : null;
  };

  /** The root sheet as the scenarios change it. */
  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror, x.pins.map((p) => p.alt ?? null), (x.fields ?? []).map((f) => [f.name, f.font ?? null, f.color ?? null])]), power: s.power_symbols.map((p) => [p.id, p.props ?? null]) });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    if (window.__schSymPropsOnly && !window.__schSymPropsOnly.test(name)) return; // `window.__schSymPropsOnly = /regexp/` runs some of the scenarios only
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
  await leave();
  await fit();
  const v0 = state().view;
  if (!v0 || !Number.isFinite(v0.scale) || v0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(v0)})`] }); check.done = true; return "no canvas size"; }
  base = await snapshot();
  // An empty part of the root sheet (below and left of the module sheets), on the 50 mil grid.
  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 110 * G;
  const LM358 = "Amplifier_Operational:LM358";
  const STM32 = "MCU_ST_STM32F0:STM32F030F4Px";

  (async () => {
    // ----- Place Symbol: the chooser first, the symbol rides, R and X turn it, the click places it, Place all units brings B and C
    await scenario("Place Symbol (A): the chooser at the press, the symbol rides numbered, R and X turn it, the click places it turned; Place all units brings B then C; one undo each", async (t) => {
      const s0 = await sch();
      await hoverAt(X0, Y0);
      await run("eeschema.InteractiveDrawing.placeSymbol");
      const dlg = await opened(/^Symbol Chooser/);
      if (!t(!!dlg, `A opens the Symbol Chooser at once (${state().dialogs})`)) return;
      t(state().tool === "sch_place_symbol" && !state().carry, `the tool is armed, nothing in the hand yet (${state().tool})`);
      t(dlg.querySelector("[data-chooser-all-units]")?.checked === true && dlg.querySelector("[data-chooser-keep]")?.checked === false, "Place all units is on and Place repeated copies off, as the dialog is made");
      if (!t(await choose(dlg, "LM358", LM358), "the search finds Amplifier_Operational:LM358")) return;
      buttonText(dlg, "OK").click();
      let c = await carried();
      if (!t(c?.kind === "symbol" && c.libId === LM358 && c.unit === 1 && /^U\d+$/.test(c.ref ?? ""), `OK puts the symbol in the hand, numbered (${JSON.stringify(c)})`)) return;
      const ref = c.ref;
      t((await sch()).symbols.length === s0.symbols.length, "nothing is on the sheet yet");
      await run("eeschema.InteractiveEdit.rotateCCW");
      await run("eeschema.InteractiveEdit.mirrorH");
      c = await carried((x) => (x.turns ?? []).length === 2);
      t(same(c?.turns, ["rot_ccw", "mirror_h"]), `R and X turn it while it rides (${JSON.stringify(c?.turns)})`);
      await clickAt(X0, Y0);
      const s1 = await sch();
      const a = newOf(s0.symbols, s1.symbols);
      t(a.length === 1 && a[0].id === ref && a[0].unit === 1 && same(a[0].at, [X0, Y0]) && (a[0].rot !== 0 || a[0].mirror !== null), `the click places unit A there, turned (${JSON.stringify(a.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror]))})`);
      c = await carried((x) => x.unit === 2);
      t(c?.ref === ref && c?.unit === 2 && same(c?.turns, ["rot_ccw", "mirror_h"]), `"Place all units": B rides next, under ${ref}, turned as A was (${JSON.stringify(c)})`);
      await clickAt(X0 + 20 * G, Y0);
      c = await carried((x) => x.unit === 3);
      t(c?.unit === 3, `then C (${JSON.stringify(c)})`);
      await escape();
      t(!state().carry && state().tool === "sch_place_symbol", "Esc drops it and the tool stays");
      await escape();
      t(state().tool === "select", "a second Esc leaves the tool");
      const s2 = await sch();
      t(newOf(s0.symbols, s2.symbols).map((x) => `${x.id}#${x.unit}`).join() === `${ref}#1,${ref}#2`, `A and B are placed (${newOf(s0.symbols, s2.symbols).map((x) => `${x.id}#${x.unit}`)})`);
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).map((x) => x.unit).join() === "1", "one undo takes B back");
      await undo();
      t((await sch()).symbols.length === s0.symbols.length, "and another A");
    });

    // ----- Place repeated copies
    await scenario("Place repeated copies: a copy with the next reference rides after each click, and Esc opens the chooser again", async (t) => {
      const s0 = await sch();
      await hoverAt(X0, Y0 + 20 * G);
      await run("eeschema.InteractiveDrawing.placeSymbol");
      const dlg = await opened(/^Symbol Chooser/);
      if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
      dlg.querySelector("[data-chooser-keep]").click();
      await sleep(200);
      buttonText(dlg, "OK").click();
      const c1 = await carried();
      if (!t(c1?.kind === "symbol" && /^R\d+$/.test(c1.ref ?? ""), `a resistor rides (${JSON.stringify(c1)})`)) return;
      await clickAt(X0, Y0 + 20 * G);
      const c2 = await carried((x) => x.ref !== c1.ref);
      const n = (r) => Number(r.slice(1));
      t(c2?.ref && n(c2.ref) > n(c1.ref), `a copy with the next reference rides (${c1.ref} -> ${c2?.ref})`);
      await clickAt(X0 + 10 * G, Y0 + 20 * G);
      t(newOf(s0.symbols, (await sch()).symbols).map((x) => x.id).join() === `${c1.ref},${c2?.ref}`, "both are placed");
      await carried();
      await escape();
      const again = await opened(/^Symbol Chooser/);
      t(!!again && !state().carry, "Esc with it drops the copy and opens the chooser again (`Re-enter symbol chooser`)");
      await closeDialogs();
      await toSelect();
      await undo();
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).length === 0, "two undos take the two back, one each");
    });

    // ----- Symbol Properties: the Unit, a pin's alternate, a field's font and colour
    await scenario("Symbol Properties: the Unit choice, the Pin Functions tab and a field's Font and Color; each OK one undo step", async (t) => {
      const s0 = await sch();
      const put = await cmd({ op: "batch", cmds: [
        { op: "add_symbol", id: "U91", lib_id: LM358, at: { x: X0, y: Y0 + 40 * G }, rot_millideg: 0, value: "", footprint: "", unit: 1 },
        { op: "add_symbol", id: "U90", lib_id: STM32, at: { x: X0 + 30 * G, y: Y0 + 40 * G }, rot_millideg: 0, value: "", footprint: "", unit: 1 },
      ] });
      if (!t(put.ok, `an LM358 and an STM32F030 are placed (${put.message})`)) return;
      await sleep(1200);
      const steps0 = (await sch()).symbols.length;
      // the Unit
      await select(["U91"]);
      await run("eeschema.InteractiveEdit.properties");
      let dlg = await opened(/^Symbol Properties/);
      if (!t(!!dlg, "E opens Symbol Properties")) return;
      const unitSel = dlg.querySelector("[data-testid=symbol-unit]");
      t(unitSel && !unitSel.disabled && [...unitSel.options].map((o) => o.textContent).join() === "A,B,C", `the Unit choice lists the units (${unitSel && [...unitSel.options].map((o) => o.textContent)})`);
      t(dlg.querySelector("[data-testid=symbol-body-style]")?.disabled === true, "Body style is disabled for a symbol of one body style");
      setValue(unitSel, "3");
      buttonText(dlg, "OK").click();
      await sleep(1200);
      t((await sch()).symbols.find((x) => x.id === "U91")?.unit === 3, "OK draws unit C");
      await undo();
      t((await sch()).symbols.find((x) => x.id === "U91")?.unit === 1, "one undo puts unit A back");
      // the Pin Functions tab
      await select(["U90"]);
      await run("eeschema.InteractiveEdit.properties");
      dlg = await opened(/^Symbol Properties/);
      dlg.querySelector("[data-testid=symbol-tab-pins]").click();
      await sleep(300);
      const grid = dlg.querySelector("[data-testid=symbol-pin-grid]");
      const heads = grid ? [...grid.querySelectorAll("th")].map((h) => h.textContent.replace(/[▲▼]/g, "").trim()) : [];
      t(same(heads, ["Pin Number", "Base Pin Name", "Alternate Assignment", "Electrical Type", "Graphic Style"]), `KiCad's columns (${heads})`);
      const row6 = grid?.querySelector('tr[data-pin="6"]');
      const alt6 = row6?.querySelector("select");
      t(!!alt6 && [...alt6.options].map((o) => o.value).includes("ADC_IN0") && alt6.value === "PA0", `pin 6 offers its alternates, on its own name (${alt6 && [...alt6.options].map((o) => o.value)})`);
      setValue(alt6, "ADC_IN0");
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      let u90 = (await sch()).symbols.find((x) => x.id === "U90");
      t(u90?.pins.find((p) => p.number === "6")?.alt === "ADC_IN0", `OK gives pin 6 the alternate (${JSON.stringify(u90?.pins.find((p) => p.number === "6"))})`);
      await undo();
      u90 = (await sch()).symbols.find((x) => x.id === "U90");
      t(!u90?.pins.find((p) => p.number === "6")?.alt, "one undo takes it back");
      // a field's Font and Color
      await select(["U90"]);
      await run("eeschema.InteractiveEdit.properties");
      dlg = await opened(/^Symbol Properties/);
      // the Font and Color columns are hidden when the dialog opens (`ShowHideColumns( "0 1 2 3 4 5 6 7" )`): the column labels' menu shows them
      for (const label of ["Font", "Color"]) {
        const head = dlg.querySelector("[data-testid=symbol-fields-grid] thead");
        head?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 200, clientY: 200, button: 2 }));
        await sleep(200);
        [...document.querySelectorAll(".menu-node-item")].find((i) => i.textContent.replace(/[\u2713\u2003]/g, "").trim() === label)?.click();
        await sleep(200);
      }
      setValue(dlg.querySelector('input[aria-label="Value font"]'), "DejaVu Sans");
      setValue(dlg.querySelector('input[aria-label="Value color"]'), "#ff0000");
      dlg.querySelector('input[aria-label="Value bold"]').click();
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const value = (await sch()).symbols.find((x) => x.id === "U90")?.fields?.find((f) => f.name === "Value");
      const red = (c) => !!c && c.r === 255 && c.g === 0 && c.b === 0 && c.a === 255;
      t(value?.font === "DejaVu Sans" && red(value?.color) && value?.bold === true, `the Value's font, colour and bold are set (${JSON.stringify([value?.font, value?.color, value?.bold])})`);
      await undo();
      const back = (await sch()).symbols.find((x) => x.id === "U90")?.fields?.find((f) => f.name === "Value");
      t(!back?.font && !back?.color && !back?.bold, "one undo puts the look back");
      t((await sch()).symbols.length === steps0, "the symbols are as they were");
    });

    // ----- U / V / F
    await scenario("U, V and F open Field Properties on the Reference, the Value or the Footprint, not Symbol Properties", async (t) => {
      const put = await cmd({ op: "add_symbol", id: "U90", lib_id: STM32, at: { x: X0 + 30 * G, y: Y0 + 40 * G }, rot_millideg: 0, value: "", footprint: "", unit: 1 });
      if (!t(put.ok, "an STM32F030 is placed")) return;
      await sleep(1200);
      for (const [action, title] of [["eeschema.InteractiveEdit.editReference", /^Edit Reference Field/], ["eeschema.InteractiveEdit.editValue", /^Edit Value Field/], ["eeschema.InteractiveEdit.editFootprint", /^Edit Footprint Field/]]) {
        await select(["U90"]);
        await run(action);
        const dlg = await opened(title);
        t(!!dlg && !document.querySelector("[data-testid=symbol-properties-title]"), `${action.split(".").pop()} opens ${title.source.slice(1)} (${dlg?.querySelector(".dialog-header")?.textContent})`);
        await closeDialogs();
      }
    });

    // ----- the canvas's Pin Function and Symbol Unit submenus, View > Show Pin Alternate Icons
    await scenario("A pin's Pin Function submenu sets its alternate, a symbol's Symbol Unit submenu changes its unit, and View > Show Pin Alternate Icons toggles", async (t) => {
      const put = await cmd({ op: "batch", cmds: [
        { op: "add_symbol", id: "U91", lib_id: LM358, at: { x: X0, y: Y0 + 40 * G }, rot_millideg: 0, value: "", footprint: "", unit: 1 },
        { op: "add_symbol", id: "U90", lib_id: STM32, at: { x: X0 + 30 * G, y: Y0 + 40 * G }, rot_millideg: 0, value: "", footprint: "", unit: 1 },
      ] });
      if (!t(put.ok, "an LM358 and an STM32F030 are placed")) return;
      await sleep(1200);
      let s = await sch();
      const u90 = s.symbols.find((x) => x.id === "U90");
      t((s.lib_symbols[STM32]?.pins.find((p) => p.number === "7")?.alternates ?? []).map((a) => a.name).includes("USART1_RTS"), "the drawn symbol's pins carry their alternates");
      const end = pinEnd(s, u90, "7");
      await select(["pin:U90:7"]);
      await contextAt(end[0], end[1]);
      const sub = [...document.querySelectorAll(".menu-node-submenu")].find((m) => m.querySelector(".menu-node-label")?.textContent.includes("Pin Function"));
      const items = sub ? [...sub.querySelectorAll(".menu-node-item")].map((i) => [i.textContent.replace("✓", "").trim(), i.getAttribute("aria-checked")]) : [];
      t(items.length > 1 && items[0][0] === "PA1" && items[0][1] === "true" && items.some(([n]) => n === "USART1_RTS"), `the pin's menu has Pin Function: its own name checked, then its alternates (${JSON.stringify(items)})`);
      [...(sub?.querySelectorAll(".menu-node-item") ?? [])].find((i) => i.textContent.includes("USART1_RTS"))?.click();
      await sleep(1500);
      s = await sch();
      t(s.symbols.find((x) => x.id === "U90")?.pins.find((p) => p.number === "7")?.alt === "USART1_RTS", "choosing USART1_RTS sets the pin's alternate");
      // the Symbol Unit submenu
      await select(["U91"]);
      const u91 = s.symbols.find((x) => x.id === "U91");
      // inside unit A's body: the middle of its pins' ends (the output on the right, the two inputs on the left)
      const ends = (s.lib_symbols[LM358]?.pins ?? []).filter((p) => p.unit === 1).map((p) => pinEnd(s, u91, p.number)).filter(Boolean);
      const b = ends.length > 0 ? [Math.round(ends.reduce((a, e) => a + e[0], 0) / ends.length), Math.round(ends.reduce((a, e) => a + e[1], 0) / ends.length)] : [u91.at[0] + 2 * G, u91.at[1] + 2 * G];
      await contextAt(b[0], b[1]);
      const unitSub = [...document.querySelectorAll(".menu-node-submenu")].find((m) => m.querySelector(".menu-node-label")?.textContent.includes("Symbol Unit"));
      const unitItems = unitSub ? [...unitSub.querySelectorAll(".menu-node-item")].map((i) => i.textContent.replace("✓", "").trim()) : [];
      const menuLabels = [...document.querySelectorAll(".menu-node-submenu > .menu-node-label")].map((l) => l.textContent.replace("▸", ""));
      t(unitItems.includes("A (already placed)") && unitItems.includes("B") && unitItems.includes("Place unit B"), `the symbol's menu has Symbol Unit (${JSON.stringify(unitItems)}; the menu: ${menuLabels}; selection ${JSON.stringify(state().selection)} at ${b})`);
      [...(unitSub?.querySelectorAll(".menu-node-item") ?? [])].find((i) => i.textContent.trim() === "B")?.click();
      await sleep(1500);
      t((await sch()).symbols.find((x) => x.id === "U91")?.unit === 2, "choosing B makes it unit B");
      await undo();
      // View > Show Pin Alternate Icons
      const viewMenu = [...document.querySelectorAll(".menubar-item")].find((m) => m.textContent.trim().startsWith("View"));
      const altItem = () => [...(viewMenu?.querySelectorAll(".menu-node-item") ?? [])].find((i) => i.textContent.includes("Show Pin Alternate Icons"));
      viewMenu?.click();
      await sleep(300);
      const was = altItem()?.getAttribute("aria-checked");
      t(was === "true", `View > Show Pin Alternate Icons is there and on, as KiCad starts (${was})`);
      await run("eeschema.EditorControl.togglePinAltIcons");
      await sleep(300);
      t(altItem()?.getAttribute("aria-checked") === "false", "it toggles off");
      await run("eeschema.EditorControl.togglePinAltIcons");
      viewMenu?.click();
      await sleep(200);
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
