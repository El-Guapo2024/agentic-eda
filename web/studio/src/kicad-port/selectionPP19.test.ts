// UI-LEFT-2 PP19: Select Connection along copper graphic shapes (`selectAllConnectedTracks`' `shapeMap`), Highlight Net under the cursor through the
// selection filter (`BOARD_INSPECTION_TOOL::highlightNet`), the flash of the filter's rows (`PANEL_SELECTION_FILTER::OnFlashEvent`), and the brightened
// colour of the clarification menu's item (`PCB_RENDER_SETTINGS::GetColor`). Worked from KiCad 8303b2ad.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Part, Shape } from "../api/types";
import { selectAllConnectedTracks, type ConnSeg, type ConnShape } from "./expandConnection";
import { planSelectConnection, shapeConnectionPoints } from "./pcbConnection";
import { findNetAtCursor } from "../components/canvas/netAtCursor";
import { DEFAULT_SELECTION_FILTER } from "../components/canvas/selectionCandidates";
import { FLASH_STEPS, flashOverlay, flashSelectionFilter, flashTick, getFilterFlash } from "./filterFlash";
import { brightenedBoard, brightenedCss } from "./brightened";

const board = (o: Partial<BoardState>): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...o }) as BoardState;
const routing = (tracks: unknown[], zones: unknown[] = []) => ({ tracks, vias: [], zones, track_width_presets: [], via_presets: [], teardrop_settings: {} }) as never;
const drawings = (shapes: unknown[]) => ({ shapes, texts: [], dimensions: [], dimension_settings: {}, groups: [] }) as never;
const seg = (track: string, start: [number, number], end: [number, number], layer = "F.Cu"): ConnSeg => ({ track, seg: 0, layer, start, end });

test("GetConnectionPoints: a line's ends, an arc's middle and ends, a rectangle's corners, a circle's quadrants, a filled shape's centre", () => {
  const base = { id: "S", layer: "F.Cu", stroke_width: 200, filled: false } as const;
  assert.deepEqual(shapeConnectionPoints({ ...base, kind: "segment", start: [0, 0], end: [100, 0] } as Shape), [[0, 0], [100, 0]]);
  assert.deepEqual(shapeConnectionPoints({ ...base, kind: "arc", start: [0, 0], mid: [50, 50], end: [100, 0] } as Shape), [[50, 50], [0, 0], [100, 0]]);
  assert.deepEqual(shapeConnectionPoints({ ...base, kind: "circle", center: [0, 0], end: [100, 0] } as Shape), [[0, 100], [100, 0], [0, -100], [-100, 0]]);
  assert.deepEqual(shapeConnectionPoints({ ...base, kind: "rect", start: [0, 0], end: [100, 50] } as Shape), [[0, 0], [100, 0], [100, 50], [0, 50]]);
  assert.deepEqual(shapeConnectionPoints({ ...base, kind: "rect", start: [0, 0], end: [100, 50], filled: true } as Shape), [[50, 25]]);
});

test("selectAllConnectedTracks follows a copper line from a track's end to the next track (shapeMap), and the filter stops it", () => {
  const segs = [seg("t1", [0, 0], [100, 0]), seg("t2", [300, 0], [400, 0])];
  const shapes: ConnShape[] = [{ id: "s1", layer: "F.Cu", points: [[100, 0], [300, 0]] }];
  const run = (passes?: (k: string, id: string) => boolean, layer = "F.Cu") => {
    const selected = { trackIds: new Set<string>(), viaIds: new Set<string>(), shapeIds: new Set<string>() };
    selectAllConnectedTracks({ segs, vias: [], pads: [], shapes: shapes.map((s) => ({ ...s, layer })) }, [{ kind: "track", id: "t1" }], "never", selected, passes);
    return { tracks: [...selected.trackIds].sort(), shapes: [...selected.shapeIds] };
  };
  assert.deepEqual(run(), { tracks: ["t1", "t2"], shapes: ["s1"] });
  assert.deepEqual(run((k) => k !== "shape"), { tracks: ["t1"], shapes: [] }, "a shape the filter refuses is neither selected nor walked");
  assert.deepEqual(run(undefined, "B.Cu"), { tracks: ["t1"], shapes: [] }, "on another layer it is not reached");
  // a shape as the start: all its points are active
  const selected = { trackIds: new Set<string>(), viaIds: new Set<string>(), shapeIds: new Set<string>() };
  selectAllConnectedTracks({ segs, vias: [], pads: [], shapes }, [{ kind: "shape", id: "s1" }], "never", selected);
  assert.deepEqual([...selected.trackIds].sort(), ["t1", "t2"]);
});

test("Select Connection with a copper line selected walks from it (PCB_SHAPE::IsConnected), and from a track reaches it", () => {
  const b = board({
    routing: routing([
      { id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [100, 0]] },
      { id: "t2", net: "N", layer: "F.Cu", width: 200, pts: [[300, 0], [400, 0]] },
    ]),
    drawings: drawings([{ id: "s1", kind: "segment", start: [100, 0], end: [300, 0], layer: "F.Cu", stroke_width: 200 }]),
  });
  assert.deepEqual(planSelectConnection(b, new Set(["s1"]), [])!.refs.sort(), ["s1", "t1", "t2"]);
  assert.deepEqual(planSelectConnection(b, new Set(["t1"]), [])!.refs.sort(), ["s1", "t1", "t2"]);
  // with nothing selected the copper line under the cursor is the connected item taken (`connectedItemFilter`)
  assert.deepEqual(planSelectConnection(b, new Set(), [{ kind: "shape", id: "s1", layer: "F.Cu" }])!.refs.sort(), ["s1", "t1", "t2"]);
});

test("highlightNet under the cursor honours the selection filter, and a filtered track still keeps the zones out (they are collected only when nothing else is)", () => {
  const zone = { id: "Z1", net: "GND", layer: "F.Cu", outline: [[-5_000, -5_000], [5_000, -5_000], [5_000, 5_000], [-5_000, 5_000]] };
  const b = board({ routing: routing([{ id: "t1", net: "SIG", layer: "F.Cu", width: 200, pts: [[-3_000, 4_990], [3_000, 4_990]] }], [zone]) });
  // on the zone's edge, where the track runs too
  assert.equal(findNetAtCursor(b, 0, 4_990, 50, {}), "SIG");
  assert.equal(findNetAtCursor(b, 0, 4_990, 50, { filter: { ...DEFAULT_SELECTION_FILTER, tracks: false } }), null, "the track is filtered out, and the zone was never collected");
  // a locked track is still taken ("allow picking locked items for highlighting")
  assert.equal(findNetAtCursor({ ...b, locked: ["t1"] } as BoardState, 0, 4_990, 50, { filter: DEFAULT_SELECTION_FILTER }), "SIG");
  // away from the track, the zone's edge: Zones off leaves nothing
  assert.equal(findNetAtCursor(b, 5_000, 0, 50, {}), "GND");
  assert.equal(findNetAtCursor(b, 5_000, 0, 50, { filter: { ...DEFAULT_SELECTION_FILTER, zones: false } }), null);
});

test("OnFlashEvent: a row flashes for ten steps, a second flash starts it again, the overlay fades", () => {
  flashSelectionFilter(["tracks"], false);
  assert.equal(getFilterFlash().get("tracks"), FLASH_STEPS);
  for (let i = 0; i < 4; i++) flashTick();
  assert.equal(getFilterFlash().get("tracks"), FLASH_STEPS - 4);
  flashSelectionFilter(["tracks", "vias"], false);
  assert.equal(getFilterFlash().get("tracks"), FLASH_STEPS);
  for (let i = 0; i < FLASH_STEPS; i++) flashTick();
  assert.equal(getFilterFlash().size, 0);
  assert.equal(flashOverlay(10), "rgba(53, 132, 228, 0.500)");
  assert.equal(flashOverlay(1), "rgba(53, 132, 228, 0.050)");
});

test("the brightened colour is the layer's brightened by m_selectFactor at 0.8 alpha; a pad is brightened alone", () => {
  assert.equal(brightenedCss("#ff0000"), "rgba(255, 128, 128, 0.8)");
  assert.equal(brightenedCss("#000000"), "rgba(128, 128, 128, 0.8)");
  const part = { ref: "U1", placed: true, at: [0, 0], side: "top", courtyard: [-10, -10, 10, 10], pads: [{ num: "1", x: -5, y: 0, w: 2, h: 2, round: false, th: false, net: null }, { num: "2", x: 5, y: 0, w: 2, h: 2, round: false, th: false, net: null }], graphics: [{ kind: "segment" }], fields: [{ id: "U1:Reference" }] } as unknown as Part;
  const lit = brightenedBoard(board({ parts: [part] }), ["U1.2"]);
  assert.equal(lit.board.parts.length, 1);
  assert.deepEqual(lit.board.parts[0]!.pads!.map((p) => p.num), ["2"]);
  assert.deepEqual(lit.board.parts[0]!.graphics, []);
  assert.deepEqual(lit.board.parts[0]!.fields, []);
  // the footprint itself is brightened whole
  assert.equal(brightenedBoard(board({ parts: [part] }), ["U1"]).board.parts[0]!.pads!.length, 2);
});
