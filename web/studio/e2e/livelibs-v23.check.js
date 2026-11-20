// Live libraries (PARITY-common.md section 34, a deliberate difference from KiCad, which sees a library table written by something else only when the project
// is opened again): a project library added on disk while the page is open -- the files an agent writes, `sym-lib-table` and a `.kicad_sym`, `fp-lib-table`
// and a `.pretty` folder -- is in the open choosers within seconds, with no reload; a library file edited on disk is too.
//   symbol      the Symbol Chooser (A) is open and has searched for the new symbol (no match); the check writes `<project>/LiveDog<s>.kicad_sym` and the
//               project's `sym-lib-table` naming it; the open chooser lists LiveDog<s>:DOG_REG with its description, no reload
//   edit        the `.kicad_sym` is written again with another description: the open chooser's row says the new one
//   reopen      closed and opened again, the chooser lists it at once (its library among the installed ones)
//   footprint   the Footprint Chooser (Place Footprint) is open on the same kind of search; the check writes `<project>/LiveDogFp<s>.pretty/DOG_PAD.kicad_mod` and
//               the project's `fp-lib-table`: the open chooser lists LiveDogFp<s>:DOG_PAD; the footprint written again in place (the folder's own time does not
//               move) gives the row its new description
//   end         the project's tables as they were, no console errors
// The files are written through the runner's `window.__runCheckWriteFile` (e2e/run-check.mjs): run it through tools/run-all-checks.sh or run-check.mjs.
// It answers through `window.__livelibsV23Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__livelibsV23Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await new Promise((r) => setTimeout(r, 250));
  const E = window.__eda;
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const run = (id, args) => E.run(id, args);
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const clickTab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1200); };
  const row = (root, id) => root?.querySelector(`[data-chooser-tree] [data-id="${id}"]`) ?? null;
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
  const write = (path, text) => (window.__runCheckWriteFile ? window.__runCheckWriteFile(path, text) : Promise.resolve("no __runCheckWriteFile: run through e2e/run-check.mjs"));
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  const s = Date.now().toString(36);
  const SYM = `LiveDog${s}`;
  const FP = `LiveDogFp${s}`;
  const errorsBefore = E.errors().length;
  const tablesBefore = await json("/api/library_tables");
  const dir = tablesBefore.project_dir;
  // a table as an agent writes it: the project's rows it had, and the new one
  const q = (v) => `"${String(v ?? "").replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  const tableText = (kind, rows) => {
    const head = kind === "symbol" ? "sym_lib_table" : "fp_lib_table";
    const lines = rows.map((r) => `  (lib (name ${q(r.nickname)})(type ${q(r.type || "KiCad")})(uri ${q(r.uri)})(options ${q(r.options)})(descr ${q(r.descr)})${r.enabled === false ? "(disabled)" : ""}${r.visible === false ? "(hidden)" : ""})`);
    return `(${head}\n  (version 7)\n${lines.map((l) => `${l}\n`).join("")})\n`;
  };
  const projectRows = (kind) => tablesBefore.tables[kind].project.rows ?? [];
  const symbolText = (descr) =>
    `(kicad_symbol_lib\n\t(version 20241209)\n\t(generator "livelibs_check")\n\t(symbol "DOG_REG"\n\t\t(exclude_from_sim no)\n\t\t(in_bom yes)\n\t\t(on_board yes)\n` +
    `\t\t(property "Reference" "U" (at 0 2.54 0) (effects (font (size 1.27 1.27))))\n\t\t(property "Value" "DOG_REG" (at 0 -2.54 0) (effects (font (size 1.27 1.27))))\n` +
    `\t\t(property "Footprint" "" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(property "Datasheet" "" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))\n` +
    `\t\t(property "Description" ${q(descr)} (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))\n\t\t(property "ki_keywords" "dog power" (at 0 0 0) (effects (font (size 1.27 1.27)) (hide yes)))\n` +
    `\t\t(symbol "DOG_REG_0_1"\n\t\t\t(rectangle (start -2.54 1.27) (end 2.54 -1.27) (stroke (width 0.254) (type default)) (fill (type background)))\n\t\t)\n` +
    `\t\t(symbol "DOG_REG_1_1"\n\t\t\t(pin power_in line (at -5.08 0 0) (length 2.54) (name "VIN" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))\n` +
    `\t\t\t(pin power_out line (at 5.08 0 180) (length 2.54) (name "VOUT" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))\n\t\t)\n\t\t(embedded_fonts no)\n\t)\n)\n`;
  const footprintText = (descr) =>
    `(footprint "DOG_PAD"\n\t(version 20241229)\n\t(generator "livelibs_check")\n\t(layer "F.Cu")\n\t(descr ${q(descr)})\n\t(tags "dog")\n\t(attr smd)\n` +
    `\t(pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))\n\t(pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))\n\t(embedded_fonts no)\n)\n`;
  const descrOf = (r) => r?.querySelector('[data-column="Description"]')?.textContent ?? r?.textContent ?? "";

  // ------------------------------------------------------------------------------------------------------------------------------------------------ symbols
  let symbolOk = false;
  await scenario("symbol: a project library written on disk shows in the open Symbol Chooser, no reload", async (t) => {
    t(!!dir, `the project folder: ${dir}`);
    await clickTab("Schematic");
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 8000);
    if (!t(root, "A opens the Symbol Chooser")) return;
    t(await search(root, "DOG_REG"), `searched before the library is there: ${status(root)}`);
    t(!row(root, `${SYM}:DOG_REG`), "no such symbol yet");
    const w1 = await write(`${dir}/${SYM}.kicad_sym`, symbolText("Dog regulator, first words"));
    const w2 = await write(`${dir}/sym-lib-table`, tableText("symbol", [...projectRows("symbol"), { nickname: SYM, type: "KiCad", uri: `\${KIPRJMOD}/${SYM}.kicad_sym`, options: "", descr: "Dog power supply" }]));
    if (!t(w1 === "ok" && w2 === "ok", `the library and the table written: ${w1} ${w2}`)) return;
    const t0 = Date.now();
    const found = await until(() => row(root, `${SYM}:DOG_REG`), 15000);
    t(found, `the open chooser lists ${SYM}:DOG_REG after ${Date.now() - t0} ms: ${status(root)}`);
    t(/Dog regulator, first words/.test(descrOf(found)), `with its description: ${descrOf(found)}`);
    const index = await json("/api/library/index?kind=symbol");
    t(index.libraries.some((l) => l.name === SYM && l.scope === "project"), "the server lists it as a project library");
    symbolOk = !!found;
  });

  await scenario("edit: the library file written again gives the open chooser's row the new description", async (t) => {
    const root = document.querySelector('[aria-label="Symbol Chooser"]');
    if (!t(symbolOk && root, "the chooser is open on the new symbol")) return;
    await sleep(50); // a modification time of its own
    const w = await write(`${dir}/${SYM}.kicad_sym`, symbolText("Dog regulator, second words"));
    if (!t(w === "ok", `written again: ${w}`)) return;
    const t0 = Date.now();
    const now = await until(() => /second words/.test(descrOf(row(root, `${SYM}:DOG_REG`))) && row(root, `${SYM}:DOG_REG`), 15000);
    t(now, `the row says the new description after ${Date.now() - t0} ms: ${descrOf(row(root, `${SYM}:DOG_REG`))}`);
  });

  await scenario("reopen: the chooser opened again lists the library and its symbol", async (t) => {
    await closeChoosers();
    await run("common.Interactive.cancel");
    await sleep(300);
    await run("eeschema.InteractiveDrawing.placeSymbol");
    const root = await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 8000);
    if (!t(root, "the chooser opens")) return;
    t(await until(() => row(root, `${SYM}`) || [...root.querySelectorAll("[data-chooser-tree] [role=treeitem]")].some((r) => r.textContent.includes(SYM)), 10000), `${SYM} is a library of the tree`);
    t(await search(root, "DOG_REG"), `searched: ${status(root)}`);
    t(await until(() => row(root, `${SYM}:DOG_REG`), 10000), `${SYM}:DOG_REG found`);
    await closeChoosers();
    await run("common.Interactive.cancel");
  });

  // ------------------------------------------------------------------------------------------------------------------------------------------------ footprints
  await scenario("footprint: a project footprint library written on disk shows in the open Footprint Chooser; an in-place edit too", async (t) => {
    await clickTab("PCB");
    if (E.state().placeFootprint && E.state().placeFootprint !== "idle") {
      await run("common.Interactive.cancel");
      await until(() => E.state().placeFootprint === "idle", 3000);
    }
    await run("pcbnew.EditorControl.placeFootprint");
    const root = await until(() => document.querySelector("[data-footprint-chooser]"), 8000);
    if (!t(root, "Place Footprint opens the Footprint Chooser")) return;
    t(await search(root, "DOG_PAD"), `searched before the library is there: ${status(root)}`);
    t(!row(root, `${FP}:DOG_PAD`), "no such footprint yet");
    const w1 = await write(`${dir}/${FP}.pretty/DOG_PAD.kicad_mod`, footprintText("Dog pad, first words"));
    const w2 = await write(`${dir}/fp-lib-table`, tableText("footprint", [...projectRows("footprint"), { nickname: FP, type: "KiCad", uri: `\${KIPRJMOD}/${FP}.pretty`, options: "", descr: "Dog footprints" }]));
    if (!t(w1 === "ok" && w2 === "ok", `the footprint and the table written: ${w1} ${w2}`)) return;
    const t0 = Date.now();
    const found = await until(() => row(root, `${FP}:DOG_PAD`), 15000);
    t(found, `the open chooser lists ${FP}:DOG_PAD after ${Date.now() - t0} ms: ${status(root)}`);
    t(/Dog pad, first words/.test(descrOf(found)), `with its description: ${descrOf(found)}`);
    if (!found) return;
    // in place: the `.pretty` folder's own time stays as it was, only the file's moves
    await sleep(50);
    const w3 = await write(`${dir}/${FP}.pretty/DOG_PAD.kicad_mod`, footprintText("Dog pad, second words"));
    if (!t(w3 === "ok", `the footprint written again: ${w3}`)) return;
    const t1 = Date.now();
    const now = await until(() => /second words/.test(descrOf(row(root, `${FP}:DOG_PAD`))), 20000);
    t(now, `the row says the new description after ${Date.now() - t1} ms: ${descrOf(row(root, `${FP}:DOG_PAD`))}`);
    await closeChoosers();
    await run("common.Interactive.cancel");
  });

  // ------------------------------------------------------------------------------------------------------------------------------------------------ end
  await scenario("end: the project's tables as they were, no console errors", async (t) => {
    await closeChoosers();
    const a = await write(`${dir}/sym-lib-table`, tableText("symbol", projectRows("symbol")));
    const b = await write(`${dir}/fp-lib-table`, tableText("footprint", projectRows("footprint")));
    t(a === "ok" && b === "ok", `the tables written back: ${a} ${b}`);
    const gone = await until(async () => { const i = await json("/api/library/index?kind=symbol"); return !i.libraries.some((l) => l.name === SYM); }, 10000);
    t(gone, `${SYM} is no library any more`);
    await sleep(2500);
    const errs = E.errors().slice(errorsBefore);
    t(errs.length === 0, `no page error: ${errs.map((e) => e.message ?? e).join(" | ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
  return check.ok;
})();
