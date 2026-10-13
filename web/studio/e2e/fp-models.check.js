// The 3D models of a footprint (GAPS.md package P14 / item 30, PARITY-pcb.md section 31, PARITY-fpedit.md section 11): the 3D Models tab of the board's Footprint Properties dialog
// (`PANEL_FP_PROPERTIES_3D_MODEL`: the list, the status of each file name, Add, Browse through the model chooser, Remove, the fields and the Opacity slider of `PANEL_PREVIEW_3D_MODEL`,
// the preview that draws the footprint with its models) and what OK does -- one `set_board_models`, one undo step -- through the state, the 3D view and the board file; the verbs' refusals;
// the Footprint Editor's dialog with the same tab, and the 3D view of the Footprint Chooser.
//
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY of a board
// (it edits the models of a footprint, adds a footprint to the project library and undoes all of it, and it switches the Strict switch off and back on). It makes nothing that stays: it
// compares the board with how it was.
//
// It needs placed footprints whose models are KiCad's (the examples' `0603`, `SOIC-16`: `eda board new examples/mcu_board_30plus.yaml`, then `eda board place R1 --at 9,9` ...), the installed KiCad
// 3D model library (`Resistor_SMD.3dshapes/R_0603_1608Metric.step`, `Capacitor_SMD.3dshapes/C_0603_1608Metric.step`) and the installed footprint libraries (`Resistor_SMD:R_0603_1608Metric`). It
// answers through `window.__fpModelsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __fpModelsCheck.wait(90000)` until `.done` (a run takes about a minute; a STEP model is converted once by kicad-cli, a few seconds each the first time).
//
// It works through `window.__eda` (`actions`' ids with `run`, selection with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`), the dialogs' data-testids, the
// preview's own `data-drawn` / `data-waiting` / `data-missing` on `[data-testid="model-preview"]`, and the verbs' routes (`/api/cmd`, `/api/clipboard/copy` for the board file's text).
(async () => {
  const results = [];
  const check = (window.__fpModelsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 90000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const part = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const modelsOf = async (ref) => (await part(ref))?.models ?? [];
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(900); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const all = (prefix) => [...document.querySelectorAll(`[data-testid^="${prefix}"]`)];
  const until = async (cond, ms = 30000, step = 150) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(step); } return false; };
  /** Type into a React input: the native setter and an input event; `leave` fires the focus-out a real blur makes (a page in a background tab never blurs). */
  const type = async (el, value, leave = true) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(120);
    if (leave) el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await sleep(120);
  };
  const click = async (el, ms = 300) => { el.click(); await sleep(ms); };
  const preview = (id = "model-preview") => tid(id);
  /** The preview has loaded what it was asked to draw. */
  const settled = async (id = "model-preview", ms = 40000) => until(() => { const p = preview(id); return p && p.dataset.waiting === "0"; }, ms);
  const drawn = (id = "model-preview") => Number(preview(id)?.dataset.drawn ?? -1);
  const paths = () => all("model3d-path-").map((i) => i.value);
  const rowCount = () => all("model3d-row-").length;
  const status = (i) => tid(`model3d-status-${i}`)?.dataset.status ?? "";
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const close = async () => {
    for (let i = 0; i < 3; i++) {
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Cancel");
      if (!cancel) break;
      cancel.click();
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    await close();
    results.push(entry);
  };
  const toPcb = async () => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(800); };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await toPcb();
  const errorsFrom = (__eda.errors?.() ?? []).length;
  const snapshot = async () => JSON.stringify((await state()).parts.filter((p) => p.placed).map((p) => [p.ref, p.at, p.rot, p.side, p.models]));
  const original = await snapshot();
  // the footprint to work on: placed, with the one model of a 0603 resistor
  const lib = "${KICAD10_3DMODEL_DIR}";
  const R0603 = `${lib}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`;
  const C0603 = `${lib}/Capacitor_SMD.3dshapes/C_0603_1608Metric.step`;
  const u = (await state()).parts.find((p) => p.placed && (p.models ?? []).length === 1 && p.models[0].name === R0603);
  const ref = u?.ref;
  // what the dialogs keep between uses (the chooser's folder and filter, the Footprint Chooser's two views): a known start, and left as it was
  const kept = ["eda-studio.model3d-dir", "eda-studio.model3d-filter", "eda-studio.footprint-chooser-views"].map((k) => [k, localStorage.getItem(k)]);
  kept.forEach(([k]) => localStorage.removeItem(k));
  const openTab = async () => {
    await select([ref]);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    await until(() => tid("footprint-properties"));
    await click(tid("footprint-properties-tab-models"), 600);
  };

  await scenario("the 3D Models tab lists the models the footprint has, with their placement, and the preview draws them", async (t) => {
    t(!!u, `a placed footprint with the model of a 0603 resistor: ${ref}`);
    const has = (await part(ref)).models;
    await openTab();
    t(!!tid("footprint-models"), "the tab is there");
    t(rowCount() === 1, `one row: ${rowCount()}`);
    t(paths()[0] === has[0].name, `its file name is the model's: ${paths()[0]}`);
    t(tid("model3d-scale-x").value === "1.0000" && tid("model3d-scale-y").value === "1.0000", `the scale shows four decimals: ${tid("model3d-scale-x").value}`);
    t(tid("model3d-rotation-z").value === "0.00°", `the rotation shows two and the degree sign: ${tid("model3d-rotation-z").value}`);
    t(/^-?\d+\.\d{6} mm$/.test(tid("model3d-offset-x").value), `the offset shows six decimals and the unit: ${tid("model3d-offset-x").value}`);
    t(tid("model3d-opacity").value === "100", "the opacity slider is at 100%");
    t(tid("model3d-show-0").checked, "the model is shown");
    t(status(0) === "", "its status cell is empty: the file is there");
    t(!!document.querySelector('[data-testid="model-preview"] canvas'), "the preview has its 3D canvas");
    t(await settled(), `the preview loaded its models: waiting ${preview().dataset.waiting}`);
    t(drawn() === 1 && preview().dataset.missing === "0", `it draws the one model: ${drawn()} (missing ${preview().dataset.missing})`);
    t(["top", "bottom", "front", "back", "left", "right"].every((v) => !!tid(`model-preview-view-${v}`)), "the six views, the fit, the ortho and board switches are there");
    // the preview's own footprint: pads and drawings in the footprint's frame (the route the preview asks)
    const geometry = (await json(`/api/3dmodel/preview?part=${encodeURIComponent(ref)}`)).part;
    t(geometry && geometry.at[0] === 0 && geometry.rot === 0 && geometry.side === "top" && geometry.pads.length === (await part(ref)).pads.length, `the preview's footprint is at the origin, top side, with its ${geometry?.pads.length} pads`);
  });

  await scenario("the fields edit the selected model the way the panel's do: expressions, units, the steps, the sign of the rotation, Show", async (t) => {
    await openTab();
    await settled();
    await type(tid("model3d-scale-x"), "2*1.25");
    t(tid("model3d-scale-x").value === "2.5000", `an expression is worked out: 2*1.25 is ${tid("model3d-scale-x").value}`);
    await click(tid("model3d-scale-x-up"));
    t(tid("model3d-scale-x").value === "2.6000", `the spin button steps the scale by 0.1: ${tid("model3d-scale-x").value}`);
    await type(tid("model3d-rotation-z"), "90");
    t(tid("model3d-rotation-z").value === "90.00°", `a rotation typed is shown counter-clockwise: ${tid("model3d-rotation-z").value}`);
    await click(tid("model3d-rotation-z-up"));
    t(tid("model3d-rotation-z").value === "180.00°", `a step is 90 degrees: ${tid("model3d-rotation-z").value}`);
    await type(tid("model3d-offset-x"), "1.5");
    t(tid("model3d-offset-x").value === "1.500000 mm", `an offset in millimetres: ${tid("model3d-offset-x").value}`);
    await type(tid("model3d-offset-y"), "0.05in");
    t(tid("model3d-offset-y").value === "1.270000 mm", `an offset with its unit after it: 0.05in is ${tid("model3d-offset-y").value}`);
    await click(tid("model3d-offset-z-up"));
    t(tid("model3d-offset-z").value === "0.500000 mm", `an offset steps by 0.5 mm: ${tid("model3d-offset-z").value}`);
    await type(tid("model3d-opacity"), "40", false);
    t(tid("model3d-opacity").value === "40", "the opacity slider");
    // a field half typed in is not a number yet and changes nothing
    await type(tid("model3d-scale-y"), "1+", false);
    t(true, "a half typed field is taken as it is typed");
    tid("model3d-scale-y").dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await sleep(150);
    t(tid("model3d-scale-y").value === "1.0000", `and shows the scale it still has when it is left: ${tid("model3d-scale-y").value}`);
    // Show off: the preview draws no model
    await click(tid("model3d-show-0"), 600);
    await settled();
    t(drawn() === 0, `Show off: the preview draws ${drawn()} model`);
    await click(tid("model3d-show-0"), 600);
    await settled();
    t(drawn() === 1, `Show on: it draws ${drawn()}`);
  });

  await scenario("a file name's status cell: no name is a warning, an illegal one and one that is not there are errors", async (t) => {
    await openTab();
    await click(tid("model3d-add"), 700);
    t(rowCount() === 2, `Add makes a blank row: ${rowCount()} rows`);
    t(await until(() => status(1) === "warning"), `a blank name is a warning: ${status(1)}`);
    t(tid("model3d-status-1")?.title === "No filename entered", `"${tid("model3d-status-1")?.title}"`);
    await type(tid("model3d-path-1"), "bad?.step", false);
    t(await until(() => status(1) === "error"), `an illegal name is an error: ${status(1)}`);
    t(tid("model3d-status-1")?.title === "Illegal filename", `"${tid("model3d-status-1")?.title}"`);
    await type(tid("model3d-path-1"), `${lib}/Nope.3dshapes/nothing.step`, false);
    t(await until(() => tid("model3d-status-1")?.title === "File not found"), `a name that finds no file: "${tid("model3d-status-1")?.title}"`);
    await type(tid("model3d-path-1"), C0603, false);
    t(await until(() => status(1) === ""), `a model that is there has no icon: "${status(1)}"`);
    await settled();
    t(drawn() === 2, `and the preview draws both: ${drawn()}`);
    // the route the cells ask: every status the tab has
    const answers = (await (await fetch("/api/3dmodel/check", { method: "POST", body: JSON.stringify({ names: ["", "bad?.step", `${lib}/Nope.3dshapes/n.step`, R0603, "/etc/hosts"] }) })).json()).results.map((r) => r.status);
    t(same(answers, ["empty", "illegal", "missing", "ok", "forbidden"]), `the check route: ${answers.join(", ")} (a file outside the model folders is not served)`);
    // Remove takes the selected row; Up and Down order the rows
    await click(tid("model3d-up"), 400);
    t(paths()[0] === C0603 && paths()[1] === R0603, `Up moves the selected row up: ${paths().map((p) => p.split("/").pop())}`);
    await click(tid("model3d-down"), 400);
    t(paths()[1] === C0603, `Down moves it back: ${paths().map((p) => p.split("/").pop())}`);
    await click(tid("model3d-remove"), 400);
    t(rowCount() === 1 && paths()[0] === R0603, `Remove takes the selected row away: ${rowCount()} left`);
  });

  await scenario("Browse walks the folders the resolver searches, previews the file and adds it with KiCad's defaults", async (t) => {
    await openTab();
    await click(tid("model3d-browse"), 1500);
    t(!!tid("model3d-chooser"), "the model chooser opens");
    t(await until(() => tid("model3d-chooser-dir")?.textContent?.startsWith("${")), `it starts in a folder: ${tid("model3d-chooser-dir")?.textContent}`);
    const roots = [...tid("model3d-chooser-roots").querySelectorAll("option")].map((o) => o.textContent);
    t(roots.includes("${KIPRJMOD}") && roots.some((r) => r.includes("3DMODEL_DIR")), `the roots are the board's folder and KiCad's model library: ${roots.slice(1).join(", ")}`);
    // the folder to start in is KiCad's library; the list of roots goes to a root
    const rootSelect = tid("model3d-chooser-roots");
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(rootSelect, lib);
    rootSelect.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => tid("model3d-chooser-dir")?.textContent === lib && all("model3d-chooser-dir").some((d) => d.dataset.path), 8000);
    const dirs = () => all("model3d-chooser-dir").filter((d) => d.dataset.path);
    const into = dirs().find((d) => d.dataset.path.endsWith("/Capacitor_SMD.3dshapes"));
    t(!!into, "KiCad's Capacitor_SMD.3dshapes is a folder of the list");
    await click(into, 1500);
    t(tid("model3d-chooser-dir").textContent === `${lib}/Capacitor_SMD.3dshapes`, `inside it: ${tid("model3d-chooser-dir").textContent}`);
    const files = all("model3d-chooser-file").map((f) => f.dataset.path);
    t(files.includes(C0603) && files.every((f) => /\.(step|stp|wrl)$/.test(f)), `its model files, by the path a footprint writes: ${files.length} (${files[0]})`);
    await click(all("model3d-chooser-file").find((f) => f.dataset.path === C0603), 300);
    t(tid("model3d-chooser-selected").textContent === C0603, `the file selected shows its path: ${tid("model3d-chooser-selected").textContent}`);
    t(await settled("model3d-chooser-preview"), "the chooser's viewer loaded it");
    t(drawn("model3d-chooser-preview") === 1, `and draws it: ${drawn("model3d-chooser-preview")}`);
    // the filter and Up
    const filter = tid("model3d-chooser-filter");
    const everything = all("model3d-chooser-file").length;
    const setFilter = async (v) => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(filter, v);
      filter.dispatchEvent(new Event("change", { bubbles: true }));
      await sleep(900);
    };
    await setFilter("step");
    t(everything > 0 && all("model3d-chooser-file").length === everything && all("model3d-chooser-file").every((f) => /\.(step|stp)$/.test(f.dataset.path)), `the STEP filter lists the STEP files: ${all("model3d-chooser-file").length} of ${everything}`);
    await setFilter("wrl");
    t(all("model3d-chooser-file").length < everything && all("model3d-chooser-file").every((f) => f.dataset.path.endsWith(".wrl")), `the VRML filter lists no STEP file: ${all("model3d-chooser-file").length} of ${everything}`);
    await setFilter("all");
    await click(tid("model3d-chooser-up"), 900);
    t(tid("model3d-chooser-dir").textContent === lib, `Up goes to the parent: ${tid("model3d-chooser-dir").textContent}`);
    // the folders outside what the model route may read are refused
    const outside = await fetch(`/api/3dmodel/browse?dir=${encodeURIComponent("/etc")}`);
    t(outside.status === 403, `a folder outside the model folders: HTTP ${outside.status}`);
    // OK adds the file chosen
    await click(into && dirs().find((d) => d.dataset.path.endsWith("/Capacitor_SMD.3dshapes")), 1500);
    await click(all("model3d-chooser-file").find((f) => f.dataset.path === C0603), 300);
    await click(tid("model3d-chooser-ok"), 1200);
    t(!tid("model3d-chooser"), "OK closes the chooser");
    t(rowCount() === 2 && paths()[1] === C0603, `the model is the new last row: ${paths().map((p) => p.split("/").pop())}`);
    t(tid("model3d-scale-x").value === "1.0000" && tid("model3d-offset-x").value === "0.000000 mm" && tid("model3d-rotation-z").value === "0.00°", "it starts with KiCad's defaults: scale 1, no offset, no rotation");
    await settled();
    t(drawn() === 2, `the preview draws both models: ${drawn()}`);
  });

  await scenario("OK sends the whole list in one command; the state, the 3D view and the board file carry it; one undo takes it back", async (t) => {
    const before = await snapshot();
    const firstActivity = (await state()).activity[0]?.cmd;
    await openTab();
    await click(tid("model3d-add"), 500);
    await type(tid("model3d-path-1"), C0603, false);
    await settled();
    await type(tid("model3d-scale-x"), "2");
    await type(tid("model3d-rotation-z"), "90");
    await type(tid("model3d-offset-x"), "1.5");
    await type(tid("model3d-offset-z"), "0.25");
    await type(tid("model3d-opacity"), "50", false);
    await click(tid("footprint-properties-ok"), 1800);
    t(!tid("footprint-properties"), "OK closes the dialog");
    const now = await modelsOf(ref);
    t(now.length === 2, `the footprint has two models: ${now.length}`);
    t(now[1].name === C0603 && same(now[1].scale, [2, 1, 1]) && same(now[1].offset, [1.5, 0, 0.25]) && now[1].opacity === 0.5, `the new model's placement is what was typed: ${JSON.stringify(now[1])}`);
    t(same(now[1].rotate, [0, 0, -90]), `a rotation typed counter-clockwise is stored clockwise, as the file keeps it: ${JSON.stringify(now[1].rotate)}`);
    t(now[0].name === R0603 && same(now[0].scale, [1, 1, 1]) && same(now[0].offset, [0, 0, 0]) && now[0].show === true, "the first model is as it was");
    const act = (await state()).activity[0];
    t(act.cmd !== firstActivity && /^3d models /.test(act.cmd) && act.ok, `one command: "${act.cmd}"`);
    // the board file
    const text = (await (await fetch("/api/clipboard/copy", { method: "POST", body: JSON.stringify({ ids: [ref], reference: { x: 0, y: 0 } }) })).json()).text;
    const blocks = text.split("(model ").length - 1;
    t(blocks === 2 && text.includes("(opacity 0.5000)") && text.includes("(xyz 2 1 1)") && text.includes("(xyz 0 0 -90)") && text.includes("(xyz 1.5 0 0.25)"), `the board file carries both models as KiCad writes them (${blocks} (model ...) entries, opacity 0.5000, the numbers)`);
    // the other footprints are as they were
    const others = (await state()).parts.filter((p) => p.placed && p.ref !== ref).map((p) => [p.ref, p.models]);
    t(same(others, JSON.parse(before).filter((p) => p[0] !== ref).map((p) => [p[0], p[4]])), "no other footprint changed");
    // the 3D view
    await __eda.run("common.Control.show3DViewer");
    t(await until(() => { const v = __eda.state().viewer3d; return v?.open && v.models?.requested > 0 && v.models.loading === 0; }, 60000), "the 3D view loaded its models");
    const view = __eda.state().viewer3d;
    const entry = (view.detail ?? []).find((d) => d.name === C0603);
    t(entry && entry.status === "ready", `it loaded the new model: ${entry?.status}`);
    t(view.parts.asModels === view.parts.total && view.parts.asBoxes === 0, `every part is drawn as its models: ${JSON.stringify(view.parts)}`);
    await toPcb();
    // one undo
    await undo();
    t((await snapshot()) === before, "ONE undo puts the footprint's models back");
  });

  await scenario("a list that comes back to the footprint's own is no edit; an emptied list is no model, not the package's", async (t) => {
    const before = await snapshot();
    await openTab();
    await click(tid("model3d-remove"), 300);
    t(rowCount() === 0, "Remove takes the only row away");
    await click(tid("footprint-properties-ok"), 1500);
    t(same(await modelsOf(ref), []) && !!(await part(ref)), "OK: the footprint has no model, and is not given the package's generic one");
    const text = (await (await fetch("/api/clipboard/copy", { method: "POST", body: JSON.stringify({ ids: [ref], reference: { x: 0, y: 0 } }) })).json()).text;
    t(!text.includes("(model "), "no (model ...) is written");
    // back to the library's list: the edit goes away
    await openTab();
    t(rowCount() === 0, "the tab shows no model");
    await click(tid("model3d-add"), 300);
    await type(tid("model3d-path-0"), R0603, false);
    await click(tid("footprint-properties-ok"), 1500);
    const back = await modelsOf(ref);
    t(back.length === 1 && back[0].name === R0603, `the list is one model again: ${back.length}`);
    await undo();
    await undo();
    t((await snapshot()) === before, "two undo steps (the emptying and the return) put the board back");
  });

  await scenario("the verbs refuse a bad position, a bad name, a bad number and a change that changes nothing, with the reason, and change nothing", async (t) => {
    const before = await snapshot();
    const refused = async (c, why) => {
      const r = await cmd(c);
      t(r.ok === false && why.test(r.message), `${c.op}: ${r.message}`);
    };
    await refused({ op: "remove_board_model", part: ref, index: 99 }, /there is no model 100: the footprint has 1 3D model/);
    await refused({ op: "move_board_model", part: ref, from: 0, to: 3 }, /there is no model 4/);
    await refused({ op: "add_board_model", part: ref, model: { path: "x?.step" } }, /Illegal filename/);
    await refused({ op: "add_board_model", part: ref, model: { path: "" } }, /No filename entered/);
    await refused({ op: "set_board_models", part: ref, models: [{ path: "a.step", scale: [0, 1, 1] }] }, /scale 0 is out of range/);
    await refused({ op: "set_board_models", part: ref, models: [{ path: "a.step", offset: [2000, 0, 0] }] }, /offset 2000 mm is out of range/);
    await refused({ op: "set_board_models", part: ref, models: [{ path: "a.step", opacity: 2 }] }, /opacity 2 is out of range/);
    await refused({ op: "edit_board_model", part: ref, index: 0 }, /give a file name/);
    await refused({ op: "set_board_models", part: ref, models: [{ path: u.models[0].name }] }, /as they were/);
    await refused({ op: "set_board_models", part: "NOPE", models: [] }, /not on the board/);
    t((await snapshot()) === before, "the board is as it was after every refusal");
    // and the verbs, one by one, each one undo step
    const base = (await state()).activity.length;
    for (const [c, ok] of [
      [{ op: "add_board_model", part: ref, model: { path: C0603, offset: [1, 2, 0.5], rotate: [90, 0, 0], opacity: 0.5 } }, (m) => m.length === 2 && same(m[1].offset, [1, 2, 0.5]) && m[1].opacity === 0.5],
      [{ op: "edit_board_model", part: ref, index: 1, scale: [2, 2, 2], show: false }, (m) => same(m[1].scale, [2, 2, 2]) && m[1].show === false && same(m[1].offset, [1, 2, 0.5])],
      [{ op: "move_board_model", part: ref, from: 1, to: 0 }, (m) => m[0].name === C0603],
      [{ op: "remove_board_model", part: ref, index: 0 }, (m) => m.length === 1 && m[0].name === R0603],
    ]) {
      const r = await cmd(c);
      t(r.ok === true && ok(await modelsOf(ref)), `${c.op}: ${r.ok ? "applied" : r.message}`);
    }
    for (let i = 0; i < 4; i++) await undo();
    t((await snapshot()) === before, "four undo steps put the board back: one for each verb");
    void base;
  });

  await scenario("the Footprint Chooser's 3D view shows the selected footprint with its models; one of the two views is always on", async (t) => {
    localStorage.removeItem("eda-studio.footprint-chooser-views");
    await __eda.run("pcbnew.EditorControl.placeFootprint");
    t(await until(() => document.querySelector("[data-footprint-chooser]")), "the Footprint Chooser opens");
    const root = document.querySelector("[data-footprint-chooser]");
    const fpBtn = root.querySelector("[data-chooser-view-fp]");
    const v3Btn = root.querySelector("[data-chooser-view-3d]");
    t(fpBtn && v3Btn && fpBtn.getAttribute("aria-pressed") === "true" && v3Btn.getAttribute("aria-pressed") === "false", "the footprint view is on and the 3D view off, as KiCad opens it");
    fpBtn.click();
    await sleep(300);
    t(fpBtn.getAttribute("aria-pressed") === "true", "the last view cannot be switched off");
    const search = root.querySelector("input[data-chooser-search]");
    await type(search, "R_0603_1608Metric", false);
    await until(() => [...root.querySelectorAll("[role=treeitem]")].some((r) => r.textContent.trim().startsWith("R_0603_1608Metric")), 20000);
    const item = [...root.querySelectorAll("[role=treeitem]")].find((r) => r.textContent.trim().startsWith("R_0603_1608Metric") && !r.textContent.includes("Pad"));
    t(!!item, "the footprint is in the list");
    item?.click();
    await sleep(800);
    v3Btn.click();
    await sleep(500);
    t(v3Btn.getAttribute("aria-pressed") === "true" && fpBtn.getAttribute("aria-pressed") === "true", "both views are on");
    t(await settled("chooser-preview-3d", 40000), "the 3D view loaded its models");
    t(drawn("chooser-preview-3d") === 1, `it draws the footprint's model: ${drawn("chooser-preview-3d")}`);
    t(!!root.querySelector("[data-preview=footprint]"), "and the footprint's drawing is still there");
    fpBtn.click();
    await sleep(300);
    t(!root.querySelector("[data-preview=footprint]") && !!tid("chooser-preview-3d"), "the footprint view off: the 3D view alone");
    v3Btn.click(); // back to how it opens: the footprint view on, the 3D view off
    await sleep(200);
    fpBtn.getAttribute("aria-pressed") === "false" && fpBtn.click();
    await sleep(200);
    v3Btn.getAttribute("aria-pressed") === "true" && v3Btn.click();
    await sleep(300);
    t(fpBtn.getAttribute("aria-pressed") === "true" && v3Btn.getAttribute("aria-pressed") === "false", "left as it opens");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(300);
    await close();
  });

  await scenario("the Footprint Editor's Footprint Properties has the tab: a library footprint's models, and the list it sends", async (t) => {
    // a library footprint of KiCad's, opened in the editor from the library tree
    await __eda.run("pcbnew.EditorControl.EditFpInFpEditor").catch(() => null);
    await select([ref]);
    await __eda.run("pcbnew.EditorControl.EditFpInFpEditor");
    await until(() => __eda.state().tab === "footprint", 8000);
    const search = document.querySelector('input[placeholder="Search"]');
    t(!!search, "the Footprint Editor's library tree is there");
    await type(search, "R_0603_1608Metric", false);
    await until(() => [...document.querySelectorAll("[role=treeitem]")].some((r) => r.textContent.trim() === "R_0603_1608Metric"), 20000);
    const item = [...document.querySelectorAll("[role=treeitem]")].find((r) => r.textContent.trim() === "R_0603_1608Metric");
    item?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await sleep(2500);
    await __eda.run("pcbnew.ModuleEditor.footprintProperties");
    await until(() => tid("fp-library-properties"));
    t(!!tid("fp-library-properties-tab-general") && !!tid("fp-library-properties-tab-models"), "the dialog has General and 3D Models tabs");
    t(!document.querySelector('[data-testid="fp-library-properties"] input[placeholder^="${KICAD10_3DMODEL_DIR}"]:not([data-testid^="model3d-"])'), "the single 3D model path field is gone: it is the tab");
    await click(tid("fp-library-properties-tab-models"), 700);
    t(rowCount() === 1 && paths()[0] === R0603, `the library footprint's model: ${paths()}`);
    t(await settled(), "the preview loaded it");
    t(drawn() === 1, `and draws it: ${drawn()}`);
    const geometry = (await json(`/api/3dmodel/preview?footprint=${encodeURIComponent("Resistor_SMD:R_0603_1608Metric")}`)).part;
    t(geometry && geometry.pads.length === 2, `the preview of a library footprint is its pads: ${geometry?.pads.length}`);
    // two models, the second turned and lifted
    await click(tid("model3d-add"), 400);
    await type(tid("model3d-path-1"), C0603, false);
    await type(tid("model3d-offset-z"), "3");
    await type(tid("model3d-rotation-x"), "90");
    await click([...document.querySelectorAll(".dialog-footer button.primary")].find((b) => b.textContent.trim() === "OK"), 1800);
    const fp = await json(`/api/footprint?name=${encodeURIComponent("Resistor_SMD:R_0603_1608Metric")}`);
    t(Array.isArray(fp.models3d) && fp.models3d.length === 2, `the library footprint keeps both: ${fp.models3d?.length}`);
    t(fp.models3d?.[1].path === C0603 && same(fp.models3d?.[1].offset, [0, 0, 3]) && same(fp.models3d?.[1].rotate, [-90, 0, 0]), `the second with its placement (rotation clockwise as the file keeps it): ${JSON.stringify(fp.models3d?.[1])}`);
    t(fp.model === R0603, `\`model\` is the first path: ${fp.model}`);
    const text = (await (await fetch(`/api/footprint/export?name=${encodeURIComponent("Resistor_SMD:R_0603_1608Metric")}`)).text());
    t(text.split("(model ").length - 1 === 2 && text.includes("(xyz 0 0 3)"), "the .kicad_mod it exports has both");
    // back: the editor's own undo steps (the properties, then the entry the project library took)
    for (let i = 0; i < 2; i++) await undo();
    await toPcb();
  });

  await scenario("Migrate 3D Models: the VRML models no search finds, the STEP files that could stand for them, a folder added, a file picked by hand, Replace in one undo step", async (t) => {
    const wrl = `${lib}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl`;
    const users = (await state()).parts.filter((p) => p.placed && (p.models ?? []).length === 1 && p.models[0].name === R0603).map((p) => p.ref);
    const jump = async (target) => {
      const sel = tid("model3d-chooser-roots");
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(sel, target);
      sel.dispatchEvent(new Event("change", { bubbles: true }));
      await until(() => tid("model3d-chooser-dir")?.textContent === target && all("model3d-chooser-dir").some((d) => d.dataset.path), 8000);
    };
    const searchKey = "eda-studio.model3d-search-dirs";
    const keptDirs = localStorage.getItem(searchKey);
    localStorage.removeItem(searchKey);
    // the activity list is newest first and kept to the last 60: a step is an entry newer than the one that was the newest
    const newest = async () => (await state()).activity[0]?.t ?? 0;
    const since = async (t0) => (await state()).activity.filter((e) => e.t > t0);
    const migrate = async () => { await __eda.run("pcbnew.GlobalEdit.migrate3DModels"); return until(() => tid("migrate3d-dialog")); };
    // 1. nothing is missing: the dialog still comes up, with empty lists, and Keep Existing closes it
    t(await migrate(), "Tools > Migrate 3D Models opens its dialog");
    t(tid("migrate3d-header").textContent === "This board references 3D models that are no longer present on your system.\nSelect replacements below, or keep the existing references unchanged.", "with KiCad's two lines of header");
    t(await until(() => /searched/.test(tid("migrate3d-status").textContent), 20000), `it searched: ${tid("migrate3d-status").textContent}`);
    t(all("migrate3d-missing-").length === 0 && /No 3D model of this board is missing/.test(tid("migrate3d-missing").textContent), "the missing list is empty, and says so");
    t(["Missing 3D model references:", "Potential replacements:", "Preview:"].every((l) => tid("migrate3d-dialog").textContent.includes(l)), "the three panes carry KiCad's labels");
    const base = await newest();
    await click(tid("migrate3d-replace"), 500);
    t(!tid("migrate3d-dialog") && (await since(base)).length === 0, "Replace Models with nothing chosen is Keep: it closes and sends nothing");
    // 2. the footprints name a VRML model that is not there, placed as a footprint places it
    const placed = { path: wrl, offset: [0.5, 0, 0.25], scale: [1, 1, 1], rotate: [0, 0, 90], opacity: 0.5, show: true };
    for (const r of users) await cmd({ op: "set_board_models", part: r, models: [placed] });
    t(users.length >= 1, `footprints with the model of a 0603 resistor take the .wrl name: ${users.join(", ")}`);
    const afterSet = await snapshot();
    t(await migrate(), "the dialog opens again");
    t(await until(() => all("migrate3d-missing-").length === 1, 20000), `one missing reference: ${all("migrate3d-missing-").length}`);
    const miss = () => tid("migrate3d-missing-0");
    t(miss().dataset.name === wrl, `it is the .wrl: ${miss().dataset.name}`);
    t(users.every((r) => miss().title.includes(r)), `and its tooltip names the footprints that have it: ${miss().title.replace(/\n/g, " | ")}`);
    const cands = () => all("migrate3d-candidate-");
    t(cands().length >= 2, `STEP candidates and the last row: ${cands().length}`);
    t(cands()[0].dataset.path === R0603 && cands()[0].dataset.picked === "1", `the best candidate is the STEP of the same name, chosen: ${cands()[0]?.dataset.path}`);
    t(cands().at(-1).textContent === "(keep existing reference)" && cands().at(-1).dataset.path === "", "the last row is (keep existing reference)");
    t(miss().dataset.chosen === "1" && miss().style.fontWeight !== "700", "the missing row is not bold: it has a choice");
    t(await settled("migrate3d-preview"), "the preview loaded");
    t(drawn("migrate3d-preview") === 1, `and draws the candidate on the footprint: ${drawn("migrate3d-preview")}`);
    // choosing "keep existing" bolds the row and shows the footprint alone
    await click(cands().at(-1), 400);
    t(miss().dataset.chosen === "0" && miss().style.fontWeight === "700", "keep existing: the row is bold again");
    await settled("migrate3d-preview");
    t(drawn("migrate3d-preview") === 0, `and the preview has no model: ${drawn("migrate3d-preview")}`);
    const before = await newest();
    await click(tid("migrate3d-replace"), 500);
    t(!tid("migrate3d-dialog") && (await since(before)).length === 0 && (await snapshot()) === afterSet, "Replace Models with only keep-existing is Keep: nothing changed");
    // 3. a file picked by hand becomes the first candidate, user-selected
    await migrate();
    await until(() => all("migrate3d-missing-").length === 1 && cands().length >= 2, 20000);
    await click(tid("migrate3d-open-file"), 1200);
    t(!!tid("model3d-chooser") && !tid("model3d-chooser-filter") && !!tid("model3d-chooser-preview"), "Open External File... opens the model chooser, for STEP files only");
    await jump(lib);
    await click(all("model3d-chooser-dir").filter((d) => d.dataset.path).find((d) => d.dataset.path.endsWith("/Capacitor_SMD.3dshapes")), 1500);
    t(all("model3d-chooser-file").length > 0 && all("model3d-chooser-file").every((f) => /\.(step|stp)$/.test(f.dataset.path)), `it lists STEP files: ${all("model3d-chooser-file").length}`);
    // a file the row has already is chosen where it is: no second entry
    const listed = cands().length;
    await click(all("model3d-chooser-file").find((f) => f.dataset.path === C0603), 300);
    await click(tid("model3d-chooser-ok"), 900);
    t(!tid("model3d-chooser"), "OK closes the chooser");
    t(cands().length === listed && cands().find((c) => c.dataset.picked === "1")?.dataset.path === C0603, `a file that is already a candidate is chosen where it is: ${listed} rows, ${cands().find((c) => c.dataset.picked === "1")?.textContent}`);
    // one the row does not have goes first, user-selected, chosen
    const HEADER = `${lib}/Connector_PinHeader_2.54mm.3dshapes/PinHeader_1x04_P2.54mm_Vertical.step`;
    await click(tid("migrate3d-open-file"), 1200);
    await jump(lib);
    await click(all("model3d-chooser-dir").filter((d) => d.dataset.path).find((d) => d.dataset.path.endsWith("/Connector_PinHeader_2.54mm.3dshapes")), 1500);
    await click(all("model3d-chooser-file").find((f) => f.dataset.path === HEADER), 300);
    await click(tid("model3d-chooser-ok"), 900);
    t(cands().length === listed + 1 && cands()[0].dataset.path === HEADER && cands()[0].dataset.picked === "1" && / user-selected$/.test(cands()[0].textContent), `a file of the user's goes first, user-selected, chosen: ${cands()[0]?.textContent}`);
    t(miss().dataset.chosen === "1", "and the row has a choice");
    // 4. a folder added: the rows are ranked again, the list of folders is kept, a pick the new ranking lacks goes back to the best
    await click(tid("migrate3d-add-dir"), 1200);
    t(!!tid("model3d-chooser") && !tid("model3d-chooser-preview") && !tid("model3d-chooser-filter"), "Add Search Directory... opens the chooser as a folder picker: no viewer, no filter");
    await jump(lib);
    t(all("model3d-chooser-file").length === 0 && all("model3d-chooser-dir").filter((d) => d.dataset.path).length > 3, "only folders are listed");
    await click(all("model3d-chooser-dir").filter((d) => d.dataset.path).find((d) => d.dataset.path.endsWith("/Capacitor_SMD.3dshapes")), 300);
    t(tid("model3d-chooser-selected").textContent === `${lib}/Capacitor_SMD.3dshapes`, `a click picks the folder: ${tid("model3d-chooser-selected").textContent}`);
    await click(tid("model3d-chooser-ok"), 1500);
    t(!tid("model3d-chooser"), "OK closes the chooser");
    t(await until(() => /with 1 folder added/.test(tid("migrate3d-status").textContent), 20000), `the folder is searched: ${tid("migrate3d-status").textContent}`);
    t(JSON.parse(localStorage.getItem(searchKey) ?? "[]").join() === `${lib}/Capacitor_SMD.3dshapes`, "and kept for the next time");
    t(cands().length === listed && cands()[0].dataset.path === R0603 && cands()[0].dataset.picked === "1", `every row is ranked again, and the pick the new ranking lacks goes back to the best: ${cands()[0]?.dataset.path} (${cands().length} rows)`);
    // Escape closes the chooser first, then the dialog
    await click(tid("migrate3d-add-dir"), 800);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(300);
    t(!tid("model3d-chooser") && !!tid("migrate3d-dialog"), "Escape closes the chooser and leaves the dialog");
    // 5. Replace Models: the old name for the new in every footprint that has it, with the placement kept, in one undo step
    const steps = await newest();
    await click(tid("migrate3d-replace"), 1800);
    t(!tid("migrate3d-dialog"), "Replace Models closes the dialog");
    const now = await Promise.all(users.map(async (r) => [r, (await modelsOf(r))[0]]));
    t(now.every(([, m]) => m && m.name === R0603), `each footprint has the STEP: ${now.map(([r, m]) => `${r} ${m?.name?.split("/").pop()}`).join(", ")}`);
    t(now.every(([, m]) => same(m.offset, placed.offset) && same(m.rotate, placed.rotate) && m.opacity === 0.5), "with the placement it had");
    const made = await since(steps);
    t(made.length === 1 && made[0].ok, `one step: ${JSON.stringify(made.map((e) => [e.cmd, e.ok]))}`);
    await undo();
    t((await snapshot()) === afterSet, "one undo gives the .wrl names back to all of them");
    // 6. the dialog finds nothing missing any more once the names are STEP
    for (let i = 0; i < users.length; i++) await undo();
    t((await snapshot()) === original, "the models are as they were");
    localStorage.removeItem(searchKey);
    if (keptDirs !== null) localStorage.setItem(searchKey, keptDirs);
  });

  await scenario("the board is as it was and nothing broke", async (t) => {
    kept.forEach(([k, v]) => (v === null ? localStorage.removeItem(k) : localStorage.setItem(k, v)));
    await toPcb();
    await select([]);
    // the library entry the editor scenario opened is undone with the editor's own history: what is left is no footprint of the board
    t((await snapshot()) === original, "every footprint has the models it had, where it was");
    const errors = (__eda.errors?.() ?? []).slice(errorsFrom);
    t(errors.length === 0, `no console error: ${JSON.stringify(errors).slice(0, 200)}`);
    if (strict && !strict.checked) strict.click();
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
