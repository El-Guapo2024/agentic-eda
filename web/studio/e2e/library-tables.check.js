// Library tables check (GAPS.md item 5): drives the studio's library table dialogs the way a person does, through `window.__eda` and the page's own controls, and checks what
// reaches the server's tables, the library index, the choosers and the schematic and the board.
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board (it adds a few
// libraries to the board's own tables, makes one empty library file in the board's folder and defines one path variable; it takes all of them back out through the dialogs,
// but the empty file stays), on a machine where KiCad is installed (the libraries it adds to the project table are KiCad's own, under other nicknames, so no library file of
// its own is needed to place from).
//
// What it checks:
//   the actions: Manage Symbol Libraries, Manage Footprint Libraries and Configure Paths run on every tab, New Library and Add Library on the two library editors;
//   the Global tab is KiCad's table, read only, says so, and the table it chains (KiCad's 200-odd libraries) opens in a tab of its own, every library of it readable and the same
//     libraries the index lists;
//   the Project tab: KiCad's OK checks (a missing nickname, an illegal one, a duplicate: the message, the cell with the cursor, nothing written), a path gives the row its format and
//     a status (a missing file is a warning with KiCad's words), OK writes the board's table and the trees, the index and the choosers have the library at once;
//   a library of the project table resolves like an installed one: a symbol placed from it keeps its definition and takes its Footprint field, a footprint placed from it is a board
//     part with its pads, both are undone;
//   Add Library (a path typed, the nickname from the file's name, written with a variable) and New Library (an empty .kicad_sym, in the project's table as ${KIPRJMOD}/...);
//   Configure Paths: KiCad's variables listed read only, the board's own added (the name rules, "path cannot be empty") and used in a library path.
//   a board variable reaches a 3D model: GET /api/3dmodel finds a model written with it, and stops finding it when the variable is gone.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }. The results also pile up in `window.__libraryTablesCheck.results` (`.done` is set at the end).
(async () => {
  const results = [];
  window.__libraryTablesCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const setNative = (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogs = () => [...document.querySelectorAll(".dialog-backdrop .dialog")];
  const top = () => dialogs()[dialogs().length - 1];
  const button = (label, scope = top()) => [...(scope ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
  const waitFor = async (fn, ms = 5000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(60); } };
  const titles = () => E.state().dialogs;
  const row = (i, col) => document.querySelector(`.lt-grid [data-row="${i}"][data-col="${col}"]`);
  const tabByName = (name) => [...document.querySelectorAll(".lt-tab")].find((t) => t.textContent.trim().startsWith(name));
  const banner = () => top()?.querySelector(".lt-banner")?.textContent ?? "";
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const undo = (domain) => post("/api/undo", { domain });
  const fresh = `LtNew${Date.now().toString(36)}`; // a name no earlier run used: the empty library file New Library makes stays in the board's folder
  const errorsBefore = E.errors().length;
  const firstTab = E.state().tab;
  const tables = () => get("/api/library_tables");
  let lt = await tables();
  let baseSym = 0; // the rows the board's table already holds: the check's own come after them
  let baseVars = 0;
  const mine = (v, kind) => v.tables[kind].project.rows.filter((r) => /^Lt/.test(r.nickname));
  const kicad = (kind) => ({ symbol: "KICAD10_SYMBOL_DIR", footprint: "KICAD10_FOOTPRINT_DIR" })[kind].replace("10", String(lt.major));
  const dev = `\${${kicad("symbol")}}/Device.kicad_sym`;
  const mount = `\${${kicad("footprint")}}/MountingHole.pretty`;

  /** Take everything the check added back out through the dialogs: the rows whose nickname starts with Lt, the variable LT_CHECK_DIR. */
  const cleanUp = async () => {
    for (const kind of ["symbol", "footprint"]) {
      if (mine(await tables(), kind).length === 0) continue;
      await E.run(kind === "symbol" ? "common.SuiteControl.showSymbolLibTable" : "common.SuiteControl.showFootprintLibTable");
      await waitFor(() => row(0, "nickname") || document.querySelector(".lt-grid"), 3000);
      for (let guard = 0; guard < 10; guard++) {
        const at = [...document.querySelectorAll('.lt-grid [data-col="nickname"]')].findIndex((i) => /^Lt/.test(i.value));
        if (at < 0) break;
        // select the row as a click on it does (a programmatic focus raises no focus event in a pane that does not have the focus)
        row(at, "nickname").closest("tr").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
        await sleep(100);
        top().querySelector('[aria-label="Remove library from table"]').click();
        await sleep(150);
      }
      button("OK")?.click();
      await sleep(900);
    }
    if ((await tables()).variables.project.some((v) => v.name === "LT_CHECK_DIR")) {
      await E.run("common.SuiteControl.configurePaths");
      await waitFor(() => document.querySelector('[aria-label="Board variables"]'), 3000);
      for (let guard = 0; guard < 5; guard++) {
        const at = [...document.querySelectorAll('[aria-label="Board variables"] [data-col="name"]')].findIndex((i) => i.value === "LT_CHECK_DIR");
        if (at < 0) break;
        top().querySelector(`[aria-label="Remove variable ${at + 1}"]`).click();
        await sleep(150);
      }
      button("OK")?.click();
      await sleep(900);
    }
  };

  try {
    await cleanUp();
    lt = await tables();
    baseSym = lt.tables.symbol.project.rows.length;
    baseVars = lt.variables.project.length;

    // ============================================================================================================================ the actions
    const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    const onEveryTab = ["common.SuiteControl.showSymbolLibTable", "common.SuiteControl.showFootprintLibTable", "common.SuiteControl.configurePaths"];
    check("Manage Symbol Libraries, Manage Footprint Libraries and Configure Paths are wired", onEveryTab.every((id) => acts[id]?.enabled), JSON.stringify(onEveryTab.map((id) => acts[id]?.enabled)));
    await clickTab("Symbol");
    const symActs = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("New Library and Add Library are wired in the Symbol Editor", symActs["common.Control.newLibrary"]?.enabled && symActs["common.Control.addLibrary"]?.enabled);
    await clickTab("Footprint");
    const fpActs = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
    check("New Library and Add Library are wired in the Footprint Editor", fpActs["common.Control.newLibrary"]?.enabled && fpActs["common.Control.addLibrary"]?.enabled);

    // ============================================================================================================================ the global table is KiCad's: read only
    const sym0 = lt.tables.symbol;
    check("the server says the global table is not writable and the board's is", sym0.global.writable === false && sym0.project.writable === true && lt.tables.footprint.global.writable === false);
    await clickTab("Symbol");
    const opened = await E.run("common.SuiteControl.showSymbolLibTable");
    await waitFor(() => document.querySelector(".lt-grid"));
    check("the action opens Manage Symbol Libraries", opened.dialog === "Manage Symbol Libraries" && titles().includes("Manage Symbol Libraries"), JSON.stringify(opened));
    check("it has a Global Libraries tab and a Project Specific Libraries tab", !!tabByName("Global Libraries") && !!tabByName("Project Specific Libraries"));
    tabByName("Global Libraries").click();
    await sleep(300);
    const globalInputs = [...document.querySelectorAll(".lt-grid input[type=text]")];
    check("the Global tab's cells are read only", globalInputs.length > 0 && globalInputs.every((i) => i.readOnly) && [...document.querySelectorAll(".lt-grid input[type=checkbox], .lt-grid select")].every((i) => i.disabled));
    check("it says KiCad's global table is never written by the studio", /never writes it/.test(top().querySelector(".lt-note")?.textContent ?? ""), top().querySelector(".lt-note")?.textContent);
    check("it has no add, move or delete buttons", !top().querySelector('[aria-label="Add empty row to table"]') && !top().querySelector('[aria-label="Remove library from table"]'));
    if (sym0.global.rows.some((r) => r.type === "Table")) {
      top().querySelector(".lt-status.lt-open").click();
      await sleep(400);
      const chained = document.querySelectorAll(".lt-grid tbody tr").length;
      const wanted = sym0.global.rows.find((r) => r.type === "Table").table.rows.length;
      check("the table the global one chains opens in a tab of its own, with every library of it", chained === wanted && chained > 20, `${chained} rows, the server has ${wanted}`);
      const bad = [...document.querySelectorAll(".lt-grid .lt-status.lt-warn")].length;
      check("every library of KiCad's table is readable", bad === 0, `${bad} with a warning`);
      const idx = await get("/api/library/index?kind=symbol");
      check("the library index lists the libraries the tables define (KiCad's through the global table)", idx.libraries.filter((l) => l.scope === "global").length === wanted, `${idx.libraries.filter((l) => l.scope === "global").length} listed, ${wanted} rows`);
      check("the substitutions list has KIPRJMOD and the symbol folder variable", [...top().querySelectorAll('[aria-label="Path substitutions"] tbody tr')].map((tr) => tr.children[0].textContent).join(" ").includes(kicad("symbol")));
    }

    // ============================================================================================================================ the Project tab: KiCad's checks, a path's format and status, OK
    tabByName("Project Specific Libraries").click();
    await sleep(300);
    top().querySelector('[aria-label="Add empty row to table"]').click();
    await sleep(150);
    setNative(row(baseSym, "uri"), dev);
    await sleep(100);
    check("a path gives the row the format it says", top().querySelector(`[aria-label="type of row ${baseSym + 1}"]`).value === "KiCad");
    button("OK").click();
    await sleep(300);
    check("OK refuses a row with no nickname: KiCad's message, the nickname cell has the cursor, nothing is written", banner() === "Library must have a nickname." && document.activeElement === row(baseSym, "nickname") && mine(await tables(), "symbol").length === 0, banner());
    setNative(row(baseSym, "nickname"), "Lt:Dev");
    button("OK").click();
    await sleep(300);
    check("OK refuses a nickname with a colon", banner() === "Illegal character ':' in nickname 'Lt:Dev'.", banner());
    setNative(row(baseSym, "nickname"), "eda");
    button("OK").click();
    await sleep(300);
    check("OK refuses the nickname the studio keeps for its own project library", /studio's own project library/.test(banner()) && mine(await tables(), "symbol").length === 0, banner());
    setNative(row(baseSym, "nickname"), "LtDevice");
    top().querySelector('[aria-label="Add empty row to table"]').click();
    await sleep(150);
    setNative(row(baseSym + 1, "nickname"), "LtDevice");
    setNative(row(baseSym + 1, "uri"), "${KIPRJMOD}/LtMissing.kicad_sym");
    await sleep(1300);
    check("a path with no file is a warning in the status column, in KiCad's words", /^Library '.*LtMissing\.kicad_sym' not found\.$/.test(top().querySelector(".lt-status.lt-warn")?.getAttribute("title") ?? ""), top().querySelector(".lt-status.lt-warn")?.getAttribute("title"));
    button("OK").click();
    await sleep(300);
    check("OK refuses two libraries with one nickname, and puts the cursor in the later one", banner() === "Multiple libraries cannot share the same nickname ('LtDevice')." && document.activeElement === row(baseSym + 1, "nickname"), banner());
    top().querySelector('[aria-label="Remove library from table"]').click();
    await sleep(150);
    setNative(row(baseSym, "descr"), "Device, through the board's table");
    button("OK").click();
    await sleep(1000);
    check("OK writes the board's table and closes", titles().length === 0 && mine(await tables(), "symbol").length === 1, JSON.stringify(mine(await tables(), "symbol").map((r) => r.nickname)));
    const written = mine(await tables(), "symbol")[0];
    check("the row is as typed: the path with its variable, the format, the description, no warning", written.uri === dev && written.type === "KiCad" && written.descr === "Device, through the board's table" && written.status === "ok", JSON.stringify(written));
    const idx = await get("/api/library/index?kind=symbol");
    const listed = idx.libraries.find((l) => l.name === "LtDevice");
    check("the library index lists it, as a library of the project, with its description", listed && listed.scope === "project" && listed.description === "Device, through the board's table", JSON.stringify(listed));
    const items = await get("/api/library/items?kind=symbol&lib=LtDevice");
    check("its items are those of the file", items.names && items.names.includes("R") && items.names.includes("C"), JSON.stringify(items).slice(0, 80));

    // ============================================================================================================================ a symbol of a table library, placed
    const placeAt = { x: 25 * 12700, y: 90 * 12700 };
    await clickTab("Schematic");
    const symsBefore = (await get("/api/schematic")).symbols.length;
    const placed = await cmd({ op: "add_symbol", id: "LT1", lib_id: "LtDevice:R", at: placeAt, rot_millideg: 0, value: "", footprint: "", unit: 1 });
    await sleep(500);
    const sch = await get("/api/schematic");
    const lt1 = sch.symbols.find((s) => s.id === "LT1");
    check("placing a symbol of the project's table library works", placed.ok && !!lt1 && lt1.lib_id === "LtDevice:R", JSON.stringify(placed));
    check("it keeps its definition with the schematic and takes the library's Value", !!sch.lib_symbols["LtDevice:R"] && lt1 && lt1.value === "R", lt1 && lt1.value);
    check("its pins are the library's two", sch.lib_symbols["LtDevice:R"] && sch.lib_symbols["LtDevice:R"].pins.length === 2);
    const undone = await undo("schematic");
    await sleep(500);
    check("one undo takes the symbol and its definition away", undone.ok && (await get("/api/schematic")).symbols.length === symsBefore && !(await get("/api/schematic")).lib_symbols["LtDevice:R"], JSON.stringify(undone));

    // ============================================================================================================================ Add Library (a footprint library by a typed path), a footprint of it placed
    await clickTab("Footprint");
    const addOpened = await E.run("common.Control.addLibrary");
    await waitFor(() => document.querySelector("#lt-path"));
    check("Add Library opens Add Footprint Library", addOpened.dialog === "Add Footprint Library", JSON.stringify(addOpened));
    setNative(document.querySelector("#lt-path"), mount);
    await sleep(150);
    check("the nickname is the library's file name", document.querySelector("#lt-nick").value === "MountingHole", document.querySelector("#lt-nick").value);
    setNative(document.querySelector("#lt-nick"), "LtMount");
    button("Add").click();
    await waitFor(() => titles().length === 0);
    const fpRows = mine(await tables(), "footprint");
    check("the row is in the board's footprint table with the path as a variable", fpRows.length === 1 && fpRows[0].uri === mount && fpRows[0].type === "KiCad", JSON.stringify(fpRows));
    const fpItems = await get("/api/library/items?kind=footprint&lib=LtMount");
    check("the footprint tree lists the library", !!document.querySelector('[data-library="LtMount"]') && fpItems.names && fpItems.names.includes("MountingHole_3.2mm_M3"), JSON.stringify(fpItems).slice(0, 100));
    await clickTab("PCB");
    const partsBefore = (await get("/api/state")).parts.length;
    const hole = await cmd({ op: "place_footprint", footprint: "LtMount:MountingHole_3.2mm_M3", at: { x: 60000, y: 60000 }, reference: "", value: "", definition: null });
    await sleep(500);
    const st = await get("/api/state");
    const part = st.parts.find((p) => p.footprint === "LtMount:MountingHole_3.2mm_M3");
    check("a footprint of the table library is a board part with its pad", hole.ok && !!part && part.placed && part.pads.length === 1, JSON.stringify(hole));
    const undoneFp = await undo("pcb");
    await sleep(500);
    check("one undo takes it away", undoneFp.ok && (await get("/api/state")).parts.length === partsBefore, JSON.stringify(undoneFp));

    // ============================================================================================================================ New Library
    await clickTab("Symbol");
    const created = await E.run("common.Control.newLibrary");
    await waitFor(() => document.querySelector("#lt-name"));
    check("New Library opens New Symbol Library, named New_Library (or the next free), in the board's folder", created.dialog === "New Symbol Library" && /^New_Library(_\d+)?$/.test(document.querySelector("#lt-name").value) && document.querySelector("#lt-folder").value === lt.project_dir, `${created.dialog} ${document.querySelector("#lt-name").value}`);
    setNative(document.querySelector("#lt-name"), fresh);
    button("Create").click();
    await waitFor(() => titles().length === 0);
    const created2 = mine(await tables(), "symbol").find((r) => r.nickname === fresh);
    check("the new library is a row of the board's table, written as ${KIPRJMOD}/..., and readable", created2 && created2.uri === `${"$"}{KIPRJMOD}/${fresh}.kicad_sym` && created2.status === "ok", JSON.stringify(created2));
    const newItems = await get(`/api/library/items?kind=symbol&lib=${fresh}`);
    check("it is empty", Array.isArray(newItems.names) && newItems.names.length === 0, JSON.stringify(newItems));
    await sleep(500);
    check("the symbol tree has it, selected", !!document.querySelector(`[data-library="${fresh}"]`));
    await E.run("common.Control.newLibrary");
    await waitFor(() => document.querySelector("#lt-name"));
    setNative(document.querySelector("#lt-name"), fresh);
    button("Create").click();
    await sleep(700);
    check("a name already used is refused, in KiCad's words", /already exists/.test(top()?.querySelector(".lt-error")?.textContent ?? ""), top()?.querySelector(".lt-error")?.textContent);
    button("Cancel").click();
    await sleep(200);

    // ============================================================================================================================ Configure Paths
    const paths = await E.run("common.SuiteControl.configurePaths");
    await waitFor(() => document.querySelector('[aria-label="Board variables"]'));
    check("Configure Paths opens", paths.dialog === "Configure Paths", JSON.stringify(paths));
    const kicadVars = [...top().querySelectorAll('[aria-label="KiCad\'s variables"] tbody tr')].map((tr) => tr.children[0].textContent);
    check("KiCad's variables are listed, KIPRJMOD first, read only", kicadVars[0] === "KIPRJMOD" && kicadVars.includes(kicad("symbol")) && kicadVars.includes(kicad("footprint")) && !top().querySelector('[aria-label="KiCad\'s variables"] input'), kicadVars.join(" "));
    top().querySelector('[aria-label="Add variable"]').click();
    await sleep(150);
    setNative(top().querySelector(`[data-row="${baseVars}"][data-col="name"]`), "lt_check-dir");
    check("a variable's name takes capitals, digits and underscores (a lower-case letter gives the capital, anything else is dropped)", top().querySelector(`[data-row="${baseVars}"][data-col="name"]`).value === "LT_CHECKDIR", top().querySelector(`[data-row="${baseVars}"][data-col="name"]`).value);
    button("OK").click();
    await sleep(300);
    check("a name with no path is refused, in KiCad's words", /Environment variable path cannot be empty\./.test(top().querySelector(".lt-error")?.textContent ?? ""), top().querySelector(".lt-error")?.textContent);
    setNative(top().querySelector(`[data-row="${baseVars}"][data-col="name"]`), "KIPRJMOD");
    setNative(top().querySelector(`[data-row="${baseVars}"][data-col="value"]`), "/x");
    button("OK").click();
    await sleep(300);
    check("KIPRJMOD is reserved", /reserved/.test(top().querySelector(".lt-error")?.textContent ?? ""), top().querySelector(".lt-error")?.textContent);
    setNative(top().querySelector(`[data-row="${baseVars}"][data-col="name"]`), "LT_CHECK_DIR");
    setNative(top().querySelector(`[data-row="${baseVars}"][data-col="value"]`), "${KIPRJMOD}");
    button("OK").click();
    await sleep(900);
    check("OK saves the board's variable", titles().length === 0 && (await tables()).variables.project.some((v) => v.name === "LT_CHECK_DIR" && v.value === "${KIPRJMOD}"), JSON.stringify((await tables()).variables.project));
    // a board variable reaches the 3D models too: GET /api/3dmodel asks the board's variables before the environment, the way a library path does
    {
      const saved = (await tables()).variables;
      const modelsDir = saved.global.find((v) => v.name === `KICAD${lt.major}_3DMODEL_DIR`)?.value;
      const ask = (name) => fetch(`/api/3dmodel?name=${encodeURIComponent(name)}`, { cache: "no-store" }).then((r) => r.status);
      if (modelsDir) {
        await post("/api/path_variables", { variables: [...saved.project, { name: "LT_MODELS_DIR", value: modelsDir }] });
        const found = await ask("${LT_MODELS_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step");
        check("a model written with a board variable is found (served, or being converted)", found === 200 || found === 202, found);
        check("a variable nothing defines finds no model", (await ask("${LT_NOT_DEFINED}/Resistor_SMD.3dshapes/R_0603_1608Metric.step")) === 404);
        await post("/api/path_variables", { variables: saved.project });
        check("and the model is not found once the variable is gone", (await ask("${LT_MODELS_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step")) === 404);
      } else {
        check("KiCad's 3D model folder is defined (skipped: this machine has no KiCad variables)", true, "skipped");
      }
    }
    await E.run("common.SuiteControl.showSymbolLibTable");
    await waitFor(() => row(0, "nickname"));
    const at = [...document.querySelectorAll('.lt-grid [data-col="nickname"]')].findIndex((i) => i.value === fresh);
    setNative(row(at, "uri"), `${"$"}{LT_CHECK_DIR}/${fresh}.kicad_sym`);
    await sleep(1300);
    check("a path written with the board's variable is found", !top().querySelectorAll(".lt-status.lt-warn").length || ![...top().querySelectorAll(".lt-grid tbody tr")][at].querySelector(".lt-status.lt-warn"), "");
    const subs = [...top().querySelectorAll('[aria-label="Path substitutions"] tbody tr')].map((tr) => `${tr.children[0].textContent}=${tr.children[1].textContent}`);
    check("the path substitutions list the variable with its value", subs.some((s) => s === "${LT_CHECK_DIR}=${KIPRJMOD}"), subs.join(" | "));
    button("Cancel").click();
    await sleep(200);
    const afterCancel = mine(await tables(), "symbol").find((r) => r.nickname === fresh);
    check("Cancel writes nothing", afterCancel && afterCancel.uri === `${"$"}{KIPRJMOD}/${fresh}.kicad_sym`);

    // ============================================================================================================================ the way back
    await cleanUp();
    const after = await tables();
    check("the dialogs take every row and the variable back out", mine(after, "symbol").length === 0 && mine(after, "footprint").length === 0 && !after.variables.project.some((v) => v.name === "LT_CHECK_DIR"), JSON.stringify([mine(after, "symbol"), mine(after, "footprint")]).slice(0, 200));
    const idx2 = await get("/api/library/index?kind=symbol");
    check("and the library index does not list them any more", !idx2.libraries.some((l) => /^Lt/.test(l.name)));
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : String(e));
  } finally {
    try {
      while (dialogs().length) { (button("Cancel") ?? button("Close"))?.click(); await sleep(150); }
      await cleanUp();
      await clickTab(firstTab === "schematic" ? "Schematic" : firstTab === "footprint" ? "Footprint" : firstTab === "symbol" ? "Symbol" : "PCB");
    } catch { /* the tab is not worth failing for */ }
  }
  check("no console error was raised", E.errors().slice(errorsBefore).length === 0, JSON.stringify(E.errors().slice(errorsBefore)).slice(0, 300));
  const failed = results.filter((r) => !r.ok);
  window.__libraryTablesCheck.done = true;
  return { ok: failed.length === 0, failed: failed.length, results };
})();
