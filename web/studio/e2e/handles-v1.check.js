// The point editor of every shape and KiCad 10's Draw Arc modes (UI-LEFT-2 SS3, YY1; task-v1-handles):
//   SS3  on the schematic, a rectangle's corner, a circle's radius and a polygon's vertex are dragged by their handles (`SCH_POINT_EDITOR`), each one undo step;
//        Create Corner adds a vertex; Draw Arc in KiCad 25ec2335's start-end-mid mode keeps the clicked ends.
//   YY1  in the Symbol Editor the same handles reshape a rectangle (a side's drag takes the pin on it along: "Keep pins attached when dragging edges"), a circle,
//        a polyline (and Create Corner) and an arc, each one undo step.
//
// Run it in the page of a served board made from examples/ldo.yaml (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --ui web/studio/dist`), on a
// COPY of a board: every schematic scenario undoes what it did; the Symbol Editor scenarios make a library of their own (nickname starting with `Hv`).
// It answers through `window.__handlesCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__handlesCheck = { results, done: false, ok: false, wait: null });
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
    await toSelect();
  };

  // ------------------------------------------------------------------------------------------------------------------------ the schematic's canvas
  const sContainer = () => document.querySelector(".pcb-canvas-container");
  const sFire = (type, x, y, extra = {}) => {
    const rect = sContainer().getBoundingClientRect();
    (sContainer().querySelector("canvas") || sContainer()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const sScreen = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const sClick = async (x, y) => { const [px, py] = sScreen(x, y); sFire("pointermove", px, py); await sleep(250); sFire("pointerdown", px, py, { buttons: 1 }); await sleep(60); sFire("pointerup", px, py); await sleep(900); };
  const sHover = async (x, y) => { const [px, py] = sScreen(x, y); sFire("pointermove", px, py); await sleep(250); };
  const sDrag = async ([x0, y0], [x1, y1]) => {
    const [a, b] = sScreen(x0, y0);
    const [c, d] = sScreen(x1, y1);
    sFire("pointermove", a, b); await sleep(200);
    sFire("pointerdown", a, b, { buttons: 1 }); await sleep(60);
    for (let i = 1; i <= 6; i++) { sFire("pointermove", a + ((c - a) * i) / 6, b + ((d - b) * i) / 6, { buttons: 1 }); await sleep(80); }
    sFire("pointerup", c, d); await sleep(1200);
  };
  const points = () => (sContainer().dataset.editPoints ?? "").split(" ").filter(Boolean).map((s) => s.split(",").map(Number));
  const sides = () => (sContainer().dataset.editLines ?? "").split(" ").filter(Boolean).map((s) => s.split(",").map(Number));
  const graphic = async (id) => (await sch()).graphics?.find((g) => g.id === id);
  /** Add a graphic by the verb and select it: its id. */
  const placed = async (t, shape) => {
    const before = new Set(((await sch()).graphics ?? []).map((g) => g.id));
    const r = await cmd({ op: "sch_edit", verb: "add_graphic", graphic: { shape } });
    t(r.ok !== false, `add_graphic ${shape.type}: ${r.message ?? "ok"}`);
    await sleep(900);
    const g = ((await sch()).graphics ?? []).find((x) => !before.has(x.id));
    if (!g) return null;
    await run("common.InteractiveSelection.selectItems", [g.id]);
    await until(() => (sContainer().dataset.editPoints ?? "") !== "", 3000);
    return g;
  };

  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  const X = mm(152.4), Y = mm(127);
  await run("common.Control.zoomFitScreen");
  await sleep(500);
  const start = await sch();

  await scenario("SS3: a rectangle's corner is dragged by its handle (PinEditedCorner), its side by the side's middle, one undo step each", async (t) => {
    const g = await placed(t, { type: "rectangle", start: { x: X, y: Y }, end: { x: X + mm(10.16), y: Y + mm(5.08) } });
    if (!t(!!g, "the rectangle is there and selected")) return;
    const pts = points();
    t(pts.length === 6, `four corners, the centre and the radius point (${pts.length}: ${JSON.stringify(pts)})`);
    t(sides().length === 4, `a handle on each side (${sides().length})`);
    const br = pts[3];
    await sDrag(br, [br[0] + mm(5.08), br[1] + mm(2.54)]);
    const r = await graphic(g.id);
    t(r && r.shape.end.x === X + mm(15.24) && r.shape.end.y === Y + mm(7.62), `the bottom-right corner moved (${JSON.stringify(r?.shape)})`);
    t(r && r.shape.start.x === X && r.shape.start.y === Y, "the top-left corner stayed");
    // the top side: its middle moved up 2.54 mm and sideways: only the top moves
    const top = sides()[0];
    if (top) await sDrag(top, [top[0] + mm(2.54), top[1] - mm(2.54)]);
    const s = await graphic(g.id);
    t(s && s.shape.start.y === Y - mm(2.54) && s.shape.start.x === X && s.shape.end.x === X + mm(15.24), `the top side moved up, nothing sideways (${JSON.stringify(s?.shape)})`);
    await undo();
    const back = await graphic(g.id);
    t(back && back.shape.start.y === Y && back.shape.end.x === X + mm(15.24), `one undo takes the side's drag back (${JSON.stringify(back?.shape)})`);
    await undo();
    const back2 = await graphic(g.id);
    t(back2 && back2.shape.end.x === X + mm(10.16), "and one more the corner's");
    await undo();
  });

  await scenario("SS3: a circle's radius is dragged by its end point, the centre by its own", async (t) => {
    const g = await placed(t, { type: "circle", center: { x: X, y: Y }, radius_nm: mm(5.08) });
    if (!t(!!g, "the circle is there and selected")) return;
    const pts = points();
    t(pts.length === 2 && pts[1][0] === X + mm(5.08) && pts[1][1] === Y, `the centre and the end on the radius to the right (${JSON.stringify(pts)})`);
    await sDrag(pts[1], [X + mm(7.62), Y]);
    const r = await graphic(g.id);
    t(r && r.shape.radius_nm === mm(7.62) && r.shape.center.x === X, `the radius follows the end (${JSON.stringify(r?.shape)})`);
    await undo();
    t((await graphic(g.id))?.shape.radius_nm === mm(5.08), "one undo puts it back");
    await undo();
  });

  await scenario("SS3: a polygon's vertex is dragged by its handle and Create Corner adds one on the outline", async (t) => {
    const g = await placed(t, { type: "polygon", pts: [{ x: X, y: Y }, { x: X + mm(10.16), y: Y }, { x: X + mm(10.16), y: Y + mm(10.16) }] });
    if (!t(!!g, "the polygon is there and selected")) return;
    const pts = points();
    t(pts.length === 3 && sides().length === 3, `a handle per vertex and per side (${pts.length}, ${sides().length})`);
    await sDrag(pts[1], [X + mm(12.7), Y - mm(2.54)]);
    const r = await graphic(g.id);
    t(r && r.shape.pts[1].x === X + mm(12.7) && r.shape.pts[1].y === Y - mm(2.54), `the vertex moved (${JSON.stringify(r?.shape.pts)})`);
    // Create Corner with the cursor on the first side
    await sHover(X + mm(5.08), Y - mm(1.27));
    const ran = await run("eeschema.PointEditor.addCorner");
    await sleep(900);
    const c = await graphic(g.id);
    t(c && c.shape.pts.length === 4, `a fourth vertex (${ran}; ${JSON.stringify(c?.shape.pts)})`);
    await undo();
    t((await graphic(g.id))?.shape.pts.length === 3, "one undo takes the corner away");
    await undo();
    await undo();
  });

  await scenario("SS3: Draw Arc in KiCad 10's start-end-mid mode keeps the clicked ends; the five modes are in the toolbar's Arc group", async (t) => {
    const before = (await sch()).graphics?.length ?? 0;
    await run("eeschema.InteractiveDrawing.drawArcStartEndMid");
    t(state().tool === "sch_arc", `the arc tool is armed (${state().tool})`);
    t(!![...document.querySelectorAll("button.toolbar-button")].find((b) => /Draw Arc: .*\(Arc:/.test(b.title)), "the toolbar has the Arc group of the modes");
    await sClick(X, Y);
    await sClick(X + mm(10.16), Y);
    await sHover(X + mm(3), Y - mm(2.54));
    await sClick(X + mm(3), Y - mm(2.54));
    const after = await until(async () => { const gs = (await sch()).graphics ?? []; return gs.length === before + 1 ? gs : null; }, 6000);
    const arc = after?.[after.length - 1];
    t(arc?.shape?.type === "arc", `an arc (${JSON.stringify(arc?.shape)})`);
    if (arc?.shape?.type === "arc") {
      const ends = [arc.shape.start, arc.shape.end].map((p) => `${p.x},${p.y}`).sort().join(" ");
      t(ends === [`${X},${Y}`, `${X + mm(10.16)},${Y}`].sort().join(" "), `the ends are the two clicks (${ends})`);
      t(arc.shape.mid.x === X + mm(5.08) && arc.shape.mid.y === Y - mm(2.54), `the middle is the third click on the chord's bisector (${JSON.stringify(arc.shape.mid)})`);
    }
    await run("eeschema.InteractiveDrawing.drawArcCenter");
    await toSelect();
    await undo();
    t(((await sch()).graphics?.length ?? 0) === before, "one undo takes the arc away");
  });

  const mid = await sch();
  results.push({ name: "the sheet is as it began", ok: (mid.graphics?.length ?? 0) === (start.graphics?.length ?? 0), notes: [`graphics ${start.graphics?.length ?? 0} -> ${mid.graphics?.length ?? 0}`] });

  // ------------------------------------------------------------------------------------------------------------------------ the Symbol Editor
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (el, value) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(80);
  };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const libRow = (lib) => [...document.querySelectorAll(".library-tree-group")].find((g) => g.textContent.replace(/[▾▸]/g, "").trim().replace(/\s*\*?\s*[\d…]*$/, "").replace(/^☆ /, "") === lib);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse" })); };
  const dragFromTo = async (from, to) => {
    fire("pointermove", from[0], from[1]); await sleep(60);
    fire("pointerdown", from[0], from[1], 1); await sleep(60);
    fire("pointermove", from[0] + 40 / state().view.scale, from[1] + 40 / state().view.scale, 1); await sleep(60);
    fire("pointermove", to[0], to[1], 1); await sleep(100);
    fire("pointerup", to[0], to[1]); await sleep(900);
  };
  const near = (a, b, tol = 1e-4) => Math.abs(a - b) <= tol;
  const s36 = Date.now().toString(36);
  const LIB = `Hv${s36}Lib`;
  const ID = `${LIB}:HvU`;
  const gid = {};
  const g = async (k) => (await symbol(ID)).graphics.find((x) => x.id === gid[k]);
  const pick = async (k) => { await E.run("common.Interactive.unselectAll"); await E.run("common.InteractiveSelection.selectItems", [gid[k]]); await sleep(400); };

  await scenario("YY1: a symbol with a pin on a rectangle's right side, a circle, a polyline and an arc", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Symbol")?.click();
    await until(() => state().tab === "symbol", 4000);
    await sleep(600);
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog("New Symbol Library"), 4000);
    if (!t(d, "New Symbol Library opens")) return;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), LIB);
    document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
    button(d, "Create").click();
    t(await until(() => !dialog("New Symbol Library"), 6000), `${LIB} made`);
    const r0 = await until(() => libRow(LIB), 6000);
    r0?.click(); await sleep(300);
    await run("eeschema.SymbolLibraryControl.newSymbol");
    if (!t(await until(() => tid("new-symbol"), 3000), "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "HvU");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    const common = { unit: 0, body_style: 1, stroke_mm: 0.254, fill: "none" };
    for (const c of [
      // the pin's root on the rectangle's right side x = 5.08: its point 2.54 mm further right, pointing left
      { op: "add_symbol_pin", lib_id: ID, pin: { number: "1", name: "A", electrical_type: "input", shape: "line", at: { x: 7.62, y: 0 }, angle_deg: 180, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "rectangle", ...common, start: { x: -5.08, y: 2.54 }, end: { x: 5.08, y: -2.54 } } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "circle", ...common, center: { x: 0, y: 10.16 }, radius_mm: 2.54 } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "polyline", ...common, pts: [{ x: -10.16, y: -7.62 }, { x: -5.08, y: -7.62 }, { x: -5.08, y: -12.7 }] } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "arc", ...common, start: { x: 10.16, y: -7.62 }, mid: { x: 12.7, y: -10.16 }, end: { x: 10.16, y: -12.7 } } },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const sym = await symbol(ID);
    for (const k of ["rectangle", "circle", "polyline", "arc"]) gid[k] = sym.graphics.find((x) => x.kind === k)?.id;
    gid.pin = sym.pins[0]?.id;
    t(Object.values(gid).every(Boolean), `ids: ${JSON.stringify(gid)}`);
    await until(() => libRow(LIB), 4000);
    if (!libRow(LIB).textContent.trim().startsWith("▾")) { libRow(LIB).click(); await sleep(300); }
    (await until(() => treeRow(ID), 4000))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await run("common.Control.zoomFitScreen");
    await sleep(600);
    t(state().tab === "symbol", "the symbol is open");
  });

  await scenario("YY1: the rectangle's right side is dragged by its middle and takes the pin on it along; a corner's drag takes none; one undo step", async (t) => {
    await pick("rectangle");
    // internal nm, y down: the right side's middle is (5.08, 0)
    await dragFromTo([5_080_000, 0], [7_620_000, 0]);
    const r = await g("rectangle");
    t(r && near(Math.max(r.start.x, r.end.x), 7.62), `the right side is at 7.62 (${JSON.stringify(r)})`);
    const pin = (await symbol(ID)).pins.find((p) => p.id === gid.pin);
    t(pin && near(pin.at.x, 10.16), `the pin went along (${JSON.stringify(pin?.at)})`);
    await undo();
    const back = await g("rectangle");
    const pin2 = (await symbol(ID)).pins.find((p) => p.id === gid.pin);
    t(back && near(Math.max(back.start.x, back.end.x), 5.08) && pin2 && near(pin2.at.x, 7.62), "one undo puts both back");
    // the top-left corner (-5.08, 2.54 mm up = -2_540_000 nm down)
    await pick("rectangle");
    await dragFromTo([-5_080_000, -2_540_000], [-7_620_000, -5_080_000]);
    const c = await g("rectangle");
    const pin3 = (await symbol(ID)).pins.find((p) => p.id === gid.pin);
    t(c && near(Math.min(c.start.x, c.end.x), -7.62) && near(Math.max(c.start.y, c.end.y), 5.08) && pin3 && near(pin3.at.x, 7.62), `the corner moved, the pin did not (${JSON.stringify(c)})`);
    await undo();
  });

  await scenario("YY1: a circle's radius, a polyline's vertex (and Create Corner) and an arc's end by their handles", async (t) => {
    await pick("circle");
    // the circle's end handle: on the radius to the right of the centre (2.54, -10.16 nm y down)
    await dragFromTo([2_540_000, -10_160_000], [5_080_000, -10_160_000]);
    t(near((await g("circle"))?.radius_mm ?? 0, 5.08), `the radius follows (${JSON.stringify(await g("circle"))})`);
    await undo();
    t(near((await g("circle"))?.radius_mm ?? 0, 2.54), "one undo puts it back");
    await pick("polyline");
    await dragFromTo([-5_080_000, 7_620_000], [-2_540_000, 7_620_000]);
    const p = await g("polyline");
    t(p && near(p.pts[1].x, -2.54) && near(p.pts[1].y, -7.62), `the vertex moved (${JSON.stringify(p?.pts)})`);
    await undo();
    t(near((await g("polyline")).pts[1].x, -5.08), "one undo puts it back");
    // Create Corner from the context menu with the cursor on the first segment (y -7.62 mm up = 7_620_000 nm down)
    await pick("polyline");
    const [cx, cy] = toScreen(-7_620_000, 7_620_000);
    canvasEl().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: cx, clientY: cy, button: 2 }));
    await sleep(500);
    const entry = await until(() => [...document.querySelectorAll("[role=menuitem]")].find((el) => el.textContent.trim().startsWith("Create Corner")), 3000);
    t(!!entry, "the context menu offers Create Corner on the outline");
    entry?.click();
    await sleep(900);
    const pc = await g("polyline");
    t(pc && pc.pts.length === 4 && near(pc.pts[1].x, -7.62) && near(pc.pts[1].y, -7.62), `a vertex is added after the first one (${JSON.stringify(pc?.pts)})`);
    await undo();
    t((await g("polyline")).pts.length === 3, "one undo takes it away");
    await pick("arc");
    await dragFromTo([10_160_000, 12_700_000], [7_620_000, 12_700_000]);
    const a = await g("arc");
    t(a && JSON.stringify(a) !== JSON.stringify({ ...a, end: { x: 10.16, y: -12.7 } }) && !(near(a.end.x, 10.16) && near(a.end.y, -12.7)), `the arc's end moved (${JSON.stringify(a)})`);
    await undo();
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
