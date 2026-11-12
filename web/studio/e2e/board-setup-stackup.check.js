// Board Setup > Physical Stackup (UI-LEFT B3, pcbnew/board_stackup_manager/panel_board_stackup.cpp and PANEL_SETUP_LAYERS::TransferDataFromWindow): four copper layers
// applied (the board keeping its thickness, prepreg outside and a core inside); Add Dielectric Layer (KiCad's list, "Select layer to add:") gives Dielectric 1 a second sublayer, named "Dielectric 1  (1/2)" / "(2/2)" with the Core/PrePreg
// choice on the first only, whose thickness and lock are typed and applied with "Impedance controlled" (the board's stackup has the sublayer, its lock and
// `dielectric_constraints`, and the board thickness counts it); Remove Dielectric Layer (KiCad's list, "Layer 'Dielectric 1' sublayer 2/2") takes it away again; a
// negative epsilon R stops Apply with the "Errors" box in KiCad's words; Adjust Dielectric Thickness asks for the thickness ("Adjust Unlocked Dielectric Layers") and
// fits the dielectrics to it; two copper layers with a track on In1.Cu ask KiCad's question ("Warning", "Items have been found on removed layers ...", "Show Details"
// listing "Track [net] on In1.Cu, length ..."; a click on it selects the track, `SetSelectionCallback`), Cancel changes nothing, OK deletes the track with the layers and
// clears the board's undo history (`ClearUndoRedoList`), so Undo brings nothing back. Before that, with nothing sent: the "..." beside a material opens KiCad's material
// dialog ("Dielectric Material Characteristics", "Common materials:") whose PTFE sets the material, epsilon R, loss tangent and colour; a mask's epsilon R shows KiCad's
// 3.3; the colour boxes have KiCad's lists and the row's icon follows the mask's colour; Export to Clipboard writes `BuildStackupReport`'s text. At the end the board
// has its two layers, its tracks and its thickness again (its stackup is now stored, and the history before the deletion is gone, as in KiCad).
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir> --port N --ui web/studio/dist`) with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/
// web/studio/e2e/board-setup-stackup.check.js`, or pasted into the Browser pane's javascript tool, on a COPY of a board (2 copper layers to start with).
// It answers through `window.__boardSetupStackupCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__boardSetupStackupCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(120); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  const run = (id, args) => __eda.run(id, args);
  const dialog = () => document.querySelector(".bs-dialog");
  const page = () => document.querySelector(`section.bs-page[aria-label="Physical Stackup"]`);
  const buttonIn = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const aria = (root, label) => root?.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    if (!el) throw new Error(`no field to set to ${v}`);
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const openPage = async () => {
    if (!dialog()) { await run("pcbnew.EditorControl.boardSetup"); await sleep(500); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Physical Stackup"))?.click();
    await sleep(350);
    const p = page();
    if (!p || p.hidden) throw new Error(`Board Setup has no "Physical Stackup" page on screen`);
    return p;
  };
  const closeDialog = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      const b = [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((x) => ["Close", "Discard and close", "OK", "Cancel"].includes(x.textContent.trim()));
      b?.click();
      await sleep(300);
    }
  };
  /** The dialog opened on top of Board Setup whose header is `title`. */
  const popup = (title) => [...document.querySelectorAll(".dialog")].find((d) => !d.closest(".bs-dialog") && d.querySelector(".dialog-header")?.textContent.trim() === title);
  const popupButton = (d, text) => [...(d?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.textContent.trim() === text);
  const rowLabels = (p) => [...p.querySelectorAll("table.bs-grid tbody tr")].map((r) => r.querySelector("td")?.textContent ?? "");
  const applyPage = async (p, changed) => {
    const r0 = await fetch("/api/version").then((r) => r.text());
    buttonIn(p, "Apply").click();
    const st = await until(async () => { const s = await state(); return changed(s) ? s : null; }, 15000);
    await until(async () => (await fetch("/api/version").then((r) => r.text())) !== r0, 3000);
    return st;
  };
  const undo = async () => { await closeDialog(); await run("common.Interactive.undo"); await sleep(900); return state(); };
  const J = (v) => JSON.stringify(v ?? null);
  const d1 = (s) => s?.board_rules?.stackup?.layers?.find((l) => l.name === "dielectric 1");

  let steps = 0; // undo steps made, to take back at the end
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  if (__eda.state().units !== "mm") { await run("common.Control.metricUnits"); await sleep(300); }
  const s0 = await state();
  const net = (s0.parts ?? []).flatMap((p) => p.pads ?? []).find((p) => p.net)?.net ?? (s0.nets ?? [])[0]?.name ?? "GND";

  (async () => {
    await scenario("four copper layers are applied, and a track is put on In1.Cu", async (t) => {
      t((s0.layers ?? []).length === 2, `the board starts with two copper layers (${J(s0.layers)})`);
      const p = await openPage();
      const thick0 = aria(p, "Board thickness from stackup")?.textContent;
      setValue(aria(p, "Copper layers"), 4);
      await sleep(300);
      t(aria(p, "Board thickness from stackup")?.textContent === thick0, `the board keeps its thickness, the dielectrics sharing it (onCopperLayersSelCount: ${thick0} -> ${aria(p, "Board thickness from stackup")?.textContent})`);
      t(aria(p, "Dielectric 1 type")?.value === "PrePreg" && aria(p, "Dielectric 2 type")?.value === "Core", "prepreg outside, the core inside (setDefaultLayerWidths)");
      const st = await applyPage(p, (s) => (s.layers ?? []).length === 4);
      if (t(J(st?.layers) === J(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]), `the board's copper layers (${J(st?.layers)})`)) steps++;
      await closeDialog();
      const r = await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "add_track", net, layer: "In1.Cu", width: 200_000, pts: [{ x: 5_000_000, y: 5_000_000 }, { x: 10_000_000, y: 5_000_000 }] }, strict: false }) }).then((x) => x.json());
      if (t(r.ok === true, `a track on In1.Cu (${r.message ?? ""})`)) steps++;
      await sleep(600);
    });

    await scenario("Add Dielectric Layer gives Dielectric 1 a sublayer; its thickness, its lock and Impedance controlled are applied", async (t) => {
      const before = await state();
      const p = await openPage();
      t(J(rowLabels(p).filter((l) => l.startsWith("Dielectric"))) === J(["Dielectric 1", "Dielectric 2", "Dielectric 3"]), `one row per dielectric (${J(rowLabels(p))})`);
      t(buttonIn(p, "Remove Dielectric Layer...")?.disabled === true, "Remove Dielectric Layer... is off while no dielectric has sublayers (onRemoveDielUI)");
      buttonIn(p, "Add Dielectric Layer...").click();
      await sleep(300);
      const dlg = popup("Add Dielectric Layer");
      t(!!dlg && /Select layer to add:/.test(dlg.textContent), "KiCad's dialog: \"Add Dielectric Layer\", \"Select layer to add:\"");
      const list = dlg?.querySelector("select");
      t(J([...(list?.options ?? [])].map((o) => o.textContent)) === J(["Dielectric 1", "Dielectric 2", "Dielectric 3"]), `the list (${J([...(list?.options ?? [])].map((o) => o.textContent))})`);
      setValue(list, "Dielectric 1");
      popupButton(dlg, "OK").click();
      await sleep(300);
      const labels = rowLabels(p);
      t(labels.includes("Dielectric 1  (1/2)") && labels.includes("Dielectric 1  (2/2)"), `the rows of the two sublayers (${J(labels.filter((l) => l.startsWith("Dielectric")))})`);
      t(!!aria(p, "Dielectric 1  (1/2) type") && !aria(p, "Dielectric 1  (2/2) type"), "the Core/PrePreg choice is on the first sublayer only");
      setValue(aria(p, "Dielectric 1  (2/2) thickness"), 0.1);
      aria(p, "Dielectric 1  (2/2) thickness locked").click();
      const imp = [...p.querySelectorAll("label.bs-check")].find((l) => l.textContent.includes("Impedance controlled"))?.querySelector("input");
      t(!!imp && imp.checked === false, "Impedance controlled starts off");
      imp.click();
      await sleep(200);
      const st = await applyPage(p, (s) => (d1(s)?.sublayers ?? []).length === 1);
      const sub = d1(st)?.sublayers?.[0];
      if (t(Math.abs((sub?.thickness_mm ?? 0) - 0.1) < 1e-9 && sub?.thickness_locked === true, `the stored sublayer (${J(sub)})`)) steps++;
      t(st?.board_rules?.stackup?.dielectric_constraints === true, "(dielectric_constraints yes)");
      t(st?.board_rules?.board_thickness_nm === (before.board_rules?.board_thickness_nm ?? 0) + 100_000, `the board thickness counts the sublayer (${before.board_rules?.board_thickness_nm} -> ${st?.board_rules?.board_thickness_nm})`);
      await closeDialog();
    });

    await scenario("Remove Dielectric Layer lists the sublayers and takes the second away", async (t) => {
      const p = await openPage();
      const b = buttonIn(p, "Remove Dielectric Layer...");
      t(b?.disabled === false, "Remove Dielectric Layer... is on");
      b.click();
      await sleep(300);
      const dlg = popup("Remove Dielectric Layer");
      const opts = [...(dlg?.querySelector("select")?.options ?? [])].map((o) => o.textContent);
      t(/Select layer to remove:/.test(dlg?.textContent ?? "") && J(opts) === J(["Layer 'Dielectric 1' sublayer 1/2", "Layer 'Dielectric 1' sublayer 2/2"]), `KiCad's list (${J(opts)})`);
      setValue(dlg.querySelector("select"), "Layer 'Dielectric 1' sublayer 2/2");
      popupButton(dlg, "OK").click();
      await sleep(300);
      t(rowLabels(p).includes("Dielectric 1") && !rowLabels(p).some((l) => l.includes("(2/2)")), `one row again (${J(rowLabels(p).filter((l) => l.startsWith("Dielectric")))})`);
      const st = await applyPage(p, (s) => (d1(s)?.sublayers ?? []).length === 0);
      if (t(!!st && !d1(st)?.sublayers, "the board's dielectric has one sublayer again")) steps++;
      await closeDialog();
    });

    await scenario("a negative epsilon R stops Apply with KiCad's Errors box; Adjust Dielectric Thickness fits the dielectrics", async (t) => {
      const v0 = await fetch("/api/version").then((r) => r.text());
      const p = await openPage();
      setValue(aria(p, "Dielectric 2 epsilon r"), -1);
      await sleep(200);
      buttonIn(p, "Apply").click();
      await sleep(400);
      const err = popup("Errors");
      t(!!err && err.textContent.includes("Incorrect value for Epsilon R (Epsilon R must be positive or null if not used)"), `the Errors box (${err?.textContent ?? "none"})`);
      popupButton(err, "OK")?.click();
      await sleep(200);
      buttonIn(p, "Revert").click();
      await sleep(300);
      buttonIn(p, "Adjust Dielectric Thickness").click();
      await sleep(300);
      const adj = popup("Adjust Unlocked Dielectric Layers");
      const field = adj?.querySelector("input");
      t(!!field && /^Enter expected board thickness \(min value 0\.16 mm\):$/.test(field.getAttribute("aria-label") ?? ""), `the prompt (${field?.getAttribute("aria-label")})`);
      setValue(field, "2");
      popupButton(adj, "OK").click();
      await sleep(300);
      t(aria(p, "Board thickness from stackup")?.textContent === "2", `Board thickness from stackup: ${aria(p, "Board thickness from stackup")?.textContent}`);
      t(aria(p, "Dielectric 1 type")?.value === "PrePreg" && aria(p, "Dielectric 2 type")?.value === "Core", "prepreg outside, the core inside (setDefaultLayerWidths)");
      buttonIn(p, "Revert").click();
      await sleep(200);
      await closeDialog();
      t((await fetch("/api/version").then((r) => r.text())) === v0, "nothing was sent");
    });

    await scenario("the material list, the epsilon R KiCad shows, the colour boxes and Export to Clipboard (nothing sent)", async (t) => {
      const v0 = await fetch("/api/version").then((r) => r.text());
      const p = await openPage();
      t(aria(p, "F.Mask epsilon r")?.value === "3.3" && aria(p, "F.Mask loss tangent")?.value === "0", `a mask's epsilon R and loss tangent are numbers, KiCad's defaults (${aria(p, "F.Mask epsilon r")?.value}, ${aria(p, "F.Mask loss tangent")?.value})`);
      aria(p, "Dielectric 1 material list")?.click();
      await sleep(300);
      const mat = popup("Dielectric Material Characteristics");
      t(!!mat && /Common materials:/.test(mat.textContent) && /Name:/.test(mat.textContent) && /Epsilon R:/.test(mat.textContent) && /Loss Tan:/.test(mat.textContent), "KiCad's material dialog");
      t(aria(mat, "Material name")?.value === "" && aria(mat, "Material epsilon r")?.value === "1" && aria(mat, "Material loss tangent")?.value === "0", "its boxes start empty, 1 and 0 (TransferDataToWindow)");
      const names = [...(mat?.querySelectorAll("tr[data-material]") ?? [])].map((r) => r.getAttribute("data-material"));
      t(J(names.slice(0, 10)) === J(["Not specified", "FR4", "FR408-HR", "Polyimide", "Kapton", "Polyolefin", "Al", "PTFE", "Teflon", "Ceramic"]), `the dielectric list (${J(names)})`);
      t(J([...(mat?.querySelectorAll("th") ?? [])].map((h) => h.textContent)) === J(["Material", "Epsilon R", "Loss Tan"]), "its columns");
      mat?.querySelector('tr[data-material="PTFE"]')?.click();
      await sleep(150);
      t(aria(mat, "Material name")?.value === "PTFE" && aria(mat, "Material epsilon r")?.value === "2.1" && aria(mat, "Material loss tangent")?.value === "0.0002", "picking PTFE fills the boxes");
      popupButton(mat, "OK")?.click();
      await sleep(300);
      t(aria(p, "Dielectric 1 material")?.value === "PTFE" && aria(p, "Dielectric 1 epsilon r")?.value === "2.1" && aria(p, "Dielectric 1 loss tangent")?.value === "0.0002", "the row takes the material, epsilon R and loss tangent");
      const dc = aria(p, "Dielectric 1 color");
      t(dc?.selectedOptions?.[0]?.textContent === "PTFE natural", `and its material's colour (${dc?.selectedOptions?.[0]?.textContent})`);
      t(J([...(dc?.options ?? [])].map((o) => o.textContent)) === J(["Not specified", "FR4 natural", "PTFE natural", "Polyimide", "Phenolic natural", "Aluminum", "User defined"]), "a dielectric's colours");
      const mc = aria(p, "F.Mask color");
      t(J([...(mc?.options ?? [])].map((o) => o.textContent)) === J(["Not specified", "Green", "Red", "Blue", "Purple", "Black", "White", "Yellow", "User defined"]), "a mask's colours");
      setValue(mc, [...mc.options].find((o) => o.textContent === "Red").value);
      await sleep(200);
      const icon = [...p.querySelectorAll("table.bs-grid tbody tr")].find((r) => r.querySelector("td")?.textContent === "F.Mask")?.querySelector("[data-icon-color]");
      t(icon?.getAttribute("data-icon-color") === "rgb(128, 0, 0)", `the row's icon is the mask's colour (${icon?.getAttribute("data-icon-color")})`);
      let copied = null;
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text) => { copied = text; } } });
      buttonIn(p, "Export to Clipboard")?.click();
      await sleep(300);
      delete navigator.clipboard;
      t(typeof copied === "string" && /^layer "F\.Silkscreen" type "Top Silk Screen"/.test(copied) && copied.includes('layer "Dielectric 1" type "prepreg"\n  sublayer "1/1" Color "PTFE natural"') && copied.includes('Material "PTFE" EpsilonR 2.1 LossTg 0.0002') && copied.includes('layer "F.Mask" type "Top Solder Mask" Color "Red"') && /\nFinish "[^"]*"/.test(copied), `BuildStackupReport's text (${J((copied ?? "").slice(0, 200))})`);
      buttonIn(p, "Revert").click();
      await sleep(200);
      await closeDialog();
      t((await fetch("/api/version").then((r) => r.text())) === v0, "nothing was sent");
    });

    await scenario("two copper layers with a track on In1.Cu: KiCad's question, Show Details selects; Cancel keeps it, OK deletes it with the layers and Undo cannot bring it back", async (t) => {
      const p = await openPage();
      setValue(aria(p, "Copper layers"), 2);
      await sleep(300);
      buttonIn(p, "Apply").click();
      await sleep(400);
      let warn = popup("Warning");
      t(!!warn && warn.textContent.includes("Items have been found on removed layers. This operation will delete all items from removed layers and cannot be undone."), `KiCad's question (${warn?.textContent?.slice(0, 160) ?? "none"})`);
      const summary = warn?.querySelector("summary");
      t(summary?.textContent.trim() === "Show Details", "a Show Details pane");
      summary?.click();
      await sleep(200);
      const items = [...(warn?.querySelectorAll("ul[aria-label='Items on removed layers'] li") ?? [])].map((li) => li.textContent);
      t(items.includes(`Track [${net}] on In1.Cu, length 5.0000 mm`), `the track is listed as GetItemDescription says it (${J(items)})`);
      const inner = ((await state()).routing?.tracks ?? []).find((x) => x.layer === "In1.Cu");
      [...(warn?.querySelectorAll("ul[aria-label='Items on removed layers'] li") ?? [])].find((li) => li.textContent === `Track [${net}] on In1.Cu, length 5.0000 mm`)?.click();
      await sleep(300);
      const sel = __eda.state().selection.map((x) => x.id);
      t(!!inner && J(sel) === J([inner.id]), `a click on it selects the track on the board (SetSelectionCallback: ${J(sel)})`);
      popupButton(warn, "Cancel").click();
      await sleep(500);
      let s = await state();
      t((s.layers ?? []).length === 4 && (s.routing?.tracks ?? []).some((x) => x.layer === "In1.Cu"), "Cancel: four layers and the track");
      buttonIn(p, "Apply").click();
      await sleep(400);
      warn = popup("Warning");
      popupButton(warn, "OK").click();
      s = await until(async () => { const x = await state(); return (x.layers ?? []).length === 2 ? x : null; }, 15000);
      t(!!s && !(s.routing?.tracks ?? []).some((x) => x.layer === "In1.Cu"), `OK: two layers and the track is gone (${J(s?.layers)})`);
      // `ClearUndoRedoList`: every step of the board before it is gone with the deleted track
      steps = 0;
      const back = await undo();
      t((back.layers ?? []).length === 2 && !(back.routing?.tracks ?? []).some((x) => x.layer === "In1.Cu"), `Undo brings nothing back: "cannot be undone" (${J(back.layers)})`);
    });

    await scenario("the board has its layers, tracks and thickness again, and the page logged no error", async (t) => {
      await closeDialog();
      const shape = (s) => J({ layers: s.layers, thickness: s.board_rules?.board_thickness_nm, tracks: (s.routing?.tracks ?? []).map((x) => [x.layer, x.net, x.pts]) });
      const now = shape(await state());
      t(now === shape(s0), `the board's layers, tracks and thickness are as they were${now === shape(s0) ? "" : `: ${shape(s0).slice(0, 300)} -> ${now.slice(0, 300)}`}`);
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
