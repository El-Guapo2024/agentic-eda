// UI-LEFT-3 backannot lane (task-v17-backannot; PARITY-pcb "backannot"), driven in the studio as a user drives it, each change read back from /api/state and undone:
//   - P3-30: a no-connect pin's pad is on the `unconnected-(..)` net KiCad's board gives it (`updateComponentPadConnections`), in the state's pads and nets;
//   - P3-23: Swap Pad Nets on the pads of a locked footprint is refused with KiCad's info-bar words (`ValidateNetSwapLocks`), and goes with Override locks checked;
//   - P3-14: Footprint Properties shows "Jumper groups:" ("none" in italics), and its OK carries a new user field to the symbol (added) and a deleted one away
//     (deleted), as Update Schematic from PCB's "Other fields" would;
//   - P3-29: Escape while Import Netlist's new footprint rides the cursor leaves it where it was spread, the import standing; one Undo takes the import back.
// It answers through `window.__backannotV17Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__backannotV17Check = { results, done: false, ok: false, wait: null });
  const json = (url, body) => fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const until = async (fn, ms = 12000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const state = () => json("/api/state");
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const padNets = async (ref) => Object.fromEntries(((await partOf(ref))?.pads ?? []).map((p) => [p.num, p.net]));
  const cmd = (c) => json("/api/cmd", { cmd: c, strict: false });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const overrideBox = () => [...document.querySelectorAll('[data-control="overrideLocks"] input')].find((b) => b.offsetParent !== null);
  const strictBox = () => [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const setStrict = async (on) => { const b = strictBox(); if (b && b.checked !== on) { b.click(); await sleep(400); } };
  const setOverride = async (on) => { const b = overrideBox(); if (b && b.checked !== on) { b.click(); await sleep(300); } return !!b; };
  const errorsFrom = Date.now();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(1200); }
  const start = JSON.stringify((await state()).parts.map((p) => [p.ref, p.at, p.placed, p.pads?.map((q) => q.net)]));

  await scenario("P3-30: a no-connect pin's pad is on its unconnected-(..) net, as KiCad's board has it", async (t) => {
    const s = await state();
    const u1 = s.parts.find((p) => p.ref === "U1");
    const nets = Object.fromEntries((u1?.pads ?? []).map((p) => [p.num, p.net]));
    t(nets["12"] === "unconnected-(U1-NC1-Pad12)" && nets["13"] === "unconnected-(U1-NC2-Pad13)", `U1.12 ${nets["12"]}, U1.13 ${nets["13"]}`);
    t(s.nets.includes("unconnected-(U1-NC1-Pad12)"), "and the board's nets have it (Search's Nets)");
  });

  await scenario("P3-23: Swap Pad Nets on a locked footprint is refused with KiCad's words; Override locks lets it through", async (t) => {
    const before = await padNets("R1");
    t(before["1"] && before["2"] && before["1"] !== before["2"], `R1's two pads are on two nets: ${JSON.stringify(before)}`);
    const locked = await cmd({ op: "set_locked", ids: ["R1"], locked: true });
    t(locked.ok, "R1 locked");
    await E.refresh?.();
    await sleep(500);
    await setOverride(false);
    await select(["R1.1", "R1.2"]);
    const run = await E.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(600);
    const said = run.toast?.message ?? E.errors(errorsFrom).map((e) => e.message ?? String(e)).find((m) => /Cannot swap nets/.test(m));
    t(/Cannot swap nets of locked pads or connected copper unless Override Locks is enabled\./.test(said ?? ""), `refused: ${said} (${JSON.stringify(run)})`);
    t(JSON.stringify(await padNets("R1")) === JSON.stringify(before), "nothing changed");
    t(await setOverride(true), "Override locks checked");
    await select(["R1.1", "R1.2"]);
    const second = await E.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(600);
    if (tid("swap-nets-prompt")) tid("swap-nets-ignore").click();
    t(!second.error, `the action ran: ${JSON.stringify(second)} (selection ${JSON.stringify(E.state().selection)})`);
    // (a board with no stored schematic draws the swap at once, so the nets may take other names: the pads have changed nets, each the other's)
    const after = await until(async () => { const n = await padNets("R1"); return n["1"] !== before["1"] && n["2"] !== before["2"] ? n : null; });
    t(!!after && after["1"] !== after["2"], `swapped with the box checked: ${JSON.stringify(after)} (toast ${JSON.stringify(second.toast)}, box ${overrideBox()?.checked}, prompt ${!!tid("swap-nets-prompt")})`);
    await setOverride(false);
    await undo();
    await undo();
    t(JSON.stringify(await padNets("R1")) === JSON.stringify(before) && !((await state()).locked ?? []).includes("R1"), "two undos: the swap and the lock");
  });

  await scenario("P3-14: Footprint Properties shows the jumper groups and carries a field added and a field deleted to the symbol (only what it changed)", async (t) => {
    const open = async () => {
      await select(["R1"]);
      await E.run("pcbnew.InteractiveEdit.properties");
      return until(() => tid("footprint-properties"));
    };
    const ok = async (root) => { root.parentElement.querySelector('[data-testid="footprint-properties-ok"]').click(); await until(() => !tid("footprint-properties")); await sleep(900); };
    let root = await open();
    t(!!root, "the dialog opens on R1");
    const jumpers = tid("fp-jumper-groups");
    t(jumpers?.textContent === "none" && jumpers?.style.fontStyle === "italic", `Jumper groups: ${jumpers?.textContent} (${jumpers?.style.fontStyle})`);
    root.querySelector('[data-testid="add-field"]').click();
    await sleep(200);
    const n = root.querySelectorAll('[data-testid^="field-row-"]').length - 1;
    const inputs = root.querySelectorAll(`[data-testid="field-row-${n}"] input`);
    setValue(inputs[0], "V17Field");
    setValue(inputs[1], "carried");
    await sleep(100);
    await ok(root);
    const added = await until(async () => (await partOf("R1"))?.symbol_fields?.V17Field);
    t(added === "carried", `the symbol has the field: ${added}`);
    root = await open();
    const rows = [...root.querySelectorAll('[data-testid^="field-row-"]')];
    const row = rows.find((r) => [...r.querySelectorAll("input")].some((i) => i.value === "V17Field"));
    t(!!row, "the field is a row of the footprint");
    row?.click();
    await sleep(150);
    tid("delete-field")?.click();
    await sleep(150);
    await ok(root);
    const gone = await until(async () => { const p = await partOf("R1"); return p && !("V17Field" in (p.symbol_fields ?? {})) ? p : null; });
    t(!!gone, `deleted from the symbol too: ${JSON.stringify(gone?.symbol_fields)}`);
    await undo();
    await undo();
    t(!("V17Field" in ((await partOf("R1"))?.symbol_fields ?? {})), "two undos: as it was");
  });

  await scenario("P3-29: Escape while Import Netlist's footprint is carried leaves it spread and the import standing; one Undo takes the import back", async (t) => {
    const board = await state();
    const libId = { U1: "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm", R2: "Resistor_SMD:R_0603_1608Metric" };
    const nets = new Map();
    const comps = [];
    for (const ref of ["U1", "R2"]) {
      const p = board.parts.find((q) => q.ref === ref);
      comps.push(`(comp (ref "${ref}") (value "${ref === "R2" ? "470" : p.value}") (footprint "${p.lib_link ?? libId[ref]}"))`);
      for (const pad of p.pads) if (pad.net && !/^unconnected-/.test(pad.net)) nets.set(pad.net, [...(nets.get(pad.net) ?? []), [ref, pad.num]]);
    }
    comps.push(`(comp (ref "TP99") (value "TP") (footprint "TestPoint:TestPoint_Pad_D1.0mm"))`);
    nets.set("GND", [...(nets.get("GND") ?? []), ["TP99", "1"]]);
    const text = `(export (version "E") (components ${comps.join(" ")}) (nets ${[...nets].map(([n, ns], i) => `(net (code "${i + 1}") (name "${n}") ${ns.map(([r, p]) => `(node (ref "${r}") (pin "${p}"))`).join(" ")})`).join(" ")}))`;
    const r2Before = board.parts.find((q) => q.ref === "R2").value;
    // (the studio's Strict switch would judge the spread footprints off the board: off for this, as the import-netlist check has it)
    const strictWas = !!strictBox()?.checked;
    await setStrict(false);
    await E.run("pcbnew.EditorControl.importNetlist", { name: "v17.net", text });
    await until(() => tid("import-netlist-dialog"));
    tid("import-netlist-link-1").click();
    await sleep(800);
    tid("import-netlist-update").click();
    t(await until(() => tid("import-netlist-label")?.textContent === "Changes Applied to PCB"), "Update PCB applied");
    tid("import-netlist-close").click();
    t(await until(() => E.state().tool === "move" && (E.state().moving?.refs ?? []).includes("TP99")), `the Move tool carries TP99: ${JSON.stringify(E.state().moving)}`);
    await E.run("common.Interactive.cancel");
    t(await until(() => E.state().tool !== "move"), "Escape ends the move");
    const tp = await until(async () => { const p = await partOf("TP99"); return p?.placed ? p : null; }, 8000);
    t(!!tp, `TP99 stays on the board where it was spread: ${JSON.stringify(tp?.at)}`);
    t((await partOf("R2"))?.value === "470", "the import stands (R2 is 470)");
    await undo();
    t((await partOf("R2"))?.value === r2Before && !(await partOf("TP99"))?.placed, "one Undo takes the whole import back");
    await setStrict(strictWas);
  });

  await scenario("the board is as it was and nothing logged an error", async (t) => {
    const now = JSON.stringify((await state()).parts.map((p) => [p.ref, p.at, p.placed, p.pads?.map((q) => q.net)]));
    t(now === start, "every part where and on what it was");
    const errs = E.errors(errorsFrom).map((e) => e.message ?? String(e)).filter((m) => !/Cannot swap nets of locked pads/.test(m));
    t(errs.length === 0, `errors: ${JSON.stringify(errs)}`);
  });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
