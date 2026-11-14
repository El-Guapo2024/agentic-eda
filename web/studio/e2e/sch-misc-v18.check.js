// sch-misc lane v18 (UI-LEFT-3 S3-11 .. S3-14, S3-16, S3-22, S3-25; task-v18-schmisc):
//   power kinds     a power symbol placed Local where the sheet has the global one is drawn from a renamed cache entry; copied, the clipboard writes the entry
//                   and its `(lib_name ..)` (with "local label" in its description: `PlaceSymbol`'s rewrite); pasted, it is the same library id again, no
//                   library symbol of its own (`ChoosePasteLibSymbol`, `SCH_SCREEN::Append`); one undo each
//   unfold mirror   X while the unfolded member's label rides the cursor mirrors it (`SCH_EDIT_TOOL::Mirror` on the riding label): placed facing left
//   sync esc        Sync Sheet Pins' Add labels with two pins: the first Esc drops only the riding label, the next click makes it again, the second label rides
//                   after it is placed, and the dialog comes back once both are placed
//   variant chooser Set Variant Symbol's chooser draws a symbol that is not pin-compatible in orange and lists KiCad's "Compatibility Warnings:"
//   fp visibility   Import Footprint Assignments asks KiCad's "Change Visibility" question; "Show all footprint fields" shows the Footprint field of the symbols
//                   the file names; Cancel does nothing
//   rescue on open  the board's "rescue on open" mark (a KiCad 5 project `eda import-kicad` brought in) was spent by the page as it opened
//   compare         Compare Symbol with Library on a resistor of a sheet this project drew finds its drawing and pins the library's: compared in the library's
//                   frame (`corner_offset_mm`), the line styles, pin text sizes and fonts now sent compare equal
// It answers through `window.__schMiscV18Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=12 fixtures=rescue-on-open
(async () => {
  const results = [];
  const check = (window.__schMiscV18Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  if (document.visibilityState === "hidden" && !window.__schMiscV18Sched) {
    // One scheduler for every timer and animation frame, driven by a fetch loop (a hidden pane stops both).
    const timers = new Map();
    let next = 9e6;
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
    window.__schMiscV18Sched = true;
  }

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const state = () => E.state();
  const sch = (sheet) => json(`/api/schematic${sheet ? `?sheet=${encodeURIComponent(sheet)}` : ""}`);
  const post = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c, sheet) => post("/api/cmd", { cmd: sheet ? { op: "on_sheet", sheet, cmd: c } : c, strict: false });
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(120); } };
  const run = async (id, args) => { const r = await E.run(id, args); await sleep(300); return r; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(350); };
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || state().carry); i++) await escape(); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const dialogs = () => [...document.querySelectorAll(".dialog")];
  const buttonText = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const titled = (title) => dialogs().find((d) => title.test(d.querySelector(".dialog-header")?.textContent ?? d.textContent ?? "")) ?? null;
  const opened = (title, ms = 8000) => until(() => titled(title), ms);
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const d = dialogs().pop(); const b = d && (buttonText(d, "Cancel") ?? buttonText(d, "Close")); if (!b) break; b.click(); await sleep(250); } };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const container = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y, extra = {}) => {
    const rect = container().getBoundingClientRect();
    (container().querySelector("canvas") || container()).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0, ...extra }));
  };
  const screenOf = (x, y) => { const v = state().view; return [v.x + x * v.scale, v.y + y * v.scale]; };
  const hoverAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); };
  const clickAt = async (x, y) => { const [px, py] = screenOf(x, y); fire("pointermove", px, py); await sleep(250); fire("pointerdown", px, py, { buttons: 1 }); await sleep(60); fire("pointerup", px, py); await sleep(900); };
  const dblClickAt = async (x, y) => {
    const [px, py] = screenOf(x, y);
    const rect = container().getBoundingClientRect();
    fire("pointermove", px, py); await sleep(200);
    fire("pointerdown", px, py, { buttons: 1 }); await sleep(40); fire("pointerup", px, py); await sleep(40);
    (container().querySelector("canvas") || container()).dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true, clientX: rect.left + px, clientY: rect.top + py, button: 0, detail: 2 }));
    await sleep(1200);
  };
  const leave = async () => { while (state().sheetPath.length > 0) { await run("eeschema.NavigateTool.leaveSheet"); await sleep(400); } };
  const scenario = async (name, fn) => {
    const notes = [];
    let ok = true;
    const t = (cond, what) => { notes.push(`${cond ? "ok" : "FAIL"}: ${what}`); if (!cond) ok = false; return cond; };
    try { await fn(t); } catch (e) { ok = false; notes.push(`threw: ${e && e.stack ? e.stack : e}`); }
    try { await closeDialogs(); await toSelect(); await leave(); } catch { /* ignore */ }
    results.push({ name, ok, notes });
  };
  const mm = (v) => Math.round(v * 1e6);
  const X = mm(25.4), Y = mm(160.02);
  /** A resistor (`Device:R`) on the root or a sheet below it, with that sheet entered: the symbol and the sheet's path for `/api/schematic?sheet=`. */
  const enterResistor = async (want = () => true) => {
    await leave();
    const root = await sch();
    let sym = root.symbols.find((x) => x.lib_id === "Device:R" && want(x));
    if (sym) return { sym, path: "" };
    for (const sheet of root.sheets ?? []) {
      await select([sheet.id]);
      await run("eeschema.NavigateTool.enterSheet");
      await sleep(1200);
      const path = state().sheetPath.join("/");
      const inner = await sch(path);
      sym = inner.symbols.find((x) => x.lib_id === "Device:R" && want(x));
      if (sym) return { sym, path };
      await leave();
    }
    return { sym: null, path: "" };
  };

  // the Schematic tab, Strict mode off (the scenarios make what a strict board refuses on the way)
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  if (state().tab !== "schematic") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click();
    await until(() => state().tab === "schematic");
    await sleep(1500);
  }
  await E.run("common.Control.zoomFitScreen");
  await sleep(500);

  // ------------------------------------------------------------------------------------------------------------------------ rescue on open
  await scenario("the rescue-on-open mark is spent by the page as it opens (OpenProjectFiles' RescueSymbolLibTableProject( false ))", async (t) => {
    const again = await post("/api/sch/rescue/on_open", {});
    t(again.ok === true && again.pending === false, `the page asked already: the mark is gone (${JSON.stringify(again)})`);
  });

  // ------------------------------------------------------------------------------------------------------------------------ power kinds
  await scenario("a local power symbol where the sheet has the global one: a renamed entry, copied with its lib_name, pasted as the same library id", async (t) => {
    const start = await sch();
    let made = 0;
    try {
      const add = (x) => ({ op: "add_power_symbol", lib_id: "power:+5V", at: { x, y: Y }, rot_millideg: 0, net: "+5V", pin: "" });
      const g = await cmd(add(X));
      if (g.ok !== false) made++;
      const l = await cmd({ op: "batch", cmds: [add(X + mm(10.16)), { op: "sch_edit", verb: "set_new_power_kind", lib_id: "power:+5V", local: true }] });
      if (l.ok !== false) made++;
      if (!t(g.ok !== false && l.ok !== false, "a global +5V and a local one placed")) return;
      const now = await sch();
      const fresh = now.power_symbols.filter((p) => !start.power_symbols.some((q) => q.id === p.id));
      t(fresh.length === 2 && fresh.every((p) => p.lib_id === "power:+5V"), `both of power:+5V (${fresh.map((p) => p.lib_id).join(", ")})`);
      const copied = await post("/api/sch/clipboard/copy", { ids: fresh.map((p) => p.id) });
      const text = copied.text ?? "";
      t(/\(lib_name "\+5V_1"\)/.test(text), "the clipboard names the renamed entry (`(lib_name \"+5V_1\")`)");
      const entry = text.split('(symbol "+5V_1"')[1]?.split('(symbol "')[0] ?? "";
      t(/\(power local\)/.test(entry), "its entry is a local power symbol");
      t(!/global label/.test(entry), "and its description does not say \"global label\"");
      const parsed = await post("/api/sch/clipboard/parse", { text });
      if (!t(parsed.ok === true, "the clipboard parses")) return;
      const pasted = await cmd({ op: "paste_sch", fragment: parsed.fragment, dx: mm(25.4), dy: 0 });
      if (pasted.ok !== false) made++;
      const after = await sch();
      const more = after.power_symbols.filter((p) => !now.power_symbols.some((q) => q.id === p.id));
      t(more.length === 2 && more.every((p) => p.lib_id === "power:+5V"), `pasted as power:+5V both (${more.map((p) => p.lib_id).join(", ")})`);
      t(!Object.keys(after.lib_symbols ?? {}).some((k) => /^power:\+5V_\d/.test(k)), "no library symbol of its own (power:+5V_2 ...)");
      const recopied = await post("/api/sch/clipboard/copy", { ids: more.map((p) => p.id) });
      t(/\(lib_name "\+5V_1"\)/.test(recopied.text ?? ""), "the pasted local one is drawn from the sheet's +5V_1 again");
    } finally {
      for (let i = 0; i < made; i++) await undo();
    }
    const back = await sch();
    t(back.power_symbols.length === start.power_symbols.length, "undone, one step each");
  });

  // ------------------------------------------------------------------------------------------------------------------------ unfold mirror
  await scenario("X while the unfolded member's label rides mirrors it (SCH_EDIT_TOOL::Mirror on the riding label)", async (t) => {
    const start = await sch();
    let made = 0;
    try {
      const w = await cmd({ op: "add_wire", pts: [{ x: X, y: Y + mm(20.32) }, { x: X + mm(25.4), y: Y + mm(20.32) }], bus: true });
      if (w.ok !== false) made++;
      const l = await cmd({ op: "add_label", net: "E2EM[0..3]", at: { x: X, y: Y + mm(20.32) }, kind: { scope: "local" } });
      if (l.ok !== false) made++;
      if (!t(w.ok !== false && l.ok !== false, "a bus labelled E2EM[0..3]")) return;
      await until(async () => (await sch()).wires.some((v) => v.bus && (v.member_tree?.length ?? 0) > 0), 5000);
      await sleep(600);
      await hoverAt(X + mm(12.7), Y + mm(20.32));
      await run("eeschema.InteractiveDrawingLineWireBus.unfoldBus");
      const dlg = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Unfold from Bus/.test(d.textContent ?? "")), 4000);
      if (!t(!!dlg, "the Unfold from Bus menu opens")) return;
      const member = dlg.querySelector('button[data-unfold-member="E2EM1"]');
      if (!t(!!member, "E2EM1 is offered")) return;
      member.click();
      await sleep(400);
      t(state().tool === "wire", `the wire tool unfolds (${state().tool})`);
      await run("eeschema.InteractiveEdit.mirrorH");
      const before = await sch();
      await clickAt(X + mm(12.7) + mm(5.08), Y + mm(27.94));
      await dblClickAt(X + mm(12.7) + mm(5.08), Y + mm(38.1));
      const after = await until(async () => { const s = await sch(); return s.labels.length > before.labels.length ? s : null; }, 6000);
      const label = after?.labels.find((x) => x.net === "E2EM1");
      if (label) made++;
      t(label?.spin === "left", `the label is placed facing left: X mirrored it while it rode (${label?.spin})`);
      await toSelect();
    } finally {
      for (let i = 0; i < made; i++) await undo();
    }
    const back = await sch();
    t(back.wires.length === start.wires.length && back.labels.length === start.labels.length, "undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ sync esc
  await scenario("Sync Sheet Pins' Add labels: the first Esc drops only the riding label; the next click makes it again; the dialog returns once both are placed", async (t) => {
    const root = await sch();
    const s = root.sheets[0];
    if (!t(!!s, "a sheet on the root")) return;
    const G = 1_270_000;
    let made = 0;
    try {
      for (const [i, name] of ["E2E_ESC_A", "E2E_ESC_B"].entries()) {
        const r = await cmd({ op: "sch_edit", verb: "add_sheet_pin", sheet: s.id, name, shape: "output", at: { x: s.at[0], y: s.at[1] + (3 + 2 * i) * G } });
        if (r.ok !== false) made++;
      }
      t(made === 2, "two pins with no label inside");
      await sleep(800);
      await select([s.id]);
      await run("eeschema.InteractiveDrawing.syncSheetPins");
      const dlg = await opened(/Synchronize Sheet Pins/);
      if (!t(!!dlg, "the dialog opens")) return;
      const addAll = [...dlg.querySelectorAll("button")].find((b) => /^Add labels \(\d+\)$/.test(b.textContent.trim()));
      if (!t(!!addAll, "the dialog offers Add labels (n)")) return;
      addAll.click();
      const riding = await until(() => (state().sheetPath.length === 1 && state().carry ? state().carry : null), 8000);
      if (!t(!!riding, "the sheet is entered with a label in the hand")) return;
      t(riding.queue >= 1, `another one waits behind it (${riding.queue})`);
      await sleep(500);
      await escape();
      t(!state().carry, "Esc drops the riding label");
      t(state().tool === "sch_label_hier", `the tool stays (${state().tool})`);
      t(!titled(/Synchronize Sheet Pins/), "the dialog does not come back yet");
      // the next click makes the dropped label again, with the other still behind it
      await clickAt(10 * G, 10 * G);
      const again = await until(() => state().carry, 4000);
      t(!!again && again.queue >= 1, `the click makes the label again, the other still waiting (${again?.queue})`);
      await clickAt(10 * G, 10 * G);
      const second = await until(() => state().carry, 4000);
      t(!!second && second.queue === 0, "placed, the second label rides");
      await clickAt(10 * G, 16 * G);
      const back = await opened(/Synchronize Sheet Pins/);
      t(!!back, "both placed: the dialog is shown again");
      const sub = await sch(s.id);
      const n = sub.labels.filter((l) => l.net === "E2E_ESC_A" || l.net === "E2E_ESC_B").length;
      t(n === 2, `two hierarchical labels inside (${n})`);
      if (n === 2) made += 2;
    } finally {
      await closeDialogs();
      await toSelect();
      await leave();
      for (let i = 0; i < made; i++) await undo();
    }
    const after = await sch();
    t(JSON.stringify(after.sheets[0]?.pins ?? []) === JSON.stringify(s.pins ?? []), "undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ variant chooser
  await scenario("Set Variant Symbol's chooser: a symbol that is not pin-compatible is orange, with KiCad's Compatibility Warnings", async (t) => {
    const added = await cmd({ op: "add_design_variant", name: "V18", description: "" });
    const { sym } = await enterResistor();
    if (!t(!!sym, "a Device:R on a sheet")) { if (added.ok !== false) await post("/api/undo", { domain: "schematic" }); return; }
    try {
      t(added.ok !== false, "a variant V18");
      const sel = await until(() => { const s = document.querySelector("[data-testid=sch-variant-select]"); return s && [...s.options].some((o) => o.textContent === "V18") ? s : null; });
      if (sel) setValue(sel, [...sel.options].find((o) => o.textContent === "V18").value);
      await sleep(1200);
      await select([sym.id]);
      await run("eeschema.InteractiveEdit.setVariantSymbol");
      const chooser = await until(() => document.querySelector("[data-symbol-picker]"), 6000);
      if (!t(!!chooser, "the chooser opens")) return;
      const search = chooser.querySelector("[data-chooser-search]");
      setValue(search, "LED");
      const led = await until(() => chooser.querySelector('[data-id="Device:LED"]'), 15000);
      if (!t(!!led, "Device:LED is listed")) return;
      const marked = await until(() => led.querySelector("[data-incompatible]"), 8000);
      t(!!marked && /200, 120, 0/.test(marked.style.color), `its name is orange (${marked?.style.color})`);
      led.click();
      const warn = await until(() => chooser.querySelector("[data-compat-warnings]"), 8000);
      t(!!warn && /Compatibility Warnings:/.test(warn.textContent) && /Unit 1, body style 1: pin '/.test(warn.textContent), `the details list the issues (${warn?.textContent?.slice(0, 120)})`);
      setValue(search, "Device:R");
      const r = await until(() => chooser.querySelector('[data-id="Device:R"]'), 15000);
      await sleep(1500);
      t(!!r && !r.querySelector("[data-incompatible]"), "Device:R is not marked");
      buttonText(chooser, "Cancel")?.click();
      await sleep(400);
    } finally {
      const sel = document.querySelector("[data-testid=sch-variant-select]");
      if (sel) setValue(sel, sel.options[0].value);
      await sleep(600);
      if (added.ok !== false) await post("/api/undo", { domain: "schematic" });
      await sleep(600);
    }
  });

  // ------------------------------------------------------------------------------------------------------------------------ fp visibility
  await scenario("Import Footprint Assignments asks Change Visibility; Show all shows the named symbols' Footprint field; Cancel does nothing", async (t) => {
    const { sym, path } = await enterResistor((s) => s.fields?.some((f) => f.name === "Footprint" && !f.visible));
    if (!t(!!sym, "a resistor whose Footprint field is hidden")) return;
    const fp = sym.footprint || "Resistor_SMD:R_0603_1608Metric";
    const cmp = `Cmp-Mod V01 Created by test\n\nBeginCmp\nTimeStamp = /x;\nReference = ${sym.id};\nValueEeschema = 1k;\nIdModule = ${fp};\nEndCmp\n\nEndListe\n`;
    // the file chooser (`pickTextFile`) gets the file as a person would pick it
    const realClick = HTMLInputElement.prototype.click;
    HTMLInputElement.prototype.click = function () {
      if (this.type !== "file") return realClick.call(this);
      const dt = new DataTransfer();
      dt.items.add(new File([cmp], "e2e.cmp", { type: "text/plain" }));
      this.files = dt.files;
      this.dispatchEvent(new Event("change"));
    };
    let changed = false;
    try {
      // Cancel: nothing
      void E.run("eeschema.EditorControl.importFPAssignments");
      let q = await opened(/Change Visibility/);
      if (!t(!!q, "the question opens")) return;
      t(/Select the footprint field visibility setting\./.test(q.textContent), "with KiCad's words");
      const options = [...q.querySelectorAll("option")].map((o) => o.textContent);
      t(JSON.stringify(options) === JSON.stringify(["Keep existing footprint field visibility", "Show all footprint fields", "Hide all footprint fields"]), `KiCad's three choices (${options.join(" | ")})`);
      buttonText(q, "Cancel").click();
      await sleep(1200);
      const same = (await sch(path)).symbols.find((s) => s.id === sym.id);
      t(same.fields.find((f) => f.name === "Footprint").visible === false, "Cancel leaves the field hidden");
      // Show all
      void E.run("eeschema.EditorControl.importFPAssignments");
      q = await opened(/Change Visibility/);
      if (!t(!!q, "asked again")) return;
      setValue(q.querySelector("select"), "Show all footprint fields");
      await sleep(200);
      buttonText(q, "OK").click();
      const shown = await until(async () => { const s = (await sch(path)).symbols.find((x) => x.id === sym.id); return s?.fields?.find((f) => f.name === "Footprint")?.visible ? s : null; }, 8000);
      changed = !!shown;
      t(!!shown, "the named symbol's Footprint field is shown");
    } finally {
      HTMLInputElement.prototype.click = realClick;
      if (changed) await undo();
    }
    const back = (await sch(path)).symbols.find((s) => s.id === sym.id);
    t(back.fields.find((f) => f.name === "Footprint").visible === false, "one undo hides it again");
  });

  // ------------------------------------------------------------------------------------------------------------------------ compare
  await scenario("Compare Symbol with Library on a resistor of a sheet this project drew: its drawing and pins are the library's (compared in the library's frame)", async (t) => {
    const { sym } = await enterResistor();
    if (!t(!!sym, "a Device:R")) return;
    await select([sym.id]);
    await run("eeschema.InspectionTool.diffSymbol");
    const d = await until(() => dialogs().find((x) => /No relevant differences detected\.|differs|differ\./.test(x.textContent ?? "")), 10000);
    const text = d?.textContent?.replace(/\s+/g, " ") ?? "";
    t(!!d, "the report opens");
    t(!/Graphic item|Pin \d|pin in schematic symbol|text sizes|line style/.test(text), `no graphic or pin differs (${text.slice(0, 240)})`);
    // (KiCad's default "Compare non-reference field text" reports the placed Value and Footprint the library's empty ones lack)
  });
  await scenario("the page's own corner_offset_mm is sent for a drawing a sheet this project drew", async (t) => {
    const { path } = await enterResistor();
    const drawn = (await sch(path)).lib_symbols?.["Device:R"];
    t(Array.isArray(drawn?.corner_offset_mm) && drawn.corner_offset_mm.length === 2, `the offset (${JSON.stringify(drawn?.corner_offset_mm)})`);
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
  return "started";
})();
