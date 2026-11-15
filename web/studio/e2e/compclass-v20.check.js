// Component classes from the schematic to the board (task-v20-compclass; PARITY-pcb section 125), driven in the studio and read back from /api/state:
//   - a symbol's `Component Class` field (Symbol Fields, `set_symbol_fields`) is its footprint's static class (`SCH_SYMBOL::GetComponentClassNames`, Update PCB
//     from Schematic's `updateComponentClass`): the part carries it, the Properties panel's Component Class row says it (`GetComponentClassAsString`) and the
//     message panel says "Component Class" (`GetMsgPanelInfo`, `GetHumanReadableName`); the field is no footprint field;
//   - an assignment rule of Board Setup > Component Classes adds its class to the whole class (`GetComponentClass`: "DYN,POWER", "DYN and POWER"), and a custom
//     condition `A.hasComponentClass('POWER')` sees the schematic's class (the page's matches, `POST /api/component_classes`);
//   - a rule area around another symbol with a directive label on its border that carries a `Component Class` gives that symbol's footprint the class;
//   - every step undone: no class left, and nothing logged an error.
// It answers through `window.__compclassV20Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-few minutes=4
(async () => {
  const results = [];
  const check = (window.__compclassV20Check = { results, done: false, ok: false, wait: null });
  const json = (url, body) => fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const S = () => E.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const state = () => json("/api/state");
  const cmd = (c) => json("/api/cmd", { cmd: c, strict: false });
  let undos = 0;
  const step = async (c) => { const r = await cmd(c); if (r.ok !== false) undos += 1; await sleep(700); return r; };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(500); };
  const tab = async (name) => {
    if (S().tab !== name) {
      [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
      await until(() => S().tab === name);
      await sleep(900);
    }
    return S().tab === name;
  };
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"] .prop-value`)?.textContent?.trim() ?? null;
  const msgClass = () => document.querySelector('.message-panel [data-msg="component-class"]')?.textContent?.replace(/^Component Class/, "").trim() ?? null;
  const part = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const xy = (at) => (Array.isArray(at) ? { x: at[0], y: at[1] } : { x: at.x, y: at.y });
  const errorsFrom = Date.now();
  const snapshot = async () => JSON.stringify((await state()).parts.map((p) => [p.ref, p.component_classes ?? [], p.component_class ?? []]));
  const start = await snapshot();

  // the symbols of mcu30's sheets (the root places the module sheets): the field goes on a placed one, the rule area around another placed one on its own sheet
  const root = await json("/api/schematic");
  const placed = new Set((await state()).parts.filter((p) => p.placed !== false).map((p) => p.ref));
  const onSheets = [];
  for (const sh of root.sheets ?? []) for (const s of (await json(`/api/schematic?sheet=${sh.id}`)).symbols ?? []) if ((s.unit ?? 1) <= 1) onSheets.push({ ...s, sheet: sh.id });
  const placedSymbols = onSheets.filter((s) => placed.has(s.id));
  const first = placedSymbols.find((s) => s.id === "U1") ?? placedSymbols[0];
  const second = placedSymbols.find((s) => s.id === "R1" && s.sheet !== first?.sheet) ?? placedSymbols.find((s) => s.sheet !== first?.sheet);
  const neighbour = placedSymbols.find((s) => s.sheet === second?.sheet && s.id !== second?.id);

  await scenario("a symbol's Component Class field is its footprint's class: the part, the Properties panel and the message panel", async (t) => {
    t(!!first && !!second && first.id !== second.id, `two symbols: ${first?.id}, ${second?.id}`);
    const r = await step({ op: "set_symbol_fields", add_fields: ["Component Class"], edits: [{ id: first.id, field: "Component Class", value: "POWER" }] });
    t(r.ok !== false, `set_symbol_fields: ${JSON.stringify(r).slice(0, 160)}`);
    const p = await until(async () => { const q = await part(first.id); return q?.component_classes ? q : null; });
    t(JSON.stringify(p?.component_classes) === '["POWER"]', `the part's static classes: ${JSON.stringify(p?.component_classes)}`);
    t(JSON.stringify(p?.component_class) === '["POWER"]', `its whole class: ${JSON.stringify(p?.component_class)}`);
    t(!(p?.fields ?? []).some?.((f) => f.name === "Component Class"), "the class is no footprint field");
    await tab("pcb");
    await select([first.id]);
    const v = await until(() => row("Component Class") === "POWER" && row("Component Class"));
    t(v === "POWER", `the Properties panel's Component Class row: ${row("Component Class")}`);
    t(msgClass() === "POWER", `the message panel: ${msgClass()}`);
  });

  await scenario("an assignment rule adds its class to the whole class, and a custom condition sees the schematic's class", async (t) => {
    const settings = { assignments: [{ class: "DYN", conditions: [{ kind: "reference", primary: first.id }] }] };
    const r = await step({ op: "set_component_classes", settings });
    t(r.ok !== false, `set_component_classes: ${JSON.stringify(r).slice(0, 160)}`);
    const p = await until(async () => { const q = await part(first.id); return (q?.component_class ?? []).length === 2 ? q : null; });
    t(JSON.stringify(p?.component_class) === '["DYN","POWER"]', `the whole class: ${JSON.stringify(p?.component_class)}`);
    t(JSON.stringify(p?.component_classes) === '["POWER"]', `the static classes stay: ${JSON.stringify(p?.component_classes)}`);
    await select([]);
    await select([first.id]);
    const v = await until(() => row("Component Class") === "DYN,POWER" && row("Component Class"));
    t(v === "DYN,POWER", `the Properties panel: ${row("Component Class")}`);
    t(msgClass() === "DYN and POWER", `the message panel: ${msgClass()}`);
    const m = await json("/api/component_classes", { assignments: [{ class: "SEEN", conditions: [{ kind: "custom", primary: "A.hasComponentClass('POWER')" }] }] });
    t(m.ok && JSON.stringify(m.matches?.[0]) === JSON.stringify([first.id]), `a custom condition matches the schematic's class: ${JSON.stringify(m.matches)}`);
  });

  await scenario("a rule area's directive label gives the symbols inside it its Component Class", async (t) => {
    const at = xy(second.at);
    const r = 5_000_000;
    const pts = [{ x: at.x - r, y: at.y - r }, { x: at.x + r, y: at.y - r }, { x: at.x + r, y: at.y + r }, { x: at.x - r, y: at.y + r }];
    const onSheet = (c) => ({ op: "on_sheet", sheet: second.sheet, cmd: c });
    const a = await step(onSheet({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "rule_area", pts } } }));
    t(a.ok !== false, `the rule area: ${JSON.stringify(a).slice(0, 160)}`);
    t(!(await part(second.id))?.component_classes, "an area with no directive gives no class");
    const d = await step(onSheet({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "directive", at: pts[0], orientation: 90000, shape: "round", pin_length_nm: 2_540_000, netclass: "", component_class: "ANALOG" } } }));
    t(d.ok !== false, `the directive: ${JSON.stringify(d).slice(0, 160)}`);
    const p = await until(async () => { const q = await part(second.id); return q?.component_classes ? q : null; });
    t((p?.component_classes ?? []).includes("ANALOG"), `${second.id}'s static classes: ${JSON.stringify(p?.component_classes)}`);
    t(!((await part(first.id))?.component_classes ?? []).includes("ANALOG"), `${first.id}, on another sheet, is not in it`);
    if (neighbour) t(!((await part(neighbour.id))?.component_classes ?? []).includes("ANALOG"), `${neighbour.id}, outside the area, is not in it`);
  });

  await scenario("every step undone: no class left, and nothing logged an error", async (t) => {
    await select([]);
    // the steps are of both editors (the symbol's field, the rule area and the directive are the schematic's; the page is the board's): undone in turn, newest first
    for (; undos > 0; undos--) {
      await json("/api/undo", {});
      await sleep(900);
    }
    const now = await snapshot();
    t(now === start, `as it was${now === start ? "" : `: ${now.slice(0, 300)}`}`);
    const errs = E.errors(errorsFrom).map((e) => e.message ?? String(e));
    t(errs.length === 0, `errors: ${JSON.stringify(errs)}`);
  });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
