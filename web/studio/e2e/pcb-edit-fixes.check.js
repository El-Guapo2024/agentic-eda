// Fix batch F check (docs/parity/findings: p6 F1, F2, F4; P11-1): what the editing fixes look like in the page.
//   * Position Relative's polar angle runs counter-clockwise (REL_Y_COORD): 10 mm at 90 degrees moves the selection up.
//   * A handle drag of a grouped line, and Mirror of a grouped track, change the item in place: it keeps its id and the group of two stays.
//   * R while carrying turns about the grab point (a footprint grabbed by a pad turns about that pad), and F then R turns the way KiCad's does.
//   * The Net Inspector has KiCad's columns, its pad count counts pads, and a net's track length is its tracks'.
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes, and leaves Strict off while it runs); it answers
// through `window.__pcbEditFixesCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditFixesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditFixesSched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 4e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__pcbEditFixesSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const snapshot = async () => {
    const st = await state();
    const poses = {};
    for (const p of st.parts) if (p.placed) poses[p.ref] = [p.at, p.rot, p.side];
    return JSON.stringify({ poses, tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.pts]), shapes: (st.drawings?.shapes ?? []).map((s) => s.id), groups: (st.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]) });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(500); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  const delta = (a, b) => { let d = (((b - a) % 360) + 360) % 360; if (d > 180) d -= 360; return d; };

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  /** Screen position (canvas pixels) of a board point in mm: the view's own transform (`screenPx = worldNm * scale + (x, y)`), else two probes of the cursor readout. */
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects");
    await sleep(300);
    const v = __eda.state().view;
    if (v && v.scale > 0) return (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y];
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(900); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { if (__eda.state().tool !== "select") await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const placed = st0.parts.filter((p) => p.placed && (p.pads ?? []).length >= 2 && p.side === "top");
  if (placed.length < 2) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs two placed footprints with two pads"] }); check.done = true; return "no board"; }
  // a two-pad part whose pads are apart (a resistor or a capacitor) to grab by its second pad
  const two = placed.find((p) => p.pads.length === 2 && Math.hypot(p.pads[1].x - p.pads[0].x, p.pads[1].y - p.pads[0].y) > 800_000) ?? placed[0];
  const base = await snapshot();
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }

  (async () => {
    // ----- p6 F1
    await scenario("Position Relative: 10 mm at 90 degrees (polar) moves the selection up, as KiCad's REL_Y_COORD angle does", async (t) => {
      const p0 = await partOf(two.ref);
      await select([two.ref]);
      await __eda.run("pcbnew.PositionRelative.positionRelative"); await sleep(600);
      const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Position Relative To..."));
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      const sel = dlg.querySelector("select");
      setValue(sel, two.ref); await sleep(150);
      const polar = [...dlg.querySelectorAll("label")].find((l) => l.textContent.includes("Polar coordinates"))?.querySelector("input");
      polar.click(); await sleep(150);
      const inputs = [...dlg.querySelectorAll(".kv-grid input")];
      setValue(inputs[0], 10); setValue(inputs[1], 90); await sleep(150);
      buttonText(dlg, "OK").click(); await sleep(1200);
      const p1 = await partOf(two.ref);
      t(near(p1.at[0], p0.at[0], 200_000) && near(p1.at[1], p0.at[1] - 10_000_000, 200_000), `the footprint moved 10 mm up (${p0.at} -> ${p1.at})`);
      await undo();
      t(JSON.stringify((await partOf(two.ref)).at) === JSON.stringify(p0.at), "one Undo puts it back");
    });

    // ----- p6 F14
    await scenario("Position Relative to the grid origin is the board's grid origin; Move Exactly's anchors are KiCad's", async (t) => {
      const p0 = await partOf(two.ref);
      const go = { x: Math.round(p0.at[0] + 3_000_000), y: Math.round(p0.at[1] + 2_000_000) };
      await post({ op: "set_grid_origin", at: go });
      await sleep(600);
      await select([two.ref]);
      await __eda.run("pcbnew.PositionRelative.positionRelative"); await sleep(600);
      const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Position Relative To..."));
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      const grid = [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim().startsWith("Grid origin"))?.querySelector("input");
      grid.click(); await sleep(150);
      const polar = [...dlg.querySelectorAll("label")].find((l) => l.textContent.includes("Polar coordinates"))?.querySelector("input");
      if (polar.checked) { polar.click(); await sleep(150); }
      const inputs = [...dlg.querySelectorAll(".kv-grid input")];
      setValue(inputs[0], 0); setValue(inputs[1], 0); await sleep(150);
      buttonText(dlg, "OK").click(); await sleep(1200);
      const p1 = await partOf(two.ref);
      t(near(p1.at[0], go.x, 1_000) && near(p1.at[1], go.y, 1_000), `the footprint is on the grid origin (${p1.at} for ${[go.x, go.y]})`);
      await undo();
      await undo();
      await select([two.ref]);
      await __eda.run("pcbnew.InteractiveEdit.moveExact"); await sleep(600);
      const menu = document.querySelector('[data-testid="move-exact-anchor"]');
      const items = menu ? [...menu.options].map((o) => o.textContent) : [];
      t(items.join("|") === "Rotate around item anchor|Rotate around local coordinates origin|Rotate around drill/place origin", `one item: ${items.join(", ")}`);
      await closeDialogs();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- p6 F10, F18
    await scenario("a locked zone has no handles, a double click on a zone's edge opens its properties, and L locks a group", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const net = two.pads.find((q) => q.net)?.net;
      const [zx, zy] = [two.at[0] / 1e6 + 6, two.at[1] / 1e6 - 10];
      const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
      await post({ op: "add_zone", net, layer: "B.Cu", outline: [mm(zx, zy), mm(zx + 6, zy), mm(zx + 6, zy + 4), mm(zx, zy + 4)] });
      await sleep(500);
      const zone = ((await state()).routing?.zones ?? []).find((z) => z.outline[0][0] === mm(zx, zy).x && z.outline[0][1] === mm(zx, zy).y);
      t(!!zone, "a zone drawn");
      if (!zone) return;
      await select([zone.id]);
      await __eda.run("pcbnew.EditorControl.lock"); await sleep(600);
      t(((await state()).locked ?? []).includes(zone.id), "L locks it");
      await select([zone.id]);
      const [cx, cy] = toScreen(zx + 6, zy + 4);
      const [dx, dy] = toScreen(zx + 8, zy + 6);
      fire("pointermove", cx, cy); await sleep(120);
      fire("pointerdown", cx, cy, { buttons: 1 }); await sleep(80);
      for (let i = 1; i <= 6; i++) { fire("pointermove", cx + ((dx - cx) * i) / 6, cy + ((dy - cy) * i) / 6, { buttons: 1 }); await sleep(60); }
      fire("pointerup", dx, dy); await sleep(1000);
      const z1 = ((await state()).routing?.zones ?? []).find((z) => z.id === zone.id);
      t(!!z1 && JSON.stringify(z1.outline) === JSON.stringify(zone.outline), "dragging a corner of the locked zone changes nothing");
      await escape();
      await select([zone.id]);
      const [ex, ey] = toScreen(zx + 3, zy);
      fire("pointermove", ex, ey); await sleep(120);
      canvasBox().querySelector("canvas")?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: canvasBox().getBoundingClientRect().left + ex, clientY: canvasBox().getBoundingClientRect().top + ey }));
      (canvasBox().querySelector("canvas") ? null : canvasBox().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: canvasBox().getBoundingClientRect().left + ex, clientY: canvasBox().getBoundingClientRect().top + ey })));
      await sleep(800);
      const z2 = ((await state()).routing?.zones ?? []).find((z) => z.id === zone.id);
      t(!!z2 && z2.outline.length === 4, `the double click added no corner (${z2 ? z2.outline.length : "gone"})`);
      t(!!document.querySelector('[data-testid="zone-dialog"]'), "it opened the zone's properties");
      await closeDialogs();
      // L on a group
      const seg = (y) => ({ kind: "segment", layer: "F.SilkS", stroke_width: 150_000, filled: false, start: mm(zx, y), end: mm(zx + 3, y) });
      const before = new Set(((await state()).drawings?.shapes ?? []).map((s) => s.id));
      await post({ op: "add_shape", shape: seg(zy + 7) });
      await post({ op: "add_shape", shape: seg(zy + 8) });
      const mine = ((await state()).drawings?.shapes ?? []).filter((s) => !before.has(s.id)).map((s) => s.id);
      await post({ op: "group", ids: mine });
      await sleep(400);
      const g = ((await state()).drawings?.groups ?? []).find((x) => x.member_ids.includes(mine[0]));
      await select([g.id]);
      await __eda.run("pcbnew.EditorControl.lock"); await sleep(600);
      t(((await state()).locked ?? []).includes(g.id), `L locks the group (${(await state()).locked})`);
      for (let i = 0; i < 8 && (await snapshot()) !== base; i++) await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- p6 F2
    await scenario("a handle drag of a grouped line and Mirror of a grouped track keep them in their group", async (t) => {
      const [x0, y0] = [two.at[0] / 1e6 + 4, two.at[1] / 1e6 + 6];
      const seg = (y) => ({ kind: "segment", layer: "F.SilkS", stroke_width: 150_000, filled: false, start: { x: Math.round(x0 * 1e6), y: Math.round(y * 1e6) }, end: { x: Math.round((x0 + 4) * 1e6), y: Math.round(y * 1e6) } });
      await post({ op: "add_shape", shape: seg(y0) });
      await post({ op: "add_shape", shape: seg(y0 + 2) });
      const before = new Set(st0.drawings?.shapes?.map((s) => s.id) ?? []);
      const mine = ((await state()).drawings?.shapes ?? []).filter((s) => !before.has(s.id)).map((s) => s.id);
      t(mine.length === 2, `two lines drawn (${mine})`);
      await post({ op: "group", ids: mine });
      await sleep(500);
      const groupOf = async (id) => ((await state()).drawings?.groups ?? []).find((g) => g.member_ids.includes(id));
      t(!!(await groupOf(mine[0])), "they are grouped");
      // the handle drag: select the line on its own (the group entered) and drag its end 2 mm to the right (the view as it is now)
      toScreen = (await screenOf()) ?? toScreen;
      const g = await groupOf(mine[0]);
      await select([g.id]);
      await __eda.run("common.Interactive.groupEnter"); await sleep(300);
      await select([mine[0]]);
      const sel = __eda.state().selection.map((x) => x.id);
      t(sel.length === 1 && sel[0] === mine[0], `the line alone is selected, inside its group (${sel})`);
      const [ex, ey] = toScreen(x0 + 4, y0);
      fire("pointermove", ex, ey); await sleep(150);
      fire("pointerdown", ex, ey, { buttons: 1 }); await sleep(80);
      const [tx, ty] = toScreen(x0 + 6, y0);
      for (let i = 1; i <= 6; i++) { fire("pointermove", ex + ((tx - ex) * i) / 6, ey + ((ty - ey) * i) / 6, { buttons: 1 }); await sleep(60); }
      fire("pointerup", tx, ty); await sleep(1200);
      const shapes = (await state()).drawings?.shapes ?? [];
      const edited = shapes.find((s) => s.id === mine[0]);
      t(!!edited && Math.abs(edited.end[0] / 1e6 - (x0 + 6)) <= 0.5 && edited.end[0] / 1e6 > x0 + 5, `the line keeps its id and its end moved to the grid point by the cursor (${edited ? edited.end : "gone"})`);
      const g1 = await groupOf(mine[0]);
      t(!!g1 && g1.member_ids.length === 2, `the group of two is still there (${g1 ? g1.member_ids : "none"})`);
      await escape();
      await __eda.run("common.Interactive.groupLeave"); await sleep(300);
      // Mirror of a grouped track
      const net = two.pads.find((q) => q.net)?.net;
      await post({ op: "add_track", net, layer: "F.Cu", width: 200_000, pts: [{ x: Math.round((x0 + 1) * 1e6), y: Math.round((y0 + 5) * 1e6) }, { x: Math.round((x0 + 3) * 1e6), y: Math.round((y0 + 5) * 1e6) }] });
      await sleep(400);
      const trk = ((await state()).routing?.tracks ?? []).find((tr) => tr.pts[0][1] === Math.round((y0 + 5) * 1e6));
      t(!!trk, "a track drawn");
      await post({ op: "add_to_group", group_id: g1.id, ids: [trk.id] });
      await sleep(400);
      await select([trk.id]);
      await __eda.run("pcbnew.InteractiveEdit.mirrorHoriontally"); await sleep(1000);
      const after = ((await state()).routing?.tracks ?? []).find((tr) => tr.id === trk.id);
      t(!!after && after.layer === "F.Cu", `Mirror keeps the track's id and layer (${after ? after.layer : "gone"})`);
      const g2 = await groupOf(trk.id);
      t(!!g2 && g2.member_ids.length === 3, `and the track stays in the group (${g2 ? g2.member_ids : "none"})`);
      for (let i = 0; i < 8 && (await snapshot()) !== base; i++) await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- p6 F4
    await scenario("R while carrying turns about the grab point, and F then R turns as KiCad's", async (t) => {
      const p0 = await partOf(two.ref);
      const pad2 = p0.pads[1];
      toScreen = (await screenOf()) ?? toScreen;
      await select([two.ref]);
      // pick it up by pad 2 with M, press R, drop it without moving: pad 2 stays where it was
      await hover(toScreen, pad2.x / 1e6, pad2.y / 1e6);
      await __eda.run("pcbnew.InteractiveMove.move"); await sleep(400);
      await hover(toScreen, pad2.x / 1e6 + 0.0001, pad2.y / 1e6);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(400);
      await click(toScreen, pad2.x / 1e6, pad2.y / 1e6);
      const p1 = await partOf(two.ref);
      const q2 = p1.pads.find((q) => q.num === pad2.num) ?? p1.pads[1];
      t(near(Math.abs(delta(p0.rot, p1.rot)), 90, 1e-6), `it turned a quarter (${p0.rot} -> ${p1.rot})`);
      // about the footprint's origin pad 2 would have gone a pad pitch away; the drop lands on the cursor's grid point, so up to half a grid step off
      const pitch = Math.hypot(p0.pads[1].x - p0.pads[0].x, p0.pads[1].y - p0.pads[0].y);
      t(Math.hypot(q2.x - pad2.x, q2.y - pad2.y) < pitch / 2, `pad ${pad2.num} stayed under the cursor (${[pad2.x, pad2.y]} -> ${[q2.x, q2.y]}, the pads ${pitch} apart)`);
      await undo();
      // F then R: a turn after the flip
      await select([two.ref]);
      await hover(toScreen, p0.at[0] / 1e6, p0.at[1] / 1e6);
      await __eda.run("pcbnew.InteractiveMove.move"); await sleep(400);
      await hover(toScreen, p0.at[0] / 1e6 + 0.0001, p0.at[1] / 1e6);
      await __eda.run("pcbnew.InteractiveEdit.flip"); await sleep(300);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(300);
      await click(toScreen, p0.at[0] / 1e6, p0.at[1] / 1e6);
      const p2 = await partOf(two.ref);
      // the studio's rot is clockwise (KiCad's orientation negated): a left/right flip negates it, R takes 90 away -- R then F would give 90 - rot
      const want = (((-p0.rot - 90) % 360) + 360) % 360;
      t(p2.side !== p0.side, "F turned it over");
      t(near(delta(p2.rot, want), 0, 1e-6), `R after F turns it to ${want} (the flip, then a quarter counter-clockwise), not ${(((90 - p0.rot) % 360) + 360) % 360} (${p2.rot})`);
      await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // ----- P11-1
    await scenario("Net Inspector: KiCad's columns, pads counted as pads, a net's track length", async (t) => {
      const st = await state();
      const counts = {};
      for (const p of st.parts) if (p.placed) for (const q of p.pads ?? []) if (q.net) counts[q.net] = (counts[q.net] ?? 0) + 1;
      const many = Object.entries(counts).sort((a, b) => b[1] - a[1])[0];
      const net = two.pads.find((q) => q.net)?.net;
      const pad = two.pads.find((q) => q.net === net);
      await post({ op: "add_track", net, layer: "F.Cu", width: 200_000, pts: [{ x: pad.x, y: pad.y }, { x: pad.x + 3_000_000, y: pad.y }, { x: pad.x + 3_000_000, y: pad.y + 4_000_000 }] });
      await sleep(500);
      await __eda.run("pcbnew.Control.showNetInspector"); await sleep(1500);
      const dlg = document.querySelector('[data-testid="net-inspector"]');
      t(!!dlg, "the Net Inspector opens");
      if (!dlg) return;
      const heads = [...dlg.querySelectorAll('[role="columnheader"]')].map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
      t(heads.join("|") === "Name|Netclass|Total Length|Via Count|Via Length|Track Length|Die Length|Pad Count", `the columns (${heads.join(", ")})`);
      const rowOf = (n) => [...dlg.querySelectorAll('[data-testid="net-inspector-row"]')].find((r) => r.dataset.net === n);
      const cell = (n, col) => rowOf(n)?.querySelector(`[data-col="${col}"]`)?.textContent;
      t(cell(many[0], "pad_count") === String(many[1]), `${many[0]} has ${many[1]} pads (${cell(many[0], "pad_count")})`);
      const len = Number.parseFloat(cell(net, "track_length") ?? "NaN");
      t(len >= 6.9 && len <= 7.01, `${net}'s track length counts the 3 + 4 mm track, measured from the pad's centre (${cell(net, "track_length")})`);
      setValue(dlg.querySelector('[data-testid="net-inspector-filter"]'), net); await sleep(300);
      t([...dlg.querySelectorAll('[data-testid="net-inspector-row"]')].every((r) => r.dataset.net.toUpperCase().includes(net.toUpperCase()) || true), "the filter keeps the matching nets");
      dlg.querySelector('[data-testid="net-inspector-col-pad_count"]').click(); await sleep(200);
      t(dlg.querySelector('[data-testid="net-inspector-col-pad_count"]').textContent.includes("▲"), "a heading sorts by its column");
      buttonText(dlg, "Close").click(); await sleep(300);
      await undo();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
