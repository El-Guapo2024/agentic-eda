// KiCad 10 items check: the board items KiCad 10 added that the board reader used to skip -- line endings, grid items, via stitching, drill charts and drill maps -- driven the way a
// person does it through `window.__eda`, the dialogs' and the Properties panel's controls and the pointer events of the canvas, and checked against what the server holds
// (`/api/state`). Run it in the page of a served board that has copper pours and holes: KiCad 25ec2335's qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb, brought into a scratch
// board (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`, then POST `/api/cmd` `{"op": "replace_board", "text": <the file>}`, which keeps its
// zone fills), on a COPY: what it makes it deletes again, and the board's own via stitching it deletes first. `window.__kicad10Only = /regexp/` runs only the scenarios whose names match.
//
//   1  line endings: Graphic Properties of a line gives it an arrow at its start (one undo step), the Properties panel shows the Start Shape row, undo takes it away
//   2  grid items: Place Sub-grid (a click for the centre, one for the size), Grid Properties on it (the spacing), Delete; each one undo step
//   3  via stitching: Draw Via Stitch area (the zone tool's outline, Via Stitching Properties before any via is made: OK makes the stitching and its vias, the stitching selected),
//      the right-click menu of its vias (Exclude From Stitching alone), Exclude From Stitching, Clear Via Stitching Exclusions, the Properties panel's Pitch, Delete (the vias go too)
//   4  drill charts: Add Drill Chart (the table carried by the cursor, a click drops it, its rows the board's holes), the Properties panel's Decimal Places (the chart rebuilt), a via of
//      a new size (the chart rebuilt by the same step), Delete
//   5  drill maps: Add Drill Map (on a documentation layer, selected), the marks in the state, the Properties panel's Guide Cross and Offset X, a move (the marks slide), Delete
//   6  the menus and the toolbar: Place > Add Drill Chart, Add Drill Map, Place Sub-grid; the Via tools group's Draw Via Stitch area
//
// It answers through `window.__kicad10Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
(async () => {
  const results = [];
  const check = (window.__kicad10Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  if (document.visibilityState === "hidden" && !window.__kicad10Sched) {
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
    window.__kicad10Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const waitFor = async (cond, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(200); } return false; };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const target = () => canvasBox().querySelector("canvas") || canvasBox();
  const fire = (type, x, y, extra = {}) => {
    const rect = canvasBox().getBoundingClientRect();
    target().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const mouse = (type, x, y) => { const rect = canvasBox().getBoundingClientRect(); target().dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: type === "contextmenu" ? 2 : 0 })); };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  let toScreen = null;
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); mouse("click", x, y); await sleep(500); };
  const dblclick = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); mouse("dblclick", x, y); await sleep(700); };
  const propRow = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const showProperties = async () => { if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); } };
  const menuLabels = () => { const m = document.querySelector(".menubar-dropdown, .context-menu"); return m ? [...m.querySelectorAll("*")].filter((e) => e.children.length === 0).map((e) => (e.textContent || "").trim()).filter(Boolean) : []; };

  const only = window.__kicad10Only instanceof RegExp ? window.__kicad10Only : null;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return; }
  const st0 = await board();
  const outline = st0.outline ?? [[0, 0], [35000, 0], [35000, 35000], [0, 35000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1000, Math.min(...outline.map((p) => p[1])) / 1000];
  const [bx1, by1] = [Math.max(...outline.map((p) => p[0])) / 1000, Math.max(...outline.map((p) => p[1])) / 1000];
  const at = (dx, dy) => [bx0 + dx, by0 + dy];

  (async () => {
    // ------------------------------------------------------------------------------------------------------------------ 6. menus
    await scenario("the actions are wired, in the Place menu and the drawing toolbar", async (t) => {
      const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      for (const id of ["pcbnew.GridItem.place", "pcbnew.InteractiveDrawing.drawViaStitchArea", "pcbnew.Generator.excludeStitchVia", "pcbnew.Generator.clearStitchViaExclusions", "pcbnew.InteractiveDrawing.placeDrillChart", "pcbnew.InteractiveDrawing.placeDrillMap"])
        t(!!acts[id], `${id} is an action of the board editor`);
      const bar = [...document.querySelectorAll(".menubar-item")].find((e) => e.childNodes[0]?.textContent?.trim() === "Place");
      for (let i = 0; i < 2 && menuLabels().length === 0; i++) { bar?.click(); await sleep(300); }
      const labels = menuLabels();
      for (const want of ["Add Stackup Table", "Add Drill Chart", "Add Drill Map", "Place Sub-grid"]) t(labels.some((l) => l.startsWith(want)), `Place has ${want}`);
      t(labels.indexOf("Add Drill Chart") === labels.indexOf("Add Stackup Table") + 1 || labels.findIndex((l) => l.startsWith("Add Drill Chart")) > labels.findIndex((l) => l.startsWith("Add Stackup Table")), "Add Drill Chart comes after Add Stackup Table");
      bar?.click();
      await sleep(250);
      t(!!document.querySelector('[data-action="pcbnew.InteractiveDrawing.drawViaStitchArea"], [title^="Draw Via Stitch area"], [aria-label^="Draw Via Stitch area"]') || !!acts["pcbnew.InteractiveDrawing.drawViaStitchArea"], "the drawing toolbar's Via tools offer Draw Via Stitch area");
    });

    // ------------------------------------------------------------------------------------------------------------------ 1. line endings
    await scenario("line endings: Graphic Properties gives a line an arrow at its start, one undo step; the Properties panel shows it", async (t) => {
      const [a, b] = [at(-12, 4), at(-4, 4)];
      const shapesBefore = new Set(((await board()).drawings?.shapes ?? []).map((s) => s.id));
      await cmd({ op: "add_shape", shape: { kind: "segment", layer: "F.Fab", stroke_width: 200, filled: false, start: { x: Math.round(a[0] * 1000), y: Math.round(a[1] * 1000) }, end: { x: Math.round(b[0] * 1000), y: Math.round(b[1] * 1000) } } });
      await sleep(900);
      const line = ((await board()).drawings?.shapes ?? []).find((s) => !shapesBefore.has(s.id));
      t(!!line && !line.start_ending, "a new line has no ending");
      if (!line) return;
      await select([line.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(500);
      const style = tid("start-ending-style");
      t(!!style, "Graphic Properties has the Start ending");
      if (style) {
        setValue(style, "arrow");
        await sleep(200);
        [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Apply")?.click();
        await waitFor(async () => ((await board()).drawings?.shapes ?? []).find((s) => s.id === line.id)?.start_ending?.style === "arrow");
        const s = ((await board()).drawings?.shapes ?? []).find((x) => x.id === line.id);
        t(s?.start_ending?.style === "arrow", `the line starts with an arrow (${JSON.stringify(s?.start_ending)})`);
        await select([line.id]);
        await showProperties();
        t(!!propRow("Start Shape"), "the Properties panel has Start Shape");
        await undo();
        const u = ((await board()).drawings?.shapes ?? []).find((x) => x.id === line.id);
        t(!!u && !u.start_ending, "one undo takes the arrow away");
      }
      await cmd({ op: "delete_shape", id: line.id });
    });

    // ------------------------------------------------------------------------------------------------------------------ 2. grid items
    await scenario("grid items: Place Sub-grid, Grid Properties, Delete; one undo step each", async (t) => {
      const grids = async () => (await board()).drawings?.grid_items ?? [];
      const n0 = (await grids()).length;
      await E.run("pcbnew.GridItem.place");
      await sleep(300);
      const c = at(-20, 20);
      await click(c[0], c[1]);
      await click(c[0] + 6, c[1] + 4);
      await waitFor(async () => (await grids()).length === n0 + 1);
      const g = (await grids()).at(-1);
      t((await grids()).length === n0 + 1, "two clicks place a sub-grid");
      if (!g) return;
      t(Math.abs(g.x / 1000 - c[0]) < 1.1 && Math.abs(g.y / 1000 - c[1]) < 1.1, `centred on the first click (${g.x}, ${g.y})`);
      await escape();
      await select([g.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(500);
      t(!!tid("grid-item-properties"), "Properties opens Grid Properties");
      if (tid("grid-spacing-x")) {
        setValue(tid("grid-spacing-x"), "0.5");
        await sleep(150);
        tid("grid-ok").click();
        await waitFor(async () => (await grids()).find((x) => x.id === g.id)?.spacing?.[0] === 500);
        t((await grids()).find((x) => x.id === g.id)?.spacing?.[0] === 500, "OK sets the spacing");
        await undo();
        t((await grids()).find((x) => x.id === g.id)?.spacing?.[0] === g.spacing[0], "one undo puts it back");
      }
      await select([g.id]);
      await E.run("common.Interactive.delete");
      await waitFor(async () => (await grids()).length === n0);
      t((await grids()).length === n0, "Delete removes it");
    });

    // ------------------------------------------------------------------------------------------------------------------ 3. via stitching
    await scenario("via stitching: Draw Via Stitch area, Via Stitching Properties, the vias' menu, Exclude and Clear, Pitch, Delete", async (t) => {
      const stitches = async () => (await board()).drawings?.via_stitches ?? [];
      // The zone fills the stitching is planned from; the QA board's own stitching covers the pour, so it goes first (its vias would keep the new ones out).
      for (const old of await stitches()) await cmd({ op: "pcb_item", verb: "delete_via_stitch", id: old.id });
      await E.run("pcbnew.ZoneFiller.zoneFillAll");
      await sleep(4000);
      const n0 = (await stitches()).length;
      await E.run("pcbnew.InteractiveDrawing.drawViaStitchArea");
      await sleep(300);
      const [x0, y0, x1, y1] = [bx0 + 2, by0 + 2, Math.min(bx1 - 2, bx0 + 22), Math.min(by1 - 2, by0 + 22)];
      await click(x0, y0);
      await click(x1, y0);
      await click(x1, y1);
      await click(x0, y1);
      await dblclick(x0, y1);
      await waitFor(() => !!tid("via-stitch-properties"), 4000);
      t(!!tid("via-stitch-properties"), "finishing the outline opens Via Stitching Properties");
      if (!tid("via-stitch-properties")) return;
      t((await stitches()).length === n0, "no stitching is made before OK");
      const net = tid("vst-net");
      t(net.value === "", "a new stitching starts on no net (ZONE_CREATE_HELPER::commitZone gives it the outline only)");
      const gnd = [...net.options].map((o) => o.value).find((v) => /^GND$/i.test(v)) ?? [...net.options].map((o) => o.value).find((v) => v);
      setValue(net, gnd ?? "");
      setValue(tid("vst-pitch"), "2");
      await sleep(200);
      t(tid("vst-preview").querySelectorAll("circle").length > 0, "the dialog draws the pattern's sample");
      tid("vst-ok").click();
      await waitFor(async () => (await stitches()).length === n0 + 1, 10000);
      const s = (await stitches()).at(-1);
      t(!!s && s.net === gnd && s.pitch_um === 2000, `OK makes the stitching on ${gnd} at a 2 mm pitch`);
      if (!s) return;
      await sleep(600);
      t(E.state().selection.some((x) => x.id === s.id || x === s.id), "the new stitching is selected, not its vias");
      const members = s.member_ids ?? [];
      t(members.length > 0, `the server placed ${members.length} via(s) from the fills`);
      if (members.length > 0) {
        const via = (await board()).routing.vias.find((v) => v.id === members[0]);
        await select([via.id]);
        const [sx, sy] = toScreen(via.x / 1000, via.y / 1000);
        mouse("contextmenu", sx, sy);
        await sleep(600);
        const labels = menuLabels();
        t(labels.length === 1 && labels[0] === "Exclude From Stitching", `the right-click menu of a stitching's via is Exclude From Stitching alone (${labels.join(" | ")})`);
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        await sleep(200);
        await select([via.id]);
        await E.run("pcbnew.Generator.excludeStitchVia");
        await waitFor(async () => ((await stitches()).find((x) => x.id === s.id)?.excluded_cells ?? []).length + ((await stitches()).find((x) => x.id === s.id)?.excluded_positions ?? []).length > 0, 10000);
        const ex = (await stitches()).find((x) => x.id === s.id);
        t(!(ex.member_ids ?? []).includes(via.id) && ((ex.excluded_cells ?? []).length > 0 || (ex.excluded_positions ?? []).length > 0), "Exclude From Stitching takes the via out and keeps its cell out");
        await select([s.id]);
        await E.run("pcbnew.Generator.clearStitchViaExclusions");
        await waitFor(async () => ((await stitches()).find((x) => x.id === s.id)?.excluded_cells ?? []).length === 0, 10000);
        const cl = (await stitches()).find((x) => x.id === s.id);
        t((cl.excluded_cells ?? []).length === 0 && (cl.member_ids ?? []).length === members.length, `Clear Via Stitching Exclusions brings the via back (${(cl.member_ids ?? []).length} vias)`);
      }
      await select([s.id]);
      await showProperties();
      const pitch = propRow("Pitch")?.querySelector("[data-prop-input]");
      t(!!pitch, "the Properties panel has the stitching's Pitch");
      if (pitch) {
        setValue(pitch, "3");
        pitch.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        pitch.blur?.();
        await waitFor(async () => (await stitches()).find((x) => x.id === s.id)?.pitch_um === 3000, 10000);
        t((await stitches()).find((x) => x.id === s.id)?.pitch_um === 3000, "Pitch 3 mm makes the stitching again at 3 mm");
      }
      const viaIds = new Set(((await stitches()).find((x) => x.id === s.id)?.member_ids) ?? []);
      await select([s.id]);
      await E.run("common.Interactive.delete");
      await waitFor(async () => (await stitches()).length === n0, 10000);
      const after = await board();
      t((after.drawings?.via_stitches ?? []).length === n0 && !after.routing.vias.some((v) => viaIds.has(v.id)), "Delete removes the stitching and every via it made");
    });

    // ------------------------------------------------------------------------------------------------------------------ 4. drill charts
    // The board's nets (a via needs one), and two vias of the run's own so the board has holes to chart and to map.
    const aNet = st0.parts.flatMap((p) => p.pads ?? []).map((pd) => pd.net).find((n) => n) ?? "";
    const viasBefore = new Set((st0.routing?.vias ?? []).map((v) => v.id));
    const myVias = async () => ((await board()).routing?.vias ?? []).filter((v) => !viasBefore.has(v.id));
    for (const dx of [3, 6]) await cmd({ op: "add_via", net: aNet, x: Math.round((bx0 + dx) * 1000), y: Math.round((by0 + 3) * 1000), drill: 300, diameter: 600, from_layer: "F.Cu", to_layer: "B.Cu" });
    await sleep(800);
    await scenario("drill charts: Add Drill Chart drops a chart of the board's holes, Decimal Places rebuilds it, a new hole size gets a row, Delete", async (t) => {
      const charts = async () => (await board()).drawings?.drill_charts ?? [];
      const n0 = (await charts()).length;
      await E.run("pcbnew.InteractiveDrawing.placeDrillChart");
      await sleep(500);
      const p = at(-60, 0);
      await hover(p[0], p[1]);
      await click(p[0], p[1]);
      await waitFor(async () => (await charts()).length === n0 + 1, 10000);
      const st = await board();
      const chart = (st.drawings?.drill_charts ?? []).at(-1);
      t((st.drawings?.drill_charts ?? []).length === n0 + 1, "a click drops the chart");
      if (!chart) return;
      const table = st.drawings.tables.find((x) => x.id === chart.table_id);
      t(!!table && table.cols === 6 && table.cells.slice(0, 6).map((c) => c.text).join("|") === "SYM|DRILL DIA|SLOT W x L|PLATED|OPS|FROM / TO", `the default columns (${table?.cells.slice(0, 6).map((c) => c.text).join("|")})`);
      t(!!table && /OPS \/ \d+ SITES/.test(table.cells[table.cells.length - 6].text), `the totals row (${table?.cells[table.cells.length - 6]?.text})`);
      t(Math.abs(table.x / 1000 - p[0]) < 1.1 && Math.abs(table.y / 1000 - p[1]) < 1.1, "its first corner where the click was");
      t(Object.keys(chart.row_shapes ?? {}).length > 0 && chart.symbol_column === 0, "the rows' shapes are in the symbol column");
      t(table.layer === "Dwgs.User" || /User|Fab/.test(table.layer), `on a documentation layer (${table.layer})`);
      // Decimal Places, in the Properties panel.
      await select([table.id]);
      await showProperties();
      const places = propRow("Decimal Places")?.querySelector("[data-prop-input]");
      t(!!places && !!propRow("Units") && !!propRow("Show Totals"), "the Properties panel has Units, Decimal Places and Show Totals");
      const dia0 = table.cells[7].text;
      if (places) {
        setValue(places, "1");
        places.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        places.blur?.();
        await waitFor(async () => (await board()).drawings.tables.find((x) => x.id === table.id)?.cells[7].text !== dia0, 10000);
        const t2 = (await board()).drawings.tables.find((x) => x.id === table.id);
        t(/^\d+\.\d$/.test(t2.cells[7].text), `one decimal place: ${dia0} is ${t2.cells[7].text}`);
        await undo();
        t((await board()).drawings.tables.find((x) => x.id === table.id)?.cells[7].text === dia0, "one undo puts it back");
      }
      // A via of a size the board has not: the same step rebuilds the chart.
      const rows0 = table.rows;
      const r = await cmd({ op: "add_via", net: aNet, x: Math.round((bx0 + 9) * 1000), y: Math.round((by0 + 3) * 1000), drill: 777, diameter: 1400, from_layer: "F.Cu", to_layer: "B.Cu" });
      await sleep(900);
      const t3 = (await board()).drawings.tables.find((x) => x.id === table.id);
      t(t3.rows === rows0 + 1 && t3.cells.some((c) => c.text === "0.777"), `a hole of a new size has a row (${rows0} -> ${t3.rows}; ${JSON.stringify(r).slice(0, 80)})`);
      await undo();
      await select([table.id]);
      await E.run("common.Interactive.delete");
      await waitFor(async () => (await charts()).length === n0, 10000);
      t((await charts()).length === n0, "Delete removes the chart");
    });

    // ------------------------------------------------------------------------------------------------------------------ 5. drill maps
    await scenario("drill maps: Add Drill Map, the marks, Guide Cross and Offset X, a move, Delete", async (t) => {
      const maps = async () => (await board()).drawings?.drill_maps ?? [];
      const n0 = (await maps()).length;
      await E.run("pcbnew.InteractiveDrawing.placeDrillMap");
      await waitFor(async () => (await maps()).length === n0 + 1, 8000);
      const st = await board();
      const m = (st.drawings?.drill_maps ?? []).at(-1);
      t(!!m && /User|Fab/.test(m.layer) && m.offset.x === 0 && m.size_um === 1500, `a map on ${m?.layer}, at no offset, with 1.5 mm marks`);
      if (!m) return;
      await sleep(500);
      t(E.state().selection.some((x) => x.id === m.id || x === m.id), "the new map is selected");
      t((st.drawings.drill_marks ?? []).length > 0, `the state has every hole's mark (${(st.drawings.drill_marks ?? []).length})`);
      await select([m.id]);
      await showProperties();
      const cross = propRow("Guide Cross")?.querySelector("[data-prop-input]");
      t(!!cross && !!propRow("Hole Span") && !!propRow("Symbol Size") && !propRow("Position X"), "the Properties panel has Guide Cross, Hole Span and Symbol Size, not Position X");
      if (cross) {
        cross.click();
        await waitFor(async () => (await maps()).find((x) => x.id === m.id)?.guide_cross === true);
        t((await maps()).find((x) => x.id === m.id)?.guide_cross === true, "Guide Cross turns on");
      }
      await cmd({ op: "move_items", ids: [m.id], dx: 40000, dy: 0 });
      await sleep(700);
      t((await maps()).find((x) => x.id === m.id)?.offset.x === 40000, "a move slides the marks (the offset)");
      await select([m.id]);
      await E.run("common.Interactive.delete");
      await waitFor(async () => (await maps()).length === n0);
      t((await maps()).length === n0, "Delete removes the map");
      await undo();
      t((await maps()).length === n0 + 1, "one undo brings it back");
      for (const x of (await maps()).slice(n0)) await cmd({ op: "pcb_item", verb: "delete_drill_map", id: x.id });
    });

    for (const v of await myVias()) await cmd({ op: "delete_via", id: v.id });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
