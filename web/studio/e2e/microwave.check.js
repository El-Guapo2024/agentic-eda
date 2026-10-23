// Microwave tools check (GAPS.md item 9, package P19): the five tools of Place > Draw Microwave Shapes -- Gaps, Stubs, Arc Stubs, Polygonal Shapes and Lines -- and the schematic's drop of a
// `.kicad_sch` file, driven the way a person does it through `window.__eda`, the dialogs' controls (their `data-testid`s and labels) and the pointer events of the canvas, and checked
// against what the server holds (`/api/state`, `/api/board.kicad_pcb`).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`) -- paste it
// into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: every footprint it makes it deletes again, every edit is undone, and it leaves
// Strict as it found it. `window.__microwaveOnly = /regexp/` runs only the scenarios whose names match.
//
// What it checks, by step of the package:
//   1  Gaps, Stubs and Arc Stubs: a click and the "Create Microwave Footprint" prompts (the size starting at the current track width, the arc stub's second prompt "Angle in degrees:" starting at 0.0,
//      a number that is none refused with "Incorrect number, abort", the angle clamped to a half turn), OK puts the footprint of the tool where the click was -- the pads where `createFootprint`
//      puts them, a footprint of its own with the name KiCad gives it, selected -- as one undo step; Cancel puts nothing and the tool waits for the next click until Escape; the footprints
//      are copper on the front layer only, with no library link and no courtyard in the board file kicad-cli reads; they are not held to a courtyard of their pads with Strict on;
//      and they are footprints like any other: rotated, turned over, duplicated and deleted, each one undo step, and opened in the Footprint Editor (Edit Footprint) without an error.
//   2  Lines: two clicks (the second held by the angle snap), "Length of track:" starting at the distance between the points, KiCad's four messages for a length the coil cannot have (said before
//      the value is asked), "Component value:" taking no character a footprint name cannot have, the coil of segments between pads 1 and 2 as one undo step; Escape takes the first click back.
//   3  Polygonal Shapes: the Complex Shape dialog -- Normal, Symmetrical and Mirrored, "Read Shape Description File..." (the file's scales in the Size boxes), a click that reads no file
//      ("Shape has a null size.", then "Shape has no points." once a file was read) -- and the polygon between two pads in the board file.
//   4  A schematic file dropped on the sheet runs Import Sheet with it: the files of the drop are put beside each other on the server, the sheets they place are looked for among them and the ones
//      that are missing are named, and the Choose Schematic dialog opens on the file with what Import Sheet's checks say.
//
// It answers through `window.__microwaveCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a minute or two (each edit waits for the server's answer): read it with `await __microwaveCheck.wait(40000)` (it returns when the run is done or the time is up) until
// `.done`. In a hidden pane the browser stops timers and animation frames; the script then drives both from a fetch loop.
// run-all-checks: board=mcu30   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__microwaveCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__microwaveSched) {
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
    window.__microwaveSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const redo = async () => { await E.run("common.Interactive.redo"); await sleep(700); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(500); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;
  const button = (text) => [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const waitFor = async (cond, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(150); } return false; };

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

  const only = window.__microwaveOnly instanceof RegExp ? window.__microwaveOnly : null;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  // The footprints of the tools, by their names; what the board had before the run is left alone.
  const isMicrowave = (p) => /^(muwave_|mw_inductor)/.test(p.footprint ?? "");
  const microwave = async () => (await board()).parts.filter(isMicrowave);
  const before = new Set((await microwave()).map((p) => p.ref));
  const mine = async () => (await microwave()).filter((p) => !before.has(p.ref));
  const rip = async (ref) => cmd({ op: "rip", part: ref });
  const deleteAll = async () => { for (const p of await mine()) await rip(p.ref); };
  const refs = async () => (await mine()).map((p) => p.ref);
  const trackWidth = async () => (await board()).board_rules?.track_width ?? 250;
  const pcbText = () => fetch("/api/board.kicad_pcb").then((r) => r.text());
  /** The `(footprint ..)` block of the board file that holds `(property "Reference" "<reference>"`. */
  const blockOf = (text, ref) => {
    const at = text.indexOf(`(property "Reference" "${ref}"`);
    if (at < 0) return "";
    const start = text.lastIndexOf("\n\t(footprint ", at) + 1;
    const end = text.indexOf("\n\t(footprint ", at);
    return text.slice(start, end < 0 ? text.length : end);
  };

  /** How many `(xy ..)` points the first shape `tag` of `block` has, up to the entry `until` that follows them. */
  const pointsOf = (block, tag, until) => {
    const i = block.indexOf(tag);
    if (i < 0) return 0;
    const j = block.indexOf(until, i);
    return (block.slice(i, j < 0 ? undefined : j).match(/\(xy /g) ?? []).length;
  };
  /** The four verdicts of the coil between two points 10 mm apart, and lengths in the middle of the range each holds, found by asking the server (they move with the track width). */
  const coilLengths = async (tw) => {
    const found = {};
    for (let mm = 9; mm <= 300; mm += 0.5) {
      const r = await post("/api/microwave/inductor", { start: { x: 0, y: 0 }, end: { x: 10000, y: 0 }, length_um: Math.round(mm * 1000), track_width_um: tw });
      (found[r.ok ? "ok" : r.message] ??= []).push(mm);
    }
    const mid = (k) => (found[k] ? found[k][Math.floor(found[k].length / 2)] : null);
    return { small: mid("Requested length too small"), unrepresentable: mid("Requested length can't be represented"), ok: mid("ok"), large: mid("Requested length too large"), minimum: mid("Requested length < minimum length") };
  };
  /** The text entry prompt on screen: its message, its box and what it starts at. */
  const prompt = () => {
    const dlg = document.querySelector(".dialog");
    const input = tid("mw-entry");
    return dlg && input ? { title: dlg.querySelector(".dialog-header")?.textContent.trim(), label: dlg.querySelector("label")?.firstChild?.textContent.trim(), input, text: input.value } : null;
  };
  /** Type `text` in the prompt and press OK. */
  const answer = async (text) => { setValue(tid("mw-entry"), text); await sleep(120); button("OK").click(); await sleep(900); };
  const toasts = () => E.errors().map((e) => e.message);
  /** Wait until the prompt is on screen (the click is answered by the picker, then the prompt opens). */
  const awaitPrompt = async () => { await waitFor(() => !!prompt(), 3000); return prompt(); };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strictBox = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = !!strictBox?.checked;
  const setStrict = async (on) => { if (strictBox && strictBox.checked !== on) { strictBox.click(); await sleep(400); } };
  await setStrict(false);
  let toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }
  const outline = (await board()).outline ?? [[0, 0], [35000, 0], [35000, 35000], [0, 35000]];
  const [bx0, by0] = [Math.min(...outline.map((p) => p[0])) / 1000, Math.min(...outline.map((p) => p[1])) / 1000];
  const at = (dx, dy) => [bx0 + dx, by0 + dy];
  const TW = await trackWidth();

  (async () => {
    // ------------------------------------------------------------------------------------------------------------------ 1. gaps, stubs and arc stubs
    await scenario("Draw Microwave Gaps: a click, Gap Size from the track width, OK puts two pads that far apart where the click was, selected, one undo step", async (t) => {
      t((await E.actions()).some((a) => a.id === "pcbnew.MicrowaveTool.createGap" && a.enabled !== false), "Draw Microwave Gaps is an action of the board editor");
      await E.run("pcbnew.MicrowaveTool.createGap");
      await sleep(300);
      const [x, y] = at(8, 8);
      await click(toScreen, x, y);
      const p = await awaitPrompt();
      t(!!p, "the click opens the prompt");
      if (!p) return;
      t(p.title === "Create Microwave Footprint" && p.label === "Gap Size:", `its title and message are KiCad's (${p.title} / ${p.label})`);
      t(Number(p.text) === TW / 1000, `the box starts at the current track width (${p.text} mm for ${TW} um)`);
      await answer("0.5");
      const made = await mine();
      t(made.length === 1, `OK puts one footprint on the board (${made.map((m) => m.ref)})`);
      const g = made[0];
      if (!g) return;
      t(g.footprint === "muwave_gap" && g.ref.startsWith("REF"), `a footprint of its own: ${g.footprint}, ${g.ref}`);
      t(near(g.at[0], x * 1000, 600) && near(g.at[1], y * 1000, 600), `where the click was (${g.at} for ${x * 1000}, ${y * 1000})`);
      const pads = [...g.pads].sort((a, b) => a.x - b.x);
      t(pads.length === 2 && pads[0].w === TW && pads[0].h === TW, `two square pads of the track width (${pads.map((q) => `${q.w}x${q.h}`)})`);
      t(near(pads[0].x, g.at[0] - (500 + TW) / 2, 1) && near(pads[1].x, g.at[0] + (500 + TW) / 2, 1), `at -(gap + w) / 2 and +(gap + w) / 2 from the origin (${pads.map((q) => q.x - g.at[0])})`);
      t(near(pads[1].x - pads[1].w / 2 - (pads[0].x + pads[0].w / 2), 500, 1), "so the copper is 0.5 mm apart");
      t(E.state().selection.some((s) => s.id === g.ref), "the footprint is selected");
      await undo();
      t((await mine()).length === 0, "one undo step takes it back");
      await redo();
      t((await mine()).length === 1, "and redo brings it");
      const text = await pcbText();
      const block = blockOf(text, g.ref);
      t(block.startsWith('\t(footprint "muwave_gap"'), "the board file has the footprint with no library link");
      t(/\(attr exclude_from_pos_files exclude_from_bom\)/.test(block), "left out of the position files and the BOM, neither smd nor through hole");
      t((block.match(/\(layers "F\.Cu"\)/g) ?? []).length === 2, "both pads are on the front copper and nowhere else");
      t(!/CrtYd|F\.Paste|F\.Mask/.test(block), "no courtyard, no paste, no mask opening");
      t(/\(property "Reference" "[A-Z0-9]+" \(at 0 -[\d.]+ 0\) \(layer "F\.SilkS"\)/.test(block) && /\(size 0\.\d+ 0\.\d+\) \(thickness 0\.0\d+\)/.test(block), "the Reference is on the silkscreen, the size of the track and a fifth of it thick");
      await deleteAll();
    });

    await scenario("Draw Microwave Gaps: Cancel puts nothing and the tool waits for the next click until Escape", async (t) => {
      await E.run("pcbnew.MicrowaveTool.createGap");
      await sleep(300);
      const [x, y] = at(12, 8);
      await click(toScreen, x, y);
      let p = await awaitPrompt();
      t(!!p, "the click opens the prompt");
      button("Cancel").click();
      await sleep(600);
      t((await mine()).length === 0, "Cancel puts no footprint on the board");
      await click(toScreen, x, y);
      p = await awaitPrompt();
      t(!!p, "the tool waits for another click (`IPO_REPEAT`): it opens the prompt again");
      await answer("0.3");
      t((await mine()).length === 1, "a second answer puts the footprint");
      await click(toScreen, ...at(16, 8));
      p = await awaitPrompt();
      t(!!p, "and after a footprint the tool asks for the next");
      button("Cancel").click();
      await sleep(500);
      await escape();
      await click(toScreen, ...at(20, 8));
      await sleep(500);
      t(!prompt(), "Escape ends the tool: a click opens nothing");
      await deleteAll();
    });

    await scenario("Draw Microwave Stubs: the square pad at the origin and the stub of the size asked above it", async (t) => {
      await E.run("pcbnew.MicrowaveTool.createStub");
      await sleep(300);
      const [x, y] = at(8, 12);
      await click(toScreen, x, y);
      const p = await awaitPrompt();
      t(!!p && p.label === "Stub Size:", `the prompt says Stub Size (${p?.label})`);
      if (!p) return;
      await answer("1.5");
      const s = (await mine())[0];
      t(!!s && s.footprint === "muwave_stub", `a muwave_stub (${s?.footprint})`);
      if (!s) return;
      const pad = (n) => s.pads.find((q) => q.num === n);
      t(near(pad("1").x, s.at[0], 1) && near(pad("1").y, s.at[1], 1) && pad("1").w === TW && pad("1").h === TW, "pad 1 is the track's square at the origin");
      t(pad("2").w === TW && pad("2").h === 1500, `pad 2 is as wide as the track and 1.5 mm long (${pad("2").w} x ${pad("2").h})`);
      t(near(pad("2").y, s.at[1] - (1500 + TW) / 2, 1), `above it: -(stub + h) / 2 (${pad("2").y - s.at[1]})`);
      const f = s.fields.find((q) => q.name === "Reference");
      t(f && f.w === TW && f.h === TW, `the Reference text is the track's size too (${f?.w})`);
      await deleteAll();
    });

    await scenario("Draw Microwave Arc Stubs: the radius, then the angle (starting at 0.0, clamped to a half turn, a number that is none refused)", async (t) => {
      await E.run("pcbnew.MicrowaveTool.createStubArc");
      await sleep(300);
      const [x, y] = at(8, 16);
      await click(toScreen, x, y);
      let p = await awaitPrompt();
      t(!!p && p.label === "Arc Stub Radius Value:", `the first prompt is the radius (${p?.label})`);
      if (!p) return;
      await answer("2");
      p = prompt();
      t(!!p && p.label === "Angle in degrees:" && p.text === "0.0", `the second is the angle, starting at 0.0 (${p?.label} ${p?.text})`);
      await answer("abc");
      t(toasts().some((m) => m === "Incorrect number, abort"), "a number that is none is refused with KiCad's words");
      t((await mine()).length === 0, "and nothing is put on the board");
      // the next click: 270 degrees is a half turn
      await click(toScreen, x, y);
      await awaitPrompt();
      await answer("2");
      await answer("270");
      const a = (await mine())[0];
      t(!!a && a.footprint === "muwave_arcstub" && a.pads.length === 1, `one custom pad (${a?.footprint}, ${a?.pads?.length})`);
      if (!a) return;
      t(a.pads[0].shape === "custom" || a.pads[0].custom != null, "the pad has a custom shape");
      const block = blockOf(await pcbText(), a.ref);
      const n = pointsOf(block, "(gr_poly", "(width");
      t(n === 39, `180 degrees is 180 / 5 + 3 = 39 points of the pie slice, the origin first and last (${n})`);
      t(/\(options \(clearance outline\) \(anchor rect\)\)/.test(block), "a rectangular anchor");
      await deleteAll();
      // 90 degrees: 21 points
      await E.run("pcbnew.MicrowaveTool.createStubArc");
      await sleep(300);
      await click(toScreen, x, y);
      await awaitPrompt();
      await answer("1.5");
      await answer("90");
      const b = (await mine())[0];
      const text = b ? blockOf(await pcbText(), b.ref) : "";
      t(pointsOf(text, "(gr_poly", "(width") === 21, "90 degrees is 21 points");
      await deleteAll();
    });

    await scenario("the microwave footprints are footprints: rotated, turned over, duplicated and deleted, each one undo step; Strict does not hold them to a courtyard of their pads", async (t) => {
      await cmd({ op: "microwave", verb: "stub", at: { x: Math.round(at(10, 20)[0] * 1000), y: Math.round(at(10, 20)[1] * 1000) }, size_um: 1000, track_width_um: TW });
      await sleep(900);
      const s = (await mine())[0];
      t(!!s, "a stub on the board");
      if (!s) return;
      await select([s.ref]);
      await E.run("pcbnew.InteractiveEdit.rotateCw");
      await sleep(900);
      let now = (await mine())[0];
      t(near(now.rot, 90, 0.5) || near(now.rot, 270, 0.5), `R turns it (${now.rot})`);
      await undo();
      now = (await mine())[0];
      t(now.rot === 0, "one undo step turns it back");
      await select([s.ref]);
      await E.run("pcbnew.InteractiveEdit.flip");
      await sleep(900);
      now = (await mine())[0];
      t(now.side === "bottom", `F turns it over (${now.side})`);
      t(now.fields.find((f) => f.name === "Reference").layer === "B.SilkS", "its Reference goes to the back silkscreen");
      await undo();
      await select([s.ref]);
      await E.run("common.Interactive.duplicate");
      await sleep(1200);
      await click(toScreen, ...at(16, 20)); // a duplicate is carried by the cursor until a click drops it
      await sleep(900);
      const all = await mine();
      t(all.length === 2, `Duplicate makes a second footprint (${all.map((p) => p.ref)})`);
      const copy = all.find((p) => p.ref !== s.ref);
      if (copy) {
        t(copy.footprint === s.footprint, `of the same footprint (${copy.footprint})`);
        const block = blockOf(await pcbText(), copy.ref);
        t(block.startsWith('\t(footprint "muwave_stub"') && !/CrtYd|F\.Paste|F\.Mask/.test(block), "and, in the board file, as copper only with no courtyard and no library, as the original");
      }
      await deleteAll();
      t((await mine()).length === 0, "Delete (rip) removes them");
      // Strict on: a stub at a corner of the board and a gap right beside it are placed
      await setStrict(true);
      const c1 = at(0.6, 0.6);
      await cmdStrict({ op: "microwave", verb: "gap", at: { x: Math.round(c1[0] * 1000), y: Math.round(c1[1] * 1000) }, size_um: 500, track_width_um: TW });
      await cmdStrict({ op: "microwave", verb: "gap", at: { x: Math.round(c1[0] * 1000) + 600, y: Math.round(c1[1] * 1000) + 3000 }, size_um: 500, track_width_um: TW });
      t((await mine()).length === 2, "with Strict on, footprints at the corner of the board and a few mm apart are accepted (no courtyard of their pads to refuse)");
      await setStrict(false);
      await deleteAll();
    });

    // ------------------------------------------------------------------------------------------------------------------ 2. the inductor
    await scenario("Draw Microwave Lines: two clicks, Length of track from the distance, KiCad's messages before the value, Component value, one undo step", async (t) => {
      t((await E.actions()).some((a) => a.id === "pcbnew.MicrowaveTool.createLine" && a.enabled !== false), "Draw Microwave Lines is an action of the board editor");
      await E.run("pcbnew.MicrowaveTool.createLine");
      await sleep(300);
      const [x1, y1] = at(8, 22);
      await click(toScreen, x1, y1);
      await sleep(300);
      t(!prompt(), "the first click asks nothing: it sets where the coil starts");
      // the end is held by the angle snap (45 degrees): a cursor 1 mm off the axis is on it
      const [x2, y2] = at(18, 23);
      await click(toScreen, x2, y2);
      let p = await awaitPrompt();
      t(!!p && p.label === "Length of track:", `the second click asks the length (${p?.label})`);
      if (!p) return;
      t(near(Number(p.text), 10, 0.001), `starting at the distance between the points, 10 mm (${p.text}): the end was held on the axis`);
      const L = await coilLengths(TW);
      t(L.small && L.unrepresentable && L.ok && L.large, `a length of each verdict for a ${TW} um track over 10 mm: ${JSON.stringify(L)}`);
      const before = toasts().length;
      await answer(String(L.small));
      t(toasts().slice(before).includes("Requested length too small"), `${L.small} mm over 10 is refused with 'Requested length too small' ...`);
      t(!prompt(), "... before the value is asked");
      t((await mine()).length === 0, "and nothing is put on the board");
      // the tool goes on: another pair of clicks for each of the other messages
      const refused = async (mm, message) => {
        await click(toScreen, x1, y1);
        await click(toScreen, x2, y2);
        await awaitPrompt();
        const b = toasts().length;
        await answer(String(mm));
        t(toasts().slice(b).includes(message) && !prompt(), `${mm} mm is '${message}', before the value is asked`);
      };
      await refused(L.large, "Requested length too large");
      await refused(L.unrepresentable, "Requested length can't be represented");
      await refused(L.minimum ?? 9, "Requested length < minimum length");
      // a length that works: the value is asked
      await click(toScreen, x1, y1);
      await click(toScreen, x2, y2);
      await awaitPrompt();
      await answer(String(L.ok));
      p = await awaitPrompt();
      t(!!p && p.label === "Component value:" && p.text === "L", `then "Component value:", starting at L (${p?.label} ${p?.text})`);
      if (!p) return;
      setValue(tid("mw-entry"), "L/1");
      await sleep(200);
      t(button("OK").disabled, "a value with a / in it is not taken (OK is off)");
      setValue(tid("mw-entry"), "L7");
      await sleep(200);
      button("OK").click();
      await sleep(1200);
      const made = await mine();
      t(made.length === 1 && made[0].footprint === "mw_inductor" && made[0].value === "L7", `the inductor is on the board with the value typed (${made.map((m) => `${m.footprint} ${m.value}`)})`);
      const c = made[0];
      if (!c) return;
      t(near(c.at[0], x2 * 1000, 600) && near(c.at[1], y1 * 1000, 600), `anchored at the end point, on the axis (${c.at})`);
      t(c.pads.length === 2, "two pads");
      const pad1 = c.pads.find((q) => q.num === "1");
      const pad2 = c.pads.find((q) => q.num === "2");
      t(near(pad1.x, c.at[0], 1) && near(pad2.x, x1 * 1000, 600), "pad 1 at the end point, pad 2 at the start");
      t(E.state().selection.some((s) => s.id === c.ref), "the footprint is selected");
      const text = await pcbText();
      const block = blockOf(text, c.ref);
      const lines = (block.match(/\(fp_line /g) ?? []).length;
      t(lines > 40, `the coil is many segments of the track's width on the front copper (${lines})`);
      t(new RegExp(`\\(stroke \\(width ${TW / 1000}\\) \\(type solid\\)\\) \\(layer "F\\.Cu"\\)`).test(block), "of the width of the track");
      t(block.startsWith('\t(footprint "mw_inductor"'), "named mw_inductor, with no library link");
      await undo();
      t((await mine()).length === 0, "one undo step takes it back");
      await redo();
      t((await mine()).length === 1, "and redo brings it");
      await deleteAll();
    });

    await scenario("Draw Microwave Lines: Escape takes the first click back and the tool waits for another start", async (t) => {
      await E.run("pcbnew.MicrowaveTool.createLine");
      await sleep(300);
      await click(toScreen, ...at(8, 24));
      await escape();
      await click(toScreen, ...at(9, 24));
      await sleep(400);
      t(!prompt(), "after Escape the next click is a start again: it asks nothing");
      await click(toScreen, ...at(19, 24));
      const p = await awaitPrompt();
      t(!!p && p.label === "Length of track:", "and the click after it asks the length: the tool is still there");
      button("Cancel").click();
      await sleep(500);
      t((await mine()).length === 0, "Cancel puts no coil");
      await escape();
      await click(toScreen, ...at(8, 24));
      await click(toScreen, ...at(19, 24));
      await sleep(500);
      t(!prompt(), "a second Escape ends the tool");
    });

    // ------------------------------------------------------------------------------------------------------------------ 3. the polygonal shape
    const SHAPE = "# a taper\nUnit=MM\nXScale=8\nYScale=2\n\n$COORD\n0 0.3\n0.2 1\n0.8 1\n1 0.3\n$ENDCOORD\n";
    /** Hand a file to the Complex Shape dialog's file chooser as a browser does. */
    const readShapeFile = async (name, text) => {
      const input = tid("mw-shape-file");
      const dt = new DataTransfer();
      dt.items.add(new File([text], name, { type: "text/plain" }));
      input.files = dt.files;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(900);
    };

    await scenario("Draw Microwave Polygonal Shapes: the Complex Shape dialog, the file's scales in the Size boxes, Normal, Symmetrical and Mirrored", async (t) => {
      t((await E.actions()).some((a) => a.id === "pcbnew.MicrowaveTool.createFunctionShape" && a.enabled !== false), "Draw Microwave Polygonal Shapes is an action of the board editor");
      await E.run("pcbnew.MicrowaveTool.createFunctionShape");
      await sleep(300);
      const [x, y] = at(14, 28);
      await click(toScreen, x, y);
      await waitFor(() => !!tid("mw-shape-file"), 3000);
      t(!!tid("mw-shape-file") && /Complex Shape/.test(document.querySelector(".dialog")?.textContent ?? ""), "the click opens the Complex Shape dialog");
      t(tid("mw-shape-normal")?.checked && !tid("mw-shape-symmetrical")?.checked && !tid("mw-shape-mirrored")?.checked, "Normal is chosen to begin with");
      t(tid("mw-size-x")?.value === "" && tid("mw-size-y")?.value === "", "the Size boxes are empty until a file is read");
      // a click that reads no file: the scales are KiCad's statics, 0 the first time
      button("OK").click();
      await sleep(800);
      t(toasts().includes("Shape has a null size."), "OK without a file says 'Shape has a null size.'");
      t((await mine()).length === 0, "and puts nothing");
      await click(toScreen, x, y);
      await waitFor(() => !!tid("mw-shape-file"), 3000);
      await readShapeFile("taper.txt", SHAPE);
      t(tid("mw-size-x").value === "8" && tid("mw-size-y").value === "2", `reading the file fills the Size boxes with its scales (${tid("mw-size-x").value} x ${tid("mw-size-y").value})`);
      t(/taper\.txt: 4 points/.test(tid("mw-shape-status").textContent), "and says what it read");
      tid("mw-shape-symmetrical").click();
      await sleep(150);
      button("OK").click();
      await sleep(1200);
      const made = await mine();
      t(made.length === 1 && made[0].footprint === "muwave_polygon", `the footprint is muwave_polygon (${made.map((m) => m.footprint)})`);
      const s = made[0];
      if (!s) return;
      const xs = s.pads.map((q) => q.x - s.at[0]).sort((a, b) => a - b);
      t(near(xs[0], -4000, 1) && near(xs[1], 4000, 1), `the two pads are the file's width apart (${xs})`);
      let block = blockOf(await pcbText(), s.ref);
      const n = pointsOf(block, "(fp_poly", "(stroke");
      t(/\(stroke \(width 0\) \(type solid\)\) \(fill yes\) \(layer "F\.Cu"\)/.test(block), "a filled polygon on the front copper with a stroke of 0");
      t(n === 12, `the 5 points, the one that closes it down to the line, and their mirror image (${n} points)`);
      await deleteAll();
      // Mirrored, with the size edited: the Size boxes count
      await E.run("pcbnew.MicrowaveTool.createFunctionShape");
      await sleep(300);
      await click(toScreen, x, y);
      await waitFor(() => !!tid("mw-shape-file"), 3000);
      await readShapeFile("taper.txt", SHAPE);
      tid("mw-shape-mirrored").click();
      setValue(tid("mw-size-x"), "4");
      await sleep(150);
      button("OK").click();
      await sleep(1200);
      const m = (await mine())[0];
      t(!!m, "a second shape");
      if (!m) return;
      block = blockOf(await pcbText(), m.ref);
      t(/\(pad "1" smd rect \(at -2 0 0\)/.test(block) && /\(pad "2" smd rect \(at 2 0 0\)/.test(block), "edited to 4 mm, the pads are 4 mm apart");
      const mir = /\(fp_poly \(pts \(xy -2 0\) \(xy -2 ([\d.]+)\)/.exec(block);
      t(!!mir && Number(mir[1]) > 0, `Mirrored puts the polygon below the line (y ${mir?.[1]})`);
      await deleteAll();
      // a file was read before: a dialog closed without one finds a size and no points
      await E.run("pcbnew.MicrowaveTool.createFunctionShape");
      await sleep(300);
      await click(toScreen, x, y);
      await waitFor(() => !!tid("mw-shape-file"), 3000);
      const b0 = toasts().length;
      button("OK").click();
      await sleep(800);
      t(toasts().slice(b0).includes("Shape has no points."), "OK without a file, after one was read, says 'Shape has no points.'");
    });

    await scenario("the footprints of the tools open in the Footprint Editor like any footprint of the board: Edit Footprint (Ctrl+E) loads each, the page keeps running", async (t) => {
      const [x0, y0] = at(6, 6).map((v) => Math.round(v * 1000));
      const mk = (c) => cmd({ op: "microwave", track_width_um: TW, ...c });
      await mk({ verb: "gap", at: { x: x0, y: y0 }, size_um: 500 });
      await mk({ verb: "stub", at: { x: x0 + 4000, y: y0 }, size_um: 1000 });
      await mk({ verb: "arc_stub", at: { x: x0 + 8000, y: y0 }, radius_um: 2000, angle_millideg: 60000 });
      await mk({ verb: "function_shape", at: { x: x0 + 12000, y: y0 }, file: SHAPE });
      const coil = (await coilLengths(TW)).ok;
      await mk({ verb: "inductor", start: { x: x0, y: y0 + 6000 }, end: { x: x0 + 10000, y: y0 + 6000 }, length_um: Math.round(coil * 1000), name: "L1" });
      await sleep(900);
      const made = await mine();
      t(made.length === 5, `five footprints of the tools (${made.map((p) => p.footprint)})`);
      // what the editor asks the server for: the footprint's own name, answered
      const asked = [];
      const fetched = window.fetch;
      window.fetch = async (...a) => { const r = await fetched(...a); const u = String((a[0] && a[0].url) || a[0]); if (u.includes("/api/footprint?name=")) asked.push(`${decodeURIComponent(u.split("name=")[1])} ${r.status}`); return r; };
      try {
        for (const p of made) {
          await toBoard();
          await select([p.ref]);
          const e0 = E.errors().length;
          asked.length = 0;
          const r = await E.run("pcbnew.EditorControl.EditFpInFpEditor");
          await waitFor(() => E.state().tab === "footprint" && asked.some((q) => q.startsWith(`${p.footprint} `)), 6000);
          await sleep(700);
          t(r.ok !== false && E.state().tab === "footprint", `${p.footprint}: Edit Footprint opens the Footprint Editor (${r.error ?? E.state().tab})`);
          t(asked.some((q) => q === `${p.footprint} 200`), `${p.footprint}: the editor loads it by its name (${asked.join(", ")})`);
          t(E.errors().length === e0 && !!document.querySelector(".status-bar"), `${p.footprint}: no error, the page is still there (${E.errors().slice(e0).map((q) => q.message).join("; ")})`);
        }
      } finally {
        window.fetch = fetched;
      }
      await toBoard();
      await deleteAll();
    });

    // ------------------------------------------------------------------------------------------------------------------ 4. a schematic file dropped on the sheet
    /** A schematic text that places the sheets `placed` ([name, file] pairs) and holds nothing else. */
    const schText = (placed) =>
      `(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "9e1f5a10-0000-4000-8000-00000000d001") (paper "A4") (lib_symbols) ` +
      placed
        .map(([name, file], i) => `(sheet (at ${20 + i * 40} 20) (size 25 12) (fields_autoplaced yes) (stroke (width 0.1524) (type solid)) (fill (color 0 0 0 0.0000)) (uuid "9e1f5a10-0000-4000-8000-00000000d1${String(i).padStart(2, "0")}") (property "Sheetname" "${name}" (at ${20 + i * 40} 19 0) (effects (font (size 1.27 1.27)) (justify left bottom))) (property "Sheetfile" "${file}" (at ${20 + i * 40} 33 0) (effects (font (size 1.27 1.27)) (justify left top))))`)
        .join(" ") +
      ` (sheet_instances (path "/" (page "1"))))`;
    /** A file dropped on the window: a DataTransfer holding the Files and the events a browser sends for them. */
    const dropFiles = async (files, target = document.body) => {
      const dt = new DataTransfer();
      for (const f of files) dt.items.add(new File([f.text], f.name, { type: "text/plain" }));
      const mk = (type) => new DragEvent(type, { bubbles: true, cancelable: true, composed: true, dataTransfer: dt });
      target.dispatchEvent(mk("dragenter"));
      const over = mk("dragover");
      target.dispatchEvent(over);
      await sleep(150);
      const drop = mk("drop");
      target.dispatchEvent(drop);
      await sleep(900);
      return { prevented: over.defaultPrevented && drop.defaultPrevented };
    };
    const lastToast = () => [...document.querySelectorAll("div")].find((el) => el.style.zIndex === "3000" && el.style.position === "absolute")?.textContent ?? ""; // the page's toast: a div with no class
    const chooser = () => [...document.querySelectorAll(".dialog, [role=dialog]")].find((d) => /Choose Schematic/.test(d.textContent ?? ""));
    const gotoTab = async (label) => { if (E.state().tab !== label.toLowerCase()) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === label)?.click(); await waitFor(() => E.state().tab === label.toLowerCase()); await sleep(1000); } };
    const sheetsOfSchematic = async () => (await json("/api/schematic")).sheets ?? [];

    await scenario("a schematic dropped on the sheet runs Import Sheet with it: the sheets it places are found among the files of the drop, the missing ones are named", async (t) => {
      await gotoTab("Schematic");
      const sheets0 = (await sheetsOfSchematic()).length;
      t((await E.actions()).some((a) => a.id === "eeschema.EditorControl.ddAppendFile" && a.enabled !== false), "ddAppendFile is an action the schematic editor handles");
      const main = schText([["Power", "dropped_power.kicad_sch"], ["Gone", "dropped_gone.kicad_sch"]]);
      const d = await dropFiles([{ name: "dropped_power.kicad_sch", text: schText([]) }, { name: "dropped_main.kicad_sch", text: main }]);
      t(d.prevented, "the browser does not take the files for itself");
      await waitFor(() => !!chooser(), 8000);
      t(!!chooser(), "Import Sheet's chooser, Choose Schematic, opens");
      const field = chooser()?.querySelector("input[placeholder*='kicad_sch']");
      t(!!field && /dropped_main\.kicad_sch$/.test(field.value), `on the file nobody places, whatever the order of the drop (${field?.value?.split("/").pop()})`);
      await waitFor(() => /was not found/.test(chooser()?.innerText ?? ""), 8000);
      const text = chooser()?.innerText ?? "";
      t(/The sheet file 'dropped_gone\.kicad_sch' that 'dropped_main\.kicad_sch' places was not found: its sheet comes in empty\./.test(text), "Import Sheet's check names the sheet file that is missing, with the file that places it");
      t(!/'dropped_power\.kicad_sch' that/.test(text), "and not the one that was dropped");
      t(/Not in the drop: dropped_gone\.kicad_sch \(placed by dropped_main\.kicad_sch\)\. Its sheet comes in empty\./.test(lastToast()), `the drop says so too (${lastToast()})`);
      // Open arms the placement tool with the file; two clicks size the sheet and Hierarchical Sheet Properties names it (the file's name)
      [...chooser().querySelectorAll("button")].find((b) => b.textContent.trim() === "Open")?.click();
      await sleep(600);
      t(E.state().tool === "sch_import_sheet", "Open arms Import Sheet's tool");
      const view = E.state().view;
      const at = (x, y) => [view.x + x * view.scale, view.y + y * view.scale];
      const clickSch = async (x, y, ms = 1200) => { const [px, py] = at(x, y); fire("pointermove", px, py); await sleep(150); fire("pointermove", px + 3, py + 3); await sleep(150); fire("pointerdown", px + 3, py + 3, { buttons: 1 }); await sleep(70); fire("pointerup", px + 3, py + 3); await sleep(ms); };
      await clickSch(60000, 220000, 400);
      await clickSch(140000, 270000, 1200);
      const findNamed = () => [...document.querySelectorAll(".dialog")].find((q) => /Hierarchical Sheet Properties/.test(q.textContent ?? ""));
      await waitFor(() => !!findNamed(), 8000);
      const named = findNamed();
      t(!!named, "the second click opens Hierarchical Sheet Properties");
      if (named) {
        t(named.querySelectorAll("input")[0].value === "dropped_main", `the sheet is named after the file (${named.querySelectorAll("input")[0].value})`);
        [...named.querySelectorAll("button")].find((b) => b.textContent.trim() === "Import")?.click();
        await sleep(3000);
      }
      const sheets1 = await sheetsOfSchematic();
      t(sheets1.length === sheets0 + 1 && sheets1.some((q) => q.name === "dropped_main"), `the sheet is on the schematic (${sheets0} -> ${sheets1.length})`);
      await post("/api/undo", { domain: "schematic" });
      await sleep(900);
      t((await sheetsOfSchematic()).length === sheets0, "one undo takes it away");
      // a drop of one file alone and a drop of files that are not placed by the first
      await escape();
      await dropFiles([{ name: "dropped_a.kicad_sch", text: schText([["B", "dropped_b.kicad_sch"]]) }, { name: "dropped_b.kicad_sch", text: schText([]) }, { name: "dropped_other.kicad_sch", text: schText([]) }]);
      await waitFor(() => !!chooser(), 8000);
      t(!!chooser() && /Only dropped_a\.kicad_sch is imported; dropped_other\.kicad_sch is not placed by it/.test(lastToast()), `files the first does not place are left out and said (${lastToast()})`);
      chooser()?.querySelector("button")?.click();
      await closeDialogs();
    });

    await gotoTab("PCB");
    await toBoard();
    await deleteAll();
    await setStrict(strictWas);
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();

  /** A command sent with Strict as it is on the switch (the gates judge it). */
  async function cmdStrict(c) {
    return post("/api/cmd", { cmd: c, strict: true });
  }
  async function toBoard() {
    if (E.state().tab !== "pcb") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await waitFor(() => E.state().tab === "pcb"); await sleep(700); }
  }
  return "started";
})();
