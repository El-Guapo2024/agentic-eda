// The Hierarchy Navigator (GAPS.md item 33, package P4's hierarchy row, where Import Sheet and the Hierarchy Navigator were the two
// checks it lacked): the docked tree of the design's sheets -- the root, then every sheet placed on it, then the sheets those place --
// against KiCad's hierarchy pane at 8303b2ad (eeschema/widgets/hierarchy_pane.cpp). KiCad's pane labels every sheet "name (page N)"
// (`formatPageString`, the page of its `SCH_SHEET_PATH`: the one it was given, else the place in the walk the load numbers,
// `SetInitialPageNumbers` in files-io.cpp; a sheet drawn or imported gets `GetNextPageNumber`, the sheets inside an imported one the lowest
// free numbers, `AddNewSheetInstances`), sorts
// each parent's children by `ComparePageNum` (`HIERARCHY_TREE::OnCompareItems` and `SortChildren`), lists a screen shown twice once for
// each placement, and a click opens that sheet (`SCH_ACTIONS::changeSheet`). The walk visits a screen's sheets by place, left to right
// (`SCH_SCREEN::GetSheets`). The studio's panel reads the same walk from
// `GET /api/sch/hierarchy` and opens a sheet with `eeschema.NavigateTool.changeSheet`; what a click opens is the editor's sheet path and
// the breadcrumb `GET /api/schematic` returns (the title block's "Sheet: /.../").
//
// The board is examples/mcu_board_30plus.yaml (three module sheets, no page numbers of their own) with KiCad's QA
// `qa/data/eeschema/issue10926_1_subsheet_1.kicad_sch` under `fixtures/` (tools/run-all-checks.sh, board=mcu30-sheet). The check brings
// it in as a sheet through `import_sheet` first, so the tree is two levels deep and holds a screen shown twice: the imported sheet's two
// nested sheets both show `issue10926_1_subsheet_1_1.kicad_sch`. The module sheets have no page stored, so they show the pages the load
// numbers by place (Header 2, MCU 3, LED channels 4); the imported sheet gets the next free page (5) and its nested sheets the lowest free
// ones in the walk, Subsheet_1_2 (on the left) 6 and Subsheet_1_1 7 -- not the 2 and 4 the QA file gave them: `AddNewSheetInstances` finds no
// instance of the new sheet path among them (`getInstance( .., aTestFromEnd )` matches the path's end, which is the new sheet's own uuid).
//
// Run it in the page of a served board (see tools/run-all-checks.sh), schematic tab open --
// `node web/studio/e2e/run-check.mjs <url> web/studio/e2e/hierarchy-navigator.check.js`. It reads and navigates; the edits are the
// setup import and one page number set and undone, on the runner's copy of the board.
//
// It answers through `window.__hierarchyNavigatorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-sheet minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__hierarchyNavigatorCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__hierarchyNavigatorSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 5e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__hierarchyNavigatorSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const SOURCE = "fixtures/issue10926_1_subsheet_1.kicad_sch";
  const NESTED = "issue10926_1_subsheet_1_1.kicad_sch";
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  /** The schematic of a sheet path: the placement ids from the root down, joined with "/" (the server splits the query on "/", so they are not escaped). */
  const schOf = (ids) => json(`/api/schematic${ids.length > 0 ? `?sheet=${ids.map(encodeURIComponent).join("/")}` : ""}`);
  const hierarchy = () => json("/api/sch/hierarchy").then((r) => r.sheets);
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const panel = () => document.querySelector("#hierarchy-panel");
  const rows = () => [...(panel()?.querySelectorAll('[role="button"]') ?? [])];
  // the row's text without its fold mark (r6 C6-L2)
  const text = (r) => (r?.innerText ?? "").replace(/[▾▸]/g, "").replace(/\s*\n\s*/g, " ").trim();
  const marked = () => rows().map((r) => r.getAttribute("aria-current") === "true");
  const rowAt = (label) => rows().find((r) => text(r).startsWith(label));
  const waitRows = async (n) => { for (let i = 0; i < 50; i++) { if (rows().length === n) return true; await sleep(200); } return rows().length === n; };
  const clickRow = async (row) => { row?.click(); await sleep(1200); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  await leave();
  // The setup: one import as a sheet, straight to the command (the Import Sheet dialog is import-sheet.check.js's subject), so the tree
  // below the root has two levels and a screen shown twice.
  const before = await hierarchy();
  const setup = await post({
    op: "import_sheet", source: SOURCE, as_sheet: true, name: "issue10926_1_subsheet_1", file: "issue10926_1_subsheet_1.kicad_sch",
    at: { x: 200_000_000, y: 100_000_000 }, size: [40_000_000, 30_000_000], keep_annotations: false, order: "y_then_x",
    options: { algo: "first_free", start_number: 1 },
  });
  if (!(before.length === 4 && setup.ok === true && (await waitRows(7)))) {
    results.push({ name: "setup", ok: false, notes: [`FAIL: the board needs the three module sheets and the QA sheet importable (${before.length} sheets, import ${JSON.stringify(setup)})`] });
    check.done = true; return "no hierarchy";
  }
  const sheets = await hierarchy();
  const root = sheets[0];
  const imported = sheets.find((h) => h.path.length === 1 && h.name === "issue10926_1_subsheet_1");
  const subs = sheets.filter((h) => h.path.length === 2 && h.path[0] === imported?.path[0]);
  const sub1 = subs.find((h) => h.name === "Subsheet_1_1");
  const sub2 = subs.find((h) => h.name === "Subsheet_1_2");
  if (!imported || !sub1 || !sub2) { results.push({ name: "setup", ok: false, notes: [`FAIL: the imported sheet's two nested sheets are missing (${sheets.map((h) => h.path.join("/") + ":" + h.name)})`] }); check.done = true; return "no nested sheets"; }

  (async () => {
    // ----- the list
    await scenario("The hierarchy: every sheet once, the root first, KiCad's order, a screen shown twice twice, with its pages", async (t) => {
      t(!!panel(), "the Hierarchy Navigator is docked");
      t(rows().length === sheets.length, `it lists every sheet of GET /api/sch/hierarchy (${rows().length} of ${sheets.length})`);
      // KiCad's pane: the root, then each sheet followed by the sheets inside it, a parent's children in page order (buildHierarchyTree,
      // m_tree->SortChildren( aParent ), HIERARCHY_TREE::OnCompareItems -> SCH_SHEET_PATH::ComparePageNum). The modules' pages are the
      // load's numbers by place (SetInitialPageNumbers over the walk, SCH_SCREEN::GetSheets: Header x 90, MCU 135, LED channels 175), the
      // imported sheet is the next free page (GetNextPageNumber, sch_drawing_tools.cpp).
      // the root row is the project's name (`getRootString`, r6 C6-L2), the name of the board's own file
      const project = (/^(\S+)\.kicad_sch/.exec(text(rows()[0]).split(" ").find((w) => w.endsWith(".kicad_sch")) ?? "") ?? [])[1] ?? "?";
      t(text(rows()[0]).startsWith(`${project} `) && project !== "Root", `the root row is the project's name (${text(rows()[0])})`);
      const order = [project, "Header (J1)", "MCU (U1)", "LED channels (D1–D8)", "issue10926_1_subsheet_1", "Subsheet_1_2", "Subsheet_1_1"];
      t(order.every((label, i) => text(rows()[i]).startsWith(label)), `the root first, then the modules and the imported sheet in page order, the imported sheet's two after it (${rows().map((r) => text(r).split(/ \S+\.kicad_sch/)[0])})`);
      t(rows()[5]?.style.marginLeft === "20px" && rows()[6]?.style.marginLeft === "20px" && rows()[1]?.style.marginLeft === "10px", `a nested sheet is shown under its parent (${rows().map((r) => r.style.marginLeft)})`);
      const nestedRows = rows().filter((r) => text(r).includes(NESTED));
      t(nestedRows.length === 2, `the screen shown twice is listed twice (${nestedRows.length})`);
      t(same(nestedRows.map((r) => text(r).replace(` ${NESTED}`, "").replace(/ · page \d+/g, "").trim()).sort(), ["Subsheet_1_1", "Subsheet_1_2"]), `under its two names (${nestedRows.map((r) => text(r))})`);
      // KiCad's pane labels every sheet with a page: formatPageString(name, GetPageNumber()) -- the root (the top-level sheet) too.
      const pages = rows().map((r) => (/ · page (\S+)$/.exec(text(r)) ?? [])[1] ?? "");
      t(same(pages, ["1", "2", "3", "4", "5", "6", "7"]), `every sheet shows its page, as KiCad's pane does: the load's numbers, then the next free ones for the import (${pages})`);
      t(/page 7$/.test(text(rowAt("Subsheet_1_1"))) && /page 6$/.test(text(rowAt("Subsheet_1_2"))), `the imported sheet's two get the lowest free pages in the walk, not the QA file's 4 and 2 (AddNewSheetInstances) (${[text(rowAt("Subsheet_1_1")), text(rowAt("Subsheet_1_2"))]})`);
      t(same(sheets.map((h) => h.page), ["1", "2", "3", "4", "5", "6", "7"]), `GET /api/sch/hierarchy gives the same pages in the walk (${sheets.map((h) => `${h.name || "root"}:${h.page}`)})`);
      // the derived .kicad_sch gives each sheet the page the pane shows (saveSheet's `(instances (project .. (path .. (page ..))))`)
      const files = (await json("/api/schematic.kicad_sch")).files ?? [];
      const pageInFiles = (name) => {
        for (const f of files) for (const block of f.text.split(/\n\t\(sheet\n/).slice(1)) if (block.includes(`"Sheetname" "${name}"`)) return (/\(page "([^"]*)"\)/.exec(block) ?? [])[1];
        return undefined;
      };
      const written = sheets.slice(1).map((h) => pageInFiles(h.name));
      t(same(written, sheets.slice(1).map((h) => h.page)), `the derived schematic writes those pages (${sheets.slice(1).map((h, i) => `${h.name}:${written[i]}`)})`);
    });

    // ----- the page order: a page set on one sheet moves it among its siblings
    await scenario("A page number set on a sheet moves it in its parent's list, as KiCad sorts by page; undo puts it back", async (t) => {
      const header = sheets.find((h) => h.name === "Header (J1)");
      const r = await post({ op: "set_sheet_page", sheet: header.path[0], page: "8", path: header.path });
      t(r.ok === true, `Edit Sheet Page Number: Header (J1) is page 8 (${JSON.stringify(r).slice(0, 200)})`);
      for (let i = 0; i < 50 && !text(rows()[6]).startsWith("Header"); i++) await sleep(200);
      const after = [text(rows()[0]).split(" ")[0], "MCU (U1)", "LED channels (D1–D8)", "issue10926_1_subsheet_1", "Subsheet_1_2", "Subsheet_1_1", "Header (J1)"];
      const labels = rows().map((r) => text(r).split(/ \S+\.kicad_sch/)[0]);
      t(after.every((label, i) => labels[i]?.startsWith(label)) && /page 8$/.test(text(rows()[6])), `Header (J1), page 8, is listed after the imported sheet (page 5) and its two, though the walk visits it first (${labels})`);
      t(rows()[6]?.style.marginLeft === "10px" && rows()[4]?.style.marginLeft === "20px", "it is still a sheet of the root, and the imported sheet keeps its two under it");
      t(same((await hierarchy()).map((h) => h.name), sheets.map((h) => h.name)), "GET /api/sch/hierarchy keeps the walk order (Next / Previous sort it themselves)");
      await fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain: "schematic" }) });
      for (let i = 0; i < 50 && !text(rows()[1]).startsWith("Header"); i++) await sleep(200);
      t(text(rows()[1]).startsWith("Header (J1)") && /page 2$/.test(text(rows()[1])), `undo: Header (J1) is page 2 again, first among the modules (${text(rows()[1])})`);
    });

    // ----- a click opens the sheet
    await scenario("A click opens that sheet: the row in view is marked and the breadcrumb follows it down and back to the root", async (t) => {
      const led = sheets.find((h) => h.name.startsWith("LED"));
      const ledId = led.path[0];
      await clickRow(rowAt("LED channels"));
      const st = __eda.state();
      t(same(st.sheetPath, [ledId]), `a click opens the sheet (${st.sheetPath})`);
      const m = marked();
      t(m.filter(Boolean).length === 1 && m[rows().indexOf(rowAt("LED channels"))], `and only its row is the sheet in view (${m.map((x, i) => (x ? i : -1)).filter((i) => i >= 0)})`);
      const one = await schOf([ledId]);
      t(same(one.sheet_path?.map((p) => [p.id, p.name]), [[ledId, led.name]]) && one.file === led.file, `the breadcrumb names it, root to here (${one.sheet_path?.map((p) => p.name)})`);
      await clickRow(rowAt("Subsheet_1_2"));
      const deepPath = [imported.path[0], sub2.path[1]];
      t(same(__eda.state().sheetPath, deepPath), `a nested row opens its own placement (${__eda.state().sheetPath})`);
      const deep = await schOf(deepPath);
      t(same(deep.sheet_path?.map((p) => p.name), [imported.name, sub2.name]) && deep.file === NESTED, `the breadcrumb follows both levels (${deep.sheet_path?.map((p) => p.name)}, ${deep.file})`);
      t(marked()[rows().indexOf(rowAt("Subsheet_1_2"))], "and the nested row is the one marked");
      await clickRow(rows()[0]);
      t(same(__eda.state().sheetPath, []) && same((await sch("")).sheet_path, []) && marked()[0] && marked().filter(Boolean).length === 1, `the root row goes back to the root and its breadcrumb is empty (${__eda.state().sheetPath})`);
    });

    // ----- one screen, two placements
    await scenario("A screen shown twice: the two placements are two rows, and each opens the same screen on a path of its own", async (t) => {
      t(sub1.file === NESTED && sub2.file === NESTED && !same(sub1.path, sub2.path), `the two hierarchy entries show one file on two paths (${sub1.path} / ${sub2.path})`);
      const openOf = async (label, entry) => {
        await clickRow(rowAt(label));
        const s = await schOf(entry.path);
        return { path: __eda.state().sheetPath, file: s.file, crumbs: s.sheet_path?.map((p) => p.name) };
      };
      const a = await openOf("Subsheet_1_1", sub1);
      const b = await openOf("Subsheet_1_2", sub2);
      t(a.file === NESTED && b.file === NESTED, `both open ${NESTED} (${a.file} / ${b.file})`);
      t(same(a.path, sub1.path) && same(b.path, sub2.path), `on the path of their own placement (${a.path} / ${b.path})`);
      t(a.crumbs?.at(-1) === "Subsheet_1_1" && b.crumbs?.at(-1) === "Subsheet_1_2", `and the breadcrumb ends with each placement's name (${a.crumbs} / ${b.crumbs})`);
      await clickRow(rows()[0]);
      t(same(__eda.state().sheetPath, []), "back at the root");
    });

    // ----- r6 C6-L2: fold and unfold
    await scenario("A sheet with sheets under it folds and unfolds; Collapse All and Expand All from the right-click menu", async (t) => {
      const fold = (label) => rowAt(label)?.querySelector('[data-testid="hierarchy-fold"]');
      t(fold("issue10926_1_subsheet_1")?.getAttribute("aria-expanded") === "true" && fold("Header (J1)")?.getAttribute("aria-expanded") == null, "only a row with sheets under it has a fold mark, unfolded at first");
      fold("issue10926_1_subsheet_1")?.click();
      await sleep(400);
      t(rows().length === sheets.length - 2 && !rowAt("Subsheet_1_1") && !!rowAt("issue10926_1_subsheet_1"), `folded: its two sheets are hidden, the row stays (${rows().length} rows)`);
      fold("issue10926_1_subsheet_1")?.click();
      await sleep(400);
      t(rows().length === sheets.length, `unfolded: every row again (${rows().length})`);
      const menuItem = async (label) => {
        panel().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 40 }));
        await sleep(300);
        const item = [...document.querySelectorAll("[role=menuitem]")].find((el) => el.textContent?.trim() === label);
        if (!item) t(false, `menu items: ${[...document.querySelectorAll("[role=menuitem]")].map((el) => el.textContent)}; panel ${!!panel()}`);
        item?.click();
        await sleep(400);
        return !!item;
      };
      t(await menuItem("Collapse All"), "the menu has Collapse All");
      t(rows().length === 1, `Collapse All: the root alone (${rows().length})`);
      t(await menuItem("Expand All"), "and Expand All");
      t(rows().length === sheets.length, `Expand All: every row (${rows().length})`);
    });

    // ----- r6 C6-L1: a sheet that leaves the hierarchy leaves Back / Forward's history (`CleanHistory`)
    await scenario("Undo the import: Back no longer goes to the imported sheets", async (t) => {
      await fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain: "schematic" }) });
      for (let i = 0; i < 50 && rows().length !== 4; i++) await sleep(200);
      t(rows().length === 4, `the imported sheet and its two are gone (${rows().length} rows)`);
      await sleep(800);
      await __eda.run("eeschema.NavigateTool.back");
      await sleep(1200);
      const left = (await hierarchy()).map((h) => JSON.stringify(h.path));
      const at = __eda.state().sheetPath;
      t(left.includes(JSON.stringify(at)), `Back lands on a sheet still in the hierarchy (${JSON.stringify(at)})`);
      t(at.length === 1 && at[0] === sheets.find((h) => h.name.startsWith("LED")).path[0], `the last one left in the history before the root: LED channels (${JSON.stringify(at)})`);
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await leave();
    results.push({ name: "end: the root is in view", ok: __eda.state().sheetPath.length === 0 && !!root, notes: [`${__eda.state().sheetPath.length === 0 ? "ok" : "FAIL"}: ${JSON.stringify(__eda.state().sheetPath)}`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
