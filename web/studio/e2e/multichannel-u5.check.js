// Multichannel check, UI-LEFT P20 (task-u5-pcbitems): what Repeat Layout copies besides the footprints and the copper -- tables, barcodes, tuning patterns, zones with holes -- and what it
// leaves (reference images and points: KiCad's `A.enclosedByArea` finds no polygon of theirs), the holes of the areas, the options of the two dialogs kept as KiCad's tool keeps them
// (for the life of the board editor: the browser tab, across a reload of the page), and Save Selection as Design Block keeping a group of the selection.
// Run it in the page of a served board made from web/studio/e2e/multichannel-board.yaml (`eda board new web/studio/e2e/multichannel-board.yaml -o <dir>` then `eda board serve -C <dir>`),
// on a COPY of a board with none of its footprints placed: it places them, makes the rule areas, adds the items, repeats and undoes; it makes one design block library (a `.kicad_blocks`
// folder in the board's folder and a row in its design-block-lib-table), saves a block into it, and takes the block and the row out again (the empty folder stays). Strict is left off.
//
// What it checks:
//   the copy: a table, a barcode on F.Cu, a tuning pattern (its track floating on the channel's summing net), a GND pour with a hole, a reference image and a point are put inside channel
//     1's area; Repeat Layout (the dialog's OK) gives channels 2 and 3 a copy of the table, the barcode, the pattern (its track on the channel's own net) and the pour (its hole moved with
//     it), and no copy of the image or the point; the target area that had a hole has the reference's outline alone; one undo step;
//   the options: Cancel keeps nothing; OK keeps the six boxes but not the anchor; Generate opens with the Group box Repeat Layout left (one setting, `m_options.m_groupItems`) and keeps
//     Replace; the page loaded again in this tab (a frame of the studio: the tab's sessionStorage) opens both dialogs with the kept boxes; Generate's OK on the Groups tab turns the
//     shared Group box off;
//   Save Selection as Design Block: a named group and a footprint selected -> the block's board file has the group with its two members, and the board's new linked group holds the
//     group (nested) and the footprint.
//
// It answers through `window.__mcU5Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=multichannel minutes=12   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__mcU5Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 20000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), last: results.slice(-1).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const LIB = `McU5${Date.now().toString(36)}`;
  const KEY = "eda-studio.multichannel-tool.v1";
  const board = () => json("/api/state");
  const postJson = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const post = (cmd) => postJson("/api/cmd", { cmd, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const select = async (ids, e = E) => { await e.run("common.InteractiveSelection.clear"); if (ids.length > 0) await e.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const tid = (id, doc = document) => doc.querySelector(`[data-testid="${id}"]`);
  const dialog = (doc = document) => [...doc.querySelectorAll(".dialog-backdrop .dialog")].pop();
  const buttonOf = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const press = async (text, ms = 1500, doc = document) => { buttonOf(dialog(doc), text)?.click(); await sleep(ms); };
  const titles = () => E.state().dialogs;
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 5; i++) { const d = dialog(); if (!d) break; (buttonOf(d, "Cancel") ?? buttonOf(d, "OK") ?? buttonOf(d, "Close"))?.click(); await sleep(200); } };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const gotoTab = async (label) => { if (E.state().tab !== label.toLowerCase()) { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === label)?.click(); await sleep(1400); } };
  const errorsSince = (t0) => E.errors(t0).map((e) => e.message);

  const ruleAreas = (b) => (b.routing?.zones ?? []).filter((z) => z.is_rule_area);
  const copperZones = (b) => (b.routing?.zones ?? []).filter((z) => !z.is_rule_area);
  const tracksOf = (b) => b.routing?.tracks ?? [];
  const tablesOf = (b) => b.drawings?.tables ?? [];
  const barcodesOf = (b) => b.drawings?.barcodes ?? [];
  const imagesOf = (b) => b.drawings?.images ?? [];
  const pointsOf = (b) => b.drawings?.points ?? [];
  const patternsOf = (b) => b.drawings?.tuning_patterns ?? [];
  const groupsOf = (b) => (b.drawings?.groups ?? []).filter((g) => !g.generator);
  const box = (pts) => ({ x0: Math.min(...pts.map((p) => p[0])), y0: Math.min(...pts.map((p) => p[1])), x1: Math.max(...pts.map((p) => p[0])), y1: Math.max(...pts.map((p) => p[1])) });
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const snapshot = async () => {
    const b = await board();
    const sorted = (rows) => rows.map((r) => JSON.stringify(r)).sort();
    return JSON.stringify({
      parts: b.parts.map((p) => [p.ref, p.at, p.rot, p.side]),
      tracks: sorted(tracksOf(b).map((t) => [t.net, t.layer, t.width, t.pts])),
      zones: sorted((b.routing?.zones ?? []).map((z) => [z.name, z.net, z.is_rule_area, z.layers, z.outline, z.holes, z.placement])),
      tables: sorted(tablesOf(b).map((t) => [t.layer, t.x, t.y])),
      barcodes: sorted(barcodesOf(b).map((t) => [t.layer, t.x, t.y])),
      images: imagesOf(b).length,
      points: pointsOf(b).length,
      patterns: sorted(patternsOf(b).map((p) => [p.layer, p.origin, p.end, p.member_ids.length])),
      groups: sorted(groupsOf(b).map((g) => [g.name, g.design_block, g.member_ids.length])),
    });
  };
  const ensureSame = async (snap, max = 12) => { for (let i = 0; i < max && (await snapshot()) !== snap; i++) await undo(); return (await snapshot()) === snap; };

  const CH = { 1: { U: "U1", sum: "/OPAMP (U1)/SUM1" }, 2: { U: "U2", sum: "/OPAMP (U2)/SUM2" }, 3: { U: "U3", sum: "/OPAMP (U3)/SUM3" } };
  const PLACE = [
    ["U1", 5_500_000, 9_000_000], ["R1", 3_500_000, 4_200_000], ["R2", 7_700_000, 4_200_000], ["C1", 5_500_000, 13_200_000],
    ["U2", 18_000_000, 9_000_000], ["R3", 20_200_000, 4_200_000], ["R4", 15_800_000, 4_200_000], ["C2", 18_000_000, 14_000_000],
    ["U3", 30_500_000, 9_000_000], ["R5", 30_500_000, 4_200_000], ["R6", 32_500_000, 13_500_000], ["C3", 28_000_000, 13_500_000],
  ];
  const AREA = {};
  const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

  const only = window.__mcU5Only;
  const scenario = async (name, fn) => {
    if (only && !only.test(name)) return;
    const notes = [];
    const t = (ok, text) => { notes.push(`${ok ? "ok" : "FAIL"}: ${text}`); return ok; };
    const t0 = Date.now();
    try { await fn(t); } catch (e) { t(false, `threw ${e && e.stack ? e.stack.split("\n").slice(0, 3).join(" | ") : e}`); }
    try { await closeDialogs(); await escape(); } catch { /* ignore */ }
    results.push({ name, ok: notes.every((n) => n.startsWith("ok")), notes, ms: Date.now() - t0 });
  };

  // ------------------------------------------------------------------------------------------------------------------------ setup
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  try { sessionStorage.removeItem(KEY); } catch { /* no storage */ }
  await sleep(300);
  const errorsBefore = E.errors().length;
  await gotoTab("PCB");
  const b00 = await board();
  if (!PLACE.every(([r]) => b00.parts.some((p) => p.ref === r)) || ruleAreas(b00).length > 0) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: run it on a board made from web/studio/e2e/multichannel-board.yaml, with no rule area"] });
    check.done = true;
    return "no board";
  }
  const start = await snapshot();
  let afterItems = null;

  (async () => {
    await scenario("setup: the channels placed, their rule areas generated, channel 1's area a rectangle and channel 2's with a hole", async (t) => {
      for (const [part, x, y] of PLACE) {
        const p = (await board()).parts.find((q) => q.ref === part);
        if (p && !p.placed) await post({ op: "place_at", part, x, y });
      }
      await sleep(500);
      await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
      await waitFor(() => tid("mc-generate-dialog"));
      for (const e of document.querySelectorAll('[data-testid^="mc-generate-check-sheet-"]')) e.click();
      await sleep(150);
      await press("OK", 1800);
      const b = await board();
      for (const k of [1, 2, 3]) AREA[k] = ruleAreas(b).find((z) => z.placement?.source === `/OPAMP (U${k})/`)?.id;
      t([1, 2, 3].every((k) => !!AREA[k]), `three areas, one for each channel's sheet (${ruleAreas(b).map((z) => z.placement?.source)})`);
      const a1 = box(ruleAreas(b).find((z) => z.id === AREA[1]).outline);
      const g = 1_500_000;
      const rect = [{ x: a1.x0 - g, y: a1.y0 - g }, { x: a1.x1 + g, y: a1.y0 - g }, { x: a1.x1 + g, y: a1.y1 + g }, { x: a1.x0 - g, y: a1.y1 + g }];
      t((await post({ op: "set_zone_outline", id: AREA[1], outline: rect })).ok, "channel 1's area is made a rectangle 1.5 mm round its hull");
      const a2 = ruleAreas(b).find((z) => z.id === AREA[2]);
      const c2 = box(a2.outline);
      const hole = [[c2.x0 + 300_000, c2.y0 + 300_000], [c2.x0 + 900_000, c2.y0 + 300_000], [c2.x0 + 900_000, c2.y0 + 900_000], [c2.x0 + 300_000, c2.y0 + 900_000]].map(([x, y]) => ({ x, y }));
      t((await post({ op: "set_zone_outline", id: AREA[2], outline: a2.outline.map(([x, y]) => ({ x, y })), holes: [hole] })).ok, "channel 2's area has a hole");
      t((ruleAreas(await board()).find((z) => z.id === AREA[2])?.holes ?? []).length === 1, "and the board says so");
    });

    await scenario("setup: a table, a barcode, a tuning pattern, a pour with a hole, a reference image and a point inside channel 1's area", async (t) => {
      const a1 = box(ruleAreas(await board()).find((z) => z.id === AREA[1]).outline);
      const at = (dx, dy) => ({ x: a1.x0 + dx, y: a1.y0 + dy });
      const ok = async (what, cmd) => { const r = await post(cmd); t(!!r.ok, `${what} (${r.ok ? "added" : r.message})`); };
      await ok("a table on F.Cu", { op: "table", verb: "draw", layer: "F.Cu", at: at(200_000, 6_800_000), cols: 1, rows: 1, cell_w_nm: 900_000, cell_h_nm: 800_000, line_width_nm: 100_000 });
      await ok("a barcode on F.Cu", { op: "pcb_item", verb: "add_barcode", barcode: { layer: "F.Cu", at: at(750_000, 5_000_000), width_nm: 800_000, height_nm: 800_000, text: "CH", text_height_nm: 500_000, show_text: false } });
      await ok("a reference image", { op: "pcb_item", verb: "add_ref_image", image: { layer: "F.Cu", at: at(750_000, 9_000_000), data: PNG } });
      await ok("a point", { op: "pcb_item", verb: "add_point", point: { layer: "F.Cu", at: at(750_000, 10_000_000), size_nm: 1_000_000 } });
      const pts = [at(400_000, 1_000_000), at(400_000, 3_000_000)];
      await ok("a tuning pattern of a track on channel 1's summing net", {
        op: "commit_tuning",
        pattern: { mode: "single", layer: "F.Cu", origin: pts[0], end: pts[1], min_amplitude: 200_000, max_amplitude: 1_000_000, spacing: 600_000, base_line: pts, last_netname: CH[1].sum },
        tracks: [{ id: "", net: CH[1].sum, pins: [], layer: "F.Cu", width: 200_000, pts }],
        member_indices: [0],
      });
      const outline = [at(100_000, 10_900_000), at(1_400_000, 10_900_000), at(1_400_000, 12_900_000), at(100_000, 12_900_000)];
      await ok("a GND pour", { op: "add_zone", net: "GND", layer: "F.Cu", outline });
      const pour = copperZones(await board()).find((z) => z.net === "GND");
      const hole = [at(500_000, 11_500_000), at(1_000_000, 11_500_000), at(1_000_000, 12_200_000), at(500_000, 12_200_000)];
      if (pour) await ok("its hole", { op: "set_zone_outline", id: pour.id, outline, holes: [hole] });
      const b = await board();
      t(tablesOf(b).length === 1 && barcodesOf(b).length === 1 && imagesOf(b).length === 1 && pointsOf(b).length === 1 && patternsOf(b).length === 1 && copperZones(b).length === 1 && copperZones(b)[0].holes.length === 1, "the board has one of each");
      afterItems = await snapshot();
    });

    await scenario("Repeat Layout copies the table, the barcode, the pattern and the pour with its hole to channels 2 and 3, and leaves the image and the point, in one undo step", async (t) => {
      if (!afterItems) return void t(false, "the setup did not finish");
      const before = await board();
      await select([AREA[1]]);
      await E.run("pcbnew.Multichannel.repeatLayout");
      const d = await waitFor(() => tid("mc-repeat-dialog"));
      if (!t(!!d, "the dialog opens on channel 1's area")) return;
      t([2, 3].every((k) => tid(`mc-repeat-copy-${AREA[k]}`)?.checked), "both other channels match and are ticked");
      const t0 = Date.now();
      await press("OK", 2500);
      t(errorsSince(t0).length === 0, `no error (${JSON.stringify(errorsSince(t0))})`);
      const b = await board();
      const ref = box(ruleAreas(before).find((z) => z.id === AREA[1]).outline);
      const shift = {};
      for (const k of [2, 3]) {
        const z = ruleAreas(b).find((x) => x.id === AREA[k]);
        const bb = box(z.outline);
        shift[k] = { x: bb.x0 - ref.x0, y: bb.y0 - ref.y0 };
        t(z.holes.length === 0 && near(bb.x1 - bb.x0, ref.x1 - ref.x0) && near(bb.y1 - bb.y0, ref.y1 - ref.y0), `channel ${k}'s area has the reference's outline, no hole (${z.holes.length})`);
      }
      const table0 = tablesOf(before)[0];
      const bar0 = barcodesOf(before)[0];
      const pour0 = copperZones(before)[0];
      t(tablesOf(b).length === 3 && [2, 3].every((k) => tablesOf(b).some((x) => near(x.x, table0.x + shift[k].x) && near(x.y, table0.y + shift[k].y))), `a table in each channel, where channel 1's is (${tablesOf(b).length})`);
      t(barcodesOf(b).length === 3 && [2, 3].every((k) => barcodesOf(b).some((x) => near(x.x, bar0.x + shift[k].x) && near(x.y, bar0.y + shift[k].y))), `a barcode in each channel (${barcodesOf(b).length})`);
      t(imagesOf(b).length === 1 && pointsOf(b).length === 1, `the reference image and the point are not copied (${imagesOf(b).length}, ${pointsOf(b).length})`);
      const pours = copperZones(b).filter((z) => z.net === "GND");
      t(pours.length === 3 && [2, 3].every((k) => pours.some((z) => z.holes.length === 1 && near(z.holes[0][0][0], pour0.holes[0][0][0] + shift[k].x) && near(z.holes[0][0][1], pour0.holes[0][0][1] + shift[k].y))), `a pour in each channel, its hole moved with it (${pours.map((z) => z.holes.length)})`);
      const patterns = patternsOf(b);
      t(patterns.length === 3, `a tuning pattern in each channel (${patterns.length})`);
      for (const k of [2, 3]) {
        const mine = tracksOf(b).filter((x) => x.net === CH[k].sum);
        const p = patterns.find((q) => q.member_ids.some((m) => mine.some((x) => x.id === m)));
        t(!!p && mine.length === 1 && near(p.origin.x, patternsOf(before)[0].origin.x + shift[k].x), `channel ${k}'s pattern holds its copy of the track, on ${CH[k].sum}`);
      }
      await undo();
      t((await snapshot()) === afterItems, "one undo takes the whole repeat back");
    });

    await scenario("the options: Cancel keeps nothing, OK keeps the boxes but not the anchor, Generate shares the Group box and keeps Replace", async (t) => {
      await select([AREA[1]]);
      await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      const opt = (n) => tid(`mc-repeat-opt-${n}`);
      t(opt("copy_routing").checked && !opt("group_items").checked && opt("include_locked_items").checked && opt("copy_other_items").checked && opt("copy_placement").checked, "the dialog opens on KiCad's defaults");
      opt("copy_routing").click();
      await sleep(100);
      await press("Cancel", 500);
      await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      t(opt("copy_routing").checked, "Cancel kept nothing");
      opt("copy_routing").click();
      opt("group_items").click();
      opt("include_locked_items").click();
      const anchor = tid("mc-repeat-anchor");
      setValue(anchor, "U1");
      await sleep(150);
      await press("OK", 2500);
      await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      t(!opt("copy_routing").checked && opt("group_items").checked && !opt("include_locked_items").checked && opt("copy_other_items").checked, "OK kept the boxes");
      t(tid("mc-repeat-anchor").value === "", "and the anchor opens empty again (`SetSelection( 0 )`)");
      await press("Cancel", 400);
      await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
      await waitFor(() => tid("mc-generate-dialog"));
      t(tid("mc-generate-group").checked && !tid("mc-generate-replace").checked, "Generate's Group box is the one Repeat Layout left on");
      tid("mc-generate-replace").click();
      await sleep(100);
      await press("OK", 1200); // nothing ticked: nothing made, the boxes kept (`TransferDataFromWindow`)
      let kept = null;
      try { kept = JSON.parse(sessionStorage.getItem(KEY) ?? "null"); } catch { /* none */ }
      t(kept?.replaceExisting === true && kept?.options?.group_items === true && kept?.options?.copy_routing === false && kept?.options?.anchor === null, `kept in this tab's session (${JSON.stringify(kept)})`);
      try { t(localStorage.getItem(KEY) === null, "and not in the browser's long-lived storage"); } catch { /* none */ }
    });

    await scenario("the options are kept when the page is loaded again in this tab (a frame of the studio: the tab's session), and Generate on the Groups tab turns the shared Group box off", async (t) => {
      const frame = document.createElement("iframe");
      frame.style.cssText = "position:fixed;left:0;top:0;width:1200px;height:800px;opacity:0.01;z-index:-1";
      frame.src = location.href;
      document.body.appendChild(frame);
      try {
        const fe = await waitFor(() => frame.contentWindow?.__eda, 60000);
        if (!t(!!fe, "the studio loads again in the frame")) return;
        await sleep(2500);
        const doc = frame.contentDocument;
        if (fe.state().tab !== "pcb") { [...doc.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(1500); }
        await select([AREA[1]], fe);
        await fe.run("pcbnew.Multichannel.repeatLayout");
        await waitFor(() => tid("mc-repeat-dialog", doc), 8000);
        const opt = (n) => tid(`mc-repeat-opt-${n}`, doc);
        t(!!opt("copy_routing") && !opt("copy_routing").checked && opt("group_items").checked && !opt("include_locked_items").checked && opt("copy_placement").checked, "the reloaded page's Repeat Layout opens with the kept boxes");
        await press("Cancel", 400, doc);
        await fe.run("pcbnew.Multichannel.generatePlacementRuleAreas");
        await waitFor(() => tid("mc-generate-dialog", doc), 8000);
        t(tid("mc-generate-replace", doc)?.checked === true && tid("mc-generate-group", doc)?.checked === true, "and its Generate with Replace and Group on");
        await press("Cancel", 400, doc);
      } finally {
        frame.remove();
        await sleep(500);
      }
      await gotoTab("PCB");
      await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
      await waitFor(() => tid("mc-generate-dialog"));
      tid("mc-generate-tab-group").click();
      await sleep(200);
      await press("OK", 1200);
      await select([AREA[1]]);
      await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      t(!tid("mc-repeat-opt-group_items").checked && !tid("mc-repeat-opt-copy_routing").checked, "Generate's OK on the Groups tab turned the Group box off for Repeat Layout too, the other boxes stay");
      await press("Cancel", 400);
      await ensureSame(afterItems);
    });

    await scenario("Save Selection as Design Block keeps a group of the selection in the block, and the block's group holds it nested", async (t) => {
      await ensureSame(afterItems);
      await E.run("pcbnew.PcbDesignBlockControl.showDesignBlockPanel");
      await sleep(600);
      await E.run("common.Control.newLibrary");
      const d = await waitFor(() => (titles().includes("New Design Block Library") ? dialog() : null));
      if (!t(!!d, "New Library opens its dialog")) return;
      const ins = d.querySelectorAll("input");
      setValue(ins[1], LIB);
      setValue(ins[2], "Multichannel u5 check");
      document.querySelector('[data-library-file-table] input[value="project"]')?.click(); // KiCad's hook: the project table (UI-LEFT-2 LL5)
      await sleep(200);
      await press("Create", 1800);
      await post({ op: "group", ids: ["R1", "C1"] });
      const g = groupsOf(await board()).find((x) => x.member_ids.includes("R1"));
      await post({ op: "edit_group", id: g.id, name: "Pair", member_ids: g.member_ids });
      await select([g.id, "U1"]);
      const sel = (E.state().selection ?? []).map((x) => x.id ?? x);
      t(sel.includes(g.id) && sel.includes("U1"), `the group and U1 are selected (${JSON.stringify(sel)})`);
      await E.run("pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock");
      const props = await waitFor(() => (titles().includes("Design Block Properties") ? dialog() : null), 8000);
      if (!t(!!props, "Save Selection as Design Block opens the properties")) return;
      setValue(props.querySelector("#db-name"), "Grouped");
      await sleep(200);
      await press("Save", 3000);
      const file = await json(`/api/design_blocks/file?lib=${LIB}&name=Grouped&what=pcb`);
      const groupBlock = file.ok ? (file.text.match(/\(group "Pair"[\s\S]*?\(members([^)]*)\)/) ?? [])[1] : null;
      t(!!groupBlock && (groupBlock.match(/"[0-9a-f-]{36}"/g) ?? []).length === 2, `the block's board file has the group "Pair" with its two members (${groupBlock?.trim().slice(0, 90)})`);
      t(file.ok && (file.text.match(/\(footprint "/g) ?? []).length === 3, "and the three footprints, nothing else of the board's");
      const linked = groupsOf(await board()).find((x) => x.design_block === `${LIB}:Grouped`);
      t(!!linked && linked.member_ids.includes(g.id) && linked.member_ids.includes("U1"), `the block's group holds the group and U1 (${JSON.stringify(linked?.member_ids)})`);
      // clean up: the link and the group, the block, the library's row
      await ensureSame(afterItems, 4);
      await gotoTab("PCB");
      const rowOf = (id) => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].find((e) => (e.getAttribute("data-id") ?? e.getAttribute("data-library")) === id);
      if (!rowOf(`${LIB}:Grouped`)) { rowOf(LIB)?.click(); await sleep(300); }
      rowOf(`${LIB}:Grouped`)?.click();
      await sleep(400);
      await E.run("pcbnew.PcbDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      await press("Yes", 1800);
      await E.run("common.SuiteControl.showDesignBLockLibTable");
      await waitFor(() => document.querySelector(".lt-grid"), 4000);
      for (let guard = 0; guard < 6; guard++) {
        const at = [...document.querySelectorAll('.lt-grid [data-col="nickname"]')].findIndex((i) => i.value === LIB);
        if (at < 0) break;
        document.querySelector(`.lt-grid [data-row="${at}"][data-col="nickname"]`).closest("tr").dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
        await sleep(100);
        dialog().querySelector('[aria-label="Remove library from table"]')?.click();
        await sleep(150);
      }
      await press("OK", 1500);
      t(!(await json("/api/design_blocks")).libraries.some((l) => l.name === LIB), "the block and the library's row are gone again");
    });

    await scenario("clean up: the board is as the check found it", async (t) => {
      t(await ensureSame(start, 60), "every edit is undone");
      const bad = E.errors().slice(errorsBefore).map((e) => e.message);
      t(bad.length === 0, `no errors ${JSON.stringify(bad.map((m) => m.slice(0, 90)))}`);
    });

    try { sessionStorage.removeItem(KEY); } catch { /* none */ }
    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
