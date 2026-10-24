// Pad shapes check (GAPS.md items 9, 13 and 19 and PARITY-pcb.md section 27): the Pad Properties dialog of a pad on the board takes KiCad's nine shape choices -- circular, oval,
// rectangular, trapezoidal, rounded rectangle, chamfered rectangle, chamfered with other corners rounded, custom with a circular or a rectangular base --, the fields each
// needs (a trapezoid's delta and axis, the corner and chamfer sizes and the chamfered corners, a custom pad's zone knockout and its list of basic shapes: add, edit, delete),
// and what it sends is one undoable `edit_board_pad`. The state then carries the pad's shape, its delta, chamfer and basic shapes and the outline of its copper
// (`outline`: the contours the DRC measures), which the canvas draws.
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY
// of a board (it edits and undoes, and it leaves Strict off while it runs). It makes nothing that stays: every edit is undone and the board compared with how it was.
//
// It needs three placed footprints with a pad numbered 1 (it uses the first three that are placed, `U1`/`R1`/`C1` ... in examples/mcu_board_30plus.yaml after
// `eda board place U1 --at 17,17`, `R1 --at 27,17`, `C1 --at 8,17`), and it answers through `window.__padShapesCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __padShapesCheck.wait(40000)` until `.done` (a run takes under a minute). In a hidden pane the browser stops timers; the script then waits with a
// fetch loop.
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, open the dialog with
// `pcbnew.InteractiveEdit.properties`), the dialog's controls (their `data-testid`s) and the Properties grid's own inputs.
// run-all-checks: board=mcu30-placed   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__padShapesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const padOf = async (id) => (await state()).parts.find((p) => p.ref === id.split(".")[0])?.pads?.find((p) => p.id === id);
  const placedRefs = async () => (await state()).parts.filter((p) => p.placed && (p.pads ?? []).some((q) => q.num === "1")).map((p) => p.ref);
  /** Every pad's shape-bearing facts, to compare the board with how it was. */
  const snapshot = async () => JSON.stringify((await state()).parts.filter((p) => p.placed).map((p) => [p.ref, (p.pads ?? []).map((q) => [q.id, q.shape, q.size, q.ratio, q.delta, q.chamfer_ratio, q.chamfer_corners, q.custom, q.edit])]));
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (id, v) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); setValue(el, v); await sleep(50); };
  const click = async (id) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); el.click(); await sleep(90); };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const openDialog = async (padId) => { await select([padId]); await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(400); };
  const ok = async () => { await click("board-pad-properties-ok"); await sleep(1300); };
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };
  const area = (c) => Math.abs(c.reduce((s, p, i) => s + (p[0] * c[(i + 1) % c.length][1] - c[(i + 1) % c.length][0] * p[1]), 0) / 2);

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  document.querySelector('button[aria-label="Show Properties"]')?.click();
  await sleep(300);
  const refs = await placedRefs();
  const [a, b, c] = refs.slice(0, 3).map((r) => `${r}.1`);
  const original = await snapshot();
  const errorsFrom = Date.now();

  await scenario("the dialog offers KiCad's nine shapes, in its words", async (t) => {
    await openDialog(a);
    t(!!tid("board-pad-properties"), "the dialog is open");
    const opts = [...tid("pad-shape").options].map((o) => o.textContent);
    t(JSON.stringify(opts) === JSON.stringify(["Circular", "Oval", "Rectangular", "Trapezoidal", "Rounded rectangle", "Chamfered rectangle", "Chamfered with other corners rounded", "Custom (circular base)", "Custom (rectangular base)"]), `choices: ${opts.join(" | ")}`);
    await type("pad-shape", "trapezoid");
    t(!!tid("pad-trap-delta") && !!tid("pad-trap-axis"), "a trapezoid has a delta and an axis");
    await type("pad-shape", "chamfered_rect");
    t(!!tid("pad-chamfer-ratio") && !!tid("pad-chamfer-tl") && !tid("pad-corner-ratio"), "a chamfered rectangle has a chamfer size and four corners, no corner size");
    t(tid("pad-chamfer-tl").checked && ["tr", "bl", "br"].every((k) => !tid(`pad-chamfer-${k}`).checked), "one corner chamfered by default (the top left)");
    t(near(Number(tid("pad-chamfer-ratio").value), 20, 0.01), `20% by default: ${tid("pad-chamfer-ratio").value}`);
    await type("pad-shape", "chamfered_round_rect");
    t(!!tid("pad-corner-ratio") && !!tid("pad-chamfer-ratio"), "the chamfered one with rounded corners has both sizes");
    await type("pad-shape", "custom_circle");
    t(!!tid("pad-knockout") && !!tid("pad-prim-add"), "a custom pad has a zone knockout and its basic shapes");
    t(tid("pad-size-y") === null, "a circular base has one size (a diameter)");
    await type("pad-shape", "custom_rect");
    t(!!tid("pad-size-y"), "a rectangular base has two");
  });

  await scenario("a rectangle becomes a trapezoid: the shape, its delta and the copper the DRC measures, undone", async (t) => {
    const before = await padOf(a);
    await openDialog(a);
    await type("pad-shape", "trapezoid");
    await type("pad-trap-delta", 0.3);
    await type("pad-trap-axis", "vertical");
    await ok();
    const after = await padOf(a);
    t(after.shape === "trapezoid" && JSON.stringify(after.delta) === "[0,300000]", `shape ${after.shape}, delta ${JSON.stringify(after.delta)}`);
    t(after.edit?.trapezoid_delta?.[1] === 300_000 && !("size" in after.edit), `the edit holds the delta only: ${JSON.stringify(after.edit)}`);
    const [contour] = after.outline ?? [];
    t(contour?.length === 4, `the outline is four corners: ${JSON.stringify(contour)}`);
    if (contour) {
      const ys = [...new Set(contour.map((p) => p[1]))].sort((x, y) => x - y);
      const top = contour.filter((p) => p[1] === ys[0]);
      const bottom = contour.filter((p) => p[1] === ys[1]);
      const len = (side) => Math.abs(side[0][0] - side[1][0]);
      // KiCad's delta: half of it squeezes one side and half expands the other, so the two parallel sides differ by twice the delta.
      t(near(len(bottom) - len(top), 600_000, 2000), `the sides differ by twice the delta: ${len(bottom)} and ${len(top)} um`);
      t(near(area(contour), (len(bottom) + len(top)) / 2 * (ys[1] - ys[0]), 3), `its area is the trapezoid's: ${Math.round(area(contour))} um2`);
    }
    t(after.w > before.w, `the box about the pad holds the wide side: ${before.w} -> ${after.w}`);
    await undo();
    const undone = await padOf(a);
    t(undone.shape === before.shape && !undone.outline && !undone.edited, `undone: ${undone.shape}`);
  });

  await scenario("a chamfered rectangle with rounded corners: defaults, limits and the cut corners", async (t) => {
    await openDialog(b);
    await type("pad-shape", "chamfered_round_rect");
    t(near(Number(tid("pad-corner-ratio").value), 25, 0.01), `corner size 25% (the IPC ratio): ${tid("pad-corner-ratio").value}`);
    await type("pad-corner-ratio", 20);
    await type("pad-chamfer-ratio", 95);
    t(near(Number(tid("pad-chamfer-ratio").value), 80, 0.01), `a chamfer past what the corners leave is cut to 80%: ${tid("pad-chamfer-ratio").value}`);
    await type("pad-chamfer-ratio", 30);
    await click("pad-chamfer-br");
    await click("pad-chamfer-tr");
    await type("pad-chamfer-ratio", 60);
    t(["tl", "tr", "bl", "br"].map((k) => tid(`pad-chamfer-${k}`).checked).join("/") === "false/false/false/true", "past half the side the neighbour of a cut corner is cleared, in the dialog's order");
    t(tid("pad-chamfer-tr").disabled && tid("pad-chamfer-bl").disabled, "and cannot be cut");
    await type("pad-chamfer-ratio", 30);
    await ok();
    const p = await padOf(b);
    t(p.shape === "chamfered_rect" && p.ratio === 0.2 && p.chamfer_ratio === 0.3 && p.chamfer_corners?.bottom_right === true && p.chamfer_corners?.top_right === false, `${p.shape}, corner ${p.ratio}, chamfer ${p.chamfer_ratio}, corners ${JSON.stringify(p.chamfer_corners)}`);
    const [contour] = p.outline ?? [];
    t(contour?.length > 8, `the outline has the arcs of the rounded corners and the cut: ${contour?.length} points`);
    // KiCad's own numbers: the polygon of a 0.8 x 0.95 mm pad, 0.3 of the 0.8 shorter side chamfered at one corner, 0.2 of it rounded at the others.
    const r = 0.2 * 800_000, ch = 0.3 * 800_000;
    const exact = 800_000 * 950_000 - ch * ch / 2 - 3 * (1 - Math.PI / 4) * r * r;
    t(contour && Math.abs(area(contour) - exact) / exact < 0.01, `the area is the rectangle less the cut and the three corners: ${Math.round(area(contour))} against ${Math.round(exact)}`);
    await undo();
    t(!(await padOf(b)).edited, "undone");
  });

  await scenario("a custom pad: add a line, a ring and a polygon, edit and delete them, and the copper is their union", async (t) => {
    await openDialog(c);
    await type("pad-shape", "custom_circle");
    await type("pad-prim-kind", "segment");
    await click("pad-prim-add");
    t(!!tid("pad-prim-editor"), "Add opens the basic shape's editor");
    await type("pad-prim-field-end.x", 0);
    await click("pad-prim-ok");
    t(/line needs a length/i.test(tid("pad-prim-error")?.textContent ?? ""), `a line with no length is refused: ${tid("pad-prim-error")?.textContent}`);
    await type("pad-prim-field-end.x", 1.5);
    await type("pad-prim-field-stroke_width", 0.3);
    await click("pad-prim-ok");
    await type("pad-prim-kind", "circle");
    await click("pad-prim-add");
    await type("pad-prim-field-center.x", 1.5);
    await type("pad-prim-field-radius", 0.8);
    await type("pad-prim-field-stroke_width", 0.3);
    await type("pad-prim-fill", "none");
    await click("pad-prim-ok");
    await type("pad-prim-kind", "polygon");
    await click("pad-prim-add");
    const pts = () => document.querySelectorAll('[data-testid="pad-prim-points"] > div').length;
    t(pts() === 3, "a new polygon is a triangle");
    await click("pad-prim-point-add");
    t(pts() === 4, "a point can be added");
    await click("pad-prim-point-3-delete");
    await click("pad-prim-point-2-delete");
    t(pts() === 3, "and a polygon keeps three points at least");
    await click("pad-prim-ok");
    t(document.querySelectorAll('[data-testid^="pad-prim-row-"]').length === 3, "three basic shapes are listed");
    await click("pad-prim-dup-0");
    t(document.querySelectorAll('[data-testid^="pad-prim-row-"]').length === 4, "Duplicate adds a copy");
    await click("pad-prim-delete-1");
    await click("pad-prim-delete-2");
    t(document.querySelectorAll('[data-testid^="pad-prim-row-"]').length === 2, "Delete takes one away: the copy and the polygon go, the line and the ring stay");
    await click("pad-prim-edit-1");
    t(!!tid("pad-prim-field-center.x"), "Edit opens the one it names (the ring)");
    await click("pad-prim-cancel");
    await type("pad-knockout", "convex_hull");
    await ok();
    const p = await padOf(c);
    t(p.shape === "custom" && p.custom?.anchor === "circle" && p.custom?.zone === "convex_hull" && p.custom?.primitives?.length === 2, `${p.shape}, ${p.custom?.anchor}, ${p.custom?.zone}, ${p.custom?.primitives?.map((q) => q.shape.kind)}`);
    t((p.outline ?? []).length === 2, `one outline and one hole (the ring's): ${p.outline?.map((q) => q.length)}`);
    t(p.w > 4_000_000, `the box about the pad holds the ring: ${p.w} x ${p.h} um`);
    // The state is what the next dialog opens on.
    await openDialog(c);
    t(tid("pad-shape").value === "custom_circle" && document.querySelectorAll('[data-testid^="pad-prim-row-"]').length === 2 && tid("pad-knockout").value === "convex_hull", "the dialog opens on the custom pad as it is");
    await closeDialogs();
    await undo();
    t(!(await padOf(c)).edited, "undone");
  });

  await scenario("the Properties panel offers the seven pad shapes and a shape changed there is one undoable edit", async (t) => {
    await select([a]);
    const sel = document.querySelector('.prop-row[data-prop="Pad Shape"] [data-prop-input]');
    t(!!sel, "the Pad Shape row");
    if (!sel) return;
    const opts = [...sel.options].map((o) => o.textContent);
    t(JSON.stringify(opts) === JSON.stringify(["Circle", "Rectangle", "Oval", "Trapezoid", "Rounded rectangle", "Chamfered rectangle", "Custom"]), `choices: ${opts.join(" | ")}`);
    setValue(sel, "chamfered_rect");
    await sleep(1300);
    const p = await padOf(a);
    t(p.shape === "chamfered_rect" && !!p.outline, `${p.shape}, outline ${!!p.outline}`);
    t(!!document.querySelector('.prop-row[data-prop="Corner Radius Ratio"]'), "a chamfered rectangle has a corner radius ratio too (`PadHasMeaningfulRoundingRadius`)");
    await undo();
    t(!(await padOf(a)).edited, "undone");
  });

  await scenario("the board is as it was and nothing logged an error", async (t) => {
    t((await snapshot()) === original, "every pad is as it was");
    const errs = __eda.errors(errorsFrom);
    t(errs.length === 0, `no errors: ${errs.map((e) => e.message).join("; ")}`);
  });

  if (strictWas && strict && !strict.checked) strict.click();
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
