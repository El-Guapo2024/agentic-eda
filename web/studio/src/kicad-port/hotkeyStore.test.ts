import { test } from "node:test";
import assert from "node:assert/strict";
import {
  KEY_NON_FOUND,
  MD_ALT,
  MD_CTRL,
  MD_SHIFT,
  WXK,
  applyHotkeyChange,
  checkKeyConflicts,
  conflictMessage,
  filterMatches,
  findHotkey,
  hotkeyRow,
  importHotkeys,
  initHotkeySections,
  keyCodeFromKeyName,
  keyCodeToStudioHotkey,
  keyName,
  mapKeypressToKeycode,
  nextHotkeySort,
  sortHotkeySections,
  planHotkeyChange,
  readHotKeyConfig,
  reservedMessage,
  resetAllHotkeys,
  resetSteps,
  savedKeys,
  studioHotkeyToKeyCode,
  toolName,
  writeHotKeyConfig,
  type HotkeyAction,
  type HotkeySection,
  type KeyPress,
} from "./hotkeyStore";

const K = (c: string) => c.charCodeAt(0);

function action(name: string, key = 0, opts: Partial<HotkeyAction> = {}): HotkeyAction {
  return { name, label: name.split(".").pop()!, description: "", scope: "AS_GLOBAL", defaultKey: key, defaultKeyAlt: 0, key, keyAlt: 0, ...opts };
}

test("KeyNameFromKeyCode / KeyCodeFromKeyName: KiCad's names, the modifiers in KiCad's order, Mac's Cmd and Option", () => {
  assert.equal(keyName(MD_CTRL + MD_SHIFT + K("Z"), false), "Ctrl+Shift+Z");
  assert.equal(keyName(MD_CTRL + MD_ALT + MD_SHIFT + K("Z"), true), "Cmd+Option+Shift+Z");
  assert.equal(keyName(WXK.BACK, false), "Back");
  assert.equal(keyName(WXK.INSERT, false), "Ins");
  assert.equal(keyName(WXK.RETURN, false), "Return");
  assert.equal(keyName(WXK.NUMPAD0 + 5, false), "Num Pad 5");
  assert.equal(keyName(MD_SHIFT + WXK.NUMPAD_ENTER, false), "Shift+Num Pad Enter");
  assert.equal(keyName(0, false), "", "0 is unassigned");
  assert.equal(keyName(MD_CTRL + K("+"), false), "Ctrl++");
  assert.equal(keyName(999, false), "<unknown>");
  assert.equal(keyCodeFromKeyName("Ctrl+Shift+Z", false), MD_CTRL + MD_SHIFT + K("Z"));
  assert.equal(keyCodeFromKeyName("Cmd+Z", false), MD_CTRL + K("Z"), "Cmd+ is Ctrl on every platform");
  assert.equal(keyCodeFromKeyName("Option+Z", true), MD_ALT + K("Z"));
  assert.equal(keyCodeFromKeyName("Option+Z", false), KEY_NON_FOUND, "Option+ is Alt only on macOS");
  assert.equal(keyCodeFromKeyName("num pad 5", false), WXK.NUMPAD0 + 5, "names compare without case");
  assert.equal(keyCodeFromKeyName("PgDn", false), WXK.PAGEDOWN);
  assert.equal(keyCodeFromKeyName("Ctrl++", false), MD_CTRL + K("+"));
  assert.equal(keyCodeFromKeyName("", false), 0);
  assert.equal(keyCodeFromKeyName("Bogus", false), KEY_NON_FOUND);
  assert.equal(keyCodeFromKeyName("Win+A", false), K("A") + 0x8000);
});

test("the studio's hotkey strings to key codes and back (Backspace / Insert / Enter are KiCad's Back / Ins / Return)", () => {
  for (const s of ["Ctrl+Shift+Z", "Del", "Backspace", "Insert", "Enter", "Ctrl++", "+", "Space", "Shift+Tab", "Alt+Left", "F12", "Num Pad 3", "PgUp", "Ctrl+Alt+Shift+F1", "`", "<"]) {
    assert.equal(keyCodeToStudioHotkey(studioHotkeyToKeyCode(s)), s, s);
  }
  assert.equal(studioHotkeyToKeyCode("Backspace"), WXK.BACK);
  assert.equal(studioHotkeyToKeyCode("Enter"), WXK.RETURN);
  assert.equal(studioHotkeyToKeyCode(null), 0);
  assert.equal(keyCodeToStudioHotkey(0), null);
  assert.equal(keyCodeToStudioHotkey(KEY_NON_FOUND), null);
});

test("ReadHotKeyConfig: tab-separated lines, CRLF read, empty fields collapse (wxTOKEN_STRTOK) so a lone alternate becomes the hotkey", () => {
  const cfg = readHotKeyConfig("a.b.c\tCtrl+X\tShift+Y\r\n\nd.e.f\t\tCtrl+Q\ng.h.i\tF5\t\nj.k.l\t\t\na.b.c\tF2\t\n", false);
  assert.deepEqual(cfg.get("a.b.c"), [WXK.F1 + 1, 0], "a later line of the same name wins");
  assert.deepEqual(cfg.get("d.e.f"), [MD_CTRL + K("Q"), 0], "KiCad's own round trip moves a lone alternate into the hotkey");
  assert.deepEqual(cfg.get("g.h.i"), [WXK.F1 + 4, 0]);
  assert.deepEqual(cfg.get("j.k.l"), [0, 0]);
  assert.equal(cfg.size, 4);
});

test("WriteHotKeyConfig: the file's keys overlaid with the actions', in std::map (byte) order, unassigned written empty -- a KiCad-written text round-trips", () => {
  // as KiCad writes it (`txtStream << name << "\t" << KeyNameFromKeyCode( .. ) << "\t" << KeyNameFromKeyCode( .. ) << endl`)
  const kicad = [
    "common.Control.zoomFitScreen\tHome\t",
    "common.Control.zoomIn\tF1\t",
    "common.Interactive.redo\tCtrl+Y\tCtrl+Shift+Z",
    "eeschema.InteractiveEdit.repeatDrawItem\tIns\t",
    "pcbnew.InteractiveRouter.SingleTrack\tX\t",
    "pcbnew.InteractiveSelection.unrouteSegment\tBack\t",
    "pcbnew.Control.layerTop\tNum Pad 7\tPgUp",
    "pcbnew.EditorControl.lineModeNext\tShift+Space\t",
    "",
  ].join("\n");
  const cfg = readHotKeyConfig(kicad, false);
  const lines = kicad.split("\n").filter(Boolean);
  const sorted = [...lines].sort((a, b) => (a.split("\t")[0]! < b.split("\t")[0]! ? -1 : 1));
  assert.equal(writeHotKeyConfig(cfg, [], false), sorted.map((l) => l + "\n").join(""), "read then written: the same lines, in name order");
  const out = writeHotKeyConfig(cfg, [{ name: "pcbnew.InteractiveRouter.SingleTrack", key: MD_SHIFT + K("X"), keyAlt: WXK.F1 + 7 }, { name: "a.new.action", key: 0, keyAlt: 0 }], false);
  assert.ok(out.includes("pcbnew.InteractiveRouter.SingleTrack\tShift+X\tF8\n"));
  assert.ok(out.startsWith("a.new.action\t\t\n"), "unassigned keys are empty fields; names sort byte-wise");
  // a Mac's file: Cmd / Option, read back on the Mac as the same keys
  const mac = writeHotKeyConfig(new Map([["x.y.z", [MD_CTRL + MD_ALT + K("Z"), 0] as const]]), [], true);
  assert.equal(mac, "x.y.z\tCmd+Option+Z\t\n");
  assert.deepEqual(readHotKeyConfig(mac, true).get("x.y.z"), [MD_CTRL + MD_ALT + K("Z"), 0]);
});

const sample = (): HotkeyAction[] => [
  action("pcbnew.InteractiveRouter.SingleTrack", K("X")),
  action("pcbnew.InteractiveRouter.PlaceVia", K("V"), { scope: "AS_CONTEXT" }),
  action("pcbnew.InteractiveDrawing.closeOutline", 0, { scope: "AS_CONTEXT" }),
  action("pcbnew.InteractiveRouter.SwitchPosture", K("/"), { scope: "AS_CONTEXT" }),
  action("common.Control.zoomIn", WXK.F1),
  action("common.Interactive.redo", MD_CTRL + K("Y"), { keyAlt: MD_CTRL + MD_SHIFT + K("Z"), defaultKeyAlt: MD_CTRL + MD_SHIFT + K("Z") }),
  action("eeschema.InteractiveDrawing.drawWire", K("W")),
  action("pcbnew.Control.internal", K("Q"), { label: "" }),
  action("pcbnew.Control.zoneDisplayOutlines", 0),
];

test("HOTKEY_STORE::Init: sections by application in name order, internal actions left out, the read-only commands and the gestures", () => {
  const s = initHotkeySections(sample(), false, { excluded: new Set(["pcbnew.Control.zoneDisplayOutlines"]) });
  assert.deepEqual(s.map((x) => x.name), ["Common", "Schematic Editor", "PCB Editor"]);
  assert.deepEqual(s[2]!.hotkeys.map((h) => h.action.name), ["pcbnew.InteractiveDrawing.closeOutline", "pcbnew.InteractiveRouter.PlaceVia", "pcbnew.InteractiveRouter.SingleTrack", "pcbnew.InteractiveRouter.SwitchPosture"]);
  const ro = initHotkeySections(sample(), true);
  assert.deepEqual(ro.map((x) => x.name), ["Common", "Schematic Editor", "PCB Editor", "Gestures"]);
  assert.deepEqual(ro[0]!.hotkeys.slice(0, 2).map((h) => h.action.label), ["Close", "Quit"]);
  assert.equal(keyName(ro[0]!.hotkeys[1]!.editKey, false), "Ctrl+Q");
  assert.equal(ro[3]!.hotkeys[0]!.action.label, "Accept Autocomplete");
  assert.equal(keyName(ro[3]!.hotkeys.find((h) => h.action.label === "Highlight Net")!.editKey, false), "Ctrl+Click");
  assert.deepEqual(initHotkeySections(sample(), true, { windows: true })[0]!.hotkeys[0]!.action.label, "Quit", "no Close on Windows");
  assert.equal(toolName({ name: "pcbnew.InteractiveRouter.PlaceVia" }), "pcbnew.InteractiveRouter");
});

const hk = (s: HotkeySection[], name: string) => s.flatMap((x) => x.hotkeys).find((h) => h.action.name === name)!;

test("CheckKeyConflicts: the action's section and Common; contextual actions of different tools may share a key", () => {
  const s = initHotkeySections(sample(), true);
  const track = hk(s, "pcbnew.InteractiveRouter.SingleTrack");
  // the same section
  assert.equal(checkKeyConflicts(s, track.action, K("/"))?.action.name, "pcbnew.InteractiveRouter.SwitchPosture", "a global action against a contextual one");
  // Common's key from an editor conflicts; an editor's key given to a Common action does not -- the loop looks at the action's own section and Common, which
  // for a Common action is Common alone (KiCad's comment says "and vice versa", its code does not)
  assert.equal(checkKeyConflicts(s, track.action, WXK.F1)?.action.name, "common.Control.zoomIn");
  assert.equal(checkKeyConflicts(s, hk(s, "common.Control.zoomIn").action, K("X")), null);
  assert.equal(checkKeyConflicts(s, hk(s, "common.Control.zoomIn").action, MD_CTRL + K("Y"))?.action.name, "common.Interactive.redo");
  // an alternate conflicts too
  assert.equal(checkKeyConflicts(s, track.action, MD_CTRL + MD_SHIFT + K("Z"))?.action.name, "common.Interactive.redo");
  // another editor's key is free
  assert.equal(checkKeyConflicts(s, track.action, K("W")), null);
  // AS_CONTEXT of different tools: no conflict; of the same tool: a conflict
  assert.equal(checkKeyConflicts(s, hk(s, "pcbnew.InteractiveDrawing.closeOutline").action, K("V")), null, "InteractiveDrawing vs InteractiveRouter");
  assert.equal(checkKeyConflicts(s, hk(s, "pcbnew.InteractiveRouter.PlaceVia").action, K("/"))?.action.name, "pcbnew.InteractiveRouter.SwitchPosture");
  // Quit is a row of Common: Ctrl+Q asks
  assert.equal(checkKeyConflicts(s, track.action, MD_CTRL + K("Q"))?.action.label, "Quit");
  const msg = conflictMessage(WXK.F1, hk(s, "common.Control.zoomIn"), false);
  assert.equal(msg, "'F1' is already assigned to 'zoomIn' in section 'Common'. Are you sure you want to change its assignment?");
});

test("changeHotkey and resolveKeyConflicts: KiCad's quirks kept", () => {
  let s = initHotkeySections(sample(), false);
  const track = hk(s, "pcbnew.InteractiveRouter.SingleTrack");
  assert.equal(planHotkeyChange(s, track, K("X")).kind, "none", "the hotkey it has");
  assert.equal(planHotkeyChange(s, track, 999).kind, "none", "a key without a name");
  assert.equal(planHotkeyChange(s, track, 0).kind, "apply", "clearing needs no question");
  assert.equal(planHotkeyChange(s, track, K("K")).kind, "apply");
  const ask = planHotkeyChange(s, track, MD_CTRL + MD_SHIFT + K("Z"));
  assert.ok(ask.kind === "ask" && ask.conflict.action.name === "common.Interactive.redo");
  // Yes: the key is the track's, and redo loses its *hotkey* (Ctrl+Y), keeping the alternate it had the key on
  s = applyHotkeyChange(s, track, MD_CTRL + MD_SHIFT + K("Z"), false, ask.kind === "ask" ? ask.conflict : undefined);
  assert.equal(hk(s, "pcbnew.InteractiveRouter.SingleTrack").editKey, MD_CTRL + MD_SHIFT + K("Z"));
  assert.equal(hk(s, "common.Interactive.redo").editKey, 0);
  assert.equal(hk(s, "common.Interactive.redo").editKeyAlt, MD_CTRL + MD_SHIFT + K("Z"));
  // the alternate is compared with the hotkey: making the alternate what the hotkey is does nothing
  assert.equal(planHotkeyChange(s, hk(s, "pcbnew.InteractiveRouter.SingleTrack"), MD_CTRL + MD_SHIFT + K("Z")).kind, "none");
  // Clear Assigned Alternate on a row with no hotkey does nothing (`m_EditKeycode != aKey` with both 0)
  const redo = hk(s, "common.Interactive.redo");
  assert.deepEqual(resetSteps(redo, "clear_alt"), [{ key: 0, alternate: true }]);
  assert.equal(planHotkeyChange(s, redo, 0).kind, "none");
});

test("resetItem's steps, ResetAllHotkeys, ImportHotKeys and SaveAllHotkeys", () => {
  const acts = sample();
  acts[0] = { ...acts[0]!, key: MD_SHIFT + K("X") }; // the person's: Shift+X over the default X
  let s = initHotkeySections(acts, true);
  const track = hk(s, "pcbnew.InteractiveRouter.SingleTrack");
  assert.deepEqual(resetSteps(track, "reset"), [{ key: MD_SHIFT + K("X"), alternate: false }, { key: MD_SHIFT + K("X"), alternate: true }], "Undo Changes passes the hotkey for the alternate too, as KiCad does");
  assert.deepEqual(resetSteps(track, "default"), [{ key: K("X"), alternate: false }, { key: 0, alternate: true }]);
  s = applyHotkeyChange(s, track, K("K"), false);
  assert.equal(hotkeyRow(hk(s, "pcbnew.InteractiveRouter.SingleTrack"), false).label, "SingleTrack *", "a changed hotkey is marked");
  assert.equal(hk(resetAllHotkeys(s, false), "pcbnew.InteractiveRouter.SingleTrack").editKey, MD_SHIFT + K("X"), "Undo All Changes: the saved keys");
  assert.equal(hk(resetAllHotkeys(s, true), "pcbnew.InteractiveRouter.SingleTrack").editKey, K("X"), "Reset to Defaults: KiCad's");
  const imported = importHotkeys(s, readHotKeyConfig("pcbnew.InteractiveRouter.SingleTrack\tF9\tCtrl+F9\nno.such.action\tA\t\n", false));
  assert.equal(hk(imported, "pcbnew.InteractiveRouter.SingleTrack").editKey, WXK.F1 + 8);
  assert.equal(hk(imported, "pcbnew.InteractiveRouter.SingleTrack").editKeyAlt, MD_CTRL + WXK.F1 + 8);
  const saved = savedKeys(imported);
  assert.ok(!saved.some((k) => k.name === ""), "the pseudo actions are not written");
  assert.deepEqual(saved.find((k) => k.name === "pcbnew.InteractiveRouter.SingleTrack"), { name: "pcbnew.InteractiveRouter.SingleTrack", key: WXK.F1 + 8, keyAlt: MD_CTRL + WXK.F1 + 8 });
  assert.ok(findHotkey(imported, track.action));
});

test("HOTKEY_FILTER and the row's text", () => {
  const s = initHotkeySections([action("pcbnew.Control.showLayersManager", K("L"), { label: "Show Appearance Manager...", keyAlt: MD_ALT + K("Q"), description: "Show/hide\nthe manager" })], false);
  const h = s[0]!.hotkeys[0]!;
  assert.ok(filterMatches(h, "appear", false));
  assert.ok(filterMatches(h, "l", false));
  assert.ok(!filterMatches(h, "alt+q", false), "the alternate is not searched");
  assert.ok(filterMatches(h, "", false));
  assert.deepEqual(hotkeyRow(h, false), { label: "Show Appearance Manager", key: "L", alt: "Alt+Q", description: "Show/hide the manager" });
  assert.equal(hotkeyRow(h, true).alt, "Option+Q");
});

const press = (p: Partial<KeyPress>): KeyPress => ({ key: "a", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...p });

test("MapKeypressToKeycode: Shift kept for letters, special keys, Tab and Space only; Esc is 0; modifiers alone press nothing", () => {
  assert.equal(mapKeypressToKeycode(press({ key: "a" }), false), K("A"));
  assert.equal(mapKeypressToKeycode(press({ key: "A", shiftKey: true }), false), MD_SHIFT + K("A"));
  assert.equal(mapKeypressToKeycode(press({ key: "?", code: "Slash", shiftKey: true }), false), K("?"), "Shift+/ is ?, without Shift");
  assert.equal(mapKeypressToKeycode(press({ key: "%", code: "Digit5", shiftKey: true, ctrlKey: true }), false), MD_CTRL + K("%"));
  assert.equal(mapKeypressToKeycode(press({ key: "5", code: "Digit5", ctrlKey: true }), false), MD_CTRL + K("5"));
  assert.equal(mapKeypressToKeycode(press({ key: "F5", shiftKey: true }), false), MD_SHIFT + WXK.F1 + 4);
  assert.equal(mapKeypressToKeycode(press({ key: "Tab", shiftKey: true }), false), MD_SHIFT + WXK.TAB);
  assert.equal(mapKeypressToKeycode(press({ key: " ", shiftKey: true }), false), MD_SHIFT + WXK.SPACE);
  assert.equal(mapKeypressToKeycode(press({ key: "Escape" }), false), 0);
  assert.equal(mapKeypressToKeycode(press({ key: "Shift", shiftKey: true }), false), null);
  assert.equal(mapKeypressToKeycode(press({ key: "5", code: "Numpad5" }), false), WXK.NUMPAD0 + 5);
  assert.equal(mapKeypressToKeycode(press({ key: "z", metaKey: true }), true), MD_CTRL + K("Z"), "Cmd is Ctrl on macOS");
  assert.equal(mapKeypressToKeycode(press({ key: "z", ctrlKey: true }), true), K("Z"), "a Mac's Control key is not read");
  assert.equal(mapKeypressToKeycode(press({ key: "Backspace", altKey: true }), false), MD_ALT + WXK.BACK);
});

test("the reserved hotkeys", () => {
  assert.equal(reservedMessage(MD_CTRL + WXK.TAB), "'Ctrl+Tab' is a reserved hotkey in KiCad and cannot be assigned.");
  assert.equal(reservedMessage(MD_CTRL + MD_SHIFT + WXK.TAB), "'Ctrl+Shift+Tab' is a reserved hotkey in KiCad and cannot be assigned.");
  assert.equal(reservedMessage(MD_CTRL + K("T")), null);
});

test("C3-4: a click on a column title sorts each section's rows by it (wxString::Cmp, ties in order), a second the other way; the sections by name in Command only", () => {
  const acts = [
    action("pcbnew.A.zeta", K("B"), { label: "zeta" }),
    action("pcbnew.A.alpha", 0, { label: "alpha" }),
    action("pcbnew.A.Beta", K("A"), { label: "Beta" }),
    action("common.A.gamma", K("C"), { label: "gamma" }),
    action("3DViewer.Control.viewTop", K("Z"), { label: "View Top" }),
  ];
  const sections = initHotkeySections(acts, false);
  assert.deepEqual(sections.map((s) => s.name), ["3D Viewer", "Common", "PCB Editor"], "the 3D viewer's actions first, in the names' byte order");
  const pcb = (ss: HotkeySection[]) => ss.find((s) => s.name === "PCB Editor")!.hotkeys.map((h) => h.action.label);
  assert.deepEqual(pcb(sortHotkeySections(sections, null, false)), ["Beta", "alpha", "zeta"], "unsorted: the names' byte order");
  const byCmd = nextHotkeySort(null, 0);
  assert.deepEqual(byCmd, { column: 0, ascending: true });
  assert.deepEqual(pcb(sortHotkeySections(sections, byCmd, false)), ["Beta", "alpha", "zeta"], "upper case before lower case (code points)");
  const back = nextHotkeySort(byCmd, 0);
  assert.deepEqual(back, { column: 0, ascending: false });
  assert.deepEqual(sortHotkeySections(sections, back, false).map((s) => s.name), ["PCB Editor", "Common", "3D Viewer"]);
  // by Hotkey: "" (alpha, no key), "A", "B"; the sections have no text there, so stay as they are
  const byKey = nextHotkeySort(back, 1);
  assert.deepEqual(byKey, { column: 1, ascending: true });
  const sorted = sortHotkeySections(sections, byKey, false);
  assert.deepEqual(pcb(sorted), ["alpha", "Beta", "zeta"]);
  assert.deepEqual(sorted.map((s) => s.name), ["3D Viewer", "Common", "PCB Editor"]);
  // descending turns the ties round too (`return ascending ? id1 - id2 : id2 - id1`)
  assert.deepEqual(sortHotkeySections(sections, { column: 2, ascending: false }, false).map((s) => s.name), ["PCB Editor", "Common", "3D Viewer"]);
});
