// Browser check for the Simulation Model dialog's built-in models and IBIS (UI-LEFT M1; web/studio/PARITY-sch.md section 57;
// kicad-port/sim/simModelBuiltin.ts, simModelDialog.ts, components/simulator/SimModelDialog.tsx), opened from Symbol Properties'
// "Simulation Model...":
//   a resistor: inferred from its Value (Built-in, Resistor, Ideal), its value edited in the Parameters grid, "Save parameter 'Resistance (r)' in
//   Value field" unticked and ticked again, the Code tab, OK handing Value, Sim.Device and Sim.Pins (shown) to Symbol Properties, whose OK
//   writes them in one undo step;
//   a voltage source: DC made a Sine, its parameters typed (the delay, damping and phase filled with 0 as KiCad's autofill does), the AC group
//   expanded, the checkbox disabled, the Code tab, OK writing Sim.Type and Sim.Params;
//   a diode: its parameters in their categories, a model and an instance parameter, the Code tab's .model card, OK writing Sim.Params;
//   kicad-cli's SPICE netlist holding the lines the Code tab shows;
//   another diode given an IBIS model of models/io.ibs: its component, pins, a pin's models, the Waveform choice, OK writing the IBIS fields;
//   nothing logged an error.
//
// It answers through `window.__simModelCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=sim-model minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__simModelCheck = { results, done: false, ok: false, wait: null });
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
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const click = (el) => el?.click();
  const dlg = () => document.querySelector('[data-testid="sim-model-dialog"]');
  const $ = (id) => dlg()?.querySelector(`[data-testid="${id}"]`) ?? null;
  const fields = async (ref) => post("/api/sim/model/symbol", { ref });
  /** A parameter's row of the grid, and its value editor. */
  const row = (name) => dlg()?.querySelector(`[data-testid="sim-model-param-grid"] tr[data-param="${name}"]`) ?? null;
  const cell = (name) => row(name)?.querySelector("input, select") ?? null;
  /** Type a value into a grid cell and leave it (the grid commits on leaving, as wxPropertyGrid does). */
  const typeParam = async (name, v) => {
    const el = cell(name);
    if (!el) return false;
    el.focus();
    setValue(el, v);
    el.blur();
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await sleep(150);
    return true;
  };
  const codeTab = async () => { click($("sim-model-code-tab")); await sleep(200); return $("sim-model-code")?.value ?? null; };
  const paramsTab = async () => { click($("sim-model-params-tab")); await sleep(200); };
  /** Symbol Properties on `ref`, then its Simulation Model... button; the dialog once it has read the symbol and the tables. */
  const openDialog = async (ref) => {
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [ref]);
    await sleep(300);
    await E.run("eeschema.InteractiveEdit.properties");
    await sleep(500);
    click(document.querySelector('[data-testid="symbol-sim-model"]'));
    const t0 = Date.now();
    while (!$("sim-model-device") && !$("sim-model-list")?.options.length && Date.now() - t0 < 10000) await sleep(200);
    await sleep(300);
    return !!dlg();
  };
  /** The Simulation Model dialog's OK: it hands its fields to Symbol Properties (`OnEditSpiceModel`), which still shows. */
  const simOk = async () => { click($("sim-model-ok")); const t0 = Date.now(); while (dlg() && Date.now() - t0 < 8000) await sleep(200); await sleep(300); return !dlg(); };
  const propsDialog = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector('[data-testid="symbol-sim-model"]')) ?? null;
  /** Symbol Properties' OK: what applies the fields, one undo step. */
  const propsOk = async () => {
    const b = [...(propsDialog()?.querySelectorAll(".dialog-footer button.primary") ?? [])].find((x) => x.textContent.trim() === "OK");
    click(b);
    const t0 = Date.now();
    while (propsDialog() && Date.now() - t0 < 8000) await sleep(200);
    await sleep(700);
    return !propsDialog();
  };
  const ok = async () => (await simOk()) && (await propsOk());
  /** KiCad's error box (`DisplayErrorMessage`): its text, closed with OK. */
  const messageBox = async () => {
    const t0 = Date.now();
    while (!document.querySelector('[data-testid="message-box"]') && Date.now() - t0 < 3000) await sleep(100);
    const text = document.querySelector('[data-testid="message-box-text"]')?.textContent ?? null;
    click(document.querySelector('[data-testid="message-box-ok"]'));
    await sleep(200);
    return text;
  };

  // ---- the board
  const schematic = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json()).catch(() => null);
  if (!["R1", "C1", "V1", "D1", "D2"].every((r) => schematic?.symbols?.some((s) => s.id === r))) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs web/studio/e2e/sim-model-board.yaml's circuit"] });
    check.done = true;
    return;
  }
  [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
  await sleep(1500);

  await scenario("a resistor: inferred Ideal, its value edited in the grid, saved in the Value, the Code tab, OK in one undo step", async (t) => {
    const before = await fields("R1");
    t(before.value === "1k" && Object.keys(before.fields).length === 0, `no model of its own: ${JSON.stringify(before.fields)}`);
    t(await openDialog("R1"), "the dialog opens");
    t($("sim-model-builtin")?.checked === true && $("sim-model-library")?.checked === false, "Built-in SPICE model (a passive's Value is inferred)");
    t($("sim-model-device")?.value === "Resistor" && $("sim-model-type")?.value === "Ideal", `Resistor, Ideal: ${$("sim-model-device")?.value} / ${$("sim-model-type")?.value}`);
    t([...($("sim-model-type")?.options ?? [])].map((o) => o.value).join("|") === "Ideal|Potentiometer|Behavioral", "its types");
    const devices = [...($("sim-model-device")?.options ?? [])].map((o) => o.value);
    t(devices[0] === "" && devices.includes("N-channel MESFET") && devices.indexOf("N-channel MESFET") < devices.indexOf("N-channel MOSFET") && !devices.includes("Subcircuit"), `the devices in KiCad's order: ${devices.join(", ")}`);
    const r = row("r");
    t(r?.textContent.startsWith("Resistance (r)") && cell("r")?.value === "1k" && r.textContent.includes("Ω") && r.textContent.endsWith("Float"), `the r row: ${r?.textContent}`);
    const save = $("sim-model-save-in-value");
    t(save?.checked === true && save.parentElement.textContent.includes("Save parameter 'Resistance (r)' in Value field"), `the checkbox: ${save?.parentElement.textContent}`);
    click(save);
    await sleep(100);
    t(save.checked === false, "unticked");
    click(save);
    await sleep(100);
    t(save.checked === true, "ticked again");
    t(await typeParam("r", "2.2k"), "r typed");
    t((await codeTab()) === "R NC--0 NC--1 2.2k", `the Code tab: ${$("sim-model-code")?.value}`);
    click($("sim-model-pins-tab"));
    await sleep(200);
    const cells = [...(dlg()?.querySelectorAll('[data-testid="sim-model-pin-grid"] select') ?? [])].map((c) => c.value);
    t(cells.join(" | ") === '1 ("+") | 2 ("-")', `the pin grid: ${cells.join(" | ")}`);
    t(await simOk(), "OK closes the dialog");
    // `OnEditSpiceModel`: Symbol Properties takes the fields, its own OK applies them (KiCad's one undo step)
    t(JSON.stringify((await fields("R1")).fields) === "{}", "nothing written before Symbol Properties' OK");
    const valueBox = [...(propsDialog()?.querySelectorAll("input") ?? [])].find((i) => i.value === "2.2k");
    t(!!valueBox, "Symbol Properties shows the new Value");
    const shownBox = (name) => propsDialog()?.querySelector(`input[aria-label="Show ${name}"]`) ?? null;
    t(shownBox("Sim.Device")?.checked === true && shownBox("Sim.Pins")?.checked === true, "the inference's fields are new rows, shown (SetFieldValue's aIsVisible true)");
    t(await propsOk(), "Symbol Properties' OK");
    const after = await fields("R1");
    t(after.value === "2.2k" && after.fields["Sim.Device"] === "R" && after.fields["Sim.Pins"] === "1=+ 2=-" && !("Sim.Params" in after.fields), `written: ${after.value} ${JSON.stringify(after.fields)}`);
    const sch = await fetch("/api/schematic", { cache: "no-store" }).then((r) => r.json());
    const r1 = sch.symbols.find((x) => x.id === "R1");
    const vis = (n) => r1?.fields?.find((f) => f.name === n)?.visible;
    t(vis("Sim.Device") === true && vis("Sim.Pins") === true, `on the sheet, visible as KiCad's: ${JSON.stringify(r1?.fields?.map((f) => [f.name, f.visible]))}`);
    await E.run("common.Interactive.undo");
    await sleep(900);
    const undone = await fields("R1");
    t(undone.value === "1k" && Object.keys(undone.fields).length === 0, `one undo step takes it back: ${undone.value} ${JSON.stringify(undone.fields)}`);
    await E.run("common.Interactive.redo");
    await sleep(900);
    t((await fields("R1")).value === "2.2k", "and redo puts it again");
  });

  await scenario("a voltage source: DC made a Sine, its parameters typed (autofill), the AC group, the Code tab, OK", async (t) => {
    const before = await fields("V1");
    t(before.fields["Sim.Device"] === "V" && before.fields["Sim.Type"] === "DC", `the library symbol's model: ${JSON.stringify(before.fields)}`);
    t(await openDialog("V1"), "the dialog opens");
    t($("sim-model-device")?.value === "Voltage Source" && $("sim-model-type")?.value === "DC", "Voltage Source, DC");
    t(cell("dc")?.value === "5", `the DC value from the Value field: ${cell("dc")?.value}`);
    setValue($("sim-model-type"), "Sine");
    await sleep(300);
    t($("sim-model-type")?.value === "Sine", "Sine chosen");
    const save = $("sim-model-save-in-value");
    t(save?.disabled === true && save.checked === false && save.parentElement.textContent.includes("Save primary parameter in Value field"), "a sine has no primary value: the checkbox is off");
    const principal = [...dlg().querySelectorAll('[data-testid="sim-model-param-grid"] tbody tr')].slice(0, 6).map((r) => r.getAttribute("data-param"));
    t(principal.join(",") === "dc,ampl,f,td,theta,phase", `the principal rows: ${principal}`);
    t(!!dlg().querySelector('tr[data-category="AC"]') && !!row("ac"), "the AC group, expanded");
    t(!!dlg().querySelector('tr[data-category="S-Parameters"]') && !row("portnum"), "the S-Parameters group, collapsed");
    t(row("f")?.textContent.includes("1/tstop"), "f's default");
    for (const [n, v] of [["dc", "0.5"], ["ampl", "2"], ["f", "10k"], ["ac", "1"]]) await typeParam(n, v);
    t(cell("td")?.value === "0" && cell("theta")?.value === "0" && cell("phase")?.value === "0", "the parameters before ac filled with 0 (SIM_MODEL_SOURCE::doSetParamValue)");
    t((await codeTab()) === "V NC--0 NC--1 DC 0.5 SIN( 0.5 2 10k 0 0 0 ) AC 1", `the Code tab: ${$("sim-model-code")?.value}`);
    t(await ok(), "OK");
    const after = await fields("V1");
    // the inferred DC source's Value back as it was (KiCad 25ec2335's `RestoreInferredValue`: a sine's parameters are not saved in the Value)
    t(after.fields["Sim.Type"] === "SIN" && after.fields["Sim.Params"] === "dc=0.5 ampl=2 f=10k td=0 theta=0 phase=0 ac=1" && after.value === "5", `written: ${after.value} ${JSON.stringify(after.fields)}`);
  });

  await scenario("a diode: its parameters by category, a model and an instance parameter, the Code tab's .model card, OK", async (t) => {
    t(await openDialog("D1"), "the dialog opens");
    t($("sim-model-device")?.value === "Diode" && !$("sim-model-type"), "Diode, one type (no Device type choice)");
    const cats = [...dlg().querySelectorAll("tr[data-category]")].map((r) => r.getAttribute("data-category"));
    t(cats.join(",") === "Geometry,DC,Capacitance,Temperature,Noise,Limiting Values", `the categories: ${cats}`);
    t(!row("is"), "DC is collapsed");
    click(dlg().querySelector('tr[data-category="DC"] td'));
    click(dlg().querySelector('tr[data-category="Geometry"] td'));
    await sleep(200);
    t(row("is")?.textContent.startsWith("Saturation current (is)") && row("is").textContent.includes("1e-14"), `is: ${row("is")?.textContent}`);
    await typeParam("is", "1e-14");
    await typeParam("n", "1.8");
    await typeParam("m", "2");
    t((await codeTab()) === ".model D1 D\n+         is=10f\n+         n=1.8\n\n\nD NC--0 NC--1 D1 m=2", `the Code tab: ${JSON.stringify($("sim-model-code")?.value)}`);
    await paramsTab();
    t(await ok(), "OK");
    const after = await fields("D1");
    t(after.fields["Sim.Params"] === "m=2 is=1e-14 n=1.8" && after.fields["Sim.Device"] === "D" && after.fields["Sim.Pins"] === "1=K 2=A", `written: ${JSON.stringify(after.fields)}`);
  });

  await scenario("kicad-cli's SPICE netlist holds the lines the Code tabs showed", async (t) => {
    // the simulator netlist of an operating point: kicad-cli's SPICE netlist with the analysis (`SPICE_CIRCUIT_MODEL::GetNetlist`)
    const c = await post("/api/sim/circuit", { command: ".op" });
    const nl = c.netlist ?? "";
    t(c.ok === true, `the circuit: ${JSON.stringify(c.messages ?? c.message)}`);
    const lines = nl.split("\n").filter((l) => /^(R1|V1|D1|\.model __D1|\+) /.test(l) || l.startsWith(".model")).join(" / ");
    t(nl.includes("\nR1 /IN /MID 2.2k\n"), `R1: ${lines}`);
    t(nl.includes("\nV1 /IN GND DC 0.5 SIN( 0.5 2 10k 0 0 0 ) AC 1  \n"), `V1: ${lines}`);
    t(nl.includes(".model __D1 D\n+           is=10f\n+           n=1.8\n") && nl.includes("\nD1 /MID GND __D1 m=2\n"), `D1's card and item: ${lines}`);
  });

  await scenario("a library model instead (the library half, section 51): the File box, the models, the grid keeping its rows' pins, OK", async (t) => {
    t(await openDialog("D1"), "the dialog opens (on its built-in diode)");
    t($("sim-model-builtin")?.checked === true, "Built-in SPICE model");
    const file = $("sim-model-file");
    setValue(file, "models/diode.lib");
    file.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const t0 = Date.now();
    while (!$("sim-model-list")?.options.length && Date.now() - t0 < 8000) await sleep(200);
    await sleep(300);
    const list = $("sim-model-list");
    t($("sim-model-library")?.checked === true && [...(list?.options ?? [])].map((o) => o.value).join(",") === "D1N4148,DFAST" && list.value === "D1N4148", `the library's models, the first chosen: ${list?.value}`);
    // the library's .model parameters in the grid (ReadModel), through the base model
    click(dlg().querySelector('tr[data-category="DC"] td'));
    click(dlg().querySelector('tr[data-category="Limiting Values"] td'));
    await sleep(200);
    t(cell("is")?.value === "2.52n" && cell("n")?.value === "1.752" && cell("rs")?.value === "0.568", `D1N4148's values: is=${cell("is")?.value} n=${cell("n")?.value} rs=${cell("rs")?.value}`);
    t((await codeTab()).startsWith(".model D1N4148 D\n"), `the Code tab's card under the list's name: ${JSON.stringify($("sim-model-code")?.value.slice(0, 40))}`);
    await paramsTab();
    setValue(list, "DFAST");
    await sleep(300);
    click($("sim-model-pins-tab"));
    await sleep(200);
    const cells = [...(dlg()?.querySelectorAll('[data-testid="sim-model-pin-grid"] select') ?? [])].map((c) => c.value);
    t(cells.join(" | ") === '2 ("K") | 1 ("A")', `the grid keeps the built-in model's rows: ${cells.join(" | ")}`);
    t(await ok(), "OK");
    const f = (await fields("D1")).fields;
    t(f["Sim.Library"] === "models/diode.lib" && f["Sim.Name"] === "DFAST" && f["Sim.Device"] === "D" && f["Sim.Pins"] === "1=K 2=A" && !("Sim.Params" in f), `written: ${JSON.stringify(f)}`);
    const nl = (await post("/api/sim/circuit", { command: ".op" })).netlist ?? "";
    t(nl.includes("\nD1 /MID GND DFAST\n") && /\.include ".*diode\.lib"/.test(nl), `kicad-cli includes the library and names its model: ${nl.split("\n").filter((l) => l.startsWith("D1") || l.startsWith(".include")).join(" / ")}`);
  });

  await scenario("an IBIS model: the file's component, its pins and a pin's models, the waveform, OK writes the IBIS fields", async (t) => {
    t(await openDialog("D2"), "the dialog opens (on the built-in diode)");
    const file = $("sim-model-file");
    setValue(file, "models/io.ibs");
    file.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const t0 = Date.now();
    while (!$("sim-model-list")?.options.length && Date.now() - t0 < 8000) await sleep(200);
    await sleep(300);
    t($("sim-model-library")?.checked === true, "SPICE model from file");
    t([...($("sim-model-list")?.options ?? [])].map((o) => o.value).join(",") === "CHIP" && dlg().textContent.includes("Component:"), "its component, under Component:");
    const pins = [...($("sim-model-ibis-pin")?.options ?? [])].map((o) => o.textContent).filter(Boolean);
    t(pins.join(",") === "1 - OUT,2 - GND,3 - IOP,4 - ION", `its pins: ${pins}`);
    const pinSel = $("sim-model-ibis-pin");
    setValue(pinSel, "2");
    await sleep(300);
    t([...($("sim-model-ibis-pin-model")?.options ?? [])].map((o) => o.value).filter(Boolean).join(",") === "io33,io18" && $("sim-model-ibis-pin-model").value === "", "pin 3's models (its model selector), none chosen");
    t(!!$("sim-model-ibis-diff"), "a differential pin: the Differential box");
    setValue(pinSel, "0");
    await sleep(300);
    t($("sim-model-ibis-pin-model")?.value === "drv", "pin 1's one model is chosen at once");
    t(!$("sim-model-ibis-diff"), "not differential");
    t([...($("sim-model-waveform")?.options ?? [])].map((o) => o.value).join(",") === "Device,DC driver,Rectangular wave driver,PRBS driver", "the waveforms");
    setValue($("sim-model-waveform"), "Rectangular wave driver");
    await sleep(300);
    t(!!row("ton") && !!row("vcc"), "the driver's parameters (vcc, ton..)");
    const box = await messageBox();
    t(box?.includes("Unknown simulation model pin 'K'"), `the diode's Sim.Pins do not read for it: KiCad's error box (${box})`);
    await typeParam("ton", "5n");
    t(await ok(), "OK");
    const after = await fields("D2");
    const f = after.fields;
    t(f["Sim.Library"] === "models/io.ibs" && f["Sim.Name"] === "CHIP" && f["Sim.Ibis.Pin"] === "1" && f["Sim.Ibis.Model"] === "drv", `the library and IBIS fields: ${JSON.stringify(f)}`);
    t(f["Sim.Device"] === "IBIS" && f["Sim.Type"] === "RECTDRIVER" && f["Sim.Params"] === "ton=5n", `the model's: ${JSON.stringify(f)}`);
    t(f["Sim.Pins"] === "1=IN/OUT 2=GND", `the pins kept their rows' model pin indices: ${f["Sim.Pins"]}`);
  });

  await scenario("nothing logged an error", async (t) => {
    const errs = E.errors(errorsFrom);
    t(errs.length === 0, `errors: ${JSON.stringify(errs).slice(0, 600)}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
