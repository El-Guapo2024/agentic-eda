// The 3D models of the board's parts, UI-LEFT D4 (GAPS.md item 30, PARITY-3d section 6, PARITY-pcb section 31): Migrate 3D Models when the board opens (`AutoMigrateByFilename` and the
// load notice "%d WRL 3D model(s) could not be matched ..." with "Show options"); an Extruded 3D Body (`EXTRUDED_3D_BODY`) drawn in the 3D view as KiCad's OpenGL renderer draws it -- its
// heights, its outline, its pins, a back footprint's below the board -- and set in the 3D Models tab's box; X3D and IGES models converted by kicad-cli and drawn; and the tab of a back
// footprint showing the numbers the board file has.
//
// Run it in the page of a served board with placed footprints whose models are KiCad's (`eda place examples/mcu_board_30plus.yaml` then `eda board new` and the design copied in: the
// mcu30-placed board) -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board: it edits footprints and leaves the edits.
// The load-time migration is checked on what the page did when it opened: when the board had no VRML reference then (a fresh copy), the check gives two footprints old references and runs
// the same pass again (`window.__edaAutoMigrate3d.rerun()`); for the real load-time path, set them before the page opens (R7: `${KICAD6_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl`,
// R8: `${KICAD6_3DMODEL_DIR}/Nowhere.3dshapes/Nothing_Like_It.wrl`, with `set_board_models`). Needs kicad-cli (the X3D and IGES conversions) and KiCad's 3D model library.
//
// It returns { ok, results: [{ name, ok, detail? }, ...] }. Waits are fetch loops (`/api/version`), not timers: a hidden pane throttles timers to once a minute.
// run-all-checks: board=mcu30-placed minutes=12
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const json = (u) => fetch(u, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const part = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const until = async (test, ms = 60000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await test()) return true; await sleep(150); } return false; };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const click = async (el, ms = 400) => { el?.click(); await sleep(ms); };
  const view = () => window.__eda.state().viewer3d;
  const toPcb = async () => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(800); };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(150); };
  const openTab = async (ref) => { await toPcb(); await select([ref]); await __eda.run("pcbnew.InteractiveEdit.properties"); await until(() => tid("footprint-properties"), 10000); await click(tid("footprint-properties-tab-models"), 700); };
  const ok = async () => click([...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "OK"), 1500);
  const cancel = async () => click([...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Cancel"), 500);
  const boardText = async (ref) => (await (await fetch("/api/clipboard/copy", { method: "POST", body: JSON.stringify({ ids: [ref], reference: { x: 0, y: 0 } }) })).json()).text;
  const near = (a, b, tol = 1e-3) => Math.abs(a - b) <= tol;
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();

  const LIB = "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step";
  const WRL = "${KICAD6_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl";
  const GONE = "${KICAD6_3DMODEL_DIR}/Nowhere.3dshapes/Nothing_Like_It.wrl";
  const plain = (path, over = {}) => ({ path, offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 1, show: true, ...over });

  // ------------------------------------------------------------------------------------------------------------- Migrate 3D Models at load
  const hook = window.__edaAutoMigrate3d;
  check("the page ran Migrate 3D Models' load-time pass", await until(() => hook && hook.runs.length > 0, 30000), hook?.runs);
  let run = hook?.runs?.[0];
  const atLoad = !!run && Object.keys(run.replaced ?? {}).length > 0;
  if (!atLoad) {
    // a board that opened with no VRML reference: two old references, and the pass again as at load
    await cmd({ op: "set_board_models", part: "R7", models: [plain(WRL, { offset: [0.1, 0, 0] })] });
    await cmd({ op: "set_board_models", part: "R8", models: [plain(GONE)] });
    run = await hook.rerun();
  }
  check(`(the pass checked ran ${atLoad ? "when the page opened the board" : "again on references set by the check"})`, true);
  check("a missing .wrl a STEP of the same name stands for is replaced by it, as the path a footprint writes", run?.stepped && run.replaced[WRL] === "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step", run);
  const r7 = await part("R7");
  check("the footprint names the STEP now, its placement kept", r7?.models?.[0]?.name === run?.replaced?.[WRL] && r7.models[0].offset[0] === 0.1, r7?.models);
  check("the reference nothing stands for is left and counted", run?.unresolved === 1 && (await part("R8"))?.models?.[0]?.name === GONE, run);
  const bar = tid("automigrate-infobar");
  check("the load notice says it in KiCad's words", bar?.textContent.includes("1 WRL 3D model could not be matched to an equivalent STEP model."), bar?.textContent);
  check("with a Show options link and a close button", !!tid("automigrate-show-options") && !!tid("automigrate-close"));
  await click(tid("automigrate-show-options"), 1500);
  const dlg = tid("migrate3d-dialog");
  check("Show options opens Migrate 3D Models, which lists the one left", !!dlg && (await until(() => tid("migrate3d-missing")?.textContent.includes("Nothing_Like_It.wrl"), 15000)), tid("migrate3d-missing")?.textContent);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await sleep(800);
  check("closed with one left, the notice stays", !tid("migrate3d-dialog") && !!tid("automigrate-infobar"));
  await click(tid("automigrate-close"));
  check("the close button takes it away", !tid("automigrate-infobar"));
  // one undo step, which the step before it does not share
  await toPcb();
  await __eda.run("common.Interactive.undo");
  await sleep(1200);
  check("one undo puts the old reference back", (await part("R7"))?.models?.[0]?.name === WRL, (await part("R7"))?.models);
  await __eda.run("common.Interactive.redo");
  await sleep(1200);
  check("and redo the STEP", (await part("R7"))?.models?.[0]?.name === run?.replaced?.[WRL]);

  // ------------------------------------------------------------------------------------------------------------- the Extruded 3D Body
  const body = (over) => ({ height: 3_000_000, standoff: 1_000_000, layer: "auto", material: "plastic", ...over });
  check("J1's body is set in one command", (await cmd({ op: "set_extruded_body", part: "J1", body: body({}) })).ok);
  check("R1's body (the pins' box, copper) too", (await cmd({ op: "set_extruded_body", part: "R1", body: body({ height: 600_000, standoff: 0, layer: "pad_bbox", material: "copper" }) })).ok);
  await cmd({ op: "flip_items", ids: ["C1"], pivot: (({ at }) => ({ x: at[0], y: at[1] }))(await part("C1")), direction: "left_right" });
  check("C1, on the back, has one", (await cmd({ op: "set_extruded_body", part: "C1", body: body({ height: 800_000, standoff: 0 }) })).ok);
  const j1 = await part("J1");
  check("the state carries J1's body with its outline and its pins", j1?.extruded_body?.outline?.length === 1 && j1.extruded_body.pins.length === 4, j1?.extruded_body && { outline: j1.extruded_body.outline.length, pins: j1.extruded_body.pins.length });
  await __eda.run("common.Control.show3DViewer");
  await until(() => view()?.open, 20000);
  const drawn = (ref) => window.__edaExtrudedBodies?.get(ref);
  check("the 3D view draws the three bodies", await until(() => ["J1", "R1", "C1"].every((r) => drawn(r)), 30000), [...(window.__edaExtrudedBodies?.keys() ?? [])]);
  const dj = drawn("J1");
  // `GetFootprintZPos`: on the front, half the board, the copper and the paste (0.04 mm) up; the body from the standoff (1 mm) to the height (3 mm)
  const top = dj ? dj.low - 1 : NaN;
  check("J1's body stands on the board past its standoff, as tall as its height less the standoff", dj && near(dj.high - dj.low, 2) && top > 0.75 && top < 1.0, dj);
  const cy = j1?.courtyard;
  check("auto: its outline is the courtyard", dj && cy && near(dj.box[0], cy[0] / 1e6, 0.06) && near(dj.box[2], cy[2] / 1e6, 0.06) && near(dj.box[1], cy[1] / 1e6, 0.06) && near(dj.box[3], cy[3] / 1e6, 0.06), { drawn: dj?.box, courtyard: cy?.map((v) => v / 1e6) });
  check("with its pins (the body stands off the board)", dj?.pins === true);
  const dr = drawn("R1");
  check("R1's body: copper's colour (GetDefaultColor), no standoff, no pins", dr && dr.color === "#b87333" && dr.pins === false && near(dr.high - dr.low, 0.6), dr);
  const dc = drawn("C1");
  check("C1's body hangs below the board", dc && dc.high < 0 && near(dc.high - dc.low, 0.8), dc);
  // (R1's 0603 STEP is in the installed library; J1's pin header may not be, on a machine with a partial library)
  const r1Model = () => view()?.detail?.find((d) => d.name === LIB);
  check("a part with a body and its model is drawn with both (the model is not replaced)", (await until(() => r1Model()?.status === "ready", 90000)) && !!drawn("R1") && view().parts.asModels > 0, r1Model());

  // the board file: KiCad's form, the back's layer
  const words = (s) => s.split(/\s+/).join(" ");
  check("the board file has J1's body as format( FOOTPRINT ) writes it", words(await boardText("J1")).includes("(model (type extruded) (hide no) (overall_height 3) (body_pcb_gap 1) (layer auto) (material plastic) (color unspecified)"));

  // the 3D Models tab's box
  await openTab("J1");
  check("the tab has the Extruded 3D Body box, filled from the body", tid("extruded-enable")?.checked === true && tid("extruded-height")?.value === "3.0000" && tid("extruded-standoff")?.value === "1.0000" && tid("extruded-source")?.value === "0", { height: tid("extruded-height")?.value, standoff: tid("extruded-standoff")?.value });
  await type(tid("extruded-height"), "0");
  await ok();
  const err = document.querySelector(".dialog-footer")?.closest(".dialog")?.textContent ?? "";
  check("a height of zero is refused in KiCad's words, and the dialog stays", !!tid("footprint-properties") && err.includes("Component height must be greater than zero."), err.slice(0, 200));
  await type(tid("extruded-height"), "4");
  await ok();
  check("OK sets the new height in one step", !tid("footprint-properties") && (await part("J1"))?.extruded_body?.height === 4_000_000, (await part("J1"))?.extruded_body?.height);
  await __eda.run("common.Interactive.undo");
  await sleep(1200);
  check("one undo takes it back", (await part("J1"))?.extruded_body?.height === 3_000_000);

  // ------------------------------------------------------------------------------------------------------------- X3D and IGES models
  const x3d = `<?xml version="1.0" encoding="UTF-8"?>\n<X3D profile="Immersive" version="3.2"><Scene><Transform><Shape><Appearance><Material diffuseColor="0.8 0.1 0.1"/></Appearance><IndexedFaceSet coordIndex="0 1 2 3 -1 7 6 5 4 -1 0 4 5 1 -1 1 5 6 2 -1 2 6 7 3 -1 3 7 4 0 -1"><Coordinate point="0 0 0 1 0 0 1 1 0 0 1 0 0 0 1 1 0 1 1 1 1 0 1 1"/></IndexedFaceSet></Shape></Transform></Scene></X3D>\n`;
  // a box of six bilinear B-spline faces (entity 128), millimetres, in IGES's fixed 80 columns
  const iges = (() => {
    const line = (t, sec, n) => `${t.padEnd(72)}${sec}${String(n).padStart(7)}\n`;
    const g = "1H,,1H;,3Hbox,7Hbox.igs,3Heda,3H1.0,32,38,6,308,15,3Hbox,1.,2,2HMM,1,1.,15H20261106.000000,0.001,10.,3Heda,3Heda,11,0,15H20261106.000000;";
    const G = []; for (let i = 0; i < g.length; i += 72) G.push(line(g.slice(i, i + 72), "G", G.length + 1));
    const [X, Y, Z] = [1.6, 0.8, 0.6];
    const faces = [[[0, 0, 0], [0, Y, 0], [X, 0, 0], [X, Y, 0]], [[0, 0, Z], [X, 0, Z], [0, Y, Z], [X, Y, Z]], [[0, 0, 0], [X, 0, 0], [0, 0, Z], [X, 0, Z]], [[0, Y, 0], [0, Y, Z], [X, Y, 0], [X, Y, Z]], [[0, 0, 0], [0, 0, Z], [0, Y, 0], [0, Y, Z]], [[X, 0, 0], [X, Y, 0], [X, 0, Z], [X, Y, Z]]];
    const D = []; const P = [];
    faces.forEach((f, i) => {
      const de = 2 * i + 1;
      let text = ["128,1,1,1,1,0,0,1,0,0,0.,0.,1.,1.,0.,0.,1.,1.,1.,1.,1.,1.", ...f.flat().map((c) => (Number.isInteger(c) ? `${c}.` : String(c))), "0.,1.,0.,1."].join(",") + ";";
      const first = P.length + 1;
      while (text) { const cut = text.length > 64 ? text.slice(0, 64).lastIndexOf(",") + 1 : text.length; P.push(`${text.slice(0, cut).padEnd(64)}${String(de).padStart(8)}P${String(P.length + 1).padStart(7)}\n`); text = text.slice(cut); }
      const f8 = (...v) => v.map((x) => String(x).padStart(8)).join("");
      D.push(line(f8(128, first, 0, 0, 0, 0, 0, 0, "00000000"), "D", de), line(f8(128, 0, 0, P.length + 1 - first, 0, "", "", "", 0), "D", de + 1));
    });
    return line("eda hand-written IGES box", "S", 1) + G.join("") + D.join("") + P.join("") + line(`S${String(1).padStart(7)}G${String(G.length).padStart(7)}D${String(D.length).padStart(7)}P${String(P.length).padStart(7)}`, "T", 1);
  })();
  const embed = async (name, text) => (await cmd({ op: "embedded", verb: "add", owner: { kind: "board" }, name, data: btoa(text), kind: "model" })).ok;
  check("an X3D and an IGES model are embedded in the board", (await embed("cube.x3d", x3d)) && (await embed("box.igs", iges)));
  await cmd({ op: "set_board_models", part: "D1", models: [plain("kicad-embed://cube.x3d")] });
  await cmd({ op: "set_board_models", part: "D2", models: [plain("kicad-embed://box.igs")] });
  const statusOf = (name) => view()?.detail?.find((d) => d.name === name);
  await __eda.run("common.Control.show3DViewer");
  check("the X3D model is converted by kicad-cli and drawn", await until(() => statusOf("kicad-embed://cube.x3d")?.status === "ready", 90000), statusOf("kicad-embed://cube.x3d"));
  check("so is the IGES model", await until(() => statusOf("kicad-embed://box.igs")?.status === "ready", 90000), statusOf("kicad-embed://box.igs"));
  const tri = (n) => statusOf(n)?.triangles ?? 0;
  check("with their faces (the cube's 12 triangles; the IGES box's, both sides as KiCad's IGES reading draws them)", tri("kicad-embed://cube.x3d") === 12 && tri("kicad-embed://box.igs") >= 12, { x3d: tri("kicad-embed://cube.x3d"), iges: tri("kicad-embed://box.igs") });

  // ------------------------------------------------------------------------------------------------------------- a back footprint's tab
  await cmd({ op: "set_board_models", part: "C2", models: [plain("${KICAD10_3DMODEL_DIR}/Capacitor_SMD.3dshapes/C_0402_1005Metric.step", { offset: [1, 2, 0.5], rotate: [0, 0, 30] })] });
  const c2 = await part("C2");
  await cmd({ op: "flip_items", ids: ["C2"], pivot: { x: c2.at[0], y: c2.at[1] }, direction: "left_right" });
  const file = words(await boardText("C2"));
  check("on the back, the board file has the model's numbers as they were (FOOTPRINT::Flip does not touch them)", file.includes('(layer "B.Cu")') && file.includes("(offset (xyz 1 2 0.5) )") && file.includes("(rotate (xyz 0 0 30) )"), file.slice(file.indexOf("(model "), file.indexOf("(model ") + 200));
  await openTab("C2");
  const shown = ["x", "y", "z"].map((a) => tid(`model3d-offset-${a}`)?.value);
  check("the tab shows those numbers, as KiCad's does", shown[0]?.startsWith("1.000000") && shown[1]?.startsWith("2.000000") && shown[2]?.startsWith("0.500000") && tid("model3d-rotation-z")?.value === "-30.00°", { offset: shown, rotation: tid("model3d-rotation-z")?.value });
  await cancel();
  await toPcb();

  check("no error was logged", (__eda.errors?.() ?? []).length === errorsFrom, (__eda.errors?.() ?? []).slice(errorsFrom, errorsFrom + 3));
  return JSON.stringify({ ok: results.every((r) => r.ok), results });
})();
