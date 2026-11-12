// The 3D viewer's Render Options and While Moving preferences and the Extruded 3D Body box's preview, Export... and Footprint Editor copy (UI-LEFT-2 DD2, DD3; PARITY-3d
// section 10): Preferences > 3D Viewer > General has KiCad's Render Options rows (`panel_3D_display_options_base.cpp`) and Realtime Renderer its While Moving rows
// (`panel_3D_opengl_options_base.cpp`), with KiCad's defaults, kept on OK; the board's Footprint Properties 3D Models tab draws the box's body in its preview as the box
// stands (`updateExtrusionPreview`), and Export... writes it (`OnExportExtrudedModel`, through kicad-cli: `POST /api/extruded_body/export`) as a file the browser saves, refusing
// a height of zero in KiCad's words; the Footprint Editor's Footprint Properties has the box, and its OK sets the library footprint's body (`set_library_extruded_body`).
//
// Run it in the page of a served board with J1 a placed pin header (the ldo example with J1 placed), on a COPY: it edits J1 and a library footprint and leaves the edits.
// Needs kicad-cli and KiCad's footprint library (`KICAD_PATH`). Returns { ok, results: [{ name, ok, detail? }, ...] }.
// run-all-checks: board=ldo-j1 minutes=6
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const json = (u) => fetch(u, { cache: "no-store" }).then((r) => r.json());
  const until = async (test, ms = 30000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await test()) return true; await sleep(150); } return false; };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const click = async (el, ms = 400) => { el?.click(); await sleep(ms); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(200); };
  const pick = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(200); };
  const box = (label) => [...document.querySelectorAll(".dialog input[type=checkbox]")].find((i) => i.getAttribute("aria-label") === label);
  const button = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const toPcb = async () => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(800); };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const stored = () => { try { return JSON.parse(localStorage.getItem("eda-studio.viewer3d") ?? "{}").prefs ?? {}; } catch { return {}; } };

  // ------------------------------------------------------------------------------------------------------------------- Preferences
  await __eda.run("common.SuiteControl.openPreferences");
  await until(() => document.querySelector('[data-page="3d_general"]'), 5000);
  await click(document.querySelector('[data-page="3d_general"]'), 400);
  const general = document.querySelector('[data-page-body="3d_general"]');
  const labels = [...(general?.querySelectorAll("label, span, div") ?? [])].map((e) => e.childNodes.length && e.textContent.trim());
  check("General has KiCad's Render Options heading and rows", ["Render Options", "Clip silkscreen at via annuli", "Clip silkscreen at solder mask edges", "Use bare copper color for unplated copper (slow)", "Material properties:"].every((l) => labels.includes(l)), labels.filter(Boolean).slice(0, 12));
  check("their defaults are KiCad's: all off, Realistic", ["Clip silkscreen at via annuli", "Clip silkscreen at solder mask edges", "Use bare copper color for unplated copper"].every((l) => box(l)?.checked === false) && document.querySelector('select[aria-label="Material properties"]')?.value === "realistic");
  check("the material choices are KiCad's", [...(document.querySelector('select[aria-label="Material properties"]')?.options ?? [])].map((o) => o.textContent).join("|") === "Realistic|Solid colors|CAD colors");
  await click(box("Clip silkscreen at solder mask edges"), 150);
  await pick(document.querySelector('select[aria-label="Material properties"]'), "cad");
  await click(document.querySelector('[data-page="3d_opengl"]'), 400);
  const opengl = document.querySelector('[data-page-body="3d_opengl"]');
  check("Realtime Renderer has While Moving with its four rows", !!opengl && opengl.textContent.includes("While Moving") && ["Disable anti-aliasing", "Disable thickness", "Disable uVia holes", "Disable all plated holes"].every((l) => box(l)?.checked === false));
  await click(box("Disable all plated holes"), 150);
  await click(button(document.querySelector(".dialog"), "OK"), 600);
  const p = stored();
  check("OK keeps them (the person's settings)", p.subtractMaskFromSilk === true && p.materialMode === "cad" && p.holesDisableOnMove === true && p.clipSilkOnViaAnnuli === false, p);
  // back to KiCad's defaults
  await __eda.run("common.SuiteControl.openPreferences");
  await until(() => document.querySelector('[data-page="3d_general"]'), 5000);
  await click(document.querySelector('[data-page="3d_general"]'), 300);
  await click(button(document.querySelector('[data-page-body="3d_general"]'), "Reset to Defaults"), 200);
  await click(document.querySelector('[data-page="3d_opengl"]'), 300);
  await click(button(document.querySelector('[data-page-body="3d_opengl"]'), "Reset to Defaults"), 200);
  await click(button(document.querySelector(".dialog"), "OK"), 600);
  const q = stored();
  check("Reset to Defaults on both pages puts KiCad's back", q.subtractMaskFromSilk === false && q.materialMode === "realistic" && q.holesDisableOnMove === false, q);

  // ------------------------------------------------------------------------------------------------------- the board's box: preview and Export...
  await toPcb();
  await select(["J1"]);
  await __eda.run("pcbnew.InteractiveEdit.properties");
  await until(() => tid("footprint-properties"), 10000);
  await click(tid("footprint-properties-tab-models"), 900);
  const preview = () => tid("model-preview-panel")?.querySelector("[data-drawn]");
  if (!tid("extruded-enable")?.checked) await click(tid("extruded-enable"), 300);
  await type(tid("extruded-height"), "5");
  await type(tid("extruded-standoff"), "1");
  await pick(tid("extruded-source"), "4");
  const drawn = await until(() => Number(preview()?.dataset.extruded ?? 0) === 2, 10000);
  check("the preview draws the box's body and its pins as the box stands", drawn, preview()?.dataset.extruded);
  const info = window.__edaExtrudedBodies?.get("preview:J1");
  check("from the board to the height typed (5 mm over the preview board's top)", info && Math.abs(info.high - info.low - 4) < 1e-6, info);
  await click(tid("extruded-show"), 300);
  check("Show off hides it in the preview (`m_show = enabled && show`)", await until(() => preview()?.dataset.extruded === "0", 5000));
  await click(tid("extruded-show"), 300);
  // Export...: a height of zero is refused in KiCad's words, before the file dialog
  await type(tid("extruded-height"), "0");
  await click(tid("extruded-export"), 400);
  check("a height of zero is refused as OnExportExtrudedModel refuses it", globalThis.__edaLastMessageBox?.title === "Export Extruded Body" && globalThis.__edaLastMessageBox?.message === "Component height must be greater than zero." && !tid("extruded-export-dialog"), globalThis.__edaLastMessageBox);
  check("in KiCad's message box", tid("message-box-text")?.textContent === "Component height must be greater than zero.");
  await click(tid("message-box-ok"), 300);
  await type(tid("extruded-height"), "5");
  await click(tid("extruded-export"), 400);
  check("the file dialog offers <REF>_extruded and KiCad's four types", tid("extruded-export-name")?.value === "J1_extruded" && [...tid("extruded-export-type").options].map((o) => o.textContent).join("|") === "STEP files (*.step)|GLB files (*.glb)|STL files (*.stl)|BREP files (*.brep)");
  const saved = [];
  const realCreate = URL.createObjectURL;
  URL.createObjectURL = (blob) => { saved.push(blob); return realCreate.call(URL, blob); };
  const realClick = HTMLAnchorElement.prototype.click;
  const names = [];
  HTMLAnchorElement.prototype.click = function () { names.push(this.download); };
  await pick(tid("extruded-export-type"), "2");
  await click(tid("extruded-export-save"), 300);
  await until(() => names.length > 0, 120000);
  URL.createObjectURL = realCreate;
  HTMLAnchorElement.prototype.click = realClick;
  const text = saved[0] ? await saved[0].text() : "";
  check("Save writes the body as an STL the browser saves (kicad-cli's STEP_PCB_MODEL)", names[0] === "J1_extruded.stl" && saved[0]?.size > 1000 && /^solid|facet normal/m.test(text.slice(0, 2000)), { names, size: saved[0]?.size });
  await click([...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Cancel"), 500);

  // ----------------------------------------------------------------------------------------------------- the Footprint Editor's box
  const NAME = "Resistor_SMD:R_0603_1608Metric";
  await toPcb();
  await select(["J1"]);
  await __eda.run("pcbnew.EditorControl.EditFpInFpEditor");
  await until(() => __eda.state().tab === "footprint", 8000);
  const search = document.querySelector('input[placeholder="Search"]');
  if (search) await type(search, "R_0603_1608Metric");
  const row = () => document.querySelector(`[role=treeitem][data-name="${NAME}"]`);
  await until(() => !!row(), 20000);
  row()?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  await sleep(2500);
  await __eda.run("pcbnew.ModuleEditor.footprintProperties");
  await until(() => tid("fp-library-properties"), 8000);
  await click(tid("fp-library-properties-tab-models"), 900);
  check("the Footprint Editor's 3D Models tab has the box, off for a footprint with no body", !!tid("extruded-body") && tid("extruded-enable")?.checked === false);
  check("its Export... is greyed (a library footprint is on no board kicad-cli could export)", tid("extruded-export")?.disabled === true);
  await click(tid("extruded-enable"), 300);
  await type(tid("extruded-height"), "1.2");
  await pick(tid("extruded-material"), "3");
  check("the preview draws the body there too", await until(() => Number(tid("model-preview-panel")?.querySelector("[data-drawn]")?.dataset.extruded ?? 0) >= 1, 10000));
  await click([...document.querySelectorAll(".dialog-footer button.primary")].find((b) => b.textContent.trim() === "OK"), 1800);
  const fp = await json(`/api/footprint?name=${encodeURIComponent(NAME)}`);
  check("OK sets the library footprint's body in the same step", fp?.extruded_body?.height === 1_200_000 && fp.extruded_body.material === "copper", fp?.extruded_body);

  const ok = results.every((r) => r.ok);
  return { ok, results };
})();
