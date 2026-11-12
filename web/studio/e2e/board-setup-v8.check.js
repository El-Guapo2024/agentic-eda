// Board Setup round v8 (UI-LEFT-2 BB4, BB8; task-v8-setup):
//   net classes   the Default class is the last row; Add Netclass puts a blank row at the top (`InsertRows()`); a 0 track width is taken as typed; the PCB colour
//                 is a cell of the table, stored with it in one undo step
//   zones         Design Rules > Zones (KiCad 25ec2335 `PANEL_SETUP_ZONES`) is after Pre-defined Sizes and holds the hatch offsets; a spoke narrower than the
//                 minimum width is refused with KiCad's words; Apply stores the board's zone defaults
//   teardrops     the "Default Properties for Rectangular Shapes" block is there and stored as `rect_shape`
//   import        "Import Settings from Another Board..." refuses a board without its project with KiCad's message; with it, the ticked pages show the other
//                 board's values, unapplied
//   end           every step undone, the rules as they were
// It answers through `window.__boardSetupV8Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed
(async () => {
  const results = [];
  const run = (window.__boardSetupV8Check = { results, done: false, ok: false, wait: null });
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
  const snap = (s) => JSON.stringify({ nc: s.board_rules?.net_classes, col: s.board_rules?.net_class_pcb_colors ?? null, td: s.routing?.teardrop_settings ?? null });
  const rulesBefore = snap(before);

  await scenario("Net Classes: Default last, a new class at the top, a 0 width taken, the PCB colour in the table's undo step", async (t) => {
    const p = await openPage("Net Classes");
    const grid = () => p.querySelectorAll("table.bs-grid")[0];
    t(/^Default/.test(grid().querySelector("tbody tr:last-child td")?.textContent?.trim() ?? ""), "the Default class is the last row (`loadNetclasses`)");
    await press(p, "Add Netclass");
    const first = grid().querySelector("tbody tr:first-child input.bs-name");
    t(!!first && first.value === "" && document.activeElement === first, "Add Netclass: a blank row at the top, its name focused (`InsertRows()`)");
    await setValue(first, "v8");
    await setValue(q(p, 'input[aria-label="v8 Track Width"]'), "0");
    await setValue(q(p, 'input[aria-label="v8 PCB Color"]'), "#00ff80");
    t(!q(p, 'input[aria-label="v8 Track Width"]').classList.contains("bs-invalid"), "a 0 track width is not marked (KiCad takes it as typed)");
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const s = await json("/api/state");
    const v8 = s.board_rules.net_classes.find((c) => c.name === "v8");
    t(saved === "Saved." && v8?.track_width === 0, `stored with its 0 width (${saved}, ${JSON.stringify(v8)})`);
    t(/0, 255, 128/.test(s.board_rules.net_class_pcb_colors?.v8 ?? ""), `the colour is the design's (${JSON.stringify(s.board_rules.net_class_pcb_colors)})`);
    await post("/api/undo", { domain: "pcb" });
    steps.pop();
    await sleep(800);
    const u = await json("/api/state");
    t(!u.board_rules.net_classes.some((c) => c.name === "v8") && !u.board_rules.net_class_pcb_colors?.v8, "one undo takes the class and its colour back");
    await closeBoardSetup();
  });

  await scenario("Zones: the page, KiCad's spoke check, Apply stores the board's defaults", async (t) => {
    const p = await openPage("Zones");
    const tree = [...document.querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim());
    t(tree.indexOf("Zones") === tree.indexOf("Pre-defined Sizes") + 1 && tree.indexOf("Teardrops") === tree.indexOf("Zones") + 1 && !tree.includes("Zone Hatch Offsets"), `after Pre-defined Sizes, before Teardrops (${tree.join(", ")})`);
    t(!document.querySelector('section.bs-page[aria-label="Zone Hatch Offsets"]')?.hidden, "the hatch offsets panel is on the page");
    await setValue(q(p, 'input[aria-label="Thermal spoke width"]'), "0.1mm");
    const refused = await apply(p);
    t(refused === "Thermal spoke width cannot be smaller than the minimum width.", `refused: ${refused}`);
    await setValue(q(p, 'input[aria-label="Thermal spoke width"]'), "0.5mm");
    await setValue(q(p, 'input[aria-label="Clearance"]'), "0.7mm");
    const saved = await apply(p);
    const z = (await json("/api/state")).drawings?.zone_defaults;
    t(saved === "Saved." && z?.clearance === 700_000, `stored (${saved}, ${JSON.stringify(z)?.slice(0, 200)})`);
    await closeBoardSetup();
  });

  await scenario("Teardrops: the rectangular-shape block", async (t) => {
    const p = await openPage("Teardrops");
    t(p.textContent.includes("Default Properties for Round Shapes") && p.textContent.includes("Default Properties for Rectangular Shapes"), "KiCad's two default blocks");
    await setValue(q(p, 'input[aria-label="Rectangular best length"]'), "30");
    await press(p, "Apply Settings", 800);
    steps.push("pcb");
    const td = (await json("/api/state")).routing?.teardrop_settings;
    t(Math.abs((td?.rect_shape?.best_length_ratio ?? 0) - 0.3) < 1e-9, `stored as rect_shape (${JSON.stringify(td?.rect_shape)})`);
    await closeBoardSetup();
  });

  await scenario("Import Settings from Another Board...: KiCad's refusal, then the ticked pages take the other board's values unapplied", async (t) => {
    await openPage("Constraints");
    await press(document.querySelector(".bs-dialog .dialog-footer"), "Import Settings from Another Board...");
    const dlg = () => document.querySelector('[role="dialog"][aria-label="Import Settings"]');
    t(!!dlg(), "the Import Settings dialog");
    const pcb = `(kicad_pcb (version 20241229) (generator "pcbnew") (general (thickness 1.6)) (paper "A4")\n  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))\n  (setup (pad_to_mask_clearance 0.05))\n  (net 0 ""))\n`;
    const pro = JSON.stringify({ board: { design_settings: { rules: { min_clearance: 0.33 }, defaults: { zones: { min_clearance: 0.9 } } } } });
    const pick = async (files) => {
      const input = q(dlg(), 'input[type="file"]');
      const dt = new DataTransfer();
      for (const [name, text] of files) dt.items.add(new File([text], name));
      input.files = dt.files;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(150);
    };
    const tick = async (label) => { [...dlg().querySelectorAll("label")].find((l) => l.textContent.trim() === label)?.querySelector("input")?.click(); await sleep(80); };
    await pick([["other.kicad_pcb", pcb]]);
    await tick("Design rule constraints");
    await tick("Zone defaults");
    await press(dlg(), "Import Settings", 1200);
    const err = dlg()?.querySelector('[role="alert"]')?.textContent ?? "";
    t(err === "Error importing settings from board:\nAssociated project file other.kicad_pro could not be loaded", `KiCad's refusal (${JSON.stringify(err)})`);
    await pick([["other.kicad_pcb", pcb], ["other.kicad_pro", pro]]);
    await press(dlg(), "Import Settings", 1500);
    t(!dlg(), "the dialog closes on an import");
    const c = document.querySelector('section.bs-page[aria-label="Constraints"]');
    const minClearance = [...c.querySelectorAll(".bs-row")].find((r) => r.querySelector(".bs-label")?.textContent?.trim() === "Minimum clearance:")?.querySelector("input");
    t(/0\.33/.test(minClearance?.value ?? ""), `Constraints shows the other board's minimum clearance (${minClearance?.value})`);
    t(/Changes not applied yet/.test(c.querySelector(".bs-msg")?.textContent ?? ""), "unapplied");
    const z = await openPage("Zones");
    t(/0\.9/.test(q(z, 'input[aria-label="Clearance"]').value), `Zones shows the other board's zone clearance (${q(z, 'input[aria-label="Clearance"]').value})`);
    const stored = (await json("/api/state")).board_rules.constraints.min_clearance_nm;
    t(stored === before.board_rules.constraints.min_clearance_nm, "nothing stored on this board");
    await closeBoardSetup();
  });

  await scenario("every step is undone", async (t) => {
    await closeBoardSetup();
    const made = steps.length;
    await undoAll();
    await sleep(800);
    t(snap(await json("/api/state")) === rulesBefore, `${made} steps undone, the classes, colours and teardrop settings as they were`);
    t((__eda.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(__eda.errors?.() ?? []).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
