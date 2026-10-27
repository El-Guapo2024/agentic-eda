// Round-4 PCB editing check (findings/round4/r4-pcb-edit.md): what the drawing-tool fixes of task-r4-pcb-edit look like in the page.
//   * A-1: the Line tool chains segments (the next starts at the end of the last); Backspace takes the last committed one back and draws on from its start.
//   * A-2: the first click of a shape clears the selection, and Delete with a rectangle started drops the rectangle, not the selection.
//   * A-3: Redo while a zone outline is being drawn is refused: the undone track stays undone.
//   * C-4: a click on the first corner closes a graphic polygon; in the 45-degree line mode a click locks in the bend and the end.
//   * C-5: a finished graphic polygon leaves the tool (back to the selection tool).
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes); it answers through `window.__pcbEditR4Check`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR4Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR4Sched) {
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
    window.__pcbEditR4Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    return JSON.stringify({
      tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.net, t.pts]),
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

  (async () => {
    await scenario("A-1: the Line tool chains segments; Backspace takes the last one back and draws on from its start", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.line"); await sleep(400);
      await click(toScreen, fx, fy);
      await click(toScreen, fx + 10, fy);
      await click(toScreen, fx + 10, fy + 10);
      let made = await newShapes(before);
      t(made.length === 2 && made.every((s) => s.kind === "segment"), `two segments (${made.length})`);
      const second = made.find((s) => near(s.start, fx + 10, fy));
      t(!!second && near(second.end, fx + 10, fy + 10), "the second starts where the first ended");
      await hover(toScreen, fx + 12, fy + 12);
      await __eda.run("pcbnew.InteractiveDrawing.deleteLastPoint"); await sleep(800);
      made = await newShapes(before);
      t(made.length === 1 && near(made[0].end, fx + 10, fy), `Backspace took the second back (${made.length} left)`);
      await click(toScreen, fx + 10, fy + 5);
      made = await newShapes(before);
      t(made.some((s) => near(s.start, fx + 10, fy) && near(s.end, fx + 10, fy + 5)), "the line in progress went on from (10,0)");
      t(__eda.state().tool !== "select", "the tool is still armed");
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("A-2: the first click of a rectangle clears the selection; Delete drops the rectangle, not the selection", async (t) => {
      const tracksBefore = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx - 5, fy + 2), mm(fx + 15, fy + 2)] });
      await sleep(400);
      const trk = ((await state()).routing?.tracks ?? []).map((q) => q.id).find((id) => !tracksBefore.includes(id));
      t(!!trk, "a track to keep");
      await select([trk]);
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy + 6);
      t(__eda.state().selection.length === 0, "the first click cleared the selection");
      await __eda.run("common.Interactive.delete"); await sleep(600);
      t(((await state()).routing?.tracks ?? []).some((q) => q.id === trk), "the track is not deleted");
      await click(toScreen, fx + 4, fy + 9);
      const made = await newShapes(before);
      t(made.length === 0, `the rectangle was dropped: the next click starts a new one (${made.length} made)`);
      await escape(); await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("A-3: Redo while a zone outline is being drawn is refused", async (t) => {
      const tracksBefore = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx - 5, fy + 2), mm(fx + 15, fy + 2)] });
      await sleep(400);
      await undo();
      const count = () => state().then((s) => (s.routing?.tracks ?? []).filter((q) => !tracksBefore.includes(q.id)).length);
      t((await count()) === 0, "the track is undone");
      await __eda.run("pcbnew.InteractiveDrawing.zone"); await sleep(400);
      await click(toScreen, fx, fy);
      await click(toScreen, fx + 10, fy);
      await __eda.run("common.Interactive.redo"); await sleep(800);
      t((await count()) === 0, "Redo did nothing while the outline is open");
      await escape(); await escape();
      await __eda.run("common.Interactive.redo"); await sleep(800);
      t((await count()) === 1, "with the tool left, Redo redoes");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C-4, C-5: a click on the first corner closes a polygon, the 45-degree mode bends a leg in, and the tool is left", async (t) => {
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.graphicPolygon"); await sleep(400);
      await click(toScreen, fx, fy);
      await click(toScreen, fx + 10, fy);
      // from (10,0) to (20,4) in the default 45-degree mode: a run to (16,0), then 45 degrees into (20,4)
      await click(toScreen, fx + 20, fy + 4);
      await click(toScreen, fx + 10, fy + 10, { ctrlKey: true });
      await click(toScreen, fx, fy);
      const made = await newShapes(before);
      t(made.length === 1 && made[0].kind === "polygon", `one polygon, made by the click on the first corner (${made.length})`);
      const pts = made[0]?.pts ?? [];
      t(pts.length === 5, `five corners: the bend is one (${pts.length}: ${JSON.stringify(pts.map((p) => [p[0] / 1e6 - fx, p[1] / 1e6 - fy]))})`);
      t(pts.some((p) => near(p, fx + 16, fy)), "the bend at (16,0)");
      t(__eda.state().tool === "select", `the tool was left (${__eda.state().tool})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
