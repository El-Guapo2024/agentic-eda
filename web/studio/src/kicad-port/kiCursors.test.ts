import { test } from "node:test";
import assert from "node:assert/strict";
import { KICURSOR_CSS, LINE_CURSOR_HOTSPOT } from "./kiCursors";

test("the wire, bus and line cursors are KiCad's bitmaps with their (5, 26) hotspot, the crosshair behind them", () => {
  assert.deepEqual([...LINE_CURSOR_HOTSPOT], [5, 26]);
  for (const c of Object.values(KICURSOR_CSS)) assert.match(c, /^url\("data:image\/svg\+xml,.+"\) 5 26, crosshair$/);
  // the wire's pencil is green (#008000), the bus's blue (#0000FF)
  assert.ok(decodeURIComponent(KICURSOR_CSS.wire).includes("#008000") && decodeURIComponent(KICURSOR_CSS.bus).includes("#0000FF"));
});
