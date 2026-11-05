// Round-7 PCB editing check (findings/round7/r7-pcb-edit.md): what task-u1-pcb's fixes look like in the page.
//   * R7-2: Swap (Alt+S) leaves zones where they are (`ZONE::SetPosition` is a no-op), swaps the layers of two, and moves a zone's partner onto its first corner.
//   * R7-3: a text box drawn on copper at the defaults is bold (`InferBold` of `m_textAttrs`).
//   * P4: the Text Properties dialog has Bold (`DIALOG_TEXT_PROPERTIES`: it starts as the text is, sets the pen, follows a typed pen, and OK writes it).
//   * R7-5: Change Track Width / Via Size asks for the chosen row of the Pre-defined Sizes lists now (`GetCurrentTrackWidth` / `GetCurrentViaSize`), not a snapshot.
// Run it in the page of a served COPY of a routed board (it edits and undoes); it answers through `window.__pcbEditR7Check`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR7Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR7Sched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 5e6;
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
    window.__pcbEditR7Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    const r = st.routing ?? {};
    return JSON.stringify({
      tracks: (r.tracks ?? []).map((t) => [t.id, t.net, t.width, t.pts]),
      vias: (r.vias ?? []).map((v) => [v.id, v.net, v.x, v.y, v.d, v.drill]),
      zones: (r.zones ?? []).map((z) => [z.id, z.layer, z.layers, z.outline]),
      presets: [r.track_width_presets ?? [], r.via_presets ?? []],
      texts: (st.drawings?.texts ?? []).map((s) => [s.id, s.stroke_width, s.bold]),
      boxes: (st.drawings?.text_boxes ?? []).map((s) => s.id),
      parts: st.parts.map((p) => [p.ref, p.at]),
    });
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };
  /** A command sent behind the studio's back is in its state after its next refresh: wait for the design revision to move. */
  const postSeen = async (cmd) => {
    const rev0 = __eda.state().revision;
    const reply = await post(cmd);
    await waitFor(() => __eda.state().revision !== rev0, 8000);
    await sleep(400);
    return reply;
  };

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects");
    await sleep(300);
    const v = __eda.state().view;
    return v && v.scale > 0 ? (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y] : null;
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(500); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { for (let i = 0; i < 2 && __eda.state().tool !== "select"; i++) await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const routing0 = st0.routing ?? { tracks: [], vias: [], zones: [] };
  const free = (id) => !(st0.locked ?? []).includes(id);
  if (routing0.tracks.filter((x) => free(x.id)).length === 0 || routing0.vias.filter((x) => free(x.id)).length === 0) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs a track and a via"] }); check.done = true; return "no board"; }
  const base = await snapshot();
  const toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: no view transform"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 12 && (await snapshot()) !== base; i++) await undo(); };
  const outline = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const [bx0, by0] = outline.length ? [Math.min(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))] : [0, 0];
  // a free spot below the board's outline, on whole millimetres
  const [fx, fy] = [Math.round(bx0) + 5, Math.round(by0) + 10];
  const net = routing0.tracks[0].net;

  (async () => {
    await scenario("R7-2: Swap of two zones leaves both where they are and swaps their layers", async (t) => {
      const sq = (x0, y0) => [mm(x0, y0), mm(x0 + 4, y0), mm(x0 + 4, y0 + 4), mm(x0, y0 + 4)];
      await postSeen({ op: "add_zone", net, layer: "F.Cu", outline: sq(fx, fy) });
      await postSeen({ op: "add_zone", net, layer: "B.Cu", outline: sq(fx + 8, fy) });
      const at = (z, p) => z.outline[0][0] === p.x && z.outline[0][1] === p.y;
      const pair = async () => { const zs = (await state()).routing?.zones ?? []; return [zs.find((z) => at(z, mm(fx, fy))), zs.find((z) => at(z, mm(fx + 8, fy)))]; };
      const [a, b] = await pair();
      t(!!a && !!b && a.layer === "F.Cu" && b.layer === "B.Cu", "two zones, on F.Cu and on B.Cu");
      if (!a || !b) return;
      await select([a.id, b.id]);
      const reply = await __eda.run("pcbnew.InteractiveEdit.swap");
      await sleep(800);
      t(!!reply && reply.ok !== false, `Swap ran (${JSON.stringify(reply)})`);
      const [na, nb] = await pair();
      t(!!na && !!nb, "both outlines are where they were (each zone's first corner unmoved)");
      t(!!na && !!nb && na.layer === "B.Cu" && nb.layer === "F.Cu", `their layers swapped (${na && na.layer}, ${nb && nb.layer})`);
      t(!!na && JSON.stringify(na.outline) === JSON.stringify(a.outline) && !!nb && JSON.stringify(nb.outline) === JSON.stringify(b.outline), "the outlines are unchanged");
      await undo();
      const [ua, ub] = await pair();
      t(!!ua && !!ub && ua.layer === "F.Cu" && ub.layer === "B.Cu", "one undo step took the swap back");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R7-2: Swap of a via and a zone moves only the via, onto the zone's first corner", async (t) => {
      const corner = mm(fx, fy);
      await postSeen({ op: "add_zone", net, layer: "F.Cu", outline: [corner, mm(fx + 4, fy), mm(fx + 4, fy + 4), mm(fx, fy + 4)] });
      const z = ((await state()).routing?.zones ?? []).find((q) => q.outline[0][0] === corner.x && q.outline[0][1] === corner.y);
      const v = routing0.vias.find((x) => free(x.id));
      t(!!z && !!v, "a zone and a via");
      if (!z || !v) return;
      await select([v.id, z.id]);
      await __eda.run("pcbnew.InteractiveEdit.swap");
      await sleep(800);
      const st = await state();
      const nv = st.routing.vias.find((x) => x.id === v.id);
      const nz = (st.routing.zones ?? []).find((q) => q.id === z.id);
      t(!!nv && nv.x === corner.x && nv.y === corner.y, `the via is on the zone's first corner (${nv && [nv.x, nv.y]})`);
      t(!!nz && JSON.stringify(nz.outline) === JSON.stringify(z.outline) && nz.layer === z.layer, "the zone stayed, on its layer");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R7-3: a text box drawn on copper at the default sizes is bold", async (t) => {
      await __eda.run("pcbnew.Control.layerTop");
      await sleep(200);
      await __eda.run("pcbnew.InteractiveDrawing.textbox");
      await sleep(300);
      await click(toScreen, fx, fy);
      await hover(toScreen, fx + 10, fy + 4);
      await click(toScreen, fx + 10, fy + 4);
      await sleep(400);
      const dlg = tid("pcb-text-box-properties");
      t(!!dlg, "Text Box Properties opened");
      if (!dlg) return;
      t(tid("ptb-layer").value === "F.Cu", `on F.Cu (${tid("ptb-layer").value})`);
      t(tid("ptb-bold").checked === true, "Bold is checked (1.5 mm text, 0.3 mm pen: InferBold)");
      const before = ((await state()).drawings?.text_boxes ?? []).map((b) => b.id);
      setValue(tid("ptb-text"), "X");
      await sleep(150);
      tid("ptb-ok").click();
      await sleep(900);
      const made = ((await state()).drawings?.text_boxes ?? []).find((b) => !before.includes(b.id));
      t(!!made && made.bold === true && made.layer === "F.Cu", `the box is bold (${made && JSON.stringify({ bold: made.bold, layer: made.layer, thickness: made.thickness })})`);
      await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("P4: the Text Properties dialog has Bold: as the text is, the pen follows it and it follows the pen, Save writes it", async (t) => {
      const before = ((await state()).drawings?.texts ?? []).map((x) => x.id);
      await postSeen({ op: "add_text", text: { content: "R7 BOLD", at: mm(fx, fy), angle: 0, layer: "F.Cu", size_nm: 1_500_000, stroke_width: 300_000, justify: "left", mirror: false } });
      const text = ((await state()).drawings?.texts ?? []).find((x) => !before.includes(x.id));
      t(!!text && text.bold === false, `a copper text with no bold of its own (${text && text.bold})`);
      if (!text) return;
      const open = async () => {
        await select([text.id]);
        await __eda.run("pcbnew.InteractiveEdit.properties");
        await waitFor(() => !!tid("text-dialog"), 4000);
        return tid("text-dialog");
      };
      let dlg = await open();
      t(!!dlg && dlg.textContent.includes("Text Properties"), "E opens Text Properties");
      if (!dlg) return;
      t(tid("text-bold").checked === false, "Bold starts as the text is (m_bold->Check( m_item->IsBold() ))");
      tid("text-bold").click();
      await sleep(200);
      t(tid("text-bold").checked && Number(tid("text-thickness").value) === 300_000, `checking it sets the bold pen (${tid("text-thickness").value})`);
      tid("text-bold").click();
      await sleep(200);
      t(!tid("text-bold").checked && Number(tid("text-thickness").value) === 187_500, `unchecking it sets the normal pen (${tid("text-thickness").value})`);
      setValue(tid("text-thickness"), 290_000);
      await sleep(200);
      t(tid("text-bold").checked === true, "a pen near the bold one checks it (onThickness)");
      setValue(tid("text-thickness"), 300_000);
      await sleep(200);
      buttonText(dlg, "Save").click();
      await sleep(900);
      const saved = ((await state()).drawings?.texts ?? []).find((x) => x.id === text.id);
      t(!!saved && saved.bold === true && saved.stroke_width === 300_000, `Save wrote it bold (${saved && JSON.stringify({ bold: saved.bold, pen: saved.stroke_width })})`);
      dlg = await open();
      t(!!dlg && tid("text-bold").checked === true, "reopened, Bold is checked");
      await closeDialogs();
      await undo();
      const back = ((await state()).drawings?.texts ?? []).find((x) => x.id === text.id);
      t(!!back && back.bold === false, "one undo step took the bold back");
      // A new text on copper starts bold, as `PlaceText`'s `InferBold` makes it.
      await select([]);
      await __eda.run("pcbnew.Control.layerTop");
      await __eda.run("pcbnew.InteractiveDrawing.text");
      await sleep(300);
      await click(toScreen, fx + 2, fy + 6);
      await waitFor(() => !!tid("text-dialog"), 4000);
      t(!!tid("text-dialog") && tid("text-bold").checked === true, "the Text tool's dialog starts with Bold checked on F.Cu");
      await closeDialogs();
      await escape();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R7-5: Change Track Width / Via Size takes the chosen row of the lists as Board Setup left it", async (t) => {
      await postSeen({ op: "set_track_width_presets", widths: [300_000, 500_000] });
      await postSeen({ op: "set_via_presets", presets: [{ diameter: 800_000, drill: 400_000 }] });
      // Pick the second width and the first via size (`m_trackWidthIndex` 2, `m_viaSizeIndex` 1).
      await select([]);
      await __eda.run("pcbnew.EditorControl.trackWidthInc");
      await __eda.run("pcbnew.EditorControl.trackWidthInc");
      await __eda.run("pcbnew.EditorControl.viaSizeInc");
      await sleep(300);
      // Board Setup > Pre-defined Sizes changes those rows.
      await postSeen({ op: "set_track_width_presets", widths: [300_000, 400_000] });
      await postSeen({ op: "set_via_presets", presets: [{ diameter: 900_000, drill: 450_000 }] });
      const tr = routing0.tracks.find((x) => free(x.id));
      const v = routing0.vias.find((x) => free(x.id));
      await select([tr.id, v.id]);
      await __eda.run("pcbnew.InteractiveEdit.changeTrackWidth");
      await sleep(900);
      const st = await state();
      const nt = st.routing.tracks.find((x) => x.id === tr.id) ?? st.routing.tracks.find((x) => JSON.stringify(x.pts) === JSON.stringify(tr.pts));
      const nv = st.routing.vias.find((x) => x.id === v.id) ?? st.routing.vias.find((x) => x.x === v.x && x.y === v.y);
      t(!!nt && nt.width === 400_000, `the track has the row's new width, 0.4 mm (${nt && nt.width})`);
      t(!!nv && nv.d === 900_000 && nv.drill === 450_000, `the via has the row's new size, 0.9 / 0.45 mm (${nv && [nv.d, nv.drill]})`);
      await undo();
      const ut = (await state()).routing.tracks.find((x) => JSON.stringify(x.pts) === JSON.stringify(tr.pts));
      t(!!ut && ut.width === tr.width, "one undo step took both back");
      // Back to "use net class values" for whatever runs next.
      await select([]);
      await __eda.run("pcbnew.EditorControl.trackWidthInc");
      await __eda.run("pcbnew.EditorControl.viaSizeInc");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
