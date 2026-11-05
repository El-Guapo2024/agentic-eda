// The schematic's placing tools in KiCad's order (`SCH_DRAWING_TOOLS::TwoClickPlace`, `createNewLabel`, `createNewText`, `DrawShape`, `PlaceSymbol` with the power
// filter; UI-LEFT S1): the tool's action opens the dialog (or the chooser of power symbols) first, where the cursor is; OK puts the item in the hand, and it rides
// the cursor until a click places it with one `/api/cmd` step; R, Shift+R, X and Y turn it while it rides; Esc, Undo and Delete drop it and keep the tool; the
// tool's own hotkey places it; another tool is refused; the next label of a multi-line entry rides at once; a label on a labelled wire takes that name with
// no dialog; Repeat (Insert) steps a label on; Increment acts on the label in the hand; a text box is drawn corner to corner and then asks for its text.
// Also S18's "Enter doesn't finish a wire": Enter is KiCad's `cursorClick` (a click where the cursor is), End its `cursorDblClick`/`finishInteractive`.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir>`, with KiCad's libraries: `KICAD_PATH`), on a COPY of a board: every scenario undoes what it did (and, if it left anything
// behind, the run undoes until the root sheet is as it was).
//
// It answers through `window.__schPlaceCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }. It works through `window.__eda` (the tools' actions, R, X,
// Esc, Undo, `__eda.state().carry`), the dialogs' controls, the pointer events a user makes on the canvas and the keys a user presses.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schPlaceCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schPlaceSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 5e6;
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
    window.__schPlaceSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = () => json("/api/schematic");
  const run = async (id, args) => { const r = await __eda.run(id, args); await sleep(250); return r; };
  const state = () => __eda.state();
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  // (on the body, where a key reaches the canvas's tools: Enter and End from a dialog's field or a button are theirs)
  const key = (k, extra = {}) => document.body.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...extra }));
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(300); };
  const toSelect = async () => { for (let i = 0; i < 3 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(dialog(), "Cancel") ?? buttonText(dialog(), "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(300); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(900); };
  const dblclickAt = async (x, y) => { const [px, py] = screenOf(x, y); const rect = container().getBoundingClientRect(); (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, detail: 2 })); await sleep(900); };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  const leave = async () => { while (state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const newOf = (before, list) => list.filter((x) => !before.some((b) => b.id === x.id));
  /** The item in the hand, once its drawing has arrived. */
  const carried = () => until(() => (state().carry?.drawn ? state().carry : null), 8000);
  /** The dialog the tool opened, by its title. */
  const opened = (title) => until(() => (dialog() && title.test(dialog().querySelector(".dialog-header")?.textContent ?? dialog().getAttribute("aria-label") ?? "") ? dialog() : null), 6000);
  /** Type the label's text in the open label dialog and press OK. */
  const typeLabel = async (dlg, text) => { const input = dlg.querySelector('input[aria-label="Label"], textarea[aria-label="Label"]'); setValue(input, text); await sleep(100); buttonText(dlg, "OK").click(); await sleep(300); };

  /** The root sheet as the scenarios change it. */
  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ labels: s.labels.map((l) => [l.id, l.net, l.at]), texts: s.texts.map((t) => [t.id, t.content]), power: s.power_symbols.map((p) => [p.id, p.at]), graphics: (s.graphics ?? []).map((g) => g.id), wires: s.wires.map((w) => [w.id, w.pts]) });
  };
  let base = null;
  const restore = async () => { for (let i = 0; i < 16; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    if (window.__schPlaceOnly && !window.__schPlaceOnly.test(name)) return; // `window.__schPlaceOnly = /regexp/` runs some of the scenarios only
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try {
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await toSelect(); } catch { /* ignore */ }
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  if (state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
  await leave();
  await fit();
  const v0 = state().view;
  if (!v0 || !Number.isFinite(v0.scale) || v0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(v0)})`] }); check.done = true; return "no canvas size"; }
  base = await snapshot();
  // An empty part of the root sheet (below and left of the module sheets), on the 50 mil grid.
  const G = 1_270_000;
  const X0 = 40 * G;
  const Y0 = 110 * G;

  (async () => {
    // ----- a local label: the dialog first, then the label rides, then a click places it
    await scenario("Place Label: L opens Label Properties first (nothing placed, nothing carried), OK puts the label in the hand, a click places it there; one undo", async (t) => {
      const s0 = await sch();
      await hoverAt(X0, Y0);
      const r = await run("eeschema.InteractiveDrawing.placeLabel");
      const dlg = await opened(/^Label Properties/);
      if (!t(!!dlg, `L opens Label Properties at once (${JSON.stringify(r)}; ${state().dialogs})`)) return;
      t(state().tool === "sch_label_local" && !state().carry, "the tool is armed, nothing is in the hand yet");
      t((await sch()).labels.length === s0.labels.length, "nothing is on the sheet yet");
      t(dlg.querySelector('input[aria-label="Label"]').value === "", "the text starts empty (createNewLabel's new label)");
      await typeLabel(dlg, "E2E_LOCAL");
      const c = await carried();
      t(!!c && c.kind === "label" && c.scope === "local" && c.text === "E2E_LOCAL", `OK puts the label in the hand, drawn (${JSON.stringify(c)})`);
      t((await sch()).labels.length === s0.labels.length, "it is still not on the sheet");
      await clickAt(X0 + 4 * G, Y0 + 2 * G);
      const s1 = await sch();
      const made = newOf(s0.labels, s1.labels);
      t(made.length === 1 && made[0].net === "E2E_LOCAL" && same(made[0].at, [X0 + 4 * G, Y0 + 2 * G]), `the click places it where it is (${JSON.stringify(made.map((l) => [l.net, l.at]))})`);
      t(state().tool === "sch_label_local" && !state().carry, "the tool stays armed with nothing in the hand");
      await toSelect();
      await undo();
      t((await sch()).labels.length === s0.labels.length, "one undo takes it away");
    });

    // ----- a global label turned while it rides
    await scenario("Place Global Label: Ctrl+L, OK, then R turns the label in the hand (up), X mirrors it back across (it stays up), Shift+R turns it back; the click places it so", async (t) => {
      const s0 = await sch();
      await hoverAt(X0, Y0 + 6 * G);
      await run("eeschema.InteractiveDrawing.placeGlobalLabel");
      const dlg = await opened(/^Global Label Properties/);
      if (!t(!!dlg, `Ctrl+L opens Global Label Properties (${state().dialogs})`)) return;
      const auto = dlg.querySelector('input[aria-label="Auto"]');
      if (auto?.checked) { auto.click(); await sleep(100); }
      setValue(dlg.querySelector('select[aria-label="Orientation"]'), "right");
      await typeLabel(dlg, "E2E_GLOBAL");
      let c = await carried();
      t(c?.spin === "right", `it rides as it was made (${c?.spin})`);
      await run("eeschema.InteractiveEdit.rotateCCW");
      c = await carried();
      t(c?.spin === "up", `R turns it counter-clockwise: up (${c?.spin})`);
      await run("eeschema.InteractiveEdit.mirrorH");
      t(state().carry?.spin === "up", `Mirror Horizontally leaves an upward label as it is (${state().carry?.spin})`);
      await run("eeschema.InteractiveEdit.mirrorV");
      t(state().carry?.spin === "bottom", `Mirror Vertically turns it down (${state().carry?.spin})`);
      await run("eeschema.InteractiveEdit.rotateCW");
      t(state().carry?.spin === "left", `Shift+R turns it clockwise: left (${state().carry?.spin})`);
      await run("eeschema.InteractiveEdit.rotateCW");
      t(state().carry?.spin === "up", `and again: up (${state().carry?.spin})`);
      await carried();
      await clickAt(X0, Y0 + 6 * G);
      const made = newOf(s0.labels, (await sch()).labels);
      t(made.length === 1 && made[0].scope === "global" && made[0].spin === "up", `placed as it was turned (${JSON.stringify(made.map((l) => [l.net, l.scope, l.spin]))})`);
      await toSelect();
      await undo();
      t((await sch()).labels.length === s0.labels.length, "one undo takes it away");
    });

    // ----- a hierarchical label entry of two lines
    await scenario("Place Hierarchical Label with Multiple label input: two lines, each click places the next, then the tool holds nothing; one undo each", async (t) => {
      const s0 = await sch();
      await hoverAt(X0 + 20 * G, Y0);
      await run("eeschema.InteractiveDrawing.placeHierarchicalLabel");
      const dlg = await opened(/^Hierarchical Label Properties/);
      if (!t(!!dlg, `H opens Hierarchical Label Properties (${state().dialogs})`)) return;
      dlg.querySelector('input[aria-label="Multiple label input"]').click();
      await sleep(200);
      t(!!dlg.querySelector('textarea[aria-label="Label"]'), "the entry takes several lines");
      await typeLabel(dlg, "E2E_H0\n E2E_H1 \n");
      let c = await carried();
      t(c?.text === "E2E_H0" && c?.queue === 1, `the first rides, one waits (${JSON.stringify(c)})`);
      await clickAt(X0 + 20 * G, Y0);
      c = await carried();
      t(c?.text === "E2E_H1" && c?.queue === 0, `after the click the next rides at once (${JSON.stringify(c)})`);
      await clickAt(X0 + 20 * G, Y0 + 2 * G);
      t(!state().carry && state().tool === "sch_label_hier", "after the last the tool holds nothing and stays armed");
      const made = newOf(s0.labels, (await sch()).labels);
      t(same(made.map((l) => [l.net, l.scope]).sort(), [["E2E_H0", "hierarchical"], ["E2E_H1", "hierarchical"]]), `two hierarchical labels (${JSON.stringify(made.map((l) => [l.net, l.at]))})`);
      await toSelect();
      await undo();
      await undo();
      t((await sch()).labels.length === s0.labels.length, "two undos take them away");
    });

    // ----- a directive label
    await scenario("Place Directive Label: its dialog first, then it rides; X mirrors the carried pole; the click places it; one undo", async (t) => {
      const s0 = await sch();
      await hoverAt(X0 + 30 * G, Y0);
      await run("eeschema.InteractiveDrawing.placeClassLabel");
      const dlg = await opened(/^Directive Label Properties/);
      if (!t(!!dlg, `Place Directive Label opens its dialog first (${state().dialogs})`)) return;
      setValue(dlg.querySelector("input"), "E2E_CLASS");
      await sleep(100);
      buttonText(dlg, "OK").click();
      let c = await carried();
      t(c?.kind === "directive" && c?.netclass === "E2E_CLASS" && c?.orientation === 0, `OK puts it in the hand (${JSON.stringify(c)})`);
      await run("eeschema.InteractiveEdit.mirrorV");
      c = await carried();
      t(c?.orientation === 180_000, `Mirror Vertically swaps the pole across (${c?.orientation})`);
      await clickAt(X0 + 30 * G, Y0);
      const made = newOf(s0.graphics ?? [], (await sch()).graphics ?? []);
      t(made.length === 1 && made[0].shape.type === "directive" && made[0].shape.netclass === "E2E_CLASS" && made[0].shape.orientation === 180_000 && same([made[0].shape.at.x, made[0].shape.at.y], [X0 + 30 * G, Y0]), `placed there, as turned (${JSON.stringify(made.map((g) => g.shape))})`);
      await toSelect();
      await undo();
      t(((await sch()).graphics ?? []).length === (s0.graphics ?? []).length, "one undo takes it away");
    });

    // ----- a text
    await scenario("Place Text: T opens Text Properties first; OK, then Shift+R turns the text in the hand up the sheet; E reopens its dialog on it; the click places it; one undo", async (t) => {
      const s0 = await sch();
      await hoverAt(X0 + 40 * G, Y0);
      await run("eeschema.InteractiveDrawing.placeSchematicText");
      const dlg = await opened(/^Text Properties/);
      if (!t(!!dlg, `T opens Text Properties (${state().dialogs})`)) return;
      setValue(dlg.querySelector('select[aria-label="Horizontal justification"]'), "left");
      setValue(dlg.querySelector(".kv-grid input"), "E2E note");
      await sleep(100);
      buttonText(dlg, "OK").click();
      let c = await carried();
      t(c?.kind === "text" && c?.text === "E2E note" && c?.vertical === false, `OK puts the text in the hand (${JSON.stringify(c)})`);
      await run("eeschema.InteractiveEdit.rotateCW");
      c = await carried();
      t(c?.vertical === true && c?.h === "right", `Shift+R turns it up the sheet, its justification flipped (${JSON.stringify(c)})`);
      // Properties of the text in the hand: its dialog on it, OK changes what rides
      await run("eeschema.InteractiveEdit.properties");
      const again = await opened(/^Text Properties/);
      t(!!again && again.querySelector(".kv-grid input")?.value === "E2E note", "E opens Text Properties on the text in the hand");
      if (again) { setValue(again.querySelector(".kv-grid input"), "E2E note 2"); await sleep(100); buttonText(again, "OK").click(); }
      c = await carried();
      t(c?.text === "E2E note 2" && c?.vertical === true, `OK changes the text that rides (${JSON.stringify(c)})`);
      await clickAt(X0 + 40 * G, Y0 + 4 * G);
      const made = newOf(s0.texts, (await sch()).texts);
      t(made.length === 1 && made[0].content === "E2E note 2" && made[0].vertical === true && made[0].h === "right", `placed as it was turned (${JSON.stringify(made)})`);
      await toSelect();
      await undo();
      t((await sch()).texts.length === s0.texts.length, "one undo takes it away");
    });

    // ----- a text box
    await scenario("Draw Text Box: two corners, then Text Box Properties; OK adds it; one undo", async (t) => {
      const s0 = await sch();
      await run("eeschema.InteractiveDrawing.drawTextBox");
      t(state().tool === "sch_textbox", `the text box tool is armed (${state().tool})`);
      await clickAt(X0 + 50 * G, Y0);
      t(!dialog(), "the first corner opens nothing");
      await clickAt(X0 + 60 * G, Y0 + 6 * G);
      const dlg = await opened(/^Text Box Properties/);
      if (!t(!!dlg, `the second corner opens Text Box Properties (${state().dialogs})`)) return;
      setValue(dlg.querySelector("textarea"), "E2E box");
      await sleep(100);
      buttonText(dlg, "OK").click();
      await sleep(1200);
      const made = newOf(s0.graphics ?? [], (await sch()).graphics ?? []);
      t(made.length === 1 && made[0].shape.type === "text_box" && made[0].shape.text === "E2E box" && same([made[0].shape.start.x, made[0].shape.start.y, made[0].shape.end.x, made[0].shape.end.y], [X0 + 50 * G, Y0, X0 + 60 * G, Y0 + 6 * G]), `the box is there (${JSON.stringify(made.map((g) => g.shape))})`);
      t(state().tool === "sch_textbox", "the tool stays armed");
      await toSelect();
      await undo();
      t(((await sch()).graphics ?? []).length === (s0.graphics ?? []).length, "one undo takes it away");
    });

    // ----- a power symbol from the chooser
    await scenario("Place Power: P opens Choose Power Symbol (power symbols only), GND rides the cursor, R turns it, the click places it with KiCad's symbol; one undo", async (t) => {
      const s0 = await sch();
      await hoverAt(X0 + 10 * G, Y0 + 12 * G);
      await run("eeschema.InteractiveDrawing.placePowerSymbol");
      const dlg = await opened(/^Choose Power Symbol/);
      if (!t(!!dlg, `P opens the chooser of power symbols (${state().dialogs})`)) return;
      const search = dlg.querySelector("[data-chooser-search]");
      setValue(search, "GND");
      await until(() => /match/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? "") && !/Searching/.test(dlg.querySelector("[data-chooser-status]")?.textContent ?? ""), 30000);
      await sleep(600);
      const ids = [...dlg.querySelectorAll("[role=treeitem][data-id]")].map((r) => r.getAttribute("data-id"));
      t(ids.length > 0 && ids.includes("power:GND"), `the search finds power:GND (${ids.slice(0, 6)})`);
      t(!ids.some((id) => /^Device:/.test(id)), "and no symbol that is not a power symbol");
      dlg.querySelector('[role=treeitem][data-id="power:GND"]')?.click();
      await sleep(400);
      buttonText(dlg, "OK").click();
      let c = await carried();
      t(c?.kind === "power" && /^power:GND$/.test(c?.libId ?? "") && c?.net === "GND", `OK puts the power symbol in the hand (${JSON.stringify(c)})`);
      await run("eeschema.InteractiveEdit.rotateCCW");
      c = await carried();
      t(c?.rot === 90_000, `R turns it a quarter (${c?.rot})`);
      await clickAt(X0 + 10 * G, Y0 + 12 * G);
      const s1 = await sch();
      const made = newOf(s0.power_symbols, s1.power_symbols);
      t(made.length === 1 && made[0].lib_id === "power:GND" && made[0].net === "GND" && made[0].rot === 90 && same(made[0].at, [X0 + 10 * G, Y0 + 12 * G]), `placed as turned (${JSON.stringify(made.map((p) => [p.id, p.lib_id, p.rot, p.at]))})`);
      t(!!s1.lib_symbols["power:GND"] && (s1.lib_symbols["power:GND"].graphics ?? []).length > 0, "drawn with its library symbol");
      t(!state().carry && state().tool === "sch_power", "the tool stays armed with nothing in the hand");
      await toSelect();
      await undo();
      t((await sch()).power_symbols.length === s0.power_symbols.length, "one undo takes it away");
    });

    // ----- the keys while an item rides
    await scenario("While a label rides: another tool is refused, Esc drops it and keeps the tool, Undo and Delete drop it, the tool's hotkey places it, a second Esc leaves", async (t) => {
      const s0 = await sch();
      const make = async (name) => {
        await run("eeschema.InteractiveDrawing.placeLabel");
        const dlg = await opened(/^Label Properties/);
        if (!dlg) return false;
        await typeLabel(dlg, name);
        return !!(await carried());
      };
      await hoverAt(X0 + 4 * G, Y0 + 20 * G);
      if (!t(await make("E2E_KEY1"), "a label rides")) return;
      await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      t(state().tool === "sch_label_local" && state().carry?.text === "E2E_KEY1", `the wire tool is refused while it rides (${state().tool})`);
      await escape();
      t(!state().carry && state().tool === "sch_label_local", "Esc drops it, the tool stays armed");
      await escape();
      t(state().tool === "select", "a second Esc leaves the tool");
      if (!t(await make("E2E_KEY1"), "a label rides again")) return;
      await __eda.run("common.Interactive.undo");
      await sleep(400);
      t(!state().carry && state().tool === "sch_label_local" && same(await snapshot(), base), "Undo drops it and undoes nothing");
      await run("eeschema.InteractiveDrawing.placeLabel");
      let dlg = await opened(/^Label Properties/);
      if (dlg) await typeLabel(dlg, "E2E_KEY1");
      await carried();
      await run("common.Interactive.delete");
      t(!state().carry, "Delete drops it");
      await run("eeschema.InteractiveDrawing.placeLabel");
      dlg = await opened(/^Label Properties/);
      if (dlg) await typeLabel(dlg, "E2E_KEY1");
      await carried();
      await hoverAt(X0 + 8 * G, Y0 + 20 * G);
      // Increment acts on the label in the hand
      await run("eeschema.Interactive.incrementPrimary");
      t(state().carry?.text === "E2E_KEY2", `Increment steps the label in the hand (${state().carry?.text})`);
      await run("eeschema.InteractiveDrawing.placeLabel");
      await sleep(800);
      const made = newOf(s0.labels, (await sch()).labels);
      t(made.length === 1 && made[0].net === "E2E_KEY2" && same(made[0].at, [X0 + 8 * G, Y0 + 20 * G]), `the tool's hotkey places it where the cursor is (${JSON.stringify(made.map((l) => [l.net, l.at]))})`);
      await toSelect();
      await undo();
    });

    // ----- a label on a labelled wire, and Repeat
    await scenario("A label on a wire a label names takes that name with no dialog and rides; Repeat (Insert) places a copy 100 mil down with its text stepped on", async (t) => {
      const s0 = await sch();
      const yw = Y0 + 28 * G;
      await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await clickAt(X0, yw);
      await clickAt(X0 + 12 * G, yw);
      await dblclickAt(X0 + 12 * G, yw);
      await toSelect();
      const wire = newOf(s0.wires, (await sch()).wires)[0];
      if (!t(!!wire, "a wire to name")) return;
      await post({ op: "sch_edit", verb: "place_label", net: "E2E_BUS3", at: { x: X0 + 2 * G, y: yw }, kind: { scope: "local" }, spin: "right" });
      // the studio reads the sheet again: the wire's driver is the label now
      await until(async () => (await sch()).wires.some((w) => w.id === wire.id && w.label_driver), 6000);
      await sleep(1500);
      await hoverAt(X0 + 8 * G, yw);
      await run("eeschema.InteractiveDrawing.placeLabel");
      const c = await carried();
      t(!dialog() && c?.text === "E2E_BUS3", `no dialog; the label in the hand has the wire's name (${JSON.stringify(c)}; ${state().dialogs})`);
      await clickAt(X0 + 8 * G, yw);
      await toSelect();
      const s1 = await sch();
      const two = newOf(s0.labels, s1.labels).filter((l) => l.net === "E2E_BUS3");
      t(two.length === 2, `two labels name the wire (${two.length})`);
      await run("eeschema.InteractiveEdit.repeatDrawItem");
      await sleep(900);
      const rep = newOf(s1.labels, (await sch()).labels);
      t(rep.length === 1 && rep[0].net === "E2E_BUS4" && same(rep[0].at, [X0 + 8 * G, yw + 2 * G]), `Repeat places E2E_BUS4 100 mil down (${JSON.stringify(rep.map((l) => [l.net, l.at]))})`);
      for (let i = 0; i < 4; i++) await undo();
    });

    // ----- S18: Enter in the wire tool is a click
    await scenario("S18: in the wire tool Enter is a click where the cursor is (KiCad's cursorClick): it starts the wire and adds a corner; End finishes it", async (t) => {
      const s0 = await sch();
      await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await hoverAt(X0 + 40 * G, Y0 + 30 * G);
      key("Enter");
      await sleep(500);
      await hoverAt(X0 + 50 * G, Y0 + 30 * G);
      key("Enter");
      await sleep(500);
      await hoverAt(X0 + 50 * G, Y0 + 36 * G);
      key("End");
      await sleep(1200);
      await toSelect();
      const made = newOf(s0.wires, (await sch()).wires);
      const pts = made.flatMap((w) => w.pts);
      t(made.length >= 1 && pts.some((p) => same(p, [X0 + 40 * G, Y0 + 30 * G])) && pts.some((p) => same(p, [X0 + 50 * G, Y0 + 30 * G])) && pts.some((p) => same(p, [X0 + 50 * G, Y0 + 36 * G])), `the wire runs through the two Enters and ends at End (${JSON.stringify(made.map((w) => w.pts))})`);
      for (let i = 0; i < made.length; i++) await undo();
    });

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();

  async function post(cmd) {
    return fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  }
  return "started";
})();
