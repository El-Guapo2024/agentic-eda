// Import Netlist check (UI-LEFT A1, PARITY-pcb "Import Netlist"): File > Import > Netlist... (`pcbnew.EditorControl.importNetlist`, DIALOG_IMPORT_NETLIST) driven
// through `window.__eda` and the dialog's own controls, on a served COPY of a board with a few footprints placed (it edits and undoes).
//
// What it checks:
//   the action is offered on the PCB tab and opens KiCad's dialog with KiCad's choices (tstamps; replace footprints and group footprints on);
//   Load and Test Netlist lists what Update PCB would do in KiCad's words (a value, a footprint added) and changes nothing; a change of the Link Method or
//     of an option tests again ("Delete footprints with no components in netlist" adds "Remove unused footprint R1.");
//   Update PCB applies it as one step ("Changes Applied to PCB", the same lines in the past tense), Close hands the added footprint to the Move tool, the
//     drop puts it there, and one Undo takes the whole import back (Redo brings it again);
//   under the Strict switch, a drop the gates refuse (off the board) takes the import back with the reason;
//   nothing logged an error that was not asked for.
//
// It answers through `window.__importNetlistCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-few minutes=8
(async () => {
  const results = [];
  const check = (window.__importNetlistCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const until = async (fn, ms = 15000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const messages = () => [...(tid("import-netlist-messages")?.querySelectorAll("div") ?? [])].map((d) => d.textContent);
  const state = () => json("/api/state");
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(200); const a = readout();
    fire("pointermove", 500, 400); await sleep(200); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  let toScreen = null;
  const click = async (xmm, ymm, wait = 1200) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(300); fire("pointermove", x + 1, y); await sleep(200); fire("pointerdown", x + 1, y, { buttons: 1 }); await sleep(60); fire("pointerup", x + 1, y); await sleep(wait); };
  const strictBox = () => [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const setStrict = async (on) => { const b = strictBox(); if (b && b.checked !== on) { b.click(); await sleep(400); } };

  const errorsFrom = Date.now();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  toScreen = await screenOf();
  const strictWas = !!strictBox()?.checked;

  // A netlist of U1 and R2 as the board has them (by reference), R2 at another value, and a test point the board has not got. R1 is left out.
  const board = await state();
  const libId = { U1: "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm", R2: "Resistor_SMD:R_0603_1608Metric" };
  const nets = new Map();
  const comps = [];
  for (const ref of ["U1", "R2"]) {
    const p = board.parts.find((q) => q.ref === ref);
    comps.push(`(comp (ref "${ref}") (value "${ref === "R2" ? "470" : p.value}") (footprint "${libId[ref]}"))`);
    for (const pad of p.pads) if (pad.net) nets.set(pad.net, [...(nets.get(pad.net) ?? []), [ref, pad.num]]);
  }
  comps.push(`(comp (ref "TP99") (value "TP") (footprint "TestPoint:TestPoint_Pad_D1.0mm"))`);
  nets.set("GND", [...(nets.get("GND") ?? []), ["TP99", "1"]]);
  const text = `(export (version "E") (components ${comps.join(" ")}) (nets ${[...nets].map(([n, ns], i) => `(net (code "${i + 1}") (name "${n}") ${ns.map(([r, p]) => `(node (ref "${r}") (pin "${p}"))`).join(" ")})`).join(" ")}))`;
  const r2Before = board.parts.find((q) => q.ref === "R2").value;

  await scenario("Import Netlist... opens DIALOG_IMPORT_NETLIST with KiCad's choices", async (t) => {
    const a = E.actions().find((x) => x.id === "pcbnew.EditorControl.importNetlist");
    t(a?.enabled === true, `offered on the PCB tab: ${JSON.stringify(a)}`);
    await E.run("pcbnew.EditorControl.importNetlist", { name: "check.net", text });
    const dlg = await until(() => tid("import-netlist-dialog"));
    t(!!dlg && /Import Netlist/.test(dlg.textContent), "the dialog is open");
    t(tid("import-netlist-file")?.value === "check.net", `the file: ${tid("import-netlist-file")?.value}`);
    t(tid("import-netlist-link-0")?.checked === true, "Link footprints using component tstamps (unique ids) is chosen");
    t(tid("import-netlist-replace_footprints")?.checked === true && tid("import-netlist-transfer_groups")?.checked === true, "Replace footprints and Group footprints are on");
    t(tid("import-netlist-delete_extra_footprints")?.checked === false && tid("import-netlist-override_locks")?.checked === false, "Delete footprints and override locks are off");
    const buttons = [...dlg.querySelectorAll(".dialog-footer button")].map((b) => b.textContent.trim());
    t(JSON.stringify(buttons) === JSON.stringify(["Load and Test Netlist", "Update PCB", "Close"]), `the buttons: ${buttons}`);
    t(tid("import-netlist-label")?.textContent === "Changes to Be Applied", "Changes to Be Applied");
  });

  await scenario("Load and Test Netlist tells the changes in KiCad's words and changes nothing; the Link Method tests again", async (t) => {
    const before = await E.state().revision;
    tid("import-netlist-test").click();
    t(await until(() => messages().some((m) => /^Processing symbol 'U1:/.test(m))), `tested: ${messages().slice(0, 4)}`);
    t(messages().includes("Using tstamps (unique IDs) to match symbols and footprints."), "matched by tstamps");
    tid("import-netlist-link-1").click();
    t(await until(() => messages().includes("Using reference designators to match symbols and footprints.")), "the Link Method tests again, by reference");
    await until(() => messages().includes(`Change R2 value from ${r2Before} to 470.`));
    t(messages().includes(`Change R2 value from ${r2Before} to 470.`), `R2's value: ${messages().filter((m) => /R2/.test(m))}`);
    t(messages().includes("Add TP99 (footprint 'TestPoint:TestPoint_Pad_D1.0mm')."), `TP99 is added: ${messages().filter((m) => /TP99/.test(m))}`);
    t(/^Total warnings: \d+, errors: 0\.$/.test(messages().at(-1) ?? ""), `the totals: ${messages().at(-1)}`);
    t((await partOf("R2")).value === r2Before && !(await partOf("TP99")), "the board is as it was");
    t(E.state().revision === before, "no step was made");
  });

  await scenario("Delete footprints with no components in netlist: the dry run removes R1, and stops when it is off again", async (t) => {
    tid("import-netlist-delete_extra_footprints").click();
    t(await until(() => messages().includes("Remove unused footprint R1.")), `R1: ${messages().filter((m) => /R1/.test(m))}`);
    tid("import-netlist-delete_extra_footprints").click();
    t(await until(() => !messages().includes("Remove unused footprint R1.") && messages().some((m) => /^Total/.test(m))), "off again: R1 stays");
    tid("import-netlist-delete_extra_footprints").click();
    await until(() => messages().includes("Remove unused footprint R1."));
  });

  await scenario("Update PCB applies it; Close hands TP99 to the Move tool; the drop places it; one Undo takes the whole import back", async (t) => {
    await setStrict(false);
    tid("import-netlist-update").click();
    t(await until(() => tid("import-netlist-label")?.textContent === "Changes Applied to PCB" && messages().some((m) => /^Total/.test(m))), "Changes Applied to PCB");
    t(messages().includes(`Changed R2 value from ${r2Before} to 470.`), `R2: ${messages().filter((m) => /R2/.test(m))}`);
    t(messages().includes("Removed unused footprint R1.") && messages().includes("Added TP99 (footprint 'TestPoint:TestPoint_Pad_D1.0mm')."), "R1 removed, TP99 added");
    t((await partOf("R2")).value === "470", "R2 is 470 on the board");
    t((await partOf("R1"))?.placed === false, "R1 is off the board");
    const tp = await partOf("TP99");
    t(tp?.placed === true && tp.pads.some((p) => p.net === "GND"), `TP99 is on the board, its pad on GND: ${JSON.stringify(tp?.pads?.map((p) => p.net))}`);
    tid("import-netlist-close").click();
    t(await until(() => !tid("import-netlist-dialog")), "Close closes the dialog");
    t(await until(() => E.state().tool === "move" && (E.state().moving?.refs ?? []).includes("TP99")), `the Move tool carries TP99: ${JSON.stringify(E.state().moving)}`);
    await click(24, 6);
    t(await until(async () => E.state().tool !== "move"), "the drop ends the move");
    const placed = await partOf("TP99");
    t(placed?.placed && Math.abs(placed.at[0] / 1e6 - 24) < 1.5 && Math.abs(placed.at[1] / 1e6 - 6) < 1.5, `TP99 dropped near (24, 6): ${placed?.at}`);
    await E.run("common.Interactive.undo");
    await sleep(900);
    t((await partOf("R2")).value === r2Before && (await partOf("R1"))?.placed === true && !(await partOf("TP99"))?.placed, "one Undo: R2 as it was, R1 back, no TP99");
    await E.run("common.Interactive.redo");
    await sleep(900);
    t((await partOf("R2")).value === "470" && (await partOf("TP99"))?.placed === true, "Redo brings the import and the drop again");
    await E.run("common.Interactive.undo");
    await sleep(900);
  });

  await scenario("under Strict, a drop the gates refuse takes the import back, with the reason", async (t) => {
    await setStrict(true);
    await E.run("pcbnew.EditorControl.importNetlist", { name: "check.net", text });
    await until(() => tid("import-netlist-dialog"));
    tid("import-netlist-link-1").click();
    await until(() => messages().some((m) => /^Total/.test(m)));
    tid("import-netlist-update").click();
    t(await until(() => tid("import-netlist-label")?.textContent === "Changes Applied to PCB" && messages().some((m) => /^Total/.test(m))), "applied");
    tid("import-netlist-close").click();
    await until(() => E.state().tool === "move");
    // far off the board: a placement the gates refuse
    await click(-60, -60, 2000);
    const refused = await until(async () => (await partOf("R2")).value === r2Before && !(await partOf("TP99"))?.placed, 15000);
    t(!!refused, `the import is taken back: R2 ${(await partOf("R2")).value}, TP99 ${JSON.stringify((await partOf("TP99"))?.placed)}`);
    const toast = document.querySelector(".toast")?.textContent ?? "";
    const said = E.errors(errorsFrom).map((e) => e.message ?? String(e)).find((m) => /refused because --strict/.test(m)) ?? (/refused because --strict/.test(toast) ? toast : "");
    t(/placement_within_outline @ TP99/.test(said) || /refused because --strict/.test(said), `the reason is said: ${said.slice(0, 200)}`);
  });

  await setStrict(strictWas);
  await scenario("nothing logged an error that was not asked for", async (t) => {
    const errs = E.errors(errorsFrom).map((e) => e.message ?? String(e)).filter((m) => !/refused because --strict/.test(m));
    t(errs.length === 0, `errors: ${JSON.stringify(errs)}`);
  });
  check.ok = results.every((r) => r.ok);
  check.done = true;
  return results;
})();
