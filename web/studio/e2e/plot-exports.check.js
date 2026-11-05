// Plot and fabrication outputs check (GAPS.md items 33 and 37, package P20, UI-LEFT X1-X4): File > Fabrication Outputs and File > Export of the board editor
// and File > Plot / Export > Netlist of the schematic editor, every file written by kicad-cli. The Plot dialog (Gerbers: the default layer list, one file per layer
// and the job file, a layer left out is not plotted; PostScript, SVG, DXF, PDF, PNG into the output folder, the one-document PDF, kicad-cli's limits disabled,
// Generate Drill Files... over it), Generate Drill Files (one merged .drl, or PTH and NPTH apart; Gerber X2 with maps, Excellon in inches with a PDF map, the
// report file), Generate Placement Files (CSV and ASCII, per side), STEP, VRML, IPC-D-356, IPC-2581, ODB++ and the board's Bill of Materials, Plot Schematic
// (PDF, SVG one file per sheet, PostScript, DXF, PNG) and Export Netlist (every format). Each is opened with its action, run with its own
// button, and judged by what the server answered (`/api/fab/*`, `/api/sch/plot`, captured as they come back: `ok`, the engine and the files written in
// the board's `export/kicad/<kind>/`) and by the dialog's result panel; none of them edits the design except the Plot and Drill dialogs' choices, which KiCad
// stores in the board's plot settings (no undo step, as KiCad's `applyPlotSettings` pushes none: r4 X4-3; put back here; the revision is the same at the end).
// Run it in the page of a served board that is placed and routed -- `eda pipeline examples/opamp_filter.yaml -o <p>`, `eda board new examples/opamp_filter.yaml
// -o <dir>`, `cp <p>/design.json <dir>/`, `eda board serve -C <dir> --port N --ui web/studio/dist`, kicad-cli on the PATH or in EDA_KICAD_CLI -- with
// `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/plot-exports.check.js`, or pasted into the Browser pane's
// javascript tool. It writes files into the board's export/ folder and nothing else.
// It answers through `window.__plotExportsCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __plotExportsCheck.wait(60000)` until `.done` (kicad-cli takes seconds per run, STEP the longest).
// run-all-checks: board=opamp-routed minutes=12   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__plotExportsCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };

  // What the dialogs ask the server, captured as it comes back.
  const replies = [];
  if (!window.__plotExportsFetch) {
    const real = window.fetch.bind(window);
    window.__plotExportsFetch = real;
    window.fetch = async (input, init) => {
      const r = await real(input, init);
      const url = typeof input === "string" ? input : input?.url ?? "";
      const m = /\/api\/(fab\/[a-z0-9_]+|sch\/plot|sch\/netlist)(\?|$)/.exec(url);
      if (m && (init?.method ?? "GET") === "POST") {
        let req = null;
        try { req = init?.body ? JSON.parse(init.body) : null; } catch { /* not JSON */ }
        r.clone().json().then((body) => replies.push({ route: m[1], req, body })).catch(() => undefined);
      }
      return r;
    };
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const run = (id, args) => __eda.run(id, args);
  // The dialog on top: the last one shown (a dialog opened from another hides it, as KiCad's modal child covers its parent).
  const dialog = () => [...document.querySelectorAll(".dialog")].filter((d) => d.offsetParent !== null).pop() ?? null;
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
  const buttonText = (root, re) => [...(root?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const labelled = (text) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim().startsWith(text));
  const box = (text) => labelled(text)?.querySelector("input");
  const close = async () => { for (let i = 0; i < 3 && dialog(); i++) { buttonText(dialog(), /^(Close|Cancel)$/)?.click(); await sleep(300); } };
  /** A React-controlled select or text input set as a user would. */
  const choose = async (el, value) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(200);
  };
  const testid = (id) => dialog()?.querySelector(`[data-testid=${id}]`);
  /** The Plot dialog's "Include Layers" checkbox of a layer. */
  const includeBox = (layer) => dialog()?.querySelector(`[data-testid=plot-include-layers] [data-layer="${layer}"] input`);
  const version = () => fetch("/api/version", { cache: "no-store" }).then((r) => r.text());
  /** Opens a dialog by its action, checks its title; resolves to whether it is open. */
  const open = async (t, action, title) => {
    const r = await run(action);
    await sleep(400);
    return t(header()?.startsWith(title), `${action} opens "${title}" (${r.dialog ?? header()})`);
  };
  /** Presses the dialog's run button and waits for the server's answer and the result panel. */
  const press = async (re, route) => {
    const n = replies.length;
    buttonText(dialog(), re)?.click();
    const reply = await until(() => replies.slice(n).find((x) => x.route === route), 240000);
    await until(() => dialog()?.querySelector(".panel-empty, .problem-row"), 5000);
    const out = dialog()?.querySelector(".panel-empty, .problem-row");
    return { reply, panel: { ok: out?.classList.contains("panel-empty") ?? false, message: out?.querySelector("b")?.textContent ?? "", files: [...(out?.querySelectorAll("li") ?? [])].map((li) => li.textContent) } };
  };
  const base = (f) => f.split("/").pop();
  /** The reply's files, and that the panel lists the same ones. */
  const judged = (t, what, res, re) => {
    const files = res.reply?.body?.files ?? [];
    t(res.reply?.body?.ok === true, `${what}: the server says ok (${res.reply ? JSON.stringify(res.reply.body).slice(0, 240) : "no reply"})`);
    t(files.length > 0 && files.every((f) => re.test(f)), `${what}: the files it wrote are ${re} (${files.map(base).join(", ")})`);
    t(res.panel.ok && JSON.stringify(res.panel.files) === JSON.stringify(files), `${what}: the dialog lists them (${res.panel.message}: ${res.panel.files.length})`);
    return files;
  };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await close(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const st0 = await fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  const copper = st0.layers ?? ["F.Cu", "B.Cu"];
  let rev0 = await version();
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const plotParams = async () => (await fetch("/api/state", { cache: "no-store" }).then((r) => r.json())).plot_params ?? null;
  if (!st0.parts?.some((p) => p.placed) || !(st0.routing?.tracks ?? []).length) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs placed footprints and routed tracks (eda pipeline examples/opamp_filter.yaml)"] }); check.done = true; return "no board"; }

  (async () => {
    await scenario("Plot (Gerbers): the default layers are the copper, mask, paste, silkscreen and outline; Plot writes one Gerber per layer and the job file; a layer left out is not plotted", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateGerbers", "Plot"))) return;
      const ticked = [...dialog().querySelectorAll("[data-testid=plot-include-layers] label.toggle input[type=checkbox]")].filter((i) => i.checked).map((i) => i.closest("label").textContent.trim());
      t(copper.every((l) => ticked.some((x) => x.startsWith(l))) && ["Edge.Cuts"].every((l) => ticked.some((x) => x.startsWith(l))), `every copper layer and Edge.Cuts start ticked (${ticked.join(", ")})`);
      let res = await press(/^Plot$/, "fab/plot");
      const files = judged(t, "Gerbers", res, /\.(g[a-z0-9]+|gbrjob)$/i);
      t(files.some((f) => /\.gbrjob$/.test(f)), "with the job file (.gbrjob)");
      const gerbers = files.filter((f) => !/\.gbrjob$/.test(f));
      t(gerbers.length === (res.reply?.req?.layers ?? []).length, `one Gerber per layer asked for (${gerbers.length} for ${(res.reply?.req?.layers ?? []).length})`);
      t(res.reply?.req?.check_zones === true, "Check zone fills before plotting is on by default (DIALOG_PLOT::m_zoneFillCheck)");
      // Leave B.SilkS out.
      const silk = includeBox("B.SilkS");
      if (t(!!silk, "the list has B.Silkscreen")) {
        silk.click();
        // The server lists the files modified since a second before the run (`run_export`: mtimes are coarse on some file systems), so the second plot waits
        // that second out; otherwise the first run's B.SilkS file would be listed again.
        await sleep(1500);
        res = await press(/^Plot$/, "fab/plot");
        t(res.reply?.body?.ok === true && !(res.reply?.req?.layers ?? []).includes("B.SilkS"), `the second plot asks for the layers without B.SilkS (${(res.reply?.req?.layers ?? []).join(",")})`);
        t((res.reply?.body?.files ?? []).filter((f) => !/\.gbrjob$/.test(f)).length === gerbers.length - 1, `and writes one Gerber fewer (${(res.reply?.body?.files ?? []).length})`);
        // r3 X3-3: `applyPlotSettings` stored the checklist in the board before the plot (no undo step, r4 X4-3), and the plot took the origin from the board.
        const stored = await plotParams();
        t(stored && !stored.layers.includes("B.SilkS") && stored.layers.includes("F.SilkS"), `the board's plot settings no longer select B.SilkS (${(stored?.layers ?? []).join(",")})`);
        t(res.reply?.req && !("use_aux_origin" in res.reply.req), "the plot does not send the origin: the server takes the board's useauxorigin");
        await close();
        await run("common.Interactive.undo");
        await sleep(1500);
        const still = await plotParams();
        t(still && !still.layers.includes("B.SilkS"), "Ctrl+Z does not take the dialog's choice back: `applyPlotSettings` only does SetPlotSettings and OnModify, no commit (r4 X4-3)");
        const put = await post({ op: "set_plot_settings", layers: [...(still?.layers ?? []), "B.SilkS"] });
        t(put.ok, "B.SilkS is put back in the board's plot settings");
        await until(async () => { const p = await plotParams(); return p?.layers.includes("B.SilkS") ? p : null; }, 15000);
        rev0 = await version();
      }
    });

    await scenario("Plot and Drill dialogs start from the board's plot settings at each opening (r3 X3-3): a selection without F.Paste and useauxorigin yes", async (t) => {
      const before = await plotParams();
      const r = await post({ op: "set_plot_settings", layers: (before?.layers ?? []).filter((l) => l !== "F.Paste"), use_aux_origin: true });
      if (!t(r.ok, `set_plot_settings (${JSON.stringify(r).slice(0, 200)})`)) return;
      await until(async () => (await plotParams())?.use_aux_origin === true, 10000);
      const v = await version();
      await until(() => __eda.state().revision === v, 10000); // the page's poll picks the new board up
      await sleep(300);
      if (!(await open(t, "pcbnew.EditorControl.generateGerbers", "Plot"))) return;
      const paste = includeBox("F.Paste");
      t(paste && paste.checked === false, "F.Paste starts unchecked, as the board's selection says");
      t(box("Use drill/place file origin")?.checked === true, "Use drill/place file origin starts checked (useauxorigin yes)");
      await close();
      if (!(await open(t, "pcbnew.EditorControl.generateDrillFiles", "Generate Drill Files"))) return;
      t(testid("drill-origin")?.value === "aux", `the drill Origin starts at the drill/place file origin (${testid("drill-origin")?.value})`);
      const res = await press(/^Generate$/, "fab/drill");
      t(/^Created file '.*-PTH\.drl'\n(Created file '.*'\n)*Done\.$/.test(res.reply?.body?.message ?? ""), `the writer's report (${JSON.stringify(res.reply?.body?.message)})`);
      t(res.panel.message.startsWith("Created file"), "and the dialog shows it");
      await close();
      await run("common.Interactive.undo");
      await sleep(1500);
      const still = await plotParams();
      t(still?.use_aux_origin === true && !still.layers.includes("F.Paste"), "Ctrl+Z leaves the board's plot settings as they were stored (no undo step, r4 X4-3)");
      const put = await post({ op: "set_plot_settings", layers: before?.layers ?? [], use_aux_origin: before?.use_aux_origin ?? false });
      t(put.ok, "the plot settings are put back");
      await until(async () => { const p = await plotParams(); return p?.use_aux_origin === (before?.use_aux_origin ?? false) && p.layers.includes("F.Paste") ? p : null; }, 15000);
      rev0 = await version();
    });

    await scenario("Generate Drill Files: the plated and non-plated holes in two Excellon files (KiCad's default), or in one with \"PTH and NPTH in single file\"", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateDrillFiles", "Generate Drill Files"))) return;
      let res = await press(/^Generate$/, "fab/drill");
      const apart = judged(t, "drill", res, /\.(drl|gbr)$/i);
      t(res.reply?.req?.merge_pth_npth === false && res.reply?.req?.alt_drill_mode === false, "two files and routed slots by default (m_Check_Merge_PTH_NPTH, m_altDrillMode unchecked)");
      t(apart.some((f) => /-PTH\.drl$/.test(f)), `apart: ${apart.map(base).join(", ")}`);
      box("PTH and NPTH in single file")?.click();
      await sleep(1500); // the second-long window of the file list, as above
      res = await press(/^Generate$/, "fab/drill");
      const merged = res.reply?.body?.files ?? [];
      t(res.reply?.body?.ok === true && res.reply?.req?.merge_pth_npth === true && !merged.some((f) => /PTH\.drl$/i.test(f)), `merged: ${merged.map(base).join(", ")}`);
      box("PTH and NPTH in single file")?.click();
    });

    await scenario("Plot (X1): every format of KiCad's dialog -- PostScript, SVG, DXF, PDF, PNG -- one file per layer into the output folder; each format's box; kicad-cli's limits disabled", async (t) => {
      const before = await plotParams();
      if (!(await open(t, "pcbnew.EditorControl.generateGerbers", "Plot"))) return;
      const formats = [...(testid("plot-format")?.options ?? [])].map((o) => o.textContent.trim());
      t(JSON.stringify(formats) === JSON.stringify(["Gerber", "Postscript", "SVG", "DXF", "PDF", "PNG"]), `the formats, in KiCad's order (${formats.join(", ")})`);
      t(testid("plot-gerber-options") && box("Generate Gerber job file")?.disabled === true && box("Generate Gerber job file")?.checked === true, "Gerber: its box, the job file always written (disabled, kicad-cli has no flag)");
      t(testid("plot-drill-marks")?.disabled === true && testid("plot-drill-marks")?.value === "0", "Gerber: no drill marks (SetPlotFormat)");
      // Three layers, into a folder of the board's.
      for (const l of [...dialog().querySelectorAll("[data-testid=plot-include-layers] [data-layer] input")]) if (l.checked !== ["F.Cu", "B.Cu", "Edge.Cuts"].includes(l.closest("[data-layer]").dataset.layer)) l.click();
      await choose(testid("plot-output-dir"), "plots/e2e/");
      for (const [format, ext] of [["ps", "ps"], ["svg", "svg"], ["dxf", "dxf"], ["pdf", "pdf"], ["png", "png"]]) {
        await choose(testid("plot-format"), format);
        t(!!testid(`plot-${format}-options`), `${format}: its options box is shown`);
        if (format === "pdf") t(box("Use drill/place file origin")?.disabled === true, "PDF: the drill/place file origin is disabled (kicad-cli has no flag)");
        if (format === "dxf") t(box("Use KiCad font to plot text")?.disabled === true, "DXF: the KiCad font box is disabled (kicad-cli has no flag)");
        if (format === "ps") t(box("Mirrored plot")?.disabled === false && box("Use drill/place file origin")?.disabled === true, "PostScript: mirror on, origin off (SetPlotFormat)");
        await sleep(1100); // the second-long window of the file list
        const res = await press(/^Plot$/, "fab/plot");
        const files = judged(t, format, res, new RegExp(`\\.${ext}$`, "i"));
        t(files.length === 3 && files.every((f) => f.startsWith("plots/e2e/")), `${format}: one file per layer, in the output folder (${files.map(base).join(", ")})`);
        t(/^Plotted to '.*'\.\n(Plotted to '.*'\.\n)*Done\.$/.test(res.reply?.body?.message ?? ""), `${format}: the plotter's report (${JSON.stringify(res.reply?.body?.message).slice(0, 120)})`);
      }
      // PDF's "Single document": one file of every layer.
      await choose(testid("plot-format"), "pdf");
      testid("plot-pdf-single")?.click();
      await sleep(1100);
      const one = await press(/^Plot$/, "fab/plot");
      t(one.reply?.body?.ok === true && (one.reply?.body?.files ?? []).length === 1, `PDF, Single document: one file (${(one.reply?.body?.files ?? []).map(base).join(", ")})`);
      const stored = await plotParams();
      t(stored?.options?.outputformat === "4" && stored?.options?.pdf_single_document === "yes" && stored?.options?.outputdirectory === "plots/e2e/", `applyPlotSettings stored the format, the PDF option and the folder (${JSON.stringify(stored?.options ?? {}).slice(0, 160)})`);
      // "Generate Drill Files..." from the Plot dialog: the drill dialog over it, with the same output folder.
      buttonText(dialog(), /^Generate Drill Files\.\.\.$/)?.click();
      await sleep(600);
      t(header() === "Generate Drill Files" && testid("drill-output-dir")?.value === "plots/e2e/", `the drill dialog opens over the Plot dialog with its folder (${header()}, ${testid("drill-output-dir")?.value})`);
      buttonText(dialog(), /^Close$/)?.click();
      await sleep(500);
      t(header() === "Plot", "closing it shows the Plot dialog again");
      await close();
      const put = await post({ op: "set_plot_settings", layers: before?.layers ?? [], use_aux_origin: before?.use_aux_origin ?? false, plot_on_all_layers: before?.plot_on_all_layers ?? [], options: before?.options ?? {} });
      t(put.ok, `the plot settings are put back (${JSON.stringify(put).slice(0, 120)})`);
      await until(async () => (await plotParams())?.options?.outputdirectory === (before?.options?.outputdirectory ?? ""), 15000);
      rev0 = await version();
    });

    await scenario("Generate Drill Files (X2): Gerber X2 drill files with a map, an Excellon map in PDF, inches and suppressed zeros, and the report file", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateDrillFiles", "Generate Drill Files"))) return;
      t(testid("drill-map-format")?.value === "gerberx2" && testid("drill-generate-map")?.checked === false, "the map: Gerber X2, unchecked (dialog_gendrill_base.cpp)");
      t(testid("drill-precision")?.textContent === "3:3", `Excellon in mm: precision 3:3 (${testid("drill-precision")?.textContent})`);
      labelled("Gerber X2")?.querySelector("input")?.click();
      await sleep(200);
      t(testid("drill-units")?.disabled === true && box("Mirror Y axis")?.disabled === true && box("Generate tenting layers")?.disabled === false, "Gerber X2: the Excellon controls disabled, the tenting box enabled (onFileFormatSelection)");
      t(/^4\.[56]$/.test(testid("drill-precision")?.textContent ?? ""), `Gerber X2: the board's precision (${testid("drill-precision")?.textContent})`);
      testid("drill-generate-map")?.click();
      await sleep(1100);
      let res = await press(/^Generate$/, "fab/drill");
      let files = judged(t, "Gerber X2 drill", res, /\.gbr$/i);
      t(files.some((f) => /-drl_map\.gbr$/.test(f)) && files.some((f) => /-drl\.gbr$/.test(f)), `drill files and maps (${files.map(base).join(", ")})`);
      t(res.reply?.req?.format === "gerber" && res.reply?.req?.generate_map === true, "the request says so");
      labelled("Excellon")?.querySelector("input")?.click();
      await sleep(200);
      await choose(testid("drill-map-format"), "pdf");
      await choose(testid("drill-units"), "in");
      await choose(testid("drill-zeros"), "suppressleading");
      t(testid("drill-precision")?.textContent === "2:4", "inches: 2:4");
      await sleep(1100);
      res = await press(/^Generate$/, "fab/drill");
      files = res.reply?.body?.files ?? [];
      t(res.reply?.body?.ok === true && files.some((f) => /\.drl$/.test(f)) && files.some((f) => /-drl_map\.pdf$/.test(f)), `Excellon with PDF maps (${files.map(base).join(", ")})`);
      res = await press(/^Generate Report File\.\.\.$/, "fab/drill_report");
      t(res.reply?.body?.ok === true && /^Report file '.*-drl\.rpt' created\.$/.test(res.reply?.body?.message ?? ""), `the report file (${res.reply?.body?.message})`);
      // Back to KiCad's defaults for the next scenarios (the dialog keeps its choices).
      testid("drill-generate-map")?.click();
      await choose(testid("drill-units"), "mm");
      await choose(testid("drill-zeros"), "decimal");
      await choose(testid("drill-map-format"), "gerberx2");
    });

    await scenario("Generate Placement Files: CSV with both sides in KiCad's -all-pos file, or the -top-pos and -bottom-pos ones; ASCII", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generatePosFile", "Generate Placement Files"))) return;
      const radio = (name, i) => dialog().querySelectorAll(`input[name="${name}"]`)[i];
      t(radio("pos-format", 0)?.checked === true && !dialog().querySelector('input[name="pos-side"]'), "Plain text first, and no side choice (KiCad's dialog always does both sides)");
      // X4: kicad-cli 10.99's `pcb export pos` has no Exclude from BOM flag, and the studio no design variants: both shown disabled; the output folder is the dialog's.
      t(box("Exclude all footprints with the Exclude from BOM flag set")?.disabled === true && labelled("Design variant:")?.querySelector("select")?.disabled === true && !!testid("pos-output-dir"), "Exclude from BOM and the design variant disabled, the output directory offered");
      radio("pos-format", 1)?.click(); // CSV
      await sleep(150);
      let res = await press(/^Generate/, "fab/pos");
      let files = judged(t, "CSV", res, /\.csv$/i);
      t(files.length === 1 && /-all-pos\.csv$/.test(files[0]), `both sides in one -all-pos file (${files.map(base).join(", ")})`);
      t(res.reply?.req?.format === "csv" && res.reply?.req?.side === "both" && res.reply?.req?.single_file === true, `asked for CSV, both sides, one file (${JSON.stringify(res.reply?.req)})`);
      // The checkbox off, as KiCad's dialog offers: the top's and the bottom's files, named as `PLACE_FILE_EXPORTER::DecorateFilename` names them.
      const single = box("Generate single file");
      if (t(!!single, "the dialog has KiCad's single-file box")) {
        single.click();
        res = await press(/^Generate/, "fab/pos");
        files = judged(t, "CSV, two files", res, /\.csv$/i);
        t(files.length === 2 && files.some((f) => /-top-pos\.csv$/.test(f)) && files.some((f) => /-bottom-pos\.csv$/.test(f)), `the top's and the bottom's files (${files.map(base).join(", ")})`);
        t(res.reply?.req?.single_file === false, "the server was told the box is off");
        single.click(); // back to one file
      }
      radio("pos-format", 0)?.click(); // ASCII
      await sleep(150);
      res = await press(/^Generate/, "fab/pos");
      files = judged(t, "ASCII", res, /\.pos$/i);
      t(files.length === 1 && /-all\.pos$/.test(files[0]), `ASCII, both sides, one file (${files.map(base).join(", ")})`);
      t(/^Placement file: '.*'\.\nComponent count: \d+\.\nDone\.$/.test(res.reply?.body?.message ?? ""), `CreateAsciiFiles' report (${JSON.stringify(res.reply?.body?.message)})`);
      radio("pos-format", 2)?.click(); // Gerber X3
      await sleep(150);
      res = await press(/^Generate/, "fab/pos");
      files = judged(t, "Gerber X3", res, /\.gbr$/i);
      t(files.length === 2 && files.some((f) => /-pnp_top\.gbr$/.test(f)) && files.some((f) => /-pnp_bottom\.gbr$/.test(f)), `Gerber X3: -pnp_top and -pnp_bottom (${files.map(base).join(", ")})`);
      radio("pos-format", 0)?.click();
    });

    const exportOne = (action, title, route, re) => async (t) => {
      if (!(await open(t, action, title))) return;
      const res = await press(/^Export$/, route);
      judged(t, title, res, re);
      t(typeof res.reply?.body?.engine !== "string" || res.reply.body.engine.startsWith("kicad-cli"), `kicad-cli wrote it (${res.reply?.body?.engine ?? "-"})`);
    };
    await scenario("Export STEP: a model scaled (2,1,1) brings KiCad's Model Scale Warning when the dialog opens (r3 X3-4)", async (t) => {
      const part = st0.parts.find((p) => p.placed && (p.models ?? []).length > 0);
      if (!t(part, `a placed part with a 3D model (${part?.ref})`)) return;
      const r = await post({ op: "edit_board_model", part: part.ref, index: 0, scale: [2, 1, 1] });
      if (!t(r.ok, `edit_board_model (${JSON.stringify(r).slice(0, 160)})`)) return;
      const v = await version();
      await until(() => __eda.state().revision === v, 10000);
      await sleep(300);
      if (await open(t, "pcbnew.EditorControl.exportSTEP", "Export STEP")) {
        const w = dialog().querySelector("[data-testid=step-scale-warning]");
        t(w && w.textContent.includes("Scaled models detected.  Model scaling is not reliable for mechanical export."), `the warning, word for word (${w?.textContent.slice(0, 160)})`);
        t(w && w.textContent.includes("Non-unity scaled models:") && w.textContent.includes(part.models[0].name), "with the model's file in the details");
        [...(w?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === "OK")?.click();
        await sleep(200);
        t(!dialog().querySelector("[data-testid=step-scale-warning]"), "OK dismisses it");
        await close();
      }
      await run("common.Interactive.undo");
      await until(async () => { const s = await fetch("/api/state", { cache: "no-store" }).then((x) => x.json()); return s.parts.find((p) => p.ref === part.ref)?.models?.[0]?.scale?.[0] === 1; }, 15000);
      rev0 = await version();
    });

    await scenario("Export STEP: the board and its models as one .step", exportOne("pcbnew.EditorControl.exportSTEP", "Export STEP", "fab/3d", /\.(step|stp)$/i));
    await scenario("Export VRML: the board as a .wrl", exportOne("pcbnew.EditorControl.exportVRML", "Export VRML", "fab/vrml", /\.wrl$/i));
    await scenario("IPC-D-356 Netlist File: the board's test netlist", exportOne("pcbnew.EditorControl.generateD356File", "IPC-D-356", "fab/ipcd356", /\.(d356|ipc)$/i));
    await scenario("Export IPC-2581: one .xml", exportOne("pcbnew.EditorControl.generateIPC2581File", "Export IPC-2581", "fab/ipc2581", /\.(xml|zip)$/i));
    await scenario("Export IPC-2581's BOM columns (r4 X4-4): choices of the footprints' field names, the distributor guessed from its P/N column, kept for the next opening and no undo step", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateIPC2581File", "Export IPC-2581"))) return;
      const pick = (title) => [...dialog().querySelectorAll("select")].find((sel) => sel.title === title);
      const distPn = pick("Column containing the distributor part number");
      const dist = () => dialog().querySelector("[data-testid=ipc2581-dist]");
      const names = [...(distPn?.options ?? [])].map((o) => o.textContent.trim());
      t(names[0] === "Omit" && ["Datasheet", "Description", "Reference", "Value"].every((n) => names.includes(n)), `Distributor P/N offers Omit and the footprints' fields (${names.join(",")})`);
      const sorted = names.slice(1);
      t(JSON.stringify(sorted) === JSON.stringify([...sorted].sort()), "sorted, once each (a std::set)");
      const d0 = dist();
      t(d0 && d0.disabled && d0.value === "N/A", `Distributor is "N/A" and disabled while no column is chosen (${d0?.value}, ${d0?.disabled})`);
      if (distPn) {
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(distPn, "Value");
        distPn.dispatchEvent(new Event("change", { bubbles: true }));
        await sleep(200);
      }
      const d1 = dist();
      t(d1 && !d1.disabled && d1.value === "N/A", `a column whose name names no distributor enables Distributor and keeps "N/A" (${d1?.value})`);
      const res = await press(/^Export$/, "fab/ipc2581");
      t(res.reply?.body?.ok === true && res.reply?.req?.col_dist_pn === "Value" && res.reply?.req?.col_dist === "", `the export names the column and no distributor (${JSON.stringify(res.reply?.req)})`);
      await close();
      const kept = await until(async () => { const s = await fetch("/api/state", { cache: "no-store" }).then((x) => x.json()); return s.plot_params?.ipc2581_bom?.dist_pn === "Value" ? s : null; }, 15000);
      t(kept, "the column is kept in the board");
      const v = await version();
      await until(() => __eda.state().revision === v, 10000); // the page's poll picks the kept columns up
      await sleep(300);
      if (await open(t, "pcbnew.EditorControl.generateIPC2581File", "Export IPC-2581")) {
        t(pick("Column containing the distributor part number")?.value === "Value", "and the next opening starts from it (TransferDataToWindow)");
        await close();
      }
      await run("common.Interactive.undo");
      await sleep(1500);
      const still = await fetch("/api/state", { cache: "no-store" }).then((x) => x.json());
      t(still.plot_params?.ipc2581_bom?.dist_pn === "Value", "Ctrl+Z leaves it: the project file has no undo");
      const put = await post({ op: "set_ipc2581_bom", bom: {} });
      t(put.ok, "the columns are cleared again");
      rev0 = await version();
    });
    await scenario("Export ODB++: the archive", exportOne("pcbnew.EditorControl.generateODBPPFile", "Export ODB++", "fab/odb", /\.(zip|tgz|tar|odb)$|odb/i));
    await scenario("Bill of Materials (the board's): one .csv", exportOne("pcbnew.EditorControl.generateBOM", "Bill of Materials", "fab/pcb_bom", /\.csv$/i));

    await scenario("Plot Schematic: PDF of every sheet in one file, SVG one file per sheet", async (t) => {
      [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "Schematic")?.click();
      await sleep(1200);
      if (!(await open(t, "common.Control.plot", "Plot Schematic"))) return;
      const radio = (text) => labelled(text)?.querySelector("input");
      // `dialog_plot_schematic_base.cpp`: PDF selected, "Plot background color" unchecked.
      t(radio("PDF")?.checked === true && radio("Plot background color")?.checked === false, "the dialog starts at PDF with no background colour, as KiCad's");
      radio("PDF")?.click();
      await sleep(150);
      let res = await press(/^Plot All Pages$/, "sch/plot");
      const pdf = judged(t, "PDF", res, /\.pdf$/i);
      t(pdf.length === 1, `one PDF (${pdf.length})`);
      radio("SVG")?.click();
      await sleep(150);
      res = await press(/^Plot All Pages$/, "sch/plot");
      const svg = judged(t, "SVG", res, /\.svg$/i);
      const sheets = (await fetch("/api/sch/hierarchy", { cache: "no-store" }).then((r) => r.json()).catch(() => null));
      const count = (n) => (n ? 1 + (n.children ?? []).reduce((a, c) => a + count(c), 0) : 0);
      const want = sheets?.root ? count(sheets.root) : Array.isArray(sheets?.sheets) ? sheets.sheets.length : null;
      t(want == null || svg.length === want, `one SVG per sheet (${svg.length}${want != null ? ` for ${want}` : ""})`);
      await close();
      [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click();
      await sleep(800);
    });

    await scenario("Plot Schematic (X3): PostScript, DXF and PNG, the theme and the PDF options; Export Netlist: every format", async (t) => {
      [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "Schematic")?.click();
      await sleep(1200);
      if (!(await open(t, "common.Control.plot", "Plot Schematic"))) return;
      const radio = (text) => labelled(text)?.querySelector("input");
      // The dialog keeps the last plot's choices (DIALOG_SHIM's saved state): the scenario before left it at SVG.
      radio("PDF")?.click();
      await sleep(200);
      const themes = [...(testid("sch-plot-theme")?.options ?? [])].map((o) => o.textContent.trim());
      t(JSON.stringify(themes) === JSON.stringify(["KiCad Classic", "KiCad Default"]), `the colour themes kicad-cli knows (${themes.join(", ")})`);
      t(box("Generate property popups")?.disabled === false, "PDF: its options enabled");
      for (const [label, ext] of [["Postscript", "ps"], ["DXF", "dxf"], ["PNG", "png"]]) {
        radio(label)?.click();
        await sleep(200);
        if (label === "DXF") t(box("Generate property popups")?.disabled === true && box("Plot background color")?.disabled === true, "DXF: no PDF options, no background (onPlotFormatSelection, onColorMode)");
        await sleep(1100);
        const res = await press(/^Plot All Pages$/, "sch/plot");
        const files = judged(t, `schematic ${label}`, res, new RegExp(`\\.${ext}$`, "i"));
        t(res.reply?.req?.format === ext, `${label}: asked for ${ext}`);
      }
      radio("PDF")?.click();
      await sleep(150);
      await close();
      if (!(await open(t, "eeschema.EditorControl.exportNetlist", "Export Netlist"))) return;
      for (const [id, ext] of [["kicad", "net"], ["orcadpcb2", "net"], ["allegro", "txt"], ["cadstar", "frp"], ["pads", "asc"], ["spice", "cir"], ["spicemodel", "cir"], ["xml", "xml"]]) {
        testid(`netlist-tab-${id}`)?.click();
        await sleep(1200);
        if (id === "spice") t(box("Save all voltages")?.disabled === true, "SPICE: its options disabled (kicad-cli has no flag)");
        const res = await press(/^Export Netlist$/, "sch/netlist");
        const files = (res.reply?.body?.files ?? []).filter((f) => !/\/devices$/.test(f));
        t(res.reply?.body?.ok === true && files.length === 1 && files[0].endsWith(`.${ext}`), `${id}: ${files.map(base).join(", ")}`);
      }
      await close();
      [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click();
      await sleep(800);
    });

    await scenario("no output edited the design, and the page logged no error", async (t) => {
      t((await version()) === rev0, `the revision is the same (${rev0} -> ${await version()})`);
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
