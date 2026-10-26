// Board Statistics and Swap Layers check (GAPS.md item 33, the checks P20 left uncovered; PARITY-pcb.md section 19): Inspect > Show Board
// Statistics -- every number computed by kicad-cli's `pcb export stats` through POST /api/board_stats (the Components, Pads and Vias totals
// against the board's own counts, the Board grid's rows filled, the three checkboxes re-running the computation and their ticks kept across
// closings, the Drill Holes page's header sort toggling ascending/descending) -- and Edit > Swap Layers... -- F.Cu and B.Cu exchanged for
// tracks and zones at once, through vias staying put, one undo step, and nothing sent when every row is left on itself.
// Run it in the page of a served, routed board with copper on both sides -- `eda pipeline examples/mcu_board_30plus.yaml -o <p>`,
// `eda board new examples/mcu_board_30plus.yaml -o <dir>`, `cp <p>/design.json <dir>/`, `eda board serve -C <dir> --port N --ui
// web/studio/dist`, kicad-cli on the PATH or in EDA_KICAD_CLI -- with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs
// http://localhost:N/ web/studio/e2e/board-stats.check.js`, or pasted into the Browser pane's javascript tool. It edits the routing (Swap
// Layers) and undoes it, so run it on a scratch copy.
// It answers through `window.__boardStatsCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __boardStatsCheck.wait(120000)` until `.done` (kicad-cli takes seconds per stats run).
// run-all-checks: board=mcu30-zones minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__boardStatsCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 30000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(200); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const run = (id, args) => __eda.run(id, args);
  const dialog = () => document.querySelector(".dialog");
  const header = () => dialog()?.querySelector(".dialog-header")?.textContent.trim() ?? null;
  const buttonText = (root, re) => [...(root?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const labelled = (text) => [...(dialog()?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim().startsWith(text));
  const box = (text) => labelled(text)?.querySelector("input");
  const tabButton = (text) => [...(dialog()?.querySelectorAll(".editor-tab") ?? [])].find((b) => b.textContent.trim() === text);
  const tables = () => [...(dialog()?.querySelectorAll("table.setup-table") ?? [])];
  const tableRows = (t) => [...(t?.querySelectorAll("tbody tr") ?? [])].map((tr) => [...tr.children].map((td) => td.textContent.trim()));
  const totalRow = (t) => tableRows(t).find((r) => r[0] === "Total:");
  const num = (text) => { const m = /-?[0-9][0-9.,]*/.exec(text ?? ""); return m ? Number(m[0].replace(/,/g, "")) : NaN; };
  const close = async () => { for (let i = 0; i < 3 && dialog(); i++) { (buttonText(dialog(), /^(Close|Cancel)$/) ?? [...(dialog()?.querySelectorAll("button") ?? [])].find((b) => /^(Close|Cancel)$/.test(b.textContent.trim())))?.click(); await sleep(250); } };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const areaOf = () => { const b = tables()[3]; const r = tableRows(b).find((x) => x[0] === "Area:"); return r ? num(r[1]) : NaN; };
  /** The routing as Swap Layers looks at it: the tracks' layers, the zones' layers, the vias' layer pair. */
  const routing = async () => {
    const s = await state();
    return {
      tracks: (s.routing?.tracks ?? []).map((t) => [t.id, t.layer]),
      zones: (s.routing?.zones ?? []).map((z) => [z.id, z.layer, z.layers ?? [z.layer]]),
      vias: (s.routing?.vias ?? []).map((v) => [v.id, v.from, v.to]),
    };
  };
  const swapped = (l) => (l === "F.Cu" ? "B.Cu" : l === "B.Cu" ? "F.Cu" : l);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await close(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  const st = await state();
  if (!st.routing?.tracks?.length) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs routed tracks (eda pipeline mcu_board_30plus)" ] }); check.done = true; return "no routing"; }
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  // Strict (the default) refuses a command that adds gate failures: swapping F.Cu and B.Cu moves the tracks and zones but not the SMD pads, so the
  // swap would be refused for the connectivity it breaks. The check is of what the dialog does with the board, so it runs with strict off, as
  // board-edit.check.js does.
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();

  (async () => {
    await scenario("Board Statistics: kicad-cli's numbers, the totals the board's own counts, and a checkbox re-runs the computation", async (t) => {
      const r = await run("pcbnew.InspectionTool.ShowBoardStatistics");
      t(r.dialog === "Board Statistics", `Inspect > Show Board Statistics opens "Board Statistics" (${r.dialog ?? r.error ?? header()})`);
      const loaded = await until(() => tables().length >= 4, 90000);
      if (!t(loaded, "the dialog fills from POST /api/board_stats")) return;
      const s = await state();
      const comp = totalRow(tables()[0]);
      const parts = s.parts.length;
      t(comp && Number(comp[3]) === parts, `Components Total is the board's ${parts} footprint(s) (${JSON.stringify(comp)})`);
      const pads = s.parts.flatMap((p) => p.pads ?? []).length;
      const padTotal = totalRow(tables()[1]);
      t(padTotal && Number(padTotal[1]) === pads, `Pads Total is the board's ${pads} pad(s) (${JSON.stringify(padTotal)})`);
      const vias = s.routing?.vias?.length ?? 0;
      const viaTotal = totalRow(tables()[2]);
      t(viaTotal && Number(viaTotal[1]) === vias, `Vias Total is the board's ${vias} via(s) (${JSON.stringify(viaTotal)})`);
      const board = tableRows(tables()[3]);
      const row = (label) => board.find((x) => x[0] === label)?.[1] ?? "";
      t(row("Dimensions:") !== "" && row("Dimensions:") !== "unknown", `the Board grid has the dimensions (${row("Dimensions:")})`);
      t(num(row("Min track width:")) > 0, `and a real minimum track width (${row("Min track width:")})`);
      t(num(row("Min drill diameter:")) > 0, `and a real minimum drill (${row("Min drill diameter:")})`);
      const before = areaOf();
      t(Number.isFinite(before) && before > 0, `the board area is a number (${before})`);
      const cb = box("Subtract holes from board area");
      if (!t(!!cb, "the three checkboxes are there")) return;
      cb.click();
      const after = await until(() => { const a = areaOf(); return Number.isFinite(a) && a !== before ? a : 0; }, 60000);
      t(after > 0 && after < before, `unticked, "Subtract holes from board area" re-runs kicad-cli and the area drops (${before} -> ${after})`);
      // Close with the box still ticked and reopen: `s_savedDialogState` keeps it, and the area is the subtracted one.
      await close();
      await run("pcbnew.InspectionTool.ShowBoardStatistics");
      const again = await until(() => tables().length >= 4 && areaOf(), 90000);
      t(again > 0 && again < before, `closed and reopened, the tick is kept and so is the subtracted area (${again})`);
      t(box("Subtract holes from board area")?.checked === true, "the box is still ticked");
      box("Subtract holes from board area")?.click(); // leave the session's saved state as the check found it
      await until(() => Math.abs(areaOf() - before) < 1e-9, 60000);
    });

    await scenario("Drill Holes: the page's header sort toggles ascending and descending on each click", async (t) => {
      const r = await run("pcbnew.InspectionTool.ShowBoardStatistics");
      t(r.dialog === "Board Statistics", `reopened (${r.dialog ?? r.error})`);
      const loaded = await until(() => tables().length >= 4, 90000);
      if (!t(loaded, "the dialog fills")) return;
      tabButton("Drill Holes")?.click();
      // The page is rendered after the click: wait for its own header (the General page's tables are gone by then).
      const drills = await until(() => {
        const headers = [...(dialog()?.querySelectorAll("th") ?? [])].map((h) => h.textContent.trim());
        const rows = headers.some((h) => h.startsWith("X Size")) ? tableRows(tables()[0]) : null;
        return rows && rows.length >= 2 && rows;
      }, 10000);
      if (!t(drills, "the Drill Holes page lists the holes")) return;
      const x = () => tableRows(tables()[0]).map((row) => num(row[2]));
      const xs = x();
      const distinct = new Set(xs).size;
      t(distinct >= 2, `the holes have more than one X size to order by (${[...new Set(xs)].join(", ")})`);
      const clickHeader = async (label) => { const th = [...dialog().querySelectorAll("th")].find((h) => h.textContent.trim().startsWith(label)); th?.click(); await sleep(200); };
      await clickHeader("X Size");
      const head1 = [...dialog().querySelectorAll("th")].find((h) => h.textContent.trim().startsWith("X Size"))?.textContent.trim() ?? "";
      const asc = x();
      t(/▲/.test(head1), `the first click marks it ascending (${head1})`);
      t(asc.every((v, i) => i === 0 || asc[i - 1] <= v), `and the rows run smallest first (${asc.join(", ")})`);
      await clickHeader("X Size");
      const head2 = [...dialog().querySelectorAll("th")].find((h) => h.textContent.trim().startsWith("X Size"))?.textContent.trim() ?? "";
      const desc = x();
      t(/▼/.test(head2), `the second click marks it descending (${head2})`);
      t(desc.every((v, i) => i === 0 || desc[i - 1] >= v), `and the rows run largest first (${desc.join(", ")})`);
      t(drills.every((row) => /^(PTH|NPTH)$/.test(row[4]) && /^(Via|Pad)$/.test(row[5])), "each row says plated and whether it is a via or a pad");
    });

    await scenario("Swap Layers: F.Cu and B.Cu trade places at once, through vias stay, and one undo puts it all back", async (t) => {
      const s = await state();
      const layers = s.layers ?? ["F.Cu", "B.Cu"];
      if (!t(layers.includes("F.Cu") && layers.includes("B.Cu"), `the board has both copper sides (${layers.join(", ")})`)) return;
      const before = await routing();
      t(before.tracks.some(([, l]) => l === "F.Cu"), "there are F.Cu tracks to move");
      const r = await run("pcbnew.GlobalEdit.swapLayers");
      t(r.dialog === "Swap Layers", `Edit > Swap Layers... opens "Swap Layers" (${r.dialog ?? r.error ?? header()})`);
      const rows = [...(dialog()?.querySelectorAll("tbody tr") ?? [])];
      const selectFor = (layer) => rows.find((tr) => tr.children[0]?.textContent.trim() === layer)?.querySelector("select");
      const f = selectFor("F.Cu"), b = selectFor("B.Cu");
      if (!t(!!f && !!b, "one row per copper layer, in UI order")) return;
      t([...f.options].map((o) => o.value).join(",") === layers.join(","), `the destination picker offers the copper layers only (${[...f.options].map((o) => o.value).join(", ")})`);
      setValue(f, "B.Cu");
      setValue(b, "F.Cu");
      await sleep(150);
      buttonText(dialog(), /^OK$/)?.click();
      const closed = await until(async () => !dialog(), 15000);
      if (!t(closed, "OK closes the dialog")) return;
      const after = await until(async () => { const a = await routing(); return JSON.stringify(a) !== JSON.stringify(before) ? a : null; }, 15000);
      if (!t(after, "the routing changed")) return;
      const tracksOk = after.tracks.every(([id, l]) => l === swapped(before.tracks.find(([x]) => x === id)?.[1]));
      t(tracksOk, `every track is on the other side (${before.tracks.filter(([, l]) => l === "F.Cu").length} F.Cu -> B.Cu at least)`);
      const zonesOk = after.zones.every(([id, l, ls]) => {
        const was = before.zones.find(([x]) => x === id);
        return was && l === swapped(was[1]) && ls.join(",") === was[2].map(swapped).join(",");
      });
      t(zonesOk, `every zone's layer set is swapped (${before.zones.length} zone(s))`);
      const viaPair = (v) => [v[1], v[2]].sort().join("|");
      t(after.vias.every((v) => viaPair(v) === viaPair(before.vias.find(([id]) => id === v[0]))), "through vias keep their layer pair");
      await run("common.Interactive.undo");
      const undone = await until(async () => { const a = await routing(); return JSON.stringify(a) === JSON.stringify(before) ? a : null; }, 15000);
      t(undone, "one Ctrl+Z restores the tracks, the zones and the vias");
    });

    await scenario("Swap Layers with every row on itself sends nothing", async (t) => {
      const before = await routing();
      const act0 = (await state()).activity?.[0] ?? null;
      const r = await run("pcbnew.GlobalEdit.swapLayers");
      t(r.dialog === "Swap Layers", `the dialog opens (${r.dialog ?? r.error})`);
      buttonText(dialog(), /^OK$/)?.click();
      const closed = await until(async () => !dialog(), 10000);
      t(closed, "OK closes it");
      await sleep(600);
      t(JSON.stringify(await routing()) === JSON.stringify(before), "the routing is untouched");
      const act1 = (await state()).activity?.[0] ?? null;
      t((act0?.t ?? null) === (act1?.t ?? null) && (act0?.cmd ?? null) === (act1?.cmd ?? null), `no command was recorded (${JSON.stringify(act1?.cmd ?? null)})`);
    });

    await scenario("no error was logged", async (t) => {
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
