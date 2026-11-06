// Placement check (UI-LEFT P19, PARITY-pcb.md section 84): Place Footprint's loop, the microwave tools carrying their footprint, a footprint's position on the
// grid, Pack and Move's packer and the two autoplacer actions on the studio's placer -- driven the way a person does it, through `window.__eda`, the
// chooser's controls and the canvas's pointer events, and checked against what the server holds (`/api/state`).
// Run it in the page of a served board whose parts are placed (`eda place examples/mcu_board_30plus.yaml -o <w>`, `eda board new examples/mcu_board_30plus.yaml
// -o <dir>`, `cp <w>/design.json <dir>/`), on a COPY: every footprint it makes it takes away again and every edit is undone. The autoplacer runs the placer in the
// server: a debug build takes about ten seconds a run.
//
// What it checks:
//   1  Place Footprint (`BOARD_EDITOR_CONTROL::PlaceFootprint`): A opens the Footprint Chooser; the chosen footprint is on the cursor (the Move tool carries it,
//      selected); R and F act on it before the click; the click puts it down where the pointer is, on the grid, turned and flipped, one undo step, nothing
//      selected; the tool then waits, and the next click opens the chooser again; a chooser cancelled leaves the tool waiting; Escape with a footprint carried
//      takes it away and the tool goes on; Escape with none ends the tool.
//   1b Place Footprint into a KiCad 10 placement grid: a footprint fresh from the library turns to the grid's frame where it is made (`prevFrameAngle = ANGLE_0`), and
//      is unturned again when carried out of it.
//   2  The microwave tools (`doInteractiveItemPlacement( .., IPO_REPEAT | IPO_ROTATE | IPO_FLIP )`): a gap made at the first click rides the cursor; R and F turn
//      and flip it; the second click puts it down there, one undo step; the tool waits for the next until Escape.
//   3  A footprint's position is the editor's point: a footprint placed at a point off the 0.1 mm step stays there (the server no longer rounds it).
//   4  Pack and Move (`PackAndMoveFootprints`, rectpack2D): footprints of several sizes packed apart, carried, dropped as one undo step.
//   5  Place Selected Footprints and Place Off-Board Footprints run the studio's placer: the selected ones (and only they) move, on the board and clear of the
//      others; an off-board footprint comes back on the board; each one undo step; nothing to place says nothing.
//
// It answers through `window.__placementCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed
(async () => {
  const results = [];
  const check = (window.__placementCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const redo = async () => { await E.run("common.Interactive.redo"); await sleep(900); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(600); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const button = (text) => [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === text);
  const S = () => E.state();

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(200); const a = readout();
    fire("pointermove", 500, 400); await sleep(200); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  let toScreen = null;
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(250); return [x, y]; };
  const click = async (xmm, ymm, wait = 900) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(wait); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    for (let i = 0; i < 3; i++) { const c = document.querySelector("[data-chooser-cancel]"); if (!c) break; c.click(); await sleep(300); }
    for (let i = 0; i < 3 && (S().placeFootprint !== "idle" || S().picker); i++) await escape();
    results.push(entry);
  };

  const poses = async () => Object.fromEntries((await board()).parts.filter((p) => p.placed).map((p) => [p.ref, { at: p.at, rot: p.rot, side: p.side, courtyard: p.courtyard, footprint: p.footprint }]));
  const overlaps = (a, b) => a && b && a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  const turned = (rot) => near(((rot % 360) + 360) % 360, 90, 0.5) || near(((rot % 360) + 360) % 360, 270, 0.5);

  /** In the open Footprint Chooser: search, choose the row `id`, OK. */
  const choose = async (query, id) => {
    const root = await until(() => document.querySelector("[data-footprint-chooser]"), 8000);
    if (!root) return "the chooser did not open";
    setValue(root.querySelector("#library-tree-search"), query);
    const row = await until(() => root.querySelector(`[data-chooser-tree] [data-id="${id}"]`), 90000);
    if (!row) return `no ${id} in the chooser`;
    row.click();
    await sleep(500);
    const ok = root.querySelector("[data-chooser-ok]");
    if (!ok || ok.disabled) return "OK is not enabled";
    ok.click();
    return null;
  };
  const TP = "TestPoint:TestPoint_Pad_D1.0mm";
  const tps = async () => (await board()).parts.filter((p) => p.placed && /TestPoint_Pad_D1\.0mm/.test(p.footprint ?? "")).map((p) => p.ref);

  const strictBox = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = !!strictBox?.checked;
  const setStrict = async (on) => { if (strictBox && strictBox.checked !== on) { strictBox.click(); await sleep(400); } };

  // --------------------------------------------------------------------------------------------------------------------------- run
  toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }
  const start = await poses();
  // free spots of this board (its outline is about (1.9, 4.1) to (28.3, 28.5) mm): the corners clear of its parts
  const FREE = [[4.0, 26.5], [26.5, 5.5], [4.0, 6.0], [26.5, 14.5]];

  (async () => {
    await scenario("Place Footprint: A opens the chooser, the footprint rides the cursor, R and F act on it, the click puts it down on the grid as one undo step, and the tool asks again", async (t) => {
      const before = await tps();
      await E.run("pcbnew.EditorControl.placeFootprint");
      await sleep(400);
      t(S().placeFootprint === "choosing", `A opens the chooser (${S().placeFootprint})`);
      const err = await choose("TestPoint_Pad_D1.0mm", TP);
      if (!t(err === null, err ?? "a test point chosen")) return;
      t(!!(await until(() => S().placeFootprint === "carrying" && S().tool === "move", 15000)), `it is carried by the Move tool (${S().placeFootprint}, ${S().tool})`);
      const made = (await tps()).filter((r) => !before.includes(r));
      t(made.length === 1, `one footprint made for the carry (${made})`);
      const ref = made[0];
      t(S().selection.some((s) => s.id === ref), "it is selected while carried (the edit tool's R and F act on the selection)");
      const [tx, ty] = FREE[0];
      await hover(tx - 1, ty - 1);
      await hover(tx, ty);
      t(S().moving && S().moving.refs.includes(ref), "the pointer carries it");
      await E.run("pcbnew.InteractiveEdit.rotateCcw");
      await sleep(300);
      t(S().moving?.turns === 1, `R turns it while carried (${S().moving?.turns})`);
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(300);
      t(S().moving?.flipped === true, "F flips it while carried");
      await click(tx, ty, 1500);
      const p = (await poses())[ref];
      t(!!p && near(p.at[0], tx * 1e6, 600_000) && near(p.at[1], ty * 1e6, 600_000), `the click puts it where the pointer is (${p?.at})`);
      const grid = S().grid;
      t(!!p && grid > 0 && p.at[0] % grid === 0 && p.at[1] % grid === 0, `on the grid of ${grid} nm (${p?.at})`);
      t(!!p && turned(p.rot) && p.side === "bottom", `turned and flipped (${p?.rot}, ${p?.side})`);
      t(S().selection.length === 0, "nothing is selected once it is down (`selectionClear` before the push)");
      t(!!(await until(() => S().placeFootprint === "waiting", 4000)), `the tool goes on: it waits for a click (${S().placeFootprint}, ${S().picker})`);
      await undo();
      t(!(await tps()).includes(ref), "one undo step takes the footprint and its drop back");
      await redo();
      t((await tps()).includes(ref), "and redo brings it");
      // the next click opens the chooser again; Cancel leaves the tool waiting
      if (S().placeFootprint !== "waiting") await E.run("pcbnew.EditorControl.placeFootprint");
      await click(FREE[1][0], FREE[1][1], 600);
      t(!!(await until(() => document.querySelector("[data-footprint-chooser]"), 5000)), "the next click opens the chooser again (`PlaceFootprint`'s loop)");
      document.querySelector("[data-chooser-cancel]")?.click();
      await sleep(500);
      t(S().placeFootprint === "waiting", `a chooser cancelled leaves the tool waiting (${S().placeFootprint})`);
      await escape();
      t(S().placeFootprint === "idle", `Escape with nothing carried ends the tool (${S().placeFootprint})`);
      await click(FREE[1][0], FREE[1][1], 600);
      t(!document.querySelector("[data-footprint-chooser]"), "a click then opens nothing");
      await cmd({ op: "rip", part: ref });
    });

    await scenario("Place Footprint: Escape while a footprint is carried takes it away and the tool goes on", async (t) => {
      const before = await tps();
      await E.run("pcbnew.EditorControl.placeFootprint");
      const err = await choose("TestPoint_Pad_D1.0mm", TP);
      if (!t(err === null, err ?? "a test point chosen")) return;
      // carried once the Move tool holds it (the tool's stage says "carrying" from the pick, before the footprint is made and picked up)
      await until(() => S().placeFootprint === "carrying" && S().tool === "move", 15000);
      await hover(FREE[2][0], FREE[2][1]);
      await until(() => S().moving, 4000);
      t((await tps()).length === before.length + 1 && !!S().moving, "the footprint is carried");
      await escape();
      t(!!(await until(async () => (await tps()).length === before.length, 6000)), "Escape takes it away (`commit.Revert()`)");
      t(!!(await until(() => S().placeFootprint === "waiting", 4000)), `the tool goes on (${S().placeFootprint})`);
      await escape();
      t(S().placeFootprint === "idle", "a second Escape ends it");
    });

    await scenario("Place Footprint into a placement grid (KiCad 25ec2335): a footprint fresh from the library turns to the grid's frame where it is made, and is unturned when carried out", async (t) => {
      // `PlaceFootprint`'s `applyPlacementFrameOrientation` with `prevFrameAngle = ANGLE_0` for a footprint fresh from the library (a move's pick-up counts the frame there as applied).
      const g = FREE[2]; // inside the outline, with room round it (Strict refuses a drop past the edge)
      const n = ((await board()).drawings?.grid_items ?? []).length;
      await cmd({ op: "pcb_item", verb: "add_grid_item", grid_item: { kind: "cartesian", at: { x: Math.round(g[0] * 1e6), y: Math.round(g[1] * 1e6) }, extent: { x: 8_000_000, y: 8_000_000 }, spacing: { x: 500_000, y: 500_000 }, angle: 30, phi_extent_deg: 360, phi_spacing_deg: 10, priority: 1, tick_interval: 0, affects: { cursor: true, routing: true, placement: true } } });
      const grid = await until(async () => ((await board()).drawings?.grid_items ?? []).length === n + 1 && ((await board()).drawings?.grid_items ?? []).at(-1), 8000);
      if (!t(!!grid && grid.angle === 30, "a placement grid turned 30 degrees")) return;
      const placeAt = async (to) => {
        const before = await tps();
        await hover(g[0] + 0.4, g[1] + 0.4);
        await E.run("pcbnew.EditorControl.placeFootprint");
        await sleep(400);
        const err = await choose("TestPoint_Pad_D1.0mm", TP);
        if (err) return { err };
        if (!(await until(() => S().placeFootprint === "carrying" && S().tool === "move", 15000))) return { err: `not carried (${S().placeFootprint}, ${S().tool})` };
        await hover(g[0] + 0.5, g[1] + 0.5);
        await hover(to[0], to[1]);
        await click(to[0], to[1], 1500);
        const ref = (await tps()).find((r) => !before.includes(r));
        const p = ref ? (await board()).parts.find((q) => q.ref === ref) : null;
        return { ref, p };
      };
      // KiCad's orientation (counter-clockwise; `rot` is the studio's clockwise angle)
      const norm = (r) => ((((r ?? 0) % 360) + 540) % 360) - 180;
      const inside = await placeAt([g[0] + 1.2, g[1] + 1.2]);
      if (t(!inside.err && !!inside.p && Math.abs(inside.p.at[0] / 1e6 - (g[0] + 1.2)) < 1 && Math.abs(inside.p.at[1] / 1e6 - (g[1] + 1.2)) < 1, inside.err ?? `put down in the grid where the pointer is (${inside.p?.at})`)) {
        t(Math.abs(norm(inside.p.orientation) - 30) < 0.01, `made inside the grid, it turns to the grid's 30 degrees at once (${inside.p.orientation})`);
        await undo();
      }
      await escape();
      const outside = await placeAt(FREE[3]);
      if (t(!outside.err && !!outside.p, outside.err ?? `placed outside the grid (${outside.p?.at})`)) {
        t(Math.abs(norm(outside.p.orientation)) < 0.01 && Math.abs(outside.p.at[0] / 1e6 - FREE[3][0]) < 1, `made inside, carried out of the grid, it is unturned again (${outside.p.orientation} at ${outside.p.at})`);
        await undo();
      }
      await escape();
      await cmd({ op: "pcb_item", verb: "delete_grid_item", id: grid.id });
      t(((await board()).drawings?.grid_items ?? []).length === n, "the grid item is gone again");
    });

    await scenario("Draw Microwave Gaps: the gap rides the cursor from the first click; R and F turn and flip it; the second click puts it down there, one undo step", async (t) => {
      const isGap = (p) => /^muwave_gap/.test(p.footprint ?? "");
      const gaps = async () => Object.entries(await poses()).filter(([, p]) => isGap(p)).map(([r]) => r);
      const before = await gaps();
      await E.run("pcbnew.MicrowaveTool.createGap");
      await sleep(300);
      const [x0, y0] = FREE[3];
      await click(x0, y0, 600);
      const input = await until(() => tid("mw-entry"), 4000);
      if (!t(!!input, "the click opens the prompt")) return;
      setValue(input, "0.5");
      await sleep(150);
      button("OK").click();
      await sleep(1200);
      const made = (await gaps()).filter((r) => !before.includes(r));
      t(made.length === 1, `the gap is made at the first click (${made})`);
      const ref = made[0];
      if (!ref) return;
      t(S().tool === "move" && S().selection.some((s) => s.id === ref), `and carried (${S().tool})`);
      const [x1, y1] = [x0 - 1.5, y0 + 2];
      await hover(x0, y0 + 1);
      await hover(x1, y1);
      await E.run("pcbnew.InteractiveEdit.rotateCcw");
      await sleep(300);
      t(S().moving?.turns === 1, "R turns it before the click (`IPO_ROTATE`)");
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(300);
      t(S().moving?.flipped === true, "F flips it (`IPO_FLIP`)");
      await click(x1, y1, 1500);
      const p = (await poses())[ref];
      t(!!p && near(p.at[0], x1 * 1e6, 600_000) && near(p.at[1], y1 * 1e6, 600_000), `the second click puts it where the pointer went (${p?.at} for ${x1}, ${y1})`);
      t(!!p && turned(p.rot) && p.side === "bottom", `turned and flipped (${p?.rot}, ${p?.side})`);
      t(/Draw Microwave Gaps/.test(S().picker ?? ""), `the tool waits for the next footprint (\`IPO_REPEAT\`: ${S().picker})`);
      await escape();
      await undo();
      t(!(await gaps()).includes(ref), "one undo step takes it back, made and moved");
    });

    await scenario("a footprint's position is the editor's point: placed off the 0.1 mm step it stays there", async (t) => {
      const before = await tps();
      const [x, y] = [Math.round(FREE[2][0] * 1e6) + 50_000, Math.round(FREE[2][1] * 1e6) + 25_400];
      const r = await cmd({ op: "place_footprint", footprint: TP, at: { x, y } });
      t(r.ok, `placed (${r.message})`);
      const ref = (await tps()).find((q) => !before.includes(q));
      const p = ref ? (await poses())[ref] : null;
      t(!!p && p.at[0] === x && p.at[1] === y, `the server keeps the point it was given (${p?.at} for ${x}, ${y}): no rounding to 0.1 mm`);
      if (ref) await cmd({ op: "rip", part: ref });
    });

    await scenario("Pack and Move packs footprints of several sizes apart (rectpack2D), carries them and drops them as one undo step", async (t) => {
      const refs = ["R1", "R2", "C5", "C6", "D1", "D8"];
      const was = await poses();
      // KiCad has no Strict: the packed block lands where it lands, on other footprints as like as not
      await setStrict(false);
      await hover(12, 8);
      await select(refs);
      await E.run("pcbnew.InteractiveEdit.packAndMoveFootprints");
      await sleep(500);
      await hover(12.2, 8.2);
      t(S().tool === "move" && S().moving && refs.every((r) => S().moving.refs.includes(r)), `the packed footprints are carried (${S().tool})`);
      await click(12.5, 8.5, 1800);
      await setStrict(strictWas);
      const now = await poses();
      t(refs.some((r) => now[r].at[0] !== was[r].at[0] || now[r].at[1] !== was[r].at[1]), "the drop moved them");
      let apart = true;
      for (let i = 0; i < refs.length; i++) for (let j = i + 1; j < refs.length; j++) if (overlaps(now[refs[i]].courtyard, now[refs[j]].courtyard)) apart = false;
      t(apart, "no two packed courtyards overlap");
      await undo();
      const back = await poses();
      t(refs.every((r) => back[r].at[0] === was[r].at[0] && back[r].at[1] === was[r].at[1]), "one undo step puts all of them back");
    });

    await scenario("Place Selected Footprints runs the studio's placer on the selection: those two move, on the board and clear of the others, the rest stay; one undo step", async (t) => {
      t((await E.actions()).some((a) => a.id === "pcbnew.Autoplacer.autoplaceSelected"), "Place Selected Footprints is an action");
      const was = await poses();
      await select(["R6", "R7"]);
      const rev = S().revision;
      await E.run("pcbnew.Autoplacer.autoplaceSelected");
      t(!!(await until(() => S().revision !== rev, 60000)), "the board changes");
      await sleep(800);
      const now = await poses();
      t(["R6", "R7"].some((r) => now[r].at[0] !== was[r].at[0] || now[r].at[1] !== was[r].at[1] || now[r].rot !== was[r].rot), `the selected footprints are placed anew (R6 ${now.R6.at}, R7 ${now.R7.at})`);
      t(Object.keys(was).filter((r) => r !== "R6" && r !== "R7").every((r) => now[r].at[0] === was[r].at[0] && now[r].at[1] === was[r].at[1] && now[r].rot === was[r].rot), "every other footprint stays where it was");
      const outline = (await board()).outline;
      const [ox0, oy0, ox1, oy1] = [Math.min(...outline.map((p) => p[0])), Math.min(...outline.map((p) => p[1])), Math.max(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))];
      for (const r of ["R6", "R7"]) {
        const c = now[r].courtyard;
        t(c[0] >= ox0 && c[1] >= oy0 && c[2] <= ox1 && c[3] <= oy1, `${r} on the board`);
        t(Object.keys(now).filter((q) => q !== r).every((q) => !overlaps(c, now[q].courtyard)), `${r} clear of every other footprint`);
      }
      await undo();
      const back = await poses();
      t(["R6", "R7"].every((r) => back[r].at[0] === was[r].at[0] && back[r].at[1] === was[r].at[1]), "one undo step puts them back");
      // nothing selected that is a footprint: nothing happens, and nothing is said
      await select([]);
      const errs = E.errors().length;
      const rev2 = S().revision;
      await E.run("pcbnew.Autoplacer.autoplaceSelected");
      await sleep(1500);
      t(S().revision === rev2 && E.errors().length === errs, "with no footprint selected nothing happens");
    });

    await scenario("Place Off-Board Footprints brings a footprint off the board back onto it with the placer, the others fixed; one undo step", async (t) => {
      const was = await poses();
      await cmd({ op: "move_items", ids: ["R8"], dx: 20_000_000, dy: 0 });
      await E.run("common.Interactive.refresh").catch(() => {});
      await sleep(800);
      const off = (await poses()).R8;
      t(off.at[0] > 28_300_000, `R8 is off the board (${off.at})`);
      const rev = S().revision;
      await E.run("pcbnew.Autoplacer.autoplaceOffboard");
      t(!!(await until(() => S().revision !== rev, 60000)), "the board changes");
      await sleep(800);
      const now = await poses();
      t(now.R8.at[0] < 28_300_000 && now.R8.at[0] > 1_890_000 && now.R8.at[1] > 4_050_000 && now.R8.at[1] < 28_530_000, `R8 is on the board again (${now.R8.at})`);
      t(Object.keys(was).filter((r) => r !== "R8").every((r) => now[r].at[0] === was[r].at[0] && now[r].at[1] === was[r].at[1]), "the footprints on the board stay");
      t(Object.keys(now).filter((q) => q !== "R8").every((q) => !overlaps(now.R8.courtyard, now[q].courtyard)), "R8 clear of the others");
      await undo();
      t((await poses()).R8.at[0] > 28_300_000, "one undo step puts it back off the board");
      await cmd({ op: "move_items", ids: ["R8"], dx: -20_000_000, dy: 0 });
      // with nothing off the board: nothing happens, nothing said
      await sleep(600);
      const errs = E.errors().length;
      const rev2 = S().revision;
      await E.run("pcbnew.Autoplacer.autoplaceOffboard");
      await sleep(12000);
      t(E.errors().length === errs, `nothing off the board: nothing said (${E.errors().slice(errs).map((e) => e.message)})`);
      t(S().revision === rev2 || JSON.stringify(Object.values(await poses()).map((p) => p.at)) === JSON.stringify(Object.values(was).map((p) => p.at)), "and the board is as it was");
    });

    // everything back as it was
    const end = await poses();
    const moved = Object.keys(start).filter((r) => !end[r] || end[r].at[0] !== start[r].at[0] || end[r].at[1] !== start[r].at[1]);
    results.push({ name: "the board is left as it was found", ok: moved.length === 0 && Object.keys(end).length === Object.keys(start).length, notes: [moved.length === 0 ? "ok: no footprint moved" : `FAIL: moved ${moved}`] });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
