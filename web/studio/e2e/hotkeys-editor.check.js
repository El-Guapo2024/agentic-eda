// Preferences > Hotkeys and List Hotkeys (UI-LEFT-2 CC2; PARITY-common.md, "Hotkeys"): KiCad's hotkey list, Set Hotkey, the conflict question, the resets,
// Import Hotkeys... and the person's user.hotkeys (`localStorage` "eda-studio.hotkeys.v1"). It drives the dialogs' own controls and real key events:
//   page        the tree's Hotkeys page: KiCad's columns, the sections Common / Schematic Editor / PCB Editor / Gestures, Close and Quit atop Common, the filter
//   reassign    Zoom to Fit (Home) given Shift+K in Set Hotkey: the row shows "Shift+K *", OK writes user.hotkeys, Shift+K zooms to fit, Home no longer does, and
//               the View menu shows the new key
//   conflict    F1 for Route Single Track asks KiCad's question about Zoom In at Cursor; No keeps both; Yes moves the key; Ctrl+Tab is refused as reserved; Undo
//               All Changes puts every row back
//   reset       Restore Defaults puts Home back on Zoom to Fit; Reset Hotkeys to Defaults resets every row; OK and Home zooms to fit again
//   import      a user.hotkeys file given to Import Hotkeys... sets its keys (not saved until OK); OK and the imported key works
//   list        List Hotkeys: "Hotkey List", read-only ("Command", no Set Hotkey on a double click), the person's key shown; Cancel
//   end         the defaults back and the person's file removed, no console errors
// It answers through `window.__hotkeysEditorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const run = (window.__hotkeysEditorCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const E = window.__eda;
  const STORE = "eda-studio.hotkeys.v1";
  const prefs = () => document.querySelector('[data-testid="preferences-dialog"]');
  const panel = () => document.querySelector('[data-testid="hotkeys-panel"]');
  const row = (name) => panel()?.querySelector(`tr[data-hotkey-action="${name}"]`);
  const cellOf = (name, col) => row(name)?.querySelector(`td[data-col="${col}"]`)?.textContent ?? null;
  const dialogLabelled = (label) => [...document.querySelectorAll('[role="dialog"]')].find((d) => d.getAttribute("aria-label") === label);
  const buttonIn = (root, text) => root && [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const click = async (el, wait = 200) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const dbl = async (el, wait = 250) => { if (!el) throw new Error("no row"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const rightClick = async (el, wait = 250) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 4 })); await sleep(wait); };
  const menuItem = (label) => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].find((m) => m.textContent.trim() === label);
  const setValue = async (el, v) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(150); };
  /** A key pressed and released where the focus is (the prompt's panel) or on the page. */
  const press = async (target, key, code, mods = {}) => {
    const init = { key, code, bubbles: true, cancelable: true, ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt, metaKey: !!mods.meta };
    target.dispatchEvent(new KeyboardEvent("keydown", init));
    await sleep(30);
    target.dispatchEvent(new KeyboardEvent("keyup", init));
    await sleep(200);
  };
  const openHotkeys = async () => {
    if (!prefs()) { await E.run("common.SuiteControl.openPreferences"); await sleep(400); }
    await click(document.querySelector('[data-testid="prefs-page-hotkeys"]'), 400);
    if (!panel()) throw new Error("no Hotkeys page");
  };
  const prefsOk = async () => { await click(document.querySelector('[data-testid="preferences-ok"]'), 400); };
  /** Set Hotkey on a row (its hotkey, or with `alt` its alternate), pressing `key`. */
  const assign = async (name, key, code, mods, alt = false) => {
    if (alt) {
      await rightClick(row(name));
      await click(menuItem("Edit Alternate..."));
    } else await dbl(row(name));
    const prompt = dialogLabelled("Set Hotkey");
    if (!prompt) throw new Error(`Set Hotkey did not open for ${name}`);
    await press(document.activeElement ?? prompt, key, code, mods);
  };
  const view = () => E.state().view;
  const fitView = async () => { await E.run("common.Control.zoomFitScreen"); await sleep(400); return { ...view() }; };
  const zoomedIn = async () => { await E.run("common.Control.zoomIn"); await sleep(200); await E.run("common.Control.zoomIn"); await sleep(400); return { ...view() }; };
  const same = (a, b) => Math.abs(a.scale - b.scale) < 1e-9 && Math.abs(a.x - b.x) < 1e-3 && Math.abs(a.y - b.y) < 1e-3;
  const storedLine = (name) => (localStorage.getItem(STORE) ?? "").split("\n").find((l) => l.split("\t")[0] === name) ?? null;

  localStorage.removeItem(STORE);
  [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click();
  await sleep(600);

  await scenario("page: KiCad's columns, the sections, Close and Quit atop Common, the filter", async (t) => {
    await openHotkeys();
    t(prefs().getAttribute("aria-label") === "Preferences - Hotkeys", `the title (${prefs().getAttribute("aria-label")})`);
    const heads = [...panel().querySelectorAll("th")].map((h) => h.textContent.trim());
    t(JSON.stringify(heads) === JSON.stringify(["Command (double-click to edit)", "Hotkey", "Alternate", "Description"]), `columns ${heads.join(" | ")}`);
    const sections = [...panel().querySelectorAll("tr[data-hotkey-section]")].map((r) => r.getAttribute("data-hotkey-section"));
    t(JSON.stringify(sections) === JSON.stringify(["Common", "Schematic Editor", "PCB Editor", "Gestures"]), `sections ${sections.join(", ")}`);
    const firstRows = [...panel().querySelectorAll("tr[data-hotkey-label]")].slice(0, 3).map((r) => r.getAttribute("data-hotkey-label"));
    t(firstRows[0] === "Close" && firstRows[1] === "Quit", `Common starts with ${firstRows.join(", ")}`);
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Home", `Zoom to Fit is Home (${cellOf("common.Control.zoomFitScreen", "hotkey")})`);
    t(cellOf("pcbnew.PointEditor.addCorner", "hotkey") === "Ins", `Create Corner is KiCad's Ins (${cellOf("pcbnew.PointEditor.addCorner", "hotkey")})`);
    t(cellOf("common.Interactive.group", "hotkey") === "", "Group Items has no key, as in KiCad (Ctrl+G is Search's)");
    t(buttonIn(prefs(), "Reset Hotkeys to Defaults")?.getAttribute("title") === "Reset all hotkeys to the built-in KiCad defaults", "the dialog's reset button and its tooltip");
    const filter = panel().querySelector('input[aria-label="Filter hotkeys"]');
    t(filter.getAttribute("placeholder") === "Type filter text", "the filter's text");
    await setValue(filter, "zoom to fit");
    const shown = [...panel().querySelectorAll("tr[data-hotkey-action]")].map((r) => r.getAttribute("data-hotkey-action"));
    t(shown.includes("common.Control.zoomFitScreen") && shown.every((n) => /zoom to fit/i.test(row(n).querySelector('td[data-col="command"]').textContent)), `filtered to ${shown.join(", ")}`);
    await setValue(filter, "ctrl+shift+z");
    const byKey = [...panel().querySelectorAll("tr[data-hotkey-action]")].map((r) => r.getAttribute("data-hotkey-action"));
    t(byKey.includes("pcbnew.InteractiveDrawing.zone"), `a filter matches the hotkey's name: ${byKey.join(", ")}`);
    await setValue(filter, "");
  });

  await scenario("reassign: Zoom to Fit on Shift+K -- the row, user.hotkeys, the key works, Home does not, the menu shows it", async (t) => {
    await openHotkeys();
    await assign("common.Control.zoomFitScreen", "K", "KeyK", { shift: true });
    t(!dialogLabelled("Set Hotkey"), "the prompt closes when the key is released");
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Shift+K", `the row's hotkey (${cellOf("common.Control.zoomFitScreen", "hotkey")})`);
    t(/ \*$/.test(row("common.Control.zoomFitScreen").querySelector('td[data-col="command"]').textContent), "marked changed");
    await prefsOk();
    t(storedLine("common.Control.zoomFitScreen") === "common.Control.zoomFitScreen\tShift+K\t", `user.hotkeys: ${JSON.stringify(storedLine("common.Control.zoomFitScreen"))}`);
    t((localStorage.getItem(STORE) ?? "").split("\n").filter(Boolean).length > 700, "WriteHotKeyConfig wrote every action");
    const fit = await fitView();
    await zoomedIn();
    await press(document.body, "Home", "Home");
    t(!same(view(), fit), "Home no longer zooms to fit");
    await press(document.body, "K", "KeyK", { shift: true });
    t(same(view(), fit), `Shift+K zooms to fit (${JSON.stringify(view())} vs ${JSON.stringify(fit)})`);
    const viewMenu = [...document.querySelectorAll(".menubar-item")].find((m) => m.firstChild?.textContent?.trim() === "View");
    await click(viewMenu, 250);
    const item = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].find((m) => m.querySelector("span")?.textContent.trim() === "Zoom to Fit");
    t(item?.querySelector(".menu-node-hotkey")?.textContent === "Shift+K", `the View menu shows ${item?.querySelector(".menu-node-hotkey")?.textContent}`);
    await click(viewMenu, 200);
  });

  await scenario("conflict: KiCad's question, No and Yes; a reserved key refused; Undo All Changes", async (t) => {
    await openHotkeys();
    await assign("pcbnew.InteractiveRouter.SingleTrack", "F1", "F1");
    let q = dialogLabelled("Confirm change");
    t(q?.querySelector('[data-testid="hotkeys-question"]')?.textContent === "'F1' is already assigned to 'Zoom In at Cursor' in section 'Common'. Are you sure you want to change its assignment?", `the question: ${q?.textContent}`);
    t(document.activeElement === buttonIn(q, "No"), "No is the default");
    await click(buttonIn(q, "No"));
    t(cellOf("pcbnew.InteractiveRouter.SingleTrack", "hotkey") === "X" && cellOf("common.Control.zoomIn", "hotkey") === "F1", "No: both keep their keys");
    await assign("pcbnew.InteractiveRouter.SingleTrack", "F1", "F1");
    q = dialogLabelled("Confirm change");
    await click(buttonIn(q, "Yes"));
    t(cellOf("pcbnew.InteractiveRouter.SingleTrack", "hotkey") === "F1" && cellOf("common.Control.zoomIn", "hotkey") === "", `Yes: the key moves (${cellOf("pcbnew.InteractiveRouter.SingleTrack", "hotkey")}, Zoom In "${cellOf("common.Control.zoomIn", "hotkey")}")`);
    // an alternate through the context menu
    await assign("pcbnew.InteractiveRouter.SingleTrack", "F9", "F9", {}, true);
    t(cellOf("pcbnew.InteractiveRouter.SingleTrack", "alternate") === "F9", `Edit Alternate...: ${cellOf("pcbnew.InteractiveRouter.SingleTrack", "alternate")}`);
    await assign("pcbnew.InteractiveRouter.SingleTrack", "Tab", "Tab", { ctrl: true });
    const err = dialogLabelled("Error");
    t(err?.querySelector('[data-testid="hotkeys-question"]')?.textContent === "'Ctrl+Tab' is a reserved hotkey in KiCad and cannot be assigned.", `reserved: ${err?.textContent}`);
    await click(buttonIn(err, "OK"));
    // Esc in the prompt changes nothing
    await assign("pcbnew.InteractiveRouter.SingleTrack", "Escape", "Escape");
    t(!dialogLabelled("Set Hotkey") && cellOf("pcbnew.InteractiveRouter.SingleTrack", "hotkey") === "F1", "Esc closes the prompt and changes nothing");
    await click(buttonIn(panel(), "Undo All Changes"));
    t(cellOf("pcbnew.InteractiveRouter.SingleTrack", "hotkey") === "X" && cellOf("pcbnew.InteractiveRouter.SingleTrack", "alternate") === "" && cellOf("common.Control.zoomIn", "hotkey") === "F1", "Undo All Changes: every row as saved");
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Shift+K", "the saved Shift+K stays (it is the saved key)");
  });

  await scenario("reset: Restore Defaults on the row, Reset Hotkeys to Defaults, OK and Home zooms to fit again", async (t) => {
    await openHotkeys();
    await rightClick(row("common.Control.zoomFitScreen"));
    const labels = [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].map((m) => m.textContent.trim());
    t(JSON.stringify(labels) === JSON.stringify(["Edit...", "Edit Alternate...", "Undo Changes", "Clear Assigned Hotkey", "Clear Assigned Alternate", "Restore Defaults"]), `the menu: ${labels.join(", ")}`);
    await click(menuItem("Restore Defaults"));
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Home" && / \*$/.test(cellOf("common.Control.zoomFitScreen", "command") ?? ""), "Restore Defaults: Home, marked changed");
    await rightClick(row("common.Control.zoomFitScreen"));
    await click(menuItem("Undo Changes"));
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Shift+K", "Undo Changes: the saved key");
    await rightClick(row("common.Control.zoomFitScreen"));
    await click(menuItem("Clear Assigned Hotkey"));
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "", "Clear Assigned Hotkey");
    await click(buttonIn(prefs(), "Reset Hotkeys to Defaults"));
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Home", "Reset Hotkeys to Defaults: KiCad's key");
    await prefsOk();
    t(storedLine("common.Control.zoomFitScreen") === "common.Control.zoomFitScreen\tHome\t", `saved: ${JSON.stringify(storedLine("common.Control.zoomFitScreen"))}`);
    const fit = await fitView();
    await zoomedIn();
    await press(document.body, "Home", "Home");
    t(same(view(), fit), "Home zooms to fit again");
  });

  await scenario("import: a user.hotkeys given to Import Hotkeys... sets its keys; OK keeps them; the imported key works", async (t) => {
    await openHotkeys();
    const input = panel().querySelector('[data-testid="hotkeys-import-file"]');
    t(input?.getAttribute("accept") === ".hotkeys", "a .hotkeys file is asked for");
    const dt = new DataTransfer();
    // as KiCad writes it: name, hotkey, alternate, tab-separated; KiCad's key names (Ins, Back)
    dt.items.add(new File(["common.Control.zoomFitScreen\tShift+J\tIns\npcbnew.InteractiveRouter.SingleTrack\tX\t\n"], "mine.hotkeys", { type: "text/plain" }));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(400);
    t(cellOf("common.Control.zoomFitScreen", "hotkey") === "Shift+J" && cellOf("common.Control.zoomFitScreen", "alternate") === "Ins", `imported: ${cellOf("common.Control.zoomFitScreen", "hotkey")} / ${cellOf("common.Control.zoomFitScreen", "alternate")}`);
    t(storedLine("common.Control.zoomFitScreen") === "common.Control.zoomFitScreen\tHome\t", "not saved before OK");
    await prefsOk();
    t(storedLine("common.Control.zoomFitScreen") === "common.Control.zoomFitScreen\tShift+J\tIns", `saved: ${JSON.stringify(storedLine("common.Control.zoomFitScreen"))}`);
    const fit = await fitView();
    await zoomedIn();
    await press(document.body, "J", "KeyJ", { shift: true });
    t(same(view(), fit), "Shift+J zooms to fit");
  });

  await scenario("list: List Hotkeys is KiCad's read-only Hotkey List with the person's keys", async (t) => {
    await E.run("common.SuiteControl.listHotKeys");
    await sleep(400);
    const dlg = dialogLabelled("Hotkey List");
    if (!t(!!dlg, "Hotkey List opens")) return;
    const p = dlg.querySelector('[data-testid="hotkeys-panel"]');
    t(p.getAttribute("data-readonly") === "true" && p.querySelector("th").textContent.trim() === "Command", "read-only: the column is \"Command\"");
    const r = p.querySelector('tr[data-hotkey-action="common.Control.zoomFitScreen"]');
    t(r?.querySelector('td[data-col="hotkey"]')?.textContent === "Shift+J", `the person's key (${r?.querySelector('td[data-col="hotkey"]')?.textContent})`);
    await dbl(r);
    t(!dialogLabelled("Set Hotkey"), "a double click edits nothing");
    t(!!buttonIn(dlg, "Import Hotkeys...") && !!buttonIn(dlg, "OK") && !!buttonIn(dlg, "Cancel"), "the panel's buttons, OK and Cancel");
    await click(buttonIn(dlg, "Cancel"));
    t(!dialogLabelled("Hotkey List"), "Cancel closes it");
  });

  await scenario("end: the defaults back, the person's file removed, no console errors", async (t) => {
    await openHotkeys();
    await click(buttonIn(prefs(), "Reset Hotkeys to Defaults"));
    await prefsOk();
    t(storedLine("common.Control.zoomFitScreen") === "common.Control.zoomFitScreen\tHome\t", "every hotkey KiCad's");
    localStorage.removeItem(STORE);
    t((E.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(E.errors?.() ?? []).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
