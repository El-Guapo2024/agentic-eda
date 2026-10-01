// Regression tests for actionsParser.js's handling of KiCad's
// `#if defined( __WXMAC__ ) ... #else ... #endif` hotkey overrides and
// literal-char hotkeys -- see extractPlatformRaw's own comment for why
// the naive regex silently spliced two platforms' tokens into one wrong
// hotkey (zoomIn/zoomOut/zoomFitScreen/zoomRedraw/redo/delete) before
// this fix, and normalizeHotkey's for why a bare `' '` char constant
// didn't canonicalize to "Space" (common.Control.resetLocalCoords, the
// status-bar relative-origin hotkey).
//
// Plain Node, no TypeScript involved -- run directly:
//   node --test tools/lib/actionsParser.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseActionsFromFile, normalizeHotkey, buildSymbolIndex } from "./actionsParser.js";

test("normalizeHotkey: named keys, modifiers, and ordering", () => {
  assert.equal(normalizeHotkey("WXK_F1"), "F1");
  assert.equal(normalizeHotkey("WXK_DELETE"), "Del");
  assert.equal(normalizeHotkey("WXK_BACK"), "Backspace");
  assert.equal(normalizeHotkey("MD_CTRL + 'Z'"), "Ctrl+Z");
  assert.equal(normalizeHotkey("MD_CTRL + MD_SHIFT + 'Z'"), "Ctrl+Shift+Z");
  assert.equal(normalizeHotkey("0"), null);
  assert.equal(normalizeHotkey("WXK_NONE"), null);
  assert.equal(normalizeHotkey(null), null);
});

test("normalizeHotkey: a literal space char canonicalizes to Space, not a literal ' '", () => {
  assert.equal(normalizeHotkey("' '"), "Space");
  assert.equal(normalizeHotkey("MD_CTRL + ' '"), "Ctrl+Space");
  assert.equal(normalizeHotkey("MD_SHIFT + ' '"), "Shift+Space");
});

/** A minimal stand-in for common/tool/actions.cpp's shape, just enough to exercise the parser. */
function wrapAction(name, body) {
  return `TOOL_ACTION ACTIONS::${name}( TOOL_ACTION_ARGS()\n        .Name( "common.Control.${name}" )\n${body}\n        .FriendlyName( _( "${name}" ) ) );\n\n`;
}

test("parseActionsFromFile: a plain (non-ifdef) DefaultHotkey still works", () => {
  const src = wrapAction("zoomTool", `        .DefaultHotkey( MD_CTRL + static_cast<int>( WXK_F5 ) )`);
  const [a] = parseActionsFromFile(src);
  assert.equal(a.hotkey, "Ctrl+F5");
  assert.equal(a.macHotkey, null);
});

test("parseActionsFromFile: #if __WXMAC__ / #else DefaultHotkey splits into hotkey (non-mac) + macHotkey, not a spliced hybrid", () => {
  const src = wrapAction(
    "zoomIn",
    [`#if defined( __WXMAC__ )`, `        .DefaultHotkey( MD_CTRL + '+' )`, `#else`, `        .DefaultHotkey( WXK_F1 )`, `#endif`].join("\n")
  );
  const [a] = parseActionsFromFile(src);
  // The historical bug produced "Ctrl+F1" here -- the mac branch's Ctrl
  // glued to the non-mac branch's F1, wrong on both platforms.
  assert.equal(a.hotkey, "F1");
  assert.equal(a.macHotkey, "Ctrl++");
});

test("parseActionsFromFile: #if __WXMAC__ with a named key on both sides (redo, delete)", () => {
  const redoSrc = wrapAction(
    "redo",
    [`#if defined( __WXMAC__ )`, `        .DefaultHotkey( MD_CTRL + MD_SHIFT + 'Z' )`, `#else`, `        .DefaultHotkey( MD_CTRL + 'Y' )`, `#endif`].join("\n")
  );
  const [redo] = parseActionsFromFile(redoSrc);
  assert.equal(redo.hotkey, "Ctrl+Y");
  assert.equal(redo.macHotkey, "Ctrl+Shift+Z");

  const deleteSrc = wrapAction("doDelete", [`#if defined( __WXMAC__ )`, `        .DefaultHotkey( WXK_BACK )`, `#else`, `        .DefaultHotkey( WXK_DELETE )`, `#endif`].join("\n"));
  const [del] = parseActionsFromFile(deleteSrc);
  assert.equal(del.hotkey, "Del");
  assert.equal(del.macHotkey, "Backspace");
});

test("parseActionsFromFile: resetLocalCoords's bare space-char hotkey", () => {
  const src = wrapAction("resetLocalCoords", `        .DefaultHotkey( ' ' )`);
  const [a] = parseActionsFromFile(src);
  assert.equal(a.hotkey, "Space");
});

test("buildSymbolIndex resolves by qualified symbol and bare func name", () => {
  const src = wrapAction("zoomIn", `        .DefaultHotkey( WXK_F1 )`);
  const actions = parseActionsFromFile(src);
  const index = buildSymbolIndex(actions);
  assert.equal(index.resolve("ACTIONS::zoomIn"), "common.Control.zoomIn");
  assert.equal(index.resolve("zoomIn"), "common.Control.zoomIn");
  assert.equal(index.resolve("nonsense"), null);
});
