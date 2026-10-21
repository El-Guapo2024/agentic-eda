// Board items check (GAPS.md item 24, package P16): the item kinds the IR gained -- the text box, the reference image, the point, the hatch fill of a shape and the barcode -- driven the
// way a person does it through `window.__eda`, the dialogs' controls (their `data-testid`s) and the pointer events of the canvas, and checked against what the server holds (`/api/state`).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`) -- paste it
// into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: every item it makes it deletes again, every edit is undone, and it leaves
// Strict off while it runs. `window.__pcbItemsOnly = /regexp/` runs only the scenarios whose names match.
//
// What it checks, by step of the package:
//   1  the text box: Draw Text Box (two corners, Text Box Properties, OK adds, Cancel does not), the box grown to hold its text, the variables in its text, the drawn look (border,
//      wrapped text, knockout), Text Box Properties on a box that is there (E: layer, border style, margins, knockout), the Properties panel rows, selecting it by a click, moving it
//      with the pointer, Move / Rotate / Flip / Delete / Lock, every edit one undo step, and the file kicad-cli reads (`/api/export`'s derived board).
//   3  the point and the hatch fill: Place Point (each click a point on the active layer, repeating until Escape, snapping to the points that are there), the X and ring painted, a click that
//      picks it, the Properties rows (Size; the lock is not writeable), Flip, the Points row of the Appearance panel and the `(point ..)` of the derived board.
//   2  the reference image and the schematic's image: Place Reference Image (the file in the hand follows the cursor, a click puts it on the active layer, Escape drops it), the picture
//      painted on the canvas, Reference Image Properties (position, layer, lock, size and scale move together, the scale is checked, Convert to Greyscale), the Properties panel rows, a
//      click that picks it and a drag that carries it, the Images row of the Appearance panel, and the `(image ..)` of the derived board; on the schematic Place Image, the drop of a
//      PNG file (ddAddImage), Image Properties and the sheet drawn with the picture.
//
// It answers through `window.__pcbItemsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a minute or two (each edit waits for the server's answer): read it with `await __pcbItemsCheck.wait(40000)` (it returns when the run is done or the time is up) until
// `.done`. In a hidden pane the browser stops timers and animation frames; the script then drives both from a fetch loop.
(async () => {
  const results = [];
  const check = (window.__pcbItemsCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__pcbItemsSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 3e6;
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
    window.__pcbItemsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(500); };
  const redo = async () => { await E.run("common.Interactive.redo"); await sleep(500); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(500); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const check_ = (el, on) => { if (el.checked !== on) el.click(); };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };

  // The canvas: the pointer events a user makes. The cursor readout of the status bar gives the screen-to-board mapping.
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const screenOf = async (fit = true) => {
    if (fit) await E.run("common.Control.zoomFitObjects");
    await sleep(300);
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (toScreen, xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (toScreen, xmm, ymm) => { const [x, y] = await hover(toScreen, xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(700); };
  const drag = async (toScreen, from, to) => {
    const [x0, y0] = await hover(toScreen, from[0], from[1]);
    fire("pointerdown", x0, y0, { buttons: 1 });
    for (let i = 1; i <= 6; i++) { const [x, y] = toScreen(from[0] + ((to[0] - from[0]) * i) / 6, from[1] + ((to[1] - from[1]) * i) / 6); fire("pointermove", x, y, { buttons: 1 }); await sleep(60); }
    const [x1, y1] = toScreen(to[0], to[1]);
    fire("pointerup", x1, y1); await sleep(900);
  };

  const only = window.__pcbItemsOnly instanceof RegExp ? window.__pcbItemsOnly : null;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { await escape(); } catch { /* ignore */ }
    results.push(entry);
  };
  const textBoxes = async () => (await board()).drawings?.text_boxes ?? [];
  // the boxes the board had before the run are left alone
  const before = new Set((await textBoxes()).map((b) => b.id));
  const mine = async () => (await textBoxes()).filter((b) => !before.has(b.id));
  const deleteAll = async () => { for (const b of await mine()) await cmd({ op: "pcb_item", verb: "delete_text_box", id: b.id }); };
  // The reference images the board had before the run are left alone too.
  const images = async () => (await board()).drawings?.images ?? [];
  const imagesBefore = new Set((await images()).map((i) => i.id));
  const myImages = async () => (await images()).filter((i) => !imagesBefore.has(i.id));
  const deleteImages = async () => { for (const i of await myImages()) await cmd({ op: "pcb_item", verb: "delete_ref_image", id: i.id }); };
  /** A PNG the page makes: `w` x `h` pixels of `fill` with a white block in the top left quarter; the picked image the actions take ({ name, data: base64 }). No resolution: it is read at 300 pixels per inch. */
  const makePng = async (w, h, fill, name = "ref.png") => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const g = c.getContext("2d");
    g.fillStyle = fill;
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#ffffff";
    g.fillRect(0, 0, w / 2, h / 2);
    const bytes = new Uint8Array(await (await new Promise((r) => c.toBlob(r, "image/png"))).arrayBuffer());
    let bin = "";
    for (const b of bytes) bin += String.fromCharCode(b);
    return { name, data: btoa(bin), bytes: bytes.length };
  };
  const pcbCanvas = () => document.querySelector(".pcb-canvas-container canvas");
  /** How many pixels of the canvas are the magenta (255, 0, 255) an image paints at 60 % opacity over the black background: (153, 0, 153) -- nothing else of a board is this colour. */
  const magentaPixels = () => {
    const c = pcbCanvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (d[i] > 60 && d[i + 2] > 60 && d[i + 1] < 45 && Math.abs(d[i] - d[i + 2]) < 40) n++;
    return n;
  };
  const MAGENTA = "#ff00ff";
  /** A command sent behind the studio's back is in its state after its next refresh: wait for that (the design revision moves) before something needs the studio to know the item. */
  const addImage = async (image) => {
    const rev0 = E.state().revision;
    await cmd({ op: "pcb_item", verb: "add_ref_image", image });
    await waitFor(() => E.state().revision !== rev0, 8000);
    await sleep(400);
    return (await myImages()).at(-1);
  };
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }
  const outline = (await board()).outline ?? [[0, 0], [35000, 0], [35000, 35000], [0, 35000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1000, Math.min(...outline.map((p) => p[1])) / 1000];
  const at = (dx, dy) => [bx0 + dx, by0 + dy];
  const startCount = (await textBoxes()).length;

  (async () => {
    // ------------------------------------------------------------------------------------------------------------------ 1. the text box
    await scenario("Draw Text Box: two corners, Text Box Properties, OK adds the box grown to hold its text, Cancel adds nothing, one undo step each", async (t) => {
      t((await E.actions()).some((a) => a.id === "pcbnew.InteractiveDrawing.textbox" && a.enabled !== false), "Draw Text Box is an action of the board editor");
      await E.run("pcbnew.InteractiveDrawing.textbox");
      await sleep(300);
      const [a, b] = [at(5, 5), at(25, 7)]; // 20 mm wide, 2 mm high: too low for three lines
      await click(toScreen, a[0], a[1]);
      await hover(toScreen, b[0], b[1]);
      await click(toScreen, b[0], b[1]);
      await sleep(400);
      const dlg = tid("pcb-text-box-properties");
      t(!!dlg, "the second corner opens Text Box Properties");
      if (!dlg) return;
      t(tid("ptb-layer").value === "F.SilkS" || /Cu|Silk|Fab|User/.test(tid("ptb-layer").value), `the layer is the active one (${tid("ptb-layer")?.value})`);
      t(Number(tid("ptb-height").value) === 1 && tid("ptb-h-left").checked && tid("ptb-v-top").checked && tid("ptb-border").checked, "the layer's text size, left and top, with a border");
      setValue(tid("ptb-text"), "Three lines\nof text in a\nbox 2 mm high");
      await sleep(150);
      tid("ptb-ok").click();
      await sleep(900);
      let boxes = await textBoxes();
      t(boxes.length === startCount + 1, "OK adds the box");
      const box = boxes.find((x) => x.text.startsWith("Three lines"));
      t(!!box, "it has the text typed");
      if (!box) return;
      t(near(box.start[0], a[0] * 1000, 600) && near(box.start[1], a[1] * 1000, 600), `it starts at the first click (${box.start})`);
      t(box.end[1] - box.start[1] > 3000, `it was grown to hold three lines (${(box.end[1] - box.start[1]) / 1000} mm high)`);
      t(E.state().selection.some((s) => s.id === box.id && s.kind === "textbox"), "the new box is selected");
      await undo();
      t((await textBoxes()).length === startCount, "one undo takes it away");
      await redo();
      t((await textBoxes()).length === startCount + 1, "one redo brings it back");
      await undo();

      // Cancel throws the box away
      await E.run("pcbnew.InteractiveDrawing.textbox");
      await sleep(300);
      await click(toScreen, ...at(5, 5));
      await click(toScreen, ...at(20, 12));
      await sleep(300);
      t(!!tid("pcb-text-box-properties"), "a second box opens the dialog again");
      tid("ptb-cancel").click();
      await sleep(600);
      t((await textBoxes()).length === startCount, "Cancel adds nothing");
      await escape();
    });

    await scenario("Draw Text Box: a box with variables in its text shows what they stand for, and the pen and size rules of the dialog", async (t) => {
      await E.run("pcbnew.InteractiveDrawing.textbox");
      await sleep(300);
      await click(toScreen, ...at(3, 3));
      await click(toScreen, ...at(30, 12));
      await sleep(400);
      const dlg = tid("pcb-text-box-properties");
      if (!dlg) { t(false, "the dialog opens"); return; }
      setValue(tid("ptb-text"), "On ${LAYER}");
      setValue(tid("ptb-height"), "0");
      await sleep(100);
      tid("ptb-ok").click();
      await sleep(500);
      t(/between 0.001 mm and 250 mm/.test(tid("ptb-error")?.textContent ?? ""), "a text size of 0 is refused in KiCad's words and the dialog stays open");
      setValue(tid("ptb-height"), "1");
      setValue(tid("ptb-thickness"), "0.9");
      check_(tid("ptb-auto-thickness"), false);
      await sleep(100);
      tid("ptb-ok").click();
      await sleep(900);
      const box = (await textBoxes()).find((x) => x.text === "On ${LAYER}");
      t(!!box, "the box is added with its text as typed, variables and all");
      t(box && /^On /.test(box.shown ?? "") && !/\$\{/.test(box.shown ?? ""), `the server expanded the variable (${box?.shown})`);
      t(box && box.thickness <= 167, `a pen wider than a sixth of the glyph is clamped (${box?.thickness} um)`);
      await undo();
    });

    await scenario("Text Box Properties on a box that is there: layer, border style, margins, knockout, lock; the Properties panel rows say the same", async (t) => {
      await deleteAll();
      await cmd({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "Edit me please, this is a longer text", start: { x: bx0 * 1000 + 4000, y: by0 * 1000 + 4000 }, end: { x: bx0 * 1000 + 24000, y: by0 * 1000 + 14000 }, margin_left_um: 800, margin_top_um: 800, margin_right_um: 800, margin_bottom_um: 800, stroke_width_um: 100, size_um: 1000, halign: "left", valign: "top" } });
      const box = (await mine())[0];
      await select([box.id]);
      await sleep(400);
      // the Properties panel
      if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); }
      const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
      t(!!row("Margin Left") && !!row("Border Style") && !!row("Knockout") && !!row("Orientation"), "the panel has the margin, border, knockout and orientation rows of a text box");
      t(row("Margin Left")?.querySelector("[data-prop-input]")?.value.replace(/[^\d.]/g, "").startsWith("0.8"), "the Margin Left row says 0.8 mm");
      // E opens Text Box Properties on the box
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(500);
      t(!!tid("pcb-text-box-properties"), "E opens Text Box Properties");
      t(tid("ptb-text").value === "Edit me please, this is a longer text", "with the box's own text");
      setValue(tid("ptb-layer"), "F.Fab");
      setValue(tid("ptb-border-style"), "dash");
      setValue(tid("ptb-margin-left"), "1.5");
      check_(tid("ptb-knockout"), true);
      check_(tid("ptb-locked"), true);
      check_(tid("ptb-bold"), true);
      await sleep(150);
      tid("ptb-ok").click();
      await sleep(900);
      const after = (await mine())[0];
      t(after.id === box.id, "the box keeps its id");
      t(after.layer === "F.Fab" && after.line_style === "dash" && after.margins[0] === 1500 && after.knockout && after.bold, "its layer, border style, left margin, knockout and weight are the dialog's");
      t(((await board()).locked ?? []).includes(box.id), "it is locked");
      await undo();
      const restored = (await mine())[0];
      t(restored.layer === "F.SilkS" && restored.margins[0] === 800 && !restored.knockout, "one undo puts the whole edit back");
      t(!((await board()).locked ?? []).includes(box.id), "lock included");
      // the panel edits the box too: one batch, one undo step
      await select([box.id]);
      await sleep(300);
      const input = row("Margin Top")?.querySelector("[data-prop-input]");
      if (input) {
        setValue(input, "1.2");
        input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        input.dispatchEvent(new Event("blur", { bubbles: true }));
        await sleep(900);
        t((await mine())[0].margins[1] === 1200, "a margin typed in the panel is the box's");
        await undo();
        t((await mine())[0].margins[1] === 800, "and one undo takes it back");
      } else t(false, "the Margin Top row has an input");
    });

    await scenario("a text box is picked by a click and carried by the pointer, one undo step; Move, Rotate, Flip, Delete and Lock take it like any item", async (t) => {
      await deleteAll();
      // the Properties panel of the last scenario changed the size of the canvas: measure it again
      toScreen = (await screenOf()) ?? toScreen;
      await cmd({ op: "pcb_item", verb: "add_text_box", text_box: { layer: "F.SilkS", text: "Pick me", start: { x: bx0 * 1000 + 6000, y: by0 * 1000 + 6000 }, end: { x: bx0 * 1000 + 16000, y: by0 * 1000 + 10000 }, margin_left_um: 800, margin_top_um: 800, margin_right_um: 800, margin_bottom_um: 800, stroke_width_um: 100, size_um: 1000, halign: "left", valign: "top" } });
      const box = (await mine())[0];
      await E.run("common.InteractiveSelection.clear");
      await escape();
      await click(toScreen, bx0 + 11, by0 + 8); // inside the box, off its text
      t(E.state().selection.some((s) => s.id === box.id && s.kind === "textbox"), "a click inside the box selects it (PCB_TEXTBOX::HitTest is its whole box)");
      // carry it with the pointer
      await drag(toScreen, [bx0 + 11, by0 + 8], [bx0 + 16, by0 + 13]);
      const moved = (await mine())[0];
      t(near(moved.start[0] - box.start[0], 5000, 600) && near(moved.start[1] - box.start[1], 5000, 600), `a drag moves it by the drag (${moved.start[0] - box.start[0]}, ${moved.start[1] - box.start[1]})`);
      await undo();
      t((await mine())[0].start[0] === box.start[0], "one undo puts it back");
      // the verbs of any item
      await cmd({ op: "move_items", ids: [box.id], dx: 1000, dy: 2000 });
      t((await mine())[0].start[0] === box.start[0] + 1000, "Move Exactly takes it");
      await undo();
      await select([box.id]);
      await E.run("pcbnew.InteractiveEdit.rotateCcw");
      await sleep(700);
      const turned = (await mine())[0];
      t(turned.angle === 90000 && turned.pts.length === 0, `R turns it a quarter, the text angle is KiCad's (${turned.angle})`);
      await undo();
      await select([box.id]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(700);
      const flipped = (await mine())[0];
      t(flipped.layer === "B.SilkS" && flipped.mirror, `F puts it on the back and mirrors the text (${flipped.layer}, mirror ${flipped.mirror})`);
      await undo();
      await select([box.id]);
      await E.run("pcbnew.EditorControl.toggleLock");
      await sleep(500);
      t(((await board()).locked ?? []).includes(box.id), "L locks it");
      await E.run("pcbnew.EditorControl.toggleLock");
      await sleep(400);
      t(!((await board()).locked ?? []).includes(box.id), "and L again unlocks it");
      await select([box.id]);
      await E.run("common.Interactive.delete");
      await sleep(700);
      t((await mine()).length === 0, "Delete removes it");
      await undo();
      t((await mine()).length === 1, "one undo brings it back");
    });

    await scenario("the 3D view draws a text box on its layer: the silkscreen, an adhesive layer, a user layer", async (t) => {
      await deleteAll();
      const mk = (layer) => cmd({ op: "pcb_item", verb: "add_text_box", text_box: { layer, text: "3D box", start: { x: bx0 * 1000 + 6000, y: by0 * 1000 + 6000 }, end: { x: bx0 * 1000 + 16000, y: by0 * 1000 + 10000 }, margin_left_um: 800, margin_top_um: 800, margin_right_um: 800, margin_bottom_um: 800, stroke_width_um: 100, size_um: 1000, halign: "left", valign: "top" } });
      await mk("F.SilkS");
      await mk("B.SilkS");
      await mk("Dwgs.User");
      await mk("F.CrtYd");
      t((await mine()).length === 4, "the board has four boxes");
      await E.run("common.Control.show3DViewer");
      const t0 = Date.now();
      while (Date.now() - t0 < 60000 && !(E.state().viewer3d && E.state().viewer3d.open)) await sleep(100);
      await sleep(1500);
      const scene = E.state().viewer3d.scene;
      const silkBefore = (await textBoxes()).filter((b) => before.has(b.id) && /\.SilkS$/.test(b.layer)).length;
      t((scene["silk-text-box"] ?? 0) === 2 + silkBefore, `the two silkscreen boxes are drawn, front and back, with the ${silkBefore} the board had (${JSON.stringify(scene)})`);
      t(Object.keys(scene).some((k) => /text-box/.test(k) && k !== "silk-text-box"), "the user layer's box is drawn on its own row");
      t(!Object.keys(scene).includes("F.CrtYd-text-box") && Object.values(scene).length > 0, "a layer KiCad's 3D view does not draw (courtyard) has none");
      // back to the board editor
      [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click();
      await sleep(800);
      await deleteAll();
    });

    // ------------------------------------------------------------------------------------------------------------------ 2. the reference image
    await scenario("Place Reference Image: the file in the hand follows the cursor, a click puts it on the active layer as one undo step and selects it, Escape drops it, the picture is painted", async (t) => {
      await deleteAll();
      await deleteImages();
      toScreen = (await screenOf()) ?? toScreen;
      t((await E.actions()).some((a) => a.id === "pcbnew.InteractiveDrawing.placeReferenceImage" && a.enabled !== false), "Place Reference Image is an action of the board editor");
      const png = await makePng(240, 160, MAGENTA);
      const base = magentaPixels();
      await E.run("pcbnew.InteractiveDrawing.placeReferenceImage", png);
      await waitFor(() => /put .* down/i.test(E.state().picker ?? ""));
      t(/put ref\.png down/i.test(E.state().picker ?? ""), `the file is in the hand (${E.state().picker})`);
      await hover(toScreen, ...at(10, 10));
      await sleep(400);
      t(magentaPixels() > base + 800, `the image follows the cursor, painted as it will be (${base} -> ${magentaPixels()} magenta pixels)`);
      // a click puts it down
      await click(toScreen, ...at(10, 10));
      await waitFor(async () => (await myImages()).length === 1);
      const list = await myImages();
      t(list.length === 1, "the click adds the image");
      const im = list[0];
      if (!im) return;
      t(near(im.x, (bx0 + 10) * 1000, 1500) && near(im.y, (by0 + 10) * 1000, 1500), `at the click (${im.x}, ${im.y}) um`);
      t(im.w === 20320 && im.h === 13547 && im.px[0] === 240 && im.ppi === 300 && im.scale_ppm === 1000000, `240 x 160 pixels at 300 ppi are 20.32 x 13.55 mm at scale 1 (${im.w} x ${im.h})`);
      t(typeof im.layer === "string" && im.layer.length > 0, `it is on the active layer (${im.layer})`);
      t(E.state().selection.some((s) => s.id === im.id && s.kind === "image"), "the new image is selected");
      const r = await fetch(`/api/pcb_image?id=${im.id}&h=${im.hash}`);
      const got = new Uint8Array(await r.arrayBuffer());
      t(r.headers.get("content-type") === "image/png" && got.length === png.bytes && got[1] === 0x50, "GET /api/pcb_image gives the PNG file that was placed, byte for byte in size");
      // the tool goes on: the next click asks for another file
      t(/choose an image file/i.test(E.state().picker ?? ""), `the tool waits for the next file (${E.state().picker})`);
      await E.run("common.InteractiveSelection.clear");
      await waitFor(() => magentaPixels() > base + 800);
      t(magentaPixels() > base + 800, `the board paints the picture (${magentaPixels()} magenta pixels against ${base})`);
      await escape();
      t(E.state().picker == null, "Escape ends the tool");
      await undo();
      t((await myImages()).length === 0, "one undo takes the image away");
      await waitFor(() => magentaPixels() <= base + 100);
      t(magentaPixels() <= base + 100, "and the picture with it");
      await redo();
      t((await myImages()).length === 1, "one redo brings it back");
      await undo();

      // Escape with an image in the hand drops it and leaves the tool waiting; a second Escape ends it
      await E.run("pcbnew.InteractiveDrawing.placeReferenceImage", png);
      await waitFor(() => /put .* down/i.test(E.state().picker ?? ""));
      await escape();
      t(/choose an image file/i.test(E.state().picker ?? ""), `Escape drops the image and the tool waits (${E.state().picker})`);
      await escape();
      t(E.state().picker == null && (await myImages()).length === 0, "the second Escape ends the tool, and no image was added");
      // a file that is no image is refused in KiCad's words
      await E.run("pcbnew.InteractiveDrawing.placeReferenceImage", { name: "x.png", data: btoa("this is not an image at all, just text") });
      await sleep(600);
      t(/Could not load image from 'x\.png'/.test(document.body.innerText), "a file that is not an image says so");
      await escape();
      await escape();
    });

    await scenario("Reference Image Properties (E): the size and the scale move together, the scale is checked, layer and lock, Convert to Greyscale; one undo step each", async (t) => {
      await deleteImages();
      const png = await makePng(240, 160, MAGENTA);
      const im = await addImage({ layer: "Cmts.User", at: { x: (bx0 + 20) * 1000, y: (by0 + 20) * 1000 }, data: png.data });
      if (!im) { t(false, "the image was added"); return; }
      await select([im.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(600);
      t(!!tid("pcb-ref-image-properties"), "E opens Reference Image Properties");
      t(tid("pri-ppi")?.textContent === "300" && Number(tid("pri-width")?.value) === 20.32 && Number(tid("pri-height")?.value) === 13.547 && tid("pri-scale")?.value === "1", "with the image's definition, size and scale");
      t(tid("pri-layer")?.value === "Cmts.User" && !tid("pri-locked")?.checked, "its layer, and not locked");
      setValue(tid("pri-width"), "10.16");
      await sleep(150);
      t(tid("pri-scale").value === "0.5" && Number(tid("pri-height").value) === 6.773, `a width of 10.16 mm is half the scale and the height follows (${tid("pri-scale").value}, ${tid("pri-height").value})`);
      setValue(tid("pri-height"), "13.547");
      await sleep(150);
      // (a height typed to the micrometre is 13.547 mm, 0.3 um more than 160 pixels at 300 ppi: the scale is 1.000025)
      t(near(Number(tid("pri-scale").value), 1, 1e-3) && near(Number(tid("pri-width").value), 20.32, 0.002), `a height back to 13.547 mm is scale 1 again (${tid("pri-scale").value}, ${tid("pri-width").value})`);
      setValue(tid("pri-scale"), "0.01");
      await sleep(150);
      tid("pri-ok").click();
      await sleep(500);
      t(/too small/i.test(tid("pri-error")?.textContent ?? "") && !!tid("pcb-ref-image-properties"), `a scale that makes the image under 15 pixels is refused (${tid("pri-error")?.textContent})`);
      setValue(tid("pri-scale"), "0.5");
      setValue(tid("pri-x"), String(bx0 + 25));
      setValue(tid("pri-layer"), "Dwgs.User");
      check_(tid("pri-locked"), true);
      await sleep(150);
      tid("pri-ok").click();
      await sleep(1000);
      const after = (await myImages())[0];
      t(after.id === im.id && after.scale_ppm === 500000 && after.layer === "Dwgs.User" && near(after.x, (bx0 + 25) * 1000, 2), `OK applies scale, layer and position (${after.scale_ppm}, ${after.layer}, ${after.x})`);
      t(((await board()).locked ?? []).includes(im.id), "and the lock");
      t(after.w === 10160 && after.hash === im.hash, "the size follows the scale and the pixels are the same");
      await undo();
      const back = (await myImages())[0];
      t(back.scale_ppm === 1000000 && back.layer === "Cmts.User" && back.x === im.x && !((await board()).locked ?? []).includes(im.id), "one undo puts the whole edit back");

      // Convert to Greyscale: the working copy is shown, OK keeps it, one undo step
      await select([im.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(500);
      tid("pri-greyscale").click();
      await waitFor(() => (tid("pri-preview")?.getAttribute("src") ?? "").startsWith("data:image/png;base64,"));
      t((tid("pri-preview")?.getAttribute("src") ?? "").startsWith("data:image/png;base64,"), "Convert to Greyscale shows the converted picture");
      tid("pri-ok").click();
      await sleep(1000);
      const grey = (await myImages())[0];
      t(grey.hash !== im.hash && grey.w === im.w && grey.ppi === 300 && grey.scale_ppm === im.scale_ppm, "the pixels changed and nothing else did (same size and definition)");
      const pic = new Image();
      pic.src = `/api/pcb_image?id=${grey.id}&h=${grey.hash}`;
      await pic.decode();
      const c = document.createElement("canvas");
      c.width = pic.naturalWidth;
      c.height = pic.naturalHeight;
      const g = c.getContext("2d");
      g.drawImage(pic, 0, 0);
      const [pr, pg, pb] = g.getImageData(c.width - 4, c.height - 4, 1, 1).data;
      t(pr === pg && pg === pb && pr > 90 && pr < 120, `a pixel of the magenta part is grey (${pr}, ${pg}, ${pb}; 0.299 R + 0.587 G + 0.114 B of (255, 0, 255) is 105)`);
      await undo();
      t((await myImages())[0].hash === im.hash, "one undo brings the colours back");
      // Cancel changes nothing
      await select([im.id]);
      await E.run("pcbnew.InteractiveEdit.properties");
      await sleep(400);
      setValue(tid("pri-scale"), "2");
      tid("pri-cancel").click();
      await sleep(400);
      t((await myImages())[0].scale_ppm === 1000000, "Cancel leaves the image as it was");
      await deleteImages();
    });

    await scenario("a reference image is picked anywhere in its box and carried by the pointer; Move, Flip, Lock and Delete take it; the Properties panel and the Appearance panel know it", async (t) => {
      await deleteImages();
      const png = await makePng(240, 160, MAGENTA);
      // outside the board, where nothing else is, on a layer that has a back
      const im = await addImage({ layer: "F.SilkS", at: { x: (bx0 - 20) * 1000, y: (by0 + 10) * 1000 }, data: png.data });
      await select([im.id]);
      await E.run("common.Control.zoomFitSelection");
      await sleep(700);
      toScreen = (await screenOf(false)) ?? toScreen;
      await E.run("common.InteractiveSelection.clear");
      await escape();
      const cx = im.x / 1000;
      const cy = im.y / 1000;
      await click(toScreen, cx + 7, cy + 4); // inside the box, off its centre
      t(E.state().selection.some((s) => s.id === im.id && s.kind === "image"), "a click in the box picks the image (PCB_REFERENCE_IMAGE::HitTest is its bounding box)");
      await drag(toScreen, [cx + 7, cy + 4], [cx + 9, cy + 6]);
      const moved = (await myImages())[0];
      t(near(moved.x - im.x, 2000, 600) && near(moved.y - im.y, 2000, 600), `a drag carries it (${moved.x - im.x}, ${moved.y - im.y})`);
      await undo();
      t((await myImages())[0].x === im.x, "one undo puts it back");

      // the Properties panel
      await select([im.id]);
      if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); }
      const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
      t(!!row("Scale") && !!row("Width") && !!row("Height") && !!row("Associated Layer"), "the panel has the Scale, Width, Height and Associated Layer rows");
      const input = row("Width")?.querySelector("[data-prop-input]");
      if (input) {
        setValue(input, "10.16");
        input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        input.dispatchEvent(new Event("blur", { bubbles: true }));
        await sleep(900);
        const w = (await myImages())[0];
        t(w.scale_ppm === 500000 && w.w === 10160 && w.x === im.x, `a width typed in the panel is a scale about the centre (${w.scale_ppm}, ${w.w})`);
        await undo();
        t((await myImages())[0].scale_ppm === 1000000, "one undo");
      } else t(false, "the Width row has an input");

      // Flip puts it on the other side's layer; Lock; Delete
      await select([im.id]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(700);
      t((await myImages())[0].layer === "B.SilkS", `F puts it on the back layer (${(await myImages())[0].layer})`);
      await undo();
      await select([im.id]);
      await E.run("pcbnew.EditorControl.toggleLock");
      await sleep(500);
      t(((await board()).locked ?? []).includes(im.id), "L locks it");
      await E.run("pcbnew.EditorControl.toggleLock");
      await sleep(400);
      await cmd({ op: "move_items", ids: [im.id], dx: 1000, dy: 0 });
      t((await myImages())[0].x === im.x + 1000, "Move Exactly takes it");
      await undo();

      // the Images row of the Appearance panel: off, the picture is gone; opacity 0 too
      await select([im.id]);
      await E.run("common.Control.zoomFitSelection");
      await sleep(300);
      await E.run("common.InteractiveSelection.clear");
      await sleep(500);
      const shown = magentaPixels();
      t(shown > 800, `the picture is painted (${shown})`);
      await E.run("studio.Appearance.op", { op: "object", id: "bitmaps", visible: false });
      await sleep(500);
      t(magentaPixels() < 100, `Images off hides it (${magentaPixels()})`);
      await E.run("studio.Appearance.op", { op: "object", id: "bitmaps", visible: true });
      await E.run("studio.Appearance.op", { op: "opacity", key: "images", value: 0 });
      await sleep(500);
      t(magentaPixels() < 100, `an opacity of 0 hides it too (${magentaPixels()})`);
      await E.run("studio.Appearance.op", { op: "opacity", key: "images", value: 0.6 });
      await sleep(400);
      t(magentaPixels() > 800, "and 60 % shows it again");
      await select([im.id]);
      await E.run("common.Interactive.delete");
      await sleep(700);
      t((await myImages()).length === 0, "Delete removes it");
      await undo();
      t((await myImages()).length === 1, "one undo brings it back");
      // the file kicad-cli reads
      const text = await fetch("/api/board.kicad_pcb").then((r) => r.text());
      t(/\(image\s+\(at [\d.-]+ [\d.-]+\)\s*\(layer "F\.SilkS"\)/.test(text) && text.includes('(data'), "the derived board has the (image ..) with its layer and data");
      await deleteImages();
    });

    // ------------------------------------------------------------------------------------------------------------------ 2. the schematic's image
    const schematic = () => json("/api/schematic");
    const schImages = async () => ((await schematic()).graphics ?? []).filter((g) => g.shape.type === "image");
    const schBefore = new Set((await schImages()).map((g) => g.id));
    const mySchImages = async () => (await schImages()).filter((g) => !schBefore.has(g.id));
    const deleteSchImages = async () => { for (const g of await mySchImages()) await cmd({ op: "sch_edit", verb: "delete_graphic", id: g.id }); };
    const sheetCanvas = () => document.querySelector(".pcb-canvas-container canvas");
    /** The pixels of the sheet that are the magenta (255, 0, 255) a schematic image paints opaque, whatever lies over them. */
    const sheetMagenta = () => {
      const c = sheetCanvas();
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      let n = 0;
      for (let i = 0; i < d.length; i += 4) if (d[i] > 235 && d[i + 2] > 235 && d[i + 1] < 25) n++;
      return n;
    };
    const sheetScreen = (x, y) => { const v = E.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
    const sheetHover = async (x, y) => { const [px, py] = sheetScreen(x, y); fire("pointermove", px, py); await sleep(200); };
    const sheetClick = async (x, y) => { const [px, py] = sheetScreen(x, y); fire("pointermove", px, py); await sleep(150); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(700); };
    /** A spot of the root sheet with nothing within `margin` um of a box `w` x `h` around it: a sheet, a wire, a label, a symbol. */
    const freeSpot = (sch, w, h, margin = 9000) => {
      const dots = [...sch.sheets.map((x) => [x.at[0], x.at[1], x.at[0] + x.size[0], x.at[1] + x.size[1]]), ...sch.wires.flatMap((x) => x.pts.map((p) => [p[0], p[1], p[0], p[1]])), ...sch.labels.map((l) => [l.at[0], l.at[1], l.at[0], l.at[1]]), ...sch.symbols.map((q) => [q.at[0] - 15000, q.at[1] - 15000, q.at[0] + 15000, q.at[1] + 15000]), ...sch.texts.map((q) => [q.at[0], q.at[1], q.at[0], q.at[1]])];
      for (let y = 45000; y < 150000; y += 10000) for (let x = 45000; x < 260000; x += 10000) {
        const box = [x - w / 2 - margin, y - h / 2 - margin, x + w / 2 + margin, y + h / 2 + margin];
        if (!dots.some((b) => b[0] <= box[2] && b[2] >= box[0] && b[1] <= box[3] && b[3] >= box[1])) return [x, y];
      }
      return null;
    };
    const toSchematic = async () => {
      if (E.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await waitFor(() => E.state().tab === "schematic"); await sleep(900); }
      await E.run("common.Control.zoomFitScreen");
      await sleep(500);
    };
    const toBoard = async () => { if (E.state().tab !== "pcb") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await waitFor(() => E.state().tab === "pcb"); await sleep(700); } };
    const dialogButton = (label) => [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === label);

    await scenario("Place Image on the schematic: the file in the hand follows the cursor, a click puts it on the grid as one undo step and selects it, Escape drops it, the sheet paints the picture", async (t) => {
      await toSchematic();
      await deleteSchImages();
      t((await E.actions()).some((a) => a.id === "eeschema.InteractiveDrawing.placeImage" && a.enabled !== false), "Place Image is an action of the schematic editor");
      const png = await makePng(240, 160, MAGENTA);
      const base = sheetMagenta();
      await E.run("eeschema.InteractiveDrawing.placeImage", png);
      await waitFor(() => /put .* down/i.test(E.state().picker ?? ""));
      t(/put ref\.png down/i.test(E.state().picker ?? ""), `the file is in the hand (${E.state().picker})`);
      const spot = freeSpot(await schematic(), 20320, 13547);
      if (!spot) { t(false, "the sheet has a free spot"); return; }
      await sheetHover(spot[0] + 300, spot[1] + 200);
      await sleep(400);
      t(sheetMagenta() > base + 500, `the image follows the cursor, painted as it will be (${base} -> ${sheetMagenta()} magenta pixels)`);
      await sheetClick(spot[0] + 300, spot[1] + 200);
      await waitFor(async () => (await mySchImages()).length === 1);
      const list = await mySchImages();
      t(list.length === 1, "the click adds the image");
      const g = list[0];
      if (!g) return;
      const sh = g.shape;
      const grid = E.state().grid ?? 1270;
      t(near(sh.at.x, spot[0] + 300, grid) && near(sh.at.y, spot[1] + 200, grid), `at the click, on the grid (${sh.at.x}, ${sh.at.y})`);
      t(grid === 0 || (sh.at.x % grid === 0 && sh.at.y % grid === 0), `the position is a multiple of the grid (${grid})`);
      t(sh.px[0] === 240 && sh.px[1] === 160 && sh.ppi === 300 && (sh.scale_ppm ?? 1000000) === 1000000 && typeof sh.hash === "string" && !("data" in sh), "the state describes the image (pixels, definition, hash) and does not carry them");
      t(E.state().selection.some((x) => x.id === g.id), "the new image is selected");
      const r = await fetch(`/api/sch_image?id=${g.id}&h=${sh.hash}`);
      const got = new Uint8Array(await r.arrayBuffer());
      t(r.headers.get("content-type") === "image/png" && got.length === png.bytes, "GET /api/sch_image gives the PNG");
      await E.run("common.InteractiveSelection.clear");
      await waitFor(() => sheetMagenta() > base + 500);
      t(sheetMagenta() > base + 500, `the sheet paints the picture (${sheetMagenta()} against ${base})`);
      await escape();
      t(E.state().picker == null, "Escape ends the tool");
      const text = (await json("/api/schematic.kicad_sch")).files.map((f) => f.text).join("\n");
      t(/\(image\s+\(at [\d.-]+ [\d.-]+\)[\s\S]*?\(data/.test(text), "the derived sheet has the (image ..) with its data");
      await undo();
      t((await mySchImages()).length === 0, "one undo takes the image away");
      await waitFor(() => sheetMagenta() <= base + 50);
      t(sheetMagenta() <= base + 50, "and the picture with it");
      await redo();
      t((await mySchImages()).length === 1, "one redo brings it back");
      await undo();
      // Escape with an image in the hand drops it; a second one ends the tool
      await E.run("eeschema.InteractiveDrawing.placeImage", png);
      await waitFor(() => /put .* down/i.test(E.state().picker ?? ""));
      await escape();
      t(/choose an image file/i.test(E.state().picker ?? ""), `Escape drops the image and the tool waits (${E.state().picker})`);
      await escape();
      t(E.state().picker == null && (await mySchImages()).length === 0, "the second Escape ends the tool");
    });

    await scenario("Image Properties on the schematic (E): position and scale are one edit, the scale is checked, Convert to Greyscale; the Properties panel; a click picks it, Move and Delete take it", async (t) => {
      await toSchematic();
      await deleteSchImages();
      const png = await makePng(240, 160, MAGENTA);
      const spot = freeSpot(await schematic(), 20320, 13547);
      if (!spot) { t(false, "the sheet has a free spot"); return; }
      const rev0 = E.state().revision;
      await cmd({ op: "sch_edit", verb: "add_graphic", graphic: { shape: { type: "image", at: { x: spot[0], y: spot[1] }, scale_ppm: 1000000, data: png.data } } });
      await waitFor(() => E.state().revision !== rev0, 8000);
      await sleep(500);
      const g = (await mySchImages())[0];
      if (!g) { t(false, "the image was added"); return; }
      // a click on the picture picks it
      await E.run("common.InteractiveSelection.clear");
      await sheetClick(spot[0] + 5000, spot[1] + 3000);
      t(E.state().selection.some((x) => x.id === g.id), `a click in the picture picks it (${JSON.stringify(E.state().selection)})`);
      // E: the dialog
      await E.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(!!tid("sch-image-properties"), "E opens Image Properties");
      t(tid("sim-ppi")?.textContent === "300" && tid("sim-scale")?.value === "1" && Number(tid("sim-x")?.value) === spot[0] / 1000, "with the image's definition, scale and position");
      setValue(tid("sim-scale"), "0.01");
      dialogButton("OK")?.click();
      await sleep(500);
      t(/too small/i.test(tid("sim-error")?.textContent ?? "") && !!tid("sch-image-properties"), `a scale that makes the image under 15 pixels is refused (${tid("sim-error")?.textContent})`);
      setValue(tid("sim-scale"), "0.5");
      setValue(tid("sim-x"), String((spot[0] + 5000) / 1000));
      await sleep(150);
      dialogButton("OK")?.click();
      await sleep(1000);
      const after = (await mySchImages())[0];
      t(after.id === g.id && after.shape.scale_ppm === 500000 && after.shape.at.x === spot[0] + 5000 && after.shape.hash === g.shape.hash, `OK applies the scale and the position and keeps the pixels (${after.shape.scale_ppm}, ${after.shape.at.x})`);
      await undo();
      const back = (await mySchImages())[0];
      t((back.shape.scale_ppm ?? 1000000) === 1000000 && back.shape.at.x === spot[0], "one undo puts both back");
      // Convert to Greyscale
      await E.run("eeschema.InteractiveEdit.properties");
      await sleep(500);
      tid("sim-greyscale").click();
      await waitFor(() => (tid("sim-preview")?.getAttribute("src") ?? "").startsWith("data:image/png;base64,"));
      t((tid("sim-preview")?.getAttribute("src") ?? "").startsWith("data:image/png;base64,"), "Convert to Greyscale shows the converted picture");
      dialogButton("OK")?.click();
      await sleep(1000);
      const grey = (await mySchImages())[0];
      t(grey.shape.hash !== g.shape.hash && grey.shape.px[0] === 240 && grey.shape.ppi === 300, "the pixels changed and nothing else did");
      await undo();
      t((await mySchImages())[0].shape.hash === g.shape.hash, "one undo brings the colours back");
      // Cancel changes nothing
      await E.run("eeschema.InteractiveEdit.properties");
      await sleep(400);
      setValue(tid("sim-scale"), "2");
      dialogButton("Cancel")?.click();
      await sleep(400);
      t(((await mySchImages())[0].shape.scale_ppm ?? 1000000) === 1000000, "Cancel leaves the image as it was");
      // the Properties panel
      await select([g.id]);
      if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); }
      const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
      t(!!row("Scale") && !!row("Width") && !!row("Height") && !!row("Position X"), "the panel has the Position, Scale, Width and Height rows of a SCH_BITMAP");
      const input = row("Width")?.querySelector("[data-prop-input]");
      if (input) {
        setValue(input, "10.16");
        input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        input.dispatchEvent(new Event("blur", { bubbles: true }));
        await sleep(1000);
        const w = (await mySchImages())[0];
        t(w.shape.scale_ppm === 500000 && w.shape.at.x === spot[0], `a width typed in the panel is a scale about the centre (${w.shape.scale_ppm})`);
        await undo();
      } else t(false, "the Width row has an input");
      // Move Exactly, a drag, Delete
      await cmd({ op: "sch_move", verb: "move", ids: [g.id], dx: 2540, dy: 1270 });
      t((await mySchImages())[0].shape.at.x === spot[0] + 2540, "Move takes it by its centre");
      await undo();
      await E.run("common.InteractiveSelection.clear");
      await sheetClick(spot[0] + 5000, spot[1] + 3000);
      await E.run("common.Interactive.delete");
      await sleep(800);
      t((await mySchImages()).length === 0, "Delete removes it");
      await undo();
      t((await mySchImages()).length === 1, "one undo brings it back");
      await deleteSchImages();
    });

    await scenario("a PNG dropped on the sheet is in the hand and one click places it (ddAddImage); a file that is no image is refused; the board takes no image drop", async (t) => {
      await toSchematic();
      await deleteSchImages();
      const png = await makePng(120, 80, MAGENTA, "dropped.png");
      const bytes = Uint8Array.from(atob(png.data), (c) => c.charCodeAt(0));
      const drop = async (name, content, type) => {
        const dt = new DataTransfer();
        dt.items.add(new File([content], name, { type }));
        window.dispatchEvent(new DragEvent("dragenter", { dataTransfer: dt, bubbles: true, cancelable: true }));
        window.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
        await sleep(900);
      };
      await drop("dropped.png", bytes, "image/png");
      t(/put dropped\.png down/i.test(E.state().picker ?? ""), `the dropped file is in the hand (${E.state().picker})`);
      const spot = freeSpot(await schematic(), 10160, 6773);
      await sheetClick(spot[0], spot[1]);
      await waitFor(async () => (await mySchImages()).length === 1);
      const list = await mySchImages();
      t(list.length === 1 && list[0].shape.px[0] === 120, "one click places it");
      t(E.state().picker == null, "and the tool ends (immediate mode: a dropped file is placed once)");
      await undo();
      t((await mySchImages()).length === 0, "one undo");
      await drop("bad.png", new TextEncoder().encode("this is not an image, only text"), "image/png");
      t(/Could not load image from 'bad\.png'/.test(document.body.innerText) && (E.state().picker ?? "") === "", "a file that is no image says so and places nothing");
      await toBoard();
      await drop("dropped.png", bytes, "image/png");
      t(E.state().picker == null && (await myImages()).length === 0, "the board editor takes no image drop (its table has no png)");
    });

    // ------------------------------------------------------------------------------------------------------------------ 3. the point
    await toBoard();
    toScreen = (await screenOf()) ?? toScreen;
    const points = async () => (await board()).drawings?.points ?? [];
    const pointsBefore = new Set((await points()).map((p) => p.id));
    const myPoints = async () => (await points()).filter((p) => !pointsBefore.has(p.id));
    const deletePoints = async () => { for (const p of await myPoints()) await cmd({ op: "pcb_item", verb: "delete_point", id: p.id }); };
    /** Pixels of the colour of the Points object (`LAYER_POINTS`, #ff26e2): the cross of a point. A footprint's anchor is the same colour, so a test compares counts. */
    const crossPixels = () => {
      const c = pcbCanvas();
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      let n = 0;
      for (let i = 0; i < d.length; i += 4) if (d[i] > 200 && d[i + 1] > 20 && d[i + 1] < 90 && d[i + 2] > 180) n++;
      return n;
    };

    await scenario("Place Point: each click puts a point on the active layer as one undo step and the tool repeats until Escape; the points that are there are snapped to", async (t) => {
      await deletePoints();
      toScreen = (await screenOf()) ?? toScreen;
      t((await E.actions()).some((a) => a.id === "pcbnew.InteractiveDrawing.placePoint" && a.enabled !== false), "Place Point is an action of the board editor");
      await E.run("pcbnew.InteractiveDrawing.placePoint");
      await sleep(300);
      t(/Place point/i.test(E.state().picker ?? ""), `the tool is running (${E.state().picker})`);
      await click(toScreen, ...at(8, 8));
      await waitFor(async () => (await myPoints()).length === 1);
      let list = await myPoints();
      t(list.length === 1 && near(list[0].x, (bx0 + 8) * 1000, 1500) && near(list[0].y, (by0 + 8) * 1000, 1500), `the first click puts a point there (${JSON.stringify(list[0])})`);
      t(list[0].size === 1000 && typeof list[0].layer === "string" && list[0].layer.length > 0, `of 1 mm on the active layer (${list[0]?.layer})`);
      t(E.state().picker != null, "the tool goes on (IPO_REPEAT)");
      await click(toScreen, ...at(14, 8));
      await waitFor(async () => (await myPoints()).length === 2);
      t((await myPoints()).length === 2, "a second click puts a second point");
      // the next click lands on the first point's own position even a little off it: the point is an anchor
      const first = list[0];
      const [fx, fy] = [first.x / 1000, first.y / 1000];
      await click(toScreen, fx + 0.06, fy + 0.05);
      await waitFor(async () => (await myPoints()).length === 3);
      const third = (await myPoints()).find((p) => p.id !== first.id && p.id !== (list[1] ?? {}).id) ?? (await myPoints()).at(-1);
      const all = await myPoints();
      t(all.filter((p) => p.x === first.x && p.y === first.y).length === 2, `a click near a point snaps onto it (${JSON.stringify(all.map((p) => [p.x, p.y]))})`);
      await escape();
      t(E.state().picker == null, "Escape ends the tool");
      await undo();
      t((await myPoints()).length === 2, "one undo takes the last point away");
      await undo();
      await undo();
      t((await myPoints()).length === 0, "and one each of the others");
      await redo();
      t((await myPoints()).length === 1, "one redo brings one back");
      await undo();
    });

    await scenario("a point is drawn as an X and a ring, picked by a click on them, listed with a Size and no lock, flipped, hidden by the Points row; the derived board has it", async (t) => {
      await deletePoints();
      toScreen = (await screenOf()) ?? toScreen;
      const base = crossPixels();
      const rev0 = E.state().revision;
      await cmd({ op: "pcb_item", verb: "add_point", point: { layer: "F.SilkS", at: { x: (bx0 + 12) * 1000, y: (by0 + 12) * 1000 }, size_um: 10000 } });
      await waitFor(() => E.state().revision !== rev0, 8000);
      await sleep(500);
      const p = (await myPoints())[0];
      t(!!p && p.size === 10000, "a 10 mm point was added");
      if (!p) return;
      await waitFor(() => crossPixels() > base + 150);
      t(crossPixels() > base + 150, `the board paints its X in the colour of the Points object (${base} -> ${crossPixels()} pixels)`);
      // a click on a bar of the X, off its centre, picks it
      await E.run("common.InteractiveSelection.clear");
      await escape();
      await click(toScreen, p.x / 1000 + 2.5, p.y / 1000 + 2.5);
      t(E.state().selection.some((s) => s.id === p.id && s.kind === "point"), `a click on a bar of the X picks the point (${JSON.stringify(E.state().selection)})`);
      // the Properties panel: a Size row and a lock that cannot be set
      if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); }
      const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
      t(!!row("Size") && !!row("Layer") && !!row("Position X"), "the panel has the Size, Layer and Position rows of a PCB_POINT");
      t(row("Locked")?.classList.contains("readonly") || row("Locked")?.querySelector("[data-prop-input]")?.disabled === true, "and a Locked row that cannot be written (the file form of a point has no lock)");
      const size = row("Size")?.querySelector("[data-prop-input]");
      if (size) {
        setValue(size, "4");
        size.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        size.dispatchEvent(new Event("blur", { bubbles: true }));
        await sleep(900);
        t((await myPoints())[0].size === 4000, "a size typed in the panel is the point's");
        await undo();
        t((await myPoints())[0].size === 10000, "one undo takes it back");
      } else t(false, "the Size row has an input");
      // Flip and Move
      await select([p.id]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(700);
      t((await myPoints())[0].layer === "B.SilkS", `F puts it on the back layer (${(await myPoints())[0].layer})`);
      await undo();
      await drag(toScreen, [p.x / 1000 + 2.5, p.y / 1000 + 2.5], [p.x / 1000 + 4.5, p.y / 1000 + 4.5]);
      const moved = (await myPoints())[0];
      t(near(moved.x - p.x, 2000, 700) && near(moved.y - p.y, 2000, 700), `a drag carries it (${moved.x - p.x}, ${moved.y - p.y})`);
      await undo();
      // `L` does nothing to it: no lock
      await select([p.id]);
      await E.run("pcbnew.EditorControl.toggleLock");
      await sleep(500);
      t(!((await board()).locked ?? []).includes(p.id), "L does not lock a point");
      // the Points row of the Appearance panel
      await E.run("common.InteractiveSelection.clear");
      await sleep(300);
      const shown = crossPixels();
      await E.run("studio.Appearance.op", { op: "object", id: "ly_points", visible: false });
      await sleep(500);
      t(crossPixels() < shown - 100, `Points off hides it (${shown} -> ${crossPixels()})`);
      await E.run("studio.Appearance.op", { op: "object", id: "ly_points", visible: true });
      await sleep(400);
      t(crossPixels() >= shown - 40, `and on shows it again (${crossPixels()} against ${shown})`);
      const text = await fetch("/api/board.kicad_pcb").then((r) => r.text());
      t(/\(point \(at [\d.-]+ [\d.-]+\) \(size 10\) \(layer "F\.SilkS"\)/.test(text), "the derived board has the (point ..) with its size and layer");
      await select([p.id]);
      await E.run("common.Interactive.delete");
      await sleep(700);
      t((await myPoints()).length === 0, "Delete removes it");
      await undo();
      t((await myPoints()).length === 1, "one undo brings it back");
      await deletePoints();
    });

    await toBoard();
    await deleteImages();
    await deleteSchImages();
    await deleteAll();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
