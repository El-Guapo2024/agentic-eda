// UI-LEFT-3's common lane (C3-4, C3-5, C3-6, C3-7, C3-9, C3-10, C3-12, Display Options' Highlight thickness; PARITY-common.md "common v17"), on a placed mcu30:
//   hotkeys     Preferences > Hotkeys lists a "3D Viewer" section (View Top: Z) first; a click on the Hotkey column's title sorts the rows by it (▲), a second
//               turns it round (▼, the sections too)
//   grids       Edit Grids opens Preferences on the PCB Editor's Grids page; a grid of the same X as another (1 mm x 2 mm beside 1 mm) chosen on that page stays the
//               current one (`last_size_idx`: drawn 1 x 2 mm, the grid box shows it), Next Grid steps from its row; Reset Grids to Defaults puts the list back
//   popup       Next Grid on the schematic shows the "Grid" popup with the current grid selected, half a second; the board's inactive-layer cycle shows "Inactive
//               Layer Display"; with Common's "Show popup indicator when toggling settings with hotkeys" off, nothing shows
//   ruler       the Measure tool's ruler is drawn over its drop shadow: the shadow pass has as many strokes and texts, wider, in black or white
//   editing     PCB Editor > Editing Options' four rows are live: Show page limits off takes the sheet's outline away; a mouse drag of a track is the router's
//               drag (the drag tool, /api/route/drag_start) with "Drag (45 degree mode)", a move with "Move"; Ctrl+click with "Highlight net" highlights the
//               track's net and leaves the selection as it was
//   display     Schematic Editor > Display Options has Selection's "Highlight thickness:" (2), kept on OK, reset by the page's reset
//   colors      PCB Editor > Colors and Schematic Editor > Colors show a preview at the right of the swatches; Reset gives Sheet references a colour; Wires pasted
//               the background's colour make OK ask "Some items have the same color as the background ..." -- No keeps the dialog up
//   end         everything put back, no console errors
// It answers through `window.__commonV17Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__commonV17Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const state = () => json("/api/state");
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const m = /\/api\/route\/([a-z_]+)/.exec(url);
    if (m) replies.push({ name: m[1], body: await res.clone().json().catch(() => null) });
    return res;
  };

  // ------------------------------------------------------------------------------------------------------------------------------------------------ the page
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const prefs = () => tid("preferences-dialog");
  const buttonIn = (root, text) => root && [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const click = async (el, wait = 250) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const openPrefs = async (page) => { if (!prefs()) { await E.run("common.SuiteControl.openPreferences"); await sleep(400); } await click(tid(`prefs-page-${page}`), 400); };
  const ok = async () => { await click(tid("preferences-ok"), 500); };
  const cancel = async () => { if (prefs()) await click(buttonIn(prefs(), "Cancel"), 400); };
  const setValue = async (el, v) => { const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v)); el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(200); };
  const container = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => container().querySelector("canvas");
  const view = () => E.state().view;
  const w2c = (wx, wy) => { const v = view(), r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : mods.held === false ? 0 : 1, ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const redraw = async () => { await E.run("common.Control.zoomRedraw").catch(() => null); await sleep(400); };
  const tracksOf = (st) => st.routing?.tracks ?? [];

  await until(() => E.state().tab === "pcb" && container() && canvas() && view()?.scale > 0, 30000);
  localStorage.removeItem("eda-studio.hotkeys.v1");

  await scenario("hotkeys: a 3D Viewer section first; the Hotkey column sorts the rows, a second click the other way round", async (t) => {
    await openPrefs("hotkeys");
    const panel = tid("hotkeys-panel");
    const sections = () => [...panel.querySelectorAll("tr[data-hotkey-section]")].map((r) => r.getAttribute("data-hotkey-section"));
    t(sections()[0] === "3D Viewer", `the sections: ${sections().join(", ")}`);
    const top = panel.querySelector('tr[data-hotkey-action="3DViewer.Control.viewTop"]');
    t(top?.querySelector('td[data-col="hotkey"]')?.textContent === "Z", `View Top is Z (${top?.querySelector('td[data-col="hotkey"]')?.textContent})`);
    const keysOf = (section) => {
      const rows = [...panel.querySelectorAll("tr[data-hotkey-section], tr[data-hotkey-label]")];
      const at = rows.findIndex((r) => r.getAttribute("data-hotkey-section") === section);
      const out = [];
      for (const r of rows.slice(at + 1)) { if (r.hasAttribute("data-hotkey-section")) break; out.push(r.querySelector('td[data-col="hotkey"]')?.textContent ?? ""); }
      return out;
    };
    const th = panel.querySelector('th[data-hotkey-column="1"]');
    await click(th);
    const asc = keysOf("3D Viewer");
    t(th.getAttribute("aria-sort") === "ascending" && th.textContent.includes("▲"), `sorted up: ${th.textContent}`);
    t(asc.every((k, i) => i === 0 || asc[i - 1] <= k), `the 3D Viewer rows by their hotkey: ${asc.slice(0, 12).join("|")}`);
    await click(th);
    const desc = keysOf("3D Viewer");
    t(th.getAttribute("aria-sort") === "descending" && desc.every((k, i) => i === 0 || desc[i - 1] >= k), `the other way: ${desc.slice(0, 6).join("|")}`);
    t(sections()[0] === "Gestures" && sections().at(-1) === "3D Viewer", `the sections turned round too: ${sections().join(", ")}`);
    await cancel();
  });

  await scenario("grids: Edit Grids is the Preferences' Grids page; of two grids with the same X the one chosen stays current", async (t) => {
    await E.run("common.Control.editGrids");
    await sleep(500);
    t(prefs()?.getAttribute("aria-label") === "Preferences - Grids" && tid("prefs-page-pcb_grids")?.getAttribute("aria-selected") === "true", `Preferences on the PCB Editor's Grids page (${prefs()?.getAttribute("aria-label")})`);
    const page = () => prefs().querySelector('[data-grids-page="pcb"]');
    const list = () => page().querySelector('select[aria-label="Grids"]');
    // a 1 mm x 2 mm grid beside the default 1 mm one
    const at = [...list().options].findIndex((o) => /^1(\.0+)? mm \(/.test(o.textContent) || /^39\.3\d* mil/.test(o.textContent));
    t(at >= 0, `the 1 mm grid is in the list (row ${at})`);
    await setValue(list(), at);
    await click(buttonIn(page(), "+") ?? page().querySelector('[aria-label="Add Grid"]'));
    const sub = page().querySelector('[aria-label="Grid Settings"]');
    const units = E.state().units ?? "mm";
    const mm = units === "mm" ? 1 : units === "mil" ? 1000 / 25.4 : 1 / 25.4;
    await setValue(sub.querySelector('[aria-label="Grid name"]'), "Tall");
    await setValue(sub.querySelector('[aria-label="Grid size X"]'), 1 * mm);
    sub.querySelector('[aria-label="Linked"]').click();
    await sleep(100);
    await setValue(sub.querySelector('[aria-label="Grid size Y"]'), 2 * mm);
    await click(buttonIn(sub, "OK"));
    const row = list().selectedIndex;
    t(/^Tall: /.test(list().options[row]?.textContent ?? ""), `added and selected: "${list().options[row]?.textContent}"`);
    await ok();
    await redraw();
    const g = window.__edaGrid?.pcb;
    t(g && Math.abs(g.size.x - 1_000_000) < 1 && Math.abs(g.size.y - 2_000_000) < 1, `drawn 1 x 2 mm, the row chosen (${JSON.stringify(g?.size)})`);
    const box = document.querySelector('.toolbar-control[title="Grid"] select');
    t(/^Tall: /.test(box?.options[box.selectedIndex]?.textContent ?? ""), `the grid box shows it: "${box?.options[box.selectedIndex]?.textContent}"`);
    // the square 1 mm grid from the grid box: drawn square, though the X is the same
    const square = [...box.options].findIndex((o, i) => i !== box.selectedIndex && /^(1(\.0+)? mm|39\.3\d* mil) \(/.test(o.textContent));
    await setValue(box, square);
    await redraw();
    t(Math.abs((window.__edaGrid?.pcb?.size.y ?? 0) - 1_000_000) < 1 && box.selectedIndex === square, `the square one picked in the grid box is drawn square (${JSON.stringify(window.__edaGrid?.pcb?.size)})`);
    // Reset Grids to Defaults
    await E.run("common.Control.editGrids");
    await sleep(400);
    await click(buttonIn(prefs(), "Reset Grids to Defaults"));
    t(![...list().options].some((o) => o.textContent.startsWith("Tall")), "Reset Grids to Defaults drops it");
    await ok();
  });

  await scenario("popup: Next Grid on the schematic, the board's inactive-layer cycle; none with Common's switch off", async (t) => {
    await clickTab("Schematic");
    await until(() => E.state().tab === "schematic" && E.state().grid > 0, 10000);
    await sleep(800);
    const popup = () => tid("hotkey-cycle-popup");
    await E.run("common.Control.gridNext");
    const shown = await until(() => popup(), 1500);
    t(!!shown && tid("hotkey-cycle-popup-title")?.textContent === "Grid", `the "Grid" popup (${tid("hotkey-cycle-popup-title")?.textContent})`);
    const sel = popup()?.querySelector('[aria-selected="true"]')?.textContent;
    t(!!sel && window.__edaHotkeyPopup?.items?.[window.__edaHotkeyPopup.selection] === sel, `the current grid selected: ${sel}`);
    t(await until(() => !popup(), 2000), "gone half a second later");
    await E.run("common.Control.gridPrev");
    await sleep(700);
    await clickTab("PCB");
    await E.run("common.Control.highContrastModeCycle");
    t(!!(await until(() => popup(), 1500)) && tid("hotkey-cycle-popup-title")?.textContent === "Inactive Layer Display", `the board's "Inactive Layer Display" (${tid("hotkey-cycle-popup-title")?.textContent})`);
    t(JSON.stringify(window.__edaHotkeyPopup?.items) === JSON.stringify(["Normal", "Dimmed", "Hidden"]), `Normal, Dimmed, Hidden (${JSON.stringify(window.__edaHotkeyPopup?.items)})`);
    await until(() => !popup(), 2000);
    await E.run("common.Control.highContrastModeCycle");
    await E.run("common.Control.highContrastModeCycle");
    await until(() => !popup(), 2000);
    // Common's switch off
    await openPrefs("common");
    tid("common-hotkey-feedback").click();
    await sleep(100);
    await ok();
    await E.run("common.Control.highContrastModeCycle");
    await sleep(400);
    t(!popup(), "off: no popup");
    await E.run("common.Control.highContrastModeCycle");
    await E.run("common.Control.highContrastModeCycle");
    await openPrefs("common");
    if (!tid("common-hotkey-feedback").checked) tid("common-hotkey-feedback").click();
    await sleep(100);
    await ok();
  });

  const st0 = await state();
  const outline = st0.outline ?? [];
  const xs = outline.map((p) => p[0]), ys = outline.map((p) => p[1]);
  const [bx0, by0, bx1, by1] = [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];

  await scenario("ruler: the Measure tool's ruler over its drop shadow", async (t) => {
    await E.run("common.Interactive.measureTool");
    await sleep(300);
    const clickW = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { held: false }); await sleep(120); fire("pointerdown", x, y); await sleep(40); fire("pointerup", x, y); await sleep(250); };
    await clickW(bx0 + 2_000_000, by0 + 6_000_000);
    await clickW(bx0 + 5_000_000, by0 + 2_000_000);
    await redraw();
    const r = window.__edaRuler?.pcb;
    t(r && r.shadow && r.shadow.segs === r.segs && r.shadow.texts === r.texts.length, `the shadow pass: ${JSON.stringify(r?.shadow)}, ${r?.segs} strokes`);
    t(r && (r.shadow.color === "#000000" || r.shadow.color === "#ffffff") && r.shadow.width > r.shadow.drawnWidth, `wider, black or white (${r?.shadow?.color}, ${r?.shadow?.width} > ${r?.shadow?.drawnWidth})`);
    await E.run("common.Interactive.cancel");
    await sleep(200);
    await E.run("common.Interactive.cancel");
    await sleep(200);
  });

  await scenario("editing: Show page limits, Track mouse-drag mode, Ctrl+click Highlight net", async (t) => {
    await E.run("common.Control.zoomFitScreen");
    await sleep(400);
    await redraw();
    t(!!window.__edaPageLimits, `the sheet's outline is drawn (${JSON.stringify(window.__edaPageLimits)})`);
    await openPrefs("pcb_edit");
    t(tid("pref-show-page-limits")?.checked === true && tid("pref-track-drag-mode")?.value === "drag" && tid("pref-ctrl-click-toggle")?.checked === true && tid("pref-show-selected-ratsnest")?.checked === true, "KiCad's defaults: page limits on, Drag (45 degree mode), Toggle selection, Always show selected ratsnest");
    tid("pref-show-page-limits").click();
    await sleep(100);
    await ok();
    await redraw();
    t(window.__edaPageLimits === null, "off: no outline");
    // a track of its own, in an empty corner of the board
    const net = (st0.nets ?? []).map((n) => (typeof n === "string" ? n : n.name)).find((n) => n && n !== "GND") ?? "";
    // the drags are of what the mouse does, not of the board's checks: Strict off while they run (as board-stats.check.js does), back after
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    const strictWas = strict?.checked ?? false;
    if (strict?.checked) strict.click();
    await sleep(200);
    const y = by0 + 1_500_000, x0 = bx0 + 3_000_000, x1 = bx0 + 9_000_000;
    const added = await cmd({ op: "add_track", net, layer: "F.Cu", width: 250_000, pts: [{ x: x0, y }, { x: x1, y }] });
    t(added.ok, `a track (${added.message ?? "added"})`);
    await sleep(900);
    const track = tracksOf(await state()).find((tr) => tr.pts[0][1] === y && tr.pts[0][0] === x0);
    // a mouse drag from its middle, the track selected first (with Preferences' default "Left button drag: Drag selected objects" a drag from an item not
    // selected draws the selection area: `m_selection.Empty() && dragAction != DRAG_ANY`): the router's drag
    const drag = async (fromW, toW) => {
      const [sx, sy] = w2c(...fromW);
      const [ex, ey] = w2c(...toW);
      fire("pointermove", sx, sy, { held: false });
      await sleep(150);
      fire("pointerdown", sx, sy);
      await sleep(40);
      fire("pointerup", sx, sy);
      await sleep(400);
      fire("pointermove", sx, sy, { held: false });
      await sleep(150);
      fire("pointerdown", sx, sy);
      await sleep(60);
      for (let i = 1; i <= 6; i++) { fire("pointermove", sx + ((ex - sx) * i) / 6, sy + ((ey - sy) * i) / 6); await sleep(120); }
      await sleep(600);
      const during = E.state().tool;
      fire("pointerup", ex, ey);
      await sleep(1200);
      return during;
    };
    let n = replies.length;
    // up towards the board's edge, away from R1 and its reference below: the router's commit is judged as Strict judges it
    const during = await drag([(x0 + x1) / 2, y], [(x0 + x1) / 2, y - 800_000]);
    t(during === "drag" && replies.slice(n).some((r) => r.name === "drag_start"), `Drag (45 degree mode): the router's drag (${during}; ${replies.slice(n).map((r) => r.name).join(", ")})`);
    t(E.state().tool === "select", `the release fixes it (${E.state().tool})`);
    const original = JSON.stringify(track?.pts);
    const shapeNow = async () => JSON.stringify(tracksOf(await state()).filter((tr) => tr.net === net && tr.layer === "F.Cu").map((tr) => tr.pts).filter((pts) => pts[0][0] === x0 || pts.at(-1)[0] === x1 || pts.some((p) => p[0] > x0 && p[0] < x1)));
    const dragged = await until(async () => { const sh = await shapeNow(); return sh !== JSON.stringify([JSON.parse(original)]) && JSON.parse(sh).flat().some((p) => p[1] < y - 300_000) ? sh : null; }, 4000);
    t(!!dragged, `the track dragged up: ${dragged ?? (await shapeNow())}; ${JSON.stringify(replies.slice(n).filter((r) => r.name === "drag_finish").map((r) => r.body)).slice(0, 300)}; strict ${strict?.checked} ${!!strict}`);
    const undoBack = async () => { for (let i = 0; i < 4 && JSON.stringify(tracksOf(await state()).find((tr) => tr.id === track?.id)?.pts) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(900); } };
    await undoBack();
    t(JSON.stringify(tracksOf(await state()).find((tr) => tr.id === track?.id)?.pts) === original, "undone");
    // Move
    await openPrefs("pcb_edit");
    await setValue(tid("pref-track-drag-mode"), "move");
    await ok();
    n = replies.length;
    const duringMove = await drag([(x0 + x1) / 2, y], [(x0 + x1) / 2, y + 1_000_000]);
    t(duringMove !== "drag" && !replies.slice(n).some((r) => r.name === "drag_start"), `Move: no router drag (${duringMove})`);
    const moved = await until(async () => { const m = tracksOf(await state()).find((tr) => tr.id === track?.id); return m && m.pts[0][1] !== y && m.pts[0][1] === m.pts[1][1] && m.pts[1][0] - m.pts[0][0] === x1 - x0 ? m : null; }, 4000);
    t(!!moved, `the track moved whole (${JSON.stringify(moved?.pts ?? tracksOf(await state()).find((tr) => tr.id === track?.id)?.pts)})`);
    await undoBack();
    // Ctrl+click: Highlight net
    await openPrefs("pcb_edit");
    await setValue(tid("pref-track-drag-mode"), "drag");
    tid("pref-ctrl-click-highlight").click();
    await sleep(100);
    await ok();
    await E.run("pcbnew.EditorControl.clearHighlight");
    await sleep(200);
    const selBefore = JSON.stringify(E.state().selection);
    const [cx, cy] = w2c((x0 + x1) / 2, y);
    fire("pointermove", cx, cy, { held: false, ctrl: true });
    await sleep(200);
    fire("pointerdown", cx, cy, { ctrl: true });
    await sleep(40);
    fire("pointerup", cx, cy, { ctrl: true });
    await sleep(500);
    t(JSON.stringify(E.state().highlight) === JSON.stringify([net]), `the net highlighted (${JSON.stringify(E.state().highlight)})`);
    t(JSON.stringify(E.state().selection) === selBefore, "the selection left as it was");
    await E.run("pcbnew.EditorControl.clearHighlight");
    // the defaults back
    await openPrefs("pcb_edit");
    await click(tid("preferences-reset"));
    await ok();
    await redraw();
    t(!!window.__edaPageLimits, "Reset to Defaults: the outline back");
    // the track taken away again (`add_track`'s undo step)
    for (let i = 0; i < 4 && tracksOf(await state()).some((tr) => tr.id === track?.id); i++) { await E.run("common.Interactive.undo"); await sleep(800); }
    t(!tracksOf(await state()).some((tr) => tr.id === track?.id), "the track undone");
    if (strictWas && strict && !strict.checked) strict.click();
  });

  await scenario("display: Schematic Editor > Display Options' Highlight thickness", async (t) => {
    await openPrefs("display_schematic");
    const box = tid("pref-sch-highlight-thickness");
    t(box && box.value === "2", `KiCad's 2 (${box?.value})`);
    await setValue(box, 7);
    await ok();
    await openPrefs("display_schematic");
    t(tid("pref-sch-highlight-thickness")?.value === "7", "kept on OK");
    await click(tid("preferences-reset"));
    t(tid("pref-sch-highlight-thickness")?.value === "2", "the page's reset puts 2 back");
    await ok();
  });

  await scenario("colors: the previews, Reset's colour for Sheet references, validateSave's question", async (t) => {
    await openPrefs("pcb_colors");
    await sleep(300);
    const pcbPreview = tid("pcb-colors-preview");
    t(!!pcbPreview && pcbPreview.getBoundingClientRect().width > 100 && !!window.__edaColorPreview?.pcb, "PCB Editor > Colors: the preview board at the right of the swatches");
    await openPrefs("sch_colors");
    await sleep(300);
    t(!!tid("sch-colors-preview") && !!window.__edaColorPreview?.schematic, `Schematic Editor > Colors: the preview sheet (${JSON.stringify(window.__edaColorPreview?.schematic)})`);
    // a theme of our own
    const page = tid("sch-colors-page");
    const theme = tid("sch-colors-theme");
    t(!!theme, "the theme list");
    await click(tid("preferences-reset"));
    const refs = page.querySelector('[data-color-key="LAYER_INTERSHEET_REFS"] [role="button"] span');
    t(refs && refs.style.background && refs.style.background !== "", `Reset gives Sheet references a colour (${refs?.style.background})`);
    // Wires the background's colour: Copy color on Background, Paste color on Wires
    const swatch = (key) => page.querySelector(`[data-color-key="${key}"] [role="button"]`);
    const menu = async (key, label) => {
      const s = swatch(key), r = s.getBoundingClientRect();
      s.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 5, clientY: r.top + 5 }));
      await sleep(250);
      const item = [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].find((m) => m.textContent.trim() === label);
      await click(item);
    };
    await menu("LAYER_SCHEMATIC_BACKGROUND", "Copy color");
    await menu("LAYER_WIRE", "Paste color");
    await click(tid("preferences-ok"), 600);
    const q = [...document.querySelectorAll('[role="dialog"]')].find((d) => /same color as the background/.test(d.textContent));
    t(!!q && /Some items have the same color as the background/.test(q.textContent), `OK asks KiCad's question (${q?.textContent?.slice(0, 80)})`);
    if (q) await click(buttonIn(q, "No") ?? buttonIn(q, "Cancel"), 400);
    t(!!prefs(), "No keeps the dialog up");
    await click(tid("preferences-reset"));
    await ok();
    t(!prefs(), "reset, OK closes it without a question");
  });

  await scenario("end: everything put back, no console errors", async (t) => {
    if (prefs()) await cancel();
    t((E.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(E.errors?.() ?? []).slice(0, 300)})`);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
