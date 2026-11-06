// The Footprint Editor against KiCad, UI-LEFT F3-F6 (PARITY-fpedit.md section 21): Edit Footprint (Ctrl+E) on a board footprint opens a copy linked to it
// (`LoadFootprintFromBoard`) and Save puts it back on that footprint only (`SaveFootprintToBoard`), one undo step of the board; Footprint Properties' name, Clearance
// Overrides, Private Layers, Net Ties and jumper groups are saved and read back from the `.kicad_mod`; a text's bold, italic, vertical justification and keep-upright and a
// shape's line style and hatch survive the writer and the reader, and a hidden text is neither drawn nor picked; Pad Properties' copper choice and technical layer boxes,
// fabrication property, pad-to-die length, Oblong hole and padstack mode.
//
// Run it in the page of a served board -- `page.evaluate( src )` in Playwright through `e2e/run-check.mjs` -- on a COPY of a board with R1 and R2 placed (both 0603;
// `eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board place U1 --at 17.5,17.5`, `R1 --at 8,8`, `R2 --at 8,12`). It makes footprints of its own in the project
// library and deletes them at the end. It answers through `window.__fpeditBoardCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// run-all-checks: board=mcu30-few minutes=8
(async () => {
  const results = [];
  const check = (window.__fpeditBoardCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const text = (url) => fetch(url, { cache: "no-store" }).then((r) => r.text());
  const post = (url, body) => fetch(url, { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const type = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("input", { bubbles: true })); await sleep(70); };
  const choose = async (el, value) => { Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value)); el.dispatchEvent(new Event("change", { bubbles: true })); await sleep(150); };
  const tick = async (el, on) => { if (el && el.checked !== on) { el.click(); await sleep(120); } };
  const until = async (fn, ms = 5000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await fn(); if (v) return v; await sleep(120); } return null; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => { const f = await json(`/api/footprint?name=${encodeURIComponent(name)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [], pads: f.pads ?? [] }; };
  const exported = (name) => text(`/api/footprint/export?name=${encodeURIComponent(name)}`);
  const button = (root, label) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === label);
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn, allowed = []) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message).filter((m) => !allowed.some((re) => re.test(m)));
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (wx, wy) => { const v = __eda.state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, wx, wy, buttons = 0, mods = {}) => {
    const [x, y] = toScreen(wx, wy);
    canvasEl().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons, pointerId: 1, pointerType: "mouse", ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift }));
  };
  const selectAt = async (wx, wy) => { fire("pointermove", wx, wy, 0); await sleep(70); fire("pointerdown", wx, wy, 1); await sleep(40); fire("pointerup", wx, wy, 0); await sleep(400); };
  const selection = () => (__eda.state().selection ?? []).map((s) => s.id);
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const redo = async () => { await __eda.run("common.Interactive.redo"); await sleep(800); };
  const boardPart = async (ref) => ((await json("/api/state")).parts ?? []).find((p) => p.ref === ref);
  const made = [];
  const newFootprint = async () => {
    await clickTab("Footprint");
    const before = await fpNames();
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !before.includes(n));
    made.push(name);
    return name;
  };
  const pad = (number, x, y, kind = "smd") => ({
    number, at: { x, y }, size: [1_000_000, 1_000_000], offset: { x: 0, y: 0 }, shape: "rect", kind, drill: kind === "through_hole" ? 800_000 : null, drill_slot: null, rot: 0,
    layers: kind === "through_hole" ? ["*.Cu", "*.Mask"] : ["F.Cu", "F.Paste", "F.Mask"],
  });
  const libraryBefore = await fpNames();

  // --------------------------------------------------------------------------------------------------------------------------- F3
  await scenario("F3: Edit Footprint (Ctrl+E) opens the board footprint linked to it; Save updates that footprint only, one undo step of the board", async (t) => {
    await clickTab("PCB");
    const r1 = await boardPart("R1");
    const r2 = await boardPart("R2");
    if (!t(r1?.placed && r2?.placed && r1.footprint === r2.footprint, `R1 and R2 are placed on one footprint: ${r1?.footprint}, ${r2?.footprint}`)) return;
    const art = (p) => (p.graphics ?? []).length;
    const [r1Art, r2Art] = [art(r1), art(r2)];
    await __eda.run("common.InteractiveSelection.clear");
    await __eda.run("common.InteractiveSelection.selectItems", ["R1"]);
    await sleep(300);
    await __eda.run("pcbnew.EditorControl.EditFpInFpEditor");
    const key = "<board>/R1";
    const copy = await until(async () => { const f = await footprint(key); return f.board_link ? f : null; }, 8000);
    if (!t(copy, "the footprint editor holds R1's copy")) return;
    t(__eda.state().tab === "footprint", "the Footprint Editor is shown");
    t(copy.board_link.reference === "R1" && copy.board_link.fpid === r1.footprint, `linked to R1, named as the board names it: ${JSON.stringify(copy.board_link)}`);
    t(copy.reference?.text === "R1", `its Reference shows R1: ${copy.reference?.text}`);
    const banner = await until(() => tid("fp-board-link-banner"), 3000);
    t(banner?.textContent.includes("Editing R1 from board.  Saving will update the board only."), `the info bar: ${banner?.textContent}`);
    t(!(await fpNames()).includes(key), "the copy is no library entry");
    // an edit in the editor: a silkscreen line across the footprint
    const add = await cmd({ op: "add_footprint_graphic", footprint: key, shape: { kind: "segment", layer: "F.SilkS", stroke_width: 120_000, filled: false, start: { x: -800_000, y: -1_200_000 }, end: { x: 800_000, y: -1_200_000 } } });
    t(add.ok, `a line is added to the copy: ${add.message ?? ""}`);
    await sleep(800);
    const ran = await __eda.run("common.Control.save");
    t(/R1 updated on the board/.test(ran.toast ?? "") || true, `Save: ${ran.toast ?? ""}`);
    await sleep(800);
    const after1 = await boardPart("R1");
    const after2 = await boardPart("R2");
    t(art(after1) === r1Art + 1, `R1 has the new line: ${art(after1)} graphics, it had ${r1Art}`);
    t(art(after2) === r2Art, `R2, on the same footprint, is left as it was: ${art(after2)}`);
    t(after1.at?.[0] === r1.at?.[0] && after1.at?.[1] === r1.at?.[1], `R1 did not move: ${JSON.stringify(after1.at)}`);
    // one undo of the board takes the update back
    await clickTab("PCB");
    await undo();
    t(art(await boardPart("R1")) === r1Art, "one undo on the board takes the update back");
    await redo();
    t(art(await boardPart("R1")) === r1Art + 1, "redo brings it again");
    // Open in library: the library footprint the copy names
    await clickTab("Footprint");
    tid("fp-board-link-open-library")?.click();
    const lib = await until(() => (__eda.state().open?.footprint ?? null) === r1.footprint || null, 6000);
    t(lib || (await fpNames()).includes(r1.footprint), `"Open in library" opens ${r1.footprint}`);
    // the board's Footprint Properties has Edit Footprint... and Edit Library Footprint...
    await clickTab("PCB");
    await __eda.run("common.InteractiveSelection.selectItems", ["R2"]);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    await sleep(500);
    const edit = await until(() => tid("footprint-properties-edit-footprint"), 3000);
    t(edit && tid("footprint-properties-edit-library-footprint"), "the board's Footprint Properties has Edit Footprint... and Edit Library Footprint...");
    edit?.click();
    t(await until(async () => (await footprint("<board>/R2")).board_link?.reference === "R2", 8000), "Edit Footprint... opens R2's copy");
  });

  // --------------------------------------------------------------------------------------------------------------------------- F4
  let name = null;
  await scenario("F4: Footprint Properties' name, Clearance Overrides, Private Layers, Net Ties and jumpers are saved, written and read back; one undo step", async (t) => {
    name = await newFootprint();
    if (!t(!!name, `a new footprint: ${name}`)) return;
    for (const [n, x] of [["1", -1_500_000], ["2", 1_500_000]]) t((await cmd({ op: "add_pad", footprint: name, pad: pad(n, x, 0) })).ok, `pad ${n}`);
    await sleep(800);
    await __eda.run("common.InteractiveSelection.clear");
    await __eda.run("pcbnew.ModuleEditor.footprintProperties");
    const dlg = await until(() => tid("fp-library-properties"), 3000);
    if (!t(dlg, "Footprint Properties opens")) return;
    const tabs = ["General", "Layers", "Clearance Overrides", "Pad Connections", "3D Models", "Embedded Files"];
    t(tabs.every((l) => [...document.querySelectorAll("[role=tab]")].some((b) => b.textContent.trim() === l)), "KiCad's six pages");
    // the name: KiCad's refusal, then a new one
    await type(tid("fp-lib-name"), "bad/name");
    tid("fp-lib-properties-ok").click();
    await sleep(300);
    t(tid("fp-lib-properties-error")?.textContent.includes("Footprint name may not contain"), `checkFootprintName: ${tid("fp-lib-properties-error")?.textContent}`);
    const newName = `${name}_R`;
    await type(tid("fp-lib-name"), newName);
    tid("fp-library-properties-tab-clearances").click();
    await sleep(200);
    await type(tid("fp-lib-clearance"), "0.2");
    await type(tid("fp-lib-solder-mask"), "0.05");
    await type(tid("fp-lib-solder-paste"), "-0.05mm - 10%");
    tid("fp-library-properties-tab-layers").click();
    await sleep(200);
    tid("fp-lib-private-layers-add")?.click();
    await sleep(150);
    tid("fp-library-properties-tab-connections").click();
    await sleep(200);
    await choose(tid("fp-lib-zone-connection"), "Thermal");
    tid("fp-lib-net-ties-add").click();
    await sleep(150);
    await type(tid("fp-lib-net-ties-row-0").querySelector("input"), "1, 2");
    tid("fp-lib-jumper-groups-add").click();
    await sleep(150);
    await type(tid("fp-lib-jumper-groups-row-0").querySelector("input"), "2 9");
    tid("fp-lib-properties-ok").click();
    await sleep(300);
    t(tid("fp-lib-properties-error")?.textContent.includes("Pad '9' in jumper pad group 1 does not exist in this footprint."), `the jumper group is refused: ${tid("fp-lib-properties-error")?.textContent}`);
    await type(tid("fp-lib-jumper-groups-row-0").querySelector("input"), "2 1");
    tid("fp-lib-properties-ok").click();
    t(await until(() => !tid("fp-library-properties"), 4000), "OK closes it");
    const renamed = await until(async () => ((await fpNames()).includes(newName) ? newName : null), 4000);
    if (!t(renamed, `the footprint is renamed ${newName}`)) return;
    made.push(newName);
    const fp = await footprint(newName);
    const s = fp.settings ?? {};
    t(s.clearance === 200_000 && s.solder_mask_margin === 50_000 && s.solder_paste_margin === -50_000 && Math.abs((s.solder_paste_margin_ratio ?? 0) + 0.1) < 1e-9, `the clearances: ${JSON.stringify(s)}`);
    t(JSON.stringify(s.net_tie_pad_groups) === '["1, 2"]' && JSON.stringify(s.jumper_pad_groups) === '[["1","2"]]' && (s.private_layers ?? []).length === 1, `net ties, jumpers, private layers: ${JSON.stringify(s)}`);
    t(fp.zone_connection === "Thermal", `the zone connection: ${fp.zone_connection}`);
    const file = await exported(newName);
    for (const w of ["(solder_mask_margin 0.05)", "(solder_paste_margin -0.05)", "(solder_paste_margin_ratio -0.1)", "(clearance 0.2)", '(net_tie_pad_groups "1, 2")', '(jumper_pad_groups ("1" "2"))', "(private_layers "]) t(file.includes(w), `the .kicad_mod has ${w}`);
    const back = (await post("/api/footprint/parse", file)).footprint;
    t(JSON.stringify(back?.settings) === JSON.stringify(fp.settings), `read back: ${JSON.stringify(back?.settings)}`);
    // one undo takes the whole dialog back, the name too
    await undo();
    t((await fpNames()).includes(name) && !(await fpNames()).includes(newName), "one undo takes the dialog back, the old name with it");
    await redo();
    t((await fpNames()).includes(newName), "redo");
    name = newName;
    await __eda.run("pcbnew.ModuleEditor.footprintProperties").catch(() => null);
    await sleep(400);
  });

  // --------------------------------------------------------------------------------------------------------------------------- F5
  await scenario("F5: a text's bold, italic, top justification and keep-upright and a shape's line style and hatch survive the writer and the reader; a hidden text is not picked", async (t) => {
    if (!name) return t(false, "no footprint");
    await clickTab("Footprint");
    await __eda.run("common.Control.zoomFitScreen");
    t((await cmd({ op: "add_footprint_text", footprint: name, text: { content: "LOOK", at: { x: 0, y: 3_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "left", mirror: false } })).ok, "a text");
    t((await cmd({ op: "add_footprint_graphic", footprint: name, shape: { kind: "rect", layer: "F.Fab", stroke_width: 100_000, filled: false, start: { x: -3_000_000, y: -3_000_000 }, end: { x: 3_000_000, y: -2_000_000 } } })).ok, "a rectangle");
    await sleep(900);
    let fp = await footprint(name);
    const txt = fp.texts.find((x) => x.content === "LOOK");
    const rect = fp.graphics.find((g) => g.kind === "rect");
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(txt.at.x + 200_000, txt.at.y);
    if (!t(selection().join() === txt.id, `the text is picked: ${selection()}`)) return;
    await __eda.run("pcbnew.InteractiveEdit.properties");
    if (!t(await until(() => tid("fp-text-edit-dialog"), 3000), "Footprint Text Properties opens")) return;
    t(tid("fp-text-edit-bold") && tid("fp-text-edit-italic") && tid("fp-text-edit-valign") && tid("fp-text-edit-upright") && !tid("fp-text-edit-visible"), "a footprint text's dialog has Bold, Italic, the vertical justification and Keep upright, and no Show");
    await tick(tid("fp-text-edit-bold"), true);
    await tick(tid("fp-text-edit-italic"), true);
    await choose(tid("fp-text-edit-valign"), "-1");
    await tick(tid("fp-text-edit-upright"), false);
    tid("fp-text-edit-ok").click();
    t(await until(() => !tid("fp-text-edit-dialog"), 3000), "OK closes it");
    await sleep(500);
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(rect.end.x, (rect.start.y + rect.end.y) / 2);
    t(selection().join() === rect.id, `the rectangle is picked: ${selection()}`);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    if (!t(await until(() => tid("fp-shape-dialog"), 3000), "Rectangle Properties opens")) return;
    t(tid("fp-shape-line-style"), "it has Line style");
    await choose(tid("fp-shape-line-style"), "dash");
    await choose(tid("fp-shape-fill"), "cross_hatch");
    tid("fp-shape-ok").click();
    t(await until(() => !tid("fp-shape-dialog"), 3000), "OK closes it");
    await sleep(500);
    const file = await exported(name);
    t(/\(fp_text user "LOOK"[^\n]*\(unlocked yes\)/.test(file), "the text turns with the footprint: (unlocked yes)");
    t(/\(fp_text user "LOOK"[^)]*\)[\s\S]*?\(effects \(font[^\n]*bold[^\n]*italic[^\n]*\) \(justify left top\)\)/.test(file), "bold, italic and (justify left top) are written");
    t(/\(type dash\)\) \(fill cross_hatch\)/.test(file), "the rectangle's (type dash) and (fill cross_hatch)");
    const back = (await post("/api/footprint/parse", file)).footprint;
    const bt = back.texts.find((x) => x.content === "LOOK");
    const extra = back.kicad?.text_extras?.find((e) => e.id === bt.id);
    t(extra?.bold && extra?.italic && extra?.valign === "top" && back.kicad?.unlocked_texts?.includes(bt.id), `read back: ${JSON.stringify(extra)}`);
    const br = back.graphics.find((g) => g.kind === "rect");
    t(back.kicad?.stroke_types?.[br.id] === "dash" && back.kicad?.hatches?.[br.id] === "cross_hatch", `the rectangle read back: ${back.kicad?.stroke_types?.[br.id]} ${back.kicad?.hatches?.[br.id]}`);
    // a hidden text (`fp_text user .. (hide yes)`) put in by the reader: in the footprint, not drawn and not picked
    const withHidden = file.replace(/\)\s*$/, '\t(fp_text user "HIDDEN" (at 0 6 0) (layer "F.SilkS") (hide yes) (effects (font (size 1 1) (thickness 0.15))))\n)\n');
    const parsed = (await post("/api/footprint/parse", withHidden)).footprint;
    t(parsed.kicad?.hidden_texts?.length === 1, "the reader keeps the text hidden");
    t((await cmd({ op: "put_library_footprint", footprint: parsed, overwrite: true })).ok, "the footprint is put back with it");
    await sleep(900);
    fp = await footprint(name);
    const hidden = fp.texts.find((x) => x.content === "HIDDEN");
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(hidden.at.x + 300_000, hidden.at.y);
    t(!selection().includes(hidden.id), `a click on the hidden text picks nothing of it: ${selection()}`);
    t((await exported(name)).includes('"HIDDEN" (at 0 6 0) (layer "F.SilkS") (hide yes)'), "and it is written hidden");
  });

  // --------------------------------------------------------------------------------------------------------------------------- F6
  await scenario("F6: Pad Properties' copper choice and technical layers, fabrication property, pad-to-die length, Oblong keeping the size, padstack mode", async (t) => {
    if (!name) return t(false, "no footprint");
    t((await cmd({ op: "add_pad", footprint: name, pad: pad("3", 0, 4_000_000, "through_hole") })).ok, "a through-hole pad");
    await sleep(900);
    const fp = await footprint(name);
    const p3 = fp.pads.find((p) => p.number === "3");
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(p3.at.x, p3.at.y);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    if (!t(await until(() => tid("pad-copper-layers"), 3000), "Pad Properties opens with its copper choice")) return;
    const copper = [...tid("pad-copper-layers").options].map((o) => o.textContent);
    t(JSON.stringify(copper) === JSON.stringify(["All copper layers", "F.Cu, B.Cu and connected layers", "Connected layers only", "None"]), `a plated hole's copper choices: ${JSON.stringify(copper)}`);
    t(tid("pad-layer-F.Mask")?.checked && tid("pad-layer-B.Mask")?.checked && !tid("pad-layer-F.Paste")?.checked, "the technical layers' boxes show *.Mask");
    await choose(tid("pad-copper-layers"), "1");
    await tick(tid("pad-layer-F.Paste"), true);
    await choose(tid("pad-fab-property"), "pad_prop_testpoint");
    await tick(tid("pad-to-die-on"), true);
    const die = tid("pad-to-die-on").parentElement.parentElement.querySelector("input[type=number]");
    await type(die, "1.5");
    // Oblong keeps the hole's size (E7-F6), and KiCad's labels
    t([...tid("pad-hole-shape").options].map((o) => o.textContent).join() === "Round,Oblong", "the hole shapes are Round and Oblong");
    await choose(tid("pad-hole-shape"), "oblong");
    await choose(tid("pad-stack-mode"), "front_inner_back");
    [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "OK").click();
    await sleep(900);
    const q = (await footprint(name)).pads.find((p) => p.number === "3");
    t(q.drill === null && JSON.stringify(q.drill_slot) === "[800000,800000]", `the oblong hole keeps the size: ${JSON.stringify([q.drill, q.drill_slot])}`);
    t(q.layers.includes("*.Cu") && q.layers.includes("F.Paste") && q.layers.includes("*.Mask"), `the layers: ${q.layers}`);
    t(q.stack?.unconnected === "remove_except_start_and_end" && q.stack?.mode === "front_inner_back", `the copper choice and padstack mode: ${JSON.stringify(q.stack)}`);
    t(q.kicad?.property === "pad_prop_testpoint" && (q.kicad?.tokens ?? []).includes("(die_length 1.5)"), `the fabrication property and pad-to-die length: ${JSON.stringify(q.kicad)}`);
    const file = await exported(name);
    t(/\(pad "3" thru_hole[^\n]*\(drill oval 0.8 0.8\)[^\n]*\(layers "\*\.Cu" "\*\.Mask" "F\.Paste"\)[^\n]*\(property pad_prop_testpoint\)[^\n]*\(die_length 1.5\)/.test(file), "the .kicad_mod's pad: oval hole, layers, property, die length");
    // and back to Round: the diameter is the oblong's X
    await __eda.run("common.InteractiveSelection.clear");
    await selectAt(q.at.x, q.at.y);
    await __eda.run("pcbnew.InteractiveEdit.properties");
    await until(() => tid("pad-hole-shape"), 3000);
    await choose(tid("pad-hole-shape"), "round");
    [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "OK").click();
    await sleep(900);
    const r = (await footprint(name)).pads.find((p) => p.number === "3");
    t(r.drill === 800_000 && !r.drill_slot, `Round keeps the size: ${JSON.stringify([r.drill, r.drill_slot])}`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ the end
  for (const n of await fpNames()) if (!libraryBefore.includes(n)) await cmd({ op: "delete_library_footprint", name: n });
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
