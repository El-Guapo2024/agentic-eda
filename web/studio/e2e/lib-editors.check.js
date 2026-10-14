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
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
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
  const padOf = (n) => ({ number: n, at: { x: 0, y: 0 }, size: [900, 600], shape: "round_rect", kind: "smd", drill: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"], roundrect_ratio: 0.25 });

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
    t(same(pairs(fp)[0], ["A1", -800, -1200]) && same(pairs(fp)[11], ["C4", 800, 1200]), `centred: ${JSON.stringify(pairs(fp)[0])} .. ${JSON.stringify(pairs(fp)[11])}`);
    // the derived file says the same
    const mod = await (await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`)).text();
    t((mod.match(/\(pad "/g) ?? []).length === 12 && /\(pad "B3" smd roundrect/.test(mod), "the exported .kicad_mod has the twelve pads");
    await undoFp();
    t((await footprint(name)).pads.length === 1, "one undo of the Footprint Editor takes the array away");
  });

  await scenario("Use first free number skips the pads that stay; Renumber pads off keeps the number of the source", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("1"), padOf("3"), { ...padOf("9"), at: { x: 0, y: 3000 } }]);
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
    const { name, ids } = await arrayDialogOn(t, [{ ...padOf("X"), at: { x: 5000, y: 0 } }]);
    await openArray(ids, "circular");
    t(!!tid("array-pad-numbering-circular"), "the circular tab has its Numbering Options");
    await baseCircle();
    await type(tid("array-center-x"), "0"); await type(tid("array-center-y"), "0");
    await setChecked("array-rotate-items", true);
    await choose(tid("array-circular-kind"), "alpha_full");
    await click(tid("array-ok"), 1000);
    const fp = await footprint(name);
    const got = fp.pads.map((p) => [p.number, p.at.x, p.at.y, p.rot ?? 0]).sort();
    t(same(got, [["A", 5000, 0, 0], ["B", 0, 5000, 270000], ["C", -5000, 0, 180000], ["D", 0, -5000, 90000]]), `around the circle: ${JSON.stringify(got)}`);
    await undoFp();
  });

  await scenario("the dialog names a start the alphabet lacks and keeps the dialog open; Arrange selection numbers nothing", async (t) => {
    const { name, ids } = await arrayDialogOn(t, [padOf("1"), { ...padOf("2"), at: { x: 0, y: 4000 } }]);
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
    t(fp.pads.length === 2 && same(pairs(fp), [["1", 0, 0], ["2", 1000, 0]]), `the second pad is on the second point, numbers kept: ${JSON.stringify(pairs(fp))}`);
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
    const baseFields = (await symbol(BASE)).fields.map((f) => (f.name === "MPN" ? { ...f, text: "B-2", at: { x: 3, y: -9 } } : f));
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
    t(fieldOf(own, "MPN")?.text === "B-2" && same(fieldOf(own, "MPN")?.at, { x: 3, y: -9 }), `MPN follows the parent: ${JSON.stringify(fieldOf(own, "MPN"))}`);
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
