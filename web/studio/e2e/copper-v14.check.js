// Copper, round v14 (docs/parity/UI-LEFT-3.md P3-5, P3-32 and P3-6's pair; web/studio/PARITY-pcb.md, the copper section): driven the way a person does it where a
// person sees the difference -- the pair tool and Draw Via on the canvas -- and with `/api/cmd` for the board it sets up, read from `/api/state`, the replies the studio
// gets and what the painter drew (`window.__edaDisplay`).
//
// Run it on a COPY of the usb-pair scratch board (tools/run-all-checks.sh makes it; diff-pairs.check.js says how): it adds a track, a zone, teardrops and vias and
// undoes every one of them, and it ends by checking the board is as it found it.
//
// What it checks:
//   pair outlines  with Display Options' default "Tracks: Show when routing w/ via clearance at end" the pair tool's two heads are drawn with their own clearance
//                  (`ROUTER::movePlacing`'s `Clearance( item, nullptr )` of each line of `m_placer->Traces()`), as a single route's head is; the reply carries
//                  `clearance_a`, `clearance_b`;
//   via info bar   Draw Via where it would break another net's clearance: "Via location violates DRC." stays on screen in the info bar over the canvas (KiCad's
//                  `WX_INFOBAR`, no timer), with a close button that takes it away; refused again, a via put down clean elsewhere dismisses it (`infobar->Dismiss()`);
//   zone teardrop  a pad and its track inside a filled pour of their net get no teardrop from Add All Teardrops while "Prefer zone connection" is on (KiCad's
//                  default, `areItemsInSameZone` on kicad-cli's fills); off, the pad gets its teardrop.
// It answers through `window.__copperV14Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=usb-pair minutes=8
(async () => {
  const results = [];
  const check = (window.__copperV14Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 20000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
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
  const shown = () => window.__edaDisplay ?? { netnames: [], clearances: [], route: [] };
  // the board editor's info bar over the canvas (components/PcbInfoBar.tsx, UI-LEFT-3 P3-32)
  const toast = () => { const bar = document.querySelector('[data-testid="pcb-infobar"]'); return bar && /Via location violates DRC\./.test(bar.textContent ?? "") ? bar : null; };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const viasOf = (st) => st.routing?.vias ?? [];
  const zonesOf = (st) => st.routing?.zones ?? [];
  const teardropsOf = (st) => zonesOf(st).filter((z) => z.teardrop);
  const outlineOf = (z) => (z.outline ?? []).map((p) => (Array.isArray(p) ? p : [p.x, p.y]));
  const inside = (pts, q) => { let c = false; for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) { const [xi, yi] = pts[i], [xj, yj] = pts[j]; if (yi > q[1] !== yj > q[1] && q[0] < ((xj - xi) * (q[1] - yi)) / (yj - yi) + xi) c = !c; } return c; };
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.layer, t.width, t.pts]), viasOf(st).map((v) => [v.id, v.net, v.at ?? [v.x, v.y]]), zonesOf(st).map((z) => [z.id, z.net, z.layer, outlineOf(z)]), st.routing?.teardrop_settings ?? null]);
  const undoUntil = async (target) => { for (let i = 0; i < 12 && snapshot(await state()) !== target; i++) { await E.run("common.Interactive.undo"); await sleep(700); } };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  await until(() => E.state().tab === "pcb" && container() && canvas() && view()?.scale > 0 && E.state().counts.footprints > 0, 30000);
  await E.run("common.Control.zoomFitScreen");
  await sleep(800);
  const st0 = await state();
  const original = snapshot(st0);
  const pads = st0.parts.filter((p) => p.placed).flatMap((p) => (p.pads ?? []).map((q) => ({ ...q, ref: p.ref })));

  await scenario("pair outlines: the pair tool's two heads are drawn with their own clearance", async (t) => {
    // diff-pairs.check.js's start and target: J3.1 (USB_DP), the pair leaving the header straight down, the cursor a long way below
    const p = pads.find((q) => q.id === "J3.1");
    const p2 = pads.find((q) => q.id === "J3.2");
    if (!t(!!p && !!p2 && /USB_DP$/.test(p.net ?? ""), `J3.1 of USB_DP: ${p && p.net}`)) return;
    const target = [Math.round(((p.x + p2.x) / 2 + 120_000) / 1e6) * 1e6, p.y + 11_000_000];
    await E.run("pcbnew.Control.layerTop");
    await E.run("pcbnew.InteractiveRouter.DiffPair");
    await sleep(300);
    let n = replies.length;
    await clickAt(p.x, p.y);
    const s = await waitReply("dp_start", n);
    t(s && s.body.ok === true, `the pair started: ${s && JSON.stringify([s.body.ok, s.body.message])}`);
    n = replies.length;
    const [hx, hy] = w2c(target[0], target[1]);
    fire("pointermove", hx, hy);
    const mv = await waitReply("dp_move", n);
    await sleep(400);
    await redraw();
    const body = mv && mv.body;
    t(body && body.clearance_a > 0 && body.clearance_b > 0, `the reply carries both lines' clearances: ${body && JSON.stringify([body.clearance_a, body.clearance_b])}`);
    t(body && body.head_a.length >= 2 && body.head_b.length >= 2, `the pair has a head: ${body && JSON.stringify([body.head_a, body.head_b, body.colliding])}`);
    const lines = shown().route.filter((x) => x.kind === "track" && x.what === "route");
    const ds = E.state().drawState;
    t(lines.length >= 2, `both heads are outlined: ${JSON.stringify(shown().route)} (draw ${ds && ds.kind}, clearances ${JSON.stringify(ds && ds.clearances)}, heads ${ds && JSON.stringify([ds.ptsA, ds.ptsB])}, mode ${JSON.stringify(E.state().prefs?.trackClearance ?? null)})`);
    t(body && lines.some((x) => x.clearance === body.clearance_a) && lines.some((x) => x.clearance === body.clearance_b), "each at its own clearance");
    await leaveTool();
    t(snapshot(await state()) === original, "nothing was committed");
  });

  await scenario("via info bar: Draw Via's DRC message stays until closed, and a clean via dismisses it", async (t) => {
    const st = await state();
    const nets = [...new Set(pads.map((q) => q.net).filter((x) => !!x))];
    // a whole-millimetre spot well inside the board, away from every pad, and a track of another net 0.52 mm off it (not snapped onto, within the clearance)
    const far = (x, y) => pads.every((q) => Math.hypot(q.x - x, q.y - y) > 3_000_000) && tracksOf(st).every((tr) => tr.pts.every((pt) => Math.hypot(pt[0] - x, pt[1] - y) > 3_000_000));
    let spot = null;
    // inside the parts' box, so inside the board's outline (a via off the board is refused under Strict)
    const xs = pads.map((q) => q.x / 1e6), ys = pads.map((q) => q.y / 1e6);
    const [bx0, bx1, by0, by1] = [Math.ceil(Math.min(...xs)), Math.floor(Math.max(...xs)), Math.ceil(Math.min(...ys)), Math.floor(Math.max(...ys))];
    for (let x = bx0; x <= bx1 && !spot; x++) for (let y = by0 + 3; y <= by1 && !spot; y++) if (far(x * 1e6, y * 1e6) && far(x * 1e6, (y - 3) * 1e6)) spot = [x, y];
    if (!t(!!spot, `a free spot: ${JSON.stringify(spot)}`)) return;
    const [x0, y0] = spot;
    const added = await cmd({ op: "add_track", net: nets[0], layer: "F.Cu", width: 250_000, pts: [{ x: (x0 - 2) * 1e6, y: (y0 + 0.52) * 1e6 }, { x: (x0 + 2) * 1e6, y: (y0 + 0.52) * 1e6 }] });
    t(added.ok, `another net's track beside the spot (${added.message})`);
    await sleep(800);
    const placeVia = async (x, y) => { await E.run("pcbnew.InteractiveDrawing.via"); await sleep(300); await clickAt(x, y); await sleep(600); await leaveTool(); await sleep(200); };
    const cmds = [];
    const ownFetch = window.fetch;
    window.fetch = async (input, init) => { const res = await ownFetch.call(window, input, init); const url = typeof input === "string" ? input : input.url ?? ""; if (/\/api\/cmd/.test(url) && init?.body?.includes("place_via")) cmds.push([init.body.slice(0, 200), await res.clone().text().catch(() => "")]); return res; };
    const viasBefore = viasOf(await state()).length;
    await placeVia(x0 * 1e6, y0 * 1e6);
    t(viasOf(await state()).length === viasBefore, "the via is refused");
    t(!!toast(), "KiCad's message is said");
    t(document.querySelector('[data-testid="pcb-infobar-text"]')?.textContent === "Via location violates DRC.", `in KiCad's words (${document.querySelector('[data-testid="pcb-infobar-text"]')?.textContent})`);
    const bar = toast(), col = document.querySelector(".canvas-col");
    t(!!bar && !!col && Math.abs(bar.getBoundingClientRect().top - col.getBoundingClientRect().top) < 2, "a bar over the canvas, at its top");
    t(!!toast()?.querySelector('[data-testid="pcb-infobar-close"]'), "with a close button, as the info bar has");
    await sleep(7000);
    t(!!toast(), "it is still there after 7 s (an error toast goes after 6 s)");
    toast()?.querySelector('[data-testid="pcb-infobar-close"]')?.click();
    await sleep(300);
    t(!toast(), "the close button takes it away");
    await placeVia(x0 * 1e6, y0 * 1e6);
    t(!!toast(), "refused again: said again");
    await placeVia(x0 * 1e6, (y0 - 3) * 1e6);
    t(viasOf(await state()).length === viasBefore + 1, `a via put down clean elsewhere (${viasOf(await state()).length - viasBefore} placed; ${toast()?.textContent ?? "no message"}; ${JSON.stringify(cmds.slice(-2))})`);
    window.fetch = ownFetch;
    await sleep(300);
    t(!toast(), "and the message is dismissed");
    await undoUntil(original);
    t(snapshot(await state()) === original, "undone, the board is as it was");
  });

  await scenario("zone teardrop: no teardrop on a pad a filled pour of its net joins to its track, while Prefer zone connection is on", async (t) => {
    // a plated pad of a net, the direction with no pad within 3 mm for its track
    const free = (q, dx, dy) => pads.every((o) => o === q || Math.hypot(o.x - (q.x + dx * 3e6), o.y - (q.y + dy * 3e6)) > 2_000_000);
    let pick = null;
    // a round pad: Board Setup's block serves a round one (`TARGET_ROUND`; a rectangle is `TARGET_RECT`'s, off by default)
    for (const q of pads.filter((q) => q.net && (q.drill ?? 0) > 0 && q.shape !== "rect" && q.w === q.h)) {
      for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) if (!pick && free(q, dx, dy)) pick = { q, dx, dy };
      if (pick) break;
    }
    if (!t(!!pick, `a pad with room for a track: ${pick && pick.q.ref + "." + pick.q.num}`)) return;
    const { q, dx, dy } = pick;
    const end = { x: q.x + dx * 3_000_000, y: q.y + dy * 3_000_000 };
    t((await cmd({ op: "add_track", net: q.net, layer: "F.Cu", width: 200_000, pts: [{ x: q.x, y: q.y }, end] })).ok, `a track of ${q.net} leaves the pad`);
    const r = 4_000_000;
    t((await cmd({ op: "add_zone", net: q.net, layer: "F.Cu", outline: [{ x: q.x - r, y: q.y - r }, { x: q.x + r, y: q.y - r }, { x: q.x + r, y: q.y + r }, { x: q.x - r, y: q.y + r }] })).ok, "a pour of its net round both");
    const filled = await post("/api/fill/kicad", {});
    t(filled.ok === true, `kicad-cli filled it (${JSON.stringify([filled.ok, filled.message, filled.zones])})`);
    const td0 = (await state()).routing?.teardrop_settings ?? {};
    t((await cmd({ op: "set_teardrop_settings", settings: { ...td0, enabled: true, prefer_zone_connections: true } })).ok, "Board Setup's teardrops on, Prefer zone connection on");
    t((await cmd({ op: "add_all_teardrops" })).ok, "Add All Teardrops");
    await sleep(600);
    const onPad = (st) => teardropsOf(st).filter((z) => inside(outlineOf(z), [q.x + dx * 400_000, q.y + dy * 400_000]) || inside(outlineOf(z), [q.x, q.y]));
    const withPrefer = teardropsOf(await state()).length;
    t(onPad(await state()).length === 0, `none on the pad the pour joins to its track (${withPrefer} teardrops on the board)`);
    t((await cmd({ op: "set_teardrop_settings", settings: { ...td0, enabled: true, prefer_zone_connections: false } })).ok, "Prefer zone connection off");
    t((await cmd({ op: "add_all_teardrops" })).ok, "Add All Teardrops again");
    await sleep(600);
    const sOff = await state();
    t(teardropsOf(sOff).length === withPrefer + 1 && onPad(sOff).length === 1, `the pad has its teardrop now (${onPad(sOff).length} on it, ${teardropsOf(sOff).length} on the board; settings ${JSON.stringify(sOff.routing?.teardrop_settings)}; pad ${JSON.stringify(q)})`);
    await post("/api/fill/clear", {}).catch(() => null);
    await undoUntil(original);
    t(snapshot(await state()) === original, "undone, the board is as it was");
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
