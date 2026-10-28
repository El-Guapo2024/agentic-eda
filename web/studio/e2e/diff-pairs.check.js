// Differential pair check (GAPS.md item 7 and package P5b; crates/pns/PARITY.md Stage 7; CODE-COMPARE-router.md section I): the pair tool (`6`) is KiCad's pair placer, the coupled
// pair walking round and shoving what is in its way, a pair of vias, one undo step -- driven the way a person does it, with synthetic pointer events on the canvas, the router
// actions (`pcbnew.InteractiveRouter.*Mode`, `pcbnew.Control.layerToggle` for `V`, `UndoLastSegment`) and `window.__eda`, and read from what the studio sends and gets back
// (`/api/route/dp_*` replies are captured while it runs) and from `/api/state` afterwards.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board below: it
// commits routes and undoes them, and it leaves the tracks and vias exactly as it found them. The scratch board (nothing in examples/ is touched):
//
//   sed -e 's/SDA/USB_DP/' -e 's/SCL/USB_DN/' examples/through_hole_headers.yaml > usb_pair.yaml
//   eda board new usb_pair.yaml -o board
//   eda board place J3 --at 11.5,3 -C board; eda board place J2 --at 6,20 -C board; eda board place J1 --at 7.5,11 -C board
//   eda board place R1 --at 3,16 -C board;   eda board place R2 --at 17,18 -C board
//   eda board track add --net GND --layer F.Cu --width 0.2 --pts "4.4,7 4.4,12" -C board    # a free track in the pair's way (and a routing section for the gates)
//   eda board serve -C board --port 8821 --ui web/studio/dist
//
// USB_DP / USB_DN (KiCad's `/USB_DP` and `/USB_DN`: the derived schematic labels them on its root) join J3.1 and J3.2 (a pad pair 2.54 mm apart) to J2.3 and J2.4; a GND track runs 0.5 mm beside the line the pair takes. What it checks:
//   starting      a pad of a net that is not half of a pair is refused with KiCad's message; the pair starts from the N pad as from the P pad (net_a is the P net), at the board's
//                 minimum track width and the clearance between its lines, and the heads leave the two pad centres;
//   mark          Highlight collisions only reports the track in the way: the pair is straight and `colliding`;
//   walk around   the pair goes round the track together: it fits, nothing is displaced, the lines keep the pair's pitch and the clearance to the track, and come back in line;
//   shove         the pair stays straight and the track is pushed aside by what it needs (`displaced`), at the clearance from the pair, its ends where they were;
//   a route       fix at the cursor, then onto the other header's pads: the click that reaches the coupled pads finishes it; the commit is two nets of tracks that start and end on
//                 the pads, a pair apart and a clearance from the pushed track, the connections are closed, and one Undo puts everything back;
//   vias          `V` shows a pair of vias a via gap apart, a fix places them and the route goes on on the other layer from them, to the pads; the commit has both vias and tracks
//                 on both layers and one Undo takes them all away;
//   keys          Backspace takes the last fixed leg back; `/` flips the posture (the heads change, a second flip brings them back); Escape ends the session.
//   pair sizes    (r5 R5-5) a pair of Pre-defined Sizes chosen (the pair menu's entry) is what the next pair is started and routed at ("user choice"); W while it is routed steps the pair list
//                 back to the net class's entry; Use Net Class Values sends no sizes;
//   other end     Ctrl+E (Route From Other End, `ROUTER::ContinueFromEnd`) after a fixed leg commits the legs and starts a pair of the same nets, at the pad pair of the far end of the P net's
//                  connection and heading for where the head ended; the click on the end of the committed legs finishes both connections, and one Undo takes both commits away.
//
//   r7            (R7-R1, R7-R5) Ctrl+Z and Delete while a pair is drawn take back the last leg and never undo the board, E opens Track and Via Dimensions;
//                 (R7-R2) V sends the chosen via size; (R7-R3) a layer key while a pair is drawn arms a pair of vias to the layer (the pair started on pads), and
//                 V with the pair tool armed before a pair flips the active layer and starts the next pair with its vias armed.
//
// It answers through `window.__diffPairsCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }. Read it with `await __diffPairsCheck.wait(60000)` until
// `.done`. In a hidden pane the browser stops timers; the script waits with a fetch loop. A run takes one to three minutes (a debug build of the server, and a machine that is busy answers
// slowly), longer than the Browser pane's javascript tool gives one call: start it (`(0, eval)( src )` returns at once) and poll with `wait(30000)`. A click that lands elsewhere because the page
// was still settling is made again (`startAt`); the pointer is placed from the layout of that moment and checked against what the studio read (`state().snap.input`).
// run-all-checks: board=usb-pair   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__diffPairsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250); // a page that was just loaded
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name) }; };

  // ---------------------------------------------------------------------------------------------------------------------------------- what the studio asks of the router
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    // the board's own undo and redo too (`board_undo`, `board_redo`): none may be asked while a pair is drawn (R7-R1)
    const m = /\/api\/route\/([a-z_]+)/.exec(url) ?? (/\/api\/(undo|redo)$/.exec(url) ? [null, `board_${/\/api\/(undo|redo)$/.exec(url)[1]}`] : null);
    if (m) replies.push({ name: m[1], req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const key = (k, extra = {}, target = document) => target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };
  /** Puts the pointer on a point of the board and checks what the studio made of it (`state().snap.input` is the point it read from the event): a layout that shifts under the pointer -- a toast, a panel -- would otherwise put it elsewhere. */
  const pointerTo = async (wx, wy) => {
    let at = null;
    for (let i = 0; i < 5; i++) {
      const [x, y] = w2c(wx, wy);
      at = { x, y };
      fire("pointermove", x, y, { buttons: 0 });
      await sleep(150);
      const sn = E.state().snap;
      if (sn && sn.input && Math.hypot(sn.input[0] - wx, sn.input[1] - wy) < 10) return at;
    }
    return E.state().snap ? null : at; // a studio with no snap report to read is taken at its word
  };
  /** The pointer to a point of the board; resolves to the router's answer (the body of the last `dp_move` reply) to the move. */
  const moveTo = async (wx, wy) => {
    for (let attempt = 0; attempt < 3; attempt++) {
      const n = replies.length;
      if (!(await pointerTo(wx, wy))) continue;
      if (!(await waitReply("dp_move", n, 8000))) continue;
      await quiet();
      return replies.slice(n).filter((q) => q.name === "dp_move").pop().body;
    }
    return null;
  };
  /** `/`: the key goes to the canvas's container, which asks the router for the other posture at the cursor (a keystroke the page missed is pressed again, twice at most). */
  const flipPosture = async () => {
    for (let attempt = 0; attempt < 3; attempt++) {
      const n = replies.length;
      key("/", {}, canvas().parentElement);
      const r = await waitReply("dp_move", n, 4000);
      if (r) { await quiet(); return replies.slice(n).filter((q) => q.name === "dp_move").pop(); }
    }
    return null;
  };
  const clickAt = async (wx, wy) => {
    const at = await pointerTo(wx, wy);
    if (!at) throw new Error(`the pointer could not be put on ${wx}, ${wy} (the view kept shifting)`);
    fire("pointerdown", at.x, at.y, {});
    await sleep(40);
    fire("pointerup", at.x, at.y, {});
    await sleep(160);
  };
  const state = () => json("/api/state");
  const arm = async () => { if (E.state().tool !== "diffpair") await E.run("pcbnew.InteractiveRouter.DiffPair"); await sleep(100); };
  const abandon = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { key("Escape"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  /** A click that starts a session. `dp_start`'s request says where the studio put the click, and one that landed elsewhere (a page that was still settling under a loaded machine) ends the session it started and is made again; resolves to the reply to the click that landed, null if none did. */
  const startAt = async (wx, wy, tries = 3) => {
    for (let i = 0; i < tries; i++) {
      const n = replies.length;
      await clickAt(wx, wy);
      const r = await waitReply("dp_start", n);
      if (r && r.req && Math.abs(r.req.x - wx) <= 1 && Math.abs(r.req.y - wy) <= 1) return r;
      await abandon();
      await arm();
    }
    return null;
  };
  /** One Undo, but only when the route did commit: an Undo with nothing of ours to undo would take the board's own history back (the scratch board's last edit). */
  const undoOurs = async (t, what) => {
    const there = snapshot(await state()) !== original;
    t(there, `${what} is on the board`);
    if (!there) return;
    await E.run("common.Interactive.undo");
    await sleep(500);
    t(snapshot(await state()) === original, "one Undo puts every track and via back where they were");
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await abandon(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------------------------------------------- geometry
  const distPointSeg = (p, a, b) => {
    const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy;
    const u = l2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
    return Math.hypot(p[0] - (a[0] + u * dx), p[1] - (a[1] + u * dy));
  };
  const cross = (o, a, b) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const segsCross = (a, b, c, d) => { const d1 = cross(a, b, c), d2 = cross(a, b, d), d3 = cross(c, d, a), d4 = cross(c, d, b); return ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0)); };
  const distSegSeg = (a, b, c, d) => (segsCross(a, b, c, d) ? 0 : Math.min(distPointSeg(a, c, d), distPointSeg(b, c, d), distPointSeg(c, a, b), distPointSeg(d, a, b)));
  /** The least distance between two polylines (centre to centre). */
  const distLines = (p, q) => { let best = Infinity; for (let i = 0; i + 1 < p.length; i++) for (let j = 0; j + 1 < q.length; j++) best = Math.min(best, distSegSeg(p[i], p[i + 1], q[j], q[j + 1])); return best; };
  const distToLine = (pt, q) => { let best = Infinity; for (let j = 0; j + 1 < q.length; j++) best = Math.min(best, distPointSeg(pt, q[j], q[j + 1])); return best; };
  const length = (p) => p.reduce((s, q, i) => (i ? s + Math.hypot(q[0] - p[i - 1][0], q[1] - p[i - 1][1]) : 0), 0);
  /** The share of line `p`'s length that runs at `pitch` (+-10 um, 10 000 nm, KiCad's coupled-gap tolerance) from line `q`. */
  const coupledShare = (p, q, pitch) => {
    let near = 0, all = 0;
    for (let i = 0; i + 1 < p.length; i++) {
      const [a, b] = [p[i], p[i + 1]], l = Math.hypot(b[0] - a[0], b[1] - a[1]), steps = Math.max(1, Math.round(l / 25_000));
      for (let s = 0; s < steps; s++) { const f = (s + 0.5) / steps, pt = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]; all++; if (Math.abs(distToLine(pt, q) - pitch) <= 10_000) near++; }
    }
    return near / all;
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const near = (a, b, tol = 1_000) => Math.abs(a - b) <= tol;
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const viasOf = (st) => st.routing?.vias ?? [];
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), viasOf(st).map((v) => [v.net, v.x, v.y, v.from, v.to])]);

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  // the board has to be on screen: on the PCB tab, loaded, and fitted to the canvas (a pointer is placed from the view)
  for (let i = 0; i < 40 && !(E.state().tab === "pcb" && E.state().counts.footprints > 0); i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  for (let i = 0; i < 40 && !(E.state().view && E.state().view.scale > 0); i++) await sleep(250);
  // the view and the canvas have to stop moving (a click is placed from them)
  for (let i = 0, last = "", same3 = 0; i < 60 && same3 < 3; i++) {
    const r = canvas().getBoundingClientRect(), v = E.state().view, now = JSON.stringify([r.left, r.top, r.width, r.height, v.x, v.y, v.scale]);
    same3 = now === last ? same3 + 1 : 0;
    last = now;
    await sleep(250);
  }
  const errorsFrom = Date.now();
  const st0 = await state();
  const original = snapshot(st0);
  const padOf = {};
  for (const part of st0.parts) for (const q of part.pads ?? []) padOf[q.id] = [q.x, q.y];
  const wanted = ["J3.1", "J3.2", "J3.3", "J2.3", "J2.4"];
  const have = wanted.every((id) => padOf[id]);
  const obstacle = tracksOf(st0).find((t) => t.net === "GND" && t.pts.length === 2);
  if (!have || !obstacle) {
    results.push({ name: "the scratch board is as the header says", ok: false, notes: [`FAIL: pads ${wanted.map((id) => `${id} ${padOf[id] ? "ok" : "missing"}`).join(", ")}; the GND track ${obstacle ? "ok" : "missing"}`] });
    window.fetch = realFetch;
    check.done = true;
    return;
  }
  const rules = st0.board_rules;
  const clearance = rules.clearance, minWidth = rules.constraints.min_track_width_nm;
  const [p1, p2] = [padOf["J3.1"], padOf["J3.2"]];
  const mid = (p1[0] + p2[0]) / 2;
  // the pair leaves the pads straight down, comes together at its pitch, and runs along x = mid -+ pitch / 2: the cursor a little to the right of that, a long way down
  // (and 2 mm below the end of the GND track: the studio's snap pulls a click onto a track end that is closer)
  const target = [Math.round((mid + 120_000) / 1e6) * 1e6, p1[1] + 11_000_000];
  const track = obstacle.pts;
  const wall = [track[0], track[1]];
  /** What `dp_move` answers at the cursor in a mode: the mode is switched by its action, which asks for a preview at the cursor itself (so the cursor has to be where the preview is wanted). */
  const inMode = async (action) => {
    const n = replies.length;
    await E.run(action);
    const r = await waitReply("dp_move", n);
    return r ? r.body : null;
  };
  const heads = (b) => [b.head_a, b.head_b];

  await scenario("a pair starts from a pad pair, from either pad, and a net that is not half of one is refused", async (t) => {
    await arm();
    t(E.state().tool === "diffpair", "the pair tool (6) is armed");
    let r = await startAt(...padOf["J3.3"]); // D2: no partner net
    t(r && r.body.ok === false && /complementary differential pair nets/.test(r.body.message || ""), `a pad of the net D2 is refused with KiCad's message: ${r && JSON.stringify([r.req && [r.req.x, r.req.y], r.body.ok, r.body.message, r.body.net_a])}`);
    r = await startAt(...p2); // the N pad
    t(r && r.body.ok === true && r.body.net_a === "/USB_DP" && r.body.net_b === "/USB_DN", `started from the N pad, the P net is net_a: ${r && JSON.stringify([r.req, r.body.ok, r.body.message, r.body.net_a, r.body.net_b])}`);
    t(r && r.body.width >= minWidth, `the pair is not narrower than the board's minimum track width: ${r && r.body.width} >= ${minWidth}`);
    t(r && r.body.gap >= clearance, `its gap is at least the clearance between its lines: ${r && r.body.gap} >= ${clearance}`);
    await inMode("pcbnew.InteractiveRouter.HighlightMode");
    const m = await moveTo(...target);
    t(m && m.ok === true && Array.isArray(m.head_a) && m.head_a.length > 2, "a move gives the two lines of the head");
    t(m && same(m.head_a[0], p1) && same(m.head_b[0], p2), `the lines leave the pad centres: ${m && JSON.stringify([m.head_a[0], m.head_b[0]])}`);
  });

  let markHeads = null;
  let pitch = 0;
  await scenario("Highlight collisions only reports the track in the way: the pair is straight and colliding", async (t) => {
    await arm();
    await startAt(...p1);
    await moveTo(...target);
    const m = await inMode("pcbnew.InteractiveRouter.HighlightMode");
    pitch = m.width + m.gap;
    t(m.colliding === true, "the pair collides with the GND track");
    t(m.displaced.length === 0 && m.displaced_vias.length === 0, "nothing is displaced");
    t(coupledShare(m.head_a, m.head_b, pitch) > 0.6 && coupledShare(m.head_b, m.head_a, pitch) > 0.6, `the lines run at the pitch (${pitch}) over most of their length: ${coupledShare(m.head_a, m.head_b, pitch).toFixed(2)}`);
    t(distLines(m.head_a, m.head_b) >= pitch - 10_000, `and never closer: ${distLines(m.head_a, m.head_b).toFixed(1)}`);
    t(m.head_a[m.head_a.length - 1][1] === target[1] && m.head_b[m.head_b.length - 1][1] === target[1], `both lines end on the cursor's row: ${JSON.stringify([m.head_a[m.head_a.length - 1], m.head_b[m.head_b.length - 1], target])}`);
    markHeads = heads(m);
  });

  await scenario("Walk around takes the pair round the track together, at its pitch and the clearance", async (t) => {
    await arm();
    await startAt(...p1);
    await moveTo(...target);
    const m = await inMode("pcbnew.InteractiveRouter.WalkaroundMode");
    t(m && m.colliding === false, `the pair fits: ${m && JSON.stringify({ colliding: m.colliding, head_a: m.head_a })}`);
    t(m.displaced.length === 0, "the track is not pushed");
    const need = clearance + (obstacle.width + m.width) / 2 - 500; // the clearance and both half widths, centre to centre
    const dA = distLines(m.head_a, wall), dB = distLines(m.head_b, wall);
    t(dA >= need && dB >= need, `both lines keep the clearance from the track: ${dA.toFixed(1)} and ${dB.toFixed(1)} nm centre to centre (${need + 500} wanted)`);
    t(!same(heads(m), markHeads), "the lines are not the straight ones: they went round");
    t(distLines(m.head_a, m.head_b) >= pitch - 10_000, `the lines never come closer than the pair's pitch: ${distLines(m.head_a, m.head_b).toFixed(1)}`);
    t(coupledShare(m.head_a, m.head_b, pitch) > 0.6, `and most of the length is at the pitch: ${coupledShare(m.head_a, m.head_b, pitch).toFixed(2)}`);
    const [ea, eb] = [m.head_a[m.head_a.length - 1], m.head_b[m.head_b.length - 1]];
    t(near(ea[0], markHeads[0][markHeads[0].length - 1][0], 2_000) && near(eb[0], markHeads[1][markHeads[1].length - 1][0], 2_000) && ea[1] === target[1], `the lines are back in line at the cursor: ${JSON.stringify([ea, eb])}`);
  });

  await scenario("Shove keeps the pair straight and pushes the track aside to the clearance", async (t) => {
    await arm();
    await startAt(...p1);
    await moveTo(...target);
    const m = await inMode("pcbnew.InteractiveRouter.ShoveMode");
    t(m && m.colliding === false, `the pair fits: ${m && JSON.stringify({ colliding: m.colliding, head_a: m.head_a })}`);
    t(same(heads(m), markHeads), "the pair is the straight one");
    t(m.displaced.length === 1 && m.displaced[0].source_track === obstacle.id, `the GND track is what moves: ${JSON.stringify(m.displaced.map((d) => d.source_track))}`);
    const moved = m.displaced[0] && m.displaced[0].pts;
    t(!!moved && moved.length >= 2 && same([moved[0][1], moved[moved.length - 1][1]], [wall[0][1], wall[1][1]]), `its ends keep their rows: ${JSON.stringify(moved)}`);
    const dA = distLines(m.head_a, moved), dB = distLines(m.head_b, moved);
    const need = clearance + (obstacle.width + m.width) / 2 - 500;
    t(dA >= need && dB >= need, `it ends a clearance from both lines: ${dA.toFixed(1)} and ${dB.toFixed(1)} nm centre to centre (${need + 500} wanted)`);
    t(!!moved && moved[0][0] > wall[0][0], `and it went away from the pair, not towards it: x ${wall[0][0]} -> ${moved && moved[0][0]}`);
  });

  await scenario("a pair is fixed at the cursor, carried to the other header's pads, finished by the click that reaches them, and undone in one step", async (t) => {
    await arm();
    await startAt(...p1);
    await inMode("pcbnew.InteractiveRouter.ShoveMode");
    let n = replies.length;
    await clickAt(...target);
    const fix = await waitReply("dp_fix", n);
    t(fix && fix.body.ok === true && fix.body.blocked === false, `fixed at the cursor: ${fix && JSON.stringify({ ok: fix.body.ok, blocked: fix.body.blocked })}`);
    t(fix && fix.body.preview.runs_a.length === 1 && fix.body.preview.runs_b.length === 1, "one leg is fixed on each line");
    t(fix && fix.body.preview.displaced.length === 1 && fix.body.preview.displaced[0].source_track === obstacle.id, "and the pushed track stays pushed on screen: the preview still has it");
    const m = await moveTo(...padOf["J2.3"]);
    t(m && m.snapped_end === true && m.colliding === false, `the cursor on a pad of the pair's other end snaps to it: ${m && JSON.stringify({ snapped_end: m.snapped_end, colliding: m.colliding })}`);
    n = replies.length;
    await clickAt(...padOf["J2.3"]);
    const fix2 = await waitReply("dp_fix", n);
    t(fix2 && fix2.body.real_end === true, "the click on the coupled pad reaches the end of the connection");
    const fin = await waitReply("dp_finish", n);
    t(fin && fin.body.ok === true, `and finishes it: ${fin && fin.body.message}`);
    await sleep(700);
    const st = await state();
    const tracks = tracksOf(st);
    const dp = tracks.filter((x) => x.net === "/USB_DP"), dn = tracks.filter((x) => x.net === "/USB_DN");
    t(dp.length >= 1 && dn.length >= 1 && dp.every((x) => x.width === minWidth && x.layer === "F.Cu") && dn.every((x) => x.width === minWidth && x.layer === "F.Cu"), `a track or more on each net, at the pair's width: ${dp.length} and ${dn.length}`);
    const ends = (list) => { const pts = list.flatMap((x) => [x.pts[0], x.pts[x.pts.length - 1]]); return pts; };
    const hasPoint = (pts, p) => pts.some((q) => q[0] === p[0] && q[1] === p[1]);
    t(hasPoint(ends(dp), p1) && hasPoint(ends(dp), padOf["J2.3"]) && hasPoint(ends(dn), p2) && hasPoint(ends(dn), padOf["J2.4"]), "each net runs from its pad of one header to its pad of the other");
    const allDp = dp.flatMap((x) => x.pts), allDn = dn.flatMap((x) => x.pts);
    t(Math.min(...dp.flatMap((a) => dn.map((b) => distLines(a.pts, b.pts)))) >= pitch - 10_000, `the two nets are a pitch apart at the closest: ${Math.min(...dp.flatMap((a) => dn.map((b) => distLines(a.pts, b.pts)))).toFixed(1)} (${pitch})`);
    const gnds = tracks.filter((x) => x.net === "GND");
    const gnd = gnds[0];
    t(gnds.length === 1 && gnd.pts.length === 2 && gnd.pts[0][1] === wall[0][1] && gnd.pts[1][1] === wall[1][1] && gnd.pts[0][0] > wall[0][0], `the GND track was replaced by the pushed one, one track still, its ends in their rows: ${JSON.stringify(gnds.map((x) => x.pts))}`);
    t(!!gnd && Math.min(...dp.concat(dn).map((x) => distLines(x.pts, gnd.pts))) >= clearance + minWidth - 500, "and is a clearance from both nets");
    t(allDp.length > 2 && allDn.length > 2, "the lines have their corners");
    const open = (st.checks ?? []).filter((c) => c.check === "routing_connectivity" && c.fail && (c.at === "/USB_DP" || c.at === "/USB_DN"));
    t(open.length === 0, `the pair's two connections are closed: ${JSON.stringify(open.map((c) => c.at))}`);
    await undoOurs(t, "the pair");
  });

  await scenario("V places a pair of vias, the route goes on on the other layer from them, and Undo takes it all away", async (t) => {
    await arm();
    await startAt(...p1);
    await inMode("pcbnew.InteractiveRouter.WalkaroundMode");
    const viaAt = [Math.round((p1[0] - 600_000) / 1e6) * 1e6, p1[1] + 5_000_000]; // well away from the GND track: the studio's snap pulls a cursor onto track ends
    let n = replies.length;
    await E.run("pcbnew.Control.layerToggle");
    const viaCall = await waitReply("dp_via", n);
    t(viaCall && viaCall.body.ok === true && viaCall.req.enabled === true, `V asks for a pair of vias: ${viaCall && JSON.stringify(viaCall.req)}`);
    const m = await moveTo(...viaAt);
    t(m && m.placing_via === true && m.vias.length === 2, `the head ends in two vias: ${m && m.vias.length}`);
    if (m && m.vias.length === 2) {
      const [va, vb] = m.vias;
      t(va.net === "/USB_DP" && vb.net === "/USB_DN" && va.from_layer === "F.Cu" && va.to_layer === "B.Cu", `P and N, from F.Cu to B.Cu: ${JSON.stringify([va.net, vb.net, va.from_layer, va.to_layer])}`);
      const apart = Math.hypot(va.x - vb.x, va.y - vb.y) - va.diameter;
      t(apart >= clearance - 500 && apart < 2 * clearance, `a via gap apart, edge to edge: ${apart.toFixed(1)} nm (the clearance is ${clearance})`);
      t(m.colliding === false, "and they fit");
    }
    n = replies.length;
    await clickAt(...viaAt);
    const fix = await waitReply("dp_fix", n);
    t(fix && fix.body.ok === true && fix.body.blocked === false && fix.body.preview.layer === "B.Cu" && fix.body.preview.fixed_vias.length === 2, `a fix places them and the pair goes on on B.Cu: ${fix && JSON.stringify([fix.body.preview && fix.body.preview.layer, fix.body.preview && fix.body.preview.fixed_vias.length])}`);
    const m2 = await moveTo(...padOf["J2.3"]);
    t(m2 && m2.layer === "B.Cu" && m2.snapped_end === true && m2.colliding === false, `on B.Cu the pair reaches the pads of the other header: ${m2 && JSON.stringify({ layer: m2.layer, snapped_end: m2.snapped_end, colliding: m2.colliding })}`);
    n = replies.length;
    await clickAt(...padOf["J2.3"]);
    const fin = await waitReply("dp_finish", n);
    t(fin && fin.body.ok === true, `finished: ${fin && fin.body.message}`);
    await sleep(700);
    const st = await state();
    const tracks = tracksOf(st), vias = viasOf(st);
    t(vias.length === 2 && vias.some((v) => v.net === "/USB_DP") && vias.some((v) => v.net === "/USB_DN"), `both vias are on the board: ${JSON.stringify(vias.map((v) => [v.net, v.x, v.y]))}`);
    t(["/USB_DP", "/USB_DN"].every((net) => tracks.some((x) => x.net === net && x.layer === "F.Cu") && tracks.some((x) => x.net === net && x.layer === "B.Cu")), "each net has copper on both layers");
    t(vias.every((v) => tracks.some((x) => x.net === v.net && x.layer === "F.Cu" && x.pts.some((q) => q[0] === v.x && q[1] === v.y)) && tracks.some((x) => x.net === v.net && x.layer === "B.Cu" && x.pts.some((q) => q[0] === v.x && q[1] === v.y))), "a track of each layer ends at each via");
    t(!(st.checks ?? []).some((c) => c.check === "routing_connectivity" && c.fail && (c.at === "/USB_DP" || c.at === "/USB_DN")), "the two connections are closed");
    await undoOurs(t, "the pair with its vias");
  });

  await scenario("/ flips the posture and back, Backspace takes the last leg back, and Escape ends the session", async (t) => {
    await arm();
    await startAt(...p1);
    // a cursor from which the pair has two fits: along the row it leaves (through the neighbouring pad: colliding) and diagonal first (clean)
    const leg = [Math.round((p2[0] + 4_850_000) / 1e6) * 1e6, p1[1] + 3_000_000];
    await moveTo(...leg);
    const first = await inMode("pcbnew.InteractiveRouter.HighlightMode");
    t(first && first.ok === true && first.head_a.length > 2, `a pair to the cursor: ${first && first.head_a.length} corners`);
    // posture: `/` asks again at the cursor for the other of the two fits
    const flipped = await flipPosture();
    t(flipped && flipped.req.flip_posture === true && flipped.body.ok === true, `/ asks for the other posture: ${flipped && JSON.stringify(flipped.req)}`);
    t(!!flipped && !same(heads(flipped.body), heads(first)), `and the lines differ: ${flipped && JSON.stringify(flipped.body.head_a.slice(-3))} against ${first && JSON.stringify(first.head_a.slice(-3))}`);
    t(!!flipped && flipped.body.colliding !== first.colliding, `one of them collides and the other does not: ${first && first.colliding} / ${flipped && flipped.body.colliding}`);
    const again = await flipPosture();
    t(!!again && same(heads(again.body), heads(first)), "a second / brings the first back");
    // the clean posture, a leg fixed, then Backspace
    const clean = await flipPosture();
    t(!!clean && !!flipped && clean.body.colliding === false && same(heads(clean.body), heads(flipped.body)), "and a third gives the clean one again");
    let n = replies.length;
    await clickAt(...leg);
    const fix = await waitReply("dp_fix", n);
    t(fix && fix.body.ok === true && fix.body.blocked === false && fix.body.preview.runs_a.length === 1, `a click fixes the first leg: ${fix && JSON.stringify({ ok: fix.body.ok, blocked: fix.body.blocked, message: fix.body.message })}`);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.UndoLastSegment");
    const undone = await waitReply("dp_undo_segment", n);
    t(undone && undone.body.popped === true, "Backspace takes it back");
    const after = await waitReply("dp_move", n);
    t(!!after && after.body.runs_a.length === 0 && after.body.runs_b.length === 0, "and the pair is as it was before the fix");
    // Escape
    await abandon();
    t(E.state().tool === "select", "Escape leaves the tool");
    const gone = await json("/api/route/dp_move", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ x: 1, y: 1 }) });
    t(gone.ok === false && /not routing a diff pair/.test(gone.message || ""), `and ends the session: ${JSON.stringify(gone)}`);
  });

  await scenario("Route From Other End (Ctrl+E) goes on as a pair: what is fixed is committed, a pair starts at the far end of the connection heading for where the head ended, and one Undo takes both commits away", async (t) => {
    await arm();
    await startAt(...p1);
    await inMode("pcbnew.InteractiveRouter.ShoveMode");
    let n = replies.length;
    await clickAt(...target);
    const fix = await waitReply("dp_fix", n);
    t(fix && fix.body.ok === true && fix.body.blocked === false && fix.body.preview.runs_a.length === 1, `a leg is fixed on each line: ${fix && JSON.stringify([fix.body.ok, fix.body.blocked])}`);
    t(snapshot(await state()) === original, "and nothing is on the board yet");
    await moveTo(...target);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.ContinueFromEnd");
    const c = await waitReply("dp_continue_from_end", n, 20000);
    await quiet();
    t(c && c.body.ok === true && c.body.committed === true, `Ctrl+E commits what is fixed and restarts: ${c && JSON.stringify([c.body.ok, c.body.committed, c.body.message])}`);
    t(c && c.body.net_a === "/USB_DP" && c.body.net_b === "/USB_DN", `as a pair of the same two nets: ${c && JSON.stringify([c.body.net_a, c.body.net_b])}`);
    const far = c && c.body.from;
    t(!!far && Math.hypot(far[0] - padOf["J2.3"][0], far[1] - padOf["J2.3"][1]) <= 5, `at the far end of the P net's connection, the pad on the other header: ${JSON.stringify(far)} (J2.3 is at ${JSON.stringify(padOf["J2.3"])})`);
    let st = await state();
    t(tracksOf(st).some((x) => x.net === "/USB_DP") && tracksOf(st).some((x) => x.net === "/USB_DN"), "the legs that were fixed are on the board");
    t(!!c && c.body.width === minWidth && c.body.gap >= clearance, `the new pair has the width and the gap of the one it goes on from: ${c && JSON.stringify([c.body.width, c.body.gap])}`);
    t(!!c && Array.isArray(c.body.head_a) && c.body.head_a.length >= 2 && Math.hypot(c.body.head_a[c.body.head_a.length - 1][0] - target[0], c.body.head_a[c.body.head_a.length - 1][1] - target[1]) <= pitch * 2, `its head makes for where the old one ended: ${c && JSON.stringify(c.body.head_a.slice(-2))}`);
    // and it is a session of its own: the click on the end of the committed legs finishes it
    n = replies.length;
    await moveTo(...target);
    await clickAt(...target);
    const fix2 = await waitReply("dp_fix", n);
    const fin = await waitReply("dp_finish", n, 20000);
    await sleep(700);
    t(!!fix2 && fix2.body.ok === true, `a click goes on: ${fix2 && JSON.stringify([fix2.body.ok, fix2.body.blocked, fix2.body.real_end, fix2.body.message])}`);
    st = await state();
    if (!fin) { await abandon(); }
    const open = (st.checks ?? []).filter((x) => x.check === "routing_connectivity" && x.fail && (x.at === "/USB_DP" || x.at === "/USB_DN"));
    t(!!fin && fin.body.ok === true && open.length === 0, `the click on the end of the committed legs finishes the connection of both nets: finish ${fin && fin.body.ok}, ${open.length} open`);
    await undoOurs(t, "the pair continued from the other end");
  });

  await scenario("r5 R5-5: a pair of Pre-defined Sizes is routed when chosen, W steps the pair list while a pair is routed, and Use Net Class Values goes back", async (t) => {
    const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
    const before = (await state()).routing?.diff_pair_presets ?? [];
    await cmd({ op: "set_diff_pair_presets", presets: [{ width: 300_000, gap: 250_000, via_gap: 300_000 }] });
    for (let i = 0; i < 20 && !((await state()).routing?.diff_pair_presets ?? []).length; i++) await sleep(200);
    await sleep(1500); // the studio's own copy of the board follows the server's at its next poll (700 ms)
    await E.run("studio.Router.setDiffPair", 1);
    await arm();
    const r = await startAt(...p1);
    t(r && r.body.ok === true && r.req.width === 300_000 && r.req.gap === 250_000 && r.req.via_gap === 300_000, `the chosen pair's sizes are sent and routed: ${r && JSON.stringify([r.req.width, r.req.gap, r.req.via_gap, r.body.ok, r.body.width, r.body.message])}`);
    t(r && r.body.width === 300_000 && r.body.gap === 250_000 && r.body.sizes?.width_source === "user choice", `the pair is at 0.3 / 0.25 mm, "user choice": ${r && JSON.stringify([r.body.width, r.body.gap, r.body.sizes])}`);
    // W: the list has the class's entry and this one, so it goes back to the class's
    const n = replies.length;
    await E.run("pcbnew.EditorControl.trackWidthInc");
    const d = await waitReply("dp_dims", n, 8000);
    t(d && r && d.req.width === r.body.sizes?.rule_width && d.req.gap === r.body.sizes?.rule_gap, `W while routing goes to the class's sizes: ${d && JSON.stringify(d.req)} (the rule's ${r && JSON.stringify([r.body.sizes?.rule_width, r.body.sizes?.rule_gap])})`);
    await abandon();
    await E.run("studio.Router.useNetclassDiffPair");
    await arm();
    const r2 = await startAt(...p1);
    t(r2 && r2.body.ok === true && r2.req.width === undefined && r2.req.gap === undefined, `Use Net Class Values: no sizes are sent: ${r2 && JSON.stringify([r2.req.width, r2.req.gap])}`);
    await abandon();
    await cmd({ op: "set_diff_pair_presets", presets: before });
  });

  await scenario("r7: Ctrl+Z and Delete while a pair is drawn take back the last leg, E opens Track and Via Dimensions, V sends the chosen via size, a layer key arms vias, and V before a pair starts it with vias", async (t) => {
    const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
    // R7-R1: a leg fixed, then Ctrl+Z, then (fixed again) Delete
    await arm();
    await startAt(...p1);
    await inMode("pcbnew.InteractiveRouter.ShoveMode");
    for (const [what, action] of [["Ctrl+Z", "common.Interactive.undo"], ["Delete", "common.Interactive.delete"]]) {
      let n = replies.length;
      await moveTo(...target);
      await clickAt(...target);
      const fix = await waitReply("dp_fix", n);
      t(fix && fix.body.ok === true && fix.body.blocked === false, `a leg is fixed before ${what}`);
      n = replies.length;
      await E.run(action);
      const undone = await waitReply("dp_undo_segment", n, 8000);
      await quiet();
      t(undone && undone.body.popped === true, `${what} takes the leg back (UndoLastSegment)`);
      t(!replies.slice(n).some((q) => q.name === "board_undo" || q.name === "delete"), `${what} asks no undo of the board`);
      t(snapshot(await state()) === original, `and the board is as it was after ${what}`);
    }
    // R7-R5: E is ACT_CustomTrackWidth
    await E.run("pcbnew.InteractiveEdit.properties");
    await sleep(400);
    t(E.state().dialogs.some((d) => /Track and Via Dimensions/.test(d)), `E while routing opens Track and Via Dimensions: ${JSON.stringify(E.state().dialogs)}`);
    key("Escape");
    await sleep(300);
    // R7-R3: a layer key; the pair started on pads, so a pair of vias to the layer is armed
    let n = replies.length;
    await E.run("pcbnew.Control.layerBottom");
    const lay = await waitReply("layer", n, 8000);
    const armed = await waitReply("dp_via", n, 8000);
    t(lay && lay.body.ok === true && lay.body.switched === false, `the layer key asks the pair placer (DIFF_PAIR_PLACER::SetLayer) and it refuses on pads: ${lay && JSON.stringify(lay.body)}`);
    t(armed && armed.req.enabled === true && armed.req.to_layer === "B.Cu", `then a pair of vias to B.Cu is armed: ${armed && JSON.stringify(armed.req)}`);
    await abandon();
    // R7-R2: a via of the Pre-defined Sizes chosen
    const before = (await state()).routing?.via_presets ?? [];
    await cmd({ op: "set_via_presets", presets: [{ diameter: 800_000, drill: 400_000 }] });
    for (let i = 0; i < 20 && !((await state()).routing?.via_presets ?? []).length; i++) await sleep(200);
    await sleep(1500); // the studio's own copy of the board follows the server's at its next poll
    await E.run("studio.Router.setViaSize", 1);
    await arm();
    await startAt(...p1);
    n = replies.length;
    await E.run("pcbnew.Control.layerToggle");
    const v = await waitReply("dp_via", n, 8000);
    t(v && v.req.enabled === true && v.req.diameter === 800_000 && v.req.drill === 400_000, `V sends the chosen via, 0.8 / 0.4 mm: ${v && JSON.stringify(v.req)}`);
    await abandon();
    await E.run("studio.Router.setViaSize", 0);
    await cmd({ op: "set_via_presets", presets: before });
    // R7-R3: V with the pair tool armed and no pair: the active layer flips, the next pair starts with its vias armed
    await E.run("pcbnew.Control.layerTop");
    await arm();
    const layer0 = E.state().activeLayer;
    await E.run("pcbnew.InteractiveRouter.PlaceVia");
    await sleep(300);
    t(layer0 === "F.Cu" && E.state().activeLayer === "B.Cu", `V before a pair flips the active layer: ${layer0} -> ${E.state().activeLayer}`);
    n = replies.length;
    const started = await startAt(...p1);
    const sv = await waitReply("dp_via", n, 8000);
    t(started && started.req.layer === "B.Cu" && sv && sv.req.enabled === true && sv.req.to_layer === "F.Cu", `the pair starts on B.Cu with its vias armed to F.Cu: ${started && started.req.layer} ${sv && JSON.stringify(sv.req)}`);
    await abandon();
    await E.run("pcbnew.Control.layerTop");
    t(snapshot(await state()) === original, "nothing was committed");
  });

  await scenario("the board is as it was, and the studio reported no error but the refusal", async (t) => {
    t(snapshot(await state()) === original, "tracks and vias are the same as before the run");
    const errors = E.errors(errorsFrom).filter((e) => !/complementary differential pair nets/.test(e.message));
    t(errors.length === 0, `no other error: ${JSON.stringify(errors.slice(0, 3))}`);
  });

  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
