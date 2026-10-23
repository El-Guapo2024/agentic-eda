// Board Setup, the pages board-setup-pages.check.js does not drive (GAPS.md item 3 and item 33, package P20): Physical Stackup (the copper layer count),
// Solder Mask/Paste, Text & Graphics > Defaults, Dimensions, Design Rules > Constraints, Pre-defined Sizes (a track width and a via size added), Teardrops,
// Custom Rules and Violation Severity. Each page is opened from the dialog's tree, edited through its own fields (their aria-labels and labels), applied with
// its own button, read back from the server (`GET /api/state`: `board_rules`, `layers`, `routing`'s presets and teardrop settings, `drawings`'
// dimension settings), and taken back with one Undo after the dialog is closed; the last check compares everything with how it was at the start.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir> --port N --ui web/studio/dist`) with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/
// web/studio/e2e/board-setup-rules.check.js`, or pasted into the Browser pane's javascript tool, on a COPY of a board.
// It answers through `window.__boardSetupRulesCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __boardSetupRulesCheck.wait(30000)` until `.done`.
// run-all-checks: board=mcu30 minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__boardSetupRulesCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  const run = (id, args) => __eda.run(id, args);
  const dialog = () => document.querySelector(".bs-dialog");
  const page = (label) => document.querySelector(`section.bs-page[aria-label="${label}"]`);
  const buttonIn = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const aria = (root, label) => root?.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    if (!el) throw new Error(`no field to set to ${v}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const openPage = async (label) => {
    if (!dialog()) { await run("pcbnew.EditorControl.boardSetup"); await sleep(500); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith(label))?.click();
    await sleep(350);
    const p = page(label);
    if (!p || p.hidden) throw new Error(`Board Setup has no "${label}" page on screen`);
    return p;
  };
  const closeDialog = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      const b = [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((x) => ["Close", "Discard and close", "OK", "Cancel"].includes(x.textContent.trim()));
      b?.click();
      await sleep(300);
    }
  };
  /** Presses Apply (or `label`) on a page and waits for the server to have it: `changed(state)` true. */
  const apply = async (root, changed, label = "Apply") => {
    const b = buttonIn(root, label);
    if (!b) throw new Error(`no "${label}" button`);
    const r0 = await fetch("/api/version").then((r) => r.text());
    b.click();
    const st = await until(async () => { const s = await state(); return changed(s) ? s : null; }, 15000);
    await until(async () => (await fetch("/api/version").then((r) => r.text())) !== r0, 3000);
    return { st, message: root.querySelector(".bs-msg")?.textContent ?? "" };
  };
  /** Closes the dialog and takes the last step back with Undo; resolves to the state after. */
  const undoAfter = async () => { await closeDialog(); await run("common.Interactive.undo"); await sleep(800); return state(); };
  const J = (v) => JSON.stringify(v ?? null);
  /** Everything the pages here edit, to compare with the start. */
  const fingerprint = (s) => J({
    layers: s.layers, constraints: s.board_rules?.constraints, mask: s.board_rules?.mask_paste, text: s.board_rules?.text_graphics, stackup: s.board_rules?.stackup ?? null,
    sev: s.board_rules?.severities ?? {}, rules: s.board_rules?.custom_rules_text ?? null, widths: s.routing?.track_width_presets ?? [], vias: s.routing?.via_presets ?? [],
    td: s.routing?.teardrop_settings ?? null, dims: s.drawings?.dimension_settings ?? null,
  });

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const before = fingerprint(await state());
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialog(); } catch { /* ignore */ }
    const after = fingerprint(await state());
    if (after !== before) { entry.notes.push(`FAIL: the scenario left the board changed: ${before.slice(0, 200)} -> ${after.slice(0, 200)}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  if (!__eda.state().units || __eda.state().units !== "mm") { await run("common.Control.metricUnits"); await sleep(300); }
  const s0 = await state();
  const start = fingerprint(s0);

  (async () => {
    await scenario("Constraints: Minimum clearance and Minimum track width typed and applied are the board's (set_constraints), one undo step", async (t) => {
      const p = await openPage("Constraints");
      const c0 = (await state()).board_rules?.constraints;
      t(!!c0, `the board reports its constraints (${J(c0).slice(0, 120)})`);
      const clr = aria(p, "Minimum clearance"), trk = aria(p, "Minimum track width");
      t(!!clr && !!trk, "the page has Minimum clearance and Minimum track width");
      const want = { clr: (c0?.min_clearance_nm ?? 200) + 50, trk: (c0?.min_track_width_nm ?? 200) + 25 };
      setValue(clr, want.clr / 1000); setValue(trk, want.trk / 1000);
      await sleep(150);
      const { st } = await apply(p, (s) => s.board_rules?.constraints?.min_clearance_nm === want.clr);
      t(st?.board_rules?.constraints?.min_clearance_nm === want.clr && st?.board_rules?.constraints?.min_track_width_nm === want.trk, `the board has ${want.clr} and ${want.trk} um (${J(st?.board_rules?.constraints).slice(0, 160)})`);
      t(st?.board_rules?.constraints_explicit === true, "they are the board's own now (constraints_explicit)");
      // A value below zero is refused by the page.
      const p2 = await openPage("Constraints");
      setValue(aria(p2, "Minimum clearance"), -1);
      await sleep(200);
      t(!!p2.querySelector(".bs-invalid") || /must|invalid|negative|greater/i.test(p2.innerText), `a negative clearance is marked wrong (${p2.querySelector(".bs-error, .bs-row .bs-error")?.textContent ?? ""})`);
      buttonIn(p2, "Revert")?.click();
      await sleep(200);
      const back = await undoAfter();
      t(J(back.board_rules?.constraints) === J(c0), "one undo gives the old constraints back");
    });

    await scenario("Pre-defined Sizes: a track width and a via size added and applied are the board's presets, one undo step", async (t) => {
      const p = await openPage("Pre-defined Sizes");
      const w0 = s0.routing?.track_width_presets ?? [], v0 = s0.routing?.via_presets ?? [];
      buttonIn(p, "Add Track Width").click();
      await sleep(200);
      const widths = [...p.querySelectorAll("input[type=number]")];
      setValue(widths[w0.length], 0.35);
      buttonIn(p, "Add Via Size").click();
      await sleep(200);
      const nums = [...p.querySelectorAll("input[type=number]")];
      const dia = nums[nums.length - 2], drill = nums[nums.length - 1];
      setValue(dia, 0.9); setValue(drill, 0.45);
      await sleep(150);
      const { st } = await apply(p, (s) => (s.routing?.track_width_presets ?? []).includes(350) && (s.routing?.via_presets ?? []).some((v) => v.diameter === 900));
      t((st?.routing?.track_width_presets ?? []).includes(350), `the track widths have 0.35 mm (${J(st?.routing?.track_width_presets)})`);
      t((st?.routing?.via_presets ?? []).some((v) => v.diameter === 900 && v.drill === 450), `the via sizes have 0.9 / 0.45 mm (${J(st?.routing?.via_presets)})`);
      const back = await undoAfter();
      t(J(back.routing?.track_width_presets ?? []) === J(w0) && J(back.routing?.via_presets ?? []) === J(v0), "one undo takes both away");
    });

    await scenario("Solder Mask/Paste: the mask expansion, a tent box and the paste clearance applied are the board's (set_mask_paste), one undo step", async (t) => {
      const p = await openPage("Solder Mask/Paste");
      const m0 = (await state()).board_rules?.mask_paste;
      t(!!m0, `the board reports its mask and paste settings (${J(m0)})`);
      setValue(aria(p, "Solder mask expansion"), 0.05);
      setValue(aria(p, "Solder paste clearance"), -0.03);
      const front = [...p.querySelectorAll("label")].find((l) => l.textContent.trim() === "Front")?.querySelector("input");
      front?.click();
      await sleep(150);
      const { st } = await apply(p, (s) => s.board_rules?.mask_paste?.expansion_nm === 50);
      const m = st?.board_rules?.mask_paste;
      t(m?.expansion_nm === 50 && m?.paste_margin_nm === -30, `expansion 50 um, paste clearance -30 um (${J(m)})`);
      t(!!front && m?.tent_vias_front === !m0?.tent_vias_front, `Tent vias: Front was switched (${m0?.tent_vias_front} -> ${m?.tent_vias_front})`);
      const back = await undoAfter();
      t(J(back.board_rules?.mask_paste) === J(m0), "one undo gives them back");
    });

    await scenario("Text & Graphics > Defaults: the silkscreen line thickness and text height applied are the board's (set_text_graphics_defaults), one undo step", async (t) => {
      const p = await openPage("Defaults");
      const d0 = (await state()).board_rules?.text_graphics;
      t(!!d0, "the board reports its defaults");
      setValue(aria(p, "Silk Layers Line Thickness"), 0.15);
      setValue(aria(p, "Silk Layers Text Height"), 1.2);
      await sleep(150);
      const { st } = await apply(p, (s) => s.board_rules?.text_graphics?.silk?.line_width_nm === 150);
      const silk = st?.board_rules?.text_graphics?.silk;
      t(silk?.line_width_nm === 150 && silk?.text_height_nm === 1200, `silk: line 150 um, text height 1200 um (${J(silk)})`);
      t(J(st?.board_rules?.text_graphics?.copper) === J(d0?.copper), "the other rows are as they were");
      const back = await undoAfter();
      t(J(back.board_rules?.text_graphics) === J(d0), "one undo gives them back");
    });

    await scenario("Dimensions: units, precision and the arrow length applied are what a new dimension starts from (dimension_settings), one undo step", async (t) => {
      const p = await openPage("Dimensions");
      const d0 = (await state()).drawings?.dimension_settings ?? null;
      const selects = [...p.querySelectorAll("select")];
      setValue(selects[0], "mil");
      const nums = [...p.querySelectorAll("input[type=number]")];
      setValue(nums[0], 2); // precision
      const arrow = [...p.querySelectorAll(".kv-grid > span")].find((s) => s.textContent.trim() === "Arrow length")?.nextElementSibling?.querySelector("input");
      setValue(arrow, 1.5);
      await sleep(150);
      const { st } = await apply(p, (s) => s.drawings?.dimension_settings?.units === "mil", "Apply Settings");
      const d = st?.drawings?.dimension_settings;
      t(d?.units === "mil" && d?.precision === 2 && d?.arrow_length === 1500, `mils, 2 places, a 1.5 mm arrow (${J(d)})`);
      const back = await undoAfter();
      t(J(back.drawings?.dimension_settings ?? null) === J(d0), "one undo gives the old ones back");
    });

    await scenario("Teardrops: Enable teardrops and the best length ratio applied are the board's teardrop settings, one undo step", async (t) => {
      const p = await openPage("Teardrops");
      const td0 = (await state()).routing?.teardrop_settings ?? null;
      const enable = [...p.querySelectorAll("label")].find((l) => l.textContent.trim().startsWith("Enable teardrops"))?.querySelector("input");
      t(!!enable, "the page has Enable teardrops");
      if (enable && !enable.checked) enable.click();
      const ratio = [...p.querySelectorAll(".kv-grid > span")].find((s) => s.textContent.trim().startsWith("Best length ratio"))?.nextElementSibling;
      setValue(ratio, 0.6);
      await sleep(150);
      const { st } = await apply(p, (s) => s.routing?.teardrop_settings?.enabled === true && s.routing?.teardrop_settings?.best_length_ratio === 0.6, "Apply Settings");
      t(st?.routing?.teardrop_settings?.enabled === true && st?.routing?.teardrop_settings?.best_length_ratio === 0.6, `enabled, ratio 0.6 (${J(st?.routing?.teardrop_settings)})`);
      const back = await undoAfter();
      t(J(back.routing?.teardrop_settings ?? null) === J(td0), "one undo gives the old settings back");
    });

    await scenario("Custom Rules: a rule typed and applied is the board's .kicad_dru text (set_custom_rules), one undo step", async (t) => {
      const p = await openPage("Custom Rules");
      const r0 = (await state()).board_rules?.custom_rules_text ?? null;
      const text = '(version 1)\n(rule "p20_wide"\n  (condition "A.NetClass == \'Default\'")\n  (constraint track_width (min 0.2mm)))\n';
      setValue(aria(p, "DRC Rules"), text);
      await sleep(200);
      const { st } = await apply(p, (s) => (s.board_rules?.custom_rules_text ?? "").includes("p20_wide"));
      t((st?.board_rules?.custom_rules_text ?? "").includes('(rule "p20_wide"'), `the board's rules text has the rule (${J(st?.board_rules?.custom_rules_text).slice(0, 120)})`);
      const back = await undoAfter();
      t((back.board_rules?.custom_rules_text ?? null) === r0, "one undo gives the old text back");
    });

    await scenario("Violation Severity: a check set to Ignore and applied is the board's severity for it (set_rule_severities), one undo step", async (t) => {
      const p = await openPage("Violation Severity");
      const sev0 = (await state()).board_rules?.severities ?? {};
      const group = p.querySelector('[role="radiogroup"][aria-label="Tracks crossing"]');
      t(!!group, "the page lists Tracks crossing");
      const ignore = [...(group?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim() === "Ignore")?.querySelector("input");
      ignore?.click();
      await sleep(150);
      const { st } = await apply(p, (s) => s.board_rules?.severities?.tracks_crossing === "ignore");
      t(st?.board_rules?.severities?.tracks_crossing === "ignore", `tracks_crossing is ignored (${J(st?.board_rules?.severities)})`);
      const back = await undoAfter();
      t((back.board_rules?.severities?.tracks_crossing ?? "error") === (sev0.tracks_crossing ?? "error"), "one undo gives it its old severity");
    });

    await scenario("Physical Stackup: four copper layers applied make a four-layer board (In1.Cu, In2.Cu), one undo step back to two", async (t) => {
      const p = await openPage("Physical Stackup");
      const l0 = (await state()).layers;
      const sel = aria(p, "Copper layers");
      t(!!sel && Number(sel.value) === l0.length, `Copper layers reads ${sel?.value} for ${l0.length}`);
      setValue(sel, 4);
      await sleep(300);
      t([...p.querySelectorAll("input[aria-label$=' thickness']")].length > 4, "the table grew the inner layers and their dielectrics");
      const { st } = await apply(p, (s) => (s.layers ?? []).length === 4);
      t(J(st?.layers) === J(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), `the board's copper layers (${J(st?.layers)})`);
      const back = await undoAfter();
      t(J(back.layers) === J(l0), `one undo makes it ${l0.length} layers again (${J(back.layers)})`);
    });

    await scenario("every page is as it was at the start, and the page logged no error", async (t) => {
      const now = fingerprint(await state());
      t(now === start, `the board is as it was${now === start ? "" : `: ${start.slice(0, 300)} -> ${now.slice(0, 300)}`}`);
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
