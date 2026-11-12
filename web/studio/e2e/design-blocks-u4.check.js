// Design Blocks, UI-LEFT L5 (task-u4-libs; GAPS.md item 32): the pane docks on the left or the right and floats (`defaultDesignBlocksPaneInfo`: `TopDockable( false )`,
// `BottomDockable( false )`, `FloatingSize( 800, 600 )` at `FloatingPosition( 50, 200 )`), a block's name may hold a dot (KiCad 25ec2335's `DesignBlockEnumerate` lists
// `a.b.kicad_block` as `a.b`), and a sheet drawn from a block starts with the block's default fields, hidden (`SCH_DRAWING_TOOLS::DrawSheet`). Driven through `window.__eda`, the
// pane's own controls and the dialogs, and checked against what the server holds. Run it in the page of a served COPY of a board made from examples/mcu_board_30plus.yaml: it makes one
// design block library in the board's folder (a `.kicad_blocks` folder and a row in its design-block-lib-table), saves a block into it, places it as a sheet and undoes that, deletes the
// block and takes the row out again (the empty folder stays). Strict is left off while it runs.
//
// It answers through `window.__designBlocksU4Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__designBlocksU4Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, passed: results.filter((r) => r.ok).map((r) => r.name.slice(0, 40)), failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__designBlocksU4Sched) {
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
    window.__designBlocksU4Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const LIB = `DbU4${Date.now().toString(36)}`; // a name no earlier run used: the empty library folder stays in the board's folder
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const board = () => json("/api/state");
  const listing = () => json("/api/design_blocks");
  const block = async (name) => (await listing()).libraries.find((l) => l.name === LIB)?.blocks.find((b) => b.name === name);
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const undo = (domain) => post("/api/undo", { domain });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialog = () => [...document.querySelectorAll(".dialog-backdrop .dialog")].pop();
  const buttonOf = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const press = async (text, ms = 900) => { buttonOf(dialog(), text)?.click(); await sleep(ms); };
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const titles = () => E.state().dialogs;
  const pane = () => document.querySelector('[data-testid="db-pane"]');
  const rowOf = (id) => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].find((e) => (e.getAttribute("data-id") ?? e.getAttribute("data-library")) === id);
  const rows = () => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].map((e) => e.getAttribute("data-id") ?? e.getAttribute("data-library"));
  const option = async (key, on) => { const b = document.querySelector(`[data-testid="db-opt-${key}"]`); if (b && b.checked !== on) { b.click(); await sleep(150); } };
  /** Select a block of the check's library in the tree (a click on the library's row opens it when it is folded). */
  const chooseBlock = async (name) => {
    let r = rowOf(`${LIB}:${name}`);
    if (!r) { rowOf(LIB)?.click(); await sleep(300); r = await waitFor(() => rowOf(`${LIB}:${name}`)); }
    r?.click();
    await sleep(350);
    return !!r;
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = E.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y, ms = 1800) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(150); fire("pointermove", px + 3, py + 3); await sleep(150); fire("pointerdown", px + 3, py + 3, { buttons: 1 }); await sleep(70); fire("pointerup", px + 3, py + 3); await sleep(ms); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(500); };
  const gotoTab = async (label) => { if (E.state().tab !== label.toLowerCase()) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === label)?.click(); await sleep(1400); } };
  const leaveSheets = async () => { while (E.state().sheetPath.length > 0) { await E.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leaveSheets(); await select([id]); await E.run("eeschema.NavigateTool.enterSheet"); await sleep(1000); await select([]); };
  const menuLabels = () => [...document.querySelectorAll(".menubar-dropdown > *")].map((c) => (c.textContent || "").trim() || "-");
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(150); };
  const contextMenuOf = async (id) => { rowOf(id).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 1200, clientY: 300, button: 2 })); await sleep(300); const l = menuLabels(); await closeMenu(); return l; };
  const toastText = () => document.body.innerText;
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const d = dialog(); if (!d) break; (buttonOf(d, "Cancel") ?? buttonOf(d, "Close"))?.click(); await sleep(200); } };
  /** The Save / Overwrite dialog of a save: `fields` fill the properties dialog; an overwrite question is answered with `overwrite`. */
  const fillProperties = async ({ name, description, keywords, fields } = {}) => {
    const d = await waitFor(() => (titles().includes("Design Block Properties") ? dialog() : null), 8000);
    if (!d) return null;
    if (name !== undefined) setValue(d.querySelector("#db-name"), name);
    if (keywords !== undefined) setValue(d.querySelector("#db-keywords"), keywords);
    if (description !== undefined) setValue(d.querySelector("#db-description"), description);
    for (const [i, f] of (fields ?? []).entries()) {
      d.querySelector('[aria-label="Add field"]').click();
      await sleep(120);
      setValue(d.querySelector(`[aria-label="Name of field ${i + 1}"]`), f[0]);
      setValue(d.querySelector(`[aria-label="Value of field ${i + 1}"]`), f[1]);
    }
    await sleep(150);
    return d;
  };
  /** Everything a scenario changes in the design, to compare with how it was. */
  const snapshotSch = async () => {
    const of = (s) => ({ sym: s.symbols.map((x) => x.id).sort(), groups: (s.groups ?? []).map((g) => [g.id, g.name, g.design_block ?? "", [...g.member_ids].sort()]), sheets: s.sheets.map((x) => x.id) });
    return JSON.stringify([of(await sch("")), of(await sch(MCU))]);
  };
  const snapshotPcb = async () => { const b = await board(); return JSON.stringify({ parts: b.parts.map((p) => [p.ref, p.at, p.placed]), groups: (b.drawings?.groups ?? []).map((g) => [g.id, g.name, g.design_block ?? "", g.member_ids]) }); };
  const groupsOfBoard = async () => (await board()).drawings?.groups ?? [];

  const only = window.__designBlocksU4Only;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const notes = [];
    const t = (ok, text) => { notes.push(`${ok ? "ok" : "FAIL"}: ${text}`); return ok; };
    const t0 = Date.now();
    try { await fn(t); } catch (e) { t(false, `threw ${e && e.stack ? e.stack.split("\n").slice(0, 3).join(" | ") : e}`); }
    await closeDialogs();
    await escape();
    results.push({ name, ok: notes.every((n) => n.startsWith("ok")), notes, ms: Date.now() - t0 });
  };
  const ensureSame = async (domain, snap, take) => { for (let i = 0; i < 8 && (await take()) !== snap; i++) { await undo(domain); await sleep(500); } return (await take()) === snap; };

  // ------------------------------------------------------------------------------------------------------------------------ setup
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const errorsBefore = E.errors().length;
  await gotoTab("Schematic");
  await leaveSheets();
  const root = await sch("");
  const MCU = root.sheets.find((s) => s.name.startsWith("MCU"))?.id;
  if (!MCU) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the MCU sheet of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no board"; }
  const view0 = E.state().view;
  if (!view0 || !Number.isFinite(view0.x) || !Number.isFinite(view0.scale) || view0.scale < 1e-7) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)}): in a hidden pane take a screenshot after loading the page`] }); check.done = true; return "no canvas size"; }
  const base0 = await snapshotSch();
  const dock = () => document.querySelector('[data-testid="db-dock"]');
  const dockTo = async (to) => { dock()?.querySelector(`[data-dock-to="${to}"]`)?.click(); await sleep(400); };
  const caption = () => dock()?.querySelector(".dock-panel-caption");
  const firePtr = (el, type, x, y, buttons = 0) => el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 11, pointerType: "mouse", isPrimary: true, clientX: x, clientY: y, button: 0, buttons }));

  (async () => {
    // ======================================================================================================================== the pane floats and docks
    await scenario("the pane floats at KiCad's floating size, is moved by its caption, docks on the left and back on the right", async (t) => {
      await gotoTab("Schematic");
      if (!pane()) await E.run("eeschema.SchDesignBlockControl.showDesignBlockPanel");
      await waitFor(() => pane());
      if (dock()?.getAttribute("data-dock") !== "right") await dockTo("right");
      t(dock()?.getAttribute("data-dock") === "right" && !!dock()?.closest(".right-dock-col"), "the pane is docked on the right to begin with (Right().Layer( 3 ))");
      const offers = [...dock().querySelectorAll("[data-dock-to]")].map((b) => b.getAttribute("data-dock-to"));
      t(JSON.stringify(offers) === JSON.stringify(["left", "float", "close"]), `the caption offers the left, floating and close, never the top or the bottom (${offers})`);
      await dockTo("float");
      const d = dock();
      t(d?.getAttribute("data-dock") === "float" && !d.closest(".right-dock-col") && getComputedStyle(d).position === "fixed", "Float makes it a window of its own over the editor");
      const r = d.getBoundingClientRect();
      const want = { x: Math.min(50, Math.max(0, innerWidth - Math.min(800, innerWidth))), y: Math.min(200, Math.max(0, innerHeight - Math.min(600, innerHeight))), w: Math.min(800, innerWidth), h: Math.min(600, innerHeight) };
      t(Math.abs(r.left - want.x) <= 1 && Math.abs(r.top - want.y) <= 1 && Math.abs(r.width - want.w) <= 2 && Math.abs(r.height - want.h) <= 2, `at FloatingPosition( 50, 200 ), FloatingSize( 800, 600 ) kept in the window (${[r.left, r.top, r.width, r.height].map(Math.round)} vs ${Object.values(want)})`);
      t(!!pane() && pane().getAttribute("data-editor") === "schematic", "the floating pane is the schematic's pane, its tree and check boxes in it");
      // drag it by the caption
      const c = caption();
      firePtr(c, "pointerdown", r.left + 40, r.top + 8, 1);
      firePtr(c, "pointermove", r.left + 90, r.top + 30, 1);
      firePtr(c, "pointerup", r.left + 90, r.top + 30);
      await sleep(400);
      const r2 = dock().getBoundingClientRect();
      const moved = { x: Math.min(r.left + 50, innerWidth - r.width), y: Math.min(r.top + 22, innerHeight - r.height) };
      t(Math.abs(r2.left - moved.x) <= 1 && Math.abs(r2.top - moved.y) <= 1, `dragging the caption moves it (${Math.round(r2.left)},${Math.round(r2.top)} vs ${Math.round(moved.x)},${Math.round(moved.y)})`);
      // a double click on the caption docks it back where it was
      caption().dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      await sleep(400);
      t(dock()?.getAttribute("data-dock") === "right" && !!dock()?.closest(".right-dock-col"), "a double click on a floating pane's caption docks it back on the right");
      await dockTo("left");
      t(dock()?.getAttribute("data-dock") === "left" && !!dock()?.closest(".properties-col") && !document.querySelector(".right-dock-col [data-testid='db-dock']"), "Dock on the left puts it beside the left column");
      caption().dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      await sleep(400);
      t(dock()?.getAttribute("data-dock") === "float", "a double click on a docked caption floats it");
      const r3 = dock().getBoundingClientRect();
      t(Math.abs(r3.left - r2.left) <= 1 && Math.abs(r3.top - r2.top) <= 1, "it floats where it was left");
      caption().dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      await sleep(400);
      t(dock()?.getAttribute("data-dock") === "left", "... and docks back on the left, where it was docked last");
      await dockTo("right");
      t(dock()?.getAttribute("data-dock") === "right" && !!dock()?.closest(".right-dock-col"), "Dock on the right puts it back");
      // the board editor's pane is a pane of its own
      await gotoTab("PCB");
      if (!document.querySelector('[data-testid="db-dock"][data-editor="pcb"]')) await E.run("pcbnew.PcbDesignBlockControl.showDesignBlockPanel");
      await waitFor(() => document.querySelector('[data-testid="db-dock"][data-editor="pcb"]'));
      t(document.querySelector('[data-testid="db-dock"][data-editor="pcb"]')?.getAttribute("data-dock") === "right", "the board editor's pane keeps its own place (docked right)");
      await E.run("pcbnew.PcbDesignBlockControl.showDesignBlockPanel");
      await gotoTab("Schematic");
    });

    // ======================================================================================================================== a dotted name, default fields on a sheet
    await scenario("a block named with a '.' is saved and listed, and placed as a sheet the sheet starts with the block's default fields, hidden", async (t) => {
      await gotoTab("Schematic");
      if (!pane()) await E.run("eeschema.SchDesignBlockControl.showDesignBlockPanel");
      await waitFor(() => pane());
      await E.run("common.Control.newLibrary");
      const nd = await waitFor(() => (titles().includes("New Design Block Library") ? dialog() : null));
      t(!!nd, "New Library opens its dialog");
      if (nd) {
        const ins = nd.querySelectorAll("input");
        setValue(ins[1], LIB);
        setValue(ins[2], "Design blocks u4 check");
        document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
        await sleep(200);
        await press("Create", 1800);
      }
      t(!!(await listing()).libraries.find((l) => l.name === LIB), "the library is made");
      await enterSheet(MCU);
      rowOf(LIB)?.click();
      await sleep(300);
      await E.run("eeschema.SchDesignBlockControl.saveSheetAsDesignBlock");
      const d = await fillProperties({ name: "MCU.core", description: "MCU with a dotted name", keywords: "mcu dot", fields: [["Rail", "3V3"], ["Owner", "Power team"]] });
      t(!!d, "Save Current Sheet as Design Block opens the dialog");
      await press("Save", 2500);
      t(!titles().includes("Design Block Properties"), `a name with a '.' is accepted (KiCad 25ec2335) ${titles().includes("Design Block Properties") ? dialog().innerText.slice(0, 200) : ""}`);
      const b = await block("MCU.core");
      t(!!b && b.schematic === true && JSON.stringify(b.fields) === JSON.stringify([{ name: "Rail", value: "3V3" }, { name: "Owner", value: "Power team" }]), `the server lists the block by its whole name, with its fields (${JSON.stringify(b)})`);
      t(!!b && /MCU\.core\.kicad_block\/MCU\.core\.kicad_sch$/.test(b.schematic_path ?? ""), `its folder is MCU.core.kicad_block (${b && b.schematic_path})`);
      await leaveSheets();
      const before = await sch("");
      await option("asSheet", true);
      await option("asGroup", true);
      t(await chooseBlock("MCU.core"), "the tree lists the block");
      await E.run("eeschema.InteractiveDrawing.placeDesignBlock");
      await sleep(400);
      await clickAt(60_000_000, 120_000_000, 400);
      await clickAt(140_000_000, 170_000_000, 1200);
      const sd = await waitFor(() => (titles().includes("Hierarchical Sheet Properties") ? dialog() : null));
      t(!!sd && dialog().querySelectorAll("input")[0].value === "MCU.core", `the sheet is named after the block (${sd && dialog().querySelectorAll("input")[0].value})`);
      await press("Import", 3000);
      const after = await sch("");
      const sheet = after.sheets.find((x) => x.name === "MCU.core" && !before.sheets.some((y) => y.id === x.id));
      t(!!sheet, "the sheet is placed");
      const uf = sheet?.user_fields ?? [];
      t(JSON.stringify(uf.map((f) => [f.name, f.text, f.visible])) === JSON.stringify([["Rail", "3V3", false], ["Owner", "Power team", false]]), `the sheet starts with the block's fields, in order, hidden (${JSON.stringify(uf)})`);
      const g = (after.groups ?? []).find((x) => x.design_block === `${LIB}:MCU.core`);
      t(!!g && sheet && g.member_ids.includes(sheet.id), "the sheet is in a group linked to the block");
      await undo("schematic");
      await sleep(800);
      const back = await sch("");
      t(!back.sheets.some((x) => x.id === sheet?.id), "one undo takes the sheet and its fields back");
      await option("asSheet", false);
      // clean up: the block, then the library's row
      await chooseBlock("MCU.core");
      await E.run("eeschema.SchDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      await press("Yes", 1500);
      t(!(await block("MCU.core")), "Delete removes the block");
      const lt = await json("/api/library_tables");
      const rowsLeft = (lt.tables?.design_block?.project?.rows ?? []).filter((r) => r.nickname !== LIB);
      await post("/api/library_tables", { kind: "design_block", rows: rowsLeft });
      t((await snapshotSch()) === base0, "the schematic is as the check found it");
      t(E.errors().slice(errorsBefore).length === 0, `no errors ${JSON.stringify(E.errors().slice(errorsBefore).map((e) => e.message.slice(0, 80)))}`);
    });

    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
