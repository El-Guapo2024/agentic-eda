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
//   the legacy rescue is reachable: the cache library came along into the board's folder, so Rescue Symbols lists the symbols whose library is not in the
//     board's tables as "found only in cache library", in KiCad's words;
//   a KiCad 4 schematic (version 2, bare symbol names): Remap Symbols opens by itself after the import, as KiCad opens it as such a project loads, and its
//     Remap Symbols gives every symbol the library of the project's legacy list ("Symbol R mapped to symbol library 'symbols'.");
//   Undo brings the old drawing back.
//
// It returns { ok, failed, results: [{ name, ok, detail }, ...] }.
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

    // ---- the legacy rescue is reachable: the cache library is the board's
    const rescue = await get("/api/sch/rescue");
    const onlyInCache = (rescue.candidates ?? []).filter((c) => /^Rescue symbol .* found only in cache library to /.test(c.action));
    check("Rescue Symbols reads the project's cache library: the symbols whose library the board's tables lack are rescued from it", rescue.legacy === false && onlyInCache.length > 0, JSON.stringify({ legacy: rescue.legacy, n: rescue.candidates?.length, first: rescue.candidates?.[0]?.action, error: rescue.error }));

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
