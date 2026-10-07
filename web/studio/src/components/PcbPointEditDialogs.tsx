// The point editor's dialogs (actions/pcbPointEditSweep.ts):
//
//   PointEntryDialog     WX_PT_ENTRY_DIALOG ("Move Corner to Location" / "Move Midpoint to Location"): X and Y.
//   VertexEditorPane     PCB_VERTEX_EDITOR_PANE ("Edit Vertices", pcbnew/widgets/vertex_editor_pane.cpp): a floating table of the
//                        selected polygon's or zone's vertices; each edited cell is its own undo step ("Edit Vertex"), and the
//                        table follows the selection.
import { useMemo, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { applyCommands } from "../actions/pcbSweepKit";
import { shapeToCmd } from "../kicad-port/pcbPointEdit";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DialogShell, LengthRow, lengthText, parseLength, useLengthField } from "./pcbDialogKit";

export function PointEntryDialog({ title, x, y, onOk }: { title: string; x: number; y: number; onOk: (x: number, y: number) => void }) {
  const fx = useLengthField(x);
  const fy = useLengthField(y);
  const px = fx.parse();
  const py = fy.parse();
  const ok = px !== null && py !== null;
  const submit = () => {
    if (!ok) return;
    closeSweepDialog();
    onOk(px, py);
  };
  return (
    <DialogShell title={title} width={320} onCancel={closeSweepDialog} onOk={submit} okDisabled={!ok}>
      <LengthRow label="X:" field={fx} autoFocus onEnter={submit} />
      <LengthRow label="Y:" field={fy} onEnter={submit} />
    </DialogShell>
  );
}

type Ring = [number, number][];
interface VertexItem {
  kind: "zone" | "shape";
  id: string;
  ring: Ring;
}

/** `getPoly()`: the outline of the one selected zone or polygon shape. */
function vertexItemOf(board: ReturnType<typeof useStudioState>["board"], selection: ReadonlySet<string>): VertexItem | null {
  if (!board || selection.size !== 1) return null;
  const id = [...selection][0]!;
  const zone = board.routing?.zones.find((z) => z.id === id);
  if (zone) return zone.teardrop ? null : { kind: "zone", id, ring: zone.outline.map((p) => [p[0], p[1]] as [number, number]) };
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  if (shape?.kind === "polygon") return { kind: "shape", id, ring: shape.pts.map((p) => [p[0], p[1]] as [number, number]) };
  return null;
}

export function VertexEditorPane() {
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const item = useMemo(() => vertexItemOf(state.board, state.selection), [state.board, state.selection]);
  if (!state.pcbx.vertexEditorOpen || state.tab !== "pcb") return null;
  const units = state.units;
  const close = () => dispatch({ type: "PCBX", patch: { vertexEditorOpen: false } });

  /** `OnGridCellChange`: parse the cell; a value that does not parse, or has not changed, is put back; otherwise one "Edit Vertex" undo step. */
  const commit = async (i: number, axis: 0 | 1, text: string) => {
    const key = `${item?.id}:${i}:${axis}`;
    setDrafts((d) => {
      const { [key]: _gone, ...rest } = d;
      return rest;
    });
    if (!item) return;
    const value = parseLength(text, units);
    if (value === null || value === item.ring[i]![axis]) return;
    const ring = item.ring.map((p) => [p[0], p[1]] as [number, number]);
    ring[i]![axis] = value;
    if (item.kind === "zone") {
      await api.cmd({ op: "set_zone_outline", id: item.id, outline: ring.map(([x, y]) => ({ x, y })) });
      return;
    }
    const shape = api.shapeById(item.id);
    if (shape?.kind !== "polygon") return;
    // A polygon's id is its geometry's: the edit makes a new one, which the pane (and the selection) follows.
    await applyCommands(api, dispatch, [{ op: "delete_shape", id: item.id }, { op: "add_shape", shape: shapeToCmd({ ...shape, pts: ring }) }], [item.id]);
  };

  const cell = (i: number, axis: 0 | 1) => {
    const key = `${item!.id}:${i}:${axis}`;
    const shown = drafts[key] ?? lengthText(item!.ring[i]![axis], units);
    return (
      <input
        style={{ width: "100%", boxSizing: "border-box" }}
        value={shown}
        aria-label={`${axis === 0 ? "X" : "Y"} of vertex ${i + 1}`}
        onChange={(e) => setDrafts((d) => ({ ...d, [key]: e.target.value }))}
        onBlur={(e) => void commit(i, axis, e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          else if (e.key === "Escape") setDrafts((d) => {
            const { [key]: _gone, ...rest } = d;
            return rest;
          });
        }}
      />
    );
  };

  return (
    <div className="dialog" role="dialog" aria-label="Edit Vertices" style={{ position: "fixed", top: 120, right: 330, width: 300, maxHeight: "60vh", zIndex: 1900 }}>
      <div className="dialog-header">
        <span>Edit Vertices</span>
        <button aria-label="Close" onClick={close} style={{ border: "none", background: "transparent", cursor: "pointer", color: "inherit" }}>
          &times;
        </button>
      </div>
      <div className="dialog-body" style={{ padding: "8px 10px" }}>
        {!item ? (
          <div style={{ fontSize: 12, opacity: 0.7 }}>Select one polygon or zone to edit its vertices.</div>
        ) : (
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
            <thead>
              <tr style={{ textAlign: "left", opacity: 0.8 }}>
                <th style={{ padding: "2px 4px", width: 26 }}>#</th>
                <th style={{ padding: "2px 4px" }}>X coord ({units})</th>
                <th style={{ padding: "2px 4px" }}>Y coord ({units})</th>
              </tr>
            </thead>
            <tbody>
              {item.ring.map((_, i) => (
                <tr key={i}>
                  <td style={{ padding: "1px 4px", opacity: 0.6 }}>{i + 1}</td>
                  <td style={{ padding: "1px 4px" }}>{cell(i, 0)}</td>
                  <td style={{ padding: "1px 4px" }}>{cell(i, 1)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
