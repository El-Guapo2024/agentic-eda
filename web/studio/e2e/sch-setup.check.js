// Schematic Setup (UI-LEFT S3; PARITY-sch.md §56): every page of KiCad's dialog in its tree, each page edited through its own controls, its Apply stored as one
// undo step and read back from the backend, and what the sheet draws by it:
//   tree            General (Formatting, Annotation, Field Name Templates, BOM Presets), Electrical Rules (Violation Severity, Pin Conflicts Map), Project (Net
//                   Classes, Bus Alias Definitions, Text Variables), Schematic Data (Embedded Files)
//   formatting      a connection grid under 25 mils is refused in UNIT_BINDER's words; Junction dot size "Largest" is stored (GET /api/sch/formatting), the
//                   sheet's `setup` dot is 12 times the Default class's wire, and the canvas draws the dots bigger (more junction-green pixels)
//   annotation      the unit notation ".1" and "First free after sheet number X 100" are stored (GET /api/sch/setup)
//   templates       a new row is "Untitled Field"; a name with white space at an end is asked about in KiCad's words, and Remove White Space stores it trimmed
//   bus aliases     a bus labelled {DATA}; the alias DATA with the members "D0 D1" typed as one (split in two) makes the bus's members D0 and D1; a second
//                   alias of the name is refused in the grid's words
//   net classes     Schematic Setup > Net Classes shows the Eeschema columns; a class with a colour and a pattern for a wire's net is stored for both dialogs
//                   (board_rules.net_class_schematic), the wire's `class_look` has the colour, and the canvas draws it in it
//   erc fix         a label naming a class the project has not got: kicad-cli's ERC reports it, and the finding's menu ends with "Edit Netclasses...", which
//                   opens Schematic Setup on Net Classes
//   end             every step undone, the design as it was, no console error
// Run it on a served scratch board (it edits and undoes): `eda board new examples/mcu_board_30plus.yaml -o <dir>`, kicad-cli on the PATH for the ERC scenario.
// It answers through `window.__schSetupCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schSetupCheck = { results, done: false, ok: false, wait: null, at: "setup" });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await sleep(250);
    return { done: check.done, ok: check.ok, n: results.length, results: check.done ? results : results.slice(-2) };
  };
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  /** The same JSON whatever order its object keys came in. */
  const canon = (v) => JSON.stringify(v, (_, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))) : x));
  const until = async (cond, ms = 15000, step = 150) => {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      try {
        const v = await cond();
        if (v) return v;
      } catch {
        // not there yet
      }
      await sleep(step);
    }
    return null;
  };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const setup = () => [...document.querySelectorAll(".dialog")].find((d) => d.querySelector(".dialog-header")?.innerText.trim() === "Schematic Setup");
  const pageEl = (title) => setup()?.querySelector(`section.bs-page[aria-label="${title}"]`);
  const q = (title, sel) => pageEl(title)?.querySelector(sel);
  const button = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const openSetup = async (label) => {
    if (!setup()) {
      await __eda.run("eeschema.EditorControl.schematicSetup");
      await until(() => setup(), 4000);
    }
    if (label) {
      [...setup().querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith(label))?.click();
      await sleep(400);
    }
    return setup();
  };
  const apply = async (title) => {
    button(pageEl(title), "Apply")?.click();
    await sleep(1200);
    return pageEl(title)?.querySelector(".bs-msg")?.textContent ?? "";
  };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].find((x) => ["Close", "Discard and close"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(350);
    }
  };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** How many pixels of the canvas are within `tol` of `rgb` in each channel. */
  const count = (rgb, tol = 30) => {
    const c = canvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  /** The same, in a square of `half` canvas pixels about the sheet's point (x, y) (nm). */
  const countAt = (x, y, half, rgb, tol = 30) => {
    const c = canvas();
    const r = c.getBoundingClientRect();
    const v = __eda.state().view;
    const k = c.width / r.width;
    const px = Math.round((v.x + x * v.scale) * k), py = Math.round((v.y + y * v.scale) * k);
    const d = c.getContext("2d").getImageData(px - half, py - half, 2 * half + 1, 2 * half + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const enter = async (id) => {
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    if (id) {
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", [id]);
      await sleep(300);
      await __eda.run("eeschema.NavigateTool.enterSheet");
      await sleep(900);
      await __eda.run("common.InteractiveSelection.clear");
    }
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(800);
  };
  const GREEN = [0, 150, 0]; // LAYER_WIRE and LAYER_JUNCTION of the KiCad Default theme
  const MAGENTA = [255, 0, 255];
  const steps = [];
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    check.at = name;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  /** Undo the steps a scenario made (count them as it makes them). */
  const undoAll = async (n) => { for (let i = 0; i < n; i++) await undo(); };

  // ------------------------------------------------------------------------------------------------------------------------------- run
  [...document.querySelectorAll("button, div")].find((e) => e.innerText === "Schematic" && /tab/.test(e.className))?.click();
  await sleep(1500);
  await __eda.run("common.Control.zoomFitScreen");
  await sleep(800);
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const design0 = JSON.stringify(await json("/api/schematic"));
  const setup0 = JSON.stringify(await json("/api/sch/setup"));
  const fmt0 = JSON.stringify(await json("/api/sch/formatting"));

  await scenario("Schematic Setup has every page of KiCad's tree, in its order", async (t) => {
    const d = await openSetup();
    if (!t(!!d, "the action opens the dialog")) return;
    const tree = [...d.querySelectorAll(".bs-nav > div")].map((g) => [g.querySelector(".bs-nav-group")?.textContent, [...g.querySelectorAll(".bs-nav-item")].map((b) => b.textContent.trim())]);
    const want = [
      ["General", ["Formatting", "Annotation", "Field Name Templates", "BOM Presets"]],
      ["Electrical Rules", ["Violation Severity", "Pin Conflicts Map"]],
      ["Project", ["Net Classes", "Bus Alias Definitions", "Text Variables"]],
      ["Schematic Data", ["Embedded Files"]],
    ];
    t(JSON.stringify(tree) === JSON.stringify(want), `the tree: ${JSON.stringify(tree)}`);
    t(/Formatting/.test(d.querySelector(".bs-nav-item.active")?.textContent ?? ""), "it opens on Formatting");
  });

  await scenario("Formatting: the grid is refused in KiCad's words; Junction dot size Largest is stored, set on the sheet and drawn bigger", async (t) => {
    await openSetup("Formatting");
    const grid = q("Formatting", 'input[aria-label="Connection grid spacing in mils"]');
    if (!t(!!grid && grid.value === "50", `the connection grid field, 50 mils (${grid?.value})`)) return;
    t(q("Formatting", 'input[aria-label="Default text size in mils"]')?.value === "50" && q("Formatting", 'input[aria-label="Label offset ratio in percent"]')?.value === "15" && q("Formatting", 'input[aria-label="Global label margin ratio in percent"]')?.value === "37.5", "KiCad's defaults: 50 mils, 15 %, 37.5 %");
    t(q("Formatting", 'select[aria-label="Junction dot size"]')?.selectedOptions[0]?.textContent === "Default" && q("Formatting", 'select[aria-label="Range (voltages)"]')?.value === "Auto", "Junction dot size Default, the voltage range Auto");
    setValue(grid, "10");
    await sleep(200);
    const msg = await apply("Formatting");
    t(msg === "Connection grid must be at least 25 mils.", `a 10 mil grid is refused: "${msg}"`);
    t(JSON.stringify(await json("/api/sch/formatting")) === fmt0, "and nothing was stored");
    setValue(grid, "50");
    await closeDialogs();
    // a T on an empty stretch of the root sheet: the stem's end on the bar's middle is a junction (`SCH_SCREEN::IsJunctionNeeded`)
    const T = [127_000_000, 139_700_000];
    const bar = await post({ op: "add_wire", pts: [{ x: 101_600_000, y: T[1] }, { x: 152_400_000, y: T[1] }], bus: false });
    const stem = await post({ op: "add_wire", pts: [{ x: T[0], y: T[1] }, { x: T[0], y: 165_100_000 }], bus: false });
    if (!t(bar.ok !== false && stem.ok !== false, "a T of two wires on the root")) return;
    try {
    await enter(null);
    await sleep(800);
    const greenBefore = countAt(T[0], T[1], 14, GREEN);
    await openSetup("Formatting");
    setValue(q("Formatting", 'select[aria-label="Junction dot size"]'), "5");
    await sleep(200);
    const ok = await apply("Formatting");
    t(ok === "Saved.", `Apply: ${ok}`);
    steps.push("formatting");
    const f = await json("/api/sch/formatting");
    t(f.formatting?.junction_size_choice === 5, `stored: junction_size_choice ${f.formatting?.junction_size_choice}`);
    const s = await until(async () => { const x = await json("/api/schematic"); return x.setup?.junction_diameter_nm === 1_828_800 ? x : null; }, 5000);
    t(!!s, "the sheet's junction dot is 12 times the Default class's 6 mil wire (1828800 nm)");
    await closeDialogs();
    await sleep(1500);
    const greenAfter = countAt(T[0], T[1], 14, GREEN);
    t(greenAfter > greenBefore + 8, `the canvas draws the T's dot bigger (${greenBefore} -> ${greenAfter} junction-green pixels about it)`);
    await undo();
    steps.pop();
    t((await json("/api/sch/formatting")).formatting?.junction_size_choice === 3, "one undo takes it back");
    } finally {
      await undoAll(2);
    }
  });

  await scenario("Annotation: the unit notation .1 and the sheet x 100 numbering are stored", async (t) => {
    await openSetup("Annotation");
    const notation = q("Annotation", 'select[aria-label="Symbol unit notation"]');
    if (!t(!!notation && notation.selectedOptions[0]?.textContent === "A", `the unit notation starts at A (${notation?.selectedOptions[0]?.textContent})`)) return;
    t([...notation.options].map((o) => o.textContent).join(" ") === "A .A -A _A .1 -1 _1", "the seven notations");
    setValue(notation, "4");
    q("Annotation", 'input[aria-label="First free after sheet number X 100"]')?.click();
    await sleep(200);
    t((await apply("Annotation")) === "Saved.", "Apply");
    const s = await json("/api/sch/setup");
    t(s.subpart_first_id === 49 && s.subpart_id_separator === 46 && s.annotation.method === 1, `stored: ${s.subpart_first_id} ${s.subpart_id_separator} ${s.annotation.method}`);
    await closeDialogs();
    await undo();
    t(JSON.stringify(await json("/api/sch/setup")) === setup0, "one undo takes it back");
  });

  await scenario("Field Name Templates: Untitled Field, the white-space question in KiCad's words, Remove White Space", async (t) => {
    await openSetup("Field Name Templates");
    button(pageEl("Field Name Templates"), "Add")?.click();
    await sleep(200);
    const name = q("Field Name Templates", 'input[aria-label="Field name 1"]');
    if (!t(!!name && name.value === "Untitled Field", `a new row is "Untitled Field" (${name?.value})`)) return;
    t(q("Field Name Templates", 'input[aria-label="Field 1 visible"]')?.checked === false, "not visible");
    setValue(name, " MPN");
    q("Field Name Templates", 'input[aria-label="Field 1 URL"]')?.click();
    await sleep(200);
    button(pageEl("Field Name Templates"), "Apply")?.click();
    const ask = await until(() => q("Field Name Templates", '[role="alertdialog"]'), 3000);
    t(!!ask && ask.innerText.includes("The field name ' MPN' contains trailing and/or leading white space.") && ask.innerText.includes("This may result in what appears to be duplicate field names"), `the question: ${ask?.innerText.slice(0, 120)}`);
    button(ask, "Remove White Space")?.click();
    await sleep(1200);
    const s = await json("/api/sch/setup");
    t(canon(s.field_name_templates) === canon([{ name: "MPN", visible: false, url: true }]), `stored trimmed: ${JSON.stringify(s.field_name_templates)}`);
    await closeDialogs();
    await undo();
    t((await json("/api/sch/setup")).field_name_templates.length === 0, "one undo takes it back");
  });

  await scenario("Bus Alias Definitions: an alias used in a bus label gives the bus its members; a name twice is refused", async (t) => {
    const s0 = await json("/api/schematic");
    // a bus on an empty stretch of the sheet, labelled {DATA}
    const x = 25_400_000, y = 12_700_000;
    const w = await post({ op: "add_wire", pts: [{ x, y }, { x: x + 25_400_000, y }], bus: true });
    const l = await post({ op: "add_label", net: "{DATA}", at: { x, y }, kind: { scope: "local" } });
    if (!t(w.ok !== false && l.ok !== false, "a bus labelled {DATA}")) return;
    let made = 2;
    try {
      const busOf = (s) => s.wires.find((v) => v.bus && v.pts[0][0] === x && v.pts[0][1] === y);
      t(JSON.stringify(busOf(await json("/api/schematic"))?.members) === JSON.stringify(["DATA"]), "with no alias, DATA is the bus's one member");
      await openSetup("Bus Alias Definitions");
      const page = pageEl("Bus Alias Definitions");
      const by = (label) => page.querySelector(`[aria-label="${label}"]`);
      by("Add alias")?.click();
      await sleep(200);
      if (!t(!!by("Alias 1"), "Add makes an alias row")) return;
      setValue(by("Alias 1"), "DATA");
      by("Alias 1").focus();
      await sleep(250);
      t(page.innerText.includes("Members of 'DATA'"), "the members' grid is titled Members of 'DATA'");
      // a second alias of the name: the grid's veto, in its words
      by("Add alias")?.click();
      await sleep(200);
      setValue(by("Alias 2"), "DATA");
      await sleep(200);
      const refused = await apply("Bus Alias Definitions");
      t(refused === "Alias name 'DATA' already in use.", `a second alias of the name is refused: "${refused}"`);
      by("Alias 2").focus();
      await sleep(150);
      by("Delete alias")?.click();
      await sleep(200);
      by("Alias 1")?.focus();
      await sleep(250);
      by("Add member")?.click();
      await sleep(200);
      const member = by("Member 1");
      if (!t(!!member, "Add makes a member row")) return;
      setValue(member, "D0 D1");
      await sleep(200);
      const ok = await apply("Bus Alias Definitions");
      t(ok === "Saved.", `Apply: ${ok}`);
      made++;
      const s = await json("/api/sch/setup");
      t(canon(s.bus_aliases) === canon([{ name: "DATA", members: ["D0", "D1"] }]), `stored, the members split at the space: ${JSON.stringify(s.bus_aliases)}`);
      const bus = await until(async () => { const b = busOf(await json("/api/schematic")); return JSON.stringify(b?.members) === JSON.stringify(["D0", "D1"]) ? b : null; }, 5000);
      t(!!bus, "the bus labelled {DATA} unfolds to D0 and D1");
      await closeDialogs();
    } finally {
      await undoAll(made);
    }
    t(JSON.stringify((await json("/api/schematic")).wires) === JSON.stringify(s0.wires) && (await json("/api/sch/setup")).bus_aliases.length === 0, "the undo steps take the alias, the label and the bus back");
  });

  await scenario("Net Classes: the Eeschema columns, a class's colour stored for both dialogs, on the wire and drawn", async (t) => {
    const root = await json("/api/schematic");
    const mcu = root.sheets[0];
    const s0 = await json(`/api/schematic?sheet=${mcu.id}`);
    const wire = s0.wires.find((w) => !w.bus && /^[A-Za-z0-9_+]+$/.test(w.net) && w.net.length > 1);
    if (!t(!!wire, `a wire on a plainly named net in ${mcu.name} (${wire?.net})`)) return;
    await enter(mcu.id);
    await openSetup("Net Classes");
    const page = pageEl("Net Classes");
    const heads = [...page.querySelectorAll("table.bs-grid")[0].querySelectorAll("th")].map((h) => h.textContent.trim());
    t(JSON.stringify(heads) === JSON.stringify(["Name", "Wire Thickness", "Bus Thickness", "Color", "Line Style"]), `the columns: ${JSON.stringify(heads)}`);
    t(page.innerText.includes("Set color to transparent to use KiCad default color."), "the colour's help text");
    t(page.querySelector('select[aria-label="Default Line Style"]')?.disabled === true, "the Default row's line style is read-only");
    const magentaBefore = count(MAGENTA);
    button(page, "Add Netclass")?.click();
    await sleep(300);
    // (a new class is the grid's first row: `InsertRows()`)
    const name = page.querySelectorAll("table.bs-grid")[0].querySelector("input.bs-name");
    setValue(name, "Signal");
    await sleep(200);
    setValue(page.querySelector('input[aria-label="Signal Color"]'), "#ff00ff");
    setValue(page.querySelector('input[aria-label="Signal Wire Thickness"]'), "20mil");
    setValue(page.querySelector('select[aria-label="Signal Line Style"]'), "2"); // the rows: <Not defined>, Solid, Dashed, ...: Dashed is LINE_STYLE 1
    await sleep(200);
    button(page, "Add Assignment")?.click();
    await sleep(200);
    const patterns = [...page.querySelectorAll('input.bs-pattern')];
    setValue(patterns[patterns.length - 1], wire.net);
    setValue([...page.querySelectorAll('select[aria-label^="Net class of pattern"]')].pop(), "Signal");
    await sleep(200);
    const ok = await apply("Net Classes");
    t(ok === "Saved.", `Apply: ${ok}`);
    let made = ok === "Saved." ? 1 : 0;
    try {
      const st = await json("/api/state");
      const look = st.board_rules?.net_class_schematic?.Signal;
      t(look?.color === "rgb(255, 0, 255)" && look?.wire_width === 508_000 && look?.line_style === 1, `stored with the board's classes: ${JSON.stringify(look)}`);
      t((st.board_rules?.net_classes ?? []).some((c) => c.name === "Signal"), "Board Setup's table has the class too");
      const drawn = await until(async () => (await json(`/api/schematic?sheet=${mcu.id}`)).wires.find((w) => w.id === wire.id && w.class_look?.color === "rgb(255, 0, 255)"), 5000);
      t(!!drawn && drawn.class_look.width_nm === 508_000 && drawn.class_look.style === 1, `the wire's class look: ${JSON.stringify(drawn?.class_look)}`);
      await closeDialogs();
      await sleep(1500);
      const magentaAfter = count(MAGENTA);
      t(magentaAfter > magentaBefore + 20, `the canvas draws the wire in the class's colour (${magentaBefore} -> ${magentaAfter} magenta pixels)`);
    } finally {
      await undoAll(made);
      await enter(null);
    }
    t(!((await json("/api/state")).board_rules?.net_classes ?? []).some((c) => c.name === "Signal"), "one undo on the schematic tab takes the class back");
  });

  await scenario("ERC: an undefined net class's finding offers Edit Netclasses..., which opens Schematic Setup on Net Classes", async (t) => {
    const root = await json("/api/schematic");
    const led = root.sheets.find((x) => /LED/.test(x.name)) ?? root.sheets[0];
    const s0 = await json(`/api/schematic?sheet=${led.id}`);
    const label = s0.labels.find((l) => s0.wires.some((w) => w.pts.some((p) => p[0] === l.at[0] && p[1] === l.at[1])));
    if (!t(!!label, `a label on a wire in ${led.name} (${label?.net})`)) return;
    const r = await post({ op: "sch_edit", verb: "edit_label", id: label.id, fields: [{ name: "Netclass", text: "NoSuchClass", visible: true, italic: true }] }, led.id);
    if (!t(r.ok !== false, "the label names the class NoSuchClass")) return;
    try {
      const erc = await __eda.run("eeschema.InspectionTool.runERC");
      t(erc.ok !== false, `the Electrical Rules Checker opens (${erc.dialog})`);
      const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith("Electrical Rules Checker")), 4000);
      // the Show boxes start unticked, as dialog_erc_base.fbp has them (UI-LEFT-2 BB5)
      const all = dlg?.querySelector(".rc-show input");
      if (all && !all.checked) all.click();
      button(dlg, "Run ERC")?.click();
      const row = await until(() => [...document.querySelectorAll(".dialog .problem-row")].find((x) => /Netclass NoSuchClass is not defined|not defined/i.test(x.innerText)), 90000);
      if (!t(!!row, "kicad-cli's ERC reports the undefined class")) return;
      row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 250, button: 2 }));
      await sleep(300);
      const items = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
      const fix = items.find((e) => e.innerText.startsWith("Edit Netclasses..."));
      t(!!fix, `the finding's menu has Edit Netclasses... (${items.map((e) => e.innerText).join(" | ")})`);
      fix?.click();
      const d = await until(() => setup(), 4000);
      t(!!d && /Net Classes/.test(d.querySelector(".bs-nav-item.active")?.textContent ?? ""), `Schematic Setup opens on Net Classes (${d?.querySelector(".bs-nav-item.active")?.textContent})`);
      await closeDialogs();
    } finally {
      await undo();
    }
  });

  await scenario("everything is undone, and nothing went wrong in the console", async (t) => {
    await closeDialogs();
    t(JSON.stringify(await json("/api/sch/setup")) === setup0, "the setup pages are as they were");
    t(JSON.stringify(await json("/api/sch/formatting")) === fmt0, "Formatting is as it was");
    const now = await json("/api/schematic");
    const strip = (s) => JSON.stringify({ ...s, setup: undefined });
    t(strip(now) === strip(JSON.parse(design0)), "the sheet is as it was");
    const errs = (__eda.errors?.() ?? []).slice(errorsFrom);
    t(errs.length === 0, `no console error (${errs.slice(0, 3).join(" | ")})`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
