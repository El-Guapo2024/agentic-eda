// UI-LEFT S8 (PARITY-sch §60): the Hierarchy Navigator's right-click menu (`HIERARCHY_PANE::onRightClick`) -- Rename in place (`onTreeEditFinished`), Edit Page
// Number, Delete Top-Level Sheet's question and "Cannot delete the last top-level sheet.", New Top-Level Sheet offered disabled on a design with one root --
// and its net highlight (`UpdateNetHighlight`); Sheet Properties' Do not populate, fill colour and Add Field (`DIALOG_SHEET_PROPERTIES`), drawn.
//
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then `eda board serve -C <dir>`),
// with the schematic tab open, on a COPY of a board: every scenario undoes what it did (and the run undoes what one left behind).
//
// It answers through `window.__schHierarchyMenuCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30
(async () => {
  const results = [];
  const check = (window.__schHierarchyMenuCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schHierarchyMenuCheckSched) {
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
    window.__schHierarchyMenuCheckSched = true;
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
  const hierarchy = () => json("/api/sch/hierarchy");
  /** What the scenarios touch: the hierarchy and the root's sheets (their props and fields). */
  const snapshot = async () => {
    const h = await hierarchy();
    const root = await sch("");
    return JSON.stringify({ h: h.sheets.map((s) => [s.path, s.name, s.page]), sheets: root.sheets.map((s) => [s.id, s.name, s.props, s.user_fields]) });
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
      try { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await closeDialogs(); await __eda.run("common.InteractiveSelection.clear"); if (__eda.state().tool !== "select") await escape(); await leave(); } catch { /* ignore */ }
      const left = await restore(baseline);
      if (left < 0) { entry.notes.push("FAIL: the schematic could not be put back"); entry.ok = false; } else if (left > 0) { entry.notes.push(`note: ${left} edit(s) the scenario left behind were undone`); }
    } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const row = (path) => document.querySelector(`[data-testid="hierarchy-row"][data-path="${path.join("/")}"]`);
  /** Right-click the navigator's row of `path` and pick `label` from its menu (`HIERARCHY_PANE::onRightClick`); the entry, for a look at it, when `pick` is off. */
  const rowMenu = async (path, label, pick = true) => {
    const r = await until(() => row(path));
    if (!r) return null;
    const b = r.getBoundingClientRect();
    r.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: b.left + 10, clientY: b.top + 5 }));
    await sleep(400);
    const entry = [...document.querySelectorAll('[role="menuitem"]')].find((e) => e.textContent.trim() === label);
    if (entry && pick) {
      entry.click();
      await sleep(600);
    }
    return entry ?? null;
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  void (async () => {
    const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
    if (strict?.checked) strict.click();
    await sleep(300);
    if (__eda.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(900); }
    await leave();
    await fit();
    const root0 = await sch("");
    const sheet = root0.sheets[0];

    await scenario("The navigator renames a sheet in place (Rename, onTreeEditFinished) and undo puts the name back", async (t) => {
      t(!!sheet, `the root places sheets (${root0.sheets.map((s) => s.name)})`);
      if (!sheet) return;
      await rowMenu([sheet.id], "Rename");
      const input = await until(() => aria("Sheet name"));
      t(!!input && input.value === sheet.name, "Rename puts the row's name in an edit box");
      if (!input) return;
      setValue(input, "E2E Renamed");
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await sleep(1500);
      const h = await hierarchy();
      t(h.sheets.some((s) => s.path.join("/") === sheet.id && s.name === "E2E Renamed"), `Enter renames the sheet (${h.sheets.map((s) => s.name)})`);
      t((await sch("")).sheets.find((s) => s.id === sheet.id)?.name === "E2E Renamed", "its Sheetname is the new name");
      await undo();
      t((await sch("")).sheets.find((s) => s.id === sheet.id)?.name === sheet.name, "one undo gives the old name back");
    });

    await scenario("The navigator's Edit Page Number sets the page of that sheet path (Edit Sheet Page Number)", async (t) => {
      if (!sheet) return t(false, "a sheet");
      await rowMenu([sheet.id], "Edit Page Number");
      const dlg = await until(() => dialogTitled(/^Edit Sheet Page Number/));
      t(!!dlg, "it opens Edit Sheet Page Number on the sheet");
      if (!dlg) return;
      setValue(dlg.querySelector("input"), "42");
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const h = await hierarchy();
      t(h.sheets.find((s) => s.path.join("/") === sheet.id)?.page === "42", `the sheet is page 42 (${h.sheets.map((s) => s.page)})`);
      t(/page 42/.test(row([sheet.id])?.textContent ?? ""), "its row says so");
      await undo();
    });

    await scenario("The navigator's menu: Delete Top-Level Sheet asks first and refuses the last one; New Top-Level Sheet waits for a project of several", async (t) => {
      const neu = await rowMenu([], "New Top-Level Sheet", false);
      t(!!neu && neu.getAttribute("aria-disabled") === "true", "New Top-Level Sheet is offered disabled on a design whose root is its only top-level sheet");
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      await sleep(300);
      await rowMenu([], "Delete Top-Level Sheet");
      const ask = await until(() => dialogTitled(/^Delete Top-Level Sheet/));
      t(!!ask && /This cannot be undone\./.test(ask.innerText), `it asks "Delete top-level sheet '..'?" (${ask?.innerText.slice(0, 80)})`);
      if (ask) buttonText(ask, "Yes").click();
      await sleep(500);
      t(/Cannot delete the last top-level sheet\./.test(document.querySelector('[data-testid="hierarchy-message"]')?.textContent ?? ""), "then refuses the last top-level sheet");
      await closeDialogs();
    });

    await scenario("Sheet Properties keeps a sheet's DNP, fill colour and a new field; the sheet is drawn with its fill and the DNP cross", async (t) => {
      if (!sheet) return t(false, "a sheet");
      await frame([sheet.id]);
      await select([sheet.id]);
      await __eda.run("eeschema.InteractiveEdit.properties");
      const dlg = await until(() => dialogTitled(/^Sheet Properties/));
      t(!!dlg, "E on a sheet opens Sheet Properties");
      if (!dlg) return;
      [...dlg.querySelectorAll("label")].find((l) => l.textContent.trim() === "Do not populate")?.querySelector("input")?.click();
      aria("Background color default", dlg)?.click();
      await sleep(150);
      setValue(aria("Background color", dlg), "#00ff00");
      buttonText(dlg, "+ Add Field")?.click();
      await sleep(200);
      const nameBox = aria("Sheet field 1 name", dlg);
      t(nameBox?.value === "Field2", `Add Field names it Field2 (GetUserFieldName( 2 )) (${nameBox?.value})`);
      setValue(aria("Sheet field 1 value", dlg), "E2E rev");
      await sleep(200);
      buttonText(dlg, "OK").click();
      await sleep(1500);
      const s = (await sch("")).sheets.find((x) => x.id === sheet.id);
      t(s?.props?.dnp === true && s?.props?.fill_color?.g === 255 && s?.props?.fill_color?.r === 0, `the sheet keeps DNP and the fill (${JSON.stringify(s?.props)})`);
      t(same(s?.user_fields?.map((f) => [f.name, f.text]), [["Field2", "E2E rev"]]), `and the new field (${JSON.stringify(s?.user_fields)})`);
      await select([]);
      await sleep(800);
      const cx = s.at[0] + s.size[0] / 4, cy = s.at[1] + s.size[1] / 2;
      t(countAt(cx, cy, 30, [0, 255, 0]) > 50, `the sheet is filled green (${countAt(cx, cy, 30, [0, 255, 0])} px)`);
      t(countAt(s.at[0] + s.size[0] / 2, s.at[1] + s.size[1] / 2, 12, [0xdc, 0x09, 0x0d], 50) > 5, "and crossed out in the DNP marker's colour");
      await undo();
      t(!(await sch("")).sheets.find((x) => x.id === sheet.id)?.props, "one undo takes it all back");
    });

    await scenario("Highlight Net lights the navigator's rows of the sheets that hold the net (UpdateNetHighlight)", async (t) => {
      if (!sheet) return t(false, "a sheet");
      await enterSheet(sheet.id);
      const inside = await sch(sheet.id);
      const wire = inside.wires.find((w) => w.net && !w.bus && w.pts.length >= 2);
      t(!!wire, "the sheet has a wire on a net");
      if (!wire) return;
      const xy = (p) => (Array.isArray(p) ? p : [p.x, p.y]);
      const [a, b] = [xy(wire.pts[0]), xy(wire.pts[1])];
      await frame([wire.id]);
      await __eda.run("eeschema.EditorControl.highlightNetTool");
      await sleep(300);
      await clickAt((a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
      await sleep(1500);
      t((__eda.state().highlight ?? []).length > 0, `the net is highlighted (${JSON.stringify(__eda.state().highlight)})`);
      const lit = await until(() => row([sheet.id])?.querySelector('[data-net="yes"]'));
      t(!!lit, "the sheet's row is written in the highlight colour");
      await escape();
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
