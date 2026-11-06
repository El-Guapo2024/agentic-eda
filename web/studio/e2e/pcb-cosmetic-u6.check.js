// The PCB editor's cosmetic differences of UI-LEFT P28 (task-u6-last), as they look in the page:
//   * High Contrast Mode's Dim mixes an inactive item's colour into the background (`color.Mix( background, m_hiContrastFactor )`, 0.2 of it kept), opaque
//     where it is; Hide draws it not; the active copper layer is drawn over the other copper (`SetTopLayer`).
//   * The net highlight brightens the net and darkens everything else, the silkscreen too (`GetColor`'s single-net highlight branch).
//   * The ratsnest: the curve is `DrawCurve( a, c, c, b )` with c 0.1 of the length off the middle; a highlighted net's lines brighten, the others' alpha halves
//     (`RATSNEST_VIEW_ITEM::ViewDraw`'s `adjustColor`, `dimStatic`), one width.
//   * A rule area is drawn as any zone, in its layer's colour: a one-pixel outline and its border hatch, no label (`PCB_PAINTER::draw( const ZONE* )`).
//   * The zone fill's debug views are not in the View menu nor on the toolbar (`ADVANCED_CFG::m_ExtraZoneDisplayModes`).
//   * R, F and E on the text Draw Text carries turn it, flip it and open its Text Properties; the click then places it as they left it (`PlaceText`, r6 A6-6).
// Run it in the page of a served COPY of a board with a few footprints placed (it edits and undoes); it answers through `window.__pcbCosmeticCheck`:
// { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-few minutes=10
(async () => {
  const results = [];
  const check = (window.__pcbCosmeticCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__pcbCosmeticSched) {
    // One scheduler for every timer, driven by a fetch loop: a hidden page throttles timers.
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
    window.__pcbCosmeticSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const snapshot = async () => {
    const st = await state();
    const r = st.routing ?? {};
    return JSON.stringify({ tracks: (r.tracks ?? []).map((t) => t.id), zones: (r.zones ?? []).map((z) => z.id), shapes: (st.drawings?.shapes ?? []).map((s) => s.id), texts: (st.drawings?.texts ?? []).map((s) => s.id) });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const op = (o) => __eda.run("studio.Appearance.op", o);

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => canvasBox().querySelector("canvas");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 9, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  let toScreen = null;
  const view = () => __eda.state().view;
  const screenOf = () => { const v = view(); return v && v.scale > 0 ? (xmm, ymm) => [xmm * 1e6 * v.scale + v.x, ymm * 1e6 * v.scale + v.y] : null; };
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(500); };
  const bg = [0x00, 0x10, 0x23];

  /** The pixels of a `r`-pixel square round the board point (x, y) mm, as [r, g, b] each. */
  const pixels = (xmm, ymm, r = 2) => {
    const c = canvas();
    const k = c.width / c.getBoundingClientRect().width;
    const [sx, sy] = toScreen(xmm, ymm);
    const x0 = Math.round(sx * k) - r, y0 = Math.round(sy * k) - r;
    const d = c.getContext("2d").getImageData(x0, y0, 2 * r + 1, 2 * r + 1).data;
    const out = [];
    for (let i = 0; i < d.length; i += 4) out.push([d[i], d[i + 1], d[i + 2]]);
    return out;
  };
  /** The colour most of the square has (the grid's dots and antialiasing left out). */
  const colourAt = (xmm, ymm) => {
    const n = new Map();
    for (const p of pixels(xmm, ymm)) { const k = p.join(","); n.set(k, (n.get(k) ?? 0) + 1); }
    return [...n.entries()].sort((a, b) => b[1] - a[1])[0][0].split(",").map(Number);
  };
  /** How far from the background the inkiest pixel of a small square is. */
  const inkAt = (xmm, ymm, r = 1) => Math.max(...pixels(xmm, ymm, r).map((p) => Math.abs(p[0] - bg[0]) + Math.abs(p[1] - bg[1]) + Math.abs(p[2] - bg[2])));
  const inkiest = (xmm, ymm, r = 1) => pixels(xmm, ymm, r).sort((a, b) => (Math.abs(b[0] - bg[0]) + Math.abs(b[1] - bg[1]) + Math.abs(b[2] - bg[2])) - (Math.abs(a[0] - bg[0]) + Math.abs(a[1] - bg[1]) + Math.abs(a[2] - bg[2])))[0];
  const near = (a, b, tol) => a.every((v, i) => Math.abs(v - b[i]) <= tol);
  const repaint = async () => { await sleep(350); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { for (let i = 0; i < 2 && __eda.state().tool !== "select"; i++) await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  const base = await snapshot();
  const undoUntilBase = async () => { for (let i = 0; i < 14 && (await snapshot()) !== base; i++) await undo(); };
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const st0 = await state();
  const nets = [...new Set(st0.parts.flatMap((p) => (p.pads ?? []).map((q) => q.net)).filter(Boolean))];
  const netA = nets.find((n) => n === "VDD") ?? nets[0];
  const netB = nets.find((n) => n === "GND") ?? nets.find((n) => n !== netA);
  // the items the scenarios look at, in an empty corner of the board: a 1 mm F.Cu track down x = 28, a 1 mm B.Cu track along y = 4 crossing it,
  // a 0.6 mm silkscreen line along y = 7.5, a rule area on F.Cu from (24, 24) to (31, 31)
  await post({ op: "add_track", net: netA, layer: "F.Cu", width: 1_000_000, pts: [mm(28, 1.5), mm(28, 9)] });
  await post({ op: "add_track", net: netB, layer: "B.Cu", width: 1_000_000, pts: [mm(24.5, 4), mm(32, 4)] });
  await post({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 600_000, filled: false, start: mm(24.5, 7.5), end: mm(26.5, 7.5) } });
  await post({ op: "add_zone", net: "", layer: "F.Cu", outline: [mm(24, 24), mm(31, 24), mm(31, 31), mm(24, 31)] });
  await sleep(800);
  const st1 = await state();
  const fTrack = st1.routing.tracks.find((t) => t.layer === "F.Cu" && t.width === 1_000_000);
  const bTrack = st1.routing.tracks.find((t) => t.layer === "B.Cu" && t.width === 1_000_000);
  const zone = st1.routing.zones.find((z) => z.outline.some(([x, y]) => x === 24_000_000 && y === 24_000_000));
  if (zone) {
    const s = zone;
    await post({ op: "edit_zone", id: s.id, net: s.net, layer: s.layer, clearance: s.clearance, min_thickness: s.min_thickness, thermal_gap: s.thermal_gap, thermal_spoke_width: s.thermal_spoke_width, pad_connection: s.pad_connection, priority: s.priority, island_removal_mode: s.island_removal_mode, min_island_area: s.min_island_area, fill_mode: s.fill_mode, hatch_thickness: s.hatch_thickness, hatch_gap: s.hatch_gap, hatch_orientation_mdeg: s.hatch_orientation_mdeg, hatch_smoothing_level: s.hatch_smoothing_level, hatch_smoothing_value: s.hatch_smoothing_value, hatch_hole_min_area: s.hatch_hole_min_area, hatch_border_algorithm: s.hatch_border_algorithm, smoothing: s.smoothing, corner_radius: s.corner_radius, is_rule_area: true, keepout_tracks: true, keepout_vias: true, keepout_pads: true, keepout_copper_pour: false, keepout_footprints: false });
  }
  await sleep(600);
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(500);
  toScreen = screenOf();
  if (!toScreen || !fTrack || !bTrack) {
    results.push({ name: "setup", ok: false, notes: [`FAIL: no view or test tracks (${!!toScreen} ${!!fTrack} ${!!bTrack})`] });
  } else {
    const fAt = [28, 7.0]; // the F.Cu track away from the crossing and the silkscreen
    const cross = [28, 4];
    const silkAt = [25.5, 7.5];

    await scenario("High Contrast Dim mixes an inactive track into the background, opaque; the active copper layer is drawn on top; Hide draws it not", async (t) => {
      await __eda.run("pcbnew.Control.layerTop"); await op({ op: "contrast", mode: "normal" }); await repaint();
      const normal = colourAt(...fAt);
      // 0.92 of KiCad Default's F.Cu (200, 52, 52) over the background
      t(near(normal, [184, 49, 51], 6), `F.Cu active, Normal: the track is F.Cu's colour (${normal})`);
      const crossFront = colourAt(...cross);
      t(crossFront[0] > 150, `F.Cu active: the F.Cu track is on top at the crossing (${crossFront})`);
      await __eda.run("pcbnew.Control.layerBottom"); await repaint();
      const crossBack = colourAt(...cross);
      t(crossBack[2] > 150 && crossBack[0] < 110, `B.Cu active, Normal: the B.Cu track is drawn over the F.Cu one (SetTopLayer) (${crossBack})`);
      await op({ op: "contrast", mode: "dimmed" }); await repaint();
      const dim = colourAt(...fAt);
      // Mix( bg, 0.2 ) of F.Cu = (40, 23, 38), at the track's 0.92 over the background: (37, 23, 38); a quarter alpha would give (46, 24, 39)
      t(near(dim, [37, 23, 38], 3), `B.Cu active, Dim: the F.Cu track is F.Cu mixed 80 % into the background (${dim})`);
      const crossDim = colourAt(...cross);
      t(crossDim[2] > 150 && crossDim[0] < 90, `Dim: the active B.Cu track covers the dimmed one (${crossDim})`);
      await op({ op: "contrast", mode: "hidden" }); await repaint();
      const hidden = colourAt(...fAt);
      t(near(hidden, bg, 6), `Hide: the F.Cu track is not drawn (${hidden})`);
      await op({ op: "contrast", mode: "normal" }); await __eda.run("pcbnew.Control.layerTop"); await repaint();
    });

    await scenario("The net highlight brightens the net and darkens everything else, the silkscreen too", async (t) => {
      const silk0 = colourAt(...silkAt);
      t(near(silk0, [242, 237, 161], 8), `the silkscreen line before (${silk0})`);
      await select([fTrack.id]);
      await __eda.run("pcbnew.EditorControl.highlightNetSelection");
      await select([]);
      await repaint();
      t(JSON.stringify(__eda.state().highlight) === JSON.stringify([netA]), `${netA} is highlighted (${JSON.stringify(__eda.state().highlight)})`);
      const lit = colourAt(...fAt);
      // Brightened( 0.5 ) of F.Cu = (228, 154, 154), at 0.92: (209, 142, 144)
      t(near(lit, [209, 142, 144], 6), `the highlighted track is F.Cu brightened by 0.5 (${lit})`);
      const silk = colourAt(...silkAt);
      t(near(silk, [121, 119, 81], 6), `the silkscreen line is darkened by 0.5 (${silk})`);
      const other = colourAt(26, 4);
      // Darkened( 0.5 ) of B.Cu (77, 127, 196) = (39, 64, 98), at 0.92: (35, 60, 93)
      t(near(other, [35, 60, 93], 6), `the track of another net is darkened by 0.5 (${other})`);
      await __eda.run("pcbnew.EditorControl.clearHighlight");
      await repaint();
      t(near(colourAt(...silkAt), silk0, 6), "cleared: the silkscreen line is as it was");
    });

    await scenario("The ratsnest: KiCad's curve; a highlighted net's lines brighten, the others' fade", async (t) => {
      const edges = (await json("/api/ratsnest")).edges ?? [];
      // the longest line between two pads (the test tracks have lines of their own), so its middle is clear of them
      const padToPad = edges.filter((x) => /^[A-Z]+\d+\./.test(x.from_id ?? "") && /^[A-Z]+\d+\./.test(x.to_id ?? ""));
      const e = [...padToPad].sort((a, b) => Math.hypot(b.to[0] - b.from[0], b.to[1] - b.from[1]) - Math.hypot(a.to[0] - a.from[0], a.to[1] - a.from[1]))[0];
      if (!e) return t(false, "the board has a ratsnest line");
      const a = [e.from[0] / 1e6, e.from[1] / 1e6], b = [e.to[0] / 1e6, e.to[1] / 1e6];
      const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
      const before = inkAt(...mid);
      t(before > 30, `a line is drawn through the middle (${before})`);
      const otherNet = nets.find((n) => n !== e.net);
      const other = st1.parts.flatMap((p) => (p.pads ?? []).map((q) => ({ p, q }))).find(({ q }) => q.net === otherNet);
      await select([`${other.p.ref}.${other.q.num}`]);
      await __eda.run("pcbnew.EditorControl.highlightNetSelection");
      await select([]);
      await repaint();
      const faded = inkAt(...mid);
      t(faded < before * 0.8, `another net highlighted: the line's alpha halves (${before} -> ${faded})`);
      await __eda.run("pcbnew.EditorControl.clearHighlight");
      const pad = st1.parts.flatMap((p) => (p.pads ?? []).map((q) => ({ p, q }))).find(({ q }) => q.net === e.net);
      await select([`${pad.p.ref}.${pad.q.num}`]);
      await __eda.run("pcbnew.EditorControl.highlightNetSelection");
      await select([]);
      await repaint();
      const brighter = inkAt(...mid);
      t(brighter > before, `its own net highlighted: the line brightens (${before} -> ${brighter})`);
      await __eda.run("pcbnew.EditorControl.clearHighlight");
      await __eda.run("pcbnew.Control.ratsnestLineMode"); await repaint();
      // DrawCurve( a, c, c, b ), c = middle + 0.1 (-dy, dx): the curve's middle is 0.075 of the length off the line's
      const dx = b[0] - a[0], dy = b[1] - a[1];
      const bow = [mid[0] - 0.075 * dy, mid[1] + 0.075 * dx];
      const old = [mid[0] - 0.03 * dy, mid[1] + 0.03 * dx];
      const onCurve = inkAt(...bow), onOld = inkAt(...old, 0), onLine = inkAt(...mid, 0);
      t(onCurve > 30 && onLine < 20 && onOld < 20, `curved: the line bows 0.075 of its length (${onCurve} at the bow, ${onOld} at the studio's old 0.03, ${onLine} on the chord)`);
      await __eda.run("pcbnew.Control.ratsnestLineMode"); await repaint();
    });

    await scenario("A rule area is drawn as any zone: its layer's colour, a one-pixel outline and its hatch, no label", async (t) => {
      const z = (await state()).routing.zones.find((x) => x.id === zone?.id);
      t(!!z?.is_rule_area, "the area is a rule area");
      const edge = inkiest(24, 27.5);
      t(edge[0] > 90 && edge[1] < 0.45 * edge[0], `its outline is F.Cu's red, not the studio's orange (${edge})`);
      const centre = colourAt(27.5, 27.5);
      t(near(centre, bg, 8), `nothing at its centre: no label of its keepouts (${centre})`);
      let hatch = 0;
      for (let y = 25; y < 30; y += 0.05) hatch = Math.max(hatch, inkAt(24.25, y, 0));
      t(hatch > 60, `the border hatch is drawn inside the edge (${hatch})`);
    });

    await scenario("The zone fill's debug views are not in the View menu nor on the toolbar (ExtraZoneDisplayModes)", async (t) => {
      const viewMenu = [...document.querySelectorAll(".menubar-item")].find((m) => m.firstChild?.textContent?.trim() === "View");
      viewMenu?.click(); await sleep(250);
      const texts = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].map((n) => n.textContent);
      t(texts.some((x) => x.includes("Draw Zone Fills")), `View has Draw Zone Fills (${texts.length} entries)`);
      t(!texts.some((x) => x.includes("Fracture Borders") || x.includes("Triangulation")), "no Draw Zone Fill Fracture Borders or Triangulation");
      viewMenu?.click(); await sleep(150);
      const titled = [...document.querySelectorAll("[title]")].map((n) => n.getAttribute("title"));
      t(titled.some((x) => x.startsWith("Draw Zone Fills")) && !titled.some((x) => x.startsWith("Draw Zone Fill Fracture") || x.startsWith("Draw Zone Fill Triangulation")), "the toolbar has Draw Zone Fills and not the two debug views");
    });

    await scenario("R, F and E on the text Draw Text carries: it turns, flips, opens its Text Properties, and the click places it so", async (t) => {
      const before = ((await state()).drawings?.texts ?? []).map((x) => x.id);
      await __eda.run("pcbnew.Control.layerTop");
      await __eda.run("pcbnew.InteractiveDrawing.text"); await sleep(400);
      await click(29, 14);
      const input = document.querySelector(".dialog .kv-grid input");
      t(!!input, "the first click opens the dialog");
      if (input) setValue(input, "U6 carried");
      const layerSel = document.querySelector(".dialog .kv-grid select");
      if (layerSel) setValue(layerSel, "F.SilkS");
      await sleep(150);
      buttonText(document.querySelector(".dialog") ?? document, "Add")?.click(); await sleep(500);
      await hover(29.5, 14.5);
      await __eda.run("pcbnew.InteractiveEdit.rotateCcw"); await sleep(200);
      const props = async () => {
        await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(400);
        const d = document.querySelector("[data-testid=text-dialog]");
        const vals = d ? { content: d.querySelector("[data-testid=text-content]")?.value, layer: d.querySelector("select")?.value, angle: d.querySelector("input[type=number][step='90']")?.value } : null;
        buttonText(d ?? document, "Cancel")?.click(); await sleep(250);
        return vals;
      };
      const afterR = await props();
      t(afterR?.content === "U6 carried" && afterR?.angle === "90", `E opens Text Properties of the carried text, turned 90 by R (${JSON.stringify(afterR)})`);
      t(__eda.state().tool === "text", `Cancel leaves it carried, the tool armed (${__eda.state().tool})`);
      await __eda.run("pcbnew.InteractiveEdit.flip"); await sleep(200);
      const afterF = await props();
      t(afterF?.layer === "B.SilkS" && afterF?.angle === "270", `F flips it to B.SilkS and negates its angle (-90, which is 270) (${JSON.stringify(afterF)})`);
      await click(30, 15);
      await sleep(400);
      const made = ((await state()).drawings?.texts ?? []).filter((x) => !before.includes(x.id));
      t(made.length === 1 && made[0].content === "U6 carried" && made[0].layer === "B.SilkS" && made[0].mirror === true && ((made[0].angle % 360000) + 360000) % 360000 === 270000, `the click places it turned, flipped and mirrored (${JSON.stringify(made.map((x) => [x.content, x.layer, x.mirror, x.angle]))})`);
      await escape(); await escape();
    });
  }

  await undoUntilBase();
  results.push({ name: "the board is as it was", ok: (await snapshot()) === base, notes: [] });
  check.ok = results.every((r) => r.ok);
  check.done = true;
  return results;
})();
