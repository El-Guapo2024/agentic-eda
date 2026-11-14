// UI-LEFT-3 lane sch-nets (task-v18-schnets, PARITY-sch section 101): S3-17 and S3-18 in the page (S3-15, S3-19, S3-20, S3-21 and S3-27 are proven in Rust,
// with kicad-cli where it has an answer).
//   * S3-17: Label Properties on a label whose fields were placed by hand: Add Field's row has X / Y Position 0 and the orientation and justification of the
//     row before it (`OnAddField`: `SetAttributes( m_fields->at( size - 1 ) )`), and OK puts the new field at the label's anchor that way, one undo step.
//   * S3-18: Do not Populate in a design variant: two symbols, one DNP in the variant and one not, go to DNP together (the server works out `new_state`
//     over every unit's record, `SetAttribute`), then back together; the symbols' own flags are untouched.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml, on a COPY of a board: every scenario undoes what it did. It answers through
// `window.__schNetsV18Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__schNetsV18Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => __eda.state();
  const run = (id, ...args) => __eda.run(id, ...args);
  // the sheet in view (`sheet=` its path) in a variant
  const sch = (variant = "") => {
    const q = [state().sheetPath.length ? `sheet=${encodeURIComponent(state().sheetPath.join("/"))}` : "", variant ? `variant=${encodeURIComponent(variant)}` : ""].filter(Boolean).join("&");
    return json(`/api/schematic${q ? `?${q}` : ""}`);
  };
  const onSheet = (cmd) => (state().sheetPath.length ? { op: "on_sheet", sheet: state().sheetPath.join("/"), cmd } : cmd);
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 8000);
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(200); } };
  const leave = async () => { while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const pick = async (sel, text) => {
    sel.selectedIndex = [...sel.options].findIndex((o) => o.textContent === text || o.value === text);
    sel.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(600);
  };
  const variantSelect = () => document.querySelector("[data-testid=sch-variant-select]");
  const mm = (v) => Math.round(v * 1e6);

  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.dnp ?? null]), labels: s.labels.map((l) => [l.id, l.net, l.at, l.fields_hand_placed ?? null, (l.fields ?? []).map((f) => [f.name, f.at])]) });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
      try { await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`);
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1500); }
  await leave();

  // a global label with a net class, its fields placed by hand: the net class turned upright, right- and top-justified, 5.08 mm right of the anchor
  const s0 = await sch();
  const known = new Set(s0.labels.map((l) => l.id));
  await post({ op: "sch_edit", verb: "place_label", net: "V18_G", at: { x: mm(110), y: mm(232) }, kind: { scope: "global", shape: "input" }, spin: "right" });
  await sleep(900);
  const glabel = (await sch()).labels.find((l) => !known.has(l.id));
  if (glabel) {
    await post({ op: "sch_edit", verb: "edit_label", id: glabel.id, fields: [{ name: "Netclass", text: "V18_CLASS", visible: true, italic: false }] });
    await post({ op: "sch_edit", verb: "edit_field", id: `fld:${glabel.id}:Netclass`, local_at: { x: mm(5.08), y: mm(2.54) }, local_vertical: true, h: "right", v: "top" });
  }
  await sleep(900);
  const placed = (await sch()).labels.find((l) => l.id === glabel?.id);
  base = await snapshot();
  results.push({ name: "setup", ok: !!(glabel && placed?.fields_hand_placed), notes: [`label ${glabel?.id}, placed by hand ${placed?.fields_hand_placed}`] });

  // ----- S3-17
  await scenario("S3-17: Add Field on a label placed by hand: X / Y 0 and the row before's orientation and justification; OK puts it at the anchor, one step", async (t) => {
    await select([glabel.id]);
    await run("eeschema.InteractiveEdit.properties");
    const dlg = await opened(/^Global Label Properties/);
    if (!t(!!dlg, "E opens Global Label Properties")) return;
    // the place columns, from the column labels' menu
    for (const col of ["X Position", "Orientation"]) {
      dlg.querySelector("table.label-fields thead").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 200, clientY: 200 }));
      await sleep(250);
      [...document.querySelectorAll(".menu-node-item")].find((el) => el.textContent.includes(col))?.click();
      await sleep(250);
    }
    aria("Add field", dlg).click();
    await sleep(250);
    const x = aria("Field 2 x position", dlg);
    const h = aria("Field 2 h align", dlg);
    const o = aria("Field 2 orientation", dlg);
    if (!t(!!x && !!h && !!o, "the new row's X Position, H Align and Orientation cells")) return;
    t(Number(x.value) === 0, `X Position 0, the label's anchor (${x.value})`);
    t(h.value === aria("Field 1 h align", dlg)?.value && h.value === "right", `the H Align of the row before (${h.value})`);
    t(o.value === "Vertical", `the orientation of the row before (${o.value})`);
    setValue(aria("Field 2 name", dlg), "Note");
    setValue(aria("Field 2 value", dlg), "V18_NOTE");
    await sleep(200);
    buttonText(dlg, "OK").click();
    await sleep(1500);
    const l = (await sch()).labels.find((y) => y.id === glabel.id);
    const note = l?.fields?.find((f) => f.name === "Note");
    const nc = l?.fields?.find((f) => f.name === "Netclass");
    t(!!note && note.at[0] === l.at[0] && note.at[1] === l.at[1], `the new field at the label's anchor (${note && note.at} / ${l?.at})`);
    t(!!note && note.vertical === true && note.h === "right", `upright and right-justified as the net class (${note?.vertical} ${note?.h})`);
    t(l?.fields_hand_placed === true && nc && Math.abs(nc.at[0] - l.at[0] - mm(5.08)) < 100, "the label stays placed by hand, the net class where it was");
    await undo();
    const back = (await sch()).labels.find((y) => y.id === glabel.id);
    t(!back?.fields?.some((f) => f.name === "Note"), "one undo takes it away");
  });

  // ----- S3-18
  await scenario("S3-18: Do not Populate in a variant: one symbol DNP there and one not go to DNP together, then back; their own flags untouched", async (t) => {
    // the symbols are on the board's sheets: into the LED channels' one
    const root = await sch();
    const sheet = root.sheets.find((x) => /led/i.test(x.file ?? "")) ?? root.sheets[0];
    if (!t(!!sheet, "a sheet with symbols")) return;
    await select([sheet.id]);
    await run("eeschema.NavigateTool.enterSheet");
    await sleep(1500);
    const s = await sch();
    const syms = s.symbols.filter((x) => !(s.locked ?? []).includes(x.id) && !x.id.startsWith("#")).slice(0, 2);
    if (!t(syms.length === 2, `two symbols (${syms.map((x) => x.id)})`)) return;
    const added = await post({ op: "add_design_variant", name: "V18", description: "" });
    if (!t(added.ok !== false, "the variant 'V18' is added")) return;
    try {
      await post(onSheet({ op: "set_symbol_variant", variant: "V18", ids: [syms[0].id], dnp: true }));
      const sel = await until(() => (variantSelect() && [...variantSelect().options].some((o) => o.textContent === "V18") ? variantSelect() : null));
      if (!t(!!sel, "the schematic's variant choice lists 'V18'")) return;
      await pick(sel, "V18");
      t(state().schVariant === "V18" || variantSelect()?.value === "V18", `'V18' is the current variant (${variantSelect()?.value})`);
      const dnpIn = async () => {
        const v = await sch("V18");
        return syms.map((x) => v.symbols.find((y) => y.id === x.id)?.variants?.V18?.dnp ?? null);
      };
      t(JSON.stringify(await dnpIn()) === JSON.stringify([true, null]), `before: the first DNP in 'V18', the second not (${JSON.stringify(await dnpIn())})`);
      await select(syms.map((x) => x.id));
      await run("eeschema.EditorControl.setDNP");
      await sleep(1500);
      t(JSON.stringify(await dnpIn()) === JSON.stringify([true, true]), `one lacked it: both DNP in 'V18' (${JSON.stringify(await dnpIn())})`);
      const own = (await sch()).symbols.filter((y) => syms.some((x) => x.id === y.id)).map((y) => !!y.dnp);
      t(own.every((d) => d === false), `the symbols' own flags untouched (${own})`);
      await select(syms.map((x) => x.id));
      await run("eeschema.EditorControl.setDNP");
      await sleep(1500);
      const after = await dnpIn();
      t(after.every((d) => d !== true), `both had it: cleared on both (${JSON.stringify(after)})`);
    } finally {
      const sel = variantSelect();
      if (sel) await pick(sel, sel.options[0]?.textContent ?? "");
      await post({ op: "remove_design_variant", name: "V18" });
      await sleep(800);
      await leave();
    }
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
