// Update Schematic from PCB carries a pin swap back (UI-LEFT-2 PP17; PARITY-pcb.md section 108; KiCad 25ec2335 `BACK_ANNOTATE::applyChangelist`, `applyPinSwaps`,
// `processNetNameChange`): after Swap Pad Nets on a resistor's two pads, the dialog lists what KiCad's dry run lists -- "Would swap R1 pins 1 and 2 to match PCB, but
// unconstrained pin swaps are disabled in the schematic preferences." and the two label changes --, its three net options run the dry run again, its button applies it
// as one undo step of the schematic, and with Preferences > Editing Options > "Allow unconstrained pin swaps" on the pins trade places instead ("Swap R1 pins 1 and 2 to
// match PCB."). Run it in the page of a served COPY of a placed board whose R1 has two signal nets (examples/mcu_board_30plus.yaml after `eda place`); it edits and undoes
// everything, the preference included, and compares the board with how it was. It answers through `window.__backAnnotateCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// It works through `window.__eda` (the actions), the dialogs' controls (their `data-testid`s), `/api/cmd`, `/api/undo`, `/api/state` and `/api/back_annotate`.
// run-all-checks: board=mcu30-placed minutes=6
(async () => {
  const results = [];
  const check = (window.__backAnnotateCheck = { results, done: false, ok: false, wait: null });
  const json = (url, body) => fetch(url, body === undefined ? undefined : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const until = async (f, ms = 8000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { const v = f(); if (v) return v; await sleep(100); } return null; };
  const cmd = (c) => json("/api/cmd", { cmd: c, strict: false });
  const undo = async (domain) => { const r = await json("/api/undo", { domain }); await __eda.refresh?.(); await sleep(500); return r; };
  const padNets = async (ref) => Object.fromEntries(((await json("/api/state")).parts.find((p) => p.ref === ref)?.pads ?? []).map((p) => [p.num, p.net]));
  const dryRun = (allow = false) => json("/api/back_annotate", { net_names: true, prefer_unit_swaps: true, prefer_pin_swaps: true, allow_pin_swaps: allow });
  const lines = () => [...(tid("update-pcb-messages")?.querySelectorAll("*") ?? [])].filter((e) => e.children.length === 0).map((e) => e.textContent.trim()).filter(Boolean);
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = [...document.querySelectorAll("button")].find((x) => ["Close", "Cancel"].includes(x.textContent.trim()) && x.closest(".dialog")); if (!b) break; b.click(); await sleep(200); } };
  const openDialog = async () => { await __eda.run("common.Control.updateSchematicFromPCB"); return until(() => tid("update-pcb-dialog") && lines().length > 0 && tid("update-sch-options")); };
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };
  const setPinSwapPref = async (on) => {
    await __eda.run("common.SuiteControl.openPreferences");
    await until(() => tid("preferences-dialog"));
    tid("prefs-page-sch_edit")?.click();
    await sleep(300);
    const box = tid("pref-allow-pin-swaps");
    if (box && box.checked !== on) box.click();
    await sleep(150);
    tid("preferences-ok")?.click();
    await sleep(500);
    return box ? on : null;
  };
  const errorsFrom = __eda.errors().length;
  const original = JSON.stringify(await padNets("R1"));

  await scenario("the board and the schematic agree to begin with: nothing to do", async (t) => {
    const r = await dryRun();
    t(r.ok && r.lines.length === 0 && r.cmds.length === 0, `the dry run is empty (${r.lines.length} lines)`);
  });

  // A board with no stored schematic shows the one its nets derive, which follows a swap by itself: a text on the root stores the schematic first (undone at the end).
  const stored = await cmd({ op: "sch_edit", verb: "place_text", content: "pp17", at: { x: 5_080_000, y: 5_080_000 }, size_nm: 1_270_000 });
  const swapped = await cmd({ op: "swap_pad_nets", ids: ["R1.1", "R1.2"], connected_pads: false });
  await __eda.refresh?.();
  await sleep(600);
  const was = JSON.parse(original);
  const after = await padNets("R1");

  await scenario("Swap Pad Nets on R1 leaves the board disagreeing with the schematic, and the dialog says what KiCad's dry run says", async (t) => {
    t(stored.ok && swapped.ok, `the schematic is stored and the swap ran (${stored.message}; ${swapped.message})`);
    t(after["1"] === was["2"] && after["2"] === was["1"], `R1's pads traded nets: ${JSON.stringify(after)}`);
    if (!(await openDialog())) return t(false, "the dialog opened with its report");
    const shown = lines();
    t(shown.some((l) => l === "Would swap R1 pins 1 and 2 to match PCB, but unconstrained pin swaps are disabled in the schematic preferences."), `the pin swap is offered and declined: ${shown.join(" | ")}`);
    const short = (n) => n.split("/").pop();
    t(shown.includes(`Change R1 pin 1 net label from '${short(was["1"])}' to '${short(was["2"])}'.`) && shown.includes(`Change R1 pin 2 net label from '${short(was["2"])}' to '${short(was["1"])}'.`), "the two label changes, KiCad's words");
    t(["update-sch-net-names", "update-sch-unit-swaps", "update-sch-pin-swaps"].every((id) => tid(id)?.checked === true), "Net names, Prefer symbol unit swaps and Prefer symbol pin swaps are on");
    // "Net names" off: the dry run runs again and lists nothing of the nets (`OnOptionChanged`)
    tid("update-sch-net-names").click();
    await until(() => !lines().some((l) => l.includes("net label")));
    t(!lines().some((l) => l.includes("net label") || l.includes("Would swap")), `with Net names off nothing of the nets is listed: ${lines().join(" | ")}`);
    t(tid("update-pcb-apply")?.disabled === true, "and Update Schematic has nothing to do");
    tid("update-sch-net-names").click();
    await until(() => lines().some((l) => l.includes("net label")));
    t(lines().filter((l) => l.includes("net label")).length === 2, "on again: the two changes are back");
  });

  await scenario("Update Schematic renames the labels in one undo step; the pins are then on their pads' nets", async (t) => {
    if (!(await openDialog())) return t(false, "the dialog opened");
    tid("update-pcb-apply").click();
    await until(() => tid("update-pcb-apply")?.disabled, 10000);
    await sleep(800);
    const left = await dryRun();
    t(left.ok && left.lines.length === 0, `nothing is left to do: ${left.lines.map((l) => l.text).join(" | ")}`);
    const now = await padNets("R1");
    t(now["1"] === after["1"] && now["2"] === after["2"], `R1's pads keep the swapped nets: ${JSON.stringify(now)}`);
    await closeDialogs();
    const u = await undo("schematic");
    t(u.ok, `one undo of the schematic (${u.message})`);
    const back = await dryRun();
    t(back.lines.filter((l) => l.text.includes("net label")).length === 2, "and the two label changes are to do again");
  });

  await scenario("with Allow unconstrained pin swaps on, R1's pins trade places instead (Swap Pins), and the preference goes back", async (t) => {
    const set = await setPinSwapPref(true);
    if (!t(set === true, "Preferences > Editing Options > Allow unconstrained pin swaps is ticked")) return;
    try {
      if (!(await openDialog())) return t(false, "the dialog opened");
      const shown = lines();
      t(shown.includes("Swap R1 pins 1 and 2 to match PCB.") && !shown.some((l) => l.includes("net label")), `a pin swap and no label: ${shown.join(" | ")}`);
      tid("update-pcb-apply").click();
      await until(() => tid("update-pcb-apply")?.disabled, 10000);
      await sleep(800);
      const left = await dryRun(true);
      t(left.ok && left.lines.length === 0, `nothing is left to do: ${left.lines.map((l) => l.text).join(" | ")}`);
      await closeDialogs();
      t((await undo("schematic")).ok, "one undo of the schematic");
    } finally {
      await setPinSwapPref(false);
    }
  });

  await scenario("the swap and the stored schematic are undone, the board is as it was and nothing logged an error", async (t) => {
    await undo("pcb");
    await undo("schematic");
    t(JSON.stringify(await padNets("R1")) === original, `R1's pads are as they were: ${JSON.stringify(await padNets("R1"))}`);
    const r = await dryRun();
    t(r.ok && r.lines.length === 0, "and there is nothing to back-annotate");
    const errs = __eda.errors(errorsFrom);
    t(errs.length === 0, `no errors: ${errs.map((e) => e.message).join("; ")}`);
  });

  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
