// UI-LEFT-2 SS12 (PARITY-sch §78): Label Properties' Label list as `DIALOG_LABEL_PROPERTIES::TransferDataToWindow` fills it -- the labels of the same kind on
// every sheet, the power nets like a label of that kind, the bus aliases as `{alias}`; a label's field keeps its own Text Size, Bold and Show Name (Label
// Properties' grid, Field Properties); Preferences' "Power Symbols:" (`new_power_symbols`) makes the power symbol Place Power places a local one.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir>`), on a COPY of a board: every scenario undoes what it did, and the preference is put back.
//
// It answers through `window.__schLabelsSs12Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schLabelsSs12Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schLabelsSs12CheckSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 5e6;
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
    window.__schLabelsSs12CheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => __eda.state();
  const run = (id, ...args) => __eda.run(id, ...args);
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const existing = (kind) => json(`/api/sch/existing_labels?kind=${kind}`).then((r) => r.labels ?? []);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 3 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 8000);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(200); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1000); };
  const fit = async () => { await run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const carried = () => until(() => (state().carry?.drawn ? state().carry : null), 8000);
  /** Preferences' "Power Symbols:" as the studio stored it (`eda-studio.preferences.v1`). */
  const powerPref = () => { try { return JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}")?.schNewPowerSymbols ?? 0; } catch { return 0; } };
  const options = (dlg, id) => [...(dlg.querySelector(`datalist#${id}`)?.querySelectorAll("option") ?? [])].map((o) => o.value);

  /** What the scenarios touch: every sheet's labels and power symbols, the bus aliases. */
  const snapshot = async () => {
    const root = await sch("");
    const subs = await Promise.all(root.sheets.map((s) => sch(s.id)));
    const of = (s) => [s.labels.map((l) => [l.id, l.net, l.at, l.fields]), s.power_symbols.map((p) => [p.id, p.lib_id, p.at])];
    return JSON.stringify([of(root), ...subs.map(of), await existing("local"), await existing("global")]);
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
  // an empty part of the root sheet (below and left of the module sheets), on the 50 mil grid
  const X0 = 40 * G;
  const Y0 = 110 * G;

  (async () => {
    await scenario("Label Properties' Label list: every sheet's labels of the kind, the power nets like them, the bus aliases (TransferDataToWindow)", async (t) => {
      const root = await sch("");
      const sub = root.sheets[0];
      if (!t(!!sub, "the root shows a sheet")) return;
      const a = await post({ op: "sch_edit", verb: "place_label", net: "E2E_SS12_SUB", at: { x: 20 * G, y: 20 * G }, kind: { scope: "local" }, spin: "right" }, sub.id);
      const b = await post({ op: "sch_edit", verb: "place_label", net: "E2E_SS12_GLOBAL", at: { x: 20 * G, y: 30 * G }, kind: { scope: "global", shape: "input" }, spin: "right" }, sub.id);
      const c = await post({ op: "set_bus_aliases", aliases: [{ name: "E2E_SS12_BUS", members: ["E2E_A", "E2E_B"] }] });
      t(a.ok !== false && b.ok !== false && c.ok !== false, "a local and a global label on the sheet, a bus alias");
      await sleep(800);
      // the new local label's dialog, on the root
      await hoverAt(X0, Y0);
      await run("eeschema.InteractiveDrawing.placeLabel");
      let dlg = await opened(/^Label Properties/);
      if (!t(!!dlg, "L opens Label Properties")) return;
      let names = await until(() => { const o = options(dlg, "label-dialog-existing"); return o.length > 0 ? o : null; });
      names = names ?? [];
      t(names.includes("E2E_SS12_SUB"), `another sheet's local label is offered (${names.length} names)`);
      t(!names.includes("E2E_SS12_GLOBAL"), "a global label is not offered for a local one");
      t(names.includes("{E2E_SS12_BUS}"), "the bus alias is offered as {alias}");
      t(names.every((n, i) => i === 0 || names[i - 1] < n), "each name once, sorted (std::set)");
      await closeDialogs();
      await toSelect();
      // the new global label's dialog: the global labels and the global power symbols' nets
      await hoverAt(X0, Y0);
      await run("eeschema.InteractiveDrawing.placeGlobalLabel");
      dlg = await opened(/^Global Label Properties/);
      if (!t(!!dlg, "Ctrl+L opens Global Label Properties")) return;
      names = (await until(() => { const o = options(dlg, "label-dialog-existing"); return o.length > 0 ? o : null; })) ?? [];
      t(names.includes("E2E_SS12_GLOBAL") && !names.includes("E2E_SS12_SUB"), `the global label of the other sheet, not the local one (${names.slice(0, 8)})`);
      const powerNets = [...new Set([root, ...(await Promise.all(root.sheets.map((s) => sch(s.id))))].flatMap((s) => s.power_symbols.filter((p) => !/PWR_FLAG/.test(p.lib_id)).map((p) => p.net)))];
      t(powerNets.length === 0 || powerNets.some((n) => names.includes(n)), `a global power symbol's net is offered (${powerNets.slice(0, 4)})`);
      await closeDialogs();
      await toSelect();
      // an existing label's Properties offer the same list
      const s1 = await sch(sub.id);
      const lbl = s1.labels.find((l) => l.net === "E2E_SS12_GLOBAL");
      await post({ op: "sch_edit", verb: "place_label", net: "E2E_SS12_ROOT", at: { x: X0, y: Y0 }, kind: { scope: "global", shape: "input" }, spin: "right" });
      await sleep(800);
      const mine = (await sch("")).labels.find((l) => l.net === "E2E_SS12_ROOT");
      await select([mine.id]);
      await run("eeschema.InteractiveEdit.properties");
      dlg = await opened(/^Global Label Properties/);
      if (!t(!!dlg && !!lbl, "E opens Global Label Properties on a label")) return;
      names = (await until(() => { const o = options(dlg, "label-props-existing"); return o.length > 0 ? o : null; })) ?? [];
      t(names.includes("E2E_SS12_GLOBAL") && names.includes("E2E_SS12_ROOT"), `its combo offers the other sheet's global label too (${names.slice(0, 8)})`);
    });

    await scenario("A label's field keeps its Text Size, Bold and Show Name: Label Properties' grid and Field Properties set them; one undo", async (t) => {
      const placed = await post({ op: "sch_edit", verb: "place_label", net: "E2E_SS12_FLD", at: { x: X0, y: Y0 }, kind: { scope: "global", shape: "input" }, spin: "right" });
      t(placed.ok !== false, "place a global label");
      await sleep(800);
      let label = (await sch("")).labels.find((l) => l.net === "E2E_SS12_FLD");
      if (!t(!!label, "the label is on the sheet")) return;
      await post({ op: "sch_edit", verb: "edit_label", id: label.id, fields: [{ name: "Netclass", text: "E2E_CLASS", visible: true, italic: false }] });
      await sleep(900);
      await select([label.id]);
      await run("eeschema.InteractiveEdit.properties");
      let dlg = await opened(/^Global Label Properties/);
      if (!t(!!dlg, "E opens Global Label Properties")) return;
      const size = aria("Field 1 text size", dlg);
      const bold = aria("Field 1 bold", dlg);
      const showName = aria("Field 1 show name", dlg);
      if (!t(!!size && !!bold && !!showName, "the grid has Show Name, Bold and Text Size (FIELDS_GRID_TABLE)")) return;
      t(size.value === "1.27" && !bold.checked && !showName.checked, `a new field: 50 mil, regular, its value alone (${size.value})`);
      setValue(size, "2.54");
      bold.click();
      showName.click();
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      label = (await sch("")).labels.find((l) => l.id === label.id);
      const f = label.fields.find((x) => x.name === "Netclass");
      t(f?.size_nm === 2_540_000 && f?.bold === true && f?.name_shown === true, `the field keeps them (${JSON.stringify([f?.size_nm, f?.bold, f?.name_shown])})`);
      // Field Properties on the field shows them and offers them
      await select([f.id]);
      await run("eeschema.InteractiveEdit.properties");
      dlg = await opened(/^Edit 'Netclass' Field/);
      if (t(!!dlg, "E on the field opens Field Properties")) {
        const sizeRow = [...dlg.querySelectorAll("span")].find((s) => s.textContent.trim() === "Text size (mm)");
        const box = (re) => [...dlg.querySelectorAll("label")].find((l) => re.test(l.textContent))?.querySelector("input");
        t(sizeRow?.nextElementSibling?.value === "2.54" && box(/Bold/)?.checked === true && box(/Show field name/)?.checked === true, "Field Properties shows the size, Bold and Show field name");
        setValue(sizeRow.nextElementSibling, "1.778");
        box(/Bold/).click();
        await sleep(150);
        buttonText(dlg, "OK").click();
        await sleep(1300);
        const g = (await sch("")).labels.find((l) => l.id === label.id).fields.find((x) => x.name === "Netclass");
        t(g?.size_nm === 1_778_000 && g?.bold === false && g?.name_shown === true, `Field Properties sets them (${JSON.stringify([g?.size_nm, g?.bold])})`);
        await undo();
      }
      await select([]);
      await undo();
      const h = (await sch("")).labels.find((l) => l.id === label.id)?.fields.find((x) => x.name === "Netclass");
      t(h?.size_nm === 1_270_000 && !h?.bold && !h?.name_shown, "one undo takes the grid's change back");
    });

    await scenario("Preferences' Power Symbols: Local makes the power symbol Place Power places a local one, in the click's step (new_power_symbols)", async (t) => {
      const was = powerPref();
      try {
        await run("common.SuiteControl.openPreferences");
        await until(() => tid("preferences-dialog"));
        tid("prefs-page-sch_edit")?.click();
        await sleep(300);
        const choice = tid("pref-sch-power-symbols");
        if (!t(!!choice && !choice.disabled && choice.value === String(was), `Editing Options' Power Symbols: is offered (${choice?.value})`)) return;
        setValue(choice, "2");
        await sleep(150);
        tid("preferences-ok")?.click();
        await sleep(500);
        t(powerPref() === 2, "the preference is Local");
        // a rail no sheet has yet, from the chooser
        const used = new Set([...(await existing("local")), ...(await existing("global"))]);
        await hoverAt(X0 + 10 * G, Y0 + 12 * G);
        await run("eeschema.InteractiveDrawing.placePowerSymbol");
        const dlg = await opened(/^Choose Power Symbol/);
        if (!t(!!dlg, "P opens the chooser of power symbols")) return;
        const search = dlg.querySelector("[data-chooser-search]");
        setValue(search, "+");
        await until(() => /match/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? "") && !/Searching/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? ""), 60000);
        await sleep(600);
        const ids = [...dlg.querySelectorAll("[role=treeitem][data-id]")].map((r) => r.getAttribute("data-id")).filter((id) => id.includes(":"));
        const pick = ids.find((id) => !used.has(id.slice(id.indexOf(":") + 1)));
        if (!t(!!pick, `a power symbol no sheet uses (${ids.slice(0, 6)})`)) return;
        dlg.querySelector(`[role=treeitem][data-id="${pick}"]`)?.click();
        await sleep(400);
        buttonText(dlg, "OK").click();
        const c = await carried();
        t(c?.kind === "power" && c?.powerKind === "local", `it rides as a local power symbol (${JSON.stringify([c?.libId, c?.powerKind])})`);
        const s0 = await sch("");
        await clickAt(X0 + 10 * G, Y0 + 12 * G);
        await toSelect();
        const s1 = await sch("");
        const made = s1.power_symbols.filter((p) => !s0.power_symbols.some((q) => q.id === p.id));
        t(made.length === 1 && made[0].lib_id === pick, `placed (${JSON.stringify(made.map((p) => [p.id, p.lib_id]))})`);
        const net = made[0]?.net;
        const local = await existing("local");
        const global = await existing("global");
        t(local.includes(net) && !global.includes(net), `its net is a local power net: offered to a local label, not to a global one (${net})`);
        await undo();
        t((await sch("")).power_symbols.length === s0.power_symbols.length && !(await existing("local")).includes(net), "one undo takes the symbol and its kind back");
      } finally {
        await closeDialogs();
        if ((powerPref()) !== was) {
          await run("common.SuiteControl.openPreferences");
          await until(() => tid("preferences-dialog"));
          tid("prefs-page-sch_edit")?.click();
          await sleep(300);
          const choice = tid("pref-sch-power-symbols");
          if (choice) setValue(choice, String(was));
          await sleep(150);
          tid("preferences-ok")?.click();
          await sleep(400);
        }
        t((powerPref()) === was, "the preference is put back");
      }
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
