// Outline fonts, round 3 (UI-LEFT-3 C3-3, C3-8; PARITY-common, the fonts section):
//   * C3-8: a face fontconfig has no bold of (IPAGothic) is a fake bold (`FF_MISSING_BOLD`: `FT_Outline_Embolden( .., 1 << 6 )`), drawn from its emboldened outlines: the
//     glyph's box is kicad-cli 10.99's plot of the same text (one pixel of the 358 to the em wider and taller than the regular face's, up and right);
//   * C3-3: the server measures a text in its face -- a board table's cell in DejaVu Sans is broken to its column with that font's advances (`LinebreakText` through
//     `OUTLINE_FONT::GetTextAsGlyphs`), where the stroke font's narrower W's fit.
// Run it in the page of a served COPY of a placed board (it edits and undoes); it answers through `window.__fontsV16Check`: { results, done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=5
(async () => {
  const results = [];
  const check = (window.__fontsV16Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__fontsV16Sched) {
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
    window.__fontsV16Sched = true;
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
    // kicad-cli 10.99's plot of "H" in IPAGothic, 1 mm, left/top: the box of the polygon it fills, mm from the text's position (the bold one faked)
    const plotted = { bold: [0.0854, -0.0551, 0.6181, 0.9297], regular: [0.0854, -0.0512, 0.6141, 0.9297] };
    await scenario("C3-8: a fake bold is drawn emboldened, where kicad-cli plots it", async (t) => {
      const m = await json("/api/fonts/match?face=IPAGothic&bold=1");
      t(m.ok && m.found && m.fake_bold === true, `IPAGothic has no bold: FF_MISSING_BOLD (${JSON.stringify(m)})`);
      const bold = await addText("H", ox, oy, { style: { valign: "top", bold: true } });
      const regular = await addText("H", ox + 20, oy, { style: { valign: "top" } });
      t(!!bold && !!regular, "two texts added");
      if (!bold || !regular) return;
      await postSeen({ op: "edit_text_style", id: bold, face: "IPAGothic" });
      await postSeen({ op: "edit_text_style", id: regular, face: "IPAGothic" });
      const loaded = await waitFor(() => [bold, regular].every((id) => (drawn(id)?.font ?? "stroke") !== "stroke"), 20000);
      t(loaded, `drawn in the outline face once it has loaded (${[bold, regular].map((id) => drawn(id)?.font).join(", ")})`);
      const box = (id, dx) => {
        const ink = drawn(id)?.ink;
        const [X, Y] = [(ox + dx) * 1e6, oy * 1e6];
        return ink ? [(ink[0] - X) / 1e6, (ink[1] - Y) / 1e6, (ink[2] - X) / 1e6, (ink[3] - Y) / 1e6].map((v) => +v.toFixed(4)) : null;
      };
      const [b, r] = [box(bold, 0), box(regular, 20)];
      t(!!b && b.every((v, k) => near(v, plotted.bold[k], 0.006)), `the fake bold H: ${JSON.stringify(b)} vs kicad-cli ${JSON.stringify(plotted.bold)}`);
      t(!!r && r.every((v, k) => near(v, plotted.regular[k], 0.006)), `the regular H: ${JSON.stringify(r)} vs kicad-cli ${JSON.stringify(plotted.regular)}`);
      // FT_Outline_Embolden: the right and the top sides one pixel (1.4 / 358 mm) out, the left and the bottom ones where they were
      t(!!b && !!r && near(b[2] - r[2], 0.0039, 0.0006) && near(r[1] - b[1], 0.0039, 0.0006) && near(b[0], r[0], 0.0002) && near(b[3], r[3], 0.0002), `the bold one is a pixel wider and taller, up and right (${b && r && [b[2] - r[2], r[1] - b[1]]})`);
    });

    await scenario("C3-3: a board table's cell in DejaVu Sans is broken to its column in that font", async (t) => {
      const before = ((await state()).drawings?.tables ?? []).map((x) => x.id);
      await postSeen({ op: "table", verb: "draw", layer: "F.SilkS", at: mm(ox, oy + 20), cols: 1, rows: 1, cell_w_nm: 12_000_000, cell_h_nm: 4_000_000, line_width_nm: 120_000, patch: { texts: [["WWW WWW"]] } });
      const tb = ((await state()).drawings?.tables ?? []).find((x) => !before.includes(x.id));
      t(!!tb, "a table drawn");
      if (!tb) return;
      const shown = async () => { const c = ((await state()).drawings?.tables ?? []).find((x) => x.id === tb.id)?.cells?.[0]; return c ? c.shown ?? c.text : null; };
      // the stroke font's W is 24/21 of the size: "WWW WWW" fits the 12 mm cell less its margins
      const s0 = await shown();
      t(s0 === "WWW WWW", `in the stroke font it fits on one line (${JSON.stringify(s0)})`);
      await postSeen({ op: "table", verb: "edit_cells", id: tb.id, cells: [[0, 0]], edit: { face: "DejaVu Sans" } });
      const s1 = await shown();
      // DejaVu Sans' W is 2025/2048 of a 1.4-size em: the second word goes to a line of its own
      t(s1 === "WWW\nWWW", `in DejaVu Sans it is broken after the first word (${JSON.stringify(s1)})`);
      await undo();
      t((await shown()) === "WWW WWW", "one undo puts the stroke font back, and the line");
      await undo();
      t(!((await state()).drawings?.tables ?? []).some((x) => x.id === tb.id), "a second undo takes the table away");
    });

    // put the board back: every text this check added goes
    const added = (await textsNow()).filter((x) => !(st0.drawings?.texts ?? []).some((s) => s.id === x.id)).map((x) => x.id);
    if (added.length) await postSeen({ op: "batch", cmds: added.map((id) => ({ op: "delete_text", id })) });
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
