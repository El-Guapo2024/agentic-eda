// Multichannel check (GAPS.md items 24 and 32, package P24): the multichannel tool -- placement rule areas, Generate Placement Rule Areas, Repeat Layout and Apply Design Block Layout --
// driven the way a person does it through `window.__eda`, the dialogs' controls (their `data-testid`s) and the menu, and checked against what the server holds (`/api/state`).
// Run it in the page of a served board made from web/studio/e2e/multichannel-board.yaml (`eda board new web/studio/e2e/multichannel-board.yaml -o <dir>` then `eda board serve -C <dir>`):
// three identical amplifier channels, each an op-amp, an input resistor, a feedback resistor and a capacitor, the studio's module sheets being one sheet per channel and the project
// giving each footprint a component class for its sheet. Paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright, on a COPY of a board that has none of
// its footprints placed (it places them, lays channel 1 out with copper, and edits and undoes from there; it makes one design block library, a `.kicad_blocks` folder in the board's folder
// and a row in its design-block-lib-table, saves a block into it, and takes the block and the row out again -- the empty folder stays). Strict is left off while it runs.
// `window.__multichannelOnly = /regexp/` runs only the scenarios whose names match (the ones after "setup" need what the scenarios before them leave).
//
// What it checks:
//   the actions: the three are wired on the PCB tab and on no other; Tools > Multi-Channel has the two menu entries; with no footprint placed Generate says there is nothing to generate;
//     Strict (which refuses a move that makes the board worse) does not refuse Generate on a board with no routing;
//   Generate Placement Rule Areas: the dialog's three tabs (Sheets, Component Classes, Groups) with a row for each channel, one rule area for each row ticked (named, with its placement
//     source, on the copper layers, its outline the rectilinear hull of the footprints of the channel, around every one of them), nothing made when nothing is ticked, one undo step; a row
//     whose area is there already is left alone, and made again with "Replace existing placement rule areas"; "Group footprints with their placement rule areas" groups each area with its
//     footprints;
//   Rule Area Properties: the Placement section (No placement; a sheet, a component class or a group), the source being the one the area was made from, a change of it kept and undone;
//   Repeat Layout: the reference area the selection names (a rule area, or a group that holds one), else the picker that asks for one ("Select a reference Rule Area to copy from...");
//     the table of target areas with the match of each, the mismatch details in KiCad's words for an area whose footprints do not match; what OK does to the other channels -- the footprints
//     laid out as the reference's are (moved by the difference of the areas' centres, or about the anchor footprint and turned with it), the routing, the zones and the other items of the
//     area copied onto the target's nets, what the target had in its area taken away and what it had outside left, one undo step -- and what each option leaves alone (Copy footprint
//     placement, Copy routing, Copy other items, Include locked items);
//   Apply Design Block Layout: a block saved from the laid-out channel is applied to the group of another channel linked to it: its footprints laid out as the block's, the copper and the other
//     items of the block copied onto the group's nets and joined to the group, nothing of the board's own taken away, one undo step; a group with a locked footprint, or one whose
//     footprints do not match the block's, is refused in KiCad's words.
//
// It answers through `window.__multichannelCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }. A run takes about three minutes: read it with
// `await __multichannelCheck.wait(20000)` (it returns when the run is done or the time is up; the Browser pane's javascript tool gives up after 45 s) until `.done`.
// run-all-checks: board=multichannel minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__multichannelCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 20000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok), last: results.slice(-1).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const LIB = `McCheck${Date.now().toString(36)}`; // a name no earlier run used: the empty library folder stays in the board's folder
  const board = () => json("/api/state");
  const postJson = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const post = (cmd) => postJson("/api/cmd", { cmd, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const redo = async () => { await E.run("common.Interactive.redo"); await sleep(700); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dialog = () => [...document.querySelectorAll(".dialog-backdrop .dialog")].pop();
  const buttonOf = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const press = async (text, ms = 1500) => { buttonOf(dialog(), text)?.click(); await sleep(ms); };
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
  /** Open Generate with the boxes at `replace` and `group` (the tool keeps the last ones), and tick every row of the Sheets tab when `tick`. */
  const openGenerate = async (replace, group, tick = true) => {
    const r = await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
    await waitFor(() => tid("mc-generate-dialog"));
    if (tid("mc-generate-replace").checked !== replace) tid("mc-generate-replace").click();
    if (!tid("mc-generate-group").disabled && tid("mc-generate-group").checked !== group) tid("mc-generate-group").click();
    if (tick) for (const e of document.querySelectorAll('[data-testid^="mc-generate-check-sheet-"]')) e.click();
    await sleep(150);
    return r;
  };
  const toastText = () => document.body.innerText;
  const errorsSince = (t0) => E.errors(t0).map((e) => e.message);
  /** The item of a menu: the entries of the open menu, by their label. */
  const menuItem = (label) => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].find((e) => e.querySelector("span")?.textContent.trim() === label);
  const menuOpen = async (name) => { [...document.querySelectorAll(".menubar .menubar-item")].find((e) => e.textContent.trim() === name)?.click(); await sleep(400); };
  const menuClose = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(200); };

  // the board
  const near = (a, b, tol = 2_000) => Math.abs(a - b) <= tol;
  const poseOf = (b, ref) => { const p = b.parts.find((q) => q.ref === ref); return p && p.placed ? { x: p.at[0], y: p.at[1], rot: p.rot, side: p.side } : null; };
  /** The place of pad `num` of footprint `ref`, as the server works it out from the footprint's pose. */
  const padOf = (b, ref, num) => { const q = b.parts.find((x) => x.ref === ref)?.pads?.find((x) => x.num === num); return q ? { x: q.x, y: q.y } : null; };
  const angle = (a) => ((a % 360) + 360) % 360;
  const sameAngle = (a, b) => near(angle(a), angle(b), 0.01) || near(Math.abs(angle(a) - angle(b)), 360, 0.01);
  const ruleAreas = (b) => (b.routing?.zones ?? []).filter((z) => z.is_rule_area);
  const copperZones = (b) => (b.routing?.zones ?? []).filter((z) => !z.is_rule_area);
  const tracksOf = (b) => b.routing?.tracks ?? [];
  const viasOf = (b) => b.routing?.vias ?? [];
  const textsOf = (b) => b.drawings?.texts ?? [];
  const groupsOf = (b) => b.drawings?.groups ?? [];
  const inside = (poly, [px, py]) => {
    let c = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [xi, yi] = poly[i];
      const [xj, yj] = poly[j];
      if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) c = !c;
    }
    return c;
  };
  const rectilinear = (poly) => poly.every((p, i) => { const q = poly[(i + 1) % poly.length]; return p[0] === q[0] || p[1] === q[1]; });
  /** The board as a string: what a scenario that edits and undoes compares. */
  const snapshot = async () => {
    const b = await board();
    const sorted = (rows) => rows.map((r) => JSON.stringify(r)).sort();
    return JSON.stringify({
      parts: b.parts.map((p) => [p.ref, p.at, p.rot, p.side]),
      tracks: sorted(tracksOf(b).map((t) => [t.net, t.layer, t.width, t.pts])),
      vias: sorted(viasOf(b).map((v) => [v.net, v.x, v.y, v.from, v.to])),
      zones: sorted((b.routing?.zones ?? []).map((z) => [z.name, z.net, z.is_rule_area, z.layers, z.outline, z.placement])),
      texts: sorted(textsOf(b).map((t) => [t.content, t.x, t.y, t.layer])),
      groups: sorted(groupsOf(b).map((g) => [g.name, g.design_block, [...g.member_ids].length])),
      locked: b.locked,
    });
  };
  const ensureSame = async (snap, max = 10) => { for (let i = 0; i < max && (await snapshot()) !== snap; i++) await undo(); return (await snapshot()) === snap; };

  // the channels: roles of each, and the nets of the roles, under KiCad's names (the labels SUM1, OUT1 in the sheet of U1: `/OPAMP (U1)/SUM1`; IN2, a net of
  // one pin, `unconnected-(R3-Pad1)`)
  const CH = {
    1: { U: "U1", Rin: "R1", Rf: "R2", C: "C1", sum: "/OPAMP (U1)/SUM1", out: "/OPAMP (U1)/OUT1" },
    2: { U: "U2", Rin: "R3", Rf: "R4", C: "C2", sum: "/OPAMP (U2)/SUM2", out: "/OPAMP (U2)/OUT2" },
    3: { U: "U3", Rin: "R5", Rf: "R6", C: "C3", sum: "/OPAMP (U3)/SUM3", out: "/OPAMP (U3)/OUT3" },
  };
  const ROLES = ["U", "Rin", "Rf", "C"];
  const refsOf = (k) => ROLES.map((r) => CH[k][r]);
  // where the footprints are put: channel 1 compact, the others scattered in ways of their own (nm)
  const PLACE = [
    ["U1", 5_500_000, 9_000_000], ["R1", 3_500_000, 4_200_000], ["R2", 7_700_000, 4_200_000], ["C1", 5_500_000, 13_200_000],
    ["U2", 18_000_000, 9_000_000], ["R3", 20_200_000, 4_200_000], ["R4", 15_800_000, 4_200_000], ["C2", 18_000_000, 14_000_000],
    ["U3", 30_500_000, 9_000_000], ["R5", 30_500_000, 4_200_000], ["R6", 32_500_000, 13_500_000], ["C3", 28_000_000, 13_500_000],
  ];
  /** Channel `k` of board `b` is channel 1 moved by one vector: the vector, or null when it is not. */
  const shiftOf = (b, k) => {
    const u1 = poseOf(b, CH[1].U);
    const uk = poseOf(b, CH[k].U);
    const v = { x: uk.x - u1.x, y: uk.y - u1.y };
    for (const role of ROLES) {
      const a = poseOf(b, CH[1][role]);
      const c = poseOf(b, CH[k][role]);
      if (!near(c.x - a.x, v.x) || !near(c.y - a.y, v.y) || !sameAngle(c.rot, a.rot) || c.side !== a.side) return null;
    }
    return v;
  };
  /** Channel `k` is channel 1 turned by `turn` degrees about its anchor `U` (the distances to it and the orientation relative to it). */
  const turnedBy = (b, k) => {
    const turn = angle(poseOf(b, CH[k].U).rot - poseOf(b, CH[1].U).rot);
    for (const role of ROLES) {
      if (role === "U") continue;
      const a = poseOf(b, CH[1][role]);
      const c = poseOf(b, CH[k][role]);
      const u1 = poseOf(b, CH[1].U);
      const uk = poseOf(b, CH[k].U);
      const d1 = Math.hypot(a.x - u1.x, a.y - u1.y);
      const dk = Math.hypot(c.x - uk.x, c.y - uk.y);
      if (!near(d1, dk, 3) || !sameAngle(c.rot - a.rot, turn)) return null;
    }
    return turn;
  };
  const moved = (a, b, refs) => refs.filter((r) => { const p = poseOf(a, r); const q = poseOf(b, r); return p.x !== q.x || p.y !== q.y || p.rot !== q.rot; });
  const shifted = (pts, v) => pts.map(([x, y]) => [x + v.x, y + v.y]);
  /** The segments of some tracks as strings, each with its net mapped by `net`, sorted: a track file holds segments, so a polyline of the studio and its segments are one and the same copper. */
  const segmentsOf = (tracks, net = (n) => n, by = { x: 0, y: 0 }) =>
    tracks
      .flatMap((x) => x.pts.slice(1).map((q, i) => {
        const [a, b] = [[x.pts[i][0] + by.x, x.pts[i][1] + by.y], [q[0] + by.x, q[1] + by.y]].sort((m, n) => m[0] - n[0] || m[1] - n[1]);
        return JSON.stringify([net(x.net), x.layer, x.width, a, b]);
      }))
      .sort();
  const AREA = {}; // channel -> id of its placement rule area (set by the Generate scenarios)
  const SHEET = {}; // channel -> the sheet path its rows have (`/OPAMP (U1)/`)

  const only = window.__multichannelOnly;
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
  await sleep(300);
  const errorsBefore = E.errors().length;
  await gotoTab("PCB");
  const b00 = await board();
  if (!["U1", "U2", "U3", "R1", "R2", "R3", "R4", "R5", "R6", "C1", "C2", "C3"].every((r) => b00.parts.some((p) => p.ref === r))) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs the three channels of web/studio/e2e/multichannel-board.yaml"] });
    check.done = true;
    return "no board";
  }
  if (ruleAreas(b00).length > 0) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board has placement rule areas already: run it on a board made from the intent"] });
    check.done = true;
    return "rule areas";
  }

  (async () => {
    // ======================================================================================================================== the actions
    await scenario("the actions are wired on the PCB tab and on no other, and Tools > Multi-Channel has the two entries", async (t) => {
      const ids = ["pcbnew.Multichannel.generatePlacementRuleAreas", "pcbnew.Multichannel.repeatLayout", "pcbnew.InteractiveDrawing.applyDesignBlockLayout"];
      let acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      t(ids.every((id) => acts[id]?.enabled), `on the PCB tab: ${ids.map((id) => `${acts[id]?.label}=${acts[id]?.enabled}`).join(", ")}`);
      await gotoTab("Schematic");
      acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      t(ids.every((id) => !acts[id]?.enabled), "on the schematic tab none of them is");
      await gotoTab("PCB");
      await menuOpen("Tools");
      const labels = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].map((e) => e.querySelector("span")?.textContent.trim());
      const submenus = [...document.querySelectorAll(".menubar-dropdown > *")].map((e) => (e.firstElementChild?.textContent ?? e.textContent).trim());
      t(submenus.some((l) => l.startsWith("Multi-Channel")) && labels.includes("Generate Placement Rule Areas...") && labels.includes("Repeat Layout..."), `Tools > Multi-Channel > Generate Placement Rule Areas... and Repeat Layout... (${[...submenus.filter((l) => /Multi/.test(l)), ...labels.filter((l) => /Rule Areas|Repeat/.test(l))].join(" | ")})`);
      t(menuItem("Generate Placement Rule Areas...")?.getAttribute("aria-disabled") === "false" && menuItem("Repeat Layout...")?.getAttribute("aria-disabled") === "false", "both entries are enabled");
      await menuClose();
    });

    await scenario("with no footprint placed there is nothing to generate, and Generate says so in KiCad's words without a dialog", async (t) => {
      if ((await board()).parts.some((p) => p.placed)) return void t(true, "(skipped: footprints are placed already)");
      const t0 = Date.now();
      await menuOpen("Tools");
      menuItem("Generate Placement Rule Areas...")?.click();
      await sleep(900);
      t(titles().length === 0, `no dialog opens (${titles()})`);
      t(errorsSince(t0).some((m) => m === "Cannot auto-generate any placement areas because the schematic has only one or no hierarchical sheets, groups, or component classes."), `the message is KiCad's (${JSON.stringify(errorsSince(t0))})`);
      await menuClose();
    });

    // ======================================================================================================================== setup: the channels
    await scenario("setup: the three channels are placed, channel 1 compact and the others scattered", async (t) => {
      for (const [part, x, y] of PLACE) {
        const p = (await board()).parts.find((q) => q.ref === part);
        if (p && !p.placed) {
          const r = await post({ op: "place_at", part, x, y });
          if (!r.ok) t(false, `${part}: ${r.message}`);
        }
      }
      await sleep(600);
      const b = await board();
      t(PLACE.every(([part, x, y]) => { const p = poseOf(b, part); return p && p.x === x && p.y === y; }), "the twelve footprints are where the check puts them");
      t(shiftOf(b, 2) === null && shiftOf(b, 3) === null, "the other two channels are not laid out like channel 1");
    });

    // ======================================================================================================================== Strict
    await scenario("Strict, which refuses a move that makes the board worse, does not refuse Generate Placement Rule Areas on a board with no routing", async (t) => {
      if (!strict) return void t(true, "(skipped: there is no Strict switch)");
      if (!strict.checked) strict.click();
      await sleep(300);
      try {
        const snap = await snapshot();
        const t0 = Date.now();
        await openGenerate(false, false);
        await press("OK", 1800);
        // the first zone of a board with no routing starts the routing section, and the connectivity gate then judges every open net: a rule area moves and routes nothing
        t(ruleAreas(await board()).length === 3 && errorsSince(t0).length === 0, `with Strict on the three areas are made (${JSON.stringify(errorsSince(t0))})`);
        await undo();
        t((await snapshot()) === snap, "and one undo takes them away");
      } finally {
        if (strict.checked) strict.click();
        await sleep(300);
      }
    });

    // ======================================================================================================================== Generate Placement Rule Areas
    await scenario("Generate Placement Rule Areas: the dialog has a tab for sheets, classes and groups, with a row for each channel", async (t) => {
      const r = await E.run("pcbnew.Multichannel.generatePlacementRuleAreas");
      t(r.dialog === "Generate Placement Rule Areas" && !!tid("mc-generate-dialog"), `the action opens the dialog (${r.dialog})`);
      const rowsOf = () => [...document.querySelectorAll('[data-testid^="mc-generate-row-"]')].map((e) => e.getAttribute("data-testid").replace("mc-generate-row-", ""));
      const sheets = rowsOf();
      t(sheets.length === 3 && sheets.every((s) => s.startsWith("sheet-")), `the Sheets tab has the three channel sheets (${sheets})`);
      for (const row of sheets) {
        const key = row.slice("sheet-".length);
        const k = Number(/\(U(\d)\)/.exec(key)?.[1]);
        if (CH[k]) SHEET[k] = key;
        t(tid(`mc-generate-row-${row}`).title.includes(refsOf(k).join(", ")) || refsOf(k).every((x) => tid(`mc-generate-row-${row}`).title.includes(x)), `the row of ${key} holds ${refsOf(k)} (${tid(`mc-generate-row-${row}`).title})`);
      }
      t(/Sheet Path/.test(tid("mc-generate-dialog").innerText) && /\.kicad_sch/.test(tid("mc-generate-dialog").innerText), "the sheets' table has the path and the file of each");
      tid("mc-generate-tab-component_class").click();
      await sleep(250);
      const classes = rowsOf();
      t(classes.length === 3 && classes.every((c) => c.startsWith("component_class-")), `the Component Classes tab has one class for each sheet (${classes})`);
      tid("mc-generate-tab-group").click();
      await sleep(250);
      t(rowsOf().length === 0 && /\(none\)/.test(tid("mc-generate-dialog").innerText), "the Groups tab has none: no group is on the board");
      t(tid("mc-generate-group")?.disabled === true, "'Group footprints with their placement rule areas' is not offered for groups");
      tid("mc-generate-tab-sheet").click();
      await sleep(250);
      t(tid("mc-generate-group")?.disabled === false && tid("mc-generate-replace")?.checked === false, "on the Sheets tab the box is offered, and 'Replace existing' is off");
      // nothing ticked: OK makes nothing
      const snap = await snapshot();
      await press("OK", 1200);
      t(titles().length === 0 && (await snapshot()) === snap, "OK with no row ticked changes nothing");
    });

    await scenario("Generate Placement Rule Areas: one rule area for each channel ticked, around its footprints, as one undo step", async (t) => {
      const snap = await snapshot();
      await openGenerate(false, false);
      const ticked = [...document.querySelectorAll('[data-testid^="mc-generate-check-sheet-"]')].every((e) => e.checked);
      t(ticked, "the three rows are ticked");
      await press("OK", 1800);
      const b = await board();
      const areas = ruleAreas(b);
      t(areas.length === 3 && titles().length === 0, `one rule area for each (${areas.length})`);
      t(/Placement rule areas generated\./.test(toastText()), "the toast says so");
      for (const k of [1, 2, 3]) {
        const a = areas.find((z) => z.placement.source === SHEET[k]);
        if (!t(!!a, `channel ${k} has its area`)) continue;
        AREA[k] = a.id;
        t(a.name === `auto-placement-area-${SHEET[k]}` && a.placement.enabled === true && a.placement.source_type === "sheetname", `it is named for the sheet and has it as its placement source (${a.name} ${JSON.stringify(a.placement)})`);
        t(a.layers.join() === "F.Cu,B.Cu", `it is on the copper layers (${a.layers})`);
        t(rectilinear(a.outline) && a.outline.length >= 4, `its outline is rectilinear (${a.outline.length} corners)`);
        const around = refsOf(k).filter((ref) => !inside(a.outline, [poseOf(b, ref).x, poseOf(b, ref).y]));
        t(around.length === 0, `every footprint of the channel is inside it${around.length ? ` (not: ${around})` : ""}`);
        const others = refsOf(k === 1 ? 2 : 1).filter((ref) => inside(a.outline, [poseOf(b, ref).x, poseOf(b, ref).y]));
        t(others.length === 0, "and no footprint of another channel");
      }
      await undo();
      t((await snapshot()) === snap && ruleAreas(await board()).length === 0, "one undo takes all three areas away");
      await redo();
      t(ruleAreas(await board()).length === 3, "one redo gives them back");
    });

    await scenario("Generate Placement Rule Areas: an area that is there already is left alone, Replace makes it again, the group box groups each area with its footprints", async (t) => {
      const ids0 = ruleAreas(await board()).map((z) => z.id).sort();
      const snap = await snapshot();
      // no box: nothing new is made -- the three exist
      let t0 = Date.now();
      await openGenerate(false, false);
      await press("OK", 1500);
      t((await snapshot()) === snap, `with the areas there and 'Replace' off nothing changes (${errorsSince(t0).join(" | ").slice(0, 120)})`);
      // Replace and Group
      await openGenerate(true, true);
      await press("OK", 1800);
      let b = await board();
      const ids1 = ruleAreas(b).map((z) => z.id).sort();
      t(ids1.length === 3 && ids1.join() === ids0.join(), `Replace made the three areas again: still three, and each has the id of the one it replaced, an id being made from the net, layer and outline (${ids1.length})`);
      const gs = groupsOf(b);
      t(gs.length === 3, `and one group each (${gs.length})`);
      for (const k of [1, 2, 3]) {
        const a = ruleAreas(b).find((z) => z.placement.source === SHEET[k]);
        const g = gs.find((x) => x.member_ids.includes(a?.id));
        t(!!g && [...refsOf(k), a.id].every((m) => g.member_ids.includes(m)) && g.member_ids.length === 5, `channel ${k}'s group holds its area and its four footprints (${g?.member_ids})`);
        AREA[k] = a?.id;
      }
      // Repeat Layout takes the reference from a selected group that holds a rule area
      await select([gs.find((x) => x.member_ids.includes(AREA[1])).id]);
      const r = await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      t(r.dialog === "Repeat Layout" && tid("mc-repeat-reference")?.textContent === `auto-placement-area-${SHEET[1]}`, `Repeat Layout with a group selected: the reference is the area in the group (${tid("mc-repeat-reference")?.textContent})`);
      await press("Cancel", 400);
      await undo();
      t((await snapshot()) === snap, "one undo takes the new areas and the groups away and gives the old areas back");
      // Generate with a group: the Groups tab lists a named group, and its area is made of everything in it
      await post({ op: "group", ids: ["U3", "R5", "C3"] });
      let g = groupsOf(await board()).find((x) => x.member_ids.includes("U3"));
      await post({ op: "edit_group", id: g.id, name: "short", member_ids: g.member_ids });
      t0 = Date.now();
      await openGenerate(false, false, false);
      tid("mc-generate-tab-group").click();
      await sleep(250);
      t(!!tid("mc-generate-row-group-short") && tid("mc-generate-group")?.disabled === true, "a named group is a row of the Groups tab");
      tid("mc-generate-check-group-short").click();
      await sleep(150);
      await press("OK", 1800);
      b = await board();
      const made = ruleAreas(b).find((z) => z.placement.source_type === "group");
      t(!!made && made.placement.source === "short" && made.name === "auto-placement-area-short", `its area has the group as its source (${made?.name} ${JSON.stringify(made?.placement)})`);
      t(!!made && ["U3", "R5", "C3"].every((ref) => inside(made.outline, [poseOf(b, ref).x, poseOf(b, ref).y])), "and holds its three footprints");
      await undo();
      await undo(); // the group
      await undo(); // its rename
      t((await snapshot()) === snap, "undone: the board is as it was");
    });

    // ======================================================================================================================== Rule Area Properties
    await scenario("Rule Area Properties: the Placement section has the source, and a change of it is kept and undone", async (t) => {
      const snap = await snapshot();
      await select([AREA[1]]);
      const r = await E.run("pcbnew.InteractiveEdit.properties");
      t(r.dialog === "Rule Area Properties", `the properties of a rule area open the rule area's dialog (${r.dialog})`);
      t(!!tid("zone-placement") && /No placement/.test(tid("zone-placement").innerText) && /Place items from sheet/.test(tid("zone-placement").innerText) && /component class/.test(tid("zone-placement").innerText) && /Place items in group/.test(tid("zone-placement").innerText), "the Placement section has KiCad's four choices");
      t(tid("zone-placement-sheetname")?.checked === true && tid("zone-placement-source-sheetname")?.value === SHEET[1], `the sheet the area was made from is the one chosen (${tid("zone-placement-source-sheetname")?.value})`);
      const offered = [...tid("zone-placement-source-sheetname").options].map((o) => o.value).sort();
      t(offered.join("|") === Object.values(SHEET).sort().join("|"), `the sheets of the board are offered (${offered})`);
      t([...tid("zone-placement-source-component_class").options].length === 3, "and the component classes");
      // another sheet
      setValue(tid("zone-placement-source-sheetname"), SHEET[2]);
      await sleep(150);
      tid("zone-ok").click();
      await sleep(1200);
      let z = ruleAreas(await board()).find((x) => x.id === AREA[1]);
      t(z.placement.source === SHEET[2] && z.placement.enabled && z.placement.source_type === "sheetname", `OK keeps the other sheet (${JSON.stringify(z.placement)})`);
      await undo();
      z = ruleAreas(await board()).find((x) => x.id === AREA[1]);
      t(z.placement.source === SHEET[1], "one undo gives the old source back");
      // a class, then none
      await select([AREA[1]]);
      await E.run("pcbnew.InteractiveEdit.properties");
      tid("zone-placement-component_class").click();
      await sleep(150);
      const cls = tid("zone-placement-source-component_class").value;
      tid("zone-ok").click();
      await sleep(1200);
      z = ruleAreas(await board()).find((x) => x.id === AREA[1]);
      t(z.placement.source_type === "component_class" && z.placement.enabled && z.placement.source === cls && cls !== "", `a component class as the source (${JSON.stringify(z.placement)})`);
      await select([AREA[1]]);
      await E.run("pcbnew.InteractiveEdit.properties");
      tid("zone-placement-none").click();
      await sleep(150);
      tid("zone-ok").click();
      await sleep(1200);
      z = ruleAreas(await board()).find((x) => x.id === AREA[1]);
      t(z.placement.enabled === false, `No placement turns it off (${JSON.stringify(z.placement)})`);
      await undo();
      await undo();
      t((await snapshot()) === snap, "undone");
    });

    // ======================================================================================================================== setup: copper
    const COPPER = {};
    await scenario("setup: channel 1's copper, and what channel 2 has of its own", async (t) => {
      const out = [];
      // the track of the middle net ends on pad 2 of the amplifier, so what a copy of it ends on says whether the copper turns the way the footprints do
      const pad2 = padOf(await board(), "U1", "2");
      out.push(await post({ op: "add_track", net: "/OPAMP (U1)/SUM1", layer: "F.Cu", width: 200_000, pts: [{ x: 3_500_000, y: 4_200_000 }, { x: 3_500_000, y: 6_500_000 }, { x: pad2.x, y: pad2.y }] }));
      out.push(await post({ op: "add_track", net: "/OPAMP (U1)/OUT1", layer: "F.Cu", width: 250_000, pts: [{ x: 7_700_000, y: 4_200_000 }, { x: 7_700_000, y: 7_000_000 }] }));
      out.push(await post({ op: "add_track", net: "GND", layer: "F.Cu", width: 300_000, pts: [{ x: 3_000_000, y: 9_000_000 }, { x: 3_000_000, y: 10_500_000 }] }));
      out.push(await post({ op: "add_via", net: "/OPAMP (U1)/SUM1", x: 5_500_000, y: 11_000_000, drill: 300_000, diameter: 600_000, from_layer: "F.Cu", to_layer: "B.Cu" }));
      out.push(await post({ op: "add_zone", net: "GND", layer: "B.Cu", outline: [{ x: 2_500_000, y: 7_000_000 }, { x: 8_500_000, y: 7_000_000 }, { x: 8_500_000, y: 10_500_000 }, { x: 2_500_000, y: 10_500_000 }] }));
      out.push(await post({ op: "add_text", text: { content: "CH", at: { x: 6_500_000, y: 12_500_000 }, angle: 0, layer: "F.Cu", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false } }));
      out.push(await post({ op: "add_text", text: { content: "SILK", at: { x: 4_000_000, y: 12_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 800_000, stroke_width: 150_000, justify: "center", mirror: false } }));
      // channel 2's own: a track and a text in its area, a track outside every area
      out.push(await post({ op: "add_track", net: "/OPAMP (U2)/SUM2", layer: "F.Cu", width: 200_000, pts: [{ x: 15_000_000, y: 8_000_000 }, { x: 16_000_000, y: 8_000_000 }] }));
      out.push(await post({ op: "add_text", text: { content: "OLD", at: { x: 19_000_000, y: 12_500_000 }, angle: 0, layer: "F.Cu", size_nm: 800_000, stroke_width: 150_000, justify: "center", mirror: false } }));
      out.push(await post({ op: "add_track", net: "GND", layer: "F.Cu", width: 300_000, pts: [{ x: 2_000_000, y: 20_000_000 }, { x: 8_000_000, y: 20_000_000 }] }));
      t(out.every((o) => o.ok), `ten edits made (${out.filter((o) => !o.ok).map((o) => o.message)})`);
      await sleep(500);
      const b = await board();
      t(tracksOf(b).length === 5 && viasOf(b).length === 1 && copperZones(b).length === 1 && textsOf(b).length === 3, `the copper is there (${tracksOf(b).length} tracks, ${viasOf(b).length} via, ${copperZones(b).length} zone, ${textsOf(b).length} texts)`);
      const a1 = ruleAreas(b).find((z) => z.id === AREA[1]);
      const ch1 = (pts) => pts.every((p) => inside(a1.outline, p));
      COPPER.ch1Tracks = tracksOf(b).filter((x) => ch1(x.pts)).map((x) => x.id);
      COPPER.outside = tracksOf(b).filter((x) => x.pts[0][1] === 20_000_000).map((x) => x.id);
      COPPER.ch2Old = tracksOf(b).filter((x) => x.net === "/OPAMP (U2)/SUM2").map((x) => x.id);
      t(COPPER.ch1Tracks.length === 3 && COPPER.outside.length === 1 && COPPER.ch2Old.length === 1, "three tracks are inside channel 1's area, one outside every area, and channel 2 has one of its own");
    });
    const afterSetup = await snapshot();

    // ======================================================================================================================== Repeat Layout: the reference and the table
    /** KiCad's options of Repeat Layout (`REPEAT_LAYOUT_OPTIONS`): the tool keeps the last ones, so a run says which it wants. */
    const DEFAULTS = { copy_placement: true, copy_routing: true, connected_routing_only: false, copy_other_items: true, group_items: false, include_locked_items: true };
    /** Open Repeat Layout with the area `id` selected, the options set to the defaults and then `overrides` (`null`: as the dialog opens). */
    const openRepeat = async (id, overrides = {}) => {
      await select(id ? [id] : []);
      const r = await E.run("pcbnew.Multichannel.repeatLayout");
      await waitFor(() => tid("mc-repeat-dialog"));
      if (overrides !== null) for (const [name, on] of Object.entries({ ...DEFAULTS, ...overrides })) { const e = tid(`mc-repeat-opt-${name}`); if (e && e.checked !== on) e.click(); }
      await sleep(100);
      return r;
    };
    const targetRows = () => [...document.querySelectorAll('[data-testid^="mc-repeat-target-"]')].map((r) => ({ id: r.getAttribute("data-testid").slice("mc-repeat-target-".length), status: r.querySelector('[data-testid^="mc-repeat-status-"]')?.textContent, copy: r.querySelector("input")?.checked }));

    await scenario("Repeat Layout: without a rule area selected the picker asks for one, and Escape ends it", async (t) => {
      await select([]);
      const r = E.run("pcbnew.Multichannel.repeatLayout");
      await sleep(600);
      t(E.state().picker === "Select a reference Rule Area to copy from...", `the picker asks (${E.state().picker})`);
      t(titles().length === 0, "no dialog yet");
      await escape();
      await r;
      t(E.state().picker === null && titles().length === 0, "Escape ends it, with no dialog");
    });

    await scenario("Repeat Layout: the table of target areas, each with its match, and what OK copies with every option on", async (t) => {
      const before = await board();
      const r = await openRepeat(AREA[1], null);
      t(r.dialog === "Repeat Layout", `with the reference area selected the action opens the dialog (${r.dialog})`);
      t(tid("mc-repeat-reference")?.textContent === `auto-placement-area-${SHEET[1]}`, `the reference is named (${tid("mc-repeat-reference")?.textContent})`);
      const rows = targetRows();
      t(rows.length === 2 && rows.every((x) => x.status === "OK" && x.copy), `the two other areas are the targets, each OK and ticked (${JSON.stringify(rows.map((x) => x.status))})`);
      t(rows.map((x) => x.id).sort().join() === [AREA[2], AREA[3]].sort().join(), "they are channels 2 and 3");
      const anchors = [...tid("mc-repeat-anchor").options].map((o) => o.value);
      t(anchors[0] === "" && refsOf(1).every((x) => anchors.includes(x)), `the anchor choices are none and the reference's footprints (${anchors})`);
      t(["copy_placement", "copy_routing", "copy_other_items", "include_locked_items"].every((o) => tid(`mc-repeat-opt-${o}`).checked) && !tid("mc-repeat-opt-connected_routing_only").checked && !tid("mc-repeat-opt-group_items").checked, "the options begin as KiCad's do: placement, routing, other items and locked items on; the rest off");
      await press("OK", 1000);
      t(/Copied to 2 Rule Areas\./.test(toastText()), "the toast says how many areas it copied to");
      await sleep(1200);
      const after = await board();
      // the footprints
      const v2 = shiftOf(after, 2);
      const v3 = shiftOf(after, 3);
      t(!!v2 && !!v3, `channels 2 and 3 are channel 1's layout moved by one vector each (${JSON.stringify([v2, v3])})`);
      t(moved(before, after, refsOf(1)).length === 0, "channel 1 did not move");
      // the vector is the difference of the areas' centres: the box of each outline
      const centre = (poly) => { const xs = poly.map((p) => p[0]); const ys = poly.map((p) => p[1]); return [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2]; };
      const c1 = centre(ruleAreas(before).find((z) => z.id === AREA[1]).outline);
      const c2 = centre(ruleAreas(before).find((z) => z.id === AREA[2]).outline);
      t(!!v2 && near(v2.x, c2[0] - c1[0], 60_000) && near(v2.y, c2[1] - c1[1], 60_000), `the vector is the difference of the areas' centres, within the snap (${JSON.stringify(v2)} against ${Math.round(c2[0] - c1[0])},${Math.round(c2[1] - c1[1])})`);
      // the routing
      const a1 = ruleAreas(after).find((z) => z.id === AREA[1]);
      const ch1Tracks = tracksOf(after).filter((x) => COPPER.ch1Tracks.includes(x.id));
      for (const k of [2, 3]) {
        const v = k === 2 ? v2 : v3;
        const ak = ruleAreas(after).find((z) => z.id === AREA[k]);
        const mine = tracksOf(after).filter((x) => x.pts.every((p) => inside(ak.outline, p)));
        const want = ch1Tracks.map((x) => JSON.stringify([x.net === "/OPAMP (U1)/SUM1" ? CH[k].sum : x.net === "/OPAMP (U1)/OUT1" ? CH[k].out : x.net, x.layer, x.width, shifted(x.pts, v)])).sort();
        const got = mine.map((x) => JSON.stringify([x.net, x.layer, x.width, x.pts])).sort();
        t(JSON.stringify(want) === JSON.stringify(got), `channel ${k} has channel 1's three tracks on its own nets, moved (${got.length} tracks${JSON.stringify(want) === JSON.stringify(got) ? "" : `: ${got.join(" ")}`})`);
        const vias = viasOf(after).filter((x) => inside(ak.outline, [x.x, x.y]));
        t(vias.length === 1 && vias[0].net === CH[k].sum && near(vias[0].x, 5_500_000 + v.x) && near(vias[0].y, 11_000_000 + v.y) && vias[0].from === "F.Cu" && vias[0].to === "B.Cu", `and the via on ${CH[k].sum} (${JSON.stringify(vias.map((x) => [x.net, x.x, x.y]))})`);
        const zones = copperZones(after).filter((z) => z.outline.every((p) => inside(ak.outline, p)));
        t(zones.length === 1 && zones[0].net === "GND" && zones[0].layer === "B.Cu" && JSON.stringify(zones[0].outline) === JSON.stringify(shifted(copperZones(before)[0].outline, v)), `and the zone, on the shared net (${zones.length})`);
        const texts = textsOf(after).filter((x) => inside(ak.outline, [x.x, x.y]));
        t(texts.length === 1 && texts[0].content === "CH" && near(texts[0].x, 6_500_000 + v.x) && near(texts[0].y, 12_500_000 + v.y), `and the text on a copper layer, and not the silkscreen one, which is on no layer of the area (${JSON.stringify(texts.map((x) => [x.content, x.x, x.y]))})`);
        t(ak.outline.length === a1.outline.length && JSON.stringify(ak.outline) === JSON.stringify(shifted(a1.outline, v)), "the target's area takes the reference area's shape");
        const toPad = tracksOf(after).find((x) => x.net === CH[k].sum && x.pts.length === 3);
        const pad = padOf(after, CH[k].U, "2");
        t(!!toPad && !!pad && near(toPad.pts[2][0], pad.x, 2000) && near(toPad.pts[2][1], pad.y, 2000), `the copy of the track that ends on pad 2 of the amplifier ends on pad 2 of the amplifier of channel ${k} (${JSON.stringify(toPad?.pts[2])} against ${JSON.stringify(pad)})`);
      }
      // what the target had of its own in the area goes, what it had outside stays
      t(!tracksOf(after).some((x) => COPPER.ch2Old.includes(x.id)), "the track channel 2 had in its area is gone");
      t(!textsOf(after).some((x) => x.content === "OLD"), "and its text");
      t(tracksOf(after).some((x) => COPPER.outside.includes(x.id)), "the track outside every area stays");
      t(tracksOf(after).length === 3 + 3 + 3 + 1 && viasOf(after).length === 3 && copperZones(after).length === 3, `in all: ${tracksOf(after).length} tracks, ${viasOf(after).length} vias, ${copperZones(after).length} zones`);
      // one undo step, redo
      const afterSnap = await snapshot();
      await undo();
      t((await snapshot()) === afterSetup, "one undo takes everything back: footprints, tracks, vias, zones, texts and the areas' shapes");
      await redo();
      t((await snapshot()) === afterSnap, "one redo does it again");
      // again: the same, nothing doubled (the target's copies are taken away before the new ones come)
      await openRepeat(AREA[1]);
      await press("OK", 2200);
      t((await snapshot()) === afterSnap, "repeating it again leaves the board as it is: the copies of the first run are replaced, not added to");
      await undo(); // the redo
      await ensureSame(afterSetup);
    });

    await scenario("Repeat Layout: each option leaves alone what it is not about", async (t) => {
      const before = await board();
      // placement off: the footprints stay, the copper comes
      await openRepeat(AREA[1], { copy_placement: false });
      await press("OK", 2200);
      let after = await board();
      t(moved(before, after, [...refsOf(2), ...refsOf(3)]).length === 0, "Copy footprint placement off: no footprint moves");
      t(tracksOf(after).length > tracksOf(before).length && viasOf(after).length === 3, "... and the copper is copied");
      await ensureSame(afterSetup);
      // routing off: the footprints move, the tracks and vias of the targets stay (the zone and text still come: other items)
      await openRepeat(AREA[1], { copy_routing: false });
      await press("OK", 2200);
      after = await board();
      t(moved(before, after, [...refsOf(2), ...refsOf(3)]).length === 8, "Copy routing off: the footprints move");
      t(tracksOf(after).length === tracksOf(before).length && viasOf(after).length === 1 && tracksOf(after).some((x) => COPPER.ch2Old.includes(x.id)), "... no track or via is copied or taken away");
      t(copperZones(after).length === 3 && textsOf(after).filter((x) => x.content === "CH").length === 3, "... the zones and texts, which are other items, are");
      await ensureSame(afterSetup);
      await openRepeat(AREA[1], null);
      t(!tid("mc-repeat-opt-copy_routing").checked && tid("mc-repeat-opt-copy_placement").checked, "the dialog opens with the options of the last run, as KiCad's does (the tool keeps them)");
      await press("Cancel", 400);
      // other items off
      await openRepeat(AREA[1], { copy_other_items: false });
      await press("OK", 2200);
      after = await board();
      t(copperZones(after).length === 1 && textsOf(after).length === 3 && textsOf(after).some((x) => x.content === "OLD"), "Copy other items off: no zone or text is copied or taken away");
      t(tracksOf(after).length === 3 + 3 + 3 + 1 && viasOf(after).length === 3, "... the routing is");
      await ensureSame(afterSetup);
      // restrict to the routing connected within the area: the track of net GND is on a net of the area's footprints (the pads of U1.3 and U1.4), so it is copied, one on a net
      // of no footprint of the area is not
      const out = await post({ op: "add_track", net: "unconnected-(R3-Pad1)", layer: "F.Cu", width: 200_000, pts: [{ x: 8_000_000, y: 8_000_000 }, { x: 8_500_000, y: 8_500_000 }] });
      t(out.ok, "a track on a net none of channel 1's footprints is on, in its area");
      const withStray = await snapshot();
      await openRepeat(AREA[1], { connected_routing_only: true });
      await press("OK", 2200);
      after = await board();
      t(tracksOf(after).filter((x) => x.net === "unconnected-(R3-Pad1)").length === 1 && tracksOf(after).length === 3 + 3 + 3 + 1 + 1, `'Restrict to routing connected within the area' leaves the unrelated track out (${tracksOf(after).length} tracks)`);
      await ensureSame(withStray);
      await openRepeat(AREA[1]);
      await press("OK", 2200);
      after = await board();
      t(tracksOf(after).filter((x) => x.net === "unconnected-(R3-Pad1)").length === 3, `with it off the unrelated track is copied too (${tracksOf(after).filter((x) => x.net === "unconnected-(R3-Pad1)").length} on IN2)`);
      await ensureSame(withStray);
      await undo(); // the stray track
      await ensureSame(afterSetup);
    });

    await scenario("Repeat Layout: locked footprints are left where they are when 'Include locked items' is off, and moved when it is on", async (t) => {
      await post({ op: "set_locked", ids: ["R4"], locked: true });
      await sleep(400);
      const locked = await snapshot();
      const before = await board();
      await openRepeat(AREA[1], { include_locked_items: false });
      await press("OK", 2200);
      let after = await board();
      t(moved(before, after, ["R4"]).length === 0 && moved(before, after, refsOf(2)).length === 3 && moved(before, after, refsOf(3)).length === 4, `off: the locked R4 stays, the other footprints move (${moved(before, after, [...refsOf(2), ...refsOf(3)])})`);
      await ensureSame(locked);
      await openRepeat(AREA[1]);
      await press("OK", 2200);
      after = await board();
      t(moved(before, after, ["R4"]).length === 1, "on: it moves with the rest");
      await ensureSame(locked);
      await undo(); // the lock
      t((await snapshot()) === afterSetup, "undone");
    });

    await scenario("Repeat Layout: an anchor footprint places the target's footprints about it, turned with it", async (t) => {
      // channel 3's amplifier is turned a quarter, the one footprint, about itself: the routing is kept
      const u3 = poseOf(await board(), "U3");
      const r1 = await post({ op: "rotate_items", ids: ["U3"], pivot: { x: u3.x, y: u3.y }, angle_millideg: 90000 });
      t(r1.ok, `U3 turned a quarter (${r1.message})`);
      await sleep(400);
      const turned = await snapshot();
      const before = await board();
      await openRepeat(AREA[1]);
      setValue(tid("mc-repeat-anchor"), "U1");
      await sleep(150);
      t(tid("mc-repeat-anchor").value === "U1", "U1 is the anchor");
      await press("OK", 2200);
      const after = await board();
      t(moved(before, after, ["U2", "U3"]).length === 0, "the anchors' counterparts stay where they are");
      const v2 = shiftOf(after, 2);
      t(!!v2 && near(v2.x, 18_000_000 - 5_500_000) && near(v2.y, 9_000_000 - 9_000_000), `channel 2, with the amplifier unturned, is channel 1 moved to it (${JSON.stringify(v2)})`);
      const turn = turnedBy(after, 3);
      t(turn !== null && near(angle(turn), angle(poseOf(before, "U3").rot - poseOf(before, "U1").rot), 0.01) && !near(angle(turn), 0, 0.01), `channel 3 is channel 1 turned about its amplifier by the angle between the two amplifiers (${turn})`);
      t(tracksOf(after).filter((x) => x.net === "/OPAMP (U3)/SUM3").length === 1, "and its routing is copied onto its nets");
      // the copper turns the way the footprints do: the track that ends on pad 2 of the amplifier ends on pad 2 of the turned amplifier, which is where the pad is on the board
      for (const k of [2, 3]) {
        const toPad = tracksOf(after).find((x) => x.net === CH[k].sum && x.pts.length === 3);
        const pad = padOf(after, CH[k].U, "2");
        t(!!toPad && !!pad && near(toPad.pts[2][0], pad.x, 2000) && near(toPad.pts[2][1], pad.y, 2000), `channel ${k}: the copy of the track that ends on pad 2 of the amplifier ends on pad 2 of the amplifier (${JSON.stringify(toPad?.pts[2])} against ${JSON.stringify(pad)})`);
      }
      await ensureSame(turned);
      await undo(); // the turn
      t((await snapshot()) === afterSetup, "undone");
    });

    await scenario("Repeat Layout: an area whose footprints do not match is refused in KiCad's words, and the others are copied", async (t) => {
      // channel 3's area is made from a group of three of its four footprints
      await post({ op: "group", ids: ["U3", "R5", "C3"] });
      const g = groupsOf(await board()).find((x) => x.member_ids.includes("U3"));
      await post({ op: "edit_group", id: g.id, name: "short", member_ids: g.member_ids });
      await sleep(300);
      await select([AREA[3]]);
      await E.run("pcbnew.InteractiveEdit.properties");
      tid("zone-placement-group").click();
      await sleep(150);
      t(tid("zone-placement-source-group")?.value === "short", "the group is offered as a source");
      tid("zone-ok").click();
      await sleep(1200);
      const mismatched = await snapshot();
      const before = await board();
      await openRepeat(AREA[1]);
      const rows = targetRows();
      const row3 = rows.find((x) => x.id === AREA[3]);
      const row2 = rows.find((x) => x.id === AREA[2]);
      t(row2?.status === "OK" && row2.copy === true, "channel 2 is OK and ticked");
      t(row3?.status === "Component count mismatch" && row3.copy === false, `channel 3 says why and is not ticked (${row3?.status})`);
      tid(`mc-repeat-details-${AREA[3]}`)?.click();
      await sleep(300);
      const reasons = tid("mc-repeat-mismatch-reasons")?.innerText ?? "";
      t(/Component count mismatch/.test(reasons) && /Reference area total components: 4/.test(reasons) && /Target area total components: 3/.test(reasons) && /C1, R1, R2, U1/.test(reasons) && /C3, R5, U3/.test(reasons), `the details name the footprints of both (${reasons.replace(/\n/g, " / ")})`);
      t(/Rule area topologies do not match: Component count mismatch/.test(tid("mc-repeat-mismatch")?.innerText ?? ""), "and the summary is KiCad's");
      const z = tid("mc-repeat-mismatch")?.parentElement;
      t(Number(getComputedStyle(z).zIndex) > Number(getComputedStyle(tid("mc-repeat-dialog").closest(".dialog-backdrop")).zIndex), "the details are above the dialog");
      tid("mc-repeat-mismatch-ok").click();
      await sleep(250);
      t(!tid("mc-repeat-mismatch"), "OK closes the details");
      await press("OK", 2200);
      const after = await board();
      t(!!shiftOf(after, 2) && moved(before, after, refsOf(3)).length === 0, "OK copies to channel 2 and leaves channel 3's footprints");
      t(tracksOf(after).filter((x) => x.net === "/OPAMP (U3)/SUM3").length === 0, "and its routing");
      await ensureSame(mismatched);
      // from the short area, the table has no area that matches: OK has nothing to copy to
      await openRepeat(AREA[3]);
      const rows3 = targetRows();
      t(rows3.length === 2 && rows3.every((x) => x.status !== "OK" && x.copy === false), `from the short area, both others do not match and none is ticked (${JSON.stringify(rows3.map((x) => x.status))})`);
      t(buttonOf(dialog(), "OK")?.disabled === true, "and OK is disabled: there is nothing to copy to");
      await press("Cancel", 400);
      await ensureSame(afterSetup);
    });

    // ======================================================================================================================== Apply Design Block Layout
    await scenario("Apply Design Block Layout: a block saved from channel 1 is applied to the group of channel 2 linked to it", async (t) => {
      await ensureSame(afterSetup);
      const setup = await board();
      // a library, and channel 1 -- footprints, the copper inside its area, the zone and the text -- saved as a block of it
      await E.run("pcbnew.PcbDesignBlockControl.showDesignBlockPanel");
      await sleep(600);
      await E.run("common.Control.newLibrary");
      const d = await waitFor(() => (titles().includes("New Design Block Library") ? dialog() : null));
      t(!!d, "New Library opens its dialog");
      if (!d) return;
      const ins = d.querySelectorAll("input");
      setValue(ins[1], LIB);
      setValue(ins[2], "Multichannel check");
      await sleep(200);
      await press("Create", 1800);
      const a1 = ruleAreas(setup).find((z) => z.id === AREA[1]);
      const ch1Ids = [
        ...refsOf(1),
        ...tracksOf(setup).filter((x) => x.pts.every((p) => inside(a1.outline, p))).map((x) => x.id),
        ...viasOf(setup).filter((x) => inside(a1.outline, [x.x, x.y])).map((x) => x.id),
        ...copperZones(setup).filter((z) => z.outline.every((p) => inside(a1.outline, p))).map((z) => z.id),
        ...textsOf(setup).filter((x) => x.content === "CH").map((x) => x.id),
      ];
      await select(ch1Ids);
      await E.run("pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock");
      const props = await waitFor(() => (titles().includes("Design Block Properties") ? dialog() : null), 8000);
      t(!!props, "Save Selection as Design Block opens the properties");
      if (!props) return;
      setValue(props.querySelector("#db-name"), "Channel");
      await sleep(200);
      await press("Save", 3000);
      const blockFile = await json(`/api/design_blocks/file?lib=${LIB}&name=Channel&what=pcb`);
      t(blockFile.ok && (blockFile.text.match(/\(footprint "/g) || []).length === 4 && (blockFile.text.match(/\(segment /g) || []).length === 4 && (blockFile.text.match(/\(via /g) || []).length === 1, "the block's board file holds the four footprints, the tracks, the via");
      const linked = groupsOf(await board()).find((g) => g.design_block === `${LIB}:Channel`);
      t(!!linked && linked.member_ids.length === ch1Ids.length, `channel 1 is in a group linked to the block (${linked?.name})`);

      // channel 2: a group linked to the block, its footprints scattered
      await post({ op: "group", ids: refsOf(2) });
      const g2 = groupsOf(await board()).find((x) => x.member_ids.includes("U2"));
      await post({ op: "edit_group", id: g2.id, name: "Ch2", member_ids: g2.member_ids, design_block: `${LIB}:Channel` });
      await post({ op: "move_items", ids: ["R3"], dx: 2_600_000, dy: 1_800_000 });
      await post({ op: "move_items", ids: ["C2"], dx: -2_000_000, dy: -500_000 });
      await sleep(400);
      const prepared = await snapshot();
      const before = await board();
      await select([g2.id]);
      const r = await E.run("pcbnew.InteractiveDrawing.applyDesignBlockLayout");
      await sleep(2500);
      const after = await board();
      t(r.ok === true && /^Applied the layout of design block /.test(r.toast ?? ""), `Apply Design Block Layout runs, and says so (${r.toast ?? r.error})`);
      // the footprints: the anchor stays, the layout is the block's
      const unmoved = moved(before, after, refsOf(2));
      t(unmoved.length >= 3, `the footprints of the group are laid out (${unmoved})`);
      const v = shiftOf(after, 2);
      t(!!v, `they are channel 1's layout moved by one vector (${JSON.stringify(v)})`);
      t(refsOf(2).filter((x) => !unmoved.includes(x)).length === 1 || (v && v.x === 12_500_000 && v.y === 0) || refsOf(2).every((x) => unmoved.includes(x)), "the first pair is the anchor");
      // the copper
      const mine = segmentsOf(tracksOf(after).filter((x) => x.pts.every((p) => v && inside(shifted(a1.outline, v), p)) && !COPPER.ch2Old.includes(x.id)));
      const want = segmentsOf(tracksOf(setup).filter((x) => COPPER.ch1Tracks.includes(x.id)), (n) => (n === "/OPAMP (U1)/SUM1" ? "/OPAMP (U2)/SUM2" : n === "/OPAMP (U1)/OUT1" ? "/OPAMP (U2)/OUT2" : n), v ?? { x: 0, y: 0 });
      t(want.length === 4 && JSON.stringify(want) === JSON.stringify(mine), `the block's four segments are on the group's nets, moved (${mine.length} there${JSON.stringify(want) === JSON.stringify(mine) ? "" : `: ${mine.join(" ")}`})`);
      t(viasOf(after).filter((x) => x.net === "/OPAMP (U2)/SUM2").length === 1 && copperZones(after).length === 2 && textsOf(after).filter((x) => x.content === "CH").length === 2, "the via, the zone and the text too");
      t(tracksOf(after).some((x) => COPPER.ch2Old.includes(x.id)) && textsOf(after).some((x) => x.content === "OLD"), "nothing of the board's own is taken away (the track and the text channel 2 had are still there)");
      const gAfter = groupsOf(after).find((x) => x.id === g2.id);
      t(!!gAfter && gAfter.design_block === `${LIB}:Channel` && gAfter.member_ids.length > g2.member_ids.length + 3, `the copies joined the group (${g2.member_ids.length} to ${gAfter?.member_ids.length} members)`);
      t(ruleAreas(after).length === ruleAreas(before).length, "and no rule area was added");
      await undo();
      t((await snapshot()) === prepared, "one undo takes the layout, the copies and their membership away");
      await redo();
      await undo();

      // refusals: a locked footprint, a group that does not match, a group with no link
      await post({ op: "set_locked", ids: ["R3"], locked: true });
      const lockedSnap = await snapshot();
      let t0 = Date.now();
      await select([g2.id]);
      await E.run("pcbnew.InteractiveDrawing.applyDesignBlockLayout");
      await sleep(1200);
      t(errorsSince(t0).some((m) => /Footprint R3 is locked and cannot be placed\./.test(m)) && (await snapshot()) === lockedSnap, `a locked footprint is refused in KiCad's words, and nothing changes (${JSON.stringify(errorsSince(t0)).slice(0, 160)})`);
      await undo(); // the lock
      await post({ op: "remove_from_group", ids: ["C2"] });
      t0 = Date.now();
      const short = await snapshot();
      await select([g2.id]);
      await E.run("pcbnew.InteractiveDrawing.applyDesignBlockLayout");
      await sleep(1200);
      t(errorsSince(t0).some((m) => /^Rule Area topologies do not match: |^Rule area topologies do not match: /.test(m)) && (await snapshot()) === short, `a group of other footprints than the block's is refused with the mismatch (${JSON.stringify(errorsSince(t0)).slice(0, 200)})`);
      await post({ op: "edit_group", id: g2.id, name: "Ch2", member_ids: g2.member_ids.filter((m) => m !== "C2"), design_block: "" });
      t0 = Date.now();
      const unlinked = await snapshot();
      await select([g2.id]);
      await E.run("pcbnew.InteractiveDrawing.applyDesignBlockLayout");
      await sleep(900);
      t(errorsSince(t0).some((m) => /Select one group linked to a design block/.test(m)) && (await snapshot()) === unlinked, "a group with no link is not one to apply it to");
      await select([]);
      await E.run("pcbnew.InteractiveDrawing.applyDesignBlockLayout");
      await sleep(600);
      await ensureSame(afterSetup, 14);

      // clean up the library
      await gotoTab("PCB");
      const rowOf = (id) => [...document.querySelectorAll('[data-testid="db-pane"] [role="treeitem"]')].find((e) => (e.getAttribute("data-id") ?? e.getAttribute("data-library")) === id);
      if (!rowOf(`${LIB}:Channel`)) { rowOf(LIB)?.click(); await sleep(300); }
      rowOf(`${LIB}:Channel`)?.click();
      await sleep(400);
      await E.run("pcbnew.PcbDesignBlockControl.deleteDesignBlock");
      await waitFor(() => titles().includes("Confirmation"));
      await press("OK", 1800);
      const listing = await json("/api/design_blocks");
      t(!(listing.libraries.find((l) => l.name === LIB)?.blocks ?? []).some((x) => x.name === "Channel"), "the block is deleted");
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
      t(!(await json("/api/design_blocks")).libraries.some((l) => l.name === LIB), "the library's row is out of the table");
    });

    // ======================================================================================================================== clean up
    await scenario("clean up: the board is as the scenarios found it, and nothing was reported as an error besides the refusals", async (t) => {
      t(await ensureSame(afterSetup, 14), "the board is as it was after the setup");
      // the setup's own edits are undone: copper, areas
      for (let i = 0; i < 14 && (ruleAreas(await board()).length > 0 || tracksOf(await board()).length > 0); i++) await undo();
      const b = await board();
      t(ruleAreas(b).length === 0 && tracksOf(b).length === 0 && viasOf(b).length === 0 && copperZones(b).length === 0 && textsOf(b).length === 0 && groupsOf(b).length === 0, "no rule area, copper, text or group is left");
      const bad = E.errors().slice(errorsBefore).map((e) => e.message).filter((m) => !/Cannot auto-generate|nothing to make|Footprint R3 is locked|topologies do not match|Select one group linked/.test(m));
      t(bad.length === 0, `no errors besides the refusals the check provoked ${JSON.stringify(bad.map((m) => m.slice(0, 90)))}`);
    });

    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
