// UI-LEFT F10 check (task-u6-last, PARITY-fpedit.md section 22): the Footprint Editor's arrays and numbering as KiCad's code has them --
//   * Create Array's circular "Pad numbering start" greys out with the Grid tab's "Renumber pads" too, and the circle's alphabet and increment stay enabled
//     (`DIALOG_CREATE_ARRAY::setControlEnablement`); the dialog's picture is the studio's addition (KiCad's dialog has none) and still draws the positions;
//   * Duplicate and Increment (Ctrl+Shift+D) of two pads gives both copies the same number (`EDIT_TOOL::Duplicate`: `GetNextPadNumber` counts the footprint's
//     pads, which the copies join only on the push);
//   * Finish Pad Edit (Ctrl+E) deletes the copies of a merged graphic on the pad's layers that `EDA_SHAPE::Compare` within 2 nm, and keeps the one 3 nm off
//     (`PAD::Recombine`'s `findMatching`).
//
// Run it on a COPY of a board (`eda board new examples/mcu_board_30plus.yaml -o <dir>`, `eda board serve -C <dir> --port N --ui web/studio/dist`):
//   PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/fpedit-array-u6.check.js
// It makes footprints of its own and deletes them at the end. It answers through `window.__fpArrayU6Check`: { results, done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=6
(async () => {
  const results = [];
  const check = (window.__fpArrayU6Check = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) };
  };

  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", body: JSON.stringify({ strict: false, cmd: c }) })).json();
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const off = (id) => !!tid(id)?.matches(":disabled");
  const clickTab = async (label) => { [...document.querySelectorAll("button, [role=tab]")].find((b) => b.textContent.trim() === label)?.click(); await sleep(700); };
  const setChecked = async (id, wanted) => { const el = tid(id); if (el && el.checked !== wanted) { el.click(); await sleep(90); } };
  const until = async (cond, ms = 10000, step = 120) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(step); } return false; };
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => json(`/api/footprint?name=${encodeURIComponent(name)}`);
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const closeDialogs = async () => {
    for (let i = 0; i < 4; i++) {
      if (document.querySelectorAll(".dialog-backdrop").length === 0) break;
      const cancel = [...document.querySelectorAll(".dialog-footer button")].find((b) => /^(Cancel|Close)$/.test(b.textContent.trim()));
      if (cancel) cancel.click(); else document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    const errorsFrom = (__eda.errors?.() ?? []).length;
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    const newErrors = (__eda.errors?.() ?? []).slice(errorsFrom).map((e) => e.message);
    if (newErrors.length) { entry.notes.push(`FAIL: errors logged: ${newErrors.slice(0, 3).join(" | ")}`); entry.ok = false; }
    await closeDialogs();
    results.push(entry);
  };

  const libraryBefore = await fpNames();
  const made = [];
  /** A new footprint (New Footprint, Ctrl+N) holding `pads` and `graphics`, open in the editor. */
  const footprintWith = async (t, pads, graphics = []) => {
    await clickTab("Footprint");
    t(__eda.state().tab === "footprint", "the Footprint Editor tab");
    await __eda.run("pcbnew.ModuleEditor.newFootprint");
    await sleep(800);
    const name = (await fpNames()).find((n) => !libraryBefore.includes(n) && !made.includes(n));
    made.push(name);
    for (const p of pads) { const r = await cmd({ op: "add_pad", footprint: name, pad: p }); if (!r.ok) t(false, `add_pad: ${r.message}`); }
    for (const g of graphics) { const r = await cmd({ op: "add_footprint_graphic", footprint: name, shape: g }); if (!r.ok) t(false, `add_footprint_graphic: ${r.message}`); }
    await sleep(700);
    return name;
  };
  const padOf = (n, x = 0, y = 0) => ({ number: n, at: { x, y }, size: [900_000, 600_000], shape: "round_rect", kind: "smd", drill: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"], roundrect_ratio: 0.25 });

  await scenario("Create Array's circular numbering: the start follows Renumber pads and its radio, the alphabet and increment stay on (setControlEnablement); the picture draws the positions", async (t) => {
    const name = await footprintWith(t, [padOf("1", 5_000_000, 0)]);
    const fp = await footprint(name);
    await select(fp.pads.map((p) => p.id));
    await __eda.run("pcbnew.Array.createArray");
    await sleep(450);
    t(!!tid("create-array"), "the dialog opens");
    await setChecked("array-renumber", true);
    tid("array-tab-circular").click(); await sleep(150);
    tid("array-circ-initial-1").click(); await sleep(100);
    t(!off("array-circular-start") && !off("array-circular-kind") && !off("array-circular-step"), "From start value: the start, the alphabet and the increment are on");
    tid("array-circ-initial-0").click(); await sleep(100);
    t(off("array-circular-start"), "Use first free number turns the start entry off");
    t(!off("array-circular-kind") && !off("array-circular-step"), "and leaves the alphabet and the increment on, as KiCad's dialog does");
    tid("array-circ-initial-1").click(); await sleep(100);
    tid("array-tab-grid").click(); await sleep(150);
    await setChecked("array-renumber", false);
    tid("array-tab-circular").click(); await sleep(150);
    t(off("array-circular-start"), "the Grid tab's Renumber pads off turns the circle's start entry off too");
    t(!off("array-circular-kind") && !off("array-circular-step"), "the circle's alphabet and increment stay on");
    t(!!tid("array-pad-numbers") && /Pad numbers: 1, 2, 3, 4/.test(tid("array-pad-numbers")?.textContent ?? ""), `the circle still numbers its pads (SetShouldNumber( m_isFootprintEditor )): ${tid("array-pad-numbers")?.textContent}`);
    const svg = tid("array-preview");
    t(svg?.tagName.toLowerCase() === "svg" && /positions/.test(svg.getAttribute("aria-label") ?? ""), `the studio's picture (an addition, KiCad has none): ${svg?.getAttribute("aria-label")}`);
    tid("array-tab-grid").click(); await sleep(150);
    await setChecked("array-renumber", true);
    tid("array-cancel").click(); await sleep(200);
    t((await footprint(name)).pads.length === 1, "Cancel makes nothing");
  });

  await scenario("Duplicate and Increment of two pads gives both copies the same next number (EDIT_TOOL::Duplicate, GetNextPadNumber before the push)", async (t) => {
    const name = await footprintWith(t, [padOf("1", 0, 0), padOf("2", 2_000_000, 0)]);
    const fp = await footprint(name);
    await select(fp.pads.map((p) => p.id));
    await __eda.run("pcbnew.InteractiveEdit.duplicateIncrementPads");
    t(await until(async () => (await footprint(name)).pads.length === 4), "two copies are made");
    const nums = (await footprint(name)).pads.map((p) => p.number).sort().join();
    t(nums === "1,2,3,3", `both copies take 3: ${nums}`);
    // Escape drops the carried copies and undoes the duplicate.
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await sleep(600);
    if ((await footprint(name)).pads.length !== 2) { await __eda.run("common.Interactive.undo"); await sleep(600); }
    t((await footprint(name)).pads.length === 2, "the duplicate is undone");
  });

  await scenario("Finish Pad Edit deletes the copies of a merged line on the pad's layers within 2 nm and keeps the one 3 nm off (PAD::Recombine's findMatching)", async (t) => {
    const seg = (layer, dx, w = 300_000) => ({ kind: "segment", layer, stroke_width: w, filled: false, start: { x: dx, y: 0 }, end: { x: 2_000_000, y: 0 } });
    const circle = { number: "1", at: { x: 0, y: 0 }, size: [1_000_000, 1_000_000], shape: "circle", kind: "smd", drill: null, rot: 0, layers: ["F.Cu", "F.Paste", "F.Mask"] };
    const name = await footprintWith(t, [circle], [seg("F.Cu", 0), seg("F.Mask", 2, 300_002), seg("F.Paste", 3), seg("F.SilkS", 0)]);
    const fp = await footprint(name);
    t(fp.graphics.length === 4, `four lines: ${fp.graphics.length}`);
    await select([fp.pads[0].id]);
    await __eda.run("pcbnew.PadTool.explodePad");
    t(await until(async () => !!(await footprint(name)).pad_edit), "the pad is being edited as graphic shapes");
    await __eda.run("pcbnew.PadTool.recombinePad");
    t(await until(async () => !(await footprint(name)).pad_edit), "Finish Pad Edit ends the mode");
    const after = await footprint(name);
    const left = after.graphics.map((g) => `${g.layer}@${g.start.x}`).sort().join();
    t(left === "F.Paste@3,F.SilkS@0", `the F.Mask copy (2 nm) goes with the merged line, the 3 nm one and the silkscreen stay: ${left}`);
    t(after.pads[0].shape === "custom" && after.pads[0].custom?.primitives?.length === 1, `one basic shape, the copy not added twice: ${JSON.stringify(after.pads[0].custom?.primitives?.length)}`);
    const mod = await (await fetch(`/api/footprint/export?name=${encodeURIComponent(name)}`)).text();
    t(/\(pad "1" smd custom/.test(mod) && (mod.match(/\(fp_line/g) ?? []).length === 2, "the exported .kicad_mod has the custom pad and the two lines left");
  });

  // ------------------------------------------------------------------------------------------------ leave the library as it was
  await closeDialogs();
  for (const n of made) await cmd({ op: "delete_library_footprint", name: n });
  await sleep(500);
  const after = await fpNames();
  const clean = JSON.stringify(after) === JSON.stringify(libraryBefore);
  results.push({ name: "the project library is left as it was found", ok: clean, notes: [clean ? "ok: same names" : `FAIL: ${JSON.stringify({ before: libraryBefore, after })}`] });
  check.done = true;
  check.ok = results.every((r) => r.ok);
})();
