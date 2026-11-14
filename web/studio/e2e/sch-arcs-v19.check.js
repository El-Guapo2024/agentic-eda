// The schematic's Draw Arc assistant (UI-LEFT-3 S3-28; task-v19-scharcs): KiCad 25ec2335's `ARC_ASSISTANT` writes its cursor strings next to the cursor --
//   centre mode   "r" and "θ" while the start is placed, "Δθ" and "θ" while the end is;
//   start-end-mid "r", "s" (the sagitta) and "Δθ" once the third point is under the cursor;
//   start-dir-end "θ" (the heading) while the direction is placed, then "r" and "Δθ";
// and no angle snap of the direction (eeschema's tool sets none: `drawManagedShape` never calls `SetAngleSnap`); the board's Draw Arc shows the same assistant.
// Every arc is cancelled: nothing is added.
//
// Run it in the page of a served board made from examples/ldo.yaml (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --ui web/studio/dist`).
// It answers through `window.__schArcsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=4
(async () => {
  const results = [];
  const check = (window.__schArcsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  const E = window.__eda;
  const state = () => E.state();
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().drawState); i++) { await E.run("common.Interactive.cancel"); await sleep(300); } };
  const sch = () => json("/api/schematic");
  const mm = (v) => Math.round(v * 1e6);
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; return !!cond; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    results.push({ name, ok, notes });
    await toSelect();
  };

  await sleep(1500);
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screen = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const click = async (x, y) => { const [px, py] = screen(x, y); fire("pointermove", px, py); await sleep(250); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(600); };
  const hover = async (x, y) => { const [px, py] = screen(x, y); fire("pointermove", px, py); await sleep(500); };
  /** The cursor strings the assistant last wrote (`paintRuler`'s record, tag `arcAssistant`). */
  const readout = () => (window.__edaRuler?.arcAssistant?.texts ?? []).join(" | ");
  const clearReadout = () => { if (window.__edaRuler) delete window.__edaRuler.arcAssistant; };
  const X = mm(152.4), Y = mm(127);
  const start = await sch();
  const unit = state().units === "mil" ? "mils" : state().units === "in" ? "in" : "mm";

  await scenario("S3-28: the centre mode's readout -- r and θ, then Δθ and θ", async (t) => {
    await run("eeschema.InteractiveDrawing.drawArcCenter");
    t(state().tool === "sch_arc", `the arc tool is armed (${state().tool})`);
    clearReadout();
    await click(X, Y);
    await hover(X + mm(10.16), Y);
    const r = mm(10.16) / (unit === "mils" ? 25_400 : unit === "in" ? 25_400_000 : 1e6);
    t(readout().startsWith(`r: ${r.toFixed(unit === "mils" ? 1 : unit === "in" ? 4 : 3)} ${unit} | θ: 0.0°`), `while the start is placed (${readout()})`);
    await click(X + mm(10.16), Y);
    await hover(X, Y - mm(10.16));
    t(readout() === "Δθ: 90.0° | θ: 90.0°", `while the end is placed, a quarter counterclockwise (${readout()})`);
    const shadow = window.__edaRuler?.arcAssistant?.shadow;
    t(!!shadow && shadow.texts === 2, `the strings have their drop shadows (${JSON.stringify(shadow)})`);
  });

  await scenario("S3-28: start, end, mid -- r, s and Δθ; the chord alone first", async (t) => {
    await run("eeschema.InteractiveDrawing.drawArcStartEndMid");
    clearReadout();
    await click(X, Y);
    await hover(X + mm(10.16), Y);
    t(readout() === "", `no strings while the chord's end is placed (${readout()})`);
    await click(X + mm(10.16), Y);
    await hover(X + mm(5.08), Y - mm(5.08));
    const parts = readout().split(" | ");
    t(parts.length === 3 && parts[0].startsWith("r: ") && parts[1].startsWith("s: ") && /^Δθ: -?180\.0°$/.test(parts[2]), `a half circle: r, s and the sweep (${readout()})`);
    t(parts[0].slice(3) === parts[1].slice(3), `its sagitta is its radius (${readout()})`);
  });

  await scenario("S3-28: start, direction, end -- the heading as it is, no 45-degree snap, then r and Δθ", async (t) => {
    await run("eeschema.InteractiveDrawing.drawArcStartDirEnd");
    clearReadout();
    await click(X, Y);
    // a direction a little off 30 degrees down-right: kept as it is (KiCad's schematic tool sets no angle snap)
    await hover(X + mm(10.16), Y + mm(5.08));
    const m = /^θ: (-?\d+\.\d)°$/.exec(readout());
    t(!!m && Math.abs(Number(m[1]) - 26.6) < 0.2, `the heading, unsnapped (${readout()})`);
    await click(X + mm(10.16), Y + mm(5.08));
    await hover(X + mm(20.32), Y);
    const parts = readout().split(" | ");
    t(parts.length === 2 && parts[0].startsWith("r: ") && parts[1].startsWith("Δθ: "), `then the radius and the sweep (${readout()})`);
  });

  await scenario("the board's Draw Arc: the same assistant in LAYER_AUX_ITEMS over the arc on its layer (KiCad 25ec2335)", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click();
    await until(() => state().tab === "pcb");
    await sleep(1500);
    const arcs = async () => ((await fetch("/api/board.kicad_pcb", { cache: "no-store" }).then((r) => r.text())).match(/\(gr_arc/g) || []).length;
    const before = await arcs();
    await run("pcbnew.InteractiveDrawing.arc");
    clearReadout();
    await click(mm(100), mm(80));
    await hover(mm(110), mm(80));
    t(/^r: 10\.000 mm \| θ: 0\.0°$/.test(readout()) || /^r: .* \| θ: 0\.0°$/.test(readout()), `r and θ while the start is placed (${readout()})`);
    await click(mm(110), mm(80));
    await hover(mm(100), mm(70));
    t(readout() === "Δθ: 90.0° | θ: 90.0°", `Δθ and θ while the end is placed (${readout()})`);
    await toSelect();
    const after = await arcs();
    t(before === after, `nothing added (${before} -> ${after})`);
  });

  const end = await sch();
  results.push({ name: "nothing was added", ok: (end.graphics?.length ?? 0) === (start.graphics?.length ?? 0), notes: [`graphics ${start.graphics?.length ?? 0} -> ${end.graphics?.length ?? 0}`] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
