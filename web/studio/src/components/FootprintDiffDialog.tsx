// Compare Footprint with Library (`pcbnew.InspectionTool.DiffFootprint`): port of BOARD_INSPECTION_TOOL::DiffFootprint's DIALOG_BOOK_REPORTER and of the Visual page's
// FOOTPRINT_DIFF_WIDGET (pcbnew/tools/board_inspection_tool.cpp, common/widgets/footprint_diff_widget.cpp at 8303b2ad).
//
// Two pages. "Summary": the footprint, its library and the library item, then the differences `FOOTPRINT::FootprintNeedsUpdate` finds -- or "No relevant differences
// detected.", or why the library has no copy to compare with. "Visual": the board's footprint and the library's copy of it on top of one another in the footprint's own
// frame, a slider fading from the one to the other ("Board" ... "Library") and the toggle between them (the "/" key). The button "Update Footprint from Library..."
// selects the footprint and opens the update dialog on it, as the C++ dialog's Apply does.
//
// The data is `GET /api/footprint_diff?ref=` (crates/cli/src/fp_assign_api.rs); the lines and the drawing helpers are kicad-port/footprintDiff.ts.
import { useEffect, useMemo, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import {
  fetchFootprintDiff,
  type FootprintDiff,
  type FootprintDrawing,
} from "../api/fpAssign";
import {
  diffOpacity,
  drawingBounds,
  layerRank,
  padFigure,
  shapePath,
  summaryLines,
  toggleSlider,
} from "../kicad-port/footprintDiff";
import { openLibraryTables } from "../state/libraryTableDialogs";
import { layerColor } from "./canvas/layers";
import { useEscape } from "./pcbDialogKit";
import { ReportWindow } from "./ReportWindow";

/** A layer's colour in the KiCad theme (`F.SilkS` is the theme's `F_SilkS`). */
const colorOf = (layer: string): string =>
  layerColor(layer.replace(/\./g, "_"));

function Footprint({
  drawing,
  opacity,
  testId,
}: {
  drawing: FootprintDrawing;
  opacity: number;
  testId: string;
}) {
  const pads = drawing.pads.map(padFigure);
  const shapes = drawing.shapes
    .map(shapePath)
    .sort((a, b) => layerRank(a.layer) - layerRank(b.layer));
  const copper = colorOf("F.Cu");
  const hole = layerColor("LAYER_PAD_PLATEDHOLES");
  return (
    <g opacity={opacity} data-testid={testId} data-opacity={opacity}>
      {drawing.courtyard.map((outline, i) => (
        <path
          key={`c${i}`}
          d={
            outline
              .map(([x, y], j) => `${j === 0 ? "M" : "L"} ${x} ${y}`)
              .join(" ") + " Z"
          }
          fill="none"
          stroke={colorOf("F.CrtYd")}
          strokeWidth={50}
        />
      ))}
      {shapes.map((s, i) => (
        <path
          key={`s${i}`}
          d={s.d}
          fill={s.fill ? colorOf(s.layer) : "none"}
          stroke={colorOf(s.layer)}
          strokeWidth={Math.max(s.width, 20)}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ))}
      {pads.map((p, i) => (
        <g
          key={`p${i}`}
          transform={`translate(${p.x} ${p.y}) rotate(${p.rot})`}
        >
          {p.shape === "ellipse" ? (
            <ellipse rx={p.w / 2} ry={p.h / 2} fill={copper} />
          ) : (
            <rect
              x={-p.w / 2}
              y={-p.h / 2}
              width={p.w}
              height={p.h}
              rx={p.radius}
              fill={copper}
            />
          )}
          {p.hole && (
            <rect
              x={-p.hole.w / 2}
              y={-p.hole.h / 2}
              width={p.hole.w}
              height={p.hole.h}
              rx={Math.min(p.hole.w, p.hole.h) / 2}
              fill={hole}
            />
          )}
        </g>
      ))}
    </g>
  );
}

export function FootprintDiffDialog({
  reference,
  onUpdate,
}: {
  reference: string;
  onUpdate: () => void;
}) {
  useEscape();
  const [diff, setDiff] = useState<FootprintDiff | null>(null);
  const [page, setPage] = useState<"summary" | "visual">("summary");
  const [slider, setSlider] = useState(50);

  useEffect(() => {
    let cancelled = false;
    void fetchFootprintDiff(reference)
      .then((d) => !cancelled && setDiff(d))
      .catch(
        (e: unknown) =>
          !cancelled &&
          setDiff({
            ok: false,
            message: e instanceof Error ? e.message : String(e),
            reference,
            library: "",
            item: "",
            found: false,
          }),
      );
    return () => {
      cancelled = true;
    };
  }, [reference]);

  // "/" toggles between the board's footprint and the library's (`FOOTPRINT_DIFF_WIDGET::onCharHook`)
  useEffect(() => {
    if (page !== "visual") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "/") return;
      e.preventDefault();
      e.stopPropagation();
      setSlider(toggleSlider);
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [page]);

  const bounds = useMemo(
    () =>
      diff?.board && diff.library_copy
        ? drawingBounds([diff.board, diff.library_copy])
        : null,
    [diff],
  );
  const opacity = diffOpacity(slider);
  const margin = bounds
    ? Math.max(bounds.x1 - bounds.x0, bounds.y1 - bounds.y0, 1000) * 0.08
    : 0;
  const viewBox = bounds
    ? `${bounds.x0 - margin} ${bounds.y0 - margin} ${bounds.x1 - bounds.x0 + 2 * margin} ${bounds.y1 - bounds.y0 + 2 * margin}`
    : "0 0 1 1";
  const library =
    diff?.found === false &&
    (diff.reason ?? "").startsWith("The library is not");
  const tab = (id: "summary" | "visual", label: string) => (
    <button
      className={page === id ? "primary" : ""}
      onClick={() => setPage(id)}
      aria-pressed={page === id}
      data-testid={`footprint-diff-tab-${id}`}
    >
      {label}
    </button>
  );

  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div
        className="dialog"
        style={{ width: 640 }}
        onClick={(e) => e.stopPropagation()}
        data-testid="footprint-diff-dialog"
      >
        <div className="dialog-header">
          <span>Compare Footprint with Library</span>
        </div>
        <div className="dialog-body">
          {!diff && <div className="panel-empty">Reading…</div>}
          {diff && !diff.ok && (
            <div className="problem-row">
              {diff.message ?? "The footprint could not be compared."}
            </div>
          )}
          {diff?.ok && (
            <>
              <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
                {tab("summary", "Summary")}
                {diff.found && tab("visual", "Visual")}
              </div>
              {page === "summary" && (
                <>
                  <ReportWindow
                    lines={summaryLines(diff)}
                    testId="footprint-diff-summary"
                    height={220}
                  />
                  {library && (
                    <div style={{ marginTop: 6 }}>
                      <button
                        data-testid="footprint-diff-manage"
                        onClick={() => {
                          closeSweepDialog();
                          openLibraryTables("footprint");
                        }}
                      >
                        Manage Footprint Libraries
                      </button>
                    </div>
                  )}
                </>
              )}
              {page === "visual" && diff.board && diff.library_copy && (
                <>
                  <svg
                    viewBox={viewBox}
                    preserveAspectRatio="xMidYMid meet"
                    style={{
                      width: "100%",
                      height: 340,
                      background: layerColor("LAYER_PCB_BACKGROUND"),
                      display: "block",
                    }}
                    data-testid="footprint-diff-visual"
                  >
                    <Footprint
                      drawing={diff.board}
                      opacity={opacity.board}
                      testId="footprint-diff-board"
                    />
                    <Footprint
                      drawing={diff.library_copy}
                      opacity={opacity.library}
                      testId="footprint-diff-library"
                    />
                  </svg>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      marginTop: 8,
                    }}
                  >
                    <span>Board</span>
                    <input
                      type="range"
                      min={0}
                      max={100}
                      value={slider}
                      onChange={(e) => setSlider(Number(e.target.value))}
                      style={{ flex: 1 }}
                      aria-label="Board to library"
                      data-testid="footprint-diff-slider"
                    />
                    <span>Library</span>
                    <button
                      onClick={() => setSlider(toggleSlider)}
                      title="Toggle between A and B display (/)"
                      data-testid="footprint-diff-toggle"
                    >
                      Swap
                    </button>
                  </div>
                </>
              )}
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button
            disabled={!diff?.ok}
            onClick={onUpdate}
            data-testid="footprint-diff-update"
          >
            Update Footprint from Library...
          </button>
          <button
            className="primary"
            onClick={closeSweepDialog}
            data-testid="footprint-diff-close"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
