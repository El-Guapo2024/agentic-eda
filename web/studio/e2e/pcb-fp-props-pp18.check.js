// UI-LEFT-2 PP18 (task-v5-pp18-fpprops): Footprint Properties on the board, driven as a user drives it, each OK read back from /api/state and undone in one step:
//   - a Datasheet edited there is the footprint's and the symbol's (Update Schematic from PCB's "Other fields": `set_symbol_fields`);
//   - a user row named "Description" is taken as KiCad's reader takes it: its text lands on the Description (the footprint's and the symbol's), no second field;
//   - the footprint's own "Solder mask expansion" and "Allow bridged solder mask apertures between pads" (`set_footprint_solder_mask`);
//   - a user row named "Value" revalues the footprint and its symbol.
// run-all-checks: board=mcu30-placed minutes=6
(async () => {
  const results = [];
  const check = (window.__pcbFpPropsPp18Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail });

  (async () => {
    const state = () => json("/api/state");
    const lastAct = async () => (await state()).activity[0] ?? null;
    const waitAct = async (before) => {
      const t0 = Date.now();
      while (Date.now() - t0 < 10000) {
        const a = await lastAct();
        if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(400); return a; }
        await sleep(60);
      }
      return null;
    };
    const select = async (ids) => {
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", ids);
      await sleep(300);
    };
    const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
    const setValue = (el, v) => {
      const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
      if (!(el instanceof HTMLSelectElement)) el.dispatchEvent(new Event("change", { bubbles: true }));
    };
    const tab = async (name) => {
      if (__eda.state().tab !== name) {
        [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
        await sleep(1500);
      }
    };
    const open = async (ref) => {
      await select([ref]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      let root = null;
      for (let i = 0; i < 40 && !root; i++) { root = document.querySelector('[data-testid="footprint-properties"]'); await sleep(100); }
      return root;
    };
    const addField = async (root, name, text) => {
      root.querySelector('[data-testid="add-field"]').click();
      await sleep(150);
      const n = root.querySelectorAll('[data-testid^="field-row-"]').length - 1;
      const inputs = root.querySelectorAll(`[data-testid="field-row-${n}"] input`);
      setValue(inputs[0], name);
      setValue(inputs[1], text);
      await sleep(60);
    };
    const ok = async (root) => {
      const before = await lastAct();
      root.parentElement.querySelector('[data-testid="footprint-properties-ok"]').click();
      return waitAct(before);
    };
    const field = (p, name) => (p.fields ?? []).filter((f) => f.name === name);

    await tab("pcb");
    const s = await state();
    const part = s.parts.find((p) => p.placed && p.fields?.length >= 4 && p.symbol_fields && !(s.locked ?? []).includes(p.ref));
    if (!part) {
      t("PP18: a placed footprint with a symbol", false, "none");
    } else {
      // ----------------------------------------------------------------------------------------- Datasheet, a user "Description", the mask
      let root = await open(part.ref);
      t("PP18: Footprint Properties opens", !!root);
      if (root) {
        t("PP18: the mask expansion and the bridging are in the dialog", !!root.querySelector('[data-testid="fp-mask-margin-on"]') && !!root.querySelector('[data-testid="fp-mask-bridges"]'));
        setValue(root.querySelector('[data-testid="field-row-2"] input[aria-label="Datasheet text"]'), "https://example.com/pp18.pdf");
        await addField(root, "Description", "From the user row");
        root.querySelector('[data-testid="fp-mask-margin-on"]').click();
        await sleep(100);
        setValue(root.querySelector('[data-testid="fp-mask-margin"]'), "-0.02");
        root.querySelector('[data-testid="fp-mask-bridges"]').click();
        await sleep(100);
        const a = await ok(root);
        const err = document.querySelector('[data-testid="footprint-properties-error"]')?.textContent ?? "";
        const p = (await state()).parts.find((x) => x.ref === part.ref);
        t("PP18: OK takes a user row named like a mandatory field (no refusal)", a?.ok && !err, `${a?.cmd?.slice(0, 160)}; ${err}`);
        t("PP18: the Datasheet is the footprint's", field(p, "Datasheet").length === 1 && field(p, "Datasheet")[0].text === "https://example.com/pp18.pdf", JSON.stringify(field(p, "Datasheet").map((f) => f.text)));
        t("PP18: ... and the symbol's", p.symbol_fields?.Datasheet === "https://example.com/pp18.pdf", JSON.stringify(p.symbol_fields));
        t("PP18: the user row named Description is the Description, one field", field(p, "Description").length === 1 && field(p, "Description")[0].text === "From the user row", JSON.stringify(field(p, "Description").map((f) => f.text)));
        t("PP18: ... and the symbol's Description", p.symbol_fields?.Description === "From the user row", JSON.stringify(p.symbol_fields));
        t("PP18: the footprint's own mask expansion and bridging", p.mask_margin === -20_000 && p.allow_soldermask_bridges === true, `${p.mask_margin} ${p.allow_soldermask_bridges}`);
        // (an undo after a refused OK would take back the step before it)
        if (a?.ok) await undo();
        const back = (await state()).parts.find((x) => x.ref === part.ref);
        t(
          "PP18: one undo takes the whole OK back, the symbol's fields too",
          back.mask_margin == null && !back.allow_soldermask_bridges && JSON.stringify(back.symbol_fields) === JSON.stringify(part.symbol_fields) && field(back, "Datasheet")[0]?.text === field(part, "Datasheet")[0]?.text,
          `${back.mask_margin} ${back.allow_soldermask_bridges} ${JSON.stringify(back.symbol_fields)}`
        );
      }

      // ----------------------------------------------------------------------------------------- a user "Value"
      root = await open(part.ref);
      if (root) {
        await addField(root, "Value", "33k");
        const a = await ok(root);
        const p = (await state()).parts.find((x) => x.ref === part.ref);
        const sch = await json("/api/schematic");
        const sym = (sch.symbols ?? []).find((x) => x.id === part.ref);
        t("PP18: a user row named Value revalues the footprint and its symbol", a?.ok && p.fields[1].text === "33k" && field(p, "Value").length === 1 && (!sym || sym.value === "33k"), `${a?.cmd?.slice(0, 160)}; ${p.fields[1].text}; symbol ${sym?.value ?? "(on a sheet below)"}`);
        if (a?.ok) await undo();
        const back = (await state()).parts.find((x) => x.ref === part.ref);
        t("PP18: undone", back.fields[1].text === part.fields[1].text, back.fields[1].text);
      } else t("PP18: Footprint Properties opens again", false);
    }

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
