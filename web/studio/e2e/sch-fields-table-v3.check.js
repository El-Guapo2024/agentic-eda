// UI-LEFT-2 SS13 (PARITY-sch §77): the Symbol Fields Table's Description column, the BOM Name labels, the Group By refusals, the grid's right-button menus,
// the export's format presets with Save / Delete, and the presets, view and format written when the dialog closes (`savePresetsToSchematic`).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with the schematic tab open, on a COPY of a board: every scenario undoes what it did (and the run undoes what one left behind).
//
// It answers through `window.__schFieldsTableCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schFieldsTableCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schFieldsTableCheckSched) {
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
    window.__schFieldsTableCheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${CSS.escape(label)}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogTitled = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const closeDialogs = async () => {
    for (let i = 0; i < 6; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].reverse().find((x) => ["Cancel", "Close", "No", "OK", "Discard Changes"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(400);
    }
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const table = (body = {}) => json("/api/sch/fields_table", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  /** What the scenarios touch: the project's presets, view and formats, the symbols' fields. */
  // the rows are counted under the view the project had when the run began: a view a scenario saves is kept (no undo step), and would regroup them
  let fixedSpec = null;
  const snapshot = async () => {
    const ft = await table(fixedSpec ? { spec: fixedSpec } : {});
    return JSON.stringify({ rows: ft.rows?.map((r) => r.cells) });
  };
  // The table's presets, view and formats are kept outside the undo history (`savePresetsToSchematic`: `OnModify()`, no commit), so a scenario's are put back by a
  // `set_bom_view`, not by Undo; a view the project never had stays as the scenario left it.
  const viewOf = async () => { const ft = await table({}); return { presets: ft.user_preset_json ?? [], view: ft.bom_settings ?? null, fmts: ft.fmt_presets ?? [], fmt: ft.fmt_settings ?? null }; };
  const putBackView = async (was) => {
    const now = await viewOf();
    const cmd = { op: "set_bom_view" };
    if (!same(now.presets, was.presets)) cmd.presets = was.presets;
    if (was.view && !same(now.view, was.view)) cmd.view = was.view;
    if (!same(now.fmts, was.fmts)) cmd.fmt_presets = was.fmts;
    if (was.fmt && !same(now.fmt, was.fmt)) cmd.fmt_view = was.fmt;
    if (Object.keys(cmd).length > 1) await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 12; i++) {
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
      const viewWas = await viewOf();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); } catch { /* ignore */ }
      const left = await restore(baseline);
      await putBackView(viewWas);
      if (left < 0) { entry.notes.push("FAIL: the project could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
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
  const headers = (dlg) => [...dlg.querySelectorAll('[data-testid="fields-table-header"] th')].map((th) => th.textContent.replace(/[▲▼]/g, "").trim()).filter((x) => x);
  const contextMenu = async (el) => {
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.top + 4, button: 2 }));
    await sleep(300);
    return document.querySelector('[data-testid="fields-table-menu"]');
  };
  const menuLabels = (menu) => [...(menu?.querySelectorAll('[role="menuitem"]') ?? [])].map((b) => b.querySelector("span")?.textContent.replace(/^[✓ ] /, "").trim());
  const menuItem = (menu, label) => [...(menu?.querySelectorAll('[role="menuitem"]') ?? [])].find((b) => b.querySelector("span")?.textContent.replace(/^[✓ ] /, "").trim() === label);

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }

    fixedSpec = (await table({})).spec ?? null;
    await scenario("The Description column (LoadFieldNames' fifth mandatory field), the BOM Name naming the column, and the view kept when the dialog closes", async (t) => {
      const dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      const list = dlg.querySelector('[data-testid="fields-table-field-list"]');
      t(same([...list.querySelectorAll("thead th")].map((th) => th.textContent.trim()).filter((x) => x), ["Field", "BOM Name", "Include", "Group By"]), "the field list's columns are KiCad's");
      const desc = aria("Show Description", dlg);
      t(!!desc && !desc.checked, "Description is a field, not included at first");
      t(!!aria("Show ${ITEM_NUMBER}", dlg) && !aria("Show ${ITEM_NUMBER}", dlg).checked, "and the generated # too");
      desc.click();
      await sleep(1500);
      t(headers(dlg).includes("Description"), `the grid shows the column (${headers(dlg)})`);
      const descCells = [...dlg.querySelectorAll('input[aria-label^="Description of "]')].map((i) => i.value);
      t(descCells.some((v) => v === "Resistor"), `a resistor's is its library symbol's description (${descCells.slice(0, 4)})`);
      const bom = aria("BOM Name of Value", dlg);
      bom.focus();
      setValue(bom, "Val");
      bom.blur();
      await sleep(1200);
      t(headers(dlg).includes("Val") && !headers(dlg).includes("Value"), `the grid's column is named by its BOM Name (${headers(dlg)})`);
      buttonText(dlg, "Close").click();
      await sleep(1800);
      const ft = await table({});
      const val = ft.bom_settings?.fields_ordered?.find((f) => f.name === "Value");
      t(val?.label === "Val", `the view it closed on is the project's (bom_settings: Value labelled ${val?.label})`);
      t(ft.spec.columns.find((c) => c.name === "Description")?.show === true, "and the table opens on it again");
    });

    await scenario("Group By refuses the Quantity and the Item Number columns (OnViewControlsCellChanged)", async (t) => {
      const dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      aria("Group by ${QUANTITY}", dlg).click();
      await sleep(500);
      t(/The Quantity column cannot be grouped by\./.test(document.querySelector('[data-testid="fields-table-preset-error"]')?.textContent ?? ""), "'The Quantity column cannot be grouped by.'");
      buttonText(dialogTitled(/^Error/), "OK")?.click();
      await sleep(300);
      t(!aria("Group by ${QUANTITY}", dlg).checked, "the box stays unticked");
      aria("Group by ${ITEM_NUMBER}", dlg).click();
      await sleep(500);
      t(/The Item Number column cannot be grouped by\./.test(document.querySelector('[data-testid="fields-table-preset-error"]')?.textContent ?? ""), "'The Item Number column cannot be grouped by.'");
      buttonText(dialogTitled(/^Error/), "OK")?.click();
      await sleep(300);
    });

    await scenario("The grid's right-button menus: Select Footprint..., Show Datasheet, Copy and Paste of a cell, a header's show / hide (FIELDS_EDITOR_GRID_TRICKS)", async (t) => {
      const dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      // KiCad's own view (a view an earlier scenario saved is kept: no undo step)
      const preset = aria("Preset", dlg);
      if (preset && preset.value !== "Default Editing") { setValue(preset, "Default Editing"); await sleep(1200); }
      const fp = dlg.querySelector('input[aria-label^="Footprint of "]');
      let menu = await contextMenu(fp);
      t(same(menuLabels(menu), ["Select Footprint...", "Cut", "Copy", "Paste", "Delete", "Select All"]), `a Footprint cell: ${menuLabels(menu)}`);
      document.querySelector('[data-testid="fields-table-menu"]')?.parentElement?.click();
      await sleep(200);
      const ds = dlg.querySelector('input[aria-label^="Datasheet of "]');
      menu = await contextMenu(ds);
      t(menuLabels(menu)[0] === "Show Datasheet", `a Datasheet cell: ${menuLabels(menu)}`);
      document.querySelector('[data-testid="fields-table-menu"]')?.parentElement?.click();
      await sleep(200);
      const ref = [...dlg.querySelectorAll("tbody tr td")].find((td) => /^[A-Z]+\d/.test(td.textContent.trim()));
      menu = await contextMenu(ref);
      t(menuItem(menu, "Cut")?.disabled && menuItem(menu, "Paste")?.disabled && !menuItem(menu, "Copy")?.disabled, "a Reference cell is read-only: Copy, not Cut or Paste");
      document.querySelector('[data-testid="fields-table-menu"]')?.parentElement?.click();
      await sleep(200);
      const values = [...dlg.querySelectorAll('input[aria-label^="Value of "]')];
      const [a, b] = values;
      const want = a.value;
      t(values.length >= 2 && a.value !== b.value, `two Value cells that differ (${a?.value}, ${b?.value})`);
      menu = await contextMenu(a);
      menuItem(menu, "Copy").click();
      await sleep(200);
      const bLabel = b.getAttribute("aria-label");
      menu = await contextMenu(b);
      menuItem(menu, "Paste").click();
      await sleep(1800);
      // the row regroups with the copied one (Default Editing groups by Value): the cell of one of its symbols reads the copied value
      const bRef = bLabel.replace(/^Value of /, "").split(", ")[0];
      const now = [...dlg.querySelectorAll('input[aria-label^="Value of "]')].find((i) => i.getAttribute("aria-label").replace(/^Value of /, "").split(", ").includes(bRef));
      t(now?.value === want, `the pasted value is staged in the other cell (${bRef}: ${now?.value}, want ${want})`);
      t(!buttonText(dlg, "Apply").disabled, "and waits for Apply");
      const header = dlg.querySelector('[data-testid="fields-table-header"] th:nth-child(2)');
      menu = await contextMenu(header);
      const items = menuLabels(menu);
      t(items.includes("Description") && items.includes("Reference"), `the header's menu lists every column (${items.length})`);
      menuItem(menu, "Description").click();
      await sleep(1500);
      t(headers(dlg).includes("Description"), "picking a hidden column shows it (ShowHideColumn)");
      buttonText(dlg, "Revert").click();
      await sleep(600);
      buttonText(dlg, "Close").click();
      await sleep(1500);
    });

    await scenario("The export's format presets: Save preset... kept by the dialog, written with the format when it closes; Delete preset... (onBomFmtPresetChanged, savePresetsToSchematic)", async (t) => {
      let dlg = await openTable();
      if (!dlg) return t(false, "the table opens");
      buttonText(dlg, "Export").click();
      await sleep(800);
      const box = () => aria("Format preset", dlg);
      t(same([...box().options].map((o) => o.value), ["CSV", "Semicolons", "TSV", "---", "Save preset...", "Delete preset..."]), `the box: ${[...box().options].map((o) => o.value)}`);
      t(box().value === "CSV", "on CSV");
      const fieldDelim = [...dlg.querySelectorAll(".kv-grid input")][0];
      setValue(fieldDelim, "|");
      await sleep(400);
      t(box().value === "---", "a format of no preset shows the separator (syncBomFmtPresetSelection)");
      setValue(box(), "Save preset...");
      const save = await until(() => dialogTitled(/^Save BOM Preset/));
      t(!!save && /BOM preset name:/.test(save.innerText), "Save preset... asks 'BOM preset name:'");
      setValue(save.querySelector("input"), "TSV");
      buttonText(save, "OK").click();
      await sleep(500);
      t(/Default presets cannot be modified\./.test(document.querySelector('[data-testid="fields-table-fmt-error"]')?.textContent ?? ""), "a built-in name is refused");
      buttonText(dialogTitled(/^Error/), "OK")?.click();
      await sleep(300);
      setValue(box(), "Save preset...");
      const again = await until(() => dialogTitled(/^Save BOM Preset/));
      setValue(again.querySelector("input"), "E2E Pipes");
      buttonText(again, "OK").click();
      await sleep(600);
      t(box().value === "E2E Pipes", `the box shows it (${box().value})`);
      t(!((await table({})).fmt_presets ?? []).some((p) => p.name === "E2E Pipes"), "the project does not have it before the dialog closes");
      buttonText(dlg, "Close").click();
      await sleep(1800);
      let ft = await table({});
      t((ft.fmt_presets ?? []).some((p) => p.name === "E2E Pipes" && p.field_delimiter === "|"), "the close writes it (schematic.bom_fmt_presets)");
      t(ft.fmt_settings?.field_delimiter === "|", "and the format it closed on (bom_fmt_settings)");
      dlg = await openTable();
      buttonText(dlg, "Export").click();
      await sleep(800);
      t(box().value === "E2E Pipes", `the export opens on it (${box().value})`);
      setValue(box(), "Delete preset...");
      const del = await until(() => dialogTitled(/^Delete Preset/));
      t(!!del && /Select preset:/.test(del.innerText), "Delete preset... lists the project's presets");
      buttonText(del, "OK").click();
      await sleep(500);
      buttonText(dlg, "Close").click();
      await sleep(1800);
      ft = await table({});
      t(!(ft.fmt_presets ?? []).some((p) => p.name === "E2E Pipes"), "the close takes it out of the project");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
