// Router check of batch u3 (docs/parity/UI-LEFT.md R2, R5, R7; crates/pns/PARITY.md Stage 23): the pair router going on from two arc track ends with a pair of blind vias and F,
// V's target the nearest ratsnest anchor's layer, V before a route cleared when the tool is entered again, Autoroute giving the active layer back, and the auxiliary toolbar's
// Track Width and Via Size boxes -- driven with synthetic pointer events on the canvas, the router's actions and `window.__eda`, and read from what the studio sends and gets back
// (`/api/route/*` replies are captured while it runs) and from `/api/state`.
//
// The board is diff-pairs.check.js's (tools/run-all-checks.sh `usb-pair`):
//
//   sed -e 's/SDA/USB_DP/' -e 's/SCL/USB_DN/' examples/through_hole_headers.yaml > usb_pair.yaml
//   eda board new usb_pair.yaml -o board
//   eda board place J3 --at 11.5,3 -C board; eda board place J2 --at 6,20 -C board; eda board place J1 --at 7.5,11 -C board
//   eda board place R1 --at 3,16 -C board;   eda board place R2 --at 17,18 -C board
//   eda board track add --net GND --layer F.Cu --width 0.2 --pts "4.4,7 4.4,12" -C board
//
// The script makes the board four-layer (`set_stackup`) and adds what it routes on with `/api/cmd`, and undoes all of it. What it checks:
//   pair arcs   two concentric arc tracks of USB_DP / USB_DN (radii 2 and 2.45 mm, the pair's pitch apart) are a start: the pair leaves their free ends (`DP_PRIMITIVE_PAIR`
//               with `ARC_T`, F4-2); Select Layer and Place Blind/Buried Via picks In1.Cu and arms a pair of blind vias (`dp_via` with `via_type`); a click places them and the
//               pair goes on on In1.Cu; F asks `dp_attempt_finish` (`ROUTER::Finish`), never `dp_finish` at the cursor; the vias are committed as blind vias F.Cu .. In1.Cu;
//   V target    R1 flipped to the bottom, a route from J3.3 on In1.Cu: V asks the nearest ratsnest anchor (`/api/route/ratsnest_anchor`: R1.1, B.Cu) and arms a through via to
//               B.Cu, not to the layer pair's other layer (F.Cu); the fix reply carries the sizes asked again on B.Cu (`updateSizesAfterRouterEvent`);
//   V before    V with the route tool armed and no route starts the next route with a via -- unless the tool was left and entered again (`MainLoop`'s `m_startWithVia = false`);
//   autoroute   Autoroute on the bottom-side R1 with F.Cu active leaves F.Cu active (`RouteSelected`'s `originalLayer`);
//   toolbar     the Track Width and Via Size boxes list KiCad's entries, and "Edit Pre-defined Sizes..." opens Board Setup on that page.
//
// It answers through `window.__routerU3Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=usb-pair
(async () => {
  const results = [];
  const check = (window.__routerU3Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name) }; };

  // ------------------------------------------------------------------------------------------------------------------------------------- what the studio asks of the router
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
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(250); } };

  // -------------------------------------------------------------------------------------------------------------------------------------------------------------- the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : type === "pointermove" ? 0 : 1 }));
  };
  const pointerTo = async (wx, wy) => {
    let at = null;
    for (let i = 0; i < 5; i++) {
      const [x, y] = w2c(wx, wy);
      at = { x, y };
      fire("pointermove", x, y);
      await sleep(150);
      const sn = E.state().snap;
      if (sn && sn.input && Math.hypot(sn.input[0] - wx, sn.input[1] - wy) < 10) return at;
    }
    return E.state().snap ? null : at;
  };
  const moveTo = async (wx, wy, name) => {
    for (let attempt = 0; attempt < 3; attempt++) {
      const n = replies.length;
      if (!(await pointerTo(wx, wy))) continue;
      if (!(await waitReply(name, n, 8000))) continue;
      await quiet();
      return replies.slice(n).filter((q) => q.name === name).pop().body;
    }
    return null;
  };
  const clickAt = async (wx, wy) => {
    const at = await pointerTo(wx, wy);
    if (!at) throw new Error(`the pointer could not be put on ${wx}, ${wy}`);
    fire("pointerdown", at.x, at.y);
    await sleep(40);
    fire("pointerup", at.x, at.y);
    await sleep(200);
  };
  const state = () => json("/api/state");
  const dialog = () => document.querySelector(".dialog");
  const dialogTitle = () => dialog()?.querySelector(".dialog-header")?.textContent ?? null;
  const press = async (label) => { const b = [...(dialog()?.querySelectorAll("button") ?? [])].find((x) => x.textContent.trim() === label); if (b) b.click(); await quiet(); await sleep(400); return !!b; };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const viasOf = (st) => st.routing?.vias ?? [];
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), viasOf(st).map((v) => [v.net, v.x, v.y, v.from, v.to, v.type ?? "through"]), (st.parts ?? []).map((p) => [p.ref, p.side, (p.pads ?? []).map((q) => [q.x, q.y])])]);
  const near = (p, q, tol = 2_000) => !!p && !!q && Math.hypot(p[0] - q[0], p[1] - q[1]) <= tol;
  const settle = async () => {
    const st = await state();
    for (let i = 0; i < 50 && !(E.state().counts.tracks === tracksOf(st).length && E.state().counts.vias === viasOf(st).length); i++) await sleep(150);
    await sleep(900);
  };
  let original = null;
  /** Undo what a scenario made, until the board is as it was (never further). */
  const restore = async () => { for (let i = 0; i < 12 && snapshot(await state()) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(450); } await settle(); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try {
      while (dialog()) { if (!(await press("Cancel")) && !(await press("Close"))) break; }
      await leaveTool();
      await restore();
    } catch { /* ignore */ }
    if (snapshot(await state()) !== original) { entry.notes.push("FAIL: the board is not as it was after the scenario"); entry.ok = false; }
    results.push(entry);
  };
  /** `eda_model::ir::tessellate_arc`, 32 chords: what an arc track's `pts` are. */
  const tessellate = (s, m, e, n = 32) => {
    const [sx, sy] = s, [mx, my] = m, [ex, ey] = e;
    const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    const ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    const uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    const r = Math.hypot(sx - ux, sy - uy), two = Math.PI * 2, norm = (a) => ((a % two) + two) % two;
    const a0 = Math.atan2(sy - uy, sx - ux), a1 = Math.atan2(my - uy, mx - ux), a2 = Math.atan2(ey - uy, ex - ux);
    let sweep = norm(a2 - a0);
    if (norm(a1 - a0) > sweep) sweep -= two;
    const pts = [];
    const ra = (v) => (v < 0 ? -Math.round(-v) : Math.round(v));
    for (let i = 0; i <= n; i++) { const a = a0 + sweep * (i / n); pts.push([ra(ux + r * Math.cos(a)), ra(uy + r * Math.sin(a))]); }
    pts[0] = [sx, sy];
    pts[n] = [ex, ey];
    return pts;
  };
  const xy = (p) => ({ x: p[0], y: p[1] });
  const arcTrack = (net, s, m, e) => ({ net, layer: "F.Cu", width: 200_000, pts: tessellate(s, m, e).map(xy), arc_mid_offset: { x: m[0] - s[0], y: m[1] - s[1] } });

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  for (let i = 0; i < 40 && !(E.state().tab === "pcb" && E.state().counts.footprints > 0); i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  for (let i = 0, last = "", same3 = 0; i < 60 && same3 < 3; i++) {
    const r = canvas().getBoundingClientRect(), v = E.state().view, now = JSON.stringify([r.left, r.top, r.width, r.height, v.x, v.y, v.scale]);
    same3 = now === last ? same3 + 1 : 0;
    last = now;
    await sleep(250);
  }
  const errorsFrom = Date.now();
  const st0 = await state();
  const padOf = {};
  for (const part of st0.parts) for (const q of part.pads ?? []) padOf[q.id] = [q.x, q.y];
  const layersNow = async () => (await state()).layers ?? [];

  // four copper layers, prepreg under the outer ones (`set_stackup`, as router-leftovers.check.js makes them); undone at the end
  const cu = (name) => ({ name, kind: "copper", thickness_mm: 0.035 });
  const di = (name, kind) => ({ name, kind, material: "FR4", thickness_mm: 0.5, epsilon_r: 4.5, loss_tangent: 0.02 });
  const stackup = { layers: [cu("F.Cu"), di("dielectric 1", "prepreg"), cu("In1.Cu"), di("dielectric 2", "core"), cu("In2.Cu"), di("dielectric 3", "prepreg"), cu("B.Cu")] };
  const made = await cmd({ op: "set_stackup", settings: { copper_layers: 4, stackup } });
  for (let i = 0; i < 40 && (await layersNow()).length !== 4; i++) await sleep(250);
  if ((await layersNow()).length !== 4 || !padOf["J3.3"] || !padOf["R1.1"]) {
    results.push({ name: "the scratch board is as the header says, made 4-layer", ok: false, notes: [`FAIL: ${JSON.stringify([made && (made.message ?? made.ok), await layersNow(), !!padOf["J3.3"], !!padOf["R1.1"]])}`] });
    window.fetch = realFetch;
    check.done = true;
    return;
  }
  await sleep(1500);
  original = snapshot(await state());

  await scenario("a pair goes on from two arc ends, places a pair of blind vias, and F finishes it through ROUTER::Finish, never at the cursor", async (t) => {
    // the arcs: about (17, 11), from straight up round to the left, USB_DP inside (2 mm), USB_DN outside (2.45 mm): their free ends at (17, 9) and (17, 8.55)
    const c = [17_000_000, 11_000_000], s45 = Math.SQRT1_2;
    const arc = (net, r) => arcTrack(net, [c[0], c[1] - r], [Math.round(c[0] - r * s45), Math.round(c[1] - r * s45)], [c[0] - r, c[1]]);
    const added = await cmd({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [arc("/USB_DP", 2_000_000), arc("/USB_DN", 2_450_000)], vias: [] });
    t(added && added.ok !== false, `the two arcs are added: ${JSON.stringify(added && (added.message ?? added.ok))}`);
    await settle();
    const arcs = tracksOf(await state()).filter((q) => q.arc_mid && (q.net.endsWith("USB_DP") || q.net.endsWith("USB_DN")));
    t(arcs.length === 2, `they are arc tracks: ${arcs.length}`);
    await E.run("pcbnew.InteractiveRouter.DiffPair");
    await E.run("pcbnew.InteractiveRouter.WalkaroundMode");
    await sleep(200);
    let n = replies.length;
    await clickAt(17_000_000, 9_000_000);
    const s = await waitReply("dp_start", n);
    t(s && s.body.ok === true, `the pair starts on the arcs: ${s && JSON.stringify([s.body.ok, s.body.message, s.body.net_a, s.body.net_b])}`);
    const mv = await moveTo(20_000_000, 6_000_000, "dp_move");
    t(mv && mv.ok && (mv.head_a ?? []).length >= 2 && near(mv.head_a[0], [17_000_000, 9_000_000]) && near(mv.head_b[0], [17_000_000, 8_550_000]), `the heads leave the two arcs' free ends: ${mv && JSON.stringify([mv.head_a && mv.head_a[0], mv.head_b && mv.head_b[0], (mv.head_a ?? []).length])}`);
    // Select Layer and Place Blind/Buried Via: the picker, In1.Cu
    await E.run("pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia");
    await sleep(300);
    t(dialogTitle() === "Select Layer", `the layer picker opens in the pair tool: ${dialogTitle()}`);
    n = replies.length;
    document.querySelector("[data-testid=select-layer-grid] tr[data-layer='In1.Cu']")?.click();
    const via = await waitReply("dp_via", n);
    t(via && via.req.enabled === true && via.req.via_type === "blind" && via.req.to_layer === "In1.Cu", `a pair of blind vias to In1.Cu is armed: ${via && JSON.stringify(via.req)}`);
    await quiet();
    const withVias = await moveTo(20_000_000, 6_000_000, "dp_move");
    const vs = withVias?.vias ?? [];
    t(vs.length === 2 && vs.every((v) => v.from_layer === "F.Cu" && v.to_layer === "In1.Cu"), `the head ends in two vias F.Cu .. In1.Cu: ${JSON.stringify(vs.map((v) => [v.from_layer, v.to_layer]))}`);
    n = replies.length;
    await clickAt(20_000_000, 6_000_000);
    const fx = await waitReply("dp_fix", n);
    t(fx && fx.body.blocked === false && fx.body.preview && fx.body.preview.layer === "In1.Cu", `a click places them and the pair goes on on In1.Cu: ${fx && JSON.stringify([fx.body.blocked, fx.body.preview && fx.body.preview.layer])}`);
    t(fx && fx.body.sizes && fx.body.sizes.clearance > 0, `the fix says the sizes asked again on In1.Cu (updateSizesAfterRouterEvent): ${fx && JSON.stringify(fx.body.sizes)}`);
    await moveTo(21_000_000, 12_000_000, "dp_move");
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.AttemptFinish");
    const af = await waitReply("dp_attempt_finish", n);
    await quiet();
    t(!!af && af.body.ok === true, `F asks the router to finish the pair at the nearest anchor: ${af && JSON.stringify([af.body.ok, af.body.finished, af.body.warp])}`);
    t(!replies.slice(n).some((r) => r.name === "dp_finish"), "and never finishes it at the cursor");
    if (af && af.body.finished !== true) {
      // the pair goes on with its end where the router got it; End (`finishInteractive`) finishes it at the cursor
      t(E.state().tool === "diffpair", "the pair goes on");
      n = replies.length;
      await E.run("common.Interactive.finish");
      await waitReply("dp_finish", n);
    }
    await sleep(800);
    const st = await state();
    const placed = viasOf(st).filter((v) => v.net.endsWith("USB_DP") || v.net.endsWith("USB_DN"));
    t(placed.length === 2 && placed.every((v) => (v.type ?? "through") === "blind" && v.from === "F.Cu" && v.to === "In1.Cu"), `the pair's vias are committed as blind vias F.Cu .. In1.Cu: ${JSON.stringify(placed.map((v) => [v.net, v.type, v.from, v.to]))}`);
    t(tracksOf(st).some((q) => q.net.endsWith("USB_DP") && q.layer === "In1.Cu") && tracksOf(st).some((q) => q.net.endsWith("USB_DN") && q.layer === "In1.Cu"), "with the legs on In1.Cu after them");
  });

  await scenario("V goes to the layer of the nearest ratsnest anchor, and the sizes are asked again on it", async (t) => {
    // R1 to the bottom: D2's other end, R1.1, is on B.Cu; a route from J3.3 on In1.Cu
    const flipped = await cmd({ op: "flip", part: "R1" });
    t(flipped && flipped.ok !== false, `R1 is flipped to the bottom: ${JSON.stringify(flipped && (flipped.message ?? flipped.ok))}`);
    await settle();
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(200);
    await E.run("pcbnew.Control.layerInner1");
    await sleep(200);
    t(E.state().activeLayer === "In1.Cu", `In1.Cu is active: ${E.state().activeLayer}`);
    const j33 = padOf["J3.3"];
    let n = replies.length;
    await clickAt(j33[0], j33[1]);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.layer === "In1.Cu", `the route starts on J3.3 on In1.Cu: ${s && JSON.stringify([s.body.ok, s.body.layer, s.body.net])}`);
    await moveTo(j33[0], j33[1] + 4_000_000, "move");
    n = replies.length;
    await E.run("pcbnew.InteractiveRouter.PlaceVia");
    const anchor = await waitReply("ratsnest_anchor", n);
    const via = await waitReply("via", n);
    t(anchor && anchor.body.found === true && anchor.body.layers && anchor.body.layers[0] === "B.Cu", `the nearest unconnected anchor is R1.1, on B.Cu: ${anchor && JSON.stringify(anchor.body)}`);
    t(via && via.req.enabled === true && via.req.to_layer === "B.Cu" && (via.req.via_type ?? "through") === "through", `V arms a through via to B.Cu, not the layer pair's F.Cu: ${via && JSON.stringify(via.req)}`);
    await quiet();
    n = replies.length;
    await clickAt(j33[0], j33[1] + 4_000_000);
    const fx = await waitReply("fix", n);
    t(fx && fx.body.blocked === false && fx.body.preview && fx.body.preview.layer === "B.Cu", `a click places it and the route goes on on B.Cu: ${fx && JSON.stringify([fx.body.blocked, fx.body.preview && fx.body.preview.layer])}`);
    t(fx && fx.body.sizes && fx.body.sizes.clearance > 0 && typeof fx.body.sizes.clearance_source === "string", `the fix carries the clearance asked again on B.Cu: ${fx && JSON.stringify(fx.body.sizes)}`);
    await sleep(300);
    t(E.state().activeLayer === "B.Cu", `the active layer follows the route: ${E.state().activeLayer}`);
    await leaveTool();
    await sleep(300);
    t(E.state().activeLayer === "In1.Cu", `and comes back to In1.Cu when it ends: ${E.state().activeLayer}`);
  });

  await scenario("V before a route arms its via, unless the router tool was left and entered again (R7-R6)", async (t) => {
    const j35 = padOf["J3.5"] ?? padOf["J3.3"];
    await E.run("pcbnew.Control.layerTop");
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(200);
    await E.run("pcbnew.InteractiveRouter.PlaceVia");
    await sleep(300);
    let n = replies.length;
    await clickAt(j35[0], j35[1]);
    let s = await waitReply("start", n);
    await quiet();
    t(s && s.body.ok === true && replies.slice(n).some((r) => r.name === "via" && r.req.enabled === true), `the route started after V starts with a via armed: ${JSON.stringify(replies.slice(n).map((r) => r.name))}`);
    await leaveTool();
    await E.run("pcbnew.Control.layerTop");
    // V, then out of the tool and back in
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(200);
    await E.run("pcbnew.InteractiveRouter.PlaceVia");
    await sleep(300);
    await E.run("common.Interactive.cancel");
    await sleep(300);
    t(E.state().tool === "select", `out of the router tool: ${E.state().tool}`);
    await E.run("pcbnew.Control.layerTop");
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(200);
    n = replies.length;
    await clickAt(j35[0], j35[1]);
    s = await waitReply("start", n);
    await quiet();
    t(s && s.body.ok === true && !replies.slice(n).some((r) => r.name === "via"), `entering the tool again cleared it: no via armed (${JSON.stringify(replies.slice(n).map((r) => r.name))})`);
  });

  await scenario("Autoroute gives back the layer that was active when it began (R7-R7)", async (t) => {
    await cmd({ op: "flip", part: "R1" });
    await settle();
    await E.run("pcbnew.Control.layerTop");
    await sleep(200);
    // R1 selected, then Autoroute. The scenario is about the layer the run gives back, not about picking: R1 is selected through the selection
    // actions, as the other checks do (what a click on its reference text picks is the selection tool's business, UI-LEFT P22)
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", ["R1"]);
    await sleep(300);
    t(E.state().selection.some((x) => x.id === "R1"), `R1 is selected: ${JSON.stringify(E.state().selection)}`);
    const n = replies.length;
    await E.run("pcbnew.InteractiveRouter.Autoroute");
    await waitReply("start", n);
    await quiet();
    // a connection the router could not finish is left live: Escape ends the run
    for (let i = 0; i < 4 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(300); }
    await sleep(500);
    t(replies.slice(n).some((r) => r.name === "start" && r.body && r.body.layer === "B.Cu"), `the bottom-side R1's connections start on B.Cu: ${JSON.stringify(replies.slice(n).filter((r) => r.name === "start").map((r) => r.body && r.body.layer))}`);
    t(E.state().activeLayer === "F.Cu", `F.Cu is active again after the run: ${E.state().activeLayer}`);
  });

  await scenario("the auxiliary toolbar's Track Width and Via Size boxes are KiCad's choices", async (t) => {
    const box = (c) => document.querySelector(`select[data-control=${c}]`);
    const labels = (c) => [...(box(c)?.options ?? [])].map((o) => o.textContent);
    t(labels("trackWidth")[0] === "Track: use netclass width" && labels("trackWidth").slice(-2).join("|") === "---|Edit Pre-defined Sizes...", `the track width box: ${JSON.stringify(labels("trackWidth"))}`);
    t(labels("viaDiameter")[0] === "Via: use netclass sizes" && labels("viaDiameter").slice(-1)[0] === "Edit Pre-defined Sizes...", `the via size box: ${JSON.stringify(labels("viaDiameter"))}`);
    const el = box("trackWidth");
    if (el) {
      el.value = String(el.options.length - 1);
      el.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(600);
      const page = document.querySelector(".bs-page:not([hidden])")?.getAttribute("aria-label");
      t(!!dialog() && page === "Pre-defined Sizes", `"Edit Pre-defined Sizes..." opens Board Setup on that page: ${dialogTitle()} / ${page}`);
    }
  });

  await scenario("the studio reported no error", async (t) => {
    const errors = E.errors(errorsFrom);
    t(errors.length === 0, `no error: ${JSON.stringify(errors.slice(0, 3))}`);
  });

  // the board back to two layers
  await E.run("common.Interactive.undo");
  for (let i = 0; i < 40 && (await layersNow()).length !== 2; i++) await sleep(250);
  results.push({ name: "the board is 2-layer again", ok: (await layersNow()).length === 2, notes: [JSON.stringify(await layersNow())] });

  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
