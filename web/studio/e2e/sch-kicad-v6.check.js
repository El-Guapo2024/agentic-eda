// PARITY-sch §82 (`task-v6-kicad-sch`): two behaviours switched to KiCad's.
// 1. The project's reference-designator tracker (`REFDES_TRACKER`, `used_designators`) is not undone: with "Allow reference reuse" off, a resistor placed by Place
//    Symbol (A) and taken back by Ctrl+Z keeps its number given -- the server's next reference skips it; Ctrl+Y puts the resistor back as it was.
// 2. Place Power with Preferences' "Power Symbols:" Local, then Global, of the same rail on one sheet: the second is not refused; the sheet has the rail in both
//    kinds (`SCH_SCREEN::Append`'s renamed cache entry, `+5V_1`), so its net is offered to a local label and to a global one; one undo takes the second back.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// on a COPY of a board: every scenario undoes what it did; the tracker keeps what it learned (as KiCad's does), and the preference is put back.
//
// It answers through `window.__schKicadV6Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__schKicadV6Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schKicadV6Sched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 8e6;
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
    window.__schKicadV6Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = () => json("/api/schematic");
  const run = async (id, args) => { const r = await __eda.run(id, args); await sleep(250); return r; };
  const state = () => __eda.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(900); };
  const contextAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    fire("pointermove", px, py);
    await sleep(200);
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, button: 2 }));
    await sleep(500);
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const newOf = (before, list) => list.filter((x) => !before.some((b) => b.id === x.id && b.unit === x.unit));
  /** The item in the hand, once its drawing has arrived. */
  const carried = (pred = () => true) => until(() => (state().carry?.drawn && pred(state().carry) ? state().carry : null), 10000);
  /** The dialog open now, by its title. */
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 8000);
  /** The chooser's search for `query`, then its row `id` chosen. */
  const choose = async (dlg, query, id) => {
    setValue(dlg.querySelector("[data-chooser-search]"), query);
    await until(() => dlg.querySelector(`[role=treeitem][data-id="${id}"]`), 40000);
    dlg.querySelector(`[role=treeitem][data-id="${id}"]`)?.click();
    await sleep(500);
    return !!dlg.querySelector(`[role=treeitem][data-id="${id}"]`);
  };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(700); };
  const existing = (kind) => json(`/api/sch/existing_labels?kind=${kind}`).then((r) => r.labels ?? []);
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  /** Preferences' "Power Symbols:" as the studio stored it (`eda-studio.preferences.v1`). */
  const powerPref = () => { try { return JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}")?.schNewPowerSymbols ?? 0; } catch { return 0; } };
  /** Preferences > Schematic Editor > Editing Options' "Power Symbols:" set to `value` (0 Default, 1 Global, 2 Local) through the dialog. */
  const setPowerPref = async (value) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => tid("preferences-dialog"));
    tid("prefs-page-sch_edit")?.click();
    await sleep(300);
    const choice = tid("pref-sch-power-symbols");
    if (!choice || choice.disabled) return false;
    setValue(choice, String(value));
    await sleep(150);
    tid("preferences-ok")?.click();
    await sleep(500);
    return powerPref() === value;
  };

  /** The root sheet as the scenarios change it. */
  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at]), power: s.power_symbols.map((p) => [p.id, p.lib_id, p.at]) });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
  await leave();
  await fit();
  const v0 = state().view;
  if (!v0 || !Number.isFinite(v0.scale) || v0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(v0)})`] }); check.done = true; return "no canvas size"; }
  base = await snapshot();
  // An empty part of the root sheet (below and left of the module sheets), on the 50 mil grid.
  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 110 * G;
  const setReuse = (reuse) => cmd({ op: "set_annotation_settings", settings: { sort_order: 0, method: 0, start_num: 0, reuse_designators: reuse }, subpart_first_id: 65, subpart_id_separator: 0 });
  const nextRef = async () => (await json("/api/sch/next_reference", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sheet: "", prefix: "R", lib_id: "Device:R", value: "R", unit: 1, unit_count: 1, options: { algo: "first_free", start_number: 0 } }) })).reference;

  (async () => {
    await scenario("The reference tracker is not undone: a placed resistor taken back by Undo keeps its number given (reuse off); Redo puts it back", async (t) => {
      const s0 = await sch();
      const off = await setReuse(false);
      if (!t(off.ok, `Schematic Setup's "Allow reference reuse" off (${off.message})`)) return;
      await sleep(600);
      await hoverAt(X0, Y0);
      await run("eeschema.InteractiveDrawing.placeSymbol");
      const dlg = await opened(/^Symbol Chooser/);
      if (!t(!!dlg && (await choose(dlg, "Device:R", "Device:R")), "the chooser finds Device:R")) return;
      if (dlg.querySelector("[data-chooser-keep]")?.checked) dlg.querySelector("[data-chooser-keep]").click();
      buttonText(dlg, "OK").click();
      const c1 = await carried((x) => x.ref && !/\?/.test(x.ref));
      if (!t(c1?.kind === "symbol" && /^R\d+$/.test(c1.ref ?? ""), `a resistor rides (${JSON.stringify(c1)})`)) return;
      await sleep(1000); // the server's `annotate()` answer replaces the page's guess while it rides
      const ref1 = state().carry?.ref ?? c1.ref;
      await clickAt(X0, Y0);
      await escape();
      await toSelect();
      const placed = newOf(s0.symbols, (await sch()).symbols);
      if (!t(placed.length === 1 && placed[0].id === ref1, `${ref1} is placed (${placed.map((x) => x.id)})`)) return;
      const n = (r) => Number(r.slice(1));
      await undo();
      t(newOf(s0.symbols, (await sch()).symbols).length === 0, `Ctrl+Z takes ${ref1} back`);
      const asked = await nextRef();
      t(asked && n(asked) > n(ref1), `but not its number: with reuse off the server's next reference skips ${ref1} (${asked})`);
      await redo();
      const again = newOf(s0.symbols, (await sch()).symbols);
      t(again.length === 1 && again[0].id === ref1, `Ctrl+Y puts ${ref1} back (${again.map((x) => x.id)})`);
      await undo();
      const on = await setReuse(true);
      t(on.ok, "reuse back on");
      await sleep(600);
      t((await nextRef()) === ref1, `with reuse on the server gives ${ref1} again`);
    });

    await scenario("Place Power, Local then Global, of one rail on one sheet: both are placed (a renamed cache entry), the net is a local and a global one; one undo each", async (t) => {
      const was = powerPref();
      try {
        if (!t(await setPowerPref(2), "Preferences' Power Symbols: Local")) return;
        const used = new Set([...(await existing("local")), ...(await existing("global"))]);
        let pick = null;
        const place = async (x, y, kind) => {
          await hoverAt(x, y);
          await run("eeschema.InteractiveDrawing.placePowerSymbol");
          const dlg = await opened(/^Choose Power Symbol/);
          if (!dlg) return null;
          const search = dlg.querySelector("[data-chooser-search]");
          setValue(search, pick ? pick.slice(pick.indexOf(":") + 1) : "+");
          await until(() => /match/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? "") && !/Searching/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? ""), 60000);
          await sleep(600);
          if (!pick) {
            const ids = [...dlg.querySelectorAll("[role=treeitem][data-id]")].map((r) => r.getAttribute("data-id")).filter((id) => id.includes(":"));
            pick = ids.find((id) => !used.has(id.slice(id.indexOf(":") + 1))) ?? null;
            if (!pick) return null;
          }
          await until(() => dlg.querySelector(`[role=treeitem][data-id="${pick}"]`), 20000);
          dlg.querySelector(`[role=treeitem][data-id="${pick}"]`)?.click();
          await sleep(400);
          buttonText(dlg, "OK").click();
          const c = await carried();
          const s0 = await sch();
          await clickAt(x, y);
          await toSelect();
          const made = (await sch()).power_symbols.filter((p) => !s0.power_symbols.some((q) => q.id === p.id));
          return { carried: c?.powerKind, made };
        };
        const first = await place(X0 + 10 * G, Y0 + 12 * G);
        if (!t(!!first && first.made.length === 1 && first.carried === "local", `a local ${pick} placed (${JSON.stringify(first && [first.carried, first.made.map((p) => p.id)])})`)) return;
        const net = first.made[0].net;
        t((await existing("local")).includes(net) && !(await existing("global")).includes(net), `${net} is a local power net`);
        if (!t(await setPowerPref(1), "Preferences' Power Symbols: Global")) return;
        const second = await place(X0 + 20 * G, Y0 + 12 * G);
        t(!!second && second.made.length === 1 && second.carried === "global", `the same rail placed as a global one, not refused (${JSON.stringify(second && [second.carried, second.made.map((p) => p.id)])})`);
        t((await existing("local")).includes(net) && (await existing("global")).includes(net), `the sheet has ${net} in both kinds: offered to a local label and to a global one`);
        await undo();
        t((await existing("local")).includes(net) && !(await existing("global")).includes(net), "one undo takes the global one back; the local one stays");
        await undo();
        t(!(await existing("local")).includes(net), "a second undo takes the local one back");
      } finally {
        await closeDialogs();
        if (powerPref() !== was) await setPowerPref(was);
        t(powerPref() === was, "the preference is put back");
      }
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
