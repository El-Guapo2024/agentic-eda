// A placed symbol's library texts and text boxes, the clarification menu's brightened rows, an outline face's bounds (UI-LEFT-3 S3-5, S3-8, S3-24, S3-29; lane
// sch-text, task-v15-schtext; PARITY-sch section 97).
//
// What it checks:
//   1  S3-5: `SCH_PAINTER::draw( SCH_SYMBOL )`'s temp copy: a library text is drawn with the symbol's variables (`${REFERENCE}`), justified (a left-justified text
//      starts at its anchor), turned with the symbol (a quarter turn runs it up the sheet), in its face (DejaVu Sans). (A hidden library text comes only from a
//      library file; the reader's and the server's tests cover it.)
//   2  S3-8: a library text box's border keeps its line style (served as the border's `line_style`) and its text its face.
//   3  S3-24: the clarification menu's row under the pointer brightens its item on the schematic canvas (`LAYER_BRIGHTENED`, magenta in KiCad's theme), not a box.
//   4  S3-29: a text in an outline face is hit where its glyphs are drawn: a click on the far end of its ink selects it.
//   5  S3-5: a library's hidden text (`(hide yes)`, read from a `.kicad_sym`) is not drawn on the placed symbol, and is drawn with Show Hidden Fields.
//   6  S3-24: the open Find dialog's matches are drawn brightened (`BrightenItem`), with their shadow; nothing once it closes.
//   7  S3-24: the clarification menu's row brightens its item in the Symbol Editor and in the Footprint Editor.
//   8  S3-29: Preferences > Schematic Editor > Display Options > "Default font:" draws the texts with no face of their own in it, and they are hit in it.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/sch-text-v15.check.js 5`. It keeps a symbol `TextT:TXT` with the schematic, places U881
// and U882 and a text.
// It answers through `window.__schTextV15Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=5
(async () => {
  const results = [];
  const check = (window.__schTextV15Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const S = () => E.state();
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    if (document.querySelector(".menubar-dropdown .menu-node-item")) document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    results.push(entry);
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = S().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(900); };
  /** How many pixels of the canvas are within `tol` of `rgb`. */
  const count = (rgb, tol = 40) => {
    const c = canvas();
    if (!c || c.width === 0) return -1;
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const MAGENTA = [255, 0, 255];
  const drawn = () => window.__edaText?.sch ?? {};
  /** The records of the library texts of the placed symbol `ref` (`<ref>/text/<n>`), by their text. */
  const textsOf = (ref) => Object.fromEntries(Object.entries(drawn()).filter(([k]) => k.startsWith(`${ref}/text/`)).map(([, v]) => [v.text, v]));
  const LIB_ID = "TextT:TXT";
  const MM = 1e6;

  if (S().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }

  await scenario("S3-5: a placed symbol's library texts are drawn with its variables, justified, turned with it, in their face", async (t) => {
    t(S().tab === "schematic", "the schematic is shown");
    const text = (id, body, x, y, more = {}) => ({ kind: "text", id, unit: 0, body_style: 1, text: body, at: { x, y }, angle_deg: 0, size_mm: 1.27, ...more });
    const symbol = {
      lib_id: LIB_ID,
      reference_prefix: "U",
      published: true,
      graphics: [
        { kind: "rectangle", id: "body", unit: 0, body_style: 1, start: { x: -7.62, y: 7.62 }, end: { x: 7.62, y: -7.62 }, stroke_mm: 0.254, fill: "none" },
        text("t_ref", "${REFERENCE}", -6.35, 5.08, { h_align: "left", v_align: "top" }),
        text("t_face", "Wide FACE", -6.35, 0, { h_align: "left", font: "DejaVu Sans", text_color: { r: 0, g: 160, b: 0, a: 1 } }),
        { kind: "text_box", id: "t_box", unit: 0, body_style: 1, text: "BOX", start: { x: 1.27, y: 6.35 }, end: { x: 6.35, y: 3.81 }, angle_deg: 0, size_mm: 1.0, h_align: "left", v_align: "top", stroke_mm: 0.254, fill: "none", font: "DejaVu Sans" },
      ],
      pins: [{ number: "1", name: "A", electrical_type: "passive", shape: "line", at: { x: -10.16, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 0, body_style: 1, hidden: false }],
    };
    const r1 = await post({ op: "embed_lib_symbol", symbol });
    t(r1.ok !== false, `embed_lib_symbol: ${r1.message ?? "ok"}`);
    // the border's line style, as Text Box Properties sets it (`set_symbol_graphic_looks` by the box's id)
    const kept = await json(`/api/symbol?lib_id=${encodeURIComponent(LIB_ID)}`);
    const boxId = kept.graphics?.find((g) => g.kind === "text_box")?.id;
    const r0 = await post({ op: "set_symbol_graphic_looks", lib_id: LIB_ID, id: boxId, looks: {}, line_style: "dash", thickness_mm: null });
    t(r0.ok !== false, `the box's border dashed: ${r0.message ?? "ok"}`);
    for (const [id, x, rot] of [["U881", 230, 0], ["U882", 260, 90_000]]) {
      const r = await post({ op: "add_symbol", id, lib_id: LIB_ID, at: { x: x * MM, y: 160 * MM }, rot_millideg: rot, value: "TXT", footprint: "", unit: 1 });
      t(r.ok !== false, `add_symbol ${id}: ${r.message ?? "ok"}`);
    }
    const sch = await until(async () => { const s = await json("/api/schematic"); return s.symbols.some((x) => x.id === "U882") ? s : null; });
    const lib = sch?.lib_symbols?.[LIB_ID];
    if (!t(lib, "the placed symbol's library symbol is served")) return;
    const served = lib.graphics.filter((g) => g.kind === "text");
    const ref = served.find((g) => g.content === "${REFERENCE}");
    t(ref?.h_align === "left" && ref?.v_align === "top", `the text's justification is served: ${JSON.stringify(ref)}`);
    t(sch.symbols.find((x) => x.id === "U881")?.text_shown?.["${REFERENCE}"] === "U881", `the shown text is the symbol's (${JSON.stringify(sch.symbols.find((x) => x.id === "U881")?.text_shown)})`);
    await select(["U881", "U882"]);
    await E.run("common.Control.zoomFitSelection");
    await sleep(900);
    await select([]);
    const u881 = await until(() => (textsOf("U881")["U881"] ? textsOf("U881") : null), 8000);
    if (!t(u881, `U881's texts are drawn (${Object.keys(drawn()).filter((k) => k.includes("/text/")).join(", ")})`)) return;
    const box = u881["U881"].box;
    const xs = box.map((p) => p[0]);
    const ys = box.map((p) => p[1]);
    // the anchor on the sheet: the symbol's place and the served text's (the symbol as the studio places it, mm, y up); left and top justified, its box starts there
    const s881 = sch.symbols.find((x) => x.id === "U881");
    const anchor = [s881.at[0] + ref.at[0] * MM, s881.at[1] - ref.at[1] * MM];
    t(Math.abs(Math.min(...xs) - anchor[0]) < 0.4 * MM && Math.min(...ys) >= anchor[1] - 0.4 * MM, `left and top justified: the box starts at the anchor and hangs below it (${(Math.min(...xs) / MM).toFixed(3)}, ${(Math.min(...ys) / MM).toFixed(3)} against ${(anchor[0] / MM).toFixed(3)}, ${(anchor[1] / MM).toFixed(3)} mm)`);
    t(Math.max(...xs) - Math.min(...xs) > Math.max(...ys) - Math.min(...ys), "unturned, it runs along the sheet");
    const u882 = await until(() => (textsOf("U882")["U882"] ? textsOf("U882") : null), 8000);
    if (t(u882, "U882's texts are drawn")) {
      const b = u882["U882"].box;
      const w = Math.max(...b.map((p) => p[0])) - Math.min(...b.map((p) => p[0]));
      const h = Math.max(...b.map((p) => p[1])) - Math.min(...b.map((p) => p[1]));
      t(h > w, `turned a quarter with the symbol, it runs up the sheet (${(w / MM).toFixed(2)} x ${(h / MM).toFixed(2)} mm)`);
    }
    const face = await until(() => ((textsOf("U881")["Wide FACE"]?.font ?? "stroke") !== "stroke" ? textsOf("U881")["Wide FACE"] : null), 20000);
    t(face, `the face text is drawn in its outline face once it has loaded (${textsOf("U881")["Wide FACE"]?.font})`);
    // the text box: its border dashed, its text's face served
    const border = lib.graphics.find((g) => g.kind === "rectangle" && g.line_style === "dash");
    t(!!border, `the text box's border keeps its line style (${JSON.stringify(lib.graphics.filter((g) => g.kind === "rectangle").map((g) => g.line_style ?? "solid"))})`);
    t(lib.graphics.find((g) => g.kind === "text_box")?.font === "DejaVu Sans", "and its text its face");
  });

  await scenario("S3-24: the clarification menu's row brightens its item on the schematic (LAYER_BRIGHTENED), it is not boxed", async (t) => {
    const before = count(MAGENTA);
    await E.run("common.InteractiveSelection.selectionMenu", ["U881", "U882"]);
    const rows = await until(() => { const r = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")]; return r.length >= 2 ? r : null; }, 4000);
    if (!t(rows, "the menu opens")) return;
    rows[0].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await sleep(700);
    const lit = S().menuHighlight;
    t(lit.length === 1 && (lit[0] === "U881" || lit[0] === "U882"), `row 1 brightens a symbol (${lit})`);
    const during = count(MAGENTA);
    t(during > before + 200, `the symbol is drawn in the brightened colour: ${before} -> ${during} magenta pixels`);
    rows[0].dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
    await sleep(700);
    t(S().menuHighlight.length === 0, "leaving the row unbrightens it");
    t(count(MAGENTA) < during - 200, `and the canvas is drawn as before (${count(MAGENTA)} magenta pixels)`);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(400);
  });

  await scenario("S3-29: a text in an outline face is hit where its glyphs are drawn", async (t) => {
    const at = { x: 230 * MM, y: 185 * MM };
    const made = await post({ op: "add_sch_text", content: "WWWWWWWWWW", at, angle_millideg: 0, size_nm: 2_540_000 });
    const st = await json("/api/schematic");
    const tx = st.texts.find((x) => x.content === "WWWWWWWWWW");
    if (!t(tx, `a text is added (${JSON.stringify(made)})`)) return;
    const r = await post({ op: "sch_edit", verb: "set_text_attrs", ids: [tx.id], font: "DejaVu Sans" });
    t(r.ok !== false, `set in DejaVu Sans: ${r.message ?? "ok"}`);
    await select([tx.id]);
    await E.run("common.Control.zoomFitSelection");
    await sleep(900);
    await select([]);
    const rec = await until(() => ((drawn()[tx.id]?.font ?? "stroke") !== "stroke" ? drawn()[tx.id] : null), 20000);
    if (!t(rec, `drawn in its face (${drawn()[tx.id]?.font})`)) return;
    const [x0, y0, x1, y1] = rec.ink;
    // the stroke font's W is narrower than DejaVu Sans' at this size: a click on the end of the ink is past the stroke box
    await clickAt(x1 - (x1 - x0) * 0.02, (y0 + y1) / 2);
    const picked = S().selection.map((s) => s.id);
    t(picked.includes(tx.id), `a click on the far end of its glyphs (${((x1 - x0) / MM).toFixed(2)} mm of ink) selects it (${picked})`);
    await select([]);
  });

  const tabTo = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => S().tab === label.toLowerCase(), 5000); await sleep(800); };
  const menuRows = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  /** The clarification menu over `ids`, its first row under the pointer: what is lit, and the canvas before, during and after. */
  const hoverFirstRow = async (ids, measure) => {
    const before = measure();
    await E.run("common.InteractiveSelection.selectionMenu", ids);
    const rows = await until(() => (menuRows().length >= 2 ? menuRows() : null), 4000);
    if (!rows) return null;
    rows[0].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await sleep(800);
    const lit = [...S().menuHighlight];
    const during = measure();
    rows[0].dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
    await sleep(800);
    const after = measure();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(400);
    return { before, during, after, lit };
  };

  await scenario("S3-5: a library's hidden text is not drawn on the placed symbol, and is with Show Hidden Fields", async (t) => {
    if (S().tab !== "schematic") await tabTo("Schematic");
    const file = `(kicad_symbol_lib (version 20241209) (generator "test")
  (symbol "HID" (pin_names (offset 0.508)) (exclude_from_sim no) (in_bom yes) (on_board yes)
    (property "Reference" "U" (at 0 5.08 0) (effects (font (size 1.27 1.27))))
    (property "Value" "HID" (at 0 -5.08 0) (effects (font (size 1.27 1.27))))
    (symbol "HID_0_1"
      (rectangle (start -3.81 3.81) (end 3.81 -3.81) (stroke (width 0.254) (type default)) (fill (type none)))
      (text "SEEN" (at 0 1.27 0) (effects (font (size 1.27 1.27))))
      (text "GONE" (at 0 -1.27 0) (effects (font (size 1.27 1.27)) (hide yes)))
    )
    (symbol "HID_1_1"
      (pin passive line (at -6.35 0 0) (length 2.54) (name "A" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
    )
  )
)`;
    const parsed = await fetch("/api/symbol_library/parse", { method: "POST", body: file }).then((r) => r.json());
    const sym = parsed.symbols?.[0];
    if (!t(sym, `the library is read (${JSON.stringify(parsed).slice(0, 200)})`)) return;
    const put = await post({ op: "embed_lib_symbol", symbol: { ...sym, lib_id: "HidT:HID" } });
    t(put.ok !== false, `the symbol is kept with the design: ${put.message ?? "ok"}`);
    const add = await post({ op: "add_symbol", id: "U883", lib_id: "HidT:HID", at: { x: 230 * MM, y: 135 * MM }, rot_millideg: 0, value: "HID", footprint: "", unit: 1 });
    t(add.ok !== false, `add_symbol: ${add.message ?? "ok"}`);
    const sch = await until(async () => { const x = await json("/api/schematic"); return x.symbols.some((y) => y.id === "U883") ? x : null; });
    const served = sch?.lib_symbols?.["HidT:HID"]?.graphics?.find((g) => g.content === "GONE");
    t(served?.hidden === true, `the hidden flag is served (${JSON.stringify(served)})`);
    await select(["U883"]);
    await E.run("common.Control.zoomFitSelection");
    await sleep(900);
    await select([]);
    const seen = await until(() => (textsOf("U883")["SEEN"] ? textsOf("U883") : null), 6000);
    t(seen && !seen["GONE"], `the shown text is drawn, the hidden one not (${Object.keys(textsOf("U883"))})`);
    await E.run("eeschema.EditorControl.showHiddenFields");
    const shown = await until(() => (textsOf("U883")["GONE"] ? textsOf("U883") : null), 6000);
    t(!!shown, "with Show Hidden Fields it is drawn");
    await E.run("eeschema.EditorControl.showHiddenFields");
    await sleep(600);
    t(!textsOf("U883")["GONE"], "and off again, it is not");
  });

  await scenario("S3-24: Find's matches are drawn brightened while it is open (BrightenItem), not boxed", async (t) => {
    if (S().tab !== "schematic") await tabTo("Schematic");
    await select(["U881"]);
    await E.run("common.Control.zoomFitSelection");
    await select([]);
    await sleep(600);
    const before = count(MAGENTA);
    await E.run("common.Interactive.find");
    const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /^Find$/.test(d.querySelector(".dialog-header")?.textContent ?? "")), 4000);
    if (!t(dlg, "Find opens")) return;
    const input = dlg.querySelector(".kv-grid input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, "U881");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(1500);
    const bright = S().brightened;
    t(bright.length > 0, `the matches are brightened (${bright.slice(0, 4)})`);
    const during = count(MAGENTA);
    t(during > before + 30, `drawn in the brightened colour: ${before} -> ${during} magenta pixels`);
    [...dlg.querySelectorAll(".dialog-footer button")].find((b) => /^(Close|Cancel)$/.test(b.textContent.trim()))?.click();
    await sleep(900);
    t(count(MAGENTA) < during - 30, `nothing brightened once it closes (${count(MAGENTA)})`);
  });

  await scenario("S3-24: the clarification menu's row brightens its item in the Symbol Editor (LAYER_BRIGHTENED) and in the Footprint Editor", async (t) => {
    if (S().tab !== "schematic") await tabTo("Schematic");
    await select(["U881"]);
    await E.run("eeschema.EditorControl.editWithSymbolEditor");
    await until(() => S().tab === "symbol" && S().symbolEditor?.libId === LIB_ID, 8000);
    if (!t(S().symbolEditor?.libId === LIB_ID, `the Symbol Editor shows ${LIB_ID} (${S().tab}, ${S().symbolEditor?.libId})`)) return;
    await E.run("common.Control.zoomFitScreen");
    await sleep(800);
    const sym = await json(`/api/symbol?lib_id=${encodeURIComponent(LIB_ID)}`);
    const rect = sym.graphics.find((g) => g.kind === "rectangle")?.id;
    const txt = sym.graphics.find((g) => g.kind === "text")?.id;
    const r = await hoverFirstRow([rect, txt], () => count(MAGENTA));
    if (t(r, "the menu opens in the Symbol Editor")) {
      t(r.lit.length === 1, `row 1 brightens an item (${r.lit})`);
      t(r.during > r.before + 100 && r.after < r.during - 100, `it is drawn in the brightened colour, and not once the pointer leaves (${r.before} -> ${r.during} -> ${r.after} magenta pixels)`);
    }
    // the Footprint Editor: a new footprint with a wide silkscreen line and a text
    await tabTo("Footprint");
    const names = async () => (await json("/api/footprint_library")).project ?? [];
    const was = await names();
    await E.run("pcbnew.ModuleEditor.newFootprint");
    const name = await until(async () => (await names()).find((n) => !was.includes(n)), 6000);
    if (!t(name, `a new footprint (${name})`)) return;
    await post({ op: "add_footprint_graphic", footprint: name, shape: { kind: "segment", layer: "F.SilkS", stroke_width: 600_000, filled: false, start: { x: -4 * MM, y: 0 }, end: { x: 4 * MM, y: 0 } } });
    await post({ op: "add_footprint_text", footprint: name, text: { content: "LIT", at: { x: 0, y: 3 * MM }, angle: 0, layer: "F.SilkS", size_nm: 2 * MM, stroke_width: 300_000, justify: "center", mirror: false } });
    const fp = await json(`/api/footprint?name=${encodeURIComponent(name)}`);
    const seg = fp.graphics.find((g) => g.kind === "segment")?.id;
    const lit = fp.texts.find((x) => x.content === "LIT")?.id;
    await E.run("common.Control.zoomFitScreen");
    await sleep(900);
    const light = () => {
      const c = canvas();
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      let n = 0;
      for (let i = 0; i < d.length; i += 4) n += d[i] + d[i + 1] + d[i + 2];
      return n / 1000;
    };
    const f = await hoverFirstRow([seg, lit], light);
    if (t(f, "the menu opens in the Footprint Editor")) {
      t(f.lit.length === 1, `row 1 brightens an item (${f.lit})`);
      t(f.during > f.before * 1.002 && f.after < f.during, `the item is drawn lighter, and as before once the pointer leaves (${f.before.toFixed(0)} -> ${f.during.toFixed(0)} -> ${f.after.toFixed(0)})`);
    }
    await tabTo("Schematic");
  });

  await scenario("S3-29: Display Options' \"Default font:\" draws the texts with no face of their own in it, and they are hit where they are drawn", async (t) => {
    if (S().tab !== "schematic") await tabTo("Schematic");
    const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
    const setFont = async (font) => {
      await E.run("common.SuiteControl.openPreferences");
      await until(() => tid("preferences-dialog"), 4000);
      tid("prefs-page-display_schematic")?.click();
      await sleep(400);
      const sel = await until(() => (tid("pref-sch-default-font")?.querySelector(`option[value="${font}"]`) ? tid("pref-sch-default-font") : null), 8000);
      if (!sel) return false;
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(sel, font);
      sel.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(200);
      tid("preferences-ok")?.click();
      await sleep(900);
      return true;
    };
    const at = { x: 260 * MM, y: 185 * MM };
    await post({ op: "add_sch_text", content: "MMMMMMMMMM", at, angle_millideg: 0, size_nm: 2_540_000 });
    const tx = (await json("/api/schematic")).texts.find((x) => x.content === "MMMMMMMMMM");
    if (!t(tx, "a text with no face of its own")) return;
    await select([tx.id, "U881"]);
    await E.run("common.Control.zoomFitSelection");
    await select([]);
    await sleep(800);
    const strokeInk = drawn()[tx.id]?.ink;
    t(drawn()[tx.id]?.font === "stroke", `in the KiCad font, KiCad's default (${drawn()[tx.id]?.font})`);
    if (!t(await setFont("DejaVu Sans"), "Default font: DejaVu Sans")) return;
    const rec = await until(() => ((drawn()[tx.id]?.font ?? "stroke") !== "stroke" ? drawn()[tx.id] : null), 20000);
    t(!!rec, `the text is drawn in it (${drawn()[tx.id]?.font})`);
    const lib = textsOf("U881")["U881"];
    t(lib && lib.font !== "stroke", `and a symbol's library text with no face (${lib?.font})`);
    const ref = Object.entries(drawn()).find(([k, v]) => k.includes("U881") && k.includes("Reference") && v)?.[1];
    t(!ref || ref.font !== "stroke", `and its Reference field (${ref?.font})`);
    if (rec && strokeInk) {
      const [x0, y0, x1, y1] = rec.ink;
      t(Math.abs(x1 - x0 - (strokeInk[2] - strokeInk[0])) > 0.2 * MM, `its glyphs are another width (${((x1 - x0) / MM).toFixed(2)} against ${((strokeInk[2] - strokeInk[0]) / MM).toFixed(2)} mm)`);
      await clickAt(x1 - (x1 - x0) * 0.02, (y0 + y1) / 2);
      t(S().selection.some((s) => s.id === tx.id), `a click on the far end of its glyphs selects it (${S().selection.map((s) => s.id)})`);
      await select([]);
    }
    t(await setFont("KiCad Font"), "put back to the KiCad font");
    await sleep(800);
    t(drawn()[tx.id]?.font === "stroke", "drawn in the stroke font again");
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
