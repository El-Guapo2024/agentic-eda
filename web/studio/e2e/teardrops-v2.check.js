// Teardrops, round 2 (docs/parity/UI-LEFT-2.md PP6, PP7; web/studio/PARITY-pcb.md section 94): teardrops kept current as the board is edited (KiCad 25ec2335
// `BOARD_COMMIT::Push`: `RemoveTeardrops` and `UpdateTeardrops` of what the commit made stale, in the commit's own undo step), fitted to other nets' clearance
// (`computeFittedTeardropPolygon`), and curved (`m_CurvedEdges`, Board Setup's "Curved edges"). Driven the way a person does it -- Board Setup's page, a click on
// a track, the `D` action, a move and a click, Ctrl+Z -- and read from `/api/state`.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of the scratch board
// below: it places R1 and R2 if they are not, adds a via and tracks and undoes everything it did, and it ends by checking the board is as it found it.
//
//   eda board new examples/mcu_board_30plus.yaml -o board        # 34 x 34 mm, outline only: the parts are unplaced
//   eda board serve -C board --port 8840 --ui web/studio/dist
//
// What it checks:
//   follow   a via with a track leaving it east, Board Setup's teardrops on and Add All Teardrops: one teardrop on the via. The track selected and D on its middle,
//            a move 2 mm north and a click: the track's first segment now leaves the via at an angle, and the via's teardrop has followed it (made again along the
//            new segment, in the same commit); one Ctrl+Z puts the track and the teardrop back as they were;
//   fitted   a track of another net put beside the teardrop's flank (one commit): the teardrop is fitted again, narrower (its area smaller, the via still under it);
//            one Ctrl+Z gives the wide one back;
//   curved   Board Setup > Teardrops' "Curved edges" checked, Apply Settings, Add All Teardrops: the teardrop's sides are curves (more than the pentagon's five
//            points); three undos (Apply, and Add All's save and add) give the straight one back.
// It answers through `window.__teardropsV2Check`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__teardropsV2Check = { results, done: false, ok: false, wait: null });
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
    if (m) replies.push({ name: m[1], body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
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
  const state = () => json("/api/state");
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const teardropsOf = (st) => (st.routing?.zones ?? []).filter((z) => z.teardrop);
  const partOf = (st, ref) => st.parts.find((p) => p.ref === ref);
  const outlineOf = (z) => (z.outline ?? []).map((p) => (Array.isArray(p) ? p : [p.x, p.y]));
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), (st.routing?.vias ?? []).map((v) => [v.id, v.at]), teardropsOf(st).map((z) => [z.layer, outlineOf(z)]).sort(), st.routing?.teardrop_settings ?? null, st.parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot])]);
  const settle = async () => {
    const st = await state();
    for (let i = 0; i < 50 && !(E.state().counts.tracks === tracksOf(st).length); i++) await sleep(150);
    await sleep(900);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const clearSelection = async () => { for (let i = 0; i < 3 && E.state().selection.length; i++) { await E.run("common.Interactive.cancel"); await sleep(200); } };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(600); await settle(); };
  const area = (pts) => Math.abs(pts.reduce((s, p, i) => s + p[0] * pts[(i + 1) % pts.length][1] - pts[(i + 1) % pts.length][0] * p[1], 0)) / 2;
  const inside = (pts, q) => { let c = false; for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) { const [xi, yi] = pts[i], [xj, yj] = pts[j]; if (yi > q[1] !== yj > q[1] && q[0] < ((xj - xi) * (q[1] - yi)) / (yj - yi) + xi) c = !c; } return c; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  // Board Setup
  const dialog = () => document.querySelector(".bs-dialog");
  const openTeardrops = async () => {
    if (!dialog()) { await E.run("pcbnew.EditorControl.boardSetup"); await sleep(600); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Teardrops"))?.click();
    await sleep(350);
    const p = document.querySelector('section.bs-page[aria-label="Teardrops"]');
    if (!p || p.hidden) throw new Error("Board Setup has no Teardrops page on screen");
    return p;
  };
  const closeDialog = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((x) => ["Close", "Discard and close", "OK", "Cancel"].includes(x.textContent.trim()))?.click();
      await sleep(300);
    }
  };
  const press = async (root, label, changed) => {
    const button = () => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === label);
    // the page's buttons are disabled while its last command is on its way (`tdBusy`)
    for (let i = 0; i < 60 && button()?.disabled; i++) await sleep(250);
    button()?.click();
    for (let i = 0; i < 60; i++) { const s = await state(); if (changed(s)) return s; await sleep(250); }
    return state();
  };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const before = await state();
  const original = snapshot(before);
  let steps = 0;
  if (!partOf(before, "R1")?.placed) { await cmd({ op: "place_at", part: "R1", x: 6_000_000, y: 6_000_000 }); steps++; }
  if (!partOf(before, "R2")?.placed) { await cmd({ op: "place_at", part: "R2", x: 28_000_000, y: 28_000_000 }); steps++; }
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);
  let st = await state();
  const nets = [...new Set(st.parts.flatMap((p) => p.pads ?? []).map((p) => p.net).filter((x) => !!x))];
  const [VX, VY] = [10_000_000, 14_000_000];
  const line = (net, pts, width = 200_000) => ({ net, layer: "F.Cu", width, pts: pts.map(([x, y]) => ({ x, y })) });
  await cmd({ op: "add_via", net: nets[0], x: VX, y: VY, drill: 300_000, diameter: 800_000, from_layer: "F.Cu", to_layer: "B.Cu" });
  await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[0], [[VX, VY], [VX + 10_000_000, VY]])], vias: [] });
  const td0 = st.routing?.teardrop_settings ?? null;
  await cmd({ op: "set_teardrop_settings", settings: { ...(td0 ?? {}), enabled: true } });
  await cmd({ op: "add_all_teardrops" });
  steps += 4;
  await settle();
  st = await state();
  const setUp = snapshot(st);
  const viaTeardrop = (s) => teardropsOf(s).find((z) => inside(outlineOf(z), [VX, VY]));

  await scenario("follow", async (t) => {
    const z0 = viaTeardrop(st);
    t(!!z0, `Add All Teardrops made the via's teardrop (${teardropsOf(st).length} teardrops)`);
    await clearSelection();
    await clickAt(VX + 6_000_000, VY);
    t(E.state().selection.length === 1, `the track is selected: ${JSON.stringify(E.state().selection.map((x) => x.id))}`);
    await pointerTo(VX + 6_000_000, VY);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Drag45Degree");
    t(!!(await waitReply("drag_start", n)), "D started a drag");
    await pointerTo(VX + 6_000_000, VY - 2_000_000);
    await waitReply("drag_move", n, 8000);
    await quiet();
    const at = w2c(VX + 6_000_000, VY - 2_000_000);
    fire("pointerdown", at[0], at[1]);
    await sleep(40);
    fire("pointerup", at[0], at[1]);
    await waitReply("drag_finish", n, 8000);
    await settle();
    const s1 = await state();
    const moved = tracksOf(s1).filter((x) => x.net === nets[0]);
    const first = moved.flatMap((x) => [x.pts]).find((pts) => pts.some((p) => Math.abs(p[0] - VX) < 2 && Math.abs(p[1] - VY) < 2));
    t(!!first && first.length >= 2, `the track still leaves the via: ${JSON.stringify(moved.map((x) => x.pts))}`);
    const z1 = viaTeardrop(s1);
    t(!!z1, "the via still has its teardrop");
    t(!!z0 && !!z1 && JSON.stringify(outlineOf(z0)) !== JSON.stringify(outlineOf(z1)), "it was made again along the moved track");
    t(teardropsOf(s1).length === 1, `one teardrop (${teardropsOf(s1).length})`);
    await undo();
    t(snapshot(await state()) === setUp, "one Ctrl+Z puts the track and its teardrop back");
  });

  await scenario("fitted", async (t) => {
    const z0 = viaTeardrop(await state());
    await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [line(nets[1], [[VX + 600_000, VY + 480_000], [VX + 600_000, VY + 4_000_000]])], vias: [] });
    await settle();
    const s1 = await state();
    const z1 = viaTeardrop(s1);
    t(!!z1, "the via keeps a teardrop");
    t(!!z0 && !!z1 && area(outlineOf(z1)) < area(outlineOf(z0)) - 1e9, `it was fitted, narrower: ${z0 && area(outlineOf(z0)) / 1e12} -> ${z1 && area(outlineOf(z1)) / 1e12} mm2`);
    await undo();
    t(snapshot(await state()) === setUp, "one Ctrl+Z gives the wide one back with the other net's track gone");
  });

  await scenario("curved", async (t) => {
    const p = await openTeardrops();
    const curved = [...p.querySelectorAll("label")].find((l) => l.textContent.trim() === "Curved edges")?.querySelector("input");
    t(!!curved, "the page has Curved edges");
    if (curved && !curved.checked) curved.click();
    await sleep(150);
    const s1 = await press(p, "Apply Settings", (s) => s.routing?.teardrop_settings?.curved_edges === true);
    t(s1.routing?.teardrop_settings?.curved_edges === true, "Curved edges is the board's");
    const s2 = await press(p, "Add All Teardrops", (s) => (outlineOf(viaTeardrop(s) ?? { outline: [] }).length > 5));
    const z = viaTeardrop(s2);
    t(!!z && outlineOf(z).length > 5, `the teardrop's sides are curves: ${z ? outlineOf(z).length : 0} points`);
    await closeDialog();
    // Apply Settings is one step, Add All Teardrops two (the page saves its fields first, `set_teardrop_settings`, then `add_all_teardrops`)
    for (let i = 0; i < 3; i++) await undo();
    t(snapshot(await state()) === setUp, "three undos give the straight teardrop and the old settings back");
  });

  // put the board back
  for (let i = 0; i < steps; i++) await undo();
  const end = await state();
  results.push({ name: "the board is as it was found", ok: snapshot(end) === original, notes: [snapshot(end) === original ? "ok: unchanged" : `FAIL: ${snapshot(end).slice(0, 300)} vs ${original.slice(0, 300)}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
