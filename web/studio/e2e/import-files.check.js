// Bringing files in, UI-LEFT I1-I3 (task-u4-import), driven through `window.__eda` and the page's own controls:
//   Append Board (`PCB_CONTROL::AppendBoard`) of a four-layer board with a user layer and a net this board has not got: the board takes the copper layers, the user layer with
//     the file's name and the net, and what came in is one group named after the file, selected, carried by the Move tool and dropped by a click as ONE undo step;
//   Import Graphics' interactive placement (`DRAWING_TOOL::PlaceImportedGraphics`): OK puts nothing on the board -- the items are a preview under the cursor --, Escape drops it,
//     and a click places the items with their top left item on the click's grid point, as ONE undo step, selected;
//   Import Non-KiCad Project... with an Eagle board and its schematic: the board was modified, so "The current PCB has been modified.  Save changes?" is asked with KiCad's
//     buttons (Save, Discard Changes, Cancel); Cancel stops the import; Discard Changes goes on to "Edit Mapping of Imported Layers", where a layer taken back to the
//     Unmatched list is left out with what is on it (`DIALOG_MAP_LAYERS`' `UNDEFINED_LAYER`); OK replaces the board and the schematic together, and ONE Undo brings both back;
//   File > Save clears the flag: the next import does not ask.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml, the UI a copy of web/studio/dist with `e2e-import-files/` holding KiCad's QA Eagle project
// (qa/data/pcbnew/plugins/eagle/Adafruit-AHT20-PCB: "Adafruit AHT20 Temperature & Humidity.brd" and ".sch", here `aht20.brd` and `aht20.sch`). It needs the real kicad-cli.
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }; the results also pile up in `window.__importFilesCheck.results`.
// run-all-checks: board=mcu30 ui=import-files minutes=10
(async () => {
  const results = [];
  window.__importFilesCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const base = window.__importFilesBase ?? "/e2e-import-files/";
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const board = () => get("/api/state");
  const pcbText = () => fetch("/api/board.kicad_pcb", { cache: "no-store" }).then((r) => r.text());
  const waitFor = async (fn, ms = 20000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dialogs = () => E.state().dialogs;
  const button = (label) => [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === label);
  const setNative = (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, buttons = 0) => { const el = document.elementFromPoint(x, y) ?? canvas(); el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons })); };
  const hover = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(150); };
  const clickAt = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointerdown", x, y, 1); fire("pointerup", x, y); await sleep(1500); };
  const key = (k) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  const undoFromPage = async () => { await E.run("common.Interactive.undo"); await sleep(1200); };
  const b64 = async (name) => {
    const bytes = new Uint8Array(await (await fetch(base + name, { cache: "no-store" })).arrayBuffer());
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { name, data: btoa(s), size: bytes.length };
  };
  const C = [30_000_000, 20_000_000];
  const D = [20_000_000, 10_000_000];
  const errorsBefore = E.errors().length;
  let applied = 0;
  let wasStrict = false;

  try {
    if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
    if (!(E.state().view?.scale > 0)) { await E.run("common.Control.zoomFitScreen"); await sleep(300); }
    const st0 = await board();
    const parts0 = st0.parts.length;
    const shapes0 = (st0.drawings?.shapes ?? []).length;
    const groups0 = (st0.drawings?.groups ?? []).length;
    const text0 = await pcbText();
    check("the board is the two-layer mcu30 example, with no User.3 layer and no net NEWNET", parts0 === 30 && !/"In1\.Cu"/.test(text0) && !/"User\.3"/.test(text0) && !text0.includes("NEWNET"), parts0);

    // ============================================================================================================== Append Board: the file's layers, its net, one group
    const other = `(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user) (43 "User.3" front "Notes"))
  (net 0 "") (net 1 "NEWNET")
  (segment (start 15 5) (end 35 5) (width 0.25) (layer "In1.Cu") (net 1) (uuid "b0000000-0000-0000-0000-000000000001"))
  (gr_line (start 15 8) (end 35 8) (stroke (width 0.1) (type solid)) (layer "User.3") (uuid "b0000000-0000-0000-0000-000000000002"))
  (gr_text "APPENDED" (at 25 12 0) (layer "F.SilkS") (uuid "b0000000-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
)`;
    // strict off, as the files-in check has it for Append Board: the appended track lands outside this board's outline, which strict refuses
    const strictBox = () => [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "strict")?.querySelector("input");
    wasStrict = !!strictBox()?.checked;
    if (wasStrict) { strictBox().click(); await sleep(300); }
    await hover(...C);
    void E.run("pcbnew.Control.DdAppendBoard", { name: "other.kicad_pcb", text: other });
    applied++;
    // the file's inner layers and its User.3 are not on this board: KiCad 25ec2335's parser asks where they go (`remapAppendedLayers`); OK with nothing matched takes the
    // suggestions, each layer its own
    const remap = await waitFor(() => byId("map-layers"), 8000);
    check("Append Board of a file whose layers this board has not got asks Edit Mapping of Imported Layers (remapAppendedLayers)", !!remap);
    button("OK")?.click();
    await waitFor(async () => E.state().tool === "move", 8000);
    const carried = E.state().selection;
    check("Append Board: what came in is carried by the Move tool, selected as one group", E.state().tool === "move" && carried.length === 1 && carried[0].kind === "group", JSON.stringify([E.state().tool, carried]));
    await hover(...D);
    await clickAt(...D);
    const st1 = await board();
    const text1 = await pcbText();
    const group = (st1.drawings?.groups ?? []).find((g) => g.name === "other");
    check("the click drops it; the group is named after the file (wxFileName::GetName)", !!group && E.state().tool === "select", JSON.stringify(group));
    check("the board takes the file's four copper layers (In1.Cu and In2.Cu in the board written from the design)", /"In1\.Cu" signal/.test(text1) && /"In2\.Cu" signal/.test(text1), (text1.match(/\(\d+ "In\d\.Cu"[^)]*\)/g) || []).join(" "));
    check("and the file's user layer, with the name and type the file gives it", /\(\d+ "User\.3" front "Notes"\)/.test(text1), (text1.match(/\(\d+ "User\.3"[^)]*\)/g) || []).join(" "));
    check("and the net the board had not got: the track is on NEWNET", text1.includes('"NEWNET"') && (st1.tracks ?? st1.routing?.tracks ?? []).some?.((t) => t.net === "NEWNET") !== false, "NEWNET");
    await undoFromPage();
    applied--;
    const text1b = await pcbText();
    check("ONE undo takes the append back: no inner layers, no User.3, no NEWNET", !/"In1\.Cu"/.test(text1b) && !/"User\.3"/.test(text1b) && !text1b.includes("NEWNET") && ((await board()).drawings?.groups ?? []).length === groups0, "after undo");

    // ============================================================================================================== Import Graphics: the preview, then the click
    const pair = (c, v) => `${c}\n${v}\n`;
    const line = (x1, y1, x2, y2) => pair(0, "LINE") + pair(8, "0") + pair(10, x1) + pair(20, y1) + pair(11, x2) + pair(21, y2);
    const dxf = pair(0, "SECTION") + pair(2, "HEADER") + pair(9, "$INSUNITS") + pair(70, 4) + pair(0, "ENDSEC") + pair(0, "SECTION") + pair(2, "ENTITIES") + line(0, 0, 10, 0) + line(10, 0, 10, 5) + line(10, 5, 0, 5) + line(0, 5, 0, 0) + pair(0, "ENDSEC") + pair(0, "EOF");
    await hover(...C);
    await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "box.dxf", text: dxf });
    const gfx = await waitFor(() => document.querySelector('[data-testid="import-graphics"]'), 5000);
    check("Import Graphics opens with the dropped file", !!gfx);
    const f = (label) => gfx?.querySelector(`[aria-label="${label}"]`);
    if (f("Place at")?.checked) f("Place at").click();
    if (f("Layer")) setNative(f("Layer"), "Dwgs.User");
    if (f("Group imported items") && !f("Group imported items").checked) f("Group imported items").click();
    await sleep(600);
    button("OK").click();
    await sleep(1500);
    const st2 = await board();
    check("interactive placement: OK puts nothing on the board -- the items are a preview under the cursor", (st2.drawings?.shapes ?? []).length === shapes0 && E.state().tool !== "move" && !dialogs().includes("Import Graphics"), `${(st2.drawings?.shapes ?? []).length} shapes, tool ${E.state().tool}`);
    await hover(...D);
    key("Escape");
    await sleep(800);
    check("Escape drops the preview: nothing on the board", ((await board()).drawings?.shapes ?? []).length === shapes0);
    await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "box.dxf", text: dxf });
    await waitFor(() => document.querySelector('[data-testid="import-graphics"]'), 5000);
    await sleep(600);
    button("OK").click();
    await sleep(1500);
    await hover(...D);
    await clickAt(...D);
    applied++;
    const st3 = await board();
    const placed = (st3.drawings?.shapes ?? []).slice(shapes0);
    // the DXF's lines at offset 0: (0, 0)-(10, 0), (10, 0)-(10, -5), (10, -5)-(0, -5), (0, -5)-(0, 0); the top left item is the last, which starts at (0, -5 mm)
    const first = placed.filter((s) => s.kind === "segment").sort((a, b) => a.start[0] - b.start[0] || a.start[1] - b.start[1])[0];
    check("a click places the import: four lines, the top left item's position on the click's point", placed.length === 4 && !!first && first.start[0] === D[0] && first.start[1] === D[1], JSON.stringify(first?.start));
    check("what was placed is selected (the group)", E.state().selection.length === 1 && E.state().selection[0].kind === "group", JSON.stringify(E.state().selection));
    // ============================================================================================================== Import Non-KiCad Project: Save changes?, the layer map, both editors
    const schBefore = JSON.stringify(((await get("/api/schematic")).symbols ?? []).map((x) => x.id).sort());
    const brd = await b64("aht20.brd");
    const sch = await b64("aht20.sch");
    check("the QA Eagle project is there", brd.size > 10000 && sch.size > 10000, [brd.size, sch.size]);
    const project = E.actions({ all: true }).find((a) => a.id === "studio.Import.nonKicadProject");
    check("Import Non-KiCad Project... is offered on the PCB tab", project?.enabled === true, JSON.stringify(project));
    await E.run("studio.Import.nonKicadProject", [brd, sch]);
    const q = await waitFor(() => byId("import-question"), 8000);
    const qText = q?.innerText ?? "";
    check("the board was modified (the import just placed): 'The current PCB has been modified.  Save changes?' is asked first", !!q && qText.includes("The current PCB has been modified.") && qText.includes("Save changes?") && qText.includes("If you don't save, all your changes will be permanently lost."), qText);
    check("with KiCad's buttons: Save, Discard Changes, Cancel", !!byId("import-question:Save") && !!byId("import-question:Discard Changes") && !!byId("import-question:Cancel"));
    byId("import-question:Cancel").click();
    await sleep(1200);
    check("Cancel stops the import: the board is as it was", (await board()).parts.length === parts0 && !byId("map-layers"));
    await E.run("studio.Import.nonKicadProject", [brd, sch]);
    await waitFor(() => byId("import-question"), 8000);
    byId("import-question:Discard Changes").click();
    const map = await waitFor(() => byId("map-layers"), 30000);
    check("Discard Changes goes on: kicad-cli converts the board with its schematic and 'Edit Mapping of Imported Layers' opens", !!map);
    // tDocu (the packages' drawings, F.Fab by the guess) back to the Unmatched list: left out, with what is on it
    byId("matched:tDocu")?.click();
    await sleep(80);
    byId("map-remove")?.click();
    await sleep(200);
    check("the layer taken back is unmatched, and the dialog says it is left out", !!byId("unmatched:tDocu") && /not imported/.test(byId("map-kept")?.textContent ?? ""), byId("map-kept")?.textContent);
    button("OK").click();
    applied++;
    const imported = await waitFor(async () => { const s = await board(); return s.parts.length !== parts0 ? s : null; }, 60000);
    check("OK replaces the board with the Eagle board", !!imported && imported.parts.length > 5 && !imported.parts.some((p) => p.ref === "U1" && p.footprint === "QFN-48"), imported?.parts.length);
    const fab = (t) => (t.match(/\(layer "F\.Fab"\)/g) || []).length;
    const fabDropped = fab(await pcbText());
    const schState = await get("/api/schematic");
    const symbols = (schState.symbols ?? []).length + Object.values(schState.sheets ?? {}).length;
    check("and replaces the schematic with the project's (its symbols are on the sheet)", (schState.symbols ?? []).length > 10, symbols);
    await undoFromPage();
    applied--;
    check("ONE undo from the PCB tab brings both back: the old board's 30 parts", (await board()).parts.length === parts0, (await board()).parts.length);
    const schAfter = JSON.stringify(((await get("/api/schematic")).symbols ?? []).map((x) => x.id).sort());
    check("and the old schematic's symbols", schAfter === schBefore, `${schAfter.slice(0, 80)} / ${schBefore.slice(0, 80)}`);
    // File > Save clears the flag: the next import asks nothing
    await E.run("common.Control.save");
    await sleep(300);
    await E.run("pcbnew.EditorControl.openNonKicadBoard", { name: brd.name, data: brd.data, skipLayerMapping: true, skipReport: true });
    await sleep(1500);
    check("after File > Save the next import asks nothing", !byId("import-question"), byId("import-question")?.innerText ?? "");
    const again = await waitFor(async () => { const s = await board(); return s.parts.length !== parts0 ? s : null; }, 60000);
    if (again) {
      // the same board with the importer's guesses: tDocu's drawings are on F.Fab there, and not in the import that left tDocu out
      const fabGuess = fab(await pcbText());
      check("nothing of tDocu came in: the board with tDocu left out has fewer F.Fab items than the importer's guess gives", fabDropped < fabGuess, `${fabDropped} < ${fabGuess}`);
      applied++;
      await undoFromPage();
      applied--;
    }
  } catch (e) {
    check("the check ran to the end", false, e?.stack ?? String(e));
  } finally {
    for (let i = 0; i < applied; i++) await undoFromPage();
    if (wasStrict && !document.querySelector("label input")?.checked) { const box = [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "strict")?.querySelector("input"); if (box && !box.checked) box.click(); }
    const errs = E.errors().slice(errorsBefore).filter((x) => !/Unable to preventDefault/.test(x.message ?? String(x)));
    check("no unexpected page errors", errs.length === 0, errs.map((x) => x.message ?? String(x)).join(" | "));
  }
  const failed = results.filter((r) => !r.ok);
  window.__importFilesCheck.done = true;
  return { ok: failed.length === 0, failed: failed.length, results };
})();
