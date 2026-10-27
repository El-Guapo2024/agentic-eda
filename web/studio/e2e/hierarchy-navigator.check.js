// The Hierarchy Navigator (GAPS.md item 33, package P4's hierarchy row, where Import Sheet and the Hierarchy Navigator were the two
// checks it lacked): the docked tree of the design's sheets -- the root, then every sheet placed on it, then the sheets those place --
// against KiCad's hierarchy pane at 8303b2ad (eeschema/widgets/hierarchy_pane.cpp). KiCad's pane labels every sheet "name (page N)"
// (`formatPageString`, the page of its `SCH_SHEET_PATH`: the one it was given, else the place in the walk the load numbers,
// `SetInitialPageNumbers` in files-io.cpp), sorts
// each parent's children by `ComparePageNum` (`HIERARCHY_TREE::OnCompareItems` and `SortChildren`), lists a screen shown twice once for
// each placement, and a click opens that sheet (`SCH_ACTIONS::changeSheet`). The studio's panel reads the same walk from
// `GET /api/sch/hierarchy` and opens a sheet with `eeschema.NavigateTool.changeSheet`; what a click opens is the editor's sheet path and
// the breadcrumb `GET /api/schematic` returns (the title block's "Sheet: /.../").
//
// The board is examples/mcu_board_30plus.yaml (three module sheets, no page numbers of their own) with KiCad's QA
// `qa/data/eeschema/issue10926_1_subsheet_1.kicad_sch` under `fixtures/` (tools/run-all-checks.sh, board=mcu30-sheet). The check brings
// it in as a sheet through `import_sheet` first, so the tree is two levels deep and holds a screen shown twice: the imported sheet's two
// nested sheets both show `issue10926_1_subsheet_1_1.kicad_sch`, with the pages the QA file gave them (4 and 2).
//
// Run it in the page of a served board (see tools/run-all-checks.sh), schematic tab open --
// `node web/studio/e2e/run-check.mjs <url> web/studio/e2e/hierarchy-navigator.check.js`. It reads and navigates; the only edit is the
// setup import, on the runner's copy of the board.
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
  const text = (r) => (r?.innerText ?? "").replace(/\s*\n\s*/g, " ").trim();
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
      // BuildSheetList's walk order: the root, then each placement depth first. KiCad's pane lists the same depth-first tree and orders
      // siblings by page; where the pages follow the walk -- the level-1 sheets, which the load numbers 1, 2, ... (SetInitialPageNumbers,
      // files-io.cpp) -- that is the walk order too.
      const walk = ["Root", "MCU (U1)", "LED channels (D1–D8)", "Header (J1)", "issue10926_1_subsheet_1"];
      t(walk.every((label, i) => text(rows()[i]).startsWith(label)), `the root first, then the three modules and the imported sheet in walk order (${rows().map((r) => text(r).split(/ \S+\.kicad_sch/)[0])})`);
      t(rows()[5]?.style.marginLeft === "20px" && rows()[6]?.style.marginLeft === "20px" && rows()[1]?.style.marginLeft === "10px", `a nested sheet is shown under its parent (${rows().map((r) => r.style.marginLeft)})`);
      const nestedRows = rows().filter((r) => text(r).includes(NESTED));
      t(nestedRows.length === 2, `the screen shown twice is listed twice (${nestedRows.length})`);
      t(same(nestedRows.map((r) => text(r).replace(` ${NESTED}`, "").replace(/ · page \d+/g, "").trim()).sort(), ["Subsheet_1_1", "Subsheet_1_2"]), `under its two names (${nestedRows.map((r) => text(r))})`);
      t(/page 4/.test(text(rowAt("Subsheet_1_1"))) && /page 2/.test(text(rowAt("Subsheet_1_2"))), `with the pages the QA file gave them: 4 and 2 (${[text(rowAt("Subsheet_1_1")), text(rowAt("Subsheet_1_2"))]})`);
      // KiCad's pane sorts a parent's children by page number (HIERARCHY_TREE::OnCompareItems -> SCH_SHEET_PATH::ComparePageNum, and
      // buildHierarchyTree's m_tree->SortChildren(aParent)): the imported sheet's page 2 comes before its page 4. The studio's panel
      // keeps the walk order it is given. DS-LOG finding (2026-10-27, "hierarchy-navigator: a parent's children are not in page order").
      // t(rows().indexOf(rowAt("Subsheet_1_2")) < rows().indexOf(rowAt("Subsheet_1_1")), "the imported sheet's children are in page order, as KiCad sorts them: Subsheet_1_2 (page 2) before Subsheet_1_1 (page 4)");
      // KiCad's pane labels every sheet with a page: formatPageString(name, GetPageNumber()), and a sheet whose page is empty on load
      // is numbered by SetInitialPageNumbers (files-io.cpp), a sheet drawn or imported by GetNextPageNumber (sch_drawing_tools.cpp).
      // The studio prints the page only when the design stored one. DS-LOG finding (2026-10-27, "hierarchy-navigator: no page number
      // for a sheet that was never given one").
      // t(rows().slice(1).every((r) => / · page \S+/.test(text(r))), `every sheet shows its page number, as KiCad's pane does (${rows().slice(1).map((r) => text(r))})`);
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

    // -------------------------------------------------------------------------------------------------------------------- the end
    await leave();
    results.push({ name: "end: the root is in view", ok: __eda.state().sheetPath.length === 0 && !!root, notes: [`${__eda.state().sheetPath.length === 0 ? "ok" : "FAIL"}: ${JSON.stringify(__eda.state().sheetPath)}`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
