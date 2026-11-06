// UI-LEFT X5 and B5 (`task-u5-dialogs`): what the export dialogs say, and the text variables the studio shows.
// - Plot Schematic's messages are `SCH_PLOTTER`'s report as kicad-cli printed it ("Plotted to '%s'.", "Done."), and the Plot dialog's are its report with any
//   warning kicad-cli gave first (`FabReply.warnings`, `eda_kicad_engine::warnings_of`).
// - A schematic text box with `${REV}` and `@{3*4}` is shown expanded (`graphic_shown`, `SCH_TEXTBOX::GetShownText`), a power symbol whose Value is `${RAIL}`
//   shows `+5V`, and a board text's `@{..}` and `${VCSSHORTHASH}` are expanded in the board's state (`shown`); every edit is undone at the end.
// Run it in the page of a served placed board -- `eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`, kicad-cli
// on the PATH -- with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/exports-u5.check.js`.
// It answers through `window.__exportsU5Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (window.__exportsU5Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const undo = (domain) => json("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain }) }).catch(() => null);

  const replies = [];
  if (!window.__exportsU5Fetch) {
    const real = window.fetch.bind(window);
    window.__exportsU5Fetch = real;
    window.fetch = async (input, init) => {
      const r = await real(input, init);
      const url = typeof input === "string" ? input : input?.url ?? "";
      const m = /\/api\/(fab\/[a-z0-9_]+|sch\/plot|sch\/netlist)(\?|$)/.exec(url);
      if (m && (init?.method ?? "GET") === "POST") r.clone().json().then((body) => replies.push({ route: m[1], body })).catch(() => undefined);
      return r;
    };
  }
  const dialog = () => [...document.querySelectorAll(".dialog")].filter((d) => d.offsetParent !== null).pop() ?? null;
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
  const buttonText = (re) => [...(dialog()?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const close = async () => { for (let i = 0; i < 3 && dialog(); i++) { buttonText(/^(Close|Cancel)$/)?.click(); await sleep(300); } };
  const tab = async (name) => { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === name)?.click(); await sleep(1000); };
  const press = async (re, route) => {
    const n = replies.length;
    buttonText(re)?.click();
    const reply = await until(() => replies.slice(n).find((x) => x.route === route), 120000);
    await until(() => dialog()?.querySelector(".panel-empty, .problem-row"), 5000);
    return { reply, message: dialog()?.querySelector(".panel-empty b, .problem-row b")?.textContent ?? "" };
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await close(); } catch { /* ignore */ }
    results.push(entry);
  };

  (async () => {
    const errorsFrom = Date.now();
    await scenario("Plot Schematic: the messages are SCH_PLOTTER's report as kicad-cli printed it", async (t) => {
      await tab("Schematic");
      await __eda.run("common.Control.plot");
      await sleep(500);
      if (!t(header()?.startsWith("Plot Schematic"), `the dialog (${header()})`)) return;
      const res = await press(/^Plot All Pages$/, "sch/plot");
      t(res.reply?.body?.ok === true, `kicad-cli plotted (${JSON.stringify(res.reply?.body?.files ?? res.reply?.body?.message)})`);
      t(/Plotted to '.+'\./.test(res.message) && /Done\./.test(res.message), `"Plotted to '..'." and "Done." (${res.message.slice(0, 160)})`);
    });
    await scenario("Plot: the report, with no warning on a clean run", async (t) => {
      await tab("PCB");
      await __eda.run("pcbnew.EditorControl.generateGerbers");
      await sleep(500);
      if (!t(header()?.startsWith("Plot"), `the dialog (${header()})`)) return;
      const res = await press(/^Plot$/, "fab/plot");
      t(res.reply?.body?.ok === true, "kicad-cli plotted");
      t(!res.reply?.body?.warnings, `no warning on this board (${JSON.stringify(res.reply?.body?.warnings)})`);
      t(/Plotted to '/.test(res.message) && /Done\./.test(res.message), `the report (${res.message.slice(0, 120)})`);
    });
    await scenario("a text box's and a power symbol's variables are shown expanded", async (t) => {
      const before = await fetch("/api/version").then((r) => r.text());
      const steps = [];
      const step = async (cmd, domain) => { const r = await post(cmd); if (r.ok) steps.push(domain); return r; };
      t((await step({ op: "set_text_variables", variables: { REV: "B7", RAIL: "+5V" } }, "pcb")).ok, "the project's variables");
      const box = { shape: { type: "text_box", start: { x: 50_800_000, y: 50_800_000 }, end: { x: 101_600_000, y: 63_500_000 }, text: "Rev ${REV} @{3*4}", size_nm: 1_270_000 } };
      t((await step({ op: "sch_edit", verb: "add_graphic", graphic: box }, "schematic")).ok, "a text box");
      t((await step({ op: "add_power_symbol", lib_id: "power:+5V", at: { x: 25_400_000, y: 25_400_000 }, rot_millideg: 0, net: "${RAIL}", pin: "" }, "schematic")).ok, "a power symbol");
      const sch = await json("/api/schematic");
      const shownBox = Object.values(sch.graphic_shown ?? {});
      t(shownBox.includes("Rev B7 12"), `the text box shows "Rev B7 12" (${JSON.stringify(shownBox)})`);
      const power = (sch.power_symbols ?? []).find((p) => p.net === "${RAIL}");
      const value = power?.fields?.find((f) => f.name === "Value");
      t(value?.shown === "+5V", `the power symbol shows +5V (${JSON.stringify(value)})`);
      await until(() => __eda.getStudio?.()?.schematic?.graphic_shown || true, 2000);
      t((await step({ op: "add_text", text: { content: "N@{2*3} ${VCSSHORTHASH}", at: { x: 5_000_000, y: 5_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false } }, "pcb")).ok, "a board text");
      const st = await json("/api/state");
      const text = (st.drawings?.texts ?? []).find((x) => x.content === "N@{2*3} ${VCSSHORTHASH}");
      t(/^N6 /.test(text?.shown ?? "") && !(text?.shown ?? "").includes("${"), `the board text is expanded (${text?.shown})`);
      while (steps.length) await undo(steps.pop());
      const after = await json("/api/schematic");
      t(Object.keys(after.graphic_shown ?? {}).length === 0 && !(after.power_symbols ?? []).some((p) => p.net === "${RAIL}"), `undone (from ${before})`);
    });
    await scenario("the page logged no error", async (t) => {
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
