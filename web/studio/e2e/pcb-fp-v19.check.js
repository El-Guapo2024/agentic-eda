// Lane pcb-fp (docs/parity/UI-LEFT-3.md P3-31, P3-33 and the free-pad / Ctrl+E items earlier lanes left; PARITY-pcb section 123): driven the way a person does it --
// Preferences, clicks on the canvas, the edit actions -- with `/api/cmd` only for what the page cannot make (a grid item), read from `/api/state` and the footprint
// library's API.
//
// Run it on a COPY of the nc-pins-placed scratch board (tools/run-all-checks.sh makes it: examples/nc_pins.yaml placed). It undoes everything it does and ends by
// checking the board is as it found it.
//
// What it checks:
//   delete      with "Allow free pads" on, Delete of a selected pad takes it off its footprint (`EDIT_TOOL::DeleteItems`' `case PCB_PAD_T`): one `remove_board_pad`, the
//               footprint keeps its other pads where they were, one undo puts it back; off, the first Delete only selects the footprint (`EDIT_TOOL::Remove`);
//   flip        with it on, F on a lone pad turns that pad over alone (`PAD::Flip`): one `flip_items` of the pad, about its own position; the pad's copper is on B.Cu,
//               its footprint stays on the front; one undo;
//   swap        Alt+S of a grid item (KiCad 10's `PCB_GRID_ITEM`) and a text trades their places (`EDIT_TOOL::Swap`, P3-33): one `swap_items`, one undo;
//   ctrl+e      Edit Footprint of J1 holds its courtyard lines as graphics (`LoadFootprintFromBoard` clones them) and Update Footprint on Board puts the footprint back
//               as it was.
// It answers through `window.__pcbFpV19Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=nc-pins-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__pcbFpV19Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 15000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // what the studio sends
  const replies = [];
  const realFetch = window.fetch;
  window.fetch = async (input, init) => {
    const res = await realFetch.call(window, input, init);
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (/\/api\/cmd/.test(url)) replies.push({ req: init && typeof init.body === "string" ? JSON.parse(init.body) : null, body: await res.clone().json().catch(() => null) });
    return res;
  };
  const waitCmd = async (from, ms = 15000) => { const t0 = Date.now(); for (;;) { const r = replies[from]; if (r) return r; if (Date.now() - t0 > ms) return null; await sleep(40); } };
  const quiet = async () => { for (let seen = -1; seen !== replies.length;) { seen = replies.length; await sleep(300); } };

  // the page
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, shift = false) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerdown" ? 1 : 0, shiftKey: shift }));
  };
  const clickAt = async (wx, wy, shift = false) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y, shift);
    await sleep(200);
    fire("pointerdown", x, y, shift);
    await sleep(40);
    fire("pointerup", x, y, shift);
    await sleep(350);
  };
  const dragBox = async (a, b) => {
    const [x0, y0] = w2c(a[0], a[1]), [x1, y1] = w2c(b[0], b[1]);
    fire("pointermove", x0, y0);
    await sleep(150);
    fire("pointerdown", x0, y0);
    for (let k = 1; k <= 6; k++) { fire("pointermove", x0 + ((x1 - x0) * k) / 6, y0 + ((y1 - y0) * k) / 6); await sleep(60); }
    fire("pointerup", x1, y1);
    await sleep(400);
  };
  const state = () => json("/api/state");
  const dialog = () => document.querySelector(".dialog");
  const partOf = (st, ref) => (st.parts ?? []).find((p) => p.ref === ref);
  const padOf = (st, id) => { for (const p of st.parts ?? []) for (const q of p.pads ?? []) if (q.id === id) return q; return null; };
  const snapshot = (st) =>
    JSON.stringify([
      (st.parts ?? []).map((p) => [p.ref, p.at, p.rot, p.side, (p.pads ?? []).map((q) => [q.id, q.px ?? q.x, q.py ?? q.y, q.rot, q.layers])]),
      (st.drawings?.texts ?? []).map((t) => [t.id, t.x, t.y, t.layer]),
      (st.drawings?.grid_items ?? []).map((g) => [g.id, g.x, g.y]),
    ]);
  let original = null;
  const restore = async () => { for (let i = 0; i < 12 && snapshot(await state()) !== original; i++) { await E.run("common.Interactive.undo"); await sleep(600); } };
  const select = async (ids) => {
    await E.run("common.InteractiveSelection.clear").catch(() => null);
    await sleep(200);
    const st = await state();
    for (const [k, id] of ids.entries()) {
      const q = padOf(st, id);
      if (q) await clickAt(q.x, q.y, k > 0);
    }
    return E.state().selection.map((s) => s.id ?? s);
  };
  const setFreePadsPref = async (on) => {
    await E.run("common.SuiteControl.openPreferences");
    await until(() => tid("preferences-dialog"));
    tid("prefs-page-pcb_edit")?.click();
    await sleep(300);
    const box = tid("pref-allow-free-pads");
    if (box && box.checked !== on) box.click();
    await sleep(150);
    tid("preferences-ok")?.click();
    await sleep(500);
    return !!box;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { while (dialog()) { const b = [...dialog().querySelectorAll("button")].find((x) => /Cancel|Close/.test(x.textContent)); if (!b) break; b.click(); await sleep(300); } await restore(); } catch { /* best effort */ }
    if (snapshot(await state()) !== original) { entry.notes.push("FAIL: the board is not as it was after the scenario"); entry.ok = false; }
    results.push(entry);
  };

  for (let i = 0; i < 40 && !(E.state().tab === "pcb" && E.state().counts.footprints > 0); i++) await sleep(250);
  await E.run("common.Control.zoomFitScreen");
  await sleep(800);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const errorsFrom = E.errors().length;
  original = snapshot(await state());
  const st0 = await state();

  await scenario("Delete of a free pad takes it off its footprint; without Allow free pads the first Delete selects the footprint", async (t) => {
    await setFreePadsPref(false);
    let sel = await select(["J1.2"]);
    t(sel.includes("J1.2"), `a click on J1.2 selects the pad: ${JSON.stringify(sel)}`);
    let n = replies.length;
    await E.run("common.Interactive.delete");
    await sleep(600);
    t(replies.length === n, "off: nothing is sent");
    t(E.state().selection.map((s) => s.id ?? s).includes("J1"), `off: the footprint is selected instead (EDIT_TOOL::Remove): ${JSON.stringify(E.state().selection)}`);
    await setFreePadsPref(true);
    sel = await select(["J1.2"]);
    n = replies.length;
    await E.run("common.Interactive.delete");
    const r = await waitCmd(n);
    await quiet();
    t(!!r && JSON.stringify(r.req).includes('"remove_board_pad"') && JSON.stringify(r.req).includes('"J1.2"') && r.body?.ok !== false, `on: one remove_board_pad of J1.2: ${r && JSON.stringify([r.req, r.body?.ok, r.body?.message])}`);
    const st = await state();
    const nums = (partOf(st, "J1").pads ?? []).map((q) => q.num);
    t(!nums.includes("2") && nums.length === (partOf(st0, "J1").pads ?? []).length - 1, `J1 has no pad 2 now: ${JSON.stringify(nums)}`);
    const others = (partOf(st0, "J1").pads ?? []).filter((q) => q.num !== "2");
    t(others.every((q) => { const now = padOf(st, q.id); return now && Math.abs(now.x - q.x) < 1000 && Math.abs(now.y - q.y) < 1000; }), "the other pads stay where they were");
    t(JSON.stringify(partOf(st, "J1").at) === JSON.stringify(partOf(st0, "J1").at), "J1 stays");
    await E.run("common.Interactive.undo");
    await sleep(800);
    t(snapshot(await state()) === original, "one undo puts the pad back");
  });

  await scenario("F on a free pad turns that pad over alone", async (t) => {
    await setFreePadsPref(true);
    // an SMD pad (its copper on F.Cu alone), of a footprint on the front
    const smd = (st0.parts ?? []).filter((p) => p.side !== "bottom").flatMap((p) => (p.pads ?? []).map((q) => ({ part: p.ref, q }))).find(({ q }) => (q.layers ?? []).includes("F.Cu") && !(q.layers ?? []).includes("B.Cu") && !(q.layers ?? []).includes("*.Cu"));
    t(!!smd, `an SMD pad on the front: ${smd && smd.q.id}`);
    if (!smd) return;
    const id = smd.q.id, ref = smd.part;
    const sel = await select([id]);
    t(sel.includes(id), `${id} selected: ${JSON.stringify(sel)}`);
    const q0 = padOf(st0, id);
    const n = replies.length;
    await E.run("pcbnew.InteractiveEdit.flip");
    const r = await waitCmd(n);
    await quiet();
    const req = r && JSON.stringify(r.req);
    t(!!r && req.includes('"flip_items"') && req.includes(`"ids":["${id}"]`) && r.body?.ok !== false, `one flip_items of the pad: ${req} ${r && r.body?.message}`);
    const st = await state();
    const q = padOf(st, id);
    t(!!q && Math.abs((q.px ?? q.x) - (q0.px ?? q0.x)) < 1000 && Math.abs((q.py ?? q.y) - (q0.py ?? q0.y)) < 1000, `about its own position it stays: ${JSON.stringify([q0.px ?? q0.x, q0.py ?? q0.y])} -> ${JSON.stringify(q && [q.px ?? q.x, q.py ?? q.y])}`);
    t(!!q && (q.layers ?? []).includes("B.Cu") && !(q.layers ?? []).includes("F.Cu"), `its copper is on the back: ${JSON.stringify(q && q.layers)}`);
    t(partOf(st, ref).side === partOf(st0, ref).side, `${ref} stays on its side`);
    await E.run("common.Interactive.undo");
    await sleep(800);
    t(snapshot(await state()) === original, "one undo puts it back");
    await setFreePadsPref(false);
  });

  await scenario("Alt+S of a grid item and a text trades their places (P3-33)", async (t) => {
    // (off the board, where nothing else is: a window drag takes the two)
    const g = await cmd({ op: "pcb_item", verb: "add_grid_item", grid_item: { kind: "cartesian", at: { x: -30_000_000, y: -30_000_000 }, extent: { x: 2_000_000, y: 2_000_000 }, spacing: { x: 500_000, y: 500_000 }, angle: 0, phi_extent_deg: 360, phi_spacing_deg: 10, priority: 1, tick_interval: 0, affects: { cursor: true, routing: true, placement: true } } });
    const tx = await cmd({ op: "add_text", text: { content: "SWAP", at: { x: -15_000_000, y: -30_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false } });
    t(g.ok !== false && tx.ok !== false, `a grid item and a text: ${JSON.stringify([g.message, tx.message])}`);
    await E.refresh?.();
    await sleep(800);
    const st = await state();
    const grid = (st.drawings?.grid_items ?? []).find((x) => x.x === -30_000_000 && x.y === -30_000_000);
    const text = (st.drawings?.texts ?? []).find((x) => x.content === "SWAP");
    t(!!grid && !!text, "both on the board");
    if (!grid || !text) return;
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
    await E.run("common.InteractiveSelection.clear").catch(() => null);
    await dragBox([-40_000_000, -40_000_000], [-8_000_000, -22_000_000]);
    t(E.state().selection.length === 2, `both selected: ${JSON.stringify(E.state().selection)}`);
    const n = replies.length;
    await E.run("pcbnew.InteractiveEdit.swap");
    const r = await waitCmd(n);
    await quiet();
    t(!!r && JSON.stringify(r.req).includes('"swap_items"') && r.body?.ok !== false, `one swap_items: ${r && JSON.stringify([r.req, r.body?.ok, r.body?.message])}`);
    const after = await state();
    const g1 = (after.drawings?.grid_items ?? []).find((x) => x.id === grid.id);
    const t1 = (after.drawings?.texts ?? []).find((x) => x.id === text.id);
    t(!!g1 && !!t1 && g1.x === text.x && g1.y === text.y && t1.x === grid.x && t1.y === grid.y, `they traded places: ${JSON.stringify([[grid.x, grid.y], [text.x, text.y]])} -> ${JSON.stringify([g1 && [g1.x, g1.y], t1 && [t1.x, t1.y]])}`);
    await E.run("common.Interactive.undo");
    await sleep(800);
    const back = await state();
    const g2 = (back.drawings?.grid_items ?? []).find((x) => x.id === grid.id);
    t(!!g2 && g2.x === grid.x && g2.y === grid.y, "one undo puts them back");
    // (the grid item and the text were this scenario's own: their two adds undone too)
    await E.run("common.Interactive.undo");
    await sleep(600);
    await E.run("common.Interactive.undo");
    await sleep(600);
  });

  await scenario("Edit Footprint (Ctrl+E) of J1 holds its courtyard lines; Update Footprint on Board puts it back as it was", async (t) => {
    const opened = await cmd({ op: "fp_editor", verb: "open_from_board", reference: "J1" });
    t(opened.ok !== false, `Ctrl+E opens J1's copy: ${JSON.stringify(opened.message ?? opened.ok)}`);
    const key = "<board>/J1";
    const copy = await json(`/api/footprint?name=${encodeURIComponent(key)}`);
    const crtyd = (copy.graphics ?? []).filter((g) => g.layer === "F.CrtYd" || g.layer === "B.CrtYd");
    const board = partOf(st0, "J1");
    t(crtyd.length > 0 || !(board.courtyard_graphics ?? []).length, `the copy holds the courtyard lines the board has: ${crtyd.length} (board ${JSON.stringify(board.courtyard_graphics ?? [])})`);
    const saved = await cmd({ op: "fp_editor", verb: "save_to_board", footprint: key });
    t(saved.ok !== false, `Update Footprint on Board: ${JSON.stringify(saved.message ?? saved.ok)}`);
    await E.refresh?.();
    await sleep(800);
    const st = await state();
    t(JSON.stringify((partOf(st, "J1").pads ?? []).map((q) => [q.id, q.x, q.y])) === JSON.stringify((board.pads ?? []).map((q) => [q.id, q.x, q.y])), "J1's pads as they were");
    await E.run("common.Interactive.undo");
    await sleep(800);
  });

  if (strictWas && strict && !strict.checked) strict.click();
  const errs = E.errors(errorsFrom);
  results.push({ name: "no errors in the page", ok: errs.length === 0, notes: errs.length ? errs.map((e) => `FAIL: ${JSON.stringify(e)}`) : ["ok: none"] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
