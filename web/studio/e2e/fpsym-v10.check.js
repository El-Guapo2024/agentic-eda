// UI-LEFT-2 FF6 and YY3 (task-v10-fpsym; PARITY-fpedit section 27, PARITY-symedit "Round 2, task-v10-fpsym"):
//   - the Footprint Editor draws a footprint's reference image with its pixels (`GET /api/pcb_image?fp=..`), its barcode with the symbol kicad-cli plots
//     (`GET /api/barcode_shape?fp=..`) and a zone with the fill it brought (`display.zone_fills`); a locked item and a locked group keep their locks, and a
//     pre-6.0 dimension is read as KiCad reads it (an aligned one) -- none of them is kept as the file's text any more;
//   - a symbol of three custom body styles is written at 20251024 with its declaration, placed on the schematic in its third style (the schematic's
//     `lib_symbols` has every body and the names), Symbol Properties lists the custom names, Cycle Body Style goes round the three, and the Symbol Editor's
//     Symbol Properties has its Embedded Files.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It answers through `window.__fpsymV10Check`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__fpsymV10Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: typeof body === "string" ? body : JSON.stringify(body) });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };
  const E = window.__eda;
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(120); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(800); };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const canvas = () => canvasEl()?.querySelector("canvas");
  const canvasEl = () => [...document.querySelectorAll(".editor-canvas-col .pcb-canvas-container")].find((e) => e.getBoundingClientRect().width > 0);
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** The canvas's colour at the editor's point (nm). */
  const pixel = (wx, wy) => {
    const c = canvas(); const ctx = c.getContext("2d"); const rect = c.getBoundingClientRect(); const dpr = c.width / rect.width;
    const [x, y] = toScreen(wx, wy);
    const d = ctx.getImageData(Math.round((x - rect.left) * dpr), Math.round((y - rect.top) * dpr), 1, 1).data;
    return [d[0], d[1], d[2]];
  };
  /** How many distinct colours a box of the canvas (nm) holds: a drawn symbol has many, an empty box one. */
  const colours = (x0, y0, x1, y1) => {
    const seen = new Set();
    for (let i = 0; i <= 12; i++) for (let j = 0; j <= 12; j++) seen.add(pixel(x0 + ((x1 - x0) * i) / 12, y0 + ((y1 - y0) * j) / 12).join(","));
    return seen.size;
  };

  // a 40 x 40 PNG, pure red: the image's pixels are told by their colour
  const png = (() => { const c = document.createElement("canvas"); c.width = 40; c.height = 40; const x = c.getContext("2d"); x.fillStyle = "#ff0000"; x.fillRect(0, 0, 40, 40); return c.toDataURL("image/png").split(",")[1]; })();
  const s = Date.now().toString(36);
  let fpName = null;

  // ------------------------------------------------------------------------------------------------------------------------ FF6
  await scenario("FF6: a footprint's image, barcode, filled zone, locks and pre-6.0 dimension are items, drawn as the board draws them", async (t) => {
    await clickTab("Footprint");
    const before = await fpNames();
    await run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    fpName = (await fpNames()).find((n) => !before.includes(n));
    if (!t(fpName, `a new footprint: ${fpName}`)) return;
    // the courtyard spans the items, so Zoom to Fit shows them all
    const body = `(footprint "X" (version 20240108) (generator "pcbnew") (layer "F.Cu")
      (fp_rect (start -15 -17) (end 15 15) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
      (image (at 10 0) (layer "F.SilkS") (scale 1) (data "${png}") (uuid "a1000000-0000-4000-8000-000000000001"))
      (barcode (at -10 0 0) (layer "F.SilkS") (size 5 5) (text "fpsym") (text_height 1) (type qr) (ecc_level M) (hide yes) (knockout no) (uuid "a1000000-0000-4000-8000-000000000002"))
      (zone (layer "F.Cu") (uuid "a1000000-0000-4000-8000-000000000003") (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25) (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))
        (polygon (pts (xy -3 6) (xy 3 6) (xy 3 12) (xy -3 12))) (filled_polygon (layer "F.Cu") (pts (xy -3 6) (xy 3 6) (xy 3 12) (xy -3 12))))
      (dimension (type aligned) (locked yes) (layer "F.Fab") (uuid "a1000000-0000-4000-8000-000000000004") (pts (xy -5 -8) (xy 5 -8)) (height 1) (format (prefix "") (suffix "") (units 2) (units_format 1) (precision 2)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5)) (gr_text "10.00 mm" (at 0 -9 0) (layer "F.Fab") (uuid "a1000000-0000-4000-8000-000000000005") (effects (font (size 1 1) (thickness 0.15)))))
      (dimension 12.7 (width 0.15) (layer "F.Fab") (gr_text "12,70 mm" (at 0 -14) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)))) (feature1 (pts (xy 6.35 -12) (xy 6.35 -14))) (feature2 (pts (xy -6.35 -12) (xy -6.35 -14))) (crossbar (pts (xy -6.35 -13) (xy 6.35 -13))))
      (group "locked" (locked yes) (uuid "a1000000-0000-4000-8000-000000000006") (members "a1000000-0000-4000-8000-000000000001"))
    )`;
    const parsed = (await post("/api/footprint/parse", body)).footprint;
    if (!t(parsed, "the studio's reader reads it")) return;
    t((parsed.kicad?.tokens ?? []).every((k) => !/^\((zone|dimension|group|image|barcode)/.test(k)), `nothing kept as text: ${JSON.stringify(parsed.kicad?.tokens ?? []).slice(0, 200)}`);
    const r = await cmd({ op: "put_library_footprint", footprint: { ...parsed, name: fpName }, overwrite: true });
    if (!t(r.ok !== false, `put_library_footprint: ${r.message ?? "ok"}`)) return;
    const fp = await until(async () => { const f = await footprint(fpName); return (f.items?.images?.length ?? 0) === 1 ? f : null; }, 5000);
    if (!t(fp, "the footprint has its image")) return;
    const it = fp.items;
    t(it.barcodes.length === 1 && it.zones.length === 1 && it.dimensions.length === 2 && it.groups.length === 1, `items: ${it.barcodes.length} barcode, ${it.zones.length} zone, ${it.dimensions.length} dimensions, ${it.groups.length} group`);
    const locked = new Set(fp.kicad?.locked ?? []);
    t(it.dimensions.some((d) => locked.has(d.id)) && locked.has(it.groups[0].id), "the dimension and the group keep their locks");
    const old = it.dimensions.find((d) => d.stroke_width === 150000);
    t(old && old.kind?.kind === "aligned" && old.precision === 4 && old.keep_text_aligned === true, `the pre-6.0 dimension is an aligned one with KiCad's settings: ${JSON.stringify(old?.kind)}`);
    t((fp.display?.zone_fills ?? []).length === 1 && fp.display.zone_fills[0].fragments[0].length === 4, "the zone's fill is served");
    const img = await fetch(`/api/pcb_image?fp=${encodeURIComponent(fpName)}&id=${encodeURIComponent(it.images[0].id)}`);
    t(img.ok && img.headers.get("content-type") === "image/png", `the image's file: ${img.status} ${img.headers.get("content-type")}`);
    const shape = await until(async () => { const j = await json(`/api/barcode_shape?fp=${encodeURIComponent(fpName)}&id=${encodeURIComponent(it.barcodes[0].id)}`); return j.status !== "pending" ? j : null; }, 30000);
    t(shape?.status === "ready" && shape.paths.length > 0, `kicad-cli plots the footprint's barcode: ${shape?.status} ${shape?.error ?? ""}`);
    // drawn: the image red at its middle, the zone's fill inked, the barcode many colours
    await run("common.Control.zoomFitScreen");
    await sleep(1500);
    const [r0, g0, b0] = pixel(10_000_000, 0);
    t(r0 > 150 && g0 < 90 && b0 < 90, `the image's pixels are drawn: rgb ${r0},${g0},${b0}`);
    const bg = pixel(-14_000_000, 14_000_000);
    const zoneInk = pixel(0, 9_000_000);
    t(zoneInk.join() !== bg.join(), `the zone's fill is drawn: ${zoneInk} on ${bg}`);
    const n = await until(() => (colours(-12_000_000, -2_000_000, -8_000_000, 2_000_000) >= 2 ? colours(-12_000_000, -2_000_000, -8_000_000, 2_000_000) : null), 6000);
    t(n >= 2, `the barcode's modules are drawn (${n} colours over its box)`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ YY3
  const ID = `V10${s}:G3`;
  await scenario("YY3: three custom body styles, written at 20251024, placed in the third, listed by name and cycled", async (t) => {
    const pin = (number, y, body_style) => ({ number, name: `P${number}`, electrical_type: "passive", shape: "line", at: { x: -7.62, y }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const c of [
      { op: "new_symbol", lib_id: ID },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("1", 2.54, 1) },
      { op: "edit_symbol_units_and_connections", lib_id: ID, unit_names: {}, demorgan: false, body_style_names: ["Front", "Side", "Top"], duplicate_pin_numbers_are_jumpers: false, jumper_rows: [] },
      { op: "add_symbol_pin", lib_id: ID, pin: pin("2", -5.08, 3) },
      // "Update Symbol in Schematic": the schematic draws the editor's symbol once it is published
      { op: "update_symbol_on_board", lib_id: ID },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(ID)}`, { cache: "no-store" })).text();
    t(text.includes("(kicad_symbol_lib (version 20251024)"), "the library says 20251024");
    t(text.includes('(body_styles "Front" "Side" "Top")') && text.includes('"G3_1_3"'), "the styles are declared and the third has its sub-symbol");
    const placed = await cmd({ op: "add_symbol", id: "U951", lib_id: ID, at: { x: 152_400_000, y: 101_600_000 }, rot_millideg: 0, value: "G3", footprint: "", unit: 1 });
    if (!t(placed.ok !== false, `placed: ${JSON.stringify(placed).slice(0, 160)}`)) return;
    const set = await cmd({ op: "sch_edit", verb: "set_body_style", ids: ["U951"], style: 3 });
    t(set.ok !== false, `set_body_style 3: ${set.message ?? "ok"}`);
    const sch = await json("/api/schematic");
    const lib = sch.lib_symbols?.[ID];
    const sym = (sch.symbols ?? []).find((x) => x.id === "U951");
    t(sym?.body_style === 3, `the symbol is in body style 3: ${sym?.body_style}`);
    t(lib?.body_style_count === 3 && JSON.stringify(lib?.body_style_names) === JSON.stringify(["Front", "Side", "Top"]), `the schematic's symbol has three named styles: ${lib?.body_style_count} ${JSON.stringify(lib?.body_style_names)}`);
    t((lib?.pins ?? []).some((p) => p.number === "2" && p.body_style === 3), "the third body's own pin is the third style's");
    // Symbol Properties: the custom names
    await clickTab("Schematic");
    await run("common.InteractiveSelection.clear");
    await run("common.InteractiveSelection.selectItems", ["U951"]);
    await run("eeschema.InteractiveEdit.properties");
    const sel = await until(() => tid("symbol-body-style"), 4000);
    if (t(sel, "Symbol Properties opens")) {
      const labels = [...sel.options].map((o) => o.textContent.trim());
      t(JSON.stringify(labels) === JSON.stringify(["Front", "Side", "Top"]) && sel.value === "3", `Body style: ${JSON.stringify(labels)} at ${sel.value}`);
    }
    await closeDialogs();
    // Cycle Body Style (`toggleDeMorgan`): 3 -> 1
    await run("common.InteractiveSelection.selectItems", ["U951"]);
    await run("eeschema.InteractiveEdit.toggleDeMorgan");
    const after = await until(async () => { const v = (await json("/api/schematic")).symbols.find((x) => x.id === "U951")?.body_style; return v === 1 ? v : null; }, 4000);
    t(after === 1, `Cycle Body Style goes round from the third to the first: ${after}`);
  });

  await scenario("YY3: the Symbol Editor's Symbol Properties has the symbol's Embedded Files", async (t) => {
    await clickTab("Schematic");
    await run("common.InteractiveSelection.clear");
    await run("common.InteractiveSelection.selectItems", ["U951"]);
    await run("eeschema.EditorControl.editWithSymbolEditor");
    await until(() => E.state().tab === "symbol", 4000);
    await sleep(1200);
    await run("eeschema.InteractiveEdit.symbolProperties");
    const d = await until(() => tid("symbol-properties"), 4000);
    if (!t(d, "Symbol Properties opens in the Symbol Editor")) return;
    const files = tid("props-embedded-files");
    t(files && /Embedded Files/.test(files.textContent), "it has the Embedded Files section");
    await closeDialogs();
  });

  // clean up: the placed symbol, the footprint
  await cmd({ op: "delete_symbol", id: "U951" });
  await cmd({ op: "delete_library_symbol", lib_id: ID });
  if (fpName) await cmd({ op: "delete_library_footprint", name: fpName });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
