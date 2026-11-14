// UI-LEFT-3 P3-37, S3-34 (PARITY-common "docking"): the movable panes docked as wxAUI's `DoDrop` docks them, the board's Search pane's Nets tab, the schematic's
// Search pane docked and its columns, and Find reaching every sheet's ERC markers. It drives the panes' captions, sashes and headings with the pointer and reads the
// layout back from the page and from what the browser keeps:
//   edge        Search's caption dragged: the pane floats at once; let go at the editor's left edge it docks there on a layer of its own (a ring round the
//               editor, outside the Properties column), the hint saying so first
//   stacked     the Net Inspector dropped on Search's lower half shares its dock, under it; the sash between them sizes both
//   rows        the Net Inspector dropped by Search's outer edge: a dock of its own, outside Search's
//   canvas      Search by the canvas's bottom edge docks under the canvas (layer 0); by the options toolbar along the edge, outside the Net Inspector; in the
//               middle of the canvas it floats; a double click docks it back where it was
//   nets        the Nets tab lists the lone pins' `unconnected-(..)` nets
//   sch-dock    the schematic's Search pane docks at the editor's right edge and back under the canvas, in a layout of its own (the board's untouched)
//   sch-columns its headings: one dragged to the front (the click that ends the drag does not sort), an edge dragged, a double click on an edge fits it
//   sch-markers a Find over every sheet from the root reaches an ERC marker of another sheet and goes there (runs kicad-cli's ERC)
// It answers through `window.__dockingCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=10
(async () => {
  const results = [];
  const run = (window.__dockingCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const until = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(150); } };
  const click = async (el, wait = 300) => { if (!el) throw new Error("nothing to click"); el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); await sleep(wait); };
  const dbl = async (el, wait = 400) => { if (!el) throw new Error("nothing to double-click"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true })); await sleep(wait); };
  const ptr = (target, type, x, y) => target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons: type === "pointerup" ? 0 : 1, pointerId: 1, isPrimary: true }));
  /** A press on `el` at (`x0`, `y0`), moves to (`x1`, `y1`), `before` called before the release there; `clickOn` gets the click a browser sends after it. */
  const drag = async (el, x0, y0, x1, y1, before, clickOn) => {
    ptr(el, "pointerdown", x0, y0);
    for (let i = 1; i <= 8; i++) { ptr(window, "pointermove", x0 + ((x1 - x0) * i) / 8, y0 + ((y1 - y0) * i) / 8); await sleep(40); }
    await sleep(150);
    if (before) await before();
    ptr(window, "pointerup", x1, y1);
    if (clickOn) clickOn.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x1, clientY: y1 }));
    await sleep(450);
  };
  const box = (el) => el?.getBoundingClientRect();
  const pane = (id) => document.querySelector(`[data-pane="${id}"]`);
  const caption = (id) => pane(id)?.querySelector(".dock-panel-caption");
  /** Drag pane `id` by its caption to (`x`, `y`); `before` sees the page before the release. */
  const dragPane = async (id, x, y, before) => { const r = box(caption(id)); await drag(caption(id), r.left + 40, r.top + 6, x, y, before); };
  const holder = (id) => document.querySelector(`[data-dock-pane="${id}"]`);
  const areaOf = (id) => holder(id)?.closest("[data-dock-layer][data-dock-editor]");
  const rowOf = (id) => holder(id)?.closest(".pane-dock-row");
  const hint = () => document.querySelector('[data-testid="pcb-pane-dock-hint"]');
  const moveBtn = (id, to) => pane(id)?.querySelector(`.pcb-pane-move[data-dock-to="${to}"]`);
  const kept = (editor) => JSON.parse(localStorage.getItem(`eda-studio.${editor}-pane-dock.v1`) ?? "null");
  const canvas = () => box(document.querySelector(".app-body .canvas-col"));
  const frameMidY = () => { const a = box(document.querySelector(".app-frame .main-toolbar-row")); const b = box(document.querySelector(".app-frame .message-panel-row")); return (a.top + b.bottom) / 2; };
  const tabTo = async (label) => { [...document.querySelectorAll(".editor-tab, button")].find((b) => b.textContent.trim() === label)?.click(); await sleep(1200); };
  const setValue = (el, v) => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v)); el.dispatchEvent(new Event("input", { bubbles: true })); };

  for (const k of ["eda-studio.pcb-pane-dock.v1", "eda-studio.sch-pane-dock.v1", "eda-studio.search-pane.v1", "eda-studio.dock-layout.v1"]) localStorage.removeItem(k);
  const errorsBefore = __eda.errors().length;
  if (__eda.state().tab !== "pcb") await tabTo("PCB");

  await scenario("a caption dragged floats the pane at once; let go at the editor's left edge it docks on a layer of its own, round the editor", async (t) => {
    await __eda.run("pcbnew.Control.showNetInspector");
    await __eda.run("common.Interactive.search");
    await sleep(900);
    t(areaOf("search")?.dataset.testid === "bottom-dock" && areaOf("netInspector")?.dataset.testid === "bottom-dock", "both start under the canvas");
    const c = canvas();
    const r = box(caption("search"));
    ptr(caption("search"), "pointerdown", r.left + 40, r.top + 6);
    for (let i = 1; i <= 4; i++) { ptr(window, "pointermove", r.left + 40 + i * 30, r.top + 6 - i * 30); await sleep(40); }
    await sleep(250);
    t(!!document.querySelector('[data-testid="pcb-pane-floating-search"]') && !areaOf("search"), "it left its dock as a floating window the moment the caption was dragged (Float())");
    const y = frameMidY();
    for (let i = 1; i <= 6; i++) { ptr(window, "pointermove", r.left + 160 - ((r.left + 158) * i) / 6, r.top - 114 + ((y - r.top + 114) * i) / 6); await sleep(40); }
    await sleep(250);
    t(hint()?.dataset.how === "edge" && hint()?.dataset.side === "left", `the hint: the editor's left edge (${hint()?.dataset.how} ${hint()?.dataset.side})`);
    ptr(window, "pointerup", 2, y);
    await sleep(600);
    const area = areaOf("search");
    t(area?.dataset.testid === "pcb-pane-dock-left-1", `docked on the left at layer 1 (${area?.dataset.testid})`);
    const a = box(area);
    const props = box(document.querySelector(".app-body .properties-col"));
    t(!!a && a.left <= 2 && (!props || props.width === 0 || a.right <= props.left + 2), `at the editor's edge, outside the Properties column (${a && Math.round(a.right)} <= ${props && Math.round(props.left)})`);
    t(!!a && a.top < c.top - 10, `as tall as the editor, past the canvas's top (${a && Math.round(a.top)} < ${Math.round(c.top)})`);
    t(kept("pcb")?.spot?.search?.layer === 1 && kept("pcb")?.at?.search === "left", `kept (${JSON.stringify(kept("pcb")?.spot?.search)})`);
    t(areaOf("netInspector")?.dataset.testid === "bottom-dock", "the Net Inspector stays under the canvas");
  });

  await scenario("the Net Inspector dropped on Search's lower half shares its dock, under it; the sash between them sizes both", async (t) => {
    const s = box(holder("search"));
    await dragPane("netInspector", s.left + s.width / 2, s.top + s.height * 0.75, async () => t(hint()?.dataset.how === "pane", `the hint: in Search's dock (${hint()?.dataset.how})`));
    t(rowOf("netInspector") && rowOf("netInspector") === rowOf("search"), "one dock");
    const ids = [...rowOf("search").querySelectorAll("[data-dock-pane]")].map((e) => e.dataset.dockPane);
    t(JSON.stringify(ids) === JSON.stringify(["search", "netInspector"]), `Search, then the Net Inspector under it (${ids})`);
    t(box(holder("netInspector")).top >= box(holder("search")).bottom - 1, "one over the other");
    const sizer = document.querySelector('[data-testid="pane-sizer-search-netInspector"]');
    if (!t(!!sizer, "a sash between them")) return;
    const h0 = box(holder("search")).height;
    const m = box(sizer);
    await drag(sizer, m.left + m.width / 2, m.top + 2, m.left + m.width / 2, m.top + 62);
    const h1 = box(holder("search")).height;
    t(Math.abs(h1 - h0 - 60) < 6, `Search 60 px taller (${Math.round(h0)} -> ${Math.round(h1)})`);
    const p = kept("pcb")?.proportion ?? {};
    t(p.search > p.netInspector, `the shares kept (${p.search}, ${p.netInspector})`);
  });

  await scenario("the Net Inspector dropped by Search's outer edge: a dock of its own, outside Search's", async (t) => {
    const s = box(holder("search"));
    // 7 px in: inside its outer 10 px (`auiInsertRowPixels`), past the editor's edge zone (`auiLayerInsertOffset`, 5 px)
    await dragPane("netInspector", s.left + 7, s.top + s.height / 2, async () => t(hint()?.dataset.how === "row", `the hint: a new row (${hint()?.dataset.how})`));
    t(rowOf("netInspector") !== rowOf("search") && areaOf("netInspector") === areaOf("search"), "two docks on the left at layer 1");
    t(box(holder("netInspector")).right <= box(holder("search")).left + 6, `the Net Inspector's outside Search's (${Math.round(box(holder("netInspector")).right)} <= ${Math.round(box(holder("search")).left)})`);
    const k = kept("pcb")?.spot;
    t(k?.netInspector?.row === 0 && k?.search?.row === 1, `rows: ${JSON.stringify(k)}`);
  });

  await scenario("by the canvas's edge a dock of layer 0; by a toolbar along the edge; in the middle of the canvas it floats; a double click docks it back", async (t) => {
    let c = canvas();
    await dragPane("search", c.left + c.width / 2, c.bottom - 12, async () => t(hint()?.dataset.how === "canvas" && hint()?.dataset.side === "bottom", `the hint: by the canvas's bottom (${hint()?.dataset.how} ${hint()?.dataset.side})`));
    t(areaOf("search")?.dataset.testid === "bottom-dock" && kept("pcb")?.spot?.search?.layer === 0, `under the canvas at layer 0 (${areaOf("search")?.dataset.testid})`);
    const bar = box(document.querySelector(".app-body .options-toolbar-col"));
    await dragPane("search", bar.left + bar.width / 2, bar.top + bar.height / 2, async () => t(hint()?.dataset.how === "toolbar", `the hint: by the toolbar (${hint()?.dataset.how})`));
    t(areaOf("search")?.dataset.testid === "pcb-pane-dock-left-1", `along the left edge at the Net Inspector's layer (${areaOf("search")?.dataset.testid})`);
    t(box(holder("search")).right <= box(holder("netInspector")).left + 6, "outside the Net Inspector (row 0)");
    const spot = kept("pcb")?.spot?.search;
    c = canvas();
    await dragPane("search", c.left + c.width / 2, c.top + c.height / 2, async () => t(!hint(), "no hint in the middle of the canvas"));
    const win = document.querySelector('[data-testid="pcb-pane-floating-search"]');
    t(!!win && kept("pcb")?.at?.search === "float", "floating where it was let go");
    await dbl(caption("search"), 600);
    t(kept("pcb")?.at?.search === "left" && JSON.stringify(kept("pcb")?.spot?.search) === JSON.stringify(spot), `a double click docks it back where it was (${JSON.stringify(kept("pcb")?.spot?.search)})`);
    // both back under the canvas by the caption's buttons
    await click(moveBtn("netInspector", "bottom"), 500);
    await click(moveBtn("search", "bottom"), 500);
    t(areaOf("search")?.dataset.testid === "bottom-dock" && rowOf("search") === rowOf("netInspector"), "both back under the canvas, in one dock");
  });

  await scenario("the Nets tab lists the lone pins' unconnected-(..) nets", async (t) => {
    const st = await json("/api/state");
    const lone = (st.nets ?? []).filter((n) => n.startsWith("unconnected-("));
    t(lone.length > 0, `the board has lone pins' nets (${lone.slice(0, 3)})`);
    await click(document.querySelector('[data-testid="search-pane-tab-nets"]'), 500);
    setValue(document.querySelector('[data-testid="search-pane"] input[type="search"]'), "unconnected-(");
    await sleep(600);
    const rows = [...document.querySelectorAll('[data-testid="search-pane-row"]')].map((r) => r.dataset.id);
    t(rows.length === lone.length && lone.every((n) => rows.includes(n)), `every one listed (${rows.length} of ${lone.length})`);
    setValue(document.querySelector('[data-testid="search-pane"] input[type="search"]'), "");
    await __eda.run("pcbnew.Control.showNetInspector");
    await __eda.run("common.Interactive.search");
    await sleep(400);
  });

  await tabTo("Schematic");
  const schDock = () => document.querySelector('[data-testid="sch-search-dock"]');
  await scenario("the schematic's Search pane docks at the editor's right edge and back under the canvas, in a layout of its own", async (t) => {
    if (!schDock()) await __eda.run("common.Interactive.search");
    await until(() => schDock());
    await sleep(800);
    t(!!schDock()?.closest(".bottom-dock-row"), "under the canvas to begin with (.Bottom())");
    const pcbBefore = JSON.stringify(kept("pcb"));
    await dragPane("schSearch", window.innerWidth - 2, frameMidY(), async () => t(hint()?.dataset.how === "edge" && hint()?.dataset.side === "right", `the hint: the right edge (${hint()?.dataset.how} ${hint()?.dataset.side})`));
    t(areaOf("schSearch")?.dataset.testid === "sch-pane-dock-right-1", `docked on the right at layer 1 (${areaOf("schSearch")?.dataset.testid})`);
    t(box(holder("schSearch")).right >= window.innerWidth - 2, "at the editor's edge");
    t(kept("sch")?.at?.schSearch === "right" && JSON.stringify(kept("pcb")) === pcbBefore, "kept in the schematic's layout, the board's untouched");
    await dbl(caption("schSearch"), 600);
    t(!!document.querySelector('[data-testid="sch-pane-floating-schSearch"]'), "a double click floats it");
    await click(moveBtn("schSearch", "bottom"), 600);
    t(!!schDock()?.closest(".bottom-dock-row") && kept("sch")?.at?.schSearch === "bottom", "the caption's button puts it back under the canvas");
  });

  await scenario("its headings: one dragged to the front, an edge dragged, a double click on an edge fits its column", async (t) => {
    const heads = () => [...document.querySelectorAll('[data-testid="sch-search-pane"] th[role="columnheader"]')];
    const labels = () => heads().map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
    document.querySelector('[data-testid="sch-search-pane-tab-symbols"]')?.click();
    await until(() => document.querySelectorAll('[data-testid="sch-search-pane-row"]').length > 5, 15000);
    t(labels()[0] === "Reference" && labels()[2] === "Footprint", `KiCad's order (${labels().slice(0, 3)})`);
    const fp = document.querySelector('[data-testid="sch-search-pane-col-2"]');
    const ref = document.querySelector('[data-testid="sch-search-pane-col-0"]');
    const a = box(fp);
    const b = box(ref);
    await drag(fp, a.left + 12, a.top + a.height / 2, b.left + 4, b.top + b.height / 2, null, fp);
    t(labels()[0] === "Footprint" && labels()[1] === "Reference", `Footprint dragged to the front (${labels().slice(0, 3)})`);
    t(!/[▲▼]/.test(fp.textContent), "the click that ends the drag does not sort");
    const w0 = box(document.querySelector('[data-testid="sch-search-pane-col-1"]')).width;
    const edge = document.querySelector('[data-testid="sch-search-pane-resize-1"]');
    const e = box(edge);
    await drag(edge, e.left + 2, e.top + 4, e.left + 42, e.top + 4);
    const w1 = box(document.querySelector('[data-testid="sch-search-pane-col-1"]')).width;
    t(Math.abs(w1 - w0 - 40) < 4, `Value's edge dragged 40 px (${Math.round(w0)} -> ${Math.round(w1)})`);
    const lib = document.querySelector('[data-testid="sch-search-pane-resize-10"]');
    if (lib) {
      await dbl(lib, 400);
      const cells = [...document.querySelectorAll('[data-testid="sch-search-pane-row"]')].map((r) => r.querySelectorAll("td")[[...heads()].findIndex((h) => h.dataset.col === "10")]);
      const wide = Math.max(...cells.map((td) => td?.scrollWidth ?? 0));
      const w = box(document.querySelector('[data-testid="sch-search-pane-col-10"]')).width;
      t(w >= wide - 2, `Library Link fits its longest link (${Math.round(w)} >= ${wide})`);
    } else t(false, "Library Link's edge");
  });

  await scenario("a Find over every sheet from the root reaches an ERC marker of another sheet and goes there", async (t) => {
    while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    const replies = [];
    const of = window.fetch;
    window.fetch = async (...a) => { const r = await of(...a); const u = String(a[0]); if (u.startsWith("/api/erc") || u.startsWith("/api/sch/find")) r.clone().json().then((j) => replies.push({ u, j, body: a[1]?.body })).catch(() => {}); return r; };
    // a label on the MCU sheet that nothing joins: a finding of that sheet alone (`label_dangling`), taken back at the end
    const added = await json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "on_sheet", sheet: "sheet_80460aabc8e9", cmd: { op: "add_label", net: "DOCKV19", at: { x: 254_000_000, y: 165_100_000 }, kind: { scope: "local" } } }, strict: false }) });
    t(added.ok !== false, `a dangling label on the MCU sheet (${JSON.stringify(added).slice(0, 100)})`);
    await sleep(1200);
    try {
      await __eda.run("eeschema.InspectionTool.runERC");
      const dlgOf = (re) => [...document.querySelectorAll(".dialog")].find((d) => re.test(d.querySelector(".dialog-header")?.textContent ?? ""));
      const erc = await until(() => dlgOf(/Electrical Rules Checker/));
      if (!t(!!erc, "the ERC dialog opens")) return;
      replies.length = 0;
      [...erc.querySelectorAll("button")].find((b) => b.textContent.trim() === "Run ERC")?.click();
      const rep = (await until(() => replies.find((x) => x.u.startsWith("/api/erc") && Array.isArray(x.j?.violations) && x.j.violations.length > 0), 120000))?.j;
      [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Close")?.click();
      await sleep(500);
      if (!t(!!rep && rep.violations.length > 0, `ERC ran (${rep?.violations?.length} findings)`)) return;
      const mcu = await json("/api/schematic?sheet=sheet_80460aabc8e9");
      const root = await json("/api/schematic");
      const on = (s, v) => v.location && (s.symbols.some((x) => v.location === x.id || v.location.startsWith(`${x.id}.`)) || s.power_symbols.some((p) => p.id === v.location) || s.labels.some((l) => l.id === v.location));
      const pick = rep.violations.map((v, i) => ({ v, i })).find(({ v }) => on(mcu, v) && !on(root, v));
      if (!t(!!pick, "a finding on the MCU sheet alone")) return;
      const { v, i } = pick;
      const descs = (v.items ?? []).map((x) => x.description).filter((d) => d);
      const tail = descs.length ? `: ${descs.join("; ")}` : "";
      const msg = tail && (v.hint ?? "").endsWith(tail) ? v.hint.slice(0, -tail.length) : v.hint;
      await __eda.run("common.Interactive.find");
      const dlg = await until(() => dlgOf(/^Find$/));
      if (!t(!!dlg, "Find opens")) return;
      const only = [...dlg.querySelectorAll("label.toggle")].find((l) => l.textContent.trim() === "Search the current sheet only")?.querySelector("input");
      if (only?.checked) { only.click(); await sleep(250); }
      setValue(dlg.querySelector(".kv-grid input"), msg);
      await sleep(400);
      replies.length = 0;
      let went = false;
      for (let k = 0; k < 12 && !went; k++) {
        [...dlg.querySelectorAll("button")].find((b) => b.textContent.trim() === "Find Next")?.click();
        await sleep(1500);
        went = JSON.stringify(__eda.state().sheetPath) === JSON.stringify(["sheet_80460aabc8e9"]);
      }
      const reply = replies.find((x) => x.u.startsWith("/api/sch/find") && x.body && JSON.parse(x.body).all_sheets)?.j;
      const hit = (reply?.matches ?? []).find((m) => m.id === `erc:${i}`);
      t(!!hit && JSON.stringify(hit.sheet) === JSON.stringify(["sheet_80460aabc8e9"]), `the root's Find lists the marker on its own sheet (${msg}; ${JSON.stringify(hit?.sheet)})`);
      const sent = replies.find((x) => x.u.startsWith("/api/sch/find") && x.body)?.body;
      t(!!sent && JSON.parse(sent).markers.some((m) => m.sheet === "sheet_80460aabc8e9"), "the markers of every sheet go with the search");
      t(went, `Find Next goes to that sheet (${JSON.stringify(__eda.state().sheetPath)})`);
      [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === "Close")?.click();
      await sleep(400);
    } finally {
      window.fetch = of;
      if (added.ok !== false) { await __eda.run("common.Interactive.undo"); await sleep(1200); }
      while (__eda.state().sheetPath.length > 0) { await __eda.run("eeschema.NavigateTool.leaveSheet"); await sleep(500); }
    }
  });

  await scenario("end: the panes put away, no console errors", async (t) => {
    if (schDock()) await __eda.run("common.Interactive.search");
    await tabTo("PCB");
    const errs = __eda.errors().slice(errorsBefore);
    t(errs.length === 0, `no errors (${JSON.stringify(errs).slice(0, 300)})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
})();
