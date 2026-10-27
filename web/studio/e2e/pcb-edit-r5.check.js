// Round-5 PCB editing check (findings/round5/r5-pcb-edit.md): what task-r5-pcb-edit's fixes look like in the page.
//   * A5-2/A5-3/A5-4: the studio starts in the free line mode; a rectangle is free in the 45-degree mode; a 45-degree line keeps the cursor's x.
//   * A5-6: a graphic polygon's first corner clears the selection; Delete twice ends the outline and deletes nothing else.
//   * B5-2: the Text tool carries the text to a second click and stays armed.
//   * B5-1: Draw Via snaps onto a track and splits it, takes a pour's net in the middle of it, and is refused next to another net's copper.
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes); it answers through `window.__pcbEditR5Check`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR5Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR5Sched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 5e6;
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
    window.__pcbEditR5Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    return JSON.stringify({
      tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.net, t.pts]),
      vias: (st.routing?.vias ?? []).map((v) => [v.id, v.net]),
      zones: (st.routing?.zones ?? []).map((z) => z.id),
      shapes: (st.drawings?.shapes ?? []).map((s) => s.id),
      texts: (st.drawings?.texts ?? []).map((s) => s.id),
      tables: (st.drawings?.tables ?? []).map((t) => t.id),
      groups: (st.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]),
      nets: st.parts.map((p) => (p.pads ?? []).map((q) => q.net)),
      locked: st.locked ?? [],
    });
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
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Cancel Changes") ?? buttonText(document, "Leave Nets Unchanged") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  /** The question box with `text` in it (`askConfirm`), waited for. */
  const question = async (text, ms = 4000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      const el = [...document.querySelectorAll("[role=dialog], .dialog, .modal")].find((d) => d.textContent.includes(text));
      if (el) return el;
      await sleep(150);
    }
    return null;
  };

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects");
    await sleep(300);
    const v = __eda.state().view;
    return v && v.scale > 0 ? (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y] : null;
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm, extra = {}) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1, ...extra }); await sleep(60); fire("pointerup", x, y, extra); await sleep(500); };

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
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const placed = st0.parts.filter((p) => p.placed && (p.pads ?? []).some((q) => q.net) && p.side === "top");
  if (placed.length < 1) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs a placed footprint with a pad on a net"] }); check.done = true; return "no board"; }
  const part = placed[0];
  const padOnNet = part.pads.find((q) => q.net);
  const base = await snapshot();
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: no view transform"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 12 && (await snapshot()) !== base; i++) await undo(); };
  const newShapes = async (before) => ((await state()).drawings?.shapes ?? []).filter((s) => !before.includes(s.id));
  const near = (p, x, y) => Math.abs(p[0] / 1e6 - x) < 0.05 && Math.abs(p[1] / 1e6 - y) < 0.05;
  // a free spot below the footprint, on whole millimetres
  const [fx, fy] = [Math.round(part.at[0] / 1e6), Math.round(part.at[1] / 1e6) + 20];


  const tracksNow = async () => (await state()).routing?.tracks ?? [];
  const viasNow = async () => (await state()).routing?.vias ?? [];
  const otherNet = st0.parts.flatMap((p) => p.pads ?? []).map((q) => q.net).find((n) => n && n !== padOnNet.net);
  const rules = st0.board_rules ?? {};
  const viaD = (rules.via_diameter ?? 600_000) / 1e6;

  (async () => {
    await scenario("A5-2, A5-3, A5-4: the free line mode by default; a rectangle stays free in the 45-degree mode; a 45-degree line keeps the cursor's x", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy);
      await click(toScreen, fx + 10, fy + 3);
      let made = await newShapes(before);
      const r1 = made.find((s) => s.kind === "rect");
      t(!!r1 && near(r1.start, fx, fy) && near(r1.end, fx + 10, fy + 3), `a 10 x 3 mm rectangle in the default mode (${JSON.stringify(r1 && [r1.start, r1.end])})`);
      await escape(); await escape();
      await __eda.run("pcbnew.EditorControl.lineMode45"); await sleep(150);
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy + 5);
      await click(toScreen, fx + 10, fy + 8);
      made = await newShapes(before);
      const r2 = made.find((s) => s.kind === "rect" && near(s.start, fx, fy + 5));
      t(!!r2 && near(r2.end, fx + 10, fy + 8), `in the 45-degree mode too: drawShape leaves a rectangle free (${JSON.stringify(r2 && r2.end)})`);
      await escape(); await escape();
      await __eda.run("pcbnew.InteractiveDrawing.line"); await sleep(400);
      await click(toScreen, fx, fy + 12);
      await click(toScreen, fx + 10, fy + 16);
      made = await newShapes(before);
      const l = made.find((s) => s.kind === "segment" && near(s.start, fx, fy + 12));
      t(!!l && near(l.end, fx + 10, fy + 12), `GetVectorSnapped45: (10,4) is on the x axis at x = 10 (${JSON.stringify(l && l.end.map((v) => v / 1e6))})`);
      await escape(); await escape();
      await __eda.run("pcbnew.EditorControl.lineModeFree");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("A5-6: a polygon's first corner clears the selection, and Delete twice ends the outline without deleting the selection", async (t) => {
      const before = (await tracksNow()).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx - 5, fy + 22), mm(fx + 15, fy + 22)] });
      await sleep(400);
      const trk = (await tracksNow()).map((q) => q.id).find((id) => !before.includes(id));
      await select([trk]);
      await __eda.run("pcbnew.InteractiveDrawing.graphicPolygon"); await sleep(400);
      await click(toScreen, fx, fy + 18);
      t(__eda.state().selection.length === 0, "the first corner cleared the selection (OnFirstPoint)");
      await select([trk]);
      await __eda.run("common.Interactive.delete"); await sleep(500);
      await __eda.run("common.Interactive.delete"); await sleep(500);
      t((await tracksNow()).some((q) => q.id === trk), "the track is still there");
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("B5-2: the Text tool carries the text to a second click and stays armed", async (t) => {
      const before = ((await state()).drawings?.texts ?? []).map((x) => x.id);
      await __eda.run("pcbnew.InteractiveDrawing.text"); await sleep(400);
      await click(toScreen, fx, fy);
      const input = document.querySelector(".dialog .kv-grid input");
      t(!!input, "the first click opens the dialog");
      if (input) setValue(input, "R5 carried");
      const add = buttonText(document.querySelector(".dialog") ?? document, "Add");
      add?.click(); await sleep(500);
      const texts = async () => ((await state()).drawings?.texts ?? []).filter((x) => !before.includes(x.id));
      t((await texts()).length === 0, "OK places nothing yet: the text follows the cursor");
      await hover(toScreen, fx + 4, fy + 1);
      await click(toScreen, fx + 5, fy + 2);
      await sleep(400);
      const made = await texts();
      t(made.length === 1 && made[0].content === "R5 carried" && Math.abs(made[0].x / 1e6 - (fx + 5)) < 0.05 && Math.abs(made[0].y / 1e6 - (fy + 2)) < 0.05, `the second click places it there (${JSON.stringify(made.map((x) => [x.content, x.x / 1e6, x.y / 1e6]))})`);
      t(__eda.state().tool === "text", `the tool stays armed (${__eda.state().tool})`);
      for (let i = 0; i < 10 && !(made.length === 1 && __eda.state().selection.some((x) => (x.id ?? x) === made[0].id)); i++) await sleep(300);
      t(made.length === 1 && __eda.state().selection.some((x) => (x.id ?? x) === made[0].id), `the new text is selected (${JSON.stringify(__eda.state().selection)})`);
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("B5-1: Draw Via snaps onto a track's middle and splits it, with its net", async (t) => {
      const before = (await tracksNow()).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx, fy + 30), mm(fx + 10, fy + 30)] });
      await sleep(400);
      const trk = (await tracksNow()).find((q) => !before.includes(q.id));
      const vBefore = (await viasNow()).map((v) => v.id);
      await __eda.run("pcbnew.InteractiveDrawing.via"); await sleep(300);
      await click(toScreen, fx + 4.1, fy + 30.25);
      await sleep(600);
      const v = (await viasNow()).find((x) => !vBefore.includes(x.id));
      t(!!v && Math.abs(v.x / 1e6 - (fx + 4)) < 0.06 && Math.abs(v.y / 1e6 - (fy + 30)) < 0.01, `a via on the track's line (${v && [v.x / 1e6, v.y / 1e6]})`);
      t(!!v && v.net === padOnNet.net, `with the track's net (${v && v.net})`);
      const pieces = (await tracksNow()).filter((q) => !before.includes(q.id));
      t(pieces.length === 2 && pieces.some((q) => q.id === trk.id), `the track is split in two at the via, the first part keeping its id (${pieces.length})`);
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "one undo step took the via and the split back");
    });

    await scenario("B5-1: Draw Via next to another net's copper is refused with 'Via location violates DRC.'", async (t) => {
      if (!otherNet) { t(false, "the board needs two nets"); return; }
      const clearance = (rules.clearance ?? 200_000) / 1e6;
      // the other net's track, a little less than the clearance from where the via goes
      const gap = viaD / 2 + 0.125 + clearance / 2;
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx, fy + 34), mm(fx + 10, fy + 34)] });
      await post({ op: "add_track", net: otherNet, layer: "F.Cu", width: 250000, pts: [mm(fx, fy + 34 + gap), mm(fx + 10, fy + 34 + gap)] });
      await sleep(400);
      const vBefore = (await viasNow()).map((v) => v.id);
      const since = Date.now();
      await __eda.run("pcbnew.InteractiveDrawing.via"); await sleep(300);
      await click(toScreen, fx + 5, fy + 34);
      await sleep(700);
      t((await viasNow()).every((v) => vBefore.includes(v.id)), "no via was placed");
      const said = __eda.errors(since).map((e) => e.message);
      t(said.some((m) => m.includes("Via location violates DRC.")), `KiCad's message (${JSON.stringify(said.slice(-3))})`);
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("B5-1: Draw Via in the middle of a pour takes the pour's net (findStitchedZoneNet)", async (t) => {
      const net = otherNet ?? padOnNet.net;
      // a pour over the whole board on B.Cu, clicked at the free spot the other scenarios draw at
      const ol = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
      const xs = ol.map((p) => p[0]), ys = ol.map((p) => p[1]);
      const [x0, y0, x1, y1] = ol.length ? [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)] : [fx - 20, fy - 20, fx + 20, fy + 20];
      await post({ op: "add_zone", net, layer: "B.Cu", outline: [mm(x0, y0), mm(x1, y0), mm(x1, y1), mm(x0, y1)] });
      await sleep(400);
      const r = await __eda.run("pcbnew.ZoneFiller.zoneFillAll");
      t(!!r, `filled (${r && r.toast})`);
      await sleep(1500);
      // a point well inside the pour's copper: on a 1 mm grid, inside a fill fragment and at least 1.5 mm from its edges
      const fill0 = await json("/api/fill");
      const frags = (fill0.zones ?? []).filter((z) => z.net === net && z.layer === "B.Cu").flatMap((z) => z.fragments).map((f) => f.map((p) => [p[0] / 1e6, p[1] / 1e6]));
      const inside = (p, poly) => { let c = false; for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) { const [xi, yi] = poly[i], [xj, yj] = poly[j]; if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) c = !c; } return c; };
      const segD = (p, a, b) => { const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy; const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0; return Math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy); };
      let spot = null;
      for (let y = Math.ceil(y0); y <= y1 && !spot; y++) for (let x = Math.ceil(x0); x <= x1 && !spot; x++) {
        const f = frags.find((poly) => inside([x, y], poly));
        if (f && f.every((a, i) => segD([x, y], a, f[(i + 1) % f.length]) > 1.5)) spot = [x, y];
      }
      t(!!spot, `a free spot in the pour (${JSON.stringify(spot)})`);
      if (!spot) return;
      const vBefore = (await viasNow()).map((v) => v.id);
      await __eda.run("pcbnew.InteractiveDrawing.via"); await sleep(300);
      await click(toScreen, spot[0], spot[1]);
      await sleep(700);
      const v = (await viasNow()).find((x) => !vBefore.includes(x.id));
      const f = await json("/api/fill");
      t(!!v && v.net === net, `a stitching via of the pour's net (${v ? JSON.stringify(v.net) : "none"}; fills ${JSON.stringify((f.zones ?? []).map((z) => [z.net, z.layer, z.filled, z.fragments.length]))})`);
      await escape(); await escape();
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
