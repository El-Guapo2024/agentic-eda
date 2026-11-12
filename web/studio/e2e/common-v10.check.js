// UI-LEFT-2 CC6, CC7, CC8 and XX3 (task-v10-common; PARITY-common.md section 25, PARITY-boardctl.md section 10):
//   - Page Settings' "Embed file" (KiCad 25ec2335 `FILEDLG_HOOK_EMBED_FILE`, on by default): a sheet uploaded with it is embedded in the board at once, named
//     `kicad-embed://<name>`, typed worksheet; the sheet's `@{..}` are evaluated (`BuildFullText`'s `EXPRESSION_EVALUATOR`, the clock's functions too) in the
//     preview and on the board; the choice is no undo step (a project setting) and stays through Ctrl+Z;
//   - the schematic's "Export to other sheets" boxes: a title ticked goes to every other sheet (`onSavePageSettings`), the sheet in view keeps the dialog's;
//   - the Datasheet cell of Symbol Properties has KiCad's URL button (`TEXT_BUTTON_URL`): the folder (disabled) for no value, the globe for one;
//   - Plot: the PostScript text controls start empty (`dialog_plot_base.cpp`), and with no layer checked on a board of two copper layers the plot goes on and says
//     "Done." (`applyPlotSettings` adds the 30 copper layers the board lacks to the selection), never "No layer selected".
// Runs on a fresh `eda board new examples/mcu_board_30plus.yaml` board (it embeds a sheet and adds a sheet to the schematic: a scratch copy).
// It answers through `window.__commonV10Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__commonV10Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 30000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(200); return v; };

  const SHEET =
    "(kicad_wks (version 20220228) (generator pl_editor)\n" +
    "  (setup (textsize 1.5 1.5)(linewidth 0.15)(textlinewidth 0.15) (left_margin 10)(right_margin 10)(top_margin 10)(bottom_margin 10))\n" +
    "  (rect (name \"\") (start 110 34) (end 2 2))\n" +
    "  (tbtext \"Answer @{40+2}\" (name \"\") (pos 100 10))\n" +
    "  (tbtext \"Day @{weekdayname(datestring('2026-11-06'))}\" (name \"\") (pos 100 14))\n" +
    "  (tbtext \"V10 OUTLINE\" (name \"\") (pos 100 20) (font (face \"DejaVu Sans\") (size 3 3)))\n" +
    ")\n";
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const schematic = (q = "") => json(`/api/schematic${q}`);
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const run = (id, args) => __eda.run(id, args);
  const dialogNamed = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.getAttribute("aria-label") ?? "") || re.test(d.querySelector(".dialog-header")?.textContent?.trim() ?? "")) ?? null;
  const pageDialog = () => dialogNamed(/^Page Settings$/);
  const button = (root, re) => [...(root?.querySelectorAll("button") ?? [])].find((b) => re.test(b.textContent.trim()) || re.test(b.getAttribute("aria-label") ?? ""));
  const field = (label) => pageDialog()?.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const painted = (tag) => (window.__edaDrawingSheet ?? {})[tag] ?? null;
  const shows = (tag, s) => !!painted(tag)?.texts?.some((x) => x === s);
  const closeAll = async () => { for (let i = 0; i < 4 && document.querySelector(".dialog"); i++) { button([...document.querySelectorAll(".dialog")].pop(), /^(Cancel|Close)$/)?.click(); await sleep(250); } };
  const tabTo = async (label) => { [...document.querySelectorAll(".editor-tabs .editor-tab")].find((e) => e.textContent.trim() === label)?.click(); await sleep(900); };
  const nudge = async () => { await run("common.Control.zoomRedraw").catch(() => null); await sleep(300); };
  const confirmYes = async () => { const d = await until(async () => dialogNamed(/^Confirmation$/), 2000); if (d) button(d, /^Yes$/)?.click(); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeAll(); } catch { /* ignore */ }
    results.push(entry);
  };

  if (__eda.state().tab !== "pcb") await tabTo("PCB");

  await scenario("Page Settings: Embed file (on by default) embeds the uploaded sheet in the board, kicad-embed://, its @{..} evaluated, no undo step", async (t) => {
    const r = await run("common.Control.pageSettings");
    t(r.dialog === "Page Settings", `the dialog opens (${r.dialog ?? r.error})`);
    t(field("Embed file")?.checked === true, "Embed file is ticked (`embed_file_defaults.drawing_sheet`)");
    const input = pageDialog()?.querySelector('[data-testid="page-drawing-sheet-upload"]');
    if (!t(input, "the dialog has the upload")) return;
    const dt = new DataTransfer();
    dt.items.add(new File([SHEET], "v10.kicad_wks", { type: "text/plain" }));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    const named = await until(async () => field("Drawing sheet file")?.value === "kicad-embed://v10.kicad_wks", 15000);
    t(named, `the File box names the embedded file (${field("Drawing sheet file")?.value})`);
    const listing = await until(async () => { const l = await json("/api/embedded?owner=board"); return l.files?.some((f) => f.name === "v10.kicad_wks") ? l : null; }, 10000);
    t(listing?.files?.find((f) => f.name === "v10.kicad_wks")?.type === "worksheet", `embedded in the board at the pick, typed worksheet (${JSON.stringify(listing?.files?.map((f) => [f.name, f.type]))})`);
    const previewed = await until(async () => shows("sketch", "Answer 42") && shows("sketch", "Day Friday"), 15000);
    t(previewed, `the preview evaluates the sheet's @{..} (${(painted("sketch")?.texts ?? []).filter((x) => /Answer|Day/.test(x)).join(" | ")})`);
    t(shows("sketch", "V10 OUTLINE"), "and draws its outline-font text");
    button(pageDialog(), /^OK$/)?.click();
    if (!t(await until(async () => !pageDialog(), 15000), "OK closes the dialog")) return;
    const after = await until(async () => { const s = await state(); return s.drawing_sheet?.path === "kicad-embed://v10.kicad_wks" ? s : null; }, 15000);
    if (!t(after, `the board's drawing sheet is the embedded one (${JSON.stringify((await state()).drawing_sheet)})`)) return;
    t(after.drawing_sheet.embedded === true && (after.drawing_sheet.text ?? "").includes("Answer @{40+2}"), "read from the board's embedded files");
    const drawn = await until(async () => { await nudge(); return shows("pcb", "Answer 42"); }, 15000);
    t(drawn, `the board draws it, evaluated (${(painted("pcb")?.texts ?? []).filter((x) => /Answer/.test(x)).join(" | ")})`);
    await run("common.Interactive.undo");
    await sleep(1500);
    t((await state()).drawing_sheet?.path === "kicad-embed://v10.kicad_wks", "Ctrl+Z leaves it: a project setting, as KiCad's");
    t((await json("/api/embedded?owner=board")).files?.some((f) => f.name === "v10.kicad_wks"), "and the embedded file with it");
  });

  await scenario("Page Settings: picking a file in place of the embedded sheet asks to remove it, and OK does", async (t) => {
    await run("common.Control.pageSettings");
    if (!t(pageDialog(), "the dialog opens")) return;
    const embed = field("Embed file");
    if (embed?.checked) embed.click();
    await sleep(100);
    const input = pageDialog()?.querySelector('[data-testid="page-drawing-sheet-upload"]');
    const dt = new DataTransfer();
    dt.items.add(new File([SHEET.replace("Answer", "Other")], "v10_other.kicad_wks", { type: "text/plain" }));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    const asked = await until(async () => dialogNamed(/^Confirmation$/), 10000);
    t(asked && /Remove the previously embedded drawing sheet file 'v10.kicad_wks'\?/.test(asked.textContent), `KiCad's question (${asked?.textContent?.slice(0, 90)})`);
    await confirmYes();
    const named = await until(async () => field("Drawing sheet file")?.value === "v10_other.kicad_wks", 10000);
    t(named, `unticked, the file goes into the board's folder (${field("Drawing sheet file")?.value})`);
    button(pageDialog(), /^OK$/)?.click();
    await until(async () => !pageDialog(), 15000);
    const gone = await until(async () => !(await json("/api/embedded?owner=board")).files?.some((f) => f.name === "v10.kicad_wks"), 10000);
    t(gone, "OK removed the embedded sheet (`m_embeddedSheetsToRemove`)");
    t((await state()).drawing_sheet?.path === "v10_other.kicad_wks", "and the board names the new one");
  });

  await scenario("the schematic's Export to other sheets: a ticked title goes to every other sheet", async (t) => {
    const added = await cmd({ op: "add_sheet", name: "V10Sub", file: "v10sub.kicad_sch", at: { x: 203_200_000, y: 152_400_000 }, size: [25_400_000, 12_700_000] });
    if (!t(added.ok !== false, `a sheet is added (${added.message ?? "ok"})`)) return;
    await tabTo("Schematic");
    const sub = (await schematic()).sheets?.find((s) => s.name === "V10Sub");
    if (!t(sub, "the sheet is on the root")) return;
    await run("common.Control.pageSettings");
    if (!t(pageDialog(), "Page Settings opens on the schematic")) return;
    const box = field("Export Title to other sheets");
    t(box && field("Export paper to other sheets"), "the boxes are there (paper and each title block field)");
    setValue(field("Title"), "V10 TITLE");
    await sleep(100);
    if (box && !box.checked) box.click();
    await sleep(100);
    button(pageDialog(), /^OK$/)?.click();
    await until(async () => !pageDialog(), 15000);
    const root = await until(async () => { const s = await schematic(); return s.title_block?.title === "V10 TITLE" ? s : null; }, 15000);
    t(root, "the sheet in view takes the title");
    const other = await until(async () => { const s = await schematic(`?sheet=${sub.id}`); return s.title_block?.title === "V10 TITLE" ? s : null; }, 15000);
    t(other, `and the other sheet too (${JSON.stringify((await schematic(`?sheet=${sub.id}`)).title_block)})`);
    await run("common.Control.pageSettings");
    t(field("Export Title to other sheets")?.checked === true, "the box is kept for the next opening (the field is not empty)");
    await closeAll();
    await tabTo("PCB");
  });

  await scenario("Symbol Properties: the Datasheet cell's URL button (folder for none, globe for a value)", async (t) => {
    const put = await cmd({ op: "add_symbol", id: "U77", lib_id: "Amplifier_Operational:LM358", at: { x: 50_800_000, y: 101_600_000 }, rot_millideg: 0, value: "", footprint: "", unit: 1 });
    t(put.ok !== false, `an LM358 is placed (${put.message ?? "ok"})`);
    await tabTo("Schematic");
    const sym = await until(async () => (await schematic()).symbols?.find((x) => x.id === "U77"), 10000);
    if (!t(sym, "the schematic has the symbol")) return;
    await run("common.InteractiveSelection.clear");
    await run("common.InteractiveSelection.selectItems", [sym.id]);
    await sleep(300);
    await run("eeschema.InteractiveEdit.properties");
    const dlg = await until(async () => dialogNamed(/^Symbol Properties/), 10000);
    if (!t(dlg, "Symbol Properties opens")) return;
    const ds = dlg.querySelector('[aria-label="Datasheet"]');
    const btn = dlg.querySelector('[aria-label="Open Datasheet"]');
    if (!t(ds && btn, "the Datasheet cell has its button")) return;
    setValue(ds, "");
    await sleep(150);
    t(btn.disabled && btn.getAttribute("data-icon") === "small_folder", "no value: the folder, disabled (a page has no path to keep)");
    setValue(ds, "kicad-embed://v10.pdf");
    await sleep(150);
    const b2 = dlg.querySelector('[aria-label="Open Datasheet"]');
    t(!b2.disabled && b2.getAttribute("data-icon") === "www", "a value: the globe, which opens it from the stack");
    await closeAll();
    await tabTo("PCB");
  });

  await scenario("Plot: the PostScript controls start empty; no layer checked on a two-layer board plots nothing and says Done.", async (t) => {
    await tabTo("PCB");
    await run("common.Control.plot");
    const dlg = await until(async () => dialogNamed(/^Plot$/), 10000);
    if (!t(dlg, "the Plot dialog opens")) return;
    const fmt = dlg.querySelector('[data-testid="plot-format"]');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(fmt, "ps");
    fmt.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(300);
    const ps = [...dlg.querySelectorAll('[data-testid="plot-ps-options"] input[type=text]')].map((i) => i.value);
    t(ps.length === 3 && ps.every((v) => v === ""), `X, Y and width correction start empty (${JSON.stringify(ps)})`);
    for (const c of dlg.querySelectorAll("label[data-layer] input[type=checkbox]")) if (c.checked) c.click();
    await sleep(200);
    button(dlg, /^Plot$/)?.click();
    const msg = await until(async () => { const b = dialogNamed(/^Plot$/)?.querySelector("b"); return b && b.textContent.trim() ? b.textContent : null; }, 30000);
    t(msg && !/No layer selected/.test(msg) && /Done\./.test(msg), `"Done.", not "No layer selected" (${msg})`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return results;
})();
