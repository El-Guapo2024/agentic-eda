// `DIALOG_TRACK_VIA_PROPERTIES` over the tracks and vias the dialog was opened on (kicad-port/trackViaDialog.ts holds the form and the edits): a field the items share
// shows its value, one where they differ is empty (indeterminate) and is left alone; OK sends what changed as one batch, one undo step.
import { useEffect, useMemo, useRef, useState } from "react";
import type { BoardState, Nm } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { pcbContext } from "../kicad-port/pcbProperties";
import { trackViaCmds, trackViaForm, withNetAnswers, type TrackViaForm } from "../kicad-port/trackViaDialog";
import { postTrackViaPreview } from "../api/client";
import { askConfirm } from "./library/libraryDialogs";
import { useStudioApi } from "../state/store";
import { formatLength, nmFrom, nmTo, type LengthUnit } from "../state/units";

type LengthKey = "startX" | "startY" | "endX" | "endY" | "width" | "viaX" | "viaY" | "diameter" | "drill";

export function TrackViaPropertiesDialog({ board, ids, units, onClose }: { board: BoardState; ids: string[]; units: LengthUnit; onClose: () => void }) {
  const api = useStudioApi();
  const before = useMemo(() => trackViaForm(board, ids), [board, ids]);
  const [form, setForm] = useState<TrackViaForm>(before);
  const [error, setError] = useState<string | null>(null);
  // the fields as typed, in display units ("" is indeterminate)
  const shown = (v: Nm | null) => (v == null ? "" : String(Number(nmTo(v, units).toFixed(6))));
  const [texts, setTexts] = useState<Record<LengthKey, string>>(() => ({ startX: "", startY: "", endX: "", endY: "", width: "", viaX: "", viaY: "", diameter: "", drill: "" }));
  const opened = useRef(false);
  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    setForm(before);
    setTexts({ startX: shown(before.startX), startY: shown(before.startY), endX: shown(before.endX), endY: shown(before.endY), width: shown(before.width), viaX: shown(before.viaX), viaY: shown(before.viaY), diameter: shown(before.diameter), drill: shown(before.drill) });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const nets = pcbContext(board, units).nets;
  const layerSetup = board.board_rules?.layer_setup;
  const copper = boardLayerList(board.layers, layerSetup).filter((l) => l.endsWith(".Cu"));
  const hasTracks = form.tracks.length > 0;
  const hasVias = form.vias.length > 0;
  const lengthRow = (label: string, key: LengthKey) => (
    <>
      <span>{label}</span>
      <span>
        <input
          aria-label={label}
          value={texts[key]}
          placeholder={form[key] == null && before[key] == null ? "<...>" : undefined}
          onChange={(e) => {
            const t = e.target.value;
            setTexts((x) => ({ ...x, [key]: t }));
            const n = Number(t);
            setForm((f) => ({ ...f, [key]: t.trim() === "" || !Number.isFinite(n) ? null : Math.round(nmFrom(n, units)) }));
          }}
          style={{ width: 100 }}
        />{" "}
        {units}
      </span>
    </>
  );
  const ok = async () => {
    const out = trackViaCmds(board, before, form, (nm) => formatLength(nm, units));
    if ("error" in out) return setError(out.error);
    if (out.cmds.length === 0) return onClose();
    // `confirmShortingNets` (Cancel Changes: `commit.Revert()`, the dialog closes with nothing done), then `confirmPadChange`
    let cmds = out.cmds;
    for (;;) {
      const pre = await postTrackViaPreview(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      if (!pre.ok) return setError(pre.message);
      if (!pre.ask) break;
      if (pre.ask.check === "ops_net_change_shorts") {
        if (!(await askConfirm({ title: "Confirmation", message: pre.ask.message, okLabel: "Apply Anyway", cancelLabel: "Cancel Changes" }))) return onClose();
        cmds = withNetAnswers(cmds, { apply_anyway: true });
      } else {
        cmds = withNetAnswers(cmds, { change_pads: await askConfirm({ title: "Confirmation", message: pre.ask.message, okLabel: "Change Nets", cancelLabel: "Leave Nets Unchanged" }) });
        break;
      }
    }
    if (await api.cmdBatch(cmds)) onClose();
  };
  // `DIALOG_TRACK_VIA_PROPERTIES_BASE`'s title, whatever is selected (no `SetTitle`)
  const title = "Track & Via Properties";
  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Track & Via Properties">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            <span>Net</span>
            <span>
              <select aria-label="Net" value={form.net ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, net: e.target.value === "\u0000" ? null : e.target.value }))}>
                {form.net == null && <option value={"\u0000"}>{"<...>"}</option>}
                <option value="">{"<no net>"}</option>
                {nets.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </span>
            <span>Locked</span>
            <span>
              <input
                type="checkbox"
                aria-label="Locked"
                checked={form.locked === true}
                ref={(el) => {
                  if (el) el.indeterminate = form.locked == null;
                }}
                onChange={(e) => setForm((f) => ({ ...f, locked: e.target.checked }))}
              />
            </span>
            {hasTracks && (
              <>
                {lengthRow("Start X", "startX")}
                {lengthRow("Start Y", "startY")}
                {lengthRow("End X", "endX")}
                {lengthRow("End Y", "endY")}
                {lengthRow("Track width", "width")}
                <span>Layer</span>
                <span>
                  <select aria-label="Layer" value={form.layer ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, layer: e.target.value === "\u0000" ? null : e.target.value }))}>
                    {form.layer == null && <option value={"\u0000"}>{"<...>"}</option>}
                    {copper.map((l) => (
                      <option key={l} value={l}>
                        {layerDisplayName(layerSetup, l)}
                      </option>
                    ))}
                  </select>
                </span>
              </>
            )}
            {hasVias && (
              <>
                {lengthRow("Via X", "viaX")}
                {lengthRow("Via Y", "viaY")}
                {lengthRow("Diameter", "diameter")}
                {lengthRow("Hole", "drill")}
              </>
            )}
          </div>
          {error && (
            <div role="alert" style={{ color: "var(--chrome-error, #c0392b)", fontSize: 12, marginTop: 6 }}>
              {error}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
