// Bringing files in, round 2 (UI-LEFT-2 II3, II4, II5; task-v10-import), driven through `window.__eda`, the page's own controls and a real drop event:
//   Append Board of a file whose layers do not fit the board (KiCad 25ec2335 `remapAppendedLayers`): "Edit Mapping of Imported Layers" opens with every layer unmatched and
//     the KiCad layers the rows permit (the copper of the larger stack, each other layer itself); In1.Cu matched to B.Cu puts the file's track on B.Cu; the file's locked
//     track stays locked (`CLIPBOARD_IO::Parse` keeps `(locked yes)`) and its group is locked;
//   two boards dropped together are appended one after the other (`DoWithAcceptedFiles`): the second is in the hand once the first is dropped;
//   the schematic's Import Graphics placed "At" a point keeps a DXF text's glyph width (`SetTextWidth`: 0.9 of its height).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (two copper layers, no inner ones).
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }; the results also pile up in `window.__importV10Check.results`.
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  window.__importV10Check = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const board = () => get("/api/state");
  const pcbText = () => fetch("/api/board.kicad_pcb", { cache: "no-store" }).then((r) => r.text());
  const waitFor = async (fn, ms = 20000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
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
  const undo = async (domain) => { await fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain }) }); await sleep(700); };
  const C = [30_000_000, 20_000_000];
  const D = [20_000_000, 10_000_000];
  const errorsBefore = E.errors().length;
  let wasStrict = false;
  const strictBox = () => [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "strict")?.querySelector("input");

  try {
    if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
    if (!(E.state().view?.scale > 0)) { await E.run("common.Control.zoomFitScreen"); await sleep(300); }
    const text0 = await pcbText();
    const groups0 = ((await board()).drawings?.groups ?? []).length;
    check("the board has two copper layers", !/"In1\.Cu"/.test(text0), "");
    // strict off: the appended items land outside this board's outline, which strict refuses at the drop
    wasStrict = !!strictBox()?.checked;
    if (wasStrict) { strictBox().click(); await sleep(300); }

    // ============================================================================================================== Append Board: the layer dialog, the locks
    const four = `(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6))
  (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "INNER")
  (segment (start 15 5) (end 35 5) (width 0.25) (layer "In1.Cu") (net 1) (locked yes) (uuid "c0000000-0000-0000-0000-000000000001"))
  (segment (start 15 9) (end 35 9) (width 0.25) (layer "F.Cu") (net 1) (uuid "c0000000-0000-0000-0000-000000000002"))
)`;
    await hover(...C);
    void E.run("pcbnew.Control.DdAppendBoard", { name: "four.kicad_pcb", text: four });
    const dlg = await waitFor(() => byId("map-layers"), 8000);
    check("a file whose layers do not fit the board opens Edit Mapping of Imported Layers (remapAppendedLayers)", !!dlg);
    const unmatched = [...(byId("unmatched-layers")?.querySelectorAll('[role="option"]') ?? [])].map((r) => r.textContent.trim());
    check("it opens with every layer of the file unmatched, as DIALOG_MAP_LAYERS does", ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu", "Edge.Cuts"].every((n) => unmatched.includes(n)) && (byId("matched-layers")?.querySelectorAll('[role="option"]').length ?? 0) === 0, JSON.stringify(unmatched));
    const kicad = [...(byId("kicad-layers")?.querySelectorAll('[role="option"]') ?? [])].map((r) => r.getAttribute("data-testid").replace("kicad:", ""));
    check("the KiCad layers are the union of the rows' PermittedLayers: the four copper layers of the larger stack and Edge.Cuts, nothing else", JSON.stringify(kicad) === JSON.stringify(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu", "Edge.Cuts"]), JSON.stringify(kicad));
    byId("unmatched:In1.Cu")?.click();
    byId("kicad:B.Cu")?.click();
    await sleep(100);
    byId("map-add")?.click();
    await sleep(200);
    check("> matches In1.Cu to B.Cu", (byId("matched-to:In1.Cu")?.textContent ?? "").trim() === "B.Cu", byId("matched-to:In1.Cu")?.textContent);
    button("OK")?.click();
    await waitFor(async () => E.state().tool === "move", 8000);
    check("OK goes on with the append: the Move tool carries what came in", E.state().tool === "move", E.state().tool);
    await hover(...D);
    await clickAt(...D);
    const st1 = await board();
    const tracks1 = st1.routing?.tracks ?? [];
    const moved = tracks1.find((t) => t.net === "INNER" && t.layer === "B.Cu");
    check("the track of In1.Cu is on B.Cu, the layer it was matched to", !!moved, JSON.stringify(tracks1.filter((t) => t.net === "INNER").map((t) => [t.layer, t.id])));
    const locked = new Set(st1.locked ?? []);
    const other = tracks1.find((t) => t.net === "INNER" && t.layer === "F.Cu");
    check("the file's locked track stays locked, the other is not (CLIPBOARD_IO::Parse keeps `(locked yes)`)", !!moved && locked.has(moved.id) && !!other && !locked.has(other.id), JSON.stringify([...locked]));
    const group = (st1.drawings?.groups ?? []).find((g) => g.name === "four");
    check("and the group of a locked item is locked (`if( IsLocked() ) group->SetLocked( true )`)", !!group && locked.has(group.id), JSON.stringify(group));
    await undo("pcb");
    check("one undo takes the append back", ((await board()).drawings?.groups ?? []).length === groups0 && !/"In1\.Cu"/.test(await pcbText()));

    // ============================================================================================================== two boards dropped together
    const small = (name, y) => `(kicad_pcb (version 20241229) (generator "pcbnew")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal))
  (net 0 "")
  (gr_text "${name}" (at 15 ${y} 0) (layer "F.Cu") (uuid "d0000000-0000-0000-0000-00000000000${y}") (effects (font (size 1 1) (thickness 0.15))))
  (gr_text "${name}2" (at 25 ${y} 0) (layer "F.Cu") (uuid "d0000000-0000-0000-0000-00000000010${y}") (effects (font (size 1 1) (thickness 0.15))))
)`;
    const dt = new DataTransfer();
    dt.items.add(new File([small("FIRST", 1)], "first.kicad_pcb"));
    dt.items.add(new File([small("SECOND", 2)], "second.kicad_pcb"));
    await hover(...C);
    const [cx, cy] = w2c(...C);
    for (const type of ["dragenter", "dragover", "drop"]) window.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true, clientX: cx, clientY: cy }));
    await waitFor(async () => E.state().tool === "move", 8000);
    check("two boards dropped: the first is appended and in the hand", E.state().tool === "move", E.state().tool);
    await hover(...D);
    await clickAt(...D);
    const second = await waitFor(async () => E.state().tool === "move", 10000);
    check("once it is dropped, the second is appended and in the hand (DoWithAcceptedFiles runs each in turn)", !!second, E.state().tool);
    await hover(C[0] + 5_000_000, C[1] + 5_000_000);
    await clickAt(C[0] + 5_000_000, C[1] + 5_000_000);
    const st2 = await board();
    const names = (st2.drawings?.groups ?? []).map((g) => g.name);
    check("both boards came in, each its own group", names.includes("first") && names.includes("second"), JSON.stringify(names));
    await undo("pcb");
    await undo("pcb");

    // ============================================================================================================== the schematic's DXF text keeps its glyph width
    [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "Schematic")?.click();
    await sleep(1200);
    const pair = (c, v) => `${c}\n${v}\n`;
    const dxf = pair(0, "SECTION") + pair(2, "ENTITIES") + pair(0, "TEXT") + pair(8, "0") + pair(10, 10) + pair(20, 5) + pair(40, 2.5) + pair(1, "WIDE") + pair(0, "ENDSEC") + pair(0, "EOF");
    await E.run("eeschema.EditorControl.ddImportGraphics", { name: "t.dxf", text: dxf });
    const root = await waitFor(() => document.querySelector('[data-testid="import-graphics"]'), 5000);
    const field = (label) => root?.querySelector(`[aria-label="${label}"]`);
    field("At")?.click();
    await sleep(50);
    if (field("Default units")) setNative(field("Default units"), "mm");
    await sleep(300);
    button("OK")?.click();
    await sleep(1500);
    const sch = await get("/api/schematic");
    const t = (sch.texts ?? []).find((x) => x.content === "WIDE");
    check("the DXF text keeps its glyph width: 0.9 of its 2.5 mm height (`DXF_IMPORT_PLUGIN::addText`, `SetTextWidth`)", !!t && t.size_nm === 2_500_000 && t.width_nm === 2_250_000, JSON.stringify(t && { size: t.size_nm, width: t.width_nm }));
    await undo("schematic");

    check("no page error", E.errors().length === errorsBefore, JSON.stringify(E.errors().slice(errorsBefore)));
  } catch (e) {
    check("the check ran to the end", false, (e && e.stack) || e);
  } finally {
    if (wasStrict && strictBox() && !strictBox().checked) strictBox().click();
  }
  const failed = results.filter((r) => !r.ok).length;
  window.__importV10Check.done = true;
  return { ok: failed === 0, failed, results };
})();
