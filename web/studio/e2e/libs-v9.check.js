// Libraries and choosers, round 2 (UI-LEFT-2 LL4, LL5, LL7; task-v9-libs): what the choosers search and show, Manage Libraries' paste and table choices, Save
// Library As, Rescue / Remap's conditions and CvPcb's keys.
//
// What it checks:
//   the server's search takes the tree's shown columns (`RebuildSearchTerms`): a Datasheet text is found only with the Datasheet column shown;
//   the choosers' regular expressions are wxWidgets 3.2's PCRE2 (`wxRE_ADVANCED`): `/\ylm358\y/` (Tcl's word boundary, which JavaScript's RegExp refuses) finds LM358;
//   Select Columns adds Value: an "-- Already Placed --" row shows its Value (`GetChooserFields` of the design's symbols); a column dragged wider keeps its
//     width the next time the chooser opens (`column_widths`);
//   KiCad 25ec2335's "Body style:" choice: a De Morgan symbol (74xx:74LS00) offers Standard and Alternate, a plain one has it disabled;
//   Manage Symbol Libraries: a tab-separated text pasted on the project grid becomes a row (`LIB_TABLE_GRID_TRICKS::paste_text`), a table's s-expression
//     its rows at the top; Cancel leaves the table as it was;
//   New Library's table choice (`FILEDLG_HOOK_NEW_LIBRARY`): global for a symbol library, project for a footprint library, as KiCad starts them;
//   Save Library As (server): "Do not update library tables" writes the file and no row; Save Library As Unpacked writes a folder with a file per symbol;
//   Remap Legacy Library Symbols is disabled on a schematic whose symbols name their libraries (`remapSymbolsCondition`);
//   Assign Footprints: Tab moves the keyboard between the lists, Enter assigns the picked footprint and moves to the next symbol with none, Delete takes it
//     away (CvPcb's hotkeys); the viewer is a window opened by View Selected Footprint. Nothing is applied: Cancel.
// The files the check makes stay in the board's folder (their rows are taken out of the table); every design edit is undone.
// It answers through `window.__libsV9Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=10   (KICAD_PATH with KiCad's symbol and footprint libraries)
(async () => {
  const results = [];
  const check = (window.__libsV9Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };
  const E = window.__eda;
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const run = (id, args) => E.run(id, args);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const rightClick = (el) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 5, button: 2 })); };
  const menuItem = (label) => [...document.querySelectorAll("[role=menuitem], .context-menu button, .context-menu div")].find((e) => e.textContent.trim() === label);
  const pick = (list, text) => [...(list?.querySelectorAll("[role=option]") ?? [])].find((o) => o.textContent.trim() === text);
  const headerColumns = (root) => [...(root?.querySelectorAll("[data-chooser-header] [data-column]") ?? [])].map((e) => e.dataset.column);
  const items = (root) => [...(root?.querySelectorAll("[data-chooser-tree] [data-id]") ?? [])].filter((r) => !r.dataset.unit).map((r) => r.dataset.id);
  const status = (root) => root?.querySelector("[data-chooser-status]")?.textContent ?? "";
  const search = async (root, q, ms = 90000) => {
    setValue(root.querySelector("#library-tree-search"), q);
    return until(() => root.querySelector("[data-chooser-status]")?.dataset.chooserAnswered === q && / match(es)?/.test(status(root)), ms);
  };
  const closeChoosers = async () => {
    for (let i = 0; i < 4; i++) {
      const c = document.querySelector("[data-chooser-cancel]");
      if (!c) break;
      c.click();
      await sleep(300);
    }
  };
  const openChooser = async () => {
    await run("eeschema.InteractiveDrawing.placeSymbol");
    return until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 8000);
  };
  const selectColumns = async (root, change) => {
    rightClick(root.querySelector("[data-chooser-header]"));
    (await until(() => menuItem("Select Columns..."), 3000))?.click();
    const dlg = await until(() => document.querySelector("[data-select-columns-dialog]"), 3000);
    if (!dlg) return false;
    await change(dlg);
    dlg.querySelector("[data-columns-ok]")?.click();
    await sleep(300);
    return true;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeChoosers(); await run("common.Interactive.cancel"); } catch { /* ignore */ }
    results.push(entry);
  };
  const errorsBefore = E.errors().length;
  const tablesBefore = await json("/api/library_tables");
  const s = Date.now().toString(36);

  // ----------------------------------------------------------------------------------------------------------- LL4: searching and showing
  await scenario("the server's search takes the shown columns, and its regular expressions are wxRE_ADVANCED's PCRE2", async (t) => {
    const q = encodeURIComponent("symlink/lm2904-n.pdf");
    const idsOf = (r) => (r.libraries ?? []).flatMap((l) => l.items.map((i) => `${l.name}:${i.name}`));
    const ask = async (cols) => until(async () => { const r = await json(`/api/library/search?kind=symbol&q=${q}${cols ? `&cols=${encodeURIComponent(cols)}` : ""}`); return r.status === "ready" ? r : null; }, 180000);
    const shown = await ask("Item\tDatasheet");
    t(shown && idsOf(shown).includes("Amplifier_Operational:LM358"), `the Datasheet column shown: LM358 is found by its datasheet (${shown ? idsOf(shown).slice(0, 5).join(", ") : "no answer"})`);
    const hidden = await ask("");
    t(hidden && !idsOf(hidden).includes("Amplifier_Operational:LM358"), `not shown (Item and Description): it is not (${hidden ? idsOf(hidden).length : "no answer"} items)`);
    await clickTab("Schematic");
    const root = await openChooser();
    if (!t(root, "A opens the Symbol Chooser")) return;
    t(await search(root, "/\\ylm358\\y/"), `the search answers: ${status(root)}`);
    t(items(root).includes("Amplifier_Operational:LM358"), `/\\ylm358\\y/ (Tcl's \\y, a word's edge) finds LM358: ${items(root).slice(0, 6).join(", ")}`);
  });

  await scenario("Select Columns adds Value: Already Placed rows show their fields; a dragged column keeps its width", async (t) => {
    const project = await json("/api/library/project");
    const placedEntry = (project.entries ?? []).find((e) => e.placed && e.fields && e.fields.Value);
    t(!!placedEntry, `the design's placed symbols carry their chooser fields (${placedEntry ? `${placedEntry.id}: Value ${placedEntry.fields.Value}` : "none"})`);
    const root = await openChooser();
    if (!t(root, "the Symbol Chooser opens")) return;
    const added = await selectColumns(root, async (dlg) => {
      pick(dlg.querySelector("[data-columns-available]"), "Value")?.click();
      await sleep(100);
      dlg.querySelector("[data-columns-add]")?.click();
      await sleep(100);
    });
    t(added && headerColumns(root).includes("Value"), `Value is shown: ${headerColumns(root).join(", ")}`);
    const group = await until(() => [...root.querySelectorAll("[data-chooser-tree] [data-library]")].find((g) => /Already Placed/.test(g.textContent)), 5000);
    if (group && group.getAttribute("aria-expanded") !== "true") {
      group.click();
      await sleep(400);
    }
    const valueCells = await until(() => [...root.querySelectorAll('[data-chooser-tree] .chooser-row.item [data-column="Value"]')].filter((c) => c.textContent.trim() !== ""), 5000);
    t(valueCells && valueCells.length > 0, `an Already Placed row's Value cell is filled: "${valueCells?.[0]?.textContent ?? ""}"`);
    // drag the Value column's edge 80 px to the right
    const head = root.querySelector('[data-chooser-header] [data-column="Value"]');
    const grip = head?.querySelector("[data-column-resizer]");
    const w0 = head?.getBoundingClientRect().width ?? 0;
    if (grip) {
      const r = grip.getBoundingClientRect();
      const ev = (type, x) => grip.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: r.top + 2, pointerId: 7 }));
      ev("pointerdown", r.left + 2);
      ev("pointermove", r.left + 82);
      ev("pointerup", r.left + 82);
      await sleep(300);
    }
    const w1 = root.querySelector('[data-chooser-header] [data-column="Value"]')?.getBoundingClientRect().width ?? 0;
    t(Math.abs(w1 - w0 - 80) < 6, `the column is 80 px wider (${Math.round(w0)} -> ${Math.round(w1)})`);
    await closeChoosers();
    await run("common.Interactive.cancel");
    const again = await openChooser();
    const w2 = again?.querySelector('[data-chooser-header] [data-column="Value"]')?.getBoundingClientRect().width ?? 0;
    t(Math.abs(w2 - w1) < 2, `opened again, it keeps that width (${Math.round(w2)})`);
    // and Value out again
    if (again) {
      await selectColumns(again, async (dlg) => {
        pick(dlg.querySelector("[data-columns-enabled]"), "Value")?.click();
        await sleep(100);
        dlg.querySelector("[data-columns-remove]")?.click();
        await sleep(100);
      });
      t(JSON.stringify(headerColumns(again)) === JSON.stringify(["Item", "Description"]), `taken out: ${headerColumns(again).join(", ")}`);
    }
  });

  await scenario("the Symbol Editor's tree scrolls sideways to its last column, under a header that stays on top, its vertical scroll bar at the pane's edge", async (t) => {
    // `LIB_TREE`'s wxDataViewCtrl: Item 300 and Description 600 in a pane narrower than that scroll sideways, header and rows together
    await clickTab("Symbol");
    await until(() => E.state().tab === "symbol", 4000);
    const area = await until(() => document.querySelector('[data-library-tree-columns="symbol"]'), 8000);
    if (!t(area, "the Symbol Editor's tree")) return;
    const pane = area.closest("[role=tree]");
    // an installed library open, so there are more rows than the pane shows
    const lib = await until(() => [...area.querySelectorAll("[data-library]")].find((g) => g.dataset.library === "Device") ?? area.querySelector("[data-library]"), 6000);
    if (lib && lib.getAttribute("aria-expanded") !== "true") lib.click();
    await until(() => area.scrollHeight > area.clientHeight + 50, 15000);
    t(area.scrollWidth > area.clientWidth + 100, `the columns are wider than the pane (${area.scrollWidth} > ${area.clientWidth})`);
    t(area.scrollHeight > area.clientHeight, `the rows scroll in the same area (${area.scrollHeight} > ${area.clientHeight})`);
    const a = area.getBoundingClientRect();
    const p = pane.getBoundingClientRect();
    t(a.right <= p.right + 1, `the scroll area, and so its vertical scroll bar, ends at the pane's edge (${Math.round(a.right)} <= ${Math.round(p.right)})`);
    area.scrollLeft = area.scrollWidth;
    area.scrollTop = 200;
    area.dispatchEvent(new Event("scroll"));
    await sleep(200);
    const head = area.querySelector('[data-library-tree-header="symbol"]');
    const last = head.querySelector('[data-column="Description"]');
    const lr = last?.getBoundingClientRect();
    const visibleRight = a.left + area.clientWidth;
    // the rows' 8 px right padding is past the last column
    t(area.scrollLeft > 0 && lr && lr.right <= visibleRight + 1 && visibleRight - lr.right < 12 && lr.left < visibleRight, `scrolled to the right end, the Description column is in view (its right ${Math.round(lr?.right ?? 0)}, the view's ${Math.round(visibleRight)})`);
    t(Math.abs(head.getBoundingClientRect().top - a.top) < 2, `scrolled down 200 px, the header is still on top (${Math.round(head.getBoundingClientRect().top)} vs ${Math.round(a.top)})`);
    const cell = [...area.querySelectorAll('[role=treeitem] [data-column="Description"]')].find((c) => { const r = c.getBoundingClientRect(); return r.top > a.top + 20 && r.bottom < a.bottom; });
    t(cell && cell.getBoundingClientRect().left < visibleRight && cell.getBoundingClientRect().right > a.left, "a row's Description cell is in view with it");
    area.scrollLeft = 0;
    area.scrollTop = 0;
    await clickTab("Schematic");
    await until(() => E.state().tab === "schematic", 4000);
  });

  await scenario("the Symbol Chooser's Body style: a De Morgan symbol offers Standard and Alternate; a plain one has it disabled", async (t) => {
    const root = await openChooser();
    if (!t(root, "the Symbol Chooser opens")) return;
    t(await search(root, "74LS00"), `search: ${status(root)}`);
    const row = await until(() => root.querySelector('[data-chooser-tree] [data-id="74xx:74LS00"]'), 5000);
    if (!t(row, "74xx:74LS00 is listed")) return;
    row.click();
    const sel = await until(() => { const s = root.querySelector("[data-chooser-body-style]"); return s && !s.disabled ? s : null; }, 15000);
    t(sel && [...sel.options].map((o) => o.textContent).join(",") === "Standard,Alternate", `Body style: ${sel ? [...sel.options].map((o) => o.textContent).join(", ") : "disabled"}`);
    if (sel) {
      setValue(sel, "2");
      await sleep(200);
      t(sel.value === "2", "Alternate chosen");
    }
    t(await search(root, "Device:R"), `search: ${status(root)}`);
    const r = await until(() => root.querySelector('[data-chooser-tree] [data-id="Device:R"]'), 5000);
    r?.click();
    await sleep(1500);
    t(root.querySelector("[data-chooser-body-style]")?.disabled === true, "Device:R has one body style: the choice is disabled");
  });

  // ----------------------------------------------------------------------------------------------------------- LL5: Manage Libraries and Save Library As
  await scenario("Manage Symbol Libraries: pasted table text becomes rows; Cancel leaves the table", async (t) => {
    await run("common.SuiteControl.showSymbolLibTable");
    const dlg = await until(() => document.querySelector('[role=dialog][aria-label="Symbol Libraries"]'), 6000);
    if (!t(dlg, "the Symbol Libraries dialog opens")) return;
    const projectTab = [...dlg.querySelectorAll("button, [role=tab]")].find((b) => /Project Specific/.test(b.textContent));
    projectTab?.click();
    await sleep(300);
    const grid = dlg.querySelector("[data-lt-grid-paste]");
    const count = () => dlg.querySelectorAll('input[data-col="nickname"]').length;
    const n0 = count();
    const paste = (target, text) => {
      const dt = new DataTransfer();
      dt.setData("text/plain", text);
      target.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: dt }));
    };
    paste(grid, `\t1\t1\tPasted${s}\t\${KIPRJMOD}/pasted.kicad_sym\tKiCad\t\tFrom the clipboard\n`);
    await until(() => count() > n0, 3000);
    const nick = [...dlg.querySelectorAll('input[data-col="nickname"]')].map((i) => i.value);
    t(nick.includes(`Pasted${s}`), `a tab-separated row is added: ${nick.join(", ")}`);
    paste(grid, `(sym_lib_table (version 7) (lib (name "Sexpr${s}") (type "KiCad") (uri "\${KIPRJMOD}/sexpr.kicad_sym") (options "") (descr "")))`);
    await until(() => [...dlg.querySelectorAll('input[data-col="nickname"]')].some((i) => i.value === `Sexpr${s}`), 4000);
    const first = dlg.querySelector('input[data-col="nickname"]')?.value;
    t(first === `Sexpr${s}`, `a table's s-expression puts its rows at the top: ${first}`);
    [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(300);
    const now = await json("/api/library_tables");
    t(now.tables.symbol.project.rows.length === tablesBefore.tables.symbol.project.rows.length, "Cancel: the project table is as it was");
  });

  await scenario("New Library's table choice starts where KiCad's does: global for symbols, project for footprints", async (t) => {
    await clickTab("Symbol");
    await until(() => E.state().tab === "symbol", 4000);
    await run("common.Control.newLibrary");
    let d = await until(() => document.querySelector("[data-library-file-table]")?.closest(".dialog"), 4000);
    const checked = (dd) => dd?.querySelector("[data-library-file-table] input:checked")?.value;
    t(d && checked(d) === "global", `New Symbol Library: ${checked(d)} ("Add new library to global library table")`);
    t(d && /Add new library to project library table/.test(d.textContent), "and the project choice");
    [...(d?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(300);
    await clickTab("Footprint");
    await until(() => E.state().tab === "footprint", 4000);
    await run("common.Control.newLibrary");
    d = await until(() => document.querySelector("[data-library-file-table]")?.closest(".dialog"), 4000);
    t(d && checked(d) === "project", `New Footprint Library: ${checked(d)}`);
    [...(d?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(300);
    await clickTab("Schematic");
  });

  let lib = null;
  await scenario("Save Library As: Do not update library tables, and Save Library As Unpacked", async (t) => {
    lib = `V9Lib${s}`;
    const add = await post("/api/library_tables/add", { kind: "symbol", path: `${lib}.kicad_sym`, create: true });
    if (!t(!add.error, `New Library ${lib}: ${add.error ?? "made"}`)) return;
    const base = (await json("/api/library/symbol?lib_id=Device:R")).symbol;
    await cmd({ op: "put_library_symbol", symbol: { ...base, lib_id: `${lib}:Rv9` }, overwrite: true });
    await cmd({ op: "put_library_symbol", symbol: { ...base, lib_id: `${lib}:Cv9` }, overwrite: true });
    const saved = await post("/api/library/save", { kind: "symbol", lib });
    t(saved.ok, `saved: ${saved.error ?? "ok"}`);
    const rows0 = (await json("/api/library_tables")).tables.symbol.project.rows.length;
    const copy = await post("/api/library/save_as", { kind: "symbol", lib, path: `${lib}-copy.kicad_sym`, table: "none" });
    t(copy.ok && copy.nickname === null, `Do not update library tables: written, no row (${JSON.stringify(copy).slice(0, 160)})`);
    t((await json("/api/library_tables")).tables.symbol.project.rows.length === rows0, "the table is as it was");
    const unpacked = await post("/api/library/save_as", { kind: "symbol", lib, path: `${lib}.kicad_symdir`, mode: "unpacked" });
    t(unpacked.ok && (unpacked.written ?? []).length === 2 && unpacked.written.every((f) => /\.kicad_symdir\/[RC]v9\.kicad_sym$/.test(f)), `Unpacked: a file per symbol in the folder (${JSON.stringify(unpacked.written ?? unpacked.error)})`);
    const index = (await json("/api/library/index?kind=symbol")).libraries.find((l) => l.name === lib);
    t(index && !index.unpacked, "the file library is no folder (Save Library As Unpacked is offered for it)");
  });

  // ----------------------------------------------------------------------------------------------------------- LL7
  await scenario("Remap Legacy Library Symbols is disabled on a schematic whose symbols name their libraries", async (t) => {
    await clickTab("Schematic");
    t((await json("/api/sch/legacy")).legacy === false, "GET /api/sch/legacy: not a legacy schematic");
    await sleep(800);
    const r = await run("eeschema.EditorControl.remapSymbols");
    t(r && r.ok === false && !document.querySelector("[data-remap-dialog]"), `the action is disabled: ${JSON.stringify(r).slice(0, 120)}`);
  });

  let padFp = null;
  await scenario("Assign Footprints: CvPcb's Tab, Enter and Delete; the viewer is a window View Selected Footprint opens", async (t) => {
    // a footprint of the project for the list to offer (a board made from an intent may have none)
    const d = await json("/api/library/details?kind=footprint&id=Resistor_SMD:R_0603_1608Metric");
    if (d.footprint) {
      padFp = `V9Pad${s}`;
      const put = await cmd({ op: "put_library_footprint", footprint: { ...d.footprint, name: padFp }, overwrite: true });
      t(put.ok !== false, `a project footprint ${padFp} to assign`);
    }
    await run("eeschema.EditorControl.assignFootprints");
    const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.textContent.trim() === "Assign Footprints"), 6000);
    if (!t(dlg, "Assign Footprints opens")) return;
    t(!dlg.querySelector("[data-cvpcb-viewer]"), "no viewer to begin with");
    dlg.querySelector("[data-cvpcb-viewer-toggle]")?.click();
    const v = await until(() => dlg.querySelector("[data-cvpcb-viewer]"), 3000);
    t(v && getComputedStyle(v).position === "fixed", "View Selected Footprint opens the viewer window");
    const symList = dlg.querySelector('[data-cvpcb-list="component"]');
    const firstRow = symList?.querySelector("[data-cvpcb-symbol]");
    firstRow?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    symList?.focus();
    await sleep(200);
    const key = (k, extra = {}) => document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
    key("Tab");
    await sleep(150);
    t(document.activeElement?.dataset.cvpcbList === "footprint", `Tab: component -> footprint (${document.activeElement?.dataset.cvpcbList})`);
    key("Tab", { shiftKey: true });
    await sleep(150);
    t(document.activeElement?.dataset.cvpcbList === "component", `Shift+Tab: back to the symbols (${document.activeElement?.dataset.cvpcbList})`);
    key("Tab", { shiftKey: true });
    await sleep(150);
    t(document.activeElement?.dataset.cvpcbList === "library", `Shift+Tab: the libraries (${document.activeElement?.dataset.cvpcbList})`);
    // pick a footprint, select the first symbol, Enter assigns it (staged) and moves on to the next symbol with none; the symbol's own filters off, so the
    // project footprint is offered whatever the symbol
    const filterBox = [...dlg.querySelectorAll("label")].find((l) => /Filter by the symbol's footprint filters/.test(l.textContent))?.querySelector("input");
    if (filterBox?.checked) filterBox.click();
    await sleep(300);
    const fp = await until(() => dlg.querySelector(padFp ? `[data-cvpcb-fp="${padFp}"]` : "[data-cvpcb-fp]") ?? dlg.querySelector("[data-cvpcb-fp]"), 4000);
    const ref = firstRow?.dataset.cvpcbSymbol;
    if (fp && ref) {
      firstRow.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      fp.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      dlg.querySelector('[data-cvpcb-list="footprint"]')?.focus();
      await sleep(200);
      key("Enter");
      await sleep(300);
      const assigned = dlg.querySelector(`[data-cvpcb-symbol="${ref}"] [data-cvpcb-assigned]`)?.textContent.trim();
      t(assigned === fp.dataset.cvpcbFp, `Enter assigns ${fp.dataset.cvpcbFp} to ${ref} (${assigned})`);
      dlg.querySelector(`[data-cvpcb-symbol="${ref}"]`)?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      symList.focus();
      await sleep(150);
      key("Delete");
      await sleep(300);
      const gone = dlg.querySelector(`[data-cvpcb-symbol="${ref}"] [data-cvpcb-assigned]`)?.textContent.trim();
      t(gone === "(none)", `Delete takes it away (${gone})`);
    } else t(false, `a symbol and a footprint to assign (${ref}, ${dlg.querySelectorAll("[data-cvpcb-fp]").length} footprints listed)`);
    [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(300);
    t(!document.querySelector("[data-cvpcb-viewer]"), "Cancel closes CvPcb and its viewer");
  });

  // ----------------------------------------------------------------------------------------------------------- clean up
  await scenario("clean up: the check's rows out of the table, no page error", async (t) => {
    if (padFp) await cmd({ op: "delete_library_footprint", name: padFp });
    if (lib) {
      for (const name of ["Rv9", "Cv9"]) await cmd({ op: "delete_library_symbol", lib_id: `${lib}:${name}` });
      const rows = (await json("/api/library_tables")).tables.symbol.project.rows.filter((r) => r.nickname !== lib);
      const back = await post("/api/library_tables", { kind: "symbol", rows });
      t(!back.error, `${lib}'s row out: ${back.error ?? "ok"}`);
    }
    const now = await json("/api/library_tables");
    t(now.tables.symbol.project.rows.length === tablesBefore.tables.symbol.project.rows.length, `the project table has the rows it had (${now.tables.symbol.project.rows.length})`);
    const errs = E.errors().slice(errorsBefore);
    t(errs.length === 0, `no page error: ${errs.map((e) => e.message ?? e).join(" | ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return check.ok;
})();
