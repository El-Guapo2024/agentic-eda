// Zone fill check (GAPS.md package P13; item 13): the zone fill is KiCad's, through kicad-cli, and nobody else's. Fill All Zones (B) runs kicad-cli's filler on a copy of the
// derived board and the fills are kept beside the design; the board shows the last fill, as KiCad does with auto-refill off; a zone never filled, or edited since, is drawn as an
// outline. The scenarios: a board with nothing filled; B (the status bar says so while it runs, the toast says what filled and how long it took, the derived .kicad_pcb carries
// kicad-cli's rings); an edit elsewhere leaves the fills and an edit to a zone empties that zone only, Undo giving it back; Fill Selected Zones and Unfill Selected Zones; Unfill
// All Zones; the Zone Manager (the preview with a tab per layer of the zone, Auto-Assign Priorities on zones that overlap, its Refill zones box); the lock in Zone Properties;
// "Refill all zones before performing DRC" and the Plot dialog's "Check zone fills before plotting".
// Run it in the page of a served board that has footprints placed and two copper pours of one net on the two outer layers (examples/mcu_board_30plus.yaml, placed and routed,
// `eda board zone add --net GND --layer F.Cu --pts "2.9,5 28.3,5 28.3,29.2 2.9,29.2"` and the same on B.Cu) -- paste it into the Browser pane's javascript tool, or
// `page.evaluate( src )` in Playwright -- on a COPY of a board (it edits and undoes, and it leaves every zone unfilled when it ends, the state a board that was never filled
// starts in). Every kicad-cli run takes a few seconds, so it runs on its own: read it with `await __zonesCheck.wait(40000)` until `.done`; it answers through
// `window.__zonesCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }.
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, B with `pcbnew.ZoneFiller.zoneFillAll`, the Zone Manager
// with `pcbnew.Control.zonesManager`), the dialogs' controls (their `data-testid`s) and what the server holds (`GET /api/fill`, `/api/state`, `/api/board.kicad_pcb`).
// run-all-checks: board=mcu30-zones minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__zonesCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const fill = () => json("/api/fill");
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const postJson = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const notes = () => [...document.querySelectorAll(".status-bar [role=status]")].map((e) => e.textContent);
  /** The fills of each zone as the server serves them: [layer, filled, number of islands]. */
  const byZone = (f) => Object.fromEntries(f.zones.map((z) => [`${z.id}@${z.layer}`, [z.filled, z.fragments.length, Math.round(z.area_um2 / 1e6)]]));
  const rings = async (layer) => { const text = await fetch("/api/board.kicad_pcb").then((r) => r.text()); return (text.match(new RegExp(`\\(filled_polygon\\s*\\(layer "${layer}"\\)`, "g")) ?? []).length; };
  const waitFill = async (pred, ms = 15000) => { const t0 = Date.now(); let f = await fill(); while (!pred(f) && Date.now() - t0 < ms) { await sleep(300); f = await fill(); } return f; };
  const zonesOf = async () => ((await state()).routing?.zones ?? []).filter((z) => !z.is_rule_area);
  const editZone = (z, patch) => post({ op: "edit_zone", ...z, ...patch });
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  const startZones = await zonesOf();
  if (startZones.length < 2 || !startZones.every((z) => z.net)) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs two copper pours on nets"] }); check.done = true; return "no zones"; }
  const [A, B] = startZones;
  const startState = JSON.stringify(startZones.map((z) => [z.id, z.priority, z.min_thickness, z.outline]));
  await postJson("/api/fill/clear", {});
  await sleep(1200);

  (async () => {
    await scenario("a board that was never filled shows its zones as outlines, and says so", async (t) => {
      const f = await fill();
      t(f.source === null, "no fill has been made: no source");
      t(f.zones.length >= 2 && f.zones.every((z) => z.filled === false && z.fragments.length === 0 && z.area_um2 === 0), `every zone is unfilled (${f.zones.length} zone layers)`);
      t(notes().some((n) => /\d+ zones? not filled \(B\)/.test(n)), `the status bar says ${JSON.stringify(notes())}`);
      t((await rings(A.layer)) === 0, "the derived .kicad_pcb has no filled_polygon: a board that was never filled is written unfilled");
    });

    await scenario("B runs kicad-cli's filler: the status bar says so, the toast says what it did, the board and the derived file carry kicad's rings", async (t) => {
      const t0 = Date.now();
      const run = __eda.run("pcbnew.ZoneFiller.zoneFillAll");
      await sleep(350);
      t(notes().some((n) => /Filling zones/.test(n)), `while it runs the status bar says so (${JSON.stringify(notes())})`);
      const r = await run;
      const seconds = (Date.now() - t0) / 1000;
      t(r.ok === true && /filled \d+ zones? in [\d.]+ s/.test(r.toast ?? ""), `the toast says what was filled (${r.toast}) in ${seconds.toFixed(1)} s`);
      const f = await waitFill((x) => x.zones.every((z) => z.filled));
      t(f.zones.every((z) => z.filled && z.fragments.length > 0 && z.area_um2 > 1e6), `every zone has a fill with copper: ${JSON.stringify(byZone(f))}`);
      t(/^kicad-cli /.test(f.source?.engine ?? "") && f.source.zones >= 2 && f.source.stale === 0 && f.source.seconds > 0, `the source is kicad-cli's (${JSON.stringify(f.source)})`);
      await sleep(900);
      t(notes().every((n) => !/not filled|Filling/.test(n)), `the status bar says nothing more about the zones (${JSON.stringify(notes())})`);
      t((await rings(A.layer)) > 0 && (await rings(B.layer)) > 0, "the derived .kicad_pcb holds kicad-cli's filled_polygon on both layers");
      check.fillSeconds = seconds;
    });

    await scenario("an edit elsewhere leaves the fills as they are, and an edit to a zone empties that zone only; Undo gives it back", async (t) => {
      const before = byZone(await fill());
      const net = A.net;
      const added = await post({ op: "add_track", net, layer: "F.Cu", width: 250, pts: [{ x: 6000, y: 8000 }, { x: 9000, y: 8000 }] });
      t(added.ok, `a track is added (${added.message})`);
      await sleep(1200);
      t(JSON.stringify(byZone(await fill())) === JSON.stringify(before), "the fills are what they were: a pour is not refilled by an edit, as in KiCad with auto-refill off");
      await undo();
      const zA = (await zonesOf()).find((z) => z.id === A.id);
      const e = await editZone(zA, { min_thickness: zA.min_thickness + 10 });
      t(e.ok, `zone A's minimum width is changed (${e.message})`);
      const f = await waitFill((x) => x.zones.some((z) => z.id === A.id && !z.filled));
      const now = byZone(f);
      t(f.zones.filter((z) => z.id === A.id).every((z) => !z.filled && z.fragments.length === 0), "zone A has no fill any more: it is drawn as an outline until it is filled again");
      t(JSON.stringify(Object.entries(now).filter(([k]) => !k.startsWith(A.id))) === JSON.stringify(Object.entries(before).filter(([k]) => !k.startsWith(A.id))), "zone B keeps its fill");
      t(f.source?.stale === 1 && f.source.zones === f.zones.length - f.zones.filter((z) => z.id === A.id).length, `the source counts the zone out of date (${JSON.stringify(f.source)})`);
      // the page learns of the edit with its next poll of the fills, a second or so after the server: wait for it rather than a fixed time
      for (let i = 0; i < 40 && !notes().some((n) => /1 zone not filled \(B\)/.test(n)); i++) await sleep(200);
      t(notes().some((n) => /1 zone not filled \(B\)/.test(n)), `the status bar counts it (${JSON.stringify(notes())})`);
      t((await rings(A.layer)) === (A.layer === B.layer ? await rings(B.layer) : 0), "and the derived .kicad_pcb is written without its rings, as KiCad writes a zone it has emptied");
      await undo();
      const back = await waitFill((x) => x.zones.every((z) => z.filled));
      t(JSON.stringify(byZone(back)) === JSON.stringify(before), "Undo puts the zone back as it was filled: the fill is current again");
    });

    await scenario("Fill Selected Zones fills the selected zone only, Unfill Selected Zones takes only the selected one away, Unfill All Zones takes the rest", async (t) => {
      const zA = (await zonesOf()).find((z) => z.id === A.id);
      await editZone(zA, { min_thickness: zA.min_thickness + 10 });
      await waitFill((x) => x.zones.some((z) => z.id === A.id && !z.filled));
      const bBefore = byZone(await fill());
      await select([A.id]);
      const r = await __eda.run("pcbnew.ZoneFiller.zoneFill");
      t(r.ok === true, `Fill Selected Zones runs (${r.toast ?? r.error ?? ""})`);
      let f = await waitFill((x) => x.zones.every((z) => z.filled));
      t(f.zones.every((z) => z.filled && z.fragments.length > 0), "both zones are filled: A was filled again and B kept its fill");
      const bAfter = byZone(f);
      t(Object.entries(bAfter).filter(([k]) => !k.startsWith(A.id)).every(([k, v]) => JSON.stringify(v) === JSON.stringify(bBefore[k])), "zone B's fill is the one it had");
      await select([B.id]);
      await __eda.run("pcbnew.ZoneFiller.zoneUnfill");
      f = await waitFill((x) => x.zones.some((z) => z.id === B.id && !z.filled));
      t(f.zones.filter((z) => z.id === B.id).every((z) => !z.filled) && f.zones.filter((z) => z.id === A.id).every((z) => z.filled), "Unfill Selected Zones took B's fill away and left A's");
      await __eda.run("pcbnew.ZoneFiller.zoneUnfillAll");
      f = await waitFill((x) => x.source === null);
      t(f.source === null && f.zones.every((z) => !z.filled), "Unfill All Zones: every zone is an outline and nothing remembers a fill");
      t((await rings(A.layer)) === 0, "the derived .kicad_pcb has no filled_polygon");
      await select([]);
      await undo();
    });

    await scenario("the Zone Manager: a preview with a tab per layer of the zone, the fill in it, Auto-Assign Priorities and Refill zones", async (t) => {
      await __eda.run("pcbnew.ZoneFiller.zoneFillAll");
      await waitFill((x) => x.zones.every((z) => z.filled));
      // A third pour of another net over the first two: the overlap the priorities are about.
      const nets = [...new Set((await state()).parts.flatMap((p) => (p.pads ?? []).map((q) => q.net)).filter(Boolean))].filter((n) => n !== A.net);
      const other = nets[0];
      const added = await post({ op: "add_zone", net: other, layer: A.layer, outline: [{ x: 5000, y: 8000 }, { x: 15000, y: 8000 }, { x: 15000, y: 14000 }, { x: 5000, y: 14000 }] });
      t(added.ok, `a small pour of ${other} is added over the first (${added.message})`);
      await sleep(800);
      const small = (await zonesOf()).find((z) => !startZones.some((s) => s.id === z.id));
      t(!!small, "it is a zone of the board");
      const r = await __eda.run("pcbnew.Control.zonesManager");
      await sleep(500);
      t(r.dialog === "Zone Manager", `the action opens the dialog (${r.dialog})`);
      const rows = () => [...document.querySelectorAll(".dialog table tbody tr")];
      const row = (id) => rows().find((tr) => tr.cells[0].textContent === id);
      t(rows().length === 3, `it lists the three pours (${rows().length})`);
      t(!!tid("zone-preview") && !!tid("zone-auto-assign") && !!tid("zone-manager-refill"), "it has the preview, the Auto-Assign button and the Refill zones box");
      row(A.id).click();
      await sleep(400);
      const tabs = [...document.querySelectorAll('[data-testid^="zone-preview-tab-"]')].map((b) => b.textContent);
      t(tabs.length >= 1 && tabs.includes(A.layer), `the preview has a tab for each layer of the zone (${tabs})`);
      t(!!tid("zone-preview-fill") && !!tid("zone-preview-outline") && !!tid("zone-preview-board-box"), "it shows the board's edges, the zone's fill (kicad-cli's) and its outline");
      row(small.id).click();
      await sleep(400);
      t(!tid("zone-preview-fill") && !!tid("zone-preview-outline"), "the new pour has no fill yet: its preview is its outline alone");
      const staged = () => Object.fromEntries(rows().map((tr) => [tr.cells[0].textContent, Number(tr.cells[3].textContent)]));
      const priorityBefore = JSON.stringify((await zonesOf()).map((z) => [z.id, z.priority]));
      tid("zone-auto-assign").click();
      await sleep(900);
      const note = tid("zone-manager-note")?.textContent ?? "";
      t(/Priorities assigned from the overlaps/.test(note), `Auto-Assign stages the priorities (${note})`);
      const prio = staged();
      t(prio[small.id] > prio[A.id], `the small pour is above the one it lies on, as KiCad's rule says (${JSON.stringify(prio)})`);
      t(JSON.stringify((await zonesOf()).map((z) => [z.id, z.priority])) === priorityBefore, "nothing is applied until OK");
      tid("zone-manager-refill").click();
      buttonText(document, "OK").click();
      await sleep(1500);
      const after = Object.fromEntries((await zonesOf()).map((z) => [z.id, z.priority]));
      t(after[small.id] > after[A.id], `OK applies them (${JSON.stringify(after)})`);
      const f = await waitFill((x) => x.zones.every((z) => z.filled) && x.zones.length === 3, 25000);
      t(f.zones.length === 3 && f.zones.every((z) => z.filled), "and Refill zones ran kicad-cli's filler again: every zone is filled, the new pour too");
      await undo(); // the priorities
      await undo(); // the pour
      const left = (await zonesOf()).length;
      t(left === startZones.length, `the added pour is undone (${left} zones)`);
    });

    await scenario("Zone Properties has a lock that Undo takes back", async (t) => {
      await select([A.id]);
      const r = await __eda.run("pcbnew.InteractiveEdit.properties");
      await sleep(600);
      t(!!tid("zone-dialog") && !!tid("zone-locked"), `Zone Properties has a Locked box (${r.dialog})`);
      t(tid("zone-locked").checked === false, "an ordinary zone is not locked");
      tid("zone-locked").click();
      await sleep(150);
      tid("zone-ok").click();
      await sleep(1200);
      const st = await state();
      t((st.locked ?? []).includes(A.id), "OK locks the zone");
      await select([A.id]);
      await __eda.run("pcbnew.InteractiveEdit.properties");
      await sleep(600);
      t(tid("zone-locked")?.checked === true, "the dialog opens on a locked zone with the box checked");
      await closeDialogs();
      await undo();
      t(!((await state()).locked ?? []).includes(A.id), "Undo unlocks it");
      await select([]);
    });

    await scenario("Refill all zones before performing DRC and Check zone fills before plotting run kicad-cli's filler first and keep what it makes", async (t) => {
      await postJson("/api/fill/clear", {});
      await waitFill((x) => x.source === null);
      const drc = await json("/api/drc?refill_zones=1");
      t(drc.zones_refilled_by_kicad === true, "the DRC says kicad-cli refilled the zones first");
      let f = await waitFill((x) => x.zones.every((z) => z.filled), 20000);
      t(f.zones.every((z) => z.filled), "and the fills are kept like B's");
      t(Array.isArray(drc.violations), `the DRC ran on the board with that fill (${(drc.violations ?? []).length} violations, ${Object.keys(drc.counts ?? {}).length} kinds)`);
      await postJson("/api/fill/clear", {});
      await waitFill((x) => x.source === null);
      const plot = await postJson("/api/fab/gerbers", { layers: ["F.Cu", "B.Cu"], check_zones: true });
      t(plot.ok === true && (plot.files ?? []).length >= 2, `the Plot dialog's Check zone fills before plotting: plotted (${(plot.files ?? []).length} files, ${plot.message ?? ""})`);
      f = await waitFill((x) => x.zones.every((z) => z.filled), 20000);
      t(f.zones.every((z) => z.filled), "the zones were filled before the copper was plotted");
      const again = await postJson("/api/fab/gerbers", { layers: ["F.Cu"], check_zones: true });
      t(again.ok === true, "a second plot, with every zone current, runs without filling again");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    await postJson("/api/fill/clear", {});
    await sleep(800);
    const endZones = JSON.stringify((await zonesOf()).map((z) => [z.id, z.priority, z.min_thickness, z.outline]));
    results.push({ name: "end: the zones are as they were and none is filled", ok: endZones === startState && (await fill()).source === null, notes: [`${endZones === startState ? "ok" : "FAIL"}: every edit was undone`, `${(await fill()).source === null ? "ok" : "FAIL"}: nothing is filled`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
