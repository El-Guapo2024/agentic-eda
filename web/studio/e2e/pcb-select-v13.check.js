// Board selection round 3 (UI-LEFT-3 P3-2, P3-3, P3-4, P3-24, C3-1; PARITY-pcb.md, the pcb-select section of task-v13-pcbselect): the clarification menu for every
// edit tool, Override locks in both editors, the footprint attribute toggles, FilterCollectorForFootprints, and a placement that keeps the editor's point.
// Driven through `window.__eda`, the menus, the toolbar's box and the canvas's pointer events, and checked against what the page and the server hold.
// Run it in the page of a served board with U1, R1 and R2 placed and the rest not (mcu30-few), on a COPY: what it adds it takes away.
//
// What it checks:
//   1  P3-2 `RequestSelection` -> `selectPoint` -> `doSelectionMenu` for a tool that took the first candidate before: Lock with the pointer on two crossing
//      tracks asks with the menu; 2 locks the second row's track only; the menu dismissed locks nothing.
//   2  P3-3 `FilterCollectorForLockedItems`: Delete leaves a selected locked track and says so with KiCad's message; with Override locks checked it deletes
//      it. The schematic's `FilterSelectionForLockedItems`: R on a locked symbol turns nothing; with its Override locks checked it turns, and stays locked.
//   3  P3-4 `ToggleFootprintAttribute`: the context menu of a footprint has Attributes with the three check items; Exclude from Bill of Materials sets the
//      footprint's attribute (checked after), again clears it; Exclude from Simulation sets its symbol's flag.
//   4  P3-24 `FilterCollectorForFootprints`: a click in a back footprint between its pads selects it; with High Contrast on F.Cu (its own layer not shown,
//      none of its items on F.Cu) the same click does not.
//   5  C3-1 a part placed from the Unplaced list with Snap to grid Never keeps the click's point off the grid; with Always it lands on the grid.
//
// It answers through `window.__pcbSelectV13Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-few minutes=10 kicad_config=fresh
(async () => {
  const results = [];
  const check = (window.__pcbSelectV13Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), results: check.done ? results : results.slice(-2) };
  };
  const S = () => E.state();
  const board = () => json("/api/state");
  const schematic = () => json("/api/schematic");
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const selected = () => S().selection.map((s) => s.id).sort();
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };

  const canvas = () => document.querySelector(".pcb-canvas-container canvas") ?? [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = S().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 0), ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, altKey: !!mods.alt }));
  };
  const hover = async (wx, wy) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(250); return [x, y]; };
  const click = async (wx, wy, wait = 700) => { const [x, y] = await hover(wx, wy); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(wait); };
  const menuRows = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  const rowText = (el) => el.querySelector("span")?.textContent ?? el.textContent;
  const key = async (k) => { document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(500); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const closeMenus = async () => { for (let i = 0; i < 3 && (menuRows().length || document.querySelector(".menu-node-submenu")); i++) await key("Escape"); };
  const tab = async (name) => {
    if (S().tab !== name) {
      [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
      await sleep(1500);
    }
    return S().tab === name;
  };
  /** The toolbar's Override locks box of the editor in view (`data-control="overrideLocks"`). */
  const overrideBox = () => [...document.querySelectorAll('[data-control="overrideLocks"] input')].find((el) => el.offsetParent !== null) ?? null;
  const setOverride = async (on) => { const b = overrideBox(); if (b && b.checked !== on) { b.click(); await sleep(300); } return !!b && b.checked === on; };
  /** The page's toast (App.tsx: a div with inline styles, its text the message) saying `text`. */
  const toastSays = (re) => [...document.querySelectorAll("div")].some((d) => d.children.length === 0 && re.test(d.textContent ?? ""));
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(120); };
  const setSnapMode = async (mode) => {
    await E.run("common.SuiteControl.openPreferences");
    await sleep(300);
    tid("prefs-page-display_pcb")?.click();
    await sleep(200);
    const sel = document.querySelector('[data-testid="display-options-pcb"] [data-testid="pref-grid-snap"]');
    if (sel) await choose(sel, mode);
    tid("preferences-ok")?.click();
    await sleep(400);
    return !!sel;
  };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try {
      await closeMenus();
      for (let i = 0; i < 3 && (S().tool !== "select" || S().picker); i++) await escape();
      await setOverride(false);
      if (S().tab !== "pcb") { await E.run("eeschema.NavigateTool.changeSheet", []); await tab("pcb"); }
    } catch { /* best effort */ }
    results.push(entry);
  };

  const MM = 1e6;
  const addTrack = async (pts, net = "GND", layer = "F.Cu", width = 250_000) => {
    const before = new Set(((await board()).routing?.tracks ?? []).map((x) => x.id));
    await cmd({ op: "add_track", net, layer, width, pts: pts.map(([x, y]) => ({ x: Math.round(x), y: Math.round(y) })) });
    return ((await board()).routing?.tracks ?? []).find((x) => !before.has(x.id))?.id ?? null;
  };
  const lockedNow = async () => new Set((await board()).locked ?? []);

  // Strict off for the run (its gates are not what this check is about), put back at the end
  const strictBox = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = !!strictBox?.checked;
  const setStrict = async (on) => { if (strictBox && strictBox.checked !== on) { strictBox.click(); await sleep(400); } };

  for (let i = 0; i < 40 && S().tab !== "pcb"; i++) await sleep(250);
  await setStrict(false);
  await E.run("pcbnew.Control.layerTop");
  await E.run("common.Control.zoomFitObjects");
  await sleep(600);
  // free spots of mcu_board_30plus with U1 at (17.5, 17.5) and R1, R2 at (8, 8), (8, 12)
  const FREE = [[24.0, 8.0], [24.0, 25.0], [6.0, 25.0]].map(([x, y]) => [x * MM, y * MM]);

  (async () => {
    // --------------------------------------------------------------------------------------------------------------------------------------------------- 1
    await scenario("P3-2: Lock on two crossing tracks asks with the clarification menu; 2 locks the second row's track; Escape locks nothing", async (t) => {
      const [cx, cy] = FREE[0];
      const a = await addTrack([[cx - 1.5 * MM, cy], [cx + 1.5 * MM, cy]]);
      const b = await addTrack([[cx, cy - 1.5 * MM], [cx, cy + 1.5 * MM]]);
      if (!t(a && b, `two crossing tracks (${a}, ${b})`)) return;
      await sleep(600);
      await select([]);
      await hover(cx, cy);
      await E.run("pcbnew.EditorControl.toggleLock");
      const rows = await until(() => (menuRows().length >= 3 ? menuRows() : null), 4000);
      if (!t(!!rows, `Lock opens the clarification menu (${menuRows().map(rowText)})`)) return;
      t(/^1 {2}Track/.test(rowText(rows[0])) && /^2 {2}Track/.test(rowText(rows[1])), `numbered rows of the two tracks (${rows.map(rowText)})`);
      t((await lockedNow()).size === 0 || (!(await lockedNow()).has(a) && !(await lockedNow()).has(b)), "nothing is locked while it asks");
      rows[1].dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      await sleep(300);
      const second = S().menuHighlight[0];
      await key("2");
      const locked = await until(async () => { const l = await lockedNow(); return l.has(a) || l.has(b) ? l : null; }, 5000);
      t(locked && second && locked.has(second) && !locked.has(second === a ? b : a), `2 locks the second row's track only (${second}; locked ${locked && [...locked].filter((x) => x === a || x === b)})`);
      await undo();
      t(!(await lockedNow()).has(second), "one undo unlocks it");
      // dismissed: nothing
      await hover(cx, cy);
      await E.run("pcbnew.EditorControl.toggleLock");
      t(!!(await until(() => menuRows().length >= 3, 3000)), "Lock there asks again");
      await key("Escape");
      await sleep(600);
      const l2 = await lockedNow();
      t(menuRows().length === 0 && !l2.has(a) && !l2.has(b), "Escape dismisses it and nothing is locked");
      await cmd({ op: "delete_track", id: a });
      await cmd({ op: "delete_track", id: b });
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 2
    await scenario("P3-3: Delete leaves a locked track (KiCad's message) and deletes it with Override locks; on the schematic R turns a locked symbol only with its box", async (t) => {
      const [cx, cy] = FREE[0];
      const a = await addTrack([[cx - 1.5 * MM, cy], [cx + 1.5 * MM, cy]]);
      if (!t(!!a, `a track (${a})`)) return;
      await cmd({ op: "set_locked", ids: [a], locked: true });
      await sleep(600);
      await select([a]);
      t(JSON.stringify(selected()) === JSON.stringify([a]), `the locked track is selected (${selected()})`);
      await E.run("common.Interactive.delete");
      await sleep(900);
      t(((await board()).routing?.tracks ?? []).some((x) => x.id === a), "Delete leaves the locked track");
      t(toastSays(/^Selection contains locked items\. Enable 'Override locks' to operate on them\.$/), "with KiCad's message");
      if (!t(await setOverride(true), "the toolbar's Override locks box is checked")) return;
      await select([a]);
      await E.run("common.Interactive.delete");
      const gone = await until(async () => !((await board()).routing?.tracks ?? []).some((x) => x.id === a), 5000);
      t(gone, "with Override locks Delete deletes it");
      await setOverride(false);
      await undo();
      t(((await board()).routing?.tracks ?? []).some((x) => x.id === a), "one undo brings it back");
      await cmd({ op: "set_locked", ids: [a], locked: false });
      await cmd({ op: "delete_track", id: a });

      // the schematic
      if (!t(await tab("schematic"), "the Schematic tab")) return;
      // the MCU sheet (the root holds only the sheets)
      const sheetId = ((await schematic()).sheets ?? [])[0]?.id;
      if (!t(!!sheetId, `a sheet (${sheetId})`)) return;
      await E.run("eeschema.NavigateTool.changeSheet", [sheetId]);
      await sleep(1500);
      const sheetSch = () => json(`/api/schematic?sheet=${encodeURIComponent(sheetId)}`);
      const sch0 = await sheetSch();
      const sym = (sch0.symbols ?? []).find((x) => /^C\d+$/.test(x.id) && !(sch0.locked ?? []).includes(x.id));
      if (!t(!!sym, `a capacitor symbol (${sym?.id})`)) return;
      await select([sym.id]);
      await E.run("eeschema.InteractiveEdit.lock");
      const lockedSym = await until(async () => ((await sheetSch()).locked ?? []).includes(sym.id), 4000);
      t(lockedSym, `${sym.id} is locked`);
      const rotOf = async () => (await sheetSch()).symbols.find((x) => x.id === sym.id && x.unit === sym.unit)?.rot;
      const r0 = await rotOf();
      await select([sym.id]);
      await E.run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(900);
      t((await rotOf()) === r0, `R leaves the locked symbol as it is (${r0} -> ${await rotOf()})`);
      if (!t(await setOverride(true), "the schematic's Override locks box is checked")) return;
      await select([sym.id]);
      await E.run("eeschema.InteractiveEdit.rotateCCW");
      const turned = await until(async () => (await rotOf()) !== r0, 5000);
      t(turned, `with Override locks R turns it (${r0} -> ${await rotOf()})`);
      t(((await sheetSch()).locked ?? []).includes(sym.id), "and it stays locked");
      await setOverride(false);
      await undo();
      t((await rotOf()) === r0, "one undo turns it back");
      await select([sym.id]);
      await E.run("eeschema.InteractiveEdit.unlock");
      await until(async () => !((await sheetSch()).locked ?? []).includes(sym.id), 4000);
      await select([]);
      await E.run("eeschema.NavigateTool.changeSheet", []);
      await sleep(800);
      await tab("pcb");
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 3
    await scenario("P3-4: a footprint's context menu has Attributes; Exclude from Bill of Materials toggles its attribute, Exclude from Simulation its symbol's flag", async (t) => {
      const st = await board();
      const r1 = st.parts.find((p) => p.ref === "R1" && p.placed);
      if (!t(!!r1, "R1 is placed")) return;
      await select(["R1"]);
      // the context menu over R1
      const [x, y] = w2c(r1.at[0], r1.at[1]);
      (document.elementFromPoint(x, y) ?? canvas()).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 2 }));
      await sleep(500);
      const sub = [...document.querySelectorAll(".menu-node-submenu")].find((n) => n.querySelector(".menu-node-label span")?.textContent === "Attributes");
      if (t(!!sub, "the menu has Attributes")) {
        const items = [...sub.querySelectorAll(":scope > .menubar-dropdown > .menu-node-item")].map(rowText);
        t(JSON.stringify(items.map((s) => s.replace(/^[✓ ]*/, ""))) === JSON.stringify(["Exclude from Bill of Materials", "Exclude from Simulation", "Exclude from Position Files"]), `its three check items (${items})`);
        t([...sub.querySelectorAll(".menu-node-item")].every((n) => n.getAttribute("role") === "menuitemcheckbox"), "as check items");
      }
      await closeMenus();
      const bomWas = !!r1.attrs?.exclude_from_bom;
      await select(["R1"]);
      await E.run("pcbnew.InteractiveEdit.toggleExcludeFromBOM");
      const bom = await until(async () => { const p = (await board()).parts.find((q) => q.ref === "R1"); return p && !!p.attrs?.exclude_from_bom !== bomWas ? p : null; }, 5000);
      t(bom && !!bom.attrs.exclude_from_bom === !bomWas, `Exclude from Bill of Materials sets it (${bomWas} -> ${bom && bom.attrs.exclude_from_bom})`);
      await sleep(400);
      await select(["R1"]);
      await E.run("pcbnew.InteractiveEdit.toggleExcludeFromBOM");
      const back = await until(async () => { const p = (await board()).parts.find((q) => q.ref === "R1"); return p && !!p.attrs?.exclude_from_bom === bomWas ? p : null; }, 5000);
      t(!!back, "again clears it");
      await select(["R1"]);
      await E.run("pcbnew.InteractiveEdit.toggleExcludeFromSim");
      const sim = await until(async () => { const p = (await board()).parts.find((q) => q.ref === "R1"); return p?.attrs?.exclude_from_sim ? p : null; }, 5000);
      t(!!sim, "Exclude from Simulation sets the footprint's (its symbol's) flag");
      await undo();
      t(!(await board()).parts.find((q) => q.ref === "R1")?.attrs?.exclude_from_sim, "one undo clears it");
      await select([]);
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 4
    await scenario("P3-24: a click between a back footprint's pads selects it; with High Contrast on F.Cu (its layer not shown) it does not", async (t) => {
      await E.run("pcbnew.Control.layerTop");
      await cmd({ op: "flip_items", ids: ["R2"], pivot: { x: 8 * MM, y: 12 * MM }, direction: "left_right" });
      await sleep(900);
      const r2 = (await board()).parts.find((p) => p.ref === "R2");
      if (!t(r2?.side === "bottom", `R2 is on the back (${r2?.side})`)) return;
      // a click there finds R2 and its fab text ${REFERENCE} (the clarification menu asks between them): R2 is a candidate
      const picked = async () => {
        await select([]);
        await click(r2.at[0], r2.at[1]);
        const rows = menuRows().map(rowText);
        const out = [...selected(), ...rows];
        await closeMenus();
        await select([]);
        return out;
      };
      const front = await picked();
      t(front.some((x) => /R2\b/.test(x) && !/~t/.test(x)), `a click between its pads finds R2 (${front})`);
      await E.run("common.Control.highContrastMode");
      await sleep(400);
      const hc = await picked();
      t(!hc.some((x) => /R2\b/.test(x) && !/~t/.test(x)), `with High Contrast on F.Cu it does not (${hc})`);
      await E.run("common.Control.highContrastMode");
      await sleep(300);
      await select([]);
      await undo();
      t((await board()).parts.find((p) => p.ref === "R2")?.side !== "bottom", "one undo flips it back");
    });

    // --------------------------------------------------------------------------------------------------------------------------------------------------- 5
    await scenario("C3-1: with Snap to grid Never a part placed from the Unplaced list keeps the click's point off the grid; with Always it is on the grid", async (t) => {
      const notes = [];
      const unplaced = (await board()).parts.filter((p) => !p.placed).map((p) => p.ref);
      if (!t(unplaced.length >= 2, `two unplaced parts (${unplaced.slice(0, 2)})`)) return;
      const placeAt = async (ref, wx, wy) => {
        // the Unplaced list is the Properties panel's with nothing selected
        await select([]);
        const row = [...document.querySelectorAll(".unplaced-row")].find((r) => r.querySelector("b")?.textContent === ref);
        if (!row) { notes.push(`no row for ${ref}; dialogs ${JSON.stringify(S().dialogs)}`); return null; }
        row.click();
        await sleep(300);
        await click(wx, wy, 1200);
        return (await until(async () => (await board()).parts.find((p) => p.ref === ref && p.placed) ?? null, 5000)) ?? null;
      };
      const grid = S().grid ?? 0;
      const onGrid = (v) => grid > 0 && Math.abs(v / grid - Math.round(v / grid)) < 1e-6;
      if (!t(await setSnapMode("never"), "Display Options: Snap to grid Never")) return;
      // a point off every grid: 0.0337 mm past a whole millimetre
      const [ax, ay] = [FREE[1][0] + 33_700, FREE[1][1] + 41_300];
      const a = await placeAt(unplaced[0], ax, ay);
      if (t(!!a, `${unplaced[0]} is placed`)) t(!onGrid(a.at[0]) && !onGrid(a.at[1]) && Math.abs(a.at[0] - ax) < 2 / S().view.scale && Math.abs(a.at[1] - ay) < 2 / S().view.scale, `at the click's point, off the ${grid} nm grid (${a.at})`);
      await setSnapMode("always");
      const [bx, by] = [FREE[2][0] + 33_700, FREE[2][1] + 41_300];
      const b = await placeAt(unplaced[1], bx, by);
      if (t(!!b, `${unplaced[1]} is placed (${notes}; armed ${document.querySelector(".unplaced-row.armed b")?.textContent}; dialogs ${JSON.stringify(S().dialogs)})`)) t(onGrid(b.at[0]) && onGrid(b.at[1]), `on the ${grid} nm grid (${b.at})`);
      if (b) await undo();
      if (a) await undo();
      const after = await board();
      t(!after.parts.find((p) => p.ref === unplaced[0])?.placed && !after.parts.find((p) => p.ref === unplaced[1])?.placed, "two undos take them off again");
    });

    await setStrict(strictWas);
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
