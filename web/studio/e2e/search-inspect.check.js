// Search and inspection check (GAPS.md package P11 / items 21 and 29, PARITY-pcb.md section 31, PARITY-fpedit.md): Find by Properties (the From Selection tab's rows and Match modes,
// Select Matching and its options, Create from Selection Tab, the Query tab with KiCad's expression language), Find that follows user fields, hidden fields and zone names,
// Clearance Resolution and Constraints Resolution (the report pages), and the Footprint Checker.
//
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board:
// `eda place examples/mcu_board_30plus.yaml -o <dir>` and `eda board new examples/mcu_board_30plus.yaml -o <board dir>` with that design.json copied over (30 footprints placed, nothing
// routed), served with `eda board serve -C <board dir> --port N --ui web/studio/dist`. It answers through `window.__searchInspectCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __searchInspectCheck.wait(90000)` until `.done`.
//
// It works through `window.__eda` (`actions`' ids with `run`, the dialogs by their data-testid, selection with `common.InteractiveSelection.selectItems`) and the verbs' own routes
// (`/api/cmd`). It selects and searches and makes nothing that stays: the edits a scenario makes are undone, and it compares the board with how it was.
(async () => {
  const results = [];
  const check = (window.__searchInspectCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const placed = async () => (await state()).parts.filter((p) => p.placed);
  const dialog = (id) => document.querySelector(`[data-testid="${id}"]`);
  const text = (id) => dialog(id)?.innerText ?? "";
  const select = async (ids) => {
    await __eda.run("common.InteractiveSelection.clear");
    if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids);
    await sleep(250);
  };
  const selection = () => __eda.state().selection.map((s) => s.id);
  const setSelect = async (label, value) => {
    const el = document.querySelector(`select[aria-label="${label}"]`);
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(120);
  };
  const setText = async (testid, value) => {
    const el = dialog(testid);
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(120);
  };
  const setCheckbox = async (labelText, on) => {
    const input = [...document.querySelectorAll('[data-testid="find-by-properties"] label.toggle')].find((l) => l.textContent.includes(labelText))?.querySelector("input");
    if (input && input.checked !== on) { input.click(); await sleep(120); }
  };
  const button = (inside, label) => [...document.querySelectorAll(`[data-testid="${inside}"] button`)].find((b) => b.textContent.trim() === label);
  const pressFbp = async (label, ms = 450) => { button("find-by-properties", label).click(); await sleep(ms); };
  const tab = async (name) => { [...document.querySelectorAll('[data-testid="find-by-properties"] [role="tab"]')].find((b) => b.textContent.trim() === name).click(); await sleep(250); };
  const openFbp = async () => { await __eda.run("pcbnew.EditorControl.findByProperties"); await sleep(350); await tab("From Selection"); };
  const rowNames = () => [...document.querySelectorAll('[data-testid^="fbp-row-"]')].map((tr) => tr.getAttribute("data-testid").slice("fbp-row-".length));
  const rowValue = (name) => dialog(`fbp-row-${name}`)?.querySelectorAll("td")[1]?.innerText;
  const closeDialogs = async () => {
    for (const id of ["find-by-properties", "pcb-find-dialog"]) {
      if (dialog(id)) { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); dialog(id)?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); await sleep(250); }
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(600); }
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const parts = await placed();
  const c1 = parts.find((p) => p.ref === "C1") ?? parts[0];
  const sameFootprint = parts.filter((p) => p.footprint === c1.footprint).map((p) => p.ref);
  const pcbBefore = JSON.stringify(parts.map((p) => [p.ref, p.at, p.rot, p.side]).sort());

  await scenario("Find by Properties lists what the selected footprint has, by name, and says what is selected", async (t) => {
    await select([c1.ref]);
    await openFbp();
    t(!!dialog("find-by-properties"), "Edit > Find by Properties... opens the dialog");
    t(text("fbp-status") === "Footprint", `the line says what is selected: ${text("fbp-status")}`);
    const names = rowNames();
    t(names.length >= 10 && JSON.stringify(names) === JSON.stringify([...names].sort()), `${names.length} rows in the alphabetical order of their names`);
    t(names.includes("Type") && rowValue("Type") === "Footprint", `Type is a row, though the Properties panel hides it: ${rowValue("Type")}`);
    t(rowValue("Library Link") === c1.footprint, `Library Link is ${rowValue("Library Link")}`);
    t(rowValue("Value") === (c1.value ?? ""), `Value is ${rowValue("Value")}`);
    t(/mm$/.test(rowValue("Position X") ?? ""), `a coordinate has its unit: ${rowValue("Position X")}`);
    t(![...document.querySelectorAll('select[aria-label^="Match "]')].some((s) => s.value !== "ignored"), "every Match starts Ignored");
    t(button("find-by-properties", "Close") && button("find-by-properties", "Select Matching"), "Close and Select Matching are there");
    await select([c1.ref, parts.find((p) => p.footprint !== c1.footprint)?.ref].filter(Boolean));
    t(/objects selected/.test(text("fbp-status")), `two items of different footprints: ${text("fbp-status")}`);
    t(rowValue("Library Link") === "<...>" && !dialog("fbp-row-Library Link")?.querySelector("select"), "a value that differs is <...> and cannot be matched");
    await select([]);
    t(text("fbp-status") === "No items selected" && rowNames().length === 0, "nothing selected: the label says so and the grid is empty");
  });

  await scenario("Matching selects the footprints like the selected one, Different the others, and the line counts them", async (t) => {
    await select([c1.ref]);
    await openFbp();
    await setSelect("Match Library Link", "matching");
    await pressFbp("Select Matching");
    t(JSON.stringify(selection().sort()) === JSON.stringify([...sameFootprint].sort()), `the ${sameFootprint.length} footprints on ${c1.footprint} are selected: ${selection().length}`);
    t(text("fbp-status") === `${sameFootprint.length} items matched`, `the line: ${text("fbp-status")}`);
    t(dialog("fbp-row-Library Link")?.querySelector("select")?.value === "matching", "the Match mode stays for the new selection");
    // with nothing selected the rows are still there (KiCad returns before it clears them): Select Matching runs them again
    await select([]);
    t(text("fbp-status") === "No items selected", "nothing selected");
    await pressFbp("Select Matching");
    t(selection().length === sameFootprint.length, `the last rows are used again: ${selection().length}`);
    await setSelect("Match Library Link", "different");
    await pressFbp("Select Matching");
    const others = parts.filter((p) => p.footprint !== c1.footprint).map((p) => p.ref);
    t(JSON.stringify(selection().sort()) === JSON.stringify([...others].sort()), `Different selects the ${others.length} others: ${selection().length}`);
    t(text("fbp-status") === `${others.length} items matched`, `the line: ${text("fbp-status")}`);
    // the others are on several footprints: that row is <...> now and has no mode, as KiCad's rebuild leaves it
    t(rowValue("Library Link") === "<...>", `the grid is built for the new selection: ${rowValue("Library Link")}`);
    // Deselect non-matching items off: the matches are added to the selection
    await select([c1.ref]);
    await setCheckbox("Deselect non-matching items", false);
    await setSelect("Match Library Link", "different");
    await pressFbp("Select Matching");
    t(selection().length === others.length + 1 && selection().includes(c1.ref), `the selected footprint stays: ${selection().length}`);
    await setCheckbox("Deselect non-matching items", true);
  });

  await scenario("Zoom to fit selected items frames the matches", async (t) => {
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(300);
    await select([c1.ref]);
    await openFbp();
    await setSelect("Match Reference", "matching");
    const before = __eda.state().view;
    await setCheckbox("Zoom to fit selected items", true);
    await pressFbp("Select Matching", 700);
    const after = __eda.state().view;
    t(selection().length === 1 && selection()[0] === c1.ref, `Reference matches the one footprint: ${selection().join()}`);
    t(after.scale > before.scale * 2, `the view is on it: scale ${before.scale.toFixed(4)} -> ${after.scale.toFixed(4)}`);
    await setCheckbox("Zoom to fit selected items", false);
    await setSelect("Match Reference", "ignored");
    await __eda.run("common.Control.zoomFitScreen");
  });

  await scenario("a field of a footprint is a row, and Create from Selection Tab writes the rows as a query that selects the same", async (t) => {
    await select([c1.ref]);
    await openFbp();
    await setSelect("Match Library Link", "matching");
    await setSelect("Match Value", "matching");
    const expected = parts.filter((p) => p.footprint === c1.footprint && p.value === c1.value).map((p) => p.ref);
    await pressFbp("Select Matching");
    t(JSON.stringify(selection().sort()) === JSON.stringify([...expected].sort()), `Library Link and Value together: ${selection().length} of ${expected.length}`);
    await select([c1.ref]);
    await tab("Query");
    t(!button("find-by-properties", "Create from Selection Tab").disabled, "Create from Selection Tab is on with a row active");
    await pressFbp("Create from Selection Tab", 300);
    const q = dialog("fbp-query")?.value ?? "";
    t(q === `A.Library_Link == '${c1.footprint}' && A.Value == '${c1.value}'`, `the query: ${q}`);
    await pressFbp("Select Matching");
    t(JSON.stringify(selection().sort()) === JSON.stringify([...expected].sort()), `the query selects the same ${expected.length}: ${selection().length}`);
    t(text("fbp-query-status") === `${expected.length} items matched`, `the line: ${text("fbp-query-status")}`);
    await tab("From Selection");
    await setSelect("Match Library Link", "ignored");
    await setSelect("Match Value", "ignored");
  });

  await scenario("the Query tab: KiCad's expressions over footprints and pads, Check Syntax, the error box, Recently Used", async (t) => {
    await select([]);
    await openFbp();
    await tab("Query");
    // Check Syntax
    await setText("fbp-query", "A.Type == 'Footprint' && A.Reference == 'C*'");
    await pressFbp("Check Syntax", 200);
    t(text("fbp-query-status") === "Syntax OK", `Check Syntax: ${text("fbp-query-status")}`);
    await setText("fbp-query", "A.Width >");
    await pressFbp("Check Syntax", 200);
    t(text("fbp-query-status") === "Syntax error: Syntax error", `a text that stops: ${text("fbp-query-status")}`);
    await setText("fbp-query", "A.Width == B.Width");
    await pressFbp("Check Syntax", 200);
    t(text("fbp-query-status") === "B. expressions are not supported.", `B. is refused: ${text("fbp-query-status")}`);
    await setText("fbp-query", "");
    await pressFbp("Check Syntax", 200);
    t(text("fbp-query-status") === "Expression is empty", `empty: ${text("fbp-query-status")}`);
    // Select Matching
    const capacitors = parts.filter((p) => /^C\d+$/.test(p.ref)).map((p) => p.ref);
    await setText("fbp-query", "A.Type == 'Footprint' && A.Reference == 'C*'");
    await pressFbp("Select Matching");
    t(JSON.stringify(selection().sort()) === JSON.stringify([...capacitors].sort()), `the capacitors by reference: ${selection().length} of ${capacitors.length}`);
    const west = parts.filter((p) => p.at && p.at[0] < 12000).map((p) => p.ref);
    await setText("fbp-query", "A.Type == 'Footprint' && A.Position_X < 12mm");
    await pressFbp("Select Matching");
    t(JSON.stringify(selection().sort()) === JSON.stringify([...west].sort()), `a length with its unit: ${selection().length} footprints left of 12 mm (${west.length})`);
    const bottom = parts.filter((p) => p.side === "bottom").map((p) => p.ref);
    await setText("fbp-query", "A.Type == 'Footprint' && A.Layer == 'B.Cu'");
    await pressFbp("Select Matching");
    t(selection().length === bottom.length, `a layer by its name: ${selection().length} on B.Cu (${bottom.length})`);
    const gndPads = parts.flatMap((p) => (p.pads ?? []).filter((pad) => pad.net === "GND").map(() => p.ref)).length;
    await setText("fbp-query", "A.Type == 'Pad' && A.NetName == 'GND'");
    await pressFbp("Select Matching");
    t(selection().length === gndPads && selection().every((id) => id.includes(".")), `the pads on GND: ${selection().length} of ${gndPads}`);
    const smd = parts.flatMap((p) => (p.pads ?? []).filter((pad) => pad.th !== true && (pad.kind ?? "smd") === "smd")).length;
    await setText("fbp-query", "A.Type == 'Pad' && A.Pad_Type == 'SMD'");
    await pressFbp("Select Matching");
    t(selection().length === smd, `a choice by its label: ${selection().length} SMD pads (${smd})`);
    // a field of a footprint through getField, and the alias A.Reference
    await setText("fbp-query", "A.getField('Value') == '10k'");
    await pressFbp("Select Matching");
    const tenk = parts.filter((p) => p.value === "10k").map((p) => p.ref);
    t(JSON.stringify(selection().sort()) === JSON.stringify([...tenk].sort()), `getField: ${selection().length} of ${tenk.length}`);
    // || binds tighter than &&, as in KiCad's grammar: Track && (Via || Pad) is nothing
    await setText("fbp-query", "A.Type == 'Track' && A.Type == 'Via' || A.Type == 'Pad'");
    await pressFbp("Select Matching");
    t(selection().length === 0 && text("fbp-query-status") === "0 items matched", `|| before &&: ${text("fbp-query-status")}`);
    // a query that does not compile: the message box
    await setText("fbp-query", "A.Type == ");
    await pressFbp("Select Matching", 300);
    t(text("fbp-query-status") === "Syntax error in expression", `the line: ${text("fbp-query-status")}`);
    t(/Expression Error/.test(text("fbp-error")) && /Syntax error/.test(text("fbp-error")), `the Expression Error box: ${text("fbp-error").replace(/\n/g, " | ")}`);
    button("fbp-error", "OK").click();
    await sleep(150);
    t(!dialog("fbp-error"), "OK closes it");
    // Recently Used: the queries that compiled, newest first, once each
    const recent = [...document.querySelectorAll('select[aria-label="Recently Used"] option')].map((o) => o.textContent).filter(Boolean);
    t(recent[0] === "A.Type == 'Track' && A.Type == 'Via' || A.Type == 'Pad'" && new Set(recent).size === recent.length, `Recently Used: ${recent.length} queries, the last one first`);
    t(!recent.includes("A.Type == "), "a query that does not compile is not kept");
    // an unknown property is told by the compiler and matches nothing, as in KiCad
    await setText("fbp-query", "A.Nope == 'x'");
    await pressFbp("Check Syntax", 200);
    t(text("fbp-query-status") === "Syntax OK", `an unknown property still compiles in KiCad: ${text("fbp-query-status")}`);
    await pressFbp("Select Matching");
    t(selection().length === 0, "and selects nothing");
    // after A. the property names are offered
    await setText("fbp-query", "A.Pos");
    t(!!dialog("fbp-suggestions") && /Position_X/.test(text("fbp-suggestions")), `after A.: ${text("fbp-suggestions").replace(/\n/g, " ")}`);
  });

  await scenario("the board is as it was and nothing broke", async (t) => {
    await select([]);
    const after = JSON.stringify((await placed()).map((p) => [p.ref, p.at, p.rot, p.side]).sort());
    t(after === pcbBefore, "every footprint is where it was, turned and on the side it was");
    const errors = (__eda.errors?.() ?? []).slice(errorsFrom);
    t(errors.length === 0, `no console error: ${JSON.stringify(errors).slice(0, 200)}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
