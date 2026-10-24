// Schematic details, part 2 (HANDOFF "Next tasks" 5; GAPS.md items 4, 12 and 20, package P10): a sheet duplicated (or copied and pasted) beside its original shows the same screen, its
// symbols with references of their own and their pins on nets of their own; a bus across sheets joins its members' nets, parted by Delete and joined again by Undo; a label's own
// fields (Label Properties' grid: a net class) and a global label's Sheet References (Schematic Setup > Formatting > Inter-sheet References).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), with the schematic tab
// open -- `node web/studio/e2e/run-check.mjs <url> web/studio/e2e/sch-details-2.check.js` -- on a COPY of a board: every scenario undoes what it did (and, if it left anything behind, the run
// undoes until the sheets are as they were).
//
// It answers through `window.__schDetails2Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }. It works through `window.__eda` (select, Duplicate, undo, open a dialog with the
// action that opens it), the dialogs' controls and the pointer events a user makes on the canvas; the setup edits go straight to `POST /api/cmd`.
// run-all-checks: board=mcu30   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__schDetails2Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schDetails2Sched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 4e6;
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
    window.__schDetails2Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(900); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1800); };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(900); await select([]); await fit(); };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  /** What the scenarios touch: the root's sheets and the screens they show, to compare the schematic with how it was. */
  const snapshot = async () => {
    const root = await sch("");
    const out = { sheets: root.sheets.map((s) => [s.id, s.name, s.file, s.at, s.pins.map((p) => [p.name, p.at])]), labels: root.labels.map((l) => [l.id, l.net, l.at, l.kind]), wires: root.wires.map((w) => [w.id, w.pts, w.bus]) };
    for (const s of root.sheets) {
      const c = await sch(s.id);
      out[s.id] = { symbols: c.symbols.map((x) => [x.id, x.reference, x.pins.map((p) => p.net)]), labels: c.labels.map((l) => [l.id, l.net, l.at, l.kind, l.fields]), wires: c.wires.map((w) => [w.id, w.pts, w.bus]) };
    }
    return JSON.stringify(out);
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 24; i++) {
      if ((await snapshot()) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    if (window.__schDetails2Only && !window.__schDetails2Only.test(name)) return; // `window.__schDetails2Only = /regexp/` runs some of the scenarios only
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  await leave();
  await fit();
  const root0 = await sch("");
  const view0 = __eda.state().view;
  if (!view0 || !Number.isFinite(view0.scale) || view0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)})`] }); check.done = true; return "no canvas size"; }
  if (root0.sheets.length < 2) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the module sheets of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no sheets"; }

  (async () => {
    // ----- (a) a sheet duplicated beside its original shows the same screen
    await scenario("Duplicate a sheet: the copy shows the same screen, its symbols numbered for it, its pins on nets of their own; one undo step", async (t) => {
      const orig = root0.sheets[0];
      const right = Math.max(...root0.sheets.map((s) => s.at[0] + s.size[0]));
      await select([orig.id]);
      await __eda.run("common.Interactive.duplicate");
      await sleep(1500);
      await clickAt(right + 40_000_000, orig.at[1] + orig.size[1] / 2);
      const root1 = await sch("");
      const made = root1.sheets.filter((s) => !root0.sheets.some((o) => o.id === s.id));
      t(made.length === 1, `one new sheet (${made.map((s) => s.name)})`);
      const copy = made[0];
      if (!copy) return;
      t(copy.file === orig.file, `it shows the same file (${copy.file} / ${orig.file})`);
      t(copy.name !== orig.name && copy.name.startsWith(orig.name.replace(/\d+$/, "")), `with a name of its own (${copy.name})`);
      const a = await sch(orig.id);
      const b = await sch(copy.id);
      t(same(a.symbols.map((s) => s.id), b.symbols.map((s) => s.id)), "the same symbols (one screen)");
      const refsA = a.symbols.map((s) => s.reference ?? s.id);
      const refsB = b.symbols.map((s) => s.reference ?? s.id);
      t(refsA.every((r, i) => r === a.symbols[i].id), "the original keeps its references");
      t(refsB.every((r) => !refsA.includes(r)) && new Set(refsB).size === refsB.length, `the copy's symbols have references of their own (${refsB.slice(0, 6)})`);
      const fieldRef = (s) => (s.fields ?? []).find((f) => f.name === "Reference")?.text;
      t(b.symbols.filter((s) => s.fields?.length).every((s) => fieldRef(s) === s.reference), "their Reference fields show them");
      const netsA = new Set(a.symbols.flatMap((s) => s.pins.map((p) => p.net)).filter(Boolean));
      const netsB = b.symbols.flatMap((s) => s.pins.map((p) => p.net)).filter(Boolean);
      t(netsB.length > 0, `the copy's pins are on nets (${netsB.length})`);
      const local = netsB.filter((n) => !netsA.has(n));
      t(local.length > 0, `nets of their own where the screen's labels are local (${[...new Set(local)].slice(0, 4)})`);
      // the copy's view in the editor: entering it shows the copy's references on the canvas's state
      await enterSheet(copy.id);
      t(same(__eda.state().sheetPath.map((x) => x.id ?? x), [copy.id]), "the copy opens in the editor");
      await leave();
      await undo();
      const root2 = await sch("");
      t(root2.sheets.length === root0.sheets.length, "one undo takes the copy away");
      const back = await sch(orig.id);
      t(back.symbols.every((s) => (s.reference ?? s.id) === s.id), "and the screen is shown once again");
    });

    // ----- (b) a bus across sheets
    await scenario("A bus across sheets: a member's nets on the two sheets are one net; Delete of the root's bus parts them, one undo joins them again", async (t) => {
      const root = await sch("");
      const mcu = root.sheets.find((s) => s.name.startsWith("MCU"));
      const led = root.sheets.find((s) => s.name.startsWith("LED"));
      if (!mcu || !led) { t(false, "the MCU and LED sheets of mcu30"); return; }
      const pinAt = (sheet, name) => sheet.pins.find((p) => p.name === name)?.at;
      const pa0 = pinAt(led, "PA0");
      const pa1 = pinAt(led, "PA1");
      t(!!pa0 && !!pa1, "the LED sheet has the pins PA0 and PA1");
      // the pins PA0 and PA1 are on, on each sheet
      // a pin's net by its label: KiCad's name is the label's text with the path of the sheet that names it (`/LED channels (D1–D8)/PA0`)
      const pinsOn = async (sheetId, net) => (await sch(sheetId)).symbols.flatMap((s) => s.pins.filter((p) => (p.net ?? "").split("/").pop() === net).map((p) => `${s.reference ?? s.id}.${p.number}`));
      const ledPA0 = await pinsOn(led.id, "PA0");
      const mcuPA0 = await pinsOn(mcu.id, "PA0");
      t(ledPA0.length > 0 && mcuPA0.length > 0, `PA0 has pins on both sheets (${mcuPA0} / ${ledPA0})`);
      const netOf = async (sheetId, pin) => { const [ref, num] = pin.split("."); const s = (await sch(sheetId)).symbols.find((x) => (x.reference ?? x.id) === ref); return s?.pins.find((p) => p.number === num)?.net ?? null; };
      // take the root's wires off the LED sheet's PA0 and PA1: the two sheets' PA0 nets part
      for (const w of root.wires.filter((w) => w.pts.some((q) => same(q, pa0) || same(q, pa1)))) await post({ op: "delete_wire", id: w.id });
      await sleep(400);
      t((await netOf(led.id, ledPA0[0])) !== (await netOf(mcu.id, mcuPA0[0])), "without the root's wire the two sheets' PA0 are apart");
      // a bus PB[0..1] across the sheets: PB0 and PB1 beside PA0 and PA1 inside each sheet, a bus there with the hierarchical label, a bus pin on each
      // sheet and a bus on the root between the pins
      for (const sh of [mcu, led]) {
        const inside = await sch(sh.id);
        for (const [pa, pb] of [["PA0", "PB0"], ["PA1", "PB1"]]) {
          const hl = inside.labels.find((l) => l.net === pa && l.scope === "hierarchical");
          const r = await post({ op: "add_label", net: pb, at: { x: hl.at[0], y: hl.at[1] }, kind: { scope: "local" } }, sh.id);
          t(r.ok !== false, `${pb} beside ${pa} in ${sh.name}`);
        }
        await post({ op: "add_wire", pts: [{ x: 12_700_000, y: 12_700_000 }, { x: 38_100_000, y: 12_700_000 }], bus: true }, sh.id);
        await post({ op: "add_label", net: "PB[0..1]", at: { x: 12_700_000, y: 12_700_000 }, kind: { scope: "hierarchical", shape: "bidirectional" } }, sh.id);
        await post({ op: "sch_edit", verb: "add_sheet_pin", sheet: sh.id, name: "PB[0..1]", shape: "bidirectional", at: { x: sh.at[0] + 2_540_000, y: sh.at[1] } });
      }
      const top = Math.min(mcu.at[1], led.at[1]) - 5_080_000;
      const [m, l] = [[mcu.at[0] + 2_540_000, mcu.at[1]], [led.at[0] + 2_540_000, led.at[1]]];
      const added = await post({ op: "add_wire", pts: [{ x: m[0], y: m[1] }, { x: m[0], y: top }, { x: l[0], y: top }, { x: l[0], y: l[1] }], bus: true });
      t(added.ok !== false, "the root's bus between the two pins");
      await sleep(400);
      const joined = await netOf(led.id, ledPA0[0]);
      t(joined !== null && joined === (await netOf(mcu.id, mcuPA0[0])), `through the bus the two sheets' PA0 are one net again (${joined})`);
      const r2 = await sch("");
      const busWire = r2.wires.find((w) => w.bus && w.pts.some((q) => same(q, m)));
      t(!!busWire && (busWire.members ?? []).length === 2 && busWire.net.split("/").pop() === "PB[0..1]", `the root's bus is PB[0..1] with its two members (${busWire?.net}: ${busWire?.members})`);
      // the user deletes the root's bus: apart; one undo: one net again
      await select([busWire.id]);
      await __eda.run("common.Interactive.delete");
      await sleep(900);
      t(!(await sch("")).wires.some((w) => w.id === busWire.id), "Delete takes the root's bus away");
      t((await netOf(led.id, ledPA0[0])) !== (await netOf(mcu.id, mcuPA0[0])), "and the two sheets' PA0 are apart");
      await undo();
      t((await netOf(led.id, ledPA0[0])) === (await netOf(mcu.id, mcuPA0[0])), "one undo brings the bus and the join back");
    });

    // ----- (c) a label's own fields: its net class, a global label's Sheet References
    await scenario("Label Properties' fields: Add Field gives a net class, OK keeps it, one undo takes it back", async (t) => {
      const led = (await sch("")).sheets.find((s) => s.name.startsWith("LED"));
      const hl = (await sch(led.id)).labels.find((l) => l.net === "PA0" && l.scope === "hierarchical");
      t(!!hl, "the LED sheet's hierarchical label PA0");
      await enterSheet(led.id);
      await select([hl.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      const dialog = () => document.querySelector(".dialog");
      t(/Hierarchical Label Properties/.test(dialog()?.innerText ?? ""), "Properties opens Hierarchical Label Properties");
      const ctl = (label) => dialog()?.querySelector(`[aria-label="${label}"]`);
      ctl("Add field")?.click();
      await sleep(200);
      t(ctl("Field 1 name")?.value === "Net Class", `Add Field gives a net class first (${ctl("Field 1 name")?.value})`);
      t(ctl("Field 1 italic")?.checked === true && ctl("Field 1 shown")?.checked === true, "shown and in italics, as KiCad's first field");
      setValue(ctl("Field 1 value"), "Signal");
      await sleep(100);
      buttonText(dialog(), "OK")?.click();
      await sleep(1200);
      const after = (await sch(led.id)).labels.find((l) => l.id === hl.id);
      const nc = (after?.fields ?? []).find((f) => f.name === "Netclass");
      t(!!nc && nc.text === "Signal" && nc.visible && nc.italic, `the label has its net class (${JSON.stringify(after?.fields)})`);
      t(!!nc && nc.at[1] !== hl.at[1] || (!!nc && nc.at[0] !== hl.at[0]), "placed off the label's text (`AutoplaceFields`)");
      // the dialog opened again shows the row, and the same OK changes nothing
      await select([hl.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(ctl("Field 1 name")?.value === "Net Class" && ctl("Field 1 value")?.value === "Signal", "Label Properties shows the field again");
      buttonText(dialog(), "Cancel")?.click();
      await sleep(300);
      await undo();
      const back = (await sch(led.id)).labels.find((l) => l.id === hl.id);
      t(!(back?.fields ?? []).some((f) => f.name === "Netclass"), "one undo takes the field away");
    });

    await scenario("Schematic Setup > Formatting > Inter-sheet References: a global label shows the pages its name is on; one undo hides them again", async (t) => {
      const root = await sch("");
      const mcu = root.sheets.find((s) => s.name.startsWith("MCU"));
      const led = root.sheets.find((s) => s.name.startsWith("LED"));
      for (const sh of [mcu, led]) await post({ op: "add_label", net: "GLOBX", at: { x: 12_700_000, y: 25_400_000 }, kind: { scope: "global", shape: "input" } }, sh.id);
      await sleep(400);
      const isrOf = async (id) => ((await sch(id)).labels.find((l) => l.net === "GLOBX")?.fields ?? []).find((f) => f.intersheet_refs);
      const hidden = await isrOf(mcu.id);
      t(!!hidden && hidden.visible === false, "a global label has its Sheet References, hidden by default");
      await __eda.run("eeschema.EditorControl.schematicSetup");
      await sleep(900);
      const setup = [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith("Schematic Setup"));
      t(!!setup, "Schematic Setup opens");
      const show = setup && [...setup.querySelectorAll("label.bs-check")].find((l) => /Show inter-sheet references/.test(l.textContent))?.querySelector("input");
      t(!!show && show.checked === false, "on Formatting, with Show inter-sheet references off");
      show?.click();
      await sleep(200);
      t(setup.querySelector('[aria-label="Inter-sheet references prefix"]')?.disabled === false, "the others are no longer greyed out");
      buttonText(setup, "Apply")?.click();
      await sleep(1500);
      const fmt = await json("/api/sch/formatting");
      t(fmt.intersheet_refs?.show === true, "the design keeps it");
      const pages = (await json("/api/schematic")).sheets;
      const a = await isrOf(mcu.id);
      const b = await isrOf(led.id);
      t(!!a && a.visible && /^\[.+,.+\]$/.test(a.shown) && a.shown === b?.shown, `both labels show the two pages (${a?.shown} / ${b?.shown}; ${pages.map((p) => p.page).join(",")})`);
      buttonText(setup, "Close")?.click() ?? buttonText(document, "Close")?.click();
      await closeDialogs();
      await undo();
      t((await isrOf(mcu.id))?.visible === false, "one undo hides them again");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
