// The schematic's point editor on the items that are not drawn shapes (UI-LEFT-3 S3-2, S3-4; task-v13-schpoints), driven by pointer drags on the canvas:
//   S3-2  a hierarchical sheet's side is dragged by its middle (`SHEET_POINT_EDIT_BEHAVIOR`): the pin on that side and the wire ending on it follow, and a corner dragged
//         across the sheet stops at the smallest size the pins allow; each drag one undo step.
//   S3-4  an image's corner scales it about its centre (`BITMAP_POINT_EDIT_BEHAVIOR`); a graphic line's end takes the line ending there along (`LINE_POINT_EDIT_BEHAVIOR`),
//         and one dragged onto its start leaves a zero-length line, as KiCad's commit does;
//         a table cell's right-side point sets its column's width (`SCH_TABLECELL_POINT_EDIT_BEHAVIOR`); each one undo step.
//
// Run it in the page of a served board made from examples/ldo.yaml, on a COPY of a board: every scenario undoes what it did.
// It answers through `window.__schPointsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__schPointsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  const E = window.__eda;
  const state = () => E.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const toSelect = async () => { for (let i = 0; i < 3 && state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(300); } };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const sch = () => json("/api/schematic");
  const mm = (v) => Math.round(v * 1e6);
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; return !!cond; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    results.push({ name, ok, notes });
    await run("common.InteractiveSelection.clear");
    await toSelect();
  };

  const sContainer = () => document.querySelector(".pcb-canvas-container");
  const sFire = (type, x, y, extra = {}) => {
    const rect = sContainer().getBoundingClientRect();
    (sContainer().querySelector("canvas") || sContainer()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const sScreen = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const sDrag = async ([x0, y0], [x1, y1]) => {
    const [a, b] = sScreen(x0, y0);
    const [c, d] = sScreen(x1, y1);
    sFire("pointermove", a, b); await sleep(200);
    sFire("pointerdown", a, b, { buttons: 1 }); await sleep(60);
    for (let i = 1; i <= 6; i++) { sFire("pointermove", a + ((c - a) * i) / 6, b + ((d - b) * i) / 6, { buttons: 1 }); await sleep(80); }
    sFire("pointerup", c, d); await sleep(1500);
  };
  const points = () => (sContainer().dataset.editPoints ?? "").split(" ").filter(Boolean).map((s) => s.split(",").map(Number));
  const sides = () => (sContainer().dataset.editLines ?? "").split(" ").filter(Boolean).map((s) => s.split(",").map(Number));
  const select = async (id) => {
    await run("common.InteractiveSelection.selectItems", [id]);
    return until(() => (sContainer().dataset.editPoints ?? "") !== "", 3000);
  };
  /** Undo until the sheet is as it was (each setup command and each drag is one step), at most `n` times. */
  const undoAll = async (n) => { for (let i = 0; i < n; i++) await undo(); };

  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  await run("common.Control.zoomFitScreen");
  await sleep(500);
  const G = mm(2.54);
  const X = mm(152.4), Y = mm(127);
  const start = JSON.stringify(((await sch()).sheets ?? []).map((s) => s.id));

  await scenario("S3-2: a sheet's right side is dragged by its middle; the pin on it and the wire ending there follow; one undo step", async (t) => {
    const added = await cmd({ op: "add_sheet", name: "E2E_POINTS", file: "e2e_points.kicad_sch", at: { x: X, y: Y }, size: [16 * G, 8 * G] });
    t(added.ok !== false, `add_sheet: ${added.message ?? "ok"}`);
    await sleep(900);
    const s0 = ((await sch()).sheets ?? []).find((s) => s.name === "E2E_POINTS");
    if (!t(!!s0, "the sheet is there")) return;
    const pin = await cmd({ op: "sch_edit", verb: "add_sheet_pin", sheet: s0.id, name: "E2E_OUT", shape: "output", at: { x: X + 16 * G, y: Y + 3 * G } });
    t(pin.ok !== false, `add_sheet_pin: ${pin.message ?? "ok"}`);
    const wire = await cmd({ op: "add_wire", pts: [{ x: X + 16 * G, y: Y + 3 * G }, { x: X + 20 * G, y: Y + 3 * G }] });
    t(wire.ok !== false, `add_wire: ${wire.message ?? "ok"}`);
    await sleep(900);
    if (!t(await select(s0.id), "the selected sheet shows its edit points")) return;
    t(points().length === 4 && sides().length === 4, `four corners and a handle on each side (${points().length}, ${sides().length})`);
    const right = sides()[1];
    await sDrag(right, [right[0] + 2 * G, right[1] + G]);
    const after = await sch();
    const s1 = after.sheets.find((s) => s.id === s0.id);
    t(s1 && s1.size[0] === 18 * G && s1.size[1] === 8 * G && s1.at[0] === X && s1.at[1] === Y, `the sheet is 2 grid steps wider, its corner and height kept (${JSON.stringify(s1 && [s1.at, s1.size])})`);
    const p1 = s1?.pins.find((p) => p.name === "E2E_OUT");
    t(p1 && p1.at[0] === X + 18 * G && p1.at[1] === Y + 3 * G, `the pin is on the new right side (${JSON.stringify(p1?.at)})`);
    const w1 = after.wires.find((w) => w.pts.some((q) => q[0] === X + 20 * G && q[1] === Y + 3 * G));
    t(w1 && w1.pts.some((q) => q[0] === X + 18 * G && q[1] === Y + 3 * G), `the wire's end followed the pin (${JSON.stringify(w1?.pts)})`);
    // the bottom-left corner dragged up and right across the sheet: it stops at 500 mils wide and below the pin (with its text) on the 50 mil grid
    if (t(await select(s0.id), "the sheet is selected again")) {
      const bl = points()[2];
      await sDrag(bl, [X + 30 * G, Y - 4 * G]);
      const s2 = (await sch()).sheets.find((s) => s.id === s0.id);
      t(s2 && s2.size[0] === mm(12.7) && s2.at[1] === Y && s2.at[1] + s2.size[1] > Y + 3 * G && (s2.at[1] + s2.size[1]) % mm(1.27) === 0, `the corner stopped at the smallest sheet the pins allow (${JSON.stringify(s2 && [s2.at, s2.size])})`);
      await undo();
    }
    await undo();
    const back = (await sch()).sheets.find((s) => s.id === s0.id);
    t(back && back.size[0] === 16 * G && back.pins[0]?.at[0] === X + 16 * G, `one undo takes the side's drag back (${JSON.stringify(back && [back.size, back.pins[0]?.at])})`);
    await undoAll(3);
    t(JSON.stringify(((await sch()).sheets ?? []).map((s) => s.id)) === start, "the setup is undone");
  });

  await scenario("S3-4: an image's corner scales it about its centre; one undo step", async (t) => {
    const c = document.createElement("canvas");
    c.width = 120;
    c.height = 60;
    const ctx = c.getContext("2d");
    ctx.fillStyle = "#c33";
    ctx.fillRect(0, 0, 120, 60);
    const data = c.toDataURL("image/png").split(",")[1];
    const before = new Set(((await sch()).graphics ?? []).map((g) => g.id));
    const r = await cmd({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "image", at: { x: X, y: Y }, scale_ppm: 1000000, data } } });
    t(r.ok !== false, `add_graphic image: ${r.message ?? "ok"}`);
    await sleep(900);
    const g = ((await sch()).graphics ?? []).find((x) => !before.has(x.id));
    if (!t(g && g.shape.type === "image", "the image is there")) return;
    if (!t(await select(g.id), "the selected image shows its edit points")) return;
    const pts = points();
    t(pts.length === 5 && pts[4][0] === X && pts[4][1] === Y, `four corners and the transform origin on the centre (${JSON.stringify(pts)})`);
    // the bottom-right corner pulled out to twice its distance from the centre (on the grid)
    const br = pts[3];
    const to = [X + Math.round((2 * (br[0] - X)) / mm(1.27)) * mm(1.27), Y + Math.round((2 * (br[1] - Y)) / mm(1.27)) * mm(1.27)];
    await sDrag(br, to);
    const s = ((await sch()).graphics ?? []).find((x) => x.id === g.id);
    const scale = s?.shape.scale_ppm ?? 0;
    t(scale > 1_700_000 && scale < 2_300_000 && s.shape.at.x === X && s.shape.at.y === Y, `scaled about twice, the centre kept (${JSON.stringify(s?.shape && { at: s.shape.at, scale_ppm: s.shape.scale_ppm })})`);
    await undo();
    t((((await sch()).graphics ?? []).find((x) => x.id === g.id)?.shape.scale_ppm ?? 1000000) === 1000000, "one undo puts the scale back");
    await undo();
  });

  await scenario("S3-4: a graphic line's end is dragged and the line ending there follows; one undo step", async (t) => {
    const a = await cmd({ op: "add_sch_line", pts: [{ x: X, y: Y }, { x: X + 4 * G, y: Y }] });
    const b = await cmd({ op: "add_sch_line", pts: [{ x: X + 4 * G, y: Y }, { x: X + 4 * G, y: Y + 4 * G }] });
    t(a.ok !== false && b.ok !== false, `add_sch_line twice: ${a.message ?? "ok"} ${b.message ?? "ok"}`);
    await sleep(900);
    const lines = (await sch()).lines ?? [];
    const la = lines.find((l) => l.pts[0][0] === X && l.pts[0][1] === Y && l.pts[1][0] === X + 4 * G);
    const lb = lines.find((l) => l.pts[0][0] === X + 4 * G && l.pts[0][1] === Y && l.pts[1][1] === Y + 4 * G);
    if (!t(la && lb, "both lines are there")) return;
    if (!t(await select(la.id), "the selected line shows its edit points")) return;
    const pts = points();
    t(pts.length === 2 && sides().length === 0, `its start and its end (${JSON.stringify(pts)})`);
    await sDrag(pts[1], [X + 6 * G, Y - 2 * G]);
    const now = (await sch()).lines ?? [];
    const na = now.find((l) => l.id === la.id);
    const nb = now.find((l) => l.id === lb.id);
    t(na && na.pts[1][0] === X + 6 * G && na.pts[1][1] === Y - 2 * G && na.pts[0][0] === X, `the end moved, the start stayed (${JSON.stringify(na?.pts)})`);
    t(nb && nb.pts[0][0] === X + 6 * G && nb.pts[0][1] === Y - 2 * G && nb.pts[1][1] === Y + 4 * G, `the other line's start followed (${JSON.stringify(nb?.pts)})`);
    await undo();
    const back = (await sch()).lines ?? [];
    t(back.find((l) => l.id === la.id)?.pts[1][0] === X + 4 * G && back.find((l) => l.id === lb.id)?.pts[0][0] === X + 4 * G, "one undo puts both back");
    // the end dragged onto the start: KiCad commits the zero-length graphic line and its clean-up keeps it (only wires and buses are cleaned up)
    if (t(await select(la.id), "the line is selected again")) {
      const [, end] = points();
      await sDrag(end, [X, Y]);
      const z = ((await sch()).lines ?? []).find((l) => l.id === la.id);
      t(z && z.pts[0][0] === X && z.pts[0][1] === Y && z.pts[1][0] === X && z.pts[1][1] === Y, `the line is kept with its ends on one point (${JSON.stringify(z?.pts)})`);
      const zb = ((await sch()).lines ?? []).find((l) => l.id === lb.id);
      t(zb && zb.pts[0][0] === X && zb.pts[0][1] === Y, `the connected line's start followed (${JSON.stringify(zb?.pts)})`);
      await undo();
      t(((await sch()).lines ?? []).find((l) => l.id === la.id)?.pts[1][0] === X + 4 * G, "one undo gives the line its length back");
    }
    await undoAll(2);
  });

  await scenario("S3-4: a table cell's right-side point sets its column's width; one undo step", async (t) => {
    const before = new Set(((await sch()).tables ?? []).map((x) => x.id));
    const r = await cmd({ op: "sch_table", verb: "draw", layer: "", at: { x: X, y: Y }, cols: 2, rows: 2, cell_w_nm: 4 * G, cell_h_nm: 2 * G, line_width_nm: 0 });
    t(r.ok !== false, `sch_table draw: ${r.message ?? "ok"}`);
    await sleep(900);
    const tbl = ((await sch()).tables ?? []).find((x) => !before.has(x.id));
    if (!t(!!tbl, "the table is there")) return;
    const cell = tbl.cells.find((c) => c.row === 0 && c.col === 0);
    if (!t(await select(cell.id), "the selected cell shows its edit points")) return;
    const pts = points();
    t(pts.length === 2 && pts[0][0] === X + 4 * G && pts[0][1] === Y + G, `the middle of its right side and of its bottom (${JSON.stringify(pts)})`);
    await sDrag(pts[0], [X + 6 * G, Y + G]);
    const now = ((await sch()).tables ?? []).find((x) => x.id === tbl.id);
    t(now && now.col_widths[0] === 6 * G && now.col_widths[1] === 4 * G, `the first column is 6 grid steps wide, the second kept (${JSON.stringify(now?.col_widths)})`);
    await undo();
    t(((await sch()).tables ?? []).find((x) => x.id === tbl.id)?.col_widths[0] === 4 * G, "one undo puts the width back");
    await undo();
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
