// Board reader and writer losses, and the teardrops' fills (fix batches B and C, PARITY-pcb.md section 52): on a board imported from `e2e/board-io.kicad_pcb` (a rounded
// rectangle outline, a footprint whose Edge.Cuts notch the board's edge, a paste-only pad, a GND pour with a hole, a teardrop zone), the studio has the zone's hole, the board
// outline KiCad builds (the rounded corners and the footprint's notch cut out, the notch moving with its footprint), the derived .kicad_pcb has what the file had, and Fill All
// Zones (B) fills the teardrop and leaves the hole empty.
// Run it in the page of the served board (`eda import-kicad web/studio/e2e/board-io.kicad_pcb -o <dir>`, `eda board serve -C <dir>`), through `run-check.mjs` or pasted into the
// page; it answers through `window.__boardIoCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }. It moves a footprint and undoes it, and leaves the zones filled.
// run-all-checks: board=board-io minutes=5
(async () => {
  const results = [];
  const check = (window.__boardIoCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const state = () => json("/api/state");
  const fill = () => json("/api/fill");
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const boardFile = () => fetch("/api/board.kicad_pcb").then((r) => r.text());
  const ringArea = (ring) => Math.abs(ring.reduce((s, p, i) => { const q = ring[(i + 1) % ring.length]; return s + p[0] * q[1] - q[0] * p[1]; }, 0) / 2) / 1e12;
  const outlineArea = (polys) => polys.reduce((s, p) => s + ringArea(p.outer) - p.holes.reduce((h, r) => h + ringArea(r), 0), 0);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const zones = async () => (await state()).routing?.zones ?? [];

  (async () => {
    await scenario("the pour with a hole is one zone with its hole, and the derived file writes it so", async (t) => {
      const z = await zones();
      const pour = z.find((x) => x.net === "GND");
      t(z.length === 2, `two zones, not three (${z.length})`);
      t(pour && (pour.holes ?? []).length === 1 && pour.holes[0].length === 4, `the GND pour has its hole (${JSON.stringify(pour?.holes)})`);
      const text = await boardFile();
      const block = text.split("\t(zone").find((b) => b.includes('"GND"') && b.includes("(polygon")) ?? "";
      t((block.match(/\(polygon/g) ?? []).length === 2, "the derived .kicad_pcb has the zone's outline and its hole");
      t(/\(legacy_teardrops yes\)/.test(text), "a board with teardrops is written (legacy_teardrops yes), so kicad-cli's refill keeps them");
      t(/\(gr_rect \(start 0 0\) \(end 40 30\) \(radius 3\)/.test(text), "the rounded rectangle keeps its corner radius");
      const j1 = text.split("\t(footprint").find((b) => b.includes('"J1"')) ?? "";
      t(/\(fp_rect \(start -3 -2\) \(end 3 2\)/.test(j1) && /"Edge.Cuts"/.test(j1), "J1's notch is written inside J1, on Edge.Cuts");
      t(/\(layers "F\.Paste"\)/.test(j1) && /primitives/.test(j1), "the paste-only pad is still there, with its primitives");
    });

    await scenario("the board outline is KiCad's: the rounded corners and J1's notch cut out, and the notch moves with J1", async (t) => {
      const s = await state();
      const polys = s.outline_polys ?? [];
      const expected = 1200 - (4 - Math.PI) * 9 - 12;
      t(polys.length === 1, `one outline (${polys.length})`);
      const rect = (s.drawings?.shapes ?? []).find((x) => x.kind === "rect" && x.layer === "Edge.Cuts");
      t(rect?.corner_radius === 3_000_000, `the canvas is told the rectangle's corner radius (${rect?.corner_radius})`);
      t(Math.abs(outlineArea(polys) - expected) < 0.05, `its area is ${outlineArea(polys).toFixed(3)} mm², KiCad's ${expected.toFixed(3)}`);
      // kicad-cli says the same of this board: one `(self-intersecting)` from `TestBoardOutlinesGraphicItems` (the notch crosses the edge), and an outline of 1180.271 mm².
      const errs = s.outline_errors ?? [];
      t(errs.length === 1 && /self-intersecting/.test(errs[0].message), `one outline finding, KiCad's (self-intersecting): ${JSON.stringify(errs.map((e) => e.message))}`);
      const before = JSON.stringify(polys[0]?.outer);
      const r = await post({ op: "move_items", ids: ["J1"], dx: 5_000_000, dy: 0 });
      t(r.ok !== false, `J1 moved (${JSON.stringify(r).slice(0, 120)})`);
      await sleep(500);
      const moved = (await state()).outline_polys ?? [];
      t(JSON.stringify(moved[0]?.outer) !== before && Math.abs(outlineArea(moved) - expected) < 0.05, "the notch went with J1, the area the same");
      await __eda.run("common.Interactive.undo");
      await sleep(600);
      t(JSON.stringify(((await state()).outline_polys ?? [])[0]?.outer) === before, "Undo puts it back");
    });

    await scenario("Fill All Zones fills the teardrop and leaves the pour's hole empty", async (t) => {
      const r = await __eda.run("pcbnew.ZoneFiller.zoneFillAll");
      t(r.ok === true, `B ran (${r.toast ?? JSON.stringify(r).slice(0, 120)})`);
      let f = await fill();
      for (let i = 0; i < 30 && !f.zones.every((z) => z.filled); i++) { await sleep(300); f = await fill(); }
      const z = await zones();
      const td = z.find((x) => x.teardrop);
      const pour = z.find((x) => x.net === "GND");
      const tdFill = f.zones.find((x) => x.id === td?.id);
      const pourFill = f.zones.find((x) => x.id === pour?.id);
      t(tdFill?.filled === true && tdFill.area_nm2 > 1e10, `the teardrop has its fill (${JSON.stringify(tdFill && { filled: tdFill.filled, area: tdFill.area_nm2 })})`);
      t(pourFill?.filled === true && pourFill.area_nm2 / 1e12 < 352 - 48, `the pour is filled, and not over its hole: ${(pourFill?.area_nm2 / 1e12).toFixed(2)} mm² (outline 352, hole 48)`);
      const inside = (p, ring) => { let c = false; for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) { const [xi, yi] = ring[i], [xj, yj] = ring[j]; if ((yi > p[1]) !== (yj > p[1]) && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) c = !c; } return c; };
      t(!(pourFill?.fragments ?? []).some((ring) => inside([9_000_000, 16_000_000], ring) && ringArea(ring) > 0), "no copper at the hole's middle");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
