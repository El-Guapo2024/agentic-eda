// Schematic editor basics check (GAPS.md item 33, package P20; items 1 and 12): what a person does first on a sheet -- place a symbol from the Symbol Chooser
// (A: search, pick, OK, a click on the sheet), draw wires (W: clicks, a double click ends), the junction a wire ending on another one makes and the one Place
// Junction (J) puts down (and refuses where nothing joins, in KiCad's words), a label on a wire naming its net (L and the label dialog), Annotate Schematic
// (Selection, Reset existing annotations, sorted by X) and the Electrical Rules Checker run through kicad-cli, its findings on the symbol placed. Every edit is
// read back from the server (`GET /api/schematic`: symbols, wires, junctions, labels and the nets of the wires; `GET /api/erc`, captured while the dialog runs),
// undone (one step each), and the sheet compared with how it was at the start.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir> --port N --ui web/studio/dist`, kicad-cli on the PATH or in EDA_KICAD_CLI, KiCad's symbol libraries installed) with
// `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/sch-edit.check.js`, or pasted into the Browser pane's javascript
// tool, on a COPY of a board. It works on the root sheet, in the empty band under the three module sheets (x 76 to 165 mm, y 125 to 170 mm).
// It answers through `window.__schEditCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __schEditCheck.wait(30000)` until `.done`.
// It works through `window.__eda` (actions by id; `__eda.state().view` maps the sheet to the canvas), the canvas' pointer events, the Symbol Chooser's
// controls (`#library-tree-search`, the tree's `[data-id]` rows, `[data-chooser-ok]`), the label and Annotate dialogs' fields and the ERC dialog's Run ERC button.
// run-all-checks: board=mcu30 minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__schEditCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // What the ERC dialog asks the server, captured as it comes back.
  const ercReplies = [];
  if (!window.__schEditFetch) {
    const real = window.fetch.bind(window);
    window.__schEditFetch = real;
    window.fetch = async (input, init) => {
      const r = await real(input, init);
      const url = typeof input === "string" ? input : input?.url ?? "";
      if (/\/api\/erc(\?|$)/.test(url)) r.clone().json().then((body) => ercReplies.push(body)).catch(() => undefined);
      return r;
    };
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sch = () => json("/api/schematic");
  const run = (id, args) => __eda.run(id, args);
  const snapshot = async () => {
    const s = await sch();
    return JSON.stringify({ symbols: s.symbols.map((x) => [x.id, x.lib_id, x.at, x.rot]), wires: s.wires.map((w) => [w.id, w.pts, w.net]), labels: s.labels.map((l) => [l.id, l.net, l.at]), junctions: (s.junctions ?? []).map((j) => j.at), sheets: s.sheets.map((x) => [x.id, x.at]) });
  };
  const select = async (ids) => { await run("common.InteractiveSelection.clear"); if (ids.length > 0) await run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await run("common.Interactive.undo"); await sleep(600); };
  const redo = async () => { await run("common.Interactive.redo"); await sleep(600); };
  const escape = async () => { await run("common.Interactive.cancel"); await sleep(400); };
  const toSelect = async () => { for (let i = 0; i < 3 && __eda.state().tool !== "select"; i++) await escape(); };
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const dialogNamed = (title) => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.textContent.trim().startsWith(title));
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      const b = buttonText(document, "Cancel") ?? [...document.querySelectorAll(".dialog-footer button")].find((x) => /^(Close|OK)$/.test(x.textContent.trim())) ?? document.querySelector("[data-chooser-cancel]");
      if (!b) break;
      b.click();
      await sleep(250);
    }
  };
  const toast = () => [...document.querySelectorAll("div")].filter((el) => el.style.zIndex === "3000" && el.style.position === "absolute").map((el) => el.textContent).join(" | ");

  // The canvas: pointer events at a point of the sheet (um), placed through the view the hook reports.
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = container();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(150); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(500); };
  const dblclickAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    const [cx, cy] = [rect.left + px, rect.top + py];
    document.elementFromPoint(cx, cy).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: cx, clientY: cy, detail: 2 }));
    await sleep(900);
  };
  /** W, then a click at each point, a double click at the last: one wire. */
  const drawWire = async (pts) => {
    await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
    await sleep(300);
    for (const [x, y] of pts) await clickAt(x, y);
    await dblclickAt(...pts[pts.length - 1]);
    await toSelect();
  };
  const newOf = (before, list) => list.filter((x) => !before.has(x.id));

  let base = null;
  const restore = async () => { for (let i = 0; i < 12; i++) { if ((await snapshot()) === base) return i; await undo(); } return -1; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); await toSelect(); } catch { /* ignore */ }
    const left = await restore();
    if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`FAIL: ${left} edit(s) the scenario left behind were undone`); entry.ok = false; }
    results.push(entry);
  };

  /** A, the Symbol Chooser: search, pick the row `libId`, OK; then a click at (x, y) places it. Resolves to the symbol the server added. */
  const placeSymbol = async (t, libId, x, y, query) => {
    const before = new Set((await sch()).symbols.map((s) => s.id));
    const r = await run("eeschema.InteractiveDrawing.placeSymbol");
    t(r.ok !== false && !!(await until(() => document.querySelector('[aria-label="Symbol Chooser"]'), 5000)), `A opens the Symbol Chooser (${JSON.stringify(r)})`);
    const search = document.querySelector("#library-tree-search");
    setValue(search, query);
    const row = await until(() => document.querySelector(`[data-chooser-tree] [data-id="${libId}"]`), 20000);
    if (!t(!!row, `the search "${query}" lists ${libId} (${document.querySelector("[data-chooser-status]")?.textContent})`)) return null;
    row.click();
    await sleep(400);
    const ok = document.querySelector("[data-chooser-ok]");
    t(!!ok && !ok.disabled, "OK is enabled once a symbol is picked");
    ok.click();
    await sleep(500);
    t(!document.querySelector('[aria-label="Symbol Chooser"]') && __eda.state().tool !== "select", `OK closes the chooser and arms the placement (${__eda.state().tool})`);
    await clickAt(x, y);
    const added = await until(async () => newOf(before, (await sch()).symbols)[0], 8000);
    await toSelect();
    return added ?? null;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1200); }
  while (__eda.state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
  await run("common.Control.zoomFitScreen");
  await sleep(500);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  const s0 = await sch();
  if (s0.sheets.length < 3) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the three module sheets of examples/mcu_board_30plus.yaml on its root sheet"] }); check.done = true; return "no board"; }
  base = await snapshot();
  // The empty band under the module sheets, on the 50 mil grid.
  const G = 1270;
  const X0 = 80 * G, Y0 = 110 * G; // 101.6, 139.7 mm

  (async () => {
    // ----- a symbol from the chooser
    let placedRef = null;
    await scenario("place symbol: A, the chooser's search and OK, a click on the sheet put Device:R there with the next free reference; one undo step", async (t) => {
      const sym = await placeSymbol(t, "Device:R", X0, Y0, "Device:R");
      if (!t(!!sym, "a symbol was added")) return;
      placedRef = sym.id;
      t(sym.lib_id === "Device:R", `it is Device:R (${sym.lib_id})`);
      t(JSON.stringify(sym.at) === JSON.stringify([X0, Y0]), `at the click, on the grid (${sym.at})`);
      const refs = s0.symbols.map((s) => s.id);
      t(/^R\d+$/.test(sym.id) && !refs.includes(sym.id), `its reference is a free R number (${sym.id})`);
      const lib = (await sch()).lib_symbols?.["Device:R"];
      t(!!lib && lib.pins?.length === 2, `the schematic holds its definition (${Object.keys((await sch()).lib_symbols ?? {})})`);
      await undo();
      t(!(await sch()).symbols.some((s) => s.id === sym.id), "one undo takes it away");
      await redo();
      t((await sch()).symbols.some((s) => s.id === sym.id), "redo puts it back");
      await undo();
    });

    // ----- wires and junctions
    await scenario("wire: W, two clicks and a double click draw one wire between the points; one undo step", async (t) => {
      const before = new Set((await sch()).wires.map((w) => w.id));
      await drawWire([[X0, Y0 + 8 * G], [X0 + 10 * G, Y0 + 8 * G]]);
      const added = newOf(before, (await sch()).wires);
      t(added.length === 1, `one wire was added (${added.length})`);
      t(JSON.stringify(added[0]?.pts) === JSON.stringify([[X0, Y0 + 8 * G], [X0 + 10 * G, Y0 + 8 * G]]), `from the first click to the last (${JSON.stringify(added[0]?.pts)})`);
      t(__eda.state().tool === "select", `the tool is left after Escape (${__eda.state().tool})`);
      await undo();
      t((await snapshot()) === base, "one undo takes it away");
    });

    await scenario("junction: a wire that ends on another wire's middle gets a junction there; J joins two wires that cross, and refuses on a lone wire or on nothing, in KiCad's words", async (t) => {
      const ya = Y0 + 12 * G;
      await drawWire([[X0, ya], [X0 + 12 * G, ya]]);
      const j0 = ((await sch()).junctions ?? []).length;
      t(j0 === (s0.junctions ?? []).length, "one wire drawn alone makes no junction");
      // A second wire from below up to the middle of the first: a T (`finishSegments` -> `IsExplicitJunctionNeeded`).
      await drawWire([[X0 + 6 * G, ya + 6 * G], [X0 + 6 * G, ya]]);
      const s1 = await sch();
      t((s1.junctions ?? []).some((j) => j.at[0] === X0 + 6 * G && j.at[1] === ya), `a junction where the second wire meets the first (${JSON.stringify((s1.junctions ?? []).map((j) => j.at))})`);
      const nets = [...new Set(s1.wires.filter((w) => w.pts.some((q) => q[1] === ya || q[0] === X0 + 6 * G)).map((w) => w.net))];
      t(nets.length === 1, `the two wires are one net (${nets})`);
      // A third wire across the first: a crossing is not joined by itself, J joins it (`IsExplicitJunctionAllowed` breaks crossings).
      await drawWire([[X0 + 3 * G, ya - 4 * G], [X0 + 3 * G, ya + 4 * G]]);
      const s2 = await sch();
      t((s2.junctions ?? []).length === (s1.junctions ?? []).length, `a wire crossing another gets no junction (${JSON.stringify((s2.junctions ?? []).map((j) => j.at))})`);
      await run("eeschema.InteractiveDrawing.placeJunction");
      await sleep(300);
      await clickAt(X0 + 3 * G, ya);
      const s3 = await sch();
      t((s3.junctions ?? []).length === (s2.junctions ?? []).length + 1 && (s3.junctions ?? []).some((j) => j.at[0] === X0 + 3 * G && j.at[1] === ya), `J on the crossing puts a junction there (${JSON.stringify((s3.junctions ?? []).map((j) => j.at))})`);
      // J on the middle of a lone wire, and on nothing: KiCad's AnalyzePoint finds fewer than two items there, so it refuses.
      await clickAt(X0 + 10 * G, ya);
      await sleep(300);
      t(((await sch()).junctions ?? []).length === (s3.junctions ?? []).length && /Junction location contains no joinable wires and\/or pins\./.test(toast()), `J on the middle of one wire is refused (${toast()})`);
      await clickAt(X0 + 20 * G, ya + 20 * G);
      await sleep(300);
      t(((await sch()).junctions ?? []).length === (s3.junctions ?? []).length, "and so is J where nothing is");
      await toSelect();
      await undo();
      t(((await sch()).junctions ?? []).length === (s2.junctions ?? []).length, "one undo takes back J");
      await undo(); // the crossing wire
      await undo(); // the T
      t(((await sch()).junctions ?? []).length === j0, "the undo of the T takes its junction with it");
      await undo(); // the first wire
      t((await snapshot()) === base, "and the last undo the first wire");
    });

    // ----- a label names the net
    await scenario("label: L, a click on a wire and the dialog's net name put a label there, and the wire's net takes its name; one undo step", async (t) => {
      const yw = Y0 + 16 * G;
      await drawWire([[X0, yw], [X0 + 10 * G, yw]]);
      const wire = (await sch()).wires.find((w) => w.pts[0][1] === yw && w.pts[0][0] === X0);
      if (!t(!!wire, "the wire is there")) return;
      const netBefore = wire.net;
      const labelsBefore = new Set((await sch()).labels.map((l) => l.id));
      await run("eeschema.InteractiveDrawing.placeLabel");
      await sleep(300);
      await clickAt(X0 + 4 * G, yw);
      const dlg = await until(() => document.querySelector(".dialog input[autofocus], .dialog .kv-grid input"), 4000);
      t(!!dlg, `a click opens the label dialog (${__eda.state().dialogs})`);
      if (!dlg) return;
      setValue(dlg, "P20_SENSE");
      await sleep(100);
      dlg.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await sleep(900);
      await toSelect();
      const s1 = await sch();
      const label = newOf(labelsBefore, s1.labels)[0];
      t(!!label && label.net === "P20_SENSE" && JSON.stringify(label.at) === JSON.stringify([X0 + 4 * G, yw]), `the label is on the wire with its name (${JSON.stringify(label && [label.net, label.at])})`);
      const w1 = s1.wires.find((w) => w.id === wire.id) ?? s1.wires.find((w) => w.pts[0][1] === yw && w.pts[0][0] === X0);
      t(w1?.net === "P20_SENSE", `the wire's net is the label's now (${netBefore} -> ${w1?.net})`);
      await undo();
      t(!(await sch()).labels.some((l) => l.net === "P20_SENSE"), "one undo takes the label away");
      await undo();
      t((await snapshot()) === base, "a second takes the wire away");
    });

    // ----- Annotate
    await scenario("annotate: two symbols placed right to left, Annotate on the selection with Reset and Sort by X numbers them left to right; one undo step", async (t) => {
      const right = await placeSymbol(t, "Device:R", X0 + 30 * G, Y0, "Device:R");
      const left = await placeSymbol(t, "Device:R", X0 + 22 * G, Y0, "Device:R");
      if (!t(!!right && !!left, `two symbols placed (${right?.id}, ${left?.id})`)) return;
      const n = (id) => Number(id.replace(/^\D+/, ""));
      t(n(left.id) > n(right.id), `the one placed second, on the left, has the higher number (${left.id} left of ${right.id})`);
      await select([right.id, left.id]);
      const r = await run("eeschema.EditorControl.annotate");
      await sleep(500);
      const dlg = dialogNamed("Annotate");
      t(r.ok !== false && !!dlg, `Annotate Schematic opens (${r.dialog})`);
      if (!dlg) return;
      const radio = (text) => [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim().startsWith(text))?.querySelector("input");
      radio("Selection")?.click();
      radio("Sort symbols by X position")?.click();
      radio("Reset existing annotations")?.click();
      await sleep(200);
      t(radio("Selection")?.checked && radio("Reset existing annotations")?.checked && radio("Sort symbols by X position")?.checked, "Selection, Reset existing annotations and Sort by X are chosen");
      buttonText(dlg, "Annotate")?.click();
      const done = await until(async () => { const s = await sch(); const at = (x) => s.symbols.find((q) => q.at[0] === x && q.at[1] === Y0); return at(X0 + 22 * G) && at(X0 + 30 * G) && n(at(X0 + 22 * G).id) < n(at(X0 + 30 * G).id) ? s : null; }, 15000);
      t(!!done, "the left one now has the lower number");
      if (done) {
        const l2 = done.symbols.find((q) => q.at[0] === X0 + 22 * G && q.at[1] === Y0), r2 = done.symbols.find((q) => q.at[0] === X0 + 30 * G && q.at[1] === Y0);
        t(new Set([l2.id, r2.id]).size === 2 && [l2.id, r2.id].every((id) => !s0.symbols.some((s) => s.id === id)), `two free references, left to right: ${l2.id}, ${r2.id}`);
      }
      await until(() => /R\d+/.test(document.querySelector('[data-testid="annotate-messages"]')?.textContent ?? ""), 8000);
      t(/R\d+/.test(document.querySelector('[data-testid="annotate-messages"]')?.textContent ?? ""), `the messages say what each was given (${document.querySelector('[data-testid="annotate-messages"]')?.textContent.slice(0, 120)})`);
      await closeDialogs();
      await undo();
      const back = await sch();
      t(back.symbols.some((q) => q.id === left.id && q.at[0] === X0 + 22 * G) && back.symbols.some((q) => q.id === right.id && q.at[0] === X0 + 30 * G), "one undo gives them their references back");
      await undo(); await undo();
      t((await snapshot()) === base, "two more take the two symbols away");
    });

    // ----- ERC through kicad-cli
    await scenario("ERC: the Electrical Rules Checker runs kicad-cli, and a resistor left unwired is two unconnected pins, where its pins are", async (t) => {
      const sym = await placeSymbol(t, "Device:R", X0, Y0, "Device:R");
      if (!t(!!sym, "a resistor was placed")) return;
      ercReplies.length = 0;
      const r = await run("eeschema.InspectionTool.runERC");
      await sleep(500);
      const dlg = dialogNamed("Electrical Rules Checker");
      t(!!dlg, `the action opens the Electrical Rules Checker (${r.dialog})`);
      if (!dlg) return;
      buttonText(dlg, "Run ERC")?.click();
      const rep = await until(() => ercReplies.find((x) => Array.isArray(x?.violations)), 90000);
      if (!t(!!rep, "the run came back")) return;
      t(typeof rep.engine === "string" && rep.engine.startsWith("kicad-cli"), `kicad-cli ran it (${rep.engine})`);
      const mine = rep.violations.filter((v) => v.check === "pin_not_connected" && (v.items ?? []).some((i) => i.id?.startsWith(`${sym.id}.`)));
      t(mine.length === 2, `two pin_not_connected findings name ${sym.id}: ${JSON.stringify(mine.map((v) => v.hint))}`);
      // Where the definition the sheet holds puts the pins (a library symbol's y points up; the design's own Device:R wins over the library's, library_place.rs).
      const def = (await sch()).lib_symbols?.["Device:R"];
      const want = (def?.pins ?? []).map((q) => [X0 + Math.round(q.at[0] * 1000), Y0 - Math.round(q.at[1] * 1000)]).sort((a, b) => a[1] - b[1]);
      const pos = mine.map((v) => v.items[0].pos).sort((a, b) => a[1] - b[1]);
      t(want.length === 2 && JSON.stringify(pos) === JSON.stringify(want), `at the symbol's pins (${JSON.stringify(pos)}; its definition puts them at ${JSON.stringify(want)})`);
      t(/Pin not connected: Symbol R\d+ Pin [12]/.test(mine[0]?.hint ?? ""), `in KiCad's words (${mine[0]?.hint})`);
      await until(() => /\d/.test(dlg.innerText), 3000);
      t(dlg.innerText.includes(sym.id), `the dialog lists them (${dlg.innerText.slice(0, 200).replace(/\n/g, " / ")})`);
      await closeDialogs();
      await undo();
      t((await snapshot()) === base, "one undo takes the resistor away");
    });

    // ----- the end
    await scenario("the sheet is as it was and the page logged no error", async (t) => {
      t((await snapshot()) === base, "the sheet is as it was");
      const errs = __eda.errors(errorsFrom).filter((e) => !/Junction location contains no joinable wires/.test(e.message));
      t(errs.length === 0, `no errors but the junction refusal: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
