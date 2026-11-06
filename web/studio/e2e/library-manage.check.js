// Manage Libraries check (UI-LEFT L2, PARITY-common section 14): the global library table edited and reset, Edit Options..., a library of another format read through
// kicad-cli, Migrate Libraries, and KiCad's path variables in Configure Paths -- driven through `window.__eda` and the dialogs' own controls, checked against what the
// server then has.
// Run it on a COPY of a board served with a KiCad configuration folder of its own (`EDA_KICAD_CONFIG_DIR` at an empty folder: the check writes the global tables and
// `kicad_common.json` there) and with KiCad's QA libraries `legacy.lib` (eeschema/netlists/legacy_power4/symbols.lib) and `gps.lbr`
// (pcbnew/plugins/eagle/lbr/SparkFun-GPS.lbr) copied into the board's folder; the header below makes tools/run-all-checks.sh do both. It refuses to edit a global
// table that already holds libraries of someone's (anything but one row), so a person's own KiCad configuration is never touched.
//
// What it checks:
//   the Global Libraries tab is editable, OK writes the global table into KiCad's configuration folder and the library index follows it;
//   Reset Libraries asks KiCad's question and writes one row naming the table KiCad installs, at once (Cancel does not take it back);
//   Edit Options... (the row menu and a double click on the Options cell): the format's choices, Append Selected Option, OK writes the row's options;
//   an Eagle footprint library and an Eagle symbol library of the project table are read through kicad-cli: listed, their items found, read only;
//   Migrate Libraries: KiCad's message for rows of KiCad's format, its question, the legacy file converted beside it and the row pointed at the .kicad_sym;
//   Configure Paths: a KiCad variable added is written to kicad_common.json and becomes a 3D model search path, and goes again.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }; the results also pile up in `window.__libraryManageCheck.results`.
// run-all-checks: board=mcu30 kicad_config=fresh fixtures=library-formats
(async () => {
  const results = [];
  window.__libraryManageCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const setNative = (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogs = () => [...document.querySelectorAll(".dialog-backdrop .dialog")];
  const top = () => dialogs()[dialogs().length - 1];
  const button = (label, scope = top()) => [...(scope ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(60); } };
  const row = (i, col) => document.querySelector(`.lt-grid [data-row="${i}"][data-col="${col}"]`);
  const tabByName = (name) => [...document.querySelectorAll(".lt-tab")].find((t) => t.textContent.trim().startsWith(name));
  const banner = () => [...(document.querySelectorAll(".lt-dialog .lt-banner"))].map((b) => b.textContent).join(" | ");
  const question = () => document.querySelector('[role="alertdialog"]');
  const answer = async (label) => { const q = await waitFor(question, 8000); const b = q && button(label, q); b?.click(); await sleep(400); return q?.querySelector(".lt-question")?.textContent ?? ""; };
  const tables = () => get("/api/library_tables");
  const gridNames = () => [...document.querySelectorAll('.lt-grid [data-col="nickname"]')].map((i) => i.value);
  const selectRow = async (i) => { row(i, "nickname").closest("tr").dispatchEvent(new MouseEvent("mousedown", { bubbles: true })); await sleep(120); };
  const addRow = async (nick, uri, type) => {
    top().querySelector('[aria-label="Add empty row to table"]').click();
    await sleep(150);
    const at = gridNames().length - 1;
    setNative(row(at, "nickname"), nick);
    setNative(row(at, "uri"), uri);
    if (type) setNative(top().querySelector(`[aria-label="type of row ${at + 1}"]`), type);
    await sleep(100);
    return at;
  };
  const openTables = async (kind, tab) => {
    await E.run(kind === "symbol" ? "common.SuiteControl.showSymbolLibTable" : "common.SuiteControl.showFootprintLibTable");
    await waitFor(() => document.querySelector(".lt-grid") && tabByName(tab));
    tabByName(tab).click();
    await sleep(400);
  };
  const ok = async () => { button("OK", document.querySelector(".lt-dialog"))?.click(); await sleep(1500); };
  const removeMine = async (kind) => {
    const v = await tables();
    if (!v.tables[kind].project.rows.some((r) => /^Lt/.test(r.nickname))) return;
    await openTables(kind, "Project Specific Libraries");
    for (let guard = 0; guard < 12; guard++) {
      const at = gridNames().findIndex((n) => /^Lt/.test(n));
      if (at < 0) break;
      await selectRow(at);
      top().querySelector('[aria-label="Remove library from table"]').click();
      await sleep(150);
    }
    await ok();
  };
  const errorsBefore = E.errors().length;
  try {
    let lt = await tables();
    const dir = lt.project_dir;
    const sym = lt.tables.symbol;
    // ============================================================================================================================ guard: never a person's own table
    const own = (k) => lt.tables[k].global.origin === "config" && lt.tables[k].global.rows.filter((r) => !/^Lt/.test(r.nickname)).length > 1;
    if (!check("the server has a KiCad configuration folder of the check's own (kicad_config=fresh)", lt.config_dir && !own("symbol") && !own("footprint"), `${lt.config_dir}: ${sym.global.origin}, ${sym.global.rows.length} rows`)) throw new Error("not on a configuration folder of the check's own");

    // ============================================================================================================================ the global table, edited
    check("the server says the global table can be written, and where", sym.global.writable === true && typeof sym.global.target === "string" && sym.global.target.endsWith("sym-lib-table"), JSON.stringify([sym.global.writable, sym.global.target]));
    await openTables("symbol", "Global Libraries");
    const cells = [...document.querySelectorAll(".lt-grid input[type=text]")];
    check("the Global tab's cells are editable", cells.length > 0 && cells.every((i) => !i.readOnly), cells.length);
    check("it says OK writes the global table", /OK writes/.test(document.querySelector(".lt-dialog .lt-note")?.textContent ?? ""), document.querySelector(".lt-dialog .lt-note")?.textContent);
    check("Reset Libraries is offered on the Global tab", !!button("Reset Libraries") && !button("Reset Libraries").disabled);
    // a library of the global table (one its chained table names when it holds only the chain): the new row names it again
    const libOf = (rows) => { for (const r of rows) { if (r.type !== "Table") return r.uri; const inner = r.table && libOf(r.table.rows); if (inner) return inner; } return null; };
    const firstUri = libOf(sym.global.rows) ?? "";
    const g = await addRow("LtGlobal", firstUri, "KiCad");
    await ok();
    lt = await tables();
    const gRows = lt.tables.symbol.global.rows;
    check("OK writes the global table into KiCad's configuration folder", lt.tables.symbol.global.origin === "config" && gRows.some((r) => r.nickname === "LtGlobal" && r.uri === firstUri), JSON.stringify(gRows.slice(-2)));
    check("and leaves the project's table alone", !lt.tables.symbol.project.rows.some((r) => r.nickname === "LtGlobal"));
    const idx = await get("/api/library/index?kind=symbol");
    check("the library index lists the new global library", idx.libraries.some((l) => l.name === "LtGlobal" && l.scope === "global"), g);

    // ============================================================================================================================ Reset Libraries
    await openTables("symbol", "Global Libraries");
    button("Reset Libraries").click();
    const asked = await answer("Yes");
    check("Reset asks KiCad's question", asked === "This action will reset your global library table on disk and cannot be undone.", asked);
    await sleep(800);
    const after = gridNames();
    check("the Global tab shows the one row Reset writes", after.length === 1 && after[0] === "KiCad" && top().querySelector('[aria-label="type of row 1"]').value === "Table", JSON.stringify(after));
    button("Cancel", document.querySelector(".lt-dialog")).click();
    await sleep(500);
    lt = await tables();
    const reset = lt.tables.symbol.global.rows;
    check("it is on disk at once, Cancel or not: KiCad, Table, KiCad Default Libraries, the installed table", reset.length === 1 && reset[0].nickname === "KiCad" && reset[0].type === "Table" && reset[0].descr === "KiCad Default Libraries" && reset[0].uri === lt.tables.symbol.global.stock, JSON.stringify(reset));
    const idx2 = await get("/api/library/index?kind=symbol");
    check("the libraries come through the chained table again, LtGlobal gone", !idx2.libraries.some((l) => l.name === "LtGlobal") && idx2.libraries.filter((l) => l.scope === "global").length > 10, idx2.libraries.length);
    await openTables("footprint", "Global Libraries");
    button("Reset Libraries").click();
    await answer("Yes");
    button("Cancel", document.querySelector(".lt-dialog")).click();
    await sleep(500);
    lt = await tables();
    check("Reset Libraries on the footprint table too", lt.tables.footprint.global.origin === "config" && lt.tables.footprint.global.rows.length === 1, JSON.stringify(lt.tables.footprint.global.rows.map((r) => r.nickname)));

    // ============================================================================================================================ Edit Options..., an Eagle library read through kicad-cli
    await removeMine("footprint");
    await openTables("footprint", "Project Specific Libraries");
    const e = await addRow("LtGps", "${KIPRJMOD}/gps.lbr", "Eagle");
    await selectRow(e);
    row(e, "nickname").closest("tr").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 300, clientY: 300 }));
    await sleep(300);
    const menuItem = [...document.querySelectorAll("button, [role=menuitem], .context-menu *")].find((b) => b.textContent.trim() === "Edit Options...");
    check("the row menu offers Edit Options...", !!menuItem);
    menuItem?.click();
    await sleep(400);
    const optDlg = await waitFor(() => document.querySelector('[aria-label="Options for Library \'LtGps\'"]'));
    check("Edit Options... opens Options for Library 'LtGps'", !!optDlg);
    if (optDlg) {
      const choices = [...optDlg.querySelectorAll('[aria-label="Option choices"] option')].map((o) => o.value);
      check("it lists the options a footprint format takes (PCB_IO::GetLibraryOptions)", choices.includes("read_filter_regex") && choices.includes("debug_level") && choices.length === 6, JSON.stringify(choices));
      setNative(optDlg.querySelector('[aria-label="Option choices"]'), "read_filter_regex");
      await sleep(100);
      check("a choice shows its help", /footprint name/.test(optDlg.querySelector(".lt-options-help")?.textContent ?? ""), optDlg.querySelector(".lt-options-help")?.textContent);
      button("<<    Append Selected Option", optDlg).click();
      await sleep(100);
      setNative(optDlg.querySelector('[aria-label="Value of option 1"]'), "NEO*");
      button("OK", optDlg).click();
      await sleep(300);
    }
    check("OK gives the row its options as KiCad formats them", row(e, "options").value === "read_filter_regex=NEO*", row(e, "options").value);
    row(e, "options").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await sleep(300);
    const again = document.querySelector('[aria-label="Options for Library \'LtGps\'"]');
    check("a double click on the Options cell opens it again, the option in its row", again && again.querySelector('[aria-label="Option 1"]').value === "read_filter_regex" && again.querySelector('[aria-label="Value of option 1"]').value === "NEO*");
    if (again) { button("Cancel", again).click(); await sleep(200); }
    await sleep(1500); // the row's status: kicad-cli has converted the library by now
    await ok();
    lt = await tables();
    const gps = lt.tables.footprint.project.rows.find((r) => r.nickname === "LtGps");
    check("the Eagle row is written with its options and reads", gps && gps.type === "Eagle" && gps.options === "read_filter_regex=NEO*" && gps.status === "ok", JSON.stringify(gps));
    const items = await get("/api/library/items?kind=footprint&lib=LtGps");
    check("its footprints are listed (kicad-cli's conversion of the .lbr)", items.names && items.names.includes("NEO-M8P") && items.names.length >= 10, JSON.stringify(items).slice(0, 160));
    const fp = await get("/api/library/footprint?name=LtGps:NEO-M8P");
    check("a footprint of it opens", fp && !fp.error && JSON.stringify(fp).includes("NEO-M8P"), JSON.stringify(fp).slice(0, 160));
    const writable = await get("/api/library/writable?kind=footprint");
    check("and it is read only, as KiCad's Eagle plugin is", writable.libraries.find((l) => l.name === "LtGps")?.writable === false, JSON.stringify(writable.libraries.find((l) => l.name === "LtGps")));

    // ============================================================================================================================ Migrate Libraries
    await removeMine("symbol");
    await openTables("symbol", "Project Specific Libraries");
    const base = gridNames().length;
    const k = await addRow("LtKicad", "${KIPRJMOD}/nothing.kicad_sym", "KiCad");
    await selectRow(k);
    button("Migrate Libraries").click();
    const none = await answer("OK");
    check("Migrate on a KiCad row: KiCad's message", none === "Select one or more rows containing libraries to save as current KiCad format.", none);
    top().querySelector('[aria-label="Remove library from table"]').click();
    await sleep(150);
    const l = await addRow("LtLegacy", "${KIPRJMOD}/legacy.lib", "Legacy");
    await selectRow(l);
    button("Migrate Libraries").click();
    const q = await answer("Yes");
    check("Migrate asks KiCad's question", q === "Save 'LtLegacy' as current KiCad format (*.kicad_sym) and replace legacy entry in table?", q);
    const over = await waitFor(question, 3000);
    if (over) {
      const said = await answer("Yes"); // a run before this one left legacy.kicad_sym
      check("a .kicad_sym that is there already is asked about", /^File '.*legacy\.kicad_sym' already exists\. Do you want overwrite this file\?$/.test(said), said);
    }
    await waitFor(() => row(l, "uri")?.value.endsWith("legacy.kicad_sym"), 15000);
    check("the row now names the KiCad library beside the old one, in KiCad's format", row(l, "uri").value === "${KIPRJMOD}/legacy.kicad_sym" && top().querySelector(`[aria-label="type of row ${l + 1}"]`).value === "KiCad", `${row(l, "uri").value} ${banner()}`);
    const eagle = await addRow("LtGpsSym", "${KIPRJMOD}/gps.lbr", "EAGLE");
    await sleep(1500);
    await ok();
    lt = await tables();
    const mig = lt.tables.symbol.project.rows.find((r) => r.nickname === "LtLegacy");
    check("OK writes the migrated row", mig && mig.uri === "${KIPRJMOD}/legacy.kicad_sym" && mig.type === "KiCad" && mig.status === "ok", JSON.stringify(mig));
    const legacyItems = await get("/api/library/items?kind=symbol&lib=LtLegacy");
    check("the migrated library holds the legacy file's symbols", legacyItems.names && ["+3V3", "GND", "R"].every((n) => legacyItems.names.includes(n)), JSON.stringify(legacyItems));
    const eagleItems = await get("/api/library/items?kind=symbol&lib=LtGpsSym");
    check("an Eagle symbol library of the table is read too", eagleItems.names && eagleItems.names.includes("ANTENNAGPS-2X7MM"), `${eagle} ${JSON.stringify(eagleItems).slice(0, 160)}`);
    void base;

    // ============================================================================================================================ Configure Paths: KiCad's variables
    await E.run("common.SuiteControl.configurePaths");
    const paths = await waitFor(() => document.querySelector('[aria-label="KiCad\'s variables"]'));
    check("Configure Paths has KiCad's variables as an editable grid", paths && [...paths.querySelectorAll("input")].length > 0);
    top().querySelector('[aria-label="Add KiCad variable"]').click();
    await sleep(200);
    const names = [...document.querySelectorAll('[aria-label="KiCad\'s variables"] [data-col="name"]')];
    const at = names.length - 1;
    setNative(names[at], "LT_TEAM");
    setNative(document.querySelector(`[aria-label="KiCad's variables"] [data-row="${at}"][data-col="value"]`), dir);
    button("OK").click();
    await sleep(1200);
    lt = await tables();
    check("OK writes KiCad's variable (kicad_common.json)", lt.variables.global.some((v) => v.name === "LT_TEAM" && v.value === dir && !v.external), JSON.stringify(lt.variables.global.map((v) => v.name)));
    check("it is a 3D model search path now", (lt.variables.search_paths ?? []).some((p) => p.alias === "${LT_TEAM}" && p.path === dir), JSON.stringify(lt.variables.search_paths));
    await E.run("common.SuiteControl.configurePaths");
    await waitFor(() => document.querySelector('[aria-label="KiCad\'s variables"]'));
    const gone = [...document.querySelectorAll('[aria-label="KiCad\'s variables"] [data-col="name"]')].findIndex((i) => i.value === "LT_TEAM");
    top().querySelector(`[aria-label="Remove KiCad variable ${gone + 1}"]`)?.click();
    await sleep(150);
    button("OK").click();
    await sleep(1200);
    check("and goes again", !(await tables()).variables.global.some((v) => v.name === "LT_TEAM"));
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : String(e));
  } finally {
    try {
      while (dialogs().length) { (button("Cancel") ?? button("Close") ?? button("OK"))?.click(); await sleep(150); }
      await removeMine("symbol");
      await removeMine("footprint");
    } catch { /* the clean-up is not worth failing for */ }
  }
  check("no console error was raised", E.errors().slice(errorsBefore).length === 0, JSON.stringify(E.errors().slice(errorsBefore)).slice(0, 300));
  const failed = results.filter((r) => !r.ok);
  window.__libraryManageCheck.done = true;
  return { ok: failed.length === 0, failed: failed.length, results };
})();
