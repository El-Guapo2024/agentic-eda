// "Show parts without 3D model" (UI-LEFT-3 X3-2, task-v19-microvia): KiCad 25ec2335's 3D viewer draws a footprint without its 3D models as nothing, or -- with the
// setting on (`render.show_missing_models`, off by default; `EDA_3D_ACTIONS::toggleShowMissingModels`) -- as an orange see-through box on its pads
// (`renderPlaceholderForFootprint`). On the ldo example with J1 placed: J1's models are taken away (`set_board_models`, undone at the end); the 3D view then draws J1 as
// nothing; Preferences > 3D Viewer > General's "Show parts without 3D model" (KiCad's Preferences menu entry: the studio's 3D tab has no menu bar) gives it its
// placeholder, kept as the person's setting; off again, nothing again.
// Returns { ok, results: [{ name, ok, detail? }, ...] }.
// run-all-checks: board=ldo-j1 minutes=5
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const results = [];
  const check = (name, ok, detail) => results.push({ name, ok: !!ok, ...(detail !== undefined ? { detail } : {}) });
  const until = async (test, ms = 30000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await test()) return true; await sleep(150); } return false; };
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const view = () => window.__eda.state().viewer3d;
  const box = (label) => [...document.querySelectorAll(".dialog input[type=checkbox]")].find((i) => i.getAttribute("aria-label") === label);
  const button = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const stored = () => { try { return JSON.parse(localStorage.getItem("eda-studio.viewer3d") ?? "{}").prefs ?? {}; } catch { return {}; } };
  const setPref = async (on) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => document.querySelector('[data-page="3d_general"]'), 5000);
    document.querySelector('[data-page="3d_general"]').click();
    await sleep(400);
    const b = box("Show parts without 3D model");
    if (b && b.checked !== on) b.click();
    await sleep(150);
    button(document.querySelector(".dialog"), "OK")?.click();
    await sleep(800);
    return !!b;
  };

  const st0 = await fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  const j1 = st0.parts.find((p) => p.ref === "J1");
  check("J1 is placed and has models", !!j1?.placed && (j1.models ?? []).length > 0, j1?.models);
  const reply = await post("/api/cmd", { cmd: { op: "set_board_models", part: "J1", models: [] }, strict: false });
  check("its models taken away", reply.ok, reply.message);
  await sleep(800);

  await __eda.run("common.Control.show3DViewer");
  check("the 3D tab opens", await until(() => view()?.open, 30000));
  check("off by default (KiCad's)", stored().showMissingModels !== true, stored());
  check("J1 is drawn as nothing", await until(() => view()?.parts?.total >= 1 && view().parts.asNothing >= 1 && view().parts.asBoxes === 0, 20000), view()?.parts);

  check("Preferences > 3D Viewer > General has the row", await setPref(true));
  check("on, J1 is drawn as its placeholder", await until(() => view()?.parts?.asBoxes >= 1 && (view().parts.asNothing ?? 0) === 0, 20000), view()?.parts);
  check("the setting is kept (the person's)", stored().showMissingModels === true, stored());

  await setPref(false);
  check("off again, nothing again", await until(() => view()?.parts?.asBoxes === 0 && view().parts.asNothing >= 1, 20000), view()?.parts);

  await post("/api/undo", { domain: "pcb" });
  await sleep(800);
  const st1 = await fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  check("undo gives J1 its models back", (st1.parts.find((p) => p.ref === "J1")?.models ?? []).length === (j1?.models ?? []).length);
  check("no error was logged", window.__eda.errors().length === 0, window.__eda.errors().slice(0, 3));
  const ok = results.every((r) => r.ok);
  return { ok, results };
})();
