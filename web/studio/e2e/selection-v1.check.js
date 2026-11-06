// Selection check, round 2 (UI-LEFT-2 PP2, PP4, RR2, PP19; PARITY-pcb.md section 93): the edit tools' clarification menu, a rounded pad hit by its corner arc,
// Place Footprint's carry on the grid point, D on the selected track, the filter row's flash, Highlight Net through the filter, Select Connection along a copper
// line. Driven through `window.__eda`, the menu, the panel's rows and the canvas's pointer events, and checked against what the page and the server hold.
// Run it in the page of a served board whose parts are placed, on a COPY: what it adds it takes away.
//
// What it checks:
//   1  `RequestSelection` -> `selectPoint` -> `doSelectionMenu`: M with the pointer on two crossing tracks opens the menu ("1  ...", "2  ...", Select All); the row
//      under the pointer brightens its track (the canvas draws it lighter); 2 picks the second, which is the one moved; R's menu dismissed turns nothing; Delete's
//      menu with 1 deletes the first only.
//   2  `ROUTER_TOOL::InlineDrag` drags the selection: D on a selected track whose end lies on a via drags the track (the router is told its id, the drag is not the
//      via's); with nothing selected, D there drags the via (`EDIT_TOOL::Drag`'s filter: two tracks and the via they end on leave the via).
//   3  `PAD::HitTest`: a click in the corner of a rounded pad's box, outside its corner arc, picks the footprint; on the pad, the pad.
//   4  `PlaceFootprint`: the carried footprint's anchor is on the grid point under the cursor, not on the off-grid line end the pointer is on; E does nothing
//      while it is carried; a click with no motion puts it on the click's grid point.
//   5  `HighlightSelectionFilter`: with Tracks off, a click on a track flashes the Tracks row, which fades; Highlight Net under the cursor finds nothing on the
//      filtered track and its net with Tracks on.
//   6  `selectAllConnectedTracks`' `shapeMap`: Select Connection from a track walks a copper line to the track beyond it.
//
// It answers through `window.__selectionV1Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=12
(async () => {
  const results = [];
  const check = (window.__selectionV1Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), results: check.done ? results : results.slice(-2) };
  };
  const S = () => E.state();
  const board = () => json("/api/state");
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const selected = () => S().selection.map((s) => s.id).sort();
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };

  // what the studio asks of the router
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

  // the page: board nanometres to client pixels through the view the hook reports
  const canvas = () => document.querySelector(".pcb-canvas-container canvas") ?? [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = S().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 0), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const hover = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(250); return [x, y]; };
  const click = async (wx, wy, wait = 700) => { const [x, y] = await hover(wx, wy); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(wait); };
  /** A click with no pointer motion before it (the press and the release where the pointer is not reported to be). */
  const stillClick = async (wx, wy, wait = 900) => { const [x, y] = w2c(wx, wy); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(wait); };
  const menuRows = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  const rowText = (el) => el.querySelector("span")?.textContent ?? el.textContent;
  const key = async (k) => { document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(500); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const closeMenus = async () => { if (menuRows().length) { await key("Escape"); } };
  /** The canvas pixel's brightness (0..765) at a board point. */
  const brightnessAt = (wx, wy) => {
    const c = canvas();
    const r = c.getBoundingClientRect();
    const [x, y] = w2c(wx, wy);
    const k = c.width / r.width;
    const d = c.getContext("2d").getImageData(Math.round((x - r.left) * k), Math.round((y - r.top) * k), 1, 1).data;
    return d[0] + d[1] + d[2];
  };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try {
      await closeMenus();
      for (let i = 0; i < 3; i++) { const c = document.querySelector("[data-chooser-cancel]"); if (!c) break; c.click(); await sleep(300); }
      for (let i = 0; i < 3 && (S().tool !== "select" || S().placeFootprint !== "idle" || S().picker); i++) await escape();
      await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null);
    } catch { /* best effort */ }
    results.push(entry);
  };

  const MM = 1e6;
  const net = "GND";
  const addTrack = async (pts, layer = "F.Cu", width = 250_000) => {
    const before = new Set(((await board()).routing?.tracks ?? []).map((t) => t.id));
    await cmd({ op: "add_track", net, layer, width, pts: pts.map(([x, y]) => ({ x: Math.round(x), y: Math.round(y) })) });
    return ((await board()).routing?.tracks ?? []).find((t) => !before.has(t.id))?.id ?? null;
  };
  const made = { tracks: [], vias: [], shapes: [], parts: [] };
  const tracksNow = async () => Object.fromEntries(((await board()).routing?.tracks ?? []).map((t) => [t.id, JSON.stringify(t.pts)]));

  // Strict off for the run (its gates are not what this check is about), put back at the end
  const strictBox = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = !!strictBox?.checked;
  const setStrict = async (on) => { if (strictBox && strictBox.checked !== on) { strictBox.click(); await sleep(400); } };

  for (let i = 0; i < 40 && S().tab !== "pcb"; i++) await sleep(250);
  await setStrict(false);
  await E.run("pcbnew.Control.layerTop");
  await E.run("common.Control.zoomFitObjects");
  await sleep(600);
  // free spots of mcu_board_30plus once placed (its outline is about (1.9, 4.1) to (28.3, 28.5) mm)
  const FREE = [[4.0, 26.5], [26.5, 5.5], [4.0, 6.0], [26.5, 14.5]].map(([x, y]) => [x * MM, y * MM]);

  (async () => {
    // --------------------------------------------------------------------------------------------------------------------------------------------------- 1
    await scenario("M, R and Delete on two crossing tracks ask with the clarification menu; its rows brighten their track; a row's number picks what the tool works on", async (t) => {
      const [cx, cy] = FREE[0];
      const a = await addTrack([[cx - 1.5 * MM, cy], [cx + 1.5 * MM, cy]]);
      const b = await addTrack([[cx, cy - 1.5 * MM], [cx, cy + 1.5 * MM]]);
      if (!t(a && b, `two crossing tracks (${a}, ${b})`)) return;
      made.tracks.push(a, b);
      await sleep(600);
      await select([]);
      await hover(cx, cy);
      await E.run("pcbnew.InteractiveMove.move");
      const rows = await until(() => (menuRows().length >= 3 ? menuRows() : null), 4000);
      if (!t(!!rows, `M opens the clarification menu (${menuRows().map(rowText)})`)) return;
      const texts = rows.map(rowText);
      t(/^1 {2}Track/.test(texts[0]) && /^2 {2}Track/.test(texts[1]), `numbered rows of the two tracks (${texts})`);
      t(texts.includes("Select All") && !texts.some((x) => /Show More/.test(x)), "Select All, and no Show More Choices (nothing was set aside)");
      t(S().tool === "select", `the move has not started yet (${S().tool})`);
      // the first row brightens its track: the canvas draws it lighter, left of the crossing (clear of the menu, which opens to the right and below)
      const firstId = await (async () => { rows[0].dispatchEvent(new MouseEvent("mouseover", { bubbles: true })); await sleep(400); return S().menuHighlight[0]; })();
      t(firstId === a || firstId === b, `the pointer on row 1 brightens one of the tracks (${S().menuHighlight})`);
      const probe = firstId === a ? [cx - 1.0 * MM, cy] : [cx, cy - 1.0 * MM];
      const lit = brightnessAt(...probe);
      rows[0].dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
      await sleep(300);
      // leaving the row unbrightens it (`unhighlight`)
      const unlit = brightnessAt(...probe);
      t(S().menuHighlight.length === 0, "leaving the row unbrightens it");
      t(lit > unlit + 30, `the brightened track is drawn lighter (${lit} against ${unlit}), not boxed`);
      await key("2");
      t(menuRows().length === 0, "2 closes the menu");
      const second = firstId === a ? b : a;
      t(S().tool === "move" && JSON.stringify(selected()) === JSON.stringify([second]), `and the move takes the second row's track (${S().tool}, ${selected()})`);
      const before = await tracksNow();
      await hover(cx + 0.5 * MM, cy + 0.5 * MM);
      await click(cx + 1.0 * MM, cy + 1.0 * MM, 1200);
      const after = await tracksNow();
      t(after[second] !== before[second] && after[firstId] === before[firstId], "the second track moved, the first stayed");
      await undo();
      t((await tracksNow())[second] === before[second], "one undo puts it back");

      // R: the menu, dismissed -- nothing turns (`aSelectionCancelledFlag`)
      await select([]);
      const was = await tracksNow();
      await hover(cx, cy);
      await E.run("pcbnew.InteractiveEdit.rotateCcw");
      t(!!(await until(() => menuRows().length >= 3, 3000)), "R there asks too");
      await key("Escape");
      await sleep(500);
      t(menuRows().length === 0 && JSON.stringify(await tracksNow()) === JSON.stringify(was), "Escape dismisses it and nothing is turned");

      // Delete: 1 deletes the first row's track only
      await hover(cx, cy);
      await E.run("common.Interactive.delete");
      const drows = await until(() => (menuRows().length >= 3 ? menuRows() : null), 3000);
      if (!t(!!drows, "Delete there asks")) return;
      drows[0].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      await sleep(250);
      const gone = S().menuHighlight[0];
      await key("1");
      await sleep(900);
      const left = await tracksNow();
      t(gone && !(gone in left) && [a, b].filter((x) => x !== gone).every((x) => x in left), `1 deletes the first row's track, the other stays (${gone})`);
      await undo();
      t(a in (await tracksNow()) && b in (await tracksNow()), "one undo brings it back");
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 2
    await scenario("D drags the selected track, not the via its end lies on (InlineDrag drags the selection); with nothing selected D there drags the via", async (t) => {
      const [px, py] = FREE[1];
      const vb = new Set(((await board()).routing?.vias ?? []).map((v) => v.id));
      await cmd({ op: "add_via", net, x: px, y: py, drill: 300_000, diameter: 600_000, from_layer: "F.Cu", to_layer: "B.Cu" });
      const via = ((await board()).routing?.vias ?? []).find((v) => !vb.has(v.id))?.id;
      const tr = await addTrack([[px - 2 * MM, py], [px, py]]);
      const tu = await addTrack([[px, py], [px, py + 2 * MM]], "B.Cu");
      if (!t(via && tr && tu, `a via and a track on each side of it (${via}, ${tr}, ${tu})`)) return;
      made.vias.push(via);
      made.tracks.push(tr, tu);
      await sleep(600);
      await select([tr]);
      await hover(px, py);
      let n = replies.length;
      await E.run("pcbnew.InteractiveRouter.Drag45Degree");
      let s = await waitReply("drag_start", n);
      t(s && s.req?.item === tr, `the router is asked to drag the selected track (${s && s.req?.item})`);
      t(s && s.body?.ok === true && s.body.drag && s.body.drag !== "via", `the drag is the track's, not the via's (${s && JSON.stringify([s.body?.ok, s.body?.drag, s.body?.message])})`);
      t(s && Array.isArray(s.body?.start) && Math.hypot(s.body.start[0] - px, s.body.start[1] - py) < 2, `starting on the track's end (${s && JSON.stringify(s.body?.start)})`);
      await escape();
      await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
      await select([]);
      await hover(px, py);
      n = replies.length;
      await E.run("pcbnew.InteractiveRouter.Drag45Degree");
      await sleep(300);
      await closeMenus();
      s = await waitReply("drag_start", n, 6000);
      t(s && s.req?.item === via && s.body?.drag === "via", `with nothing selected, the via (the Drag filter's knee rule) (${s && JSON.stringify([s.req?.item, s.body?.drag])})`);
      await escape();
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 3
    await scenario("A rounded pad is hit only inside its corner arc: the corner of its box picks the footprint, the pad itself the pad", async (t) => {
      const st = await board();
      const part = st.parts.find((p) => p.placed && /^R\d/.test(p.ref) && (p.pads ?? []).some((q) => q.shape === "round_rect" && q.size && (q.ratio ?? 0) > 0.1));
      if (!t(!!part, `a resistor with rounded pads (${part?.ref})`)) return;
      const pad = part.pads.find((q) => q.shape === "round_rect");
      await select([part.ref]);
      await E.run("common.Control.zoomFitSelection");
      await sleep(600);
      await select([]);
      const [w, h] = pad.size;
      const a = ((pad.rot ?? 0) * Math.PI) / 180;
      const at = (u, v) => [pad.x + u * Math.cos(a) - v * Math.sin(a), pad.y + u * Math.sin(a) + v * Math.cos(a)];
      // the corner of the box farthest from the footprint's anchor, 5 um inside both of its edges
      const corners = [[1, 1], [1, -1], [-1, 1], [-1, -1]].map(([su, sv]) => at(su * (w / 2 - 5_000), sv * (h / 2 - 5_000)));
      const corner = corners.sort((p, q) => Math.hypot(q[0] - part.at[0], q[1] - part.at[1]) - Math.hypot(p[0] - part.at[0], p[1] - part.at[1]))[0];
      const r = Math.min(w, h) * pad.ratio;
      t(r > 50_000, `its corners are rounded (${Math.round(r)} nm)`);
      t(1 / S().view.scale < 0.4 * r * (Math.SQRT2 - 1), `zoomed in enough that a pixel is well under the gap (${(1 / S().view.scale).toFixed(0)} nm a pixel)`);
      await click(corner[0], corner[1]);
      t(JSON.stringify(selected()) === JSON.stringify([part.ref]), `the box corner, outside the arc: the footprint (${selected()})`);
      await select([]);
      await click(pad.x, pad.y);
      t(S().selection.length === 1 && S().selection[0].kind === "pad", `the pad's middle: the pad (${JSON.stringify(S().selection)})`);
      await select([]);
      await E.run("common.Control.zoomFitObjects");
      await sleep(500);
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 4
    await scenario("Place Footprint carries the footprint's anchor on the grid point under the cursor (not onto the off-grid line end there); E does nothing; a click with no motion drops it on its grid point", async (t) => {
      const [fx, fy] = FREE[2];
      // a copper line ending well off the grid, where the pointer goes: a move's BestSnapAnchor (graphics are magnetic by default) would put the footprint on it
      const end = [fx + 37_000, fy + 13_000];
      const sb = new Set(((await board()).drawings?.shapes ?? []).map((s) => s.id));
      await cmd({ op: "add_shape", shape: { kind: "segment", layer: "F.Cu", stroke_width: 150_000, filled: false, start: { x: end[0] - 1.5 * MM, y: end[1] }, end: { x: end[0], y: end[1] } } });
      const line = ((await board()).drawings?.shapes ?? []).find((s) => !sb.has(s.id))?.id;
      if (!t(!!line, "a copper line ending off the grid")) return;
      made.shapes.push(line);
      await sleep(500);
      const before = new Set((await board()).parts.map((p) => p.ref));
      await hover(fx - 2 * MM, fy);
      await E.run("pcbnew.EditorControl.placeFootprint");
      const root = await until(() => document.querySelector("[data-footprint-chooser]"), 8000);
      if (!t(!!root, "A opens the chooser")) return;
      const search = root.querySelector("#library-tree-search");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(search, "TestPoint_Pad_D1.0mm");
      search.dispatchEvent(new Event("input", { bubbles: true }));
      const row = await until(() => root.querySelector('[data-chooser-tree] [data-id="TestPoint:TestPoint_Pad_D1.0mm"]'), 90000);
      if (!t(!!row, "the test point is in the chooser")) return;
      row.click();
      await sleep(400);
      root.querySelector("[data-chooser-ok]")?.click();
      if (!t(!!(await until(() => S().placeFootprint === "carrying" && S().tool === "move", 15000)), `it is carried (${S().placeFootprint}, ${S().tool})`)) return;
      const ref = (await board()).parts.find((p) => !before.has(p.ref))?.ref;
      made.parts.push(ref);
      const dialogs = S().dialogs.length;
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(600);
      t(S().dialogs.length === dialogs && S().placeFootprint === "carrying", `E does nothing while it is carried (${S().dialogs})`);
      await hover(end[0] - 0.3 * MM, end[1] + 0.2 * MM);
      await click(end[0], end[1], 1500);
      const grid = S().grid;
      const p = (await board()).parts.find((q) => q.ref === ref);
      const onGrid = (v) => Math.abs(v / grid - Math.round(v / grid)) < 1e-6;
      t(!!p && onGrid(p.at[0]) && onGrid(p.at[1]), `its anchor is on the grid of ${grid} nm (${p?.at})`);
      t(!!p && Math.abs(p.at[0] - end[0]) <= grid / 2 + 1 && Math.abs(p.at[1] - end[1]) <= grid / 2 + 1, "the grid point under the cursor");
      t(!!p && !(p.at[0] === end[0] && p.at[1] === end[1]), `not the line's end the pointer was on (${end})`);
      t(!!(await until(() => S().placeFootprint === "waiting", 4000)), `the tool waits for the next click (${S().placeFootprint})`);
      // the next one, put down by a click the pointer never moved to
      const before2 = new Set((await board()).parts.map((q) => q.ref));
      await click(fx - 2 * MM, fy + 2 * MM);
      const root2 = await until(() => document.querySelector("[data-footprint-chooser]"), 8000);
      if (!t(!!root2, "the next click opens the chooser")) return;
      const s2 = root2.querySelector("#library-tree-search");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(s2, "TestPoint_Pad_D1.0mm");
      s2.dispatchEvent(new Event("input", { bubbles: true }));
      const row2 = await until(() => root2.querySelector('[data-chooser-tree] [data-id="TestPoint:TestPoint_Pad_D1.0mm"]'), 90000);
      row2?.click();
      await sleep(400);
      root2.querySelector("[data-chooser-ok]")?.click();
      await until(() => S().placeFootprint === "carrying" && S().tool === "move", 15000);
      const ref2 = (await board()).parts.find((q) => !before2.has(q.ref))?.ref;
      made.parts.push(ref2);
      const target = [fx + 1.013 * MM, fy + 2.027 * MM];
      await stillClick(target[0], target[1], 1500);
      const p2 = (await board()).parts.find((q) => q.ref === ref2);
      t(!!p2 && onGrid(p2.at[0]) && onGrid(p2.at[1]) && Math.abs(p2.at[0] - target[0]) <= grid / 2 + 1 && Math.abs(p2.at[1] - target[1]) <= grid / 2 + 1, `a click with no motion puts it on that click's grid point (${p2?.at} for ${target})`);
      await escape();
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 5
    await scenario("With Tracks off, a click on a track flashes the Tracks row, which fades; Highlight Net under the cursor goes through the filter", async (t) => {
      const dockTab = (label) => [...document.querySelectorAll(".dock-tab")].find((x) => x.textContent.trim() === label);
      const dockWas = document.querySelector(".dock-tab.active")?.textContent.trim() ?? "Appearance";
      dockTab("Filter")?.click();
      await sleep(400);
      const rowEl = await until(() => document.querySelector('[data-filter-row="tracks"]'), 4000);
      if (!t(!!rowEl, "the Tracks row is on screen")) return;
      const [cx, cy] = FREE[0];
      const probe = [cx - 1.0 * MM, cy];
      rowEl.querySelector("input").click();
      await sleep(300);
      await select([]);
      await click(...probe, 120);
      const flashing = document.querySelector('[data-filter-row="tracks"]')?.getAttribute("data-flash");
      t(Number(flashing) > 0, `the Tracks row flashes (${flashing})`);
      t(!document.querySelector('[data-filter-row="vias"]')?.getAttribute("data-flash"), "no other row does");
      await sleep(900);
      t(!document.querySelector('[data-filter-row="tracks"]')?.getAttribute("data-flash"), "the flash is gone half a second on");
      t(S().selection.length === 0, "nothing is selected");
      await hover(...probe);
      await E.run("pcbnew.EditorControl.highlightNet");
      await sleep(300);
      t(S().highlight.length === 0, `Highlight Net finds nothing on the filtered track (${S().highlight})`);
      document.querySelector('[data-filter-row="tracks"] input').click();
      await sleep(300);
      await hover(...probe);
      await E.run("pcbnew.EditorControl.highlightNet");
      await sleep(300);
      t(JSON.stringify(S().highlight) === JSON.stringify([net]), `with Tracks on, its net (${S().highlight})`);
      await E.run("pcbnew.EditorControl.highlightNet");
      await sleep(200);
      dockTab(dockWas)?.click();
      await sleep(200);
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 6
    await scenario("Select Connection from a track walks a copper line (a board graphic on F.Cu) to the track beyond it", async (t) => {
      const [x, y] = FREE[3];
      const t1 = await addTrack([[x - 2 * MM, y], [x - 1 * MM, y]]);
      const sb = new Set(((await board()).drawings?.shapes ?? []).map((s) => s.id));
      await cmd({ op: "add_shape", shape: { kind: "segment", layer: "F.Cu", stroke_width: 250_000, filled: false, start: { x: x - 1 * MM, y }, end: { x: x + 1 * MM, y } } });
      const line = ((await board()).drawings?.shapes ?? []).find((s) => !sb.has(s.id))?.id;
      const t2 = await addTrack([[x + 1 * MM, y], [x + 2 * MM, y]]);
      if (!t(t1 && line && t2, `a track, a copper line and a track end to end (${t1}, ${line}, ${t2})`)) return;
      made.tracks.push(t1, t2);
      made.shapes.push(line);
      await sleep(600);
      await select([t1]);
      await E.run("pcbnew.InteractiveSelection.SelectConnection");
      await sleep(500);
      const sel = selected();
      t([t1, line, t2].every((id) => sel.includes(id)), `the line and the track beyond it are selected (${sel})`);
      await select([]);
    });

    // ------------------------------------------------------------------------------------------------------------------------------------------- clean up
    for (const ref of made.parts.filter(Boolean)) await cmd({ op: "rip", part: ref });
    for (const id of made.tracks.filter(Boolean)) await cmd({ op: "delete_track", id });
    for (const id of made.vias.filter(Boolean)) await cmd({ op: "delete_via", id });
    for (const id of made.shapes.filter(Boolean)) await cmd({ op: "delete_shape", id });
    await setStrict(strictWas);
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
