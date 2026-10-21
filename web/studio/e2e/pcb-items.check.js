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
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
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
      document.querySelector('button[aria-label="Show Properties"]')?.click();
      await sleep(500);
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

    await deleteAll();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
