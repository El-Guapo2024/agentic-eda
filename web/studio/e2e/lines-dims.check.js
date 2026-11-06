// Line styles and dimensions in the PCB editor (docs/parity/UI-LEFT.md P5 and P13; PARITY-pcb.md section 69), in the page:
//   * P5: a line drawn with Draw Line is made dashed and put on F.SilkS in Shape Properties ("Line style:", `lineTypeNames`): the board has its line style, the
//     derived .kicad_pcb writes `(stroke (width ..) (type dash))`, the Properties panel's Line Style says Dashed, and Undo takes it back.
//   * P13: each kind of dimension drawn with its own click sequence (`DRAWING_TOOL::DrawDimension`): an aligned and an orthogonal one take a third click for their
//     height, a centre mark, a radial one and a leader are done at the second; every motion asks the server for the preview (`POST /api/dimension/preview`); the
//     tool stays armed; a leader's dialog opens at once, and its text frame is set there.
//   * The aligned dimension's text dragged by its handle (`PCB_POINT_EDITOR`): placed by hand from then on (`DIM_TEXT_POSITION::MANUAL`), written as
//     `(text_position_mode 2)` with the text where it was dropped; the crossbar's handle sets the height.
// Run it in the page of a served COPY of a board with an outline and room in it (it draws, edits and undoes): `eda board new examples/ldo.yaml -o <dir>` (a 26 mm
// square). It answers through `window.__linesDimsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__linesDimsCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden page throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const mm = (v) => Math.round(v * 1_000_000);
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const state = () => json("/api/state");
  const boardFile = () => fetch("/api/board.kicad_pcb").then((r) => r.text());
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const waitFor = async (pred, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await pred(); if (v) return v; await sleep(150); } return null; };
  const setValue = (el, value) => {
    const proto = el.tagName === "SELECT" ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event(el.tagName === "SELECT" ? "change" : "input", { bubbles: true }));
  };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** Board millimetres to the page's pixels. */
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + mm(wx) * v.scale, r.top + v.y + mm(wy) * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1) }));
  };
  const moveTo = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { buttons: 0 }); await sleep(150); };
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y, { buttons: 0 });
    await sleep(150);
    fire("pointerdown", x, y);
    await sleep(40);
    fire("pointerup", x, y);
    await sleep(400);
  };
  const dragFromTo = async (a, b) => {
    const [x0, y0] = w2c(a[0], a[1]);
    const [x1, y1] = w2c(b[0], b[1]);
    fire("pointermove", x0, y0, { buttons: 0 });
    await sleep(120);
    fire("pointerdown", x0, y0);
    await sleep(60);
    for (let i = 1; i <= 8; i++) { fire("pointermove", x0 + ((x1 - x0) * i) / 8, y0 + ((y1 - y0) * i) / 8); await sleep(60); }
    await sleep(300);
    fire("pointerup", x1, y1);
    await sleep(800);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(200); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  const dims = async () => (await state()).drawings?.dimensions ?? [];
  // The preview requests the tools make, and whether the server drew them.
  const previews = { asked: 0, answered: 0 };
  const realFetch = window.fetch.bind(window);
  window.fetch = async (url, init) => {
    const r = await realFetch(url, init);
    if (String(url).includes("/api/dimension/preview")) {
      previews.asked++;
      r.clone().json().then((v) => { if (v.ok && v.dimension?.lines?.length) previews.answered++; });
    }
    return r;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);

  await scenario("P5: a line drawn with Draw Line, made dashed on F.SilkS in Shape Properties, is drawn, written and undone as such", async (t) => {
    const before = ((await state()).drawings?.shapes ?? []).length;
    await E.run("pcbnew.InteractiveDrawing.line");
    t(E.state().tool === "draw_segment", `the line tool is armed (${E.state().tool})`);
    await clickAt(3, 3);
    await clickAt(15, 3);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    await leaveTool();
    const shapes = (await state()).drawings?.shapes ?? [];
    const line = shapes.find((s) => s.kind === "segment" && near(s.start[0], mm(3), 50_000) && near(s.end[0], mm(15), 50_000));
    t(shapes.length === before + 1 && !!line, `one line drawn (${shapes.length - before})`);
    if (!line) return;
    t(!line.dash, "a new line is solid");
    await clickAt(9, 3);
    t(E.state().selection.some((s) => s.id === line.id), `the line is selected (${JSON.stringify(E.state().selection)})`);
    await E.run("pcbnew.InteractiveEdit.properties");
    const style = await waitFor(() => tid("shape-line-style"));
    t(!!style, "Shape Properties has the Line style choice");
    if (!style) return;
    t([...style.options].map((o) => o.textContent).join(",") === "Solid,Dashed,Dotted,Dash-Dot,Dash-Dot-Dot", `KiCad's five choices (${[...style.options].map((o) => o.textContent)})`);
    setValue(style, "dash");
    const layerSel = [...document.querySelectorAll(".dialog select")].find((s) => [...s.options].some((o) => o.value === "F.SilkS"));
    if (layerSel) setValue(layerSel, "F.SilkS");
    await sleep(100);
    [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Apply")?.click();
    const dashed = await waitFor(async () => ((await state()).drawings?.shapes ?? []).find((s) => s.id === line.id && s.dash === "dash"));
    t(!!dashed && dashed.layer === "F.SilkS", `the line is dashed on F.SilkS (${JSON.stringify(dashed && { dash: dashed.dash, layer: dashed.layer })})`);
    const file = await boardFile();
    t(/\(gr_line \(start 3 3\) \(end 15 3\) \(stroke \(width [0-9.]+\) \(type dash\)\)/.test(file), "the derived .kicad_pcb writes (type dash)");
    // Undo takes the style back (one step with the layer); redo puts it back for the rest of the check.
    await E.run("common.Interactive.undo");
    await sleep(700);
    const undone = ((await state()).drawings?.shapes ?? []).find((s) => s.id === line.id);
    t(!!undone && !undone.dash && undone.layer === line.layer, `undo gives the solid line on its layer back, one step (${JSON.stringify(undone && { dash: undone.dash, layer: undone.layer })}, was ${line.layer})`);
    await E.run("common.Interactive.redo");
    await sleep(700);
    const again = ((await state()).drawings?.shapes ?? []).find((s) => s.id === line.id);
    t(!!again && again.dash === "dash", "redo makes it dashed again");
  });

  const drawn = {};
  const drawDimension = async (t, action, kind, clicks, motion) => {
    const before = await dims();
    const asked = previews.asked;
    await E.run(action);
    t(E.state().tool === "dimension", `${kind}: the dimension tool is armed`);
    for (let i = 0; i < clicks.length; i++) {
      await clickAt(clicks[i][0], clicks[i][1]);
      if (i < clicks.length - 1) {
        // the motion toward the next click: the preview follows it
        await moveTo((clicks[i][0] + clicks[i + 1][0]) / 2 + 0.3, (clicks[i][1] + clicks[i + 1][1]) / 2 + 0.3);
        if (motion) await motion(i);
      }
    }
    const after = await waitFor(async () => { const d = await dims(); return d.length > before.length ? d : null; });
    const made = after?.find((d) => !before.some((b) => b.id === d.id));
    t(!!made && made.kind === kind, `${kind}: drawn in ${clicks.length} clicks (${made?.kind})`);
    t(previews.asked > asked, `${kind}: the preview followed the cursor (${previews.asked - asked} previews)`);
    drawn[kind] = made;
    return made;
  };

  await scenario("P13: an aligned dimension takes its origin, its end and its height; the tool stays armed and the new one is selected", async (t) => {
    const d = await drawDimension(t, "pcbnew.InteractiveDrawing.alignedDimension", "aligned", [[4, 8], [20, 8], [12, 6]]);
    if (!d) return;
    t(near(d.start[0], mm(4), 50_000) && near(d.end[0], mm(20), 50_000), `the feature points (${d.start}, ${d.end})`);
    t(near(d.height, mm(-2), 60_000), `the third click is the height: ${d.height} nm`);
    t(E.state().tool === "dimension", "the tool stays armed for the next one");
    t(!!(await waitFor(() => E.state().selection.some((s) => s.id === d.id))), "the new dimension is selected (selectItem)");
    t(previews.answered > 0, `the server drew the previews (${previews.answered} of ${previews.asked})`);
  });

  await scenario("P13: an orthogonal dimension's height click outside its box, above it, makes it horizontal", async (t) => {
    const d = await drawDimension(t, "pcbnew.InteractiveDrawing.orthogonalDimension", "orthogonal", [[4, 12], [20, 14], [12, 10]]);
    if (!d) return;
    t(d.horizontal === true, `horizontal (${d.horizontal})`);
    t(near(d.height, mm(-2), 60_000), `height from the start across: ${d.height}`);
    t(near(d.measured_value_nm, mm(16), 60_000), `measures the x span: ${d.measured_value_nm}`);
  });

  await scenario("P13: a centre mark and a radial dimension are done at their second click", async (t) => {
    const c = await drawDimension(t, "pcbnew.InteractiveDrawing.centerDimension", "center", [[6, 18], [8, 18]]);
    t(!!c && c.lines.length === 2, `a centre mark's two arms (${c?.lines.length})`);
    const r = await drawDimension(t, "pcbnew.InteractiveDrawing.radialDimension", "radial", [[12, 18], [14, 18]]);
    t(!!r && near(r.measured_value_nm, mm(2), 60_000) && r.text.startsWith("R "), `the radius, prefixed R (${r?.text})`);
    t(!!r?.text_pos, "its text past the knee");
  });

  await scenario("P13: a leader opens its dialog at once; its text frame is set there and drawn", async (t) => {
    const l = await drawDimension(t, "pcbnew.InteractiveDrawing.leader", "leader", [[18, 22], [21, 19]]);
    if (!l) return;
    const ok = await waitFor(() => tid("dim-ok"));
    t(!!ok, "Dimension Properties opened on the leader");
    if (!ok) return;
    const frame = tid("dim-text-frame");
    t(!!frame && [...frame.options].map((o) => o.textContent).join(",") === "None,Rectangle,Circle", "the Text frame choice: None, Rectangle, Circle");
    t(!tid("dim-text-position"), "a leader has no position mode");
    setValue(tid("dim-override"), "NOTE");
    if (frame) setValue(frame, "circle");
    await sleep(100);
    ok.click();
    const framed = await waitFor(async () => (await dims()).find((d) => d.id === l.id && d.text_frame === "circle"));
    t(!!framed && framed.text === "NOTE", `the leader says NOTE in a circle (${framed?.text}, ${framed?.text_frame})`);
    t((framed?.circles ?? []).length === 1, "the circle is drawn");
    t(near(framed?.text_pos?.[0] ?? 0, l.text_pos[0], 10) && near(framed?.text_pos?.[1] ?? 0, l.text_pos[1], 10), "the dialog kept the text where the tool put it");
  });

  await scenario("P13: the aligned dimension's text dragged by its handle is placed by hand; the crossbar handle sets the height; both are written", async (t) => {
    await leaveTool();
    const a0 = (await dims()).find((d) => d.id === drawn.aligned?.id);
    if (!a0) { t(false, "no aligned dimension to move"); return; }
    await clickAt(12, 8 - 2);
    t(E.state().selection.length === 1 && E.state().selection[0].id === a0.id, `the dimension is selected (${JSON.stringify(E.state().selection)})`);
    t(a0.text_position !== "manual", `its text starts in the ${a0.text_position} place`);
    const to = [8, 3];
    const asked = previews.asked;
    await dragFromTo([a0.text_at[0] / 1e6, a0.text_at[1] / 1e6], to);
    const a1 = await waitFor(async () => (await dims()).find((d) => d.id === a0.id && d.text_position === "manual"));
    t(!!a1, "the text is placed by hand (manual)");
    t(previews.asked > asked, "the point editor's drag is previewed");
    if (a1) t(near(a1.text_at[0], mm(to[0]), 80_000) && near(a1.text_at[1], mm(to[1]), 80_000), `the text is where it was dropped (${a1.text_at})`);
    // the crossbar's end handle: 1 mm further from the measured line
    if (a1?.crossbar) {
      await dragFromTo([a1.crossbar[1][0] / 1e6, a1.crossbar[1][1] / 1e6], [a1.crossbar[1][0] / 1e6, a1.crossbar[1][1] / 1e6 - 1]);
      const a2 = await waitFor(async () => (await dims()).find((d) => d.id === a0.id && !near(d.height, a1.height, 100_000)));
      t(!!a2 && near(a2.height, a1.height - mm(1), 80_000), `the height follows the crossbar's handle (${a1.height} -> ${a2?.height})`);
      if (a2) t(a2.text_position === "manual" && near(a2.text_at[1], a1.text_at[1] - mm(1), 80_000), `the manual text moves with the crossbar (${a1.text_at} -> ${a2.text_at})`);
    }
    const file = await boardFile();
    t((file.match(/\(text_position_mode 2\)/g) ?? []).length === 1, "the derived .kicad_pcb has one text placed by hand (text_position_mode 2)");
    t(/\(text_frame 2\)/.test(file), "and the leader's circle frame (text_frame 2)");
    const fresh = (await dims()).find((d) => d.id === a0.id);
    t(fresh?.text_position === "manual", "a fresh read of the board has it manual");
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
  window.fetch = realFetch;
  return check.ok ? "ok" : "failed";
})();
