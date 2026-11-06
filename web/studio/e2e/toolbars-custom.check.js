// Customised toolbars (FF7's Toolbars; PARITY-common.md "Preferences > <editor> > Toolbars"): KiCad's PANEL_TOOLBAR_CUSTOMIZATION on the board editor's and the
// Footprint Editor's Preferences pages, and the toolbars they draw. It drives the page's own controls and reads the toolbars' buttons (`data-action`, `data-group`)
// and the store (`localStorage` "eda-studio.toolbars.v1"):
//   off             the page starts with Customize toolbars off and its other controls dimmed; the toolbar choice is KiCad's (Left, Right, Top main, Top
//                   auxiliary for the board editor)
//   pcb             Customize toolbars on, Top main: Print removed, Measure Tool added after Zoom to Objects, a spacer after it, the zoom box moved here from the
//                   auxiliary toolbar; OK: the board's top toolbar draws them, the auxiliary toolbar no longer has the zoom box, the store holds pcbnew-toolbars
//   use             the added Measure Tool button runs the measure tool
//   footprint       the Footprint Editor's page: Print removed, a group inserted, renamed "My tools" and given Zoom to Fit and Zoom In; OK: the editor's top toolbar
//                   has the group button (its first tool's icon, a right click lists both) and no Print
//   cancel          an edit cancelled leaves the toolbar as it was
//   reset           Reset Toolbars to Defaults on each page brings KiCad's toolbars back (Customize toolbars stays on)
//   end             Customize toolbars off on both: the toolbars are the defaults again, button for button; the store is cleared; no console errors
// It answers through `window.__toolbarsCustomCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const run = (window.__toolbarsCustomCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const errors = [];
  const onErr = (e) => errors.push(String(e?.message ?? e?.reason ?? e));
  window.addEventListener("error", onErr);
  window.addEventListener("unhandledrejection", onErr);
  const E = window.__eda;
  const tid = (id, root = document) => root.querySelector(`[data-testid="${id}"]`);
  const prefs = () => tid("preferences-dialog");
  const footerButton = (text) => prefs() && [...prefs().querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === text);
  const click = async (el, wait = 200) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const dbl = async (el, wait = 200) => { if (!el) throw new Error("nothing to double-click"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(150);
  };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(900); };
  const toolbar = (editor, id) => document.querySelector(`.toolbar[data-editor="${editor}"][data-toolbar="${id}"]`);
  const actionsOf = (editor, id) => [...(toolbar(editor, id)?.children ?? [])].map((el) => el.getAttribute("data-group") ?? el.getAttribute("data-action") ?? (el.classList.contains("toolbar-separator") ? "|" : el.classList.contains("toolbar-spacer") ? `spacer:${el.getAttribute("data-spacer")}` : el.classList.contains("toolbar-control") ? `control:${el.getAttribute("title")}` : el.tagName));
  const stored = () => JSON.parse(localStorage.getItem("eda-studio.toolbars.v1") ?? "null");
  const page = () => document.querySelector('[data-testid^="toolbars-page-"]');
  const treeRow = (name) => tid("toolbars-tree")?.querySelector(`[data-name="${name}"]`);
  const treeRows = () => [...(tid("toolbars-tree")?.querySelectorAll("[role=treeitem]") ?? [])];
  const openPage = async (id) => {
    if (!prefs()) { await E.run("common.SuiteControl.openPreferences"); await sleep(500); }
    if (!prefs()) throw new Error("Preferences did not open");
    await click(tid(`prefs-page-${id}`), 400);
    if (!page()) throw new Error(`no Toolbars page at prefs-page-${id}`);
  };
  const ok = async () => { await click(footerButton("OK"), 600); };
  const cancel = async () => { await click(footerButton("Cancel"), 600); };
  /** Picks the list entry of `name` after filtering by `filter`, then presses the add button. */
  const addEntry = async (filter, name) => {
    await setValue(tid("toolbars-filter"), filter);
    await click(tid("toolbars-actions").querySelector(`[data-entry="${name}"]`), 120);
    await click(tid("toolbars-add"), 200);
  };

  const pcbStart = { main: actionsOf("pcb", "main"), auxiliary: actionsOf("pcb", "auxiliary") };
  const storeStart = localStorage.getItem("eda-studio.toolbars.v1");
  let fpStart = null;

  await scenario("off: Customize toolbars starts off with the page dimmed; the board editor's toolbars are Left, Right, Top main and Top auxiliary", async (t) => {
    await clickTab("PCB");
    await openPage("pcb_toolbars");
    t(tid("toolbars-customize")?.checked === false, "Customize toolbars is off");
    t(tid("toolbars-choice")?.disabled && tid("toolbars-add")?.disabled && tid("toolbars-filter")?.disabled && tid("toolbars-insert")?.disabled, "the choice, Add, the filter and Insert Separator are dimmed");
    t(JSON.stringify([...tid("toolbars-choice").options].map((o) => o.textContent)) === JSON.stringify(["Left", "Right", "Top main", "Top auxiliary"]), `the toolbar choice (${[...tid("toolbars-choice").options].map((o) => o.textContent)})`);
    t(tid("toolbars-choice").value === "LEFT", "the first toolbar (Left) is shown");
    t(pcbStart.main.includes("common.Control.print") && pcbStart.auxiliary.includes("control:Zoom"), `the board's toolbars start as KiCad's (print ${pcbStart.main.includes("common.Control.print")}, zoom box ${pcbStart.auxiliary.includes("control:Zoom")})`);
  });

  await scenario("pcb: Print removed, Measure Tool and a spacer added, the zoom box moved to Top main; OK draws them and stores pcbnew-toolbars", async (t) => {
    await openPage("pcb_toolbars");
    await click(tid("toolbars-customize"));
    t(!tid("toolbars-choice").disabled && !tid("toolbars-add").disabled, "Customize toolbars on: the controls are live");
    await setValue(tid("toolbars-choice"), "TOP_MAIN");
    t(!!treeRow("common.Control.print") && !!treeRow("common.Control.zoomFitObjects"), `the tree has Top main's items (${treeRows().length} rows)`);
    await click(treeRow("common.Control.print"));
    await click(tid("toolbars-delete"));
    t(!treeRow("common.Control.print"), "Delete takes Print off the tree");
    await click(treeRow("common.Control.zoomFitObjects"));
    await addEntry("measure", "common.Interactive.measureTool");
    const rows = treeRows().map((r) => r.getAttribute("data-name") ?? r.getAttribute("data-kind"));
    const at = rows.indexOf("common.Interactive.measureTool");
    t(at > 0 && rows[at - 1] === "common.Control.zoomFitObjects", `Measure Tool is added after the selected Zoom to Objects (${rows.slice(Math.max(0, at - 1), at + 1)})`);
    t(treeRow("common.Interactive.measureTool")?.getAttribute("aria-selected") === "true", "and selected");
    await click(tid("toolbars-insert-menu"));
    const spacerItem = [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].find((m) => m.textContent.trim() === "Insert Spacer");
    await click(spacerItem);
    t(treeRows().some((r) => r.textContent.trim() === "Spacer: 5"), "Insert Spacer adds \"Spacer: 5\"");
    await addEntry("Zoom Selection box", "control.ZoomSelector");
    t(!!treeRow("control.ZoomSelector"), "the zoom box (found by its description) is on Top main");
    await setValue(tid("toolbars-choice"), "TOP_AUX");
    t(!treeRow("control.ZoomSelector") && !!treeRow("control.GridSelector"), "and no longer on Top auxiliary (a control is on one toolbar only)");
    await ok();
    const main = actionsOf("pcb", "main");
    t(!main.includes("common.Control.print"), "the board's top toolbar has no Print");
    const m = main.indexOf("common.Interactive.measureTool");
    t(m > 0 && main[m - 1] === "common.Control.zoomFitObjects" && main[m + 1] === "spacer:5", `Measure Tool after Zoom to Objects, then the spacer (${main.slice(m - 1, m + 3)})`);
    t(main.includes("control:Zoom") && !actionsOf("pcb", "auxiliary").includes("control:Zoom"), "the zoom box is on the top toolbar, not the auxiliary one");
    const s = stored();
    t(s?.["pcbnew-toolbars"]?.custom_toolbars === true && s["pcbnew-toolbars"].toolbars.some((x) => x.name === "TOP_MAIN" && x.contents.some((c) => c.type === "SPACER" && c.size === 5)), "the store holds pcbnew-toolbars in KiCad's form, Customize toolbars on");
  });

  await scenario("use: the added Measure Tool button runs the measure tool", async (t) => {
    const btn = toolbar("pcb", "main")?.querySelector('[data-action="common.Interactive.measureTool"]');
    t(!!btn && !btn.disabled, "the button is there and enabled");
    await click(btn, 400);
    t(E.state().tool === "measure", `the measure tool is active (${E.state().tool})`);
    await E.run("common.Interactive.cancel").catch(() => null);
    await sleep(200);
  });

  await scenario("footprint: Print removed, a group \"My tools\" of Zoom to Fit and Zoom In; OK draws the group button", async (t) => {
    await clickTab("Footprint");
    fpStart = actionsOf("footprint", "main");
    t(fpStart.includes("common.Control.print"), "the Footprint Editor's top toolbar starts with Print");
    await openPage("fp_toolbars");
    t(JSON.stringify([...tid("toolbars-choice").options].map((o) => o.value)) === JSON.stringify(["LEFT", "RIGHT", "TOP_MAIN"]), "the Footprint Editor has no auxiliary toolbar");
    await click(tid("toolbars-customize"));
    await setValue(tid("toolbars-choice"), "TOP_MAIN");
    await click(treeRow("common.Control.print"));
    await click(tid("toolbars-delete"));
    await click(tid("toolbars-insert-menu"));
    await click([...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].find((m) => m.textContent.trim() === "Insert Group"));
    const group = treeRows().find((r) => r.getAttribute("data-kind") === "group" && r.textContent.trim() === "Group");
    t(!!group && group.getAttribute("aria-selected") === "true", "Insert Group adds a selected \"Group\"");
    await dbl(group);
    const input = tid("toolbars-tree").querySelector('input[aria-label="Group name"]');
    await setValue(input, "My tools");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await sleep(200);
    const renamed = treeRows().find((r) => r.getAttribute("data-kind") === "group" && r.textContent.trim() === "My tools");
    t(!!renamed, "the group renamed \"My tools\"");
    await click(renamed);
    await addEntry("zoom to fit", "common.Control.zoomFitScreen");
    await click(treeRows().find((r) => r.getAttribute("data-kind") === "group" && r.textContent.trim() === "My tools"));
    await addEntry("zoom in", "common.Control.zoomInCenter");
    const kids = treeRows().filter((r) => r.getAttribute("aria-level") === "2" && ["common.Control.zoomFitScreen", "common.Control.zoomInCenter"].includes(r.getAttribute("data-name")));
    t(kids.length === 2, `both tools are the group's (${kids.length})`);
    await ok();
    const main = actionsOf("footprint", "main");
    t(!main.includes("common.Control.print") && main.includes("My tools"), `the top toolbar: no Print, the group ${main.includes("My tools")}`);
    const gb = toolbar("footprint", "main")?.querySelector('[data-group="My tools"]');
    t(gb?.getAttribute("data-action") === "common.Control.zoomFitScreen", `the group shows its first tool (${gb?.getAttribute("data-action")})`);
    if (gb) {
      const r = gb.getBoundingClientRect();
      gb.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 3, clientY: r.top + 3 }));
      await sleep(250);
      const entries = [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")].map((m) => m.textContent);
      t(entries.length === 2, `a right click lists both tools (${entries.length})`);
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      await sleep(200);
    }
  });

  await scenario("cancel: an edit cancelled leaves the toolbar as it was", async (t) => {
    const before = actionsOf("footprint", "main");
    await openPage("fp_toolbars");
    await setValue(tid("toolbars-choice"), "TOP_MAIN");
    await click(treeRow("common.Control.save"));
    await click(tid("toolbars-delete"));
    await cancel();
    t(JSON.stringify(actionsOf("footprint", "main")) === JSON.stringify(before), "Save is still on the toolbar");
  });

  await scenario("reset: Reset Toolbars to Defaults brings KiCad's toolbars back, Customize toolbars stays on", async (t) => {
    await openPage("fp_toolbars");
    const reset = footerButton("Reset Toolbars to Defaults");
    t(!!reset, "the dialog's reset button reads \"Reset Toolbars to Defaults\"");
    await click(reset, 300);
    t(tid("toolbars-customize").checked && tid("toolbars-choice").value === "LEFT", "Customize toolbars on, the first toolbar shown");
    await ok();
    t(JSON.stringify(actionsOf("footprint", "main")) === JSON.stringify(fpStart), "the Footprint Editor's top toolbar is KiCad's again");
    await clickTab("PCB");
    await openPage("pcb_toolbars");
    await click(footerButton("Reset Toolbars to Defaults"), 300);
    await ok();
    t(JSON.stringify(actionsOf("pcb", "main")) === JSON.stringify(pcbStart.main) && JSON.stringify(actionsOf("pcb", "auxiliary")) === JSON.stringify(pcbStart.auxiliary), "the board's top and auxiliary toolbars are KiCad's again");
  });

  await scenario("end: Customize toolbars off on both; the toolbars are the defaults; the store is put back; no console errors", async (t) => {
    // customise again, then turn it off: the default toolbars come back even with a stored one
    await openPage("pcb_toolbars");
    await setValue(tid("toolbars-choice"), "TOP_MAIN");
    await click(treeRow("common.Control.plot"));
    await click(tid("toolbars-delete"));
    await ok();
    t(!actionsOf("pcb", "main").includes("common.Control.plot"), "a stored toolbar is drawn while Customize toolbars is on");
    await openPage("pcb_toolbars");
    await click(tid("toolbars-customize"));
    t(tid("toolbars-choice").disabled, "off: the page is dimmed again");
    await ok();
    t(JSON.stringify(actionsOf("pcb", "main")) === JSON.stringify(pcbStart.main), "Customize toolbars off: the board's toolbar is the default");
    await clickTab("Footprint");
    await openPage("fp_toolbars");
    await click(tid("toolbars-customize"));
    await ok();
    t(JSON.stringify(actionsOf("footprint", "main")) === JSON.stringify(fpStart), "and the Footprint Editor's");
    t(stored()?.["pcbnew-toolbars"]?.custom_toolbars === false && stored()?.["fpedit-toolbars"]?.custom_toolbars === false, "the store says both are off");
    if (storeStart === null) localStorage.removeItem("eda-studio.toolbars.v1");
    else localStorage.setItem("eda-studio.toolbars.v1", storeStart);
    await clickTab("PCB");
    t(errors.length === 0, `no console errors (${errors.slice(0, 3).join(" | ")})`);
  });

  window.removeEventListener("error", onErr);
  window.removeEventListener("unhandledrejection", onErr);
  run.ok = results.every((r) => r.ok);
  run.done = true;
  return results;
})();
