// Router check, round 3 lane router-2 (docs/parity/UI-LEFT-3.md R3-3, R3-6, R3-10 and the lone pin's net; crates/pns/PARITY.md Stage 29): what
// `ROUTER_TOOL::InlineDrag` does with the router's mode actions, Delete, another tool's activation and the net highlight while it runs
// (pcbnew/router/router_tool.cpp at 8303b2ad), and what a route started on the pad of a pin on no net is committed on (`syncPad`: the pad's own
// `unconnected-(..)` net). Driven the way a person does it -- the pointer on the canvas, the actions, a click, Esc -- and read from the `/api/route/*`
// replies, `__eda.state()` and `/api/state`.
//
// Run it on a COPY of the mcu30 scratch board (34 x 34 mm, the parts unplaced; U1.12 and U1.13 are pins on no net): it places R1, R2 and U1 if they are
// not, adds tracks, and undoes everything it did, ending by checking the board is as it found it.
//
// What it checks:
//   mode         a multi-track drag running, Highlight collisions and back to Walk around: no toast, the drag goes on (`ChangeRouterMode` only updates the
//                message panel); Esc ends it;
//   activate     a multi-track drag, a move, then the route tool (X): the drag is cancelled as Esc cancels it (`IsActivate()`), nothing committed, the two
//                tracks selected again (`restoreSelection`), the route tool active;
//   delete       a multi-track drag, a move, then Delete: the drag ends (`EDIT_TOOL::Remove`'s `Activate()`), and the selection it gave back is deleted;
//                one Ctrl+Z puts the tracks back;
//   no-net       a drag of a net with the highlight on another net (`highlightNets( true )`: the start set left empty), then a drag of two tracks of no net:
//                its end puts back `m_startHighlightNetcodes` -- nothing, so the highlight goes off as KiCad's does;
//   lone-pad     the route tool on U1.12 (a pin on no net), a move and End: the track is on U1.12's `unconnected-(..)` net;
// It answers through `window.__routerV17Check`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__routerV17Check = { results, done: false, ok: false, wait: null });
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
  if (!partOf(before, "R1")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "R1", x: 6_000_000, y: 6_000_000 }));
  if (!partOf(before, "R2")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "R2", x: 28_000_000, y: 28_000_000 }));
  if (!partOf(before, "U1")?.placed) madeSteps.push(await cmd({ op: "place_at", part: "U1", x: 17_000_000, y: 8_000_000 }));
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);
  let st = await state();
  const nets = [...new Set(st.parts.flatMap((p) => p.pads ?? []).map((p) => p.net).filter((x) => !!x))];
  const line = (net, pts) => ({ net, layer: "F.Cu", width: 200_000, pts: pts.map(([x, y]) => ({ x, y })) });
  const [Y0, Y1, Y2, Y3, Y4] = [16_000_000, 18_000_000, 20_000_000, 24_000_000, 26_000_000];
  madeSteps.push(await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[0], [[5_000_000, Y1], [25_000_000, Y1]]), line(nets[1], [[5_000_000, Y2], [25_000_000, Y2]]), line("", [[5_000_000, Y3], [25_000_000, Y3]]), line("", [[5_000_000, Y4], [25_000_000, Y4]])], vias: [] }));
  await settle();
  st = await state();
  const withBus = snapshot(st);
  const idsOf = (pred) => tracksOf(st).filter(pred).map((x) => x.id).sort();
  const busIds = () => idsOf((x) => x.net === nets[0] || x.net === nets[1]);
  const noNetIds = () => idsOf((x) => x.net === "" && x.pts.some((p) => near(p[1], Y3) || near(p[1], Y4)));
  const startDrag = async (t, ya, yb, want) => {
    await clearSelection();
    await clickAt(15_000_000, ya);
    await clickAt(15_000_000, yb, { shift: true });
    t(JSON.stringify(ids()) === JSON.stringify(want), `the two tracks are selected: ${JSON.stringify(ids())}`);
    await pointerTo(15_000_000, ya);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    await sleep(400);
    t(s && s.body.ok === true && s.body.drag === "multi", `D on them starts a multi-track drag: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    return s;
  };

  await scenario("mode", async (t) => {
    await startDrag(t, Y1, Y2, busIds());
    await moveTo(15_000_000, Y0);
    const r = await E.run("pcbnew.InteractiveRouter.HighlightMode");
    t(!r.toast, `Highlight collisions: no toast (\`ChangeRouterMode\` updates the message panel only): ${JSON.stringify(r.toast ?? null)}`);
    const r2 = await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
    t(!r2.toast, `Walk around: no toast: ${JSON.stringify(r2.toast ?? null)}`);
    t(E.state().tool === "drag", `the drag goes on: ${E.state().tool}`);
    const m = await moveTo(15_000_000, Y0 - 500_000);
    t(m && m.body.ok === true, "and follows the pointer");
    const n = replies.length;
    await E.run("common.Interactive.cancel");
    t(!!(await waitReply("cancel", n)), "Esc ends it");
    t(snapshot(await state()) === withBus, "nothing changed");
  });

  await scenario("activate", async (t) => {
    await startDrag(t, Y1, Y2, busIds());
    await moveTo(15_000_000, Y0);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    t(!!(await waitReply("cancel", n)), "the route tool's activation ends the drag's router session");
    await sleep(500);
    t(E.state().tool === "route", `the route tool is active: ${E.state().tool}`);
    t(JSON.stringify(ids()) === JSON.stringify(busIds()), `the two tracks are selected again (\`restoreSelection\`): ${JSON.stringify(ids())}`);
    t(!replies.slice(n).some((x) => x.name === "drag_finish"), "nothing was committed");
    t(snapshot(await state()) === withBus, "the board is as it was");
  });

  await scenario("delete", async (t) => {
    await startDrag(t, Y1, Y2, busIds());
    await moveTo(15_000_000, Y0);
    const n = replies.length;
    await E.run("common.Interactive.delete");
    t(!!(await waitReply("cancel", n)), "Delete ends the drag (`EDIT_TOOL::Remove`'s `Activate()`)");
    await settle();
    const now = await state();
    const left = tracksOf(now).filter((x) => x.net === nets[0] || x.net === nets[1]);
    t(left.length === 0, `and deletes the selection it gave back: ${JSON.stringify(left.map((x) => x.id))}`);
    await undo();
    t(snapshot(await state()) === withBus, "one Ctrl+Z puts them back");
  });

  await scenario("no-net", async (t) => {
    // a drag of nets[0] with nets[1] highlighted: the highlight does not hold the dragged net, so the start set is empty
    await clearSelection();
    await pointerTo(15_000_000, Y2);
    await E.run("pcbnew.EditorControl.highlightNet");
    await sleep(300);
    t(JSON.stringify(highlight()) === JSON.stringify([nets[1]]), `nets[1] highlighted: ${JSON.stringify(highlight())}`);
    const s = await startDrag(t, Y1, Y2, busIds());
    t(s && s.body.highlight_net === nets[0], `the dragged net is shown: ${s && s.body.highlight_net}`);
    let n = replies.length;
    await E.run("common.Interactive.cancel");
    await waitReply("cancel", n);
    await sleep(400);
    t(highlight().length === 0, `the end turns it off (it did not hold the dragged net): ${JSON.stringify(highlight())}`);
    // highlight again, then a drag of two tracks of no net: no `highlightNets( true )`, and its end puts back what the last drag left -- nothing
    await clearSelection();
    await pointerTo(15_000_000, Y2);
    await E.run("pcbnew.EditorControl.highlightNet");
    await sleep(300);
    t(JSON.stringify(highlight()) === JSON.stringify([nets[1]]), `nets[1] highlighted again: ${JSON.stringify(highlight())}`);
    const s2 = await startDrag(t, Y3, Y4, noNetIds());
    t(s2 && !s2.body.highlight_net, `a drag of no net highlights nothing: ${s2 && s2.body.highlight_net}`);
    await moveTo(15_000_000, Y3 - 1_000_000);
    n = replies.length;
    await clickAt(15_000_000, Y3 - 1_000_000);
    const f = await waitReply("drag_finish", n);
    t(f && f.body.ok === true, `the click commits: ${f && JSON.stringify(f.body.message ?? f.body.ok)}`);
    await settle();
    t(highlight().length === 0, `\`highlightNets( false )\` puts back the empty start set: the highlight is off: ${JSON.stringify(highlight())}`);
    await undo();
    t(snapshot(await state()) === withBus, "one Ctrl+Z puts the tracks back");
  });

  await scenario("lone-pad", async (t) => {
    await clearSelection();
    st = await state();
    const pad = st.parts.find((p) => p.ref === "U1")?.pads?.find((q) => q.id === "U1.12" || q.number === "12");
    t(!!pad && !pad.net, `U1.12 is a pin on no net of the model: ${JSON.stringify(pad && [pad.id, pad.net])}`);
    if (!pad) return;
    const at = [pad.px ?? pad.x, pad.py ?? pad.y];
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    let n = replies.length;
    await clickAt(at[0], at[1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok !== false, `the route starts on it: ${s && JSON.stringify(s.body.message ?? s.body.ok)}`);
    const away = [at[0], at[1] + 3_000_000];
    await moveTo(away[0], away[1]);
    n = replies.length;
    await E.run("common.Interactive.finish");
    const f = await waitReply("finish", n);
    t(f && f.body.ok !== false, `End finishes it: ${f && JSON.stringify(f.body.message ?? f.body.ok)}`);
    await settle();
    const now = await state();
    const made = tracksOf(now).filter((x) => x.pts.some((p) => nearPt(p, at, 50_000)));
    t(made.length > 0 && made.every((x) => /^unconnected-\(U1-.*Pad12\)$/.test(x.net)), `the track is on U1.12's unconnected-(..) net (\`syncPad\`): ${JSON.stringify(made.map((x) => x.net))}`);
    await leaveTool();
    await undo();
    t(snapshot(await state()) === withBus, "one Ctrl+Z takes it away");
  });

  // leave the board as it was
  await leaveTool();
  for (let i = 0; i < 8 && snapshot(await state()) !== original; i++) await undo();
  const end = await state();
  results.push({ name: "board restored", ok: snapshot(end) === original, notes: [`${snapshot(end) === original ? "ok" : "FAIL"}: the board is as the check found it (${madeSteps.length} commands made)`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
