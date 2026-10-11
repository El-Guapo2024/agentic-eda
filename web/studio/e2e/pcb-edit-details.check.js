// PCB editing details check (GAPS.md package P6; items 8, 15, 17 and 35): Paste Special on the board (the reference-designator choices and Clear net assignments), Duplicate and
// Paste being one undo step with the drop that ends them (and Escape leaving nothing, not even a redo entry), items made while a group is entered joining it, the rotation
// step and the flip direction of the Preferences, the routing staying when a routed footprint is turned, flipped or deleted, Select Connection with nothing selected, Ctrl+U
// returning to mils, Alt+` switching the net highlight off and on, and Add Corner putting the corner on the edge.
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY
// of a board (it edits and undoes, and it leaves Strict off while it runs). It makes nothing that stays: every edit is undone and the board compared with how it was.
//
// It needs a board with a few placed footprints that have two or more pads on nets (the first three that are placed are used, `U1`/`R1`/`R2` in
// examples/mcu_board_30plus.yaml after `eda board place U1 --at 17.5,17.5` and a few more), and it answers through `window.__pcbEditDetailsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a minute or two (each edit waits for the board's answer), longer than a tool call may last, so it runs on its own: read it with
// `await __pcbEditDetailsCheck.wait(40000)` (it returns when the run is done or the time is up) until `.done`. In a hidden pane the browser stops timers; the script
// then drives them from a fetch loop (the `__eda.run` waits would never end otherwise).
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, open a dialog with the action that opens it),
// the dialogs' controls (their `data-testid`s) and the pointer events a user makes on the canvas; the setup edits go straight to `POST /api/cmd`.
(async () => {
  const results = [];
  const check = (window.__pcbEditDetailsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditDetailsSched) {
    // One scheduler for every timer, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 3e6;
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
    window.__pcbEditDetailsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd, carry) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(carry ? { cmd, strict: false, carry } : { cmd, strict: false }) }).then((r) => r.json());
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const placedRefs = async () => (await state()).parts.filter((p) => p.placed).map((p) => p.ref).sort();
  const refText = (p) => p?.fields?.find((f) => f.name === "Reference")?.text;
  const nets = (p) => (p?.pads ?? []).map((q) => q.net);
  /** What the scenarios touch, to compare the board with how it was: poses, copper, graphics, groups. */
  const snapshot = async () => {
    const st = await state();
    const poses = {};
    for (const p of st.parts) if (p.placed) poses[p.ref] = [p.at, p.rot, p.side, refText(p)];
    return JSON.stringify({ poses, tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.pts]), vias: (st.routing?.vias ?? []).length, zones: (st.routing?.zones ?? []).map((z) => [z.id, z.outline]), shapes: (st.drawings?.shapes ?? []).map((s) => s.id), groups: (st.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]) });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(700); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
  /** The angle between two orientations in degrees, in (-180, 180]. */
  const delta = (a, b) => { let d = (((b - a) % 360) + 360) % 360; if (d > 180) d -= 360; return d; };

  // The canvas: the pointer events a user makes. The cursor readout of the status bar gives the screen-to-board mapping.
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  /** Screen position (canvas pixels) of a board point in mm, from two probes of the cursor readout. */
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects");
    await sleep(300);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(900); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { if (__eda.state().tool !== "select") await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const placed = (await state()).parts.filter((p) => p.placed && (p.pads ?? []).length >= 2 && nets(p).some((n) => n));
  if (placed.length < 3) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs three placed footprints with pads on nets"] }); check.done = true; return "no board"; }
  const [A, B, C] = placed;
  const refA = A.ref, refB = B.ref, refC = C.ref;
  const base = await snapshot();
  const baseRefs = await placedRefs();
  const toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }

  (async () => {
    // ----- Paste Special on the board
    await scenario("Paste Special: the dialog, its three choices, Clear net assignments, and what each does to the pasted footprints", async (t) => {
      await select([refA, refB]);
      await __eda.run("common.Interactive.copy");
      await sleep(500);
      const r = await __eda.run("common.Interactive.pasteSpecial");
      await sleep(600);
      t(r.dialog === "Paste Special", `the action opens the dialog (${r.dialog})`);
      t(!!tid("paste-special-unique") && !!tid("paste-special-keep") && !!tid("paste-special-remove"), "it has KiCad's three choices");
      t(tid("paste-special-keep")?.checked === true, "it opens on keeping the reference designators (PASTE_MODE::KEEP_ANNOTATIONS)");
      t(!!tid("paste-special-clear-nets") && tid("paste-special-clear-nets").checked === false, "a board fragment is offered Clear net assignments, unchecked");
      t(document.querySelector(".dialog")?.innerText.includes("Keep existing reference designators, even if they are duplicated"), "with KiCad's own words");
      // "Clear reference designators" and "Clear net assignments"
      tid("paste-special-remove").click(); await sleep(80);
      tid("paste-special-clear-nets").click(); await sleep(80);
      buttonText(document.querySelector(".dialog"), "OK").click();
      await sleep(1200);
      let sel = __eda.state().selection.map((s) => s.id);
      t(sel.length === 2 && !sel.includes(refA) && !sel.includes(refB), `the pasted footprints are selected, new parts (${sel})`);
      t(__eda.state().tool === "move", "and carried by the Move tool");
      let parts = await Promise.all(sel.map(partOf));
      t(parts.every((p) => refText(p) === "REF**"), `every pasted reference reads REF** (${parts.map(refText)})`);
      t(parts.every((p) => nets(p).every((n) => n == null)), "and every pad is on no net");
      await escape();
      t((await placedRefs()).join() === baseRefs.join(), "Escape takes the pasted footprints away");
      // Assign unique reference designators
      await __eda.run("common.Interactive.pasteSpecial"); await sleep(600);
      tid("paste-special-unique").click(); await sleep(80);
      buttonText(document.querySelector(".dialog"), "OK").click(); await sleep(1200);
      sel = __eda.state().selection.map((s) => s.id);
      parts = await Promise.all(sel.map(partOf));
      t(parts.length === 2 && parts.every((p) => refText(p) === p.ref), `unique: each reads the reference it was given (${parts.map((p) => p.ref + "=" + refText(p))})`);
      t(parts.some((p) => nets(p).some((n) => n)), "and the nets are kept");
      await escape();
      // a plain Paste keeps the reference the clipboard had, even though another footprint has it
      await __eda.run("common.Interactive.paste"); await sleep(1200);
      sel = __eda.state().selection.map((s) => s.id);
      parts = await Promise.all(sel.map(partOf));
      t(parts.length === 2 && parts.map(refText).sort().join() === [refA, refB].sort().join(), `a plain Paste leaves the copies reading ${refA} and ${refB} (${parts.map((p) => p.ref + "=" + refText(p))})`);
      t(parts.every((p) => p.ref !== refText(p)), "though each is a part of its own under a free reference");
      await escape();
      // a footprint on its own: no Clear net assignments
      await select([refA]);
      await __eda.run("common.Interactive.copy"); await sleep(500);
      await __eda.run("common.Interactive.pasteSpecial"); await sleep(600);
      t(!!tid("paste-special-keep") && !tid("paste-special-clear-nets"), "a single footprint is not offered Clear net assignments (HideClearNets)");
      buttonText(document.querySelector(".dialog"), "Cancel").click(); await sleep(400);
      t((await placedRefs()).join() === baseRefs.join() && __eda.state().tool === "select", "Cancel pastes nothing");
      t((await snapshot()) === base, "the board is as it was");
    });

    // ----- Duplicate and Paste are one undo step with the drop; Escape leaves nothing
    await scenario("Duplicate and Paste are one undo step with the drop, and Escape leaves the history as it was", async (t) => {
      // a marker step of our own, so that "one step too many" shows
      await post({ op: "move_items", ids: [refC], dx: 1000, dy: 0 });
      await sleep(500);
      const movedC = (await partOf(refC)).at;
      const home = await hover(toScreen, A.at[0] / 1000, A.at[1] / 1000);
      void home;
      await select([refA]);
      await __eda.run("common.Interactive.duplicate"); await sleep(900);
      t(__eda.state().tool === "move" && __eda.state().selection.length === 1, "Duplicate carries the copy");
      await click(toScreen, A.at[0] / 1000 + 6, A.at[1] / 1000 + 5);
      const copy = (await placedRefs()).filter((r) => !baseRefs.includes(r));
      t(copy.length === 1, `the copy is on the board (${copy})`);
      const dropped = (await partOf(copy[0])).at;
      t(near(dropped[0], A.at[0] + 6000, 600) && near(dropped[1], A.at[1] + 5000, 600), `at the dropped place (${dropped} for ${A.at})`);
      await undo();
      t((await placedRefs()).join() === baseRefs.join(), "one Undo takes the copy away: the duplicate and the drop were one step");
      t(JSON.stringify((await partOf(refC)).at) === JSON.stringify(movedC), "and the step before it is still done");
      await redo();
      t((await placedRefs()).length === baseRefs.length + 1 && JSON.stringify((await partOf(copy[0])).at) === JSON.stringify(dropped), "one Redo brings the copy back where it was dropped");
      await undo();
      await undo();
      t(JSON.stringify((await partOf(refC)).at) === JSON.stringify(C.at), "the marker step goes with the next Undo");

      // Escape: a cancelled carry is `commit.Revert()` -- not even a redo entry
      await post({ op: "move_items", ids: [refC], dx: 1000, dy: 0 });
      await sleep(400);
      await undo();
      t(JSON.stringify((await partOf(refC)).at) === JSON.stringify(C.at), "setup: the marker step is undone, and waits for a Redo");
      await select([refA]);
      await __eda.run("common.Interactive.duplicate"); await sleep(900);
      t((await placedRefs()).length === baseRefs.length + 1, "Duplicate adds the copy");
      await escape();
      t((await placedRefs()).join() === baseRefs.join() && __eda.state().tool === "select", "Escape takes it away");
      await redo();
      t(JSON.stringify((await partOf(refC)).at) === JSON.stringify(movedC), "and the earlier Redo is still there");
      await undo();

      // Paste and its drop
      await select([refA, refB]);
      await __eda.run("common.Interactive.copy"); await sleep(500);
      await __eda.run("common.Interactive.paste"); await sleep(1200);
      t(__eda.state().selection.length === 2, "Paste carries both footprints");
      await click(toScreen, A.at[0] / 1000 + 6, A.at[1] / 1000 + 8);
      t((await placedRefs()).length === baseRefs.length + 2, "dropped");
      await undo();
      t((await placedRefs()).join() === baseRefs.join(), "one Undo takes the whole paste away");
      t((await snapshot()) === base, "the board is as it was");
    });

    // ----- items made while a group is entered join it
    await scenario("a shape drawn and a paste made while a group is entered join the group", async (t) => {
      await select([refB, refC]);
      await __eda.run("common.Interactive.group"); await sleep(700);
      const g = (await state()).drawings.groups.find((q) => q.member_ids.includes(refB) && q.member_ids.includes(refC));
      t(!!g, "the two footprints are a group");
      await select([g.id]);
      await __eda.run("common.Interactive.groupEnter"); await sleep(400);
      t(__eda.state().entered === g.id, "the group is entered");
      await __eda.run("pcbnew.InteractiveDrawing.line"); await sleep(300);
      await click(toScreen, 5, 31);
      await click(toScreen, 12, 31);
      await escape();
      let st = await state();
      const shape = st.drawings.shapes.find((s) => !(base.includes(s.id)));
      const now = st.drawings.groups.find((q) => q.id === g.id);
      t(!!shape && now.member_ids.includes(shape.id), `the line drawn with the group entered is a member of it (${shape?.id} in ${now.member_ids})`);
      // (with a group entered only its own members can be picked: selecting something outside it would leave the group)
      await select([refB]);
      await __eda.run("common.Interactive.copy"); await sleep(500);
      t(__eda.state().entered === g.id, "the group is still entered");
      await __eda.run("common.Interactive.paste"); await sleep(1200);
      const pasted = __eda.state().selection.map((s) => s.id);
      await click(toScreen, 30, 31);
      st = await state();
      const after = st.drawings.groups.find((q) => q.id === g.id);
      t(pasted.length === 1 && after.member_ids.includes(pasted[0]), `the pasted footprint is a member too (${pasted} in ${after.member_ids})`);
      t(!st.drawings.groups.some((q) => q.id !== g.id && q.member_ids.includes(pasted[0])), "and of no other group");
      // leave, undo the paste, the line and the group
      await __eda.run("common.Interactive.groupLeave"); await sleep(300);
      for (let i = 0; i < 4 && (await snapshot()) !== base; i++) await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- the rotation step and the flip direction of the Preferences
    await scenario("the rotation step and the flip direction of the Preferences reach Rotate, Flip and a carried copy", async (t) => {
      const setPrefs = async (stepDeg, flip) => {
        await __eda.run("common.SuiteControl.openPreferences"); await sleep(500);
        setValue(tid("pref-rotation-step"), stepDeg); await sleep(100);
        tid(flip === "top_bottom" ? "pref-flip-top-bottom" : "pref-flip-left-right").click(); await sleep(100);
        buttonText(document.querySelector(".dialog"), "OK").click(); await sleep(500);
      };
      const p0 = await partOf(refA);
      await select([refA]);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(800);
      const d90 = delta(p0.rot, (await partOf(refA)).rot);
      t(Math.abs(Math.abs(d90) - 90) < 1e-6, `R turns a quarter by default (${d90})`);
      await undo();
      await __eda.run("common.SuiteControl.openPreferences"); await sleep(500);
      const step = tid("pref-rotation-step");
      t(!!step && step.value === "90" && tid("pref-flip-left-right")?.checked === true, "Preferences has the step (90) and the flip direction (left/right) of KiCad's defaults");
      buttonText(document.querySelector(".dialog"), "Cancel").click(); await sleep(300);
      try {
        await setPrefs(15, "top_bottom");
        await select([refA]);
        await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(800);
        const d15 = delta(p0.rot, (await partOf(refA)).rot);
        t(near(d15, d90 / 6, 1e-6), `R turns by the step now: ${d15} for ${d90} before`);
        await undo();
        // a carried copy turns by the step too
        await hover(toScreen, p0.at[0] / 1000, p0.at[1] / 1000);
        await select([refA]);
        await __eda.run("common.Interactive.duplicate"); await sleep(900);
        const copy = __eda.state().selection[0]?.id;
        t(!!copy && copy !== refA, `Duplicate carries a copy (${copy})`);
        await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(300);
        await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(300);
        await click(toScreen, p0.at[0] / 1000 + 5, p0.at[1] / 1000 + 6);
        const c = copy ? await partOf(copy) : null;
        t(!!c && near(Math.abs(delta(p0.rot, c.rot)), Math.abs(d90) / 3, 1e-6), `two presses of R during the carry turn the copy by 2 steps (${c ? delta(p0.rot, c.rot) : "no copy"})`);
        await undo();
        // F flips across the top/bottom axis: the angle becomes 180 - rot (left/right would make it -rot)
        await select([refA]);
        await __eda.run("pcbnew.InteractiveEdit.flip"); await sleep(800);
        const f = await partOf(refA);
        t(f.side !== p0.side, "F puts the footprint on the other side");
        t(near(delta(f.rot, (((180 - p0.rot) % 360) + 360) % 360), 0, 1e-6), `across the top/bottom axis (rot ${p0.rot} -> ${f.rot}, expected ${(((180 - p0.rot) % 360) + 360) % 360})`);
        await undo();
      } finally {
        await setPrefs(90, "left_right");
      }
      await __eda.run("common.SuiteControl.openPreferences"); await sleep(500);
      t(tid("pref-rotation-step")?.value === "90" && tid("pref-flip-left-right")?.checked === true, "and the defaults are back");
      buttonText(document.querySelector(".dialog"), "Cancel").click(); await sleep(300);
      t((await snapshot()) === base, "the board is as it was");
    });

    await scenario("turning, flipping and deleting a routed footprint leaves its tracks where they are", async (t) => {
      const p = await partOf(refA);
      const pad = p.pads.find((q) => q.net);
      await post({ op: "add_track", net: pad.net, layer: "F.Cu", width: 200, pts: [{ x: pad.x, y: pad.y }, { x: pad.x + 3000, y: pad.y - 2000 }] });
      await sleep(500);
      const tracks = async () => ((await state()).routing?.tracks ?? []).length;
      t((await tracks()) === 1, "a track runs out of a pad of the footprint");
      await select([refA]);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(900);
      t((await tracks()) === 1 && (await partOf(refA)).rot !== p.rot, "R turns the footprint and the track stays");
      await __eda.run("pcbnew.InteractiveEdit.flip"); await sleep(900);
      t((await tracks()) === 1 && (await partOf(refA)).side !== p.side, "F flips it and the track stays");
      const act = (await state()).activity.slice(0, 3).map((a) => a.message ?? "");
      t(!act.some((m) => /routing cleared/.test(m)), `no edit says the routing was cleared (${act.map((m) => m.slice(0, 40))})`);
      await __eda.run("common.Interactive.delete"); await sleep(900);
      t(!(await placedRefs()).includes(refA) && (await tracks()) === 1, "deleting the footprint leaves the track");
      await undo(); await undo(); await undo(); await undo();
      t((await tracks()) === 0 && (await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- Select Connection with nothing selected
    await scenario("Select Connection with nothing selected takes the track under the cursor and its connection", async (t) => {
      const p = await partOf(refA);
      const pad = p.pads.find((q) => q.net);
      const a = [pad.x, pad.y], b = [pad.x + 4000, pad.y], c = [pad.x + 4000, pad.y + 4000];
      await post({ op: "add_track", net: pad.net, layer: "F.Cu", width: 200, pts: [{ x: a[0], y: a[1] }, { x: b[0], y: b[1] }] });
      await post({ op: "add_track", net: pad.net, layer: "F.Cu", width: 200, pts: [{ x: b[0], y: b[1] }, { x: c[0], y: c[1] }] });
      await sleep(600);
      const ids = ((await state()).routing?.tracks ?? []).map((q) => q.id);
      t(ids.length === 2, "two connected tracks");
      await select([]);
      await hover(toScreen, (a[0] + b[0]) / 2000, a[1] / 1000);
      await __eda.run("pcbnew.InteractiveSelection.SelectConnection"); await sleep(600);
      const sel = __eda.state().selection.map((s) => s.id);
      t(ids.every((id) => sel.includes(id)), `no selection, the cursor on the first track: both tracks are selected (${sel})`);
      await select([]);
      await hover(toScreen, 2, 2);
      await __eda.run("pcbnew.InteractiveSelection.SelectConnection"); await sleep(400);
      t(__eda.state().selection.length === 0, "nothing under the cursor selects nothing");
      await undo(); await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- Ctrl+U returns to mils; Alt+` switches the highlight off and on
    await scenario("Ctrl+U goes back to mils after mils were picked; Alt+` toggles the net highlight without a selection", async (t) => {
      await __eda.run("common.Control.imperialUnits"); await sleep(200);
      await __eda.run("common.Control.metricUnits"); await sleep(200);
      t(__eda.state().units === "mm", "millimetres");
      await __eda.run("common.Control.toggleUnits"); await sleep(200);
      t(__eda.state().units === "in", `Ctrl+U goes to inches while inches were the last imperial unit picked (${__eda.state().units})`);
      await __eda.run("common.Control.mils"); await sleep(200);
      t(__eda.state().units === "mil", "mils picked");
      await __eda.run("common.Control.toggleUnits"); await sleep(200);
      t(__eda.state().units === "mm", "Ctrl+U goes to millimetres");
      await __eda.run("common.Control.toggleUnits"); await sleep(200);
      t(__eda.state().units === "mil", `and back to mils, not inches (${__eda.state().units})`);
      await __eda.run("common.Control.metricUnits"); await sleep(200);

      await select([refA]);
      await __eda.run("pcbnew.EditorControl.highlightNetSelection"); await sleep(400);
      const shown = __eda.state().highlight;
      t(shown.length > 0, `the nets of the selected footprint are highlighted (${shown})`);
      await select([]);
      await __eda.run("pcbnew.EditorControl.toggleNetHighlight"); await sleep(300);
      t(__eda.state().highlight.length === 0, "Alt+` switches it off, whatever is selected");
      await __eda.run("pcbnew.EditorControl.toggleNetHighlight"); await sleep(300);
      t(JSON.stringify(__eda.state().highlight) === JSON.stringify(shown), `and brings the same nets back (${__eda.state().highlight})`);
      await __eda.run("pcbnew.EditorControl.clearHighlight"); await sleep(300);
      await __eda.run("pcbnew.EditorControl.toggleNetHighlight"); await sleep(300);
      t(__eda.state().highlight.length === 0, "once cleared there is nothing to bring back");
    });

    // ----- Add Corner puts the corner on the edge
    await scenario("Add Corner puts the new corner on the nearest edge of a zone and splits a line there", async (t) => {
      const netName = (await partOf(refA)).pads.find((q) => q.net).net;
      const sq = [[11000, 11000], [21000, 11000], [21000, 21000], [11000, 21000]].map(([x, y]) => ({ x, y }));
      await post({ op: "add_zone", net: netName, layer: "F.Cu", outline: sq });
      await sleep(600);
      const zone = ((await state()).routing?.zones ?? [])[0];
      t(!!zone, "a zone");
      await select([zone.id]);
      // the cursor is 0.7 mm above the upper edge (y = 11 mm), at x = 15.4 mm: the grid takes it to (15, 10), the nearest point of the edge is (15, 11)
      await hover(toScreen, 15.4, 10.3);
      await __eda.run("pcbnew.PointEditor.addCorner"); await sleep(800);
      const z2 = ((await state()).routing?.zones ?? []).find((q) => q.id === zone.id);
      t(z2.outline.length === 5, `the outline has a corner more (${z2.outline.length})`);
      const added = z2.outline.find((q) => !zone.outline.some((o) => o[0] === q[0] && o[1] === q[1]));
      t(!!added && added[0] === 15000 && added[1] === 11000, `the new corner is the nearest point of the edge, (15, 11) mm, not where the cursor is (${added})`);
      await undo();
      await post({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 5000, y: 33000 }, end: { x: 15000, y: 33000 } } });
      await sleep(500);
      const seg = ((await state()).drawings?.shapes ?? []).find((s) => s.kind === "segment" && s.start[1] === 33000 && s.end[0] === 15000);
      await select([seg.id]);
      await hover(toScreen, 9.1, 32.4);
      await __eda.run("pcbnew.PointEditor.addCorner"); await sleep(800);
      const segs = ((await state()).drawings?.shapes ?? []).filter((s) => s.kind === "segment" && s.start[1] === 33000 && s.end[1] === 33000);
      t(segs.length === 2 && segs.some((s) => s.end[0] === 9000) && segs.some((s) => s.start[0] === 9000), `the line is two lines meeting at (9, 33) mm, on it (${segs.map((s) => s.start[0] + ".." + s.end[0])})`);
      for (let i = 0; i < 3 && (await snapshot()) !== base; i++) await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- Rotate about several items: the pivot is the best snap anchor of the centre of their box
    await scenario("Rotate about several footprints turns them about a grid point or an anchor, not about a point between", async (t) => {
      const grid = __eda.state().grid;
      const a0 = (await partOf(refA)).at, b0 = (await partOf(refB)).at;
      await select([refA, refB]);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(900);
      const a1 = (await partOf(refA)).at, b1 = (await partOf(refB)).at;
      // a quarter turn counter-clockwise on a y-down screen takes (dx, dy) about P to (P.x + dy, P.y - dx): solve for P from the first footprint
      const px = (a1[0] - a0[1] + a1[1] + a0[0]) / 2;
      const py = (a1[1] + a0[0] - a1[0] + a0[1]) / 2;
      // (a footprint lands on the 100 um placement grid, so the pivot worked back from two poses is good to about that)
      const offGrid = (v) => Math.min(Math.abs(v % grid), grid - Math.abs(v % grid));
      const onGrid = offGrid(px) <= 160;
      const onGridY = offGrid(py) <= 160;
      const anchor = [a0, b0].some((o) => near(o[0], px, 200) && near(o[1], py, 200));
      t((onGrid && onGridY) || anchor, `the pivot (${px}, ${py}) is on the ${grid} um grid or at a footprint's origin`);
      // the same pivot turns the second footprint
      t(near(b1[0], px + (b0[1] - py), 250) && near(b1[1], py - (b0[0] - px), 250), `and it is the pivot of the other one too (${b0} -> ${b1})`);
      await undo();
      t((await snapshot()) === base, "the board is as it was");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
