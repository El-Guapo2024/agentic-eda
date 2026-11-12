// Page Settings' drawing sheet file on both editors (UI-LEFT C3; PARITY-common.md section 9, GAPS.md item 18): the board shows KiCad's default sheet with its
// zone letters and numbers from the start (`GAL_SET::DefaultVisible`'s LAYER_DRAWINGSHEET); a `.kicad_wks` from this computer is put in the board's folder from
// the dialog's "Upload..." with "Embed file" unticked and previewed (`UpdateDrawingSheetExample`), OK sets it on the board (`set_drawing_sheet`) and the board draws it; the
// browse button lists the `.kicad_wks` files of the board's folder with KiCad's template folder one click away (`OnWksFileSelection`); on the schematic the name
// typed into the File box is taken on OK and the sheet draws it; Ctrl+Z on each leaves the sheet (a project setting in KiCad, CC8). The sheet is KiCad's own QA sheet
// (qa/data/cli/basic_test/custom_ds.kicad_wks): four "CUSTOM DRAWING SHEET" texts the default sheet has not.
// What was drawn is read from `window.__edaDrawingSheet` (components/drawingSheet/dsPainter.ts: the texts of the last sheet painted on each canvas).
// Runs on a fresh `eda board new examples/mcu_board_30plus.yaml` board; it writes `custom_ds.kicad_wks` into the board's folder, so run it on a scratch copy.
// It answers through `window.__drawingSheetCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__drawingSheetCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 30000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(200); return v; };

  const SHEET = "(kicad_wks (version 20220228) (generator pl_editor)\n  (setup (textsize 1.5 1.5)(linewidth 0.15)(textlinewidth 0.15)\n  (left_margin 10)(right_margin 10)(top_margin 10)(bottom_margin 10))\n  (rect (name \"\") (start 110 34) (end 2 2) (comment \"rect around the title block\")\n)\n  (rect (name \"\") (start 0 0 ltcorner) (end 0 0) (repeat 2) (incrx 2) (incry 2))\n  (line (name \"\") (start 50 2 ltcorner) (end 50 0 ltcorner) (repeat 30) (incrx 50))\n  (tbtext \"1\" (name \"\") (pos 25 1 ltcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n  (line (name \"\") (start 50 2 lbcorner) (end 50 0 lbcorner) (repeat 30) (incrx 50))\n  (tbtext \"1\" (name \"\") (pos 25 1 lbcorner) (font (size 1.3 1.3)) (repeat 100) (incrx 50))\n  (line (name \"\") (start 0 50 ltcorner) (end 2 50 ltcorner) (repeat 30) (incry 50))\n  (tbtext \"A\" (name \"\") (pos 1 25 ltcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n  (line (name \"\") (start 0 50 rtcorner) (end 2 50 rtcorner) (repeat 30) (incry 50))\n  (tbtext \"A\" (name \"\") (pos 1 25 rtcorner) (font (size 1.3 1.3)) (justify center) (repeat 100) (incry 50))\n  (line (name \"\") (start 110 5.5) (end 2 5.5))\n  (line (name \"\") (start 110 8.5) (end 2 8.5))\n  (tbtext \"Rev: ${REVISION}\" (name \"\") (pos 24 6.9) (font bold))\n  (tbtext \"Size: ${PAPER}\" (name \"\") (pos 109 6.9) (comment \"Paper format name\")\n)\n  (tbtext \"Id: ${#}/${##}\" (name \"\") (pos 24 4.1) (comment \"Sheet id\")\n)\n  (line (name \"\") (start 110 12.5) (end 2 12.5))\n  (tbtext \"Title: ${TITLE}\" (name \"\") (pos 109 10.7) (font (size 2 2) bold italic))\n  (tbtext \"File: ${FILENAME}\" (name \"\") (pos 109 14.3))\n  (line (name \"\") (start 110 18.5) (end 2 18.5))\n  (tbtext \"Sheet: ${SHEETPATH}\" (name \"\") (pos 109 17))\n  (tbtext \"${COMPANY}\" (name \"\") (pos 109 20) (font bold) (comment \"Company name\")\n)\n  (tbtext \"${COMMENT1}\" (name \"\") (pos 109 23) (comment \"Comment 0\")\n)\n  (tbtext \"${COMMENT2}\" (name \"\") (pos 109 26) (comment \"Comment 1\")\n)\n  (tbtext \"${COMMENT3}\" (name \"\") (pos 109 29) (comment \"Comment 2\")\n)\n  (tbtext \"${COMMENT4}\" (name \"\") (pos 109 32) (comment \"Comment 3\")\n)\n  (line (name \"\") (start 90 8.5) (end 90 5.5))\n  (line (name \"\") (start 26 8.5) (end 26 2))\n  (tbtext \"CUSTOM DRAWING SHEET\" (name \"\") (pos 79.789 22.8422) (font (size 4 4) bold))\n  (tbtext \"CUSTOM DRAWING SHEET\" (name \"\") (pos 274.0992 6.3322) (font (size 4 4) bold))\n  (tbtext \"CUSTOM DRAWING SHEET\" (name \"\") (pos 274.0992 184.1322) (font (size 4 4) bold))\n  (tbtext \"CUSTOM DRAWING SHEET\" (name \"\") (pos 77.2492 184.1322) (font (size 4 4) bold))\n)\n";
  const MARK = "CUSTOM DRAWING SHEET";
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const schematic = () => json("/api/schematic");
  const run = (id, args) => __eda.run(id, args);
  const dialog = () => [...document.querySelectorAll(".dialog")].find((d) => d.getAttribute("aria-label") === "Page Settings") ?? null;
  const browser = () => [...document.querySelectorAll(".dialog")].find((d) => d.getAttribute("aria-label") === "Drawing Sheet File") ?? null;
  const button = (root, re) => [...(root?.querySelectorAll("button") ?? [])].find((b) => re.test(b.textContent.trim()) || re.test(b.getAttribute("aria-label") ?? ""));
  const field = (label) => dialog()?.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const painted = (tag) => (window.__edaDrawingSheet ?? {})[tag] ?? null;
  const shows = (tag, s) => !!painted(tag)?.texts?.some((x) => x === s);
  const close = async () => { for (let i = 0; i < 3 && (dialog() || browser()); i++) { button(browser() ?? dialog(), /^Cancel$/)?.click(); await sleep(250); } };
  const tabTo = async (label) => { [...document.querySelectorAll(".editor-tabs .editor-tab")].find((e) => e.textContent.trim() === label)?.click(); await sleep(900); };
  const nudge = async () => { await run("common.Control.zoomRedraw").catch(() => null); await sleep(300); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await close(); } catch { /* ignore */ }
    results.push(entry);
  };

  if (__eda.state().tab !== "pcb") await tabTo("PCB");
  const start = await state();
  const startSch = await schematic();

  await scenario("the board shows KiCad's default drawing sheet from the start, with its zone letters and numbers", async (t) => {
    t(start.drawing_sheet == null, `no sheet chosen yet (${JSON.stringify(start.drawing_sheet)})`);
    await run("common.Control.zoomFitScreen");
    const p = await until(async () => { await nudge(); return painted("pcb"); }, 15000);
    if (!t(p, "the board canvas painted a drawing sheet")) return;
    t(["1", "2", "3", "4", "5", "6"].every((n) => p.texts.includes(n)) && ["A", "B", "C", "D"].every((l) => p.texts.includes(l)), `the A4 sheet's zone references 1..6 and A..D (${p.texts.filter((x) => x.length === 1).join(" ")})`);
    t(p.texts.some((x) => x.startsWith("Size: ")) && p.texts.some((x) => x.startsWith("File: ")), `and the title block (${p.texts.filter((x) => x.includes(":")).join(" | ")})`);
    t(!shows("pcb", MARK), "not the custom sheet's texts");
  });

  await scenario("on the board: Upload... (Embed file unticked) puts the sheet in the board's folder and previews it, OK sets it and the board draws it", async (t) => {
    const r = await run("common.Control.pageSettings");
    t(r.dialog === "Page Settings", `File > Page Settings... opens the dialog (${r.dialog ?? r.error})`);
    const embed = field("Embed file");
    if (embed?.checked) embed.click();
    await sleep(100);
    const input = dialog()?.querySelector('[data-testid="page-drawing-sheet-upload"]');
    if (!t(input, "the dialog has the drawing sheet's upload")) return;
    const dt = new DataTransfer();
    dt.items.add(new File([SHEET], "custom_ds.kicad_wks", { type: "text/plain" }));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    const named = await until(async () => field("Drawing sheet file")?.value === "custom_ds.kicad_wks", 15000);
    t(named, `the File box names it relative to the board's folder (${field("Drawing sheet file")?.value})`);
    const previewed = await until(async () => shows("sketch", MARK), 10000);
    t(previewed, `the preview draws the chosen sheet (${painted("sketch")?.texts?.length ?? 0} texts)`);
    const before = (await state()).activity?.length ?? 0;
    button(dialog(), /^OK$/)?.click();
    if (!t(await until(async () => !dialog(), 15000), "OK closes the dialog")) return;
    const after = await until(async () => { const s = await state(); return s.drawing_sheet?.path === "custom_ds.kicad_wks" ? s : null; }, 15000);
    if (!t(after, `the board's drawing sheet is the file (${JSON.stringify((await state()).drawing_sheet?.path)})`)) return;
    t((after.drawing_sheet.text ?? "").includes(MARK) && after.drawing_sheet.error == null, "the state carries its text");
    t((after.activity?.length ?? 0) === before + 1 && /drawing-sheet "custom_ds.kicad_wks"/.test(after.activity?.[0]?.cmd ?? ""), `one set_drawing_sheet command (${after.activity?.[0]?.cmd})`);
    const drawn = await until(async () => { await nudge(); return shows("pcb", MARK); }, 15000);
    t(drawn, `the board draws the custom sheet (${(painted("pcb")?.texts ?? []).filter((x) => x.length > 6).slice(0, 4).join(" | ")})`);
    t(["1", "A"].every((x) => painted("pcb")?.texts?.includes(x)), "with its own zone references");
  });

  await scenario("the browse button lists the board folder's .kicad_wks files, KiCad's template folder one click away", async (t) => {
    await run("common.Control.pageSettings");
    button(dialog(), /^Browse$/)?.click();
    const b = await until(async () => browser(), 10000);
    if (!t(b, "Browse opens the Drawing Sheet File browser")) return;
    const listed = await until(async () => [...b.querySelectorAll("[role=option]")].map((o) => o.getAttribute("data-name")).includes("custom_ds.kicad_wks"), 10000);
    t(listed, `the board's folder lists custom_ds.kicad_wks (${[...b.querySelectorAll("[role=option]")].map((o) => o.getAttribute("data-name")).join(", ")})`);
    t([...b.querySelectorAll("[role=option]")].every((o) => o.querySelector(".lt-browser-icon")?.textContent === "▸" || /\.kicad_wks$/i.test(o.getAttribute("data-name"))), "and only folders and drawing sheets");
    const templates = button(b, /^KiCad templates$/);
    if (templates) {
      templates.click();
      const iso = await until(async () => [...browser().querySelectorAll("[role=option]")].some((o) => /ISO5457.*\.kicad_wks$/.test(o.getAttribute("data-name"))), 10000);
      t(iso, "KiCad templates lists KiCad's own sheets");
    } else {
      t(true, "no KiCad template folder on this machine (the button is not offered)");
    }
    button(browser(), /^Cancel$/)?.click();
    await sleep(300);
    t(dialog() && !browser(), "Cancel goes back to Page Settings");
    button(dialog(), /^Cancel$/)?.click();
    await until(async () => !dialog(), 5000);
  });

  await scenario("on the schematic: a name typed into the File box is taken on OK and the sheet draws it", async (t) => {
    await tabTo("Schematic");
    const p0 = await until(async () => { await nudge(); return painted("sch"); }, 15000);
    t(p0 && !shows("sch", MARK) && p0.texts.includes("A"), `the schematic draws KiCad's default sheet first (${(p0?.texts ?? []).length} texts)`);
    t(startSch.drawing_sheet == null, "no sheet chosen on the schematic yet");
    const r = await run("common.Control.pageSettings");
    t(r.dialog === "Page Settings", `the dialog opens on the Schematic tab (${r.dialog ?? r.error})`);
    setValue(field("Drawing sheet file"), "custom_ds.kicad_wks");
    await sleep(200);
    button(dialog(), /^OK$/)?.click();
    if (!t(await until(async () => !dialog(), 15000), "OK closes the dialog")) return;
    const s = await until(async () => { const x = await schematic(); return x.drawing_sheet?.path === "custom_ds.kicad_wks" ? x : null; }, 15000);
    t(s, `the schematic's drawing sheet is the file (${JSON.stringify((await schematic()).drawing_sheet?.path)})`);
    t(s?.sheet_page?.first === true && s?.sheet_page?.count >= 1, `the page it is drawn for: ${JSON.stringify(s?.sheet_page)}`);
    const drawn = await until(async () => { await nudge(); return shows("sch", MARK); }, 15000);
    t(drawn, "the schematic draws the custom sheet");
    t((await state()).drawing_sheet?.path === "custom_ds.kicad_wks", "the board keeps its own");
    await run("common.Interactive.undo");
    await sleep(1500);
    t((await schematic()).drawing_sheet?.path === "custom_ds.kicad_wks", "Ctrl+Z on the schematic leaves its sheet: a project setting (`SavePageSettings`, outside the commit)");
  });

  await scenario("Ctrl+Z on the board leaves its sheet too", async (t) => {
    await tabTo("PCB");
    await run("common.Interactive.undo");
    await sleep(1500);
    t((await state()).drawing_sheet?.path === "custom_ds.kicad_wks", `the board's drawing sheet stays (${JSON.stringify((await state()).drawing_sheet?.path ?? null)})`);
    const drawn = await until(async () => { await nudge(); return shows("pcb", MARK); }, 15000);
    t(drawn, "and the board still draws it");
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return results;
})();
