// Autoroute on a board with a cutout (UI-LEFT R4, GAPS item 27): the board editor's Route button (QuickActions, POST /api/route: the FreeRouting port,
// `eda_freeroute::design::route_design`) routes every connection round the window cut out of the board -- the cutout is a keep-out on every copper layer,
// as KiCad's `SPECCTRA_DB::fillBOUNDARY` writes it into the DSN -- with the copper-to-edge clearance kept, and kicad-cli's DRC finds no copper against
// the edge, no clearance, short or crossing, and nothing unconnected. Undo takes the routing back in one step.
//
// The board: `examples/through_hole_headers.yaml` placed with J3 along the top (11.5,3), J2 (7,20) and J1 (17,20) along the bottom, R1 (20,8) and R2 (20,14)
// on the right, and a rectangle on Edge.Cuts from (4,8) to (11,15) mm -- across the straight runs of SDA and SCL (`eda board new`, `eda board place .. --at`,
// `eda board shape add --kind rect --layer Edge.Cuts --pts "4,8 11,15" --width 0.05`, as tools/run-all-checks.sh's `cutout-board` makes it). Run it in the
// page of the served board through `run-check.mjs`; it answers through `window.__autorouteCutoutCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// It leaves the board as it found it (the routing undone).
// run-all-checks: board=cutout-board minutes=5   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__autorouteCutoutCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden pane throttles timers
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const state = () => json("/api/state");

  // The cutout, nm: the rectangle on Edge.Cuts, and the edge clearance the router keeps from it.
  const CUT = { x0: 4e6, y0: 8e6, x1: 11e6, y1: 15e6 };
  const EDGE = 500_000;
  const ring = [[CUT.x0, CUT.y0], [CUT.x1, CUT.y0], [CUT.x1, CUT.y1], [CUT.x0, CUT.y1]];
  const segPoint = (a, b, p) => {
    const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
    const l2 = dx * dx + dy * dy;
    const u = l2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
    return Math.hypot(a[0] + u * dx - p[0], a[1] + u * dy - p[1]);
  };
  const crosses = (a, b, c, d) => {
    const side = (p, q, r) => (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    return side(c, d, a) > 0 !== side(c, d, b) > 0 && side(a, b, c) > 0 !== side(a, b, d) > 0;
  };
  const inside = (p) => p[0] > CUT.x0 && p[0] < CUT.x1 && p[1] > CUT.y0 && p[1] < CUT.y1;
  // The least distance from a segment's copper to the cutout's rim (0 when it crosses or lies in it).
  const toCut = (a, b, halfWidth) => {
    if (inside(a) || inside(b)) return 0;
    let d = Infinity;
    for (let i = 0; i < 4; i++) {
      const [c, e] = [ring[i], ring[(i + 1) % 4]];
      if (crosses(a, b, c, e)) return 0;
      d = Math.min(d, segPoint(a, b, c), segPoint(c, e, a), segPoint(c, e, b));
    }
    return d - halfWidth;
  };

  (async () => {
    if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }

    await scenario("the board has its cutout: a hole in the board outline, nothing routed yet", async (t) => {
      const s = await state();
      const polys = s.outline_polys ?? [];
      t(polys.length === 1 && (polys[0].holes ?? []).length === 1, `one outline with one hole (${JSON.stringify(polys.map((p) => (p.holes ?? []).length))})`);
      const hole = polys[0]?.holes?.[0] ?? [];
      const xs = hole.map((p) => p[0]), ys = hole.map((p) => p[1]);
      t(Math.min(...xs) === CUT.x0 && Math.max(...xs) === CUT.x1 && Math.min(...ys) === CUT.y0 && Math.max(...ys) === CUT.y1, `the hole is the rectangle (${JSON.stringify(hole)})`);
      t((s.routing?.tracks ?? []).length === 0, `no tracks yet (${(s.routing?.tracks ?? []).length})`);
    });

    await scenario("Route autoroutes every connection round the cutout, the edge clearance kept from its rim", async (t) => {
      const button = [...document.querySelectorAll("button")].find((b) => b.title === "Route: autoroute every unrouted connection");
      if (!t(!!button, "the Route button is on the board editor's toolbar")) return;
      button.click();
      let s = await state();
      const t0 = Date.now();
      await sleep(300);
      while (Date.now() - t0 < 60000) {
        s = await state();
        if (s.job && s.job !== "running") break;
        await sleep(300);
      }
      t(/^route: \d+ track\(s\)/.test(s.job ?? "") && !/problem/.test(s.job ?? ""), `the route finished with nothing left (${s.job})`);
      const tracks = s.routing?.tracks ?? [];
      const vias = s.routing?.vias ?? [];
      t(tracks.length > 0, `${tracks.length} tracks, ${vias.length} vias`);
      let least = Infinity, worst = null;
      for (const tr of tracks) {
        for (let i = 1; i < tr.pts.length; i++) {
          const d = toCut(tr.pts[i - 1], tr.pts[i], tr.width / 2);
          if (d < least) { least = d; worst = `${tr.net} ${tr.layer} ${JSON.stringify([tr.pts[i - 1], tr.pts[i]])}`; }
        }
      }
      for (const v of vias) {
        const d = inside([v.x, v.y]) ? 0 : toCut([v.x, v.y], [v.x, v.y], v.d / 2);
        if (d < least) { least = d; worst = `via ${v.net} (${v.x}, ${v.y})`; }
      }
      t(least >= EDGE - 1000, `no copper crosses the cutout or comes within the edge clearance of it: the nearest is ${(least / 1e6).toFixed(3)} mm (${worst})`);
      // SDA and SCL ran straight through the cutout's place; they still connect, round it.
      for (const net of ["/SDA", "/SCL"]) t(tracks.some((tr) => tr.net === net), `${net} is routed`);
    });

    await scenario("kicad-cli's DRC of the routed board: no copper against the edge, no clearance, short or crossing, nothing unconnected", async (t) => {
      const r = await json("/api/drc");
      const types = (r.violations ?? []).map((v) => v.type);
      const routerTypes = types.filter((x) => ["copper_edge_clearance", "clearance", "shorting_items", "tracks_crossing", "hole_clearance", "track_width"].includes(x));
      t(!r.error, `the DRC ran (${r.engine ?? r.error})`);
      t(routerTypes.length === 0, `none of the router's violations (${JSON.stringify(types)})`);
      t((r.unconnected_items ?? []).length === 0, `nothing unconnected (${(r.unconnected_items ?? []).length})`);
    });

    await scenario("Undo takes the whole routing back in one step", async (t) => {
      await E.run("common.Interactive.undo");
      await sleep(800);
      const s = await state();
      t((s.routing?.tracks ?? []).length === 0 && (s.routing?.vias ?? []).length === 0, `no tracks or vias left (${(s.routing?.tracks ?? []).length}, ${(s.routing?.vias ?? []).length})`);
      t(((s.outline_polys ?? [])[0]?.holes ?? []).length === 1, "the cutout stays");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
})();
