// IDF and GenCAD check (PARITY-pcb.md section 49): File > Export > IDFv3... (`DIALOG_EXPORT_IDF3`) and GenCAD... (`DIALOG_GENCAD_EXPORT_OPTIONS`),
// both written by kicad-cli (`kicad-cli pcb export idf|gencad`), driven through `window.__eda` and the dialog's own controls. Run it in the page
// of a served board that is placed -- `eda pipeline examples/opamp_filter.yaml -o <p>`, `eda board new examples/opamp_filter.yaml -o <dir>`,
// `cp <p>/design.json <dir>/`, `eda board serve -C <dir> --ui web/studio/dist` -- with `run-check.mjs`, or pasted into the Browser pane.
//
// It checks the kicad-cli the server has, as `GET /api/fab/exporters` says:
//   with IDF and GenCAD (the box's nightly): both entries are offered and enabled under File > Export; the IDF dialog has KiCad's title,
//     labels and defaults (Millimeters, nothing checked, the positions greyed out until "Set board reference point:" is checked), and its
//     Export writes `export/kicad/idf/<board>.emn` and `.emp`, again with Mils and a reference point; the GenCAD dialog has KiCad's title and
//     its five options, and writes `export/kicad/gencad/<board>.cad` with all of them checked;
//   without IDF (serve with EDA_KICAD_CLI pointing at a kicad-cli whose `pcb export --help` leaves `idf` out, e.g. a wrapper that drops it
//     from the real one's list): the entry is disabled, its tooltip and the test hook give the server's reason ("This kicad-cli (...) has no
//     IDF exporter; KiCad 10.99 nightly or later has one."), running it is refused, and GenCAD is still offered;
//   nothing logged an error.
//
// It answers through `window.__cliExportsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
(async () => {
  const results = [];
  const check = (window.__cliExportsCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden pane throttles timers
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // The dialog's controls, by their text.
  const dialog = () => document.querySelector(".dialog");
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent ?? null;
  const labelled = (text) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim() === text);
  const box = (text) => labelled(text)?.querySelector("input[type=checkbox]");
  const row = (text) => { const s = [...(dialog()?.querySelectorAll(".kv-grid > span") ?? [])].find((e) => e.textContent.trim() === text); return s?.nextElementSibling ?? null; };
  const button = (text) => [...(dialog()?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const setValue = (el, value, event) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(event, { bubbles: true }));
  };
  /** Export, then the files the dialog lists (or its message). */
  const exportNow = async () => {
    button("Export").click();
    // The last result goes when the run starts; the new one comes when it ends.
    await until(() => !dialog().querySelector(".panel-empty, .problem-row"), 5000);
    await until(() => button("Export") && !button("Export").disabled && dialog().querySelector(".panel-empty, .problem-row"), 120000);
    const out = dialog().querySelector(".panel-empty, .problem-row");
    return { ok: out?.classList.contains("panel-empty") ?? false, message: out?.querySelector("b")?.textContent ?? "", files: [...(out?.querySelectorAll("li") ?? [])].map((li) => li.textContent) };
  };
  const close = async () => { button("Close")?.click(); await until(() => !dialog(), 3000); };

  // The File > Export entry of an action: open the File menu (its submenus are in the page), read the entry, close the menu.
  const menuEntry = async (label) => {
    const file = [...document.querySelectorAll(".menubar-item")].find((m) => m.firstChild?.textContent?.trim() === "File");
    file?.click();
    await sleep(150);
    const exportMenu = [...document.querySelectorAll(".menu-node-submenu")].find((s) => s.querySelector(".menu-node-label")?.textContent.startsWith("Export"));
    const item = [...(exportMenu?.querySelectorAll(".menu-node-item") ?? [])].find((i) => i.textContent.trim() === label);
    const seen = item ? { disabled: item.getAttribute("aria-disabled") === "true", title: item.title } : null;
    file?.click();
    await sleep(100);
    return seen;
  };

  const errorsFrom = Date.now();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const caps = await fetch("/api/fab/exporters", { cache: "no-store" }).then((r) => r.json());
  const action = (id) => E.actions().find((x) => x.id === id);
  const IDF = "pcbnew.EditorControl.exportIDF", GENCAD = "pcbnew.EditorControl.exportGenCAD";

  await scenario("the server says which exporters its kicad-cli has", async (t) => {
    t(typeof caps.engine === "string" && caps.engine.startsWith("kicad-cli "), `engine: ${caps.engine}`);
    t(caps.unavailable && typeof caps.unavailable === "object", `unavailable: ${JSON.stringify(caps.unavailable)}`);
    for (const [kind, why] of Object.entries(caps.unavailable ?? {})) t(/^This kicad-cli( \(.*\))? has no (IDF|GenCAD) exporter; KiCad 10\.99 nightly or later has one\.$/.test(why), `${kind}: ${why}`);
  });

  if (!caps.unavailable?.idf) {
    await scenario("IDFv3... and GenCAD... are offered under File > Export", async (t) => {
      for (const id of [IDF, GENCAD]) t(action(id)?.enabled === true, `${id}: ${JSON.stringify(action(id))}`);
      for (const label of ["Export IDFv3...", "Export GenCAD..."]) {
        const m = await menuEntry(label);
        t(m && !m.disabled, `the menu entry ${label}: ${JSON.stringify(m)}`);
      }
    });

    await scenario("Export IDFv3: KiCad's title, labels and defaults, and the board's .emn and .emp", async (t) => {
      await E.run(IDF);
      await until(() => header());
      t(header() === "Export IDFv3", `title: ${header()}`);
      for (const l of ["Set board reference point:", "Ignore 'Do not populate' components", "Ignore 'Unspecified' components"]) t(box(l) && !box(l).checked, `"${l}" unchecked`);
      const x = row("X position:")?.querySelector("input"), y = row("Y position:")?.querySelector("input");
      t(x?.disabled && y?.disabled && x.value === "0" && y.value === "0", `X and Y greyed out at 0 (OnBoardReferencePointChecked): ${x?.disabled} ${y?.disabled} ${x?.value} ${y?.value}`);
      const units = row("Output units:")?.querySelector("select");
      t(units && units.value === "mm" && [...units.options].map((o) => o.textContent).join(",") === "Millimeters,Mils", `units: ${units?.value} ${[...(units?.options ?? [])].map((o) => o.textContent)}`);
      const r = await exportNow();
      t(r.ok, `exported: ${r.message}`);
      t(r.files.length === 2 && r.files.some((f) => /^export\/kicad\/idf\/.+\.emn$/.test(f)) && r.files.some((f) => /^export\/kicad\/idf\/.+\.emp$/.test(f)), `files: ${r.files.join(", ")}`);
      t(new Set(r.files.map((f) => f.replace(/\.em[np]$/, ""))).size === 1, `one name, the board's: ${r.files}`);
    });

    await scenario("Export IDFv3 with a board reference point, in mils", async (t) => {
      box("Set board reference point:").click();
      await sleep(100);
      const x = row("X position:").querySelector("input"), y = row("Y position:").querySelector("input");
      t(!x.disabled && !y.disabled, "checking the box enables the positions");
      setValue(x, 1.225, "input");
      setValue(y, 0.025, "input");
      setValue(row("Output units:").querySelector("select"), "mils", "change");
      box("Ignore 'Do not populate' components").click();
      await sleep(100);
      t(x.value === "1.225" && row("Output units:").querySelector("select").value === "mils", `the controls took the values: ${x.value}, ${row("Output units:").querySelector("select").value}`);
      const r = await exportNow();
      t(r.ok && r.files.length === 2, `exported: ${r.message} ${r.files.join(", ")}`);
      await close();
    });

    await scenario("Export to GenCAD: KiCad's title and five options, and the board's .cad", async (t) => {
      await E.run(GENCAD);
      await until(() => header());
      t(header() === "Export to GenCAD", `title: ${header()}`);
      const options = ["Flip bottom footprint padstacks", "Generate unique pin names", "Generate a new shape for each footprint instance (do not reuse shapes)", "Use drill/place file origin as origin", "Save the origin coordinates in the file"];
      for (const l of options) t(box(l) && !box(l).checked, `"${l}" unchecked`);
      for (const l of options) box(l).click();
      await sleep(100);
      t(options.every((l) => box(l).checked), "all five checked");
      const r = await exportNow();
      t(r.ok && r.files.length === 1 && /^export\/kicad\/gencad\/.+\.cad$/.test(r.files[0]), `exported: ${r.message} ${r.files.join(", ")}`);
      await close();
    });
  } else {
    await scenario("without an IDF exporter, IDFv3... is disabled with the reason and GenCAD... is still offered", async (t) => {
      const why = caps.unavailable.idf;
      const a = action(IDF);
      t(a && a.enabled === false && a.reason === why, `the test hook: ${JSON.stringify(a)}`);
      const m = await menuEntry("Export IDFv3...");
      t(m && m.disabled && m.title === why, `the menu entry: ${JSON.stringify(m)}`);
      const r = await E.run(IDF);
      t(r.ok === false && r.error === why, `running it is refused: ${JSON.stringify(r)}`);
      t(!dialog(), "no dialog opened");
      if (!caps.unavailable.gencad) {
        t(action(GENCAD)?.enabled === true, `GenCAD: ${JSON.stringify(action(GENCAD))}`);
        const g = await menuEntry("Export GenCAD...");
        t(g && !g.disabled, `the GenCAD entry: ${JSON.stringify(g)}`);
      }
    });
  }

  await scenario("nothing logged an error", async (t) => {
    const errs = E.errors(errorsFrom);
    t(errs.length === 0, `no errors: ${errs.map((e) => e.message).join("; ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
