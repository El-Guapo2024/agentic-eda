// A placed symbol drawn with its library shapes' looks (UI-LEFT-2 YY2's leftover, task-v8-sch; PARITY-sch section 87): `SCH_PAINTER::draw( SCH_SHAPE )` of a symbol's
// shape in its border colour and line style, its hatch in its fill colour, a rounded rectangle, and a shape private to the Symbol Editor left off the sheet.
//
// Run it in the page of a served board (a COPY: `eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`), e.g.
// `node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/sch-symbol-looks.check.js 4`. It keeps a symbol `LooksT:LOOK` with the schematic and
// places `U877`.
// It answers through `window.__schSymbolLooksCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=4   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__schSymbolLooksCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const E = window.__eda;
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    results.push(entry);
  };
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** How many pixels of the whole canvas are within `tol` of `rgb`. */
  const count = (rgb, tol = 30) => {
    const c = canvas();
    if (!c || c.width === 0) return -1;
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const MAGENTA = [255, 0, 255];
  const GREEN = [0, 200, 0];
  const CYAN = [0, 200, 200];
  const LIB_ID = "LooksT:LOOK";

  await scenario("a placed symbol draws its shapes' border colour, line style, hatch and corner radius; a private shape stays off the sheet", async (t) => {
    if (E.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
    t(E.state().tab === "schematic", "the schematic is shown");
    const before = { magenta: count(MAGENTA), green: count(GREEN), cyan: count(CYAN) };
    const rect = { kind: "rectangle", unit: 0, body_style: 1, start: { x: -7.62, y: 7.62 }, end: { x: 7.62, y: -7.62 }, stroke_mm: 0.5, fill: "none" };
    const circle = { kind: "circle", unit: 0, body_style: 1, center: { x: 0, y: 0 }, radius_mm: 3.81, stroke_mm: 0.254, fill: "color" };
    const hidden = { kind: "rectangle", unit: 0, body_style: 1, start: { x: -6.35, y: -3.81 }, end: { x: -3.81, y: -6.35 }, stroke_mm: 0.5, fill: "none" };
    const symbol = {
      lib_id: LIB_ID,
      reference_prefix: "U",
      published: true,
      graphics: [rect, circle, hidden],
      pins: [{ number: "1", name: "A", electrical_type: "passive", shape: "line", at: { x: -10.16, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 0, body_style: 1, hidden: false }],
    };
    const r1 = await post({ op: "embed_lib_symbol", symbol });
    t(r1.ok !== false, `embed_lib_symbol: ${r1.message ?? "ok"}`);
    // the looks, as the Symbol Editor's Properties pane and Shape Properties set them beside each shape (`set_symbol_graphic_looks`)
    const kept = await json(`/api/symbol?lib_id=${encodeURIComponent(LIB_ID)}`);
    const idOf = (want) => kept.graphics?.find((g) => g.kind === want.kind && JSON.stringify(g.start ?? g.center) === JSON.stringify(want.start ?? want.center))?.id;
    for (const [shape, looks, line_style] of [
      [rect, { stroke_color: { r: 255, g: 0, b: 255, a: 255 }, radius_mm: 1.27 }, "dash"],
      [circle, { fill_color: { r: 0, g: 200, b: 0, a: 255 }, hatch: "cross_hatch" }, null],
      [hidden, { private: true, stroke_color: { r: 0, g: 200, b: 200, a: 255 } }, null],
    ]) {
      const r = await post({ op: "set_symbol_graphic_looks", lib_id: LIB_ID, id: idOf(shape), looks, line_style, thickness_mm: null });
      t(r.ok !== false, `set_symbol_graphic_looks ${shape.kind}: ${r.message ?? "ok"}`);
    }
    const r2 = await post({ op: "add_symbol", id: "U877", lib_id: LIB_ID, at: { x: 50_800_000, y: 50_800_000 }, rot_millideg: 0, value: "LOOK", footprint: "", unit: 1 });
    t(r2.ok !== false, `add_symbol: ${r2.message ?? "ok"}`);
    await sleep(1500);
    const sch = await json("/api/schematic");
    const lib = sch.lib_symbols?.[LIB_ID];
    if (!t(lib, "the placed symbol's library symbol is served")) return;
    const g = lib.graphics;
    t(g.length === 2, `the private shape is not drawn on a sheet: ${g.map((x) => x.kind).join(", ")}`);
    const r = g.find((x) => x.kind === "rectangle");
    t(r && r.line_style === "dash" && r.stroke_color?.r === 255 && r.stroke_color?.b === 255 && r.corner_radius_mm === 1.27, `the rectangle's looks: ${JSON.stringify(r)}`);
    const c = g.find((x) => x.kind === "circle");
    t(c && c.fill === "color" && c.hatch === "cross_hatch" && c.fill_color?.g === 200, `the circle's hatch: ${JSON.stringify(c)}`);
    await select(["U877"]);
    await E.run("common.Control.zoomFitSelection");
    await sleep(900);
    await select([]);
    await sleep(900);
    const after = { magenta: count(MAGENTA), green: count(GREEN), cyan: count(CYAN) };
    t(after.magenta > before.magenta + 50, `the border in its own colour: ${before.magenta} -> ${after.magenta} magenta pixels`);
    t(after.green > before.green + 20, `the hatch lines in the fill colour: ${before.green} -> ${after.green} green pixels`);
    t(after.cyan <= before.cyan, `the private shape is not drawn: ${before.cyan} -> ${after.cyan} cyan pixels`);
    // selected, the symbol is drawn in the selection colour: its own border colour gives way (`getRenderColor`)
    await select(["U877"]);
    await sleep(900);
    t(count(MAGENTA) < after.magenta, `selected, the border takes the selection colour: ${count(MAGENTA)} magenta pixels`);
    await select([]);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
