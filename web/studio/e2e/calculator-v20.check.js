// The Calculator Tools (KiCad's pcb_calculator; PARITY-common.md section 32, "calculator v20"), on a placed mcu30:
//   open        Tools > Calculator Tools in the PCB Editor opens the "Calculator Tools" window; File > Close closes it; the same entry in the
//               Schematic Editor's Tools menu opens it again
//   tree        the tree lists KiCad's five groups and fifteen calculators in KiCad's order; a click on a group shows its first calculator;
//               the page chosen is kept (`last_page`) when the window is closed and opened again
//   <panels>    each calculator exercised once with values KiCad computes
//   end         settings back to the defaults, no console errors
// It answers through `window.__calculatorV20Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__calculatorV20Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // ------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const win = () => tid("calculator-window");
  const click = async (el, wait = 250) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const setValue = async (el, v) => {
    if (!el) throw new Error("no control to set");
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(150);
  };
  const setCheck = async (el, on) => { if (!el) throw new Error("no check box"); if (el.checked !== on) el.click(); await sleep(150); };
  const val = (id) => { const el = tid(id); return el ? ("value" in el && el.tagName !== "DIV" && el.tagName !== "SPAN" && el.tagName !== "TD" ? el.value : el.textContent) : null; };
  const num = (id) => parseFloat(String(val(id) ?? "").replace(",", "."));
  const near = (a, b, rel = 1e-3) => Number.isFinite(a) && Math.abs(a - b) <= rel * Math.max(Math.abs(a), Math.abs(b), 1e-12);
  const menuItem = (menuLabel, itemLabel, root = document) => {
    const menu = [...root.querySelectorAll(".menubar-item")].find((m) => m.firstChild?.textContent?.trim() === menuLabel);
    return { menu, item: () => [...(menu?.querySelectorAll(".menu-node-item") ?? [])].find((i) => i.textContent.trim().startsWith(itemLabel)) };
  };
  const runMenu = async (menuLabel, itemLabel, root = document) => {
    const { menu, item } = menuItem(menuLabel, itemLabel, root);
    if (!menu) throw new Error(`no ${menuLabel} menu`);
    menu.click();
    await sleep(200);
    const it = item();
    if (!it) { menu.click(); throw new Error(`no ${menuLabel} > ${itemLabel}`); }
    it.click();
    await sleep(400);
  };
  const editorMenus = () => document.querySelector(".menubar:not(.calc-menubar):not(.sim-menubar)");
  const showPage = async (id) => { await click(win().querySelector(`[data-calc-page="${id}"]`), 300); return tid(`calc-panel-${id}`); };
  const page = async (id) => { if (!win()) { await E.run("common.Control.showCalculatorTools"); await sleep(300); } await showPage(id); };
  const shown = (id) => { const slot = win()?.querySelector(`[data-calc-slot="${id}"]`); return !!slot && !slot.hidden; };

  await until(() => E.state().tab === "pcb", 30000);
  localStorage.removeItem("eda-studio.pcb_calculator");
  localStorage.removeItem("eda-studio.pcb_calculator.regulators");

  await scenario("open: Tools > Calculator Tools in the PCB Editor and in the Schematic Editor; File > Close", async (t) => {
    t(!win(), "no calculator window at first");
    await runMenu("Tools", "Calculator Tools", editorMenus());
    t(!!win(), "the PCB Editor's Tools > Calculator Tools opens the window");
    t(win()?.querySelector(".calc-title")?.textContent === "Calculator Tools", "its title is Calculator Tools");
    const menus = [...win().querySelectorAll(".calc-menubar .menubar-item")].map((m) => m.firstChild?.textContent?.trim());
    t(menus.join(",") === "File,Preferences,Help", `its menus: ${menus.join(", ")}`);
    await runMenu("File", "Close", win());
    t(!win(), "File > Close closes it");
    await clickTab("Schematic");
    await until(() => E.state().tab === "schematic", 10000);
    await runMenu("Tools", "Calculator Tools", editorMenus());
    t(!!win(), "the Schematic Editor's Tools > Calculator Tools opens it");
    await clickTab("PCB");
    await until(() => E.state().tab === "pcb", 10000);
    t(!!win(), "it stays open on the PCB tab");
  });

  await scenario("tree: KiCad's groups and calculators; a group shows its first calculator; the page is kept", async (t) => {
    const rows = [...win().querySelectorAll(".calc-tree-row")].map((r) => r.textContent.replace("▾", "").trim());
    const want = ["General system design", "Regulators", "Resistor Calculator", "Footprint design", "Through-hole Size", "Power, current and isolation", "Electrical Spacing", "Via Size", "Track Width", "Fusing Current", "Cable Size", "High Speed", "Wavelength", "RF Attenuators", "Transmission Lines", "Memo", "E-Series", "Color Code", "Board Classes", "Galvanic Corrosion"];
    t(JSON.stringify(rows) === JSON.stringify(want), `the rows: ${rows.join(" | ")}`);
    t(shown("regulators"), "Regulators is shown first (last_page 1)");
    await click(win().querySelector('[data-calc-page="group-highspeed"]'), 300);
    t(shown("wavelength"), "a click on High Speed shows Wavelength");
    await showPage("board_class");
    await click(tid("calculator-close"), 300);
    await E.run("common.Control.showCalculatorTools");
    await sleep(300);
    t(shown("board_class"), "closed and opened again, Board Classes is still the page");
    const stored = JSON.parse(localStorage.getItem("eda-studio.pcb_calculator") || "{}");
    t(stored.last_page === 18, `last_page stored as KiCad's index 18 (${stored.last_page})`);
  });

  // ------------------------------------------------------------------------------------------------------------------------------------------------ panels
  await scenario("Regulators: LM317 values solved for Vout, the Vref message, a regulator added to the list", async (t) => {
    await page("regulators");
    await click(tid("regulator-reset"), 200);
    await click(tid("regulator-calculate"), 300);
    t(tid("regulator-vout-typ").value === "5.036" && tid("regulator-vout-min").value === "4.764" && tid("regulator-vout-max").value === "5.352", "Vout");
    t(tid("regulator-tol-min").value === "-5.39" && tid("regulator-tol-max").value === "5.9", "tolerance");
    t(tid("regulator-comment").value === "5.04V [4.76V ... 5.35V]", "power comment");
    await setValue(tid("regulator-vref-min"), "0"); await click(tid("regulator-calculate"), 200);
    t(tid("regulator-message").textContent === "Vref set to 0 !", "message"); await click(tid("regulator-reset"), 200);
    await click(tid("regulator-add"), 200);
    await setValue(tid("regulator-form-name"), "LM1117"); await setValue(tid("regulator-form-vref-min"), "1.238");
    await setValue(tid("regulator-form-vref-typ"), "1.25"); await setValue(tid("regulator-form-vref-max"), "1.262");
    await click(tid("regulator-form-ok"), 200);
    t(!tid("regulator-form") && [...tid("regulator-selector").options].some((o) => o.textContent === "LM1117"), "added");
    t(tid("regulator-formula").textContent === "Vout = Vref * (R1 + R2) / R2", "standard type after selecting it");
  });

  await scenario("Resistor Calculator: 4.7 k from E6 as KiCad finds it", async (t) => {
    await page("r_calculator");
    await setValue(tid("r-calc-required"), "4.7"); tid("r-calc-series-E6").click(); await click(tid("r-calc-calculate"), 400);
    t(tid("r-calc-sol-2r").value === "6K8 | 15K" && tid("r-calc-err-2r").value === "-0.45", `2R ${tid("r-calc-sol-2r").value} ${tid("r-calc-err-2r").value}`);
    t(tid("r-calc-sol-3r").value === "1K + 1K5 + 2K2" && tid("r-calc-err-3r").value === "Exact", `3R ${tid("r-calc-sol-3r").value}`);
    t(tid("r-calc-sol-4r").value === "Not worth using" && tid("r-calc-err-4r").value === "", "4R");
  });

  await scenario("E-Series: E3 and E96 tables; the help", async (t) => {
    await page("eseries");
    t(tid("eseries-112-col-1").querySelectorAll("[data-eseries-value]").length === 3, "E3: 100 220 470");
    t(tid("eseries-2496-col-14").lastElementChild.textContent === "976", "last E96");
    t(tid("eseries-help").textContent.includes("The value 0 is a special case"), "help");
  });

  await scenario("Color Code: a 4th value band for 2% and below; the multiplier column", async (t) => {
    await page("color_code");
    tid("color-code-tolerance-0").click(); await sleep(150); t(!tid("color-code-band4"), "3 value bands");
    tid("color-code-tolerance-1").click(); await sleep(150); t(!!tid("color-code-band4"), "4th band for <= 2%");
    t(tid("color-code-multiplier").querySelector('[data-band="Orange"]').textContent === "x 1k", "multiplier"); tid("color-code-tolerance-0").click();
    await sleep(150);
    t(JSON.parse(localStorage.getItem("eda-studio.pcb_calculator")).color_code_tolerance === 0, "color_code_tolerance stored");
  });

  await scenario("Fusing Current: current, time, width with KiCad's values; its comment and Error", async (t) => {
    await page("fusing_current");
    await click(tid("fusing-radio-current")); await click(tid("fusing-calculate"));
    t(tid("fusing-current").value === "11.219222", `current ${tid("fusing-current").value}`);
    await setValue(tid("fusing-current"), "10"); await click(tid("fusing-radio-time")); await click(tid("fusing-calculate"));
    t(tid("fusing-time").value === "0.012587", `time ${tid("fusing-time").value}`);
    t(tid("fusing-comment").textContent === "", "no comment");
    await setValue(tid("fusing-time"), "5"); await click(tid("fusing-radio-width")); await click(tid("fusing-calculate"));
    t(tid("fusing-comment").textContent === "Current calculation is underestimated due to long fusing time.", "long-time comment");
    await setValue(tid("fusing-ambient"), "2000"); await click(tid("fusing-calculate"));
    t(tid("fusing-width").value === "Error", "Error on ambient > melting");
  });

  await scenario("Cable Size: a 1 mm wire, AWG10, a typed diameter, the current density saved", async (t) => {
    await page("cable_size");
    t(tid("cable-diameter").value === "1" && tid("cable-area").value === "0.785398", "1 mm wire");
    t(tid("cable-resistivity").value === "1.72e-08" && tid("cable-conductorTemp").value === "20" && tid("cable-thermCoef").value === "3.93e-3", "copper defaults");
    t(tid("cable-linResistance").value === "0.0218997" && tid("cable-ampacity").value === "2.35619" && tid("cable-length").value === "100" && tid("cable-vDrop").value === "21.8997", "derived");
    await setValue(tid("cable-size"), String(13));
    t(tid("cable-diameter").value === "2.58826" && tid("cable-area").value === "5.26145" && tid("cable-ampacity").value === "15.7844", "AWG10");
    await setValue(tid("cable-diameter"), "2");
    t(tid("cable-area").value === "3.14159" && tid("cable-size").value === "-1", "typed diameter");
    await setValue(tid("cable-current-density"), "6");
    t(tid("cable-ampacity").value === "18.8496", "6 A/mm²");
    t(JSON.parse(localStorage.getItem("eda-studio.pcb_calculator")).cable_size.currentDensityChoice === 6, "saved");
  });

  await scenario("Through-hole Size: the defaults, a square lead (KiCad QA Ipc2222SquareLead), an invalid input", async (t) => {
    await page("pth_size");
    t(tid("pth-hole").value === "1.1" && tid("pth-pad").value === "1.9", "1.1 / 1.9 mm");
    t(tid("pth-report").value.includes("Recommended hole size:         1.1 mm"), "report");
    await setValue(tid("pth-shape"), "1");
    t(tid("pth-sizeX-label").textContent === "Side length:", "label");
    await setValue(tid("pth-sizeXMin"), "0.60"); await setValue(tid("pth-sizeXMax"), "0.65");
    await setValue(tid("pth-rounding-choice"), "1");
    t(tid("pth-hole").value === "1.35", `square hole ${tid("pth-hole").value}`);
    t(tid("pth-pad").value === "2.15", "pad");
    await setValue(tid("pth-sizeXMin"), "0.9");
    t(tid("pth-report").value === "" && tid("pth-hole").value === "1.35", "invalid: report emptied, result kept");
    t(tid("pth-summary").textContent.includes("Maximum lead diameter + 0.15mm"), "summary");
  });

  await scenario("Electrical Spacing: IPC 2221's table at 600 V and in mil; IEC 60664 with KiCad's values, PD4, an unreadable altitude", async (t) => {
    await page("electrical_spacing");
    await click(tid("calc-es-tab-0"));
    t(tid("calc-es-cell-0-0").textContent === "0.05" && tid("calc-es-cell-8-2").textContent === "12.5", "IPC grid in mm");
    await setValue(tid("calc-es-voltage"), "600"); await click(tid("calc-es-update"));
    t(tid("calc-es-cell-9-0").textContent === "0.5" && tid("calc-es-cell-9-2").textContent === "15" && tid("calc-es-cell-9-7").textContent === "3.61", "> 500 V row at 600 V");
    await setValue(tid("calc-es-voltage"), "120"); await click(tid("calc-es-update"));
    t(tid("calc-es-voltage").value === "500", "below 500 V becomes 500");
    await setValue(tid("calc-es-units"), 3);
    t(tid("calc-es-cell-0-0").textContent === "1.9685", "B1 0..15 V in mil");
    await setValue(tid("calc-es-units"), 0);
    await click(tid("calc-es-tab-1"));
    t(tid("calc-iec-impulse").value === "1.5", "impulse 1.5 kV");
    t(tid("calc-iec-clearance").value === "0.15" && tid("calc-iec-creepage").value === "0.56" && tid("calc-iec-groove").value === "0.05", "defaults");
    await setValue(tid("calc-iec-ovc"), 3); t(tid("calc-iec-impulse").value === "6", "OVC IV -> 6 kV");
    await setValue(tid("calc-iec-insulation"), 2);
    t(tid("calc-iec-clearance").value === "1" && tid("calc-iec-creepage").value === "1.12" && tid("calc-iec-groove").value === "0.333333", "reinforced");
    await setValue(tid("calc-iec-insulation"), 0); await setValue(tid("calc-iec-pollution"), 3);
    t(tid("calc-iec-clearance").value === "1.6" && tid("calc-iec-creepage").value === "N/A" && tid("calc-iec-groove").value === "N/A", "PD4");
    await setValue(tid("calc-iec-pollution"), 0); await setValue(tid("calc-iec-altitude"), "x");
    t(tid("calc-iec-clearance").value === "Error", "unreadable altitude"); await setValue(tid("calc-iec-altitude"), "2000"); await setValue(tid("calc-iec-ovc"), 0);
    t(!!tid("calc-iec-help").querySelector("li"), "help rendered");
  });

  await scenario("Board Classes: the table in mm and in mil", async (t) => {
    await page("board_class");
    t(tid("calc-bc-cell-0-0").textContent === "0.8" && tid("calc-bc-cell-2-0").textContent === "--" && tid("calc-bc-cell-4-0").textContent === "1.57", "classes in mm");
    await setValue(tid("calc-bc-units"), 3); t(tid("calc-bc-cell-0-0").textContent === "31.4961", "class 1 lines in mil"); await setValue(tid("calc-bc-units"), 0);
  });

  await scenario("Galvanic Corrosion: KiCad's potentials and colours, the threshold, symbols or names", async (t) => {
    await page("galvanic_corrosion");
    const cell = (i, j) => tid(`calc-gc-cell-${i}-${j}`);
    t(cell(0, 38).textContent === "-1980" && cell(0, 38).style.background === "rgb(59, 26, 3)" && cell(0, 38).style.color === "rgb(255, 255, 255)", "Rh vs Mg");
    t(cell(15, 15).textContent === "0" && cell(15, 15).style.background === "rgb(193, 231, 255)", "Cu vs Cu");
    t(cell(15, 3).textContent === "440" && cell(15, 3).style.background === "rgb(182, 182, 202)", "Cu vs Au over threshold 0");
    await setValue(tid("calc-gc-threshold"), "500"); t(cell(15, 3).style.background === "rgb(122, 166, 194)", "within 500 mV");
    t(tid("calc-gc-row-15").textContent === "Cu", "symbols"); await click(tid("calc-gc-names")); t(tid("calc-gc-row-15").textContent === "Copper", "names");
    await click(tid("calc-gc-symbols")); await setValue(tid("calc-gc-threshold"), "0");
  });

  await scenario("RF Attenuators: Pi, Tee, Bridged tee with KiCad's values; the minimum attenuation error; the formula pane", async (t) => {
    await page("rf_attenuators");
    await click(tid("att-type-0"));
    await setValue(tid("att-attenuation"), "6"); await setValue(tid("att-zin"), "50"); await setValue(tid("att-zout"), "50");
    await click(tid("att-calculate"));
    t(tid("att-r1").value === "150.476" && tid("att-r2").value === "37.3519" && tid("att-r3").value === "150.476", `pi ${tid("att-r1").value} ${tid("att-r2").value}`);
    await click(tid("att-type-1")); await click(tid("att-calculate"));
    t(tid("att-r1").value === "16.6139" && tid("att-r2").value === "66.931", `tee ${tid("att-r1").value} ${tid("att-r2").value}`);
    await click(tid("att-type-2")); t(tid("att-zin").disabled, "bridged tee: Zin disabled");
    await click(tid("att-calculate"));
    t(tid("att-r1").value === "49.7631" && tid("att-r2").value === "50.238" && tid("att-r3").value === "", "bridged tee R1/R2");
    await click(tid("att-type-0")); await setValue(tid("att-zin"), "300"); await setValue(tid("att-attenuation"), "3"); await click(tid("att-calculate"));
    t(tid("att-r1").value === "--" && tid("att-messages").textContent.includes("Attenuation more than 13.415226 dB"), tid("att-messages").textContent);
    t(tid("att-formula").textContent.includes("Pi Attenuator"), "formula pane");
  });

  await scenario("Wavelength: 1 GHz in FR4; from a vacuum wavelength; a speed unit; er from its list", async (t) => {
    await page("wavelength");
    t(tid("wl-frequency").value === "1" && tid("wl-period").value === "1" && tid("wl-vacuum").value === "29.9792" && tid("wl-medium").value === "14.1324" && tid("wl-speed").value === "1.41324e+08", "1 GHz in FR4");
    await setValue(tid("wl-vacuum"), "10");
    t(tid("wl-frequency").value === "2.99792", `from 10 cm: ${tid("wl-frequency").value}`);
    await setValue(tid("wl-speed-unit"), 2); t(tid("wl-speed").value === "5.08765e+08", `km/h ${tid("wl-speed").value}`);
    await click(tid("wl-er-list")); t(!!tid("wavelength-er-list"), "er list opens");
    await click(document.querySelector('[data-testid="wavelength-er-list"] [data-choice-index="8"]')); await click(tid("wavelength-er-list-ok"));
    t(tid("wl-er").value === "2.2", `er from list ${tid("wl-er").value}`);
  });

  await scenario("Via Size: KiCad's results at the defaults; the pad >= clearance warning", async (t) => {
    await page("via_size");
    await click(tid("via-reset"));
    const vr = (k) => tid(`via-result-${k}`).textContent;
    t(vr("viaResistance") === "0.000575362" && vr("estimatedAmpacity") === "2.9993" && vr("thermalResistance") === "83.2937" && vr("capacitance") === "0.599508" && vr("timeDegradation") === "32.9729" && vr("inductance") === "1.20723" && vr("reactance") === "3.79262", `via ${vr("viaResistance")} ${vr("capacitance")}`);
    t(!tid("via-warning"), "no warning at the defaults");
    await setValue(tid("via-pad_diameter"), "1.2"); t(!!tid("via-warning"), "pad >= clearance warns");
    await click(tid("via-reset"));
  });

  await scenario("Track Width: widths for 1 A, the controlling value in bold, from an external width", async (t) => {
    await page("track_width");
    await click(tid("tw-reset"));
    t(tid("tw-ext-width").value === "0.277287" && tid("tw-int-width").value === "0.248423", `widths ${tid("tw-ext-width").value} ${tid("tw-int-width").value}`);
    t(tid("tw-ext-area").textContent === "0.00970504" && tid("tw-ext-area-unit").textContent === "mm²" && tid("tw-ext-resistance").textContent === "0.0354455" && tid("tw-int-resistance").textContent === "0.0395639", "ext area / resistances");
    t(getComputedStyle(tid("tw-current")).fontWeight === "700", "current is the controlling value");
    await setValue(tid("tw-ext-width"), "0.5");
    t(tid("tw-current").value === "1.40354" && tid("tw-int-width").value === "0.446156", `from ext 0.5 mm: ${tid("tw-current").value} ${tid("tw-int-width").value}`);
    t(getComputedStyle(tid("tw-ext-width")).fontWeight === "700", "ext width now bold");
    t(tid("tw-resistivity").value === "1.72e-08", "copper resistivity");
    await click(tid("tw-reset"));
  });

  await scenario("Transmission Lines: KiCad's results for microstrip, coax, coupled microstrip and WR-90; the εr list", async (t) => {
    await page("transline");
    const v = (id) => tid(id)?.value;
    const txt = (id) => tid(id)?.textContent ?? "";
    await click(tid("tl-type-0")); await click(tid("tl-reset"));
    await click(tid("tl-synthesize"));
    t(v("tl-phys0") === "0.355542", `defaults synthesized: W ${v("tl-phys0")} mm (KiCad 0.355542)`);
    await click(tid("tl-reset"));
    await click(tid("tl-analyze"));
    t(v("tl-elec0") === "66.9548", `microstrip defaults Z0 ${v("tl-elec0")} (KiCad 66.9548)`);
    t(v("tl-elec2") === "1.79748", `Ang_l ${v("tl-elec2")} (KiCad 1.79748)`);
    t(txt("tl-result-label-0") === "Effective εr:" && txt("tl-result-0").trim() === "2.94219", `εeff ${txt("tl-result-0")}`);
    t(txt("tl-result-1") === "57.3673 ps/cm", `delay ${txt("tl-result-1")}`);
    await click(tid("tl-synthesize"));
    t(v("tl-phys0") === "0.2", `synthesized back from its own Z0: W ${v("tl-phys0")} mm`);
    await click(tid("tl-type-7")); await click(tid("tl-reset")); await click(tid("tl-analyze"));
    t(v("tl-elec0") === "58.7748", `coax Z0 ${v("tl-elec0")} (KiCad 58.7748)`);
    t(txt("tl-result-3") === "none" && txt("tl-result-label-3") === "TE-modes:", `coax TE ${txt("tl-result-3")}`);
    await setValue(tid("tl-phys0"), "3"); await setValue(tid("tl-phys1"), "2"); await click(tid("tl-analyze"));
    t(tid("tl-phys0").dataset.status === "warning" && tid("tl-elec0").dataset.status === "error", "Dout < Din: diameters warned, Z0 red");
    await click(tid("tl-reset"));
    await click(tid("tl-type-1")); await click(tid("tl-reset"));
    await setValue(tid("tl-elec0"), "60"); await setValue(tid("tl-elec1"), "40"); await setValue(tid("tl-elec2"), "1.5");
    await click(tid("tl-synthesize"));
    t(v("tl-phys0") === "0.365686" && v("tl-phys1") === "0.117294", `coupled 60/40: W ${v("tl-phys0")} S ${v("tl-phys1")} (KiCad 0.365686 / 0.117294)`);
    t(txt("tl-result-9") === "80 Ω", `Zd ${txt("tl-result-9")}`);
    await click(tid("tl-reset"));
    await click(tid("tl-type-6")); await click(tid("tl-reset"));
    await setValue(tid("tl-subs0"), "1"); await setValue(tid("tl-subs1"), "0");
    await setValue(tid("tl-phys0"), "22.86"); await setValue(tid("tl-phys1"), "10.16"); await setValue(tid("tl-freq0"), "10");
    await click(tid("tl-analyze"));
    t(v("tl-elec0") === "498.974", `WR-90 Z0 ${v("tl-elec0")} (KiCad 498.974)`);
    t(txt("tl-result-4") === "H(1,0) ", `WR-90 TE ${JSON.stringify(txt("tl-result-4"))}`);
    await click(tid("tl-reset"));
    await click(tid("tl-subs0-list")); t(!!tid("tl-choice-dialog"), "εr list opens");
    await click([...tid("tl-choice-dialog").querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel"));
    await click(tid("tl-type-0"));
    const st = JSON.parse(localStorage.getItem("eda-studio.pcb_calculator") || "{}");
    t(st.translines?.type === 0 && st.trans_line?.MicroStrip?.values && Object.keys(st.trans_line.MicroStrip.values).length > 0, "translines.type and trans_line.MicroStrip.values stored");
  });


  await scenario("pad dialog: the hole size button opens Calculator Tools on Through-hole Size, raised over the dialog", async (t) => {
    if (win()) await click(tid("calculator-close"), 300);
    const s0 = await fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
    const th = (s0.parts ?? []).filter((p) => p.placed).flatMap((p) => (p.pads ?? []).filter((q) => q.th).map((q) => q.id))[0];
    if (!t(!!th, `a through-hole pad on the board (${th})`)) return;
    await E.run("common.InteractiveSelection.clear"); await E.run("common.InteractiveSelection.selectItems", [th]); await sleep(250);
    await E.run("pcbnew.InteractiveEdit.properties"); await sleep(500);
    if (!t(!!tid("board-pad-properties"), "Pad Properties is open")) return;
    await click(tid("pad-hole-calculator"), 400);
    t(!!win() && shown("pth_size"), "the window opens on Through-hole Size");
    t(win()?.classList.contains("raised") && getComputedStyle(win()).zIndex === "2100", `raised over the dialog (z ${win() && getComputedStyle(win()).zIndex})`);
    const cancel = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel" && b.closest(".dialog"));
    if (cancel) await click(cancel, 300);
  });

  await scenario("end: settings back to the defaults, window closed", async (t) => {
    window.confirm = () => true; // the regulator list was changed and has no file: OnClosePcbCalc asks
    localStorage.removeItem("eda-studio.pcb_calculator");
    localStorage.removeItem("eda-studio.pcb_calculator.regulators");
    if (win()) await click(tid("calculator-close"), 300);
    t(!win(), "closed");
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return results;
})();
