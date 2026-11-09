// UI-LEFT S11 (PARITY-sch §60): the Symbol Fields Table's scope (`OnScope`: Entire Project, Current Sheet Only, Current Sheet and Down, Selection), its view
// presets (`onBomPresetChanged`: Save preset... kept by the dialog and written to the project's `schematic.bom_presets` when it closes -- UI-LEFT-2 SS13,
// `savePresetsToSchematic` --, a built-in name refused, Delete preset...; the box shows the preset the view is), a field name template as a hidden column
// (`LoadFieldNames`) and "Save changes?" on a close with staged edits.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with the schematic tab open, on a COPY of a board: every scenario undoes what it did (and the run undoes what one left behind).
//
// It answers through `window.__schFieldsPresetsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schFieldsPresetsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schFieldsPresetsCheckSched) {
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
    window.__schFieldsPresetsCheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogTitled = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const closeDialogs = async () => {
    for (let i = 0; i < 5; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].reverse().find((x) => ["Cancel", "Close", "No", "OK", "Discard Changes"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(300);
    }
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(400); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1200); };
  const dragFrom = async (x, y, dxNm, dyNm) => {
    const [px, py] = screenOf(x, y);
    const [qx, qy] = screenOf(x + dxNm, y + dyNm);
    fire("pointermove", px, py); await sleep(400);
    fire("pointerdown", px, py, { buttons: 1 }); await sleep(120);
    for (let k = 1; k <= 6; k++) { fire("pointermove", px + ((qx - px) * k) / 6, py + ((qy - py) * k) / 6, { buttons: 1 }); await sleep(120); }
    fire("pointerup", qx, qy); await sleep(1600);
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  /** Frame the items `ids` (Zoom to Selection), then leave nothing selected (the selection colour would hide the items' own). */
  const frame = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(700); await select([]); await sleep(400); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(1100); await select([]); await fit(); };
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** How many canvas pixels within `tol` of `rgb` lie in a square of `half` canvas pixels about the sheet's point (x, y) (nm). */
  const countAt = (x, y, half, rgb, tol = 40) => {
    const c = canvas();
    if (!c) return -1;
    const r = c.getBoundingClientRect();
    const v = __eda.state().view;
    const k = c.width / r.width;
    const px = Math.round((v.x + x * v.scale) * k), py = Math.round((v.y + y * v.scale) * k);
    const d = c.getContext("2d").getImageData(px - half, py - half, 2 * half + 1, 2 * half + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const table = (body = {}) => json("/api/sch/fields_table", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  /** What the scenarios touch: the project's presets and templates, the root's symbols' fields. */
  const snapshot = async () => {
    const ft = await table({});
    const setup = await json("/api/sch/setup").catch(() => ({}));
    return JSON.stringify({ presets: (ft.user_presets ?? []).map((p) => p.name), view: ft.bom_settings ?? null, templates: setup.field_name_templates ?? null, rows: ft.rows?.length });
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 16; i++) {
      if ((await snapshot()) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const openTable = async () => {
    await __eda.run("eeschema.EditorControl.editSymbolFields");
    const dlg = await until(() => dialogTitled(/^Symbol Fields Table/));
    await until(() => /\d+ rows?/.test(dlg?.innerText ?? ""));
    await sleep(500);
    return dlg;
  };
  const rowCount = (dlg) => Number((/(\d+) rows?/.exec(dlg.innerText) ?? [])[1] ?? -1);

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();
    const root0 = await sch("");

    await scenario("The scope: Entire Project, Current Sheet Only, Current Sheet and Down (OnScope, RebuildRows)", async (t) => {
      const sheet = root0.sheets.find((s) => s) ?? null;
      if (!sheet) return t(false, "a sheet");
      await enterSheet(sheet.id);
      const dlg = await openTable();
      t(!!dlg, "the table opens");
      if (!dlg) return;
      const scope = aria("Scope", dlg);
      t(scope?.value === "all" && [...scope.options].map((o) => o.textContent).join("|") === "Entire Project|Current Sheet Only|Current Sheet and Down|Selection", "it opens on the whole project, with KiCad's choices");
      const all = rowCount(dlg);
      setValue(scope, "sheet");
      await sleep(1500);
      const here = rowCount(dlg);
      setValue(scope, "sheet_and_down");
      await sleep(1500);
      const down = rowCount(dlg);
      t(all > 0 && here > 0 && here < all && down >= here, `the sheet's rows only (${all} in the project, ${here} on the sheet, ${down} on it and below)`);
      const api = await table({ scope: "sheet", sheet_path: [sheet.id] });
      t(api.rows.length === here, "as the server lists them");
      await closeDialogs();
    });

    await scenario("Save preset... keeps the view in the dialog and the project when it closes; a built-in name is refused; Delete preset... takes it out (onBomPresetChanged, savePresetsToSchematic)", async (t) => {
      const dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      const box = aria("Preset", dlg);
      t(box?.value === "Default Editing", `the box shows the view's preset (${box?.value})`);
      const opts = [...box.options].map((o) => o.value);
      t(same(opts.slice(-3), ["---", "Save preset...", "Delete preset..."]), `the separator and the two actions last (${opts})`);
      [...dlg.querySelectorAll("label.toggle")].find((l) => l.textContent.trim() === "Group symbols")?.querySelector("input")?.click();
      await sleep(1200);
      t(aria("Preset", dlg).value === "---", "a view of no preset shows the separator (syncBomPresetSelection)");
      setValue(aria("Preset", dlg), "Save preset...");
      const save = await until(() => dialogTitled(/^Save BOM Preset/));
      t(!!save && /BOM preset name:/.test(save.innerText), "Save preset... asks 'BOM preset name:'");
      if (!save) return;
      setValue(save.querySelector("input"), "E2E View");
      buttonText(save, "OK").click();
      await sleep(1800);
      t(!((await table({})).user_presets ?? []).some((p) => p.name === "E2E View"), "the dialog keeps it, the project does not have it yet (KiCad writes the presets when the dialog closes)");
      t(aria("Preset", dlg).value === "E2E View", `the box shows it (${aria("Preset", dlg).value})`);
      setValue(aria("Preset", dlg), "Save preset...");
      const again = await until(() => dialogTitled(/^Save BOM Preset/));
      setValue(again.querySelector("input"), "Attributes");
      buttonText(again, "OK").click();
      await sleep(600);
      t(/Default presets cannot be modified\./.test(document.querySelector('[data-testid="fields-table-preset-error"]')?.textContent ?? ""), "a built-in name: 'Default presets cannot be modified.'");
      buttonText(dialogTitled(/^Error/), "OK")?.click();
      await sleep(300);
      setValue(aria("Preset", dlg), "Delete preset...");
      const del = await until(() => dialogTitled(/^Delete Preset/));
      t(!!del && /Select preset:/.test(del.innerText), "Delete preset... lists the project's presets");
      if (del) buttonText(del, "OK").click();
      await sleep(800);
      t(![...aria("Preset", dlg).options].some((o) => o.value === "E2E View"), "and takes the one picked out of the box");
      // saved again, then the dialog closed: the project has it, and the view the table closed on
      setValue(aria("Preset", dlg), "Save preset...");
      const third = await until(() => dialogTitled(/^Save BOM Preset/));
      setValue(third.querySelector("input"), "E2E View");
      buttonText(third, "OK").click();
      await sleep(600);
      buttonText(dlg, "Close").click();
      await sleep(1800);
      const ft = await table({});
      t((ft.user_presets ?? []).some((p) => p.name === "E2E View"), "the close writes it to the project (schematic.bom_presets)");
      t(ft.bom_settings && ft.bom_settings.group_symbols === false, "and the view it closed on (schematic.bom_settings: Group symbols off)");
      const reopened = await openTable();
      t(reopened && aria("Preset", reopened).value === "E2E View", `the table opens on that view (${reopened && aria("Preset", reopened).value})`);
      await closeDialogs();
    });

    await scenario("A field name template is a hidden column; a close with staged edits asks 'Save changes?' and Discard Changes leaves them (LoadFieldNames, HandleUnsavedChanges)", async (t) => {
      const set = await post({ op: "set_field_name_templates", fields: [{ name: "E2E_TPL", visible: false, url: false }] });
      t(set.ok !== false, "a field name template is set");
      await sleep(900);
      const dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      const tpl = aria("Show E2E_TPL", dlg);
      t(!!tpl && tpl.checked === false, "the template is a column, not shown");
      setValue([...dlg.querySelectorAll("input")].find((i) => i.placeholder === "New field name"), "E2E_NEW");
      buttonText(dlg, "Add Field").click();
      await sleep(900);
      buttonText(dlg, "Close").click();
      const ask = await until(() => document.querySelector('[data-testid="fields-table-save-changes"]'));
      t(!!ask && ask.textContent === "Save changes?", "Close asks 'Save changes?'");
      buttonText(document, "Discard Changes")?.click();
      await sleep(900);
      t(!dialogTitled(/^Symbol Fields Table/), "Discard Changes closes the table");
      t(!(await table({})).user_fields.includes("E2E_NEW"), "and the field was not added");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
