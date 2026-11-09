// Delete, Rename, New Footprint and the file watch of the libraries of the library tables (UI-LEFT-2 LL1, LL2, LL3, LL6; PARITY-common.md section 16, PARITY-fpedit
// section 19): a symbol deleted and another renamed in a library of the project table leave its file when the library is saved; a new footprint goes into the
// library the tree has selected and is written at once; the loaded footprint renamed and saved is written under its new name and its old file goes; a deleted
// footprint's file goes at once; a library file changed on disk by something else than the studio reloads the item that is not modified, and asks KiCad's
// question for one that is.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs`, whose `window.__runCheckWriteFile` changes a library
// file the way another program would -- on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui
// web/studio/dist`). It makes libraries of its own in the board's folder (nicknames starting with `Lb`), takes their rows out of the board's tables at the end
// and leaves their files. It answers through `window.__libraryBufferCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__libraryBufferCheck = { results, done: false, ok: false, wait: null });
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
  const until = async (cond, ms = 8000, step = 150) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(600); };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const libRow = (lib) => document.querySelector(`.library-tree-group[data-library="${CSS.escape(lib)}"]`);
  const modified = (kind) => json(`/api/library/modified?kind=${kind}`);
  const items = async (kind, lib) => ((await json(`/api/library/items?kind=${kind}&lib=${encodeURIComponent(lib)}`)).names ?? []).slice().sort();
  const libPath = async (kind, lib) => (await json(`/api/library/writable?kind=${kind}`)).libraries.find((l) => l.name === lib)?.path;
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const footprint = (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const tables = () => json("/api/library_tables");
  const statusText = () => document.querySelector('[data-testid="library-status-text"]')?.textContent ?? "";
  const writeOutside = (path, text) => (window.__runCheckWriteFile ? window.__runCheckWriteFile(path, text) : Promise.resolve("no __runCheckWriteFile: run through e2e/run-check.mjs"));
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close|No)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  const selectLibrary = async (lib) => {
    const r = await until(() => libRow(lib), 6000);
    if (!r) return false;
    r.click();
    await sleep(250);
    if (!isOpen(lib)) libRow(lib)?.click();
    await sleep(250);
    return true;
  };
  const itemRow = async (id) => {
    const lib = id.split(":")[0];
    if (await until(() => libRow(lib), 6000)) if (!isOpen(lib)) { libRow(lib).click(); await sleep(250); }
    return until(() => treeRow(id), 6000);
  };
  const selectItem = async (id) => { const r = await itemRow(id); r?.click(); await sleep(300); return !!r; };
  const newLibrary = async (t, title, name) => {
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog(title), 4000);
    if (!t(d, `${title} opens`)) return false;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), name);
    button(d, "Create").click();
    return !!t(await until(() => !dialog(title), 6000), `${title}: ${name} made and put in the project's table`);
  };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };
  /** The prompt of Rename ("Change Symbol Name" / "Change Footprint Name"): the new name typed and Rename clicked. */
  const answerRename = async (title, name) => {
    const d = await until(() => dialog(title), 4000);
    if (!d) return false;
    await type(d.querySelector("input"), name);
    button(d, "Rename")?.click();
    await until(() => !dialog(title), 4000);
    return true;
  };
  const newSymbol = async (t, name) => {
    await run("eeschema.SymbolLibraryControl.newSymbol");
    const nd = await until(() => document.querySelector('[data-testid="new-symbol"]'), 3000);
    if (!t(nd, `New Symbol opens for ${name}`)) return;
    await type(document.querySelector('[data-testid="new-symbol-name"]'), name);
    document.querySelector('[data-testid="new-symbol-ok"]').click();
    await sleep(600);
  };

  const s = Date.now().toString(36);
  const SYM = `LbSym${s}`, FP = `LbFp${s}`;
  const symText = (name, descr) =>
    `(kicad_symbol_lib (version 20251024) (generator "check")\n\t(symbol "${name}" (property "Reference" "U" (at 0 0 0)) (property "Value" "${name}" (at 0 0 0)) (property "Description" "${descr}" (at 0 0 0))\n\t\t(symbol "${name}_1_1" (pin passive line (at 0 0 0) (length 2.54) (name "~") (number "1"))))\n)\n`;
  const fpText = (name, descr) => `(footprint "${name}" (version 20240108) (generator "check") (layer "F.Cu") (descr "${descr}")\n\t(attr smd)\n\t(pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))\n)\n`;

  // ------------------------------------------------------------------------------------------------------------------------ the Symbol Editor
  await scenario("symbol: a deleted symbol is no longer listed, its library is marked, and Save takes it out of the file", async (t) => {
    await clickTab("Symbol");
    if (!(await newLibrary(t, "New Symbol Library", SYM))) return;
    t(await selectLibrary(SYM), "the new library is in the tree");
    await newSymbol(t, "LbA");
    await selectLibrary(SYM);
    await newSymbol(t, "LbB");
    await selectLibrary(SYM);
    await run("common.Control.save");
    t(JSON.stringify(await items("symbol", SYM)) === JSON.stringify(["LbA", "LbB"]), `saved: ${JSON.stringify(await items("symbol", SYM))}`);
    // Delete: no question for a symbol that is saved and derives nothing
    await selectItem(`${SYM}:LbB`);
    await run("eeschema.SymbolLibraryControl.deleteSymbol");
    await sleep(500);
    t(!document.querySelector(".dialog-backdrop"), "no question for a saved plain symbol");
    t(await until(() => !treeRow(`${SYM}:LbB`), 5000), "the tree no longer lists it");
    t(JSON.stringify(await items("symbol", SYM)) === JSON.stringify(["LbA", "LbB"]), "the file keeps it until Save");
    t((await modified("symbol")).libraries.includes(SYM), "the library has something to save");
    t(await until(() => / \*$/.test(libRow(SYM)?.textContent.trim() ?? "") || /\*/.test(libRow(SYM)?.textContent ?? ""), 4000), `the library row is marked: "${libRow(SYM)?.textContent.trim()}"`);
    await selectLibrary(SYM);
    const r = await run("common.Control.save");
    t(r.ok, `Save: ${JSON.stringify(r).slice(0, 160)}`);
    t(await until(async () => JSON.stringify(await items("symbol", SYM)) === JSON.stringify(["LbA"]), 5000), `the file lacks it: ${JSON.stringify(await items("symbol", SYM))}`);
    t(!(await modified("symbol")).libraries.includes(SYM), "nothing left to save");
  });

  await scenario("symbol: a renamed symbol is written under its new name, and the old name leaves the file", async (t) => {
    await selectItem(`${SYM}:LbA`);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    E.run("eeschema.SymbolLibraryControl.renameFootprint"); // KiCad names the Symbol Editor's Rename action so
    t(await answerRename("Change Symbol Name", "LbA2"), "Change Symbol Name asks for the name");
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(`${SYM}:LbA2`), 5000), "the symbol is LbA2 in the library's buffer");
    t(await until(() => E.state().tab === "symbol" && treeRow(`${SYM}:LbA2`) && !treeRow(`${SYM}:LbA`), 5000), "the tree lists the new name only");
    await selectLibrary(SYM);
    await run("common.Control.save");
    t(await until(async () => JSON.stringify(await items("symbol", SYM)) === JSON.stringify(["LbA2"]), 5000), `the file: ${JSON.stringify(await items("symbol", SYM))}`);
  });

  await scenario("symbol: a library changed on disk reloads the open symbol that is not modified, and asks for one that is", async (t) => {
    const id = `${SYM}:LbA2`;
    await selectItem(id);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1500); // the watch takes the file's time
    const path = await libPath("symbol", SYM);
    let w = await writeOutside(path, symText("LbA2", "changed outside"));
    if (!t(w === "ok", `the file is changed outside the studio (${w})`)) return;
    t(await until(async () => (await symbol(id)).description === "changed outside", 8000), "not modified: reloaded unasked");
    t(!dialog("Confirmation"), "no question");
    // an edit, then the file changes again: KiCad's question; No keeps the edit
    const sym = await symbol(id);
    await cmd({ op: "put_library_symbol", symbol: { ...sym, description: "edited here" }, overwrite: true });
    await sleep(1500);
    w = await writeOutside(path, symText("LbA2", "changed outside again"));
    const q = await until(() => dialog("Confirmation"), 8000);
    t(q && q.textContent.includes("The library containing the current symbol has changed.") && q.textContent.includes("Do you want to reload the library?"), `the question: "${q?.textContent}"`);
    button(q, "No")?.click();
    await sleep(800);
    t((await symbol(id)).description === "edited here", "No keeps the edit");
    // the next change asks again; Yes reloads
    w = await writeOutside(path, symText("LbA2", "changed a third time"));
    const q2 = await until(() => dialog("Confirmation"), 8000);
    t(q2, "a later change asks again");
    button(q2, "Yes")?.click();
    t(await until(async () => (await symbol(id)).description === "changed a third time", 6000), "Yes reloads the library");
    t(!(await modified("symbol")).items.includes(id), "reloaded: not modified");
  });

  // ------------------------------------------------------------------------------------------------------------------------ the Footprint Editor
  await scenario("footprint: New Footprint goes into the library the tree has selected and is written into its folder at once", async (t) => {
    await clickTab("Footprint");
    if (!(await newLibrary(t, "New Footprint Library", FP))) return;
    await selectLibrary(FP);
    await run("pcbnew.ModuleEditor.newFootprint");
    const id = `${FP}:Untitled`;
    t(await until(() => E.state().footprint?.name === id || document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`), 6000), `the editor has ${id}`);
    t(await until(async () => (await items("footprint", FP)).includes("Untitled"), 6000), `Untitled.kicad_mod is in ${FP}: ${JSON.stringify(await items("footprint", FP))}`);
    t(!(await modified("footprint")).items.includes(id), "saved: not modified");
  });

  await scenario("footprint: the loaded footprint renamed and saved is written under its new name and its old file goes", async (t) => {
    const id = `${FP}:Untitled`;
    await selectItem(id);
    E.run("pcbnew.ModuleEditor.renameFootprint");
    t(await answerRename("Change Footprint Name", "LbChip"), "Change Footprint Name asks for the name");
    const renamed = `${FP}:LbChip`;
    t(await until(async () => (await json("/api/footprint_library")).project.includes(renamed), 5000), "renamed in the design");
    t((await items("footprint", FP)).includes("Untitled"), "the folder keeps the old file until Save");
    t((await modified("footprint")).items.includes(renamed), "the renamed footprint is modified");
    await selectItem(renamed);
    const r = await run("common.Control.save");
    t(r.ok, `Save: ${JSON.stringify(r).slice(0, 160)}`);
    t(await until(async () => JSON.stringify(await items("footprint", FP)) === JSON.stringify(["LbChip"]), 5000), `the folder: ${JSON.stringify(await items("footprint", FP))}`);
  });

  await scenario("footprint: the loaded footprint's file changed on disk reloads it unasked when it is not modified", async (t) => {
    const id = `${FP}:LbChip`;
    await sleep(1500);
    const path = `${await libPath("footprint", FP)}/LbChip.kicad_mod`;
    const w = await writeOutside(path, fpText("LbChip", "changed outside"));
    if (!t(w === "ok", `the file is changed outside the studio (${w})`)) return;
    t(await until(async () => (await footprint(id)).description === "changed outside", 8000), "reloaded");
    t(!dialog("Confirmation"), "no question");
  });

  await scenario("footprint: Delete asks KiCad's question and deletes the file at once, saying so in the status bar", async (t) => {
    await selectLibrary(FP);
    await run("pcbnew.ModuleEditor.newFootprint");
    const id = `${FP}:Untitled`;
    t(await until(async () => (await items("footprint", FP)).includes("Untitled"), 6000), "a second footprint, saved");
    await selectItem(id);
    E.run("pcbnew.ModuleEditor.deleteFootprint");
    const q = await until(() => dialog("Confirmation"), 4000);
    t(q && q.textContent.includes(`Delete footprint 'Untitled' from library '${FP}'?`), `the question: "${q?.textContent}"`);
    button(q, "Yes")?.click();
    t(await until(async () => !(await items("footprint", FP)).includes("Untitled"), 6000), `the file went: ${JSON.stringify(await items("footprint", FP))}`);
    t(await until(() => statusText() === `Footprint 'Untitled' deleted from library '${FP}'`, 4000), `the status bar: "${statusText()}"`);
    t(!(await modified("footprint")).libraries.includes(FP), "nothing left to save");
  });

  // ------------------------------------------------------------------------------------------------------------------------ clean up: the check's rows out of the board's tables
  for (const kind of ["symbol", "footprint"]) {
    const v = await tables();
    const rows = v.tables[kind].project.rows.filter((r) => !/^Lb(Sym|Fp)/.test(r.nickname));
    await post("/api/library_tables", { kind, rows });
  }

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
