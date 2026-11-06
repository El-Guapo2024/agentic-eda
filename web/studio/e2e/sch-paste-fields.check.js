// UI-LEFT-2 SS8 (task-v3-schdlg): Copy and Paste carry every field of a symbol where and as it was -- `SCH_IO_KICAD_SEXPR::saveSymbol` writes each `SCH_FIELD`
// with its own `(at ..)`, angle, look and visibility, and `SCH_EDITOR_CONTROL::Paste` keeps them; only a symbol whose fields were autoplaced is marked
// `(fields_autoplaced yes)`. On the schematic of mcu30: a symbol's Value is moved and turned by hand, a user field is added, shown and placed; Copy, Paste and a
// click drop the copy; each field of the copy is where the original's is, moved by the drop, the user field shown, and the copy is not autoplaced; one Undo
// takes the paste away. A symbol left autoplaced is pasted autoplaced. Everything is undone at the end.
// run-all-checks: board=mcu30 minutes=8
(async () => {
  const results = [];
  const check = (window.__schPasteFieldsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail });

  (async () => {
    // the sheet in view (mcu30's symbols are on the sheets below its root)
    const path = () => __eda.state().sheetPath.join("/");
    const sch = () => json(`/api/schematic${path() ? `?sheet=${__eda.state().sheetPath.map(encodeURIComponent).join("/")}` : ""}`);
    const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: path() ? { op: "on_sheet", sheet: path(), cmd } : cmd, strict: false }) });
    const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
    const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
    const container = () => document.querySelector(".pcb-canvas-container");
    const fire = (type, x, y, extra = {}) => {
      const rect = container().getBoundingClientRect();
      (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
    };
    const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
    const fitTo = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(600); await select([]); };
    /** Copy `ids`, Paste, and drop the copy `dx` to the right of where `ref` is. Returns the symbols the paste made. */
    const copyPaste = async (ids, ref, dx) => {
      const s0 = await sch();
      await select(ids);
      await __eda.run("common.Interactive.copy");
      await sleep(900);
      await __eda.run("common.Interactive.paste");
      await sleep(1500);
      const at = s0.symbols.find((x) => x.id === ref).at;
      const [px, py] = screenOf(at[0] + dx, at[1]);
      fire("pointermove", px, py); await sleep(900);
      fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1800);
      const s1 = await sch();
      return s1.symbols.filter((x) => !s0.symbols.some((o) => o.id === x.id && o.unit === x.unit));
    };
    /** The fields as `name -> [at relative to the symbol, visible, vertical, h]`. */
    const rel = (sym) => Object.fromEntries((sym.fields ?? []).filter((f) => f.name !== "Description").map((f) => [f.name, [f.at[0] - sym.at[0], f.at[1] - sym.at[1], f.visible, !!f.vertical, f.h]]));
    const near = (a, b) => Object.keys(a).length === Object.keys(b).length && Object.keys(a).every((k) => b[k] && Math.abs(a[k][0] - b[k][0]) <= 100 && Math.abs(a[k][1] - b[k][1]) <= 100 && a[k].slice(2).join() === b[k].slice(2).join());

    if (__eda.state().tab !== "schematic") {
      [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
      await sleep(1500);
    }
    let s = await sch();
    if (!s.symbols.length && s.sheets?.length) {
      await select([s.sheets[0].id]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1500);
      s = await sch();
    }
    const locked = new Set(s.locked ?? []);
    const pick = s.symbols.filter((x) => !locked.has(x.id) && (x.fields ?? []).some((f) => f.name === "Value" && f.visible) && s.symbols.filter((o) => o.id === x.id).length === 1);
    const sym = pick[0];
    const other = pick.find((x) => x.id !== sym?.id);
    if (!sym) {
      t("the sheet has a symbol with a shown Value", false);
    } else {
      // the setup: R's Value moved and turned by hand, a user field shown where it is put (three steps, undone at the end)
      const value = sym.fields.find((f) => f.name === "Value");
      const steps = [
        { op: "sch_edit", verb: "edit_field", id: value.id, at: { x: value.at[0] + 5_080_000, y: value.at[1] + 2_540_000 }, vertical: !value.vertical },
        { op: "sch_edit", verb: "add_user_field", id: sym.id, name: "SS8_MPN", text: "PART-0001" },
        { op: "sch_edit", verb: "edit_field", id: `fld:${sym.id}:SS8_MPN`, visible: true, at: { x: sym.at[0] - 7_620_000, y: sym.at[1] + 10_160_000 } },
      ];
      let made = 0;
      for (const c of steps) { const r = await post(c); if (r.ok !== false) made++; else t(`setup: ${c.verb}`, false, JSON.stringify(r).slice(0, 200)); }
      await sleep(900);
      s = await sch();
      const orig = s.symbols.find((x) => x.id === sym.id);
      t("setup: the Value placed by hand takes the symbol out of the autoplaced ones", made === 3 && !orig.fields_autoplaced && orig.fields.some((f) => f.name === "SS8_MPN" && f.visible), `fields_autoplaced ${orig.fields_autoplaced}`);
      await fitTo([sym.id]);
      const pasted = await copyPaste([sym.id], sym.id, 50_800_000);
      const copy = pasted[0];
      t("Paste made one copy", pasted.length === 1 && copy, `${pasted.map((x) => x.id)}`);
      if (copy) {
        t("every field of the copy is where the original's is, as shown and turned as it was (`saveSymbol` / `Paste`)", near(rel(copy), rel(orig)), `copy ${JSON.stringify(rel(copy))} original ${JSON.stringify(rel(orig))}`);
        t("the user field is shown on the copy", copy.fields.some((f) => f.name === "SS8_MPN" && f.visible && f.text === "PART-0001"));
        t("the copy is not autoplaced again (no `(fields_autoplaced yes)` on the clipboard)", !copy.fields_autoplaced, `fields_autoplaced ${copy.fields_autoplaced}`);
        await undo();
        const after = await sch();
        t("one Undo takes the paste away", !after.symbols.some((x) => x.id === copy.id));
      }
      // a symbol whose fields Autoplace Fields placed is pasted autoplaced, its fields where they are
      if (other) {
        await fitTo([other.id]);
        const o = (await sch()).symbols.find((x) => x.id === other.id);
        const p2 = await copyPaste([other.id], other.id, 50_800_000);
        const c2 = p2[0];
        t("a symbol never placed by hand is pasted with its fields where they were, autoplaced when it was", !!c2 && (!o.fields_autoplaced || !!c2.fields_autoplaced) && near(rel(c2), rel(o)), c2 ? `fields_autoplaced ${o.fields_autoplaced} -> ${c2.fields_autoplaced}; ${JSON.stringify(rel(c2))} vs ${JSON.stringify(rel(o))}` : "no copy");
        if (c2) await undo();
      }
      for (let i = 0; i < made; i++) await undo();
      const end = (await sch()).symbols.find((x) => x.id === sym.id);
      t("the setup is undone", !end.fields.some((f) => f.name === "SS8_MPN") && JSON.stringify(end.fields.find((f) => f.name === "Value").at) === JSON.stringify(value.at));
    }
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
