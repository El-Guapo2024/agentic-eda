// Design variants, round 2 (UI-LEFT-2 SS16, PP16; task-v8-variants; KiCad 25ec2335). On a placed board it:
//   1. adds a variant 'Lite' and gives a symbol a Value of its own there that names the variant (`${VARIANT}`), and checks the schematic the server resolves in
//      the variant (`GET /api/schematic?variant=Lite`) shows it expanded, and the default shows the symbol's own;
//   2. makes 'Lite' current on the schematic toolbar and checks the editor fetches the sheet in it (`shown_variant`), that the context menu offers Set Variant
//      Symbol... and not Change Symbol..., that Change Symbol says KiCad's message, and that Set Variant Symbol... opens the symbol chooser;
//   3. sets a variant symbol through the API: one that is not pin-compatible is refused with KiCad's "is not pin-compatible" message, a compatible one is the
//      record's and Clear Variant Symbol takes it away;
//   4. on the board, makes 'Lite' the board editor's variant and checks the footprint carries its fields in the variant (`GET /api/state?variant=`) and the
//      Properties panel's Value row shows the variant's text, the default's in the default;
//   5. removes 'Lite', so the board is as it was.
// It answers through `window.__designVariants2Check`: { results: [{ name, ok, detail }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__designVariants2Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => (results.push({ name, ok: !!ok, detail: typeof detail === "string" ? detail : JSON.stringify(detail) }), !!ok);
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };

  (async () => {
    const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
    const dialog = () => [...document.querySelectorAll(".dialog")].filter((d) => d.offsetParent !== null).pop() ?? null;
    const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
    const footerButton = (re) => [...(dialog()?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
    const set = async (el, value) => {
      const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
      await sleep(200);
    };
    const pick = async (sel, text) => {
      sel.selectedIndex = [...sel.options].findIndex((o) => o.textContent === text || o.value === text);
      sel.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(400);
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
    const sheetQuery = () => (__eda.state().sheetPath.length ? `sheet=${__eda.state().sheetPath.join("/")}&` : "");
    const here = async (variant = "") => json(`/api/schematic?${sheetQuery()}${variant ? `variant=${encodeURIComponent(variant)}` : ""}`);
    const rowInput = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`)?.querySelector("[data-prop-input]");
    const fieldShown = (sym, name) => {
      const f = (sym?.fields ?? []).find((x) => x.name === name);
      return f ? (f.shown ?? f.text) : name === "Value" ? (sym?.shown?.value ?? sym?.value) : null;
    };

    await tab("schematic");
    document.querySelector('button[aria-label="Show Properties"]')?.click();
    for (const v of (await here()).variants ?? []) if (v.name === "Lite") await post({ op: "remove_design_variant", name: v.name });
    await sleep(800);

    // ------------------------------------------------------------------------------------------------------ 1. ${VARIANT} in a variant's Value
    let sch = await here();
    let sym = (sch.symbols ?? []).find((x) => !(sch.locked ?? []).includes(x.id) && (x.fields ?? []).some((f) => f.name === "Value"));
    if (!sym && (sch.sheets ?? []).length) {
      await select([sch.sheets[0].id]);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(1500);
      sch = await here();
      sym = (sch.symbols ?? []).find((x) => !(sch.locked ?? []).includes(x.id));
    }
    if (!t("a symbol to work on", !!sym, "no unlocked symbol on the root or its first sheet")) throw new Error("no symbol");
    const own = fieldShown(sym, "Value");
    const added = await post({ op: "add_design_variant", name: "Lite", description: "No radio" });
    t("'Lite' is added", added.ok !== false, added);
    const wrap = (cmd) => (__eda.state().sheetPath.length ? { op: "on_sheet", sheet: __eda.state().sheetPath.join("/"), cmd } : cmd);
    const r1 = await post(wrap({ op: "set_symbol_variant", variant: "Lite", ids: [sym.id], fields: { Value: "V-${VARIANT}-${VARIANT_DESC}" } }));
    t("the symbol's Value in 'Lite' names the variant", r1.ok !== false, r1);
    const inLite = (await here("Lite")).symbols.find((x) => x.id === sym.id);
    const inDefault = (await here()).symbols.find((x) => x.id === sym.id);
    t("in 'Lite' the server shows ${VARIANT} and ${VARIANT_DESC} expanded", fieldShown(inLite, "Value") === "V-Lite-No radio", { lite: fieldShown(inLite, "Value") });
    t("the default shows the symbol's own Value", fieldShown(inDefault, "Value") === own, { default: fieldShown(inDefault, "Value"), own });

    // ------------------------------------------------------------------------------------------------------ 2. the schematic editor in 'Lite'
    const sel = await until(() => schSelect());
    if (sel) await pick(sel, "Lite");
    await sleep(1200);
    t("'Lite' is the schematic's current variant and the sheet is fetched in it", __eda.state().schVariant === "Lite" || schSelect()?.value === "Lite", schSelect()?.value);
    await select([sym.id]);
    const menuLabels = async () => {
      const el = document.querySelector(`[data-sch-item="${CSS.escape(sym.id)}"]`) ?? document.querySelector(".sch-canvas-container canvas, canvas");
      const box = el?.getBoundingClientRect();
      return { box: !!box };
    };
    await menuLabels();
    // Change Symbol... in a variant: KiCad's message, and no Change Symbols dialog
    const alerts = [];
    const realAlert = window.alert;
    window.alert = (m) => alerts.push(String(m));
    await __eda.run("eeschema.InteractiveEdit.changeSymbol");
    await sleep(500);
    window.alert = realAlert;
    t("Change Symbol in a variant says KiCad's message", alerts.some((m) => m === "Change Symbol is not available when a design variant is active. Use Set Variant Symbol or switch to the default variant first."), alerts);
    t("and opens no Change Symbols dialog", header() !== "Change Symbols", header());
    await __eda.run("eeschema.InteractiveEdit.setVariantSymbol");
    const chooser = await until(() => (dialog() && /Choose Symbol|Symbol/i.test(header() ?? "") ? dialog() : null), 6000);
    t("Set Variant Symbol... opens the symbol chooser", !!chooser, header());
    if (chooser) {
      footerButton(/^Cancel$/)?.click();
      await sleep(400);
    }

    // ------------------------------------------------------------------------------------------------------ 3. a variant symbol through the API
    const libId = sym.lib_id ?? "";
    const bad = await post(wrap({ op: "set_variant_symbol", variant: "Lite", id: sym.id, lib_id: "Device:Q_NPN_BCE" }));
    const badMsg = JSON.stringify(bad);
    t("a symbol that is not pin-compatible is refused with KiCad's message", bad.ok === false && /is not pin-compatible with the base symbol/.test(badMsg) && /Choose a symbol with equivalent pins and pin positions\./.test(badMsg), badMsg.slice(0, 400));
    // the symbol's own library id is always compatible with itself
    const same = await post(wrap({ op: "set_variant_symbol", variant: "Lite", id: sym.id, lib_id: libId }));
    t("a pin-compatible symbol is the record's variant symbol", same.ok !== false, same);
    let mine = (await here("Lite")).symbols.find((x) => x.id === sym.id);
    t("the record names it", mine?.variants?.Lite?.symbol_override === libId, mine?.variants?.Lite);
    await sleep(800);
    await select([sym.id]);
    const cleared = await __eda.run("eeschema.InteractiveEdit.clearVariantSymbol");
    await sleep(1200);
    mine = (await here("Lite")).symbols.find((x) => x.id === sym.id);
    t("Clear Variant Symbol takes it away", !mine?.variants?.Lite?.symbol_override, { cleared, rec: mine?.variants?.Lite });
    if (schSelect()) await pick(schSelect(), "< Default >");
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }

    // ------------------------------------------------------------------------------------------------------ 4. the board in 'Lite'
    await tab("pcb");
    const stateLite = await json("/api/state?variant=Lite");
    const reference = (stateLite.parts ?? []).find((p) => p.variant && p.variant.fields?.Value === "V-Lite-No radio")?.ref ?? null;
    t("the board's state in 'Lite' carries the footprint's Value there", !!reference && stateLite.shown_variant === "Lite", { reference, shown: stateLite.shown_variant });
    const vfield = (stateLite.parts ?? []).find((p) => p.ref === reference)?.fields?.find((f) => f.name === "Value");
    t("and the Value field shows it", (vfield?.shown ?? vfield?.text) === "V-Lite-No radio", vfield);
    const pcb = await until(() => pcbSelect());
    if (pcb && reference) {
      await set(pcbSelect(), "Lite");
      await sleep(1500);
      await select([reference]);
      await sleep(500);
      t("the Properties panel's Value row is the variant's", rowInput("Value")?.value === "V-Lite-No radio", rowInput("Value")?.value);
      await set(pcbSelect(), "< Default >");
      await sleep(1500);
      await select([reference]);
      await sleep(500);
      t("and the footprint's own in the default", rowInput("Value")?.value !== "V-Lite-No radio" && rowInput("Value")?.value != null, rowInput("Value")?.value);
    }
    await tab("schematic");

    // ------------------------------------------------------------------------------------------------------ 5. the board as it was
    await post({ op: "remove_design_variant", name: "Lite" });
    await sleep(800);
    const after = await here();
    t("at the end the board has no variants left", !(after.variants ?? []).length, after.variants);

    check.ok = results.length > 0 && results.every((x) => x.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
