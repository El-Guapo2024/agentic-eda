// KiCad 10 items, round u5 (UI-LEFT P18): Drill Chart Properties, Drill Groups, grid items' roles in a route and a footprint move, and Via Stitching's via -- driven the way a
// person does it through `window.__eda`, the dialogs' controls and the pointer events of the canvas, and checked against what the server holds (`/api/state`). Run it in the
// page of a served board with copper pours and holes: KiCad 25ec2335's qa/data/pcbnew/stitch_fp_rulearea.kicad_pcb, brought into a scratch board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`, then POST `/api/cmd` `{"op": "replace_board", "text": <the file>}`, which keeps its zone fills), on a
// COPY: what it makes it deletes again. `window.__kicad10u5Only = /regexp/` runs only the scenarios whose names match.
//
//   1  the actions: Tools > Drill Groups... (pcbnew.InteractiveDrawing.showDrillGroups)
//   2  Drill Chart Properties: a double click on a chart's heading opens it on its General page; Units, Decimal places, the Columns page's Show / Heading / Move Up, Show totals
//      row, Group holes by and the Appearance page's Row separators; OK is one undo step; no column shown is refused with KiCad's words; a double click on a data row selects
//      that row's cells instead
//   3  Drill Groups: the groups with their marks; a group's Mark, Letter and Description committed when another row is selected (the chart shows the letter); Flash on
//      Canvas keeps the selection; Freeze assignments; Close
//   4  a grid item's role in a footprint move: carried into a grid item that affects placement, a footprint turns to the grid's 30 degrees; into one that affects only the cursor,
//      it does not
//   5  a grid item's role in a route: the route tool's point snaps to a grid item that affects the cursor, not to one that affects only routing
//   6  Via Stitching's via: Draw Via Stitch Area, Via Stitching Properties' Configure (Track & Via Properties on the template: the toolbar's via size, ValidateViaParameters'
//      words, the annular rings), OK: the stitching's template and every via it made
//   7  Board Setup > Drill Chart: the profile's Default marks applied (one undo step); Close and Edit Drill Groups... closes Board Setup and opens Drill Groups
//
// It answers through `window.__kicad10u5Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=kicad10-stitch minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__kicad10u5Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  if (document.visibilityState === "hidden" && !window.__kicad10u5Sched) {
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
    window.__kicad10u5Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const waitFor = async (cond, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(200); } return false; };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel" || x.textContent.trim() === "Close"); if (!b) break; b.click(); await sleep(250); } };
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const target = () => canvasBox().querySelector("canvas") || canvasBox();
  const fire = (type, x, y, extra = {}) => {
    const rect = canvasBox().getBoundingClientRect();
    target().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const mouse = (type, x, y) => { const rect = canvasBox().getBoundingClientRect(); target().dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: type === "contextmenu" ? 2 : 0 })); };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  let toScreen = null;
  const calibrate = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); mouse("click", x, y); await sleep(500); };
  const dblclick = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); mouse("dblclick", x, y); await sleep(800); };
  const menuLabels = () => { const m = document.querySelector(".menubar-dropdown, .context-menu"); return m ? [...m.querySelectorAll("*")].filter((e) => e.children.length === 0).map((e) => (e.textContent || "").trim()).filter(Boolean) : []; };
  const selIds = () => E.state().selection.map((x) => (typeof x === "string" ? x : x.id));

  const only = window.__kicad10u5Only instanceof RegExp ? window.__kicad10u5Only : null;
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
  toScreen = await calibrate();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return; }
  const st0 = await board();
  const outline = st0.outline ?? [[0, 0], [35000000, 0], [35000000, 35000000], [0, 35000000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1e6, Math.min(...outline.map((p) => p[1])) / 1e6];
  const [bx1, by1] = [Math.max(...outline.map((p) => p[0])) / 1e6, Math.max(...outline.map((p) => p[1])) / 1e6];
  const at = (dx, dy) => [bx0 + dx, by0 + dy];
  const aNet = st0.parts.flatMap((p) => p.pads ?? []).map((pd) => pd.net).find((n) => n) ?? "";
  const viasBefore = new Set((st0.routing?.vias ?? []).map((v) => v.id));

  (async () => {
    // ------------------------------------------------------------------------------------------------------------------ 1. the actions
    await scenario("Tools > Drill Groups... is wired", async (t) => {
      const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      t(!!acts["pcbnew.InteractiveDrawing.showDrillGroups"] && acts["pcbnew.InteractiveDrawing.showDrillGroups"].reason !== "no handler", "pcbnew.InteractiveDrawing.showDrillGroups has a handler");
      const bar = [...document.querySelectorAll(".menubar-item")].find((e) => e.childNodes[0]?.textContent?.trim() === "Tools");
      for (let i = 0; i < 2 && menuLabels().length === 0; i++) { bar?.click(); await sleep(300); }
      const labels = menuLabels();
      t(labels.some((l) => l.startsWith("Drill Groups")), `Tools has Drill Groups... (${labels.slice(0, 12).join(" | ")})`);
      bar?.click();
      await sleep(250);
    });

    // A chart of the board's holes, two vias of the run's own (0.3 and 0.4 mm) so it has rows.
    for (const [dx, drill] of [[3, 300_000], [6, 400_000]]) await cmd({ op: "add_via", net: aNet, x: Math.round((bx0 + dx) * 1e6), y: Math.round((by0 + 3) * 1e6), drill, diameter: drill + 300_000, from_layer: "F.Cu", to_layer: "B.Cu" });
    await sleep(800);
    const chartAt = at(-70, 0);
    await cmd({ op: "pcb_item", verb: "place_drill_chart", layer: "Dwgs.User", at: { x: Math.round(chartAt[0] * 1e6), y: Math.round(chartAt[1] * 1e6) } });
    await sleep(900);
    const chartOf = async () => {
      const st = await board();
      const chart = (st.drawings?.drill_charts ?? []).at(-1);
      return { st, chart, table: chart ? st.drawings.tables.find((x) => x.id === chart.table_id) : null };
    };
    const centreOf = (cell) => [cell.corners.reduce((a, p) => a + p[0], 0) / 4e6, cell.corners.reduce((a, p) => a + p[1], 0) / 4e6];
    toScreen = await calibrate();

    // ------------------------------------------------------------------------------------------------------------------ 2. Drill Chart Properties
    await scenario("Drill Chart Properties: a double click on the heading opens it; its pages set the chart, one undo step; a data row's double click selects the row", async (t) => {
      const { chart, table } = await chartOf();
      t(!!chart && !!table, "a chart is on the board");
      if (!chart || !table) return;
      await select([]);
      const head = table.cells.find((c) => c.row === 0 && c.col === 1);
      await dblclick(...centreOf(head));
      await waitFor(() => !!tid("drill-chart-properties"), 4000);
      t(!!tid("drill-chart-properties"), "a double click on the heading opens Drill Chart Properties, not Table Properties");
      if (!tid("drill-chart-properties")) return;
      t(!!tid("dcp-layer") && tid("dcp-layer").value === "Dwgs.User", "on its General page, the layer the chart is on");
      t([...tid("dcp-layer").options].every((o) => /User|Fab/.test(o.value)), "only documentation layers are offered");
      setValue(tid("dcp-units"), "mils");
      setValue(tid("dcp-precision"), "1");
      tid("dcp-page-columns").click();
      await sleep(200);
      const rows = () => [...tid("dcp-columns").querySelectorAll("tbody tr")].map((r) => r.getAttribute("data-testid").replace("dcp-col-", ""));
      t(rows().slice(0, 7).join(",") === "symbol,drill_diameter,slot_size,plating,operation_count,layer_span,site_count", `the chart's columns, then the schema's others (${rows().join(",")})`);
      tid("dcp-show-slot_size").click();
      tid("dcp-show-site_count").click();
      setValue(tid("dcp-heading-drill_diameter"), "DIA");
      await sleep(100);
      tid("dcp-col-drill_diameter").click();
      tid("dcp-up").click();
      await sleep(150);
      t(rows()[0] === "drill_diameter", "Move Up puts the drill diameter first");
      tid("dcp-totals").click();
      tid("dcp-group-span").click();
      tid("dcp-page-appearance").click();
      await sleep(150);
      tid("dcp-rows").click();
      await sleep(100);
      tid("dcp-ok").click();
      await waitFor(async () => (await chartOf()).chart?.units === "mils", 10000);
      const after = await chartOf();
      const ids = after.chart.columns.map((c) => c.id);
      t(after.chart.units === "mils" && after.chart.precision === 1, `the units and decimal places (${after.chart.units}, ${after.chart.precision})`);
      t(ids.join(",") === "drill_diameter,symbol,plating,operation_count,layer_span,site_count", `the shown columns in the page's order (${ids.join(",")})`);
      t(after.chart.columns[0].heading === "DIA" && after.table.cells[0].text === "DIA", "the heading typed");
      t(after.chart.show_totals === false && !after.table.cells.some((c) => /OPS \//.test(c.text)), "no totals row");
      t(!(after.st.drawings.drill_profile?.group_by ?? []).includes("span"), "the board's grouping without the layer span");
      t(after.table.separators.rows === false, "no row separators");
      await undo();
      const back = await chartOf();
      t(back.chart.units === (chart.units ?? "mm") && back.chart.columns.map((c) => c.id).join(",") === chart.columns.map((c) => c.id).join(","), "one undo puts the chart back");
      // No column shown: KiCad's message, the dialog stays open.
      await select([back.table.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await waitFor(() => !!tid("drill-chart-properties"), 4000);
      tid("dcp-page-columns").click();
      await sleep(150);
      for (const b of [...tid("dcp-columns").querySelectorAll('input[type="checkbox"]')]) if (b.checked) b.click();
      await sleep(100);
      tid("dcp-ok").click();
      await sleep(400);
      t(tid("dcp-error")?.textContent === "A table needs at least one column.", `refused: ${tid("dcp-error")?.textContent}`);
      tid("dcp-cancel").click();
      await sleep(300);
      // A double click on a data row: that row's cells.
      await select([back.table.id]);
      const data = back.table.cells.find((c) => c.row === 1 && c.col === 1);
      await dblclick(...centreOf(data));
      const sel = selIds();
      t(!tid("drill-chart-properties") && sel.length === back.table.cols && sel.every((id) => id.startsWith(`${back.table.id}:1:`)), `the row's cells are selected (${sel.join(",")})`);
    });

    // ------------------------------------------------------------------------------------------------------------------ 3. Drill Groups
    await scenario("Drill Groups: the groups and their marks; a group's mark committed on the next row, the chart shows it; Flash on Canvas; Freeze; Close", async (t) => {
      await select([]);
      await E.run("pcbnew.InteractiveDrawing.showDrillGroups");
      await waitFor(() => !!tid("drill-groups") && !!tid("dg-row-0"), 6000);
      t(!!tid("drill-groups"), "Drill Groups is open");
      if (!tid("dg-row-0")) return;
      const cells = (i) => [...tid(`dg-row-${i}`).querySelectorAll("td")].map((c) => c.textContent);
      t(/^Shape \d+$|^[A-Z]+$|^\d+\.\d\d$/.test(cells(0)[0]) && ["Yes", "No"].includes(cells(0)[2]) && / \/ /.test(cells(0)[3]), `a row: ${cells(0).join(" | ")}`);
      t(tid("dg-profile").textContent === "Default", "the profile is the Default one");
      const groups = (await json("/api/drill_groups")).groups;
      const row = groups.findIndex((g) => g.diameter_nm === 400_000);
      t(row >= 0, `the run's 0.4 mm via is a group (${groups.length} groups)`);
      if (row < 0) return;
      tid(`dg-row-${row}`).click();
      await sleep(300);
      setValue(tid("dg-mark"), "letter");
      setValue(tid("dg-letter"), "Q");
      setValue(tid("dg-description"), "probe");
      await sleep(100);
      const before = selIds();
      tid("dg-flash").click();
      await sleep(600);
      t(JSON.stringify(selIds()) === JSON.stringify(before), "Flash on Canvas brightens, the selection stays");
      const key = groups[row].key;
      await waitFor(async () => (await board()).drawings?.drill_profile?.assignments?.[key]?.letter === "Q", 8000);
      const st = await board();
      t(st.drawings.drill_profile?.assignments?.[key]?.letter === "Q" && st.drawings.drill_profile.assignments[key].description === "probe", "the group's mark and description are the board's");
      const { chart, table } = await chartOf();
      const r = Object.entries(chart.row_keys ?? {}).find(([, k]) => k === key)?.[0];
      t(r != null && table.cells.find((c) => c.row === Number(r) && c.col === chart.symbol_column)?.text === "Q", "the chart's row shows the letter");
      await waitFor(() => cells(row)[0] === "Q", 6000);
      t(cells(row)[0] === "Q", "the dialog reloads with the new mark");
      const freeze = tid("dg-freeze");
      const was = freeze.checked;
      freeze.click();
      await waitFor(async () => ((await board()).drawings?.drill_profile?.freeze_assignments ?? true) === !was, 6000);
      t(((await board()).drawings?.drill_profile?.freeze_assignments ?? true) === !was, "Freeze assignments is the profile's");
      await undo();
      tid("dg-close").click();
      await sleep(400);
      t(!tid("drill-groups"), "Close");
      await undo();
      t(!(await board()).drawings?.drill_profile?.assignments?.[key] || (await board()).drawings.drill_profile.assignments[key].letter !== "Q", "an undo takes the mark back");
    });

    // ------------------------------------------------------------------------------------------------------------------ 4. a footprint move into a grid
    await scenario("a footprint carried into a grid item that affects placement turns to its frame; one that affects only the cursor turns nothing", async (t) => {
      const fp = st0.parts.find((p) => p.placed && p.at);
      t(!!fp, "a footprint to move");
      if (!fp) return;
      const g = at(-25, 25);
      const add = async (affects) => {
        const n = ((await board()).drawings?.grid_items ?? []).length;
        await cmd({ op: "pcb_item", verb: "add_grid_item", grid_item: { kind: "cartesian", at: { x: Math.round(g[0] * 1e6), y: Math.round(g[1] * 1e6) }, extent: { x: 8_000_000, y: 8_000_000 }, spacing: { x: 1_000_000, y: 1_000_000 }, angle: 30, phi_extent_deg: 360, phi_spacing_deg: 10, priority: 1, tick_interval: 0, affects } });
        await waitFor(async () => ((await board()).drawings?.grid_items ?? []).length === n + 1);
        return ((await board()).drawings?.grid_items ?? []).at(-1);
      };
      const part = async () => (await board()).parts.find((p) => p.ref === fp.ref);
      const carry = async () => {
        await select([fp.ref]);
        const p0 = await part();
        await hover(p0.at[0] / 1e6, p0.at[1] / 1e6);
        await E.run("pcbnew.InteractiveMove.move");
        await sleep(200);
        await hover((p0.at[0] / 1e6 + g[0]) / 2, (p0.at[1] / 1e6 + g[1]) / 2);
        await hover(g[0] + 0.4, g[1] + 0.4);
        const [x, y] = toScreen(g[0] + 0.4, g[1] + 0.4);
        fire("pointerdown", x, y, { buttons: 1 }); await sleep(80); fire("pointerup", x, y); await sleep(1200);
        return part();
      };
      const turn = (a, b) => ((((b - a) % 360) + 540) % 360) - 180;
      const grid = await add({ cursor: true, routing: true, placement: true });
      t(!!grid && grid.angle === 30, "a grid item turned 30 degrees, affecting placement");
      const o0 = (await part()).orientation ?? 0;
      const moved = await carry();
      t(Math.abs(moved.at[0] / 1e6 - g[0]) < 3 && Math.abs(moved.at[1] / 1e6 - g[1]) < 3, `dropped in the grid (${moved.at})`);
      t(Math.abs(turn(o0, moved.orientation ?? 0) - 30) < 0.01, `the footprint turned to the grid's frame: ${o0} -> ${moved.orientation}`);
      await undo();
      t(Math.abs(turn(o0, (await part()).orientation ?? 0)) < 0.01, "one undo puts it back, unturned");
      await cmd({ op: "pcb_item", verb: "delete_grid_item", id: grid.id });
      const cursorOnly = await add({ cursor: true, routing: true, placement: false });
      const kept = await carry();
      t(Math.abs(turn(o0, kept.orientation ?? 0)) < 0.01, `a grid that does not affect placement turns nothing (${kept.orientation})`);
      await undo();
      await cmd({ op: "pcb_item", verb: "delete_grid_item", id: cursorOnly.id });
    });

    // ------------------------------------------------------------------------------------------------------------------ 5. a route's point on a grid
    await scenario("the route tool's point snaps to a grid item that affects the cursor, not to one that affects only routing", async (t) => {
      const g = at(-25, 45);
      const add = async (affects) => {
        const n = ((await board()).drawings?.grid_items ?? []).length;
        await cmd({ op: "pcb_item", verb: "add_grid_item", grid_item: { kind: "cartesian", at: { x: Math.round(g[0] * 1e6), y: Math.round(g[1] * 1e6) }, extent: { x: 6_000_000, y: 6_000_000 }, spacing: { x: 700_000, y: 700_000 }, angle: 0, phi_extent_deg: 360, phi_spacing_deg: 10, priority: 1, tick_interval: 0, affects } });
        await waitFor(async () => ((await board()).drawings?.grid_items ?? []).length === n + 1);
        return ((await board()).drawings?.grid_items ?? []).at(-1);
      };
      const snapAt = async () => {
        await E.run("pcbnew.InteractiveRouter.SingleTrack");
        await sleep(300);
        await hover(g[0] + 1.62, g[1] + 0.29);
        await hover(g[0] + 1.63, g[1] + 0.3);
        const s = E.state().snap;
        await escape();
        await escape();
        return s;
      };
      const cursor = await add({ cursor: true, routing: false, placement: false });
      const s1 = await snapAt();
      const want = [Math.round((g[0] + 1.4) * 1e6), Math.round(g[1] * 1e6)];
      t(!!s1 && s1.tool === "route" && Math.abs(s1.output[0] - want[0]) < 2 && Math.abs(s1.output[1] - want[1]) < 2, `on the grid item's point (${want}): ${JSON.stringify(s1?.output)}`);
      await cmd({ op: "pcb_item", verb: "delete_grid_item", id: cursor.id });
      const routing = await add({ cursor: false, routing: true, placement: false });
      const s2 = await snapAt();
      t(!!s2 && (Math.abs(s2.output[0] - want[0]) > 50_000 || Math.abs(s2.output[1] - want[1]) > 50_000), `a routing-only grid: the editor's grid (${JSON.stringify(s2?.output)})`);
      await cmd({ op: "pcb_item", verb: "delete_grid_item", id: routing.id });
    });

    // ------------------------------------------------------------------------------------------------------------------ 6. Via Stitching's via
    await scenario("Via Stitching's via: Configure is Track & Via Properties on the template; OK makes every via a copy of it", async (t) => {
      const stitches = async () => (await board()).drawings?.via_stitches ?? [];
      for (const old of await stitches()) await cmd({ op: "pcb_item", verb: "delete_via_stitch", id: old.id });
      await E.run("pcbnew.ZoneFiller.zoneFillAll");
      await sleep(4000);
      toScreen = await calibrate();
      const n0 = (await stitches()).length;
      await E.run("pcbnew.InteractiveDrawing.drawViaStitchArea");
      await sleep(300);
      const [x0, y0, x1, y1] = [bx0 + 2, by0 + 6, Math.min(bx1 - 2, bx0 + 22), Math.min(by1 - 2, by0 + 22)];
      await click(x0, y0);
      await click(x1, y0);
      await click(x1, y1);
      await click(x0, y1);
      await dblclick(x0, y1);
      await waitFor(() => !!tid("via-stitch-properties"), 4000);
      t(!!tid("via-stitch-properties"), "Via Stitching Properties opens");
      if (!tid("via-stitch-properties")) return;
      t(!!tid("vst-configure") && !tid("vst-size"), "Via Properties is a Configure button");
      const net = tid("vst-net");
      setValue(net, [...net.options].map((o) => o.value).find((v) => /^GND$/i.test(v)) ?? [...net.options].map((o) => o.value).find((v) => v) ?? "");
      setValue(tid("vst-pitch"), "2");
      tid("vst-configure").click();
      await waitFor(() => !!tid("via-template-properties"), 3000);
      t(!!tid("via-template-properties"), "Configure opens Track & Via Properties on the template");
      if (!tid("via-template-properties")) return;
      // the standalone Track & Via Properties (UI-LEFT-2 PP22): its rows by their labels
      const vt = (label) => tid("via-template-properties")?.querySelector(`[aria-label="${label}"]`);
      const vtOk = () => tid("via-template-properties")?.querySelector(".dialog-footer .primary");
      t(Number(vt("Via diameter").value) > 0 && Number(vt("Via hole").value) > 0 && vt("Via type").value === "through", `the template: a through via of the toolbar's size (${vt("Via diameter").value} / ${vt("Via hole").value})`);
      t(!!vt("Net") && !!vt("Locked") && !!vt("Protection features") && !!vt("Backdrill") && !!vt("Post-machining front") && !!vt("Pre-defined via sizes") && !!tid("via-template-properties").querySelector('fieldset[aria-label="Teardrops"]'), "Net, Locked, Pre-defined sizes, Protection features, Backdrill, Post-machining and Teardrops are offered for the template");
      setValue(vt("Via hole"), vt("Via diameter").value);
      await sleep(100);
      vtOk().click();
      await sleep(300);
      const vtError = () => tid("via-template-properties")?.querySelector('[role="alert"]')?.textContent;
      t(vtError() === "Via hole size must be smaller than via diameter", `ValidateViaParameters' words: ${vtError()}`);
      setValue(vt("Via diameter"), "0.7");
      setValue(vt("Via hole"), "0.35");
      t(!!vt("Annular rings"), "a through via of a four-layer board has Annular rings");
      if (vt("Annular rings")) setValue(vt("Annular rings"), "remove_all");
      // Protection features: filled and capped (IPC-4761 type VII), and the lock
      setValue(vt("Protection features"), "vii");
      vt("Locked").click();
      await sleep(100);
      vtOk().click();
      await sleep(300);
      t(!tid("via-template-properties"), "OK goes back to the stitching's dialog");
      tid("vst-ok").click();
      await waitFor(async () => (await stitches()).length === n0 + 1, 12000);
      const s = (await stitches()).at(-1);
      t(!!s && s.via?.size_nm === 700_000 && s.via?.drill_nm === 350_000 && s.via?.annular === "remove_all" && s.via?.locked === true && s.via?.props?.filling === true, `the stitching's template (${JSON.stringify(s?.via)})`);
      if (!s) return;
      const st1 = await board();
      const vias = st1.routing.vias.filter((v) => (s.member_ids ?? []).includes(v.id));
      t(vias.length > 0 && vias.every((v) => v.d === 700_000 && v.drill === 350_000 && v.unconnected === "remove_all" && v.props?.filling === true), `every via is a copy of it (${vias.length} vias: ${JSON.stringify(vias[0] ? { d: vias[0].d, drill: vias[0].drill, unconnected: vias[0].unconnected, props: vias[0].props } : null)})`);
      t(vias.length > 0 && vias.every((v) => (st1.locked ?? []).includes(v.id)), "and locked, as the template is");
      await select([s.id]);
      await E.run("common.Interactive.delete");
      await waitFor(async () => (await stitches()).length === n0, 10000);
      t((await stitches()).length === n0, "Delete removes it");
    });

    // ------------------------------------------------------------------------------------------------------------------ 7. Board Setup's Drill Chart page
    await scenario("Board Setup > Drill Chart: Default marks applied, one undo step; Close and Edit Drill Groups... opens Drill Groups", async (t) => {
      const policy = async () => (await board()).drawings?.drill_profile?.mark_policy;
      const p0 = await policy();
      await E.run("pcbnew.EditorControl.boardSetup");
      await sleep(600);
      const nav = [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim() === "Drill Chart");
      t(!!nav, "Board Setup has the Drill Chart page");
      if (!nav) return;
      nav.click();
      await sleep(300);
      const pageEl = document.querySelector('section.bs-page[aria-label="Drill Chart"]');
      const marks = pageEl?.querySelector('select[aria-label="Default marks"]');
      t(!!marks && [...marks.options].map((o) => o.textContent).join("|") === "Curated shapes|Letters|Curated shapes, then letters|Hole size text", `Default marks' choices (${marks ? [...marks.options].map((o) => o.textContent).join("|") : "none"})`);
      const labels = [...pageEl.querySelectorAll(".bs-check")].map((l) => l.textContent.trim());
      t(labels.includes("IPC-4761 protection") && labels.includes("Keep existing symbol assignments when holes change"), "the grouping checklist and the freeze");
      setValue(marks, "letters");
      await sleep(150);
      [...pageEl.querySelectorAll("button")].find((b) => b.textContent.trim() === "Apply").click();
      await waitFor(async () => (await policy()) === "letters", 6000);
      t((await policy()) === "letters", "Apply sets the profile's default marks");
      await undo();
      t((await policy()) === p0, "one undo puts them back");
      [...pageEl.querySelectorAll("button")].find((b) => b.textContent.trim() === "Close and Edit Drill Groups...").click();
      await waitFor(() => !!tid("drill-groups"), 6000);
      t(!!tid("drill-groups") && !document.querySelector(".bs-dialog"), "Board Setup closes and Drill Groups opens");
      tid("dg-close")?.click();
      await sleep(300);
    });

    const { chart } = await chartOf();
    if (chart) await cmd({ op: "table", verb: "delete", id: chart.table_id });
    for (const v of ((await board()).routing?.vias ?? []).filter((v) => !viasBefore.has(v.id))) await cmd({ op: "delete_via", id: v.id });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
