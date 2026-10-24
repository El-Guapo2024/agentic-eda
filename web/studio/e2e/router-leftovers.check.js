// Router leftovers check (GAPS.md item 7 and package P5d; crates/pns/PARITY.md Stage 9; CODE-COMPARE-router.md D18): what the router did not have before P5d, driven the way a person
// does it -- the router tools (`X`, `D`), synthetic pointer events on the canvas, the router actions, the dialogs' own fields and `window.__eda` -- and read from what the studio sends and
// gets back (`/api/route/*` and `/api/tune/*` replies are captured while it runs) and from `/api/state` afterwards.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board below: it commits tracks
// and undoes them, and it leaves the board exactly as it found it (every scenario undoes what it made, and the run ends by checking that). The scratch board (nothing in examples/ is touched):
//
//   eda board new examples/mcu_board_30plus.yaml -o board        # 35 x 35 mm, outline only: the parts are unplaced, so there is room for the tracks this script adds
//   eda board serve -C board --port 8837 --ui web/studio/dist
//
// The script builds what it routes on with `/api/cmd` (`commit_route`, `set_locked`, `set_tuning_profiles`): a fillet of net PA0 -- a run along y = 10 mm, a quarter arc about (12, 15) of
// radius 5 mm, a run down x = 17 mm -- and stubs of other nets to route from. 1 mm is the grid, so every point it clicks on is where it asks. What it checks:
//   settings     Interactive Router Settings has the rows "Use mouse path to set track posture" and "Optimize entire track being dragged" enabled, in KiCad's defaults (on, off); what OK keeps
//                is what the next route's `start` carries (`auto_posture`, `optimize_entire_dragged_track`);
//   arcs         D on an arc track drags an arc (`drag: "arc"`, `DM_ARC`): the arc passes through the cursor, the two runs next to it follow tangent to it, it stays an `(arc ..)` track, and one
//                Undo puts it back; a route that pushes a run of the line the arc is on leaves the arc an arc (the very arc, the run a polyline); a route that ends in the middle of an arc
//                of its net cuts it into two arcs there, and one that starts in the middle of one does too (`SplitAdjacentArcs`);
//   locked       D on a locked track from the selection tool only warns; with the router tool active it asks "The selected item is locked." and Drag Anyway starts the drag (`drag_locked`); a route in Shove mode is walked round a locked track and pushes
//                nothing;
//   via head     `V` while routing in Shove mode: the track under the head's via, on the other layer, is pushed aside with the via in the preview, and the committed route (tracks on both layers,
//                the via) leaves the gates no clearance failure and is one undo step;
//   posture      the head's elbow follows the path the cursor took (a path up the diagonal first leaves the pad diagonally, one along the row first leaves straight) -- and not with the row off; `/` flips it and back;
//   tuner on arc  a track that is an arc is picked by the tuner (the line's length along the arc), a pattern is started on it and tuned to 24 mm through the arc, which stays an arc;
//   delay        a net whose class names a tuning profile with the time domain on is tuned in picoseconds: the status box reads "current delay", the Tuning Pattern dialog has the Delay
//                radio button and a target delay, the pattern is tuned to it within 0.2 ps and stored as a time domain one.
//   corner mode  (Stage 10) the Track Corner Mode actions: arc 45 rounds the head's corner and commits it as an arc track, 90 routes axis legs,
//                Ctrl+/ steps through the modes and the running route takes the new one;
//   virtual via  (Stage 10) D at the end of a segment where three tracks meet drags the junction (its virtual via, `DRAGGER::checkVirtualVia`): every track
//                follows, no via is drawn or committed, one Undo puts it back; in the middle the segment slides;
//   springback   (Stage 10) in Shove mode a track the head pushed stays pushed, is pushed on from where it was left, and springs back once the head draws back off it; a click keeps the
//                push and the next leg pushes on from it, Backspace takes the leg and its push back, and the finished route commits the track where the session left it in one undo step;
//   via actions  (task-router-actions) on the board made 4-layer for the scenario (`set_stackup`, undone after): `/` (Switch Track Posture) flips the head, Ctrl+V (Place Microvia) arms a
//                microvia and takes it back, `<` opens the layer picker and arms a through via to the picked layer, Select Layer and Place Micro Via picks among the layers but the one
//                being routed, Q (Custom Track/Via Size) refuses a hole as big as the via in KiCad's words and its sizes go with the route, Alt+< and Alt+Shift+V arm blind vias; the
//                finished route has the microvia and the two blind vias with their types, layers and sizes, as one undo step; outside the router tool the actions do nothing;
//
// It answers through `window.__routerLeftoversCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }. Read it with `await __routerLeftoversCheck.wait(60000)` until
// `.done`. In a hidden pane the browser stops timers; the script waits with a fetch loop. A run takes four or five minutes (a debug build of the server), longer than the Browser pane's javascript
// tool gives one call: start it (`(0, eval)( src )` returns at once) and poll with `wait(30000)`.
// run-all-checks: board=mcu30 minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__routerLeftoversCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
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
    const m = /\/api\/(route|tune)\/([a-z_]+)/.exec(url);
    if (m) replies.push({ name: m[2], req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
  const lastReply = (name, from = 0) => replies.slice(from).filter((x) => x.name === name).pop();
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const key = (k, extra = {}) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
  /** The pointer on a point of the board; the studio's snap says what it read (`state().snap.input`). */
  const pointerTo = async (wx, wy) => {
    for (let i = 0; i < 4; i++) {
      const [x, y] = w2c(wx, wy);
      fire("pointermove", x, y, { buttons: 0 });
      await sleep(150);
      const sn = E.state().snap;
      if (!sn || !sn.input || Math.hypot(sn.input[0] - wx, sn.input[1] - wy) < 20) return [x, y];
    }
    return null;
  };
  const clickAt = async (wx, wy) => {
    const at = await pointerTo(wx, wy);
    if (!at) throw new Error(`the pointer could not be put on ${wx}, ${wy}`);
    fire("pointerdown", at[0], at[1], {});
    await sleep(40);
    fire("pointerup", at[0], at[1], {});
    await sleep(200);
  };
  const moveTo = async (wx, wy, name = "move") => {
    const n = replies.length;
    if (!(await pointerTo(wx, wy))) return null;
    await waitReply(name, n, 8000);
    await quiet();
    return lastReply(name, n);
  };
  const state = () => json("/api/state");
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const viasOf = (st) => st.routing?.vias ?? [];
  const arcsOf = (st) => tracksOf(st).filter((t) => t.arc_mid);
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), viasOf(st).map((v) => [v.id, v.net, v.x, v.y]), st.locked ?? [], st.board_rules?.tuning_profiles ?? null]);
  const dialog = () => document.querySelector(".dialog");
  const dialogText = () => dialog()?.textContent ?? "";
  const dialogTitle = () => dialog()?.querySelector(".dialog-header")?.textContent ?? null;
  const setVal = (el, v) => { Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };
  const press = async (label) => { const b = [...(dialog()?.querySelectorAll("button") ?? [])].find((x) => x.textContent.trim() === label); if (b) b.click(); await quiet(); await sleep(400); return !!b; };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const dockTab = (label) => [...document.querySelectorAll(".dock-tab")].find((x) => x.textContent.trim() === label);
  const lockedBox = () => [...document.querySelectorAll("label.filter-row")].find((l) => l.textContent.trim() === "Locked items")?.querySelector("input");
  /** The selection filter's "Locked items" (the Filter tab of the right dock: KiCad's own default is off, `m_filter.lockedItems = false`), then back to the tab that was open. Resolves to what the box says. */
  const lockedFilter = async (on) => {
    const active = document.querySelector(".dock-tab.active")?.textContent.trim() ?? "Appearance";
    dockTab("Filter")?.click();
    await sleep(250);
    const box = lockedBox();
    if (box && box.checked !== on) { box.click(); await sleep(200); }
    const now = lockedBox()?.checked;
    dockTab(active)?.click();
    await sleep(200);
    return now;
  };
  let original = null;
  /** Undo what the scenario made: until the board is as it was (never further: an Undo with nothing of ours left would take the scratch board's own history back). */
  const restore = async () => { for (let i = 0; i < 12 && snapshot(await state()) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(450); } await settle(); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try {
      while (dialog()) { if (!(await press("Cancel"))) break; }
      await lockedFilter(false); // the selection filter is as KiCad's default for the next scenario
      await leaveTool();
      await restore();
    } catch { /* ignore */ }
    const left = snapshot(await state()) === original;
    if (!left) { entry.notes.push("FAIL: the board is not as it was after the scenario"); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------------------------------------------- geometry
  const roundAway = (v) => (v < 0 ? -Math.round(-v) : Math.round(v));
  /** `eda_model::ir::tessellate_arc`, 32 chords: what an arc track's `pts` are. */
  const tessellate = (start, mid, end, n = 32) => {
    const [sx, sy] = start, [mx, my] = mid, [ex, ey] = end;
    const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    const ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    const uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    const r = Math.hypot(sx - ux, sy - uy), two = Math.PI * 2, norm = (a) => ((a % two) + two) % two;
    const a0 = Math.atan2(sy - uy, sx - ux), a1 = Math.atan2(my - uy, mx - ux), a2 = Math.atan2(ey - uy, ex - ux);
    let sweep = norm(a2 - a0);
    if (norm(a1 - a0) > sweep) sweep -= two;
    const pts = [];
    for (let i = 0; i <= n; i++) { const a = a0 + sweep * (i / n); pts.push([roundAway(ux + r * Math.cos(a)), roundAway(uy + r * Math.sin(a))]); }
    pts[0] = [sx, sy];
    pts[n] = [ex, ey];
    return pts;
  };
  const xy = (p) => ({ x: p[0], y: p[1] });
  const line = (net, layer, pts) => ({ net, layer, width: 200_000, pts: pts.map(xy) });
  const arcTrack = (net, layer, s, m, e) => ({ net, layer, width: 200_000, pts: tessellate(s, m, e).map(xy), arc_mid_offset: { x: m[0] - s[0], y: m[1] - s[1] } });
  /** The studio's own board follows the server's by polling its version: wait for it before the pointer is put on what was just made (a tool that looks for an item asks the studio's board). */
  const settle = async () => {
    const st = await state();
    for (let i = 0; i < 50 && !(E.state().counts.tracks === tracksOf(st).length && E.state().counts.vias === viasOf(st).length); i++) await sleep(150);
    await sleep(900);
  };
  const commit = async (tracks, vias = []) => { const r = await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks, vias }); await settle(); return r; };
  /** The fillet of PA0: a run along y = 10 mm to (12, 10), the quarter arc about (12, 15) of radius 5 mm to (17, 15), a run down x = 17 mm. */
  const C = [12_000_000, 15_000_000], R = 5_000_000;
  const S = [12_000_000, 10_000_000], M = [Math.round(C[0] + R * Math.SQRT1_2), Math.round(C[1] - R * Math.SQRT1_2)], En = [17_000_000, 15_000_000];
  const fillet = () => [line("PA0", "F.Cu", [[6_000_000, 10_000_000], S]), arcTrack("PA0", "F.Cu", S, M, En), line("PA0", "F.Cu", [En, [17_000_000, 21_000_000]])];
  const ON_ARC = [15_000_000, 11_000_000], ON_ARC2 = [16_000_000, 12_000_000]; // grid points on the arc: (3, -4) and (4, -3) mm from its centre
  const near = (a, b, tol = 1_000) => Math.abs(a - b) <= tol;
  const nearPt = (p, q, tol = 2) => Math.hypot(p[0] - q[0], p[1] - q[1]) <= tol;
  const centreDist = (p) => Math.hypot(p[0] - C[0], p[1] - C[1]);
  const distToPoly = (pts, p) => {
    let best = Infinity;
    for (let i = 0; i + 1 < pts.length; i++) {
      const [a, b] = [pts[i], pts[i + 1]], dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy;
      const u = l2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
      best = Math.min(best, Math.hypot(p[0] - (a[0] + u * dx), p[1] - (a[1] + u * dy)));
    }
    return best;
  };
  const trackAt = (st, p, net) => tracksOf(st).find((t) => (!net || t.net === net) && (nearPt(t.pts[0], p) || nearPt(t.pts[t.pts.length - 1], p)));

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  for (let i = 0; i < 40 && !(E.state().tab === "pcb" && E.state().counts.footprints > 0); i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  for (let i = 0, lastView = "", same3 = 0; i < 60 && same3 < 3; i++) {
    const r = canvas().getBoundingClientRect(), v = E.state().view, now = JSON.stringify([r.left, r.top, r.width, r.height, v.x, v.y, v.scale]);
    same3 = now === lastView ? same3 + 1 : 0;
    lastView = now;
    await sleep(250);
  }
  const errorsFrom = Date.now();
  const st0 = await state();
  original = snapshot(st0);
  if (tracksOf(st0).length > 0 || E.state().view.scale <= 0) {
    results.push({ name: "the scratch board is as the header says", ok: false, notes: [`FAIL: ${tracksOf(st0).length} tracks on it (none wanted), view scale ${E.state().view.scale}`] });
    window.fetch = realFetch;
    check.done = true;
    return;
  }
  const rules = st0.board_rules;

  await scenario("Interactive Router Settings: the two rows are live, in KiCad's defaults, and what OK keeps goes with the next route", async (t) => {
    const r = await E.run("pcbnew.InteractiveRouter.SettingsDialog");
    await sleep(400);
    t(!!dialog(), `the dialog opens: ${JSON.stringify(r)}`);
    const box = (label) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim() === label)?.querySelector("input");
    const posture = box("Use mouse path to set track posture"), optimize = box("Optimize entire track being dragged");
    t(!!posture && !posture.disabled && posture.checked === true, `"Use mouse path to set track posture" is enabled and on: ${posture && JSON.stringify([posture.disabled, posture.checked])}`);
    t(!!optimize && !optimize.disabled && optimize.checked === false, `"Optimize entire track being dragged" is enabled and off: ${optimize && JSON.stringify([optimize.disabled, optimize.checked])}`);
    t(!!box("Smooth dragged segments") && !box("Smooth dragged segments").disabled, "Smooth dragged segments is live too");
    if (posture && optimize) { posture.click(); optimize.click(); }
    await sleep(150);
    t(await press("OK"), "OK");
    t(!dialog(), "closes the dialog");
    // the next route carries them
    await commit([line("PA2", "F.Cu", [[8_000_000, 15_000_000], [8_000_000, 14_000_000]])]);
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    const n = replies.length;
    await clickAt(8_000_000, 14_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.req.settings.auto_posture === false && s.req.settings.optimize_entire_dragged_track === true, `the route starts with the new settings: ${s && JSON.stringify([s.body.ok, s.req.settings.auto_posture, s.req.settings.optimize_entire_dragged_track])}`);
    await leaveTool();
    // and put back
    await E.run("pcbnew.InteractiveRouter.SettingsDialog");
    await sleep(400);
    const again = (label) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim() === label)?.querySelector("input");
    t(again("Use mouse path to set track posture")?.checked === false && again("Optimize entire track being dragged")?.checked === true, "the dialog shows what was kept");
    again("Use mouse path to set track posture")?.click();
    again("Optimize entire track being dragged")?.click();
    await sleep(150);
    await press("OK");
    await E.run("pcbnew.InteractiveRouter.SettingsDialog");
    await sleep(400);
    t(again("Use mouse path to set track posture")?.checked === true && again("Optimize entire track being dragged")?.checked === false, "and the defaults are back");
    await press("Cancel");
  });

  await scenario("the mouse path sets the posture of the head, and with \"Use mouse path to set track posture\" off it does not", async (t) => {
    await commit([line("PA2", "F.Cu", [[8_000_000, 15_000_000], [8_000_000, 14_000_000]])]);
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
    await sleep(300);
    /** One route from the stub's end along `trail` (every point a cursor move the router sees); the head it ends with. */
    const session = async (trail) => {
      if (E.state().tool !== "route") { await E.run("pcbnew.InteractiveRouter.SingleTrack"); await sleep(300); } // Escape leaves the tool
      const n = replies.length;
      await clickAt(8_000_000, 14_000_000);
      const s = await waitReply("start", n);
      if (!(s && s.body.ok === true)) return null;
      let last = null;
      for (const [x, y] of trail) last = await moveTo(x, y);
      await E.run("common.Interactive.cancel"); // Escape ends the route
      await sleep(400);
      return last && last.body.head;
    };
    const diagonalFirst = [[9_000_000, 13_000_000], [10_000_000, 12_000_000], [11_000_000, 11_000_000], [13_000_000, 11_000_000]];
    const straightFirst = [[10_000_000, 14_000_000], [12_000_000, 14_000_000], [13_000_000, 13_000_000], [13_000_000, 11_000_000]];
    const [diag, straight] = [JSON.stringify([[8_000_000, 14_000_000], [11_000_000, 11_000_000], [13_000_000, 11_000_000]]), JSON.stringify([[8_000_000, 14_000_000], [10_000_000, 14_000_000], [13_000_000, 11_000_000]])];
    const a = await session(diagonalFirst);
    t(JSON.stringify(a) === diag, `a cursor that went up the diagonal first leaves the pad diagonally: ${JSON.stringify(a)}`);
    const b = await session(straightFirst);
    t(JSON.stringify(b) === straight, `one that went along the row first leaves straight: ${JSON.stringify(b)}`);
    // the row off: the posture is the one the geometry gives, whatever the path was
    await E.run("pcbnew.InteractiveRouter.SettingsDialog");
    await sleep(400);
    const box = () => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim() === "Use mouse path to set track posture")?.querySelector("input");
    box()?.click();
    await sleep(150);
    await press("OK");
    const c = await session(diagonalFirst);
    t(JSON.stringify(c) === straight, `with the row off the same diagonal path gives the head the geometry gives: ${JSON.stringify(c)}`);
    // and back on
    await E.run("pcbnew.InteractiveRouter.SettingsDialog");
    await sleep(400);
    box()?.click();
    await sleep(150);
    await press("OK");
    const d = await session(diagonalFirst);
    t(JSON.stringify(d) === diag, `with it on again the path counts again: ${JSON.stringify(d)}`);
    // `/` flips the posture from the head's first direction, and back (the key goes to the PCB canvas's container, the element that has the handler)
    if (E.state().tool !== "route") { await E.run("pcbnew.InteractiveRouter.SingleTrack"); await sleep(300); }
    let n = replies.length;
    await clickAt(8_000_000, 14_000_000);
    await waitReply("start", n);
    for (const [x, y] of straightFirst) await moveTo(x, y);
    const flip = async () => {
      const from = replies.length;
      document.querySelector(".pcb-canvas-container").dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true, cancelable: true }));
      const r = await waitReply("move", from, 6000);
      await quiet();
      return r;
    };
    const f1 = await flip();
    t(!!f1 && f1.req.flip_posture === true && JSON.stringify(f1.body.head) === diag, `/ turns the straight-first head diagonal-first: ${f1 && JSON.stringify([f1.req.flip_posture, f1.body.head])}`);
    const f2 = await flip();
    t(!!f2 && f2.req.flip_posture === true && JSON.stringify(f2.body.head) === straight, `and back: ${f2 && JSON.stringify(f2.body.head)}`);
  });

  await scenario("D on an arc track drags an arc: it goes through the cursor, the runs next to it follow, it stays an arc, and one Undo puts it back", async (t) => {
    await commit(fillet());
    let st = await state();
    t(arcsOf(st).length === 1 && tracksOf(st).length === 3, `the board has the fillet: ${tracksOf(st).length} tracks, ${arcsOf(st).length} arc`);
    const arcBefore = arcsOf(st)[0];
    await E.run("pcbnew.InteractiveRouter.ShoveMode");
    await pointerTo(...ON_ARC);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === true && s.body.drag === "arc", `D takes hold of the arc as an arc: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    t(s && s.body.line_tracks.length === 3, `with the two runs it is a fillet of: ${s && s.body.line_tracks.length} tracks`);
    t(E.state().tool === "drag", `the drag tool is armed: ${E.state().tool}`);
    const cursor = [16_000_000, 11_000_000]; // outside the arc, in the corner (17, 10) the two runs make
    const m = await moveTo(...cursor, "drag_move");
    t(m && m.body.ok === true && m.body.drag === "arc" && m.body.colliding === false, `the cursor drags it: ${m && JSON.stringify([m.body.ok, m.body.drag, m.body.colliding])}`);
    t(!!m && distToPoly(m.body.pts, cursor) <= 5_000, `the preview passes through the cursor: ${m && distToPoly(m.body.pts, cursor).toFixed(1)} nm off`);
    t(m && nearPt(m.body.pts[0], [6_000_000, 10_000_000], 1_000) && nearPt(m.body.pts[m.body.pts.length - 1], [17_000_000, 21_000_000], 1_000), "and the far ends of the runs stay where they were");
    const f0 = replies.length;
    await clickAt(...cursor);
    await waitReply("drag_finish", f0);
    await quiet();
    st = await state();
    const arcs = arcsOf(st);
    t(arcs.length === 1, `it is still one arc track: ${arcs.length}`);
    const arc = arcs[0];
    t(!!arc && !nearPt(arc.arc_mid, arcBefore.arc_mid, 50), `its middle moved: ${arcBefore.arc_mid} -> ${arc && arc.arc_mid}`);
    t(!!arc && distToPoly(arc.pts, cursor) <= 5_000, "and it goes through the cursor");
    const a = tracksOf(st).find((q) => q.net === "PA0" && !q.arc_mid && q.pts[0][0] === 6_000_000), b = tracksOf(st).find((q) => q.net === "PA0" && !q.arc_mid && q.pts[q.pts.length - 1][1] === 21_000_000);
    t(!!a && !!b && a.pts.every((p) => p[1] === 10_000_000) && b.pts.every((p) => p[0] === 17_000_000), `the runs are still along y = 10 mm and x = 17 mm: ${a && JSON.stringify(a.pts)} ${b && JSON.stringify(b.pts)}`);
    t(!!a && !!arc && nearPt(a.pts[a.pts.length - 1], arc.pts[0], 1_000) && !!b && nearPt(b.pts[0], arc.pts[arc.pts.length - 1], 1_000), "and meet the arc's ends: it was not cut loose");
    t(!!a && a.pts[a.pts.length - 1][0] > 12_000_000 && !!b && b.pts[0][1] < 15_000_000, `its ends slid along them, towards the corner they make at (17, 10) mm: ${a && a.pts[a.pts.length - 1]} ${b && b.pts[0]}`);
    await E.run("common.Interactive.undo");
    await sleep(500);
    t(snapshot(await state()) !== original, "one Undo leaves the fillet (the arc as it was)");
    const back = arcsOf(await state())[0];
    t(!!back && nearPt(back.arc_mid, arcBefore.arc_mid, 0) && JSON.stringify(back.pts) === JSON.stringify(arcBefore.pts), "the very arc");
  });

  await scenario("a route that pushes a run of the line an arc is on leaves the arc an arc", async (t) => {
    await commit(fillet().concat([line("PA2", "F.Cu", [[8_000_000, 15_000_000], [8_000_000, 14_000_000]])]));
    const arcBefore = arcsOf(await state())[0];
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.ShoveMode");
    await sleep(300);
    let n = replies.length;
    await clickAt(8_000_000, 14_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.net === "PA2", `the route starts on the PA2 stub: ${s && JSON.stringify([s.body.ok, s.body.net, s.body.message])}`);
    const m = await moveTo(8_000_000, 10_000_000);
    t(m && m.body.colliding === false, `the head ends on the PA0 run and Shove resolves it: ${m && m.body.colliding}`);
    const pushed = m && m.body.displaced.filter((d) => d.pts.length > 0);
    t(!!pushed && pushed.length === 1 && pushed[0].pts.length > 4, `the line of the fillet is what moves, as one polyline: ${pushed && JSON.stringify(pushed.map((d) => d.pts.length))}`);
    await clickAt(8_000_000, 10_000_000);
    await waitReply("fix", n);
    await quiet();
    await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
    await waitReply("finish", n);
    await sleep(600);
    const st = await state();
    const arcs = arcsOf(st);
    t(arcs.length === 1 && !!arcBefore && JSON.stringify(arcs[0].pts) === JSON.stringify(arcBefore.pts) && nearPt(arcs[0].arc_mid, arcBefore.arc_mid, 0), `the arc is the arc it was: ${arcs.length} arc(s), ${arcs[0] && arcs[0].arc_mid}`);
    const run = tracksOf(st).find((q) => q.net === "PA0" && !q.arc_mid && q.pts[0][0] === 6_000_000);
    t(!!run && run.pts.length > 3 && near(run.pts[run.pts.length - 1][1], 10_000_000, 0), `the run was pushed aside, a polyline of ${run && run.pts.length} points joined to the arc: ${run && JSON.stringify(run.pts)}`);
    const route = trackAt(st, [8_000_000, 10_000_000], "PA2");
    t(!!route, "the PA2 route ends where the cursor was");
    t(!(st.checks ?? []).some((c) => c.check === "routing_clearance" && c.fail), "the gates find no clearance failure");
  });

  await scenario("a route that ends in the middle of an arc of its net cuts it in two there, and one that starts in the middle of one does too", async (t) => {
    await commit(fillet().concat([line("PA0", "F.Cu", [[22_000_000, 9_000_000], [20_000_000, 9_000_000]])]));
    let st = await state();
    const arcBefore = arcsOf(st)[0];
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
    await sleep(300);
    let n = replies.length;
    await clickAt(20_000_000, 9_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.net === "PA0", `the route starts on the PA0 stub: ${s && JSON.stringify([s.body.ok, s.body.net])}`);
    const m = await moveTo(...ON_ARC);
    t(m && m.body.snapped_end && nearPt(m.body.snapped_end, ON_ARC, 2), `the end snaps onto the arc: ${m && JSON.stringify(m.body.snapped_end)}`);
    n = replies.length;
    await clickAt(...ON_ARC);
    const fx = await waitReply("fix", n);
    t(fx && fx.body.ok === true && fx.body.real_end === true, `the click reaches the end of the connection: ${fx && JSON.stringify([fx.body.ok, fx.body.real_end])}`);
    await waitReply("finish", n);
    await sleep(600);
    st = await state();
    let arcs = arcsOf(st);
    t(arcs.length === 2 && arcs.every((q) => q.net === "PA0"), `the arc is two arc tracks now: ${arcs.length}`);
    t(arcs.length === 2 && ((nearPt(arcs[0].pts[arcs[0].pts.length - 1], ON_ARC, 1_000) && nearPt(arcs[1].pts[0], ON_ARC, 1_000)) || (nearPt(arcs[1].pts[arcs[1].pts.length - 1], ON_ARC, 1_000) && nearPt(arcs[0].pts[0], ON_ARC, 1_000))), "they meet where the route ends");
    t(arcs.every((q) => q.arc_mid && near(centreDist(q.arc_mid), R, 3) && q.pts.every((p) => near(centreDist(p), R, 4))), "on the circle the arc was on");
    t(!arcs.some((q) => q.id === arcBefore.id), "the arc track it replaced is gone");
    t(!!trackAt(st, ON_ARC, "PA0") && tracksOf(st).some((q) => q.net === "PA0" && !q.arc_mid && nearPt(q.pts[0], [20_000_000, 9_000_000], 1_000) && nearPt(q.pts[q.pts.length - 1], ON_ARC, 1_000)), "and the route is a track from the stub to that point");
    await E.run("common.Interactive.undo");
    await sleep(600);
    st = await state();
    t(arcsOf(st).length === 1 && arcsOf(st)[0].id === arcBefore.id, "one Undo puts the arc back whole");
    // a route that starts on the arc (an Undo leaves the route tool: arm it again)
    await settle();
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    n = replies.length;
    await clickAt(...ON_ARC2);
    const s2 = await waitReply("start", n);
    t(s2 && s2.body.ok === true && s2.body.net === "PA0", `a click in the middle of the arc starts a route of its net: ${s2 && JSON.stringify([s2.body.ok, s2.body.net, s2.body.message])}`);
    await moveTo(20_000_000, 12_000_000);
    await clickAt(20_000_000, 12_000_000);
    await waitReply("fix", n);
    await quiet();
    await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
    await waitReply("finish", n);
    await sleep(600);
    st = await state();
    arcs = arcsOf(st);
    t(arcs.length === 2 && arcs.some((q) => nearPt(q.pts[q.pts.length - 1], ON_ARC2, 1_000)) && arcs.some((q) => nearPt(q.pts[0], ON_ARC2, 1_000)), `the arc is cut where the route starts: ${arcs.length} arc tracks`);
    t(tracksOf(st).some((q) => q.net === "PA0" && !q.arc_mid && nearPt(q.pts[0], ON_ARC2, 1_000) && nearPt(q.pts[q.pts.length - 1], [20_000_000, 12_000_000], 1_000)), "and the route leaves from there");
  });

  await scenario("a locked track: D asks, Drag Anyway drags it, and a route in Shove mode is walked round it and pushes nothing", async (t) => {
    await commit(fillet().concat([line("PA1", "F.Cu", [[24_000_000, 17_000_000], [22_000_000, 17_000_000]])]));
    let st = await state();
    const bTrack = tracksOf(st).find((q) => q.net === "PA0" && !q.arc_mid && q.pts[q.pts.length - 1][1] === 21_000_000);
    const lock = await cmd({ op: "set_locked", ids: [bTrack.id], locked: true });
    await settle();
    st = await state();
    t(lock.ok === true && (st.locked ?? []).includes(bTrack.id), `the run down x = 17 mm is locked: ${JSON.stringify(lock)}`);
    await E.run("pcbnew.InteractiveRouter.ShoveMode");
    // a route in Shove mode walks round it
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    let n = replies.length;
    await clickAt(22_000_000, 17_000_000);
    await waitReply("start", n);
    const m = await moveTo(15_000_000, 18_000_000);
    t(m && m.body.colliding === false, `the head across the locked run fits: ${m && m.body.colliding}`);
    t(m && m.body.displaced.length === 0, `and nothing is pushed: ${m && JSON.stringify(m.body.displaced.map((d) => d.source_track))}`);
    t(m && Math.max(...m.body.head.map((p) => p[1])) >= 21_000_000, `it goes round the end of the run: ${m && JSON.stringify(m.body.head)}`);
    await leaveTool();
    // D from the selection tool (`EDIT_TOOL::Drag`): the locked item is filtered out of the selection and the tool only warns
    // (`FilterCollectorForLockedItems`, `ReportFilteredLockedItems`) -- it does not ask (findings p5 F3-3)
    t((await lockedFilter(false)) === false, "the selection filter's Locked items is off, as KiCad's default is");
    await lockedFilter(true);
    await pointerTo(17_000_000, 18_000_000);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    await waitReply("drag_start", n);
    await sleep(500);
    t(!dialog() && E.state().tool === "select", `from the selection tool D does not ask and drags nothing: ${dialogText().slice(0, 80)} tool ${E.state().tool}`);
    // D while the router tool is active (`ROUTER_TOOL::performDragging`) asks
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    await pointerTo(17_000_000, 18_000_000);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === false && s.body.locked === true && /selected item is locked/.test(s.body.message), `D on the locked run is refused: ${s && JSON.stringify([s.body.ok, s.body.locked, s.body.message])}`);
    await sleep(500);
    t(!!dialog() && /The selected item is locked\./.test(dialogText()), `and asks: ${dialogText().slice(0, 80)}`);
    const buttons = [...(dialog()?.querySelectorAll("button") ?? [])].map((b) => b.textContent.trim());
    t(buttons.includes("Drag Anyway"), `with Drag Anyway among its buttons: ${JSON.stringify(buttons)}`);
    // not now
    const noWay = buttons.find((b) => /cancel|no/i.test(b));
    if (noWay) await press(noWay);
    t(!dialog() && E.state().tool !== "drag", `declining drags nothing: tool ${E.state().tool}`);
    // Drag Anyway
    n = replies.length;
    await pointerTo(17_000_000, 18_000_000);
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    await waitReply("drag_start", n);
    await sleep(500);
    await press("Drag Anyway");
    const s2 = await waitReply("drag_start", n + 1, 8000) ?? replies.slice(n).filter((x) => x.name === "drag_start")[1];
    t(s2 && s2.req.drag_locked === true && s2.body.ok === true && s2.body.drag === "segment", `Drag Anyway starts the drag with drag_locked: ${s2 && JSON.stringify([s2.req.drag_locked, s2.body.ok, s2.body.drag])}`);
    t(E.state().tool === "drag", `the drag tool is armed: ${E.state().tool}`);
    const f0 = replies.length;
    await moveTo(19_000_000, 18_000_000, "drag_move");
    await clickAt(19_000_000, 18_000_000);
    await waitReply("drag_finish", f0);
    await quiet();
    st = await state();
    const moved = tracksOf(st).find((q) => q.net === "PA0" && !q.arc_mid && q.pts.some((p) => p[0] === 19_000_000));
    t(!!moved, `the locked run moved with the cursor: ${JSON.stringify(tracksOf(st).filter((q) => q.net === "PA0" && !q.arc_mid).map((q) => q.pts))}`);
  });

  await scenario("V in Shove mode: the track under the head's via, on the other layer, is pushed aside with the via, and the route commits clean as one undo step", async (t) => {
    const clear = rules.clearance, via = { d: rules.via_diameter, drill: rules.via_drill };
    // GND runs on B.Cu 50 um inside the via's clearance ring (the via at (28, 21) mm: its radius, the clearance and half the track's width, less 50)
    const gx = 28_000_000 + via.d / 2 + clear + 100_000 - 50_000;
    await commit([line("PA3", "F.Cu", [[28_000_000, 24_000_000], [28_000_000, 22_000_000]]), line("GND", "B.Cu", [[gx, 18_000_000], [gx, 22_000_000]])]);
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.ShoveMode");
    await sleep(300);
    let n = replies.length;
    await clickAt(28_000_000, 22_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.net === "PA3", `the route starts on the PA3 stub: ${s && JSON.stringify([s.body.ok, s.body.net])}`);
    n = replies.length;
    await E.run("pcbnew.Control.layerToggle");
    const v = await waitReply("via", n);
    t(v && v.body.ok === true && v.req.enabled === true, `V arms a via: ${v && JSON.stringify(v.req)}`);
    const m = await moveTo(28_000_000, 21_000_000);
    t(m && m.body.via && m.body.via.x === 28_000_000 && m.body.via.y === 21_000_000, `the head ends in the via: ${m && JSON.stringify(m.body.via)}`);
    t(m && m.body.colliding === false, `it fits: ${m && m.body.colliding}`);
    const pushed = m && m.body.displaced.filter((d) => d.pts.length > 0);
    t(!!pushed && pushed.length === 1 && pushed[0].layer === "B.Cu" && pushed[0].pts.some((p) => p[0] > gx), `the GND track on B.Cu is pushed aside, under the via: ${pushed && JSON.stringify(pushed.map((d) => [d.layer, d.pts.length]))}`);
    n = replies.length;
    await clickAt(28_000_000, 21_000_000);
    const fx = await waitReply("fix", n);
    t(fx && fx.body.ok === true && fx.body.blocked === false && fx.body.preview.layer === "B.Cu", `a click places it and the route goes on on B.Cu: ${fx && JSON.stringify([fx.body.ok, fx.body.blocked, fx.body.preview && fx.body.preview.layer])}`);
    await moveTo(28_000_000, 19_000_000);
    n = replies.length;
    await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
    await waitReply("finish", n);
    await sleep(700);
    const st = await state();
    t(viasOf(st).length === 1 && viasOf(st)[0].x === 28_000_000 && viasOf(st)[0].y === 21_000_000 && viasOf(st)[0].net === "PA3", `the via is on the board: ${JSON.stringify(viasOf(st).map((q) => [q.net, q.x, q.y]))}`);
    t(tracksOf(st).some((q) => q.net === "PA3" && q.layer === "F.Cu" && q.pts.some((p) => p[0] === 28_000_000 && p[1] === 21_000_000)) && tracksOf(st).some((q) => q.net === "PA3" && q.layer === "B.Cu"), "the route has copper on both layers, at the via");
    const gnd = tracksOf(st).filter((q) => q.net === "GND");
    t(gnd.length >= 1 && gnd.some((q) => q.pts.length > 2 && q.pts.some((p) => p[0] > gx)), `the GND track is a polyline that goes round the via: ${JSON.stringify(gnd.map((q) => q.pts.length))}`);
    t(!(st.checks ?? []).some((c) => c.check === "routing_clearance" && c.fail), `the gates find no clearance failure: ${JSON.stringify((st.checks ?? []).filter((c) => c.fail).map((c) => c.check))}`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(snapshot(await state()) !== original && viasOf(await state()).length === 0 && tracksOf(await state()).length === 2, "one Undo takes the route, the via and the push away");
  });

  await scenario("a net whose class names a tuning profile with the time domain is tuned in picoseconds", async (t) => {
    const profile = { name: "FR4", profile_type: "single", target_impedance: 50, enable_time_domain_tuning: true, layer_entries: [{ signal_layer: "F.Cu", top_reference_layer: null, bottom_reference_layer: null, width_nm: 200_000, diff_pair_gap_nm: 0, delay_as_per_mm: 6_100_000 }], via_propagation_delay_as: 0, via_overrides: [] };
    const set = await cmd({ op: "set_tuning_profiles", settings: { profiles: [profile], class_profiles: { Default: "FR4" } } });
    t(set.ok === true, `Board Setup's Tuning Profiles take a profile of 6.1 ps/mm and the Default class names it: ${JSON.stringify(set)}`);
    await commit([line("PA1", "F.Cu", [[6_000_000, 28_000_000], [26_000_000, 28_000_000]])]);
    key("7");
    await sleep(300);
    let n = replies.length;
    await pointerTo(16_000_000, 28_040_000);
    await waitReply("hover", n, 8000);
    await quiet();
    const hv = lastReply("hover", n);
    t(hv && hv.body.hit === true && hv.body.readout && hv.body.readout.time_domain === true, `a track under the cursor reads in time: ${hv && JSON.stringify(hv.body.readout && [hv.body.readout.time_domain, hv.body.readout.delay])}`);
    t(hv && hv.body.readout.delay && hv.body.readout.delay.label === "current delay" && near(hv.body.readout.delay.current, 122, 0.05), `its delay is 122 ps (20 mm at 6.1 ps/mm): ${hv && hv.body.readout.delay && hv.body.readout.delay.current}`);
    const r = await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    t(!!dialog() && /Delay/.test(dialogText()), `Ctrl+L: the Tuning Pattern dialog has the Delay radio button: ${dialogTitle()}`);
    const radios = [...(dialog()?.querySelectorAll("input[type=radio]") ?? [])];
    t(radios.length === 2, `with Length and Delay: ${radios.length} radio buttons`);
    // the dialog does not know the net it is for: its radio button starts as the page's settings have it, and Delay is what a person chooses for this one
    if (radios[1]) radios[1].click();
    await sleep(200);
    const texts = [...(dialog()?.querySelectorAll("input[type=text]") ?? [])];
    const delayField = texts.find((q) => q.placeholder === "none" && !q.disabled);
    t(!!delayField && /ps/.test(delayField.parentElement?.textContent ?? ""), `Delay enables the target delay field (in ps): ${!!delayField}`);
    if (delayField) setVal(delayField, "150");
    await sleep(100);
    await press("OK");
    n = replies.length;
    await clickAt(8_000_000, 28_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.req.settings.time_domain === true && s.req.settings.target_delay === 150, `the first click starts a pattern with a target of 150 ps: ${s && JSON.stringify([s.body.ok, s.req.settings.time_domain, s.req.settings.target_delay])}`);
    const m = await moveTo(24_000_000, 27_600_000);
    const ro = m && m.body.readout;
    t(!!ro && ro.time_domain === true && ro.status === "tuned" && ro.delay && near(ro.delay.current, 150, 0.2), `the cursor makes a meander of 150 ps: ${ro && JSON.stringify([ro.status, ro.delay && ro.delay.current])}`);
    t(!!ro && ro.delay && near(ro.delay.min, 149.9, 0.001) && near(ro.delay.max, 150.1, 0.001), `within 0.1 ps either side: ${ro && ro.delay && JSON.stringify([ro.delay.min, ro.delay.max])}`);
    n = replies.length;
    await clickAt(24_000_000, 27_600_000);
    const f = await waitReply("fix", n, 20000);
    await quiet();
    t(f && f.body.ok === true && !!f.body.pattern_id, `the second click puts it on the board: ${f && JSON.stringify(f.body.ok)}`);
    const st = await state();
    const pat = (st.drawings?.tuning_patterns ?? []).find((p) => p.id === (f && f.body.pattern_id));
    t(!!pat && pat.is_time_domain === true && pat.target_delay && pat.target_delay.opt === 150_000_000, `the pattern is a time domain one: ${pat && JSON.stringify([pat.is_time_domain, pat.target_delay && pat.target_delay.opt])}`);
    t(!!pat && /^150(\.\d+)? ps \(tuned\)$/.test(pat.info), `its row reads ${pat && pat.info}`);
  });

  await scenario("a track that is an arc is picked by the length tuner, and a pattern is tuned along the arc", async (t) => {
    await commit(fillet());
    const lineMm = (6_000_000 + (Math.PI / 2) * R + 6_000_000) / 1e6; // the run, the quarter arc, the run
    key("7");
    await sleep(300);
    // the tool carries what the last pattern had (the time domain scenario before this one): the dialog says Length and a target of 24 mm
    await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    const radios = [...(dialog()?.querySelectorAll("input[type=radio]") ?? [])];
    if (radios[0]) radios[0].click();
    await sleep(200);
    const field = [...(dialog()?.querySelectorAll("input[type=text]") ?? [])].find((q) => q.placeholder === "none" && !q.disabled);
    if (field) setVal(field, "24");
    await sleep(100);
    await press("OK");
    let n = replies.length;
    await pointerTo(...ON_ARC);
    await waitReply("hover", n, 8000);
    await quiet();
    const hv = lastReply("hover", n);
    t(hv && hv.body.hit === true && hv.body.pick.net === "PA0", `the arc track under the cursor is picked: ${hv && JSON.stringify([hv.body.hit, hv.body.pick && hv.body.pick.net])}`);
    t(hv && hv.body.readout && near(hv.body.readout.current, lineMm, 0.05), `it reads the length of the line along the arc, ${lineMm.toFixed(3)} mm: ${hv && hv.body.readout && hv.body.readout.current}`);
    // from the middle of the arc to the run after it
    n = replies.length;
    await clickAt(...ON_ARC2);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true, `a click on the arc starts a pattern: ${s && JSON.stringify([s.body.ok, s.body.message])}`);
    const m = await moveTo(17_000_000, 19_000_000);
    const ro = m && m.body.readout;
    t(!!ro && ro.status === "tuned" && near(ro.current, 24, 0.1), `the cursor fits a meander through the arc: ${ro && JSON.stringify([ro.status, ro.current])}`);
    t(!!m && m.body.preview && m.body.preview.pieces.length > 8, `of ${m && m.body.preview && m.body.preview.pieces.length} pieces`);
    n = replies.length;
    await clickAt(17_000_000, 19_000_000);
    const f = await waitReply("fix", n, 20000);
    await quiet();
    t(f && f.body.ok === true && !!f.body.pattern_id, `the second click puts it on the board: ${f && JSON.stringify(f.body.ok)}`);
    const st = await state();
    const len = tracksOf(st).filter((q) => q.net === "PA0").reduce((sum, q) => sum + q.pts.reduce((a, p, i) => (i ? a + Math.hypot(p[0] - q.pts[i - 1][0], p[1] - q.pts[i - 1][1]) : 0), 0), 0) / 1e6;
    t(near(len, 24, 0.15), `PA0 is ${len.toFixed(3)} mm long on the board`);
    t(arcsOf(st).some((q) => nearPt(q.pts[0], S, 1_000) && distToPoly(q.pts, ON_ARC2) <= 150_000), `and the arc the pattern was started on is still the arc (a meander is made on the straight parts of a line, the arc goes through as it is): ${JSON.stringify(arcsOf(st).slice(0, 1).map((q) => [q.pts[0], q.pts[q.pts.length - 1]]))}`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(arcsOf(await state()).length === 1 && tracksOf(await state()).length === 3, "one Undo takes the pattern away and the fillet is as it was");
  });

  await scenario("springback: a pushed track stays pushed, is pushed on, springs back as the head draws back; a click keeps the push, Backspace takes it back", async (t) => {
    // a PA4 stub ending at (21, 30) mm and a GND track across its way down x = 24 mm
    await commit([line("PA4", "F.Cu", [[20_000_000, 30_000_000], [21_000_000, 30_000_000]]), line("GND", "F.Cu", [[24_000_000, 27_000_000], [24_000_000, 33_000_000]])]);
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.ShoveMode");
    await sleep(300);
    let n = replies.length;
    await clickAt(21_000_000, 30_000_000);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.net === "PA4", `the route starts on the PA4 stub: ${s && JSON.stringify([s.body.ok, s.body.net])}`);
    const pushedOf = (m) => (m ? m.body.displaced.filter((d) => d.pts.length > 0) : []);
    const reach = (m) => Math.max(24_000_000, ...pushedOf(m).flatMap((d) => d.pts.map((p) => p[0])));
    const first = await moveTo(22_000_000, 30_000_000);
    t(first && pushedOf(first).length === 0, `a short head touches nothing: ${first && JSON.stringify(first.body.displaced)}`);
    const pushed = await moveTo(25_000_000, 30_000_000);
    t(pushed && pushed.body.colliding === false && pushedOf(pushed).length === 1 && pushedOf(pushed)[0].layer === "F.Cu", `the head pushes the GND track: ${pushed && JSON.stringify(pushedOf(pushed).map((d) => d.pts.length))}`);
    const gap = 100_000 + rules.clearance + 100_000;
    t(reach(pushed) >= 25_000_000 + gap - 2, `round the head's end, a clearance off it: ${reach(pushed)}`);
    const further = await moveTo(26_000_000, 30_000_000);
    t(further && reach(further) >= 26_000_000 + gap - 2 && pushedOf(further).length === 1, `the head goes on and pushes it on from where it was: ${further && reach(further)}`);
    const back = await moveTo(22_000_000, 30_000_000);
    t(back && back.body.colliding === false && pushedOf(back).length === 0, `drawn back off it, the track springs back to where it was: ${back && JSON.stringify(back.body.displaced)}`);
    const again = await moveTo(25_000_000, 30_000_000);
    t(again && pushedOf(again).length === 1, "pushed again");
    n = replies.length;
    await clickAt(25_000_000, 30_000_000);
    const fx = await waitReply("fix", n);
    t(fx && fx.body.ok === true && fx.body.blocked === false, `a click fixes the leg: ${fx && JSON.stringify([fx.body.ok, fx.body.blocked])}`);
    // the next leg down x = 25 mm runs into the track where the first leg left it: it is pushed on from there, and what the click kept is still pushed
    let leg = null;
    for (const y of [31_000_000, 32_000_000]) leg = await moveTo(25_000_000, y);
    t(leg && leg.body.colliding === false && pushedOf(leg).length === 1 && reach(leg) >= 25_000_000 + gap - 2, `the second leg pushes the track on from the world the click left: ${leg && reach(leg)}`);
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.UndoLastSegment");
    const un = await waitReply("undo_segment", n);
    t(un && un.body.ok === true && un.body.popped === true, `Backspace takes the first leg back: ${un && JSON.stringify(un.body)}`);
    const after = await moveTo(22_000_000, 30_000_000);
    t(after && pushedOf(after).length === 0, `and its push with it: ${after && JSON.stringify(after.body.displaced)}`);
    await moveTo(25_000_000, 30_000_000);
    n = replies.length;
    await clickAt(25_000_000, 30_000_000);
    await waitReply("fix", n);
    for (const y of [31_000_000, 32_000_000]) await moveTo(25_000_000, y);
    n = replies.length;
    await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
    await waitReply("finish", n);
    await sleep(700);
    const st = await state();
    const gnd = tracksOf(st).filter((q) => q.net === "GND");
    t(gnd.length >= 1 && gnd.some((q) => q.pts.some((p) => p[0] >= 25_000_000 + gap - 2)), `the GND track is committed where the session left it: ${JSON.stringify(gnd.map((q) => q.pts))}`);
    t(tracksOf(st).some((q) => q.net === "PA4" && q.pts.some((p) => p[0] === 25_000_000 && p[1] === 32_000_000)), "the route ends where the cursor was");
    t(!(st.checks ?? []).some((c) => c.check === "routing_clearance" && c.fail), `the gates find no clearance failure: ${JSON.stringify((st.checks ?? []).filter((c) => c.fail).map((c) => c.check))}`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(tracksOf(await state()).length === 2, "one Undo takes the route and the push away");
  });

  await scenario("Track Corner Mode: arc 45 rounds the head's corner and commits it as an arc track, 90 routes axis legs, Ctrl+/ steps through the four", async (t) => {
    await commit([line("PA5", "F.Cu", [[5_000_000, 30_000_000], [6_000_000, 30_000_000]])]);
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
    await sleep(300);
    const route = async (corner, to) => {
      // the Track Corner Mode actions are the router tool's (`AS_CONTEXT`): live only while it is active
      if (E.state().tool !== "route") { await E.run("pcbnew.InteractiveRouter.SingleTrack"); await sleep(300); }
      await E.run(corner);
      await sleep(300);
      const n = replies.length;
      await clickAt(6_000_000, 30_000_000);
      const s = await waitReply("start", n);
      // along the row first, so the posture is straight-first
      for (const [x, y] of [[7_000_000, 30_000_000], [8_000_000, 30_000_000], [9_000_000, 29_000_000], to]) await moveTo(x, y);
      return { start: s, head: lastReply("move", n) };
    };
    const arc45 = await route("pcbnew.InteractiveRouter.SwitchRoundingArc45", [11_000_000, 28_000_000]);
    t(arc45.start && arc45.start.req.settings && arc45.start.req.settings.corner_mode === "rounded_45", `the route starts with the corner mode: ${arc45.start && JSON.stringify(arc45.start.req.settings && arc45.start.req.settings.corner_mode)}`);
    const pts = arc45.head && arc45.head.body.head;
    t(!!pts && pts.length > 4 && nearPt(pts[0], [6_000_000, 30_000_000], 0) && nearPt(pts[pts.length - 1], [11_000_000, 28_000_000], 0), `the head's corner is an arc (the points of its polyline): ${pts && pts.length} points`);
    let n = replies.length;
    await clickAt(11_000_000, 28_000_000);
    await waitReply("fix", n);
    n = replies.length;
    await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
    await waitReply("finish", n);
    await sleep(700);
    let st = await state();
    const arcs = arcsOf(st).filter((q) => q.net === "PA5");
    t(arcs.length === 1 && nearPt(arcs[0].pts[arcs[0].pts.length - 1], [11_000_000, 28_000_000], 1_000), `the corner is committed as an arc track: ${JSON.stringify(arcs.map((q) => [q.pts[0], q.arc_mid, q.pts[q.pts.length - 1]]))}`);
    t(tracksOf(st).some((q) => q.net === "PA5" && !q.arc_mid && nearPt(q.pts[0], [6_000_000, 30_000_000], 0)), "and the straight leg before it as a track");
    t(!(st.checks ?? []).some((c) => c.check === "routing_clearance" && c.fail), "the gates find no clearance failure");
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(tracksOf(await state()).length === 1, "one Undo takes the route away");
    await leaveTool();
    const ninety = await route("pcbnew.InteractiveRouter.SwitchRounding90", [11_000_000, 28_000_000]);
    const p90 = ninety.head && ninety.head.body.head;
    t(!!p90 && p90.length === 3 && p90.every((q, i) => i === 0 || q[0] === p90[i - 1][0] || q[1] === p90[i - 1][1]), `90 routes two axis legs: ${JSON.stringify(p90)}`);
    // Ctrl+/ (SwitchRoundingToNext) from 90 is arc 90, then 45, the default again
    const container = document.querySelector(".pcb-canvas-container");
    const ctrlSlash = async () => { container.dispatchEvent(new KeyboardEvent("keydown", { key: "/", code: "Slash", ctrlKey: true, bubbles: true, cancelable: true })); await sleep(500); await quiet(); };
    n = replies.length;
    await ctrlSlash();
    const set1 = replies.slice(n).find((r) => r.name === "settings");
    t(!!set1 && set1.req.settings.corner_mode === "rounded_90", `Ctrl+/ steps 90 to arc 90 and the running route takes it: ${set1 && JSON.stringify(set1.req.settings.corner_mode)}`);
    const again = lastReply("move", n);
    t(!!again && again.body.head.length > 3, `the head is refreshed with a rounded corner: ${again && again.body.head.length} points`);
    n = replies.length;
    await ctrlSlash();
    const set2 = replies.slice(n).find((r) => r.name === "settings");
    t(!!set2 && set2.req.settings.corner_mode === "mitered_45", `and again to 45, the default: ${set2 && JSON.stringify(set2.req.settings.corner_mode)}`);
    // `AS_CONTEXT`: outside the router tool the actions (and so their hotkeys, now in the action catalog) do nothing
    await leaveTool();
    const outside = await E.run("pcbnew.InteractiveRouter.SwitchRoundingArc90");
    t(!!outside && outside.ok === false, `not in the selection tool: ${JSON.stringify(outside)}`);
  });

  await scenario("virtual vias: D at the end of a segment where tracks of two widths meet drags the junction, every track follows, no via is drawn or committed", async (t) => {
    // a T of PA6 at (30, 10) mm whose stem is wider: no pad or via there and the width changes, so the router's world has a virtual via at it
    // (`NODE::FixupVirtualVias`; a T of one width has none -- KiCad never counts `n_seg`)
    await commit([line("PA6", "F.Cu", [[27_000_000, 10_000_000], [30_000_000, 10_000_000]]), line("PA6", "F.Cu", [[30_000_000, 10_000_000], [33_000_000, 10_000_000]]), { ...line("PA6", "F.Cu", [[30_000_000, 10_000_000], [30_000_000, 13_000_000]]), width: 300_000 }]);
    for (const mode of ["pcbnew.InteractiveRouter.WalkaroundMode", "pcbnew.InteractiveRouter.ShoveMode"]) {
      await E.run(mode);
      await pointerTo(30_000_000, 10_000_000);
      let n = replies.length;
      await E.run("pcbnew.InteractiveRouter.Drag45Degree");
      const s = await waitReply("drag_start", n);
      t(s && s.body.ok === true && s.body.drag === "via" && s.body.virtual_via === true, `${mode}: D at the junction takes hold of its virtual via: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.virtual_via, s.body.message])}`);
      const m = await moveTo(32_000_000, 12_000_000, "drag_move");
      // the cursor as the studio's snapping put it
      const to = m ? [Math.round(m.req.x), Math.round(m.req.y)] : [32_000_000, 12_000_000];
      const legs = m && m.body.fanout;
      t(!!legs && legs.length === 3 && legs.every((l) => nearPt(l.pts[0], to, 0) || nearPt(l.pts[l.pts.length - 1], to, 0)), `${mode}: the three tracks follow the cursor: ${legs && JSON.stringify(legs.map((l) => [l.pts[0], l.pts[l.pts.length - 1]]))}`);
      t(m && m.body.colliding === false, `${mode}: and fit`);
      n = replies.length;
      await clickAt(...to);
      await waitReply("drag_finish", n);
      await quiet();
      await sleep(400);
      const st = await state();
      const pa6 = tracksOf(st).filter((q) => q.net === "PA6");
      const ends = pa6.flatMap((q) => [q.pts[0], q.pts[q.pts.length - 1]]);
      t(ends.filter((p) => nearPt(p, to, 0)).length === 3 && [[27_000_000, 10_000_000], [33_000_000, 10_000_000], [30_000_000, 13_000_000]].every((p) => ends.some((e) => nearPt(e, p, 0))), `${mode}: the junction is at the cursor and the far ends where they were: ${JSON.stringify(pa6.map((q) => q.pts))}`);
      t(viasOf(st).length === 0, `${mode}: no via is committed: ${viasOf(st).length}`);
      t(!(st.checks ?? []).some((c) => c.check === "routing_clearance" && c.fail), `${mode}: the gates find no clearance failure`);
      await E.run("common.Interactive.undo");
      await sleep(600);
      t(tracksOf(await state()).filter((q) => q.net === "PA6").every((q) => q.pts.every((p) => !nearPt(p, to, 0))), `${mode}: one Undo puts the junction back`);
      await settle();
    }
    // taken hold of in the middle, a segment of the T slides as a segment
    await pointerTo(28_500_000, 10_000_000);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    const s = await waitReply("drag_start", n);
    t(s && s.body.ok === true && s.body.drag === "segment" && s.body.virtual_via === false, `the middle of a segment is a segment drag: ${s && JSON.stringify([s.body.drag, s.body.virtual_via])}`);
  });

  await scenario("the router's via actions, Switch Track Posture and Custom Track/Via Size: typed vias on the picked layers, one undo step", async (t) => {
    // four copper layers, prepreg under the outer ones (`set_stackup`; undone at the end -- the snapshot does not look at the stackup)
    const cu = (name) => ({ name, kind: "copper", thickness_mm: 0.035 });
    const di = (name, kind) => ({ name, kind, material: "FR4", thickness_mm: 0.5, epsilon_r: 4.5, loss_tangent: 0.02 });
    const stackup = { layers: [cu("F.Cu"), di("dielectric 1", "prepreg"), cu("In1.Cu"), di("dielectric 2", "core"), cu("In2.Cu"), di("dielectric 3", "prepreg"), cu("B.Cu")] };
    const set = await cmd({ op: "set_stackup", settings: { copper_layers: 4, stackup } });
    t(set && set.ok !== false, `the board is made 4-layer: ${JSON.stringify(set && (set.message ?? set.ok))}`);
    const layersNow = async () => (await state()).layers ?? [];
    for (let i = 0; i < 40 && (await layersNow()).length !== 4; i++) await sleep(250);
    t((await layersNow()).length === 4, `the board has the four layers: ${JSON.stringify(await layersNow())}`);
    await sleep(1500); // the studio's board follows by polling (the layer picker below shows what it has)
    try {
      await commit([line("PA7", "F.Cu", [[5_000_000, 5_000_000], [6_000_000, 5_000_000]])]);
      await E.run("pcbnew.InteractiveRouter.SingleTrack");
      await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
      await sleep(300);
      const host = () => document.querySelector(".pcb-canvas-container");
      const canvasKey = async (k, code, mods = {}) => { host().focus(); host().dispatchEvent(new KeyboardEvent("keydown", { key: k, code, bubbles: true, cancelable: true, ctrlKey: !!mods.ctrl, altKey: !!mods.alt, shiftKey: !!mods.shift })); await sleep(300); await quiet(); };
      let n = replies.length;
      await clickAt(6_000_000, 5_000_000);
      const s = await waitReply("start", n);
      t(s && s.body.ok === true && s.body.net === "PA7", `the route starts on the PA7 stub: ${s && JSON.stringify([s.body.ok, s.body.net])}`);
      await moveTo(7_000_000, 5_000_000);
      await moveTo(8_000_000, 6_000_000);
      // `/`: ACT_SwitchPosture -- `FlipPosture()` and a refreshing Move
      n = replies.length;
      await canvasKey("/", "Slash");
      const flip = replies.slice(n).find((r) => r.name === "move" && r.req && r.req.flip_posture === true);
      t(!!flip && flip.body.ok === true, `/ flips the posture of the head: ${flip && JSON.stringify(flip.req)}`);
      const via = async (fn) => { const k = replies.length; await fn(); return replies.slice(k).filter((r) => r.name === "via").pop() ?? null; };
      // Ctrl+V: Place Microvia, implicit target on the F.Cu / B.Cu pair -- and Ctrl+V again takes it back (`onViaCommand`: `ToggleViaPlacement()`)
      let v = await via(() => canvasKey("v", "KeyV", { ctrl: true }));
      t(v && v.req.enabled === true && v.req.via_type === "micro" && JSON.stringify(v.req.span) === JSON.stringify(["F.Cu", "B.Cu"]) && v.req.diameter === 300_000 && v.req.drill === 100_000, `Ctrl+V arms a microvia of the net class's microvia size on the pair: ${v && JSON.stringify(v.req)}`);
      v = await via(() => canvasKey("v", "KeyV", { ctrl: true }));
      t(v && v.req.enabled === false, `Ctrl+V again takes it back: ${v && JSON.stringify(v.req)}`);
      t(tracksOf(await state()).length === 1 && E.state().tool === "route", `Ctrl+V did not paste: ${tracksOf(await state()).length} track(s), tool ${E.state().tool}`);
      // `<`: Select Layer and Place Through Via -- the picker lists the copper layers, the current one too
      await canvasKey("<", "Comma", { shift: true });
      t(dialogTitle() === "Select Layer", `< opens the layer picker: ${dialogTitle()}`);
      const rows = () => [...document.querySelectorAll("[data-testid=select-layer-grid] tr")].map((r) => r.getAttribute("data-layer"));
      t(JSON.stringify(rows()) === JSON.stringify(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), `with every copper layer: ${JSON.stringify(rows())}`);
      v = await via(async () => { document.querySelector("[data-testid=select-layer-grid] tr[data-layer='In2.Cu']").click(); await sleep(300); await quiet(); });
      t(v && v.req.enabled === true && v.req.via_type === "through" && JSON.stringify(v.req.span) === JSON.stringify(["F.Cu", "B.Cu"]) && v.req.to_layer === "In2.Cu", `a click on In2.Cu arms a through via, the route to go on on In2.Cu: ${v && JSON.stringify(v.req)}`);
      t(!dialog(), "and closes the picker");
      v = await via(() => canvasKey("<", "Comma", { shift: true }));
      t(v && v.req.enabled === false && !dialog(), `< again takes the via back, no picker: ${v && JSON.stringify(v.req)}`);
      // Select Layer and Place Micro Via: every layer but the one being routed
      await E.run("pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia");
      await sleep(300);
      t(JSON.stringify(rows()) === JSON.stringify(["In1.Cu", "In2.Cu", "B.Cu"]), `the micro via's picker leaves out F.Cu: ${JSON.stringify(rows())}`);
      v = await via(async () => { document.querySelector("[data-testid=select-layer-grid] tr[data-layer='In1.Cu']").click(); await sleep(300); await quiet(); });
      t(v && v.req.via_type === "micro" && JSON.stringify(v.req.span) === JSON.stringify(["F.Cu", "In1.Cu"]) && v.req.to_layer === "In1.Cu", `a microvia F.Cu .. In1.Cu: ${v && JSON.stringify(v.req)}`);
      await moveTo(9_000_000, 5_000_000);
      n = replies.length;
      await clickAt(9_000_000, 5_000_000);
      let fx = await waitReply("fix", n);
      t(fx && fx.body.blocked === false && fx.body.preview.layer === "In1.Cu", `a click places it and the route goes on on In1.Cu: ${fx && JSON.stringify([fx.body.blocked, fx.body.preview && fx.body.preview.layer])}`);
      // Q: Custom Track/Via Size -- KiCad's refusal of a hole as big as the via, then 0.3 mm tracks and 0.7 / 0.35 mm vias
      await canvasKey("q", "KeyQ");
      t(dialogTitle() === "Track and Via Dimensions", `Q opens the dialog: ${dialogTitle()}`);
      const field = (label) => dialog()?.querySelector(`input[aria-label='${label}']`);
      setVal(field("Track width:"), "0.3");
      setVal(field("Via diameter:"), "0.7");
      setVal(field("Via hole:"), "0.7");
      await press("OK");
      t(dialogText().includes("Via hole size must be smaller than via diameter"), `a hole as big as the via is refused in KiCad's words: ${dialogText().slice(0, 160)}`);
      setVal(field("Via hole:"), "0.35");
      n = replies.length;
      await press("OK");
      t(!dialog(), "OK with a good via closes it");
      const w = replies.slice(n).find((r) => r.name === "move" && r.req && r.req.width === 300_000);
      t(!!w, `the head takes the custom width at once: ${JSON.stringify(replies.slice(n).map((r) => [r.name, r.req && r.req.width]))}`);
      // Alt+<: Select Layer and Place Blind/Buried Via, from In1.Cu: In1.Cu is not offered
      await canvasKey("<", "Comma", { alt: true, shift: true });
      t(JSON.stringify(rows()) === JSON.stringify(["F.Cu", "In2.Cu", "B.Cu"]), `Alt+< offers every layer but In1.Cu: ${JSON.stringify(rows())}`);
      v = await via(async () => { document.querySelector("[data-testid=select-layer-grid] tr[data-layer='In2.Cu']").click(); await sleep(300); await quiet(); });
      t(v && v.req.via_type === "blind" && JSON.stringify(v.req.span) === JSON.stringify(["In1.Cu", "In2.Cu"]) && v.req.diameter === 700_000 && v.req.drill === 350_000, `a blind via In1.Cu .. In2.Cu of the custom size: ${v && JSON.stringify(v.req)}`);
      await moveTo(12_000_000, 5_000_000);
      n = replies.length;
      await clickAt(12_000_000, 5_000_000);
      fx = await waitReply("fix", n);
      t(fx && fx.body.blocked === false && fx.body.preview.layer === "In2.Cu", `the route goes on on In2.Cu: ${fx && fx.body.preview && fx.body.preview.layer}`);
      // Alt+Shift+V: Place Blind/Buried Via -- In2.Cu is off the F.Cu / B.Cu pair, so the via goes to the pair's top, F.Cu
      v = await via(() => canvasKey("V", "KeyV", { alt: true, shift: true }));
      t(v && v.req.via_type === "blind" && JSON.stringify(v.req.span) === JSON.stringify(["F.Cu", "In2.Cu"]) && v.req.to_layer === "F.Cu", `Alt+Shift+V arms a blind via In2.Cu .. F.Cu: ${v && JSON.stringify(v.req)}`);
      await moveTo(15_000_000, 5_000_000);
      n = replies.length;
      await clickAt(15_000_000, 5_000_000);
      fx = await waitReply("fix", n);
      t(fx && fx.body.blocked === false && fx.body.preview.layer === "F.Cu", `the route goes on on F.Cu: ${fx && fx.body.preview && fx.body.preview.layer}`);
      await moveTo(18_000_000, 5_000_000);
      n = replies.length;
      await E.run("common.Interactive.finish"); // End (`finishInteractive`): finish at the cursor -- `F` (AttemptFinish) routes to the nearest unconnected anchor
      await waitReply("finish", n);
      await sleep(700);
      const st = await state();
      const got = viasOf(st).filter((q) => q.net === "PA7").sort((a, b) => a.x - b.x).map((q) => [q.x, q.from, q.to, q.d, q.drill, q.type ?? "through"]);
      t(JSON.stringify(got) === JSON.stringify([[9_000_000, "F.Cu", "In1.Cu", 300_000, 100_000, "micro"], [12_000_000, "In1.Cu", "In2.Cu", 700_000, 350_000, "blind"], [15_000_000, "F.Cu", "In2.Cu", 700_000, 350_000, "blind"]]), `the route commits the three vias with their types, layers and sizes: ${JSON.stringify(got)}`);
      t(["In1.Cu", "In2.Cu"].every((l) => tracksOf(st).some((q) => q.net === "PA7" && q.layer === l)), "and copper on the inner layers between them");
      await E.run("common.Interactive.undo");
      await sleep(600);
      t(viasOf(await state()).length === 0 && tracksOf(await state()).length === 1, "one Undo takes the route and its vias away");
      // `AS_CONTEXT`: outside the router tool they do nothing
      await leaveTool();
      const outside = await E.run("pcbnew.InteractiveRouter.PlaceBlindVia");
      t(!!outside && outside.ok === false, `not in the selection tool: ${JSON.stringify(outside)}`);
    } finally {
      // the sizes back to the net class's, and the board back to two layers
      await E.run("pcbnew.InteractiveRouter.SingleTrack");
      await sleep(200);
      await E.run("studio.Router.useNetclassSizes");
      await leaveTool();
      await restore();
      await E.run("common.Interactive.undo");
      for (let i = 0; i < 40 && (await layersNow()).length !== 2; i++) await sleep(250);
      t((await layersNow()).length === 2, `the board is 2-layer again: ${JSON.stringify(await layersNow())}`);
      await sleep(1500);
    }
  });

  await scenario("the board is as it was, and the studio reported no error", async (t) => {
    t(snapshot(await state()) === original, "tracks, vias, locks and tuning profiles are the same as before the run");
    const errors = E.errors(errorsFrom);
    t(errors.length === 0, `no error: ${JSON.stringify(errors.slice(0, 3))}`);
  });

  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
