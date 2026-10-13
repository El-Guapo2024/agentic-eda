// The Footprint Editor's Footprint Checker (pcbnew/dialogs/dialog_footprint_checker.cpp over dialog_footprint_checker_base.cpp): "Show:" All / Errors / Warnings / Exclusions with a
// badge of each count, the list of markers (what failed and, under it, the items it names), and Delete Marker, Delete All Markers, Run Checks and Close. Modeless: the markers are on
// the canvas while it is up. The checks are kicad-cli's DRC on a scratch board that holds the footprint (actions/footprintCheckerActions.ts); the state is state/footprintChecker.ts.
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { setFootprintChecker, useFootprintChecker } from "../../state/footprintChecker";
import { badges, badgeText, setShowAll, showAll, visibleMarkers } from "../../kicad-port/footprintChecker";
import { closeFootprintChecker, deleteAllMarkers, deleteSelectedMarker, runChecks, selectMarker, stepMarker } from "../../actions/footprintCheckerActions";

const SEVERITY_COLOR: Record<string, string> = { error: "#e0525a", warning: "#e0b341", exclusion: "#9aa5b1" };

function Badge({ label, n, severity, testid }: { label: string; n: number | null; severity: string; testid: string }) {
  return (
    <span data-testid={testid} title={label} style={{ display: "inline-flex", alignItems: "center", gap: 4, padding: "0 6px", borderRadius: 9, background: SEVERITY_COLOR[severity], color: "#111", fontSize: 11, fontWeight: 700, minWidth: 18, justifyContent: "center" }}>
      {badgeText(n)}
    </span>
  );
}

export function FootprintCheckerDialog() {
  const f = useFootprintChecker();
  const fp = useFpState();
  const api = useFpApi();
  const dispatch = useFpDispatch();
  if (!f.open) return null;
  const rows = visibleMarkers(f.markers, f.show);
  const b = badges(f.markers, f.checksRun);
  const box = (label: string, key: "errors" | "warnings" | "exclusions", n: number | null, severity: string) => (
    <label className="toggle" style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
      <input type="checkbox" data-testid={`fpc-show-${key}`} checked={f.show[key]} onChange={(e) => setFootprintChecker({ show: { ...f.show, [key]: e.target.checked } })} />
      {label}
      <Badge label={`${label}`} n={n} severity={severity} testid={`fpc-badge-${key}`} />
    </label>
  );
  return (
    <div
      className="dialog"
      role="dialog"
      aria-label="Footprint Checker"
      data-testid="footprint-checker"
      style={{ position: "fixed", top: 96, right: 24, width: "min(520px, calc(100vw - 24px))", maxHeight: "74vh", zIndex: 1900, display: "flex", flexDirection: "column" }}
      onKeyDown={(e) => {
        if (e.key === "Escape") closeFootprintChecker();
        else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          stepMarker(e.key === "ArrowDown" ? 1 : -1, api, dispatch);
        }
      }}
    >
      <div className="dialog-header">
        <span>Footprint Checker{fp.name ? ` - ${fp.name}` : ""}</span>
        <button aria-label="Close" onClick={closeFootprintChecker} style={{ border: "none", background: "transparent", cursor: "pointer", color: "inherit" }}>
          &times;
        </button>
      </div>
      <div className="dialog-body" style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8, minHeight: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
          <span>Show:</span>
          <label className="toggle" style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
            <input type="checkbox" data-testid="fpc-show-all" checked={showAll(f.show)} onChange={(e) => setFootprintChecker({ show: setShowAll(e.target.checked) })} />
            All
          </label>
          {box("Errors", "errors", b.errors, "error")}
          {box("Warnings", "warnings", b.warnings, "warning")}
          {box("Exclusions", "exclusions", b.exclusions, "exclusion")}
        </div>
        <div data-testid="fpc-list" role="listbox" aria-label="Markers" style={{ overflow: "auto", border: "1px solid var(--chrome-border)", borderRadius: 3, minHeight: 150, flex: 1 }}>
          {f.running && <div style={{ padding: 8 }}>Checking the footprint with kicad-cli...</div>}
          {f.error && (
            <div data-testid="fpc-error" role="alert" style={{ padding: 8, color: "#ffb4b4" }}>
              {f.error}
            </div>
          )}
          {!f.running && !f.error && f.checksRun && rows.length === 0 && (
            <div data-testid="fpc-none" style={{ padding: 8, color: "var(--chrome-text-dim)" }}>
              {f.markers.length === 0 ? "No problems found." : "Every marker is hidden by the Show boxes."}
            </div>
          )}
          {!f.running && !f.error && !f.checksRun && f.markers.length === 0 && <div style={{ padding: 8, color: "var(--chrome-text-dim)" }}>Run Checks to check the footprint.</div>}
          {rows.map((i) => {
            const m = f.markers[i]!;
            return (
              <div
                key={i}
                data-testid={`fpc-marker-${i}`}
                data-type={m.type}
                data-severity={m.severity}
                role="option"
                aria-selected={f.selected === i}
                onClick={() => selectMarker(i, api, dispatch)}
                style={{ padding: "5px 8px", cursor: "pointer", borderBottom: "1px solid var(--chrome-border)", background: f.selected === i ? "var(--chrome-active)" : undefined }}
              >
                <div style={{ display: "flex", gap: 6, alignItems: "baseline" }}>
                  <span style={{ color: SEVERITY_COLOR[m.severity], fontWeight: 700 }}>{m.severity === "error" ? "●" : "▲"}</span>
                  <span>
                    <span style={{ color: "var(--chrome-text-dim)" }}>[{m.type}]</span> {m.description}
                  </span>
                </div>
                {m.items.map((it, k) => (
                  <div key={k} style={{ marginLeft: 22, fontSize: 12, color: "var(--chrome-text-dim)" }}>
                    {it.description}
                  </div>
                ))}
              </div>
            );
          })}
        </div>
        <div style={{ display: "flex", justifyContent: "space-between", gap: 8, flexWrap: "wrap" }}>
          <span style={{ display: "flex", gap: 8 }}>
            <button data-testid="fpc-delete-one" disabled={f.selected === null} onClick={() => deleteSelectedMarker(dispatch)}>
              Delete Marker
            </button>
            <button data-testid="fpc-delete-all" disabled={f.markers.length === 0} onClick={() => deleteAllMarkers(dispatch)}>
              Delete All Markers
            </button>
          </span>
          <span style={{ display: "flex", gap: 8 }}>
            <button data-testid="fpc-run" className="primary" disabled={f.running || !fp.name} onClick={() => void runChecks(api, dispatch)}>
              Run Checks
            </button>
            <button data-testid="fpc-close" onClick={closeFootprintChecker}>
              Close
            </button>
          </span>
        </div>
        {f.engine && <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>{f.engine}</div>}
      </div>
    </div>
  );
}
