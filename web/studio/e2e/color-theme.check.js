// Colour themes of the board editor (UI-LEFT P11; PARITY-pcb.md section 75): Preferences > PCB Editor > Colors, the Appearance panel's swatches, Use Color from
// Schematic and a net class rename. It drives the dialogs' own controls and reads the canvas's pixels, the store (`localStorage` "eda-studio.color-themes.v1")
// and `GET /api/state`:
//   page            the Preferences tree has a Colors page under each editor, PCB Editor > Colors among them; the theme list is KiCad's (the editable "KiCad Default", the two read-only built-ins, ---,
//                   New Theme...); the page lists 116 layers in createSwatches' order
//   edit            the Background swatch's dialog (Color Picker, Reset to Default) sets a colour: the board is drawn in it while the dialog is up, OK keeps it and
//                   the theme is stored
//   appearance      F.Cu's swatch in the Layers tab edits the theme: the pads are drawn in the new colour
//   read-only       KiCad Classic chosen: the board is drawn in Classic's black; its swatches do not open, the Appearance panel says KiCad's read-only message
//   new theme       New Theme... asks "New theme name:" ("Add Color Theme"), copies the theme shown (Classic's F.Cu) and is selected; the same name again says
//                   "Theme already exists!"
//   reset/revert    Reset Colors to Defaults puts KiCad's defaults on the page; a swatch's menu has Revert to saved color; OK saves the reset
//   cancel          a theme picked and the dialog cancelled: the board is back in the editor's theme
//   schematic       a net class with a schematic colour: Use Color from Schematic sets its PCB colour; a class without one has the entry dimmed
//   rename          Board Setup > Net Classes: the class renamed keeps its colour under the new name (the old name's is gone)
//   end             every board step undone, the editor back on its default theme, no console errors
// It answers through `window.__colorThemeCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed
(async () => {
  const results = [];
  const run = (window.__colorThemeCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(120);
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const dialogs = () => [...document.querySelectorAll(".dialog")];
  const dialogTitled = (title) => dialogs().find((d) => d.querySelector(".dialog-header span")?.textContent.trim() === title);
  const prefs = () => document.querySelector('[data-testid="preferences-dialog"]');
  const footerButton = (dlg, text) => dlg && [...dlg.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === text);
  const click = async (el, wait = 250) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const dbl = async (el, wait = 300) => { if (!el) throw new Error("no swatch"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const rightClick = async (el, wait = 250) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.top + 4 })); await sleep(wait); };
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")];
  const menuItem = (label) => menuItems().find((m) => m.textContent.trim() === label);
  const stored = () => JSON.parse(localStorage.getItem("eda-studio.color-themes.v1") ?? "null");
  const canvas = () => document.querySelector(".pcb-canvas-container canvas");
  /** The colour the canvas has at its top-left corner (the background, off the board). */
  const corner = () => {
    // the most common colour of a few points near the corner (a grid dot may sit on one of them)
    const c = canvas();
    const ctx = c.getContext("2d");
    const seen = new Map();
    for (let x = 3; x < 60; x += 7) for (let y = 3; y < 60; y += 7) { const d = ctx.getImageData(x, y, 1, 1).data; const k = `${d[0]},${d[1]},${d[2]}`; seen.set(k, (seen.get(k) ?? 0) + 1); }
    return [...seen.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  };
  const count = (r, g, b, tol = 6) => {
    const c = canvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - r) <= tol && Math.abs(d[i + 1] - g) <= tol && Math.abs(d[i + 2] - b) <= tol) n++;
    return n;
  };
  const hexRgb = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
  const near = (a, b, tol = 3) => a.every((v, i) => Math.abs(v - b[i]) <= tol);
  const openColors = async () => {
    if (!prefs()) { await __eda.run("common.SuiteControl.openPreferences"); await sleep(400); }
    // each editor has a Colors page (UI-LEFT-2 SS9, FF7): the board editor's is PCB Editor > Colors
    await click(prefs().querySelector('[data-testid="prefs-page-pcb_colors"]'), 300);
    const page = document.querySelector('[data-testid="pcb-colors-page"]');
    if (!page) throw new Error("no Colors page");
    return page;
  };
  const themeSelect = () => document.querySelector('[data-testid="pcb-colors-theme"]');
  const pageSwatch = (key) => document.querySelector(`[data-color-key="${key}"] [role=button]`);
  const pickColor = async (hex) => {
    const dlg = dialogTitled("Color Picker");
    if (!dlg) throw new Error("no Color Picker");
    await setValue(dlg.querySelector('input[type="color"]'), hex);
    await click(footerButton(dlg, "OK"), 300);
  };
  const prefsOk = async () => click(footerButton(prefs(), "OK"), 400);
  const prefsCancel = async () => click(footerButton(prefs(), "Cancel"), 400);
  const DEFAULT_BG = [0, 16, 35]; // s_defaultTheme LAYER_PCB_BACKGROUND, CSS_COLOR( 0, 16, 35, 1 )

  // fresh themes: the store starts from an empty localStorage in the check's browser
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);
  const before = await json("/api/state");
  const steps = [];

  await scenario("the Colors page: the tree, KiCad's theme list and the layers in createSwatches' order", async (t) => {
    t(near(corner(), DEFAULT_BG), `the board starts on KiCad Default's background (${corner()})`);
    const page = await openColors();
    t(prefs().querySelector(".dialog-header span").textContent.trim() === "Preferences - Colors", "the dialog's title names the page");
    // KiCad's tree (`EDA_BASE_FRAME::ShowPreferences`): every editor has its Colors page, the board editor's under "PCB Editor" (UI-LEFT-2 CC2, SS9, FF7)
    const items = [...prefs().querySelectorAll("[role=treeitem]")];
    const tree = items.map((e) => e.textContent.trim());
    const parentOf = (id) => { const i = items.findIndex((e) => e.getAttribute("data-testid") === `prefs-page-${id}`); for (let j = i - 1; j >= 0; j--) if (!items[j].getAttribute("data-testid")) return tree[j]; return null; };
    t(
      [["sym_colors", "Symbol Editor"], ["sch_colors", "Schematic Editor"], ["fp_colors", "Footprint Editor"], ["pcb_colors", "PCB Editor"]].every(([id, editor]) => parentOf(id) === editor && tree[items.findIndex((e) => e.getAttribute("data-testid") === `prefs-page-${id}`)] === "Colors"),
      `each editor's Colors page (${["sym_colors", "sch_colors", "fp_colors", "pcb_colors"].map(parentOf).join(", ")})`
    );
    const opts = [...themeSelect().options].map((o) => o.textContent);
    t(opts.join("|") === "KiCad Classic (read-only)|KiCad Default (read-only)|KiCad Default|---|New Theme...", `the theme list (${opts.join("|")})`);
    t(themeSelect().value === "user", `the editor's theme is "user" (${themeSelect().value})`);
    const rows = [...page.querySelectorAll("[data-color-key]")];
    t(rows.length === 116, `116 layers (${rows.length})`);
    const labels = rows.map((r) => r.textContent.trim());
    t(labels[0] === "F.Cu" && labels[31] === "B.Cu" && labels[32] === "F.Adhesive" && labels[95] === "Anchors" && labels[115] === "Via holes", `the order (${labels[0]}, ${labels[31]}, ${labels[32]}, ${labels[95]}, ${labels[115]})`);
    t(!!footerButton(prefs(), "Reset Colors to Defaults"), "the dialog's Reset Colors to Defaults");
  });

  await scenario("a swatch of the page: the Color Picker with Reset to Default; the board drawn in the page's colour, OK keeps it", async (t) => {
    await dbl(pageSwatch("LAYER_PCB_BACKGROUND"));
    const dlg = dialogTitled("Color Picker");
    t(!!dlg && !!footerButton(dlg, "OK") && [...dlg.querySelectorAll("button")].some((b) => b.textContent.trim() === "Reset to Default"), "the Color Picker opens with Reset to Default");
    await pickColor("#203040");
    await sleep(300);
    t(near(corner(), [32, 48, 64]), `the board is drawn in the new background while the dialog is up (${corner()})`);
    const s1 = stored();
    t(s1?.themes?.find((x) => x.filename === "user")?.colors?.LAYER_PCB_BACKGROUND === "#203040ff", `the edit is saved at once (${JSON.stringify(s1?.themes?.map((x) => [x.filename, x.colors?.LAYER_PCB_BACKGROUND]))})`);
    await prefsOk();
    await sleep(300);
    t(!prefs() && near(corner(), [32, 48, 64]), `after OK the board keeps it (${corner()})`);
  });

  await scenario("the Appearance panel's F.Cu swatch edits the theme: the pads are drawn in it", async (t) => {
    document.querySelector('button[aria-label="Show Appearance"]')?.click();
    await sleep(300);
    [...document.querySelectorAll('.dock-tab')].find((d) => d.textContent.trim() === "Appearance")?.click();
    await sleep(200);
    document.querySelector('.ap-root [data-tab="layers"]')?.click();
    await sleep(200);
    const sw = document.querySelector('.ap-row[data-layer="F.Cu"] .ap-swatch');
    t(sw?.getAttribute("title") === "Double click or middle click for color change, right click for menu", `the swatch's tooltip (${sw?.getAttribute("title")})`);
    const greenBefore = count(0, 255, 0);
    await dbl(sw);
    t(!!dialogTitled("Color Picker"), "the Color Picker opens");
    await pickColor("#00ff00");
    await sleep(500);
    const greenAfter = count(0, 255, 0);
    t(greenAfter > greenBefore + 50, `the F.Cu pads are green (${greenBefore} -> ${greenAfter} pixels)`);
    t(stored()?.themes?.find((x) => x.filename === "user")?.colors?.F_Cu === "#00ff00ff", "the theme keeps it");
  });

  await scenario("a read-only theme: Classic chosen is drawn; its swatches do not open; the panel says KiCad's message", async (t) => {
    await openColors();
    await setValue(themeSelect(), "_builtin_classic");
    await sleep(300);
    t(near(corner(), [0, 0, 0]), `the page's theme is drawn while the dialog is up (${corner()})`);
    await dbl(pageSwatch("F_Cu"));
    t(!dialogTitled("Color Picker"), "a read-only theme's swatch does not open");
    t(!footerButton(prefs(), "Reset Colors to Defaults").disabled, "Reset is there (it does nothing on a read-only theme)");
    await prefsOk();
    t(near(corner(), [0, 0, 0]) && stored()?.pcbTheme === "_builtin_classic", `OK makes Classic the editor's theme (${corner()}, ${stored()?.pcbTheme})`);
    const red = count(132, 0, 0, 4);
    t(red > 50, `F.Cu is Classic's red (${red} pixels)`);
    await dbl(document.querySelector('.ap-row[data-layer="F.Cu"] .ap-swatch'));
    t(!dialogTitled("Color Picker"), "the Appearance swatch does not open");
    const toast = [...document.querySelectorAll("div")].find((d) => d.textContent === "The current color theme is read-only.  Create a new theme in Preferences to enable color editing.");
    t(!!toast, "KiCad's read-only message");
  });

  await scenario("New Theme...: the name, the shown theme's colours, the theme selected; the same name: Theme already exists!", async (t) => {
    await openColors();
    await setValue(themeSelect(), "new");
    const ask = dialogTitled("Add Color Theme");
    t(!!ask && ask.textContent.includes("New theme name:"), "Add Color Theme asks New theme name:");
    await setValue(ask.querySelector("input"), "My Theme");
    await click(footerButton(ask, "OK"), 300);
    t(themeSelect().value === "My Theme", `the new theme is selected (${themeSelect().value})`);
    const mine = stored()?.themes?.find((x) => x.filename === "My Theme");
    t(mine?.name === "My Theme" && mine?.colors?.F_Cu === "#840000ff" && mine?.colors?.LAYER_PCB_BACKGROUND === "#000000ff", `it has Classic's colours (${mine?.colors?.F_Cu}, ${mine?.colors?.LAYER_PCB_BACKGROUND})`);
    const opts = [...themeSelect().options].map((o) => o.textContent);
    t(opts.includes("My Theme") && opts.indexOf("My Theme") < opts.indexOf("---"), `listed before --- (${opts.join("|")})`);
    await setValue(themeSelect(), "new");
    const again = dialogTitled("Add Color Theme");
    await setValue(again.querySelector("input"), "My Theme");
    await click(footerButton(again, "OK"), 300);
    const msg = dialogTitled("Message");
    t(!!msg && msg.textContent.includes("Theme already exists!"), "Theme already exists!");
    if (msg) await click(footerButton(msg, "OK"), 200);
    // the new theme is editable: its swatch opens
    await dbl(pageSwatch("F_Cu"));
    t(!!dialogTitled("Color Picker"), "the new theme's swatches open");
    const cp = dialogTitled("Color Picker");
    if (cp) await click(footerButton(cp, "Cancel"), 200);
  });

  await scenario("Reset Colors to Defaults, Revert to saved color, OK saves the reset", async (t) => {
    await click(footerButton(prefs(), "Reset Colors to Defaults"), 300);
    await sleep(200);
    t(near(corner(), DEFAULT_BG), `the page shows KiCad's default background (${corner()})`);
    await rightClick(pageSwatch("LAYER_PCB_BACKGROUND"));
    const items = menuItems().map((m) => m.textContent.trim());
    t(items.join("|") === "Copy color|Revert to saved color", `the swatch's menu (${items.join("|")})`);
    await click(menuItem("Revert to saved color"), 300);
    t(near(corner(), [0, 0, 0]), `Revert to saved color puts Classic's black back (${corner()})`);
    await rightClick(pageSwatch("F_Cu"));
    await click(menuItem("Copy color"), 200);
    await rightClick(pageSwatch("LAYER_PCB_BACKGROUND"));
    t(!!menuItem("Paste color"), "Paste color once a colour is copied");
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    document.body.click();
    await sleep(200);
    await click(footerButton(prefs(), "Reset Colors to Defaults"), 300);
    await prefsOk();
    const mine = stored()?.themes?.find((x) => x.filename === "My Theme");
    t(stored()?.pcbTheme === "My Theme" && mine?.colors?.LAYER_PCB_BACKGROUND === "#001023ff" && near(corner(), DEFAULT_BG), `OK saved the reset and chose the theme (${stored()?.pcbTheme}, ${mine?.colors?.LAYER_PCB_BACKGROUND}, ${corner()})`);
  });

  await scenario("Cancel: a theme picked on the page, the dialog cancelled, the board back in the editor's theme", async (t) => {
    await openColors();
    await setValue(themeSelect(), "_builtin_classic");
    await sleep(200);
    t(near(corner(), [0, 0, 0]), "the page's theme on the board");
    await prefsCancel();
    await sleep(200);
    t(near(corner(), DEFAULT_BG) && stored()?.pcbTheme === "My Theme", `back to the editor's theme (${corner()}, ${stored()?.pcbTheme})`);
  });

  // a class with a schematic colour and one without, through the verb Board Setup sends (one undo step)
  const rules = before.board_rules;
  const net = [...new Set(before.parts.flatMap((p) => (p.pads ?? []).map((x) => x.net)).filter((n) => n))].sort()[0];
  const def = rules.default_class ?? { name: "Default", nets: [], track_width: rules.track_width, clearance: rules.clearance, via_diameter: rules.via_diameter, via_drill: rules.via_drill, priority: 0 };
  const classes = [...rules.net_classes, { name: "theme_sch", nets: [net], priority: 0 }, { name: "theme_plain", nets: [], priority: 0 }];
  const schematic = { ...(rules.net_class_schematic ?? {}), theme_sch: { color: "rgb(0, 200, 120)" } };
  const reply = await post("/api/cmd", { cmd: { op: "set_net_classes", settings: { default: { ...def, name: "Default", nets: [] }, classes, schematic } } });
  if (reply.ok !== false) steps.push("classes");
  await sleep(800);

  await scenario("Use Color from Schematic: the class's schematic colour becomes its PCB colour; dimmed for a class without one", async (t) => {
    t(reply.ok !== false, `the classes are added (${JSON.stringify(reply).slice(0, 200)})`);
    document.querySelector('.ap-root [data-tab="classes"]')?.click();
    await sleep(300);
    const row = document.querySelector('.ap-row[data-netclass="theme_sch"]');
    t(!!row, "the class is listed");
    await rightClick(row);
    const use = menuItem("Use Color from Schematic");
    t(!!use && use.getAttribute("aria-disabled") !== "true", "Use Color from Schematic is on");
    await click(use, 300);
    // the class's PCB colour is a cell of its net class (`NETCLASS::SetPcbColor`), set through `set_net_classes`: an undo step here (UI-LEFT-2 BB8)
    for (let i = 0; i < 30 && __eda.state().appearance.netclassColors?.theme_sch !== "rgb(0, 200, 120)"; i++) await sleep(100);
    if (__eda.state().appearance.netclassColors?.theme_sch === "rgb(0, 200, 120)") steps.push("color");
    t(__eda.state().appearance.netclassColors?.theme_sch === "rgb(0, 200, 120)", `the PCB colour (${JSON.stringify(__eda.state().appearance.netclassColors)})`);
    await rightClick(document.querySelector('.ap-row[data-netclass="theme_plain"]'));
    t(menuItem("Use Color from Schematic")?.getAttribute("aria-disabled") === "true", "dimmed for a class with no schematic colour");
    document.body.click();
    await sleep(200);
  });

  await scenario("a net class renamed in Board Setup keeps its colour", async (t) => {
    await __eda.run("pcbnew.EditorControl.boardSetup");
    await sleep(500);
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Net Classes"))?.click();
    await sleep(300);
    const p = document.querySelector('section.bs-page[aria-label="Net Classes"]');
    const name = [...p.querySelectorAll('input[aria-label^="Netclass "][aria-label$=" name"]')].find((i) => i.value === "theme_sch");
    t(!!name, "the class's name cell");
    for (const v of ["theme_sc", "theme_s", "theme_s2"]) await setValue(name, v); // typed a key at a time
    [...p.querySelectorAll("button")].find((b) => b.textContent.trim() === "Apply")?.click();
    for (let i = 0; i < 60; i++) { const m = p.querySelector(".bs-msg")?.textContent ?? ""; if (m && m !== "Changes not applied yet.") break; await sleep(100); }
    const msg = p.querySelector(".bs-msg")?.textContent ?? "";
    if (msg === "Saved.") steps.push("rename");
    await sleep(500);
    const colors = __eda.state().appearance.netclassColors ?? {};
    t(msg === "Saved." && colors.theme_s2 === "rgb(0, 200, 120)" && colors.theme_sch === undefined, `${msg}: the colour is under the new name (${JSON.stringify(colors)})`);
    for (let i = 0; i < 3 && document.querySelector(".bs-dialog"); i++) {
      [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((b) => ["Close", "Discard and close"].includes(b.textContent.trim()))?.click();
      await sleep(300);
    }
  });

  await scenario("end: the board steps undone, the editor back on its default theme, no console errors", async (t) => {
    const made = steps.length;
    while (steps.length) { steps.pop(); await post("/api/undo", { domain: "pcb" }); await sleep(500); }
    const after = await json("/api/state");
    t(JSON.stringify(after.board_rules?.net_classes) === JSON.stringify(before.board_rules?.net_classes), `${made} steps undone, the classes as they were`);
    await __eda.run("studio.Appearance.op", { op: "netclass_color", name: "theme_s2", color: null });
    await __eda.run("studio.Appearance.op", { op: "netclass_color", name: "theme_sch", color: null });
    await openColors();
    await setValue(themeSelect(), "user");
    await click(footerButton(prefs(), "Reset Colors to Defaults"), 200);
    await prefsOk();
    t(stored()?.pcbTheme === "user" && near(corner(), DEFAULT_BG), `the editor is on its default theme with KiCad's colours (${stored()?.pcbTheme}, ${corner()})`);
    localStorage.removeItem("eda-studio.color-themes.v1");
    t((__eda.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(__eda.errors?.() ?? []).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
