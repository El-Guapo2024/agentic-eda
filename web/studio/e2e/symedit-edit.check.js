// The Symbol Editor's editing (UI-LEFT Y1-Y5, PARITY-symedit "Rotate, Mirror, beziers, the Properties pane and the carried items"): Rotate and Mirror of a mixed
// selection (a pin, a rectangle, a text and a field) about KiCad's pivot, the bezier tool and its point editor, the pin tool's TwoClickPlace (Pin Properties first,
// then the pin follows the cursor until the click), Text Properties of a plain text, a pin row of the Properties pane, Copy and Paste with the items following the
// cursor (and a paste of KiCad's own clipboard text), and Save into a library of the project table -- whose file kicad-cli then loads (`kicad-cli sym export svg`,
// run after the check on the path its last note gives).
//
// Run it in the page of a served board (`e2e/run-check.mjs`) on a COPY of a board (`eda board new examples/ldo.yaml -o <dir>`, then `eda board serve -C <dir> --port N
// --ui web/studio/dist`). It makes a library of its own in the board's folder (nickname starting with `Sy`) and leaves it there with its file. It answers through
// `window.__symeditCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms), file }.
// run-all-checks: board=ldo minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__symeditCheck = { results, done: false, ok: false, wait: null, file: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2), file: check.file };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (el, value) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(value));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(80);
  };
  const until = async (cond, ms = 8000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await cond(); if (v) return v; await sleep(step); } return null; };
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await until(() => E.state().tab === label.toLowerCase(), 4000); await sleep(600); };
  const dialog = (label) => document.querySelector(`[role=dialog][aria-label="${label}"]`);
  const button = (scope, label) => [...(scope?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === label);
  const treeRow = (id) => document.querySelector(`[role=treeitem][data-name="${CSS.escape(id)}"]`);
  const libRow = (lib) => [...document.querySelectorAll(".library-tree-group")].find((g) => g.textContent.replace(/[▾▸]/g, "").trim().replace(/\s*\*?\s*[\d…]*$/, "").replace(/^☆ /, "") === lib);
  const isOpen = (lib) => libRow(lib)?.textContent.trim().startsWith("▾");
  const symbol = (id) => json(`/api/symbol?lib_id=${encodeURIComponent(id)}`);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].reverse().find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (E.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (E.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const selectLibrary = async (lib) => {
    const r = await until(() => libRow(lib), 6000);
    if (!r) return false;
    r.click();
    await sleep(250);
    if (!isOpen(lib)) libRow(lib)?.click();
    await sleep(250);
    return true;
  };
  const itemRow = async (id) => {
    const lib = id.split(":")[0];
    if (await until(() => libRow(lib), 6000)) if (!isOpen(lib)) { libRow(lib).click(); await sleep(250); }
    return until(() => treeRow(id), 6000);
  };
  const select = async (ids) => { await E.run("common.Interactive.unselectAll"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const run = async (id) => { const r = await E.run(id); await sleep(300); return r; };

  // the canvas, in the editor's nm (y down)
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = E.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (kind, wx, wy, buttons = 0) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(kind, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse" }));
  };
  const hover = async (wx, wy) => { fire("pointermove", wx, wy); await sleep(150); };
  const clickAt = async (wx, wy, withHover = true) => { if (withHover) await hover(wx, wy); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy); await sleep(500); };
  const dragFromTo = async (from, to) => {
    fire("pointermove", from[0], from[1]); await sleep(60);
    fire("pointerdown", from[0], from[1], 1); await sleep(60);
    fire("pointermove", from[0] + 40 / E.state().view.scale, from[1] + 40 / E.state().view.scale, 1); await sleep(60);
    fire("pointermove", to[0], to[1], 1); await sleep(100);
    fire("pointerup", to[0], to[1]); await sleep(800);
  };
  const dbl = async (wx, wy) => { const [sx, sy] = toScreen(wx, wy); canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: sx, clientY: sy, detail: 2 })); await sleep(600); };
  /** The pixels near an editor point that are not the background's: the ink of what is drawn there. */
  const inkAt = (wx, wy, rPx = 6) => {
    const c = document.querySelector(".editor-canvas-col canvas"); const ctx = c.getContext("2d"); const dpr = c.width / parseFloat(c.style.width || c.width); const rect = c.getBoundingClientRect();
    const [sx, sy] = toScreen(wx, wy);
    const px = Math.max(0, Math.round((sx - rect.left - rPx) * dpr)), py = Math.max(0, Math.round((sy - rect.top - rPx) * dpr));
    const d = ctx.getImageData(px, py, Math.round(2 * rPx * dpr), Math.round(2 * rPx * dpr)).data;
    let n = 0; for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - 26) + Math.abs(d[i + 1] - 26) + Math.abs(d[i + 2] - 26) > 90) n++;
    return n;
  };
  const near = (a, b, tol = 1e-4) => Math.abs(a - b) <= tol;
  const at = (p, x, y) => !!p && near(p.x, x) && near(p.y, y);
  const fmt = (p) => (p ? `(${p.x}, ${p.y})` : "none");

  const s = Date.now().toString(36);
  const LIB = `Sy${s}Lib`; // (a nickname that ends in a digit reads as a count in the tree row)
  const ID = `${LIB}:SyU`;
  let ids = {};
  const items = async () => {
    const sym = await symbol(ID);
    return {
      sym,
      pin: sym.pins.find((p) => p.id === ids.pin),
      rect: sym.graphics.find((g) => g.id === ids.rect),
      text: sym.graphics.find((g) => g.id === ids.text),
      value: (sym.fields ?? []).find((f) => f.name === "Value"),
    };
  };

  // ------------------------------------------------------------------------------------------------------------------------ the symbol
  await scenario("a library of the project table and a symbol in it: a pin, a rectangle and a left-top text, made by the editor's verbs", async (t) => {
    await clickTab("Symbol");
    await E.run("common.Control.newLibrary");
    const d = await until(() => dialog("New Symbol Library"), 4000);
    if (!t(d, "New Symbol Library opens")) return;
    await until(() => d.querySelector("#lt-folder")?.value, 4000);
    await type(d.querySelector("#lt-name"), LIB);
    button(d, "Create").click();
    t(await until(() => !dialog("New Symbol Library"), 6000), `${LIB} made`);
    t(await selectLibrary(LIB), "the library is in the tree");
    await run("eeschema.SymbolLibraryControl.newSymbol");
    const nd = await until(() => tid("new-symbol"), 3000);
    if (!t(nd, "New Symbol opens")) return;
    await type(tid("new-symbol-name"), "SyU");
    tid("new-symbol-ok").click();
    t(await until(async () => (await json("/api/symbol_editor/names")).project.includes(ID), 5000), `the new symbol is ${ID}`);
    for (const c of [
      { op: "add_symbol_pin", lib_id: ID, pin: { number: "1", name: "IN", electrical_type: "input", shape: "line", at: { x: -7.62, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false, name_size_mm: null, number_size_mm: null } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "rectangle", unit: 0, body_style: 1, start: { x: -5.08, y: 2.54 }, end: { x: 5.08, y: -2.54 }, stroke_mm: 0.254, fill: "background" } },
      { op: "add_symbol_graphic", lib_id: ID, graphic: { kind: "text", unit: 0, body_style: 1, text: "T", at: { x: 0, y: 3.81 }, angle_deg: 0, size_mm: 1.27, h_align: "left", v_align: "top" } },
    ]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op}: ${r.message ?? "ok"}`);
    }
    const sym = await symbol(ID);
    ids = { pin: sym.pins[0]?.id, rect: sym.graphics.find((g) => g.kind === "rectangle")?.id, text: sym.graphics.find((g) => g.kind === "text")?.id, value: "fld:lib:Value" };
    t(ids.pin && ids.rect && ids.text, `ids: ${JSON.stringify(ids)}`);
    await (await itemRow(ID))?.click();
    await sleep(200);
    await run("eeschema.SymbolLibraryControl.editSymbol");
    await sleep(1000);
    await run("common.Control.zoomFitScreen");
    await sleep(600);
    t(E.state().selection.length === 0 && E.state().tab === "symbol", "the symbol is open on the Symbol tab");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y1, Y2: Rotate and Mirror
  await scenario("Rotate turns every selected item -- pin, rectangle, text and field -- about the selection's half-grid centre; one undo step", async (t) => {
    await select([ids.pin, ids.rect, ids.text, ids.value]);
    t(E.state().selection.length === 4, `four selected: ${JSON.stringify(E.state().selection)}`);
    // the pivot: the merged box of the pin (its line and texts), the rectangle and the field, texts left out -- centre (-1.31, 0) mm, on the half grid (-1.27, 0)
    await run("eeschema.InteractiveEdit.rotateCCW");
    await sleep(500);
    let { pin, rect, text, value } = await items();
    t(at(pin.at, -1.27, -6.35) && pin.angle_deg === 90, `the pin turns about (-1.27, 0) and points up: ${fmt(pin.at)} ${pin.angle_deg}`);
    t(at(rect.start, -3.81, -3.81) && at(rect.end, 1.27, 6.35), `the rectangle turns: ${fmt(rect.start)} ${fmt(rect.end)}`);
    t(at(text.at, -5.08, 1.27) && text.angle_deg === 90 && text.h_align === "left" && text.v_align === "top", `the text turns and runs up, its justification kept (Rotate90( false ) of a horizontal text): ${fmt(text.at)} ${text.angle_deg} ${text.h_align}/${text.v_align}`);
    t(at(value.at, -1.27, 1.27) && value.angle_deg === 90, `the Value field turns: ${fmt(value.at)} ${value.angle_deg}`);
    await undo();
    ({ pin, rect, text, value } = await items());
    t(at(pin.at, -7.62, 0) && pin.angle_deg === 0 && at(rect.start, -5.08, 2.54) && at(text.at, 0, 3.81) && text.angle_deg === 0 && at(value.at, 0, 0), "one undo puts all four back");
    // a lone pin turns about its own position (`item->GetPosition()`)
    await select([ids.pin]);
    await run("eeschema.InteractiveEdit.rotateCW");
    ({ pin } = await items());
    t(at(pin.at, -7.62, 0) && pin.angle_deg === 270, `a lone pin turns in place, clockwise to Down: ${fmt(pin.at)} ${pin.angle_deg}`);
    await undo();
  });

  await scenario("Mirror Horizontally and Vertically mirror the mixed selection about the same pivot: Right/Left, Up/Down, the text's justification across the mirror", async (t) => {
    await select([ids.pin, ids.rect, ids.text, ids.value]);
    await run("eeschema.InteractiveEdit.mirrorH");
    await sleep(400);
    let { pin, rect, text, value } = await items();
    t(at(pin.at, 5.08, 0) && pin.angle_deg === 180, `mirrorH: the pin is mirrored about x = -1.27 and points left: ${fmt(pin.at)} ${pin.angle_deg}`);
    t(at(rect.start, 2.54, 2.54) && at(rect.end, -7.62, -2.54), `the rectangle: ${fmt(rect.start)} ${fmt(rect.end)}`);
    t(at(text.at, -2.54, 3.81) && text.h_align === "right" && text.v_align === "top", `the horizontal text's horizontal justification flips: ${fmt(text.at)} ${text.h_align}/${text.v_align}`);
    t(at(value.at, -2.54, 0), `the field's position mirrors: ${fmt(value.at)}`);
    await undo();
    await select([ids.pin, ids.rect, ids.text, ids.value]);
    await run("eeschema.InteractiveEdit.mirrorV");
    await sleep(400);
    ({ pin, rect, text, value } = await items());
    t(at(pin.at, -7.62, 0) && pin.angle_deg === 0, `mirrorV: a pin pointing right keeps its orientation: ${fmt(pin.at)} ${pin.angle_deg}`);
    t(at(rect.start, -5.08, -2.54) && at(rect.end, 5.08, 2.54), `the rectangle flips about y = 0: ${fmt(rect.start)} ${fmt(rect.end)}`);
    t(at(text.at, 0, -3.81) && text.h_align === "left" && text.v_align === "bottom", `the horizontal text's vertical justification flips: ${fmt(text.at)} ${text.h_align}/${text.v_align}`);
    await undo();
    ({ text } = await items());
    t(at(text.at, 0, 3.81) && text.v_align === "top", "one undo");
    t(E.actions().some((a) => a.id === "eeschema.InteractiveEdit.mirrorH" && a.enabled) && E.actions().some((a) => a.id === "eeschema.InteractiveDrawing.drawBezier" && a.enabled), "Mirror and Draw Bezier Curve are live actions of the Symbol tab");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y2: the bezier
  await scenario("Draw Bezier Curve: start, end, then the two control points; the curve is drawn and its point editor moves a control point", async (t) => {
    const before = (await symbol(ID)).graphics.length;
    await run("eeschema.InteractiveDrawing.drawBezier");
    t(E.state().tool === "draw_bezier", `the tool is armed: ${E.state().tool}`);
    const S = [-5_080_000, 7_620_000], End = [5_080_000, 7_620_000], C1 = [-2_540_000, 10_160_000], C2 = [2_540_000, 10_160_000];
    await clickAt(...S, false);
    await clickAt(...End, false);
    await clickAt(...C1, false);
    t((await symbol(ID)).graphics.length === before, "three clicks do not end it");
    await clickAt(...C2, false);
    const sym = await symbol(ID);
    const bez = sym.graphics.find((g) => g.kind === "bezier");
    t(sym.graphics.length === before + 1 && bez, `the fourth click adds it: ${JSON.stringify(bez)}`);
    if (!bez) return;
    t(at(bez.start, -5.08, -7.62) && at(bez.end, 5.08, -7.62) && at(bez.c1, -2.54, -10.16) && at(bez.c2, 2.54, -10.16), `the points in the clicks' order (Y up): ${fmt(bez.start)} ${fmt(bez.c1)} ${fmt(bez.c2)} ${fmt(bez.end)}`);
    ids.bezier = bez.id;
    await run("common.InteractiveSelection.selectionTool");
    await sleep(300);
    // the curve at t = 0.5: (0, 9.525) mm down the sheet
    t(inkAt(0, 9_525_000, 4) > 3, `the curve is drawn (${inkAt(0, 9_525_000, 4)} px)`);
    await clickAt(0, 9_525_000, true);
    t(JSON.stringify(E.state().selection.map((x) => x.id)) === JSON.stringify([bez.id]), `a click on the curve picks it: ${JSON.stringify(E.state().selection)}`);
    await sleep(300);
    t(inkAt(...C1, 7) > 3, `the point editor draws a handle at the first control point (${inkAt(...C1, 7)} px)`); // a square of 10 px round the point
    await dragFromTo(C1, [-3_810_000, 11_430_000]);
    const moved = (await symbol(ID)).graphics.find((g) => g.id === bez.id);
    t(at(moved.c1, -3.81, -11.43) && at(moved.start, -5.08, -7.62) && at(moved.c2, 2.54, -10.16), `a drag of the handle moves control 1 only: ${fmt(moved.c1)}`);
    await undo();
    t(at((await symbol(ID)).graphics.find((g) => g.id === bez.id).c1, -2.54, -10.16), "one undo takes the point back");
    const text = await (await fetch(`/api/symbol/export?lib_id=${encodeURIComponent(ID)}`)).text();
    t(text.includes("(bezier (pts (xy -5.08 -7.62) (xy -2.54 -10.16) (xy 2.54 -10.16) (xy 5.08 -7.62))"), "the exported .kicad_sym writes it as KiCad's formatBezier does");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y5: the pin tool
  await scenario("the Pin tool: the click opens Pin Properties first, then the pin follows the cursor and the next click places it there", async (t) => {
    const count = async () => (await symbol(ID)).pins.length;
    const n0 = await count();
    await run("eeschema.SymbolDrawing.placeSymbolPin");
    t(E.state().tool === "pin", `the Pin tool: ${E.state().tool}`);
    await clickAt(7_620_000, -5_080_000);
    t(await until(() => tid("pin-props-ok"), 3000), "the click opens Pin Properties");
    t((await count()) === n0, "nothing is added yet");
    tid("pin-props-ok").click();
    await sleep(500);
    t(!tid("pin-props-ok") && (await count()) === n0, "OK closes the dialog and still adds nothing: the pin is carried");
    await hover(12_700_000, 2_540_000);
    await sleep(300);
    t(inkAt(12_700_000 + 1_270_000, 2_540_000, 4) > 3, `the carried pin is drawn under the cursor (${inkAt(12_700_000 + 1_270_000, 2_540_000, 4)} px)`);
    await clickAt(12_700_000, 2_540_000);
    const sym = await symbol(ID);
    t(sym.pins.length === n0 + 1, `the click places it: ${sym.pins.length} pins`);
    const placed = sym.pins.find((p) => at(p.at, 12.7, -2.54));
    t(!!placed, `where the cursor took it, not the first click: ${JSON.stringify(sym.pins.map((p) => p.at))}`);
    ids.ghostPin = placed?.id;
    t(E.state().tool === "pin", "the tool stays armed for the next pin");
    canvasEl().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(200);
    await run("common.InteractiveSelection.selectionTool");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y4: Text Properties
  await scenario("Text Properties of a plain text: a double click opens it on the text's looks; OK is one undo step", async (t) => {
    await run("common.InteractiveSelection.selectionTool");
    await dbl(0, -3_810_000);
    const d = await until(() => tid("text-properties"), 3000);
    if (!t(d, "a double click on the text opens Text Properties")) return;
    t(tid("text-text").value === "T" && tid("text-h-left").checked && tid("text-v-top").checked && tid("text-horizontal").checked && tid("text-size").value === "1.27", "it shows the text, left, top, horizontal, 1.27 mm");
    await type(tid("text-text"), "TT");
    tid("text-bold").click(); await sleep(60);
    tid("text-h-right").click(); await sleep(60);
    await type(tid("text-size"), "2");
    tid("text-ok").click();
    await sleep(800);
    let { text } = await items();
    t(!tid("text-properties") && text.text === "TT" && text.bold === true && text.h_align === "right" && text.v_align === "top" && text.size_mm === 2, `OK: ${JSON.stringify(text)}`);
    t(at(text.at, 0, 3.81) && text.id === ids.text, "the same text, where it was");
    await undo();
    ({ text } = await items());
    t(text.text === "T" && !text.bold && text.h_align === "left", "one undo");
    // E opens it too
    await select([ids.text]);
    await run("eeschema.InteractiveEdit.properties");
    t(await until(() => tid("text-properties"), 2000), "E opens it on the selected text");
    await type(tid("text-text"), "TX");
    tid("text-ok").click();
    await sleep(700);
    t((await items()).text.text === "TX", "and its OK edits it");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y3: the pane
  await scenario("the Properties pane: a pin's rows, an edit of Pin Number and Orientation (one undo each), the symbol's rows with nothing selected", async (t) => {
    const pane = () => tid("sym-properties");
    const row = (name) => pane()?.querySelector(`.prop-row[data-prop="${name}"]`);
    const input = (name) => row(name)?.querySelector("[data-prop-input]");
    await select([]);
    t(await until(() => row("Keywords") && row("Define as Power Symbol") && row("Number of Symbol Units"), 3000), "with nothing selected the pane shows the symbol (Fields, Symbol Definition, Units and Body Styles)");
    await select([ids.pin]);
    t(await until(() => row("Pin Number") && row("Electrical Type") && row("Orientation") && row("Length"), 3000), `a pin's rows: ${[...(pane()?.querySelectorAll(".prop-row") ?? [])].map((r) => r.dataset.prop).join(", ")}`);
    t(!row("Locked"), "no Locked row in a library editor");
    t(input("Position Y")?.value === "0" || (input("Position Y")?.value ?? "").startsWith("0"), `Position Y is KiCad's internal Y: ${input("Position Y")?.value}`);
    const num = input("Pin Number");
    num.focus();
    await type(num, "42");
    num.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    t(await until(async () => (await items()).pin.number === "42", 4000), "Pin Number edits the pin");
    await type(input("Orientation"), "90");
    t(await until(async () => (await items()).pin.angle_deg === 90, 4000), "Orientation: Up");
    await undo();
    t((await items()).pin.angle_deg === 0 && (await items()).pin.number === "42", "one undo takes the orientation back, not the number");
  });

  // ------------------------------------------------------------------------------------------------------------------------ Y5: the clipboard
  await scenario("Copy puts .kicad_sym text on the clipboard; Paste carries the items from the cursor until a click; a paste of KiCad's own text works too", async (t) => {
    await select([ids.pin, ids.rect]);
    const text = (await post("/api/symbol/clipboard/copy", { lib_id: ID, ids: [ids.pin, ids.rect] })).text ?? "";
    t(text.includes("(symbol \"SyU\"") && text.includes("(pin input line") && text.includes("(rectangle") && !text.includes("(text \"T"), "the clipboard text is the symbol with only the pin and the rectangle");
    await run("common.Interactive.copy");
    await hover(0, 0);
    const before = await symbol(ID);
    await run("common.Interactive.paste");
    await sleep(300);
    t((await symbol(ID)).pins.length === before.pins.length, "nothing is added while the items are carried");
    await hover(2_540_000, 0);
    await clickAt(2_540_000, 0);
    let after = await symbol(ID);
    t(after.pins.length === before.pins.length + 1 && after.graphics.length === before.graphics.length + 1, `the click adds them: ${after.pins.length} pins, ${after.graphics.length} graphics`);
    const newPin = after.pins.find((p) => !before.pins.some((q) => q.id === p.id));
    const newRect = after.graphics.find((g) => !before.graphics.some((q) => q.id === g.id));
    t(newPin && at(newPin.at, -7.62 + 2.54, 0) && newRect && at(newRect.start, -5.08 + 2.54, 2.54), `moved as far as the cursor went: ${fmt(newPin?.at)} ${fmt(newRect?.start)}`);
    t(E.state().selection.length === 2, `the pasted items are the selection: ${JSON.stringify(E.state().selection)}`);
    // KiCad's clipboard: a `(symbol ..)` form with a bezier and a text (the browser's clipboard is the page's own here)
    const kicad = `(symbol "FromKiCad" (property "Reference" "U" (at 0 0 0) (effects (font (size 1.27 1.27))))
  (symbol "FromKiCad_0_1" (bezier (pts (xy 0 0) (xy 1.27 2.54) (xy 2.54 2.54) (xy 3.81 0)) (stroke (width 0) (type default)) (fill (type none)))
    (text "K" (at 0 -2.54 0) (effects (font (size 1.27 1.27)) (justify left)))))`;
    const realRead = navigator.clipboard?.readText?.bind(navigator.clipboard);
    try { Object.defineProperty(navigator.clipboard, "readText", { configurable: true, value: async () => kicad }); } catch { /* no clipboard object */ }
    await hover(0, 0);
    await run("common.Interactive.paste");
    await hover(0, -12_700_000);
    await clickAt(0, -12_700_000);
    after = await symbol(ID);
    const k = after.graphics.find((g) => g.kind === "text" && g.text === "K");
    const kb = after.graphics.filter((g) => g.kind === "bezier");
    t(k && k.h_align === "left" && at(k.at, 0, 12.7 - 2.54) && kb.length === 2, `KiCad's text pasted: the text, left-justified, and the bezier, moved with the cursor: ${JSON.stringify(k)} ${kb.length} curves`);
    if (realRead) try { Object.defineProperty(navigator.clipboard, "readText", { configurable: true, value: realRead }); } catch { /* */ }
    await undo();
    t((await symbol(ID)).graphics.filter((g) => g.kind === "bezier").length === 1, "one undo takes that paste away");
  }, [/clipboard/i]);

  // ------------------------------------------------------------------------------------------------------------------------ Save
  await scenario("Save writes the symbol into its library's file (kicad-cli then loads it)", async (t) => {
    await (await itemRow(ID))?.click();
    await sleep(300);
    const r = await run("common.Control.save");
    t(r.ok && /Symbol library file '.*\.kicad_sym' saved\./.test(r.toast?.message ?? r.toast ?? ""), `Save: ${JSON.stringify(r).slice(0, 200)}`);
    const saved = (await json(`/api/library/saved?kind=symbol&id=${encodeURIComponent(ID)}`)).symbol;
    t(saved && saved.graphics.some((g) => g.kind === "bezier") && saved.pins.some((p) => p.number === "42") && saved.graphics.some((g) => g.kind === "text" && g.text === "TX"), `the file holds the bezier, the pin 42 and the text TX: ${saved && saved.graphics.map((g) => g.kind).join(",")}`);
    const row = (await json("/api/library_tables")).tables.symbol.project.rows.find((x) => x.nickname === LIB);
    check.file = row?.uri;
    t(!!row, `the file: ${row?.uri} (kicad-cli sym export svg runs on it after the check)`);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
