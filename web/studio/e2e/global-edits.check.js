// Global edits check (UI-LEFT P12; PARITY-pcb.md section 76): Edit Text & Graphics Properties on the footprints' items -- the Reference designators, Values, Footprint
// graphic items and Footprint text items scopes, the By layer, By parent reference designator, By parent footprint library link and Selected items only filters, the
// Text width / height, Bold, Italic, Keep upright, Visible (fields only), Font, Line thickness and Center on footprint rows, Set to layer default values -- and Edit
// Track & Via Properties' Microvias box, Via annular rings and Via protection features (IPC-4761). Each Apply and Close is checked against `/api/state` and undone
// with ONE Undo, back to the board as it was.
//
// Run it in the page of a served board with footprints placed (examples/mcu_board_30plus.yaml after `eda place`, served with KICAD_PATH set so the footprints have
// their KiCad art), on a COPY of a board. It answers through `window.__globalEditsCheck` = { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__globalEditsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__globalEditsSched) {
    // One scheduler for every timer, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 4e6;
    let running = false;
    const drive = async () => {
      if (running) return;
      running = true;
      try {
        while (timers.size > 0) {
          await fetch("/index.html").then((r) => r.text());
          const now = performance.now();
          for (const [id, t] of [...timers]) {
            if (t.due > now || !timers.has(id)) continue;
            if (t.every == null) timers.delete(id); else t.due = now + t.every;
            try { t.fn(...t.args); } catch (e) { console.error(e); }
          }
        }
      } finally { running = false; }
    };
    window.setTimeout = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, args }); void drive(); return id; };
    window.setInterval = (fn, ms = 0, ...args) => { const id = ++next; timers.set(id, { fn, due: performance.now() + ms, every: Math.max(ms, 1), args }); void drive(); return id; };
    window.clearTimeout = window.clearInterval = (id) => { timers.delete(id); };
    window.requestAnimationFrame = (fn) => window.setTimeout(() => fn(performance.now()), 16);
    window.cancelAnimationFrame = window.clearTimeout;
    window.__globalEditsSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(700); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const d = document.querySelector(".dialog"); const b = d && buttonText(d, "Close"); if (!b) break; b.click(); await sleep(200); } };
  const dlg = () => document.querySelector(".dialog");
  const byAria = (a) => dlg()?.querySelector(`[aria-label="${a}"]`);
  const tri = (a) => byAria(a)?.dataset.state;
  /** Click a three-state box until it says `want` (undetermined -> unchecked -> checked -> undetermined). */
  const setTri = async (a, want) => { for (let i = 0; i < 3 && tri(a) !== want; i++) { byAria(a).click(); await sleep(80); } };
  const tgCounts = () => {
    // (the footprint dimensions' count, UI-LEFT-2 PP23, is left out: the six of before)
    const m = /(\d+) field\(s\), (\d+) footprint text\(s\), (\d+) footprint graphic\(s\),(?: (\d+) footprint dimension\(s\),)? (\d+) graphic item\(s\), (\d+) text\(s\) and (\d+) dimension\(s\)/.exec(document.querySelector('[data-testid="global-edit-text-graphics-count"]')?.textContent ?? "");
    return m ? [m[1], m[2], m[3], m[5], m[6], m[7]].map(Number) : null;
  };
  const tvCounts = () => { const m = /(\d+) track\(s\) and (\d+) via\(s\)/.exec([...dlg().querySelectorAll("div")].map((d) => d.textContent).find((x) => /match the current scope/.test(x) && x.length < 140) ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  const field = (p, name) => p?.fields?.find((f) => f.name === name);
  /** What the scenarios touch: every placed footprint's fields, texts and graphics, and the vias. */
  const snapshot = async () => {
    const st = await state();
    const fps = {};
    for (const p of st.parts) if (p.placed) fps[p.ref] = { fields: p.fields, texts: p.texts, graphics: p.graphics };
    return JSON.stringify({ fps, vias: st.routing?.vias ?? [] });
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };
  const openTextAndGraphics = async () => { const r = await __eda.run("pcbnew.GlobalEdit.editTextAndGraphics"); await sleep(700); return r; };
  const apply = async () => { buttonText(dlg(), "Apply and Close").click(); await sleep(1800); };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  await sleep(300);
  const st0 = await state();
  const placed = st0.parts.filter((p) => p.placed);
  const rParts = placed.filter((p) => /^R\d+$/.test(p.ref));
  const cParts = placed.filter((p) => /^C\d+$/.test(p.ref));
  const dPart = placed.find((p) => /^D\d+$/.test(p.ref) && (p.texts ?? []).length > 0);
  if (rParts.length < 2 || cParts.length < 1 || !dPart || !cParts.some((p) => (p.graphics ?? []).some((g) => g.layer === "F.SilkS"))) {
    results.push({ name: "setup", ok: false, notes: ["FAIL: the board needs placed R, C and D footprints with their KiCad art (serve it with KICAD_PATH set)"] });
    check.done = true;
    return "no board";
  }
  const base = await snapshot();

  (async () => {
    // ----- the fields of the footprints: By parent reference designator, the text rows
    await scenario("Edit Text & Graphics: Reference designators of the footprints R*, with Text width / height, Bold, Keep upright, Visible, Font; one undo step", async (t) => {
      const r = await openTextAndGraphics();
      t(r.dialog === "Edit Text & Graphics Properties", `the action opens the dialog (${r.dialog})`);
      t(JSON.stringify(tgCounts()) === "[0,0,0,0,0,0]", `nothing in scope until a box is ticked (${tgCounts()})`);
      t(["Bold", "Italic", "Keep upright", "Visible  (fields only)"].every((a) => tri(a) === "undetermined"), "the three-state rows start undetermined (leave unchanged)");
      t(byAria("Text height").value === "" && byAria("Text height").placeholder === "-- leave unchanged --", "the value boxes start on -- leave unchanged --");
      byAria("Reference designators").click(); await sleep(120);
      t(tgCounts()[0] === placed.length && tgCounts()[1] === placed.filter((p) => (p.texts ?? []).some((x) => x.role === "reference")).length, `Reference designators: every footprint's Reference and its \${REFERENCE} text (${tgCounts()})`);
      setValue(byAria("Reference filter"), "r*"); await sleep(150);
      t(byAria("By parent reference designator").checked, "typing in the filter ticks its box (OnReferenceFilterText)");
      t(tgCounts()[0] === rParts.length, `By parent reference designator r* (case-blind wildcards): ${tgCounts()[0]} of ${rParts.length} R fields`);
      // the size check (`UNIT_BINDER::Validate`): nothing is sent
      setValue(byAria("Text width"), "0.0001"); await sleep(100);
      await setTri("Bold", "checked");
      await apply();
      t(dlg()?.querySelector('[role="alert"]')?.textContent === "Text width must be at least 0.001 mm.", `a width under 1 um is refused with KiCad's message (${dlg()?.querySelector('[role="alert"]')?.textContent})`);
      t((await snapshot()) === base, "and nothing changed");
      setValue(byAria("Text width"), "1.2");
      setValue(byAria("Text height"), "1.5");
      await setTri("Keep upright", "unchecked");
      await setTri("Visible  (fields only)", "unchecked");
      const fonts = [...byAria("Font").options].map((o) => o.value);
      t(fonts[0] === "-- leave unchanged --" && fonts[1] === "KiCad Font", `the font list: -- leave unchanged --, KiCad Font, the families (${fonts.slice(0, 3)})`);
      const face = fonts[2] ?? null;
      if (face) setValue(byAria("Font"), face);
      await sleep(100);
      await apply();
      t(!dlg(), "Apply and Close closes the dialog");
      const st = await state();
      const mine = rParts.map((p) => field(st.parts.find((q) => q.ref === p.ref), "Reference"));
      t(mine.every((f) => f.w === 1_200_000 && f.h === 1_500_000 && f.bold && !f.upright && !f.visible), `every R reference is 1.2 x 1.5 mm, bold, not kept upright, hidden (${JSON.stringify(mine[0])})`);
      if (face) t(mine.every((f) => f.face === face), `and in the font ${face}`);
      const others = placed.filter((p) => !/^R\d+$/.test(p.ref)).map((p) => JSON.stringify(field(st.parts.find((q) => q.ref === p.ref), "Reference")));
      t(others.every((f, i) => f === JSON.stringify(field(placed.filter((p) => !/^R\d+$/.test(p.ref))[i], "Reference"))), "the other footprints' references are untouched");
      const refTexts = rParts.map((p) => st.parts.find((q) => q.ref === p.ref).texts.find((x) => x.role === "reference"));
      t(refTexts.every((x) => x && x.w === 1_200_000 && x.h === 1_500_000 && x.bold && !x.upright), "their ${REFERENCE} texts of the fab layer went with the Reference designators box, in each footprint's own copy (Visible is for fields only)");
      await undo();
      t((await snapshot()) === base, "one Undo takes the whole edit back");
    });

    // ----- the footprints' graphics: By layer and By parent footprint library link
    await scenario("Edit Text & Graphics: Footprint graphic items on F.SilkS of the footprints linked to 0402, Line thickness; one undo step", async (t) => {
      await openTextAndGraphics();
      byAria("Footprint graphic items").click(); await sleep(100);
      const all = placed.reduce((n, p) => n + (p.graphics ?? []).length, 0);
      t(tgCounts()[2] === all, `Footprint graphic items: every footprint's graphics (${tgCounts()[2]} of ${all})`);
      byAria("Filter by layer").click(); setValue(byAria("Layer filter"), "F.SilkS"); await sleep(120);
      const silk = placed.reduce((n, p) => n + (p.graphics ?? []).filter((g) => g.layer === "F.SilkS").length, 0);
      t(tgCounts()[2] === silk, `By layer F.SilkS: ${tgCounts()[2]} of ${silk}`);
      const link = cParts[0].footprint;
      setValue(byAria("Footprint filter"), link); await sleep(120);
      const want = placed.filter((p) => p.footprint === link).reduce((n, p) => n + (p.graphics ?? []).filter((g) => g.layer === "F.SilkS").length, 0);
      t(tgCounts()[2] === want && want > 0, `By parent footprint library link ${link}: ${tgCounts()[2]} of ${want}`);
      setValue(byAria("Line thickness"), "0.25"); await sleep(100);
      await apply();
      const st = await state();
      const mineOk = placed.filter((p) => p.footprint === link).every((p) => {
        const now = st.parts.find((q) => q.ref === p.ref).graphics;
        return now.every((g, k) => (g.layer === "F.SilkS" ? g.stroke_width === 250_000 : g.stroke_width === p.graphics[k].stroke_width));
      });
      t(mineOk, "their silkscreen graphics are 0.25 mm, their fab graphics as they were");
      t(placed.filter((p) => p.footprint !== link).every((p) => JSON.stringify(st.parts.find((q) => q.ref === p.ref).graphics) === JSON.stringify(p.graphics)), "the other footprints' graphics are untouched (another library footprint, or none)");
      await undo();
      t((await snapshot()) === base, "one Undo takes it back");
    });

    // ----- selected items only, Center on footprint, Italic on a footprint's own text
    await scenario(`Edit Text & Graphics: Values and Footprint text items of the selected ${dPart.ref}, Center on footprint and Italic; one undo step`, async (t) => {
      await select([dPart.ref]);
      await openTextAndGraphics();
      byAria("Values").click(); byAria("Footprint text items").click(); await sleep(100);
      byAria("Selected items only").click(); await sleep(120);
      t(tgCounts()[0] === 1 && tgCounts()[1] === dPart.texts.length, `Selected items only: the selected footprint's Value and its own texts, through the footprint (${tgCounts()})`);
      byAria("Center on footprint").click();
      await setTri("Italic", "checked");
      await apply();
      const st = await state();
      const d = st.parts.find((p) => p.ref === dPart.ref);
      const v = field(d, "Value");
      const txt = d.texts[0];
      t(v.italic && txt.italic, "the value and the footprint text are italic");
      t(Math.abs(v.x - txt.x) <= 1 && Math.abs(v.y - txt.y) <= 1, `both are at the centre of the footprint's box (${v.x},${v.y} and ${txt.x},${txt.y})`);
      t(v.x !== field(dPart, "Value").x || v.y !== field(dPart, "Value").y || txt.x !== dPart.texts[0].x || txt.y !== dPart.texts[0].y, "and something moved");
      t(placed.filter((p) => p.ref !== dPart.ref).every((p) => JSON.stringify(field(st.parts.find((q) => q.ref === p.ref), "Value")) === JSON.stringify(field(p, "Value"))), "no other footprint's value moved");
      await undo();
      await select([]);
      t((await snapshot()) === base, "one Undo takes it back");
    });

    // ----- Set to layer default values
    await scenario("Edit Text & Graphics: Set to layer default values on the reference of one footprint", async (t) => {
      const one = rParts[0].ref;
      await openTextAndGraphics();
      byAria("Reference designators").click();
      setValue(byAria("Reference filter"), one); await sleep(120);
      t(tgCounts()[0] === 1, `By parent reference designator ${one}: one field (${tgCounts()})`);
      setValue(byAria("Text height"), "2"); setValue(byAria("Text width"), "2"); await sleep(80);
      await apply();
      t(field((await state()).parts.find((p) => p.ref === one), "Reference").h === 2_000_000, "first set to 2 mm");
      await openTextAndGraphics();
      t(byAria("Reference filter").value === one && !byAria("By parent reference designator").checked, "reopened, the reference filter keeps its text (g_referenceFilter) and its box is off");
      byAria("Reference designators").click(); byAria("By parent reference designator").click(); await sleep(80);
      byAria("Set to layer default values").click(); await sleep(80);
      const grid = [...(byAria("Layer class defaults")?.querySelectorAll("tbody tr") ?? [])].map((r) => r.firstChild.textContent);
      t(JSON.stringify(grid) === JSON.stringify(["Silk Layers", "Copper Layers", "Edge Cuts", "Courtyards", "Fab Layers", "Other Layers"]), `the layer-class grid (${grid})`);
      await apply();
      const silk = (await state()).board_rules?.text_graphics?.silk;
      const f = field((await state()).parts.find((p) => p.ref === one), "Reference");
      t(silk && f.w === silk.text_width_nm && f.h === silk.text_height_nm && f.thickness === silk.text_thickness_nm && f.italic === silk.italic && f.upright === silk.upright, `the reference took the silk class of Board Setup (${f.w} x ${f.h}, ${f.thickness})`);
      await undo();
      t(field((await state()).parts.find((p) => p.ref === one), "Reference").h === 2_000_000, "one Undo goes back to the 2 mm of the first edit");
      await undo();
      t((await snapshot()) === base, "and a second to the board as it was");
    });

    // ----- Edit Track & Via Properties: Microvias, annular rings, IPC-4761
    await scenario("Edit Track & Via Properties: the Microvias box, Via annular rings and Via protection features on the vias of one net; one undo step", async (t) => {
      const netA = (placed.flatMap((p) => p.pads ?? []).map((q) => q.net).filter(Boolean))[0];
      const netB = (placed.flatMap((p) => p.pads ?? []).map((q) => q.net).filter((n) => n && n !== netA))[0];
      const layers = st0.layers;
      const yMax = Math.max(...placed.map((p) => (p.courtyard ? p.courtyard[3] : p.at[1])));
      t((await post({ op: "add_via", net: netA, x: 5_000_000, y: yMax + 4_000_000, drill: 300_000, diameter: 600_000, from_layer: layers[0], to_layer: layers[layers.length - 1] })).ok !== false, `a via on ${netA}`);
      await post({ op: "add_via", net: netB, x: 8_000_000, y: yMax + 4_000_000, drill: 300_000, diameter: 600_000, from_layer: layers[0], to_layer: layers[layers.length - 1] });
      await post({ op: "add_via", net: netB, x: 11_000_000, y: yMax + 4_000_000, drill: 100_000, diameter: 300_000, from_layer: layers[0], to_layer: layers[layers.length - 1] });
      await sleep(600);
      let vias = (await state()).routing.vias;
      const micro = vias.find((v) => v.d === 300_000);
      const mr = await post({ op: "set_via_layers", ids: [micro.id], kind: "micro" });
      await sleep(600);
      vias = (await state()).routing.vias;
      const microOk = vias.find((v) => v.id === micro.id)?.type === "micro";
      t(microOk, `the third via made a microvia (${JSON.stringify(mr).slice(0, 120)})`);
      const before = await snapshot();
      await __eda.run("pcbnew.GlobalEdit.editTracksAndVias"); await sleep(700);
      t(!!byAria("Microvias"), "the Scope box has Microvias");
      byAria("Microvias").click(); await sleep(100);
      t(byAria("Vias").indeterminate, "one of four types ticked leaves the Vias box in between");
      if (microOk) t(JSON.stringify(tvCounts()) === "[0,1]", `Microvias: the one microvia (${tvCounts()})`);
      byAria("Microvias").click(); byAria("Through vias").click(); await sleep(100);
      t(tvCounts()[1] === vias.filter((v) => v.type == null).length, `Through vias: the through vias, not the microvia (${tvCounts()})`);
      byAria("Filter items by net").click(); setValue(byAria("Net filter"), netA); await sleep(150);
      t(JSON.stringify(tvCounts()) === "[0,1]", `Filter items by net ${netA}: its via (${tvCounts()})`);
      const ringsOpts = [...byAria("Via annular rings").options].map((o) => o.textContent);
      t(JSON.stringify(ringsOpts) === JSON.stringify(["All copper layers", "Start, end, and connected layers", "Connected layers only", "Start and end layers only", "-- leave unchanged --"]) && byAria("Via annular rings").value === "", `the annular-ring choices, on -- leave unchanged -- (${ringsOpts})`);
      const prot = [...byAria("Via protection features").options].map((o) => o.textContent);
      t(prot.length === 20 && prot[0] === "From rules" && prot[19] === "-- leave unchanged --", `the IPC-4761 presets and -- leave unchanged -- (${prot.length})`);
      setValue(byAria("Via annular rings"), "start_end_only");
      setValue(byAria("Via protection features"), "ivb"); await sleep(100);
      buttonText(dlg(), "Apply and Close").click(); await sleep(1800);
      const after = (await state()).routing.vias;
      const a = after.find((v) => v.net === netA && v.x === 5_000_000);
      t(a?.unconnected === "start_end_only", `the ${netA} via keeps its start and end layers only (${a?.unconnected})`);
      t(a?.props?.front_tenting === true && a?.props?.back_tenting === true && a?.props?.front_plugging === true && a?.props?.back_plugging === true && a?.props?.filling === false, `and is Type IV-b, plugged and tented both sides (${JSON.stringify(a?.props)})`);
      t(after.filter((v) => v.net === netB).every((v) => !v.unconnected && !v.props), `the ${netB} vias are untouched`);
      await undo();
      t((await snapshot()) === before, "one Undo takes the via rows back");
      for (let i = 0; i < 6 && (await snapshot()) !== base; i++) await undo();
      t((await snapshot()) === base, "the setup undone, the board is as it was");
    });

    // -------------------------------------------------------------------------------------------------------------------- the end
    results.push({ name: "end: the board is as it was", ok: (await snapshot()) === base, notes: [`${(await snapshot()) === base ? "ok" : "FAIL"}: every edit was undone`] });
    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
