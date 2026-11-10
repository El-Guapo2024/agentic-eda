// UI-LEFT-2 SS15 (PARITY-sch §83): sheets as eeschema has them --
//   * Sheet Properties' fields grid gives a sheet's user field its looks (`FIELDS_GRID_TABLE`: H Align, Bold, X Position, ...), one undo step;
//   * Sheet Pin Properties sets the pin's Font and Color (`m_fontCtrl`, `m_textColorSwatch`), one undo step;
//   * Sheet Properties' Attributes in a design variant are the sheet's record of it (`SetDNP( .., &instance, variantName )`), not its own flags;
//   * a new sheet whose file is a screen the project keeps but no sheet places is linked only after `ChangeSheetFile`'s question;
//   * Sync Sheet Pins' Add label comes back to the dialog once the label is placed (`EndPlaceItem`, `m_dialogSyncSheetPin->Show( true )`).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir>`), on a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schSheetsSs15Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schSheetsSs15Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schSheetsSs15CheckSched) {
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
    window.__schSheetsSs15CheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => __eda.state();
  const run = (id, ...args) => __eda.run(id, ...args);
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 3 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialogs = () => [...document.querySelectorAll(".dialog")];
  const dialog = () => dialogs().pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const titled = (title) => dialogs().find((d) => title.test(d.querySelector(".dialog-header")?.textContent ?? d.textContent ?? "")) ?? null;
  const opened = (title) => until(() => titled(title), 8000);
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(250); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1000); };
  const fit = async () => { await run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const variantSelect = () => document.querySelector("[data-testid=sch-variant-select]");

  /** What the scenarios touch: the root's sheets (fields, looks, pins, records), the screens the project keeps, the first sheet's labels. */
  const snapshot = async () => {
    const root = await sch("");
    const h = await json("/api/sch/hierarchy");
    const sub = root.sheets[0] ? await sch(root.sheets[0].id) : null;
    return JSON.stringify([root.sheets.map((s) => [s.id, s.user_fields, s.user_field_looks, s.props, s.variants, s.pins]), h.screens, sub?.labels.map((l) => [l.id, l.net])]);
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await toSelect(); await leave(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
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
  base = await snapshot();
  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 110 * G;
  const first = async () => (await sch("")).sheets[0];

  (async () => {
    await scenario("Sheet Properties' grid: a user field's H Align, Bold and X Position (FIELDS_GRID_TABLE), drawn where it says, one undo step", async (t) => {
      const s = await first();
      if (!t(!!s, "the root shows a sheet")) return;
      const added = await post({ op: "sch_edit", verb: "set_sheet_fields", id: s.id, fields: [...(s.user_fields ?? []), { name: "E2E_REV", text: "B", visible: true }] });
      t(added.ok !== false, "a shown user field E2E_REV");
      await sleep(800);
      await select([s.id]);
      await run("eeschema.InteractiveEdit.properties");
      const dlg = await opened(/^Sheet Properties/);
      if (!t(!!dlg, "Sheet Properties opens")) return;
      const row = (s.user_fields ?? []).length + 1;
      const cols = [...dlg.querySelectorAll("thead th")].map((th) => th.textContent.trim()).filter(Boolean);
      t(JSON.stringify(cols) === JSON.stringify(["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold"]), `the first eight columns are shown (${cols.join(", ")})`);
      setValue(aria(`Sheet field ${row} h align`, dlg), "right");
      aria(`Sheet field ${row} bold`, dlg).click();
      // the column labels' right-click shows X Position (`GRID_TRICKS::onGridLabelRightClick`)
      dlg.querySelector("thead").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 200, clientY: 200 }));
      await sleep(300);
      const item = [...document.querySelectorAll(".menu-node-item")].find((el) => /X Position/.test(el.textContent));
      t(!!item, "the column menu lists X Position");
      item?.click();
      await sleep(300);
      const x = await until(() => aria(`Sheet field ${row} x position`, dlg), 3000);
      if (!t(!!x, "the X Position column is shown")) return;
      setValue(x, "2.54");
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const after = (await sch("")).sheets.find((q) => q.id === s.id);
      const look = after?.user_field_looks?.E2E_REV;
      const f = after?.user_fields?.find((q) => q.name === "E2E_REV");
      t(look?.h === "right" && look?.dx === 2_540_000, `the field's justification and offset (${JSON.stringify(look)})`);
      t(f?.bold === true, `the field is bold (${JSON.stringify(f)})`);
      const drawn = after?.user_fields_drawn?.find((q) => q.name === "E2E_REV");
      t(drawn && Math.abs(drawn.at[0] - (s.at[0] + 2_540_000)) < 2 && drawn.h === "right" && drawn.bold, `it is drawn at the sheet's corner plus 2.54 mm, right-justified, bold (${JSON.stringify(drawn)})`);
      await undo();
      const back = (await sch("")).sheets.find((q) => q.id === s.id);
      t(!back?.user_field_looks?.E2E_REV?.dx && !back?.user_fields?.find((q) => q.name === "E2E_REV")?.bold, "one undo takes the looks back");
    });

    await scenario("Sheet Pin Properties: the pin's Font and Color (m_fontCtrl, m_textColorSwatch), one undo step", async (t) => {
      const s = await first();
      const at = { x: s.at[0], y: s.at[1] + 2 * G };
      const added = await post({ op: "sch_edit", verb: "add_sheet_pin", sheet: s.id, name: "E2E_PIN", shape: "input", at });
      t(added.ok !== false, "a pin on the sheet's left border");
      await sleep(800);
      const pin = (await sch("")).sheets.find((q) => q.id === s.id)?.pins.find((p) => p.name === "E2E_PIN");
      if (!t(!!pin, "the pin is there")) return;
      await select([pin.id]);
      await run("eeschema.InteractiveEdit.properties");
      const dlg = await opened(/^Sheet Pin Properties/);
      if (!t(!!dlg, "Sheet Pin Properties opens")) return;
      setValue(aria("Font", dlg), "KiCad Font");
      aria("Text color default", dlg).click();
      await sleep(150);
      setValue(aria("Text color", dlg), "#ff0000");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const now = (await sch("")).sheets.find((q) => q.id === s.id)?.pins.find((p) => p.id === pin.id);
      t(now?.font === "KiCad Font" && now?.color?.r === 255 && now?.color?.g === 0, `the pin's font and colour (${JSON.stringify([now?.font, now?.color])})`);
      await undo();
      const back = (await sch("")).sheets.find((q) => q.id === s.id)?.pins.find((p) => p.id === pin.id);
      t(back && !back.font && !back.color, "one undo takes them back");
    });

    await scenario("Sheet Properties in a design variant: Do not populate is the sheet's record of the variant, not its own flag", async (t) => {
      const s = await first();
      const v = await post({ op: "add_design_variant", name: "E2E_SS15", description: "" });
      t(v.ok !== false, "a design variant E2E_SS15");
      await sleep(1000);
      const sel = variantSelect();
      if (!t(!!sel, "the variant selector")) return;
      setValue(sel, "E2E_SS15");
      t(!!(await until(() => variantSelect()?.value === "E2E_SS15", 3000)), "it is the current variant");
      await sleep(400);
      await select([s.id]);
      await run("eeschema.InteractiveEdit.properties");
      const dlg = await opened(/^Sheet Properties/);
      if (!t(!!dlg, "Sheet Properties opens")) return;
      const dnp = [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim() === "Do not populate")?.querySelector("input");
      t(dnp && !dnp.checked, "Do not populate starts off");
      dnp?.click();
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const after = (await sch("")).sheets.find((q) => q.id === s.id);
      t(after?.variants?.E2E_SS15?.dnp === true, `the variant's record (${JSON.stringify(after?.variants)})`);
      t(!after?.props?.dnp, "the sheet's own flag stays off");
      setValue(sel, [...sel.options].find((o) => /Default/.test(o.value))?.value ?? "");
      await sleep(500);
      await undo();
      const removed = await post({ op: "remove_design_variant", name: "E2E_SS15" });
      t(removed.ok !== false, "the variant goes again");
      await sleep(800);
    });

    await scenario("A new sheet on a screen the project keeps but no sheet places is linked only after ChangeSheetFile's question", async (t) => {
      const a = await post({ op: "add_sheet", name: "E2E_ORPHAN", file: "e2e_orphan.kicad_sch", at: { x: X0, y: Y0 }, size: [16 * G, 8 * G] });
      t(a.ok !== false, "a sheet showing e2e_orphan.kicad_sch");
      await sleep(800);
      const orphan = (await sch("")).sheets.find((q) => q.name === "E2E_ORPHAN");
      const d = await post({ op: "sch_edit", verb: "delete_sheet", id: orphan?.id ?? "" });
      t(d.ok !== false, "the sheet is deleted");
      await sleep(800);
      const h = await json("/api/sch/hierarchy");
      t((h.screens ?? []).includes("e2e_orphan.kicad_sch") && !h.sheets.some((q) => q.file === "e2e_orphan.kicad_sch"), "the project keeps the screen, no sheet places it");
      await run("eeschema.InteractiveDrawing.drawSheet");
      await clickAt(X0, Y0);
      await clickAt(X0 + 20 * G, Y0 + 10 * G);
      const dlg = await opened(/Hierarchical Sheet Properties/);
      if (!t(!!dlg, "the new sheet's dialog opens")) return;
      setValue(dlg.querySelectorAll("input")[1], "e2e_orphan.kicad_sch");
      await sleep(150);
      buttonText(dlg, "Add").click();
      const q = await until(() => dialogs().find((x) => /already exists\./.test(x.textContent ?? "")), 4000);
      t(q && /'e2e_orphan\.kicad_sch' already exists\.\s*Link 'e2e_orphan\.kicad_sch' to this file\?/.test(q.textContent), `the question (${q?.textContent?.slice(0, 120)})`);
      if (q) buttonText(q, "Cancel")?.click();
      await sleep(500);
      t(!!titled(/Hierarchical Sheet Properties/), "Cancel keeps the dialog open");
      t(!(await sch("")).sheets.some((q2) => q2.file === "e2e_orphan.kicad_sch"), "nothing was added");
    });

    await scenario("Sync Sheet Pins' Add label comes back to the dialog once the label is placed (EndPlaceItem, Show( true ))", async (t) => {
      const s = await first();
      const added = await post({ op: "sch_edit", verb: "add_sheet_pin", sheet: s.id, name: "E2E_SYNC", shape: "output", at: { x: s.at[0], y: s.at[1] + 3 * G } });
      t(added.ok !== false, "a pin with no label inside");
      await sleep(800);
      await select([s.id]);
      await run("eeschema.InteractiveDrawing.syncSheetPins");
      const dlg = await opened(/Synchronize Sheet Pins/);
      if (!t(!!dlg, "the dialog opens")) return;
      const row = [...dlg.querySelectorAll("tr")].find((r) => r.textContent.includes("E2E_SYNC"));
      const add = row && buttonText(row, "Add label");
      if (!t(!!add, "the pin's row offers Add label")) return;
      add.click();
      const inside = await until(() => (state().sheetPath.length === 1 && state().carry ? true : null), 8000);
      t(!!inside, "the sheet is entered with the label in the hand");
      t(!titled(/Synchronize Sheet Pins/), "the dialog is hidden while it rides");
      await sleep(600);
      await clickAt(10 * G, 10 * G);
      const again = await opened(/Synchronize Sheet Pins/);
      t(!!again, "the dialog is shown again once the label is placed");
      const synced = await until(() => [...(titled(/Synchronize Sheet Pins/)?.querySelectorAll("tr") ?? [])].find((r) => r.textContent.includes("E2E_SYNC") && /In sync|Shape differs/.test(r.textContent)), 6000);
      t(!!synced, "the pin and its new label are matched");
      const labels = (await sch(s.id)).labels.filter((l) => l.net === "E2E_SYNC");
      t(labels.length === 1, `one hierarchical label E2E_SYNC inside (${labels.length})`);
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
