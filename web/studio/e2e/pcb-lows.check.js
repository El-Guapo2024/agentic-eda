// The PCB editor's and router's smaller differences of UI-LEFT P24-P29, R8 and R10 (task-u5-pcblows), as they look in the page:
//   * P24: Cleanup Tracks & Vias merges two overlapping collinear tracks into the first, which keeps its id (`mergeCollinearSegments`);
//     Intersect Polygons says "Some of the polygons could not be intersected." when one shape is apart (`GetStatusMessage`);
//     Break Track on a locked track asks with KiCad's "Do not show again" box, and once ticked the question is answered at once (`KIDIALOG`).
//   * R10: a router-mode change shows no toast (`ChangeRouterMode` only updates the message panel).
//   * R8: Pre-defined Sizes' Add makes a row of 0, its drill left empty (`AppendTrackWidth( 0 )`, `AppendViaSize( 0, 0 )`).
//   * P29: Add Rule Area's dialog keeps out tracks, vias and pads (`ZONE_SETTINGS::ZONE_SETTINGS()`).
// Run it in the page of a served COPY of a routed board (it edits and undoes); it answers through `window.__pcbLowsCheck`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbLowsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbLowsSched) {
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
    window.__pcbLowsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const postJson = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    const r = st.routing ?? {};
    return JSON.stringify({
      tracks: (r.tracks ?? []).map((t) => [t.id, t.net, t.width, t.pts]),
      vias: (r.vias ?? []).map((v) => [v.id, v.x, v.y]),
      zones: (r.zones ?? []).map((z) => [z.id, z.outline]),
      presets: [r.track_width_presets ?? [], r.via_presets ?? []],
      shapes: (st.drawings?.shapes ?? []).map((s) => s.id),
      locked: st.locked ?? [],
    });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };
  const postSeen = async (cmd) => {
    const rev0 = __eda.state().revision;
    const reply = await post(cmd);
    await waitFor(() => __eda.state().revision !== rev0, 8000);
    await sleep(400);
    return reply;
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
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(250); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(500); };

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
  const st0 = await state();
  const routing0 = st0.routing ?? { tracks: [], vias: [], zones: [] };
  if (routing0.tracks.length === 0) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs a track"] }); check.done = true; return "no board"; }
  const base = await snapshot();
  const toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: no view transform"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 12 && (await snapshot()) !== base; i++) await undo(); };
  const outline = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const [bx0, by0] = outline.length ? [Math.min(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))] : [0, 0];
  // a free spot below the board's outline, on whole millimetres
  const [fx, fy] = [Math.round(bx0) + 5, Math.round(by0) + 10];
  const net = routing0.tracks[0].net;
  const bodyHas = (text) => document.body.innerText.includes(text);

  (async () => {
    await scenario("P24: Cleanup Tracks merges two overlapping collinear tracks into the first, which keeps its id; one undo step", async (t) => {
      await postSeen({ op: "add_track", net, layer: "B.Cu", width: 250_000, pts: [mm(fx, fy), mm(fx + 6, fy)] });
      await postSeen({ op: "add_track", net, layer: "B.Cu", width: 250_000, pts: [mm(fx + 4, fy), mm(fx + 10, fy)] });
      const find = (st, x0) => (st.routing?.tracks ?? []).find((q) => q.layer === "B.Cu" && q.pts[0][0] === mm(x0, fy).x && q.pts[0][1] === mm(x0, fy).y);
      const st1 = await state();
      const a = find(st1, fx), b = find(st1, fx + 4);
      t(!!a && !!b, "two overlapping tracks on B.Cu");
      if (!a || !b) return;
      const reply = await postJson("/api/cleanup_tracks/apply", { merge_segments: true, selected_ids: [a.id, b.id] });
      t(reply.ok === true && (reply.changes ?? []).some((c) => c.kind === "merged_tracks"), `the cleanup merged them (${JSON.stringify((reply.changes ?? []).map((c) => c.kind))})`);
      await sleep(600);
      const st2 = await state();
      const merged = (st2.routing?.tracks ?? []).find((q) => q.id === a.id);
      t(!!merged && JSON.stringify(merged.pts) === JSON.stringify([[mm(fx, fy).x, mm(fx, fy).y], [mm(fx + 10, fy).x, mm(fx + 10, fy).y]]), `the first track spans both, under its own id (${merged && JSON.stringify(merged.pts)})`);
      t(!(st2.routing?.tracks ?? []).some((q) => q.id === b.id), "the second is gone");
      await undo();
      const st3 = await state();
      t(!!find(st3, fx) && !!find(st3, fx + 4), "one undo brings the two back");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("P24: Intersect Polygons with one shape apart says KiCad's \"Some of the polygons could not be intersected.\"", async (t) => {
      const rect = (x0, y0, x1, y1) => ({ kind: "rect", layer: "Dwgs.User", stroke_width: 100_000, filled: false, start: mm(x0, y0), end: mm(x1, y1) });
      const before = ((await state()).drawings?.shapes ?? []).map((s) => s.id);
      await postSeen({ op: "add_shape", shape: rect(fx, fy, fx + 10, fy + 10) });
      await postSeen({ op: "add_shape", shape: rect(fx + 6, fy + 6, fx + 16, fy + 16) });
      await postSeen({ op: "add_shape", shape: rect(fx + 30, fy, fx + 32, fy + 2) });
      const shapes = ((await state()).drawings?.shapes ?? []).filter((s) => !before.includes(s.id));
      const startX = (s) => s.start?.[0] ?? s.start?.x;
      const apart = shapes.find((s) => startX(s) === mm(fx + 30, fy).x);
      const made = [apart, ...shapes.filter((s) => s !== apart)].filter(Boolean).map((s) => s.id);
      t(made.length === 3 && !!apart, `three rectangles (${made.length})`);
      // the last one added is the donor (`GetLastAddedItem` swapped to the front): one of the two that overlap, the one apart first
      await select(made);
      await __eda.run("pcbnew.InteractiveEdit.intersectPolygons");
      const said = await waitFor(() => bodyHas("Some of the polygons could not be intersected."), 6000);
      t(said, "the info message shows");
      const after = ((await state()).drawings?.shapes ?? []).filter((s) => !before.includes(s.id));
      t(after.length === 2, `the intersection and the shape apart are left (${after.length})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("P24 / R10: Break Track on a locked track asks with \"Do not show again\"; ticked, the next time it is answered at once", async (t) => {
      await postSeen({ op: "add_track", net, layer: "F.Cu", width: 250_000, pts: [mm(fx, fy + 20), mm(fx + 10, fy + 20)] });
      const track = ((await state()).routing?.tracks ?? []).find((q) => q.layer === "F.Cu" && q.pts[0][0] === mm(fx, fy + 20).x && q.pts[0][1] === mm(fx, fy + 20).y);
      t(!!track, "a straight track");
      if (!track) return;
      await postSeen({ op: "set_locked", ids: [track.id], locked: true });
      const toScreenNow = await screenOf();
      const count = async () => ((await state()).routing?.tracks ?? []).filter((q) => q.layer === "F.Cu" && q.pts.some((p) => p[1] === mm(0, fy + 20).y)).length;
      await select([track.id]);
      await hover(toScreenNow, fx + 3, fy + 20);
      void __eda.run("pcbnew.InteractiveRouter.BreakTrack");
      const asked = await waitFor(() => !!document.querySelector('[data-testid="do-not-show-again"]'), 4000);
      t(asked, "the locked track's question shows a Do not show again box");
      if (!asked) return;
      t(bodyHas("The selected item is locked."), "in KiCad's words");
      document.querySelector('[data-testid="do-not-show-again"]').click();
      await sleep(150);
      buttonText(document, "Break Track")?.click();
      t(await waitFor(async () => (await count()) === 2, 6000), "Break Track broke it in two");
      await undo();
      t((await count()) === 1, "one undo joins it again");
      await select([track.id]);
      await hover(toScreenNow, fx + 6, fy + 20);
      void __eda.run("pcbnew.InteractiveRouter.BreakTrack");
      await sleep(600);
      t(!document.querySelector('[data-testid="do-not-show-again"]'), "no question this time (g_doNotShowAgainDlgs)");
      t(await waitFor(async () => (await count()) === 2, 6000), "and the track is broken");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R10: a router-mode change shows no toast (only the message panel says it)", async (t) => {
      await __eda.run("pcbnew.InteractiveRouter.ShoveMode");
      await sleep(300);
      t(!bodyHas("Router mode:"), "no \"Router mode:\" toast");
      await __eda.run("pcbnew.InteractiveRouter.WalkaroundMode");
      await sleep(300);
      t(!bodyHas("Router mode:"), "none for Walk around either");
      await __eda.run("pcbnew.InteractiveRouter.HighlightMode");
      await sleep(200);
    });

    await scenario("R8: Pre-defined Sizes' Add makes a row of 0 and a via row with no drill", async (t) => {
      await __eda.run("pcbnew.EditorControl.boardSetup");
      await sleep(600);
      [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Pre-defined Sizes"))?.click();
      await sleep(400);
      const p = document.querySelector('section.bs-page[aria-label="Pre-defined Sizes"]');
      t(!!p && !p.hidden, "the page is on screen");
      if (!p) return;
      const inputs = () => [...p.querySelectorAll("input[type=number]")];
      const n0 = inputs().length;
      buttonText(p, "Add Track Width")?.click();
      await sleep(250);
      const widths = inputs();
      t(widths.length === n0 + 1, "a row was added");
      const nWidths = (st0.routing?.track_width_presets ?? []).length;
      t(widths[nWidths]?.value === "0", `the new width is 0 (${widths[nWidths]?.value})`);
      buttonText(p, "Add Via Size")?.click();
      await sleep(250);
      const all = inputs();
      // the last via row's two fields come before the pairs' rows (none on this board)
      const nVias = (st0.routing?.via_presets ?? []).length;
      const viaStart = nWidths + 1 + 2 * nVias;
      t(all[viaStart]?.value === "0" && all[viaStart + 1]?.value === "", `the new via is 0 with an empty drill (${all[viaStart]?.value} / "${all[viaStart + 1]?.value}")`);
      buttonText(document.querySelector(".bs-dialog"), "Close")?.click() ?? buttonText(document, "Cancel")?.click();
      await sleep(300);
      // Close asks before discarding what was not applied
      buttonText(document, "Discard Changes")?.click();
      await sleep(300);
    });

    await scenario("P29: Add Rule Area's dialog starts keeping out tracks, vias and pads, not copper pours or footprints", async (t) => {
      await __eda.run("pcbnew.InteractiveDrawing.ruleArea");
      await sleep(300);
      await click(toScreen, fx, fy + 30);
      const open = await waitFor(() => [...document.querySelectorAll("label")].some((l) => l.textContent.trim() === "Copper pours"), 4000);
      t(open, "the rule area's dialog opened at the first corner");
      if (!open) return;
      const box = (label) => [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === label)?.querySelector("input");
      const got = ["Tracks", "Vias", "Pads", "Copper pours", "Footprints"].map((l) => box(l)?.checked);
      t(JSON.stringify(got) === JSON.stringify([true, true, true, false, false]), `Tracks, Vias, Pads kept out; Copper pours, Footprints allowed (${JSON.stringify(got)})`);
      await closeDialogs();
      await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "nothing was made");
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
