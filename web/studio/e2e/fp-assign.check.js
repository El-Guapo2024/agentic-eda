// Footprint assignment check (GAPS.md package P2 / item 26, PARITY-pcb.md section 29, PARITY-sch.md): a rename carries the board's part; the schematic symbol's Footprint field is followed by
// the board and Update PCB from Schematic (F8) lists and applies what is left (a locked footprint); Change Footprint (this one / the same value), Update Footprint from Library (what
// differs is listed), Compare Footprint with Library (the Summary page, the Visual page's slider, the update it ends in), Geographical Reannotate (the schematic's symbols follow, one undo step)
// and Insert footprint into PCB (a part of its own where no part names the footprint).
//
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it renames,
// changes, updates and renumbers parts and undoes each of them, and it switches the Strict switch off and back on. It makes nothing that stays: it compares the board with how it was.
//
// It needs a board of the examples whose parts are KiCad's: `eda board new examples/mcu_board_30plus.yaml -o <dir>` with the parts placed (R1... on `0603`, a part on `SOIC-16`), a schematic the
// studio can edit (the module sheets it derives), and the installed KiCad footprint libraries (`Resistor_SMD:R_0805_2012Metric`, `R_1206_3216Metric`, `R_0402_1005Metric`). It answers through
// `window.__fpAssignCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __fpAssignCheck.wait(90000)` until `.done` (a run takes about a minute).
//
// It works through `window.__eda` (`actions`' ids with `run`, the dialogs by their data-testid, selection with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`) and
// the verbs' own routes (`/api/cmd` for the edits a dialog of the Schematic tab would send).
// run-all-checks: board=mcu30-placed minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__fpAssignCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const placed = async () => (await state()).parts.filter((p) => p.placed);
  const part = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const summary = async () => JSON.stringify((await placed()).map((p) => [p.ref, p.at, p.rot, p.side, p.footprint]).sort());
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const dialog = (id) => document.querySelector(`[data-testid="${id}"]`);
  const text = (id) => dialog(id)?.innerText ?? "";
  const click = async (id, ms = 700) => { dialog(id).click(); await sleep(ms); };
  const typeInto = async (id, value) => {
    const el = dialog(id);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(150);
  };
  const closeDialogs = async () => {
    for (const id of ["exchange-dialog", "footprint-diff-dialog", "reannotate-dialog", "update-pcb-dialog"]) {
      if (dialog(id)) { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); await sleep(300); }
    }
  };
  /** The sheet of the schematic that has the symbol `ref` (the root screen is `null`). */
  const sheetOf = async (ref) => {
    const root = await json("/api/schematic");
    if ((root.symbols ?? []).some((s) => s.id === ref)) return null;
    for (const sh of root.sheets ?? []) {
      const sub = await json(`/api/schematic?sheet=${encodeURIComponent(sh.id)}`);
      if ((sub.symbols ?? []).some((s) => s.id === ref)) return sh.id;
    }
    throw new Error(`no sheet has ${ref}`);
  };
  const onSheet = async (ref, c) => { const sheet = await sheetOf(ref); return sheet ? { op: "on_sheet", sheet, cmd: c } : c; };
  const symbolIds = async () => {
    const root = await json("/api/schematic");
    const ids = new Set((root.symbols ?? []).map((s) => s.id));
    for (const sh of root.sheets ?? []) for (const s of (await json(`/api/schematic?sheet=${encodeURIComponent(sh.id)}`)).symbols ?? []) ids.add(s.id);
    return ids;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  /** Undo until the board is as it was (the steps a scenario made are all that is on top of the history). */
  const restore = async (original) => {
    for (let i = 0; i < 12 && (await summary()) !== original; i++) await undo();
    return (await summary()) === original;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const wasStrict = !!strict?.checked;
  if (wasStrict) strict.click();
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(600); }
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const parts = await placed();
  const resistors = parts.filter((p) => /^R\d+$/.test(p.ref)).map((p) => p.ref).sort((a, b) => parseInt(a.slice(1)) - parseInt(b.slice(1)));
  const original = await summary();
  const symbolsBefore = [...(await symbolIds())].sort().join();
  const entriesBefore = (await json("/api/footprint_library")).project ?? [];
  const [r1, r2, r3, r4, r5] = resistors;

  await scenario("a rename carries the board's part: its pose, footprint and pads go with it, and one undo takes the schematic's half and the board's back", async (t) => {
    t(resistors.length >= 5, `${resistors.length} resistors on the board`);
    const before = await part(r1);
    const ids = await symbolIds();
    t(ids.has(r1), `${r1} has a symbol`);
    const res = await cmd(await onSheet(r1, { op: "rename_symbol", id: r1, new_id: "R91" }));
    t(res.ok, `Edit Reference ${r1} -> R91: ${res.message}`);
    const after = await part("R91");
    t(!!after && !(await part(r1)), "the part is R91 and nothing is left under " + r1);
    t(!!after && JSON.stringify(after.at) === JSON.stringify(before.at) && after.footprint === before.footprint && after.pads.length === before.pads.length, "with its pose, its footprint and its pads");
    const names = new Set((await state()).parts.map((p) => p.ref));
    t([...(await state()).routing?.tracks ?? []].every((tr) => (tr.pins ?? []).every((pin) => names.has(pin.split(".")[0]))), "no route ends on a pad of a part that is not there");
    const ids2 = await symbolIds();
    t(ids2.has("R91") && !ids2.has(r1), "the schematic has the symbol under the new reference");
    await undo();
    t((await summary()) === original && (await symbolIds()).has(r1), "one Ctrl+Z on the PCB tab takes both back");
  });

  await scenario("the symbol's Footprint field is the board's footprint, and F8 lists the change a locked footprint refuses", async (t) => {
    const lib = "Resistor_SMD:R_0805_2012Metric";
    const wide = "Resistor_SMD:R_1206_3216Metric";
    let res = await cmd(await onSheet(r2, { op: "edit_symbol_fields", id: r2, footprint: lib }));
    t(res.ok, `the Footprint field of ${r2} is ${lib}: ${res.message}`);
    const p2 = await part(r2);
    t(p2.footprint === lib && p2.pads.length === 2, `the board has it: ${p2.footprint}`);
    const f8 = await json("/api/update_pcb");
    t(f8.ok && f8.changes.length === 0, "Update PCB from Schematic has nothing to bring");
    res = await cmd({ op: "set_locked", ids: [r3], locked: true });
    t(res.ok, `${r3} is locked`);
    res = await cmd(await onSheet(r3, { op: "edit_symbol_fields", id: r3, footprint: wide }));
    t(res.ok, `the Footprint field of the locked ${r3} is ${wide}`);
    t((await part(r3)).footprint === "0603", `the board keeps the locked footprint it has`);
    const locked = (await json("/api/update_pcb")).changes.find((c) => c.reference === r3);
    t(!!locked && locked.locked === true && locked.found === true, `F8 lists it as locked: ${JSON.stringify(locked)}`);
    await __eda.run("common.Control.updatePcbFromSchematic");
    await sleep(600);
    t(!!dialog("update-pcb-dialog") && text("update-pcb-messages").includes(r3), "the F8 dialog lists it");
    t(dialog("update-pcb-apply")?.disabled === true, "and has nothing it can apply");
    await closeDialogs();
    await cmd({ op: "set_locked", ids: [r3], locked: false });
    await __eda.run("common.Control.updatePcbFromSchematic");
    await sleep(600);
    t(dialog("update-pcb-apply") && !dialog("update-pcb-apply").disabled, "unlocked, the dialog can apply it");
    await click("update-pcb-apply", 1500);
    t((await part(r3)).footprint === wide, `Update PCB puts ${r3} on ${wide}`);
    t((await json("/api/update_pcb")).changes.length === 0, "and F8 has nothing left");
    await closeDialogs();
    t(await restore(original), "the undos bring the board back");
  });

  await scenario("Change Footprint: this footprint, then the footprints of the same value", async (t) => {
    const wide = "Resistor_SMD:R_1206_3216Metric";
    await select([r4]);
    await __eda.run("pcbnew.GlobalEdit.changeFootprint");
    await sleep(500);
    t(!!dialog("exchange-dialog"), "the dialog opens");
    t(dialog("exchange-match-selected")?.checked === true, "on the selected footprint");
    await typeInto("exchange-new-id", wide);
    const was = await part(r4);
    await click("exchange-run", 2000);
    const msg = text("exchange-messages");
    t(msg.includes(`Changed footprint ${r4} from '0603' to '${wide}': OK`), `the message window: ${msg.split("\n")[0]}`);
    const now = await part(r4);
    t(now.footprint === wide && JSON.stringify(now.at) === JSON.stringify(was.at) && now.rot === was.rot, "R4 is on it, where it was");
    t(__eda.state().selection.some((s) => s.id === r4), "and still selected");
    await closeDialogs();
    // the footprints of the same value: the dialog's second choice
    const value = (await part(r5)).value;
    const same = (await placed()).filter((p) => p.value === value).map((p) => p.ref);
    await select([r5]);
    await __eda.run("pcbnew.GlobalEdit.changeFootprints");
    await sleep(500);
    await click("exchange-match-value", 200);
    await typeInto("exchange-new-id", "Resistor_SMD:R_0402_1005Metric");
    await click("exchange-run", 2500);
    const changed = (await placed()).filter((p) => p.footprint === "Resistor_SMD:R_0402_1005Metric").map((p) => p.ref);
    t(same.length > 0 && same.every((r) => changed.includes(r)) && changed.length === same.length, `the ${same.length} parts of value ${value} are on it: ${changed.join(" ")}`);
    await closeDialogs();
    t(await restore(original), "two undos bring the board back (one step each)");
  });

  await scenario("Update Footprint from Library lists what differs and puts the library's copy back", async (t) => {
    const lib = "Resistor_SMD:R_0805_2012Metric";
    await cmd({ op: "exchange_footprints", references: [r4], footprint: lib });
    const pad = (await part(r4)).pads[0];
    const res = await cmd({ op: "edit_board_pad", part: r4, edit: { number: pad.num ?? "1", size: [pad.w + 200_000, pad.h] } });
    t(res.ok, `pad 1 of ${r4} is 0.2 mm wider than the library's: ${res.message}`);
    const plan = await (await fetch("/api/exchange_plan", { method: "POST", body: JSON.stringify({ references: [r4, r5], footprint: null }) })).json();
    const e4 = plan.entries.find((e) => e.reference === r4);
    t(e4.found && e4.differs && e4.lines.some((l) => /Pad 1 size differs/.test(l)), `the plan lists it: ${JSON.stringify(e4.lines)}`);
    const e5 = plan.entries.find((e) => e.reference === r5);
    t(e5.found === true && e5.from === e5.to && e5.from.includes(":"), `${r5}'s built-in package is the installed footprint the board names, whose library copy an update takes: ${e5.from}`);
    await select([r4]);
    await __eda.run("pcbnew.GlobalEdit.updateFootprint");
    await sleep(500);
    t(dialog("exchange-match-selected")?.checked === true, "the update dialog opens on it");
    await click("exchange-run", 2000);
    const msg = text("exchange-messages");
    t(msg.includes("OK") && /Pad 1 size differs/.test(msg), `the window lists what it changed: ${msg.replace(/\n/g, " | ").slice(0, 160)}`);
    const diff = await json(`/api/footprint_diff?ref=${r4}`);
    t(diff.ok && diff.found && diff.lines.length === 0, "the footprint is the library's again");
    await click("exchange-run", 1500);
    t(text("exchange-messages").includes("(no changes)"), "a second update says (no changes)");
    await closeDialogs();
    t(await restore(original), "the undos bring the board back");
  });

  await scenario("Compare Footprint with Library: the summary, the visual page and the update it ends in", async (t) => {
    const lib = "Resistor_SMD:R_0805_2012Metric";
    await cmd({ op: "exchange_footprints", references: [r4], footprint: lib });
    const pad = (await part(r4)).pads[0];
    await cmd({ op: "edit_board_pad", part: r4, edit: { number: pad.num ?? "1", size: [pad.w + 200_000, pad.h] } });
    await select([r4, r5]);
    let run = await __eda.run("pcbnew.InspectionTool.DiffFootprint");
    t(!dialog("footprint-diff-dialog") && /Select a footprint to diff/.test(run.toast ?? ""), "two footprints selected: the message KiCad gives");
    await select([r4]);
    run = await __eda.run("pcbnew.InspectionTool.DiffFootprint");
    await sleep(800);
    const summary_ = text("footprint-diff-summary");
    t(summary_.includes(`Footprint ${r4}`) && summary_.includes("Library: Resistor_SMD") && summary_.includes("Library item: R_0805_2012Metric") && /Pad 1 size differs/.test(summary_), "the summary names the footprint, its library and item, and what differs");
    await click("footprint-diff-tab-visual", 400);
    const svg = dialog("footprint-diff-visual");
    const op = () => [dialog("footprint-diff-board").getAttribute("data-opacity"), dialog("footprint-diff-library").getAttribute("data-opacity")].join(",");
    t(!!svg && svg.querySelectorAll("path").length > 0 && svg.querySelectorAll("rect").length >= 2, "the visual page draws both footprints");
    t(op() === "1,1", "the slider starts in the middle: both whole");
    await click("footprint-diff-toggle", 200);
    t(op() === "1,0", `Swap shows the board's: ${op()}`);
    await click("footprint-diff-toggle", 200);
    t(op() === "0,1", `and again the library's: ${op()}`);
    await click("footprint-diff-update", 700);
    t(!dialog("footprint-diff-dialog") && !!dialog("exchange-dialog") && dialog("exchange-match-selected")?.checked === true, "Update Footprint from Library... opens the update dialog on the footprint");
    await closeDialogs();
    // a built-in package has no library
    await select([r5]);
    await __eda.run("pcbnew.InspectionTool.DiffFootprint");
    await sleep(600);
    t(text("footprint-diff-summary").includes("The library is not included in the current configuration."), "a built-in package says the library is not in the configuration");
    t(!dialog("footprint-diff-tab-visual"), "and has no visual page");
    await closeDialogs();
    t(await restore(original), "the undos bring the board back");
  });

  await scenario("Geographical Reannotate: renumbered by place, the symbols follow, one undo step", async (t) => {
    await select([]);
    const before = await placed();
    const origin = (p) => `${p.at[0]},${p.at[1]}`;
    await __eda.run("pcbnew.ReannotateTool.ShowReannotateDialog");
    await sleep(500);
    t(!!dialog("reannotate-dialog") && dialog("reannotate-scope-all")?.checked === true && dialog("reannotate-front-start")?.value === "1", "the dialog opens on the whole board, front start 1");
    await click("reannotate-run", 2500);
    const msg = text("reannotate-messages");
    t(/Front Footprints \(sorted by footprint location\)/.test(msg) && /Reference Designator Prefixes in Use/.test(msg) && /Change Log/.test(msg), "the window lists the footprints, the prefixes and the change log");
    t(msg.includes("PCB successfully reannotated"), "and says it was done");
    const after = await placed();
    t(after.length === before.length, `${after.length} footprints, none lost`);
    // by place: for each prefix the numbers run in the order of top to bottom, left to right
    const byPrefix = {};
    for (const p of after) (byPrefix[p.ref.replace(/\d+$/, "")] ??= []).push(p);
    // `RoundToGrid` of the dialog: the remainder takes the sign of the coordinate, exactly half goes down
    const rtg = (v) => { const r = v % 1_000_000; let c = v - r; if (Math.abs(r) > 500_000) c += c < 0 ? -1_000_000 : 1_000_000; return c; };
    const ordered = Object.values(byPrefix).every((list) => {
      const rows = [...list].sort((a, b) => parseInt(a.ref.replace(/\D+/g, "")) - parseInt(b.ref.replace(/\D+/g, "")));
      return rows.every((p, i) => i === 0 || rtg(rows[i - 1].at[1]) < rtg(p.at[1]) || (rtg(rows[i - 1].at[1]) === rtg(p.at[1]) && rtg(rows[i - 1].at[0]) <= rtg(p.at[0])));
    });
    t(ordered, "the numbers of each prefix go by place (rows of a 1 mm grid, left to right)");
    const places = new Set(before.map(origin));
    t(after.every((p) => places.has(origin(p))), "every footprint is at a place a footprint was");
    const ids = await symbolIds();
    t(after.every((p) => ids.has(p.ref)), "every footprint has its symbol, under the same reference");
    const names = new Set(after.map((p) => p.ref));
    t([...(await state()).routing?.tracks ?? []].every((tr) => (tr.pins ?? []).every((pin) => names.has(pin.split(".")[0]))), "no route ends on a pad of a part that is not there");
    await closeDialogs();
    await undo();
    t((await summary()) === original && (await symbolIds()).size === ids.size, "one Ctrl+Z takes the footprints and the symbols back");
    // the selection scope names the selected footprints only
    await select([r2]);
    await __eda.run("pcbnew.ReannotateTool.ShowReannotateDialog");
    await sleep(500);
    t(dialog("reannotate-scope-selection")?.checked === true, "with a footprint selected the scope is the selection");
    await closeDialogs();
    await select([]);
  });

  await scenario("Insert footprint into PCB adds a part of its own where no part names the footprint", async (t) => {
    const name = "Resistor_SMD:R_0603_1608Metric";
    t(!(await placed()).some((p) => p.footprint === name), "no part names the footprint");
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Footprint")?.click();
    await sleep(900);
    const findRow = () => [...document.querySelectorAll("*")].filter((e) => e.children.length === 0 && e.textContent.trim() === "R_0603_1608Metric").pop();
    if (!findRow()) {
      // the library's node, to open it
      const lib = [...document.querySelectorAll("*")].filter((e) => e.children.length === 0 && /^Resistor_SMD/.test(e.textContent.trim()) && !/R_/.test(e.textContent)).pop();
      lib?.click();
      await sleep(600);
    }
    const row = findRow();
    t(!!row, "the footprint is in the library tree");
    if (row) {
      const target = row.closest("[class*=row], li, div") ?? row;
      target.scrollIntoView({ block: "center" });
      for (const type of ["mousedown", "mouseup", "click", "mousedown", "mouseup", "click", "dblclick"]) target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, detail: type === "dblclick" ? 2 : 1 }));
      await sleep(1500);
    }
    const had = new Set((await state()).parts.map((p) => p.ref));
    const run = await __eda.run("pcbnew.ModuleEditor.saveFootprintToBoard");
    await sleep(2000);
    const added = (await state()).parts.filter((p) => !had.has(p.ref));
    t(added.length === 1 && added[0].footprint === name && added[0].placed, `a part of its own: ${added.map((p) => p.ref + " on " + p.footprint).join(", ")}`);
    t(added[0] && added[0].pads.length === 2, "with the footprint's pads");
    t(__eda.state().tab === "pcb" && __eda.state().selection.some((s) => s.id === added[0]?.ref), "the board is shown with it selected");
    t(/inserted into the board as/.test(run.toast ?? ""), `the toast: ${run.toast}`);
    await undo();
    t(!(await state()).parts.some((p) => p.ref === added[0]?.ref), "one Ctrl+Z takes the part away");
  });

  await scenario("the board is as it was and nothing broke", async (t) => {
    await select([]);
    t((await summary()) === original, "every footprint is where it was, on the footprint it had, turned and on the side it was");
    t([...(await symbolIds())].sort().join() === symbolsBefore, "the schematic has the symbols it had");
    // the Footprint Editor's own entry the Insert scenario opened goes away again
    for (const n of (await json("/api/footprint_library")).project ?? []) if (!entriesBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
    t(JSON.stringify((await json("/api/footprint_library")).project ?? []) === JSON.stringify(entriesBefore), "and the project's footprint library is as it was");
    // the one error toast a scenario asks for: Compare Footprint with two footprints selected
    const errors = (__eda.errors?.() ?? []).slice(errorsFrom).filter((e) => !/^Select a footprint to diff with its library equivalent\.$/.test(e.message));
    t(errors.length === 0, `no console error: ${JSON.stringify(errors).slice(0, 200)}`);
    if (wasStrict && !strict.checked) strict.click();
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click();
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
