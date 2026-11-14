// The board's Properties panel and point editors of UI-LEFT-3 P3-8 .. P3-12 and P3-36 (task-v16-pcbprops; PARITY-pcb section 120), in the page:
//   P3-8   a point's Size of 0 is taken (`PCB_POINT_DESC` registers no validator) and a negative one refused;
//   P3-9   a pad renamed to the number of another pad on another net keeps its own net (`PAD::SetNumber`), the other pad keeps its;
//   P3-10  a reference image's Transform Offset X is live; Scale scales the image about that origin; the image's point editor moves the origin and scales it by a
//          corner, the origin staying where it is (`REFERENCE_IMAGE_POINT_EDIT_BEHAVIOR`);
//   P3-11  a dimension's Auto Thickness row turns its pen automatic and back; Dimension Properties has the auto-thickness button (KiCad 25ec2335);
//   P3-12  Draw Dimension shows the hidden layer it starts on, and the hidden layer a layer change while drawing goes to (`SetLayerVisible( layer, true )`);
//   P3-36  a footprint's user fields are rows of "Fields", sorted by name without regard to case, and an edit of one sets the field's text.
// Every scenario undoes what it did. It answers through `window.__pcbPropsV16Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=ldo minutes=8
(async () => {
  const results = [];
  const check = (window.__pcbPropsV16Check = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...(init ?? {}) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden page throttles timers
  check.wait = async (ms = 60000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const mm = (v) => Math.round(v * 1_000_000);
  const state = () => json("/api/state");
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const waitFor = async (pred, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = await pred(); if (v) return v; await sleep(150); } return null; };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(700); };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(450); };
  const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
  const input = (name) => row(name)?.querySelector("[data-prop-input]");
  const lastAct = async () => (await state()).activity[0] ?? null;
  const waitAct = async (before) => {
    const t0 = Date.now();
    while (Date.now() - t0 < 10000) {
      const a = await lastAct();
      if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(400); return a; }
      await sleep(60);
    }
    return null;
  };
  /** Type or tick `value` in the row's editor, as a user does; the command the grid sent, or why none went. */
  const edit = async (name, value, wait = true) => {
    for (let i = 0; i < 25 && !input(name); i++) await sleep(200);
    const el = input(name);
    if (!el) return row(name) ? `read-only row ${name}: ${row(name).dataset.note ?? ""}` : `no row ${name}`;
    const before = await lastAct();
    if (el.type === "checkbox") {
      if (el.checked === !!value) return "already";
      el.click();
    } else {
      el.focus();
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(30);
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    if (!wait) { await sleep(500); return "sent"; }
    const a = await waitAct(before);
    if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
    return a.ok ? `ok: ${a.cmd.slice(0, 90)}` : `REFUSED: ${a.message.slice(0, 120)}`;
  };
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  /** Board nanometres to the page's pixels. */
  const w2c = (x, y) => { const v = E.state().view, r = canvas().getBoundingClientRect(); return [r.left + v.x + x * v.scale, r.top + v.y + y * v.scale]; };
  const fire = (type, x, y, buttons = 1) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : buttons }));
  };
  const clickAt = async (x, y) => { const [px, py] = w2c(x, y); fire("pointermove", px, py, 0); await sleep(150); fire("pointerdown", px, py); await sleep(40); fire("pointerup", px, py); await sleep(450); };
  const moveTo = async (x, y) => { const [px, py] = w2c(x, y); fire("pointermove", px, py, 0); await sleep(200); };
  const dragFromTo = async (a, b) => {
    const [x0, y0] = w2c(a[0], a[1]);
    const [x1, y1] = w2c(b[0], b[1]);
    fire("pointermove", x0, y0, 0);
    await sleep(150);
    fire("pointerdown", x0, y0);
    await sleep(60);
    for (let i = 1; i <= 8; i++) { fire("pointermove", x0 + ((x1 - x0) * i) / 8, y0 + ((y1 - y0) * i) / 8); await sleep(60); }
    await sleep(300);
    fire("pointerup", x1, y1);
    await sleep(1000);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const closeDialogs = async () => { for (let i = 0; i < 3; i++) { const b = [...document.querySelectorAll(".dialog button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(200); } };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); await leaveTool(); await select([]); } catch { /* best effort */ }
    results.push(entry);
  };
  const hidden = () => E.state().appearance.hiddenLayers;
  /** The Appearance panel's eye of a layer: hide it when it is shown. */
  const hideLayer = async (layer) => {
    const eye = document.querySelector(`.ap-row[data-layer="${layer}"] .ap-eye`);
    if (eye && !hidden().includes(layer)) eye.click();
    await sleep(300);
    return hidden().includes(layer);
  };

  if (E.state().tab !== "pcb") {
    [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click();
    await waitFor(() => E.state().tab === "pcb");
    await sleep(1200);
  }
  document.querySelector('button[aria-label="Show Properties"]')?.click();
  await E.run("common.Control.zoomFitScreen");
  await sleep(600);
  const s0 = await state();
  const [ox, oy] = s0.outline?.length ? [Math.min(...s0.outline.map((p) => p[0])), Math.min(...s0.outline.map((p) => p[1]))] : [0, 0];
  const at = (x, y) => [ox + mm(x), oy + mm(y)];

  // ------------------------------------------------------------------------------------------------------------------------ P3-8
  await scenario("P3-8: a point takes a Size of 0 and refuses a negative one", async (t) => {
    const [x, y] = at(3, 3);
    const r = await post({ op: "pcb_item", verb: "add_point", point: { layer: "F.SilkS", at: { x, y }, size_nm: mm(1) } });
    t(r.ok !== false, `add_point: ${r.message ?? "ok"}`);
    await sleep(500);
    const p = (await state()).drawings.points.at(-1);
    await select([p.id]);
    const e = await edit("Size", "0");
    t(e.startsWith("ok") && (await state()).drawings.points.find((q) => q.id === p.id)?.size === 0, `Size 0: ${e}`);
    await select([p.id]);
    await edit("Size", "-1", false);
    t((await state()).drawings.points.find((q) => q.id === p.id)?.size === 0 && !!document.querySelector(".prop-error"), "a negative size is refused");
    await undo();
    await undo();
    t(!(await state()).drawings.points.some((q) => q.id === p.id), "both undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ P3-9
  await scenario("P3-9: a pad renamed to another pad's number keeps its own net", async (t) => {
    const s = await state();
    const part = s.parts.find((p) => p.placed && p.pads?.length >= 2 && p.pads.some((a) => a.net) && new Set(p.pads.map((a) => a.net ?? "")).size >= 2);
    if (!t(!!part, `a placed footprint with pads on two nets (${part?.ref})`)) return;
    const a = part.pads.find((q) => q.net);
    const b = part.pads.find((q) => (q.net ?? "") !== (a.net ?? ""));
    await select([`${part.ref}.${a.num}`]);
    const e = await edit("Pad Number", b.num);
    const now = (await state()).parts.find((p) => p.ref === part.ref);
    const nets = now.pads.filter((q) => q.num === b.num).map((q) => q.net ?? "").sort();
    t(e.startsWith("ok") && JSON.stringify(nets) === JSON.stringify([a.net ?? "", b.net ?? ""].sort()), `${part.ref}.${a.num} -> ${b.num}: the pads numbered ${b.num} are on ${JSON.stringify(nets)} (${e})`);
    const file = await fetch("/api/board.kicad_pcb").then((r) => r.text());
    t(file.includes(`(net "${a.net}")`) || file.includes(`"${a.net}"`), "the board file still has the pad's net");
    await undo();
    const back = (await state()).parts.find((p) => p.ref === part.ref);
    t(back.pads.some((q) => q.num === a.num && (q.net ?? "") === (a.net ?? "")), "undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ P3-10
  await scenario("P3-10: a reference image's transform origin: the Properties rows, Scale about it, the point editor's origin and corner", async (t) => {
    const c = document.createElement("canvas");
    c.width = 120;
    c.height = 80;
    const g = c.getContext("2d");
    g.fillStyle = "#3a6";
    g.fillRect(0, 0, 120, 80);
    const data = c.toDataURL("image/png").split(",")[1];
    const [x, y] = at(13, 13);
    const r = await post({ op: "pcb_item", verb: "add_ref_image", image: { layer: "Cmts.User", at: { x, y }, scale_ppm: 1_000_000, data } });
    t(r.ok !== false, `add_ref_image: ${r.message ?? "ok"}`);
    await sleep(600);
    const img0 = (await state()).drawings.images.at(-1);
    const image = async () => (await state()).drawings.images.find((i) => i.id === img0.id);
    await select([img0.id]);
    t(!!input("Transform Offset X"), "Transform Offset X is writeable");
    const e = await edit("Transform Offset X", "2");
    t(e.startsWith("ok") && (await image()).offset?.x === mm(2), `Transform Offset X = 2 mm (${e}; ${JSON.stringify((await image()).offset)})`);
    await select([img0.id]);
    const sc = await edit("Scale", "0.5");
    const half = await image();
    t(sc.startsWith("ok") && half.scale_ppm === 500_000 && half.x + (half.offset?.x ?? 0) === img0.x + mm(2), `Scale 0.5 about the origin: centre ${half.x}, offset ${JSON.stringify(half.offset)} (origin ${img0.x + mm(2)})`);
    await undo();
    // the point editor: the origin handle, then the bottom-right corner
    await select([img0.id]);
    const i1 = await image();
    const origin = [i1.x + (i1.offset?.x ?? 0), i1.y + (i1.offset?.y ?? 0)];
    await dragFromTo(origin, [origin[0] - mm(1), origin[1] + mm(1)]);
    const i2 = await image();
    t(i2.offset && i2.offset.x === mm(1) && i2.offset.y === mm(1) && i2.x === i1.x, `the origin handle moves the origin only (${JSON.stringify(i2.offset)})`);
    const br = [i2.x + Math.trunc(i2.w / 2), i2.y + Math.trunc(i2.h / 2)];
    const o2 = [i2.x + i2.offset.x, i2.y + i2.offset.y];
    await select([img0.id]);
    await dragFromTo(br, [o2[0] + (br[0] - o2[0]) * 2, o2[1] + (br[1] - o2[1]) * 2]);
    const i3 = await image();
    const o3 = [i3.x + (i3.offset?.x ?? 0), i3.y + (i3.offset?.y ?? 0)];
    t(i3.scale_ppm > 1_500_000 && Math.abs(o3[0] - o2[0]) <= 2 && Math.abs(o3[1] - o2[1]) <= 2, `a corner scales about the origin (scale ${i3.scale_ppm}, origin ${o3} vs ${o2})`);
    for (let k = 0; k < 4; k++) await undo();
    t(!(await state()).drawings.images.some((i) => i.id === img0.id), "all undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ P3-12, P3-11
  await scenario("P3-12, P3-11: Draw Dimension shows a hidden layer; the dimension's automatic pen in the Properties panel and the dialog", async (t) => {
    await E.run("pcbnew.Control.layerTop");
    await sleep(300);
    t(await hideLayer("F.Cu"), `F.Cu hidden (${JSON.stringify(hidden())})`);
    const before = (await state()).drawings?.dimensions ?? [];
    await E.run("pcbnew.InteractiveDrawing.alignedDimension");
    const [ax, ay] = at(4, 22);
    await clickAt(ax, ay);
    t(!hidden().includes("F.Cu"), `SET_ORIGIN shows F.Cu (${JSON.stringify(hidden())})`);
    t(await hideLayer("B.Cu"), "B.Cu hidden");
    await moveTo(ax + mm(6), ay);
    await E.run("pcbnew.Control.layerBottom");
    await sleep(500);
    t(!hidden().includes("B.Cu"), `a layer change while drawing shows B.Cu (${JSON.stringify(hidden())})`);
    await E.run("pcbnew.Control.layerTop");
    await sleep(300);
    await clickAt(ax + mm(10), ay);
    await moveTo(ax + mm(10), ay - mm(2));
    await clickAt(ax + mm(10), ay - mm(2));
    const made = await waitFor(async () => ((await state()).drawings?.dimensions ?? []).find((d) => !before.some((b) => b.id === d.id)));
    if (!t(!!made, `the dimension is added (${made?.layer}, pen ${made?.text_thickness_nm})`)) return;
    await leaveTool();
    const dim = async () => (await state()).drawings.dimensions.find((d) => d.id === made.id);
    await select([made.id]);
    const on = await edit("Auto Thickness", true);
    t(on.startsWith("ok") && (await dim()).text_thickness_nm == null, `Auto Thickness on: no pen of its own (${on}; ${(await dim()).text_thickness_nm})`);
    await select([made.id]);
    const shown = Number(input("Thickness")?.value);
    const w = (await dim()).text_width_nm ?? (await dim()).text_size_nm;
    t(Math.abs(shown - (Math.round(w / 8) / 1e6)) < 1e-6, `Thickness shows the automatic pen, an eighth of the width (${shown} mm, width ${w})`);
    // the dialog: the auto-thickness button is down and the thickness greyed; up again, OK keeps GetPenSizeForNormal( width )
    await E.run("pcbnew.InteractiveEdit.properties");
    const dialog = await waitFor(() => tid("dimension-properties"));
    if (t(!!dialog, "Dimension Properties opens")) {
      const btn = tid("dim-auto-thickness");
      t(btn?.getAttribute("aria-pressed") === "true" && tid("dim-text-thickness")?.disabled === true, "the auto-thickness button is down, the thickness greyed");
      btn?.click();
      await sleep(200);
      t(tid("dim-text-thickness")?.disabled === false, "up: the thickness is editable");
      [...document.querySelectorAll(".dialog button")].find((b) => b.textContent.trim() === "OK")?.click();
      await sleep(900);
      t((await dim()).text_thickness_nm === Math.round(w / 8), `OK: the pen it showed is the label's own (${(await dim()).text_thickness_nm})`);
      await undo();
    }
    await undo();
    await undo();
    t(!(await state()).drawings.dimensions.some((d) => d.id === made.id), "all undone");
  });

  // ------------------------------------------------------------------------------------------------------------------------ P3-36
  await scenario("P3-36: a footprint's user fields are rows of Fields, sorted without regard to case, and set the field's text", async (t) => {
    const s = await state();
    const part = s.parts.find((p) => p.placed && p.fields?.some((f) => f.name === "Value"));
    if (!t(!!part, `a placed footprint (${part?.ref})`)) return;
    const layoutOf = (f) => ({ at: { x: f.lx, y: f.ly }, angle: f.langle, size: [f.w, f.h], thickness: f.thickness, layer: f.layer, visible: false, halign: f.halign, valign: f.valign, mirror: f.mirror, bold: f.bold, italic: f.italic, keep_upright: f.upright, knockout: f.knockout });
    const kept = part.fields.filter((f) => f.name !== "Reference" && f.name !== "Value" && (!(f.name === "Datasheet" || f.name === "Description") || f.custom)).map((f) => ({ name: f.name, text: f.text, layout: { ...layoutOf(f), visible: f.visible } }));
    const value = part.fields.find((f) => f.name === "Value");
    const r = await post({ op: "edit_board_footprint", part: part.ref, fields: [...kept, { name: "zeta", text: "Z1", layout: layoutOf(value) }, { name: "Alpha", text: "A1", layout: layoutOf(value) }] });
    t(r.ok !== false, `two user fields added: ${r.message ?? "ok"}`);
    await sleep(600);
    await select([part.ref]);
    const a = row("Alpha"), z = row("zeta");
    t(!!a && !!z && !!(a.compareDocumentPosition(z) & Node.DOCUMENT_POSITION_FOLLOWING), "Alpha's row comes before zeta's");
    const e = await edit("zeta", "Z2");
    const f = (await state()).parts.find((p) => p.ref === part.ref).fields.find((x) => x.name === "zeta");
    t(e.startsWith("ok") && f?.text === "Z2", `zeta = Z2 (${e})`);
    await undo();
    await undo();
    t(!(await state()).parts.find((p) => p.ref === part.ref).fields.some((x) => x.name === "zeta"), "undone");
  });

  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})().catch((e) => {
  window.__pcbPropsV16Check.results.push({ name: "the check ran", ok: false, notes: [String((e && e.stack) || e)] });
  window.__pcbPropsV16Check.done = true;
});
