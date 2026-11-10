// The Preferences dialog's tree and the pages of task-v2-prefs that are not an editor's colours, hotkeys or toolbars (UI-LEFT-2 CC2; PARITY-common.md, the
// Preferences section): it drives the dialog's own controls and reads the document, `GET /api/state` and `window.__eda`:
//   tree          KiCad's order (Common, Mouse and Touchpad, Hotkeys, Symbol Editor > ..., Schematic Editor > ..., Footprint Editor > ..., PCB Editor > ...,
//                 3D Viewer > ..., Maintenance); the dialog opens on Common and says "Preferences - Common"; the greyed rows carry their reason
//   common        Toolbar icon size Large makes the toolbar buttons 32 px icons; "Use alternating row colors in tables" stripes; the dimming factor typed in percent
//                 is kept; Reset Common to Defaults puts KiCad's defaults back
//   editing       PCB Editor > Editing Options: "<ESC> clears net highlighting" off keeps a highlight through Escape, on clears it; the rows moved off Mouse and
//                 Touchpad (the step of the rotate commands is not there any more)
//   grids         PCB Editor > Grids lists the editor's grids, its reset is "Reset Grids to Defaults"
//   maintenance   Reset "Don't Show Again" Dialogs forgets the kept answers (the scaled-models warning's) and says so in the info bar; Reset All Dialogs too
//   end           every preference as it was, no console errors
// It answers through `window.__prefsPagesCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=6
(async () => {
  const results = [];
  const run = (window.__prefsPagesCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const E = window.__eda;
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dlg = () => tid("preferences-dialog");
  const title = () => dlg()?.querySelector(".dialog-header span")?.textContent.trim();
  const until = async (fn, ms = 4000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(60); } return null; };
  const open = async (page) => {
    if (!dlg()) { await E.run("common.SuiteControl.openPreferences"); await until(dlg); }
    if (page) { tid(`prefs-page-${page}`)?.click(); await sleep(200); }
    return dlg();
  };
  const ok = async () => { tid("preferences-ok")?.click(); await sleep(300); };
  const cancel = async () => { [...(dlg()?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.textContent.trim() === "Cancel")?.click(); await sleep(250); };
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(120);
  };
  const stored = (key) => { try { return JSON.parse(localStorage.getItem(key) ?? "null"); } catch { return null; } };
  const errors0 = E.errors ? E.errors().length : 0;
  /** The stored preferences, keys sorted (a save may list them in another order). */
  const sortedJson = (v) => JSON.stringify(v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, v[k]])) : v);
  const prefs0Obj = stored("eda-studio.preferences.v1");
  const prefs0 = sortedJson(prefs0Obj);
  const common0 = JSON.stringify(stored("eda-studio.common-options")?.common ?? null);

  await scenario("The tree is KiCad's, in KiCad's order; the dialog opens on Common", async (t) => {
    await open();
    t(title() === "Preferences - Common", `title "${title()}"`);
    const labels = [...dlg().querySelectorAll('[role="treeitem"]')].map((e) => e.textContent.trim());
    const want = ["Common", "Mouse and Touchpad", "Hotkeys", "Symbol Editor", "Display Options", "Grids", "Editing Options", "Colors", "Toolbars", "Schematic Editor", "Display Options", "Grids", "Editing Options", "Colors", "Toolbars", "Footprint Editor", "Display Options", "Grids", "Origins & Axes", "Editing Options", "Colors", "Toolbars", "Footprint Defaults", "Graphics Defaults", "User Layer Names", "PCB Editor", "Display Options", "Grids", "Origins & Axes", "Editing Options", "Colors", "Toolbars", "3D Viewer", "General", "Toolbars", "Realtime Renderer", "Maintenance"];
    t(JSON.stringify(labels) === JSON.stringify(want), `tree: ${labels.join(" | ")}`);
    const greyed = [...dlg().querySelectorAll('[data-not-offered="true"]')];
    t(greyed.length >= 10 && greyed.every((g) => (g.getAttribute("title") ?? "").length > 20), `${greyed.length} greyed rows, each with its reason`);
    t(!!greyed.find((g) => /cannot move the pointer/.test(g.getAttribute("title") ?? "") && /Warp mouse/.test(g.textContent)), "Warp mouse to anchor of moved object is greyed: a page cannot move the pointer");
    t(tid("preferences-reset")?.textContent.trim() === "Reset Common to Defaults", `the reset button "${tid("preferences-reset")?.textContent.trim()}"`);
    await open("mouse");
    t(title() === "Preferences - Mouse and Touchpad" && !tid("pref-rotation-step") && !!dlg().querySelector('[aria-label="Middle button drag"]'), "Mouse and Touchpad has the mouse settings and no editor's editing options");
    await open("maintenance");
    t(tid("preferences-reset")?.disabled === true && tid("preferences-reset")?.textContent.trim() === "Reset Maintenance to Defaults" || tid("preferences-reset")?.textContent.trim() === "Reset to Defaults", `Maintenance: "${tid("preferences-reset")?.textContent.trim()}"`);
    await cancel();
  });

  await scenario("Common: the toolbar icon size, the tables' stripes and the dimming factor; Reset Common to Defaults", async (t) => {
    await open("common");
    tid("common-icon-size-32").click();
    if (!tid("common-grid-striping").checked) tid("common-grid-striping").click();
    await setValue(tid("common-hicontrast"), "60");
    await ok();
    const button = document.querySelector(".toolbar .toolbar-button");
    const img = document.querySelector(".toolbar .toolbar-button > img");
    t(button && Math.round(button.getBoundingClientRect().width) === 38, `a toolbar button is 38 px wide (${button && button.getBoundingClientRect().width})`);
    t(img && Math.round(img.getBoundingClientRect().width) === 32, `its icon is 32 px (${img && img.getBoundingClientRect().width})`);
    t(document.documentElement.getAttribute("data-grid-striping") === "1", "the document stripes the tables");
    const c = stored("eda-studio.common-options")?.common;
    t(c?.toolbarIconSize === 32 && c?.gridStriping === true && Math.abs(c?.hicontrastDimmingFactor - 0.6) < 1e-9, `kept: ${JSON.stringify(c)}`);
    await open("common");
    t(tid("common-hicontrast").value === "60", `the page shows 60 % (${tid("common-hicontrast").value})`);
    tid("preferences-reset").click();
    await sleep(150);
    t(tid("common-icon-size-24").checked && !tid("common-grid-striping").checked && tid("common-hicontrast").value === "80" && tid("common-icon-theme-auto").checked, "Reset Common to Defaults: Normal icons, no stripes, 80 %, Automatic icon theme");
    await ok();
    const b2 = document.querySelector(".toolbar .toolbar-button");
    t(b2 && Math.round(b2.getBoundingClientRect().width) === 30 && !document.documentElement.hasAttribute("data-grid-striping"), "back to 24 px icons and plain tables");
  });

  await scenario("PCB Editor > Editing Options: <ESC> clears net highlighting", async (t) => {
    const st = await json("/api/state");
    const part = st.parts.find((p) => p.pads?.some((q) => q.net));
    const pad = part && part.pads.find((q) => q.net);
    if (!t(!!pad, "a placed pad with a net")) return;
    const highlight = async () => { await E.run("common.InteractiveSelection.clear"); await E.run("common.InteractiveSelection.selectItems", [`${part.ref}.${pad.num}`]); await sleep(200); await E.run("pcbnew.EditorControl.highlightNetSelection"); await E.run("common.InteractiveSelection.clear"); await sleep(250); };
    const escape = async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(250); };
    await open("pcb_edit");
    t(title() === "Preferences - Editing Options" && tid("pref-pcb-esc-clears-highlight")?.checked === true, "the page, with the box on (KiCad's default)");
    tid("pref-pcb-esc-clears-highlight").click();
    await ok();
    await highlight();
    const lit = E.state().highlight;
    t(lit.length === 1, `a net is highlighted (${JSON.stringify(lit)})`);
    await escape();
    t(JSON.stringify(E.state().highlight) === JSON.stringify(lit), `off: Escape keeps it (${JSON.stringify(E.state().highlight)})`);
    await open("pcb_edit");
    tid("pref-pcb-esc-clears-highlight").click();
    await ok();
    await escape();
    t(E.state().highlight.length === 0, `on: Escape clears it (${JSON.stringify(E.state().highlight)})`);
  });

  await scenario("PCB Editor > Grids: the editor's list, Reset Grids to Defaults", async (t) => {
    await open("pcb_grids");
    const list = dlg().querySelector('select[aria-label="Grids"]');
    t(title() === "Preferences - Grids" && list && list.options.length > 3, `the list (${list?.options.length} grids)`);
    t(tid("preferences-reset")?.textContent.trim() === "Reset Grids to Defaults", `"${tid("preferences-reset")?.textContent.trim()}"`);
    await cancel();
  });

  await scenario('Maintenance: Reset "Don\'t Show Again" Dialogs and Reset All Dialogs to Defaults', async (t) => {
    localStorage.setItem("eda.doNotShowAgain.scaled_3d_models_warning", "1");
    localStorage.setItem("eda-studio.footprint-chooser-views", JSON.stringify({ probe: 1 }));
    await open("maintenance");
    tid("maintenance-clear-dont-show").click();
    await sleep(200);
    t(localStorage.getItem("eda.doNotShowAgain.scaled_3d_models_warning") === null, "the scaled-models warning is shown again");
    t(tid("preferences-infobar")?.textContent.trim() === '"Don\'t show again" dialogs reset.', `info bar: ${tid("preferences-infobar")?.textContent}`);
    t(localStorage.getItem("eda-studio.footprint-chooser-views") !== null, "the dialogs' own state is kept");
    tid("maintenance-clear-dialog-state").click();
    await sleep(200);
    t(localStorage.getItem("eda-studio.footprint-chooser-views") === null && tid("preferences-infobar")?.textContent.trim() === "All dialogs reset to defaults.", "Reset All Dialogs to Defaults forgets them, and says so");
    t(!!tid("maintenance-reset-all") && [...dlg().querySelectorAll("button")].some((b) => b.disabled && /Open Recent/.test(b.textContent)), "Reset All Program Settings is there; Clear \"Open Recent\" History is greyed");
    await cancel();
  });

  await scenario("end: the preferences are as they were, no console errors", async (t) => {
    const end = stored("eda-studio.preferences.v1");
    const changed = prefs0Obj && end ? Object.keys({ ...prefs0Obj, ...end }).filter((k) => JSON.stringify(prefs0Obj[k]) !== JSON.stringify(end[k])) : [];
    t(sortedJson(end) === prefs0 || prefs0 === "null", `the preferences${changed.length ? `: changed ${changed.map((k) => `${k} ${JSON.stringify(prefs0Obj[k])} -> ${JSON.stringify(end[k])}`).join(", ")}` : ""}`);
    t(JSON.stringify(stored("eda-studio.common-options")?.common ?? null) === common0 || common0 === "null", "the common settings");
    const errs = E.errors ? E.errors().slice(errors0) : [];
    t(errs.length === 0, `errors: ${JSON.stringify(errs)}`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
