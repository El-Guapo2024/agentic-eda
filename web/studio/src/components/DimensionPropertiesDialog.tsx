// Port of pcbnew/dialogs/dialog_dimension_properties{,_base}.cpp -- opens
// on "E"/double-click of an existing dimension, and automatically right
// after the two-click placement tool creates one (task item 7; see
// Canvas.tsx's own `nextDimensionKind` handling). One dialog for every
// kind (source has several near-identical `DIALOG_DIMENSION_PROPERTIES`
// panels gated by `GetType()`; this just shows/hides the one field group
// that differs -- height/horizontal/leader-length) rather than five
// separate dialogs.
//
// Not ported: changing a dimension's kind after creation (source has no
// such control either -- each kind is a different drawing tool); `DIM_
// PRECISION`'s unit-dependent "V_VVV" levels (`precision` here is a flat
// decimal count, see `Dimension`'s own doc); `DIM_TEXT_POSITION::MANUAL`
// (no point-editor-style text dragging); the interactive centre-point/
// -item picker buttons (plain numeric X/Y fields instead, same convention
// every other dialog in this app already uses).
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { toCmdDimension } from "../kicad-port/dimensionConvert";
import { umFrom, umTo, type LengthUnit } from "../state/units";
import type { ArrowDirection, CmdDimensionKind, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat } from "../api/types";

export function DimensionPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const id = state.dimensionEditId;
  const dim = id ? api.dimensionById(id) : undefined;
  const unit: LengthUnit = state.units;

  const [layer, setLayer] = useState("");
  const [startX, setStartX] = useState(0);
  const [startY, setStartY] = useState(0);
  const [endX, setEndX] = useState(0);
  const [endY, setEndY] = useState(0);
  const [height, setHeight] = useState(0);
  const [horizontal, setHorizontal] = useState(true);
  const [leaderLength, setLeaderLength] = useState(0);
  const [prefix, setPrefix] = useState("");
  const [suffix, setSuffix] = useState("");
  const [overrideOn, setOverrideOn] = useState(false);
  const [overrideText, setOverrideText] = useState("");
  const [units, setUnits] = useState<DimensionUnits>("automatic");
  const [unitsFormat, setUnitsFormat] = useState<DimensionUnitsFormat>("no_suffix");
  const [precision, setPrecision] = useState(4);
  const [suppressZeros, setSuppressZeros] = useState(true);
  const [textPosition, setTextPosition] = useState<DimensionTextPosition>("outside");
  const [keepAligned, setKeepAligned] = useState(true);
  const [textAngle, setTextAngle] = useState(0);
  const [textSize, setTextSize] = useState(0);
  const [strokeWidth, setStrokeWidth] = useState(0);
  const [arrowLength, setArrowLength] = useState(0);
  const [extensionOffset, setExtensionOffset] = useState(0);
  const [extensionHeight, setExtensionHeight] = useState(0);
  const [arrowDirection, setArrowDirection] = useState<ArrowDirection>("outward");
  const [busy, setBusy] = useState(false);

  // Load the current dimension's fields whenever the dialog opens on a
  // (possibly different) one -- same "reset from the real thing every
  // open" convention ZoneDialog.tsx's edit mode already uses.
  useEffect(() => {
    if (!dim) return;
    setLayer(dim.layer);
    setStartX(umTo(dim.start[0], unit));
    setStartY(umTo(dim.start[1], unit));
    setEndX(umTo(dim.end[0], unit));
    setEndY(umTo(dim.end[1], unit));
    setHeight(umTo(dim.height ?? 0, unit));
    setHorizontal(dim.horizontal ?? true);
    setLeaderLength(umTo(dim.leader_length ?? 0, unit));
    setPrefix(dim.prefix);
    setSuffix(dim.suffix);
    setOverrideOn(dim.override_text != null);
    setOverrideText(dim.override_text ?? "");
    setUnits(dim.units);
    setUnitsFormat(dim.units_format);
    setPrecision(dim.precision);
    setSuppressZeros(dim.suppress_trailing_zeros);
    setTextPosition(dim.text_position);
    setKeepAligned(dim.keep_text_aligned);
    setTextAngle(dim.text_angle / 1000);
    setTextSize(umTo(dim.text_size_um, unit));
    setStrokeWidth(umTo(dim.stroke_width, unit));
    setArrowLength(umTo(dim.arrow_length, unit));
    setExtensionOffset(umTo(dim.extension_offset, unit));
    setExtensionHeight(umTo(dim.extension_height, unit));
    setArrowDirection(dim.arrow_direction);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dim?.id]);

  if (!id || !dim) return null;

  const close = () => dispatch({ type: "SET_DIMENSION_EDIT_ID", id: null });

  const kindObj: CmdDimensionKind =
    dim.kind === "aligned"
      ? { kind: "aligned", height: Math.round(umFrom(height, unit)) }
      : dim.kind === "orthogonal"
        ? { kind: "orthogonal", height: Math.round(umFrom(height, unit)), horizontal }
        : dim.kind === "radial"
          ? { kind: "radial", leader_length: Math.round(umFrom(leaderLength, unit)) }
          : dim.kind === "leader"
            ? { kind: "leader" }
            : { kind: "center" };

  const save = async () => {
    setBusy(true);
    try {
      const cmd = toCmdDimension(dim);
      cmd.layer = layer;
      cmd.kind = kindObj;
      cmd.start = { x: Math.round(umFrom(startX, unit)), y: Math.round(umFrom(startY, unit)) };
      cmd.end = { x: Math.round(umFrom(endX, unit)), y: Math.round(umFrom(endY, unit)) };
      cmd.prefix = prefix;
      cmd.suffix = suffix;
      cmd.override_text = overrideOn ? overrideText : null;
      cmd.units = units;
      cmd.units_format = unitsFormat;
      cmd.precision = precision;
      cmd.suppress_trailing_zeros = suppressZeros;
      cmd.text_position = textPosition;
      cmd.keep_text_aligned = keepAligned;
      cmd.text_angle = Math.round(textAngle * 1000);
      cmd.text_size_um = Math.round(umFrom(textSize, unit));
      cmd.stroke_width = Math.round(umFrom(strokeWidth, unit));
      cmd.arrow_length = Math.round(umFrom(arrowLength, unit));
      cmd.extension_offset = Math.round(umFrom(extensionOffset, unit));
      cmd.extension_height = Math.round(umFrom(extensionHeight, unit));
      cmd.arrow_direction = arrowDirection;
      const ok = await api.cmd({ op: "edit_dimension", id, dimension: cmd });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      const ok = await api.cmd({ op: "delete_dimension", id });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const kindLabel = dim.kind[0]!.toUpperCase() + dim.kind.slice(1);
  const layers = state.board?.layers ?? [];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480, maxHeight: "85vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{kindLabel} Dimension</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", rowGap: 6 }}>
            <span>Layer</span>
            <select value={layer} onChange={(e) => setLayer(e.target.value)}>
              {!layers.includes(layer) && <option value={layer}>{layer}</option>}
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>

            <span>
              Start ({unit}) X / Y
            </span>
            <span style={{ display: "flex", gap: 4 }}>
              <input type="number" step="any" value={startX} onChange={(e) => setStartX(Number(e.target.value))} style={{ width: "50%" }} />
              <input type="number" step="any" value={startY} onChange={(e) => setStartY(Number(e.target.value))} style={{ width: "50%" }} />
            </span>

            <span>{dim.kind === "radial" || dim.kind === "center" ? `${dim.kind === "radial" ? "Point on circle" : "Arm end"} (${unit}) X / Y` : `End (${unit}) X / Y`}</span>
            <span style={{ display: "flex", gap: 4 }}>
              <input type="number" step="any" value={endX} onChange={(e) => setEndX(Number(e.target.value))} style={{ width: "50%" }} />
              <input type="number" step="any" value={endY} onChange={(e) => setEndY(Number(e.target.value))} style={{ width: "50%" }} />
            </span>

            {(dim.kind === "aligned" || dim.kind === "orthogonal") && (
              <>
                <span>Height ({unit})</span>
                <input type="number" step="any" value={height} onChange={(e) => setHeight(Number(e.target.value))} />
              </>
            )}
            {dim.kind === "orthogonal" && (
              <>
                <span>Orientation</span>
                <span>
                  <label className="filter-row" style={{ marginRight: 12 }}>
                    <input type="radio" checked={horizontal} onChange={() => setHorizontal(true)} /> Horizontal
                  </label>
                  <label className="filter-row">
                    <input type="radio" checked={!horizontal} onChange={() => setHorizontal(false)} /> Vertical
                  </label>
                </span>
              </>
            )}
            {dim.kind === "radial" && (
              <>
                <span>Leader length ({unit})</span>
                <input type="number" step="any" value={leaderLength} onChange={(e) => setLeaderLength(Number(e.target.value))} />
              </>
            )}
          </div>

          {dim.kind !== "center" && (
            <>
              <div style={{ fontWeight: 600, fontSize: 11, marginTop: 10, marginBottom: 4 }}>Text</div>
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", rowGap: 6 }}>
                <span>Prefix</span>
                <input type="text" value={prefix} onChange={(e) => setPrefix(e.target.value)} />
                <span>Suffix</span>
                <input type="text" value={suffix} onChange={(e) => setSuffix(e.target.value)} />
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <input type="checkbox" checked={overrideOn} onChange={(e) => setOverrideOn(e.target.checked)} /> Override value
                </label>
                <input type="text" value={overrideText} disabled={!overrideOn} onChange={(e) => setOverrideText(e.target.value)} placeholder={dim.text} />

                {!overrideOn && (
                  <>
                    <span>Units</span>
                    <select value={units} onChange={(e) => setUnits(e.target.value as DimensionUnits)}>
                      <option value="automatic">Automatic</option>
                      <option value="mm">Millimeters</option>
                      <option value="mil">Mils</option>
                      <option value="inch">Inches</option>
                    </select>
                    <span>Units format</span>
                    <select value={unitsFormat} onChange={(e) => setUnitsFormat(e.target.value as DimensionUnitsFormat)}>
                      <option value="no_suffix">1234.0</option>
                      <option value="bare_suffix">1234.0 mm</option>
                      <option value="paren_suffix">1234.0 (mm)</option>
                    </select>
                    <span>Precision</span>
                    <input type="number" min={0} max={5} step={1} value={precision} onChange={(e) => setPrecision(Math.max(0, Math.min(5, Math.round(Number(e.target.value)))))} />
                    <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      <input type="checkbox" checked={suppressZeros} onChange={(e) => setSuppressZeros(e.target.checked)} /> Suppress trailing zeros
                    </label>
                    <span />
                  </>
                )}

                <span>Text position</span>
                <select value={textPosition} onChange={(e) => setTextPosition(e.target.value as DimensionTextPosition)}>
                  <option value="outside">Outside</option>
                  <option value="inline">Inline</option>
                </select>
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <input type="checkbox" checked={keepAligned} onChange={(e) => setKeepAligned(e.target.checked)} /> Keep text aligned with dimension
                </label>
                <span />
                <span>Text angle (&deg;, clockwise)</span>
                <input type="number" step="any" value={textAngle} disabled={keepAligned} onChange={(e) => setTextAngle(Number(e.target.value))} />
                <span>Text size ({unit})</span>
                <input type="number" step="any" value={textSize} onChange={(e) => setTextSize(Number(e.target.value))} />
              </div>
            </>
          )}

          <div style={{ fontWeight: 600, fontSize: 11, marginTop: 10, marginBottom: 4 }}>Line &amp; arrows</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", rowGap: 6 }}>
            <span>Line thickness ({unit})</span>
            <input type="number" step="any" value={strokeWidth} onChange={(e) => setStrokeWidth(Number(e.target.value))} />
            {dim.kind !== "center" && (
              <>
                <span>Arrow length ({unit})</span>
                <input type="number" step="any" value={arrowLength} onChange={(e) => setArrowLength(Number(e.target.value))} />
              </>
            )}
            {(dim.kind === "aligned" || dim.kind === "orthogonal") && (
              <>
                <span>Extension offset ({unit})</span>
                <input type="number" step="any" value={extensionOffset} onChange={(e) => setExtensionOffset(Number(e.target.value))} />
                <span>Extension past crossbar ({unit})</span>
                <input type="number" step="any" value={extensionHeight} onChange={(e) => setExtensionHeight(Number(e.target.value))} />
              </>
            )}
            {(dim.kind === "aligned" || dim.kind === "orthogonal") && (
              <>
                <span>Arrows</span>
                <span>
                  <label className="filter-row" style={{ marginRight: 12 }}>
                    <input type="radio" checked={arrowDirection === "outward"} onChange={() => setArrowDirection("outward")} /> Outward
                  </label>
                  <label className="filter-row">
                    <input type="radio" checked={arrowDirection === "inward"} onChange={() => setArrowDirection("inward")} /> Inward
                  </label>
                </span>
              </>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={remove} disabled={busy} style={{ marginRight: "auto" }}>
            Delete
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={save} disabled={busy}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
