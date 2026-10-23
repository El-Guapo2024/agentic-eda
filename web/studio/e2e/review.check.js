// The review workflow's leftovers (GAPS.md package P15), driven through `window.__eda` (src/actions/useEdaTestHook.ts): in both Checker windows Delete Marker and Delete All Markers (and what they
// do to the exclusions, which KiCad records from the markers that are left), a marker selected by a left click on the canvas, "Exclude all violations of rule", the Inspect and Fix entries of the
// marker menus, an ERC exclusion with a comment that kicad-cli honours, Save... (the file kicad-cli writes, in both formats) and Edit connection grid spacing (Schematic Setup > Formatting).
//
// It is one expression that resolves to a list of { name, ok, detail }: paste it into the Browser pane's javascript tool, or hand it to Playwright's `page.evaluate`. It changes the board it runs
// on (two tracks, a custom rule, a wire, exclusions, the connection grid), so serve a PRISTINE scratch COPY of a board -- one with nothing excluded yet -- and a kicad-cli on the PATH or in
// EDA_KICAD_CLI (every run goes through kicad-cli, a few seconds each):
//
//   target/debug/eda board serve -C <scratch copy> --port 8830 --ui web/studio/dist     # after `cd web/studio && npm run build`
//   open http://127.0.0.1:8830/ in a tab of your own, then evaluate this file
//
// It needs a placed board whose schematic has module sheets with wired symbols and some silkscreen warnings (examples/mcu_board_30plus.yaml placed by `eda place`, work/mcu30): the check makes
// its own findings -- two tracks too close together, a wire left hanging, two symbol pins left unwired -- and puts them back at the end. The canvas clicks need the pages laid out, and a Browser
// pane that is not showing draws no frame: give the tab a size (`resize_window`, 1400 x 900), open the PCB and the Schematic tab once and take a screenshot of each before starting. The Browser
// pane's javascript tool gives up on a call after 45 s, so start it without waiting and read the answer later:
//
//   window.__checkResults = null; window.__checkPromise = (0, eval)(src).then((r) => { window.__checkResults = r; });   // src = this file's text
//   ... later: window.__checkResults
//
// (a copy of this file in web/studio/dist is served by the same server, so `src = await (await fetch("/review.check.js")).text()`). `window.__reviewOnly = ["pcb"]` (or ["sch"]) runs one half.
// Nothing is downloaded: the page's file save is held back and its text read instead.
// run-all-checks: board=mcu30-placed minutes=15   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const eda = window.__eda;
  const results = [];
  const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail: String(detail) });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (fn, ms = 20000) => {
    const t = Date.now();
    while (Date.now() - t < ms) {
      try {
        const v = fn();
        if (v) return v;
      } catch {
        // not there yet
      }
      await sleep(150);
    }
    return null;
  };
  const only = window.__reviewOnly ?? null;
  const json = async (url, init) => (await fetch(url, init)).json();
  const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
  const undo = (domain) => json("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain }) });

  // ---------------------------------------------------------------------------------------------------------------------------------- the page
  const dialogOf = (title) => [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith(title));
  const rows = () => [...document.querySelectorAll(".dialog .problem-row")];
  const rowIndex = (row) => Number(row.dataset.rcIndex);
  const rowAt = (index) => rows().find((r) => rowIndex(r) === index);
  const selectedRow = () => rows().find((r) => r.classList.contains("selected"));
  const badges = () => document.querySelector(".dialog .rc-badges")?.innerText.replace(/\n/g, " | ") ?? "";
  const count = (word) => Number(new RegExp(`(\\d+) ${word}`).exec(badges())?.[1] ?? NaN);
  const button = (label) => [...document.querySelectorAll(".dialog button")].find((b) => b.innerText.trim().startsWith(label));
  const footer = (title, label) => [...(dialogOf(title)?.querySelectorAll(".dialog-footer button") ?? [])].find((b) => b.innerText.trim() === label);
  const page = (label) => [...document.querySelectorAll(".dialog .dock-tab")].find((t) => t.innerText.trim().startsWith(label));
  const rightClick = async (el) => {
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 250, button: 2 }));
    await sleep(150);
  };
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  const menuLabels = () => menuItems().map((e) => e.innerText);
  const pick = async (label) => {
    const it = menuItems().find((e) => e.innerText.startsWith(label));
    if (!it) throw new Error(`no menu entry "${label}" in ${JSON.stringify(menuLabels())}`);
    it.click();
    await sleep(300);
  };
  // A run in flight: the Run button says "Running…" (and `button("Run ...")` finds nothing meanwhile).
  const busy = () => [...document.querySelectorAll(".dialog button")].some((b) => /^Running/.test(b.innerText.trim()));
  const idle = async (ms = 60000) => {
    await until(busy, 2500);
    return until(() => !busy(), ms);
  };
  const closeDialog = async (title) => {
    dialogOf(title)?.querySelector(".dialog-footer button.primary")?.click();
    await sleep(250);
  };
  const goTab = async (name) => {
    [...document.querySelectorAll("button, div")].find((e) => e.innerText === name && /tab/.test(e.className))?.click();
    await sleep(1200);
  };
  const setValue = (el, value) => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const answerComment = async (text) => {
    const prompt = await until(() => document.querySelector('textarea[aria-label="Exclusion comment"]'), 3000);
    if (!prompt) return false;
    setValue(prompt, text);
    await sleep(100);
    [...document.querySelectorAll(".dialog button")].find((x) => x.innerText === "OK").click();
    await sleep(1200);
    return true;
  };
  // The canvas, and the pointer on it: dispatched events, as the other checks do.
  const canvas = () => [...document.querySelectorAll("canvas")].sort((a, b) => b.width * b.height - a.width * a.height)[0];
  const w2c = (wx, wy) => {
    const v = eda.state().view;
    const r = canvas().getBoundingClientRect();
    return [r.left + v.x + wx * v.scale, r.top + v.y + wy * v.scale];
  };
  const fire = (type, x, y, buttons = 0) => canvas().dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pointerId: 7, pointerType: "mouse", isPrimary: true, button: 0, buttons }));
  const clickAt = async (wx, wy) => {
    const [x, y] = w2c(wx, wy);
    fire("pointermove", x, y);
    await sleep(150);
    fire("pointerdown", x, y, 1);
    await sleep(60);
    fire("pointerup", x, y);
    await sleep(500);
  };
  // "Save..." hands the report to the browser as a file; hold that back and keep what it was given.
  const saved = [];
  const realCreate = URL.createObjectURL;
  const realClick = HTMLAnchorElement.prototype.click;
  const holdDownloads = () => {
    URL.createObjectURL = (blob) => {
      const url = realCreate.call(URL, blob);
      saved.push({ url, blob, name: null });
      return url;
    };
    HTMLAnchorElement.prototype.click = function () {
      if (this.download) {
        const s = saved.find((d) => d.url === this.href);
        if (s) s.name = this.download;
        return;
      }
      return realClick.call(this);
    };
  };
  const releaseDownloads = () => {
    URL.createObjectURL = realCreate;
    HTMLAnchorElement.prototype.click = realClick;
  };
  const nextSave = async (n, ms = 60000) => {
    const s = await until(() => saved.length > n && saved[n].name !== null && saved[n], ms);
    return s ? { name: s.name, text: await s.blob.text() } : null;
  };

  // A pane that is not showing never draws a frame, so the canvas has no size and the view no scale; every click on the canvas below needs both.
  const laidOut = async () => (await until(() => (eda.state().view?.scale ?? 0) > 0, 5000)) !== null;

  // ============================================================================================================================ the board
  const board = async () => {
    await goTab("PCB");
    if (eda.state().tab !== "pcb") return check("the PCB tab opens", false, eda.state().tab);
    if (!(await laidOut())) return check("the board is drawn (a hidden Browser pane draws nothing: show it, or take a screenshot, and start again)", false, JSON.stringify(eda.state().view));

    // Two tracks of two nets 50 um apart, where nothing else is: a clearance violation (its menu has an Inspect entry) and, as they join nothing, dangling tracks (which have a Fix entry).
    const state = await json("/api/state");
    const padNets = [...new Set(state.parts.flatMap((p) => (p.pads ?? []).map((pad) => pad.net).filter(Boolean)))];
    const boxes = state.parts.map((p) => p.courtyard ?? p.body).filter(Boolean);
    const xs = state.outline.map((p) => p[0]);
    const ys = state.outline.map((p) => p[1]);
    let spot = null;
    for (let y = Math.min(...ys) + 3000; y < Math.max(...ys) - 3000 && !spot; y += 1000) {
      for (let x = Math.min(...xs) + 3000; x < Math.max(...xs) - 8000 && !spot; x += 1000) {
        const free = boxes.every(([x0, y0, x1, y1]) => x + 4000 < x0 - 1000 || x - 1000 > x1 || y + 1500 < y0 - 1000 || y - 1500 > y1);
        if (free) spot = [x, y];
      }
    }
    check("the board has room for two tracks to get too close", !!spot && padNets.length >= 2, JSON.stringify(spot));
    if (!spot) return;
    const before = eda.state().revision;
    const t1 = await post({ op: "add_track", net: padNets[0], layer: "F.Cu", width: 250, pts: [{ x: spot[0], y: spot[1] }, { x: spot[0] + 3000, y: spot[1] }] });
    const t2 = await post({ op: "add_track", net: padNets[1], layer: "F.Cu", width: 250, pts: [{ x: spot[0], y: spot[1] + 300 }, { x: spot[0] + 3000, y: spot[1] + 300 }] });
    check("two tracks 50 um apart are added", t1.ok && t2.ok, JSON.stringify([t1.message, t2.message]));
    await until(() => eda.state().revision !== before);

    const ran = await eda.run("pcbnew.DRCTool.runDRC");
    check("Run DRC opens the Design Rules Checker", ran.ok && /Design Rules Checker/.test(ran.dialog ?? ""), ran.dialog);
    const loaded = await until(() => rows().length >= 3 && !busy(), 60000);
    check("the Violations page lists the findings", !!loaded, `${rows().length} rows`);
    if (!loaded) return;
    const TITLE = "Design Rules Checker";
    const report = async () => json("/api/drc");
    const first = await report();
    const total = rows().length;
    check("the report on screen is the server's", first.violations.length === total, `${first.violations.length} / ${total}`);

    // ----------------------------------------------------------------------------------------------------------------------- Delete Marker
    check("Delete Marker waits for a marker to be selected, and Delete All Markers and Save... are on", footer(TITLE, "Delete Marker")?.disabled === true && footer(TITLE, "Delete All Markers")?.disabled === false && footer(TITLE, "Save...")?.disabled === false);
    const silk = first.violations.findIndex((v) => v.type === "silk_overlap");
    rowAt(silk).click();
    await sleep(300);
    check("a click on a row selects its marker and turns Delete Marker on", selectedRow() && rowIndex(selectedRow()) === silk && footer(TITLE, "Delete Marker").disabled === false, `row ${selectedRow() ? rowIndex(selectedRow()) : "none"}`);
    const warnings = count("warning");
    footer(TITLE, "Delete Marker").click();
    await sleep(600);
    check("Delete Marker takes the marker off the list and the counts follow", rows().length === total - 1 && count("warning") === warnings - 1, `${rows().length} rows, ${warnings} -> ${count("warning")} warnings`);
    check("and selects the row that followed it (the list closes up: it has the deleted one's place)", selectedRow() && rowIndex(selectedRow()) === silk, selectedRow() ? rowIndex(selectedRow()) : "none");
    check("the design is untouched: the server still has every violation", (await report()).violations.length === total);
    // The list on the page has lost a marker the server's report still has; a run puts the two in step again, with the deleted violation found again.
    const rerun = async () => {
      button("Run DRC").click();
      await idle();
      return rows().length >= total;
    };
    check("Run DRC finds the deleted violation again", !!(await rerun()) && rows().length === total, `${rows().length} rows of ${total}`);

    // ------------------------------------------------------------------------------------------------------------- a left click on a marker
    // A marker nothing else is near, so that the circle under the pointer is the one the click means.
    const near = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]) < 3000;
    const alone = first.violations.findIndex((v, i) => v.items[0]?.pos && first.violations.every((w, j) => j === i || !w.items[0]?.pos || !near(v.items[0].pos, w.items[0].pos)));
    check("there is a marker to click on the board", alone >= 0, alone);
    if (alone >= 0) {
      const at = first.violations[alone].items[0].pos;
      await eda.run("common.Interactive.selectAll");
      await sleep(300);
      const picked = eda.state().selection.length;
      await clickAt(at[0], at[1]);
      check("a left click on a marker selects it and its row in the dialog", selectedRow() && rowIndex(selectedRow()) === alone, `row ${selectedRow() ? rowIndex(selectedRow()) : "none"}, wanted ${alone}`);
      check("the marker is the selection: the items that were selected are not", picked > 0 && eda.state().selection.length === 0, `${picked} selected before, ${eda.state().selection.length} after`);
    }

    // ------------------------------------------------------------------- an excluded marker that is deleted takes its exclusion with it
    const key = (v) => JSON.stringify([v.type, v.items.map((i) => i.uuid)]);
    const waivedNow = async () => (await report()).violations.filter((v) => v.excluded).map(key);
    const silkRows = () => rows().filter((r) => /Silkscreen clearance/.test(r.innerText) && !r.classList.contains("excluded"));
    const none = (await waivedNow()).length;
    const victim = silkRows()[1];
    await rightClick(victim);
    await pick("Exclude with comment");
    check("Exclude with comment asks for the comment", await answerComment("known and accepted"));
    const commented = () => rows().find((r) => /known and accepted/.test(r.innerText));
    check("the marker is excluded, with its comment", commented()?.classList.contains("excluded"), commented()?.innerText.replace(/\n/g, " / "));
    check("the design holds the exclusion", (await waivedNow()).length === none + 1);
    commented().click();
    await sleep(300);
    const withExcluded = rows().length;
    footer(TITLE, "Delete Marker").click();
    await sleep(1500);
    check("Delete Marker on the excluded marker takes it off the list", rows().length === withExcluded - 1 && !commented());
    check("and its exclusion goes with it, as KiCad records only the markers that are left", (await waivedNow()).length === none, JSON.stringify(await waivedNow()));
    await undo("pcb");
    await sleep(800);
    check("Undo brings the exclusion back", (await waivedNow()).length === none + 1);
    await undo("pcb");
    await sleep(500);
    check("and another takes it away: the design is as it was", (await waivedNow()).length === none);

    // ---------------------------------------------------------------------------------------------------------------------- Delete All Markers
    // Two exclusions, so that Delete All Markers has the question to ask.
    for (let i = 0; i < 2; i++) {
      await rightClick(silkRows()[0]);
      await pick("Exclude this violation");
      await sleep(800);
    }
    check("two violations are excluded", (await waivedNow()).length === none + 2, (await waivedNow()).length);
    const unconnected = page("Unconnected Items");
    footer(TITLE, "Delete All Markers").click();
    await sleep(300);
    check("with exclusions in the report Delete All Markers asks whether they go too", /Delete exclusions too\?/.test(dialogOf(TITLE).innerText) && !!footer(TITLE, "Errors and Warnings Only") && !!footer(TITLE, "Errors, Warnings and Exclusions") && !!footer(TITLE, "Cancel"));
    footer(TITLE, "Cancel").click();
    await sleep(200);
    check("Cancel leaves everything", rows().length > 2 && !!footer(TITLE, "Delete Marker"));
    footer(TITLE, "Delete All Markers").click();
    await sleep(200);
    footer(TITLE, "Errors and Warnings Only").click();
    await sleep(600);
    check("Errors and Warnings Only leaves the excluded markers", rows().length === 2 && rows().every((r) => r.classList.contains("excluded")), `${rows().length} rows`);
    check("and the counts are zero but for the exclusions", count("error") === 0 && count("warning") === 0 && count("excluded") === 2, badges());
    check("the exclusions are still the design's", (await waivedNow()).length === none + 2);
    unconnected?.click();
    await sleep(200);
    check("the Unconnected Items page is empty too", rows().length === 0, `${rows().length} rows`);
    page("Violations")?.click();
    await sleep(200);
    footer(TITLE, "Delete All Markers").click();
    await sleep(200);
    footer(TITLE, "Errors, Warnings and Exclusions").click();
    await sleep(1500);
    check("Errors, Warnings and Exclusions empties the list", rows().length === 0 && /All markers were deleted/.test(dialogOf(TITLE).innerText), dialogOf(TITLE).innerText.slice(0, 200).replace(/\n/g, " | "));
    check("and the design forgets every exclusion, in one step", (await waivedNow()).length === 0);
    await undo("pcb");
    await sleep(800);
    check("Undo brings both back", (await waivedNow()).length === none + 2);
    await undo("pcb");
    await undo("pcb");
    await sleep(500);
    check("and two more take them away: the design is as it was", (await waivedNow()).length === none);

    // ------------------------------------------------------------------------------------------------- Run DRC again: the violations are found again
    const found = await rerun();
    check("Run DRC finds every violation again, none of them excluded", !!found && rows().length === total && rows().every((r) => !r.classList.contains("excluded")) && (await waivedNow()).length === 0, `${rows().length} rows`);

    // ------------------------------------------------------------------------------------------------------------------------- Inspect and Fix
    const now = await report();
    const clearance = now.violations.findIndex((v) => v.type === "clearance");
    const dangling = now.violations.findIndex((v) => v.type === "track_dangling");
    check("the two tracks are a clearance violation", clearance >= 0, JSON.stringify(now.violations.map((v) => v.type).filter((t, i, a) => a.indexOf(t) === i)));
    if (clearance >= 0) {
      await rightClick(rowAt(clearance));
      const labels = menuLabels();
      check("a clearance violation's menu has Clearance Resolution", labels.includes("Clearance Resolution"), JSON.stringify(labels));
      check("with its own separator before the severities", labels.indexOf("Clearance Resolution") > labels.indexOf("Exclude all 'Clearance' violations") && labels.indexOf("Clearance Resolution") < labels.findIndex((l) => l.startsWith("Change severity")), JSON.stringify(labels));
      await pick("Clearance Resolution");
      const report1 = await until(() => dialogOf("Clearance Report"), 8000);
      check("Clearance Resolution opens the report of the two items", !!report1 && /Clearance/.test(report1.innerText), JSON.stringify(eda.state().dialogs));
      if (report1) await closeDialog("Clearance Report");
    }
    if (dangling >= 0) {
      await rightClick(rowAt(dangling));
      const labels = menuLabels();
      check("a dangling track's menu has the Fix entry Cleanup Tracks & Vias", labels.includes("Cleanup Tracks & Vias..."), JSON.stringify(labels));
      await pick("Cleanup Tracks & Vias");
      const opened = await until(() => /Cleanup Tracks/.test(eda.state().dialogs.join("|")), 8000);
      check("and it opens that dialog", !!opened, JSON.stringify(eda.state().dialogs));
      if (opened) {
        const d = [...document.querySelectorAll(".dialog")].find((x) => /Cleanup Tracks/.test(x.innerText));
        [...d.querySelectorAll("button")].find((b) => /Close|Cancel/.test(b.innerText))?.click();
        await sleep(300);
      }
    } else check("the tracks are dangling (their Fix entry is on the menu)", false, "no track_dangling in the report: " + JSON.stringify(now.violations.map((v) => v.type)));
    const plain = first.violations.findIndex((v) => v.type === "silk_overlap");
    await rightClick(rowAt(plain));
    check("a silkscreen violation has neither: its menu is the plain one", !menuLabels().some((l) => /Resolution|Cleanup|Update|Edit .*Properties|Compare/.test(l)), JSON.stringify(menuLabels()));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await sleep(200);

    // ------------------------------------------------------------------------------------------------ Exclude all violations of rule '...'
    await post({ op: "set_custom_rules", text: '(version 1)\n(rule "silk_apart"\n  (constraint silk_clearance (min 2mm)))\n' });
    await sleep(800);
    button("Run DRC").click();
    const ruled = await until(() => rows().some((r) => /rule 'silk_apart'/.test(r.innerText)), 60000);
    check("a violation of a custom rule names the rule in its text", !!ruled, rows()[0]?.innerText.replace(/\n/g, " / "));
    if (ruled) {
      const ofRule = rows().filter((r) => /rule 'silk_apart'/.test(r.innerText));
      await rightClick(ofRule[0]);
      const labels = menuLabels();
      check("its menu offers Exclude all violations of rule 'silk_apart'", labels.some((l) => l.startsWith("Exclude all violations of rule 'silk_apart'")), JSON.stringify(labels));
      check("and the one for the whole check as well", labels.some((l) => l.startsWith("Exclude all '")));
      await pick("Exclude all violations of rule 'silk_apart'");
      await until(() => rows().filter((r) => /rule 'silk_apart'/.test(r.innerText) && r.classList.contains("excluded")).length === ofRule.length, 15000);
      const excludedOfRule = rows().filter((r) => /rule 'silk_apart'/.test(r.innerText) && r.classList.contains("excluded")).length;
      check("it excludes every violation of that rule, and only those", excludedOfRule === ofRule.length && rows().filter((r) => r.classList.contains("excluded")).length === ofRule.length, `${excludedOfRule} of ${ofRule.length}, ${rows().filter((r) => r.classList.contains("excluded")).length} excluded`);
      await rightClick(rows().find((r) => r.classList.contains("excluded")));
      check("an excluded one offers to remove all exclusions of the rule", menuLabels().some((l) => l.startsWith("Remove all exclusions for violations of rule 'silk_apart'")), JSON.stringify(menuLabels()));
      await pick("Remove all exclusions for violations of rule 'silk_apart'");
      await until(() => rows().every((r) => !r.classList.contains("excluded")), 15000);
      check("and removing them puts every one back", rows().every((r) => !r.classList.contains("excluded")));
    }
    await post({ op: "set_custom_rules", text: "" });

    // ----------------------------------------------------------------------------------------------------------------------------- Save...
    holdDownloads();
    try {
      const n = saved.length;
      footer(TITLE, "Save...").click();
      await sleep(200);
      check("Save... offers the two file types", menuLabels().join("|") === "Report file (*.rpt)|JSON file (*.json)", JSON.stringify(menuLabels()));
      await pick("Report file");
      const rpt = await nextSave(n);
      check("the report file is DRC.rpt, kicad-cli's text report", rpt && rpt.name === "DRC.rpt" && /\*\* Drc report for /.test(rpt.text) && /\*\* Found \d+ DRC violations \*\*/.test(rpt.text), rpt ? `${rpt.name}: ${rpt.text.slice(0, 80).replace(/\n/g, " ")}` : "nothing saved");
      footer(TITLE, "Save...").click();
      await sleep(200);
      await pick("JSON file");
      const jsn = await nextSave(n + 1);
      let parsed = null;
      try {
        parsed = jsn && JSON.parse(jsn.text);
      } catch {
        // reported below
      }
      check("the JSON file is DRC.json, kicad-cli's JSON report", jsn && jsn.name === "DRC.json" && parsed && Array.isArray(parsed.violations) && parsed.violations.length > 0 && Array.isArray(parsed.unconnected_items), jsn ? jsn.name : "nothing saved");
      check("with every violation, as the Show boxes list them: a fresh run, the markers deleted from the list are in it", parsed && parsed.violations.length === total, parsed ? `${parsed.violations.length} violations, ${total} listed` : "");
      // With the Show boxes for warnings off, the report holds the errors alone.
      const show = [...document.querySelectorAll(".dialog .rc-show input")];
      const warningsBox = show[2];
      warningsBox.click();
      await sleep(200);
      footer(TITLE, "Save...").click();
      await sleep(200);
      await pick("JSON file");
      const errorsOnly = await nextSave(n + 2);
      const errs = errorsOnly ? JSON.parse(errorsOnly.text) : null;
      check("a Show box that is off leaves its severity out of the saved report", errs && errs.violations.every((v) => v.severity === "error"), errs ? JSON.stringify([...new Set(errs.violations.map((v) => v.severity))]) : "nothing saved");
      warningsBox.click();
      await sleep(200);
    } finally {
      releaseDownloads();
    }
    await closeDialog(TITLE);
    // Put the board back as it was found: the two tracks go (every other step of this check undid itself).
    const mine = ((await json("/api/state")).routing?.tracks ?? []).filter((t) => t.width === 250 && t.pts[0][0] === spot[0] && (t.pts[0][1] === spot[1] || t.pts[0][1] === spot[1] + 300));
    for (const t of mine) await post({ op: "delete_track", id: t.id });
    await sleep(500);
    const left = (await json("/api/state")).routing?.tracks ?? [];
    check("the two tracks are taken away again and no exclusion is left", mine.length === 2 && !left.some((t) => mine.some((m) => m.id === t.id)) && (await waivedNow()).length === none, `${mine.length} found, ${left.length} tracks left`);
  };

  // ======================================================================================================================== the schematic
  const schematic = async () => {
    await goTab("Schematic");
    if (eda.state().tab !== "schematic") return check("the Schematic tab opens", false, eda.state().tab);
    if (!(await laidOut())) return check("the sheet is drawn (a hidden Browser pane draws nothing: show it, or take a screenshot, and start again)", false, JSON.stringify(eda.state().view));
    const TITLE = "Electrical Rules Checker";
    const report = async () => json("/api/erc");
    const runErc = async () => {
      button("Run ERC").click();
      await idle();
      return rows().length > 0;
    };

    // A wire on the 50 mil grid and off the 100 mil one, joining nothing, below everything on the sheet: its ends are findings (Unconnected wire endpoint), and with a 100 mil connection
    // grid they are off the grid too.
    const sheet = await json("/api/schematic");
    const points = [...sheet.sheets.flatMap((x) => [x.at, [x.at[0] + x.size[0], x.at[1] + x.size[1]]]), ...sheet.symbols.map((x) => x.at), ...sheet.wires.flatMap((x) => x.pts), ...sheet.labels.map((x) => x.at), ...(sheet.power_symbols ?? []).map((x) => x.at), ...(sheet.no_connects ?? []).map((x) => x.at)];
    const odd = (v) => (Math.ceil(v / 1270) % 2 === 0 ? Math.ceil(v / 1270) + 1 : Math.ceil(v / 1270)) * 1270;
    const wireAt = [odd(Math.min(...points.map((p) => p[0]))), odd(Math.max(...points.map((p) => p[1])) + 10000)];
    const before = eda.state().revision;
    const w = await post({ op: "add_wire", pts: [{ x: wireAt[0], y: wireAt[1] }, { x: wireAt[0] + 5080, y: wireAt[1] }] });
    check("a wire is added", w.ok, w.message);
    // Symbol pins that join nothing: two wires that land on them are deleted from a module sheet (the board's own schematic is wired whole).
    let deleted = 0;
    const removed = [];
    for (const child of sheet.sheets) {
      const inner = await json(`/api/schematic?sheet=${child.id}`);
      for (const wire of inner.wires.filter((x) => x.pins.some((p) => /^[A-Za-z]+\d+\.\d+$/.test(p)))) {
        if (deleted >= 2) break;
        if ((await post({ op: "on_sheet", sheet: child.id, cmd: { op: "delete_wire", id: wire.id } })).ok) {
          deleted++;
          removed.push({ sheet: child.id, pts: wire.pts });
        }
      }
      if (deleted >= 2) break;
    }
    check("two wires that land on symbol pins are deleted", deleted === 2, deleted);
    await until(() => eda.state().revision !== before);

    const ran = await eda.run("eeschema.InspectionTool.runERC");
    check("Run ERC opens the Electrical Rules Checker", ran.ok && /Electrical Rules Checker/.test(ran.dialog ?? ""), ran.dialog);
    const loaded = await until(() => rows().length >= 3 && !busy(), 60000);
    check("the Violations page lists the findings", !!loaded, `${rows().length} rows`);
    if (!loaded) return;
    let first = await report();
    const total = rows().length;
    check("the report on screen is the server's", first.violations.length === total, `${first.violations.length} / ${total}`);

    // ----------------------------------------------------------------------------------------------------------------------- Delete Marker
    check("Delete Marker waits for a marker to be selected", footer(TITLE, "Delete Marker")?.disabled === true && footer(TITLE, "Delete All Markers")?.disabled === false && footer(TITLE, "Save...")?.disabled === false);
    rowAt(0).click();
    await sleep(300);
    check("a click on a row selects its marker and turns Delete Marker on", selectedRow() && rowIndex(selectedRow()) === 0 && footer(TITLE, "Delete Marker").disabled === false);
    const kind0 = first.violations[0].severity === "warning" ? "warning" : "error";
    const n0 = count(kind0);
    footer(TITLE, "Delete Marker").click();
    await sleep(600);
    check("Delete Marker takes the marker off the list and the counts follow", rows().length === total - 1 && count(kind0) === n0 - 1, `${rows().length} rows, ${n0} -> ${count(kind0)} ${kind0}s`);
    check("and selects the row that followed it (the list closes up: it has the deleted one's place)", selectedRow() && rowIndex(selectedRow()) === 0, selectedRow() ? rowIndex(selectedRow()) : "none");
    check("the design is untouched: the server still has every finding", (await report()).violations.length === total);
    // The list on the page has lost a marker the server's report still has; a run puts the two in step again, with the deleted finding found again.
    check("Run ERC finds the deleted finding again", !!(await runErc()) && rows().length === total, `${rows().length} rows of ${total}`);
    first = await report();

    // ------------------------------------------------------------------------------------------------------------- a left click on a marker
    const sch = await json("/api/schematic");
    const wire = sch.wires.find((x) => x.pts[0][0] === wireAt[0] && x.pts[0][1] === wireAt[1]);
    const ofWire = first.violations.findIndex((v) => v.check === "wire_dangling" && wire && v.location === wire.id);
    check("the wire's ends are reported", ofWire >= 0, JSON.stringify([...new Set(first.violations.map((v) => v.check))]));
    if (ofWire >= 0) {
      await clickAt(wire.pts[0][0], wire.pts[0][1]);
      const hit = selectedRow();
      check("a left click on a marker on the sheet selects its row in the dialog", hit && first.violations[rowIndex(hit)].location === wire.id, hit ? `row ${rowIndex(hit)}: ${first.violations[rowIndex(hit)].check} ${first.violations[rowIndex(hit)].location}` : "none");
    }

    // ------------------------------------------------------------------------- an ERC exclusion with a comment reaches kicad-cli
    const symbolPin = first.violations.findIndex((v, i) => i !== 0 && v.check === "pin_not_connected" && v.severity !== "excluded" && /^[A-Za-z]+\d+\.\d+$/.test(v.location ?? ""));
    check("a symbol's unconnected pin is reported", symbolPin >= 0);
    if (symbolPin >= 0) {
      await rightClick(rowAt(symbolPin));
      const labels = menuLabels();
      check("the finding's menu has Exclude with comment and Exclude all", labels.some((l) => l.startsWith("Exclude with comment")) && labels.some((l) => l.startsWith("Exclude all '")), JSON.stringify(labels));
      await pick("Exclude with comment");
      check("Exclude with comment asks for the comment", await answerComment("left open on purpose"));
      check("the finding is excluded, with its comment under it", rowAt(symbolPin)?.classList.contains("excluded") && /left open on purpose/.test(rowAt(symbolPin).innerText), rowAt(symbolPin)?.innerText.replace(/\n/g, " / "));
      check("the marker kicad-cli reported goes with it", !!first.violations[symbolPin].marker && first.violations[symbolPin].marker.error_type === "ERCET_PIN_NOT_CONNECTED", JSON.stringify(first.violations[symbolPin].marker));
      await runErc();
      const after = await report();
      const waived = after.violations.find((v) => v.check === "pin_not_connected" && v.location === first.violations[symbolPin].location);
      check("on the next run kicad-cli itself reports it excluded, with the comment", waived && waived.severity === "excluded" && waived.kicad_matched === true && waived.comment === "left open on purpose", JSON.stringify(waived && { severity: waived.severity, matched: waived.kicad_matched, comment: waived.comment }));
      check("and lists no marker of its own beside it", after.violations.filter((v) => v.check === "pin_not_connected" && v.location === first.violations[symbolPin].location).length === 1);
      // Delete Marker on the excluded marker: the exclusion goes (the next run records the markers that are left, and this one is not).
      const idx = after.violations.indexOf(waived);
      rowAt(idx).click();
      await sleep(300);
      footer(TITLE, "Delete Marker").click();
      await sleep(1500);
      await runErc();
      const back = (await report()).violations.filter((v) => v.check === "pin_not_connected" && v.location === first.violations[symbolPin].location);
      check("Delete Marker on it forgets the exclusion: the next run reports a violation again, and no excluded marker", back.length === 1 && back[0].severity !== "excluded", JSON.stringify(back.map((v) => v.severity)));
    }

    // ---------------------------------------------------------------------------------------------------------------------- Delete All Markers
    const nowRows = rows().length;
    const aMarker = (await report()).violations.findIndex((v) => v.severity !== "excluded" && /^[A-Za-z]+\d+\.\d+$/.test(v.location ?? ""));
    await rightClick(rowAt(aMarker));
    await pick("Exclude this violation");
    await sleep(800);
    footer(TITLE, "Delete All Markers").click();
    await sleep(300);
    check("with an exclusion in the report Delete All Markers asks whether it goes too", /Delete exclusions too\?/.test(dialogOf(TITLE).innerText));
    footer(TITLE, "Errors and Warnings Only").click();
    await sleep(600);
    check("Errors and Warnings Only leaves the excluded marker", rows().length === 1 && rows()[0].classList.contains("excluded"), `${rows().length} rows of ${nowRows}`);
    footer(TITLE, "Delete All Markers").click();
    await sleep(200);
    footer(TITLE, "Errors, Warnings and Exclusions").click();
    await sleep(1500);
    check("Errors, Warnings and Exclusions empties the list", rows().length === 0 && /All markers were deleted/.test(dialogOf(TITLE).innerText));
    await runErc();
    check("the next run reports every finding again and excludes none", rows().length >= nowRows - 1 && rows().every((r) => !r.classList.contains("excluded")), `${rows().length} rows`);

    // ---------------------------------------------------------------------------------------------------------- Edit connection grid spacing
    const noOffGrid = (await report()).violations.filter((v) => v.check === "endpoint_off_grid").length;
    check("on KiCad's 50 mil connection grid the wire's ends are on the grid", noOffGrid === 0, noOffGrid);
    const opened = await eda.run("eeschema.EditorControl.schematicSetup");
    const setup = await until(() => dialogOf("Schematic Setup"), 3000);
    check("Schematic Setup opens", opened.ok && !!setup, opened.dialog);
    const grid = () => dialogOf("Schematic Setup")?.querySelector('input[aria-label="Connection grid spacing in mils"]');
    const formatting = async () => json("/api/sch/formatting");
    if (setup) {
      const nav = [...setup.querySelectorAll(".bs-nav-item")].find((n) => n.innerText.trim() === "Formatting");
      nav?.click();
      await sleep(300);
      check("it has a Formatting page with the connection grid, 50 mils to begin with", !!grid() && grid().value === "50", grid()?.value);
      setValue(grid(), "100");
      await sleep(200);
      dialogOf("Schematic Setup").querySelector('section[aria-label="Formatting"] .bs-actions button.primary').click();
      await sleep(1200);
      const f = await formatting();
      check("Apply makes it 100 mils: the design's setting", f.ok && f.connection_grid_mils === 100, JSON.stringify(f));
      await closeDialog("Schematic Setup");
    }
    await runErc();
    const off = (await report()).violations.filter((v) => v.check === "endpoint_off_grid");
    check("with a 100 mil grid kicad-cli finds the wire's ends off the grid", off.length >= 1, off.length);
    if (off.length >= 1) {
      const row = rows().find((r) => /off connection grid/.test(r.innerText));
      await rightClick(row);
      const labels = menuLabels();
      check("the finding's menu ends with Edit connection grid spacing", labels.some((l) => l.startsWith("Edit violation severities")) && labels.some((l) => l.startsWith("Edit connection grid spacing")), JSON.stringify(labels));
      await pick("Edit connection grid spacing");
      const again = await until(() => dialogOf("Schematic Setup"), 3000);
      check("which opens Schematic Setup on Formatting, showing 100", !!again && /Formatting/.test(again.querySelector(".bs-nav-item.active")?.innerText ?? "") && grid()?.value === "100", again?.querySelector(".bs-nav-item.active")?.innerText + " " + grid()?.value);
      if (again) {
        setValue(grid(), "50");
        await sleep(200);
        dialogOf("Schematic Setup").querySelector('section[aria-label="Formatting"] .bs-actions button.primary').click();
        await sleep(1200);
        const f = await formatting();
        check("going back to 50 is KiCad's default: nothing is stored", f.ok && f.connection_grid_mils === 50, JSON.stringify(f));
        await closeDialog("Schematic Setup");
      }
      await runErc();
      check("and the wire's ends are on the grid again", (await report()).violations.filter((v) => v.check === "endpoint_off_grid").length === 0);
    }

    // ----------------------------------------------------------------------------------------------------------------------------- Save...
    holdDownloads();
    try {
      const n = saved.length;
      footer(TITLE, "Save...").click();
      await sleep(200);
      check("Save... offers the two file types", menuLabels().join("|") === "Report file (*.rpt)|JSON file (*.json)", JSON.stringify(menuLabels()));
      await pick("Report file");
      const rpt = await nextSave(n);
      check("the report file is ERC.rpt, kicad-cli's text report", rpt && rpt.name === "ERC.rpt" && /^ERC report \(.*\)\nReport includes: Errors, Warnings, Exclusions\n/.test(rpt.text) && /\*\*\*\*\* Sheet \//.test(rpt.text) && /Wires not connected to anything|Pin not connected/.test(rpt.text), rpt ? `${rpt.name}: ${rpt.text.slice(0, 80).replace(/\n/g, " ")}` : "nothing saved");
      footer(TITLE, "Save...").click();
      await sleep(200);
      await pick("JSON file");
      const jsn = await nextSave(n + 1);
      let parsed = null;
      try {
        parsed = jsn && JSON.parse(jsn.text);
      } catch {
        // reported below
      }
      check("the JSON file is ERC.json, kicad-cli's JSON report", jsn && jsn.name === "ERC.json" && parsed && Array.isArray(parsed.sheets) && parsed.sheets.some((s) => s.violations.length > 0), jsn ? jsn.name : "nothing saved");
    } finally {
      releaseDownloads();
    }
    await closeDialog(TITLE);
    // Put the sheets back as they were found: the two wires are drawn again and the wire that was added goes.
    for (const r of removed) await post({ op: "on_sheet", sheet: r.sheet, cmd: { op: "add_wire", pts: r.pts.map(([x, y]) => ({ x, y })) } });
    const mine = (await json("/api/schematic")).wires.find((x) => x.pts[0][0] === wireAt[0] && x.pts[0][1] === wireAt[1]);
    if (mine) await post({ op: "delete_wire", id: mine.id });
    await sleep(800);
    const wireHere = (wires) => wires.some((x) => x.pts[0][0] === wireAt[0] && x.pts[0][1] === wireAt[1]);
    const drawnAgain = (await Promise.all(removed.map((r) => json(`/api/schematic?sheet=${r.sheet}`)))).every((inner, i) => inner.wires.some((x) => JSON.stringify(x.pts) === JSON.stringify(removed[i].pts)));
    check("the wires are as they were found: the one that was added is gone and the two that were deleted are drawn again", !!mine && !wireHere((await json("/api/schematic")).wires) && drawnAgain, `${removed.length} drawn again`);
  };

  try {
    if (!only || only.includes("pcb")) await board();
    if (!only || only.includes("sch")) await schematic();
  } catch (e) {
    check("the check ran to its end", false, e && e.stack ? e.stack : e);
  } finally {
    releaseDownloads();
  }
  results.push({ name: "no errors were logged", ok: eda.errors().length === 0, detail: JSON.stringify(eda.errors()) });
  return results;
})()
