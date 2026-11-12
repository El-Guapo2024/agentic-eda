// UI-LEFT-2 PP21, PP22, PP24 (task-v7-pcb; web/studio/PARITY-pcb.md section 109): what the editor shows of them, driven through `window.__eda` and the dialogs' controls and
// checked against what was drawn (`window.__edaText`) and what the server holds (`/api/state`). Run it in the page of a served scratch board made from
// `examples/mcu_board_30plus.yaml` (`eda board new .. -o <dir>`, `eda board serve -C <dir>`): its last scenario replaces the board with a small one of its own.
//
//   1  Flip Board View (KiCad 25ec2335 `PCB_PAINTER::draw( PCB_TEXTBOX )`, `BOARD_ITEM::IsSideSpecific`): a text box and a table cell on Dwgs.User are drawn mirrored the
//      other way (read as before), a text box on F.SilkS is not; a text on a user layer Board Setup typed "front" is side specific and is not either, one on a plain user
//      layer is
//   2  Generate Placement Rule Areas: every sheet row names its file -- a footprint on the root sheet the project's `<name>.kicad_sch`, not an empty cell
//   3  Track & Via Properties has KiCad's Pre-defined sizes row for a via (Board Setup's via sizes; a choice fills the diameter and the hole)
//   4  a board read with legacy teardrops (`(legacy_teardrops yes)`): the Properties panel has no Teardrops rows for its via, Track & Via Properties greys its Teardrops part
//      under "Board contains legacy teardrops."; Add All Teardrops (Edit Teardrops) ends it: the rows and the part come back
//
// The script's value is { results: [{ name, ok, notes }] }.
// run-all-checks: board=mcu30-placed minutes=8   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  const board = () => json("/api/state");
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(400); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const waitFor = async (cond, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(200); } return false; };
  const dialog = (label) => document.querySelector(`[role="dialog"][aria-label="${label}"]`);
  const closeDialog = async (label) => { const d = dialog(label); const b = d && [...d.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (b) b.click(); await sleep(300); };
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const drawn = (id) => window.__edaText?.pcb?.[id];
  const scenario = async (name, body) => {
    const notes = [];
    const t = (ok, what) => notes.push(`${ok ? "ok" : "FAIL"}: ${what}`);
    try {
      await body(t);
    } catch (e) {
      notes.push(`FAIL: threw ${String((e && e.stack) || e)}`);
    }
    results.push({ name, ok: notes.length > 0 && notes.every((n) => n.startsWith("ok")), notes });
  };

  // ------------------------------------------------------------------------------------------------------------------ 1. Flip Board View
  await scenario("Flip Board View: text boxes, table cells and texts on a user layer typed front", async (t) => {
    const st0 = await board();
    const enabled = st0.board_rules?.layer_setup?.enabled ?? [];
    const types = { ...(st0.board_rules?.layer_setup?.types ?? {}), "User.1": "front" };
    const setup = { ...(st0.board_rules?.layer_setup ?? {}), enabled: [...new Set([...enabled, "User.1", "User.2", "Dwgs.User"])], types };
    t((await cmd({ op: "set_board_layers", settings: setup })).ok !== false, "Board Setup: User.1 typed Off-board, front");
    const ol = st0.outline ?? [[0, 0], [50e6, 40e6]];
    const [x0, y0] = [Math.min(...ol.map((p) => p[0])) + 2e6, Math.min(...ol.map((p) => p[1])) + 2e6];
    const box = (layer, dy, text) => ({ op: "pcb_item", verb: "add_text_box", text_box: { layer, text, start: { x: x0, y: y0 + dy }, end: { x: x0 + 12e6, y: y0 + dy + 4e6 }, margin_left_nm: 500000, margin_top_nm: 500000, margin_right_nm: 500000, margin_bottom_nm: 500000, border: true, stroke_width_nm: 100000, size_nm: 1000000 } });
    const text = (layer, dy, content) => ({ op: "add_text", text: { content, at: { x: x0 + 20e6, y: y0 + dy }, angle: 0, layer, size_nm: 1000000, stroke_width: 150000, justify: "left", mirror: false } });
    for (const c of [box("Dwgs.User", 0, "USER NOTE"), box("F.SilkS", 6e6, "SILK NOTE"), text("User.1", 0, "FRONT TYPED"), text("User.2", 6e6, "PLAIN USER")]) {
      const r = await cmd(c);
      t(r.ok !== false, `${c.op} ${c.text_box?.layer ?? c.text?.layer}: ${r.ok === false ? JSON.stringify(r).slice(0, 200) : "made"}`);
    }
    await cmd({ op: "table", verb: "draw", layer: "Dwgs.User", at: { x: x0, y: y0 + 14e6 }, cols: 2, rows: 1, cell_w_nm: 8e6, cell_h_nm: 3e6, line_width_nm: 100000, patch: { texts: [["CELL A", "CELL B"]] } });
    await sleep(800);
    const st = await board();
    const tb = (st.drawings?.text_boxes ?? []).filter((b) => b.text === "USER NOTE" || b.text === "SILK NOTE");
    const user = tb.find((b) => b.text === "USER NOTE");
    const silk = tb.find((b) => b.text === "SILK NOTE");
    const front = (st.drawings?.texts ?? []).find((x) => x.content === "FRONT TYPED");
    const plain = (st.drawings?.texts ?? []).find((x) => x.content === "PLAIN USER");
    const table = (st.drawings?.tables ?? []).find((x) => x.layer === "Dwgs.User" && x.cells.some((c) => c.text === "CELL A"));
    t(!!user && !!silk && !!front && !!plain && !!table, "the two text boxes, the two texts and the table are on the board");
    if (!user || !silk || !front || !plain || !table) return;
    const cell = table.cells.find((c) => c.text === "CELL A");
    await E.run("pcbnew.Control.flipBoard");
    await sleep(900);
    const ok = await waitFor(() => drawn(user.id)?.drawnMirrored === true, 4000);
    t(ok, `a Dwgs.User text box is drawn mirrored the other way, so it reads as it did (mirror=${drawn(user.id)?.drawnMirrored})`);
    t(drawn(silk.id)?.drawnMirrored === false, `an F.SilkS text box is not (mirror=${drawn(silk.id)?.drawnMirrored})`);
    t(drawn(cell.id)?.drawnMirrored === true, `a Dwgs.User table cell is drawn as a text box is (mirror=${drawn(cell.id)?.drawnMirrored})`);
    t(drawn(front.id)?.drawnMirrored === false, `a text on User.1, typed front, is side specific: not mirrored (mirror=${drawn(front.id)?.drawnMirrored})`);
    t(drawn(plain.id)?.drawnMirrored === true, `a text on User.2, a plain user layer, is (mirror=${drawn(plain.id)?.drawnMirrored})`);
    await E.run("pcbnew.Control.flipBoard");
    await sleep(900);
    t(drawn(user.id)?.drawnMirrored === false && drawn(front.id)?.drawnMirrored === false, "Flip Board View off: drawn as they are");
  });

  // ------------------------------------------------------------------------------------------------------------------ 2. Generate Placement Rule Areas
  await scenario("Generate Placement Rule Areas: every sheet row names its file", async (t) => {
    // the dialog's rows (`GeneratePotentialRuleAreas`, POST /api/multichannel/areas); KiCad opens the dialog only for a board of two sheets or more
    const r = await json("/api/multichannel/areas", { method: "POST" });
    const rows = (r.areas ?? []).filter((a) => a.kind === "sheet").map((a) => [a.key, a.file]);
    t(rows.length > 0, `sheet rows: ${JSON.stringify(rows).slice(0, 400)}`);
    t(rows.every((x) => /\.kicad_sch$/.test(x[1] ?? "")), "each with its file -- the root's the project's .kicad_sch");
    if (rows.length > 1) {
      await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
      if (await waitFor(() => !!tid("mc-generate-dialog"), 6000)) {
        tid("mc-generate-tab-sheet")?.click();
        await sleep(500);
        const cells = [...document.querySelectorAll('[data-testid^="mc-generate-row-sheet-"]')].map((x) => [...x.querySelectorAll("td")].map((c) => c.textContent.trim()));
        t(cells.length === rows.length && cells.every((c) => /\.kicad_sch$/.test(c[2] ?? "")), `the dialog's Sheet Name column: ${JSON.stringify(cells).slice(0, 300)}`);
        [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Cancel" && b.closest(".dialog"))?.click();
        await sleep(300);
      }
    }
  });

  // ------------------------------------------------------------------------------------------------------------------ 3. Pre-defined sizes
  await scenario("Track & Via Properties: Pre-defined sizes of a via", async (t) => {
    const st = await board();
    const presets = [{ diameter: 800000, drill: 400000 }, { diameter: 1000000, drill: 500000 }];
    await cmd({ op: "set_via_presets", presets });
    const net = (st.nets ?? []).find((n) => /GND$/i.test(n)) ?? st.nets?.[0] ?? "";
    const ol = st.outline ?? [[0, 0]];
    const [vx, vy] = [Math.min(...ol.map((p) => p[0])) + 30e6, Math.min(...ol.map((p) => p[1])) + 30e6];
    await cmd({ op: "add_via", net, x: vx, y: vy, drill: 300000, diameter: 600000, from_layer: "F.Cu", to_layer: "B.Cu" });
    await sleep(500);
    const via = (await board()).routing.vias.find((v) => v.x === vx && v.y === vy);
    t(!!via, "a via");
    if (!via) return;
    await select([via.id]);
    await E.run("pcbnew.InteractiveEdit.properties");
    await waitFor(() => !!dialog("Track & Via Properties"), 4000);
    const d = dialog("Track & Via Properties");
    const pre = d?.querySelector('[aria-label="Pre-defined via sizes"]');
    t(!!pre && pre.options.length === 3, `the row lists Board Setup's sizes (${pre ? [...pre.options].map((o) => o.textContent).join(" | ") : "no row"})`);
    if (pre) {
      setValue(pre, "1");
      await sleep(200);
      const dia = d.querySelector('[aria-label="Via diameter"]')?.value;
      const hole = d.querySelector('[aria-label="Via hole"]')?.value;
      t(Number(dia) === 1 && Number(hole) === 0.5, `a choice fills the diameter and the hole (${dia} / ${hole})`);
    }
    await closeDialog("Track & Via Properties");
  });

  // ------------------------------------------------------------------------------------------------------------------ 4. legacy teardrops
  await scenario("A board with legacy teardrops: no Teardrops rows, the dialog's part greyed with KiCad's warning, until Edit Teardrops", async (t) => {
    const file = `(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
  (general (thickness 1.6) (legacy_teardrops yes))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen") (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "GND")
  (gr_rect (start 0 0) (end 40 30) (stroke (width 0.1) (type default)) (fill no) (layer "Edge.Cuts") (uuid "6a1f0000-0000-0000-0000-000000000001"))
  (segment (start 10 10) (end 20 10) (width 0.25) (layer "F.Cu") (net 1) (uuid "6a1f0000-0000-0000-0000-000000000002"))
  (via (at 10 10) (size 0.8) (drill 0.4) (layers "F.Cu" "B.Cu") (net 1) (uuid "6a1f0000-0000-0000-0000-000000000003"))
  (zone (net 1) (net_name "GND") (layer "F.Cu") (uuid "6a1f0000-0000-0000-0000-000000000004") (name "$teardrop_padvia$") (hatch full 0.1) (priority 30000)
    (attr (teardrop (type padvia))) (connect_pads yes (clearance 0)) (min_thickness 0.0254) (filled_areas_thickness no)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1) (island_area_min 10))
    (polygon (pts (xy 10.9 9.875) (xy 10.9 10.125) (xy 10.4 10.39) (xy 9.6 10.39) (xy 9.6 9.61) (xy 10.4 9.61)))))
`;
    const r = await cmd({ op: "replace_board", text: file, name: "legacy-teardrops" });
    t(r.ok !== false, `the board is read (${r.ok === false ? JSON.stringify(r).slice(0, 300) : "ok"})`);
    await sleep(1500);
    let st = await board();
    t(st.drawings?.legacy_teardrops === true, "the state says the board has legacy teardrops");
    const via = st.routing?.vias?.[0];
    t(!!via, "its via");
    if (!via) return;
    await select([via.id]);
    await waitFor(() => !!document.querySelector(".prop-row"), 4000);
    t(!!document.querySelector(".prop-row") && !row("Enable Teardrops"), `the Properties panel has no Teardrops rows (rows: ${[...document.querySelectorAll(".prop-row")].map((x) => x.dataset.prop).slice(0, 12).join(", ")}..)`);
    await E.run("pcbnew.InteractiveEdit.properties");
    await waitFor(() => !!dialog("Track & Via Properties"), 4000);
    let fs = dialog("Track & Via Properties")?.querySelector('fieldset[aria-label="Teardrops"]');
    t(!!fs && fs.disabled, "Track & Via Properties' Teardrops part is greyed");
    t(/Board contains legacy teardrops\./.test(fs?.textContent ?? "") && /Use Edit > Edit Teardrops to apply automatic teardrops\./.test(fs?.textContent ?? ""), "under KiCad's warning, word for word");
    await closeDialog("Track & Via Properties");
    // Edit Teardrops' OK (Board Setup's Teardrops page: Add All Teardrops) ends the legacy board
    await cmd({ op: "add_all_teardrops" });
    await sleep(1200);
    st = await board();
    t(!st.drawings?.legacy_teardrops, "Edit Teardrops clears the flag (DIALOG_GLOBAL_EDIT_TEARDROPS::TransferDataFromWindow)");
    await select([]);
    await select([via.id]);
    await waitFor(() => !!row("Enable Teardrops"), 4000);
    t(!!row("Enable Teardrops"), "the Teardrops rows are back");
    await E.run("pcbnew.InteractiveEdit.properties");
    await waitFor(() => !!dialog("Track & Via Properties"), 4000);
    fs = dialog("Track & Via Properties")?.querySelector('fieldset[aria-label="Teardrops"]');
    t(!!fs && !fs.disabled && !/legacy teardrops/.test(fs.textContent), "and the dialog's part is live, no warning");
    await closeDialog("Track & Via Properties");
  });

  return { results };
})();
