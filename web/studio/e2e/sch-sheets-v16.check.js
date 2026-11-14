// UI-LEFT-3 S3-6, S3-7, S3-33, S3-36 (lane sch-sheets, PARITY-sch §97+):
//   * a sheet's user field is an item of its own (`SCH_FIELD` of an `SCH_SHEET`): a click on it selects it, a drag moves it alone (one undo step), and Field
//     Properties edits its text;
//   * Import Sheet of a file that shows one screen twice (KiCad's QA issue10926_1_subsheet_1: two nested sheets of issue10926_1_subsheet_1_1) brings ONE copy of
//     that screen shown by both placements (`loadHierarchy`'s `SearchHierarchy`), each placement with references of its own;
//   * a wire of that shared screen is on each placement's own net (`CONNECTION_GRAPH` per `SCH_SHEET_PATH`): the sheet in view's wire is sent with its path's net;
//   * a sheet drawn on that shared screen keeps its record of a design variant per sheet path (`SCH_SHEET::SetDNP( .., &instance, variant )`).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml with the QA files under `fixtures/` (tools/run-all-checks.sh `board=mcu30-sheet`), on
// a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schSheetsV16Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-sheet minutes=12
(async () => {
  const results = [];
  const check = (window.__schSheetsV16Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schSheetsV16CheckSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 8e6;
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
    window.__schSheetsV16CheckSched = true;
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
    return JSON.stringify([root.sheets.map((s) => [s.id, s.at, s.user_fields, s.user_field_looks, s.props, s.variants, s.pins]), h.screens, h.sheets?.map((x) => [x.path, x.file]), sub?.labels.map((l) => [l.id, l.net]), root.variants]);
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
  const first = async () => (await sch("")).sheets[0];
  const SOURCE = "fixtures/issue10926_1_subsheet_1.kicad_sch";
  const importContents = () => post({ op: "import_sheet", source: SOURCE, as_sheet: false, name: "", file: "", at: { x: 40 * G, y: 150 * G }, keep_annotations: true });
  /** The root's two sheets that show one file: the copy of the screen the imported file shows twice. */
  const sharedPair = (root) => { const by = {}; for (const s of root.sheets) (by[s.file] ??= []).push(s); return Object.values(by).find((v) => v.length === 2) ?? []; };
  const schOf = (ids) => json(`/api/schematic${ids.length > 0 ? `?sheet=${ids.map(encodeURIComponent).join("/")}` : ""}`);

  (async () => {
    await scenario("a sheet's user field is an item: clicked, dragged alone (one undo step), edited in Field Properties (SCH_FIELD of an SCH_SHEET)", async (t) => {
      const s = await first();
      if (!t(!!s, "the root shows a sheet")) return;
      const added = await post({ op: "sch_edit", verb: "set_sheet_fields", id: s.id, fields: [...(s.user_fields ?? []), { name: "E2E_REV", text: "REVISION_B", visible: true }] });
      t(added.ok !== false, "a shown user field E2E_REV");
      const placed = await post({ op: "sch_edit", verb: "set_sheet_field_looks", id: s.id, fields: [{ name: "E2E_REV", dx: 2 * G, dy: s.size[1] + 4 * G, angle: 0, h: "left", v: "center" }] });
      t(placed.ok !== false, "placed below the sheet");
      await sleep(1200);
      const id = `fld:${s.id}:E2E_REV`;
      const drawn = (await sch("")).sheets.find((q) => q.id === s.id)?.user_fields_drawn?.find((f) => f.name === "E2E_REV");
      if (!t(drawn?.id === id, `the server sends it with its id (${drawn?.id})`)) return;
      await select([]);
      await fit();
      // a click on its text picks the field, not the sheet
      await clickAt(drawn.at[0] + 3 * G, drawn.at[1]);
      const picked = state().selection.map((x) => x.id);
      t(picked.length === 1 && picked[0] === id, `the click selects the field (${picked.join(", ")})`);
      // a drag moves it alone, by its offset from the sheet's corner
      const [px, py] = screenOf(drawn.at[0] + 3 * G, drawn.at[1]);
      const [qx, qy] = screenOf(drawn.at[0] + 8 * G, drawn.at[1] + 2 * G);
      fire("pointermove", px, py); await sleep(250);
      fire("pointerdown", px, py, { buttons: 1 }); await sleep(120);
      for (let k = 1; k <= 6; k++) { fire("pointermove", px + ((qx - px) * k) / 6, py + ((qy - py) * k) / 6, { buttons: 1 }); await sleep(90); }
      fire("pointerup", qx, qy); await sleep(1500);
      const moved = (await sch("")).sheets.find((q) => q.id === s.id);
      const look = moved?.user_field_looks?.E2E_REV;
      t(look && look.dx === 7 * G && look.dy === s.size[1] + 6 * G, `the field moved by five and two grid steps (dx ${look?.dx / G}, dy ${(look?.dy - s.size[1]) / G} below the sheet)`);
      t(JSON.stringify(moved.at) === JSON.stringify(s.at), "the sheet did not move");
      await undo();
      const back = (await sch("")).sheets.find((q) => q.id === s.id)?.user_field_looks?.E2E_REV;
      t(back?.dx === 2 * G, `one undo puts it back (${back?.dx / G})`);
      // Field Properties on the field
      await select([id]);
      await run("eeschema.InteractiveEdit.properties");
      const dlg = await opened(/Edit 'E2E_REV' Field/);
      if (!t(!!dlg, "Field Properties opens, captioned as KiCad's")) return;
      const input = dlg.querySelector("input");
      t(input?.value === "REVISION_B", `its text (${input?.value})`);
      setValue(input, "REVISION_C");
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const f = (await sch("")).sheets.find((q) => q.id === s.id)?.user_fields?.find((q) => q.name === "E2E_REV");
      t(f?.text === "REVISION_C", `the sheet's field has the new text (${f?.text})`);
    });

    await scenario("Import Sheet of a file showing one screen twice: one copy, shown by both placements, each with references of its own (loadHierarchy)", async (t) => {
      const r = await importContents();
      if (!t(r.ok !== false, `the file's contents come in (${JSON.stringify(r).slice(0, 200)})`)) return;
      await sleep(1500);
      const root = await sch("");
      const nested = sharedPair(root);
      t(nested.length === 2 && nested[0].file === nested[1].file, `two sheets show one file (${nested.map((s) => s.file).join(", ")})`);
      if (nested.length !== 2) return;
      const refs = async (s) => (await schOf([s.id])).symbols.map((x) => x.reference ?? x.id).sort();
      const [a, b] = [await refs(nested[0]), await refs(nested[1])];
      t(a.length > 0 && a.every((x) => !b.includes(x)), `each placement's references are its own (${a.join(",")} / ${b.join(",")})`);
    });

    await scenario("a wire of the shared screen is on each placement's own net (CONNECTION_GRAPH per sheet path)", async (t) => {
      const r = await importContents();
      if (!t(r.ok !== false, "the file's contents come in")) return;
      await sleep(1500);
      const nested = sharedPair(await sch(""));
      if (!t(nested.length === 2, "two placements")) return;
      // a wire from the output label (on the symbol's pin 2) on the shared screen
      const lbl = (await schOf([nested[0].id])).labels.find((l) => l.net === "OUT_A");
      if (!t(!!lbl, "the screen's OUT_A label")) return;
      const w = await post({ op: "add_wire", pts: [{ x: lbl.at[0], y: lbl.at[1] }, { x: lbl.at[0] + 4 * G, y: lbl.at[1] }] }, nested[0].id);
      t(w.ok !== false, "a wire drawn on the shared screen");
      await sleep(1200);
      const [wa, wb] = [(await schOf([nested[0].id])).wires, (await schOf([nested[1].id])).wires];
      t(wa.length === 1 && wb.length === 1, "both placements show it");
      if (wa.length !== 1 || wb.length !== 1) return;
      const refA = (await schOf([nested[0].id])).symbols[0]?.reference, refB = (await schOf([nested[1].id])).symbols[0]?.reference;
      t(wa[0].pins[0] === `${refA}.2` && wb[0].pins[0] === `${refB}.2`, `on each placement it touches that placement's pin (${wa[0].pins} on ${wa[0].net} / ${wb[0].pins} on ${wb[0].net})`);
    });

    await scenario("a sheet on the shared screen keeps a variant record per sheet path (SCH_SHEET::SetDNP( .., &instance, variant ))", async (t) => {
      const r = await importContents();
      if (!t(r.ok !== false, "the file's contents come in")) return;
      await sleep(1500);
      const v = await post({ op: "add_design_variant", name: "E2E_V16", description: "" });
      t(v.ok !== false, "a design variant");
      const root = await sch("");
      const nested = sharedPair(root);
      if (!t(nested.length === 2, "two placements of the shared screen")) return;
      const inner = (await schOf([nested[0].id])).sheets;
      // a sheet drawn on the shared screen: draw one there if it has none
      let target = inner[0];
      if (!target) {
        const drawn = await post({ op: "add_sheet", name: "E2E_IN", file: "e2e_in.kicad_sch", at: { x: 10 * G, y: 10 * G }, size: [20 * G, 10 * G] }, nested[0].id);
        t(drawn.ok !== false, `a sheet drawn on the shared screen (${JSON.stringify(drawn).slice(0, 160)})`);
        await sleep(1200);
        target = (await schOf([nested[0].id])).sheets[0];
      }
      if (!t(!!target, "a sheet on the shared screen")) return;
      const set = await post({ op: "sch_edit", verb: "set_sheet_variant", id: target.id, variant: "E2E_V16", dnp: true }, nested[1].id);
      t(set.ok !== false, "Do not populate in the variant, on the second placement");
      await sleep(1200);
      const onA = (await schOf([nested[0].id])).sheets.find((s) => s.id === target.id)?.variants?.E2E_V16?.dnp ?? false;
      const onB = (await schOf([nested[1].id])).sheets.find((s) => s.id === target.id)?.variants?.E2E_V16?.dnp ?? false;
      t(onB === true && onA === false, `the record is the second placement's only (first ${onA}, second ${onB})`);
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
