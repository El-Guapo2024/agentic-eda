// pcbnew.InspectionTool.ShowFootprintAssociations -- port of pcbnew/dialogs/dialog_footprint_associations.cpp: for the one selected
// footprint, its "Library Association" (the library and the footprint in it, each with the description the library gives it) and
// its "Schematic Association" (the sheet and the symbol it belongs to). The data is `GET /api/footprint_associations?ref=`
// (crates/cli/src/board_control_api.rs): the part's footprint name, the project library's descriptions, and the schematic symbol
// with the same reference. KiCad shows the symbol's and sheets' uuids; this studio's symbols are known by reference, so that is shown.
import { useEffect, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { fetchFootprintAssociations, type FootprintAssociations } from "../api/boardControl";

function Table({ rows }: { rows: [string, string, string][] }) {
  return (
    <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
      <tbody>
        {rows.map(([a, b, c], i) => (
          <tr key={i} style={{ borderBottom: "1px solid var(--chrome-border, #444)" }}>
            <td style={{ padding: "3px 8px 3px 0", color: "var(--chrome-text-dim)", whiteSpace: "nowrap" }}>{a}</td>
            <td style={{ padding: "3px 8px" }}>{b || <span style={{ color: "var(--chrome-text-dim)" }}>(none)</span>}</td>
            <td style={{ padding: "3px 0", color: "var(--chrome-text-dim)" }}>{c}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function FootprintAssociationsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const ref = state.bcx.associationsRef;
  const [report, setReport] = useState<FootprintAssociations | null>(null);

  useEffect(() => {
    setReport(null);
    if (!ref) return;
    let cancelled = false;
    void fetchFootprintAssociations(ref).then((r) => {
      if (!cancelled) setReport(r);
    });
    return () => {
      cancelled = true;
    };
  }, [ref]);

  if (!ref) return null;
  const close = () => dispatch({ type: "BCX", patch: { associationsRef: null } });

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Footprint Associations -- {ref}</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          {!report && <div className="panel-empty">Reading…</div>}
          {report && !report.ok && <div className="problem-row">{report.message ?? "No report."}</div>}
          {report?.ok && (
            <>
              <p style={{ margin: "0 0 4px", fontWeight: 600 }}>Library Association</p>
              <Table
                rows={[
                  ["Library:", report.library, report.library_description],
                  ["Footprint:", report.footprint, report.footprint_description],
                ]}
              />
              <p style={{ margin: "12px 0 4px", fontWeight: 600 }}>Schematic Association</p>
              {report.symbol ? (
                <Table
                  rows={[
                    ["Sheet:", report.symbol.sheet, ""],
                    ["Symbol:", report.symbol.reference, [report.symbol.lib_id, report.symbol.value].filter(Boolean).join("  ")],
                  ]}
                />
              ) : (
                <div className="panel-empty">The schematic has no symbol for {report.reference}.</div>
              )}
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
