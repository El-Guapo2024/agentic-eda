// UI-LEFT S6 (PARITY-sch §60): Text Properties' Font, Bold, Color and Link (`DIALOG_TEXT_PROPERTIES`: `ValidateHyperlink`'s message on a bad link), the text drawn
// in its own colour (`SCH_PAINTER::getRenderColor`); a label's field as an item of its own -- dragged by hand it stays where it was put through a label edit
// (`SCH_MOVE_TOOL::moveItem`: `SetFieldsAutoplaced( AUTOPLACE_NONE )`), Field Properties opens on it with Font and Color, Autoplace Fields puts it back.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with the schematic tab open, on a COPY of a board: every scenario undoes what it did (and the run undoes what one left behind).
//
// It answers through `window.__schTextLooksCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schTextLooksCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schTextLooksCheckSched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
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
    window.__schTextLooksCheckSched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (cmd, sheet) => fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: sheet ? { op: "on_sheet", sheet, cmd } : cmd, strict: false }) }).then((r) => r.json());
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); if (ids.length > 0) await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await __eda.run("common.Interactive.cancel"); await sleep(400); };
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const aria = (label, root = document) => root.querySelector(`[aria-label="${label}"]`);
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const dialogTitled = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const closeDialogs = async () => {
    for (let i = 0; i < 5; i++) {
      const b = [...document.querySelectorAll(".dialog-footer button")].reverse().find((x) => ["Cancel", "Close", "No", "OK", "Discard Changes"].includes(x.textContent.trim()));
      if (!b) break;
      b.click();
      await sleep(300);
    }
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = __eda.state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(400); fire("pointerdown", px, py, { buttons: 1 }); await sleep(80); fire("pointerup", px, py); await sleep(1200); };
  const dragFrom = async (x, y, dxNm, dyNm) => {
    const [px, py] = screenOf(x, y);
    const [qx, qy] = screenOf(x + dxNm, y + dyNm);
    fire("pointermove", px, py); await sleep(400);
    fire("pointerdown", px, py, { buttons: 1 }); await sleep(120);
    for (let k = 1; k <= 6; k++) { fire("pointermove", px + ((qx - px) * k) / 6, py + ((qy - py) * k) / 6, { buttons: 1 }); await sleep(120); }
    fire("pointerup", qx, qy); await sleep(1600);
  };
  const fit = async () => { await __eda.run("common.Control.zoomFitScreen"); await sleep(500); };
  /** Frame the items `ids` (Zoom to Selection), then leave nothing selected (the selection colour would hide the items' own). */
  const frame = async (ids) => { await select(ids); await __eda.run("common.Control.zoomFitSelection"); await sleep(700); await select([]); await sleep(400); };
  const leave = async () => { while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); } };
  const enterSheet = async (id) => { await leave(); await select([id]); await __eda.run("eeschema.NavigateTool.enterSheet"); await sleep(1100); await select([]); await fit(); };
  const canvas = () => [...document.querySelectorAll(".pcb-canvas-container canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** How many canvas pixels within `tol` of `rgb` lie in a square of `half` canvas pixels about the sheet's point (x, y) (nm). */
  const countAt = (x, y, half, rgb, tol = 40) => {
    const c = canvas();
    if (!c) return -1;
    const r = c.getBoundingClientRect();
    const v = __eda.state().view;
    const k = c.width / r.width;
    const px = Math.round((v.x + x * v.scale) * k), py = Math.round((v.y + y * v.scale) * k);
    const d = c.getContext("2d").getImageData(px - half, py - half, 2 * half + 1, 2 * half + 1).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - rgb[0]) <= tol && Math.abs(d[i + 1] - rgb[1]) <= tol && Math.abs(d[i + 2] - rgb[2]) <= tol) n++;
    return n;
  };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  /** What the scenarios touch: the root's texts, labels and graphics. */
  const snapshot = async () => {
    const root = await sch("");
    return JSON.stringify({ texts: root.texts.map((t) => [t.id, t.content, t.font, t.color, t.bold]), labels: root.labels.map((l) => [l.id, l.net, l.at, l.fields, l.fields_hand_placed]) });
  };
  const restore = async (baseline) => {
    for (let i = 0; i < 16; i++) {
      if ((await snapshot()) === baseline) return i;
      await undo();
    }
    return -1;
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try {
      const baseline = await snapshot();
      try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
      try { await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();

    await scenario("Text Properties: a text's font, bold and colour are kept and drawn in that colour (DIALOG_TEXT_PROPERTIES, SCH_PAINTER::getRenderColor)", async (t) => {
      const placed = await post({ op: "sch_edit", verb: "place_text", content: "E2E LOOK TEXT", at: { x: 30_480_000, y: 20_320_000 }, size_nm: 2_540_000 });
      t(placed.ok !== false, `place a text (${JSON.stringify(placed).slice(0, 120)})`);
      await sleep(900);
      const text = (await sch("")).texts.find((x) => x.content === "E2E LOOK TEXT");
      t(!!text, "the text is on the sheet");
      if (!text) return;
      await frame([text.id]);
      const before = countAt(text.at[0], text.at[1], 80, [255, 0, 0]);
      await select([text.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      const dlg = await until(() => dialogTitled(/^Text Properties/));
      t(!!dlg, "E opens Text Properties");
      if (!dlg) return;
      const font = aria("Font", dlg);
      t(!!font && [...font.options].some((o) => o.value === "KiCad Font"), "the Font choice lists KiCad's fonts");
      setValue(font, "KiCad Font");
      aria("Bold", dlg).click();
      aria("Text color default", dlg).click();
      await sleep(200);
      setValue(aria("Text color", dlg), "#ff0000");
      await sleep(200);
      // a link `ValidateHyperlink` refuses keeps the dialog open with KiCad's message
      setValue(aria("Hyperlink", dlg), "not a link");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(500);
      t(/Invalid hyperlink destination/.test(dlg.innerText), "an invalid link is refused with ValidateHyperlink's message");
      setValue(aria("Hyperlink", dlg), "#2");
      await sleep(150);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const now = (await sch("")).texts.find((x) => x.id === text.id);
      t(now?.font === "KiCad Font" && now?.bold === true && now?.color?.r === 255 && now?.color?.g === 0 && now?.href === "#2", `the text keeps the font, bold, colour and link (${JSON.stringify([now?.font, now?.bold, now?.color, now?.href])})`);
      await select([]);
      await sleep(800);
      const red = countAt(text.at[0], text.at[1], 80, [255, 0, 0]);
      t(red > before + 20, `the text is drawn in its colour (${before} -> ${red} red pixels about its anchor)`);
      await undo();
      const back = (await sch("")).texts.find((x) => x.id === text.id);
      t(!back?.font && !back?.bold && !back?.color, "one undo takes the looks back");
    });

    await scenario("A label's field moved by hand stays where it was put, through a label edit; Autoplace Fields puts it back (AUTOPLACE_NONE)", async (t) => {
      const at = { x: 50_800_000, y: 25_400_000 };
      const placed = await post({ op: "sch_edit", verb: "place_label", net: "E2E_FLD", at, kind: { scope: "global", shape: "input" }, spin: "right" });
      t(placed.ok !== false, "place a global label");
      await sleep(800);
      let label = (await sch("")).labels.find((l) => l.net === "E2E_FLD");
      if (!label) return t(false, "the label is on the sheet");
      await post({ op: "sch_edit", verb: "edit_label", id: label.id, fields: [{ name: "Netclass", text: "E2E_CLASS", visible: true, italic: false }] });
      await sleep(900);
      label = (await sch("")).labels.find((l) => l.id === label.id);
      const field = label.fields?.find((f) => f.name === "Netclass");
      t(!!field && field.id === `fld:${label.id}:Netclass`, `the net class field is an item (${field?.id})`);
      if (!field) return;
      await frame([label.id]);
      await select([field.id]); // Drag selected objects, KiCad's default (UI-LEFT-2 CC3)
      await dragFrom(field.at[0], field.at[1], 5_080_000, 2_540_000);
      label = (await sch("")).labels.find((l) => l.id === label.id);
      const moved = label.fields.find((f) => f.name === "Netclass");
      t(label.fields_hand_placed === true && (moved.at[0] !== field.at[0] || moved.at[1] !== field.at[1]), `a drag moves it and the label's fields are no longer autoplaced (${field.at} -> ${moved.at})`);
      await post({ op: "sch_edit", verb: "edit_label", id: label.id, text: "E2E_FLD_LONGER_NAME" });
      await sleep(900);
      label = (await sch("")).labels.find((l) => l.id === label.id);
      const kept = label.fields.find((f) => f.name === "Netclass");
      t(same(kept.at, moved.at), `a new text leaves it where it was put (${kept.at})`);
      // Field Properties on it (a double-click, E)
      await select([kept.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      const dlg = await until(() => dialogTitled(/Field/));
      t(!!dlg && !!aria("Field color", dlg), "E on the field opens Field Properties with its Font and Color");
      await closeDialogs();
      await select([kept.id]);
      await __eda.run("eeschema.InteractiveEdit.autoplaceFields");
      await sleep(1200);
      label = (await sch("")).labels.find((l) => l.id === label.id);
      t(label.fields_hand_placed === false, "Autoplace Fields puts the label's fields back under it");
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
