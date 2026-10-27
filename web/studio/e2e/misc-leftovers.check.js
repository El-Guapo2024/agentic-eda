// The round-2 leftovers of task-r2-misc-leftovers in the page (findings/round2/r2-pcb-edit.md):
//   * A5: Track & Via Properties on a via has the Via type, Start layer and End layer rows (no Annular rings on two layers); Micro is one set_via_layers, one Undo.
//   * A6: a footprint's Properties panel rows are FOOTPRINT_DESC's: Library Description, Keywords, Component Class, the overrides with Zone Connection Style;
//     no Value, MPN, Block or Nets row; Zone Connection Style "Solid" is one undo step.
//   * Mirror of a text box (PCB_TEXTBOX::Mirror): the box mirrors about its position and its text angle becomes 180; one Undo.
// Run it in the page of a served COPY of a board with footprints placed (it edits and undoes, and leaves Strict off while it runs); it answers
// through `window.__miscLeftoversCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=6
(async () => {
  const results = [];
  const check = (window.__miscLeftoversCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__miscLeftoversSched) {
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
    window.__miscLeftoversSched = true;
  }

  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const snapshot = async () => {
    const st = await state();
    return JSON.stringify({ vias: (st.routing?.vias ?? []).map((v) => [v.id, v.type ?? null, v.from, v.to]), boxes: (st.drawings?.text_boxes ?? []).map((b) => [b.id, b.start, b.end, b.angle]), zone: st.parts.map((p) => [p.ref, p.zone?.connection ?? null]) });
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const fp = st0.parts.find((p) => p.placed && (p.pads ?? []).length >= 2);
  if (!fp) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs a placed footprint"] }); check.done = true; return "no board"; }
  const net = fp.pads.find((q) => q.net)?.net ?? "";
  const base = await snapshot();
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const [x0, y0] = [fp.at[0] / 1e6 + 8, fp.at[1] / 1e6 + 25];

  (async () => {
    await scenario("A5: a via's type and layer pair in Track & Via Properties; Micro is one undo step", async (t) => {
      const before = new Set(((await state()).routing?.vias ?? []).map((v) => v.id));
      await post({ op: "add_via", net, x: mm(x0, y0).x, y: mm(x0, y0).y, drill: 300_000, diameter: 600_000, from_layer: "F.Cu", to_layer: "B.Cu" });
      await sleep(600);
      const mine = ((await state()).routing?.vias ?? []).find((v) => !before.has(v.id));
      t(!!mine, "a via drawn");
      if (!mine) return;
      const drawn = await snapshot();
      await select([mine.id]);
      await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(900);
      const dlg = document.querySelector('.dialog[aria-label="Track & Via Properties"]');
      t(!!dlg, "the dialog opens");
      if (!dlg) return;
      const type = dlg.querySelector('select[aria-label="Via type"]');
      const start = dlg.querySelector('select[aria-label="Start layer"]');
      t(!!type && type.value === "through", `Via type: Through (${type?.value})`);
      t(!!start && start.disabled, "a through via's start layer is not a choice");
      t(!dlg.querySelector('select[aria-label="Annular rings"]'), "no Annular rings row on a board of two copper layers (getLayerDepth() > 1)");
      setValue(type, "micro"); await sleep(200);
      t(!dlg.querySelector('select[aria-label="Start layer"]').disabled, "a micro via's layers are choices");
      buttonText(dlg, "OK").click(); await sleep(1200);
      const after = ((await state()).routing?.vias ?? []).find((v) => v.id === mine.id);
      t(after?.type === "micro", `the via is a microvia (${after?.type})`);
      await undo();
      t((await snapshot()) === drawn, "one Undo puts it back");
      await undo();
      t((await snapshot()) === base, "and the via goes");
    });

    await scenario("A6: a footprint's Properties panel rows are FOOTPRINT_DESC's; Zone Connection Style is one undo step", async (t) => {
      await select([fp.ref]); await sleep(600);
      const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
      for (const name of ["Library Link", "Library Description", "Keywords", "Component Class", "Clearance Override", "Zone Connection Style"]) t(!!row(name), `the ${name} row`);
      for (const name of ["Value", "MPN", "Block", "Nets"]) t(!row(name), `no ${name} row`);
      const zc = row("Zone Connection Style")?.querySelector("select");
      t(!!zc, "Zone Connection Style is a choice");
      if (!zc) return;
      setValue(zc, "Full"); await sleep(1200);
      const p = (await state()).parts.find((q) => q.ref === fp.ref);
      t(p?.zone?.connection === "Full", `the footprint's zone connection is Solid (${p?.zone?.connection})`);
      await undo();
      t((await snapshot()) === base, "one Undo puts it back");
    });

    await scenario("Mirror of a text box: mirrored about its position, its text angle 180, one undo step", async (t) => {
      const before = new Set(((await state()).drawings?.text_boxes ?? []).map((b) => b.id));
      await post({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "Mirror me", start: mm(x0, y0), end: mm(x0 + 10, y0 + 4), margin_left_nm: 800_000, margin_top_nm: 800_000, margin_right_nm: 800_000, margin_bottom_nm: 800_000, stroke_width_nm: 100_000, size_nm: 1_000_000, halign: "left", valign: "top" } });
      await sleep(600);
      const box = ((await state()).drawings?.text_boxes ?? []).find((b) => !before.has(b.id));
      t(!!box, "a text box drawn");
      if (!box) return;
      const drawn = await snapshot();
      await select([box.id]);
      await __eda.run("pcbnew.InteractiveEdit.mirrorHoriontally"); await sleep(1200);
      const m = ((await state()).drawings?.text_boxes ?? []).find((b) => b.id === box.id);
      t(m?.angle === 180_000, `the text angle is 180 (${m?.angle})`);
      t(!!m && m.start[0] === box.start[0] && m.end[0] === 2 * box.start[0] - box.end[0], `mirrored about its position (${m?.start} ${m?.end})`);
      await undo();
      t((await snapshot()) === drawn, "one Undo puts it back");
      await undo();
      t((await snapshot()) === base, "and the box goes");
    });

    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
