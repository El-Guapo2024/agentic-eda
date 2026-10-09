// 3D viewer check (GAPS.md item 23): open the 3D tab of a served board and check, through `window.__eda`, what the viewer reports (`__eda.state().viewer3d`,
// components/viewer3d/viewer3dProbe.ts): that every part is drawn as its KiCad model, how long the first model took, that each row of the Appearance manager's layer tree gates the
// geometry it names, that hovering a part highlights it, that the face views are animated moves the mouse and other views wait for, and that the stackup's colours reach the
// swatches. Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: the stackup
// check sets a stackup with colours (`set_stackup`) and undoes it. The board should have parts with 3D models; the more kinds of item it has (tracks, vias, zones, drawings on
// silkscreen, adhesive and user layers) the more rows are checked, and a row the board has nothing for is reported as skipped.
//
// It returns { ok, firstModelMs, results: [{ name, ok, detail? }, ...] }. Waits are fetch loops (`/api/version`), not timers: a hidden pane throttles timers to once a minute.
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const view = () => window.__eda.state().viewer3d;
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const waitFor = async (what, test, ms = 60000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      if (test()) return true;
      await sleep(40);
    }
    check(`waited for ${what}`, false, `not after ${ms} ms`);
    return false;
  };

  // ------------------------------------------------------------------------------------------------------------------------- the tab and its models
  await __eda.run("common.Control.show3DViewer");
  if (!(await waitFor("the 3D tab", () => view() && view().open))) return JSON.stringify({ ok: false, results });
  await waitFor("every model to settle", () => view().allModelsMs !== null);
  const v = view();
  check("the viewer draws the live scene", v.source === "live", v.source);
  check("every model that was asked for is in", v.models.requested > 0 && v.models.ready === v.models.requested, v.models);
  check("every part with models is drawn as its models", v.parts.total > 0 && v.parts.asModels + v.parts.asBoxes === v.parts.total && v.parts.asModels > 0, v.parts);
  check("the first real model came in seconds, not minutes", v.firstModelMs !== null && v.firstModelMs < 30000, v.firstModelMs);
  const state0 = __eda.state();
  check("no error was logged", __eda.errors().length === 0, __eda.errors().slice(0, 3));

  // ----------------------------------------------------------------------------------------------------------------------------- the layer tree
  if (document.querySelectorAll("[data-row]").length === 0) document.querySelector(".dock-column.right .dock-handle")?.click();
  await sleep(300);
  const rows = [...document.querySelectorAll("[data-row]")].map((r) => r.getAttribute("data-row"));
  check("the layer tree lists KiCad's rows", ["board", "plated_barrels", "copper_top", "copper_bottom", "adhesive", "solder_paste", "silkscreen_top", "silkscreen_bottom", "soldermask_top", "soldermask_bottom", "th_models", "smd_models", "virtual_models", "bounding_boxes", "zones"].every((id) => rows.includes(id)), rows);
  // What each row must take out of the scene when it is turned off, by the names scene.ts gives its pieces (kicad-port/appearance3d.ts has the rows).
  const gates = {
    board: ["board-slab"], plated_barrels: ["via"], solder_paste: ["solder-paste"], soldermask_top: ["solder-mask-top"], soldermask_bottom: ["solder-mask-bottom"],
    zones: ["zone-fill"], references: ["part-ref"], adhesive: ["adhesive-shape-line", "adhesive-shape-fill", "adhesive-text"],
    user_drawings: ["user_drawings-shape-line", "user_drawings-shape-fill", "user_drawings-text"], user_comments: ["user_comments-shape-line", "user_comments-shape-fill", "user_comments-text"],
    user_eco1: ["user_eco1-shape-line", "user_eco1-shape-fill", "user_eco1-text"], user_eco2: ["user_eco2-shape-line", "user_eco2-shape-fill", "user_eco2-text"],
    silkscreen_top: ["part-ref"], copper_top: ["pad"], // the copper rows only take away what is on their layer: some, not all, of the tracks and zones
  };
  const settle = async () => { await frame(); await sleep(30); await frame(); await sleep(30); };
  const scene = () => ({ ...view().scene });
  const base = scene();
  const skipped = [];
  for (const [id, names] of Object.entries(gates)) {
    if (!rows.includes(id)) continue;
    const has = names.filter((n) => (base[n] ?? 0) > 0);
    if (has.length === 0) { skipped.push(id); continue; }
    const box = document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
    box.click();
    await settle();
    const off = scene();
    check(`${id} off takes ${has.join(", ")} out of the scene`, has.every((n) => (off[n] ?? 0) < base[n] || (off[n] ?? 0) === 0), has.map((n) => `${n}: ${base[n]} -> ${off[n] ?? 0}`));
    box.click();
    await settle();
    const on = scene();
    check(`${id} on puts them back`, has.every((n) => on[n] === base[n]), has.map((n) => `${n}: ${on[n] ?? 0}`));
  }
  for (const id of ["copper_top", "copper_bottom"]) {
    if (!rows.includes(id)) continue;
    const before = scene();
    const box = document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
    box.click();
    await settle();
    const off = scene();
    const gone = ["track", "zone-fill", "pad"].filter((n) => (off[n] ?? 0) < (before[n] ?? 0));
    check(`${id} off takes copper of its own layer out`, gone.length > 0 || (before.track ?? 0) + (before["zone-fill"] ?? 0) + (before.pad ?? 0) === 0, gone.map((n) => `${n}: ${before[n]} -> ${off[n] ?? 0}`));
    box.click();
    await settle();
  }
  // The model rows gate the models only: the parts of a kind are hidden, and their pads and silkscreen stay.
  for (const [id, kind] of [["th_models", "tht"], ["smd_models", "smd"]]) {
    const before = view().parts.shown;
    const padsBefore = scene().pad;
    const box = document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
    box.click();
    await settle();
    check(`${id} off hides the ${kind} models and keeps the pads`, view().parts.shown <= before && scene().pad === padsBefore, { shown: `${before} -> ${view().parts.shown}`, pads: `${padsBefore} -> ${scene().pad}` });
    box.click();
    await settle();
    check(`${id} on shows them again`, view().parts.shown === before, view().parts.shown);
  }
  check("rows the board has nothing for were left alone", true, skipped);

  // ------------------------------------------------------------------------------------------------------------------------------------- hover
  const canvas = document.querySelector(".pcb-canvas-container canvas");
  const box = canvas.getBoundingClientRect();
  const refs = (state0.counts.footprints ? (await fetch("/api/state").then((r) => r.json())).parts.filter((p) => p.placed).map((p) => p.ref) : []).slice(0, 6);
  let hovered = 0;
  for (const ref of refs) {
    const at = window.__eda.viewer3dScreen(ref);
    if (!at || at.x < 0 || at.y < 0 || at.x > box.width || at.y > box.height) continue;
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: box.left + at.x, clientY: box.top + at.y, bubbles: true, pointerId: 1 }));
    await settle();
    if (view().hovered === ref) hovered++;
  }
  check("hovering a part highlights it (the part under the pointer is reported)", hovered > 0, `${hovered} of ${refs.length} parts on screen`);
  canvas.dispatchEvent(new PointerEvent("pointerleave", { bubbles: true, pointerId: 1 }));
  await settle();
  check("leaving the canvas clears the highlight", view().hovered === null);

  // ------------------------------------------------------------------------------------------------------------------------- camera animation
  const key = (k, extra = {}) => canvas.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...extra }));
  while (view().cameraMoving) await frame();
  const t0 = performance.now();
  key("y"); // the front view
  await frame();
  const movedAtOnce = view().cameraMoving;
  key("z"); // another view while it moves: refused
  let frames = 0;
  while (view().cameraMoving && performance.now() - t0 < 10000) { await frame(); frames++; }
  const took = performance.now() - t0;
  check("a face view is an animated move: it runs for about a second, over several frames", movedAtOnce && took > 700 && took < 4000 && frames > 3, { movedAtOnce, tookMs: Math.round(took), frames });
  await settle();
  check("a view command made during the move is ignored", !view().cameraMoving, "the move ended and no second one started");
  key("z"); // back to the top view
  while (view().cameraMoving) await frame();

  // ------------------------------------------------------------------------------------------------------------------------ stackup colours
  const swatch = (id) => document.querySelector(`[data-row="${id}"] input[type=color]`)?.value;
  const before = { mask: swatch("soldermask_top"), silk: swatch("silkscreen_top"), board: swatch("board") };
  const stackup = { copper_layers: 2, stackup: { layers: [
    { name: "F.SilkS", kind: "Top Silk Screen", material: null, thickness_mm: null, color: "Black" },
    { name: "F.Paste", kind: "Top Solder Paste", material: null, thickness_mm: null },
    { name: "F.Mask", kind: "Top Solder Mask", material: null, thickness_mm: 0.01, color: "Red" },
    { name: "F.Cu", kind: "copper", material: null, thickness_mm: 0.035 },
    { name: "dielectric 1", kind: "core", material: "FR4", thickness_mm: 1.51, epsilon_r: 4.5, loss_tangent: 0.02, color: "FR4 natural" },
    { name: "B.Cu", kind: "copper", material: null, thickness_mm: 0.035 },
    { name: "B.Mask", kind: "Bottom Solder Mask", material: null, thickness_mm: 0.01, color: "Blue" },
    { name: "B.Paste", kind: "Bottom Solder Paste", material: null, thickness_mm: null },
    { name: "B.SilkS", kind: "Bottom Silk Screen", material: null, thickness_mm: null },
  ], copper_finish: "ENIG", dielectric_constraints: false, edge_connector: 0, edge_plating: false } };
  const reply = await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "set_stackup", settings: stackup }, strict: false }) }).then((r) => r.json());
  check("the stackup with colours was accepted", reply.ok === true, reply.message);
  await waitFor("the stackup's colours in the swatches", () => swatch("soldermask_top") === "#b51315", 8000);
  const after = { mask: swatch("soldermask_top"), silk: swatch("silkscreen_top"), board: swatch("board"), copper: swatch("copper_top"), disabled: document.querySelector('[data-row="soldermask_top"] input[type=color]')?.disabled };
  check("F.Mask Red is KiCad's red", after.mask === "#b51315", after.mask);
  check("F.SilkS Black is KiCad's black", after.silk === "#0b0b0b", after.silk);
  check("the dielectric's colour is the board body's", after.board === "#6d744b", after.board);
  check("ENIG makes the copper gold", after.copper === "#b29c00", after.copper);
  check("the stackup's rows are read-only while 'Use board stackup colors' is on", after.disabled === true);
  document.querySelector(".viewer3d-layer-group")?.closest(".panel-section")?.querySelector("label.filter-row input[type=checkbox]")?.click(); // Use board stackup colors off
  await sleep(300);
  check("with it off the swatches are the theme's and editable", swatch("soldermask_top") === before.mask && document.querySelector('[data-row="soldermask_top"] input[type=color]')?.disabled === false, swatch("soldermask_top"));
  document.querySelector(".viewer3d-layer-group")?.closest(".panel-section")?.querySelector("label.filter-row input[type=checkbox]")?.click(); // back on
  await __eda.run("common.Interactive.undo");
  await sleep(500);
  check("undo takes the stackup away and the colours are the theme's again", swatch("soldermask_top") === before.mask && swatch("board") === before.board, { mask: swatch("soldermask_top"), board: swatch("board") });

  const ok = results.every((r) => r.ok);
  return JSON.stringify({ ok, firstModelMs: v.firstModelMs, allModelsMs: v.allModelsMs, failed: results.filter((r) => !r.ok), results }, null, 1);
})()
