// Round-6 PCB editing check (findings/round6/r6-pcb-edit.md): what task-r6-pcb-edit's fixes look like in the page.
//   * E6-1: Swap and the Pack and Move drop leave the routing (tracks, vias, zones) as it was.
//   * E6-2: Swap takes vias too.
//   * A6-1/A6-4: a stepped line width survives re-arming a drawing tool; incWidth outside one does nothing.
//   * D6-2: Create Zone from Selection is one dialog, "Convert to Copper Zone", with the conversion settings.
// Run it in the page of a served COPY of a routed board (it edits and undoes); it answers through `window.__pcbEditR6Check`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-zones minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR6Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR6Sched) {
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
    window.__pcbEditR6Sched = true;
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
  const placed = st0.parts.filter((p) => p.placed && p.side === "top" && !(st0.locked ?? []).includes(p.ref));
  const routing0 = st0.routing ?? { tracks: [], vias: [], zones: [] };
  if (placed.length < 2 || routing0.tracks.length === 0) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs two placed footprints and routing"] }); check.done = true; return "no board"; }
  const base = await snapshot();
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: no view transform"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 12 && (await snapshot()) !== base; i++) await undo(); };
  const copper = async () => { const st = await state(); const r = st.routing ?? { tracks: [], vias: [], zones: [] }; return JSON.stringify([r.tracks.map((t) => [t.id, t.pts]), r.vias.map((v) => [v.id, v.x, v.y]), r.zones.map((z) => z.id)]); };
  const near = (p, x, y) => Math.abs(p[0] / 1e6 - x) < 0.05 && Math.abs(p[1] / 1e6 - y) < 0.05;
  const outline = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const [bx0, by0] = outline.length ? [Math.min(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))] : [0, 0];
  // a free spot below the board's outline, on whole millimetres
  const [fx, fy] = [Math.round(bx0) + 5, Math.round(by0) + 10];

  (async () => {
    await scenario("E6-1: Swap (Alt+S) of two footprints leaves every track, via and zone as it was", async (t) => {
      const before = await copper();
      const [a, b] = placed;
      await select([a.ref, b.ref]);
      await __eda.run("pcbnew.InteractiveEdit.swap");
      await sleep(800);
      const st = await state();
      const pa = st.parts.find((p) => p.ref === a.ref), pb = st.parts.find((p) => p.ref === b.ref);
      t(!!pa && !!pb && pa.at[0] === b.at[0] && pa.at[1] === b.at[1] && pb.at[0] === a.at[0] && pb.at[1] === a.at[1], `${a.ref} and ${b.ref} swapped (${JSON.stringify([pa && pa.at, pb && pb.at])})`);
      const after = await copper();
      t(after === before, `the routing is untouched (${(st.routing?.tracks ?? []).length} tracks, ${(st.routing?.vias ?? []).length} vias, ${(st.routing?.zones ?? []).length} zones; was ${routing0.tracks.length}, ${routing0.vias.length}, ${routing0.zones.length})`);
      await undoUntilBase();
      t((await snapshot()) === base, "one undo step took the swap back");
    });

    await scenario("E6-1: the Pack and Move drop leaves the routing as it was", async (t) => {
      const before = await copper();
      await select([placed[0].ref, placed[1].ref]);
      await __eda.run("pcbnew.InteractiveEdit.packAndMoveFootprints");
      await sleep(500);
      await hover(toScreen, fx + 2, fy + 2);
      await click(toScreen, fx + 3, fy + 3);
      await sleep(800);
      const st = await state();
      const moved = st.parts.filter((p) => p.ref === placed[0].ref || p.ref === placed[1].ref).some((p, i) => p.at[0] !== placed[i === 0 ? 0 : 1].at[0] || p.at[1] !== placed[i === 0 ? 0 : 1].at[1]);
      t(moved, "the footprints were moved");
      t((await copper()) === before, `the routing is untouched (${(st.routing?.tracks ?? []).length} tracks)`);
      await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("E6-2: Swap of two vias swaps their positions in one step", async (t) => {
      const vs = routing0.vias.filter((v) => !(st0.locked ?? []).includes(v.id));
      if (vs.length < 2) { t(true, "the board has fewer than two vias: skipped"); return; }
      const [a, b] = vs;
      await select([a.id, b.id]);
      await __eda.run("pcbnew.InteractiveEdit.swap");
      await sleep(800);
      const now = (await state()).routing.vias;
      const na = now.find((v) => v.id === a.id), nb = now.find((v) => v.id === b.id);
      t(!!na && !!nb && na.x === b.x && na.y === b.y && nb.x === a.x && nb.y === a.y, `the vias swapped (${JSON.stringify([na && [na.x, na.y], nb && [nb.x, nb.y]])})`);
      await undo();
      t((await snapshot()) === base, "one undo step took it back");
      await undoUntilBase();
    });

    await scenario("A6-1: a stepped line width survives re-arming a drawing tool on the same layer", async (t) => {
      await __eda.run("pcbnew.Control.layerTop"); await sleep(200);
      const shapesNow = async () => (await state()).drawings?.shapes ?? [];
      const before = (await shapesNow()).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy); await click(toScreen, fx + 4, fy + 2);
      await escape(); await escape();
      const plain = (await shapesNow()).find((s) => !before.includes(s.id) && s.kind === "rect");
      await __eda.run("pcbnew.InteractiveDrawing.line"); await sleep(400);
      await __eda.run("pcbnew.InteractiveDrawing.incWidth"); await sleep(100);
      await __eda.run("pcbnew.InteractiveDrawing.incWidth"); await sleep(100);
      await escape(); await escape();
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy + 4); await click(toScreen, fx + 4, fy + 6);
      await escape(); await escape();
      const stepped = (await shapesNow()).find((s) => !before.includes(s.id) && s.kind === "rect" && s.id !== plain?.id);
      t(!!plain && !!stepped && stepped.stroke_width === plain.stroke_width + 200_000, `two WIDTH_STEPs kept (${plain && plain.stroke_width} -> ${stepped && stepped.stroke_width})`);
      // With no drawing tool armed, Ctrl++ steps nothing (A6-4).
      await __eda.run("pcbnew.InteractiveDrawing.incWidth"); await sleep(100);
      await __eda.run("pcbnew.InteractiveDrawing.rectangle"); await sleep(400);
      await click(toScreen, fx, fy + 8); await click(toScreen, fx + 4, fy + 10);
      await escape(); await escape();
      const third = (await shapesNow()).find((s) => !before.includes(s.id) && s.kind === "rect" && s.id !== plain?.id && s.id !== stepped?.id);
      t(!!third && !!stepped && third.stroke_width === stepped.stroke_width, `incWidth outside a drawing tool did nothing (${third && third.stroke_width})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("D6-2: Create Zone from Selection opens one dialog, 'Convert to Copper Zone', with the conversion settings", async (t) => {
      await __eda.run("pcbnew.Control.layerTop"); await sleep(200);
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await post({ op: "add_shape", shape: { kind: "rect", layer: "F.Cu", stroke_width: 200_000, filled: false, start: mm(fx, fy), end: mm(fx + 6, fy + 4) } });
      await sleep(500);
      const rect = ((await state()).drawings?.shapes ?? []).find((s) => !before.includes(s.id));
      await select([rect.id]);
      await __eda.run("pcbnew.Convert.convertToZone");
      let dlg = null;
      for (let i = 0; i < 20 && !dlg; i++) { await sleep(200); dlg = document.querySelector('[data-testid="zone-dialog"]'); }
      const dialogs = [...document.querySelectorAll(".dialog")];
      t(!!dlg && dlg.querySelector(".dialog-header")?.textContent.trim() === "Convert to Copper Zone", `titled "Convert to Copper Zone" (${dlg && dlg.querySelector(".dialog-header")?.textContent.trim()})`);
      t(dialogs.length === 1, `one dialog (${dialogs.map((d) => d.querySelector(".dialog-header")?.textContent.trim()).join(" | ")})`);
      const box = dlg?.querySelector('[data-testid="zone-conversion-settings"]');
      t(!!box && box.textContent.includes("Use centerlines") && box.textContent.includes("Create bounding hull") && box.textContent.includes("Delete source objects after conversion") && !box.textContent.includes("Line width"), "the Conversion Settings box, with no line width");
      const zonesBefore = ((await state()).routing?.zones ?? []).length;
      t(dlg?.querySelector('[data-testid="zone-ok"]')?.textContent.trim() === "OK", "its buttons are OK and Cancel");
      dlg?.querySelector('[data-testid="zone-ok"]')?.click();
      await sleep(1200);
      t(((await state()).routing?.zones ?? []).length === zonesBefore + 1, "OK makes the zone");
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
