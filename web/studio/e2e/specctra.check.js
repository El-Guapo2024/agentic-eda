// Specctra check (GAPS.md item 19, package P19b, PARITY-pcb.md "Specctra DSN export and session import"): File > Export > Specctra DSN... and File > Import > Specctra
// Session..., driven through `window.__eda`. Run it in the page of a served board that is placed and ROUTED with vias -- `eda pipeline examples/opamp_filter.yaml -o <p>`,
// `eda board new examples/opamp_filter.yaml -o <dir>`, `cp <p>/design.json <dir>/`, `eda board serve -C <dir> --ui web/studio/dist` -- pasted into the Browser pane's
// javascript tool, or `page.evaluate( src )` in Playwright (`~/tools/run-check.mjs`). It changes nothing that stays: the import is taken back with Undo and the board compared
// with how it was. Nothing is downloaded: the page's file save is held back and the text it was given read instead.
//
// What it checks:
//   both actions are offered on the PCB tab;
//   Export: the DSN is the board's (`(pcb ..` named by the file, its parser, the board's layers, a wire for every routed net, its vias), saved as `<board>.dsn`, the
//     status says "BOARD exported OK.", and the server's reply names the engine that wrote it (`kicad-cli` or `eda`);
//   Import: a session made from that DSN's wiring (what a router that keeps the routing sends back) is put on the board as ONE undo step -- the same segments as before,
//     the same vias, "Session file imported and merged OK." -- and Undo brings the board's own tracks back, Redo the imported ones;
//   a session KiCad refuses (a layer the board has not got) is refused in KiCad's words and the board is untouched;
//   nothing logged an error that was not asked for.
//
// It answers through `window.__specctraCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }; read it with `await __specctraCheck.wait(30000)` until `.done`.
// run-all-checks: board=opamp-routed   (the board tools/run-all-checks.sh serves this check on, and how long it may take)
(async () => {
  const results = [];
  const check = (window.__specctraCheck = { results, done: false, ok: false, wait: null });
  const json = (url) => fetch(url, { cache: "no-store" }).then((r) => r.json());
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); }; // a hidden pane throttles timers
  check.wait = async (ms = 60000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const E = window.__eda;
  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  // The board's copper as segments and vias, order aside (`/api/state`'s routing: a track's points as [x, y], a via's x, y, d, from, to): a track of n points is n - 1 segments.
  const copper = async () => {
    const r = (await json("/api/state")).routing ?? {};
    const segs = (r.tracks ?? []).flatMap((t) => t.pts.slice(1).map((p, i) => [t.net, t.layer, t.width, ...t.pts[i], ...p].join(",")));
    const vias = (r.vias ?? []).map((v) => [v.net, v.x, v.y, v.d, v.drill, v.from, v.to].join(","));
    return { segs: segs.sort(), vias: vias.sort(), tracks: (r.tracks ?? []).length };
  };
  const same = (a, b) => JSON.stringify(a.segs) === JSON.stringify(b.segs) && JSON.stringify(a.vias) === JSON.stringify(b.vias);

  // The page's downloads, held back.
  const saved = [];
  const realCreate = URL.createObjectURL;
  const realClick = HTMLAnchorElement.prototype.click;
  URL.createObjectURL = (blob) => { const url = realCreate.call(URL, blob); saved.push({ url, blob, name: null }); return url; };
  HTMLAnchorElement.prototype.click = function () {
    if (this.download) { const s = saved.find((d) => d.url === this.href); if (s) s.name = this.download; return; }
    return realClick.call(this);
  };

  // The balanced s-expressions of `text` that start with `head` (quoted strings respected).
  const forms = (text, head) => {
    const out = [];
    let i = text.indexOf(head);
    while (i >= 0) {
      let depth = 0, q = false, j = i;
      for (; j < text.length; j++) {
        const c = text[j];
        if (c === '"') q = !q;
        else if (!q && c === "(") depth++;
        else if (!q && c === ")" && --depth === 0) break;
      }
      out.push(text.slice(i, j + 1));
      i = text.indexOf(head, j + 1);
    }
    return out;
  };
  // A session of the DSN's own wiring: its wires and vias as they are, in micrometres (`(resolution um 1)`), the via padstacks as the library_out.
  const sessionOf = (dsn) => {
    const wiring = dsn.slice(dsn.indexOf("(wiring"));
    const wires = forms(wiring, "(wire ");
    const vias = forms(wiring, "(via ");
    const netOf = (f) => (f.match(/\(net ("[^"]*"|[^\s()]+)\)/) || [])[1] ?? "";
    const padstacks = [...new Set(vias.map((v) => v.match(/^\(via ("[^"]*"|[^\s()]+)/)[1]))];
    const library = forms(dsn.slice(dsn.indexOf("(library"), dsn.indexOf("(network")), "(padstack ").filter((p) => padstacks.some((id) => p.startsWith(`(padstack ${id}`)));
    const nets = [...new Set([...wires, ...vias].map(netOf))];
    const body = nets.map((n) => `   (net ${n}\n${[...wires, ...vias].filter((f) => netOf(f) === n).map((f) => `    ${f}`).join("\n")})`).join("\n");
    return { text: `(session check.ses\n (base_design check.dsn)\n (routes\n  (resolution um 1)\n  (library_out\n${library.join("\n")})\n  (network_out\n${body})))\n`, wires: wires.length, vias: vias.reduce((n, v) => n + 1, 0) };
  };

  const errorsFrom = Date.now();
  if (E.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const original = await copper();
  let dsn = "";

  await scenario("the board to check is routed, with vias", async (t) => {
    t(original.segs.length > 0 && original.vias.length > 0, `${original.segs.length} segments, ${original.vias.length} vias`);
    for (const id of ["pcbnew.EditorControl.exportSpecctraDSN", "pcbnew.EditorControl.importSpecctraSession"]) {
      const a = E.actions().find((x) => x.id === id);
      t(a?.enabled === true, `${id} is offered on the PCB tab: ${JSON.stringify(a)}`);
    }
  });

  await scenario("Export Specctra DSN... saves the board's DSN as <board>.dsn and says BOARD exported OK.", async (t) => {
    const r = await E.run("pcbnew.EditorControl.exportSpecctraDSN");
    t(r.ok !== false, `the action ran: ${JSON.stringify(r)}`);
    await sleep(300);
    const s = saved.find((d) => d.name);
    t(!!s && s.name.endsWith(".dsn"), `saved as ${s?.name}`);
    dsn = s ? await s.blob.text() : "";
    t(dsn.startsWith("(pcb ") && /\(host_cad "KiCad's Pcbnew"\)/.test(dsn) && /\(layer F\.Cu/.test(dsn) && /\(layer B\.Cu/.test(dsn), dsn.slice(0, 120));
    const wires = forms(dsn, "(wire ").length;
    const vias = forms(dsn.slice(dsn.indexOf("(wiring")), "(via ").length;
    t(wires > 0 && vias === original.vias.length, `${wires} wires, ${vias} vias in the wiring (the board has ${original.vias.length} vias)`);
    t(/^BOARD exported OK\./.test(r.toast?.message ?? r.toast ?? ""), `the status: ${JSON.stringify(r.toast)}`);
    // The reply names the engine that wrote the file (kicad-cli where it has `pcb export specctra`, else our own exporter); the page shows the same either way.
    const reply = await fetch("/api/specctra/export", { method: "POST", body: "{}" }).then((x) => x.json());
    t(reply.ok && ["kicad-cli", "eda"].includes(reply.engine) && reply.text === dsn, `the reply's engine: ${reply.engine}, the same text as the download`);
  });

  await scenario("Import Specctra Session... puts the session's wires and vias on the board as one undo step", async (t) => {
    const ses = sessionOf(dsn);
    t(ses.wires > 0, `a session of ${ses.wires} wires and ${ses.vias} vias`);
    const r = await E.run("pcbnew.EditorControl.importSpecctraSession", { name: "check.ses", text: ses.text });
    t(r.ok !== false && /^Session file imported and merged OK\./.test(r.toast?.message ?? r.toast ?? ""), `the status: ${JSON.stringify(r.toast ?? r.error)}`);
    await sleep(500);
    const now = await copper();
    t(same(now, original), `the same segments (${now.segs.length}) and vias (${now.vias.length}) as before`);
    t(now.tracks === now.segs.length, `a track for each segment of the session (${now.tracks} tracks), where the board had ${original.tracks}`);
    await E.run("common.Interactive.undo");
    await sleep(600);
    const back = await copper();
    t(same(back, original) && back.tracks === original.tracks, `one Undo brings the board's own tracks back (${back.tracks})`);
    await E.run("common.Interactive.redo");
    await sleep(600);
    t((await copper()).tracks === now.tracks, "Redo brings the import again");
    await E.run("common.Interactive.undo");
    await sleep(600);
    t((await copper()).tracks === original.tracks, "and Undo takes it away again");
  });

  await scenario("a session with a layer the board has not got is refused in KiCad's words, the board untouched", async (t) => {
    const bad = sessionOf(dsn).text.replace(/\(path F\.Cu /, "(path In7.Cu ");
    const r = await E.run("pcbnew.EditorControl.importSpecctraSession", { name: "bad.ses", text: bad });
    const said = r.toast?.message ?? r.toast ?? "";
    t(/Session file uses invalid layer id 'In7\.Cu'\./.test(said), `the error: ${JSON.stringify(said)}`);
    await sleep(300);
    const now = await copper();
    t(same(now, original) && now.tracks === original.tracks, "the board is as it was");
  });

  await scenario("nothing logged an error that was not asked for", async (t) => {
    const errs = E.errors(errorsFrom).filter((e) => !/invalid layer id/.test(e.message));
    t(errs.length === 0, `no errors: ${errs.map((e) => e.message).join("; ")}`);
  });

  URL.createObjectURL = realCreate;
  HTMLAnchorElement.prototype.click = realClick;
  check.ok = results.every((r) => r.ok);
  check.done = true;
})();
