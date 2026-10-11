// Footprint graphics check (GAPS.md items 2 and 25, PARITY-pcb.md section 28): a placed footprint carries the silkscreen outline, the pin-1 mark, the fab body and the texts of its
// footprint (`parts[].graphics`, `parts[].texts` in `GET /api/state`, board space), and the canvas draws them -- each on its layer, in that layer's colour, under the Appearance
// panel's rows (the layer's switch, Footprints Front, Footprint Text) -- a click on one of their strokes picks the footprint, and a turn and a flip of the footprint carry them with it
// (the layers go to the other side, the text turns and is mirrored).
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board
// (it selects, turns and flips a footprint and undoes both, and it switches Appearance rows off and back on). It makes nothing that stays: it compares the board with how it was.
//
// It needs placed footprints whose packages are KiCad's (the examples' `0603`, `SOIC-8`: `eda board new examples/opamp_filter.yaml`, then `eda board place U1 --at 9,9` ...) and the
// installed KiCad footprint libraries (their silkscreen is what the built-in packages are given). It answers through `window.__fpGraphicsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __fpGraphicsCheck.wait(40000)` until `.done` (a run takes about ten seconds).
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, `pcbnew.InteractiveEdit.rotateCcw` / `flip`, undo with `common.Interactive.undo`, the Appearance
// rows through `studio.Appearance.op`), the canvas's own pixels and a synthetic click on the canvas (pointer events at the position a stroke is drawn at).
(async () => {
  const results = [];
  const check = (window.__fpGraphicsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const placed = async () => (await state()).parts.filter((p) => p.placed && (p.graphics ?? []).length > 0);
  const canvas = () => document.querySelector(".pcb-canvas-container canvas") ?? [...document.querySelectorAll("canvas")].find((c) => c.getBoundingClientRect().width > 300);
  /** Where board `(x, y)` µm is on the canvas, in the canvas's own pixels (the backing store: the device pixel ratio is in it). */
  const toPixel = (x, y) => {
    const c = canvas();
    const r = c.getBoundingClientRect();
    const v = __eda.state().view;
    const k = c.width / r.width;
    return { px: (v.x + x * v.scale) * k, py: (v.y + y * v.scale) * k, client: [r.left + v.x + x * v.scale, r.top + v.y + y * v.scale] };
  };
  const pixel = (x, y) => {
    const c = canvas();
    const { px, py } = toPixel(x, y);
    return [...c.getContext("2d").getImageData(Math.round(px), Math.round(py), 1, 1).data].slice(0, 3);
  };
  /** How many pixels of the canvas are within `tol` of `rgb` in each channel. */
  const count = (rgb, tol = 14) => {
    const c = canvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const near = (a, b, tol = 24) => a.every((v, i) => Math.abs(v - b[i]) <= tol);
  const SILK = [242, 237, 161]; // F_SilkS of the KiCad Default theme
  const FAB = [175, 175, 175]; // F_Fab
  const mid = (g) => (g.kind === "segment" ? [(g.start[0] + g.end[0]) / 2, (g.start[1] + g.end[1]) / 2] : g.kind === "arc" ? g.mid : g.kind === "circle" ? [g.center[0] + (g.end[0] - g.center[0]), g.center[1] + (g.end[1] - g.center[1])] : g.kind === "polygon" ? g.pts[0] : g.kind === "rect" ? [g.start[0], (g.start[1] + g.end[1]) / 2] : g.start);
  const bbox = (u) => {
    const pts = [];
    for (const g of u.graphics) {
      for (const k of ["start", "end", "mid", "center", "c1", "c2"]) if (g[k]) pts.push(g[k]);
      if (g.pts) pts.push(...g.pts);
    }
    const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
    return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
  };
  const layersOf = (u) => [...new Set(u.graphics.map((g) => g.layer))].sort();
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const appearance = (o) => __eda.run("studio.Appearance.op", o);
  const click = async (x, y) => {
    const c = canvas();
    const { client } = toPixel(x, y);
    const fire = (type) => c.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: client[0], clientY: client[1], button: 0, buttons: type === "pointerup" ? 0 : 1, pointerId: 1, pointerType: "mouse", isPrimary: true }));
    await __eda.run("common.InteractiveSelection.clear");
    fire("pointerdown");
    fire("pointerup");
    await sleep(400);
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await __eda.run("common.Control.zoomFitObjects");
  await sleep(500);
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const parts = await placed();
  const u = parts.find((p) => p.graphics.some((g) => g.layer === "F.SilkS") && p.graphics.some((g) => g.layer === "F.Fab")) ?? parts[0];
  const original = JSON.stringify((await state()).parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot, p.side]));

  await scenario("a footprint carries its silkscreen, its fab body and its texts in the board state", async (t) => {
    t(parts.length >= 3, `${parts.length} placed footprints have graphics`);
    t(!!u, "one has both a silkscreen and a fab drawing");
    t(u.graphics.every((g) => typeof g.kind === "string" && typeof g.layer === "string" && g.stroke_width > 0 && !("id" in g)), "a graphic is a shape with its layer and width, no id");
    t(layersOf(u).join() === "F.Fab,F.SilkS", `its layers: ${layersOf(u).join(", ")}`);
    const t0 = (u.texts ?? [])[0];
    t(!!t0 && t0.role === "reference" && t0.layer === "F.Fab" && t0.text === u.ref, `its ${"${REFERENCE}"} is on F.Fab and reads ${t0?.text}`);
    const body = u.body;
    t(Array.isArray(body) && body[2] > body[0] && body[3] > body[1], `its body (the fab box) is ${JSON.stringify(body)}`);
    const bb = bbox(u);
    t(bb[0] <= u.courtyard[2] && bb[2] >= u.courtyard[0], `its drawings lie round its courtyard: ${JSON.stringify(bb)} against ${JSON.stringify(u.courtyard)}`);
  });

  await scenario("the canvas draws them on their layers, in their colours", async (t) => {
    const silk = u.graphics.filter((g) => g.layer === "F.SilkS" && g.kind === "segment").sort((a, b) => Math.hypot(b.end[0] - b.start[0], b.end[1] - b.start[1]) - Math.hypot(a.end[0] - a.start[0], a.end[1] - a.start[1]))[0];
    const fab = u.graphics.filter((g) => g.layer === "F.Fab" && g.kind === "segment").sort((a, b) => Math.hypot(b.end[0] - b.start[0], b.end[1] - b.start[1]) - Math.hypot(a.end[0] - a.start[0], a.end[1] - a.start[1]))[0];
    t(!!silk, "a silkscreen line to look at");
    const at = mid(silk);
    t(near(pixel(...at), SILK), `the pixel on the silkscreen line is ${pixel(...at)} (F.SilkS ${SILK})`);
    if (fab) {
      const f = mid(fab);
      t(near(pixel(...f), FAB, 40), `the pixel on the fab line is ${pixel(...f)} (F.Fab ${FAB})`);
    }
    const silkPixels = count(SILK);
    // The layer's own switch.
    await appearance({ op: "layer", key: "f_silks", visible: false });
    await sleep(300);
    const without = count(SILK);
    t(without < silkPixels * 0.6, `F.SilkS off: ${silkPixels} silkscreen pixels become ${without}`);
    t(!near(pixel(...at), SILK), `and the line's pixel is ${pixel(...at)}`);
    await appearance({ op: "layer", key: "f_silks", visible: true });
    await sleep(300);
    t(count(SILK) >= silkPixels * 0.9, "F.SilkS on again: the drawings are back");
    // Footprints Front takes all of a footprint's drawings with it.
    const fabPixels = count(FAB, 20);
    await appearance({ op: "object", id: "footprints_front", visible: false });
    await sleep(300);
    t(count(SILK) < silkPixels * 0.4 && count(FAB, 20) < fabPixels, `Footprints Front off: the silkscreen (${count(SILK)} pixels) and the fab drawings (${count(FAB, 20)}) are gone from the front footprints`);
    await appearance({ op: "object", id: "footprints_front", visible: true });
    await sleep(300);
    // Footprint Text takes the texts (the reference on F.Fab).
    const before = count(FAB, 20);
    await appearance({ op: "object", id: "footprint_text", visible: false });
    await sleep(300);
    t(count(FAB, 20) < before, `Footprint Text off: fewer fab pixels (${before} -> ${count(FAB, 20)})`);
    await appearance({ op: "object", id: "footprint_text", visible: true });
    await sleep(300);
    t(count(FAB, 20) >= before * 0.95, "Footprint Text on again");
  });

  await scenario("a click on a stroke of the outline picks the footprint, wherever the stroke is", async (t) => {
    const [x0, y0, x1, y1] = u.courtyard;
    const outside = u.graphics.map((g) => ({ g, at: mid(g) })).find(({ at }) => at[0] < x0 || at[0] > x1 || at[1] < y0 || at[1] > y1);
    const target = outside ?? { g: u.graphics[0], at: mid(u.graphics[0]) };
    t(!!outside, `a stroke outside the courtyard (${JSON.stringify(target.g.kind)} on ${target.g.layer} at ${JSON.stringify(target.at)})`);
    await click(...target.at);
    const sel = __eda.state().selection.map((s) => s.id);
    t(sel.length === 1 && sel[0] === u.ref, `the click selects ${u.ref}: ${JSON.stringify(sel)}`);
    // a click on empty board, well away from every footprint, picks nothing
    const bb = parts.map((p) => p.courtyard);
    const free = [0, 0].map(() => 0);
    free[0] = Math.min(...bb.map((b) => b[0])) - 4000;
    free[1] = Math.min(...bb.map((b) => b[1])) - 4000;
    await click(...free);
    t(__eda.state().selection.length === 0, `a click on empty board selects nothing: ${JSON.stringify(__eda.state().selection)}`);
    await __eda.run("common.InteractiveSelection.clear");
  });

  await scenario("a turn and a flip carry the graphics and the texts", async (t) => {
    const get = async () => (await state()).parts.find((p) => p.ref === u.ref);
    const w = (b) => b[2] - b[0], h = (b) => b[3] - b[1];
    const start = await get();
    const b0 = bbox(start);
    await select([u.ref]);
    await __eda.run("pcbnew.InteractiveEdit.rotateCcw");
    await sleep(900);
    const turned = await get();
    const b1 = bbox(turned);
    t(Math.abs(w(b1) - h(b0)) <= 2 && Math.abs(h(b1) - w(b0)) <= 2, `a quarter turn swaps the drawings' width and height: ${w(b0)}x${h(b0)} -> ${w(b1)}x${h(b1)}`);
    const c0 = [(b0[0] + b0[2]) / 2, (b0[1] + b0[3]) / 2];
    t(turned.at[0] === start.at[0] && turned.at[1] === start.at[1], "the footprint did not move");
    t(turned.texts[0].angle === (start.texts[0].angle + 90000) % 360000, `its text turned with it: ${start.texts[0].angle} -> ${turned.texts[0].angle} (counter-clockwise millidegrees)`);
    await __eda.run("pcbnew.InteractiveEdit.flip");
    await sleep(900);
    const flipped = await get();
    t(flipped.side === "bottom", "flipped to the back");
    t(layersOf(flipped).join() === "B.Fab,B.SilkS", `the drawings are on the back layers: ${layersOf(flipped).join(", ")}`);
    t(flipped.texts[0].layer === "B.Fab" && flipped.texts[0].mirror === true, `and the text is on B.Fab, mirrored: ${flipped.texts[0].layer} ${flipped.texts[0].mirror}`);
    const b2 = bbox(flipped);
    t(Math.abs(w(b2) - w(b1)) <= 2 && Math.abs(h(b2) - h(b1)) <= 2, "a flip keeps their size");
    const centreX = (b2[0] + b2[2]) / 2, mirrored = 2 * start.at[0] - (b1[0] + b1[2]) / 2;
    t(Math.abs(centreX - mirrored) <= 2, `and mirrors them about the footprint's origin (centre x ${Math.round(centreX)} against ${Math.round(mirrored)})`);
    await undo();
    await undo();
    const back = await get();
    t(JSON.stringify(bbox(back)) === JSON.stringify(b0) && back.side === "top" && JSON.stringify(layersOf(back)) === JSON.stringify(layersOf(start)), "two undos bring the drawings back exactly");
    void c0;
  });

  await scenario("the board is as it was and nothing broke", async (t) => {
    await __eda.run("common.InteractiveSelection.clear");
    const now = JSON.stringify((await state()).parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot, p.side]));
    t(now === original, "every footprint is where it was, turned and on the side it was");
    const errors = (__eda.errors?.() ?? []).slice(errorsFrom);
    t(errors.length === 0, `no console error: ${JSON.stringify(errors).slice(0, 200)}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
