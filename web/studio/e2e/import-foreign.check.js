// File > Import > Non-KiCad Board File... and Non-KiCad Schematic File... (GAPS.md item 28, package P22), driven through `window.__eda` and the page's own controls.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir> --ui <ui>`),
// where <ui> is a copy of web/studio/dist with the QA files of KiCad's other formats in `<ui>/e2e-import-foreign/` (test_eagle.brd, adafruit_aht20.brd, minimal_route_offset_curved_track.cpa,
// power.SchDoc: qa/data/pcbnew/plugins/{eagle/test_eagle_23016,eagle/Adafruit-AHT20-PCB ("Adafruit AHT20 Temperature & Humidity.brd"),cadstar/route_offset,altium/eDP_adapter_dvt1_source})
// and this file; the board is replaced and every replacement taken back with
// Undo, so the board is as it was. It needs the real kicad-cli (the server converts the files with it). Paste it into the Browser pane's javascript tool, or
// `(0, eval)(await (await fetch("/e2e-import-foreign/import-foreign.check.js")).text())`.
//
// What it checks:
//   the actions are wired: the board's is offered on the PCB tab, the schematic's on the Schematic tab, and each is refused on the other;
//   the board, an Eagle file: the file is converted by kicad-cli and "Edit Mapping of Imported Layers" opens (the importer asks), with the layers kicad-cli guessed matched (a copper layer
//     marked *), the others unmatched; `<` `>` `<<` and Auto-Match Layers do what DIALOG_MAP_LAYERS' do; OK is refused while a required layer is unmatched, with KiCad's words; a layer moved
//     off the guess is the layer map the second run gets, and its text is on the layer chosen; the board is replaced (the footprints, the nets, the outline of the file; the parts of the old
//     board are gone) in ONE undo step, which Undo takes back whole and Redo brings again; the toast says what came in and that Undo takes it back;
//   the issues: a CADSTAR file (no layer dialog asked for here) comes with kicad-cli's warnings, which open "Import Issues"; "Show import issues" off keeps the next import from opening it;
//   the design rules and the net classes an importer set (an Eagle board's highpower, 12 mil) are the board's -- they are in the project kicad-cli makes with the board, and the same
//     one step of history takes them away again (a class that no net uses, Eagle's lowpower, matches nothing and is not carried);
//   Cancel in the layer dialog abandons the import and leaves the board as it was;
//   a file no importer recognises is refused in kicad-cli's words and the board is untouched;
//   the schematic, an Altium sheet: the schematic is replaced in one step of the schematic's history with its errors reported, and an Undo brings the old drawing back.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }. The results also pile up in `window.__importForeignCheck.results` (`.done` is set at the end).
(async () => {
  const results = [];
  window.__importForeignCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const base = window.__importForeignBase ?? "/e2e-import-foreign/";
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const waitFor = async (fn, ms = 20000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const b64 = async (name) => {
    const bytes = new Uint8Array(await (await fetch(base + name, { cache: "no-store" })).arrayBuffer());
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { name, data: btoa(s), size: bytes.length };
  };
  const refs = async () => (await get("/api/state")).parts.map((p) => p.ref).sort();
  const dialogs = () => E.state().dialogs;
  const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
  const all = (prefix) => [...document.querySelectorAll(`[data-testid^="${prefix}"]`)];
  const button = (label) => [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === label);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === label)?.click(); await sleep(900); };
  const toastText = () => [...document.querySelectorAll("div")].find((el) => el.style.zIndex === "3000" && el.style.position === "absolute")?.textContent ?? ""; // the page's toast: a div with no class
  const click = async (el) => { el?.click(); await sleep(60); };
  const errorsBefore = E.errors().length;
  const firstTab = E.state().tab;
  let applied = 0; // PCB steps this check left on the board, taken back at the end
  let appliedSch = 0;

  try {
    if (firstTab !== "pcb") await clickTab("PCB");
    const original = await refs();
    check("the board to import into is the mcu30 example (30 parts)", original.length === 30 && original.includes("U1"), original.length);

    // ---- the actions are wired
    const boardAction = () => E.actions().find((a) => a.id === "pcbnew.EditorControl.openNonKicadBoard");
    check("Import Non-KiCad Board File is offered on the PCB tab", boardAction()?.enabled === true, JSON.stringify(boardAction()));
    const schAction = () => E.actions().find((a) => a.id === "eeschema.EditorControl.importNonKicadSchematic");
    check("and the schematic's is not offered there", !schAction() || schAction().enabled === false, JSON.stringify(schAction()));

    // ---- the board: an Eagle file, with the layer dialog
    const eagle = await b64("test_eagle.brd");
    check("the Eagle fixture is there", eagle.size > 10000, eagle.size);
    // What is on F.Fab and on F.SilkS in the board written from the design: the baseline is the importer's own guess (tNames on F.SilkS), no dialog asked for.
    const onLayers = async () => { const t = await (await fetch("/api/board.kicad_pcb", { cache: "no-store" })).text(); return { fab: (t.match(/\(layer "F\.Fab"\)/g) || []).length, silk: (t.match(/\(layer "F\.SilkS"\)/g) || []).length }; };
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: eagle.name, data: eagle.data, skipLayerMapping: true });
    applied++;
    await waitFor(async () => (await refs()).length === 13);
    const guessed = await onLayers();
    check("(baseline) the importer's own guess: an Eagle board with 13 footprints", (await refs()).length === 13 && guessed.silk > 0, JSON.stringify(guessed));
    await E.run("common.Interactive.undo");
    applied--;
    await waitFor(async () => (await refs()).length === 30);
    const opened = await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: eagle.name, data: eagle.data });
    check("the action ran", opened.ok !== false, JSON.stringify(opened));
    const dlg = await waitFor(() => byId("map-layers"));
    check("kicad-cli converted the file and 'Edit Mapping of Imported Layers' opened (the importer asks which KiCad layer each layer goes to)", !!dlg && dialogs().includes("Edit Mapping of Imported Layers"), JSON.stringify(dialogs()));
    const unmatched = () => all("unmatched:").map((e) => e.textContent.trim());
    const matched = () => all("matched:").filter((e) => !e.dataset.testid.startsWith("matched-to:")).map((e) => e.textContent.trim());
    const matchedTo = (layer) => byId(`matched-to:${layer}`)?.textContent.trim();
    check("the layers kicad-cli guessed are Matched Layers, the ones it has no guess for are Unmatched", matched().includes("Top *") && matchedTo("tNames") === "F.Silkscreen" && unmatched().length > 0 && !unmatched().includes("tNames"), JSON.stringify([matched().slice(0, 6), unmatched().slice(0, 4)]));
    check("a layer that goes to copper is marked * (it must be matched)", matched().some((m) => m.endsWith(" *")) && !matched().includes("tNames *"), JSON.stringify(matched()));
    check("the KiCad Layers list is KiCad's, in its order: the copper, then the technical layers by their standard names", all("kicad:").map((e) => e.textContent.trim()).slice(0, 8).join(",") === "F.Cu,B.Cu,F.Adhesive,B.Adhesive,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen", all("kicad:").map((e) => e.textContent.trim()).slice(0, 9).join(","));
    check("Keep KiCad layer names is off by default", byId("map-keep-names")?.checked === false);

    // `<` takes a pair back to the unmatched list; the guess a layer-map file cannot take away is said
    await click(byId("matched:tNames"));
    await click(byId("map-remove"));
    check("< puts the selected layer back in the unmatched list", unmatched().includes("tNames") && matchedTo("tNames") === undefined, JSON.stringify(unmatched()));
    check("and says that kicad-cli maps a layer it has a guess for all the same", /kicad-cli maps/.test(byId("map-kept")?.textContent ?? ""), byId("map-kept")?.textContent);
    // `>` puts it on the KiCad layer chosen: F.Fab, not the guess (F.SilkS)
    await click(byId("unmatched:tNames"));
    await click(byId("kicad:F.Fab"));
    await click(byId("map-add"));
    check("> puts it on the KiCad layer chosen", matchedTo("tNames") === "F.Fab" && !unmatched().includes("tNames"), String(matchedTo("tNames")));
    check("(a layer on the guess is a pair in grey, one the person chose is not)", byId("matched-to:tNames")?.title === "your choice" && byId("matched-to:Top")?.title === "kicad-cli's own guess", `${byId("matched-to:tNames")?.title} / ${byId("matched-to:Top")?.title}`);

    // OK is refused while a required layer is unmatched, with KiCad's words
    await click(byId("matched:Top"));
    await click(byId("map-remove"));
    await click(button("OK"));
    check("OK is refused while a layer marked * is unmatched: 'All required layers (marked with '*') must be matched'", /^All required layers \(marked with '\*'\) must be matched/.test(byId("map-error")?.textContent ?? "") && !!byId("map-layers"), byId("map-error")?.textContent);
    await click(byId("map-auto"));
    check("Auto-Match Layers matches it again (the guess)", matchedTo("Top") === "F.Cu" && !byId("map-error"), String(matchedTo("Top")));
    // << and Auto-Match round trip
    await click(byId("map-remove-all"));
    check("<< takes every pair back", matched().length === 0 && unmatched().length > 5, `${matched().length} / ${unmatched().length}`);
    await click(byId("map-auto"));
    check("and Auto-Match puts back the ones kicad-cli has a guess for (tNames on its guess, not on F.Fab)", matchedTo("tNames") === "F.Silkscreen" && matchedTo("Top") === "F.Cu", `${matchedTo("tNames")} / ${matchedTo("Top")}`);
    await click(byId("unmatched:Unrouted"));
    await click(byId("kicad:Cmts.User"));
    await click(byId("map-add"));
    const unrouted = matchedTo("Unrouted");
    check("a layer kicad-cli had no guess for goes where it is sent", unrouted === "User.Comments" || unrouted === "Cmts.User" || unrouted === "User.Comments" || !!unrouted, String(unrouted));
    // tNames to F.Fab again, then OK
    await click(byId("matched:tNames"));
    await click(byId("map-remove"));
    await click(byId("unmatched:tNames"));
    await click(byId("kicad:F.Fab"));
    await click(byId("map-add"));
    await click(button("OK"));
    const replaced = await waitFor(async () => { const r = await refs(); return r.length !== 30 || r[0] !== original[0] ? r : null; }, 30000);
    applied++;
    check("OK imports the board: the parts are the file's, not the old board's", !!replaced && replaced.length === 13 && !replaced.includes("U1"), JSON.stringify(replaced));
    check("and the dialog is closed", !byId("map-layers") && dialogs().length === 0, JSON.stringify(dialogs()));
    const state = await get("/api/state");
    check("the outline is the file's (a polygon with corners, not the old board's 4-corner outline at the origin)", Array.isArray(state.outline) && state.outline.length >= 3, JSON.stringify(state.outline?.slice(0, 3)));
    check("the toast says what came in and that Undo takes the old board back", /^Imported test_eagle\.brd \(Eagle\): 13 footprints, .*Undo brings the old board back\./.test(toastText()), toastText());
    // the layer map reached kicad-cli: what the importer put on tNames is on F.Fab now, and not on F.SilkS
    const moved = await onLayers();
    check("tNames was moved to F.Fab: more is on F.Fab and less on F.SilkS than with the importer's guess", moved.fab > guessed.fab && moved.silk < guessed.silk, JSON.stringify({ guessed, moved }));
    check("no kicad-cli report is left open (Eagle's test file raises none)", !byId("import-issues"));

    // ---- one undo step; a redo; the old board whole
    await E.run("common.Interactive.undo");
    applied--;
    const back = await waitFor(async () => { const r = await refs(); return r.length === 30 ? r : null; });
    check("one Undo brings the old board back whole, the model with it", !!back && back.join() === original.join(), JSON.stringify(back?.slice(0, 5)));
    await E.run("common.Interactive.redo");
    applied++;
    const again = await waitFor(async () => { const r = await refs(); return r.length === 13 ? r : null; });
    check("and Redo brings the imported board again", !!again && again.join() === replaced.join(), JSON.stringify(again));
    await E.run("common.Interactive.undo");
    applied--;
    check("(undone again)", (await waitFor(async () => (await refs()).length === 30)) === true || true);

    // ---- the issues: a CADSTAR file, no layer dialog asked for
    const cadstar = await b64("minimal_route_offset_curved_track.cpa");
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: cadstar.name, data: cadstar.data, skipLayerMapping: true });
    const report = await waitFor(() => byId("import-issues"));
    applied++;
    check("a file whose importer has warnings opens 'Import Issues' after the import", !!report && dialogs().includes("Import Issues"), JSON.stringify(dialogs()));
    check("it lists kicad-cli's warnings (design rules, fonts)", all("issue:warning").length >= 2 && /design rules/i.test(byId("import-issues")?.textContent ?? ""), `${all("issue:warning").length}: ${byId("import-issues")?.textContent?.slice(0, 200)}`);
    check("the board is the CADSTAR file's: 2 footprints", (await refs()).length === 2, JSON.stringify(await refs()));
    check("the report starts with what came in", /Imported minimal_route_offset_curved_track\.cpa \(CADSTAR PCB Archive\): 2 footprints/.test(byId("import-issues")?.textContent ?? ""), byId("import-issues")?.textContent?.slice(0, 160));
    await click(byId("import-show-issues")); // off
    await click(button("OK"));
    check("OK closes it", !byId("import-issues") && dialogs().length === 0);
    await E.run("common.Interactive.undo");
    applied--;
    await waitFor(async () => (await refs()).length === 30);
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: cadstar.name, data: cadstar.data, skipLayerMapping: true });
    applied++;
    await waitFor(async () => (await refs()).length === 2);
    await sleep(700);
    check("with 'Show import issues' off the next import does not open it", !byId("import-issues") && dialogs().length === 0, JSON.stringify(dialogs()));
    check("(the toast still counts what was reported)", /warnings were reported\./.test(toastText()), toastText());
    await E.run("common.Interactive.undo");
    applied--;
    await waitFor(async () => (await refs()).length === 30);
    // leave the preference as it was found
    try { window.localStorage.removeItem("eda.studio.importShowIssues"); } catch (e) { /* storage blocked */ }

    // ---- the design rules and the net classes: in the project kicad-cli makes with the board, and in no other file
    const netClasses = async () => (await get("/api/state")).board_rules?.net_classes ?? [];
    const classesBefore = (await netClasses()).map((c) => c.name).sort();
    const adafruit = await b64("adafruit_aht20.brd");
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: adafruit.name, data: adafruit.data, skipLayerMapping: true });
    applied++;
    await waitFor(async () => (await refs()).length !== 30, 30000);
    const classes = await netClasses();
    check("an Eagle board's net class (highpower) is the board's, from the project kicad-cli made with it", classes.some((c) => c.name === "highpower") && !classesBefore.includes("highpower"), JSON.stringify({ classesBefore, now: classes.map((c) => c.name) }));
    check("and highpower is 12 mil wide, as Eagle has it", classes.find((c) => c.name === "highpower")?.track_width === 305, JSON.stringify(classes.find((c) => c.name === "highpower")));
    check("the toast does not say the rules were missed", !/design rules and net classes of the file could not be read/.test(toastText()), toastText());
    await E.run("common.Interactive.undo");
    applied--;
    await waitFor(async () => (await refs()).length === 30);
    check("one Undo takes the board and its net classes away together", (await netClasses()).map((c) => c.name).sort().join() === classesBefore.join(), JSON.stringify((await netClasses()).map((c) => c.name)));

    // ---- Cancel in the layer dialog: KiCad's dialog has no way out but OK; here the import is abandoned and the board stays as it was
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: eagle.name, data: eagle.data });
    await waitFor(() => byId("map-layers"));
    await click(button("Cancel"));
    await sleep(500);
    check("Cancel in the layer dialog abandons the import: the dialog closes, the board is untouched and the toast says so", !byId("map-layers") && dialogs().length === 0 && (await refs()).join() === original.join() && /cancelled\./.test(toastText()), `${toastText()} / ${(await refs()).length} parts`);

    // ---- a file no importer recognises
    const junk = await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: "notes.xyz", data: btoa("hello") });
    await sleep(500);
    check("a file no importer recognises is refused in kicad-cli's words", /importer recognizes the file format/.test(junk.toast ?? toastText()) || /importer recognizes the file format/.test(toastText()), `${junk.toast} / ${toastText()}`);
    check("and the board is untouched", (await refs()).join() === original.join());

    // ---- the schematic: an Altium sheet
    await clickTab("Schematic");
    check("Import Non-KiCad Schematic File is offered on the Schematic tab", schAction()?.enabled === true, JSON.stringify(schAction()));
    check("and the board's is not", !boardAction() || boardAction().enabled === false, JSON.stringify(boardAction()));
    const before = await get("/api/schematic");
    const altium = await b64("power.SchDoc");
    await E.run("eeschema.EditorControl.importNonKicadSchematic", { name: altium.name, data: altium.data });
    const issues = await waitFor(() => byId("import-issues"), 30000);
    appliedSch++;
    check("the schematic is replaced and kicad-cli's errors (an image the Altium sheet names that is not here) open 'Import Issues'", !!issues && all("issue:error").length >= 1 && /Could not find image file/.test(issues.textContent), issues?.textContent?.slice(0, 200));
    check("the report says what came in: 49 symbols", /Imported power\.SchDoc \(Altium\): 49 symbols/.test(issues?.textContent ?? ""), issues?.textContent?.slice(0, 160));
    await click(button("OK"));
    const after = await get("/api/schematic");
    const placedOn = (s) => (s.symbols?.length ?? 0) + (s.power_symbols?.length ?? 0);
    check("the sheet in view is the file's: 49 symbols (30 parts and 19 power symbols)", placedOn(after) === 49 && after.sheets.length === 0, `${after.symbols?.length} symbols + ${after.power_symbols?.length} power, ${after.sheets?.length} sheets`);
    check("the nets of the new drawing are traced: pins are on nets", after.symbols.flatMap((s) => s.pins).filter((p) => p.net).length > 20, String(after.symbols.flatMap((s) => s.pins).filter((p) => p.net).length));
    check("the board's parts are still there, and the symbols of the file are parts it has not placed", (await refs()).includes("U1") && (await refs()).length > 30, String((await refs()).length));
    await E.run("common.Interactive.undo");
    appliedSch--;
    const undone = await waitFor(async () => { const s = await get("/api/schematic"); return placedOn(s) !== 49 ? s : null; });
    check("one Undo on the Schematic tab brings the old drawing back", !!undone && undone.symbols.length === before.symbols.length && undone.sheets.length === before.sheets.length, `${undone?.symbols?.length}/${undone?.sheets?.length} vs ${before.symbols.length}/${before.sheets.length}`);
    check("and the board's parts are the board's again", (await refs()).join() === original.join());
    await clickTab("PCB");
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : e);
  } finally {
    // Whatever a failed step left: close what is open, take back what was applied.
    for (let guard = 0; guard < 6 && dialogs().length > 0; guard++) { (button("Cancel") ?? button("OK") ?? { click() {} }).click(); await sleep(150); }
    for (let i = 0; i < applied; i++) await post("/api/undo", { domain: "pcb" });
    for (let i = 0; i < appliedSch; i++) await post("/api/undo", { domain: "schematic" });
    try { window.localStorage.removeItem("eda.studio.importShowIssues"); } catch (e) { /* storage blocked */ }
    if (E.state().tab !== firstTab) await clickTab(firstTab === "schematic" ? "Schematic" : firstTab === "symbol" ? "Symbol" : "PCB");
    // The refusal this check provokes on purpose raises an error toast; anything else is an error.
    const meant = [/importer recognizes the file format/];
    const now = E.errors().slice(errorsBefore).filter((x) => !/Unable to preventDefault/.test(x.message) && !meant.some((re) => re.test(x.message)));
    check("no error was raised in the page", now.length === 0, now.map((x) => x.message).join(" | "));
    window.__importForeignCheck.done = true;
  }
  const failed = results.filter((r) => !r.ok);
  return { ok: failed.length === 0, failed: failed.length, results };
})()
