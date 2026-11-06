// Schematic polylines drawn open (UI-LEFT-2 SS21; task-v2-teardrops):
//   - a polyline of the sheet is stroked along its open chain, no side from its last point back to its first (`SCH_PAINTER::draw( SCH_SHAPE )` for `SHAPE_T::POLY`:
//     `MakeEffectiveShapes( true )` of the open chain `parseSchPolyLine` makes), its fill still the closed polygon; a rule area is stroked closed;
//   - Create Corner on the sheet is the Symbol Editor's `addCorner`: not offered on an open polyline's closing side (menu and action), offered on a real side, and on a
//     rule area's closing side; Remove Corner takes the vertex under the cursor; each one undo step.
//
// Run it in the page of a served board made from examples/ldo.yaml (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --ui web/studio/dist`), on a
// COPY of a board: every scenario undoes what it did.
// It answers through `window.__schPolylineCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (window.__schPolylineCheck = { results, done: false, ok: false, wait: null });
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
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await toSelect();
  };

  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hover = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const contextMenuAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    fire("pointermove", px, py);
    await sleep(250);
    const rect = container().getBoundingClientRect();
    container().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, button: 2 }));
    await sleep(500);
    return [...document.querySelectorAll("[role=menuitem]")].map((el) => el.textContent.trim());
  };
  const closeMenu = async () => { document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(300); };
  /** The canvas's pixels over the world box `[x0, y0, x1, y1]` (nm): how many are stroke ink (dark), and how many are of the fill colour (yellow). */
  const pixels = ([x0, y0, x1, y1]) => {
    const canvas = container().querySelector("canvas");
    const dpr = canvas.width / canvas.clientWidth;
    const [a, b] = screenOf(x0, y0);
    const [c, d] = screenOf(x1, y1);
    const img = canvas.getContext("2d").getImageData(Math.floor(Math.min(a, c) * dpr), Math.floor(Math.min(b, d) * dpr), Math.max(1, Math.ceil(Math.abs(c - a) * dpr)), Math.max(1, Math.ceil(Math.abs(d - b) * dpr)));
    let ink = 0;
    let yellow = 0;
    let h = 0;
    for (let i = 0; i < img.data.length; i += 4) {
      const [r, g, bl] = [img.data[i], img.data[i + 1], img.data[i + 2]];
      h = (h * 31 + r * 7 + g * 3 + bl) | 0;
      if (r + g + bl < 300) ink++;
      if (r > 180 && g > 180 && bl < 120) yellow++;
    }
    return { ink, yellow, hash: h };
  };
  const settled = async (box) => { let a = pixels(box); for (let i = 0; i < 8; i++) { await sleep(700); const b = pixels(box); if (b.hash === a.hash) return b; a = b; } return a; };
  const graphic = async (id) => (await sch()).graphics?.find((g) => g.id === id);
  /** Add a graphic by the verb: its id (unselected). */
  const placed = async (t, g) => {
    const before = new Set(((await sch()).graphics ?? []).map((x) => x.id));
    const r = await cmd({ op: "sch_edit", verb: "add_graphic", graphic: g });
    t(r.ok !== false, `add_graphic ${g.shape.type}: ${r.message ?? "ok"}`);
    await sleep(900);
    return ((await sch()).graphics ?? []).find((x) => !before.has(x.id)) ?? null;
  };
  const select = async (id) => { await E.run("common.Interactive.unselectAll"); await run("common.InteractiveSelection.selectItems", [id]); await until(() => (container().dataset.editPoints ?? "") !== "", 3000); };
  const unselect = async () => { await E.run("common.Interactive.unselectAll"); await sleep(500); };

  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  const X = mm(152.4), Y = mm(127), S = mm(10.16);
  const square = [{ x: X, y: Y }, { x: X + S, y: Y }, { x: X + S, y: Y + S }, { x: X, y: Y + S }];
  await run("common.Control.zoomFitScreen");
  await sleep(500);
  // zoom about the square's middle, so a fraction of a millimetre is pixels apart
  for (let i = 0; i < 7; i++) { await hover(X + S / 2, Y + S / 2); await run("common.Control.zoomIn"); }
  await sleep(600);
  // the probes: the closing side (x = X, the lower part), the first side (y = Y, the right part), just inside the closing side; the pointer parked where its
  // crosshair crosses none of them
  const closingSide = [X - mm(0.4), Y + mm(6), X + mm(0.4), Y + mm(9)];
  const firstSide = [X + mm(6), Y - mm(0.4), X + mm(9), Y + mm(0.4)];
  const insideClosing = [X + mm(0.6), Y + mm(6), X + mm(1.6), Y + mm(9)];
  const park = () => hover(X + mm(4.5), Y + mm(2.5));

  await scenario("SS21: a polyline of the sheet is stroked open (no side back to its first point), its fill the closed polygon; a rule area is stroked closed", async (t) => {
    const g = await placed(t, { shape: { type: "polygon", pts: square } });
    if (!t(!!g, "the polyline is there")) return;
    await unselect();
    await park();
    const first = await settled(firstSide);
    const closing = await settled(closingSide);
    t(first.ink > 0, `its first side is drawn (${first.ink} stroke pixels)`);
    t(closing.ink === 0, `no side from its last point back to its first (${closing.ink} stroke pixels there)`);
    // filled: the fill reaches the closing side, which still has no stroke
    const r = await cmd({ op: "sch_edit", verb: "edit_graphic", id: g.id, graphic: { ...g, fill: "color", fill_color: { r: 255, g: 255, b: 0, a: 255 } } });
    t(r.ok !== false, `filled with a colour: ${r.message ?? "ok"}`);
    await sleep(900);
    await park();
    const filled = await settled(insideClosing);
    const filledClosing = await settled(closingSide);
    t(filled.yellow > 0, `the fill is the closed polygon, up to the closing side (${filled.yellow} fill pixels inside it)`);
    t(filledClosing.ink === 0, `and the closing side is still not stroked (${filledClosing.ink} stroke pixels)`);
    await undo();
    t(!(await graphic(g.id))?.fill || (await graphic(g.id)).fill === "none", "one undo takes the fill away");
    await undo();
    t(!(await graphic(g.id)), "one undo takes the polyline away");
    // a rule area: closed
    const a = await placed(t, { shape: { type: "rule_area", pts: square }, line_style: "dash" });
    if (!t(!!a, "the rule area is there")) return;
    await unselect();
    await park();
    const areaClosing = await settled(closingSide);
    t(areaClosing.ink > 0, `a rule area's closing side is drawn (${areaClosing.ink} stroke pixels)`);
    await undo();
    t(!(await graphic(a.id)), "one undo takes the rule area away");
  });

  await scenario("SS21: Create Corner on the sheet is not offered on an open polyline's closing side; on a real side it adds a vertex after that side's start", async (t) => {
    const g = await placed(t, { shape: { type: "polygon", pts: square } });
    if (!t(!!g, "the polyline is there")) return;
    await select(g.id);
    const onClosing = await contextMenuAt(X, Y + S / 2 + mm(1));
    t(!onClosing.some((l) => l.startsWith("Create Corner")), `the menu has no Create Corner on the closing side (${onClosing.join(" | ")})`);
    await closeMenu();
    await select(g.id);
    await hover(X, Y + S / 2 + mm(1));
    const refused = await run("eeschema.PointEditor.addCorner");
    t(refused.ok === false, `nor is the action there (${JSON.stringify(refused)})`);
    t((await graphic(g.id))?.shape.pts.length === 4, "the polyline is as it was");
    const onSide = await contextMenuAt(X + S / 2 + mm(1), Y);
    t(onSide.some((l) => l.startsWith("Create Corner")), `on its first side the menu has Create Corner (${onSide.join(" | ")})`);
    await closeMenu();
    await select(g.id);
    await hover(X + S / 2 + mm(1.27), Y);
    const ran = await run("eeschema.PointEditor.addCorner");
    await sleep(600);
    const c = await graphic(g.id);
    t(c && c.shape.pts.length === 5 && c.shape.pts[1].y === Y && c.shape.pts[1].x > X && c.shape.pts[1].x < X + S, `a vertex after the first (${ran.ok}; ${JSON.stringify(c?.shape.pts)})`);
    await undo();
    t((await graphic(g.id))?.shape.pts.length === 4, "one undo takes the corner away");
    await undo();
    t(!(await graphic(g.id)), "the polyline goes with the next undo");
  });

  await scenario("SS21: a rule area's closing side takes a corner (it is closed); Remove Corner takes the vertex under the cursor, one undo step", async (t) => {
    const a = await placed(t, { shape: { type: "rule_area", pts: square }, line_style: "dash" });
    if (!t(!!a, "the rule area is there")) return;
    await select(a.id);
    await hover(X, Y + S / 2 + mm(1.27));
    const ran = await run("eeschema.PointEditor.addCorner");
    await sleep(600);
    const c = await graphic(a.id);
    t(c && c.shape.pts.length === 5 && c.shape.pts[4].x === X, `a fifth vertex, on the closing side, last (${ran.ok}; ${JSON.stringify(c?.shape.pts)})`);
    await undo();
    t((await graphic(a.id))?.shape.pts.length === 4, "one undo takes it away");
    await undo();
    // Remove Corner on a polyline's vertex
    const g = await placed(t, { shape: { type: "polygon", pts: square } });
    if (!t(!!g, "the polyline is there")) return;
    await select(g.id);
    const menu = await contextMenuAt(X + S, Y + S);
    t(menu.some((l) => l.startsWith("Remove Corner")), `on a vertex the menu has Remove Corner (${menu.join(" | ")})`);
    await closeMenu();
    await select(g.id);
    await hover(X + S, Y + S);
    const removed = await run("eeschema.PointEditor.removeCorner");
    await sleep(600);
    const r = await graphic(g.id);
    t(r && r.shape.pts.length === 3 && !r.shape.pts.some((p) => p.x === X + S && p.y === Y + S), `that vertex goes (${removed.ok}; ${JSON.stringify(r?.shape.pts)})`);
    await undo();
    t((await graphic(g.id))?.shape.pts.length === 4, "one undo puts it back");
    await undo();
    t(!(await graphic(g.id)), "the polyline goes with the next undo");
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
