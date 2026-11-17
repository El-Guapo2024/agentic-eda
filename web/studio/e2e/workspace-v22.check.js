// A workspace (`eda board serve --workspace <dir> -C <dir>/board`; PARITY-common.md section 33, "workspace v22"): several projects in one folder, one open
// at a time on the same address, as KiCad's project manager (KiCad 25ec2335 kicad/tools/kicad_manager_control.cpp NewProject / OpenProject):
//   chrome      the page's title is the project and the editor ("board — PCB Editor", "board — Schematic Editor"); the toolbar's New and Open are enabled;
//               File shows New Project..., Open Project... and Open Recent
//   new         File > New Project... refuses "../x" (a path) and the name of a project that exists; "fresh" makes <workspace>/fresh, which opens empty
//               (no footprint, no symbol) under its own title
//   open        File > Open Project... lists the projects newest first, the open one marked; opening "board" shows its placed footprints again
//   recent      File > Open Recent lists both, most recent first; its "fresh" entry opens fresh
//   reload      a reload keeps the project that is open; another page on the workspace follows a switch made elsewhere
//   end         "board" open again, no console errors
// Each switch loads the page again, so the studio runs in a frame of this page (the frame reloads, this script does not); this page's own timers are
// stopped first, so it does not follow the switches itself.
// It answers through `window.__workspaceV22Check`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30-placed minutes=6 workspace=1
(async () => {
  const results = [];
  const check = (window.__workspaceV22Check = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await new Promise((r) => setTimeout(r, Math.min(100, ms))); };
  check.wait = async (ms = 120000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const until = async (fn, ms = 20000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(150); return v; };
  for (let i = 0; i < 80 && !window.__eda; i++) await sleep(250);
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };
  const errorsBefore = window.__eda.errors().length;
  const initial = await json("/api/workspace");
  const home = initial.active;
  const placed = window.__eda.state().counts.footprints;

  // ------------------------------------------------------------------------------------------------------------------------------------------------ frames
  // a studio page in a frame: `load` waits for its test hook; `reloaded` for the page after a switch (a page without the mark this script left on the last)
  const frame = (id) => {
    const f = document.createElement("iframe");
    f.id = id;
    f.style.cssText = "position:fixed;left:0;top:0;width:1400px;height:900px;border:0;z-index:99999;background:#000";
    document.body.appendChild(f);
    const w = () => f.contentWindow;
    const doc = () => f.contentDocument;
    const ready = () => !!(w() && w().__eda && doc() && doc().readyState === "complete" && !w().__v22Mark && (doc().querySelector("[data-action]") || doc().querySelector('[data-testid="no-project"]')));
    return {
      f, w, doc,
      async load() { f.src = "/"; await until(ready, 60000); await sleep(800); w().__v22Mark = 1; },
      async reloaded() { const ok = await until(ready, 60000); await sleep(800); if (w()) w().__v22Mark = 1; return ok; },
      tid: (id) => doc().querySelector(`[data-testid="${id}"]`),
      button: (action) => doc().querySelector(`button[data-action="${action}"]`),
    };
  };
  const setValue = async (win, el, v) => { Object.getOwnPropertyDescriptor(win.HTMLInputElement.prototype, "value").set.call(el, String(v)); el.dispatchEvent(new win.Event("input", { bubbles: true })); await sleep(250); };
  const menuLabels = async (doc, menu) => {
    const item = [...doc.querySelectorAll(".menubar-item")].find((m) => m.firstChild && m.firstChild.textContent.trim() === menu);
    if (!item) return [];
    item.click();
    await sleep(300);
    const labels = [...item.querySelectorAll(".menu-node-item, .menu-node-label")].map((e) => e.textContent.replace(/\s+/g, " ").trim());
    item.click();
    await sleep(200);
    return labels;
  };

  // this page stops following the workspace (and polling): the frames are the studio from here on
  { const last = setInterval(() => {}, 1e6); for (let i = 0; i <= last; i++) clearInterval(i); }

  await scenario("chrome", async (t) => {
    t(initial.workspace && home === "board", `a workspace with "board" open: ${JSON.stringify({ workspace: initial.workspace, active: home })}`);
    t(document.title === `${home} — PCB Editor`, `the title is the project and the editor: ${document.title}`);
    for (const a of ["common.Control.new", "common.Control.open"]) {
      const b = document.querySelector(`button[data-action="${a}"]`);
      t(b && !b.disabled, `the toolbar's ${a} is enabled`);
    }
    const acts = window.__eda.actions();
    t(acts.some((a) => a.id === "common.Control.new" && a.enabled) && acts.some((a) => a.id === "common.Control.open" && a.enabled), "New and Open are handled");
    const labels = await menuLabels(document, "File");
    t(labels.some((l) => l.startsWith("New Project...")) && labels.some((l) => l.startsWith("Open Project...")) && labels.some((l) => l.startsWith("Open Recent")), `File menu: ${labels.slice(0, 6).join(" | ")}`);
    t(placed > 0, `the board has its placed footprints: ${placed}`);
  });

  // this page's errors up to here (it no longer follows the project from now on)
  const topErrors = window.__eda.errors().slice(errorsBefore);
  const A = frame("v22-a");
  await A.load();

  await scenario("new", async (t) => {
    t(A.doc().title === `${home} — PCB Editor`, `the frame shows ${home}: ${A.doc().title}`);
    A.button("common.Control.new").click();
    await sleep(400);
    const dlg = await until(() => A.tid("new-project-dialog"));
    t(!!dlg, "File > New Project... opens its dialog");
    const input = A.tid("new-project-name");
    await setValue(A.w(), input, "../x");
    t(/path separators/.test(A.tid("workspace-error")?.textContent ?? "") && A.tid("new-project-ok").disabled, `a path is refused: ${A.tid("workspace-error")?.textContent}`);
    await setValue(A.w(), input, home);
    t(/already exists/.test(A.tid("workspace-error")?.textContent ?? ""), `an existing project's name is refused: ${A.tid("workspace-error")?.textContent}`);
    const refused = await post("/api/workspace/new", { name: "a/b" });
    t(refused.ok === false && /path separators/.test(refused.message), `the server refuses a path too: ${refused.message}`);
    await setValue(A.w(), input, "fresh");
    t(!A.tid("workspace-error"), "a good name");
    A.tid("new-project-ok").click();
    t(await A.reloaded(), "the page loads again on the new project");
    t(A.doc().title === "fresh — PCB Editor", `its title: ${A.doc().title}`);
    const c = A.w().__eda.state().counts;
    t(c.footprints === 0 && c.symbols === 0 && c.tracks === 0, `an empty board and schematic: ${JSON.stringify(c)}`);
    const ws = await json("/api/workspace");
    t(ws.active === "fresh" && ws.projects.some((p) => p.name === "fresh" && p.path === `${ws.workspace}/fresh`), `<workspace>/fresh is open: ${JSON.stringify(ws.projects.map((p) => p.name))}`);
    const st = await json("/api/state");
    t(Array.isArray(st.parts) && st.parts.length === 0, `the server's board is fresh's: ${st.parts?.length} part(s)`);
  });

  await scenario("open", async (t) => {
    await A.w().__eda.run("common.Control.open");
    const dlg = await until(() => A.tid("open-project-dialog"));
    t(!!dlg, "File > Open Project... opens its dialog");
    const rows = [...dlg.querySelectorAll('[role="option"]')].map((r) => r.textContent);
    t(rows.length >= 2 && rows[0].startsWith("fresh (open)") && rows.some((r) => r.startsWith(home)), `newest first, the open one marked: ${JSON.stringify(rows)}`);
    A.tid(`open-project-${home}`).click();
    await sleep(200);
    A.tid("open-project-ok").click();
    t(await A.reloaded(), "the page loads again");
    t(A.doc().title === `${home} — PCB Editor`, `its title: ${A.doc().title}`);
    t(A.w().__eda.state().counts.footprints === placed, `its placed footprints are shown: ${A.w().__eda.state().counts.footprints} of ${placed}`);
    const badOpen = await post("/api/workspace/open", { name: "../board" });
    t(badOpen.ok === false, `a path is not opened: ${badOpen.message}`);
  });

  await scenario("recent", async (t) => {
    const labels = await menuLabels(A.doc(), "File");
    const at = labels.findIndex((l) => l.startsWith("Open Recent"));
    t(at >= 0 && labels.slice(at + 1, at + 3).join("|") === `${home}|fresh`, `Open Recent: ${labels.slice(at, at + 3).join(" | ")}`);
    const item = [...A.doc().querySelectorAll(".menubar-item")].find((m) => m.firstChild && m.firstChild.textContent.trim() === "File");
    item.click();
    await sleep(300);
    const entry = [...item.querySelectorAll(".menu-node-item")].find((e) => e.textContent.trim() === "fresh");
    t(!!entry, "its fresh entry");
    entry?.click();
    t(await A.reloaded(), "the page loads again");
    t(A.doc().title === "fresh — PCB Editor", `fresh is open: ${A.doc().title}`);
  });

  await scenario("reload", async (t) => {
    A.w().location.reload();
    t(await A.reloaded(), "reloaded");
    t(A.doc().title === "fresh — PCB Editor", `a reload keeps fresh: ${A.doc().title}`);
    // another page on the workspace follows a switch made elsewhere (an agent's `eda workspace open`, here the API)
    const B = frame("v22-b");
    await B.load();
    t(B.doc().title === "fresh — PCB Editor", `a second page on fresh: ${B.doc().title}`);
    const r = await post("/api/workspace/open", { name: home });
    t(r.ok === true && r.active === home, `the switch: ${JSON.stringify(r)}`);
    t(await B.reloaded(), "the second page loads again by itself");
    t(B.doc().title === `${home} — PCB Editor`, `on ${home}: ${B.doc().title}`);
    t(B.w().__eda.state().counts.footprints === placed, "with its board");
    await B.w().__eda.run("pcbnew.EditorControl.showEeschema").catch(() => null);
    await until(() => B.doc().title === `${home} — Schematic Editor`, 8000);
    t(B.doc().title === `${home} — Schematic Editor`, `the schematic editor's title: ${B.doc().title}`);
    t(A.w() == null || (await A.reloaded()), "the first page follows too");
    B.f.remove();
  });

  await scenario("end", async (t) => {
    const ws = await json("/api/workspace");
    t(ws.active === home, `${home} is open again: ${ws.active}`);
    t(JSON.stringify(ws.recent.slice(0, 2)) === JSON.stringify([home, "fresh"]), `the history: ${JSON.stringify(ws.recent)}`);
    const errs = topErrors;
    const frameErrs = A.w() ? A.w().__eda.errors().filter((e) => !/409|No project/.test(e.message)) : [];
    t(errs.length === 0 && frameErrs.length === 0, `no console errors: ${JSON.stringify([...errs, ...frameErrs].slice(0, 3))}`);
    A.f.remove();
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
