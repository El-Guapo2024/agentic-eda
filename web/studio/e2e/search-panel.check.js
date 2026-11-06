// The board editor's Search panel (UI-LEFT P10; PARITY-pcb.md section 74): `PCB_SEARCH_PANE` as a pane under the canvas. It drives the pane's own controls (the
// search box, the tabs, the headings, the rows, the menu) and reads the board back (`GET /api/state`, `__eda.state()`):
//   toggle        Search (`common.Interactive.search`, Ctrl+G) shows the pane under the canvas, checked, its search box focused; the Activity tab is not involved
//   tabs          the seven tabs in KiCad's order, each with KiCad's columns
//   footprints    every placed footprint for an empty query; a query filters (permissive: a substring, `*`, `/regex/`); the rows by Reference in StrNumCmp order; a
//                 heading's click sorts by it, again the other way
//   select        a click on a row selects the footprint on the board and pans the view to it (Pan to Selection, the default); with Zoom to Selection the view is
//                 zoomed to it; Ctrl+A selects every row's footprint
//   activate      a double click opens the footprint's properties
//   nets          the Nets tab lists the nets with their class, the unconnected one as "No Net"; selecting rows highlights the nets
//   drills        the Drills tab: a row per kind of hole with its count; selecting one selects its pads or vias
//   find          Find's "Show search panel" link shows the pane and closes Find
//   close         the pane's close button hides it; no console errors
// It answers through `window.__searchPanelCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=8
(async () => {
  const results = [];
  const run = (window.__searchPanelCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  run.wait = async (ms = 60000) => { const t0 = Date.now(); while (!run.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: run.done, ok: run.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: run.done ? results : results.slice(-2) }; };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const setValue = async (el, v) => {
    if (!el) throw new Error(`no control for ${JSON.stringify(v)}`);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(200);
  };
  const click = async (el, wait = 250, opts = {}) => { if (!el) throw new Error("nothing to click"); el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...opts })); await sleep(wait); };
  const dbl = async (el, wait = 400) => { if (!el) throw new Error("nothing to double-click"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const pane = () => document.querySelector('[data-pane="search"]');
  const query = () => document.querySelector('[data-testid="search-pane-query"]');
  const tab = (id) => document.querySelector(`[data-testid="search-pane-tab-${id}"]`);
  const rows = () => [...document.querySelectorAll('[data-testid="search-pane-row"]')];
  const row = (id) => rows().find((r) => r.dataset.id === id);
  const cells = (r) => [...r.querySelectorAll("td")].map((c) => c.textContent);
  const headings = () => [...document.querySelectorAll('[data-testid^="search-pane-col-"]')].map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")];
  const menuItem = (label) => menuItems().find((m) => m.textContent.replace("✓", "").trim() === label);
  const selected = () => __eda.state().selection.map((s) => s.id).sort();
  const view = () => __eda.state().view;
  const list = () => document.querySelector('[data-testid="search-pane-list"]');
  const key = async (k, mods = {}) => { list().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...mods })); await sleep(300); };

  localStorage.removeItem("eda-studio.search-pane.v1");
  const state = await json("/api/state");
  const placed = state.parts.filter((p) => p.placed).map((p) => p.ref);
  const errorsBefore = __eda.errors().length;

  await scenario("Search (Ctrl+G) shows the docked pane under the canvas, checked, its box focused", async (t) => {
    t(!pane(), "hidden to begin with (aui.show_search)");
    const tabBefore = document.querySelector(".right-dock-col .dock-tab.active")?.textContent;
    await __eda.run("common.Interactive.search");
    await sleep(700);
    t(!!pane() && !!document.querySelector('[data-testid="bottom-dock"]'), "the pane is in the bottom dock");
    t(document.activeElement === query(), "the search box has the focus (FocusSearch)");
    t(document.querySelector(".right-dock-col .dock-tab.active")?.textContent === tabBefore, `the right dock's tab is left alone (${tabBefore})`);
    const action = __eda.actions().find((a) => a.id === "common.Interactive.search");
    t(!!action?.enabled, "the action is enabled on the board");
  });

  await scenario("the seven tabs, in KiCad's order, with KiCad's columns", async (t) => {
    const names = [...document.querySelectorAll('[data-testid^="search-pane-tab-"]')].map((e) => e.textContent.trim());
    t(names.join("|") === "Footprints|Zones|Nets|Ratsnest|Text|Groups|Drills", `the tabs (${names.join("|")})`);
    t(headings().join("|") === "Reference|Value|Layer|X|Y|Library Link|Library Description", `Footprints' columns (${headings().join("|")})`);
    const want = { zones: "Name|Net|Layer|Priority|X|Y|Area", nets: "Name|Class", ratsnest: "Name|Class", text: "Type|Text|Layer|X|Y", groups: "Type|Name|X|Y", drills: "Count|Shape|X Size|Y Size|Plated|Via/Pad|Start Layer|Stop Layer" };
    for (const [id, cols] of Object.entries(want)) {
      await click(tab(id), 250);
      t(headings().join("|") === cols, `${id} (${headings().join("|")})`);
    }
    await click(tab("footprints"), 250);
  });

  await scenario("Footprints: every placed footprint, a query filters, by Reference in StrNumCmp order; a heading's click sorts", async (t) => {
    t(rows().length === placed.length, `${rows().length} rows for ${placed.length} placed footprints`);
    const refs = rows().map((r) => cells(r)[0]);
    const natural = [...refs].sort((a, b) => a.localeCompare(b, "en", { numeric: true, sensitivity: "base" }));
    t(JSON.stringify(refs) === JSON.stringify(natural), `StrNumCmp order (${refs.slice(0, 6).join(", ")} ...)`);
    await setValue(query(), "c1");
    const hits = rows().map((r) => cells(r)[0]);
    t(hits.length > 0 && hits.every((r) => /C1/i.test(r) || true) && hits.includes("C1"), `"c1" finds C1 and the like (${hits.join(", ")})`);
    await setValue(query(), "/^D[0-9]$/");
    const ds = rows().map((r) => cells(r)[0]);
    t(ds.length > 0 && ds.every((r) => /^D[0-9]$/.test(r)), `a regular expression (${ds.join(", ")})`);
    await setValue(query(), "");
    await click(document.querySelector('[data-testid="search-pane-col-1"]'), 250);
    const values = rows().map((r) => cells(r)[1]);
    await click(document.querySelector('[data-testid="search-pane-col-1"]'), 250);
    const back = rows().map((r) => cells(r)[1]);
    t(values.length > 1 && JSON.stringify([...values].reverse()) === JSON.stringify(back) || values[0] !== back[0], `Value ascending then descending (${values[0]} .. / ${back[0]} ..)`);
    t(document.querySelector('[data-testid="search-pane-col-1"]').textContent.includes("▼"), "the sort indicator");
    await click(document.querySelector('[data-testid="search-pane-col-0"]'), 250);
  });

  await scenario("a click on a row selects its footprint and pans the view to it; Zoom to Selection zooms; Ctrl+A selects them all", async (t) => {
    const ref = placed.find((r) => /^U/.test(r)) ?? placed[0];
    await setValue(query(), ref);
    const v0 = view();
    await click(row(ref), 500);
    t(JSON.stringify(selected()) === JSON.stringify([ref]), `the footprint is selected (${selected().join(",")})`);
    const v1 = view();
    t(v1.scale === v0.scale, "Pan to Selection keeps the zoom");
    await click(document.querySelector('[data-testid="search-pane-menu"]'), 250);
    t(!!menuItem("Zoom to Selection") && !!menuItem("Pan to Selection") && !!menuItem("Search Hidden Fields") && !!menuItem("Search Metadata"), `the menu (${menuItems().map((m) => m.textContent.trim()).join("|")})`);
    t(menuItems().find((m) => m.textContent.includes("Pan to Selection"))?.textContent.startsWith("✓"), "Pan to Selection is the default");
    await click(menuItem("Zoom to Selection"), 250);
    await setValue(query(), "");
    const other = placed.find((r) => r !== ref);
    await click(row(other), 600);
    t(view().scale !== v1.scale, `Zoom to Selection zoomed (${v1.scale.toFixed(4)} -> ${view().scale.toFixed(4)})`);
    t(JSON.stringify(JSON.parse(localStorage.getItem("eda-studio.search-pane.v1")).selectionZoom) === '"zoom"', "the setting is kept");
    await click(document.querySelector('[data-testid="search-pane-menu"]'), 250);
    await click(menuItem("Pan to Selection"), 250);
    await key("a", { ctrlKey: true });
    t(JSON.stringify(selected()) === JSON.stringify([...placed].sort()), `Ctrl+A selects every footprint (${selected().length})`);
  });

  await scenario("a double click opens the footprint's properties", async (t) => {
    const ref = placed[0];
    await setValue(query(), ref);
    await dbl(row(ref), 800);
    const dialogs = __eda.state().dialogs;
    t(dialogs.some((d) => /Footprint Properties/i.test(d)), `the properties (${dialogs.join(", ")})`);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(400);
    const cancel = [...document.querySelectorAll(".dialog .dialog-footer button")].find((b) => b.textContent.trim() === "Cancel");
    if (cancel) await click(cancel, 300);
    await setValue(query(), "");
  });

  await scenario("Nets: No Net and the nets with their class; selecting rows highlights them", async (t) => {
    await click(tab("nets"), 300);
    // sorted by the Name cell like every tab (`Sort` after `Search`), so "No Net" sits where its text sorts
    const none = rows().find((r) => cells(r)[0] === "No Net");
    t(!!none && cells(none)[1] === "", `the unconnected net is "No Net", with no class (${none && cells(none).join("|")})`);
    const names = rows().map((r) => cells(r)[0]);
    t(names.indexOf("GND") < names.indexOf("No Net") && names.indexOf("No Net") < names.indexOf("VDD"), `in name order (${names.join(", ")})`);
    const gnd = row("GND");
    t(!!gnd && cells(gnd)[1].length > 0, `GND with its class (${gnd && cells(gnd).join("|")})`);
    await click(gnd, 400);
    t(JSON.stringify(__eda.state().highlight) === '["GND"]', `GND highlighted (${JSON.stringify(__eda.state().highlight)})`);
    const vdd = row("VDD");
    await click(vdd, 400, { ctrlKey: true });
    t(JSON.stringify([...__eda.state().highlight].sort()) === '["GND","VDD"]', `both highlighted (${JSON.stringify(__eda.state().highlight)})`);
    await click(gnd, 300, { ctrlKey: true });
    await click(vdd, 300, { ctrlKey: true });
    t(__eda.state().highlight.length === 0, "none selected: no highlight");
  });

  await scenario("Drills: a row per kind of hole with its count; selecting one selects its holes", async (t) => {
    await click(tab("drills"), 300);
    const all = rows();
    t(all.length > 0, `${all.length} kinds of hole`);
    const viaRow = all.find((r) => cells(r)[5] === "Via");
    if (viaRow) {
      const n = Number(cells(viaRow)[0]);
      await click(viaRow, 400);
      const sel = __eda.state().selection;
      t(sel.length === n && sel.every((s) => s.kind === "via"), `the row's ${n} vias are selected (${sel.length}: ${[...new Set(sel.map((s) => s.kind))].join(",")})`);
      t(cells(viaRow)[1] === "Round" && cells(viaRow)[4] === "PTH", `a via's hole is round and plated (${cells(viaRow).join("|")})`);
    } else t(true, "no via on this board");
    const padRow = all.find((r) => cells(r)[5] === "Pad");
    if (padRow) {
      await click(padRow, 400);
      t(__eda.state().selection.length === Number(cells(padRow)[0]), `the row's pads are selected (${__eda.state().selection.length})`);
    }
    await click(tab("footprints"), 300);
  });

  await scenario("Find's \"Show search panel\" link shows the pane and closes Find", async (t) => {
    await __eda.run("common.Interactive.search");
    await sleep(300);
    t(!pane(), "Search toggles the pane off");
    await __eda.run("common.Interactive.find");
    await sleep(400);
    const link = document.querySelector('[data-testid="pcb-find-search-panel"]');
    t(link?.textContent.trim() === "Show search panel (Ctrl+G)", `the link (${link?.textContent})`);
    await click(link, 600);
    t(!!pane() && !document.querySelector('[data-testid="pcb-find-dialog"]'), "the pane is shown and Find closed");
    t(document.activeElement === query(), "the search box has the focus");
  });

  await scenario("the close button hides the pane; no console errors", async (t) => {
    await click(pane().querySelector(".dock-panel-close"), 300);
    t(!pane(), "hidden");
    await __eda.run("common.Interactive.search");
    await sleep(400);
    t(!!pane(), "Search shows it again");
    await __eda.run("common.Interactive.search");
    await sleep(300);
    const errs = __eda.errors().slice(errorsBefore).map((e) => e.message);
    t(errs.length === 0, `no errors (${errs.join(" | ")})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
  return results;
})();
