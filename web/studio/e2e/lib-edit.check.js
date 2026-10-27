// Library editing basics check (GAPS.md item 33, package P20; items 19 and 22): what a person does first in the two library editors. In the Footprint Editor:
// New Footprint, two pads put down with the pad tool (numbered 1 and 2), a pad's Number and Position through Pad Properties (E), a pad moved by a drag, turned
// (R) and deleted (Del), a silkscreen line drawn with the line tool, every edit one undo step, and Delete Footprint with its confirmation, in KiCad's words. In
// the Symbol Editor: New Symbol (Ctrl+N asks for the name first, `DIALOG_LIB_NEW_SYMBOL`, and refuses an empty or a taken one in the dialog), two pins put down
// with the pin tool (each click asks in Pin Properties first -- Cancel places nothing; the second is numbered after the first), a pin's name and number through Pin Properties (E),
// a rectangle drawn, a pin moved, turned and deleted, undo for each, and Delete Symbol. Every edit is read back from the server (`GET /api/footprint`,
// `/api/footprint_library`, `/api/symbol`, `/api/symbol_editor/names`) and the project library is left with the names it had.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir> --port N --ui web/studio/dist`) with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/
// web/studio/e2e/lib-edit.check.js`, or pasted into the Browser pane's javascript tool, on a COPY of a board.
// It answers through `window.__libEditCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __libEditCheck.wait(30000)` until `.done`.
// It works through `window.__eda` (actions by id; `__eda.state().view` maps the editor's um to the canvas), the canvas' pointer events and the dialogs' fields
// (by their labels) and buttons.
// run-all-checks: board=mcu30 minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__libEditCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const run = (id, args) => __eda.run(id, args);
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const symNames = async () => (await json("/api/symbol_editor/names")).project ?? [];
  const footprint = (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const symbol = (libId) => json(`/api/symbol?lib_id=${encodeURIComponent(libId)}`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(900); };
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(600); };
  const redo = async () => { await run("common.Interactive.redo"); await sleep(600); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(400); };
  const toSelect = async () => { for (let i = 0; i < 3 && __eda.state().tool !== "select"; i++) await escape(); };
  const arm = async (action, tool) => { if (__eda.state().tool !== tool) { await run(action); await sleep(300); } return __eda.state().tool === tool; };
  const dialog = () => document.querySelector(".dialog");
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  /** The field after a label of a dialog's grid (`<span>Label</span><input>`). */
  const fieldOf = (label) => { const s = [...(dialog()?.querySelectorAll(".kv-grid > span") ?? [])].find((e) => e.textContent.trim() === label); const n = s?.nextElementSibling; return n?.matches("input,select") ? n : n?.querySelector("input,select") ?? null; };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 4 && document.querySelector(".dialog-backdrop"); i++) { const b = [...document.querySelectorAll(".dialog-footer button")].find((x) => /^(Cancel|Close)$/.test(x.textContent.trim())); if (b) b.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(250); } };

  // The canvas of the editor tab: pointer events at the editor's point (um, y down).
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, wx, wy, buttons = 0) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", isPrimary: true })); };
  const clickAt = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(80); fire("pointerdown", wx, wy, 1); await sleep(50); fire("pointerup", wx, wy); await sleep(600); };
  const dblAt = async (wx, wy) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: x, clientY: y, detail: 2 })); await sleep(700); };
  const dragFromTo = async (a, b) => {
    fire("pointermove", a[0], a[1]); await sleep(80);
    fire("pointerdown", a[0], a[1], 1); await sleep(60);
    for (let i = 1; i <= 5; i++) { fire("pointermove", a[0] + ((b[0] - a[0]) * i) / 5, a[1] + ((b[1] - a[1]) * i) / 5, 1); await sleep(70); }
    fire("pointerup", b[0], b[1]); await sleep(800);
  };
  /** A view where 1 mm is a fair number of pixels round the editor's origin: fit, then zoom in until the scale is good for clicking. */
  const frame = async () => { await run("common.Control.zoomFitScreen"); await sleep(400); for (let i = 0; i < 6 && !(__eda.state().view?.scale > 0.00002); i++) { await run("common.Control.zoomInCenter"); await sleep(200); } };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); await toSelect(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  const before = { fp: await fpNames(), sym: await symNames() };
  let FP = null;
  let SYM = null;

  (async () => {
    // ================================================================================================ the Footprint Editor
    await scenario("New Footprint makes an empty footprint in the project library and opens it", async (t) => {
      await clickTab("Footprint");
      t(__eda.state().tab === "footprint", `the Footprint Editor tab (${__eda.state().tab})`);
      const r = await run("pcbnew.ModuleEditor.newFootprint");
      FP = await until(async () => (await fpNames()).find((n) => !before.fp.includes(n)), 8000);
      t(r.ok !== false && !!FP, `a new name in the library: ${FP}`);
      if (!FP) return;
      const fp = await footprint(FP);
      t((fp.pads ?? []).length === 0, `it has no pads (${(fp.pads ?? []).length})`);
      await frame();
      t(__eda.state().view?.scale > 0, "the editor shows it");
    });

    await scenario("the pad tool puts a pad where it is clicked, the next one numbered after it; one undo step each", async (t) => {
      if (!FP) return t(false, "no footprint");
      t(await arm("pcbnew.PadTool.placePad", "pad"), `the pad tool is armed (${__eda.state().tool})`);
      await clickAt(0, 0);
      await clickAt(2_500_000, 0);
      await toSelect();
      const pads = (await footprint(FP)).pads ?? [];
      t(pads.length === 2, `two pads (${pads.length})`);
      const byNum = Object.fromEntries(pads.map((p) => [p.number, p]));
      t(!!byNum["1"] && !!byNum["2"], `numbered 1 and 2 (${pads.map((p) => p.number)})`);
      t(byNum["1"]?.at.x === 0 && byNum["1"]?.at.y === 0 && byNum["2"]?.at.x === 2_500_000 && byNum["2"]?.at.y === 0, `where they were clicked (${pads.map((p) => [p.number, p.at.x, p.at.y].join(":"))})`);
      await undo();
      t(((await footprint(FP)).pads ?? []).length === 1, "one undo takes the second pad away");
      await redo();
      t(((await footprint(FP)).pads ?? []).length === 2, "redo puts it back");
    });

    await scenario("Pad Properties (E) edits a pad's number and position; OK is one undo step", async (t) => {
      const pads = (await footprint(FP)).pads ?? [];
      const p2 = pads.find((p) => p.number === "2");
      if (!t(!!p2, "pad 2 is there")) return;
      await select([p2.id]);
      const r = await run("pcbnew.InteractiveEdit.properties");
      await sleep(400);
      t(/Pad Properties/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), `E opens Pad Properties (${r.dialog})`);
      const num = fieldOf("Number");
      t(num?.value === "2", `its Number reads 2 (${num?.value})`);
      setValue(num, "5");
      const pos = [...(fieldOf("Position")?.parentElement?.querySelectorAll("input") ?? [])];
      if (pos.length >= 2) setValue(pos[1], "1.5");
      await sleep(100);
      buttonText(dialog(), "OK")?.click();
      await sleep(900);
      const p = ((await footprint(FP)).pads ?? []).find((q) => q.id === p2.id) ?? ((await footprint(FP)).pads ?? []).find((q) => q.number === "5");
      t(p?.number === "5", `the pad is numbered 5 (${p?.number})`);
      t(pos.length < 2 || (p?.at.x === 2_500_000 && p?.at.y === 1_500_000), `and moved to (2.5, 1.5) mm (${p?.at.x}, ${p?.at.y})`);
      await undo();
      const back = ((await footprint(FP)).pads ?? []).find((q) => q.number === "2");
      t(!!back && back.at.y === 0, "one undo gives it its number and place back");
    });

    await scenario("a pad dragged moves; R turns it; Del deletes it; one undo step each", async (t) => {
      const p2 = ((await footprint(FP)).pads ?? []).find((p) => p.number === "2");
      if (!t(!!p2, "pad 2 is there")) return;
      await select([]);
      await dragFromTo([p2.at.x, p2.at.y], [p2.at.x + 1_500_000, p2.at.y + 2_500_000]);
      let p = ((await footprint(FP)).pads ?? []).find((q) => q.number === "2");
      t(p.at.x === p2.at.x + 1_500_000 && p.at.y === p2.at.y + 2_500_000, `the drag moved it by (1.5, 2.5) mm: (${p2.at.x}, ${p2.at.y}) -> (${p.at.x}, ${p.at.y})`);
      await undo();
      p = ((await footprint(FP)).pads ?? []).find((q) => q.number === "2");
      t(p.at.x === p2.at.x && p.at.y === p2.at.y, "undo puts it back");
      await select([p2.id]);
      await run("pcbnew.InteractiveEdit.rotateCcw");
      await sleep(700);
      p = ((await footprint(FP)).pads ?? []).find((q) => q.number === "2");
      t(Math.abs(((p.rot ?? 0) - (p2.rot ?? 0) + 360_000) % 180_000) === 90_000 || Math.abs(((p.rot ?? 0) - (p2.rot ?? 0) + 360_000) % 180) === 90, `R turns it a quarter (${p2.rot} -> ${p.rot})`);
      await undo();
      await select([p2.id]);
      await run("common.Interactive.delete");
      await sleep(700);
      t(!((await footprint(FP)).pads ?? []).some((q) => q.number === "2"), "Del deletes it");
      await undo();
      t(((await footprint(FP)).pads ?? []).some((q) => q.number === "2"), "undo brings it back");
    });

    await scenario("the line tool draws a silkscreen line: two clicks and a double click; one undo step", async (t) => {
      const g0 = ((await footprint(FP)).graphics ?? []).length;
      t(await arm("pcbnew.InteractiveDrawing.line", "draw_segment"), `the line tool is armed (${__eda.state().tool})`);
      await clickAt(-2_500_000, -2_500_000);
      await clickAt(5_000_000, -2_500_000);
      await dblAt(5_000_000, -2_500_000);
      await toSelect();
      const g = (await footprint(FP)).graphics ?? [];
      const mine = g.slice(g0);
      t(mine.length === 1, `one line (${mine.length})`);
      const s = mine[0];
      t(!!s && s.start?.x === -2_500_000 && s.start?.y === -2_500_000 && s.end?.x === 5_000_000 && s.end?.y === -2_500_000, `from the first click to the second (${JSON.stringify(s && [s.start, s.end])})`);
      t(!!s && /SilkS/.test(s.layer ?? ""), `on the silkscreen, the editor's first active layer (${s?.layer})`);
      await undo();
      t(((await footprint(FP)).graphics ?? []).length === g0, "one undo takes it away");
    });

    await scenario("Delete Footprint asks first, in KiCad's words, and takes it out of the library", async (t) => {
      if (!FP) return t(false, "no footprint");
      // the footprint must be the one selected in the library tree: it is, as it was just made and opened
      const r = run("pcbnew.ModuleEditor.deleteFootprint");
      const ask = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Delete footprint/.test(d.innerText)), 5000);
      const item = FP.includes(":") ? FP.slice(FP.indexOf(":") + 1) : FP;
      t(!!ask && new RegExp(`Delete footprint '${item}' from library '`).test(ask.innerText), `it asks "Delete footprint '${item}' from library ...?" (${ask?.innerText.replace(/\n/g, " / ").slice(0, 120)})`);
      if (!ask) { await r; return; }
      buttonText(ask, "Delete")?.click();
      await r;
      await sleep(800);
      t(!(await fpNames()).includes(FP), "the footprint is gone from the library");
      FP = (await fpNames()).includes(FP) ? FP : null;
    });

    // ================================================================================================ the Symbol Editor
    await scenario("New Symbol starts with All units are interchangeable unchecked and Pin name inside checked; the pin name offset follows them", async (t) => {
      await clickTab("Symbol");
      const openNew = async () => {
        await run("eeschema.SymbolLibraryControl.newSymbol");
        return until(() => /New Symbol/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 5000);
      };
      const made = [];
      let open = await openNew();
      if (!t(!!open, "the dialog opens")) return;
      const interchangeable = document.querySelector('[data-testid="new-symbol-interchangeable"]');
      const namesInside = document.querySelector('[data-testid="new-symbol-names-inside"]');
      t(!!interchangeable && !interchangeable.checked, `All units are interchangeable starts unchecked (${interchangeable?.checked})`);
      t(!!namesInside && namesInside.checked, `Pin name inside starts checked (${namesInside?.checked})`);
      // KiCad's CreateSymbol: two units with the box unchecked is LockUnits( true ); inside with an offset of 0 is 1 internal unit (SetPinNameOffset( 1 ))
      const nameA = `P20Lock${Date.now() % 100000}`;
      setValue(fieldOf("Symbol name"), nameA);
      setValue(fieldOf("Number of units per package"), 2);
      setValue(fieldOf("Pin name position offset"), 0);
      buttonText(dialog(), "OK")?.click();
      const idA = await until(async () => (await symNames()).find((n) => n === `eda:${nameA}`), 8000);
      if (t(!!idA, `a two-unit symbol is made (${idA})`)) {
        made.push(idA);
        const s = await symbol(idA);
        t(s.unit_count === 2 && s.units_locked === true, `two units and All units are interchangeable unchecked is locked (${JSON.stringify([s.unit_count, s.units_locked])})`);
        t(s.pin_name_offset_mm === 0.0001, `Pin name inside with an offset of 0 is 1 IU, 0.0001 mm (${s.pin_name_offset_mm})`);
      }
      // outside: "Pin name inside" unchecked writes an offset of 0
      open = await openNew();
      if (!t(!!open, "the dialog opens again")) return;
      const nameB = `P20Out${Date.now() % 100000}`;
      setValue(fieldOf("Symbol name"), nameB);
      document.querySelector('[data-testid="new-symbol-names-inside"]')?.click();
      await sleep(100);
      buttonText(dialog(), "OK")?.click();
      const idB = await until(async () => (await symNames()).find((n) => n === `eda:${nameB}`), 8000);
      if (t(!!idB, `a symbol without pin names inside is made (${idB})`)) {
        made.push(idB);
        const s = await symbol(idB);
        t(s.pin_name_offset_mm === 0, `Pin name inside unchecked writes an offset of 0 (${s.pin_name_offset_mm})`);
      }
      for (const id of made) await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ strict: false, cmd: { op: "delete_library_symbol", lib_id: id } }) });
      await sleep(400);
      t(!(await symNames()).some((n) => made.includes(n)), "the two symbols are out of the library again");
    });

    await scenario("New Symbol asks for the name first, in KiCad's own dialog, and makes the symbol on OK", async (t) => {
      await clickTab("Symbol");
      t(__eda.state().tab === "symbol", `the Symbol Editor tab (${__eda.state().tab})`);
      await run("eeschema.SymbolLibraryControl.newSymbol");
      const open = await until(() => /New Symbol/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 5000);
      t(!!open, `Ctrl+N opens New Symbol (${dialog()?.querySelector(".dialog-header")?.textContent})`);
      if (!open) return;
      const prefilled = await until(() => fieldOf("Symbol name")?.value, 4000);
      t(/^Untitled(_\d+)?$/.test(prefilled ?? ""), `the name starts as the free Untitled (${prefilled})`);
      setValue(fieldOf("Symbol name"), "   ");
      buttonText(dialog(), "OK")?.click();
      await sleep(300);
      const err = document.querySelector('[data-testid="new-symbol-error"]');
      t(/Symbol must have a name\./.test(err?.textContent ?? ""), `an empty name is refused in KiCad's words (${err?.textContent})`);
      t(!!dialog(), "and the dialog stays open");
      const name = `P20New${Date.now() % 100000}`;
      setValue(fieldOf("Symbol name"), name);
      buttonText(dialog(), "OK")?.click();
      SYM = await until(async () => (await symNames()).find((n) => !before.sym.includes(n)), 8000);
      t(SYM === `eda:${name}`, `the symbol made is eda:${name} (${SYM})`);
      if (!SYM) return;
      t(((await symbol(SYM)).pins ?? []).length === 0, "with no pins");
      t(!dialog(), "the dialog is gone");
      await frame();
    });

    await scenario("New Symbol refuses a name the library has, in the dialog; Cancel makes nothing", async (t) => {
      if (!SYM) return t(false, "no symbol");
      const item = SYM.includes(":") ? SYM.slice(SYM.indexOf(":") + 1) : SYM;
      const n0 = (await symNames()).length;
      await run("eeschema.SymbolLibraryControl.newSymbol");
      const open = await until(() => /New Symbol/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 5000);
      if (!t(!!open, "the dialog opens")) return;
      setValue(fieldOf("Symbol name"), item);
      buttonText(dialog(), "OK")?.click();
      await sleep(400);
      const err = document.querySelector('[data-testid="new-symbol-error"]');
      t(/Symbol '.+' already exists in library 'eda'\./.test(err?.textContent ?? ""), `the taken name is refused in KiCad's words (${err?.textContent})`);
      t(!/Overwrite/.test(err?.textContent ?? ""), "and offers no Overwrite, which the verb cannot do");
      t(!!dialog(), "the dialog stays open");
      buttonText(dialog(), "Cancel")?.click();
      await sleep(400);
      t(!dialog(), "Cancel closes it");
      t((await symNames()).length === n0, "and makes nothing");
    });

    await scenario("the pin tool asks for the new pin's properties first (no number, as CreatePin leaves it): Cancel places nothing, OK places it; Insert repeats it numbered and named one up", async (t) => {
      if (!SYM) return t(false, "no symbol");
      t(await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin"), `the pin tool is armed (${__eda.state().tool})`);
      await clickAt(-5_080_000, 0);
      let open = await until(() => /Pin Properties/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 4000);
      t(!!open, `the click opens Pin Properties first (${dialog()?.querySelector(".dialog-header")?.textContent})`);
      t(fieldOf("Pin number")?.value === "", `the new pin has no number in the dialog (${fieldOf("Pin number")?.value})`);
      await closeDialogs(); // Cancel: SYMBOL_EDITOR_PIN_TOOL::CreatePin drops the pin
      await sleep(400);
      t(((await symbol(SYM)).pins ?? []).length === 0, "Cancel places no pin");
      t(__eda.state().tool === "pin", `and the pin tool stays armed (${__eda.state().tool})`);
      await clickAt(-5_080_000, 0);
      open = await until(() => /Pin Properties/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 4000);
      t(!!open, "a second click asks again");
      setValue(fieldOf("Pin number"), "1");
      setValue(fieldOf("Pin name"), "A1");
      await sleep(100);
      buttonText(dialog(), "OK")?.click();
      await sleep(900);
      await toSelect();
      // Insert: SYMBOL_EDITOR_PIN_TOOL::RepeatPin -- 100 mil down for a horizontal pin, IncrementString on the name and the number
      await run("eeschema.InteractiveEdit.repeatDrawItem");
      await sleep(900);
      const rep = ((await symbol(SYM)).pins ?? []).find((p) => p.number === "2");
      t(!!rep && rep.name === "A2", `Insert repeats the last pin with the next number and name (${JSON.stringify(rep && [rep.number, rep.name])})`);
      const pins = (await symbol(SYM)).pins ?? [];
      t(pins.length === 2, `two pins (${pins.length})`);
      const nums = pins.map((p) => p.number).sort();
      t(nums.length === 2 && Number(nums[1]) === Number(nums[0]) + 1, `the second numbered after the first (${nums})`);
      // the editor's y points down, a library symbol's up
      t(pins.some((p) => Math.abs(p.at.x + 5.08) < 1e-6 && Math.abs(p.at.y) < 1e-6) && pins.some((p) => Math.abs(p.at.x + 5.08) < 1e-6 && Math.abs(p.at.y + 2.54) < 1e-6), `where they were clicked (${pins.map((p) => [p.at.x, p.at.y].join(","))})`);
      await undo();
      t(((await symbol(SYM)).pins ?? []).length === 1, "one undo takes the second pin away");
      await redo();
    });

    await scenario("Pin Properties (E) edits a pin's name and number; OK is one undo step", async (t) => {
      const pins = (await symbol(SYM)).pins ?? [];
      const pin = pins.slice().sort((a, b) => a.at.y - b.at.y)[0];
      if (!t(!!pin, "a pin is there")) return;
      await select([pin.id ?? `pin:${pin.number}`]);
      await sleep(200);
      if (__eda.state().selection.length === 0) { await clickAt(pin.at.x * 1e6, -pin.at.y * 1e6); }
      await run("eeschema.InteractiveEdit.properties");
      const open = await until(() => /Pin Properties/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 4000);
      t(!!open, `E opens Pin Properties (${dialog()?.querySelector(".dialog-header")?.textContent})`);
      if (!open) return;
      setValue(fieldOf("Pin name"), "VIN");
      setValue(fieldOf("Pin number"), "7");
      await sleep(100);
      document.querySelector('[data-testid="pin-props-ok"]')?.click();
      await sleep(900);
      const p = ((await symbol(SYM)).pins ?? []).find((q) => q.number === "7");
      t(!!p && p.name === "VIN" && p.at.x === pin.at.x && p.at.y === pin.at.y, `the pin is VIN, number 7, where it was (${JSON.stringify(p && [p.name, p.number])})`);
      await undo();
      t(((await symbol(SYM)).pins ?? []).some((q) => q.number === pin.number && q.name === pin.name), "one undo gives it its name and number back");
    });

    await scenario("the pin template is the last Pin Properties OK, not the selected pin, and carries no name", async (t) => {
      if (!SYM) return t(false, "no symbol");
      const openPinDialog = async () => until(() => /Pin Properties/.test(dialog()?.querySelector(".dialog-header")?.textContent ?? ""), 4000);
      t(await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin"), `the pin tool is armed (${__eda.state().tool})`);
      // A pin with a look of its own: its name and length are what the next click starts with (`g_LastPin*` has no name, so only the length is).
      await clickAt(-2_540_000, 5_080_000);
      if (!t(!!(await openPinDialog()), "a click opens Pin Properties")) return;
      setValue(fieldOf("Pin name"), "TMPL");
      setValue(fieldOf("Pin length"), 5.08);
      buttonText(dialog(), "OK")?.click();
      await sleep(900);
      // The next click starts from that look, but without the name.
      await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
      await clickAt(-2_540_000, 7_620_000);
      if (!t(!!(await openPinDialog()), "the next click asks again")) return;
      t(fieldOf("Pin name")?.value === "", `the template carries no name (${fieldOf("Pin name")?.value})`);
      t(Number(fieldOf("Pin length")?.value) === 5.08, `but the last OK's length (${fieldOf("Pin length")?.value})`);
      buttonText(dialog(), "Cancel")?.click();
      await sleep(500);
      // Merely selecting another pin (2.54 mm long) must not become the template.
      const other = ((await symbol(SYM)).pins ?? []).find((q) => q.name === "");
      if (!t(!!other, "another pin is there")) return;
      await select([other.id ?? `pin:${other.number}`]);
      await sleep(500);
      await arm("eeschema.SymbolDrawing.placeSymbolPin", "pin");
      await clickAt(-2_540_000, 10_160_000);
      if (!t(!!(await openPinDialog()), "and once more")) return;
      t(Number(fieldOf("Pin length")?.value) === 5.08, `the selected pin did not reset it (${fieldOf("Pin length")?.value})`);
      buttonText(dialog(), "Cancel")?.click();
      await sleep(500);
    });

    await scenario("a rectangle drawn with two clicks; a pin dragged, turned (R) and deleted (Del); one undo step each", async (t) => {
      const g0 = ((await symbol(SYM)).graphics ?? []).length;
      t(await arm("eeschema.SymbolDrawing.drawSymbolRectangle", "draw_rect") || await arm("eeschema.InteractiveDrawing.drawRectangle", "draw_rect") || __eda.state().tool !== "select", `the rectangle tool is armed (${__eda.state().tool})`);
      await clickAt(-2_540_000, -2_540_000);
      await clickAt(2_540_000, 5_080_000);
      await toSelect();
      const g = (await symbol(SYM)).graphics ?? [];
      t(g.length === g0 + 1 && g[g.length - 1]?.kind === "rectangle", `a rectangle (${JSON.stringify(g.slice(g0).map((x) => [x.kind, x.start, x.end]))})`);
      await undo();
      t(((await symbol(SYM)).graphics ?? []).length === g0, "one undo takes it away");
      const pin = ((await symbol(SYM)).pins ?? []).slice().sort((a, b) => a.at.y - b.at.y)[0];
      const n = pin.number;
      await select([]);
      await dragFromTo([pin.at.x * 1e6, -pin.at.y * 1e6], [pin.at.x * 1e6 - 2_540_000, -pin.at.y * 1e6]);
      let p = ((await symbol(SYM)).pins ?? []).find((q) => q.number === n);
      t(Math.abs(p.at.x - (pin.at.x - 2.54)) < 1e-6 && Math.abs(p.at.y - pin.at.y) < 1e-6, `the drag moved it 2.54 mm left (${pin.at.x} -> ${p.at.x})`);
      await undo();
      await select([p.id ?? `pin:${n}`]);
      if (__eda.state().selection.length === 0) await clickAt(pin.at.x * 1e6, -pin.at.y * 1e6);
      await run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(700);
      p = ((await symbol(SYM)).pins ?? []).find((q) => q.number === n);
      t(Math.abs(((p.angle_deg - pin.angle_deg) + 360) % 180) === 90, `R turns it a quarter (${pin.angle_deg} -> ${p.angle_deg})`);
      await undo();
      if (__eda.state().selection.length === 0) await clickAt(pin.at.x * 1e6, -pin.at.y * 1e6);
      await run("common.Interactive.delete");
      await sleep(700);
      t(!((await symbol(SYM)).pins ?? []).some((q) => q.number === n), "Del deletes it");
      await undo();
      t(((await symbol(SYM)).pins ?? []).some((q) => q.number === n), "undo brings it back");
    });

    await scenario("Delete Symbol takes it out of the library at once (KiCad asks only for a base symbol or one with unsaved changes)", async (t) => {
      if (!SYM) return t(false, "no symbol");
      const r = run("eeschema.SymbolLibraryControl.deleteSymbol");
      await sleep(600);
      t(!document.querySelector(".dialog-backdrop"), `no question for a plain symbol whose edits are all stored (${__eda.state().dialogs})`);
      await closeDialogs();
      await r;
      await sleep(600);
      t(!(await symNames()).includes(SYM), "the symbol is gone from the library");
      SYM = (await symNames()).includes(SYM) ? SYM : null;
    });

    // ================================================================================================ the end
    await clickTab("PCB");
    if (FP) await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ strict: false, cmd: { op: "delete_library_footprint", name: FP } }) });
    if (SYM) await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ strict: false, cmd: { op: "delete_library_symbol", lib_id: SYM } }) });
    await sleep(400);
    await scenario("the project library has the names it had, and the page logged no error", async (t) => {
      const after = { fp: await fpNames(), sym: await symNames() };
      t(JSON.stringify(after) === JSON.stringify(before), `the same names (${JSON.stringify({ before, after })})`);
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
