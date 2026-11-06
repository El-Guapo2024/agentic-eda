// Assign Footprints (CvPcb), UI-LEFT L4 (task-u4-libs; PARITY-sch.md section 13): the footprint viewer pane, Manage Footprint Association Files... and Automatically Assign
// Footprints, ported from KiCad 25ec2335's cvpcb/ (`DISPLAY_FOOTPRINTS_FRAME::InitDisplay` / `GetFootprint`, `DIALOG_CONFIG_EQUFILES`, `AutomaticFootprintMatching`), driven
// through `window.__eda` and the dialogs' own controls and checked against what the server holds (`/api/cvpcb/*`, the schematic).
// Run it in the page of a served COPY of a board made from examples/mcu_board_30plus.yaml with e2e/cvpcb.equ copied into the board's folder as `cvpcb-check.equ` (the
// `fixtures=cvpcb` of tools/run-all-checks.sh) and KiCad's footprint libraries installed (Resistor_SMD, LED_SMD, Capacitor_SMD): it adds that file to the project's list, assigns
// footprints, undoes that, and empties the list again.
//
// It answers through `window.__cvpcbCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=5 fixtures=cvpcb
(async () => {
  const results = [];
  const check = (window.__cvpcbCheck = { results, done: false, ok: false, wait: null });
  const E = window.__eda;
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const post = (url, body) => json(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok) };
  };
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); for (;;) { const v = fn(); if (v) return v; if (Date.now() - t0 > ms) return null; await sleep(80); } };
  const titles = () => E.state().dialogs ?? [];
  const dialogs = () => [...document.querySelectorAll(".dialog-backdrop .dialog")];
  const assignDialog = () => dialogs().find((d) => d.querySelector(".dialog-header")?.textContent.trim() === "Assign Footprints");
  const equDialog = () => document.querySelector("[data-equ-dialog]")?.closest(".dialog");
  const buttonOf = (root, text) => [...(root ?? document).querySelectorAll("button")].find((b) => b.textContent.trim() === text);
  const symRow = (ref) => assignDialog()?.querySelector(`[data-cvpcb-symbol="${ref}"]`);
  const assignedOf = (ref) => symRow(ref)?.querySelector("[data-cvpcb-assigned]")?.textContent.trim();
  const clickRow = async (el, extra = {}) => { el?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...extra })); await sleep(250); };
  const viewer = () => assignDialog()?.querySelector("[data-cvpcb-viewer]");
  const symbolsOf = async () => {
    const s = await json("/api/schematic");
    const out = {};
    for (const x of s.symbols ?? []) out[x.id] = x.footprint ?? "";
    for (const sh of s.sheets ?? []) {
      const sub = await json(`/api/schematic?sheet=${encodeURIComponent(sh.id)}`);
      for (const x of sub.symbols ?? []) out[x.id] = x.footprint ?? "";
    }
    return out;
  };
  const scenario = async (name, fn) => {
    const notes = [];
    const t = (ok, text) => { notes.push(`${ok ? "ok" : "FAIL"}: ${text}`); return ok; };
    try { await fn(t); } catch (e) { t(false, `threw ${e && e.stack ? e.stack.split("\n").slice(0, 3).join(" | ") : e}`); }
    results.push({ name, ok: notes.every((n) => n.startsWith("ok")), notes });
  };
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  const errorsBefore = E.errors().length;
  if (E.state().tab !== "schematic") { [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "Schematic")?.click(); await sleep(1400); }
  const FILE = "${KIPRJMOD}/cvpcb-check.equ";
  const before = await symbolsOf();
  // the board's LED channels sheet holds R1..R8 (330) and D1..D8 (LED), none with a footprint yet: the dialog lists the sheet in view
  const root = await json("/api/schematic");
  const LEDS = (root.sheets ?? []).find((s) => /^LED/.test(s.name))?.id;
  const leds = LEDS ? await json(`/api/schematic?sheet=${encodeURIComponent(LEDS)}`) : { symbols: [] };
  const refsOf = (re) => leds.symbols.map((s) => s.id).filter((r) => re.test(r));
  const RS = refsOf(/^R\d+$/);
  const DS = refsOf(/^D\d+$/);
  const R1 = "R1";
  const D1 = "D1";
  if (LEDS) {
    await E.run("common.InteractiveSelection.clear");
    await E.run("common.InteractiveSelection.selectItems", [LEDS]);
    await sleep(250);
    await E.run("eeschema.NavigateTool.enterSheet");
    await sleep(1200);
    await E.run("common.InteractiveSelection.clear");
  }

  (async () => {
    await scenario("Assign Footprints opens with the viewer pane: the picked footprint, else the selected symbol's; GetFootprint's messages", async (t) => {
      t(!!LEDS && RS.length === 8 && DS.length === 8, `the board's LED channels sheet has R1..R8 and D1..D8 (${RS.length}, ${DS.length})`);
      await E.run("eeschema.EditorControl.assignFootprints");
      t(!!(await waitFor(() => assignDialog())), "Assign Footprints opens");
      t(!!symRow(R1) && !!symRow(D1) && assignedOf(R1) === "(none)", `it lists R1 and D1, R1 with no footprint (${assignedOf(R1)})`);
      t(!!viewer() && viewer().querySelector("canvas"), "the viewer pane is there (View Selected Footprint)");
      await clickRow(symRow(R1));
      t(viewer()?.getAttribute("data-cvpcb-viewer") === "" && /No footprint selected/.test(viewer()?.textContent ?? ""), "R1 has no footprint and nothing is picked: the viewer is empty");
      const firstFp = assignDialog().querySelector("[data-cvpcb-fp]");
      if (firstFp) {
        const id = firstFp.getAttribute("data-cvpcb-fp");
        await clickRow(firstFp);
        await waitFor(() => viewer()?.getAttribute("data-cvpcb-viewer") === id, 4000);
        t(viewer()?.getAttribute("data-cvpcb-viewer") === id && viewer().querySelector("[data-cvpcb-viewer-title]")?.textContent === `Footprint: ${id}`, `a footprint picked in the list is the one shown, titled "Footprint: %s" (${id})`);
        const reply = await json(`/api/cvpcb/footprint?id=${encodeURIComponent(id)}`);
        t(!!reply.footprint && (reply.footprint.pads ?? []).length > 0, `the server gives the viewer the footprint with its pads (${(reply.footprint?.pads ?? []).length})`);
        await sleep(400);
        t((viewer()?.querySelector(".status")?.textContent ?? "") === "", `it is drawn, no message (${viewer()?.querySelector(".status")?.textContent})`);
        await clickRow(symRow(R1));
        t(viewer()?.getAttribute("data-cvpcb-viewer") === "", "selecting a symbol with no footprint clears the pick (OnSelectComponent)");
      } else t(true, "(the board has no footprint of its own to pick: the list part is skipped)");
      const lib = await json("/api/cvpcb/footprint?id=Resistor_SMD%3AR_0805_2012Metric");
      t(lib.lib === "Resistor_SMD" && (lib.footprint?.pads ?? []).length === 2, `a footprint of KiCad's libraries is read for the viewer ("Lib: ${lib.lib}")`);
      t((await json("/api/cvpcb/footprint?id=NoSuchLib%3AX")).error === "Library 'NoSuchLib' is not in the footprint library table.", "a library not in the table: KiCad's words");
      t((await json("/api/cvpcb/footprint?id=Resistor_SMD%3ANope")).error === "Footprint 'Resistor_SMD:Nope' not found.", "a footprint the library lacks: KiCad's words");
    });

    await scenario("Manage Footprint Association Files: Add writes the file under ${KIPRJMOD}, a second Add is refused, OK saves the project's list", async (t) => {
      buttonOf(assignDialog(), "Manage Footprint Association Files…")?.click();
      const d = await waitFor(() => equDialog());
      t(!!d, "the dialog opens");
      await waitFor(() => /^Project file: '.*cvpcb\.json'$/.test(d?.querySelector(".dialog-header")?.textContent.trim() ?? ""), 3000);
      t(/^Project file: '.*cvpcb\.json'$/.test(d?.querySelector(".dialog-header")?.textContent.trim() ?? ""), `titled "Project file: '%s'" (${d?.querySelector(".dialog-header")?.textContent.trim()})`);
      const vars = [...d.querySelectorAll('table[aria-label="Available path substitutions"] tbody tr')].map((tr) => tr.children[0].textContent);
      t(vars[0] === "KIPRJMOD" && /^KICAD\d+_FOOTPRINT_DIR$/.test(vars[1] ?? ""), `the substitutions are KIPRJMOD and the footprint folder's (${vars})`);
      const add = async () => {
        d.querySelector('[aria-label="Add association file"]').click();
        const file = await waitFor(() => d.querySelector('[data-equ-file="cvpcb-check.equ"]'), 4000);
        if (!file) return false;
        await clickRow(file);
        d.querySelector("[data-equ-open]").click();
        await sleep(300);
        return true;
      };
      t(await add(), "Add's file dialog starts in the project's folder and lists the .equ file");
      t(!!d.querySelector(`[data-equ-entry="${FILE}"]`), `the list has ${FILE}`);
      await add();
      t((d.querySelector("[data-equ-message]")?.textContent ?? "").includes(`File '${FILE}' already exists in list.`), "a second Add of it is refused in KiCad's words");
      await clickRow(d.querySelector(`[data-equ-entry="${FILE}"]`));
      d.querySelector('[aria-label="Edit association file"]').click();
      await sleep(200);
      t(/cannot start a text editor/.test(d.querySelector("[data-equ-message]")?.textContent ?? ""), "Edit says the page cannot start a text editor");
      d.querySelector("[data-equ-ok]").click();
      await waitFor(() => !equDialog(), 4000);
      t(!equDialog() && !!assignDialog(), "OK closes it and Assign Footprints stays open");
      const v = await json("/api/cvpcb/equ_files");
      t(JSON.stringify(v.files) === JSON.stringify([FILE]) && v.resolved[0]?.found === true, `the project's list is saved (${JSON.stringify(v.files)}, found ${v.resolved[0]?.found})`);
    });

    await scenario("Automatically Assign Footprints assigns by value the symbols with no footprint; the viewer shows the one assigned; OK applies it as one undo step", async (t) => {
      assignDialog().querySelector("[data-cvpcb-auto]").click();
      await waitFor(() => assignDialog()?.querySelector("[data-cvpcb-status]"), 6000);
      t(assignDialog()?.querySelector("[data-cvpcb-status]")?.textContent === "5 footprint/symbol equivalences found.", `the status says how many associations the files hold (${assignDialog()?.querySelector("[data-cvpcb-status]")?.textContent})`);
      const rs = RS.map(assignedOf);
      const ds = DS.map(assignedOf);
      t(rs.every((f) => f === "Resistor_SMD:R_0805_2012Metric"), `every 330 resistor gets the file's footprint (${[...new Set(rs)]})`);
      t(ds.every((f) => f === "LED_SMD:LED_0603_1608Metric"), `every LED gets the first of the value's two entries its footprint filters keep (${[...new Set(ds)]})`);
      t(!assignDialog()?.querySelector("[data-cvpcb-warnings]") && !assignDialog()?.querySelector("[data-cvpcb-load-errors]"), "no warning and no load error");
      await clickRow(symRow(R1));
      await waitFor(() => viewer()?.getAttribute("data-cvpcb-viewer") === "Resistor_SMD:R_0805_2012Metric" && viewer().querySelector("[data-cvpcb-viewer-status]")?.textContent === "Lib: Resistor_SMD", 6000);
      t(viewer()?.getAttribute("data-cvpcb-viewer") === "Resistor_SMD:R_0805_2012Metric", `the viewer shows R1's footprint, an assignment not applied yet (InitDisplay: comp->GetFPID(); ${viewer()?.getAttribute("data-cvpcb-viewer")})`);
      t(viewer()?.querySelector("[data-cvpcb-viewer-status]")?.textContent === "Lib: Resistor_SMD", `with "Lib: %s" (${viewer()?.querySelector("[data-cvpcb-viewer-status]")?.textContent})`);
      const n = RS.length + DS.length;
      const apply = [...assignDialog().querySelectorAll(".dialog-footer button")].find((b) => b.textContent.trim() === `Apply ${n} assignments`);
      t(!!apply, `OK applies the ${n} assignments (${[...assignDialog().querySelectorAll(".dialog-footer button")].map((b) => b.textContent.trim())})`);
      apply?.click();
      await waitFor(() => !assignDialog(), 6000);
      await sleep(600);
      const after = await symbolsOf();
      t(after.R1 === "Resistor_SMD:R_0805_2012Metric" && after.D1 === "LED_SMD:LED_0603_1608Metric", `the schematic has them (${after.R1}, ${after.D1})`);
      const changed = Object.keys(after).filter((r) => after[r] !== before[r]);
      t(JSON.stringify(changed.sort()) === JSON.stringify([...RS, ...DS].sort()), `the 330s and the LEDs changed, nothing else (${changed.length})`);
      await post("/api/undo", { domain: "schematic" });
      await sleep(900);
      const back = await symbolsOf();
      t(JSON.stringify(back) === JSON.stringify(before), "one undo takes all of them back");
    });

    await scenario("a file the list names that is not there is reported as KiCad reports it", async (t) => {
      await post("/api/cvpcb/equ_files", { files: [FILE, "${KIPRJMOD}/missing.equ"] });
      const r = await post("/api/cvpcb/auto_associate", { components: [{ ref: "U1", value: "MCU", footprint: "", lib_id: "" }] });
      t(r.load_errors === "Equivalence file 'missing.equ' could not be found.", `"Equivalence File Load Error" (${r.load_errors})`);
      t(r.warnings === "Component U1: footprint Nowhere:Nothing not found in any of the project footprint libraries.", `"CvPcb Warning" for an entry whose footprint no library has (${r.warnings})`);
      t(r.assignments.length === 0, "nothing assigned");
      await post("/api/cvpcb/equ_files", { files: [] });
      t((await json("/api/cvpcb/equ_files")).files.length === 0, "the list is emptied again");
      t(E.errors().slice(errorsBefore).length === 0, `no errors ${JSON.stringify(E.errors().slice(errorsBefore).map((e) => e.message.slice(0, 80)))}`);
    });

    if (strictWas && strict && !strict.checked) strict.click();
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
