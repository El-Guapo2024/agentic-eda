// Board view controls and context menu check (GAPS.md item 33, the checks P20 left uncovered; PARITY-pcb.md sections 1 and 7): the wheel zooms about
// the cursor, Ctrl+wheel pans horizontally, a middle and a right drag pan (a right drag opens no menu, a plain right click does), F1/F2 zoom in and
// out at the cursor by 1.3, Home zooms to fit, F5 is a no-op, N / Shift+N cycle the grid, Ctrl+U switches units and Space resets the local
// coordinates the status bar's dx/dy are measured from; and the right-click menu of an empty spot lists KiCad's seven entries (with no Move), its
// Zoom submenu sets the preset, and a right click on a footprint selects it and offers Move and Properties....
// Runs on a served routed board (`eda pipeline examples/mcu_board_30plus.yaml -o <p>`, `eda board serve -C <p> --port N --ui web/studio/dist`) --
// run-all-checks.sh builds it as mcu30-routed -- with the page in view. Nothing on the board is edited (the menu's Zoom preset and the view changes
// are the view's own), so a scratch copy is still the polite thing. `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/
// web/studio/e2e/board-view.check.js`, or paste it into the Browser pane's javascript tool.
// It answers through `window.__boardViewCheck`: { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }; read it with
// `await __boardViewCheck.wait(120000)`.
// run-all-checks: board=mcu30-routed minutes=10   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__boardViewCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 30000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const state = () => json("/api/state");
  const run = (id, args) => __eda.run(id, args);
  const view = () => __eda.state().view ?? { scale: 1, x: 0, y: 0 };
  const container = () => document.querySelector(".pcb-canvas-container");
  const canvas = () => container()?.querySelector("canvas");
  const rect = () => container().getBoundingClientRect();
  /** Board nm -> container-local pixels, as the painter's transform (`view.x + nm * view.scale`). */
  const w2l = (xNm, yNm) => [view().x + xNm * view().scale, view().y + yNm * view().scale];
  /** Container-local pixels -> board nm. */
  const l2w = (px, py) => [(px - view().x) / view().scale, (py - view().y) / view().scale];
  const fire = (type, lx, ly, extra = {}) => canvas().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 7, pointerType: "mouse", isPrimary: true, clientX: rect().left + lx, clientY: rect().top + ly, button: 0, buttons: type === "pointerdown" || type === "pointermove" ? extra.buttons ?? 0 : 0, ...extra }));
  const wheel = async (lx, ly, dy, extra = {}) => { canvas().dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, clientX: rect().left + lx, clientY: rect().top + ly, deltaY: dy, deltaMode: 0, ...extra })); await sleep(200); };
  const key = (k, extra = {}) => document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...extra }));
  const status = () => document.querySelector(".status-bar")?.innerText.replace(/\s+/g, " ") ?? "";
  const near = (a, b, eps) => Math.abs(a - b) <= eps;
  /** The context menu: its direct children, an item's label (a submenu's too), and a submenu's own items. */
  const menu = () => container()?.querySelector(".menubar-dropdown") ?? null;
  const labelOf = (el) => el.classList.contains("menu-node-submenu") ? el.querySelector(":scope > .menu-node-label span")?.textContent.trim() : el.querySelector(":scope > span")?.textContent.replace(/^✓\s*/, "").trim();
  const entries = () => [...(menu()?.children ?? [])].map((c) => c.classList.contains("menu-separator") ? null : labelOf(c)).filter(Boolean);
  const submenu = (label) => [...(menu()?.children ?? [])].find((c) => c.classList.contains("menu-node-submenu") && labelOf(c) === label);
  const submenuLabels = (label) => [...(submenu(label)?.querySelectorAll(":scope > .menubar-dropdown > .menu-node-item") ?? [])].map((el) => el.querySelector(":scope > span")?.textContent.replace(/^✓\s*/, "").trim());
  const rightClick = (lx, ly) => canvas().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: rect().left + lx, clientY: rect().top + ly, button: 2 }));
  const closeMenu = async () => { key("Escape"); await sleep(150); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await closeMenu(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const errorsFrom = Date.now();
  const st = await state();
  if (!st.routing?.tracks?.length) { results.push({ name: "setup", ok: false, notes: ["FAIL: the board has no tracks and no view to work on (eda pipeline mcu_board_30plus)"] }); check.done = true; return "no board"; }
  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll(".editor-tabs .editor-tab")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  await run("common.Control.zoomFitObjects");
  await until(() => view().scale > 0, 5000);

  (async () => {
    await scenario("Wheel zoom keeps the cursor's point fixed; Ctrl+wheel pans horizontally; middle and right drags pan, the right one opening no menu", async (t) => {
      await run("common.Control.zoomFitObjects"); await sleep(300);
      const r = rect();
      const lx = Math.round(r.width / 2 + 40), ly = Math.round(r.height / 2 - 30);
      const v0 = view();
      const world0 = l2w(lx, ly);
      await wheel(lx, ly, -120);
      const v1 = view();
      if (!t(v1.scale > v0.scale, `one wheel notch up zooms in (${v0.scale} -> ${v1.scale})`)) return;
      const world1 = l2w(lx, ly);
      t(near(world1[0], world0[0], 2000) && near(world1[1], world0[1], 2000), `the point under the cursor stays put (${(world0[0] / 1e6).toFixed(4)}, ${(world0[1] / 1e6).toFixed(4)} -> ${(world1[0] / 1e6).toFixed(4)}, ${(world1[1] / 1e6).toFixed(4)} mm)`);
      const v2 = view();
      await wheel(lx, ly, 120, { ctrlKey: true });
      const v3 = view();
      t(near(v3.scale, v2.scale, 1e-12) && !near(v3.x, v2.x, 0.01) && near(v3.y, v2.y, 0.01), `Ctrl+wheel pans horizontally and keeps the zoom (x ${v2.x.toFixed(2)} -> ${v3.x.toFixed(2)}, y ${v2.y.toFixed(2)}, scale ${v3.scale})`);
      // Middle-button drag: PAN, by the drag's pixel delta.
      const v4 = view();
      fire("pointerdown", lx, ly, { button: 1, buttons: 4 });
      await sleep(50);
      fire("pointermove", lx + 40, ly + 25, { buttons: 4 });
      await sleep(50);
      fire("pointerup", lx + 40, ly + 25, { button: 1 });
      await sleep(200);
      const v5 = view();
      t(near(v5.x - v4.x, 40, 2) && near(v5.y - v4.y, 25, 2) && near(v5.scale, v4.scale, 1e-12), `a middle drag pans by its delta (${(v5.x - v4.x).toFixed(1)}, ${(v5.y - v4.y).toFixed(1)} px)`);
      // Right-button drag: PAN too, and the context menu the browser would fire afterwards is suppressed.
      const v6 = view();
      fire("pointerdown", lx, ly, { button: 2, buttons: 2 });
      await sleep(50);
      fire("pointermove", lx - 55, ly + 10, { buttons: 2 });
      await sleep(50);
      fire("pointerup", lx - 55, ly + 10, { button: 2 });
      await sleep(100);
      t(near(view().x - v6.x, -55, 2) && near(view().y - v6.y, 10, 2), `a right drag pans by its delta (${(view().x - v6.x).toFixed(1)}, ${(view().y - v6.y).toFixed(1)} px)`);
      rightClick(lx, ly);
      await sleep(250);
      t(menu() === null, "and the right click at the end of that drag opens no menu");
      await closeMenu();
    });

    await scenario("F1 and F2 zoom in and out at the cursor by 1.3, Home fits the board, F5 does nothing", async (t) => {
      await run("common.Control.zoomFitObjects"); await sleep(300);
      const r = rect();
      const lx = Math.round(r.width / 3), ly = Math.round(r.height / 3);
      fire("pointermove", lx, ly);
      await sleep(200);
      const v0 = view();
      const world0 = l2w(lx, ly);
      key("F1");
      await sleep(250);
      const v1 = view();
      t(near(v1.scale / v0.scale, 1.3, 0.001), `F1 zooms in by 1.3 (${(v1.scale / v0.scale).toFixed(4)})`);
      const world1 = l2w(lx, ly);
      t(near(world1[0], world0[0], 2000) && near(world1[1], world0[1], 2000), "anchored at the cursor");
      key("F2");
      await sleep(250);
      const v2 = view();
      t(near(v2.scale / v1.scale, 1 / 1.3, 0.001), `F2 zooms back out by 1/1.3 (${(v2.scale / v1.scale).toFixed(4)})`);
      // Home: the board outline fits the canvas, whatever the zoom was.
      key("F1"); await sleep(150); key("F1"); await sleep(250);
      key("Home");
      await sleep(300);
      const v3 = view();
      const pts = (st.outline_polys ?? []).flatMap((p) => [p.outer, ...p.holes].flat());
      const box = pts.length ? pts : (st.outline ?? []);
      t(box.length > 0, `the board has an outline to fit (${box.length} point(s))`);
      const inside = box.every(([x, y]) => { const [sx, sy] = w2l(x, y); return sx >= -1 && sy >= -1 && sx <= r.width + 1 && sy <= r.height + 1; });
      t(inside, "Home puts every corner of the outline on the canvas");
      t(v3.scale < v2.scale * 1.3, `and it zooms out from the zoomed-in view (${v2.scale} -> ${v3.scale})`);
      const v4 = view();
      key("F5");
      await sleep(200);
      t(JSON.stringify(view()) === JSON.stringify(v4), "F5 is a true no-op");
    });

    await scenario("N and Shift+N cycle the grid, Ctrl+U switches units, and the status bar's X/Y and dx/dy follow the pointer and Space", async (t) => {
      const g0 = __eda.state().grid;
      key("n");
      await sleep(150);
      const g1 = __eda.state().grid;
      t(g1 !== g0, `N goes to the next grid (${(g0 / 1e6).toFixed(3)} -> ${(g1 / 1e6).toFixed(3)} mm)`);
      key("N", { shiftKey: true });
      await sleep(150);
      t(__eda.state().grid === g0, `Shift+N goes back (${(__eda.state().grid / 1e6).toFixed(3)} mm)`);
      const u0 = __eda.state().units;
      key("u", { ctrlKey: true });
      await sleep(150);
      const u1 = __eda.state().units;
      t(u1 !== u0, `Ctrl+U switches units (${u0} -> ${u1})`);
      t(document.querySelector(".status-bar select")?.value === u1, "the status bar's unit picker follows");
      for (let i = 0; i < 3 && __eda.state().units !== u0; i++) { key("u", { ctrlKey: true }); await sleep(150); }
      t(__eda.state().units === u0, `back to ${u0}`);
      // The status bar's X/Y is the pointer's board point; dx/dy are measured from the Space origin.
      const r = rect();
      const lx = Math.round(r.width / 2), ly = Math.round(r.height / 2);
      const [wx, wy] = l2w(lx, ly);
      fire("pointermove", lx, ly);
      await sleep(250);
      // `formatXY` prints the unit's decimals (`state/units.ts` DECIMALS): 3 for mm, 1 for mil, 4 for in.
      const digits = { mm: 3, mil: 1, in: 4 }[u0] ?? 3;
      const xy = new RegExp(`${(wx / 1e6).toFixed(digits)}, ${(wy / 1e6).toFixed(digits)} ${u0}`).test(status());
      t(xy, `the status bar shows the pointer at ${(wx / 1e6).toFixed(digits)}, ${(wy / 1e6).toFixed(digits)} ${u0} (${status().slice(0, 120)})`);
      key(" ", { code: "Space" });
      await sleep(200);
      const lx2 = lx + 60, ly2 = ly + 30;
      const [wx2, wy2] = l2w(lx2, ly2);
      fire("pointermove", lx2, ly2);
      await sleep(250);
      // `status()` collapses every run of whitespace to one space, so the fields are separated by single spaces here.
      const m = /dx (-?[\d.]+) (\S+) dy (-?[\d.]+) (\S+) dist (-?[\d.]+)/.exec(status());
      const dxmm = wx2 / 1e6 - wx / 1e6, dymm = wy2 / 1e6 - wy / 1e6;
      t(m && near(Number(m[1]) / (m[2] === "mm" ? 1 : m[2] === "mil" ? 0.0254 : 25.4), dxmm, 0.001) && near(Number(m[3]) / (m[4] === "mm" ? 1 : m[4] === "mil" ? 0.0254 : 25.4), dymm, 0.001), `dx/dy are measured from the Space point (${m?.[0]}; expected ${dxmm.toFixed(4)}, ${dymm.toFixed(4)} mm)`);
      const dist = Math.hypot(dxmm, dymm);
      t(m && near(Number(m[5]) / (m[4] === "mm" ? 1 : m[4] === "mil" ? 0.0254 : 25.4), dist, 0.001), `and dist is their length (${m?.[5]} ${m?.[4]}; expected ${dist.toFixed(4)} mm)`);
    });

    await scenario("An empty spot's right-click menu is KiCad's seven entries, and its Zoom submenu sets the preset", async (t) => {
      await run("common.Interactive.unselectAll");
      await run("common.Control.zoomFitObjects"); await sleep(300);
      const r = rect();
      const lx = 8, ly = 8; // a corner of the canvas: outside the fitted board's margin
      rightClick(lx, ly);
      const opened = await until(() => menu(), 3000);
      if (!t(opened, "a right click on an empty spot opens the menu")) return;
      const labels = entries();
      const want = ["Select All", "Unselect All", "Get and Move Footprint", "Paste", "Paste Special...", "Zoom", "Grid"];
      t(JSON.stringify(labels) === JSON.stringify(want), `it lists KiCad's entries in order (${JSON.stringify(labels)})`);
      t(!labels.includes("Move") && !labels.includes("Properties..."), "and nothing that needs a selection");
      const zooms = submenuLabels("Zoom");
      t(zooms.length > 0 && zooms.every((z) => /^Zoom: \d+\.\d\d$/.test(z)), `the Zoom submenu lists the presets (${zooms.join(", ")})`);
      const checked = [...(submenu("Zoom")?.querySelectorAll(":scope > .menubar-dropdown > .menu-node-item") ?? [])].filter((el) => el.getAttribute("aria-checked") === "true");
      t(checked.length === 1, `exactly one preset is checked (${checked.map((el) => labelOf(el)).join(", ")})`);
      const item = [...(submenu("Zoom")?.querySelectorAll(":scope > .menubar-dropdown > .menu-node-item") ?? [])].find((el) => labelOf(el) === "Zoom: 50.00");
      if (!t(item, "Zoom: 50.00 is there")) return;
      item.click();
      const z50 = 50 * 91 / 25_400_000;
      const settled = await until(() => near(view().scale, z50, 1e-9), 5000);
      t(settled, `clicking it sets the 50.00 zoom (${view().scale} vs ${z50})`);
      t(menu() === null, "and the menu closes");
      rightClick(lx, ly);
      await until(() => menu(), 3000);
      await closeMenu();
      t(menu() === null, "Escape closes it");
    });

    await scenario("A right click on a footprint selects it and offers Move and Properties...", async (t) => {
      await run("common.Interactive.unselectAll");
      await run("common.Control.zoomFitObjects"); await sleep(300);
      const part = st.parts.find((p) => p.placed && p.at);
      if (!t(part, "a placed footprint to right-click")) return;
      const [lx, ly] = w2l(part.at[0], part.at[1]);
      rightClick(lx, ly);
      const opened = await until(() => menu(), 3000);
      if (!t(opened, `the menu opens on ${part.ref}`)) return;
      const labels = entries();
      await until(() => __eda.state().selection.length > 0, 2000);
      t(__eda.state().selection.some((s) => s.id === part.ref), `the click selected ${part.ref} (${JSON.stringify(__eda.state().selection)})`);
      t(labels.includes("Move"), `Move is offered (${labels.slice(0, 12).join(", ")}...)`);
      t(labels.includes("Properties..."), "and Properties...");
      t(labels.includes("Select All") && !labels.includes("Select All only on the empty menu"), "the standard entries are still there");
      await closeMenu();
      t(menu() === null, "Escape closes it");
    });

    await scenario("no error was logged", async (t) => {
      const errs = __eda.errors(errorsFrom);
      t(errs.length === 0, `no errors: ${JSON.stringify(errs.slice(0, 3))}`);
    });
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
