// UI-LEFT-3 pcb-misc lane (task-v18-pcbmisc; PARITY-pcb "pcb-misc"), driven in the studio as a user drives it, each change read back from /api/state and undone:
//   - P3-18: Draw Rule Area asks for the zone's properties at the first corner (`ZONE_CREATE_HELPER::OnFirstPoint`), "Rule area" set; Cancel draws nothing;
//   - P3-19: Paste of a bitmap (no text on the clipboard) makes a reference image on the board, carried and dropped as one undo step, and an image on the sheet
//     (Place Image in immediate mode);
//   - P3-20: Create Tracks from Selection has no dialog on a copper layer, makes tracks of no net and deletes the line; on a technical layer it asks for a copper
//     layer (`SelectOneLayer`);
//   - P3-25: Plot's "Use KiCad font to plot text" is enabled while "Plot graphic items using their contours" is unticked;
//   - P3-26: Plot of no layer in Gerber writes the job file ("Created Gerber job file ...", "Done.");
//   - P3-27: Placement Files' "Exclude all footprints with the Exclude from BOM flag set" is a box of its own, and Gerber X3 keeps the DNP and BOM filters;
//   - Custom Properties (KiCad 25ec2335): the Properties panel's group, "+" adds `Property0` whose name is typed at once, its text edited, then removed from the
//     row's context menu; each one undo step; the Fields group's "+" (Add Field), a field renamed and removed; a duplicate and a pasted copy keep the properties.
// It answers through `window.__pcbmiscV18Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__pcbmiscV18Check = { results, done: false, ok: false, wait: null });
  const json = (url, body) => fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const S = () => E.state();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const until = async (fn, ms = 10000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const state = () => json("/api/state");
  const schematic = () => json("/api/schematic");
  const cmd = (c) => json("/api/cmd", { cmd: c, strict: false });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(900); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const canvas = () => document.querySelector(".pcb-canvas-container canvas") ?? [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => { const v = S().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y, mods = {}) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : (mods.buttons ?? 0) }));
  };
  const click = async (wx, wy, wait = 700) => { const [x, y] = w2c(wx, wy); fire("pointermove", x, y); await sleep(250); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); await sleep(wait); };
  const tab = async (name) => {
    if (S().tab !== name) {
      [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === (name === "pcb" ? "PCB" : "Schematic"))?.click();
      await until(() => S().tab === name);
      await sleep(900);
    }
    return S().tab === name;
  };
  const makePng = async (w, h) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const g = c.getContext("2d");
    g.fillStyle = "#ff00ff";
    g.fillRect(0, 0, w, h);
    return new Uint8Array(await (await new Promise((r) => c.toBlob(r, "image/png"))).arrayBuffer());
  };
  // the system clipboard is the page's own here: no text, and a bitmap while `image` is set
  let image = null;
  try {
    Object.defineProperty(navigator.clipboard, "readText", { configurable: true, value: async () => "" });
    Object.defineProperty(navigator.clipboard, "writeText", { configurable: true, value: async () => {} });
    Object.defineProperty(navigator.clipboard, "read", { configurable: true, value: async () => (image ? [{ types: ["image/png"], getType: async () => new Blob([image], { type: "image/png" }) }] : []) });
  } catch { /* no clipboard object */ }
  // (the studio's Strict switch would refuse a track on this unrouted board, as it judges the routing from then on: off here, as the import-netlist check has it)
  const strictBox = () => [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const setStrict = async (on) => { const b = strictBox(); if (b && b.checked !== on) { b.click(); await sleep(400); } };
  const errorsFrom = Date.now();
  await tab("pcb");
  const strictWas = !!strictBox()?.checked;
  await setStrict(false);
  const snapshot = async () => { const s = await state(); return JSON.stringify([s.parts.map((p) => [p.ref, p.at, p.placed]), (s.routing?.tracks ?? []).length, (s.drawings?.shapes ?? []).length, (s.drawings?.images ?? []).length, s.custom_properties ?? {}]); };
  const start = await snapshot();
  const board0 = await state();
  const r1 = board0.parts.find((p) => p.ref === "R1" && p.placed) ?? board0.parts.find((p) => p.placed);

  await scenario("P3-18: Draw Rule Area asks for the zone's properties at the first corner; Cancel draws nothing and the tool stays", async (t) => {
    await E.run("pcbnew.InteractiveDrawing.ruleArea");
    await sleep(400);
    await click(r1.at[0] + 6_000_000, r1.at[1] + 6_000_000);
    const dialog = await until(() => tid("zone-dialog"), 5000);
    t(!!dialog, "the dialog opens at the first click, before any outline");
    t(!!dialog && /rule area/i.test(dialog.textContent ?? ""), "on a rule area");
    tid("zone-cancel")?.click();
    await sleep(500);
    t(!tid("zone-dialog") && ((await state()).routing?.zones ?? []).length === (board0.routing?.zones ?? []).length, "Cancel: no zone");
    await escape();
    await escape();
  });

  await scenario("P3-19: a bitmap on the clipboard pastes as a reference image on the board, carried and dropped as one undo step", async (t) => {
    image = await makePng(40, 30);
    const before = (await state()).drawings?.images?.length ?? 0;
    await select([]);
    await E.run("common.Interactive.paste");
    await sleep(1200);
    t(S().tool === "move", `the image rides the cursor (tool ${S().tool})`);
    await click(r1.at[0] + 10_000_000, r1.at[1]);
    const imgs = await until(async () => { const s = await state(); return (s.drawings?.images?.length ?? 0) === before + 1 ? s.drawings.images : null; });
    t(!!imgs, "a reference image is on the board");
    const img = imgs?.[imgs.length - 1];
    t(img?.layer === "F.Cu", `on F.Cu, PCB_REFERENCE_IMAGE's own layer (${img?.layer})`);
    await undo();
    t(((await state()).drawings?.images?.length ?? 0) === before, "one Undo takes it away");
  });

  await scenario("P3-19: on the sheet the bitmap is Place Image's, dropped at the next click", async (t) => {
    await tab("schematic");
    await E.run("common.Control.zoomFitScreen");
    await sleep(500);
    const images = async () => ((await schematic()).graphics ?? []).filter((g) => g.shape.type === "image");
    const before = (await images()).length;
    await E.run("common.Interactive.paste");
    t(await until(() => /put clipboard down/i.test(S().picker ?? ""), 5000), `the image is in the hand (${S().picker})`);
    const [px, py] = w2c(230000, 160000);
    fire("pointermove", px, py);
    await sleep(200);
    fire("pointerdown", px, py, { buttons: 1 });
    await sleep(60);
    fire("pointerup", px, py);
    t(await until(async () => (await images()).length === before + 1), "the click puts it on the sheet");
    t(S().picker == null, "immediate mode: the tool ends");
    await undo();
    t((await images()).length === before, "one Undo takes it away");
    image = null;
    await tab("pcb");
  });

  await scenario("P3-20: Create Tracks from Selection: no dialog on a copper layer, tracks of no net, the line deleted; a technical layer asks for a copper one", async (t) => {
    const added = await cmd({ op: "add_shape", shape: { kind: "segment", layer: "F.SilkS", stroke_width: 250000, filled: false, start: { x: r1.at[0] + 4_000_000, y: r1.at[1] + 8_000_000 }, end: { x: r1.at[0] + 9_000_000, y: r1.at[1] + 8_000_000 } } });
    t(added.ok, "a line on F.SilkS");
    const line = await until(async () => (await state()).drawings?.shapes?.find((s) => s.kind === "segment" && s.layer === "F.SilkS" && s.start[0] === r1.at[0] + 4_000_000));
    const tracks0 = (await state()).routing?.tracks?.length ?? 0;
    await E.run("pcbnew.Control.layerTop");
    await select([line.id]);
    await E.run("pcbnew.Convert.convertToTracks");
    await sleep(900);
    t(!document.querySelector('[role="dialog"]') || !/Create Tracks/i.test(document.querySelector('[role="dialog"]')?.textContent ?? ""), "no dialog");
    const s = await until(async () => { const st = await state(); return (st.routing?.tracks?.length ?? 0) === tracks0 + 1 ? st : null; });
    const track = s?.routing?.tracks?.[s.routing.tracks.length - 1] ?? s?.routing?.tracks?.find((x) => x.width === 250000);
    t(!!s && track?.layer === "F.Cu" && (track?.net ?? "") === "" && track?.width === 250000, `a track on F.Cu of no net, the line's width: ${JSON.stringify(track && { layer: track.layer, net: track.net, width: track.width })}`);
    t(!(s?.drawings?.shapes ?? []).some((x) => x.id === line.id), "the line is deleted (Delete source objects, on by default)");
    await undo();
    t(((await state()).routing?.tracks?.length ?? 0) === tracks0 && !!(await state()).drawings?.shapes?.some((x) => x.id === line.id), "one Undo: the line back, no track");
    // a technical layer active (the Appearance panel's F.SilkS row): KiCad's layer picker, from F.Cu; B.Cu picked
    document.querySelector('.ap-root [data-tab="layers"]')?.click();
    await sleep(200);
    const row = document.querySelector('.ap-root .ap-row[data-layer="f_silks"]');
    if (row && !row.classList.contains("active")) row.click();
    await sleep(300);
    t(S().activeLayer === "f_silks", `F.SilkS is the active layer (${S().activeLayer})`);
    await select([line.id]);
    await E.run("pcbnew.Convert.convertToTracks");
    const grid = await until(() => tid("select-layer-grid"), 4000);
    t(!!grid, "Select Layer opens");
    grid?.querySelector('[data-layer="B.Cu"]')?.click();
    const s2 = await until(async () => { const st = await state(); return (st.routing?.tracks?.length ?? 0) === tracks0 + 1 ? st : null; });
    t(!!s2?.routing?.tracks?.some((x) => x.layer === "B.Cu" && x.width === 250000), "the track on the layer picked");
    await undo();
    await cmd({ op: "delete_shape", id: line.id });
    await E.run("pcbnew.Control.layerTop");
  });

  await scenario("P3-25 and P3-26: Plot's DXF font box follows the contours box; a Gerber plot of no layer writes its job file", async (t) => {
    await E.run("pcbnew.EditorControl.generateGerbers");
    const fmt = await until(() => tid("plot-format"), 5000);
    if (!t(!!fmt, "the Plot dialog opens")) return;
    const sel = fmt.tagName === "SELECT" ? fmt : fmt.querySelector("select");
    if (sel) { sel.value = "dxf"; sel.dispatchEvent(new Event("change", { bubbles: true })); }
    await sleep(300);
    const font = () => tid("plot-dxf-kicad-font");
    const contours = () => [...(tid("plot-dxf-options")?.querySelectorAll("label") ?? [])].find((l) => /contours/.test(l.textContent))?.querySelector("input");
    if (contours()?.checked === false) { contours().click(); await sleep(200); }
    t(font()?.disabled === true && font()?.checked === true, "contours ticked: the font box ticked and disabled");
    contours()?.click();
    await sleep(200);
    t(font()?.disabled === false, "contours unticked: the font box can be unticked");
    contours()?.click();
    await sleep(200);
    if (sel) { sel.value = "gerber"; sel.dispatchEvent(new Event("change", { bubbles: true })); }
    await sleep(300);
    // every layer unchecked
    for (const box of tid("plot-include-layers")?.querySelectorAll("input[type=checkbox]") ?? []) if (box.checked) box.click();
    await sleep(200);
    [...document.querySelectorAll("button.primary")].find((b) => b.textContent.trim() === "Plot")?.click();
    const said = await until(() => { const b = [...document.querySelectorAll("b")].find((x) => /Done\./.test(x.textContent ?? "")); return b?.textContent; }, 30000);
    t(/Created Gerber job file '.*-job\.gbrjob'\.\s*Done\./.test(said ?? ""), `the job file alone: ${said}`);
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Close")?.click();
    await sleep(400);
  });

  await scenario("P3-27: Placement Files has the Exclude from BOM box, and Gerber X3 keeps the DNP and BOM filters", async (t) => {
    await E.run("pcbnew.EditorControl.generatePosFile");
    const bom = await until(() => tid("pos-exclude-bom"), 5000);
    if (!t(!!bom, "the dialog opens with the box")) return;
    t(bom.disabled === false, "the box can be ticked");
    const gerber = [...document.querySelectorAll('input[name="pos-format"]')][2];
    gerber?.click();
    await sleep(200);
    t(tid("pos-exclude-bom").disabled === false && tid("pos-exclude-dnp").disabled === false, "Gerber X3: DNP and BOM filters stay (`FormatSupportsFilter`)");
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Close")?.click();
    await sleep(300);
  });

  await scenario("Custom Properties: + adds Property0 and its name is typed, its text edited, the context menu removes it; each one undo step", async (t) => {
    await select([r1.ref]);
    await sleep(500);
    const group = () => document.querySelector('[data-group="Custom Properties"]');
    t(!!group(), "the group is there with no row (its button)");
    tid("prop-add-custom")?.click();
    const label = await until(() => tid("prop-key-label"), 5000);
    t(!!label && label.value === "", "the new row's name is in edit, blank");
    const added = await until(async () => (await state()).custom_properties?.[r1.ref]?.Property0 === "");
    t(!!added, "Property0 with an empty text on the footprint");
    setValue(label, "Supplier");
    label.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    const renamed = await until(async () => { const p = (await state()).custom_properties?.[r1.ref]; return p && "Supplier" in p && !("Property0" in p) ? p : null; });
    t(!!renamed, `renamed: ${JSON.stringify(renamed)}`);
    const row = await until(() => document.querySelector('[data-custom-prop="Supplier"]'));
    const input = row?.querySelector(".prop-value input");
    if (input) {
      input.focus();
      setValue(input, "ACME");
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    t(await until(async () => (await state()).custom_properties?.[r1.ref]?.Supplier === "ACME"), "its text edited");
    const r = document.querySelector('[data-custom-prop="Supplier"]');
    r?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    await sleep(300);
    const remove = [...(tid("prop-custom-menu")?.querySelectorAll('[role="menuitem"]') ?? [])].find((m) => m.textContent.trim() === "Remove Custom Property");
    t(!!remove, "the row's menu has Remove Custom Property");
    remove?.click();
    t(await until(async () => !(await state()).custom_properties?.[r1.ref]), "removed");
    await undo();
    t((await state()).custom_properties?.[r1.ref]?.Supplier === "ACME", "Undo puts it back");
    await undo();
    await undo();
    await undo();
    t(!(await state()).custom_properties?.[r1.ref], "three more: none");
    await select([]);
  });

  await scenario("Fields: + adds Field0 (hidden, empty) and its name is typed; the row's context menu removes it; each one undo step", async (t) => {
    await select([r1.ref]);
    await sleep(500);
    const fieldNames = async () => ((await state()).parts.find((p) => p.ref === r1.ref)?.fields ?? []).map((f) => f.name);
    const before = await fieldNames();
    const plus = tid("prop-add-fields");
    t(!!plus, "the Fields group has its + button");
    plus?.click();
    const label = await until(() => tid("prop-key-label"), 5000);
    t(!!label && label.value === "", "the new row's name is in edit, blank");
    const added = await until(async () => { const f = (await state()).parts.find((p) => p.ref === r1.ref)?.fields?.find((x) => x.name === "Field0"); return f ?? null; });
    t(!!added && added.text === "" && added.visible === false, `Field0, empty and hidden: ${JSON.stringify(added && { text: added.text, visible: added.visible })}`);
    if (label) {
      setValue(label, "MPN");
      label.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    t(await until(async () => { const n = await fieldNames(); return n.includes("MPN") && !n.includes("Field0"); }), "renamed MPN");
    const row = await until(() => document.querySelector('[data-keyed-prop="MPN"]'));
    row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    await sleep(300);
    const remove = [...(tid("prop-custom-menu")?.querySelectorAll('[role="menuitem"]') ?? [])].find((m) => m.textContent.trim() === "Remove Field");
    t(!!remove, "the row's menu has Remove Field");
    remove?.click();
    t(await until(async () => !(await fieldNames()).includes("MPN")), "removed");
    await undo();
    await undo();
    await undo();
    t(JSON.stringify(await fieldNames()) === JSON.stringify(before), "three undos: as it was");
    await select([]);
  });

  await scenario("Custom properties go with a copy: Duplicate and Copy / Paste", async (t) => {
    t((await cmd({ op: "set_custom_property", ids: [r1.ref], name: "Supplier", value: "ACME" })).ok, "R1 has Supplier = ACME");
    await E.refresh?.();
    await sleep(600);
    const withSupplier = async () => Object.entries((await state()).custom_properties ?? {}).filter(([, p]) => p.Supplier === "ACME").map(([id]) => id).sort();
    await select([r1.ref]);
    await E.run("common.Interactive.duplicate");
    await sleep(1200);
    await click(r1.at[0] + 12_000_000, r1.at[1] + 12_000_000);
    const dup = await until(async () => { const w = await withSupplier(); return w.length === 2 ? w : null; });
    t(!!dup, `the duplicate has it: ${JSON.stringify(dup)}`);
    await undo();
    t((await withSupplier()).length === 1, "Undo: the copy and its property go");
    await select([r1.ref]);
    await E.run("common.Interactive.copy");
    await sleep(800);
    await E.run("common.Interactive.paste");
    await sleep(1500);
    await click(r1.at[0] + 14_000_000, r1.at[1] + 14_000_000);
    const pasted = await until(async () => { const w = await withSupplier(); return w.length === 2 ? w : null; });
    t(!!pasted, `the pasted footprint has it (the clipboard's text carries it): ${JSON.stringify(pasted)}`);
    await undo();
    await undo();
    t(!(await state()).custom_properties?.[r1.ref], "two more undos: none");
    await select([]);
  });

  await setStrict(strictWas);
  await scenario("the board is as it was and nothing logged an error", async (t) => {
    const now = await snapshot();
    t(now === start, `as it was${now === start ? "" : `: ${now} vs ${start}`}`);
    const errs = E.errors(errorsFrom).map((e) => e.message ?? String(e));
    t(errs.length === 0, `errors: ${JSON.stringify(errs)}`);
  });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
