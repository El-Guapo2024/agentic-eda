// The Footprint Editor's drawing tools for the items `eda_model::fp_items` adds (UI-LEFT F2; kicad-port/fpDrawTools.ts, components/footprint/fpItemTools.ts): each tool of the right
// toolbar is armed by its action, draws on a new footprint with real pointer events on the canvas, and commits one `fp_item add` (`GET /api/footprint`'s `items` has the item);
// a double click on the item opens its Properties dialog (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`), OK edits it with one `fp_item set`, and one undo takes the edit back:
//   - Draw Aligned / Orthogonal / Center / Radial Dimension and Leader (`DRAWING_TOOL::DrawDimension`: origin, end, height; a leader's text asked for at once), Dimension Properties;
//   - Draw Rule Area (`DrawZone`, the outline, then Rule Area Properties: no tracks, vias or pads by default), the rule area editor;
//   - Draw Text Box (`DrawRectangle( drawTextBox )`, then Text Box Properties), Place Point (`PlacePoint`), Draw Table (`DrawTable`, then Table Properties),
//     Add Barcode (`DrawBarcode`, then Barcode Properties) and Place Reference Image (`PlaceReferenceImage`, an image given to the action), each with its dialog.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes footprints of its own (New Footprint) and deletes them at the
// end, leaving the project library as it found it. It answers through `window.__fpeditItemsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__fpeditItemsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const setValue = async (el, value) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(80);
  };
  const until = async (fn, ms = 5000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(100); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const itemsOf = async (name, kind) => ((await footprint(name)).items ?? {})[kind] ?? [];
  const button = (root, label) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
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
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    await key("Escape");
    await key("Escape");
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** `mods`: Ctrl+Shift by default -- neither the grid nor an anchor moves the point (a click lands where it is sent), and Ctrl frees the line mode. */
  const fire = (type, wx, wy, buttons = 0, mods = { ctrl: true, shift: true }) => {
    const [x, y] = toScreen(wx, wy);
    const mac = /Mac/.test(navigator.platform);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !mac && !!mods.ctrl, metaKey: mac && !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const clickAt = async (wx, wy, mods) => { fire("pointermove", wx, wy, 0, mods); await sleep(80); fire("pointerdown", wx, wy, 1, mods); await sleep(40); fire("pointerup", wx, wy, 0, mods); await sleep(450); };
  /** A plain click of the select tool, then the double click (`PCB_SELECTION_TOOL`'s double click -> `EDIT_TOOL::Properties`). */
  const dblClickAt = async (wx, wy) => {
    const [x, y] = toScreen(wx, wy);
    await clickAt(wx, wy, {});
    canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, detail: 2 }));
    await sleep(500);
  };
  const key = async (k, opts = {}) => { canvasEl()?.focus(); canvasEl()?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...opts })); await sleep(250); };
  const arm = async (action, tool, args) => { const r = await __eda.run(action, args); await sleep(300); return { r, armed: __eda.state().tool === tool }; };
  const selection = () => (__eda.state().selection ?? []).map((s) => s.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(800); };
  const made = [];
  const newFootprint = async () => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(900);
    const name = (await fpNames()).find((n) => !before.includes(n));
    made.push(name);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(400);
    return name;
  };
  const MM = 1_000_000;
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;

  // --------------------------------------------------------------------------------------------------------------------------- run
  const libraryBefore = await fpNames();
  const name = await newFootprint();

  await scenario("Draw Aligned Dimension: origin, end, height -- one fp_item add, selected, the tool stays; Dimension Properties edits it, one undo step", async (t) => {
    t(!!name, `a new footprint: ${name}`);
    const a = await arm("pcbnew.InteractiveDrawing.alignedDimension", "dim_aligned");
    t(a.armed, `the tool is armed from its action: ${JSON.stringify(a.r)} ${__eda.state().tool}`);
    await clickAt(-4 * MM, 0);
    await clickAt(4 * MM, 0);
    t((await itemsOf(name, "dimensions")).length === 0, "nothing is added before the height");
    await clickAt(0, -3 * MM);
    const dims = await until(async () => { const d = await itemsOf(name, "dimensions"); return d.length > 0 ? d : null; });
    const d = dims?.[0];
    t(d && d.kind?.kind === "aligned" && d.start.x === -4 * MM && d.end.x === 4 * MM && d.kind.height === -3 * MM, `an aligned dimension, height -3 mm: ${JSON.stringify(d && [d.kind, d.start, d.end])}`);
    t(d && d.layer === "F.SilkS" && d.text_size_nm === MM && d.stroke_width === 100_000 && d.arrow_length === 1_270_000 && d.extension_offset === 500_000 && d.units === "automatic" && d.precision === 4, `the footprint editor's style: ${JSON.stringify(d && [d.layer, d.text_size_nm, d.stroke_width, d.arrow_length, d.extension_offset, d.units, d.precision])}`);
    t(d && selection().join() === d.id, `the new dimension is selected: ${selection()}`);
    t(__eda.state().tool === "dim_aligned", `the tool stays armed: ${__eda.state().tool}`);
    await key("Escape");
    t(__eda.state().tool === "select", `Escape leaves the tool: ${__eda.state().tool}`);
    // the crossbar at y = -3 mm
    await dblClickAt(0, -3 * MM);
    const dlg = await until(() => tid("dimension-properties"));
    if (t(!!dlg, "a double click on the dimension opens Dimension Properties")) {
      t(/Aligned Dimension/.test(dlg.textContent), "its title names the kind");
      await setValue(tid("dim-prefix"), "W=");
      tid("dim-ok").click();
      await sleep(900);
      const e = (await itemsOf(name, "dimensions"))[0];
      t(e && e.prefix === "W=" && e.id === d.id && e.kind.height === -3 * MM, `OK is one fp_item set: ${JSON.stringify(e && [e.prefix, e.kind])}`);
      await undo();
      const u = await itemsOf(name, "dimensions");
      t(u.length === 1 && u[0].prefix === "", `one undo takes the edit back: ${JSON.stringify(u.map((x) => x.prefix))}`);
      await undo();
      t((await itemsOf(name, "dimensions")).length === 0, "a second undo takes the dimension away (its add was one step)");
      await redo();
      t((await itemsOf(name, "dimensions")).length === 1, "redo brings it back");
    }
  });

  await scenario("Draw Orthogonal / Center / Radial Dimension and Leader: each its kind; a leader opens Dimension Properties for its text", async (t) => {
    const before = (await itemsOf(name, "dimensions")).length;
    await arm("pcbnew.InteractiveDrawing.orthogonalDimension", "dim_orthogonal");
    await clickAt(-2 * MM, 2 * MM);
    await clickAt(2 * MM, 3 * MM);
    await clickAt(0, 6 * MM);
    let dims = await itemsOf(name, "dimensions");
    const o = dims[before];
    t(o && o.kind?.kind === "orthogonal" && o.kind.horizontal === true && o.kind.height === 4 * MM, `an orthogonal dimension, horizontal, height 4 mm: ${JSON.stringify(o?.kind)}`);
    await key("Escape");
    await arm("pcbnew.InteractiveDrawing.centerDimension", "dim_center");
    await clickAt(6 * MM, 6 * MM);
    await clickAt(7 * MM, 6 * MM);
    dims = await itemsOf(name, "dimensions");
    const c = dims[before + 1];
    t(c && c.kind?.kind === "center" && c.end.x === 7 * MM, `a centre mark at its second click: ${JSON.stringify(c && [c.kind, c.start, c.end])}`);
    await key("Escape");
    await arm("pcbnew.InteractiveDrawing.radialDimension", "dim_radial");
    await clickAt(-6 * MM, -6 * MM);
    await clickAt(-5 * MM, -6 * MM);
    dims = await itemsOf(name, "dimensions");
    const r = dims[before + 2];
    t(r && r.kind?.kind === "radial" && r.prefix === "R " && r.kind.leader_length === 3_810_000, `a radial dimension, "R ", 3 arrow lengths of leader: ${JSON.stringify(r && [r.kind, r.prefix])}`);
    await key("Escape");
    await arm("pcbnew.InteractiveDrawing.leader", "dim_leader");
    await clickAt(6 * MM, -6 * MM);
    await clickAt(8 * MM, -8 * MM);
    const dlg = await until(() => tid("dimension-properties"));
    dims = await itemsOf(name, "dimensions");
    const l = dims[before + 3];
    t(l && l.kind?.kind === "leader" && l.override_text === "", `a leader, its text still empty: ${JSON.stringify(l && [l.kind, l.override_text])}`);
    if (t(!!dlg && /Leader Dimension/.test(dlg.textContent), "the leader's Dimension Properties opens at once")) {
      await setValue(tid("dim-override"), "PIN 1");
      tid("dim-ok").click();
      await sleep(900);
      const e = (await itemsOf(name, "dimensions"))[before + 3];
      t(e && e.override_text === "PIN 1", `the leader's text: ${e?.override_text}`);
    }
  });

  await scenario("Draw Rule Area: the outline, then Rule Area Properties (no tracks, vias, pads); its double click opens the rule area editor, one undo step", async (t) => {
    const a = await arm("pcbnew.InteractiveDrawing.ruleArea", "rule_area");
    t(a.armed, `armed: ${__eda.state().tool}`);
    await clickAt(-10 * MM, 10 * MM);
    await clickAt(-4 * MM, 10 * MM);
    await clickAt(-4 * MM, 14 * MM);
    await clickAt(-10 * MM, 14 * MM);
    await key("Enter");
    const dlg = await until(() => tid("zone-dialog"));
    if (!t(!!dlg && dlg.getAttribute("data-zone-dialog") === "rule-area", `Rule Area Properties opens on the outline: ${dlg?.getAttribute("data-zone-dialog")}`)) return;
    t(/Rule Area Properties/.test(dlg.textContent) && !tid("zone-locked"), "titled so, with no Locked box (a footprint's item keeps none)");
    t((await itemsOf(name, "zones")).length === 0, "nothing is added before OK");
    tid("zone-ok").click();
    const zones = await until(async () => { const z = await itemsOf(name, "zones"); return z.length > 0 ? z : null; });
    const z = zones?.[0];
    t(z && z.is_rule_area && z.keepout_tracks && z.keepout_vias && z.keepout_pads && !z.keepout_copper_pour && !z.keepout_footprints && z.net === "" && z.outline.length >= 4, `the rule area: ${JSON.stringify(z && [z.is_rule_area, z.keepout_tracks, z.keepout_vias, z.keepout_pads, z.keepout_copper_pour, z.keepout_footprints, z.net, z.layer, z.outline.length])}`);
    t(z && (await until(() => selection().join() === z.id)), `selected: ${selection()}`);
    await key("Escape");
    await key("Escape");
    await dblClickAt(-7 * MM, 12 * MM);
    const ed = await until(() => tid("zone-dialog"));
    if (t(!!ed && /Rule Area Properties/.test(ed.textContent) && button(ed, "OK"), "a double click opens Rule Area Properties on it")) {
      const fp = [...ed.querySelectorAll("label")].find((l) => l.textContent.trim() === "Footprints")?.querySelector("input");
      fp?.click();
      await sleep(150);
      tid("zone-ok").click();
      await sleep(900);
      const e = (await itemsOf(name, "zones"))[0];
      t(e && e.keepout_footprints === true && e.id === z.id, `OK keeps footprints out: ${e?.keepout_footprints}`);
      await undo();
      t((await itemsOf(name, "zones"))[0]?.keepout_footprints === false, "one undo takes it back");
    }
  });

  await scenario("Draw Text Box: two corners, then Text Box Properties; its double click edits it, one undo step", async (t) => {
    await arm("pcbnew.InteractiveDrawing.textbox", "textbox");
    // Shift only: the grid holds the corners (they are on it) and Ctrl, which would make the box a square (`drawShape`'s modifier), is up
    await clickAt(2 * MM, 10 * MM, { shift: true });
    await clickAt(10 * MM, 13 * MM, { shift: true });
    const dlg = await until(() => tid("pcb-text-box-properties"));
    if (!t(!!dlg && !tid("ptb-locked"), "Text Box Properties opens on the box, with no Locked row")) return;
    await setValue(tid("ptb-text"), "HELLO");
    tid("ptb-ok").click();
    const boxes = await until(async () => { const b = await itemsOf(name, "text_boxes"); return b.length > 0 ? b : null; });
    const b = boxes?.[0];
    t(b && b.text === "HELLO" && b.layer === "F.SilkS" && b.start.x === 2 * MM && b.end.y >= 13 * MM, `the text box: ${JSON.stringify(b && [b.text, b.layer, b.start, b.end])}`);
    t(b && (await until(() => selection().join() === b.id)), `selected: ${selection()}`);
    t(__eda.state().tool === "textbox", "the tool waits for the next box");
    await key("Escape");
    await dblClickAt(2 * MM, 11.5 * MM);
    if (t(!!(await until(() => tid("pcb-text-box-properties"))), "a double click on its border opens Text Box Properties")) {
      await setValue(tid("ptb-text"), "WORLD");
      tid("ptb-ok").click();
      await sleep(900);
      t((await itemsOf(name, "text_boxes"))[0]?.text === "WORLD", "OK edits it");
      await undo();
      t((await itemsOf(name, "text_boxes"))[0]?.text === "HELLO", "one undo takes the edit back");
    }
  });

  await scenario("Place Point: a point at every click, one step each; a point has no dialog", async (t) => {
    await arm("pcbnew.InteractiveDrawing.placePoint", "point");
    await clickAt(12 * MM, -2 * MM);
    await clickAt(14 * MM, -2 * MM);
    const pts = await itemsOf(name, "points");
    t(pts.length === 2 && pts[0].at.x === 12 * MM && pts[0].size_nm === MM && pts[0].layer === "F.SilkS", `two points: ${JSON.stringify(pts.map((p) => [p.at, p.size_nm, p.layer]))}`);
    t(selection().length === 1 && selection()[0] === pts[1]?.id, `the last one is selected: ${selection()}`);
    await key("Escape");
    await undo();
    t((await itemsOf(name, "points")).length === 1, "one undo takes the last point away");
    await dblClickAt(12 * MM, -2 * MM);
    t(document.querySelectorAll(".dialog-backdrop").length === 0, "a double click on a point opens nothing (`case PCB_POINT_T: break;`)");
  });

  await scenario("Draw Table: the first corner, the grid sized by the cursor, then Table Properties; its double click edits it, one undo step", async (t) => {
    await arm("pcbnew.InteractiveDrawing.drawTable", "table");
    await clickAt(-14 * MM, -14 * MM);
    await clickAt(2 * MM, -8 * MM);
    const dlg = await until(() => tid("table-properties"));
    if (!t(!!dlg && !tid("table-locked"), "Table Properties opens on the drawn table, with no Locked box")) return;
    await setValue(tid("table-cell-0-0"), "A1");
    tid("table-ok").click();
    const tables = await until(async () => { const x = await itemsOf(name, "tables"); return x.length > 0 ? x : null; });
    const tb = tables?.[0];
    t(tb && tb.layer === "F.SilkS" && tb.at.x === -14 * MM && tb.cells[0].text === "A1" && tb.cells.length === tb.col_widths.length * tb.row_heights.length, `the table: ${JSON.stringify(tb && [tb.layer, tb.at, tb.col_widths.length, tb.row_heights.length, tb.cells[0].text])}`);
    t(tb && (await until(() => selection().join() === tb.id)), `selected: ${selection()}`);
    await key("Escape");
    await dblClickAt(-13 * MM, -13 * MM);
    if (t(!!(await until(() => tid("table-properties"))), "a double click opens Table Properties on it")) {
      await setValue(tid("table-cell-0-0"), "B2");
      tid("table-ok").click();
      await sleep(900);
      t((await itemsOf(name, "tables"))[0]?.cells[0].text === "B2", "OK edits the cell");
      await undo();
      t((await itemsOf(name, "tables"))[0]?.cells[0].text === "A1", "one undo takes the edit back");
    }
  });

  await scenario("Add Barcode: a click opens Barcode Properties on a QR code of the layer; its double click edits it, one undo step", async (t) => {
    await arm("pcbnew.InteractiveDrawing.barcode", "barcode");
    await clickAt(30 * MM, 30 * MM);
    const dlg = await until(() => tid("pcb-barcode-properties"));
    if (!t(!!dlg && !tid("pbc-locked"), "Barcode Properties opens, with no Locked row")) return;
    await setValue(tid("pbc-text"), "KICAD");
    t(!!(await until(() => tid("pbc-preview-svg"), 60000)), "the dialog's preview draws the symbol (kicad-cli's plot)");
    tid("pbc-ok").click();
    // OK waits for kicad-cli's plot of the symbol (`refreshPreview`: a text that makes none is refused)
    const codes = await until(async () => { const x = await itemsOf(name, "barcodes"); return x.length > 0 ? x : null; }, 60000);
    const bc = codes?.[0];
    t(bc && bc.text === "KICAD" && bc.at.x === 30 * MM && bc.layer === "F.SilkS", `the barcode: ${JSON.stringify(bc && [bc.text, bc.at, bc.layer, bc.width_nm])} ${tid("pbc-error")?.textContent ?? ""} ${tid("pbc-preview")?.textContent ?? ""}`);
    t(bc && (await until(() => selection().join() === bc.id)), `selected: ${selection()}`);
    await key("Escape");
    await dblClickAt(30 * MM, 30 * MM);
    if (t(!!(await until(() => tid("pcb-barcode-properties"))), "a double click opens Barcode Properties on it")) {
      await setValue(tid("pbc-width"), "30");
      await sleep(400);
      await until(() => tid("pbc-preview-svg"), 60000);
      tid("pbc-ok").click();
      await until(async () => (await itemsOf(name, "barcodes"))[0]?.width_nm === 30 * MM, 60000);
      t((await itemsOf(name, "barcodes"))[0]?.width_nm === 30 * MM, "OK edits its width");
      await undo();
      t((await itemsOf(name, "barcodes"))[0]?.width_nm === 40 * MM, "one undo takes the edit back");
    }
  });

  await scenario("Place Reference Image: the image held at the cursor, a click puts it down; its double click opens Reference Image Properties, one undo step", async (t) => {
    // a 40 x 40 pixel PNG
    const cv = document.createElement("canvas");
    cv.width = 40;
    cv.height = 40;
    const g = cv.getContext("2d");
    g.fillStyle = "#c33";
    g.fillRect(0, 0, 40, 40);
    const data = cv.toDataURL("image/png").split(",")[1];
    const a = await arm("pcbnew.InteractiveDrawing.placeReferenceImage", "ref_image", { name: "red.png", data });
    t(a.armed, `armed with the image: ${__eda.state().tool}`);
    await sleep(500);
    await clickAt(-20 * MM, 20 * MM);
    const imgs = await until(async () => { const x = await itemsOf(name, "images"); return x.length > 0 ? x : null; });
    const im = imgs?.[0];
    t(im && im.at.x === -20 * MM && im.at.y === 20 * MM && (im.scale_ppm ?? 1_000_000) === 1_000_000 && im.layer === "F.SilkS", `the image: ${JSON.stringify(im && [im.at, im.scale_ppm, im.layer])}`);
    t(im && (await until(() => selection().join() === im.id)), `selected: ${selection()}`);
    await key("Escape");
    // 40 px at 300 ppi: 3.39 mm square about its centre -- its box's edge
    const shown = (await footprint(name)).display?.images?.[0];
    await dblClickAt(-20 * MM - shown.w / 2, 20 * MM);
    if (t(!!(await until(() => tid("pcb-ref-image-properties"))) && !tid("pri-locked"), "a double click on its box opens Reference Image Properties, with no Locked row")) {
      await setValue(tid("pri-x"), "-18");
      tid("pri-ok").click();
      await sleep(900);
      t((await itemsOf(name, "images"))[0]?.at.x === -18 * MM, "OK moves it");
      await undo();
      t((await itemsOf(name, "images"))[0]?.at.x === -20 * MM, "one undo takes the edit back");
    }
  });

  // ================================================================================================ leave the library as it was
  await closeDialogs();
  for (const n of made) if (n) await cmd({ op: "delete_library_footprint", name: n });
  await sleep(500);
  const libraryAfter = await fpNames();
  const clean = JSON.stringify(libraryAfter) === JSON.stringify(libraryBefore);
  results.push({ name: "the project library is left as it was found", ok: clean, notes: [clean ? "ok: same names" : `FAIL: ${JSON.stringify({ before: libraryBefore, after: libraryAfter })}`] });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
