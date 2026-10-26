// Page Settings and the drill/place file origin check (GAPS.md item 33, the checks P20 left uncovered; PARITY-common.md section 9 and
// PARITY-boardctl.md section 4): File > Page Settings... on the board and on the schematic -- the paper and the title block saved as one undo step
// (`set_board_page` / `set_schematic_page`), Cancel and an unchanged OK sending nothing -- and Place > Drill/Place File Origin -- the picker takes
// one click, the origin lands on the snapped point, the tool ends, Esc cancels, and Reset Drill/Place Origin puts it back to (0, 0) through
// `set_aux_origin` with `at: null`.
// Runs on a fresh `eda board new examples/mcu_board_30plus.yaml` board (`eda board serve -C <dir> --port N --ui web/studio/dist`); it changes the
// page settings of both documents and the board's aux origin, so run it on a scratch copy. Run it with `PLAYWRIGHT_MODULE=... node
// web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/board-page-origin.check.js`, or paste it into the Browser pane's javascript tool.
// It answers through `window.__boardPageCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __boardPageCheck.wait(120000)`.
// run-all-checks: board=mcu30 minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__boardPageCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 30000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(200); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const schematic = () => json("/api/schematic");
  const run = (id, args) => __eda.run(id, args);
  const dialog = () => document.querySelector(".dialog");
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
  const buttonText = (root, re) => [...(root?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const field = (label) => dialog()?.querySelector(`[aria-label="${label}"]`);
  const paperOf = (doc) => doc?.page?.paper ?? "A4";
  const titleOf = (doc) => doc?.title_block?.title ?? "";
  const close = async () => { for (let i = 0; i < 3 && dialog(); i++) { (buttonText(dialog(), /^Cancel$/) ?? [...(dialog()?.querySelectorAll("button") ?? [])].find((b) => /^Cancel$/.test(b.textContent.trim())))?.click(); await sleep(250); } };
  const tabTo = async (label) => { [...document.querySelectorAll(".editor-tabs .editor-tab")].find((e) => e.textContent.trim() === label)?.click(); await sleep(900); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await close(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  const st = await state();
  if (!st.parts?.length) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board has no footprints (eda board new examples/mcu_board_30plus.yaml)"] }); check.done = true; return "no board"; }
  if (__eda.state().tab !== "pcb") await tabTo("PCB");

  (async () => {
    await scenario("Page Settings on the board: paper and title block in one undo step, Cancel and an unchanged OK sending nothing", async (t) => {
      const before = await state();
      const r = await run("common.Control.pageSettings");
      t(r.dialog === "Page Settings", `File > Page Settings... opens "Page Settings" (${r.dialog ?? r.error ?? header()})`);
      if (!field("Size")) { t(false, "the dialog has the paper's Size picker"); return; }
      t([...field("Size").options].map((o) => o.value).includes("A3"), `its formats are KiCad's (${[...field("Size").options].map((o) => o.value).join(", ")})`);
      setValue(field("Size"), "A3");
      setValue(field("Title"), "P9A page");
      setValue(field("Company"), "BraneBoard");
      await sleep(200);
      buttonText(dialog(), /^OK$/)?.click();
      const closed = await until(async () => !dialog(), 15000);
      if (!t(closed, "OK closes the dialog")) return;
      const after = await state();
      t(paperOf(after) === "A3", `the board's paper is A3 (${paperOf(after)})`);
      t(titleOf(after) === "P9A page" && (after.title_block?.company ?? "") === "BraneBoard", `and the title block carries the title and company (${JSON.stringify(after.title_block)})`);
      // The activity log holds the CLI line the command would be typed as (`cmd_line`, crates/cli/src/board.rs), not the op name: one `page`
      // entry, not a batch of two, is what "one set_board_page command" means here.
      const act1 = after.activity?.[0] ?? null;
      const pageLine = 'page --paper A3 --title "P9A page"';
      t(act1?.cmd === pageLine && act1?.ok === true && (after.activity?.length ?? 0) === (before.activity?.length ?? 0) + 1,
        `the change is one set_board_page command (${act1?.cmd ?? null}; ${before.activity?.length ?? 0} -> ${after.activity?.length ?? 0} activity entries)`);
      await run("common.Interactive.undo");
      const undone = await until(async () => { const b = await state(); return paperOf(b) === paperOf(before) ? b : null; }, 15000);
      t(undone, `one Ctrl+Z puts the paper back (${paperOf(undone)})`);
      t(titleOf(undone) === titleOf(before), `and the title block with it (${JSON.stringify(undone?.title_block ?? null)})`);
      // Cancel: the edits are dropped.
      await run("common.Control.pageSettings");
      setValue(field("Size"), "A5");
      setValue(field("Title"), "not saved");
      await sleep(200);
      buttonText(dialog(), /^Cancel$/)?.click();
      await until(async () => !dialog(), 10000);
      const kept = await state();
      t(paperOf(kept) === paperOf(before) && titleOf(kept) === titleOf(before), `Cancel leaves the board as it was (${paperOf(kept)}, ${JSON.stringify(kept.title_block ?? null)})`);
      // Nothing changed: OK sends nothing (`changed` false -- no commit, no activity entry).
      const act2 = (await state()).activity?.[0] ?? null;
      await run("common.Control.pageSettings");
      buttonText(dialog(), /^OK$/)?.click();
      await until(async () => !dialog(), 10000);
      const act3 = (await state()).activity?.[0] ?? null;
      t((act3?.t ?? null) === (act2?.t ?? null) && (act3?.cmd ?? null) === (act2?.cmd ?? null), `an OK with nothing changed sends no command (${act3?.cmd ?? null})`);
    });

    await scenario("Page Settings on the schematic: the same dialog saves the sheet's own paper", async (t) => {
      await tabTo("Schematic");
      const before = await schematic();
      const r = await run("common.Control.pageSettings");
      t(r.dialog === "Page Settings", `File > Page Settings... opens the dialog on the Schematic tab (${r.dialog ?? r.error})`);
      if (!field("Size")) { t(false, "the dialog has the Size picker"); return; }
      setValue(field("Size"), "A5");
      setValue(field("Title"), "P9A sheet");
      await sleep(200);
      buttonText(dialog(), /^OK$/)?.click();
      const closed = await until(async () => !dialog(), 15000);
      if (!t(closed, "OK closes the dialog")) return;
      const after = await until(async () => { const s = await schematic(); return paperOf(s) === "A5" ? s : null; }, 15000);
      t(after, `the schematic's paper is A5 (${paperOf(await schematic())})`);
      t(titleOf(await schematic()) === "P9A sheet", `and its title block the one typed (${JSON.stringify((await schematic()).title_block)})`);
      await run("common.Interactive.undo");
      const undone = await until(async () => { const s = await schematic(); return paperOf(s) === paperOf(before) ? s : null; }, 15000);
      t(undone, `one Ctrl+Z puts the sheet back (${paperOf(undone)})`);
      await tabTo("PCB");
    });

    await scenario("Drill/Place File Origin: one click sets it, Esc cancels, Reset puts it back to (0, 0)", async (t) => {
      await run("common.Control.zoomFitObjects");
      await until(() => (__eda.state().view?.scale ?? 0) > 0, 5000);
      await sleep(500);
      const canvas = () => document.querySelector(".pcb-canvas-container canvas");
      const rect = () => document.querySelector(".pcb-canvas-container").getBoundingClientRect();
      const screenOf = (xmm, ymm) => { const v = __eda.state().view; const r = rect(); return [v.x + xmm * 1e6 * v.scale, v.y + ymm * 1e6 * v.scale]; };
      const fire = (type, x, y, extra = {}) => canvas().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect().left + x, clientY: rect().top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
      const origin = async () => (await state()).aux_origin ?? null;
      const started = await origin();
      const r = await run("pcbnew.EditorControl.drillOrigin");
      t(__eda.state().tool === "drill_origin", `Place > Drill/Place File Origin arms the picker (${__eda.state().tool})`);
      t(/Drill\/Place File Origin/.test(document.querySelector(".status-bar")?.innerText ?? ""), "the status bar says what the picker wants");
      const [x, y] = screenOf(5, 5);
      fire("pointermove", x, y);
      await sleep(150);
      fire("pointerdown", x, y, { buttons: 1 });
      await sleep(80);
      fire("pointerup", x, y);
      const set = await until(async () => { const o = await origin(); return o ? o : null; }, 15000);
      if (!t(set, `the click sets the origin (${JSON.stringify(set)})`)) return;
      const grid = __eda.state().grid ?? 0;
      t(Math.abs(set[0] - 5e6) <= grid && Math.abs(set[1] - 5e6) <= grid, `it lands on the snapped point near (5, 5) mm (${(set[0] / 1e6).toFixed(4)}, ${(set[1] / 1e6).toFixed(4)} mm; grid ${(grid / 1e6).toFixed(4)} mm)`);
      t(__eda.state().tool === "select", `and the one-shot picker ends (${__eda.state().tool})`);
      // Esc while armed: nothing moves.
      await run("pcbnew.EditorControl.drillOrigin");
      t(__eda.state().tool === "drill_origin", "armed again");
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true }));
      const cancelled = await until(() => __eda.state().tool === "select", 5000);
      t(cancelled, "Esc ends the picker");
      t(JSON.stringify(await origin()) === JSON.stringify(set), "and leaves the origin where it was");
      // The reset is `Cmd::SetAuxOrigin { at: null }`.
      await run("pcbnew.EditorControl.drillResetOrigin");
      const reset = await until(async () => { const o = await origin(); return o === null ? true : null; }, 15000);
      t(reset, `Reset Drill/Place Origin puts it back to (0, 0) (${JSON.stringify(await origin())})`);
      // Leave the board as it was found (the scenario runs on a fresh copy, but the origin is the one thing it changed for good).
      if (started) await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "set_aux_origin", at: { x: started[0], y: started[1] } }, strict: false }) });
    });

    await scenario("no error was logged", async (t) => {
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
