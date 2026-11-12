// UI-LEFT-2 PP3 and PP20 (task-v9-pcbview), as they look in the page:
//   * PP3, `PCB_DRAW_PANEL_GAL`'s layer order and `PCB_PAINTER`'s pads and vias: a through-hole pad is drawn on each copper layer in that layer's colour, the
//     active layer's on top, its hole in the background's colour over all the copper; a via's ring is the copper layer's colour and its hole the via-hole colour
//     (227, 183, 46); with F.Cu active a pad's copper covers a silkscreen line crossing it, with F.SilkS active the line is over the pad (`SetTopLayer` puts an
//     active technical layer under the marker shadows); the net highlight darkens a DRC marker as it darkens every item of no net.
//   * PP20, Auto-Place: KiCad's progress window ("Autoplace Footprints", "Autoplacing components...", Elapsed time, Cancel) while the placer runs, Cancel stops it
//     and nothing moves; the top toolbar's Override locks box places a locked footprint.
// Run it in the page of a served COPY of a placed board (it edits and undoes; it adds a via if the board has none); it answers through `window.__pcbV9Check`: { results, done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=15
(async () => {
  const results = [];
  const check = (window.__pcbV9Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  // driven by `/api/autoplace/progress`, which the server answers on the connection's own thread: the autoplacer's /api/cmd holds the request loop while it runs
  const tick = () => fetch("/api/autoplace/progress").then((r) => r.text());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await tick(); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbV9Sched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 6e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await tick();
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
    window.__pcbV9Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    const r = st.routing ?? {};
    return JSON.stringify({ tracks: (r.tracks ?? []).map((t) => t.id), zones: (r.zones ?? []).map((z) => z.id), shapes: (st.drawings?.shapes ?? []).map((s) => s.id), texts: (st.drawings?.texts ?? []).map((s) => s.id) });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const op = (o) => __eda.run("studio.Appearance.op", o);

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => canvasBox().querySelector("canvas");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  let toScreen = null;
  const view = () => __eda.state().view;
  const screenOf = () => { const v = view(); return v && v.scale > 0 ? (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y] : null; };
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(500); };
  const bg = [0x00, 0x10, 0x23];

  /** The pixels of a `r`-pixel square round the board point (x, y) mm, as [r, g, b] each. */
  const pixels = (xmm, ymm, r = 2) => {
    const c = canvas();
    const k = c.width / c.getBoundingClientRect().width;
    const [sx, sy] = toScreen(xmm, ymm);
    const x0 = Math.round(sx * k) - r, y0 = Math.round(sy * k) - r;
    const d = c.getContext("2d").getImageData(x0, y0, 2 * r + 1, 2 * r + 1).data;
    const out = [];
    for (let i = 0; i < d.length; i += 4) out.push([d[i], d[i + 1], d[i + 2]]);
    return out;
  };
  /** The colour most of the square has (the grid's dots and antialiasing left out). */
  const colourAt = (xmm, ymm) => {
    const n = new Map();
    for (const p of pixels(xmm, ymm)) { const k = p.join(","); n.set(k, (n.get(k) ?? 0) + 1); }
    return [...n.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  };
  /** How far from the background the inkiest pixel of a small square is. */
  const inkAt = (xmm, ymm, r = 1) => Math.max(...pixels(xmm, ymm, r).map((p) => Math.abs(p[0] - bg[0]) + Math.abs(p[1] - bg[1]) + Math.abs(p[2] - bg[2])));
  const inkiest = (xmm, ymm, r = 1) => pixels(xmm, ymm, r).sort((a, b) => (Math.abs(b[0] - bg[0]) + Math.abs(b[1] - bg[1]) + Math.abs(b[2] - bg[2])) - (Math.abs(a[0] - bg[0]) + Math.abs(a[1] - bg[1]) + Math.abs(a[2] - bg[2])))[0];
  const near = (a, b, tol) => a.every((v, i) => Math.abs(v - b[i]) <= tol);
  const repaint = async () => { await sleep(350); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { for (let i = 0; i < 2 && __eda.state().tool !== "select"; i++) await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const one = (xmm, ymm) => pixels(xmm, ymm, 0)[0];
  const F_CU = [200, 52, 52], B_CU = [77, 127, 196], VIA_HOLE = [227, 183, 46];
  // a hole's colour: the colour most of eight points half way out from its centre have (a ratsnest line ends at the centre, crossing one or two of them)
  const holeColour = (cx, cy, r) => {
    const n = new Map();
    for (let k = 0; k < 8; k++) { const c = one(cx + r * Math.cos((k * Math.PI) / 4), cy + r * Math.sin((k * Math.PI) / 4)); const key = c.join(","); n.set(key, (n.get(key) ?? 0) + 1); }
    return [...n.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  };
  const st0 = await state();
  // a through-hole pad and a through via, both on F.Cu and B.Cu
  const thPart = st0.parts.find((p) => (p.pads ?? []).some((q) => q.th && q.drill));
  const thPad = thPart?.pads.find((q) => q.th && q.drill);
  let via = (st0.routing?.vias ?? []).find((v) => !v.type && v.drill > 0);
  if (!via) {
    // a GND via where no footprint is, 1 mm clear of every courtyard
    const xs = st0.outline.map((p) => p[0] / 1e6), ys = st0.outline.map((p) => p[1] / 1e6);
    let spot = null;
    for (let x = Math.min(...xs) + 1.5; x < Math.max(...xs) - 1.5 && !spot; x += 0.5)
      for (let y = Math.min(...ys) + 1.5; y < Math.max(...ys) - 1.5 && !spot; y += 0.5)
        if (st0.parts.every((p) => !p.placed || !p.courtyard || x < p.courtyard[0] / 1e6 - 1 || x > p.courtyard[2] / 1e6 + 1 || y < p.courtyard[1] / 1e6 - 1 || y > p.courtyard[3] / 1e6 + 1)) spot = [x, y];
    if (spot) await post({ op: "add_via", net: "GND", x: Math.round(spot[0] * 1e6), y: Math.round(spot[1] * 1e6), drill: 400_000, diameter: 800_000, from_layer: "F.Cu", to_layer: "B.Cu" });
    await sleep(600);
    via = ((await state()).routing?.vias ?? []).find((v) => !v.type && v.drill > 0);
  }
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);
  toScreen = screenOf();
  if (!toScreen || !thPad || !via) {
    results.push({ name: "setup", ok: false, notes: [`FAIL: no view, through-hole pad or via (${!!toScreen} ${!!thPad} ${!!via})`] });
  } else {
    const px = thPad.x / 1e6, py = thPad.y / 1e6;
    // a point of the pad's copper ring, below its hole: half way from the hole's edge to the pad's
    const ringR = (thPad.drill / 2 + Math.min(thPad.w, thPad.h) / 2) / 2 / 1e6;
    const vx = via.x / 1e6, vy = via.y / 1e6;
    const viaRing = (via.drill / 2 + via.d / 2) / 2 / 1e6;

    await scenario("A through-hole pad is drawn on each copper layer in that layer's colour, the active one on top, its hole over the copper", async (t) => {
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
      const front = one(px, py + ringR);
      t(near(front, F_CU, 12), `F.Cu active: the pad's ring is F.Cu's colour (${front})`);
      const hole = holeColour(px, py, thPad.drill / 4 / 1e6);
      t(near(hole, bg, 12), `the hole is the background's colour, over the copper (${hole})`);
      await __eda.run("pcbnew.Control.layerBottom"); await repaint();
      const back = one(px, py + ringR);
      t(near(back, B_CU, 12), `B.Cu active: the pad's ring is B.Cu's colour -- its B.Cu copper on top (${back})`);
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
    });

    await scenario("A via's ring is the copper layer's colour, the active one on top, and its hole the via-hole colour", async (t) => {
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
      const ringF = one(vx + viaRing, vy);
      t(near(ringF, F_CU, 16), `F.Cu active: the ring is F.Cu's colour (${ringF}), not the via-hole colour`);
      const hole = holeColour(vx, vy, via.drill / 4 / 1e6);
      t(near(hole, VIA_HOLE, 16), `the hole is LAYER_VIA_HOLES' (227, 183, 46) (${hole})`);
      await __eda.run("pcbnew.Control.layerBottom"); await repaint();
      const ringB = one(vx + viaRing, vy);
      t(near(ringB, B_CU, 16), `B.Cu active: the ring is B.Cu's colour (${ringB})`);
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
    });

    await scenario("With F.Cu active a pad's copper covers a silkscreen line across it; with F.SilkS active the line is over the pad", async (t) => {
      // a 0.3 mm F.SilkS line across the pad's ring, below the hole
      const y = py + ringR;
      const r = await post({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 300_000, filled: false, start: mm(px - 0.6, y), end: mm(px + 0.6, y) } });
      t(r.ok, `the line is drawn (${r.message ?? ""})`);
      await sleep(600);
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
      const underPad = one(px, y);
      t(near(underPad, F_CU, 14), `F.Cu active: the pad (pad:F.Cu over F.SilkS) (${underPad})`);
      // the Appearance panel's F.SilkS row makes it the active layer
      if (!document.querySelector(".ap-root")) document.querySelector('button[aria-label="Show Appearance"]')?.click();
      await sleep(250);
      document.querySelector('.ap-root [data-tab="layers"]')?.click();
      await sleep(200);
      const row = document.querySelector('.ap-root .ap-row[data-layer="f_silks"]');
      if (row && !row.classList.contains("active")) row.click();
      await repaint();
      toScreen = screenOf();
      const active = __eda.state().activeLayer;
      const overPad = one(px, y);
      t(active === "f_silks", `F.SilkS is the active layer (${active})`);
      t(near(overPad, [242, 237, 161], 14), `F.SilkS active: the silkscreen line is over the pad (${overPad})`);
      await __eda.run("pcbnew.Control.layerTop"); await repaint();
      await undo();
    });

    await scenario("The net highlight darkens a DRC marker as it darkens every item of no net", async (t) => {
      await __eda.run("pcbnew.DRCTool.runDRC");
      let drc = null;
      for (let i = 0; i < 60; i++) { drc = await json("/api/drc").catch(() => null); if (drc && (drc.violations ?? []).length > 0) break; await sleep(1000); }
      await closeDialogs();
      const v = (drc?.violations ?? []).find((x) => !x.excluded && x.items?.[0]?.pos);
      if (!v) return t(false, `the board has a DRC violation to look at (${(drc?.violations ?? []).length})`);
      await sleep(800);
      const [mx, my] = [v.items[0].pos[0] / 1e6, v.items[0].pos[1] / 1e6];
      // the marker's ring (radius 0.3 mm): the inkiest pixel round its right side
      // the marker's ring (radius 0.3 mm, an opaque stroke): the pixel of the square round its right side nearest the marker's colours, then the same pixel
      const marks = [[215, 91, 107], [255, 208, 66], [255, 255, 255]];
      const square0 = pixels(mx + 0.3, my, 2);
      const score = (c) => Math.min(...marks.map((m) => Math.abs(c[0] - m[0]) + Math.abs(c[1] - m[1]) + Math.abs(c[2] - m[2])));
      const at = square0.reduce((best, c, i) => (score(c) < score(square0[best]) ? i : best), 0);
      const ring = () => pixels(mx + 0.3, my, 2)[at];
      const before = ring();
      // a net with nothing within 4 mm of the marker, so what lies under the marker is darkened too (an item of the net would be brightened)
      const near4 = (x, y) => Math.hypot(x / 1e6 - mx, y / 1e6 - my) < 4;
      const pads = st0.parts.filter((p) => p.placed).flatMap((p) => (p.pads ?? []).map((q, i) => ({ id: `${p.ref}.${q.num}`, net: q.net, x: q.x, y: q.y, first: (p.pads ?? []).findIndex((r) => r.num === q.num) === i })));
      const far = pads.find((q) => q.net && q.first && pads.every((r) => r.net !== q.net || !near4(r.x, r.y)) && !(q.net === via.net && near4(via.x, via.y)));
      if (!far) return t(false, "a net far from the marker");
      await select([far.id]);
      await __eda.run("pcbnew.EditorControl.highlightNetSelection");
      await select([]);
      await repaint();
      const after = ring();
      const sum = (c) => c[0] + c[1] + c[2];
      t(JSON.stringify(__eda.state().highlight) === JSON.stringify([far.net]), `${far.net} is highlighted (${JSON.stringify(__eda.state().highlight)})`);
      t(sum(after) < sum(before) * 0.75, `the marker is darkened with ${far.net} highlighted (${before} -> ${after})`);
      await __eda.run("pcbnew.EditorControl.clearHighlight");
      await repaint();
      const cleared = ring();
      t(sum(cleared) > sum(after), `cleared: the marker is as it was (${cleared})`);
    });

    await scenario("Auto-Place: the progress window while the placer runs; Cancel stops it and nothing moves; Override locks places a locked footprint", async (t) => {
      const poses = async () => JSON.stringify((await state()).parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot]));
      const before = await poses();
      const refs = st0.parts.filter((p) => p.placed).map((p) => p.ref);
      const target = refs.find((r) => /^R/.test(r)) ?? refs[0];
      // lock one footprint and select it with others
      const locked = await post({ op: "set_locked", ids: [target], locked: true });
      t(locked.ok, `${target} is locked`);
      await sleep(400);
      await select(refs.filter((r) => /^[RC]\d/.test(r)).slice(0, 6));
      const run = __eda.run("pcbnew.Autoplacer.autoplaceSelected");
      let win = null;
      for (let i = 0; i < 80 && !win; i++) { await sleep(100); win = document.querySelector('[data-testid="progress-reporter"]'); }
      t(!!win, "the progress window is up while the placer runs");
      const title = document.querySelector('[data-testid="progress-reporter-title"]')?.textContent;
      const message = document.querySelector('[data-testid="progress-reporter-message"]')?.textContent;
      t(title === "Autoplace Footprints" && message === "Autoplacing components...", `its title and message are KiCad's (${title} / ${message})`);
      t(/^\d+:\d\d:\d\d$/.test(document.querySelector('[data-testid="progress-reporter-elapsed"]')?.textContent ?? ""), "it shows the elapsed time");
      document.querySelector('[data-testid="progress-reporter-cancel"]')?.click();
      await run;
      for (let i = 0; i < 40 && document.querySelector('[data-testid="progress-reporter"]'); i++) await sleep(150);
      t(!document.querySelector('[data-testid="progress-reporter"]'), "the window goes when the run stops");
      await sleep(500);
      t((await poses()) === before, "Cancel: nothing moved (the commit reverted)");
      t(!__eda.errors().some((e) => /abort/i.test(e.message)), "nothing is said of the abort");
      // Override locks: the locked footprint alone -- without the box it is left out and nothing happens (KiCad's empty commit), with it the placer runs on it
      const box = document.querySelector('[data-control="overrideLocks"] input');
      t(!!box && !box.checked, "the top toolbar has the Override locks box, unchecked");
      await select([target]);
      const errorsBefore = __eda.errors().length;
      await __eda.run("pcbnew.Autoplacer.autoplaceSelected");
      for (let i = 0; i < 60; i++) { const p = await json("/api/autoplace/progress"); if (!p.running && i > 5) break; await sleep(200); }
      await sleep(600);
      t((await poses()) === before && __eda.errors().length === errorsBefore, "without Override locks: nothing is placed and nothing is said");
      if (box && !box.checked) box.click();
      await sleep(200);
      await select([target]);
      await __eda.run("pcbnew.Autoplacer.autoplaceSelected");
      // until the run is over (the progress route says so), then the board is read again
      for (let i = 0; i < 600; i++) { const p = await json("/api/autoplace/progress"); if (!p.running && i > 5) break; await sleep(200); }
      await sleep(1500);
      const after = JSON.parse(await poses());
      const was = JSON.parse(before).find((p) => p[0] === target);
      const now = after.find((p) => p[0] === target);
      // placed, or the placement judged and refused by the gates (the board's own rules), named in the toast: either way the placer ran on the locked footprint
      const judged = __eda.errors().slice(errorsBefore).find((e) => /failure/.test(e.message))?.message;
      t(JSON.stringify(was) !== JSON.stringify(now) || !!judged, `with Override locks the placer takes the locked ${target} (${JSON.stringify(was)} -> ${JSON.stringify(now)}${judged ? `; ${judged.slice(0, 120)}` : ""})`);
      if (box && box.checked) box.click();
      await undo();
      await post({ op: "set_locked", ids: [target], locked: false });
      await sleep(400);
    });
  }
  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
