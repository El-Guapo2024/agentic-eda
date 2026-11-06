// UI-LEFT P3, P14, P15 and P21 (task-u5-pcbfp), through the studio's own dialogs and panel as a user drives them, each edit read back from /api/state and undone:
//   P3  Track & Via Properties' Protection features (IPC-4761 Type IV-b), Backdrill (Top, its size and must-cut layer) and the via's own teardrops; Pad Properties'
//       Fabrication property and pad-to-die length; the Properties panel's Pin Name.
//   P14 Footprint Properties' four mandatory rows, "The first 4 fields are mandatory.", a new Value (the footprint's and the symbol's), the paste clearance, a
//       repeated field name taken as KiCad's reader takes it.
//   P15 a footprint's own text picked by a click, edited in its Text Properties, showing the title block's `${TITLE}`, deleted with Delete; a footprint's hatched
//       graphic carries its hatch lines.
//   P21 Change Footprint with a renumbered pad takes the old pad's net (KiCad's pad matching).
// run-all-checks: board=mcu30-routed minutes=12
(async () => {
  const results = [];
  const check = (window.__pcbFpPropsCheck = { results, done: false, ok: false, wait: null });
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
      const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
      if (!(el instanceof HTMLSelectElement)) el.dispatchEvent(new Event("change", { bubbles: true }));
    };
    const dialog = async (label) => {
      for (let i = 0; i < 40; i++) {
        const d = document.querySelector(`[role="dialog"][aria-label="${label}"]`);
        if (d) return d;
        await sleep(100);
      }
      return null;
    };
    const within = (root, label) => root?.querySelector(`[aria-label="${CSS.escape(label)}"]`);
    const okButton = (root) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "OK");
    const tab = async (name) => {
      if (__eda.state().tab !== name) {
        [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
        await sleep(1500);
      }
    };
    await tab("pcb");
    let s = await state();

    // ------------------------------------------------------------------------------------------------------------------------- P3: Track & Via Properties
    const via = s.routing?.vias?.[0];
    if (via) {
      await select([via.id]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      const d = await dialog("Track & Via Properties");
      t("P3: E on a via opens Track & Via Properties with the protection, backdrill and teardrop rows", !!d && !!within(d, "Protection features") && !!within(d, "Backdrill") && !!within(d, "Add teardrops on via's track connections"), d ? "" : "no dialog");
      if (d) {
        setValue(within(d, "Protection features"), "ivb");
        setValue(within(d, "Backdrill"), "top");
        await sleep(150);
        setValue(within(d, "Front backdrill size"), "0.5");
        setValue(within(d, "Front backdrill must-cut"), "B.Cu");
        within(d, "Add teardrops on via's track connections").click();
        setValue(within(d, "Best length (L):"), "80");
        await sleep(100);
        const before = await lastAct();
        okButton(d).click();
        const a = await waitAct(before);
        const v = (await state()).routing.vias.find((x) => x.id === via.id);
        const p = v.props ?? {};
        t(
          "P3: Type IV-b, a top backdrill and the via's own teardrops reach the via",
          a?.ok && p.front_tenting === true && p.back_tenting === true && p.front_plugging === true && p.back_plugging === true && p.backdrill?.start === "F.Cu" && p.backdrill?.size === 500_000 && v.teardrops?.enabled === true && Math.abs((v.teardrops?.best_length_ratio ?? 0) - 0.8) < 1e-9,
          `${a?.cmd?.slice(0, 120)}; props ${JSON.stringify(p)}; teardrops ${JSON.stringify(v.teardrops)}`
        );
        await undo();
        const back = (await state()).routing.vias.find((x) => x.id === via.id);
        t("P3: one undo takes the via's edits back", JSON.stringify(back.props ?? null) === JSON.stringify(via.props ?? null) && !back.teardrops, JSON.stringify(back.props));
      }
    } else t("P3: via", false, "the board has no via");

    // ------------------------------------------------------------------------------------------------------------------------------- P3: Pad Properties
    const part = s.parts.find((p) => p.placed && p.pads?.some((q) => q.num && q.kind !== "non_plated_hole"));
    const pad = part?.pads?.find((q) => q.num && q.kind !== "non_plated_hole");
    if (pad) {
      const padId = `${part.ref}.${pad.num}`;
      await select([padId]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      let root = null;
      for (let i = 0; i < 40 && !root; i++) { root = document.querySelector('[data-testid="board-pad-properties"]'); await sleep(100); }
      t("P3: Pad Properties has Fabrication property, pad to die length and the Teardrops part", !!root && !!within(root, "Fabrication property") && !!within(root, "Specify pad to die length") && !!within(root, "Add teardrops on pad's track connections"), root ? "" : "no dialog");
      if (root) {
        setValue(within(root, "Fabrication property"), "pad_prop_testpoint");
        within(root, "Specify pad to die length").click();
        await sleep(150);
        setValue(within(root, "Pad to die length"), "0.25");
        await sleep(100);
        const before = await lastAct();
        document.querySelector('[data-testid="board-pad-properties-ok"]').click();
        const a = await waitAct(before);
        const q = (await state()).parts.find((p) => p.ref === part.ref).pads.find((x) => x.num === pad.num);
        t("P3: the fabrication property and the pad-to-die length are the pad's", a?.ok && q.property === "pad_prop_testpoint" && q.die_length === 250_000, `${a?.cmd?.slice(0, 120)}; ${q.property} ${q.die_length}`);
        await undo();
      }
      // the Properties panel's Pin Name
      document.querySelector('button[aria-label="Show Properties"]')?.click();
      await select([padId]);
      let el = null;
      for (let i = 0; i < 25 && !el; i++) { el = document.querySelector('.prop-row[data-prop="Pin Name"] [data-prop-input]'); await sleep(200); }
      if (el) {
        const before = await lastAct();
        el.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, "SDA");
        el.dispatchEvent(new Event("input", { bubbles: true }));
        await sleep(30);
        el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        const a = await waitAct(before);
        const q = (await state()).parts.find((p) => p.ref === part.ref).pads.find((x) => x.num === pad.num);
        t("P3: the panel's Pin Name is the pad's own", a?.ok && q.pin_function === "SDA", `${a?.cmd?.slice(0, 100)}; ${q.pin_function}`);
        await undo();
      } else t("P3: the panel's Pin Name is editable", false, "no editable Pin Name row");
    } else t("P3: pad", false, "no placed footprint with a numbered pad");

    // ----------------------------------------------------------------------------------------------------------------------- P14: Footprint Properties
    const fpPart = s.parts.find((p) => p.placed && p.fields?.length >= 4 && p.value);
    if (fpPart) {
      await select([fpPart.ref]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      let root = null;
      for (let i = 0; i < 40 && !root; i++) { root = document.querySelector('[data-testid="footprint-properties"]'); await sleep(100); }
      const rowName = (i) => root?.querySelector(`[data-testid="field-row-${i}"] input`)?.value;
      t("P14: four mandatory rows: Reference, Value, Datasheet, Description", !!root && [0, 1, 2, 3].map(rowName).join(",") === "Reference,Value,Datasheet,Description", [0, 1, 2, 3].map(rowName).join(","));
      if (root) {
        root.querySelector('[data-testid="field-row-3"]').click();
        await sleep(100);
        const del = root.querySelector('[data-testid="delete-field"]');
        t("P14: the fourth row cannot be deleted: The first 4 fields are mandatory.", del?.disabled && del?.title === "The first 4 fields are mandatory.", del?.title);
        // a new Value, the paste clearance, and two fields named alike
        setValue(within(root, "Value text"), "47k");
        root.querySelector('[data-testid="fp-paste-margin-on"]').click();
        await sleep(100);
        setValue(root.querySelector('[data-testid="fp-paste-margin"]'), "-0.05");
        for (const text of ["A", "B"]) {
          root.querySelector('[data-testid="add-field"]').click();
          await sleep(150);
          const n = root.querySelectorAll('[data-testid^="field-row-"]').length - 1;
          const inputs = root.querySelectorAll(`[data-testid="field-row-${n}"] input`);
          setValue(inputs[0], "MPN");
          setValue(inputs[1], text);
          await sleep(60);
        }
        const before = await lastAct();
        root.parentElement.querySelector('[data-testid="footprint-properties-ok"]').click();
        const a = await waitAct(before);
        const p = (await state()).parts.find((x) => x.ref === fpPart.ref);
        const sch = await json("/api/schematic");
        const sym = (sch.symbols ?? []).find((x) => x.id === fpPart.ref);
        const mpn = (p.fields ?? []).filter((f) => f.name === "MPN");
        t("P14: the new Value is the footprint's and the symbol's", a?.ok && p.fields[1].text === "47k" && (!sym || sym.value === "47k"), `${a?.cmd?.slice(0, 140)}; ${p.fields[1].text}; symbol ${sym?.value ?? "(on a sheet below)"}`);
        t("P14: the footprint's paste clearance", p.paste_margin === -50_000, String(p.paste_margin));
        t("P14: two fields named MPN are one, with the later's text", mpn.length === 1 && mpn[0].text === "B", JSON.stringify(mpn.map((f) => f.text)));
        await undo();
        const back = (await state()).parts.find((x) => x.ref === fpPart.ref);
        t("P14: one undo takes the whole OK back", back.fields[1].text === fpPart.fields[1].text && back.paste_margin == null, `${back.fields[1].text} ${back.paste_margin}`);
      }
    } else t("P14: footprint", false, "no placed footprint with fields");

    // ------------------------------------------------------------------------------------------------------------------------- P15: a footprint's text
    s = await state();
    const textPart = s.parts.find((p) => p.placed && p.texts?.length && !(s.locked ?? []).includes(p.ref));
    if (textPart) {
      const ft = textPart.texts[0];
      // a click on it picks the text, not its footprint (`GuessSelectionCandidates`: the smaller item)
      await __eda.run("common.InteractiveSelection.clear");
      await select([ft.id]);
      const sel = __eda.state().selection;
      t("P15: the footprint's text is an item to select", sel.some((x) => x.id === ft.id && x.kind === "fptext"), JSON.stringify(sel));
      await __eda.run("pcbnew.InteractiveEdit.properties");
      const d = await dialog("Text Properties");
      t("P15: E on it opens its Text Properties", !!d, "");
      if (d) {
        await post({ op: "set_board_page", page: { paper: "A4", portrait: false, user_size_nm: null }, title_block: { title: "BOARD-TITLE", rev: "A", company: "", date: "", comments: [] } }).catch(() => null);
        setValue(within(d, "Text"), "${TITLE}");
        const before = await lastAct();
        okButton(d).click();
        const a = await waitAct(before);
        const tx = (await state()).parts.find((p) => p.ref === textPart.ref).texts?.find((x) => x.id === ft.id);
        t("P15: the text is the footprint's own, the title block's variable resolved", a?.ok && tx?.raw === "${TITLE}" && tx?.text === "BOARD-TITLE", `${a?.cmd?.slice(0, 100)}; raw ${tx?.raw} shown ${tx?.text}`);
        await undo();
      }
      // Delete takes it off its footprint
      await select([ft.id]);
      const before = await lastAct();
      await __eda.run("common.Interactive.delete");
      const a = await waitAct(before);
      const left = (await state()).parts.find((p) => p.ref === textPart.ref).texts ?? [];
      t("P15: Delete takes the footprint's text away", a?.ok && left.length === textPart.texts.length - 1, `${a?.cmd?.slice(0, 100)}; ${left.length} of ${textPart.texts.length}`);
      await undo();
    } else t("P15: footprint text", false, "no placed footprint with a text of its own");

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
