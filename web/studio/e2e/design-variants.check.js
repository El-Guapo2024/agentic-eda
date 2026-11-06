// Design variants (UI-LEFT S2, task-u1-variants; KiCad 25ec2335's `SCH_EDIT_FRAME` variant selector and dialogs, `DIALOG_SYMBOL_PROPERTIES` and
// `DIALOG_SYMBOL_FIELDS_TABLE` in a variant, the output dialogs' "Design variant:"). On a placed board it:
//   1. adds two variants through the schematic toolbar's choice ("Add New Design Variant..." -> New Design Variant), each becoming the current one;
//   2. in the first, sets Do not populate and a Value of its own on one symbol through Symbol Properties (the title names the variant), and checks the
//      symbol's own flag and Value did not change;
//   3. switches the current variant back and forth and checks the sheet follows: the Properties panel's rows and the canvas's picture;
//   4. checks the Symbol Fields Table lists the variants and highlights the variant's own cells;
//   5. exports a BOM (the Fields Table's preview and kicad-cli's file) and a placement file (the Generate Placement Files dialog, its "Design variant:"
//      starting at the board's current one) for each variant, and checks the DNP symbol and its Value follow the variant;
//   6. removes the second variant through Tools > Variants > Remove Design Variant..., and at the end the first through the API, so the board is as it was.
// It answers through `window.__designVariantsCheck`: { results: [{ name, ok, detail }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__designVariantsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => (results.push({ name, ok: !!ok, detail: typeof detail === "string" ? detail : JSON.stringify(detail) }), !!ok);
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };

  // What the dialogs ask the server, captured as it comes back.
  const replies = [];
  if (!window.__designVariantsFetch) {
    const real = window.fetch.bind(window);
    window.__designVariantsFetch = real;
    window.fetch = async (input, init) => {
      const r = await real(input, init);
      const url = typeof input === "string" ? input : input?.url ?? "";
      const m = /\/api\/(fab\/[a-z0-9_]+|sch\/[a-z_]+)(\?|$)/.exec(url);
      if (m && (init?.method ?? "GET") === "POST") {
        let req = null;
        try { req = init?.body ? JSON.parse(init.body) : null; } catch { /* not JSON */ }
        r.clone().json().then((body) => replies.push({ route: m[1], req, body })).catch(() => undefined);
      }
      return r;
    };
  }

  (async () => {
    const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
    const postJson = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    const dialog = () => [...document.querySelectorAll(".dialog")].filter((d) => d.offsetParent !== null).pop() ?? null;
    const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
    const footerButton = (re) => [...(dialog()?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
    const set = async (el, value) => {
      const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
      await sleep(150);
    };
    const pickIndex = async (sel, i) => {
      sel.selectedIndex = i;
      sel.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(300);
    };
    const tab = async (name) => {
      if (__eda.state().tab !== name) {
        [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
        await sleep(1500);
      }
    };
    const select = async (ids) => {
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", ids);
      await sleep(400);
    };
    const schSelect = () => document.querySelector("[data-testid=sch-variant-select]");
    const pcbSelect = () => document.querySelector("[data-testid=pcb-variant-select]");
    const here = async () => json(`/api/schematic${__eda.state().sheetPath.length ? `?sheet=${__eda.state().sheetPath.join("/")}` : ""}`);
    const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
    const rowInput = (name) => row(name)?.querySelector("[data-prop-input]");
    const canvasShot = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].find((c) => c.offsetParent !== null)?.toDataURL() ?? "";

    await tab("schematic");
    document.querySelector('button[aria-label="Show Properties"]')?.click();
    // a previous run's variants go first
    for (const v of (await here()).variants ?? []) if (v.name === "Lite" || v.name === "Pro") await post({ op: "remove_design_variant", name: v.name });
    await sleep(800);

    // ------------------------------------------------------------------------------------------------------ 1. two variants through the toolbar
    const addThroughToolbar = async (name, description) => {
      const sel = await until(() => schSelect());
      if (!sel) return "no variant choice on the toolbar";
      const last = sel.options.length - 1;
      if (sel.options[last]?.textContent !== "Add New Design Variant...") return `the last entry is ${sel.options[last]?.textContent}`;
      await pickIndex(sel, last);
      const form = await until(() => document.querySelector("[data-testid=new-variant-dialog]"));
      if (!form) return "the New Design Variant dialog did not open";
      if (header() !== "New Design Variant") return `the dialog is "${header()}"`;
      await set(document.querySelector("[data-testid=new-variant-name]"), `  ${name} `);
      await set(document.querySelector("[data-testid=new-variant-description]"), description);
      footerButton(/^OK$/)?.click();
      const ok = await until(async () => ((await here()).variants ?? []).some((v) => v.name === name), 8000);
      return ok ? "ok" : "the variant was not added";
    };
    let r = await addThroughToolbar("Lite", "No radio");
    t("Add New Design Variant... adds 'Lite' (the name trimmed)", r === "ok", r);
    await sleep(500);
    t("the new variant is the current one", schSelect()?.value === "Lite", schSelect()?.value);
    r = await addThroughToolbar("Pro", "");
    t("a second variant 'Pro'", r === "ok", r);
    await sleep(500);
    let sch = await here();
    t("the variants and their descriptions", JSON.stringify((sch.variants ?? []).map((v) => [v.name, v.description])) === JSON.stringify([["Lite", "No radio"], ["Pro", ""]]), sch.variants);
    t("the choice lists the default, the variants, '---' and the Add entry", JSON.stringify([...(schSelect()?.options ?? [])].map((o) => o.textContent)) === JSON.stringify(["< Default >", "Lite", "Pro", "---", "Add New Design Variant..."]), [...(schSelect()?.options ?? [])].map((o) => o.textContent));
    // a name taken (in another case) is refused with KiCad's message
    r = await addThroughToolbar("LITE", "");
    const errs = __eda.errors().map((e) => e.message);
    t("'LITE' is refused: Variant 'Lite' already exists.", r !== "ok" && errs.some((m) => m.includes("Variant 'Lite' already exists.")), { r, errs: errs.slice(-3) });

    // ------------------------------------------------------------------------------------------------------ 2. DNP and a Value of its own in 'Lite'
    let sym = (sch.symbols ?? []).find((x) => !(sch.locked ?? []).includes(x.id));
    if (!sym && (sch.sheets ?? []).length) {
      await select([sch.sheets[0].id]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1500);
      sch = await here();
      sym = (sch.symbols ?? []).find((x) => !(sch.locked ?? []).includes(x.id));
    }
    if (!t("a symbol to work on", !!sym, "no unlocked symbol on the root or its first sheet")) throw new Error("no symbol");
    const baseValue = sym.value ?? "";
    await pickIndex(schSelect(), [...schSelect().options].findIndex((o) => o.textContent === "Lite"));
    t("'Lite' chosen on the toolbar", schSelect().value === "Lite", schSelect().value);
    await select([sym.id]);
    await __eda.run("eeschema.InteractiveEdit.properties");
    const title = await until(() => document.querySelector("[data-testid=symbol-properties-title]"));
    t("Symbol Properties names the variant", title?.textContent === "Symbol Properties - Lite Design Variant", title?.textContent);
    if (title) {
      // the fields grid's Value cell (Symbol Properties is a grid since task-u2-symprops)
      const valueInput = dialog().querySelector('input[aria-label="Value"]');
      await set(valueInput, "VARIANT-VAL");
      const dnpBox = dialog().querySelector("[data-testid=symbol-attr-dnp]");
      if (dnpBox && !dnpBox.checked) dnpBox.click();
      await sleep(150);
      footerButton(/^OK$/)?.click();
      await sleep(1200);
    }
    sch = await here();
    let mine = sch.symbols.find((x) => x.id === sym.id);
    t("the symbol's record of 'Lite': DNP and its Value", mine?.variants?.Lite?.dnp === true && mine?.variants?.Lite?.fields?.Value === "VARIANT-VAL", mine?.variants);
    t("the symbol's own flag and Value are as they were", !mine?.dnp && (mine?.value ?? "") === baseValue, { dnp: mine?.dnp, value: mine?.value });

    // ------------------------------------------------------------------------------------------------------ 3. the sheet follows the current variant
    await select([sym.id]);
    await sleep(400);
    const dnpRow = () => rowInput("Do not Populate");
    const valueRow = () => rowInput("Value");
    t("in 'Lite' the Properties panel shows the variant's DNP and Value", dnpRow()?.checked === true && valueRow()?.value === "VARIANT-VAL", { dnp: dnpRow()?.checked, value: valueRow()?.value });
    await __eda.run("common.InteractiveSelection.clear");
    await sleep(600);
    const liteShot = canvasShot();
    await pickIndex(schSelect(), 0);
    await sleep(600);
    const defaultShot = canvasShot();
    t("the canvas draws another picture in the default variant", liteShot !== "" && liteShot !== defaultShot, `${liteShot.length} / ${defaultShot.length} bytes`);
    await pickIndex(schSelect(), [...schSelect().options].findIndex((o) => o.textContent === "Lite"));
    await sleep(600);
    t("and the same picture again in 'Lite'", canvasShot() === liteShot);
    await pickIndex(schSelect(), 0);
    await select([sym.id]);
    t("in the default variant the panel shows the symbol's own values", dnpRow()?.checked === false && (valueRow()?.value ?? "") === baseValue, { dnp: dnpRow()?.checked, value: valueRow()?.value });

    // ------------------------------------------------------------------------------------------------------ 4. the Symbol Fields Table in a variant
    await __eda.run("eeschema.EditorControl.editSymbolFields");
    const list = await until(() => document.querySelector("[data-testid=fields-table-variant-list]"));
    if (t("the Fields Table has the 'Schematic variants:' list", !!list && JSON.stringify([...list.options].map((o) => o.value)) === JSON.stringify(["< Default >", "Lite", "Pro"]), list ? [...list.options].map((o) => o.value) : null)) {
      t("the default selected, Delete disabled for it", list.value === "< Default >" && document.querySelector("[data-testid=fields-table-delete-variant]")?.disabled === true);
      await set(list, "Lite");
      await until(() => document.querySelector("[data-override='1']"), 6000);
      t("in 'Lite' the variant's own cells are highlighted", !!document.querySelector("[data-override='1']") && schSelect()?.value === "Lite", { cells: document.querySelectorAll("[data-override='1']").length, current: schSelect()?.value });
      await set(list, "< Default >");
      await sleep(800);
      t("none in the default", !document.querySelector("[data-override='1']"));
    }
    footerButton(/^Close$/)?.click();
    await sleep(400);

    // ------------------------------------------------------------------------------------------------------ 5. a BOM and a placement file per variant
    const ft = await postJson("/api/sch/fields_table", {});
    const spec = { ...ft.spec, exclude_dnp: false };
    const fmt = { name: "CSV", field_delimiter: ",", string_delimiter: '"', ref_delimiter: ",", ref_range_delimiter: "", keep_tabs: false, keep_line_breaks: false };
    const refIn = (text) => new RegExp(`(^|[",])${sym.id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}([",]|$)`, "m").test(text);
    for (const v of ["", "Lite", "Pro"]) {
      const shown = await postJson("/api/sch/bom_export", { spec, fmt, preview: true, ...(v ? { variant: v } : {}) });
      const lite = v === "Lite";
      t(`BOM preview of ${v || "the default"}: the Value ${lite ? "is" : "is not"} the variant's`, shown.ok && shown.text.includes("VARIANT-VAL") === lite, shown.message ?? `${(shown.text ?? "").length} bytes`);
      const noDnp = await postJson("/api/sch/bom_export", { spec: { ...spec, exclude_dnp: true }, fmt, preview: true, ...(v ? { variant: v } : {}) });
      t(`BOM of ${v || "the default"} without DNP symbols: ${sym.id} ${lite ? "left out" : "listed"}`, noDnp.ok && refIn(noDnp.text) === !lite, noDnp.message ?? "");
      const file = await postJson("/api/sch/bom", { spec, fmt, path: `export/bom-${v || "default"}.csv`, ...(v ? { variant: v } : {}) });
      t(`kicad-cli's BOM file for ${v || "the default"}`, file.ok === true, file.message ?? file.files);
    }
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    await tab("pcb");
    const pcb = await until(() => pcbSelect());
    t("the board editor has its own variant choice (the names only)", !!pcb && JSON.stringify([...pcb.options].map((o) => o.value)) === JSON.stringify(["< Default >", "Lite", "Pro"]), pcb ? [...pcb.options].map((o) => o.value) : null);
    const counts = {};
    for (const v of ["", "Lite"]) {
      if (pcb) await set(pcbSelect(), v || "< Default >");
      await __eda.run("pcbnew.EditorControl.generatePosFile");
      await until(() => header() === "Generate Placement Files");
      const choice = dialog()?.querySelector("[data-testid=pos-variant]");
      t(`Generate Placement Files starts at the board's variant (${v || "the default"})`, choice?.value === (v || "< Default >"), choice?.value);
      const radio = dialog()?.querySelectorAll('input[name="pos-format"]')[0];
      radio?.click();
      const exclude = dialog()?.querySelector("[data-testid=pos-exclude-dnp]");
      if (exclude && !exclude.checked) exclude.click();
      await sleep(200);
      const n = replies.length;
      footerButton(/^Generate/)?.click();
      const rep = await until(() => replies.slice(n).find((x) => x.route === "fab/pos"), 120000);
      t(`the placement request names the variant (${v || "the default"})`, (rep?.req?.variant ?? "") === v, rep?.req);
      const m = /Component count: (\d+)/.exec(rep?.body?.message ?? "");
      counts[v || "default"] = m ? Number(m[1]) : null;
      footerButton(/^(Close|Cancel)$/)?.click();
      await sleep(400);
    }
    t("the DNP symbol of 'Lite' is left out of its placement file", counts.default != null && counts.Lite === counts.default - 1, counts);
    if (pcb) await set(pcbSelect(), "< Default >");
    await tab("schematic");

    // ------------------------------------------------------------------------------------------------------ 6. Remove Design Variant...
    await __eda.run("eeschema.EditorControl.removeVariant");
    const chooser = await until(() => (header() === "Remove Design Variant" ? dialog().querySelector("[data-testid=variant-choice]") : null));
    if (t("Remove Design Variant... asks which variant", !!chooser && /Select variant name to remove:/.test(dialog().textContent), header())) {
      t("its choices leave the default out", JSON.stringify([...chooser.options].map((o) => o.value)) === JSON.stringify(["Lite", "Pro"]));
      await set(chooser, "Pro");
      footerButton(/^OK$/)?.click();
      const gone = await until(async () => !((await here()).variants ?? []).some((v) => v.name === "Pro"), 8000);
      t("'Pro' is removed, 'Lite' stays", gone && ((await here()).variants ?? []).map((v) => v.name).join() === "Lite");
      await sleep(400);
      t("the choice no longer lists it", ![...(schSelect()?.options ?? [])].some((o) => o.textContent === "Pro"));
    }
    // the board as it was: 'Lite' (and the symbol's record of it) goes too
    await post({ op: "remove_design_variant", name: "Lite" });
    await sleep(800);
    const after = await here();
    t("at the end the board has no variants left", !(after.variants ?? []).length && !(after.symbols ?? []).some((s) => s.variants && Object.keys(s.variants).length), after.variants);

    check.ok = results.length > 0 && results.every((x) => x.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
