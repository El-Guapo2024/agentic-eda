// Preferences > PCB Editor > Display Options' net names and clearance outlines (UI-LEFT-3 P3-1, P3-6): `PCB_PAINTER::renderNetNameForSegment`, the netname
// branch of `draw( const PCB_VIA* )`, `PCB_TRACK::ViewGetLOD`, `PNS_KICAD_IFACE::DisplayItem`.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  boardShowsTrackClearance,
  DEFAULT_PCB_DISPLAY,
  GAL_PX_PER_NM_AT_ZOOM_1,
  lodShown,
  netNamesOnPads,
  netNamesOnTracks,
  netNamesOnVias,
  printableCharCount,
  routerShowsClearance,
  sanitizeNetNamesMode,
  sanitizeTrackClearanceMode,
  shortNetname,
  TRACK_CLEARANCE_CHOICES,
  trackNetNameLabels,
  viaNetNameLayout,
  outlinePolys,
  customPadJobs,
  previewClearancesOf,
} from "./pcbDisplayOptions";
import { DEFAULT_PREFERENCES, sanitizePreferences } from "./preferences";

const MM = 1_000_000;
/** A zoom at which a 0.25 mm track is wide enough for its name (4 mm / 0.25 mm = 16 < zoom). */
const ZOOMED = 20 * GAL_PX_PER_NM_AT_ZOOM_1;
const VIEW = { x0: 0, y0: 0, x1: 20 * MM, y1: 20 * MM };

test("the defaults are a fresh PCBNEW_SETTINGS': names on pads & tracks, track clearance while routing with vias, pad clearance on", () => {
  assert.deepEqual(DEFAULT_PCB_DISPLAY, { netNames: 3, trackClearance: "routing_via", padClearance: true });
  assert.equal(DEFAULT_PREFERENCES.pcbNetNames, 3);
  assert.equal(DEFAULT_PREFERENCES.pcbTrackClearance, "routing_via");
  assert.equal(DEFAULT_PREFERENCES.pcbPadClearance, true);
  // the stored settings are taken back, nonsense is the default (`PARAM<int>( .., 3, 0, 3 )`)
  const p = sanitizePreferences({ pcbNetNames: 1, pcbTrackClearance: "always", pcbPadClearance: false });
  assert.deepEqual([p.pcbNetNames, p.pcbTrackClearance, p.pcbPadClearance], [1, "always", false]);
  assert.equal(sanitizeNetNamesMode(7), 3);
  assert.equal(sanitizeTrackClearanceMode("sometimes"), "routing_via");
  assert.deepEqual(TRACK_CLEARANCE_CHOICES.map((c) => c.label), ["Do not show clearances", "Show when routing", "Show when routing w/ via clearance at end", "Show when routing and editing", "Show always"]);
});

test("Net names: pads with 1 and 3, tracks with 2 and 3, vias with anything but 0", () => {
  assert.deepEqual([0, 1, 2, 3].map((m) => netNamesOnPads(m as 0 | 1 | 2 | 3)), [false, true, false, true]);
  assert.deepEqual([0, 1, 2, 3].map((m) => netNamesOnTracks(m as 0 | 1 | 2 | 3)), [false, false, true, true]);
  assert.deepEqual([0, 1, 2, 3].map((m) => netNamesOnVias(m as 0 | 1 | 2 | 3)), [false, true, true, true]);
});

test("the board's tracks and vias outline their clearance with Show always only; the router's preview by DisplayItem's switch", () => {
  assert.deepEqual(TRACK_CLEARANCE_CHOICES.map((c) => boardShowsTrackClearance(c.mode)), [false, false, false, false, true]);
  // [track routing, via routing, track dragging, via dragging]
  const row = (m: Parameters<typeof routerShowsClearance>[0]) => [routerShowsClearance(m, "track", false), routerShowsClearance(m, "via", false), routerShowsClearance(m, "track", true), routerShowsClearance(m, "via", true)];
  assert.deepEqual(row("none"), [false, false, false, false]);
  assert.deepEqual(row("routing"), [true, false, false, false]);
  assert.deepEqual(row("routing_via"), [true, true, false, false]);
  assert.deepEqual(row("routing_editing"), [true, true, true, true]);
  assert.deepEqual(row("always"), [true, true, true, true]);
});

test("lodScaleForThreshold: shown once the threshold over the size is below the zoom", () => {
  // a 0.25 mm track's name needs a zoom over 16
  assert.equal(lodShown(15 * GAL_PX_PER_NM_AT_ZOOM_1, 0.25 * MM, 4 * MM), false);
  assert.equal(lodShown(17 * GAL_PX_PER_NM_AT_ZOOM_1, 0.25 * MM, 4 * MM), true);
  assert.equal(lodShown(ZOOMED, 0, 4 * MM), false);
});

test("a horizontal track's name: centred on it, unturned, 0.55 of its width, a twelfth of it the pen", () => {
  const labels = trackNetNameLabels([2 * MM, 5 * MM], [8 * MM, 5 * MM], 0.25 * MM, "GND", VIEW, ZOOMED);
  assert.equal(labels.length, 1);
  const [l] = labels;
  assert.deepEqual([l!.x, l!.y, l!.angleRad], [5 * MM, 5 * MM, 0]);
  assert.equal(l!.sizeNm, 0.25 * MM * 0.55);
  assert.equal(l!.penNm, 0.25 * MM / 12);
});

test("a vertical track reads bottom to top, a diagonal one along itself within -90 .. 90 degrees", () => {
  const [v] = trackNetNameLabels([5 * MM, 2 * MM], [5 * MM, 8 * MM], 0.25 * MM, "GND", VIEW, ZOOMED);
  assert.equal(v!.angleRad, -Math.PI / 2);
  // down and to the right on a y-down board: KiCad's -45 degrees, the canvas turns clockwise
  const [d] = trackNetNameLabels([2 * MM, 2 * MM], [8 * MM, 8 * MM], 0.25 * MM, "GND", VIEW, ZOOMED);
  assert.ok(Math.abs(d!.angleRad - Math.PI / 4) < 1e-9);
  // drawn from its other end, the same: never upside down
  const [r] = trackNetNameLabels([8 * MM, 8 * MM], [2 * MM, 2 * MM], 0.25 * MM, "GND", VIEW, ZOOMED);
  assert.ok(Math.abs(r!.angleRad - Math.PI / 4) < 1e-9);
});

test("no name on a track shorter than its width times the name's length, out of view, or under the zoom threshold", () => {
  // 10 characters at 0.5 mm need 5 mm of track
  assert.equal(trackNetNameLabels([0, 5 * MM], [4.9 * MM, 5 * MM], 0.5 * MM, "ABCDEFGHIJ", VIEW, ZOOMED).length, 0);
  assert.equal(trackNetNameLabels([0, 5 * MM], [5.1 * MM, 5 * MM], 0.5 * MM, "ABCDEFGHIJ", VIEW, ZOOMED).length, 1);
  assert.equal(trackNetNameLabels([30 * MM, 30 * MM], [40 * MM, 30 * MM], 0.25 * MM, "GND", VIEW, ZOOMED).length, 0);
  assert.equal(trackNetNameLabels([2 * MM, 5 * MM], [8 * MM, 5 * MM], 0.25 * MM, "GND", VIEW, 10 * GAL_PX_PER_NM_AT_ZOOM_1).length, 0);
});

test("a track longer than the view is named once per view's width, the names in view only", () => {
  // 60 mm across a 20 mm view: 3 names at 15, 30 and 45 mm, only the one at 15 mm in view
  const labels = trackNetNameLabels([0, 5 * MM], [60 * MM, 5 * MM], 0.25 * MM, "GND", VIEW, ZOOMED);
  assert.deepEqual(labels.map((l) => l.x), [15 * MM]);
  const wide = trackNetNameLabels([0, 5 * MM], [60 * MM, 5 * MM], 0.25 * MM, "GND", { x0: 0, y0: 0, x1: 60 * MM, y1: 20 * MM }, ZOOMED);
  assert.deepEqual(wide.map((l) => l.x), [30 * MM]);
});

test("a via's net name: 1.5 x width over at least 3 characters, at most the width, times 0.75; a blind via adds its layers a line above", () => {
  const through = viaNetNameLayout(0.6 * MM, "GND", true, null)!;
  // 1.5 * 0.6 / 3 = 0.3 mm, times 0.75
  assert.ok(Math.abs(through.sizeNm - 0.225 * MM) < 1);
  assert.deepEqual(through.lines, [{ text: "GND", dy: 0 }]);
  assert.ok(Math.abs(through.penNm - through.sizeNm / 10) < 1e-9);
  const blind = viaNetNameLayout(0.6 * MM, "GND", true, [1, 2])!;
  // 6 characters at least: 1.5 * 0.6 / 6 * 0.75
  const t = ((1.5 * 0.6 * MM) / 6) * 0.75;
  assert.ok(Math.abs(blind.sizeNm - t) < 1);
  assert.deepEqual(blind.lines.map((l) => l.text), ["GND", "1-2"]);
  assert.ok(Math.abs(blind.lines[0]!.dy - (t * 1.3) / 2) < 1 && Math.abs(blind.lines[1]!.dy + (t * 1.3) / 2) < 1, "the name below the middle, the layers above");
  assert.equal(viaNetNameLayout(0.6 * MM, "GND", false, null), null, "nothing to write");
  assert.deepEqual(viaNetNameLayout(0.6 * MM, "", true, [1, 4])!.lines.map((l) => l.text), ["1-4"]);
  // the size is held at MAX_FONT_SIZE (10 mm)
  assert.ok(viaNetNameLayout(40 * MM, "X", true, null)!.sizeNm <= 10 * MM * 0.75);
});

test("PrintableCharCount leaves the markup out; a hierarchical net's short name is after its last slash", () => {
  assert.equal(printableCharCount("~{RESET}"), 5);
  assert.equal(printableCharCount("V_{CC}"), 3);
  assert.equal(printableCharCount("A\tB"), 2);
  assert.equal(shortNetname("/sheet1/SDA"), "SDA");
  assert.equal(shortNetname("GND"), "GND");
});

test("a custom pad's outline is grouped into polygons with their holes, and only a custom pad with a clearance is grown", () => {
  const sq = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
  const polys = outlinePolys([sq(0, 0, 10, 10), sq(20, 0, 30, 10), sq(2, 2, 4, 4)]);
  assert.equal(polys.length, 2);
  assert.deepEqual(polys[0]!.holes, [sq(2, 2, 4, 4)], "the ring's hole goes with the outline holding it");
  assert.deepEqual(polys[1]!.holes, []);
  const pad = (num: string, shape: string, outline: [number, number][][] | null) => ({ num, shape, outline, x: 0, y: 0, w: 1, h: 1, round: false, net: "A", th: false });
  const board = { parts: [{ ref: "U1", placed: true, pads: [pad("1", "custom", [sq(0, 0, 10, 10)]), pad("2", "rect", null), pad("3", "custom", [sq(0, 0, 5, 5)])] }] } as never;
  const jobs = customPadJobs(board, { ok: true, pads: { "U1.1": 200_000, "U1.2": 200_000, "U1.3": 0 } });
  assert.deepEqual(jobs.map((j) => [j.key, j.inflate]), [["U1.1", 200_000]], "a rectangle is grown by the page, a zero clearance not at all");
});

test("a preview reply's clearances: the drag's line, via and fanout, the shoved items; none from an older server", () => {
  assert.deepEqual(previewClearancesOf({ clearance: 1, via_clearance: null, fanout_clearances: [2], displaced_clearances: [3, 4], displaced_via_clearances: [5] }), { line: 1, via: null, fanout: [2], displaced: [3, 4], displacedVias: [5] });
  assert.deepEqual(previewClearancesOf({}), { line: null, via: null, fanout: [], displaced: [], displacedVias: [] });
});
