// Colour themes of the schematic, the Symbol Editor and the Footprint Editor (UI-LEFT-2 SS9, FF7, CC2; PARITY-sch.md section 71): Preferences > Schematic
// Editor > Colors, Symbol Editor > Colors and Footprint Editor > Colors, and KiCad theme files in and out. It drives the dialogs' own controls and reads the
// canvases' pixels and the store (`localStorage` "eda-studio.color-themes.v1"):
//   page        Schematic Editor > Colors: KiCad's theme list, 46 swatches in LayerName() order ("Anchors", "Axes (symbol editor only)", ...), Override individual
//               item colors showing Sheet borders and Sheet backgrounds, Import Theme... / Export Theme..., the dialog's Reset Colors to Defaults
//   classic     KiCad Classic picked: the schematic is drawn on Classic's white and its wires in Classic's green while the page is up; OK keeps it (eeschema's
//               theme, not the board's)
//   wire        New Theme... with the Wires swatch red and the symbols' outlines, fills and pins magenta: the sheet's wires are drawn red
//   symbol      the Symbol Editor draws in the schematic editor's theme (magenta); Symbol Editor > Colors' "Use theme:" KiCad Default takes it
//               away, "Use schematic editor color theme" brings them back
//   footprint   Footprint Editor > Colors: KiCad Classic for the Footprint Editor alone -- its pads in Classic's F.Cu, the board's stay KiCad Default's
//   export      Export Theme... writes the shown theme as KiCad does (meta, sorted keys, "rgb(255, 0, 0)" for the red wires)
//   import      Import Theme... reads a KiCad theme file: it is listed, shown and drawn; OK makes it the schematic editor's
//   cancel      a theme picked and the dialog cancelled: the schematic back in the editor's theme
//   end         every editor back on its default theme, no console errors
// It answers through `window.__schColorThemeCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed
(async () => {
  const results = [];
  const run = (window.__schColorThemeCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await sleep(250); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const until = async (cond, ms = 6000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(150);
  };
  const errors0 = (E.errors?.() ?? []).length;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const prefs = () => tid("preferences-dialog");
  const dialogs = () => [...document.querySelectorAll(".dialog")];
  const dialogTitled = (title) => dialogs().find((d) => d.querySelector(".dialog-header span")?.textContent.trim() === title);
  const footerButton = (dlg, text) => dlg && [...dlg.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === text);
  const click = async (el, wait = 250) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const dbl = async (el, wait = 300) => { if (!el) throw new Error("no swatch"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const stored = () => JSON.parse(localStorage.getItem("eda-studio.color-themes.v1") ?? "null");
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(700); };
  /** The canvas on screen (each editor's tab keeps its own). */
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].find((c) => c.offsetParent !== null && c.width > 0);
  const pixels = () => { const c = canvas(); return c.getContext("2d").getImageData(0, 0, c.width, c.height).data; };
  const count = (pred) => { const d = pixels(); let n = 0; for (let i = 0; i < d.length; i += 4) if (pred(d[i], d[i + 1], d[i + 2])) n++; return n; };
  const near = (r, g, b, tol = 6) => (R, G, B) => Math.abs(R - r) <= tol && Math.abs(G - g) <= tol && Math.abs(B - b) <= tol;
  /** The most common colour near the canvas's top-left corner: the background, off the sheet. */
  const corner = () => {
    const c = canvas();
    const ctx = c.getContext("2d");
    const seen = new Map();
    for (let x = 3; x < 60; x += 7) for (let y = 3; y < 60; y += 7) { const d = ctx.getImageData(x, y, 1, 1).data; const k = `${d[0]},${d[1]},${d[2]}`; seen.set(k, (seen.get(k) ?? 0) + 1); }
    return [...seen.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  };
  const same = (a, b, tol = 3) => a.every((v, i) => Math.abs(v - b[i]) <= tol);
  const openPage = async (id) => {
    if (!prefs()) { await E.run("common.SuiteControl.openPreferences"); await until(() => prefs(), 4000); }
    await click(tid(`prefs-page-${id}`), 350);
  };
  const prefsOk = async () => { await click(footerButton(prefs(), "OK"), 500); };
  const prefsCancel = async () => { await click(footerButton(prefs(), "Cancel"), 500); };
  const redraw = async () => { await E.run("common.Control.zoomRedraw").catch(() => null); await sleep(400); };
  const pickColor = async (hex) => {
    const dlg = dialogTitled("Color Picker");
    if (!dlg) throw new Error("no Color Picker");
    await setValue(dlg.querySelector('input[type="color"]'), hex);
    await click(footerButton(dlg, "OK"), 300);
  };
  const swatch = (key) => document.querySelector(`[data-testid="sch-colors-page"] [data-color-key="${key}"] [role=button]`);
  const DEFAULT_SCH_BG = [245, 244, 239]; // s_defaultTheme LAYER_SCHEMATIC_BACKGROUND
  const CLASSIC_SCH_BG = [255, 255, 255]; // COLOR4D( WHITE )
  /** The wires' green: the most saturated green pixel's (a thin line is blended with the background, so its full colour is the one with the least red and blue). */
  const wireGreen = () => {
    const d = pixels();
    let best = null;
    for (let i = 0; i < d.length; i += 4) if (d[i + 1] > d[i] + 80 && d[i + 1] > d[i + 2] + 80 && (!best || d[i] + d[i + 2] < best[0] + best[2])) best = [d[i], d[i + 1], d[i + 2]];
    return best;
  };
  // CSS_COLOR( 0, 150, 0, 1 ) in KiCad Default, COLOR4D( GREEN ) -- (0, 132, 0) in colorRefs() -- in KiCad Classic
  const isDefaultWire = (c) => !!c && c[1] >= 146 && c[1] <= 162;
  const isClassicWire = (c) => !!c && c[1] >= 124 && c[1] <= 143;
  const RED = (r, g, b) => r > 200 && g < 90 && b < 90 && Math.abs(g - b) < 25;
  /** Near (255, 0, 255) -- not KiCad Default's (132, 0, 132) fields. */
  const MAGENTA = (r, g, b) => r > 160 && b > 160 && g < 90;
  const BLUE = (r, g, b) => b > r + 80 && b > g + 80;
  const DEFAULT_FCU = near(200, 52, 52, 4); // s_defaultTheme F_Cu
  const CLASSIC_FCU = near(132, 0, 0, 4); // COLOR4D( RED )

  const sch = await json("/api/schematic");
  await clickTab("Schematic");
  await E.run("common.Control.zoomFitScreen").catch(() => null);
  await sleep(600);

  await scenario("Schematic Editor > Colors: KiCad's theme list, the swatches in LayerName() order, the override box, Import / Export, Reset Colors to Defaults", async (t) => {
    t((sch.wires ?? []).length > 0, `the sheet has wires to look at (${(sch.wires ?? []).length})`);
    t(same(corner(), DEFAULT_SCH_BG), `the schematic starts on KiCad Default's background (${corner()})`);
    t(isDefaultWire(wireGreen()), `and its wires in KiCad Default's green (${wireGreen()})`);
    await openPage("sch_colors");
    const page = tid("sch-colors-page");
    if (!t(page, "the page opens")) return;
    t(prefs().querySelector(".dialog-header span").textContent.trim() === "Preferences - Colors", "the dialog's title names the page");
    const opts = [...tid("sch-colors-theme").options].map((o) => o.textContent);
    t(opts.join("|") === "KiCad Classic (read-only)|KiCad Default (read-only)|KiCad Default|---|New Theme...", `the theme list (${opts.join("|")})`);
    const labels = [...page.querySelectorAll("[data-color-key]")].map((r) => r.textContent.trim());
    t(labels.length === 46 && labels[0] === "Anchors" && labels[1] === "Axes (symbol editor only)" && labels.includes("Schematic text & graphics") && !labels.includes("Sheet borders"), `46 swatches by name (${labels.length}: ${labels.slice(0, 3).join(", ")} ...)`);
    await click(tid("sch-colors-override"), 200);
    const withOverride = [...page.querySelectorAll("[data-color-key]")].map((r) => r.textContent.trim());
    t(withOverride.length === 48 && withOverride.includes("Sheet borders") && withOverride.includes("Sheet backgrounds"), `Override individual item colors shows the sheet's two (${withOverride.length})`);
    await click(tid("sch-colors-override"), 200);
    t(!!tid("sch-colors-import") && !!tid("sch-colors-export"), "Import Theme... and Export Theme...");
    t(!!footerButton(prefs(), "Reset Colors to Defaults"), "the dialog's Reset Colors to Defaults");
  });

  await scenario("KiCad Classic for the schematic: drawn while the page is up, kept by OK, the board's theme untouched", async (t) => {
    await setValue(tid("sch-colors-theme"), "_builtin_classic");
    await sleep(400);
    t(same(corner(), CLASSIC_SCH_BG), `the schematic is drawn on Classic's white while the page is up (${corner()})`);
    t(isClassicWire(wireGreen()), `its wires in Classic's green (${wireGreen()})`);
    t(document.querySelector('[data-testid="sch-colors-page"] [data-color-key="LAYER_WIRE"] [role=button]') && !swatch("LAYER_WIRE").title, "a read-only theme's swatch offers no edit");
    await prefsOk();
    await redraw();
    t(!prefs() && same(corner(), CLASSIC_SCH_BG), `OK keeps it (${corner()})`);
    const s = stored();
    t(s?.schTheme === "_builtin_classic" && (s?.pcbTheme ?? "user") === "user", `eeschema's theme is Classic, pcbnew's is not (${JSON.stringify({ sch: s?.schTheme, pcb: s?.pcbTheme })})`);
  });

  await scenario("New Theme... with red wires and magenta symbols: the sheet's wires are drawn red", async (t) => {
    await openPage("sch_colors");
    const red0 = count(RED);
    await setValue(tid("sch-colors-theme"), "new");
    const nd = await until(() => dialogTitled("Add Color Theme"), 3000);
    if (!t(nd, "New Theme... asks for the name")) return;
    await setValue(nd.querySelector("input"), "Red Wires");
    await click(footerButton(nd, "OK"), 400);
    t(tid("sch-colors-theme").value === "Red Wires", `the new theme is shown (${tid("sch-colors-theme").value})`);
    t(same(corner(), CLASSIC_SCH_BG), `it has the colours of the theme it was made from (Classic's white: ${corner()})`);
    await dbl(swatch("LAYER_WIRE"));
    await pickColor("#ff0000");
    for (const key of ["LAYER_DEVICE", "LAYER_DEVICE_BACKGROUND", "LAYER_PIN"]) {
      await dbl(swatch(key));
      await pickColor("#ff00ff");
    }
    await sleep(300);
    t(count(RED) > red0 + 20, `the wires are drawn red while the page is up (${red0} -> ${count(RED)} px)`);
    const th = stored()?.themes?.find((x) => x.filename === "Red Wires");
    t(th?.colors?.LAYER_WIRE === "#ff0000ff" && th?.colors?.LAYER_DEVICE === "#ff00ffff", `the swatches are saved at once (${JSON.stringify(th?.colors && { w: th.colors.LAYER_WIRE, d: th.colors.LAYER_DEVICE })})`);
    await prefsOk();
    await redraw();
    t(count(RED) > red0 + 20 && stored()?.schTheme === "Red Wires", `OK makes it the schematic editor's (${stored()?.schTheme}, ${count(RED)} red px)`);
  });

  await scenario("the Symbol Editor draws in the schematic editor's theme until Symbol Editor > Colors says Use theme:", async (t) => {
    // the symbols are on the sheets under the root: the first one with a library symbol (a generated `gen:` one has no library body to draw)
    let sheet = null;
    let symbol = null;
    for (const s of sch.sheets ?? []) {
      const sub = await json(`/api/schematic?sheet=${encodeURIComponent(s.id)}`);
      symbol = (sub.symbols ?? []).find((x) => x.lib_id && /:/.test(x.lib_id) && !/^(power|gen):/.test(x.lib_id));
      if (symbol) { sheet = s; break; }
    }
    if (!t(sheet && symbol, `a library symbol to open (${symbol?.lib_id} on ${sheet?.name})`)) return;
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [sheet.id]);
    await sleep(300);
    await E.run("eeschema.NavigateTool.enterSheet");
    await sleep(1000);
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [symbol.id]);
    await sleep(300);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "E", code: "KeyE", ctrlKey: true, shiftKey: true, bubbles: true }));
    if (!t(await until(() => E.state().tab === "symbol", 5000), "Edit Symbol (Ctrl+Shift+E) shows the Symbol Editor")) return;
    await sleep(1500);
    await E.run("common.Control.zoomFitScreen").catch(() => null);
    await sleep(500);
    const m0 = count(MAGENTA);
    t(m0 > 20, `its outlines are the schematic theme's magenta (${m0} px)`);
    await openPage("sym_colors");
    t(tid("sym-colors-use-sch")?.checked === true, "Use schematic editor color theme is on");
    const names = [...tid("sym-colors-theme").options].map((o) => o.textContent);
    t(names.includes("KiCad Classic") && names.includes("Red Wires") && !names.some((n) => /read-only/.test(n)), `the themes by name, no "(read-only)" (${names.join("|")})`);
    await setValue(tid("sym-colors-theme"), "_builtin_default");
    t(tid("sym-colors-use-theme")?.checked === true, "picking a theme picks Use theme:");
    await prefsOk();
    await sleep(400);
    t(count(MAGENTA) < m0 / 4, `with KiCad Default of its own the outlines are not magenta (${m0} -> ${count(MAGENTA)} px)`);
    t(stored()?.symUseSchTheme === false && stored()?.symTheme === "_builtin_default", `kept (${JSON.stringify({ use: stored()?.symUseSchTheme, theme: stored()?.symTheme })})`);
    await openPage("sym_colors");
    await click(tid("sym-colors-use-sch"), 150);
    await prefsOk();
    await sleep(400);
    t(count(MAGENTA) > 20, `back on the schematic editor's theme they are (${count(MAGENTA)} px)`);
    await clickTab("Schematic");
    while (E.state().sheetPath.length > 0) { await E.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    await E.run("common.Control.zoomFitScreen").catch(() => null);
    await sleep(400);
  });

  await scenario("Footprint Editor > Colors: KiCad Classic for the Footprint Editor alone", async (t) => {
    await clickTab("PCB");
    const boardFcu = count(DEFAULT_FCU);
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", ["R1"]);
    await sleep(300);
    await E.run("pcbnew.EditorControl.EditFpInFpEditor");
    if (!t(await until(() => E.state().tab === "footprint", 6000), "Edit Footprint shows R1 in the Footprint Editor")) return;
    await sleep(1500);
    await E.run("common.Control.zoomFitScreen").catch(() => null);
    await sleep(500);
    const fp0 = count(DEFAULT_FCU);
    t(fp0 > 50, `its pads are KiCad Default's F.Cu (${fp0} px)`);
    await openPage("fp_colors");
    const page = tid("fp-colors-page");
    const labels = page ? [...page.querySelectorAll("[data-color-key]")].map((r) => r.textContent.trim()) : [];
    t(labels.slice(0, 3).join("|") === "F.Cu|Internal Layers|B.Cu" && labels.length === 21, `F.Cu, Internal Layers, B.Cu and the GAL layers (${labels.length}: ${labels.slice(0, 4).join(", ")})`);
    await setValue(tid("fp-colors-theme"), "_builtin_classic");
    await sleep(300);
    t(count(CLASSIC_FCU) > 50, `the page's theme is drawn while it is up (${count(CLASSIC_FCU)} px of Classic's F.Cu)`);
    await prefsOk();
    await sleep(400);
    t(count(CLASSIC_FCU) > 50 && count(DEFAULT_FCU) < fp0 / 4, `OK keeps it (${count(CLASSIC_FCU)} Classic, ${count(DEFAULT_FCU)} default px)`);
    t(stored()?.fpTheme === "_builtin_classic" && (stored()?.pcbTheme ?? "user") === "user", `the Footprint Editor's theme only (${JSON.stringify({ fp: stored()?.fpTheme, pcb: stored()?.pcbTheme })})`);
    await clickTab("PCB");
    await redraw();
    t(count(DEFAULT_FCU) > 0 && Math.abs(count(DEFAULT_FCU) - boardFcu) <= boardFcu / 5 + 20, `the board keeps KiCad Default's F.Cu (${boardFcu} -> ${count(DEFAULT_FCU)} px)`);
    await openPage("fp_colors");
    await setValue(tid("fp-colors-theme"), "user");
    await prefsOk();
    t((stored()?.fpTheme ?? "user") === "user", "back on its default theme");
  });

  await scenario("Export Theme...: the shown theme as KiCad writes colors/<name>.json", async (t) => {
    await clickTab("Schematic");
    await openPage("sch_colors");
    let saved = null;
    const createURL = URL.createObjectURL;
    const aClick = HTMLAnchorElement.prototype.click;
    URL.createObjectURL = (blob) => { saved = { blob }; return "blob:check"; };
    HTMLAnchorElement.prototype.click = function () { if (saved) saved.name = this.download; };
    try {
      await click(tid("sch-colors-export"), 300);
    } finally {
      URL.createObjectURL = createURL;
      HTMLAnchorElement.prototype.click = aClick;
    }
    if (!t(saved?.blob, "a file is saved")) return;
    const text = await saved.blob.text();
    const j = JSON.parse(text);
    t(saved.name === "Red Wires.json", `named after the theme (${saved.name})`);
    t(j.meta?.name === "Red Wires" && j.meta?.version === 6 && j.meta?.filename === "Red Wires.json", `meta (${JSON.stringify(j.meta)})`);
    t(j.schematic?.wire === "rgb(255, 0, 0)" && j.schematic?.component_outline === "rgb(255, 0, 255)" && j.schematic?.background === "rgb(255, 255, 255)", `the schematic's colours in KiCad's words (${j.schematic?.wire}, ${j.schematic?.component_outline})`);
    t(typeof j.board?.copper?.f === "string" && j.schematic?.override_item_colors === false, "the board's colours too, and the override flag");
    t(JSON.stringify(Object.keys(j)) === JSON.stringify(Object.keys(j).sort()) && text.startsWith('{\n  "board": {'), "sorted keys, indented by two");
  });

  await scenario("Import Theme...: a KiCad theme file listed, shown and drawn; OK makes it the schematic editor's", async (t) => {
    const file = new File([JSON.stringify({ meta: { name: "Imported Yellow", version: 6 }, schematic: { background: "rgb(250, 250, 200)", wire: "rgb(0, 0, 255)" } })], "imported_yellow.json", { type: "application/json" });
    const input = tid("sch-colors-import-file");
    if (!t(input, "the page's file input")) return;
    const dt = new DataTransfer();
    dt.items.add(file);
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => tid("sch-colors-theme")?.value === "imported_yellow", 4000);
    t(tid("sch-colors-theme").value === "imported_yellow", `the imported theme is shown (${tid("sch-colors-theme").value})`);
    t([...tid("sch-colors-theme").options].some((o) => o.textContent === "Imported Yellow"), "listed by its meta.name");
    await sleep(300);
    t(same(corner(), [250, 250, 200]), `drawn while the page is up (${corner()})`);
    t(count(BLUE) > 20, `its blue wires (${count(BLUE)} px)`);
    await prefsOk();
    t(stored()?.schTheme === "imported_yellow" && stored()?.themes?.some((x) => x.filename === "imported_yellow" && x.colors.LAYER_WIRE === "#0000ffff"), `kept as the schematic editor's (${stored()?.schTheme})`);
  });

  await scenario("Cancel: a theme picked on the page, the dialog cancelled, the schematic back in the editor's theme", async (t) => {
    await openPage("sch_colors");
    await setValue(tid("sch-colors-theme"), "_builtin_default");
    await sleep(300);
    t(same(corner(), DEFAULT_SCH_BG), `the picked theme is drawn (${corner()})`);
    await prefsCancel();
    await redraw();
    t(!prefs() && same(corner(), [250, 250, 200]), `Cancel puts the editor's theme back (${corner()})`);
    t(stored()?.schTheme === "imported_yellow", "and keeps the editor's choice");
  });

  await scenario("end: every editor on its default theme, no console errors", async (t) => {
    await openPage("sch_colors");
    await setValue(tid("sch-colors-theme"), "user");
    await prefsOk();
    await redraw();
    const s = stored();
    t((s?.schTheme ?? "user") === "user" && (s?.fpTheme ?? "user") === "user" && (s?.pcbTheme ?? "user") === "user" && s?.symUseSchTheme !== false, `back on "user" (${JSON.stringify({ sch: s?.schTheme, fp: s?.fpTheme, pcb: s?.pcbTheme, sym: s?.symUseSchTheme })})`);
    t(same(corner(), DEFAULT_SCH_BG), `the schematic on KiCad Default's background (${corner()})`);
    const errs = (E.errors?.() ?? []).slice(errors0);
    t(errs.length === 0, `no console errors (${errs.map((e) => e.message ?? e).slice(0, 3).join(" | ")})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
