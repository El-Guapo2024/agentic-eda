// The Footprint Editor's round-3 leftovers of lane fpedit-1 (UI-LEFT-3 F3-1, F3-2, F3-3, F3-4, F3-10; task-v13-fpedit; PARITY-fpedit section 29):
//   F3-1   a user layer named in Preferences > Footprint Editor > User Layer Names shows by that name in the toolbar's layer box and the graphic dialog's layer box;
//   F3-2   the point editor's Create Corner (a polygon, a split line), Remove Corner, Chamfer Corner and Move Corner To on the one selected graphic, each one undo step;
//          the context menu's Shape Modification submenu;
//   F3-3   with a group entered, a line drawn joins the group;
//   F3-4   Mirror while a drag moves the selection mirrors it about the point held, one undo step with the drop; Escape reverts both;
//   F3-10  Copy's reference is the copied items' anchor nearest the cursor (BestDragOrigin); Copy as Text; Paste Special asks DIALOG_PASTE_SPECIAL and pastes; a board's
//          clipboard pastes its footprint's pad where the board had it and its own line; a bitmap on the clipboard is a reference image, an image copied comes back;
//          cells dragged over in a footprint table take a board's copied table;
//   F3-2   (zones) a footprint zone's corner dragged and a corner created; the zone dialog lists "Inner layers".
//
// Run it on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes a footprint of its own and
// puts the preferences it changes back. It answers through `window.__fpV13Check`: { results, done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=6
(async () => {
  const results = [];
  const check = (window.__fpV13Check = { results, done: false, ok: false, wait: null });
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
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(120); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(80); };
  const prefsStored = () => JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}");
  const prefs = async (page, fn) => {
    await E.run("common.SuiteControl.openPreferences");
    await sleep(300);
    tid(`prefs-page-${page}`)?.click();
    await sleep(200);
    await fn();
    tid("preferences-ok")?.click();
    await sleep(400);
  };
  const closeDialogs = async () => { for (let i = 0; i < 3 && document.querySelector(".dialog-backdrop"); i++) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel")?.click(); await sleep(250); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  // the canvas, in the editor's nm (y down)
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container") ?? document.querySelector(".pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => { const [x, y] = toScreen(wx, wy); canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", isPrimary: true })); };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(200); };
  const clickAt = async (wx, wy) => { await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(900); };
  const key = (k, target = document) => target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  // the system clipboard is the page's own here
  let clip = "";
  try {
    Object.defineProperty(navigator.clipboard, "writeText", { configurable: true, value: async (s) => { clip = s; } });
    Object.defineProperty(navigator.clipboard, "readText", { configurable: true, value: async () => clip });
  } catch { /* no clipboard object */ }

  const mm = (v) => Math.round(v * 1e6);
  const before = await fpNames();
  let name = null;
  const ids = {};
  /** An L-ish polygon (no symmetry, so a mirror shows), a line and a text. */
  const POLY = [[10, 0], [14, 0], [14, 1], [10, 3]].map(([x, y]) => ({ x: mm(x), y: mm(y) }));
  const polyOf = (fp) => fp.graphics.find((g) => g.id === ids.poly);
  await scenario("a footprint with a polygon, a line and a text", async (t) => {
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Footprint")?.click();
    await until(() => E.state().tab === "footprint", 4000);
    await sleep(500);
    await E.run("pcbnew.ModuleEditor.newFootprint");
    name = await until(async () => (await fpNames()).find((n) => !before.includes(n)), 6000);
    if (!t(!!name, `a new footprint (${name})`)) return;
    const r1 = await cmd({ op: "add_footprint_graphic", footprint: name, shape: { kind: "polygon", layer: "F.SilkS", stroke_width: 120_000, filled: false, pts: POLY } });
    const r2 = await cmd({ op: "add_footprint_graphic", footprint: name, shape: { kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: mm(-6), y: mm(6) }, end: { x: mm(-2), y: mm(6) } } });
    const r3 = await cmd({ op: "add_footprint_text", footprint: name, text: { content: "HELLO", at: { x: mm(-4), y: mm(9) }, angle: 0, layer: "F.Fab", size_nm: mm(1), stroke_width: mm(0.15), justify: "center", mirror: false } });
    t([r1, r2, r3].every((r) => r.ok !== false), `added (${[r1, r2, r3].map((r) => r.message ?? "ok").join(", ")})`);
    const fp = await footprint(name);
    ids.poly = fp.graphics.find((g) => g.kind === "polygon")?.id;
    ids.line = fp.graphics.find((g) => g.kind === "segment")?.id;
    ids.text = fp.texts.find((x) => x.content === "HELLO")?.id;
    t(ids.poly && ids.line && ids.text, `ids ${JSON.stringify(ids)}`);
    await E.run("common.Control.zoomFitScreen");
    await sleep(700);
  });

  await scenario("F3-1: a user layer named in User Layer Names shows by that name in the toolbar's layer box and the graphic dialog's", async (t) => {
    await prefs("fp_layer_names", async () => {
      document.querySelector('button[aria-label="Add Layer Name"]')?.click();
      await sleep(150);
      await choose(document.querySelector('select[aria-label="Layer name 1 layer"]'), "User.2");
      await type(document.querySelector('input[aria-label="Layer name 1"]'), "Assembly");
    });
    t(JSON.stringify(prefsStored().fpUserLayerNames) === JSON.stringify({ "User.2": "Assembly" }), `kept: ${JSON.stringify(prefsStored().fpUserLayerNames)}`);
    await sleep(300);
    const boxes = [...document.querySelectorAll(".toolbar-control select")].filter((s) => [...s.options].some((o) => o.value === "F.SilkS"));
    const opt = boxes.flatMap((s) => [...s.options]).find((o) => o.value === "User.2");
    t(opt && opt.textContent === "Assembly", `the toolbar's layer box: User.2 is "${opt?.textContent}"`);
    const silk = boxes.flatMap((s) => [...s.options]).find((o) => o.value === "F.SilkS");
    t(silk && silk.textContent === "F.Silkscreen", `a standard layer by its standard name ("${silk?.textContent}")`);
    await select([ids.line]);
    await E.run("pcbnew.InteractiveEdit.properties");
    const dlg = await until(() => tid("fp-shape-layer"), 4000);
    const o2 = dlg ? [...dlg.options].find((o) => o.value === "User.2") : null;
    t(o2 && o2.textContent === "Assembly", `the graphic dialog's layer box: User.2 is "${o2?.textContent}"`);
    await closeDialogs();
    // back to KiCad's defaults
    await prefs("fp_layer_names", async () => { [...document.querySelectorAll(".dialog-footer button")].find((b) => /^Reset .* to Defaults$/.test(b.textContent.trim()))?.click(); await sleep(150); });
    t(JSON.stringify(prefsStored().fpUserLayerNames ?? {}) === "{}", "the names put back");
  });

  await scenario("F3-2: Create / Remove / Chamfer Corner and Move Corner To on the selected polygon, Create Corner splitting a line; each one undo step", async (t) => {
    await select([ids.poly]);
    // Create Corner (Insert) with the cursor near the bottom side: a corner at its nearest point
    await hover(mm(12.5), mm(0.2));
    let r = await E.run("pcbnew.PointEditor.addCorner");
    t(r.ok !== false, `addCorner ran (${r.error ?? "ok"})`);
    await sleep(600);
    let p = polyOf(await footprint(name));
    t(p?.pts.length === 5 && p.pts.some((q) => q.y === 0 && q.x > mm(10.5) && q.x < mm(13.5)), `a fifth corner on the side (${JSON.stringify(p?.pts)})`);
    await undo();
    t(polyOf(await footprint(name))?.pts.length === 4, "one undo takes it away");
    // Remove Corner on the corner under the pointer
    await select([ids.poly]);
    await hover(mm(14), mm(1));
    r = await E.run("pcbnew.PointEditor.removeCorner");
    await sleep(600);
    p = polyOf(await footprint(name));
    t(p?.pts.length === 3 && !p.pts.some((q) => q.x === mm(14) && q.y === mm(1)), `the corner under the pointer removed (${JSON.stringify(p?.pts)})`);
    await undo();
    // Chamfer Corner: the corner nearest the cursor becomes two
    await select([ids.poly]);
    await hover(mm(10.1), mm(0.1));
    await E.run("pcbnew.PointEditor.chamferCorner");
    await sleep(600);
    p = polyOf(await footprint(name));
    t(p?.pts.length === 5 && !p.pts.some((q) => q.x === mm(10) && q.y === 0), `the corner at (10, 0) chamfered (${JSON.stringify(p?.pts)})`);
    await undo();
    // Move Corner To...: the dialog on the corner under the pointer
    await select([ids.poly]);
    await hover(mm(14), mm(0));
    r = await E.run("pcbnew.InteractiveEdit.moveCorner");
    t((r.dialog ?? E.state().dialogs.join("|")).includes("Move Corner to Location"), `Move Corner to Location opens (${r.dialog ?? E.state().dialogs})`);
    await closeDialogs();
    // the context menu's Shape Modification submenu, opened on a corner of the selected polygon
    await select([ids.poly]);
    const [cx, cy] = toScreen(mm(14), mm(0));
    canvasEl().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: cx, clientY: cy, button: 2 }));
    await sleep(400);
    const rows = [...document.querySelectorAll("div, li, button")].filter((el) => el.textContent.trim().startsWith("Shape Modification") && el.textContent.trim().length < 24);
    const row = rows[rows.length - 1];
    t(!!row, "the context menu has Shape Modification");
    if (row) {
      row.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      row.click();
      await sleep(300);
      const texts = [...document.querySelectorAll("div, li, button, span")].map((el) => el.textContent.trim());
      const want = ["Move Corner To...", "Create Corner", "Remove Corner", "Chamfer Corner"];
      t(want.every((l) => texts.includes(l)), `its rows: ${want.filter((l) => texts.includes(l)).join(", ")}`);
    }
    key("Escape");
    await sleep(300);
    // Create Corner on a line: it is split at the point nearest the cursor, the copy keeps its looks
    await select([ids.line]);
    await hover(mm(-4), mm(6.3));
    await E.run("pcbnew.PointEditor.addCorner");
    await sleep(900);
    const fp = await footprint(name);
    const segs = fp.graphics.filter((g) => g.kind === "segment");
    t(segs.length === 2 && segs.some((s) => s.end.x === mm(-4)) && segs.some((s) => s.start.x === mm(-4)), `the line split at x = -4 mm (${JSON.stringify(segs.map((s) => [s.start, s.end]))})`);
    await undo();
    t((await footprint(name)).graphics.filter((g) => g.kind === "segment").length === 1, "one undo joins it again");
  });

  await scenario("F3-3: with a group entered, a line drawn joins the group", async (t) => {
    await select([ids.line, ids.text]);
    await E.run("common.Interactive.group");
    await sleep(800);
    let fp = await footprint(name);
    const g = (fp.items?.groups ?? []).find((x) => x.member_ids.includes(ids.line));
    if (!t(!!g, "a group of the line and the text")) return;
    await select([g.id]);
    await E.run("common.Interactive.groupEnter");
    await sleep(300);
    await E.run("pcbnew.InteractiveDrawing.line");
    await sleep(300);
    await clickAt(mm(-6), mm(7));
    await clickAt(mm(-2), mm(7));
    key("Escape", canvasEl());
    await sleep(300);
    key("Escape", canvasEl());
    await sleep(600);
    fp = await footprint(name);
    const drawn = fp.graphics.find((x) => x.kind === "segment" && x.id !== ids.line);
    const group = (fp.items?.groups ?? []).find((x) => x.id === g.id);
    t(!!drawn, `the line is drawn (${JSON.stringify(fp.graphics.map((x) => x.kind))})`);
    t(drawn && group && group.member_ids.includes(drawn.id), `it joined the entered group (${JSON.stringify(group?.member_ids)})`);
    await E.run("common.Interactive.groupLeave");
    await sleep(200);
    await undo();
    t((await footprint(name)).graphics.filter((x) => x.kind === "segment").length === 1, "one undo takes the line and its membership away");
  });

  await scenario("F3-4: Mirror while a drag moves the polygon mirrors it about the point held, one undo step with the drop; Escape reverts both", async (t) => {
    const orig = polyOf(await footprint(name)).pts;
    const ySide = (pts, y) => pts.find((q) => q.y === y)?.x;
    await select([ids.poly]);
    // press on the polygon's bottom side away from its edit points, drag well past the threshold, mirror, drop
    await hover(mm(11), 0);
    fire("pointerdown", mm(11), 0, 1);
    await sleep(60);
    for (let i = 1; i <= 6; i++) { fire("pointermove", mm(11), mm(i), 1); await sleep(60); }
    const moving = await until(() => E.state().selection.length === 1 && E.state().selection[0]?.id === ids.poly, 1000);
    let r = await E.run("pcbnew.InteractiveEdit.mirrorHoriontally");
    t(r.ok !== false && moving, `Mirror Horizontally while moving (${r.error ?? "ok"})`);
    await sleep(700);
    fire("pointerup", mm(11), mm(6));
    await sleep(1200);
    let p = polyOf(await footprint(name));
    t(p && p.pts[0].y === orig[0].y + mm(6), `moved down 6 mm (${JSON.stringify(p?.pts)})`);
    // the corner at y = 3 was left of the one at y = 1; mirrored, it is right of it
    t(p && ySide(p.pts, orig[3].y + mm(6)) > ySide(p.pts, orig[2].y + mm(6)), "mirrored left to right");
    await undo();
    p = polyOf(await footprint(name));
    t(JSON.stringify(p?.pts) === JSON.stringify(orig), `one undo takes the drop and the mirror away (${JSON.stringify(p?.pts)})`);
    // Escape in the middle of the drag: nothing stays
    await select([ids.poly]);
    await hover(mm(11), 0);
    fire("pointerdown", mm(11), 0, 1);
    await sleep(60);
    for (let i = 1; i <= 4; i++) { fire("pointermove", mm(11 + i), mm(i), 1); await sleep(60); }
    r = await E.run("pcbnew.InteractiveEdit.mirrorVertically");
    await sleep(700);
    key("Escape", canvasEl());
    await sleep(700);
    fire("pointerup", mm(15), mm(4));
    await sleep(900);
    p = polyOf(await footprint(name));
    t(JSON.stringify(p?.pts) === JSON.stringify(orig), `Escape reverts the mirror and the move (${JSON.stringify(p?.pts)})`);
  });

  await scenario("F3-10: Copy measures from the nearest anchor (BestDragOrigin); Copy as Text; Paste Special; a board's clipboard", async (t) => {
    // the cursor a little off the polygon's corner at (10, 0): the clipboard is measured from that corner
    await select([ids.poly]);
    await hover(mm(10.3), mm(0.2));
    await E.run("common.Interactive.copy");
    await sleep(800);
    t(/^\(footprint "clipboard:/.test(clip), `KiCad's clipboard footprint (${clip.slice(0, 40)})`);
    t(/\(xy 0 0\)/.test(clip), `the polygon's corner is the origin of the copy (${(clip.match(/\(pts[^]*?\)\s*\)/) ?? [""])[0].slice(0, 80)})`);
    // Copy as Text of the text
    await select([ids.text]);
    let r = await E.run("common.Interactive.copyAsText");
    await sleep(300);
    t(clip === "HELLO", `Copy as Text gives the text (${JSON.stringify(clip)}; ${r.error ?? "ok"})`);
    // Paste Special: DIALOG_PASTE_SPECIAL, then the paste
    await select([ids.poly]);
    await hover(mm(10), 0);
    await E.run("common.Interactive.copy");
    await sleep(600);
    await select([]);
    const n0 = (await footprint(name)).graphics.length;
    await hover(mm(25), 0);
    r = await E.run("common.Interactive.pasteSpecial");
    await sleep(500);
    const dlg = [...document.querySelectorAll(".dialog-header")].find((h) => h.textContent.includes("Paste Special"));
    t(!!dlg, `Paste Special asks first (${E.state().dialogs})`);
    t(!tid("paste-special-clear-nets"), "no Clear net assignments for a footprint's clipboard");
    [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "OK")?.click();
    await until(() => E.state().tool === "move", 5000);
    await clickAt(mm(25), 0);
    t((await footprint(name)).graphics.length === n0 + 1, "OK pastes the polygon");
    await undo();
    // a board's clipboard: the footprint's pad where the board had it, and the board's own line
    clip = `(kicad_pcb (version 20240108) (generator "pcbnew")
	(footprint "R" (layer "F.Cu") (uuid "f0000000-0000-4000-8000-000000000001") (at 10 5 90)
		(pad "7" smd rect (at 1 0 90) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask") (uuid "f0000000-0000-4000-8000-000000000007"))
	)
	(gr_line (start 20 20) (end 25 20) (stroke (width 0.2) (type dash)) (layer "Dwgs.User") (uuid "f0000000-0000-4000-8000-000000000010"))
)`;
    const fp0 = await footprint(name);
    await select([]);
    await hover(0, 0);
    await E.run("common.Interactive.paste");
    await until(() => E.state().tool === "move", 5000);
    await clickAt(0, 0);
    const fp = await footprint(name);
    const pad = fp.pads.find((x) => x.number === "7");
    t(pad && pad.at.x === mm(10) && pad.at.y === mm(4), `the pad where the board had it (${JSON.stringify(pad?.at)})`);
    t(fp.graphics.some((g) => g.kind === "segment" && g.start.x === mm(20) && g.start.y === mm(20)), "the board's own line pasted too");
    await undo();
    t((await footprint(name)).pads.length === fp0.pads.length, "one undo takes the paste away");
  });

  await scenario("F3-2: a zone of the footprint gets the point editor -- a corner dragged, Create Corner -- and its dialog lists the editor board's layers by name", async (t) => {
    const outline = [[-20, -20], [-12, -20], [-12, -12], [-20, -12]].map(([x, y]) => ({ x: mm(x), y: mm(y) }));
    const r = await cmd({ op: "fp_item", verb: "add", footprint: name, item: { zone: { net: "", layer: "F.Cu", outline } } });
    if (!t(r.ok !== false, `a zone added (${r.message ?? "ok"})`)) return;
    let fp = await footprint(name);
    const z = (fp.items?.zones ?? [])[0];
    if (!t(!!z, "the zone")) return;
    ids.zone = z.id;
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
    await select([z.id]);
    // drag the corner at (-20, -20) to (-22, -21)
    await hover(mm(-20), mm(-20));
    fire("pointerdown", mm(-20), mm(-20), 1);
    await sleep(60);
    for (let i = 1; i <= 5; i++) { fire("pointermove", mm(-20 - (2 * i) / 5), mm(-20 - i / 5), 1); await sleep(60); }
    fire("pointerup", mm(-22), mm(-21));
    await sleep(1000);
    fp = await footprint(name);
    let o = (fp.items?.zones ?? []).find((x) => x.id === z.id)?.outline ?? [];
    t(o[0] && o[0].x === mm(-22) && o[0].y === mm(-21), `the corner dragged (${JSON.stringify(o[0])})`);
    await undo();
    // Create Corner on the top side
    await select([z.id]);
    await hover(mm(-16), mm(-19.8));
    await E.run("pcbnew.PointEditor.addCorner");
    await sleep(800);
    o = ((await footprint(name)).items?.zones ?? []).find((x) => x.id === z.id)?.outline ?? [];
    t(o.length === 5 && o.some((q) => q.y === mm(-20) && q.x > mm(-19) && q.x < mm(-13)), `a fifth corner on the side (${JSON.stringify(o)})`);
    await undo();
    // the zone dialog: the copper layers of the footprint's stackup by their names
    await select([z.id]);
    await E.run("pcbnew.InteractiveEdit.properties");
    await sleep(600);
    const dlg = document.querySelector(".dialog-backdrop .dialog, [role='dialog']");
    t(dlg && /Inner layers/.test(dlg.textContent), `the zone dialog lists "Inner layers" (${dlg ? "open" : "no dialog"})`);
    await closeDialogs();
  });

  await scenario("F3-10: a bitmap on the clipboard is pasted as a reference image; an image copied and pasted comes back", async (t) => {
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    const bytes = Uint8Array.from(atob(png), (ch) => ch.charCodeAt(0));
    let restore = null;
    try {
      const old = navigator.clipboard.read;
      Object.defineProperty(navigator.clipboard, "read", { configurable: true, value: async () => [{ types: ["image/png"], getType: async () => new Blob([bytes], { type: "image/png" }) }] });
      restore = () => Object.defineProperty(navigator.clipboard, "read", { configurable: true, value: old });
    } catch { /* no clipboard object */ }
    clip = "";
    const n0 = ((await footprint(name)).items?.images ?? []).length;
    await select([]);
    await hover(mm(30), mm(10));
    await E.run("common.Interactive.paste");
    const carried = await until(() => E.state().tool === "move", 5000);
    t(!!carried, "the image rides the cursor");
    await clickAt(mm(30), mm(10));
    let fp = await footprint(name);
    const imgs = fp.items?.images ?? [];
    t(imgs.length === n0 + 1, `a reference image (${imgs.length})`);
    restore?.();
    const img = imgs.find((x) => !x.id || true);
    if (!img) return;
    await select([imgs[imgs.length - 1].id]);
    await hover(mm(30), mm(10));
    await E.run("common.Interactive.copy");
    await sleep(600);
    t(/\(image/.test(clip), "Copy puts the image in KiCad's clipboard footprint");
    await select([]);
    await hover(mm(36), mm(10));
    await E.run("common.Interactive.paste");
    await until(() => E.state().tool === "move", 5000);
    await clickAt(mm(36), mm(10));
    fp = await footprint(name);
    t((fp.items?.images ?? []).length === n0 + 2, `the copy pasted back (${(fp.items?.images ?? []).length})`);
  });

  await scenario("F3-10: cells dragged over in a footprint table take a board's copied table (Paste Cells)", async (t) => {
    const cells = (n, prefix) => Array.from({ length: n }, (_, i) => ({ text: `${prefix}${i}`, margins: [mm(0.5), mm(0.5), mm(0.5), mm(0.5)] }));
    const table = (at, prefix) => ({ layer: "F.SilkS", at, col_widths: [mm(5), mm(5)], row_heights: [mm(3), mm(3)], cells: cells(4, prefix) });
    const rb = await cmd({ op: "table", verb: "add", table: { ...table({ x: mm(100), y: mm(100) }, "S"), cells: cells(2, "S"), row_heights: [mm(3)] } });
    const rf = await cmd({ op: "fp_item", verb: "add", footprint: name, item: { table: table({ x: mm(-20), y: mm(10) }, "F") } });
    if (!t(rb.ok !== false && rf.ok !== false, `the tables (${rb.message ?? "ok"}, ${rf.message ?? "ok"})`)) return;
    const st = await json("/api/state");
    const bt = (st.drawings?.tables ?? st.board?.drawings?.tables ?? []).slice(-1)[0];
    const copy = bt ? await fetch("/api/clipboard/copy", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ ids: [bt.id], reference: { x: mm(100), y: mm(100) } }) }).then((r) => r.json()) : null;
    if (!t(copy && copy.text && /\(table/.test(copy.text), "the board's table copied")) return;
    let fp = await footprint(name);
    const ft = (fp.items?.tables ?? [])[0];
    await E.run("common.Control.zoomFitScreen");
    await sleep(600);
    await select([]);
    // drag within the second row's first cell: it alone is selected
    await hover(mm(-18), mm(14.5));
    fire("pointerdown", mm(-18), mm(14.5), 1);
    await sleep(60);
    for (let i = 1; i <= 4; i++) { fire("pointermove", mm(-18 + i * 0.4), mm(14.5 + i * 0.2), 1); await sleep(60); }
    fire("pointerup", mm(-16.4), mm(15.3));
    await sleep(400);
    const sel = E.state().selection.map((x) => x.id);
    t(sel.length === 1 && /:1:0$/.test(sel[0]), `the cell selected (${JSON.stringify(sel)})`);
    clip = copy.text;
    await E.run("common.Interactive.paste");
    await sleep(900);
    fp = await footprint(name);
    const after = (fp.items?.tables ?? []).find((x) => x.id === ft.id);
    t(after && after.cells[2]?.text === "S0" && after.cells[3]?.text === "S1" && after.cells[0]?.text === "F0", `the copied cells in the second row (${JSON.stringify(after?.cells.map((c) => c.text))})`);
    await undo();
  });

  // the footprint made here goes
  if (name) await cmd({ op: "delete_library_footprint", name });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
