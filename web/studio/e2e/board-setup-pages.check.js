// Board Setup pages check (GAPS.md package P9a, item 3): drives the pages of the Board Setup dialog that P9a added -- Text Variables, Board Editor Layers, Formatting, Zone Hatch
// Offsets, Length-tuning Patterns, Tuning Profiles -- through the dialog's own controls and `window.__eda`, reads what the backend kept (`GET /api/state`'s `board_rules`, the
// text items' `shown` form, the schematic) and puts everything back with Undo. Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or
// `page.evaluate( src )` in Playwright -- on a scratch board (it edits and undoes). The board needs a few placed footprints; examples/mcu_board_30plus.yaml with
// `eda board place U1 --at 17.5,17.5` and a few more is the board it was written on.
//
// It answers through `window.__boardSetupPagesCheck`: { results: [{ name, ok, detail }], done, ok, summary, wait(ms) }. A run takes a minute or two (every click waits for the board's answer),
// longer than a tool call may last, so it runs on its own: start it, then read it with `await __boardSetupPagesCheck.wait(40000)` (it returns the summary
// `{ ok, failed, results }` when the run is done, or where it is when the time is up) until `.done`. In a hidden pane the browser throttles timers; the script then drives them from
// a fetch loop. The checks, in the order of the sections:
//   text variables   the page is in the tree; a name KiCad's validator refuses is marked and stops Apply; Apply stores the set as one undo step; ${NAME} in a board text, a
//                    footprint field, a schematic text and the title block reads as the value (the state's `shown`, kept beside the stored text); the same list is Configure Paths'
//                    (a variable named like a path appears there, one with a space does not and survives its OK); Undo takes each step back
//   drawing tools   with Text & Graphics > Defaults set for the silkscreen, copper, fab and edge classes: a new line, rectangle, circle, arc and polygon takes the line thickness
//                    of its layer's class (the Appearance panel's row picks the layer; the state keeps its key, "f_silks", and the item is made on "F.SilkS"), incWidth steps it and arming the tool again starts over, a text from the Text tool and a
//                    dimension take the class's size and thickness, and a text on a back layer is mirrored
//   end              no console error came up while it ran, and the board is as it was (texts, variables, overlay, shapes, dimensions, title block, footprint fields)
//
// A scenario that throws does not stop the run: its section ends with a failed check "<section> ran to the end (stopped at: <step>)" naming the step it was at (and why), the next section
// runs, and whatever happens the run ends by closing the dialogs, taking back the undo steps it made (only those that were made) and checking the board is as it was. `__boardSetupPagesCheck.at`
// is the step a run that is still going is at.
(async () => {
  const results = [];
  const run = (window.__boardSetupPagesCheck = { results, done: false, ok: false, summary: null, wait: null, at: "setup" });
  run.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return run.done ? run.summary : { done: false, n: results.length, at: run.at, last: results.slice(-2) };
  };
  if (document.visibilityState === "hidden" && !window.__boardSetupPagesSched) {
    // One scheduler for every timer, driven by a fetch loop (a hidden pane throttles timers).
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
    window.__boardSetupPagesSched = true;
  }
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail: ok ? "" : String(detail) });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const clock = "2026-10-10T21:30:05";
  const state = () => json(`/api/state?clock=${clock}`);
  const schematic = () => json(`/api/schematic?clock=${clock}`);
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const postTo = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const why = (e) => (e && e.message ? e.message : String(e));
  let at = "setup"; // the step the script is at, named when something throws
  const step = (name) => { at = run.at = name; };
  const steps = []; // the undo steps the scenarios made ("pcb" or "schematic"), taken back at the end -- only the ones that were made
  /** A command through `/api/cmd`; when it was accepted it is one undo step of `domain`. */
  const edit = async (cmd, domain = "pcb") => {
    const r = await post(cmd);
    if (r.ok) steps.push(domain);
    return r;
  };
  const setText = async (el, value) => {
    if (!el) throw new Error(`no field to type "${value}" into`);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(60);
  };
  const dialog = () => document.querySelector(".bs-dialog");
  const page = (label) => document.querySelector(`section.bs-page[aria-label="${label}"]`);
  const button = (root, text) => (root ? [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text) : undefined);
  const press = async (root, text, wait = 150) => {
    if (!root) throw new Error(`nothing open to press "${text}" in`);
    const b = button(root, text);
    if (!b) throw new Error(`no "${text}" button`);
    b.click();
    await sleep(wait);
  };
  const openPage = async (label) => {
    if (!dialog()) {
      await __eda.run("pcbnew.EditorControl.boardSetup");
      await sleep(400);
    }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith(label))?.click();
    await sleep(300);
    const p = page(label);
    if (!p) throw new Error(`Board Setup has no "${label}" page`);
    return p;
  };
  const closeDialog = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      const close = [...document.querySelectorAll(".dialog-footer button")].find((b) => ["Close", "Discard and close"].includes(b.textContent.trim()));
      close?.click();
      await sleep(250);
    }
  };
  /** Back to the Select tool with nothing half drawn. */
  const cancelTool = async () => {
    for (let i = 0; i < 3 && __eda.state().tool !== "select"; i++) { await __eda.run("common.Interactive.cancel"); await sleep(250); }
  };
  /** Whatever a scenario left open: a tool armed, the Text dialog, Board Setup, a layer picked in the Appearance panel. */
  const tidy = async () => {
    for (let i = 0; i < 3; i++) {
      const cancel = button(document.querySelector(".dialog:not(.bs-dialog)"), "Cancel");
      if (!cancel || !document.querySelector(".dialog:not(.bs-dialog)")) break;
      cancel.click();
      await sleep(250);
    }
    await cancelTool();
    await closeDialog();
    document.querySelector(".ap-root .ap-row.active[data-layer]")?.click();
    await sleep(250);
  };
  // One undo step of the editor the step was made in (the schematic's history is its own, as in KiCad).
  const undo = async (domain) => {
    const r = await postTo("/api/undo", { domain });
    await sleep(500);
    return r;
  };
  const apply = async (root) => {
    await press(root, "Apply", 1000);
    return root.querySelector(".bs-msg")?.textContent ?? "";
  };
  /** What the run must put back, as one string. */
  const fingerprint = (s) => JSON.stringify({
    variables: s.board_rules?.text_variables ?? {},
    overlay: s.board_rules?.overlay ?? [],
    texts: (s.drawings?.texts ?? []).length,
    shapes: (s.drawings?.shapes ?? []).length,
    dimensions: (s.drawings?.dimensions ?? []).length,
    title: s.title_block ?? null,
    fields: (s.parts ?? []).map((q) => [q.ref, (q.fields ?? []).map((f) => [f.name, f.text])]),
  });
  /** One scenario: whatever it throws becomes a failed check naming the step it was at, and the next one starts from a clean screen. */
  const section = async (name, body) => {
    step(name);
    try {
      await body();
    } catch (e) {
      check(`${name} ran to the end (stopped at: ${at})`, false, why(e));
    } finally {
      await tidy().catch((e) => check(`${name}: the screen was left clean (stopped at: ${at})`, false, why(e)));
    }
  };

  let errorsBefore = 0;
  let start = null;
  try {
    errorsBefore = __eda.errors().length;
    start = await state();
    const startPrint = fingerprint(start);

    // ---------------------------------------------------------------------------------------------------------------------------------- text variables
    await section("text variables", async () => {
      step("opening the Text Variables page");
      const p = await openPage("Text Variables");
      check("Text Variables is a page of Board Setup, under Text & Graphics", !!p && [...document.querySelectorAll(".bs-nav-group")].some((g) => g.textContent === "Text & Graphics"));
      step("adding two variables");
      await press(p, "Add Variable");
      const rows = () => p.querySelectorAll("tbody tr input[aria-label$=' name']").length;
      const n0 = rows();
      await press(p, "Add Variable");
      check("Add Variable adds a row", rows() === n0 + 1, `${n0} -> ${rows()}`);
      const field = (row, what) => p.querySelector(`input[aria-label="Variable ${row} ${what}"]`);
      step("typing a variable and a name KiCad refuses");
      await setText(field(n0, "name"), "P9A_REV");
      await setText(field(n0, "value"), "B7");
      await setText(field(n0 + 1, "name"), "Bad.Name");
      await sleep(100);
      check("a name with a character KiCad's validator filters out is marked and said", field(n0 + 1, "name")?.classList.contains("bs-invalid") && /cannot hold "\."/.test(p.querySelector("td.bs-error:not(:empty)")?.textContent ?? ""));
      step("Apply with a row marked");
      const refused = await apply(p);
      check("Apply stops while a row is marked, and stores nothing", /Fix the rows marked in red/.test(refused) && JSON.stringify((await state()).board_rules.text_variables ?? {}) === JSON.stringify(start.board_rules?.text_variables ?? {}), refused);
      step("fixing the name and applying");
      await setText(field(n0 + 1, "name"), "Designer name");
      await setText(field(n0 + 1, "value"), "Ada");
      const saved = await apply(p);
      if (saved === "Saved.") steps.push("pcb");
      let st = await state();
      check("Apply stores the whole set and the page is marked as set", saved === "Saved." && st.board_rules.text_variables.P9A_REV === "B7" && st.board_rules.text_variables["Designer name"] === "Ada" && st.board_rules.overlay.includes("text_variables"), JSON.stringify(st.board_rules));
      await closeDialog();

      // ${NAME} in a board text, a footprint's field, the title block and a schematic text.
      step("adding a board text with variables");
      const part = st.parts.find((q) => q.placed);
      const text = { id: "", content: "Rev ${P9A_REV} by ${Designer name} on ${LAYER} \\${P9A_REV}", at: { x: 9000, y: 28000 }, angle: 0, layer: "F.SilkS", size_um: 1000, stroke_width: 150, justify: "left", mirror: false };
      check("a board text is added", (await edit({ op: "add_text", text })).ok);
      step("adding a plain board text");
      check("a plain board text is added", (await edit({ op: "add_text", text: { ...text, content: "plain P9A text", at: { x: 9000, y: 31000 } } })).ok);
      step("adding a footprint field with variables");
      const layout = { at: { x: 0, y: -2500 }, angle: 0, size: [1000, 1000], thickness: 150, layer: "F.Fab", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, keep_upright: true, knockout: false };
      check("a footprint field with a variable is added", !!part && (await edit({ op: "edit_board_footprint", part: part.ref, fields: [{ name: "MPN", text: "ERJ-${P9A_REV} ${NET_NAME(1)} ${FOOTPRINT_NAME}", layout }] })).ok);
      step("setting a title block with a variable");
      check("a title block with a variable is set", (await edit({ op: "set_board_page", page: { paper: "A4" }, title_block: { title: "Board ${P9A_REV}", date: "", rev: "C", company: "", comments: [] } })).ok);
      await sleep(900);
      step("reading the shown texts");
      st = await state();
      const t = st.drawings.texts.find((x) => x.content.startsWith("Rev ${P9A_REV}"));
      check("a board text keeps the stored text and carries the shown one (the layer is KiCad's name for it, an escaped variable stays)", t?.shown === "Rev B7 by Ada on F.Silkscreen ${P9A_REV}", JSON.stringify(t));
      const mpn = st.parts.find((q) => q.ref === part?.ref)?.fields.find((f) => f.name === "MPN");
      check("a footprint field is shown with its variables, a net and the footprint name", /^ERJ-B7 \S+ \S+$/.test(mpn?.shown ?? ""), JSON.stringify(mpn));
      check("the title block is edited as stored and drawn expanded", st.title_block?.title === "Board ${P9A_REV}" && st.title_block?.shown?.title === "Board B7", JSON.stringify(st.title_block));
      check("a text with no variable is sent as it was", !("shown" in (st.drawings.texts.find((x) => x.content === "plain P9A text") ?? { shown: 1 })), "");
      step("adding a schematic text with variables");
      const added = await edit({ op: "add_sch_text", content: "Sheet ${#}/${##} ${SHEETPATH} ${P9A_REV} ${CURRENT_DATE}", at: { x: 15000, y: 30000 }, angle_millideg: 0, size_um: 1270 }, "schematic");
      await sleep(900);
      const sch = await schematic();
      const st1 = (sch.texts ?? []).find((x) => x.content.startsWith("Sheet ${#}"));
      check("a schematic text is shown with the page, the sheet path, the variable and the browser's day", added.ok && /^Sheet 1\/\d+ \/ B7 2026-10-10$/.test(st1?.shown ?? ""), JSON.stringify(st1));
      const canvas = document.querySelector(".pcb-canvas-container canvas");
      check("the board canvas is drawing (the shown text reaches the painter without an error)", !!canvas && __eda.errors().length === errorsBefore, JSON.stringify(__eda.errors().slice(errorsBefore)));

      // Configure Paths is the other view of the list.
      step("reading Configure Paths");
      const view = await json("/api/library_tables");
      const listed = (view.variables?.project ?? []).map((v) => v.name);
      check("Configure Paths lists the variables named like a path and not the others", listed.includes("P9A_REV") && !listed.includes("Designer name"), JSON.stringify(listed));
      step("saving Configure Paths");
      const kept = await postTo("/api/path_variables", { variables: [...(view.variables?.project ?? []), { name: "P9A_TEAM", value: "/srv/p9a" }] });
      if (kept.ok) steps.push("pcb");
      await sleep(700);
      st = await state();
      check("its OK stores through the same verb, keeping the variables it cannot list", kept.ok && st.board_rules.text_variables.P9A_TEAM === "/srv/p9a" && st.board_rules.text_variables["Designer name"] === "Ada", JSON.stringify([kept.error, st.board_rules.text_variables]));
    });

    // ---------------------------------------------------------------------------------------------------------------------------------- drawing tools
    // The canvas: the pointer events a user makes. The status bar's cursor readout gives the screen-to-board mapping.
    const canvasBox = () => document.querySelector(".pcb-canvas-container");
    const fire = (type, x, y, extra = {}) => {
      const box = canvasBox();
      const rect = box.getBoundingClientRect();
      (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
    };
    const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
    const screenOf = async () => {
      await __eda.run("common.Control.zoomFitObjects");
      await sleep(400);
      fire("pointermove", 100, 100); await sleep(200); const a = readout();
      fire("pointermove", 500, 400); await sleep(200); const b = readout();
      if (!a || !b) return null;
      const sx = (b[0] - a[0]) / 400;
      const sy = (b[1] - a[1]) / 300;
      return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
    };
    const pickLayer = async (key) => {
      if (!document.querySelector(".ap-root")) document.querySelector('button[aria-label="Show Appearance"]')?.click();
      await sleep(250);
      document.querySelector('.ap-root [data-tab="layers"]')?.click();
      await sleep(200);
      const row = document.querySelector(`.ap-root .ap-row[data-layer="${key}"]`);
      if (!row) throw new Error(`the Appearance panel has no layer row "${key}"`);
      if (!row.classList.contains("active")) row.click();
      await sleep(250);
    };
    /** Arm a drawing tool by its action (the actions toggle, so start from Select). */
    const arm = async (action, tool) => {
      await cancelTool();
      await __eda.run(action);
      await sleep(300);
      if (__eda.state().tool !== tool) throw new Error(`${action} left the tool at ${__eda.state().tool}, not ${tool}`);
    };
    const lastShape = async () => ((await state()).drawings?.shapes ?? []).at(-1);
    const shapeCount = async () => ((await state()).drawings?.shapes ?? []).length;
    const dialogNumbers = () => [...document.querySelectorAll(".dialog input[type=number]")].map((i) => Number(i.value));
    const mirrorBox = () => [...document.querySelectorAll(".dialog .kv-grid > span")].find((x) => x.textContent === "Mirrored")?.nextElementSibling;

    await section("drawing tools", async () => {
      step("mapping the canvas to the board");
      const toScreen = await screenOf();
      check("the canvas gave a cursor readout to place clicks by", !!toScreen);
      if (!toScreen) return;
      const click = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(800); };
      /** Arm a tool, click the points, and expect exactly one new shape: one undo step. */
      const draw = async (action, tool, points) => {
        const before = await shapeCount();
        await arm(action, tool);
        for (const [x, y] of points) await click(x, y);
        const after = await shapeCount();
        if (after > before) steps.push("pcb");
        if (after !== before + 1) throw new Error(`${action}: ${before} shapes before, ${after} after the clicks`);
        return lastShape();
      };
      const tg = JSON.parse(JSON.stringify((await state()).board_rules.text_graphics));
      tg.silk.line_width_um = 180; tg.silk.text_width_um = 1200; tg.silk.text_height_um = 1200; tg.silk.text_thickness_um = 200;
      tg.copper.line_width_um = 250;
      tg.fab.line_width_um = 90; tg.fab.text_width_um = 800; tg.fab.text_height_um = 800; tg.fab.text_thickness_um = 120;
      tg.others.line_width_um = 130;
      step("setting the defaults");
      check("the defaults are set for the silkscreen, copper, fab and other classes", (await edit({ op: "set_text_graphics_defaults", settings: tg })).ok);
      await sleep(900);

      // Lines, rectangles, circles, arcs and polygons: the width of the layer's class.
      step("a line on the silkscreen");
      await pickLayer("f_silks");
      let sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 4], [11, 4]]);
      check("a line on the front silkscreen is on F.SilkS, at its class's line thickness", sh?.kind === "segment" && sh.layer === "F.SilkS" && sh.stroke_width === 180, JSON.stringify(sh));
      step("incWidth");
      await __eda.run("pcbnew.InteractiveDrawing.incWidth");
      await sleep(200);
      {
        const before = await shapeCount();
        await click(3, 6); await click(11, 6);
        const after = await shapeCount();
        if (after > before) steps.push("pcb");
        sh = await lastShape();
        check("incWidth steps the width by 0.1 mm from the class's", after === before + 1 && sh?.stroke_width === 280, JSON.stringify(sh));
      }
      step("arming again");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 8], [11, 8]]);
      check("arming the tool again starts at the class's width", sh?.stroke_width === 180, JSON.stringify(sh));
      step("a rectangle");
      sh = await draw("pcbnew.InteractiveDrawing.rectangle", "draw_rect", [[14, 4], [20, 9]]);
      check("a rectangle takes it too", sh?.kind === "rect" && sh.layer === "F.SilkS" && sh.stroke_width === 180, JSON.stringify(sh));
      step("a circle");
      sh = await draw("pcbnew.InteractiveDrawing.circle", "draw_circle", [[24, 7], [27, 7]]);
      check("and a circle", sh?.kind === "circle" && sh.stroke_width === 180, JSON.stringify(sh));
      step("an arc");
      sh = await draw("pcbnew.InteractiveDrawing.arc", "draw_arc", [[14, 14], [18, 14], [14, 18]]);
      check("and an arc", sh?.kind === "arc" && sh.stroke_width === 180, JSON.stringify(sh));
      step("a line on the fabrication layer");
      await pickLayer("f_fab");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 12], [11, 12]]);
      check("a line on another layer has that class's width: the front fabrication layer's", sh?.layer === "F.Fab" && sh.stroke_width === 90, JSON.stringify(sh));
      step("a line on a user layer");
      await pickLayer("Dwgs_User");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 14], [11, 14]]);
      check("on a user layer the Others class's", sh?.layer === "Dwgs.User" && sh.stroke_width === 130, JSON.stringify(sh));
      step("a line on copper");
      await pickLayer("F.Cu");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 16], [11, 16]]);
      check("on copper the copper class's", sh?.layer === "F.Cu" && sh.stroke_width === 250, JSON.stringify(sh));

      // The Text tool: its dialog starts from the layer class's text.
      step("the Text tool on the silkscreen");
      await pickLayer("f_silks");
      await arm("pcbnew.InteractiveDrawing.text", "text");
      await click(3, 22);
      const [size, thickness] = dialogNumbers();
      check("the Text tool's dialog starts at the silkscreen class's size and thickness, not mirrored", size === 1200 && thickness === 200 && mirrorBox()?.checked === false, `${size} ${thickness} ${mirrorBox()?.checked}`);
      await setText(document.querySelector(".dialog .kv-grid input"), "P9A tool text");
      const textsBefore = ((await state()).drawings?.texts ?? []).length;
      await press(document.querySelector(".dialog"), "Add", 900);
      const t = ((await state()).drawings?.texts ?? []).find((x) => x.content === "P9A tool text");
      if (((await state()).drawings?.texts ?? []).length > textsBefore) steps.push("pcb");
      check("the text is made with them, left aligned", t?.layer === "F.SilkS" && t.size === 1200 && t.stroke_width === 200 && t.justify === "left" && t.mirror === false, JSON.stringify(t));
      step("the Text tool on a back layer");
      await pickLayer("b_fab");
      await arm("pcbnew.InteractiveDrawing.text", "text");
      await click(3, 24);
      const [size2, thickness2] = dialogNumbers();
      check("on the back fabrication layer: that class's text, mirrored", size2 === 800 && thickness2 === 120 && mirrorBox()?.checked === true, `${size2} ${thickness2} ${mirrorBox()?.checked}`);
      await press(document.querySelector(".dialog"), "Cancel", 300);

      // A dimension: the class's text size and line thickness, the arrows are the Dimensions page's.
      step("a dimension");
      await pickLayer("f_silks");
      await arm("pcbnew.InteractiveDrawing.alignedDimension", "dimension");
      const dimsBefore = ((await state()).drawings?.dimensions ?? []).length;
      await click(3, 28); await click(11, 28);
      await sleep(500);
      const dims = (await state()).drawings?.dimensions ?? [];
      if (dims.length > dimsBefore) steps.push("pcb");
      const dim = dims.at(-1);
      check("a dimension takes the class's text size and line thickness", dim?.layer === "F.SilkS" && dim.text_size_um === 1200 && dim.stroke_width === 180, JSON.stringify(dim && { layer: dim.layer, text: dim.text_size_um, line: dim.stroke_width }));
    });
  } catch (e) {
    check(`the run reached its end (stopped at: ${at})`, false, why(e));
  } finally {
    // ------------------------------------------------------------------------------------------------------------------------------------ end
    // Whatever happened: no tool armed, no dialog open, every undo step the run made taken back, the board as it was.
    try {
      step("putting the board back");
      await tidy();
      const made = steps.length;
      for (const domain of steps.reverse()) {
        const r = await undo(domain);
        if (!r.ok) check(`an undo step of the ${domain} was taken back (stopped at: ${at})`, false, JSON.stringify(r));
      }
      if (start) {
        const now = await state();
        const [was, is] = [JSON.parse(fingerprint(start)), JSON.parse(fingerprint(now))];
        const differs = Object.keys(was).filter((k) => JSON.stringify(was[k]) !== JSON.stringify(is[k]));
        check("Undo takes every step back: the variables, the texts, the shapes, the title block, the footprint fields and the overlay are as they were", differs.length === 0, `${made} steps undone; different: ${differs.map((k) => `${k} ${JSON.stringify(was[k])} -> ${JSON.stringify(is[k])}`).join("; ")}`);
      }
      check("no console errors while it ran", __eda.errors().length === errorsBefore, JSON.stringify(__eda.errors().slice(errorsBefore)));
    } catch (e) {
      check(`the board was put back (stopped at: ${at})`, false, why(e));
    }
    const failed = results.filter((r) => !r.ok);
    run.summary = { ok: failed.length === 0, failed: failed.map((r) => r.name), results };
    run.ok = run.summary.ok;
    run.done = true;
  }
  return run.summary;
})();
