// Library editor leftovers check (GAPS.md package P17 / items 19 and 22, PARITY-fpedit.md sections 13 and on, PARITY-symedit.md): in the Footprint Editor, Create Array on pads with
// KiCad's numbering schemes (`ARRAY_AXIS`, `ARRAY_PAD_NUMBER_PROVIDER`) and the fields grid of Footprint Properties; in the Symbol Editor, derived symbols (Derive from Existing Symbol,
// Flatten, Update Symbol Fields, the related fields table), symbol fields on the canvas with Show Hidden Fields, alternate pin functions, the symbol text box and the shape
// properties dialog; and snapping to other items' anchors in both editors.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board
// (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes library entries of its own (names that start with `P17`),
// edits and undoes them, and leaves the project library as it found it: every scenario ends by comparing the library names and undoing what it added. It answers through `window.__libEditorsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __libEditorsCheck.wait(90000)` until `.done`.
//
// It works through `window.__eda` (`actions` with `run`, `state()`, `errors()`), the dialogs' `data-testid`s, the verbs' own routes (`/api/cmd`, `/api/footprint`, `/api/symbol`) and the
// export routes (`/api/footprint/export`, `/api/symbol/export`) for what the derived KiCad files say.
// run-all-checks: board=mcu30 minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__libEditorsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const type = async (el, value) => { const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(70); };
  const click = async (el, ms = 120) => { el.click(); await sleep(ms); };
  const until = async (cond, ms = 15000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(step); } return false; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const symNames = async () => (await json("/api/symbol_editor/names")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const symbol = async (libId) => json(`/api/symbol?lib_id=${encodeURIComponent(libId)}`);
  const undoFp = async (n = 1) => { for (let i = 0; i < n; i++) { await __eda.run("common.Interactive.undo"); await sleep(500); } };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      const open = document.querySelectorAll(".dialog-backdrop");
      if (open.length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  /** `allowed`: messages (regular expressions) the scenario provokes on purpose -- the error toasts of a refusal are errors the page logs. */
  const scenario = async (name, fn, allowed = []) => {
    // `window.__libEditorsOnly = /snapping/` (set before the script is loaded) runs the scenarios whose names match, while one is being worked on.
    if (window.__libEditorsOnly && !window.__libEditorsOnly.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const padOf = (n) => ({ number: n, at: { x: 0, y: 0 }, size: [900_000, 600_000], shape: "round_rect", kind: "smd", drill: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"], roundrect_ratio: 0.25 });

  // --------------------------------------------------------------------------------------------------------------------------- run
  const libraryBefore = { fp: await fpNames(), sym: await symNames() };

  // ================================================================================================ 1. the footprint editor's pad arrays
  /** Makes a footprint of one pad, selects the pad and opens Create Array on it. Returns what the scenario needs. */
  const arrayDialogOn = async (t, pads) => {
    await clickTab("Footprint");
    t(__eda.state().tab === "footprint", "the Footprint Editor tab");
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !libraryBefore.fp.includes(n) && !made.includes(n));
    made.push(name);
    for (const p of pads) { const r = await cmd({ op: "add_pad", footprint: name, pad: p }); if (!r.ok) t(false, `add_pad: ${r.message}`); }
    await sleep(700);
    const fp = await footprint(name);
    return { name, fp, ids: fp.pads.map((p) => p.id) };
  };
  const made = [];
  /** Opens Create Array on `ids` on the Grid tab (the dialog keeps the tab it was last used on, as KiCad's does). */
  const openArray = async (ids, tab = "grid") => {
    await select(ids);
    const r = await __eda.run("pcbnew.Array.createArray");
    await sleep(450);
    tid(`array-tab-${tab}`)?.click();
    await sleep(150);
    return r;
  };
  const off = (id) => !!tid(id)?.matches(":disabled");
  const setChecked = async (id, wanted) => { const el = tid(id); if (el && el.checked !== wanted) { el.click(); await sleep(90); } };
  /** The dialog remembers what it was last used with (KiCad's `s_arrayOptions`): every scenario starts from the same entries. */
  const baseGrid = async () => {
    await setChecked("array-renumber", true);
    tid("array-duplicate").click(); tid("array-centred").click(); tid("array-direction-0").click(); await setChecked("array-reverse-numbering", false);
    tid("array-initial-1").click(); tid("array-scheme-0").click(); await sleep(90);
    for (const which of ["primary", "secondary"]) { await choose(tid(`array-${which}-kind`), "numeric"); await type(tid(`array-${which}-start`), "1"); await type(tid(`array-${which}-step`), "1"); }
  };
  const baseCircle = async () => {
    tid("array-duplicate").click(); tid("array-clockwise").click(); await setChecked("array-full-circle", false); await setChecked("array-rotate-items", false);
    tid("array-circ-initial-1").click(); await sleep(90);
    await choose(tid("array-circular-kind"), "numeric"); await type(tid("array-circular-start"), "1"); await type(tid("array-circular-step"), "1");
    await type(tid("array-count"), "4"); await type(tid("array-angle"), "90"); await type(tid("array-offset-angle"), "0");
  };
  const setGrid = async (nx, ny, dx, dy) => { await type(tid("array-nx"), nx); await type(tid("array-ny"), ny); await type(tid("array-dx"), dx); await type(tid("array-dy"), dy); };
  const pairs = (fp) => fp.pads.map((p) => [p.number, p.at.x, p.at.y]).sort((a, b) => (a[1] - b[1]) || (a[2] - b[2]));

  await scenario("Ctrl+T in the Footprint Editor opens Create Array with the pad numbering panels, without Footprint Annotation (KiCad's defaults need a freshly loaded page)", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("X")]);
    const acts = __eda.actions();
    t(acts.some((a) => a.id === "pcbnew.Array.createArray" && a.enabled), "pcbnew.Array.createArray is enabled on the Footprint tab");
    const r = await openArray(ids);
    t(/^Create Array/.test(r.dialog ?? ""), `the dialog opens: ${r.dialog}`);
    t(!!tid("array-pad-numbering-grid") && !!tid("array-renumber"), "the grid tab has Renumber pads and the numbering panel");
    t(!tid("array-unique-refs") && !tid("array-keep-refs"), "no Footprint Annotation box (the board editor's)");
    t(tid("array-renumber").checked, "Renumber pads starts on");
    t(tid("array-initial-1").checked && tid("array-scheme-0").checked && tid("array-direction-0").checked, "From start value, Continuous, Horizontal then vertical: KiCad's defaults");
    t(tid("array-primary-start").value === "1" && tid("array-primary-step").value === "1", "start 1, increment 1");
    t(off("array-secondary-kind") && off("array-secondary-start"), "the secondary axis waits for the Coordinate scheme");
    tid("array-scheme-1").click(); await sleep(100);
    t(!off("array-secondary-kind") && !off("array-secondary-start"), "the Coordinate scheme turns the secondary axis on");
    tid("array-initial-0").click(); await sleep(100);
    t(off("array-primary-kind") && off("array-primary-start") && off("array-scheme-0"), "Use first free number turns the scheme and the axis entries off");
    t(!!tid("array-pad-numbers"), "a line tells the numbers the pads will take");
    tid("array-cancel").click(); await sleep(200);
    t((await footprint(name)).pads.length === 1, "Cancel makes nothing");
  });

  await scenario("a grid is numbered by coordinates with the BGA alphabet: A1 to C4, centred on the pad, one undo step", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("X")]);
    await openArray(ids);
    await baseGrid();
    await setGrid(3, 4, "0.8", "0.8");
    tid("array-scheme-1").click(); await sleep(80);
    await choose(tid("array-primary-kind"), "alpha_no_iosqxz");
    t(tid("array-primary-start").value === "1" ? false : tid("array-primary-start").value === "A", `the new alphabet starts over at its first name: ${tid("array-primary-start").value}`);
    t(/A1, B1, C1, A2, B2, C2/.test(tid("array-pad-numbers")?.textContent ?? ""), `the preview names the points: ${tid("array-pad-numbers")?.textContent}`);
    await click(tid("array-ok"), 1100);
    t(__eda.state().dialogs.length === 0, "OK closes the dialog");
    const fp = await footprint(name);
    const got = pairs(fp).map((p) => p[0]).sort().join();
    t(got === "A1,A2,A3,A4,B1,B2,B3,B4,C1,C2,C3,C4", `twelve pads: ${got}`);
    t(same(pairs(fp)[0], ["A1", -800_000, -1_200_000]) && same(pairs(fp)[11], ["C4", 800_000, 1_200_000]), `centred: ${JSON.stringify(pairs(fp)[0])} .. ${JSON.stringify(pairs(fp)[11])}`);
    // the derived file says the same
    const mod = await (await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`)).text();
    t((mod.match(/\(pad "/g) ?? []).length === 12 && /\(pad "B3" smd roundrect/.test(mod), "the exported .kicad_mod has the twelve pads");
    await undoFp();
    t((await footprint(name)).pads.length === 1, "one undo of the Footprint Editor takes the array away");
  });

  await scenario("Use first free number skips the pads that stay; Renumber pads off keeps the number of the source", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("1"), padOf("3"), { ...padOf("9"), at: { x: 0, y: 3_000_000 } }]);
    const nine = (await footprint(name)).pads.find((p) => p.number === "9").id;
    await openArray([nine]);
    await baseGrid();
    await setGrid(3, 1, "1", "1");
    tid("array-initial-0").click(); await sleep(100);
    t(/2, 4, 5/.test(tid("array-pad-numbers")?.textContent ?? ""), `the preview skips 1 and 3: ${tid("array-pad-numbers")?.textContent}`);
    await click(tid("array-ok"), 1000);
    let nums = (await footprint(name)).pads.map((p) => p.number).sort().join();
    t(nums === "1,2,3,4,5", `pads 2, 4 and 5 are the new ones: ${nums}`);
    await undoFp();
    // Renumber pads off
    await openArray([nine]);
    await baseGrid();
    await setGrid(3, 1, "1", "1");
    await setChecked("array-renumber", false);
    t(off("array-initial-0") && off("array-direction-0") && off("array-reverse-numbering"), "with Renumber pads off the numbering controls are off");
    await click(tid("array-ok"), 1000);
    nums = (await footprint(name)).pads.map((p) => p.number).sort().join();
    t(nums === "1,3,9,9,9", `the copies keep the 9: ${nums}`);
    await undoFp();
    void ids;
  });

  await scenario("a circular array turns the pads with their points and numbers them around the circle", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [{ ...padOf("X"), at: { x: 5_000_000, y: 0 } }]);
    await openArray(ids, "circular");
    t(!!tid("array-pad-numbering-circular"), "the circular tab has its Numbering Options");
    await baseCircle();
    await type(tid("array-center-x"), "0"); await type(tid("array-center-y"), "0");
    await setChecked("array-rotate-items", true);
    await choose(tid("array-circular-kind"), "alpha_full");
    await click(tid("array-ok"), 1000);
    const fp = await footprint(name);
    const got = fp.pads.map((p) => [p.number, p.at.x, p.at.y, p.rot ?? 0]).sort();
    t(same(got, [["A", 5_000_000, 0, 0], ["B", 0, 5_000_000, 270000], ["C", -5_000_000, 0, 180000], ["D", 0, -5_000_000, 90000]]), `around the circle: ${JSON.stringify(got)}`);
    await undoFp();
  });

  await scenario("the dialog names a start the alphabet lacks and keeps the dialog open; Arrange selection numbers nothing", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("1"), { ...padOf("2"), at: { x: 0, y: 4_000_000 } }]);
    await openArray(ids);
    await baseGrid();
    await setGrid(2, 1, "1", "1");
    await choose(tid("array-primary-kind"), "hex");
    await type(tid("array-primary-start"), "G");
    await click(tid("array-ok"), 400);
    t(/Could not determine numbering start from 'G': expected value consistent with alphabet '0123456789ABCDEF'\./.test(tid("array-error")?.textContent ?? ""), `the dialog's message: ${tid("array-error")?.textContent}`);
    t(!!tid("create-array"), "the dialog stays");
    t((await footprint(name)).pads.length === 2, "nothing was made");
    await type(tid("array-primary-start"), "A");
    tid("array-arrange").click(); tid("array-in-place").click(); await sleep(100);
    await click(tid("array-ok"), 1000);
    const fp = await footprint(name);
    t(fp.pads.length === 2 && same(pairs(fp), [["1", 0, 0], ["2", 1_000_000, 0]]), `the second pad is on the second point, numbers kept: ${JSON.stringify(pairs(fp))}`);
    await undoFp();
  });

  // ================================================================================================ 2. derived symbols
  const madeSyms = [];
  const itemEl = (libId) => document.querySelector(`[role="treeitem"][data-name="${libId}"]`);
  /** A symbol with a body, two pins and fields, made with the verbs (what the person would have drawn). */
  const makeBase = async (t, libId, extra = {}) => {
    madeSyms.push(libId);
    let r = await cmd({ op: "new_symbol", lib_id: libId });
    t(r.ok, `new_symbol ${libId}: ${r.message ?? "ok"}`);
    const pin = (number, name, x) => ({ number, name, electrical_type: "passive", shape: "line", at: { x, y: 0 }, angle_deg: x < 0 ? 0 : 180, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const pn of [pin("1", "A", -5.08), pin("2", "B", 5.08)]) { r = await cmd({ op: "add_symbol_pin", lib_id: libId, pin: pn }); if (!r.ok) t(false, `add_symbol_pin: ${r.message}`); }
    r = await cmd({ op: "add_symbol_graphic", lib_id: libId, graphic: { kind: "rectangle", unit: 1, body_style: 1, start: { x: -2.54, y: -2.54 }, end: { x: 2.54, y: 2.54 }, stroke_mm: 0.254, fill: "none" } });
    if (!r.ok) t(false, `add_symbol_graphic: ${r.message}`);
    const cur = await symbol(libId);
    r = await cmd({ op: "edit_symbol_properties", lib_id: libId, reference_prefix: "U", description: "Base part", keywords: "p17 base", datasheet: "http://p17/base.pdf", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 1, has_alternate_body_style: false, footprint_filters: [], ...extra });
    if (!r.ok) t(false, `edit_symbol_properties: ${r.message}`);
    r = await cmd({ op: "edit_library_symbol_fields", lib_id: libId, fields: [
      { name: "Reference", text: "U", at: { x: 0, y: 5.08 } }, { name: "Value", text: bareOf(libId), at: { x: 0, y: -5.08 } }, { name: "Footprint", text: "Package_SO:SOIC-8", hidden: true },
      { name: "Datasheet", text: "http://p17/base.pdf", hidden: true }, { name: "Description", text: "Base part", hidden: true },
      { name: "MPN", text: "B-1", at: { x: 0, y: -7.62 }, hidden: false } ] });
    t(r.ok, `edit_library_symbol_fields: ${r.message ?? "ok"}`);
  };
  const bareOf = (libId) => libId.slice(libId.indexOf(":") + 1);
  const openSymbolTab = async () => { await clickTab("Symbol"); await until(() => __eda.state().tab === "symbol", 4000); await sleep(500); };
  /** Selects the tree row and opens its context menu; returns the labels of the entries (null when there is no row). */
  const menuFor = async (libId) => {
    await until(() => itemEl(libId), 6000);
    const el = itemEl(libId);
    if (!el) return null;
    el.click(); await sleep(200);
    const r = el.getBoundingClientRect();
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 5, button: 2 }));
    await sleep(250);
    return [...document.querySelectorAll('[role="menuitem"]')].map((m) => ({ label: m.textContent.trim(), disabled: m.getAttribute("aria-disabled") === "true" }));
  };
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); await sleep(150); };
  const has = (menu, label) => !!menu?.some((m) => m.label.startsWith(label));
  const selectRow = async (libId) => { await until(() => itemEl(libId), 6000); itemEl(libId)?.click(); await sleep(250); };
  const flatOf = (libId) => json(`/api/symbol?lib_id=${encodeURIComponent(libId)}&flatten=1`);
  const fieldOf = (sym, name) => (sym.fields ?? []).find((f) => f.name === name);
  const toastText = () => document.querySelector('[role="status"]:not([data-testid])')?.textContent ?? "";
  const BASE = "eda:P17Base";
  const LEAF = "eda:P17Leaf";

  await scenario("Derive from Existing Symbol: the tree menu offers it, the dialog names P17Base_1, OK makes a derived symbol that draws its parent's pins and opens", async (t) => {
    await openSymbolTab();
    t(__eda.state().tab === "symbol", "the Symbol Editor tab");
    await makeBase(t, BASE);
    const menu = await menuFor(BASE);
    t(has(menu, "Derive from Existing Symbol"), `the tree's menu has Derive from Existing Symbol...: ${menu?.map((m) => m.label).join(" | ")}`);
    t(!has(menu, "Flatten Symbol"), "no Flatten Symbol for a symbol that is not derived");
    t(has(menu, "Bulk Edit Related Symbol Fields") === false, "no related fields table for a symbol with nothing derived from it");
    await closeMenu();
    t(__eda.actions().some((a) => a.id === "eeschema.SymbolLibraryControl.deriveFromExistingSymbol" && a.enabled), "the action is enabled on the Symbol tab");
    await selectRow(BASE);
    const r = await __eda.run("eeschema.SymbolLibraryControl.deriveFromExistingSymbol");
    await sleep(500);
    t(/Derive from Existing Symbol/.test(r.dialog ?? ""), `the dialog opens: ${r.dialog}`);
    t(tid("derive-name")?.value === "P17Base_1", `the name starts as <parent>_1: ${tid("derive-name")?.value}`);
    t(tid("derive-parent")?.value === BASE, "the parent is the selected symbol");
    t(!tid("derive-keep-datasheet").checked && !tid("derive-keep-footprint").checked && !tid("derive-transfer").checked, "the three keep options start off");
    t(off("derive-keep-content"), "Keep content of user-defined fields waits for Transfer user-defined fields");
    tid("derive-transfer").click(); await sleep(100);
    t(!off("derive-keep-content"), "Transfer user-defined fields turns it on");
    await type(tid("derive-name"), "P17Leaf");
    await click(tid("derive-ok"), 1500);
    t(!tid("derive-symbol"), "OK closes the dialog");
    madeSyms.push(LEAF);
    const own = await symbol(LEAF);
    t(own.extends === BASE, `P17Leaf extends ${own.extends}`);
    t((own.pins ?? []).length === 0 && (own.graphics ?? []).length === 0, "it keeps no drawing of its own");
    const mpn = fieldOf(own, "MPN");
    t(!!mpn && (mpn.text ?? "") === "", `Transfer user-defined fields copies MPN without its content: ${JSON.stringify(mpn)}`);
    t((fieldOf(own, "Footprint")?.text ?? "") === "" && (own.datasheet ?? "") === "", "the footprint and the datasheet are not kept");
    const flat = await flatOf(LEAF);
    t(flat.pins.length === 2 && flat.graphics.length === 1, `the flat symbol draws the parent's ${flat.pins.length} pins and ${flat.graphics.length} body`);
    t(!!tid("derived-banner") && /P17Leaf is a derived symbol/.test(tid("derived-banner").textContent), `the banner says so: ${tid("derived-banner")?.textContent}`);
    t(!!tid("derived-open-root"), "with a link to the root symbol");
    const menu2 = await menuFor(LEAF);
    t(has(menu2, "Flatten Symbol") && has(menu2, "Bulk Edit Related Symbol Fields"), `a derived symbol's menu has Flatten Symbol and the related fields table: ${menu2?.map((m) => m.label).join(" | ")}`);
    await closeMenu();
    const menu3 = await menuFor(BASE);
    t(!has(menu3, "Flatten Symbol") && has(menu3, "Bulk Edit Related Symbol Fields"), "the parent's menu has the related fields table but not Flatten Symbol");
    await closeMenu();
  });

  await scenario("the banner's link opens the root symbol; an edit of the drawing is refused with KiCad's reason", async (t) => {
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    t(!!tid("derived-banner"), "the derived symbol is open with the banner");
    // a drawing edit goes to the verb, which refuses it
    const r = await cmd({ op: "add_symbol_graphic", lib_id: LEAF, graphic: { kind: "text", unit: 1, body_style: 1, text: "x", at: { x: 0, y: 0 }, angle_deg: 0, size_mm: 1.27 } });
    t(!r.ok && /derived/i.test(r.message ?? ""), `the verb refuses: ${r.message}`);
    tid("derived-open-root").click(); await sleep(1100);
    t(!tid("derived-banner"), "the root symbol has no banner");
  });

  await scenario("Symbol Properties: the fields grid lists the fields, a derived symbol's inherited ones too; OK writes them in one undo step", async (t) => {
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    const r = await __eda.run("eeschema.InteractiveEdit.symbolProperties");
    await sleep(500);
    t(/Symbol Properties/.test(r.dialog ?? ""), `the dialog opens: ${r.dialog}`);
    const names = [...document.querySelectorAll('[data-testid^="field-row-"]')].map((row) => row.getAttribute("data-field"));
    t(same(names.slice(0, 5), ["Reference", "Value", "Footprint", "Datasheet", "Description"]), `the five mandatory fields first: ${names.join(", ")}`);
    const mpnRow = document.querySelector('[data-field="MPN"]');
    t(mpnRow?.getAttribute("data-inherited") === "true", "MPN is also the parent's: it is shown as inherited");
    t(!off("props-parent"), "Derive from symbol is enabled for a derived symbol");
    t(tid("props-parent").value === BASE, "and names the parent");
    t(!tid("props-power"), "the lower General panel (power, pin text, attributes) is the parent's: hidden");
    // edit MPN, the description and hide the value
    const rowIndex = names.indexOf("MPN");
    await type(tid(`field-text-${rowIndex}`), "L-9");
    await type(tid("field-text-4"), "Leaf part");
    tid("field-show-1").click(); await sleep(80);
    await type(tid("props-keywords"), "p17 leaf");
    await click(tid("props-ok"), 1200);
    t(!tid("symbol-properties"), "OK closes the dialog");
    const own = await symbol(LEAF);
    t(fieldOf(own, "MPN")?.text === "L-9" && own.description === "Leaf part" && own.keywords === "p17 leaf", `own entry: MPN ${fieldOf(own, "MPN")?.text}, description ${own.description}, keywords ${own.keywords}`);
    t(fieldOf(own, "Value")?.hidden === true, "the Value field is hidden now");
    const flat = await flatOf(LEAF);
    t(flat.description === "Leaf part" && flat.pins.length === 2, "the flat symbol takes the description and still draws the parent's pins");
    const base = await symbol(BASE);
    t(base.description === "Base part" && fieldOf(base, "MPN")?.text === "B-1", "the parent is untouched");
    await undoFp();
    const back = await symbol(LEAF);
    t(fieldOf(back, "MPN")?.text === "" && back.description !== "Leaf part", "one undo of the Symbol Editor takes the whole dialog back");
  });

  await scenario("Symbol Properties refuses what KiCad refuses: a field name used twice, a field with text and no name, a root symbol whose reference has no letter", async (t) => {
    await selectRow(BASE);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    await __eda.run("eeschema.InteractiveEdit.symbolProperties");
    await sleep(500);
    t(off("props-parent") && tid("props-parent").value === "", "a root symbol's Derive from symbol is <None> and off");
    t(!!tid("props-power"), "its lower General panel is shown");
    // a new field: Field<n>, hidden
    tid("field-add").click(); await sleep(150);
    const rows = [...document.querySelectorAll('[data-testid^="field-row-"]')];
    const last = rows.length - 1;
    t(tid(`field-name-${last}`).value === `Field${last}`, `a new field is named Field<rows>: ${tid(`field-name-${last}`).value}`);
    t(!tid(`field-show-${last}`).checked, "and starts hidden");
    await type(tid(`field-name-${last}`), "mpn");
    await type(tid(`field-text-${last}`), "x");
    await click(tid("props-ok"), 300);
    t(/The name 'mpn' is already in use\./.test(tid("props-error")?.textContent ?? ""), `names compare without case: ${tid("props-error")?.textContent}`);
    await type(tid(`field-name-${last}`), "");
    await click(tid("props-ok"), 300);
    t(/Fields must have a name\./.test(tid("props-error")?.textContent ?? ""), `a field with text needs a name: ${tid("props-error")?.textContent}`);
    await type(tid(`field-text-${last}`), "");
    await type(tid("field-text-0"), "12");
    await click(tid("props-ok"), 300);
    t(/References must start with a letter\./.test(tid("props-error")?.textContent ?? ""), `a root symbol's reference: ${tid("props-error")?.textContent}`);
    t(!!tid("symbol-properties"), "the dialog stays open on each");
    // the mandatory rows cannot be deleted
    document.querySelector('[data-testid="field-row-1"]').click(); await sleep(60);
    tid("field-delete").click(); await sleep(100);
    t(/The first 5 fields are mandatory\./.test(tid("props-error")?.textContent ?? ""), `${tid("props-error")?.textContent}`);
    await closeDialogs();
    t((await symbol(BASE)).reference_prefix === "U", "nothing was written");
  });

  await scenario("Update Symbol Fields: a parent's change reaches the derived symbol per the chosen options; a symbol that is not derived is told so", async (t) => {
    // change the parent: description, MPN text and Value place
    const baseFields = (await symbol(BASE)).fields.map((f) => (f.name === "MPN" ? { ...f, text: "B-2", at: { x: 3000, y: -9000 } } : f));
    let r = await cmd({ op: "edit_library_symbol_fields", lib_id: BASE, fields: baseFields });
    t(r.ok, "the parent's MPN is B-2 now");
    const baseNow = await symbol(BASE);
    r = await cmd({ op: "edit_symbol_properties", lib_id: BASE, reference_prefix: "U", description: "Base two", keywords: "p17 base", datasheet: baseNow.datasheet ?? "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: baseNow.pin_name_offset_mm ?? 0.508, unit_count: 1, has_alternate_body_style: false, footprint_filters: [] });
    t(r.ok, `the parent's description is "Base two": ${r.message ?? ""}`);
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    const before = await symbol(LEAF);
    const run = await __eda.run("eeschema.SymbolLibraryControl.updateSymbolFields");
    await sleep(600);
    t(/Update Symbol Fields/.test(run.dialog ?? ""), `the dialog opens: ${run.dialog}`);
    t(tid("update-fields-parent")?.value === "P17Base", "it names the parent");
    const rows = [...document.querySelectorAll('[data-testid^="update-fields-row-"]')].map((e) => e.getAttribute("data-testid").replace("update-fields-row-", ""));
    t(same(rows, ["Reference", "Value", "Footprint", "Datasheet", "Description", "MPN"]), `the five mandatory fields and the user field: ${rows.join(", ")}`);
    t(tid("update-fields-text").checked && tid("update-fields-visibility").checked && tid("update-fields-effects").checked && tid("update-fields-positions").checked, "text, visibility, sizes and positions start on");
    t(!tid("update-fields-remove").checked && !tid("update-fields-empty").checked, "removing extras and resetting empty fields start off");
    // update MPN only
    tid("update-fields-none").click(); await sleep(80);
    tid("update-fields-row-MPN").click(); await sleep(80);
    await click(tid("update-fields-ok"), 1200);
    t(!tid("update-symbol-fields"), "OK closes the dialog");
    const own = await symbol(LEAF);
    t(fieldOf(own, "MPN")?.text === "B-2" && same(fieldOf(own, "MPN")?.at, { x: 3000, y: -9000 }), `MPN follows the parent: ${JSON.stringify(fieldOf(own, "MPN"))}`);
    t(own.description === before.description, "a field that was not ticked is left alone");
    // the same with the description ticked, text off: nothing changes
    await __eda.run("eeschema.SymbolLibraryControl.updateSymbolFields");
    await sleep(500);
    tid("update-fields-none").click(); tid("update-fields-row-Description").click(); await setChecked("update-fields-text", false); await sleep(80);
    await click(tid("update-fields-ok"), 1000);
    t((await symbol(LEAF)).description === before.description, "with Update/reset field text off the description stays");
    await __eda.run("eeschema.SymbolLibraryControl.updateSymbolFields");
    await sleep(500);
    tid("update-fields-all").click(); await setChecked("update-fields-text", true); await sleep(80);
    await click(tid("update-fields-ok"), 1000);
    t((await symbol(LEAF)).description === "Base two", `with it on the description is the parent's: ${(await symbol(LEAF)).description}`);
    // a symbol that is not derived
    await selectRow(BASE);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    const none = await __eda.run("eeschema.SymbolLibraryControl.updateSymbolFields");
    await sleep(300);
    t(!tid("update-symbol-fields"), "no dialog for a symbol that is not derived");
    t(/Symbol is not derived from another symbol\./.test(toastText() + (none.toast ?? "")), `KiCad's message: ${toastText() || none.toast}`);
  }, [/Symbol is not derived from another symbol\./]);

  await scenario("Bulk Edit Related Symbol Fields lists the root and what is derived from it; the Library scope lists them all; OK writes the cells", async (t) => {
    await selectRow(BASE);
    const r = await __eda.run("eeschema.SymbolLibraryControl.showRelatedLibraryFieldsTable");
    await sleep(900);
    t(/Symbol Fields Table/.test(r.dialog ?? ""), `the dialog opens: ${r.dialog}`);
    t(tid("fields-scope")?.value === "related", "its scope is Related Symbols");
    const rowsOf = () => [...document.querySelectorAll('[data-testid^="fields-row-"]')].map((e) => e.getAttribute("data-testid").replace("fields-row-", ""));
    t(same(rowsOf().sort(), ["P17Base", "P17Leaf"]), `only the related symbols: ${rowsOf().join(", ")}`);
    const headers = [...document.querySelectorAll('[data-testid="fields-table"] th')].map((h) => h.textContent.trim());
    t(["Reference", "Value", "Footprint", "Datasheet", "Description", "MPN"].every((h) => headers.includes(h)), `the columns include Value, Footprint and the user field: ${headers.join(", ")}`);
    await type(tid("cell-P17Leaf-Footprint"), "Package_SO:SOIC-16");
    await type(tid("cell-P17Base-MPN"), "B-3");
    tid("fields-new-name").value; await type(tid("fields-new-name"), "Supplier"); tid("fields-add-field").click(); await sleep(150);
    t([...document.querySelectorAll('[data-testid="fields-table"] th')].some((h) => h.textContent.trim() === "Supplier"), "Add Field makes a column");
    await type(tid("cell-P17Leaf-Supplier"), "Acme");
    await click(tid("fields-ok"), 1300);
    t(!tid("fields-table"), "OK closes the dialog");
    const leaf = await symbol(LEAF), base = await symbol(BASE);
    t(fieldOf(leaf, "Footprint")?.text === "Package_SO:SOIC-16" && fieldOf(leaf, "Supplier")?.text === "Acme", `the leaf: ${fieldOf(leaf, "Footprint")?.text}, ${fieldOf(leaf, "Supplier")?.text}`);
    t(fieldOf(base, "MPN")?.text === "B-3", "the base: MPN B-3");
    // the library scope
    await __eda.run("eeschema.SymbolLibraryControl.showLibraryFieldsTable");
    await sleep(900);
    t(tid("fields-scope")?.value === "library", "Bulk Edit Symbol Fields has the Library scope");
    t(rowsOf().includes("P17Base") && rowsOf().includes("P17Leaf"), "and lists the symbols of the library");
    await choose(tid("fields-scope"), "related"); await sleep(900);
    t(same(rowsOf().sort(), ["P17Base", "P17Leaf"]), "switching the scope shows the related symbols");
    await closeDialogs();
    await undoFp(); await undoFp();
  });

  await scenario("Flatten Symbol: the derived symbol becomes the flat symbol; one undo derives it again", async (t) => {
    const menu = await menuFor(LEAF);
    t(has(menu, "Flatten Symbol"), "the tree's menu offers Flatten Symbol");
    const item = [...document.querySelectorAll('[role="menuitem"]')].find((m) => m.textContent.trim() === "Flatten Symbol");
    item?.click(); await sleep(1200);
    const flat = await symbol(LEAF);
    t(!flat.extends && flat.pins.length === 2 && flat.graphics.length === 1, `P17Leaf stands alone with ${flat.pins?.length} pins and ${flat.graphics?.length} body`);
    const m2 = await menuFor(LEAF);
    t(!has(m2, "Flatten Symbol"), "and no longer offers Flatten Symbol");
    await closeMenu();
    t((await symbol(BASE)).pins.length === 2, "the parent keeps its drawing");
    await undoFp();
    const back = await symbol(LEAF);
    t(back.extends === BASE && back.pins.length === 0, "an undo makes it derived again");
  });

  await scenario("Deleting a base symbol asks first and takes the derived symbols with it; Cancel keeps all; an undo brings them back", async (t) => {
    await selectRow(BASE);
    __eda.run("eeschema.SymbolLibraryControl.deleteSymbol");
    await until(() => document.querySelector('[role="dialog"][aria-label="Warning"]'), 4000);
    const dlg = document.querySelector('[role="dialog"][aria-label="Warning"]');
    t(!!dlg && /Deleting a base symbol will delete all symbols derived from it\./.test(dlg.textContent) && /P17Base \(base\)/.test(dlg.textContent) && /P17Leaf/.test(dlg.textContent), `the warning lists them: ${dlg?.textContent.replace(/\s+/g, " ")}`);
    [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel").click(); await sleep(800);
    t((await symNames()).includes(BASE) && (await symNames()).includes(LEAF), "Cancel deletes nothing");
    __eda.run("eeschema.SymbolLibraryControl.deleteSymbol");
    await until(() => document.querySelector('[role="dialog"][aria-label="Warning"]'), 4000);
    [...document.querySelectorAll('[role="dialog"][aria-label="Warning"] button')].find((b) => b.textContent.trim() === "Delete All Listed Symbols").click(); await sleep(1200);
    const names = await symNames();
    t(!names.includes(BASE) && !names.includes(LEAF), "Delete All Listed Symbols removes both");
    await undoFp();
    const back = await symNames();
    t(back.includes(BASE) && back.includes(LEAF), "one undo brings both back");
    t((await symbol(LEAF)).extends === BASE, "still linked");
  });

  await scenario("Save Symbol As on a derived symbol: into its own library it stays derived; Flatten/remove symbol inheritance saves the flat symbol", async (t) => {
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.saveSymbolCopyAs");
    await sleep(700);
    t(!!tid("save-as-flatten"), "the dialog offers Flatten/remove symbol inheritance");
    t(/current parent symbols: P17Base/.test(tid("save-as-flatten").parentElement.textContent), `and names the parents: ${tid("save-as-flatten").parentElement.textContent.trim()}`);
    const nameInput = document.querySelector('[aria-label="Save Symbol As"] input[type="text"], [aria-label="Save Symbol As"] input:not([type])');
    await type(nameInput, "P17Copy");
    [...document.querySelectorAll('[aria-label="Save Symbol As"] .dialog-footer button')].find((b) => b.textContent.trim() === "Save").click(); await sleep(1200);
    madeSyms.push("eda:P17Copy");
    let copy = await symbol("eda:P17Copy");
    t(copy.extends === BASE, `the copy is derived from the same parent: ${copy.extends}`);
    // a name in the symbol's own ancestry is refused
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.saveSymbolCopyAs");
    await sleep(700);
    const input2 = document.querySelector('[aria-label="Save Symbol As"] input[type="text"], [aria-label="Save Symbol As"] input:not([type])');
    await type(input2, "P17Base");
    [...document.querySelectorAll('[aria-label="Save Symbol As"] .dialog-footer button')].find((b) => b.textContent.trim() === "Save").click(); await sleep(900);
    t(/cannot replace another symbol 'P17Base' that it descends from/.test(tid("save-as-error")?.textContent ?? ""), `${tid("save-as-error")?.textContent}`);
    // flattened
    await type(input2, "P17Flat");
    await setChecked("save-as-flatten", true);
    [...document.querySelectorAll('[aria-label="Save Symbol As"] .dialog-footer button')].find((b) => b.textContent.trim() === "Save").click(); await sleep(1200);
    madeSyms.push("eda:P17Flat");
    const flat = await symbol("eda:P17Flat");
    t(!flat.extends && flat.pins.length === 2, `flattened: stands alone with ${flat.pins?.length} pins`);
    copy = await symbol("eda:P17Copy");
    t(copy.extends === BASE, "the first copy is untouched");
  });

  await scenario("Copy and Paste Symbol move a derived symbol as the flat symbol KiCad's clipboard holds; Duplicate keeps it derived", async (t) => {
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.copySymbol");
    await sleep(500);
    await __eda.run("eeschema.SymbolLibraryControl.pasteSymbol");
    await sleep(1400);
    const names = await symNames();
    const pasted = names.find((n) => /^eda:P17Leaf_1$/.test(n));
    t(!!pasted, `the paste made ${pasted}`);
    if (pasted) { madeSyms.push(pasted); const s = await symbol(pasted); t(!s.extends && s.pins.length === 2, `and it stands alone with ${s.pins?.length} pins`); }
    await selectRow(LEAF);
    await __eda.run("eeschema.SymbolLibraryControl.duplicateSymbol");
    await sleep(1400);
    const dup = (await symNames()).find((n) => /^eda:P17Leaf_(2|1)$/.test(n) && n !== pasted);
    t(!!dup, `the duplicate: ${dup}`);
    if (dup) { madeSyms.push(dup); t((await symbol(dup)).extends === BASE, "a duplicate is derived from the same parent"); }
  });

  await scenario("Rename Symbol on a parent keeps its children linked and the open child drawing", async (t) => {
    const r = await cmd({ op: "rename_library_symbol", lib_id: BASE, new_lib_id: "eda:P17BaseRenamed", overwrite: false });
    t(r.ok, `rename_library_symbol: ${r.message ?? "ok"}`);
    t((await symbol(LEAF)).extends === "eda:P17BaseRenamed", `the child follows: ${(await symbol(LEAF)).extends}`);
    const fl = await flatOf(LEAF);
    t(fl.pins.length === 2, "and still draws its parent's pins");
    await undoFp();
    t((await symbol(LEAF)).extends === BASE, "an undo gives the old name back");
  });

  // ================================================================================================ 3. snapping to other items' anchors in both editors
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const isMacPage = /Mac/.test(navigator.platform);
  /** Where the editor's point `(wx, wy)` (um, +y down) is on the screen. */
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** A pointer event on the canvas at the editor's point; `mods` are `ctrl` (Cmd on a Mac: the grid off), `shift` (the anchors off). */
  const fire = (type, wx, wy, mods = {}, buttons = 0) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !!mods.ctrl && !isMacPage, metaKey: !!mods.ctrl && isMacPage, shiftKey: !!mods.shift }));
  };
  /**
   * A click of the left button at the editor's point. `hover: false` sends the press alone: a hover first (the pointer arriving) already snaps once, and snapping to an anchor starts the snap lines
   * through it (`SNAP_LINE_MANAGER::SetSnapLineOrigin`), which the press that follows then sticks to -- KiCad's own behaviour, but it makes the point of a press depend on the hover before it.
   */
  const clickAt = async (wx, wy, mods = {}, hover = true) => { if (hover) { fire("pointermove", wx, wy, mods); await sleep(70); } fire("pointerdown", wx, wy, mods, 1); await sleep(40); fire("pointerup", wx, wy, mods); await sleep(450); };
  /** Arms a placing tool: an undo (like Escape) puts the editor back on the Select tool, and a tool that is armed already would be switched off by its action. */
  const arm = async (action, tool) => { if (__eda.state().tool !== tool) { await __eda.run(action); await sleep(250); } };
  const dragFromTo = async (from, to, mods = {}) => {
    fire("pointermove", from[0], from[1], mods); await sleep(60);
    fire("pointerdown", from[0], from[1], mods, 1); await sleep(60);
    // far enough to be a drag (8 px), then on to the target, then the release
    fire("pointermove", from[0] + 40 / __eda.state().view.scale, from[1] + 40 / __eda.state().view.scale, mods, 1); await sleep(60);
    fire("pointermove", to[0], to[1], mods, 1); await sleep(80);
    fire("pointerup", to[0], to[1], mods); await sleep(700);
  };
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  /** A double click on the canvas at the editor's point. */
  const dbl = async (wx, wy) => { const [sx, sy] = toScreen(wx, wy); canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: sx, clientY: sy, detail: 2 })); await sleep(600); };
  const onGrid = (v, g) => Math.abs(v - Math.round(v / g) * g) < 1e-6 * Math.max(1, Math.abs(v)) + 1e-6;

  await scenario("Footprint Editor snapping: a pad tool click goes to the centre of a pad under it, Shift puts it on the grid, Ctrl+Shift leaves it where it is", async (t) => {
    await clickTab("Footprint");
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !libraryBefore.fp.includes(n) && !made.includes(n));
    made.push(name);
    const r = await cmd({ op: "add_pad", footprint: name, pad: { ...padOf("1"), at: { x: 5_400_000, y: 2_300_000 } } });
    t(r.ok, `a pad at (5400, 2300), off the 250 um grid: ${r.message ?? "ok"}`);
    await sleep(600);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const scale = __eda.state().view.scale;
    const d = Math.min(60_000, 10 / scale);
    t(scale > 0, `the view: ${scale} px/um; the cursor goes ${Math.round(d)} um from the pad's centre`);
    await arm("pcbnew.PadTool.placePad", "pad");
    t(__eda.state().tool === "pad", "the pad tool is armed");
    await clickAt(5_400_000 + d, 2_300_000 + d);
    let pads = (await footprint(name)).pads;
    t(pads.length === 2 && pads.filter((p) => p.at.x === 5_400_000 && p.at.y === 2_300_000).length === 2, `the new pad is at the centre of the first: ${JSON.stringify(pads.map((p) => [p.number, p.at.x, p.at.y]))}`);
    t(__eda.state().snap?.anchored === true, `the snap report says an anchor: ${JSON.stringify(__eda.state().snap)}`);
    await undoFp();
    t((await footprint(name)).pads.length === 1, "one undo takes the pad away");
    // Shift turns the anchors off: the grid
    await arm("pcbnew.PadTool.placePad", "pad");
    await clickAt(5_400_000 + d, 2_300_000 + d, { shift: true }, false);
    pads = (await footprint(name)).pads;
    const shifted = pads.find((p) => p.number !== "1");
    t(!!shifted && onGrid(shifted.at.x, 250) && onGrid(shifted.at.y, 250) && !(shifted.at.x === 5_400_000 && shifted.at.y === 2_300_000), `with Shift the pad is on the grid: ${JSON.stringify(shifted?.at)}`);
    await undoFp();
    // Ctrl+Shift turns both off: where the pointer is
    await arm("pcbnew.PadTool.placePad", "pad");
    await clickAt(5_400_000 + d, 2_300_000 + d, { shift: true, ctrl: true }, false);
    pads = (await footprint(name)).pads;
    const free = pads.find((p) => p.number !== "1");
    t(!!free && near(free.at.x, 5_400_000 + d, 3000) && near(free.at.y, 2_300_000 + d, 3000), `with both off the pad is where the pointer was: ${JSON.stringify(free?.at)} for (${Math.round(5400 + d)}, ${Math.round(2300 + d)})`);
    await undoFp();
    // Ctrl alone: the grid is off, the anchor stays
    await arm("pcbnew.PadTool.placePad", "pad");
    await clickAt(5_400_000 + d, 2_300_000 + d, { ctrl: true }, false);
    pads = (await footprint(name)).pads;
    t(pads.filter((p) => p.at.x === 5_400_000 && p.at.y === 2_300_000).length === 2, "with only the grid off the anchor still takes it");
    await undoFp();
  });

  await scenario("Footprint Editor snapping: a line goes to the end of a line on the active layer, not to a pad on another; a dragged line lands its end on another's", async (t) => {
    await clickTab("Footprint");
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !libraryBefore.fp.includes(n) && !made.includes(n));
    made.push(name);
    const seg = (x0, y0, x1, y1) => ({ kind: "segment", layer: "F.SilkS", stroke_width: 150_000, filled: false, start: { x: x0, y: y0 }, end: { x: x1, y: y1 } });
    for (const c of [{ op: "add_pad", footprint: name, pad: { ...padOf("1"), at: { x: 5_430_000, y: 2_330_000 } } }, { op: "add_footprint_graphic", footprint: name, shape: seg(4_310_000, 710_000, 7_310_000, 710_000) }, { op: "add_footprint_graphic", footprint: name, shape: seg(1_130_000, 3_330_000, 2_130_000, 3_330_000) }]) {
      const r = await cmd(c);
      if (!r.ok) t(false, `${c.op}: ${r.message}`);
    }
    await sleep(700);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const scale = __eda.state().view.scale;
    const d = Math.min(60_000, 10 / scale);
    // the line tool: the first click near the end of the silk line (on the active layer F.SilkS), the second away from everything
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    t(__eda.state().tool === "draw_segment", "the line tool is armed");
    await clickAt(4_310_000 + d, 710_000 + d, {}, false);
    await clickAt(3_000_000, 5_000_000, { shift: true, ctrl: true }, false);
    let graphics = (await footprint(name)).graphics;
    const made1 = graphics.find((g) => g.start.x === 4_310_000 && g.start.y === 710_000 && g.end.x !== 7_310_000);
    t(graphics.length === 3 && !!made1, `the new line starts at the end of the silk line: ${JSON.stringify(graphics.map((g) => [g.start, g.end]))}`);
    await undoFp();
    t((await footprint(name)).graphics.length === 2, "one undo takes the line away");
    // a click near the pad (on F.Cu, not the active layer): no snap to it
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    await clickAt(5_430_000 + d, 2_330_000 + d, { ctrl: true }, false);
    await clickAt(3_000_000, 5_000_000, { shift: true, ctrl: true }, false);
    graphics = (await footprint(name)).graphics;
    const toPad = graphics.find((g) => g.start.x === 5_430_000 && g.start.y === 2_330_000);
    t(graphics.length === 3 && !toPad, `a line on the silk layer does not snap to a pad on the copper one: ${JSON.stringify(graphics.map((g) => g.start))}`);
    await undoFp();
    // a drag: the second silk line, picked up at its start, dropped near the end of the first
    await select([]);
    const second = graphics.find((g) => g.start.x === 1_130_000);
    await dragFromTo([1_130_000, 3_330_000], [4_310_000 + d, 710_000 + d]);
    const moved = (await footprint(name)).graphics.find((g) => g.id === second.id);
    t(!!moved && moved.start.x === 4_310_000 && moved.start.y === 710_000, `the dropped line has its start on the other's end: ${JSON.stringify(moved?.start)} (it was (1130, 3330))`);
    t(!!moved && moved.end.x - moved.start.x === 1_000_000 && moved.end.y === moved.start.y, "and the same length");
    await undoFp();
    t((await footprint(name)).graphics.find((g) => g.id === second.id)?.start.x === 1_130_000, "one undo puts it back");
  });

  await scenario("Symbol Editor snapping: the pin tool goes to the end of another pin when the grid is off, to the grid otherwise", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17Snap";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    r = await cmd({ op: "add_symbol_pin", lib_id: id, pin: { number: "9", name: "X", electrical_type: "passive", shape: "line", at: { x: 3.1, y: 0.7 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null } });
    t(r.ok, `a pin whose end is at (3.1, 0.7) mm, off the 1.27 mm grid: ${r.message ?? "ok"}`);
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const scale = __eda.state().view.scale;
    const d = Math.min(300_000, 10 / scale);
    t(scale > 0, `the view: ${scale} px/um; the cursor goes ${Math.round(d)} um from the pin's end`);
    await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
    t(__eda.state().tool === "pin", `the pin tool is armed: ${__eda.state().tool}`);
    const tip = [3_100_000, -700_000];
    // The pin tool asks in Pin Properties before it places (`SYMBOL_EDITOR_PIN_TOOL::CreatePin`): every click below is OK'd.
    const okPin = async () => {
      await until(() => tid("pin-props-ok"), 4000);
      tid("pin-props-ok")?.click();
      await sleep(500);
    };
    // grid off (Ctrl): the end of the other pin
    await clickAt(tip[0] + d, tip[1] + d, { ctrl: true }, false);
    await okPin();
    let pins = (await symbol(id)).pins;
    t(pins.length === 2 && pins.filter((p) => p.at.x === 3.1 && p.at.y === 0.7).length === 2, `with the grid off the new pin ends where the other does: ${JSON.stringify(pins.map((p) => [p.number, p.at.x, p.at.y]))}`);
    t(__eda.state().snap?.anchored === true, `the snap report: ${JSON.stringify(__eda.state().snap)}`);
    await undoFp();
    // the grid on: the grid decides (a pin end is within the range, but so is a grid point)
    await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
    await clickAt(tip[0] + d, tip[1] + d, {}, false);
    await okPin();
    pins = (await symbol(id)).pins;
    const gridPin = pins.find((p) => p.number !== "9");
    t(!!gridPin && onGrid(gridPin.at.x, 1.27) && onGrid(gridPin.at.y, 1.27), `with the grid on the new pin is on the grid: ${JSON.stringify(gridPin?.at)}`);
    await undoFp();
    // Shift: no anchors, the grid
    await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
    await clickAt(tip[0] + d, tip[1] + d, { shift: true }, false);
    await okPin();
    pins = (await symbol(id)).pins;
    const shiftPin = pins.find((p) => p.number !== "9");
    t(!!shiftPin && onGrid(shiftPin.at.x, 1.27) && onGrid(shiftPin.at.y, 1.27), `with Shift too: ${JSON.stringify(shiftPin?.at)}`);
    await undoFp();
    // both off: the pointer
    await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
    await clickAt(tip[0] + d, tip[1] + d, { ctrl: true, shift: true }, false);
    await okPin();
    pins = (await symbol(id)).pins;
    const freePin = pins.find((p) => p.number !== "9");
    t(!!freePin && near(freePin.at.x * 1e6, tip[0] + d, 3000) && near(-freePin.at.y * 1e6, tip[1] + d, 3000), `with both off the pin is where the pointer was: ${JSON.stringify(freePin?.at)}`);
    await undoFp();
  });

  // ================================================================================================ 4. the Footprint Editor's fields grid
  await scenario("Footprint Properties has a fields grid: Reference, Value, Datasheet and Description, user fields with their layer, place and visibility; OK is one undo step and the exported .kicad_mod has them", async (t) => {
    await clickTab("Footprint");
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !libraryBefore.fp.includes(n) && !made.includes(n));
    made.push(name);
    await cmd({ op: "add_pad", footprint: name, pad: { ...padOf("1"), at: { x: 0, y: 0 } } });
    await sleep(500);
    const r = await __eda.run("pcbnew.ModuleEditor.footprintProperties");
    await sleep(600);
    t(/Footprint Properties/.test(r.dialog ?? ""), `the dialog opens: ${r.dialog}`);
    const row = (i) => document.querySelector(`[data-testid="lib-field-row-${i}"]`);
    t(row(0)?.getAttribute("data-field") === "Reference" && tid("lib-field-text-0").value === "REF**", `the Reference starts as REF**: ${tid("lib-field-text-0")?.value}`);
    t(row(1)?.getAttribute("data-field") === "Value" && tid("lib-field-text-1").value === name, `the Value as the footprint's name: ${tid("lib-field-text-1")?.value}`);
    // `GetMandatoryRowCount`: a footprint's four mandatory fields, the Datasheet and the Description empty and hidden
    t(row(2)?.getAttribute("data-field") === "Datasheet" && row(3)?.getAttribute("data-field") === "Description", `then the Datasheet and the Description: ${row(2)?.getAttribute("data-field")}, ${row(3)?.getAttribute("data-field")}`);
    t(off("lib-field-name-0") && off("lib-field-name-1") && off("lib-field-name-2") && off("lib-field-name-3"), "the names of the four mandatory fields are fixed");
    t(tid("lib-field-show-0").checked && tid("lib-field-show-1").checked && !tid("lib-field-show-2").checked && !tid("lib-field-show-3").checked, "the Reference and the Value are shown, the Datasheet and the Description hidden");
    // delete a mandatory row: refused in KiCad's words
    row(0).click(); await sleep(60);
    tid("lib-field-delete").click(); await sleep(120);
    t(/The first 4 fields are mandatory\./.test(tid("fp-lib-properties-error")?.textContent ?? ""), `${tid("fp-lib-properties-error")?.textContent}`);
    // add a field: Field<rows>, hidden
    tid("lib-field-add").click(); await sleep(150);
    t(row(4)?.getAttribute("data-field") === "Field4" && !tid("lib-field-show-4").checked, `a new field is Field4 and hidden: ${row(4)?.getAttribute("data-field")}`);
    // a name that is a mandatory field's is refused, with the grid's message
    await type(tid("lib-field-name-4"), "Datasheet");
    await click(tid("fp-lib-properties-ok"), 300);
    t(/is the name of a mandatory field/.test(tid("fp-lib-properties-error")?.textContent ?? "") && !!tid("fp-library-properties"), `refused: ${tid("fp-lib-properties-error")?.textContent}`);
    await type(tid("lib-field-name-4"), "MPN");
    await type(tid("lib-field-text-4"), "ABC-1");
    tid("lib-field-show-4").click(); await sleep(80);
    await choose(tid("lib-field-layer-4"), "F.SilkS");
    tid("lib-fields-more").click(); await sleep(150);
    const unitsMm = __eda.state().units === "mm";
    const num = (label) => document.querySelector(`[data-testid="lib-field-row-4"] input[aria-label="${label}"]`);
    await type(num("X offset of MPN"), unitsMm ? "1.5" : "59.055"); // 1.5 mm
    await type(num("Y offset of MPN"), unitsMm ? "3" : "118.11");
    tid("lib-field-bold-4").click(); await sleep(60);
    // the Reference's text and the Value hidden, a datasheet
    await type(tid("lib-field-text-0"), "J**");
    await type(tid("lib-field-text-2"), "https://example.com/p17.pdf");
    tid("lib-field-show-1").click(); await sleep(80);
    await click(tid("fp-lib-properties-ok"), 1200);
    t(!tid("fp-library-properties"), "OK closes the dialog");
    const fp = await footprint(name);
    const mpn = (fp.fields ?? []).find((f) => f.name === "MPN");
    t(!!mpn && mpn.value === "ABC-1" && mpn.visible === true, `the user field is stored: ${JSON.stringify(mpn)}`);
    t(!!mpn?.layout && mpn.layout.layer === "F.SilkS" && mpn.layout.at.x === 1_500_000 && mpn.layout.at.y === 3_000_000 && mpn.layout.bold === true, `with its layout: ${JSON.stringify(mpn?.layout)}`);
    t(fp.reference?.text === "J**" && fp.value_visible === false, `the Reference reads J** and the Value is hidden: ${JSON.stringify([fp.reference?.text, fp.value_visible])}`);
    t(fp.datasheet?.text === "https://example.com/p17.pdf" && fp.datasheet?.visible === false, `the Datasheet field is stored: ${JSON.stringify(fp.datasheet)}`);
    const mod = await (await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`)).text();
    t(/\(property "MPN" "ABC-1" \(at 1\.5 3 0\)/.test(mod) && /\(property "Reference" "J\*\*"/.test(mod) && /\(property "Value" "[^"]*" \(at [^)]*\) \(layer "F\.Fab"\) \(hide yes\)/.test(mod) && /\(property "Datasheet" "https:\/\/example\.com\/p17\.pdf"/.test(mod), "the exported .kicad_mod has the fields with their places");
    // reopened, the grid shows what was stored
    await __eda.run("pcbnew.ModuleEditor.footprintProperties");
    await sleep(600);
    t(tid("lib-field-name-4")?.value === "MPN" && tid("lib-field-text-4")?.value === "ABC-1" && tid("lib-field-text-0")?.value === "J**" && !tid("lib-field-show-1").checked, "reopened, the grid is what was stored");
    await closeDialogs();
    // one undo takes the whole panel away
    await undoFp();
    const back = await footprint(name);
    t((back.fields ?? []).length === 0 && back.value_visible !== false && !back.reference && !back.datasheet, "one undo of the Footprint Editor takes the whole panel back");
  });

  // ================================================================================================ 5. symbol fields on the canvas
  await scenario("Symbol Editor fields: drawn where the symbol puts them, Show Hidden Fields shows the hidden ones, a click picks one, a drag moves it, a double click opens its row", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17Fields";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    r = await cmd({ op: "edit_library_symbol_fields", lib_id: id, fields: [
      { name: "Reference", text: "U", at: { x: 0, y: 5.08 } }, { name: "Value", text: "P17Fields", at: { x: 0, y: -5.08 } },
      { name: "Footprint", text: "", hidden: true }, { name: "Datasheet", text: "", hidden: true }, { name: "Description", text: "", hidden: true },
      { name: "MPN", text: "H-1", at: { x: 10.16, y: 0 }, hidden: true } ] });
    t(r.ok, `fields set: ${r.message ?? "ok"}`);
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const canvas = () => document.querySelector(".editor-canvas-col canvas");
    /** The pixels near an editor point that are not the background's: the ink of what is drawn there. */
    const ink = (wx, wy, radiusPx = 14) => {
      const c = canvas(); const ctx = c.getContext("2d"); const dpr = c.width / parseFloat(c.style.width || c.width);
      const [sx, sy] = toScreen(wx, wy); const rect = c.getBoundingClientRect();
      const x = Math.round((sx - rect.left - radiusPx) * dpr), y = Math.round((sy - rect.top - radiusPx) * dpr), w = Math.round(2 * radiusPx * dpr);
      const d = ctx.getImageData(Math.max(0, x), Math.max(0, y), w, w).data;
      let n = 0; for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - 26) + Math.abs(d[i + 1] - 26) + Math.abs(d[i + 2] - 26) > 90) n++;
      return n;
    };
    const REF = [0, -5_080_000], MPN = [10_160_000, 0], VAL = [0, 5_080_000];
    const withHidden = ink(...MPN);
    t(ink(...REF) > 20 && ink(...VAL) > 20, `the Reference and the Value are drawn: ${ink(...REF)}, ${ink(...VAL)} px of ink`);
    t(withHidden > 20, `the hidden MPN is drawn while Show Hidden Fields is on (it starts on): ${withHidden} px`);
    t(__eda.actions().some((a) => a.id === "eeschema.SymbolLibraryControl.showHiddenFields" && a.enabled), "Show Hidden Fields is an enabled action of the Symbol tab");
    await __eda.run("eeschema.SymbolLibraryControl.showHiddenFields");
    await sleep(400);
    t(ink(...MPN) < withHidden / 4, `with it off the hidden field is not drawn: ${ink(...MPN)} px`);
    t(ink(...REF) > 20, "the shown ones stay");
    // a click picks a field it is on, a hidden one only while it is drawn
    await clickAt(MPN[0], MPN[1], {}, false);
    t(__eda.state().selection.length === 0, `nothing hidden is picked with Show Hidden Fields off: ${JSON.stringify(__eda.state().selection)}`);
    await __eda.run("eeschema.SymbolLibraryControl.showHiddenFields");
    await sleep(300);
    await clickAt(MPN[0], MPN[1], {}, false);
    t(same(__eda.state().selection.map((s) => s.id), ["fld:lib:MPN"]), `with it on a click picks the hidden field: ${JSON.stringify(__eda.state().selection.map((s) => s.id))}`);
    await clickAt(REF[0], REF[1], {}, false);
    t(same(__eda.state().selection.map((s) => s.id), ["fld:lib:Reference"]), `a click on the Reference picks it: ${JSON.stringify(__eda.state().selection.map((s) => s.id))}`);
    // a drag moves it, one undo step
    await dragFromTo(REF, [REF[0], REF[1] - 2_540_000]);
    let sym = await symbol(id);
    const refNow = fieldOf(sym, "Reference");
    t(refNow && near(refNow.at.y, 7.62, 0.01) && near(refNow.at.x, 0, 0.01), `the dragged Reference is 2.54 mm higher: ${JSON.stringify(refNow?.at)}`);
    t(fieldOf(sym, "Value")?.at?.y === -5.08, "the other fields stayed");
    await undoFp();
    t(near(fieldOf(await symbol(id), "Reference")?.at?.y ?? 0, 5.08, 0.01), "one undo of the Symbol Editor puts it back");
    // a double click on a field opens Symbol Properties on its row
    const [sx, sy] = toScreen(VAL[0], VAL[1]);
    canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: sx, clientY: sy, detail: 2 }));
    await sleep(700);
    t(!!tid("symbol-properties"), "a double click on a field opens Symbol Properties");
    t(document.querySelector('[data-testid="field-row-1"]')?.getAttribute("aria-selected") === "true", "on the Value's row");
    await closeDialogs();
  });

  // ================================================================================================ 5b. Symbol Properties' units and attributes (P17 review)
  await scenario("Symbol Properties: All units are interchangeable (LockUnits, ki_locked), Define as local power symbol, the four Exclude from boxes (checked and disabled for a power symbol) and Place pin names inside, written as KiCad writes them", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17Units";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(900);
    await __eda.run("eeschema.InteractiveEdit.symbolProperties");
    await sleep(500);
    t(off("props-interchangeable") && tid("props-interchangeable").checked, "one unit: All units are interchangeable is checked and off");
    t(off("props-power-local"), "Define as local power symbol is off until the symbol is a power symbol");
    await type(tid("props-unit-count"), "2");
    await sleep(100);
    t(!off("props-interchangeable"), "two units: the box is on");
    tid("props-interchangeable").click(); await sleep(60);
    tid("props-exclude_from_sim").click(); await sleep(60);
    tid("props-exclude_from_pos_files").click(); await sleep(60);
    await click(tid("props-ok"), 1200);
    t(!tid("symbol-properties"), "OK closes the dialog");
    let sym = await symbol(id);
    t(sym.unit_count === 2 && sym.units_locked === true && sym.exclude_from_sim === true && sym.exclude_from_pos_files === true, `stored: ${JSON.stringify([sym.unit_count, sym.units_locked, sym.exclude_from_sim, sym.exclude_from_pos_files])}`);
    let text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`)).text();
    t(/\(property "ki_locked" ""/.test(text) && /\(exclude_from_sim yes\)/.test(text) && /\(in_pos_files no\)/.test(text), "the .kicad_sym has ki_locked and the exclusions");
    // a power symbol: the exclusions are checked and off, the local power box is on
    await __eda.run("eeschema.InteractiveEdit.symbolProperties");
    await sleep(500);
    t(!tid("props-interchangeable").checked, "reopened, the units are not interchangeable");
    tid("props-power").click(); await sleep(80);
    t(off("props-in_bom") && tid("props-in_bom").checked && off("props-exclude_from_sim") && tid("props-exclude_from_sim").checked, "onPowerCheckBox: the Exclude from boxes are checked and disabled");
    t(!off("props-power-local"), "and Define as local power symbol is on");
    tid("props-power-local").click(); await sleep(60);
    await click(tid("props-ok"), 1200);
    sym = await symbol(id);
    t(sym.power === true && sym.power_local === true && sym.in_bom === false && sym.on_board === false, `a local power symbol: ${JSON.stringify([sym.power, sym.power_local, sym.in_bom, sym.on_board])}`);
    text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`)).text();
    t(/\(power local\)/.test(text), "written (power local), not (power), which is a global one");
    // Place pin names inside off: the offset is 0 (outside)
    await __eda.run("eeschema.InteractiveEdit.symbolProperties");
    await sleep(500);
    t(tid("props-names-inside").checked && !off("props-pin-name-offset"), "the names are inside, with an offset to edit");
    tid("props-names-inside").click(); await sleep(60);
    t(off("props-pin-name-offset"), "outside: no offset");
    await click(tid("props-ok"), 1200);
    t((await symbol(id)).pin_name_offset_mm === 0, "the offset is 0");
    await undoFp();
    t((await symbol(id)).pin_name_offset_mm !== 0, "one undo of the Symbol Editor takes it back");
  });

  // ================================================================================================ 6. alternate pin functions
  await scenario("Symbol Editor pin alternates: Pin Properties edits them in KiCad's grid, the icon is drawn beside the name while Show Pin Alternate Icons is on, the file has them, a symbol with two body styles takes none", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17Alt";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    const pin = (number, y) => ({ number, name: `PA${number}`, electrical_type: "bidirectional", shape: "line", at: { x: -5.08, y }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null });
    for (const p of [pin("1", 0), pin("2", -5.08)]) { r = await cmd({ op: "add_symbol_pin", lib_id: id, pin: p }); if (!r.ok) t(false, `add_symbol_pin: ${r.message}`); }
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const canvas = () => document.querySelector(".editor-canvas-col canvas");
    const shot = () => { const c = canvas(); const ctx = c.getContext("2d"); return { w: c.width, h: c.height, dpr: c.width / parseFloat(c.style.width || c.width), d: ctx.getImageData(0, 0, c.width, c.height).data }; };
    /** The box of the pixels that differ between two shots, in the editor's um (+y down), and how many there are. */
    const diffBox = (a, b) => {
      const v = __eda.state().view;
      let x0 = 1e9, y0 = 1e9, x1 = -1, y1 = -1, n = 0;
      for (let y = 0; y < a.h; y++) for (let x = 0; x < a.w; x++) {
        const i = (y * a.w + x) * 4;
        if (Math.abs(a.d[i] - b.d[i]) + Math.abs(a.d[i + 1] - b.d[i + 1]) + Math.abs(a.d[i + 2] - b.d[i + 2]) > 60) { n++; x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
      }
      const w = (px, py) => [(px / a.dpr - v.x) / v.scale, (py / a.dpr - v.y) / v.scale];
      return { n, from: n ? w(x0, y0) : null, to: n ? w(x1, y1) : null };
    };
    t(__eda.actions().some((a) => a.id === "eeschema.EditorControl.togglePinAltIcons" && a.enabled), "Show Pin Alternate Icons is an enabled action of the Symbol tab");
    // no pin has alternates yet: the toggle changes nothing on the canvas
    const bare = shot();
    await __eda.run("eeschema.EditorControl.togglePinAltIcons");
    await sleep(400);
    t(diffBox(bare, shot()).n === 0, "with no alternates the icons toggle draws nothing");
    await __eda.run("eeschema.EditorControl.togglePinAltIcons");
    await sleep(300);

    // double click on the pin: Pin Properties, the pane for alternates starts closed
    const PIN1 = [-4_000_000, 0];
    await dbl(...PIN1);
    t(!!tid("pin-alternates") && !tid("pin-alternates-add"), "Pin Properties has the alternates pane, closed while the pin has none");
    tid("pin-alternates-toggle").click(); await sleep(150);
    t(!!tid("pin-alternates-add") && !!tid("pin-alternates-delete"), "the turndown opens the grid with its buttons");
    t(/Alternate Pin Name/.test(tid("pin-alternates")?.textContent ?? "") && /Electrical Type/.test(tid("pin-alternates")?.textContent ?? "") && /Graphic Style/.test(tid("pin-alternates")?.textContent ?? ""), "with KiCad's three columns");
    // + adds a row with no name and the pin's own type and style
    tid("pin-alternates-add").click(); await sleep(150);
    t(tid("alt-name-0")?.value === "" && tid("alt-type-0")?.value === "bidirectional" && tid("alt-shape-0")?.value === "line", `a new row has no name and the pin's type and style: ${JSON.stringify([tid("alt-name-0")?.value, tid("alt-type-0")?.value, tid("alt-shape-0")?.value])}`);
    // OK with a nameless row: refused in the dialog's words
    await click(tid("pin-props-ok"), 300);
    t(/Alternate pin definitions must have a name\./.test(tid("pin-props-error")?.textContent ?? "") && !!tid("pin-alternates"), `refused: ${tid("pin-props-error")?.textContent}`);
    await type(tid("alt-name-0"), "UART_TX");
    await choose(tid("alt-type-0"), "output");
    tid("pin-alternates-add").click(); await sleep(150);
    await type(tid("alt-name-1"), "ADC1");
    await choose(tid("alt-type-1"), "input");
    // a name given twice: the last row stays
    tid("pin-alternates-add").click(); await sleep(150);
    await type(tid("alt-name-2"), "UART_TX");
    await choose(tid("alt-shape-2"), "inverted");
    await click(tid("pin-props-ok"), 900);
    t(!tid("pin-alternates"), "OK closes the dialog");
    let sym = await symbol(id);
    let alts = (sym.pins.find((p) => p.number === "1")?.alternates ?? []).map((a) => `${a.name} ${a.electrical_type} ${a.shape}`);
    t(same(alts, ["ADC1 input line", "UART_TX bidirectional inverted"]), `the pin keeps each name once, in the order of the names: ${JSON.stringify(alts)}`);
    t((sym.pins.find((p) => p.number === "2")?.alternates ?? []).length === 0, "the other pin has none");
    // the file
    const text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`)).text();
    t(text.includes('(alternate "ADC1" input line)') && text.includes('(alternate "UART_TX" bidirectional inverted)') && text.indexOf('"ADC1"') < text.indexOf('"UART_TX"'), "the exported .kicad_sym has (alternate ...) inside the pin, by name");
    // the icon: toggling it off changes pixels beside the first pin's name, none at the second pin
    await sleep(500);
    const withIcon = shot();
    await __eda.run("eeschema.EditorControl.togglePinAltIcons");
    await sleep(400);
    const without = shot();
    const d = diffBox(withIcon, without);
    t(d.n >= 20, `Show Pin Alternate Icons draws an icon (${d.n} px)`);
    // pin 1 runs right from (-5.08, 0) to the body at (-2.54, 0); the name is inside from 0.508 mm on, the icon after it, level with the line
    t(!!d.from && d.from[0] > -2_540_000 && d.to[0] < 9_000_000 && d.from[1] > -1_500_000 && d.to[1] < 1_500_000, `the icon is beside the first pin's name, level with its line: ${JSON.stringify([d.from, d.to])}`);
    await __eda.run("eeschema.EditorControl.togglePinAltIcons");
    await sleep(300);
    t(diffBox(withIcon, shot()).n === 0, "and back on, it is drawn again");

    // reopened: the pane starts open with the rows; a deleted row is gone, Cancel keeps the library as it was
    await dbl(...PIN1);
    t(!!tid("pin-alternates-add") && tid("alt-name-0")?.value === "ADC1" && tid("alt-name-1")?.value === "UART_TX", `reopened, the pane is open with the rows: ${tid("alt-name-0")?.value}, ${tid("alt-name-1")?.value}`);
    tid("alt-row-0").click(); await sleep(80);
    tid("pin-alternates-delete").click(); await sleep(150);
    t(!tid("alt-name-1") && tid("alt-name-0")?.value === "UART_TX", "the delete button removes the selected row");
    await click(tid("pin-props-cancel"), 300);
    sym = await symbol(id);
    t(sym.pins.find((p) => p.number === "1")?.alternates?.length === 2, "Cancel keeps both");
    // one undo of the Symbol Editor takes the whole edit back
    await undoFp();
    sym = await symbol(id);
    t((sym.pins.find((p) => p.number === "1")?.alternates ?? []).length === 0, "one undo of the Symbol Editor takes the alternates away");

    // a symbol with two body styles takes none: the pane is disabled with KiCad's tooltip, and the verb refuses
    const cur = await symbol(id);
    r = await cmd({ op: "edit_symbol_properties", lib_id: id, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 1, has_alternate_body_style: true, footprint_filters: [] });
    t(r.ok, `two body styles: ${r.message ?? "ok"}`);
    await sleep(900);
    await dbl(...PIN1);
    t(!!tid("pin-alternates") && !tid("pin-alternates-add") && tid("pin-alternates-toggle")?.disabled === true, "the pane is disabled");
    t(/not available for symbols with multiple body styles/.test(tid("pin-alternates-toggle")?.getAttribute("title") ?? ""), `with KiCad's tooltip: ${tid("pin-alternates-toggle")?.getAttribute("title")}`);
    await click(tid("pin-props-cancel"), 300);
    r = await cmd({ op: "edit_symbol_pin", lib_id: id, id: cur.pins.find((p) => p.number === "1").id, pin: { ...cur.pins.find((p) => p.number === "1"), alternates: [{ name: "A", electrical_type: "input", shape: "line" }] } });
    t(!r.ok && /multiple body styles/.test(r.message ?? JSON.stringify(r)), `the verb refuses alternates: ${r.message ?? JSON.stringify(r)}`);
  }, [/body styles/i]);

  // ================================================================================================ 7. the shape properties dialog
  await scenario("Shape Properties: a double click on a rectangle opens it with the shape's border, width and fill; OK is one undo step (width, fill, common to all units); Border off is the width KiCad keeps for none", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17Shape";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    r = await cmd({ op: "add_symbol_graphic", lib_id: id, graphic: { kind: "rectangle", unit: 1, body_style: 1, start: { x: -5.08, y: 5.08 }, end: { x: 5.08, y: -5.08 }, stroke_mm: 0.254, fill: "none" } });
    t(r.ok, `rectangle: ${r.message ?? "ok"}`);
    const cur = await symbol(id);
    r = await cmd({ op: "edit_symbol_properties", lib_id: id, reference_prefix: "U", description: "", keywords: "", datasheet: "", power: false, in_bom: true, on_board: true, pin_numbers_hidden: false, pin_names_hidden: false, pin_name_offset_mm: cur.pin_name_offset_mm ?? 0.508, unit_count: 2, has_alternate_body_style: false, footprint_filters: [] });
    t(r.ok, `two units: ${r.message ?? "ok"}`);
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const EDGE = [-5_080_000, 0]; // the middle of the rectangle's left edge, the editor's nm (Y down): the top and bottom edges are where a new symbol puts its Reference and Value
    await dbl(...EDGE);
    t(!!tid("shape-properties"), "a double click on the rectangle's edge opens the dialog");
    t(/Rectangle Properties/.test(tid("shape-properties")?.textContent ?? ""), `titled by the shape: ${tid("shape-properties")?.getAttribute("aria-label")}`);
    t(tid("shape-border")?.checked === true && tid("shape-width")?.value === "0.254", `the border is on at 0.254: ${tid("shape-width")?.value}`);
    t(tid("shape-fill-none")?.checked === true, "no fill");
    t(!off("shape-common-units") && off("shape-common-styles"), "Common to all units is enabled for two units, Common to all body styles is not (one body style)");
    t(/Set border width to 0 to use schematic's default line width\./.test(tid("shape-properties")?.textContent ?? ""), "with KiCad's help line");
    await type(tid("shape-width"), "0.5");
    tid("shape-fill-outline").click(); await sleep(80);
    tid("shape-common-units").click(); await sleep(80);
    await click(tid("shape-ok"), 900);
    t(!tid("shape-properties"), "OK closes the dialog");
    let g = (await symbol(id)).graphics[0];
    t(g.stroke_mm === 0.5 && g.fill === "outline" && g.unit === 0, `width 0.5, outline fill, common to all units: ${JSON.stringify([g.stroke_mm, g.fill, g.unit])}`);
    // Border off: the width KiCad keeps for none, written to the file as it
    await dbl(...EDGE);
    t(tid("shape-width")?.value === "0.5" && tid("shape-common-units")?.checked === true, "reopened on what was stored");
    tid("shape-border").click(); await sleep(80);
    t(off("shape-width"), "the width is off while the border is");
    await click(tid("shape-ok"), 900);
    g = (await symbol(id)).graphics[0];
    t(g.stroke_mm === -0.0001, `no border is -0.0001: ${g.stroke_mm}`);
    const text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`)).text();
    t(/\(stroke \(width -0\.0001\) \(type default\)\) \(fill \(type outline\)\)/.test(text), "the exported .kicad_sym says (width -0.0001)");
    // the right-click menu has Properties... for the one selected shape
    await clickAt(...EDGE, {}, false);
    const [sx, sy] = toScreen(...EDGE);
    canvasEl().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: sx, clientY: sy, button: 2 }));
    await sleep(300);
    const labels = [...document.querySelectorAll('[role="menuitem"]')].map((m) => m.textContent.trim());
    t(labels.includes("Properties..."), `the context menu offers Properties...: ${JSON.stringify(labels)}`);
    await closeMenu();
    // one undo per OK
    await undoFp();
    g = (await symbol(id)).graphics[0];
    t(g.stroke_mm === 0.5 && g.fill === "outline", "one undo takes the Border off back");
    await undoFp();
    g = (await symbol(id)).graphics[0];
    t(g.stroke_mm === 0.254 && g.fill === "none" && g.unit === 1, `one more takes the first OK back: ${JSON.stringify([g.stroke_mm, g.fill, g.unit])}`);
  });

  // ================================================================================================ 8. the symbol text box
  await scenario("Symbol Editor text box: the tool draws a box with two clicks and Text Box Properties adds it (Cancel drops it); it is written, wrapped and picked as KiCad does, remembered for the next, edited, moved, deleted", async (t) => {
    await clickTab("Symbol");
    await sleep(500);
    const id = "eda:P17TextBox";
    madeSyms.push(id);
    let r = await cmd({ op: "new_symbol", lib_id: id });
    t(r.ok, `new_symbol: ${r.message ?? "ok"}`);
    await selectRow(id);
    await __eda.run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(500);
    const canvas = () => document.querySelector(".editor-canvas-col canvas");
    /** The pixels near an editor point that are not the background's: the ink of what is drawn there. */
    const inkIn = (x0, y0, x1, y1) => {
      const c = canvas(); const ctx = c.getContext("2d"); const dpr = c.width / parseFloat(c.style.width || c.width); const rect = c.getBoundingClientRect();
      const [ax, ay] = toScreen(x0, y0); const [bx, by] = toScreen(x1, y1);
      const px = Math.max(0, Math.round((Math.min(ax, bx) - rect.left) * dpr)), py = Math.max(0, Math.round((Math.min(ay, by) - rect.top) * dpr));
      const w = Math.max(1, Math.round(Math.abs(bx - ax) * dpr)), h = Math.max(1, Math.round(Math.abs(by - ay) * dpr));
      const d = ctx.getImageData(px, py, w, h).data;
      let n = 0; for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - 26) + Math.abs(d[i + 1] - 26) + Math.abs(d[i + 2] - 26) > 90) n++;
      return n;
    };
    const boxes = async () => (await symbol(id)).graphics.filter((g) => g.kind === "text_box");
    const A = [-7_620_000, -5_080_000], B = [7_620_000, 5_080_000]; // the corners, the editor's nm (Y down): 15.24 by 10.16 mm
    t(__eda.actions().some((a) => a.id === "eeschema.SymbolDrawing.drawSymbolTextBox" && a.enabled), "Draw Text Box is an enabled action of the Symbol tab");
    await arm("eeschema.SymbolDrawing.drawSymbolTextBox", "draw_textbox");
    t(__eda.state().tool === "draw_textbox", `the tool is armed: ${__eda.state().tool}`);
    await clickAt(...A, {}, false);
    t(!tid("text-box-properties"), "one click only begins the box");
    await clickAt(...B, {}, true);
    t(!!tid("text-box-properties"), "the second click opens Text Box Properties");
    t(tid("tb-text")?.value === "" && tid("tb-size")?.value === "1.27" && tid("tb-bold")?.checked === false && tid("tb-h-left")?.checked === true && tid("tb-v-top")?.checked === true && tid("tb-horizontal")?.checked === true, "it starts empty, 1.27 mm, left and top, horizontal, not bold");
    t(tid("tb-border")?.checked === true && tid("tb-filled")?.checked === false, "with a border and no fill");
    t((await boxes()).length === 0, "nothing is added before OK");
    // Cancel drops the box
    await click(tid("tb-cancel"), 500);
    t(!tid("text-box-properties") && (await boxes()).length === 0, "Cancel adds nothing");
    // again: a text of two lines, bold
    await arm("eeschema.SymbolDrawing.drawSymbolTextBox", "draw_textbox");
    await clickAt(...A, {}, false);
    await clickAt(...B, {}, true);
    await type(tid("tb-text"), "10V\nREF");
    tid("tb-bold").click(); await sleep(80);
    await click(tid("tb-ok"), 1000);
    t(!tid("text-box-properties"), "OK closes the dialog");
    let bs = await boxes();
    t(bs.length === 1, `OK adds the box: ${bs.length}`);
    const b0 = bs[0];
    t(b0?.text === "10V\nREF" && b0.bold === true && b0.h_align === "left" && b0.v_align === "top" && b0.size_mm === 1.27 && (b0.angle_deg ?? 0) === 0, `what the dialog said: ${JSON.stringify(b0)}`);
    t(b0 && near(b0.start.x, -7.62, 0.001) && near(b0.start.y, 5.08, 0.001) && near(b0.end.x, 7.62, 0.001) && near(b0.end.y, -5.08, 0.001), `the corners are the clicks (Y up): ${JSON.stringify([b0?.start, b0?.end])}`);
    t(b0?.unit === 1 && b0.margin_mm === 1.016 && b0.stroke_mm === 0, `on the unit being edited, with the margin of four fifths of the text and the default width: ${JSON.stringify([b0?.unit, b0?.margin_mm, b0?.stroke_mm])}`);
    const text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(id)}`)).text();
    t(text.includes('(text_box "10V\\nREF" (at -7.62 5.08 0) (size 15.24 -10.16) (margins 1.016 1.016 1.016 1.016)'), "the exported .kicad_sym has the box as KiCad writes it, the line break as \\n");
    t(inkIn(-7_000_000, -4_500_000, 7_000_000, -3_000_000) > 40, `the text is drawn in the box (${inkIn(-7_000_000, -4_500_000, 7_000_000, -3_000_000)} px)`);
    t(inkIn(-8_200_000, -5_800_000, 8_200_000, -5_600_000) > 40, "and the border is drawn");
    // a click in the box picks it, anywhere in it (not on the border alone)
    await __eda.run("common.InteractiveSelection.selectionTool");
    await sleep(200);
    await clickAt(0, 2_500_000, {}, false);
    t(same(__eda.state().selection.map((s) => s.id), [b0.id]), `a click inside the box picks it: ${JSON.stringify(__eda.state().selection.map((s) => s.id))}`);
    // the next box starts as the last one was: bold
    await arm("eeschema.SymbolDrawing.drawSymbolTextBox", "draw_textbox");
    await clickAt(-12_700_000, 7_620_000, {}, false);
    await clickAt(-10_160_000, 10_160_000, {}, true);
    t(tid("tb-bold")?.checked === true, "the tool remembers bold for the next box (m_lastTextBold)");
    await click(tid("tb-cancel"), 500);
    await __eda.run("common.InteractiveSelection.selectionTool");
    // edit: a double click opens it on what it holds
    await dbl(0, 0);
    t(!!tid("text-box-properties") && tid("tb-text")?.value === "10V\nREF" && tid("tb-bold")?.checked === true, `a double click opens the box: ${tid("tb-text")?.value}`);
    await type(tid("tb-text"), "5k");
    tid("tb-vertical").click(); await sleep(60);
    tid("tb-border").click(); await sleep(60);
    tid("tb-filled").click(); await sleep(60);
    tid("tb-h-center").click(); await sleep(60);
    await type(tid("tb-size"), "0.635");
    await click(tid("tb-ok"), 1000);
    bs = await boxes();
    const e0 = bs[0];
    t(bs.length === 1 && e0.id === b0.id && e0.text === "5k" && e0.angle_deg === 90 && e0.stroke_mm === -0.0001 && e0.fill === "background" && e0.h_align === "center" && e0.size_mm === 0.635, `the edit replaced the box and kept its id: ${JSON.stringify(e0)}`);
    t(e0.margin_mm === 1.016, `and the margin it had: ${e0.margin_mm}`);
    // a size the dialog refuses
    await dbl(0, 0);
    await type(tid("tb-size"), "0");
    await click(tid("tb-ok"), 300);
    t(/between 0\.01 mm and 1000 mm/.test(tid("tb-error")?.textContent ?? "") && !!tid("text-box-properties"), `a text size of 0 is refused: ${tid("tb-error")?.textContent}`);
    await click(tid("tb-cancel"), 400);
    // moved by a drag: both corners
    const before = (await boxes())[0];
    await dragFromTo([0, 0], [2_540_000, 1_270_000]);
    const after = (await boxes())[0];
    t(near(after.start.x - before.start.x, 2.54, 0.001) && near(after.end.x - before.end.x, 2.54, 0.001) && near(after.start.y - before.start.y, -1.27, 0.001), `a drag moves both corners: ${JSON.stringify([before.start, after.start])}`);
    await undoFp();
    // one undo per step: the move, the edit, the add
    t(near((await boxes())[0].start.x, before.start.x, 0.001), "one undo takes the move back");
    await undoFp();
    t((await boxes())[0]?.text === "10V\nREF", "one undo takes the edit back");
    await undoFp();
    t((await boxes()).length === 0, "one undo takes the whole box away");
    // the Select Tool while a box is begun drops it and ends the tool: the next click only selects
    await arm("eeschema.SymbolDrawing.drawSymbolTextBox", "draw_textbox");
    await clickAt(...A, {}, false);
    await __eda.run("common.InteractiveSelection.selectionTool");
    await sleep(300);
    t(__eda.state().tool === "select", `the Select Tool leaves the Text Box tool: ${__eda.state().tool}`);
    await clickAt(...B, {}, true);
    t(!tid("text-box-properties") && (await boxes()).length === 0, "and the box that was begun is gone: no dialog, no box");
    await closeDialogs();
  }, [/derived|text size/i]);

  // ================================================================================================ leave the library as it was
  await closeDialogs();
  for (const n of made) { await cmd({ op: "delete_library_footprint", name: n }); }
  // the symbols made above: the base symbol first (a delete takes the symbols derived from it), then whatever is left
  for (const id of [...madeSyms]) { if ((await symNames()).includes(id)) await cmd({ op: "delete_library_symbol", lib_id: id }); }
  await sleep(500);
  const libraryAfter = { fp: await fpNames(), sym: await symNames() };
  const clean = same(libraryAfter.fp, libraryBefore.fp) && same(libraryAfter.sym, libraryBefore.sym);
  results.push({ name: "the project library is left as it was found", ok: clean, notes: [clean ? "ok: same names" : `FAIL: ${JSON.stringify({ before: libraryBefore, after: libraryAfter })}`] });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
