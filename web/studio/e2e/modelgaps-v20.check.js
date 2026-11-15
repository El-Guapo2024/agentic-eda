// The small model gaps of batch v20 (task-v20-modelgaps; docs/parity/GAPS.md items 8 and 9, web/studio/PARITY-pcb.md section 126): a via's free flag
// (`PCB_VIA::GetIsFree`, `(free yes)`), driven the way a person does it, read back from `/api/state`.
//
// What it checks:
//   draw via     Draw Via on a filled pour of a net puts down a via of that net that is free (`VIA_PLACER::PlaceItem`: `via->SetIsFree( via->GetNetCode() > 0 )`
//                after `findStitchedZoneNet`); on bare board it is a via of no net, not free;
//   panel row    the Properties panel's "Automatically Update Net" (KiCad 25ec2335 `PCB_VIA::GetIsNotFree`) shows the flag; ticking it is one undo step;
//   dialog box   Track & Via Properties' "Automatically update via nets" (`m_viaNotFree`) shows it; ticked with no track selected it greys the Net box
//                (`onViaNotFreeClicked`); OK sets it (`via->SetIsFree( !m_viaNotFree->GetValue() )`), one undo step.
// It adds a zone and vias and undoes every one; it ends by checking the board is as it found it.
// It answers through `window.__modelgapsV20Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__modelgapsV20Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 20000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const cmd = (c) => post("/api/cmd", { cmd: c, strict: false });
  const state = () => json("/api/state");

  const container = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => container().querySelector("canvas");
  const view = () => E.state().view;
  const w2c = (wx, wy) => { const v = view(), r = canvas().getBoundingClientRect(); return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale]; };
  const fire = (type, x, y) => {
    const el = document.elementFromPoint(x, y) ?? canvas();
    el.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerdown" ? 1 : 0 }));
  };
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y);
    await sleep(200);
    fire("pointerdown", x, y);
    await sleep(40);
    fire("pointerup", x, y);
    await sleep(300);
  };
  const leaveTool = async () => { for (let i = 0; i < 3 && E.state().tool !== "select"; i++) { await E.run("common.Interactive.cancel"); await sleep(250); } };
  const select = async (ids) => { await E.run("common.InteractiveSelection.clear"); if (ids.length > 0) await E.run("common.InteractiveSelection.selectItems", ids); await sleep(400); };
  const undo = async () => { await E.run("common.Interactive.undo"); await sleep(800); };
  const buttonText = (root, text) => [...root.querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await leaveTool(); } catch { /* best effort */ }
    results.push(entry);
  };
  const tracksOf = (st) => st.routing?.tracks ?? [];
  const viasOf = (st) => st.routing?.vias ?? [];
  const zonesOf = (st) => st.routing?.zones ?? [];
  const snapshot = (st) => JSON.stringify([tracksOf(st).map((t) => [t.id, t.net, t.pts]), viasOf(st).map((v) => [v.id, v.net, v.x, v.y, !!v.free]), zonesOf(st).map((z) => [z.id, z.net, z.layer])]);
  const undoUntil = async (target) => { for (let i = 0; i < 14 && snapshot(await state()) !== target; i++) await undo(); };

  // ------------------------------------------------------------------------------------------------------------------------------------------------------------ run
  if (E && E.state().tab !== "pcb") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click(); await sleep(1500); }
  await until(() => E.state().tab === "pcb" && container() && canvas() && view()?.scale > 0 && E.state().counts.footprints > 0, 30000);
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  if (strict?.checked) { strict.click(); await sleep(300); }
  await E.run("common.Control.zoomFitScreen");
  await sleep(800);
  const st0 = await state();
  const original = snapshot(st0);
  const pads = st0.parts.filter((p) => p.placed).flatMap((p) => (p.pads ?? []).map((q) => ({ ...q, ref: p.ref })));
  const nets = [...new Set(pads.map((q) => q.net).filter((x) => !!x))];
  // two half-millimetre spots 4 mm apart, 1.5 mm inside the board outline's box, each 1.5 mm clear of every pad and 2 mm from every track point
  const far = (x, y) => pads.every((q) => Math.hypot(q.x - x, q.y - y) > Math.max(q.w ?? 0, q.h ?? 0) / 2 + 1_500_000) && tracksOf(st0).every((tr) => tr.pts.every((pt) => Math.hypot(pt[0] - x, pt[1] - y) > 2_000_000));
  const ol = (st0.outline ?? []).map((p) => [p[0] / 1e6, p[1] / 1e6]);
  const xs = (ol.length ? ol : pads.map((q) => [q.x / 1e6, q.y / 1e6])).map((p) => p[0]), ys = (ol.length ? ol : pads.map((q) => [q.x / 1e6, q.y / 1e6])).map((p) => p[1]);
  const [bx0, bx1, by0, by1] = [Math.ceil(Math.min(...xs) + 1.5), Math.floor(Math.max(...xs) - 1.5), Math.ceil(Math.min(...ys) + 1.5), Math.floor(Math.max(...ys) - 1.5)];
  let spot = null;
  for (let y = by0; y <= by1 && !spot; y += 0.5) for (let x = bx0; x + 4 <= bx1 && !spot; x += 0.5) if (far(x * 1e6, y * 1e6) && far((x + 4) * 1e6, y * 1e6)) spot = [x, y];
  let freeVia = null;

  await scenario("draw via: a via on a pour of a net is that net's and free; one on bare board is of no net and not free", async (t) => {
    if (!t(!!spot && nets.length > 0, `a free spot: ${JSON.stringify(spot)}, a net: ${nets[0]}`)) return;
    const [x0, y0] = spot;
    const net = nets[0];
    const r = 1_000_000;
    t((await cmd({ op: "add_zone", net, layer: "F.Cu", outline: [{ x: x0 * 1e6 - r, y: y0 * 1e6 - r }, { x: x0 * 1e6 + r, y: y0 * 1e6 - r }, { x: x0 * 1e6 + r, y: y0 * 1e6 + r }, { x: x0 * 1e6 - r, y: y0 * 1e6 + r }] })).ok, `a pour of ${net} round the spot`);
    const filled = await post("/api/fill/kicad", {});
    t(filled.ok === true, `kicad-cli filled it (${JSON.stringify([filled.ok, filled.message])})`);
    await sleep(1500);
    const before = new Set(viasOf(await state()).map((v) => v.id));
    await E.run("pcbnew.InteractiveDrawing.via"); await sleep(400);
    await clickAt(x0 * 1e6, y0 * 1e6);
    await until(async () => viasOf(await state()).length > before.size, 8000);
    let made = viasOf(await state()).filter((v) => !before.has(v.id));
    t(made.length === 1, `one via put down (${made.length})`);
    const v = made[0];
    t(v && v.net === net, `of the pour's net (${v && v.net})`);
    t(v && v.free === true, `free: SetIsFree( GetNetCode() > 0 ) (${v && JSON.stringify(v.free)})`);
    freeVia = v && v.id;
    const before2 = new Set(viasOf(await state()).map((x) => x.id));
    await clickAt((x0 + 4) * 1e6, y0 * 1e6);
    await until(async () => viasOf(await state()).length > before2.size, 8000);
    made = viasOf(await state()).filter((x) => !before2.has(x.id));
    t(made.length === 1 && made[0].net === "" && !made[0].free, `on bare board: of no net, not free (${JSON.stringify(made.map((x) => [x.net, x.free]))})`);
    await leaveTool();
    await undo();
    t(viasOf(await state()).length === before2.size, "Undo takes the second via away");
  });

  await scenario("panel row: Automatically Update Net is !free; ticking it is one undo step", async (t) => {
    if (!t(!!freeVia, `the free via: ${freeVia}`)) return;
    document.querySelector('button[aria-label="Show Properties"]')?.click();
    await select([freeVia]);
    const row = () => document.querySelector('.prop-row[data-prop="Automatically Update Net"]');
    const input = () => row()?.querySelector("[data-prop-input]");
    await until(() => input(), 6000);
    t(!!input() && input().type === "checkbox", `the row is a checkbox (${row() ? row().textContent.slice(0, 60) : "no row"})`);
    t(input() && input().checked === false, "unticked: the via is free");
    input()?.click();
    await until(async () => !viasOf(await state()).find((v) => v.id === freeVia)?.free, 6000);
    t(!viasOf(await state()).find((v) => v.id === freeVia)?.free, "ticked: the via follows its connections' nets");
    await undo();
    t(viasOf(await state()).find((v) => v.id === freeVia)?.free === true, "one Undo makes it free again");
    await select([]);
  });

  await scenario("dialog box: Track & Via Properties' Automatically update via nets greys the Net box and is set on OK", async (t) => {
    if (!t(!!freeVia, `the free via: ${freeVia}`)) return;
    await select([freeVia]);
    await E.run("pcbnew.InteractiveEdit.properties"); await sleep(900);
    const dlg = document.querySelector('.dialog[aria-label="Track & Via Properties"]');
    if (!t(!!dlg, "the dialog opens")) return;
    const box = dlg.querySelector('input[aria-label="Automatically update via nets"]');
    const netSel = () => dlg.querySelector('select[aria-label="Net"]');
    t(!!box && box.checked === false && !box.indeterminate, `the box is there, unticked (${box && [box.checked, box.indeterminate]})`);
    t(netSel() && !netSel().disabled, "the Net box is live");
    box?.click(); await sleep(200);
    t(box && box.checked === true, "ticked");
    t(netSel() && netSel().disabled, "ticked with no track selected: the Net box is greyed (onViaNotFreeClicked)");
    buttonText(dlg, "OK")?.click(); await sleep(1200);
    t(!viasOf(await state()).find((v) => v.id === freeVia)?.free, "OK: the via is not free any more");
    await undo();
    t(viasOf(await state()).find((v) => v.id === freeVia)?.free === true, "one Undo makes it free again");
    await select([]);
  });

  await post("/api/fill/clear", {}).catch(() => null);
  await undoUntil(original);
  results.push({ name: "the board is as it was", ok: snapshot(await state()) === original, notes: [] });
  check.ok = results.length > 0 && results.every((r) => r.ok);
  check.done = true;
})();
