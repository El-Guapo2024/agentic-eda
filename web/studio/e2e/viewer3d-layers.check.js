// The 3D viewer's layers as KiCad's `BOARD_ADAPTER::createLayers` builds them and its OpenGL view culls and lights them (UI-LEFT-2 DD1, DD2, DD4; PARITY-3d section 10):
// open the 3D tab of a served board with tracks, vias and zones, and check through `window.__eda` (`state().viewer3d`, components/viewer3d/viewer3dProbe.ts):
//
//   1  every layer is culled as `OPENGL_RENDER_LIST::DrawCulled` culls it: the through holes `m_TH_ODs` (a via, a plated hole, a non-plated one each) and the board's outside
//   2  a track with "Soldermask" set (`set_solder_mask`) opens its side's mask; an untented via (`set_via_props` front_tenting false) opens it and loses its cover
//   3  a counterbored via cuts the body (the server's subtraction, `POST /api/board3d/polys`) and gets the plug of board material under it
//   4  the Render Options: "Clip silkscreen at solder mask edges" culls the silkscreen by the mask's openings, "Clip silkscreen at via annuli" by the annuli,
//      "Use bare copper color for unplated copper" draws the plated copper over the bare
//   5  a footprint moved off the board has its pads drawn with Off-board Silkscreen on (`m_offboardPadsFront`)
//   6  every material of the board is lit with OpenGL's equation and KiCad's lights (glLighting.ts), and no error is logged
//
// Run it on a COPY of a served board: it edits a track, a via and a footprint, and leaves the edits (the preferences it changes it puts back).
// Returns { ok, results: [{ name, ok, detail? }, ...] }.
// run-all-checks: board=mcu30-zones minutes=8
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const settle = async () => { await frame(); await sleep(60); await frame(); await sleep(60); };
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const view = () => __eda.state().viewer3d;
  const L = () => view().layers3d ?? {};
  const until = async (test, ms = 30000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await test()) return true; await sleep(150); } return false; };
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const json = (u) => fetch(u, { cache: "no-store" }).then((r) => r.json());
  const row = (id) => document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
  const box = (label) => [...document.querySelectorAll(".dialog input[type=checkbox]")].find((i) => i.getAttribute("aria-label") === label);
  const button = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const prefs = async (labels) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => document.querySelector('[data-page="3d_general"]'), 5000);
    document.querySelector('[data-page="3d_general"]').click();
    await sleep(300);
    for (const [label, on] of labels) { const b = box(label); if (b && b.checked !== on) { b.click(); await sleep(100); } }
    button(document.querySelector(".dialog"), "OK")?.click();
    await sleep(500);
  };
  const version = async () => (await json("/api/version")).version ?? (await json("/api/state")).version;

  try {
    await __eda.run("common.Control.show3DViewer");
    if (!(await until(() => view() && view().open, 20000))) throw new Error("the 3D tab did not open");
    await until(() => view().bodyFromServer && (view().scene["pad"] ?? 0) > 0, 60000);
    await settle();
    const board = await json("/api/state");
    const pads = board.parts.filter((p) => p.placed).flatMap((p) => p.pads ?? []);
    const holes = pads.filter((p) => p.drill || p.slot).length;
    const vias = board.routing?.vias ?? [];
    const through = vias.filter((v) => !v.type && !v.props?.capping).length;

    // 1 ----------------------------------------------------------------------------------------------------------------------------------- culling
    check("m_TH_ODs holds every through via's and drilled pad's hole", L().thODs === through + holes, { thODs: L().thODs, through, holes });
    check("the copper and the technical layers are culled through the stencil (the holes, the board's outside)", L().cullPieces > 0 && L().silkBoardCulled > 0, { pieces: L().cullPieces, silk: L().silkBoardCulled });
    const fills = (await json("/api/fill")).zones ?? [];
    check("tracks, vias and the filled zones are drawn as copper", (view().scene.track ?? 0) > 0 && (view().scene.via ?? 0) > 0 && (view().scene["zone-fill"] ?? 0) > 0 === fills.some((z) => z.filled), { scene: view().scene, filled: fills.filter((z) => z.filled).length });
    check("the vias tented by Board Setup are covered by the mask", (L().coversTop ?? 0) + (L().coversBottom ?? 0) > 0, { top: L().coversTop, bottom: L().coversBottom });

    // 2 -------------------------------------------------------------------------------------------------------------------- the mask's own openings
    const track = (board.routing?.tracks ?? []).find((t) => t.layer === "F.Cu" && !t.mask);
    const before = { tracks: L().maskTracksTop ?? 0, vias: L().maskViasTop ?? 0, covers: L().coversTop ?? 0 };
    if (track) {
      const r = await cmd({ op: "set_solder_mask", ids: [track.id], solder_mask: true, margin: null });
      await until(() => (L().maskTracksTop ?? 0) > before.tracks, 20000);
      check("a track with Soldermask set opens its side's mask (createTrackWithMargin on F.Mask)", (L().maskTracksTop ?? 0) === before.tracks + (track.pts.length - 1), { reply: r.ok, before: before.tracks, after: L().maskTracksTop });
    } else check("a track with Soldermask set opens its side's mask", false, "no F.Cu track on the board");
    const via = vias.find((v) => !v.type && v.props?.front_tenting === undefined);
    if (via) {
      await cmd({ op: "set_via_props", id: via.id, props: { ...(via.props ?? {}), front_tenting: false } });
      await until(() => (L().maskViasTop ?? 0) > before.vias, 20000);
      check("an untented via opens the front mask and has no cover there (PCB_VIA::IsOnLayer, generateViaCovers)", L().maskViasTop === before.vias + 1 && L().coversTop === before.covers - 1, { vias: `${before.vias} -> ${L().maskViasTop}`, covers: `${before.covers} -> ${L().coversTop}` });
    } else check("an untented via opens the front mask", false, "no through via");

    // 3 ------------------------------------------------------------------------------------------------------------------------------ post-machining
    const cb = vias.find((v) => !v.type && v.id !== via?.id);
    if (cb) {
      const holesBefore = view().bodyHoles;
      await cmd({ op: "set_via_props", id: cb.id, props: { ...(cb.props ?? {}), front_post_machining: { mode: "counterbore", size: Math.round(cb.drill * 2), depth: 300000, angle_ddeg: 0 } } });
      await until(() => (L().bodyCuts ?? 0) > 0 && L().jobsAnswered === L().jobs, 30000);
      await settle();
      check("a counterbored via cuts the body and leaves a plug under it (bgWorker, backfillPostMachine)", L().bodyCuts === 1 && L().plugs === 1 && (view().scene["post-machine-plug"] ?? 0) === 1, { cuts: L().bodyCuts, plugs: L().plugs, jobs: `${L().jobsAnswered}/${L().jobs}`, holes: `${holesBefore} -> ${view().bodyHoles}` });
    }

    // 4 -------------------------------------------------------------------------------------------------------------------------- the Render Options
    await prefs([["Clip silkscreen at solder mask edges", true], ["Clip silkscreen at via annuli", true], ["Use bare copper color for unplated copper (slow)", true], ["Use bare copper color for unplated copper", true]]);
    await __eda.run("common.Control.show3DViewer");
    await until(() => (L().silkMaskCulled ?? 0) > 0, 15000);
    await settle();
    check("Clip silkscreen at solder mask edges culls the silkscreen by the mask's openings", L().silkMaskCulled > 0, L().silkMaskCulled);
    check("Clip silkscreen at via annuli culls it by the via annuli instead of the holes", L().silkAnnuliCulled > 0 && L().viaAnnuli >= through, { annuli: L().viaAnnuli, culled: L().silkAnnuliCulled });
    check("Use bare copper color draws the plated copper over the bare (m_platedPadsFront)", (view().scene["plated-copper"] ?? 0) > 0, view().scene["plated-copper"]);
    await prefs([["Clip silkscreen at solder mask edges", false], ["Clip silkscreen at via annuli", false], ["Use bare copper color for unplated copper (slow)", false], ["Use bare copper color for unplated copper", false]]);
    await __eda.run("common.Control.show3DViewer");
    await until(() => (L().silkMaskCulled ?? 1) === 0, 15000);
    check("off again, KiCad's defaults", L().silkMaskCulled === 0 && L().silkAnnuliCulled === 0 && (view().scene["plated-copper"] ?? 0) === 0, L());

    // 5 ------------------------------------------------------------------------------------------------------------------------------ off-board pads
    const part = board.parts.find((p) => p.placed && p.side === "top" && (p.pads ?? []).length > 0);
    if (part) {
      const xs = (board.outline_polys ?? []).flatMap((p) => p.outer.map((q) => q[0]));
      const right = xs.length ? Math.max(...xs) : 100_000_000;
      const left = Math.min(...part.pads.map((q) => q.x - q.w / 2));
      const moved = await cmd({ op: "move_items", ids: [part.ref], dx: right + 30_000_000 - left, dy: 0 });
      await sleep(800);
      if (!row("off_board_silk")) document.querySelector(".dock-column.right .dock-handle")?.click();
      await sleep(300);
      const was = row("off_board_silk")?.checked;
      if (!was) row("off_board_silk")?.click();
      await until(() => (L().offboard ?? 0) > 0, 15000);
      check("with Off-board Silkscreen the pads of a footprint off the board are drawn (m_offboardPadsFront)", (L().offboard ?? 0) > 0 && (view().scene["offboard-pad"] ?? 0) > 0, { part: part.ref, moved: JSON.stringify(moved).slice(0, 200), offboard: L().offboard, meshes: view().scene["offboard-pad"], row: row("off_board_silk")?.checked });
      if (!was) row("off_board_silk")?.click();
      await settle();
    }

    // 6 ------------------------------------------------------------------------------------------------------------------------------------ lighting
    check("every material of the board is lit as KiCad's OpenGL view lights it", (L().unlit ?? 1) === 0 && (L().lit ?? 0) > 0, { lit: L().lit, unlit: L().unlit });
    check("no error was logged", __eda.errors().length === 0, __eda.errors().slice(0, 3));
    void version;
  } catch (e) {
    check("the check ran to its end", false, String(e && e.stack ? e.stack : e));
  }
  return { ok: results.every((r) => r.ok), results };
})();
