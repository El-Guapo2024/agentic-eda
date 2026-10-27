// Round-3 PCB editing check (findings/round3/r3-pcb-edit.md): what the fixes of task-r3-pcb-edit look like in the page.
//   * C-1: Delete and Undo while a graphic polygon is being drawn remove its last corner (DRAWING_TOOL::DrawZone), and the selection is not deleted.
//   * C-6: Undo in the measure tool undoes the board.
//   * B-1: Cut of a table cell empties the cell and keeps the table.
//   * C-2: a net change on a track that touches a pad of its net asks "Changing the net will also update ..."; Leave Nets Unchanged changes nothing,
//     Change Nets moves the track and the pad.
//   * D-8: Break Track on a locked track asks "The selected item is locked."; both halves stay locked.
//   * D-10: Simplify Shapes keeps the polygon's id and group.
//   * D-6: Select Connection follows a branch from a polyline's inner vertex.
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes, and leaves Strict off while it runs); it answers
// through `window.__pcbEditR3Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR3Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR3Sched) {
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
    window.__pcbEditR3Sched = true;
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
  const otherNet = [...new Set(st0.parts.flatMap((p) => (p.pads ?? []).map((q) => q.net)).filter((n) => n && n !== padOnNet.net))][0];
  const base = await snapshot();
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: no view transform"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 12 && (await snapshot()) !== base; i++) await undo(); };
  const newIds = (before, after) => after.filter((id) => !before.includes(id));
  // a free spot below the footprint
  const [fx, fy] = [part.at[0] / 1e6, part.at[1] / 1e6 + 20];

  (async () => {
    await scenario("C-1: Delete and Undo while drawing a polygon remove its last corner; the selection stays", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const tracksBefore = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx - 5, fy + 2), mm(fx + 15, fy + 2)] });
      await sleep(400);
      const trk = newIds(tracksBefore, ((await state()).routing?.tracks ?? []).map((q) => q.id))[0];
      t(!!trk, "a track to keep");
      await select([trk]);
      const shapesBefore = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await __eda.run("pcbnew.InteractiveDrawing.graphicPolygon"); await sleep(400);
      await click(toScreen, fx, fy);
      await click(toScreen, fx + 10, fy);
      await click(toScreen, fx + 10, fy + 10);
      await hover(toScreen, fx + 5, fy + 2);
      await __eda.run("common.Interactive.delete"); await sleep(500);
      t(((await state()).routing?.tracks ?? []).some((q) => q.id === trk), "Delete did not delete the selected track");
      await __eda.run("common.Interactive.undo"); await sleep(500);
      t(((await state()).routing?.tracks ?? []).some((q) => q.id === trk), "nor did Undo undo it");
      // left: the first corner; two more (free-angle with Ctrl: the 45-degree line mode would bend the second leg in, round 4 C-4) and close
      await click(toScreen, fx, fy + 8, { ctrlKey: true });
      await click(toScreen, fx - 6, fy + 4, { ctrlKey: true });
      await __eda.run("pcbnew.InteractiveDrawing.closeOutline"); await sleep(800);
      const made = ((await state()).drawings?.shapes ?? []).filter((s) => !shapesBefore.includes(s.id));
      t(made.length === 1 && made[0].kind === "polygon", `one polygon made (${made.map((s) => s.kind).join(",")})`);
      const pts = made[0]?.pts ?? [];
      t(pts.length === 3, `with the corners left: the first and the two after (${pts.length} points)`);
      t(!pts.some(([x, y]) => Math.abs(x / 1e6 - (fx + 10)) < 0.6), "the removed corners are not in it");
      await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C-6: Undo in the measure tool undoes the board", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const textsBefore = ((await state()).drawings?.texts ?? []).map((q) => q.id);
      await post({ op: "add_text", text: { content: "R3", at: mm(fx, fy), angle: 0, layer: "F.SilkS", size_nm: 1000000, stroke_width: 150000, justify: "center", mirror: false } });
      await sleep(400);
      t(newIds(textsBefore, ((await state()).drawings?.texts ?? []).map((q) => q.id)).length === 1, "a text added");
      await __eda.run("common.Interactive.measureTool"); await sleep(400);
      await click(toScreen, fx, fy);
      await hover(toScreen, fx + 5, fy);
      await __eda.run("common.Interactive.undo"); await sleep(700);
      t(newIds(textsBefore, ((await state()).drawings?.texts ?? []).map((q) => q.id)).length === 0, "Undo took the text away");
      await escape(); await escape();
      t((await snapshot()) === base, "the board is as it was");
    });

    await scenario("B-1: Cut of a table cell empties the cell and keeps the table", async (t) => {
      const before = ((await state()).drawings?.tables ?? []).map((q) => q.id);
      await post({ op: "table", verb: "draw", layer: "F.SilkS", at: mm(fx, fy), cols: 2, rows: 2, cell_w_nm: 5000000, cell_h_nm: 3000000, line_width_nm: 100000 });
      await sleep(400);
      const tid = newIds(before, ((await state()).drawings?.tables ?? []).map((q) => q.id))[0];
      t(!!tid, "a table drawn");
      await post({ op: "table", verb: "edit_cells", id: tid, cells: [[0, 1]], edit: { text: "cut me" } });
      await sleep(400);
      await select([`${tid}:0:1`]);
      await __eda.run("common.Interactive.cut"); await sleep(900);
      const tb = ((await state()).drawings?.tables ?? []).find((q) => q.id === tid);
      t(!!tb, "the table is still there");
      const cell = tb?.cells.find((c) => c.row === 0 && c.col === 1);
      t(cell && cell.text === "", `the cell is empty (${JSON.stringify(cell?.text)})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C-2: a net change on a track touching a pad of its net asks to update the pad", async (t) => {
      if (!otherNet) { t(false, "the board needs a second net"); return; }
      const tracksBefore = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 200000, pts: [{ x: padOnNet.x, y: padOnNet.y }, { x: padOnNet.x, y: padOnNet.y + 8000000 }] });
      await sleep(400);
      const trk = newIds(tracksBefore, ((await state()).routing?.tracks ?? []).map((q) => q.id))[0];
      t(!!trk, "a track from the pad");
      const run = async (answer) => {
        await select([trk]);
        await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(700);
        const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Track & Via Properties"));
        t(!!dlg, "Track & Via Properties opens");
        if (!dlg) return;
        setValue(dlg.querySelector("select[aria-label=Net]"), otherNet); await sleep(150);
        buttonText(dlg, "OK").click();
        // the track may also touch copper of another net on its way: KiCad asks about the short first (`confirmShortingNets`)
        const short = await question("Applying these changes will short net", 1500);
        if (short) {
          t(short.textContent.includes(otherNet), "the short question names the new net");
          buttonText(short, "Apply Anyway").click(); await sleep(400);
        }
        const q = await question("Changing the net will also update");
        t(!!q, `it asks (${q ? q.textContent.slice(0, 90) : "no question"})`);
        if (!q) return;
        t(q.textContent.includes(`${part.ref} pad ${padOnNet.num} to ${otherNet}`), "naming the pad and the net");
        buttonText(q, answer).click(); await sleep(1500);
      };
      await run("Leave Nets Unchanged");
      let st = await state();
      t(st.routing.tracks.find((q) => q.id === trk)?.net === padOnNet.net, "Leave Nets Unchanged: the track keeps its net");
      await run("Change Nets");
      st = await state();
      // (the net's shown name is KiCad's, from its new driver: a pad that joins it can rename it, so the check is that both moved together, off the old net)
      const trackNet = st.routing.tracks.find((q) => q.id === trk)?.net;
      const padNet = st.parts.find((p) => p.ref === part.ref)?.pads.find((q) => q.num === padOnNet.num)?.net;
      t(trackNet !== padOnNet.net, `Change Nets: the track left its net (${trackNet})`);
      t(padNet === trackNet, `and the pad is on the track's new net (${padNet})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("D-8: Break Track on a locked track asks, and both halves stay locked", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const tracksBefore = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx - 5, fy + 5), mm(fx + 15, fy + 5)] });
      await sleep(400);
      const trk = newIds(tracksBefore, ((await state()).routing?.tracks ?? []).map((q) => q.id))[0];
      await post({ op: "set_locked", ids: [trk], locked: true });
      await sleep(300);
      await select([trk]);
      await hover(toScreen, fx + 5, fy + 5);
      await __eda.run("pcbnew.InteractiveRouter.BreakTrack"); await sleep(300);
      const q = await question("The selected item is locked.");
      t(!!q, "it asks");
      if (!q) return;
      buttonText(q, "Break Track").click(); await sleep(900);
      const st = await state();
      const halves = newIds(tracksBefore, st.routing.tracks.map((x) => x.id));
      t(halves.length === 2 && !halves.includes(trk), `the track is in two (${halves.length})`);
      t(halves.every((id) => (st.locked ?? []).includes(id)), "both halves are locked");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("D-10: Simplify Shapes keeps the polygon's id and group", async (t) => {
      const shapesBefore = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      const ring = [mm(fx, fy), mm(fx + 5, fy + 0.001), mm(fx + 10, fy), mm(fx + 10, fy + 10), mm(fx, fy + 10)];
      await post({ op: "add_shape", shape: { kind: "polygon", layer: "F.SilkS", stroke_width: 150000, filled: false, pts: ring } });
      await post({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150000, filled: false, start: mm(fx, fy - 3), end: mm(fx + 5, fy - 3) } });
      await sleep(400);
      const [poly, seg] = newIds(shapesBefore, ((await state()).drawings?.shapes ?? []).map((s) => s.id));
      await post({ op: "group", ids: [poly, seg] });
      await sleep(400);
      const g = ((await state()).drawings?.groups ?? []).find((x) => x.member_ids.includes(poly));
      t(!!g, "the polygon is in a group");
      // a click on a member picks its group: enter the group to work on the polygon
      await select([g.id]);
      await __eda.run("common.Interactive.groupEnter"); await sleep(300);
      await select([poly]);
      await __eda.run("pcbnew.InteractiveEdit.simplifyPolygons"); await sleep(600);
      const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Simplify Shapes"));
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      setValue(dlg.querySelector("input"), 0.05); await sleep(150);
      buttonText(dlg, "OK").click(); await sleep(900);
      const st = await state();
      const s = st.drawings.shapes.find((x) => x.id === poly);
      t(!!s, "the polygon keeps its id");
      t(s && s.pts.length === 4, `the point on the line is gone (${s?.pts.length})`);
      t((st.drawings.groups ?? []).some((x) => x.id === g?.id && x.member_ids.includes(poly)), "and it is still in its group");
      await __eda.run("common.Interactive.groupLeave"); await sleep(300);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("D-6: Select Connection follows a branch from a polyline's inner vertex", async (t) => {
      const before = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx, fy), mm(fx + 5, fy), mm(fx + 10, fy)] });
      await post({ op: "add_track", net: padOnNet.net, layer: "F.Cu", width: 250000, pts: [mm(fx + 5, fy), mm(fx + 5, fy + 6)] });
      await sleep(400);
      const st = await state();
      const ids = newIds(before, st.routing.tracks.map((q) => q.id));
      const poly = st.routing.tracks.find((q) => ids.includes(q.id) && q.pts.length === 3)?.id;
      const branch = ids.find((id) => id !== poly);
      t(!!poly && !!branch, "a polyline and a branch from its middle vertex");
      await select([poly]);
      await __eda.run("pcbnew.InteractiveSelection.SelectConnection"); await sleep(500);
      const sel = __eda.state().selection.map((s) => s.id);
      t(sel.includes(branch), `the branch is selected (${sel.join(",")})`);
      await select([]);
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
