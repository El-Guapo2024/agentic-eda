// The length tuner on arcs, and a tuning pattern moved and turned as a generator (UI-LEFT R2's tuner half and R3; crates/pns/PARITY.md, the tuner stage of
// task-u3-router): `8` started on an arc of a pair tunes the pair along its coupled straight with the arcs kept (`DP_MEANDER_PLACER::Start` on `ARC_T`,
// `TOPOLOGY::AssembleDiffPair` pairing arcs by centre and radius, `addCornersUntilIndex`'s `AddArc`), `9` reads the skew from an arc, a placed pattern moved on its
// own slides along its track and is tuned again (`doMoveSelection` of a lone generator), and R turns it with its tracks (`PCB_TUNING_PATTERN::Rotate`).
// Driven with the hotkeys, synthetic pointer events, the dialog's fields and `window.__eda`, read from the `/api/tune/*` replies and `/api/state`.
//
// The scratch board (nothing in examples/ is touched): an empty 34 mm board with the pair of tuner-arcs.board.mjs on it, USB_DP outside at the first corner
// (r 2 mm), USB_DN inside (r 1 mm), each corner a pair of concentric quarter arcs, both nets 17 mm + 4 pi mm = 29.566 mm long:
//
//   perl -pe 's/\bPA2\b/USB_DP/g; s/\bPA3\b/USB_DN/g' examples/mcu_board_30plus.yaml > tune_board.yaml
//   eda board new tune_board.yaml -o board
//   node web/studio/e2e/tuner-arcs.board.mjs board
//   eda board serve -C board --port 8992 --ui web/studio/dist
//
// It answers through `window.__tunerArcsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=tuner-arcs
(async () => {
  const results = [];
  const check = (window.__tunerArcsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name) }; };

  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/api\/tune\/([a-z_]+)/.exec(url);
    if (m) replies.push({ name: m[1], req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };

  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" || type === "pointermove" ? 0 : 1 }));
  };
  const key = (k, extra = {}) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
  const mm = (v) => Math.round(v * 1e6);
  const hover = async (x, y) => { const n = replies.length; fire("pointermove", ...w2c(mm(x), mm(y))); const r = await waitReply("hover", n, 6000); await quiet(); return r ? r.body : null; };
  const clickAt = async (x, y) => { const [cx, cy] = w2c(mm(x), mm(y)); fire("pointermove", cx, cy); await sleep(150); fire("pointerdown", cx, cy); await sleep(40); fire("pointerup", cx, cy); await sleep(300); };
  const moveTo = async (x, y) => { const n = replies.length; fire("pointermove", ...w2c(mm(x), mm(y))); const r = await waitReply("move", n, 8000); await quiet(); return r ? r.body : null; };
  const state = () => json("/api/state");
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const patternsOf = (st) => st.drawings?.tuning_patterns ?? [];
  const snapshot = (st) => JSON.stringify(tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]));
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  const segDist = (q, a, b) => { const [dx, dy] = [b[0] - a[0], b[1] - a[1]]; const l = dx * dx + dy * dy; const u = l ? Math.max(0, Math.min(1, ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l)) : 0; return Math.hypot(q[0] - a[0] - u * dx, q[1] - a[1] - u * dy); };
  const dialog = () => document.querySelector(".dialog");
  const setVal = (el, v) => { Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };
  const press = async (label) => { const b = [...(dialog()?.querySelectorAll("button") ?? [])].find((x) => x.textContent === label); if (b) b.click(); await quiet(); await sleep(400); };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const undoTo = async (snap) => { for (let i = 0; i < 8 && snapshot(await state()) !== snap; i++) { await E.run("common.Interactive.undo"); await sleep(500); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { if (dialog()) await press("Cancel"); await leaveTool(); } catch { /* ignore */ }
    results.push(entry);
  };
  const P = "/LED channels (D1–D8)/USB_DP", N = "/LED channels (D1–D8)/USB_DN";
  const arcs = (st) => tracksOf(st).filter((t) => t.arc_mid);

  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  await sleep(1500);
  const st0 = await state();
  const original = snapshot(st0);
  if (arcs(st0).length !== 8 || patternsOf(st0).length > 0) {
    results.push({ name: "the scratch board is as the header says", ok: false, notes: [`FAIL: ${arcs(st0).length} arcs, ${patternsOf(st0).length} patterns`] });
    window.fetch = realFetch;
    check.done = true;
    return;
  }
  const perimeter = 17 + 4 * Math.PI;
  let placed = null;

  await scenario("8 started on an arc of the pair tunes the pair along its coupled straight and keeps its eight arcs", async (t) => {
    key("8");
    await sleep(300);
    t(E.state().tool === "tune", `the tuner is armed: ${E.state().tool}`);
    // the middle of USB_DP's second arc (centre (12, 14) mm, r 2 mm)
    const h = await hover(12 - 2 * Math.SQRT1_2, 14 + 2 * Math.SQRT1_2);
    t(h && h.ok && h.hit === true && h.pick.net === P, `the arc is picked: ${h && JSON.stringify(h.pick)}`);
    t(h && h.readout && near(h.readout.current, perimeter, 0.01) && h.readout.nets && near(h.readout.nets[1], perimeter, 0.01), `the pair reads its length along the arcs: ${h && JSON.stringify(h.readout)}`);
    // the target through the Tuning Pattern dialog
    await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    const f = [...(dialog()?.querySelectorAll("input,select") ?? [])].filter((x) => x.type !== "radio");
    t(!!dialog() && f.length > 3, "Ctrl+L opens the dialog");
    if (f[0]) setVal(f[0], "34");
    await press("OK");
    const n = replies.length;
    await clickAt(11.6, 15.9);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.origin && s.body.origin.x === mm(12) && s.body.origin.y === mm(16), `the pattern starts at the arc's nearer end: ${s && JSON.stringify([s.body.ok, s.body.message, s.body.origin])}`);
    const m = await moveTo(18, 15);
    t(m && m.readout && m.readout.status === "tuned" && near(m.readout.current, 34, 0.1), `the pair is tuned to 34 mm: ${m && JSON.stringify(m.readout)}`);
    const nf = replies.length;
    await clickAt(18, 15);
    const fx = await waitReply("fix", nf);
    t(fx && fx.body.ok === true, `the second click puts it on the board: ${fx && JSON.stringify(fx.body)}`);
    await quiet();
    const st = await state();
    placed = patternsOf(st)[0] ?? null;
    t(patternsOf(st).length === 1 && placed.mode === "diff_pair", `one pair pattern: ${JSON.stringify(patternsOf(st).map((p) => [p.mode, p.info]))}`);
    const memberNets = new Set(tracksOf(st).filter((x) => placed && placed.member_ids.includes(x.id)).map((x) => x.net));
    t(memberNets.has(P) && memberNets.has(N), "its tracks are of both nets");
    t(arcs(st).filter((x) => x.id.startsWith("trk_arcpair_arc_")).length === 8, `the eight corners are still the arcs they were: ${arcs(st).length} arcs`);
  });

  await scenario("a pattern moved on its own slides along its track and is tuned again there, one undo step", async (t) => {
    t(!!placed, "a pattern from the scenario before");
    if (!placed) return;
    const before = await state();
    const member = tracksOf(before).find((x) => x.id === placed.member_ids[Math.min(2, placed.member_ids.length - 1)]);
    const grab = member.pts[Math.floor(member.pts.length / 2)];
    await clickAt(grab[0] / 1e6, grab[1] / 1e6);
    t(E.state().selection.length === 1 && E.state().selection[0].id === placed.id, `the pattern is selected: ${JSON.stringify(E.state().selection)}`);
    await E.run("pcbnew.InteractiveMove.move");
    await sleep(300);
    // carried 1.5 mm to the left and 2 mm off the track: it comes back onto the track
    const [gx, gy] = [grab[0] / 1e6, grab[1] / 1e6];
    fire("pointermove", ...w2c(mm(gx - 0.75), mm(gy - 1)));
    await sleep(200);
    const n = replies.length;
    await clickAt(gx - 1.5, gy - 2);
    const fx = await waitReply("fix", n, 15000);
    t(fx && fx.body.ok === true, `the move tunes it again (edit_start, edit point 4, fix): ${fx && JSON.stringify(fx.body.message)}`);
    const em = replies.slice(n).find((r) => r.name === "edit_move");
    t(em && em.req && em.req.handle === 4, `edit point 4, the whole pattern: ${em && JSON.stringify(em.req)}`);
    await quiet();
    const st = await state();
    const now = patternsOf(st)[0];
    // `resetToBaseline`: the start and the end come back to the nearest points of the line (here round the corner, up its arc), not 2 mm off it
    const onP = (q) => tracksOf(st).filter((x) => x.net === P).some((x) => x.pts.some((a, i) => i > 0 && segDist(q, x.pts[i - 1], a) < 2000));
    t(now && now.id === placed.id && (now.origin.x !== placed.origin.x || now.origin.y !== placed.origin.y), `it moved: ${now && JSON.stringify([now.origin, placed.origin])}`);
    t(now && onP([now.origin.x, now.origin.y]) && onP([now.end.x, now.end.y]), `its start and end are on the track: ${now && JSON.stringify([now.origin, now.end])}`);
    t(now && now.last_status === "tuned", `and is tuned there: ${now && now.info}`);
    await E.run("common.Interactive.undo");
    await sleep(800);
    const back = patternsOf(await state())[0];
    t(back && back.origin.x === placed.origin.x, "one undo puts it back");
  });

  await scenario("R turns a pattern with its tracks, its origin and its end (PCB_TUNING_PATTERN::Rotate), one undo step", async (t) => {
    if (!placed) return t(false, "a pattern from the first scenario");
    const before = await state();
    const member = tracksOf(before).find((x) => x.id === placed.member_ids[Math.min(2, placed.member_ids.length - 1)]);
    const grab = member.pts[Math.floor(member.pts.length / 2)];
    await clickAt(grab[0] / 1e6, grab[1] / 1e6);
    t(E.state().selection.length === 1 && E.state().selection[0].id === placed.id, "the pattern is selected");
    await E.run("pcbnew.InteractiveEdit.rotateCcw");
    await sleep(1000);
    const st = await state();
    const now = patternsOf(st)[0];
    t(now && now.origin.x === placed.origin.x && now.origin.y === placed.origin.y, `it turns about its own position, its origin: ${now && JSON.stringify(now.origin)}`);
    t(now && now.end.x === placed.origin.x && now.end.y !== placed.end.y, `its end turned a quarter about it: ${now && JSON.stringify(now.end)}`);
    const turned = tracksOf(st).find((x) => x.id === member.id);
    t(turned && JSON.stringify(turned.pts) !== JSON.stringify(member.pts), "its tracks turned with it");
    const others = tracksOf(st).filter((x) => x.id.startsWith("trk_arcpair_arc_"));
    t(others.length === 8 && others.every((x) => JSON.stringify(x.pts) === JSON.stringify(tracksOf(before).find((y) => y.id === x.id).pts)), "the tracks that are not the pattern's stay");
    await E.run("common.Interactive.undo");
    await sleep(800);
    t(snapshot(await state()) === snapshot(before), "one undo turns it back");
  });

  await scenario("9 reads the skew of the pair from an arc of it", async (t) => {
    key("9");
    await sleep(300);
    const h = await hover(8 + Math.SQRT1_2, 11.5 - Math.SQRT1_2);
    t(h && h.ok && h.hit === true, `an arc of USB_DN is picked by the skew tuner: ${h && JSON.stringify([h.ok, h.message, h.pick])}`);
    t(h && h.readout && h.readout.label === "current skew", `it reads the skew: ${h && JSON.stringify(h.readout)}`);
  });

  await undoTo(original);
  results.push({ name: "the board is left as it was found", ok: snapshot(await state()) === original, notes: [] });
  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
