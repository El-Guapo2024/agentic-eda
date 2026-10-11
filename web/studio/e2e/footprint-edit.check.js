// Footprint editing check (GAPS.md items 9 and 22): a footprint's Reference, Value and user fields are items of the board, the Properties panel and the Footprint
// Properties / Pad Properties dialogs edit them and the footprint's attributes and pads (and show the zone filler's connection fields beside them), Copy and Paste keep a
// footprint's pad edit and zone overrides, and Create Array copies, arranges and numbers footprints and other items.
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY
// of a board (it edits and undoes, and it leaves Strict off while it runs). It makes nothing that stays: every edit is undone and the board compared with how it was.
//
// It needs a placed footprint with two or more pads and a reference (it uses the first three footprints that are placed, `R1`/`U1`/`D1` in
// examples/mcu_board_30plus.yaml after `eda board place U1 --region centre` and a few `--near U1`), and it answers through `window.__fpEditCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// A run takes a minute or two (each edit waits for the board's answer), longer than a tool call may last, so it runs on its own: read it with
// `await __fpEditCheck.wait(40000)` (it returns when the run is done or the time is up) until `.done`. In a hidden pane the browser stops timers; the script
// then drives them from a fetch loop (the `__eda.run` waits would never end otherwise).
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, open a dialog with the action that
// opens it), the Properties grid's own inputs, the dialogs' controls (their `data-testid`s) and the pointer events a user makes on the canvas.
(async () => {
  const results = [];
  const check = (window.__fpEditCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 40000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  if (document.visibilityState === "hidden" && !window.__fpEditSched) {
    // One scheduler for every timer, driven by a fetch loop (see above).
    const timers = new Map();
    let next = 3e6;
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
    window.__fpEditSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const fieldOf = async (ref, name) => (await partOf(ref))?.fields?.find((f) => f.name === name);
  const padOf = async (id) => (await partOf(id.split(".")[0]))?.pads?.find((p) => p.id === id);
  const placedRefs = async () => (await state()).parts.filter((p) => p.placed).map((p) => p.ref).sort();
  /** What the edits touch, by footprint: fields, attributes, pads' extras, the zone connection facts, pose. A copy made by an array shows up as one more entry. */
  const snapshot = async () => {
    const m = {};
    for (const p of (await state()).parts) if (p.placed) m[p.ref] = { at: p.at, rot: p.rot, side: p.side, fields: p.fields, attrs: p.attrs, zone: p.zone ?? null, pads: (p.pads ?? []).map((q) => [q.id, q.shape, q.size, q.offset, q.drill, q.slot, q.mask_margin, q.paste_margin, q.edit]) };
    return JSON.stringify(m);
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(450); };
  const lastAct = async () => (await state()).activity?.[0] ?? null;
  const waitAct = async (before) => {
    const t0 = Date.now();
    while (Date.now() - t0 < 5000) {
      const a = await lastAct();
      if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(350); return a; }
      await sleep(40);
    }
    return null;
  };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (id, v) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); setValue(el, v); await sleep(40); };
  const click = async (id) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); el.click(); await sleep(60); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const open = async (action) => { const r = await __eda.run(action); await sleep(250); return r; };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = buttonText(document, "Cancel"); if (!b) break; b.click(); await sleep(150); } };

  // The Properties grid: set a row's editor the way a user does, and wait for the command it sends.
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const edit = async (name, value) => {
    const el = input(name);
    if (!el) return `no editable row ${name}`;
    const before = await lastAct();
    if (el instanceof HTMLSelectElement) setValue(el, value);
    else if (el.type === "checkbox") { if (el.checked === !!value) return "already"; el.click(); }
    else { el.focus(); setValue(el, value); await sleep(30); el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); }
    const a = await waitAct(before);
    if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
    return a.ok ? `ok: ${a.cmd.slice(0, 80)}` : `REFUSED: ${a.message.slice(0, 100)}`;
  };
  const current = (name) => { const el = input(name); return el ? (el.type === "checkbox" ? el.checked : el.value) : null; };
  const other = (name) => { const el = input(name); return el instanceof HTMLSelectElement ? [...el.options].find((o) => !o.disabled && o.value !== "" && o.value !== el.value)?.value : undefined; };

  // The canvas: the pointer events a user makes. The cursor readout of the status bar gives the screen-to-board mapping.
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  /** Screen position (canvas pixels) of a board point in mm, from two probes of the cursor readout. */
  const screenOf = async () => {
    await __eda.run("common.Control.zoomFitObjects"); // a page that loaded before the canvas had a size has no valid view until it is fitted
    await sleep(300);
    fire("pointermove", 100, 100); await sleep(120); const a = readout();
    fire("pointermove", 500, 400); await sleep(120); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };
  const near = (a, b, tol = 2) => Math.abs(a - b) <= tol;

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click(); // a gate that refuses what makes the board worse would refuse the array copies
  document.querySelector('button[aria-label="Show Properties"]')?.click();
  await sleep(300);

  const placed = (await state()).parts.filter((p) => p.placed);
  const A = placed.find((p) => (p.pads ?? []).length >= 2 && p.fields);
  const U = placed.find((p) => (p.pads ?? []).length >= 4) ?? A;
  const D = placed.find((p) => p.ref !== A?.ref && p.ref !== U?.ref) ?? A;
  if (!A) { results.push({ name: "setup", ok: false, notes: ["FAIL: no placed footprint with two pads and fields in /api/state"] }); check.done = true; return "no board"; }
  const refA = A.ref;
  const refU = U.ref;
  const refD = D.ref;
  const base = await snapshot();
  const baseRefs = await placedRefs();

  (async () => {
    // ----- a field is an item: selected, moved with Move Exactly, undone
    await scenario("field: selected as an item of the board, Move Exactly moves it, undo puts it back", async (t) => {
      const f0 = await fieldOf(refA, "Reference");
      await select([`${refA}:Reference`]);
      t(__eda.state().selection[0]?.kind === "field", `the selection is a field (${JSON.stringify(__eda.state().selection[0])})`);
      await open("pcbnew.InteractiveEdit.moveExact");
      const nums = [...document.querySelectorAll(".dialog input[type=number]")];
      setValue(nums[0], 2);
      setValue(nums[1], 1);
      await sleep(100);
      buttonText(document, "OK").click();
      await sleep(900);
      const f1 = await fieldOf(refA, "Reference");
      t(f1.x === f0.x + 2000 && f1.y === f0.y + 1000, `moved by (2, 1) mm: (${f0.x},${f0.y}) -> (${f1.x},${f1.y})`);
      t(f1.lx === f0.lx + 2000 && f1.ly === f0.ly + 1000, `its place in the footprint's own frame moved with it (${f0.lx},${f0.ly}) -> (${f1.lx},${f1.ly})`);
      t((await snapshot()) !== base, "the board differs while it is moved");
      await undo();
      t((await snapshot()) === base, "undo puts the board back");
    });

    // ----- M: the move tool carries a field
    await scenario("field: the Move tool (M) carries it with the pointer, one undo step", async (t) => {
      const f0 = await fieldOf(refA, "Reference");
      await select([`${refA}:Reference`]);
      const to = await screenOf();
      t(!!to, "the cursor readout gives the mapping to the board");
      if (!to) return;
      const [px, py] = to(f0.x / 1000, f0.y / 1000);
      fire("pointermove", px, py); await sleep(150);
      await open("pcbnew.InteractiveMove.move");
      fire("pointermove", px + 60, py + 25); await sleep(250);
      fire("pointerdown", px + 60, py + 25, { buttons: 1 }); await sleep(100);
      fire("pointerup", px + 60, py + 25); await sleep(900);
      const f1 = await fieldOf(refA, "Reference");
      t(f1.x > f0.x && f1.y > f0.y, `moved right and down: (${f0.x},${f0.y}) -> (${f1.x},${f1.y})`);
      t((await partOf(refA)).at.join() === A.at.join(), "its footprint stayed where it was");
      await undo();
      t((await snapshot()) === base, "one undo puts the board back");
    });

    // ----- drag: pressing on a field and dragging it moves the field alone
    await scenario("field: dragging it with the pointer moves it, and not the footprint", async (t) => {
      const f0 = await fieldOf(refA, "Reference");
      await __eda.run("common.InteractiveSelection.clear");
      const to = await screenOf();
      if (!to) return t(false, "no mapping");
      const [px, py] = to(f0.x / 1000, f0.y / 1000);
      fire("pointermove", px, py, { buttons: 0 }); await sleep(120);
      fire("pointerdown", px, py, { buttons: 1 }); await sleep(100);
      fire("pointermove", px + 40, py + 20, { buttons: 1 }); await sleep(150);
      fire("pointermove", px + 80, py + 40, { buttons: 1 }); await sleep(150);
      fire("pointerup", px + 80, py + 40); await sleep(900);
      const f1 = await fieldOf(refA, "Reference");
      t(__eda.state().selection[0]?.id === `${refA}:Reference` || f1.x !== f0.x, `the field was picked (${JSON.stringify(__eda.state().selection)})`);
      t(f1.x !== f0.x || f1.y !== f0.y, `the field moved: (${f0.x},${f0.y}) -> (${f1.x},${f1.y})`);
      t((await partOf(refA)).at.join() === A.at.join(), "the footprint did not");
      await undo();
      t((await snapshot()) === base, "undo puts the board back");
    });

    // ----- the Appearance panel's rows hide fields
    await scenario("Appearance: the References, Values and Footprint Text rows hide a footprint's fields, and a field that is hidden cannot be picked", async (t) => {
      const f0 = await fieldOf(refA, "Reference");
      const v0 = await fieldOf(refA, "Value");
      [...document.querySelectorAll(".dock-tab")].find((e) => /^Objects$/.test(e.textContent.trim()))?.click();
      await sleep(300);
      const box = (id) => document.querySelector(`.ap-row[data-object="${id}"] .ap-eye`); // the eye of an Objects row
      const to = await screenOf();
      if (!to) return t(false, "no mapping");
      const at = (f) => to(f.x / 1000, f.y / 1000);
      const pick = async (f) => {
        const [px, py] = at(f);
        await __eda.run("common.InteractiveSelection.clear");
        fire("pointermove", px, py, { buttons: 0 }); await sleep(120);
        fire("pointerdown", px, py, { buttons: 1 }); await sleep(100);
        fire("pointerup", px, py); await sleep(400);
        return __eda.state().selection.map((x) => x.id).join();
      };
      t((await pick(f0)) === `${refA}:Reference` && (await pick(v0)) === `${refA}:Value`, "with the rows on a press on the Reference or the Value picks it");
      const refs = box("footprint_references");
      refs.click(); await sleep(250);
      t((await pick(f0)) !== `${refA}:Reference`, "with References off the Reference is not picked");
      t((await pick(v0)) === `${refA}:Value`, "and the Value still is");
      refs.click(); await sleep(250);
      const values = box("footprint_values");
      values.click(); await sleep(250);
      t((await pick(v0)) !== `${refA}:Value` && (await pick(f0)) === `${refA}:Reference`, "with Values off it is the Value that is not picked");
      values.click(); await sleep(250);
      const text = box("footprint_text");
      text.click(); await sleep(250);
      t((await pick(f0)) !== `${refA}:Reference` && (await pick(v0)) !== `${refA}:Value`, "with Footprint Text off neither is");
      text.click(); await sleep(250);
      t((await pick(f0)) === `${refA}:Reference`, "the rows back on, the Reference is picked again");
      await __eda.run("common.InteractiveSelection.clear");
      [...document.querySelectorAll(".dock-tab")].find((e) => /^Layers$/.test(e.textContent.trim()))?.click();
    });

    // ----- the Properties panel edits a field
    await scenario("field: the Properties panel edits visibility, thickness, justification and layer; each is one undo step", async (t) => {
      await select([`${refA}:Reference`]);
      let applied = 0;
      for (const [name, v, key, want] of [
        ["Visible", false, "visible", false],
        ["Thickness", String(Math.round(Number(current("Thickness")) * 1000 + 50) / 1000), "thickness", null],
        ["Horizontal Justification", other("Horizontal Justification"), "halign", null],
        ["Layer", "F.Fab", "layer", "F.Fab"],
      ]) {
        const r = await edit(name, v);
        t(r.startsWith("ok"), `${name} -> ${r}`);
        if (r.startsWith("ok")) applied++;
        const f = await fieldOf(refA, "Reference");
        if (want !== null) t(f[key] === want, `${key} is now ${JSON.stringify(f[key])}`);
        else t(f[key] !== A.fields.find((x) => x.name === "Reference")[key], `${key} changed to ${JSON.stringify(f[key])}`);
      }
      for (let i = 0; i < applied; i++) await undo();
      t((await snapshot()) === base, "undoing every edit puts the board back");
    });

    // ----- the Properties panel edits a footprint's attributes
    await scenario("footprint: the Properties panel sets Do not Populate, Exclude From BOM and Component Type; the symbol follows", async (t) => {
      await select([refA]);
      let applied = 0;
      for (const [name, v] of [["Do not Populate", true], ["Exclude From Bill of Materials", true], ["Exclude From Position Files", true], ["Component Type", "through_hole"]]) {
        const r = await edit(name, v);
        t(r.startsWith("ok"), `${name} -> ${r}`);
        if (r.startsWith("ok")) applied++;
      }
      const attrs = (await partOf(refA)).attrs;
      t(attrs.dnp && attrs.exclude_from_bom && attrs.exclude_from_pos_files && attrs.kind === "through_hole", `attributes are ${JSON.stringify(attrs)}`);
      for (let i = 0; i < applied; i++) await undo();
      t((await snapshot()) === base, "undoing every edit puts the board back");
    });

    // ----- the Footprint Properties dialog: fields, placement and attributes, one undo step
    await scenario("Footprint Properties: reference size, a new user field, orientation and attributes in one OK, one undo", async (t) => {
      await select([refA]);
      await open("pcbnew.InteractiveEdit.properties");
      t(!!tid("footprint-properties"), "the dialog is open");
      const grid = tid("footprint-fields-grid");
      const rows = () => [...(grid?.querySelectorAll('[data-testid^="field-row-"]') ?? [])];
      const n0 = rows().length;
      const ref = rows()[0];
      const nums = [...ref.querySelectorAll("input[type=number]")];
      setValue(nums[0], 1.5); setValue(nums[1], 1.5); setValue(nums[2], 0.2); // width, height, thickness
      await click("add-field");
      t(rows().length === n0 + 1, "Add Field adds a row");
      const added = rows()[rows().length - 1];
      const texts = [...added.querySelectorAll("input")].filter((i) => i.type === "text"); // an input with no type attribute is a text input too
      setValue(texts[0], "MPN"); setValue(texts[1], "RC0603FR-07330RL");
      await type("fp-orientation", 90);
      const dnp = [...document.querySelectorAll('[data-testid="footprint-properties"] label')].find((l) => /do not populate/i.test(l.textContent))?.querySelector("input");
      dnp?.click();
      await sleep(150);
      await click("footprint-properties-ok");
      await sleep(1000);
      const p = await partOf(refA);
      const r = p.fields.find((f) => f.name === "Reference");
      t(r.w === 1500 && r.h === 1500 && r.thickness === 200, `reference text is ${r.w} x ${r.h}, ${r.thickness} thick`);
      const mpn = p.fields.find((f) => f.name === "MPN");
      t(!!mpn && mpn.text === "RC0603FR-07330RL" && mpn.custom, `the user field MPN is there: ${JSON.stringify(mpn && { text: mpn.text, custom: mpn.custom, layer: mpn.layer })}`);
      t(p.attrs.dnp === true, "Do not populate is set");
      t(p.rot === 270, `the footprint turned 90 degrees counter-clockwise, KiCad's orientation (the board's rot runs clockwise): rot ${p.rot}`);
      t(!tid("footprint-properties"), "the dialog closed");
      await undo();
      t((await snapshot()) === base, "ONE undo puts the board back (fields, attributes and pose)");
    });

    // ----- user fields: added, renamed, deleted, and a bad name refused
    await scenario("Footprint Properties: a user field is added, renamed and deleted (each OK is one undo step); a reserved name is refused", async (t) => {
      const props = "pcbnew.InteractiveEdit.properties";
      const rowsNow = () => [...(tid("footprint-fields-grid")?.querySelectorAll('[data-testid^="field-row-"]') ?? [])];
      const textsOf = (r) => [...r.querySelectorAll("input")].filter((i) => i.type === "text");
      const ok = async () => { await click("footprint-properties-ok"); await sleep(1000); };
      await select([refA]); await open(props);
      await click("add-field");
      let texts = textsOf(rowsNow().at(-1));
      setValue(texts[0], "Alpha"); setValue(texts[1], "one");
      await ok();
      t(!!(await fieldOf(refA, "Alpha")), "Alpha was added");
      await select([refA]); await open(props);
      const alpha = rowsNow().find((r) => textsOf(r)[0]?.value === "Alpha");
      t(!!alpha, "the dialog lists Alpha");
      setValue(textsOf(alpha)[0], "Beta");
      await ok();
      t(!(await fieldOf(refA, "Alpha")) && !!(await fieldOf(refA, "Beta")), "Alpha was renamed Beta");
      t((await fieldOf(refA, "Beta"))?.text === "one", "its text came with it");
      await select([refA]); await open(props);
      await click("add-field");
      texts = textsOf(rowsNow().at(-1));
      setValue(texts[0], "Reference");
      const before = await lastAct();
      await click("footprint-properties-ok"); await sleep(300);
      t(/two fields called|reserved|unique|already|duplicate/i.test(tid("footprint-properties-error")?.textContent ?? ""), `a reserved name is refused: ${JSON.stringify(tid("footprint-properties-error")?.textContent)}`);
      t(JSON.stringify(await lastAct()) === JSON.stringify(before), "nothing was sent");
      await closeDialogs();
      await select([refA]); await open(props);
      const beta = rowsNow().find((r) => textsOf(r)[0]?.value === "Beta");
      beta.click(); await sleep(80);
      await click("delete-field");
      await ok();
      t(!(await fieldOf(refA, "Beta")), "Beta was deleted");
      await undo(); await undo(); await undo();
      t((await snapshot()) === base, "three undos (add, rename, delete) put the board back");
    });

    // ----- E or a double click on a field opens its footprint's dialog on that field's row
    await scenario("Footprint Properties: asked for a field (E on it) the dialog starts on that field's row", async (t) => {
      await select([`${refA}:Value`]);
      await open("pcbnew.InteractiveEdit.properties");
      const rows = [...document.querySelectorAll('[data-testid^="field-row-"]')];
      t(rows.length >= 2 && rows[1].style.background !== "" && rows[0].style.background === "", `the Value row is the selected one (${rows.map((r) => (r.style.background ? "selected" : "-")).join(" ")})`);
    });

    // ----- Flip turns the fields over with the footprint
    await scenario("footprint: Flip takes its Reference and Value to the other side's layers, mirrored, in one undo step", async (t) => {
      await select([refA]);
      await __eda.run("pcbnew.InteractiveEdit.flip");
      await sleep(900);
      const p = await partOf(refA);
      const r = p.fields.find((f) => f.name === "Reference");
      const v = p.fields.find((f) => f.name === "Value");
      t(p.side === "bottom", `the footprint is on the ${p.side}`);
      t(r.layer === "B.SilkS" && r.mirror === true, `the Reference is on ${r.layer}, mirrored: ${r.mirror}`);
      t(v.layer === "B.Fab" && v.mirror === true, `the Value is on ${v.layer}, mirrored: ${v.mirror}`);
      await undo();
      t((await snapshot()) === base, "one undo turns it all back");
    });

    // ----- the Pad Properties dialog
    await scenario("Pad Properties: shape, size, offset, clearance and the zone connection in one OK, one undo", async (t) => {
      const padId = U.pads[0].id;
      const was = U.pads[0];
      await select([padId]);
      await open("pcbnew.InteractiveEdit.properties");
      t(!!tid("board-pad-properties"), "the dialog is open");
      await type("pad-shape", "oval");
      await type("pad-size-x", 2.2);
      await type("pad-size-y", 0.7);
      await type("pad-offset-x", 0.1);
      await click("pad-clearance-on");
      await type("pad-clearance", 0.15);
      t(!!tid("pad-zone"), "the zone connection fields are in the dialog");
      await type("pad-zone-connection", "Full");
      await click("pad-zone-gap-on");
      await type("pad-zone-gap", 0.4);
      await click("board-pad-properties-ok");
      await sleep(1000);
      const pad = await padOf(padId);
      t(pad.shape === "oval", `shape is ${pad.shape}`);
      t(pad.size?.join() === "2200,700", `size is ${pad.size}`);
      t(pad.offset?.[0] === 100, `offset is ${JSON.stringify(pad.offset)}`);
      const zonePad = (await partOf(U.ref)).zone?.pads.find((z) => z.num === U.pads[0].num);
      t(zonePad?.clearance === 150, `clearance is ${zonePad?.clearance} (a pad has one, the zone overlay's)`);
      t(zonePad?.connection === "Full" && zonePad?.gap === 400, `zone connection ${zonePad?.connection}, relief gap ${zonePad?.gap}`);
      t(pad.edited === true, "the pad is marked as edited");
      await undo();
      t((await snapshot()) === base, `one undo puts the pad back (was ${was.shape} ${was.size})`);
    });

    // ----- Pad Properties refuses what the pad check refuses
    await scenario("Pad Properties: a pad with no size is refused in the check's words, and nothing is sent", async (t) => {
      const padId = U.pads[0].id;
      await select([padId]);
      await open("pcbnew.InteractiveEdit.properties");
      const before = await lastAct();
      await type("pad-size-x", 0);
      await click("board-pad-properties-ok");
      await sleep(300);
      t(/positive size/i.test(tid("board-pad-properties-error")?.textContent ?? ""), `message: ${tid("board-pad-properties-error")?.textContent}`);
      t(JSON.stringify(await lastAct()) === JSON.stringify(before), "no command was sent");
    });

    // ----- the zone filler's fields are in the same dialogs
    await scenario("Footprint Properties: the zone connection fields sit with the field grid and the attributes; one OK sends them together, one undo takes them all", async (t) => {
      const num = A.pads[0].num;
      await select([refA]);
      await open("pcbnew.InteractiveEdit.properties");
      t(!!tid("footprint-fields-grid") && !!tid("fp-zone"), "the field grid and the zone connection fields are in the dialog");
      await click("add-field");
      const rows = [...tid("footprint-fields-grid").querySelectorAll('[data-testid^="field-row-"]')];
      const texts = [...rows.at(-1).querySelectorAll("input")].filter((i) => i.type === "text");
      setValue(texts[0], "Zoned"); setValue(texts[1], "both");
      [...document.querySelectorAll('[data-testid="footprint-properties"] label')].find((l) => /do not populate/i.test(l.textContent))?.querySelector("input")?.click();
      await type("fp-zone-connection", "None");
      await click("fp-zone-clearance-on");
      await type("fp-zone-clearance", 0.3);
      await type("fp-zone-pad-connection", "Full");
      await click("fp-zone-pad-gap-on");
      await type("fp-zone-pad-gap", 0.4);
      await click("fp-zone-pad-clearance-on");
      await type("fp-zone-pad-clearance", 0.25);
      await click("footprint-properties-ok");
      await sleep(1200);
      const p = await partOf(refA);
      const z = p.zone?.pads.find((x) => x.num === num);
      t(p.zone?.connection === "None" && p.zone?.clearance === 300, `the footprint's zone connection ${p.zone?.connection} and clearance ${p.zone?.clearance}`);
      t(z?.connection === "Full" && z?.gap === 400 && z?.clearance === 250, `pad ${num}'s zone fields ${JSON.stringify(z)}`);
      t(!!p.fields.find((f) => f.name === "Zoned") && p.attrs.dnp === true, "the new field and Do not populate went in the same OK");
      await undo();
      t((await snapshot()) === base, "ONE undo takes the fields, the attribute and the zone fields all back");
    });

    // ----- copy and paste carry both
    await scenario("Copy and Paste: a pasted footprint has the pad edit and the zone overrides of the one copied", async (t) => {
      const num = A.pads[0].num;
      const post = (cmd) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) }).then((r) => r.json());
      const r1 = await post({ op: "edit_board_pad", part: refA, edit: { number: num, nth: 1, offset: { x: 100, y: 0 }, solder_mask_margin: 50 } });
      const r2 = await post({ op: "set_pad_zone_overrides", part: refA, pad: num, zone_connection: "Full", thermal_gap: 400, thermal_spoke_width: null, thermal_spoke_angle_mdeg: null, clearance: 250 });
      t(r1.ok && r2.ok, `both overlays are set on ${refA} (${r1.message} / ${r2.message})`);
      await sleep(400);
      await select([refA]);
      // A pane that will not hand the system clipboard over (no focus, no permission) makes a Paste take the copy this session made: say so, so that it does not wait.
      const clip = navigator.clipboard;
      const realRead = clip?.readText;
      if (clip) clip.readText = () => Promise.reject(new Error("the system clipboard is not read here"));
      await __eda.run("common.Interactive.copy");
      await sleep(500);
      await __eda.run("common.Interactive.paste");
      await sleep(1500);
      if (clip && realRead) clip.readText = realRead;
      const added = (await placedRefs()).filter((r) => !baseRefs.includes(r));
      t(added.length === 1, `one footprint was pasted: ${added.join(", ")}`);
      if (added.length === 1) {
        const q = await partOf(added[0]);
        const pad = q.pads.find((x) => x.num === num);
        t(pad.offset?.[0] === 100 && pad.mask_margin === 50, `the pasted pad has the edit's offset and margin (${JSON.stringify([pad.offset, pad.mask_margin])})`);
        const zone = q.zone?.pads.find((x) => x.num === num);
        t(zone?.connection === "Full" && zone?.gap === 400 && zone?.clearance === 250, `and the zone overrides (${JSON.stringify(zone)})`);
      }
      await __eda.run("common.Interactive.cancel"); // Escape takes a carried paste away again
      await sleep(900);
      for (let i = 0; i < 3 && (await placedRefs()).length > baseRefs.length; i++) await undo();
      await undo(); await undo(); // the zone overrides, then the pad edit
      t((await snapshot()) === base && (await placedRefs()).join() === baseRefs.join(), "the board is as it was after the paste and the two edits are undone");
    });

    // ----- Create Array: a grid of one footprint, in place; the refs are unique; one undo step
    await scenario("Create Array: a 3 x 2 grid of a footprint, source stays in place, unique references, one undo", async (t) => {
      await select([refA]);
      await open("pcbnew.Array.createArray");
      t(!!tid("create-array"), "the dialog is open");
      await click("array-tab-grid");
      await type("array-nx", 3); await type("array-ny", 2); await type("array-dx", 4); await type("array-dy", 3);
      await type("array-offset-x", 0); await type("array-offset-y", 0); await type("array-stagger", 1);
      await click("array-in-place");
      t(tid("array-preview") && /6 positions/.test(tid("array-count-note")?.textContent ?? ""), `the preview counts the positions: "${tid("array-count-note")?.textContent}"`);
      await click("array-ok");
      await sleep(1500);
      const refs = await placedRefs();
      const added = refs.filter((r) => !baseRefs.includes(r));
      t(added.length === 5, `five new footprints: ${added.join(", ")}`);
      const dups = new Set(refs).size !== refs.length;
      t(!dups, "every reference is unique");
      const at = A.at;
      const wanted = new Set(["4000,0", "8000,0", "0,3000", "4000,3000", "8000,3000"]);
      for (const r of added) { const q = (await partOf(r)).at; wanted.delete(`${q[0] - at[0]},${q[1] - at[1]}`); }
      t(wanted.size === 0, `the copies sit on the grid (missing: ${[...wanted].join(" ")})`);
      t((await partOf(refA)).at.join() === at.join(), "the source stayed in place");
      await undo();
      t((await snapshot()) === base && (await placedRefs()).join() === baseRefs.join(), "ONE undo takes the five copies away");
    });

    // ----- the values are kept for the next time
    await scenario("Create Array: the last values are kept, a bad entry is named, Escape closes", async (t) => {
      await select([refA]);
      await open("pcbnew.Array.createArray");
      await type("array-nx", 2); await type("array-ny", 1); await type("array-dx", 5);
      await click("array-ok");
      await sleep(1500);
      await undo();
      await select([refA]);
      await open("pcbnew.Array.createArray");
      t(tid("array-nx")?.value === "2" && tid("array-dx")?.value === "5", `reopened with ${tid("array-nx")?.value} x ${tid("array-ny")?.value} at ${tid("array-dx")?.value}`);
      await type("array-dx", "wide"); await type("array-nx", "3.5");
      const before = await lastAct();
      await click("array-ok");
      await sleep(200);
      const msg = tid("array-error")?.textContent ?? "";
      t(/Bad numeric value for horizontal count: 3\.5/.test(msg) && /horizontal spacing: wide/.test(msg), `the message names both entries: ${JSON.stringify(msg)}`);
      t(JSON.stringify(await lastAct()) === JSON.stringify(before), "nothing was sent");
      await type("array-nx", 3); await type("array-dx", 0);
      await click("array-ok"); await sleep(200);
      t(/horizontal delta of zero with 3 objects/.test(tid("array-error")?.textContent ?? ""), `zero spacing: ${tid("array-error")?.textContent}`);
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(200);
      t(!tid("create-array"), "Escape closed the dialog");
    });

    // ----- Create Array: a circle of one footprint about a centre
    await scenario("Create Array: four footprints on a circle (Full circle), clockwise, one undo", async (t) => {
      const c = U.at;
      const d0 = D.at;
      await select([refD]);
      await open("pcbnew.Array.createArray");
      await click("array-tab-circular");
      await type("array-center-x", c[0] / 1000); await type("array-center-y", c[1] / 1000);
      await type("array-count", 4);
      if (!tid("array-full-circle").checked) await click("array-full-circle");
      if (!tid("array-clockwise").checked) await click("array-clockwise");
      await click("array-ok");
      await sleep(1500);
      const added = (await placedRefs()).filter((r) => !baseRefs.includes(r));
      t(added.length === 3, `three new footprints: ${added.join(", ")}`);
      const rx = d0[0] - c[0];
      const ry = d0[1] - c[1];
      const want = [[-ry, rx], [-rx, -ry], [ry, -rx]].map(([x, y]) => `${c[0] + x},${c[1] + y}`);
      const got = [];
      for (const r of added) got.push((await partOf(r)).at.join());
      t(want.every((w) => got.some((g) => { const [gx, gy] = g.split(",").map(Number); const [wx, wy] = w.split(",").map(Number); return near(gx, wx, 50) && near(gy, wy, 50); })), `the copies are a quarter turn apart about the centre: ${got.join(" ")} (wanted ${want.join(" ")}, to the grid)`);
      await undo();
      t((await snapshot()) === base, "ONE undo takes them away");
    });

    // ----- Create Array: arrange the selection
    await scenario("Create Array: Arrange selection puts the selected footprints on the array's points", async (t) => {
      await select([refA, refD]);
      await open("pcbnew.Array.createArray");
      await click("array-tab-grid");
      await type("array-nx", 2); await type("array-ny", 1); await type("array-dx", 10);
      await click("array-in-place");
      await click("array-arrange");
      t(!tid("array-keep-refs"), "Footprint Annotation steps aside for Arrange");
      await click("array-ok");
      await sleep(1500);
      const a = (await partOf(refA)).at;
      const d = (await partOf(refD)).at;
      t(d[0] - a[0] === 10000 && d[1] === a[1], `the second sits 10 mm to the right of the first: ${a} and ${d}`);
      t((await placedRefs()).join() === baseRefs.join(), "no footprint was made");
      await undo();
      t((await snapshot()) === base, "undo puts them back");
    });

    // ----- the kicad-cli position file honours the attributes (a quick look; the Rust slow test does the byte-for-byte check)
    await scenario("the board is as it was", async (t) => {
      t((await snapshot()) === base, "nothing the check did remains");
      t((await placedRefs()).join() === baseRefs.join(), "the same footprints are placed");
    });

    if (strict && strictWas && !strict.checked) strict.click();
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "running: read it with `await __fpEditCheck.wait(40000)`";
})()
