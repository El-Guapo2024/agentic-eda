// The DRC and ERC review workflow, driven through `window.__eda` (src/kicad-port/edaTestHook.ts): Next / Previous / Exclude Marker, the marker menus, exclusions with
// a comment and for a whole check, the Ignored Tests and Schematic Parity pages, severity changes from a menu and from Schematic Setup.
//
// It is one expression that resolves to a list of { name, ok, detail }: paste it into the Browser pane's javascript tool, or hand it to Playwright's
// `page.evaluate`. It changes the board it runs on (exclusions, severities), so serve a PRISTINE scratch COPY of a board -- one with nothing excluded yet:
//
//   target/debug/eda board serve -C <scratch copy> --port 8802 --ui web/studio/dist     # after `cd web/studio && npm run build`
//   open http://127.0.0.1:8802/ in a tab of your own, then evaluate this file
//
// It needs a board with at least three DRC findings of one check (work/mcu30 has 21 silkscreen warnings) and a schematic ERC finds something in.
// Each step waits for what it asked for (a kicad-cli run takes seconds, the whole check about a minute). The Browser pane's javascript tool gives up on
// a call after 45 s, so start it without waiting and read the answer later:
//
//   window.__checkResults = null; window.__checkPromise = (0, eval)(src).then((r) => { window.__checkResults = r; });   // src = this file's text
//   ... later: window.__checkResults
//
// (a copy of this file in web/studio/dist is served by the same server, so `src = await (await fetch("/drc-review.check.js")).text()`).
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
  const dialog = () => document.querySelector(".dialog");
  const rows = () => [...document.querySelectorAll(".dialog .problem-row")];
  const selectedIndex = () => rows().findIndex((r) => r.classList.contains("selected"));
  const badges = () => document.querySelector(".dialog .rc-badges")?.innerText.replace(/\n/g, " | ") ?? "";
  const button = (label) => [...document.querySelectorAll(".dialog button")].find((b) => b.innerText.trim().startsWith(label));
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
  const closeDialog = async (title) => {
    const d = [...document.querySelectorAll(".dialog")].find((x) => x.innerText.startsWith(title));
    d?.querySelector(".dialog-footer button.primary")?.click();
    await sleep(250);
  };

  const goTab = async (name) => {
    [...document.querySelectorAll("button, div")].find((e) => e.innerText === name && /tab/.test(e.className))?.click();
    await sleep(1200);
  };

  // ------------------------------------------------------------------------------------------------------------------ the board
  await goTab("PCB");
  if (eda.state().tab !== "pcb") return [{ name: "the PCB tab opens", ok: false, detail: eda.state().tab }];
  const ran = await eda.run("pcbnew.DRCTool.runDRC");
  check("Run DRC opens the Design Rules Checker", ran.ok && /Design Rules Checker/.test(ran.dialog ?? ""), ran.dialog);
  const loaded = await until(() => rows().length >= 3);
  check("the Violations page lists the findings", !!loaded, `${rows().length} rows`);
  if (!loaded) return results;
  const total = rows().length;
  check("every row says what it is (Error: / Warning:) and names its items", rows().every((r) => /^(Error|Warning): /.test(r.innerText)) && rows().every((r) => r.querySelectorAll("small.rc-item").length > 0));

  check("Next, Previous and Exclude Marker are on in the PCB editor", ["common.Checker.nextMarker", "common.Checker.prevMarker", "common.Checker.excludeMarker"].every((id) => eda.actions().find((a) => a.id === id)?.enabled));
  const before = selectedIndex();
  await eda.run("common.Checker.nextMarker");
  const a = selectedIndex();
  await eda.run("common.Checker.nextMarker");
  const b = selectedIndex();
  await eda.run("common.Checker.prevMarker");
  const c = selectedIndex();
  check("Next Marker selects the first row, then the next; Previous goes back", before === -1 && a === 0 && b === 1 && c === 0, `${before} ${a} ${b} ${c}`);
  await eda.run("common.Checker.prevMarker");
  check("Previous Marker at the first row stays", selectedIndex() === 0);
  check("selecting a marker selects the items it names on the board", eda.state().selection.length > 0, JSON.stringify(eda.state().selection));

  const counts0 = badges();
  await eda.run("common.Checker.excludeMarker");
  await sleep(500);
  check("Exclude Marker waives the selected violation (a row goes dim, the badge counts it)", rows()[0].classList.contains("excluded") && /1 excluded/.test(badges()) && /^Excluded /.test(rows()[0].innerText), `${counts0} -> ${badges()}`);

  await rightClick(rows()[1]);
  const fresh = menuLabels();
  check("the marker menu offers exclude, exclude with comment, exclude all, change severity, ignore, edit severities", ["Exclude this violation", "Exclude with comment", "Exclude all '", "Change severity to", "Ignore all '", "Edit violation severities"].every((p) => fresh.some((l) => l.startsWith(p))), JSON.stringify(fresh));
  await pick("Exclude with comment");
  const prompt = await until(() => document.querySelector('textarea[aria-label="Exclusion comment"]'), 3000);
  check("Exclude with comment asks for the comment", !!prompt);
  if (prompt) {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set.call(prompt, "known and accepted");
    prompt.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(100);
    [...document.querySelectorAll(".dialog button")].find((x) => x.innerText === "OK").click();
    await sleep(1500);
    check("the comment is kept with the exclusion and shown under it", rows()[1].classList.contains("excluded") && /known and accepted/.test(rows()[1].innerText), rows()[1].innerText.replace(/\n/g, " / "));
  }

  await rightClick(rows()[2]);
  await pick("Exclude all '");
  await sleep(1500);
  check("Exclude all of a check waives every violation of it", rows().filter((r) => r.classList.contains("excluded")).length >= 3, badges());
  const persisted = await (await fetch("/api/drc")).json();
  check("the waivers are the design's (GET /api/drc says excluded)", persisted.violations.filter((v) => v.excluded).length >= 3, `${persisted.violations.filter((v) => v.excluded).length} of ${persisted.violations.length}`);
  check("an exclusion's comment comes back with the report", persisted.violations.some((v) => v.comment === "known and accepted"));

  await rightClick(rows()[0]);
  const waived = menuLabels();
  check("a waived violation's menu offers to remove the exclusion", waived.some((l) => l.startsWith("Remove exclusion")) && waived.some((l) => l.startsWith("Edit exclusion comment")), JSON.stringify(waived));
  await pick("Remove exclusion");
  await sleep(1000);
  check("Remove exclusion puts it back", !rows()[0].classList.contains("excluded"));

  const show = [...document.querySelectorAll(".dialog .rc-show input")];
  show[show.length - 1].click();
  await sleep(200);
  check("the Show boxes leave exclusions out of the list", rows().length < total && rows().every((r) => !r.classList.contains("excluded")), `${rows().length} of ${total}`);
  show[show.length - 1].click();
  await sleep(200);

  // ---------------------------------------------------------------------------------------------------------- Ignored Tests, Schematic Parity
  page("Ignored Tests").click();
  await sleep(250);
  const ignored = [...document.querySelectorAll(".dialog .rc-ignored-row")];
  check("Ignored Tests lists the checks set to Ignore", ignored.length > 0, `${ignored.length}`);
  if (ignored.length > 0) {
    await rightClick(ignored[0]);
    check("a right click on one offers Error / Warning / Ignore", ["Error", "Warning", "Ignore"].every((l) => menuLabels().some((m) => m.startsWith(l))));
    await pick("Warning");
    await sleep(1200);
    check("choosing Warning takes the check off the list", document.querySelectorAll(".dialog .rc-ignored-row").length === ignored.length - 1);
    await fetch("/api/undo", { method: "POST" });
  }
  page("Schematic Parity").click();
  await sleep(250);
  const parityBox = [...document.querySelectorAll(".dialog label.toggle")].find((l) => l.innerText.includes("parity"))?.querySelector("input");
  check("Schematic Parity says it was not run until it is asked for", /Not run|not run/.test(dialog().innerText));
  if (parityBox) {
    if (!parityBox.checked) parityBox.click();
    button("Run DRC").click();
    const done = await until(() => /Schematic Parity \(\d+\)/.test(dialog().innerText), 60000);
    check("with the parity box on, Run DRC fills the Schematic Parity page (kicad-cli --schematic-parity)", !!done, [...document.querySelectorAll(".dialog .dock-tab")].map((t) => t.innerText).join(" | "));
    parityBox.click();
  }
  await closeDialog("Design Rules Checker");

  // ---------------------------------------------------------------------------------------------------------------------- the schematic
  await goTab("Schematic");
  if (eda.state().tab !== "schematic") {
    check("the Schematic tab opens", false, eda.state().tab);
    return results;
  }
  const erc = await eda.run("eeschema.InspectionTool.runERC");
  check("Run ERC opens the Electrical Rules Checker", erc.ok && /Electrical Rules Checker/.test(erc.dialog ?? ""), erc.dialog);
  const ercRows = await until(() => rows().length > 0);
  check("the Violations page lists the findings", !!ercRows, `${rows().length} rows`);
  if (ercRows) {
    check("Next, Previous and Exclude Marker are on in the Schematic Editor too", ["common.Checker.nextMarker", "common.Checker.prevMarker", "common.Checker.excludeMarker"].every((id) => eda.actions().find((x) => x.id === id)?.enabled));
    await eda.run("common.Checker.nextMarker");
    check("Next Marker selects the first row", selectedIndex() === 0, selectedIndex());
    await eda.run("common.Checker.excludeMarker");
    await sleep(800);
    check("Exclude Marker waives it", rows()[0].classList.contains("excluded") && /^Excluded /.test(rows()[0].innerText), badges());
    const target = rows().find((r) => !r.classList.contains("excluded") && /^Error: /.test(r.innerText));
    if (target) {
      const title = target.innerText.split("\n")[0].replace(/^Error: /, "");
      await rightClick(target);
      const labels = menuLabels();
      check("the finding's menu has Change severity, Ignore all and Edit violation severities", labels.some((l) => l.startsWith("Change severity to Warning")) && labels.some((l) => l.startsWith("Ignore all")) && labels.some((l) => l.startsWith("Edit violation severities")), JSON.stringify(labels));
      const errorsBefore = rows().filter((r) => /^Error: /.test(r.innerText)).length;
      await pick("Change severity to Warning");
      await sleep(1500);
      const errorsAfter = rows().filter((r) => /^Error: /.test(r.innerText)).length;
      check(`Change severity to Warning turns every '${title}' error into a warning`, errorsAfter < errorsBefore && rows().some((r) => r.innerText.startsWith(`Warning: ${title}`)), `${errorsBefore} -> ${errorsAfter} errors`);
      const table = await (await fetch("/api/sch/erc_severities")).json();
      check("the severity is the design's (GET /api/sch/erc_severities)", table.custom === true && Object.values(table.severities).includes("warning"), JSON.stringify(table.severities));
      await rightClick(rows().find((r) => r.innerText.startsWith("Warning: ")));
      await pick("Edit violation severities");
      const setup = await until(() => [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith("Schematic Setup")), 3000);
      check("Edit violation severities opens Schematic Setup on Violation Severity", !!setup && /Violation Severity/.test(setup.querySelector(".bs-nav-item.active")?.innerText ?? ""), setup?.querySelector(".bs-nav-item.active")?.innerText);
      if (setup) {
        const radios = setup.querySelectorAll('section[aria-label="Violation Severity"] input[type=radio]');
        check("the page has a row of Error / Warning / Ignore for every ERC check", radios.length >= 46 * 3, `${radios.length} radios`);
        await closeDialog("Schematic Setup");
      }
    }
    await closeDialog("Electrical Rules Checker");
  }
  results.push({ name: "no errors were logged", ok: eda.errors().length === 0, detail: JSON.stringify(eda.errors()) });
  return results;
})()
