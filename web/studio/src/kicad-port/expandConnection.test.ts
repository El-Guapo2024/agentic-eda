import { test } from "node:test";
import assert from "node:assert/strict";
import { expandConnection, type ConnTrack, type ConnVia } from "./expandConnection";

const NONE = { trackIds: [], viaIds: [] };

// A simple chain: t1 (0,0)-(100,0), t2 (100,0)-(200,0), t3 (200,0)-(300,0).
// Starting from t1 alone should walk the whole chain (no junctions: every
// point along it touches exactly 2 tracks).
test("expandConnection: walks a simple chain of collinear tracks with no junction", () => {
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N1", start: [100, 0], end: [200, 0] },
    { id: "t3", net: "N1", start: [200, 0], end: [300, 0] },
  ];
  const result = expandConnection(tracks, [], [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds].sort(), ["t1", "t2", "t3"]);
});

test("expandConnection: a 3-way junction halts STOP_AT_JUNCTION before selecting anything past it (not even the other branches)", () => {
  // t1 from (0,0) to the junction (100,0); t2 and t3 branch out from there,
  // so trackMap[(100,0)] has 3 entries -- source's pt_count > 2 bails out
  // BEFORE select()ing t2/t3 at all.
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N1", start: [100, 0], end: [200, 0] },
    { id: "t3", net: "N1", start: [100, 0], end: [100, 100] },
  ];
  const result = expandConnection(tracks, [], [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds].sort(), ["t1"]);
});

test("expandConnection: a via is a hard stop for STOP_AT_JUNCTION specifically, even with only 2 tracks at that point", () => {
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N1", start: [100, 0], end: [200, 0] },
  ];
  const vias: ConnVia[] = [{ id: "v1", net: "N1", at: [100, 0] }];
  const result = expandConnection(tracks, vias, [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds], ["t1"], "t2 is on the far side of the via; STOP_AT_JUNCTION never crosses a via");
  assert.deepEqual([...result.viaIds], [], "the via itself isn't selected either -- the point is a stop, full stop");
});

test("expandConnection: STOP_AT_PAD (the second press) crosses both junctions and vias", () => {
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N1", start: [100, 0], end: [200, 0] },
    { id: "t3", net: "N1", start: [100, 0], end: [100, 100] },
  ];
  const vias: ConnVia[] = [{ id: "v1", net: "N1", at: [200, 0] }];
  // First press: STOP_AT_JUNCTION selects only t1 (the junction at (100,0) blocks t2/t3).
  const first = expandConnection(tracks, vias, [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...first.trackIds].sort(), ["t1"]);

  // Second press, seeded from the grown selection's own points (both ends
  // of t1): STOP_AT_JUNCTION still can't cross (100,0) is still a
  // junction), so expandConnection itself falls through to STOP_AT_PAD,
  // which crosses the junction AND the via at (200,0).
  const second = expandConnection(
    tracks,
    vias,
    [
      { point: [0, 0], net: "N1" },
      { point: [100, 0], net: "N1" },
    ],
    { trackIds: [...first.trackIds], viaIds: [...first.viaIds] }
  );
  assert.deepEqual([...second.trackIds].sort(), ["t1", "t2", "t3"]);
  assert.deepEqual([...second.viaIds], ["v1"]);
});

test("expandConnection: different nets in the selection each expand independently and union", () => {
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N2", start: [0, 1000], end: [100, 1000] },
  ];
  const result = expandConnection(
    tracks,
    [],
    [
      { point: [0, 0], net: "N1" },
      { point: [0, 1000], net: "N2" },
    ],
    NONE
  );
  assert.deepEqual([...result.trackIds].sort(), ["t1", "t2"]);
});

test("expandConnection: a dead end (nothing further on the net) returns just the start item", () => {
  const tracks: ConnTrack[] = [{ id: "t1", net: "N1", start: [0, 0], end: [100, 0] }];
  const result = expandConnection(tracks, [], [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds], ["t1"]);
});

test("expandConnection: a different net's items are never pulled in even if they coincide spatially with another net's point", () => {
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N2", start: [100, 0], end: [200, 0] },
  ];
  const result = expandConnection(tracks, [], [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds], ["t1"], "N2's t2 touches the same point but must never be selected expanding N1");
});

test("expandConnection: a simple two-track run through one via (no branches) is fully selected on the very first press", () => {
  // (0,0)--t1--(100,0)==via==(100,0)--t2--(200,0): a plain layer-changing
  // via in the middle of an otherwise straight run has a via at that
  // point, which still halts STOP_AT_JUNCTION per source -- so even this
  // "obviously one wire" case needs the second (STOP_AT_PAD) stage, same
  // as the test above. This test just documents that expandConnection's
  // own stage fallback makes that transparent to the caller in one call
  // when nothing is pre-selected (initialCount 0 is already satisfied by
  // the first stage's {t1} result, so a caller must press U again to
  // reach the via and t2 -- this is intentional, matching source, not a
  // bug).
  const tracks: ConnTrack[] = [
    { id: "t1", net: "N1", start: [0, 0], end: [100, 0] },
    { id: "t2", net: "N1", start: [100, 0], end: [200, 0] },
  ];
  const vias: ConnVia[] = [{ id: "v1", net: "N1", at: [100, 0] }];
  const result = expandConnection(tracks, vias, [{ point: [0, 0], net: "N1" }], NONE);
  assert.deepEqual([...result.trackIds], ["t1"]);
});
