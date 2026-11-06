// Texts drawn where KiCad draws them, in the stroke font and in outline fonts (UI-LEFT-2 PP1, CC1; PARITY-pcb section 93):
//   * PP1: a board text's lines start where `FONT::getLinePositions` puts them (left/top: `pen / 1.52` in and a size less `0.052` pens down; centred: half the
//     whole advance back, the block's 1.17 sizes halved), as the canvas drew them (`window.__edaText.pcb[id]`, components/text/kicadTextPaint.ts);
//   * CC1: a text set in DejaVu Sans (`edit_text_style`'s face) is drawn in the face the server's fontconfig matched (`GET /api/fonts/match`, the file served by
//     `/api/fonts/file`, loaded as a `FontFace`), and its glyphs' box is kicad-cli's: the boxes below are kicad-cli 10.99's `pcb export svg` of the same texts on this
//     box (left/top, centred, right/bottom, two lines, turned 90 degrees, a faked italic, the bold face, mirrored and twice as high as wide), within 0.006 mm;
//   * the Text Properties dialog's Font choice sets the face in one undo step, and a footprint field takes a face through its layout.
// Run it in the page of a served COPY of a placed board (it edits and undoes); it answers through `window.__textFontsCheck`: { results, done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__textFontsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__textFontsSched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 6e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__textFontsSched = true;
  }

  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const waitFor = async (cond, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };
  const postSeen = async (cmd) => {
    const rev0 = __eda.state().revision;
    const reply = await post(cmd);
    await waitFor(() => __eda.state().revision !== rev0, 8000);
    await sleep(400);
    return reply;
  };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const drawn = (id) => (window.__edaText && window.__edaText.pcb ? window.__edaText.pcb[id] : undefined);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await __eda.run("common.Control.zoomFitObjects");
  await sleep(500);
  const st0 = await state();
  const outline = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const [bx0, by1] = outline.length ? [Math.min(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))] : [0, 0];
  // a free stretch below the board, on whole millimetres: the texts sit 20 mm apart, as in the kicad-cli plot
  const [ox, oy] = [Math.round(bx0) + 10, Math.round(by1) + 15];
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const textsNow = async () => (await state()).drawings?.texts ?? [];
  /** Adds a 1 mm text with a 0.1 mm pen at (x, y) mm and its looks; its id. */
  const addText = async (content, x, y, { justify = "left", angle = 0, mirror = false, size = 1, style = null } = {}) => {
    const before = (await textsNow()).map((s) => s.id);
    await postSeen({ op: "add_text", text: { content, at: mm(x, y), angle, layer: "F.SilkS", size_nm: Math.round(size * 1e6), stroke_width: 100_000, justify, mirror } });
    const made = (await textsNow()).find((s) => !before.includes(s.id));
    if (made && style) await postSeen({ op: "edit_text_style", id: made.id, ...style });
    return made?.id ?? null;
  };
  const near = (a, b, tol) => Math.abs(a - b) <= tol;

  (async () => {
    await scenario("PP1: a stroke text's lines start where FONT::getLinePositions puts them, as drawn", async (t) => {
      const lt = await addText("H", ox, oy, { style: { valign: "top" } });
      const cc = await addText("H", ox + 20, oy, { justify: "center" });
      const rb = await addText("H", ox + 40, oy, { justify: "right", style: { valign: "bottom" } });
      t(!!lt && !!cc && !!rb, "three texts added");
      if (!lt || !cc || !rb) return;
      await waitFor(() => !!drawn(lt) && !!drawn(cc) && !!drawn(rb));
      const X = ox * 1e6;
      const Y = oy * 1e6;
      const a = drawn(lt)?.lines?.[0];
      // left/top: pen / 1.52 in, a size less 0.052 pens down
      t(!!a && a.x === X + 65_789 && a.y === Y + 994_800, `left/top baseline starts at (+65789, +994800) nm: ${a && [a.x - X, a.y - Y]}`);
      const b = drawn(cc)?.lines?.[0];
      // centred: half of H's whole advance (22/21 mm) back, the block's 1.17 mm halved
      t(!!b && b.x === X + 20e6 - 523_809 && b.y === Y + 994_800 - 585_000, `centred baseline starts at (-523809, +409800) nm: ${b && [b.x - X - 20e6, b.y - Y]}`);
      const c = drawn(rb)?.lines?.[0];
      t(!!c && c.x === X + 40e6 - 1_047_619 - 65_789 && c.y === Y + 994_800 - 1_170_000, `right/bottom baseline starts at (-1113408, -175200) nm: ${c && [c.x - X - 40e6, c.y - Y]}`);
      // the ink of the left/top H: its left stroke 0.238 of the size in, the pen's half outside it (kicad-cli plots the stroke at 50.3039 for a text at 50)
      const ink = drawn(lt)?.ink;
      t(!!ink && near(ink[0], X + 303_884 - 50_000, 2) && near(ink[1], Y + 994_800 - 952_381 - 50_000, 2), `the H's strokes as kicad-cli plots them: ${ink && [ink[0] - X, ink[1] - Y]}`);
      t(drawn(lt)?.font === "stroke", "in the stroke font");
    });

    await scenario("CC1: the server's fontconfig gives DejaVu Sans as KiCad would load it", async (t) => {
      const m = await json("/api/fonts/match?face=DejaVu%20Sans");
      t(m.ok && m.found && /\.ttf$/.test(m.file) && m.result === "ok", `DejaVu Sans is a file of its own (${JSON.stringify(m)})`);
      const it = await json("/api/fonts/match?face=DejaVu%20Sans&italic=1");
      t(it.ok && it.found && it.fake_italic === true, `its italic is the Book face slanted (FF_MISSING_ITAL: ${JSON.stringify(it)})`);
      const r = await fetch("/api/fonts/file?face=DejaVu%20Sans");
      t(r.ok && (r.headers.get("content-type") || "").startsWith("font/"), `the file is served (${r.status} ${r.headers.get("content-type")})`);
    });

    // kicad-cli 10.99's plot of these texts (DejaVu Sans, 1 mm, at the cell's corner): the box of the polygons it fills, mm from the text's position
    const plotted = [
      { text: "H", dx: 0, dy: 20, opt: { style: { valign: "top" } }, box: [0.1373, -0.02, 0.9147, 1.0] },
      { text: "Hxg", dx: 20, dy: 20, opt: { justify: "center" }, box: [-1.247, -0.605, 1.2573, 0.7042] },
      { text: "H", dx: 40, dy: 20, opt: { justify: "right", style: { valign: "bottom" } }, box: [-0.9149, -1.19, -0.1375, -0.17] },
      { text: "H\nH", dx: 60, dy: 20, opt: { justify: "center" }, box: [-0.3888, -1.445, 0.3886, 1.255] },
      { text: "H", dx: 0, dy: 40, opt: { angle: 90_000 }, box: [-0.605, -0.9147, 0.415, -0.1373] },
      { text: "H", dx: 20, dy: 40, opt: { style: { italic: true } }, box: [0.1343, -0.605, 1.1067, 0.415] },
      { text: "H", dx: 40, dy: 40, opt: { style: { bold: true } }, box: [0.1284, -0.605, 1.0424, 0.415] },
      { text: "H", dx: 60, dy: 40, opt: { mirror: true, size: 2, style: { width_nm: 1_000_000 } }, box: [-0.9147, -1.2099, -0.1373, 0.83] },
    ];
    await scenario("CC1: texts in DejaVu Sans are drawn in it, where kicad-cli plots their glyphs", async (t) => {
      const ids = [];
      for (const p of plotted) {
        const id = await addText(p.text, ox + p.dx, oy + p.dy, p.opt);
        if (id) await postSeen({ op: "edit_text_style", id, face: "DejaVu Sans" });
        ids.push(id);
      }
      t(ids.every(Boolean), "eight texts added, each set in DejaVu Sans");
      const st = await state();
      t(ids.every((id) => st.drawings.texts.find((x) => x.id === id)?.face === "DejaVu Sans"), "their face is kept (the text's extra)");
      const loaded = await waitFor(() => ids.every((id) => (drawn(id)?.font ?? "stroke") !== "stroke"), 20000);
      t(loaded, `drawn in the outline face once it has loaded (${ids.map((id) => drawn(id)?.font).join(", ")})`);
      plotted.forEach((p, i) => {
        const ink = ids[i] ? drawn(ids[i])?.ink : null;
        const [X, Y] = [(ox + p.dx) * 1e6, (oy + p.dy) * 1e6];
        const got = ink ? [(ink[0] - X) / 1e6, (ink[1] - Y) / 1e6, (ink[2] - X) / 1e6, (ink[3] - Y) / 1e6].map((v) => +v.toFixed(4)) : null;
        // a fake italic's box is that of each glyph's slanted box: as wide as kicad-cli's here (H's top corner is its widest)
        t(!!got && got.every((v, k) => near(v, p.box[k], 0.006)), `${JSON.stringify(p.text)} ${JSON.stringify(p.opt)}: ${JSON.stringify(got)} vs kicad-cli ${JSON.stringify(p.box)}`);
      });
    });

    await scenario("CC1: Text Properties' Font sets the face in one undo step", async (t) => {
      const id = await addText("Font", ox, oy + 60);
      t(!!id, "a text added");
      if (!id) return;
      await select([id]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      await sleep(500);
      const dlg = document.querySelector('[data-testid="text-dialog"]');
      const font = dlg?.querySelector('select[aria-label="Font"]');
      t(!!font && font.value === "KiCad Font", `the dialog's Font is KiCad Font (${font && font.value})`);
      t(!!font && [...font.options].some((o) => o.value === "DejaVu Sans") && ![...font.options].some((o) => o.value === "Default Font"), "the server's families are offered, no Default Font (pcbnew's FONT_CHOICE)");
      if (!font) return;
      setValue(font, "DejaVu Sans");
      await sleep(150);
      const rev0 = __eda.state().revision;
      buttonText(dlg, "Save").click();
      await waitFor(() => __eda.state().revision !== rev0);
      await sleep(400);
      t((await textsNow()).find((x) => x.id === id)?.face === "DejaVu Sans", "the text is in DejaVu Sans");
      await waitFor(() => (drawn(id)?.font ?? "stroke") !== "stroke", 10000);
      t((drawn(id)?.font ?? "stroke") !== "stroke", "and drawn in it");
      await undo();
      const back = (await textsNow()).find((x) => x.id === id);
      t(!!back && !back.face, `one undo takes the face back (${back && back.face})`);
    });

    await scenario("CC1: a footprint field takes a face through its layout and is drawn in it", async (t) => {
      const part = st0.parts.find((p) => p.placed && (p.fields ?? []).some((f) => f.name === "Reference" && f.visible));
      const f = part?.fields?.find((x) => x.name === "Reference");
      t(!!f, `a placed footprint's Reference (${part && part.ref})`);
      if (!f) return;
      const layout = { at: { x: f.lx, y: f.ly }, angle: f.langle, size: [f.w, f.h], thickness: f.thickness, layer: f.layer, visible: f.visible, halign: f.halign, valign: f.valign, mirror: f.mirror, bold: f.bold, italic: f.italic, keep_upright: f.upright, knockout: f.knockout, face: "DejaVu Sans" };
      await postSeen({ op: "edit_board_field", part: part.ref, name: "Reference", layout });
      const nf = (await state()).parts.find((p) => p.ref === part.ref)?.fields?.find((x) => x.name === "Reference");
      t(nf?.face === "DejaVu Sans", `the field's face is DejaVu Sans (${nf && nf.face})`);
      await waitFor(() => (drawn(f.id)?.font ?? "stroke") !== "stroke", 10000);
      t((drawn(f.id)?.font ?? "stroke") !== "stroke", `drawn in it (${drawn(f.id)?.font})`);
      await undo();
    });

    // put the board back: every text this check added goes
    const added = (await textsNow()).filter((x) => !(st0.drawings?.texts ?? []).some((s) => s.id === x.id)).map((x) => x.id);
    if (added.length) await postSeen({ op: "batch", cmds: added.map((id) => ({ op: "delete_text", id })) });
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
