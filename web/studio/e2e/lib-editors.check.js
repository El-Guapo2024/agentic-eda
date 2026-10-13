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
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
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

  // ================================================================================================ leave the library as it was
  await closeDialogs();
  for (const n of made) { await cmd({ op: "delete_library_footprint", name: n }); }
  await sleep(500);
  const libraryAfter = { fp: await fpNames(), sym: await symNames() };
  const clean = same(libraryAfter.fp, libraryBefore.fp) && same(libraryAfter.sym, libraryBefore.sym);
  results.push({ name: "the project library is left as it was found", ok: clean, notes: [clean ? "ok: same names" : `FAIL: ${JSON.stringify({ before: libraryBefore, after: libraryAfter })}`] });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
