// The embedded files (GAPS.md items 24 and 30, package P26, UI-LEFT P26; PARITY-pcb.md sections 47 and 78): the Embedded Files panel (`PANEL_EMBEDDED_FILES`) in Board Setup > Board
// Data, in Schematic Setup > Schematic Data and on the Embedded Files tab of Footprint Properties -- it edits a copy (`m_localFiles`) that the page's Apply / the dialog's OK writes
// back as one undo step (`TransferDataFromWindow`), Close asks before it drops the copy; Add Files by upload, the conflict question ("Replace" / "Reuse Existing" / "Cancel"; none for
// the same content), Remove (and its refusal of a file only a footprint holds), Export into a folder (`onExportFiles`: the File System Access API, here an in-memory folder put in
// its place, "File '%s' already exists." with Overwrite / Skip and "Do not show again"), Embed fonts taking in the fonts the board's texts use (`onFontEmbedClick`); the actions
// `common.Embed.embededFile`, `removeFile` and `extractFile`; Collect and Embed 3D Models (`pcbnew.EditorControl.collect3DModels`): the board's models embedded, named
// `kicad-embed://`, served from the board by the 3D model route, and one undo takes it back; Show Datasheet on a symbol whose datasheet is `kicad-embed://` opens the file
// (`GetAssociatedDocument`).
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
    await sleep(300);
  };
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const pendingRows = () => [...(panel()?.querySelectorAll('tbody tr[data-pending="1"]') ?? [])].map((r) => r.dataset.name);
  /** The page's Apply (Board Setup, Schematic Setup): the copy written back. */
  const applyPage = async (n = 1) => {
    const v0 = (await json("/api/version")).version ?? JSON.stringify(await json("/api/version"));
    tid("embedded-files-apply", panel())?.click();
    await until(async () => ((await json("/api/version")).version ?? JSON.stringify(await json("/api/version"))) !== v0 && pendingRows().length === 0, 10000);
    await sleep(400 * n);
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
    t(!!tid("embedded-files-add", panel()) && !!tid("embedded-files-remove", panel()) && !!tid("embedded-files-export", panel()) && !!tid("embedded-files-fonts", panel()) && !!tid("embedded-files-apply", panel()), "Add Files, Remove, Export, Embed fonts and Apply are there");
  });

  await scenario("Add Files goes into the panel's copy, and Apply writes the copy back as one undo step", async (t) => {
    await openBoardSetup();
    await upload([["notes.txt", "hello, embedded world"], ["part.pdf", "%PDF-1.4 a datasheet"]]);
    t(JSON.stringify(pendingRows()) === JSON.stringify(["notes.txt", "part.pdf"]), `the grid shows them, not applied: ${pendingRows()}`);
    t(!(await names("owner=board")).includes("notes.txt"), "the board's list is as it was until Apply (`TransferDataFromWindow`)");
    const nav = [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.includes("Embedded Files"));
    t(!!nav?.querySelector(".bs-dot"), "the page is marked not applied");
    const linkCell = panel().querySelector('tr[data-name="notes.txt"] code')?.textContent;
    t(linkCell === "kicad-embed://notes.txt", `the Embedded Reference column: ${linkCell}`);
    await applyPage();
    const l = await listing("owner=board");
    const notes = l.files.find((f) => f.name === "notes.txt");
    t(!!notes && notes.link === "kicad-embed://notes.txt" && notes.status === "ok" && notes.size === 21, `notes.txt is embedded: ${JSON.stringify(notes)}`);
    t(l.files.find((f) => f.name === "part.pdf")?.type === "datasheet", "a PDF is a datasheet");
    t(pendingRows().length === 0 && rows().includes("notes.txt") && rows().includes("part.pdf"), `the grid shows them applied: ${rows()}`);
    t((await extracted("owner=board", "notes.txt")) === "hello, embedded world", "extracted byte for byte");
    t((await extracted("owner=board", "kicad-embed://part.pdf")) === "%PDF-1.4 a datasheet", "by its link too");
    await closeDialogs();
    await undo();
    t(!(await names("owner=board")).includes("part.pdf") && !(await names("owner=board")).includes("notes.txt"), "one undo takes both back");
  });

  await scenario("Close with the copy not applied asks, and Discard drops it", async (t) => {
    await openBoardSetup();
    await upload([["dropped.txt", "never applied"]]);
    t(pendingRows().includes("dropped.txt"), "in the copy");
    button(document.querySelector(".bs-dialog .dialog-footer"), "Close")?.click();
    await sleep(300);
    const footer = document.querySelector(".bs-dialog .dialog-footer");
    t(/Not applied yet: .*Embedded Files.*Discard these changes\?/.test(footer?.textContent ?? ""), `Close asks: ${footer?.textContent}`);
    button(footer, "Discard and close")?.click();
    await sleep(500);
    t(!panel() && !(await names("owner=board")).includes("dropped.txt"), "the dialog closes and the board has no such file");
  });

  await scenario("a name already in the copy asks Replace or Reuse Existing", async (t) => {
    await openBoardSetup();
    await upload([["dup.txt", "first"]]);
    await applyPage();
    await upload([["dup.txt", "second"]]);
    const q = tid("embedded-files-question", panel());
    t(!!q && q.textContent.includes("A file named 'dup.txt' is already embedded, but the file on disk has different content.") && q.textContent.includes("Do you want to replace the embedded file with the new version?"), `KiCad's question, word for word: ${q?.textContent}`);
    t(q?.getAttribute("aria-label") === "Embedded File Conflict" && !!button(q, "Cancel"), "its title, and Replace / Reuse Existing / Cancel");
    button(q, "Reuse Existing").click();
    await sleep(500);
    t(pendingRows().length === 0, "Reuse Existing keeps the file that is there");
    // the same content again: the file that is there is reused without a question (`existingFile->data_hash == newFileHash`)
    await upload([["dup.txt", "first"]]);
    t(!tid("embedded-files-question", panel()), "the same content asks nothing");
    // an empty file is not embedded (`EMBEDDED_FILES::AddFile` fails on it)
    await upload([["empty.txt", ""]]);
    t((tid("embedded-files-message", panel())?.textContent ?? "") === "Failed to add file 'empty.txt'.", `an empty file: ${tid("embedded-files-message", panel())?.textContent}`);
    t(!rows().includes("empty.txt"), "is not added");
    await upload([["dup.txt", "second"]]);
    button(tid("embedded-files-question", panel()), "Replace").click();
    await sleep(500);
    t(pendingRows().includes("dup.txt"), "Replace puts the new content in the copy");
    // a file the copy took in, added again with the same content, asks nothing
    await upload([["dup.txt", "second"]]);
    t(!tid("embedded-files-question", panel()), "the copy's own content is compared too");
    await applyPage();
    t((await extracted("owner=board", "dup.txt")) === "second", "Apply: the new content");
    t((await names("owner=board")).filter((n) => n === "dup.txt").length === 1, "one file of that name");
    await closeDialogs();
    await undo();
    t((await extracted("owner=board", "dup.txt")) === "first", "undo: the replaced content is back");
    await undo();
    t(!(await names("owner=board")).includes("dup.txt"), "and the add is undone");
  });

  await scenario("Remove takes the selected rows out of the copy, Remove File acts on the panel, Apply removes them", async (t) => {
    await openBoardSetup();
    await upload([["a.txt", "A"], ["b.txt", "B"]]);
    await applyPage();
    panel().querySelector('tr[data-name="a.txt"]').click();
    await sleep(200);
    tid("embedded-files-remove", panel()).click();
    await sleep(300);
    t(!rows().includes("a.txt") && rows().includes("b.txt"), "the selected row leaves the grid, the other stays");
    t((await names("owner=board")).includes("a.txt"), "the board keeps it until Apply");
    panel().querySelector('tr[data-name="b.txt"]').click();
    await until(() => panel().querySelector('tr[data-name="b.txt"]')?.getAttribute("aria-selected") === "true", 5000);
    await __eda.run("common.Embed.removeFile");
    await sleep(300);
    t(!rows().includes("b.txt"), "Remove File removes the selected row of the panel showing");
    await applyPage();
    t(!(await names("owner=board")).includes("a.txt") && !(await names("owner=board")).includes("b.txt"), "Apply: both gone, one step");
    await closeDialogs();
    await undo();
    t((await names("owner=board")).includes("a.txt") && (await names("owner=board")).includes("b.txt"), "one undo brings both back");
    await undo();
    t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board), "the next: the board's list as it was");
  });

  await scenario("Export writes every file of the copy into the folder picked, and a file there asks Overwrite or Skip (Do not show again)", async (t) => {
    // the File System Access API's folder, in memory: `notes.txt` is there already
    const folder = new Map([["notes.txt", new TextEncoder().encode("old")]]);
    const realPicker = window.showDirectoryPicker;
    let picked = 0;
    window.showDirectoryPicker = async () => {
      picked++;
      return {
        async getFileHandle(name, o) {
          if (!folder.has(name) && !o?.create) throw new DOMException("not found", "NotFoundError");
          return { async createWritable() { return { async write(d) { folder.set(name, new Uint8Array(d)); }, async close() {} }; } };
        },
      };
    };
    try {
      await openBoardSetup();
      await upload([["notes.txt", "new notes"], ["more.txt", "more"]]);
      await applyPage();
      await upload([["pending.txt", "taken in, not applied"]]);
      tid("embedded-files-export", panel()).click();
      await until(() => !!tid("embedded-files-question", panel()), 5000);
      const q = tid("embedded-files-question", panel());
      t(q?.getAttribute("aria-label") === "Confirmation" && q.textContent.includes("File 'notes.txt' already exists.") && !!button(q, "Overwrite") && !!button(q, "Skip"), `KiCad's question: ${q?.textContent}`);
      t(!!tid("embedded-files-question-remember", q) && /Do not show again/.test(q.textContent), "with Do not show again");
      tid("embedded-files-question-remember", q).click();
      button(q, "Overwrite").click();
      await until(() => folder.has("pending.txt"), 5000);
      const text = (n) => new TextDecoder().decode(folder.get(n) ?? new Uint8Array());
      t(text("notes.txt") === "new notes" && text("more.txt") === "more", "the files of the list, overwritten as asked");
      t(text("pending.txt") === "taken in, not applied", "and the copy's file not applied yet (`m_localFiles`)");
      folder.set("more.txt", new TextEncoder().encode("changed"));
      tid("embedded-files-export", panel()).click();
      await sleep(800);
      t(!tid("embedded-files-question", panel()) && text("more.txt") === "more", "the remembered Overwrite: no question the second time");
      t(picked === 2, "the folder is asked for each Export");
      button(document.querySelector(".bs-dialog .dialog-footer"), "Close")?.click();
      await sleep(300);
      button(document.querySelector(".bs-dialog .dialog-footer"), "Discard and close")?.click();
      await sleep(300);
      await undo();
      t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board), "undo: the board's list as it was");
    } finally {
      window.showDirectoryPicker = realPicker;
    }
  });

  await scenario("Embed fonts takes in the fonts the board's texts use, and Apply embeds them typed font", async (t) => {
    const add = await cmd({ op: "add_text", text: { content: "EMBED-FONT-CHECK", at: { x: 5_000_000, y: 5_000_000 }, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000 } });
    t(add.ok, `a board text: ${add.message ?? ""}`);
    const text = ((await state()).drawings?.texts ?? []).find((x) => x.content === "EMBED-FONT-CHECK");
    const face = await cmd({ op: "edit_text_and_graphics", text_ids: [text?.id], rows: { font: "DejaVu Sans" } });
    t(face.ok, `set in DejaVu Sans (Edit Text & Graphics Properties' Font): ${face.message ?? ""}`);
    const used = await json("/api/embedded/fonts?owner=board");
    t(used.fonts.some((f) => f.name === "DejaVuSans.ttf" && f.permission === "installable"), `GET /api/embedded/fonts: ${JSON.stringify(used.fonts.map((f) => f.name))}`);
    await openBoardSetup();
    const box = tid("embedded-files-fonts", panel());
    const was = box.checked;
    if (was) {
      box.click();
      await sleep(500);
    }
    tid("embedded-files-fonts", panel()).click();
    await until(() => pendingRows().includes("DejaVuSans.ttf"), 5000);
    const row = panel().querySelector('tr[data-name="DejaVuSans.ttf"]');
    t(!!row && /Font/.test(row.textContent), `the font comes into the copy at once (onFontEmbedClick): ${row?.textContent}`);
    await applyPage();
    const l = await listing("owner=board");
    t(l.fonts === true && l.files.some((f) => f.name === "DejaVuSans.ttf" && f.type === "font" && f.status === "ok"), `Apply: the board embeds it, typed font: ${JSON.stringify(l.files.map((f) => [f.name, f.type]))}`);
    tid("embedded-files-fonts", panel()).click();
    await sleep(400);
    t(!rows().includes("DejaVuSans.ttf"), "unticked: the fonts leave the copy");
    await applyPage();
    t(!(await names("owner=board")).includes("DejaVuSans.ttf") && (await listing("owner=board")).fonts === false, "Apply: no font, fonts not embedded");
    await closeDialogs();
    await undo();
    await undo();
    await undo();
    await undo();
    t(JSON.stringify(await names("owner=board")) === JSON.stringify(before.board) && !((await state()).drawings?.texts ?? []).some((x) => x.content === "EMBED-FONT-CHECK"), "four undos: the font steps, the face and the text taken back");
  });

  await scenario("Footprint Properties' Embedded Files tab applies with the dialog's OK, and Board Setup lists its files as the footprint's", async (t) => {
    t(!!ref, `a placed footprint: ${ref}`);
    await select([ref]);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    await until(() => tid("footprint-properties"));
    tid("footprint-properties-tab-embedded")?.click();
    await until(() => panel());
    t(!!panel(), "the tab shows the panel");
    await upload([["own.step", "ISO-10303-21; a footprint's model"]]);
    t(pendingRows().includes("own.step") && !(await names(`owner=footprint&id=${encodeURIComponent(ref)}`)).includes("own.step"), "in the copy, not yet the footprint's");
    t(!tid("embedded-files-apply", panel()), "no Apply of its own: the dialog's OK sends it");
    // the copy outlives a tab switch
    tid("footprint-properties-tab-general")?.click();
    await sleep(300);
    tid("footprint-properties-tab-embedded")?.click();
    await until(() => panel());
    t(pendingRows().includes("own.step"), "the copy is the dialog's: back on the tab, it is there");
    tid("footprint-properties-ok").click();
    await until(async () => (await names(`owner=footprint&id=${encodeURIComponent(ref)}`)).includes("own.step"), 8000);
    t((await names(`owner=footprint&id=${encodeURIComponent(ref)}`)).includes("own.step"), "OK: the file is the footprint's");
    const board = await listing("owner=board");
    t(board.inherited.some((f) => f.name === "own.step" && f.used_by.includes(ref)), `Board Setup lists it as ${ref}'s`);
    await openBoardSetup();
    t(rows().includes("own.step") && /from/.test(panel().querySelector('tr[data-name="own.step"]').textContent), "the grid marks it");
    panel().querySelector('tr[data-name="own.step"]').click();
    await sleep(200);
    tid("embedded-files-remove", panel()).click();
    await sleep(400);
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

  await scenario("Schematic Setup has Schematic Data > Embedded Files, and Show Datasheet opens a kicad-embed:// datasheet from it", async (t) => {
    await tab("Schematic");
    await __eda.run("common.Embed.embededFile");
    await until(() => panel());
    t(!!panel(), "the panel is showing");
    t([...document.querySelectorAll(".bs-nav-group")].some((g) => g.textContent === "Schematic Data"), "under Schematic Data");
    await upload([["sheet.kicad_wks", "(kicad_wks (version 20231118))"], ["ds.pdf", "%PDF-1.4 the embedded datasheet"]]);
    await applyPage();
    const l = await listing("owner=schematic");
    t(l.files.some((f) => f.name === "sheet.kicad_wks" && f.type === "worksheet") && l.files.some((f) => f.name === "ds.pdf"), `the schematic holds them: ${JSON.stringify(l.files.map((f) => f.name))}`);
    await closeDialogs();
    // a symbol whose datasheet names the file (`GetAssociatedDocument`'s kicad-embed branch, the schematic's files first): on the root, or in the first sheet that has one
    const root = await json("/api/schematic");
    const real = (list) => (list ?? []).find((x) => x.id && !x.id.startsWith("#"));
    let sym = real(root.symbols);
    let sheet = null;
    for (const sh of sym ? [] : root.sheets ?? []) {
      sym = real((await json(`/api/schematic?sheet=${encodeURIComponent(sh.id)}`)).symbols);
      if (sym) {
        sheet = sh.id;
        break;
      }
    }
    const onSheet = (c) => (sheet ? { op: "on_sheet", sheet, cmd: c } : c);
    const set = await cmd(onSheet({ op: "edit_symbol_fields", id: sym?.id, datasheet: "kicad-embed://ds.pdf" }));
    t(set.ok, `${sym?.id}'s datasheet is kicad-embed://ds.pdf: ${set.message ?? ""}`);
    await sleep(900);
    if (sheet) {
      await select([sheet]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1200);
    }
    const realOpen = window.open;
    const opened = [];
    window.open = (url, target) => {
      const tab = { url, target, closed: false, opener: window, location: { href: url ?? "" }, close() { this.closed = true; } };
      opened.push(tab);
      return tab;
    };
    try {
      await select([sym.id]);
      await __eda.run("common.Control.showDatasheet");
      await until(() => opened.length > 0 && /\/api\/embedded\/doc/.test(opened[0].location.href), 5000);
      const href = opened[0]?.location.href ?? "";
      t(/\/api\/embedded\/doc\?name=kicad-embed%3A%2F%2Fds\.pdf/.test(href) && !opened[0].closed, `a tab opened on the document: ${href}`);
      const doc = href ? await fetch(href) : null;
      t(!!doc && doc.headers.get("content-type") === "application/pdf" && (await doc.text()) === "%PDF-1.4 the embedded datasheet", "the file, as a PDF");
      // a file no list has: the tab closes again and nothing else happens (KiCad opens nothing)
      opened.length = 0;
      await cmd(onSheet({ op: "edit_symbol_fields", id: sym.id, datasheet: "kicad-embed://missing.pdf" }));
      await sleep(900);
      await select([sym.id]);
      await __eda.run("common.Control.showDatasheet");
      await until(() => opened.length > 0 && opened[0].closed, 5000);
      t(opened.length === 1 && opened[0].closed, "a missing file: nothing opened");
    } finally {
      window.open = realOpen;
    }
    while ((__eda.state().sheetPath ?? []).length > 0) {
      await __eda.run("eeschema.NavigateTool.leaveSheet");
      await sleep(500);
    }
    await undo();
    await undo();
    await undo();
    await until(async () => JSON.stringify(await names("owner=schematic")) === JSON.stringify(before.schematic), 8000);
    t(JSON.stringify(await names("owner=schematic")) === JSON.stringify(before.schematic), "undo on the schematic takes the two datasheet edits and the files back");
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
