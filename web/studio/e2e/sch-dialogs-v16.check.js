// UI-LEFT-3 lane sch-dialogs (task-v16-schdialogs, PARITY-sch section 98): S3-9, S3-10, S3-30, S3-31, S3-32 in the page.
//   * S3-9: with "Override individual item colors" on, Shape (Rule Area), Text Box and Sheet Properties open with KiCad's note; Sheet Properties warns before
//     setting a border colour (Do not show again); with it off, no note.
//   * S3-10: Edit Text & Graphics' H Align turns a label's spin (one undo step).
//   * S3-30: Symbol Properties on the symbol in the hand: the four buttons enabled, Angle and Mirror give the orientation the click places it in; the power
//     symbol's Properties open at once while the server's number is held back, and its Footprint is placed with it; a field position typed in the hand is
//     kept when automatic placement is off.
//   * S3-31: Label Properties' grid: Name .. Bold at open, X Position from the column labels' menu, a field moved there and the Sheet References made bold, one step.
//   * S3-32: the Properties panel: a text's Auto Thickness, a rule area's Fill, a power symbol's Footprint row, each one undo step.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml, on a COPY of a board: every scenario undoes what it did and the preferences
// it changes are put back. It answers through `window.__schDialogsV16Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__schDialogsV16Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => __eda.state();
  const run = (id, ...args) => __eda.run(id, ...args);
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry); i++) await escape(); };
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
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(200); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1200); };
  const fit = async () => { await run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const click = async (el, ms = 250) => { el?.click(); await sleep(ms); };
  const prefs = () => tid("preferences-dialog");
  const openPage = async (id) => { if (!prefs()) { await run("common.SuiteControl.openPreferences"); await until(() => prefs(), 4000); } await click(tid(`prefs-page-${id}`), 400); };
  const prefsOk = async () => { await click(buttonText(prefs(), "OK"), 600); };
  const storedPrefs = () => { try { return JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}"); } catch { return {}; } };
  const storedThemes = () => { try { return JSON.parse(localStorage.getItem("eda-studio.color-themes.v1") ?? "null"); } catch { return null; } };
  const choose = async (dlg, query, id) => {
    setValue(dlg.querySelector("[data-chooser-search]"), query);
    await until(() => dlg.querySelector(`[role=treeitem][data-id="${id}"]`), 40000);
    dlg.querySelector(`[role=treeitem][data-id="${id}"]`)?.click();
    await sleep(500);
    return !!dlg.querySelector(`[role=treeitem][data-id="${id}"]`);
  };
  const newOf = (before, list) => list.filter((x) => !before.some((b) => b.id === x.id && (b.unit ?? 0) === (x.unit ?? 0)));

  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror, x.value]), power: s.power_symbols.map((p) => [p.id, p.at, p.net, p.props ?? null]), labels: s.labels.map((l) => [l.id, l.net, l.at, l.spin ?? null]), texts: s.texts.map((t) => [t.id, t.thickness_nm ?? null]), graphics: (s.graphics ?? []).map((g) => [g.id, g.fill ?? null]), sheets: s.sheets.map((x) => [x.id, x.props ?? null]) });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
      try { await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`);
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1500); }
  await leave();
  await fit();
  document.querySelector('button[aria-label="Show Properties"]')?.click();
  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 110 * G;
  const mm = (v) => Math.round(v * 1e6);

  // the items the scenarios work on, where the root sheet has room: a rule area, a text box, a free text, a global label with a net class field
  const s0 = await sch();
  const known = new Set([...(s0.graphics ?? []).map((g) => g.id), ...s0.texts.map((x) => x.id), ...s0.labels.map((l) => l.id)]);
  await post({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "rule_area", pts: [{ x: mm(20), y: mm(230) }, { x: mm(40), y: mm(230) }, { x: mm(40), y: mm(245) }, { x: mm(20), y: mm(245) }] } } });
  await post({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "text_box", start: { x: mm(50), y: mm(230) }, end: { x: mm(80), y: mm(240) }, text: "v16 box", size_nm: mm(1.27) } } });
  await post({ op: "sch_edit", verb: "place_text", content: "v16 text", at: { x: mm(90), y: mm(232) }, size_nm: mm(1.27) });
  await post({ op: "sch_edit", verb: "place_label", net: "V16_GLOBAL", at: { x: mm(110), y: mm(232) }, kind: { scope: "global", shape: "input" }, spin: "right" });
  await sleep(900);
  let s1 = await sch();
  const rule = (s1.graphics ?? []).find((g) => !known.has(g.id) && g.shape.type === "rule_area");
  const box = (s1.graphics ?? []).find((g) => !known.has(g.id) && g.shape.type === "text_box");
  const text = s1.texts.find((x) => !known.has(x.id));
  const glabel = s1.labels.find((l) => !known.has(l.id));
  if (glabel) await post({ op: "sch_edit", verb: "edit_label", id: glabel.id, fields: [{ name: "Netclass", text: "V16_CLASS", visible: true, italic: false }] });
  await sleep(900);
  base = await snapshot();
  results.push({ name: "setup", ok: !!(rule && box && text && glabel), notes: [`rule ${rule?.id}, box ${box?.id}, text ${text?.id}, label ${glabel?.id}`] });

  // ----- S3-9
  await scenario("S3-9: the override note in Rule Area, Text Box and Sheet Properties; the sheet colour warning; no note with the override off", async (t) => {
    const themes0 = storedThemes();
    await openPage("sch_colors");
    if (!t(!!tid("sch-colors-page"), "Preferences > Schematic Editor > Colors")) return;
    setValue(tid("sch-colors-theme"), "user");
    await sleep(400);
    const box0 = tid("sch-colors-override");
    if (!box0?.checked) await click(box0, 300);
    await prefsOk();
    const note = () => tid("color-override-note");
    await select([rule.id]);
    await run("eeschema.InteractiveEdit.properties");
    let dlg = await opened(/Rule Area Properties/);
    t(!!dlg && note()?.textContent === "Note: individual item colors overridden in Preferences.", `Rule Area Properties shows KiCad's note (${note()?.textContent})`);
    await closeDialogs();
    await select([box.id]);
    await run("eeschema.InteractiveEdit.properties");
    dlg = await opened(/Text Box Properties/);
    t(!!dlg && !!note(), "so does Text Box Properties");
    await closeDialogs();
    const sheet = s1.sheets[0];
    await select([sheet.id]);
    await run("eeschema.InteractiveEdit.properties");
    dlg = await opened(/^Sheet Properties/);
    if (t(!!dlg && !!note(), "and Sheet Properties")) {
      const def = aria("Border color default", dlg);
      if (def?.checked) await click(def, 200);
      else setValue(aria("Border color", dlg), "#ff0000");
      await sleep(200);
      buttonText(dlg, "OK").click();
      const warn = await until(() => tid("sheet-color-warning"), 4000);
      t(!!warn && /Note: item colors are overridden in the current color theme\./.test(warn.textContent) && /Override individual item colors/.test(warn.textContent), "OK warns that item colours are overridden");
      if (warn) {
        buttonText(warn, "OK").click();
        await sleep(1200);
        const s = await sch();
        t(!!s.sheets.find((x) => x.id === sheet.id)?.props?.border_color, "its OK goes on and the colour is set");
        await undo();
      }
    }
    await closeDialogs();
    // the override off again: no note
    await openPage("sch_colors");
    const box1 = tid("sch-colors-override");
    if (box1?.checked) await click(box1, 300);
    if (themes0?.schTheme && themes0.schTheme !== "user") setValue(tid("sch-colors-theme"), themes0.schTheme);
    await sleep(300);
    await prefsOk();
    await select([rule.id]);
    await run("eeschema.InteractiveEdit.properties");
    dlg = await opened(/Rule Area Properties/);
    t(!!dlg && !note(), "with the override off there is no note");
  });

  // ----- S3-10
  await scenario("S3-10: Edit Text & Graphics' H Align Right turns a global label's spin to Left, one undo step", async (t) => {
    const r = await post({ op: "sch_edit", verb: "global_edit_text_graphics", spec: { global_labels: true, reference_filter: null, selected_only: true, selection: [glabel.id], h_align: "right" } });
    t(r.ok !== false, `the verb runs (${JSON.stringify(r).slice(0, 120)})`);
    await sleep(800);
    const l = (await sch()).labels.find((x) => x.id === glabel.id);
    t(l?.spin === "left", `right justified: the text ends at the anchor (${l?.spin})`);
    await undo();
    t(((await sch()).labels.find((x) => x.id === glabel.id)?.spin ?? "right") === "right", "one undo puts it back");
  });

  // ----- S3-31
  await scenario("S3-31: Label Properties' grid: Name .. Bold at open, X Position from the menu, a field moved and the Sheet References bold, one step", async (t) => {
    await select([glabel.id]);
    await run("eeschema.InteractiveEdit.properties");
    const dlg = await opened(/^Global Label Properties/);
    if (!t(!!dlg, "E opens Global Label Properties")) return;
    const cols = () => [...dlg.querySelectorAll("table.label-fields thead th")].map((th) => th.textContent.trim()).filter(Boolean);
    t(cols().join("|") === "Name|Value|Show|Show Name|H Align|V Align|Italic|Bold", `the columns it opens with (${cols().join("|")})`);
    dlg.querySelector("table.label-fields thead").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 200, clientY: 200 }));
    await sleep(250);
    const item = [...document.querySelectorAll(".menu-node-item")].find((el) => /X Position/.test(el.textContent));
    t(!!item && [...document.querySelectorAll(".menu-node-item")].some((el) => /Allow Autoplacement/.test(el.textContent)) && [...document.querySelectorAll(".menu-node-item")].some((el) => /Font/.test(el.textContent)), "the column menu lists X Position, Font and Allow Autoplacement");
    item?.click();
    await sleep(250);
    const x = aria("Field 1 x position", dlg);
    const isrBold = aria("Sheet References bold", dlg);
    if (!t(!!x && !!isrBold, "the net class's X Position and the Sheet References' Bold cells")) return;
    setValue(x, "5.08");
    isrBold.click();
    await sleep(200);
    buttonText(dlg, "OK").click();
    await sleep(1500);
    const l = (await sch()).labels.find((y) => y.id === glabel.id);
    const nc = l?.fields?.find((f) => f.name === "Netclass");
    const isr = l?.fields?.find((f) => f.intersheet_refs);
    t(!!nc && Math.abs(nc.at[0] - l.at[0] - mm(5.08)) < 100 && l.fields_hand_placed === true, `the net class is 5.08 mm right of the anchor and the fields are placed by hand (${nc && nc.at[0] - l.at[0]})`);
    t(isr?.bold === true, `the Sheet References are bold (${isr?.bold})`);
    await undo();
    const back = (await sch()).labels.find((y) => y.id === glabel.id);
    t(!back?.fields_hand_placed && !back?.fields?.find((f) => f.intersheet_refs)?.bold, "one undo takes both back");
  });

  // ----- S3-32
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const lastAct = async () => (await json("/api/state")).activity[0] ?? null;
  const editRow = async (name, value) => {
    for (let i = 0; i < 25 && !input(name); i++) await sleep(200);
    const el = input(name);
    if (!el) return `no editable row ${name}${row(name) ? ` (read-only: ${row(name).dataset.note ?? ""})` : ""}`;
    const before = await lastAct();
    if (el instanceof HTMLSelectElement) setValue(el, value);
    else if (el.type === "checkbox") { if (el.checked === !!value) return "already"; el.click(); }
    else { el.focus(); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(30); el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); }
    const t0 = Date.now();
    while (Date.now() - t0 < 10000) { const a = await lastAct(); if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(400); return a.ok ? "ok" : `REFUSED: ${a.message}`; } await sleep(60); }
    return "no command sent";
  };
  await scenario("S3-32: a text's Auto Thickness, a rule area's Fill, a power symbol's Footprint, each one undo step", async (t) => {
    await select([text.id]);
    let r = await editRow("Auto Thickness", false);
    let s = await sch();
    t(r === "ok" && (s.texts.find((x) => x.id === text.id)?.thickness_nm ?? 0) > 0, `Auto Thickness unticked keeps a pen of its own (${r}, ${s.texts.find((x) => x.id === text.id)?.thickness_nm})`);
    await undo();
    await select([rule.id]);
    r = await editRow("Fill", "solid");
    s = await sch();
    t(r === "ok" && (s.graphics.find((g) => g.id === rule.id)?.fill ?? "none") !== "none", `a rule area's Fill is set (${r}, ${s.graphics.find((g) => g.id === rule.id)?.fill})`);
    await undo();
    await post({ op: "add_power_symbol", lib_id: "power:GND", at: { x: mm(150), y: mm(232) }, rot_millideg: 0, net: "GND", pin: "" });
    await sleep(900);
    const pwr = (await sch()).power_symbols.find((p) => !s1.power_symbols.some((q) => q.id === p.id));
    if (t(!!pwr, `a power symbol to edit (${pwr?.id})`)) {
      await select([pwr.id]);
      r = await editRow("Footprint", "V16:FP");
      s = await sch();
      t(r === "ok" && s.power_symbols.find((p) => p.id === pwr.id)?.props?.footprint === "V16:FP", `its Footprint row sets it (${r})`);
      await undo();
    }
  });

  // ----- S3-30
  const opensChooser = async () => { await hoverAt(X0, Y0); await run("eeschema.InteractiveDrawing.placeSymbol"); return opened(/^Symbol Chooser/); };
  await scenario("S3-30: Symbol Properties in the hand: the four buttons, Angle +90 and Mirror Around Y axis placed with it", async (t) => {
    const before = await sch();
    const dlg = await opensChooser();
    if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
    buttonText(dlg, "OK").click();
    const c = await until(() => (state().carry?.server ? state().carry : null), 15000);
    if (!t(!!c, "the resistor rides")) return;
    await run("eeschema.InteractiveEdit.properties");
    const props = await until(() => (tid("symbol-properties-title") ? dialog() : null), 6000);
    if (!t(!!props, "E opens Symbol Properties on it")) return;
    t(["symbol-update-from-library", "symbol-change", "symbol-edit", "symbol-edit-library"].every((id) => tid(id) && !tid(id).disabled), "the four buttons are enabled, as KiCad's");
    t(!tid("symbol-angle").disabled && !tid("symbol-mirror").disabled, "Angle and Mirror are offered");
    setValue(tid("symbol-angle"), "90");
    setValue(tid("symbol-mirror"), "y");
    await sleep(150);
    buttonText(props, "OK").click();
    const turned = await until(() => (state().carry?.rotDeg === 90 && state().carry?.mirror === "y" ? state().carry : null), 6000);
    t(!!turned, `the symbol in the hand turns (${JSON.stringify([state().carry?.rotDeg, state().carry?.mirror])})`);
    const ref = state().carry?.ref;
    await clickAt(X0, Y0);
    const made = newOf(before.symbols, (await sch()).symbols);
    t(made.length === 1 && made[0].id === ref && made[0].rot === 90 && made[0].mirror === "y", `the click places it so (${JSON.stringify(made.map((x) => [x.id, x.rot, x.mirror]))})`);
    await toSelect();
    await undo();
  });

  await scenario("S3-30: the power symbol's Properties open at once with the server's number held back; its Footprint goes with the click", async (t) => {
    const real = window.fetch.bind(window);
    let hold = true;
    window.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input.url;
      if (hold && url.includes("/api/sch/place_preview")) { const t0 = Date.now(); while (hold && Date.now() - t0 < 20000) await real("/api/version").then((r) => r.text()); }
      return real(input, init);
    };
    try {
      const before = await sch();
      await hoverAt(X0 + 10 * G, Y0);
      await run("eeschema.InteractiveDrawing.placePowerSymbol");
      const dlg = await opened(/^Choose Power Symbol/);
      if (!t(!!dlg && (await choose(dlg, "GND", "power:GND")), "the chooser finds power:GND")) return;
      buttonText(dlg, "OK").click();
      await until(() => state().carry?.kind === "power", 6000);
      t(!state().carry?.server, "the server's drawing is held back");
      await run("eeschema.InteractiveEdit.properties");
      const props = await until(() => (tid("power-general") ? dialog() : null), 4000);
      if (!t(!!props, "E opens the power symbol's Properties at once")) return;
      const fp = aria("Footprint", props);
      if (!t(!!fp, "its Footprint")) return;
      setValue(fp, "V16:PWR");
      await sleep(150);
      buttonText(props, "OK").click();
      await until(() => state().carry?.edits > 0, 4000);
      t(state().carry?.edits > 0, `the power symbol in the hand keeps the edit (${state().carry?.edits})`);
      hold = false;
      await clickAt(X0 + 10 * G, Y0);
      await sleep(1500);
      const made = (await sch()).power_symbols.filter((p) => !before.power_symbols.some((q) => q.id === p.id));
      t(made.length === 1 && made[0].props?.footprint === "V16:PWR", `placed with its Footprint (${JSON.stringify(made.map((p) => [p.id, p.props?.footprint]))})`);
      await toSelect();
      await undo();
    } finally {
      hold = false;
      window.fetch = real;
    }
  });

  await scenario("S3-30: a field position typed in the hand is kept with automatic placement off, turned with the symbol", async (t) => {
    const was = storedPrefs().schAutoplaceFields ?? true;
    const setAutoplace = async (on) => {
      await openPage("sch_edit");
      const box = tid("pref-sch-autoplace-fields");
      if (box && box.checked !== on) await click(box, 200);
      await prefsOk();
    };
    try {
      await setAutoplace(false);
      t(storedPrefs().schAutoplaceFields === false, "Automatically place symbol fields is off");
      for (const turn of [false, true]) {
        const before = await sch();
        const dlg = await opensChooser();
        if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
        buttonText(dlg, "OK").click();
        await until(() => (state().carry?.server ? state().carry : null), 15000);
        if (turn) { await run("eeschema.InteractiveEdit.rotateCCW"); await until(() => (state().carry?.server ? state().carry : null), 15000); }
        await run("eeschema.InteractiveEdit.editValue");
        const field = await opened(/^Edit Value Field/);
        if (!t(!!field, "V opens Field Properties on its Value")) return;
        const xRow = [...field.querySelectorAll("span")].find((s) => s.textContent.trim() === "Position X (mm)")?.nextElementSibling;
        const x0 = Number(xRow?.value);
        const yRow = [...field.querySelectorAll("span")].find((s) => s.textContent.trim() === "Position Y (mm)")?.nextElementSibling;
        const y0 = Number(yRow?.value);
        setValue(xRow, String(Math.round((x0 + 2.54) * 1e4) / 1e4));
        await sleep(150);
        buttonText(field, "OK").click();
        await sleep(400);
        // where the drawing has the Value now, from the symbol's place, before the click
        const ref = state().carry?.ref;
        await clickAt(X0, Y0);
        const after = (await until(async () => { const a = await sch(); return newOf(before.symbols, a.symbols)[0]?.fields?.length ? a : null; }, 8000)) ?? (await sch());
        const made = newOf(before.symbols, after.symbols)[0];
        const v = made?.fields?.find((f) => f.name === "Value");
        t(!!v, `placed with its Value (${ref}: ${JSON.stringify(newOf(before.symbols, after.symbols).map((x) => [x.id, (x.fields ?? []).map((f) => f.name)]))})`);
        // placed where it was drawn (the click at the point it rode at): the Value where it was typed
        t(!!v && Math.abs(v.at[0] - mm(x0 + 2.54)) < 200 && Math.abs(v.at[1] - mm(y0)) < 200, `${turn ? "turned a quarter: " : ""}the Value is where it was typed (${v && [v.at[0] - mm(x0), v.at[1] - mm(y0)]})`);
        await toSelect();
        await restore();
      }
    } finally {
      await setAutoplace(was);
    }
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
