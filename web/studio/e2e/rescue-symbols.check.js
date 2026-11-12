// Rescue Symbols, Remap Legacy Library Symbols and Select Columns check (UI-LEFT A2, GAPS.md item 5).
//
// What it checks:
//   Select Columns in the Symbol Chooser (`LIB_TREE::onHeaderContextMenu`): a right click on the tree's header offers "Select Columns..."; the dialog
//     ("Select Columns", Available: / Enabled:) lists Keywords once Device is read; `>` puts it after the selected enabled column, OK shows it as a column,
//     whose cell for Device:R is the symbol's keywords; the column is taken out again the same way;
//   Select Columns in the Symbol Editor: the action (`common.Control.selectColumns`) opens its tree's dialog, which starts on Item and Description, and the
//     tree's header shows those two;
//   Rescue Symbols over a library that went away: a library the check makes through the routes (New Library, a symbol saved into its file), a symbol of it
//     placed (its definition kept with the schematic), then the library's row taken out of the table: Tools > Rescue Symbols... opens "Project Rescue Helper"
//     with the candidate, KiCad's words ("Rescue symbol RescueLib:RescueRes found only in cache library to <board>-rescue:RescueRes-RescueLib."), the
//     instances ("Instances of this symbol (1 items):"); Rescue Symbols writes `<board>-rescue.kicad_sym`, puts its row in the project table and relinks
//     the symbol in one undo step; run again it says "This project has nothing to rescue.";
//   Remap Legacy Library Symbols on a schematic whose symbols name libraries: the dialog says it is for legacy projects and its button is disabled.
// The rescue library and the library the check makes stay in the board's folder (their rows are taken out of the table); every design edit is undone.
// Rescue reads the project's legacy cache library only (KiCad's): the check writes one through the runner's `window.__runCheckWriteFile`, so run it through e2e/run-check.mjs.
// It answers through `window.__rescueSymbolsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__rescueSymbolsCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };
  const E = window.__eda;
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const run = (id, args) => E.run(id, args);
  const sch = () => json("/api/schematic");
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const undo = async (domain) => { await post("/api/undo", { domain }); await sleep(300); };
  const rightClick = (el) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 5, button: 2 })); };
  const menuItem = (label) => [...document.querySelectorAll("[role=menuitem], .context-menu button, .context-menu div")].find((e) => e.textContent.trim() === label);
  const pick = (list, text) => [...(list?.querySelectorAll("[role=option]") ?? [])].find((o) => o.textContent.trim() === text);
  const headerColumns = (root) => [...(root?.querySelectorAll("[data-column]") ?? [])].filter((e) => e.closest("[data-chooser-header], [data-library-tree-header]")).map((e) => e.dataset.column);
  const closeChoosers = async () => {
    for (let i = 0; i < 4; i++) {
      const c = document.querySelector("[data-chooser-cancel]");
      if (!c) break;
      c.click();
      await sleep(300);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeChoosers(); } catch { /* ignore */ }
    results.push(entry);
  };
  const errorsBefore = E.errors().length;
  const LIB = "RescueLib";
  const NAME = "RescueRes";
  const board = (await json("/api/state")).name;
  const tablesBefore = await json("/api/library_tables");

  // ----------------------------------------------------------------------------------------------------------- Select Columns
  await scenario("Select Columns in the Symbol Chooser: the header's menu, the dialog, Keywords shown and taken out", async (t) => {
    await clickTab("Schematic");
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 6000);
    if (!t(root, "A opens the Symbol Chooser")) return;
    setValue(root.querySelector("#library-tree-search"), "Device:R");
    // the Device library's own row (the columns it offers come with its items, `addColumnIfNecessary`), not an "-- Already Placed --" one
    const row = await until(() => root.querySelector(`[data-chooser-tree] [data-key="i:Device\u0001Device:R"]`), 90000);
    if (!t(row, "the search lists Device:R from the Device library")) return;
    const header = root.querySelector("[data-chooser-header]");
    t(JSON.stringify(headerColumns(root)) === JSON.stringify(["Item", "Description"]), `the columns at first: ${headerColumns(root).join(", ")}`);
    rightClick(header);
    const entry = await until(() => menuItem("Select Columns..."), 3000);
    if (!t(entry, "a right click on the header offers Select Columns...")) return;
    entry.click();
    const dlg = await until(() => document.querySelector("[data-select-columns-dialog]"), 3000);
    if (!t(dlg, "the Select Columns dialog opens")) return;
    t(/Select Columns/.test(dlg.textContent) && /Available:/.test(dlg.textContent) && /Enabled:/.test(dlg.textContent), "its title and two lists");
    const avail = dlg.querySelector("[data-columns-available]");
    const kw = pick(avail, "Keywords");
    if (!t(kw, `Keywords is available: ${[...avail.querySelectorAll("[role=option]")].map((o) => o.textContent).join(", ")}`)) return;
    kw.click();
    await sleep(100);
    dlg.querySelector("[data-columns-add]").click();
    await sleep(100);
    const enabled = [...dlg.querySelectorAll("[data-columns-enabled] [role=option]")].map((o) => o.textContent);
    t(JSON.stringify(enabled) === JSON.stringify(["Item", "Keywords", "Description"]), `> puts it after the selected enabled column (Item): ${enabled.join(", ")}`);
    dlg.querySelector("[data-columns-ok]").click();
    await sleep(300);
    t(JSON.stringify(headerColumns(root)) === JSON.stringify(["Item", "Keywords", "Description"]), `the tree shows it: ${headerColumns(root).join(", ")}`);
    // the Device library's row (an "-- Already Placed --" row of the same symbol comes from the design, whose entries carry no fields)
    const cell = root.querySelector(`[data-chooser-tree] [data-key="i:Device\u0001Device:R"] [data-column="Keywords"]`);
    t(cell && /resistor/i.test(cell.textContent), `Device:R's Keywords cell: "${cell?.textContent ?? ""}"`);
    // and out again
    rightClick(root.querySelector("[data-chooser-header]"));
    (await until(() => menuItem("Select Columns..."), 3000))?.click();
    const dlg2 = await until(() => document.querySelector("[data-select-columns-dialog]"), 3000);
    pick(dlg2?.querySelector("[data-columns-enabled]"), "Keywords")?.click();
    await sleep(100);
    dlg2?.querySelector("[data-columns-remove]")?.click();
    await sleep(100);
    dlg2?.querySelector("[data-columns-ok]")?.click();
    await sleep(300);
    t(JSON.stringify(headerColumns(root)) === JSON.stringify(["Item", "Description"]), `< takes it out: ${headerColumns(root).join(", ")}`);
    await closeChoosers();
    await run("common.Interactive.cancel");
  });

  await scenario("Select Columns in the Symbol Editor: the action opens its tree's dialog on Item and Description", async (t) => {
    await clickTab("Symbol");
    await until(() => E.state().tab === "symbol", 4000);
    const header = await until(() => document.querySelector('[data-library-tree-header="symbol"]'), 8000);
    if (!t(header, "the Symbol Editor's tree has a column header")) return;
    t(JSON.stringify(headerColumns(header.parentElement)) === JSON.stringify(["Item", "Description"]), `its columns: ${headerColumns(header.parentElement).join(", ")}`);
    await run("common.Control.selectColumns");
    const dlg = await until(() => document.querySelector("[data-select-columns-dialog]"), 3000);
    if (!t(dlg, "common.Control.selectColumns opens the Select Columns dialog")) return;
    const enabled = [...dlg.querySelectorAll("[data-columns-enabled] [role=option]")].map((o) => o.textContent);
    t(JSON.stringify(enabled) === JSON.stringify(["Item", "Description"]), `enabled: ${enabled.join(", ")}`);
    // Item, the first, is not removed (`onRemoveItem`: `info.m_itemId == 0`)
    pick(dlg.querySelector("[data-columns-enabled]"), "Item")?.click();
    await sleep(100);
    dlg.querySelector("[data-columns-remove]").click();
    await sleep(100);
    t([...dlg.querySelectorAll("[data-columns-enabled] [role=option]")].some((o) => o.textContent === "Item"), "Item stays");
    [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(200);
    t(!document.querySelector("[data-select-columns-dialog]"), "Cancel closes it");
    await clickTab("Schematic");
  });

  // ----------------------------------------------------------------------------------------------------------- Rescue Symbols
  let made = false;
  let placed = false;
  await scenario("Rescue Symbols: a symbol whose library went away is rescued into the project's rescue library in one undo step", async (t) => {
    await clickTab("Schematic");
    const add = await post("/api/library_tables/add", { kind: "symbol", path: `${LIB}.kicad_sym`, create: true });
    if (!t(!add.error, `New Library ${LIB}: ${add.error ?? "made"}`)) return;
    made = true;
    const base = (await json("/api/library/symbol?lib_id=Device:R")).symbol;
    if (!t(base, "Device:R read as the template")) return;
    const put = await cmd({ op: "put_library_symbol", symbol: { ...base, lib_id: `${LIB}:${NAME}`, description: "Rescue test resistor" }, overwrite: true });
    t(put.ok !== false, "the symbol is in the design's buffer of the library");
    const saved = await post("/api/library/save", { kind: "symbol", lib: LIB });
    t(saved.ok, `saved into the file: ${saved.message ?? saved.error ?? "ok"}`);
    const placedReply = await cmd({ op: "add_symbol", id: "R901", lib_id: `${LIB}:${NAME}`, at: { x: 152400000, y: 101600000 }, rot_millideg: 0, value: "1k", footprint: "", unit: 1 });
    placed = placedReply.ok !== false;
    if (!t(placed, `R901 placed from ${LIB}: ${JSON.stringify(placedReply).slice(0, 200)}`)) return;
    // the library goes away: its row out of the project table
    const rows = (await json("/api/library_tables")).tables.symbol.project.rows.filter((r) => r.nickname !== LIB);
    const out = await post("/api/library_tables", { kind: "symbol", rows });
    t(!out.error, `${LIB}'s row out of the table: ${out.error ?? "ok"}`);
    // KiCad's rescuer reads the project's legacy cache library only, never the schematic's own copies (UI-LEFT-2 LL7): without one, nothing of ours to rescue
    const firstLook = await json("/api/sch/rescue");
    t(!(firstLook.candidates ?? []).some((c) => c.requested === `${LIB}:${NAME}`), `no cache library: ${LIB}:${NAME} is no candidate`);
    // the cache library a KiCad 5 project kept beside it (`<project>-cache.lib`, the `Lib-Name` names `findSymbol` looks up second)
    const projectDir = (await json("/api/library_tables")).project_dir;
    const cacheLib = `EESchema-LIBRARY Version 2.4\n#encoding utf-8\n#\n# ${LIB}-${NAME}\n#\nDEF ${LIB}-${NAME} R 0 0 N Y 1 F N\nF0 "R" 80 0 50 V V C CNN\nF1 "${LIB}-${NAME}" 0 0 50 V V C CNN\nF2 "" -70 0 50 V I C CNN\nF3 "" 0 0 50 H I C CNN\nDRAW\nS -40 -100 40 100 0 1 10 N\nX ~ 1 0 150 50 D 50 50 1 1 P\nX ~ 2 0 -150 50 U 50 50 1 1 P\nENDDRAW\nENDDEF\n#\n#End Library\n`;
    const wrote = window.__runCheckWriteFile ? await window.__runCheckWriteFile(`${projectDir}/${board}-cache.lib`, cacheLib) : "no __runCheckWriteFile: run through e2e/run-check.mjs";
    if (!t(wrote === "ok", `the project's cache library written: ${wrote}`)) return;

    await run("eeschema.EditorControl.rescueSymbols");
    const dlg = await until(() => document.querySelector("[data-rescue-dialog] [data-rescue-candidates]")?.closest(".dialog"), 20000);
    if (!t(dlg, "Tools > Rescue Symbols... opens the Project Rescue Helper with candidates")) return;
    t(/Project Rescue Helper/.test(dlg.textContent) && /Symbols to update:/.test(dlg.textContent), "its title and list");
    const row = dlg.querySelector(`[data-requested="${LIB}:${NAME}"]`);
    const words = `Rescue symbol ${LIB}:${NAME} found only in cache library to ${board}-rescue:${NAME}-${LIB}.`;
    t(row && row.textContent.includes(words), `the candidate and KiCad's words: "${row?.textContent ?? ""}" (wanted "${words}")`);
    row?.click();
    await sleep(200);
    const title = dlg.querySelector("[data-rescue-instances-title]")?.textContent ?? "";
    const inst = [...dlg.querySelectorAll("[data-rescue-instances] tbody tr")].map((r) => r.textContent);
    t(title === "Instances of this symbol (1 items):" && inst.some((x) => x.includes("R901") && x.includes("1k")), `its instances: ${title} ${inst.join(" | ")}`);
    t(/Cached Symbol:/.test(dlg.textContent) && /Library Symbol:/.test(dlg.textContent), "the two previews' labels");
    // every candidate other than ours left out, so the check rescues only what it made
    const others = [...dlg.querySelectorAll("[data-requested]")].map((r) => r.dataset.requested).filter((r) => r !== `${LIB}:${NAME}`);
    for (const other of dlg.querySelectorAll("[data-requested]")) {
      if (other.dataset.requested === `${LIB}:${NAME}`) continue;
      const box = other.querySelector("input[type=checkbox]");
      if (box?.checked) box.click();
    }
    await sleep(100);
    dlg.querySelector("[data-rescue-ok]").click();
    const relinked = await until(async () => (await sch()).symbols.find((s) => s.id === "R901" && s.lib_id === `${board}-rescue:${NAME}-${LIB}`), 15000);
    t(relinked, `R901 links to ${board}-rescue:${NAME}-${LIB}`);
    t(await until(() => !document.querySelector("[data-rescue-dialog]"), 8000), "the dialog closes");
    const tables = await json("/api/library_tables");
    const rescueRow = tables.tables.symbol.project.rows.find((r) => r.nickname === `${board}-rescue`);
    t(rescueRow && rescueRow.uri === `\${KIPRJMOD}/${board}-rescue.kicad_sym`, `the rescue library's row: ${JSON.stringify(rescueRow)}`);
    // run again: nothing to rescue (R901 now names the rescue library, which has it as the schematic has it)
    const again = await run("eeschema.EditorControl.rescueSymbols");
    if (others.length === 0) {
      const said = /This project has nothing to rescue\./.test(again?.toast?.message ?? "") || (await until(() => document.body.innerText.includes("This project has nothing to rescue."), 15000));
      t(said, `run again: "This project has nothing to rescue." (${JSON.stringify(again?.toast ?? null)})`);
      t(!document.querySelector("[data-rescue-dialog]"), "and no dialog stays open");
    } else {
      // the board has candidates of its own (kept copies that differ from the installed libraries): ours is not among them any more
      const dlg2 = await until(() => document.querySelector("[data-rescue-dialog] [data-rescue-candidates]")?.closest(".dialog"), 20000);
      const left = [...(dlg2?.querySelectorAll("[data-requested]") ?? [])].map((r) => r.dataset.requested);
      t(dlg2 && !left.some((r) => r.startsWith(`${LIB}:`) || r.startsWith(`${board}-rescue:`)), `run again: the board's own candidates only (${others.join(", ")}): ${left.join(", ")}`);
      dlg2?.querySelector("[data-rescue-skip]")?.click();
      await sleep(300);
      t(!document.querySelector("[data-rescue-dialog]"), "Skip Symbol Rescue closes it");
    }
    // one undo step back to the old link
    await undo("schematic");
    const back = (await sch()).symbols.find((s) => s.id === "R901");
    t(back && back.lib_id === `${LIB}:${NAME}`, `one undo step puts the old link back: ${back?.lib_id}`);
  });

  await scenario("Remap Legacy Library Symbols: only for a schematic none of whose symbols names a library", async (t) => {
    // `remapSymbolsCondition` (UI-LEFT-2 LL7): the menu entry is disabled on a schematic whose symbols name their libraries
    t((await json("/api/sch/legacy")).legacy === false, "the server says the schematic is not a legacy one");
    await sleep(800);
    const r = await run("eeschema.EditorControl.remapSymbols");
    t(r && r.ok === false && !document.querySelector("[data-remap-dialog]"), `Tools > Remap Legacy Library Symbols... is disabled: ${JSON.stringify(r).slice(0, 160)}`);
  });

  // ----------------------------------------------------------------------------------------------------------- clean up
  await scenario("clean up: the design as it was, the rows the check added out of the table, no page error", async (t) => {
    if (placed) await undo("schematic"); // the placement of R901
    if (made) await cmd({ op: "delete_library_symbol", lib_id: `${LIB}:${NAME}` });
    const rows = (await json("/api/library_tables")).tables.symbol.project.rows.filter((r) => r.nickname !== LIB && r.nickname !== `${board}-rescue`);
    const back = await post("/api/library_tables", { kind: "symbol", rows });
    t(!back.error, `the check's rows out of the table: ${back.error ?? "ok"}`);
    const now = await json("/api/library_tables");
    t(now.tables.symbol.project.rows.length === tablesBefore.tables.symbol.project.rows.length, `the project table has the rows it had: ${now.tables.symbol.project.rows.length}`);
    t(!(await sch()).symbols.some((s) => s.id === "R901"), "R901 is gone again");
    const errs = E.errors().slice(errorsBefore);
    t(errs.length === 0, `no page error: ${errs.map((e) => e.message ?? e).join(" | ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return check.ok;
})();
