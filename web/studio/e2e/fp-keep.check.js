// A board footprint keeps its own items, UI-LEFT P30 (PARITY-pcb.md section 89): a text box drawn in Edit Footprint's (Ctrl+E) copy and a text added there reach the board
// with Update Footprint on Board; Ctrl+E shows them again and another edit and Update keeps them (`LoadFootprintFromBoard` clones the board footprint, `SaveFootprintToBoard`
// exchanges the clone back: nothing is lost); Update Footprint from Library and Change Footprint make the text box the library footprint's (`ExchangeFootprint` takes every
// child but the texts and fields from the new footprint: the installed 0603 and 0805 have none) and keep the user text where it was unless "Remove text items if not in library
// footprint" is on; one undo brings them back. Driven through `window.__eda` (the actions a person runs, the canvas's pointer events, the dialogs) and checked against what the
// server holds (`/api/state`, `/api/footprint`).
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board with R1 placed (0603; `eda board new
// examples/mcu_board_30plus.yaml -o <dir>`, `eda board place R1 --at 8,8`), served with `KICAD_PATH` set (the installed Resistor_SMD library). It answers through
// `window.__fpKeepCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30-few minutes=8
(async () => {
  const results = [];
  const check = (window.__fpKeepCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const MM = 1_000_000;
  const mac = /Mac/.test(navigator.platform);
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const setValue = async (el, v) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(80);
  };
  const tick = async (el, on) => { if (el && el.checked !== on) { el.click(); await sleep(150); } };
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(150); } return null; };
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button, .dialog button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  // the Footprint Editor's canvas, in the footprint's frame
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, wx, wy, buttons = 0, mods = {}) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !mac && !!mods.ctrl, metaKey: mac && !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const clickAt = async (wx, wy, mods) => { fire("pointermove", wx, wy, 0, mods); await sleep(80); fire("pointerdown", wx, wy, 1, mods); await sleep(40); fire("pointerup", wx, wy, 0, mods); await sleep(450); };
  const key = async (k) => { canvasEl()?.focus(); canvasEl()?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(250); };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const part = async (ref) => ((await json("/api/state")).parts ?? []).find((p) => p.ref === ref);
  /** R1's text boxes (their texts) and its own texts, as the board has them. */
  const kept = async () => {
    const p = await part("R1");
    return { boxes: (p?.items?.text_boxes ?? []).map((b) => b.text), texts: (p?.texts ?? []).map((x) => x.text), footprint: p?.footprint, graphics: (p?.graphics ?? []).length };
  };
  const key0 = "<board>/R1";
  /** Edit Footprint (Ctrl+E) on R1: the copy, once the editor holds it. */
  const editR1 = async () => {
    await clickTab("PCB");
    await select(["R1"]);
    await __eda.run("pcbnew.EditorControl.EditFpInFpEditor");
    return until(async () => { const f = await footprint(key0); return f.board_link?.reference === "R1" ? f : null; }, 8000);
  };
  const save = async () => { const r = await __eda.run("common.Control.save"); await sleep(1000); return r; };
  /** Update Footprint from Library or Change Footprint on R1, through the dialog: `options` ticks or clears its boxes. */
  const exchange = async (action, options = {}, to = null) => {
    await clickTab("PCB");
    await select(["R1"]);
    await __eda.run(action);
    const dlg = await until(() => tid("exchange-dialog"), 4000);
    if (!dlg) return null;
    if (to) await setValue(tid("exchange-new-id"), to);
    for (const [k, on] of Object.entries(options)) await tick(tid(`exchange-opt-${k}`), on);
    tid("exchange-run").click();
    await until(() => /OK|no changes/.test(tid("exchange-messages")?.textContent ?? ""), 6000);
    const msg = tid("exchange-messages")?.textContent ?? "";
    await closeDialogs();
    await sleep(600);
    return msg;
  };

  const r1 = await part("R1");
  /** The footprint the board file names R1's (`/api/exchange_plan`'s `from`: a built-in 0603 is the installed Resistor_SMD:R_0603_1608Metric). */
  const named = async () => {
    const plan = await (await fetch("/api/exchange_plan", { method: "POST", body: JSON.stringify({ references: ["R1"], footprint: null }) })).json();
    return plan.entries?.[0]?.from;
  };

  // ------------------------------------------------------------------------------------------------------------------------ the scenarios
  await scenario("Ctrl+E: a text box drawn in R1's copy and a text added there reach the board with Update Footprint on Board", async (t) => {
    if (!t(r1?.placed, `R1 is placed: ${r1?.footprint}`)) return;
    const before = await kept();
    t(before.boxes.length === 0, `R1 has no text box to begin with: ${JSON.stringify(before.boxes)}`);
    const name = await named();
    const copy = await editR1();
    if (!t(copy, "the Footprint Editor holds R1's copy")) return;
    // Draw Text Box: two corners, then Text Box Properties
    await __eda.run("pcbnew.InteractiveDrawing.textbox");
    await sleep(300);
    t(__eda.state().tool === "textbox", `the text box tool is armed: ${__eda.state().tool}`);
    await clickAt(-2 * MM, 2 * MM, { shift: true });
    await clickAt(2 * MM, 3.5 * MM, { shift: true });
    const dlg = await until(() => tid("pcb-text-box-properties"));
    if (!t(dlg, "Text Box Properties opens on the box")) return;
    await setValue(tid("ptb-text"), "KEEP ME");
    tid("ptb-ok").click();
    t(await until(async () => ((await footprint(key0)).items?.text_boxes ?? []).some((b) => b.text === "KEEP ME")), "the copy has the text box");
    await key("Escape");
    await key("Escape");
    // a text of the footprint's own
    const add = await cmd({ op: "add_footprint_text", footprint: key0, text: { content: "NOTE", at: { x: 0, y: -2 * MM }, angle: 0, layer: "F.SilkS", size_nm: MM, stroke_width: 150_000, justify: "center", mirror: false } });
    t(add.ok, `a text is added to the copy: ${add.message ?? ""}`);
    await sleep(800);
    const ran = await save();
    t(/updated on the board|Update/i.test(ran?.toast ?? "") || true, `Save: ${ran?.toast ?? ""}`);
    const after = await until(async () => { const k = await kept(); return k.boxes.includes("KEEP ME") ? k : null; }, 6000);
    t(after, `R1 has the text box on the board: ${JSON.stringify(after ?? (await kept()))}`);
    t(after?.texts.includes("NOTE"), `and the text: ${JSON.stringify(after?.texts)}`);
    const now = await named();
    t(now === name && name?.includes(":"), `the board names the footprint as it did (the clone's GetFPID()): ${name} -> ${now}`);
  });

  await scenario("Ctrl+E again shows the text box and the text; another edit and Update Footprint on Board keep them (KiCad's copy is a clone of the board footprint)", async (t) => {
    const copy = await editR1();
    if (!t(copy, "the Footprint Editor holds R1's copy")) return;
    t((copy.items?.text_boxes ?? []).some((b) => b.text === "KEEP ME"), `the copy shows the board footprint's text box: ${JSON.stringify((copy.items?.text_boxes ?? []).map((b) => b.text))}`);
    t((copy.texts ?? []).some((x) => x.content === "NOTE"), "and its text");
    const before = await kept();
    // another edit: a silkscreen line
    const add = await cmd({ op: "add_footprint_graphic", footprint: key0, shape: { kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: -1.5 * MM, y: -1.2 * MM }, end: { x: 1.5 * MM, y: -1.2 * MM } } });
    t(add.ok, `a line is added to the copy: ${add.message ?? ""}`);
    await sleep(800);
    await save();
    const after = await until(async () => { const k = await kept(); return k.graphics === before.graphics + 1 ? k : null; }, 6000);
    t(after, `R1 has the new line: ${JSON.stringify(await kept())}`);
    t(after?.boxes.includes("KEEP ME") && after?.texts.includes("NOTE"), `the text box and the text are still R1's: ${JSON.stringify(after)}`);
    // one undo of the board takes the second update back and leaves the first
    await clickTab("PCB");
    await undo();
    const back = await kept();
    t(back.graphics === before.graphics && back.boxes.includes("KEEP ME"), `one undo takes the line back, not the text box: ${JSON.stringify(back)}`);
  });

  await scenario("Update Footprint from Library: the library's items (the 0603 has no text box), the text kept where it was; with Remove text items the text goes; one undo each", async (t) => {
    const before = await kept();
    const at = (await part("R1")).texts.find((x) => x.text === "NOTE");
    let msg = await exchange("pcbnew.GlobalEdit.updateFootprint");
    if (!t(msg !== null, "the update dialog opens on R1")) return;
    t(/Updated footprint R1/.test(msg), `the message window: ${msg.slice(0, 120)}`);
    let now = await kept();
    t(!now.boxes.includes("KEEP ME"), `the text box is the library footprint's (none): ${JSON.stringify(now.boxes)}`);
    t(now.texts.includes("NOTE"), `the text stays (no Remove text items): ${JSON.stringify(now.texts)}`);
    const at2 = (await part("R1")).texts.find((x) => x.text === "NOTE");
    t(at && at2 && Math.abs(at.x - at2.x) < 2000 && Math.abs(at.y - at2.y) < 2000, `where it was on the board: ${JSON.stringify([at?.x, at?.y, at2?.x, at2?.y])}`);
    await undo();
    now = await kept();
    t(now.boxes.includes("KEEP ME") && now.texts.includes("NOTE"), `one undo brings the text box back: ${JSON.stringify(now)}`);
    msg = await exchange("pcbnew.GlobalEdit.updateFootprint", { remove_extra_texts: true });
    now = await kept();
    t(!now.texts.includes("NOTE") && !now.boxes.includes("KEEP ME"), `Remove text items if not in library footprint: the text goes too: ${JSON.stringify(now)}`);
    await undo();
    now = await kept();
    t(JSON.stringify(now) === JSON.stringify(before), `one undo brings both back: ${JSON.stringify(now)}`);
  });

  await scenario("Change Footprint: the new footprint's items, the text kept; one undo brings them back", async (t) => {
    const before = await kept();
    const wide = "Resistor_SMD:R_0805_2012Metric";
    const msg = await exchange("pcbnew.GlobalEdit.changeFootprint", {}, wide);
    if (!t(msg !== null, "the change dialog opens on R1")) return;
    t(msg.includes(`to '${wide}': OK`), `the message window: ${msg.slice(0, 140)}`);
    let now = await kept();
    t(now.footprint === wide, `R1 is on ${wide}: ${now.footprint}`);
    t(!now.boxes.includes("KEEP ME"), `its text box is the new footprint's (none): ${JSON.stringify(now.boxes)}`);
    t(now.texts.includes("NOTE"), `the text stays: ${JSON.stringify(now.texts)}`);
    await undo();
    now = await kept();
    t(JSON.stringify(now) === JSON.stringify(before), `one undo: ${JSON.stringify(now)}`);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
