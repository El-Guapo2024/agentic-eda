// "E" on a selected track/via/shape (a zone gets its own full dialog --
// ZoneDialog.tsx, same one "Add Zone" uses). dialog_track_via_properties.cpp
// (track width; via diameter/drill) and pcb_shape's own properties
// (layer/line width/filled) are now editable through `set_track_width`/
// `edit_via`/`edit_shape` (GAPS.md #11) -- net, position and a via's
// layer span stay read-only, same as real KiCad's own dialogs (re-netting
// or re-spanning an existing via isn't a field edit there either, it's
// delete-and-redraw).
import { useEffect, useState } from "react";
import type { Cmd } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { canFill, fillOf, fillWidthError, SHAPE_FILL_CHOICES, type ShapeFill } from "../kicad-port/shapeFill";
import { LINE_ENDING_CHOICES, LINE_ENDING_HELP, NO_ENDING, type LineEnding, type LineEndingStyle } from "../kicad-port/lineEnding";
import type { CmdLineEnding } from "../api/pcbItemTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, formatXY, umFrom, umTo } from "../state/units";

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

  const [width, setWidth] = useState("");
  const [viaDiameter, setViaDiameter] = useState(0);
  const [viaDrill, setViaDrill] = useState(0);
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
    if (via) {
      setViaDiameter(umTo(via.d, units));
      setViaDrill(umTo(via.drill, units));
    }
    if (shape) {
      setShapeLayer(shape.layer);
      setShapeWidth(umTo(shape.stroke_width, units));
      setShapeFill(fillOf(shape));
      setShapeError("");
      setStartEnding(endingFields(shape.start_ending ?? NO_ENDING, units));
      setEndEnding(endingFields(shape.end_ending ?? NO_ENDING, units));
    }
    setWidth("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || (!track && !via && !shape)) return null;

  const title = track ? "Track Properties" : via ? "Via Properties" : "Shape Properties";
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
            {track && (
              <>
                <span>Net</span>
                <span>{track.net}</span>
                <span>Layer</span>
                <span>{track.layer}</span>
                <span>Width</span>
                <span>
                  <input placeholder={formatLength(track.width, units)} value={width} onChange={(e) => setWidth(e.target.value)} style={{ width: 90 }} />
                </span>
              </>
            )}
            {via && (
              <>
                <span>Net</span>
                <span>{via.net}</span>
                <span>Position</span>
                <span>{formatXY(via.x, via.y, units)}</span>
                <span>Diameter</span>
                <span>
                  <input type="number" step="any" value={viaDiameter} onChange={(e) => setViaDiameter(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                <span>Drill</span>
                <span>
                  <input type="number" step="any" value={viaDrill} onChange={(e) => setViaDrill(Number(e.target.value) || 0)} style={{ width: 90 }} /> {units}
                </span>
                <span>Layers</span>
                <span>
                  {via.from} - {via.to}
                </span>
              </>
            )}
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
          {track && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>Net and layer have no edit command yet -- delete and redraw to change either.</p>}
          {via && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>Net and layer span have no edit command yet, same as real KiCad's own dialog -- delete and redraw to change either.</p>}
        </div>
        <div className="dialog-footer">
          {track && (
            <button
              disabled={!width.trim()}
              onClick={() => {
                const um = Math.round(umFrom(Number(width), units));
                if (Number.isFinite(um) && um > 0) api.cmd({ op: "set_track_width", id, width: um });
                setWidth("");
              }}
            >
              Apply Width
            </button>
          )}
          {via && (
            <button
              className="primary"
              onClick={async () => {
                const diameter = Math.round(umFrom(viaDiameter, units));
                const drill = Math.round(umFrom(viaDrill, units));
                if (await api.cmd({ op: "edit_via", id, diameter, drill })) close();
              }}
            >
              Apply
            </button>
          )}
          {shape && (
            <button
              className="primary"
              onClick={async () => {
                const stroke_width = Math.round(umFrom(shapeWidth, units));
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
          <button
            onClick={() => {
              if (track) api.cmd({ op: "delete_track", id });
              else if (via) api.cmd({ op: "delete_via", id });
              else if (shape) api.cmd({ op: "delete_shape", id });
              close();
            }}
          >
            Delete
          </button>
          <button onClick={close}>{via || shape ? "Cancel" : "Close"}</button>
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

function endingFields(e: LineEnding, units: Parameters<typeof umTo>[1]): EndingFields {
  return { style: e.style, length: umTo(e.length, units), width: umTo(e.width, units), thickness: umTo(e.stroke_width, units) };
}

/** `TransferDataFromWindow`: the style, and each size at 0 or more (`std::max( 0, .. )`). */
function endingCmd(f: EndingFields, units: Parameters<typeof umFrom>[1]): CmdLineEnding {
  const um = (v: number) => Math.max(0, Math.round(umFrom(v, units)));
  return { style: f.style, length_um: um(f.length), width_um: um(f.width), stroke_width_um: um(f.thickness) };
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
      <span>{label} shape</span>
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
