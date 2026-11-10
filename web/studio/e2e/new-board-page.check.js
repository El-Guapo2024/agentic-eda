// A new board and the left button's default, as KiCad has them (UI-LEFT-2 CC3, CC4; PARITY-common.md section 23):
//   page    `eda board new` puts the board's outline in the middle of its page (`BOARD_NETLIST_UPDATER::estimateFootprintInsertionPosition`: the page's centre on
//           an empty board), inside the drawing sheet's frame (the default sheet's 10 mm margins), and the sheet is drawn from the start
//           (`GAL_SET::DefaultVisible`'s LAYER_DRAWINGSHEET, `m_ShowPageLimits( true )`);
//   drag    with nothing stored, Preferences > Mouse and Touchpad's "Left button drag:" is "Drag selected objects; otherwise draw selection rectangle"
//           (`COMMON_SETTINGS`'s `input.mouse_left` DRAG_SELECTED): a drag from a footprint not selected leaves it and draws the area, a drag from it once it is
//           selected moves it (`PCB_SELECTION_TOOL::Main`'s `IsDrag( BUT_LEFT )`), undone in one step; a choice saved in Preferences is the one read back.
// Runs on a fresh `eda board new examples/mcu_board_30plus.yaml` board made with no `--origin` (run-all-checks' mcu30-new).
// It answers through `window.__newBoardPageCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-new minutes=6
(async () => {
  const results = [];
  const check = (window.__newBoardPageCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };

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
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(900); };
  const prefsDialog = () => document.querySelector('[data-testid="preferences-dialog"]');
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(80); };
  const buttonIn = (root, label) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label || b.getAttribute("aria-label") === label);
  /** Opens Preferences on Mouse and Touchpad, hands `fn` the dialog, then presses `close` (OK or Cancel). */
  const mousePrefs = async (fn, close = "OK") => {
    await E.run("common.SuiteControl.openPreferences");
    const d = await until(() => prefsDialog(), 4000);
    if (!d) throw new Error("Preferences did not open");
    [...d.querySelectorAll('[role="treeitem"]')].find((x) => x.textContent.trim() === "Mouse and Touchpad")?.click();
    await sleep(200);
    const out = await fn(prefsDialog());
    buttonIn(prefsDialog(), close)?.click();
    await sleep(300);
    return out;
  };
  const state = () => json("/api/state");
  const partOf = (st, ref) => st.parts.find((p) => p.ref === ref);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { for (const d of document.querySelectorAll(".dialog")) buttonIn(d, "Cancel")?.click(); } catch { /* best effort */ }
    results.push(entry);
  };
  const MM = 1_000_000;

  if (E.state().tab !== "pcb") await clickTab("PCB");
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const before = await state();
  const xs = (before.outline ?? []).map((p) => p[0]), ys = (before.outline ?? []).map((p) => p[1]);
  const box = xs.length ? [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)] : null;

  await scenario("a new board lies in the middle of its page, inside the drawing sheet's frame, and the sheet is drawn from the start", async (t) => {
    if (!t(box, `the board has an outline (${JSON.stringify(before.outline)})`)) return;
    await E.run("common.Control.zoomFitScreen");
    const p = await until(async () => { await E.run("common.Control.zoomRedraw").catch(() => null); await sleep(300); return (window.__edaDrawingSheet ?? {}).pcb; }, 15000);
    if (!t(p, "the board canvas painted the drawing sheet")) return;
    const [pw, ph] = p.page;
    t(pw === 297 * MM && ph === 210 * MM, `on A4 landscape (${pw / MM} x ${ph / MM} mm)`);
    const [cx, cy] = [(box[0] + box[2]) / 2, (box[1] + box[3]) / 2];
    t(Math.abs(cx - pw / 2) <= MM && Math.abs(cy - ph / 2) <= MM, `its centre (${cx / MM}, ${cy / MM}) mm is the page's (${pw / 2 / MM}, ${ph / 2 / MM}) to a millimetre`);
    t(box[0] >= 10 * MM && box[1] >= 10 * MM && box[2] <= pw - 10 * MM && box[3] <= ph - 10 * MM, `inside the frame's 10 mm margins (${box.map((v) => v / MM).join(", ")} mm)`);
    t(box[0] % MM === 0 && box[1] % MM === 0, "its corner on a whole millimetre");
    t(["1", "2", "3", "4", "5", "6"].every((n) => p.texts.includes(n)) && ["A", "B", "C", "D"].every((l) => p.texts.includes(l)), "with the default sheet's zone references");
  });

  await scenario("Left button drag starts as KiCad's Drag selected objects: a footprint not selected stays and the area is drawn, selected it moves", async (t) => {
    let stored = null;
    try { stored = localStorage.getItem("eda-studio.preferences.v1"); } catch { /* storage blocked */ }
    // the studio stores its preferences on start, so a fresh browser holds the defaults, version 2 (PREFERENCES_SCHEMA_VERSION)
    const kept = stored ? JSON.parse(stored) : null;
    t(kept == null || (kept.dragLeft === "drag_selected" && kept.schemaVersion === 2), `nothing chosen yet: ${kept ? `stored ${kept.dragLeft}, version ${kept.schemaVersion}` : "nothing stored"}`);
    const shown = await mousePrefs(async (d) => d.querySelector('[data-testid="pref-drag-left"]')?.value, "Cancel");
    t(shown === "drag_selected", `Preferences > Mouse and Touchpad shows "Drag selected objects; otherwise draw selection rectangle" (${shown})`);
    // R1 in the middle of the outline, where a drag of a few millimetres keeps it on the board
    const mid = [Math.round((box[0] + box[2]) / 2 / MM) * MM, Math.round((box[1] + box[3]) / 2 / MM) * MM];
    await cmd({ op: "place_at", part: "R1", x: mid[0], y: mid[1] });
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
    const st1 = await state();
    const r1 = partOf(st1, "R1");
    if (!t(r1?.placed, "R1 placed in the middle of the board")) return;
    const body = bodyPoint(st1, r1, 1 / view().scale);
    const at = () => w2c(body[0], body[1]);
    const moved = async () => { const p = partOf(await state(), "R1"); return p.at[0] !== r1.at[0] || p.at[1] !== r1.at[1]; };
    await E.run("common.Interactive.cancel");
    await sleep(200);
    t(E.state().selection.length === 0, "nothing selected");
    await drag(at(), 60, 40, 0);
    t(!(await moved()), "a drag from R1 not selected leaves it where it is");
    E.run("common.InteractiveSelection.SelectItem", "R1").catch(() => null);
    await sleep(200);
    if (!E.state().selection.some((s) => s.id === "R1")) { fire("pointerdown", ...at(), 0); await sleep(40); fire("pointerup", ...at(), 0); await sleep(300); }
    t(E.state().selection.some((s) => s.id === "R1"), `R1 selected (${JSON.stringify(E.state().selection)})`);
    await drag(at(), 60, 40, 0);
    t(await moved(), "selected, a drag from it moves it");
    await E.run("common.Interactive.undo");
    await sleep(600);
    t(!(await moved()), "one undo puts it back");
  });

  await scenario("a choice saved in Preferences is kept over the default", async (t) => {
    await mousePrefs(async (d) => choose(d.querySelector('[data-testid="pref-drag-left"]'), "drag_any"));
    let stored = null;
    try { stored = JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "null"); } catch { /* storage blocked */ }
    t(stored?.dragLeft === "drag_any", `Drag any object saved (${stored?.dragLeft})`);
    const shown = await mousePrefs(async (d) => d.querySelector('[data-testid="pref-drag-left"]')?.value, "Cancel");
    t(shown === "drag_any", `and shown on reopening (${shown})`);
    await mousePrefs(async (d) => choose(d.querySelector('[data-testid="pref-drag-left"]'), "drag_selected"));
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
