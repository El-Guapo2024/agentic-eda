// Round-2 PCB editing check (findings/round2/r2-pcb-edit.md): what the fixes of task-r2-pcb-edit look like in the page.
//   * R1: Delete with a lone zone selected and the cursor inside one of its cutouts removes that cutout only, one undo step, the zone still selected.
//   * A2: a footprint flipped here shows KiCad's orientation (half a turn from -rot) in the state and in Footprint Properties.
//   * C2: Copy of one pad pastes a footprint of that pad; Cut of a pad copies it and deletes nothing; Cut of a text box removes it.
//   * A5: E on two tracks opens Track & Via Properties over both; a new width is one undo step for both.
//   * C6: Escape with a line started keeps the line tool; a second Escape leaves it.
//   * A4: Group Properties has a Locked box that locks the group.
//   * R3: the dimension dialog lists KiCad's ten precision levels.
//   * R5: Position Relative opens with the offsets of last time.
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes, and leaves Strict off while it runs); it answers
// through `window.__pcbEditR2Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbEditR2Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbEditR2Sched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
    const timers = new Map();
    let next = 4e6;
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
    window.__pcbEditR2Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const snapshot = async () => {
    const st = await state();
    const poses = {};
    for (const p of st.parts) if (p.placed) poses[p.ref] = [p.at, p.rot, p.side];
    return JSON.stringify({ poses, tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.pts]), shapes: (st.drawings?.shapes ?? []).map((s) => s.id), groups: (st.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]) });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(500); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  const delta = (a, b) => { let d = (((b - a) % 360) + 360) % 360; if (d > 180) d -= 360; return d; };

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  /** Screen position (canvas pixels) of a board point in mm: the view's own transform (`screenPx = worldNm * scale + (x, y)`), else two probes of the cursor readout. */
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects");
    await sleep(300);
    const v = __eda.state().view;
    if (v && v.scale > 0) return (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y];
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(900); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { if (__eda.state().tool !== "select") await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const placed = st0.parts.filter((p) => p.placed && (p.pads ?? []).length >= 2 && p.side === "top");
  if (placed.length < 2) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs two placed footprints with two pads"] }); check.done = true; return "no board"; }
  const two = placed.find((p) => p.pads.length === 2) ?? placed[0];
  const base = await snapshot();
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const undoUntilBase = async () => { for (let i = 0; i < 10 && (await snapshot()) !== base; i++) await undo(); };
  const dialogTitled = (text) => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.textContent.includes(text));

  (async () => {
    await scenario("R1: Delete from inside a lone zone's cutout removes that cutout only, as one undo step", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const [zx, zy] = [two.at[0] / 1e6 + 8, two.at[1] / 1e6 - 12];
      const zonesBefore = new Set(((await state()).routing?.zones ?? []).map((z) => z.id));
      await post({ op: "add_zone", net: "", layer: "B.Cu", outline: [mm(zx, zy), mm(zx + 12, zy), mm(zx + 12, zy + 12), mm(zx, zy + 12)] });
      await sleep(500);
      const zone = ((await state()).routing?.zones ?? []).find((z) => !zonesBefore.has(z.id));
      t(!!zone, "a zone drawn");
      if (!zone) return;
      await post({ op: "set_zone_outline", id: zone.id, outline: zone.outline.map(([x, y]) => ({ x, y })), holes: [[mm(zx + 3, zy + 3), mm(zx + 9, zy + 3), mm(zx + 9, zy + 9), mm(zx + 3, zy + 9)]] });
      await sleep(500);
      t((((await state()).routing?.zones ?? []).find((z) => z.id === zone.id)?.holes ?? []).length === 1, "it has a cutout");
      await select([zone.id]);
      await hover(toScreen, zx + 6, zy + 6);
      await __eda.run("common.Interactive.delete"); await sleep(900);
      const z1 = ((await state()).routing?.zones ?? []).find((z) => z.id === zone.id);
      t(!!z1, "the zone is still there");
      t(!!z1 && (z1.holes ?? []).length === 0, `its cutout is gone (${z1 ? (z1.holes ?? []).length : "no zone"})`);
      t(__eda.state().selection.some((s) => s.id === zone.id), "and it is still selected");
      await undo();
      t((((await state()).routing?.zones ?? []).find((z) => z.id === zone.id)?.holes ?? []).length === 1, "one Undo brings the cutout back");
      await select([zone.id]);
      await hover(toScreen, zx + 1, zy + 1);
      await __eda.run("common.Interactive.delete"); await sleep(900);
      t(!((await state()).routing?.zones ?? []).some((z) => z.id === zone.id), "outside the cutout, Delete takes the whole zone");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("A2: a footprint flipped here shows KiCad's orientation, half a turn from -rot", async (t) => {
      const p0 = await partOf(two.ref);
      await post({ op: "flip_items", ids: [two.ref], pivot: { x: p0.at[0], y: p0.at[1] }, direction: "left_right" });
      await sleep(600);
      const p1 = await partOf(two.ref);
      const want = (((-(p1.rot ?? 0) + 180) % 360) + 540) % 360 - 180;
      t(p1.side === "bottom" && Math.abs((p1.orientation ?? NaN) - want) < 1e-6, `the state says ${p1.orientation} for rot ${p1.rot} on the back (KiCad: ${want})`);
      await select([two.ref]);
      await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(900);
      const dlg = dialogTitled("Footprint Properties");
      t(!!dlg, "Footprint Properties opens");
      const orient = dlg?.querySelector('[data-testid="fp-orientation"]');
      t(!!orient && Math.abs(Number(orient.value) - want) < 1e-3, `its Orientation is ${orient?.value}`);
      await closeDialogs();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C2: Copy of one pad pastes a footprint of that pad; Cut of a pad deletes nothing; Cut of a text box removes it", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const p0 = await partOf(two.ref);
      const padId = `${two.ref}.${p0.pads[0].num}`;
      await select([padId]);
      await hover(toScreen, p0.pads[0].x / 1e6, p0.pads[0].y / 1e6);
      await __eda.run("common.Interactive.copy"); await sleep(800);
      const refs0 = new Set((await state()).parts.filter((p) => p.placed).map((p) => p.ref));
      await hover(toScreen, p0.at[0] / 1e6 + 15, p0.at[1] / 1e6 + 15);
      await __eda.run("common.Interactive.paste"); await sleep(1500);
      await click(toScreen, p0.at[0] / 1e6 + 15, p0.at[1] / 1e6 + 15);
      await sleep(600);
      const added = (await state()).parts.filter((p) => p.placed && !refs0.has(p.ref));
      t(added.length === 1 && (added[0].pads ?? []).length === 1, `one footprint with one pad pasted (${added.map((p) => `${p.ref}:${(p.pads ?? []).length}`).join(", ") || "none"})`);
      await undoUntilBase();
      await select([padId]);
      await __eda.run("common.Interactive.cut"); await sleep(900);
      t((await state()).parts.some((p) => p.ref === two.ref && p.placed && (p.pads ?? []).length === p0.pads.length), "Cut of a pad leaves the footprint whole");
      const boxesBefore = new Set(((await state()).drawings?.text_boxes ?? []).map((b) => b.id));
      const [bx, by] = [p0.at[0] / 1e6 + 10, p0.at[1] / 1e6 - 15];
      await post({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "cut me", start: mm(bx, by), end: mm(bx + 8, by + 4), pts: [], angle: 0, margin_left_nm: 0, margin_top_nm: 0, margin_right_nm: 0, margin_bottom_nm: 0, size_nm: 1_000_000, stroke_width_nm: 100_000 } });
      await sleep(600);
      const box = ((await state()).drawings?.text_boxes ?? []).find((b) => !boxesBefore.has(b.id));
      t(!!box, "a text box drawn");
      if (box) {
        await select([box.id]);
        await __eda.run("common.Interactive.cut"); await sleep(1000);
        t(!((await state()).drawings?.text_boxes ?? []).some((b) => b.id === box.id), "Cut copied the text box and removed it");
      }
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("A5: E on two tracks opens Track & Via Properties over both; a new width is one undo step", async (t) => {
      const net = two.pads.find((q) => q.net)?.net ?? "";
      const [x0, y0] = [two.at[0] / 1e6 + 5, two.at[1] / 1e6 + 20];
      const before = new Set(((await state()).routing?.tracks ?? []).map((x) => x.id));
      await post({ op: "add_track", net, layer: "F.Cu", width: 200_000, pts: [mm(x0, y0), mm(x0 + 5, y0)] });
      await post({ op: "add_track", net, layer: "F.Cu", width: 300_000, pts: [mm(x0, y0 + 2), mm(x0 + 5, y0 + 2)] });
      await sleep(600);
      const mine = ((await state()).routing?.tracks ?? []).filter((x) => !before.has(x.id)).map((x) => x.id);
      t(mine.length === 2, "two tracks drawn");
      await select(mine);
      await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(900);
      const dlg = document.querySelector('.dialog[aria-label="Track & Via Properties"]');
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      const width = dlg.querySelector('input[aria-label="Track width"]');
      t(!!width && width.value === "", `the widths differ: the field is indeterminate (${width?.value})`);
      t(!!dlg.querySelector('select[aria-label="Net"]'), "the net is a choice, not read-only");
      setValue(width, 0.4); await sleep(150);
      buttonText(dlg, "OK").click(); await sleep(1200);
      const widths = ((await state()).routing?.tracks ?? []).filter((x) => mine.includes(x.id)).map((x) => x.width);
      t(widths.length === 2 && widths.every((w) => w === 400_000), `both tracks are 0.4 mm (${widths})`);
      await undo();
      const back = ((await state()).routing?.tracks ?? []).filter((x) => mine.includes(x.id)).map((x) => x.width).sort();
      t(JSON.stringify(back) === JSON.stringify([200_000, 300_000]), `one Undo gives both widths back (${back})`);
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("C6: Escape with a line started keeps the line tool; a second Escape leaves it", async (t) => {
      toScreen = (await screenOf()) ?? toScreen;
      const [x0, y0] = [two.at[0] / 1e6 + 5, two.at[1] / 1e6 + 25];
      await __eda.run("pcbnew.InteractiveDrawing.line"); await sleep(400);
      const tool = __eda.state().tool;
      await click(toScreen, x0, y0);
      await hover(toScreen, x0 + 3, y0);
      await __eda.run("common.Interactive.cancel"); await sleep(400);
      t(__eda.state().tool === tool && tool !== "select", `the line tool is still armed (${tool} -> ${__eda.state().tool})`);
      await __eda.run("common.Interactive.cancel"); await sleep(400);
      t(__eda.state().tool === "select", `the second Escape leaves it (${__eda.state().tool})`);
      t((await snapshot()) === base, "nothing was drawn");
    });

    await scenario("A4: Group Properties has a Locked box; ticking it locks the group", async (t) => {
      const [x0, y0] = [two.at[0] / 1e6 + 5, two.at[1] / 1e6 - 25];
      const before = new Set(((await state()).drawings?.shapes ?? []).map((s) => s.id));
      for (const dy of [0, 1]) await post({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 150_000, filled: false, start: mm(x0, y0 + dy), end: mm(x0 + 3, y0 + dy) } });
      await sleep(400);
      const mine = ((await state()).drawings?.shapes ?? []).filter((s) => !before.has(s.id)).map((s) => s.id);
      await select(mine);
      await __eda.run("common.Interactive.group"); await sleep(900);
      const g = ((await state()).drawings?.groups ?? []).find((x) => x.member_ids.includes(mine[0]));
      t(!!g, "grouped");
      if (!g) return;
      await select([g.id]);
      await __eda.run("common.Groups.groupProperties"); await sleep(700);
      const dlg = document.querySelector('.dialog[aria-label="Group Properties"]');
      const box = dlg?.querySelector('input[aria-label="Locked"]');
      t(!!box && !box.checked, "the Locked box is there, unticked");
      if (!box) return;
      box.click(); await sleep(150);
      buttonText(dlg, "OK").click(); await sleep(1000);
      t(((await state()).locked ?? []).includes(g.id), "OK locks the group");
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R3: the dimension dialog lists KiCad's ten precision levels", async (t) => {
      const [x0, y0] = [two.at[0] / 1e6 + 5, two.at[1] / 1e6 - 30];
      const before = new Set(((await state()).drawings?.dimensions ?? []).map((d) => d.id));
      await post({ op: "add_dimension", dimension: { layer: "Dwgs.User", kind: { kind: "aligned", height: 2_000_000 }, start: mm(x0, y0), end: mm(x0 + 10, y0), prefix: "", suffix: "", units: "mm", units_format: "no_suffix", precision: 6, suppress_trailing_zeros: false, text_position: "outside", keep_text_aligned: true, text_angle: 0, text_size_nm: 1_000_000, stroke_width: 150_000, arrow_length: 1_270_000, extension_offset: 500_000, extension_height: 586_000, arrow_direction: "outward" } });
      await sleep(700);
      const dim = ((await state()).drawings?.dimensions ?? []).find((d) => !before.has(d.id));
      t(!!dim, `a dimension drawn at precision 6 (${dim?.text})`);
      if (!dim) return;
      t(dim.text === "10.0", `V_VV in millimetres is one decimal (${dim.text})`);
      await select([dim.id]);
      await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(800);
      const sel = [...document.querySelectorAll(".dialog select")].find((s) => [...s.options].some((o) => o.textContent.includes("0.00 in / 0 mils / 0.0 mm")));
      t(!!sel && sel.options.length === 10 && sel.value === "6", `ten levels, V_VV chosen (${sel ? sel.options.length : 0}, ${sel?.value})`);
      await closeDialogs();
      await undoUntilBase();
      t((await snapshot()) === base, "undone, the board is as it was");
    });

    await scenario("R5: Position Relative opens with the offsets of last time", async (t) => {
      await select([two.ref]);
      await __eda.run("pcbnew.PositionRelative.positionRelative"); await sleep(600);
      let dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Position Relative To..."));
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      let inputs = [...dlg.querySelectorAll(".kv-grid input")];
      setValue(inputs[0], 2.5); setValue(inputs[1], -1.5); await sleep(150);
      buttonText(dlg, "Cancel").click(); await sleep(400);
      await __eda.run("pcbnew.PositionRelative.positionRelative"); await sleep(600);
      dlg = [...document.querySelectorAll(".dialog")].find((d) => d.textContent.includes("Position Relative To..."));
      inputs = [...dlg.querySelectorAll(".kv-grid input")];
      t(Number(inputs[0].value) === 2.5 && Number(inputs[1].value) === -1.5, `the offsets are as they were left (${inputs[0].value}, ${inputs[1].value})`);
      setValue(inputs[0], 0); setValue(inputs[1], 0); await sleep(150);
      buttonText(dlg, "Cancel").click(); await sleep(300);
      t((await snapshot()) === base, "nothing moved");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
