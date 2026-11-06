// The Properties panel's rows that came with UI-LEFT P2 and S7 (task-u1-props): for each kind, select the item, edit one of the new rows through the grid the
// way a user does, read the edit back from the board or the sheet, undo it, and check the item is as it was. Also: a read-only row says why in its tooltip.
// On a routed board it adds a text and a copper rectangle (PCB) and a text and a label (schematic) where the board has none, and takes them away at the end.
// run-all-checks: board=mcu30-routed minutes=12
(async () => {
  const results = [];
  const check = (window.__propsRowsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
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
      await sleep(350);
    };
    const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
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
      if (!el) return `no editable row ${name}`;
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
    /**
     * One scenario: select `ids`, edit `name` to `value`, check `read()` (what the board or sheet says of the item) is `want`, undo, check `read()` is what it
     * was before.
     */
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
    const tab = async (name) => {
      if (__eda.state().tab !== name) {
        [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
        await sleep(1500);
      }
    };
    document.querySelector('button[aria-label="Show Properties"]')?.click();

    // ---------------------------------------------------------------------------------------------------------------- the board
    await tab("pcb");
    let s = await json("/api/state");
    const added = [];
    if (!(s.drawings?.texts ?? []).length) {
      const r = await post({ op: "add_text", text: { content: "PROPS", at: { x: 20_000_000, y: 30_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false } });
      if (r.ok !== false) added.push("text");
    }
    const r2 = await post({ op: "add_shape", shape: { kind: "rect", layer: "F.Cu", stroke_width: 100_000, filled: false, start: { x: 4_000_000, y: 30_000_000 }, end: { x: 9_000_000, y: 33_000_000 } } });
    if (r2.ok !== false) added.push("rect");
    await sleep(600);
    s = await json("/api/state");
    const via = s.routing?.vias?.[0];
    const track = (s.routing?.tracks ?? []).find((x) => !x.arc_mid && (x.layer === "F.Cu" || x.layer === "B.Cu"));
    const text = s.drawings?.texts?.[0];
    const rect = (s.drawings?.shapes ?? []).filter((x) => x.kind === "rect" && x.layer === "F.Cu").at(-1);
    const part = s.parts.find((p) => p.placed && p.pads?.length);
    const pad = part?.pads?.[0];
    const viaOf = async (id) => (await json("/api/state")).routing.vias.find((v) => v.id === id);
    const trackOf = async (id) => (await json("/api/state")).routing.tracks.find((v) => v.id === id);
    const textOf = async (id) => (await json("/api/state")).drawings.texts.find((v) => v.id === id);
    const shapeOf = async (id) => (await json("/api/state")).drawings.shapes.find((v) => v.id === id);
    const padOf = async (ref, num) => (await json("/api/state")).parts.find((p) => p.ref === ref).pads.find((p) => p.num === num);

    if (via) {
      await scenario("via", [via.id], "Front tenting", "yes", async () => (await viaOf(via.id)).props?.front_tenting ?? null, true);
      await scenario("via", [via.id], "Backdrill Mode", "bottom", async () => (await viaOf(via.id)).props?.backdrill?.start ?? null, "B.Cu");
      // The Teardrops rows are the via's own parameters, writeable (`BOARD_CONNECTED_ITEM_DESC`: `supportsTeardrops` is their only AvailableFunc, false only on a
      // legacy-teardrop board; no WriteableFunc): the checkbox turned over gives the via its own parameters (`set_teardrop_params`, UI-LEFT P3), undo takes them away.
      await select([via.id]);
      const tdOn = input("Enable Teardrops")?.checked;
      if (tdOn === undefined) t("via: Enable Teardrops is a writeable row", false, row("Enable Teardrops") ? `read-only: ${row("Enable Teardrops").dataset.note ?? ""}` : "no row");
      else await scenario("via", [via.id], "Enable Teardrops", !tdOn, async () => (await viaOf(via.id)).teardrops?.enabled ?? null, !tdOn);
    } else t("via", false, "the board has no via");
    if (track) await scenario("track", [track.id], "Soldermask", true, async () => !!(await trackOf(track.id)).mask, true);
    else t("track", false, "the board has no track on outer copper");
    if (text) {
      await scenario("text", [text.id], "Italic", true, async () => !!(await textOf(text.id)).italic, true);
      await scenario("text", [text.id], "Vertical Justification", "bottom", async () => (await textOf(text.id)).valign ?? "center", "bottom");
      // `SetTextWidth` alone: the height stays
      await scenario("text", [text.id], "Width", "1.5", async () => { const x = await textOf(text.id); return [x.width ?? x.size, x.size]; }, [1_500_000, text.size]);
    } else t("text", false, "the board has no text");
    if (rect) {
      await scenario("copper rectangle", [rect.id], "Corner Radius", "0.5", async () => (await shapeOf(rect.id)).corner_radius ?? 0, 500_000);
      await scenario("copper rectangle", [rect.id], "Soldermask", true, async () => !!(await shapeOf(rect.id)).solder_mask, true);
      // "Line Style" is the shape's own since UI-LEFT P5 (`set_shape_style`'s `line_style`): it was read-only
      await scenario("copper rectangle", [rect.id], "Line Style", "dash", async () => (await shapeOf(rect.id)).dash ?? "solid", "dash");
    } else t("copper rectangle", false, "no rectangle was added");
    if (pad) {
      await scenario("pad", [`${part.ref}.${pad.num}`], "Soldermask Margin Override", "0.05", async () => (await padOf(part.ref, pad.num)).mask_margin ?? null, 50_000);
      await scenario("pad", [`${part.ref}.${pad.num}`], "Clearance Override", "0.3", async () => (await json("/api/state")).parts.find((p) => p.ref === part.ref).zone?.pads?.find((x) => x.num === pad.num)?.clearance ?? null, 300_000);
    } else t("pad", false, "no placed footprint with pads");
    if (part) {
      await select([part.ref]);
      t("footprint: KiCad's panel has no Component Type row", !row("Component Type") && !!row("Solderpaste Margin Override"), `rows: ${[...document.querySelectorAll(".prop-row")].map((r) => r.dataset.prop).join(", ")}`);
    }
    for (let i = 0; i < added.length; i++) await undo();

    // ------------------------------------------------------------------------------------------------------------- the schematic
    await tab("schematic");
    let sch = await json("/api/schematic");
    const addedSch = [];
    if (!(sch.texts ?? []).length) {
      const r = await post({ op: "add_sch_text", content: "NOTE", at: { x: 25_400_000, y: 25_400_000 }, angle_millideg: 0, size_nm: 1_270_000 });
      if (r.ok !== false) addedSch.push("text");
    }
    if (!(sch.labels ?? []).length) {
      const r = await post({ op: "add_label", net: "PROPS_NET", at: { x: 50_800_000, y: 25_400_000 }, kind: { scope: "local" } });
      if (r.ok !== false) addedSch.push("label");
    }
    await sleep(800);
    sch = await json("/api/schematic");
    const stext = sch.texts?.[0];
    const label = sch.labels?.[0];
    const wire = (sch.wires ?? []).find((w) => !w.bus && w.pts.length === 2 && !(sch.locked ?? []).includes(w.id));
    const sheetItem = (sch.sheets ?? [])[0];
    // the sheet in view (a symbol may be on a sheet below the root)
    const here = () => json(`/api/schematic${__eda.state().sheetPath.length ? `?sheet=${__eda.state().sheetPath.map(encodeURIComponent).join("/")}` : ""}`);
    const stextOf = async (id) => (await here()).texts.find((x) => x.id === id);
    const labelOf = async (id) => (await here()).labels.find((x) => x.id === id);
    const wireOf = async (id) => (await here()).wires.find((x) => x.id === id);
    const symOf = async (id) => (await here()).symbols.find((x) => x.id === id);
    const sheetOf = async (id) => (await here()).sheets.find((x) => x.id === id);
    if (stext) {
      await scenario("free text", [stext.id], "Bold", true, async () => !!(await stextOf(stext.id)).bold, true);
      await scenario("free text", [stext.id], "Hyperlink", "https://kicad.org", async () => (await stextOf(stext.id)).href ?? null, "https://kicad.org");
    } else t("free text", false, "the sheet has no text");
    if (label) {
      await scenario("label", [label.id], "Text Size", "2", async () => (await labelOf(label.id)).size_nm ?? 1_270_000, 2_000_000);
      await scenario("label", [label.id], "Italic", true, async () => !!(await labelOf(label.id)).italic, true);
    } else t("label", false, "the sheet has no label");
    if (wire) {
      const end = wire.pts[1];
      await scenario("wire", [wire.id], "End X", String((end[0] + 1_270_000) / 1e6), async () => (await wireOf(wire.id))?.pts.at(-1) ?? null, [end[0] + 1_270_000, end[1]]);
    } else t("wire", false, "the sheet has no two-point wire");
    if (sheetItem) {
      // a sheet's fields are rows (`SCH_SHEET_FIELD_PROPERTY`): its Sheetname is its name
      await scenario("sheet", [sheetItem.id], "Sheetname", "Renamed", async () => (await sheetOf(sheetItem.id)).name, "Renamed");
    }
    for (let i = 0; i < addedSch.length; i++) await undo();
    addedSch.length = 0;
    // a symbol: on the root, or on the first sheet below it
    let sym = (sch.symbols ?? []).find((x) => !(sch.locked ?? []).includes(x.id));
    if (!sym && sheetItem) {
      await select([sheetItem.id]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1500);
      const below = await here();
      sym = (below.symbols ?? []).find((x) => !(below.locked ?? []).includes(x.id));
    }
    if (sym) {
      await scenario("symbol", [sym.id], "Exclude From Position Files", true, async () => !!(await symOf(sym.id)).exclude_from_pos_files, true);
      await select([sym.id]);
      t("symbol: the pin display is read-only and says why", row("Show Pin Number")?.classList.contains("readonly") && /library symbol/.test(row("Show Pin Number")?.dataset.note ?? ""), row("Show Pin Number")?.dataset.note ?? "no row");
      t("symbol: its Footprint and Datasheet fields are rows of Fields", !!row("Footprint") && !!row("Datasheet"), `rows: ${[...document.querySelectorAll(".prop-row")].map((r) => r.dataset.prop).join(", ")}`);
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
