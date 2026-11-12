// The Footprint Editor's click-through (UI-LEFT-2 FF10; PARITY-fpedit "Known gaps summary" 1 and 2), walked with real input: every click, drag, wheel turn
// and key goes through the browser's own input pipeline (`window.__runCheckInput`, Playwright's mouse and keyboard from e2e/run-check.mjs), and each step
// is compared with what KiCad's code does:
//   new + pads   File > New Footprint (`FOOTPRINT_EDITOR_CONTROL::NewFootprint`), the pad tool from the right toolbar (`PAD_TOOL::PlacePad`): each click puts
//                down a pad numbered after the last, Esc ends the tool; one undo step per pad;
//   renumber     Edit > Renumber Pads... (`PAD_TOOL::EnumeratePads`): the Pad Enumeration Settings dialog, then the loop -- a click numbers a pad, a click on
//                a numbered pad gives its number back (handed out first next time), a drag over pads numbers each one it crosses, the popup names the next
//                number, a double click commits it all as one undo step ("Renumber Pads"); Esc reverts everything and sends nothing;
//   main tools   a line (two clicks, then Esc), a rectangle and a circle (two clicks each), a text (a click, Text Properties, OK); a pad picked by a
//                click, dragged, turned with R, edited with E (Pad Properties), deleted with Del; every one an undo step of its own.
//
// Run it through e2e/run-check.mjs (it needs `__runCheckInput`) on a COPY of a board (`eda board new examples/mcu_board_30plus.yaml -o <dir>`). It makes a
// footprint of its own in the project library and deletes it at the end.
// It answers through `window.__fpClickCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=10
(async () => {
  const results = [];
  const check = (window.__fpClickCheck = { results, done: false, ok: false, wait: null });
  const json = (url, init) => fetch(url, { cache: "no-store", ...init }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => {
    const t0 = Date.now();
    while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text());
    return { done: check.done, ok: check.ok, n: results.length, results, failed: results.filter((r) => !r.ok).map((r) => r.name) };
  };

  // ------------------------------------------------------------------------------------------------------------------------ helpers
  const E = window.__eda;
  const state = () => E.state();
  const until = async (fn, ms = 8000) => { const t0 = Date.now(); let v; while (!(v = await fn()) && Date.now() - t0 < ms) await sleep(100); return v; };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const fpNames = async () => (await json("/api/footprint_library")).project ?? [];
  const footprint = async (name) => { const f = await json(`/api/footprint?name=${encodeURIComponent(name)}`); return { ...f, graphics: f.graphics ?? [], texts: f.texts ?? [], pads: f.pads ?? [] }; };

  // what the page sends to /api/cmd
  const posts = [];
  if (!window.__fpClickFetch) {
    const real = window.fetch.bind(window);
    window.__fpClickFetch = real;
    window.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input?.url ?? "";
      if (/\/api\/cmd\b/.test(url) && (init?.method ?? "GET") === "POST") posts.push(typeof init.body === "string" ? init.body : "");
      return real(input, init);
    };
  }

  // real input, played by the runner
  const input = async (ops) => {
    if (typeof window.__runCheckInput !== "function") throw new Error("no real input: run this check through e2e/run-check.mjs");
    const r = await window.__runCheckInput(ops);
    if (r !== "ok") throw new Error(`input: ${r}`);
  };
  const press = async (key, wait = 400) => { await input([{ t: "press", key }]); await sleep(wait); };
  const canvasEl = () => document.querySelector(".editor-canvas-col .pcb-canvas-container");
  const toScreen = (x, y) => { const v = state().view; const r = canvasEl().getBoundingClientRect(); return [r.left + v.x + x * v.scale, r.top + v.y + y * v.scale]; };
  const moveTo = async (x, y, steps = 6) => { const [px, py] = toScreen(x, y); await input([{ t: "move", x: px, y: py, steps }]); await sleep(150); };
  const clickAt = async (x, y, wait = 600) => {
    const [px, py] = toScreen(x, y);
    await input([{ t: "move", x: px, y: py, steps: 4 }, { t: "pause", ms: 100 }, { t: "down" }, { t: "pause", ms: 40 }, { t: "up" }]);
    await sleep(wait);
  };
  const dblAt = async (x, y, wait = 900) => { const [px, py] = toScreen(x, y); await input([{ t: "move", x: px, y: py, steps: 3 }, { t: "pause", ms: 100 }, { t: "dblclick", x: px, y: py }]); await sleep(wait); };
  const dragFromTo = async (a, b, wait = 1000, steps = 14) => {
    const [ax, ay] = toScreen(a[0], a[1]);
    const [bx, by] = toScreen(b[0], b[1]);
    await input([{ t: "move", x: ax, y: ay, steps: 3 }, { t: "pause", ms: 150 }, { t: "down" }, { t: "pause", ms: 60 }, { t: "move", x: bx, y: by, steps }, { t: "pause", ms: 250 }, { t: "up" }]);
    await sleep(wait);
  };
  const clickEl = async (el, wait = 300) => { el.scrollIntoView({ block: "nearest" }); const r = el.getBoundingClientRect(); await input([{ t: "click", x: r.left + r.width / 2, y: r.top + r.height / 2 }]); await sleep(wait); };
  const typeInto = async (el, text) => { await clickEl(el, 100); await input([{ t: "press", key: "Control+a" }, { t: "press", key: "Delete" }, ...(String(text) === "" ? [] : [{ t: "type", text: String(text) }])]); await sleep(250); };
  const clickTab = async (label) => { const b = [...document.querySelectorAll("button, [role=tab]")].find((x) => x.textContent.trim() === label); if (b) await clickEl(b, 1200); };
  const tool = async (action) => { const b = document.querySelector(`button.toolbar-button[data-action="${action}"]`); if (!b) throw new Error(`no toolbar button ${action}`); await clickEl(b, 400); };
  const menu = async (top, item) => {
    const m = [...document.querySelectorAll(".menubar-item")].find((x) => x.textContent.trim().startsWith(top));
    if (!m) throw new Error(`no menu ${top}`);
    await clickEl(m, 300);
    const it = [...m.querySelectorAll(".menu-node-item")].find((x) => item.test(x.textContent.trim()));
    if (!it) throw new Error(`no item ${item} in ${top}: ${[...m.querySelectorAll(".menu-node-item")].map((x) => x.textContent.trim()).join(" | ")}`);
    await clickEl(it, 800);
  };
  const dialog = () => [...document.querySelectorAll(".dialog")].pop() ?? null;
  const dialogTitle = () => dialog()?.querySelector(".dialog-header")?.textContent?.trim() ?? "";
  const buttonText = (root, text) => [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent.trim() === text);
  const toSelect = async () => { for (let i = 0; i < 4 && (state().tool !== "select" || document.querySelector(".dialog")); i++) await press("Escape", 300); };
  const undo = async () => { await press("Control+z", 900); };
  const popup = () => [...document.querySelectorAll('[role="status"]')].map((e) => e.textContent).find((x) => /^Click on pad /.test(x)) ?? null;

  const scenario = async (name, fn) => {
    const only = new URLSearchParams(location.hash.slice(1)).get("only");
    if (only && !new RegExp(only).test(name)) return;
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    try { await toSelect(); } catch { /* ignore */ }
    results.push(entry);
  };

  // --------------------------------------------------------------------------------------------------------------------------- run
  const MM = 1_000_000;
  const before = await fpNames();
  let FP = null;
  // the four pads of the walk, left to right, 5 mm apart
  const X = [-7.5 * MM, -2.5 * MM, 2.5 * MM, 7.5 * MM];
  const padAt = (fp, x) => fp.pads.find((p) => p.at.x === x && p.at.y === 0);

  (async () => {
    await scenario("File > New Footprint opens an empty footprint; the pad tool puts a pad at each click, numbered 1, 2, 3, 4; Esc ends it; one undo per pad", async (t) => {
      await clickTab("Footprint");
      t(state().tab === "footprint", `the Footprint Editor (${state().tab})`);
      await menu("File", /^New Footprint/);
      FP = await until(async () => (await fpNames()).find((n) => !before.includes(n)), 8000);
      if (!t(!!FP, `a new footprint in the project library: ${FP}`)) return;
      t((await footprint(FP)).pads.length === 0, "with no pads");
      // a view where a pad is easy to hit: fit, then the wheel over the origin until 1 mm is about 25 px
      await E.run("common.Control.zoomFitScreen");
      await sleep(500);
      const scales = [state().view.scale];
      for (let i = 0; i < 30 && state().view.scale < 2.5e-5; i++) { await moveTo(0, 0, 1); await input([{ t: "wheel", dy: -120 }]); await sleep(250); scales.push(state().view.scale); }
      for (let i = 0; i < 30 && state().view.scale > 4e-5; i++) { await moveTo(0, 0, 1); await input([{ t: "wheel", dy: 120 }]); await sleep(250); scales.push(state().view.scale); }
      t(state().view.scale >= 2.5e-5 && state().view.scale <= 4e-5, `the wheel zooms about the cursor to about 30 px a millimetre (${scales.map((x) => x.toExponential(2)).join(" ")})`);
      await tool("pcbnew.PadTool.placePad");
      t(state().tool === "pad", `the right toolbar's Add Pad arms the pad tool (${state().tool})`);
      for (const x of X) await clickAt(x, 0, 800);
      await press("Escape", 400);
      t(state().tool === "select", `Esc ends the tool (${state().tool})`);
      const fp = await footprint(FP);
      t(same(X.map((x) => padAt(fp, x)?.number), ["1", "2", "3", "4"]), `four pads where they were clicked, numbered in order (${fp.pads.map((p) => `${p.number}@${p.at.x / MM},${p.at.y / MM}`).join(" ")})`);
      await undo();
      t((await footprint(FP)).pads.length === 3, "one Ctrl+Z takes the last pad away");
      await press("Control+y", 900);
      t((await footprint(FP)).pads.length === 4, "Ctrl+Y puts it back");
    });

    await scenario("Renumber Pads: Esc in the loop reverts what was numbered and sends nothing", async (t) => {
      if (!FP) return t(false, "no footprint");
      await menu("Edit", /^Renumber Pads/);
      t(dialogTitle() === "Pad Enumeration Settings", `the dialog (${dialogTitle()})`);
      await clickEl(buttonText(dialog(), "OK"), 400);
      t(state().tool === "enumerate", `OK arms the tool (${state().tool})`);
      const n0 = posts.length;
      await clickAt(X[3], 0);
      t(/Click on pad 2/.test(popup() ?? ""), `a click numbers the pad; the popup asks for pad 2 (${popup()})`);
      await press("Escape", 600);
      t(state().tool === "select" && !popup(), `Esc ends the tool (${state().tool})`);
      t(posts.length === n0, `nothing was sent (${posts.slice(n0)})`);
      t(same((await footprint(FP)).pads.map((p) => p.number).sort(), ["1", "2", "3", "4"]), "the pads keep their numbers");
    });

    await scenario("Renumber Pads: clicks number pads, a second click gives a number back (handed out first), a drag numbers the pads it crosses, a double click commits as one undo step", async (t) => {
      if (!FP) return t(false, "no footprint");
      const fp0 = await footprint(FP);
      await menu("Edit", /^Renumber Pads/);
      const dlg = dialog();
      t(/Pad names are restricted to 4 characters \(including number\)\./.test(dlg?.textContent ?? ""), "the dialog's note, in KiCad's words");
      await typeInto(dlg.querySelector('[data-testid="enum-pads-prefix"]'), "P");
      await typeInto(dlg.querySelector('[data-testid="enum-pads-start"]'), "1");
      await typeInto(dlg.querySelector('[data-testid="enum-pads-step"]'), "1");
      await clickEl(buttonText(dlg, "OK"), 400);
      t(/^Click on pad P1\s*Press <esc> to cancel all; double-click to finish$/.test(popup() ?? ""), `the popup, in KiCad's words (${JSON.stringify(popup())})`);
      const n0 = posts.length;
      await clickAt(X[3], 0); // the rightmost pad: P1
      await clickAt(X[2], 0); // P2
      t(/Click on pad P3/.test(popup() ?? ""), `after two clicks the popup asks for P3 (${popup()})`);
      await clickAt(X[2], 0); // a second click: P2 is given back
      t(/Click on pad P2/.test(popup() ?? ""), `a second click gives P2 back; the popup asks for it again (${popup()})`);
      // a drag from the second pad over the first: P2 (the given-back number) then P3
      await dragFromTo([X[1], 0], [X[0], 0]);
      t(/Click on pad P4/.test(popup() ?? ""), `the drag numbered the two pads it crossed; the popup asks for P4 (${popup()})`);
      await clickAt(X[2], 0); // P4
      t(posts.length === n0, `nothing is sent while the loop runs (${posts.length - n0})`);
      // a double click on empty space commits
      await dblAt(0, 6 * MM, 1500);
      t(state().tool === "select" && !popup(), `the double click ends the tool (${state().tool})`);
      const fp = await footprint(FP);
      const nums = X.map((x) => padAt(fp, x)?.number);
      t(same(nums, ["P3", "P2", "P4", "P1"]), `left to right the pads read P3 P2 P4 P1 (${nums.join(" ")})`);
      // the numbers can be seen: the board's pad-number look (white strokes over the copper and the hole), not dark text a hole hides
      const cv = canvasEl().querySelector("canvas");
      const rect = cv.getBoundingClientRect();
      const k = cv.width / rect.width;
      const [sx, sy] = toScreen(X[3], 0);
      const px = cv.getContext("2d").getImageData(Math.round((sx - rect.left) * k) - 12, Math.round((sy - rect.top) * k) - 12, 25, 25).data;
      let light = 0;
      for (let i = 0; i < px.length; i += 4) if (px[i] > 200 && px[i + 1] > 200 && px[i + 2] > 200) light++;
      t(light >= 6, `the pad's number is drawn light over it (${light} light pixels in the middle of P1)`);
      const sent = posts.slice(n0);
      t(sent.length === 1 && /set_pad_numbers/.test(sent[0]), `one command (${sent.map((s) => s.slice(0, 80))})`);
      // the pad tool goes on from the last number the loop handed out (`SetLastPadNumber( "P4" )`, then `GetNextPadNumber`: P5)
      await tool("pcbnew.PadTool.placePad");
      await clickAt(0, 13 * MM, 900);
      await press("Escape", 400);
      const after5 = (await footprint(FP)).pads;
      const added = after5.find((p) => p.at.x === 0 && p.at.y === 13 * MM);
      t(added?.number === "P5", `the next pad placed is P5 (${added?.number}; pads ${after5.map((p) => `${p.number}@${p.at.x / MM},${p.at.y / MM}`).join(" ")})`);
      await undo();
      await undo();
      const back = await footprint(FP);
      t(same(X.map((x) => padAt(back, x)?.number), X.map((x) => padAt(fp0, x)?.number)), `one Ctrl+Z gives every pad its number back (${X.map((x) => padAt(back, x)?.number).join(" ")})`);
      await press("Control+y", 900);
    });

    await scenario("graphics: a line (two clicks, Esc), a rectangle and a circle (two clicks each), a text (a click, Text Properties, OK); one undo each", async (t) => {
      if (!FP) return t(false, "no footprint");
      const g0 = (await footprint(FP)).graphics.length;
      await tool("pcbnew.InteractiveDrawing.line");
      t(state().tool === "draw_segment", `Draw Line (${state().tool})`);
      await clickAt(-7.5 * MM, -5 * MM, 400);
      await clickAt(7.5 * MM, -5 * MM, 400);
      await press("Escape", 300);
      await press("Escape", 300);
      let g = (await footprint(FP)).graphics;
      const line = g[g.length - 1];
      t(g.length === g0 + 1 && line?.start?.x === -7.5 * MM && line?.start?.y === -5 * MM && line?.end?.x === 7.5 * MM && line?.end?.y === -5 * MM, `one line from the first click to the second (${JSON.stringify(g.slice(g0).map((x) => [x.kind ?? x.shape, x.start, x.end, x.layer]))})`);
      t(/F\.SilkS/.test(line?.layer ?? ""), `on F.Silkscreen (${line?.layer})`);
      await tool("pcbnew.InteractiveDrawing.rectangle");
      await clickAt(-7.5 * MM, 4 * MM, 400);
      await clickAt(-2.5 * MM, 7 * MM, 800);
      await tool("pcbnew.InteractiveDrawing.circle");
      await clickAt(5 * MM, 5.5 * MM, 400);
      await clickAt(6.5 * MM, 5.5 * MM, 800);
      await toSelect();
      g = (await footprint(FP)).graphics;
      const kinds = g.slice(g0).map((x) => x.kind ?? x.shape ?? x.type);
      t(g.length === g0 + 3, `three graphics (${JSON.stringify(g.slice(g0).map((x) => [x.kind ?? x.shape ?? x.type, x.start, x.end, x.center, x.radius]))})`);
      t(kinds.some((k) => /rect/i.test(k)) && kinds.some((k) => /circle/i.test(k)), `a rectangle and a circle (${kinds})`);
      const t0 = (await footprint(FP)).texts.length;
      await tool("pcbnew.InteractiveDrawing.text");
      await clickAt(0, 9 * MM, 800);
      const dlg = await until(() => (/Text Properties/.test(dialogTitle()) ? dialog() : null), 5000);
      if (t(!!dlg, `a click opens Text Properties (${dialogTitle()})`)) {
        const box = dlg.querySelector("textarea") ?? dlg.querySelector('input:not([type]), input[type="text"]');
        t(!!box && document.activeElement === box, `the text box has the focus (${document.activeElement?.tagName}.${document.activeElement?.className} ${document.activeElement?.getAttribute("aria-label") ?? ""})`);
        await typeInto(box, "CLICK");
        await clickEl(buttonText(dlg, "OK"), 900);
        // KiCad's text tool: the text rides the cursor until the next click puts it down
        if (state().carry || /text/.test(state().tool ?? "")) await clickAt(0, 9 * MM, 900);
      }
      await toSelect();
      const texts = (await footprint(FP)).texts;
      t(texts.length === t0 + 1 && texts.some((x) => /CLICK/.test(x.text ?? x.content ?? "")), `the text is there (${JSON.stringify(texts.slice(t0).map((x) => [x.text ?? x.content, x.at ?? x.pos]))})`);
      for (let i = 0; i < 4; i++) await undo();
      const after = await footprint(FP);
      t(after.graphics.length === g0 && after.texts.length === t0, `four Ctrl+Z take the four away (${after.graphics.length - g0} graphics, ${after.texts.length - t0} texts left)`);
    });

    await scenario("a pad picked by a click, dragged, turned with R, edited with E (Pad Properties), deleted with Del; one undo each", async (t) => {
      if (!FP) return t(false, "no footprint");
      const fp0 = await footprint(FP);
      const pad = padAt(fp0, X[0]);
      if (!t(!!pad, "the left pad")) return;
      await clickAt(X[0], 0);
      t(state().selection.some((s) => (s.id ?? s) === pad.id), `the click selects it (${JSON.stringify(state().selection)})`);
      await dragFromTo([X[0], 0], [X[0], 3 * MM]);
      let p = padAt(await footprint(FP), X[0]) ?? (await footprint(FP)).pads.find((q) => q.id === pad.id);
      t(p?.at.x === X[0] && p?.at.y === 3 * MM, `the drag moved it 3 mm down (${p?.at.x / MM}, ${p?.at.y / MM})`);
      await undo();
      p = (await footprint(FP)).pads.find((q) => q.id === pad.id);
      t(p?.at.y === 0, "Ctrl+Z puts it back");
      await clickAt(X[0], 0);
      await press("r", 900);
      p = (await footprint(FP)).pads.find((q) => q.id === pad.id);
      t(Math.abs((p?.rot ?? 0) - (pad.rot ?? 0)) === 90000, `R turns it a quarter, counter-clockwise (${pad.rot} -> ${p?.rot} millidegrees)`);
      await undo();
      await clickAt(X[0], 0);
      await press("e", 900);
      const dlg = await until(() => (/Pad Properties/.test(dialogTitle()) ? dialog() : null), 5000);
      if (t(!!dlg, `E opens Pad Properties (${dialogTitle()})`)) {
        const span = [...dlg.querySelectorAll(".kv-grid > span")].find((s) => /^Pad number/.test(s.textContent.trim()));
        const num = span?.nextElementSibling?.matches("input") ? span.nextElementSibling : span?.nextElementSibling?.querySelector("input");
        t(num?.value === pad.number, `its number reads ${pad.number} (${num?.value})`);
        await typeInto(num, "42");
        await clickEl(buttonText(dlg, "OK"), 1000);
        p = (await footprint(FP)).pads.find((q) => q.id === pad.id);
        t(p?.number === "42", `OK renumbers it (${p?.number})`);
        await undo();
      }
      await clickAt(X[0], 0);
      await press("Delete", 900);
      t(!(await footprint(FP)).pads.some((q) => q.id === pad.id), "Del deletes it");
      await undo();
      t((await footprint(FP)).pads.some((q) => q.id === pad.id), "Ctrl+Z brings it back");
    });

    // ----------------------------------------------------------------------------------------------------------------------- clean-up
    if (FP) {
      await toSelect();
      const r = E.run("pcbnew.ModuleEditor.deleteFootprint");
      const ask = await until(() => [...document.querySelectorAll(".dialog")].find((d) => /Delete footprint/.test(d.innerText)), 5000);
      if (ask) buttonText(ask, "Yes")?.click();
      await r;
      await sleep(800);
    }
    check.ok = results.length > 0 && results.every((r) => r.ok);
    check.done = true;
  })();
  return "started";
})();
