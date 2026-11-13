// Preferences > PCB Editor > Display Options' annotations and clearance outlines (docs/parity/UI-LEFT-3.md P3-1, P3-6, P3-7; kicad-port/pcbDisplayOptions.ts):
//   net names   with KiCad's default "Net names: Show on pads & tracks" a track's net name is written along it (`renderNetNameForSegment`: centred, 0.55 of its width, along
//               a vertical track bottom to top) once the zoom gives the track 4 mm (`PCB_TRACK::ViewGetLOD`), a via's in its ring (`draw( const PCB_VIA* )`'s netname
//               branch); "Show on pads" drops the tracks' and keeps the vias', "Do not show" drops both;
//   clearance   "Show pad clearance" (on by default) outlines the pads of the active copper layer at their own clearance (`POST /api/display/clearances`); "Tracks: Show
//               always" adds the tracks and vias; the outlines follow the active copper layer (`PCB_EDIT_FRAME::SetActiveLayer`: only its clearance layer is shown);
//   routing     with the default "Show when routing w/ via clearance at end" the route's head is drawn with its clearance (`ROUTER::movePlacing`'s `Clearance( line, nullptr
//               )`), with "Do not show clearances" it is not;
//   dragging    with "Show when routing and editing" a drag's line is drawn with its clearance (`moveDragging` -> `updateView( .., aDragging )`), not with the
//               routing-only modes;
//   reset       the page's Reset sets them back to a fresh PCBNEW_SETTINGS';
//   plating     the state carries KiCad's hole plating thickness, `ADVANCED_CFG::m_HoleWallThickness` 0.020 mm (P3-7).
// It reads what the painter drew through `window.__edaDisplay` ({ netnames, clearances, route }), which the board paint fills. Nothing on the board is edited; the
// preferences it changes are put back to KiCad's defaults at the end.
// `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/pcb-display-options.check.js`; it answers through
// `window.__pcbDisplayCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=8
(async () => {
  const results = [];
  const check = (window.__pcbDisplayCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 20000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const state = () => json("/api/state");

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

  // the page
  const container = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => container().querySelector("canvas");
  const view = () => E.state().view;
  const w2c = (wx, wy) => { const v = view(), r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerdown" ? 1 : 0 }));
  };
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y);
    await sleep(200);
    fire("pointerdown", x, y);
    await sleep(40);
    fire("pointerup", x, y);
    await sleep(300);
  };
  const redraw = async () => { await E.run("common.Control.zoomRedraw").catch(() => null); await sleep(400); };
  /** Zooms about board point (wx, wy) until the canvas draws `pxPerNm` pixels a nanometre (the wheel zooms about the pointer), then puts the point mid-canvas. */
  const zoomTo = async (wx, wy, pxPerNm) => {
    for (let i = 0; i < 80 && view().scale < pxPerNm; i++) {
      const [x, y] = w2c(wx, wy);
      canvas().dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: x, clientY: y, deltaY: -120, deltaMode: 0 }));
      await sleep(60);
    }
    await sleep(300);
  };
  const shown = () => window.__edaDisplay ?? { netnames: [], clearances: [], route: [] };
  const prefsDialog = () => document.querySelector('[data-testid="preferences-dialog"]');
  const buttonIn = (root, label) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label || b.getAttribute("aria-label") === label || (b.getAttribute("title") ?? "").startsWith(label));
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(80); };
  /** Preferences > PCB Editor > Display Options: `fn` on its page, then OK. */
  const displayOptions = async (fn) => {
    await E.run("common.SuiteControl.openPreferences");
    const d = await until(() => prefsDialog(), 4000);
    if (!d) throw new Error("Preferences did not open");
    // the fourth "Display Options" is the PCB Editor's (Symbol, Schematic, Footprint, PCB)
    [...d.querySelectorAll('[role="treeitem"]')].filter((x) => x.textContent.trim() === "Display Options")[3]?.click();
    await sleep(250);
    const page = prefsDialog().querySelector('[data-testid="display-options-pcb"]');
    if (!page) throw new Error("no PCB Display Options page");
    await fn(page, prefsDialog());
    buttonIn(prefsDialog(), "OK")?.click();
    await sleep(400);
    await redraw();
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try {
      while (prefsDialog()) { buttonIn(prefsDialog(), "Cancel")?.click(); await sleep(200); }
      await leaveTool();
    } catch { /* best effort */ }
    results.push(entry);
  };

  await until(() => container() && canvas() && view()?.scale > 0, 30000);
  const st0 = await state();
  const short = (net) => net.slice(net.lastIndexOf("/") + 1);
  // a straight F.Cu segment of a short-named net, long enough for its name, short enough to be in view when zoomed on
  const pick = () => {
    for (const tr of st0.routing.tracks) {
      if (tr.layer !== "F.Cu" || !tr.net || tr.arc_mid) continue;
      for (let i = 0; i + 1 < tr.pts.length; i++) {
        const [a, b] = [tr.pts[i], tr.pts[i + 1]];
        const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
        if ((a[0] === b[0] || a[1] === b[1]) && len >= tr.width * short(tr.net).length * 1.5 && len <= 3_000_000) return { tr, a, b, vertical: a[0] === b[0] };
      }
    }
    return null;
  };
  const seg = pick();
  const via = st0.routing.vias.find((v) => v.net && (v.type ?? null) === null);
  const ZOOM = 30 * ((91 * 1e-9) / 0.0254); // KiCad's zoom 30: a 0.2 mm track gets its name (> 20), a 0.6 mm via too (> 16.7)

  await scenario("the state carries KiCad's hole plating thickness (ADVANCED_CFG::m_HoleWallThickness, 0.020 mm)", async (t) => {
    t(st0.board_rules?.hole_plating_nm === 20000, `board_rules.hole_plating_nm: ${st0.board_rules?.hole_plating_nm}`);
  });

  await scenario("by default a track's net name is written along it and a via's in its ring, at KiCad's sizes, once zoomed in", async (t) => {
    if (!t(!!seg && !!via, `a straight F.Cu segment (${seg && seg.tr.id}) and a through via (${via && via.id}) to look at`)) return;
    await E.run("pcbnew.Control.layerTop");
    await redraw();
    t(!shown().netnames.some((n) => n.kind === "track" && n.id === seg.tr.id), "zoomed out, the track's name is not drawn (under the 4 mm threshold)");
    const mid = [(seg.a[0] + seg.b[0]) / 2, (seg.a[1] + seg.b[1]) / 2];
    await zoomTo(mid[0], mid[1], ZOOM);
    await redraw();
    const names = shown().netnames.filter((n) => n.kind === "track" && n.id === seg.tr.id);
    const near = names.find((n) => Math.hypot(n.x - mid[0], n.y - mid[1]) < 2);
    t(!!near, `the name sits at the segment's middle: ${JSON.stringify(names.map((n) => [n.text, Math.round(n.x), Math.round(n.y)]))} vs ${mid}`);
    t(near && near.text === short(seg.tr.net), `it is the net's short name ${short(seg.tr.net)}: ${near && near.text}`);
    t(near && Math.abs(near.sizeNm - seg.tr.width * 0.55) < 1, `0.55 of the width: ${near && near.sizeNm}`);
    t(near && Math.abs(near.angleRad - (seg.vertical ? -Math.PI / 2 : 0)) < 1e-9, `${seg.vertical ? "a vertical track's reads bottom to top" : "a horizontal track's is level"}: ${near && near.angleRad}`);
    await zoomTo(via.x, via.y, ZOOM);
    const [vx, vy] = w2c(via.x, via.y);
    canvas().dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: vx, clientY: vy, deltaY: -1, deltaMode: 0 }));
    await redraw();
    const v = shown().netnames.find((n) => n.kind === "via" && n.id === via.id);
    t(!!v && v.text === short(via.net), `the via's ring holds its net name: ${JSON.stringify(v)}`);
    // `tsize = min( 1.5 * width / max( chars, 3 ), width ) * 0.75`
    const want = Math.min((1.5 * via.d) / Math.max([...short(via.net)].length, 3), via.d) * 0.75;
    t(v && Math.abs(v.sizeNm - want) < 1, `at 1.5 x its width over the name's length, times 0.75: ${v && v.sizeNm} vs ${want}`);
  });

  await scenario("Net names: Show on pads drops the tracks' names and keeps the vias'; Do not show drops both", async (t) => {
    if (!seg || !via) return t(false, "nothing to look at");
    await displayOptions(async (page) => {
      const sel = page.querySelector('[data-testid="pref-net-names"]');
      t(sel && sel.value === "3" && [...sel.options].map((o) => o.textContent).join("|") === "Do not show|Show on pads|Show on tracks|Show on pads & tracks", `the choice is KiCad's, on pads & tracks: ${sel && sel.value}`);
      await choose(sel, "1");
    });
    t(E.state && !shown().netnames.some((n) => n.kind === "track"), `no track names: ${shown().netnames.filter((n) => n.kind === "track").length}`);
    t(shown().netnames.some((n) => n.kind === "via" && n.id === via.id), "the via keeps its name");
    await displayOptions(async (page) => choose(page.querySelector('[data-testid="pref-net-names"]'), "0"));
    t(shown().netnames.length === 0, `Do not show: nothing written (${shown().netnames.length})`);
    await displayOptions(async (page) => choose(page.querySelector('[data-testid="pref-net-names"]'), "3"));
    t(shown().netnames.some((n) => n.kind === "via"), "and back on pads & tracks");
  });

  await scenario("clearance outlines: pads by default, tracks and vias with Show always, on the active copper layer only", async (t) => {
    await E.run("common.Control.zoomFitScreen").catch(() => null);
    await E.run("pcbnew.Control.layerTop");
    await sleep(800);
    await redraw();
    const own = await json("/api/display/clearances", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ layer: "F.Cu" }) });
    const padsDrawn = await until(() => shown().clearances.filter((c) => c.kind === "pad").length > 0 && shown().clearances, 8000);
    const pads = (padsDrawn || []).filter((c) => c.kind === "pad");
    t(pads.length > 0 && pads.every((c) => c.clearance === own.pads[c.id]), `the pads' outlines at the server's clearances: ${pads.length} (${pads.slice(0, 3).map((c) => `${c.id} ${c.clearance}`).join(", ")})`);
    t(!shown().clearances.some((c) => c.kind === "track" || c.kind === "via"), "the default Tracks mode (while routing w/ via) outlines no board track");
    await displayOptions(async (page) => {
      const sel = page.querySelector('[data-testid="pref-track-clearance"]');
      t(sel && sel.value === "routing_via" && sel.options[sel.selectedIndex].textContent === "Show when routing w/ via clearance at end", `KiCad's default mode: ${sel && sel.value}`);
      t(page.querySelector('[data-testid="pref-pad-clearance"]')?.checked === true, "Show pad clearance on");
      await choose(sel, "always");
    });
    const all = await until(() => shown().clearances.some((c) => c.kind === "track") && shown().clearances, 8000);
    const trk = (all || []).filter((c) => c.kind === "track");
    const fcu = new Set(st0.routing.tracks.filter((q) => q.layer === "F.Cu").map((q) => q.id));
    t(trk.length > 0 && trk.every((c) => fcu.has(c.id) && c.clearance === Math.max(...own.tracks[c.id])), `F.Cu's tracks are outlined at their clearance: ${trk.length} of ${fcu.size}`);
    t((all || []).some((c) => c.kind === "via"), "and the vias");
    await E.run("pcbnew.Control.layerBottom");
    await sleep(800);
    await redraw();
    const bcu = new Set(st0.routing.tracks.filter((q) => q.layer === "B.Cu").map((q) => q.id));
    const back = await until(() => shown().clearances.some((c) => c.kind === "track" && bcu.has(c.id)) && shown().clearances, 8000);
    t(!!back && back.filter((c) => c.kind === "track").every((c) => bcu.has(c.id)), `with B.Cu active, B.Cu's tracks and only those: ${back && back.filter((c) => c.kind === "track").length}`);
    await displayOptions(async (page) => page.querySelector('[data-testid="pref-pad-clearance"]').click());
    t(!shown().clearances.some((c) => c.kind === "pad"), "Show pad clearance off: no pad outline");
    await E.run("pcbnew.Control.layerTop");
  });

  await scenario("routing: the head is drawn with its clearance in the routing modes, not with Do not show clearances", async (t) => {
    await displayOptions(async (page) => {
      await choose(page.querySelector('[data-testid="pref-track-clearance"]'), "routing_via");
      const pc = page.querySelector('[data-testid="pref-pad-clearance"]');
      if (!pc.checked) pc.click();
    });
    const st = await state();
    // a pad of a net on the top side, away from the board edge
    const part = st.parts.find((p) => p.placed && p.side !== "bottom" && (p.pads ?? []).some((q) => q.net && !q.th));
    const pad = part && part.pads.find((q) => q.net && !q.th);
    if (!t(!!pad, `a top pad to route from: ${part && part.ref}`)) return;
    await zoomTo(pad.x, pad.y, ZOOM / 3);
    await E.run("pcbnew.Control.layerTop");
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    let n = replies.length;
    await clickAt(pad.x, pad.y);
    const s = await waitReply("start", n);
    t(s && s.body.ok === true && s.body.clearance > 0, `the start's preview carries the head's clearance: ${s && JSON.stringify([s.body.ok, s.body.clearance, s.body.message])}`);
    // a head to draw: the pointer a little way off the pad
    n = replies.length;
    const [hx, hy] = w2c(pad.x + 3_000_000, pad.y + 3_000_000);
    fire("pointermove", hx, hy);
    await waitReply("move", n);
    await sleep(400);
    await redraw();
    const r = shown().route.find((x) => x.kind === "track");
    const mv = replies.filter((q) => q.name === "move").pop();
    t(mv && (mv.body.head ?? []).length >= 2, `the head has a leg: ${mv && JSON.stringify(mv.body.head)}`);
    t(r && s && r.clearance === s.body.clearance, `the head is drawn with it: ${JSON.stringify(shown().route)}`);
    await leaveTool();
    await displayOptions(async (page) => choose(page.querySelector('[data-testid="pref-track-clearance"]'), "none"));
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    n = replies.length;
    await clickAt(pad.x, pad.y);
    await waitReply("start", n);
    await sleep(400);
    await redraw();
    t(shown().route.length === 0, `Do not show clearances: no outline (${JSON.stringify(shown().route)})`);
    await leaveTool();
  });

  await scenario("dragging: the dragged line is drawn with its clearance with Show when routing and editing, not with the routing-only modes", async (t) => {
    if (!seg) return t(false, "nothing to drag");
    const mid = [(seg.a[0] + seg.b[0]) / 2, (seg.a[1] + seg.b[1]) / 2];
    const off = seg.vertical ? [300_000, 0] : [0, 300_000];
    const drag = async () => {
      await E.run("pcbnew.Control.layerTop");
      await zoomTo(mid[0], mid[1], ZOOM / 3);
      await clickAt(mid[0], mid[1]);
      const [x, y] = w2c(mid[0], mid[1]);
      fire("pointermove", x, y);
      await sleep(150);
      let n = replies.length;
      await E.run("pcbnew.InteractiveRouter.Drag45Degree");
      const s = await waitReply("drag_start", n);
      n = replies.length;
      const [x2, y2] = w2c(mid[0] + off[0], mid[1] + off[1]);
      fire("pointermove", x2, y2);
      const m = await waitReply("drag_move", n);
      await sleep(400);
      await redraw();
      return { s, m };
    };
    await displayOptions(async (page) => choose(page.querySelector('[data-testid="pref-track-clearance"]'), "routing_editing"));
    const { s, m } = await drag();
    t(s && s.body.ok === true, `the drag starts on the segment: ${s && JSON.stringify([s.body.ok, s.body.drag, s.body.message])}`);
    t(m && m.body.clearance > 0, `the drag's reply carries the line's clearance: ${m && m.body.clearance}`);
    const d = shown().route.filter((r) => r.what === "drag");
    t(d.length > 0 && m && d.some((r) => r.kind === "track" && r.clearance === m.body.clearance), `the dragged line is drawn with it: ${JSON.stringify(shown().route)}`);
    await leaveTool();
    await displayOptions(async (page) => choose(page.querySelector('[data-testid="pref-track-clearance"]'), "routing_via"));
    await drag();
    t(!shown().route.some((r) => r.what === "drag"), `while routing w/ via: a drag shows none (\`!aEdit\`): ${JSON.stringify(shown().route)}`);
    await leaveTool();
  });

  await scenario("Reset sets the page back to a fresh PCBNEW_SETTINGS'", async (t) => {
    await displayOptions(async (page, d) => {
      await choose(page.querySelector('[data-testid="pref-net-names"]'), "0");
      const reset = buttonIn(d, "Reset Display Options to Defaults") ?? buttonIn(d, "Reset");
      reset?.click();
      await sleep(200);
      t(page.querySelector('[data-testid="pref-net-names"]').value === "3" && page.querySelector('[data-testid="pref-track-clearance"]').value === "routing_via" && page.querySelector('[data-testid="pref-pad-clearance"]').checked && page.querySelector('[data-testid="pref-pad-numbers"]').checked, `net names on pads & tracks, tracks while routing w/ via, pad clearance and pad numbers on (reset ${!!reset})`);
    });
    const p = JSON.parse(localStorage.getItem(Object.keys(localStorage).find((k) => /pref/i.test(k) && (localStorage.getItem(k) ?? "").includes("pcbNetNames")) ?? "x") ?? "{}");
    t(p.pcbNetNames === 3 && p.pcbTrackClearance === "routing_via" && p.pcbPadClearance === true, `and the defaults are kept: ${JSON.stringify([p.pcbNetNames, p.pcbTrackClearance, p.pcbPadClearance])}`);
  });

  window.fetch = realFetch;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
