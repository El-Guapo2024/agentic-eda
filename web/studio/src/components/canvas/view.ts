// Thin re-export: the actual view-transform math is now the dependency-
// free port at src/kicad-port/view.ts (so it can be unit-tested with
// plain `node --test`, with no React/store import graph to compile).
// Kept here so every existing `from "./view"` import in this directory
// (Canvas.tsx, painter.ts, routing.ts, ...) keeps working unchanged.
export * from "../../kicad-port/view";
