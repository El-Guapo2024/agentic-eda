// "E" on a selected track/via/shape (a zone gets its own full dialog --
// ZoneDialog.tsx, same one "Add Zone" uses). A track or a via opens
// `DIALOG_TRACK_VIA_PROPERTIES` on every selected track and via when the
// selection holds only those (`EDIT_TOOL::Properties`' `OnlyTypes( { PCB_TRACE_T,
// PCB_ARC_T, PCB_VIA_T } )`): TrackViaPropertiesDialog.tsx. A shape gets its
// layer, line width, fill and endings here (`edit_shape`); its geometry and net
// are the Properties panel's (not this dialog's yet).
import { useEffect, useState } from "react";
import type { Cmd } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { canFill, fillOf, fillWidthError, SHAPE_FILL_CHOICES, type ShapeFill } from "../kicad-port/shapeFill";
import { LINE_ENDING_CHOICES, LINE_ENDING_HELP, NO_ENDING, type LineEnding, type LineEndingStyle } from "../kicad-port/lineEnding";
import type { CmdLineEnding } from "../api/pcbItemTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { nmFrom, nmTo } from "../state/units";
import { TrackViaPropertiesDialog } from "./TrackViaPropertiesDialog";

export function ItemPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const id = state.itemPropertiesId;
  const close = () => dispatch({ type: "SET_ITEM_PROPERTIES_ID", id: null });

  const track = id ? api.trackById(id) : undefined;
  const via = id ? api.viaById(id) : undefined;
  const shape = id ? api.shapeById(id) : undefined;
  const units = state.units;

  const [shapeLayer, setShapeLayer] = useState("");
  const [shapeWidth, setShapeWidth] = useState(0);
  const [shapeFill, setShapeFill] = useState<ShapeFill>("none");
  const [shapeError, setShapeError] = useState("");
  // The line endings of a line, an arc or a curve (KiCad 10's Start and End rows of DIALOG_SHAPE_PROPERTIES): the style and the three sizes, in the dialog's units.
  const [startEnding, setStartEnding] = useState<EndingFields>(endingFields(NO_ENDING, units));
  const [endEnding, setEndEnding] = useState<EndingFields>(endingFields(NO_ENDING, units));

  // Populate the editable copies whenever the dialog opens on a
  // (possibly different) item -- not every render, or a mid-edit
  // keystroke would be clobbered by the next /api/state poll.
  useEffect(() => {
    if (shape) {
      setShapeLayer(shape.layer);
      setShapeWidth(nmTo(shape.stroke_width, units));
      setShapeFill(fillOf(shape));
      setShapeError("");
      setStartEnding(endingFields(shape.start_ending ?? NO_ENDING, units));
      setEndEnding(endingFields(shape.end_ending ?? NO_ENDING, units));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || (!track && !via && !shape)) return null;
  if ((track || via) && state.board) {
    // the whole selection when it is tracks and vias only and holds this one, else this one
    const sel = [...state.selection];
    const onlyTrackVia = sel.length > 0 && sel.includes(id) && sel.every((s) => api.trackById(s) || api.viaById(s));
    return <TrackViaPropertiesDialog key={id} board={state.board} ids={onlyTrackVia ? sel : [id]} units={units} onClose={close} />;
  }

  const title = "Shape Properties";
  // The layers the board has (Board Setup > Board Editor Layers); a shape on a layer the board has since turned off keeps its own.
  const layerSetup = state.board?.board_rules?.layer_setup;
  const layerOptions = boardLayerList(state.board?.layers ?? [], layerSetup);
  if (shape && !layerOptions.includes(shapeLayer || shape.layer)) layerOptions.push(shapeLayer || shape.layer);

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            {shape && (
              <>
                <span>Kind</span>
                <span>{shape.kind}</span>
                <span>Layer</span>
                <span>
                  <select value={shapeLayer} onChange={(e) => setShapeLayer(e.target.value)}>
                    {layerOptions.map((l) => (
                      <option key={l} value={l}>
                        {layerDisplayName(layerSetup, l)}
                      </option>
                    ))}
                  </select>
                </span>
                <span>Line width</span>
                <span>
                  <input type="number" step="any" value={shapeWidth} onChange={(e) => setShapeWidth(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                {isOpenKind(shape.kind) && (
                  <>
                    <EndingRows label="Start" value={startEnding} onChange={setStartEnding} units={units} testid="start" />
                    <EndingRows label="End" value={endEnding} onChange={setEndEnding} units={units} testid="end" />
                  </>
                )}
                {canFill(shape.kind) && (
                  <>
                    <span>Fill</span>
                    <span>
                      <select value={shapeFill} onChange={(e) => setShapeFill(e.target.value as ShapeFill)} data-testid="shape-fill">
                        {SHAPE_FILL_CHOICES.map((c) => (
                          <option key={c.value} value={c.value}>
                            {c.label}
                          </option>
                        ))}
                      </select>
                    </span>
                  </>
                )}
              </>
            )}
          </div>
          {shape && isOpenKind(shape.kind) && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, fontStyle: "italic", marginTop: 10 }}>{LINE_ENDING_HELP}</p>}
          {shape && shapeError && (
            <p style={{ color: "var(--chrome-error, #d33)", fontSize: 11, marginTop: 10 }} data-testid="shape-error">
              {shapeError}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          {shape && (
            <button
              className="primary"
              onClick={async () => {
                const stroke_width = Math.round(nmFrom(shapeWidth, units));
                // `DIALOG_SHAPE_PROPERTIES::Validate`: a fill that is not solid needs a line to be seen by.
                const error = canFill(shape.kind) ? fillWidthError(shape.kind, shapeFill, stroke_width) : null;
                setShapeError(error ?? "");
                if (error) return;
                // The layer and the line width as before, then the fill as one of the five choices: one undo step.
                const cmds: Cmd[] = [{ op: "edit_shape", id, layer: shapeLayer, stroke_width, filled: shapeFill === "solid" }];
                if (canFill(shape.kind) && shapeFill !== fillOf(shape)) cmds.push({ op: "pcb_item", verb: "set_shape_style", ids: [id], fill: shapeFill });
                // The endings of an open shape, as `TransferDataFromWindow` sets them (`SetStartEndingStyle`, the sizes at 0 or more), when they changed.
                if (isOpenKind(shape.kind)) {
                  const start = endingCmd(startEnding, units);
                  const end = endingCmd(endEnding, units);
                  const changed = (a: CmdLineEnding, b: LineEnding | undefined) => JSON.stringify(a) !== JSON.stringify(endingCmd(endingFields(b ?? NO_ENDING, units), units));
                  if (changed(start, shape.start_ending) || changed(end, shape.end_ending)) cmds.push({ op: "pcb_item", verb: "set_shape_style", ids: [id], start_ending: start, end_ending: end });
                }
                if (await api.cmdBatch(cmds)) close();
              }}
            >
              Apply
            </button>
          )}
          <button onClick={close}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

/** The kinds a line ending goes on (`isOpenShape`: a line, an arc, a curve; this board's polygons are closed). */
function isOpenKind(kind: string): boolean {
  return kind === "segment" || kind === "arc" || kind === "bezier";
}

/** One side's controls as the dialog holds them: the style choice and Length, Width and Thickness in the dialog's units. */
interface EndingFields {
  style: LineEndingStyle;
  length: number;
  width: number;
  thickness: number;
}

function endingFields(e: LineEnding, units: Parameters<typeof nmTo>[1]): EndingFields {
  return { style: e.style, length: nmTo(e.length, units), width: nmTo(e.width, units), thickness: nmTo(e.stroke_width, units) };
}

/** `TransferDataFromWindow`: the style, and each size at 0 or more (`std::max( 0, .. )`). */
function endingCmd(f: EndingFields, units: Parameters<typeof nmFrom>[1]): CmdLineEnding {
  const um = (v: number) => Math.max(0, Math.round(nmFrom(v, units)));
  return { style: f.style, length_nm: um(f.length), width_nm: um(f.width), stroke_width_nm: um(f.thickness) };
}

/** The Start (or End) shape choice and its Length, Width and Thickness (`DIALOG_SHAPE_PROPERTIES::createLineEndingControls`). */
function EndingRows({ label, value, onChange, units, testid }: { label: string; value: EndingFields; onChange: (f: EndingFields) => void; units: string; testid: string }) {
  const num = (k: "length" | "width" | "thickness", caption: string) => (
    <>
      <span>{caption}</span>
      <span>
        <input type="number" step="any" min={0} value={value[k]} onChange={(e) => onChange({ ...value, [k]: Number(e.target.value) || 0 })} style={{ width: 90 }} data-testid={`${testid}-ending-${k}`} /> {units}
      </span>
    </>
  );
  return (
    <>
      <span>{label} Shape:</span>
      <span>
        <select value={value.style} onChange={(e) => onChange({ ...value, style: e.target.value as LineEndingStyle })} data-testid={`${testid}-ending-style`}>
          {LINE_ENDING_CHOICES.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
      </span>
      {num("length", "Length:")}
      {num("width", "Width:")}
      {num("thickness", "Thickness:")}
    </>
  );
}
