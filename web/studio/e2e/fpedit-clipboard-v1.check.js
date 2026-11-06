// The Footprint Editor's Copy, Cut and Paste of items and its Duplicate of every kind (UI-LEFT-2 FF1, FF2; task-v1-handles; PARITY-fpedit section 23):
//   FF1  Copy of a text box and a dimension puts KiCad's clipboard footprint on the clipboard (`(footprint "clipboard:<uuid>" ..)`, CLIPBOARD_IO::SaveSelection); Paste
//        brings the copies riding the cursor until a click (one undo step, Escape leaves nothing); a paste of the text a `.kicad_mod` holds works too.
//   FF2  Duplicate of a dimension and a dashed line keeps the line style and the dimension's own style; the copies ride the cursor, one undo step with the drop.
//
// Run it on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes a footprint of its own.
// It answers through `window.__fpClipCheck`: { results, done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=5
(async () => {
  const results = [];
  const check = (window.__fpClipCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results };
  };
  const E = window.__eda;
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const until = async (cond, ms = 10000, step = 150) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    results.push(entry);
  };
  // the canvas, in the editor's nm (y down)
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container") ?? document.querySelector(".pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse" })); };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(200); };
  const clickAt = async (wx, wy) => { await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(900); };
  // the system clipboard is the page's own here
  let clip = "";
  try {
    Object.defineProperty(navigator.clipboard, "writeText", { configurable: true, value: async (s) => { clip = s; } });
    Object.defineProperty(navigator.clipboard, "readText", { configurable: true, value: async () => clip });
  } catch { /* no clipboard object */ }

  const before = await fpNames();
  let name = null;
  const ids = {};
  await scenario("a footprint with a text box, a dimension and a dashed line", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Footprint")?.click();
    await until(() => E.state().tab === "footprint", 4000);
    await sleep(500);
    await E.run("pcbnew.ModuleEditor.newFootprint");
    name = await until(async () => (await fpNames()).find((n) => !before.includes(n)), 6000);
    if (!t(!!name, `a new footprint (${name})`)) return;
    // the items' JSON as the studio's reader gives it (`/api/footprint/parse`), added by the editor's verbs
    const src = `(footprint "Src" (version 20241229) (generator "pcbnew") (layer "F.Cu")
	(fp_text_box "BOX" (start 1 1) (end 4 2) (margins 0.25 0.25 0.25 0.25) (layer "F.Fab") (uuid "c0000000-0000-4000-8000-000000000001") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
	(dimension (type aligned) (layer "Dwgs.User") (uuid "d0000000-0000-4000-8000-000000000001") (pts (xy 0 -3) (xy 5 -3)) (height 1)
		(format (prefix "") (suffix "") (units 3) (units_format 1) (precision 4))
		(style (thickness 0.12) (arrow_length 1.27) (text_position_mode 0) (arrow_direction outward) (extension_height 0.58642) (extension_offset 0.5) (keep_text_aligned yes))
		(gr_text "5,0000 mm" (at 2.5 -4.15 0) (layer "Dwgs.User") (uuid "d0000000-0000-4000-8000-000000000002") (effects (font (size 1 1) (thickness 0.15)))))
)`;
    const parsed = await fetch("/api/footprint/parse", { method: "POST", body: src }).then((r) => r.json());
    const tb = parsed.footprint?.items?.text_boxes?.[0];
    const dimension = parsed.footprint?.items?.dimensions?.[0];
    if (!t(tb && dimension, `the reader gives a text box and a dimension (${parsed.error ?? "ok"})`)) return;
    const r1 = await cmd({ op: "fp_item", verb: "add", footprint: name, item: { text_box: tb } });
    t(r1.ok !== false, `the text box: ${r1.message ?? "ok"}`);
    const r2 = await cmd({ op: "fp_item", verb: "add", footprint: name, item: { dimension } });
    t(r2.ok !== false, `the dimension: ${r2.message ?? "ok"}`);
    const r3 = await cmd({ op: "add_footprint_graphic", footprint: name, shape: { kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: -2_000_000, y: 3_000_000 }, end: { x: 2_000_000, y: 3_000_000 } } });
    t(r3.ok !== false, `the line: ${r3.message ?? "ok"}`);
    let fp = await footprint(name);
    ids.line = fp.graphics[0]?.id;
    const r4 = await cmd({ op: "fp_item", verb: "set_graphic", footprint: name, id: ids.line, shape: { ...fp.graphics[0] }, look: { line_style: "dash" } });
    t(r4.ok !== false, `the line dashed: ${r4.message ?? "ok"}`);
    fp = await footprint(name);
    ids.tbx = fp.items?.text_boxes?.[0]?.id;
    ids.dim = fp.items?.dimensions?.[0]?.id;
    t(ids.tbx && ids.dim && ids.line, `ids ${JSON.stringify(ids)}`);
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
  });

  await scenario("FF1: Copy writes KiCad's clipboard footprint; Paste carries the copies from the cursor until a click, one undo step", async (t) => {
    await select([ids.tbx, ids.dim]);
    await hover(0, 0);
    await E.run("common.Interactive.copy");
    await sleep(800);
    t(/^\(footprint "clipboard:[0-9a-f-]+"/.test(clip), `KiCad's clipboard footprint (${clip.slice(0, 60)})`);
    t(clip.includes("(dimension") && /\((fp_text_box|text_box)/.test(clip), "with the dimension and the text box");
    await select([]);
    await hover(10_000_000, 0);
    await E.run("common.Interactive.paste");
    const carried = await until(() => E.state().tool === "move" && E.state().selection.length === 2, 5000);
    t(!!carried, `the pasted items are selected and carried (${E.state().tool}, ${JSON.stringify(E.state().selection)})`);
    await hover(12_000_000, 0);
    await clickAt(12_000_000, 0);
    const fp = await footprint(name);
    t((fp.items?.text_boxes ?? []).length === 2 && (fp.items?.dimensions ?? []).length === 2, `a second text box and dimension (${(fp.items?.text_boxes ?? []).length}, ${(fp.items?.dimensions ?? []).length})`);
    const moved = (fp.items?.dimensions ?? []).find((d) => d.id !== ids.dim);
    t(moved && moved.start.x === 12_000_000, `the copy's start is where the cursor dropped it (${JSON.stringify(moved?.start)})`);
    await undo();
    const back = await footprint(name);
    t((back.items?.text_boxes ?? []).length === 1 && (back.items?.dimensions ?? []).length === 1, "one undo takes the paste and its drop away");
    // Escape while carried leaves nothing
    await hover(10_000_000, 0);
    await E.run("common.Interactive.paste");
    await until(() => E.state().tool === "move", 4000);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(1200);
    const esc = await footprint(name);
    t((esc.items?.dimensions ?? []).length === 1, `Escape leaves no copy (${(esc.items?.dimensions ?? []).length})`);
  });

  await scenario("FF1: a paste of the text a .kicad_mod holds brings its items", async (t) => {
    clip = `(footprint "Other"
	(version 20241229)
	(generator "pcbnew")
	(layer "F.Cu")
	(fp_line (start -1 0) (end 1 0) (stroke (width 0.2) (type dot)) (layer "F.SilkS") (uuid "a1b2c3d4-0000-4000-8000-000000000001"))
	(fp_text_box "FROM KICAD" (start 0 1) (end 3 2) (margins 0.25 0.25 0.25 0.25) (layer "F.Fab") (uuid "a1b2c3d4-0000-4000-8000-000000000003") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.1) (type solid)))
	(pad "9" smd rect (at 0 3) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "a1b2c3d4-0000-4000-8000-000000000002"))
)`;
    const fp0 = await footprint(name);
    await select([]);
    await hover(-10_000_000, 0);
    await E.run("common.Interactive.paste");
    await until(() => E.state().tool === "move", 5000);
    await clickAt(-10_000_000, 0);
    const fp = await footprint(name);
    const pad = fp.pads.find((p) => p.number === "9");
    t(!!pad && pad.at.x === -10_000_000 && pad.at.y === 3_000_000, `the pad, its place moved by the paste point (${JSON.stringify(pad?.at)})`);
    t(fp.graphics.length === fp0.graphics.length + 1, "the line");
    t((fp.items?.text_boxes ?? []).some((b) => b.text === "FROM KICAD"), "and the text box");
    await undo();
    t((await footprint(name)).pads.length === fp0.pads.length, "one undo takes them away");
  });

  await scenario("FF2: Duplicate of a dimension and a dashed line keeps the line style and the dimension's own style; one undo step with the drop", async (t) => {
    await select([ids.dim, ids.line]);
    await hover(0, 0);
    await E.run("common.Interactive.duplicate");
    await until(() => E.state().tool === "move", 5000);
    await hover(0, 5_000_000);
    await clickAt(0, 5_000_000);
    const fp = await footprint(name);
    const dims = fp.items?.dimensions ?? [];
    t(dims.length === 2, `two dimensions (${dims.length})`);
    const copy = dims.find((d) => d.id !== ids.dim);
    const orig = dims.find((d) => d.id === ids.dim);
    t(copy && orig && copy.stroke_width === orig.stroke_width && copy.arrow_length === orig.arrow_length && copy.text_size_nm === orig.text_size_nm, "the copy has the dimension's style");
    t(copy && copy.start.y === orig.start.y + 5_000_000, `the copy moved with the drop (${JSON.stringify(copy?.start)})`);
    const line2 = fp.graphics.find((g) => g.id !== ids.line && g.kind === "segment");
    // the dashed style of the copy: the exported .kicad_mod says it
    const text = await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`, { cache: "no-store" }).then((r) => r.text()).catch(() => "");
    const dashes = (text.match(/\(type dash\)/g) ?? []).length;
    t(!!line2 && dashes >= 2, `the copy of the line is dashed too (${dashes} dashed lines in the .kicad_mod)`);
    await undo();
    const back = await footprint(name);
    t((back.items?.dimensions ?? []).length === 1 && back.graphics.length === fp.graphics.length - 1, "one undo takes the duplicate and its drop away");
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
