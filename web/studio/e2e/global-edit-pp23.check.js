// Global edit, round 2 check (UI-LEFT-2 PP23, PARITY-pcb.md section 107): Edit Text & Graphics Properties reaches a footprint's dimensions ("Footprint dimensions"),
// text boxes and hidden texts ("Footprint text items") and courtyard graphics ("Footprint graphic items"), and its value boxes take NUMERIC_EVALUATOR's expressions
// ("0.1+0.05", "2*0.6", "10mil") -- worked out when the box loses the focus, the text as typed back when it takes it again. Each Apply and Close is one undo step.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`) -- through
// `e2e/run-check.mjs` -- on a COPY of a board: it pastes a footprint of its own (PPK1: a dimension, a text box, a hidden text, a courtyard rectangle) and undoes it all.
// It answers through `window.__globalEditPp23Check` = { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__globalEditPp23Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };
  const send = async (c) => { const rev0 = E.state().revision; const r = await cmd(c); await waitFor(() => E.state().revision !== rev0, 8000); await sleep(400); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const dlg = () => document.querySelector('[data-testid="global-edit-text-graphics"]');
  const byAria = (a) => dlg()?.querySelector(`[aria-label="${a}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  /** Type `v` into the value box `a` the way a person does: focus, type, leave (React's onFocus / onBlur are focusin / focusout). */
  const typeInto = async (a, v) => {
    const el = byAria(a);
    el.focus();
    el.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await sleep(60);
    setValue(el, v);
    await sleep(60);
    el.blur();
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await sleep(150);
  };
  const counts = () => {
    const m = /(\d+) field\(s\), (\d+) footprint text\(s\), (\d+) footprint graphic\(s\), (\d+) footprint dimension\(s\), (\d+) graphic item\(s\), (\d+) text\(s\) and (\d+) dimension\(s\)/.exec(document.querySelector('[data-testid="global-edit-text-graphics-count"]')?.textContent ?? "");
    return m ? m.slice(1).map(Number) : null;
  };
  const closeDialog = async () => { const d = dlg(); const b = d && buttonText(d, "Close"); if (b) { b.click(); await sleep(250); } };
  const open = async () => { await E.run("pcbnew.GlobalEdit.editTextAndGraphics"); await sleep(600); };
  const apply = async () => { const rev0 = E.state().revision; buttonText(dlg(), "Apply and Close").click(); await waitFor(() => E.state().revision !== rev0, 8000); await sleep(600); };
  /** Only PPK1's items: By parent reference designator. */
  const onlyPpk1 = async () => { setValue(byAria("Reference filter"), "PPK1"); await sleep(150); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialog(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await waitFor(() => E.state().tab === "pcb"); }
  const outline = (await board()).outline ?? [[0, 0], [35_000_000, 0], [35_000_000, 35_000_000], [0, 35_000_000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1e6, Math.min(...outline.map((p) => p[1])) / 1e6];
  const mm = (v) => Math.round(v * 1e6);
  const clip = `(kicad_pcb (version 20241229) (generator "pcbnew")
  (footprint "Pp23:Probe" (layer "F.Cu") (at 0 0 0)
    (property "Reference" "PPK1" (at 0 -7 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Probe" (at 0 7 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
    (fp_text user "\${REFERENCE}" (at 0 2 0) (layer "F.Fab") (hide yes) (effects (font (size 1 1) (thickness 0.15))))
    (fp_text_box "NOTE" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (dimension (type aligned) (layer "F.Fab") (pts (xy -4 4) (xy 4 4)) (height 1) (format (units 3) (units_format 1) (precision 4)) (style (thickness 0.1) (arrow_length 1.27) (text_position_mode 0) (extension_height 0.58642) (extension_offset 0.5) keep_text_aligned) (gr_text "8.0000 mm" (at 0 3 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15)))))
    (fp_rect (start -6 -6) (end 6 6) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  ))`;
  const ppk = async () => (await board()).parts.find((p) => p.ref === "PPK1" && p.placed);

  (async () => {
    const r = await send({ op: "paste_clipboard", text: clip, at: { x: mm(bx0 + 10), y: mm(by0 + 9) } });
    const p0 = await ppk();
    if (!p0) {
      results.push({ name: "setup", ok: false, notes: [`FAIL: the paste made no PPK1 (${JSON.stringify(r).slice(0, 300)})`] });
      check.done = true;
      return;
    }
    const pasted = JSON.stringify(p0);

    await scenario("PPK1 carries a dimension, a text box, a hidden text and a courtyard graphic the global edit can name", async (t) => {
      t((p0.items?.dimensions ?? []).length === 1 && (p0.items?.text_boxes ?? []).length === 1, `its dimension and text box (${JSON.stringify(Object.keys(p0.items ?? {}))})`);
      t((p0.hidden_texts ?? []).length === 1 && p0.hidden_texts[0].role === "reference", `its hidden \${REFERENCE} text (${JSON.stringify(p0.hidden_texts)})`);
      t((p0.courtyard_graphics ?? []).length === 1 && p0.courtyard_graphics[0].layer === "F.CrtYd", `its courtyard rectangle (${JSON.stringify(p0.courtyard_graphics)})`);
    });

    await scenario("Footprint dimensions picks the footprint's dimension; the boxes take expressions, shown worked out on leaving and as typed on coming back; one undo step", async (t) => {
      await open();
      byAria("Footprint dimensions").click(); await sleep(120);
      await onlyPpk1();
      t(JSON.stringify(counts()) === "[0,0,0,1,0,0,0]", `one footprint dimension (${counts()})`);
      await typeInto("Line thickness", "0.1+0.05");
      t(byAria("Line thickness").value === "0.15", `"0.1+0.05" shows 0.15 once the box loses the focus (${byAria("Line thickness").value})`);
      byAria("Line thickness").focus(); byAria("Line thickness").dispatchEvent(new FocusEvent("focusin", { bubbles: true })); await sleep(150);
      t(byAria("Line thickness").value === "0.1+0.05", `and the text as typed when it takes the focus again (${byAria("Line thickness").value})`);
      byAria("Line thickness").blur(); byAria("Line thickness").dispatchEvent(new FocusEvent("focusout", { bubbles: true })); await sleep(150);
      await typeInto("Text height", "2*0.6");
      await typeInto("Text width", "w=0.5; w*2+10mil");
      t(byAria("Text width").value === "1.254", `variables and a unit: "w=0.5; w*2+10mil" is 1.254 (${byAria("Text width").value})`);
      await apply();
      t(!dlg(), "Apply and Close closes the dialog");
      const d = (await ppk()).items.dimensions[0];
      t(d.stroke_width === 150_000 && d.text_size_nm === 1_200_000, `the footprint's dimension has 0.15 mm lines and 1.2 mm text (${d.stroke_width}, ${d.text_size_nm})`);
      await undo();
      t(JSON.stringify(await ppk()) === pasted, "one Undo puts PPK1 back as it was pasted");
    });

    await scenario("Footprint text items reaches the text box and the hidden text; Footprint graphic items the courtyard; one undo step each", async (t) => {
      await open();
      byAria("Footprint text items").click(); await sleep(120);
      await onlyPpk1();
      const c = counts();
      t(c[1] === 2, `the hidden \${REFERENCE} text and the text box (${c})`);
      await typeInto("Text height", "1.5");
      await apply();
      const p1 = await ppk();
      t(p1.items.text_boxes[0].size === 1_500_000, `the text box is 1.5 mm high (${p1.items.text_boxes[0].size})`);
      await undo();
      t(JSON.stringify(await ppk()) === pasted, "one Undo puts it back");

      await open();
      byAria("Footprint graphic items").click(); await sleep(120);
      await onlyPpk1();
      setValue(byAria("Layer filter"), "F.CrtYd"); await sleep(150);
      t(JSON.stringify(counts()) === "[0,0,1,0,0,0,0]", `By layer F.CrtYd: the courtyard rectangle alone (${counts()})`);
      await typeInto("Line thickness", "3mil");
      const rev0 = E.state().revision;
      await apply();
      t(E.state().revision !== rev0 && !dlg(), "the edit is taken");
      await undo();
      t(JSON.stringify(await ppk()) === pasted, "one Undo puts it back");
    });

    // the pasted footprint goes
    await undo();
    const gone = !(await ppk());
    results.push({ name: "cleanup", ok: gone, notes: [`${gone ? "ok" : "FAIL"}: undoing the paste takes PPK1 away`] });
    check.ok = results.every((x) => x.ok);
    check.done = true;
  })();
  return "started";
})();
