// Board Setup's Component Classes and Net Classes columns, the DRC Rule Editor's Show Matches and tree drag-and-drop, and Assign Netclass's preview (UI-LEFT B1, B2, B4):
//   component classes  the page is in the tree after Net Classes; Add Custom Assignment; the "+" menu lists KiCad's seven conditions; a Reference condition's import
//                      menu offers the selection; "Highlight matching footprints" brightens what the rule engine says the assignment matches; Apply refuses a
//                      condition that does not compile with KiCad's message, then stores the page as one undo step (`board_rules.component_classes`)
//   net classes        the Tuning Profile and PCB Color columns are on the board's grid, Import colors from schematic is there; a bus pattern `D[0..3]` is stored as
//                      its four members and the page shows four rows; a PCB colour is the Appearance panel's net class colour once Apply succeeds
//   assign netclass    the dialog selects every item of the nets its pattern matches as the pattern is typed
//   rule editor        Show Matches selects what a panel's rule matches and says how many ("Show Matches (n)"); a condition that does not compile says "(error)";
//                      a rule dragged onto another of its node moves in the file (one undo step)
//   end                every step undone, the rules, the classes and the colours as they were
// It answers through `window.__boardSetupClassesCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed
(async () => {
  const results = [];
  const run = (window.__boardSetupClassesCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const stored = async () => (await json("/api/state")).board_rules;
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(80);
  };
  const button = (root, text) => (root ? [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text) : undefined);
  const press = async (root, text, wait = 200) => { const b = button(root, text); if (!b) throw new Error(`no "${text}" button`); b.click(); await sleep(wait); };
  const q = (root, sel) => { const el = root?.querySelector(sel); if (!el) throw new Error(`no ${sel}`); return el; };
  const dialog = () => document.querySelector(".bs-dialog");
  const openPage = async (label) => {
    if (!dialog()) { await __eda.run("pcbnew.EditorControl.boardSetup"); await sleep(500); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith(label))?.click();
    await sleep(300);
    const p = document.querySelector(`section.bs-page[aria-label="${label}"]`);
    if (!p) throw new Error(`Board Setup has no "${label}" page`);
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
  const rulesBefore = JSON.stringify({ cc: before.board_rules?.component_classes ?? {}, nc: before.board_rules?.net_classes, dru: before.board_rules?.custom_rules_text ?? "", tp: before.board_rules?.tuning_profiles?.class_profiles ?? {} });
  const parts = before.parts.filter((p) => p.placed !== false).map((p) => p.ref).sort();
  const nets = [...new Set(before.parts.flatMap((p) => (p.pads ?? []).map((x) => x.net)).filter((n) => n))].sort();
  if (__eda.state().tab !== "pcb") { await __eda.run("pcbnew.EditorControl.boardSetup"); }

  await scenario("Component Classes: the page, its conditions, Highlight matching footprints and Apply", async (t) => {
    const p = await openPage("Component Classes");
    const tree = [...document.querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim());
    t(tree.indexOf("Component Classes") === tree.indexOf("Net Classes") + 1 && tree.indexOf("Custom Rules") === tree.indexOf("Component Classes") + 1, `after Net Classes, before Custom Rules (${tree.join(", ")})`);
    t(p.textContent.includes("Assign component class per sheet") && p.textContent.includes("Custom Assignments:"), "KiCad's checkbox and label");
    await press(p, "Add Custom Assignment");
    await setValue(q(p, 'input[aria-label="Assignment 1 component class"]'), "BIG");
    q(p, 'button[aria-label="Assignment 1 add condition"]').click();
    await sleep(200);
    const menu = [...document.querySelectorAll(".bs-cc-menu button")].map((b) => b.textContent.trim());
    t(menu.join("|") === "Add Reference Condition|Add Footprint Condition|Add Side Condition|Add Rotation Condition|Add Footprint Field Value Condition|Add Sheet Condition|Add Custom Expression Condition", `the + menu (${menu.join("|")})`);
    [...document.querySelectorAll(".bs-cc-menu button")].find((b) => b.textContent.trim() === "Add Reference Condition").click();
    await sleep(200);
    q(p, 'button[aria-label="Assignment 1 condition 1 import references"]').click();
    await sleep(200);
    const imp = [...document.querySelectorAll(".bs-cc-menu button")].find((b) => b.textContent.trim() === "Import references from selection");
    t(!!imp && imp.disabled, "Import references from selection is there, off with nothing selected");
    document.querySelector(".bs-cc-menu-backdrop")?.click();
    await sleep(150);
    await setValue(q(p, 'input[aria-label="Assignment 1 condition 1 references"]'), ` ${parts[0]},${parts[1]} `);
    q(p, 'button[aria-label="Assignment 1 highlight"]').click();
    await sleep(800);
    const bright = __eda.state().brightened.slice().sort();
    t(JSON.stringify(bright) === JSON.stringify([parts[0], parts[1]].sort()), `the two footprints are brightened (${bright})`);
    // a custom condition that does not compile
    q(p, 'button[aria-label="Assignment 1 add condition"]').click();
    await sleep(200);
    [...document.querySelectorAll(".bs-cc-menu button")].find((b) => b.textContent.trim() === "Add Custom Expression Condition").click();
    await sleep(200);
    await setValue(q(p, 'input[aria-label="Assignment 1 condition 2 expression"]'), "A.Reference ==");
    const refused = await apply(p);
    t(refused === "Error with conditions for component class assignment BIG", `refused: ${refused}`);
    t(JSON.stringify((await stored()).component_classes ?? {}) === JSON.stringify(before.board_rules?.component_classes ?? {}), "nothing stored");
    q(p, 'button[aria-label="Assignment 1 condition 2 delete"]').click();
    await sleep(200);
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const cc = (await stored()).component_classes;
    t(saved === "Saved." && cc?.assignments?.[0]?.class === "BIG" && cc.assignments[0].conditions?.[0]?.primary === `${parts[0]},${parts[1]}` && cc.assignments[0].conditions[0].kind === "reference", `stored, trimmed as GetPrimaryField trims (${JSON.stringify(cc)})`);
    await closeBoardSetup();
    t(__eda.state().brightened.length === 0, "closing the dialog unbrightens the footprints");
  });

  await scenario("Net Classes: the Tuning Profile and PCB Color columns, a bus pattern, the colour", async (t) => {
    const p = await openPage("Net Classes");
    const heads = [...p.querySelectorAll("table.bs-grid thead th")].map((th) => th.textContent.trim());
    t(heads.join("|").startsWith("Name|Clearance|Track Width|Via Size|Via Hole|uVia Size|uVia Hole|DP Width|DP Gap|Tuning Profile|PCB Color"), `KiCad's columns in its order (${heads.join("|")})`);
    t(!!button(p, "Import colors from schematic") && p.textContent.includes("Set color to transparent to use layer default color."), "Import colors from schematic and the help text");
    await press(p, "Add Netclass");
    const name = [...p.querySelectorAll('input[aria-label^="Netclass "][aria-label$=" name"]')].pop();
    await setValue(name, "busy");
    await press(p, "Add Assignment");
    const pattern = [...p.querySelectorAll('input[aria-label^="Pattern "]')].pop();
    const row = pattern.getAttribute("aria-label").replace("Pattern ", "");
    await setValue(pattern, "D[0..3]");
    await setValue(q(p, `select[aria-label="Net class of pattern ${row}"]`), "busy");
    await setValue(q(p, 'input[aria-label="busy PCB Color"]'), "#ff8000");
    const saved = await apply(p);
    if (saved === "Saved.") steps.push("pcb");
    const rules = await stored();
    const busy = rules.net_classes.find((c) => c.name === "busy");
    t(saved === "Saved." && JSON.stringify(busy?.nets) === '["D0","D1","D2","D3"]', `D[0..3] is stored as its members (${JSON.stringify(busy?.nets)})`);
    await sleep(500);
    const shown = [...p.querySelectorAll('input[aria-label^="Pattern "]')].map((i) => i.value).filter((v) => /^D\d$/.test(v));
    t(shown.join(",") === "D0,D1,D2,D3", `the page shows a row a member (${shown})`);
    t(/rgb\(255, 128, 0\)/.test(__eda.state().appearance.netclassColors?.busy ?? ""), `the PCB colour is the Appearance panel's (${JSON.stringify(__eda.state().appearance.netclassColors)})`);
    await closeBoardSetup();
  });

  await scenario("Assign Netclass previews the nets its pattern matches", async (t) => {
    const net = nets.find((n) => n === "GND") ?? nets[0];
    const outline = before.outline ?? [[0, 0], [20000000, 20000000]];
    const [cx, cy] = [(Math.min(...outline.map((p) => p[0])) + Math.max(...outline.map((p) => p[0]))) / 2, (Math.min(...outline.map((p) => p[1])) + Math.max(...outline.map((p) => p[1]))) / 2].map(Math.round);
    const r = await post("/api/cmd", { strict: false, cmd: { op: "add_track", net, layer: "F.Cu", width: 250000, pts: [[cx, cy], [cx + 2000000, cy]] } });
    if (r.ok) steps.push("pcb");
    t(r.ok, `a track on ${net} to select (${JSON.stringify(r).slice(0, 200)})`);
    await sleep(600);
    const track = (await json("/api/state")).routing.tracks.find((x) => x.net === net);
    await __eda.run("common.InteractiveSelection.clear");
    await __eda.run("common.InteractiveSelection.selectItems", [track.id]);
    await sleep(300);
    await __eda.run("pcbnew.EditorControl.assignNetclass");
    await sleep(500);
    const dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Assign Netclass"));
    if (!dlg) throw new Error("the Assign Netclass dialog did not open");
    const sel = __eda.state().selection.map((s) => s.id);
    t(sel.includes(track.id), `the opening pattern's nets are selected (${sel.slice(0, 6)})`);
    await setValue(q(dlg, 'input[aria-label="Pattern"]'), "NO_SUCH_NET");
    t(__eda.state().selection.length === 0, "a pattern that matches nothing selects nothing");
    await setValue(q(dlg, 'input[aria-label="Pattern"]'), net);
    t(__eda.state().selection.some((s) => s.id === track.id), "typing the net's name selects its track again");
    await press(dlg, "Cancel");
  });

  await scenario("DRC Rule Editor: Show Matches and dragging a rule", async (t) => {
    const add = (name, mm, cond) => post("/api/cmd", { cmd: { op: "edit_drc_rule", action: "save", entry: null, name: "", rule: { panel: "minimum_clearance", name, comment: "", condition: cond, layer: "", fields: { value: mm }, value_text: String(mm), taken_names: [], severity: null, constraint_code: null } } });
    for (const [n, mm, c] of [["one", 0.3, `A.NetName == '${nets[0]}'`], ["two", 0.4, ""], ["three", 0.5, ""]]) {
      const r = await add(n, mm, c);
      if (r.ok) steps.push("pcb");
      t(r.ok, `rule ${n} saved (${JSON.stringify(r).slice(0, 160)})`);
    }
    await __eda.run("pcbnew.DRETool.drcRuleEditor");
    await sleep(900);
    for (let i = 0; i < 6; i++) { const tw = [...document.querySelectorAll(".re-twisty")].filter((x) => x.textContent === "▸"); if (!tw.length) break; for (const x of tw) x.click(); await sleep(100); }
    const node = (name) => [...document.querySelectorAll('[data-testid^="re-node-"]')].find((n) => n.dataset.name === name);
    node("one").click();
    await sleep(300);
    const btn = document.querySelector('[data-testid="re-show-matches"]');
    t(btn?.textContent === "Show Matches", `the panel has Show Matches (${btn?.textContent})`);
    btn.click();
    await sleep(1200);
    const sel = __eda.state().selection.map((s) => s.id);
    const expected = (await post("/api/rule_editor/show_matches", { entry: 0, rule: { panel: "minimum_clearance", name: "one", comment: "", condition: `A.NetName == '${nets[0]}'`, layer: "", fields: { value: 0.3 }, value_text: "0.3", taken_names: [], severity: null, constraint_code: null } })).ids ?? [];
    t(/^Show Matches \(\d+\)$/.test(btn.textContent) && sel.length > 0 && expected.every((id) => sel.includes(id)), `the rule's items are selected and counted (${btn.textContent}; ${sel.length} selected)`);
    // a condition that does not compile
    const custom = [...document.querySelectorAll('[data-testid="re-cond-type"]')][0];
    await setValue(custom, "custom");
    t(btn.textContent === "Show Matches", "changing the conditions resets the label");
    await setValue(document.querySelector('[data-testid="re-cond-custom"]'), "A.NetName ==");
    btn.click();
    await sleep(1000);
    t(btn.textContent === "Show Matches (error)", `a condition that does not compile (${btn.textContent})`);
    // drag "three" onto "one": the first of the node, so it goes first
    document.querySelector('[data-testid="re-cancel"]').click();
    await sleep(300);
    if (document.querySelector('[data-testid="re-ask"]')) { document.querySelector('[data-testid="re-ask-no"]')?.click(); await sleep(300); }
    await __eda.run("pcbnew.DRETool.drcRuleEditor");
    await sleep(900);
    for (let i = 0; i < 6; i++) { const tw = [...document.querySelectorAll(".re-twisty")].filter((x) => x.textContent === "▸"); if (!tw.length) break; for (const x of tw) x.click(); await sleep(100); }
    const dt = new DataTransfer();
    node("three").dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: dt }));
    await sleep(100);
    node("one").dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: dt }));
    await sleep(100);
    t(node("one").classList.contains("re-drop-target"), "the rule under the drag is highlighted");
    node("one").dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
    await sleep(1500);
    steps.push("pcb");
    const order = (await json("/api/rule_editor")).entries.map((e) => e.rule_name);
    t(JSON.stringify(order) === '["three","one","two"]', `dropped on the first rule, it goes first (${order})`);
    document.querySelector('[data-testid="re-cancel"]')?.click();
    await sleep(300);
  });

  await scenario("every step is undone", async (t) => {
    await closeBoardSetup();
    const made = steps.length;
    await undoAll();
    await sleep(800);
    const after = await json("/api/state");
    const rulesAfter = JSON.stringify({ cc: after.board_rules?.component_classes ?? {}, nc: after.board_rules?.net_classes, dru: after.board_rules?.custom_rules_text ?? "", tp: after.board_rules?.tuning_profiles?.class_profiles ?? {} });
    t(rulesAfter === rulesBefore, `${made} steps undone, the rules as they were`);
    // the colour is a view setting (appearance.json, as the Appearance panel keeps it), not an undo step: taken back as that panel would
    await __eda.run("studio.Appearance.op", { op: "netclass_color", name: "busy", color: null });
    await __eda.run("common.InteractiveSelection.clear");
    t((__eda.errors?.() ?? []).length === 0, `no console errors (${JSON.stringify(__eda.errors?.() ?? []).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
