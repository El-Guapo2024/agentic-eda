// Text and dimensions, round 2 (docs/parity/UI-LEFT-2.md PP11; PARITY-pcb.md section 100), in the page:
//   * Text Properties has KiCad's Italic (`DIALOG_TEXT_PROPERTIES::m_italic`): ticked on a text, Save writes it in one undo step, the derived .kicad_pcb says
//     `italic`, Undo takes it back.
//   * A dimension's label is its own text: Dimension Properties' Width, Height, Bold, Italic, Align left and Mirrored (`updateDimensionFromDialog`), and the
//     Font when the server has DejaVu Sans, are kept, drawn (`window.__edaText.pcb`: a left-justified mirrored label reaches left of its anchor, twice as wide
//     glyphs twice as wide a label) and written (`(effects (font .. (size H W) .. bold italic) (justify left mirror))`); one Undo takes it all back.
//   * The Properties panel's Horizontal Justification, Italic and Keep Upright rows of the dimension are writeable, one undoable edit each.
//   * While a dimension is drawn (`DRAWING_TOOL::DrawDimension`): Ctrl++ / Ctrl+- (`incWidth` / `decWidth`) step its line by 0.1 mm, a change of the active layer
//     gives it that layer's text and line, the height of a slanted aligned dimension is off the grid; E opens its dialog and, closed, ends the tool with nothing
//     added (KiCad's `break` out of the loop).
// Run it in the page of a served COPY of a board with an outline and room in it: `eda board new examples/ldo.yaml -o <dir>` (a 26 mm square). It answers
// through `window.__dimsTextCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__dimsTextCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden page throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const mm = (v) => Math.round(v * 1_000_000);
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const state = () => json("/api/state");
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const boardFile = () => fetch("/api/board.kicad_pcb").then((r) => r.text());
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const waitFor = async (pred, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await pred(); if (v) return v; await sleep(150); } return null; };
  const setValue = (el, value) => {
    const proto = el.tagName === "SELECT" ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event(el.tagName === "SELECT" ? "change" : "input", { bubbles: true }));
  };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + mm(wx) * v.scale, r.top + v.y + mm(wy) * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1) }));
  };
  const moveTo = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y, { buttons: 0 }); await sleep(200); };
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y, { buttons: 0 });
    await sleep(150);
    fire("pointerdown", x, y);
    await sleep(40);
    fire("pointerup", x, y);
    await sleep(400);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(200); } };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); await E.run("common.InteractiveSelection.selectItems", ids); await sleep(400); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  const dims = async () => (await state()).drawings?.dimensions ?? [];
  const dimOf = async (id) => (await dims()).find((d) => d.id === id);
  // The dimension the draw tool previews: what it sends to `/api/dimension/preview` (the drawing's own dimension).
  const preview = { last: null, n: 0 };
  const realFetch = window.fetch.bind(window);
  window.fetch = async (url, init) => {
    if (String(url).includes("/api/dimension/preview") && init?.body) { try { preview.last = JSON.parse(init.body).dimension; preview.n++; } catch { /* not ours */ } }
    return realFetch(url, init);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (E.state().units !== undefined && E.state().units !== "mm") await E.run("common.Control.metricUnits");
  await E.run("common.Control.metricUnits");
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);

  await scenario("Text Properties: Italic ticked is written in one undo step and taken back by Undo", async (t) => {
    const before = (await state()).drawings?.texts ?? [];
    await post({ op: "add_text", text: { content: "SLANT", at: { x: mm(8), y: mm(20) }, angle: 0, layer: "F.SilkS", size_nm: mm(1), stroke_width: mm(0.15), justify: "center", mirror: false } });
    const text = await waitFor(async () => ((await state()).drawings?.texts ?? []).find((x) => x.content === "SLANT" && !before.some((b) => b.id === x.id)));
    t(!!text, "a text to edit");
    if (!text) return;
    await select([text.id]);
    await E.run("pcbnew.InteractiveEdit.properties");
    const box = await waitFor(() => tid("text-italic"));
    t(!!box && !box.checked, "Text Properties has Italic, unticked for an upright text");
    if (!box) return;
    box.click();
    await sleep(100);
    [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Save")?.click();
    const slanted = await waitFor(async () => ((await state()).drawings?.texts ?? []).find((x) => x.id === text.id && x.italic));
    t(!!slanted, "the text is italic");
    const file = await boardFile();
    const at = file.indexOf('(gr_text "SLANT"');
    t(at >= 0 && /italic/.test(file.slice(at, file.indexOf("(uuid", at) + 300)), "the derived .kicad_pcb writes it italic");
    await undo();
    const back = ((await state()).drawings?.texts ?? []).find((x) => x.id === text.id);
    t(!!back && !back.italic, "one Undo makes it upright again");
    await undo(); // the text added for the check
  });

  let dimId = null;
  await scenario("Dimension Properties: the label's width, height, bold, italic, left justification, mirror and font are kept, drawn, written and undone", async (t) => {
    const before = await dims();
    await post({
      op: "add_dimension",
      dimension: {
        layer: "Cmts.User", kind: { kind: "aligned", height: mm(-2) }, start: { x: mm(4), y: mm(8) }, end: { x: mm(20), y: mm(8) }, prefix: "", suffix: "", units: "mm", units_format: "no_suffix",
        precision: 2, suppress_trailing_zeros: true, text_position: "outside", keep_text_aligned: true, text_angle: 0, text_size_nm: mm(1), text_thickness_nm: mm(0.15), stroke_width: mm(0.1),
        arrow_length: mm(1.27), extension_offset: mm(0.5), extension_height: 586_420, arrow_direction: "outward",
      },
    });
    const d0 = await waitFor(async () => (await dims()).find((d) => !before.some((b) => b.id === d.id)));
    t(!!d0, "a dimension to edit");
    if (!d0) return;
    dimId = d0.id;
    await sleep(500);
    const ink0 = window.__edaText?.pcb?.[d0.id]?.ink;
    t(!!ink0, `its label is drawn (${JSON.stringify(ink0)})`);
    await select([d0.id]);
    await E.run("pcbnew.InteractiveEdit.properties");
    const ok = await waitFor(() => tid("dim-ok"));
    t(!!ok && !!tid("dim-text-width") && !!tid("dim-bold") && !!tid("dim-italic") && !!tid("dim-align-left") && !!tid("dim-mirrored"), "the Dimension Text box: Width, Height, Thickness, Bold, Italic, the alignments, Mirrored");
    if (!ok) return;
    setValue(tid("dim-text-width"), "2");
    setValue(tid("dim-text-height"), "1.5");
    await sleep(50);
    tid("dim-bold").click();
    tid("dim-italic").click();
    tid("dim-align-left").click();
    tid("dim-mirrored").click();
    await sleep(50);
    const thick = Number(tid("dim-text-thickness")?.value);
    t(near(thick, 0.3, 0.0005), `Bold sets the bold pen of the smaller size: ${thick} mm`);
    const font = document.querySelector('[data-testid="dimension-properties"] select[aria-label="Font"]');
    const dejavu = font && [...font.options].some((o) => o.value === "DejaVu Sans");
    if (dejavu) setValue(font, "DejaVu Sans");
    await sleep(50);
    tid("dim-ok").click();
    const d1 = await waitFor(async () => { const d = await dimOf(d0.id); return d && d.bold && d.italic ? d : null; });
    t(!!d1, "the label is bold and italic");
    if (!d1) return;
    t(d1.text_width_nm === mm(2) && d1.text_size_nm === mm(1.5), `2 mm wide, 1.5 mm high (${d1.text_width_nm}, ${d1.text_size_nm})`);
    t(d1.h_justify === "left" && d1.mirrored === true, `left justified, mirrored (${d1.h_justify}, ${d1.mirrored})`);
    if (dejavu) t(d1.face === "DejaVu Sans", `in DejaVu Sans (${d1.face})`);
    await sleep(800);
    const rec = window.__edaText?.pcb?.[d0.id];
    const ink1 = rec?.ink;
    t(!!ink1 && ink1[2] <= d1.text_at[0] + mm(0.3), `a mirrored left-justified label reaches left of its anchor (ink ${JSON.stringify(ink1)}, anchor x ${d1.text_at[0]})`);
    t(!!ink1 && !!ink0 && ink1[2] - ink1[0] > 1.6 * (ink0[2] - ink0[0]), `twice as wide glyphs draw a much wider label (${ink0 && ink0[2] - ink0[0]} -> ${ink1 && ink1[2] - ink1[0]})`);
    if (dejavu) t(rec?.font && rec.font !== "stroke", `drawn in an outline face (${rec?.font})`);
    const file = await boardFile();
    const block = file.slice(file.indexOf("(dimension"), file.indexOf("(dimension") + 1200);
    t(/\(size 1\.5 2\)/.test(block) && /bold italic\)/.test(block) && /\(justify left mirror\)/.test(block), "the derived .kicad_pcb writes (size 1.5 2) .. bold italic) (justify left mirror)");
    if (dejavu) t(/\(face "DejaVu Sans"\)/.test(block), "and the face");
    await undo();
    const d2 = await dimOf(d0.id);
    t(!!d2 && !d2.bold && !d2.italic && d2.h_justify === "center" && d2.mirrored === false && d2.text_width_nm === mm(1) && d2.text_size_nm === mm(1) && !d2.face, "one Undo takes every look back");
  });

  await scenario("Properties panel: the label's Horizontal Justification, Italic and Keep Upright rows edit the dimension, one undo step each", async (t) => {
    if (!dimId) { t(false, "no dimension"); return; }
    document.querySelector('button[aria-label="Show Properties"]')?.click();
    await select([dimId]);
    const input = (name) => document.querySelector(`.prop-row[data-prop="${name}"] [data-prop-input]`);
    const lastT = async () => (await state()).activity?.[0]?.t;
    for (const [name, value, read] of [["Horizontal Justification", "right", (d) => d.h_justify === "right"], ["Italic", true, (d) => d.italic], ["Keep Upright", true, (d) => d.keep_upright]]) {
      const el = await waitFor(() => input(name));
      t(!!el && !el.disabled, `${name} is writeable`);
      if (!el) continue;
      const before = await lastT();
      if (el instanceof HTMLSelectElement) setValue(el, value);
      else el.click();
      const done = await waitFor(async () => { const d = await dimOf(dimId); return d && read(d) ? d : null; });
      t(!!done, `${name} = ${value} reaches the dimension`);
      t((await lastT()) !== before, `${name}: a command of the history`);
      await undo();
      const back = await dimOf(dimId);
      t(!!back && !read(back), `${name}: Undo takes it back`);
      await select([dimId]);
    }
  });

  await scenario("Drawing a dimension: the width keys step its line, a change of layer restyles it, a slanted height is off the grid", async (t) => {
    const before = await dims();
    const grid = E.state().grid;
    await E.run("pcbnew.InteractiveDrawing.alignedDimension");
    t(E.state().tool === "dimension", "the dimension tool is armed");
    await clickAt(4, 13);
    await moveTo(10, 15);
    await sleep(300);
    const w0 = preview.last?.stroke_width;
    t(w0 > 0, `the preview has its line (${w0})`);
    await E.run("pcbnew.InteractiveDrawing.incWidth");
    const w1 = await waitFor(() => (preview.last?.stroke_width === w0 + 100_000 ? preview.last.stroke_width : null));
    t(w1 === w0 + 100_000, `Ctrl++ adds 0.1 mm to the line (${w0} -> ${preview.last?.stroke_width})`);
    await E.run("pcbnew.InteractiveDrawing.decWidth");
    await waitFor(() => preview.last?.stroke_width === w0);
    t(preview.last?.stroke_width === w0, `Ctrl+- takes it back (${preview.last?.stroke_width})`);
    // the active layer changes: the dimension takes F.Cu's text and line
    await E.run("pcbnew.Control.layerTop");
    const moved = await waitFor(() => (preview.last?.layer === "F.Cu" ? preview.last : null));
    t(!!moved, `the dimension goes to the new active layer (${preview.last?.layer})`);
    if (moved) t(moved.text_size_nm === mm(1.5) && moved.text_thickness_nm === mm(0.3), `with the copper class's text (${moved.text_size_nm}, ${moved.text_thickness_nm})`);
    // the end, slanted; then the height off the grid
    await clickAt(14, 18);
    const g = grid || mm(0.1);
    const hx = 6 + (0.43 * g) / 1e6, hy = 20 + (0.37 * g) / 1e6;
    await moveTo(hx, hy);
    await clickAt(hx, hy);
    const made = await waitFor(async () => (await dims()).find((d) => !before.some((b) => b.id === d.id)));
    t(!!made && made.layer === "F.Cu", `the dimension is added on F.Cu (${made?.layer})`);
    if (made) {
      const [sx, sy, ex, ey] = [made.start[0], made.start[1], made.end[0], made.end[1]];
      const a = Math.atan2(ey - sy, ex - sx) + Math.PI / 2;
      const raw = (mm(hx) - ex) * Math.cos(a) + (mm(hy) - ey) * Math.sin(a);
      const snapped = [Math.round(mm(hx) / g) * g, Math.round(mm(hy) / g) * g];
      const onGrid = (snapped[0] - ex) * Math.cos(a) + (snapped[1] - ey) * Math.sin(a);
      t(sx !== ex && sy !== ey, `a slanted dimension (${made.start} -> ${made.end})`);
      t(Math.abs(made.height - raw) < Math.abs(made.height - onGrid) && Math.abs(made.height - raw) < 0.15 * g, `its height follows the cursor off the grid: ${made.height} (cursor ${Math.round(raw)}, grid ${Math.round(onGrid)})`);
      t(made.stroke_width === mm(0.2) || made.stroke_width > 0, `its line (${made.stroke_width})`);
      await undo();
    }
    await leaveTool();
  });

  await scenario("Drawing a dimension: E opens its dialog, and closing it ends the tool with nothing added (DrawDimension's break)", async (t) => {
    await E.run("pcbnew.Control.layerTop");
    const before = await dims();
    await E.run("pcbnew.InteractiveDrawing.alignedDimension");
    await clickAt(4, 22);
    await moveTo(12, 22);
    await E.run("pcbnew.InteractiveEdit.properties");
    const dialog = await waitFor(() => tid("dimension-properties"));
    t(!!dialog, "Dimension Properties opens on the dimension being drawn");
    if (!dialog) return;
    [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Cancel")?.click();
    await sleep(600);
    t(!tid("dimension-properties"), "the dialog is closed");
    t(E.state().tool === "select", `the tool is done (${E.state().tool})`);
    await clickAt(14, 22);
    await sleep(400);
    t((await dims()).length === before.length, "nothing was added");
  });

  // the dimension added for the check
  if (dimId) await post({ op: "delete_dimension", id: dimId });
  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
  window.fetch = realFetch;
  return check.ok ? "ok" : "failed";
})();
