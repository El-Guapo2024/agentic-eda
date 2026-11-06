// Item kinds inside footprints check (UI-LEFT P17, PARITY-pcb.md section 84): a board footprint's text box, reference image, point and barcode -- drawn with it, picked as it, moved, turned and
// flipped with it, copied by Duplicate and Create Array -- an image's pixels turned and mirrored by Rotate and Flip on the board and on the sheet, and Duplicate and Create Array of the board's
// own text boxes, images, points and barcodes. Driven through `window.__eda` (the actions a person runs: R, F, Ctrl+D, Ctrl+T, the canvas's pointer events) and checked against what the
// server holds (`/api/state`, `/api/pcb_image`, `/api/sch_image`).
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir>`) -- through
// `e2e/run-check.mjs` -- on a COPY of a board: it pastes a footprint of its own (FPK1) outside the board's outline, adds board items and a sheet image, and undoes or deletes all of it.
// It answers through `window.__fpKindsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__fpKindsCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const schematic = () => json("/api/schematic");
  const cmd = (c) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json());
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };
  /** A command sent behind the studio's back is in its state after its next refresh: wait for the revision to move. */
  const send = async (c) => { const rev0 = E.state().revision; const r = await cmd(c); await waitFor(() => E.state().revision !== rev0, 8000); await sleep(400); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v)); el.dispatchEvent(new Event("input", { bubbles: true })); };
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(800); };
  const drag = async (toScreen, from, to) => {
    const [x0, y0] = await hover(toScreen, from[0], from[1]);
    fire("pointerdown", x0, y0, { buttons: 1 });
    for (let i = 1; i <= 6; i++) { const [x, y] = toScreen(from[0] + ((to[0] - from[0]) * i) / 6, from[1] + ((to[1] - from[1]) * i) / 6); fire("pointermove", x, y, { buttons: 1 }); await sleep(60); }
    const [x1, y1] = toScreen(to[0], to[1]);
    fire("pointerup", x1, y1); await sleep(1000);
  };

  const only = window.__fpKindsOnly instanceof RegExp ? window.__fpKindsOnly : null;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  /** A PNG of `w` x `h` magenta pixels with a white block in the top left quarter, as base64 (no resolution: read at 300 pixels per inch). */
  const makePng = async (w, h) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const g = c.getContext("2d");
    g.fillStyle = "#ff00ff";
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, w / 2, h / 2);
    const bytes = new Uint8Array(await (await new Promise((r) => c.toBlob(r, "image/png"))).arrayBuffer());
    let bin = "";
    for (const b of bytes) bin += String.fromCharCode(b);
    return btoa(bin);
  };
  /** The file at `url` decoded: its size and which quarter is white ("tl", "tr", "bl", "br"). */
  const whiteQuarter = async (url) => {
    const bmp = await createImageBitmap(await (await fetch(url, { cache: "no-store" })).blob());
    const c = document.createElement("canvas");
    c.width = bmp.width;
    c.height = bmp.height;
    const g = c.getContext("2d");
    g.drawImage(bmp, 0, 0);
    const white = (fx, fy) => { const d = g.getImageData(Math.floor(fx * c.width), Math.floor(fy * c.height), 1, 1).data; return d[0] > 200 && d[1] > 200 && d[2] > 200; };
    const q = [["tl", 0.25, 0.25], ["tr", 0.75, 0.25], ["bl", 0.25, 0.75], ["br", 0.75, 0.75]].filter(([, x, y]) => white(x, y)).map(([n]) => n);
    return { w: c.width, h: c.height, white: q.join(",") };
  };
  const pcbImageUrl = (i) => `/api/pcb_image?id=${encodeURIComponent(i.id)}&h=${encodeURIComponent(i.hash)}`;
  const pcbCanvas = () => document.querySelector(".pcb-canvas-container canvas");
  /** Pixels of the canvas the magenta image paints over the dark background (at its layer's alpha times the Images opacity). */
  const magentaPixels = () => {
    const c = pcbCanvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i] > 60 && d[i + 2] > 60 && d[i + 1] < 45 && Math.abs(d[i] - d[i + 2]) < 40) n++;
    return n;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await waitFor(() => E.state().tab === "pcb"); }
  const outline = (await board()).outline ?? [[0, 0], [35_000_000, 0], [35_000_000, 35_000_000], [0, 35_000_000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1e6, Math.min(...outline.map((p) => p[1])) / 1e6];
  // FPK1 goes on the board (Zoom to Fit fits the outline), the board's own items below it
  const home = [bx0 + 10, by0 + 9];
  const png = await makePng(40, 20);
  const lines = png.match(/.{1,76}/g).map((l) => `"${l}"`).join(" ");
  const clip = `(kicad_pcb (version 20241229) (generator "pcbnew")
  (footprint "FpKinds:Logo" (layer "F.Cu") (at 0 0 0)
    (property "Reference" "FPK1" (at 0 -7 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
    (property "Value" "Logo" (at 0 7 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))
    (attr smd)
    (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
    (fp_text_box "REV \${REFERENCE}" (start 1 1) (end 9 4) (margins 0.5 0.5 0.5 0.5) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15)) (justify left top)) (border yes) (stroke (width 0.12) (type solid)))
    (image (at -6 0) (layer "F.SilkS") (data ${lines}))
    (barcode (at 0 -3 0) (layer "F.SilkS") (size 3 3) (text "FPK1") (text_height 1) (type qr) (ecc_level L) (hide yes) (knockout no))
    (point (at 3 2) (size 1) (layer "F.Fab"))
    (pad "1" smd rect (at -2 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
  ))`;
  const fpk = async () => (await board()).parts.find((p) => p.ref === "FPK1" && p.placed);
  const mm = (v) => Math.round(v * 1e6);

  (async () => {
    let toScreen = null;
    await scenario("A footprint's text box, image, point and barcode come with it in a paste and are drawn with it; a click on one picks the footprint", async (t) => {
      const r = await send({ op: "paste_clipboard", text: clip, at: { x: mm(home[0]), y: mm(home[1]) } });
      t(r.ok !== false, `the paste is taken (${JSON.stringify(r).slice(0, 200)})`);
      const p = await fpk();
      if (!p) { t(false, "FPK1 is on the board"); return; }
      const it = p.items ?? {};
      t((it.text_boxes ?? []).length === 1 && (it.images ?? []).length === 1 && (it.points ?? []).length === 1 && (it.barcodes ?? []).length === 1, `the four kinds are the footprint's (${JSON.stringify(Object.keys(it))})`);
      const tb = it.text_boxes[0];
      t(near(tb.start[0], mm(home[0] + 1)) && near(tb.start[1], mm(home[1] + 1)) && tb.shown === "REV FPK1", `the text box is where the footprint puts it and shows its reference (${tb.start}, ${tb.shown})`);
      t(near(it.images[0].x, mm(home[0] - 6)) && near(it.points[0].x, mm(home[0] + 3)) && near(it.barcodes[0].y, mm(home[1] - 3)), "the image, the point and the barcode too");
      t(/^FPK1~img_/.test(it.images[0].id), `the image has the id its file is served by (${it.images[0].id})`);
      toScreen = await screenOf();
      if (!toScreen) { t(false, "the canvas gave a cursor readout"); return; }
      await E.run("common.InteractiveSelection.clear");
      await sleep(400);
      t(magentaPixels() > 20, `the canvas paints the footprint's image (${magentaPixels()} magenta pixels)`);
      await click(toScreen, home[0] + 7, home[1] + 3);
      t(E.state().selection.some((s) => s.id === "FPK1"), `a click in the text box picks the footprint (${JSON.stringify(E.state().selection)})`);
      await E.run("common.InteractiveSelection.clear");
      await sleep(200);
      await click(toScreen, home[0] - 6, home[1]);
      t(E.state().selection.some((s) => s.id === "FPK1"), "a click on the image picks it too");
    });
    if (!toScreen || !(await fpk())) { check.done = true; check.ok = false; return; }

    await scenario("Rotate turns the footprint's items with it and the image's pixels a quarter (REFERENCE_IMAGE::Rotate), one undo step for both", async (t) => {
      const p0 = await fpk();
      const img0 = p0.items.images[0];
      t((await whiteQuarter(pcbImageUrl(img0))).white === "tl", "the image's white block is at the top left");
      await select(["FPK1"]);
      await E.run("pcbnew.InteractiveEdit.rotateCcw");
      await waitFor(async () => (await fpk())?.items?.images?.[0]?.px?.[0] === 20, 8000);
      const p1 = await fpk();
      const img1 = p1.items.images[0];
      // a quarter counter-clockwise about the footprint's position: (-6, 0) goes to (0, 6)
      t(near(img1.x, mm(home[0])) && near(img1.y, mm(home[1] + 6)), `the image goes round with the footprint (${img1.x}, ${img1.y})`);
      t(near(p1.items.points[0].x, mm(home[0] + 2)) && near(p1.items.points[0].y, mm(home[1] - 3)), `so does the point (${p1.items.points[0].x}, ${p1.items.points[0].y})`);
      t(p1.items.text_boxes[0].angle === 90000 && p1.items.barcodes[0].angle === 90000, `the box's text and the barcode turn (${p1.items.text_boxes[0].angle}, ${p1.items.barcodes[0].angle})`);
      const q = await whiteQuarter(pcbImageUrl(img1));
      t(img1.px[0] === 20 && img1.px[1] === 40 && q.w === 20 && q.h === 40 && q.white === "bl", `the pixels are turned a quarter counter-clockwise (${img1.px}, white ${q.white})`);
      await undo();
      const p2 = await fpk();
      t(near(p2.items.images[0].x, mm(home[0] - 6)) && p2.items.images[0].px[0] === 40 && (await whiteQuarter(pcbImageUrl(p2.items.images[0]))).white === "tl", "one undo takes the turn and the pixels back");
    });

    await scenario("Flip puts the footprint's items on the back with it and mirrors the image (FOOTPRINT::Flip: top-bottom, then half a turn), one undo step", async (t) => {
      await select(["FPK1"]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await waitFor(async () => (await fpk())?.side === "bottom", 8000);
      const p = await fpk();
      const it = p.items;
      t(p.side === "bottom", "FPK1 is on the back");
      t(it.text_boxes[0].layer === "B.SilkS" && it.text_boxes[0].mirror === true && it.images[0].layer === "B.SilkS" && it.points[0].layer === "B.Fab" && it.barcodes[0].layer === "B.SilkS", "every item is on the back's layer, the box's text mirrored");
      t(near(it.images[0].x, mm(home[0] + 6)) && near(it.points[0].x, mm(home[0] - 3)), `mirrored about the footprint (${it.images[0].x}, ${it.points[0].x})`);
      const q = await whiteQuarter(pcbImageUrl(it.images[0]));
      t(q.white === "tr" && q.w === 40, `the image's pixels are mirrored left to right (white ${q.white})`);
      await undo();
      const back = await fpk();
      t(back.side === "top" && (await whiteQuarter(pcbImageUrl(back.items.images[0]))).white === "tl", "one undo takes both back");
    });

    await scenario("A drag of the footprint carries its items", async (t) => {
      await E.run("common.InteractiveSelection.clear");
      await sleep(200);
      await drag(toScreen, [home[0] - 2, home[1]], [home[0] + 3, home[1] + 2]);
      const p = await fpk();
      t(near(p.at[0], mm(home[0] + 5), 300_000) && near(p.items.images[0].x - p.at[0], mm(-6)) && near(p.items.text_boxes[0].start[0] - p.at[0], mm(1)), `the items keep their place on the footprint (${p.at}, image ${p.items.images[0].x})`);
      await undo();
      t(near((await fpk()).at[0], mm(home[0])), "one undo takes the move back");
    });

    await scenario("Duplicate (Ctrl+D) and Create Array (Ctrl+T) of the footprint copy its items", async (t) => {
      const refs0 = (await board()).parts.filter((p) => p.placed).map((p) => p.ref);
      await select(["FPK1"]);
      await E.run("common.Interactive.duplicate");
      await sleep(1200);
      await click(toScreen, home[0], home[1] + 15);
      const copy = (await board()).parts.find((p) => p.placed && !refs0.includes(p.ref));
      t(!!copy && (copy.items?.images ?? []).length === 1 && (copy.items?.text_boxes ?? []).length === 1 && (copy.items?.points ?? []).length === 1 && (copy.items?.barcodes ?? []).length === 1, `the duplicate (${copy?.ref}) has the four kinds`);
      if (copy) t(copy.items.text_boxes[0].shown === `REV ${copy.ref}`, `its text box shows its own reference (${copy.items.text_boxes[0].shown})`);
      await undo();
      t((await board()).parts.filter((p) => p.placed).length === refs0.length, "one undo takes the duplicate away");
      await select(["FPK1"]);
      await E.run("pcbnew.Array.createArray");
      await waitFor(() => !!tid("create-array"));
      setValue(tid("array-nx"), "3");
      setValue(tid("array-ny"), "1");
      setValue(tid("array-dx"), "15");
      await sleep(200);
      tid("array-ok").click();
      await waitFor(async () => (await board()).parts.filter((p) => p.placed).length === refs0.length + 2, 8000);
      const made = (await board()).parts.filter((p) => p.placed && !refs0.includes(p.ref));
      t(made.length === 2 && made.every((p) => (p.items?.images ?? []).length === 1 && (p.items?.barcodes ?? []).length === 1), `the array's two copies have the footprint's items (${made.map((p) => p.ref)})`);
      t(made.every((p) => near(p.items.images[0].x - p.at[0], mm(-6))), "each in its place on its copy");
      await undo();
      t((await board()).parts.filter((p) => p.placed).length === refs0.length, "one undo takes the array away");
    });

    // ------------------------------------------------------------------------------------------- the board's own items
    const at = [home[0], home[1] + 16];
    const items0 = async () => { const d = (await board()).drawings ?? {}; return { tb: d.text_boxes ?? [], img: d.images ?? [], pt: d.points ?? [], bar: d.barcodes ?? [] }; };
    const before = await items0();
    const known = new Set([...before.tb, ...before.img, ...before.pt, ...before.bar].map((x) => x.id));
    const mine = async () => { const d = await items0(); return { tb: d.tb.filter((x) => !known.has(x.id)), img: d.img.filter((x) => !known.has(x.id)), pt: d.pt.filter((x) => !known.has(x.id)), bar: d.bar.filter((x) => !known.has(x.id)) }; };
    await send({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "BOARD", start: { x: mm(at[0]), y: mm(at[1]) }, end: { x: mm(at[0] + 8), y: mm(at[1] + 3) }, margin_left_nm: 300000, margin_top_nm: 300000, margin_right_nm: 300000, margin_bottom_nm: 300000, stroke_width_nm: 120000, size_nm: 1000000 } });
    await send({ op: "pcb_item", verb: "add_ref_image", image: { layer: "F.SilkS", at: { x: mm(at[0] - 6), y: mm(at[1]) }, data: png } });
    await send({ op: "pcb_item", verb: "add_point", point: { layer: "F.Fab", at: { x: mm(at[0] + 12), y: mm(at[1]) }, size_nm: 1000000 } });
    await send({ op: "pcb_item", verb: "add_barcode", barcode: { layer: "F.SilkS", at: { x: mm(at[0] + 16), y: mm(at[1]) }, width_nm: 3000000, height_nm: 3000000, text: "B1", text_height_nm: 1000000 } });
    const added = await mine();
    const ids = [...added.tb, ...added.img, ...added.pt, ...added.bar].map((x) => x.id);

    await scenario("The board's text box, image, point and barcode are duplicated (EDIT_TOOL::Duplicate) and arrayed (ARRAY_TOOL) as KiCad's are", async (t) => {
      t(ids.length === 4, `the four board items are there (${ids.length})`);
      await select(ids);
      await E.run("common.Interactive.duplicate");
      await sleep(1200);
      await click(toScreen, at[0] + 4, at[1] + 10);
      let m = await mine();
      t(m.tb.length === 2 && m.img.length === 2 && m.pt.length === 2 && m.bar.length === 2, `Duplicate copies each of them (${m.tb.length}, ${m.img.length}, ${m.pt.length}, ${m.bar.length})`);
      t(m.img[1]?.hash === m.img[0]?.hash, "the copy of the image has its pixels");
      await undo();
      m = await mine();
      t(m.tb.length === 1 && m.img.length === 1 && m.pt.length === 1 && m.bar.length === 1, "one undo takes the copies away");
      await select(ids);
      await E.run("pcbnew.Array.createArray");
      await waitFor(() => !!tid("create-array"));
      setValue(tid("array-nx"), "1");
      setValue(tid("array-ny"), "3");
      setValue(tid("array-dy"), "6");
      await sleep(200);
      tid("array-ok").click();
      await waitFor(async () => (await mine()).tb.length === 3, 8000);
      m = await mine();
      t(m.tb.length === 3 && m.img.length === 3 && m.pt.length === 3 && m.bar.length === 3, `Create Array makes two copies of each (${m.tb.length}, ${m.img.length}, ${m.pt.length}, ${m.bar.length})`);
      const ys = m.pt.map((p) => p.y).sort((a, b) => a - b);
      t(ys.length === 3 && near(ys[1] - ys[0], mm(6)) && near(ys[2] - ys[1], mm(6)) && m.pt.every((p) => near(p.x, mm(at[0] + 12))), `the points 6 mm apart down the array (${m.pt.map((p) => [p.x, p.y].join(",")).join(" ")})`);
      await undo();
      t((await mine()).tb.length === 1, "one undo takes the array away");
    });

    await scenario("Rotate and Flip of a board image turn and mirror its pixels (PCB_REFERENCE_IMAGE::Rotate, Flip), one undo step each", async (t) => {
      const img = (await mine()).img[0];
      if (!img) { t(false, "the image is there"); return; }
      await select([img.id]);
      await E.run("pcbnew.InteractiveEdit.rotateCw");
      await waitFor(async () => (await mine()).img[0]?.px?.[0] === 20, 8000);
      let now = (await mine()).img[0];
      const q = await whiteQuarter(pcbImageUrl(now));
      t(now.px[0] === 20 && q.white === "tr", `a clockwise quarter: the white block at the top right (${now.px}, ${q.white})`);
      await undo();
      now = (await mine()).img[0];
      t(now.px[0] === 40 && (await whiteQuarter(pcbImageUrl(now))).white === "tl", "one undo turns it back");
      await select([img.id]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await waitFor(async () => (await mine()).img[0]?.layer === "B.SilkS", 8000);
      now = (await mine()).img[0];
      t(now.layer === "B.SilkS" && (await whiteQuarter(pcbImageUrl(now))).white === "tr", `a left-right flip mirrors it (${now.layer}, ${(await whiteQuarter(pcbImageUrl(now))).white})`);
      await undo();
      now = (await mine()).img[0];
      t(now.layer === "F.SilkS" && (await whiteQuarter(pcbImageUrl(now))).white === "tl", "one undo flips it back");
    });

    // tidy the board: the board items and FPK1
    for (const x of (await mine()).tb) await cmd({ op: "pcb_item", verb: "delete_text_box", id: x.id });
    for (const x of (await mine()).img) await cmd({ op: "pcb_item", verb: "delete_ref_image", id: x.id });
    for (const x of (await mine()).pt) await cmd({ op: "pcb_item", verb: "delete_point", id: x.id });
    for (const x of (await mine()).bar) await cmd({ op: "pcb_item", verb: "delete_barcode", id: x.id });
    await sleep(600);

    // ------------------------------------------------------------------------------------------- the sheet
    await scenario("On the sheet, Rotate and Mirror of an image turn and mirror its pixels (SCH_BITMAP::Rotate, MirrorHorizontally, MirrorVertically), one undo step each", async (t) => {
      if (E.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await waitFor(() => E.state().tab === "schematic"); await sleep(900); }
      const imagesOf = async () => ((await schematic()).graphics ?? []).filter((g) => g.shape.type === "image");
      const known = new Set((await imagesOf()).map((g) => g.id));
      await send({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "image", at: { x: 25_400_000, y: 25_400_000 }, scale_ppm: 1000000, data: png } } });
      const g = (await imagesOf()).find((x) => !known.has(x.id));
      if (!g) { t(false, "the image is on the sheet"); return; }
      const url = (x) => `/api/sch_image?id=${encodeURIComponent(x.id)}&h=${encodeURIComponent(x.shape.hash)}`;
      const mineNow = async () => (await imagesOf()).find((x) => x.id === g.id);
      t((await whiteQuarter(url(g))).white === "tl", "its white block is at the top left");
      await select([g.id]);
      await E.run("eeschema.InteractiveEdit.rotateCCW");
      await waitFor(async () => (await mineNow())?.shape.px?.[0] === 20, 8000);
      let now = await mineNow();
      t(now.shape.px[0] === 20 && (await whiteQuarter(url(now))).white === "bl", `R turns it a quarter counter-clockwise (${now.shape.px}, ${(await whiteQuarter(url(now))).white})`);
      await undo();
      now = await mineNow();
      t(now.shape.px[0] === 40 && (await whiteQuarter(url(now))).white === "tl", "one undo turns it back");
      await select([g.id]);
      await E.run("eeschema.InteractiveEdit.mirrorH");
      await waitFor(async () => (await mineNow())?.shape.hash !== now.shape.hash, 8000);
      now = await mineNow();
      t((await whiteQuarter(url(now))).white === "tr", `X mirrors it left to right (${(await whiteQuarter(url(now))).white})`);
      await undo();
      await select([g.id]);
      await E.run("eeschema.InteractiveEdit.mirrorV");
      await waitFor(async () => (await whiteQuarter(url(await mineNow()))).white === "bl", 8000);
      t((await whiteQuarter(url(await mineNow()))).white === "bl", "Y mirrors it top to bottom");
      await undo();
      t((await whiteQuarter(url(await mineNow()))).white === "tl", "one undo each");
      await cmd({ op: "sch_edit", verb: "delete_graphic", id: g.id });
      [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click();
      await sleep(600);
    });

    // the pasted footprint goes
    await cmd({ op: "rip", part: "FPK1" }).catch(() => null);
    await sleep(300);
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
