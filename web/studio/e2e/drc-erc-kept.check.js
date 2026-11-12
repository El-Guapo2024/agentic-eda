// The checker dialogs' kept controls and the Custom Rules editor's Scintilla habits (UI-LEFT-2 BB5, BB7; task-v8-drcerc):
//   drc kept      the DRC dialog's "Refill all zones before performing DRC" and "Test for parity between PCB and schematic" are kept from one opening to the
//                 next and in this browser (KiCad 25ec2335 DIALOG_SHIM::SaveControlState keeps every check box of the dialog);
//   erc show      the ERC dialog's Show boxes start unticked (dialog_erc_base.fbp), so nothing is listed until one is ticked, and what is ticked is kept;
//   erc menu      the gear button is DIALOG_ERC::OnMenu: Cross-probe Selected Items, Center on Cross-probe, Show all errors (greyed: kicad-cli has no
//                 option for it), kept; with Cross-probe off a row's click leaves the canvas's selection alone;
//   rules editor  Board Setup > Custom Rules: the indentation guides (SetIndentationGuides), Backspace un-indenting (SetBackSpaceUnIndents),
//                 Ctrl+wheel zoom (Scintilla's DoMouseWheel); nothing is applied.
// It is one expression resolving to [{ name, ok, detail }]. Serve a scratch board made from examples/mcu_board_30plus.yaml:
//   target/debug/eda board new examples/mcu_board_30plus.yaml -o <dir> && target/debug/eda board serve -C <dir> --port N --ui web/studio/dist
//   PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/ web/studio/e2e/drc-erc-kept.check.js 10
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
  const dialogNamed = (title) => [...document.querySelectorAll(".dialog")].find((d) => d.innerText.startsWith(title));
  const close = async (title) => {
    dialogNamed(title)?.querySelector(".dialog-footer button.primary")?.click();
    await sleep(300);
  };
  const labelBox = (dlg, text) => [...(dlg?.querySelectorAll("label") ?? [])].find((l) => l.textContent.trim().startsWith(text))?.querySelector("input");
  const menuItems = () => [...document.querySelectorAll(".menubar-dropdown [role=menuitem]")];
  const stored = (key) => {
    try {
      return JSON.parse(localStorage.getItem(key) ?? "null");
    } catch {
      return null;
    }
  };
  const goTab = async (name) => {
    [...document.querySelectorAll("button, div")].find((e) => e.innerText === name && /tab/.test(e.className))?.click();
    await sleep(1200);
  };

  try {
    // ------------------------------------------------------------------------------------------------------------- DRC: Refill and Parity kept
    await goTab("PCB");
    const DRC = "Design Rules Checker";
    await eda.run("pcbnew.DRCTool.runDRC");
    let drc = await until(() => dialogNamed(DRC));
    check("the Design Rules Checker opens", !!drc);
    await until(() => !drc.innerText.includes("kicad-cli is checking"), 120000);
    const refill = labelBox(drc, "Refill all zones");
    const parity = labelBox(drc, "Test for parity");
    check("Refill starts ticked and Parity not, as dialog_drc_base.cpp has them", refill?.checked === true && parity?.checked === false, `${refill?.checked} ${parity?.checked}`);
    refill.click();
    parity.click();
    await sleep(200);
    await close(DRC);
    await eda.run("pcbnew.DRCTool.runDRC");
    drc = await until(() => dialogNamed(DRC));
    await until(() => !drc.innerText.includes("kicad-cli is checking"), 120000);
    check("opened again, the two boxes are as they were left (SaveControlState)", labelBox(drc, "Refill all zones")?.checked === false && labelBox(drc, "Test for parity")?.checked === true);
    check("and kept in this browser for the next session", JSON.stringify(stored("eda.drcDialog")?.run) === JSON.stringify({ refillZones: false, parity: true }), JSON.stringify(stored("eda.drcDialog")));
    labelBox(drc, "Refill all zones").click();
    labelBox(drc, "Test for parity").click();
    await sleep(200);
    await close(DRC);

    // ------------------------------------------------------------------------------------------------------------- ERC: the Show boxes and the menu
    await goTab("Schematic");
    // a wire joining nothing, below the sheet: findings for the list (taken back at the end)
    const post = async (cmd) => (await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) })).json();
    const wire = await post({ op: "add_wire", pts: [{ x: 25_400_000, y: 254_000_000 }, { x: 30_480_000, y: 254_000_000 }] });
    check("a wire joining nothing is added", wire.ok, wire.message);
    await sleep(500);
    const ERC = "Electrical Rules Checker";
    await eda.run("eeschema.InspectionTool.runERC");
    let erc = await until(() => dialogNamed(ERC));
    check("the Electrical Rules Checker opens", !!erc);
    await sleep(600);
    await until(() => !dialogNamed(ERC).innerText.includes("kicad-cli is checking") && !dialogNamed(ERC).innerText.includes("Run ERC to check"), 120000);
    const report = await (await fetch("/api/erc")).json();
    const boxes = () => [...(dialogNamed(ERC)?.querySelectorAll(".rc-show input") ?? [])];
    const rows = () => [...(dialogNamed(ERC)?.querySelectorAll(".problem-row") ?? [])];
    check("the Show boxes start unticked (dialog_erc_base.fbp sets none)", boxes().length === 4 && boxes().every((b) => !b.checked), boxes().map((b) => b.checked).join(","));
    check("so no finding is listed although the run found some", report.violations.length > 0 && rows().length === 0, `${report.violations.length} found, ${rows().length} listed`);
    boxes()[0].click(); // All: errors, warnings and exclusions
    await sleep(300);
    check("All lists them", rows().length === report.violations.length, `${rows().length} of ${report.violations.length}`);
    await close(ERC);
    await eda.run("eeschema.InspectionTool.runERC");
    erc = await until(() => dialogNamed(ERC));
    await sleep(300);
    check("opened again the boxes are as they were left", boxes().every((b) => b.checked) && rows().length === report.violations.length);
    check("and kept in this browser", stored("eda.ercDialog")?.filter?.errors === true, JSON.stringify(stored("eda.ercDialog")));

    const gear = dialogNamed(ERC).querySelector('button[aria-label="ERC options"]');
    check("the ERC dialog has the options button (m_bMenu)", !!gear);
    gear.click();
    await sleep(200);
    const labels = menuItems().map((m) => m.textContent.replace("✓", "").trim());
    check("its menu is DIALOG_ERC::OnMenu's", JSON.stringify(labels) === JSON.stringify(["Cross-probe Selected Items", "Center on Cross-probe", "Show all errors"]), JSON.stringify(labels));
    const showAll = menuItems().find((m) => m.textContent.includes("Show all errors"));
    check("Show all errors is greyed (kicad-cli's ERC has no option for it), with KiCad's tooltip", showAll?.getAttribute("aria-disabled") === "true" && /Show duplicate ERC markers on all applicable pins/.test(showAll?.title ?? ""), showAll?.title);
    menuItems().find((m) => m.textContent.includes("Cross-probe Selected Items")).click();
    await sleep(200);
    check("Cross-probe Selected Items off is kept", stored("eda.ercDialog")?.options?.crossprobe === false, JSON.stringify(stored("eda.ercDialog")));
    // the canvas as it is: its selection and its view (a cross-probe selects the finding's item and frames it)
    const canvas = () => JSON.stringify([eda.state().selection ?? [], eda.state().view ?? null]);
    const selBefore = canvas();
    const wireRow = Math.max(0, report.violations.findIndex((v) => /wire/i.test(v.check)));
    rows()[wireRow]?.click();
    await sleep(300);
    check("with it off a row's click selects the row and leaves the canvas alone (`if( !m_crossprobe ) return`)", rows()[wireRow]?.classList.contains("selected") && canvas() === selBefore, `${selBefore} -> ${canvas()}`);
    gear.click();
    await sleep(200);
    menuItems().find((m) => m.textContent.includes("Cross-probe Selected Items")).click();
    await sleep(200);
    check("and on again", stored("eda.ercDialog")?.options?.crossprobe === true);
    rows()[(wireRow + 1) % rows().length]?.click();
    await sleep(200);
    rows()[wireRow]?.click();
    await sleep(400);
    check("with it on the click frames the finding on the sheet", canvas() !== selBefore, `${selBefore} -> ${canvas()}`);
    await close(ERC);
    await fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain: "schematic" }) });

    // ------------------------------------------------------------------------------------------------------------- the Custom Rules editor
    await goTab("PCB");
    await eda.run("pcbnew.EditorControl.boardSetup");
    await sleep(600);
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Custom Rules"))?.click();
    await sleep(400);
    const page = document.querySelector('section.bs-page[aria-label="Custom Rules"]');
    const area = page?.querySelector('textarea[aria-label="DRC Rules"]');
    check("Board Setup shows the Custom Rules editor", !!area);
    const original = area.value;
    const setValue = (v) => {
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(area), "value").set.call(area, v);
      area.dispatchEvent(new Event("input", { bubbles: true }));
    };
    setValue("(version 1)\n(rule a\n\t(constraint clearance (min 1mm))\n\t\t(condition \"A.Type == 'Pad'\"))\n");
    await sleep(300);
    const guides = () => [...page.querySelectorAll(".rte-guide")];
    check("an indentation guide is drawn in the leading blanks at each step after the first column", guides().length === 1, guides().length);
    // the caret after the two tabs of the fourth line: Backspace takes the line back one step
    const at = area.value.indexOf("\t\t(condition") + 2;
    area.focus();
    area.setSelectionRange(at, at);
    area.dispatchEvent(new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true }));
    await sleep(300);
    check("Backspace in the leading blanks un-indents the line a step (SetBackSpaceUnIndents)", area.value.includes("\n\t(condition") && !area.value.includes("\t\t(condition") && area.selectionStart === at - 1, JSON.stringify(area.value));
    const box = page.querySelector(".rte");
    const size = () => parseFloat(getComputedStyle(box).fontSize);
    const before = size();
    box.dispatchEvent(new WheelEvent("wheel", { deltaY: -100, ctrlKey: true, bubbles: true, cancelable: true }));
    await sleep(200);
    const zoomed = size();
    box.dispatchEvent(new WheelEvent("wheel", { deltaY: 100, ctrlKey: true, bubbles: true, cancelable: true }));
    await sleep(200);
    check("Ctrl+wheel zooms the text a point a notch, and back", zoomed > before && Math.abs(size() - before) < 0.01, `${before} -> ${zoomed} -> ${size()}`);
    setValue(original);
    await sleep(200);
    for (let i = 0; i < 3 && document.querySelector(".bs-dialog"); i++) {
      [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((x) => ["Close", "Discard and close"].includes(x.textContent.trim()))?.click();
      await sleep(300);
    }
    const rules = (await (await fetch("/api/state", { cache: "no-store" })).json()).board_rules?.custom_rules_text ?? "";
    check("nothing was applied to the board's rules", rules === original, rules.length);
  } catch (e) {
    check("the check ran to its end", false, (e && e.stack) || e);
  }
  try {
    localStorage.removeItem("eda.drcDialog");
    localStorage.removeItem("eda.ercDialog");
  } catch {
    // nothing kept
  }
  return results;
})();
