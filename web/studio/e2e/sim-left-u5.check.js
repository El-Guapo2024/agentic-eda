// The simulator's leftovers of UI-LEFT M3 (task-u5-dialogs; PARITY-sch.md, the simulator section of task-u5-dialogs): run in the page of a served
// board made from web/studio/e2e/sim-left-board.yaml (`eda board new web/studio/e2e/sim-left-board.yaml -o <dir>`, `eda board serve -C <dir>
// --ui web/studio/dist`): `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:<port>/ web/studio/e2e/sim-left-u5.check.js`.
//
// No ngspice is needed: the circuit is kicad-cli's (`/api/sim/circuit`, real), and the check answers `/api/sim/run` itself with vectors of the
// shape ngspice's raw file gives (each run's v(/w) scaled by R1's tuned value, so the runs of a multi-run differ). What it checks:
//   an SP tab lists S_<a>_<b> of the two sources made ports (`portnum`), amplitude and phase, and plots one;
//   User-defined Signals: the cell offers ngspice's functions and the run's vectors as KiCad's onScintillaCharAdded does, Enter takes the item and
//     types no new line, Syntax help is KiCad's page;
//   Probe: KiCad's voltage-probe cursor, the current probe over a pin, the net under the pointer highlighted, and the highlight gone with Escape;
//   Tune a potentiometer: its range 0 to 1 from its wiper's position, its Save written into Sim.Params;
//   Tune's Save of an inferred R: the Value with the hidden Sim.Device and Sim.Pins (r7 R7-SM-1), one undo step;
//   a multi-run tuner: each run's cursor label ("R1=500", ...) drawn by the cursor, a double click on the cursor going on to the next run;
//   nothing logged an error.
//
// It answers through `window.__simLeftCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=sim-left minutes=8
(async () => {
  const results = [];
  const check = (window.__simLeftCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await realFetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await realFetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const realFetch = window.fetch.bind(window);
  const post = (url, body) => realFetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const errorsFrom = Date.now();

  // ---- ngspice, answered here: the run's vectors as the server reads them from the raw file
  const runs = [];
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input.url;
    if (url.endsWith("/api/sim/run")) {
      const req = JSON.parse(init?.body ?? "{}");
      runs.push(req);
      const r1 = (req.tuners ?? []).find((x) => x.ref === "R1")?.value ?? 1000;
      const cmd = String(req.command ?? "").trim().toLowerCase();
      let plot;
      if (cmd.startsWith(".sp")) {
        const f = [1e3, 1e4, 1e5, 1e6];
        const s = (a) => ({ name: a, kind: "notype", re: f.map((_, i) => 0.1 * (i + 1)), im: f.map(() => 0.05) });
        plot = { plot_name: "sp1", complex: true, points: f.length, vectors: [{ name: "frequency", kind: "frequency", re: f, im: f.map(() => 0) }, s("S_1_1"), s("S_1_2"), s("S_2_1"), s("S_2_2")] };
      } else {
        const tm = [0, 1e-3, 2e-3, 3e-3, 4e-3];
        plot = { plot_name: "tran1", complex: false, points: tm.length, vectors: [{ name: "time", kind: "time", re: tm }, { name: "v(/w)", kind: "voltage", re: tm.map((x) => (x * r1) / 1e-3 / 1000) }, { name: "v(/in)", kind: "voltage", re: tm.map(() => 1) }, { name: "v(/out)", kind: "voltage", re: tm.map(() => 0) }, { name: "i(v1)", kind: "current", re: tm.map(() => -1e-4) }] };
      }
      return new Response(JSON.stringify({ ok: true, console: "Circuit: KiCad schematic\n", plots: [plot], second_plots: [], messages: [] }), { status: 200, headers: { "Content-Type": "application/json" } });
    }
    return realFetch(input, init);
  };
  // the texts the plot's canvas writes (the cursors' numbers and run labels)
  const written = [];
  const realFillText = CanvasRenderingContext2D.prototype.fillText;
  CanvasRenderingContext2D.prototype.fillText = function (text, x, y, w) { if (this.canvas?.dataset?.testid === "sim-plot") written.push(String(text)); return realFillText.call(this, text, x, y, w); };

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
  const runButton = () => q('button[aria-label="Run Simulation"]');
  const settle = async (ms = 30000) => { await sleep(300); const t0 = Date.now(); while (runButton()?.disabled && Date.now() - t0 < ms) await sleep(200); await sleep(300); };
  const signalRow = (name) => [...(q('table[aria-label="Signals"]')?.querySelectorAll("tbody tr") ?? [])].find((r) => r.querySelector("td")?.textContent === name);
  const signalNames = () => [...(q('table[aria-label="Signals"]')?.querySelectorAll("tbody tr") ?? [])].map((r) => r.querySelector("td")?.textContent ?? "");
  const plotSignal = async (name) => { const box = signalRow(name)?.querySelector(`input[aria-label="Plot ${name}"]`); if (box && !box.checked) { box.click(); await sleep(300); } return !!box?.checked; };
  const newTab = async (type, fill) => {
    await run("eeschema.Simulation.newAnalysisTab");
    await sleep(800);
    const d = dialog("New Simulation Tab");
    if (!d) return false;
    setValue(d.querySelector('select[aria-label="Analysis type"]'), type);
    await sleep(150);
    await fill(d);
    click([...d.querySelectorAll("button")].find((b) => b.textContent.trim() === "OK"));
    await sleep(600);
    return !dialog("New Simulation Tab");
  };
  const symbolFields = (ref) => post("/api/sim/model/symbol", { ref });
  // the sheet, and a point of it as the canvas sees it (`state.schematicView`)
  const box = () => document.querySelector(".pcb-canvas-container");
  const toScreen = ([x, y]) => { const v = E.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const fire = (type, [x, y]) => { const b = box(); const rect = b.getBoundingClientRect(); (b.querySelector("canvas") || b).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" ? 1 : 0 })); };
  const clickAt = async (p) => { fire("pointermove", p); fire("pointerdown", p); fire("pointerup", p); await sleep(600); };

  // ---- the board: V1 and V2 S-parameter ports 1 and 2, RV1's wiper at 0.3
  const schematic = await realFetch("/api/schematic", { cache: "no-store" }).then((r) => r.json()).catch(() => null);
  if (!["V1", "V2", "RV1", "R1"].every((r) => schematic?.symbols?.some((s) => s.id === r))) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs web/studio/e2e/sim-left-board.yaml's circuit"] });
    check.done = true;
    return;
  }
  const edit = (id, field, value) => ({ id, field, value });
  const set = await post("/api/cmd", { cmd: { op: "set_symbol_fields", edits: [edit("V1", "Sim.Params", "dc=0 ac=1 portnum=1 z0=50"), edit("V2", "Sim.Params", "dc=0 ac=1 portnum=2 z0=50"), edit("RV1", "Sim.Params", "r=10k pos=0.3")] }, strict: false });
  [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
  await sleep(2000);

  await scenario("an SP tab lists the S-parameters of the sources' ports and plots one", async (t) => {
    t(set.ok !== false, `the sources are ports 1 and 2: ${JSON.stringify(set).slice(0, 200)}`);
    await E.run("eeschema.EditorControl.showSimulator");
    await sleep(2000);
    t(!!win(), "the window is open");
    const ok = await newTab("SP", async (d) => {
      setValue(d.querySelector('[aria-label="Number of points per decade:"]'), "10");
      setValue(d.querySelector('[aria-label="Start frequency:"]'), "1k");
      setValue(d.querySelector('[aria-label="Stop frequency:"]'), "1Meg");
      await sleep(100);
    });
    t(ok, "the dialog makes the SP tab");
    await sleep(800);
    const names = signalNames();
    const want = ["S_1_1 (amplitude)", "S_1_1 (phase)", "S_1_2 (amplitude)", "S_1_2 (phase)", "S_2_1 (amplitude)", "S_2_1 (phase)", "S_2_2 (amplitude)", "S_2_2 (phase)"];
    t(want.every((w) => names.includes(w)), `the signals: ${JSON.stringify(names)}`);
    t(!names.some((n) => /^S_.*_3/.test(n)), "no third port");
    await run("eeschema.Simulation.runSimulation");
    await settle();
    t(runs.length >= 1 && /^\.sp /.test(runs[runs.length - 1].command), `the run is the SP analysis: ${runs[runs.length - 1]?.command}`);
    t(await plotSignal("S_2_1 (amplitude)"), "S_2_1 (amplitude) plotted");
  });

  await scenario("User-defined Signals complete ngspice's functions and the run's vectors", async (t) => {
    await run("eeschema.Simulation.editUserDefinedSignals");
    await sleep(400);
    const u = dialog("User-defined Signals");
    t(!!u, "the dialog");
    click(u?.querySelector('button[aria-label="Add signal"]'));
    await sleep(200);
    const cell = u?.querySelector('textarea[aria-label="Signal 1"]');
    t(!!cell, "the row's cell is the completing editor");
    if (!cell) return;
    cell.focus();
    for (const ch of "sq") { document.execCommand("insertText", false, ch); await sleep(60); }
    await sleep(200);
    // the list is Scintilla's popup, on the page (a portal), not inside the dialog
    const list = () => [...document.querySelectorAll('[data-testid="sim-user-signal-0-completions"] .rte-item')].map((e) => e.textContent);
    t(list().includes("sqrt(x)"), `"sq" offers sqrt(x): ${JSON.stringify(list())}`);
    cell.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await sleep(200);
    t(cell.value === "sqrt(x)", `Enter takes it: ${JSON.stringify(cell.value)}`);
    cell.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    document.execCommand("insertText", false, "\n");
    await sleep(150);
    t(!/\n/.test(cell.value), `one line: no new line goes in (${JSON.stringify(cell.value)})`);
    // the run's vectors are offered too (the SP run's `frequency`; a '_' ends the word as it does KiCad's scan, so "S_2" offers from "2")
    setValue(cell, "");
    await sleep(100);
    cell.focus();
    for (const ch of "2*freq") { document.execCommand("insertText", false, ch); await sleep(60); }
    await sleep(200);
    t(list().includes("frequency"), `"freq" offers the plot's vector: ${JSON.stringify(list())}`);
    click([...u.querySelectorAll("button")].find((b) => b.textContent.trim() === "Syntax help"));
    await sleep(200);
    const help = u.querySelector('[aria-label="Syntax Help"]')?.textContent ?? "";
    t(help.startsWith("SPICE functions:") && help.includes("ternary_fcn(x, y, z)") && help.includes("limit(nom, avar)"), "Syntax help is KiCad's page");
    click([...u.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel"));
    await sleep(300);
  });

  await scenario("Probe: KiCad's probe cursors, the net under the pointer highlighted, Escape takes it away", async (t) => {
    const ok = await newTab("TRAN", async (d) => {
      setValue(d.querySelector('[aria-label="Time step:"]'), "1m");
      setValue(d.querySelector('[aria-label="Final time:"]'), "4m");
      await sleep(100);
    });
    t(ok, "a TRAN tab");
    await run("eeschema.Simulation.runSimulation");
    await settle();
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
    t((await run("eeschema.Simulation.probe")).ok === true, "Probe is enabled once a run is over");
    await sleep(300);
    const c = box();
    t(/data:image\/svg\+xml.*\) 1 31, crosshair/.test(c?.style.cursor ?? ""), `the voltage probe at its hot spot (1, 31): ${(c?.style.cursor ?? "").slice(-40)}`);
    const sch = await realFetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const wire = sch.wires.find((w) => w.net === "/W" && w.pts.length >= 2);
    t(!!wire, "a wire of W");
    if (!wire) return;
    const [a, b] = wire.pts;
    fire("pointermove", toScreen([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]));
    await sleep(300);
    t(E.state().highlight.includes("/W"), `the net under the pointer is highlighted: ${JSON.stringify(E.state().highlight)}`);
    t(/\) 1 31, crosshair/.test(c.style.cursor), "a wire: the voltage probe");
    // a pin at a wire's end: the current probe, the pin brightened and no net highlighted
    let onPin = false;
    for (const w of sch.wires.filter((x) => x.pins?.length)) {
      for (const end of [w.pts[0], w.pts[w.pts.length - 1]]) {
        fire("pointermove", toScreen(end));
        await sleep(120);
        if (/\) 4 27, crosshair/.test(c.style.cursor)) { onPin = true; break; }
      }
      if (onPin) break;
    }
    t(onPin, "over a pin: the current probe at its hot spot (4, 27)");
    t(onPin && E.state().highlight.length === 0, `no net highlighted over a pin: ${JSON.stringify(E.state().highlight)}`);
    fire("pointermove", toScreen([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]));
    await sleep(200);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    t(E.state().highlight.length === 0, `Escape takes the highlight away: ${JSON.stringify(E.state().highlight)}`);
    t(!/svg/.test(c.style.cursor), "and the probe cursor");
  });

  const tuneSymbol = async (ref) => {
    await run("eeschema.Simulation.tune");
    await sleep(300);
    const sch = await realFetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const s = sch.symbols.find((x) => x.id === ref);
    const at = s?.fields?.find((f) => f.name === "Reference")?.at ?? s?.at;
    if (at) await clickAt(toScreen(at));
    await E.run("common.Interactive.cancel");
    await sleep(300);
    return q(`[aria-label="Tune ${ref}"]`);
  };

  await scenario("Tune a potentiometer: 0 to 1 from its wiper, saved into Sim.Params", async (t) => {
    const tuner = await tuneSymbol("RV1");
    t(!!tuner, "a tuner for RV1");
    if (!tuner) return;
    t(tuner.querySelector('[aria-label="RV1 minimum"]')?.value === "0" && tuner.querySelector('[aria-label="RV1 maximum"]')?.value === "1", `its range is 0 to 1: ${tuner.querySelector('[aria-label="RV1 minimum"]')?.value} .. ${tuner.querySelector('[aria-label="RV1 maximum"]')?.value}`);
    t(tuner.querySelector('[aria-label="RV1 value"]')?.value === "300m", `it starts at the wiper's 0.3: ${tuner.querySelector('[aria-label="RV1 value"]')?.value}`);
    const before = runs.length;
    await commit(tuner.querySelector('[aria-label="RV1 value"]'), "250m");
    await settle();
    t(runs.length > before && runs[runs.length - 1].tuners?.some((x) => x.ref === "RV1" && Math.abs(x.value - 0.25) < 1e-9), `the run tunes RV1 to 0.25: ${JSON.stringify(runs[runs.length - 1]?.tuners)}`);
    click([...tuner.querySelectorAll("button")].find((b) => b.textContent.trim() === "Save"));
    await sleep(1500);
    const f = await symbolFields("RV1");
    t(f.fields?.["Sim.Params"] === "r=10k pos=250m", `Save writes the wiper into Sim.Params: ${JSON.stringify(f.fields)}`);
    click(tuner.querySelector('button[aria-label="Remove tuner RV1"]'));
    await sleep(300);
  });

  await scenario("Tune's Save of an inferred R writes its Value with the hidden Sim.Device and Sim.Pins, one undo step", async (t) => {
    const tuner = await tuneSymbol("R1");
    t(!!tuner, "a tuner for R1");
    if (!tuner) return;
    await commit(tuner.querySelector('[aria-label="R1 value"]'), "2k");
    await settle();
    const before = await symbolFields("R1");
    t(!before.fields?.["Sim.Device"], `R1 has no model of its own before: ${JSON.stringify(before.fields)}`);
    click([...tuner.querySelectorAll("button")].find((b) => b.textContent.trim() === "Save"));
    await sleep(1500);
    const after = await symbolFields("R1");
    t(after.value === "2k", `the Value: ${after.value}`);
    t(after.fields?.["Sim.Device"] === "R" && after.fields?.["Sim.Pins"] === "1=+ 2=-" && !after.fields?.["Sim.Params"], `Sim.Device and Sim.Pins written as WriteFields writes them: ${JSON.stringify(after.fields)}`);
    t((after.own ?? []).includes("Sim.Device"), "they are the instance's own");
    const sch = await realFetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const r1 = sch.symbols.find((s) => s.id === "R1");
    const dev = (r1?.fields ?? []).find((x) => x.name === "Sim.Device");
    t(!dev || dev.visible === false || dev.hidden === true, `Sim.Device is hidden on the sheet: ${JSON.stringify(dev)}`);
    await E.run("common.Interactive.undo");
    await sleep(1200);
    const undone = await symbolFields("R1");
    t(undone.value === "1k" && !undone.fields?.["Sim.Device"], `one undo takes it back: ${undone.value} ${JSON.stringify(undone.fields)}`);
  });

  await scenario("a multi-run tuner: each run's label by the cursor, a double click goes on to the next run", async (t) => {
    const tuner = q('[aria-label="Tune R1"]');
    t(!!tuner, "R1's tuner");
    if (!tuner) return;
    t(await plotSignal("V(/W)"), "V(/W) plotted");
    await commit(tuner.querySelector('[aria-label="R1 minimum"]'), "500");
    await commit(tuner.querySelector('[aria-label="R1 maximum"]'), "1.5k");
    await settle();
    const steps = tuner.querySelector('input[type="number"]');
    setValue(steps, "3");
    const before = runs.length;
    setValue(tuner.querySelector('[aria-label="R1 run mode"]'), "multi");
    await sleep(500);
    const t0 = Date.now();
    while (runs.length < before + 3 && Date.now() - t0 < 20000) await sleep(200);
    await settle();
    const tuned = runs.slice(before).map((r) => r.tuners?.find((x) => x.ref === "R1")?.value);
    t(JSON.stringify(tuned) === JSON.stringify([500, 1000, 1500]), `one run per step: ${JSON.stringify(tuned)}`);
    signalRow("V(/W)")?.querySelector('input[aria-label="Cursor 1 on V(/W)"]')?.click();
    await sleep(500);
    written.length = 0;
    E.run("common.Control.zoomFitScreen");
    focusSim();
    await sleep(600);
    t(written.includes("R1=500"), `the cursor names its run: ${JSON.stringify([...new Set(written)].filter((w) => /=|Run/.test(w)))}`);
    // a double click on the cursor's line: the next run
    const canvas = q("canvas[data-testid=sim-plot]");
    const rect = canvas.getBoundingClientRect();
    const v = window.__eda && null;
    void v;
    // the cursor stands at 40 % of the plot area's width (`EnableCursor`)
    let moved = false;
    for (let px = 0.2; px < 0.8 && !moved; px += 0.01) {
      written.length = 0;
      canvas.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, clientX: rect.left + rect.width * px, clientY: rect.top + rect.height / 2 }));
      await sleep(80);
      if (written.includes("R1=1k")) moved = true;
    }
    t(moved, "a double click on the cursor goes on to R1=1k");
  });

  await scenario("nothing logged an error", async (t) => {
    const errs = E.errors(errorsFrom);
    t(errs.length === 0, `errors: ${JSON.stringify(errs).slice(0, 600)}`);
  });

  window.fetch = realFetch;
  CanvasRenderingContext2D.prototype.fillText = realFillText;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
