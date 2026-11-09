// UI-LEFT-2 SS14 (PARITY-sch §77): Edit Text and Graphics Properties over every sheet of the hierarchy (`DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::TransferDataFromWindow`
// walks `Schematic().Hierarchy()`), with the reference, type and net filters, the text colour and a field's alignment, one undo step; and Change Symbols' reset of
// the fields in the change's own step (`processSymbols`' one commit).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with the schematic tab open, on a COPY of a board: every scenario undoes what it did (and the run undoes what one left behind).
//
// It answers through `window.__schGlobalEditCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schGlobalEditCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schGlobalEditCheckSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 4e6;
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
    window.__schGlobalEditCheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogTitled = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const closeDialogs = async () => {
    for (let i = 0; i < 5; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].reverse().find((x) => ["Cancel", "Close", "No", "OK"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(300);
    }
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(1100); await select([]); await fit(); };
  const field = (s, name) => (s?.fields ?? []).find((f) => f.name === name);
  /** Every sheet's symbols' and power symbols' fields, and the wires' strokes: what the scenarios touch. */
  const snapshot = async (sheets) => {
    const parts = [];
    for (const id of ["", ...sheets]) {
      const s = await sch(id);
      parts.push(JSON.stringify([s.symbols.map((x) => x.fields), s.power_symbols.map((x) => x.fields), s.wires.map((w) => w.stroke ?? null)]));
    }
    return parts.join("|");
  };
  const restore = async (baseline, sheets) => {
    for (let i = 0; i < 12; i++) {
      if ((await snapshot(sheets)) === baseline) return i;
      await undo();
    }
    return -1;
  };
  let sheetIds = [];
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot(sheetIds);
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline, sheetIds);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const openDialog = async () => {
    await __eda.run("eeschema.InteractiveEdit.editTextAndGraphics");
    return until(() => document.querySelector('[data-testid="sch-global-edit"]'));
  };
  const tick = (dlg, label) => { const b = aria(label, dlg); if (b && !b.checked) b.click(); };
  const ok = async (dlg) => { buttonText(dlg, "OK").click(); await sleep(1800); };

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();
    const root0 = await sch("");
    sheetIds = root0.sheets.map((s) => s.id);
    const mcu = root0.sheets.find((s) => /MCU/.test(s.name));
    const led = root0.sheets.find((s) => /LED/.test(s.name));

    await scenario("Reference designators over every sheet, from the root that has none: the text size on every sheet's symbols, one undo step (TransferDataFromWindow's Hierarchy())", async (t) => {
      const dlg = await openDialog();
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      t([...dlg.querySelectorAll('[data-testid="global-edit-scope"] input')].every((b) => !b.checked), "nothing ticked as it opens (the base sets no box)");
      tick(dlg, "Reference designators");
      setValue(aria("Text size", dlg), "2");
      await ok(dlg);
      const a = await sch(mcu.id);
      const b = await sch(led.id);
      t(field(a.symbols.find((s) => s.id === "U1"), "Reference")?.size_nm === 2_000_000, "U1 on the MCU sheet");
      t(field(b.symbols.find((s) => s.id === "R1"), "Reference")?.size_nm === 2_000_000, "R1 on the LED sheet");
      t(field(b.symbols.find((s) => s.id === "R1"), "Value")?.size_nm === 1_270_000, "Values not ticked");
      await undo();
      const back = await sch(led.id);
      t(field(back.symbols.find((s) => s.id === "R1"), "Reference")?.size_nm === 1_270_000 && field((await sch(mcu.id)).symbols.find((s) => s.id === "U1"), "Reference")?.size_nm === 1_270_000, "one undo takes both sheets back");
    });

    await scenario("By parent reference designator and By parent symbol type (a text ticks its box; Power symbols)", async (t) => {
      let dlg = await openDialog();
      if (!dlg) return t(false, "the dialog opens");
      tick(dlg, "Values");
      setValue(aria("By parent reference designator: text", dlg), "R1");
      await sleep(200);
      t(aria("By parent reference designator:", dlg).checked, "typing the filter ticks its box (OnReferenceFilterText)");
      // a three-state box cycles unchecked, checked, undetermined: from undetermined, two clicks
      aria("Bold", dlg).click();
      await sleep(100);
      aria("Bold", dlg).click();
      await sleep(100);
      t(aria("Bold", dlg).checked && !aria("Bold", dlg).indeterminate, "Bold checked after two clicks");
      await ok(dlg);
      const b = await sch(led.id);
      t(field(b.symbols.find((s) => s.id === "R1"), "Value")?.bold === true, "R1's Value is bold");
      t(!field(b.symbols.find((s) => s.id === "R2"), "Value")?.bold, "R2's is not");
      await undo();
      dlg = await openDialog();
      t(aria("By parent reference designator: text", dlg).value === "R1", "the filter text is kept for the session (g_referenceFilter)");
      setValue(aria("By parent reference designator: text", dlg), "");
      aria("By parent reference designator:", dlg).click();
      tick(dlg, "Values");
      tick(dlg, "By parent symbol type:");
      setValue(aria("Parent symbol type", dlg), "power");
      aria("Italic", dlg).click();
      await sleep(100);
      aria("Italic", dlg).click();
      await sleep(100);
      await ok(dlg);
      const c = await sch(led.id);
      t(c.power_symbols.length > 0 && c.power_symbols.every((p) => field(p, "Value")?.italic === true), "every power symbol's Value is italic");
      t(!field(c.symbols.find((s) => s.id === "R1"), "Value")?.italic, "a resistor's is not");
    });

    await scenario("By net: the wires of the net take the line width; the symbols, which have no connection, are left (visitItem)", async (t) => {
      const before = await sch(led.id);
      const net = before.wires.map((w) => w.net).find((n) => /LED1$/.test(n));
      t(!!net, `a net LED1 on the LED sheet (${net})`);
      const dlg = await openDialog();
      if (!dlg) return t(false, "the dialog opens");
      tick(dlg, "Wires & wire labels");
      tick(dlg, "Reference designators");
      setValue(aria("By net: text", dlg), "*LED1");
      setValue(aria("Line width", dlg), "0.3");
      setValue(aria("Text size", dlg), "2");
      await ok(dlg);
      const after = await sch(led.id);
      const on = after.wires.filter((w) => w.net === net);
      const off = after.wires.filter((w) => w.net !== net);
      t(on.length > 0 && on.every((w) => w.stroke?.width_nm === 300_000), `the ${on.length} wire(s) of ${net} are 0.3 mm`);
      t(off.every((w) => !w.stroke || w.stroke.width_nm !== 300_000), "the others are not");
      t(field(after.symbols.find((s) => s.id === "R1"), "Reference")?.size_nm === 1_270_000, "a symbol has no net: its Reference is left");
    });

    await scenario("Selected items only, the text colour and H Align on a field of the sheet in view", async (t) => {
      await enterSheet(led.id);
      await select(["R1"]);
      const dlg = await openDialog();
      if (!dlg) return t(false, "the dialog opens");
      tick(dlg, "Reference designators");
      tick(dlg, "Selected items only");
      setValue(aria("Text color: swatch", dlg), "#ff0000");
      setValue(aria("H Align", dlg), "left");
      await ok(dlg);
      const b = await sch(led.id);
      const r1 = field(b.symbols.find((s) => s.id === "R1"), "Reference");
      t(r1?.color && r1.color.r === 255 && r1.color.g === 0, `R1's Reference is red (${JSON.stringify(r1?.color)})`);
      t(r1?.h === "left", `and left-justified (${r1?.h})`);
      t(!field(b.symbols.find((s) => s.id === "R2"), "Reference")?.color, "R2, not selected, keeps its colour");
    });

    await scenario("Change Symbols resets the fields in the change's own step: one undo puts a field moved by hand back (processSymbols' one commit)", async (t) => {
      await enterSheet(led.id);
      const id = (await sch(led.id)).symbols.find((s) => s.id === "R1") ? "R1" : null;
      if (!id) return t(false, "R1 on the LED sheet");
      const moved = await post({ op: "sch_move", verb: "move", ids: [`fld:R1:Reference`], dx: 5_080_000, dy: 2_540_000 }, led.id);
      t(moved.ok !== false, "R1's Reference moved by hand");
      await sleep(900);
      const placed = field((await sch(led.id)).symbols.find((s) => s.id === "R1"), "Reference")?.at;
      await select(["R1"]);
      await __eda.run("eeschema.InteractiveEdit.changeSymbol");
      const dlg = await until(() => dialogTitled(/^Change Symbols/));
      if (!dlg) return t(false, "Change Symbols opens");
      setValue(dlg.querySelector("[data-change-new-id]"), "Device:R");
      await sleep(300);
      buttonText(dlg, "Change").click();
      await sleep(2500);
      const reset = field((await sch(led.id)).symbols.find((s) => s.id === "R1"), "Reference")?.at;
      t(JSON.stringify(reset) !== JSON.stringify(placed), `the field is placed again (${JSON.stringify(placed)} -> ${JSON.stringify(reset)})`);
      await closeDialogs();
      await undo();
      const back = field((await sch(led.id)).symbols.find((s) => s.id === "R1"), "Reference")?.at;
      t(JSON.stringify(back) === JSON.stringify(placed), "one undo takes the change and the reset back together");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
