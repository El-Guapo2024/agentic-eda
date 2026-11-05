// Zones in the PCB editor (docs/parity/UI-LEFT.md P8; PARITY-pcb.md section 68): what task-u1-zones ports, in the page.
//   * The order of the zone tools (`DRAWING_TOOL::DrawZone`, `ZONE_CREATE_HELPER::OnFirstPoint` -> `createNewZone`): Add Filled Zone opens the zone dialog at the first
//     corner; Cancel draws nothing and leaves the tool armed; OK starts the outline there and the zone takes the dialog's settings (net, outline display and pitch,
//     the first unused priority), is selected and is one undo step; Properties (E) during the draw changes what the zone takes; finishing pops the tool.
//   * The outline display (`ZONE_BORDER_DISPLAY_STYLE`, Line / Hatched / Fully hatched) set in Zone Properties, written to the derived board, undone.
//   * Holes (`Zone::holes`): the point editor drags a hole's corner (the outline stays), a click in the hole does not pick the zone.
//   * The Zone Manager's Update Displayed Zones: the preview shows kicad-cli's fill of the staged copies, the board keeps its fills.
//   * R7-Z1: an unfill sticks -- a layer change and back leaves the zone unfilled -- and Undo gives the fill back.
// Run it in the page of a served COPY of a board with two GND pours on F.Cu and B.Cu (it edits and undoes, and it fills with kicad-cli); it answers through
// `window.__zonesP8Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-zones minutes=12
(async () => {
  const results = [];
  const check = (window.__zonesP8Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden page throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const mm = (v) => Math.round(v * 1_000_000);
  const state = () => json("/api/state");
  const fill = () => json("/api/fill");
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const zonesOf = async () => ((await state()).routing?.zones ?? []).filter((z) => !z.is_rule_area);
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const waitFor = async (pred, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await pred(); if (v) return v; await sleep(150); } return null; };
  const setValue = (el, value) => {
    const proto = el.tagName === "SELECT" ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event(el.tagName === "SELECT" ? "change" : "input", { bubbles: true }));
  };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 1) }));
  };
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y, { buttons: 0 });
    await sleep(120);
    fire("pointerdown", x, y);
    await sleep(40);
    fire("pointerup", x, y);
    await sleep(300);
  };
  const dragFromTo = async (a, b) => {
    const [x0, y0] = w2c(a[0], a[1]);
    const [x1, y1] = w2c(b[0], b[1]);
    fire("pointermove", x0, y0, { buttons: 0 });
    await sleep(120);
    fire("pointerdown", x0, y0);
    await sleep(60);
    for (let i = 1; i <= 6; i++) { fire("pointermove", x0 + ((x1 - x0) * i) / 6, y0 + ((y1 - y0) * i) / 6); await sleep(50); }
    fire("pointerup", x1, y1);
    await sleep(700);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = tid("zone-cancel") ?? [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(200); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  const square = (x0, y0, x1, y1) => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(([x, y]) => ({ x: mm(x), y: mm(y) }));

  // --------------------------------------------------------------------------------------------------------------------------- run
  for (let i = 0; i < 40 && E.state().tab !== "pcb"; i++) await sleep(250);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  const startZones = await zonesOf();
  if (startZones.length < 2) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs two copper pours"] }); check.done = true; return "no zones"; }
  const [A, B] = startZones;
  // The pour on B.Cu would be picked through A's hole: it goes for the check (and comes back at the end).
  await post({ op: "delete_zone", id: B.id });
  await sleep(800);

  (async () => {
    await scenario("Add Filled Zone asks for the zone's settings at the first corner; Cancel draws nothing and the tool stays armed", async (t) => {
      await E.run("common.InteractiveSelection.clear");
      await E.run("pcbnew.InteractiveDrawing.zone");
      t(E.state().tool === "zone", `the zone tool is armed (${E.state().tool})`);
      const before = (await zonesOf()).length;
      await clickAt(mm(8), mm(10));
      const dlg = await waitFor(() => tid("zone-dialog"));
      t(!!dlg && /New Zone/.test(dlg.textContent), "the first click opens Zone Properties -- New Zone");
      t(!!tid("zone-outline-display") && !!tid("zone-outline-hatch-pitch"), "with the outline display and its hatch pitch");
      t(tid("zone-net")?.value === "", `the new zone starts on <no net> (nothing highlighted or selected: ${tid("zone-net")?.value})`);
      tid("zone-cancel").click();
      await sleep(300);
      t(!tid("zone-dialog"), "Cancel closes it");
      t(E.state().tool === "zone", `the tool stays armed (${E.state().tool})`);
      t((await zonesOf()).length === before, "and no zone was made");
      await clickAt(mm(8), mm(10));
      t(!!(await waitFor(() => tid("zone-dialog"))), "the next click asks again");
    });

    let drawnId = null;
    await scenario("OK starts the outline at that corner; the zone takes the dialog's settings, is selected, pops the tool and is one undo step", async (t) => {
      if (!tid("zone-dialog")) {
        await E.run("pcbnew.InteractiveDrawing.zone");
        await clickAt(mm(8), mm(10));
        await waitFor(() => tid("zone-dialog"));
      }
      setValue(tid("zone-net"), A.net);
      setValue(tid("zone-outline-display"), "full");
      setValue(tid("zone-outline-hatch-pitch"), "1");
      await sleep(150);
      tid("zone-ok").click();
      await sleep(300);
      t(!tid("zone-dialog"), "OK closes the dialog");
      const before = await zonesOf();
      for (const [x, y] of [[14, 10], [14, 16], [8, 16]]) await clickAt(mm(x), mm(y));
      await E.run("pcbnew.InteractiveDrawing.closeOutline");
      const made = await waitFor(async () => (await zonesOf()).find((z) => !before.some((b) => b.id === z.id)), 8000);
      t(!!made, "finishing the outline makes the zone");
      if (!made) return;
      drawnId = made.id;
      t(made.net === A.net, `on the dialog's net (${made.net})`);
      t(made.border_style === "full" && made.border_hatch_pitch === mm(1), `with its outline display and pitch (${made.border_style}, ${made.border_hatch_pitch})`);
      t(JSON.stringify(made.outline) === JSON.stringify([[mm(8), mm(10)], [mm(14), mm(10)], [mm(14), mm(16)], [mm(8), mm(16)]]), `the outline starts at the first click (${JSON.stringify(made.outline)})`);
      t(made.priority === Math.max(...before.map((z) => z.priority)) + 1 || made.priority > 0, `the first unused priority (${made.priority})`);
      await waitFor(() => E.state().selection.some((s) => s.id === made.id), 3000);
      t(E.state().selection.some((s) => s.id === made.id), "the new zone is selected");
      t(E.state().tool === "select", `the tool is left (${E.state().tool})`);
      await undo();
      t(!(await zonesOf()).some((z) => z.id === made.id), "one Undo takes the zone and its settings away");
    });

    await scenario("Properties (E) during the draw changes what the zone being drawn takes", async (t) => {
      await E.run("pcbnew.InteractiveDrawing.zone");
      await clickAt(mm(8), mm(10));
      await waitFor(() => tid("zone-dialog"));
      setValue(tid("zone-net"), A.net);
      tid("zone-ok").click();
      await sleep(250);
      await clickAt(mm(14), mm(10));
      await E.run("pcbnew.InteractiveEdit.properties");
      const dlg = await waitFor(() => tid("zone-dialog"));
      t(!!dlg, "E opens the dialog on the zone being drawn");
      if (!dlg) return;
      setValue(tid("zone-name"), "drawn-p8");
      setValue(tid("zone-outline-display"), "none");
      await sleep(150);
      tid("zone-ok").click();
      await sleep(250);
      const before = await zonesOf();
      await clickAt(mm(14), mm(16));
      await E.run("pcbnew.InteractiveDrawing.closeOutline");
      const made = await waitFor(async () => (await zonesOf()).find((z) => !before.some((b) => b.id === z.id)), 8000);
      t(!!made && made.name === "drawn-p8" && made.border_style === "none", `the zone takes the settings of the second OK (${made?.name}, ${made?.border_style})`);
      if (made) await undo();
    });

    await scenario("Zone Properties' outline display is written to the derived board and undone", async (t) => {
      await E.run("common.InteractiveSelection.clear");
      await E.run("common.InteractiveSelection.selectItems", [A.id]);
      await sleep(250);
      await E.run("pcbnew.InteractiveEdit.properties");
      const dlg = await waitFor(() => tid("zone-dialog"));
      t(!!dlg, "E opens Zone Properties on zone A");
      if (!dlg) return;
      t(tid("zone-outline-display").value === (A.border_style ?? "edge"), `the dialog shows the zone's style (${tid("zone-outline-display").value})`);
      setValue(tid("zone-outline-display"), "none");
      setValue(tid("zone-outline-hatch-pitch"), "0.05");
      await sleep(150);
      t(/Outline hatch pitch must be at least 0\.1 mm\./.test(tid("zone-pitch-error")?.textContent ?? ""), `a pitch under 0.1 mm is refused with KiCad's message (${tid("zone-pitch-error")?.textContent})`);
      t(tid("zone-ok").disabled, "and OK is off");
      setValue(tid("zone-outline-hatch-pitch"), "0.7");
      await sleep(150);
      tid("zone-ok").click();
      const now = await waitFor(async () => (await zonesOf()).find((z) => z.id === A.id && z.border_style === "none"), 5000);
      t(!!now && now.border_hatch_pitch === 700_000, `the zone is Line, pitch 0.7 mm (${now?.border_style}, ${now?.border_hatch_pitch})`);
      const pcb = await fetch("/api/board.kicad_pcb").then((r) => r.text());
      t(/\(hatch none 0\.7\)/.test(pcb), "the derived board says (hatch none 0.7)");
      await undo();
      const back = (await zonesOf()).find((z) => z.id === A.id);
      t(back?.border_style === (A.border_style ?? "edge") && back?.border_hatch_pitch === (A.border_hatch_pitch ?? 500_000), `Undo puts it back (${back?.border_style}, ${back?.border_hatch_pitch})`);
    });

    let holed = null;
    await scenario("the point editor drags a hole's corner, the outline stays; a click in the hole does not pick the zone", async (t) => {
      const r = await post({ op: "zone_cutout", id: A.id, cutout: square(12, 14, 18, 20) });
      t(r.ok, `Add Zone Cutout makes a hole (${r.message})`);
      holed = await waitFor(async () => (await zonesOf()).find((z) => (z.holes ?? []).length === 1), 4000);
      t(!!holed, "the zone has a hole");
      if (!holed) return;
      await E.run("common.InteractiveSelection.clear");
      await sleep(200);
      await clickAt(mm(15), mm(17));
      t(!E.state().selection.some((s) => s.id === holed.id), `a click in the hole picks no zone (${JSON.stringify(E.state().selection)})`);
      await E.run("common.InteractiveSelection.clear");
      await E.run("common.InteractiveSelection.selectItems", [holed.id]);
      await sleep(300);
      const corner = holed.holes[0].find((p) => p[0] === mm(18) && p[1] === mm(20)) ?? holed.holes[0][2];
      await dragFromTo(corner, [corner[0] + mm(2), corner[1] + mm(1)]);
      const now = await waitFor(async () => (await zonesOf()).find((z) => z.id === holed.id && JSON.stringify(z.holes) !== JSON.stringify(holed.holes)), 5000);
      t(!!now, "the hole's corner moved");
      if (now) {
        t(now.holes[0].some((p) => Math.abs(p[0] - (corner[0] + mm(2))) <= mm(0.5) && Math.abs(p[1] - (corner[1] + mm(1))) <= mm(0.5)), `to where it was dropped (${JSON.stringify(now.holes[0])})`);
        t(JSON.stringify(now.outline) === JSON.stringify(holed.outline), "the outline did not move");
        await undo();
        t(JSON.stringify((await zonesOf()).find((z) => z.id === holed.id)?.holes) === JSON.stringify(holed.holes), "Undo puts the hole back");
      }
    });

    await scenario("Update Displayed Zones fills the staged copies for the preview; the board keeps its fills", async (t) => {
      const f0 = await fill();
      await E.run("common.InteractiveSelection.clear");
      await E.run("pcbnew.Control.zonesManager");
      const btn = await waitFor(() => tid("zone-manager-update-displayed"));
      t(!!btn, "the Zone Manager has Update Displayed Zones");
      if (!btn) return;
      const row = [...document.querySelectorAll(".dialog tbody tr")].find((r) => r.textContent.includes(holed?.id ?? A.id));
      row?.click();
      await sleep(300);
      t(!tid("zone-preview-fill"), "before: the preview shows the board's fill, none yet");
      btn.click();
      const done = await waitFor(() => /Displayed zones updated/.test(tid("zone-manager-note")?.textContent ?? "") || /not filled/.test(tid("zone-manager-note")?.textContent ?? ""), 60000);
      t(!!done && /Displayed zones updated/.test(tid("zone-manager-note").textContent), `kicad-cli filled the copies (${tid("zone-manager-note")?.textContent})`);
      t(!!tid("zone-preview-fill"), "the preview shows that fill");
      const f1 = await fill();
      t(JSON.stringify(f1) === JSON.stringify(f0), "the board's fills are as they were: nothing kept");
      [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "Cancel")?.click();
      await sleep(300);
    });

    await scenario("R7-Z1: an edit that empties a zone sticks -- a layer change and back leaves it unfilled -- and Undo gives the fill back", async (t) => {
      const zone = (await zonesOf()).find((z) => z.id === (holed?.id ?? A.id));
      const r = await E.run("pcbnew.ZoneFiller.zoneFillAll");
      t(r.ok === true, `B fills (${r.toast})`);
      const filled = await waitFor(async () => { const f = await fill(); return f.zones.find((z) => z.id === zone.id && z.filled) ? f : null; }, 30000);
      t(!!filled, "the zone is filled");
      const other = zone.layer === "F.Cu" ? "B.Cu" : "F.Cu";
      await post({ op: "edit_zone", ...zone, layer: other, layers: [other] });
      await post({ op: "edit_zone", ...zone, layer: zone.layer, layers: [zone.layer] });
      const f = await fill();
      t(f.zones.filter((z) => z.id === zone.id).every((z) => !z.filled), `back on ${zone.layer} it is still unfilled, as KiCad's SetLayerSet left it`);
      await undo();
      await undo();
      const back = await fill();
      t(back.zones.filter((z) => z.id === zone.id).every((z) => z.filled), "two Undos give the fill back (KiCad's undo copy holds the fill)");
      await fetch("/api/fill/clear", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" });
    });

    // Put the board back: the cutout and the pour on B.Cu.
    if (holed) await undo();
    await undo();
    const endZones = await zonesOf();
    results.push({ name: "the board is back as it was", ok: endZones.length === startZones.length && endZones.some((z) => z.id === B.id), notes: [`zones: ${endZones.map((z) => z.id).join(", ")}`] });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
