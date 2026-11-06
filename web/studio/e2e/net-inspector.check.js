// The docked Net Inspector (UI-LEFT P9; PARITY-pcb.md section 73): `PCB_NET_INSPECTOR_PANEL` as a pane under the canvas. It drives the pane's own controls (the
// filter, the configure button's menu, the rows' menu, the prompts) and reads the board back (`GET /api/state`, `GET /api/net_inspector`, `__eda.state()`):
//   toggle        View > Panels > Net Inspector (`pcbnew.Control.showNetInspector`) shows the pane under the canvas, checked; KiCad's columns; the rows listed
//   group         the configure menu's Group by Netclass: a group per class ("Netclass: <class>") before the nets, opened by its triangle; the setting is kept
//   custom        Add Custom Group... ("New Group", "Group name / pattern:", KiCad's help line): a "Custom: <pattern>" group with the matching nets; Remove All
//                 Custom Groups takes it away
//   time          Show Time Domain Details: the delay columns (Total Delay, Via Delay, Track Delay, Die Delay)
//   highlight     a double click on a row highlights its net; the rows' menu's Clear Net Highlighting clears it; a net highlighted on the board selects its row
//   rename        Rename Selected Net...: "Rename Net" with the short name, "Net name '<x>' is already in use." for a name in use, then the net renamed on every
//                 track, via and pad (one undo step brings it back)
//   delete        Delete Selected Net: "Net '<x>' is in use.  Delete anyway?"; with the studio's Strict on, a routed net is refused (its copper would short on
//                 no net), with Strict off Yes puts its pads and copper on no net (undo brings it back)
//   add           Add Net...: "New Net"; the same name again is refused ("already in use")
//   close         the pane's close button hides it; the action shows it again with the settings it had
//   end           every board step undone, no console errors
// It answers through `window.__netInspectorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-routed minutes=10
(async () => {
  const results = [];
  const run = (window.__netInspectorCheck = { results, done: false, ok: false, wait: null });
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
    await sleep(150);
  };
  const click = async (el, wait = 250) => { if (!el) throw new Error("nothing to click"); el.click(); await sleep(wait); };
  const dbl = async (el, wait = 300) => { if (!el) throw new Error("nothing to double-click"); el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await sleep(wait); };
  const rightClick = async (el, wait = 250) => { const r = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.top + 4 })); await sleep(wait); };
  const pane = () => document.querySelector('[data-pane="netInspector"]');
  const panel = () => document.querySelector('[data-testid="net-inspector"]');
  const rows = () => [...document.querySelectorAll('[data-testid="net-inspector-row"]')];
  const netRow = (net) => rows().find((r) => r.dataset.net === net);
  const groupRows = () => rows().filter((r) => r.dataset.group);
  const firstCell = (row) => row.querySelector('[data-col="name"]');
  const headings = () => [...document.querySelectorAll('[data-testid^="net-inspector-col-"]')].map((h) => h.textContent.replace(/[▲▼]/g, "").trim());
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")];
  // a check item's text starts with its mark when it is checked
  const menuItem = (label) => menuItems().find((m) => m.textContent.replace("✓", "").trim() === label);
  const closeMenu = async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await sleep(150); };
  const config = async () => { await click(document.querySelector('[data-testid="net-inspector-config"]')); };
  const prompt = () => document.querySelector('[data-testid="net-inspector-prompt"]');
  const promptTitle = () => prompt()?.querySelector(".dialog-header span")?.textContent.trim();
  const promptOk = async () => click(prompt().querySelector('[data-testid="net-inspector-prompt-ok"]'), 500);
  const stored = () => JSON.parse(localStorage.getItem("eda-studio.net-inspector.v1") ?? "{}");
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(500); };
  const netsOfCopper = (s) => new Set([...(s.routing?.tracks ?? []).map((x) => x.net), ...(s.routing?.vias ?? []).map((x) => x.net)]);

  localStorage.removeItem("eda-studio.net-inspector.v1");
  const before = await json("/api/state");
  const inspector = await json("/api/net_inspector");
  const errorsBefore = __eda.errors().length;
  // a routed net of a sheet (`/sheet/NAME`) with pads and copper, and another net of the same sheet, whose short name a rename may not take
  const routed = [...netsOfCopper(before)].filter((n) => n && inspector.nets.some((r) => r.name === n && r.pad_count > 0 && r.track_length > 0));
  const path = (n) => n.slice(0, n.lastIndexOf("/") + 1);
  const target = routed.find((n) => n.includes("/") && inspector.nets.some((r) => r.name !== n && path(r.name) === path(n))) ?? routed[0];
  const other = inspector.nets.find((r) => r.name !== target && path(r.name) === path(target) && r.pad_count > 0)?.name ?? inspector.nets.find((r) => r.name !== target && r.pad_count > 0)?.name;

  await scenario("View > Panels > Net Inspector shows the docked pane under the canvas, with KiCad's columns and the nets", async (t) => {
    t(!pane(), "hidden to begin with (aui.show_net_inspector)");
    await __eda.run("pcbnew.Control.showNetInspector");
    await sleep(800);
    t(!!pane() && !!document.querySelector('[data-testid="bottom-dock"]'), "the pane is in the bottom dock");
    t(!document.querySelector(".dialog-backdrop [data-testid=net-inspector]"), "not a dialog over the board");
    const h = headings();
    t(h.slice(0, 8).join("|") === "Name|Netclass|Total Length|Via Count|Via Length|Track Length|Die Length|Pad Count", `the columns (${h.join("|")})`);
    t(h.includes("F.Cu") && h.includes("B.Cu"), "a column per copper layer");
    t(rows().length > 3 && !!netRow(target), `the nets are listed (${rows().length}, ${target})`);
    const canvas = document.querySelector(".pcb-canvas-container").getBoundingClientRect();
    const dock = document.querySelector('[data-testid="bottom-dock"]').getBoundingClientRect();
    t(dock.top >= canvas.bottom - 2 && Math.abs(dock.left - canvas.left) < 4, `under the canvas (${Math.round(canvas.bottom)} / ${Math.round(dock.top)})`);
  });

  await scenario("Group by Netclass: a group per class before the nets, opened by its triangle; the setting is kept", async (t) => {
    await config();
    t(!!menuItem("Group by Netclass") && !!menuItem("Show Time Domain Details") && !!menuItem("Save Net Inspector Report...") && !!menuItem("Add Custom Group..."), `the configure menu (${menuItems().map((m) => m.textContent.trim()).filter(Boolean).join("|")})`);
    await click(menuItem("Group by Netclass"), 400);
    const groups = groupRows();
    t(groups.length >= 1 && groups.every((g) => firstCell(g).textContent.includes("Netclass: ")), `groups (${groups.map((g) => firstCell(g).textContent).join(", ")})`);
    t(rows()[0].dataset.group !== undefined && !rows().some((r) => r.dataset.net), "every net is in a group, the groups closed");
    t(stored().groupByNetclass === true, "kept in the person's settings");
    const g = groups.find((x) => x.dataset.group.startsWith("netclass:"));
    await click(g.querySelector('[data-testid="net-inspector-expander"]'), 300);
    const open = rows().filter((r) => r.dataset.net);
    t(open.length > 0, `the group opens on its nets (${open.length})`);
    const pads = Number(g.querySelector('[data-col="pad_count"]').textContent);
    const sum = open.reduce((s, r) => s + Number(r.querySelector('[data-col="pad_count"]').textContent), 0);
    t(pads === sum, `the group's Pad Count is its nets' sum (${pads} = ${sum})`);
    await config();
    await click(menuItem("Group by Netclass"), 300);
    t(groupRows().length === 0 && !!netRow(target), "unticked: the nets alone");
  });

  await scenario("Add Custom Group...: KiCad's prompt, a Custom group of the matching nets; Remove All Custom Groups", async (t) => {
    await config();
    await click(menuItem("Add Custom Group..."), 300);
    t(promptTitle() === "New Group" && prompt().textContent.includes("Group name / pattern:") && prompt().textContent.includes("(Use /.../ to indicate a regular expression.)"), `the prompt (${prompt()?.textContent})`);
    await setValue(prompt().querySelector("input"), target);
    await promptOk();
    const g = groupRows().find((r) => r.dataset.group === `custom:${target}`);
    t(!!g && firstCell(g).textContent.includes(`Custom: ${target}`), `the group (${groupRows().map((r) => firstCell(r).textContent).join(", ")})`);
    t(!netRow(target), "its net is under it, the group closed");
    t(JSON.stringify(stored().customGroupRules) === JSON.stringify([target]), "the rule is kept");
    await config();
    await click(menuItem("Remove All Custom Groups"), 300);
    t(groupRows().length === 0 && !!netRow(target), "gone again");
  });

  await scenario("Show Time Domain Details: the delay columns", async (t) => {
    await config();
    await click(menuItem("Show Time Domain Details"), 300);
    const h = headings();
    t(h.slice(2, 7).join("|") === "Total Delay|Via Count|Via Delay|Track Delay|Die Delay", `the columns (${h.join("|")})`);
    t(/ps$/.test(netRow(target).querySelector('[data-col="track_length"]').textContent.trim()), "in picoseconds");
    await config();
    await click(menuItem("Show Time Domain Details"), 300);
    t(headings()[2] === "Total Length", "back to the lengths");
  });

  await scenario("a double click highlights the net; Clear Net Highlighting; a net highlighted on the board selects its row", async (t) => {
    await click(netRow(target), 150);
    await dbl(netRow(target), 400);
    t(JSON.stringify(__eda.state().highlight) === JSON.stringify([target]), `highlighted (${JSON.stringify(__eda.state().highlight)})`);
    await rightClick(netRow(target));
    await click(menuItem("Clear Net Highlighting"), 300);
    t(__eda.state().highlight.length === 0, "cleared");
    t(netRow(target).getAttribute("aria-selected") === "true", "the row stays selected");
    // a highlight from the board: the Search pane's Nets tab highlights the net it selects, as KiCad's does
    await __eda.run("common.Interactive.search");
    await sleep(600);
    await click(document.querySelector('[data-testid="search-pane-tab-nets"]'), 300);
    const q = document.querySelector('[data-testid="search-pane-query"]');
    await setValue(q, other);
    const hit = [...document.querySelectorAll('[data-testid="search-pane-row"]')].find((r) => r.dataset.id === other);
    await click(hit, 500);
    t(netRow(other)?.getAttribute("aria-selected") === "true" && netRow(target).getAttribute("aria-selected") !== "true", `the board's highlight selects its row (${JSON.stringify(__eda.state().highlight)})`);
    await setValue(q, "");
    await __eda.run("common.Interactive.search");
    await sleep(300);
    await click(netRow(target), 150);
    await rightClick(netRow(target));
    await click(menuItem("Clear Net Highlighting"), 300);
  });

  let renamed = null;
  await scenario("Rename Selected Net...: KiCad's prompt and message, the net renamed everywhere, one undo step", async (t) => {
    await click(netRow(target), 150);
    await rightClick(netRow(target));
    t(!!menuItem("Rename Selected Net...") && !!menuItem("Delete Selected Net") && !!menuItem("Add Net...") && !!menuItem("Highlight Selected Net"), `the rows' menu (${menuItems().map((m) => m.textContent.trim()).filter(Boolean).join("|")})`);
    await click(menuItem("Rename Selected Net..."), 300);
    const short = target.slice(target.lastIndexOf("/") + 1);
    t(promptTitle() === "Rename Net" && prompt().querySelector("input").value === short, `"Rename Net" with the short name (${promptTitle()}, ${prompt()?.querySelector("input")?.value})`);
    const otherShort = other.slice(other.lastIndexOf("/") + 1);
    await setValue(prompt().querySelector("input"), otherShort);
    await promptOk();
    t(promptTitle() === "Error" && prompt().textContent.includes(`Net name '${otherShort}' is already in use.`), `the name in use is refused (${prompt()?.textContent})`);
    await promptOk();
    t(promptTitle() === "Rename Net" && prompt().querySelector("input").value === "", "the dialog again, empty");
    renamed = `${target.slice(0, target.lastIndexOf("/") + 1)}${short}_RENAMED`;
    await setValue(prompt().querySelector("input"), `${short}_RENAMED`);
    await promptOk();
    await sleep(800);
    const after = await json("/api/state");
    t(netsOfCopper(after).has(renamed) && !netsOfCopper(after).has(target), `the tracks and vias are on ${renamed}`);
    const pads = after.parts.flatMap((p) => p.pads ?? []);
    t(pads.some((p) => p.net === renamed) && !pads.some((p) => p.net === target), "the pads too");
    t(!!netRow(renamed) && netRow(renamed).getAttribute("aria-selected") === "true", "its row is listed and selected");
    await undo();
    const back = await json("/api/state");
    t(netsOfCopper(back).has(target) && !netsOfCopper(back).has(renamed), "one undo step brings the name back");
  });

  const strictBox = () => [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  await scenario("Delete Selected Net: KiCad's question; with Strict on a routed net is not deleted (its copper would be left on no net); off, its copper and pads on no net; undo", async (t) => {
    await sleep(500);
    const e0 = __eda.errors().length;
    const ask = async () => {
      await click(netRow(target), 150);
      await rightClick(netRow(target));
      await click(menuItem("Delete Selected Net"), 300);
    };
    await ask();
    const shown = target;
    t(prompt()?.textContent.includes(`Net '${shown}' is in use.  Delete anyway?`), `the question (${prompt()?.textContent})`);
    await promptOk();
    await sleep(800);
    t(netsOfCopper(await json("/api/state")).has(target), "Strict (the studio's gates, on by default) refuses it: the copper would short on no net");
    t(__eda.errors().slice(e0).some((e) => e.message.includes("refused because --strict")), "and says why");
    const box = strictBox();
    if (box?.checked) await click(box, 200);
    await ask();
    await promptOk();
    await sleep(800);
    const after = await json("/api/state");
    t(!netsOfCopper(after).has(target), "Strict off: no copper is on the net");
    t(!after.parts.flatMap((p) => p.pads ?? []).some((p) => p.net === target), "no pad is on it");
    t(!netRow(target), "gone from the list");
    await undo();
    t(netsOfCopper(await json("/api/state")).has(target), "undo brings it back");
    if (box && !box.checked) await click(box, 200);
  });

  await scenario("Add Net...: \"New Net\"; the same name again is refused", async (t) => {
    await sleep(400);
    await rightClick(netRow(target) ?? rows()[0]);
    await click(menuItem("Add Net..."), 300);
    t(promptTitle() === "New Net" && prompt().textContent.includes("Net name:"), `the prompt (${promptTitle()})`);
    await setValue(prompt().querySelector("input"), "NI_CHECK_NEW");
    const e0 = __eda.errors().length;
    const r1 = await (async () => { await promptOk(); await sleep(600); return __eda.errors().length; })();
    t(r1 === e0, "added without an error");
    await rightClick(rows()[0]);
    await click(menuItem("Add Net..."), 300);
    await setValue(prompt().querySelector("input"), "NI_CHECK_NEW");
    await promptOk();
    await sleep(700);
    const errs = __eda.errors().slice(e0).map((e) => e.message);
    t(errs.some((m) => m.includes("Net name 'NI_CHECK_NEW' is already in use.")), `refused the second time (${errs.join(" | ")})`);
    await undo();
  });

  await scenario("the close button hides the pane; the action shows it again with its settings", async (t) => {
    await config();
    await click(menuItem("Group by Netclass"), 300);
    await click(pane().querySelector(".dock-panel-close"), 300);
    t(!pane(), "hidden");
    await __eda.run("pcbnew.Control.showNetInspector");
    await sleep(800);
    t(!!pane() && groupRows().length > 0, "shown again, still grouped");
    await config();
    await click(menuItem("Group by Netclass"), 300);
    await __eda.run("pcbnew.Control.showNetInspector");
    await sleep(300);
    t(!pane(), "the action hides it too");
  });

  await scenario("the board is as it was; no console errors", async (t) => {
    const after = await json("/api/state");
    t(JSON.stringify([...netsOfCopper(after)].sort()) === JSON.stringify([...netsOfCopper(before)].sort()), "the copper's nets are the ones it had");
    const errs = __eda.errors().slice(errorsBefore).map((e) => e.message).filter((m) => !m.includes("already in use") && !m.includes("refused because --strict"));
    t(errs.length === 0, `no errors (${errs.join(" | ")})`);
  });

  run.ok = results.every((r) => r.ok);
  run.done = true;
  return results;
})();
