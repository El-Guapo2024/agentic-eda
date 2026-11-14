// A KiCad 5 legacy schematic (`.sch`) opened in the studio (UI-LEFT-2 II1, PARITY-sch.md section 90): File > Import > Non-KiCad Schematic File... with the
// project's files (the browser hands the page files, not a folder), read by the Rust port of KiCad's legacy reader (crates/kicad/src/legacy_sch.rs) and
// its libraries by kicad-cli's `sym upgrade` (crates/cli/src/legacy_import.rs).
// Run it in the page of a served scratch board (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --ui <ui>`), where <ui> is a copy of
// web/studio/dist with KiCad's QA legacy projects in `<ui>/e2e-legacy/`: complex_hierarchy/ (complex_hierarchy.sch, ampli_ht.sch, complex_hierarchy-cache.lib,
// complex_hierarchy_schlib.lib, complex_hierarchy.pro, sym-lib-table) and legacy_power4/ (legacy_power4.sch, legacy_power4-cache.lib, legacy_power4-cache.dcm,
// symbols.lib, symbols.dcm, legacy_power4.pro), from qa/data/eeschema/netlists. It needs the real kicad-cli (the libraries are converted with it). Every
// import is taken back with Undo.
//
// What it checks:
//   a KiCad 5 hierarchy (file version 5, a screen shown on two sheets, library nicknames): the schematic is replaced in one step, the root has its two sheets
//     and its symbols, the toast says "KiCad legacy schematic", and Remap Symbols is not opened (its symbols name their library);
//   Rescue Symbols on it is the symbol library table rescuer's, which finds what KiCad finds there: nothing (KiCad looks the cache up as `Lib:Name` and
//     `Lib-Name`; a KiCad 5 cache holds `Lib_Name`);
//   a KiCad 4 schematic (version 2, bare symbol names): Remap Symbols opens by itself after the import, as KiCad opens it as such a project loads, and its
//     Remap Symbols gives every symbol the library of the project's legacy list ("Symbol R mapped to symbol library 'symbols'.");
//   the legacy rescuer: on a KiCad 4 project whose libraries are gone (pic_programmer), Rescue Symbols lists every symbol "found only in cache library" and
//     writes `<project>-rescue.lib` (LEGACY_RESCUER, KiCad's legacy library writer);
//   Undo brings the old drawing back.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }.
// run-all-checks: board=ldo ui=legacy minutes=10
(async () => {
  const results = [];
  window.__legacyImportCheck = { results, done: false };
  const check = (name, ok, detail = "") => {
    results.push({ name, ok: !!ok, detail: String(detail) });
    return !!ok;
  };
  const E = window.__eda;
  const base = window.__legacyImportBase ?? "/e2e-legacy/";
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const get = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const waitFor = async (fn, ms = 30000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(100); } };
  const b64 = async (path) => {
    const r = await fetch(base + path, { cache: "no-store" });
    const bytes = new Uint8Array(await r.arrayBuffer());
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { name: path.split("/").pop(), data: btoa(s), size: r.ok ? bytes.length : 0 };
  };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === label)?.click(); await sleep(900); };
  const toastText = () => [...document.querySelectorAll("div")].find((el) => el.style.zIndex === "3000" && el.style.position === "absolute")?.textContent ?? "";
  const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
  const placedOn = (s) => (s.symbols?.length ?? 0) + (s.power_symbols?.length ?? 0);
  const errorsBefore = E.errors().length;
  // "This operation replaces the contents of the current schematic ...": answered Yes (import-files.check.js checks the question)
  const answering = setInterval(() => byId("import-question:Yes")?.click(), 100);
  const firstTab = E.state().tab;
  let appliedSch = 0;

  try {
    if (firstTab !== "schematic") await clickTab("Schematic");
    const before = await get("/api/schematic");

    // ---- a KiCad 5 hierarchy
    const ch = await Promise.all(["complex_hierarchy.sch", "ampli_ht.sch", "complex_hierarchy-cache.lib", "complex_hierarchy_schlib.lib", "complex_hierarchy.pro", "sym-lib-table"].map((f) => b64(`complex_hierarchy/${f}`)));
    check("the QA project complex_hierarchy is served to the page", ch.every((f) => f.size > 0), ch.map((f) => `${f.name}:${f.size}`).join(" "));
    await E.run("eeschema.EditorControl.importNonKicadSchematic", ch);
    const after = await waitFor(async () => { const s = await get("/api/schematic"); return s.sheets?.length === 2 ? s : null; }, 60000);
    if (after) appliedSch++;
    check("the legacy schematic replaces the schematic: the root's two sheets (ampli_ht.sch shown twice) and its symbols", !!after && after.sheets.length === 2 && placedOn(after) > 5, after ? `${after.sheets.length} sheets, ${placedOn(after)} symbols` : "not replaced");
    check("the sheets show the screen as its .kicad_sch", !!after && after.sheets.every((s) => /ampli_ht\.kicad_sch$/.test(s.file ?? s.filename ?? "")), JSON.stringify(after?.sheets?.map((s) => s.file ?? s.filename)));
    check("the toast says it is a KiCad legacy schematic", /complex_hierarchy\.sch \(KiCad legacy schematic\)/.test(toastText()), toastText());
    await sleep(500);
    check("Remap Symbols is not opened: its symbols name their library", !document.querySelector("[data-remap-dialog]"));
    const pins = (after?.symbols ?? []).flatMap((s) => s.pins ?? []).filter((p) => p.net).length;
    check("the nets of the drawing are traced", pins > 10, String(pins));

    // ---- Rescue Symbols on it: the symbol library table rescuer (its symbols name libraries). KiCad looks a symbol up in the cache as `Lib:Name`, then
    // `Lib-Name` (`RESCUE_SYMBOL_LIB_TABLE_CANDIDATE::FindRescues`), and a KiCad 5 cache holds `Lib_Name`, so with the library gone it finds neither: nothing to rescue
    const rescue = await get("/api/sch/rescue");
    check("Rescue Symbols is the table rescuer's here, and finds what KiCad finds: nothing (the cache's `Lib_Name` is not `Lib:Name` nor `Lib-Name`)", rescue.legacy === false && (rescue.candidates ?? []).length === 0, JSON.stringify({ legacy: rescue.legacy, n: rescue.candidates?.length, error: rescue.error }));

    await E.run("common.Interactive.undo");
    appliedSch--;
    const undone = await waitFor(async () => { const s = await get("/api/schematic"); return s.sheets.length === before.sheets.length && placedOn(s) === placedOn(before) ? s : null; });
    check("one Undo brings the old drawing back", !!undone);

    // ---- a KiCad 4 schematic: Remap Symbols
    const lp = await Promise.all(["legacy_power4.sch", "legacy_power4-cache.lib", "legacy_power4-cache.dcm", "symbols.lib", "symbols.dcm", "legacy_power4.pro"].map((f) => b64(`legacy_power4/${f}`)));
    check("the QA project legacy_power4 is served to the page", lp.every((f) => f.size > 0), lp.map((f) => `${f.name}:${f.size}`).join(" "));
    await E.run("eeschema.EditorControl.importNonKicadSchematic", lp);
    const four = await waitFor(async () => { const s = await get("/api/schematic"); return placedOn(s) === 6 ? s : null; }, 60000);
    if (four) appliedSch++;
    check("the version 2 schematic comes in: R1, R2 and four power symbols", !!four && four.symbols.length === 2 && four.power_symbols.length === 4, four ? `${four.symbols.length} + ${four.power_symbols.length}` : "not replaced");
    check("its symbols name no library", !!four && four.symbols.every((s) => s.lib_id === "R"), JSON.stringify(four?.symbols?.map((s) => s.lib_id)));
    const dialog = await waitFor(() => document.querySelector("[data-remap-dialog]"), 10000);
    check("Remap Symbols opens by itself after the import, as KiCad opens it as a legacy project loads", !!dialog);
    const run = await waitFor(() => { const b = document.querySelector("[data-remap-run]"); return b && !b.disabled ? b : null; }, 10000);
    check("and offers Remap Symbols (the schematic is a legacy one)", !!run);
    run?.click();
    // `OnRemapSymbols`' "one last rescue": the Project Rescue Helper when the legacy rescuer has candidates (UI-LEFT-3 S3-35), every one kept
    const helper = await waitFor(() => document.querySelector("[data-rescue-ok]:not(:disabled)") ?? document.querySelector("[data-remap-report]"), 15000);
    if (helper?.matches?.("[data-rescue-ok]")) helper.click();
    const report = await waitFor(() => document.querySelector("[data-remap-report]"), 30000);
    const text = report?.textContent ?? "";
    check("the remap gives each symbol the library of the project's legacy list, in KiCad's words", /Symbol R mapped to symbol library 'symbols'\./.test(text) && /Symbol \+3\.3V mapped to symbol library 'symbols'\./.test(text) && /Symbol library table mapping complete!/.test(text), text.slice(0, 400));
    const remapped = await waitFor(async () => { const s = await get("/api/schematic"); return s.symbols.every((x) => x.lib_id === "symbols:R") ? s : null; });
    if (remapped) appliedSch++;
    check("the symbols now name it (one undoable step)", !!remapped, JSON.stringify((await get("/api/schematic")).symbols.map((s) => s.lib_id)));
    (document.querySelector("[data-remap-dialog]")?.closest("[role=dialog]")?.querySelector("button[aria-label=Close], button[title=Close]") ?? { click() {} }).click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(300);
    await E.run("common.Interactive.undo");
    appliedSch--;
    await E.run("common.Interactive.undo");
    appliedSch--;
    const back = await waitFor(async () => { const s = await get("/api/schematic"); return s.sheets.length === before.sheets.length && placedOn(s) === placedOn(before) ? s : null; });
    check("two Undos take the remap and the import back", !!back);

    // ---- the legacy rescuer: a KiCad 4 project whose libraries are gone (pic_programmer: bare names, the 2007 cache `pic_programmer.cache.lib`)
    const pp = await Promise.all(["pic_programmer.sch", "pic_sockets.sch", "pic_programmer.cache.lib", "pic_programmer.pro"].map((f) => b64(`pic_programmer/${f}`)));
    check("the QA project pic_programmer is served to the page", pp.every((f) => f.size > 0), pp.map((f) => `${f.name}:${f.size}`).join(" "));
    await E.run("eeschema.EditorControl.importNonKicadSchematic", pp);
    const pic = await waitFor(async () => { const s = await get("/api/schematic"); return placedOn(s) > 10 ? s : null; }, 60000);
    if (pic) appliedSch++;
    check("the KiCad 4 schematic comes in", !!pic, pic ? `${placedOn(pic)} symbols` : "not replaced");
    await waitFor(() => document.querySelector("[data-remap-dialog]"), 10000);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    (document.querySelector("[data-remap-dialog]")?.closest("[role=dialog]")?.querySelector("button[aria-label=Close], button[title=Close]") ?? { click() {} }).click();
    await sleep(300);
    await E.run("eeschema.EditorControl.rescueSymbols");
    const rows = await waitFor(() => { const r = [...document.querySelectorAll("[data-rescue-candidates] tbody tr")]; return r.length > 0 ? r : null; }, 30000);
    check("Rescue Symbols lists the legacy rescuer's candidates in KiCad's words", !!rows && rows.every((r) => /Rescue symbol .* found only in cache library to /.test(r.textContent)), rows?.slice(0, 2).map((r) => r.textContent).join(" | "));
    document.querySelector("[data-rescue-ok]")?.click();
    const said = await waitFor(() => (/Rescued \d+ symbols? into .*-rescue\.lib\./.test(toastText()) ? toastText() : null), 30000);
    check("Rescue Symbols writes the legacy rescue library", !!said, toastText());
    await E.run("common.Interactive.undo");
    appliedSch--;
    const back2 = await waitFor(async () => { const s = await get("/api/schematic"); return s.sheets.length === before.sheets.length && placedOn(s) === placedOn(before) ? s : null; });
    check("Undo takes the KiCad 4 import back", !!back2);
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : e);
  } finally {
    for (let i = 0; i < appliedSch; i++) await post("/api/undo", { domain: "schematic" });
    clearInterval(answering);
    if (E.state().tab !== firstTab) await clickTab(firstTab === "pcb" ? "PCB" : firstTab === "symbol" ? "Symbol" : "Schematic");
    const now = E.errors().slice(errorsBefore).filter((x) => !/Unable to preventDefault/.test(x.message));
    check("no error was raised in the page", now.length === 0, now.map((x) => x.message).join(" | "));
    window.__legacyImportCheck.done = true;
  }
  const failed = results.filter((r) => !r.ok);
  return { ok: failed.length === 0, failed: failed.length, results };
})()
