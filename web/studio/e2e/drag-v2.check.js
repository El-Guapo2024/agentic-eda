// Drag check, round 2 (docs/parity/UI-LEFT-2.md RR1, RR10; crates/pns/PARITY.md Stage 26): what `ROUTER_TOOL::InlineDrag` does with the selection and the net
// highlight (pcbnew/router/router_tool.cpp at 8303b2ad: `ACTIONS::selectionClear` at the start, `restoreSelection( selection )` on Esc, `selectItems` of
// `GetLastCommittedLeaderSegments()` after a multi-track drag, `highlightNets( true, { closestItem->Net() } )` while it goes on), and a corner drag at the
// joint of a selected track with a locked one (KiCad's `DRAGGER`: the joint's virtual via is dragged, and the locked track's end follows it). Driven the way a
// person does it -- the pointer on the canvas, the `D` action, a click, Esc, Ctrl+Z -- and read from the `/api/route/*` replies and `/api/state`.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board
// below: it places R1 and R2 if they are not, adds tracks and undoes everything it did, and it ends by checking the board is as it found it.
//
//   eda board new examples/mcu_board_30plus.yaml -o board        # 34 x 34 mm, outline only: the parts are unplaced
//   eda board serve -C board --port 8840 --ui web/studio/dist
//
// What it checks:
//   multi-esc     two parallel tracks (y = 18 and 20 mm) selected, the pointer on the first and D: a multi-track drag; while it goes on nothing is selected and
//                 the first track's net is highlighted (the item nearest the cursor, `highlight_net`); a move and Esc: the two tracks are selected again and the
//                 highlight is off, as it was;
//   multi-commit  the same, a move 2 mm north and a click: the commit selects the two new tracks (the leader segments' tracks, `select`), the highlight is off;
//                 one Ctrl+Z puts both back;
//   single        one track selected and D on its middle: nothing selected while dragging, its net highlighted; Esc selects it again; D again, a move and a
//                 click: committed, and nothing is selected (a single drag has no leaders);
//   locked        a track whose end meets a locked track: selected, D on that end takes hold of the joint's virtual via (`virtual_via`), a move and a click: both
//                 tracks' ends go to the new joint (KiCad's `findViaFanoutByHandle` takes the locked line along), the new track is not locked (as in KiCad);
//                 one Ctrl+Z puts both back, the locked one locked.
// It answers through `window.__dragV2Check`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__dragV2Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name) }; };

  // what the studio asks of the router
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/api\/route\/([a-z_]+)/.exec(url);
    if (m) replies.push({ name: m[1], req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
  const lastReply = (name, from = 0) => replies.slice(from).filter((x) => x.name === name).pop();
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };

  // the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const pointerTo = async (wx, wy) => {
    for (let i = 0; i < 4; i++) {
      const was = JSON.stringify(E.state().snap);
      const [x, y] = w2c(wx, wy);
      fire("pointermove", x, y, { buttons: 0 });
      await sleep(150);
      const sn = E.state().snap;
      if (!sn || !sn.input || JSON.stringify(sn) === was || Math.hypot(sn.input[0] - wx, sn.input[1] - wy) < 20) return [x, y];
    }
    return null;
  };
  const clickAt = async (wx, wy, mods = {}) => {
    const at = await pointerTo(wx, wy);
    if (!at) throw new Error(`the pointer could not be put on ${wx}, ${wy}`);
    fire("pointerdown", at[0], at[1], mods);
    await sleep(40);
    fire("pointerup", at[0], at[1], mods);
    await sleep(250);
  };
  const moveTo = async (wx, wy, name = "drag_move") => {
    const n = replies.length;
    if (!(await pointerTo(wx, wy))) return null;
    await waitReply(name, n, 8000);
    await quiet();
    return lastReply(name, n);
  };
  const state = () => json("/api/state");
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const partOf = (st, ref) => st.parts.find((p) => p.ref === ref);
  const lockedIds = (st) => new Set(st.locked ?? []);
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), st.parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot]), [...lockedIds(st)].sort()]);
  const settle = async () => {
    const st = await state();
    for (let i = 0; i < 50 && !(E.state().counts.tracks === tracksOf(st).length); i++) await sleep(150);
    await sleep(900);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const clearSelection = async () => { for (let i = 0; i < 3 && (E.state().selection.length || (E.state().highlight ?? []).length); i++) { await E.run("common.Interactive.cancel"); await sleep(200); } };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(600); await settle(); };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
  const nearPt = (p, q, tol = 2) => !!p && !!q && near(p[0], q[0], tol) && near(p[1], q[1], tol);
  const ids = () => E.state().selection.map((x) => x.id).sort();
  const highlight = () => [...(E.state().highlight ?? [])];
  let original = null;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const before = await state();
  original = snapshot(before);
  const madeSteps = [];
  // R1 and R2 out of the way, so that their nets are the board's (the tracks below take them)
  if (!partOf(before, "R1")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "R1", x: 6_000_000, y: 6_000_000 }));
  if (!partOf(before, "R2")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "R2", x: 28_000_000, y: 28_000_000 }));
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);
  let st = await state();
  const nets = [...new Set(st.parts.flatMap((p) => p.pads ?? []).map((p) => p.net).filter((x) => !!x))];
  const line = (net, pts) => ({ net, layer: "F.Cu", width: 200_000, pts: pts.map(([x, y]) => ({ x, y })) });
  const [Y0, Y1, Y2] = [16_000_000, 18_000_000, 20_000_000];
  madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[0], [[5_000_000, Y1], [25_000_000, Y1]]), line(nets[1], [[5_000_000, Y2], [25_000_000, Y2]])], vias: [] }));
  await settle();
  st = await state();
  const withBus = snapshot(st);
  const busIds = () => tracksOf(st).filter((x) => x.net === nets[0] || x.net === nets[1]).map((x) => x.id).sort();
  const startBusDrag = async (t) => {
    await clearSelection();
    await clickAt(15_000_000, Y1);
    await clickAt(15_000_000, Y2, { shift: true });
    t(JSON.stringify(ids()) === JSON.stringify(busIds()), `the two tracks are selected: ${JSON.stringify(ids())}`);
    await pointerTo(15_000_000, Y1);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    await sleep(400);
    t(s && s.body.ok === true && s.body.drag === "multi", `D on them starts a multi-track drag: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    t(s && s.body.highlight_net === nets[0], `the net of the track nearest the cursor is the one to highlight: ${s && s.body.highlight_net}`);
    t(ids().length === 0, `the selection is cleared while dragging (\`selectionClear\`): ${JSON.stringify(ids())}`);
    t(JSON.stringify(highlight()) === JSON.stringify([nets[0]]), `and that net is highlighted (\`highlightNets\`): ${JSON.stringify(highlight())}`);
    return s;
  };

  await scenario("multi-esc", async (t) => {
    await startBusDrag(t);
    const m = await moveTo(15_000_000, Y0);
    t(m && m.body.ok === true && (m.body.fanout ?? []).length === 2, `both lines follow: ${m && (m.body.fanout ?? []).length}`);
    const n = replies.length;
    await E.run("common.Interactive.cancel");
    t(!!(await waitReply("cancel", n)), "Esc ends the router session");
    await sleep(400);
    t(E.state().tool === "select", `the tool is Select again: ${E.state().tool}`);
    t(JSON.stringify(ids()) === JSON.stringify(busIds()), `Esc selects the two tracks again (\`restoreSelection\`): ${JSON.stringify(ids())}`);
    t(highlight().length === 0, `and the highlight is off, as it was: ${JSON.stringify(highlight())}`);
    t(snapshot(await state()) === withBus, "nothing changed");
  });

  await scenario("multi-commit", async (t) => {
    await startBusDrag(t);
    await moveTo(15_000_000, Y0);
    const n = replies.length;
    await clickAt(15_000_000, Y0);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body)}`);
    await settle();
    await sleep(300);
    const now = await state();
    const bus = tracksOf(now).filter((x) => x.net === nets[0] || x.net === nets[1]);
    t(bus.length === 2 && bus.some((x) => x.pts.some((p) => near(p[1], Y0))) && bus.some((x) => x.pts.some((p) => near(p[1], Y1))), `both tracks moved 2 mm north: ${JSON.stringify(bus.map((x) => x.pts))}`);
    const select = [...((f && f.body.select) || [])].sort();
    t(JSON.stringify(select) === JSON.stringify(bus.map((x) => x.id).sort()), `the leaders' tracks are the new ones: ${JSON.stringify(select)} of ${JSON.stringify(bus.map((x) => x.id))}`);
    t(JSON.stringify(ids()) === JSON.stringify(select), `and they are selected (\`selectItems\` of the leader segments): ${JSON.stringify(ids())}`);
    t(highlight().length === 0, `the highlight is off again: ${JSON.stringify(highlight())}`);
    await undo();
    st = await state();
    t(snapshot(st) === withBus, "one Ctrl+Z puts both back");
  });

  await scenario("single", async (t) => {
    await clearSelection();
    await clickAt(15_000_000, Y1);
    const sel = ids();
    t(sel.length === 1, `one track selected: ${JSON.stringify(sel)}`);
    await pointerTo(15_000_000, Y1);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    let s = await waitReply("drag_start", n);
    await sleep(400);
    t(s && s.body.ok === true && s.body.drag === "segment", `D on its middle slides the segment: ${s && JSON.stringify([s.body.ok, s.body.drag])}`);
    t(ids().length === 0, `nothing is selected while dragging: ${JSON.stringify(ids())}`);
    t(JSON.stringify(highlight()) === JSON.stringify([nets[0]]), `its net is highlighted: ${JSON.stringify(highlight())}`);
    await moveTo(15_000_000, Y1 - 1_000_000);
    n = replies.length;
    await E.run("common.Interactive.cancel");
    await waitReply("cancel", n);
    await sleep(400);
    t(JSON.stringify(ids()) === JSON.stringify(sel) && highlight().length === 0, `Esc selects it again, the highlight off: ${JSON.stringify([ids(), highlight()])}`);
    // and committed: a single drag has no leaders, so nothing is selected after it
    await pointerTo(15_000_000, Y1);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    s = await waitReply("drag_start", n);
    await moveTo(15_000_000, Y1 - 1_000_000);
    n = replies.length;
    await clickAt(15_000_000, Y1 - 1_000_000);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body)}`);
    await settle();
    t(ids().length === 0 && highlight().length === 0, `nothing selected, nothing highlighted: ${JSON.stringify([ids(), highlight()])}`);
    await undo();
    t(snapshot(await state()) === withBus, "one Ctrl+Z puts it back");
  });

  await scenario("locked", async (t) => {
    // A from (5, 24) to the joint J (15, 24); the locked B from J down to (15, 30)
    const [J, FA, FB] = [[15_000_000, 24_000_000], [5_000_000, 24_000_000], [15_000_000, 30_000_000]];
    madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[0], [FA, J]), line(nets[0], [J, FB])], vias: [] }));
    await settle();
    let now = await state();
    const tA = tracksOf(now).find((x) => nearPt(x.pts[0], FA) && nearPt(x.pts[x.pts.length - 1], J));
    const tB = tracksOf(now).find((x) => nearPt(x.pts[0], J) && nearPt(x.pts[x.pts.length - 1], FB));
    t(!!tA && !!tB, "the two tracks are on the board");
    madeSteps.push(await cmd({ op: "set_locked", ids: [tB.id], locked: true }));
    await settle();
    now = await state();
    const withLocked = snapshot(now);
    t(lockedIds(now).has(tB.id), "B is locked");
    await clearSelection();
    await clickAt(10_000_000, 24_000_000);
    t(JSON.stringify(ids()) === JSON.stringify([tA.id]), `A is selected: ${JSON.stringify(ids())}`);
    await pointerTo(J[0], J[1]);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === true && s.body.drag === "via" && s.body.virtual_via === true, `D on A's end at the joint drags the joint's virtual via (\`checkVirtualVia\`): ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.virtual_via, s.body.message])}`);
    const TO = [16_000_000, 22_000_000];
    const m = await moveTo(TO[0], TO[1]);
    const fan = (m && m.body.fanout) || [];
    t(fan.length === 2 && fan.some((l) => l.pts.some((p) => nearPt(p, FB))), `both lines follow the joint, the locked one too (\`findViaFanoutByHandle\`): ${JSON.stringify(fan.map((l) => l.pts))}`);
    n = replies.length;
    await clickAt(TO[0], TO[1]);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body)}`);
    await settle();
    now = await state();
    const ofNet = tracksOf(now).filter((x) => x.net === nets[0] && (x.pts.some((p) => nearPt(p, FA)) || x.pts.some((p) => nearPt(p, FB))));
    const toB = ofNet.find((x) => x.pts.some((p) => nearPt(p, FB)));
    t(!!toB && toB.id !== tB.id && toB.pts.some((p) => nearPt(p, TO)), `B's end went to the new joint: ${JSON.stringify(ofNet.map((x) => [x.id, x.pts]))}`);
    t(!!toB && !lockedIds(now).has(toB.id), "and the track that replaces it is not locked (as KiCad's: the new segment takes the line's marker, which `ClearLinks` emptied)");
    t(ofNet.some((x) => x.pts.some((p) => nearPt(p, FA)) && x.pts.some((p) => nearPt(p, TO))), `A runs from its far end to the new joint: ${JSON.stringify(ofNet.map((x) => x.pts))}`);
    await undo();
    now = await state();
    t(snapshot(now) === withLocked && lockedIds(now).has(tB.id), "one Ctrl+Z puts both back, B locked");
  });

  // leave the board as it was
  await leaveTool();
  for (let i = 0; i < 8 && snapshot(await state()) !== original; i++) await undo();
  const end = await state();
  results.push({ name: "board restored", ok: snapshot(end) === original, notes: [`${snapshot(end) === original ? "ok" : "FAIL"}: the board is as the check found it (${madeSteps.length} commands made)`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
