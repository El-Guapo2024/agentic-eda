// Files in (GAPS.md item 28, package P8): Import Graphics (DXF and SVG), Append Board and the drops, driven through `window.__eda` and the page's own controls.
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board (it imports and appends
// things, and takes every one of them back with Undo; the pointer only moves inside the page: dispatched events, never the real one), on a machine where kicad-cli is NOT needed.
//
// What it checks:
//   Import Graphics, the board: the actions are wired and open the dialog; a file dropped on it (the drop action's parameter is `{ name, text }`: a browser gives the page a file's
//     text, not its path) is read by the server and described ("8 items: 4 lines, 4 arcs", what it could not handle); the dialog's defaults are KiCad's; OK with "Place at" puts the
//     items on the layer asked for, at the origin over the scale (KiCad's own arithmetic), with its default line width scaled, grouped, selected, as ONE undo step; interactive
//     placement puts the picture's upper left corner at the cursor and arms the Move tool, Escape takes it back; the DXF unit of the dialog is used when the file names none and the
//     file's own wins; "Fix discontinuities" joins corners that miss; an outline imported on Edge.Cuts replaces the polygon of the board's intent; a file with nothing to import, a
//     scale of zero and a file type no plugin handles are refused with KiCad's words;
//   Import Graphics, the schematic and the Symbol Editor (DIALOG_IMPORT_GFX_SCH): the actions and the File > Import entry are wired on both tabs; the dialog has "Interactive placement" / "At" X, Y, the
//     scale, the DXF width and units and no layer, group or discontinuities; OK with "At" puts the lines (graphic lines of the sheet, shapes for the arcs) at the origin over the scale with the
//     sheet's default width, as ONE undo step of the schematic's own history; interactive placement puts the drawing's origin at the cursor's grid point and arms the Move tool, Escape takes it
//     back; an SVG becomes polygons; in the Symbol Editor the items are polylines and arcs of the open symbol in mm with the Y axis up, on the unit being edited, as one undo step of that editor;
//   Append Board (PCB_CONTROL::AppendBoard): the action, its drop and the File menu entry are wired; a dropped `.kicad_pcb` is read by the server (what is in it and the point it is held by:
//     its top left item), its items are pasted with that item at the cursor's grid point, selected and carried by the Move tool as ONE undo step (Escape takes them back, a click drops them where
//     the pointer went), the footprint it brings is a part of this board and the copper keeps its place relative to the line; a file that is not a KiCad board is refused in KiCad's words;
//   The file drops (EDA_BASE_FRAME::OnDropFiles, FileDropHost.tsx): a file dropped on the window (a DataTransfer with the File, the events a browser sends) is kept from the browser's own use of
//     it (dragover and drop are default-prevented) and runs the action its editor's accepted extensions name -- board: .dxf opens Import Graphics with the file, .kicad_pcb is appended; Footprint
//     Editor: .kicad_mod is imported and opened; schematic: .dxf opens its Import Graphics, a .kicad_sch is said not to be openable by a drop; Symbol Editor: nothing is accepted. A drop of several
//     files opens the first and says the rest were skipped; one the editor does not take is said not to be a file it opens; while Import Graphics is open a graphic dropped outside it replaces
//     the file in it and anything else is ignored.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }. The results also pile up in `window.__filesInCheck.results` (`.done` is set at the end).
(async () => {
  const results = [];
  window.__filesInCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const board = () => get("/api/state");
  const undo = async (n = 1) => { for (let i = 0; i < n; i++) await post("/api/undo", { domain: "pcb" }); await sleep(300); };
  const setNative = (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogs = () => E.state().dialogs;
  const root = () => document.querySelector('[data-testid="import-graphics"]');
  const field = (label) => root()?.querySelector(`[aria-label="${label}"]`);
  const button = (label) => [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === label);
  const waitFor = async (fn, ms = 5000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(60); } };
  const errorsBefore = E.errors().length;

  // ---- the pointer: dispatched events, as the other checks do
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, buttons = 0) => { const el = document.elementFromPoint(x, y) ?? canvas(); el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons })); };
  const hover = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(150); };
  const key = (k) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  /** Two places for the cursor, in the part of the board the fitted view shows (x 0 to 35 mm, y 0 to 35 mm and a little past): the first is where things are carried to, the second where a click drops them. */
  const C = [30000, 20000];
  const D = [20000, 10000];

  // ---- the DXFs: an ASCII DXF is pairs of lines
  const pair = (c, v) => `${c}\n${v}\n`;
  const dxf = (entities, units = 4) => pair(0, "SECTION") + pair(2, "HEADER") + (units == null ? "" : pair(9, "$INSUNITS") + pair(70, units)) + pair(0, "ENDSEC") + pair(0, "SECTION") + pair(2, "ENTITIES") + entities + pair(0, "ENDSEC") + pair(0, "EOF");
  const line = (x1, y1, x2, y2) => pair(0, "LINE") + pair(8, "OUTLINE") + pair(10, x1) + pair(20, y1) + pair(11, x2) + pair(21, y2);
  const arc = (cx, cy, r, a1, a2) => pair(0, "ARC") + pair(8, "OUTLINE") + pair(10, cx) + pair(20, cy) + pair(40, r) + pair(50, a1) + pair(51, a2);
  /** A 40 x 30 mm board with 3 mm corners, as the LINEs and ARCs a mechanical CAD writes. */
  const boardDxf = (() => {
    const r = 3, W = 40, H = 30;
    return dxf(line(r, 0, W - r, 0) + arc(W - r, r, r, 270, 360) + line(W, r, W, H - r) + arc(W - r, H - r, r, 0, 90) + line(W - r, H, r, H) + arc(r, H - r, r, 90, 180) + line(0, H - r, 0, r) + arc(r, r, r, 180, 270));
  })();
  const rect2x1 = (units) => dxf(line(0, 0, 2, 0) + line(2, 0, 2, 1) + line(2, 1, 0, 1) + line(0, 1, 0, 0), units);
  /** A 10 mm square whose corners miss by 0.3 mm. */
  const gappy = dxf([[0, 0, 9.7, 0], [10, 0.3, 10, 9.7], [9.7, 10, 0.3, 10], [0, 9.7, 0, 0.3]].map((l) => line(...l)).join(""));

  /** A 40 x 30 mm rounded rect drawn with a 0.1 mm stroke, the way a vector drawing tool writes it, and a small filled logo with a hole. */
  const svgRect = `<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="30mm" viewBox="0 0 40 30"><rect x="0" y="0" width="40" height="30" rx="3" fill="none" stroke="#000" stroke-width="0.1"/></svg>`;
  const svgLogo = `<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 h20 v20 h-20 z M5 5 v10 h10 v-10 z" fill="#333" fill-rule="evenodd"/><text x="1" y="1">x</text></svg>`;

  /** Another board: a part ZZ1 (two pads, one on GND), a track on GND, a line on Dwgs.User and a text. The line's start (15, 5) is its top left item. */
  const otherBoard = `(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user) (5 "F.SilkS" user) (7 "F.Fab" user) (40 "Dwgs.User" user))
  (net 0 "")
  (net 1 "GND")
  (footprint "Lib:R0603" (layer "F.Cu") (uuid "a0000000-0000-0000-0000-000000000001") (at 30 20)
    (property "Reference" "ZZ1" (at 0 -1 0) (layer "F.SilkS") (uuid "a0000000-0000-0000-0000-000000000002") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "10k" (at 0 1 0) (layer "F.Fab") (uuid "a0000000-0000-0000-0000-000000000003") (effects (font (size 1 1) (thickness 0.15))))
    (pad "1" smd rect (at -0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (net 1 "GND") (uuid "a0000000-0000-0000-0000-000000000004"))
    (pad "2" smd rect (at 0.8 0) (size 0.8 0.9) (layers "F.Cu" "F.Mask" "F.Paste") (uuid "a0000000-0000-0000-0000-000000000005")))
  (segment (start 20 10) (end 40 10) (width 0.25) (layer "F.Cu") (net 1) (uuid "a0000000-0000-0000-0000-000000000006"))
  (gr_line (start 15 5) (end 45 5) (stroke (width 0.1) (type solid)) (layer "Dwgs.User") (uuid "a0000000-0000-0000-0000-000000000007"))
  (gr_text "ZZ" (at 25 30 0) (layer "F.SilkS") (uuid "a0000000-0000-0000-0000-000000000008") (effects (font (size 1 1) (thickness 0.15))))
)`;

  /** The extent of the shapes on the board: [minx, miny, maxx, maxy] in um. */
  const extent = (shapes) => {
    const pts = shapes.flatMap((s) => s.kind === "arc" ? [s.start, s.mid, s.end] : s.kind === "circle" ? [s.center] : s.kind === "polygon" ? s.pts : [s.start, s.end]);
    return [Math.min(...pts.map((p) => p[0])), Math.min(...pts.map((p) => p[1])), Math.max(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[1]))];
  };

  /** Drop a file on the board through the drop action, set what the dialog is set to, press OK. Returns what the board has afterwards. */
  const importFile = async (file, set = {}, { ok = true } = {}) => {
    const res = await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", file);
    await waitFor(() => root());
    await sleep(500); // what the server says about the file
    const f = (l) => field(l);
    // The dialog remembers its last settings: put every one where this run wants it.
    const want = { Layer: "Dwgs.User", "Import scale": "1.0", "DXF default units": "mm", "Fix discontinuities": false, "Group imported items": true, "Place at": false, "Place at X": "0", "Place at Y": "0", "Use as the board outline": true, ...set };
    for (const [label, v] of Object.entries(want)) {
      const el = f(label);
      if (!el) continue;
      if (el.type === "checkbox") { if (el.checked !== v) el.click(); } else setNative(el, v);
      await sleep(30);
    }
    await sleep(500);
    const info = root()?.querySelector('[aria-label="File contents"]')?.innerText ?? "";
    if (ok) { button("OK").click(); await sleep(1500); }
    return { res, info, st: await board() };
  };
  const added = (st) => (st.drawings?.shapes ?? []);

  let applied = 0;
  let appliedSch = 0;
  let appliedSym = 0;
  const firstTab = E.state().tab;
  try {
    // A pane that is not on screen never fits the board to its canvas (no animation frames run): do it, so the pointer has a view to work in.
    if (!(E.state().view?.scale > 0)) { await E.run("common.Control.zoomFitScreen"); await sleep(300); }
    const st0 = await board();
    const base = { shapes: added(st0).length, groups: (st0.drawings?.groups ?? []).length, outline: JSON.stringify(st0.outline), isShapes: !!st0.outline_is_shapes };

    // ============================================================================================================================ the actions
    const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("Import Graphics and its drop are wired on the PCB tab", acts["pcbnew.InteractiveDrawing.placeImportedGraphics"]?.enabled && acts["pcbnew.InteractiveDrawing.ddImportGraphics"]?.enabled);
    // File > Import > Graphics...: the menu's own entry (`menus.json`), enabled, and it opens the same dialog.
    const fromMenu = async (menu, label) => {
      const find = () => [...document.querySelectorAll('.menubar-dropdown [role="menuitem"]')].find((e) => e.textContent.trim().startsWith(label));
      const bar = [...document.querySelectorAll(".menubar-item")].find((e) => e.childNodes[0]?.textContent?.trim() === menu);
      for (let i = 0; i < 2 && !find(); i++) { bar?.click(); await sleep(250); } // a click toggles: a menu left open is closed by the first
      return find();
    };
    const entry = await fromMenu("File", "Import Graphics");
    check("File > Import has Import Graphics... and it is enabled", !!entry && entry.getAttribute("aria-disabled") === "false", entry?.outerHTML?.slice(0, 120));
    entry?.click();
    await sleep(300);
    check("clicking the menu entry opens Import Graphics", dialogs().includes("Import Graphics"), JSON.stringify(dialogs()));
    button("Cancel")?.click();
    await sleep(200);
    const opened = await E.run("pcbnew.InteractiveDrawing.placeImportedGraphics");
    await waitFor(() => root());
    check("the action opens the dialog with nothing in File: and OK waiting for a file", opened.dialog === "Import Graphics" && dialogs().includes("Import Graphics") && field("File").value === "" && button("OK").disabled, JSON.stringify(opened));
    button("Cancel").click();
    await sleep(200);
    check("Cancel closes it", dialogs().length === 0);

    // a file no plugin handles
    const refused = await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "notes.txt", text: "hello" });
    check("a dropped file no plugin handles is refused with KiCad's words and opens nothing", refused.toast === "There is no plugin to handle this file type." && dialogs().length === 0, JSON.stringify(refused));

    // ============================================================================================================================ the dialog
    const dropped = await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "board.dxf", text: boardDxf });
    await waitFor(() => root());
    await sleep(600);
    check("a dropped DXF is in File: and the server says what it holds", field("File").value === "board.dxf" && /8 items: 4 lines, 4 arcs/.test(root().innerText), root()?.innerText.slice(0, 120));
    check("OK is waiting now", !button("OK").disabled);
    check("the line width and the unit are the DXF's", !field("DXF default line width").disabled && !field("DXF default units").disabled);
    const labels = [...field("DXF default units").options].map((o) => o.textContent);
    check("the DXF units are KiCad's list", JSON.stringify(labels) === JSON.stringify(["Inches", "Feet", "Millimeters", "Centimeter", "Mils"]), labels.join(","));
    // (The dialog keeps its last settings while the page lives, as KiCad's keeps them between runs: the defaults are checked only on a page that has not used it.)
    const remembered = field("Place at").checked || field("Layer").value !== "Dwgs.User" || field("Import scale").value !== "1.0";
    check("the options start as KiCad's: scale 1.0, line width 0.2, group, fix with 1 mm, interactive placement, the layer box checked", remembered || (field("Import scale").value === "1.0" && field("DXF default line width").value === "0.2" && field("Group imported items").checked && field("Fix discontinuities").checked && field("Tolerance").value === "1" && !field("Place at").checked && field("Set layer").checked), remembered ? "remembered from an earlier use of the dialog" : "");
    check("the layer list has the board's copper and the technical layers", ["F.Cu", "B.Cu", "Edge.Cuts", "Dwgs.User", "User.9"].every((l) => [...field("Layer").options].some((o) => o.value === l)));
    check("the place-at fields follow the Place at box and the layer box follows its checkbox", field("Place at X").disabled === !field("Place at").checked && field("Place at Y").disabled === !field("Place at").checked && field("Layer").disabled === !field("Set layer").checked);
    // a scale of zero
    setNative(field("Import scale"), "0");
    await sleep(100);
    button("OK").click();
    await sleep(300);
    check("a scale of zero is refused in the dialog's words", /scale/i.test(root()?.querySelector('[role="alert"]')?.textContent ?? "") && added(await board()).length === base.shapes);
    button("Cancel").click();
    await sleep(200);

    // ============================================================================================================================ Place at, the layer, the group, one undo step
    let r = await importFile({ name: "board.dxf", text: boardDxf }, { Layer: "Edge.Cuts", "Place at": true, "Fix discontinuities": true, "Group imported items": true });
    applied++;
    let sh = added(r.st);
    check("OK puts the 4 lines and 4 arcs on the layer asked for", sh.length === base.shapes + 8 && sh.filter((s) => s.layer === "Edge.Cuts").length === 8 && sh.filter((s) => s.kind === "segment").length === 4 && sh.filter((s) => s.kind === "arc").length === 4, JSON.stringify(sh.map((s) => s.kind + ":" + s.layer)));
    check("the dialog closed and the items are selected as the group they are in", dialogs().length === 0 && E.state().selection.length === 1 && E.state().selection[0].kind === "group", JSON.stringify(E.state().selection));
    const [x0, y0, x1, y1] = extent(sh);
    check("at the origin: x 0 to 40 mm, y -30 to 0 (a DXF's Y axis points up, the board's down)", x0 === 0 && x1 === 40000 && y0 === -30000 && y1 === 0, [x0, y0, x1, y1]);
    check("each item has the default line width of 0.2 mm", sh.every((s) => s.stroke_width === 200), sh.map((s) => s.stroke_width).join(","));
    check("the items are one group of 8", (r.st.drawings.groups ?? []).length === base.groups + 1 && r.st.drawings.groups.at(-1).member_ids.length === 8);
    check("an outline imported on Edge.Cuts replaced the polygon of the board's intent: the shapes are the outline", r.st.outline_is_shapes === true && Math.min(...r.st.outline.map((p) => p[0])) === 0 && Math.max(...r.st.outline.map((p) => p[0])) === 40000 && Math.min(...r.st.outline.map((p) => p[1])) === -30000, JSON.stringify(r.st.outline.slice(0, 3)));
    await undo();
    applied--;
    const back = await board();
    check("ONE undo takes the whole import back: no shapes, no group, the polygon outline again", added(back).length === base.shapes && (back.drawings?.groups ?? []).length === base.groups && JSON.stringify(back.outline) === base.outline && !!back.outline_is_shapes === base.isShapes, `${added(back).length} shapes, ${JSON.stringify(back.outline)}`);

    // ============================================================================================================================ the scale and the origin (KiCad's arithmetic)
    r = await importFile({ name: "board.dxf", text: boardDxf }, { Layer: "Dwgs.User", "Import scale": "2", "Place at": true, "Place at X": "10", "Place at Y": "5" });
    applied++;
    sh = added(r.st);
    const [sx0, sy0, sx1, sy1] = extent(sh);
    // x: 0..40 doubled is 0..80, then the offset 10 mm / 2 = 5 mm; y: -30..0 doubled is -60..0, then 5 mm / 2 = 2.5 mm: KiCad divides the origin it is given by the scale.
    check("scale 2 doubles the picture; the origin typed is divided by the scale, as KiCad does", sx0 === 5000 && sx1 === 85000 && sy0 === -57500 && sy1 === 2500, [sx0, sy0, sx1, sy1]);
    check("the default line width scales too (0.2 mm becomes 0.4 mm)", sh.every((s) => s.stroke_width === 400), sh.map((s) => s.stroke_width).join(","));
    await undo();
    applied--;

    // ============================================================================================================================ interactive placement
    await hover(...C);
    r = await importFile({ name: "board.dxf", text: boardDxf }, { Layer: "Dwgs.User", "Place at": false, "Group imported items": true });
    applied++;
    sh = added(r.st);
    const [ix0, iy0] = extent(sh);
    check("interactive placement: the picture's upper left corner is at the cursor's grid point", ix0 === C[0] && iy0 === C[1], [ix0, iy0]);
    check("and the Move tool carries the group", E.state().tool === "move" && E.state().selection.length === 1 && E.state().selection[0].kind === "group", JSON.stringify(E.state()));
    key("Escape");
    await sleep(1000);
    const gone = await board();
    check("Escape takes the placement back, one undo step", added(gone).length === base.shapes && E.state().tool === "select", `${added(gone).length} shapes, tool ${E.state().tool}`);
    applied--;

    // ============================================================================================================================ the DXF's units
    r = await importFile({ name: "box.dxf", text: rect2x1(null) }, { "DXF default units": "inch", "Place at": true });
    applied++;
    let [ux0, , ux1] = extent(added(r.st));
    check("a DXF with no unit takes the dialog's (inches: 2 x 1 in is 50.8 x 25.4 mm)", ux1 - ux0 === 50800, [ux0, ux1]);
    await undo();
    applied--;
    r = await importFile({ name: "box.dxf", text: rect2x1(4) }, { "DXF default units": "inch", "Place at": true });
    applied++;
    [ux0, , ux1] = extent(added(r.st));
    check("a DXF that names its unit ($INSUNITS 4, millimetres) keeps it whatever the dialog says", ux1 - ux0 === 2000, [ux0, ux1]);
    await undo();
    applied--;

    // ============================================================================================================================ Fix discontinuities
    r = await importFile({ name: "gappy.dxf", text: gappy }, { Layer: "Edge.Cuts", "Place at": true, "Fix discontinuities": false, "Group imported items": false, "Use as the board outline": false });
    applied++;
    const open = added(r.st);
    const joined = (shapes) => shapes.every((s, i) => { const n = shapes[(i + 1) % shapes.length]; return s.end[0] === n.start[0] && s.end[1] === n.start[1]; });
    check("without the fix the corners miss by 0.3 mm", open.length === base.shapes + 4 && !joined(open.slice(-4)), JSON.stringify(open.slice(-4).map((s) => [s.start, s.end])));
    await undo();
    applied--;
    r = await importFile({ name: "gappy.dxf", text: gappy }, { Layer: "Edge.Cuts", "Place at": true, "Fix discontinuities": true, Tolerance: "1", "Group imported items": false, "Use as the board outline": false });
    applied++;
    check("with it (tolerance 1 mm) each side ends where the next begins", joined(added(r.st).slice(-4)), JSON.stringify(added(r.st).slice(-4).map((s) => [s.start, s.end])));
    await undo();
    applied--;


    // ============================================================================================================================ SVG
    r = await importFile({ name: "outline.svg", text: svgRect }, { Layer: "Dwgs.User", "Place at": true, "Group imported items": true }, { ok: false });
    check("an SVG is read by the same dialog: its contents are described", /1 item: 1 polygon/.test(r.info), r.info);
    check("the DXF's line width and unit have no say for an SVG: those controls are off", field("DXF default line width").disabled && field("DXF default units").disabled);
    button("OK").click();
    await sleep(1500);
    applied++;
    sh = added(await board());
    const poly = sh.find((s) => s.kind === "polygon");
    check("a stroked rounded rect is one unfilled polygon with the stroke's own width (0.1 mm), not the DXF default", !!poly && !poly.filled && poly.stroke_width === 100, JSON.stringify(poly && { kind: poly.kind, w: poly.stroke_width, filled: poly.filled }));
    const [vx0, vy0, vx1, vy1] = extent([poly]);
    check("at the origin, 40 x 30 mm, y down as the SVG has it", vx0 === 0 && vy0 === 0 && vx1 === 40000 && vy1 === 30000, [vx0, vy0, vx1, vy1]);
    await undo();
    applied--;
    r = await importFile({ name: "logo.svg", text: svgLogo }, { Layer: "F.SilkS", "Place at": true, "Group imported items": false });
    applied++;
    const logo = added(r.st).filter((s) => s.kind === "polygon");
    check("a filled shape with a hole is one filled polygon of no width, and the SVG's text is said not to be imported", logo.length === 1 && logo[0].filled && logo[0].stroke_width === 0 && /text elements are not currently supported/.test(r.info), `${logo.length} ${r.info}`);
    await undo();
    applied--;

    // ============================================================================================================================ refusals
    const noItems = await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "dims.dxf", text: dxf(pair(0, "DIMENSION") + pair(8, "0")) });
    await waitFor(() => root());
    await sleep(600);
    check("a file with nothing to import says so, and what could not be handled", /No graphic items found in file\./.test(root().innerText) && /DXF dimensions not currently supported\./.test(root().innerText), root()?.innerText.slice(0, 200));
    button("OK").click();
    await sleep(900);
    check("OK on it is refused with the reason and the dialog stays", dialogs().includes("Import Graphics") && added(await board()).length === base.shapes, JSON.stringify(E.state().dialogs));
    button("Cancel").click();
    await sleep(200);
    const garbage = await E.run("pcbnew.InteractiveDrawing.ddImportGraphics", { name: "bad.dxf", text: "hello" });
    await waitFor(() => root());
    await sleep(600);
    check("a file that is not a DXF is described as such", /not a DXF file/.test(root().innerText), root()?.innerText.slice(0, 160));
    button("Cancel").click();
    await sleep(200);
    check("nothing is left on the board by all of this", added(await board()).length === base.shapes);
    void noItems; void garbage; void dropped;

    // ============================================================================================================================ Append Board
    const acts2 = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("Append Board and its drop are wired on the PCB tab", acts2["pcbnew.EditorControl.appendBoard"]?.enabled && acts2["pcbnew.Control.DdAppendBoard"]?.enabled, JSON.stringify([acts2["pcbnew.EditorControl.appendBoard"], acts2["pcbnew.Control.DdAppendBoard"]]));
    const appendEntry = await fromMenu("File", "Append Board");
    check("File has Append Board... and it is enabled (it opens a file chooser, so it is not clicked here)", !!appendEntry && appendEntry.getAttribute("aria-disabled") === "false", appendEntry?.outerHTML?.slice(0, 120));
    if (document.querySelector('.menubar-dropdown [role="menuitem"]')) { [...document.querySelectorAll(".menubar-item")].find((e) => e.childNodes[0]?.textContent?.trim() === "File")?.click(); await sleep(250); } // a click toggles: close the menu left open
    const notBoard = await E.run("pcbnew.Control.DdAppendBoard", { name: "old.brd", text: "x" });
    check("a dropped file that is not a KiCad board is refused in KiCad's words", notBoard.toast === "Only KiCad boards (.kicad_pcb files) can be appended.", JSON.stringify(notBoard));
    const garbageBoard = await E.run("pcbnew.Control.DdAppendBoard", { name: "bad.kicad_pcb", text: "hello" });
    check("a .kicad_pcb that does not parse is refused as 'Error loading board.'", /^Error loading board\./.test(garbageBoard.toast ?? ""), JSON.stringify(garbageBoard));
    const partsBefore = (await board()).parts.length;
    check("nothing came of the refusals", (await board()).parts.length === partsBefore && added(await board()).length === base.shapes);

    // Under strict (the default) a command that makes the board worse is refused, as any other is: the part would land outside this 35 x 35 mm board.
    await hover(...C);
    const strictRefusal = await E.run("pcbnew.Control.DdAppendBoard", { name: "other.kicad_pcb", text: otherBoard });
    const afterStrict = await board();
    check("in strict mode an append that makes the board worse is refused with the reason, and nothing is added or carried", /refused because --strict/.test(strictRefusal.toast ?? "") && afterStrict.parts.length === partsBefore && added(afterStrict).length === base.shapes && E.state().tool === "select", JSON.stringify([strictRefusal.toast, E.state().tool]));
    const strictBox = () => [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "strict")?.querySelector("input");
    const wasStrict = strictBox()?.checked;
    if (wasStrict) { strictBox().click(); await sleep(300); }
    await hover(...C);
    const appended = await E.run("pcbnew.Control.DdAppendBoard", { name: "other.kicad_pcb", text: otherBoard });
    applied++;
    await sleep(800);
    const after = await board();
    const zz = after.parts.find((p) => p.ref === "ZZ1");
    check("the file's part is a part of this board, placed", !!zz && zz.placed && after.parts.length === partsBefore + 1, JSON.stringify(zz && { ref: zz.ref, at: zz.at }));
    const newLine = added(after).find((x) => x.layer === "Dwgs.User" && x.kind === "segment" && x.start[0] === C[0] && x.start[1] === C[1]);
    check("the line that was the file's top left item is at the cursor's grid point, 30 mm long as it was", !!newLine && newLine.end[0] === C[0] + 30000 && newLine.end[1] === C[1], JSON.stringify(newLine && [newLine.start, newLine.end]));
    check("the part keeps its place relative to the line: the file's (30, 20) against (15, 5) is 15, 15 mm on", !!zz && zz.at[0] === C[0] + 15000 && zz.at[1] === C[1] + 15000, JSON.stringify(zz?.at));
    const trk = (after.routing?.tracks ?? []).find((t) => t.pts?.[0]?.[0] === C[0] + 5000 && t.pts?.[0]?.[1] === C[1] + 5000);
    check("and the track starts 5, 5 mm from the cursor: the file's (20, 10) less (15, 5)", !!trk && trk.net === "GND", JSON.stringify(trk && { net: trk.net, pts: trk.pts }));
    check("the Move tool carries what came in, and it is what is selected", E.state().tool === "move" && E.state().selection.length >= 4, JSON.stringify([E.state().tool, E.state().selection.map((x) => x.kind)]));
    check("the toast says what was appended", /Appended 1 footprint, 1 track, 1 graphic, 1 text\./.test(appended.toast ?? ""), JSON.stringify(appended.toast));
    key("Escape");
    await sleep(1000);
    const gone3 = await board();
    check("Escape takes the whole append back, one undo step: no part ZZ1, no new shapes", !gone3.parts.some((p) => p.ref === "ZZ1") && gone3.parts.length === partsBefore && added(gone3).length === base.shapes && E.state().tool === "select", `${gone3.parts.length} parts, ${added(gone3).length} shapes, tool ${E.state().tool}`);
    applied--;
    // again, and this time a click drops it where the pointer went
    await hover(...C);
    await E.run("pcbnew.Control.DdAppendBoard", { name: "other.kicad_pcb", text: otherBoard });
    applied++;
    await sleep(800);
    await hover(...D);
    const [dx, dy] = w2c(...D);
    fire("pointerdown", dx, dy, 1);
    fire("pointerup", dx, dy);
    await sleep(1200);
    applied++;
    const dropped2 = await board();
    const line2 = added(dropped2).find((x) => x.layer === "Dwgs.User" && x.kind === "segment" && x.end[0] === D[0] + 30000);
    check("a click drops the carried board where the pointer went: the line's start is at the second place", !!line2 && line2.start[0] === D[0] && line2.start[1] === D[1] && E.state().tool === "select", JSON.stringify(line2 && [line2.start, line2.end]) + " tool " + E.state().tool);
    await undo(2);
    applied -= 2;
    const clean = await board();
    check("two undos (the move, then the append) leave the board as it was", clean.parts.length === partsBefore && added(clean).length === base.shapes && !clean.parts.some((p) => p.ref === "ZZ1"));
    if (wasStrict && !strictBox()?.checked) { strictBox().click(); await sleep(300); }
    void appended;

    // ============================================================================================================================ file drops
    const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
    const undoDomain = async (domain, n = 1) => { for (let i = 0; i < n; i++) await post("/api/undo", { domain }); await sleep(400); };
    /** A file dropped on the window: a DataTransfer holding the File and the events a browser sends for it. */
    const dropFiles = async (files, target = document.body) => {
      const dt = new DataTransfer();
      for (const f of files) dt.items.add(new File([f.text], f.name, { type: "text/plain" }));
      const mk = (type) => new DragEvent(type, { bubbles: true, cancelable: true, composed: true, dataTransfer: dt });
      target.dispatchEvent(mk("dragenter"));
      const over = mk("dragover");
      target.dispatchEvent(over);
      await sleep(150); // React draws the outline
      const outline = !!document.querySelector('[data-testid="file-drop-outline"]');
      const drop = mk("drop");
      target.dispatchEvent(drop);
      await sleep(900);
      return { prevented: over.defaultPrevented && drop.defaultPrevented, outline };
    };
    const lastToast = () => [...document.querySelectorAll("div")].find((el) => el.style.zIndex === "3000" && el.style.position === "absolute")?.textContent ?? ""; // the page's toast: a div with no class
    let d = await dropFiles([{ name: "board.dxf", text: boardDxf }]);
    await waitFor(() => root());
    await sleep(500);
    check("a DXF dropped on the board editor opens Import Graphics with the file in File:", d.prevented && d.outline && dialogs().includes("Import Graphics") && field("File")?.value === "board.dxf" && /8 items/.test(root()?.innerText ?? ""), JSON.stringify([d, field("File")?.value]));
    // a graphic dropped outside the open dialog replaces the file in it; anything else is ignored while it is open
    d = await dropFiles([{ name: "box.dxf", text: rect2x1(4) }]);
    await waitFor(() => field("File")?.value === "box.dxf");
    check("while Import Graphics is open another graphic dropped on the window takes its place in File:", field("File")?.value === "box.dxf" && /4 items/.test(root()?.innerText ?? ""), field("File")?.value);
    const partsNow = (await board()).parts.length;
    d = await dropFiles([{ name: "other.kicad_pcb", text: otherBoard }]);
    check("and a board dropped then is ignored, as a modal dialog blocks KiCad's frame", dialogs().includes("Import Graphics") && (await board()).parts.length === partsNow);
    button("Cancel").click();
    await sleep(200);

    d = await dropFiles([{ name: "a.dxf", text: boardDxf }, { name: "b.dxf", text: boardDxf }]);
    await waitFor(() => root());
    await sleep(400);
    check("two files dropped: the first opens and the toast says the other was skipped", field("File")?.value === "a.dxf" && /Only a\.dxf was opened; b\.dxf was skipped/.test(lastToast()), `${field("File")?.value} / ${lastToast()}`);
    button("Cancel").click();
    await sleep(200);
    d = await dropFiles([{ name: "notes.txt", text: "hello" }]);
    check("a file the editor does not take is said not to be one, and the browser did not take it either", d.prevented && dialogs().length === 0 && /notes\.txt is not a file this editor opens \(it takes \.kicad_pcb, \.brd, \.dxf, \.svg\)\./.test(lastToast()), lastToast());

    // a board dropped on the board editor is appended (strict off, as the Append Board checks have it)
    if (strictBox()?.checked) { strictBox().click(); await sleep(300); }
    await hover(...C);
    await dropFiles([{ name: "other.kicad_pcb", text: otherBoard }]);
    applied++;
    await sleep(500);
    const dropped3 = await board();
    check("a .kicad_pcb dropped on the board editor is appended: its part is on the board and carried by the Move tool", dropped3.parts.some((p) => p.ref === "ZZ1") && E.state().tool === "move", `${dropped3.parts.length} parts, tool ${E.state().tool}`);
    key("Escape");
    await sleep(1000);
    applied--;
    check("and Escape takes it back", !(await board()).parts.some((p) => p.ref === "ZZ1") && E.state().tool === "select");
    if (wasStrict && !strictBox()?.checked) { strictBox().click(); await sleep(300); }

    // the Footprint Editor takes a .kicad_mod
    await clickTab("Footprint");
    const kicadMod = `(footprint "DropTest" (version 20240108) (generator "pcbnew") (layer "F.Cu")
  (descr "dropped")
  (pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste"))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Mask" "F.Paste"))
  (fp_line (start -2 -1) (end 2 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS")))`;
    const fpNames = async () => (await get("/api/footprint_library")).names;
    const fpBefore = await fpNames();
    d = await dropFiles([{ name: "DropTest.kicad_mod", text: kicadMod }]);
    await sleep(800);
    check("a .kicad_mod dropped on the Footprint Editor is imported into the library", d.prevented && (await fpNames()).some((n) => n.endsWith("DropTest")) && !fpBefore.some((n) => n.endsWith("DropTest")), JSON.stringify([d, (await fpNames()).filter((n) => /DropTest/.test(n))]));
    check("and it is the selected, opened footprint of the editor", [...document.querySelectorAll('[aria-selected="true"]')].some((e) => e.textContent.trim() === "DropTest"));
    for (let i = 0; i < 4 && (await fpNames()).some((n) => n.endsWith("DropTest")); i++) await undoDomain("footprint_editor");
    check("one undo of the Footprint Editor takes the import away", !(await fpNames()).some((n) => n.endsWith("DropTest")));
    d = await dropFiles([{ name: "x.dxf", text: boardDxf }]);
    check("a DXF dropped on the Footprint Editor is not taken (its frame accepts footprints only)", dialogs().length === 0 && /x\.dxf is not a file this editor opens \(it takes \.kicad_mod\)\./.test(lastToast()), lastToast());

    // the schematic editor
    await clickTab("Schematic");
    d = await dropFiles([{ name: "board.dxf", text: boardDxf }]);
    await waitFor(() => root());
    await sleep(500);
    check("a DXF dropped on the schematic editor opens its Import Graphics (interactive placement, no layer) with the file", dialogs().includes("Import Graphics") && field("File")?.value === "board.dxf" && !!field("Interactive placement") && !field("Layer"));
    button("Cancel").click();
    await sleep(200);
    d = await dropFiles([{ name: "part.kicad_sch", text: "(kicad_sch)" }]);
    check("a schematic dropped on it is said to be beyond a drop (Import Sheet reads files by their path on the server)", d.prevented && dialogs().length === 0 && /part\.kicad_sch cannot be opened by a drop in the studio\./.test(lastToast()), lastToast());

    // the Symbol Editor accepts no file
    await clickTab("Symbol");
    d = await dropFiles([{ name: "a.dxf", text: boardDxf }]);
    check("nothing dropped on the Symbol Editor is taken, and the browser did not take it either", d.prevented && dialogs().length === 0 && /a\.dxf is not a file this editor opens\./.test(lastToast()), lastToast());
    await clickTab("PCB");

    // ============================================================================================================================ the schematic
    await clickTab("Schematic");
    if (!(E.state().view?.scale > 0)) { await E.run("common.Control.zoomFitScreen"); await sleep(300); } // (a pane that is not on screen has not fitted the sheet either)
    const sheetQuery = () => (E.state().sheetPath.length ? `?sheet=${E.state().sheetPath.join("/")}` : "");
    const sch = () => get(`/api/schematic${sheetQuery()}`);
    const schActs = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("Import Graphics and its drop are wired on the Schematic tab", schActs["eeschema.EditorControl.importGraphics"]?.enabled && schActs["eeschema.EditorControl.ddImportGraphics"]?.enabled, JSON.stringify([schActs["eeschema.EditorControl.importGraphics"], schActs["eeschema.EditorControl.ddImportGraphics"]]));
    const s0 = await sch();
    const sBase = { lines: s0.lines.length, graphics: s0.graphics.length, texts: s0.texts.length, lineIds: new Set(s0.lines.map((l) => l.id)), graphicIds: new Set(s0.graphics.map((g) => g.id)) };
    const sEntry = await fromMenu("File", "Import Graphics");
    check("the schematic's File > Import has Import Graphics... and it is enabled", !!sEntry && sEntry.getAttribute("aria-disabled") === "false", sEntry?.outerHTML?.slice(0, 120));
    sEntry?.click();
    await sleep(400);
    check("it opens the schematic's dialog: interactive placement, the scale, the DXF width and units, and no layer, group or discontinuities", dialogs().includes("Import Graphics") && field("Interactive placement")?.checked && !!field("Import scale") && !!field("Default line width") && !!field("Default units") && !field("Layer") && !field("Group imported items") && !field("Fix discontinuities"));
    button("Cancel")?.click();
    await sleep(200);
    const schRefused = await E.run("eeschema.EditorControl.ddImportGraphics", { name: "notes.txt", text: "hello" });
    check("a dropped file no plugin handles is refused on the schematic too", schRefused.toast === "There is no plugin to handle this file type." && dialogs().length === 0, JSON.stringify(schRefused));

    /** Drop a file with the drop action, set the dialog (the schematic's and the symbol editor's share it), press OK. */
    const importSch = async (file, set = {}, { ok = true } = {}) => {
      await E.run("eeschema.EditorControl.ddImportGraphics", file);
      await waitFor(() => root());
      await sleep(500);
      const { placeAt = false, ...rest } = set;
      field(placeAt ? "At" : "Interactive placement").click();
      await sleep(30);
      const want = { "Import scale": "1.0", "Default units": "mm", "Default line width": "0", "At X": "0", "At Y": "0", ...rest };
      for (const [label, v] of Object.entries(want)) {
        const el = field(label);
        if (el) { setNative(el, v); await sleep(30); }
      }
      await sleep(400);
      const info = root()?.querySelector('[aria-label="File contents"]')?.innerText ?? "";
      if (ok) { button("OK").click(); await sleep(1500); }
      return { info, sch: await sch() };
    };
    const newLines = (s) => s.lines.filter((l) => !sBase.lineIds.has(l.id));
    const newGraphics = (s) => s.graphics.filter((g) => !sBase.graphicIds.has(g.id));
    const span = (pts) => [Math.min(...pts.map((p) => p[0])), Math.min(...pts.map((p) => p[1])), Math.max(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[1]))];

    let q = await importSch({ name: "board.dxf", text: boardDxf }, { placeAt: true, "At X": "10", "At Y": "5" });
    appliedSch++;
    check("the file is described as in the board's dialog", /8 items: 4 lines, 4 arcs/.test(q.info), q.info);
    let nl = newLines(q.sch), ng = newGraphics(q.sch);
    check("OK adds the 4 outline lines as graphic lines and the 4 corners as arc shapes", nl.length === 4 && ng.length === 4 && ng.every((g) => g.shape.type === "arc"), `${nl.length} lines, ${ng.map((g) => g.shape.type)}`);
    const [lx0, ly0, lx1, ly1] = span(nl.flatMap((l) => l.pts));
    check("At 10, 5 mm: the outline spans x 10 to 50 mm and y -25 to 5 mm (the DXF's Y axis points up, the sheet's down)", lx0 === 10000 && lx1 === 50000 && ly0 === -25000 && ly1 === 5000, [lx0, ly0, lx1, ly1]);
    check("no line width in the file or the dialog is the sheet's default width (0)", nl.every((l) => l.width_um === 0) && ng.every((g) => !g.width_um));
    check("the dialog closed and the 8 new items are selected", dialogs().length === 0 && E.state().selection.length === 8, `${E.state().selection.length} selected, ${JSON.stringify(dialogs())}`);
    await undoDomain("schematic");
    appliedSch--;
    let back2 = await sch();
    check("ONE undo of the schematic takes the whole import back", back2.lines.length === sBase.lines && back2.graphics.length === sBase.graphics, `${back2.lines.length} lines, ${back2.graphics.length} graphics`);

    // the scale, the default line width and a DXF with no unit in the dialog's unit
    q = await importSch({ name: "box.dxf", text: rect2x1(null) }, { placeAt: true, "Import scale": "2", "Default line width": "0.15", "Default units": "mm" });
    appliedSch++;
    nl = newLines(q.sch);
    const [bx0, , bx1] = span(nl.flatMap((l) => l.pts));
    check("scale 2 doubles the 2 x 1 mm box and its default line width (0.15 becomes 0.3 mm)", nl.length === 4 && bx1 - bx0 === 4000 && nl.every((l) => l.width_um === 300), `${nl.length} lines, width ${bx1 - bx0}, widths ${nl.map((l) => l.width_um)}`);
    await undoDomain("schematic");
    appliedSch--;

    // an SVG becomes polygons with the stroke's own width
    q = await importSch({ name: "outline.svg", text: svgRect }, { placeAt: true });
    appliedSch++;
    ng = newGraphics(q.sch);
    const poly2 = ng.find((g) => g.shape.type === "polygon");
    const [px0, py0, px1, py1] = poly2 ? span(poly2.shape.pts.map((p) => [p.x, p.y])) : [0, 0, 0, 0];
    check("an SVG's rounded rect is one polygon of its own stroke width (0.1 mm), 40 x 30 mm, y down as it has it", ng.length === 1 && !!poly2 && poly2.width_um === 100 && px0 === 0 && py0 === 0 && px1 === 40000 && py1 === 30000, JSON.stringify(ng.map((g) => [g.shape.type, g.width_um])));
    await undoDomain("schematic");
    appliedSch--;

    // interactive placement: the drawing's origin at the cursor
    const rect = canvas().getBoundingClientRect();
    const v0 = E.state().view;
    let cx = rect.left + rect.width * 0.4, cy = rect.top + rect.height * 0.4;
    const worldOf = (x, y) => [(x - rect.left - v0.x) / v0.scale, (y - rect.top - v0.y) / v0.scale];
    const nearHalf = (u) => Math.abs(((u / 1270) % 1 + 1) % 1 - 0.5) < 0.05;
    while (nearHalf(worldOf(cx, cy)[0]) || nearHalf(worldOf(cx, cy)[1])) { cx += 7; cy += 5; }
    fire("pointermove", cx, cy);
    await sleep(200);
    const [cwx, cwy] = worldOf(cx, cy);
    const gx = Math.round(cwx / 1270) * 1270, gy = Math.round(cwy / 1270) * 1270;
    q = await importSch({ name: "board.dxf", text: boardDxf }, { placeAt: false });
    appliedSch++;
    nl = newLines(q.sch);
    const [qx0, , , qy1] = span(nl.flatMap((l) => l.pts));
    check("interactive placement: the drawing's origin is at the cursor's grid point", qx0 === gx && qy1 === gy, `got ${qx0}, ${qy1}; the cursor's grid point is ${gx}, ${gy}`);
    check("and the Move tool carries the 8 new items", E.state().tool === "move" && E.state().selection.length === 8, JSON.stringify([E.state().tool, E.state().selection.length]));
    key("Escape");
    await sleep(1000);
    const gone2 = await sch();
    check("Escape takes the placement back, one undo step of the schematic", gone2.lines.length === sBase.lines && gone2.graphics.length === sBase.graphics && E.state().tool === "select", `${gone2.lines.length} lines, tool ${E.state().tool}`);
    appliedSch--;

    // refusals
    const sNone = await importSch({ name: "dims.dxf", text: dxf(pair(0, "DIMENSION") + pair(8, "0")) }, { placeAt: true }, { ok: false });
    button("OK").click();
    await sleep(900);
    check("a file with nothing to import is refused with the reason and the dialog stays", dialogs().includes("Import Graphics") && /No graphic items found in file\./.test(root().innerText), root()?.innerText.slice(0, 160));
    setNative(field("Import scale"), "0");
    await sleep(100);
    button("OK").click();
    await sleep(300);
    check("a scale of zero is refused in the dialog's words", /scale/i.test(root()?.querySelector('[role="alert"]')?.textContent ?? ""));
    button("Cancel").click();
    await sleep(200);
    void sNone;
    const sEnd = await sch();
    check("nothing is left on the sheet by all of this", sEnd.lines.length === sBase.lines && sEnd.graphics.length === sBase.graphics && sEnd.texts.length === sBase.texts);

    // ============================================================================================================================ the Symbol Editor
    await clickTab("Symbol");
    const symActs = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("Import Graphics and its drop are wired in the Symbol Editor", symActs["eeschema.EditorControl.importGraphics"]?.enabled && symActs["eeschema.EditorControl.ddImportGraphics"]?.enabled);
    const names0 = (await get("/api/symbol_editor/names")).names;
    await E.run("eeschema.SymbolLibraryControl.newSymbol");
    await sleep(800);
    appliedSym++;
    const libId = (await get("/api/symbol_editor/names")).names.find((n) => !names0.includes(n));
    check("a new symbol is open to import into", !!libId, JSON.stringify((await get("/api/symbol_editor/names")).names));
    const symbol = () => get(`/api/symbol?lib_id=${encodeURIComponent(libId)}`);
    const sym0 = await symbol();
    const symBase = new Set((sym0.graphics ?? []).map((g) => g.id));
    const symNew = (s) => (s.graphics ?? []).filter((g) => !symBase.has(g.id));
    const mEntry = await fromMenu("File", "Import Graphics");
    check("the Symbol Editor's File > Import has Import Graphics... and it is enabled", !!mEntry && mEntry.getAttribute("aria-disabled") === "false", mEntry?.outerHTML?.slice(0, 120));
    mEntry?.click();
    await sleep(400);
    check("it opens the same dialog", dialogs().includes("Import Graphics") && !!field("Interactive placement"));
    button("Cancel")?.click();
    await sleep(200);

    q = await importSch({ name: "board.dxf", text: boardDxf }, { placeAt: true, "At X": "10", "At Y": "5" });
    appliedSym++;
    let sg = symNew(await symbol());
    const symLines = sg.filter((g) => g.kind === "polyline"), symArcs = sg.filter((g) => g.kind === "arc");
    check("OK adds 4 polylines and 4 arcs to the open symbol", symLines.length === 4 && symArcs.length === 4 && sg.length === 8, JSON.stringify(sg.map((g) => g.kind)));
    const symPts = symLines.flatMap((g) => g.pts.map((p) => [p.x, p.y]));
    const [mx0, my0, mx1, my1] = span(symPts);
    check("in millimetres with the library's Y axis up: x 10 to 50, y -5 to 25 for the outline (At 10, 5 on the drawing)", Math.abs(mx0 - 10) < 1e-9 && Math.abs(mx1 - 50) < 1e-9 && Math.abs(my0 + 5) < 1e-9 && Math.abs(my1 - 25) < 1e-9, [mx0, my0, mx1, my1]);
    check("they are on the unit being edited, and selected", sg.every((g) => (g.unit ?? 1) === 1) && E.state().selection.length === 8, JSON.stringify([sg.map((g) => g.unit), E.state().selection.length]));
    await undoDomain("symbol_editor");
    appliedSym--;
    check("ONE undo of the Symbol Editor takes the whole import back", symNew(await symbol()).length === 0);

    q = await importSch({ name: "outline.svg", text: svgRect }, { placeAt: true });
    appliedSym++;
    sg = symNew(await symbol());
    check("an SVG outline is a closed polyline of its stroke width", sg.length === 1 && sg[0].kind === "polyline" && Math.abs(sg[0].stroke_mm - 0.1) < 1e-9 && sg[0].pts.length > 20, JSON.stringify(sg.map((g) => [g.kind, g.stroke_mm, g.pts?.length])));
    await undoDomain("symbol_editor");
    appliedSym--;

    const canvas2 = canvas().getBoundingClientRect();
    fire("pointermove", canvas2.left + canvas2.width * 0.45, canvas2.top + canvas2.height * 0.45);
    await sleep(200);
    q = await importSch({ name: "board.dxf", text: boardDxf }, { placeAt: false });
    appliedSym++;
    sg = symNew(await symbol());
    const ipts = sg.filter((g) => g.kind === "polyline").flatMap((g) => g.pts.map((p) => [p.x, p.y]));
    const [jx0] = span(ipts);
    check("interactive placement: the drawing's origin is at the cursor's grid point (a multiple of 1.27 mm), and the items are selected", sg.length === 8 && Math.abs(jx0 / 1.27 - Math.round(jx0 / 1.27)) < 1e-6 && E.state().selection.length === 8, JSON.stringify([sg.length, jx0, E.state().selection.length]));
    await undoDomain("symbol_editor");
    appliedSym--;
    await undoDomain("symbol_editor");
    appliedSym--;
    check("and with the new symbol taken back too, nothing is left of this", !(await get("/api/symbol_editor/names")).names.includes(libId));
    await clickTab(firstTab === "schematic" ? "Schematic" : firstTab === "symbol" ? "Symbol" : "PCB");
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : e);
  } finally {
    // Whatever a failed step left on the board: take it back, and close what is open.
    for (let guard = 0; guard < 6 && dialogs().length > 0; guard++) { (button("Cancel") ?? { click() {} }).click(); await sleep(150); }
    if (E.state().tool !== "select") { key("Escape"); await sleep(300); }
    if (applied > 0) await undo(applied);
    { const box = [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "strict")?.querySelector("input"); if (box && !box.checked) box.click(); }
    for (let i = 0; i < appliedSch; i++) await post("/api/undo", { domain: "schematic" });
    for (let i = 0; i < appliedSym; i++) await post("/api/undo", { domain: "symbol_editor" });
    if (E.state().tab !== firstTab) { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === (firstTab === "schematic" ? "Schematic" : firstTab === "symbol" ? "Symbol" : "PCB"))?.click(); await sleep(800); }
    // The refusals this check provokes on purpose raise an error toast each; anything else is an error.
    const meant = [/There is no plugin to handle this file type\./, /No graphic items found in file\./, /Only KiCad boards \(\.kicad_pcb files\) can be appended\./, /Error loading board\./, /refused because --strict/];
    const now = E.errors().slice(errorsBefore).filter((x) => !/Unable to preventDefault/.test(x.message) && !meant.some((re) => re.test(x.message)));
    check("no error was raised in the page", now.length === 0, now.map((x) => x.message).join(" | "));
    window.__filesInCheck.done = true;
  }
  const failed = results.filter((r) => !r.ok);
  return { ok: failed.length === 0, failed: failed.length, results };
})()
