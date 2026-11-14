// Setup lane v15 (UI-LEFT-3 B3-4, B3-8, B3-9; task-v15-setup):
//   board finish    Board Finish is its own page after Physical Stackup, with KiCad's choices; Apply stores the finish in the stackup, Physical Stackup has no
//                   finish of its own any more
//   microvia stacks Design Rules > Microvia Stacks after Pre-defined Sizes: Add... asks the name, the Microvia Stack dialog refuses a layer the board lacks with
//                   KiCad's words, OK adds the row; Apply stores `via_stack_presets`
//   profiles        a tuning profile has Frequency (Hz .. GHz) and Model solder mask; a new one is 1 GHz with both boxes ticked; Apply stores them
//   zones           Zones has "Locked", stored with the defaults
//   teardrops       "Round pads only" stored as `round_shapes_only`
//   net classes     a negative track width is taken as typed
//   sch             Schematic Setup > Electrical Rules > Compare Symbol with Library: KiCad's twelve boxes after Violation Severity; Apply stores them; the
//                   Import Settings dialog has "Compare symbol with library settings"
//   end             every undoable step undone
// It answers through `window.__setupV15Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed
(async () => {
  const results = [];
  const run = (window.__setupV15Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(80);
  };
  const button = (root, text) => (root ? [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text) : undefined);
  const press = async (root, text, wait = 200) => { const b = button(root, text); if (!b) throw new Error(`no "${text}" button`); b.click(); await sleep(wait); };
  const q = (root, sel) => { const el = root?.querySelector(sel); if (!el) throw new Error(`no ${sel}`); return el; };
  const dialog = () => document.querySelector(".bs-dialog");
  const openPage = async (label, section = label) => {
    if (!dialog()) { await __eda.run("pcbnew.EditorControl.boardSetup"); await sleep(500); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith(label))?.click();
    await sleep(300);
    const p = document.querySelector(`section.bs-page[aria-label="${section}"]`);
    if (!p) throw new Error(`Board Setup has no "${section}" page`);
    return p;
  };
  const closeBoardSetup = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((b) => ["Close", "Discard and close"].includes(b.textContent.trim()))?.click();
      await sleep(300);
    }
  };
  const apply = async (root) => {
    await press(root, "Apply", 300);
    for (let i = 0; i < 80; i++) {
      const msg = root.querySelector(".bs-msg")?.textContent ?? "";
      if (!button(root, "Apply")?.disabled && msg !== "" && msg !== "Changes not applied yet.") return msg;
      await sleep(100);
    }
    return root.querySelector(".bs-msg")?.textContent ?? "";
  };
  const steps = [];
  const undoAll = async () => { while (steps.length) { steps.pop(); await post("/api/undo", { domain: "pcb" }); await sleep(400); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  const before = await json("/api/state");
  const snap = (s) => JSON.stringify({ nc: s.board_rules?.net_classes, td: s.routing?.teardrop_settings ?? null, st: s.board_rules?.stackup ?? null, vs: s.drawings?.via_stack_presets ?? null, tp: s.board_rules?.tuning_profiles ?? null });
  const rulesBefore = snap(before);
  const tree = () => [...document.querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim());

  await scenario("Board Finish: its own page with KiCad's choices, stored in the stackup", async (t) => {
    const p = await openPage("Board Finish");
    const tr = tree();
    t(tr.indexOf("Board Finish") === tr.indexOf("Physical Stackup") + 1, `after Physical Stackup (${tr.join(", ")})`);
    const finish = q(p, 'select[aria-label="Copper finish"]');
    const opts = [...finish.options].map((o) => o.textContent);
    t(opts[0] === "Not specified" && opts[opts.length - 1] === "User defined" && opts.includes("ENIG"), `GetStandardCopperFinishes (${opts.join("|")})`);
    t([...q(p, 'select[aria-label="Edge card connectors"]').options].map((o) => o.textContent).join("|") === "None|Yes|Yes, bevelled", "the edge connector choices");
    const stack = await openPage("Physical Stackup");
    t(!stack.querySelector('select[aria-label="Copper finish"]'), "Physical Stackup has no finish section");
    await openPage("Board Finish");
    await setValue(finish, "ENIG");
    await setValue(q(p, 'select[aria-label="Edge card connectors"]'), "2");
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const st = (await json("/api/state")).board_rules?.stackup;
    t(saved === "Saved." && st?.copper_finish === "ENIG" && st?.edge_connector === 2, `stored (${saved}, ${st?.copper_finish}, ${st?.edge_connector})`);
    await closeBoardSetup();
  });

  await scenario("Microvia Stacks: Add..., the dialog's checks, Apply stores the presets", async (t) => {
    const p = await openPage("Microvia Stacks");
    const tr = tree();
    t(tr.indexOf("Microvia Stacks") === tr.indexOf("Pre-defined Sizes") + 1, `after Pre-defined Sizes (${tr.join(", ")})`);
    t([...p.querySelectorAll("table.bs-grid thead th")].map((h) => h.textContent).join("|") === "Name|Type|Layers|Diameter / Hole|Fill|Pitch", "the grid's columns");
    await press(p, "Add...");
    const nameInput = document.querySelector('input[aria-label="Preset name:"]');
    t(!!nameInput, "Add... asks the preset's name");
    await setValue(nameInput, "HDI");
    await press(nameInput.closest(".dialog"), "OK", 300);
    const mv = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.innerText.trim() === "Microvia Stack");
    t(!!mv(), "the Microvia Stack dialog");
    await setValue(q(mv(), 'input[aria-label="Via diameter"]'), "0.3");
    await setValue(q(mv(), 'input[aria-label="Via hole"]'), "0.15");
    await press(mv().querySelector(".dialog-footer"), "OK", 300);
    const err = [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.innerText.trim() === "Error");
    const copper = (await json("/api/state")).layers.filter((l) => /\.Cu$/.test(l));
    if (!copper.includes("In1.Cu")) {
      t(/Select a start and an end layer present on this board\./.test(err?.textContent ?? ""), `a layer the board lacks is refused with KiCad's words (${err?.textContent})`);
      await press(err.querySelector(".dialog-footer"), "OK", 200);
      await setValue(q(mv(), 'select[aria-label="End layer"]'), "B.Cu");
      await press(mv().querySelector(".dialog-footer"), "OK", 300);
    }
    t(!mv(), "OK closes the dialog");
    const row = p.querySelector('table.bs-grid tbody tr[data-row="0"]');
    t(/Stacked/.test(row?.textContent ?? "") && /F\.Cu -> /.test(row?.textContent ?? ""), `the row (${row?.textContent})`);
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const vs = (await json("/api/state")).drawings?.via_stack_presets;
    t(saved === "Saved." && vs?.length === 1 && vs[0].name === "HDI" && vs[0].via_size_nm === 300_000 && vs[0].filled === true, `stored (${saved}, ${JSON.stringify(vs)})`);
    await closeBoardSetup();
  });

  await scenario("Tuning Profiles: Frequency and Model solder mask", async (t) => {
    const p = await openPage("Tuning Profiles");
    await press(p, "Add Tuning Profile");
    t(q(p, 'input[aria-label="Frequency"]').value === "1" && q(p, 'select[aria-label="Frequency units"]').selectedOptions[0]?.textContent === "GHz", "a new profile is 1 GHz");
    const mask = [...p.querySelectorAll("label")].find((l) => l.textContent.trim() === "Model solder mask")?.querySelector("input");
    t(!!mask && mask.checked, "Model solder mask, ticked");
    await setValue(q(p, 'input[aria-label="Profile name"]'), "v15");
    await setValue(q(p, 'select[aria-label="Frequency units"]'), "2");
    await setValue(q(p, 'input[aria-label="Frequency"]'), "500");
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const prof = (await json("/api/state")).board_rules?.tuning_profiles?.profiles?.find((x) => x.name === "v15");
    t(saved === "Saved." && prof?.frequency_hz === 500e6 && prof?.model_solder_mask === true, `stored (${saved}, ${JSON.stringify(prof)?.slice(0, 200)})`);
    await closeBoardSetup();
  });

  await scenario("Zones: Locked; Teardrops: Round pads only; Net Classes: a negative width", async (t) => {
    const z = await openPage("Zones");
    const locked = q(z, 'input[aria-label="Locked"]');
    const was = locked.checked;
    locked.click();
    await sleep(100);
    const saved = await apply(z);
    t(saved === "Saved." && (await json("/api/state")).drawings?.zone_defaults_locked === !was, `stored (${saved})`);
    locked.click();
    await sleep(100);
    await apply(z);
    const td = await openPage("Teardrops");
    q(td, 'input[aria-label="Round pads only"]').click();
    await sleep(100);
    await press(td, "Apply Settings", 800);
    steps.push("pcb");
    t((await json("/api/state")).routing?.teardrop_settings?.round_shapes_only === true, "stored as round_shapes_only");
    const nc = await openPage("Net Classes");
    await press(nc, "Add Netclass");
    const first = nc.querySelector("table.bs-grid tbody tr:first-child input.bs-name");
    await setValue(first, "neg");
    await setValue(q(nc, 'input[aria-label="neg Track Width"]'), "-0.1");
    const ncSaved = await apply(nc);
    if (ncSaved === "Saved.") steps.push("pcb");
    const cls = (await json("/api/state")).board_rules?.net_classes?.find((c) => c.name === "neg");
    t(ncSaved === "Saved." && cls?.track_width === -100_000, `a negative width is taken as typed (${ncSaved}, ${JSON.stringify(cls)?.slice(0, 120)})`);
    await closeBoardSetup();
  });

  await scenario("Schematic Setup > Compare Symbol with Library: KiCad's boxes, stored; the Import Settings box", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "Schematic")?.click();
    await sleep(1500);
    const setup = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.innerText.trim() === "Schematic Setup");
    await __eda.run("eeschema.EditorControl.schematicSetup");
    await sleep(800);
    const tr = [...setup().querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim());
    t(tr.indexOf("Compare Symbol with Library") === tr.indexOf("Violation Severity") + 1 && tr.indexOf("Pin Conflicts Map") === tr.indexOf("Compare Symbol with Library") + 1, `between Violation Severity and Pin Conflicts Map (${tr.join(", ")})`);
    [...setup().querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim() === "Compare Symbol with Library")?.click();
    await sleep(800);
    const p = setup().querySelector('section.bs-page[aria-label="Compare Symbol with Library"]');
    const boxes = [...(p?.querySelectorAll("label.bs-check") ?? [])].map((l) => [l.textContent.trim(), l.querySelector("input").checked]);
    t(boxes.length === 12 && boxes[0][0] === "Check for missing fields" && boxes[0][1] === true && boxes[5][0] === "Compare field positions" && boxes[5][1] === false, `KiCad's boxes and defaults (${JSON.stringify(boxes)})`);
    [...p.querySelectorAll("label.bs-check")].find((l) => l.textContent.trim() === "Compare field positions").querySelector("input").click();
    await sleep(100);
    const saved = await apply(p);
    const after = await json("/api/sch/setup");
    t(saved === "Saved." && after.symbol_parity?.field_positions === true, `stored (${saved}, ${JSON.stringify(after.symbol_parity)})`);
    await __eda.run("common.Interactive.undo");
    await sleep(1000);
    t((await json("/api/sch/setup")).symbol_parity?.field_positions === false, "one undo takes it back");
    await press(setup().querySelector(".dialog-footer"), "Import Settings from Another Project...");
    const dlg = document.querySelector('[role="dialog"][aria-label="Import Settings"]');
    t([...(dlg?.querySelectorAll("label") ?? [])].some((l) => l.textContent.trim() === "Compare symbol with library settings"), "the Import Settings dialog's box");
    [...(dlg?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(300);
    for (let i = 0; i < 3 && setup(); i++) {
      [...setup().querySelectorAll(".dialog-footer button")].find((b) => ["Close", "Discard and close"].includes(b.textContent.trim()))?.click();
      await sleep(300);
    }
    [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click();
    await sleep(1000);
  });

  await scenario("every step is undone", async (t) => {
    await closeBoardSetup();
    const made = steps.length;
    await undoAll();
    await sleep(800);
    t(snap(await json("/api/state")) === rulesBefore, `${made} steps undone, the pages' settings as they were`);
    t((__eda.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(__eda.errors?.() ?? []).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
