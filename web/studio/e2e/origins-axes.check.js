// Origins & Axes in the board editor (UI-LEFT-2 PP9; PARITY-pcb.md section 98): Preferences > PCB Editor > Origins & Axes (`PANEL_PCBNEW_DISPLAY_ORIGIN`) and
// `PCB_ORIGIN_TRANSFORMS` wherever the board editor shows or takes a coordinate. It sets the board's drill/place origin to (10, 20) mm and its grid origin to
// (5, 7) mm (undone at the end), then drives the Preferences page itself and reads:
//   page          the page's rows (Display Origin, X Axis, Y Axis with KiCad's labels), KiCad's defaults (page origin, right, down), OK keeps a choice, the reset
//                 button puts the defaults back
//   status        the status bar's X and Y at one pointer position from the page, the drill/place origin with X increasing left, the grid origin with Y increasing
//                 up; dx and dy from the local origin (Space) turned by the axes; polar coordinates from the local origin, untransformed
//   properties    a footprint's Position X / Y rows in the frame chosen, and a value typed there lands where it means
//   search        the Search panel's X and Y cells of a footprint
//   position      Position Relative's offsets from the grid origin with Y increasing up; Move Exactly's Move X with X increasing left
//   dialogs       Footprint Properties' Position X / Y, typed and read back; Grid Origin's X and Y (the grid origin as the display origin shows as 0, 0)
// Every edit is undone; no console errors. It answers through `window.__originsAxesCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const check = (window.__originsAxesCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 20000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const state = () => json("/api/state");
  const cmd = (c) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) });
  const run = (id, args) => __eda.run(id, args);
  const MM = 1_000_000;
  const near = (a, b, tol = 0.0011) => Math.abs(a - b) <= tol;

  const dialog = () => document.querySelector(".dialog");
  const buttonIn = (root, re) => [...(root?.querySelectorAll("button") ?? [])].find((b) => re.test(b.textContent.trim()));
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    el.focus?.();
    Object.getOwnPropertyDescriptor(el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
    await sleep(120);
  };
  const closeDialogs = async () => {
    for (let i = 0; i < 4 && dialog(); i++) {
      (buttonIn(dialog(), /^Cancel$/) ?? buttonIn(dialog(), /^Close$/))?.click();
      await sleep(250);
    }
  };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };

  // The Preferences dialog, driven as a user does: Preferences..., the page in the tree, the radios, OK.
  const prefsDialog = () => document.querySelector('[data-testid="preferences-dialog"]');
  const openAxesPage = async () => {
    await run("common.SuiteControl.openPreferences");
    if (!(await until(() => prefsDialog(), 8000))) throw new Error("Preferences did not open");
    document.querySelector('[data-testid="prefs-page-pcb_axes"]')?.click();
    return until(() => document.querySelector('[data-testid="pcb-prefs-axes"]'), 5000);
  };
  const radio = (id) => document.querySelector(`[data-testid="pcb-prefs-${id}"]`);
  const setAxes = async (origin, invertX, invertY) => {
    if (!(await openAxesPage())) throw new Error("no Origins & Axes page");
    radio(`origin-${origin}`).click();
    radio(invertX ? "x-left" : "x-right").click();
    radio(invertY ? "y-up" : "y-down").click();
    await sleep(100);
    document.querySelector('[data-testid="preferences-ok"]').click();
    await until(() => !prefsDialog(), 5000);
    await sleep(300);
  };

  // The canvas: the pointer moves a user makes, and the status bar's fields.
  const canvasBox = () => document.querySelector(".pcb-canvas-container");
  const fire = (type, x, y) => {
    const box = canvasBox();
    const rect = box.getBoundingClientRect();
    (box.querySelector("canvas") || box).dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect.left + x, clientY: rect.top + y, button: 0, buttons: 0 }));
  };
  const statusText = () => document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? "";
  const readout = () => { const m = /Z [\d.]+ (-?[\d.]+), (-?[\d.]+) mm/.exec(statusText()); return m ? [Number(m[1]), Number(m[2])] : null; };
  const deltas = () => { const m = /dx (-?[\d.]+) mm dy (-?[\d.]+) mm dist (-?[\d.]+) mm/.exec(statusText()); return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null; };
  const polarText = () => { const m = /r (-?[\d.]+) mm theta (-?[\d.]+)/.exec(statusText()); return m ? [Number(m[1]), Number(m[2])] : null; };
  const at = async (x, y) => { fire("pointermove", x, y); await sleep(250); };

  const select = async (ids) => {
    await run("common.InteractiveSelection.clear");
    await run("common.InteractiveSelection.selectItems", ids);
    await sleep(350);
  };
  const partAt = async (ref) => (await state()).parts.find((p) => p.ref === ref)?.at ?? null;
  const undo = async () => { await run("common.Interactive.undo"); await sleep(500); };

  // ------------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = __eda.errors().length;
  const st0 = await state();
  const placed = st0.parts.filter((p) => p.placed && p.at);
  if (placed.length === 0) { results.push({ name: "setup", ok: false, notes: ["FAIL: no placed footprint (run on mcu30-placed)"] }); check.done = true; return; }
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll(".editor-tabs .editor-tab")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const ref = placed.find((p) => p.ref === "U1") ? "U1" : placed[0].ref;
  // Strict mode refuses a move that adds placement failures: off for the moves here, as it was at the end
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  // the two origins the Display Origin can count from
  const AUX = [10, 20];
  const GRID = [5, 7];
  await cmd({ op: "set_aux_origin", at: { x: AUX[0] * MM, y: AUX[1] * MM } });
  await cmd({ op: "set_grid_origin", at: { x: GRID[0] * MM, y: GRID[1] * MM } });
  await until(async () => { const s = await state(); return s.aux_origin?.[0] === AUX[0] * MM && s.grid_origin?.[0] === GRID[0] * MM; });
  await sleep(600);

  (async () => {
    await scenario("Preferences > PCB Editor > Origins & Axes: KiCad's rows and defaults, OK keeps the choice, Reset puts the defaults back", async (t) => {
      const page = await openAxesPage();
      if (!t(!!page, "the PCB Editor's tree has Origins & Axes, after Grids")) return;
      const tree = [...document.querySelectorAll('[role="tree"] [role="treeitem"]')].map((e) => e.textContent.trim());
      const pcb = tree.indexOf("PCB Editor");
      t(tree.slice(pcb + 1, pcb + 5).join("|") === "Display Options|Grids|Origins & Axes|Editing Options", `the order (${tree.slice(pcb + 1, pcb + 5).join("|")})`);
      const text = page.innerText.replace(/\s+/g, " ");
      t(/Display Origin Page origin Drill\/place file origin Grid origin X Axis Increases right Increases left Y Axis Increases up Increases down/.test(text), `the labels are KiCad's (${text})`);
      t(radio("origin-page").checked && radio("x-right").checked && radio("y-down").checked, "page origin, X right and Y down to begin with (PCBNEW_SETTINGS' defaults)");
      t(/Reset Origins & Axes to Defaults/.test(document.querySelector('[data-testid="preferences-reset"]')?.textContent ?? ""), "the page can be reset");
      radio("origin-grid").click();
      radio("y-up").click();
      await sleep(100);
      document.querySelector('[data-testid="preferences-ok"]').click();
      await until(() => !prefsDialog(), 5000);
      await openAxesPage();
      t(radio("origin-grid").checked && radio("y-up").checked && radio("x-right").checked, "reopened, the page shows what OK kept");
      document.querySelector('[data-testid="preferences-reset"]').click();
      await sleep(150);
      t(radio("origin-page").checked && radio("y-down").checked, "Reset: the page origin and Y down again");
      document.querySelector('[data-testid="preferences-ok"]').click();
      await until(() => !prefsDialog(), 5000);
      const saved = JSON.parse(localStorage.getItem("eda-studio.preferences.v1") ?? "{}");
      t(saved.pcbDisplayOrigin === "page" && saved.pcbInvertYAxis === false, `kept as a preference of the browser, not the design (${saved.pcbDisplayOrigin}, ${saved.pcbInvertYAxis})`);
    });

    await scenario("the status bar: X and Y from the chosen origin, each axis either way; dx and dy turned by the axes; polar from the local origin", async (t) => {
      await run("common.Control.zoomFitObjects");
      await sleep(400);
      await setAxes("page", false, false);
      await at(320, 260);
      const p0 = readout();
      if (!t(!!p0, `the readout at the page origin (${statusText()})`)) return;
      await setAxes("aux", true, false);
      await at(320, 260);
      const p1 = readout();
      t(p1 && near(p1[0], -(p0[0] - AUX[0])) && near(p1[1], p0[1] - AUX[1]), `from the drill/place origin, X increasing left: (${p1}) for (${p0})`);
      await setAxes("grid", false, true);
      await at(320, 260);
      const p2 = readout();
      t(p2 && near(p2[0], p0[0] - GRID[0]) && near(p2[1], -(p0[1] - GRID[1])), `from the grid origin, Y increasing up: (${p2}) for (${p0})`);
      // dx and dy: Space puts the local origin under the cursor
      await setAxes("page", false, false);
      await at(200, 200);
      await run("common.Control.resetLocalCoords");
      await sleep(200);
      await at(380, 320);
      const d0 = deltas();
      if (!t(!!d0 && (d0[0] !== 0 || d0[1] !== 0), `dx and dy from the local origin (${statusText()})`)) return;
      await setAxes("aux", true, true);
      await at(380, 320);
      const d1 = deltas();
      t(d1 && near(d1[0], -d0[0]) && near(d1[1], -d0[1]) && near(d1[2], d0[2]), `both axes the other way: dx ${d1?.[0]}, dy ${d1?.[1]} for ${d0[0]}, ${d0[1]}; the distance as it was`);
      // polar coordinates: field 2 keeps X and Y, field 3 is r and theta of the cursor from the local origin, untransformed
      const polarBox = [...document.querySelectorAll(".status-bar label.toggle")].find((l) => /polar/.test(l.textContent))?.querySelector("input");
      polarBox?.click();
      await sleep(200);
      await at(380, 320);
      const pt = polarText();
      const theta = (Math.atan2(-d0[1], d0[0]) * 180) / Math.PI;
      t(pt && near(pt[0], d0[2]) && Math.abs(pt[1] - theta) < 0.01, `"r .. theta .." (${pt}) is the page-frame vector's (${d0[2]}, ${theta.toFixed(3)})`);
      t(!!readout(), `field 2 still shows X and Y with polar coordinates on (${statusText()})`);
      polarBox?.click();
      await sleep(150);
    });

    await scenario("the Properties panel: Position X / Y in the chosen frame, and a value typed there lands where it means", async (t) => {
      await setAxes("aux", true, false);
      await select([ref]);
      const atBefore = await partAt(ref);
      const input = (name) => document.querySelector(`.prop-row[data-prop="${name}"] [data-prop-input]`);
      if (!t(!!input("Position X"), "the footprint's Position X row")) return;
      const shownX = parseFloat(input("Position X").value);
      const shownY = parseFloat(input("Position Y").value);
      t(near(shownX, -(atBefore[0] / MM - AUX[0])) && near(shownY, atBefore[1] / MM - AUX[1]), `Position X / Y show (${shownX}, ${shownY}) for (${atBefore.map((v) => v / MM)}) from the drill/place origin, X increasing left`);
      const el = input("Position X");
      el.focus();
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, "-3");
      el.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(40);
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      const moved = await until(async () => { const a = await partAt(ref); return a && a[0] !== atBefore[0] ? a : null; }, 10000);
      t(moved && near(moved[0] / MM, AUX[0] + 3, 1e-6) && moved[1] === atBefore[1], `-3 in that frame is X = ${AUX[0] + 3} mm on the board (${moved?.map((v) => v / MM)})`);
      if (moved) await undo();
      const back = await partAt(ref);
      t(back && back[0] === atBefore[0], "one undo puts it back");
    });

    await scenario("the Search panel's X and Y cells count from the chosen origin", async (t) => {
      await setAxes("grid", true, true);
      if (!document.querySelector('[data-pane="search"]')) await run("common.Interactive.search");
      await until(() => document.querySelector('[data-testid="search-pane"]'), 5000);
      document.querySelector('[data-testid="search-pane-tab-footprints"]')?.click();
      const q = document.querySelector('[data-testid="search-pane-query"]');
      await setValue(q, ref);
      await sleep(300);
      const heads = [...document.querySelectorAll('[data-testid^="search-pane-col-"]')].map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
      const row = [...document.querySelectorAll('[data-testid="search-pane-row"]')].find((r) => r.dataset.id === ref);
      if (!t(!!row, `the footprint's row (${heads.join("|")})`)) return;
      const cells = [...row.querySelectorAll("td")].map((c) => c.textContent);
      const a = await partAt(ref);
      const want = [-(a[0] / MM - GRID[0]), -(a[1] / MM - GRID[1])].map((v) => `${(v === 0 ? 0 : v).toFixed(4)} mm`);
      t(cells[heads.indexOf("X")] === want[0] && cells[heads.indexOf("Y")] === want[1], `X ${cells[heads.indexOf("X")]}, Y ${cells[heads.indexOf("Y")]} (want ${want.join(", ")})`);
      await setValue(q, "");
      await run("common.Interactive.search");
      await sleep(300);
    });

    await scenario("Position Relative's offsets with Y increasing up; Move Exactly's Move X with X increasing left", async (t) => {
      await setAxes("grid", false, true);
      await select([ref]);
      const a0 = await partAt(ref);
      await run("pcbnew.PositionRelative.positionRelative");
      const d = await until(() => (dialog()?.textContent.includes("Position Relative To") ? dialog() : null), 5000);
      if (!t(!!d, "Position Relative To... opens")) return;
      [...d.querySelectorAll("label")].find((l) => /Grid origin/.test(l.textContent))?.querySelector("input")?.click();
      await sleep(100);
      await setValue(d.querySelector('[aria-label="Offset X"]'), 2);
      await setValue(d.querySelector('[aria-label="Offset Y"]'), 3);
      buttonIn(d, /^OK$/).click();
      const moved = await until(async () => { const a = await partAt(ref); return a && (a[0] !== a0[0] || a[1] !== a0[1]) ? a : null; }, 10000);
      t(moved && near(moved[0] / MM, GRID[0] + 2, 1e-6) && near(moved[1] / MM, GRID[1] - 3, 1e-6), `2, 3 from the grid origin with Y up lands at (${moved?.map((v) => v / MM)}), want (${GRID[0] + 2}, ${GRID[1] - 3})`);
      if (moved) await undo();
      await setAxes("page", true, false);
      await select([ref]);
      await run("pcbnew.InteractiveEdit.moveExact");
      const m = await until(() => (dialog()?.textContent.includes("Move Exactly") ? dialog() : null), 5000);
      if (!t(!!m, "Move Exactly... opens")) return;
      await setValue(m.querySelector('[aria-label="Move X"]'), 1);
      await setValue(m.querySelector('[aria-label="Move Y"]'), 1);
      buttonIn(m, /^OK$/).click();
      const moved2 = await until(async () => { const a = await partAt(ref); return a && (a[0] !== a0[0] || a[1] !== a0[1]) ? a : null; }, 10000);
      t(moved2 && moved2[0] - a0[0] === -MM && moved2[1] - a0[1] === MM, `Move X 1 with X increasing left moves by (${moved2 ? [(moved2[0] - a0[0]) / MM, (moved2[1] - a0[1]) / MM] : null}) mm, want (-1, 1)`);
      if (moved2) await undo();
      const back = await partAt(ref);
      t(back && back[0] === a0[0] && back[1] === a0[1], "both undone");
    });

    await scenario("Footprint Properties' Position X / Y and Grid Origin's X / Y are in the chosen frame", async (t) => {
      await setAxes("aux", true, true);
      await select([ref]);
      const a0 = await partAt(ref);
      await run("pcbnew.InteractiveEdit.properties");
      const d = await until(() => dialog()?.querySelector('[aria-label="Position X"]') ? dialog() : null, 6000);
      if (!t(!!d, "Footprint Properties opens with Position X")) return;
      const sx = parseFloat(d.querySelector('[aria-label="Position X"]').value);
      const sy = parseFloat(d.querySelector('[aria-label="Position Y"]').value);
      t(near(sx, -(a0[0] / MM - AUX[0])) && near(sy, -(a0[1] / MM - AUX[1])), `Position X / Y show (${sx}, ${sy}) for (${a0.map((v) => v / MM)}) from the drill/place origin, both axes the other way`);
      await setValue(d.querySelector('[aria-label="Position X"]'), 0);
      buttonIn(d, /^OK$/).click();
      const moved = await until(async () => { const a = await partAt(ref); return a && a[0] !== a0[0] ? a : null; }, 10000);
      t(moved && moved[0] === AUX[0] * MM && moved[1] === a0[1], `0 there is the drill/place origin's X (${moved?.map((v) => v / MM)})`);
      if (moved) await undo();
      await setAxes("grid", false, false);
      await run("common.Control.editGridOrigin");
      const g = await until(() => document.querySelector('.dialog[aria-label="Grid Origin"]'), 5000);
      if (!t(!!g, "Grid Origin... opens")) return;
      const gx = parseFloat(g.querySelector('[aria-label="X"]').value);
      const gy = parseFloat(g.querySelector('[aria-label="Y"]').value);
      t(gx === 0 && gy === 0, `the grid origin counted from itself is (${gx}, ${gy})`);
      buttonIn(g, /^Cancel$/).click();
      await sleep(200);
      await setAxes("page", false, false);
      await run("common.Control.editGridOrigin");
      const g2 = await until(() => document.querySelector('.dialog[aria-label="Grid Origin"]'), 5000);
      t(g2 && parseFloat(g2.querySelector('[aria-label="X"]').value) === GRID[0] && parseFloat(g2.querySelector('[aria-label="Y"]').value) === GRID[1], "from the page it is where it is");
    });

    await scenario("everything undone and the defaults back; no console errors", async (t) => {
      await setAxes("page", false, false);
      // the two origins this check set
      await undo();
      await undo();
      const s = await state();
      t(!s.aux_origin && !s.grid_origin, `the origins are back to (0, 0) (${JSON.stringify(s.aux_origin)}, ${JSON.stringify(s.grid_origin)})`);
      const errs = __eda.errors().slice(errorsFrom);
      t(errs.length === 0, `no console errors (${errs.map((e) => e.message).join(" | ").slice(0, 400)})`);
      if (strictWas && strict && !strict.checked) strict.click();
    });

    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
})();
