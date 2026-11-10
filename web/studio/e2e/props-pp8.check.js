// The Properties panel's rows UI-LEFT-2 PP8 made writeable (PARITY-pcb section 104): for each, select the item, edit the row through the grid the way a user
// does, read the edit back from /api/state, undo it, and check the item is as it was. A pad's Pad Type "Edge connector" and its Pad Number (`set_pad_number`,
// which keeps the pad's net); a through-hole pad's Backdrill Mode and Top Post-machining; a size of 0 taken where KiCad takes it (a pad's Size X; a text's Width,
// clamped to TEXT_MIN_SIZE_MM) and a negative one refused; a copper rectangle's Net (`set_item_net`); a footprint field's Auto Thickness and its own Locked.
// It adds a text and a copper rectangle and takes them away at the end.
// run-all-checks: board=mcu30-placed minutes=8
(async () => {
  const results = [];
  const check = (window.__propsPp8Check = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, init).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a fetch loop: a hidden pane throttles timers
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };
  const t = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail });

  (async () => {
    const post = (cmd) => json("/api/cmd", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ cmd, strict: false }) });
    const row = (name) => document.querySelector(`.prop-row[data-prop="${name}"]`);
    const input = (name) => row(name)?.querySelector("[data-prop-input]");
    const lastAct = async () => (await json("/api/state")).activity[0] ?? null;
    const select = async (ids) => {
      await __eda.run("common.InteractiveSelection.clear");
      await __eda.run("common.InteractiveSelection.selectItems", ids);
      await sleep(350);
    };
    const undo = async () => { await __eda.run("common.Interactive.undo"); await sleep(600); };
    const waitAct = async (before) => {
      const t0 = Date.now();
      while (Date.now() - t0 < 10000) {
        const a = await lastAct();
        if (a && (!before || a.t !== before.t || a.cmd !== before.cmd)) { await sleep(400); return a; }
        await sleep(50);
      }
      return null;
    };
    /** Type or pick `value` in the row's editor, as a user does, and wait for the command the grid sent. */
    const edit = async (name, value, wait = true) => {
      for (let i = 0; i < 25 && !input(name); i++) await sleep(200);
      const el = input(name);
      if (!el) return row(name) ? `read-only row ${name}: ${row(name).dataset.note ?? ""}` : `no row ${name}`;
      const before = await lastAct();
      if (el instanceof HTMLSelectElement) {
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, String(value));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      } else if (el.type === "checkbox") {
        if (el.checked === !!value) return "already";
        el.click();
      } else {
        el.focus();
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, String(value));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        await sleep(30);
        el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      }
      if (!wait) { await sleep(500); return "sent"; }
      const a = await waitAct(before);
      if (!a) return document.querySelector(".prop-error")?.textContent ? `refused: ${document.querySelector(".prop-error").textContent}` : "no command sent";
      return a.ok ? `ok: ${a.cmd.slice(0, 90)}` : `REFUSED: ${a.message.slice(0, 120)}`;
    };
    /** Select `ids`, edit `name` to `value`, check `read()` is `want`, undo, check `read()` is what it was before. */
    const scenario = async (label, ids, name, value, read, want) => {
      await select(ids);
      const was = JSON.stringify(await read());
      const r = await edit(name, value);
      const got = await read();
      const applied = r.startsWith("ok");
      const okEdit = applied && JSON.stringify(got) === JSON.stringify(want);
      if (applied) await undo();
      const back = JSON.stringify(await read());
      t(`${label}: ${name} = ${value}`, okEdit && back === was, `edit: ${r}; read ${JSON.stringify(got)} (want ${JSON.stringify(want)}); after undo ${back === was ? "as before" : back}`);
    };
    if (__eda.state().tab !== "pcb") {
      [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === "PCB")?.click();
      await sleep(1500);
    }
    document.querySelector('button[aria-label="Show Properties"]')?.click();

    let s = await json("/api/state");
    const added = [];
    const r1 = await post({ op: "add_text", text: { content: "PP8", at: { x: 20_000_000, y: 30_000_000 }, angle: 0, layer: "F.SilkS", size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false } });
    if (r1.ok !== false) added.push("text");
    const r2 = await post({ op: "add_shape", shape: { kind: "rect", layer: "F.Cu", stroke_width: 100_000, filled: true, start: { x: 4_000_000, y: 30_000_000 }, end: { x: 9_000_000, y: 33_000_000 } } });
    if (r2.ok !== false) added.push("rect");
    await sleep(600);
    s = await json("/api/state");
    const state = () => json("/api/state");
    const partOf = async (ref) => (await state()).parts.find((p) => p.ref === ref);
    const padOf = async (ref, num) => (await partOf(ref))?.pads?.find((p) => p.num === num);
    const text = (s.drawings?.texts ?? []).find((x) => x.content === "PP8");
    const rect = (s.drawings?.shapes ?? []).filter((x) => x.kind === "rect" && x.layer === "F.Cu").at(-1);
    const placed = s.parts.filter((p) => p.placed && p.pads?.length);
    const smdPart = placed.find((p) => p.pads.every((q) => (q.kind ?? (q.th ? "through_hole" : "smd")) === "smd"));
    const thPart = placed.find((p) => p.pads.some((q) => (q.kind ?? (q.th ? "through_hole" : "smd")) === "through_hole" && !q.slot));

    // ------------------------------------------------------------------------------------------------ a pad: type, number, size
    if (smdPart) {
      const pad = smdPart.pads[0];
      const id = `${smdPart.ref}.${pad.num}`;
      await select([id]);
      const types = [...(input("Pad Type")?.options ?? [])].map((o) => o.textContent);
      t("pad: Pad Type offers KiCad's four (PAD_ATTRIB)", JSON.stringify(types) === JSON.stringify(["Through-hole", "SMD", "Edge connector", "NPTH, mechanical"]), JSON.stringify(types));
      await scenario("SMD pad", [id], "Pad Type", "conn", async () => (await padOf(smdPart.ref, pad.num))?.connector === true, true);
      // `PAD::SetNumber`: the pad renamed, its net kept (its new pin is on the net the old one was on)
      await scenario("SMD pad", [id], "Pad Number", "77", async () => {
        const p = await partOf(smdPart.ref);
        return { nums: p.pads.map((q) => q.num).sort(), net: p.pads.find((q) => q.num === "77")?.net ?? null };
      }, { nums: smdPart.pads.map((q) => (q === pad ? "77" : q.num)).sort(), net: pad.net ?? null });
      // `PGPROPERTY_SIZE` takes 0 (`PAD_DESC` registers no validator) and refuses a negative size
      await scenario("SMD pad", [id], "Size X", "0", async () => { const p = await padOf(smdPart.ref, pad.num); return (p.size ?? [p.w, p.h])[0]; }, 0);
      await select([id]);
      const neg = await edit("Size X", "-0.1", false);
      const err = document.querySelector(".prop-error")?.textContent ?? "";
      const after = await padOf(smdPart.ref, pad.num);
      t("SMD pad: a negative Size X is refused", (after.size ?? [after.w])[0] === (pad.size ?? [pad.w])[0] && err.length > 0, `${neg}; error "${err}"`);
      await select([]);
    } else t("SMD pad", false, "no placed footprint with SMD pads");

    if (thPart) {
      const pad = thPart.pads.find((q) => (q.kind ?? (q.th ? "through_hole" : "smd")) === "through_hole" && !q.slot);
      const id = `${thPart.ref}.${pad.num}`;
      // `PADSTACK::SetBackdrillMode( BACKDRILL_BOTTOM )`: a backdrill from B.Cu
      await scenario("through-hole pad", [id], "Backdrill Mode", "bottom", async () => (await padOf(thPart.ref, pad.num))?.drill_props?.backdrill?.start ?? null, "B.Cu");
      await scenario("through-hole pad", [id], "Top Post-machining", "countersink", async () => (await padOf(thPart.ref, pad.num))?.drill_props?.front_post_machining?.mode ?? null, "countersink");
    } else t("through-hole pad", false, "no placed footprint with a round-holed through-hole pad");

    // ------------------------------------------------------------------------------------------------ a copper shape's net
    if (rect) {
      const nets = (s.nets ?? []).map((n) => (typeof n === "string" ? n : n.name)).filter(Boolean);
      const want = nets.includes("GND") ? "GND" : nets[0];
      await scenario("copper rectangle", [rect.id], "Net", want, async () => (await state()).drawings.shapes.find((x) => x.id === rect.id)?.net ?? null, want);
    } else t("copper rectangle", false, "no rectangle was added");

    // ------------------------------------------------------------------------------------------------ a text's size of 0
    if (text) {
      // `SetTextWidth( 0 )` clamps to TEXT_MIN_SIZE_MM (1 um)
      await scenario("text", [text.id], "Width", "0", async () => { const x = (await state()).drawings.texts.find((v) => v.id === text.id); return x.width ?? x.size; }, 1_000);
    } else t("text", false, "no text was added");

    // ------------------------------------------------------------------------------------------------ a footprint field
    const fpart = smdPart ?? placed[0];
    if (fpart) {
      const fid = `${fpart.ref}:Reference`;
      const fieldOf = async () => (await partOf(fpart.ref)).fields?.find((f) => f.name === "Reference");
      const f0 = await fieldOf();
      // `SetAutoThickness( true )`: a thickness of 0
      if (f0 && f0.thickness !== 0) await scenario("field", [fid], "Auto Thickness", true, async () => (await fieldOf()).thickness, 0);
      else t("field: Auto Thickness", false, `the field's thickness is ${f0?.thickness}`);
      // the field's own lock (`BOARD_ITEM::SetLocked` of the PCB_FIELD), not its footprint's
      await scenario("field", [fid], "Locked", true, async () => { const l = (await state()).locked ?? []; return [l.includes(fid), l.includes(fpart.ref)]; }, [true, false]);
    } else t("field", false, "no placed footprint");

    for (let i = 0; i < added.length; i++) await undo();
    const end = await state();
    t("the added text and rectangle are gone", !(end.drawings?.texts ?? []).some((x) => x.content === "PP8") && !(end.drawings?.shapes ?? []).some((x) => rect && x.id === rect.id), "");

    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })().catch((e) => {
    t("the check ran", false, String((e && e.stack) || e));
    check.done = true;
  });
  return "started";
})();
