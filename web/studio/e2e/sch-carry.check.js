// UI-LEFT-2 SS1, SS2 and SS4 (PARITY-sch §54, §56, §59): the item a placing tool carries is drawn by the page -- it appears, rides and turns with the server's
// drawing held back (`SCH_DRAWING_TOOLS::TwoClickPlace`'s `updatePreview`, `PlaceSymbol`'s `addSymbol`), and once placed it is left selected (`AddItemToSel`); a
// symbol in the hand takes Properties (E), Field Properties (V), the attribute toggles and its own right-button menu (`PlaceSymbol` hands them to the edit tool);
// Schematic Setup > Formatting's Label offset ratio and Hop-over size are drawn on the canvas.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with KiCad's libraries (`KICAD_PATH`: Device:R), on a COPY of a board: every scenario undoes what it did (and, if it left anything behind, the run undoes until the
// root sheet is as it was).
//
// It answers through `window.__schCarryCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }. It works through `window.__eda` (the tools' actions, R, E, V,
// `__eda.state().carry` with its `drawn`, `server` and `requests`), the dialogs' controls, the pointer events a user makes on the canvas and the canvas's pixels.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__schCarryCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schCarrySched) {
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
    window.__schCarrySched = true;
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
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror, x.value, !!x.dnp]), power: s.power_symbols.map((p) => [p.id, p.at, p.net]), labels: s.labels.map((l) => [l.id, l.net, l.at]), wires: s.wires.map((w) => [w.id, w.pts]), setup: s.setup ?? null });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    if (window.__schCarryOnly && !window.__schCarryOnly.test(name)) return; // `window.__schCarryOnly = /regexp/` runs some of the scenarios only
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

  // The server's drawing of a carried item, held back (`delay` ms) and counted: the page must draw the item without it.
  const net = (window.__schCarryNet = window.__schCarryNet ?? { delay: 0, previews: 0 });
  if (!window.__schCarryFetch) {
    const real = window.fetch.bind(window);
    window.__schCarryFetch = real;
    window.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input.url;
      if (url.includes("/api/sch/place_preview")) {
        net.previews++;
        if (net.delay > 0) { const t0 = Date.now(); while (Date.now() - t0 < net.delay) await real("/api/version").then((r) => r.text()); }
      }
      return real(input, init);
    };
  }
  /** A cheap hash of the canvas's pixels over the world box `[x0, y0, x1, y1]` (nm), and the mean row of its darkest pixels (the strokes, not the grid): what the
   *  painter drew there, and how high. */
  const pixels = ([x0, y0, x1, y1]) => {
    const canvas = container().querySelector("canvas");
    const dpr = canvas.width / canvas.clientWidth;
    const [a, b] = screenOf(x0, y0);
    const [c, d] = screenOf(x1, y1);
    const img = canvas.getContext("2d").getImageData(Math.floor(Math.min(a, c) * dpr), Math.floor(Math.min(b, d) * dpr), Math.max(1, Math.ceil(Math.abs(c - a) * dpr)), Math.max(1, Math.ceil(Math.abs(d - b) * dpr)));
    let h = 0;
    let ink = 0;
    let rows = 0;
    for (let i = 0; i < img.data.length; i += 4) {
      h = (h * 31 + img.data[i] * 7 + img.data[i + 1] * 3 + img.data[i + 2]) | 0;
      if (img.data[i] + img.data[i + 1] + img.data[i + 2] < 300) {
        ink++;
        rows += Math.floor(i / 4 / img.width);
      }
    }
    return { hash: h, ink, row: ink > 0 ? rows / ink : -1 };
  };
  /** The pixels of `box` once the canvas has settled (two looks a second apart agree). */
  const settled = async (box) => { let a = pixels(box); for (let i = 0; i < 10; i++) { await sleep(1000); const b = pixels(box); if (b.hash === a.hash) return b; a = b; } return a; };
  /** Zoom on the world point (`zoomIn` about the cursor), so a fraction of a millimetre is pixels apart. */
  const zoomOn = async (x, y, steps = 6) => { for (let i = 0; i < steps; i++) { await hoverAt(x, y); await run("common.Control.zoomIn"); } await sleep(500); };
  const isSelected = (id) => state().selection.some((x) => x.id === id);
  const opensChooser = async () => { await hoverAt(X0, Y0); await run("eeschema.InteractiveDrawing.placeSymbol"); return opened(/^Symbol Chooser/); };

  (async () => {
    // `?only=<regexp>` in the page's address runs some of the scenarios only
    if (window.__schCarryOnly === undefined && new URLSearchParams(location.search).get("only")) window.__schCarryOnly = new RegExp(new URLSearchParams(location.search).get("only"));
    // ----- SS1: a label drawn by the page alone
    await scenario("A label rides and turns with no server drawing at all, and once placed it is left selected", async (t) => {
      const s0 = await sch();
      const asked = net.previews;
      await hoverAt(X0, Y0);
      await run("eeschema.InteractiveDrawing.placeLabel");
      const dlg = await opened(/^Label Properties/);
      if (!t(!!dlg, `L opens Label Properties (${state().dialogs})`)) return;
      setValue(dlg.querySelector('input[aria-label="Label"]'), "E2E_CARRY");
      await sleep(100);
      buttonText(dlg, "OK").click();
      const c = await carried();
      t(!!c && c.kind === "label" && c.drawn && c.requests === 0 && c.paintMs < 1000, `it is on the canvas at once, the server never asked (${JSON.stringify(c && { drawn: c.drawn, paintMs: c.paintMs, requests: c.requests })})`);
      await hoverAt(X0 + 6 * G, Y0 + 2 * G);
      await run("eeschema.InteractiveEdit.rotateCCW");
      const turned = await carried((x) => x.spin === "up");
      t(!!turned && turned.paintMs < 1000, `R turns it, drawn at once (${JSON.stringify(turned && { spin: turned.spin, paintMs: turned.paintMs })})`);
      t(net.previews === asked, `no place_preview was asked for a local label (${net.previews - asked})`);
      await clickAt(X0 + 6 * G, Y0 + 2 * G);
      const made = newOf(s0.labels, (await sch()).labels);
      t(made.length === 1 && made[0].net === "E2E_CARRY", `the click places it (${JSON.stringify(made.map((l) => [l.net, l.at, l.spin]))})`);
      const selected = await until(() => (made[0] && isSelected(made[0].id) ? state().selection : null), 6000);
      t(!!selected && selected.length === 1, `and it is left selected, as KiCad's tool leaves the item it carried (${JSON.stringify(state().selection)})`);
      await toSelect();
      await undo();
      t((await sch()).labels.length === s0.labels.length, "one undo takes it away");
    });

    // ----- SS1: a symbol drawn by the page while the server is slow
    await scenario("A symbol appears and turns at once while the server's drawing is held back; its fields come with it later; placed, it is left selected", async (t) => {
      const s0 = await sch();
      net.delay = 4000;
      try {
        const dlg = await opensChooser();
        if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
        buttonText(dlg, "OK").click();
        const c = await until(() => (state().carry?.drawn ? state().carry : null), 2500);
        t(!!c && c.kind === "symbol" && !c.server, `the resistor is on the canvas before the server's drawing is back (${JSON.stringify(c && { drawn: c.drawn, server: c.server, paintMs: c.paintMs })})`);
        await run("eeschema.InteractiveEdit.rotateCCW");
        const r = await until(() => (state().carry?.drawn && same(state().carry.turns, ["rot_ccw"]) ? state().carry : null), 2500);
        t(!!r && !r.server, `R turns it at once, the server's drawing of the turn not back yet (${JSON.stringify(r && { turns: r.turns, server: r.server })})`);
        const back = await until(() => (state().carry?.server ? state().carry : null), 15000);
        t(!!back, "the server's drawing (its autoplaced fields) comes back and is laid over it");
      } finally {
        net.delay = 0;
      }
      const ref = state().carry?.ref;
      await clickAt(X0, Y0);
      const made = newOf(s0.symbols, (await sch()).symbols);
      t(made.length === 1 && made[0].id === ref && made[0].rot === 90, `the click places it turned (${JSON.stringify(made.map((x) => [x.id, x.rot, x.mirror]))})`);
      const selected = await until(() => (isSelected(ref) ? state().selection : null), 6000);
      t(!!selected, `and it is left selected (${JSON.stringify(state().selection)})`);
      await toSelect();
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).length === 0, "one undo takes it away");
    });

    // ----- SS2: E, V and DNP on the symbol in the hand
    await scenario("E and V with a symbol in the hand open Symbol Properties and Field Properties on it; OK changes it there; DNP marks it; the click places it so", async (t) => {
      const s0 = await sch();
      const dlg = await opensChooser();
      if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
      buttonText(dlg, "OK").click();
      const c = await until(() => (state().carry?.server ? state().carry : null), 15000);
      if (!t(!!c, "the resistor rides, its fields drawn")) return;
      await run("eeschema.InteractiveEdit.properties");
      const props = await until(() => document.querySelector("[data-testid=symbol-properties-title]") ? dialog() : null, 6000);
      if (!t(!!props, `E opens Symbol Properties on it (${state().dialogs})`)) return;
      const inputs = [...props.querySelectorAll("input")];
      const refIn = inputs.find((i) => i.value === c.ref);
      t(!!refIn, `the dialog is the carried symbol's: Reference ${c.ref}`);
      t(!props.querySelector("[data-testid=symbol-change]")?.disabled, "Change Symbol... is enabled on the symbol in the hand too, as KiCad's dialog has it (UI-LEFT-3 S3-30)");
      const valueIn = inputs.find((i) => i.value === c.value);
      if (!t(!!valueIn, `its Value ${c.value} is there`)) return;
      setValue(valueIn, "4k7");
      await sleep(100);
      buttonText(props, "OK").click();
      const v1 = await until(() => (state().carry?.value === "4k7" && !document.querySelector("[data-testid=symbol-properties-title]") ? state().carry : null), 6000);
      t(!!v1, `OK changes the symbol in the hand (${JSON.stringify(state().carry && state().carry.value)})`);
      t((await sch()).symbols.length === s0.symbols.length, "nothing is placed yet");
      await until(() => (state().carry?.server ? state().carry : null), 15000);
      await run("eeschema.InteractiveEdit.editValue");
      const field = await opened(/^Edit Value Field/);
      if (!t(!!field, `V opens Field Properties on its Value (${state().dialogs})`)) return;
      const text = [...field.querySelectorAll("input")].find((i) => i.value === "4k7");
      if (!t(!!text, "the field shows the Value the dialog gave it")) return;
      setValue(text, "10k");
      await sleep(100);
      buttonText(field, "OK").click();
      t(!!(await until(() => (state().carry?.value === "10k" ? state().carry : null), 6000)), `OK changes it again (${state().carry?.value})`);
      await run("eeschema.EditorControl.setDNP");
      t(!!(await until(() => (state().carry?.attrs?.dnp ? state().carry : null), 4000)), `Do not Populate marks the symbol in the hand (${JSON.stringify(state().carry?.attrs)})`);
      const ref = state().carry?.ref;
      await clickAt(X0, Y0);
      const made = newOf(s0.symbols, (await sch()).symbols);
      t(made.length === 1 && made[0].id === ref && made[0].value === "10k" && made[0].dnp === true, `the click places it as it was edited (${JSON.stringify(made.map((x) => [x.id, x.value, x.dnp]))})`);
      await toSelect();
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).length === 0, "one undo takes it away, edits and all");
    });

    // ----- SS2: the right-button menu of the symbol in the hand
    await scenario("The right button with a symbol in the hand shows the symbol's menu, whose Properties opens on it", async (t) => {
      const dlg = await opensChooser();
      if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
      buttonText(dlg, "OK").click();
      const c = await carried();
      if (!t(!!c, "the resistor rides")) return;
      await contextAt(X0 + 10 * G, Y0 + 10 * G);
      const menu = document.querySelector(".menubar-dropdown");
      const items = [...(menu?.querySelectorAll(".menu-node-item") ?? [])].map((i) => i.textContent.trim());
      t(items.some((x) => /^Properties/.test(x)) && items.some((x) => /Rotate/.test(x)), `the menu is the symbol's (${items.slice(0, 8).join(" | ")})`);
      t(!!state().carry && state().selection.length === 0, "the click did not pick what is under the cursor, and the symbol still rides");
      const props = [...(menu?.querySelectorAll(".menu-node-item") ?? [])].find((i) => /^Properties/.test(i.textContent.trim()));
      props?.click();
      const opened2 = await until(() => document.querySelector("[data-testid=symbol-properties-title]") ? dialog() : null, 6000);
      t(!!opened2 && [...opened2.querySelectorAll("input")].some((i) => i.value === c.ref), "its Properties opens Symbol Properties on the carried symbol");
      await closeDialogs();
      t(!!state().carry, "Cancel leaves it in the hand");
    });

    // ----- SS4: Formatting's Label offset ratio drawn
    await scenario("Schematic Setup's Label offset ratio moves a label's text on the canvas; undone, it is drawn as before", async (t) => {
      const at = { x: X0 + 30 * G, y: Y0 + 30 * G };
      const made = await cmd({ op: "add_label", net: "E2E_OFFSET", at, kind: { scope: "local" } });
      if (!t(made.ok !== false, "a local label on the sheet")) return;
      await until(async () => (await sch()).labels.some((l) => l.net === "E2E_OFFSET"), 6000);
      await zoomOn(at.x, at.y);
      // the pointer away from the box (the cursor and what it hovers are drawn too)
      await hoverAt(at.x + 20 * G, at.y + 10 * G);
      await sleep(800);
      const box = [at.x - G, at.y - 3 * G, at.x + 8 * G, at.y + G / 2];
      const r = await cmd({ op: "set_sch_formatting", settings: { text_offset_ratio: 0.6 } });
      t(r.ok !== false, "Formatting's Label offset ratio set to 60%");
      await until(() => (state().schLook?.textOffsetRatio === 0.6 ? true : null), 8000);
      const at60 = await settled(box);
      t(state().schLook?.textOffsetRatio === 0.6, `the canvas lays text out at 60% (${JSON.stringify(state().schLook)})`);
      await undo();
      await until(() => (state().schLook?.textOffsetRatio === 0.15 ? true : null), 8000);
      const at15 = await settled(box);
      // 45% of the 1.27 mm size higher: the strokes' mean row moves up by that many pixels
      const px = 0.45 * G * state().view.scale * (container().querySelector("canvas").width / container().querySelector("canvas").clientWidth);
      t(at15.ink > 0 && Math.abs(at15.row - at60.row - px) < Math.max(1.5, px * 0.2), `at 15% the label's text stands nearer its wire by the 45% of its size (${at60.row.toFixed(1)} -> ${at15.row.toFixed(1)} rows, ${px.toFixed(1)} px expected)`);
      await run("common.Interactive.redo");
      await until(() => (state().schLook?.textOffsetRatio === 0.6 ? true : null), 8000);
      const redo60 = await settled(box);
      t(Math.abs(redo60.row - at60.row) < 0.5, `redone, it is drawn at 60% again (${at60.row.toFixed(1)} / ${redo60.row.toFixed(1)})`);
      await undo();
      await fit();
    });

    // ----- SS4: hop-overs drawn
    await scenario("Crossing wires hop over each other at Formatting's Hop-over size; at None they cross flat", async (t) => {
      const c = { x: X0 + 60 * G, y: Y0 + 20 * G };
      const h = await cmd({ op: "add_wire", pts: [{ x: c.x - 10 * G, y: c.y }, { x: c.x + 10 * G, y: c.y }], bus: false });
      const v = await cmd({ op: "add_wire", pts: [{ x: c.x, y: c.y - 10 * G }, { x: c.x, y: c.y + 10 * G }], bus: false });
      if (!t(h.ok !== false && v.ok !== false, "two wires crossing on the sheet")) return;
      await until(async () => (await sch()).wires.filter((w) => w.pts.some((p) => p[0] === c.x - 10 * G)).length > 0, 6000);
      await zoomOn(c.x, c.y);
      await hoverAt(c.x - 6 * G, c.y + 6 * G);
      await sleep(800);
      // the arc rises above the horizontal wire, right of the vertical one (Largest: 12 default line widths, 1.83 mm)
      const box = [c.x + G / 4, c.y - 2 * G, c.x + 2 * G, c.y - G / 4];
      const flat = await settled(box);
      const r = await cmd({ op: "set_sch_formatting", settings: { hop_over_size_choice: 5 } });
      t(r.ok !== false, "Hop-over size: Largest");
      await until(async () => ((await sch()).setup?.hop_over_size_choice === 5 ? true : null), 8000);
      const hopped = await settled(box);
      t(flat.ink === 0 && hopped.ink > 0, `the hop's arc is drawn above the crossing, where nothing was (${flat.ink} -> ${hopped.ink} stroke pixels)`);
      await undo();
      await until(async () => ((await sch()).setup?.hop_over_size_choice === 0 ? true : null), 8000);
      t((await settled(box)).ink === 0, "None again: flat");
      await fit();
    });

    // ----- SS5: a library text box's text on the symbol, in the hand and placed
    await scenario("A library text box's text is drawn on the symbol: in the hand, by the page, and once placed, in the same place (Analog_ADC:AD574A's 10V REF box)", async (t) => {
      const s0 = await sch();
      const dlg = await opensChooser();
      if (!t(!!dlg && (await choose(dlg, "AD574A", "Analog_ADC:AD574A")), "the chooser finds Analog_ADC:AD574A")) return;
      const at = [X0 + 40 * G, Y0];
      // the box's inside, its margin in, where a placed AD574A has it: (11.43, -38.1) to (19.05, -45.72) mm from the corner it is put down by (the server's
      // frame, `corner_symbol`), less 1.016 mm
      const inside = [at[0] + 12_446_000, at[1] + 39_116_000, at[0] + 18_034_000, at[1] + 44_704_000];
      net.delay = 30000; // the page's own drawing of the symbol in the hand, from the chooser's library drawing
      let carriedInk = -1;
      try {
        buttonText(dlg, "OK").click();
        await hoverAt(at[0], at[1]);
        const c = await carried();
        if (!t(!!c && c.libId === "Analog_ADC:AD574A" && !c.server, `it rides, drawn by the page (${JSON.stringify(c && { libId: c.libId, server: c.server })})`)) return;
        await zoomOn(at[0] + 15_000_000, at[1] + 42_000_000, 4);
        await hoverAt(at[0], at[1]);
        carriedInk = (await settled(inside)).ink;
      } finally {
        net.delay = 0;
      }
      t(carriedInk > 0, `the text is drawn in the box while the symbol rides, where the click will put it (${carriedInk} stroke pixels)`);
      await clickAt(at[0], at[1]);
      const made = newOf(s0.symbols, (await sch()).symbols);
      if (!t(made.length === 1 && made[0].lib_id === "Analog_ADC:AD574A" && same(made[0].at, at), `the click places it there (${JSON.stringify(made.map((x) => [x.id, x.at]))})`)) return;
      const lib = (await sch()).lib_symbols["Analog_ADC:AD574A"];
      const box = (lib?.graphics ?? []).find((g) => g.kind === "text_box" && g.text === "10V\nREF");
      t(!!box && same(box.start.map((v) => Math.round(v * 1000)), [11_430, -38_100]), `the sheet's drawing of the symbol has the box there (${JSON.stringify(box && [box.start, box.end, box.margin_mm])})`);
      await toSelect();
      await select([]);
      await hoverAt(at[0] - 30 * G, at[1] - 10 * G);
      const placedInk = (await settled(inside)).ink;
      t(placedInk > 0, `placed, the text is drawn in the box (${placedInk} stroke pixels)`);
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).length === 0, "one undo takes it away");
      await fit();
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
