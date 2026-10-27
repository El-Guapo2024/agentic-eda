// Footprint Editor drawing tools check (round 4 findings E4-F1, E4-F2, E4-F3; PARITY-fpedit.md): a drawn shape takes the line width of its layer's class and a polygon on a
// courtyard is unfilled (`drawing_tool.cpp` `GetLineThickness`, `zone_create_helper.cpp`); the Text tool opens "Footprint Text Properties" on its first click, carries the text to
// the next click, places it left justified, mirrored on a back layer, in the layer's size and pen, and stays armed (`DRAWING_TOOL::PlaceText`); the Pad tool carries its pad, and R
// and F before the click turn and flip it (`PAD_TOOL::PlacePad`, `doInteractiveItemPlacement` with IPO_ROTATE | IPO_FLIP), the next pad taking the same turn.
//
// Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board
// (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, then `eda board serve -C <dir> --port N --ui web/studio/dist`). It makes footprints of its own (New Footprint), and
// deletes them at the end, leaving the project library as it found it. It answers through `window.__fpToolsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __fpToolsCheck.wait(60000)` until `.done`.
// run-all-checks: board=mcu30 minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__fpToolsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(120); };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  /** Ctrl+Shift: neither the grid nor an anchor moves the point (a click lands where it is sent). */
  const fire = (type, wx, wy, buttons = 0) => {
    const [x, y] = toScreen(wx, wy);
    const mac = /Mac/.test(navigator.platform);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !mac, metaKey: mac, shiftKey: true }));
  };
  const clickAt = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(70); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(500); };
  const key = async (k) => { canvasEl().focus(); canvasEl().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(250); };
  const arm = async (action, tool) => { if (__eda.state().tool !== tool) { await __eda.run(action); await sleep(250); } };
  /** The Footprint Editor's layer box (its top toolbar). */
  const setLayer = async (layer) => {
    const sel = [...document.querySelectorAll('.toolbar[data-editor="footprint"] .toolbar-control select')].find((s) => [...s.options].some((o) => o.value === "F.CrtYd"));
    if (!sel) return false;
    await choose(sel, layer);
    return sel.value === layer;
  };
  const made = [];
  const newFootprint = async () => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !before.includes(n));
    made.push(name);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(400);
    return name;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const libraryBefore = await fpNames();

  await scenario("E4-F1: a drawn shape takes its layer class's width; a polygon on a courtyard is unfilled, on the silkscreen filled", async (t) => {
    const name = await newFootprint();
    t(!!name, `a new footprint: ${name}`);
    const before = (await footprint(name)).graphics.length;
    t(await setLayer("F.CrtYd"), "the layer box offers F.CrtYd");
    await arm("pcbnew.InteractiveDrawing.rectangle", "draw_rect");
    await clickAt(-2_000_000, -2_000_000);
    await clickAt(2_000_000, 2_000_000);
    let g = (await footprint(name)).graphics.slice(before);
    t(g.length === 1 && g[0].kind === "rect" && g[0].stroke_width === 50_000, `the courtyard rectangle is 0.05 mm: ${JSON.stringify(g.map((x) => [x.kind, x.layer, x.stroke_width]))}`);
    await arm("pcbnew.InteractiveDrawing.graphicPolygon", "draw_polygon");
    await clickAt(-1_000_000, -1_000_000);
    await clickAt(1_000_000, -1_000_000);
    await clickAt(0, 1_000_000);
    await key("Enter");
    await sleep(500);
    g = (await footprint(name)).graphics.slice(before);
    const poly = g.find((x) => x.kind === "polygon");
    t(!!poly && poly.filled === false && poly.stroke_width === 50_000, `the courtyard polygon is unfilled, 0.05 mm: ${JSON.stringify(poly && [poly.filled, poly.stroke_width])}`);
    await key("Escape");
    t(await setLayer("F.SilkS"), "the layer box back to F.SilkS");
    await arm("pcbnew.InteractiveDrawing.graphicPolygon", "draw_polygon");
    await clickAt(3_000_000, -1_000_000);
    await clickAt(4_000_000, -1_000_000);
    await clickAt(3_500_000, 1_000_000);
    await key("Enter");
    await sleep(500);
    const silk = (await footprint(name)).graphics.slice(before).find((x) => x.kind === "polygon" && x.layer === "F.SilkS");
    t(!!silk && silk.filled === true && silk.stroke_width === 100_000, `the silkscreen polygon is filled, 0.1 mm: ${JSON.stringify(silk && [silk.filled, silk.stroke_width])}`);
    await key("Escape");
    t(await setLayer("Edge.Cuts"), "the layer box offers Edge.Cuts");
    await arm("pcbnew.InteractiveDrawing.line", "draw_segment");
    await clickAt(-3_000_000, 3_000_000);
    await clickAt(3_000_000, 3_000_000);
    const edge = (await footprint(name)).graphics.find((x) => x.layer === "Edge.Cuts");
    t(!!edge && edge.stroke_width === 50_000, `the Edge.Cuts line is 0.05 mm: ${edge?.stroke_width}`);
    await key("Escape");
    await setLayer("F.SilkS");
  });

  await scenario("E4-F2: the Text tool opens Footprint Text Properties, carries the text, places it with the layer's defaults (mirrored on a back layer) and stays armed", async (t) => {
    const name = await newFootprint();
    const before = (await footprint(name)).texts.length;
    t(await setLayer("B.SilkS"), "the layer box offers B.SilkS");
    await arm("pcbnew.InteractiveDrawing.text", "text");
    await clickAt(1_000_000, 1_000_000);
    const dlg = tid("fp-text-dialog");
    t(!!dlg && /Footprint Text Properties/.test(dlg.textContent), "the first click opens Footprint Text Properties");
    t(tid("fp-text-size")?.value === "1" && tid("fp-text-thickness")?.value === "0.1" && tid("fp-text-justify")?.value === "left" && tid("fp-text-mirror")?.checked === true, `it starts with B.SilkS's 1 mm / 0.1 mm, left, mirrored: ${[tid("fp-text-size")?.value, tid("fp-text-thickness")?.value, tid("fp-text-justify")?.value, tid("fp-text-mirror")?.checked]}`);
    await type(tid("fp-text-content"), "R4TXT");
    tid("fp-text-ok").click();
    await sleep(400);
    t((await footprint(name)).texts.length === before, "OK does not place it yet: it follows the cursor");
    await clickAt(2_000_000, 1_500_000);
    await sleep(400);
    const texts = (await footprint(name)).texts.slice(before);
    const tx = texts[0];
    t(texts.length === 1 && tx.content === "R4TXT" && tx.layer === "B.SilkS", `the next click places it: ${JSON.stringify(texts.map((x) => [x.content, x.layer]))}`);
    t(!!tx && tx.at.x === 2_000_000 && tx.at.y === 1_500_000, `where the click was: ${JSON.stringify(tx?.at)}`);
    t(!!tx && tx.size_nm === 1_000_000 && tx.stroke_width === 100_000 && tx.justify === "left" && tx.mirror === true, `1 mm, 0.1 mm pen, left, mirrored: ${JSON.stringify(tx && [tx.size_nm, tx.stroke_width, tx.justify, tx.mirror])}`);
    t(__eda.state().tool === "text", `the Text tool stays armed: ${__eda.state().tool}`);
    // an empty text is dropped
    await clickAt(0, 2_500_000);
    t(!!tid("fp-text-dialog"), "another click opens the dialog again");
    tid("fp-text-ok").click();
    await sleep(300);
    await clickAt(0, 3_000_000);
    await closeDialogs();
    t((await footprint(name)).texts.length === before + 1, "an empty text is not placed");
    // Escape with a text carried drops it and keeps the tool; a second Escape leaves it
    await clickAt(0, 2_500_000);
    await type(tid("fp-text-content"), "DROP");
    tid("fp-text-ok").click();
    await sleep(300);
    await key("Escape");
    t(__eda.state().tool === "text", "Escape drops the carried text and keeps the tool");
    await key("Escape");
    t(__eda.state().tool === "select", "a second Escape leaves the tool");
    t((await footprint(name)).texts.length === before + 1, "the dropped text was never placed");
    await setLayer("F.SilkS");
  });

  await scenario("E4-F3: R and F before the click turn and flip the carried pad, and the next pad takes the same turn", async (t) => {
    const name = await newFootprint();
    await arm("pcbnew.PadTool.placePad", "pad");
    t(__eda.state().tool === "pad", "the pad tool is armed");
    fire("pointermove", 0, 0);
    await sleep(100);
    await __eda.run("pcbnew.InteractiveEdit.rotateCcw");
    await key("f");
    await clickAt(0, 0);
    let pads = (await footprint(name)).pads;
    const p1 = pads[pads.length - 1];
    t(!!p1 && p1.layers.includes("B.Cu") && !p1.layers.includes("F.Cu"), `F flipped the pad to the back: ${JSON.stringify(p1?.layers)}`);
    t(!!p1 && p1.rot === 270_000, `R turned it 90 degrees and the flip negated it: ${p1?.rot}`);
    t(__eda.state().tool === "pad", "the pad tool stays armed");
    await clickAt(3_000_000, 0);
    pads = (await footprint(name)).pads;
    const p2 = pads[pads.length - 1];
    t(pads.length === 2 && p2.rot === p1.rot && p2.layers.includes("B.Cu"), `the next pad takes the same turn and side (the default pad took them): ${JSON.stringify(p2 && [p2.rot, p2.layers])}`);
    await key("Escape");
    // put the default pad back as it was for the other checks: a fresh F.Cu pad
    await arm("pcbnew.PadTool.placePad", "pad");
    await key("f");
    await __eda.run("pcbnew.InteractiveEdit.rotateCw");
    await clickAt(-3_000_000, 0);
    await key("Escape");
  });

  // ================================================================================================ leave the library as it was
  await closeDialogs();
  for (const n of made) if (n) await cmd({ op: "delete_library_footprint", name: n });
  await sleep(500);
  const libraryAfter = await fpNames();
  const clean = JSON.stringify(libraryAfter) === JSON.stringify(libraryBefore);
  results.push({ name: "the project library is left as it was found", ok: clean, notes: [clean ? "ok: same names" : `FAIL: ${JSON.stringify({ before: libraryBefore, after: libraryAfter })}`] });

  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
