// A canvas's wheel zoom calls `preventDefault()`, which a PASSIVE listener ignores (and the browser logs "Unable to preventDefault inside passive event
// listener invocation" on every tick, with the page scrolling under the zoom). React's `onWheel` prop is passive, so every canvas attaches its wheel
// listener itself with `{ passive: false }` (src/hooks/useNonPassiveWheel.ts; the 3D viewer does it inline). This test keeps that true: no component may
// hand a wheel handler to React's `onWheel` (the schematic context menu's `onWheel={keep}` only stops propagation and never calls preventDefault), and
// each of the five canvases must attach a non-passive listener.
//
// Plain Node, no TypeScript involved -- run directly:
//   node --test tools/lib/passiveWheel.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "src");

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (p.endsWith(".tsx") || p.endsWith(".ts")) out.push(p);
  }
  return out;
}

const read = (rel) => readFileSync(join(SRC, rel), "utf8");

test("no component gives React's passive onWheel a handler that can zoom", () => {
  const offenders = [];
  for (const file of walk(SRC)) {
    const text = readFileSync(file, "utf8");
    // `onWheel={keep}` (stopPropagation only) is the one legitimate use: the schematic context menu shielding the sheet underneath.
    for (const m of text.matchAll(/onWheel=\{([^}]*)\}/g)) {
      if (m[1].trim() !== "keep") offenders.push(`${relative(SRC, file)}: onWheel={${m[1].trim()}}`);
    }
  }
  assert.deepEqual(offenders, [], `use useNonPassiveWheel (src/hooks/useNonPassiveWheel.ts) instead:\n${offenders.join("\n")}`);
});

test("every canvas attaches its wheel listener with passive: false", () => {
  for (const rel of ["components/canvas/Canvas.tsx", "components/SchematicView.tsx", "components/footprint/FootprintCanvas.tsx", "components/symbol/SymbolEditorCanvas.tsx"]) {
    assert.match(read(rel), /useNonPassiveWheel\(\s*containerRef\s*,\s*onWheel\s*\)/, `${rel} must use useNonPassiveWheel(containerRef, onWheel)`);
  }
  assert.match(read("components/viewer3d/Viewer3D.tsx"), /addEventListener\("wheel",\s*onWheel,\s*\{\s*passive:\s*false\s*\}\)/, "the 3D viewer must attach its wheel listener with { passive: false }");
  assert.match(read("hooks/useNonPassiveWheel.ts"), /addEventListener\("wheel",\s*onWheel,\s*\{\s*passive:\s*false\s*\}\)/, "the hook must attach with { passive: false }");
});
