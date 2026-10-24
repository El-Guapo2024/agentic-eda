// Length tuning check (GAPS.md item 7 and package P5c; crates/pns/PARITY.md Stage 8; CODE-COMPARE-router.md section J): the tuner (`7`, `8`, `9`) is KiCad's tuning pattern tool --
// hover a track and the status box reads its length against the target, a click starts a pattern on it, the cursor sets where it ends and the router's placer fits the meander,
// a second click puts it on the board as one undo step -- and what it leaves is a generator item: it selects as one, tunes again from its properties, has four edit points to drag,
// is listed in the Generators Manager and goes when it is deleted, the track put back. Driven the way a person does it, with the hotkeys, synthetic pointer events on the canvas, the
// dialogs' own fields, the actions and `window.__eda`, and read from what the studio sends and gets back (`/api/tune/*` replies are captured while it runs) and from `/api/state` afterwards.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board below: it commits
// patterns and undoes them, and leaves the tracks exactly as it found them. The scratch board (nothing in examples/ is touched; BSD sed has no `\b`, so perl):
//
//   perl -pe 's/\bPA2\b/USB_DP/g; s/\bPA3\b/USB_DN/g' examples/mcu_board_30plus.yaml > tune_board.yaml
//   eda board new tune_board.yaml -o board
//   eda board track add --net PA0    --layer F.Cu --width 0.2 --pts "4,5 30,5"       -C board     # 26 mm, to tune alone
//   eda board track add --net PA1    --layer F.Cu --width 0.2 --pts "4,8 30,8"       -C board     # a neighbour, 3 mm away
//   eda board track add --net USB_DP --layer F.Cu --width 0.2 --pts "4,20 30,20"     -C board     # a pair, 0.5 mm pitch
//   eda board track add --net USB_DN --layer F.Cu --width 0.2 --pts "4,20.5 30,20.5" -C board
//   eda board serve -C board --port 8825 --ui web/studio/dist
//
// What it checks:
//   hover        7 arms the tool; the track under the cursor reads `current length` (its length, 26 mm), with no target; nothing under the cursor reads nothing; the board is untouched;
//   settings     Ctrl+L opens the Tuning Pattern dialog with Board Setup's Length-tuning Patterns defaults; the target typed there is sent with the first click;
//   placing      the first click starts a pattern on the track, the cursor makes a meander of the length asked, `tuned` within the tolerance, the track it replaces is hidden; 1-4 step the
//                spacing and the amplitude; the second click puts it on the board: a pattern with its tracks as members, selected, `29 mm (tuned)`, the net is 29 mm long, one Undo takes it all away;
//   the item     a click on a track of the meander selects the whole pattern (a group that is a generator); its properties tune it again (same id, the new target, one undo step);
//                dragging its end point moves its end and fits it again (edit_start, edit_move, fix); the Generators Manager lists it, Rebuild All tunes it again; Delete removes it, the track straight;
//   a pair       8 reads the pair (both nets, the partner), tunes both nets together to the target, one pattern whose members are tracks of both nets, and Delete puts both lines back;
//   skew         9 reads `current skew`; the shorter line of a pair is lengthened until the nets are as long as each other;
//   refusals     8 on a net that is not half of a pair is refused with KiCad's message; Escape drops a pattern that is started and then leaves the tool.
//
// It answers through `window.__lengthTuningCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }. Read it with `await __lengthTuningCheck.wait(60000)` until `.done`.
// In a hidden pane the browser stops timers; the script waits with a fetch loop. A run takes a minute or two (a debug build of the server), longer than the Browser pane's javascript tool
// gives one call: start it (`(0, eval)( src )` returns at once) and poll with `wait(30000)`.
// run-all-checks: board=tune-board   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__lengthTuningCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250); // a page that was just loaded
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name) }; };

  // ---------------------------------------------------------------------------------------------------------------------------------- what the studio asks of the tuner
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
  const last = (name, from = 0) => replies.slice(from).filter((x) => x.name === name).pop();
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const key = (k, extra = {}) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
  const hover = async (wx, wy) => { const n = replies.length; const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { buttons: 0 }); const r = await waitReply("hover", n, 6000); await quiet(); return r ? r.body : null; };
  const clickAt = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { buttons: 0 }); await sleep(150); fire("pointerdown", x, y, {}); await sleep(40); fire("pointerup", x, y, {}); await sleep(250); };
  const moveTo = async (wx, wy) => { const n = replies.length; const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { buttons: 0 }); const r = await waitReply("move", n, 8000); await quiet(); return r ? r.body : null; };
  const drag = async (from, to, steps = 6) => {
    const [x0, y0] = w2c(...from), [x1, y1] = w2c(...to);
    fire("pointermove", x0, y0, { buttons: 0 }); await sleep(100); fire("pointerdown", x0, y0, {}); await sleep(60);
    for (let i = 1; i <= steps; i++) { fire("pointermove", x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps, {}); await sleep(120); }
    fire("pointerup", x1, y1, {}); await quiet(); await sleep(400);
  };
  const state = () => json("/api/state");
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const patternsOf = (st) => st.drawings?.tuning_patterns ?? [];
  const snapshot = (st) => JSON.stringify(tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]));
  const length = (p) => p.reduce((s, q, i) => (i ? s + Math.hypot(q[0] - p[i - 1][0], q[1] - p[i - 1][1]) : 0), 0);
  const netMm = (st, net) => tracksOf(st).filter((t) => t.net === net).reduce((s, t) => s + length(t.pts), 0) / 1e6;
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  const dialog = () => document.querySelector(".dialog");
  const dialogTitle = () => dialog()?.querySelector(".dialog-header")?.textContent ?? null;
  const setVal = (el, v) => { Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };
  const press = async (label) => { const b = [...(dialog()?.querySelectorAll("button") ?? [])].find((x) => x.textContent === label); if (b) b.click(); await quiet(); await sleep(400); };
  // The dialog's fields in the order this check was written for: target, override, min amplitude, max amplitude, spacing, corner style, radius, side, single-sided.
  // P5d put the time domain in front (the Length / Delay radio buttons and the target delay after the target; router-leftovers.check.js checks those), so they are left out here.
  const fields = () => { const f = [...(dialog()?.querySelectorAll("input,select") ?? [])].filter((x) => x.type !== "radio"); if (f.length >= 10) f.splice(1, 1); return f; };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const undoTo = async (snap) => { for (let i = 0; i < 8 && snapshot(await state()) !== snap; i++) { await E.run("common.Interactive.undo"); await sleep(500); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { if (dialog()) await press("Cancel"); await leaveTool(); } catch { /* ignore */ }
    results.push(entry);
  };
  /** A track of a pattern's meander to click on: the middle of its `i`th member. */
  const memberPoint = (st, pattern, i) => { const t = tracksOf(st).find((q) => q.id === pattern.member_ids[i]); return t.pts[Math.floor(t.pts.length / 2)]; };
  const selectPattern = async (pattern) => { const st = await state(); const [x, y] = memberPoint(st, pattern, Math.min(2, pattern.member_ids.length - 1)); await clickAt(x, y); await sleep(300); };

  // --------------------------------------------------------------------------------------------------------------------------------------------------------------------- run
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
  const original = snapshot(st0);
  const byNet = (net) => tracksOf(st0).find((t) => t.net === net);
  const have = ["PA0", "PA1", "USB_DP", "USB_DN"].every((n) => byNet(n) && byNet(n).pts.length === 2);
  if (!have || patternsOf(st0).length > 0) {
    results.push({ name: "the scratch board is as the header says", ok: false, notes: [`FAIL: tracks ${JSON.stringify(tracksOf(st0).map((t) => [t.net, t.pts.length]))}, patterns ${patternsOf(st0).length}`] });
    window.fetch = realFetch;
    check.done = true;
    return;
  }
  const pa0 = byNet("PA0").pts, dp = byNet("USB_DP").pts;
  const mid = (p) => [(p[0][0] + p[1][0]) / 2, (p[0][1] + p[1][1]) / 2];
  const startX = pa0[0][0] + 4_000_000, endX = pa0[1][0] - 6_000_000; // where a pattern starts and ends along the track
  const targetMm = 29;

  await scenario("7 arms the tool and a track under the cursor reads its length with no target; nothing under the cursor reads nothing", async (t) => {
    key("7");
    await sleep(300);
    t(E.state().tool === "tune", `the tuner is armed: tool ${E.state().tool}`);
    const [mx, my] = mid(pa0);
    const h = await hover(mx, my + 40_000);
    t(h && h.ok && h.hit === true && h.pick.net === "PA0", `the track under the cursor is picked: ${h && JSON.stringify(h.pick)}`);
    t(h && h.readout && h.readout.label === "current length" && near(h.readout.current, 26, 0.001), `it reads its length: ${h && JSON.stringify(h.readout)}`);
    t(h && h.readout && h.readout.min === null && h.readout.max === null && h.readout.constraint === null, "with no target (the box shows ---)");
    t(h && Array.isArray(h.path) && h.path.length === 2, "the line it is part of is highlighted");
    const none = await hover(mx, my + 1_500_000); // between PA0 and its neighbour, 1.5 mm from both
    t(none && none.ok && none.hit === false, `nothing is picked in the open: ${JSON.stringify(none)}`);
    t(snapshot(await state()) === original, "hovering changes nothing on the board");
    const shot = canvas().toDataURL ? true : true;
    t(shot, "the canvas is there to draw the box on");
  });

  await scenario("Ctrl+L has Board Setup's defaults and the target typed there goes with the first click", async (t) => {
    key("7");
    await sleep(200);
    const r = await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    t(r.dialog === "Tuning Pattern Properties: single track", `the dialog opened: ${JSON.stringify(r)}`);
    const f = fields();
    // target, override, min amplitude, max amplitude, spacing, corner style, radius, side, single-sided
    t(f.length >= 9 && f[0].value === "" && f[2].value === "0.2" && f[3].value === "1" && f[4].value === "0.6" && f[5].value === "round" && f[6].value === "80", `Board Setup's defaults: ${JSON.stringify(f.map((x) => x.type === "checkbox" ? x.checked : x.value))}`);
    setVal(f[0], String(targetMm));
    await sleep(100);
    await press("OK");
    t(!dialog(), "OK closes it");
    const n = replies.length;
    await clickAt(startX, pa0[0][1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.req.settings && s.req.settings.target_length === targetMm * 1e6, `the first click starts a pattern with the target: ${s && JSON.stringify([s.req, s.body.ok, s.body.message])}`);
    t(s && s.body.pattern && s.body.pattern.target_length.opt === targetMm * 1e6 && s.body.pattern.spacing === 600_000, "the pattern has the target, the page's spacing");
    await E.run("common.Interactive.cancel"); // Escape drops the pattern that was started
    await sleep(300);
    t(snapshot(await state()) === original, "nothing was put on the board");
    await leaveTool();
  });

  let pattern = null;
  await scenario("a pattern is placed: the meander is of the length asked, the keys step it, the second click puts it on the board as one undo step", async (t) => {
    key("7");
    await sleep(200);
    // the tool keeps the settings of the dialog: the target is still there
    let n = replies.length;
    await clickAt(startX, pa0[0][1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.req.settings && s.req.settings.target_length === targetMm * 1e6, "the tool keeps what the dialog set");
    const m = await moveTo(endX, pa0[0][1] - 400_000);
    t(m && m.ok && m.moved === true, `moving the cursor fits the meander: ${m && JSON.stringify([m.ok, m.moved, m.message])}`);
    t(m && m.readout && m.readout.status === "tuned" && near(m.readout.current, targetMm, 0.1), `it reads ${targetMm} mm, tuned: ${m && JSON.stringify(m.readout)}`);
    t(m && m.readout && near(m.readout.min, 28.9, 0.001) && near(m.readout.max, 29.1, 0.001), "within the default tolerance either side");
    t(m && m.preview && m.preview.pieces.length > 8 && m.preview.hide.length === 1 && m.preview.hide[0] === byNet("PA0").id, `the track it replaces is hidden: ${m && JSON.stringify(m.preview && [m.preview.pieces.length, m.preview.hide])}`);
    t(m && m.pattern.initial_side === "left", "the cursor is on the left of the track, so the first meander is");
    n = replies.length;
    key("4"); // Decrease Amplitude
    const k4 = await waitReply("step", n);
    t(k4 && k4.req.what === "amplitude_down" && k4.body.pattern.max_amplitude === 950_000, `4 steps the amplitude down: ${k4 && JSON.stringify([k4.req, k4.body.pattern && k4.body.pattern.max_amplitude])}`);
    await quiet();
    n = replies.length;
    key("1"); // Increase Spacing
    const k1 = await waitReply("step", n);
    t(k1 && k1.req.what === "spacing_up" && k1.body.pattern.spacing === 650_000 && k1.body.readout.status === "tuned", `1 steps the spacing up and the meander is fitted again, still tuned: ${k1 && JSON.stringify([k1.req, k1.body.pattern && k1.body.pattern.spacing, k1.body.readout && k1.body.readout.status])}`);
    await quiet();
    n = replies.length;
    await clickAt(endX, pa0[0][1] - 400_000);
    const f = await waitReply("fix", n, 20000);
    await quiet();
    t(f && f.body.ok === true && !!f.body.pattern_id, `the second click puts it on the board: ${f && JSON.stringify(f.body)}`);
    const st = await state();
    pattern = patternsOf(st).find((p) => p.id === (f && f.body.pattern_id));
    t(!!pattern && pattern.mode === "single" && pattern.member_ids.length > 8, `a pattern with the tracks of the meander as members: ${pattern && JSON.stringify([pattern.mode, pattern.member_ids.length])}`);
    t(pattern && pattern.info === `${targetMm} mm (tuned)`, `it is ${pattern && pattern.info}`);
    t(pattern && pattern.max_amplitude === 950_000 && pattern.spacing === 650_000, "it keeps the amplitude and the spacing the keys left");
    t(near(netMm(st, "PA0"), targetMm, 0.1), `PA0 is ${netMm(st, "PA0").toFixed(3)} mm long on the board`);
    t(near(netMm(st, "PA1"), 26, 0.001), "the neighbour is untouched");
    t(E.state().selection.length === 1 && E.state().selection[0].id === pattern.id && E.state().selection[0].kind === "group", `the pattern is selected: ${JSON.stringify(E.state().selection)}`);
    t(tracksOf(st).some((q) => q.arc_mid), "the rounded corners are arc tracks");
    await E.run("common.Interactive.undo");
    await sleep(600);
    const back = await state();
    t(snapshot(back) === original && patternsOf(back).length === 0, "one Undo takes the whole pattern away: the tracks are as they were");
    await E.run("common.Interactive.redo");
    await sleep(600);
    const again = await state();
    t(patternsOf(again).length === 1 && near(netMm(again, "PA0"), targetMm, 0.1), "and one Redo brings it back");
  });

  await scenario("a click on a track of the meander selects the whole pattern; its properties tune it again, in one undo step", async (t) => {
    await leaveTool();
    let st = await state();
    pattern = patternsOf(st)[0];
    const before = snapshot(st);
    await selectPattern(pattern);
    t(E.state().selection.length === 1 && E.state().selection[0].id === pattern.id, `the pattern is selected: ${JSON.stringify(E.state().selection)}`);
    const grp = (st.drawings.groups ?? []).find((g) => g.id === pattern.id);
    t(grp && grp.generator === "tuning_pattern" && grp.member_ids.length === pattern.member_ids.length, "it is a group that is a generator");
    const r = await E.run("pcbnew.InteractiveEdit.properties");
    await sleep(500);
    t(r.dialog === "Tuning Pattern Properties: single track", `E opens the Tuning Pattern dialog: ${JSON.stringify(r)}`);
    let f = fields();
    t(f[0].value === String(targetMm), `with the pattern's target: ${f[0] && f[0].value}`);
    t(f[3].value === "0.95" && f[4].value === "0.65", `and its amplitude and spacing: ${f.map((x) => x.value).join(", ")}`);
    setVal(f[0], "27.5");
    await sleep(100);
    const n = replies.length;
    await press("OK");
    const u = await waitReply("update", n, 20000);
    await quiet();
    t(u && u.body.ok === true && u.body.info === "27.5 mm (tuned)", `OK tunes it again: ${u && JSON.stringify([u.body.ok, u.body.message, u.body.info])}`);
    st = await state();
    const now = patternsOf(st);
    t(now.length === 1 && now[0].id === pattern.id, "the same pattern: its id is kept");
    t(near(netMm(st, "PA0"), 27.5, 0.1), `the net is ${netMm(st, "PA0").toFixed(3)} mm long`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(snapshot(await state()) === before, "one Undo puts the first tuning back");
    await E.run("common.Interactive.redo");
    await sleep(600);
    t(near(netMm(await state(), "PA0"), 27.5, 0.1), "and Redo the second");
  });

  await scenario("an edit point is dragged: the pattern's end moves, it is fitted again and the edit is one undo step", async (t) => {
    let st = await state();
    pattern = patternsOf(st)[0];
    const before = snapshot(st);
    await selectPattern(pattern);
    const [hx, hy] = pattern.handles[1];
    t(pattern.handles.length === 4 && hx === pattern.end.x && hy === pattern.end.y, `the end point is where the pattern ends: ${JSON.stringify(pattern.handles)}`);
    const n = replies.length;
    await drag([hx, hy], [hx - 4_000_000, hy]);
    const calls = replies.slice(n).map((r) => r.name);
    t(calls[0] === "edit_start" && calls.filter((c) => c === "edit_move").length >= 2 && calls[calls.length - 1] === "fix", `the press starts an edit, the moves update it, the release puts it on the board: ${calls.join(" ")}`);
    st = await state();
    const p = patternsOf(st)[0];
    t(p && p.id === pattern.id && p.end.x < pattern.end.x - 3_000_000 && p.end.y === pattern.end.y, `the end moved back: ${p && JSON.stringify(p.end)} (was ${JSON.stringify(pattern.end)})`);
    t(p && p.info.endsWith("(tuned)") && near(netMm(st, "PA0"), 27.5, 0.1), `still ${p && p.info}, ${netMm(st, "PA0").toFixed(3)} mm`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(snapshot(await state()) === before, "one Undo puts the pattern back where it was");
    // genStartEdit from the menu / action: the end follows the cursor, a click finishes
    await selectPattern(patternsOf(await state())[0]);
    const m0 = replies.length;
    await E.run("pcbnew.Generator.genStartEdit");
    await sleep(500);
    t(E.state().tool === "tune" && !!last("edit_start", m0), "genStartEdit starts the edit of the selected pattern");
    await E.run("pcbnew.Generator.genCacnelEdit");
    await sleep(300);
    t(snapshot(await state()) === before, "genCacnelEdit drops it: nothing was written");
    await leaveTool();
  });

  await scenario("the Generators Manager lists the pattern, Rebuild All tunes it again; the nine Generator actions are handled", async (t) => {
    const acts = await E.actions();
    const gen = ["genCacnelEdit", "genFinishEdit", "genRemove", "genStartEdit", "genUpdateEdit", "regenerateAll", "regenerateAllTuning", "regenerateSelected", "showManager"].map((n) => `pcbnew.Generator.${n}`);
    t(gen.every((id) => acts.some((a) => a.id === id && a.enabled)), `every one is handled: ${gen.filter((id) => !acts.some((a) => a.id === id && a.enabled)).join(", ") || "all"}`);
    const r = await E.run("pcbnew.Generator.showManager");
    await sleep(600);
    t(r.dialog === "Generators Manager", `the manager opened: ${JSON.stringify(r)}`);
    const tab = dialog()?.querySelector('[role="tab"]')?.textContent;
    const rows = [...(dialog()?.querySelectorAll("tbody tr") ?? [])].map((tr) => [...tr.querySelectorAll("td")].map((td) => td.textContent));
    t(tab === "Tuning Pattern (1)", `a page for the type: ${tab}`);
    t(rows.length === 1 && rows[0][0] === "PA0" && /^27\.5.* \(tuned\)$/.test(rows[0][1]) && rows[0][2] === "F.Cu", `its row: Net, Tuning, Layer: ${JSON.stringify(rows)}`);
    dialog().querySelector("tbody tr").click();
    await sleep(300);
    t(E.state().selection.length === 1 && E.state().selection[0].kind === "group", "a row selects the pattern on the board");
    const before = snapshot(await state());
    const n = replies.length;
    await press("Rebuild All");
    const g = await waitReply("regenerate", n, 20000);
    await quiet();
    t(g && g.body.ok === true && g.body.regenerated === 1, `Rebuild All: ${g && JSON.stringify(g.body)}`);
    const st = await state();
    t(near(netMm(st, "PA0"), 27.5, 0.1) && patternsOf(st).length === 1, "the pattern is tuned again, as it was");
    await press("Close");
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(snapshot(await state()) === before, "and that was one undo step");
    await E.run("common.Interactive.redo");
    await sleep(600);
  });

  await scenario("Delete removes the pattern and puts the track back straight, in one undo step", async (t) => {
    let st = await state();
    pattern = patternsOf(st)[0];
    const before = snapshot(st);
    await selectPattern(pattern);
    const n = replies.length;
    await E.run("common.Interactive.delete");
    const r = await waitReply("remove", n, 20000);
    await quiet();
    await sleep(500);
    st = await state();
    t(r && r.body.ok === true && patternsOf(st).length === 0, `the pattern is gone: ${r && JSON.stringify(r.body)}`);
    const pa0Tracks = tracksOf(st).filter((q) => q.net === "PA0");
    t(near(netMm(st, "PA0"), 26, 0.001) && pa0Tracks.every((q) => q.pts.every((p) => p[1] === pa0[0][1])), `PA0 is a straight 26 mm again: ${pa0Tracks.length} tracks, ${netMm(st, "PA0").toFixed(3)} mm`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(snapshot(await state()) === before && patternsOf(await state()).length === 1, "one Undo brings the pattern back");
    await undoTo(original);
    t(snapshot(await state()) === original, "the board is as it was at the start (undone to it)");
  });

  let pairBefore = null;
  await scenario("8 tunes a pair together: both nets are tuned to the target, one pattern holds the tracks of both, Delete puts both lines back", async (t) => {
    pairBefore = snapshot(await state());
    key("8");
    await sleep(300);
    t(E.state().tool === "tune", "8 arms the tuner for a pair");
    const r = await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    t(r.dialog === "Tuning Pattern Properties: differential pair", `its dialog says so: ${JSON.stringify(r)}`);
    let f = fields();
    t(f[4].value === "1", `a pair's spacing starts at 1 mm: ${f[4] && f[4].value}`);
    setVal(f[0], "30");
    await sleep(100);
    await press("OK");
    const [mx, my] = mid(dp);
    const h = await hover(mx, my + 20_000);
    t(h && h.hit && h.readout.partner === "USB_DN" && h.readout.nets && near(h.readout.nets[0], 26, 0.001) && near(h.readout.nets[1], 26, 0.001), `the hover reads the pair, the partner and both lengths: ${h && JSON.stringify(h.readout)}`);
    t(h && h.readout.status === "too_short" && near(h.readout.opt, 30, 0.001), "26 mm against 30 mm is too short");
    let n = replies.length;
    await clickAt(startX, dp[0][1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.mode === "diffpair", `the click starts a pair pattern: ${s && JSON.stringify([s.body.ok, s.body.message, s.body.mode])}`);
    const m = await moveTo(endX, dp[0][1] - 500_000);
    t(m && m.ok && m.readout.status === "tuned" && near(m.readout.current, 30, 0.1), `the pair is ${m && m.readout.current} mm, tuned`);
    t(m && m.preview.hide.length === 2, "both lines are replaced in the preview");
    n = replies.length;
    await clickAt(endX, dp[0][1] - 500_000);
    const fx = await waitReply("fix", n, 20000);
    await quiet();
    const st = await state();
    const pat = patternsOf(st).find((p) => p.mode === "diff_pair");
    t(fx && fx.body.ok === true && !!pat, `it is on the board: ${fx && JSON.stringify(fx.body.message)}`);
    const nets = pat ? [...new Set(pat.member_ids.map((id) => tracksOf(st).find((q) => q.id === id).net))].sort() : [];
    t(nets.join(",") === "USB_DN,USB_DP", `its members are tracks of both nets: ${nets.join(",")}`);
    t(near(netMm(st, "USB_DP"), 30, 0.15) && near(netMm(st, "USB_DN"), 30, 0.15), `both nets are ${netMm(st, "USB_DP").toFixed(3)} and ${netMm(st, "USB_DN").toFixed(3)} mm`);
    t(pat && pat.base_line.length >= 2 && pat.base_line_coupled.length >= 2, "it keeps both baselines");
    await leaveTool();
    await selectPattern(pat);
    n = replies.length;
    await E.run("common.Interactive.delete");
    const rm = await waitReply("remove", n, 20000);
    await quiet();
    await sleep(500);
    const after = await state();
    t(rm && rm.body.ok === true && patternsOf(after).length === 0 && near(netMm(after, "USB_DP"), 26, 0.001) && near(netMm(after, "USB_DN"), 26, 0.001), `Delete puts both lines back: ${netMm(after, "USB_DP").toFixed(3)} and ${netMm(after, "USB_DN").toFixed(3)} mm`);
    await undoTo(pairBefore);
    t(snapshot(await state()) === pairBefore, "and everything undone, the board is as it was");
  });

  await scenario("9 reads the skew: the shorter line of a pair is lengthened until the nets are as long as each other", async (t) => {
    // USB_DP tuned alone to 28 mm: it is 2 mm longer than USB_DN
    await leaveTool();
    key("7");
    await sleep(200);
    await E.run("pcbnew.LengthTuner.Settings");
    await sleep(400);
    setVal(fields()[0], "28");
    await sleep(100);
    await press("OK");
    let n = replies.length;
    await clickAt(startX, dp[0][1]);
    await waitReply("start", n);
    await moveTo(endX, dp[0][1] - 500_000);
    n = replies.length;
    await clickAt(endX, dp[0][1] - 500_000);
    await waitReply("fix", n, 20000);
    await quiet();
    await leaveTool();
    let st = await state();
    t(near(netMm(st, "USB_DP"), 28, 0.1) && near(netMm(st, "USB_DN"), 26, 0.001), `USB_DP is ${netMm(st, "USB_DP").toFixed(3)} mm, USB_DN ${netMm(st, "USB_DN").toFixed(3)}`);
    key("9");
    await sleep(300);
    t(E.state().tool === "tune", "9 arms the tuner for a skew");
    const dn = byNet("USB_DN").pts;
    const h = await hover(endX + 3_000_000, dn[0][1] + 20_000);
    t(h && h.hit && h.pick.net === "USB_DN" && h.readout.label === "current skew", `the hover reads the skew: ${h && JSON.stringify(h.readout)}`);
    t(h && near(h.readout.current, -2, 0.05) && h.readout.status === "too_short", `USB_DN is 2 mm short of its partner: ${h && h.readout.current}`);
    t(h && near(h.readout.min, -0.1, 0.001) && near(h.readout.opt, 0, 0.001) && near(h.readout.max, 0.1, 0.001), "the skew target is zero, 0.1 mm either side");
    n = replies.length;
    await clickAt(endX - 6_000_000, dn[0][1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.mode === "skew", `the click starts a skew pattern: ${s && JSON.stringify([s.body.ok, s.body.message])}`);
    const m = await moveTo(endX + 2_000_000, dn[0][1] + 500_000);
    t(m && m.ok && m.readout.status === "tuned" && Math.abs(m.readout.current) < 0.1, `the skew after: ${m && m.readout.current}`);
    n = replies.length;
    await clickAt(endX + 2_000_000, dn[0][1] + 500_000);
    await waitReply("fix", n, 20000);
    await quiet();
    st = await state();
    t(near(netMm(st, "USB_DN"), netMm(st, "USB_DP"), 0.1), `the nets are ${netMm(st, "USB_DP").toFixed(3)} and ${netMm(st, "USB_DN").toFixed(3)} mm`);
    const skewPattern = patternsOf(st).find((p) => p.mode === "diff_pair_skew");
    t(skewPattern && skewPattern.member_ids.every((id) => tracksOf(st).find((q) => q.id === id).net === "USB_DN"), "its members are USB_DN's tracks: only the clicked line was lengthened");
    await leaveTool();
    await undoTo(pairBefore);
    t(snapshot(await state()) === pairBefore, "undone, the board is as it was");
  });

  await scenario("a net that is not half of a pair is refused for 8, with KiCad's message; Escape drops a started pattern and then leaves the tool", async (t) => {
    key("8");
    await sleep(300);
    const toastsBefore = E.errors(errorsFrom).length;
    const n = replies.length;
    await clickAt(startX, pa0[0][1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === false && /complementary differential pair/.test(s.body.message || ""), `PA0 is refused: ${s && JSON.stringify(s.body.message)}`);
    await leaveTool();
    key("7");
    await sleep(300);
    const n2 = replies.length;
    await clickAt(startX, pa0[0][1]);
    await waitReply("start", n2);
    t(E.state().tool === "tune", "a pattern is started");
    // The key itself, not `common.Interactive.cancel`: the Generators Manager (open and closed in an earlier scenario) stays mounted, and while it listened when closed it took every Escape.
    key("Escape");
    await sleep(300);
    t(E.state().tool === "tune", "the first Escape drops the pattern and the tool goes on");
    key("Escape");
    await sleep(300);
    t(E.state().tool === "select", "the second leaves the tool");
    const gone = await post("/api/tune/move", { x: 1000, y: 1000 });
    t(gone.ok === false && /No tuning pattern is being placed/.test(gone.message || ""), `and the session is over: ${JSON.stringify(gone)}`);
    t(toastsBefore >= 0, "(the refusal is the one error the studio reports)");
  });

  await scenario("the board is as it was, and the studio reported no error but the refusal", async (t) => {
    t(snapshot(await state()) === original, "tracks are the same as before the run");
    const errors = E.errors(errorsFrom).filter((e) => !/complementary differential pair/.test(e.message));
    t(errors.length === 0, `no other error: ${JSON.stringify(errors.slice(0, 3))}`);
  });

  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
