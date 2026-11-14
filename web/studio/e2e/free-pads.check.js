// Free pads (docs/parity/UI-LEFT-3.md R3-7; crates/pns/PARITY.md, the router-1 stage): driven the way a person does it -- Preferences, clicks on the canvas, the edit
// actions and the router tool -- with `/api/cmd` for the one board fact the page cannot make (a pad's pin type), read from `/api/state` and the router's replies.
//
// Run it on a COPY of the nc-pins-placed scratch board (tools/run-all-checks.sh makes it: examples/nc_pins.yaml placed, so J1.4 .. J1.6 and U1.4 are pins alone on
// their nets). It undoes everything it does and ends by checking the board is as it found it.
//
// What it checks:
//   preference   Preferences > PCB Editor > Editing Options has "Allow free pads" live (KiCad's default off), and OK keeps it (`PCBNEW_SETTINGS::m_AllowFreePads`);
//   off          with it off a selected pad stands for its footprint (`FilterCollectorForFreePads`): Rotate turns J1;
//   on           with it on Rotate turns the pad alone, M carries it alone and the click drops it there (its footprint stays), Swap trades two pads' places, each one
//                undo step; Flip still turns the footprint over (the studio holds a pad on its footprint's side);
//   free pad     J1.4 alone on its net with the pin type "free" (`PAD::IsFreePad`) is `free` in the state; a route from J1.3 ends in it when the cursor is inside it
//                (`pickSingleItem`'s free pad) and is committed with no clearance to it (`collideSimple`), and the routing_clearance gate does not fail it.
// It answers through `window.__freePadsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=nc-pins-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__freePadsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 15000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // what the studio asks of the router and of the board
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/api\/route\/([a-z_]+)/.exec(url) ?? (/\/api\/cmd/.test(url) ? [null, "cmd"] : null);
    if (m) replies.push({ name: m[1], req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(300); } };

  // the page
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, shift = false) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerdown" ? 1 : 0, shiftKey: shift }));
  };
  const clickAt = async (wx, wy, shift = false) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y, shift);
    await sleep(200);
    fire("pointerdown", x, y, shift);
    await sleep(40);
    fire("pointerup", x, y, shift);
    await sleep(350);
  };
  const pointerTo = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(250); };
  const state = () => json("/api/state");
  const dialog = () => document.querySelector(".dialog");
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const partOf = (st, ref) => (st.parts ?? []).find((p) => p.ref === ref);
  const padOf = (st, id) => { for (const p of st.parts ?? []) for (const q of p.pads ?? []) if (q.id === id) return q; return null; };
  const snapshot = (st) => JSON.stringify([(st.routing?.tracks ?? []).map((t) => [t.id, t.net, t.pts]), (st.parts ?? []).map((p) => [p.ref, p.at, p.rot, p.side, (p.pads ?? []).map((q) => [q.id, q.px ?? q.x, q.py ?? q.y, q.rot, q.free ?? false])])]);
  let original = null;
  const restore = async () => { for (let i = 0; i < 12 && snapshot(await state()) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(600); } };
  const select = async (ids) => {
    await E.run("common.InteractiveSelection.clear").catch(() => null);
    await sleep(200);
    const st = await state();
    for (const [k, id] of ids.entries()) { const q = padOf(st, id); await clickAt(q.x, q.y, k > 0); }
    return E.state().selection.map((s) => s.id ?? s);
  };
  const setFreePadsPref = async (on) => {
    await E.run("common.SuiteControl.openPreferences");
    await until(() => tid("preferences-dialog"));
    tid("prefs-page-pcb_edit")?.click();
    await sleep(300);
    const box = tid("pref-allow-free-pads");
    const live = !!box && !box.disabled;
    if (box && box.checked !== on) box.click();
    await sleep(150);
    tid("preferences-ok")?.click();
    await sleep(500);
    return live;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { while (dialog()) { const b = [...dialog().querySelectorAll("button")].find((x) => /Cancel|Close/.test(x.textContent)); if (!b) break; b.click(); await sleep(300); } await leaveTool(); await restore(); } catch { /* best effort */ }
    if (snapshot(await state()) !== original) { entry.notes.push("FAIL: the board is not as it was after the scenario"); entry.ok = false; }
    results.push(entry);
  };

  for (let i = 0; i < 40 && !(E.state().tab === "pcb" && E.state().counts.footprints > 0); i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  await sleep(800);
  // The checks are of which items the tools take, not of what the board gates make of a pad moved off its place: they run with Strict off, as board-edit does
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const errorsFrom = E.errors().length;
  original = snapshot(await state());
  const st0 = await state();
  const j1 = partOf(st0, "J1");

  await scenario("Preferences > Editing Options: Allow free pads is live, off by default, and OK keeps it", async (t) => {
    t(E.state().prefs?.allowFreePads !== true, "off to begin with (KiCad's default)");
    const live = await setFreePadsPref(true);
    t(live, "the row is a live check box, not greyed");
    const back = await setFreePadsPref(false);
    t(back, "and it can be unticked again");
  });

  await scenario("off: a selected pad stands for its footprint -- Rotate turns J1", async (t) => {
    await setFreePadsPref(false);
    const sel = await select(["J1.2"]);
    t(sel.includes("J1.2"), `a click on J1.2 selects the pad: ${JSON.stringify(sel)}`);
    const n = replies.length;
    await E.run("pcbnew.InteractiveEdit.rotateCcw");
    const r = await waitReply("cmd", n);
    t(!!r && JSON.stringify(r.req).includes('"ids":["J1"]'), `the rotation is J1's: ${r && JSON.stringify(r.req)}`);
    await quiet();
    const st = await state();
    t(partOf(st, "J1").rot !== j1.rot, `J1 turned: ${j1.rot} -> ${partOf(st, "J1").rot}`);
  });

  await scenario("on: Rotate turns the pad alone, M carries it alone, Swap trades two pads; Flip still turns the footprint", async (t) => {
    await setFreePadsPref(true);
    let sel = await select(["J1.2"]);
    let n = replies.length;
    await E.run("pcbnew.InteractiveEdit.rotateCcw");
    let r = await waitReply("cmd", n);
    t(!!r && JSON.stringify(r.req).includes('"ids":["J1.2"]'), `Rotate sends the pad: ${r && JSON.stringify(r.req)}`);
    await quiet();
    let st = await state();
    t(partOf(st, "J1").rot === j1.rot && JSON.stringify(partOf(st, "J1").at) === JSON.stringify(j1.at), "J1 stays where it was");
    t(padOf(st, "J1.2").rot !== padOf(st0, "J1.2").rot, `the pad turned on its own: ${padOf(st0, "J1.2").rot} -> ${padOf(st, "J1.2").rot}`);
    await E.run("common.Interactive.undo");
    await sleep(700);
    t(snapshot(await state()) === original, "one undo puts it back");

    // M: the pad carried from its centre to 3 mm below, dropped with a click
    sel = await select(["J1.2"]);
    const q = padOf(st0, "J1.2");
    await pointerTo(q.x, q.y);
    await E.run("pcbnew.InteractiveMove.move");
    await sleep(300);
    await pointerTo(q.x, q.y + 1_000_000);
    t((E.state().moving?.refs ?? []).includes("J1.2"), `M carries the pad itself: ${JSON.stringify(E.state().moving)}`);
    n = replies.length;
    await clickAt(q.x, q.y + 3_000_000);
    r = await waitReply("cmd", n);
    await quiet();
    st = await state();
    const moved = padOf(st, "J1.2");
    t(!!r && JSON.stringify(r.req).includes('"J1.2"') && r.body?.ok !== false, `the drop moves the pad: ${r && JSON.stringify([r.req, r.body?.ok, r.body?.message])}`);
    t(Math.abs((moved.py ?? moved.y) - (q.py ?? q.y) - 3_000_000) < 200_000 && Math.abs((moved.px ?? moved.x) - (q.px ?? q.x)) < 200_000, `it is 3 mm lower: ${JSON.stringify([q.px ?? q.x, q.py ?? q.y])} -> ${JSON.stringify([moved.px ?? moved.x, moved.py ?? moved.y])}`);
    t(JSON.stringify(partOf(st, "J1").at) === JSON.stringify(j1.at), "J1 stays");
    await E.run("common.Interactive.undo");
    await sleep(700);
    t(snapshot(await state()) === original, "one undo puts it back");

    // Swap: J1.1 and J1.2 trade places
    sel = await select(["J1.1", "J1.2"]);
    t(sel.includes("J1.1") && sel.includes("J1.2"), `both pads are selected: ${JSON.stringify(sel)}`);
    n = replies.length;
    await E.run("pcbnew.InteractiveEdit.swap");
    r = await waitReply("cmd", n);
    await quiet();
    st = await state();
    const [a0, b0, a1, b1] = [padOf(st0, "J1.1"), padOf(st0, "J1.2"), padOf(st, "J1.1"), padOf(st, "J1.2")];
    t(!!r && JSON.stringify(r.req).includes('"swap_items"'), `Swap sends the two pads: ${r && JSON.stringify(r.req)}`);
    t(Math.abs(a1.x - b0.x) < 1000 && Math.abs(a1.y - b0.y) < 1000 && Math.abs(b1.x - a0.x) < 1000 && Math.abs(b1.y - a0.y) < 1000, `they traded places: ${JSON.stringify([[a0.x, a0.y], [b0.x, b0.y]])} -> ${JSON.stringify([[a1.x, a1.y], [b1.x, b1.y]])}`);
    await E.run("common.Interactive.undo");
    await sleep(700);
    t(snapshot(await state()) === original, "one undo puts it back");

    // Flip: the footprint is turned over (a pad is held on its footprint's side)
    sel = await select(["J1.2"]);
    n = replies.length;
    await E.run("pcbnew.InteractiveEdit.flip");
    r = await waitReply("cmd", n);
    await quiet();
    t(!!r && JSON.stringify(r.req).includes('"ids":["J1"]'), `Flip turns J1 over: ${r && JSON.stringify(r.req)}`);
    await setFreePadsPref(false);
  });

  await scenario("a free pad: J1.4 with the pin type free is free, a route from J1.3 ends in it and is committed with no clearance to it", async (t) => {
    const set = await cmd({ op: "edit_board_pad", part: "J1", edit: { number: "4", pin_type: "free" } });
    t(set.ok !== false, `J1.4's pin type is free: ${JSON.stringify(set.message ?? set.ok)}`);
    await E.refresh?.();
    await sleep(800);
    let st = await state();
    t(padOf(st, "J1.4")?.free === true, `the state says J1.4 is a free pad: ${JSON.stringify(padOf(st, "J1.4"))}`);
    // (a board with no routing yet judges no connection; the route's commit is judged strictly whatever the toolbar says, so the board is given a GND stub
    // first and the first track does not count every net's open connections against the route)
    const stub = await cmd({ op: "add_track", net: "GND", layer: "F.Cu", width: 250_000, pts: [{ x: padOf(st, "J1.2").x, y: padOf(st, "J1.2").y }, { x: padOf(st, "J1.2").x, y: padOf(st, "J1.2").y + 2_000_000 }] });
    t(stub.ok !== false, `a GND stub on J1.2: ${JSON.stringify(stub.message ?? stub.ok)}`);
    await sleep(600);
    const from = padOf(st, "J1.3"), to = padOf(st, "J1.4");
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(250);
    let n = replies.length;
    await clickAt(from.x, from.y);
    const s = await waitReply("start", n);
    t(!!s && s.body.ok === true, `the route starts on J1.3: ${s && JSON.stringify([s.body.ok, s.body.net])}`);
    n = replies.length;
    await pointerTo(to.x + 150_000, to.y + 100_000);
    const mv = await waitReply("move", n);
    await quiet();
    const last = replies.filter((x) => x.name === "move").pop();
    const snapped = last && last.body.snapped_end;
    t(!!snapped && Math.abs(snapped[0] - (to.px ?? to.x)) < 1000 && Math.abs(snapped[1] - (to.py ?? to.y)) < 1000, `the cursor inside the free pad ends the route on it: ${JSON.stringify(snapped)} (pad at ${JSON.stringify([to.px ?? to.x, to.py ?? to.y])})`);
    t(!!mv && last.body.colliding !== true, "and the head does not collide with it");
    n = replies.length;
    await E.run("common.Interactive.finish");
    const fin = await waitReply("finish", n);
    await quiet();
    t(!!fin && fin.body.ok !== false, `End commits the route: ${fin && JSON.stringify(fin.body).slice(0, 400)}`);
    const committed = await waitReply("cmd", n, 5000);
    t(!committed || committed.body?.ok !== false, `the commit applies: ${committed && JSON.stringify([committed.req?.cmd?.op ?? committed.req?.op, committed.body?.ok, committed.body?.message])}`);
    st = await state();
    const xy = (p) => (Array.isArray(p) ? p : [p.x, p.y]);
    const track = (st.routing?.tracks ?? []).find((tr) => tr.pts.map(xy).some((p) => Math.abs(p[0] - (to.px ?? to.x)) < 1000 && Math.abs(p[1] - (to.py ?? to.y)) < 1000));
    t(!!track, `a track ends in J1.4: ${JSON.stringify((st.routing?.tracks ?? []).map((x) => x.pts))}`);
    const gate = (st.checks ?? []).filter((c) => c.check === "routing_clearance" && c.status === "fail");
    t(gate.length === 0, `the routing_clearance gate does not fail it: ${JSON.stringify(gate)}`);
  });

  if (strictWas && strict && !strict.checked) strict.click();
  const errs = E.errors(errorsFrom);
  results.push({ name: "no errors in the page", ok: errs.length === 0, notes: errs.length ? errs.map((e) => `FAIL: ${JSON.stringify(e)}`) : ["ok: none"] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
