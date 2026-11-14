// Port of pcbnew/dialogs/dialog_dimension_properties{,_base}.cpp -- opens
// on "E"/double-click of an existing dimension, and on a leader right after
// the Draw Dimension tool adds it ("Run the edit immediately to set the leader
// text", `DRAWING_TOOL::DrawDimension`). One dialog for every
// kind (source has several near-identical `DIALOG_DIMENSION_PROPERTIES`
// panels gated by `GetType()`; this just shows/hides the one field group
// that differs -- height/horizontal/leader-length) rather than five
// separate dialogs. The text's Position mode has KiCad's Outside, Inline and
// Manual, the text's X and Y taking effect in the last (`allowPositioning`); a
// leader has no position mode but its Text frame (None, Rectangle, Circle),
// UI-LEFT P13.
//
// Not ported: changing a dimension's kind after creation (source has no
// such control either -- each kind is a different drawing tool); the interactive centre-point/
// -item picker buttons (plain numeric X/Y fields instead, same convention
// every other dialog in this app already uses).
//
// The "Dimension Text" box's Font, Width, Height, Thickness and its row of Bold, Italic, the three alignments and Mirrored (UI-LEFT-2 PP11) are the
// label's own looks (`Dimension::text_effects`, kicad-port/dimensionLabel.ts): `TransferDataToWindow` shows them, the Bold button sets the thickness to the
// bold or normal pen (`onBoldToggle`), a thickness typed in sets Bold (`onThickness`), the stroke font shows the thickness and an outline face hides it
// (`onFontSelected`), and OK writes them (`updateDimensionFromDialog`).
//
// The form is one for both editors (`DIALOG_DIMENSION_PROPERTIES` is opened by the board editor's and the footprint editor's `OnEditItemRequest`): the board's dialog is
// `DimensionPropertiesDialog` below, the Footprint Editor's components/footprint/FpItemDialogs.tsx, each a host of `DimensionPropertiesForm`.
import { useEffect, useState, type ReactNode } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DIM_PRECISION_CHOICES, toCmdDimension } from "../kicad-port/dimensionConvert";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";
import type { ArrowDirection, CmdDimension, CmdDimensionKind, Dimension, DimensionTextBorder, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, TextJustify } from "../api/types";
import { autoThicknessToggled, boldToggled, fontPicked, labelLooksOf, thicknessTyped, widthTyped, withLabelLooks, type DimensionLabelLooks } from "../kicad-port/dimensionLabel";
import { KICAD_FONT } from "../kicad-port/textLooks";
import { FontSelect, useFontFamilies } from "./TextLookControls";
import { fetchDimensionPreview } from "../api/dimensionPreview";
import { coordField, coordValue, useOriginTransforms } from "../state/originTransforms";

/** What the form needs from the editor it is open in. */
export interface DimensionHost {
  /** The dimension as the editor's state shows it. */
  dim: Dimension;
  /** The dimension as the verbs take it, which OK starts from (the fields the dialog has no control for ride through). */
  base: () => CmdDimension;
  units: LengthUnit;
  layers: string[];
  layerName: (layer: string) => string;
  /** OK: the edited dimension, one undo step; true when it was taken. */
  submit: (cmd: CmdDimension) => Promise<boolean>;
  remove: () => Promise<boolean>;
  close: () => void;
}

/** The board's dialog: on the board's dimension `state.dimensionEditId`, `edit_dimension` / `delete_dimension`. */
export function DimensionPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  if (state.pcbx.dimensionDrawDialog) return <DimensionDrawDialog />;
  const id = state.dimensionEditId;
  const dim = id ? api.dimensionById(id) : undefined;
  if (!id || !dim) return null;
  const layerSetup = state.board?.board_rules?.layer_setup;
  const host: DimensionHost = {
    dim,
    base: () => toCmdDimension(dim),
    units: state.units,
    layers: boardLayerList(state.board?.layers ?? [], layerSetup),
    layerName: (l) => layerDisplayName(layerSetup, l),
    submit: (cmd) => api.cmd({ op: "edit_dimension", id, dimension: cmd }),
    remove: () => api.cmd({ op: "delete_dimension", id }),
    close: () => dispatch({ type: "SET_DIMENSION_EDIT_ID", id: null }),
  };
  return <DimensionPropertiesForm host={host} />;
}

/**
 * E while Draw Dimension has a dimension going (`DRAWING_TOOL::DrawDimension`: `frame()->OnEditItemRequest( dimension ); dimension->Update(); ...; break;`): the dialog on the
 * dimension being drawn, as the preview shows it. The `break` leaves the tool's event loop once the dialog is closed, OK or Cancel, and the code after the loop deletes a
 * dimension that was not finished (`if( step != SET_ORIGIN ) delete dimension`): nothing is added, and the tool is done (25ec2335's `SCOPED_TOOL_PUSHER` pops it; 8303b2ad
 * leaves it on the stack, inactive). No undo step is made (KiCad's dialog pushes an "Edit Dimension Properties" commit for an item the board does not hold).
 */
function DimensionDrawDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const draw = state.drawState?.kind === "dimension" ? state.drawState.dim : null;
  const [dim, setDim] = useState<Dimension | null>(null);
  useEffect(() => {
    if (draw) void fetchDimensionPreview(draw.dim).then((d) => setDim(d));
    // the dimension as it was when E was pressed
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const end = () => {
    dispatch({ type: "PCBX", patch: { dimensionDrawDialog: false } });
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  };
  // the dimension went (Escape): the dialog has nothing to show
  useEffect(() => {
    if (!draw) dispatch({ type: "PCBX", patch: { dimensionDrawDialog: false } });
  }, [draw, dispatch]);
  if (!draw || !dim) return null;
  const layerSetup = state.board?.board_rules?.layer_setup;
  const host: DimensionHost = {
    dim,
    base: () => draw.dim,
    units: state.units,
    layers: boardLayerList(state.board?.layers ?? [], layerSetup),
    layerName: (l) => layerDisplayName(layerSetup, l),
    submit: async () => {
      end();
      return false;
    },
    remove: async () => {
      end();
      return false;
    },
    close: end,
  };
  return <DimensionPropertiesForm host={host} />;
}

export function DimensionPropertiesForm({ host }: { host: DimensionHost }) {
  const dim = host.dim;
  const unit: LengthUnit = host.units;

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
  // `m_textPosX` / `m_textPosY`: the text's place, editable in the Manual position mode only (`allowPositioning`); `ABS_X_COORD` / `ABS_Y_COORD`, so counted
  // from Origins & Axes' origin, each axis either way (UI-LEFT-2 PP9).
  const [textPosX, setTextPosX] = useState(0);
  const [textPosY, setTextPosY] = useState(0);
  const xf = useOriginTransforms();
  // A leader's "Text frame:" (`m_cbTextFrame`: None, Rectangle, Circle; `DIM_TEXT_BORDER`).
  const [textFrame, setTextFrame] = useState<DimensionTextBorder>("none");
  const [keepAligned, setKeepAligned] = useState(true);
  const [textAngle, setTextAngle] = useState(0);
  // The label's own looks in the dialog's units (Width, Height, Thickness) and the rest as they are (kicad-port/dimensionLabel.ts).
  const [looks, setLooks] = useState<DimensionLabelLooks>(() => labelLooksOf(dim));
  const families = useFontFamilies();
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
    setStartX(nmTo(dim.start[0], unit));
    setStartY(nmTo(dim.start[1], unit));
    setEndX(nmTo(dim.end[0], unit));
    setEndY(nmTo(dim.end[1], unit));
    setHeight(nmTo(dim.height ?? 0, unit));
    setHorizontal(dim.horizontal ?? true);
    setLeaderLength(nmTo(dim.leader_length ?? 0, unit));
    setPrefix(dim.prefix);
    setSuffix(dim.suffix);
    setOverrideOn(dim.override_text != null);
    setOverrideText(dim.override_text ?? "");
    setUnits(dim.units);
    setUnitsFormat(dim.units_format);
    setPrecision(dim.precision);
    setSuppressZeros(dim.suppress_trailing_zeros);
    setTextPosition(dim.text_position);
    setTextPosX(coordField(xf, dim.text_at[0], "abs_x", unit));
    setTextPosY(coordField(xf, dim.text_at[1], "abs_y", unit));
    setTextFrame(dim.text_frame ?? "none");
    setKeepAligned(dim.keep_text_aligned);
    setTextAngle(dim.text_angle / 1000);
    setLooks(labelLooksOf(dim));
    setStrokeWidth(nmTo(dim.stroke_width, unit));
    setArrowLength(nmTo(dim.arrow_length, unit));
    setExtensionOffset(nmTo(dim.extension_offset, unit));
    setExtensionHeight(nmTo(dim.extension_height, unit));
    setArrowDirection(dim.arrow_direction);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dim?.id]);

  const close = host.close;

  const kindObj: CmdDimensionKind =
    dim.kind === "aligned"
      ? { kind: "aligned", height: Math.round(nmFrom(height, unit)) }
      : dim.kind === "orthogonal"
        ? { kind: "orthogonal", height: Math.round(nmFrom(height, unit)), horizontal }
        : dim.kind === "radial"
          ? { kind: "radial", leader_length: Math.round(nmFrom(leaderLength, unit)) }
          : dim.kind === "leader"
            ? { kind: "leader" }
            : { kind: "center" };

  const save = async () => {
    setBusy(true);
    try {
      const cmd = host.base();
      cmd.layer = layer;
      cmd.kind = kindObj;
      cmd.start = { x: Math.round(nmFrom(startX, unit)), y: Math.round(nmFrom(startY, unit)) };
      cmd.end = { x: Math.round(nmFrom(endX, unit)), y: Math.round(nmFrom(endY, unit)) };
      cmd.prefix = prefix;
      cmd.suffix = suffix;
      cmd.override_text = overrideOn ? overrideText : null;
      cmd.units = units;
      cmd.units_format = unitsFormat;
      cmd.precision = precision;
      cmd.suppress_trailing_zeros = suppressZeros;
      cmd.text_position = textPosition;
      // `if( tpm == DIM_TEXT_POSITION::MANUAL ) aTarget->SetTextPos( pos )`: the place is the dialog's only in the manual mode.
      if (textPosition === "manual" && dim.kind !== "leader") cmd.text_pos = { x: Math.round(coordValue(xf, textPosX, "abs_x", unit)), y: Math.round(coordValue(xf, textPosY, "abs_y", unit)) };
      // `leader->SetTextBorder( static_cast<DIM_TEXT_BORDER>( m_cbTextFrame->GetSelection() ) )`
      if (dim.kind === "leader") {
        if (textFrame === "none") delete cmd.text_frame;
        else cmd.text_frame = textFrame;
      }
      cmd.keep_text_aligned = keepAligned;
      cmd.text_angle = Math.round(textAngle * 1000);
      // `SetTextWidth`, `SetTextHeight`, `SetTextThickness`, `SetFont` (a face the list lacks is kept: `HaveFontSelection`), `SetBold`, `SetItalic`,
      // `SetHorizJustify`, `SetMirrored`; keep-upright (no row here) rides through
      Object.assign(cmd, withLabelLooks({ ...cmd, layer }, looks));
      cmd.stroke_width = Math.round(nmFrom(strokeWidth, unit));
      cmd.arrow_length = Math.round(nmFrom(arrowLength, unit));
      cmd.extension_offset = Math.round(nmFrom(extensionOffset, unit));
      cmd.extension_height = Math.round(nmFrom(extensionHeight, unit));
      cmd.arrow_direction = arrowDirection;
      const ok = await host.submit(cmd);
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      const ok = await host.remove();
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const kindLabel = dim.kind[0]!.toUpperCase() + dim.kind.slice(1);
  const layers = host.layers;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" data-testid="dimension-properties" style={{ width: 480, maxHeight: "85vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()}>
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
                  {host.layerName(l)}
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
                <input type="text" data-testid="dim-prefix" value={prefix} onChange={(e) => setPrefix(e.target.value)} />
                <span>Suffix</span>
                <input type="text" value={suffix} onChange={(e) => setSuffix(e.target.value)} />
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <input type="checkbox" checked={overrideOn} onChange={(e) => setOverrideOn(e.target.checked)} /> Override value
                </label>
                <input type="text" data-testid="dim-override" value={overrideText} disabled={!overrideOn} onChange={(e) => setOverrideText(e.target.value)} placeholder={dim.text} />

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
                    <select value={precision} onChange={(e) => setPrecision(Number(e.target.value))}>
                      {DIM_PRECISION_CHOICES.map((c) => (
                        <option key={c.value} value={c.value}>{c.label}</option>
                      ))}
                    </select>
                    <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      <input type="checkbox" checked={suppressZeros} onChange={(e) => setSuppressZeros(e.target.checked)} /> Suppress trailing zeros
                    </label>
                    <span />
                  </>
                )}

                {dim.kind === "leader" && (
                  <>
                    <span title="Draw a shape around the leader text">Text frame:</span>
                    <select value={textFrame === "round_rect" ? "none" : textFrame} title="Draw a shape around the leader text" data-testid="dim-text-frame" onChange={(e) => setTextFrame(e.target.value as DimensionTextBorder)}>
                      <option value="none">None</option>
                      <option value="rectangle">Rectangle</option>
                      <option value="circle">Circle</option>
                    </select>
                  </>
                )}
                {/* A leader has no position mode (`m_lblTextPositionMode->Hide()`): its text is where it was put. */}
                {dim.kind !== "leader" && (
                  <>
                    <span title="Choose how to position the text relative to the dimension line">Position mode:</span>
                    <select value={textPosition} title="Choose how to position the text relative to the dimension line" data-testid="dim-text-position" onChange={(e) => setTextPosition(e.target.value as DimensionTextPosition)}>
                      <option value="outside">Outside</option>
                      <option value="inline">Inline</option>
                      <option value="manual">Manual</option>
                    </select>
                    <span>Position ({unit}) X / Y</span>
                    <span style={{ display: "flex", gap: 4 }}>
                      <input type="number" step="any" value={textPosX} disabled={textPosition !== "manual"} data-testid="dim-text-x" onChange={(e) => setTextPosX(Number(e.target.value))} style={{ width: "50%" }} />
                      <input type="number" step="any" value={textPosY} disabled={textPosition !== "manual"} data-testid="dim-text-y" onChange={(e) => setTextPosY(Number(e.target.value))} style={{ width: "50%" }} />
                    </span>
                  </>
                )}
                <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <input type="checkbox" checked={keepAligned} onChange={(e) => setKeepAligned(e.target.checked)} /> Keep text aligned with dimension
                </label>
                <span />
                <span>Text angle (&deg;, clockwise)</span>
                <input type="number" step="any" value={textAngle} disabled={keepAligned} onChange={(e) => setTextAngle(Number(e.target.value))} />
              </div>
              <div style={{ fontWeight: 600, fontSize: 11, marginTop: 10, marginBottom: 4 }}>Dimension Text</div>
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", rowGap: 6 }}>
                <span>Font:</span>
                <FontSelect
                  families={families}
                  current={looks.face}
                  value={looks.face ?? KICAD_FONT}
                  withDefault={false}
                  onChange={(f) => setLooks((l) => fontPicked(l, f === KICAD_FONT ? null : f))}
                />
                <span title="Text width">Width: ({unit})</span>
                <input type="number" step="any" title="Text width" data-testid="dim-text-width" value={nmTo(looks.widthNm, unit)} onChange={(e) => setLooks((l) => widthTyped(l, Math.round(nmFrom(Number(e.target.value), unit))))} />
                <span title="Text height">Height: ({unit})</span>
                <input type="number" step="any" title="Text height" data-testid="dim-text-height" value={nmTo(looks.heightNm, unit)} onChange={(e) => setLooks((l) => ({ ...l, heightNm: Math.round(nmFrom(Number(e.target.value), unit)) }))} />
                {/* `onFontSelected`: `m_textThickness.Show( IsStroke )` -- an outline face's weight is its own */}
                {!looks.face && (
                  <>
                    <span title="Text thickness">Thickness: ({unit})</span>
                    {/* KiCad 25ec2335's `m_autoTextThickness` (a check button with the link bitmap, "Adjust the text thickness"): checked, the thickness is the
                        automatic pen and greyed (`onAutoTextThickness`; UI-LEFT-3 P3-11) */}
                    <span style={{ display: "flex", gap: 3, alignItems: "center" }}>
                      <input type="number" step="any" title="Text thickness" data-testid="dim-text-thickness" disabled={looks.autoThickness} value={nmTo(looks.thicknessNm, unit)} onChange={(e) => setLooks((l) => thicknessTyped(l, Math.round(nmFrom(Number(e.target.value), unit))))} />
                      <TextToggle label="Adjust the text thickness" glyph={LINK_GLYPH} checked={looks.autoThickness} testId="dim-auto-thickness" onChange={(on) => setLooks((l) => autoThicknessToggled(l, on))} />
                    </span>
                  </>
                )}
                <span />
                <span style={{ display: "flex", gap: 4, alignItems: "center" }} data-testid="dim-text-style">
                  <TextToggle label="Bold" glyph={<b>B</b>} checked={looks.bold} testId="dim-bold" onChange={(b) => setLooks((l) => boldToggled(l, b))} />
                  <TextToggle label="Italic" glyph={<i>I</i>} checked={looks.italic} testId="dim-italic" onChange={(i) => setLooks((l) => ({ ...l, italic: i }))} />
                  <span style={{ width: 8 }} />
                  {(["left", "center", "right"] as TextJustify[]).map((j) => (
                    <TextToggle
                      key={j}
                      label={j === "left" ? "Align left" : j === "center" ? "Align horizontal center" : "Align right"}
                      glyph={j === "left" ? "\u21E4" : j === "center" ? "\u2194" : "\u21E5"}
                      checked={looks.hJustify === j}
                      testId={`dim-align-${j}`}
                      // `onAlignButton`: the three are radio buttons
                      onChange={() => setLooks((l) => ({ ...l, hJustify: j }))}
                    />
                  ))}
                  <span style={{ width: 8 }} />
                  <TextToggle label="Mirrored" glyph={"\u21C4"} checked={looks.mirrored} testId="dim-mirrored" onChange={(m) => setLooks((l) => ({ ...l, mirrored: m }))} />
                </span>
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
          <button className="primary" data-testid="dim-ok" onClick={save} disabled={busy}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** One of the "Dimension Text" box's bitmap toggle buttons (`BITMAP_BUTTON::SetIsCheckButton` / `SetIsRadioButton`): pressed while `checked`. */
/** KiCad's `BITMAPS::link`: two links of a chain. */
const LINK_GLYPH = (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
    <rect x="1.5" y="5.5" width="7" height="5" rx="2.5" />
    <rect x="7.5" y="5.5" width="7" height="5" rx="2.5" />
  </svg>
);

function TextToggle({ label, glyph, checked, onChange, testId }: { label: string; glyph: ReactNode; checked: boolean; onChange: (v: boolean) => void; testId: string }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={checked}
      data-testid={testId}
      onClick={() => onChange(!checked)}
      style={{ minWidth: 26, padding: "1px 4px", fontWeight: checked ? 700 : 400, background: checked ? "var(--accent-bg, #3a5f8f)" : undefined, color: checked ? "#fff" : undefined }}
    >
      {glyph}
    </button>
  );
}
