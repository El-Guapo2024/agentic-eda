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
//   board layers     Board Editor Layers is a page of Board Setup, in KiCad's row order with the courtyards, the outline, the margin and the copper layers locked; Add User Defined Layer adds a row;
//                    a name another layer has is marked and stops Apply; Apply stores the layers, the names and the types as one undo step; the Appearance panel's rows, the Text tool's layer list and
//                    ${LAYER} follow the names; an item can be put on a user layer; a layer that holds an item cannot be turned off; one that does not is gone from the lists and takes no new item
//   setup pages      the page tree has KiCad's order; Formatting (a ratio of 0 is marked and stops Apply; Apply stores the ratios and the footprint boxes), Zone Hatch Offsets (an offset per copper layer, a
//                    metre too far is marked), Length-tuning Patterns (a minimum above the maximum is marked; Apply stores the three groups; the length tuner opens at the maximum amplitude and the
//                    spacing of its kind) and Tuning Profiles (a profile with no name is marked; a new row takes the layers KiCad gives it; a layer named twice stops Apply; a class may name a profile that
//                    is not defined and the page says so) each store one undo step
//   net classes      a pattern is a wildcard pattern or a regular expression (the list of the nets it matches says which nets), a net several classes match is said to be in all of them with its
//                    composite class's name, the patterns are stored as typed, and moving a class changes the order they are kept in (a class's place in the table is its priority)
//   end              no console error came up while it ran, and the board is as it was (texts, variables, overlay, shapes, dimensions, title block, footprint fields)
//
// A scenario that throws does not stop the run: its section ends with a failed check "<section> ran to the end (stopped at: <step>)" naming the step it was at (and why), the next section
// runs, and whatever happens the run ends by closing the dialogs, taking back the undo steps it made (only those that were made) and checking the board is as it was. `__boardSetupPagesCheck.at`
// is the step a run that is still going is at.
// run-all-checks: board=mcu30-few   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
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
  const setSelect = async (el, value) => {
    if (!el) throw new Error(`no list to choose "${value}" in`);
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value));
    el.dispatchEvent(new Event("change", { bubbles: true }));
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
  /** Press Apply and wait for the page's answer (the board answers slowly when the machine is busy): the message is no longer "Changes not applied yet." and Apply is free again. */
  const apply = async (root) => {
    await press(root, "Apply", 300);
    for (let i = 0; i < 80; i++) {
      const msg = root.querySelector(".bs-msg")?.textContent ?? "";
      if (!button(root, "Apply")?.disabled && msg !== "" && msg !== "Changes not applied yet.") return msg;
      await sleep(100);
    }
    return root.querySelector(".bs-msg")?.textContent ?? "";
  };
  /** What the run must put back, as one string. */
  const fingerprint = (s) => JSON.stringify({
    variables: s.board_rules?.text_variables ?? {},
    layers: s.board_rules?.layer_setup ?? null,
    classes: s.board_rules?.net_classes ?? [],
    pages: [s.board_rules?.formatting, s.board_rules?.zone_hatch_offsets, s.board_rules?.tuning_patterns, s.board_rules?.tuning_profiles],
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
      const text = { id: "", content: "Rev ${P9A_REV} by ${Designer name} on ${LAYER} \\${P9A_REV}", at: { x: 9_000_000, y: 28_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false };
      check("a board text is added", (await edit({ op: "add_text", text })).ok);
      step("adding a plain board text");
      check("a plain board text is added", (await edit({ op: "add_text", text: { ...text, content: "plain P9A text", at: { x: 9_000_000, y: 31_000_000 } } })).ok);
      step("adding a footprint field with variables");
      const layout = { at: { x: 0, y: -2_500_000 }, angle: 0, size: [1_000_000, 1_000_000], thickness: 150_000, layer: "F.Fab", visible: true, halign: 0, valign: 0, mirror: false, bold: false, italic: false, keep_upright: true, knockout: false };
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
      const added = await edit({ op: "add_sch_text", content: "Sheet ${#}/${##} ${SHEETPATH} ${P9A_REV} ${CURRENT_DATE}", at: { x: 15_000_000, y: 30_000_000 }, angle_millideg: 0, size_nm: 1_270_000 }, "schematic");
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

    // ---------------------------------------------------------------------------------------------------------------------------------- board editor layers
    await section("board editor layers", async () => {
      step("opening the Board Editor Layers page");
      const p = await openPage("Board Editor Layers");
      const row = (layer) => p.querySelector(`tbody tr[data-layer="${layer}"]`);
      const input = (layer, what) => row(layer)?.querySelector(`[aria-label="${layer} ${what}"]`);
      const order = [...p.querySelectorAll("tbody tr")].map((tr) => tr.dataset.layer);
      check("Board Editor Layers is the first page of Board Stackup", [...document.querySelectorAll(".bs-nav-group")].some((g) => g.textContent === "Board Stackup") && document.querySelector(".bs-nav-item")?.textContent.trim() === "Board Editor Layers");
      check("the rows are in the order of KiCad's dialog: front layers, copper, back layers, outline, auxiliary layers", order.slice(0, 8).join() === "F.CrtYd,F.Fab,F.Adhes,F.Paste,F.SilkS,F.Mask,F.Cu,B.Cu" && order.slice(-5).join() === "Margin,Eco1.User,Eco2.User,Cmts.User,Dwgs.User", order.join());
      const locked = (layer) => row(layer)?.querySelector(`input[aria-label="Enable ${layer}"]`)?.disabled;
      check("the courtyards, the outline, the margin and the copper layers cannot be switched; the silkscreen can", locked("F.CrtYd") && locked("B.CrtYd") && locked("Edge.Cuts") && locked("Margin") && locked("F.Cu") && locked("B.Cu") && locked("F.SilkS") === false, JSON.stringify(["F.CrtYd", "Edge.Cuts", "F.Cu", "F.SilkS"].map(locked)));
      check("the standard layers are called what KiCad calls them", input("F.SilkS", "name")?.value === "F.Silkscreen" && input("Dwgs.User", "name")?.value === "User.Drawings" && input("F.Mask", "name")?.value === "F.Mask", [input("F.SilkS", "name")?.value, input("Dwgs.User", "name")?.value].join());

      step("adding a user-defined layer");
      await setSelect(p.querySelector('select[aria-label="User-defined layer to add"]'), "User.1");
      await press(p, "Add User Defined Layer");
      check("Add User Defined Layer adds a checked row, auxiliary, called User.1", !!row("User.1") && row("User.1").querySelector('input[type="checkbox"]').checked && input("User.1", "type")?.value === "auxiliary" && input("User.1", "name")?.value === "User.1" && [...p.querySelectorAll('select[aria-label="User-defined layer to add"] option')].every((o) => o.value !== "User.1"));
      step("naming and typing layers");
      await setText(input("User.1", "name"), "Notes");
      await setSelect(input("User.1", "type"), "back");
      await setText(input("F.SilkS", "name"), "Top art");
      await setSelect(input("F.Cu", "type"), "mixed");
      await press(p, "Add User Defined Layer");
      check("the next one added is the next free number", !!row("User.2"));
      step("a name another layer has");
      await setText(input("F.Fab", "name"), "B.Fab");
      const refused = await apply(p);
      const marked = row("F.Fab")?.querySelector("input.bs-invalid") && /already in use/.test(row("F.Fab")?.querySelector("td.bs-error")?.textContent ?? "");
      const startLayers = JSON.stringify(start.board_rules?.layer_setup ?? null);
      check("a name another layer has is marked (the later one in KiCad's order) and Apply stops, storing nothing", marked && /Fix the layers marked in red/.test(refused) && JSON.stringify((await state()).board_rules.layer_setup ?? null) === startLayers, `${marked} ${refused}`);
      step("applying the page");
      await setText(input("F.Fab", "name"), "F.Fab");
      const saved = await apply(p);
      if (saved === "Saved.") steps.push("pcb");
      let st = await state();
      const ls = st.board_rules.layer_setup;
      check("Apply stores the layers, the names and the types as the page says, and marks the page as set", saved === "Saved." && ls.enabled.includes("User.1") && ls.names["User.1"] === "Notes" && ls.names["F.SilkS"] === "Top art" && ls.types["User.1"] === "back" && ls.types["F.Cu"] === "mixed" && st.board_rules.overlay.includes("board_layers"), `${saved} ${JSON.stringify(ls)}`);
      await closeDialog();

      step("the layer lists");
      if (!document.querySelector(".ap-root")) document.querySelector('button[aria-label="Show Appearance"]')?.click();
      await sleep(300);
      document.querySelector('.ap-root [data-tab="layers"]')?.click();
      await sleep(250);
      const apRow = (key) => document.querySelector(`.ap-root .ap-row[data-layer="${key}"]`);
      check("the Appearance panel lists the user layer by its name, and the renamed silkscreen", apRow("User_1")?.querySelector(".ap-name")?.textContent === "Notes" && apRow("f_silks")?.querySelector(".ap-name")?.textContent === "Top art", `${apRow("User_1")?.textContent} ${apRow("f_silks")?.textContent}`);

      step("an item on the user layer");
      const note = { id: "", content: "On ${LAYER}", at: { x: 9_000_000, y: 33_000_000 }, angle: 0, layer: "User.1", size_nm: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false };
      check("a text is added on the user layer", (await edit({ op: "add_text", text: note })).ok);
      await sleep(900);
      st = await state();
      const shownNote = st.drawings.texts.find((x) => x.content === "On ${LAYER}");
      check("${LAYER} shows the name the user gave the layer", shownNote?.shown === "On Notes", JSON.stringify(shownNote));

      step("turning off a layer that holds an item");
      const p2 = await openPage("Board Editor Layers");
      const held = p2.querySelector('tbody tr[data-layer="User.1"] input[aria-label="Enable User.1"]');
      held.click();
      await sleep(150);
      const heldMessage = await apply(p2);
      st = await state();
      check("a layer that holds an item cannot be turned off: the page says which and how many, and stores nothing", /1 item\(s\) are on Notes/.test(heldMessage) && st.board_rules.layer_setup.enabled.includes("User.1"), `${heldMessage}`);
      await press(p2, "Revert");

      step("turning off a layer that holds nothing");
      p2.querySelector('tbody tr[data-layer="Eco2.User"] input[aria-label="Enable Eco2.User"]').click();
      await sleep(150);
      const off = await apply(p2);
      if (off === "Saved.") steps.push("pcb");
      st = await state();
      check("a layer nothing is on can be turned off", off === "Saved." && !st.board_rules.layer_setup.enabled.includes("Eco2.User"), `${off}`);
      await closeDialog();
      await sleep(300);
      check("the Appearance panel no longer lists it", !apRow("Eco2_User") && !!apRow("Eco1_User"));
      const refusedItem = await edit({ op: "add_text", text: { ...note, content: "nowhere", layer: "Eco2.User" } });
      check("nothing new goes on it: the verb refuses with the layer's name", refusedItem.ok === false && JSON.stringify(refusedItem).includes("User.Eco2"), JSON.stringify(refusedItem).slice(0, 300));

      step("the Text tool's layer list");
      await pickLayer("f_silks");
      await cancelTool();
      await __eda.run("pcbnew.InteractiveDrawing.text");
      await sleep(300);
      const toScreenForText = await screenOf();
      if (!toScreenForText) throw new Error("no cursor readout to place the text click by");
      const [tx, ty] = toScreenForText(3, 36);
      fire("pointermove", tx, ty); await sleep(200); fire("pointerdown", tx, ty, { buttons: 1 }); await sleep(60); fire("pointerup", tx, ty); await sleep(800);
      const options = [...document.querySelectorAll(".dialog .kv-grid select option")].map((o) => `${o.value}=${o.textContent}`);
      check("the Text tool lists the layers the board has by their names: the user layer, not the one turned off", options.includes("User.1=Notes") && options.includes("F.SilkS=Top art") && !options.some((o) => o.startsWith("Eco2.User=")) && options.some((o) => o.startsWith("Dwgs.User=")), options.join(" | "));
      await press(document.querySelector(".dialog"), "Cancel", 300);
    });

    // ---------------------------------------------------------------------------------------------------------------------------------- the smaller setup pages
    await section("setup pages", async () => {
      step("opening Formatting");
      const nav = () => [...document.querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim());
      let p = await openPage("Formatting");
      const order = nav();
      const after = (a, b) => order.indexOf(a) >= 0 && order.indexOf(b) === order.indexOf(a) + 1;
      check("the page tree has KiCad's order for the new pages", after("Defaults", "Formatting") && after("Solder Mask/Paste", "Zone Hatch Offsets") && after("Teardrops", "Length-tuning Patterns") && after("Length-tuning Patterns", "Tuning Profiles") && after("Tuning Profiles", "Net Classes"), order.join(" | "));
      const stored = async () => (await state()).board_rules;
      const formattingBefore = JSON.stringify((await stored()).formatting);
      const dash = () => p.querySelector('input[aria-label="Dash length"]');
      const gap = () => p.querySelector('input[aria-label="Gap length"]');
      const box = (text) => [...p.querySelectorAll("label.bs-check")].find((l) => l.textContent.includes(text))?.querySelector("input");
      step("a dash length of 0");
      await setText(dash(), "0");
      const refusedFormatting = await apply(p);
      check("Formatting: a ratio of 0 is marked and Apply stores nothing", dash().classList.contains("bs-invalid") && /Fix the values marked in red/.test(refusedFormatting) && JSON.stringify((await stored()).formatting) === formattingBefore, refusedFormatting);
      step("applying Formatting");
      await setText(dash(), "8");
      await setText(gap(), "2.5");
      box("footprint fields").click();
      box("non-copper footprint shapes").click();
      const savedFormatting = await apply(p);
      if (savedFormatting === "Saved.") steps.push("pcb");
      let rules = await stored();
      check("Formatting: Apply stores the dash and gap lengths and the footprint boxes, and marks the page as set", savedFormatting === "Saved." && rules.formatting.dash_length_ratio === 8 && rules.formatting.gap_length_ratio === 2.5 && rules.formatting.apply_to_fp_fields === true && rules.formatting.apply_to_fp_shapes === true && rules.formatting.apply_to_fp_text === false && rules.overlay.includes("formatting"), JSON.stringify(rules.formatting));

      step("opening Zone Hatch Offsets");
      p = await openPage("Zone Hatch Offsets");
      const layers = [...p.querySelectorAll("tbody tr[data-layer]")].map((tr) => tr.dataset.layer);
      check("Zone Hatch Offsets: a row for each copper layer, front to back", layers.join() === ((await state()).layers ?? ["F.Cu", "B.Cu"]).join(), layers.join());
      const hx = (layer) => p.querySelector(`input[aria-label="${layer} hatch offset X"]`);
      const hy = (layer) => p.querySelector(`input[aria-label="${layer} hatch offset Y"]`);
      step("a hatch offset a metre too far");
      await setText(hx("B.Cu"), "2000mm");
      const hatchBefore = JSON.stringify((await stored()).zone_hatch_offsets);
      const refusedHatch = await apply(p);
      check("Zone Hatch Offsets: an offset above a metre is marked and Apply stores nothing", hx("B.Cu").classList.contains("bs-invalid") && /Fix the offsets marked in red/.test(refusedHatch) && JSON.stringify((await stored()).zone_hatch_offsets) === hatchBefore, refusedHatch);
      step("applying Zone Hatch Offsets");
      await setText(hx("B.Cu"), "0mm");
      await setText(hx("F.Cu"), "1.5mm");
      await setText(hy("F.Cu"), "-2mm");
      const savedHatch = await apply(p);
      if (savedHatch === "Saved.") steps.push("pcb");
      rules = await stored();
      check("Zone Hatch Offsets: Apply stores the layer that has an offset, in micrometres", savedHatch === "Saved." && JSON.stringify(rules.zone_hatch_offsets) === JSON.stringify({ "F.Cu": { x_nm: 1_500_000, y_nm: -2_000_000 } }), JSON.stringify(rules.zone_hatch_offsets));

      step("opening Length-tuning Patterns");
      p = await openPage("Length-tuning Patterns");
      const field = (group, what) => p.querySelector(`input[aria-label="${group} Tuning Pattern ${what}"]`);
      const single = "Single Track";
      const pair = "Differential Pair";
      const patternsBefore = JSON.stringify((await stored()).tuning_patterns);
      check("Length-tuning Patterns: three groups that start as KiCad's meander settings", field(single, "Maximum amplitude")?.value && JSON.parse(patternsBefore).single_track.max_amplitude_nm === 1_000_000 && JSON.parse(patternsBefore).diff_pair.spacing_nm === 1_000_000 && JSON.parse(patternsBefore).diff_pair_skew.spacing_nm === 600_000, patternsBefore);
      step("a minimum amplitude above the maximum");
      await setText(field(single, "Minimum amplitude"), "2mm");
      const refusedPatterns = await apply(p);
      check("Length-tuning Patterns: a minimum above the maximum is marked and Apply stores nothing", field(single, "Minimum amplitude").classList.contains("bs-invalid") && /Fix the values marked in red/.test(refusedPatterns) && JSON.stringify((await stored()).tuning_patterns) === patternsBefore, refusedPatterns);
      step("applying Length-tuning Patterns");
      await setText(field(single, "Minimum amplitude"), "0.3mm");
      await setText(field(single, "Maximum amplitude"), "1.5mm");
      await setText(field(pair, "Spacing"), "1.2mm");
      await setSelect(p.querySelector(`select[aria-label="${single} Tuning Pattern corner style"]`), "chamfer");
      const savedPatterns = await apply(p);
      if (savedPatterns === "Saved.") steps.push("pcb");
      rules = await stored();
      check("Length-tuning Patterns: Apply stores the groups", savedPatterns === "Saved." && rules.tuning_patterns.single_track.max_amplitude_nm === 1_500_000 && rules.tuning_patterns.single_track.min_amplitude_nm === 300_000 && rules.tuning_patterns.single_track.corner_style === "chamfer" && rules.tuning_patterns.diff_pair.spacing_nm === 1_200_000 && rules.tuning_patterns.diff_pair_skew.spacing_nm === 600_000, JSON.stringify(rules.tuning_patterns));
      await closeDialog();

      step("the length tuner");
      const st = await state();
      const net = st.parts.flatMap((q) => q.pads ?? []).find((q) => q.net)?.net;
      const added = await edit({ op: "add_track", net, layer: "F.Cu", width: 200_000, pts: [{ x: 3_000_000, y: 40_000_000 }, { x: 20_000_000, y: 40_000_000 }] });
      await sleep(600);
      const trackId = ((await state()).routing?.tracks ?? []).at(-1)?.id;
      check("a straight track is added for the tuner", added.ok && !!trackId, JSON.stringify(added).slice(0, 200));
      // The Tuning Pattern dialog's amplitude and spacing: its text fields after the target length and the target delay (P5d put the time domain in front),
      // [minimum amplitude, maximum amplitude, spacing, corner radius].
      const tunerFields = () => [...document.querySelectorAll(".dialog .kv-grid input[type=text]")].slice(2).map((i) => Number(i.value));
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", [trackId]);
      await sleep(250);
      // The tuner tools (7, 8) arm the tool since P5c; its settings are the Tuning Pattern dialog that Ctrl+L (`LengthTuner.Settings`) opens.
      const openTuner = async (tool) => { await __eda.run(tool); await sleep(300); await __eda.run("pcbnew.LengthTuner.Settings"); await sleep(500); };
      await openTuner("pcbnew.LengthTuner.TuneSingleTrack");
      const singleFields = tunerFields();
      const closeTuner = async () => { [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Cancel")?.click(); await sleep(300); await __eda.run("common.Interactive.cancel"); await sleep(200); await __eda.run("common.Interactive.cancel"); await sleep(200); };
      check("the length tuner opens at the maximum amplitude and the spacing of the single track pattern", singleFields[1] === 1.5 && singleFields[2] === 0.6, JSON.stringify(singleFields));
      await closeTuner();
      await openTuner("pcbnew.LengthTuner.TuneDiffPair");
      const pairFields = tunerFields();
      check("and for a differential pair at that of the pair pattern", pairFields[1] === 1 && pairFields[2] === 1.2, JSON.stringify(pairFields));
      await closeTuner();

      step("opening Tuning Profiles");
      p = await openPage("Tuning Profiles");
      const profilesBefore = JSON.stringify((await stored()).tuning_profiles);
      await press(p, "Add Tuning Profile");
      const refusedProfile = await apply(p);
      check("Tuning Profiles: a profile with no name is said so and Apply stores nothing", /Fix the profile marked in red/.test(refusedProfile) && /must have a name/.test(p.querySelector(".bs-error")?.textContent ?? "") && JSON.stringify((await stored()).tuning_profiles) === profilesBefore, refusedProfile);
      step("filling a profile");
      await setText(p.querySelector('input[aria-label="Profile name"]'), "USB 90");
      await setSelect(p.querySelector('select[aria-label="Profile type"]'), "differential");
      await setText(p.querySelector('input[aria-label="Target impedance"]'), "90");
      [...p.querySelectorAll("label.bs-check")].find((l) => l.textContent.includes("Enable time-domain tuning")).querySelector("input").click();
      await setText(p.querySelector('input[aria-label="Via propagation delay"]'), "25");
      await press(p, "Add Layer");
      const row1 = { signal: p.querySelector('select[aria-label="Row 1 signal layer"]')?.value, top: p.querySelector('select[aria-label="Row 1 top reference"]')?.value, bottom: p.querySelector('select[aria-label="Row 1 bottom reference"]')?.value };
      check("a new track row starts as KiCad's does: the front layer with the next one as its bottom reference", row1.signal === "F.Cu" && row1.top === "" && row1.bottom === "B.Cu", JSON.stringify(row1));
      await setText(p.querySelector('input[aria-label="Row 1 track width"]'), "0.13mm");
      await setText(p.querySelector('input[aria-label="Row 1 diff pair gap"]'), "0.15mm");
      await setText(p.querySelector('input[aria-label="Row 1 delay"]'), "6.1");
      await press(p, "Add Layer");
      check("the next row is the layer below, with the one above as its top reference", p.querySelector('select[aria-label="Row 2 signal layer"]')?.value === "B.Cu" && p.querySelector('select[aria-label="Row 2 top reference"]')?.value === "F.Cu", "");
      await press(p, "Add Via Override");
      await press(p, "Add Layer");
      const duplicated = await apply(p);
      check("a layer named twice stops Apply", /Fix the profile marked in red/.test(duplicated) && /Duplicated signal layer/.test(p.querySelector(".bs-error")?.textContent ?? "") && JSON.stringify((await stored()).tuning_profiles) === profilesBefore, duplicated);
      p.querySelector('button[aria-label="Remove row 3"]').click();
      await sleep(150);
      step("naming a profile for a class");
      const cls = p.querySelector('input[aria-label="Tuning profile of Default"]');
      await setText(cls, "Gone");
      check("a class that names a profile the page does not have is marked and the page says kicad-cli reports it", cls.classList.contains("bs-invalid") && /kicad-cli reports it as missing/.test(p.querySelector('tr[data-class="Default"] td.bs-error')?.textContent ?? ""), p.querySelector('tr[data-class="Default"]')?.textContent);
      const savedProfiles = await apply(p);
      if (savedProfiles === "Saved.") steps.push("pcb");
      rules = await stored();
      const prof = rules.tuning_profiles.profiles[0];
      check("Tuning Profiles: Apply stores the profile, its rows, its via override and the class's name for a profile", savedProfiles === "Saved." && prof?.name === "USB 90" && prof.profile_type === "differential" && prof.target_impedance === 90 && prof.enable_time_domain_tuning === true && prof.via_propagation_delay_as === 25000000 && prof.layer_entries.length === 2 && prof.layer_entries[0].width_nm === 130_000 && prof.layer_entries[0].diff_pair_gap_nm === 150_000 && prof.layer_entries[0].delay_as_per_mm === 6100000 && prof.via_overrides.length === 1 && rules.tuning_profiles.class_profiles.Default === "Gone" && rules.overlay.includes("tuning_profiles"), JSON.stringify(rules.tuning_profiles).slice(0, 400));
      await setText(p.querySelector('input[aria-label="Tuning profile of Default"]'), "USB 90");
      check("naming a profile the page has clears the mark", !p.querySelector('input[aria-label="Tuning profile of Default"]').classList.contains("bs-invalid"), "");
      const savedAgain = await apply(p);
      if (savedAgain === "Saved.") steps.push("pcb");
      check("and it is stored too", savedAgain === "Saved." && (await stored()).tuning_profiles.class_profiles.Default === "USB 90", savedAgain);
    });

    // ---------------------------------------------------------------------------------------------------------------------------------- net classes
    await section("net classes", async () => {
      step("opening Net Classes");
      const p = await openPage("Net Classes");
      const classesBefore = JSON.stringify((await state()).board_rules.net_classes);
      await press(p, "Add Netclass");
      await press(p, "Add Netclass");
      const names = [...p.querySelectorAll('input[aria-label$=" name"]')].filter((i) => /^Netclass \d+ name$/.test(i.getAttribute("aria-label")));
      check("Add Netclass adds a class named like KiCad's", names.length === 2 && names[0].value === "Netclass 1" && names[1].value === "Netclass 2", names.map((i) => i.value).join());
      await setText(names[0], "pins");
      await setText(names[1], "leds");
      await setText(p.querySelector('input[aria-label="pins Clearance"]'), "0.3mm");
      await setText(p.querySelector('input[aria-label="leds Clearance"]'), "0.1mm");
      step("assigning patterns");
      await press(p, "Add Assignment");
      await press(p, "Add Assignment");
      await press(p, "Add Assignment");
      const pattern = (i) => p.querySelector(`input[aria-label="Pattern ${i}"]`);
      const owner = (i) => p.querySelector(`select[aria-label="Net class of pattern ${i}"]`);
      // the board's nets have KiCad's names: `/LED channels (D1–D8)/PA0` (the hierarchical label in that sheet), `/LED channels (D1–D8)/LED1` (the
      // local label the derived schematic puts on every net the intent names)
      await setText(pattern(1), ".*/PA[0-3]");
      await setSelect(owner(1), "pins");
      await setText(pattern(2), "*/LED?");
      await setSelect(owner(2), "leds");
      await setText(pattern(3), ".*/PA(1|7)");
      await setSelect(owner(3), "leds");
      pattern(1).closest("tr").click();
      await sleep(200);
      const shown = (label) => [...(p.querySelector(`[aria-label="${label}"]`)?.querySelectorAll("div") ?? [])].map((d) => d.textContent.trim());
      check("a regular expression's nets are listed: .*/PA[0-3] is the four pins", ["PA0", "PA1", "PA2", "PA3"].every((n) => shown("Nets matching the selected pattern").includes("/LED channels (D1–D8)/" + n)) && !shown("Nets matching the selected pattern").includes("/LED channels (D1–D8)/PA4"), shown("Nets matching the selected pattern").join());
      pattern(2).closest("tr").click();
      await sleep(200);
      check("a wildcard pattern's too: */LED? is the two LEDs' nets", shown("Nets matching the selected pattern").join() === "/LED channels (D1–D8)/LED1,/LED channels (D1–D8)/LED2", shown("Nets matching the selected pattern").join());
      pattern(3).closest("tr").click();
      await sleep(200);
      check("and a group: .*/PA(1|7)", shown("Nets matching the selected pattern").join() === "/LED channels (D1–D8)/PA1,/LED channels (D1–D8)/PA7", shown("Nets matching the selected pattern").join());
      const composite = p.querySelector('[aria-label="Nets in more than one class"]')?.textContent ?? "";
      check("a net two classes match is said to be in both, with the name KiCad gives the composite class", /1 net is in more than one class/.test(composite) && /\/PA1: pins,leds,Default/.test(composite), composite);
      step("applying the net classes");
      const savedClasses = await apply(p);
      if (savedClasses === "Saved.") steps.push("pcb");
      let st = await state();
      const stored = st.board_rules.net_classes;
      check("Apply stores the classes in the order of the table with their patterns as typed", savedClasses === "Saved." && stored.map((c) => c.name).join() === "pins,leds" && JSON.stringify(stored[0].nets) === JSON.stringify([".*/PA[0-3]"]) && JSON.stringify(stored[1].nets) === JSON.stringify(["*/LED?", ".*/PA(1|7)"]) && stored[0].clearance === 300_000 && stored[1].clearance === 100_000, JSON.stringify(stored));
      step("moving a class");
      const rows = [...p.querySelectorAll("table.bs-grid")][0].querySelectorAll("tbody tr");
      rows[1].click();
      await sleep(150);
      await press(p, "Down");
      const movedApply = await apply(p);
      if (movedApply === "Saved.") steps.push("pcb");
      st = await state();
      check("moving pins down stores the order leds, pins: a class's place is its priority", movedApply === "Saved." && st.board_rules.net_classes.map((c) => c.name).join() === "leds,pins", JSON.stringify(st.board_rules.net_classes.map((c) => c.name)));
      const compositeAfter = p.querySelector('[aria-label="Nets in more than one class"]')?.textContent ?? "";
      check("and the composite class's name follows it", /\/PA1: leds,pins,Default/.test(compositeAfter), compositeAfter);
      check("the classes were not there before", classesBefore === "[]", classesBefore);
    });

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
      tg.silk.line_width_nm = 180_000; tg.silk.text_width_nm = 1_200_000; tg.silk.text_height_nm = 1_200_000; tg.silk.text_thickness_nm = 200_000;
      tg.copper.line_width_nm = 250_000;
      tg.fab.line_width_nm = 90_000; tg.fab.text_width_nm = 800_000; tg.fab.text_height_nm = 800_000; tg.fab.text_thickness_nm = 120_000;
      tg.others.line_width_nm = 130_000;
      step("setting the defaults");
      check("the defaults are set for the silkscreen, copper, fab and other classes", (await edit({ op: "set_text_graphics_defaults", settings: tg })).ok);
      await sleep(900);

      // Lines, rectangles, circles, arcs and polygons: the width of the layer's class.
      step("a line on the silkscreen");
      await pickLayer("f_silks");
      let sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 4], [11, 4]]);
      check("a line on the front silkscreen is on F.SilkS, at its class's line thickness", sh?.kind === "segment" && sh.layer === "F.SilkS" && sh.stroke_width === 180_000, JSON.stringify(sh));
      step("incWidth");
      await __eda.run("pcbnew.InteractiveDrawing.incWidth");
      await sleep(200);
      {
        const before = await shapeCount();
        await click(3, 6); await click(11, 6);
        const after = await shapeCount();
        if (after > before) steps.push("pcb");
        sh = await lastShape();
        check("incWidth steps the width by 0.1 mm from the class's", after === before + 1 && sh?.stroke_width === 280_000, JSON.stringify(sh));
      }
      step("arming again");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 8], [11, 8]]);
      check("arming the tool again starts at the class's width", sh?.stroke_width === 180_000, JSON.stringify(sh));
      step("a rectangle");
      sh = await draw("pcbnew.InteractiveDrawing.rectangle", "draw_rect", [[14, 4], [20, 9]]);
      check("a rectangle takes it too", sh?.kind === "rect" && sh.layer === "F.SilkS" && sh.stroke_width === 180_000, JSON.stringify(sh));
      step("a circle");
      sh = await draw("pcbnew.InteractiveDrawing.circle", "draw_circle", [[24, 7], [27, 7]]);
      check("and a circle", sh?.kind === "circle" && sh.stroke_width === 180_000, JSON.stringify(sh));
      step("an arc");
      sh = await draw("pcbnew.InteractiveDrawing.arc", "draw_arc", [[14, 14], [18, 14], [14, 18]]);
      check("and an arc", sh?.kind === "arc" && sh.stroke_width === 180_000, JSON.stringify(sh));
      step("a line on the fabrication layer");
      await pickLayer("f_fab");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 12], [11, 12]]);
      check("a line on another layer has that class's width: the front fabrication layer's", sh?.layer === "F.Fab" && sh.stroke_width === 90_000, JSON.stringify(sh));
      step("a line on a user layer");
      await pickLayer("Dwgs_User");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 14], [11, 14]]);
      check("on a user layer the Others class's", sh?.layer === "Dwgs.User" && sh.stroke_width === 130_000, JSON.stringify(sh));
      step("a line on copper");
      await pickLayer("F.Cu");
      sh = await draw("pcbnew.InteractiveDrawing.line", "draw_segment", [[3, 16], [11, 16]]);
      check("on copper the copper class's", sh?.layer === "F.Cu" && sh.stroke_width === 250_000, JSON.stringify(sh));

      // The Text tool: its dialog starts from the layer class's text.
      step("the Text tool on the silkscreen");
      await pickLayer("f_silks");
      await arm("pcbnew.InteractiveDrawing.text", "text");
      await click(3, 22);
      const [size, thickness] = dialogNumbers();
      check("the Text tool's dialog starts at the silkscreen class's size and thickness, not mirrored", size === 1_200_000 && thickness === 200_000 && mirrorBox()?.checked === false, `${size} ${thickness} ${mirrorBox()?.checked}`);
      await setText(document.querySelector(".dialog .kv-grid input"), "P9A tool text");
      const textsBefore = ((await state()).drawings?.texts ?? []).length;
      await press(document.querySelector(".dialog"), "Add", 900);
      const t = ((await state()).drawings?.texts ?? []).find((x) => x.content === "P9A tool text");
      if (((await state()).drawings?.texts ?? []).length > textsBefore) steps.push("pcb");
      check("the text is made with them, left aligned", t?.layer === "F.SilkS" && t.size === 1_200_000 && t.stroke_width === 200_000 && t.justify === "left" && t.mirror === false, JSON.stringify(t));
      step("the Text tool on a back layer");
      await pickLayer("b_fab");
      await arm("pcbnew.InteractiveDrawing.text", "text");
      await click(3, 24);
      const [size2, thickness2] = dialogNumbers();
      check("on the back fabrication layer: that class's text, mirrored", size2 === 800_000 && thickness2 === 120_000 && mirrorBox()?.checked === true, `${size2} ${thickness2} ${mirrorBox()?.checked}`);
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
      check("a dimension takes the class's text size and line thickness", dim?.layer === "F.SilkS" && dim.text_size_nm === 1_200_000 && dim.stroke_width === 180_000, JSON.stringify(dim && { layer: dim.layer, text: dim.text_size_nm, line: dim.stroke_width }));
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
