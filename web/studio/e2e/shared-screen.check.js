// A screen shown by two sheets (UI-LEFT S9): KiCad keeps a reference, a unit and the attributes' scope per sheet path (`SCH_SYMBOL_INSTANCE`), and its
// tools act on what they act on in eeschema (KiCad 8303b2ad):
//  - Annotate numbers a `SCH_REFERENCE` per symbol and path (`SCH_SHEET_LIST::GetSymbols`, `SCH_REFERENCE::Annotate`'s `SetRef( &m_sheetPath, .. )`), each path
//    by its page (`SetSheetNumber( GetPageNumberAsInt() )`, `SortByXPosition`), power symbols too, and "Current sheet only" is the path in view;
//  - a unit edit is the path in view's (`SCH_EDIT_FRAME::SelectUnit`: `SetUnitSelection( &GetCurrentSheet(), aUnit )`), the other path draws what it drew;
//  - Do not Populate goes to one new state over the units of the reference the path in view gives the symbol (`SCH_EDIT_TOOL::SetAttribute`).
//
// The board is mcu30 with KiCad's QA `issue10926_1_subsheet_1.kicad_sch` under `fixtures/` (tools/run-all-checks.sh, board=mcu30-sheet), brought in as a
// sheet through `import_sheet`: its two nested sheets Subsheet_1_1 and Subsheet_1_2 both show `issue10926_1_subsheet_1_1.kicad_sch`. A two-unit op-amp
// (`Amplifier_Operational:LM358`, A and B) is placed on that screen, so the screen has a multi-unit part on two paths.
//
// Run it in the page of a served board: `node web/studio/e2e/run-check.mjs <url> web/studio/e2e/shared-screen.check.js`. Every edit is undone.
//
// It answers through `window.__sharedScreenCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-sheet minutes=10
(async () => {
  const results = [];
  const check = (window.__sharedScreenCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  if (document.visibilityState === "hidden" && !window.__sharedScreenSched) {
    const timers = new Map();
    let next = 6e6;
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
    window.__sharedScreenSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const SOURCE = "fixtures/issue10926_1_subsheet_1.kicad_sch";
  const NESTED = "issue10926_1_subsheet_1_1.kicad_sch";
  const schOf = (ids) => json(`/api/schematic${ids.length > 0 ? `?sheet=${ids.map(encodeURIComponent).join("/")}` : ""}`);
  const hierarchy = () => json("/api/sch/hierarchy").then((r) => r.sheets);
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const undo = () => fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain: "schematic" }) }).then((r) => r.json());
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const panel = () => document.querySelector("#hierarchy-panel");
  const rows = () => [...(panel()?.querySelectorAll('[role="button"]') ?? [])];
  const text = (r) => (r?.innerText ?? "").replace(/[▾▸]/g, "").replace(/\s*\n\s*/g, " ").trim();
  const rowAt = (label) => rows().find((r) => text(r).startsWith(label));
  const waitRows = async (n) => { for (let i = 0; i < 50; i++) { if (rows().length === n) return true; await sleep(200); } return rows().length === n; };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const open = async (label, path) => { rowAt(label)?.click(); for (let i = 0; i < 40 && !same(__eda.state().sheetPath, path); i++) await sleep(250); await sleep(600); };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  /** What a sheet path shows of each symbol of the shared screen: own id, own unit -> the reference and the unit drawn there (the canvas draws these). */
  const shown = async (path) => {
    const s = await schOf(path);
    return (s.symbols ?? []).map((x) => ({ id: x.id, unit: x.unit, reference: x.reference ?? x.id, drawn: x.path_unit ?? x.unit, field: (x.fields ?? []).find((f) => f.name === "Reference")?.text, dnp: x.dnp, at: x.at }));
  };
  const powerShown = async (path) => ((await schOf(path)).power_symbols ?? []).map((p) => ({ id: p.id, reference: p.reference ?? p.id }));
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const num = (r) => Number((/(\d+)$/.exec(r) ?? [])[1] ?? NaN);

  // --------------------------------------------------------------------------------------------------------------------------- setup
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  await leave();
  const before = await hierarchy();
  const setup = await post({
    op: "import_sheet", source: SOURCE, as_sheet: true, name: "issue10926_1_subsheet_1", file: "issue10926_1_subsheet_1.kicad_sch",
    at: { x: 200_000_000, y: 100_000_000 }, size: [40_000_000, 30_000_000], keep_annotations: false, order: "y_then_x",
    options: { algo: "first_free", start_number: 1 },
  });
  const sheets = await hierarchy();
  const imported = sheets.find((h) => h.path.length === 1 && h.name === "issue10926_1_subsheet_1");
  const subs = sheets.filter((h) => h.path.length === 2 && h.path[0] === imported?.path[0] && h.file === NESTED);
  const sub1 = subs.find((h) => h.name === "Subsheet_1_1");
  const sub2 = subs.find((h) => h.name === "Subsheet_1_2");
  // the op-amp's unit A on the shared screen, through the first placement (the server takes the definition out of the library, `library_place.rs`)
  const placed = sub1 ? await post({ op: "on_sheet", sheet: sub1.path.join("/"), cmd: { op: "add_symbol", id: "U100", lib_id: "Amplifier_Operational:LM358", at: { x: 60_000_000, y: 60_000_000 }, rot_millideg: 0, value: "", footprint: "", unit: 1 } }) : null;
  if (!(setup.ok === true && sub1 && sub2 && placed?.ok === true && (await waitRows(sheets.length)))) {
    results.push({ name: "setup", ok: false, notes: [`FAIL: the QA sheet with its screen shown twice and the op-amp on it (${before.length} sheets before, import ${JSON.stringify(setup).slice(0, 200)}, place ${JSON.stringify(placed).slice(0, 300)})`] });
    check.done = true;
    return "no shared screen";
  }
  const lower = Number(sub1.page) < Number(sub2.page) ? sub1 : sub2;
  const higher = lower === sub1 ? sub2 : sub1;

  (async () => {
    // ----- Annotate, entire schematic, reset: every path numbered, each by its page
    await scenario("Annotate the entire schematic with a reset: both placements of the screen are numbered, each by its own page; power symbols too", async (t) => {
      await open("Subsheet_1_2", sub2.path);
      t(same(__eda.state().sheetPath, sub2.path), `the second placement is in view (${__eda.state().sheetPath})`);
      const r = await __eda.run("eeschema.EditorControl.annotate");
      t(r.ok !== false && !!document.querySelector(".annotate-dialog"), `Annotate Schematic opens (${JSON.stringify(r).slice(0, 120)})`);
      const radio = (label) => [...document.querySelectorAll(".annotate-dialog label")].find((l) => l.textContent.trim() === label)?.querySelector("input");
      radio("Entire schematic")?.click();
      radio("Reset existing annotations")?.click();
      await sleep(300);
      const go = [...document.querySelectorAll(".annotate-dialog button")].find((b) => b.textContent.trim() === "Annotate");
      t(!!go, "the dialog has KiCad's Annotate button");
      go?.click();
      for (let i = 0; i < 40 && !/Annotation complete\./.test(document.querySelector('[data-testid="annotate-messages"]')?.textContent ?? ""); i++) await sleep(300);
      const log = document.querySelector('[data-testid="annotate-messages"]')?.textContent ?? "";
      t(/Annotation complete\./.test(log), `the report ends with "Annotation complete." (${log.slice(-160)})`);
      [...document.querySelectorAll(".annotate-dialog button")].find((b) => b.textContent.trim() === "Close")?.click();
      await sleep(900);
      const a = await shown(lower.path);
      const b = await shown(higher.path);
      t(a.length > 1 && a.length === b.length, `one screen, the same symbols on both paths (${a.length} / ${b.length})`);
      t(a.every((x) => !/\?/.test(x.reference)) && b.every((x) => !/\?/.test(x.reference)), `every placement is annotated (${a.map((x) => x.reference)} / ${b.map((x) => x.reference)})`);
      const all = [...a, ...b].map((x) => `${x.reference}#${x.drawn}`);
      t(new Set(all).size === all.length, `no reference is handed out twice across the two paths (${all})`);
      // `SortByXPosition`: the prefix, then the sheet number, then the place: the lower page takes the lower numbers, symbol by symbol
      const pairs = a.map((x) => [x, b.find((y) => y.id === x.id && y.unit === x.unit)]);
      t(pairs.every(([x, y]) => y && x.reference.replace(/\d+$/, "") === y.reference.replace(/\d+$/, "") && num(x.reference) < num(y.reference)), `page ${lower.page} numbers below page ${higher.page} (${pairs.map(([x, y]) => `${x.reference}<${y?.reference}`)})`);
      t([...a, ...b].every((x) => x.field === x.reference), "the Reference field drawn on each sheet is that path's reference");
      // the op-amp's two placements are two parts
      const amp = pairs.find(([x]) => x.id.startsWith("U") && a.filter((y) => y.id === x.id).length >= 1 && x.at[0] === 60_000_000);
      t(!!amp && amp[0].reference !== amp[1]?.reference, `the op-amp is a part per path (${amp?.[0].reference} / ${amp?.[1]?.reference})`);
      // the power symbols: a reference per path (`SYMBOL_FILTER_ALL`)
      const pa = await powerShown(lower.path);
      const pb = await powerShown(higher.path);
      t(pa.every((p) => !pb.some((q) => q.reference === p.reference)), `the power symbols have a reference per path (${pa.map((p) => p.reference)} / ${pb.map((p) => p.reference)})`);
      const u = await undo();
      t(u.ok !== false, "one undo takes the annotation back");
    });

    // ----- Current sheet only: the path in view, not the other
    await scenario("Annotate the current sheet with a reset on one placement: that placement's references only (the sheet path in view)", async (t) => {
      const otherBefore = await shown(lower.path);
      await open(higher === sub2 ? "Subsheet_1_2" : "Subsheet_1_1", higher.path);
      const report = await json("/api/sch/annotate", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sheet: higher.path.join("/"), reset_existing: true, order: "x_then_y", options: { scope: "current_sheet", algo: "first_free" } }) });
      t(report.ok === true && Array.isArray(report.messages) && report.messages.some((m) => m.text === "Annotation complete."), `the dialog's report for the path in view (${JSON.stringify(report).slice(0, 200)})`);
      const r = await post({ op: "on_sheet", sheet: higher.path.join("/"), cmd: { op: "annotate", reset_existing: true, order: "x_then_y", options: { scope: "current_sheet", algo: "first_free" } } });
      t(r.ok === true, `Annotate, current sheet only (${JSON.stringify(r).slice(0, 160)})`);
      const other = await shown(lower.path);
      t(same(other.map((x) => x.reference), otherBefore.map((x) => x.reference)), `the other placement keeps its references (${other.map((x) => x.reference)})`);
      const here = await shown(higher.path);
      t(here.every((x) => !/\?/.test(x.reference)) && here.every((x) => !other.some((o) => o.reference === x.reference)), `the placement in view is numbered apart from the other's (${here.map((x) => x.reference)})`);
      if (r.ok === true) await undo();
    });

    // ----- a unit edit is the path in view's
    await scenario("Symbol Unit on one placement: that placement draws unit B, the other still draws unit A; undo puts it back", async (t) => {
      await open("Subsheet_1_2", sub2.path);
      const amp = (await shown(sub2.path)).find((x) => x.at[0] === 60_000_000);
      t(!!amp && amp.drawn === 1, `the op-amp draws unit A here (${JSON.stringify(amp)})`);
      await select([amp.id]);
      const r = await __eda.run("studio.SymbolUnit.select", "1:2");
      t(r.ok !== false, `Symbol Unit > B (${JSON.stringify(r).slice(0, 160)})`);
      await sleep(900);
      const here = (await shown(sub2.path)).find((x) => x.id === amp.id && x.unit === amp.unit);
      const there = (await shown(sub1.path)).find((x) => x.id === amp.id && x.unit === amp.unit);
      t(here?.drawn === 2, `this placement draws unit B (${JSON.stringify(here)})`);
      t(there?.drawn === 1, `the other placement still draws unit A (${JSON.stringify(there)})`);
      t(here?.reference === amp.reference, `and keeps its reference (${here?.reference})`);
      // the pins the placement in view has are unit B's (`GetPins( &sheet )`)
      const pins = ((await schOf(sub2.path)).symbols ?? []).find((x) => x.id === amp.id && x.unit === amp.unit)?.pins?.map((p) => p.number) ?? [];
      t(pins.includes("5") && !pins.includes("1"), `its pins are unit B's (${pins})`);
      await undo();
      await sleep(600);
      t((await shown(sub2.path)).find((x) => x.id === amp.id)?.drawn === 1, "undo: unit A again");
    });

    // ----- Do not Populate: the server's new state over the reference's units
    await scenario("Do not Populate on one placement: the symbol's own flag (shared by both paths), set and cleared by the server's new state", async (t) => {
      await open("Subsheet_1_2", sub2.path);
      const amp = (await shown(sub2.path)).find((x) => x.at[0] === 60_000_000);
      await select([amp.id]);
      await __eda.run("eeschema.EditorControl.setDNP");
      await sleep(900);
      t((await shown(sub2.path)).find((x) => x.id === amp.id)?.dnp === true, "set");
      t((await shown(sub1.path)).find((x) => x.id === amp.id)?.dnp === true, "the other placement shows it too: the flag is the symbol's (`m_DNP`), one object on a shared screen");
      await select([amp.id]);
      await __eda.run("eeschema.EditorControl.setDNP");
      await sleep(900);
      t((await shown(sub2.path)).find((x) => x.id === amp.id)?.dnp === false, "the toggle clears it again");
      await undo();
      await undo();
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await leave();
    await undo(); // the op-amp
    await undo(); // the import
    for (let i = 0; i < 50 && rows().length !== before.length; i++) await sleep(200);
    results.push({ name: "end: the board as it was", ok: rows().length === before.length, notes: [`${rows().length === before.length ? "ok" : "FAIL"}: ${rows().length} rows`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
