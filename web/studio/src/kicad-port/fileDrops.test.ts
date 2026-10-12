import { test } from "node:test";
import assert from "node:assert/strict";
import { acceptedExtensions, dropActionFor, dropMessage, extensionOf, planDrop } from "./fileDrops";

const all = () => true;

test("the extension is what follows the last dot, in lower case", () => {
  assert.equal(extensionOf("Board.KiCad_PCB"), "kicad_pcb");
  assert.equal(extensionOf("a.b.dxf"), "dxf");
  assert.equal(extensionOf("noext"), "");
});

test("each editor takes the extensions its frame's m_acceptedExts has", () => {
  assert.equal(dropActionFor("pcb", "x.kicad_pcb"), "pcbnew.Control.DdAppendBoard");
  assert.equal(dropActionFor("pcb", "x.brd"), "pcbnew.Control.DdAppendBoard");
  assert.equal(dropActionFor("pcb", "x.DXF"), "pcbnew.InteractiveDrawing.ddImportGraphics");
  assert.equal(dropActionFor("pcb", "x.svg"), "pcbnew.InteractiveDrawing.ddImportGraphics");
  assert.equal(dropActionFor("pcb", "x.kicad_mod"), null);
  assert.equal(dropActionFor("footprint", "x.kicad_mod"), "pcbnew.Control.ddImportFootprint");
  assert.equal(dropActionFor("footprint", "x.dxf"), null, "the Footprint Editor takes only footprints");
  assert.equal(dropActionFor("schematic", "x.dxf"), "eeschema.EditorControl.ddImportGraphics");
  assert.equal(dropActionFor("schematic", "x.kicad_sch"), "eeschema.EditorControl.ddAppendFile");
  assert.equal(dropActionFor("schematic", "x.png"), "eeschema.EditorControl.ddAddImage");
  assert.equal(dropActionFor("symbol", "x.dxf"), null, "the Symbol Editor accepts no file");
  assert.equal(dropActionFor("3d", "x.kicad_pcb"), null);
  assert.deepEqual(acceptedExtensions("footprint"), ["kicad_mod"]);
  assert.deepEqual(acceptedExtensions("symbol"), []);
});

test("the first file the studio can open is opened; the others are skipped, unavailable or ignored as they are", () => {
  const enabled = (a: string) => a !== "eeschema.EditorControl.ddAppendFile" && a !== "eeschema.EditorControl.ddAddImage";
  const plan = planDrop("schematic", ["part.kicad_sch", "logo.svg", "board.dxf", "notes.txt", "pic.png"], enabled);
  assert.deepEqual(plan.open, { name: "logo.svg", action: "eeschema.EditorControl.ddImportGraphics" });
  assert.deepEqual(plan.extra, ["board.dxf"]);
  assert.deepEqual(plan.unavailable.map((u) => u.name), ["part.kicad_sch", "pic.png"]);
  assert.deepEqual(plan.ignored, ["notes.txt"]);
  assert.equal(dropMessage("schematic", plan), "Only logo.svg was opened; board.dxf was skipped (drop one file at a time).");
});

test("a drop nothing of which is opened says why", () => {
  const none = planDrop("symbol", ["a.dxf"], all);
  assert.equal(none.open, null);
  assert.equal(dropMessage("symbol", none), "a.dxf is not a file this editor opens.");
  const text = planDrop("pcb", ["notes.txt", "more.md"], all);
  assert.equal(dropMessage("pcb", text), "notes.txt, more.md are not files this editor opens (it takes .kicad_pcb, .brd, .dxf, .svg).");
  const unavailable = planDrop("schematic", ["x.kicad_sch"], () => false);
  assert.equal(dropMessage("schematic", unavailable), "x.kicad_sch cannot be opened by a drop in the studio.");
  const ok = planDrop("pcb", ["b.kicad_pcb"], all);
  assert.equal(dropMessage("pcb", ok), null, "an opened file needs no word");
});
