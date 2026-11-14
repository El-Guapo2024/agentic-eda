// Microvia stacks (UI-LEFT-3 X3-1, task-v19-microvia): KiCad 25ec2335's `PCB_VIA_STACK` generator, driven the way a person does it through `window.__eda`, the dialogs and the
// pointer events of the canvas, and checked against what the server holds (`/api/state`). The board is KiCad's own qa/data/pcbnew/via_stacks.kicad_pcb (six copper layers, a
// stacked F.Cu..In2.Cu stack and a staggered In1.Cu..In3.Cu one), brought in with `replace_board`.
//
//   1  read      both of the file's stacks are stacks of the board (their settings, their vias and trace as members), each listed with the groups as a generator
//   2  menus     Place has Place Microvia Stack and Create Stack from Selection after Place Vias; the drawing toolbar's Via tools group has Place Microvia Stack; the
//                auxiliary toolbar's microvia stack selector says "Microvia stack: none defined"
//   3  place     with no preset Place Microvia Stack opens the Microvia Stack dialog (a span the board lacks refused with KiCad's words); OK arms the tool, a click
//                places a stacked stack there: its microvias, filled; Undo takes it away in one step
//   4  preset    a staggered preset picked in the toolbar selector: the tool places it without a dialog, a click per hop (the second steered at the pitch, Delete takes
//                back a hop); the last hop places the stack with its traces
//   5  dialog    a double click on the stack opens the Microvia Stack dialog on it; a new pitch re-spaces its hops, its members kept
//   6  panel     the Properties panel shows the stack's rows (Start Layer, End Layer, Style, Pitch, Copper-filled, Capped), read-only
//   7  make      two loose microvias of a column selected: Create Stack from Selection makes them a stacked stack, which is then selected
//   8  delete    Delete on a stack takes its vias and traces with it
//   end          every step undone
// It answers through `window.__viaStacksCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=kicad10-viastacks minutes=12
(async () => {
  const results = [];
  const check = (window.__viaStacksCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 40000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const board = () => json("/api/state");
  const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const escape = async () => { await E.run("common.Interactive.cancel"); await sleep(400); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(300); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const waitFor = async (cond, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await cond()) return true; await sleep(200); } return false; };
  const dialogs = () => [...document.querySelectorAll(".dialog")];
  const stackDialog = () => dialogs().find((d) => /Microvia Stack/.test(d.querySelector(".dialog-title, header, h2, h3")?.textContent ?? d.textContent.slice(0, 40)) && d.querySelector('select[aria-label="Start layer"]'));
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => ["Cancel", "OK"].includes(x.textContent.trim()) && x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const press = (root, text) => { const b = [...root.querySelectorAll("button")].find((x) => x.textContent.trim() === text); if (!b) throw new Error(`no "${text}" button`); b.click(); };
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const target = () => canvasBox().querySelector("canvas") || canvasBox();
  const fire = (type, x, y, extra = {}) => {
    const rect = canvasBox().getBoundingClientRect();
    target().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  };
  const mouse = (type, x, y) => { const rect = canvasBox().getBoundingClientRect(); target().dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX: rect.left + x, clientY: rect.top + y, button: 0 })); };
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? ""); return m ? [Number(m[1]), Number(m[2])] : null; };
  let toScreen = null;
  const screenOf = async () => {
    fire("pointermove", 100, 100); await sleep(150); const a = readout();
    fire("pointermove", 500, 400); await sleep(150); const b = readout();
    if (!a || !b) return null;
    const sx = (b[0] - a[0]) / 400;
    const sy = (b[1] - a[1]) / 300;
    return (xmm, ymm) => [100 + (xmm - a[0]) / sx, 100 + (ymm - a[1]) / sy];
  };
  const hover = async (xmm, ymm) => { const [x, y] = toScreen(xmm, ymm); fire("pointermove", x, y); await sleep(200); return [x, y]; };
  const click = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); fire("pointerdown", x, y, { buttons: 1 }); await sleep(60); fire("pointerup", x, y); mouse("click", x, y); await sleep(600); };
  const dblclick = async (xmm, ymm) => { const [x, y] = await hover(xmm, ymm); mouse("dblclick", x, y); await sleep(700); };
  const menuLabels = () => { const m = document.querySelector(".menubar-dropdown, .context-menu"); return m ? [...m.querySelectorAll("*")].filter((e) => e.children.length === 0).map((e) => (e.textContent || "").trim()).filter(Boolean) : []; };
  const stacks = async () => (await board()).drawings?.via_stacks ?? [];
  const mm = (nm) => nm / 1e6;
  const near = (a, b, tol = 2000) => Math.abs(a - b) <= tol;

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    try { await escape(); await escape(); } catch { /* ignore */ }
    results.push(entry);
  };

  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) strict.click();
  await sleep(300);
  await E.run("common.Control.zoomFitObjects");
  await sleep(500);
  toScreen = await screenOf();
  if (!toScreen) { results.push({ name: "setup", ok: false, notes: ["FAIL: the canvas gave no cursor readout"] }); check.done = true; return; }
  const versions = [];
  const mark = async () => versions.push((await json("/api/version")) ?? null);

  (async () => {
    await scenario("read: the file's two stacks are stacks of the board, listed with the groups as generators", async (t) => {
      const st = await board();
      const list = st.drawings?.via_stacks ?? [];
      t(list.length === 2, `two stacks (${list.length})`);
      const stacked = list.find((s) => s.start_layer === "F.Cu");
      const staggered = list.find((s) => s.start_layer === "In1.Cu");
      t(stacked && stacked.end_layer === "In2.Cu" && (stacked.style ?? "stacked") === "stacked" && stacked.capped === true && stacked.preset === "HDI 1+N+1" && (stacked.member_ids ?? []).length === 2, `the stacked one (${JSON.stringify(stacked)})`);
      t(staggered && staggered.style === "staggered" && staggered.pitch_nm === 450000 && staggered.filled === false && (staggered.hops ?? []).length === 2 && (staggered.member_ids ?? []).length === 3, `the staggered one with its trace (${JSON.stringify(staggered)})`);
      const gens = (st.drawings?.groups ?? []).filter((g) => g.generator === "via_stack");
      t(gens.length === 2, `listed with the groups as generators (${gens.length})`);
    });

    await scenario("menus: Place and the toolbars have the microvia stack's actions and selector", async (t) => {
      const acts = Object.fromEntries(E.actions({ all: true }).map((a) => [a.id, a]));
      for (const id of ["pcbnew.InteractiveDrawing.viaStack", "pcbnew.Generator.makeViaStack"]) t(!!acts[id], `${id} is an action of the board editor`);
      const bar = [...document.querySelectorAll(".menubar-item")].find((e) => e.childNodes[0]?.textContent?.trim() === "Place");
      for (let i = 0; i < 2 && menuLabels().length === 0; i++) { bar?.click(); await sleep(300); }
      const labels = menuLabels();
      const at = (want) => labels.findIndex((l) => l.startsWith(want));
      t(at("Place Microvia Stack") > at("Place Vias") && at("Place Vias") >= 0, `Place has Place Microvia Stack after Place Vias (${labels.slice(0, 8).join(" | ")})`);
      t(at("Create Stack from Selection") === at("Place Microvia Stack") + 1, "then Create Stack from Selection");
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await sleep(200);
      const sel = document.querySelector('select[data-control="viaStack"]');
      t(!!sel, "the auxiliary toolbar has the microvia stack selector");
      t(sel && [...sel.options].map((o) => o.textContent).join("|") === "Microvia stack: none defined|---|Edit Pre-defined Microvia Stacks...", `with KiCad's entries (${sel && [...sel.options].map((o) => o.textContent).join("|")})`);
    });

    await scenario("place: with no preset the dialog first, then a click places a stacked stack (one undo step)", async (t) => {
      const before = await stacks();
      await E.run("pcbnew.InteractiveDrawing.viaStack");
      t(await waitFor(() => !!stackDialog(), 3000), "the Microvia Stack dialog opens");
      const d = stackDialog();
      setValue(d.querySelector('select[aria-label="End layer"]'), "F.Cu");
      await sleep(150);
      press(d, "OK");
      await sleep(300);
      const err = dialogs().find((x) => /start and end layers must be different/.test(x.textContent));
      t(!!err, "the same start and end layer are refused with KiCad's words");
      if (err) press(err, "OK");
      await sleep(200);
      const d2 = stackDialog();
      setValue(d2.querySelector('select[aria-label="End layer"]'), "In2.Cu");
      const size = d2.querySelector('input[aria-label="Via diameter"]');
      const hole = d2.querySelector('input[aria-label="Via hole"]');
      setValue(size, "0.3"); size.dispatchEvent(new Event("blur", { bubbles: true }));
      setValue(hole, "0.1"); hole.dispatchEvent(new Event("blur", { bubbles: true }));
      await sleep(200);
      press(d2, "OK");
      await sleep(400);
      t((await E.state()).tool === "via_stack", `the tool is armed (${(await E.state()).tool})`);
      await click(60, 30);
      const after = await stacks();
      const made = after.find((s) => !before.some((b) => b.id === s.id));
      t(!!made, "a click places a stack");
      if (made) {
        t(made.start_layer === "F.Cu" && made.end_layer === "In2.Cu" && near(made.origin.x, 60e6, 200000) && near(made.origin.y, 30e6, 200000), `F.Cu..In2.Cu where it was clicked (${JSON.stringify(made.origin)})`);
        const st = await board();
        const vias = st.routing.vias.filter((v) => made.member_ids.includes(v.id));
        t(vias.length === 2 && vias.every((v) => v.d === 300000 && v.drill === 100000), `two microvias of 0.3 / 0.1 mm (${JSON.stringify(vias.map((v) => [v.d, v.drill]))})`);
      }
      await escape();
      await undo();
      t((await stacks()).length === before.length, "Undo takes it away");
    });

    await scenario("preset: a staggered preset from the toolbar, a click per hop (Delete takes one back), its traces", async (t) => {
      const preset = { name: "Stag 0.6", start_layer: "In1.Cu", end_layer: "In4.Cu", staggered: true, via_size_nm: 250000, via_drill_nm: 100000, use_netclass: false, filled: false, capped: false, pitch_nm: 600000 };
      t((await cmd({ op: "set_via_stack_presets", presets: [preset] })).ok, "a preset defined (Board Setup's Microvia Stacks)");
      await sleep(900);
      const sel = document.querySelector('select[data-control="viaStack"]');
      t(sel && sel.options[0]?.textContent === "Microvia stack: Stag 0.6", `the selector lists it (${sel?.options[0]?.textContent})`);
      setValue(sel, "0");
      await sleep(200);
      const before = await stacks();
      await E.run("pcbnew.InteractiveDrawing.viaStack");
      await sleep(300);
      t(!stackDialog(), "no dialog with a preset");
      t((await E.state()).tool === "via_stack", `the tool is armed (${(await E.state()).tool})`);
      await click(60, 40);
      await E.run("common.Interactive.delete");
      await sleep(200);
      t((await stacks()).length === before.length, "nothing placed by the first hop, nor by Delete");
      // the 45-degree line mode: the steered hop snaps to 45-degree steps (`GetAngleSnapMode()`), here from a cursor a little off straight
      await E.run("pcbnew.EditorControl.lineMode45");
      await sleep(200);
      await click(60, 40);
      await click(61, 40.1);
      t((await stacks()).length === before.length, "two hops of three: nothing placed yet");
      await click(62, 40.5);
      const made = (await stacks()).find((s) => !before.some((b) => b.id === s.id));
      t(!!made, "the last hop places the stack");
      if (made) {
        const hops = made.hops ?? [];
        t(made.style === "staggered" && hops.length === 3 && made.preset === "Stag 0.6", `staggered, three hops, its preset (${JSON.stringify(made)})`);
        if (hops.length === 3) {
          const step = Math.hypot(hops[1].x - hops[0].x, hops[1].y - hops[0].y);
          t(near(step, 600000, 2), `the second hop steered at the pitch (${step})`);
          t(near(hops[1].y, hops[0].y, 2), `at a 45-degree step: straight on (${JSON.stringify(hops)})`);
        }
        const st = await board();
        const vias = st.routing.vias.filter((v) => made.member_ids.includes(v.id));
        const tracks = st.routing.tracks.filter((x) => made.member_ids.includes(x.id));
        t(vias.length === 3 && tracks.length === 2, `three microvias and two traces (${vias.length}, ${tracks.length})`);
        t(tracks.map((x) => x.layer).sort().join(",") === "In2.Cu,In3.Cu", `the traces on the landing layers (${tracks.map((x) => x.layer)})`);
        check.staggered = made.id;
      }
      await E.run("pcbnew.EditorControl.lineModeFree");
      await sleep(200);
    });

    await scenario("dialog: a double click opens the Microvia Stack dialog; a new pitch re-spaces the hops, the members kept", async (t) => {
      const s = (await stacks()).find((x) => x.id === check.staggered);
      if (!s) return t(false, "no stack from the last scenario");
      await escape();
      await dblclick(mm(s.hops[0].x), mm(s.hops[0].y));
      t(await waitFor(() => !!stackDialog(), 3000), "the Microvia Stack dialog opens on the stack");
      const d = stackDialog();
      if (!d) return;
      t(d.querySelector('select[aria-label="Start layer"]').value === "In1.Cu" && d.querySelector('select[aria-label="Type"]').value === "1", "it shows the stack's span and type");
      const pitch = d.querySelector('input[aria-label="Pitch"]');
      setValue(pitch, "0.8"); pitch.dispatchEvent(new Event("blur", { bubbles: true }));
      await sleep(150);
      press(d, "OK");
      await sleep(900);
      const n = (await stacks()).find((x) => x.id === s.id);
      t(n && n.pitch_nm === 800000, `the pitch is 0.8 mm (${n?.pitch_nm})`);
      if (n) {
        const step = Math.hypot(n.hops[1].x - n.hops[0].x, n.hops[1].y - n.hops[0].y);
        t(near(step, 800000, 2), `the hops re-spaced (${step})`);
        t(JSON.stringify([...n.member_ids].sort()) === JSON.stringify([...s.member_ids].sort()), "its members kept (reshaped in place)");
      }
    });

    await scenario("panel: the Properties panel shows the stack's rows, read-only", async (t) => {
      await select([check.staggered]);
      if (!document.querySelector(".prop-row")) { document.querySelector('button[aria-label="Show Properties"]')?.click(); await sleep(500); }
      for (const name of ["Start Layer", "End Layer", "Style", "Pitch", "Copper-filled", "Capped", "Use Netclass Values", "Via Diameter", "Via Hole"]) {
        const row = document.querySelector(`.prop-row[data-prop="${name}"]`);
        t(!!row, `${name} is shown`);
        if (row) t(!!row.querySelector("input:disabled, select:disabled") || !row.querySelector("input, select"), `${name} is read-only`);
      }
      t(!document.querySelector('.prop-row[data-prop="Layer"]'), "no Layer row (masked)");
    });

    await scenario("make: two loose microvias selected become a stacked stack (Create Stack from Selection)", async (t) => {
      const before = await stacks();
      const place = (a, z) => cmd({ op: "place_via", net: "", x: 70e6, y: 30e6, drill: 100000, diameter: 300000, from_layer: a, to_layer: z, allow_drc_violations: true });
      t((await place("F.Cu", "In1.Cu")).ok && (await place("In1.Cu", "In2.Cu")).ok, "two vias of a column");
      await sleep(600);
      const st = await board();
      const ids = st.routing.vias.filter((v) => v.x === 70e6 && v.y === 30e6).map((v) => v.id);
      t((await cmd({ op: "set_via_layers", ids, kind: "micro" })).ok, "made microvias");
      await sleep(600);
      await select(ids);
      await E.run("pcbnew.Generator.makeViaStack");
      await sleep(1200);
      const made = (await stacks()).find((s) => !before.some((b) => b.id === s.id));
      t(!!made && made.start_layer === "F.Cu" && made.end_layer === "In2.Cu" && (made.style ?? "stacked") === "stacked" && made.via_size_nm === 300000, `a stacked F.Cu..In2.Cu stack read off the vias (${JSON.stringify(made)})`);
      t(made && (await E.state()).selection.some((x) => x.id === made.id), `the new stack is selected (${JSON.stringify((await E.state()).selection)})`);
      check.made = made?.id;
    });

    await scenario("delete: Delete on a stack takes its vias and traces with it", async (t) => {
      const s = (await stacks()).find((x) => x.id === check.staggered);
      if (!s) return t(false, "no staggered stack");
      await select([s.id]);
      await E.run("common.Interactive.delete");
      await sleep(900);
      const st = await board();
      t(!(st.drawings?.via_stacks ?? []).some((x) => x.id === s.id), "the stack is gone");
      t(!st.routing.vias.some((v) => s.member_ids.includes(v.id)) && !st.routing.tracks.some((x) => s.member_ids.includes(x.id)), "and its vias and traces");
      await undo();
      t((await stacks()).some((x) => x.id === s.id), "Undo brings it back");
    });

    await scenario("end: every step undone", async (t) => {
      for (let i = 0; i < 12; i++) {
        const n = (await stacks()).length;
        const p = (await board()).drawings?.via_stack_presets ?? [];
        if (n === 2 && p.length === 0) break;
        await undo();
      }
      const st = await board();
      t((st.drawings?.via_stacks ?? []).length === 2, `the board's two stacks are left (${(st.drawings?.via_stacks ?? []).length})`);
      t((st.drawings?.via_stack_presets ?? []).length === 0, "and no preset");
      const errs = (E.errors?.() ?? []).filter((e) => !/favicon/.test(String(e)));
      t(errs.length === 0, `no page errors (${errs.slice(0, 3).join(" / ")})`);
    });

    void mark;
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
})();
