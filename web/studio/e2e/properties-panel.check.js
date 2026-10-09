// Properties panel check (GAPS.md item 10): select each kind of item, edit every editable row through the grid, read the edit back from the board, undo it, and
// compare the board with how it was. Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in
// Playwright -- on a COPY of a board (it edits and undoes, and it leaves Strict off). It adds nothing: a kind the board has none of is reported as skipped, so
// first put one of each kind on the board (the track, arc, via, zone, rule area, text, shapes, dimensions and group that `eda board` and the studio's tools make;
// on the schematic tab a junction, labels, text, line, sheet and graphics).
//
// It returns { tab, ok, failed, skipped, results: [{ kind, id, ok, rows: ["Width -> ok: track width ...", ...], undoOK }, ...] }: `ok` is the AND of the results and `skipped` names the
// kinds the board has none of (they count as ok, so look at that list). It works through
// `window.__eda` (select an item with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`) and the grid's own inputs, with the events a user
// makes. A whole board takes minutes (each edit waits for the board's answer), longer than a tool call may last: set `window.__propertiesCheckOnly = ["track", "via"]`
// first to run only the kinds whose names start with one of those (a kind that is left out is not reported). The results also pile up in
// `window.__propertiesCheck.results` (`.done` is set at the end), so a run that outlasts the tool call can be read afterwards.
(async () => {
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  const json = (url) => fetch(url).then((r) => r.json());
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const lastAct = async () => (await json("/api/state")).activity[0] ?? null;
  const results = [];
  window.__propertiesCheck = { results, done: false };
  const only = window.__propertiesCheckOnly;
  const wanted = (kind) => !Array.isArray(only) || only.some((o) => kind.startsWith(o));

  // Strict refuses a move that makes the board worse: an edit check wants the verb, not the gate.
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  document.querySelector('button[aria-label="Show Properties"]')?.click(); // the dock folds in a narrow window
  await sleep(300);

  const select = async (ids) => {
    await __eda.run("common.InteractiveSelection.clear");
    await __eda.run("common.InteractiveSelection.selectItems", ids);
    await sleep(250);
  };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(450); };
  const waitAct = async (before) => {
    const t0 = Date.now();
    while (Date.now() - t0 < 4000) {
      const a = await lastAct();
      if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(350); return a; }
      await sleep(40);
    }
    return null;
  };
  /** Type or pick `value` in the row's editor the way a user does, and wait for the command the grid sent. */
  const edit = async (name, value) => {
    const el = input(name);
    if (!el) return `no editable row ${name}`;
    const before = await lastAct();
    if (el instanceof HTMLSelectElement) {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    } else if (el.type === "checkbox") {
      if (el.checked === !!value) return "already";
      el.click();
    } else if (el.type === "color") {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(60);
      el.dispatchEvent(new Event("change", { bubbles: true }));
    } else {
      el.focus();
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(30);
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    const a = await waitAct(before);
    if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
    return a.ok ? `ok: ${a.cmd.slice(0, 80)}` : `REFUSED: ${a.message.slice(0, 100)}`;
  };
  const current = (name) => { const el = input(name); return el ? (el.type === "checkbox" ? el.checked : el.value) : null; };
  const num = (name) => Number(current(name));
  /** Another choice than the one the row has (never "<no net>": a track needs a net). */
  const other = (name) => { const el = input(name); return el instanceof HTMLSelectElement ? [...el.options].find((o) => !o.disabled && o.value !== "" && o.value !== el.value)?.value : undefined; };
  const bump = (name, d) => String(Math.round((num(name) + d) * 10000) / 10000);

  // What the board holds, by id, without what a geometry pass derives.
  const snapshot = async () => {
    const s = await json("/api/state");
    const m = {};
    for (const p of s.parts) m[p.ref] = { at: p.at, rot: p.rot, side: p.side };
    for (const k of ["tracks", "vias", "zones"]) for (const t of s.routing?.[k] ?? []) m[t.id] = t;
    for (const k of ["shapes", "texts", "groups"]) for (const t of s.drawings?.[k] ?? []) m[t.id] = t;
    for (const d of s.drawings?.dimensions ?? []) m[d.id] = { ...d, lines: 0, text_at: 0, computed_text_angle: 0, measured_value_um: 0 };
    m.__locked = s.locked;
    return JSON.stringify(m);
  };
  const snapshotSch = async () => {
    const s = await json("/api/schematic");
    const m = {};
    for (const k of ["symbols", "power_symbols", "wires", "no_connects", "labels", "texts", "bus_entries", "junctions", "lines", "graphics", "sheets"]) for (const it of s[k] ?? []) m[it.id + (k === "symbols" ? `#${it.unit}` : "")] = { ...it, pins: 0 };
    m.__locked = s.locked;
    return JSON.stringify(m);
  };

  /** Select `ids`, run `plan()` (it returns [[row, value-or-undefined]] from what the grid shows), undo every edit that was applied, compare. */
  const cycle = async (kind, ids, plan, snap) => {
    if (!wanted(kind)) return;
    const before = await snap();
    await select(ids);
    const rows = [];
    let applied = 0;
    let bad = false;
    for (const [name, value] of plan()) {
      if (value === undefined) { rows.push(`${name} -> skipped (no row or no other choice)`); continue; }
      const r = await edit(name, value);
      rows.push(`${name}=${value} -> ${r}`);
      if (r.startsWith("ok")) applied++;
      else if (r !== "already") bad = true;
    }
    for (let i = 0; i < applied; i++) await undo();
    const undoOK = (await snap()) === before;
    results.push({ kind, id: ids.join(","), ok: !bad && undoOK && applied > 0, rows, undoOK });
  };
  const skip = (kind) => { if (wanted(kind)) results.push({ kind, id: "-", ok: true, skipped: true, rows: ["skipped: the board has none"], undoOK: true }); };

  const tab = __eda.state().tab;
  if (tab === "pcb") {
    const s = await json("/api/state");
    const rt = s.routing ?? { tracks: [], vias: [], zones: [] };
    const dr = s.drawings ?? { shapes: [], texts: [], dimensions: [], groups: [] };
    const part = s.parts.find((p) => p.placed);
    const straight = rt.tracks.find((t) => !t.arc_mid);
    const arc = rt.tracks.find((t) => t.arc_mid);
    const via = rt.vias[0];
    const zone = rt.zones.find((z) => !z.is_rule_area && !z.teardrop);
    const rule = rt.zones.find((z) => z.is_rule_area);
    const text = dr.texts[0];
    const grouped = new Set(dr.groups.flatMap((g) => g.member_ids));
    // a member of a group is selected as the group: edit the members that are free
    const free = (x) => x && !grouped.has(x.id);
    const shape = (kind) => dr.shapes.find((x) => x.kind === kind && free(x));
    const dim = (kind) => dr.dimensions.find((d) => d.kind === kind);

    if (part) await cycle("footprint", [part.ref], () => [["Position X", bump("Position X", 2)], ["Orientation", String((num("Orientation") + 90) % 360)], ["Layer", other("Layer")], ["Locked", true]], snapshot); else skip("footprint");
    if (part?.pads?.length) await cycle("pad", [`${part.ref}.${part.pads[0].num}`], () => [["Position X", bump("Position X", 0.5)]], snapshot); else skip("pad");
    if (free(straight)) await cycle("track", [straight.id], () => [["Width", bump("Width", 0.1)], ["Layer", other("Layer")], ["Net", other("Net")], ["Start X", bump("Start X", 0.5)], ["End Y", bump("End Y", 0.5)], ["Locked", true]], snapshot); else skip("track");
    if (free(arc)) await cycle("arc", [arc.id], () => [["Width", bump("Width", 0.1)], ["Net", other("Net")], ["End X", bump("End X", 0.5)]], snapshot); else skip("arc");
    if (free(via)) await cycle("via", [via.id], () => [["Diameter", bump("Diameter", 0.2)], ["Hole", bump("Hole", 0.05)], ["Net", other("Net")], ["Position X", bump("Position X", 0.5)], ["Locked", true]], snapshot); else skip("via");
    if (zone) await cycle("zone", [zone.id], () => [["Priority", String(num("Priority") + 1)], ["Name", "pour"], ["Net", other("Net")], ["Fill Mode", "HatchPattern"], ["Hatch Orientation", "45"], ["Clearance", bump("Clearance", 0.1)], ["Pad Connections", "Full"], ["Remove Islands", "Area"], ["Minimum Island Area", "12"]], snapshot); else skip("zone");
    if (rule) await cycle("rule area", [rule.id], () => [["Keep Out Tracks", !current("Keep Out Tracks")], ["Name", "keepout"]], snapshot); else skip("rule area");
    if (text) await cycle("text", [text.id], () => [["Text", "edited"], ["Orientation", String((num("Orientation") + 90) % 360)], ["Layer", other("Layer")], ["Thickness", bump("Thickness", 0.05)], ["Height", bump("Height", 0.5)], ["Mirrored", !current("Mirrored")], ["Horizontal Justification", other("Horizontal Justification")], ["Position X", bump("Position X", 0.5)]], snapshot); else skip("text");
    for (const k of ["segment", "rect", "circle", "arc", "polygon"]) {
      const sh = shape(k);
      if (!sh) { skip(`shape ${k}`); continue; }
      const geometry = { segment: ["End X", "Start Y"], rect: ["Width", "Height", "Start X"], circle: ["Radius", "Center X"], arc: ["End Y"], polygon: ["Position X"] }[k];
      await cycle(`shape ${k}`, [sh.id], () => [...geometry.map((g) => [g, row(g) ? bump(g, 0.5) : undefined]), ["Line Width", bump("Line Width", 0.05)], ["Layer", other("Layer")]], snapshot);
    }
    for (const k of ["aligned", "radial", "leader"]) {
      const d = dim(k);
      if (!d) { skip(`dimension ${k}`); continue; }
      const plan = { aligned: () => [["Prefix", "L="], ["Crossbar Height", bump("Crossbar Height", 1)], ["Precision", other("Precision")], ["Units", other("Units")], ["Position X", bump("Position X", 1)]], radial: () => [["Leader Length", bump("Leader Length", 1)], ["Suffix", " mm"]], leader: () => [["Text", "NOTE2"]] }[k];
      await cycle(`dimension ${k}`, [d.id], plan, snapshot);
    }
    const group = dr.groups[0];
    if (group) await cycle("group", [group.id], () => [["Name", "named"], ["Locked", true]], snapshot); else skip("group");
    // several items: one edit, one undo step (the cycle undoes exactly one step for the one edit)
    const three = rt.tracks.filter(free).slice(0, 3);
    if (three.length >= 2) await cycle("three tracks", three.map((t) => t.id), () => [["Width", "0.5"]], snapshot); else skip("several tracks");
    // items of different kinds: the rows they share (Locked), one edit for all, one undo step
    const mixed = [part?.ref, free(straight) ? straight.id : null, free(via) ? via.id : null, free(text) ? text.id : null].filter(Boolean);
    if (mixed.length >= 3) await cycle("mixed items", mixed, () => [["Locked", true]], snapshot); else skip("mixed items");
  } else if (tab === "schematic") {
    const s = await json("/api/schematic");
    const first = (list, pred = () => true) => (list ?? []).find(pred);
    const sym = first(s.symbols);
    const power = first(s.power_symbols);
    const wire = first(s.wires, (w) => !w.bus);
    const junction = first(s.junctions);
    const entry = first(s.bus_entries);
    const label = (scope) => first(s.labels, (l) => l.scope === scope);
    const text = first(s.texts);
    const line = first(s.lines);
    const sheet = first(s.sheets);
    const graphic = (type) => first(s.graphics, (g) => g.shape.type === type);
    if (sym) await cycle("symbol", [sym.id], () => [["Position X", bump("Position X", 1)], ["Orientation", String(((Number(current("Orientation")) || 0) + 90) % 360)], ["Mirror X", !current("Mirror X")], ["Value", "22k"], ["Footprint", "R_0805"], ["Do not Populate", true], ["Locked", true]], snapshotSch); else skip("symbol");
    if (power) await cycle("power symbol", [power.id], () => [["Position Y", bump("Position Y", 1)], ["Orientation", String(((Number(current("Orientation")) || 0) + 90) % 360)]], snapshotSch); else skip("power symbol");
    if (wire) await cycle("wire", [wire.id], () => [["Wire Style", "dash"], ["Line Width", "0.3"], ["Color", "#ff0000"]], snapshotSch); else skip("wire");
    if (junction) await cycle("junction", [junction.id], () => [["Diameter", "0.9"], ["Color", "#0000ff"]], snapshotSch); else skip("junction");
    if (entry) await cycle("bus entry", [entry.id], () => [["Wire Style", "dot"], ["Line Width", "0.4"]], snapshotSch); else skip("bus entry");
    for (const scope of ["local", "global", "hierarchical"]) {
      const l = label(scope);
      if (l) await cycle(`${scope} label`, [l.id], () => (scope === "local" ? [["Text", "RENAMED"]] : [["Text", "RENAMED"], ["Shape", other("Shape")]]), snapshotSch); else skip(`${scope} label`);
    }
    if (text) await cycle("text", [text.id], () => [["Text", "NEW TEXT"], ["Text Size", "2"]], snapshotSch); else skip("text");
    if (line) await cycle("graphic line", [line.id], () => [["Line Style", "dash_dot"], ["Line Width", "0.3"]], snapshotSch); else skip("graphic line");
    if (sheet) await cycle("sheet", [sheet.id], () => [["Sheet Name", "Supply"]], snapshotSch); else skip("sheet");
    const shapes = { rectangle: [["Width", "50"], ["Fill", "background"], ["Line Style", "dash"]], circle: [["Radius", "10"], ["Center X", "110"]], arc: [["End Y", "162"], ["Line Width", "0.4"]], polygon: [["Fill", "outline"], ["Line Width", "0.5"]], text_box: [["Text", "changed box"], ["Bold", true], ["Horizontal Justification", "right"]], rule_area: [["Exclude From Board", true], ["Do not Populate", true]], directive: [["Shape", "diamond"], ["Pin length", "5"]] };
    for (const [type, plan] of Object.entries(shapes)) {
      const g = graphic(type);
      if (g) await cycle(`graphic ${type}`, [g.id], () => plan, snapshotSch); else skip(`graphic ${type}`);
    }
    if (s.symbols.length >= 2) await cycle("several symbols", s.symbols.slice(0, 3).map((x) => x.id), () => [["Do not Populate", true], ["Value", "0"]], snapshotSch); else skip("several symbols");
    const mixed = [sym?.id, label("local")?.id, text?.id, junction?.id].filter(Boolean);
    if (mixed.length >= 3) await cycle("mixed items", mixed, () => [["Locked", true]], snapshotSch); else skip("mixed items");
  }

  if (strictWas && strict && !strict.checked) strict.click();
  window.__propertiesCheck.done = true;
  return { tab, ok: results.every((r) => r.ok), failed: results.filter((r) => !r.ok), skipped: results.filter((r) => r.skipped).map((r) => r.kind), results };
})()
