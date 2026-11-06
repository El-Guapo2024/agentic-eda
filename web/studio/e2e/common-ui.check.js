// The parts every editor shares (UI-LEFT C1, C2, C4; PARITY-common.md sections 2, 3, 10 and 17, GAPS.md items 15 and 20):
//   drag buttons  Preferences > Mouse and Touchpad's "Middle button drag:" and "Right button drag:" pan, zoom (by exp( dy x zoom speed / 1000 ) about the press point) or do
//                 nothing on the board, the Footprint Editor and the Symbol Editor (`WX_VIEW_CONTROLS::onButton`, `onMotion`);
//   left button   "Left button drag:" -- "Draw selection rectangle" leaves a footprint where it is and boxes, "Drag any object" picks it up, "Drag selected objects"
//                 boxes from it while it is not selected and drags it once it is (`PCB_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )`); each drag undone;
//   pinch         a wheel event with ctrlKey and no Control key down zooms about the pointer by exp( -deltaY / 100 ) (`onMagnify`); Control held and the wheel still pans;
//   grid display  each editor's Display Options > Grid Display (Lines, Small crosses, the thickness, the minimum spacing) is the one its canvas draws with, and only its;
//   edit grids    Grid Settings' Name, X, Linked and Y: a named grid with its own Y is listed "name: X x Y (..)", refused when it is in the list or Y is out of range,
//                 made current by OK and drawn with its X and Y; the toolbar's grid box words it the same;
//   sch menu      the schematic's context menu ends with the Zoom and Grid submenus (`AddStandardSubMenus`);
//   enter         during a route Enter is a click where the cursor is (`ACTIONS::cursorClick`): the route is fixed there and goes on; End finishes it;
//   measure       the Measure tool's ruler shows x, y, r and θ next to its end (`RULER_ITEM::GetDimensionStrings`), with its ticks, on the board and the Footprint Editor.
// What the canvases drew is read from `window.__edaGrid` (components/canvas/gridPaint.ts) and `window.__edaRuler` (components/canvas/rulerPaint.ts).
// Runs on a fresh `eda board new examples/mcu_board_30plus.yaml` board; every scenario puts back what it changed, the preferences too.
// It answers through `window.__commonUiCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__commonUiCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };

  // the router's replies, to see a fix from a finish
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/api\/route\/([a-z_]+)/.exec(url);
    if (m) replies.push({ name: m[1], body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitReply = async (name, from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies.slice(from).find((x) => x.name === name); if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(60); } };

  // ------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const container = () => [...document.querySelectorAll(".pcb-canvas-container")].filter((c) => c.offsetParent !== null).sort((a, b) => b.clientWidth * b.clientHeight - a.clientWidth * a.clientHeight)[0];
  const rect = () => container().getBoundingClientRect();
  const view = () => E.state().view;
  const w2c = (wx, wy) => { const v = view(), r = rect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const target = (x, y) => { const el = document.elementFromPoint(x, y); return el && container().contains(el) ? el : container().querySelector("canvas") ?? container(); };
  const BUTTONS = { 0: 1, 1: 4, 2: 2 };
  const fire = (type, x, y, button = 0, extra = {}) => target(x, y).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button, buttons: type === "pointerup" ? 0 : type === "pointermove" && extra.held === false ? 0 : BUTTONS[button], ...extra }));
  /**
   * A point on a footprint's body that holds nothing else (UI-LEFT P15): inside its courtyard, off every pad and track, and clear of every footprint's field
   * and text. A click at a footprint's anchor is often on its fab reference (`${REFERENCE}` on F.Fab, at the anchor); KiCad's selection takes that text
   * there -- `PCB_SELECTION_TOOL::Selectable` lets a footprint's `PCB_TEXT` be picked, and `GuessSelectionCandidates` keeps the smaller of two exact hits --
   * so a check that means the footprint presses on its body. `nmPerPx` is the canvas' scale; the point is the one farthest from all of those (at least
   * `minPx` pixels when the body has room), the nearest the anchor of the equally good.
   */
  const bodyPoint = (st, part, nmPerPx, minPx = 7) => {
    const boxDist = (x, y, cx, cy, hw, hh, angDeg = 0) => {
      const a = (-angDeg * Math.PI) / 180, dx = x - cx, dy = y - cy;
      const u = dx * Math.cos(a) + dy * Math.sin(a), v = -dx * Math.sin(a) + dy * Math.cos(a);
      return Math.hypot(Math.max(Math.abs(u) - hw, 0), Math.max(Math.abs(v) - hh, 0));
    };
    const segDist = (x, y, a, b) => { const dx = b[0] - a[0], dy = b[1] - a[1], l = dx * dx + dy * dy || 1; const t = Math.max(0, Math.min(1, ((x - a[0]) * dx + (y - a[1]) * dy) / l)); return Math.hypot(x - (a[0] + t * dx), y - (a[1] + t * dy)); };
    const texts = st.parts.filter((p) => p.placed).flatMap((p) => [...(p.fields ?? []).filter((f) => f.visible && f.text), ...(p.texts ?? []).filter((t) => t.text)]);
    const pads = st.parts.filter((p) => p.placed).flatMap((p) => p.pads ?? []);
    const others = st.parts.filter((p) => p.placed && p.ref !== part.ref && p.courtyard);
    const clearance = (x, y) => {
      let d = Infinity;
      for (const q of pads) d = Math.min(d, boxDist(x, y, q.x, q.y, q.w / 2, q.h / 2));
      for (const t of texts) {
        const n = Math.max(String(t.shown ?? t.text).length, 1);
        // generous: a glyph's advance is about its height, the box 1.3 heights tall; a text not centred may run either way from its anchor
        d = Math.min(d, boxDist(x, y, t.x, t.y, (t.halign ? n : n / 2) * t.h, (t.valign ? 1.3 : 0.65) * t.h, (t.angle ?? 0) / 1000));
      }
      for (const tr of st.routing?.tracks ?? []) for (let i = 1; i < tr.pts.length; i++) d = Math.min(d, segDist(x, y, tr.pts[i - 1], tr.pts[i]) - tr.width / 2);
      for (const v of st.routing?.vias ?? []) d = Math.min(d, Math.hypot(x - v.x, y - v.y) - v.d / 2);
      return d;
    };
    const [x0, y0, x1, y1] = part.courtyard;
    const [mx, my] = [(x1 - x0) * 0.15, (y1 - y0) * 0.15];
    let best = null;
    const step = Math.max(nmPerPx, (x1 - x0) / 150, (y1 - y0) / 150);
    for (let x = x0 + mx; x <= x1 - mx; x += step) {
      for (let y = y0 + my; y <= y1 - my; y += step) {
        if (others.some((o) => x >= o.courtyard[0] && x <= o.courtyard[2] && y >= o.courtyard[1] && y <= o.courtyard[3])) continue;
        const c = Math.min(clearance(x, y) / nmPerPx, minPx * 2);
        const r = Math.hypot(x - part.at[0], y - part.at[1]);
        if (!best || c > best.c + 1e-9 || (Math.abs(c - best.c) <= 1e-9 && r < best.r)) best = { x, y, c, r };
      }
    }
    return best ? [Math.round(best.x), Math.round(best.y)] : [part.at[0], part.at[1]];
  };
  /** A drag of `button` from screen point `a` by (dx, dy), in steps so it becomes a drag (8 px) first. */
  const drag = async (a, dx, dy, button = 0) => {
    fire("pointermove", a[0], a[1], button, { held: false, buttons: 0 });
    await sleep(60);
    fire("pointerdown", a[0], a[1], button);
    await sleep(60);
    for (let i = 1; i <= 4; i++) {
      fire("pointermove", a[0] + (dx * i) / 4, a[1] + (dy * i) / 4, button);
      await sleep(70);
    }
    fire("pointerup", a[0] + dx, a[1] + dy, button);
    await sleep(500);
  };
  const centre = () => { const r = rect(); return [r.left + r.width / 2, r.top + r.height / 2]; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(900); };
  const prefsDialog = () => document.querySelector('[data-testid="preferences-dialog"]');
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(80); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(60); };
  const buttonIn = (root, label) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label || b.getAttribute("aria-label") === label);
  /** Opens Preferences on `page` (the tree's label; for a page every editor has, `editorIndex` in KiCad's order: 0 Symbol, 1 Schematic, 2 Footprint, 3 PCB Editor),
   *  applies `fn` to the dialog and presses OK. */
  const prefs = async (fn, page = "Mouse and Touchpad", editorIndex = 0) => {
    await E.run("common.SuiteControl.openPreferences");
    const d = await until(() => prefsDialog(), 4000);
    if (!d) throw new Error("Preferences did not open");
    const items = [...d.querySelectorAll('[role="treeitem"]')].filter((x) => x.textContent.trim() === page);
    items[editorIndex]?.click();
    await sleep(200);
    await fn(prefsDialog());
    buttonIn(prefsDialog(), "OK")?.click();
    await sleep(300);
  };
  const setDrags = (left, middle, right) => prefs(async (d) => { if (left) await choose(d.querySelector('[data-testid="pref-drag-left"]'), left); if (middle) await choose(d.querySelector('[data-testid="pref-drag-middle"]'), middle); if (right) await choose(d.querySelector('[data-testid="pref-drag-right"]'), right); });
  const redraw = async () => { await E.run("common.Control.zoomRedraw").catch(() => null); await sleep(300); };
  const state = () => json("/api/state");
  const partOf = (st, ref) => st.parts.find((p) => p.ref === ref);
  const snapshot = (st) => JSON.stringify([st.parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot]), (st.routing?.tracks ?? []).map((t) => [t.id, t.pts])]);
  let original = null;
  const restore = async () => { for (let i = 0; i < 10 && original && snapshot(await state()) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(500); } };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } await realFetch("/api/route/cancel", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" }).catch(() => null); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try {
      for (const d of document.querySelectorAll(".dialog")) buttonIn(d, "Cancel")?.click();
      await leaveTool();
    } catch { /* best effort */ }
    results.push(entry);
  };
  const near = (a, b, tol) => Math.abs(a - b) <= tol;

  // ------------------------------------------------------------------------------------------------------------------------------------------------ run
  if (E.state().tab !== "pcb") await clickTab("PCB");
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const before = await state();
  // R1 in the middle of the outline, where a drag of a few millimetres keeps it on the board (a move off it is refused: placement_within_outline)
  const xs = (before.outline ?? []).map((p) => p[0]), ys = (before.outline ?? []).map((p) => p[1]);
  const mid = xs.length ? [Math.round((Math.min(...xs) + Math.max(...xs)) / 2_000_000) * 1_000_000, Math.round((Math.min(...ys) + Math.max(...ys)) / 2_000_000) * 1_000_000] : [30_000_000, 30_000_000];
  await cmd({ op: "place_at", part: "R1", x: mid[0], y: mid[1] });
  await E.run("common.Control.zoomFitScreen");
  await sleep(600);
  original = snapshot(await state());
  const units = E.state().units;

  await scenario("drag buttons: the middle and right drags pan, zoom or do nothing on the board, the Footprint Editor and the Symbol Editor", async (t) => {
    await setDrags(null, "pan", "zoom");
    for (const [label, tab] of [["PCB", "pcb"], ["Footprint", "footprint"], ["Symbol", "symbol"]]) {
      if (E.state().tab !== tab) await clickTab(label);
      if (!t(E.state().tab === tab, `on the ${tab} tab`)) continue;
      const v0 = view();
      const c = centre();
      await drag(c, 50, 30, 1);
      const v1 = view();
      t(near(v1.x - v0.x, 50, 1) && near(v1.y - v0.y, 30, 1) && v1.scale === v0.scale, `${tab}: a middle drag pans by the pointer's travel (${(v1.x - v0.x).toFixed(1)}, ${(v1.y - v0.y).toFixed(1)})`);
      await drag(c, 0, -60, 2);
      const v2 = view();
      const want = Math.exp(60 * (/Mac/.test(navigator.platform) ? 5 : 1) * 0.001);
      t(near(v2.scale / v1.scale, want, 0.01), `${tab}: a right drag up 60 px zooms in by exp( 60 x zoom speed / 1000 ) (${(v2.scale / v1.scale).toFixed(4)}, want ${want.toFixed(4)})`);
    }
    await setDrags(null, "none", "none");
    for (const [label, tab] of [["PCB", "pcb"], ["Symbol", "symbol"]]) {
      if (E.state().tab !== tab) await clickTab(label);
      const v0 = view();
      await drag(centre(), 40, 40, 1);
      await drag(centre(), 40, -40, 2);
      const v1 = view();
      t(v1.x === v0.x && v1.y === v0.y && v1.scale === v0.scale, `${tab}: with None neither drag moves the view`);
    }
    await setDrags(null, "pan", "pan");
    await clickTab("PCB");
  });

  await scenario("Left button drag: Draw selection rectangle boxes from a footprint, Drag any object picks it up, Drag selected objects drags it only once it is selected", async (t) => {
    await clickTab("PCB");
    await E.run("common.Control.zoomFitScreen");
    await sleep(400);
    const st1 = await state();
    const r1 = partOf(st1, "R1");
    // R1's body, not its anchor: the anchor is under its fab reference, which a press there takes (bodyPoint)
    const body = bodyPoint(st1, r1, 1 / view().scale);
    const at = () => w2c(body[0], body[1]);
    const moved = async () => { const p = partOf(await state(), "R1"); return p.at[0] !== r1.at[0] || p.at[1] !== r1.at[1]; };
    const clear = async () => { await E.run("common.Interactive.cancel"); await sleep(150); await E.run("common.Interactive.cancel"); await sleep(150); };

    await setDrags("select", null, null);
    await clear();
    await drag(at(), 60, 40, 0);
    t(!(await moved()), "Draw selection rectangle: R1 stays where it is");
    await setDrags("drag_any", null, null);
    await clear();
    await drag(at(), 60, 40, 0);
    t(await moved(), "Drag any object: a drag from R1 moves it");
    await restore();
    t(!(await moved()), "one undo puts it back");
    await setDrags("drag_selected", null, null);
    await clear();
    await drag(at(), 60, 40, 0);
    t(!(await moved()), "Drag selected objects: R1 not selected stays");
    await clear();
    E.run("common.InteractiveSelection.SelectItem", "R1").catch(() => null);
    await sleep(200);
    if (!E.state().selection.some((s) => s.id === "R1")) { fire("pointerdown", ...at(), 0); await sleep(40); fire("pointerup", ...at(), 0); await sleep(300); }
    t(E.state().selection.some((s) => s.id === "R1"), `R1 selected (${JSON.stringify(E.state().selection)})`);
    await drag(at(), 60, 40, 0);
    t(await moved(), "and selected, a drag from it moves it");
    await restore();
    await setDrags("drag_any", null, null);
    await clear();
  });

  await scenario("a touchpad pinch zooms about the pointer as onMagnify does; Control held and the wheel still pans", async (t) => {
    await clickTab("PCB");
    const c = centre();
    const r = rect();
    const v0 = view();
    const w0 = [(c[0] - r.left - v0.x) / v0.scale, (c[1] - r.top - v0.y) / v0.scale];
    container().dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -20, ctrlKey: true, clientX: c[0], clientY: c[1] }));
    await sleep(300);
    const v1 = view();
    t(near(v1.scale / v0.scale, Math.exp(0.2), 1e-6), `the scale grows by exp( 20 / 100 ) (${(v1.scale / v0.scale).toFixed(6)})`);
    const w1 = [(c[0] - r.left - v1.x) / v1.scale, (c[1] - r.top - v1.y) / v1.scale];
    t(near(w1[0], w0[0], 1 / v1.scale) && near(w1[1], w0[1], 1 / v1.scale), `the point under the pointer stays (within a pixel: ${(w1[0] - w0[0]).toFixed(0)}, ${(w1[1] - w0[1]).toFixed(0)} nm)`);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Control", bubbles: true }));
    container().dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -100, ctrlKey: true, clientX: c[0], clientY: c[1] }));
    window.dispatchEvent(new KeyboardEvent("keyup", { key: "Control", bubbles: true }));
    await sleep(300);
    const v2 = view();
    t(v2.scale === v1.scale && v2.x !== v1.x && v2.y === v1.y, `Ctrl and the wheel pans left/right (scale ${v2.scale === v1.scale}, dx ${(v2.x - v1.x).toFixed(1)})`);
  });

  await scenario("Display Options > Grid Display: each editor draws its grid in its own style, thickness and minimum spacing", async (t) => {
    await clickTab("PCB");
    await prefs(async (d) => {
      const page = d.querySelector('[data-testid="display-options-pcb"]');
      page.querySelector('[data-testid="pref-grid-lines"]').click();
      await choose(page.querySelector('[data-testid="pref-grid-thickness"]'), "2");
      await type(page.querySelector('[data-testid="pref-grid-min-spacing"]'), 20);
    }, "Display Options", 3);
    await redraw();
    const g = window.__edaGrid?.pcb;
    t(g?.style === "lines" && (g.marks.line ?? 0) > 0 && !g.marks.rect, `the board draws lines (${JSON.stringify(g)})`);
    t(g?.lineWidthPx === 2 && g?.minSpacingPx === 20, "2 px thick, 20 px apart at the least");
    await clickTab("Schematic");
    await redraw();
    const s = window.__edaGrid?.schematic;
    t(s?.style === "dots" && (s.marks.rect ?? 0) > 0, `the schematic keeps its dots (${s?.style})`);
    await prefs(async (d) => d.querySelector('[data-testid="display-options-schematic"] [data-testid="pref-grid-crosses"]').click(), "Display Options", 1);
    await redraw();
    const s2 = window.__edaGrid?.schematic;
    t(s2?.style === "small-cross" && (s2.marks.cross ?? 0) > 0, `and draws small crosses once its own page says so (${JSON.stringify(s2?.marks)})`);
    // put both back
    await prefs(async (d) => d.querySelector('[data-testid="display-options-schematic"] [data-testid="pref-grid-dots"]').click(), "Display Options", 1);
    await clickTab("PCB");
    await prefs(async (d) => {
      const page = d.querySelector('[data-testid="display-options-pcb"]');
      page.querySelector('[data-testid="pref-grid-dots"]').click();
      await choose(page.querySelector('[data-testid="pref-grid-thickness"]'), "1");
      await type(page.querySelector('[data-testid="pref-grid-min-spacing"]'), 10);
    }, "Display Options", 3);
    await redraw();
    t(window.__edaGrid?.pcb?.style === "dots", "back to dots");
  });

  await scenario("Edit Grids: a named grid with its own X and Y -- listed, checked, made current, drawn and shown in the grid box as KiCad words it", async (t) => {
    await clickTab("PCB");
    const gridBefore = E.state().grid;
    await E.run("common.Control.editGrids");
    const dlg = () => document.querySelector('[aria-label="Preferences - Grids"]');
    if (!t(await until(() => dlg(), 3000), "Preferences - Grids opens")) return;
    const sub = () => dlg().querySelector('[aria-label="Grid Settings"]');
    const fill = async (name, x, linked, y) => {
      buttonIn(dlg(), "Add Grid").click();
      await sleep(150);
      await type(sub().querySelector('[aria-label="Grid name"]'), name);
      await type(sub().querySelector('[aria-label="Grid size X"]'), x);
      const link = sub().querySelector('[aria-label="Linked"]');
      if (link.checked !== linked) { link.click(); await sleep(60); }
      if (!linked) await type(sub().querySelector('[aria-label="Grid size Y"]'), y);
      buttonIn(sub(), "OK").click();
      await sleep(200);
    };
    const mm = units === "mm" ? 1 : units === "mil" ? 1000 / 25.4 : 1 / 25.4;
    await fill("Wide", 2 * mm, false, 5000 * mm);
    t(/Grid size Y out of range\./.test(dlg().querySelector('[role="alert"]')?.textContent ?? ""), `Y of 5 m: "${dlg().querySelector('[role="alert"]')?.textContent}"`);
    await type(sub().querySelector('[aria-label="Grid size Y"]'), 1 * mm);
    buttonIn(sub(), "OK").click();
    await sleep(200);
    const list = dlg().querySelector('select[aria-label="Grids"]');
    const label = list.options[list.selectedIndex]?.textContent ?? "";
    t(/^Wide: .+ x .+ \(.+ x .+\)$/.test(label), `listed and selected as "${label}"`);
    await fill("Wide", 2 * mm, false, 1 * mm);
    t(/already exists\./.test(dlg().querySelector('[role="alert"]')?.textContent ?? ""), `the same grid again: "${dlg().querySelector('[role="alert"]')?.textContent}"`);
    buttonIn(sub(), "Cancel")?.click();
    await sleep(100);
    buttonIn(dlg(), "OK").click();
    await sleep(400);
    await redraw();
    t(near(E.state().grid, 2_000_000, 1), `the grid is its X (${E.state().grid})`);
    const g = window.__edaGrid?.pcb;
    t(g && near(g.size.x, 2_000_000, 1) && near(g.size.y, 1_000_000, 1), `drawn 2 by 1 mm (${JSON.stringify(g?.size)})`);
    const box = document.querySelector('.toolbar-control[title="Grid"] select');
    t(/^Wide: /.test(box?.options[box.selectedIndex]?.textContent ?? ""), `the toolbar's grid box shows "${box?.options[box.selectedIndex]?.textContent}"`);
    // Reset to Defaults and the grid that was current
    await E.run("common.Control.editGrids");
    await until(() => dlg(), 3000);
    buttonIn(dlg(), "Reset to Defaults").click();
    await sleep(150);
    const l2 = dlg().querySelector('select[aria-label="Grids"]');
    t(![...l2.options].some((o) => o.textContent.startsWith("Wide")), "Reset to Defaults drops it");
    buttonIn(dlg(), "OK").click();
    await sleep(300);
    // `DefaultGridSizeList()` of the board editor: the grid that was current, by its place in it
    const DEFAULTS = [25_400_000, 12_700_000, 6_350_000, 5_080_000, 2_540_000, 1_270_000, 635_000, 508_000, 254_000, 127_000, 50_800, 25_400, 5_000_000, 2_500_000, 1_000_000, 500_000, 250_000, 200_000, 100_000, 50_000, 25_000, 10_000];
    await E.run("common.Control.gridPreset", Math.max(0, DEFAULTS.indexOf(gridBefore)));
    await sleep(200);
    t(E.state().grid === gridBefore, `the grid that was current is back (${E.state().grid}, was ${gridBefore})`);
  });

  await scenario("the schematic's context menu ends with the Zoom and Grid submenus", async (t) => {
    await clickTab("Schematic");
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
    const r = rect();
    const x = r.left + 12, y = r.top + 12;
    target(x, y).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 2 }));
    await sleep(400);
    const menu = [...document.querySelectorAll(".menubar-dropdown")].find((m) => m.style.position === "fixed");
    if (!t(menu, "the context menu opens")) return;
    const subs = [...menu.querySelectorAll(":scope > .menu-node-submenu")].map((s) => s.querySelector(".menu-node-label")?.textContent.trim());
    t(subs.slice(-2).join("|").startsWith("Zoom") && subs.slice(-1)[0]?.startsWith("Grid"), `the last submenus: ${subs.join(", ")}`);
    const text = menu.textContent;
    const grids = units === "mm" ? ["2.540 mm", "1.270 mm", "0.635 mm", "0.254 mm"] : ["100", "50", "25", "10"];
    t(/Zoom: 1\.00/.test(text) && /Zoom: 0\.05/.test(text) && /Grid Origin\.\.\./.test(text) && grids.every((g) => text.includes(g)), `Zoom lists the eeschema zooms and Grid the origin entry and the schematic grids: ${text.slice(text.indexOf("Zoom: 0.05"), text.indexOf("Zoom: 0.05") + 40)} .. ${text.slice(text.indexOf("Grid Origin"), text.indexOf("Grid Origin") + 120)}`);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(200);
    await clickTab("PCB");
  });

  await scenario("Enter during a route is a click where the cursor is: the route is fixed there and goes on; End finishes it", async (t) => {
    await clickTab("PCB");
    await E.run("common.Control.zoomFitScreen");
    await sleep(400);
    const r1 = partOf(await state(), "R1");
    const pad = r1.pads.find((p) => p.num === "1") ?? r1.pads[0];
    await E.run("pcbnew.InteractiveRouter.SingleTrack");
    await sleep(300);
    let n = replies.length;
    const p0 = w2c(pad.x, pad.y);
    fire("pointermove", p0[0], p0[1], 0, { buttons: 0 });
    await sleep(150);
    fire("pointerdown", p0[0], p0[1], 0);
    await sleep(40);
    fire("pointerup", p0[0], p0[1], 0);
    if (!t(await waitReply("start", n), "the route starts on R1's pad")) return;
    const p1 = w2c(pad.x, pad.y + 4_000_000);
    n = replies.length;
    fire("pointermove", p1[0], p1[1], 0, { buttons: 0 });
    await waitReply("move", n);
    await sleep(500);
    n = replies.length;
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
    const fix = await waitReply("fix", n, 6000);
    t(fix, "Enter fixes the route at the cursor (/api/route/fix)");
    t(!replies.slice(n).some((r) => r.name === "finish"), "and does not finish it");
    t(E.state().tool === "route", `the router is still routing (${E.state().tool})`);
    n = replies.length;
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "End", code: "End", bubbles: true, cancelable: true }));
    await sleep(1500);
    t(replies.slice(n).some((r) => r.name === "finish" || r.name === "fix"), `End finishes it (${replies.slice(n).map((r) => r.name).join(", ")})`);
    await leaveTool();
    await restore();
    t(snapshot(await state()) === original, "undone");
  });

  await scenario("the Measure tool shows x, y, r and θ next to the ruler's end, with its ticks, on the board and the Footprint Editor", async (t) => {
    await clickTab("PCB");
    await E.run("common.Interactive.measureTool");
    await sleep(300);
    const click = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, 0, { buttons: 0 }); await sleep(120); fire("pointerdown", x, y, 0); await sleep(40); fire("pointerup", x, y, 0); await sleep(250); };
    await click(20_000_000, 20_000_000);
    await click(23_000_000, 16_000_000);
    await redraw();
    const r = window.__edaRuler?.pcb;
    const want = units === "mm" ? ["x: 3.000 mm", "y: -4.000 mm", "r: 5.000 mm", "θ: 53.1°"] : null;
    t(r && (want ? want.every((s) => r.texts.includes(s)) : r.texts.some((s) => s.startsWith("θ: 53.1"))), `the board's ruler: ${JSON.stringify(r?.texts?.filter((s) => s.includes(":")))}`);
    t(r && r.segs > 10, `with its ticks (${r?.segs} strokes)`);
    await leaveTool();
    await clickTab("Footprint");
    await E.run("common.Interactive.measureTool");
    await sleep(300);
    await click(0, 0);
    await click(2_540_000, 0);
    await redraw();
    const f = window.__edaRuler?.footprint;
    t(f && f.texts.some((s) => s.startsWith("r: ")) && f.texts.includes("θ: 0.0°"), `the Footprint Editor's ruler: ${JSON.stringify(f?.texts?.filter((s) => s.includes(":")))}`);
    await E.run("common.Interactive.cancel");
    await sleep(200);
    await clickTab("PCB");
  });

  await restore();
  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
