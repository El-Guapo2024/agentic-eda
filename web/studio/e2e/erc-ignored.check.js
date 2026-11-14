// The ERC dialog's Ignored Tests list is DIALOG_ERC's m_ignoredList (UI-LEFT-3 B3-5; KiCad 25ec2335 eeschema/dialogs/dialog_erc.cpp):
//   ignore     Ignore all from a pin-to-pin error's menu (ID_SET_SEVERITY_TO_IGNORE) adds a row of the error's own code -- the pin-to-pin error's slot, not
//              the warning's `pin_to_pin` -- after the rows the run listed;
//   menu       the row's menu (OnIgnoredItemRClick) has Error, Warning and Ignore with the code's severity checked (GetSeverity); Error sets the error's slot
//              back and deletes that row only;
//   kept       the rows are kept from one opening of the dialog to the next (g_lastERCIgnored), until a run builds the list again from the severities
//              (OnRunERCClick): the warning's slot, ignored with the error's, is listed then.
// The passive-passive cell of the pin conflict map is set to an error for the run (and reset at the end), so the board has pin-to-pin errors.
// It is one expression resolving to [{ name, ok, detail }]. Serve a scratch board made from examples/mcu_board_30plus.yaml:
//   target/debug/eda board new examples/mcu_board_30plus.yaml -o <dir> && target/debug/eda board serve -C <dir> --port N --ui web/studio/dist
//   PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/erc-ignored.check.js 10
// run-all-checks: board=mcu30 minutes=10
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
  const ERC = "Electrical Rules Checker";
  const dialogNamed = (title) => [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith(title));
  const rows = () => [...(dialogNamed(ERC)?.querySelectorAll(".problem-row") ?? [])];
  const ignoredRows = () => [...(dialogNamed(ERC)?.querySelectorAll(".rc-ignored-row") ?? [])];
  const keys = () => ignoredRows().map((r) => r.dataset.key);
  const page = (label) => [...(dialogNamed(ERC)?.querySelectorAll(".dock-tab") ?? [])].find((t) => t.innerText.trim().startsWith(label));
  const rightClick = async (el) => {
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 300, clientY: 250, button: 2 }));
    await sleep(400);
  };
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown .menu-node-item")];
  const pick = async (label) => {
    const it = menuItems().find((e) => e.innerText.startsWith(label));
    if (!it) throw new Error(`no menu entry "${label}" in ${JSON.stringify(menuItems().map((e) => e.innerText))}`);
    it.click();
    await sleep(300);
  };
  const close = async () => {
    dialogNamed(ERC)?.querySelector(".dialog-footer button.primary")?.click();
    await sleep(300);
  };
  const post = async (cmd) => (await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) })).json();
  const severities = async () => (await fetch("/api/sch/erc_severities")).json();
  const goTab = async (name) => {
    [...document.querySelectorAll("button, div")].find((e) => e.innerText === name && /tab/.test(e.className))?.click();
    await sleep(1200);
  };
  const runDone = () => until(() => !dialogNamed(ERC).innerText.includes("kicad-cli is checking") && !dialogNamed(ERC).innerText.includes("Run ERC to check"), 120000);

  const before = await severities();
  try {
    await goTab("Schematic");
    // passive (4) to passive (4): an error
    const cell = await post({ op: "set_erc_pin_map_cell", a: 4, b: 4, level: 2 });
    check("the pin map's passive-passive cell is set to an error", cell.ok, cell.message);
    await sleep(400);
    await eda.run("eeschema.InspectionTool.runERC");
    await until(() => dialogNamed(ERC));
    await sleep(600);
    await runDone();
    const all = dialogNamed(ERC).querySelector(".rc-show input");
    if (all && !all.checked) all.click();
    await sleep(300);
    const pinError = rows().find((r) => /^Error: Pins of type /.test(r.innerText));
    check("the run reports pin-to-pin errors", !!pinError, rows().slice(0, 5).map((r) => r.innerText.split("\n")[0]).join(" | "));
    page("Ignored Tests").click();
    await sleep(300);
    const runRows = keys();
    check("the run lists the checks set to Ignore", runRows.length > 0 && !runRows.includes("pin_to_pin_error"), JSON.stringify(runRows));
    page("Violations").click();
    await sleep(300);

    // Ignore all on a pin-to-pin error
    await rightClick(rows().find((r) => /^Error: Pins of type /.test(r.innerText)));
    await pick("Ignore all");
    await sleep(1500);
    check("the pin-to-pin markers leave the list", !rows().some((r) => /Pins of type /.test(r.innerText)));
    page("Ignored Tests").click();
    await sleep(300);
    check("Ignore adds a row of the error's own code after the run's rows (ID_SET_SEVERITY_TO_IGNORE)", JSON.stringify(keys()) === JSON.stringify([...runRows, "pin_to_pin_error"]), JSON.stringify(keys()));
    const table = await severities();
    check("both pin-to-pin slots are ignored", table.pin_to_pin_errors_ignored === true && table.severities.pin_to_pin === "ignore", JSON.stringify(table));
    check("the tab counts the rows", /Ignored Tests \(\d+\)/.test(page("Ignored Tests").innerText) && page("Ignored Tests").innerText.includes(`(${runRows.length + 1})`), page("Ignored Tests").innerText);

    // kept between openings
    await close();
    await eda.run("eeschema.InspectionTool.runERC");
    await until(() => dialogNamed(ERC));
    await sleep(500);
    page("Ignored Tests").click();
    await sleep(300);
    check("opened again, the rows are as they were left (g_lastERCIgnored)", JSON.stringify(keys()) === JSON.stringify([...runRows, "pin_to_pin_error"]), JSON.stringify(keys()));

    // the row's menu
    const errorRow = ignoredRows().find((r) => r.dataset.key === "pin_to_pin_error");
    await rightClick(errorRow);
    const labels = menuItems().map((e) => e.innerText.trim());
    const checked = menuItems().filter((e) => e.querySelector('span[style*="font-weight"]')).map((e) => e.innerText.trim());
    check("the row's menu is Error, Warning, Ignore with Ignore checked (GetSeverity of the error's code)", JSON.stringify(labels) === JSON.stringify(["Error", "Warning", "Ignore"]) && JSON.stringify(checked) === JSON.stringify(["Ignore"]), `${JSON.stringify(labels)} ${JSON.stringify(checked)}`);
    await pick("Error");
    await until(() => !keys().includes("pin_to_pin_error"), 15000);
    check("Error deletes that row only", JSON.stringify(keys()) === JSON.stringify(runRows), JSON.stringify(keys()));
    const after = await severities();
    check("and sets the error's slot back, the warning's left ignored", after.pin_to_pin_errors_ignored === false && after.severities.pin_to_pin === "ignore", JSON.stringify(after));

    // a run builds the list again from the severities
    const run = [...dialogNamed(ERC).querySelectorAll("button")].find((b) => b.innerText.trim() === "Run ERC");
    run.click();
    await sleep(600);
    await runDone();
    page("Ignored Tests").click();
    await sleep(300);
    check("Run ERC lists the ignored checks again: the warning's slot, ignored with the error's, is there now", keys().includes("pin_to_pin") && !keys().includes("pin_to_pin_error"), JSON.stringify(keys()));
    await close();
  } catch (e) {
    check("the check ran to the end", false, e && e.stack ? e.stack : String(e));
  } finally {
    await post({ op: "reset_erc_pin_map" });
    await post({ op: "set_erc_severities", severities: { ...before.severities, pin_to_pin_error: "error" } });
  }
  results.push({ name: "no errors were logged", ok: eda.errors().length === 0, detail: JSON.stringify(eda.errors()) });
  return results;
})()
