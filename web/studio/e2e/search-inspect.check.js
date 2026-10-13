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
    for (const id of ["find-by-properties", "pcb-find-dialog", "inspect-clearance", "inspect-constraints"]) {
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

  await scenario("Find follows the IR: a user field, a hidden field only with Include hidden fields, a zone by its name", async (t) => {
    const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
    const layout = (visible) => ({ at: { x: 0, y: 1500 }, size: [1000, 1000], thickness: 150, layer: "F.Fab", visible });
    let steps = 0;
    const findBox = (label) => [...document.querySelectorAll('[data-testid="pcb-find-dialog"] label.toggle')].find((l) => l.textContent.includes(label))?.querySelector("input");
    const setBox = async (label, on) => { const el = findBox(label); if (el && el.checked !== on) { el.click(); await sleep(120); } };
    const findFor = async (needle) => {
      const input = dialog("pcb-find-text");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, needle);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(120);
      button("pcb-find-dialog", "Restart Search").click();
      await sleep(450);
      return { status: text("pcb-find-status"), selection: selection() };
    };
    try {
      let res = await cmd({ op: "edit_board_footprint", part: c1.ref, fields: [{ name: "Supplier", text: "ACME-4711", layout: layout(true) }, { name: "Batch", text: "LOT-77", layout: layout(false) }] });
      t(res.ok, `${c1.ref} has two user fields, one hidden: ${res.message ?? ""}`);
      if (res.ok) steps++;
      const outline = [{ x: 1000, y: 1000 }, { x: 5000, y: 1000 }, { x: 5000, y: 5000 }];
      res = await cmd({ op: "add_zone", net: "GND", layer: "F.Cu", outline });
      t(res.ok, `a zone: ${res.message ?? ""}`);
      if (res.ok) steps++;
      const zones = (await state()).routing?.zones ?? [];
      const zone = zones[zones.length - 1];
      res = await cmd({ op: "set_zone_name", id: zone.id, name: "keepout-e2e" });
      t(res.ok, `named keepout-e2e: ${res.message ?? ""}`);
      if (res.ok) steps++;
      await select([]);
      await __eda.run("common.Interactive.find");
      await sleep(350);
      t(!!dialog("pcb-find-dialog"), "Find (Ctrl+F) is open");
      await setBox("Match case", false);
      await setBox("Search other text items", true);
      await setBox("Include hidden fields", false);

      let r = await findFor("ACME-4711");
      t(JSON.stringify(r.selection) === JSON.stringify([c1.ref]) && r.status === "Hit(s): 1 / 1", `a user field finds its footprint: ${r.selection.join()} ${r.status}`);
      await setBox("Search other text items", false);
      r = await findFor("ACME-4711");
      t(r.selection.length === 0 && /not found/.test(r.status), `with other text items off nothing: ${r.status}`);
      await setBox("Search other text items", true);

      r = await findFor("LOT-77");
      t(r.selection.length === 0 && /not found/.test(r.status), `a hidden field is not searched: ${r.status}`);
      await setBox("Include hidden fields", true);
      r = await findFor("LOT-77");
      t(JSON.stringify(r.selection) === JSON.stringify([c1.ref]), `Include hidden fields finds it: ${r.selection.join()} ${r.status}`);
      await setBox("Include hidden fields", false);

      r = await findFor("keepout-e2e");
      t(JSON.stringify(r.selection) === JSON.stringify([zone.id]) && r.status === "Hit(s): 1 / 1", `a zone is found by its name: ${r.selection.join()} ${r.status}`);
      await setBox("Search other text items", false);
      r = await findFor("keepout-e2e");
      t(r.selection.length === 0, `zones are other text items: ${r.status}`);
      await setBox("Search other text items", true);
      // a field selected on the board is what the dialog opens with
      await closeDialogs();
      await select([`${c1.ref}:Supplier`]);
      await __eda.run("common.Interactive.find");
      await sleep(350);
      t(dialog("pcb-find-text")?.value === "ACME-4711", `Find opens with the text of the selected field: ${dialog("pcb-find-text")?.value}`);
    } finally {
      await closeDialogs();
      for (let i = 0; i < steps; i++) { await __eda.run("common.Interactive.undo"); await sleep(700); }
    }
    const left = (await state()).routing?.zones ?? [];
    t(left.every((z) => z.name !== "keepout-e2e") && ((await state()).parts.find((p) => p.ref === c1.ref).fields ?? []).every((f) => f.name !== "Supplier"), "the undos take the zone and the fields away");
  });

  // ---------------------------------------------------------------------------------------------------------- Inspect > Clearance / Constraints Resolution
  const cmdPost = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const padId = (p, i) => {
    const num = p.pads[i].num;
    const k = p.pads.slice(0, i + 1).filter((q) => q.num === num).length;
    return k === 1 ? `${p.ref}.${num}` : `${p.ref}.${num}#${k}`;
  };
  // two SMD pads on the front, on different nets, that nobody shares a number with
  const smdPads = parts.filter((p) => p.side === "top").flatMap((p) => p.pads.map((pad, i) => ({ id: padId(p, i), net: pad.net, ref: p.ref, num: pad.num, th: pad.th === true }))).filter((x) => !x.th && x.net);
  const padA = smdPads[0];
  const padB = smdPads.find((x) => x.net !== padA.net && x.ref === padA.ref) ?? smdPads.find((x) => x.net !== padA.net);
  const inspectOpen = async (kind, ms = 3000) => {
    const t0 = Date.now();
    while (!dialog(`inspect-${kind}`) && Date.now() - t0 < ms) await sleep(100);
    await sleep(150);
    return dialog(`inspect-${kind}`);
  };
  const tabTitles = (kind) => [...document.querySelectorAll(`[data-testid="inspect-${kind}"] [role="tab"]`)].map((b) => b.textContent.trim());
  const clickTab = async (kind, title) => { dialog(`inspect-tab-${title}`)?.click(); await sleep(150); };
  const lines = (kind) => [...document.querySelectorAll(`[data-testid="inspect-${kind}"] [data-testid="inspect-line"]`)].map((l) => l.innerText);
  const postInspect = (route, body) => fetch(`/api/inspect/${route}`, { method: "POST", body: JSON.stringify(body) }).then((r) => r.json());

  await scenario("Clearance Resolution says which clearance applies between two pads, and every rule it looked at, in KiCad's words", async (t) => {
    t(!!padA && !!padB, `two pads on different nets: ${padA?.id} [${padA?.net}] and ${padB?.id} [${padB?.net}]`);
    const acts = __eda.actions({ all: true });
    t(acts.some((a) => a.id === "pcbnew.InspectionTool.InspectClearance" && a.enabled) && acts.some((a) => a.id === "pcbnew.InspectionTool.InspectConstraints" && a.enabled), "both Inspect commands are in the registry");
    await select([padA.id, padB.id]);
    const ran = await __eda.run("pcbnew.InspectionTool.InspectClearance");
    t(ran.ok, `the command ran: ${ran.error ?? ""}`);
    t(!!(await inspectOpen("clearance")), "the Clearance Report dialog opens");
    const titles = tabTitles("clearance");
    t(titles[0] === "F.Cu" && titles[titles.length - 1] === "Physical Clearances", `the pages: ${titles.join(" | ")}`);
    const reply = await postInspect("clearance", { a: padA.id, b: padB.id, active_layer: "F.Cu", units: "mm" });
    const res = reply.pages[0].resolutions[0];
    t(reply.ok && res.constraint === "clearance" && res.min > 0, `the server resolves ${res.min} nm by ${res.rule}`);
    const text0 = lines("clearance").join("\n");
    t(/^Clearance resolution for:/m.test(text0) && /^Layer F\.Cu$/m.test(text0), "the header names the layer");
    t(text0.includes(`[${padA.net}]`) && text0.includes(`[${padB.net}]`) && /\[netclass [^\]]+\]/.test(text0), "and both items, with their net and net class");
    t(/Checking netclass '[^']+' clearance: \d+\.\d{4} mm\./.test(text0), `an implicit rule is checked: ${(text0.match(/Checking netclass[^\n]*/) ?? [""])[0]}`);
    t(/Board minimum clearance: \d+\.\d{4} mm\./.test(text0), "the board minimum clearance is told");
    const mm = (res.min / 1e6).toString();
    t(new RegExp(`Resolved min clearance: ${mm.replace(".", "\\.")} mm\\.`).test(text0), `the verdict: Resolved min clearance: ${mm} mm.`);
    await clickTab("clearance", "Physical Clearances");
    // (KiCad's implicit "barcode visual separation default" rule is a physical_clearance rule, so the page goes through the layers the items share and tells none is in effect)
    const phys = lines("clearance").join("\n");
    t(/Checking barcode visual separation default clearance: 1\.0000 mm\./.test(phys) && /Membership not satisfied; constraint ignored\./.test(phys) && /No 'physical_clearance' constraints in effect on F\.Cu\./.test(phys), "the Physical Clearances page goes layer by layer and says none is in effect");
    await closeDialogs();
    t(!dialog("inspect-clearance"), "Escape closes it");

    // a rules file rule: it wins, and the report says it overrode the implicit ones
    let steps = 0;
    try {
      const rule = `(version 1)\n(rule "e2e wide"\n\t(condition "A.Type == 'Pad' && B.Type == 'Pad'")\n\t(constraint clearance (min 0.77mm)))\n`;
      let r = await cmdPost({ op: "set_custom_rules", text: rule });
      t(r.ok, `custom rules set: ${r.message ?? ""}`);
      if (r.ok) steps++;
      await select([padA.id, padB.id]);
      await __eda.run("pcbnew.InspectionTool.InspectClearance");
      await inspectOpen("clearance");
      const t1 = lines("clearance").join("\n");
      t(/Checking rule 'e2e wide' clearance: 0\.7700 mm\./.test(t1) && /Checking rule condition 'A\.Type == 'Pad' && B\.Type == 'Pad''\./.test(t1), "the rule and its condition are checked");
      t(/Rule applied; overrides previous constraints\./.test(t1) && /Resolved min clearance: 0\.77 mm\./.test(t1), "it applies, and its clearance is the verdict");
      t(!/Board minimum clearance/.test(t1), "an explicit rule ends the evaluation: no board minimum line");
      const j = await postInspect("clearance", { a: padA.id, b: padB.id });
      t(j.pages[0].resolutions[0].rule === "rule 'e2e wide'" && j.pages[0].resolutions[0].min === 770000, `as numbers: ${j.pages[0].resolutions[0].rule} ${j.pages[0].resolutions[0].min}`);
      await closeDialogs();

      // a rules file that does not compile: the implicit rules alone, and the notice with its link
      r = await cmdPost({ op: "set_custom_rules", text: `(version 1)\n(rule x (constraint clearance (min 1)))\n` });
      if (r.ok) steps++;
      await select([padA.id, padB.id]);
      await __eda.run("pcbnew.InspectionTool.InspectClearance");
      await inspectOpen("clearance");
      const t2 = lines("clearance").join("\n");
      t(/Report incomplete: could not compile custom design rules\./.test(t2) && /Show design rules\./.test(t2), "the report says the rules could not be compiled");
      t(!/e2e wide/.test(t2) && /Resolved min clearance: /.test(t2), "and goes on with the implicit rules");
      t(!!document.querySelector('[data-testid="inspect-clearance"] a'), "with a link");
      await closeDialogs();
    } finally {
      for (let i = 0; i < steps; i++) { await __eda.run("common.Interactive.undo"); await sleep(700); }
    }
    const back = await postInspect("clearance", { a: padA.id, b: padB.id });
    t(back.compile_error === null && back.pages[0].resolutions[0].rule !== "rule 'e2e wide'", "the undos take the rules away");
  });

  await scenario("Clearance and Constraints Resolution pick what is not selected, and refuse what makes no sense", async (t) => {
    // two items selected: no picker; none: both are picked; one: the second is
    await select([]);
    let r = await __eda.run("pcbnew.InspectionTool.InspectClearance");
    t(__eda.state().picker === "Select first item for clearance resolution...", `nothing selected: ${__eda.state().picker}`);
    await __eda.run("common.Interactive.cancel");
    t(__eda.state().picker === null && !dialog("inspect-clearance"), "Escape cancels it and no report opens");
    await select([padA.id]);
    r = await __eda.run("pcbnew.InspectionTool.InspectClearance");
    t(__eda.state().picker === "Select second item for clearance resolution...", `one item selected: ${__eda.state().picker}`);
    await __eda.run("common.Interactive.cancel");
    t(JSON.stringify(selection()) === JSON.stringify([padA.id]), `the selection is as it was: ${selection().join()}`);
    // constraints: one item reports, none is picked, several are refused
    await select([]);
    await __eda.run("pcbnew.InspectionTool.InspectConstraints");
    t(__eda.state().picker === "Select item for constraints resolution...", `constraints, nothing selected: ${__eda.state().picker}`);
    await __eda.run("common.Interactive.cancel");
    await select([padA.id, padB.id, parts[0].ref]);
    r = await __eda.run("pcbnew.InspectionTool.InspectConstraints");
    t(/Select a single item for a constraints resolution report\./.test(r.toast ?? ""), `three selected: ${r.toast}`);
    t(!dialog("inspect-constraints") && __eda.state().picker === null, "no report, no picker");
    // the server refuses the same item twice and an unknown one
    const same = await postInspect("clearance", { a: padA.id, b: padA.id });
    t(same.ok === false && same.message === "Select two different items for clearance resolution.", `twice the same: ${same.message}`);
    const nope = await postInspect("constraints", { item: "no-such-item" });
    t(nope.ok === false && /no such item/.test(nope.message), `unknown: ${nope.message}`);
  });

  await scenario("Constraints Resolution of a pad: the pages of a pad, the keepout verdict and the assertions of the rules file", async (t) => {
    let steps = 0;
    try {
      await select([padA.id]);
      await __eda.run("pcbnew.InspectionTool.InspectConstraints");
      t(!!(await inspectOpen("constraints")), "the Constraints Report dialog opens on one selected item");
      const titles = tabTitles("constraints");
      t(JSON.stringify(titles) === JSON.stringify(["Solder Mask", "Solder Paste", "Keepouts", "Assertions"]), `the pages of an SMD pad: ${titles.join(" | ")}`);
      await clickTab("constraints", "Keepouts");
      t(/Item allowed at current location\./.test(lines("constraints").join("\n")), "Keepouts: allowed where it is");
      await clickTab("constraints", "Solder Mask");
      t(/Resolved solder mask expansion: /.test(lines("constraints").join("\n")), "Solder Mask: resolved");
      await closeDialogs();
      const rule = `(version 1)\n(rule "wide pads"\n\t(condition "A.Type == 'Pad'")\n\t(constraint assertion "A.Size_X >= 20mm"))\n`;
      const set = await cmdPost({ op: "set_custom_rules", text: rule });
      t(set.ok, `an assertion rule: ${set.message ?? ""}`);
      if (set.ok) steps++;
      await select([padA.id]);
      await __eda.run("pcbnew.InspectionTool.InspectConstraints");
      await inspectOpen("constraints");
      await clickTab("constraints", "Assertions");
      const a = lines("constraints").join("\n");
      t(/Checking rule assertion 'A\.Size_X >= 20mm'\./.test(a) && /--> Assertion failed\. <--/.test(a), `the assertion is tested and fails: ${(a.match(/Assertion[^\n]*/g) ?? []).join(" / ")}`);
    } finally {
      await closeDialogs();
      for (let i = 0; i < steps; i++) { await __eda.run("common.Interactive.undo"); await sleep(700); }
    }
  });

  await scenario("the board is as it was and nothing broke", async (t) => {
    await select([]);
    const after = JSON.stringify((await placed()).map((p) => [p.ref, p.at, p.rot, p.side]).sort());
    t(after === pcbBefore, "every footprint is where it was, turned and on the side it was");
    // (the error toast of Constraints Resolution with several items selected is the one a scenario above asked for)
    const errors = (__eda.errors?.() ?? []).slice(errorsFrom).filter((e) => e.message !== "Select a single item for a constraints resolution report.");
    t(errors.length === 0, `no console error: ${JSON.stringify(errors).slice(0, 200)}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
