// Appearance panel check (GAPS.md item 18): drives the panel's own controls and `window.__eda` on a served board, reads what the canvas drew (pixels) and what the
// project file kept (`GET /api/appearance`), and puts every setting back. Run it in the page of a served board -- paste it into the Browser pane's javascript tool, or
// `page.evaluate( src )` in Playwright. It edits only the Appearance settings (never the board), but it does overwrite `appearance.json` of that board while it runs:
// run it on a COPY (or a scratch board) so the settings a person set are not lost. The board needs at least two nets and some routed copper; a net class or two make the
// class checks mean something (without one they check only the Default class). It starts from the Appearance settings a board has when nobody has touched them (everything
// it switches off it switches back ON, not to what it found) and its last check says whether it left them as they were.
//
// It returns { ok, failed, results: [{ name, ok, detail }] }. The checks:
//   objects     an object switched off is gone from `__eda.state().appearance` and from the canvas (tracks: fewer drawn pixels), and an opacity of 0 removes it too
//   net colours a net colour shows on copper in "All" and not in "None" (the net's own colour counted in the canvas), and `pcbnew.Control.netColorMode` goes round
//   contrast    `common.Control.highContrastModeCycle` cycles normal, dimmed, hidden
//   presets     Back Layers flips the board and hides F.Cu, All Layers undoes it, a saved preset comes back with its objects, a built-in name is refused, delete removes it
//   viewports   a viewport saved, the view moved, the viewport recalled it (the status bar's zoom is the same again)
//   nets        a net's eye hides its ratsnest set, a class's eye hides all its nets and the class is remembered, Show All brings them back
//   file        the settings reach `appearance.json` (`local` and `project` sections), within a second or so
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const results = [];
  const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail: ok ? "" : String(detail) });
  const op = (o) => __eda.run("studio.Appearance.op", o);
  const app = () => __eda.state().appearance;
  const canvas = () => document.querySelector(".pcb-canvas-container canvas");
  const count = (pred) => {
    const c = canvas();
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) if (pred(d[i], d[i + 1], d[i + 2])) n++;
    return n;
  };
  const zoomText = () => [...document.querySelectorAll(".field")].find((f) => f.textContent.startsWith("Z "))?.textContent;
  const setSelect = async (selector, value) => {
    const el = document.querySelector(selector);
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("change", { bubbles: true }));
    await sleep(250);
  };
  const setText = async (el, value) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(80);
  };
  const dialogButton = async (label) => {
    [...document.querySelectorAll(".dialog-footer button")].find((b) => b.textContent === label)?.click();
    await sleep(250);
  };
  const file = () => fetch("/api/appearance", { cache: "no-store" }).then((r) => r.json());

  // The dock: open it if it is folded, and look at the Objects tab.
  document.querySelector('button[aria-label="Show Appearance"]')?.click();
  await sleep(300);
  await __eda.run("common.Control.zoomFitScreen");
  const tab = async (id) => {
    document.querySelector(`.ap-root [data-tab="${id}"]`)?.click();
    await sleep(200);
  };
  await tab("objects");
  check("the panel is on screen with its four tabs", document.querySelectorAll(".ap-root [data-tab]").length === 4);
  const startedWith = JSON.stringify(app());

  // ------------------------------------------------------------------------------------------------------------------------------ objects
  const eyes = [...document.querySelectorAll(".ap-row[data-object]")].map((r) => r.getAttribute("data-object"));
  check("the Objects tab lists the 20 rows", eyes.length === 20, eyes.join(","));
  const bg = canvas().getContext("2d").getImageData(2, 2, 1, 1).data;
  const inked = () => count((r, g, b) => Math.abs(r - bg[0]) + Math.abs(g - bg[1]) + Math.abs(b - bg[2]) > 90);
  const all = inked();
  await op({ op: "object", id: "tracks", visible: false });
  await sleep(250);
  const noTracks = inked();
  check("Tracks off: the object is listed off and the canvas draws less", app().hiddenObjects.includes("tracks") && noTracks < all, `${all} -> ${noTracks}`);
  await op({ op: "object", id: "tracks", visible: true });
  await op({ op: "opacity", key: "tracks", value: 0 });
  await sleep(250);
  const clearTracks = inked();
  check("Tracks at opacity 0 draw like Tracks off", Math.abs(clearTracks - noTracks) < Math.max(40, noTracks * 0.02), `${noTracks} vs ${clearTracks}`);
  await op({ op: "opacity", key: "tracks", value: 1 });
  await op({ op: "object", id: "footprint_text", visible: false });
  check("Footprint Text drags References and Values with it", ["footprint_references", "footprint_values", "footprint_text"].every((o) => app().hiddenObjects.includes(o)));
  await op({ op: "object", id: "footprint_text", visible: true });

  // ------------------------------------------------------------------------------------------------------------------------ net colours
  const net = [...document.querySelectorAll("[data-net]")].map((n) => n.getAttribute("data-net"))[0];
  await tab("nets");
  const nets = [...document.querySelectorAll("[data-net]")].map((n) => n.getAttribute("data-net"));
  check("the Nets tab lists the board's nets", nets.length >= 2, nets.join(","));
  const target = nets.find((n) => n === "GND") ?? net;
  await op({ op: "net_color", net: target, color: "rgb(0, 255, 0)" });
  const green = () => count((r, g, b) => g > 200 && r < 60 && b < 60);
  await op({ op: "net_color_mode", mode: "off" });
  await sleep(250);
  const off = green();
  await op({ op: "net_color_mode", mode: "all" });
  await sleep(250);
  const onAll = green();
  check("a net colour shows on copper in the All mode only", onAll > off + 20, `none ${off}, all ${onAll}`);
  await op({ op: "net_color_mode", mode: "all" });
  const modes = [];
  for (let i = 0; i < 3; i++) {
    await __eda.run("pcbnew.Control.netColorMode");
    modes.push(app().netColorMode);
  }
  check("pcbnew.Control.netColorMode goes all -> ratsnest -> off -> all", modes.join(",") === "ratsnest,off,all", modes.join(","));
  await op({ op: "net_color", net: target, color: null });
  await op({ op: "net_color_mode", mode: "ratsnest" });

  // ------------------------------------------------------------------------------------------------------------------------------ contrast
  const cycle = [app().contrast];
  for (let i = 0; i < 3; i++) {
    await __eda.run("common.Control.highContrastModeCycle");
    cycle.push(app().contrast);
  }
  check("the inactive-layer mode cycles normal, dimmed, hidden, normal", cycle.join(",") === "normal,dimmed,hidden,normal", cycle.join(","));

  // -------------------------------------------------------------------------------------------------------------------------------- presets
  await op({ op: "select_preset", name: "Back Layers" });
  check("Back Layers hides F.Cu and flips the board", app().hiddenLayers.includes("F.Cu") && app().flipped && app().preset === "Back Layers", JSON.stringify(app()));
  await op({ op: "select_preset", name: "All Layers" });
  check("All Layers shows everything again", app().hiddenLayers.length === 0 && !app().flipped && app().preset === "All Layers", JSON.stringify(app()));
  await tab("layers");
  await op({ op: "object", id: "vias", visible: false });
  await setSelect("#ap-presets", "cmd:save");
  check("Save preset... opens its dialog", document.querySelector(".dialog-header")?.textContent === "Save Layer Preset");
  await setText(document.querySelector(".dialog input"), "check preset");
  await dialogButton("OK");
  check("the preset is saved and shown as the current one", app().presets.includes("check preset") && app().preset === "check preset", JSON.stringify(app()));
  await setSelect("#ap-presets", "cmd:save");
  await setText(document.querySelector(".dialog input"), "Front Layers");
  await dialogButton("OK");
  check("a built-in name is refused", document.querySelector(".dialog-header")?.textContent === "Error", document.querySelector(".dialog-header")?.textContent);
  await dialogButton("OK");
  await op({ op: "select_preset", name: "All Layers" });
  await op({ op: "select_preset", name: "check preset" });
  check("the saved preset brings its objects back", app().hiddenObjects.includes("vias"), JSON.stringify(app().hiddenObjects));
  await setSelect("#ap-presets", "cmd:delete");
  await dialogButton("OK");
  check("Delete preset... removes it", !app().presets.includes("check preset"));
  await op({ op: "object", id: "vias", visible: true });

  // ------------------------------------------------------------------------------------------------------------------------------ viewports
  const zoom0 = zoomText();
  await setSelect("#ap-viewports", "cmd:save");
  await setText(document.querySelector(".dialog input"), "check view");
  await dialogButton("OK");
  await __eda.run("common.Control.zoomInCenter");
  await __eda.run("common.Control.zoomInCenter");
  const zoom1 = zoomText();
  await setSelect("#ap-viewports", "v:check view");
  await sleep(250);
  check("a saved viewport comes back to the same zoom", app().viewports.includes("check view") && zoom1 !== zoom0 && zoomText() === zoom0, `${zoom0} -> ${zoom1} -> ${zoomText()}`);
  await setSelect("#ap-viewports", "cmd:delete");
  await dialogButton("OK");
  check("Delete viewport... removes it", !app().viewports.includes("check view"));

  // ---------------------------------------------------------------------------------------------------------------------------------- nets
  await op({ op: "net_visible", net: target, visible: false });
  check("a net's eye hides its ratsnest", app().hiddenNets.includes(target));
  await op({ op: "show_all_nets" });
  check("Show All Nets shows every net", app().hiddenNets.length === 0);
  await tab("classes");
  const classes = [...document.querySelectorAll("[data-netclass]")].map((n) => n.getAttribute("data-netclass"));
  check("the Net Classes tab lists the Default class first", classes[0] === "Default", classes.join(","));
  const other = classes.find((c) => c !== "Default");
  if (other) {
    await op({ op: "netclass_visible", name: other, visible: false });
    check("a class's eye hides its nets and is remembered", app().hiddenNetclasses.includes(other) && app().hiddenNets.length > 0, JSON.stringify(app()));
    await op({ op: "show_all_netclasses" });
    check("Show All Netclasses brings them back", app().hiddenNets.length === 0 && app().hiddenNetclasses.length === 0);
    await op({ op: "netclass_color", name: other, color: "rgb(255, 160, 0)" });
    check("a class colour is kept", app().netclassColors[other] === "rgb(255, 160, 0)");
    await op({ op: "netclass_color", name: other, color: null });
  }

  // ------------------------------------------------------------------------------------------------------------------------------------ file
  await op({ op: "net_color", net: target, color: "rgb(1, 2, 3)" });
  await op({ op: "opacity", key: "zones", value: 0.4 });
  await sleep(1400);
  const saved = await file();
  check("the settings reach appearance.json (local and project sections)", saved.project?.net_colors?.[target] === "rgb(1, 2, 3)" && saved.local?.opacity?.zones === 0.4 && Array.isArray(saved.local?.visible_items), JSON.stringify(saved).slice(0, 300));

  // Put it back.
  await op({ op: "net_color", net: target, color: null });
  await op({ op: "opacity", key: "zones", value: 0.6 });
  await sleep(1000);
  check("the settings are back as they were", JSON.stringify(app()) === startedWith, `${startedWith}\n${JSON.stringify(app())}`);

  const failed = results.filter((r) => !r.ok);
  return { ok: failed.length === 0, failed: failed.map((r) => r.name), results };
})();
