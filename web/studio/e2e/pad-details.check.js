// Pad details check (GAPS.md items 26 and 31 and PARITY-pcb.md sections 40 to 43): the padstack in Pad Properties (Padstack mode, Edit layer, Copper layers) and what it does
// to the state's copper of each layer, Remove Unused Pads (the dialog, its four check boxes and two buttons, one verb each), Swap Pad Nets (the rotation of the nets, the
// copper that follows, the question about the pads that were not asked for and its two answers) and Swap Gate Nets (refused with KiCad's words on a footprint with no gates).
// Run it in the page of a served board that has footprints placed -- paste it into the Browser pane's javascript tool, or `page.evaluate( src )` in Playwright -- on a COPY
// of a board (it edits and undoes, and it leaves Strict off while it runs). It makes nothing that stays: every edit is undone and the board compared with how it was.
//
// It needs two placed footprints that share a net and a third on another net (`C1`, `C2` and `R1` in examples/mcu_board_30plus.yaml after `eda board place C1 --at 20,20`,
// `C2 --at 28,20`, `R1 --at 20,28`), and it places a pin header from the installed KiCad libraries for the through-hole pads. Swap Gate Nets is exercised too when a placed
// footprint has units: a board made from an intent whose part has `symbol: "74xx:74HC00"` (`package: SOIC-14`) does; any other board skips that scenario. It answers through
// `window.__padDetailsCheck`:
//   { results: [{ name, ok, notes: ["ok: ...", "FAIL: ..."] }], done, ok, wait(ms) }
// Read it with `await __padDetailsCheck.wait(40000)` until `.done` (a run takes about two minutes). In a hidden pane the browser stops timers; the script then waits with a
// fetch loop.
//
// It works through `window.__eda` (select with `common.InteractiveSelection.selectItems`, undo with `common.Interactive.undo`, open the dialogs by their actions), the
// dialogs' controls (their `data-testid`s) and the state the server reports (`/api/state`).
// run-all-checks: board=mcu30-placed   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__padDetailsCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const state = () => json("/api/state");
  const part = async (ref) => (await state()).parts.find((p) => p.ref === ref);
  const padOf = async (id) => (await part(id.split(".")[0]))?.pads?.find((p) => p.id === id);
  const cmd = async (c) => (await fetch("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd: c, strict: false }) }).then((r) => r.json()));
  /** Every pad's net and the nets of the tracks and vias, to compare the board with how it was. */
  const snapshot = async () => {
    const s = await state();
    return JSON.stringify([s.parts.filter((p) => p.placed).map((p) => [p.ref, (p.pads ?? []).map((q) => [q.id, q.net, q.stack ?? null, q.flashed ?? null])]), (s.routing?.tracks ?? []).map((t) => [t.id, t.net]), (s.routing?.vias ?? []).map((v) => [v.id, v.net, v.unconnected ?? null])]);
  };
  const select = async (ids) => { await __eda.run("common.InteractiveSelection.clear"); await __eda.run("common.InteractiveSelection.selectItems", ids); await sleep(250); };
  const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
  const setValue = (el, v) => {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, String(v));
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  };
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const type = async (id, v) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); setValue(el, v); await sleep(60); };
  const click = async (id) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); el.click(); await sleep(120); };
  const tick = async (id, on) => { const el = tid(id); if (!el) throw new Error(`no control ${id}`); if (el.checked !== on) { el.click(); await sleep(80); } };
  const closeDialogs = async () => { for (let i = 0; i < 4; i++) { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Cancel"); if (!b) break; b.click(); await sleep(150); } };
  const openPadDialog = async (padId) => { await select([padId]); await __eda.run("pcbnew.InteractiveEdit.properties"); await sleep(450); };
  const ok = async () => { await click("board-pad-properties-ok"); await sleep(1300); };
  const labels = (el) => [...el.options].map((o) => o.textContent);
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.message) || e}`); entry.ok = false; }
    try { await closeDialogs(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const strict = [...document.querySelectorAll("label.toggle")].find((l) => /strict/i.test(l.textContent))?.querySelector("input");
  const strictWas = strict?.checked ?? false;
  if (strict?.checked) strict.click();
  document.querySelector('button[aria-label="Show Properties"]')?.click();
  await sleep(300);
  const errorsFrom = Date.now();

  // Two pads on different nets, and a third on the first one's net, from the board as it is.
  const s0 = await state();
  const withNet = s0.parts.filter((p) => p.placed && !p.pads?.some((q) => q.th)).flatMap((p) => (p.pads ?? []).filter((q) => q.net).map((q) => ({ ref: p.ref, id: q.id, net: q.net, x: q.x, y: q.y })));
  // `a` and `c` share a net (the nearest other pad of it), `b` is on another net and in another footprint.
  const shared = Object.values(withNet.reduce((m, q) => ((m[q.net] ??= []).push(q), m), {})).find((v) => v.length >= 2) ?? [];
  const a = shared[0];
  const c = a && shared.filter((q) => q.id !== a.id).sort((p, q) => Math.hypot(p.x - a.x, p.y - a.y) - Math.hypot(q.x - a.x, q.y - a.y))[0];
  const b = a && withNet.find((q) => q.net !== a.net && q.ref !== a.ref && withNet.some((r) => r.net === q.net && r.id !== q.id));
  // a pad that stays on b's net, whose net's name after an edit is the one b's net then has
  const peer = b && withNet.find((r) => r.net === b.net && r.id !== b.id);
  const original = await snapshot();

  // A pin header (through-hole pads, no nets) from the installed libraries.
  const before = new Set((await state()).parts.map((p) => p.ref));
  const placed = await cmd({ op: "place_footprint", footprint: "Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical", at: { x: 5_000_000, y: 5_000_000 } });
  const header = (await state()).parts.find((p) => !before.has(p.ref))?.ref;
  const th = header ? `${header}.1` : null;
  const afterPlace = await snapshot();

  await scenario("the board has what the check needs", async (t) => {
    t(placed.ok === true, `the pin header is placed: ${placed.message}`);
    t(!!a && !!b && !!c && !!peer && ![a.id, c.id].includes(peer.id), `four pads on two nets: ${a?.id} and ${c?.id} (${a?.net}), ${b?.id} and ${peer?.id} (${b?.net})`);
    t(!!th && (await padOf(th))?.th === true, `${th} is a through-hole pad`);
  });

  await scenario("Pad Properties of a through-hole pad offers the padstack in KiCad's words, and a surface-mount pad does not", async (t) => {
    await openPadDialog(th);
    t(!!tid("board-pad-properties"), "the dialog is open");
    t(JSON.stringify(labels(tid("pad-stack-mode"))) === JSON.stringify(["Normal", "Front/Inner/Back", "Custom"]), `Padstack mode: ${labels(tid("pad-stack-mode")).join(" | ")}`);
    t(tid("pad-stack-mode").value === "normal" && tid("pad-stack-layer").disabled, "a normal stack has one shape, so no layer to edit");
    const layers = (await state()).layers;
    t(JSON.stringify(labels(tid("pad-copper-layers"))) === JSON.stringify(["All copper layers", `${layers[0]}, ${layers[layers.length - 1]} and connected layers`, "Connected layers only"]), `Copper layers: ${labels(tid("pad-copper-layers")).join(" | ")}`);
    t(tid("pad-copper-layers").value === "keep_all", "all the layers are kept to begin with");
    await closeDialogs();
    await openPadDialog(a.id);
    t(!tid("pad-stack-mode") && !tid("pad-copper-layers"), "a surface-mount pad has neither (PTH only)");
    await closeDialogs();
  });

  await scenario("Front/Inner/Back gives the back layer a shape of its own: one undoable edit, and the state has the copper of each layer", async (t) => {
    const was = await padOf(th);
    await openPadDialog(th);
    await type("pad-stack-mode", "front_inner_back");
    t(!tid("pad-stack-layer").disabled && labels(tid("pad-stack-layer")).includes("Inner Layers"), `Edit layer: ${labels(tid("pad-stack-layer")).join(" | ")}`);
    await type("pad-stack-layer", "B.Cu");
    await type("pad-shape", "rect");
    await type("pad-size-x", 2.6);
    await type("pad-size-y", 2.2);
    await ok();
    const p = await padOf(th);
    t(p.stack?.mode === "front_inner_back" && p.stack?.back?.size?.[0] === 2_600_000 && p.stack?.back?.size?.[1] === 2_200_000, `stack: ${JSON.stringify(p.stack?.back)} (${p.stack?.mode})`);
    t(p.size[0] === was.size[0] && p.shape === was.shape, `the front is as it was: ${p.shape} ${p.size}`);
    const groups = p.stack_layers ?? [];
    const back = groups.find((g) => g.layers.includes("B.Cu"));
    const front = groups.find((g) => g.layers.includes("F.Cu"));
    t(groups.length >= 2 && back && front && back.w === 2_600_000 && back.h === 2_200_000 && front.w === was.w, `the copper of each layer: back ${back?.w}x${back?.h}, front ${front?.w}x${front?.h} (${groups.length} groups)`);
    // The dialog opens on the stack as it is.
    await openPadDialog(th);
    t(tid("pad-stack-mode").value === "front_inner_back", "the dialog opens on the stack");
    await type("pad-stack-layer", "B.Cu");
    t(tid("pad-size-x").value === "2.6", `and on the back layer's shape: ${tid("pad-size-x").value}`);
    await closeDialogs();
    await undo();
    const undone = await padOf(th);
    t(!undone.stack && !undone.stack_layers && !undone.edited, `undone: ${JSON.stringify(undone.stack)}`);
  });

  await scenario("Copper layers: Connected layers only keeps no ring where nothing is connected, and undo gives them back", async (t) => {
    await openPadDialog(th);
    await type("pad-copper-layers", "remove_all");
    await ok();
    const p = await padOf(th);
    t(p.stack?.unconnected === "remove_all", `unconnected: ${p.stack?.unconnected}`);
    t(Array.isArray(p.flashed) && p.flashed.length === 0, `no layer has its ring (nothing is connected to it): ${JSON.stringify(p.flashed)}`);
    t(Array.isArray(p.stack_layers) && p.stack_layers.length === 0, `so the canvas draws the hole alone: ${JSON.stringify(p.stack_layers)}`);
    await undo();
    const undone = await padOf(th);
    t(!undone.stack && !undone.flashed, "undone");
    await openPadDialog(th);
    await type("pad-copper-layers", "remove_except_start_and_end");
    await ok();
    const kept = await padOf(th);
    const layers = (await state()).layers;
    t(JSON.stringify(kept.flashed) === JSON.stringify([layers[0], layers[layers.length - 1]]), `Front, back and connected layers: the outside rings stay: ${JSON.stringify(kept.flashed)}`);
    await undo();
  });

  await scenario("Remove Unused Pads: KiCad's dialog, its choices and its two buttons, each one verb", async (t) => {
    await __eda.run("pcbnew.GlobalEdit.removeUnusedPads");
    await sleep(300);
    t(!!tid("remove-unused-pads"), "the dialog opens");
    t(tid("rup-vias").checked === false && tid("rup-pads").checked === false && tid("rup-selected").checked === false && tid("rup-keep-outside").checked === true, "Vias, Pads and Selected only start off, Keep outside layers on");
    t(!!tid("rup-image"), "with the picture of a pad's layers");
    const names = [...document.querySelectorAll('[data-testid="remove-unused-pads"] button')].map((x) => x.textContent.trim());
    t(JSON.stringify(names) === JSON.stringify(["Cancel", "Restore All Layers", "Remove Unused Layers"]), `buttons: ${names.join(" | ")}`);
    // Nothing chosen: nothing is sent.
    await click("rup-remove");
    t(!tid("remove-unused-pads") && (await snapshot()) === afterPlace, "with neither Pads nor Vias checked the dialog closes and the board is as it was");

    // Pads, without keeping the outside layers: every through-hole pad removes what nothing connects.
    await __eda.run("pcbnew.GlobalEdit.removeUnusedPads");
    await sleep(250);
    await tick("rup-pads", true);
    await tick("rup-keep-outside", false);
    await click("rup-remove");
    await sleep(1300);
    const pads = (await part(header)).pads;
    t(pads.length === 4 && pads.every((q) => q.stack?.unconnected === "remove_all"), `all four pads of the header: ${pads.map((q) => q.stack?.unconnected).join(", ")}`);
    t(!(await padOf(a.id)).stack, "a surface-mount pad has nothing to remove");
    t(pads.every((q) => Array.isArray(q.flashed) && q.flashed.length === 0), "and none keeps a ring: nothing is connected");
    await undo();
    t((await snapshot()) === afterPlace, "one undo step takes it all back");

    // Selected only: the one pad.
    await select([th]);
    await __eda.run("pcbnew.GlobalEdit.removeUnusedPads");
    await sleep(250);
    await tick("rup-pads", true);
    await tick("rup-selected", true);
    await click("rup-remove");
    await sleep(1300);
    const some = (await part(header)).pads;
    t(some[0].stack?.unconnected === "remove_except_start_and_end" && some.slice(1).every((q) => !q.stack), `only the selected pad, outside layers kept: ${some.map((q) => q.stack?.unconnected ?? "-").join(", ")}`);

    // Restore All Layers, for all.
    await __eda.run("pcbnew.GlobalEdit.removeUnusedPads");
    await sleep(250);
    await tick("rup-pads", true);
    await click("rup-restore");
    await sleep(1300);
    t((await snapshot()) === afterPlace, "Restore All Layers gives every pad back all its layers (and the edit goes with them)");
    await undo();
    t((await padOf(th)).stack?.unconnected === "remove_except_start_and_end", "and it is an undo step of its own");
    await undo();
    t((await snapshot()) === afterPlace, "undone");
  });

  // A net after a swap by a pad that stayed on it: the swap redraws the derived schematic, and KiCad's name of a net is its label's text with the path of
  // the sheet it is on now (PA0 with only U1's pins on it is `/MCU (U1)/PA0`, where it was `/LED channels (D1–D8)/PA0`), the name its other pads have.
  const now = async (pad) => (await padOf(pad.id))?.net;

  await scenario("Swap Pad Nets rotates the nets of the selected pads in the order they were selected", async (t) => {
    t(!!a && !!b, "two pads on different nets");
    await select([a.id, b.id]);
    const res = await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(900);
    t(!res.error, `the action ran: ${res.error ?? "ok"}`);
    const [pa, pb] = [await padOf(a.id), await padOf(b.id)];
    t(pa.net === (await now(peer)) && pb.net === (await now(c)), `${a.id}: ${a.net} -> ${pa.net} (${peer.id}'s), ${b.id}: ${b.net} -> ${pb.net} (${c.id}'s)`);
    t(!tid("swap-nets-prompt"), "no question: no copper joins them to other pads");
    await undo();
    t((await snapshot()) === afterPlace, "one undo step");
    // The other way round: the pad selected first takes the second's net, whichever it is.
    await select([b.id, a.id]);
    await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(900);
    const [qa, qb] = [await padOf(a.id), await padOf(b.id)];
    t(qa.net === (await now(peer)) && qb.net === (await now(c)), "two pads exchange, whichever is first");
    await undo();
  });

  await scenario("copper that reaches pads nobody selected raises KiCad's question, and the two answers differ in what they do to those pads", async (t) => {
    t(!!a && !!b && !!c, "a third pad on the first one's net");
    // A track from a to c on the net, so that c is in a's cluster.
    const added = await cmd({ op: "add_track", net: a.net, layer: "F.Cu", width: 200_000, pts: [{ x: a.x, y: a.y }, { x: c.x, y: c.y }] });
    t(added.ok === true, `a track joins ${a.id} and ${c.id}: ${added.message}`);
    const withTrack = await snapshot();
    await select([a.id, b.id]);
    await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(900);
    t(!!tid("swap-nets-prompt"), "the question is asked");
    const text = tid("swap-nets-message")?.textContent ?? "";
    t(/^1 unselected pad\(s\) are connected to these nets\. How do you want to proceed\?/.test(text), `its words: ${text}`);
    const listed = [...document.querySelectorAll('[data-testid="swap-nets-pads"] li')].map((x) => x.textContent);
    t(JSON.stringify(listed) === JSON.stringify([`${c.ref}:${c.id.split(".")[1].split("#")[0]}`]), `it lists ${listed.join(", ")}`);
    t(["swap-nets-cancel", "swap-nets-ignore", "swap-nets-all"].map((k) => tid(k)?.textContent.trim()).join("|") === "Cancel|Ignore Unselected Pads|Swap All Connected Pads", "with KiCad's three buttons");
    t((await snapshot()) === withTrack, "nothing changed while it is asked");
    // Ignore Unselected Pads: the track goes with the pad, c stays.
    await click("swap-nets-ignore");
    await sleep(1300);
    const track = (await state()).routing.tracks.find((x) => x.pts.some((p) => p[0] === a.x && p[1] === a.y));
    const [na, nc] = [(await padOf(a.id)).net, (await padOf(c.id)).net];
    t(na === (await now(peer)) && nc === (await now(b)) && track?.net === na, `ignore: ${a.id} ${na} (${peer.id}'s), ${c.id} ${nc} (the net ${b.id} took), track ${track?.net}`);
    await undo();
    t((await snapshot()) === withTrack, "undone");
    // Swap All Connected Pads: c follows.
    await select([a.id, b.id]);
    await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(900);
    await click("swap-nets-all");
    await sleep(1300);
    const [ma, mc] = [(await padOf(a.id)).net, (await padOf(c.id)).net];
    t(ma === (await now(peer)) && mc === ma, `all: ${a.id} and ${c.id} are on ${ma} (${peer.id}'s)`);
    await undo();
    // Cancel changes nothing.
    await select([a.id, b.id]);
    await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(900);
    await click("swap-nets-cancel");
    t((await snapshot()) === withTrack, "Cancel leaves the board as it was");
    await undo();
    t((await snapshot()) === afterPlace, "the track is undone");
  });

  await scenario("Swap Gate Nets on a footprint with no gates is refused in KiCad's words, and Swap Pad Nets needs two pads", async (t) => {
    await select([a.id, b.id]);
    const since = Date.now();
    const res = await __eda.run("pcbnew.InteractiveEdit.swapGateNets");
    await sleep(900);
    const said = [res.toast ?? "", ...__eda.errors(since).map((e) => e.message)].join(" | ");
    t(/Gate swapping must be performed on pads within one multi-gate footprint\./.test(said), `toast: ${said}`);
    t((await snapshot()) === afterPlace, "the board is as it was");
    await select([a.id]);
    const one = await __eda.run("pcbnew.InteractiveEdit.swapPadNets");
    await sleep(500);
    t(!tid("swap-nets-prompt") && (await snapshot()) === afterPlace && !one.error, "one pad is no swap");
  });

  await scenario("Swap Gate Nets exchanges the nets of two gates of a multi-gate footprint, position by position, in one undo step (when the board has one)", async (t) => {
    const g = (await state()).parts.find((p) => p.placed && (p.units?.length ?? 0) >= 2);
    if (!g) { t(true, "skipped: no placed footprint has units (a 74xx:74HC00 does: `U1` of a board made from a 74HC00's intent)"); return; }
    const ua = g.units[0];
    const ub = g.units.slice(1).find((u) => u.pins.length === ua.pins.length);
    t(!!ub, `two units with as many pins: ${ua.name} ${ua.pins} and ${ub?.name} ${ub?.pins}`);
    const nets = async () => Object.fromEntries(((await part(g.ref)).pads ?? []).map((q) => [q.num, q.net]));
    const was = await nets();
    // The context menu's entry names the unit: a pad of the first gate selected, "Swap with B" runs the action with that unit's name.
    await select([`${g.ref}.${ua.pins[0]}`]);
    const res = await __eda.run("pcbnew.InteractiveEdit.swapGateNets", ub.name);
    await sleep(1300);
    t(!res.error && !tid("swap-nets-prompt"), `the action ran: ${res.error ?? "ok"}`);
    const now = await nets();
    t(ua.pins.every((p, i) => now[p] === was[ub.pins[i]] && now[ub.pins[i]] === was[p]), `${ua.name} ${ua.pins.map((p) => was[p]).join("/")} and ${ub.name} ${ub.pins.map((p) => was[p]).join("/")} exchanged: ${ua.pins.map((p) => now[p]).join("/")} and ${ub.pins.map((p) => now[p]).join("/")}`);
    t(Object.keys(was).filter((n) => ![...ua.pins, ...ub.pins].includes(n)).every((n) => now[n] === was[n]), "the other pads are as they were");
    await undo();
    t(JSON.stringify(await nets()) === JSON.stringify(was), "one undo step");
    // Pads of both gates selected: the same, with no name to give.
    await select([`${g.ref}.${ua.pins[0]}`, `${g.ref}.${ub.pins[0]}`]);
    await __eda.run("pcbnew.InteractiveEdit.swapGateNets");
    await sleep(1300);
    const both = await nets();
    t(ua.pins.every((p, i) => both[p] === was[ub.pins[i]] && both[ub.pins[i]] === was[p]), "selecting a pad of each gate swaps them too");
    await undo();
    t(JSON.stringify(await nets()) === JSON.stringify(was), "undone");
  });

  await scenario("the header goes and the board is as it was and nothing logged an error", async (t) => {
    await undo();
    t((await snapshot()) === original, "every pad, track and via is as it was");
    const errs = __eda.errors(errorsFrom).filter((e) => !/Gate swapping must be performed/.test(e.message));
    t(errs.length === 0, `no errors: ${errs.map((e) => e.message).join("; ")}`);
  });

  if (strictWas && strict && !strict.checked) strict.click();
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
