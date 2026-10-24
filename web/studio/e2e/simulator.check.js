// SPICE simulator check (package P21, PARITY-sch.md "The SPICE simulator"): the schematic editor's Inspect > Simulator window and every action of it,
// driven through `window.__eda` and the window's own controls. Run it in the page of a served board made from web/studio/e2e/simulator-board.yaml
// (`eda board new web/studio/e2e/simulator-board.yaml -o <dir>`, `eda board serve -C <dir> --ui web/studio/dist`), with ngspice installed:
// `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:<port>/ web/studio/e2e/simulator.check.js`.
//
// What it checks, against the circuit's hand-worked values (a non-inverting amplifier of KiCad's built-in op-amp, gain 11, on 0.1 V; an RC of 1k and
// 1u charging from 12 V):
//   the window opens on the schematic's own analysis (a `.tran` text on the sheet), and without one on nothing;
//   a new tab from the analysis dialog (TRAN with "use initial conditions"), run by ngspice: the RC's voltage plotted, cursor 1 on it at 1 ms reads
//     12 (1 - e^-1) = 7.59 V, a measurement (MAX) and a user-defined signal come back from the next run;
//   the plot's zooms and their history (Undo/Redo Last Zoom), the View toggles (legend, dotted, dark mode, side panel, console) and their check marks;
//   an operating point: "Simulation results:" in the console with V(/OUT) = 989mV, and the voltages and currents on the sheet toggled;
//   a DC sweep of the supply; Show SPICE Netlist; Probe (a wire's voltage) and Tune (a tuner, a value tuned and run, Save as one undo step);
//   the exports (PNG, CSV, the clipboard, the plot pasted on the sheet), held back from the disk and the clipboard;
//   the workbook saved in the design (one undo step), the window closed and opened again on it, and a KiCad workbook opened;
//   Stop Simulation killing a long run; nothing logged an error.
//
// It answers through `window.__simulatorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=simulator minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__simulatorCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const errorsFrom = Date.now();

  // ---- the page's downloads and clipboard, held back
  const saved = [];
  const realCreate = URL.createObjectURL;
  const realClick = HTMLAnchorElement.prototype.click;
  URL.createObjectURL = (blob) => { const url = realCreate.call(URL, blob); saved.push({ url, blob, name: null }); return url; };
  HTMLAnchorElement.prototype.click = function () {
    if (this.download) { const s = saved.find((d) => d.url === this.href); if (s) s.name = this.download; return; }
    return realClick.call(this);
  };
  const clipboard = [];
  if (navigator.clipboard) navigator.clipboard.write = async (items) => { for (const it of items) clipboard.push(it); };
  // a file chooser answered with this file
  let nextFile = null;
  const realInputClick = HTMLInputElement.prototype.click;
  HTMLInputElement.prototype.click = function () {
    if (this.type === "file" && nextFile) {
      const dt = new DataTransfer();
      dt.items.add(nextFile);
      nextFile = null;
      this.files = dt.files;
      this.dispatchEvent(new Event("change"));
      return;
    }
    return realInputClick.call(this);
  };

  // ---- the window
  const win = () => document.querySelector('[role=dialog][aria-label="SPICE Simulator"]');
  const q = (sel) => win()?.querySelector(sel) ?? null;
  const dialog = (title) => document.querySelector(`.sim-dialog [role=dialog][aria-label="${title}"]`);
  const focusSim = () => { const w = q(".sim-titlebar"); w?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); w?.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })); };
  const run = async (id, arg) => { focusSim(); await sleep(60); return E.run(id, arg); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const commit = async (el, v) => { setValue(el, v); await sleep(80); el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await sleep(80); };
  const click = (el) => el?.click();
  const consoleText = () => q(".sim-console")?.value ?? "";
  const runButton = () => q('button[aria-label="Run Simulation"]');
  /** Until the run is over: the Run button enabled again (it is disabled while ngspice runs). */
  const settle = async (ms = 60000) => { await sleep(300); const t0 = Date.now(); while (runButton()?.disabled && Date.now() - t0 < ms) await sleep(200); await sleep(300); };
  const signalRow = (name) => [...(q('table[aria-label="Signals"]')?.querySelectorAll("tbody tr") ?? [])].find((r) => r.querySelector("td")?.textContent === name);
  const plotSignal = async (name) => { const box = signalRow(name)?.querySelector(`input[aria-label="Plot ${name}"]`); if (box && !box.checked) { box.click(); await sleep(300); } return !!box; };
  const cursorRows = () => [...(q('table[aria-label="Cursors"]')?.querySelectorAll("tbody tr") ?? [])].map((r) => [...r.querySelectorAll("td")].map((td) => td.querySelector("input")?.value ?? td.textContent));
  const measurementRows = () => [...(q('table[aria-label="Measurements"]')?.querySelectorAll("tbody tr") ?? [])].map((r) => [r.querySelector("input")?.value ?? "", r.querySelectorAll("td")[1]?.textContent ?? ""]);
  const action = (id) => E.actions({ all: true }).find((a) => a.id === id);
  const plotPixels = () => { const c = q("canvas[data-testid=sim-plot]"); if (!c) return null; const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data; let n = 0; for (let i = 0; i < d.length; i += 4) n += d[i] * 7 + d[i + 1] * 3 + d[i + 2]; return n; };
  const RC = "V(Net-_C1-Pad1_)";
  const newTab = async (type, fill) => {
    await run("eeschema.Simulation.newAnalysisTab");
    await sleep(800);
    const d = dialog("New Simulation Tab");
    if (!d) return false;
    setValue(d.querySelector('select[aria-label="Analysis type"]'), type);
    await sleep(150);
    await fill(d);
    click([...d.querySelectorAll("button")].find((b) => b.textContent.trim() === "OK"));
    await sleep(500);
    return !dialog("New Simulation Tab");
  };

  // ---- the board
  const schematic = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json()).catch(() => null);
  if (!schematic?.symbols?.some((s) => s.id === "U1") || !schematic.symbols.some((s) => s.id === "C1")) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs web/studio/e2e/simulator-board.yaml's circuit"] });
    check.done = true;
    return;
  }
  [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
  await sleep(1500);

  await scenario("the Simulator is offered, opens on the schematic's analysis and closes", async (t) => {
    t(action("eeschema.EditorControl.showSimulator")?.enabled === true, "Inspect > Simulator is enabled on the schematic");
    t((await E.run("eeschema.Simulation.runSimulation")).ok === false, "the simulator's own actions wait for its window");
    // a `.tran` directive on the sheet (`GetSchTextSimCommand`), taken away again after
    const added = await post("/api/cmd", { cmd: { op: "add_sch_text", content: ".tran 10u 5m 0 10u uic", at: { x: 20_000_000, y: 20_000_000 }, angle_millideg: 0, size_nm: 1_270_000 }, strict: false });
    t(added.ok !== false, "a .tran text on the sheet");
    await sleep(800);
    const r = await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2500);
    t(!!win(), `the window is open (${JSON.stringify(r)})`);
    t((q(".sim-title")?.textContent ?? "").endsWith("[Unsaved] — SPICE Simulator"), `the title: ${q(".sim-title")?.textContent}`);
    const tabs = [...(win()?.querySelectorAll(".sim-tab") ?? [])].map((x) => x.textContent);
    t(tabs.length === 1 && tabs[0].startsWith("Analysis 1 - TRAN"), `one tab from the schematic's .tran: ${tabs}`);
    await run("eeschema.Simulation.runSimulation");
    await settle();
    t(/binary raw file/.test(consoleText()), "ngspice ran and wrote its raw file");
    t(await plotSignal(RC), "the RC's voltage is a signal of the run");
    // the closing question: the window was changed
    click(q('button[aria-label="Close"]'));
    await sleep(500);
    const ask = dialog("Save Changes?");
    t(!!ask && /Save changes to workbook\?/.test(ask.textContent), "closing asks to save the workbook");
    click([...(ask?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Discard Changes"));
    await sleep(500);
    t(!win(), "Discard Changes closes the window");
    await E.run("common.Interactive.undo");
    await sleep(800);
    const sch = await post("/api/sim/circuit", {});
    t(sch.sch_sim_command === "", `the text is gone again: ${JSON.stringify(sch.sch_sim_command)}`);
  });

  await scenario("a transient tab: run, plot, cursor at 1 ms, a measurement and a user-defined signal", async (t) => {
    await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2000);
    t(win()?.querySelectorAll(".sim-tab").length === 0, "no analysis on the sheet: no tab");
    const ok = await newTab("TRAN", async (d) => {
      setValue(d.querySelector('[aria-label="Time step:"]'), "10u");
      setValue(d.querySelector('[aria-label="Final time:"]'), "5m");
      const uic = [...d.querySelectorAll("label")].find((l) => l.textContent.includes("Use initial conditions"))?.querySelector("input");
      uic?.click();
      await sleep(100);
    });
    t(ok, "the dialog makes the tab");
    t((q(".sim-tab.active")?.textContent ?? "").startsWith("Analysis 1 - TRAN"), "Analysis 1 - TRAN");
    await run("eeschema.Simulation.runSimulation");
    await settle();
    const before = plotPixels();
    t(await plotSignal(RC), `${RC} plotted`);
    await sleep(300);
    t(plotPixels() !== before, "the trace is drawn");
    const c1 = signalRow(RC)?.querySelector(`input[aria-label="Cursor 1 on ${RC}"]`);
    c1?.click();
    await sleep(300);
    const x = q('table[aria-label="Cursors"] input');
    t(!!x, "cursor 1's row");
    if (x) await commit(x, "1m");
    await sleep(300);
    const rows = cursorRows();
    t(rows.length === 1 && rows[0][0] === "1" && rows[0][1] === RC, `the cursors grid: ${JSON.stringify(rows)}`);
    t(rows[0]?.[3] === "7.58V" || rows[0]?.[3] === "7.59V", `cursor 1 at 1 ms reads 12 (1 - e^-1) = 7.59 V: ${rows[0]?.[3]}`);
    // a measurement typed in the empty row, measured by the run it starts
    const m = q('input[aria-label="Measurement 1"]');
    if (m) await commit(m, `MAX ${RC}`);
    await settle();
    const ms = measurementRows();
    t(ms[0]?.[0] === `MAX ${RC}` && /^11\.9V$/.test(ms[0]?.[1] ?? ""), `MAX of the RC is 12 (1 - e^-5) = 11.9 V: ${JSON.stringify(ms)}`);
    t(ms.length === 2 && ms[1][0] === "", "one empty row stays to type in");
    // a user-defined signal
    await run("eeschema.Simulation.editUserDefinedSignals");
    await sleep(400);
    const u = dialog("User-defined Signals");
    click(u?.querySelector('button[aria-label="Add signal"]'));
    await sleep(150);
    setValue(u.querySelector('input[aria-label="Signal 1"]'), `${RC}*2`);
    click([...u.querySelectorAll("button")].find((b) => b.textContent.trim() === "OK"));
    await settle();
    t(!!signalRow(`${RC}*2`), "the user-defined signal is listed");
    t(/let user0 = /.test(consoleText()), "the run made it (`let user0 = ...`)");
  });

  await scenario("the plot's zooms, their history, and the View toggles", async (t) => {
    t((await run("common.Control.undoZoom")).ok === false, "no zoom to undo yet");
    const p0 = plotPixels();
    await run("common.Control.zoomInHorizontally");
    await sleep(300);
    t(plotPixels() !== p0, "Zoom In Horizontally changes the plot");
    await run("common.Control.zoomInVertically");
    await run("common.Control.zoomOutHorizontally");
    await run("common.Control.zoomOutVertically");
    t((await run("common.Control.undoZoom")).ok === true, "Undo Last Zoom is enabled after a zoom");
    await sleep(200);
    t((await run("common.Control.redoZoom")).ok === true, "Redo Last Zoom after an undo");
    await run("common.Control.zoomFitScreen");
    await sleep(200);
    for (const [id, label] of [["eeschema.Simulator.toggleLegend", "legend"], ["eeschema.Simulator.toggleDottedSecondary", "dotted current/phase"], ["eeschema.Simulator.toggleDarkModePlots", "dark mode"]]) {
      const p = plotPixels();
      const r = await run(id);
      await sleep(300);
      t(r.ok === true, `${label} is enabled`);
      if (id !== "eeschema.Simulator.toggleDottedSecondary") t(plotPixels() !== p, `${label} changes the plot`);
      await run(id);
      await sleep(200);
    }
    await run("eeschema.Simulator.toggleSimConsole");
    await sleep(200);
    t(!q(".sim-console"), "the console hides");
    await run("eeschema.Simulator.toggleSimConsole");
    await run("eeschema.Simulator.toggleSimSidePanel");
    await sleep(200);
    t(!q(".sim-side"), "the side panel hides");
    await run("eeschema.Simulator.toggleSimSidePanel");
    await sleep(200);
    t(!!q(".sim-side") && !!q(".sim-console"), "both back");
  });

  await scenario("the exports: PNG, CSV, the clipboard and the plot pasted on the sheet", async (t) => {
    saved.length = 0;
    await run("eeschema.Simulator.exportPNG");
    await sleep(800);
    const png = saved.find((s) => s.name?.endsWith(".png"));
    t(!!png && png.blob.type === "image/png" && png.blob.size > 1000, `a PNG of the plot: ${png?.name} ${png?.blob.size}`);
    await run("eeschema.Simulator.exportCSV");
    await sleep(500);
    const csv = saved.find((s) => s.name?.endsWith(".csv"));
    const text = csv ? await csv.blob.text() : "";
    const lines = text.split("\r\n");
    t(lines[0] === `time;${RC};`, `the CSV's header: ${JSON.stringify(lines[0])}`);
    const rows = lines.slice(1).filter((l) => l !== "").map((l) => l.split(";"));
    t(rows.length > 100 && rows.every((r) => r.length === 3 && r[2] === "" && Number.isFinite(Number(r[0])) && Number.isFinite(Number(r[1]))), `a row per point, '%g;' each: ${JSON.stringify(lines[1])}`);
    t(rows.every((r, i) => i === 0 || Number(r[0]) > Number(rows[i - 1][0])), "the times rise");
    await run("eeschema.Simulator.exportToClipboard");
    await sleep(800);
    t(clipboard.length === 1 && clipboard[0].types.includes("image/png"), "the picture on the clipboard");
    const r = await run("eeschema.Simulator.exportPlotToSchematic");
    await sleep(1500);
    t(r.ok !== false, "Export Plot to Schematic starts a paste");
    await E.run("common.Interactive.cancel");
    await sleep(400);
  });

  await scenario("an operating point in the console and on the sheet", async (t) => {
    t(await newTab("OP", async () => {}), "an OP tab");
    await run("eeschema.Simulation.runSimulation");
    await settle();
    const c = consoleText();
    t(/Simulation results:/.test(c), "Simulation results: in the console");
    t(/V\(\/out\):\s+989(\.\d+)?mV/.test(c), `V(/out) = 989 mV, the gain of 11 on 0.1 V less the offset and the finite gain: ${(c.match(/V\(\/out\):.*$/m) || [""])[0]}`);
    t(q(".sim-noplot")?.textContent === "Simulation provided no plots. Please refer to console window for results.", "an OP tab has no plot");
    t(action("eeschema.EditorControl.showOperatingPointVoltages")?.enabled === true, "Show OP Voltages is offered");
    const before = document.querySelector(".pcb-canvas-container canvas")?.toDataURL();
    await E.run("eeschema.EditorControl.showOperatingPointVoltages");
    await E.run("eeschema.EditorControl.showOperatingPointCurrents");
    await sleep(500);
    const after = document.querySelector(".pcb-canvas-container canvas")?.toDataURL();
    t(before !== after, "the voltages and currents go from the sheet");
    await E.run("eeschema.EditorControl.showOperatingPointVoltages");
    await E.run("eeschema.EditorControl.showOperatingPointCurrents");
    await sleep(300);
    t(document.querySelector(".pcb-canvas-container canvas")?.toDataURL() === before, "and come back");
  });

  await scenario("a DC sweep of the supply, and the SPICE netlist", async (t) => {
    t(await newTab("DC", async (d) => {
      setValue(d.querySelector('select[aria-label="Source 1"]'), "V1");
      setValue(d.querySelector('[aria-label="Starting value:"]'), "0");
      setValue(d.querySelector('[aria-label="Final value:"]'), "12");
      setValue(d.querySelector('[aria-label="Increment step:"]'), "1");
      await sleep(100);
    }), "a DC tab");
    await run("eeschema.Simulation.runSimulation");
    await settle();
    t(await plotSignal("V(/IN)"), "V(/IN) plotted");
    await run("eeschema.Simulation.showNetlist");
    await sleep(1500);
    const n = dialog("SPICE Netlist");
    const text = n?.querySelector('[aria-label="SPICE netlist"]')?.textContent ?? "";
    t(/\.dc V1 0 12 1/.test(text) && /\.save all/.test(text) && /XU1 /.test(text), "the netlist has the tab's command, .save all and the op-amp");
    click([...(n?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Cancel"));
    await sleep(300);
  });

  await scenario("Probe a wire, Tune a resistor, and Save the tuned value", async (t) => {
    // back to the transient tab
    click(win()?.querySelector(".sim-tab"));
    await sleep(300);
    await run("eeschema.Simulation.runSimulation");
    await settle();
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
    t((await run("eeschema.Simulation.probe")).ok === true, "Probe is enabled once a run is over");
    await sleep(300);
    const sch = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const wire = sch.wires.find((w) => w.pins.includes("U1.5") || w.pins.includes("R2.2"));
    const box = document.querySelector(".pcb-canvas-container");
    const fire = (type, x, y) => { const rect = box.getBoundingClientRect(); (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" ? 1 : 0 })); };
    const view = E.state().view;
    if (wire) {
      const [a, b] = wire.pts;
      const mx = (a[0] + b[0]) / 2, my = (a[1] + b[1]) / 2;
      const px = view.x + mx * view.scale, py = view.y + my * view.scale;
      fire("pointermove", px, py);
      fire("pointerdown", px, py);
      fire("pointerup", px, py);
      await sleep(500);
    }
    t(!!wire && !!signalRow("V(/OUT)")?.querySelector('input[aria-label="Plot V(/OUT)"]')?.checked, "the probe plots V(/OUT)");
    await E.run("common.Interactive.cancel");
    await sleep(300);
    // Tune R5
    await run("eeschema.Simulation.tune");
    await sleep(300);
    // R5's Reference: a field picks its symbol (`SelectPoint( { SCH_SYMBOL_T, SCH_FIELD_T } )`)
    const r5 = sch.symbols.find((s) => s.id === "R5");
    const ref = r5.fields?.find((f) => f.name === "Reference")?.at ?? r5.at;
    const px = view.x + ref[0] * view.scale, py = view.y + ref[1] * view.scale;
    fire("pointermove", px, py);
    fire("pointerdown", px, py);
    fire("pointerup", px, py);
    await sleep(800);
    await E.run("common.Interactive.cancel");
    const tuner = q('[aria-label="Tune R5"]');
    t(!!tuner, "a tuner for R5");
    t(tuner?.querySelector('[aria-label="R5 minimum"]')?.value === "500" && tuner?.querySelector('[aria-label="R5 maximum"]')?.value === "2k", "its range is half to twice 1k");
    const value = tuner?.querySelector('[aria-label="R5 value"]');
    if (value) await commit(value, "2k");
    await settle();
    const x = q('table[aria-label="Cursors"] input');
    if (x) await commit(x, "1m");
    await sleep(300);
    const rows = cursorRows();
    t(rows[0]?.[3] === "4.72V", `with R5 = 2k the RC at 1 ms is 12 (1 - e^-0.5) = 4.72 V: ${rows[0]?.[3]}`);
    const save = [...(tuner?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Save");
    save?.click();
    await sleep(1200);
    const after = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const val = (s) => s.symbols.find((x) => x.id === "R5")?.fields?.find((f) => f.name === "Value")?.text;
    t(val(after) === "2k", `Save writes 2k to R5's value: ${val(after)}`);
    await E.run("common.Interactive.undo");
    await sleep(800);
    const back = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    t(val(back) === "1k", `one undo step takes it back: ${val(back)}`);
    click(tuner?.querySelector('button[aria-label="Remove tuner R5"]'));
    await sleep(200);
  });

  await scenario("the workbook: saved in the design, opened again, and a KiCad workbook opened", async (t) => {
    await run("eeschema.Simulation.saveWorkbook");
    await sleep(500);
    const d = dialog("Save Simulation Workbook As");
    t(!!d && d.querySelector('[aria-label="Workbook file name"]').value.endsWith(".wbk"), "Save asks for a name: <project>.wbk");
    click([...(d?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Save"));
    await sleep(1500);
    const settings = (await post("/api/sim/circuit", {})).settings;
    t(!!settings?.workbook && JSON.parse(settings.workbook).version === 7 && settings.ngspice?.workbook_filename?.endsWith(".wbk"), "the design has the workbook and its name");
    const wb = JSON.parse(settings?.workbook ?? "{}");
    t(wb.tabs?.length === 3 && wb.tabs[0].analysis === "TRAN" && wb.user_defined_signals?.[0] === `${RC}*2`, "its three tabs and the user-defined signal");
    t(settings?.workbook?.startsWith('{\n  "custom_cursors": 2,\n'), "written as KiCad writes it: sorted keys, two-space indents");
    t(!(q(".sim-title")?.textContent ?? "").startsWith("*"), `saved, the title has no *: ${q(".sim-title")?.textContent}`);
    // closed and opened again: the workbook the project names
    click(q('button[aria-label="Close"]'));
    await sleep(600);
    t(!win(), "closed without a question");
    await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2500);
    t(win()?.querySelectorAll(".sim-tab").length === 3, "opened again on its three tabs");
    // a KiCad workbook from a file
    const kicad = '{\n  "custom_cursors": 2,\n  "last_sch_text_sim_command": "",\n  "tabs": [\n    {\n      "analysis": "OP",\n      "commands": [\n        ".op",\n        ".save all"\n      ]\n    }\n  ],\n  "user_defined_signals": [],\n  "version": 7\n}\n';
    nextFile = new File([kicad], "kicad.wbk");
    await run("eeschema.Simulation.openWorkbook");
    await sleep(2000);
    const tabs = [...(win()?.querySelectorAll(".sim-tab") ?? [])].map((x) => x.textContent);
    t(tabs.length === 1 && tabs[0].startsWith("Analysis 1 - OP"), `the file's one tab: ${tabs}`);
    const s2 = (await post("/api/sim/circuit", {})).settings;
    t(s2?.ngspice?.workbook_filename === "kicad.wbk" && s2?.workbook === kicad, "the project names it, byte for byte");
    // the workbook steps undone
    await E.run("common.Interactive.undo");
    await sleep(600);
    await E.run("common.Interactive.undo");
    await sleep(800);
    const s3 = (await post("/api/sim/circuit", {})).settings;
    t(!s3?.workbook && !s3?.ngspice, "two undo steps and the design has no workbook again");
  });

  await scenario("Stop Simulation kills a long run", async (t) => {
    t(await newTab("TRAN", async (d) => {
      setValue(d.querySelector('[aria-label="Time step:"]'), "1n");
      setValue(d.querySelector('[aria-label="Final time:"]'), "10");
      setValue(d.querySelector('[aria-label="Max time step:"]'), "1n");
      await sleep(100);
    }), "a transient of ten seconds in steps of a nanosecond");
    focusSim();
    void E.run("eeschema.Simulation.runSimulation");
    await sleep(1500);
    t(q('button[aria-label="Stop Simulation"]')?.disabled === false, "Stop is enabled while it runs");
    t(runButton()?.disabled === true, "and Run is not");
    await run("eeschema.Simulation.stopSimulation");
    await settle(20000);
    t(runButton() && !runButton().disabled, "the run is over");
    // `onSimFinished` follows a Stop (`m_simFinished = true`): Probe and Tune stay enabled
    t((await run("eeschema.Simulation.probe")).ok === true, "Probe is enabled after a Stop, as onSimFinished leaves it");
    await E.run("common.Interactive.cancel");
    await sleep(300);
    // close, discarding the new tab
    click(q('button[aria-label="Close"]'));
    await sleep(500);
    click([...(dialog("Save Changes?")?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Discard Changes"));
    await sleep(400);
  });

  await scenario("nothing logged an error", async (t) => {
    const errs = E.errors(errorsFrom);
    t(errs.length === 0, `errors: ${JSON.stringify(errs).slice(0, 600)}`);
  });

  URL.createObjectURL = realCreate;
  HTMLAnchorElement.prototype.click = realClick;
  HTMLInputElement.prototype.click = realInputClick;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
