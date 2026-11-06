// UI-LEFT-2 PP12 (task-v3-pcbmisc, PARITY-pcb section 101): vias in the PCB editor.
//   * A via whose padstack gives its back a wider ring is drawn with that ring under the front's (`PCB_PAINTER::draw( const PCB_VIA*, aLayer )` on each
//     copper layer at `GetWidth( layer )`), and with B.Cu active the back's ring is the via's own on top.
//   * Draw Via's preview sits where `SnapItem` puts the via: on the track under the cursor, not at the grid point (round 6 C6-7).
//   * With Allow DRC violations a via next to another net's copper goes down and "Via location violates DRC." is still said (round 6 C6-6).
//   * A plated pad whose unused layers are removed is its hole there, judged at the copper clearance (round 6 C6-4); a copper dimension refuses a via, a
//     copper text box does not (C6-5: `connectedItemNet == 0`).
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes); it answers through `window.__viasPadstackCheck`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__viasPadstackCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__viasPadstackSched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 6e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__viasPadstackSched = true;
  }

  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    return JSON.stringify({
      tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.net, t.pts]),
      vias: (st.routing?.vias ?? []).map((v) => [v.id, v.net, v.props ?? null]),
      dims: (st.drawings?.items?.dimensions ?? st.drawings?.dimensions ?? []).map((d) => d.id),
      boxes: (st.drawings?.items?.text_boxes ?? []).map((t) => t.id),
      rules: st.board_rules?.custom_rules_text ?? null,
    });
  };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(300); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" ? 1 : 0, ...extra }));
  };
  const view = () => __eda.state().view;
  const toScreen = (xmm, ymm) => { const v = view(); return [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y]; };
  const hover = async (xmm, ymm, extra = {}) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y, extra); await sleep(250); };
  const click = async (xmm, ymm) => { await hover(xmm, ymm); const [x, y] = toScreen(xmm, ymm); fire("pointerdown", x, y); await sleep(60); fire("pointerup", x, y); await sleep(700); };
  /** The canvas's colour at a board point, as `r,g,b`. */
  const pixel = (xmm, ymm) => {
    const c = canvasBox().querySelector("canvas");
    const rect = c.getBoundingClientRect();
    const [x, y] = toScreen(xmm, ymm);
    const k = c.width / rect.width;
    const d = c.getContext("2d").getImageData(Math.round(x * k), Math.round(y * k), 1, 1).data;
    return `${d[0]},${d[1]},${d[2]}`;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { for (let i = 0; i < 2 && __eda.state().tool !== "select"; i++) await escape(); } catch { /* ignore */ }
    results.push(entry);
  };
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const base = await snapshot();
  const undoUntilBase = async () => { for (let i = 0; i < 14 && (await snapshot()) !== base; i++) await undo(); };
  const viasNow = async () => (await state()).routing?.vias ?? [];
  const nets = [...new Set(st0.parts.flatMap((p) => (p.pads ?? []).map((q) => q.net)).filter(Boolean))];
  // a free spot: the board's outline box, below its footprints' centre
  const ol = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const xs = ol.map((p) => p[0]), ys = ol.map((p) => p[1]);
  const [bx0, by0, bx1, by1] = [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
  // the zoom: about the cursor held on the spot (`common.Control.zoomIn`, F1), until a millimetre is 60 px
  const zoomOn = async (xmm, ymm) => {
    await __eda.run("common.Control.zoomFitObjects"); await sleep(300);
    for (let i = 0; i < 14 && view().scale * 1e6 < 60; i++) {
      await hover(xmm, ymm);
      await __eda.run("common.Control.zoomIn"); await sleep(200);
    }
  };

  (async () => {
    // a free spot: at least 3 mm from every pad and part, 3 mm inside the outline's box, nearest the board's middle
    const padsAt = st0.parts.filter((p) => p.placed).flatMap((p) => [[p.at[0] / 1e6, p.at[1] / 1e6], ...(p.pads ?? []).map((q) => [q.x / 1e6, q.y / 1e6])]);
    const mid = [(bx0 + bx1) / 2, (by0 + by1) / 2];
    let spot = null;
    for (let y = Math.ceil(by0 + 3); y <= by1 - 3; y++) for (let x = Math.ceil(bx0 + 3); x <= bx1 - 3; x++) {
      if (padsAt.some((q) => Math.hypot(q[0] - x, q[1] - y) < 3)) continue;
      if (!spot || Math.hypot(x - mid[0], y - mid[1]) < Math.hypot(spot[0] - mid[0], spot[1] - mid[1])) spot = [x, y];
    }
    spot = spot ?? [bx0 + 8, by1 - 4];
    const occupied = false;
    await zoomOn(spot[0], spot[1]);
    const v0 = view();
    const [sx, sy] = toScreen(spot[0], spot[1]);
    const rect = canvasBox().getBoundingClientRect();
    const onScreen = sx > 60 && sy > 60 && sx < rect.width - 60 && sy < rect.height - 60;

    await scenario("PP12: a padstack's wider back ring is drawn under the front's, and is the via's own with B.Cu active", async (t) => {
      t(!occupied && onScreen && v0.scale * 1e6 > 20, `a free spot on screen at ${(v0.scale * 1e6).toFixed(1)} px/mm (${JSON.stringify(spot)}, ${sx.toFixed(0)},${sy.toFixed(0)})`);
      await __eda.run("pcbnew.Control.layerTop"); await sleep(300);
      const before = (await viasNow()).map((v) => v.id);
      const r = await post({ op: "add_via", net: nets[0], x: Math.round(spot[0] * 1e6), y: Math.round(spot[1] * 1e6), drill: 300000, diameter: 600000, from_layer: "F.Cu", to_layer: "B.Cu" });
      t(r.ok, `a via (${r.message})`);
      await sleep(900);
      const via = (await viasNow()).find((v) => !before.includes(v.id));
      const ring = [spot[0] + 0.5, spot[1]]; // between the front's 0.3 mm radius and the back's 0.7 mm
      const inFront = [spot[0] + 0.15, spot[1]];
      const bg = pixel(ring[0], ring[1]);
      const frontColour = pixel(inFront[0], inFront[1]);
      const r2 = await post({ op: "set_via_props", id: via.id, props: { padstack: { mode: "front_inner_back", sizes: { Inner: 600000, "B.Cu": 1400000 } } } });
      t(r2.ok, `the back made 1.4 mm (${r2.message})`);
      await sleep(900);
      const st = await state();
      t(st.routing.vias.find((v) => v.id === via.id)?.props?.padstack?.sizes?.["B.Cu"] === 1400000, "the state sends the padstack");
      const backUnder = pixel(ring[0], ring[1]);
      t(backUnder !== bg, `0.5 mm from the centre the back's ring now shows under the front's (${bg} -> ${backUnder})`);
      t(pixel(inFront[0], inFront[1]) === frontColour, `inside the front's ring it is the via as before (${frontColour})`);
      await __eda.run("pcbnew.Control.layerBottom"); await sleep(500);
      const backOnTop = pixel(ring[0], ring[1]);
      t(backOnTop === frontColour, `with B.Cu active the back's ring is the via's colour, on top (${backOnTop} vs ${frontColour})`);
      await __eda.run("pcbnew.Control.layerTop"); await sleep(300);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C6-7: Draw Via's preview sits on the track SnapItem puts it on, not at the grid point", async (t) => {
      const y0 = Math.round(spot[1]);
      const x0 = Math.round(spot[0]);
      const r = await post({ op: "add_track", net: nets[0], layer: "F.Cu", width: 250000, pts: [mm(x0 - 3, y0), mm(x0 + 3, y0)] });
      t(r.ok, `a track (${r.message})`);
      await sleep(800);
      await hover(x0, y0 + 2); // out of the way: the baseline pixels
      const above = [x0, y0 - 0.22]; // inside a via on the track, outside the track
      const below = [x0, y0 + 0.62]; // inside a via at the cursor, outside one on the track
      const bgAbove = pixel(...above), bgBelow = pixel(...below);
      await __eda.run("pcbnew.InteractiveDrawing.via"); await sleep(300);
      await hover(x0, y0 + 0.35);
      await hover(x0, y0 + 0.35);
      t(pixel(...above) !== bgAbove, `the preview covers the track's axis side above it (${bgAbove} -> ${pixel(...above)})`);
      t(pixel(...below) === bgBelow, `and not the cursor's side below it (${bgBelow} -> ${pixel(...below)})`);
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C6-6: with Allow DRC violations the via goes down and KiCad's message is still said", async (t) => {
      const y0 = Math.round(spot[1]);
      const x0 = Math.round(spot[0]);
      // the other net's track 0.52 mm from a whole-millimetre point: a via there is not snapped onto it (0.425 mm) and is through its clearance (0.625 mm)
      await post({ op: "add_track", net: nets[0], layer: "F.Cu", width: 250000, pts: [mm(x0 - 3, y0 + 0.52), mm(x0 + 3, y0 + 0.52)] });
      await sleep(600);
      const setAllow = async (allow) => {
        await __eda.run("pcbnew.InteractiveRouter.SettingsDialog"); await sleep(400);
        const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Interactive Router Settings"));
        if (!dlg) return false;
        const radio = [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim() === "Highlight collisions")?.querySelector("input");
        if (allow && radio && !radio.checked) { radio.click(); await sleep(150); }
        const box = [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim() === "Allow DRC violations")?.querySelector("input");
        if (box && box.checked !== allow) { box.click(); await sleep(150); }
        buttonText(dlg, "OK")?.click(); await sleep(300);
        return true;
      };
      const tryVia = async () => {
        const before = (await viasNow()).map((v) => v.id);
        const since = Date.now();
        await __eda.run("pcbnew.InteractiveDrawing.via"); await sleep(300);
        await click(x0 + 1, y0);
        await escape(); await escape();
        return { placed: (await viasNow()).filter((v) => !before.includes(v.id)).length, said: __eda.errors(since).map((e) => e.message).some((m) => m.includes("Via location violates DRC.")) };
      };
      const refused = await tryVia();
      t(refused.placed === 0 && refused.said, `without it: refused, with KiCad's message (${JSON.stringify(refused)})`);
      t(await setAllow(true), "Highlight collisions with Allow DRC violations, in the router's settings");
      const allowed = await tryVia();
      t(allowed.placed === 1 && allowed.said, `with it: placed, and the message still said (${JSON.stringify(allowed)})`);
      await setAllow(false);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C6-4, C6-5: an unflashed plated pad is its hole at the copper clearance; a copper dimension refuses a via, a text box does not", async (t) => {
      const st = await state();
      const pad = st.parts.flatMap((p) => (p.pads ?? []).map((q) => ({ ...q, ref: p.ref, placed: p.placed }))).find((q) => q.placed && q.net && (q.drill ?? 0) > 0);
      t(!!pad, `a plated pad on a net (${pad ? pad.ref + "." + pad.num : "none"})`);
      if (!pad) return;
      const half = Math.max(pad.w ?? 0, pad.h ?? 0) / 2e6;
      const hole = pad.drill / 2e6;
      // to the left of the pad, 0.3 mm from its hole's edge and outside its copper; the hole clearance is 0.25 mm
      const d = Math.max(hole + 0.3 + 0.3, half + 0.05);
      const gap = d - hole - 0.3;
      const at = [pad.x / 1e6 - d, pad.y / 1e6];
      t(gap > 0.25 && gap < 0.6, `the spot is ${gap.toFixed(2)} mm from the hole`);
      // `place_via`, the verb Draw Via's click sends, at the exact spot (the canvas would put it on the grid)
      const tryVia = async () => {
        const r = await post({ op: "place_via", net: "", x: Math.round(at[0] * 1e6), y: Math.round(at[1] * 1e6), drill: 300000, diameter: 600000, from_layer: "F.Cu", to_layer: "B.Cu" });
        await sleep(300);
        return { placed: r.ok ? 1 : 0, said: !r.ok && (r.message ?? "").includes("Via location violates DRC.") };
      };
      await post({ op: "set_custom_rules", text: "(version 1)\n(rule wide (constraint clearance (min 0.6mm)))\n" });
      await post({ op: "remove_unused_pads", selected: [pad.ref], vias: false, pads: true, keep_outside: false, remove: true });
      await sleep(600);
      const unflashed = await tryVia();
      t(unflashed.placed === 0 && unflashed.said, `with the pad's layers removed its hole, 0.6 mm clearance, refuses it (${JSON.stringify(unflashed)})`);
      await undoUntilBase();
      await post({ op: "remove_unused_pads", selected: [pad.ref], vias: false, pads: true, keep_outside: false, remove: true });
      await sleep(600);
      const holeOnly = await tryVia();
      t(holeOnly.placed === 1, `at the 0.2 mm clearance the hole alone lets it go down (${JSON.stringify(holeOnly)})`);
      await undoUntilBase();

      // a centre dimension on F.Cu: a cross of 1 mm arms
      const c = [Math.round(spot[0]) + 1, Math.round(spot[1])];
      const dim = { layer: "F.Cu", kind: { kind: "center" }, start: mm(c[0], c[1]), end: mm(c[0] + 1, c[1]), prefix: "", suffix: "", units: "mm", units_format: "no_suffix", precision: 2, suppress_trailing_zeros: true, text_position: "outside", keep_text_aligned: true, text_angle: 0, text_size_nm: 1000000, stroke_width: 150000, arrow_length: 1000000, extension_offset: 0, extension_height: 0, arrow_direction: "outward" };
      const rd = await post({ op: "add_dimension", dimension: dim });
      t(rd.ok, `a centre dimension on F.Cu (${rd.message})`);
      await sleep(600);
      const onDim = async (x, y) => {
        const r = await post({ op: "place_via", net: "", x: Math.round(x * 1e6), y: Math.round(y * 1e6), drill: 300000, diameter: 600000, from_layer: "F.Cu", to_layer: "B.Cu" });
        await sleep(300);
        return r.ok ? 1 : 0;
      };
      t((await onDim(c[0] + 0.5, c[1] + 0.1)) === 0, "a via on its arm is refused");
      t((await onDim(c[0], c[1] - 2.2)) === 1, "one 1.2 mm past the end of its arm goes down");
      await undoUntilBase();
      await sleep(300);
      // a text box on F.Cu with its border: a via on the border goes down (a PCB_SHAPE of no net)
      const tb = { layer: "F.Cu", text: "PP12", start: mm(c[0], c[1]), end: mm(c[0] + 6, c[1] + 3), margin_left_nm: 250000, margin_top_nm: 250000, margin_right_nm: 250000, margin_bottom_nm: 250000, border: true, stroke_width_nm: 200000, size_nm: 1000000 };
      const rt = await post({ op: "pcb_item", verb: "add_text_box", text_box: tb });
      t(rt.ok, `a copper text box (${rt.message})`);
      await sleep(600);
      t((await onDim(c[0], c[1] + 1.5)) === 1, "a via on its border goes down");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
