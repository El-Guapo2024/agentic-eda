// Preferences > Schematic Editor and what it drives (UI-LEFT-3 S3-1, S3-3, S3-26, C3-2; task-v14-schprefs), driven by the dialog's own controls and pointer events on
// the canvas:
//   tree      Schematic Editor > ... Toolbars, Field Name Templates, Simulator; the Editing Options rows that were greyed are live (only Automatically annotate
//             symbols stays greyed); the Symbol Editor's Defaults for New Objects are inputs
//   S3-1      a click of the selection tool on an unconnected pin starts a wire there (the hover shows KiCad's wire cursor first); the wire finishes with a
//             double-click and the selection tool is back (`quitOnDraw`); the command carried the person's autoplace and sheet colours; with "Automatically start
//             wires on unconnected pins" off, the click selects the pin; KiCad's LINE_WIRE cursor over such a point; a wire started from a wire's end takes its stroke
//   S3-26     "Mouse drag performs Drag (G)" off: a drag of the symbol moves it away from its wire (the wire stays); on (KiCad's default) the wire's end follows; the
//             line mode is kept with the preferences; a sheet drawn takes the default sheet colours; the never-show-rescue row is stored
//   S3-3      a symbol placed with "Automatically place symbol fields" off is not autoplaced (the request's `autoplace`), with it on it is
//   C3-2      Field Name Templates: a global template added on the page is stored and offered by the Symbol Fields Table as a column; Simulator: the wheel's actions
//             are stored, and both reset buttons work
//   end       every preference as it was, nothing left on the sheet, no console errors
// Run it in the page of a served board made from examples/ldo.yaml, on a COPY of a board: every scenario undoes what it did.
// It answers through `window.__schPrefsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__schPrefsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  const E = window.__eda;
  const state = () => E.state();
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(80); return v; };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const toSelect = async () => { for (let i = 0; i < 3 && state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(300); } };
  const cmd = (c, extra = {}) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false, ...extra }) });
  const sch = () => json("/api/schematic");
  const mm = (v) => Math.round(v * 1e6);
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; return !!cond; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    results.push({ name, ok, notes });
    if (dlg()) await cancel();
    await run("common.InteractiveSelection.clear");
    await toSelect();
  };

  // the Preferences dialog
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dlg = () => tid("preferences-dialog");
  const openPrefs = async (page) => {
    if (!dlg()) { await E.run("common.SuiteControl.openPreferences"); await until(dlg); }
    if (page) { tid(`prefs-page-${page}`)?.click(); await sleep(250); }
    return dlg();
  };
  const ok = async () => { tid("preferences-ok")?.click(); await sleep(500); };
  const cancel = async () => { [...(dlg()?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.textContent.trim() === "Cancel")?.click(); await sleep(250); };
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(120);
  };
  const setCheck = async (id, on) => { const el = tid(id); if (el && el.checked !== on) { el.click(); await sleep(120); } return el; };
  const PREFS_KEY = "eda-studio.preferences.v1";
  const stored = () => { try { return JSON.parse(localStorage.getItem(PREFS_KEY) ?? "null"); } catch { return null; } };
  const sortedJson = (v) => JSON.stringify(v && typeof v === "object" ? Object.fromEntries(Object.keys(v).filter((k) => k !== "schemaVersion").sort().map((k) => [k, v[k]])) : v);
  const prefs0 = sortedJson(stored());
  const errors0 = E.errors ? E.errors().length : 0;

  // the canvas
  const sContainer = () => document.querySelector(".pcb-canvas-container");
  const sFire = (type, x, y, extra = {}) => {
    const rect = sContainer().getBoundingClientRect();
    (sContainer().querySelector("canvas") || sContainer()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const sScreen = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async ([x, y]) => { const [a, b] = sScreen(x, y); sFire("pointermove", a, b); await sleep(60); return sContainer().dataset.autostartHover ?? null; };
  const clickAt = async ([x, y]) => { const [a, b] = sScreen(x, y); sFire("pointermove", a, b); await sleep(80); sFire("pointerdown", a, b, { buttons: 1 }); await sleep(60); sFire("pointerup", a, b); await sleep(400); };
  const dblClickAt = async ([x, y]) => {
    await clickAt([x, y]);
    const [a, b] = sScreen(x, y);
    const rect = sContainer().getBoundingClientRect();
    sContainer().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: rect.left + a, clientY: rect.top + b, button: 0 }));
    await sleep(1200);
  };
  const dragFrom = async ([x0, y0], [x1, y1]) => {
    const [a, b] = sScreen(x0, y0);
    const [c, d] = sScreen(x1, y1);
    sFire("pointermove", a, b); await sleep(150);
    sFire("pointerdown", a, b, { buttons: 1 }); await sleep(60);
    for (let i = 1; i <= 6; i++) { sFire("pointermove", a + ((c - a) * i) / 6, b + ((d - b) * i) / 6, { buttons: 1 }); await sleep(80); }
    sFire("pointerup", c, d); await sleep(1500);
  };

  // what the page sends with its commands
  const sent = [];
  const pageFetch = window.fetch;
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input.url;
    if (/\/api\/cmd$/.test(url) && init?.body) { try { sent.push(JSON.parse(init.body)); } catch { /* not json */ } }
    return pageFetch(input, init);
  };

  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  await run("common.Control.zoomFitScreen");
  await sleep(500);
  const G = mm(2.54);
  // a free corner of the page for the scenarios' items
  const X = mm(50.8), Y = mm(50.8);
  const symbols0 = JSON.stringify(((await sch()).symbols ?? []).map((s) => `${s.id}#${s.unit}`).sort());

  await scenario("tree: Field Name Templates and Simulator after Toolbars; the Editing Options rows are live", async (t) => {
    await openPrefs("sch_edit");
    const labels = [...dlg().querySelectorAll('[role="treeitem"]')].map((e) => e.textContent.trim());
    const i = labels.indexOf("Schematic Editor");
    t(JSON.stringify(labels.slice(i, i + 8)) === JSON.stringify(["Schematic Editor", "Display Options", "Grids", "Editing Options", "Colors", "Toolbars", "Field Name Templates", "Simulator"]), `Schematic Editor's pages: ${labels.slice(i, i + 8).join(" | ")}`);
    for (const id of ["pref-sch-mouse-drag-is-drag", "pref-sch-auto-start-wires", "pref-sch-autoplace-fields", "pref-sch-autoplace-justify", "pref-sch-autoplace-align", "pref-sch-never-show-rescue"]) t(tid(id) && !tid(id).disabled, `${id} is a live switch`);
    t(tid("pref-sch-mouse-drag-is-drag")?.checked === true && tid("pref-sch-auto-start-wires")?.checked === true, "KiCad's defaults: mouse drag is Drag (G), wires start on unconnected pins");
    t(!!tid("pref-sch-sheet-border")?.querySelector('input[type="color"]') && !!tid("pref-sch-sheet-background"), "the new sheets' colour swatches");
    const greyed = [...dlg().querySelectorAll('[data-not-offered="true"]')].map((g) => g.textContent.trim());
    t(greyed.length === 1 && /Automatically annotate symbols/.test(greyed[0] ?? ""), `greyed: ${JSON.stringify(greyed)}`);
    await openPrefs("sym_edit");
    t(["pref-sym-default-line-width", "pref-sym-default-text-size", "pref-sym-default-pin-length", "pref-sym-default-pin-num-size", "pref-sym-default-pin-name-size"].every((id) => tid(id) && !tid(id).disabled), "the Symbol Editor's Defaults for New Objects are inputs");
    t(Number(tid("pref-sym-default-pin-length")?.value) === 100 && Number(tid("pref-sym-default-text-size")?.value) === 50, "KiCad's defaults (100 and 50 mils)");
    await cancel();
  });

  // a resistor with nothing on its pins
  const added = await cmd({ op: "add_symbol", id: "R91", lib_id: "Device:R", at: { x: X, y: Y }, rot_millideg: 0, value: "1k", footprint: "", unit: 1 });
  await sleep(1200);
  /** The points of the sheet around the resistor where the selection tool would start a wire (the pin ends). */
  const findPinEnds = async () => {
    const out = [];
    for (let dy = -4; dy <= 10; dy++) for (let dx = -3; dx <= 5; dx++) {
      const p = [X + dx * mm(1.27), Y + dy * mm(1.27)];
      if ((await hoverAt(p)) === "wire") out.push(p);
    }
    return out;
  };

  await scenario("S3-1: a click on an unconnected pin starts a wire there; the double-click finishes it and the selection tool is back", async (t) => {
    if (!t(added.ok !== false, `add_symbol: ${added.message ?? "ok"}`)) return;
    const ends = await findPinEnds();
    if (!t(ends.length >= 2, `the hover finds the resistor's pin ends (${ends.length} points)`)) return;
    await hoverAt(ends[0]);
    t(/^url\("data:image\/svg\+xml,.*\) 5 26, crosshair$/.test(sContainer().style.cursor), `the cursor over a pin end is KiCad's LINE_WIRE pencil (${sContainer().style.cursor.slice(0, 40)}...)`);
    const [top] = ends;
    sent.length = 0;
    await clickAt(top);
    t(state().tool === "wire", `the click armed the wire tool (${state().tool})`);
    const to = [top[0] - 4 * G, top[1] - 2 * G];
    await dblClickAt(to);
    t(state().tool === "select", `the selection tool is back after the wire (${state().tool})`);
    const w = (await sch()).wires.find((x) => x.pts.some((q) => q[0] === to[0] && q[1] === to[1]));
    t(!!w, `a wire ends where the double-click was (${JSON.stringify(w?.pts)})`);
    const body = sent.find((b) => b.cmd?.op === "add_wire");
    t(!!body && body.autoplace?.enable === true && "sheet_colors" in body, `the command carried the preferences (${JSON.stringify(body && { autoplace: body.autoplace, sheet_colors: body.sheet_colors })})`);
    // the pin is connected now: no wire starts there
    t((await hoverAt(top)) === null, "the pin with the wire is no longer an autostart point");
    // with the switch off, a click on the other pin end selects the pin
    await openPrefs("sch_edit");
    await setCheck("pref-sch-auto-start-wires", false);
    await ok();
    t(stored()?.schAutoStartWires === false, "stored off");
    const other = ends[ends.length - 1];
    t((await hoverAt(other)) === null, "no crosshair with the switch off");
    await clickAt(other);
    t(state().tool === "select" && state().selection.some((s) => /^pin:R91:/.test(s.id ?? s)), `the click selected the pin (${JSON.stringify(state().selection)})`);
    await openPrefs("sch_edit");
    await setCheck("pref-sch-auto-start-wires", true);
    await ok();
    await undo();
    t(!(await sch()).wires.some((x) => x.pts.some((q) => q[0] === to[0] && q[1] === to[1])), "one undo takes the wire back");
  });

  await scenario("S3-1: a wire started from a wire's free end takes that wire's stroke", async (t) => {
    const a0 = [X + 10 * G, Y + 30 * G], a1 = [X + 16 * G, Y + 30 * G];
    await cmd({ op: "add_wire", pts: [{ x: a0[0], y: a0[1] }, { x: a1[0], y: a1[1] }] });
    await sleep(800);
    const w0 = (await sch()).wires.find((w) => w.pts.some((q) => q[0] === a1[0] && q[1] === a1[1]));
    if (!t(!!w0, "the source wire")) return;
    const stroke = { width_nm: 300000, style: "dash", color: { r: 200, g: 40, b: 40, a: 255 } };
    const set = await cmd({ op: "sch_edit", verb: "set_stroke", ids: [w0.id], ...stroke });
    t(set.ok !== false, `set_stroke: ${set.message ?? "ok"}`);
    await sleep(800);
    t((await hoverAt(a1)) === "wire", "its free end is an autostart point");
    await clickAt(a1);
    const to = [a1[0], a1[1] + 4 * G];
    await dblClickAt(to);
    const s1 = await sch();
    const w1 = s1.wires.find((w) => w.id !== w0.id && w.pts.some((q) => q[0] === to[0] && q[1] === to[1]));
    t(!!w1 && w1.stroke?.width_nm === 300000 && w1.stroke?.color?.r === 200 && w1.stroke?.style === "dash", `the new wire has the clicked wire's stroke (${JSON.stringify(w1?.stroke)})`);
    await undo();
    await undo();
    await undo();
    t(!(await sch()).wires.some((w) => w.pts.some((q) => q[0] === a1[0] && q[1] === a1[1])), "undone");
  });

  await scenario("S3-26: a mouse drag moves (M) with Drag (G) off, and drags (the wire follows) with it on", async (t) => {
    const ends = await findPinEnds();
    if (!t(ends.length >= 1, "the resistor's pin ends")) return;
    const pinEnd = ends[0];
    const far = [pinEnd[0] - 4 * G, pinEnd[1]];
    await cmd({ op: "add_wire", pts: [{ x: far[0], y: far[1] }, { x: pinEnd[0], y: pinEnd[1] }] });
    await sleep(1000);
    const grab = [X + mm(1.016), Y + mm(3.81)];
    const r0 = (await sch()).symbols.find((s) => s.id === "R91");
    await openPrefs("sch_edit");
    await setCheck("pref-sch-mouse-drag-is-drag", false);
    await ok();
    t(stored()?.schDragIsMove === true, "stored: drag_is_move");
    // "Left button drag:" Drag selected objects (KiCad's default): the symbol is selected first
    await clickAt(grab);
    await dragFrom(grab, [grab[0] + 2 * G, grab[1]]);
    let s1 = await sch();
    const r1 = s1.symbols.find((s) => s.id === "R91");
    t(r1 && r1.at[0] === r0.at[0] + 2 * G, `the symbol moved two grid steps (${JSON.stringify(r0?.at)} -> ${JSON.stringify(r1?.at)})`);
    t(s1.wires.some((w) => w.pts.some((q) => q[0] === pinEnd[0] && q[1] === pinEnd[1])), "the wire stayed where it was (Move)");
    await undo();
    await openPrefs("sch_edit");
    await setCheck("pref-sch-mouse-drag-is-drag", true);
    await ok();
    await clickAt(grab);
    await dragFrom(grab, [grab[0] + 2 * G, grab[1]]);
    s1 = await sch();
    t(s1.wires.some((w) => w.pts.some((q) => q[0] === pinEnd[0] + 2 * G && q[1] === pinEnd[1])), "the wire's end followed the pin (Drag)");
    await undo();
    await undo();
  });

  await scenario("S3-26: the line mode is kept with the preferences; a sheet drawn takes the new sheets' colours; Never show Rescue Symbols tool is stored", async (t) => {
    await openPrefs("sch_edit");
    await setValue(tid("pref-sch-line-mode"), "2");
    await setCheck("pref-sch-never-show-rescue", true);
    // the border's colour: "Default" unticked, then a colour
    const border = tid("pref-sch-sheet-border");
    border.querySelector('input[type="checkbox"]').click();
    await sleep(120);
    await setValue(border.querySelector('input[type="color"]'), "#c80000");
    await ok();
    const s = stored();
    t(s?.schLineMode === 2 && s?.schRescueNeverShow === true, `stored: line mode ${s?.schLineMode}, never show rescue ${s?.schRescueNeverShow}`);
    t(s?.schSheetBorderColor?.r === 200 && s?.schSheetBorderColor?.g === 0 && s?.schSheetBackgroundColor === null, `the sheet border ${JSON.stringify(s?.schSheetBorderColor)}, the background unspecified`);
    // a sheet drawn through the page's own command path carries them
    await run("eeschema.InteractiveDrawing.drawSheet");
    await clickAt([X + 20 * G, Y]);
    await clickAt([X + 28 * G, Y + 6 * G]);
    const okButton = await until(() => [...document.querySelectorAll(".dialog .dialog-footer button")].find((b) => b.textContent.trim() === "Add"));
    if (t(!!okButton, "Hierarchical Sheet Properties opened")) {
      okButton.click();
      await sleep(1500);
      const sheet = (await sch()).sheets.find((x) => x.at[0] === X + 20 * G && x.at[1] === Y);
      t(sheet && sheet.props?.border_color?.r === 200 && !sheet.props?.fill_color, `the new sheet's border is the default colour, its background none of its own (${JSON.stringify(sheet?.props)})`);
      await toSelect();
      await undo();
    }
    t(!(await sch()).sheets.some((x) => x.at[0] === X + 20 * G), "the sheet is undone");
  });

  await scenario("S3-3: a symbol placed with Automatically place symbol fields off is not autoplaced; on, it is", async (t) => {
    const off = await cmd({ op: "add_symbol", id: "R92", lib_id: "Device:R", at: { x: X, y: Y + 20 * G }, rot_millideg: 0, value: "2k", footprint: "", unit: 1 }, { autoplace: { enable: false, allow_rejustify: true, align_to_grid: true } });
    const on = await cmd({ op: "add_symbol", id: "R93", lib_id: "Device:R", at: { x: X + 10 * G, y: Y + 20 * G }, rot_millideg: 0, value: "3k", footprint: "", unit: 1 }, { autoplace: { enable: true, allow_rejustify: true, align_to_grid: true } });
    t(off.ok !== false && on.ok !== false, `placed (${off.message ?? "ok"}, ${on.message ?? "ok"})`);
    await sleep(1000);
    const s = await sch();
    const r92 = s.symbols.find((x) => x.id === "R92");
    const r93 = s.symbols.find((x) => x.id === "R93");
    t(r92 && !r92.fields_autoplaced, `R92 placed with it off: not autoplaced (${JSON.stringify(r92?.fields_autoplaced)})`);
    // turned (with automatic placement on): R92's fields, not autoplaced, turn with it and read up the sheet; R93's are placed again and read across
    const one = async (id) => cmd({ op: "sch_move", verb: "rotate", ids: [id], vertices: {}, ccw: true }, { autoplace: { enable: true, allow_rejustify: true, align_to_grid: true } });
    await one("R92");
    await one("R93");
    await sleep(1000);
    const s2 = await sch();
    const ref = (id) => s2.symbols.find((x) => x.id === id)?.fields?.find((f) => f.name === "Reference");
    t(ref("R92")?.vertical === true, `R92's Reference turned with it (vertical ${ref("R92")?.vertical})`);
    t(ref("R93")?.vertical === false, `R93's Reference was placed again, across the sheet (vertical ${ref("R93")?.vertical})`);
    await undo();
    await undo();
    await undo();
    await undo();
  });

  await scenario("C3-2: Field Name Templates (global) and Simulator", async (t) => {
    await openPrefs("sch_field_names");
    t(/Global Field Name Templates/.test(tid("sch-field-name-templates")?.textContent ?? ""), "the page says Global Field Name Templates");
    tid("global-field-add").click();
    await sleep(150);
    await setValue(dlg().querySelector('[aria-label="Field name 1"]'), "E2E_Supplier");
    t(tid("preferences-reset")?.disabled === true, "no reset on this page (a plain wxPanel)");
    await openPrefs("sch_simulator");
    await setValue(tid("pref-sim-wheel-verticalUnmodified"), "zoom_horizontally");
    await ok();
    const s = stored();
    t(JSON.stringify(s?.schGlobalFieldNames) === JSON.stringify([{ name: "E2E_Supplier", visible: false, url: false }]), `stored templates ${JSON.stringify(s?.schGlobalFieldNames)}`);
    t(s?.simWheel?.verticalUnmodified === "zoom_horizontally", `stored wheel action ${s?.simWheel?.verticalUnmodified}`);
    // the Symbol Fields Table offers the global template as a column
    const table = await json("/api/sch/fields_table", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ spec: null, changes: null, global_field_names: s.schGlobalFieldNames }) });
    t((table.spec?.columns ?? []).some((c) => c.name === "E2E_Supplier" && !c.show), `the fields table has a hidden E2E_Supplier column (${table.ok !== false})`);
    await openPrefs("sch_simulator");
    tid("sim-wheel-trackpad-defaults").click();
    await sleep(120);
    t(tid("pref-sim-wheel-verticalUnmodified")?.value === "pan_up_down", "Reset to Trackpad Defaults");
    tid("preferences-reset").click();
    await sleep(120);
    t(tid("pref-sim-wheel-verticalUnmodified")?.value === "zoom", "Reset Simulator to Defaults: the mouse defaults");
    await cancel();
  });

  await scenario("end: the preferences as they were, nothing left on the sheet, no console errors", async (t) => {
    // the preferences back as they were (the stored object, as it was read at the start)
    const back = JSON.parse(prefs0);
    if (back) {
      await openPrefs("sch_edit");
      await setValue(tid("pref-sch-line-mode"), String(back.schLineMode ?? 1));
      await setCheck("pref-sch-never-show-rescue", !!back.schRescueNeverShow);
      const border = tid("pref-sch-sheet-border");
      const dflt = border?.querySelector('input[type="checkbox"]');
      if (dflt && !dflt.checked && !back.schSheetBorderColor) { dflt.click(); await sleep(100); }
      await openPrefs("sch_field_names");
      for (let i = 0; i < 5 && dlg().querySelector('[aria-label="Field name 1"]'); i++) {
        dlg().querySelector('[aria-label="Field name 1"]').focus();
        dlg().querySelector('[aria-label="Field name 1"]').closest("tr")?.click();
        await sleep(80);
        dlg().querySelector('[aria-label="Delete field"]')?.click();
        await sleep(120);
      }
      await openPrefs("sch_simulator");
      tid("preferences-reset").click();
      await sleep(120);
      await ok();
    }
    // the R91 symbol the scenarios used
    await undo();
    const symbols = JSON.stringify(((await sch()).symbols ?? []).map((s) => `${s.id}#${s.unit}`).sort());
    t(symbols === symbols0, `the sheet's symbols are as they were (${symbols === symbols0 ? "same" : symbols})`);
    const now = sortedJson(stored());
    t(now === prefs0 || !prefs0 || prefs0 === "null", `the stored preferences are as they were${now === prefs0 ? "" : `: ${now.slice(0, 300)}`}`);
    const errors = E.errors ? E.errors(errors0) : [];
    t(errors.length === 0, `no console errors (${JSON.stringify(errors.slice(0, 3))})`);
  });

  window.fetch = pageFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
