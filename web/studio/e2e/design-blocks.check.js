// Design blocks check (GAPS.md item 32, package P18): the Design Blocks panes of the schematic and the board editor, driven the way a person does it through `window.__eda`, the pane's
// own controls (their `data-testid`s) and the dialogs' controls, and checked against what the server holds (`/api/design_blocks`, a block's files, the schematic and the board).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), with a machine
// that has kicad-cli (the preview is its drawing) -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it makes one design
// block library (a `.kicad_blocks` folder in the board's folder and a row in its design-block-lib-table), saves blocks into it, places them and deletes them again. Every edit it makes to the
// design is undone, the blocks are deleted through the pane, and the library's row is taken out through Manage Design Block Libraries (the empty folder stays). Strict is left off while it
// runs. `window.__designBlocksOnly = /regexp/` runs only the scenarios whose names match.
//
// What it checks:
//   the actions: every Design Blocks action is wired on its editor's tab and not on the other's; View > Design Blocks shows and hides the pane;
//   New Library: the dialog makes the library, its row is the one selected, the server lists it writable;
//   Save Current Sheet as Design Block: the dialog's defaults (the sheet's name), its checks (a name with a '/', a '.', a duplicate field), what reaches the files (description, keywords,
//     default fields, the sheet's schematic), the tree, the details and kicad-cli's preview; a sheet with sheets in it is refused in KiCad's words;
//   Save Selection as Design Block: the items go in a group named after the block and linked to it, the group is selected, one undo step; Update from Selection asks before it overwrites;
//   Place Design Block: contents grouped and linked, renumbered, one undo step; Place repeated copies numbers the groups; as a sheet: a sheet in a linked group; Place Linked and Save to
//     Linked from a linked group; Update from Sheet asks, shows the dialog with the name locked; Properties renames; Delete asks;
//   Group Properties' "Library link:" (an invalid one is refused in KiCad's words, a valid one is kept, empty clears it);
//   the board: Save Board / Save Selection as Design Block, Place Design Block carried by the Move tool and dropped as one undo step (grouped, linked, a lone drop for repeated copies that goes
//     on and ends on Escape), Place Linked, Save to Linked, Delete;
//   the tree: the search keeps the blocks that match, a double click places and fills Recently Used, the context menu has KiCad's entries.
//
// It answers through `window.__designBlocksCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a few minutes (every edit waits for the server's answer, the previews wait for kicad-cli): read it with `await __designBlocksCheck.wait(30000)` (it returns when the run is done
// or the time is up) until `.done`. In a hidden pane the browser stops timers and animation frames; the script then drives both from a fetch loop. A hidden pane also tells the canvas its size
// only after a screenshot: take one after loading the page, or the view the script works from has no size and the setup says so.
// run-all-checks: board=mcu30 minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__designBlocksCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, passed: results.filter((r) => r.ok).map((r) => r.name.slice(0, 40)), failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__designBlocksSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 3e6;
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
    window.__designBlocksSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const LIB = `DbCheck${Date.now().toString(36)}`; // a name no earlier run used: the empty library folder stays in the board's folder
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const board = () => json("/api/state");
  const listing = () => json("/api/design_blocks");
  const block = async (name) => (await listing()).libraries.find((l) => l.name === LIB)?.blocks.find((b) => b.name === name);
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const undo = (domain) => post("/api/undo", { domain });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialog = () => [...document.querySelectorAll(".dialog-backdrop .dialog")].pop();
  const buttonOf = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const press = async (text, ms = 900) => { buttonOf(dialog(), text)?.click(); await sleep(ms); };
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const titles = () => E.state().dialogs;
  const pane = () => document.querySelector('[data-testid="db-pane"]');
  const rowOf = (id) => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].find((e) => (e.getAttribute("data-id") ?? e.getAttribute("data-library")) === id);
  const rows = () => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].map((e) => e.getAttribute("data-id") ?? e.getAttribute("data-library"));
  const option = async (key, on) => { const b = document.querySelector(`[data-testid="db-opt-${key}"]`); if (b && b.checked !== on) { b.click(); await sleep(150); } };
  /** Select a block of the check's library in the tree (a click on the library's row opens it when it is folded). */
  const chooseBlock = async (name) => {
    let r = rowOf(`${LIB}:${name}`);
    if (!r) { rowOf(LIB)?.click(); await sleep(300); r = await waitFor(() => rowOf(`${LIB}:${name}`)); }
    r?.click();
    await sleep(350);
    return !!r;
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = E.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y, ms = 1800) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(150); fire("pointermove", px + 3, py + 3); await sleep(150); fire("pointerdown", px + 3, py + 3, { buttons: 1 }); await sleep(70); fire("pointerup", px + 3, py + 3); await sleep(ms); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(500); };
  const gotoTab = async (label) => { if (E.state().tab !== label.toLowerCase()) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === label)?.click(); await sleep(1400); } };
  const leaveSheets = async () => { while (E.state().sheetPath.length > 0) { await E.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leaveSheets(); await select([id]); await E.run("eeschema.NavigateTool.enterSheet"); await sleep(1000); await select([]); };
  const menuLabels = () => [...document.querySelectorAll(".menubar-dropdown > *")].map((c) => (c.textContent || "").trim() || "-");
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(150); };
  const contextMenuOf = async (id) => { rowOf(id).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 1200, clientY: 300, button: 2 })); await sleep(300); const l = menuLabels(); await closeMenu(); return l; };
  const toastText = () => document.body.innerText;
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const d = dialog(); if (!d) break; (buttonOf(d, "Cancel") ?? buttonOf(d, "Close"))?.click(); await sleep(200); } };
  /** The Save / Overwrite dialog of a save: `fields` fill the properties dialog; an overwrite question is answered with `overwrite`. */
  const fillProperties = async ({ name, description, keywords, fields } = {}) => {
    const d = await waitFor(() => (titles().includes("Design Block Properties") ? dialog() : null), 8000);
    if (!d) return null;
    if (name !== undefined) setValue(d.querySelector("#db-name"), name);
    if (keywords !== undefined) setValue(d.querySelector("#db-keywords"), keywords);
    if (description !== undefined) setValue(d.querySelector("#db-description"), description);
    for (const [i, f] of (fields ?? []).entries()) {
      d.querySelector('[aria-label="Add field"]').click();
      await sleep(120);
      setValue(d.querySelector(`[aria-label="Name of field ${i + 1}"]`), f[0]);
      setValue(d.querySelector(`[aria-label="Value of field ${i + 1}"]`), f[1]);
    }
    await sleep(150);
    return d;
  };
  /** Everything a scenario changes in the design, to compare with how it was. */
  const snapshotSch = async () => {
    const of = (s) => ({ sym: s.symbols.map((x) => x.id).sort(), groups: (s.groups ?? []).map((g) => [g.id, g.name, g.design_block ?? "", [...g.member_ids].sort()]), sheets: s.sheets.map((x) => x.id) });
    return JSON.stringify([of(await sch("")), of(await sch(MCU))]);
  };
  const snapshotPcb = async () => { const b = await board(); return JSON.stringify({ parts: b.parts.map((p) => [p.ref, p.at, p.placed]), groups: (b.drawings?.groups ?? []).map((g) => [g.id, g.name, g.design_block ?? "", g.member_ids]) }); };
  const groupsOfBoard = async () => (await board()).drawings?.groups ?? [];

  const only = window.__designBlocksOnly;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const notes = [];
    const t = (ok, text) => { notes.push(`${ok ? "ok" : "FAIL"}: ${text}`); return ok; };
    const t0 = Date.now();
    try { await fn(t); } catch (e) { t(false, `threw ${e && e.stack ? e.stack.split("\n").slice(0, 3).join(" | ") : e}`); }
    await closeDialogs();
    await escape();
    results.push({ name, ok: notes.every((n) => n.startsWith("ok")), notes, ms: Date.now() - t0 });
  };
  const ensureSame = async (domain, snap, take) => { for (let i = 0; i < 8 && (await take()) !== snap; i++) { await undo(domain); await sleep(500); } return (await take()) === snap; };

  // ------------------------------------------------------------------------------------------------------------------------ setup
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const errorsBefore = E.errors().length;
  await gotoTab("Schematic");
  await leaveSheets();
  const root = await sch("");
  const MCU = root.sheets.find((s) => s.name.startsWith("MCU"))?.id;
  if (!MCU) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the MCU sheet of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no board"; }
  const view0 = E.state().view;
  if (!view0 || !Number.isFinite(view0.x) || !Number.isFinite(view0.scale) || view0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)}): in a hidden pane take a screenshot after loading the page`] }); check.done = true; return "no canvas size"; }
  const base0 = await snapshotSch();
  // the board's parts the scenarios place (the example board comes with none placed). U1 at the middle of the board `eda board new` sizes from the
  // parts (34 mm square with the installed footprints' courtyards): the installed SOIC-16's courtyard reaches 5.2 mm from its centre.
  const PLACE = [["U1", 17_000_000, 17_000_000], ["C1", 22_000_000, 24_000_000], ["C2", 24_500_000, 24_000_000]];

  (async () => {
    // ======================================================================================================================== the actions
    await scenario("the actions are wired on their own editor's tab", async (t) => {
      const want = {
        schematic: [
          "eeschema.SchDesignBlockControl.showDesignBlockPanel", "eeschema.SchDesignBlockControl.saveSheetAsDesignBlock", "eeschema.SchDesignBlockControl.saveSelectionAsDesignBlock",
          "eeschema.SchDesignBlockControl.updateDesignBlockFromSheet", "eeschema.SchDesignBlockControl.updateDesignBlockFromSelection", "eeschema.SchDesignBlockControl.editDesignBlockProperties",
          "eeschema.SchDesignBlockControl.deleteDesignBlock", "eeschema.InteractiveDrawing.placeDesignBlock", "eeschema.InteractiveDrawing.placeLinkedDesignBlock",
          "eeschema.InteractiveDrawing.saveToLinkedDesignBlock", "eeschema.InteractiveDrawing.drawSheetFromDesignBlock",
        ],
        pcb: [
          "pcbnew.PcbDesignBlockControl.showDesignBlockPanel", "pcbnew.PcbDesignBlockControl.saveBoardAsDesignBlock", "pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock",
          "pcbnew.PcbDesignBlockControl.updateDesignBlockFromBoard", "pcbnew.PcbDesignBlockControl.updateDesignBlockFromSelection", "pcbnew.PcbDesignBlockControl.editDesignBlockProperties",
          "pcbnew.PcbDesignBlockControl.deleteDesignBlock", "pcbnew.InteractiveDrawing.placeDesignBlock", "pcbnew.InteractiveDrawing.placeLinkedDesignBlock", "pcbnew.InteractiveDrawing.saveToLinkedDesignBlock",
        ],
      };
      for (const tab of ["schematic", "pcb"]) {
        await gotoTab(tab === "pcb" ? "PCB" : "Schematic");
        const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
        const bad = want[tab].filter((id) => !acts[id]?.enabled);
        t(bad.length === 0, `on the ${tab} tab every action of its editor is wired${bad.length ? ` (not: ${bad})` : ""}`);
        const other = want[tab === "pcb" ? "schematic" : "pcb"].filter((id) => acts[id]?.enabled);
        t(other.length === 0, `... and none of the other editor's${other.length ? ` (enabled: ${other})` : ""}`);
      }
      const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      t(acts["pcbnew.InteractiveDrawing.applyDesignBlockLayout"]?.enabled === true, "Apply Design Block Layout is wired on the PCB tab (the multichannel tool, package P24; e2e/multichannel.check.js drives it)");
      await gotoTab("Schematic");
    });

    // ======================================================================================================================== the pane and New Library
    await scenario("View > Design Blocks shows the pane and New Library makes a library that is the one selected", async (t) => {
      await gotoTab("Schematic");
      if (!pane()) await E.run("eeschema.SchDesignBlockControl.showDesignBlockPanel");
      await waitFor(() => pane());
      t(!!pane() && pane().getAttribute("data-editor") === "schematic", "the action shows the schematic's pane");
      t(!!document.querySelector('[data-testid="db-search"]') && !!document.querySelector('[data-testid="db-opt-asGroup"]'), "it has the search box and the check boxes");
      t(document.querySelector('[data-testid="db-opt-asGroup"]')?.checked === true && document.querySelector('[data-testid="db-opt-repeated"]')?.checked === false, "Place as group is on and Place repeated copies off to begin with");
      t(!!document.querySelector('[data-testid="db-opt-asSheet"]') && !!document.querySelector('[data-testid="db-opt-keepAnnotations"]'), "the schematic's pane has Place as sheet and Keep annotations");
      await E.run("common.Control.newLibrary");
      const d = await waitFor(() => (titles().includes("New Design Block Library") ? dialog() : null));
      t(!!d, "New Library opens the New Design Block Library dialog");
      if (d) {
        const ins = d.querySelectorAll("input");
        setValue(ins[1], LIB);
        setValue(ins[2], "Design blocks check");
        await sleep(200);
        await press("Create", 1800);
      }
      const lib = (await listing()).libraries.find((l) => l.name === LIB);
      t(!!lib && lib.writable === true && lib.scope === "project", `the server lists the library, a project one, writable ${JSON.stringify(lib && { w: lib.writable, s: lib.scope })}`);
      t(!!rowOf(LIB) && rowOf(LIB).textContent.includes("Design blocks check"), "the tree has its row with the description");
      t(pane().innerText.includes(`Library ${LIB}`), "the new library is the one selected");
    });

    // ======================================================================================================================== Save Current Sheet as Design Block
    await scenario("Save Current Sheet as Design Block: the dialog, its checks, the files, the details and kicad-cli's preview", async (t) => {
      await gotoTab("Schematic");
      await enterSheet(MCU);
      rowOf(LIB)?.click();
      await sleep(300);
      await E.run("eeschema.SchDesignBlockControl.saveSheetAsDesignBlock");
      const d = await fillProperties({});
      t(!!d, "the action opens Design Block Properties");
      t(d && /^MCU/.test(d.querySelector("#db-name").value), `the name is the sheet's (${d && d.querySelector("#db-name").value})`);
      t(d && d.querySelector("#db-name").readOnly === false, "the name can be edited");
      // KiCad's checks (`TransferDataFromWindow`, `TransferDataFromGrid`)
      setValue(d.querySelector("#db-name"), "bad/name");
      await press("Save", 300);
      t(titles().includes("Design Block Properties") && /Illegal character '\/'/.test(dialog().innerText), "a name with a '/' is refused and the dialog stays");
      setValue(dialog().querySelector("#db-name"), "bad.name");
      await press("Save", 300);
      t(titles().includes("Design Block Properties") && /Illegal character '\.'/.test(dialog().innerText), "a name with a '.' is refused (KiCad would hide the block)");
      setValue(dialog().querySelector("#db-name"), "MCU_core");
      setValue(dialog().querySelector("#db-keywords"), "mcu core decoupling");
      setValue(dialog().querySelector("#db-description"), "The MCU with its decoupling");
      dialog().querySelector('[aria-label="Add field"]').click();
      await sleep(120);
      dialog().querySelector('[aria-label="Add field"]').click();
      await sleep(120);
      setValue(dialog().querySelector('[aria-label="Name of field 1"]'), "Designer");
      setValue(dialog().querySelector('[aria-label="Value of field 1"]'), "me");
      setValue(dialog().querySelector('[aria-label="Name of field 2"]'), "Designer");
      setValue(dialog().querySelector('[aria-label="Value of field 2"]'), "you");
      await press("Save", 300);
      t(titles().includes("Design Block Properties") && /Duplicate fields are not allowed/.test(dialog().innerText), "two default fields of one name are refused");
      setValue(dialog().querySelector('[aria-label="Name of field 2"]'), "Version");
      setValue(dialog().querySelector('[aria-label="Value of field 2"]'), "1");
      await press("Save", 2500);
      t(!titles().includes("Design Block Properties"), "OK closes the dialog");
      const b = await block("MCU_core");
      t(!!b && b.description === "The MCU with its decoupling" && b.keywords === "mcu core decoupling", `the library has the block with its description and keywords (${JSON.stringify(b && [b.description, b.keywords])})`);
      t(!!b && JSON.stringify(b.fields) === JSON.stringify([{ name: "Designer", value: "me" }, { name: "Version", value: "1" }]), `... and its default fields in order (${JSON.stringify(b && b.fields)})`);
      t(!!b && b.schematic === true && b.board === false, "... a schematic and no layout");
      const f = await json(`/api/design_blocks/file?lib=${LIB}&name=MCU_core&what=sch`);
      t(f.ok && /\(kicad_sch/.test(f.text) && (f.text.match(/\(lib_id "/g) || []).length >= 13, "the schematic file is the sheet's: its symbols are in it");
      t(rowOf(`${LIB}:MCU_core`)?.getAttribute("aria-selected") === "true", "the tree selects the new block");
      const info = document.querySelector('[data-testid="db-info"]')?.innerText ?? "";
      t(/MCU_core/.test(info) && /decoupling/.test(info) && /Designer/.test(info) && /Version/.test(info), "the details show its description, keywords and fields");
      const svg = await waitFor(() => document.querySelector('[data-testid="db-preview"] svg'), 30000);
      t(!!svg && (svg.getAttribute("viewBox") || "").split(/\s+/).length === 4, "kicad-cli's drawing of the sheet is the preview, fitted to its drawing");
      // a sheet with sheets in it is refused
      await leaveSheets();
      await E.run("eeschema.SchDesignBlockControl.saveSheetAsDesignBlock");
      await sleep(1200);
      t(!titles().includes("Design Block Properties") && /Design blocks with nested sheets are not supported/.test(toastText()), "a sheet that has sheets is refused in KiCad's words");
    });

    // ======================================================================================================================== Save Selection as Design Block, Place
    await scenario("Save Selection as Design Block groups and links the selection; Place Design Block puts the block back, grouped, one undo step", async (t) => {
      await gotoTab("Schematic");
      await enterSheet(MCU);
      const inSheet = async () => (await sch(MCU));
      await select(["C1", "C2"]);
      await chooseBlock("MCU_core");
      await E.run("eeschema.SchDesignBlockControl.saveSelectionAsDesignBlock");
      let d = await fillProperties({});
      t(!!d && d.querySelector("#db-name").value !== "", `the dialog opens with a name (${d && d.querySelector("#db-name").value})`);
      await fillProperties({ name: "Pair", description: "Two capacitors", keywords: "decoupling" });
      await press("Save", 3000);
      const b = await block("Pair");
      const f = await json(`/api/design_blocks/file?lib=${LIB}&name=Pair&what=sch`);
      t(!!b && b.schematic === true && f.ok && (f.text.match(/\(lib_id "/g) || []).length === 2, "the block's schematic has the two symbols");
      let s = await inSheet();
      const g = (s.groups ?? []).find((x) => x.design_block === `${LIB}:Pair`);
      t(!!g && g.name === "Pair" && [...g.member_ids].sort().join() === "C1,C2", `the items are in a group named after the block and linked to it (${JSON.stringify(g && [g.name, g.member_ids])})`);
      t(E.state().selection.length === 1 && E.state().selection[0].id === g?.id, "the group is what is selected");
      await undo("schematic");
      await sleep(900);
      s = await inSheet();
      t((s.groups ?? []).length === 0, "one undo takes the group away (the block stays: it is a file, not a design edit)");
      // an existing name asks before it overwrites
      await select(["C1", "C2"]);
      await E.run("eeschema.SchDesignBlockControl.saveSelectionAsDesignBlock");
      await fillProperties({ name: "Pair" });
      await press("Save", 1000);
      t(titles().includes("Confirmation") && /Design block 'Pair' already exists in library/.test(dialog().innerText), "a name the library has asks 'Overwrite existing design block?'");
      await press("Cancel", 600);
      t((await inSheet()).groups.length === 0 && !!(await block("Pair")), "Cancel writes nothing and groups nothing");
      await select([]);
      // place the block: contents, grouped and linked
      await leaveSheets();
      const before = await snapshotSch();
      const symsBefore = (await sch("")).symbols.length;
      await option("asSheet", false);
      await option("asGroup", true);
      await option("repeated", false);
      await chooseBlock("Pair");
      await E.run("eeschema.InteractiveDrawing.placeDesignBlock");
      await sleep(500);
      t(E.state().tool === "sch_import_sheet", "Place Design Block arms Import Sheet's tool");
      await clickAt(40_000_000, 40_000_000, 2500);
      s = await sch("");
      const placed = (s.groups ?? []).find((x) => x.design_block === `${LIB}:Pair`);
      t(s.symbols.length === symsBefore + 2 && !!placed && placed.name === "Pair" && placed.member_ids.length >= 2, `a click places its two symbols in a group linked to the block (${s.symbols.length - symsBefore} symbols)`);
      t(placed && placed.member_ids.filter((id) => /^C\d+$/.test(id)).every((id) => !["C1", "C2"].includes(id)), "the capacitors are numbered anew");
      t(E.state().tool === "select" && E.state().selection.some((x) => x.id === placed?.id), "the tool ends and the new group is selected");
      await undo("schematic");
      await sleep(900);
      t((await snapshotSch()) === before, "one undo takes the whole placement away");
      // repeated copies: the group names end with 1, 2, ...
      await option("repeated", true);
      await chooseBlock("Pair");
      await E.run("eeschema.InteractiveDrawing.placeDesignBlock");
      await sleep(500);
      await clickAt(40_000_000, 40_000_000, 2200);
      await clickAt(80_000_000, 40_000_000, 2200);
      t(E.state().tool === "sch_import_sheet", "with Place repeated copies the tool stays armed");
      await escape();
      s = await sch("");
      const names = (s.groups ?? []).filter((x) => x.design_block === `${LIB}:Pair`).map((x) => x.name).sort();
      t(names.join() === "Pair1,Pair2", `the groups are Pair1 and Pair2 (${names})`);
      await option("repeated", false);
      t(await ensureSame("schematic", before, snapshotSch), "undoing both leaves the sheet as it was");
    });

    // ======================================================================================================================== Place as a sheet, linked
    await scenario("Place as sheet: a sheet of its own in a linked group; Place Linked and Save to Linked from the group", async (t) => {
      await gotoTab("Schematic");
      await leaveSheets();
      const before = await snapshotSch();
      await option("asSheet", true);
      await option("asGroup", true);
      await chooseBlock("Pair");
      await E.run("eeschema.InteractiveDrawing.placeDesignBlock");
      await sleep(400);
      await clickAt(60_000_000, 120_000_000, 400);
      await clickAt(140_000_000, 170_000_000, 1200);
      const dlgName = await waitFor(() => (titles().includes("Hierarchical Sheet Properties") ? dialog() : null));
      t(!!dlgName && dialog().querySelectorAll("input")[0].value === "Pair", `the sheet is named after the block (${dlgName && dialog().querySelectorAll("input")[0].value})`);
      await press("Import", 3000);
      let s = await sch("");
      const sheet = s.sheets.find((x) => x.name === "Pair");
      const g = (s.groups ?? []).find((x) => x.design_block === `${LIB}:Pair`);
      t(!!sheet && !!g && g.member_ids.length === 1 && g.member_ids[0] === sheet.id, "the sheet is alone in a group linked to the block");
      t(E.state().selection.length === 1 && E.state().selection[0].id === g?.id, "the group is what is selected");
      // Place Linked Design Block from the group
      await option("asSheet", false);
      await select([g.id]);
      await E.run("eeschema.InteractiveDrawing.placeLinkedDesignBlock");
      await sleep(600);
      t(E.state().tool === "sch_import_sheet", "Place Linked Design Block arms the tool for the block the group is linked to");
      t(rowOf(`${LIB}:Pair`)?.getAttribute("aria-selected") === "true", "... and selects that block in the pane");
      await clickAt(40_000_000, 60_000_000, 2500);
      s = await sch("");
      t((s.groups ?? []).filter((x) => x.design_block === `${LIB}:Pair`).length === 2, "the linked block is placed again in a group of its own");
      // a group with no link: nothing happens
      await escape();
      await ensureSame("schematic", before, snapshotSch);
      t((await snapshotSch()) === before, "undoing the sheet and the copy leaves the sheet as it was");
    });

    // ======================================================================================================================== Update, Properties, Delete
    await scenario("Update Design Block asks before it overwrites, the sheet's update shows the dialog with the name locked; Properties renames; Delete asks", async (t) => {
      await gotoTab("Schematic");
      await enterSheet(MCU);
      await chooseBlock("MCU_core");
      await E.run("eeschema.SchDesignBlockControl.updateDesignBlockFromSheet");
      await waitFor(() => titles().includes("Confirmation"));
      t(titles().includes("Confirmation") && /Design block 'MCU_core' already has a schematic/.test(dialog().innerText) && /Overwrite existing schematic\?/.test(dialog().innerText), "Update from Current Sheet asks 'Overwrite existing schematic?'");
      await press("Overwrite", 800);
      const d = await waitFor(() => (titles().includes("Design Block Properties") ? dialog() : null));
      t(!!d && d.querySelector("#db-name").readOnly === true && d.querySelector("#db-name").value === "MCU_core", "the dialog opens with the block's name locked");
      t(!!d && [...d.querySelectorAll('[data-testid="db-fields"] input')].map((i) => i.value).join() === "Designer,me,Version,1", "... and the block's own fields");
      setValue(d.querySelector("#db-description"), "updated from the sheet");
      await press("Save", 2500);
      t((await block("MCU_core"))?.description === "updated from the sheet", "Save writes the new description over the block");
      // Cancel at the question writes nothing
      await E.run("eeschema.SchDesignBlockControl.updateDesignBlockFromSheet");
      await waitFor(() => titles().includes("Confirmation"));
      await press("Cancel", 500);
      t(!titles().length && (await block("MCU_core"))?.description === "updated from the sheet", "Cancel at the question changes nothing");
      // Update from Selection: no dialog, the metadata stays
      await select(["C1"]);
      await E.run("eeschema.SchDesignBlockControl.updateDesignBlockFromSelection");
      await waitFor(() => titles().includes("Confirmation"));
      await press("Overwrite", 2500);
      const ub = await block("MCU_core");
      t(!!ub && ub.description === "updated from the sheet" && ub.keywords === "mcu core decoupling", "Update from Selection keeps the block's metadata");
      const uf = await json(`/api/design_blocks/file?lib=${LIB}&name=MCU_core&what=sch`);
      t(uf.ok && (uf.text.match(/\(lib_id "/g) || []).length <= 3, "... and its schematic is the selection's (one capacitor and its power symbols)");
      await select([]);
      await ensureSame("schematic", base0, snapshotSch);
      // Properties: rename; renaming over another block asks first
      await leaveSheets();
      await chooseBlock("MCU_core");
      await E.run("eeschema.SchDesignBlockControl.editDesignBlockProperties");
      let p = await fillProperties({});
      t(!!p && p.querySelector("#db-name").readOnly === false && p.querySelector("#db-name").value === "MCU_core", "Properties opens with the block's name, which can be edited");
      setValue(p.querySelector("#db-name"), "Pair");
      await press("OK", 1000);
      t(titles().includes("Confirmation") && /Design block 'Pair' already exists/.test(dialog().innerText), "a new name the library has asks to overwrite");
      await press("Cancel", 600);
      t(!!(await block("MCU_core")) && !!(await block("Pair")), "Cancel keeps both blocks");
      await E.run("eeschema.SchDesignBlockControl.editDesignBlockProperties");
      await fillProperties({ name: "MCU_core_v2", description: "renamed" });
      await press("OK", 2500);
      const names = ((await listing()).libraries.find((l) => l.name === LIB)?.blocks ?? []).map((x) => x.name).sort();
      t(names.includes("MCU_core_v2") && !names.includes("MCU_core"), `the block is saved under its new name and the old one is gone (${names})`);
      t(rowOf(`${LIB}:MCU_core_v2`)?.getAttribute("aria-selected") === "true", "the tree selects the renamed block");
      // Delete
      await chooseBlock("MCU_core_v2");
      await E.run("eeschema.SchDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      t(/Delete design block 'MCU_core_v2' in library '.*' from disk\?/.test(dialog()?.innerText ?? ""), "Delete asks 'Delete design block ... from disk?'");
      await press("Cancel", 500);
      t(!!(await block("MCU_core_v2")), "Cancel keeps it");
      await E.run("eeschema.SchDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      await press("OK", 1800);
      t(!(await block("MCU_core_v2")) && !rowOf(`${LIB}:MCU_core_v2`), "OK deletes the block's folder and the tree loses it");
      t(/Design block 'MCU_core_v2' deleted from library/.test(toastText()), "the status says so");
    });

    // ======================================================================================================================== Group Properties' library link
    await scenario("Group Properties has a Library link: an invalid one is refused, a valid one kept, empty clears it", async (t) => {
      await gotoTab("Schematic");
      await enterSheet(MCU);
      await select(["C1", "C2"]);
      await E.run("common.Interactive.group");
      await sleep(1200);
      const gid = (await sch(MCU)).groups.find((x) => [...x.member_ids].sort().join() === "C1,C2")?.id;
      t(!!gid, "a group of two capacitors");
      await select([gid]);
      await E.run("common.Groups.groupProperties");
      await waitFor(() => document.querySelector('.dialog[aria-label="Group Properties"]'));
      const dlg = () => document.querySelector('.dialog[aria-label="Group Properties"]');
      t(!!dlg() && dlg().querySelector('[aria-label="Library link"]').value === "", "the dialog has an empty 'Library link:' box for a group with no link");
      setValue(dlg().querySelector('[aria-label="Library link"]'), "nocolon");
      buttonOf(dlg(), "OK").click();
      await sleep(400);
      t(!!dlg() && /Invalid library link: 'nocolon'/.test(dlg().innerText), "a link that is no library id is refused in KiCad's words and the dialog stays");
      setValue(dlg().querySelector('[aria-label="Library link"]'), `${LIB}:Pair`);
      buttonOf(dlg(), "OK").click();
      await sleep(1500);
      t((await sch(MCU)).groups.find((x) => x.id === gid)?.design_block === `${LIB}:Pair`, "a valid link is kept on the group");
      // the group's menu has Place Linked / Save to Linked, on for a linked group
      await select([gid]);
      const names = E.actions().filter((a) => /placeLinkedDesignBlock|saveToLinkedDesignBlock/.test(a.id) && a.enabled).map((a) => a.id.split(".").pop());
      t(names.length === 2, "Place Linked Design Block and Save to Linked Design Block are enabled actions");
      await E.run("common.Groups.groupProperties");
      await waitFor(() => dlg());
      t(dlg().querySelector('[aria-label="Library link"]').value === `${LIB}:Pair`, "the box shows the link when the dialog is opened again");
      setValue(dlg().querySelector('[aria-label="Library link"]'), "");
      buttonOf(dlg(), "OK").click();
      await sleep(1500);
      t(((await sch(MCU)).groups.find((x) => x.id === gid)?.design_block ?? "") === "", "an empty box clears the link");
      await select([]);
      await ensureSame("schematic", base0, snapshotSch);
      await leaveSheets();
    });

    // ======================================================================================================================== the tree
    await scenario("the tree: the search keeps what matches, a double click places and fills Recently Used, the context menu is KiCad's", async (t) => {
      await gotoTab("Schematic");
      await leaveSheets();
      const before = await snapshotSch();
      await option("asSheet", false);
      const search = document.querySelector('[data-testid="db-search"]');
      setValue(search, "two capacitors");
      await sleep(400);
      t(rows().includes(`${LIB}:Pair`) && rows().length <= 3, `the search keeps the blocks whose description matches (${rows()})`);
      setValue(search, "zzzz-nothing");
      await sleep(400);
      t(!rows().some((r) => r.includes(":")), "a search nothing matches leaves no block");
      setValue(search, "");
      await sleep(400);
      const item = rowOf(`${LIB}:Pair`);
      item.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, detail: 2 }));
      await sleep(600);
      t(E.state().tool === "sch_import_sheet", "a double click on a block places it");
      await escape();
      t(rows().includes("-- Recently Used --"), "the block is in the Recently Used group");
      const saved = JSON.parse(localStorage.getItem("eda-studio.design-blocks") || "{}");
      t(Array.isArray(saved.history) && saved.history[0] === `${LIB}:Pair`, "... and first in the history the pane remembers");
      await select([]);
      const m1 = await contextMenuOf(`${LIB}:Pair`);
      t(m1.join("|") === "New Library...|-|Place Design Block|-|Properties...|Save Current Sheet as Design Block...|Update Design Block from Current Sheet|Delete Design Block|-|Hide Library Tree", `a block's menu with nothing selected (${m1.join("|")})`);
      await select(["C1"]);
      const m2 = await contextMenuOf(`${LIB}:Pair`);
      t(m2.includes("Save Selection as Design Block...") && m2.includes("Update Design Block from Selection"), "with a selection the two selection entries come in");
      const m3 = await contextMenuOf(LIB);
      t(m3[0] === "Pin Library" && m3.includes("New Library...") && !m3.includes("Place Design Block"), `a library's menu pins it (${m3.join("|")})`);
      await select([]);
      await ensureSame("schematic", before, snapshotSch);
    });

    // ======================================================================================================================== the board
    await scenario("the board: Save Board and Save Selection as Design Block, Place carried and dropped as one undo step, repeated copies, Place Linked, Save to Linked, Delete", async (t) => {
      await gotoTab("PCB");
      if (!pane()) await E.run("pcbnew.PcbDesignBlockControl.showDesignBlockPanel");
      await waitFor(() => pane());
      t(!!pane() && pane().getAttribute("data-editor") === "pcb", "the board editor has its own Design Blocks pane");
      t(!document.querySelector('[data-testid="db-opt-asSheet"]') && !!document.querySelector('[data-testid="db-opt-asGroup"]'), "it has no Place as sheet box");
      const b0 = await board();
      const s0 = await snapshotPcb();
      for (const [part, x, y] of PLACE) await post("/api/cmd", { cmd: { op: "place_at", part, x, y }, strict: false });
      await sleep(1200);
      const placedBase = await snapshotPcb();
      rowOf(LIB)?.click();
      await sleep(300);
      await E.run("pcbnew.PcbDesignBlockControl.saveBoardAsDesignBlock");
      const d = await fillProperties({});
      t(!!d && d.querySelector("#db-name").value !== "", `Save Board as Design Block opens the dialog (${d && d.querySelector("#db-name").value})`);
      await fillProperties({ name: "BoardAll", description: "the placed parts" });
      await press("Save", 3000);
      const bb = await block("BoardAll");
      t(!!bb && bb.board === true && bb.schematic === false, "the block has a layout and no schematic");
      const bf = await json(`/api/design_blocks/file?lib=${LIB}&name=BoardAll&what=pcb`);
      t(bf.ok && (bf.text.match(/\(footprint "/g) || []).length === 3, "its board file has the three placed footprints");
      // Save Selection
      await select(["U1", "C1"]);
      await E.run("pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock");
      await fillProperties({ name: "BoardPair" });
      await press("Save", 3000);
      let gs = ((await board()).drawings?.groups ?? []).filter((g) => g.design_block === `${LIB}:BoardPair`);
      t(gs.length === 1 && gs[0].name === "BoardPair" && [...gs[0].member_ids].sort().join() === "C1,U1", `the selection is in a group named after the block and linked to it (${JSON.stringify(gs.map((g) => [g.name, g.member_ids]))})`);
      t(E.state().selection.length === 1 && E.state().selection[0].id === gs[0]?.id, "the new group is selected");
      // Place carried and dropped
      await select([]);
      const beforePlace = await snapshotPcb();
      await option("repeated", false);
      await option("asGroup", true);
      await chooseBlock("BoardAll");
      await E.run("pcbnew.InteractiveDrawing.placeDesignBlock");
      await sleep(1500);
      t(E.state().tool === "move" && E.state().selection.length === 1 && E.state().selection[0].kind === "group", "Place Design Block puts the block on the board, grouped, carried by the Move tool");
      await clickAt(70_000_000, 40_000_000, 2200);
      let bnow = await board();
      const placed = (bnow.drawings?.groups ?? []).find((g) => g.design_block === `${LIB}:BoardAll`);
      t(!!placed && placed.name === "BoardAll" && placed.member_ids.length === 3 && bnow.parts.length === b0.parts.length + 3, `the click drops three footprints in a group linked to the block (${placed && placed.member_ids})`);
      t(E.state().tool === "select", "the tool ends with the drop");
      await undo("pcb");
      await sleep(1000);
      t((await snapshotPcb()) === beforePlace, "one undo takes the footprints and the group away");
      // repeated copies
      await option("repeated", true);
      await chooseBlock("BoardAll");
      await E.run("pcbnew.InteractiveDrawing.placeDesignBlock");
      await sleep(1500);
      await clickAt(70_000_000, 40_000_000, 2500);
      t(E.state().tool === "move", "with Place repeated copies the next copy is carried when one is dropped");
      await clickAt(100_000_000, 40_000_000, 2500);
      t(E.state().tool === "move", "... and again");
      await escape();
      await sleep(1200);
      bnow = await board();
      const copies = (bnow.drawings?.groups ?? []).filter((g) => g.design_block === `${LIB}:BoardAll`);
      t(copies.length === 2, `Escape takes the carried copy back: two copies stay (${copies.length})`);
      await option("repeated", false);
      // Place Linked and Save to Linked
      await select([copies[0].id]);
      await E.run("pcbnew.InteractiveDrawing.placeLinkedDesignBlock");
      await sleep(1800);
      t(E.state().tool === "move" && E.state().selection.length === 1, "Place Linked Design Block carries another copy of the block the group is linked to");
      await escape();
      await sleep(1000);
      t(((await board()).drawings?.groups ?? []).filter((g) => g.design_block === `${LIB}:BoardAll`).length === 2, "Escape takes it back");
      await select([copies[0].id]);
      await E.run("pcbnew.InteractiveDrawing.saveToLinkedDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      t(titles().includes("Confirmation") && /Design block 'BoardAll' already has a layout/.test(dialog().innerText) && /Overwrite existing layout\?/.test(dialog().innerText), "Save to Linked Design Block asks 'Overwrite existing layout?'");
      await press("Overwrite", 2500);
      const sf = await json(`/api/design_blocks/file?lib=${LIB}&name=BoardAll&what=pcb`);
      t(sf.ok && (sf.text.match(/\(footprint "/g) || []).length === 3, "the block's layout is the group's footprints");
      // Delete the blocks of the board
      await chooseBlock("BoardPair");
      await E.run("pcbnew.PcbDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      await press("OK", 1800);
      t(!(await block("BoardPair")), "Delete removes the block");
      // clean up the design: everything the scenario placed
      t(await ensureSame("pcb", placedBase, snapshotPcb), "undoing the copies leaves the board as the parts were placed");
      await ensureSame("pcb", s0, snapshotPcb);
      t((await snapshotPcb()) === s0, "... and the board as it was found");
    });

    // ======================================================================================================================== clean up
    await scenario("clean up: the blocks are deleted through the pane and the library's row goes out of the table", async (t) => {
      await gotoTab("Schematic");
      for (const name of ((await listing()).libraries.find((l) => l.name === LIB)?.blocks ?? []).map((b) => b.name)) {
        await chooseBlock(name);
        await E.run("eeschema.SchDesignBlockControl.deleteDesignBlock");
        await waitFor(() => titles().includes("Confirmation"));
        await press("OK", 1500);
      }
      t(((await listing()).libraries.find((l) => l.name === LIB)?.blocks ?? []).length === 0, "every block the check made is deleted");
      await E.run("common.SuiteControl.showDesignBLockLibTable");
      await waitFor(() => document.querySelector(".lt-grid"), 4000);
      for (let guard = 0; guard < 6; guard++) {
        const at = [...document.querySelectorAll('.lt-grid [data-col="nickname"]')].findIndex((i) => i.value === LIB);
        if (at < 0) break;
        document.querySelector(`.lt-grid [data-row="${at}"][data-col="nickname"]`).closest("tr").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
        await sleep(100);
        dialog().querySelector('[aria-label="Remove library from table"]')?.click();
        await sleep(150);
      }
      await press("OK", 1500);
      t(!(await listing()).libraries.some((l) => l.name === LIB), "the library's row is out of the table");
      t((await snapshotSch()) === base0, "the schematic is as the check found it");
      t(E.errors().slice(errorsBefore).filter((e) => !/nested sheets|refused because/.test(e.message)).length === 0, `no errors besides the refusals the check provoked ${JSON.stringify(E.errors().slice(errorsBefore).map((e) => e.message.slice(0, 80)))}`);
    });

    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
