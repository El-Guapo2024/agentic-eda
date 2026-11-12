// Browser check for the simulator round v10 (UI-LEFT-2 MM1-MM4, task-v10-sim; web/studio/PARITY-sch.md section 89), on a served board made from
// web/studio/e2e/sim-v10-board.yaml with web/studio/e2e/sim-model-diode.lib copied to models/diode.lib:
//   User-defined Signals before any run offer libngspice's const plot (`SimPlotVectors` of the current plot);
//   a real operating-point run (ngspice), then Probe: a sheet pin under the pointer brightened (the motion handler collects SCH_SHEET_PIN_T),
//     a click on it keeping the probe (`GetNode` finds it);
//   the Simulation Model dialog: a `${VAR}` in Sim.Params read resolved (`GetFieldValue( .., aResolve = true )`), arithmetic in a Float row
//     ("1k*2" + Enter is "2k", `SIM_STRING_PROPERTY`), the Code tab taking typing;
//   the K device's l1 / l2 a choice of the circuit's inductors, none selected;
//   a library .model's parameters in the grid (the library's values behind the instance's), an override written to Sim.Params alone, OK
//     handing the fields to Symbol Properties and its OK applying them as one undo step, kicad-cli's netlist naming the instance's model;
//   "No simulation model definition found for symbol 'R3'." in KiCad's error box;
//   nothing logged an error.
//
// It answers through `window.__simV10Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=sim-v10 minutes=10
(async () => {
  const results = [];
  const check = (window.__simV10Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const errorsFrom = Date.now();
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const click = (el) => el?.click();
  const fields = async (ref) => post("/api/sim/model/symbol", { ref });
  const sch = () => fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());

  // ---- the simulator window
  const win = () => document.querySelector('[role=dialog][aria-label="SPICE Simulator"]');
  const q = (sel) => win()?.querySelector(sel) ?? null;
  const simDialog = (title) => document.querySelector(`.sim-dialog [role=dialog][aria-label="${title}"]`);
  const focusSim = () => { const w = q(".sim-titlebar"); w?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); w?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })); };
  const run = async (id, arg) => { focusSim(); await sleep(60); return E.run(id, arg); };
  const runButton = () => q('button[aria-label="Run Simulation"]');
  const settle = async (ms = 60000) => { await sleep(300); const t0 = Date.now(); while (runButton()?.disabled && Date.now() - t0 < ms) await sleep(200); await sleep(300); };
  const newTab = async (type, fill) => {
    await run("eeschema.Simulation.newAnalysisTab");
    await sleep(800);
    const d = simDialog("New Simulation Tab");
    if (!d) return false;
    setValue(d.querySelector('select[aria-label="Analysis type"]'), type);
    await sleep(150);
    await fill(d);
    click([...d.querySelectorAll("button")].find((b) => b.textContent.trim() === "OK"));
    await sleep(600);
    return !simDialog("New Simulation Tab");
  };
  // the sheet, and a point of it as the canvas sees it
  const box = () => document.querySelector(".pcb-canvas-container");
  const toScreen = ([x, y]) => { const v = E.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const fire = (type, [x, y]) => { const b = box(); const rect = b.getBoundingClientRect(); (b.querySelector("canvas") || b).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" ? 1 : 0 })); };

  // ---- the Simulation Model dialog, from Symbol Properties
  const dlg = () => document.querySelector('[data-testid="sim-model-dialog"]');
  const $ = (id) => dlg()?.querySelector(`[data-testid="${id}"]`) ?? null;
  const row = (name) => dlg()?.querySelector(`[data-testid="sim-model-param-grid"] tr[data-param="${name}"]`) ?? null;
  const cell = (name) => row(name)?.querySelector("input, select") ?? null;
  const propsDialog = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector('[data-testid="symbol-sim-model"]')) ?? null;
  const openDialog = async (ref) => {
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [ref]);
    await sleep(300);
    await E.run("eeschema.InteractiveEdit.properties");
    await sleep(500);
    click(document.querySelector('[data-testid="symbol-sim-model"]'));
    const t0 = Date.now();
    while (!$("sim-model-device") && !$("sim-model-list")?.options.length && !document.querySelector('[data-testid="message-box"]') && Date.now() - t0 < 10000) await sleep(200);
    await sleep(400);
    return !!dlg();
  };
  const cancelBoth = async () => {
    for (let i = 0; i < 2; i++) {
      const top = dlg() ?? propsDialog();
      click([...(top?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.textContent.trim() === "Cancel"));
      await sleep(300);
    }
  };
  const simOk = async () => { click($("sim-model-ok")); const t0 = Date.now(); while (dlg() && Date.now() - t0 < 8000) await sleep(200); await sleep(300); return !dlg(); };
  const propsOk = async () => {
    click([...(propsDialog()?.querySelectorAll(".dialog-footer button.primary") ?? [])].find((x) => x.textContent.trim() === "OK"));
    const t0 = Date.now();
    while (propsDialog() && Date.now() - t0 < 8000) await sleep(200);
    await sleep(700);
    return !propsDialog();
  };
  const messageBox = async () => {
    const t0 = Date.now();
    while (!document.querySelector('[data-testid="message-box"]') && Date.now() - t0 < 4000) await sleep(100);
    const text = document.querySelector('[data-testid="message-box-text"]')?.textContent ?? null;
    click(document.querySelector('[data-testid="message-box-ok"]'));
    await sleep(200);
    return text;
  };

  // ---- the board, and a sheet with a pin to probe
  const s0 = await sch().catch(() => null);
  if (!["V1", "R1", "R2", "R3", "L1", "L2", "D1"].every((r) => s0?.symbols?.some((s) => s.id === r))) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs web/studio/e2e/sim-v10-board.yaml's circuit"] });
    check.done = true;
    return;
  }
  const xs = s0.symbols.map((s) => s.at[0]);
  const ys = s0.symbols.map((s) => s.at[1]);
  const G = 1_270_000;
  const sheetAt = { x: Math.round((Math.max(...xs) + 30 * G) / G) * G, y: Math.round(Math.min(...ys) / G) * G };
  const added = await cmd({ op: "add_sheet", name: "Probe", file: "probe.kicad_sch", at: sheetAt, size: [16 * G, 8 * G] });
  const s1 = await sch();
  const sheet = s1.sheets.find((x) => x.name === "Probe");
  if (sheet) await cmd({ op: "sch_edit", verb: "add_sheet_pin", sheet: sheet.id, name: "PIN_A", shape: "input", at: { x: sheet.at[0], y: sheet.at[1] + 2 * G } });
  [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
  await sleep(2000);

  await scenario("before a run, User-defined Signals offer libngspice's const plot", async (t) => {
    t(added.ok !== false && !!sheet, `a sheet to probe: ${JSON.stringify(added).slice(0, 160)}`);
    await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2000);
    t(!!win(), "the window is open");
    t(await newTab("OP", async () => {}), "an OP tab");
    await run("eeschema.Simulation.editUserDefinedSignals");
    await sleep(400);
    const u = simDialog("User-defined Signals");
    t(!!u, "the dialog");
    click(u?.querySelector('button[aria-label="Add signal"]'));
    await sleep(200);
    const c = u?.querySelector('textarea[aria-label="Signal 1"]');
    if (!t(!!c, "the row's completing editor")) return;
    c.focus();
    for (const ch of "pl") { document.execCommand("insertText", false, ch); await sleep(60); }
    await sleep(200);
    const list = [...document.querySelectorAll('[data-testid="sim-user-signal-0-completions"] .rte-item')].map((e) => e.textContent);
    t(list.includes("planck"), `"pl" offers the const plot's planck: ${JSON.stringify(list)}`);
    click([...u.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel"));
    await sleep(300);
  });

  await scenario("a real operating point (ngspice), then Probe brightens a sheet pin and a click on it keeps the probe", async (t) => {
    await run("eeschema.Simulation.runSimulation");
    await settle();
    const console_ = q('[aria-label="Console"]')?.textContent ?? q("textarea")?.value ?? "";
    t(!/error/i.test(console_.split("\n").filter((l) => !/no compatibility|warning/i.test(l)).join("\n")), `the run: ${console_.slice(0, 300)}`);
    await E.run("common.Control.zoomFitScreen");
    await sleep(800);
    t((await run("eeschema.Simulation.probe")).ok === true, "Probe, after the run");
    await sleep(300);
    const s = await sch();
    const pin = s.sheets.find((x) => x.name === "Probe")?.pins?.[0];
    if (!t(!!pin, "the sheet's pin")) return;
    fire("pointermove", toScreen(pin.at));
    await sleep(400);
    t(E.state().pickerHover === pin.id, `the sheet pin under the pointer is brightened: ${E.state().pickerHover} / ${pin.id}`);
    const c = box();
    t(/\) 1 31, crosshair/.test(c.style.cursor), "the voltage probe over it");
    fire("pointerdown", toScreen(pin.at));
    fire("pointerup", toScreen(pin.at));
    await sleep(500);
    t(E.state().picker != null && /svg/.test(c.style.cursor), `a click on a sheet pin keeps the probe (a node, no trace): picker ${JSON.stringify(E.state().picker)}`);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    t(E.state().pickerHover === null, "Escape: nothing brightened");
  });

  await scenario("the dialog reads ${VAR} resolved, evaluates arithmetic on Enter, and its Code tab takes typing", async (t) => {
    const set = await cmd({ op: "set_symbol_fields", edits: [{ id: "R1", field: "Sim.Device", value: "R" }, { id: "R1", field: "Sim.Params", value: "r=${MYR}" }, { id: "R1", field: "MYR", value: "2.2k" }] });
    t(set.ok !== false, "R1's Sim.Params names its MYR field");
    await sleep(800);
    t(await openDialog("R1"), "the dialog opens");
    t(cell("r")?.value === "2.2k", `r is the field resolved: ${cell("r")?.value}`);
    const el = cell("r");
    el.focus();
    setValue(el, "1k*2");
    el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await sleep(300);
    t(cell("r")?.value === "2k", `"1k*2" Enter: ${cell("r")?.value}`);
    click($("sim-model-code-tab"));
    await sleep(200);
    const code = $("sim-model-code");
    t(code?.value === "R NC--0 NC--1 2k", `the Code tab: ${code?.value}`);
    t(code && !code.readOnly, "the Code tab takes typing");
    await cancelBoth();
  });

  await scenario("the K device's l1 and l2 are a choice of the circuit's inductors, none selected", async (t) => {
    t(await openDialog("R2"), "the dialog opens");
    setValue($("sim-model-device"), "Mutual Inductance Statement");
    const t0 = Date.now();
    while (!(cell("l1") instanceof HTMLSelectElement && cell("l1").options.length > 1) && Date.now() - t0 < 60000) await sleep(300);
    const l1 = cell("l1");
    const opts = l1 ? [...l1.options].map((o) => o.value) : [];
    t(l1 instanceof HTMLSelectElement && opts.join(",") === ",L1,L2", `l1's choices: ${JSON.stringify(opts)}`);
    t(l1?.value === "", "none selected");
    await cancelBoth();
  });

  await scenario("a library .model's parameters in the grid; an override alone in Sim.Params; Symbol Properties' OK applies it, one undo step", async (t) => {
    const set = await cmd({ op: "set_symbol_fields", edits: [{ id: "D1", field: "Sim.Library", value: "models/diode.lib" }, { id: "D1", field: "Sim.Name", value: "D1N4148" }, { id: "D1", field: "Sim.Params", value: "n=1.9" }] });
    t(set.ok !== false, "D1 on the library's D1N4148, n overridden");
    await sleep(800);
    t(await openDialog("D1"), "the dialog opens");
    t($("sim-model-library")?.checked === true && $("sim-model-list")?.value === "D1N4148", "SPICE model from file, D1N4148");
    click(dlg()?.querySelector('tr[data-category="DC"] td'));
    await sleep(200);
    t(cell("is")?.value === "2.52n" && cell("n")?.value === "1.9" && cell("rs")?.value === "0.568", `the library's is and rs, the instance's n: ${cell("is")?.value} ${cell("n")?.value} ${cell("rs")?.value}`);
    const el = cell("rs");
    el.focus();
    setValue(el, "1");
    el.blur();
    await sleep(300);
    t(await simOk(), "OK");
    t((await fields("D1")).fields["Sim.Params"] === "n=1.9", "nothing written before Symbol Properties' OK");
    t(await propsOk(), "Symbol Properties' OK");
    const f = (await fields("D1")).fields;
    const params = (f["Sim.Params"] ?? "").split(" ").sort().join(" ");
    t(params === "n=1.9 rs=1" && f["Sim.Library"] === "models/diode.lib" && f["Sim.Name"] === "D1N4148", `written: ${JSON.stringify(f)}`);
    const nl = (await post("/api/sim/circuit", { command: ".op" })).netlist ?? "";
    t(/\n\.model D1\.D1N4148 D/.test(nl) && /\nD1 \S+ \S+ D1\.D1N4148/.test(nl), `kicad-cli's netlist names the instance's model: ${nl.split("\n").filter((l) => /D1/.test(l)).join(" / ")}`);
    await E.run("common.Interactive.undo");
    await sleep(900);
    t((await fields("D1")).fields["Sim.Params"] === "n=1.9", "one undo takes it back");
  });

  await scenario("a Sim.Type no model has: KiCad's error box", async (t) => {
    await cmd({ op: "set_symbol_fields", edits: [{ id: "R3", field: "Sim.Device", value: "R" }, { id: "R3", field: "Sim.Type", value: "BOGUS" }] });
    await sleep(800);
    await openDialog("R3");
    const text = await messageBox();
    t(text === "No simulation model definition found for symbol 'R3'.", `the box: ${text}`);
    t(!$("sim-model-info")?.textContent.includes("No simulation model"), "not the info bar");
    await cancelBoth();
  });

  await scenario("nothing logged an error", async (t) => {
    const errs = E.errors(errorsFrom);
    t(errs.length === 0, `errors: ${JSON.stringify(errs).slice(0, 600)}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
