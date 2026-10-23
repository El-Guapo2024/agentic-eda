// DRC Rule Editor check (P9b, PARITY-pcb.md section 48): pcbnew.DRETool.drcRuleEditor -- the tree (KiCad's categories, a node per constraint, the rules
// under them, the filter), New Rule, Copy Rule, Save with KiCad's validation messages, Move Down, Delete with its confirmation, the panels' pictures and
// fields (a numeric panel, Via style, Allowed orientation), the condition rows, the layer selector, a Custom rule's Check Syntax, and Close with a new
// rule and with unsaved changes. Every edit is one `edit_drc_rule` verb; the rules file is read back through `GET /api/rule_editor` and `/api/state`.
// Run it in the page of a served COPY of a board (it edits and undoes): every verb it sends is undone at the end and the file compared with how it was.
// It answers through `window.__ruleEditorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__ruleEditorCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  const rulesText = async () => (await json("/api/state")).board_rules?.custom_rules_text ?? "";
  const editor = () => json("/api/rule_editor");
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const all = (id) => [...document.querySelectorAll(`[data-testid="${id}"]`)];
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const type = async (id, v, el = tid(id)) => { if (!el) throw new Error(`no control ${id}`); setValue(el, v); await sleep(80); };
  const click = async (id, el = tid(id)) => { if (!el) throw new Error(`no control ${id}`); el.click(); await sleep(150); };
  const node = (name) => [...document.querySelectorAll('[data-testid^="re-node-"]')].find((n) => n.dataset.name === name);
  const items = () => [...document.querySelectorAll('[data-testid^="re-node-"]')].map((n) => n.textContent.replace(/^[▾▸]/, "").trim());
  const pick = async (name) => { const n = node(name); if (!n) throw new Error(`no tree item ${name}`); n.click(); await sleep(150); };
  /** Expand every collapsed node, so the rules show. */
  const expandAll = async () => { for (let i = 0; i < 6; i++) { const t = [...document.querySelectorAll(".re-twisty")].filter((x) => x.textContent === "▸"); if (!t.length) break; for (const x of t) x.click(); await sleep(80); } };
  const ask = () => tid("re-ask-text")?.textContent ?? null;
  const answer = async (v) => { await click(`re-ask-${v}`); await sleep(300); };
  const save = async () => { await click("re-save"); await sleep(1200); };
  let verbs = 0;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  const original = await rulesText();
  const nets = (await editor()).nets ?? [];
  await __eda.run("pcbnew.DRETool.drcRuleEditor");
  await sleep(800);

  await scenario("the tree is KiCad's, and a category shows its rules", async (t) => {
    t(!!tid("rule-editor"), "the dialog opens from pcbnew.DRETool.drcRuleEditor");
    t(document.querySelector(".re-dialog .dialog-header")?.textContent === "Design Rule Editor", "titled Design Rule Editor");
    const top = items();
    t(JSON.stringify(top) === JSON.stringify(["Electrical", "Manufacturability", "Highspeed design", "Footprints", "Custom"]), `collapsed: the five categories (${top})`);
    t(!!tid("re-group") && [...tid("re-group").querySelectorAll("th")].map((x) => x.textContent).join("|") === "Rule type|Rule name|Comment", "Electrical selected: the group header");
    await expandAll();
    t(items().includes("Minimum clearance") && items().includes("Vias under SMD") && items().includes("Custom rule"), "expanded: a node per constraint");
    t(tid("re-add").disabled && tid("re-delete").disabled, "on a category, New Rule and Delete Rule are off");
  });

  await scenario("New Rule on Minimum clearance, Save with KiCad's messages", async (t) => {
    await pick("Minimum clearance");
    t(!tid("re-add").disabled && tid("re-copy").disabled, "on a constraint node: New Rule on, Copy Rule off");
    await click("re-add");
    t(items().includes("Minimum clearance 1 *"), "the new rule is Minimum clearance 1, starred");
    t(tid("re-panel")?.dataset.panel === "minimum_clearance", "its panel opens");
    t(tid("re-overlay")?.querySelector("img")?.getAttribute("src")?.endsWith("constraint_minimum_clearance.png"), "with KiCad's picture");
    t(document.querySelector(".re-panel h4")?.textContent === "Minimum clearance Constraint", "headed Minimum clearance Constraint");
    t(tid("re-conditions")?.textContent.includes("Where conditions match:") && !!tid("re-cond-object"), "a two-object constraint: Object A / B");
    const layers = [...tid("re-layer").options].map((o) => o.textContent);
    t(layers[0] === "Any" && layers[1] === "outer" && layers[2] === "inner" && layers.includes("F.Cu"), `the layers of a copper constraint (${layers})`);
    t(!tid("re-save").disabled, "Save is on");
    await type("re-field-value", "");
    await save();
    t(ask() === "1. Value is required.\n", `empty: ${JSON.stringify(ask())}`);
    await answer("ok");
    await type("re-field-value", "-1");
    await save();
    t(ask() === "1. Value must be greater than 0.\n", `negative: ${JSON.stringify(ask())}`);
    await answer("ok");
    await type("re-field-value", "0.3");
    await type("re-name", "hv");
    await type("re-comment", "high voltage");
    if (nets.length) {
      await type("re-cond-type", "net");
      await type("re-cond-value", nets[0]);
    }
    await type("re-layer", "outer");
    await save();
    verbs++;
    const text = await rulesText();
    const cond = nets.length ? `\t(condition "A.NetName == '${nets[0]}'")\n` : "";
    t(text === `(version 1)\n(rule hv\n\t# high voltage\n\t(layer outer)\n\t(constraint clearance (min 0.3mm))\n${cond})\n`, `the file: ${JSON.stringify(text)}`);
    t(items().includes("hv") && !items().some((x) => x.endsWith("*")), "the tree shows the saved rule, no star");
    t(tid("re-panel")?.dataset.panel === "minimum_clearance" && tid("re-name")?.value === "hv", "the saved rule stays open");
  });

  await scenario("Copy Rule, a name that is taken, Move Down, Delete", async (t) => {
    await pick("hv");
    await click("re-copy");
    t(items().includes("hv 1 *"), `Copy Rule names the copy hv 1 (${items()})`);
    t(tid("re-field-value")?.value === "0.3" && tid("re-comment")?.value === "high voltage", "with the source's values");
    await type("re-name", "hv");
    await save();
    t(ask() === "1. Rule name must be unique.\n", `a taken name: ${JSON.stringify(ask())}`);
    await answer("ok");
    await type("re-name", "lv");
    await type("re-field-value", "0.1");
    await save();
    verbs++;
    let e = (await editor()).entries.map((x) => x.rule_name);
    t(JSON.stringify(e) === '["hv","lv"]', `two rules (${e})`);
    await pick("hv");
    t(tid("re-up").disabled && !tid("re-down").disabled, "the first rule moves down only");
    await click("re-down");
    await sleep(1200);
    verbs++;
    e = (await editor()).entries.map((x) => x.rule_name);
    t(JSON.stringify(e) === '["lv","hv"]', `Move Down changes the file's order (${e})`);
    await pick("lv");
    await click("re-delete");
    t(ask() === "Are you sure you want to delete?", `asked first: ${JSON.stringify(ask())}`);
    await answer("cancel");
    t((await editor()).entries.length === 2, "Cancel keeps it");
    await click("re-delete");
    await answer("ok");
    await sleep(1200);
    verbs++;
    e = (await editor()).entries.map((x) => x.rule_name);
    t(JSON.stringify(e) === '["hv"]', `Delete takes it out (${e})`);
  });

  await scenario("Via style and Allowed orientation write what their panels show", async (t) => {
    await pick("Via style");
    await click("re-add");
    t(tid("re-overlay")?.querySelector("img")?.getAttribute("src")?.endsWith("constraint_via_style.png"), "Via style's picture");
    t(tid("re-conditions")?.textContent.includes("Where object matches:") && !tid("re-cond-object"), "a one-object constraint");
    await save();
    t(ask() === "1. At least one constraint must be specified\n", `nothing set: ${JSON.stringify(ask())}`);
    await answer("ok");
    await type("re-field-min_via_diameter", "0.3");
    await type("re-field-max_via_diameter", "0.6");
    await type("re-field-min_via_hole", "0.15");
    await type("re-field-max_via_hole", "0.3");
    await type("re-field-via_type", "micro");
    await save();
    verbs++;
    let text = await rulesText();
    t(text.includes("(rule \"Via style 1\"\n\t(constraint via_diameter (min 0.3mm) (max 0.6mm))\n\t(constraint hole_size (min 0.15mm) (max 0.3mm))\n\t(condition \"A.Via_Type == 'Micro'\")\n)"), `the file: ${JSON.stringify(text)}`);
    await pick("Allowed orientation");
    await click("re-add");
    await click("re-field-all_degrees");
    t(tid("re-field-deg0").checked && tid("re-field-deg270").checked, "Allow All checks the four");
    await click("re-field-deg90");
    t(!tid("re-field-all_degrees").checked, "one unchecked: Allow All goes off");
    const layers = [...tid("re-layer").options].map((o) => o.textContent);
    t(JSON.stringify(layers) === '["Any","Top","Bottom"]', `Top / Bottom (${layers})`);
    await type("re-layer", "bottom");
    await save();
    verbs++;
    text = await rulesText();
    t(text.includes("(rule \"Allowed orientation 1\"\n\t(layer \"B.Cu\")\n\t(constraint assertion \"A.Orientation == 0 deg || A.Orientation == 180 deg || A.Orientation == 270 deg\")\n)"), `the file: ${JSON.stringify(text)}`);
  });

  await scenario("a Custom rule's text and Check Syntax", async (t) => {
    await pick("Custom rule");
    await click("re-add");
    t(tid("re-custom-text")?.value === "   (constraint clearance (min 0.2mm))", "a new Custom rule starts with KiCad's example body");
    t(!tid("re-conditions") && !tid("re-layer"), "no condition rows and no layer for a Custom rule");
    await click("re-check-syntax");
    await sleep(400);
    // KiCad's parser tells its reporter "No errors found.", so the tip says it under "Syntax check passed with warnings:".
    t(tid("re-syntax-tip")?.textContent === "Syntax check passed with warnings:\nNo errors found.\n", `Check Syntax: ${JSON.stringify(tid("re-syntax-tip")?.textContent)}`);
    await type("re-custom-text", "(constraint nonsense)");
    await click("re-check-syntax");
    await sleep(400);
    t((tid("re-syntax-tip")?.textContent ?? "").startsWith("ERROR: <a href='3:13'>Unrecognized item 'nonsense'.</a>"), `an error: ${tid("re-syntax-tip")?.textContent}`);
  });

  await scenario("the filter, and Close with a new rule and with unsaved changes", async (t) => {
    await type("re-filter", "orient");
    t(JSON.stringify(items()) === JSON.stringify(["Footprints", "Allowed orientation", "Allowed orientation 1"]), `filtered (${items()})`);
    await type("re-filter", "");
    // The Custom rule from the scenario before is new: Close (Discard) takes it away and the dialog stays.
    t(tid("re-cancel").textContent === "Discard", "with changes the button says Discard");
    await click("re-cancel");
    t(!items().some((x) => x.startsWith("Custom rule 1")) && !!tid("rule-editor"), "the new rule goes, the dialog stays");
    t(tid("re-cancel").textContent === "Close", `and the changes with it (RemoveRule's ClearModified): ${tid("re-cancel").textContent}`);
    await expandAll();
    await pick("hv");
    await type("re-field-value", "0.5");
    await click("re-cancel");
    t((ask() ?? "").startsWith("The following rules have unsaved changes:\n\n  • hv\n"), `asked: ${JSON.stringify(ask())}`);
    await answer("cancel");
    t(!!tid("rule-editor"), "Cancel keeps the dialog");
    await click("re-cancel");
    await answer("no");
    t(!tid("rule-editor"), "No closes it");
    t((await rulesText()).includes("(constraint clearance (min 0.3mm))"), "and the change was not saved");
  });

  await scenario("every edit was one undo step", async (t) => {
    for (let i = 0; i < verbs; i++) {
      await __eda.run("common.Interactive.undo");
      await sleep(500);
    }
    const text = await rulesText();
    t(text === original, `after ${verbs} undos the rules are as they were (${JSON.stringify(text)})`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
