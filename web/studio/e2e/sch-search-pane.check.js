// UI-LEFT-2 SS10 (PARITY-sch §80): the schematic's Search pane (`SCH_SEARCH_PANE`, eeschema/widgets/search_handlers.cpp), Find's groups and ERC markers
// (`SCH_GROUP::Matches`, `SCH_MARKER::Matches`), a power symbol's Reference replaced (`SCH_FIELD::Replace`), and the Find dialog's "Show search panel" link.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// the schematic tab open, on a COPY of a board: every scenario undoes what it did. The ERC scenario runs kicad-cli.
//
// It answers through `window.__schSearchPaneCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schSearchPaneCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schSearchPaneCheckSched) {
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
    window.__schSearchPaneCheckSched = true;
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
  /** What the scenarios touch: every sheet's power symbols and groups. */
  const snapshot = async () => {
    const out = [];
    for (const p of ["", ...SHEETS]) { const s = await sch(p); out.push([p, s.power_symbols.map((x) => x.id), (s.groups ?? []).map((g) => [g.id, g.name])]); }
    return JSON.stringify(out);
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

  const SHEETS = (await json("/api/sch/hierarchy")).sheets.map((s) => s.path.join("/")).filter((p) => p !== "");
  const dock = () => document.querySelector('[data-testid="sch-search-dock"]');
  const query = () => document.querySelector('[data-testid="sch-search-pane-query"]');
  const rowsOf = () => [...document.querySelectorAll('[data-testid="sch-search-pane-row"]')].map((r) => ({ el: r, cells: [...r.querySelectorAll("td")].map((td) => td.textContent) }));
  const tabEl = (id) => document.querySelector(`[data-testid="sch-search-pane-tab-${id}"]`);
  const type = async (text) => { setValue(query(), text); await sleep(400); };
  const showPane = async () => { if (!dock()) { await __eda.run("common.Interactive.search"); await until(() => dock()); await sleep(800); } };

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();

    await scenario("Search (Ctrl+G) shows the schematic's own Search pane under the canvas, its box focused, the five tabs", async (t) => {
      if (dock()) await __eda.run("common.Interactive.search");
      t(!dock(), "hidden to begin with (or hidden by the toggle)");
      await __eda.run("common.Interactive.search");
      const d = await until(() => dock());
      t(!!d, "the action shows it");
      await sleep(600);
      t(document.activeElement === query(), "its search box has the focus (FocusSearch)");
      t(!!d && !!d.closest(".bottom-dock-row"), "docked under the canvas (.Bottom())");
      const tabs = [...document.querySelectorAll('[data-testid^="sch-search-pane-tab-"]')].map((x) => x.textContent.trim());
      t(same(tabs, ["Symbols", "Power", "Text", "Labels", "Groups"]), `the tabs (${tabs})`);
      const cols = [...document.querySelectorAll('[data-testid^="sch-search-pane-col-"]')].map((x) => x.textContent.replace(/[▲▼]/g, "").trim());
      t(cols.length === 12 && cols[0] === "Reference" && cols[11] === "Library Description", `the Symbols columns (${cols})`);
      t(!document.querySelector('[data-testid="search-pane"]'), "the board editor's pane is not shown on the schematic");
      await __eda.run("common.Interactive.search");
      t(!dock(), "the action hides it again");
    });

    await scenario("The Symbols tab lists the symbols of every sheet; a row goes to its sheet, selects the symbol and pans to it", async (t) => {
      await showPane();
      tabEl("symbols").click();
      await type("");
      const all = await until(() => rowsOf().length >= 30 ? rowsOf() : null, 15000);
      t(!!all, `every placed symbol of the hierarchy is listed (${rowsOf().length})`);
      await type("r3");
      const rows = rowsOf();
      const r3 = rows.find((r) => r.cells[0] === "R3");
      t(!!r3 && r3.cells[3] === "4", `R3 is found, on page 4 (${JSON.stringify(rows.map((r) => r.cells.slice(0, 4)))})`);
      if (!r3) return;
      const v0 = JSON.stringify(__eda.state().view);
      r3.el.click();
      await until(() => __eda.state().sheetPath.length > 0, 6000);
      await sleep(800);
      t(same(__eda.state().sheetPath, ["sheet_84ea8b99f6a5"]), `the editor goes to its sheet (${JSON.stringify(__eda.state().sheetPath)})`);
      t(__eda.state().selection.some((s) => s.id === "R3"), `R3 is selected (${JSON.stringify(__eda.state().selection)})`);
      t(JSON.stringify(__eda.state().view) !== v0, "the view moved to it (Pan to Selection)");
    });

    await scenario("Power and Labels: the power symbols by reference and value, the labels by name; rows of two sheets select nothing", async (t) => {
      await showPane();
      tabEl("power").click();
      await type("#PWR27");
      const p = rowsOf();
      t(p.length === 1 && p[0].cells[0] === "#PWR27" && p[0].cells[1] === "GND", `#PWR27 (${JSON.stringify(p.map((r) => r.cells))})`);
      await type("vdd");
      t(rowsOf().length > 0 && rowsOf().every((r) => r.cells[1] === "VDD"), `by value (${rowsOf().length} VDD rows)`);
      tabEl("labels").click();
      await type("/^PA0$");
      const l = rowsOf();
      t(l.length === 2 && new Set(l.map((r) => r.cells[2])).size === 2, `PA0 on two pages, by a regular expression (${JSON.stringify(l.map((r) => r.cells))})`);
      if (l.length === 2) {
        l[0].el.click();
        await sleep(1200);
        l[1].el.dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
        await sleep(1200);
        t(__eda.state().selection.length === 0, `two sheets: nothing selected (${JSON.stringify(__eda.state().selection)})`);
      }
    });

    await scenario("A group is found by its name in the Groups tab and by Find; neither replaces it", async (t) => {
      await leave();
      const mcu = "sheet_80460aabc8e9";
      const s = await sch(mcu);
      const ids = s.labels.slice(0, 2).map((x) => x.id);
      const r1 = await post({ op: "sch_edit", verb: "group", ids }, mcu);
      t(r1.ok !== false, `grouped two labels (${JSON.stringify(r1).slice(0, 120)})`);
      const g = ((await sch(mcu)).groups ?? [])[0];
      if (!t(!!g, "the group is there")) return;
      const r2 = await post({ op: "sch_edit", verb: "edit_group", id: g.id, name: "E2E_INPUTS", member_ids: g.member_ids }, mcu);
      t(r2.ok !== false, "named E2E_INPUTS");
      await showPane();
      tabEl("groups").click();
      await type("e2e_inp");
      const rows = await until(() => rowsOf().length > 0 ? rowsOf() : null, 8000);
      t(!!rows && rows.length === 1 && rows[0].cells[0] === "E2E_INPUTS" && rows[0].cells[1] === "3", `the Groups tab (${JSON.stringify(rows?.map((r) => r.cells))})`);
      const search = { find: "E2E_INPUTS", replace: "X", match_case: false, mode: "plain", search_hidden_fields: false, search_pins: false, search_net_names: false, replace_references: false, search_and_replace: false };
      const found = await json("/api/sch/find", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ search, all_sheets: true }) });
      t((found.matches ?? []).some((m) => m.kind === "group" && m.id === g.id), `Find reaches the group (${(found.matches ?? []).map((m) => m.key)})`);
      const replacing = await json("/api/sch/find", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ search: { ...search, search_and_replace: true }, all_sheets: true }) });
      t((replacing.matches ?? []).length === 0, "not while replacing (IsReplaceable)");
    });

    await scenario("Find and Replace sets a power symbol's Reference with \"Replace matches in reference designators\", one undo step", async (t) => {
      await leave();
      const dlg = await findDialog(true);
      if (!dlg) return t(false, "Find and Replace opens");
      setValue(searchBox(dlg), "#PWR27");
      setValue(replaceBox(dlg), "#PWR90");
      await tick(dlg, "Include hidden fields");
      await tick(dlg, "Search the current sheet only", false);
      await sleep(300);
      buttonText(dlg, "Replace All").click();
      await sleep(1800);
      let led = await sch("sheet_84ea8b99f6a5");
      t(!led.power_symbols.some((p) => p.id === "#PWR90"), "not without the reference designators box");
      await tick(dlg, "Replace matches in reference designators");
      await sleep(300);
      buttonText(dlg, "Replace All").click();
      await sleep(1800);
      led = await sch("sheet_84ea8b99f6a5");
      t(led.power_symbols.some((p) => p.id === "#PWR90") && !led.power_symbols.some((p) => p.id === "#PWR27"), `#PWR27 is #PWR90 (${led.power_symbols.map((p) => p.id).slice(0, 4)})`);
      await closeDialogs();
      await undo();
      led = await sch("sheet_84ea8b99f6a5");
      t(led.power_symbols.some((p) => p.id === "#PWR27"), "one undo gives it back");
    });

    await scenario("Find's \"Show search panel (Ctrl+G)\" link shows the pane, closes the dialog and focuses the box", async (t) => {
      if (dock()) await __eda.run("common.Interactive.search");
      const dlg = await findDialog(false);
      if (!dlg) return t(false, "Find opens");
      const link = dlg.querySelector('[data-testid="sch-find-search-panel"]');
      t(!!link && link.textContent.trim() === "Show search panel (Ctrl+G)", `the link (${link?.textContent})`);
      link?.click();
      await sleep(800);
      t(!dialogTitled(/^Find$/), "the dialog closed");
      t(!!dock() && document.activeElement === query(), "the pane is up with the focus in its box");
    });

    await scenario("Find walks the sheet's ERC markers (SCH_MARKER::Matches on the message) and brightens and visits one", async (t) => {
      await leave();
      await enterSheet("sheet_80460aabc8e9");
      const ercReplies = [];
      const of = window.fetch;
      window.fetch = async (...a) => { const r = await of(...a); if (String(a[0]).startsWith("/api/erc")) r.clone().json().then((j) => ercReplies.push(j)).catch(() => {}); return r; };
      await __eda.run("eeschema.InspectionTool.runERC");
      const erc = await until(() => dialogTitled(/Electrical Rules Checker/));
      if (!t(!!erc, "the ERC dialog opens")) { window.fetch = of; return; }
      buttonText(erc, "Run ERC")?.click();
      const rep = await until(() => ercReplies.find((x) => Array.isArray(x?.violations)), 120000);
      window.fetch = of;
      if (!t(!!rep && rep.violations.length > 0, `ERC ran (${rep?.violations?.length} findings)`)) return;
      await closeDialogs();
      const here = await sch("sheet_80460aabc8e9");
      const onSheet = rep.violations.map((v, i) => ({ v, i })).filter(({ v }) => v.location && (here.symbols.some((s) => v.location === s.id || v.location.startsWith(`${s.id}.`)) || here.power_symbols.some((p) => p.id === v.location) || here.labels.some((l) => l.id === v.location)));
      if (!t(onSheet.length > 0, `a finding on the MCU sheet (${onSheet.length})`)) return;
      const { v, i } = onSheet[0];
      const descs = (v.items ?? []).map((x) => x.description).filter((d) => d);
      const tail = descs.length ? `: ${descs.join("; ")}` : "";
      const msg = tail && (v.hint ?? "").endsWith(tail) ? v.hint.slice(0, -tail.length) : v.hint;
      const dlg = await findDialog(false);
      if (!dlg) return t(false, "Find opens");
      setValue(searchBox(dlg), msg);
      await tick(dlg, "Search the current sheet only");
      await sleep(1500);
      t(__eda.state().brightened.includes(`erc:${i}`), `the marker is brightened with the matches (${msg}; ${__eda.state().brightened.slice(0, 5)})`);
      buttonText(dlg, "Find Next").click();
      await sleep(1500);
      t(/match/.test(dlg.innerText), "Find Next found matches");
      await closeDialogs();
    });

    if (dock()) await __eda.run("common.Interactive.search");
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
