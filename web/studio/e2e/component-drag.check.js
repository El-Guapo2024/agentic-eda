// Component and multi-track drag check (docs/parity/UI-LEFT.md R1; crates/pns/PARITY.md Stage 22; GAPS.md item 7 R7-R9): `D` on a footprint drags it with its tracks
// (`ROUTER_TOOL::InlineDrag` -> `COMPONENT_DRAGGER`), and `D` on several selected tracks drags them together (`MULTI_DRAGGER`). Driven the way a person does it -- the pointer
// on the canvas, the `D` action, a click, Esc, Ctrl+Z -- and read from what the studio sends and gets back (`/api/route/*` replies are captured while it runs) and from
// `/api/state` afterwards.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board below: it
// places R1, adds tracks and undoes everything it did, and it ends by checking the board is as it found it. The scratch board (nothing in examples/ is touched):
//
//   eda board new examples/mcu_board_30plus.yaml -o board        # 34 x 34 mm, outline only: the parts are unplaced
//   eda board serve -C board --port 8840 --ui web/studio/dist
//
// What it checks:
//   component  R1 placed at (10, 10) (and R2 in a corner) with a track on each pad (PA0 from R1.1 up and west, LED1 from R1.2 down and east). The pointer on R1.1 (clear of PA0's end: `G1`) and D (nothing selected:
//              the footprint under the cursor is the hover selection) starts a component drag at R1's anchor (`drag: "component"`, `start` = R1's position, where KiCad
//              warps the cursor); a click before the pointer has moved does nothing (`hasMouseMoved`); moving the pointer 2 mm east and 3 mm south moves R1 by that
//              (`offset`) with both tracks following (`fanout`, their ends on the pads' new centres); the click commits: R1 at (12, 13), each track from its pad's new centre
//              to its old far end; one Ctrl+Z puts R1 and both tracks back;
//   esc        D, a move, Esc: the drag is cancelled (`/api/route/cancel`), the tool is Select again and nothing changed;
//   refused    R1 dragged onto a GND track: the preview collides, the click ends the drag with nothing committed and no error toast (`refused`: `InlineDrag` breaks out
//              whatever `FixRoute` says);
//   multi      two parallel tracks (y = 18 and 20 mm) selected (a click, a Shift+click), the pointer on the first and D: `drag: "multi"`, the second follows the first 2 mm north at its own
//              distance, the click commits both, one Ctrl+Z puts both back.
// It answers through `window.__componentDragCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__componentDragCheck = { results, done: false, ok: false, wait: null });
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
  /** The pointer on a point of the board; the studio's snap says what it read (`state().snap.input`) -- a tool that does not snap (Select) leaves the report as it was. */
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
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), st.parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot])]);
  const settle = async () => {
    const st = await state();
    for (let i = 0; i < 50 && !(E.state().counts.tracks === tracksOf(st).length); i++) await sleep(150);
    await sleep(900);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const clearSelection = async () => { await E.run("common.Interactive.cancel"); await sleep(200); if (E.state().selection.length) { await E.run("common.Interactive.cancel"); await sleep(200); } };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(600); await settle(); };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
  const nearPt = (p, q, tol = 2) => !!p && !!q && near(p[0], q[0], tol) && near(p[1], q[1], tol);
  const endsOf = (t) => [t.pts[0], t.pts[t.pts.length - 1]];
  const errorsSince = (n) => E.errors().slice(n);
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
  const historyStart = before.revision ?? null;
  const madeSteps = []; // the commands this check made, undone at the end
  // R1 at (10, 10): its pads at (9.175, 10) and (10.825, 10) (0603); a track on each
  const wasPlaced = !!partOf(before, "R1")?.placed;
  if (!wasPlaced) madeSteps.push(await cmd({ op: "place_at", part: "R1", x: 10_000_000, y: 10_000_000 }));
  // R2, out of the way in a corner: the nets of its pads are the bus's (the scenario multi)
  if (!partOf(before, "R2")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "R2", x: 28_000_000, y: 28_000_000 }));
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);
  let st = await state();
  const r1 = partOf(st, "R1");
  const pad1 = r1.pads.find((p) => p.num === "1"), pad2 = r1.pads.find((p) => p.num === "2");
  const P1 = [pad1.x, pad1.y], P2 = [pad2.x, pad2.y];
  const line = (net, pts) => ({ net, layer: "F.Cu", width: 200_000, pts: pts.map(([x, y]) => ({ x, y })) });
  const FAR1 = [4_000_000, 5_000_000], FAR2 = [16_000_000, 15_000_000];
  madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(pad1.net, [P1, [P1[0], 5_000_000], FAR1]), line(pad2.net, [P2, [P2[0], 15_000_000], FAR2])], vias: [] }));
  await settle();
  st = await state();
  const withR1 = snapshot(st);
  const r1Tracks = (s) => tracksOf(s).filter((t) => t.net === pad1.net || t.net === pad2.net);
  // Where D is pressed: on R1.1 but off PA0, which ends at the pad's centre. At the centre the track is under the cursor too, and `EDIT_TOOL::Drag`'s
  // selection filter (`FilterCollectorForFreePads`, then `GuessSelectionCandidates` since it holds a track) weighs R1 against it: the track's area is
  // `width * width` (`FOOTPRINT::GetCoverageArea`), far smaller, and R1 is kept only while its `CoverageRatio` -- its pads, its texts and fields grown by the
  // collector's 5 pixels, the track -- is over 0.70; on this board at this zoom it is 0.76 or so, which leaves both and asks (`doSelectionMenu`). 0.4 mm
  // below the centre, still on the pad, the track is past the collector's reach and R1 alone is under the cursor: the component drag.
  const G1 = [P1[0], P1[1] + 400_000];

  await scenario("component", async (t) => {
    await clearSelection();
    t(E.state().selection.length === 0, "nothing is selected");
    await pointerTo(G1[0], G1[1]);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === true && s.body.drag === "component", `D on R1.1 starts a component drag: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    t(s && JSON.stringify(s.req.footprints) === JSON.stringify(["R1"]), `of R1, the footprint under the cursor: ${s && JSON.stringify(s.req.footprints)}`);
    t(s && nearPt(s.body.start, [10_000_000, 10_000_000]), `at R1's anchor: ${s && JSON.stringify(s.body.start)}`);
    await sleep(300);
    t(E.state().tool === "drag", `the drag tool is active: ${E.state().tool}`);
    // a click before the pointer moves does nothing
    n = replies.length;
    const [cx, cy] = w2c(G1[0], G1[1]);
    fire("pointerdown", cx, cy, {});
    await sleep(40);
    fire("pointerup", cx, cy, {});
    await sleep(500);
    t(!replies.slice(n).some((r) => r.name === "drag_finish") && E.state().tool === "drag", "a click before any move commits nothing; the drag goes on");
    // 2 mm east, 3 mm south
    const m = await moveTo(G1[0] + 2_000_000, G1[1] + 3_000_000);
    t(m && JSON.stringify(m.body.offset) === JSON.stringify([2_000_000, 3_000_000]), `R1 follows the cursor from its anchor: offset ${m && JSON.stringify(m.body.offset)}`);
    t(m && m.body.colliding === false, `the drag is clear: ${m && m.body.colliding}`);
    const N1 = [P1[0] + 2_000_000, P1[1] + 3_000_000], N2 = [P2[0] + 2_000_000, P2[1] + 3_000_000];
    const fan = m ? m.body.fanout : [];
    t(fan.length === 2 && fan.some((l) => nearPt(l.pts[0], N1) || nearPt(l.pts[l.pts.length - 1], N1)) && fan.some((l) => nearPt(l.pts[0], N2) || nearPt(l.pts[l.pts.length - 1], N2)), `both tracks follow their pads: ${JSON.stringify(fan.map((l) => [l.pts[0], l.pts[l.pts.length - 1]]))}`);
    n = replies.length;
    await clickAt(G1[0] + 2_000_000, G1[1] + 3_000_000);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body)}`);
    await settle();
    st = await state();
    t(nearPt(partOf(st, "R1").at, [12_000_000, 13_000_000]), `R1 is at (12, 13): ${JSON.stringify(partOf(st, "R1").at)}`);
    const tr = r1Tracks(st);
    const t1 = tr.find((x) => x.net === pad1.net), t2 = tr.find((x) => x.net === pad2.net);
    t(tr.length === 2 && !!t1 && !!t2, `one track on each pad: ${JSON.stringify(tr.map((x) => [x.net, x.pts]))}`);
    t(t1 && endsOf(t1).some((e) => nearPt(e, N1)) && endsOf(t1).some((e) => nearPt(e, FAR1)), `PA0 runs from R1.1's new centre to its far end: ${t1 && JSON.stringify(t1.pts)}`);
    t(t2 && endsOf(t2).some((e) => nearPt(e, N2)) && endsOf(t2).some((e) => nearPt(e, FAR2)), `LED1 runs from R1.2's new centre to its far end: ${t2 && JSON.stringify(t2.pts)}`);
    t(t1 && t1.pts.every((p, i) => i === 0 || (() => { const dx = Math.abs(p[0] - t1.pts[i - 1][0]), dy = Math.abs(p[1] - t1.pts[i - 1][1]); return dx === 0 || dy === 0 || near(dx, dy, 2); })()), `and stays at 45 degrees: ${t1 && JSON.stringify(t1.pts)}`);
    t(E.state().tool === "select", `the tool is Select again: ${E.state().tool}`);
    await undo();
    st = await state();
    t(snapshot(st) === withR1, "one Ctrl+Z puts R1 and both tracks back");
  });

  await scenario("esc", async (t) => {
    await clearSelection();
    await pointerTo(G1[0], G1[1]);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.drag === "component", `D starts a component drag: ${s && s.body.drag}`);
    const m = await moveTo(G1[0] + 1_000_000, G1[1] - 2_000_000);
    t(m && m.body.ok === true, "a move");
    n = replies.length;
    await E.run("common.Interactive.cancel");
    const c = await waitReply("cancel", n);
    await sleep(400);
    t(!!c, "Esc ends the router session (`/api/route/cancel`)");
    t(E.state().tool === "select", `the tool is Select again: ${E.state().tool}`);
    st = await state();
    t(snapshot(st) === withR1, "nothing changed");
  });

  await scenario("refused", async (t) => {
    // a GND track R1 is dragged onto
    const gnd = st.parts.flatMap((p) => p.pads ?? []).find((p) => /GND/.test(p.net ?? ""))?.net ?? "GND";
    madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(gnd, [[5_000_000, 22_000_000], [15_000_000, 22_000_000]])], vias: [] }));
    await settle();
    const withGnd = snapshot(await state());
    await clearSelection();
    await pointerTo(G1[0], G1[1]);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    await waitReply("drag_start", n);
    const m = await moveTo(G1[0], G1[1] + 22_000_000 - P1[1]);
    t(m && m.body.colliding === true, `R1 on the GND track collides: ${m && m.body.colliding}`);
    const errs = E.errors().length;
    n = replies.length;
    await clickAt(G1[0], G1[1] + 22_000_000 - P1[1]);
    const f = await waitReply("drag_finish", n);
    await sleep(500);
    t(f && f.body.ok === false && f.body.refused === true && f.body.dragging === false, `the click is refused and the drag is over: ${f && JSON.stringify(f.body)}`);
    t(E.state().tool === "select", `the tool is Select again: ${E.state().tool}`);
    t(errorsSince(errs).length === 0, `no error toast: ${JSON.stringify(errorsSince(errs))}`);
    t(snapshot(await state()) === withGnd, "nothing was committed");
    await undo();
    t(snapshot(await state()) === withR1, "(the GND track taken away again)");
  });

  await scenario("multi", async (t) => {
    // the bus at y = 18 and 20 mm, clear of R1's tracks; dragged 2 mm north
    const [Y0, Y1, Y2] = [16_000_000, 18_000_000, 20_000_000];
    const nets = [...new Set(st.parts.flatMap((p) => p.pads ?? []).map((p) => p.net).filter((x) => x && x !== pad1.net && x !== pad2.net))];
    madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[0], [[5_000_000, Y1], [25_000_000, Y1]]), line(nets[1], [[5_000_000, Y2], [25_000_000, Y2]])], vias: [] }));
    await settle();
    const withBus = snapshot(await state());
    await clearSelection();
    await clickAt(15_000_000, Y1);
    await clickAt(15_000_000, Y2, { shift: true });
    const sel = E.state().selection.map((x) => x.id);
    t(sel.length === 2 && E.state().selection.every((x) => x.kind === "track"), `the two tracks are selected: ${JSON.stringify(E.state().selection)}`);
    await pointerTo(15_000_000, Y1);
    let n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === true && s.body.drag === "multi", `D on them starts a multi-track drag: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    t(s && JSON.stringify([...(s.req.tracks ?? [])].sort()) === JSON.stringify([...sel].sort()), `of the selected tracks: ${s && JSON.stringify(s.req.tracks)}`);
    const m = await moveTo(15_000_000, Y0);
    const fan = m ? m.body.fanout : [];
    t(fan.length === 2, `both lines are dragged: ${fan.length}`);
    t(fan.some((l) => l.pts.some((p) => near(p[1], Y0, 2))) && fan.some((l) => l.pts.some((p) => near(p[1], Y1, 2))), `the second follows the first 2 mm north at its own distance: ${JSON.stringify(fan.map((l) => l.pts))}`);
    n = replies.length;
    await clickAt(15_000_000, Y0);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body)}`);
    await settle();
    const now = await state();
    const bus = tracksOf(now).filter((x) => x.net === nets[0] || x.net === nets[1]);
    t(bus.length === 2 && bus.some((x) => x.pts.some((p) => near(p[1], Y0, 2))) && bus.some((x) => x.pts.some((p) => near(p[1], Y1, 2))), `both tracks moved: ${JSON.stringify(bus.map((x) => x.pts))}`);
    await undo();
    t(snapshot(await state()) === withBus, "one Ctrl+Z puts both back");
    await undo();
  });

  // leave the board as it was: undo what the check made (the tracks of R1, then R1's placement)
  await leaveTool();
  for (let i = 0; i < 6 && snapshot(await state()) !== original; i++) await undo();
  const end = await state();
  results.push({ name: "board restored", ok: snapshot(end) === original, notes: [`${snapshot(end) === original ? "ok" : "FAIL"}: the board is as the check found it (${madeSteps.length} commands made, from ${historyStart})`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
