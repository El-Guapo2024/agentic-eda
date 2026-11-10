// Swap (Alt+S) of every kind of item (UI-LEFT-2 PP15; `EDIT_TOOL::Swap`, crates/ops/src/pcb_swap.rs, kicad-port/pcbSwap.ts):
//   * an arc track and a via: the arc's start goes to the via (`PCB_ARC::SetPosition`, its middle and end stay), the via to the arc's centre (`GetPosition`);
//   * two aligned dimensions: only their starts trade places (`SetStart`), and their layers swap (two items of one type);
//   * a text box and a point: their places;
//   * a group and a text: the group moves by the difference to the text, the text to the centre of the group's box (`PCB_GROUP::GetPosition`);
//   * the Footprint Editor: Alt+S on two pads trades the places of their copper (`PAD::SwapShapePositions`), a pad with an offset included.
// Each swap is one undo step. Run it on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`).
// It answers through `window.__pcbSwapCheck`: { results, done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=5
(async () => {
  const results = [];
  const check = (window.__pcbSwapCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results };
  };
  const E = window.__eda;
  const state = () => json("/api/state");
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const until = async (cond, ms = 10000, step = 150) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  /** A command sent behind the studio's back is in its state after its next refresh: wait for the design revision to move. */
  const post = async (c) => { const rev0 = E.state().revision; const r = await cmd(c); await until(() => E.state().revision !== rev0, 8000); await sleep(400); return r; };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const swap = async () => { const rev0 = E.state().revision; await E.run("pcbnew.InteractiveEdit.swap"); await until(() => E.state().revision !== rev0, 6000); await sleep(500); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    results.push(entry);
  };
  const mm = (x, y) => ({ x: Math.round(x * 1e6), y: Math.round(y * 1e6) });
  const same = (a, b) => !!a && !!b && a[0] === b[0] && a[1] === b[1];
  const roundAway = (v) => (v < 0 ? -Math.round(-v) : Math.round(v));
  const tessellate = (start, mid, end, n = 32) => {
    const [sx, sy] = start, [mx, my] = mid, [ex, ey] = end;
    const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
    const ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
    const uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
    const r = Math.hypot(sx - ux, sy - uy), two = Math.PI * 2, norm = (a) => ((a % two) + two) % two;
    const a0 = Math.atan2(sy - uy, sx - ux), a1 = Math.atan2(my - uy, mx - ux), a2 = Math.atan2(ey - uy, ex - ux);
    let sweep = norm(a2 - a0);
    if (norm(a1 - a0) > sweep) sweep -= two;
    const pts = [];
    for (let i = 0; i <= n; i++) { const a = a0 + sweep * (i / n); pts.push([roundAway(ux + r * Math.cos(a)), roundAway(uy + r * Math.sin(a))]); }
    pts[0] = [sx, sy];
    pts[n] = [ex, ey];
    return pts;
  };

  // --------------------------------------------------------------------------------------------------------------------------------- run
  await until(() => E.state().tab === "pcb", 10000);
  const st0 = await state();
  const outline = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const [bx0, by0] = outline.length ? [Math.min(...outline.map((p) => p[0])), Math.max(...outline.map((p) => p[1]))] : [0, 0];
  // a free spot below the board's outline, on whole millimetres
  const [fx, fy] = [Math.round(bx0) + 5, Math.round(by0) + 10];
  const net = (st0.routing?.tracks ?? [])[0]?.net ?? (st0.nets ?? [])[0]?.name ?? "GND";

  await scenario("an arc track and a via: the arc's start goes to the via, the via to the arc's centre; one undo step", async (t) => {
    const s = [fx * 1e6, fy * 1e6], m = [(fx + 2) * 1e6, (fy - 2) * 1e6], e = [(fx + 4) * 1e6, fy * 1e6]; // centre (fx + 2, fy)
    const arc = { net, layer: "F.Cu", width: 200_000, pts: tessellate(s, m, e).map((p) => ({ x: p[0], y: p[1] })), arc_mid_offset: { x: m[0] - s[0], y: m[1] - s[1] } };
    const r = await post({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [arc], vias: [{ net, at: mm(fx + 10, fy + 3), drill: 300_000, diameter: 600_000, from_layer: "F.Cu", to_layer: "B.Cu" }] });
    if (!t(r.ok !== false, `an arc and a via (${r.message ?? "ok"})`)) return;
    const st = await state();
    const a = st.routing.tracks.find((x) => same(x.pts[0], s) && x.arc_mid);
    const v = st.routing.vias.find((x) => x.x === mm(fx + 10, 0).x && x.y === mm(0, fy + 3).y);
    if (!t(!!a && !!v, `found them (${a?.id}, ${v?.id})`)) return;
    await select([a.id, v.id]);
    await swap();
    const st1 = await state();
    const a1 = st1.routing.tracks.find((x) => x.id === a.id);
    const v1 = st1.routing.vias.find((x) => x.id === v.id);
    t(!!a1 && same(a1.pts[0], [v.x, v.y]), `the arc starts at the via's place (${JSON.stringify(a1?.pts[0])})`);
    t(!!a1 && same(a1.pts[a1.pts.length - 1], e) && same(a1.arc_mid, m), `its middle and end stayed (${JSON.stringify([a1?.arc_mid, a1?.pts.at(-1)])})`);
    t(!!v1 && Math.abs(v1.x - (fx + 2) * 1e6) <= 1 && Math.abs(v1.y - fy * 1e6) <= 1, `the via is on the arc's centre (${v1 && [v1.x, v1.y]})`);
    await undo();
    const st2 = await state();
    t(same(st2.routing.tracks.find((x) => x.id === a.id)?.pts[0], s) && st2.routing.vias.find((x) => x.id === v.id)?.x === v.x, "one undo step took it back");
  });

  await scenario("two dimensions trade their starts and layers; a text box and a point their places", async (t) => {
    const dim = (layer, x, y) => ({ layer, kind: { kind: "aligned", height: 1_000_000 }, start: mm(x, y), end: mm(x + 6, y), prefix: "", suffix: "", units: "mm", units_format: "no_suffix", precision: 2, suppress_trailing_zeros: true, text_position: "outside", keep_text_aligned: true, text_angle: 0, text_size_nm: 1_000_000, stroke_width: 150_000, arrow_length: 1_270_000, extension_offset: 500_000, extension_height: 586_000, arrow_direction: "outward" });
    const before = new Set(((await state()).drawings?.dimensions ?? []).map((d) => d.id));
    await post({ op: "add_dimension", dimension: dim("Dwgs.User", fx, fy + 10) });
    await post({ op: "add_dimension", dimension: dim("Cmts.User", fx + 20, fy + 15) });
    const made = ((await state()).drawings?.dimensions ?? []).filter((d) => !before.has(d.id));
    const da = made.find((d) => d.layer === "Dwgs.User"), db = made.find((d) => d.layer === "Cmts.User");
    if (!t(!!da && !!db, "two dimensions")) return;
    await select([da.id, db.id]);
    await swap();
    const ds = (await state()).drawings.dimensions;
    const na = ds.find((d) => d.id === da.id), nb = ds.find((d) => d.id === db.id);
    t(same(na?.start, db.start) && same(na?.end, da.end), `the first's start moved to the second's, its end stayed (${JSON.stringify([na?.start, na?.end])})`);
    t(same(nb?.start, da.start) && same(nb?.end, db.end), `the second's start moved to the first's (${JSON.stringify([nb?.start, nb?.end])})`);
    t(na?.layer === "Cmts.User" && nb?.layer === "Dwgs.User", `their layers swapped (${na?.layer}, ${nb?.layer})`);
    await undo();
    const back = (await state()).drawings.dimensions.find((d) => d.id === da.id);
    t(same(back?.start, da.start) && back?.layer === "Dwgs.User", "one undo step took it back");

    await post({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "SWAP", start: mm(fx, fy + 25), end: mm(fx + 8, fy + 28), margin_left_nm: 300000, margin_top_nm: 300000, margin_right_nm: 300000, margin_bottom_nm: 300000, stroke_width_nm: 120000, size_nm: 1000000 } });
    await post({ op: "pcb_item", verb: "add_point", point: { layer: "F.Fab", at: mm(fx + 20, fy + 30), size_nm: 1000000 } });
    const dr = (await state()).drawings;
    const tb = (dr.text_boxes ?? []).find((x) => same(x.start, [mm(fx, 0).x, mm(0, fy + 25).y]));
    const pt = (dr.points ?? []).find((x) => x.x === mm(fx + 20, 0).x && x.y === mm(0, fy + 30).y);
    if (!t(!!tb && !!pt, "a text box and a point")) return;
    await select([tb.id, pt.id]);
    await swap();
    const dr1 = (await state()).drawings;
    const tb1 = dr1.text_boxes.find((x) => x.id === tb.id), pt1 = dr1.points.find((x) => x.id === pt.id);
    t(same(tb1?.start, [pt.x, pt.y]) && same(tb1?.end, [pt.x + 8_000_000, pt.y + 3_000_000]), `the text box is at the point's place (${JSON.stringify(tb1?.start)})`);
    t(pt1?.x === tb.start[0] && pt1?.y === tb.start[1], `the point at the text box's first corner (${pt1 && [pt1.x, pt1.y]})`);
    t(tb1?.layer === "F.SilkS" && pt1?.layer === "F.Fab", "two types: their layers stay");
  });

  await scenario("a group and a text: the group moves to the text, the text to the centre of the group's box", async (t) => {
    const seg = (y) => ({ kind: "segment", layer: "F.SilkS", stroke_width: 0, filled: false, start: mm(fx, y), end: mm(fx + 10, y) });
    const before = new Set(((await state()).drawings?.shapes ?? []).map((s) => s.id));
    await post({ op: "add_shape", shape: seg(fy + 40) });
    await post({ op: "add_shape", shape: seg(fy + 44) });
    await post({ op: "add_text", text: { content: "SWAPPED", at: mm(fx + 30, fy + 50), angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false } });
    let dr = (await state()).drawings;
    const segs = dr.shapes.filter((s) => !before.has(s.id)).map((s) => s.id);
    const txt = dr.texts.find((x) => x.content === "SWAPPED");
    if (!t(segs.length === 2 && !!txt, "two lines and a text")) return;
    const gBefore = new Set((dr.groups ?? []).map((g) => g.id));
    await post({ op: "group", ids: segs });
    dr = (await state()).drawings;
    const g = (dr.groups ?? []).find((x) => !gBefore.has(x.id));
    if (!t(!!g, "the lines grouped")) return;
    await select([g.id, txt.id]);
    await swap();
    dr = (await state()).drawings;
    const s0 = dr.shapes.find((s) => s.id === segs[0]);
    const t1 = dr.texts.find((x) => x.id === txt.id);
    // the box of the two lines (0 wide strokes) is (fx, fy+40)-(fx+10, fy+44), its centre (fx+5, fy+42)
    t(!!t1 && Math.abs(t1.x - mm(fx + 5, 0).x) <= 1 && Math.abs(t1.y - mm(0, fy + 42).y) <= 1, `the text is on the group's centre (${t1 && [t1.x, t1.y]})`);
    const s0start = s0?.start ? (Array.isArray(s0.start) ? s0.start : [s0.start.x, s0.start.y]) : null;
    t(!!s0start && Math.abs(s0start[0] - mm(fx + 25, 0).x) <= 1 && Math.abs(s0start[1] - mm(0, fy + 48).y) <= 1, `the group moved by the text's place less its centre (${JSON.stringify(s0start)})`);
    await undo();
    const back = (await state()).drawings.texts.find((x) => x.id === txt.id);
    t(back?.x === txt.x && back?.y === txt.y, "one undo step took it back");
  });

  await scenario("the Footprint Editor: Alt+S on two pads trades the places of their copper", async (t) => {
    const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
    const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
    const before = await fpNames();
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Footprint")?.click();
    await until(() => E.state().tab === "footprint", 4000);
    await sleep(500);
    await E.run("pcbnew.ModuleEditor.newFootprint");
    const name = await until(async () => (await fpNames()).find((n) => !before.includes(n)), 6000);
    if (!t(!!name, `a new footprint (${name})`)) return;
    const src = `(footprint "Src" (version 20241229) (generator "pcbnew") (layer "F.Cu")
	(pad "1" smd rect (at -2 0) (size 1 0.6) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "e0000000-0000-4000-8000-000000000001"))
	(pad "2" smd rect (at 2 1 90) (size 1 0.6) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "e0000000-0000-4000-8000-000000000002") (offset 0.5 0))
)`;
    const parsed = await fetch("/api/footprint/parse", { method: "POST", body: src }).then((r) => r.json());
    const pads = parsed.footprint?.pads ?? [];
    if (!t(pads.length === 2, `the reader gives two pads (${parsed.error ?? pads.length})`)) return;
    for (const p of pads) { const r = await cmd({ op: "add_pad", footprint: name, pad: p }); t(r.ok !== false, `add_pad ${p.number}: ${r.message ?? "ok"}`); }
    const fp0 = await until(async () => { const f = await footprint(name); return (f.pads ?? []).length === 2 ? f : null; }, 6000);
    if (!t(!!fp0, "two pads in the footprint")) return;
    await sleep(600);
    const p1 = fp0.pads.find((p) => p.number === "1"), p2 = fp0.pads.find((p) => p.number === "2");
    await select([p1.id, p2.id]);
    // the hotkey, as a person presses it
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "s", code: "KeyS", altKey: true, bubbles: true, cancelable: true }));
    const fp1 = await until(async () => { const f = await footprint(name); return f.pads.find((p) => p.number === "1").at.x !== p1.at.x ? f : null; }, 6000);
    if (!t(!!fp1, "Alt+S swapped the pads")) return;
    // pad 2's copper is at its hole plus its offset turned with it (90 degrees); a pad with no offset has its copper at its hole
    const shape = (p) => { const a = (p.rot / 1000) * Math.PI / 180, o = p.offset ?? { x: 0, y: 0 }; return [p.at.x + o.x * Math.cos(a) - o.y * Math.sin(a), p.at.y + o.x * Math.sin(a) + o.y * Math.cos(a)]; };
    const n1 = fp1.pads.find((p) => p.number === "1"), n2 = fp1.pads.find((p) => p.number === "2");
    const near = (a, b) => Math.abs(a[0] - b[0]) <= 1 && Math.abs(a[1] - b[1]) <= 1;
    t(near(shape(n1), shape(p2)) && near(shape(n2), shape(p1)), `the copper places traded (${JSON.stringify([shape(n1), shape(p2), shape(n2), shape(p1)])})`);
    t(n2.rot === p2.rot && n1.number === "1", "the pads keep their numbers and turns");
    await undo();
    const fp2 = await footprint(name);
    t(fp2.pads.find((p) => p.number === "1").at.x === p1.at.x, "one undo step took it back");
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click();
    await sleep(400);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
