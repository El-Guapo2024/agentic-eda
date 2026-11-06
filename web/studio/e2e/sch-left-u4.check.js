// The schematic editor's smaller leftovers of UI-LEFT (task-u4-schleft), the visible ones:
//   S13  Draw Arc takes KiCad 25ec2335's three clicks (centre, start, end angle; `EE_GRAPHIC_TOOL::DrawArc`), and the arc point editor drags its end
//        (`EDA_ARC_POINT_EDIT_BEHAVIOR`), one undo step each.
//   S18  a click of the no-connect tool on a flag removes it (`SingleClickPlace`: "No connects can be "toggled"/removed by clicking on them again").
//   S19  Alt+S swaps two sheets (`SCH_EDIT_TOOL::Swap` over `SwappableItems`), each with its pins.
//   S21  two sheets of one name are allowed and kicad-cli's ERC reports them (`duplicate_sheet_names`); a new sheet's dialog says "Untitled Sheet".
//   S17  a dropped schematic is placed with the options last chosen, no chooser; as contents it rides the cursor before the click, one undo step.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`,
// with KiCad's libraries: `KICAD_PATH`), on a COPY of a board: every scenario undoes what it did.
//
// It answers through `window.__schLeftCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schLeftCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schLeftSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
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
    window.__schLeftSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const sch = () => json("/api/schematic");
  const state = () => E.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 3 && state().tool !== "select"; i++) await escape(); };
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); };
  const clickAt = async (x, y, extra = {}) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); fire("pointerdown", px, py, { buttons: 1, ...extra }); await sleep(60); fire("pointerup", px, py, extra); await sleep(900); };
  const dragFrom = async ([x0, y0], [x1, y1]) => {
    const [a, b] = screenOf(x0, y0);
    const [c, d] = screenOf(x1, y1);
    fire("pointermove", a, b); await sleep(200);
    fire("pointerdown", a, b, { buttons: 1 }); await sleep(60);
    for (let i = 1; i <= 6; i++) { fire("pointermove", a + ((c - a) * i) / 6, b + ((d - b) * i) / 6, { buttons: 1 }); await sleep(80); }
    fire("pointerup", c, d); await sleep(1200);
  };
  const lastToast = () => [...document.querySelectorAll("div")].find((el) => el.style.zIndex === "3000" && el.style.position === "absolute")?.textContent ?? "";
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    results.push({ name, ok, notes });
    await toSelect();
  };
  const mm = (v) => Math.round(v * 1e6);
  // A free part of the root sheet, far from the module sheets, on the 50 mil grid the tools snap to
  const X = mm(25.4), Y = mm(160.02);

  // the Schematic tab
  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  const start = await sch();

  // ------------------------------------------------------------------------------------------------------------------------ S13
  await scenario("S13: Draw Arc takes three clicks (centre, start, end) and the arc point editor drags its end, one undo step each", async (t) => {
    const before = (await sch()).graphics?.length ?? 0;
    await run("eeschema.InteractiveDrawing.drawArc");
    t(state().tool === "sch_arc", `the arc tool is armed (${state().tool})`);
    const c = [X, Y], s = [X + mm(10.16), Y], e = [X, Y - mm(10.16)];
    await clickAt(...c);
    await clickAt(...s);
    let mid = (await sch()).graphics?.length ?? 0;
    t(mid === before, "two clicks make no arc yet (centre, then start)");
    await hoverAt(X + mm(5), Y - mm(9));
    await clickAt(...e);
    const after = await until(async () => { const g = (await sch()).graphics ?? []; return g.length === before + 1 ? g : null; }, 6000);
    const arc = after?.[after.length - 1];
    t(arc?.shape?.type === "arc", `the third click puts an arc down (${JSON.stringify(arc?.shape)})`);
    if (arc?.shape?.type === "arc") {
      const pts = [arc.shape.start, arc.shape.end].map((p) => [p.x, p.y]);
      const r = (p) => Math.hypot(p[0] - c[0], p[1] - c[1]);
      t(pts.every((p) => Math.abs(r(p) - mm(10.16)) < 2000), `its ends are on the radius the start fixed (${pts.map((p) => Math.round(r(p))).join(", ")})`);
      t(pts.some((p) => Math.abs(p[0] - s[0]) < 2000 && Math.abs(p[1] - s[1]) < 2000), "one end is the start clicked");
    }
    t(state().tool === "sch_arc", "the tool stays armed for the next arc");
    await toSelect();
    // the arc point editor: select the arc and drag its end point outward
    if (arc) {
      await clickAt(X + mm(7.18), Y - mm(7.18));
      await until(() => state().selection.some((s) => s.id === arc.id), 3000);
      const points = container().dataset.arcPoints ?? "";
      t(/start:.*mid:.*end:.*center:/.test(points), `the point editor shows start, middle, end and centre (${points})`);
      const end = points.split(" ").find((p) => p.startsWith("end:"))?.slice(4).split(",").map(Number);
      if (end) {
        await dragFrom(end, [end[0] + (end[0] === X ? 0 : mm(5.08)), end[1] - (end[0] === X ? mm(5.08) : 0)]);
        const edited = (await sch()).graphics?.find((g) => g.id === arc.id);
        t(edited && JSON.stringify(edited.shape) !== JSON.stringify(arc.shape), `the drag edits the arc (${JSON.stringify(edited?.shape)})`);
        await undo();
        const back = (await sch()).graphics?.find((g) => g.id === arc.id);
        t(back && JSON.stringify(back.shape) === JSON.stringify(arc.shape), "one undo puts the arc back as drawn");
      }
    }
    await undo();
    t(((await sch()).graphics?.length ?? 0) === before, "one undo takes the arc away");
  });

  // ------------------------------------------------------------------------------------------------------------------------ S18
  await scenario("S18: a click of the no-connect tool on a flag removes it", async (t) => {
    const before = (await sch()).no_connects.length;
    await run("eeschema.InteractiveDrawing.placeNoConnect");
    t(state().tool === "sch_no_connect", `the no-connect tool is armed (${state().tool})`);
    await clickAt(X + mm(50.8), Y);
    t((await until(async () => ((await sch()).no_connects.length === before + 1 ? true : null), 4000)) === true, "a click places a flag");
    await clickAt(X + mm(50.8), Y);
    t((await until(async () => ((await sch()).no_connects.length === before ? true : null), 4000)) === true, "a second click on it removes it");
    await toSelect();
    await undo();
    t((await sch()).no_connects.length === before + 1, "an undo brings the removed flag back");
    await undo();
    t((await sch()).no_connects.length === before, "and one more takes the placement away");
  });

  // ------------------------------------------------------------------------------------------------------------------------ S19 + S21
  await scenario("S21 + S19: two sheets of one name are allowed and ERC reports them; Alt+S swaps them with their pins", async (t) => {
    const sheets0 = (await sch()).sheets.length;
    const a = await cmd({ op: "add_sheet", name: "Dup", file: "dup_a.kicad_sch", at: { x: X, y: Y + mm(20.32) }, size: [mm(20.32), mm(10.16)] });
    const b = await cmd({ op: "add_sheet", name: "Dup", file: "dup_b.kicad_sch", at: { x: X + mm(60.96), y: Y + mm(40.64) }, size: [mm(25.4), mm(12.7)] });
    t(a.ok !== false && b.ok !== false, `both are added (${JSON.stringify([a.ok, b.ok, a.message ?? "", b.message ?? ""])})`);
    await sleep(1200);
    const added = (await sch()).sheets.filter((s) => s.name === "Dup");
    t(added.length === 2, `two sheets named Dup (${added.length})`);
    const erc = await json("/api/erc");
    const kinds = (erc.violations ?? []).map((v) => v.check ?? v.type);
    t(kinds.includes("duplicate_sheet_names"), `kicad-cli's ERC reports duplicate_sheet_names (${[...new Set(kinds)].join(", ")})`);
    // Alt+S: select both (a click, then a shift click inside each), swap
    if (added.length === 2) {
      const [s1, s2] = added;
      // on the sheet's top border (`SCH_SHEET::HitTest`)
      const inside = (s) => [s.at[0] + Math.round(s.size[0] / 2 / 1270000) * 1270000, s.at[1]];
      await clickAt(...inside(s1));
      await clickAt(...inside(s2), { shiftKey: true });
      await until(() => state().selection.length === 2, 3000);
      t(state().selection.length === 2, `both sheets are selected (${JSON.stringify(state().selection)})`);
      await run("eeschema.InteractiveEdit.swap");
      await sleep(1200);
      const swapped = (await sch()).sheets;
      const n1 = swapped.find((s) => s.id === s1.id), n2 = swapped.find((s) => s.id === s2.id);
      t(n1 && n2 && n1.at[0] === s2.at[0] && n1.at[1] === s2.at[1] && n2.at[0] === s1.at[0] && n2.at[1] === s1.at[1], `Alt+S trades their corners (${JSON.stringify([n1?.at, n2?.at])})`);
      await undo();
      const back = (await sch()).sheets.find((s) => s.id === s1.id);
      t(back && back.at[0] === s1.at[0] && back.at[1] === s1.at[1], "one undo puts them back");
    }
    await undo();
    await undo();
    t((await sch()).sheets.length === sheets0, "two undos take the sheets away");
    // a new sheet's dialog: Untitled Sheet / untitled.kicad_sch, as every new sheet starts
    await run("eeschema.InteractiveDrawing.drawSheet");
    await clickAt(X, Y + mm(20.32));
    await clickAt(X + mm(25.4), Y + mm(35.56));
    const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Hierarchical Sheet Properties/.test(d.textContent ?? "")), 4000);
    const inputs = dlg ? [...dlg.querySelectorAll("input")].map((i) => i.value) : [];
    t(inputs[0] === "Untitled Sheet" && inputs[1] === "untitled.kicad_sch", `the dialog starts at Untitled Sheet / untitled.kicad_sch (${inputs.join(" / ")})`);
    if (dlg) [...dlg.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel")?.click();
    await sleep(300);
  });

  // ------------------------------------------------------------------------------------------------------------------------ S17
  await scenario("S17: a dropped schematic is in the hand with no chooser; as contents it rides the cursor and one click places it, one undo step", async (t) => {
    // Place as sheet off, in the chooser (the options a file given to Import Sheet is placed with)
    await run("eeschema.InteractiveDrawing.importSheet");
    const chooser = await until(() => [...document.querySelectorAll(".dialog, [role=dialog]")].find((d) => /Choose Schematic/.test(d.textContent ?? "")), 4000);
    const asSheet = chooser && [...chooser.querySelectorAll("label")].find((l) => /Place as sheet/.test(l.textContent ?? ""))?.querySelector("input");
    if (asSheet?.checked) asSheet.click();
    await sleep(200);
    const close = chooser && [...chooser.querySelectorAll("button")].find((x) => /Cancel|Close|×/.test(x.textContent.trim()));
    if (close) close.click(); else await escape();
    await sleep(300);
    // a schematic of a wire, a label on it and a text, as KiCad writes one
    const file =
      `(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "9e1f5a10-0000-4000-8000-00000000e001") (paper "A4") (lib_symbols) ` +
      `(wire (pts (xy 20.32 20.32) (xy 40.64 20.32)) (stroke (width 0) (type default)) (uuid "9e1f5a10-0000-4000-8000-00000000e002")) ` +
      `(label "DROPPED" (at 40.64 20.32 0) (fields_autoplaced yes) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid "9e1f5a10-0000-4000-8000-00000000e003")) ` +
      `(text "dropped text" (exclude_from_sim no) (at 20.32 30.48 0) (effects (font (size 1.27 1.27))) (uuid "9e1f5a10-0000-4000-8000-00000000e004")) ` +
      `(sheet_instances (path "/" (page "1"))))`;
    const before = await sch();
    const dt = new DataTransfer();
    dt.items.add(new File([file], "dropped_copy.kicad_sch", { type: "text/plain" }));
    const mk = (type) => new DragEvent(type, { bubbles: true, cancelable: true, composed: true, dataTransfer: dt });
    document.body.dispatchEvent(mk("dragenter"));
    document.body.dispatchEvent(mk("dragover"));
    await sleep(150);
    document.body.dispatchEvent(mk("drop"));
    await until(() => state().tool === "sch_import_sheet", 8000);
    t(state().tool === "sch_import_sheet", `the file is in the hand (${state().tool})`);
    t(![...document.querySelectorAll(".dialog, [role=dialog]")].some((d) => /Choose Schematic/.test(d.textContent ?? "")), "no chooser opens");
    await hoverAt(X + mm(200), Y);
    const ghost = await until(() => Number(container().dataset.importGhost ?? 0) > 0, 15000);
    t(!!ghost, `the contents ride the cursor before the click (${container().dataset.importGhost ?? 0} items)`);
    t((await sch()).wires.length === before.wires.length, "nothing is on the sheet yet");
    await clickAt(X + mm(200), Y);
    const placed = await until(async () => { const s = await sch(); return s.wires.length > before.wires.length && s.labels.some((l) => l.net === "DROPPED") ? s : null; }, 15000);
    t(!!placed, `the click places the wire and the label (${before.wires.length} -> ${placed?.wires.length} wires)`);
    await toSelect();
    await undo();
    const back = await sch();
    t(back.wires.length === before.wires.length && !back.labels.some((l) => l.net === "DROPPED"), "one undo takes them away");
  });

  // the sheet is as it began
  const end = await sch();
  results.push({ name: "the sheet is as it began", ok: end.symbols.length === start.symbols.length && end.sheets.length === start.sheets.length && end.wires.length === start.wires.length && (end.graphics?.length ?? 0) === (start.graphics?.length ?? 0) && end.no_connects.length === start.no_connects.length, notes: [`symbols ${start.symbols.length} -> ${end.symbols.length}, sheets ${start.sheets.length} -> ${end.sheets.length}, wires ${start.wires.length} -> ${end.wires.length}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
