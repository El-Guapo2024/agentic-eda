// A library footprint on the board whole, UI-LEFT PP5, PP14 and FF4 (PARITY-pcb.md section 95): a footprint placed from a library of the tables, or a part changed to it,
// keeps the library's pads' layer sets, die lengths and fabrication properties, its description and its dimension, table and keepout (`PCB_BASE_FRAME::LoadFootprint`'s
// footprint added whole, `ExchangeFootprint` keeping `aNew`'s children), so Compare with Library finds nothing; the dimension and the table are drawn with the footprint and a click
// on one picks the footprint; R and F turn and flip the dimension, the table and the keepout with it (`FOOTPRINT::SetOrientation`, `Flip`); Edit Footprint's copy (Ctrl+E) shows
// them, and a dimension, a keepout and a table drawn there and a description typed there reach the board with Update Footprint on Board. Driven through `window.__eda` (the actions
// a person runs, the canvas's pointer events, the exchange dialog) and checked against what the server holds (`/api/state`, `/api/footprint`, `/api/exchange_plan`).
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board with R1 placed (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, `eda board place R1 --at 8,8`), served with `KICAD_PATH` set. It makes a project footprint library of its own (`KeptLib.pretty`)
// in the board's folder and removes nothing: run it on a copy. It answers through `window.__fpRecordCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30-few minutes=8
(async () => {
  const results = [];
  const check = (window.__fpRecordCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const MM = 1_000_000;
  const cmd = (c) => post("/api/cmd", { strict: false, cmd: c });
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(150); } return null; };
  /** A command sent behind the studio's back is in its state after its next refresh: wait for the revision to move. */
  const send = async (c) => { const rev0 = E.state().revision; const r = await cmd(c); await until(() => E.state().revision !== rev0, 8000); await sleep(400); return r; };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = async (el, v) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(80); };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button, .dialog button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  // the board's canvas: a world point in millimetres to the screen, from the cursor readout
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(800); };

  const state = () => json("/api/state");
  const part = async (ref) => ((await state()).parts ?? []).find((p) => p.ref === ref);
  const zonesOf = async (ref) => ((await state()).routing?.zones ?? []).filter((z) => z.parent === ref);
  /** Compare with Library's lines for `ref` (`/api/exchange_plan` of an update). */
  const differences = async (ref) => (await post("/api/exchange_plan", { references: [ref], footprint: null })).entries?.[0]?.lines ?? ["(no answer)"];
  /** What the board holds of `ref` beyond its pads: its dimensions, tables and zones. */
  const kept = async (ref) => {
    const p = await part(ref);
    return { dims: p?.items?.dimensions ?? [], tables: p?.items?.tables ?? [], zones: await zonesOf(ref), descr: p?.lib_description };
  };

  // ------------------------------------------------------------------------------------------------------------------------ the library
  const LIB = "KeptLib";
  const NAME = `${LIB}:Kept`;
  const KEPT = `(footprint "Kept" (version 20241229) (generator "pcbnew") (layer "F.Cu")
  (descr "a kept footprint") (tags "kept test")
  (property "Reference" "REF**" (at 0 -3 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
  (property "Value" "Kept" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
  (attr smd)
  (fp_rect (start -2.5 -2.5) (end 2.5 2.5) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd"))
  (dimension (type aligned) (layer "F.Fab") (pts (xy -1 2) (xy 1 2)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned))
  (table (column_count 1) (layer "F.Fab") (border (external yes) (header yes) (stroke (width 0.1) (type solid))) (separators (rows yes) (cols yes) (stroke (width 0.1) (type solid))) (column_widths 3) (row_heights 1.5)
    (cells (table_cell "T" (start -1.5 -2) (end 1.5 -0.5) (margins 0.2 0.2 0.2 0.2) (span 1 1) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)) (justify left top)))))
  (pad "1" smd roundrect (at -1 0) (size 1 1) (layers "F.Cu" "F.Mask") (roundrect_rratio 0.25) (property pad_prop_heatsink) (die_length 0.5))
  (pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  (zone (net 0) (net_name "") (layer "F.Cu") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour not_allowed) (footprints allowed)) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5)) (polygon (pts (xy -3 3) (xy 3 3) (xy 3 4) (xy -3 4))))
)`;

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await clickTab("PCB");
  const outline = (await state()).outline ?? [[0, 0], [35 * MM, 0], [35 * MM, 35 * MM], [0, 35 * MM]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])), Math.min(...outline.map((p) => p[1]))];
  const home = [bx0 + 20 * MM, by0 + 25 * MM];

  // ------------------------------------------------------------------------------------------------------------------------ the scenarios
  let ref = null;
  await scenario("Place Footprint from a library of the tables: the pads' layers, die length and property, the description, the dimension, the table and the keepout are the library's", async (t) => {
    const add = await post("/api/library_tables/add", { kind: "footprint", path: `${LIB}.pretty`, create: true });
    if (!t(!add.error, `New Library ${LIB}: ${add.error ?? "made"}`)) return;
    const parsed = await post("/api/footprint/parse", KEPT);
    if (!t(parsed.footprint, `the footprint file reads: ${parsed.error ?? ""}`)) return;
    const put = await cmd({ op: "put_library_footprint", footprint: { ...parsed.footprint, name: NAME }, overwrite: true });
    t(put.ok !== false, "the footprint is in the Footprint Editor's buffer of the library");
    const saved = await post("/api/library/save", { kind: "footprint", id: NAME });
    if (!t(saved.ok, `saved into the library's folder: ${saved.message ?? saved.error}`)) return;
    const before = new Set((await state()).parts.map((p) => p.ref));
    // the Place Footprint tool's drop (`place_footprint` of the chooser's name; the server brings the library's footprint)
    const r = await send({ op: "place_footprint", footprint: NAME, at: { x: home[0], y: home[1] } });
    t(r.ok !== false, `placed: ${r.message ?? ""}`);
    ref = (await state()).parts.find((p) => !before.has(p.ref) && p.placed)?.ref ?? null;
    if (!t(ref, `a new footprint is on the board: ${ref}`)) return;
    const k = await kept(ref);
    t(k.dims.length === 1 && k.tables.length === 1, `its dimension and table are drawn with it: ${k.dims.length}, ${k.tables.length}`);
    t(k.zones.length === 1 && k.zones[0].is_rule_area, `its keepout is a zone of the footprint: ${k.zones.length}`);
    t(near(k.dims[0].start[0], home[0] - MM) && near(k.dims[0].start[1], home[1] + 2 * MM), `the dimension is where the footprint puts it: ${k.dims[0]?.start}`);
    t(k.descr === "a kept footprint", `the library's description: ${k.descr}`);
    const lines = await differences(ref);
    t(lines.length === 0, `Compare with Library finds nothing (a no-paste pad, a die length, a property, the description): ${JSON.stringify(lines)}`);
    // a click on the dimension's crossbar picks the footprint (`FilterCollectorForHierarchy`), not the keepout (a footprint's child is picked in the Footprint Editor only)
    const toScreen = await screenOf();
    if (!t(toScreen, "the canvas gave a cursor readout")) return;
    await E.run("common.InteractiveSelection.clear");
    const [ax, ay] = k.dims[0].text_at;
    await click(toScreen, ax / MM, ay / MM);
    t(E.state().selection.some((s) => s.id === ref), `a click on the dimension picks ${ref}: ${JSON.stringify(E.state().selection)}`);
    t(!E.state().selection.some((s) => s.id === k.zones[0].id), "and not its keepout");
  });

  await scenario("R and F turn and flip the footprint's dimension, table and keepout with it; one undo each", async (t) => {
    if (!t(ref, "the placed footprint")) return;
    const k0 = await kept(ref);
    await select([ref]);
    await E.run("pcbnew.InteractiveEdit.rotateCw");
    await sleep(900);
    const k1 = await kept(ref);
    const p = await part(ref);
    // (the state's orientation is in degrees, clockwise on the screen as the board's verbs turn)
    t(p.rot === 90, `the footprint is turned a quarter: ${p.rot}`);
    t(!near(k1.dims[0].start[0], k0.dims[0].start[0]) || !near(k1.dims[0].start[1], k0.dims[0].start[1]), `the dimension turned with it: ${k0.dims[0].start} -> ${k1.dims[0].start}`);
    t(k1.tables[0].angle !== k0.tables[0].angle, `the table turned: ${k0.tables[0].angle} -> ${k1.tables[0].angle}`);
    t(JSON.stringify(k1.zones[0].outline) !== JSON.stringify(k0.zones[0].outline), "the keepout turned");
    // where the turn puts the keepout's first corner: about the footprint's position, a quarter clockwise on the screen
    const [cx, cy] = [p.at[0], p.at[1]];
    const [zx, zy] = k0.zones[0].outline[0];
    const turned = [cx - (zy - cy), cy + (zx - cx)];
    t(near(k1.zones[0].outline[0][0], turned[0]) && near(k1.zones[0].outline[0][1], turned[1]), `the keepout's corner is where the turn takes it: ${k1.zones[0].outline[0]} vs ${turned}`);
    await select([ref]);
    await E.run("pcbnew.InteractiveEdit.flip");
    await sleep(900);
    const k2 = await kept(ref);
    t((await part(ref)).side === "bottom", "the footprint is on the back");
    t(k2.zones[0].layers?.[0] === "B.Cu" || k2.zones[0].layer === "B.Cu", `the keepout is on the back's copper: ${k2.zones[0].layer}`);
    t(k2.dims[0].layer === "B.Fab" && k2.tables[0].layer === "B.Fab", `the dimension and the table are on the back's fab layer: ${k2.dims[0].layer} ${k2.tables[0].layer}`);
    t((await differences(ref)).length === 0, "flipped, still the library's");
    await undo();
    await undo();
    const back = await kept(ref);
    t(JSON.stringify(back.zones[0].outline) === JSON.stringify(k0.zones[0].outline) && near(back.dims[0].start[0], k0.dims[0].start[0]), "two undos put everything back");
  });

  /** Change Footprint on R1 through the dialog. */
  const exchange = async (to) => {
    await clickTab("PCB");
    await select(["R1"]);
    await E.run("pcbnew.GlobalEdit.changeFootprint");
    const dlg = await until(() => tid("exchange-dialog"), 4000);
    if (!dlg) return null;
    await setValue(tid("exchange-new-id"), to);
    tid("exchange-run").click();
    await until(() => /OK|no changes/.test(tid("exchange-messages")?.textContent ?? ""), 6000);
    const msg = tid("exchange-messages")?.textContent ?? "";
    await closeDialogs();
    await sleep(600);
    return msg;
  };

  await scenario("Change Footprint R1 to the library's: R1 has its dimension, table and keepout, and nothing differs from the library; one undo", async (t) => {
    const r1 = await part("R1");
    if (!t(r1?.placed, `R1 is placed: ${r1?.footprint}`)) return;
    const before = await kept("R1");
    t(before.dims.length === 0 && before.zones.length === 0, "R1 has none to begin with");
    const msg = await exchange(NAME);
    if (!t(msg !== null, "the Change Footprint dialog opens on R1")) return;
    t(msg.includes(`to '${NAME}': OK`), `the message window: ${msg.slice(0, 160)}`);
    const k = await kept("R1");
    t(k.dims.length === 1 && k.tables.length === 1 && k.zones.length === 1, `R1 has the library's dimension, table and keepout: ${k.dims.length} ${k.tables.length} ${k.zones.length}`);
    const lines = await differences("R1");
    t(lines.length === 0, `Compare with Library finds nothing: ${JSON.stringify(lines)}`);
    await undo();
    const back = await kept("R1");
    t(back.dims.length === 0 && back.zones.length === 0 && (await part("R1")).footprint === r1.footprint, `one undo: R1 is on ${r1.footprint} again with none of them`);
  });

  await scenario("Ctrl+E shows the dimension, the table and the keepout; a dimension, a keepout and a table drawn in the copy and a description typed there reach the board", async (t) => {
    if (!t(ref, "the placed footprint")) return;
    await clickTab("PCB");
    await select([ref]);
    await E.run("pcbnew.EditorControl.EditFpInFpEditor");
    const key = `<board>/${ref}`;
    const copy = await until(async () => { const f = await json(`/api/footprint?name=${encodeURIComponent(key)}`); return f.board_link?.reference === ref ? f : null; }, 8000);
    if (!t(copy, "the Footprint Editor holds the copy")) return;
    const it = copy.items ?? {};
    t((it.dimensions ?? []).length === 1 && (it.tables ?? []).length === 1 && (it.zones ?? []).length === 1, `the copy shows the dimension, the table and the keepout: ${JSON.stringify(Object.keys(it))}`);
    t(copy.description === "a kept footprint" && copy.keywords === "kept test", `and the board's description and keywords: ${copy.description} / ${copy.keywords}`);
    // the editor's tools add items with `fp_item add` (Draw Dimension, Add Keepout Area, Draw Table) and Footprint Properties sends `edit_footprint_properties`
    const dim = { ...it.dimensions[0], id: "", start: { x: -MM, y: -2.5 * MM }, end: { x: MM, y: -2.5 * MM } };
    const zone = { ...it.zones[0], id: "", outline: it.zones[0].outline.map((p) => ({ x: p.x, y: -p.y })) };
    const table = { ...it.tables[0], id: "", at: { x: 3 * MM, y: -2 * MM } };
    for (const [kind, body] of [["dimension", dim], ["zone", zone], ["table", table]]) {
      const r = await cmd({ op: "fp_item", verb: "add", footprint: key, item: { [kind]: body } });
      t(r.ok !== false, `a ${kind} drawn in the copy: ${r.message ?? ""}`);
    }
    const now = await json(`/api/footprint?name=${encodeURIComponent(key)}`);
    const props = await cmd({ op: "edit_footprint_properties", name: key, description: "edited in the copy", keywords: now.keywords, attributes: now.attributes, reference_visible: now.reference_visible, value_visible: now.value_visible, model: now.model ?? null, zone_connection: now.zone_connection ?? null });
    t(props.ok !== false, `the description is typed in Footprint Properties: ${props.message ?? ""}`);
    await sleep(600);
    await E.run("common.Control.save");
    const after = await until(async () => { const k = await kept(ref); return k.dims.length === 2 ? k : null; }, 8000);
    t(after, `Update Footprint on Board: two dimensions on the board: ${JSON.stringify((await kept(ref)).dims.length)}`);
    t(after?.tables.length === 2 && after?.zones.length === 2, `two tables and two keepouts: ${after?.tables.length} ${after?.zones.length}`);
    t(after?.descr === "edited in the copy", `the description typed in the copy: ${after?.descr}`);
    await clickTab("PCB");
    await undo();
    const back = await kept(ref);
    t(back.dims.length === 1 && back.zones.length === 1 && back.descr === "a kept footprint", "one undo of the board takes the update back");
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
