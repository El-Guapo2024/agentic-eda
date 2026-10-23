// Board editor basics check (GAPS.md item 33, package P20; items 8 and 10): what a person does first on a board -- select footprints with the pointer (a click,
// Shift+click, a click on nothing, a window), move one by dragging it, with the Move tool (M, and Escape) and with Move Exactly, rotate it both ways (R, Shift+R),
// flip it (F), delete it (Del), and undo and redo each of those, one step each; and the Properties panel's Position and Orientation rows of a footprint.
// Every edit is read back from the server (`GET /api/state`: the footprint's `at`, `rot`, `side`, its pads' positions and layers, `placed`), undone, and the
// board compared with how it was at the start.
// Run it in the page of a served board that has footprints placed -- `eda place examples/mcu_board_30plus.yaml -o <p>`, `eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, `cp <p>/design.json <dir>/`, `eda board serve -C <dir> --port N --ui web/studio/dist` -- with
// `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/board-edit.check.js`, or pasted into the Browser pane's
// javascript tool, on a COPY of a board (it edits and undoes, and switches Strict off while it runs).
// It needs three placed footprints with two or more pads (it takes the first three: `U1`, `R1`, `R2` on that board), and it answers through
// `window.__boardEditCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with `await __boardEditCheck.wait(30000)`
// until `.done`. A failed note names what the server held.
// It works through `window.__eda` (actions by id, `__eda.state()` for the selection and the tool), the canvas' pointer events (the status bar's cursor readout
// gives the screen-to-board mapping), the Move Exactly dialog's fields and the Properties panel's rows (`.prop-row[data-prop=...]`).
// run-all-checks: board=mcu30-placed minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__boardEditCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const pose = (p) => ({ at: p.at, rot: p.rot ?? 0, side: p.side, placed: p.placed, pads: (p.pads ?? []).map((q) => [q.x, q.y]) });
  const snapshot = async () => {
    const st = await state();
    return JSON.stringify({ parts: st.parts.map((p) => [p.ref, p.placed, p.at, p.rot, p.side]), tracks: (st.routing?.tracks ?? []).map((t) => [t.id, t.pts]), vias: (st.routing?.vias ?? []).length, shapes: (st.drawings?.shapes ?? []).length });
  };
  const run = (id, args) => __eda.run(id, args);
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const selected = () => __eda.state().selection.map((s) => s.id).sort();
  const undo = async () => { await run("common.Interactive.undo"); await sleep(600); };
  const redo = async () => { await run("common.Interactive.redo"); await sleep(600); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(500); };
  const lastAct = async () => (await state()).activity?.[0] ?? null;
  const waitAct = async (before) => {
    const t0 = Date.now();
    while (Date.now() - t0 < 5000) {
      const a = await lastAct();
      if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(350); return a; }
      await sleep(50);
    }
    return null;
  };
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  /** The angle from a to b in degrees, in (-180, 180]. */
  const turn = (a, b) => { let d = (((b - a) % 360) + 360) % 360; if (d > 180) d -= 360; return d; };

  // The canvas: the pointer events a user makes. The cursor readout of the status bar gives the screen-to-board mapping.
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  let mmPerPx = 0;
  /** Screen position (canvas pixels) of a board point in um, from two probes of the cursor readout. */
  const screenOf = async () => {
    await run("common.Control.zoomFitObjects");
    await sleep(300);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    mmPerPx = Math.abs(sx);
    return (xum, yum) => [100 + (xum / 1000 - a[0]) / sx, 100 + (yum / 1000 - a[1]) / sy];
  };
  const click = async (px, py, extra = {}) => { fire("pointermove", px, py, extra); await sleep(150); fire("pointerdown", px, py, { buttons: 1, ...extra }); await sleep(60); fire("pointerup", px, py, extra); await sleep(700); };
  /** A press, a motion in steps with the button held, a release: a drag. */
  const drag = async (x0, y0, x1, y1, extra = {}) => {
    fire("pointermove", x0, y0); await sleep(120);
    fire("pointerdown", x0, y0, { buttons: 1, ...extra }); await sleep(80);
    for (let i = 1; i <= 6; i++) { fire("pointermove", x0 + ((x1 - x0) * i) / 6, y0 + ((y1 - y0) * i) / 6, { buttons: 1, ...extra }); await sleep(80); }
    fire("pointerup", x1, y1, extra); await sleep(900);
  };

  // The Properties panel: set a row's editor the way a user does, and wait for the command it sends.
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const editRow = async (name, value) => {
    const el = input(name);
    if (!el) return `no editable row ${name}`;
    const before = await lastAct();
    el.focus(); setValue(el, value); await sleep(30);
    el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const a = await waitAct(before);
    if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
    return a.ok ? `ok: ${a.cmd.slice(0, 80)}` : `REFUSED: ${a.message.slice(0, 100)}`;
  };

  let base = null;
  /** Undo until the board is as it was (a scenario that left something behind). */
  const restore = async () => { for (let i = 0; i < 10; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); if (__eda.state().tool !== "select") await escape(); } catch { /* ignore */ }
    const left = await restore();
    if (left < 0) { entry.notes.push("FAIL: the board could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`FAIL: ${left} edit(s) the scenario left behind were undone`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  document.querySelector('button[aria-label="Show Properties"]')?.click(); // the dock folds in a narrow window
  await sleep(300);
  const placed = (await state()).parts.filter((p) => p.placed && (p.pads ?? []).length >= 2);
  if (placed.length < 3) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs three placed footprints with two or more pads"] }); check.done = true; return "no board"; }
  const [A, B, C] = placed.map((p) => p.ref);
  base = await snapshot();
  const to = await screenOf();
  if (!to) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }

  (async () => {
    // ----- selection with the pointer
    await scenario("select: a click on a footprint selects it, Shift+click adds another, a click on nothing clears, a window takes what it encloses", async (t) => {
      await select([]);
      const a = await partOf(A), b = await partOf(B);
      const [ax, ay] = to(a.at[0], a.at[1]);
      await click(ax, ay);
      t(JSON.stringify(selected()) === JSON.stringify([A]), `a click on ${A} selects it: ${JSON.stringify(__eda.state().selection)}`);
      t(__eda.state().selection[0]?.kind === "footprint", `as a footprint (${__eda.state().selection[0]?.kind})`);
      const [bx, by] = to(b.at[0], b.at[1]);
      await click(bx, by, { shiftKey: true });
      t(JSON.stringify(selected()) === JSON.stringify([A, B].sort()), `Shift+click adds ${B}: ${selected()}`);
      const st = await state();
      const [ox0, oy0] = to(st.outline[0][0], st.outline[0][1]);
      // Outside the board outline, left of it: nothing is there.
      await click(Math.max(5, ox0 - 40), Math.max(5, oy0 - 40));
      t(selected().length === 0, `a click on nothing clears the selection: ${selected()}`);
      // A window from left to right takes only what it encloses. It starts off the board (a press on an item would drag the item), at the outline's
      // upper left corner, and ends past the footprint nearest that corner.
      const parts = (await state()).parts.filter((p) => p.placed && (p.pads ?? []).length > 0);
      const extent = (p) => { const xs = p.pads.map((q) => q.x), ys = p.pads.map((q) => q.y); return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]; };
      const corner = parts.slice().sort((p, q) => (extent(p)[2] + extent(p)[3]) - (extent(q)[2] + extent(q)[3]))[0];
      const [, , ex, ey] = extent(corner);
      const w = [st.outline[0][0] - 2000, st.outline[0][1] - 2000, ex + 1200, ey + 1200];
      const [wx0, wy0] = to(w[0], w[1]), [wx1, wy1] = to(w[2], w[3]);
      await drag(wx0, wy0, wx1, wy1);
      t(selected().includes(corner.ref), `a window round ${corner.ref} selects it: ${selected()}`);
      const outside = selected().filter((ref) => { const p = parts.find((q) => q.ref === ref); if (!p) return false; const e = extent(p); return e[0] < w[0] || e[1] < w[1] || e[2] > w[2] || e[3] > w[3]; });
      t(outside.length === 0, `and nothing whose pads it does not enclose (${outside})`);
      await select([]);
      t((await snapshot()) === base, "selecting changed nothing on the board");
    });

    // ----- move by dragging
    await scenario("move: dragging a footprint moves it by the pointer's distance, one undo step; redo moves it again", async (t) => {
      await select([]);
      const p0 = await partOf(B);
      const [px, py] = to(p0.at[0], p0.at[1]);
      await drag(px, py, px + 60, py + 30);
      const p1 = await partOf(B);
      const want = [60 * mmPerPx * 1000, 30 * mmPerPx * 1000];
      const dx = p1.at[0] - p0.at[0], dy = p1.at[1] - p0.at[1];
      t(Math.abs(dx - want[0]) < 1000 && Math.abs(dy - want[1]) < 1000, `moved by (${dx}, ${dy}) um for a drag of (${Math.round(want[0])}, ${Math.round(want[1])}) um`);
      t(p1.pads.every((q, i) => q.x - p0.pads[i].x === dx && q.y - p0.pads[i].y === dy), "its pads moved with it");
      t(p1.rot === p0.rot && p1.side === p0.side, "it kept its orientation and side");
      await undo();
      t((await snapshot()) === base, "one undo puts it back");
      await redo();
      t(JSON.stringify((await partOf(B)).at) === JSON.stringify(p1.at), `redo moves it again (${(await partOf(B)).at})`);
      await undo();
    });

    // ----- the Move tool
    await scenario("move: M carries the selection with the pointer and a click drops it; Escape leaves it where it was", async (t) => {
      const p0 = await partOf(C);
      await select([C]);
      const [px, py] = to(p0.at[0], p0.at[1]);
      fire("pointermove", px, py); await sleep(150);
      const r = await run("pcbnew.InteractiveMove.move");
      t(r.ok !== false, `M runs (${JSON.stringify(r)})`);
      fire("pointermove", px + 40, py - 20); await sleep(300);
      fire("pointerdown", px + 40, py - 20, { buttons: 1 }); await sleep(80);
      fire("pointerup", px + 40, py - 20); await sleep(900);
      const p1 = await partOf(C);
      t(p1.at[0] > p0.at[0] && p1.at[1] < p0.at[1], `dropped right of and above where it was: ${p0.at} -> ${p1.at}`);
      t(__eda.state().tool === "select", `the tool is Select again (${__eda.state().tool})`);
      await undo();
      t((await snapshot()) === base, "one undo puts it back");
      // Escape while it is carried
      await select([C]);
      fire("pointermove", px, py); await sleep(150);
      await run("pcbnew.InteractiveMove.move");
      fire("pointermove", px + 50, py + 50); await sleep(300);
      await escape();
      t(JSON.stringify((await partOf(C)).at) === JSON.stringify(p0.at), `Escape leaves it where it was (${(await partOf(C)).at})`);
      t((await snapshot()) === base, "and the board is as it was");
    });

    await scenario("move: Move Exactly moves the selection by what is typed, one undo step", async (t) => {
      const p0 = await partOf(C);
      await select([C]);
      const r = await run("pcbnew.InteractiveEdit.moveExact");
      await sleep(300);
      t(r.dialog != null, `the action opens a dialog (${r.dialog})`);
      const nums = [...document.querySelectorAll(".dialog input[type=number]")];
      t(nums.length >= 2, `it has the two offsets (${nums.length} number fields)`);
      setValue(nums[0], 2); setValue(nums[1], -1.5);
      await sleep(100);
      buttonText(document, "OK")?.click();
      await sleep(900);
      const p1 = await partOf(C);
      t(p1.at[0] === p0.at[0] + 2000 && p1.at[1] === p0.at[1] - 1500, `moved by (2, -1.5) mm: ${p0.at} -> ${p1.at}`);
      await undo();
      t((await snapshot()) === base, "one undo puts it back");
    });

    // ----- rotate
    await scenario("rotate: R turns the selection 90 degrees counter-clockwise about its centre, Shift+R clockwise, each one undo step", async (t) => {
      const p0 = await partOf(A);
      await select([A]);
      await run("pcbnew.InteractiveEdit.rotateCcw");
      await sleep(700);
      const p1 = await partOf(A);
      t(Math.abs(Math.abs(turn(p0.rot, p1.rot)) - 90) < 1e-6, `R turns it by 90 degrees: ${p0.rot} -> ${p1.rot}`);
      // KiCad turns the footprint about its position when it alone is selected: the anchor stays.
      t(JSON.stringify(p1.at) === JSON.stringify(p0.at), `about its own position (${p0.at} -> ${p1.at})`);
      // The pads go round with it: each pad's offset from the anchor is turned by 90 degrees.
      const off = (p, q) => [q.x - p.at[0], q.y - p.at[1]];
      const ok90 = p0.pads.every((q, i) => { const [x0, y0] = off(p0, q), [x1, y1] = off(p1, p1.pads[i]); return Math.abs(x0 * x1 + y0 * y1) < 2 && Math.abs(Math.hypot(x0, y0) - Math.hypot(x1, y1)) < 2; });
      t(ok90, "its pads went round with it");
      await run("pcbnew.InteractiveEdit.rotateCw");
      await sleep(700);
      const p2 = await partOf(A);
      t(Math.abs(turn(p0.rot, p2.rot)) < 1e-6 && JSON.stringify(pose(p2).pads) === JSON.stringify(pose(p0).pads), `Shift+R turns it back: ${p1.rot} -> ${p2.rot}`);
      await undo();
      t(Math.abs(turn(p1.rot, (await partOf(A)).rot)) < 1e-6, "the first undo takes back Shift+R only");
      await undo();
      t((await snapshot()) === base, "the second takes back R");
      await redo();
      t(Math.abs(turn(p1.rot, (await partOf(A)).rot)) < 1e-6, "redo turns it again");
      await undo();
    });

    // ----- flip
    await scenario("flip: F puts the footprint on the other side (its copper on B.Cu, its pads mirrored), F again brings it back, one undo step each", async (t) => {
      const p0 = await partOf(B);
      await select([B]);
      await run("pcbnew.InteractiveEdit.flip");
      await sleep(800);
      const p1 = await partOf(B);
      t(p1.side !== p0.side && p1.side === "bottom", `it is on the back: ${p0.side} -> ${p1.side}`);
      const mirrored = p0.pads.every((q, i) => p1.pads[i] && Math.abs((q.x - p0.at[0]) + (p1.pads[i].x - p1.at[0])) < 2 && Math.abs((q.y - p0.at[1]) - (p1.pads[i].y - p1.at[1])) < 2);
      t(mirrored, "its pads are mirrored left to right about its position (KiCad's default flip direction)");
      t(row("Layer") ? input("Layer")?.value === "B.Cu" || row("Layer").textContent.includes("B.Cu") : true, `the Properties panel says B.Cu (${row("Layer")?.textContent})`);
      await run("pcbnew.InteractiveEdit.flip");
      await sleep(800);
      t((await partOf(B)).side === p0.side, "F again brings it back to the front");
      await undo(); await undo();
      t((await snapshot()) === base, "two undos put the board back");
    });

    // ----- delete
    await scenario("delete: Del takes the footprint off the board, undo brings it back where it was, redo deletes it again", async (t) => {
      const p0 = await partOf(C);
      await select([C]);
      await run("common.Interactive.delete");
      await sleep(900);
      const p1 = await partOf(C);
      t(!p1 || p1.placed === false, `it is gone from the board (${p1 ? `placed=${p1.placed}` : "no part"})`);
      t(selected().length === 0, `nothing is selected after (${selected()})`);
      await undo();
      const p2 = await partOf(C);
      t(p2?.placed === true && JSON.stringify(p2.at) === JSON.stringify(p0.at) && p2.rot === p0.rot, `undo brings it back where it was (${p2?.at})`);
      await redo();
      const p3 = await partOf(C);
      t(!p3 || p3.placed === false, "redo deletes it again");
      await undo();
      t((await snapshot()) === base, "and undo puts it back");
    });

    // ----- a chain of edits, undone and redone
    await scenario("undo and redo: three edits are three steps, undone in reverse and redone in order", async (t) => {
      const states = [await snapshot()];
      await select([A]); await run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(700); states.push(await snapshot());
      await select([B]); await run("pcbnew.InteractiveEdit.flip"); await sleep(700); states.push(await snapshot());
      await select([C]); await run("common.Interactive.delete"); await sleep(700); states.push(await snapshot());
      t(new Set(states).size === 4, "each edit changed the board");
      for (let i = 3; i > 0; i--) { await undo(); t((await snapshot()) === states[i - 1], `undo ${4 - i} gives the board after edit ${i - 1}`); }
      for (let i = 1; i <= 3; i++) { await redo(); t((await snapshot()) === states[i], `redo ${i} gives the board after edit ${i}`); }
      await undo(); await undo(); await undo();
      t((await snapshot()) === base, "three undos give the board it began with");
      await redo();
      t((await snapshot()) === states[1], "redo after them gives the first edit again");
      await select([A]); await run("pcbnew.InteractiveEdit.flip"); await sleep(700);
      const s = await snapshot();
      await run("common.Interactive.redo"); await sleep(500);
      t((await snapshot()) === s, "a new edit after an undo drops what was undone: redo then changes nothing");
      await undo();
      t((await snapshot()) === states[1], "undo takes back the new edit");
      await undo();
    });

    // ----- the Properties panel
    await scenario("Properties panel: a footprint's Position X, Position Y and Orientation rows edit it, one undo step each", async (t) => {
      const p0 = await partOf(B);
      await select([B]);
      await sleep(400);
      t(!!row("Position X") && !!row("Orientation"), `the panel shows the footprint's rows (${[...document.querySelectorAll(".prop-row")].slice(0, 8).map((r) => r.dataset.prop)})`);
      const x0 = Number(input("Position X")?.value);
      t(Math.abs(x0 - p0.at[0] / 1000) < 1e-6, `Position X reads the footprint's x in mm (${x0} for ${p0.at[0]} um)`);
      let r = await editRow("Position X", String(x0 + 1.25));
      t(r.startsWith("ok"), `Position X takes a new value: ${r}`);
      t((await partOf(B)).at[0] === p0.at[0] + 1250, `the footprint moved 1.25 mm right (${(await partOf(B)).at})`);
      const y0 = Number(input("Position Y")?.value);
      r = await editRow("Position Y", String(y0 - 0.5));
      t(r.startsWith("ok") && (await partOf(B)).at[1] === p0.at[1] - 500, `Position Y moves it 0.5 mm up: ${r} (${(await partOf(B)).at})`);
      r = await editRow("Orientation", "90");
      const p1 = await partOf(B);
      t(r.startsWith("ok") && Math.abs(turn(p0.rot, p1.rot)) === 90, `Orientation 90 turns it a quarter: ${r} (rot ${p0.rot} -> ${p1.rot})`);
      await sleep(300);
      t(Math.abs(Number(input("Orientation")?.value) - 90) < 1e-6, `the row reads 90 after (${input("Orientation")?.value})`);
      await undo();
      t(Math.abs(turn(p0.rot, (await partOf(B)).rot)) < 1e-6, "undo takes back the turn only");
      await undo(); await undo();
      t((await snapshot()) === base, "two more put the board back");
    });

    // ----- the end
    await scenario("the board is as it was and the page logged no error", async (t) => {
      await select([]);
      t((await snapshot()) === base, "the board is as it was");
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
