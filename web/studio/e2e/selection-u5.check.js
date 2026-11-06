// Selection check (UI-LEFT P22, PARITY-pcb.md section 82): the Selection Filter's right-click "Only ..." menu and the filter kept with the project, the
// clarification menu's numbered rows and keys and the item its row brightens, GuessSelectionCandidates' coverage pass for overlapping footprints, and Select
// Connection (U) along a chain of graphic lines with the selection filter -- driven through `window.__eda`, the panel's rows, the menu and the canvas's
// pointer events, and checked against what the page and the server hold.
// Run it in the page of a served board whose parts are placed (as e2e/placement-u5.check.js), on a COPY: what it adds it takes away, the filter is put back.
//
// What it checks:
//   1  The Selection Filter (`PANEL_SELECTION_FILTER::onRightClick` / `onPopupSelection`): a row's right-click menu says "Only <row>"; choosing it leaves that
//      row alone on (All items off, Locked items as it was); the filter goes to the project's local settings (`appearance.json`'s `local.selection_filter`,
//      KiCad's `board.selection_filter` of the `.kicad_prl`).
//   2  The clarification menu (`SELECTION_TOOL::doSelectionMenu`): Alt+click on a pad lists what is under it, the first rows numbered ("1  ...") with their
//      number at the right and "Select All" (A); the pointer over a row brightens its item; pressing a row's number selects that item.
//   3  Overlapping footprints (`GuessSelectionCandidates`' coverage pass, `FOOTPRINT::CoverageRatio` over 0.70): two footprints on one spot are both kept for
//      the menu, where the pads alone would be offered otherwise.
//   4  Select Connection (U) on a graphic line (`selectAllConnectedShapes`): the lines met end to end on its layer are added; a line on another layer is not; with
//      the Graphics row off the walk adds nothing (`itemPassesFilter`).
//
// It answers through `window.__selectionCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed
(async () => {
  const results = [];
  const check = (window.__selectionCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), results: check.done ? results : results.slice(-2) };
  };
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const S = () => E.state();
  const selected = () => S().selection.map((s) => s.id).sort();

  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  let toScreen = null;
  const screenOf = async () => {
    await E.run("common.Control.zoomFitObjects");
    await sleep(400);
    fire("pointermove", 100, 100); await sleep(200); const a = readout();
    fire("pointermove", 500, 400); await sleep(200); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const click = async (xmm, ymm, extra = {}) => {
    const [x, y] = toScreen(xmm, ymm);
    fire("pointermove", x, y);
    await sleep(200);
    fire("pointerdown", x, y, { buttons: 1, ...extra });
    await sleep(60);
    fire("pointerup", x, y, extra);
    await sleep(600);
  };
  const menuRows = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  const rowText = (el) => el.querySelector("span")?.textContent ?? el.textContent;
  const closeMenus = async () => { if (menuRows().length) { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(300); } };
  const key = async (k) => { document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(500); };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeMenus(); await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  const filterRow = (k) => document.querySelector(`[data-filter-row="${k}"]`);
  const rowChecked = (k) => !!filterRow(k)?.querySelector("input")?.checked;
  const allItems = () => [...document.querySelectorAll(".filter-row")].find((l) => l.textContent.trim() === "All items")?.querySelector("input");
  const lockedItems = () => [...document.querySelectorAll(".filter-row")].find((l) => l.textContent.trim() === "Locked items")?.querySelector("input");
  const ROWS = ["footprints", "text", "tracks", "vias", "pads", "graphics", "zones", "keepouts", "dimensions", "points", "otherItems"];

  toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return "no canvas"; }
  // the right dock's Filter tab shows the Selection Filter (put back as it was at the end)
  const dockTab = (label) => [...document.querySelectorAll(".dock-tab")].find((x) => x.textContent.trim() === label);
  const dockWas = document.querySelector(".dock-tab.active")?.textContent.trim() ?? "Appearance";
  dockTab("Filter")?.click();
  await sleep(400);

  (async () => {
    await scenario("Selection Filter: a row's right-click menu says Only <row>; choosing it leaves that row alone on; the filter goes to the project's local settings", async (t) => {
      const row = await until(() => filterRow("vias"), 4000);
      if (!t(!!row, "the Selection Filter's Vias row is on screen")) return;
      const lockedWas = lockedItems()?.checked;
      const r = row.getBoundingClientRect();
      row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 10, clientY: r.top + 5 }));
      await sleep(300);
      const entries = menuRows().map(rowText);
      t(entries.length === 1 && entries[0] === "Only vias", `the menu offers "Only vias" (${entries})`);
      menuRows()[0]?.click();
      await sleep(400);
      t(rowChecked("vias") && ROWS.filter((k) => k !== "vias").every((k) => !filterRow(k) || !rowChecked(k)), "only the Vias row is left on");
      t(allItems() && !allItems().checked, "All items is off");
      t(lockedItems()?.checked === lockedWas, "Locked items is as it was");
      const rr = filterRow("keepouts").getBoundingClientRect();
      filterRow("keepouts").dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rr.left + 10, clientY: rr.top + 5 }));
      await sleep(300);
      t(menuRows().map(rowText)[0] === "Only rule areas", `the Rule Areas row says "Only rule areas" (${menuRows().map(rowText)})`);
      await closeMenus();
      // kept with the project (`SaveProjectLocalSettings`), a moment after the change
      const saved = await until(async () => { const a = await json("/api/appearance"); const f = a?.local?.selection_filter; return f && f.vias === true && f.footprints === false ? f : null; }, 6000);
      t(!!saved, `the filter is in appearance.json's local settings (${JSON.stringify(saved)})`);
      t(!!saved && saved.lockedItems === !!lockedWas && saved.tracks === false, "with every switch, by KiCad's keys");
      // back to everything on
      allItems()?.click();
      await sleep(300);
      t(ROWS.every((k) => !filterRow(k) || rowChecked(k)), "All items puts every row back on");
      const back = await until(async () => { const a = await json("/api/appearance"); return a?.local?.selection_filter?.footprints === true ? true : null; }, 6000);
      t(!!back, "and that is saved too");
    });

    // a pad of U1 to click on
    const u1 = (await board()).parts.find((p) => p.ref === "U1");
    const pad = u1?.pads?.[0];

    await scenario("The clarification menu: Alt+click lists what is under the pointer, the first nine numbered with their key at the right, Select All on A; the row under the pointer brightens its item; a row's number selects it", async (t) => {
      if (!t(!!pad, "U1 has a pad")) return;
      await select([]);
      await click(pad.x / 1e6, pad.y / 1e6, { altKey: true });
      const rows = await until(() => (menuRows().length >= 3 ? menuRows() : null), 3000);
      if (!t(!!rows, `Alt+click opens the clarification menu (${menuRows().map(rowText)})`)) return;
      const texts = rows.map(rowText);
      t(/^1 {2}/.test(texts[0]) && /^2 {2}/.test(texts[1]), `the rows are numbered "1  ...", "2  ..." (${texts.slice(0, 3)})`);
      t(rows[0].querySelector(".menu-accel")?.textContent === "1", "with the number at the right (the accelerator)");
      const all = rows.find((r) => rowText(r) === "Select All");
      t(!!all && all.querySelector(".menu-accel")?.textContent === "A", "Select All, chosen by A");
      t(!!document.querySelector(".menubar-dropdown .menu-separator"), "a separator before Select All");
      rows[1].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      await sleep(250);
      t(S().menuHighlight.length === 1, `the pointer on the second row brightens its item (${S().menuHighlight})`);
      const hovered = S().menuHighlight[0];
      // the pointer leaves the row for Select All (React reads the pair from the `mouseout`)
      rows[1].dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: all }));
      all.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: rows[1] }));
      await sleep(250);
      t(S().menuHighlight.length >= texts.length - 1, `on Select All, every item (${S().menuHighlight.length})`);
      await key("2");
      t(menuRows().length === 0, "pressing 2 closes the menu");
      t(selected().length === 1 && selected()[0] === hovered, `and selects the second row's item (${selected()} for ${hovered})`);
      t(S().menuHighlight.length === 0, "nothing stays brightened");
      // Select All by its key
      await click(pad.x / 1e6, pad.y / 1e6, { altKey: true });
      await until(() => menuRows().length >= 3, 3000);
      const n = menuRows().filter((r) => /^\d {2}/.test(rowText(r))).length;
      await key("a");
      t(selected().length === n, `A selects every item of the menu (${selected().length} of ${n})`);
      await select([]);
    });

    await scenario("Overlapping footprints: two footprints on one spot are both kept for the menu (the coverage pass), where the pads alone would be offered", async (t) => {
      const before = new Set((await board()).parts.map((p) => p.ref));
      const at = { x: 26_500_000, y: 14_500_000 };
      await cmd({ op: "place_footprint", footprint: "TestPoint:TestPoint_Pad_D1.0mm", at });
      await cmd({ op: "place_footprint", footprint: "TestPoint:TestPoint_Pad_D1.0mm", at });
      await E.run("common.Interactive.refresh").catch(() => {});
      await sleep(1200);
      const made = (await board()).parts.filter((p) => !before.has(p.ref)).map((p) => p.ref);
      t(made.length === 2, `two test points on one spot (${made})`);
      await select([]);
      await click(at.x / 1e6, at.y / 1e6);
      const rows = await until(() => (menuRows().length ? menuRows() : null), 3000);
      const texts = (rows ?? []).map(rowText);
      t(!!rows, `a plain click there cannot choose: the clarification menu (${texts})`);
      t(made.every((r) => texts.some((x) => x.includes(`${r} `) && /footprint/.test(x))), `both footprints are in it, each over 70 % covered by the other (${texts})`);
      t(texts.filter((x) => /^Pad|  Pad/.test(x)).length === 2, "with the two pads");
      await closeMenus();
      for (const r of made) await cmd({ op: "rip", part: r });
    });

    await scenario("Select Connection (U) on a graphic line: the lines met end to end on its layer are added, not another layer's; with Graphics off the walk adds nothing", async (t) => {
      const before = new Set(((await board()).drawings?.shapes ?? []).map((s) => s.id));
      const seg = (layer, a, b) => cmd({ op: "add_shape", shape: { kind: "segment", layer, stroke_width: 150_000, filled: false, start: { x: a[0] * 1e6, y: a[1] * 1e6 }, end: { x: b[0] * 1e6, y: b[1] * 1e6 } } });
      await seg("F.SilkS", [4, 26], [6, 26]);
      await seg("F.SilkS", [6, 26], [8, 26.5]);
      await seg("F.SilkS", [8, 26.5], [8, 27.5]);
      await seg("F.Fab", [8, 27.5], [10, 27.5]);
      await seg("F.SilkS", [11, 26], [12, 26]);
      await sleep(1200);
      const shapes = ((await board()).drawings?.shapes ?? []).filter((s) => !before.has(s.id));
      const id = (layer, x0) => shapes.find((s) => s.layer === layer && Math.round(s.start[0] / 1e5) === x0 * 10)?.id;
      const [a, b, c, d, e] = [id("F.SilkS", 4), id("F.SilkS", 6), id("F.SilkS", 8), id("F.Fab", 8), id("F.SilkS", 11)];
      if (!t(a && b && c && d && e, `five lines drawn (${shapes.length})`)) return;
      await select([a]);
      await E.run("pcbnew.InteractiveSelection.SelectConnection");
      await sleep(500);
      t(JSON.stringify(selected()) === JSON.stringify([a, b, c].sort()), `the chain on F.SilkS is selected (${selected()})`);
      t(!selected().includes(d) && !selected().includes(e), "not the F.Fab line its end touches, nor the line apart");
      // the Graphics row off: the walk adds nothing (the line stays as it was selected)
      const gfx = filterRow("graphics")?.querySelector("input");
      gfx?.click();
      await sleep(300);
      await select([a]);
      await E.run("pcbnew.InteractiveSelection.SelectConnection");
      await sleep(500);
      t(JSON.stringify(selected()) === JSON.stringify([a]), `with Graphics off nothing is added (${selected()})`);
      gfx?.click();
      await sleep(300);
      await select([]);
      for (const s of [a, b, c, d, e]) await cmd({ op: "delete_shape", id: s });
    });

    dockTab(dockWas)?.click();
    await sleep(200);
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
