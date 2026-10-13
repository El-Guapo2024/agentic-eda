// Schematic details check (GAPS.md package P10; items 1, 12, 15, 17 and 20): a symbol's pins as items (picked, listed in the selection, the net highlight and Select Node starting from one),
// Swap Pins and Swap Pin Labels with their preference; a sheet's pins one by one (moved along the border, edited, deleted); a global label with Auto taking the spin of the wire it is on when it
// is placed, edited and moved; a free text justified, turned and mirrored as KiCad turns and mirrors it; the net-collision overlay of a held selection; the Symbol Fields Table's attribute columns,
// the Include 'DNP' Symbols option and KiCad's built-in views; a power symbol's Properties; Copy as Text; and the mouse drag settings and the auto-pan.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), with the schematic tab
// open -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it edits and undoes, and it leaves Strict off while it runs. It makes
// nothing that stays: every scenario undoes what it did (and, if it left anything behind, the run undoes until the sheets are as they were), and the preferences it changes are put back.
//
// It answers through `window.__schDetailsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a few minutes (each edit waits for the server's answer), longer than a tool call may last, so it runs on its own: read it with `await __schDetailsCheck.wait(30000)` (it returns
// when the run is done or the time is up) until `.done`. In a hidden pane the browser stops timers and animation frames; the script then drives both from a fetch loop (the `__eda.run`
// waits would never end otherwise, and the auto-pan loop would never run). A hidden pane also tells the canvas its size only after a screenshot (its ResizeObserver does not fire before):
// take one after loading the page, or the view the script works from has no size and the setup says so.
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, open a dialog with the action that opens it), the dialogs' controls
// (their `aria-label`s) and the pointer events a user makes on the canvas; the setup edits go straight to `POST /api/cmd`, on the sheet they name.
(async () => {
  const results = [];
  const check = (window.__schDetailsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, passed: results.filter((r) => r.ok).map((r) => r.name.slice(0, 40)), failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__schDetailsSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 3e6;
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
    window.__schDetailsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sheetsOf = async () => (await json("/api/schematic")).sheets;
  const sheetNamed = async (prefix) => (await sheetsOf()).find((s) => s.name.startsWith(prefix));
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const preview = (cmds, sheet) => json("/api/sch/move_preview", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sheet: sheet ?? "", cmds }) });
  /** What the scenarios touch, to compare the sheets with how they were. */
  const snapshotOf = async (sheets) => {
    const out = {};
    for (const id of ["", ...sheets]) {
      const s = await sch(id);
      out[id || "root"] = {
        symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror, x.dnp, x.exclude_from_bom, x.exclude_from_board, x.exclude_from_sim, x.pin_swaps, x.pins.map((p) => p.net)]),
        wires: s.wires.map((w) => [w.id, w.pts]),
        labels: s.labels.map((l) => [l.id, l.net, l.at, l.spin, l.auto_rotate, l.size_um]),
        texts: s.texts.map((t) => [t.id, t.content, t.at, t.angle, t.h, t.v]),
        power: s.power_symbols.map((p) => [p.id, p.net, p.at, p.rot, (p.fields ?? []).map((f) => [f.name, f.text, f.visible])]),
        sheets: s.sheets.map((x) => [x.id, x.at, x.size, x.pins.map((p) => [p.id, p.name, p.shape, p.at])]),
      };
    }
    return JSON.stringify(out);
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(500); };
  const aria = (label) => document.querySelector(`.dialog [aria-label="${label}"]`);
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = container();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  /** Where a point of the sheet (um) is on the canvas (px), from the view the hook reports. */
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(400); };
  const enter = async (id) => {
    if (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    await select([id]);
    await __eda.run("eeschema.NavigateTool.enterSheet");
    await sleep(900);
    await select([]);
    await fit();
  };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const menuOpen = () => document.querySelector("[role=menu], .menubar-dropdown") != null;

  // Preferences: the four this script changes, read and set through the dialog (the page keeps them in memory, so the stored copy alone would not do).
  const PREF = { pinSwaps: /unconstrained pin swaps/i, autoPan: /Automatically pan/i, dragMiddle: "Middle button drag", dragRight: "Right button drag" };
  const prefEl = (k) => (PREF[k] instanceof RegExp ? [...document.querySelectorAll(".dialog input[type=checkbox]")].find((i) => PREF[k].test(i.closest("label")?.textContent ?? "")) : aria(PREF[k]));
  const readPrefs = async () => {
    await __eda.run("common.SuiteControl.openPreferences");
    await sleep(500);
    const out = {};
    for (const k of Object.keys(PREF)) { const el = prefEl(k); out[k] = el == null ? null : el.type === "checkbox" ? el.checked : el.value; }
    buttonText(document.querySelector(".dialog"), "Cancel")?.click();
    await sleep(300);
    return out;
  };
  const setPrefs = async (values) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await sleep(500);
    for (const [k, v] of Object.entries(values)) {
      const el = prefEl(k);
      if (!el) throw new Error(`the Preferences dialog has no ${k}`);
      if (el.type === "checkbox") { if (el.checked !== v) el.click(); } else setValue(el, v);
      await sleep(100);
    }
    buttonText(document.querySelector(".dialog"), "OK")?.click();
    await sleep(500);
  };

  let SHEETS = [];
  /** Undo until the sheets are as they were (a scenario that left something behind, or one that undid a step too few). */
  const restore = async (baseline) => {
    for (let i = 0; i < 14; i++) {
      if ((await snapshotOf(SHEETS)) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshotOf(SHEETS);
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); } catch { /* ignore */ }
      try { if (__eda.state().tool !== "select") await escape(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the sheets could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  const mcu = await sheetNamed("MCU");
  const led = await sheetNamed("LED");
  if (!mcu || !led) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the MCU and LED channel sheets of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no board"; }
  await leave();
  await fit();
  const view0 = __eda.state().view;
  if (!view0 || !Number.isFinite(view0.x) || !Number.isFinite(view0.scale) || view0.scale < 1e-4) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)}): in a hidden pane take a screenshot after loading the page`] }); check.done = true; return "no canvas size"; }
  SHEETS = [mcu.id, led.id];
  const base = await snapshotOf(SHEETS);
  const prefs0 = await readPrefs();

  (async () => {
    // ----- pins are items
    await scenario("Pins are items: listed in the selection, the net highlight and Select Node start from one", async (t) => {
      await enter(led.id);
      const s = await sch(led.id);
      const r1 = s.symbols.find((x) => x.id === "R1");
      t(!!r1 && r1.pins.length === 2, `R1 is placed with two pins (${r1?.pins.map((p) => p.net)})`);
      await select(["pin:R1:1"]);
      const sel = __eda.state().selection;
      t(sel.length === 1 && sel[0].id === "pin:R1:1" && sel[0].kind === "pin", `a pin is an item of the selection (${JSON.stringify(sel)})`);
      // R1's pin 1 is on the PA0 net: its end is at (49530, 83820), its wire runs up from there to a hierarchical label
      await select([]);
      await hoverAt(49530, 83820);
      await __eda.run("eeschema.EditorControl.highlightNet");
      await sleep(600);
      t(__eda.state().highlight.includes("PA0"), `Highlight Net with the cursor on a pin highlights its net (${__eda.state().highlight})`);
      await __eda.run("eeschema.EditorControl.clearHighlight");
      await sleep(300);
      t(__eda.state().highlight.length === 0, "Clear Highlight clears it");
      await __eda.run("eeschema.InteractiveSelection.SelectNode");
      await sleep(500);
      t(__eda.state().selection.map((x) => x.id).join() === "pin:R1:1", `Select Node on a pin's end selects the pin (an exact hit on a pin wins over the wire there): ${__eda.state().selection.map((x) => x.id)}`);
      await hoverAt(49530, 82550);
      await __eda.run("eeschema.InteractiveSelection.SelectNode");
      await sleep(500);
      const ids = __eda.state().selection.map((x) => x.id);
      t(ids.length === 1 && ids[0].startsWith("wire_"), `and on the wire above it, the wire (${ids})`);
    });

    // ----- Swap Pins, Swap Pin Labels
    await scenario("Swap Pins and Swap Pin Labels: one undo step each; Swap Pins does nothing unless Preferences allow it", async (t) => {
      await enter(led.id);
      await setPrefs({ pinSwaps: false });
      const netsOf = (s) => s.symbols.find((x) => x.id === "R1").pins.map((p) => `${p.number}:${p.net}`).join(",");
      const r1Of = (s) => JSON.stringify(s.symbols.find((x) => x.id === "R1"));
      const before = await sch(led.id);
      // SCH_EDIT_TOOL::SwapPins: "if( !allow_unconstrained_pin_swaps ) return 0;" before it looks at the selection
      await select(["pin:R1:1", "pin:R1:2"]);
      await __eda.run("eeschema.InteractiveEdit.swapPins");
      await sleep(900);
      t(r1Of(await sch(led.id)) === r1Of(before), "with \"Allow unconstrained pin swaps\" off, Swap Pins does nothing");
      await setPrefs({ pinSwaps: true });
      // fewer than two pins: nothing
      await select(["pin:R1:1"]);
      await __eda.run("eeschema.InteractiveEdit.swapPins");
      await sleep(900);
      t(r1Of(await sch(led.id)) === r1Of(before), "a single pin trades with nothing");
      await select(["pin:R1:1", "pin:R1:2"]);
      await __eda.run("eeschema.InteractiveEdit.swapPins");
      await sleep(1200);
      const after = await sch(led.id);
      const r1 = after.symbols.find((x) => x.id === "R1");
      t(JSON.stringify(r1.pin_swaps) === JSON.stringify({ 1: "2", 2: "1" }), `with it on, R1's pins traded places (${JSON.stringify(r1.pin_swaps)})`);
      t(netsOf(before) === "1:PA0,2:LED1" && netsOf(after) === "1:LED1,2:PA0", `and the nets they are on (${netsOf(before)} -> ${netsOf(after)})`);
      t(JSON.stringify(after.wires.map((w) => w.pts)) === JSON.stringify(before.wires.map((w) => w.pts)), "the wires stayed where they were");
      t(JSON.stringify(after.symbols.find((x) => x.id === "R1").at) === JSON.stringify(before.symbols.find((x) => x.id === "R1").at), "so did the symbol");
      await undo();
      t(r1Of(await sch(led.id)) === r1Of(before), "one Undo puts the pins back");
      // Swap Pin Labels: U1's PA0 and PA1 pins each have a hierarchical label on their wire; the two labels trade their text
      await enter(mcu.id);
      const m0 = await sch(mcu.id);
      const lbl = (s, at) => s.labels.find((l) => l.at[0] === at[0] && l.at[1] === at[1])?.net;
      t(lbl(m0, [135890, 69850]) === "PA0" && lbl(m0, [135890, 72390]) === "PA1", "U1.3 has the label PA0 and U1.4 the label PA1");
      await select(["pin:U1:3", "pin:U1:4"]);
      const swapLabels = __eda.actions().find((a) => a.id === "eeschema.InteractiveEdit.swapPinLabels");
      t(swapLabels?.enabled === true, `Swap Pin Labels is offered for two pins that each have a label (${swapLabels?.reason ?? "enabled"})`);
      await __eda.run("eeschema.InteractiveEdit.swapPinLabels");
      await sleep(1200);
      const m1 = await sch(mcu.id);
      t(lbl(m1, [135890, 69850]) === "PA1" && lbl(m1, [135890, 72390]) === "PA0", `the labels traded their text (${lbl(m1, [135890, 69850])}, ${lbl(m1, [135890, 72390])})`);
      const u1 = m1.symbols.find((x) => x.id === "U1");
      t(u1.pins.find((p) => p.number === "3").net === "PA1" && u1.pins.find((p) => p.number === "4").net === "PA0", "and the pins are on each other's nets");
      t(JSON.stringify(u1.pin_swaps ?? null) === JSON.stringify(m0.symbols.find((x) => x.id === "U1").pin_swaps ?? null), "the pins did not move");
      await undo();
      t(lbl(await sch(mcu.id), [135890, 69850]) === "PA0", "one Undo puts the labels back");
      await setPrefs({ pinSwaps: prefs0.pinSwaps });
    });

    // ----- sheet pins
    await scenario("Sheet pins one by one: selected, moved along the border, edited, deleted", async (t) => {
      await leave();
      const root = await sch("");
      const sheet = root.sheets.find((s) => s.id === mcu.id);
      const pin = sheet.pins.find((p) => p.name === "PA3");
      await select([pin.id]);
      const sel = __eda.state().selection;
      t(sel.length === 1 && sel[0].id === pin.id && sel[0].kind === "sheet_pin", `a sheet pin is an item of its own (${JSON.stringify(sel)})`);
      // a move along the right border: the part of the move that leaves the border is dropped (SCH_SHEET_PIN::ConstrainOnEdge)
      const onRight = (p) => p.at[0] === sheet.at[0] + sheet.size[0];
      const moved = await post({ op: "sch_move", verb: "move", ids: [pin.id], dx: 6000, dy: 5080 });
      const p1 = (await sch("")).sheets.find((s) => s.id === mcu.id).pins.find((p) => p.id === pin.id);
      t(moved.ok === true && onRight(p1) && p1.at[1] === pin.at[1] + 5080, `the pin slid down the right border and kept its side (${JSON.stringify(pin.at)} -> ${JSON.stringify(p1.at)})`);
      await undo();
      // Sheet Pin Properties
      await select([pin.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(!!document.querySelector(".dialog") && /Sheet Pin/i.test(document.querySelector(".dialog").innerText), "Properties of a sheet pin opens its dialog");
      await closeDialogs();
      const edit = await post({ op: "sch_edit", verb: "edit_sheet_pin", id: pin.id, name: "PA3X", shape: "bidirectional" });
      const edited = (await sch("")).sheets.find((s) => s.id === mcu.id).pins.find((p) => p.id === pin.id);
      t(edit.ok === true && edited.name === "PA3X" && edited.shape === "bidirectional", `renamed and reshaped, still the same pin (${edited.name}, ${edited.shape}, ${edited.id === pin.id})`);
      await undo();
      // Delete
      await select([pin.id]);
      await __eda.run("common.Interactive.delete");
      await sleep(1000);
      t(!(await sch("")).sheets.find((s) => s.id === mcu.id).pins.find((p) => p.id === pin.id), "Delete removes the one pin");
      await undo();
      t(!!(await sch("")).sheets.find((s) => s.id === mcu.id).pins.find((p) => p.id === pin.id), "one Undo brings it back");
    });

    // ----- labels and text
    await scenario("Auto-rotate: a global label with Auto takes the spin of the wire it is on, when placed, edited and moved", async (t) => {
      await enter(led.id);
      const y0 = 38100;
      // two wires in an empty corner of the sheet: a horizontal one (ends at 60.96, 38.1) and a vertical one down from (83.82, 38.1)
      await post({ op: "add_wire", pts: [{ x: 38100, y: y0 }, { x: 60960, y: y0 }] }, led.id);
      await post({ op: "add_wire", pts: [{ x: 83820, y: y0 }, { x: 83820, y: 60960 }] }, led.id);
      const place = await post({ op: "sch_edit", verb: "place_label", net: "CLK_E2E", at: { x: 60960, y: y0 }, kind: { scope: "global", shape: "output" }, spin: "up", auto_rotate: true }, led.id);
      let l = (await sch(led.id)).labels.find((x) => x.net === "CLK_E2E");
      t(place.ok === true && l?.auto_rotate === true, `the label is placed with Auto on (${l?.auto_rotate})`);
      t((l?.spin ?? "right") === "right", `at the end of a wire that runs to the right it reads on to the right, not up as the dialog said (${l?.spin})`);
      const moved = await post({ op: "sch_move", verb: "move", ids: [l.id], dx: 22860, dy: 0 }, led.id);
      l = (await sch(led.id)).labels.find((x) => x.net === "CLK_E2E");
      t(moved.ok === true && JSON.stringify(l.at) === JSON.stringify([83820, y0]) && l.spin === "up", `moved to the top of the vertical wire it reads up (${JSON.stringify(l.at)}, ${l.spin})`);
      // Label Properties with Auto off keeps the spin picked; with it on the wire decides again
      await post({ op: "sch_edit", verb: "edit_label", id: l.id, auto_rotate: false, spin: "left" }, led.id);
      l = (await sch(led.id)).labels.find((x) => x.net === "CLK_E2E");
      t(l.spin === "left" && !l.auto_rotate, `Auto off: the spin picked is kept (${l.spin})`);
      await post({ op: "sch_edit", verb: "edit_label", id: l.id, auto_rotate: true }, led.id);
      l = (await sch(led.id)).labels.find((x) => x.net === "CLK_E2E");
      t(l.spin === "up" && l.auto_rotate, `Auto on again: the wire decides (${l.spin})`);
      // the dialog
      await select([l.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(!!aria("Auto") && aria("Auto").checked === true && aria("Orientation")?.disabled === true, "Label Properties shows Auto checked and the orientation fixed by it");
      await closeDialogs();
      // a local label has no Auto
      const refused = await post({ op: "sch_edit", verb: "place_label", net: "LOC_E2E", at: { x: 38100, y: y0 }, kind: { scope: "local" }, auto_rotate: true }, led.id);
      t(refused.ok === false, "a plain label cannot rotate itself: refused");
    });

    await scenario("A free text: placed centred, turned and mirrored as KiCad turns and mirrors it, its justification in the file's terms", async (t) => {
      await enter(led.id);
      const place = await post({ op: "sch_edit", verb: "place_text", content: "e2e note", at: { x: 40640, y: 50800 }, size_um: 1270 }, led.id);
      const x = (await sch(led.id)).texts.find((q) => q.content === "e2e note");
      t(place.ok === true && x.h === "center" && x.v === "center" && x.vertical === false, `a new text is centred on its anchor, as KiCad's last-used justification starts (${x?.h}, ${x?.v})`);
      await post({ op: "sch_edit", verb: "edit_text", id: x.id, h: "left", v: "bottom" }, led.id);
      await select([x.id]);
      const lie = async () => { const q = (await sch(led.id)).texts.find((y) => y.id === x.id); return `${q.vertical ? "up" : "along"} ${q.h}/${q.v}`; };
      t((await lie()) === "along left/bottom", `left and bottom to start (${await lie()})`);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW"); await sleep(700);
      t((await lie()) === "up left/bottom", `Rotate: the text runs up the sheet (${await lie()})`);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW"); await sleep(700);
      t((await lie()) === "along right/bottom", `Rotate again: along the sheet with the other end on the anchor, not upside down (${await lie()})`);
      await __eda.run("eeschema.InteractiveEdit.rotateCW"); await sleep(700);
      t((await lie()) === "up left/bottom", `Rotate clockwise: up again (${await lie()})`);
      await __eda.run("eeschema.InteractiveEdit.mirrorH"); await sleep(700);
      t((await lie()) === "up left/bottom", `X on a text that runs up leaves it (${await lie()})`);
      await __eda.run("eeschema.InteractiveEdit.mirrorV"); await sleep(700);
      t((await lie()) === "up right/bottom", `Y flips its justification (${await lie()})`);
      // the dialog
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(aria("Horizontal justification")?.value === "right" && aria("Vertical justification")?.value === "bottom" && aria("Orientation")?.value === "v", "Text Properties shows the justification and the orientation");
    });

    // ----- the net-collision overlay
    await scenario("The net-collision overlay: where a held wire would join two nets, and where it pulls a connection apart", async (t) => {
      await enter(led.id);
      await post({ op: "add_wire", pts: [{ x: 38100, y: 38100 }, { x: 60960, y: 38100 }] }, led.id);
      await post({ op: "add_wire", pts: [{ x: 83820, y: 38100 }, { x: 83820, y: 60960 }] }, led.id);
      const s = await sch(led.id);
      const ends = (w, x, y) => w.pts.some((p) => p[0] === x && p[1] === y);
      const wh = s.wires.find((w) => ends(w, 38100, 38100) && ends(w, 60960, 38100));
      const wv = s.wires.find((w) => ends(w, 83820, 38100) && ends(w, 83820, 60960));
      t(!!wh && !!wv, "two wires of different nets stand in an empty corner");
      // the horizontal wire carried onto the middle of the vertical one: a T of two different nets
      const hit = await preview([{ op: "sch_move", verb: "move", ids: [wh.id], dx: 22860, dy: 7620 }], led.id);
      t(JSON.stringify(hit.net_collision?.collisions.map((c) => c.at)) === JSON.stringify([[83820, 45720]]), `a circle where the T would be (${JSON.stringify(hit.net_collision?.collisions)})`);
      t(Math.abs((hit.net_collision?.collisions[0]?.radius_um ?? 0) - 1371.6) < 1, `1.5 times the 36 mil junction (${hit.net_collision?.collisions[0]?.radius_um} um)`);
      const clear = await preview([{ op: "sch_move", verb: "move", ids: [wh.id], dx: 0, dy: 5080 }], led.id);
      t(clear.net_collision?.collisions.length === 0 && clear.net_collision?.disconnections.length === 0, "short of the other wire nothing is marked");
      // a symbol carried off its wires: a mark at each pin it left
      const off = await preview([{ op: "sch_move", verb: "move", ids: ["R1"], dx: 20320, dy: 0 }], led.id);
      t(off.net_collision?.disconnections.length === 2, `R1 carried off its two wires: a mark for each (${off.net_collision?.disconnections.length})`);
      const stretched = await preview([{ op: "sch_move", verb: "drag", ids: ["R1"], dx: 20320, dy: 0, ortho: true }], led.id);
      t(stretched.net_collision?.disconnections.length === 0, "dragged, the wires follow and nothing comes apart");
    });

    // ----- Symbol Fields Table
    await scenario("Symbol Fields Table: the attribute columns, Include 'DNP' Symbols, a built-in view, one undo step", async (t) => {
      await enter(led.id);
      await __eda.run("eeschema.EditorControl.editSymbolFields");
      await sleep(1500);
      const dlg = () => document.querySelector(".dialog");
      const heads = () => [...(dlg()?.querySelectorAll("thead")[1]?.querySelectorAll("th") ?? [])].map((h) => h.textContent.trim());
      const dnpBox = (ref) => dlg().querySelector(`input[type=checkbox][aria-label^="DNP of ${ref},"], input[type=checkbox][aria-label="DNP of ${ref}"]`);
      t(["DNP", "Exclude from BOM", "Exclude from Board"].every((h) => heads().includes(h)), `the attribute columns are there (${heads()})`);
      t(!!dnpBox("R1"), "a group row has a checkbox for DNP");
      dnpBox("R1").click();
      await sleep(900);
      t(dnpBox("R1").checked === true, "ticking it stages the flag on the whole group");
      buttonText(dlg(), "Apply").click();
      await sleep(1500);
      const dnp = (await sch(led.id)).symbols.filter((x) => x.dnp).map((x) => x.id);
      t(dnp.length === 8 && dnp.every((id) => /^R\d$/.test(id)), `Apply set Do not populate on R1 to R8 (${dnp})`);
      // Include 'DNP' symbols off: the rows leave the table
      aria("Include DNP symbols").click();
      await sleep(900);
      t(!dnpBox("R1") && !!dnpBox("D1"), "without the DNP symbols the resistors are not listed (the LEDs still are)");
      aria("Include DNP symbols").click();
      await sleep(600);
      t(!!dnpBox("R1"), "with them on again they are");
      // KiCad's built-in views
      setValue(aria("Preset"), "Attributes");
      await sleep(1200);
      t(heads().includes("Exclude from Simulation") && heads().includes("Do Not Place"), `the Attributes view shows the attribute columns (${heads()})`);
      await closeDialogs();
      await sleep(300);
      await undo();
      t((await sch(led.id)).symbols.every((x) => !x.dnp), "one Undo clears every flag Apply set");
    });

    // ----- a power symbol's Properties, Copy as Text
    await scenario("A power symbol's Properties (Reference, Value as the net, Show checks, one undo step) and Copy as Text", async (t) => {
      await enter(led.id);
      const s = await sch(led.id);
      const pwr = s.power_symbols[0];
      await select([pwr.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(700);
      t(/Symbol Properties/.test(document.querySelector(".dialog")?.innerText ?? ""), "E on a power symbol opens Symbol Properties");
      t(aria("Reference")?.value === pwr.id && aria("Value")?.value === pwr.net, `it shows the reference and the net the symbol asserts (${aria("Reference")?.value}, ${aria("Value")?.value})`);
      setValue(aria("Value"), `${pwr.net}_E2E`);
      aria("Show Reference").click();
      await sleep(150);
      buttonText(document.querySelector(".dialog"), "OK").click();
      await sleep(1300);
      const after = (await sch(led.id)).power_symbols.find((p) => p.id === pwr.id);
      t(after.net === `${pwr.net}_E2E` && after.fields.find((f) => f.name === "Reference").visible === true, `OK renamed the net and showed the reference (${after.net})`);
      await undo();
      const back = (await sch(led.id)).power_symbols.find((p) => p.id === pwr.id);
      t(back.net === pwr.net && back.fields.find((f) => f.name === "Reference").visible === false, "one Undo put both back");
      // Copy as Text: the text of a text, a label and a pin, in the order selected
      const copied = [];
      const real = navigator.clipboard.writeText.bind(navigator.clipboard);
      navigator.clipboard.writeText = (txt) => { copied.push(txt); return Promise.resolve(); };
      try {
        await post({ op: "sch_edit", verb: "place_text", content: "e2e copy", at: { x: 40640, y: 50800 }, size_um: 1270 }, led.id);
        const x = (await sch(led.id)).texts.find((q) => q.content === "e2e copy");
        const label = (await sch(led.id)).labels[0];
        await select([x.id, label.id, "pin:R1:1"]);
        await __eda.run("common.Interactive.copyAsText");
        await sleep(400);
      } finally { navigator.clipboard.writeText = real; }
      t(copied.length === 1 && copied[0].startsWith("e2e copy\nPA0"), `Copy as Text: the text, then the label's text, one per line, in the order selected (${JSON.stringify(copied)})`);
    });

    // ----- view controls
    await scenario("View controls: the drag settings pan, zoom or do nothing; a right click that is no drag opens the menu; auto-pan at the edge while drawing a wire", async (t) => {
      await enter(led.id);
      await __eda.run("common.SuiteControl.openPreferences");
      await sleep(500);
      const opts = (k) => [...(aria(k)?.options ?? [])].map((o) => o.value);
      t(["pan", "zoom", "none"].every((v) => opts("Middle button drag").includes(v) && opts("Right button drag").includes(v)), `Preferences has the middle and right button drag settings (${opts("Right button drag")})`);
      await closeDialogs();
      await sleep(200);
      // the browser has no real pointer to capture for the events this script makes
      const realCapture = Element.prototype.setPointerCapture;
      Element.prototype.setPointerCapture = function () {};
      const pointerDrag = async (button, from, to) => {
        const bit = button === 2 ? 2 : button === 1 ? 4 : 1;
        fire("pointermove", from[0], from[1]); await sleep(100);
        fire("pointerdown", from[0], from[1], { button, buttons: bit }); await sleep(80);
        for (let i = 1; i <= 4; i++) { fire("pointermove", from[0] + ((to[0] - from[0]) * i) / 4, from[1] + ((to[1] - from[1]) * i) / 4, { button: -1, buttons: bit }); await sleep(60); }
        fire("pointerup", to[0], to[1], { button, buttons: 0 }); await sleep(250);
      };
      const contextMenuAt = async (x, y) => {
        const rect = container().getBoundingClientRect();
        container().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: 2 }));
        await sleep(400);
      };
      try {
        await setPrefs({ dragMiddle: "pan", dragRight: "pan" });
        let v0 = __eda.state().view;
        await pointerDrag(2, [400, 300], [460, 330]);
        let v1 = __eda.state().view;
        t(Math.abs(v1.x - v0.x - 60) < 1 && Math.abs(v1.y - v0.y - 30) < 1 && v1.scale === v0.scale, `Right button drag = Pan: the view follows the pointer (${Math.round(v1.x - v0.x)}, ${Math.round(v1.y - v0.y)})`);
        await contextMenuAt(460, 330);
        t(!menuOpen(), "the context menu the browser sends after the drag does not open");
        await contextMenuAt(300, 300);
        t(menuOpen(), "a right click that is no drag opens the context menu");
        await escape();
        v0 = __eda.state().view;
        await pointerDrag(1, [400, 300], [380, 280]);
        v1 = __eda.state().view;
        t(Math.abs(v1.x - v0.x + 20) < 1 && Math.abs(v1.y - v0.y + 20) < 1, `Middle button drag = Pan (${Math.round(v1.x - v0.x)}, ${Math.round(v1.y - v0.y)})`);
        await setPrefs({ dragRight: "zoom" });
        t(JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}").dragRight === "zoom", "the choice is saved with the preferences");
        v0 = __eda.state().view;
        await pointerDrag(2, [400, 300], [400, 240]);
        v1 = __eda.state().view;
        t(v1.scale > v0.scale * 1.2 && v1.scale < v0.scale * 1.5, `Right button drag = Zoom: up zooms in by exp(60 * 5 / 1000) (${(v1.scale / v0.scale).toFixed(3)})`);
        await setPrefs({ dragRight: "none" });
        v0 = __eda.state().view;
        await pointerDrag(2, [400, 300], [460, 330]);
        v1 = __eda.state().view;
        t(v1.x === v0.x && v1.y === v0.y && v1.scale === v0.scale, "Right button drag = nothing: the view stays");
      } finally { Element.prototype.setPointerCapture = realCapture; }
      // auto-pan: on, a wire begun, the pointer in the border at the left edge
      await setPrefs({ dragRight: prefs0.dragRight, dragMiddle: prefs0.dragMiddle, autoPan: true });
      await __eda.run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await sleep(300);
      fire("pointermove", 400, 300); await sleep(150);
      fire("pointerdown", 400, 300, { buttons: 1 }); await sleep(60); fire("pointerup", 400, 300); await sleep(300);
      const a0 = __eda.state().view;
      fire("pointermove", 2, 300);
      await sleep(1500);
      const a1 = __eda.state().view;
      t(a1.x > a0.x + 5 && Math.abs(a1.y - a0.y) < 1, `with the pointer in the left border the view pans left (x ${Math.round(a0.x)} -> ${Math.round(a1.x)})`);
      fire("pointermove", 400, 300);
      await sleep(300);
      const a2 = __eda.state().view;
      await sleep(600);
      t(Math.abs(__eda.state().view.x - a2.x) < 0.5, "and stops when the pointer leaves the border");
      await escape();
      await setPrefs({ autoPan: false });
      const b0 = __eda.state().view;
      await __eda.run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await sleep(300);
      fire("pointermove", 400, 300); await sleep(150);
      fire("pointerdown", 400, 300, { buttons: 1 }); await sleep(60); fire("pointerup", 400, 300); await sleep(300);
      fire("pointermove", 2, 300);
      await sleep(1200);
      t(Math.abs(__eda.state().view.x - b0.x) < 0.5, "with Automatically pan off the view stays");
      await escape();
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await setPrefs({ pinSwaps: prefs0.pinSwaps, autoPan: prefs0.autoPan, dragMiddle: prefs0.dragMiddle, dragRight: prefs0.dragRight });
    await leave();
    const end = await snapshotOf(SHEETS);
    results.push({ name: "end: the sheets are as they were", ok: end === base, notes: [`${end === base ? "ok" : "FAIL"}: every edit was undone`] });
    const prefsEnd = await readPrefs();
    results.push({ name: "end: the preferences are as they were", ok: JSON.stringify(prefsEnd) === JSON.stringify(prefs0), notes: [`${JSON.stringify(prefsEnd) === JSON.stringify(prefs0) ? "ok" : "FAIL"}: ${JSON.stringify(prefs0)} -> ${JSON.stringify(prefsEnd)}`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
