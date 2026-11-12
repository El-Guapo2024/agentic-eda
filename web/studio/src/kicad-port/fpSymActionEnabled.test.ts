import { test } from "node:test";
import assert from "node:assert/strict";
import { fpActionEnabled, symActionEnabled } from "./fpSymActionEnabled";

test("fpActionEnabled: haveFootprintCond rows, others unconditioned", () => {
  assert.equal(fpActionEnabled("pcbnew.ModuleEditor.exportFootprint", { footprintLoaded: false }), false);
  assert.equal(fpActionEnabled("pcbnew.ModuleEditor.repairFootprint", { footprintLoaded: true }), true);
  assert.equal(fpActionEnabled("pcbnew.PadTool.enumeratePads", { footprintLoaded: false }), undefined);
  assert.equal(fpActionEnabled("pcbnew.ModuleEditor.copyFootprint", { footprintLoaded: false }), undefined);
});

test("symActionEnabled: drawing tools need an editable root symbol, Synchronized Pins a multi-unit symbol with interchangeable units", () => {
  const base = { symbolLoaded: true, derived: false, unitCount: 2, unitsLocked: false };
  assert.equal(symActionEnabled("eeschema.SymbolDrawing.drawSymbolLines", base), true);
  assert.equal(symActionEnabled("eeschema.SymbolDrawing.placeSymbolAnchor", { ...base, derived: true }), false);
  assert.equal(symActionEnabled("eeschema.SymbolDrawing.drawSymbolPolygon", { ...base, symbolLoaded: false }), false);
  assert.equal(symActionEnabled("eeschema.SymbolLibraryControl.toggleSyncedPinsMode", base), true);
  assert.equal(symActionEnabled("eeschema.SymbolLibraryControl.toggleSyncedPinsMode", { ...base, unitsLocked: true }), false);
  assert.equal(symActionEnabled("eeschema.SymbolLibraryControl.toggleSyncedPinsMode", { ...base, unitCount: 1 }), false);
  assert.equal(symActionEnabled("eeschema.SymbolLibraryControl.cutSymbol", base), undefined);
});
