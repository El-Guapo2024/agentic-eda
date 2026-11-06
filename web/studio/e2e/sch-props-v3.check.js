// The schematic Properties panel's rows that were read-only before UI-LEFT-2 SS11 (task-v3-schdlg): a text box's four margins, fill, colour and hyperlink, a
// directive label's font, colour and text size, an image's transform offset and Scale about it, a graphic line's length, a rectangle's Fill with KiCad's
// UI_FILL_MODE choices and a symbol's Description field. For each: select the item, edit the row through the grid the way a user does, read the edit back from
// `/api/schematic`, undo it (one step), and check the item is as it was. The items are added through `/api/cmd` first and taken away at the end.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__schPropsV3Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail });

  (async () => {
    const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
    const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
    const input = (name) => row(name)?.querySelector("[data-prop-input]");
    const lastAct = async () => (await json("/api/state")).activity[0] ?? null;
    const select = async (ids) => {
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", ids);
      await sleep(400);
    };
    const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
    const waitAct = async (before) => {
      const t0 = Date.now();
      while (Date.now() - t0 < 10000) {
        const a = await lastAct();
        if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(400); return a; }
        await sleep(50);
      }
      return null;
    };
    /** Type or pick `value` in the row's editor, as a user does, and wait for the command the grid sent. */
    const edit = async (name, value) => {
      for (let i = 0; i < 25 && !input(name); i++) await sleep(200);
      const el = input(name);
      if (!el) return `no editable row ${name}${row(name) ? ` (read-only: ${row(name).dataset.note ?? ""})` : ""}`;
      const before = await lastAct();
      if (el instanceof HTMLSelectElement) {
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      } else if (el.type === "checkbox") {
        if (el.checked === !!value) return "already";
        el.click();
      } else if (el.type === "color") {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        await sleep(60);
        el.dispatchEvent(new Event("change", { bubbles: true }));
      } else {
        el.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        await sleep(30);
        el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      }
      const a = await waitAct(before);
      if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
      return a.ok ? `ok: ${a.cmd.slice(0, 90)}` : `REFUSED: ${a.message.slice(0, 120)}`;
    };
    const scenario = async (label, ids, name, value, read, want) => {
      await select(ids);
      const was = JSON.stringify(await read());
      const r = await edit(name, value);
      const got = await read();
      const applied = r.startsWith("ok");
      const okEdit = applied && JSON.stringify(got) === JSON.stringify(want);
      if (applied) await undo();
      const back = JSON.stringify(await read());
      t(`${label}: ${name} = ${value}`, okEdit && back === was, `edit: ${r}; read ${JSON.stringify(got)} (want ${JSON.stringify(want)}); after undo ${back === was ? "as before" : back}`);
    };
    if (__eda.state().tab !== "schematic") {
      [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
      await sleep(1500);
    }
    document.querySelector('button[aria-label="Show Properties"]')?.click();

    // the items: a text box, a directive label, an image, a graphic line and a rectangle, where the sheet has room
    const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    const mm = (v) => Math.round(v * 1e6);
    const before = await json("/api/schematic");
    const known = new Set((before.graphics ?? []).map((g) => g.id).concat((before.lines ?? []).map((l) => l.id)));
    const adds = [
      { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "text_box", start: { x: mm(20), y: mm(200) }, end: { x: mm(60), y: mm(215) }, text: "SS11 box", size_nm: mm(1.27) } } },
      { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "directive", at: { x: mm(80), y: mm(200) }, pin_length_nm: mm(2.54), netclass: "HV" } } },
      { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "image", at: { x: mm(110), y: mm(205) }, scale_ppm: 20_000_000, data: PNG } } },
      { op: "add_sch_line", pts: [{ x: mm(130), y: mm(200) }, { x: mm(140), y: mm(200) }] },
      { op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "rectangle", start: { x: mm(150), y: mm(200) }, end: { x: mm(160), y: mm(210) } } } },
    ];
    let added = 0;
    for (const c of adds) if ((await post(c)).ok !== false) added++;
    await sleep(900);
    const sch = await json("/api/schematic");
    const fresh = (pred) => (sch.graphics ?? []).find((g) => !known.has(g.id) && pred(g));
    const box = fresh((g) => g.shape.type === "text_box");
    const flag = fresh((g) => g.shape.type === "directive");
    const img = fresh((g) => g.shape.type === "image");
    const rect = fresh((g) => g.shape.type === "rectangle");
    const line = (sch.lines ?? []).find((l) => !known.has(l.id));
    t("the items were added", added === adds.length && box && flag && img && rect && line, `${added} of ${adds.length}`);
    const graphicOf = async (id) => (await json("/api/schematic")).graphics.find((g) => g.id === id);
    const attrsOf = async (id) => (await json("/api/schematic")).graphic_text_attrs?.[id] ?? {};
    const rgba = (c) => (c ? [c.r, c.g, c.b, c.a] : null);

    if (box) {
      // `SetMarginLeft` .. : each side its own
      await scenario("text box", [box.id], "Margin Left", "2", async () => (await graphicOf(box.id)).shape.margins_nm?.[0] ?? null, mm(2));
      // `UI_FILL_MODE`: the hatches and Solid
      await select([box.id]);
      const choices = [...(input("Fill")?.options ?? [])].map((o) => o.textContent.trim());
      t("text box: Fill offers None, Solid, Hatch, Reverse Hatch, Cross-hatch", JSON.stringify(choices) === JSON.stringify(["None", "Solid", "Hatch", "Reverse Hatch", "Cross-hatch"]), JSON.stringify(choices));
      await scenario("text box", [box.id], "Fill", "hatch", async () => (await graphicOf(box.id)).fill ?? "none", "hatch");
      await scenario("text box", [box.id], "Color", "#00ff00", async () => rgba((await attrsOf(box.id)).color), [0, 255, 0, 255]);
      await scenario("text box", [box.id], "Hyperlink", "https://kicad.org", async () => (await attrsOf(box.id)).href ?? null, "https://kicad.org");
    }
    if (flag) {
      await scenario("directive label", [flag.id], "Text Size", "2", async () => (await graphicOf(flag.id)).shape.text_size_nm ?? 0, mm(2));
      await scenario("directive label", [flag.id], "Font", "KiCad Font", async () => (await attrsOf(flag.id)).font ?? null, "KiCad Font");
      await scenario("directive label", [flag.id], "Color", "#ff00ff", async () => rgba((await attrsOf(flag.id)).color), [255, 0, 255, 255]);
    }
    if (img) {
      await scenario("image", [img.id], "Transform Offset X", "1", async () => (await json("/api/schematic")).image_offsets?.[img.id] ?? null, { x: mm(1), y: 0 });
      // with an offset, Scale scales about it (`REFERENCE_IMAGE::scaleBy`): the centre moves away from it, the offset scales; one undo step takes both back
      await select([img.id]);
      const set = await edit("Transform Offset X", "2");
      const after = async () => {
        const s = await json("/api/schematic");
        const g = s.graphics.find((x) => x.id === img.id);
        return [g.shape.at.x, g.shape.scale_ppm ?? 1_000_000, s.image_offsets?.[img.id]?.x ?? 0];
      };
      const was = await after();
      const r = await edit("Scale", String((was[1] / 1e6) * 2));
      const got = await after();
      const want = [was[0] + was[2] - 2 * was[2], was[1] * 2, was[2] * 2];
      await undo();
      const back = await after();
      t("image: Scale about the transform origin", set.startsWith("ok") && r.startsWith("ok") && JSON.stringify(got) === JSON.stringify(want) && JSON.stringify(back) === JSON.stringify(was), `${set}; ${r}; got ${JSON.stringify(got)} want ${JSON.stringify(want)}; undo ${JSON.stringify(back)} (was ${JSON.stringify(was)})`);
      if (set.startsWith("ok")) await undo();
    }
    if (line) {
      // `SCH_LINE::SetLength`: the end goes along the line
      await scenario("graphic line", [line.id], "Length", "5", async () => (await json("/api/schematic")).lines.find((l) => l.id === line.id).pts.at(-1), [mm(135), mm(200)]);
    }
    if (rect) {
      // "Solid" on a shape of the sheet: `SCH_SHAPE::SetFilled( true )`, filled with its colour
      await scenario("rectangle", [rect.id], "Fill", "solid", async () => (await graphicOf(rect.id)).fill ?? "none", "color");
    }
    for (let i = 0; i < added; i++) await undo();
    const end = await json("/api/schematic");
    t("everything added is gone again", (end.graphics ?? []).length === (before.graphics ?? []).length && (end.lines ?? []).length === (before.lines ?? []).length);
    // a symbol's Description (`FIELD_T::DESCRIPTION`): every symbol has the row. On the root, or on the first sheet below it.
    const here = () => json(`/api/schematic${__eda.state().sheetPath.length ? `?sheet=${__eda.state().sheetPath.map(encodeURIComponent).join("/")}` : ""}`);
    let sym = (sch.symbols ?? []).find((s) => !(sch.locked ?? []).includes(s.id));
    if (!sym && (sch.sheets ?? [])[0]) {
      await select([sch.sheets[0].id]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1500);
      const below = await here();
      sym = (below.symbols ?? []).find((s) => !(below.locked ?? []).includes(s.id));
    }
    if (sym) {
      await select([sym.id]);
      t("symbol: a Description row", !!row("Description"), `rows: ${[...document.querySelectorAll(".prop-row")].map((r) => r.dataset.prop).join(", ")}`);
      await scenario("symbol", [sym.id], "Description", "SS11 description", async () => (await here()).symbols.find((s) => s.id === sym.id && s.unit === sym.unit).fields?.find((f) => f.name === "Description")?.text ?? null, "SS11 description");
    } else t("symbol", false, "the sheet has no unlocked symbol");
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
