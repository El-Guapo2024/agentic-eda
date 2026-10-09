// Snap, grid and click-versus-drag check (GAPS.md item 14): drives the studio with synthetic pointer events, the way the other e2e checks do, and reads what the editors
// decided from `window.__eda` -- `state().snap` is the last snapping decision ({ tool, input, output, types, anchored }), `state().grid` and `.gridOverrides` the grid in force.
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board with a routed
// PCB and a schematic that has symbols (the `mcu30` example does, in one of its hierarchical sheets): the pointer only moves inside the page (dispatched events, never the real
// one), every edit it makes it undoes, and it puts the grid overrides, the preferences, the tool and the tab back.
//
// What it checks:
//   click versus drag (`tool_dispatcher.cpp`): with the Linux rule 8 px along an axis is a click, 9 a drag, (6, 6) a click, and a drag stays one when the pointer comes back;
//     with the macOS rule a motion more than 300 ms after the press is a drag, one inside it is not; a drag Escaped before the release commits nothing;
//   the board's grid (the measure tool: every layer, the current grid): the point goes to the grid, Next / Previous Grid change it, Ctrl turns it off (whole um);
//   grid overrides on the board: the graphics category ticked in Edit Grids is the grid of the line tool, and Toggle Grid Overrides switches it off;
//   `BestSnapAnchor`: with Snap to Pads on Always a point near a pad's centre goes to it, Shift turns that off, and the snap line through the pad keeps its y on the way along its
//     row (the hysteresis and the intersections are the unit tests': kicad-port/pcbGridHelper.test.ts -- the snap lines would take over from the anchor in a browser);
//   the schematic's grid list (Next / Previous Grid cycle the four eeschema grids, the painter's dots follow, Edit Grids has the Schematic page and its overrides), the text
//     tool on the text grid (10 mil) while overrides are on and on the current grid when off, a wire's first point on the grid and not pulled to a pin far away, and a move
//     with the grid off (Ctrl; Cmd on a Mac) landing on whole micrometres.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }. The results also pile up in `window.__snapGridCheck.results` (`.done` is set at the end).
(async () => {
  const results = [];
  window.__snapGridCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  const json = (url) => fetch(url).then((r) => r.json());
  const E = window.__eda;
  const state = () => E.state();
  const snap = () => state().snap;
  const near = (a, b, tol = 1) => Array.isArray(a) && Array.isArray(b) && Math.abs(a[0] - b[0]) <= tol && Math.abs(a[1] - b[1]) <= tol;
  const roundTo = (v, g) => Math.round(v / g) * g;
  const isMac = () => /Mac/.test(navigator.platform);
  const errorsBefore = E.errors().length;
  const firstTab = state().tab;

  // ---- the page
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const c2w = (cx, cy) => { const v = state().view, r = canvas().getBoundingClientRect(); return [(cx - r.left - v.x) / v.scale, (cy - r.top - v.y) / v.scale]; };
  /** The part of the canvas a pointer can reach, as a box of the sheet or board (`inset` px in from its edges): the canvas can be wider than the window. */
  const reach = (inset = 40) => { const r = canvas().getBoundingClientRect(); const [x0, y0] = c2w(Math.max(r.left, 0) + inset, Math.max(r.top, 0) + inset); const [x1, y1] = c2w(Math.min(r.right, innerWidth) - inset, Math.min(r.bottom, innerHeight) - inset); return { x0, y0, x1, y1 }; };
  /** One pointer event on whatever is under the point. `mods.ctrl` is the grid-off key of the platform (Cmd on a Mac). */
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: mods.button ?? 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1), ctrlKey: !!mods.ctrl && !isMac(), metaKey: !!mods.ctrl && isMac(), shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const hover = async (wx, wy, mods = {}) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { ...mods, buttons: 0 }); await sleep(60); return snap(); };
  const click = async (wx, wy, mods = {}) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { ...mods, buttons: 0 }); await sleep(60); fire("pointerdown", x, y, mods); await sleep(40); fire("pointerup", x, y, mods); await sleep(120); };
  const key = (k, extra = {}) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...extra }));
  const run = async (id, args) => { await E.run(id, args); await sleep(250); };
  const setTool = async (id, tool) => { if (state().tool !== tool) await run(id); return state().tool === tool; };
  const toSelect = async () => { key("Escape"); await sleep(120); if (state().tool !== "select") { key("Escape"); await sleep(120); } };
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const setNative = (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialog = () => document.querySelector(".dialog");
  const button = (label) => [...(dialog() ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
  const wheel = async (wx, wy, dy, times) => { const [x, y] = w2c(wx, wy); const el = document.elementFromPoint(x, y) ?? canvas(); for (let i = 0; i < times; i++) { el.dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: x, clientY: y, deltaY: dy, deltaMode: 0 })); await sleep(50); } await sleep(200); };
  const board = () => json("/api/state");
  const activityCount = async () => (await board()).activity.length;
  const platformOf = (p) => Object.defineProperty(navigator, "platform", { value: p, configurable: true });
  let padsWas = null;

  try {
    // ============================================================================================================================ click versus drag, on the board
    await clickTab("PCB");
    await toSelect();
    await run("common.Interactive.unselectAll");
    const st0 = await board();
    const target = [...st0.parts].sort((a, b) => b.size[0] * b.size[1] - a.size[0] * a.size[1])[0]; // the biggest footprint: its centre is a sure hit
    const px0 = target.at;
    /** Press on the part, wait `dwell` ms, move by (dx, dy) px in one event, report whether the move tool started; Escape, then release: nothing may be committed. */
    const trial = async (dx, dy, dwell = 0, back = false) => {
      const [x0, y0] = w2c(px0[0], px0[1]);
      fire("pointermove", x0, y0, { buttons: 0 });
      await sleep(40);
      fire("pointerdown", x0, y0);
      await sleep(40 + dwell);
      fire("pointermove", x0 + dx, y0 + dy);
      await sleep(60);
      let started = snap() && snap().tool === "move";
      if (back) { fire("pointermove", x0 + 1, y0); await sleep(60); started = snap() && snap().tool === "move"; }
      key("Escape");
      await sleep(80);
      fire("pointerup", x0 + dx, y0 + dy);
      await sleep(150);
      return !!started;
    };
    const actBefore = await activityCount();
    platformOf("Linux x86_64");
    check("click: 8 px along one axis is a click", (await trial(8, 0)) === false);
    check("drag: 9 px along one axis is a drag", (await trial(9, 0)) === true);
    check("click: (6, 6) -- 8.5 px away -- is a click (the axes are compared, not the length)", (await trial(6, 6)) === false);
    check("drag: 9 px on the other axis is a drag", (await trial(0, -9)) === true);
    check("drag: a drag stays a drag when the pointer comes back to the press", (await trial(14, 0, 0, true)) === true);
    check("Linux: a motion after 350 ms held is not a drag by itself", (await trial(2, 0, 350)) === false);
    platformOf("MacIntel");
    check("macOS: a motion after 350 ms held is a drag", (await trial(2, 0, 350)) === true);
    check("macOS: a motion at once is a click", (await trial(2, 0, 0)) === false);
    delete navigator.platform;
    await toSelect();
    await run("common.Interactive.unselectAll");
    check("an Escaped drag commits nothing", (await activityCount()) === actBefore, `activity ${actBefore} -> ${await activityCount()}`);

    // ============================================================================================================================ the board's grid
    // A spot with nothing within 3 mm of it, where the pointer can go: the nearest pad, via, track, drawing and footprint is far, so only the grid acts.
    const emptySpot = (st) => {
      const segs = [], pts = [];
      for (const t of st.routing.tracks) for (let i = 0; i + 1 < t.pts.length; i++) segs.push([t.pts[i], t.pts[i + 1]]);
      for (const sh of st.drawings.shapes) { if (sh.start && sh.end) segs.push([sh.start, sh.end]); for (const k of ["start", "end", "center", "mid"]) if (sh[k]) pts.push(sh[k]); for (const q of sh.pts ?? []) pts.push(q); }
      for (const p of st.parts) for (const q of p.pads) pts.push([q.x, q.y]);
      for (const v of st.routing.vias) pts.push([v.x, v.y]);
      const dSeg = (p, [a, b]) => { const dx = b[0] - a[0], dy = b[1] - a[1], l = dx * dx + dy * dy || 1; const t = Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l)); return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy)); };
      const { x0, y0, x1, y1 } = reach(40);
      const [bx0, by0] = st.outline[0], [bx1, by1] = st.outline[2];
      let best = null;
      for (let x = Math.max(x0, bx0 + 500); x < Math.min(x1, bx1 - 500) - 800; x += 300) for (let y = Math.max(y0, by0 + 500); y < Math.min(y1, by1 - 500) - 600; y += 300) {
        let d = Infinity;
        for (const q of pts) d = Math.min(d, Math.hypot(q[0] - x, q[1] - y));
        for (const sg of segs) d = Math.min(d, dSeg([x, y], sg));
        for (const part of st0.parts) d = Math.min(d, Math.hypot(part.at[0] - x, part.at[1] - y) - 1500);
        if (!best || d > best.d) best = { x, y, d };
      }
      return best;
    };
    const empty = emptySpot(await board());
    check("the board has an empty spot on screen to probe", !!empty && empty.d > 1500, JSON.stringify(empty));
    const probe = [empty.x + 345, empty.y + 189];
    check("the measure tool is armed", await setTool("common.Interactive.measureTool", "measure"), state().tool);
    const g0 = state().grid;
    let s = await hover(...probe);
    check("board: a point goes to the current grid", s && near(s.output, [roundTo(probe[0], g0), roundTo(probe[1], g0)], 0.5), JSON.stringify(s));
    await run("common.Control.gridNext");
    const g1 = state().grid;
    s = await hover(...probe);
    check("board: Next Grid changes the grid and the snap follows", g1 !== g0 && s && near(s.output, [roundTo(probe[0], g1), roundTo(probe[1], g1)], 0.5), `${g0} -> ${g1}: ${JSON.stringify(s?.output)}`);
    await run("common.Control.gridPrev");
    check("board: Previous Grid goes back", state().grid === g0, `${state().grid}`);
    s = await hover(...probe, { ctrl: true });
    check("board: Ctrl turns the grid off, to whole um", s && near(s.output, probe, 0.5) && s.output.every(Number.isInteger), JSON.stringify(s));

    // grid overrides on the board: the graphics category
    const setGraphicsOverride = async (on) => {
      await run("common.Control.editGrids");
      const box = document.querySelector('input[aria-label="Graphics grid override"]');
      if (!box) { button("Cancel")?.click(); await sleep(200); return null; }
      const was = box.checked;
      if (was !== on) box.click();
      await sleep(80);
      const label = document.querySelector('select[aria-label="Graphics override grid"]').selectedOptions[0].textContent;
      button("OK")?.click();
      await sleep(300);
      return { was, label };
    };
    const overridesWere = state().gridOverrides;
    if (overridesWere === false) await run("common.Control.toggleGridOverrides");
    await toSelect();
    await setTool("pcbnew.InteractiveDrawing.line", "draw_segment");
    const first = await setGraphicsOverride(false);
    check("Edit Grids has a Graphics override row", !!first);
    if (first) {
      s = await hover(...probe);
      check("board: the line tool snaps to the current grid while the graphics override is off", s && near(s.output, [roundTo(probe[0], g0), roundTo(probe[1], g0)], 0.5), JSON.stringify(s?.output));
      const second = await setGraphicsOverride(true);
      const overrideGrid = parseFloat(second.label) * (/^[\d.]+ mil/.test(second.label) ? 25.4 : 1000); // "0.500 mm (19.7 mil)" -> 500 um
      s = await hover(...probe);
      check("board: the graphics override is the grid of the line tool", overrideGrid > 0 && overrideGrid !== g0 && s && near(s.output, [roundTo(probe[0], overrideGrid), roundTo(probe[1], overrideGrid)], 0.5), `override ${second.label}: ${JSON.stringify(s?.output)}`);
      await run("common.Control.toggleGridOverrides");
      s = await hover(...probe);
      check("board: Toggle Grid Overrides puts the line tool back on the current grid", state().gridOverrides === false && s && near(s.output, [roundTo(probe[0], g0), roundTo(probe[1], g0)], 0.5), `${state().gridOverrides} ${JSON.stringify(s?.output)}`);
      await run("common.Control.toggleGridOverrides");
      check("board: and on again", state().gridOverrides === true);
      await setGraphicsOverride(first.was);
    }
    if (overridesWere === false && state().gridOverrides) await run("common.Control.toggleGridOverrides");
    await toSelect();

    // ============================================================================================================================ anchors and snap lines (Snap to Pads: Always)
    {
      const st = await board();
      const { x0, y0, x1, y1 } = reach(60);
      const g = state().grid;
      const all = st.parts.flatMap((p) => p.pads.map((q) => ({ ref: p.ref, num: q.num, at: [q.x, q.y] })));
      const clear = (q) => Math.min(...all.filter((o) => o !== q).map((o) => Math.hypot(o.at[0] - q.at[0], o.at[1] - q.at[1])));
      const pad = all.filter((q) => q.at[0] >= x0 && q.at[0] <= x1 && q.at[1] >= y0 && q.at[1] <= y1 && Math.abs(q.at[0] - roundTo(q.at[0], g)) > 200 && Math.abs(q.at[1] - roundTo(q.at[1], g)) > 200).sort((a, b) => clear(b) - clear(a))[0];
      await run("common.SuiteControl.openPreferences");
      const padsSel = document.querySelector('select[aria-label="Snap to Pads:"]');
      check("Preferences has Snap to Pads (Magnetic Points)", !!padsSel);
      check("a pad off the grid is on screen to snap to", !!pad);
      if (padsSel && pad) {
        padsWas = padsSel.value;
        setNative(padsSel, "always");
        await sleep(80);
        button("OK")?.click();
        await sleep(300);
        await toSelect();
        await setTool("common.Interactive.measureTool", "measure");
        const perPx = 1 / state().view.scale; // um per px
        const nearCentre = [pad.at[0] + 60, pad.at[1] - 40]; // closer to the centre than to any corner (a pad's corners are anchors too)
        await hover(pad.at[0] - 60 * perPx, pad.at[1]); // far: no anchor held
        s = await hover(...nearCentre);
        check(`anchor: near ${pad.ref}.${pad.num} the point goes to the pad's centre`, s && near(s.output, pad.at, 0.5) && s.anchored, `${JSON.stringify(s)} pad ${pad.at}`);
        await hover(pad.at[0] - 60 * perPx, pad.at[1]);
        s = await hover(...nearCentre, { shift: true });
        check("anchor: Shift turns anchor snapping off (the grid point instead)", s && !near(s.output, pad.at, 0.5) && near(s.output, [roundTo(nearCentre[0], g), roundTo(nearCentre[1], g)], 0.5), JSON.stringify(s?.output));
        // the snap line through the pad: out along its row the point keeps the pad's y (off the grid), the x goes to the grid
        const dx = [-3, 3, -4, 4, -2, 2, -5, 5].map((k) => k * g).find((d) => { const x = pad.at[0] + d; return x > x0 && x < x1 && all.every((o) => Math.hypot(o.at[0] - x, o.at[1] - pad.at[1]) > 2000); });
        await hover(pad.at[0] - 60 * perPx, pad.at[1]);
        await hover(...nearCentre);
        s = dx === undefined ? null : await hover(pad.at[0] + dx, pad.at[1] + 7 * perPx);
        check("snap line: along the pad's row the point keeps the pad's y and goes to the grid in x", s && s.output[1] === pad.at[1] && s.output[0] === roundTo(pad.at[0] + dx, g), `${JSON.stringify(s?.output)} pad ${pad.at}`);
        await run("common.SuiteControl.openPreferences");
        const again = document.querySelector('select[aria-label="Snap to Pads:"]');
        if (again) setNative(again, padsWas);
        await sleep(80);
        button("OK")?.click();
        await sleep(300);
        padsWas = null;
      } else if (padsSel) {
        button("Cancel")?.click();
        await sleep(200);
      }
      await toSelect();
    }

    // ============================================================================================================================ the schematic
    await clickTab("Schematic");
    await toSelect();
    const sheetQuery = () => (state().sheetPath.length > 0 ? `?sheet=${state().sheetPath.join("/")}` : "");
    const sch = async () => json(`/api/schematic${sheetQuery()}`);
    let sc = await sch();
    if (sc.symbols.length === 0 && sc.sheets.length > 0) { // a root of sheets: go into the first one
      await run("common.InteractiveSelection.selectItems", [sc.sheets[0].id]);
      await run("eeschema.NavigateTool.enterSheet");
      await sleep(1200);
      sc = await sch();
    }
    check("the schematic has symbols to work on", sc.symbols.length > 0, `${sc.symbols.length} symbols, sheet path "${state().sheetPath.join("/")}"`);
    for (let i = 0; i < 6 && state().view.scale < 0.012; i++) { // zoom in until 1 mm is at least 12 px
      const { x0, y0, x1, y1 } = reach(0);
      await wheel((x0 + x1) / 2, (y0 + y1) / 2, -100, 1);
    }
    check("the schematic is zoomed in", state().view.scale >= 0.012, `scale ${state().view.scale}`);

    // The grid list.
    const list = [];
    const gs = state().grid;
    for (let i = 0; i < 5; i++) { list.push(state().grid); await run("common.Control.gridNext"); }
    check("schematic: Next Grid cycles the four eeschema grids and wraps", new Set(list.slice(0, 4)).size === 4 && list[4] === list[0] && list.slice(0, 4).every((g) => [2540, 1270, 635, 254].includes(g)), list.join(" "));
    const gridTo = async (target) => { for (let i = 0; i < 4 && state().grid !== target; i++) await run("common.Control.gridNext"); return state().grid === target; };
    await gridTo(gs);
    await run("common.Control.gridPrev");
    const prevGrid = state().grid;
    await run("common.Control.gridNext");
    check("schematic: Previous Grid undoes Next", prevGrid !== gs && state().grid === gs, `${gs} -> ${prevGrid} -> ${state().grid}`);

    // The painter's dots follow the grid: a dot is drawn at the grid's points, so at 25 mil there is one between the 50 mil ones (read from the canvas: paper colour is the
    // commonest colour of its middle row, ink is a pixel round the point that is not paper).
    const paperOf = () => { const c = canvas(), d = c.getContext("2d").getImageData(0, Math.floor(c.height / 2), c.width, 1).data, h = new Map(); for (let i = 0; i < d.length; i += 4) { const k = `${d[i]},${d[i + 1]},${d[i + 2]}`; h.set(k, (h.get(k) ?? 0) + 1); } return [...h.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number); };
    const ink = (wx, wy, paper) => {
      const c = canvas(), r = c.getBoundingClientRect(), ctx = c.getContext("2d"), k = c.width / r.width;
      const [cx, cy] = w2c(wx, wy);
      const px = Math.round((cx - r.left) * k), py = Math.round((cy - r.top) * k);
      if (px < 3 || py < 3 || px > c.width - 3 || py > c.height - 3) return null;
      const d = ctx.getImageData(px - 1, py - 1, 3, 3).data;
      let max = 0;
      for (let i = 0; i < d.length; i += 4) max = Math.max(max, Math.abs(d[i] - paper[0]), Math.abs(d[i + 1] - paper[1]), Math.abs(d[i + 2] - paper[2]));
      return max > 30;
    };
    await gridTo(1270);
    await sleep(200);
    {
      const paper = paperOf();
      const { x0, y0, x1, y1 } = reach(60);
      let base = null;
      for (let x = Math.ceil(x0 / 2540) * 2540; x < x1 - 2540 && !base; x += 2540) for (let y = Math.ceil(y0 / 2540) * 2540; y < y1 - 2540; y += 2540)
        if (ink(x, y, paper) && ink(x + 1270, y, paper) && ink(x, y + 1270, paper) && !ink(x + 635, y, paper) && !ink(x, y + 635, paper) && !ink(x + 635, y + 635, paper) && !ink(x + 1905, y + 635, paper)) { base = [x, y]; break; }
      check("schematic: the dots of the 50 mil grid are drawn at its points and none between", !!base, JSON.stringify(base));
      if (base) {
        await gridTo(635);
        await sleep(300);
        const paper2 = paperOf();
        const got = [ink(base[0] + 635, base[1], paper2), ink(base[0], base[1] + 635, paper2), ink(base[0] + 635, base[1] + 635, paper2)];
        check("schematic: the painter's dots follow the grid (25 mil: a dot between the 50 mil ones)", got.every((g) => g === true), got.join(","));
      }
    }
    await gridTo(1270);

    // The text tool: the text grid (10 mil = 254 um) while overrides are on, the current grid when off.
    await toSelect();
    const area = reach(80);
    const ix = Math.floor((area.x0 + (area.x1 - area.x0) * 0.35) / 1270) * 1270 + 600; // 600 past a 50 mil grid line: the 10 mil grid and the 50 mil one disagree here
    const iy = Math.floor((area.y0 + (area.y1 - area.y0) * 0.35) / 1270) * 1270 + 600;
    check("schematic: the text tool is armed", await setTool("eeschema.InteractiveDrawing.placeSchematicText", "sch_text"), state().tool);
    const schOverridesWere = state().gridOverrides;
    if (!schOverridesWere) await run("common.Control.toggleGridOverrides");
    s = await hover(ix, iy);
    check("schematic: the text tool is on the text grid (10 mil) with overrides on", s && near(s.output, [roundTo(ix, 254), roundTo(iy, 254)], 0.5) && !near(s.output, [roundTo(ix, 1270), roundTo(iy, 1270)], 0.5), JSON.stringify(s?.output));
    await run("common.Control.toggleGridOverrides");
    s = await hover(ix, iy);
    check("schematic: with overrides off it is on the current grid", state().gridOverrides === false && s && near(s.output, [roundTo(ix, 1270), roundTo(iy, 1270)], 0.5), JSON.stringify(s?.output));
    if (schOverridesWere) await run("common.Control.toggleGridOverrides");
    await toSelect();

    // Edit Grids: the Schematic page and its overrides.
    await run("common.Control.editGrids");
    const editorSel = document.querySelector('select[aria-label="Editor"]');
    const text = dialog()?.innerText ?? "";
    check("Edit Grids on the schematic opens the Schematic page with its overrides", !!editorSel && /Schematic Editor/.test(editorSel.selectedOptions[0].textContent) && /Grid Overrides/.test(text) && /Wires/.test(text) && !/Vias/.test(text), `${editorSel?.selectedOptions[0]?.textContent}`);
    const on = (label) => document.querySelector(`input[aria-label="${label} grid override"]`)?.checked;
    check("the schematic's overrides start as KiCad's: connected items, wires and text on, graphics off", on("Connected items") === true && on("Wires") === true && on("Text") === true && on("Graphics") === false, [on("Connected items"), on("Wires"), on("Text"), on("Graphics")].join(","));
    button("Cancel")?.click();
    await sleep(250);

    // A wire's first point is on the grid, not pulled to a pin far away.
    sc = await sch();
    const wiresBefore = new Set(sc.wires.map((w) => w.id));
    check("schematic: the wire tool is armed", await setTool("eeschema.InteractiveDrawingLineWireBus.drawWires", "wire"), state().tool);
    const wx = Math.floor((area.x0 + (area.x1 - area.x0) * 0.5) / 1270) * 1270 + 600;
    const wy = Math.floor((area.y0 + (area.y1 - area.y0) * 0.8) / 1270) * 1270 + 600;
    await click(wx, wy);
    await click(wx + 5 * 1270, wy);
    const [ex, ey] = w2c(wx + 5 * 1270, wy);
    document.elementFromPoint(ex, ey).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: ex, clientY: ey, detail: 2 }));
    await sleep(1200);
    const added = (await sch()).wires.filter((w) => !wiresBefore.has(w.id));
    if (added.length > 0) {
      const pts = added[0].pts;
      check("schematic: a wire's first point is the grid point, whatever pins are about", pts.every((p) => p[0] % 1270 === 0 && p[1] % 1270 === 0) && pts[0][0] === roundTo(wx, 1270) && pts[0][1] === roundTo(wy, 1270), JSON.stringify(pts));
      await run("common.Interactive.undo");
      await sleep(600);
      check("the wire's undo leaves the sheet as it was", (await sch()).wires.length === wiresBefore.size);
    } else {
      check("schematic: the wire was committed", false, "no new wire");
    }
    await toSelect();

    // A move with the grid off lands on whole um. A symbol's origin is not always inside its drawing, so find a point that picks it.
    const onScreen = (q) => { const [cx, cy] = w2c(q.at[0], q.at[1]); const r = canvas().getBoundingClientRect(); return cx > r.left + 60 && cx < Math.min(r.right, innerWidth) - 100 && cy > r.top + 60 && cy < r.bottom - 100; };
    const pickPoint = async (q) => {
      for (const [dx, dy] of [[0, 0], [2540, 0], [-2540, 0], [0, 2540], [0, -2540], [1270, 0], [-1270, 0], [0, 1270], [0, -1270], [3810, 0], [-3810, 0], [0, 3810], [0, -3810]]) {
        await click(q.at[0] + dx, q.at[1] + dy);
        if (state().selection.some((e) => e.id === q.id)) { await run("common.Interactive.unselectAll"); return [q.at[0] + dx, q.at[1] + dy]; }
      }
      return null;
    };
    let sym = null, press = null;
    for (const q of sc.symbols.filter((q) => Array.isArray(q.at) && onScreen(q))) { press = await pickPoint(q); if (press) { sym = q; break; } }
    check("a symbol on screen can be picked", !!sym, sym ? `${sym.id} at ${sym.at}, picked at ${press}` : "none");
    if (sym) {
      const [sx0, sy0] = w2c(press[0], press[1]);
      const nBefore = await activityCount();
      fire("pointermove", sx0, sy0, { buttons: 0 });
      await sleep(60);
      fire("pointerdown", sx0, sy0);
      await sleep(60);
      let last = null;
      for (let i = 1; i <= 8; i++) { fire("pointermove", sx0 + (37.3 * i) / 8, sy0 + (21.7 * i) / 8, { ctrl: true }); await sleep(50); last = snap(); }
      fire("pointerup", sx0 + 37.3, sy0 + 21.7, { ctrl: true });
      await sleep(900);
      const after = (await sch()).symbols.find((q) => q.id === sym.id);
      const moved = !!after && (after.at[0] !== sym.at[0] || after.at[1] !== sym.at[1]);
      const act = (await board()).activity[0];
      check("schematic: a move with the grid off lands on whole um and is accepted", moved && last && last.output.every(Number.isInteger) && (await activityCount()) > nBefore, `${sym.id} ${sym.at} -> ${after && after.at}; last snap ${JSON.stringify(last && last.output)}; ${act ? `${act.cmd} => ${act.message}` : ""}`);
      if (moved) { await run("common.Interactive.undo"); await sleep(700); }
      const back = (await sch()).symbols.find((q) => q.id === sym.id);
      check("the move's undo puts the symbol back", !!back && back.at[0] === sym.at[0] && back.at[1] === sym.at[1], `${back && back.at}`);
    }
    await toSelect();
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : String(e));
  } finally {
    try { delete navigator.platform; } catch { /* not ours */ }
    try {
      if (padsWas !== null) { await run("common.SuiteControl.openPreferences"); const sel = document.querySelector('select[aria-label="Snap to Pads:"]'); if (sel) setNative(sel, padsWas); await sleep(80); button("OK")?.click(); await sleep(300); }
      await toSelect();
      await clickTab(firstTab === "schematic" ? "Schematic" : "PCB");
    } catch { /* the tab and the preference are not worth failing for */ }
  }
  check("no console error was raised (a --strict refusal of a probe's move is not one)", E.errors().slice(errorsBefore).every((x) => /refused because --strict/.test(x.message)), JSON.stringify(E.errors().slice(errorsBefore)));
  const failed = results.filter((r) => !r.ok);
  window.__snapGridCheck.done = true;
  return { ok: failed.length === 0, failed: failed.length, results };
})();
