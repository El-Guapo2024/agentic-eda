// The schematic editor's click-through (UI-LEFT-2 SS20 and XX4; PARITY-sch "Manual click-through needed" 7, 9, 10, 16, 17, 19, 22-23 and §62's C3-L3), walked with
// real input: every click, drag, wheel turn and key goes through the browser's own input pipeline (`window.__runCheckInput`, Playwright's mouse and keyboard
// from e2e/run-check.mjs), not through dispatched DOM events or action ids, and each step is compared with what KiCad's code does:
//   wire        W, a click near a pin snaps onto it (`SCH_LINE_WIRE_BUS_TOOL::doDrawSegments`, `EE_GRID_HELPER::BestSnapAnchor`), a click on a second pin ends the
//               wire there (`IsTerminalPoint` -> `finishSegments`); one undo step;
//   G drag      a symbol selected by a click, G, a move and a click: the wire end on its pin follows the pin (`SCH_MOVE_TOOL::Main` with `m_isDrag`), the wire's
//               other end stays; one undo step;
//   M move      the same with M: the wire stays where it was (`moveItem` of the symbol only), dangling;
//   plain drag  a press on the symbol and a move past the drag threshold drags it as G does (`EESCHEMA_SETTINGS::m_DragIsMove` false, `SCH_SELECTION_TOOL::Main`'s
//               `IsDrag` -> `SCH_ACTIONS::drag`); a press and release with no move selects only, and nothing is sent to /api/cmd;
//   group       two symbols (a click and a Shift+click) and a drag of one: both move;
//   keys        on the Schematic tab E opens Symbol Properties, U / V / F the Reference / Value / Footprint field; on the PCB tab E opens Footprint Properties,
//               F flips the footprint, V toggles the active layer (the same keys, each tab's own actions: `TOOL_DISPATCHER`'s per-frame hotkeys);
//   Y mirror    Y flips a symbol top to bottom (`SCH_SYMBOL::SetOrientation( SYM_MIRROR_X )`), and a wire then snaps onto a pin where it is drawn: the pin's place
//               after `TRANSFORM` (sch_symbol.cpp), the server's net and the canvas' pixels agree;
//   sheet pin   a sheet pin and the wire on it selected and dragged: the pin stays on the sheet's edge (`SCH_SHEET_PIN::ConstrainOnEdge`) and the wire's end goes
//               with it (`m_specialCaseSheetPins`, `performItemMove`);
//   fields      the Symbol Fields Table's Export tab: the format presets (CSV, TSV, Semicolons) change the preview, the range delimiter folds R1-R3, a column hidden
//               on the Edit tab leaves the preview, Export writes the file (kicad-cli) and says so in KiCad's words, a path outside the board is refused.
//
// Run it through e2e/run-check.mjs (it needs `__runCheckInput`) on a COPY of a board with placed footprints (`eda place examples/mcu_board_30plus.yaml`, the
// run-all-checks kind mcu30-placed). It works on the root sheet, in the empty band under the module sheets; every scenario undoes what it did.
// It answers through `window.__schClickCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=12
(async () => {
  const results = [];
  const check = (window.__schClickCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const state = () => E.state();
  const sch = () => json("/api/schematic", { cache: "no-store" });
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  // What the page sends to /api/cmd (a click that selects must send nothing).
  const posts = [];
  if (!window.__schClickFetch) {
    const real = window.fetch.bind(window);
    window.__schClickFetch = real;
    window.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input?.url ?? "";
      if (/\/api\/cmd\b/.test(url) && (init?.method ?? "GET") === "POST") posts.push({ url, body: typeof init.body === "string" ? init.body : "" });
      return real(input, init);
    };
  }

  // Real input, played by the runner.
  const input = async (ops) => {
    if (typeof window.__runCheckInput !== "function") throw new Error("no real input: run this check through e2e/run-check.mjs");
    const r = await window.__runCheckInput(ops);
    if (r !== "ok") throw new Error(`input: ${r}`);
  };
  const press = async (key, wait = 400) => { await input([{ t: "press", key }]); await sleep(wait); };
  const canvasBox = () => [...document.querySelectorAll(".pcb-canvas-container")].find((c) => c.getBoundingClientRect().width > 0) ?? null;
  const toScreen = (x, y) => { const v = state().view; const r = canvasBox().getBoundingClientRect(); return [r.left + v.x + x * v.scale, r.top + v.y + y * v.scale]; };
  const moveTo = async (x, y, steps = 6) => { const [px, py] = toScreen(x, y); await input([{ t: "move", x: px, y: py, steps }]); await sleep(200); };
  const clickAt = async (x, y, mods = [], wait = 700) => {
    const [px, py] = toScreen(x, y);
    await input([{ t: "move", x: px, y: py, steps: 4 }, { t: "pause", ms: 120 }, ...mods.map((k) => ({ t: "keydown", key: k })), { t: "down" }, { t: "pause", ms: 40 }, { t: "up" }, ...mods.map((k) => ({ t: "keyup", key: k }))]);
    await sleep(wait);
  };
  const dragFromTo = async (a, b, wait = 1200) => {
    const [ax, ay] = toScreen(a[0], a[1]);
    const [bx, by] = toScreen(b[0], b[1]);
    await input([{ t: "move", x: ax, y: ay, steps: 3 }, { t: "pause", ms: 150 }, { t: "down" }, { t: "pause", ms: 60 }, { t: "move", x: bx, y: by, steps: 14 }, { t: "pause", ms: 300 }, { t: "up" }]);
    await sleep(wait);
  };
  const undo = async () => { await press("Control+z", 900); };
  const escape = async () => { await press("Escape", 300); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry || state().dialogs.length); i++) await escape(); };
  const dialogTitle = () => [...document.querySelectorAll(".dialog")].map((d) => d.querySelector(".dialog-header")?.textContent?.trim() ?? d.getAttribute("aria-label") ?? "").pop() ?? "";
  const clickTab = async (label) => { const b = [...document.querySelectorAll("button, [role=tab]")].find((x) => x.textContent.trim() === label); if (b) { const r = b.getBoundingClientRect(); await input([{ t: "click", x: r.left + r.width / 2, y: r.top + r.height / 2 }]); } await sleep(1200); };
  const clickEl = async (el, wait = 300) => { el.scrollIntoView({ block: "nearest" }); const r = el.getBoundingClientRect(); await input([{ t: "click", x: r.left + r.width / 2, y: r.top + r.height / 2 }]); await sleep(wait); };
  /** A menu of the menu bar, then one of its items, clicked. */
  const menu = async (top, item) => {
    const m = [...document.querySelectorAll(".menubar-item")].find((x) => x.firstChild?.textContent?.trim() === top || x.textContent.trim().startsWith(top));
    if (!m) throw new Error(`no menu ${top}`);
    await clickEl(m, 300);
    const it = [...m.querySelectorAll(".menu-node-item")].find((x) => item.test(x.textContent.trim()));
    if (!it) throw new Error(`no item ${item} in ${top}: ${[...m.querySelectorAll(".menu-node-item")].map((x) => x.textContent.trim()).join(" | ")}`);
    await clickEl(it, 800);
  };
  const typeInto = async (el, text) => { await clickEl(el, 100); await input([{ t: "press", key: "Control+a" }, { t: "press", key: "Delete" }, ...(text === "" ? [] : [{ t: "type", text }])]); await sleep(400); };

  /** A library pin's connection point on the sheet: sch_symbol.cpp's `TRANSFORM` (the default `( 1, 0, 0, -1 )` -- the library's Y up -- then `SetOrientation`'s
   * rotation and mirror) applied to the pin's `(at ..)`, plus the symbol's place. Written here from KiCad's matrices, not taken from the studio's code. */
  const KI = { "0": [1, 0, 0, -1], "90": [0, -1, -1, 0], "180": [-1, 0, 0, 1], "270": [0, 1, 1, 0] };
  const pinTip = (s, lib, number) => {
    const p = lib.pins.find((q) => q.number === number && (q.unit === 0 || q.unit === (s.unit ?? 1)));
    let [x1, y1, x2, y2] = KI[String(((Math.round((s.rot ?? 0) / 90) * 90) % 360 + 360) % 360)];
    // SYM_MIRROR_X (Y key): `temp.y2 = -1` composed after: y' negated; SYM_MIRROR_Y (X key): x' negated
    if (s.mirror === "x") { x2 = -x2; y2 = -y2; }
    if (s.mirror === "y") { x1 = -x1; y1 = -y1; }
    const lx = Math.round(p.at[0] * 1e6), ly = Math.round(p.at[1] * 1e6);
    const len = Math.round(p.length_mm * 1e6);
    const d = { 0: [1, 0], 90: [0, 1], 180: [-1, 0], 270: [0, -1] }[((Math.round(p.angle_deg) % 360) + 360) % 360];
    const tf = (x, y) => [s.at[0] + x1 * x + y1 * y, s.at[1] + x2 * x + y2 * y];
    return { tip: tf(lx, ly), root: tf(lx + d[0] * len, ly + d[1] * len) };
  };
  const symOf = async (id) => (await sch()).symbols.find((s) => s.id === id);
  const tipOf = async (id, number) => { const d = await sch(); const s = d.symbols.find((q) => q.id === id); return pinTip(s, d.lib_symbols[s.lib_id], number).tip; };
  /** The middle of the symbol's body (where a press grabs the symbol, not a pin). */
  const bodyOf = async (id) => { const d = await sch(); const s = d.symbols.find((q) => q.id === id); const a = pinTip(s, d.lib_symbols[s.lib_id], "1"); const b = pinTip(s, d.lib_symbols[s.lib_id], "2"); return [Math.round((a.root[0] + b.root[0]) / 2), Math.round((a.root[1] + b.root[1]) / 2)]; };
  const wiresAt = (d, pt) => d.wires.filter((w) => w.pts.some((q) => same([q[0] ?? q.x, q[1] ?? q.y], pt)));
  const ptOf = (q) => [q[0] ?? q.x, q[1] ?? q.y];

  /** The sheet as the scenarios change it. */
  const snapshot = async () => { const s = await sch(); return JSON.stringify({ sy: s.symbols.map((x) => [x.id, x.at, x.rot, x.mirror]).sort(), w: s.wires.map((w) => [w.id, w.pts]).sort(), sh: s.sheets.map((x) => [x.id, x.at, (x.pins ?? []).map((p) => [p.name, p.at])]) }); };
  let base = null;
  let entryNote = () => {};
  const restore = async () => { for (let i = 0; i < 12; i++) { if ((await snapshot()) === base) return i; await E.run("common.Interactive.undo"); await sleep(700); } return -1; };
  const scenario = async (name, fn, { keepState = false } = {}) => {
    const only = window.__schClickOnly ?? (new URLSearchParams(location.hash.slice(1)).get("only") ? new RegExp(new URLSearchParams(location.hash.slice(1)).get("only")) : null); // `#only=<regexp>` in the page's address runs some scenarios only
    if (only && !only.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    entryNote = (msg) => entry.notes.push(`note: ${msg}`);
    try { if (state().tab !== "schematic") await clickTab("Schematic"); await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await toSelect(); } catch { /* ignore */ }
    if (!keepState && base) {
      const left = await restore();
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) entry.notes.push(`note: ${left} edit(s) left behind were undone`);
    }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- setup
  const G = 1_270_000;
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (state().tab !== "schematic") await clickTab("Schematic");
  while (state().sheetPath.length > 0) { await E.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
  // Two resistors and a third for the mirror, in the empty band under the module sheets (on the 50 mil grid).
  const A = "R971", B = "R972", C = "R973";
  const at = { [A]: [70 * G, 104 * G], [B]: [82 * G, 104 * G], [C]: [96 * G, 104 * G] };
  for (const id of [A, B, C]) {
    const r = await cmd({ op: "add_symbol", id, lib_id: "Device:R", at: { x: at[id][0], y: at[id][1] }, rot_millideg: 0, value: "10k", footprint: "", unit: 1 });
    if (r.ok === false) { results.push({ name: "setup", ok: false, notes: [`FAIL: add_symbol ${id}: ${JSON.stringify(r)}`] }); check.done = true; return; }
  }
  await sleep(1000);
  // The view: fit the page, then the wheel (real) over the band until 1 mm is about 14 px.
  await E.run("common.Control.zoomFitScreen");
  await sleep(600);
  if (!(state().view?.scale > 1e-7)) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (${JSON.stringify(state().view)})`] }); check.done = true; return; }
  for (let i = 0; i < 30 && state().view.scale < 1.4e-5; i++) { await moveTo(83 * G, 108 * G, 1); await input([{ t: "wheel", dy: -120 }]); await sleep(250); }
  await sleep(500);
  base = await snapshot();
  const tipsAB = { a2: await tipOf(A, "2"), b2: await tipOf(B, "2") };

  // One wire from A's pin 2 to B's pin 2, drawn with W; used by the drag scenarios (made again by each, as each scenario undoes what it did).
  const drawWire = async (t, nearOffset = Math.round(0.4 * G)) => {
    const s0 = await sch();
    await moveTo(tipsAB.a2[0] + nearOffset, tipsAB.a2[1] + nearOffset);
    await press("w", 300);
    t(state().tool === "sch_wire" || /wire/i.test(state().tool ?? ""), `W arms the wire tool (${state().tool})`);
    await clickAt(tipsAB.a2[0] + nearOffset, tipsAB.a2[1] + nearOffset, [], 400);
    await clickAt(tipsAB.a2[0], tipsAB.a2[1] + 4 * G, [], 300);
    await clickAt(tipsAB.b2[0], tipsAB.a2[1] + 4 * G, [], 300);
    await clickAt(tipsAB.b2[0] - nearOffset, tipsAB.b2[1] + nearOffset, [], 1200);
    const s1 = await sch();
    const made = s1.wires.filter((w) => !s0.wires.some((x) => x.id === w.id));
    return { s1, made };
  };

  (async () => {
    // ----- 7: the wire tool
    await scenario("W: a click near a pin starts the wire on it, a click near a second pin ends the wire there; the pins share one net; one undo", async (t) => {
      const { s1, made } = await drawWire(t);
      const ends = made.flatMap((w) => w.pts.map(ptOf));
      t(made.length >= 1, `wire segments made (${made.length}: ${JSON.stringify(made.map((w) => w.pts))})`);
      t(ends.some((p) => same(p, tipsAB.a2)), `one end is exactly on ${A}'s pin 2 ${JSON.stringify(tipsAB.a2)} (the snap to the pin)`);
      t(ends.some((p) => same(p, tipsAB.b2)), `one end is exactly on ${B}'s pin 2 ${JSON.stringify(tipsAB.b2)}`);
      const na = s1.symbols.find((s) => s.id === A).pins.find((p) => p.number === "2").net, nb = s1.symbols.find((s) => s.id === B).pins.find((p) => p.number === "2").net;
      t(na === nb && !/^unconnected/.test(na ?? ""), `the two pins are on one net (${na}, ${nb})`);
      t(/wire/i.test(state().tool ?? "") && !state().carry, `the click on the pin ended the wire and the tool stays armed (${state().tool})`);
      await toSelect();
      await undo();
      t((await sch()).wires.length === s1.wires.length - made.length, "one Ctrl+Z takes the whole wire away");
    });

    // ----- 9: G drags the wire end along, M leaves it
    await scenario("G: a symbol clicked, G, a move and a click: the wire end on its pin follows the pin, the other end stays; one undo", async (t) => {
      const { made } = await drawWire(t);
      await toSelect();
      const body = await bodyOf(A);
      await clickAt(body[0], body[1]);
      t(same(state().selection.map((s) => s.id ?? s), [A]) || state().selection.some((s) => (s.id ?? s) === A), `the click selects ${A} (${JSON.stringify(state().selection)})`);
      await press("g", 300);
      await moveTo(body[0] - 3 * G, body[1] - 2 * G, 10);
      await sleep(400);
      await clickAt(body[0] - 3 * G, body[1] - 2 * G, [], 1200);
      const d = await sch();
      const s = d.symbols.find((q) => q.id === A);
      t(same(s.at, [at[A][0] - 3 * G, at[A][1] - 2 * G]), `${A} moved by (-3, -2) grid (${JSON.stringify(s.at)})`);
      const tip = pinTip(s, d.lib_symbols[s.lib_id], "2").tip;
      t(wiresAt(d, tip).length >= 1, `a wire end is on the pin's new place ${JSON.stringify(tip)} (${JSON.stringify(d.wires.filter((w) => made.some((m) => m.id === w.id) || !base.includes(w.id)).map((w) => w.pts))})`);
      t(wiresAt(d, tipsAB.b2).length >= 1, `the wire still ends on ${B}'s pin ${JSON.stringify(tipsAB.b2)}`);
      t(wiresAt(d, tipsAB.a2).length === 0, "nothing is left at the pin's old place");
      const na = s.pins.find((p) => p.number === "2").net, nb = d.symbols.find((q) => q.id === B).pins.find((p) => p.number === "2").net;
      t(na === nb && !/^unconnected/.test(na ?? ""), `still one net (${na}, ${nb})`);
      await undo();
      const back = await symOf(A);
      t(same(back.at, at[A]) && wiresAt(await sch(), tipsAB.a2).length >= 1, "one Ctrl+Z puts the symbol and the wire back");
    });

    await scenario("M: the same with M moves the symbol only; the wire stays where it was, dangling", async (t) => {
      const { made } = await drawWire(t);
      await toSelect();
      const body = await bodyOf(A);
      await clickAt(body[0], body[1]);
      await press("m", 300);
      await moveTo(body[0] - 3 * G, body[1] - 2 * G, 10);
      await sleep(400);
      await clickAt(body[0] - 3 * G, body[1] - 2 * G, [], 1200);
      const d = await sch();
      const s = d.symbols.find((q) => q.id === A);
      t(same(s.at, [at[A][0] - 3 * G, at[A][1] - 2 * G]), `${A} moved (${JSON.stringify(s.at)})`);
      t(made.every((m) => same(d.wires.find((w) => w.id === m.id)?.pts, m.pts)), "every wire segment is where it was");
      t(/^unconnected/.test(s.pins.find((p) => p.number === "2").net ?? ""), `the moved pin is unconnected (${s.pins.find((p) => p.number === "2").net}; wires ${JSON.stringify(d.wires.filter((w) => !JSON.parse(base).w.some((b) => b[0] === w.id)).map((w) => w.pts))}; pin at ${JSON.stringify(pinTip(s, d.lib_symbols[s.lib_id], "2").tip)})`);
      await undo();
      t(same((await symOf(A)).at, at[A]), "one Ctrl+Z puts it back");
    });

    // ----- 10: a plain drag, a plain click, a group
    await scenario("a press on a symbol and a move drags it as G does (the default, KiCad's m_DragIsMove off); a click with no move selects and sends nothing", async (t) => {
      const { made } = await drawWire(t);
      await toSelect();
      await clickAt(60 * G, 125 * G); // empty: nothing selected
      const body = await bodyOf(A);
      const n0 = posts.length;
      await clickAt(body[0], body[1]);
      t(state().selection.some((s) => (s.id ?? s) === A), `a click selects ${A} (${JSON.stringify(state().selection)})`);
      t(posts.length === n0, `nothing is posted to /api/cmd (${posts.slice(n0).map((p) => p.body.slice(0, 80))})`);
      await clickAt(60 * G, 125 * G);
      await dragFromTo(body, [body[0] - 3 * G, body[1] - 2 * G]);
      const d = await sch();
      const s = d.symbols.find((q) => q.id === A);
      t(same(s.at, [at[A][0] - 3 * G, at[A][1] - 2 * G]), `the drag moved ${A} by (-3, -2) grid (${JSON.stringify(s.at)})`);
      const tip = pinTip(s, d.lib_symbols[s.lib_id], "2").tip;
      t(wiresAt(d, tip).length >= 1 && wiresAt(d, tipsAB.a2).length === 0, `the wire end followed the pin to ${JSON.stringify(tip)} (${JSON.stringify(d.wires.filter((w) => !made.some((m) => same(m.pts, w.pts))).map((w) => w.pts))})`);
      await undo();
      t(same((await symOf(A)).at, at[A]), "one Ctrl+Z puts it back");
    });

    await scenario("group: a click and a Shift+click select two symbols; a drag of one moves both", async (t) => {
      await toSelect();
      const ba = await bodyOf(A), bb = await bodyOf(B);
      await clickAt(ba[0], ba[1]);
      await clickAt(bb[0], bb[1], ["Shift"]);
      const ids = state().selection.map((s) => s.id ?? s);
      t(ids.includes(A) && ids.includes(B), `both selected (${JSON.stringify(ids)})`);
      await dragFromTo(bb, [bb[0] + 2 * G, bb[1] + 3 * G]);
      const d = await sch();
      const sa = d.symbols.find((q) => q.id === A), sb = d.symbols.find((q) => q.id === B);
      t(same(sa.at, [at[A][0] + 2 * G, at[A][1] + 3 * G]) && same(sb.at, [at[B][0] + 2 * G, at[B][1] + 3 * G]), `both moved by (2, 3) grid (${JSON.stringify([sa.at, sb.at])})`);
      await undo();
      const d2 = await sch();
      t(same(d2.symbols.find((q) => q.id === A).at, at[A]) && same(d2.symbols.find((q) => q.id === B).at, at[B]), "one Ctrl+Z puts both back");
    });

    // ----- 16, 17: the same keys, each tab's own actions
    await scenario("keys on the Schematic tab: E opens Symbol Properties, U / V / F the Reference, Value and Footprint fields", async (t) => {
      await toSelect();
      const body = await bodyOf(A);
      await clickAt(body[0], body[1]);
      const want = { e: /^Symbol Properties/, u: /Reference/, v: /Value/, f: /Footprint/ };
      for (const [k, re] of Object.entries(want)) {
        await press(k, 700);
        const title = dialogTitle();
        t(re.test(title), `${k.toUpperCase()} opens "${title}"`);
        if (k !== "e") {
          const field = [...document.querySelectorAll(".dialog input, .dialog textarea")].find((x) => x.offsetParent && document.activeElement === x);
          t(!!field, `the field's text box has the focus, ready for typing (${document.activeElement?.tagName} ${document.activeElement?.getAttribute("aria-label") ?? ""})`);
          if (k === "v") t(field?.value === "10k", `it holds the value (${field?.value})`);
        }
        await escape();
        t(state().dialogs.length === 0, `Escape closes it (${state().dialogs})`);
      }
      t((await snapshot()) === base, "nothing changed");
    });

    await scenario("keys on the PCB tab: E opens Footprint Properties, F flips the footprint (one undo), V toggles the active layer F.Cu / B.Cu", async (t) => {
      await clickTab("PCB");
      t(state().tab === "pcb", `the PCB tab (${state().tab})`);
      const parts = async () => (await json("/api/state", { cache: "no-store" })).parts ?? [];
      const fp = (await parts()).find((f) => f.placed && /^R\d+$/.test(f.ref ?? ""));
      const ref = fp?.ref;
      if (!t(!!ref, `a placed footprint (${ref})`)) return;
      await E.run("common.Control.zoomFitScreen");
      await sleep(600);
      await E.run("common.InteractiveSelection.clear");
      await E.run("common.InteractiveSelection.selectItems", [ref]);
      await sleep(400);
      const cv = canvasBox().getBoundingClientRect();
      await input([{ t: "move", x: cv.left + cv.width / 2, y: cv.top + cv.height / 2 }]); // the pointer on the canvas, where the keys go
      await press("e", 900);
      t(/Footprint Properties/.test(dialogTitle()), `E opens "${dialogTitle()}"`);
      await escape();
      await escape();
      await E.run("common.InteractiveSelection.selectItems", [ref]);
      await sleep(300);
      const side0 = fp.side;
      await press("f", 1200);
      const side1 = (await parts()).find((f) => f.ref === ref)?.side;
      t(side1 && side1 !== side0, `F flips ${ref} (${side0} -> ${side1})`);
      await undo();
      t((await parts()).find((f) => f.ref === ref)?.side === side0, "one Ctrl+Z on the PCB tab flips it back");
      await E.run("common.InteractiveSelection.clear");
      const l0 = state().activeLayer ?? "F.Cu";
      await press("v", 500);
      const l1 = state().activeLayer;
      t(l0 === "F.Cu" ? l1 === "B.Cu" : l1 === "F.Cu", `V toggles the active layer of the pair (${l0} -> ${l1})`);
      await press("v", 500);
      t(state().activeLayer === l0, `V again goes back (${state().activeLayer})`);
      t((await snapshot()) === base, "the schematic did not change");
      await clickTab("Schematic");
    });

    // ----- 19: Y mirrors top to bottom, and a wire snaps where the pin is drawn
    await scenario("Y flips a symbol top to bottom, X then flips it left to right instead; a wire then snaps onto the mirrored pin where the canvas draws it", async (t) => {
      await toSelect();
      const body0 = await bodyOf(C);
      await clickAt(body0[0], body0[1]);
      await press("y", 1000);
      let s = await symOf(C);
      t(s.mirror === "x" || s.mirrored === true, `Y mirrors ${C} (SYM_MIRROR_X: mirror ${s.mirror})`);
      await press("x", 1000);
      s = await symOf(C);
      t(s.mirror === "y" || (s.mirror == null && s.rot === 180), `X on the Y-mirrored symbol leaves it mirrored left-right instead (mirror ${s.mirror}, rot ${s.rot}); KiCad's SetOrientation( SYM_MIRROR_Y ) on top of SYM_MIRROR_X`);
      await undo();
      s = await symOf(C);
      t(s.mirror === "x", `one Ctrl+Z takes the X back off (mirror ${s.mirror})`);
      const d = await sch();
      const lib = d.lib_symbols[s.lib_id];
      const p1 = pinTip(s, lib, "1"), p2 = pinTip(s, lib, "2");
      const unm = pinTip({ ...s, mirror: null }, lib, "1");
      t(!same(p1.tip, unm.tip), `pin 1 is somewhere else than before the mirror (${JSON.stringify(unm.tip)} -> ${JSON.stringify(p1.tip)})`);
      // the canvas: pixels on pin 1's line (between its tip and its root) are drawn, in the pin's colour, not the background
      await toSelect();
      await clickAt(60 * G, 125 * G);
      await sleep(400);
      const cv = canvasBox().querySelector("canvas");
      const ctx = cv.getContext("2d");
      const rect = cv.getBoundingClientRect();
      const px = (x, y) => { const [sx, sy] = toScreen(x, y); const k = cv.width / rect.width; const d2 = ctx.getImageData(Math.round((sx - rect.left) * k) - 1, Math.round((sy - rect.top) * k) - 1, 3, 3).data; let best = 0; for (let i = 0; i < d2.length; i += 4) best = Math.max(best, Math.abs(d2[i] - d2[0]) + Math.abs(d2[i + 1] - d2[1]) + Math.abs(d2[i + 2] - d2[2])); return [d2[4 * 4], d2[4 * 4 + 1], d2[4 * 4 + 2]]; };
      const bg = px(60 * G, 125 * G);
      const mid = [Math.round((p1.tip[0] + p1.root[0]) / 2), Math.round((p1.tip[1] + p1.root[1]) / 2)];
      const onPin = px(mid[0], mid[1]);
      const differs = (a, b) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 60;
      t(differs(onPin, bg), `the canvas draws pin 1's line at its mirrored place ${JSON.stringify(mid)} (pixel ${onPin}, background ${bg})`);
      // the wire: W near pin 1's mirrored tip, a click, a click away, End
      const near = [p1.tip[0] + Math.round(0.4 * G), p1.tip[1] - Math.round(0.3 * G)];
      const s0 = await sch();
      await moveTo(near[0], near[1]);
      await press("w", 300);
      await clickAt(near[0], near[1], [], 400);
      const away = [p1.tip[0] + 6 * G, p1.tip[1]];
      await clickAt(away[0], away[1], [], 300);
      await press("End", 1200);
      await toSelect();
      const d1 = await sch();
      const made = d1.wires.filter((w) => !s0.wires.some((x) => x.id === w.id));
      t(made.some((w) => w.pts.map(ptOf).some((p) => same(p, p1.tip))), `the wire starts exactly on the mirrored pin 1 ${JSON.stringify(p1.tip)} (${JSON.stringify(made.map((w) => w.pts))})`);
      // the server's connectivity: the wire lists the pins it reaches (a pin and a dangling wire are still `unconnected-(..)`, KiCad's single-pin net)
      const reached = made.flatMap((w) => w.pins ?? []);
      t(reached.includes(`${C}.1`) && !reached.includes(`${C}.2`), `the server has the wire on ${C}'s pin 1, not pin 2 (${JSON.stringify(reached)}): the snap, the drawing and the netlist agree`);
      void p2;
    });

    // ----- §62 C3-L3: a sheet pin and its wire dragged together
    await scenario("a sheet pin and the wire on it, selected together and dragged: the pin slides along the sheet's edge and the wire's end stays on it (m_specialCaseSheetPins)", async (t) => {
      await toSelect();
      const d0 = await sch();
      // a sheet pin with a wire on it (the module sheets' pins are wired to each other on the root)
      let sheet = null, pin = null, wire = null;
      for (const sh of d0.sheets) for (const p of sh.pins ?? []) { const w = d0.wires.find((x) => x.pts.map(ptOf).some((q) => same(q, ptOf(p.at))) && x.pts.length === 2); if (!wire && w) { sheet = sh; pin = p; wire = w; } }
      if (!t(!!wire, `a sheet pin with a wire on it (${pin?.name} of ${sheet?.name})`)) return;
      const pat = ptOf(pin.at);
      const [sx, sy] = ptOf(sheet.at), [sw, shh] = sheet.size;
      // the edge the pin is on, and outward from it
      const out = pat[0] === sx ? [-1, 0] : pat[0] === sx + sw ? [1, 0] : pat[1] === sy ? [0, -1] : [0, 1];
      const far = ptOf(wire.pts.map(ptOf).find((q) => !same(q, pat)));
      await E.run("common.Control.zoomFitScreen");
      await sleep(500);
      for (let i = 0; i < 30 && state().view.scale < 1.4e-5; i++) { await moveTo(pat[0], pat[1], 1); await input([{ t: "wheel", dy: -120 }]); await sleep(250); }
      // the pin by a click on it, the wire by a Shift+click on its middle
      await clickAt(pat[0] - out[0] * Math.round(0.6 * G), pat[1] - out[1] * Math.round(0.6 * G));
      const mid = [Math.round((pat[0] + far[0]) / 2), Math.round((pat[1] + far[1]) / 2)];
      await clickAt(mid[0], mid[1], ["Shift"]);
      const sel = state().selection;
      t(sel.length === 2, `two items selected, the pin and the wire (${JSON.stringify(sel)})`);
      // a drag along the edge and outward (2 grid along, 3 out): the pin can only go along
      const along = out[0] === 0 ? [1, 0] : [0, 1];
      const delta = [along[0] * 2 * G + out[0] * 3 * G, along[1] * 2 * G + out[1] * 3 * G];
      await press("g", 300);
      await moveTo(mid[0] + delta[0], mid[1] + delta[1], 12);
      await sleep(400);
      await clickAt(mid[0] + delta[0], mid[1] + delta[1], [], 1300);
      const d2 = await sch();
      const sh2 = d2.sheets.find((s) => s.id === sheet.id);
      const pin2 = (sh2.pins ?? []).find((p) => p.name === pin.name);
      const pat2 = ptOf(pin2.at);
      t(same(pat2, [pat[0] + along[0] * 2 * G, pat[1] + along[1] * 2 * G]), `the pin slid along the edge only (${JSON.stringify(pat)} -> ${JSON.stringify(pat2)})`);
      const w2 = d2.wires.find((w) => w.id === wire.id) ?? d2.wires.find((w) => w.pts.map(ptOf).some((p) => same(p, pat2)));
      t(!!w2 && w2.pts.map(ptOf).some((p) => same(p, pat2)), `the wire's end is on the pin's new place (${JSON.stringify(w2?.pts)})`);
      entryNote(`the wire's other end: ${JSON.stringify(far)} -> ${JSON.stringify(w2 && w2.pts.map(ptOf).find((p) => !same(p, pat2)))}`);
      await undo();
    });

    // ----- 23: the Fields Table's Export tab
    await scenario("Symbol Fields Table, Export tab: the presets change the preview, the range delimiter folds R1-R3, a hidden column leaves the preview, Export writes the file", async (t) => {
      await toSelect();
      await menu("Tools", /^Bulk Edit Symbol Fields/);
      const D = () => [...document.querySelectorAll(".dialog")].find((d) => /Symbol Fields Table/.test(d.querySelector(".dialog-header")?.textContent ?? ""));
      if (!t(!!(await until(D, 6000)), `Tools > Bulk Edit Symbol Fields... opens the Symbol Fields Table (${state().dialogs})`)) return;
      const tabBtn = (name) => [...D().querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === name);
      await clickEl(tabBtn("Export"), 800);
      const preview = () => D().querySelector("textarea")?.value ?? "";
      const fmtBox = () => D().querySelector('[data-testid="fields-table-fmt-preset"]');
      // a <select> by the keyboard: focused, Home, then ArrowDown to the option (a closed select changes its value, and fires change, on each arrow)
      const pick = async (name) => { const sel = fmtBox(); sel.focus(); const i = [...sel.options].findIndex((o) => o.value === name); const k = sel.selectedIndex; const n = i - k; if (n !== 0) await input(Array.from({ length: Math.abs(n) }, () => ({ t: "press", key: n > 0 ? "ArrowDown" : "ArrowUp" }))); await sleep(900); };
      const label = (re) => [...D().querySelectorAll(".kv-grid > span")].find((s) => re.test(s.textContent.trim()));
      const inputAfter = (re) => label(re)?.nextElementSibling;
      t(!!label(/^Field delimiter:$/) && !!label(/^String delimiter:$/) && !!label(/^Reference delimiter:$/) && !!label(/^Range delimiter:$/) && !!label(/^Output file:$/) && /Format presets:/.test(D().textContent) && /Preview:/.test(D().textContent), `KiCad's labels: ${[...D().querySelectorAll(".kv-grid > span")].map((s) => s.textContent.trim()).join(" | ")}`);
      await until(() => preview().length > 0, 6000);
      await pick("CSV");
      const csv = preview();
      const firstData = csv.split("\n")[1] ?? "";
      t(/^"/.test(csv) && /","/.test(csv.split("\n")[0]), `CSV quotes every field with " and separates with , (${csv.split("\n")[0].slice(0, 80)})`);
      await pick("TSV");
      const tsv = preview();
      t(!/"/.test(tsv.split("\n")[0]) && /\t/.test(tsv.split("\n")[0]), `TSV: tabs, no quotes (${JSON.stringify(tsv.split("\n")[0].slice(0, 60))})`);
      await pick("Semicolons");
      const semi = preview();
      t(/^'[^']*';'/.test(semi.split("\n")[0]), `Semicolons: ; between fields quoted with ' (BOM_FMT_PRESET::Semicolons) (${semi.split("\n")[0].slice(0, 60)})`);
      await pick("CSV");
      // the range delimiter: "-" folds a run of three or more
      const range = D().querySelector('input[aria-label="Range delimiter"]');
      t(range?.title === "Leave blank to disable ranges.", `the range delimiter's tooltip (${range?.title})`);
      await typeInto(range, "-");
      await sleep(800);
      const folded = preview();
      t(/R\d+-R\d+/.test(folded), `with "-" a run of references is folded (${(folded.match(/"[^"]*R\d+-R\d+[^"]*"/) ?? [""])[0].slice(0, 60)})`);
      await typeInto(range, "");
      await sleep(800);
      t(!/R\d+-R\d+/.test(preview()), "empty: every reference is listed (\"Leave blank to disable ranges.\")");
      // a column hidden on the Edit tab leaves the preview
      await clickEl(tabBtn("Edit"), 600);
      const showDs = D().querySelector('input[aria-label="Show Datasheet"]') ?? D().querySelector('input[aria-label^="Show "]');
      const colName = showDs?.getAttribute("aria-label").replace(/^Show /, "");
      const wasShown = showDs?.checked;
      if (showDs?.checked) await clickEl(showDs, 400);
      t(wasShown === true && showDs?.checked === false, `the Show box of "${colName}" is cleared by the click (${wasShown} -> ${showDs?.checked}; under the pointer: ${(() => { const r = showDs?.getBoundingClientRect(); const e = r && document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2); return e ? e.tagName + " " + (e.getAttribute("aria-label") ?? "") : "?"; })()})`);
      await clickEl(tabBtn("Export"), 900);
      t(!!colName && !preview().split("\n")[0].includes(colName), `"${colName}" hidden on the Edit tab is not in the preview's header (${preview().split("\n")[0].slice(0, 100)})`);
      await clickEl(tabBtn("Edit"), 500);
      const showAgain = D().querySelector(`input[aria-label="Show ${colName}"]`); // the Edit page is drawn anew
      if (showAgain && !showAgain.checked) await clickEl(showAgain, 400);
      t(!!showAgain?.checked, `ticked again (${showAgain?.checked})`);
      await clickEl(tabBtn("Export"), 900);
      // Export: the default path, the file written by kicad-cli, KiCad's message
      const out = D().querySelector('input[aria-label="Output file"]');
      t(/\.csv$/.test(out?.value ?? ""), `the output file has a default (${out?.value})`);
      const shown = preview();
      const replies = [];
      const realFetch = window.fetch;
      window.fetch = async (i, init) => { const r = await realFetch(i, init); const u = typeof i === "string" ? i : i?.url ?? ""; if (/\/api\/sch\/bom\b/.test(u)) replies.push(await r.clone().json().catch(() => null)); return r; };
      const exportBtn = [...D().querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Export");
      await clickEl(exportBtn, 300);
      const msg = await until(() => [...D().querySelectorAll("div")].map((d) => d.textContent).find((x) => /^Wrote BOM output to '/.test(x) || /fail/i.test(x)), 60000);
      t(/^Wrote BOM output to '.*\.csv'/.test(msg ?? ""), `Export says "${msg}"`);
      const rep = replies.pop();
      t(!!rep?.ok && typeof rep.text === "string", `the server wrote ${JSON.stringify(rep?.files)}`);
      const norm = (s) => s.replace(/\r\n/g, "\n").trimEnd();
      t(!!rep?.text && norm(rep.text) === norm(shown), `the file holds what the preview shows (${rep?.text?.length} / ${shown.length} chars)${rep?.text && norm(rep.text) !== norm(shown) ? (() => { const a = norm(rep.text), b = norm(shown); let i = 0; while (i < a.length && a[i] === b[i]) i++; return `: from char ${i}, file ${JSON.stringify(a.slice(Math.max(0, i - 40), i + 120))} preview ${JSON.stringify(b.slice(Math.max(0, i - 40), i + 120))}`; })() : ""}`);
      check.bomFile = rep?.files?.[0] ?? null;
      // a path out of the board's folder is refused
      await typeInto(out, "../x.csv");
      await clickEl(exportBtn, 300);
      const refused = await until(() => [...D().querySelectorAll("div")].map((d) => d.textContent).find((x) => /relative path|refused|outside|must be/i.test(x)), 20000);
      t(!!refused, `"../x.csv" is refused (${refused})`);
      window.fetch = realFetch;
      const close = [...D().querySelectorAll(".dialog-footer button")].find((b) => /^(Close|Cancel)$/.test(b.textContent.trim()));
      if (close) await clickEl(close, 400);
    });

    // ----------------------------------------------------------------------------------------------------------------------- clean-up
    for (const id of [A, B, C]) await cmd({ op: "delete_symbol", id });
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
