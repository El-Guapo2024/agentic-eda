// The 3D view's items of UI-LEFT-3 D3-1 .. D3-5 (PARITY-3d, task-v15-3d): open the 3D tab of a served board and check through `window.__eda` (`state().viewer3d`,
// components/viewer3d/viewer3dProbe.ts: `layers3d` counts what kicad-port/layers3d.ts built per layer and kind, `n:<layer>:<tag>` and `poly:<layer>:<tag>`):
//
//   1  D3-5  a viewport the project holds as its matrix alone (`board.3dviewports`, read back on import) puts the camera where `CAMERA::SetViewMatrix` puts it
//   2  D3-1  a text in an outline font (DejaVu Sans) is drawn as its glyphs' polygons (`addText`'s triangles), not its ink box
//   3  D3-7  a dimension, a table, a text box on copper (and a knockout one), a line with an arrow ending are drawn (`createLayers`' PCB_DIM_*, PCB_TABLE_T, PCB_TEXTBOX_T)
//   4  D3-2  KiCad 25ec2335's F.Fab / B.Fab / F.Courtyard / B.Courtyard rows are listed, off, and draw the footprints' fab graphics when on
//   5  D3-3  a custom rule's solder_mask_expansion reaches the 3D masks (`GET /api/board3d`'s `rule_margins`, `PAD::GetSolderMaskExpansion`'s `EvalRules`)
//   6  D3-4  with "Disable all plated holes", the body is drawn without its holes while the camera moves (`renderBoardBody( skipRenderHoles )`)
//
// Run it on a COPY of a served board (it adds items and a rules file and leaves them; the preferences it changes it puts back). Returns { ok, results: [{ name, ok, detail? }, ...] }.
// run-all-checks: board=mcu30-zones minutes=8
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const view = () => __eda.state().viewer3d;
  const L = () => view().layers3d ?? {};
  const until = async (test, ms = 30000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await test()) return true; await sleep(150); } return false; };
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const json = (u) => fetch(u, { cache: "no-store" }).then((r) => r.json());
  const row = (id) => document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
  const box = (label) => [...document.querySelectorAll(".dialog input[type=checkbox]")].find((i) => i.getAttribute("aria-label") === label);
  const button = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const choose = async (selectId, value) => { const s = document.getElementById(selectId); s.value = value; s.dispatchEvent(new Event("change", { bubbles: true })); await sleep(300); };
  const cam = () => __eda.viewer3dCamera();
  // "Disable all plated holes" (Realtime Renderer) and the slowest "Redraw speed" (General: a face view then takes seconds, so a slow headless frame still sees it move)
  const opengl = async (on) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => document.querySelector('[data-page="3d_general"]'), 5000);
    document.querySelector('[data-page="3d_general"]').click();
    await sleep(300);
    const speed = document.querySelector('.dialog input[aria-label="Redraw speed"]');
    if (speed) {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(speed, on ? "1" : "3");
      speed.dispatchEvent(new Event("input", { bubbles: true }));
      speed.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(100);
    }
    document.querySelector('[data-page="3d_opengl"]').click();
    await sleep(300);
    const b = box("Disable all plated holes");
    if (b && b.checked !== on) { b.click(); await sleep(100); }
    button(document.querySelector(".dialog"), "OK")?.click();
    await sleep(500);
  };
  const n = (k) => L()[k] ?? 0;
  const ver = async () => (await json("/api/state")).version;
  const rebuilt = async (test, ms = 30000) => until(() => test(), ms);

  try {
    // 1 (before the tab opens: the panel reads the viewports when it mounts) --------------------------------------------- a KiCad project's viewport
    // a top view (no rotation), the board's look-at point 8 of KiCad's 3D units off: half the home distance (1.25 x 12.8), a zoom of 0.5
    const app = await json("/api/appearance");
    const matrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, -8, 1];
    await fetch("/api/appearance", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ viewer3d: { ...(app.viewer3d ?? {}), viewports: [...(app.viewer3d?.viewports ?? []), { name: "From KiCad", matrix }] } }) });

    await __eda.run("common.Control.show3DViewer");
    if (!(await until(() => view() && view().open, 20000))) throw new Error("the 3D tab did not open");
    await until(() => view().bodyFromServer && (view().scene["pad"] ?? 0) > 0, 60000);
    await sleep(500);

    await until(() => document.querySelector('#v3d-viewports option[value="v:From KiCad"]'), 5000);
    check("the project's viewport is listed", !!document.querySelector('#v3d-viewports option[value="v:From KiCad"]'));
    await choose("v3d-viewports", "v:From KiCad");
    const c = cam();
    const q = c?.quat ?? {};
    check("choosing it puts the camera at its matrix: no rotation, the zoom its depth over the home distance (CAMERA::SetViewMatrix)", c && Math.abs(Math.abs(q.w) - 1) < 1e-6 && Math.abs(c.zoom - 0.5) < 0.05, c);
    check("and the list stays on it", document.getElementById("v3d-viewports").value === "v:From KiCad");

    const board = await json("/api/state");
    const parts = board.parts.filter((p) => p.placed);
    const ox = 4_000_000;
    const oy = 32_000_000;

    // 2 ------------------------------------------------------------------------------------------------------------------------------- an outline font's glyphs
    const textsBefore = new Set((board.drawings?.texts ?? []).map((t) => t.id));
    await cmd({ op: "add_text", text: { id: "", content: "HHH", at: { x: ox, y: oy }, angle: 0, layer: "F.SilkS", size_nm: 1_500_000, stroke_width: 200_000, justify: "left", mirror: false } });
    const text = ((await json("/api/state")).drawings?.texts ?? []).find((t) => !textsBefore.has(t.id));
    check("a board text is added", !!text);
    await rebuilt(() => n("n:F.SilkS:text") > 0);
    const strokeSegs = n("n:F.SilkS:text");
    check("in the stroke font it is its pen strokes", strokeSegs >= 9 && n("poly:F.SilkS:text") === 0, { n: strokeSegs, poly: n("poly:F.SilkS:text") });
    await cmd({ op: "edit_text_style", id: text.id, face: "DejaVu Sans" });
    const glyphs = await rebuilt(() => n("poly:F.SilkS:text") >= 3, 30000);
    check("in DejaVu Sans it is its three glyphs' polygons (OUTLINE_GLYPH's triangles), not one ink box", glyphs && n("poly:F.SilkS:text") === 3 && n("n:F.SilkS:text") === 3, { poly: n("poly:F.SilkS:text"), n: n("n:F.SilkS:text") });

    // 3 ------------------------------------------------------------------------------------------------------------------------- dimensions, tables, text boxes, endings
    await cmd({ op: "add_dimension", dimension: { layer: "F.SilkS", kind: { kind: "aligned", height: 2_000_000 }, start: { x: ox, y: oy + 4_000_000 }, end: { x: ox + 10_000_000, y: oy + 4_000_000 }, prefix: "", suffix: "", units: "mm", units_format: "no_suffix", precision: 1, suppress_trailing_zeros: false, text_position: "outside", keep_text_aligned: true, text_angle: 0, text_size_nm: 1_000_000, stroke_width: 150_000, arrow_length: 1_270_000, extension_offset: 500_000, extension_height: 586_000, arrow_direction: "outward" } });
    check("a dimension is drawn: its lines and its text (addShape( PCB_DIMENSION_BASE ))", await rebuilt(() => n("n:F.SilkS:dimension") > 6), { n: n("n:F.SilkS:dimension") });
    await cmd({ op: "table", verb: "draw", layer: "F.SilkS", at: { x: ox + 14_000_000, y: oy }, cols: 2, rows: 2, cell_w_nm: 6_000_000, cell_h_nm: 3_000_000, line_width_nm: 120_000, patch: { texts: [["A1", "B1"], ["A2", "B2"]] } });
    check("a table is drawn: its borders and its cells' texts (addTable)", await rebuilt(() => n("n:F.SilkS:table") > 10), { n: n("n:F.SilkS:table") });
    const tb = (layer, knockout) => cmd({ op: "pcb_item", verb: "add_text_box", text_box: { layer, text: "BOX", knockout, start: { x: ox, y: oy + 8_000_000 }, end: { x: ox + 10_000_000, y: oy + 12_000_000 }, margin_left_nm: 500_000, margin_top_nm: 500_000, margin_right_nm: 500_000, margin_bottom_nm: 500_000, stroke_width_nm: 100_000, size_nm: 1_000_000, halign: "left", valign: "top" } });
    await tb("F.Cu", false);
    check("a text box on copper is drawn: its border and its text", await rebuilt(() => n("n:F.Cu:text-box") >= 6), { n: n("n:F.Cu:text-box") });
    check("in the copper's mesh (copper-text-box)", (view().scene["copper-text-box"] ?? 0) > 0, view().scene);
    await tb("B.Cu", true);
    // the knockout (one of the server's polygons, `PCB_TEXTBOX::TransformTextToPolySet`) and the border drawn after it (`addShape( PCB_TEXTBOX )`): no stroke of the text
    check("a knockout box of the board is its box less the text: one polygon the server works out, and its border", await rebuilt(() => n("n:B.Cu:text-box") >= 1), { n: n("n:B.Cu:text-box") });
    check("none of its text's strokes", n("n:B.Cu:text-box") <= 5, { n: n("n:B.Cu:text-box") });
    const shapesBefore = new Set((((await json("/api/state")).drawings?.shapes) ?? []).map((s) => s.id));
    await cmd({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 200_000, filled: false, start: { x: ox, y: oy + 14_000_000 }, end: { x: ox + 10_000_000, y: oy + 14_000_000 } } });
    const line = ((await json("/api/state")).drawings?.shapes ?? []).find((s) => !shapesBefore.has(s.id));
    const polyShapes = n("poly:F.SilkS:shape");
    await cmd({ op: "pcb_item", verb: "set_shape_style", ids: [line.id], end_ending: { style: "arrow" } });
    check("a line's arrow ending is drawn (KiCad 25ec2335's LINE_ENDING::GetShapes)", await rebuilt(() => n("poly:F.SilkS:shape") === polyShapes + 1), { before: polyShapes, after: n("poly:F.SilkS:shape") });

    // 4 ---------------------------------------------------------------------------------------------------------------------- the fab and courtyard rows
    const rowIds = [...document.querySelectorAll("[data-row]")].map((e) => e.getAttribute("data-row"));
    const at = rowIds.indexOf("f_fab");
    check("the tree lists F.Fab, B.Fab, F.Courtyard and B.Courtyard after User.Eco2 (APPEARANCE_CONTROLS_3D 25ec2335)", at > 0 && rowIds.slice(at - 1, at + 4).join() === "user_eco2,f_fab,b_fab,f_courtyard,b_courtyard", rowIds.slice(Math.max(0, at - 1), at + 4));
    check("they are off by default", row("f_fab") && !row("f_fab").checked && !row("f_courtyard").checked);
    const fabGraphics = parts.some((p) => (p.graphics ?? []).some((g) => g.layer === "F.Fab"));
    row("f_fab").click();
    const fab = await rebuilt(() => n("n:F.Fab:fp-shape") > 0 || n("n:F.Fab:part-value") > 0, 15000);
    check("F.Fab on draws the footprints' fab graphics in its own colour (f_fab-*)", !fabGraphics || (fab && Object.keys(view().scene).some((k) => k.startsWith("f_fab-"))), { fabGraphics, scene: Object.keys(view().scene).filter((k) => k.startsWith("f_fab")) });
    row("f_fab").click();
    await rebuilt(() => n("n:F.Fab:fp-shape") === 0, 10000);

    // 5 ----------------------------------------------------------------------------------------------------------------------- a custom rule's mask expansion
    const area0 = n("maskPadArea");
    const v0 = await ver();
    await cmd({ op: "set_custom_rules", text: "(version 1)\n(rule \"wide mask\" (condition \"A.Type == 'Pad'\") (constraint solder_mask_expansion (opt 0.3mm)))\n" });
    await until(async () => (await ver()) !== v0, 10000);
    const b3d = await json("/api/board3d");
    check("GET /api/board3d answers the rule's expansion per pad", b3d.rule_margins?.mask === true && Object.values(b3d.rule_margins.pads ?? {}).every((p) => p.mask?.[0] === 300_000), Object.entries(b3d.rule_margins?.pads ?? {}).slice(0, 2));
    check("and the pads' mask openings grow by it", await rebuilt(() => n("maskPadArea") > area0 * 1.2, 20000), { before: area0, after: n("maskPadArea") });

    // 6 ----------------------------------------------------------------------------------------------------------------------- the body while the camera moves
    await opengl(true);
    // the front view by its key (`EDA_3D_ACTIONS::viewFront`, Y): an animated move
    const canvas = document.querySelector(".pcb-canvas-container canvas");
    canvas.dispatchEvent(new KeyboardEvent("keydown", { key: "y", bubbles: true }));
    let seenMoving = null;
    const t0 = performance.now();
    while (performance.now() - t0 < 8000) {
      await frame();
      if (view().cameraMoving && (view().movingBody.noHoles > 0 || performance.now() - t0 > 3000)) { seenMoving = { ...view().movingBody }; break; }
    }
    check("while it moves the body is the outlines without the holes (m_board), the holed one hidden", seenMoving && seenMoving.noHoles > 0 && seenMoving.withHoles === 0, seenMoving ?? { dialog: !!document.querySelector(".dialog"), aux: cam()?.auxDeg, prefs: __eda.state().viewer3dPrefs ?? null });
    await until(() => !view().cameraMoving, 20000);
    await frame();
    await frame();
    check("once it stops, the body with its holes again", view().movingBody.withHoles > 0 && view().movingBody.noHoles === 0, view().movingBody);
    await opengl(false);
  } catch (e) {
    check("no exception", false, String(e?.stack ?? e));
  }
  const ok = results.every((r) => r.ok);
  return { ok, results };
})();
