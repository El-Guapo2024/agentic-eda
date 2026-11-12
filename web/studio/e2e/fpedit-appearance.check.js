// Footprint Editor Appearance manager check (UI-LEFT F2; components/footprint/FootprintAppearancePanel.tsx, kicad-port/fpAppearance.ts): KiCad's `APPEARANCE_CONTROLS` in its
// footprint-editor form, the "LayersManager" pane of footprint_edit_frame.cpp. On a new footprint it checks that the pane sits outside the drawing toolbar with the Layers and
// Objects pages and is shown and hidden by Show Appearance Manager (`pcbnew.Control.showLayersManager`, the toolbar button); that the Layers page lists the editor's board
// (F.Cu, Inner layers, B.Cu, the technical layers, User.1 .. User.4); that a layer's eye hides its graphics on the canvas and the canvas no longer picks them; that the Objects
// rows (s_allowedInFpEditor) hide the pads, the Reference and the Value and stop them being picked, Footprint Text taking References and Values along and the Grid row being the
// grid switch; that a click on a layer row makes it the active layer the next line is drawn on; the layer menu's Hide All Non Copper Layers / Show All Layers; and the
// Inactive layers modes (Dim, Hide, H cycling them) with high contrast picking only the active layer's items.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board
// (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes a footprint of its own (New Footprint) and
// deletes it at the end, leaving the project library as it found it. It answers through `window.__fpAppearanceCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ...", "note: ..."] }], done, ok, wait(ms) }
// Read it with `await __fpAppearanceCheck.wait(60000)` until `.done`.
// run-all-checks: board=mcu30 minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__fpAppearanceCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    t.note = (msg) => entry.notes.push(`note: ${msg}`);
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** A drawing tool's click: Ctrl+Shift, so neither the grid nor an anchor moves the point. */
  const fire = (type, wx, wy, buttons = 0, mods = true) => {
    const [x, y] = toScreen(wx, wy);
    const mac = /Mac/.test(navigator.platform);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: mods && !mac, metaKey: mods && mac, shiftKey: mods }));
  };
  const clickAt = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(70); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(500); };
  const key = async (k) => { canvasEl().focus(); canvasEl().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(250); };
  const arm = async (action, tool) => { if (__eda.state().tool !== tool) { await __eda.run(action); await sleep(250); } };
  const toSelect = async () => { for (let i = 0; i < 4 && __eda.state().tool !== "select"; i++) await key("Escape"); };
  /** A plain click of the select tool (no modifier): what it picks. */
  const pick = async (wx, wy) => {
    await __eda.run("common.InteractiveSelection.clear");
    await sleep(150);
    fire("pointermove", wx, wy, 0, false); await sleep(60);
    fire("pointerdown", wx, wy, 1, false); await sleep(40);
    fire("pointerup", wx, wy, 0, false); await sleep(350);
    const ids = __eda.state().selection.map((s) => s.id);
    await __eda.run("common.InteractiveSelection.clear");
    await sleep(250);
    return ids;
  };
  /** How much the canvas differs from its background around a world box (the sum of the colour differences of its pixels); NaN when the box is not all on the canvas. */
  const ink = (wx0, wy0, wx1, wy1, margin = 200_000) => {
    const c = canvasEl().querySelector("canvas");
    const r = c.getBoundingClientRect();
    const k = c.width / r.width;
    const ctx = c.getContext("2d");
    const bg = ctx.getImageData(2, 2, 1, 1).data;
    const [ax, ay] = toScreen(Math.min(wx0, wx1) - margin, Math.min(wy0, wy1) - margin);
    const [bx, by] = toScreen(Math.max(wx0, wx1) + margin, Math.max(wy0, wy1) + margin);
    const x = Math.floor((ax - r.left) * k);
    const y = Math.floor((ay - r.top) * k);
    const w = Math.ceil((bx - ax) * k);
    const h = Math.ceil((by - ay) * k);
    if (x < 0 || y < 0 || w <= 0 || h <= 0 || x + w > c.width || y + h > c.height) return NaN; // off the canvas: every comparison with it fails
    const d = ctx.getImageData(x, y, w, h).data;
    let sum = 0;
    for (let i = 0; i < d.length; i += 4) sum += Math.abs(d[i] - bg[0]) + Math.abs(d[i + 1] - bg[1]) + Math.abs(d[i + 2] - bg[2]);
    return sum;
  };
  /** The Footprint Editor's layer box (its top toolbar). */
  const layerBox = () => [...document.querySelectorAll('.toolbar[data-editor="footprint"] .toolbar-control select')].find((s) => [...s.options].some((o) => o.value === "F.CrtYd"));
  const pane = () => document.querySelector('[data-testid="fp-appearance"]');
  const page = async (id) => { pane()?.querySelector(`[data-tab="${id}"]`)?.click(); await sleep(250); };
  const layerRow = (name) => pane()?.querySelector(`.ap-row[data-layer="${name}"]`);
  const objectRow = (id) => pane()?.querySelector(`.ap-row[data-object="${id}"]`);
  const eyeOn = (row) => row?.querySelector(".ap-eye")?.getAttribute("aria-pressed") === "true";
  const clickEye = async (row) => { row?.querySelector(".ap-eye")?.click(); await sleep(350); };
  const activeRow = () => pane()?.querySelector(".ap-row.active[data-layer]")?.getAttribute("data-layer") ?? null;
  const setActive = async (name) => { layerRow(name)?.querySelector(".ap-name")?.click(); await sleep(300); };
  const layerMenu = async (label) => {
    pane().querySelector(".ap-scroll").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 200, clientY: 300 }));
    await sleep(250);
    const items = [...document.querySelectorAll('.menubar-dropdown [role="menuitem"]')];
    const labels = items.map((i) => i.textContent.trim());
    const hit = items.find((i) => i.textContent.trim() === label);
    hit?.click();
    await sleep(350);
    return { labels, found: !!hit };
  };
  const toolbarButton = (label) => [...document.querySelectorAll('.toolbar[data-editor="footprint"] button.toolbar-button')].find((b) => b.title === label || b.title.startsWith(`${label} (`) || b.title.startsWith(`${label} —`));
  const setRange = async (input, value) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, String(value));
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(300);
  };
  const graphics = async (name) => (await footprint(name)).graphics ?? [];
  const drawLine = async (name, x0, y0, x1, y1) => {
    const before = (await graphics(name)).map((g) => g.id);
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    await clickAt(x0, y0);
    await clickAt(x1, y1);
    await toSelect();
    await sleep(300);
    return (await graphics(name)).find((g) => !before.includes(g.id)) ?? null;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const libraryBefore = await fpNames();
  await clickTab("Footprint");
  await __eda.run("pcbnew.ModuleEditor.newFootprint");
  await sleep(900);
  const name = (await fpNames()).find((n) => !libraryBefore.includes(n));
  const items = {};
  // where the lines go: clear of the new footprint's fields (Reference at y = -0.5 mm, Value at 1 mm) and of its `${REFERENCE}` text on F.Fab at 2.5 mm
  const SILK_Y = -2_000_000;
  const FAB_Y = -3_000_000;

  await scenario("the Appearance pane: captioned Appearance, outside the drawing toolbar, Layers and Objects pages; Show Appearance Manager shows and hides it", async (t) => {
    t(!!name, `a new footprint: ${name}`);
    const p = pane();
    if (!t(!!p, "the pane is there to begin with (appearanceShown)")) return;
    t(/Appearance/.test(p.querySelector(".dock-panel-caption")?.textContent ?? ""), "its caption is Appearance");
    t(JSON.stringify([...p.querySelectorAll("[data-tab]")].map((e) => e.textContent.trim())) === JSON.stringify(["Layers", "Objects"]), "KiCad's two pages, Layers and Objects (the footprint editor drops Nets)");
    const drawing = document.querySelector('.editor-frame-body > .toolbar[data-toolbar="drawing"]');
    const pr = p.getBoundingClientRect();
    const dr = drawing?.getBoundingClientRect();
    t(!!dr && pr.left >= dr.right - 1, `it is docked outside the drawing toolbar (toolbar right ${dr?.right}, pane left ${pr.left})`);
    const btn = toolbarButton("Appearance");
    if (btn && !btn.disabled) {
      t(btn.getAttribute("aria-pressed") === "true", "the toolbar's Appearance button is pressed while the pane shows");
      btn.click();
      await sleep(400);
      t(!pane() && btn.getAttribute("aria-pressed") !== "true", "the button hides the pane");
      toolbarButton("Appearance").click();
      await sleep(400);
      t(!!pane() && toolbarButton("Appearance").getAttribute("aria-pressed") === "true", "and shows it again");
    } else {
      // editorFrameActions.ts unregisters on the tab every toolbar action editor_toolbar_support.json does not call supported, so neither the button nor the action runs yet
      t(!!btn, "the toolbar has the Appearance button");
      t.note(`skipped the show / hide: the button is dead (${btn?.title}) until editor_toolbar_support.json's pcbnew.Control.showLayersManager row says supported`);
    }
    await page("layers");
    const rows = [...pane().querySelectorAll(".ap-row[data-layer]")];
    const keys = rows.map((r) => r.getAttribute("data-layer"));
    const labels = rows.map((r) => r.querySelector(".ap-name")?.textContent.trim());
    t(rows.length === 25, `25 layer rows (3 copper, 18 technical, 4 user): ${rows.length}`);
    t(JSON.stringify(labels.slice(0, 3)) === JSON.stringify(["F.Cu", "Inner layers", "B.Cu"]), `copper first, In1.Cu named Inner layers: ${labels.slice(0, 3)}`);
    t(labels[keys.indexOf("F.SilkS")] === "F.Silkscreen" && labels[keys.indexOf("F.CrtYd")] === "F.Courtyard" && labels[keys.indexOf("Dwgs.User")] === "User.Drawings", "the technical layers by their standard names");
    t(keys[keys.length - 1] === "User.4", `the user layers end the list: ${keys.slice(-4)}`);
    t(layerRow("Edge.Cuts")?.querySelector(".ap-name")?.title === "Board's perimeter definition", "a row's tooltip is non_cu_seq's");
    t(activeRow() === "F.SilkS", `the active layer (F.SilkS to begin with) is marked: ${activeRow()}`);
  });

  await scenario("Objects: s_allowedInFpEditor's rows; Grid is the grid switch; Footprint Text takes References and Values along", async (t) => {
    await page("objects");
    const ids = [...pane().querySelectorAll(".ap-row[data-object]")].map((r) => r.getAttribute("data-object"));
    t(JSON.stringify(ids) === JSON.stringify(["tracks", "vias", "pads", "zones", "shapes", "bitmaps", "footprint_values", "footprint_references", "footprint_text", "ly_points", "via_stitching", "grid"]), `the rows: ${ids.join(",")}`);
    t(pane().querySelectorAll('.ap-row[data-object] input[type="range"]').length === 6, "six opacity sliders (tracks, vias, pads, zones, filled shapes, images)");
    const gridBtn = toolbarButton("Show Grid");
    const gridBefore = gridBtn?.getAttribute("aria-pressed");
    t(eyeOn(objectRow("grid")) && gridBefore === "true", `the Grid row and Show Grid are on (${gridBefore})`);
    await clickEye(objectRow("grid"));
    t(!eyeOn(objectRow("grid")) && toolbarButton("Show Grid")?.getAttribute("aria-pressed") !== "true", "the Grid row's eye turns the grid off, and the toolbar's Show Grid with it");
    // the grid stays off for the pixel checks below; the last scenario turns it back on
    await clickEye(objectRow("footprint_text"));
    t(!eyeOn(objectRow("footprint_text")) && !eyeOn(objectRow("footprint_references")) && !eyeOn(objectRow("footprint_values")), "Footprint Text off turns References and Values off");
    await clickEye(objectRow("footprint_references"));
    t(eyeOn(objectRow("footprint_references")) && eyeOn(objectRow("footprint_text")) && !eyeOn(objectRow("footprint_values")), "References on puts Footprint Text back on, Values stays off");
    await clickEye(objectRow("footprint_values"));
    t(eyeOn(objectRow("footprint_values")), "Values back on");
  });

  await scenario("a layer's eye hides its graphics and the canvas no longer picks them", async (t) => {
    await page("layers");
    await setActive("F.SilkS");
    const silk = await drawLine(name, -2_000_000, SILK_Y, 2_000_000, SILK_Y);
    items.silk = silk;
    if (!t(!!silk && silk.layer === "F.SilkS", `a line drawn on F.SilkS: ${JSON.stringify(silk && [silk.id, silk.layer])}`)) return;
    // a pad, for the Objects scenario, then the view fitted to everything
    await arm("pcbnew.PadTool.placePad", "pad");
    await clickAt(4_000_000, 0);
    await toSelect();
    // Zoom to Fit fits the graphics and the pads; two steps out leave room for the fields, the texts and the lines drawn later
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    await __eda.run("common.Control.zoomOutCenter");
    await __eda.run("common.Control.zoomOutCenter");
    await sleep(500);
    const inked = ink(-2_000_000, SILK_Y, 2_000_000, SILK_Y);
    t(inked > 0, `the line is drawn (${inked})`);
    t(JSON.stringify(await pick(0, SILK_Y)) === JSON.stringify([silk.id]), "a click on it picks it");
    await clickEye(layerRow("F.SilkS"));
    t(!eyeOn(layerRow("F.SilkS")), "F.SilkS's eye is off");
    const hidden = ink(-2_000_000, SILK_Y, 2_000_000, SILK_Y);
    t(hidden < inked * 0.1, `the line is gone from the canvas (${inked} -> ${hidden})`);
    t((await pick(0, SILK_Y)).length === 0, "and a click there picks nothing");
    await clickEye(layerRow("F.SilkS"));
    t(eyeOn(layerRow("F.SilkS")) && JSON.stringify(await pick(0, SILK_Y)) === JSON.stringify([silk.id]), "its eye on again: the line is picked again");
  });

  await scenario("the Objects rows hide the pads, the Reference and the Value: the canvas draws less there; the Reference and the Value are not picked, a pad is", async (t) => {
    const fp = await footprint(name);
    const p = (fp.pads ?? [])[0];
    if (!t(!!p, `the footprint has a pad: ${p?.id}`)) return;
    const ref = fp.reference?.at ?? { x: 0, y: -500_000 };
    const val = fp.value?.at ?? { x: 0, y: 1_000_000 };
    await page("objects");
    const cases = [
      ["pads", p.at, p.id],
      ["footprint_references", ref, "field:Reference"],
      ["footprint_values", val, "field:Value"],
    ];
    for (const [id, at, item] of cases) {
      // a pad's box takes its copper ring (the hole alone is drawn near the background colour, which is the theme's since UI-LEFT-2 FF5)
      const r = id === "pads" ? 700_000 : 300_000;
      const before = ink(at.x - r, at.y - r, at.x + r, at.y + r, 0);
      const picked = await pick(at.x, at.y);
      t(JSON.stringify(picked) === JSON.stringify([item]), `${id}: a click at ${at.x},${at.y} picks ${item}: ${picked}`);
      await clickEye(objectRow(id));
      const after = ink(at.x - r, at.y - r, at.x + r, at.y + r, 0);
      t(!eyeOn(objectRow(id)) && after < before * 0.5, `${id} off: drawn less (${before} -> ${after})`);
      // `Selectable` asks no Pads row of a pad (the element visibility it asks of zones and points is not the rows' in this editor either): a hidden pad is still
      // picked, as KiCad's is (UI-LEFT-2 FF5); a hidden Reference or Value is not (`LAYER_FP_REFERENCES`, `LAYER_FP_VALUES` are the view's layers)
      if (id === "pads") t((await pick(at.x, at.y)).includes(item), `${id} off: ${item} is still picked (KiCad's Selectable)`);
      else t(!(await pick(at.x, at.y)).includes(item), `${id} off: ${item} is not picked`);
      await clickEye(objectRow(id));
      t(eyeOn(objectRow(id)), `${id} on again`);
    }
    // a pad at opacity 0 is not picked either (`m_PadOpacity == 0.00`)
    const slider = objectRow("pads")?.querySelector('input[type="range"]');
    if (t(!!slider, "the Pads row has its slider")) {
      await setRange(slider, 0);
      t(!(await pick(p.at.x, p.at.y)).includes(p.id), "Pads at opacity 0: the pad is not picked");
      await setRange(slider, 100);
      t(JSON.stringify(await pick(p.at.x, p.at.y)) === JSON.stringify([p.id]), "back to 100 %: picked again");
    }
  });

  await scenario("a click on a layer row makes it the active layer: the row is marked, the layer box follows, the next line is drawn on it", async (t) => {
    await page("layers");
    for (const [layer, y] of [["F.Fab", FAB_Y], ["Dwgs.User", FAB_Y - 1_000_000]]) {
      await setActive(layer);
      t(activeRow() === layer, `${layer}'s row is marked active: ${activeRow()}`);
      t(layerBox()?.value === layer, `the layer box shows ${layer}: ${layerBox()?.value}`);
      const g = await drawLine(name, -1_000_000, y, 1_000_000, y);
      t(!!g && g.layer === layer, `the line is drawn on ${layer}: ${JSON.stringify(g && [g.id, g.layer])}`);
      if (layer === "F.Fab") items.fab = g;
    }
    await setActive("F.SilkS");
  });

  await scenario("the layer menu: Hide All Non Copper Layers keeps the copper layers and Edge.Cuts and moves the active layer; Show All Layers brings the rest back", async (t) => {
    await page("layers");
    const { labels, found } = await layerMenu("Hide All Non Copper Layers");
    t(found && labels.includes("Show Only Inner Layers") && labels[0] === "Show All Copper Layers", `the menu is KiCad's, with Show Only Inner Layers (three copper layers): ${labels.join(" | ")}`);
    const on = [...pane().querySelectorAll(".ap-row[data-layer]")].filter((r) => eyeOn(r)).map((r) => r.getAttribute("data-layer"));
    t(JSON.stringify(on) === JSON.stringify(["F.Cu", "In1.Cu", "B.Cu", "Edge.Cuts"]), `what is left on: ${on}`);
    t(activeRow() === "F.Cu", `the active layer moved to the first one left on: ${activeRow()}`);
    t(items.silk && (await pick(0, SILK_Y)).length === 0, "the silkscreen line is not picked");
    await layerMenu("Show All Layers");
    const off = [...pane().querySelectorAll(".ap-row[data-layer]")].filter((r) => !eyeOn(r)).map((r) => r.getAttribute("data-layer"));
    t(off.length === 0, `Show All Layers: every layer on (${off})`);
    await setActive("F.SilkS");
  });

  await scenario("Inactive layers: Dim and Hide (H goes round Normal, Dim, Hide); with high contrast only the active layer's items are picked", async (t) => {
    await page("layers");
    pane().querySelector(".ap-pane")?.setAttribute("open", "");
    const radio = (label) => [...pane().querySelectorAll('.ap-radios label')].find((l) => l.textContent.trim() === label)?.querySelector("input");
    const mode = () => ["Normal", "Dim", "Hide"].find((l) => radio(l)?.checked) ?? null;
    if (!t(!!radio("Normal") && !!radio("Dim") && !!radio("Hide") && mode() === "Normal", `Inactive layers: Normal / Dim / Hide, Normal to begin with (${mode()})`)) return;
    t(/Inactive layers \(H\)/.test(pane().textContent), "the label names its hotkey");
    await setActive("F.Fab");
    const box = [-2_000_000, SILK_Y, 2_000_000, SILK_Y];
    const normal = ink(...box);
    radio("Dim").click();
    await sleep(350);
    const dim = ink(...box);
    t(toolbarButton("Inactive Layer View Mode")?.getAttribute("aria-pressed") === "true", "Dim turns the toolbar's high contrast on");
    t(dim < normal && dim > 0, `Dim: the F.SilkS line is dimmed (${normal} -> ${dim})`);
    t((await pick(0, SILK_Y)).length === 0, "with high contrast the F.SilkS line is not picked (F.Fab is active)");
    t(!!items.fab && JSON.stringify(await pick(0, FAB_Y)) === JSON.stringify([items.fab.id]), "the F.Fab line is");
    radio("Hide").click();
    await sleep(350);
    const hide = ink(...box);
    t(hide < dim * 0.2, `Hide: the F.SilkS line is not drawn (${dim} -> ${hide})`);
    await __eda.run("common.Control.highContrastModeCycle");
    await sleep(300);
    t(mode() === "Normal", `H from Hide: Normal (${mode()})`);
    await __eda.run("common.Control.highContrastModeCycle");
    await sleep(300);
    t(mode() === "Dim", `H: Dim (${mode()})`);
    await __eda.run("common.Control.highContrastModeCycle");
    await sleep(300);
    t(mode() === "Hide", `H: Hide (${mode()})`);
    radio("Normal").click();
    await sleep(350);
    t(mode() === "Normal" && toolbarButton("Inactive Layer View Mode")?.getAttribute("aria-pressed") !== "true", "Normal: high contrast off");
    t(JSON.stringify(await pick(0, SILK_Y)) === JSON.stringify([items.silk?.id]), "and the F.SilkS line is picked again");
    await setActive("F.SilkS");
  });

  // ================================================================================================ leave things as they were
  await scenario("the grid back on, the pane shown, the footprint deleted: the project library is left as it was found", async (t) => {
    await toSelect();
    await page("objects");
    if (!eyeOn(objectRow("grid"))) await clickEye(objectRow("grid"));
    t(eyeOn(objectRow("grid")) && toolbarButton("Show Grid")?.getAttribute("aria-pressed") === "true", "the grid is on again");
    await page("layers");
    await closeDialogs();
    if (name) await cmd({ op: "delete_library_footprint", name });
    await sleep(500);
    const libraryAfter = await fpNames();
    t(JSON.stringify(libraryAfter) === JSON.stringify(libraryBefore), `the same names: ${JSON.stringify({ before: libraryBefore.length, after: libraryAfter.length })}`);
  });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
