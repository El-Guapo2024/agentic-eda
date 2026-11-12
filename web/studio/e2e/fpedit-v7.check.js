// The Footprint Editor against KiCad, UI-LEFT-2 FF3, FF5 and FF8 (PARITY-fpedit.md section 26):
//   FF3  the Properties pane sets a pad's Pad Type (`PAD::SetAttribute`), Copper Layers, Backdrill Mode and Enable Teardrops, and a line's End Shape (KiCad 10's endings);
//   FF8  a double click on a group enters it (`EnterGroup`), Remove Items / Add Items (`GROUP_TOOL`), Escape leaves; Mirror leaves a point where it is with
//        `BOARD_ITEM::Mirror`'s message; Draw Dimension previews the dimension itself (`POST /api/dimension/preview`); a built-in package opened has its 3D models;
//   FF5  the Appearance manager's Presets (Front Layers hides the back) and Viewports, the Selection Filter pane gating a click and kept in the preferences.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board (`eda board new
// examples/mcu_board_30plus.yaml -o <dir>`). It makes footprints of its own and deletes them at the end; it puts the preferences back.
// It answers through `window.__fpeditV7Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__fpeditV7Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const post = (url, body) => fetch(url, { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(250); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const until = async (fn, ms = 5000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(120); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => { const f = await json(`/api/footprint?name=${encodeURIComponent(name)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [], pads: f.pads ?? [] }; };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button, button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const key = async (k) => { canvasEl()?.focus(); canvasEl()?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })); await sleep(250); };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    await key("Escape");
    await key("Escape");
    results.push(entry);
  };
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, wx, wy, buttons = 0, mods = {}) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const clickAt = async (wx, wy, mods = { ctrl: true, shift: true }) => { fire("pointermove", wx, wy, 0, mods); await sleep(70); fire("pointerdown", wx, wy, 1, mods); await sleep(40); fire("pointerup", wx, wy, 0, mods); await sleep(450); };
  const selectAt = async (wx, wy) => clickAt(wx, wy, {});
  const dblClickAt = async (wx, wy) => {
    const [x, y] = toScreen(wx, wy);
    await selectAt(wx, wy);
    canvasEl().dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, detail: 2 }));
    await sleep(500);
  };
  const selection = () => (__eda.state().selection ?? []).map((s) => s.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const MM = 1_000_000;
  const libraryBefore = await fpNames();
  const prefsBefore = localStorage.getItem("eda-studio.preferences.v1");
  const putFootprint = async (body) => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !before.includes(n));
    if (!name) return null;
    const parsed = (await post("/api/footprint/parse", `(footprint "X" (version 20260818) (generator "pcbnew") (layer "F.Cu")\n${body}\n)`)).footprint;
    if (!parsed) return null;
    const r = await cmd({ op: "put_library_footprint", footprint: { ...parsed, name }, overwrite: true });
    if (!r.ok) return null;
    await until(async () => (await footprint(name)).pads.length === parsed.pads.length || null, 4000);
    await __eda.run("common.Control.zoomFitScreen");
    await sleep(900);
    return name;
  };
  const BODY = `(attr smd)
 (fp_line (start -3 -2) (end 3 -2) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
 (pad "1" smd rect (at -1.5 0) (size 1 0.8) (layers "F.Cu" "F.Paste" "F.Mask"))
 (pad "2" thru_hole circle (at 1.5 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask"))
 (point (at 0 3) (size 1) (layer "F.Fab"))`;
  const name = await putFootprint(BODY);
  const pane = () => tid("fp-properties");
  const row = (n) => pane()?.querySelector(`.prop-row[data-prop="${n}"]`);

  // --------------------------------------------------------------------------------------------------------------------------- FF3
  await scenario("FF3: the Properties pane sets Pad Type (SetAttribute), Copper Layers, Backdrill Mode, Enable Teardrops and a line's End Shape, one undo step each", async (t) => {
    if (!t(name, `the footprint is in the project library: ${name}`)) return;
    await selectAt(-1.5 * MM, 0);
    const typeSel = await until(() => row("Pad Type")?.querySelector("select"), 3000);
    if (!t(typeSel && !row("Pad Type").classList.contains("readonly"), "Pad Type is a choice, not read-only")) return;
    await choose(typeSel, "through_hole");
    const th = await until(async () => { const f = await footprint(name); const p = f.pads.find((q) => q.number === "1"); return p?.kind === "through_hole" ? p : null; }, 4000);
    t(th && th.layers.includes("*.Cu") && (th.drill ?? 0) > 0, `the pad is plated, on every copper layer, with a hole: ${JSON.stringify(th && [th.layers, th.drill])}`);
    await undo();
    t((await footprint(name)).pads.find((q) => q.number === "1")?.kind === "smd", "one undo takes it back");
    await selectAt(1.5 * MM, 0);
    const cu = await until(() => row("Copper Layers")?.querySelector("select"), 3000);
    if (t(cu, "the plated pad has Copper Layers")) {
      await choose(cu, "remove_all");
      t(await until(async () => (await footprint(name)).pads.find((q) => q.number === "2")?.stack?.unconnected === "remove_all", 4000), "Connected layers only is set");
    }
    const bd = await until(() => row("Backdrill Mode")?.querySelector("select"), 3000);
    if (t(bd, "Backdrill Mode is a choice")) {
      await choose(bd, "bottom");
      const f = await until(async () => { const x = await footprint(name); const p = x.pads.find((q) => q.number === "2"); return x.pad_extras?.[p.id]?.drill_props?.backdrill ? x : null; }, 4000);
      t(f, `the pad's backdrill is kept with the footprint: ${JSON.stringify(f?.pad_extras)}`);
    }
    const td = await until(() => row("Enable Teardrops")?.querySelector("input[type=checkbox]"), 3000);
    if (t(td && !td.checked, "Enable Teardrops is a check box, off")) {
      td.click();
      await sleep(400);
      t(await until(async () => { const x = await footprint(name); const p = x.pads.find((q) => q.number === "2"); return x.pad_extras?.[p.id]?.teardrops?.enabled === true; }, 4000), "the pad's own teardrops are enabled");
    }
    await selectAt(0, -2 * MM);
    const end = await until(() => row("End Shape")?.querySelector("select"), 3000);
    if (t(end, "a line has End Shape")) {
      await choose(end, "1");
      const f = await until(async () => { const x = await footprint(name); return Object.values(x.kicad?.endings ?? {})[0]?.end?.style === "arrow" ? x : null; }, 4000);
      t(f, `the line ends in an arrow: ${JSON.stringify(f?.kicad?.endings)}`);
      const mod = await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`).then((r) => r.text());
      t(/\(end_shape arrow\)/.test(mod), "the .kicad_mod writes (end_shape arrow)");
      await undo();
      t(Object.keys((await footprint(name)).kicad?.endings ?? {}).length === 0, "one undo takes it back");
    }
  });

  // --------------------------------------------------------------------------------------------------------------------------- FF8
  await scenario("FF8: a double click on a group enters it; Remove Items, Add Items; Escape leaves the group", async (t) => {
    const f = await footprint(name);
    const p1 = f.pads.find((q) => q.number === "1").id;
    const line = f.graphics[0].id;
    __eda.fp?.select?.([p1, line]);
    await selectAt(-1.5 * MM, 0);
    await clickAt(0, -2 * MM, { shift: true });
    t(selection().length === 2, `two items selected: ${selection()}`);
    await __eda.run("common.Interactive.group");
    const g = await until(async () => (await footprint(name)).items?.groups?.[0], 4000);
    if (!t(g && g.member_ids.length === 2, `a group of two: ${JSON.stringify(g)}`)) return;
    await key("Escape");
    await dblClickAt(-1.5 * MM, 0);
    t(await until(() => selection().sort().join() === [p1, line].sort().join(), 2000), `the double click enters the group: its members are selected (${selection()})`);
    await selectAt(-1.5 * MM, 0);
    t(selection().join() === p1, `inside the group a click picks the member alone: ${selection()}`);
    await __eda.run("common.Interactive.removeFromGroup");
    t(await until(async () => ((await footprint(name)).items?.groups ?? []).length === 0, 4000), "Remove Items: the group left with one member dissolves");
    await key("Escape");
    await key("Escape");
    // group again, then Add Items with the group and the plated pad selected
    await selectAt(-1.5 * MM, 0);
    await clickAt(0, -2 * MM, { shift: true });
    await __eda.run("common.Interactive.group");
    const g2 = await until(async () => (await footprint(name)).items?.groups?.[0], 4000);
    await key("Escape");
    await selectAt(-1.5 * MM, 0);
    await clickAt(1.5 * MM, 0, { shift: true });
    t(selection().includes(g2?.id), `the group and the pad are selected: ${selection()}`);
    await __eda.run("common.Interactive.addToGroup");
    const g3 = await until(async () => { const x = (await footprint(name)).items?.groups?.[0]; return x && x.member_ids.length === 3 ? x : null; }, 4000);
    t(g3, `Add Items: the pad joins the group: ${JSON.stringify(g3?.member_ids)}`);
    t(selection().join() === g2?.id, `the group alone is selected after: ${selection()}`);
    await __eda.run("common.Interactive.ungroup");
    await sleep(600);
  });

  await scenario("FF8: Mirror leaves a point where it is and says BOARD_ITEM::Mirror's message", async (t) => {
    const f = await footprint(name);
    const pt = f.items?.points?.[0];
    if (!t(pt, "the footprint has a point")) return;
    __eda.fp?.select?.([pt.id]);
    await selectAt(0, 3 * MM);
    t(selection().join() === pt.id, `the point is selected: ${selection()}`);
    await __eda.run("pcbnew.InteractiveEdit.mirrorHoriontally");
    await sleep(800);
    const after = (await footprint(name)).items.points[0];
    t(after.at.x === pt.at.x && after.at.y === pt.at.y, `the point stays: ${JSON.stringify(after.at)}`);
    t(/virtual BOARD_ITEM::Mirror used, should not occur/.test(document.body.textContent), "KiCad's message is shown");
  }, [/virtual BOARD_ITEM::Mirror used, should not occur/]);

  await scenario("FF8: Draw Dimension's preview is the dimension worked out by the server (arrows and text)", async (t) => {
    const asked = [];
    const real = window.fetch;
    window.fetch = (u, o) => { if (String(u).includes("/api/dimension/preview")) asked.push(String(u)); return real(u, o); };
    try {
      await __eda.run("pcbnew.InteractiveDrawing.alignedDimension");
      await sleep(300);
      await clickAt(-4 * MM, 5 * MM);
      fire("pointermove", 4 * MM, 5 * MM, 0, { ctrl: true, shift: true });
      await sleep(900);
      t(asked.length > 0, `the preview is asked of the server: ${asked.length}`);
    } finally {
      window.fetch = real;
    }
    await key("Escape");
    await key("Escape");
  });

  await scenario("FF8: a built-in package opened in the Footprint Editor has the 3D models of the footprint it stands for", async (t) => {
    const parts = (await json("/api/state")).parts ?? [];
    const pkg = ["0402", "0603", "0805"].find((p) => parts.some((x) => x.package === p || x.footprint === p));
    if (!pkg) return t(true, "no built-in passive package on this board: nothing to open");
    const had = (await fpNames()).includes(pkg);
    if (had) return t(true, `${pkg} is already in the project library`);
    const r = await cmd({ op: "open_footprint_for_edit", name: pkg });
    t(r.ok, `opened ${pkg}`);
    const f = await footprint(pkg);
    t((f.models3d ?? []).length > 0 && /\.step$/i.test(f.models3d[0].path ?? ""), `its model list: ${JSON.stringify((f.models3d ?? []).map((m) => m.path))}`);
    await cmd({ op: "delete_library_footprint", name: pkg });
  });

  // --------------------------------------------------------------------------------------------------------------------------- FF5
  await scenario("FF5: Presets (Front Layers hides the back), a viewport saved and gone back to, the Selection Filter gating a click and kept", async (t) => {
    await clickTab("Footprint");
    await __eda.run("common.Control.zoomFitScreen");
    const presets = await until(() => tid("fp-presets"), 3000);
    if (!t(presets, "the Appearance manager has a Presets list")) return;
    const names = [...presets.querySelectorAll("option")].map((o) => o.textContent.trim());
    t(names.includes("Front Layers") && names.includes("All Layers") && names.includes("Save preset...") && names.includes("Delete preset..."), `KiCad's built-in presets and commands: ${names.join(", ")}`);
    await choose(presets, "p:Front Layers");
    const bRow = document.querySelector('[data-testid="fp-appearance"] .ap-row[data-layer="B.Cu"]');
    t(bRow && !bRow.querySelector(".ap-eye.on, [aria-pressed=true]") && __eda.fp?.state?.()?.layerVisible?.["B.Cu"] !== true, "Front Layers hides B.Cu");
    t(presets.value === "p:Front Layers", `the list shows the preset in force: ${presets.value}`);
    await choose(presets, "p:All Layers");
    // a viewport: saved, the view moved, chosen again
    const vps = tid("fp-viewports");
    await choose(vps, "cmd:save");
    const input = await until(() => document.querySelector(".dialog-backdrop input[type=text], .dialog-backdrop input:not([type])"), 2000);
    if (t(input, "Save Viewport asks for a name")) {
      await type(input, "here");
      [...document.querySelectorAll(".dialog-backdrop button")].find((b) => b.textContent.trim() === "OK")?.click();
      await sleep(300);
      const saved = { ...__eda.state().view };
      await __eda.run("common.Control.zoomInCenter");
      await sleep(300);
      await choose(vps, "v:here");
      const back = __eda.state().view;
      t(Math.abs(back.scale - saved.scale) / saved.scale < 0.02, `choosing it goes back to it: ${saved.scale} -> ${back.scale}`);
    }
    // the Selection Filter pane
    const filter = await until(() => tid("fp-selection-filter"), 2000);
    if (!t(filter, "the Selection Filter pane is under the Appearance manager")) return;
    const padsBox = filter.querySelector('[data-filter-row="pads"] input');
    padsBox.click();
    await sleep(300);
    await key("Escape");
    const padId = (await footprint(name)).pads.find((q) => q.number === "1").id;
    await selectAt(-1.5 * MM, 0);
    t(!selection().includes(padId), `Pads off: a click on the pad does not pick it (${selection()})`);
    const stored = JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}");
    t(stored.fpSelectionFilter?.pads === false || JSON.stringify(stored).includes('"pads":false'), "the filter is kept with the preferences");
    padsBox.click();
    await sleep(300);
    await key("Escape");
    await selectAt(-1.5 * MM, 0);
    t(selection().join() === padId, `Pads on: picked again (${selection()})`);
  });

  // --------------------------------------------------------------------------------------------------------------------------- the end
  for (const n of await fpNames()) if (!libraryBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
  if (prefsBefore !== null) localStorage.setItem("eda-studio.preferences.v1", prefsBefore);
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
