import { test } from "node:test";
import assert from "node:assert/strict";
import { ClickDragGesture, DRAG_DISTANCE_PX, DRAG_TIME_MS, dragRuleFor, type ButtonRelease } from "./dragThreshold";

/** A scripted pointer sequence: a press, some motions, a release, each at a screen point and a time in ms. */
type Step = { type: "down" | "move" | "up"; x: number; y: number; t: number };

/** Runs the sequence through a fresh gesture and says what the release was and whether (and at which motion) the press became a drag. */
function run(mac: boolean, steps: Step[]): { release: ButtonRelease | null; startedAt: number | null; draggingAfter: boolean[] } {
  const g = new ClickDragGesture(dragRuleFor(mac));
  let release: ButtonRelease | null = null;
  let startedAt: number | null = null;
  const draggingAfter: boolean[] = [];
  steps.forEach((s, i) => {
    if (s.type === "down") g.down(s.x, s.y, s.t);
    else if (s.type === "move") {
      const r = g.move(s.x, s.y, s.t);
      if (r.started && startedAt === null) startedAt = i;
    } else release = g.up();
    draggingAfter.push(g.dragging);
  });
  return { release, startedAt, draggingAfter };
}

const down = (x: number, y: number, t = 0): Step => ({ type: "down", x, y, t });
const move = (x: number, y: number, t: number): Step => ({ type: "move", x, y, t });
const up = (x: number, y: number, t: number): Step => ({ type: "up", x, y, t });

test("the constants are the dispatcher's: 8 px, 300 ms, and the time rule is the macOS build's alone", () => {
  assert.equal(DRAG_DISTANCE_PX, 8);
  assert.equal(DRAG_TIME_MS, 300);
  assert.deepEqual(dragRuleFor(true), { distancePx: 8, holdMs: 300 });
  assert.deepEqual(dragRuleFor(false), { distancePx: 8, holdMs: null });
});

test("a press followed by a few pixels of jitter is a click, not a drag", () => {
  // The case that used to count as a drag: one grid step of pointer travel (a grid of 1.27 mm at a normal zoom is some 5 to 7 px).
  for (const mac of [true, false]) {
    const r = run(mac, [down(100, 100), move(103, 100, 20), move(106, 102, 40), move(101, 99, 60), up(101, 99, 80)]);
    assert.equal(r.release, "click", `mac=${mac}`);
    assert.equal(r.startedAt, null);
    assert.deepEqual(r.draggingAfter, [false, false, false, false, false]);
  }
});

test("the distance rule is per axis and strict: 8 px is a click, 9 px is a drag", () => {
  assert.equal(run(false, [down(0, 0), move(8, 0, 10), up(8, 0, 20)]).release, "click");
  assert.equal(run(false, [down(0, 0), move(-8, 0, 10), up(-8, 0, 20)]).release, "click");
  assert.equal(run(false, [down(0, 0), move(0, 8, 10), up(0, 8, 20)]).release, "click");
  assert.equal(run(false, [down(0, 0), move(9, 0, 10), up(9, 0, 20)]).release, "dragEnd");
  assert.equal(run(false, [down(0, 0), move(-9, 0, 10), up(-9, 0, 20)]).release, "dragEnd");
  assert.equal(run(false, [down(0, 0), move(0, -9, 10), up(0, -9, 20)]).release, "dragEnd");
  // It is not the length that is compared: (8, 8) is 11.3 px away and still a click; one more pixel on either axis makes it a drag.
  assert.equal(run(false, [down(50, 50), move(58, 58, 10), up(58, 58, 20)]).release, "click");
  assert.equal(run(false, [down(50, 50), move(59, 58, 10), up(59, 58, 20)]).release, "dragEnd");
  assert.equal(run(false, [down(50, 50), move(58, 59, 10), up(58, 59, 20)]).release, "dragEnd");
});

test("the drag starts at the first motion past the threshold, and the sequence says which", () => {
  const r = run(false, [down(10, 10), move(12, 10, 5), move(15, 10, 10), move(19, 10, 15), move(40, 10, 20), up(40, 10, 25)]);
  assert.equal(r.startedAt, 3, "19 - 10 = 9 > 8 is the third motion");
  assert.deepEqual(r.draggingAfter, [false, false, false, true, true, false]);
  assert.equal(r.release, "dragEnd");
});

test("a drag that has started stays a drag when the pointer comes back to where it began", () => {
  const r = run(false, [down(10, 10), move(30, 10, 5), move(10, 10, 10), up(10, 10, 15)]);
  assert.equal(r.release, "dragEnd");
  assert.deepEqual(r.draggingAfter, [false, true, true, false]);
});

test("macOS: held longer than 300 ms, the next motion starts a drag however small it is", () => {
  const r = run(true, [down(100, 100, 1000), move(101, 100, 1301), up(101, 100, 1310)]);
  assert.equal(r.release, "dragEnd");
  assert.equal(r.startedAt, 1);
});

test("macOS: exactly 300 ms is not enough (the C++ compares with >), and a hold with no motion is no drag", () => {
  assert.equal(run(true, [down(0, 0, 1000), move(1, 0, 1300), up(1, 0, 1310)]).release, "click");
  // Held for two seconds without a single motion event: the time rule only runs inside a motion, so this is a click.
  assert.equal(run(true, [down(0, 0, 0), up(0, 0, 2000)]).release, "click");
  // A motion before the 300 ms, then still: still a click.
  assert.equal(run(true, [down(0, 0, 0), move(2, 0, 100), up(2, 0, 900)]).release, "click");
});

test("other platforms have no time rule: a long press with a nudge is a click", () => {
  const r = run(false, [down(100, 100, 0), move(101, 101, 5000), up(101, 101, 6000)]);
  assert.equal(r.release, "click");
  assert.equal(r.startedAt, null);
});

test("the origin is where the press happened, and a later press is not seen while one is down", () => {
  const g = new ClickDragGesture(dragRuleFor(false));
  g.down(10, 20, 100);
  g.down(500, 500, 150);
  assert.deepEqual(g.origin, [10, 20]);
  assert.equal(g.downAt, 100);
  assert.equal(g.move(14, 22, 160).dragging, false);
  assert.equal(g.move(19, 22, 170).dragging, true, "9 px from the FIRST press");
});

test("a motion with nothing pressed is not a drag; releasing resets, and the next press starts afresh", () => {
  const g = new ClickDragGesture(dragRuleFor(true));
  assert.equal(g.move(300, 300, 5000).dragging, false);
  assert.equal(g.origin, null);
  g.down(0, 0, 0);
  g.move(50, 0, 10);
  assert.equal(g.dragging, true);
  assert.equal(g.up(), "dragEnd");
  assert.equal(g.pressed, false);
  assert.equal(g.dragging, false);
  g.down(50, 0, 100);
  assert.equal(g.move(52, 0, 110).dragging, false, "new press, new origin: 2 px");
  assert.equal(g.up(), "click");
});

test("reset forgets the press (a tool was cancelled under it)", () => {
  const g = new ClickDragGesture(dragRuleFor(false));
  g.down(0, 0, 0);
  g.move(30, 0, 5);
  g.reset();
  assert.equal(g.pressed, false);
  assert.equal(g.dragging, false);
  assert.equal(g.move(60, 0, 10).dragging, false, "nothing is pressed any more");
});
