// Plot and fabrication outputs check (GAPS.md item 33, package P20): File > Fabrication Outputs and File > Export of the board editor and File > Plot of the
// schematic editor, every file written by kicad-cli. The Plot dialog (Gerbers: the default layer list, one file per layer and the job file, a layer left out
// is not plotted), Generate Drill Files (one merged .drl, or PTH and NPTH apart), Generate Placement Files (CSV and ASCII, per side), STEP, VRML, IPC-D-356,
// IPC-2581, ODB++ and the board's Bill of Materials, and Plot Schematic (PDF, SVG one file per sheet). Each is opened with its action, run with its own
// button, and judged by what the server answered (`/api/fab/*`, `/api/sch/plot`, captured as they come back: `ok`, the engine and the files written in
// the board's `export/kicad/<kind>/`) and by the dialog's result panel; none of them edits the design (the revision is the same at the end).
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
      const m = /\/api\/(fab\/[a-z0-9_]+|sch\/plot)(\?|$)/.exec(url);
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
  const dialog = () => document.querySelector(".dialog");
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
  const buttonText = (root, re) => [...(root?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const labelled = (text) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim().startsWith(text));
  const box = (text) => labelled(text)?.querySelector("input");
  const close = async () => { for (let i = 0; i < 3 && dialog(); i++) { buttonText(dialog(), /^(Close|Cancel)$/)?.click(); await sleep(300); } };
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
  const rev0 = await version();
  if (!st0.parts?.some((p) => p.placed) || !(st0.routing?.tracks ?? []).length) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs placed footprints and routed tracks (eda pipeline examples/opamp_filter.yaml)"] }); check.done = true; return "no board"; }

  (async () => {
    await scenario("Plot (Gerbers): the default layers are the copper, mask, paste, silkscreen and outline; Plot writes one Gerber per layer and the job file; a layer left out is not plotted", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateGerbers", "Plot"))) return;
      const ticked = [...dialog().querySelectorAll("label.toggle input[type=checkbox]")].filter((i) => i.checked && !i.dataset.testid).map((i) => i.closest("label").textContent.trim());
      t(copper.every((l) => ticked.some((x) => x.startsWith(l))) && ["Edge.Cuts"].every((l) => ticked.some((x) => x.startsWith(l))), `every copper layer and Edge.Cuts start ticked (${ticked.join(", ")})`);
      let res = await press(/^Plot$/, "fab/gerbers");
      const files = judged(t, "Gerbers", res, /\.(g[a-z0-9]+|gbrjob)$/i);
      t(files.some((f) => /\.gbrjob$/.test(f)), "with the job file (.gbrjob)");
      const gerbers = files.filter((f) => !/\.gbrjob$/.test(f));
      t(gerbers.length === (res.reply?.req?.layers ?? []).length, `one Gerber per layer asked for (${gerbers.length} for ${(res.reply?.req?.layers ?? []).length})`);
      t(res.reply?.req?.check_zones === true, "Check zone fills before plotting is on by default (DIALOG_PLOT::m_zoneFillCheck)");
      // Leave B.SilkS out.
      const silk = [...dialog().querySelectorAll("label.toggle")].find((l) => l.textContent.trim().startsWith("B.Silk"))?.querySelector("input");
      if (t(!!silk, "the list has B.Silkscreen")) {
        silk.click();
        // The server lists the files modified since a second before the run (`run_export`: mtimes are coarse on some file systems), so the second plot waits
        // that second out; otherwise the first run's B.SilkS file would be listed again.
        await sleep(1500);
        res = await press(/^Plot$/, "fab/gerbers");
        t(res.reply?.body?.ok === true && !(res.reply?.req?.layers ?? []).includes("B.SilkS"), `the second plot asks for the layers without B.SilkS (${(res.reply?.req?.layers ?? []).join(",")})`);
        t((res.reply?.body?.files ?? []).filter((f) => !/\.gbrjob$/.test(f)).length === gerbers.length - 1, `and writes one Gerber fewer (${(res.reply?.body?.files ?? []).length})`);
        silk.click();
      }
    });

    await scenario("Generate Drill Files: the plated and non-plated holes in two Excellon files (KiCad's default), or in one with \"PTH and NPTH in single file\"", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generateDrillFiles", "Generate Drill Files"))) return;
      let res = await press(/^Generate Drill File$/, "fab/drill");
      const apart = judged(t, "drill", res, /\.(drl|gbr)$/i);
      t(res.reply?.req?.merge_pth_npth === false && res.reply?.req?.alt_drill_mode === false, "two files and routed slots by default (m_Check_Merge_PTH_NPTH, m_altDrillMode unchecked)");
      t(apart.some((f) => /-PTH\.drl$/.test(f)), `apart: ${apart.map(base).join(", ")}`);
      box("PTH and NPTH in single file")?.click();
      await sleep(1500); // the second-long window of the file list, as above
      res = await press(/^Generate Drill File$/, "fab/drill");
      const merged = res.reply?.body?.files ?? [];
      t(res.reply?.body?.ok === true && res.reply?.req?.merge_pth_npth === true && !merged.some((f) => /PTH\.drl$/i.test(f)), `merged: ${merged.map(base).join(", ")}`);
      box("PTH and NPTH in single file")?.click();
    });

    await scenario("Generate Placement Files: CSV with both sides in KiCad's -all-pos file, or the -top-pos and -bottom-pos ones; ASCII", async (t) => {
      if (!(await open(t, "pcbnew.EditorControl.generatePosFile", "Generate Placement Files"))) return;
      const radio = (name, i) => dialog().querySelectorAll(`input[name="${name}"]`)[i];
      radio("pos-format", 1)?.click(); // CSV
      const both = [...dialog().querySelectorAll('input[name="pos-side"]')].find((x) => x.closest("label")?.textContent.includes("Both"));
      both?.click();
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
    });

    const exportOne = (action, title, route, re) => async (t) => {
      if (!(await open(t, action, title))) return;
      const res = await press(/^Export$/, route);
      judged(t, title, res, re);
      t(typeof res.reply?.body?.engine !== "string" || res.reply.body.engine.startsWith("kicad-cli"), `kicad-cli wrote it (${res.reply?.body?.engine ?? "-"})`);
    };
    await scenario("Export STEP: the board and its models as one .step", exportOne("pcbnew.EditorControl.exportSTEP", "Export STEP", "fab/3d", /\.(step|stp)$/i));
    await scenario("Export VRML: the board as a .wrl", exportOne("pcbnew.EditorControl.exportVRML", "Export VRML", "fab/vrml", /\.wrl$/i));
    await scenario("IPC-D-356 Netlist File: the board's test netlist", exportOne("pcbnew.EditorControl.generateD356File", "IPC-D-356", "fab/ipcd356", /\.(d356|ipc)$/i));
    await scenario("Export IPC-2581: one .xml", exportOne("pcbnew.EditorControl.generateIPC2581File", "Export IPC-2581", "fab/ipc2581", /\.(xml|zip)$/i));
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
      radio("Plot all pages")?.click();
      await sleep(150);
      let res = await press(/^Plot$/, "sch/plot");
      const pdf = judged(t, "PDF", res, /\.pdf$/i);
      t(pdf.length === 1, `one PDF (${pdf.length})`);
      radio("SVG")?.click();
      await sleep(150);
      res = await press(/^Plot$/, "sch/plot");
      const svg = judged(t, "SVG", res, /\.svg$/i);
      const sheets = (await fetch("/api/sch/hierarchy", { cache: "no-store" }).then((r) => r.json()).catch(() => null));
      const count = (n) => (n ? 1 + (n.children ?? []).reduce((a, c) => a + count(c), 0) : 0);
      const want = sheets?.root ? count(sheets.root) : Array.isArray(sheets?.sheets) ? sheets.sheets.length : null;
      t(want == null || svg.length === want, `one SVG per sheet (${svg.length}${want != null ? ` for ${want}` : ""})`);
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
