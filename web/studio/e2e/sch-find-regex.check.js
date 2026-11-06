// UI-LEFT S12 (PARITY-sch §60): Find / Replace's Regular Expression (`EDA_SEARCH_MATCH_MODE::REGEX`, `EDA_ITEM::Matches` / `Replace`: sub-expressions in the
// replacement) over a symbol's pins (`searchAllPins`), a sheet's pins (`SCH_SHEET_PIN`, replaceable) and a sheet's name (`SHEET_NAME`), not its file.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with KiCad's libraries (`KICAD_PATH`: named pins), the schematic tab open, on a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schFindRegexCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schFindRegexCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schFindRegexCheckSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
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
    window.__schFindRegexCheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogTitled = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const closeDialogs = async () => {
    for (let i = 0; i < 5; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].reverse().find((x) => ["Cancel", "Close", "No", "OK", "Discard Changes"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(300);
    }
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(400); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1200); };
  const dragFrom = async (x, y, dxNm, dyNm) => {
    const [px, py] = screenOf(x, y);
    const [qx, qy] = screenOf(x + dxNm, y + dyNm);
    fire("pointermove", px, py); await sleep(400);
    fire("pointerdown", px, py, { buttons: 1 }); await sleep(120);
    for (let k = 1; k <= 6; k++) { fire("pointermove", px + ((qx - px) * k) / 6, py + ((qy - py) * k) / 6, { buttons: 1 }); await sleep(120); }
    fire("pointerup", qx, qy); await sleep(1600);
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  /** Frame the items `ids` (Zoom to Selection), then leave nothing selected (the selection colour would hide the items' own). */
  const frame = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(700); await select([]); await sleep(400); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(1100); await select([]); await fit(); };
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** How many canvas pixels within `tol` of `rgb` lie in a square of `half` canvas pixels about the sheet's point (x, y) (nm). */
  const countAt = (x, y, half, rgb, tol = 40) => {
    const c = canvas();
    if (!c) return -1;
    const r = c.getBoundingClientRect();
    const v = __eda.state().view;
    const k = c.width / r.width;
    const px = Math.round((v.x + x * v.scale) * k), py = Math.round((v.y + y * v.scale) * k);
    const d = c.getContext("2d").getImageData(px - half, py - half, 2 * half + 1, 2 * half + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  /** What the scenarios touch: the root's sheets (names, pins) and the first sheet's symbols. */
  const snapshot = async () => {
    const root = await sch("");
    return JSON.stringify({ sheets: root.sheets.map((s) => [s.id, s.name, s.pins.map((p) => p.name)]), labels: root.labels.map((l) => [l.id, l.net]) });
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 16; i++) {
      if ((await snapshot()) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const reEscape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const toggle = (dlg, label) => [...dlg.querySelectorAll("label.toggle")].find((l) => l.textContent.trim() === label)?.querySelector("input");
  const tick = async (dlg, label, on = true) => { const box = toggle(dlg, label); if (box && box.checked !== on) { box.click(); await sleep(250); } return box; };
  const findDialog = async (replace) => {
    await __eda.run(replace ? "common.Interactive.findAndReplace" : "common.Interactive.find");
    return until(() => dialogTitled(replace ? /^Find and Replace/ : /^Find$/));
  };
  const searchBox = (dlg) => dlg.querySelector(".kv-grid input");
  const replaceBox = (dlg) => dlg.querySelectorAll(".kv-grid input")[1];

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();
    const root0 = await sch("");

    await scenario("A regular expression finds a symbol's pin by its name (REGEX, searchAllPins) and Find Next selects it", async (t) => {
      // the sheet whose symbols have the most named pins
      let best = null;
      for (const s of root0.sheets) {
        const c = await sch(s.id);
        const pin = c.symbols.flatMap((x) => x.pins ?? []).find((p) => p.name && p.name.length >= 3 && p.name !== "~");
        if (pin) { best = { sheet: s, pin }; break; }
      }
      t(!!best, `a sheet with a named pin (${best?.pin?.name})`);
      if (!best) return;
      await enterSheet(best.sheet.id);
      const dlg = await findDialog(false);
      t(!!dlg && !!toggle(dlg, "Regular Expression"), "Find has the Regular Expression box (shown in Find, as the base shows it)");
      if (!dlg) return;
      const pattern = `^${reEscape(best.pin.name.slice(0, -1))}.$`;
      setValue(searchBox(dlg), pattern);
      await tick(dlg, "Regular Expression");
      await tick(dlg, "Search pin names and numbers");
      await tick(dlg, "Search the current sheet only");
      const reply = await json("/api/sch/find", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ search: { find: pattern, replace: "", match_case: false, mode: "regex", search_hidden_fields: false, search_pins: true, search_net_names: false, replace_references: false, search_and_replace: false }, sheet: best.sheet.id }) });
      t((reply.matches ?? []).some((m) => m.kind === "pin" && m.text === best.pin.name), `the server's regex finds the pin (${pattern}: ${(reply.matches ?? []).slice(0, 4).map((m) => `${m.kind}:${m.text}`)})`);
      buttonText(dlg, "Find Next").click();
      await sleep(1500);
      t(__eda.state().selection.length > 0, `Find Next selects the match (${JSON.stringify(__eda.state().selection)})`);
      await closeDialogs();
    });

    await scenario("Replace All with a regular expression renames a sheet pin (SCH_SHEET_PIN is replaceable, \\1 in the replacement)", async (t) => {
      const sheet = root0.sheets.find((s) => s.pins.length > 0);
      t(!!sheet, "the root has a sheet with pins");
      if (!sheet) return;
      const pin = sheet.pins[0];
      const dlg = await findDialog(true);
      if (!dlg) return t(false, "Find and Replace opens");
      setValue(searchBox(dlg), `^(${reEscape(pin.name)})$`);
      setValue(replaceBox(dlg), "E2E_\\1");
      await tick(dlg, "Regular Expression");
      await tick(dlg, "Search the current sheet only");
      await sleep(300);
      buttonText(dlg, "Replace All").click();
      await sleep(1800);
      const now = (await sch("")).sheets.find((s) => s.id === sheet.id);
      t(now?.pins.some((p) => p.name === `E2E_${pin.name}`), `the pin is renamed (${now?.pins.map((p) => p.name)})`);
      await closeDialogs();
      await undo();
      t((await sch("")).sheets.find((s) => s.id === sheet.id)?.pins.some((p) => p.name === pin.name), "one undo gives the name back");
    });

    await scenario("Find and Replace reaches a sheet's name (SHEET_NAME field) and replaces it; its file is no replace target", async (t) => {
      const sheet = root0.sheets[0];
      if (!sheet) return t(false, "a sheet");
      const dlg = await findDialog(true);
      if (!dlg) return t(false, "Find and Replace opens");
      setValue(searchBox(dlg), `^${reEscape(sheet.name)}$`);
      setValue(replaceBox(dlg), "E2E & sheet");
      await tick(dlg, "Regular Expression");
      await tick(dlg, "Search the current sheet only");
      await sleep(300);
      buttonText(dlg, "Replace All").click();
      await sleep(1800);
      const now = (await sch("")).sheets.find((s) => s.id === sheet.id);
      t(now?.name === `E2E ${sheet.name} sheet` && now?.file === sheet.file, `the sheet is renamed and keeps its file (${now?.name}, ${now?.file})`);
      await closeDialogs();
      await undo();
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
