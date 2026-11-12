// Schematic wire to board ratsnest (UI-LEFT-2 CC10; PARITY-sch "Manual click-through" step 7; task-v10-handlers): a wire drawn on the schematic with the W tool
// joins two nets, and the board shows it as a ratsnest line on the PCB tab -- KiCad's Update PCB from Schematic carries the new net to the pads
// (`BOARD_NETLIST_UPDATER::updateComponentPadConnections`) and the board's connectivity draws the line (`RATSNEST_VIEW_ITEM`); here the two editors are views of
// one design.json, so the line is there as soon as the wire is (`GET /api/ratsnest` goes through the same `board::load` that applies the schematic's nets).
// Until now that path was proven in Rust only.
// The wire runs on the Header (J1) sheet between the ends of the SWD and RESET label wires (J1.3 and J1.4, 2.54 mm apart): before it, J1.3's ratsnest goes to
// U1 on the SWD net and J1.4's to U1 on RESET, in two separate sets; after it, one net holds the four pads and the board's ratsnest joins J1.3 and J1.4. The
// line is read where the canvas draws it from (`__eda.state().ratsnest`, the store's copy of `GET /api/ratsnest`) and from the server. One undo on the
// schematic takes the wire away and the board's two nets come back.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml with the parts placed (`eda place examples/mcu_board_30plus.yaml -o <t>`,
// `eda board new examples/mcu_board_30plus.yaml -o <dir> --origin 0,0`, then copy <t>/design.json into <dir>; `eda board serve -C <dir> --port N --ui web/studio/dist`)
// with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/sch-wire-ratsnest.check.js`.
// run-all-checks: board=mcu30-placed minutes=6   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__schWireRatsnestCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const run = (id, args) => __eda.run(id, args);
  const state = () => __eda.state();

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // The sets of pads the ratsnest lines join (union-find over the lines' two ends): whether `a` and `b` are in one.
  const joined = (edges, a, b) => {
    const up = new Map();
    const find = (x) => { while (up.has(x) && up.get(x) !== x) x = up.get(x); return x; };
    for (const e of edges) { if (!e.from || !e.to) continue; const [x, y] = [find(e.from), find(e.to)]; if (x !== y) up.set(x, y); }
    return find(a) === find(b);
  };
  const serverEdges = async () => (await json("/api/ratsnest")).edges.map((e) => ({ from: e.from_id, to: e.to_id, net: e.net }));
  const pinNets = async (sheet) => Object.fromEntries((await json(`/api/schematic?sheet=${sheet}`)).symbols.filter((s) => s.id === "J1").flatMap((s) => s.pins.map((p) => [`J1.${p.number}`, p.net])));

  const toTab = async (name, tab) => {
    if (state().tab === tab) return;
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === name)?.click();
    await until(() => state().tab === tab, 8000);
    await sleep(800);
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(150); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(500); };
  const dblclickAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    const [cx, cy] = [rect.left + px, rect.top + py];
    document.elementFromPoint(cx, cy).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: cx, clientY: cy, detail: 2 }));
    await sleep(900);
  };

  // The Header sheet and the two label wire ends on it (examples/mcu_board_30plus.yaml: SWD at (142.24, 90.17) mm, RESET at (142.24, 92.71) mm).
  const MM = 1_000_000;
  const A = [142.24 * MM, 90.17 * MM];
  const B = [142.24 * MM, 92.71 * MM];

  (async () => {
    await toTab("Schematic", "schematic");
    while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
    const root = await json("/api/schematic");
    const header = root.sheets.find((s) => /Header/.test(s.name ?? ""));
    if (!header) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs examples/mcu_board_30plus.yaml's Header (J1) sheet"] }); check.done = true; return; }
    const before = await serverEdges();
    const nets0 = await pinNets(header.id);
    const wires0 = (await json(`/api/schematic?sheet=${header.id}`)).wires.length;

    await scenario("before: J1.3 (SWD) and J1.4 (RESET) are two nets, and the board's ratsnest keeps their pads apart", async (t) => {
      t(nets0["J1.3"] && nets0["J1.4"] && nets0["J1.3"] !== nets0["J1.4"], `two nets on the schematic (${nets0["J1.3"]}, ${nets0["J1.4"]})`);
      t(before.some((e) => [e.from, e.to].includes("J1.3")) && before.some((e) => [e.from, e.to].includes("J1.4")), "each pad has a ratsnest line");
      t(!joined(before, "J1.3", "J1.4"), "no chain of ratsnest lines joins J1.3 and J1.4");
    });

    await scenario("W on the Header sheet: a wire between the SWD and RESET label wires makes one net on the schematic", async (t) => {
      await run("common.InteractiveSelection.clear");
      await run("common.InteractiveSelection.selectItems", [header.id]);
      await run("eeschema.NavigateTool.enterSheet");
      t(!!(await until(() => state().sheetPath.length === 1 && state().sheetPath[0] === header.id, 8000)), `the Header sheet is open (${JSON.stringify(state().sheetPath)})`);
      await run("common.InteractiveSelection.clear");
      await run("common.Control.zoomFitScreen");
      await sleep(600);
      await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await sleep(300);
      await clickAt(...A);
      await clickAt(...B);
      await dblclickAt(...B);
      for (let i = 0; i < 3 && state().tool !== "select"; i++) { await run("common.Interactive.cancel"); await sleep(400); }
      const s = await json(`/api/schematic?sheet=${header.id}`);
      const added = s.wires.slice(wires0);
      t(s.wires.length === wires0 + 1, `one wire was added (${s.wires.length - wires0})`);
      t(added.length === 1 && JSON.stringify(added[0].pts) === JSON.stringify([A, B]), `from the SWD wire's end to the RESET wire's (${JSON.stringify(added[0]?.pts)})`);
      const nets = await pinNets(header.id);
      t(nets["J1.3"] === nets["J1.4"] && !!nets["J1.3"], `J1.3 and J1.4 are one net now (${nets["J1.3"]}, ${nets["J1.4"]})`);
    });

    await scenario("the PCB tab: the board's ratsnest has the new connection, J1.3 joined to J1.4, where the canvas draws it from and on the server", async (t) => {
      await toTab("PCB", "pcb");
      const drawn = await until(() => { const r = state().ratsnest; return r && joined(r.edges, "J1.3", "J1.4") ? r : null; }, 15000);
      t(!!drawn, `the canvas's ratsnest joins J1.3 and J1.4 (${JSON.stringify(state().ratsnest?.edges.filter((e) => [e.from, e.to].some((x) => /^J1\.[34]$/.test(x ?? ""))))})`);
      t(!!drawn?.shown, "the ratsnest layer is shown");
      const direct = drawn?.edges.find((e) => [e.from, e.to].sort().join() === "J1.3,J1.4");
      t(!!direct, `a ratsnest line runs from J1.3 to J1.4, the two pads the wire joined (${JSON.stringify(direct)})`);
      const server = await serverEdges();
      t(joined(server, "J1.3", "J1.4") && server.length === before.length + 1, `GET /api/ratsnest agrees: one line more than before (${before.length} -> ${server.length})`);
    });

    await scenario("one undo on the schematic takes the wire away, and the board's ratsnest has the two nets apart again", async (t) => {
      await toTab("Schematic", "schematic");
      await run("common.Interactive.undo");
      await sleep(800);
      t((await json(`/api/schematic?sheet=${header.id}`)).wires.length === wires0, "the wire is gone");
      const nets = await pinNets(header.id);
      t(nets["J1.3"] === nets0["J1.3"] && nets["J1.4"] === nets0["J1.4"], `the nets are as they were (${nets["J1.3"]}, ${nets["J1.4"]})`);
      await toTab("PCB", "pcb");
      const apart = await until(() => { const r = state().ratsnest; return r && !joined(r.edges, "J1.3", "J1.4") ? r : null; }, 15000);
      t(!!apart, "the canvas's ratsnest keeps J1.3 and J1.4 apart again");
      const server = await serverEdges();
      t(JSON.stringify(server) === JSON.stringify(before), `GET /api/ratsnest is what it was (${server.length} lines)`);
    });

    // U1's two no-connect pins (NC1, NC2; `PT_NC`) wired together on the MCU sheet: KiCad 25ec2335's connectivity makes a no-connect pin a contact that joins
    // nothing (`conn_islands.cpp::BuildScreenIslands`: `if( a.kind == NC || b.kind == NC ) continue;`), and 8303b2ad's netlist updater gives every no-connect
    // pad a net of its own (`BOARD_NETLIST_UPDATER::updateComponentPadConnections`, `IsNoConnectPad`: the `_1` suffix) -- so the board gets no ratsnest line.
    const mcu = root.sheets.find((s) => /MCU/.test(s.name ?? ""));
    if (mcu) await scenario("U1's two no-connect pins wired together stay apart on the board, as KiCad: no ratsnest line between U1.12 and U1.13", async (t) => {
      await toTab("Schematic", "schematic");
      while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
      await run("common.InteractiveSelection.clear");
      await run("common.InteractiveSelection.selectItems", [mcu.id]);
      await run("eeschema.NavigateTool.enterSheet");
      t(!!(await until(() => state().sheetPath[0] === mcu.id, 8000)), "the MCU sheet is open");
      await run("common.InteractiveSelection.clear");
      await run("common.Control.zoomFitScreen");
      await sleep(600);
      const w0 = (await json(`/api/schematic?sheet=${mcu.id}`)).wires.length;
      const [x1, x2, y, y2] = [149.86 * MM, 152.4 * MM, 88.9 * MM, 91.44 * MM];
      await run("eeschema.InteractiveDrawingLineWireBus.drawWires");
      await sleep(300);
      for (const p of [[x1, y], [x1, y2], [x2, y2]]) await clickAt(...p);
      await clickAt(x2, y);
      await dblclickAt(x2, y);
      for (let i = 0; i < 3 && state().tool !== "select"; i++) { await run("common.Interactive.cancel"); await sleep(400); }
      const s = await json(`/api/schematic?sheet=${mcu.id}`);
      t(s.wires.length > w0, `the wire was drawn (${s.wires.length - w0} segment(s))`);
      const u1 = s.symbols.find((x) => x.id === "U1");
      const nc = u1.pins.filter((p) => p.number === "12" || p.number === "13").map((p) => p.net ?? null);
      t(nc.length === 2 && (nc[0] === null || nc[0] !== nc[1]), `the two pins are not one net (${JSON.stringify(nc)})`);
      await toTab("PCB", "pcb");
      await sleep(1500);
      const r = state().ratsnest;
      t(!!r && !joined(r.edges, "U1.12", "U1.13"), "the board's ratsnest has no line between U1.12 and U1.13");
      t(JSON.stringify(await serverEdges()) === JSON.stringify(before), "GET /api/ratsnest is unchanged");
      await toTab("Schematic", "schematic");
      await run("common.Interactive.undo");
      await sleep(800);
      t((await json(`/api/schematic?sheet=${mcu.id}`)).wires.length === w0, "one undo takes the wire away");
    });

    await toTab("Schematic", "schematic");
    while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); }
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
