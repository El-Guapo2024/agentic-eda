// 3D viewer controls and scene check (UI-LEFT D1-D3, PARITY-3d sections 3, 5, 9): open the 3D tab of a served board with KiCad's trapezoid, chamfered and rounded pads and
// non-plated holes (KiCad 8303b2ad's qa/data/pcbnew/padstacks_normal.kicad_pcb with an Edge.Cuts rectangle added, brought in with `replace_board`), and check through
// `window.__eda` (`state().viewer3d`, components/viewer3d/viewer3dProbe.ts) and the panel's and the menu's own controls:
//
//   1  the scene: the body has every through and non-plated hole cut out (GET /api/board3d), the pads are their real polygons (a trapezoid four corners, a chamfered and a
//      rounded rectangle more), the plated holes have barrels, the pads open the solder masks
//   2  the layer tree: Values, Footprint Text (off takes References and Values with it), Off-board Silkscreen (off: the silkscreen is culled by the board's outside),
//      3D Navigator
//   3  the right-click menu: a right click that is not a drag opens it, Rotate Board opens its submenu, View Front moves the camera to the front view
//   4  the 3D Navigator: a click on its +X sphere goes to the right view
//   5  layer presets: Follow PCB Editor, Save preset... (a name, OK), a row toggled leaves it, choosing it brings the rows back, Delete preset...
//   6  viewports: Save viewport... keeps the camera in appearance.json, choosing it brings the camera back, Delete viewport...
//   7  the selection: a click on a footprint in 3D selects it (the PCB editor's selection) and it is drawn in the selection colour
//   8  Preferences > 3D Viewer > General: the rotation increment a Rotate command turns by
//   9  inner copper: on a four-layer stackup the inner layers are drawn only with the body hidden, each in its PCB editor colour with Use PCB editor copper colors
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright (web/studio/e2e/run-check.mjs) -- on a COPY: it saves and deletes a preset and a viewport.
// It answers through `window.__viewer3dControlsCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=padstacks-3d minutes=12   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__viewer3dControlsCheck = { results, done: false, ok: false, wait: null });
  let waiters = [];
  check.wait = (ms) => new Promise((resolve) => { if (check.done) return resolve(check); const t = setTimeout(() => resolve(check), ms); waiters.push(() => { clearTimeout(t); resolve(check); }); });
  const finish = () => { check.done = true; check.ok = results.length > 0 && results.every((r) => r.ok); for (const w of waiters) w(); waiters = []; };
  const ok = (name, cond, notes = []) => results.push({ name, ok: !!cond, notes: cond ? notes : [`FAIL ${notes.join("; ")}`] });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const frame = () => new Promise((r) => requestAnimationFrame(() => r()));
  const settle = async () => { await frame(); await sleep(60); await frame(); };
  const view = () => window.__eda.state().viewer3d;
  const waitFor = async (test, ms = 30000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (test()) return true; await sleep(50); } return false; };
  const canvas = () => document.querySelector(".pcb-canvas-container canvas");
  const pointer = (type, x, y, button) => {
    const c = canvas();
    const r = c.getBoundingClientRect();
    c.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: r.left + x, clientY: r.top + y, button, buttons: type === "pointerup" ? 0 : 1 << (button === 2 ? 1 : 0), pointerId: 7, pointerType: "mouse" }));
  };
  const click = async (x, y, button = 0) => { pointer("pointerdown", x, y, button); await frame(); pointer("pointerup", x, y, button); await settle(); };
  const row = (id) => document.querySelector(`[data-row="${id}"] input[type=checkbox]`);
  const menuItem = (label) => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].find((e) => e.textContent.trim().startsWith(label));
  const cam = () => window.__eda.viewer3dCamera();
  const near = (a, b, tol = 0.5) => Math.abs(a - b) <= tol;
  const choose = async (selectId, value) => { const s = document.getElementById(selectId); s.value = value; s.dispatchEvent(new Event("change", { bubbles: true })); await settle(); };
  const dialogInput = () => document.querySelector(".dialog input[type=text], .dialog input:not([type])");
  const dialogButton = (label) => [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === label);
  const typeInto = (input, text) => { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set; set.call(input, text); input.dispatchEvent(new Event("input", { bubbles: true })); };

  try {
    await window.__eda.run("common.Control.show3DViewer");
    if (!(await waitFor(() => view() && view().open))) throw new Error("the 3D tab did not open");
    // 1 -------------------------------------------------------------------------------------------------------------------------------------- the scene
    await waitFor(() => view().bodyFromServer && (view().scene["pad"] ?? 0) > 0, 60000);
    const board = await fetch("/api/state").then((r) => r.json());
    const pads = board.parts.flatMap((p) => p.pads ?? []);
    const npth = pads.filter((p) => p.kind === "non_plated_hole").length;
    const pth = pads.filter((p) => p.th && p.kind !== "non_plated_hole" && (p.drill || p.slot)).length;
    const vias = (board.routing?.vias ?? []).filter((v) => !v.type).length;
    const b3d = await fetch("/api/board3d").then((r) => r.json());
    const served = (b3d.body ?? []).reduce((n, p) => n + p.holes.length, 0);
    ok("the body has every through and non-plated hole cut out", npth > 0 && served === npth + pth + vias && view().bodyHoles === served, [`npth ${npth}, plated ${pth}, vias ${vias}, served ${served}, drawn ${view().bodyHoles}`]);
    const shapes = view().padShapes;
    const trap = shapes.trapezoid;
    ok("a trapezoid pad is drawn as its four corners", trap && trap.meshes > 0 && trap.corners === 4 * trap.meshes, [JSON.stringify(trap)]);
    const ch = shapes.chamfered_rect;
    ok("a chamfered pad has its corners cut", ch && ch.meshes > 0 && ch.corners > 4 * ch.meshes, [JSON.stringify(ch)]);
    const rr = shapes.round_rect;
    ok("a rounded rectangle has its arcs", rr && rr.meshes > 0 && rr.corners > 16 * rr.meshes, [JSON.stringify(rr)]);
    ok("the plated holes have barrels", (view().scene["plated-barrel"] ?? 0) >= pth, [`${view().scene["plated-barrel"]} barrels for ${pth} plated holes`]);
    ok("the pads open both solder masks (the stencil pieces of renderSolderMaskLayer's culling)", (view().scene["mask-opening-top"] ?? 0) === 1 && (view().scene["mask-opening-bottom"] ?? 0) === 1, [JSON.stringify([view().scene["mask-opening-top"], view().scene["mask-opening-bottom"]])]);

    // 2 ---------------------------------------------------------------------------------------------------------------------------------- the layer tree
    const ids = [...document.querySelectorAll("[data-row]")].map((r) => r.getAttribute("data-row"));
    ok("the tree lists Values, Footprint Text, Off-board Silkscreen and 3D Navigator in KiCad's order", ["values", "references", "footprint_text", "off_board_silk", "navigator"].every((id, i, a) => ids.includes(id) && (i === 0 || ids.indexOf(a[i - 1]) < ids.indexOf(id))), [ids.slice(-10).join(",")]);
    const refsBefore = view().scene["part-ref"] ?? 0;
    row("footprint_text").click();
    await settle();
    ok("Footprint Text off takes References and Values with it", !row("references").checked && !row("values").checked && (view().scene["part-ref"] ?? 0) === 0, [`refs ${refsBefore} -> ${view().scene["part-ref"] ?? 0}`]);
    row("references").click();
    await settle();
    ok("References on turns Footprint Text back on", row("footprint_text").checked && row("references").checked && !row("values").checked, []);
    row("values").click();
    await settle();
    ok("the silkscreen is clipped to the board while Off-board Silkscreen is off", !row("off_board_silk").checked && (view().layers3d?.silkBoardCulled ?? 0) > 0, [JSON.stringify(view().layers3d?.silkBoardCulled)]);
    row("off_board_silk").click();
    await settle();
    ok("Off-board Silkscreen on leaves it unclipped", (view().layers3d?.silkBoardCulled ?? 1) === 0, []);
    row("off_board_silk").click();
    await settle();
    ok("the 3D Navigator is drawn by default and its row hides it", view().navigator === true && (row("navigator").click(), await settle(), view().navigator === false), []);
    row("navigator").click();
    await settle();

    // 3 ------------------------------------------------------------------------------------------------------------------------------ the right-click menu
    const c = canvas();
    const [w, h] = [c.clientWidth, c.clientHeight];
    await click(w / 2, h / 2, 2);
    const labels = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].map((e) => e.textContent.trim());
    ok("a right click opens EDA_3D_CONTROLLER's menu", ["Zoom In", "Zoom Out", "View Top", "View Bottom", "View Right", "View Left", "View Front", "View Back", "Rotate Board", "Flip Board", "Move Board"].every((l) => labels.some((x) => x.startsWith(l))), [labels.join(" | ")]);
    menuItem("Rotate Board").dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await settle();
    const sub = [...document.querySelectorAll(".menubar-dropdown .menubar-dropdown .menu-node-item")].map((e) => e.textContent.trim());
    ok("Rotate Board opens its submenu", ["Rotate X Clockwise", "Rotate Y Counterclockwise", "Rotate Z Clockwise"].every((l) => sub.includes(l)), [sub.join(" | ")]);
    menuItem("View Front").click();
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.x, -90), 8000);
    ok("View Front from the menu goes to the front view", near(cam().auxDeg.x, -90) && !document.querySelector(".menubar-dropdown"), [JSON.stringify(cam().auxDeg)]);

    // 4 -------------------------------------------------------------------------------------------------------------------------------- the 3D Navigator
    await click(w / 2, h / 2, 2);
    menuItem("View Top").click();
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.x, 0), 8000);
    // Seen from the top the +X sphere is right of the gizmo's centre: 1.6 / ( 4.4 * tan 30 ) of its half-width (SPHERES_GIZMO's camera).
    const size = Math.trunc(h / 8);
    const ndcX = 1.6 / (4.4 * Math.tan(Math.PI / 6));
    await click(4 + ((ndcX + 1) / 2) * size, h - 4 - size / 2, 0);
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.z, -90), 8000);
    ok("a click on the navigator's X sphere goes to the right view", near(cam().auxDeg.x, -90) && near(cam().auxDeg.z, -90), [JSON.stringify(cam().auxDeg)]);

    // 5 ------------------------------------------------------------------------------------------------------------------------------------ layer presets
    const presets = () => [...document.querySelectorAll("#v3d-presets option")].map((o) => o.textContent.trim());
    ok("the presets list KiCad's two Follow presets and the commands", ["Follow PCB Editor", "Follow PCB Plot Settings", "Save preset...", "Delete preset..."].every((p) => presets().includes(p)), [presets().join(" | ")]);
    await choose("v3d-presets", "p:follow_pcb_editor");
    ok("Follow PCB Editor is chosen", document.getElementById("v3d-presets").value === "p:follow_pcb_editor", []);
    row("board").click(); // off: a board row takes the view off the Follow preset
    await settle();
    ok("a board row turned off leaves the Follow preset", document.getElementById("v3d-presets").value === "", [document.getElementById("v3d-presets").value]);
    await choose("v3d-presets", "cmd:save");
    typeInto(dialogInput(), "Check no body");
    dialogButton("OK").click();
    await settle();
    ok("Save preset... keeps the rows under the name typed", document.getElementById("v3d-presets").value === "p:Check no body" && presets().includes("Check no body"), [document.getElementById("v3d-presets").value]);
    row("board").click(); // the body back on: no preset matches
    await settle();
    ok("a row changed after it matches no preset", document.getElementById("v3d-presets").value === "" && row("board").checked, []);
    await choose("v3d-presets", "p:Check no body");
    ok("choosing the preset brings its rows back", !row("board").checked && (view().scene["board-slab"] ?? 0) === 0, [`slab ${view().scene["board-slab"]}`]);
    await choose("v3d-presets", "cmd:delete");
    const pick = document.querySelector(".dialog select, .dialog [role=listbox]");
    if (pick && pick.tagName === "SELECT") { pick.value = "Check no body"; pick.dispatchEvent(new Event("change", { bubbles: true })); }
    else [...document.querySelectorAll(".dialog *")].find((e) => e.textContent.trim() === "Check no body" && e.children.length === 0)?.click();
    await settle();
    dialogButton("OK").click();
    await settle();
    ok("Delete preset... takes it out of the list", !presets().includes("Check no body"), [presets().join(" | ")]);
    await choose("v3d-presets", "p:follow_plot_settings");
    ok("Follow PCB Plot Settings shows the body again", row("board").checked, []);

    // 6 ------------------------------------------------------------------------------------------------------------------------------------- viewports
    await click(w / 2, h / 2, 2);
    menuItem("View Front").click();
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.x, -90) && near(cam().auxDeg.z, 0), 8000);
    const saved = cam();
    await choose("v3d-viewports", "cmd:save");
    typeInto(dialogInput(), "Check front");
    dialogButton("OK").click();
    await settle();
    const stored = await fetch("/api/appearance").then((r) => r.json());
    ok("Save viewport... keeps the camera in the project's appearance.json", (stored.viewer3d?.viewports ?? []).some((v) => v.name === "Check front" && near(v.camera.auxDeg.x, -90)), [JSON.stringify(stored.viewer3d ?? null).slice(0, 200)]);
    await click(w / 2, h / 2, 2);
    menuItem("View Top").click();
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.x, 0), 8000);
    await choose("v3d-viewports", "v:Check front");
    ok("choosing the viewport puts the camera back", near(cam().auxDeg.x, saved.auxDeg.x, 1e-6) && near(cam().zoom, saved.zoom, 1e-9), [JSON.stringify(cam().auxDeg)]);
    await choose("v3d-viewports", "cmd:delete");
    dialogButton("OK").click();
    await settle();
    const after = await fetch("/api/appearance").then((r) => r.json());
    ok("Delete viewport... takes it out", !(after.viewer3d?.viewports ?? []).some((v) => v.name === "Check front"), []);

    // 7 ------------------------------------------------------------------------------------------------------------------------------------ the selection
    await click(w / 2, h / 2, 2);
    menuItem("View Top").click();
    await waitFor(() => !view().cameraMoving && near(cam().auxDeg.x, 0), 8000);
    const ref = board.parts.find((p) => p.placed)?.ref;
    const at = ref && window.__eda.viewer3dScreen(ref);
    if (at) await click(at.x, at.y, 0);
    await waitFor(() => (view().selected ?? []).includes(ref), 5000);
    const drawn = [...view().selected];
    // The board editor's selection, as its own tab reports it.
    [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click();
    await waitFor(() => window.__eda.state().tab === "pcb", 5000);
    const sel = window.__eda.state().selection.map((s) => s.id);
    await window.__eda.run("common.Control.show3DViewer");
    await waitFor(() => view() && view().open, 10000);
    await settle();
    ok("a click on a footprint in 3D selects it on the board, drawn in the selection colour", sel.includes(ref) && drawn.includes(ref) && view().selected.includes(ref), [`selection ${sel.join(",")}, drawn ${drawn.join(",")} then ${view().selected.join(",")}`]);

    // 8 ---------------------------------------------------------------------------------------------------------------------------- Preferences > 3D Viewer
    await window.__eda.run("common.SuiteControl.openPreferences");
    await settle();
    document.querySelector('[data-page="3d_general"]').click();
    await settle();
    const inc = document.querySelector('[aria-label="Rotation increment"]');
    const setNum = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setNum.call(inc, "45");
    inc.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    dialogButton("OK").click();
    await settle();
    const z0 = cam().auxDeg.z;
    await click(w / 2, h / 2, 2);
    menuItem("Rotate Board").click();
    await settle();
    [...document.querySelectorAll(".menubar-dropdown .menubar-dropdown .menu-node-item")].find((e) => e.textContent.trim() === "Rotate Z Clockwise").click();
    await settle();
    ok("Rotate Z Clockwise turns by the rotation increment set in Preferences", near(Math.abs(cam().auxDeg.z - z0), 45, 1e-6), [`${z0} -> ${cam().auxDeg.z}`]);
    // Back to KiCad's 10 degrees.
    await window.__eda.run("common.SuiteControl.openPreferences");
    await settle();
    document.querySelector('[data-page="3d_general"]').click();
    await settle();
    setNum.call(document.querySelector('[aria-label="Rotation increment"]'), "10");
    document.querySelector('[aria-label="Rotation increment"]').dispatchEvent(new Event("input", { bubbles: true }));
    dialogButton("OK").click();
    await settle();
    // 9 ---------------------------------------------------------------------------------------------------------------------------- inner copper
    const layer = (name, kind, t, extra = {}) => ({ name, kind, material: kind === "copper" ? null : "FR4", thickness_mm: t, ...extra });
    const four = { copper_layers: 4, stackup: { layers: [
      { name: "F.SilkS", kind: "Top Silk Screen", material: null, thickness_mm: null }, { name: "F.Paste", kind: "Top Solder Paste", material: null, thickness_mm: null },
      { name: "F.Mask", kind: "Top Solder Mask", material: null, thickness_mm: 0.01 }, layer("F.Cu", "copper", 0.035), layer("dielectric 1", "prepreg", 0.2, { epsilon_r: 4.5, loss_tangent: 0.02 }),
      layer("In1.Cu", "copper", 0.035), layer("dielectric 2", "core", 1.065, { epsilon_r: 4.5, loss_tangent: 0.02 }), layer("In2.Cu", "copper", 0.035),
      layer("dielectric 3", "prepreg", 0.2, { epsilon_r: 4.5, loss_tangent: 0.02 }), layer("B.Cu", "copper", 0.035),
      { name: "B.Mask", kind: "Bottom Solder Mask", material: null, thickness_mm: 0.01 }, { name: "B.Paste", kind: "Bottom Solder Paste", material: null, thickness_mm: null },
      { name: "B.SilkS", kind: "Bottom Silk Screen", material: null, thickness_mm: null },
    ], copper_finish: "None", dielectric_constraints: false, edge_connector: 0, edge_plating: false } };
    const reply = await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "set_stackup", settings: four }, strict: false }) }).then((r) => r.json());
    // The board's new layers come with the next state poll.
    await waitFor(() => fetch("/api/state").then((r) => r.json()).then((st) => st.layers.length === 4) && true, 100);
    await sleep(2000);
    await settle();
    const withBody = Object.keys(view().copperColors);
    ok("four copper layers: with the opaque body shown, the inner ones are not drawn", reply.ok && withBody.includes("F.Cu") && !withBody.includes("In1.Cu"), [reply.message ?? "", withBody.join(",")]);
    row("board").click();
    await settle();
    const without = Object.keys(view().copperColors);
    ok("with the body hidden the inner copper is drawn (Redraw's opacity_min)", without.includes("In1.Cu") && without.includes("In2.Cu"), [without.join(",")]);
    const editorBox = [...document.querySelectorAll("label.filter-row")].find((l) => /Use PCB editor copper colors/.test(l.textContent))?.querySelector("input");
    editorBox.click();
    await settle();
    const cc = view().copperColors;
    ok("with Use PCB editor copper colors each copper layer has its own colour", cc["In1.Cu"] === "#7fc87f" && cc["F.Cu"] === "#c83434" && cc["In1.Cu"] !== cc["In2.Cu"], [JSON.stringify(cc)]);
    editorBox.click();
    row("board").click();
    await window.__eda.run("common.Interactive.undo");
    await settle();

    const errors = window.__eda.errors().filter((e) => !/404|Failed to load resource/.test(e.message));
    ok("no errors in the page", errors.length === 0, errors.map((e) => e.message.slice(0, 200)));
  } catch (e) {
    results.push({ name: "the check ran to the end", ok: false, notes: [String((e && e.stack) || e).slice(0, 600)] });
  }
  finish();
  return check;
})();
