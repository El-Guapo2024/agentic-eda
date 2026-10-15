// Schematic groups and user-defined fields check (GAPS.md package P10b; items 1, 12 and 17). Groups: Group, Ungroup, Add Items, Remove Items and Group Properties on the sheet,
// groups that nest, the whole group selected by a click on a member, Enter Group (a double click) and Leave Group, a window and a crossing box around a group, a group moved,
// turned, mirrored, locked, deleted, copied, pasted and duplicated as one tree, what is made while a group is entered joining it, the Grouping and Enter Group entries of the
// context menu, and the `(group ...)` forms the derived `.kicad_sch` has. User-defined fields: one added in Symbol Properties, shown and placed beside the others, picked,
// moved, turned, mirrored, edited and hidden like Reference and Value, and written to the file where the sheet draws it. Pin Helpers: the submenu of a pin's context menu, and a wire stub
// (held by the Drag tool), a no-connect and a net, hierarchical and global label on a pin and on a sheet pin. Swapped pins kept by Copy and Paste and by Duplicate.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`), with the
// schematic tab open -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it edits and undoes, and it leaves Strict
// off while it runs. It makes nothing that stays: every scenario undoes what it did (and, if it left anything behind, the run undoes until the sheet is as it was).
// `window.__schGroupsOnly = /regexp/` runs only the scenarios whose names match.
//
// It answers through `window.__schGroupsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a few minutes (each edit waits for the server's answer), longer than a tool call may last, so it runs on its own: read it with `await __schGroupsCheck.wait(30000)` (it
// returns when the run is done or the time is up) until `.done`. In a hidden pane the browser stops timers and animation frames; the script then drives both from a fetch loop. A
// hidden pane also tells the canvas its size only after a screenshot: take one after loading the page, or the view the script works from has no size and the setup says so.
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, every action by its id), the dialog's controls (their
// `aria-label`s) and the pointer events a user makes on the canvas.
(async () => {
  const results = [];
  const check = (window.__schGroupsCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, passed: results.filter((r) => r.ok).map((r) => r.name.slice(0, 40)), failed: results.filter((r) => !r.ok) };
  };

  if (document.visibilityState === "hidden" && !window.__schGroupsSched) {
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
    window.__schGroupsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  /** What the scenarios touch, to compare the sheet with how it was. */
  const snapshotOf = async (sheet) => {
    const s = await sch(sheet);
    return JSON.stringify({
      symbols: s.symbols.map((x) => [x.id, x.unit, x.at, x.rot, x.mirror]),
      wires: s.wires.map((w) => [w.id, w.pts]),
      labels: s.labels.map((l) => [l.id, l.net, l.at, l.spin]),
      texts: s.texts.map((t) => [t.id, t.content, t.at]),
      junctions: s.junctions.map((j) => [j.id, j.at]),
      ncs: (s.no_connects ?? []).map((n) => [n.id, n.at]),
      groups: (s.groups ?? []).map((g) => [g.id, g.name, [...g.member_ids].sort()]),
      locked: [...(s.locked ?? [])].sort(),
      fields: s.symbols.map((x) => [x.id, (x.fields ?? []).map((f) => [f.name, f.text, f.visible, f.at, f.vertical, f.h])]),
    });
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const selectedIds = () => __eda.state().selection.map((x) => x.id);
  const selectedKinds = () => __eda.state().selection.map((x) => x.kind);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const aria = (label) => document.querySelector(`.dialog [aria-label="${label}"]`);
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel") ?? buttonText(document, "Close"); if (!b) break; b.click(); await sleep(150); } };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const mouse = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: type === "contextmenu" ? 2 : 0, ...extra }));
  };
  /** Where a point of the sheet (um) is on the canvas (px), from the view the hook reports. */
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(120); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(300); };
  const dblclickAt = async (x, y) => { const [px, py] = screenOf(x, y); mouse("dblclick", px, py); await sleep(400); };
  const rclickAt = async (x, y) => { const [px, py] = screenOf(x, y); mouse("contextmenu", px, py); await sleep(500); };
  const dragBox = async (from, to) => {
    const [ax, ay] = screenOf(...from);
    const [bx, by] = screenOf(...to);
    fire("pointermove", ax, ay); await sleep(100);
    fire("pointerdown", ax, ay, { buttons: 1 }); await sleep(60);
    for (let i = 1; i <= 6; i++) { fire("pointermove", ax + ((bx - ax) * i) / 6, ay + ((by - ay) * i) / 6, { buttons: 1 }); await sleep(40); }
    fire("pointerup", bx, by); await sleep(400);
  };
  const fitTo = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(600); await select([]); };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(400); };
  const enterSheet = async (id) => {
    if (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    await select([id]);
    await __eda.run("eeschema.NavigateTool.enterSheet");
    await sleep(900);
    await select([]);
    await fit();
  };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const menu = () => document.querySelector(".menubar-dropdown");
  const menuLabels = () => (menu() ? [...menu().children].map((c) => (c.textContent || "").trim().replace(/\s+/g, " ")) : []);
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(250); };
  const groupingEnables = () => {
    const sub = menu() && [...menu().querySelectorAll(".menu-node-submenu")].find((e) => /Grouping/.test(e.textContent));
    if (!sub) return null;
    return Object.fromEntries([...sub.querySelectorAll(".menu-node-item")].map((e) => [(e.textContent || "").trim().replace(/\s+/g, " ").replace(/(Items?)\S*$/, "$1"), !(e.className.includes("disabled") || e.getAttribute("aria-disabled") === "true")]));
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const sorted = (a) => [...a].sort();

  let MCU = null;
  /** Undo until the sheet is as it was (a scenario that left something behind, or one that undid a step too few). */
  const restore = async (baseline) => {
    for (let i = 0; i < 16; i++) {
      if ((await snapshotOf(MCU)) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    if (window.__schGroupsOnly && !window.__schGroupsOnly.test(name)) return; // set `window.__schGroupsOnly = /regexp/` to run some of the scenarios only
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshotOf(MCU);
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeMenu(); await closeDialogs(); } catch { /* ignore */ }
      try { await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); if (__eda.state().entered) { await __eda.run("common.Interactive.groupLeave"); } } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the sheet could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const groups = async () => (await sch(MCU)).groups ?? [];
  /** The capacitors of the MCU sheet sit in a row, C1 first: where to click to be on one (the symbol's `at` is the corner of its box). */
  const bodyOf = (s, id) => { const x = s.symbols.find((q) => q.id === id); return [x.at[0] + 1000, x.at[1] + 3000]; };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
  const root = await sch("");
  MCU = root.sheets.find((s) => s.name.startsWith("MCU"))?.id;
  if (!MCU) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the MCU sheet of examples/mcu_board_30plus.yaml"] }); check.done = true; return "no board"; }
  await leave();
  await enterSheet(MCU);
  const view0 = __eda.state().view;
  if (!view0 || !Number.isFinite(view0.x) || !Number.isFinite(view0.scale) || view0.scale < 1e-4) { results.push({ name: "setup", ok: false, notes: [`FAIL: the canvas has no size (view ${JSON.stringify(view0)}): in a hidden pane take a screenshot after loading the page`] }); check.done = true; return "no canvas size"; }
  const base = await snapshotOf(MCU);
  const sheet0 = await sch(MCU);
  const need = ["U1", "C1", "C2", "C3", "C4", "C5"].filter((id) => !sheet0.symbols.some((s) => s.id === id));
  if (need.length > 0 || (sheet0.groups ?? []).length > 0) { results.push({ name: "setup", ok: false, notes: [`FAIL: the MCU sheet needs U1 and C1 to C5 and no groups (missing ${need}; groups ${(sheet0.groups ?? []).length})`] }); check.done = true; return "bad board"; }

  (async () => {
    // ----- Group, Ungroup
    await scenario("Group and Ungroup: one undo step each; the new group is selected, the members are afterwards", async (t) => {
      await select(["C1", "C2", "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(1000);
      let g = await groups();
      t(g.length === 1 && g[0].id.startsWith("grp_") && same(sorted(g[0].member_ids), ["C1", "C2", "C3"]), `Group over C1, C2 and C3 makes a group of them (${JSON.stringify(g.map((x) => x.member_ids))})`);
      t(same(selectedIds(), [g[0]?.id]) && same(selectedKinds(), ["group"]), `and the group is what is selected (${JSON.stringify(__eda.state().selection)})`);
      await select(["C2"]);
      t(same(selectedIds(), [g[0].id]), `a member selected on its own selects the group (${selectedIds()})`);
      await __eda.run("common.Interactive.ungroup");
      await sleep(1000);
      t((await groups()).length === 0, "Ungroup dissolves it");
      t(same(sorted(selectedIds()), ["C1", "C2", "C3"]), `and what was in it is what is selected (${selectedIds()})`);
      await undo();
      t((await groups()).length === 1, "one Undo brings the group back");
      await undo();
      t((await groups()).length === 0, "and one more takes it away again");
      // fewer than two items: no group
      await select(["C4"]);
      const before = await snapshotOf(MCU);
      await __eda.run("common.Interactive.group");
      await sleep(700);
      t((await snapshotOf(MCU)) === before, "one item is no group (`if( selection.GetSize() < 2 ) return 0`)");
      // what belongs to a symbol is no member: a pin and a symbol make no group (one item), a pin and two symbols make a group of the two
      await select(["C4", "pin:C5:1", "C5"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      g = await groups();
      t(g.length === 1 && same(sorted(g[0].member_ids), ["C4", "C5"]), `a pin in the selection is left out of the group (${JSON.stringify(g.map((x) => x.member_ids))})`);
    });

    // ----- nested groups
    await scenario("Groups nest: a group is a member of another, a click takes the outermost, Enter Group works one level in", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const inner = (await groups())[0];
      await select([inner.id, "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const all = await groups();
      const outer = all.find((x) => x.id !== inner.id);
      t(all.length === 2 && outer && outer.member_ids.includes(inner.id) && outer.member_ids.includes("C3") && outer.member_ids.length === 2, `a group of the group and C3 holds the group (${JSON.stringify(all.map((x) => x.member_ids))})`);
      await select(["C1"]);
      t(same(selectedIds(), [outer.id]), `a click on C1 takes the outermost group (${selectedIds()})`);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(400);
      t(__eda.state().entered === outer.id, `Enter Group works in the outer group (${__eda.state().entered})`);
      t(same(sorted(selectedIds()), sorted([inner.id, "C3"])), `its members are selected, the inner group as a group (${selectedIds()})`);
      await select(["C1"]);
      t(same(selectedIds(), [inner.id]), `inside it C1 is the inner group (${selectedIds()})`);
      await select(["C3"]);
      t(same(selectedIds(), ["C3"]), `and C3 is C3 (${selectedIds()})`);
      await select(["C5"]);
      t(__eda.state().entered === null && same(selectedIds(), ["C5"]), `an item outside it leaves it (entered ${__eda.state().entered}; ${selectedIds()})`);
      await select([outer.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      await __eda.run("common.Interactive.groupLeave");
      await sleep(300);
      t(__eda.state().entered === null && same(selectedIds(), [outer.id]), `Leave Group leaves it and selects the group (${selectedIds()})`);
      // Escape: first the selection, then the group
      await select([outer.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      await __eda.run("common.Interactive.cancel");
      await sleep(300);
      const afterFirst = { entered: __eda.state().entered, sel: selectedIds().length };
      await __eda.run("common.Interactive.cancel");
      await sleep(300);
      t(afterFirst.entered === outer.id && afterFirst.sel === 0 && __eda.state().entered === null, `Escape clears the selection first and then leaves the group (${JSON.stringify(afterFirst)} then ${__eda.state().entered})`);
      // Ungroup the outer one: the inner stays a group
      await select([outer.id]);
      await __eda.run("common.Interactive.ungroup");
      await sleep(900);
      const left = await groups();
      t(left.length === 1 && left[0].id === inner.id && same(sorted(selectedIds()), sorted([inner.id, "C3"])), `Ungroup frees the inner group, which stays a group (${JSON.stringify(left.map((x) => x.id))}; selected ${selectedIds()})`);
    });

    // ----- the pointer
    await scenario("The pointer: a click on a member selects the group, a double click enters it, a click inside picks a member, a click outside leaves", async (t) => {
      const s = await sch(MCU);
      await select(["C1", "C2", "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      await fitTo(["C1", "C2", "C3", "C4"]);
      await clickAt(...bodyOf(s, "C2"));
      t(same(selectedIds(), [grp.id]), `a click on C2 selects its group (${selectedIds()})`);
      await select([]);
      await clickAt(...bodyOf(s, "C4"));
      t(same(selectedIds(), ["C4"]), `a click on C4, in no group, selects C4 (${selectedIds()})`);
      await clickAt(...bodyOf(s, "C1"));
      await dblclickAt(...bodyOf(s, "C1"));
      t(__eda.state().entered === grp.id && same(sorted(selectedIds()), ["C1", "C2", "C3"]), `a double click on the selected group enters it and selects its members (entered ${__eda.state().entered}; ${selectedIds()})`);
      await clickAt(...bodyOf(s, "C2"));
      t(same(selectedIds().map((id) => id.replace(/^pin:(C\d+):.*$/, "$1")), ["C2"]) && __eda.state().entered === grp.id, `inside it a click picks C2 alone (${selectedIds()})`);
      await clickAt(...bodyOf(s, "C4"));
      t(__eda.state().entered === null && same(selectedIds(), ["C4"]), `a click on C4, outside it, leaves the group and selects C4 (entered ${__eda.state().entered}; ${selectedIds()})`);
      await select([grp.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      const [ex, ey] = bodyOf(s, "C4");
      await clickAt(ex, ey - 12000);
      t(__eda.state().entered === null, `a click on nothing outside the group's box leaves it (entered ${__eda.state().entered})`);
      await fit();
    });

    // ----- boxes
    await scenario("A window takes a group only whole, a crossing box takes the group of any member", async (t) => {
      const s = await sch(MCU);
      await select(["C1", "C2", "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      await fitTo(["C1", "C2", "C3", "C4"]);
      const at = (id) => s.symbols.find((q) => q.id === id).at;
      const top = Math.min(...["C1", "C2", "C3"].map((id) => at(id)[1])) - 3000;
      const bottom = Math.max(...["C1", "C2", "C3"].map((id) => at(id)[1])) + 14000;
      const left = at("C1")[0] - 3000;
      const rightAll = at("C3")[0] + 9000;
      // The boxes also take the power symbols, flags and wires about the capacitors (items in no group), so the answers are read as "holds" and "does not hold".
      const has = (id) => selectedIds().includes(id);
      const members = ["C1", "C2", "C3"];
      // a window (left to right) around all three: the group, and the members only as the group
      await select([]);
      await dragBox([left, bottom + 4000], [rightAll, top]);
      t(has(grp.id) && !members.some(has), `a window that holds the whole group selects the group, not its members one by one (${selectedIds()})`);
      // a window around C1 only: not C1, not the group (a member does not count on its own; "if all the members are inside, the group is selected instead")
      await select([]);
      await dragBox([left, bottom + 4000], [at("C1")[0] + 9000, top]);
      t(!has(grp.id) && !members.some(has), `a window around C1 alone selects neither C1 nor the group (${selectedIds()})`);
      // a crossing box (right to left) that touches C1 only: the group
      await select([]);
      await dragBox([at("C1")[0] + 4000, bottom + 4000], [left, top + 4000]);
      t(has(grp.id) && !members.some(has), `a crossing box that touches C1 selects its group (${selectedIds()})`);
      // a window around C4, in no group: C4
      await select([]);
      await dragBox([at("C4")[0] - 3000, bottom + 4000], [at("C4")[0] + 9000, top]);
      t(has("C4") && !has(grp.id), `a window around C4 selects C4 (${selectedIds()})`);
      await fit();
    });

    // ----- Move, Rotate, Mirror
    await scenario("A group moves, turns and mirrors as the items below it: one step each, nothing else touched", async (t) => {
      await select(["C1", "C2", "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      const before = await sch(MCU);
      const pos = (s, id) => s.symbols.find((q) => q.id === id).at;
      await fitTo(["C1", "C2", "C3", "C4"]);
      // M: the pointer carries the group, a click drops it -- the whole tree, one undo step
      const [px, py] = screenOf(...bodyOf(before, "C2"));
      await select([grp.id]);
      fire("pointermove", px, py); await sleep(200);
      await __eda.run("eeschema.InteractiveMove.move");
      const [qx, qy] = screenOf(bodyOf(before, "C2")[0] + 6350, bodyOf(before, "C2")[1] + 2540);
      fire("pointermove", qx, qy); await sleep(900);
      fire("pointerdown", qx, qy, { buttons: 1 }); await sleep(80); fire("pointerup", qx, qy); await sleep(1300);
      const moved = await sch(MCU);
      const dx = pos(moved, "C1")[0] - pos(before, "C1")[0];
      t(dx > 0 && ["C1", "C2", "C3"].every((id) => pos(moved, id)[0] - pos(before, id)[0] === dx && pos(moved, id)[1] - pos(before, id)[1] === 2540), `M carries C1, C2 and C3 by the same offset (${dx}, 2540)`);
      t(["C4", "C5", "U1"].every((id) => same(pos(moved, id), pos(before, id))), "and nothing outside the group");
      t(same(moved.groups.map((g) => [g.id, [...g.member_ids].sort()]), before.groups.map((g) => [g.id, [...g.member_ids].sort()])), "the tree is as it was");
      await undo();
      t((await snapshotOf(MCU)) === (await snapshotOf(MCU)) && same(pos(await sch(MCU), "C1"), pos(before, "C1")), "one Undo puts them back");
      // R: counter-clockwise, about the group's own centre; four turns are the identity
      await select([grp.id]);
      const start = await snapshotOf(MCU);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(1100);
      const turned = await sch(MCU);
      t(["C1", "C2", "C3"].every((id) => turned.symbols.find((q) => q.id === id).rot === (before.symbols.find((q) => q.id === id).rot + 90) % 360), "R turns every member a quarter");
      t(["C4", "C5"].every((id) => same(pos(turned, id), pos(before, id))), "and only the members");
      await undo();
      t((await snapshotOf(MCU)) === start, "one Undo puts them back");
      await select([grp.id]);
      for (let i = 0; i < 4; i++) { await __eda.run("eeschema.InteractiveEdit.rotateCCW"); await sleep(900); await select([grp.id]); }
      const r4 = await sch(MCU);
      t(["C1", "C2", "C3"].every((id) => same(pos(r4, id), pos(before, id)) && r4.symbols.find((q) => q.id === id).rot === before.symbols.find((q) => q.id === id).rot), "four turns bring the group home (a turn about the half-grid point nearest its centre, put back on it)");
      // X: mirrored about the group's centre
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.mirrorH");
      await sleep(1100);
      const mir = await sch(MCU);
      const xs = ["C1", "C2", "C3"].map((id) => [pos(before, id)[0], pos(mir, id)[0]]);
      t(xs.every(([a, b]) => a + b === xs[0][0] + xs[0][1]) && xs[0][0] !== xs[0][1], `X mirrors the members about one axis (${JSON.stringify(xs)})`);
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.mirrorH");
      await sleep(1100);
      const mir2 = await sch(MCU);
      t(["C1", "C2", "C3"].every((id) => same(pos(mir2, id), pos(before, id))), "and twice is the identity");
    });

    // ----- a drag takes nothing attached along
    await scenario("A dragged group stretches nothing attached to it; the same symbol dragged alone does", async (t) => {
      const before = await sch(MCU);
      const wireEnds = (s) => s.wires.map((w) => `${w.id}:${JSON.stringify(w.pts)}`).sort().join("|");
      await post({ op: "sch_move", verb: "drag", ids: ["C4"], dx: 0, dy: 5080, ortho: true }, MCU);
      await sleep(900);
      const alone = await sch(MCU);
      t(wireEnds(alone) !== wireEnds(before), "dragging C4 alone stretches the wires that end on its pins");
      await undo();
      await select(["C4", "C5"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      const held = await sch(MCU);
      await post({ op: "sch_move", verb: "drag", ids: [grp.id], dx: 0, dy: 5080, ortho: true }, MCU);
      await sleep(900);
      const dragged = await sch(MCU);
      t(["C4", "C5"].every((id) => dragged.symbols.find((q) => q.id === id).at[1] === held.symbols.find((q) => q.id === id).at[1] + 5080), "the group's symbols moved");
      t(wireEnds(dragged) === wireEnds(held), "and the wires on their pins stayed where they were (a group has no connection points)");
    });

    // ----- Lock
    await scenario("A locked group is no selection for a turn; the lock on the group locks what is below it", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.lock");
      await sleep(900);
      t((await sch(MCU)).locked.includes(grp.id), `Lock on a group locks the group (${(await sch(MCU)).locked})`);
      const locked = await snapshotOf(MCU);
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(800);
      t((await snapshotOf(MCU)) === locked, "Rotate does nothing to it");
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.unlock");
      await sleep(900);
      t(!(await sch(MCU)).locked.includes(grp.id), "Unlock frees it");
      await select([grp.id]);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(900);
      t((await snapshotOf(MCU)) !== locked, "and then it turns");
    });

    // ----- Delete
    await scenario("Delete takes the whole tree, group and all; one Undo brings it back", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const inner = (await groups())[0];
      await select([inner.id, "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const outer = (await groups()).find((g) => g.id !== inner.id);
      const start = await snapshotOf(MCU);
      await select([outer.id]);
      await __eda.run("common.Interactive.delete");
      await sleep(1300);
      const s = await sch(MCU);
      t(!s.symbols.some((x) => ["C1", "C2", "C3"].includes(x.id)) && s.symbols.some((x) => x.id === "C4"), "Delete removes the three symbols and leaves C4");
      t((s.groups ?? []).length === 0, `and the two groups go with them (${(s.groups ?? []).length} left)`);
      await undo();
      t((await snapshotOf(MCU)) === start, "one Undo brings back the symbols, the wires on them and both groups");
    });

    // ----- Copy, Paste, Duplicate
    await scenario("Copy and Paste bring the tree with new references; Duplicate does too; each is one undo step", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      const s0 = await sch(MCU);
      await fitTo(["C1", "C2", "C3", "C4"]);
      await select([grp.id]);
      await __eda.run("common.Interactive.copy");
      await sleep(700);
      await __eda.run("common.Interactive.paste");
      await sleep(1300);
      const [qx, qy] = screenOf(bodyOf(s0, "C4")[0], bodyOf(s0, "C4")[1] + 30000);
      fire("pointermove", qx, qy); await sleep(900);
      fire("pointerdown", qx, qy, { buttons: 1 }); await sleep(80); fire("pointerup", qx, qy); await sleep(1800);
      const s1 = await sch(MCU);
      const made = s1.groups.filter((g) => g.id !== grp.id);
      const fresh = s1.symbols.filter((x) => !s0.symbols.some((o) => o.id === x.id)).map((x) => x.id);
      t(s1.groups.length === 2 && made.length === 1 && fresh.length === 2 && same(sorted(made[0].member_ids), sorted(fresh)), `Paste makes a second group of the two new symbols (${JSON.stringify(made.map((g) => g.member_ids))}; new ${fresh})`);
      t(same(s1.groups.find((g) => g.id === grp.id).member_ids, grp.member_ids), "the first group is as it was");
      t(same(selectedIds(), [made[0]?.id]), `and the new group is what is selected (${selectedIds()})`);
      await undo();
      t((await groups()).length === 1 && (await sch(MCU)).symbols.length === s0.symbols.length, "one Undo takes the pasted tree away");
      // Duplicate (Ctrl+D)
      await select([grp.id]);
      await __eda.run("common.Interactive.duplicate");
      await sleep(1500);
      const [rx, ry] = screenOf(bodyOf(s0, "C4")[0], bodyOf(s0, "C4")[1] + 30000);
      fire("pointermove", rx, ry); await sleep(900);
      fire("pointerdown", rx, ry, { buttons: 1 }); await sleep(80); fire("pointerup", rx, ry); await sleep(1800);
      const s2 = await sch(MCU);
      t(s2.groups.length === 2 && s2.symbols.length === s0.symbols.length + 2, `Duplicate makes another copy of the group (${s2.groups.length} groups, ${s2.symbols.length} symbols)`);
      await fit();
    });

    // ----- Add, Remove, Properties
    await scenario("Add Items and Remove Items, and the dialog: a name, a row removed, a member picked on the sheet; one undo step each", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      // Add Items: the group and an item in no group
      await select([grp.id, "C3"]);
      await __eda.run("common.Interactive.addToGroup");
      await sleep(1000);
      let g = (await groups())[0];
      t((await groups()).length === 1 && same(sorted(g.member_ids), ["C1", "C2", "C3"]) && same(selectedIds(), [grp.id]), `Add Items puts C3 in the group and selects the group (${g.member_ids}; ${selectedIds()})`);
      await undo();
      // Add Items with two groups selected does nothing
      await select(["C4", "C5"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const other = (await groups()).find((x) => x.id !== grp.id);
      const two = await snapshotOf(MCU);
      await select([grp.id, other.id]);
      await __eda.run("common.Interactive.addToGroup");
      await sleep(700);
      t((await snapshotOf(MCU)) === two, "with two groups selected Add Items does nothing (\"Only allow one group to be selected\")");
      // Remove Items: a member inside the entered group
      await select([grp.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      await select(["C1"]);
      await __eda.run("common.Interactive.removeFromGroup");
      await sleep(1000);
      t(!(await groups()).some((x) => x.id === grp.id) && (await groups()).length === 1, `Remove Items takes C1 out; the group left with one member dissolves (${JSON.stringify((await groups()).map((x) => x.member_ids))})`);
      await undo();
      // the dialog
      await select([]);
      await __eda.run("common.Groups.groupProperties", grp.id);
      await sleep(500);
      const dlg = () => document.querySelector('.dialog[aria-label="Group Properties"]');
      t(!!dlg(), "Group Properties opens on the group");
      const rows = () => [...dlg().querySelectorAll('[role=option]')].map((o) => o.textContent.trim());
      t(same(rows(), ["Symbol C1 100nF", "Symbol C2 100nF"]), `with its members listed (${rows()})`);
      setValue(dlg().querySelector("input"), "bypass");
      await sleep(150);
      buttonText(dlg(), "+").click();
      await sleep(400);
      t(__eda.state().picker === "Click on new member..." && dlg() === null, `+ hides the dialog and asks for a click ("${__eda.state().picker}")`);
      const s = await sch(MCU);
      await fitTo(["C1", "C2", "C3", "C4"]);
      await clickAt(...bodyOf(s, "C3"));
      t(dlg() !== null && rows().length === 3 && /C3/.test(rows()[2]), `the click on C3 brings the dialog back with C3 in the list (${dlg() ? rows() : "no dialog"})`);
      [...dlg().querySelectorAll('[role=option]')][0].click();
      await sleep(150);
      buttonText(dlg(), "Remove").click();
      await sleep(200);
      t(rows().length === 2 && !rows().some((r) => /C1/.test(r)), `Remove drops the selected row (${rows()})`);
      buttonText(dlg(), "OK").click();
      await sleep(1100);
      g = (await groups()).find((x) => x.name === "bypass");
      t(g && same(sorted(g.member_ids), ["C2", "C3"]) && same(selectedIds(), [g.id]), `OK makes the group "bypass" of C2 and C3 and selects it (${g && g.member_ids})`);
      await undo();
      t((await groups()).some((x) => x.id === grp.id && x.name === ""), "and is one Undo step");
      await fit();
    });

    // ----- the menu
    await scenario("The context menu: Enter Group for one group, Grouping with the entries KiCad enables", async (t) => {
      const s = await sch(MCU);
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      await fitTo(["C1", "C2", "C3", "C4"]);
      await select([grp.id]);
      await rclickAt(...bodyOf(s, "C2"));
      let labels = menuLabels();
      t(/^Enter Group/.test(labels[0] ?? ""), `Enter Group heads the menu of one selected group (${labels.slice(0, 3)})`);
      let en = groupingEnables();
      t(en && !en["Group Items"] && en["Ungroup Items"] && !en["Add Items"] && !en["Remove Items"], `Grouping: Ungroup is on, Group, Add Items and Remove Items are off (${JSON.stringify(en)})`);
      await closeMenu();
      await select([grp.id, "C3"]);
      await rclickAt(...bodyOf(s, "C3"));
      en = groupingEnables();
      t(en && en["Group Items"] && en["Ungroup Items"] && en["Add Items"] && !en["Remove Items"], `a group and a free item: Group, Ungroup and Add Items are on (${JSON.stringify(en)})`);
      t(!/^Enter Group/.test(menuLabels()[0] ?? ""), "and Enter Group is not offered for two items");
      await closeMenu();
      await select(["C3", "C4"]);
      await rclickAt(...bodyOf(s, "C3"));
      en = groupingEnables();
      t(en && en["Group Items"] && !en["Ungroup Items"] && !en["Add Items"] && !en["Remove Items"], `two free items: only Group is on (${JSON.stringify(en)})`);
      await closeMenu();
      await select([grp.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      await select(["C1"]);
      await rclickAt(...bodyOf(s, "C1"));
      labels = menuLabels();
      en = groupingEnables();
      t(labels.some((l) => /^Leave Group/.test(l)) && en && en["Remove Items"], `inside the group: Leave Group is offered and Remove Items is on (${labels.slice(0, 2)}; ${JSON.stringify(en)})`);
      await closeMenu();
      await fit();
    });

    // ----- what is made while a group is entered joins it
    await scenario("What is made while a group is entered joins it: a duplicate of a member", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const grp = (await groups())[0];
      const s0 = await sch(MCU);
      await fitTo(["C1", "C2", "C3", "C4"]);
      await select([grp.id]);
      await __eda.run("common.Interactive.groupEnter");
      await sleep(300);
      await select(["C1"]);
      await __eda.run("common.Interactive.duplicate");
      await sleep(1500);
      const [qx, qy] = screenOf(bodyOf(s0, "C4")[0], bodyOf(s0, "C4")[1] + 30000);
      fire("pointermove", qx, qy); await sleep(900);
      fire("pointerdown", qx, qy, { buttons: 1 }); await sleep(80); fire("pointerup", qx, qy); await sleep(1800);
      const s1 = await sch(MCU);
      const fresh = s1.symbols.filter((x) => !s0.symbols.some((o) => o.id === x.id)).map((x) => x.id);
      const g = s1.groups.find((x) => x.id === grp.id);
      t(fresh.length === 1 && g && g.member_ids.length === 3 && g.member_ids.includes(fresh[0]), `the copy of C1 made inside the group is a member of it (${fresh} in ${JSON.stringify(g && g.member_ids)})`);
      await fit();
    });

    // ----- the file
    await scenario("The derived .kicad_sch has the groups: a name, a uuid and the members' uuids, a group in a group by its uuid", async (t) => {
      await select(["C1", "C2"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      const inner = (await groups())[0];
      await select([inner.id, "C3"]);
      await __eda.run("common.Interactive.group");
      await sleep(900);
      await __eda.run("common.Groups.groupProperties", inner.id);
      await sleep(500);
      setValue(document.querySelector('.dialog[aria-label="Group Properties"] input'), "channel A");
      await sleep(150);
      buttonText(document.querySelector('.dialog[aria-label="Group Properties"]'), "OK").click();
      await sleep(1100);
      const files = (await json("/api/schematic.kicad_sch")).files;
      const text = (files.find((f) => /mcu_u1/.test(f.name)) ?? files[0] ?? { text: "" }).text;
      const forms = [...text.matchAll(/\(group\s+"([^"]*)"\s*\(uuid\s+"([^"]+)"\)\s*\(members((?:\s+"[^"]+")+)\)\)/g)].map((m) => ({ name: m[1], uuid: m[2], members: [...m[3].matchAll(/"([^"]+)"/g)].map((q) => q[1]) }));
      t(forms.length === 2, `the file has two (group ...) forms (${forms.map((f) => f.name)})`);
      const named = forms.find((f) => f.name === "channel A");
      const outer = forms.find((f) => f.name !== "channel A");
      t(named && named.members.length === 2, `the named group lists the uuids of C1 and C2 (${named && named.members.length})`);
      t(outer && named && outer.members.includes(named.uuid) && outer.members.length === 2, "and the group that holds it lists that group's uuid and C3's");
      t(forms.every((f) => f.members.every((u) => text.includes(`(uuid "${u}")`) || forms.some((g) => g.uuid === u))), "every member uuid is the uuid of an item in the file");
    });


    // ----- a user-defined field
    const fieldOf = async (ref, name) => ((await sch(MCU)).symbols.find((x) => x.id === ref)?.fields ?? []).find((f) => f.name === name);
    /** `(property "name" "text" (at x y angle) ...)` of the symbol `ref` in the file the studio derives (millimetres), or null. */
    const fileProperty = async (ref, name) => {
      const files = (await json("/api/schematic.kicad_sch")).files;
      const text = (files.find((f) => /mcu_u1/.test(f.name)) ?? files[0]).text;
      const start = text.indexOf(`(property "Reference" "${ref}"`);
      if (start < 0) return null;
      const end = text.indexOf("(pin ", start);
      const m = text.slice(start, end < 0 ? undefined : end).match(new RegExp(`\\(property "${name}" "([^"]*)" \\(at ([-\\d.]+) ([-\\d.]+) (\\d+)\\)([^\\n]*)\\n([^\\n]*)`));
      return m ? { text: m[1], at: [Number(m[2]), Number(m[3])], angle: Number(m[4]), flags: m[5], effects: m[6] } : null;
    };
    const dialog = () => document.querySelector(".dialog");
    const addUserField = async (ref, name, text) => {
      await post({ op: "sch_edit", verb: "add_user_field", id: ref, name, text }, MCU);
      await post({ op: "sch_edit", verb: "edit_field", id: `fld:${ref}:${name}`, visible: true }, MCU);
      await sleep(300);
    };
    const near = (a, b, tol = 1) => Math.abs(a - b) <= tol;

    await scenario("A user-defined field is added in Symbol Properties, shown and placed with the others, and written to the file; one undo step", async (t) => {
      t(!(await fieldOf("C1", "MPN")), "C1 has no MPN field");
      await select(["C1"]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(700);
      t(!!dialog() && /Symbol Properties/.test(dialog().textContent), "Properties on a symbol opens Symbol Properties");
      buttonText(dialog(), "Add Field").click();
      await sleep(250);
      const nameBox = dialog().querySelector('input[aria-label="New field name"]');
      t(!!nameBox && /^Field\d+$/.test(nameBox.value), `Add Field starts a row called Field<N> (${nameBox && nameBox.value})`);
      // a name the symbol has is refused, and nothing is applied
      setValue(nameBox, "value");
      await sleep(100);
      buttonText(dialog(), "OK").click();
      await sleep(500);
      t(!!dialog() && /already exists/.test(dialog().textContent), "a name a standard field has is refused in the dialog");
      setValue(dialog().querySelector('input[aria-label="New field name"]'), "MPN");
      await sleep(100);
      setValue(dialog().querySelector('input[aria-label="MPN value"]'), "CL05B104KO5NNNC");
      await sleep(100);
      const shown = [...dialog().querySelectorAll("label")].find((l) => /Show the MPN field/.test(l.title)).querySelector("input");
      t(!shown.checked, "a new field is hidden until it is shown");
      shown.click();
      await sleep(100);
      buttonText(dialog(), "OK").click();
      await sleep(1400);
      t(!dialog(), "OK closes the dialog");
      const f = await fieldOf("C1", "MPN");
      const [r, v] = [await fieldOf("C1", "Reference"), await fieldOf("C1", "Value")];
      t(f && f.id === "fld:C1:MPN" && f.text === "CL05B104KO5NNNC" && f.visible, `C1 has the field, shown (${JSON.stringify(f && [f.id, f.text, f.visible])})`);
      t(f && new Set([r.at[1], v.at[1], f.at[1]]).size === 3 && f.at[1] > v.at[1] && v.at[1] > r.at[1], `the three texts are stacked on their own lines (${[r.at[1], v.at[1], f.at[1]]})`);
      const p = await fileProperty("C1", "MPN");
      t(p && p.text === "CL05B104KO5NNNC" && f && near(p.at[0], f.at[0] / 1000, 0.002) && near(p.at[1], f.at[1] / 1000, 0.002) && !/hide/.test(p.effects), `the file has it where the sheet draws it (${JSON.stringify(p)} for ${f && f.at})`);
      await undo();
      t(!(await fieldOf("C1", "MPN")) && !(await fileProperty("C1", "MPN")), "one Undo takes the field away again");
    });

    await scenario("A user-defined field is an item: a click selects it, and it moves, turns, mirrors and hides like Reference and Value", async (t) => {
      await addUserField("C1", "MPN", "CL05B104");
      const id = "fld:C1:MPN";
      let f = await fieldOf("C1", "MPN");
      t(f && f.visible, "the field is shown");
      await fitTo(["C1", id]);
      await select([]);
      await clickAt(f.at[0] + 600, f.at[1]);
      t(same(selectedIds(), [id]), `a click on its text selects the field (${selectedIds()})`);
      // the others stay when it moves
      const ref0 = (await fieldOf("C1", "Reference")).at;
      await post({ op: "sch_move", verb: "move", ids: [id], dx: 2540, dy: -2540 }, MCU);
      await sleep(500);
      const moved = await fieldOf("C1", "MPN");
      t(near(moved.at[0], f.at[0] + 2540) && near(moved.at[1], f.at[1] - 2540), `Move takes it by the offset (${f.at} -> ${moved.at})`);
      t(same((await fieldOf("C1", "Reference")).at, ref0), "and the Reference stays");
      const p = await fileProperty("C1", "MPN");
      t(p && near(p.at[0], moved.at[0] / 1000, 0.002) && near(p.at[1], moved.at[1] / 1000, 0.002), `the file has it where it was moved (${JSON.stringify(p && p.at)})`);
      await undo();
      t(same((await fieldOf("C1", "MPN")).at, f.at), "one Undo puts it back");
      // R turns its text a quarter where it stands, X flips its justification
      await select([id]);
      await __eda.run("eeschema.InteractiveEdit.rotateCCW");
      await sleep(800);
      let g = await fieldOf("C1", "MPN");
      t(g.vertical === true && same(g.at, f.at), `R turns the text where it stands (vertical ${g.vertical})`);
      const q = await fileProperty("C1", "MPN");
      t(q && q.angle === 90, `and the file says 90 (${q && q.angle})`);
      await undo();
      await select([id]);
      await __eda.run("eeschema.InteractiveEdit.mirrorH");
      await sleep(800);
      g = await fieldOf("C1", "MPN");
      t(g.h !== f.h && same(g.at, f.at), `X flips the justification (${f.h} -> ${g.h})`);
      await undo();
      // Delete hides it and keeps its text (`SCH_EDIT_TOOL::DoDelete` on a field)
      await select([id]);
      await __eda.run("common.Interactive.delete");
      await sleep(800);
      g = await fieldOf("C1", "MPN");
      t(g && g.visible === false && g.text === "CL05B104", "Delete hides it and keeps its text");
      const hidden = await fileProperty("C1", "MPN");
      t(hidden && /hide yes/.test(hidden.effects + hidden.flags), `and the file says so (${JSON.stringify(hidden && hidden.effects)})`);
      await undo();
      t((await fieldOf("C1", "MPN")).visible === true, "one Undo shows it again");
      // the symbol carries it along
      await post({ op: "sch_move", verb: "move", ids: ["C1"], dx: 2540, dy: 0 }, MCU);
      await sleep(500);
      const carried = await fieldOf("C1", "MPN");
      t(near(carried.at[0], f.at[0] + 2540) && near(carried.at[1], f.at[1]), `a symbol takes it along (${f.at} -> ${carried.at})`);
    });

    await scenario("Field Properties edit the text of a user-defined field (an empty one too), and its look", async (t) => {
      await addUserField("C1", "MPN", "CL05B104");
      const id = "fld:C1:MPN";
      await select([id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      t(!!dialog() && /Edit MPN Field/.test(dialog().textContent), "Properties on the field opens Field Properties");
      const text = dialog().querySelector(".kv-grid input");
      t(text.value === "CL05B104" && !text.disabled, "the text is editable");
      setValue(text, "CL05B104KO5NNNC");
      await sleep(100);
      const checks = [...dialog().querySelectorAll("label")].map((l) => [l.textContent.trim(), l.querySelector("input")]);
      checks.find(([n]) => /Bold/.test(n))[1].click();
      await sleep(100);
      buttonText(dialog(), "OK").click();
      await sleep(1200);
      let f = await fieldOf("C1", "MPN");
      t(f.text === "CL05B104KO5NNNC" && f.bold === true, `the text and the bold look are set (${f.text} ${f.bold})`);
      const p = await fileProperty("C1", "MPN");
      t(p && p.text === "CL05B104KO5NNNC" && /bold yes/.test(p.effects), `and written (${JSON.stringify(p)})`);
      await undo();
      f = await fieldOf("C1", "MPN");
      t(f.text === "CL05B104" && !f.bold, "one Undo takes both back");
      // an empty text is allowed for a user-defined field, not for a Value
      await select([id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(600);
      setValue(dialog().querySelector(".kv-grid input"), "");
      await sleep(100);
      t(!buttonText(dialog(), "OK").disabled, "OK is allowed with no text for a user-defined field");
      buttonText(dialog(), "OK").click();
      await sleep(1000);
      t((await fieldOf("C1", "MPN")).text === "", "the field is empty");
      await closeDialogs();
    });

    await scenario("Symbol Properties edit and remove a user-defined field; Autoplace Fields places a shown one with the others", async (t) => {
      await addUserField("C1", "MPN", "CL05B104");
      await select(["C1"]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(700);
      const value = dialog().querySelector('input[aria-label="MPN value"]');
      t(!!value && value.value === "CL05B104", "the field has its row, with its text");
      setValue(value, "TEXT2");
      await sleep(100);
      buttonText(dialog(), "OK").click();
      await sleep(1200);
      t((await fieldOf("C1", "MPN")).text === "TEXT2", "OK sets the text of the row");
      await undo();
      t((await fieldOf("C1", "MPN")).text === "CL05B104", "one Undo takes it back");
      // Autoplace Fields (O) puts the field with the others again, in one undo step
      await post({ op: "sch_move", verb: "move", ids: ["fld:C1:MPN"], dx: 10000, dy: 10000 }, MCU);
      await sleep(400);
      await select(["C1"]);
      await __eda.run("eeschema.InteractiveEdit.autoplaceFields");
      await sleep(900);
      const f = await fieldOf("C1", "MPN");
      const [r, v] = [await fieldOf("C1", "Reference"), await fieldOf("C1", "Value")];
      t(near(f.at[0], r.at[0]) && f.at[1] > v.at[1] && v.at[1] > r.at[1], `Autoplace Fields puts it on its own line under the Value (${[r.at, v.at, f.at].map((a) => a.join(",")).join(" | ")})`);
      await undo();
      // remove it from the dialog
      await select(["C1"]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      await sleep(700);
      dialog().querySelector('button[aria-label="Remove MPN"]').click();
      await sleep(150);
      t(!dialog().querySelector('input[aria-label="MPN value"]'), "the row is gone");
      buttonText(dialog(), "OK").click();
      await sleep(1200);
      t(!(await fieldOf("C1", "MPN")) && !(await fileProperty("C1", "MPN")), "OK removes the field from the symbol and the file");
      await undo();
      t(!!(await fieldOf("C1", "MPN")), "one Undo brings it back");
    });


    // ----- Pin Helpers
    /** Where pin `number` of U1 ends: a no-connect flag sits on the connection point of the two NC pins of the MCU. */
    const tipOf = async (pin) => ((await sch(MCU)).no_connects ?? []).find((n) => n.pin === `U1.${pin}`)?.at;
    const newItems = async (kind, before) => ((await sch(MCU))[kind] ?? []).filter((x) => !before[kind].some((o) => o.id === x.id));
    const submenu = (name) => menu() && [...menu().querySelectorAll(".menu-node-submenu")].find((e) => (e.querySelector(".menu-node-label")?.textContent ?? e.textContent).trim().startsWith(name));
    const itemOf = (sub, label) => sub && [...sub.querySelectorAll(".menu-node-item")].find((e) => (e.textContent || "").trim().replace(/\s+/g, " ").startsWith(label));

    await scenario("Pin Helpers: the context menu of a pin has the submenu, and Net Label puts a label named by the pin where it reads away from it, selected; one undo step", async (t) => {
      const tip = await tipOf(12);
      t(!!tip, `U1's pin 12 has a no-connect flag to find its tip by (${tip})`);
      const before = await sch(MCU);
      await fitTo(["U1"]);
      await select([]);
      await rclickAt(tip[0], tip[1]);
      const labels = menuLabels();
      t(/^Pin Helpers/.test(labels[0] ?? ""), `Pin Helpers heads the menu of a selected pin (${labels.slice(0, 3)})`);
      const sub = submenu("Pin Helpers");
      const names = sub ? [...sub.querySelectorAll(".menu-node-item")].map((e) => (e.textContent || "").trim().replace(/\s+/g, " ")) : [];
      t(same(names, ["Wire", "No Connect", "Net Label", "Hierarchical Label", "Global Label"]), `with KiCad's five entries (${names})`);
      t(same(selectedIds(), ["pin:U1:12"]), `the right click selected the pin (${selectedIds()})`);
      itemOf(sub, "Net Label").click();
      await sleep(1400);
      const made = await newItems("labels", before);
      t(made.length === 1 && made[0].net === "NC1" && same(made[0].at, tip), `a label named by the pin's name is at the pin's end (${JSON.stringify(made.map((l) => [l.net, l.at]))})`);
      t(made[0] && made[0].spin === "bottom", `it reads away from the pin, which runs up into the symbol: down (${made[0] && made[0].spin})`);
      t(same(selectedIds(), [made[0]?.id]), `and the new label is what is selected (${selectedIds()})`);
      await undo();
      t((await newItems("labels", before)).length === 0, "one Undo takes it away");
    });

    await scenario("Pin Helpers: No Connect flags every selected pin, Hierarchical and Global Label take the pin's name and a shape of its type; a helper that changes nothing is refused", async (t) => {
      const before = await sch(MCU);
      await select(["pin:U1:3", "pin:U1:4"]);
      await __eda.run("studio.PinHelpers.noConnect");
      await sleep(1200);
      const flags = await newItems("no_connects", before);
      t(flags.length === 2, `a no-connect on each of the two pins (${flags.length})`);
      t(selectedIds().length === 0, `and nothing is selected afterwards (${selectedIds()})`);
      // again: both pins have one, so nothing would change
      const steps = await snapshotOf(MCU);
      await select(["pin:U1:3", "pin:U1:4"]);
      await __eda.run("studio.PinHelpers.noConnect");
      await sleep(900);
      t((await snapshotOf(MCU)) === steps, "a second No Connect on them changes nothing (and adds no undo step)");
      await undo();
      t((await newItems("no_connects", before)).length === 0, "one Undo takes both flags away");
      for (const [action, scope] of [["studio.PinHelpers.hierLabel", "hierarchical"], ["studio.PinHelpers.globalLabel", "global"]]) {
        const b = await sch(MCU);
        await select(["pin:U1:11"]);
        await __eda.run(action);
        await sleep(1200);
        const made = await newItems("labels", b);
        t(made.length === 1 && made[0].net === "RESET" && made[0].scope === scope && ["input", "output", "bidirectional", "tri_state", "passive"].includes(made[0].shape), `${scope}: RESET with a shape (${JSON.stringify(made.map((l) => [l.net, l.scope, l.shape]))})`);
        await undo();
      }
    });

    await scenario("Pin Helpers: Wire puts a stub one grid step out of the pin and holds it by its far end; a click drops it (one undo step), Escape takes it away", async (t) => {
      const tip = await tipOf(12);
      const before = await sch(MCU);
      await fitTo(["U1"]);
      await select(["pin:U1:12"]);
      await __eda.run("studio.PinHelpers.wire");
      await sleep(1500);
      const made = await newItems("wires", before);
      const stub = made[0];
      t(made.length === 1 && same(stub.pts[0], tip) && stub.pts[1][0] === tip[0] && stub.pts[1][1] - tip[1] === 1270, `a stub 1.27 mm long runs from the pin's end down, out of the symbol (${JSON.stringify(stub && stub.pts)})`);
      t(__eda.state().tool === "drag" && same(selectedIds(), [stub?.id]), `the Drag tool holds it (${__eda.state().tool}; selected ${selectedIds()})`);
      const [px, py] = screenOf(tip[0], tip[1] + 5080);
      fire("pointermove", px, py); await sleep(500);
      fire("pointermove", px + 1, py + 1); await sleep(600);
      fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1500);
      const moved = (await newItems("wires", before))[0];
      t(moved && same(moved.pts[0], tip) && moved.pts[moved.pts.length - 1][1] === tip[1] + 5080, `a click drops it: the far end follows the pointer, the end at the pin stays (${JSON.stringify(moved && moved.pts)})`);
      t(__eda.state().tool === "select", "and the tool is the selection tool again");
      await undo();
      t((await newItems("wires", before)).length === 0, "one Undo takes the stub and the drag away");
      // Escape: the commit is reverted
      await select(["pin:U1:12"]);
      await __eda.run("studio.PinHelpers.wire");
      await sleep(1500);
      t((await newItems("wires", before)).length === 1, "the stub is there while it is held");
      await __eda.run("common.Interactive.cancel");
      await sleep(1500);
      t((await newItems("wires", before)).length === 0 && __eda.state().tool === "select", "Escape takes it away again (`commit.Revert()`)");
    });

    await scenario("Pin Helpers: a sheet pin on the root sheet gets a global label named by it, in the shape of its pin, reading out of the sheet", async (t) => {
      await leave();
      const rootSheet = await sch("");
      const mcu = rootSheet.sheets.find((x) => x.id === MCU);
      const pin = mcu.pins[0];
      t(!!pin && !!pin.id, `the MCU sheet has pins on the root sheet (${mcu.pins.length})`);
      const before = await sch("");
      await select([pin.id]);
      await __eda.run("studio.PinHelpers.globalLabel");
      await sleep(1400);
      const after = await sch("");
      const made = after.labels.filter((l) => !before.labels.some((o) => o.id === l.id));
      t(made.length === 1 && made[0].net === pin.name && made[0].scope === "global" && same(made[0].at, pin.at), `a global label named ${pin.name} is at the sheet pin (${JSON.stringify(made.map((l) => [l.net, l.scope, l.at]))})`);
      t(made[0] && made[0].shape === (pin.shape === "passive" ? "input" : pin.shape), `in the shape of the pin, a passive one being an input (${made[0] && made[0].shape} for ${pin.shape})`);
      t(same(selectedIds(), [made[0]?.id]), `and it is the selection (${selectedIds()})`);
      await undo();
      t((await sch("")).labels.length === before.labels.length, "one Undo takes it away");
      await enterSheet(MCU);
    });


    // ----- swapped pins through the clipboard
    await scenario("Swapped pins: Copy and Paste and Duplicate give the copy the same pins", async (t) => {
      const swapOf = async (id) => (await sch(MCU)).symbols.filter((x) => x.id === id).map((x) => x.pin_swaps ?? null)[0];
      await post({ op: "sch_edit", verb: "swap_pins", pins: ["pin:U1:3", "pin:U1:4"] }, MCU);
      await sleep(900);
      const s0 = await sch(MCU);
      const swaps = await swapOf("U1");
      t(swaps && swaps["3"] === "4" && swaps["4"] === "3", `U1's pins 3 and 4 have traded places (${JSON.stringify(swaps)})`);
      // Duplicate: the copy is carried; a click drops it
      await fitTo(["U1"]);
      await select(["U1"]);
      await __eda.run("common.Interactive.duplicate");
      await sleep(1500);
      const [qx, qy] = screenOf(s0.symbols.find((x) => x.id === "U1").at[0] + 60000, s0.symbols.find((x) => x.id === "U1").at[1]);
      fire("pointermove", qx, qy); await sleep(900);
      fire("pointerdown", qx, qy, { buttons: 1 }); await sleep(80); fire("pointerup", qx, qy); await sleep(1800);
      const s1 = await sch(MCU);
      const copy = s1.symbols.find((x) => !s0.symbols.some((o) => o.id === x.id));
      t(!!copy && copy.id !== "U1", `Duplicate made a symbol of its own (${copy && copy.id})`);
      t(copy && copy.pin_swaps && copy.pin_swaps["3"] === "4" && copy.pin_swaps["4"] === "3", `with the same pins traded (${JSON.stringify(copy && copy.pin_swaps)})`);
      await undo();
      t(!(await sch(MCU)).symbols.some((x) => !s0.symbols.some((o) => o.id === x.id)), "one Undo takes the copy away");
      // Copy and Paste
      await select(["U1"]);
      await __eda.run("common.Interactive.copy");
      await sleep(900);
      await __eda.run("common.Interactive.paste");
      await sleep(1500);
      const [rx, ry] = screenOf(s0.symbols.find((x) => x.id === "U1").at[0] + 60000, s0.symbols.find((x) => x.id === "U1").at[1]);
      fire("pointermove", rx, ry); await sleep(900);
      fire("pointerdown", rx, ry, { buttons: 1 }); await sleep(80); fire("pointerup", rx, ry); await sleep(1800);
      const s2 = await sch(MCU);
      const pasted = s2.symbols.find((x) => !s0.symbols.some((o) => o.id === x.id));
      t(!!pasted && pasted.pin_swaps && pasted.pin_swaps["3"] === "4" && pasted.pin_swaps["4"] === "3", `a pasted copy has them too (${JSON.stringify(pasted && [pasted.id, pasted.pin_swaps])})`);
      await undo();
      await fit();
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await leave();
    const end = await snapshotOf(MCU);
    results.push({ name: "end: the sheet is as it was", ok: end === base, notes: [`${end === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
