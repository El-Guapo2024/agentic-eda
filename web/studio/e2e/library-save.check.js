// Saving into the libraries of the library tables (UI-LEFT L1, GAPS.md item 5, PARITY-common.md section 16): in the Symbol Editor and the Footprint Editor, New Library
// makes a library of the project table, a new symbol / footprint goes into it and is edited, Save writes it into the library's file, Save As puts a copy into another
// table library (the overwrite question in any case, the copy opened), Revert takes an edit back to the file's version, Save Library As writes the whole library to a
// new file with its own table row, and a symbol or footprint the design forgets is opened again from its library's file as it was saved. The tree marks what is not
// saved yet " *".
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new examples/ldo.yaml
// -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes libraries of its own in the board's folder (nicknames starting with `Ls`), takes
// their rows out of the board's tables at the end and leaves their files. It answers through `window.__librarySaveCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__librarySaveCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(80); };
  const until = async (cond, ms = 8000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(600); };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  // the library's row by the name it carries: its text ends in the count (and " *" when modified), and a name that ends in a digit -- the base-36 time in these
  // checks' names does, 10 times in 36 -- read as part of the count once the digits were stripped from the text
  const libRow = (lib) => document.querySelector(`.library-tree-group[data-library="${CSS.escape(lib)}"]`);
  const modified = (kind) => json(`/api/library/modified?kind=${kind}`);
  const savedSymbol = async (id) => (await json(`/api/library/saved?kind=symbol&id=${encodeURIComponent(id)}`)).symbol;
  const savedFootprint = async (id) => (await json(`/api/library/saved?kind=footprint&id=${encodeURIComponent(id)}`)).footprint;
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const footprint = (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const tables = () => json("/api/library_tables");
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
  /** Select a tree row as a click does (the library rows: the tree's library selection; an item: the editor's tree selection). */
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  /** A click on a library row selects it and opens or folds it: it is clicked again when it folded. */
  const selectLibrary = async (lib) => {
    const r = await until(() => libRow(lib), 6000);
    if (!r) return false;
    r.click();
    await sleep(250);
    if (!isOpen(lib)) libRow(lib)?.click();
    await sleep(250);
    return true;
  };
  /** An item's row, with its library opened first (a folded library shows none). */
  const itemRow = async (id) => {
    const lib = id.split(":")[0];
    if (await until(() => libRow(lib), 6000)) if (!isOpen(lib)) { libRow(lib).click(); await sleep(250); }
    return until(() => treeRow(id), 6000);
  };
  const selectItem = async (id) => { const r = await itemRow(id); r?.click(); await sleep(300); return !!r; };
  /** New Library through the dialog the action opens: the folder is the board's, the name typed. */
  const newLibrary = async (t, title, name) => {
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog(title), 4000);
    if (!t(d, `${title} opens`)) return false;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), name);
    document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
    button(d, "Create").click();
    return !!t(await until(() => !dialog(title), 6000), `${title}: ${name} made and put in the project's table`);
  };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };

  const s = Date.now().toString(36);
  const SYM_A = `LsSymA${s}`, SYM_B = `LsSymB${s}`, FP_A = `LsFpA${s}`, FP_B = `LsFpB${s}`;
  const pin = (n, y) => ({ id: "", number: String(n), name: `P${n}`, electrical_type: "passive", shape: "line", at: { x: 0, y }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false });

  // ------------------------------------------------------------------------------------------------------------------------ the Symbol Editor
  await scenario("symbol: New Library, a new symbol in it, edited and saved into its file", async (t) => {
    await clickTab("Symbol");
    if (!(await newLibrary(t, "New Symbol Library", SYM_A))) return;
    const row = (await tables()).tables.symbol.project.rows.find((r) => r.nickname === SYM_A);
    t(row && row.uri === `\${KIPRJMOD}/${SYM_A}.kicad_sym`, `the row: ${JSON.stringify(row)}`);
    t(await selectLibrary(SYM_A), "the new library is in the tree");
    // `CreateNewSymbol` in the target library (`getTargetLib()`: the library selected)
    await run("eeschema.SymbolLibraryControl.newSymbol");
    const nd = await until(() => document.querySelector('[data-testid="new-symbol"]'), 3000);
    if (!t(nd, "New Symbol opens")) return;
    await type(document.querySelector('[data-testid="new-symbol-name"]'), "LsR");
    document.querySelector('[data-testid="new-symbol-ok"]').click();
    const id = `${SYM_A}:LsR`;
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(id), 5000), `the new symbol is ${id}`);
    // an edit, as the editor's verbs make it: two pins and a description
    const sym = await symbol(id);
    const put = await cmd({ op: "put_library_symbol", symbol: { ...sym, description: "Saved by the browser check", pins: [pin(1, 2.54), pin(2, -2.54)] }, overwrite: true });
    t(put.ok !== false, `edited: ${JSON.stringify(put).slice(0, 120)}`);
    const m = await modified("symbol");
    t(m.items.includes(id) && m.libraries.includes(SYM_A), `modified before the save: ${JSON.stringify(m)}`);
    await itemRow(id);
    t(await until(() => treeRow(id)?.textContent.trim() === "LsR *", 5000), `the tree marks it: "${treeRow(id)?.textContent.trim()}"`);
    t(await until(() => /\*/.test(libRow(SYM_A)?.textContent ?? ""), 3000), "and its library");
    t((await savedSymbol(id)) === null, "the file has no LsR yet");
    // Save (Ctrl+S) with the symbol selected in the tree: the library's file
    await selectItem(id);
    const r = await run("common.Control.save");
    // `saveLibrary` formats "Symbol library file '%s' saved." and shows nothing (UI-LEFT-2 LL6)
    t(r.ok && !/saved/.test(JSON.stringify(r.toast ?? "")), `Save, with no message: ${JSON.stringify(r).slice(0, 200)}`);
    const file = await savedSymbol(id);
    t(file && file.pins.length === 2 && file.description === "Saved by the browser check", `the file holds the edited symbol: ${file && file.pins.length} pins, "${file && file.description}"`);
    t((await modified("symbol")).items.length === 0, "nothing is modified after the save");
    await itemRow(id);
    t(await until(() => treeRow(id)?.textContent.trim() === "LsR", 5000), `the mark is gone: "${treeRow(id)?.textContent.trim()}"`);
  });

  await scenario("symbol: Save As into another table library asks before it overwrites, in any case, and the copy is saved there", async (t) => {
    if (!(await newLibrary(t, "New Symbol Library", SYM_B))) return;
    const from = `${SYM_A}:LsR`;
    await selectItem(from);
    await run("eeschema.SymbolLibraryControl.saveSymbolAs");
    let d = await until(() => dialog("Save Symbol As"), 3000);
    if (!t(d, "Save Symbol As opens")) return;
    t(await until(() => d.querySelector(`[data-lib="${SYM_B}"]`), 4000), "the other library is listed");
    t(d.querySelector('[data-lib="Device"]') || !(await json("/api/library/writable?kind=symbol")).libraries.some((l) => l.name === "Device"), "KiCad's libraries are listed too");
    await type(d.querySelector("input"), "LsR2");
    d.querySelector(`[data-lib="${SYM_B}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    const copy = `${SYM_B}:LsR2`;
    // Save As is over when its dialog is (it puts the copy, opens it, then closes)
    t(await until(() => !dialog("Save Symbol As"), 8000), "Save Symbol As closes");
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(copy), 5000), `the copy ${copy} is in the library's buffer`);
    t((await modified("symbol")).items.includes(copy), "and is modified until the library is saved");
    // Save: the open symbol is the copy (Save As opens it)
    await selectLibrary(SYM_B);
    const r = await run("common.Control.save");
    t(r.ok, `Save ${SYM_B}: ${JSON.stringify(r.toast ?? r.error)}`);
    const file = await savedSymbol(copy);
    t(file && file.pins.length === 2, `the copy is in ${SYM_B}'s file`);
    // the same name in another case is the same name (`SymbolNameInUse`, CmpNoCase): the question, and Cancel keeps the dialog
    await selectItem(from);
    await run("eeschema.SymbolLibraryControl.saveSymbolAs");
    d = await until(() => dialog("Save Symbol As"), 3000);
    await until(() => d.querySelector(`[data-lib="${SYM_B}"]`), 4000);
    await type(d.querySelector("input"), "lsr2");
    d.querySelector(`[data-lib="${SYM_B}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    const q = await until(() => dialog("Confirmation"), 4000);
    t(q && q.textContent.includes(`Symbol 'lsr2' already exists in library '${SYM_B}'. Do you want to overwrite it?`), `the question: "${q?.textContent}"`);
    button(q, "Cancel")?.click();
    await sleep(300);
    t(dialog("Save Symbol As"), "Cancel keeps Save Symbol As open");
  });

  await scenario("symbol: Revert takes an edit back to the file, and a symbol the design forgets opens again from its file", async (t) => {
    const id = `${SYM_B}:LsR2`;
    const sym = await symbol(id);
    await cmd({ op: "put_library_symbol", symbol: { ...sym, description: "not saved" }, overwrite: true });
    await selectItem(id);
    await E.run("common.Control.revert");
    const q = await until(() => dialog("Revert"), 4000);
    t(q && q.textContent.includes("Revert 'LsR2' to last version saved?") && q.textContent.includes("Your current changes will be permanently lost."), `the question: "${q?.textContent}"`);
    button(q, "Revert")?.click();
    t(await until(async () => (await symbol(id)).description === "Saved by the browser check", 5000), "the description is the file's again");
    // forget the design's copies (Revert's verb: the library keeps them), open from the tree: the symbol comes from the library's file
    await cmd({ op: "forget_library_symbols", lib_ids: [`${SYM_A}:LsR`, id] });
    await until(async () => !(await json("/api/symbol_editor/names")).project.includes(id), 4000);
    const r = await itemRow(id);
    t(r, "the tree lists it from the library's file");
    r?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    const back = await until(async () => { const v = await symbol(id); return v && !v.error ? v : null; }, 6000);
    t(back && back.pins.length === 2 && back.description === "Saved by the browser check", `reopened from the file: ${back && back.pins.length} pins, "${back && back.description}"`);
  });

  await scenario("symbol: Save Library As writes the library to a new file with a table row of its own", async (t) => {
    await selectLibrary(SYM_A);
    await run("eeschema.SymbolLibraryControl.saveLibraryAs");
    const d = await until(() => dialog(`Save Library '${SYM_A}' As...`), 3000);
    if (!t(d, "Save Library As opens with KiCad's title")) return;
    await until(() => d.querySelector("#sla-folder")?.value, 4000);
    t(d.querySelector("#sla-name").value === `${SYM_A}.kicad_sym`, `the file is named after the library: ${d.querySelector("#sla-name").value}`);
    await type(d.querySelector("#sla-name"), `${SYM_A}Copy.kicad_sym`);
    // the hook's four choices, "Do not update library tables" first and chosen (UI-LEFT-2 LL5); this check adds the project row
    const choices = [...d.querySelectorAll("[data-save-library-table] input")].map((i) => `${i.value}${i.checked ? "*" : ""}`);
    t(choices.join(",") === "none*,replace,global,project", `the table choices: ${choices.join(", ")}`);
    d.querySelector('[data-save-library-table] input[value="project"]')?.click();
    await sleep(100);
    button(d, "Save").click();
    t(await until(() => !dialog(`Save Library '${SYM_A}' As...`), 6000), "saved");
    const row = (await tables()).tables.symbol.project.rows.find((r) => r.nickname === `${SYM_A}Copy`);
    t(row, `the new library's row: ${JSON.stringify(row)}`);
    const items = await json(`/api/library/items?kind=symbol&lib=${SYM_A}Copy`);
    t(items.names?.includes("LsR"), `its symbols: ${JSON.stringify(items.names)}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ the Footprint Editor
  await scenario("footprint: New Library, Save As into it writes the footprint's file, an edit is saved and reverted", async (t) => {
    await clickTab("Footprint");
    if (!(await newLibrary(t, "New Footprint Library", FP_A))) return;
    // New Footprint goes into the library the tree has selected (New Library selected it), saved at once (UI-LEFT-2 LL2)
    await run("pcbnew.ModuleEditor.newFootprint");
    const names = (await json("/api/footprint_library")).project;
    const fresh = await until(async () => (await json("/api/footprint_library")).project.filter((n) => n.startsWith(`${FP_A}:Untitled`)).sort().pop(), 6000);
    if (!t(fresh, `New Footprint made ${fresh} in ${FP_A} (${JSON.stringify(names.filter((n) => /Untitled/.test(n)))})`)) return;
    t(await until(async () => savedFootprint(fresh), 6000), "and wrote it into the library's folder at once");
    await selectItem(fresh);
    await run("common.Control.saveAs");
    const d = await until(() => dialog("Save Footprint As"), 3000);
    if (!t(d, "Save Footprint As opens")) return;
    await until(() => d.querySelector(`[data-lib="${FP_A}"]`), 4000);
    await type(d.querySelector('[aria-label="Name"]'), "LsFp1");
    d.querySelector(`[data-lib="${FP_A}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    const id = `${FP_A}:LsFp1`;
    // Save As is over when its dialog is: after the file it opens the copy and closes; an action run before that is the dialog's to lose
    t(await until(() => !dialog("Save Footprint As"), 8000), "Save Footprint As closes");
    t(await until(async () => savedFootprint(id), 6000), `Save As wrote ${id} into the library's folder`);
    // an edit: modified until Save; Save writes it
    const fp = await footprint(id);
    await cmd({ op: "put_library_footprint", footprint: { ...fp, description: "Saved by the browser check" }, overwrite: true });
    t((await modified("footprint")).items.includes(id), "the edit is not saved yet");
    await itemRow(id);
    t(await until(() => treeRow(id)?.textContent.trim() === "LsFp1 *", 5000), `the tree marks it: "${treeRow(id)?.textContent.trim()}"`);
    await selectItem(id);
    const r = await run("common.Control.save");
    t(r.ok, `Save: ${JSON.stringify(r.toast ?? r.error)}`);
    t((await savedFootprint(id))?.description === "Saved by the browser check", "the file has the edit");
    // Revert: an edit not saved goes back to the file's
    const again = await footprint(id);
    await cmd({ op: "put_library_footprint", footprint: { ...again, description: "not saved" }, overwrite: true });
    await E.run("common.Control.revert");
    const q = await until(() => dialog("Revert"), 4000);
    t(q && q.textContent.includes("Revert 'LsFp1' to last version saved?"), `the question: "${q?.textContent}"`);
    button(q, "Revert")?.click();
    t(await until(async () => (await footprint(id)).description === "Saved by the browser check", 5000), "reverted to the file's");
  });

  await scenario("footprint: Save As into another library, reopened from its file, and Save Library As", async (t) => {
    if (!(await newLibrary(t, "New Footprint Library", FP_B))) return;
    const from = `${FP_A}:LsFp1`;
    await selectItem(from);
    await run("common.Control.saveAs");
    let d = await until(() => dialog("Save Footprint As"), 3000);
    await until(() => d.querySelector(`[data-lib="${FP_B}"]`), 4000);
    d.querySelector(`[data-lib="${FP_B}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    const copy = `${FP_B}:LsFp1`;
    // (the second Save As below, run while this one still opened its copy, was closed by this one's close: "Save Footprint As" never showed)
    t(await until(() => !dialog("Save Footprint As"), 8000), "Save Footprint As closes");
    t(await until(async () => savedFootprint(copy), 6000), `the copy is in ${FP_B}'s folder`);
    // the name the library's file has: the question before it is replaced
    await selectItem(from);
    await run("common.Control.saveAs");
    d = await until(() => dialog("Save Footprint As"), 3000);
    await until(() => d.querySelector(`[data-lib="${FP_B}"]`), 4000);
    d.querySelector(`[data-lib="${FP_B}"]`).click();
    await sleep(100);
    button(d, "Save").click();
    t(await until(() => dialog("Save Footprint As")?.textContent.includes(`Footprint LsFp1 already exists in ${FP_B}.`), 4000), "Footprint %s already exists in %s.");
    await closeDialogs();
    // forget the design's copy, open from the tree: it comes from the file
    await cmd({ op: "forget_library_footprints", names: [copy] });
    await until(async () => !(await json("/api/footprint_library")).project.includes(copy), 4000);
    const r = await itemRow(copy);
    t(r, "the tree lists it from the library's folder");
    r?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    const back = await until(async () => { const v = await footprint(copy); return v && !v.error ? v : null; }, 6000);
    t(back && back.description === "Saved by the browser check", `reopened from the file: "${back && back.description}"`);
    // Save Library As of the footprint library: a library row selected
    await selectLibrary(FP_A);
    await run("common.Control.saveAs");
    const la = await until(() => dialog("Save Footprint Library As"), 3000);
    if (!t(la, "Save Footprint Library As opens")) return;
    await until(() => la.querySelector("#sla-folder")?.value, 4000);
    await type(la.querySelector("#sla-name"), `${FP_A}Copy.pretty`);
    button(la, "Save").click();
    t(await until(() => !dialog("Save Footprint Library As"), 6000), "saved");
    const items = await json(`/api/library/items?kind=footprint&lib=${FP_A}Copy`);
    t(items.names?.includes("LsFp1"), `the copy's footprints: ${JSON.stringify(items.names)}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ clean up: the check's rows out of the board's tables
  for (const kind of ["symbol", "footprint"]) {
    const v = await tables();
    const rows = v.tables[kind].project.rows.filter((r) => !/^Ls(Sym|Fp)/.test(r.nickname));
    await post("/api/library_tables", { kind, rows });
  }

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
