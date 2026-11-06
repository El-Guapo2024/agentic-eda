// The Net Inspector and Search panes where wxAUI lets them go, and their lists' columns (UI-LEFT-2 PP13; PARITY-pcb.md section 102). It drives the panes' own
// captions, sashes and headings with the pointer and reads the layout back from the page and from what the browser keeps:
//   buttons       the caption's buttons dock the Net Inspector on the left, the right and the top of the canvas, float it as a window, and bring it back under
//                 the canvas; each place is kept (`eda-studio.pcb-pane-dock.v1`)
//   drag          Search's caption dragged onto the centre dock guide's right arrow docks it on the right (the hint shows first); dragged anywhere else it floats
//                 where it was let go; a double click on the caption docks it back on the side it came from, again floats it
//   sash          a side dock's sash dragged sizes it, and the size is kept
//   ni-columns    the Net Inspector's headings: Pad Count dragged before Netclass (the click that ends the drag does not sort), the Name never moves, an edge
//                 dragged widens its column, the headings' right-click menu (Auto-fit Column, Auto-fit All Columns, then Show / Hide Columns) fits a column to its
//                 values, then every column; the order and widths kept
//   search-columns Search's headings: Library Link dragged to the front (a click still sorts by it), Value's edge dragged, a double click on Library Link's edge
//                 fits it to its longest link
//   nets-link     the Nets tab lists a net no item is on (Add Net's); each footprint's Library Link is the link the board file gives it
//   end           both panes back under the canvas, the board as it was, no console errors
// It answers through `window.__panelsDockCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=8
(async () => {
  const results = [];
  const run = (window.__panelsDockCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const click = async (el, wait = 300) => { if (!el) throw new Error("nothing to click"); el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true })); await sleep(wait); };
  const dbl = async (el, wait = 400) => { if (!el) throw new Error("nothing to double-click"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, cancelable: true })); await sleep(wait); };
  const rightClick = async (el, wait = 250) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.top + 4 })); await sleep(wait); };
  const ptr = (target, type, x, y) => target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons: type === "pointerup" ? 0 : 1, pointerId: 1, isPrimary: true }));
  /** A press on `el` at (`x0`, `y0`), a few moves to (`x1`, `y1`) on the window, a release there; `clickOn` gets the click a browser sends right after the release. */
  const dragFrom = async (el, x0, y0, x1, y1, clickOn) => {
    ptr(el, "pointerdown", x0, y0);
    for (let i = 1; i <= 6; i++) { ptr(window, "pointermove", x0 + ((x1 - x0) * i) / 6, y0 + ((y1 - y0) * i) / 6); await sleep(30); }
    await sleep(120);
    ptr(window, "pointerup", x1, y1);
    if (clickOn) clickOn.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x1, clientY: y1 }));
    await sleep(350);
  };
  const mid = (el) => { const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; };
  const kept = () => JSON.parse(localStorage.getItem("eda-studio.pcb-pane-dock.v1") ?? "null");
  const niKept = () => JSON.parse(localStorage.getItem("eda-studio.net-inspector.v1") ?? "{}");
  const pane = (id) => document.querySelector(`[data-pane="${id}"]`);
  const holder = (id) => pane(id)?.closest("[data-dock-side], [data-testid^=pcb-pane-floating]");
  const where = (id) => { const h = holder(id); return h ? (h.dataset.dockSide ?? (h.dataset.testid?.startsWith("pcb-pane-floating") ? "float" : "?")) : null; };
  const caption = (id) => pane(id)?.querySelector(".dock-panel-caption");
  const moveBtn = (id, to) => pane(id)?.querySelector(`.pcb-pane-move[data-dock-to="${to}"]`);
  const canvas = () => document.querySelector(".app-body .canvas-col").getBoundingClientRect();
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")];
  const menuItem = (label) => menuItems().find((m) => m.textContent.replace("✓", "").trim() === label);
  const niHeads = () => [...document.querySelectorAll('[data-testid="net-inspector"] [role="columnheader"]')];
  const niHead = (key) => document.querySelector(`[data-testid="net-inspector-col-${key}"]`);
  const niLabels = () => niHeads().map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
  const sHeads = () => [...document.querySelectorAll('[data-testid="search-pane"] th[role="columnheader"]')];
  const sHead = (i) => document.querySelector(`[data-testid="search-pane-col-${i}"]`);
  const sRows = () => [...document.querySelectorAll('[data-testid="search-pane-row"]')];

  for (const k of ["eda-studio.pcb-pane-dock.v1", "eda-studio.net-inspector.v1", "eda-studio.search-pane.v1", "eda-studio.dock-layout.v1"]) localStorage.removeItem(k);
  const before = await json("/api/state");
  const errorsBefore = __eda.errors().length;

  await scenario("the caption's buttons dock the Net Inspector on each side of the canvas and float it; each place is kept", async (t) => {
    await __eda.run("pcbnew.Control.showNetInspector");
    await __eda.run("common.Interactive.search");
    await sleep(800);
    t(where("netInspector") === "bottom" && where("search") === "bottom", `both start under the canvas (${where("netInspector")}, ${where("search")})`);
    t(!!pane("search").querySelector(".dock-panel-close[aria-label='Hide Search']"), "the close button is still the caption's close");
    await click(moveBtn("netInspector", "left"), 500);
    let c = canvas();
    let r = holder("netInspector")?.getBoundingClientRect();
    t(where("netInspector") === "left" && r && r.right <= c.left + 2 && r.top <= c.top + 2, `docked on the left of the canvas (${where("netInspector")}: ${r && Math.round(r.right)} <= ${Math.round(c.left)})`);
    t(kept()?.at?.netInspector === "left", "kept");
    t(where("search") === "bottom", "Search stays under the canvas");
    await click(moveBtn("netInspector", "right"), 500);
    c = canvas();
    r = holder("netInspector")?.getBoundingClientRect();
    t(where("netInspector") === "right" && r && r.left >= c.right - 2, `on the right (${r && Math.round(r.left)} >= ${Math.round(c.right)})`);
    await click(moveBtn("netInspector", "top"), 500);
    c = canvas();
    r = holder("netInspector")?.getBoundingClientRect();
    t(where("netInspector") === "top" && r && r.bottom <= c.top + 2, `at the top (${r && Math.round(r.bottom)} <= ${Math.round(c.top)})`);
    await click(moveBtn("netInspector", "float"), 500);
    const win = document.querySelector('[data-testid="pcb-pane-floating-netInspector"]');
    t(where("netInspector") === "float" && !!win && getComputedStyle(win).position === "fixed", "floating as a window of its own");
    t(!!win && Math.abs(win.offsetWidth - 300) < 3 && Math.abs(win.offsetHeight - 200) < 3, `at its FloatingSize (${win?.offsetWidth} x ${win?.offsetHeight})`);
    t(document.querySelectorAll('[data-testid="net-inspector-row"]').length > 3, "its nets are listed there");
    await click(moveBtn("netInspector", "bottom"), 500);
    t(where("netInspector") === "bottom" && kept()?.at?.netInspector === "bottom", "back under the canvas");
  });

  await scenario("Search dragged by its caption onto a dock guide docks there; let go elsewhere it floats; a double click docks it back, again floats it", async (t) => {
    const cap = caption("search");
    const [x0, y0] = [cap.getBoundingClientRect().left + 30, cap.getBoundingClientRect().top + 6];
    // the right arrow of the centre dock guide, known once the drag has begun
    ptr(cap, "pointerdown", x0, y0);
    ptr(window, "pointermove", x0 + 20, y0 - 20);
    await sleep(150);
    const guide = document.querySelector('[data-testid="pcb-pane-guide-right"]');
    t(!!guide && document.querySelectorAll(".pcb-pane-dock-guide").length === 4, "the centre dock guide's four arrows show while the caption is dragged");
    t(!!document.querySelector('[data-testid="pcb-pane-drag-ghost"]'), "the pane's window follows the pointer");
    const [gx, gy] = mid(guide);
    for (let i = 1; i <= 5; i++) { ptr(window, "pointermove", x0 + 20 + ((gx - x0 - 20) * i) / 5, y0 - 20 + ((gy - y0 + 20) * i) / 5); await sleep(30); }
    await sleep(150);
    const hint = document.querySelector('[data-testid="pcb-pane-dock-hint"]');
    const c = canvas();
    t(hint?.dataset.side === "right" && hint.getBoundingClientRect().right >= c.right - 2, `over the arrow, the hint shows the right of the canvas (${hint?.dataset.side})`);
    ptr(window, "pointerup", gx, gy);
    await sleep(500);
    t(where("search") === "right" && kept()?.at?.search === "right", `docked on the right (${where("search")})`);
    t(!document.querySelector(".pcb-pane-dock-guide"), "the guide is gone");
    // let go away from the guide: a floating window where it was let go
    const cap2 = caption("search");
    const [x1, y1] = [cap2.getBoundingClientRect().left + 20, cap2.getBoundingClientRect().top + 6];
    const drop = [c.left + 60, c.top + 60];
    await dragFrom(cap2, x1, y1, drop[0], drop[1]);
    const win = document.querySelector('[data-testid="pcb-pane-floating-search"]');
    const wr = win?.getBoundingClientRect();
    t(where("search") === "float" && !!wr && Math.abs(wr.left - (drop[0] - Math.min(20, 240))) < 30 && Math.abs(wr.top - (drop[1] - 6)) < 30, `floating where it was let go (${wr && Math.round(wr.left)}, ${wr && Math.round(wr.top)} by ${drop.map(Math.round)})`);
    t(!!win && Math.abs(win.offsetWidth - 480) < 3, `at Search's FloatingSize (${win?.offsetWidth})`);
    // its caption moves the window
    const cap3 = caption("search");
    const r3 = cap3.getBoundingClientRect();
    await dragFrom(cap3, r3.left + 40, r3.top + 6, r3.left + 140, r3.top + 46);
    const wr2 = document.querySelector('[data-testid="pcb-pane-floating-search"]')?.getBoundingClientRect();
    t(!!wr2 && Math.abs(wr2.left - wr.left - 100) < 4 && Math.abs(wr2.top - wr.top - 40) < 4, `the caption moves the window (${wr2 && Math.round(wr2.left - wr.left)}, ${wr2 && Math.round(wr2.top - wr.top)})`);
    t(kept()?.floating?.search && Math.abs(kept().floating.search.x - wr2.left) < 3, "where it is kept");
    await dbl(caption("search"), 500);
    t(where("search") === "right", `a double click docks it back where it came from (${where("search")})`);
    await dbl(caption("search"), 500);
    t(where("search") === "float", `again: it floats (${where("search")})`);
    await click(moveBtn("search", "right"), 500);
  });

  await scenario("a side dock's sash sizes it, and the size is kept", async (t) => {
    const dock = document.querySelector('[data-testid="pcb-pane-dock-right"]');
    const w0 = dock.getBoundingClientRect().width;
    const sash = document.querySelector('[data-testid="pcb-pane-sash-right"]');
    const [sx, sy] = mid(sash);
    await dragFrom(sash, sx, sy, sx - 80, sy);
    const w1 = document.querySelector('[data-testid="pcb-pane-dock-right"]').getBoundingClientRect().width;
    t(Math.abs(w1 - w0 - 80) < 4, `80 px wider (${Math.round(w0)} -> ${Math.round(w1)})`);
    t(Math.abs((kept()?.size?.right ?? 0) - w1) < 4, `kept (${kept()?.size?.right})`);
    await click(moveBtn("search", "bottom"), 500);
    t(where("search") === "bottom", "Search back under the canvas");
  });

  await scenario("the Net Inspector's headings: one dragged to another place, the Name never; an edge dragged; Auto-fit Column and Auto-fit All Columns", async (t) => {
    t(niLabels().slice(0, 3).join("|") === "Name|Netclass|Total Length", `KiCad's order to begin with (${niLabels().slice(0, 3).join("|")})`);
    const sortBefore = niKept().sortingColumn ?? null;
    const pad = niHead("pad_count");
    const [px, py] = mid(pad);
    const nc = niHead("netclass").getBoundingClientRect();
    await dragFrom(pad, px, py, nc.left + 3, py, pad); // with the click that ends the drag
    t(niLabels().slice(0, 3).join("|") === "Name|Pad Count|Netclass", `Pad Count before Netclass (${niLabels().slice(0, 4).join("|")})`);
    t((niKept().colOrder ?? []).slice(0, 3).join(",") === "name,pad_count,netclass", `the order kept (${(niKept().colOrder ?? []).slice(0, 3)})`);
    t((niKept().sortingColumn ?? null) === sortBefore, "the click that ended the drag did not sort");
    const name = niHead("name");
    const [nx, ny] = mid(name);
    const last = niHeads().at(-1).getBoundingClientRect();
    await dragFrom(name, nx, ny, last.right - 2, ny);
    t(niLabels()[0] === "Name", "the Name is not reorderable (no wxDATAVIEW_COL_REORDERABLE)");
    // the Netclass column's edge dragged 60 px
    const w0 = niHead("netclass").getBoundingClientRect().width;
    const edge = document.querySelector('[data-testid="net-inspector-resize-netclass"]');
    const [ex, ey] = mid(edge);
    await dragFrom(edge, ex, ey, ex + 60, ey);
    const w1 = niHead("netclass").getBoundingClientRect().width;
    t(Math.abs(w1 - w0 - 60) < 3, `60 px wider (${Math.round(w0)} -> ${Math.round(w1)})`);
    t(Math.abs((niKept().colWidths?.netclass ?? 0) - w1) < 2, `the width kept (${niKept().colWidths?.netclass})`);
    // the headings' menu
    await rightClick(niHead("netclass"));
    const labels = menuItems().map((m) => m.textContent.replace("✓", "").trim()).filter(Boolean);
    t(labels[0] === "Auto-fit Column" && labels[1] === "Auto-fit All Columns" && labels.includes("Netclass") && labels.includes("Pad Count"), `the menu (${labels.slice(0, 5).join("|")} ...)`);
    await click(menuItem("Auto-fit Column"), 400);
    const w2 = niHead("netclass").getBoundingClientRect().width;
    const cells = [...document.querySelectorAll('[data-testid="net-inspector-row"] [data-col="netclass"]')];
    t(w2 < w1 && cells.every((c) => c.scrollWidth <= c.clientWidth + 1), `Auto-fit Column: as wide as its widest class, none cut (${Math.round(w1)} -> ${Math.round(w2)})`);
    t(Object.keys(niKept().colWidths ?? {}).length === 1, "only that column's width set");
    await rightClick(niHead("name"));
    await click(menuItem("Auto-fit All Columns"), 400);
    const keptWidths = niKept().colWidths ?? {};
    t(niHeads().every((h) => keptWidths[h.dataset.col] !== undefined), `Auto-fit All Columns: every shown column's width (${Object.keys(keptWidths).length})`);
    const names = [...document.querySelectorAll('[data-testid="net-inspector-row"] [data-col="name"]')];
    t(names.every((c) => c.scrollWidth <= c.clientWidth + 1), "no net name cut");
  });

  await scenario("Search's headings: Library Link dragged to the front still sorts by itself; Value's edge dragged; a double click on an edge fits the column", async (t) => {
    await click(document.querySelector('[data-testid="search-pane-tab-footprints"]'), 400);
    const link = sHead(5);
    const [lx, ly] = mid(link);
    const first = sHead(0).getBoundingClientRect();
    await dragFrom(link, lx, ly, first.left + 3, ly);
    t(sHeads()[0]?.dataset.col === "5", `Library Link first (${sHeads().map((h) => h.dataset.col).join(",")})`);
    const firstCells = sRows().slice(0, 3).map((r) => r.querySelector("td")?.textContent);
    const parts = new Map(before.parts.map((p) => [p.ref, p]));
    t(sRows().slice(0, 3).every((r, i) => (parts.get(r.dataset.id)?.lib_link ?? "") === firstCells[i]), "its cells moved with it");
    await click(sHead(5), 400);
    t(sHead(5).textContent.includes("▲"), "a click on it sorts by the Library Link (the model's column)");
    const w0 = sHead(1).getBoundingClientRect().width;
    const edge = document.querySelector('[data-testid="search-pane-resize-1"]');
    const [ex, ey] = mid(edge);
    await dragFrom(edge, ex, ey, ex + 50, ey);
    const w1 = sHead(1).getBoundingClientRect().width;
    t(Math.abs(w1 - w0 - 50) < 3, `Value 50 px wider (${Math.round(w0)} -> ${Math.round(w1)})`);
    await dbl(document.querySelector('[data-testid="search-pane-resize-5"]'), 400);
    const cells = sRows().map((r) => r.querySelector("td"));
    t(cells.length > 0 && cells.every((c) => c.scrollWidth <= c.clientWidth + 1), `a double click on the edge: every link shown whole (${Math.round(sHead(5).getBoundingClientRect().width)} px)`);
  });

  await scenario("the Nets tab lists a net no item is on; each footprint's Library Link is the link the board file gives it", async (t) => {
    const r = await json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: { op: "add_net", name: "PP13_SPARE" }, strict: false }) });
    t(r.ok !== false, `Add Net (${JSON.stringify(r).slice(0, 120)})`);
    await sleep(1500);
    await click(document.querySelector('[data-testid="search-pane-tab-nets"]'), 500);
    const spare = sRows().find((x) => x.dataset.id === "PP13_SPARE");
    t(!!spare, `PP13_SPARE is listed (${sRows().length} nets)`);
    const ni = await json("/api/net_inspector");
    t(!(ni.nets ?? []).some((n) => n.name === "PP13_SPARE"), "(the Net Inspector lists the nets of items only, as KiCad's calculateNets does)");
    await __eda.run("common.Interactive.undo");
    await sleep(1500);
    await click(document.querySelector('[data-testid="search-pane-tab-footprints"]'), 500);
    const st = await json("/api/state");
    const linkOf = new Map(st.parts.map((p) => [p.ref, p.lib_link]));
    const shown = sRows().map((x) => [x.dataset.id, [...x.querySelectorAll("td")].find((td, i) => sHeads()[i]?.dataset.col === "5")?.textContent]);
    t(shown.length > 0 && shown.every(([ref, cell]) => cell === linkOf.get(ref)), `every Library Link is the board file's (${shown.slice(0, 2).map((s) => s.join(" ")).join(", ")})`);
    const differs = st.parts.filter((p) => p.placed && p.lib_link && p.lib_link !== p.footprint);
    t(differs.length > 0 && differs.every((p) => p.lib_link.includes(":")), `a built-in package shows its installed footprint, not its bare name (${differs.slice(0, 2).map((p) => `${p.footprint} -> ${p.lib_link}`).join(", ")})`);
  });

  await scenario("both panes back under the canvas; the board as it was; no console errors", async (t) => {
    await __eda.run("pcbnew.Control.showNetInspector");
    await __eda.run("common.Interactive.search");
    await sleep(400);
    t(!pane("netInspector") && !pane("search"), "both hidden");
    for (const k of ["eda-studio.pcb-pane-dock.v1", "eda-studio.net-inspector.v1", "eda-studio.search-pane.v1"]) localStorage.removeItem(k);
    const after = await json("/api/state");
    t(JSON.stringify(after.routing) === JSON.stringify(before.routing) && JSON.stringify(after.nets) === JSON.stringify(before.nets), "the board's copper and nets as they were");
    const errs = __eda.errors().slice(errorsBefore).map((e) => e.message);
    t(errs.length === 0, `no errors (${errs.join(" | ")})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
  return results;
})();
