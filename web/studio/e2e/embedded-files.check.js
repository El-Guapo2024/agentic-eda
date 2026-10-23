// The embedded files (GAPS.md items 24 and 30, package P26; PARITY-common.md): the Embedded Files panel (`PANEL_EMBEDDED_FILES`) in Board Setup > Board Data, in Schematic Setup >
// Schematic Data and on the Embedded Files tab of Footprint Properties -- Add Files by upload, the conflict question ("Replace" / "Reuse Existing" / "Cancel"; none for the same content), Remove (and its refusal of a file
// only a footprint holds), Export (the extract route), Embed fonts -- each one undo step; the actions `common.Embed.embededFile`, `removeFile` and `extractFile`; Collect and Embed 3D
// Models (`pcbnew.EditorControl.collect3DModels`): the board's models embedded, named `kicad-embed://`, served from the board by the 3D model route, and one undo takes it back.
//
// Run it in the page of a served board with placed footprints whose models are KiCad's (`eda board new examples/mcu_board_30plus.yaml`), with the KiCad 3D model library installed --
// paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board (it adds files and undoes every step, and switches the Strict
// switch off and back on). It answers through `window.__embeddedFilesCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __embeddedFilesCheck.wait(60000)` until `.done`.
// run-all-checks: board=mcu30-placed   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__embeddedFilesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await sleep(250);
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const listing = (q) => json(`/api/embedded?${q}`);
  const names = async (q) => ((await listing(q)).files ?? []).map((f) => f.name);
  const tid = (id, root = document) => root.querySelector(`[data-testid="${id}"]`);
  const until = async (cond, ms = 15000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(step); } return false; };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const tab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(800); };
  const button = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  /** The panel that is showing (a dialog keeps the hidden pages in the tree). */
  const panel = () => [...document.querySelectorAll('[data-testid="embedded-files-panel"]')].find((p) => !p.hidden && p.offsetParent !== null) ?? null;
  const rows = () => [...(panel()?.querySelectorAll("tbody tr[data-name]") ?? [])].map((r) => r.dataset.name);
  const bytesOf = (text) => new TextEncoder().encode(text);
  /** Add Files: the chooser's answer put on the panel's file input, as a person picking `files` does. */
  const upload = async (files) => {
    const input = tid("embedded-files-input", panel());
    const dt = new DataTransfer();
    for (const [name, text] of files) dt.items.add(new File([bytesOf(text)], name));
    input.files = dt.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(300);
    // each file is its own round trip (and a question when its name is taken): wait for the grid to list the ones that went in, not a fixed time
    await until(() => files.every(([name]) => rows().includes(name)) || !!document.querySelector(".dialog .dialog .dialog-footer, [data-testid^='embedded-files-conflict']"), 8000);
    await sleep(300);
  };
  const extracted = async (q, name) => {
    const r = await fetch(`/api/embedded/file?${q}&name=${encodeURIComponent(name)}`, { cache: "no-store" });
    return r.ok ? new TextDecoder().decode(await r.arrayBuffer()) : `HTTP ${r.status}`;
  };
  const closeDialogs = async () => {
    for (let i = 0; i < 3; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].find((x) => ["Close", "Cancel", "Discard and close"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(300);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const openBoardSetup = async () => {
    await __eda.run("common.Embed.embededFile");
    await until(() => panel());
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const wasStrict = !!strict?.checked;
  if (wasStrict) strict.click();
  await tab("PCB");
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const before = { board: await names("owner=board"), schematic: await names("owner=schematic") };
  const placed = (await state()).parts.filter((p) => p.placed);
  const ref = placed[0]?.ref;

  await scenario("Embedded Files opens Board Setup on Board Data > Embedded Files with the board's list", async (t) => {
    await openBoardSetup();
    t(!!panel(), "the panel is showing");
    const nav = [...document.querySelectorAll(".bs-nav-item")].find((b) => b.getAttribute("aria-current") === "page");
    t(nav?.textContent.includes("Embedded Files"), `the page tree has it current: ${nav?.textContent}`);
    t([...document.querySelectorAll(".bs-nav-group")].some((g) => g.textContent === "Board Data"), "under Board Data");
    t(JSON.stringify(rows()) === JSON.stringify([...before.board].sort()), `the rows are the board's files: ${rows()}`);
    t(!!tid("embedded-files-add", panel()) && !!tid("embedded-files-remove", panel()) && !!tid("embedded-files-export", panel()) && !!tid("embedded-files-fonts", panel()), "Add Files, Remove, Export and Embed fonts are there");
  });

  await scenario("Add Files embeds each file, one undo step each, and Export gives back the bytes", async (t) => {
    await openBoardSetup();
    await upload([["notes.txt", "hello, embedded world"], ["part.pdf", "%PDF-1.4 a datasheet"]]);
    const l = await listing("owner=board");
    const notes = l.files.find((f) => f.name === "notes.txt");
    t(!!notes && notes.link === "kicad-embed://notes.txt" && notes.status === "ok" && notes.size === 21, `notes.txt is embedded: ${JSON.stringify(notes)}`);
    t(l.files.find((f) => f.name === "part.pdf")?.type === "datasheet", "a PDF is a datasheet");
    t(rows().includes("notes.txt") && rows().includes("part.pdf"), `the grid shows them: ${rows()}`);
    const linkCell = panel().querySelector('tr[data-name="notes.txt"] code')?.textContent;
    t(linkCell === "kicad-embed://notes.txt", `the Embedded Reference column: ${linkCell}`);
    t((await extracted("owner=board", "notes.txt")) === "hello, embedded world", "extracted byte for byte");
    t((await extracted("owner=board", "kicad-embed://part.pdf")) === "%PDF-1.4 a datasheet", "by its link too");
    // `common.Embed.extractFile` acts on the panel: a download per file, no error
    panel().querySelector('tr[data-name="notes.txt"]').click();
    await sleep(200);
    await __eda.run("common.Embed.extractFile");
    await sleep(300);
    t(!tid("embedded-files-message", panel()) || !/fail/i.test(tid("embedded-files-message", panel()).textContent), "Extract File ran on the selected row");
    await closeDialogs();
    await undo();
    t(!(await names("owner=board")).includes("part.pdf") && (await names("owner=board")).includes("notes.txt"), "one undo takes the last file back");
    await undo();
    t(!(await names("owner=board")).includes("notes.txt"), "the next takes the first");
  });

  await scenario("a name already embedded asks Replace or Reuse Existing", async (t) => {
    await openBoardSetup();
    await upload([["dup.txt", "first"]]);
    await upload([["dup.txt", "second"]]);
    const q = tid("embedded-files-question", panel());
    t(!!q && q.textContent.includes("A file named 'dup.txt' is already embedded, but the file on disk has different content.") && q.textContent.includes("Do you want to replace the embedded file with the new version?"), `KiCad's question, word for word: ${q?.textContent}`);
    t(q?.getAttribute("aria-label") === "Embedded File Conflict" && !!button(q, "Cancel"), "its title, and Replace / Reuse Existing / Cancel");
    button(q, "Reuse Existing").click();
    await sleep(700);
    t((await extracted("owner=board", "dup.txt")) === "first", "Reuse Existing keeps the file that is there");
    // the same content again: the file that is there is reused without a question (`existingFile->data_hash == newFileHash`)
    await upload([["dup.txt", "first"]]);
    t(!tid("embedded-files-question", panel()), "the same content asks nothing");
    t(!/fail|refused|already/i.test(tid("embedded-files-message", panel())?.textContent ?? ""), "and says nothing");
    // an empty file is not embedded (`EMBEDDED_FILES::AddFile` fails on it)
    await upload([["empty.txt", ""]]);
    t((tid("embedded-files-message", panel())?.textContent ?? "") === "Failed to add file 'empty.txt'.", `an empty file: ${tid("embedded-files-message", panel())?.textContent}`);
    t(!(await names("owner=board")).includes("empty.txt"), "is not added");
    await upload([["dup.txt", "second"]]);
    button(tid("embedded-files-question", panel()), "Replace").click();
    await sleep(900);
    t((await extracted("owner=board", "dup.txt")) === "second", "Replace puts the new content in");
    t((await names("owner=board")).filter((n) => n === "dup.txt").length === 1, "one file of that name");
    await closeDialogs();
    await undo();
    t((await extracted("owner=board", "dup.txt")) === "first", "undo: the replaced content is back");
    await undo();
    t(!(await names("owner=board")).includes("dup.txt"), "and the add is undone");
  });

  await scenario("Remove takes the selected rows out, and Remove File acts on the panel", async (t) => {
    await openBoardSetup();
    await upload([["a.txt", "A"], ["b.txt", "B"]]);
    panel().querySelector('tr[data-name="a.txt"]').click();
    await sleep(200);
    tid("embedded-files-remove", panel()).click();
    await until(async () => !(await names("owner=board")).includes("a.txt"), 8000);
    t(!(await names("owner=board")).includes("a.txt") && (await names("owner=board")).includes("b.txt"), "the selected row is removed, the other stays");
    panel().querySelector('tr[data-name="b.txt"]').click();
    await sleep(200);
    await __eda.run("common.Embed.removeFile");
    await until(async () => !(await names("owner=board")).includes("b.txt"), 8000);
    t(!(await names("owner=board")).includes("b.txt"), "Remove File removes the selected row of the panel showing");
    await closeDialogs();
    for (let i = 0; i < 4; i++) await undo();
    t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board), "four undos: the board's list as it was");
  });

  await scenario("Embed fonts is a step of its own", async (t) => {
    await openBoardSetup();
    const box = tid("embedded-files-fonts", panel());
    const was = box.checked;
    box.click();
    await sleep(900);
    t((await listing("owner=board")).fonts === !was, "the board's fonts flag follows the box");
    await closeDialogs();
    await undo();
    t((await listing("owner=board")).fonts === was, "undo puts it back");
  });

  await scenario("Footprint Properties has an Embedded Files tab, and Board Setup lists its files as the footprint's", async (t) => {
    t(!!ref, `a placed footprint: ${ref}`);
    await select([ref]);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    await until(() => tid("footprint-properties"));
    tid("footprint-properties-tab-embedded")?.click();
    await until(() => panel());
    t(!!panel(), "the tab shows the panel");
    await upload([["own.step", "ISO-10303-21; a footprint's model"]]);
    t((await names(`owner=footprint&id=${encodeURIComponent(ref)}`)).includes("own.step"), "the file is the footprint's");
    await closeDialogs();
    const board = await listing("owner=board");
    t(board.inherited.some((f) => f.name === "own.step" && f.used_by.includes(ref)), `Board Setup lists it as ${ref}'s`);
    await openBoardSetup();
    t(rows().includes("own.step") && /from/.test(panel().querySelector('tr[data-name="own.step"]').textContent), "the grid marks it");
    panel().querySelector('tr[data-name="own.step"]').click();
    await sleep(200);
    tid("embedded-files-remove", panel()).click();
    await sleep(600);
    t(/cannot be removed/.test(tid("embedded-files-message", panel())?.textContent ?? ""), "removing it from the board is refused");
    await closeDialogs();
    await undo();
    t(!(await names(`owner=footprint&id=${encodeURIComponent(ref)}`)).includes("own.step"), "one undo takes it back");
  });

  await scenario("Collect and Embed 3D Models embeds the board's models, names them by link, and one undo takes it back", async (t) => {
    const plan = await json("/api/embedded/collect");
    t(plan.count > 0, `there are models to embed: ${plan.count} (skipped ${plan.skipped.length})`);
    if (plan.count === 0) return;
    const modelsBefore = JSON.stringify((await state()).parts.map((p) => [p.ref, (p.models ?? []).map((m) => m.name)]));
    await __eda.run("pcbnew.EditorControl.collect3DModels");
    await until(async () => (await names("owner=board")).length > before.board.length, 30000);
    const after = await state();
    const linked = after.parts.flatMap((p) => p.models ?? []).filter((m) => m.name.startsWith("kicad-embed://"));
    t(linked.length >= plan.count, `the footprints name their models by link: ${linked.length} of ${plan.count}`);
    const files = await names("owner=board");
    t(plan.models.every((m) => m.files.every((f) => files.includes(f))), `the board holds the files: ${files.length}`);
    const one = linked[0]?.name;
    const r = one ? await fetch(`/api/3dmodel?name=${encodeURIComponent(one)}&wait=1`, { cache: "no-store" }) : null;
    t(!!r && (r.status === 200 || r.status === 202), `the 3D model route serves ${one} from the board: ${r?.status}`);
    const again = await json("/api/embedded/collect");
    t(again.count === 0, "nothing is left to collect");
    await undo();
    t(JSON.stringify((await state()).parts.map((p) => [p.ref, (p.models ?? []).map((m) => m.name)])) === modelsBefore, "one undo: the model names as they were");
    t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board), "and the files are gone");
  });

  await scenario("Schematic Setup has Schematic Data > Embedded Files on the schematic's list", async (t) => {
    await tab("Schematic");
    await __eda.run("common.Embed.embededFile");
    await until(() => panel());
    t(!!panel(), "the panel is showing");
    t([...document.querySelectorAll(".bs-nav-group")].some((g) => g.textContent === "Schematic Data"), "under Schematic Data");
    await upload([["sheet.kicad_wks", "(kicad_wks (version 20231118))"]]);
    const l = await listing("owner=schematic");
    t(l.files.some((f) => f.name === "sheet.kicad_wks" && f.type === "worksheet"), `the schematic holds it, a drawing sheet: ${JSON.stringify(l.files.map((f) => f.name))}`);
    await closeDialogs();
    await undo();
    t(JSON.stringify(await names("owner=schematic")) === JSON.stringify(before.schematic), "undo on the schematic takes it back");
    await tab("PCB");
  });

  // ------------------------------------------------------------------------------------------------------------------------ done
  await scenario("nothing stays, and the page logged no error", async (t) => {
    t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board), "the board's files are as they were");
    t(JSON.stringify(await names("owner=schematic")) === JSON.stringify(before.schematic), "the schematic's too");
    const errs = (__eda.errors?.() ?? []).slice(errorsFrom);
    t(errs.length === 0, `no console error: ${JSON.stringify(errs).slice(0, 300)}`);
  });
  if (wasStrict && strict && !strict.checked) strict.click();
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
