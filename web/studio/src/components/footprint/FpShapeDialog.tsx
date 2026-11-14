// The Footprint Editor's Shape Properties: `DIALOG_SHAPE_PROPERTIES` (pcbnew/dialogs/dialog_shape_properties.cpp) on a graphic of the open footprint
// (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`: `case PCB_SHAPE_T: ShowGraphicItemPropertiesDialog`), opened by a double click or E. The title is "%s Properties" with the
// shape's friendly name (`EDA_SHAPE::getFriendlyName`: Segment, Rectangle, Circle, Arc, Polygon, Curve); the geometry of a segment, a rectangle (its two corners), a
// circle (its centre and radius), an arc (its three points) and a curve (its four points) in the footprint's frame; the line width, the fill (the footprint editor's model
// fills solid or not at all: the hatch fills of the board's dialog are left out) and the layer. Validate is KiCad's with its words (`DIALOG_SHAPE_PROPERTIES::Validate`, the
// board dialog's `kicad-port/shapeFill.ts::fillWidthError` for the closed shapes). OK is one `fp_item set_graphic`, one undo step.
import { useEffect, useState } from "react";
import type { CmdShape } from "../../api/types";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { useFpLayerChoices } from "./useFpLayerChoices";
import { canFill, fillWidthError } from "../../kicad-port/shapeFill";
import { FILL_CHOICES, LINE_STYLE_CHOICES, fillChoiceOf, fillOfChoice, shapeLookOf } from "../../kicad-port/fpKeep";

type FillChoice = (typeof FILL_CHOICES)[number]["value"];

const mm = (nm: number) => String(Math.round(nm) / 1e6);
const nm = (v: string) => Math.round(Number(v) * 1e6) || 0;

/** `EDA_SHAPE::getFriendlyName`. */
export function shapeFriendlyName(kind: CmdShape["kind"]): string {
  switch (kind) {
    case "circle":
      return "Circle";
    case "arc":
      return "Arc";
    case "bezier":
      return "Curve";
    case "polygon":
      return "Polygon";
    case "rect":
      return "Rectangle";
    default:
      return "Segment";
  }
}

/** `DIALOG_SHAPE_PROPERTIES::Validate`'s per-shape line-width check: an arc and a segment always need a line, the closed shapes and a curve unless they are filled. */
export function shapeWidthError(kind: CmdShape["kind"], filled: boolean, widthNm: number): string | null {
  if (kind === "arc" || kind === "segment") return widthNm > 0 ? null : "Line width must be greater than zero.";
  if (kind === "bezier") return filled || widthNm > 0 ? null : "Line width must be greater than zero for an unfilled curve.";
  return fillWidthError(kind, filled ? "solid" : "none", widthNm);
}

/** The geometry rows of a shape: label and the point's key, in the dialog's order. */
function pointRows(s: CmdShape): { label: string; key: string }[] {
  switch (s.kind) {
    case "segment":
      return [
        { label: "Start", key: "start" },
        { label: "End", key: "end" },
      ];
    case "rect":
      return [
        { label: "Top Left", key: "start" },
        { label: "Bottom Right", key: "end" },
      ];
    case "circle":
      return [{ label: "Center", key: "center" }];
    case "arc":
      return [
        { label: "Start", key: "start" },
        { label: "Mid", key: "mid" },
        { label: "End", key: "end" },
      ];
    case "bezier":
      return [
        { label: "Start", key: "start" },
        { label: "Control Point 1", key: "c1" },
        { label: "Control Point 2", key: "c2" },
        { label: "End", key: "end" },
      ];
    default:
      return [];
  }
}

export function FpShapeDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const id = state.shapeDialogId;
  const shape = id ? state.footprint?.graphics.find((g) => g.id === id) : undefined;
  const [pts, setPts] = useState<Record<string, [string, string]>>({});
  const [radius, setRadius] = useState("0");
  const [width, setWidth] = useState("0");
  const [fill, setFill] = useState<FillChoice>("none");
  // "Line style:" (`m_lineStyleCombo`, UI-LEFT F5): kept by the graphic's id
  const [lineStyle, setLineStyle] = useState("solid");
  const [layer, setLayer] = useState("F.SilkS");
  // the editor board's enabled layers by their names (`SetNotAllowedLayerSet( AllLayersMask() & ~GetEnabledLayers() )`, `SetBoardFrame`; UI-LEFT-3 F3-1)
  const layerChoices = useFpLayerChoices();
  const [errors, setErrors] = useState<string[]>([]);

  useEffect(() => {
    if (!shape) return;
    const rec: Record<string, [string, string]> = {};
    for (const { key } of pointRows(shape)) {
      const p = (shape as unknown as Record<string, { x: number; y: number }>)[key]!;
      rec[key] = [mm(p.x), mm(p.y)];
    }
    setPts(rec);
    if (shape.kind === "circle") setRadius(mm(Math.hypot(shape.end.x - shape.center.x, shape.end.y - shape.center.y)));
    setWidth(mm(shape.stroke_width));
    const look = shapeLookOf(state.footprint, shape.id ?? "");
    setFill(fillChoiceOf(shape.filled, look.hatch));
    setLineStyle(look.line_style === "default" ? "solid" : look.line_style);
    setLayer(shape.layer);
    setErrors([]);
    // only when the dialog opens on a shape, not on every poll of the footprint
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || !shape) return null;
  const close = () => dispatch({ type: "SET_SHAPE_DIALOG_ID", id: null });
  const point = (key: string) => ({ x: nm(pts[key]?.[0] ?? "0"), y: nm(pts[key]?.[1] ?? "0") });
  const ok = async () => {
    const stroke = nm(width);
    const { filled, hatch } = canFill(shape.kind) ? fillOfChoice(fill) : { filled: false, hatch: null };
    const errs = [shapeWidthError(shape.kind, filled || hatch !== null, stroke)].filter((e): e is string => !!e);
    setErrors(errs);
    if (errs.length > 0) return; // `HTML_MESSAGE_BOX dlg( this, _( "Error List" ) )`
    let next: CmdShape = { ...shape, layer, stroke_width: stroke, filled };
    for (const { key } of pointRows(shape)) next = { ...next, [key]: point(key) } as CmdShape;
    if (next.kind === "circle") next = { ...next, end: { x: next.center.x + nm(radius), y: next.center.y } };
    // the line style and the hatch are kept by id (`LibraryFootprintKeep`): `m_item->SetLineStyle`, `SetFillMode`
    const was = shapeLookOf(state.footprint, id);
    const look = { line_style: lineStyle === "solid" && was.line_style === "default" ? "default" : lineStyle, hatch };
    if (await api.setGraphic(id, next, look)) close();
  };
  const setPt = (key: string, axis: 0 | 1, v: string) => setPts((p) => ({ ...p, [key]: (axis === 0 ? [v, p[key]?.[1] ?? "0"] : [p[key]?.[0] ?? "0", v]) as [string, string] }));
  const title = `${shapeFriendlyName(shape.kind)} Properties`;
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={title} data-testid="fp-shape-dialog">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
            {pointRows(shape).map(({ label, key }) => (
              <span key={key} style={{ display: "contents" }}>
                <span>{label}:</span>
                <span>
                  X <input type="number" step="any" value={pts[key]?.[0] ?? "0"} data-testid={`fp-shape-${key}-x`} onChange={(e) => setPt(key, 0, e.target.value)} style={{ width: 70 }} /> Y{" "}
                  <input type="number" step="any" value={pts[key]?.[1] ?? "0"} data-testid={`fp-shape-${key}-y`} onChange={(e) => setPt(key, 1, e.target.value)} style={{ width: 70 }} /> mm
                </span>
              </span>
            ))}
            {shape.kind === "circle" && (
              <>
                <span>Radius:</span>
                <span>
                  <input type="number" step="any" value={radius} data-testid="fp-shape-radius" onChange={(e) => setRadius(e.target.value)} style={{ width: 80 }} /> mm
                </span>
              </>
            )}
            <span>Line width:</span>
            <span>
              <input type="number" step="any" value={width} data-testid="fp-shape-width" onChange={(e) => setWidth(e.target.value)} style={{ width: 80 }} /> mm
            </span>
            <span>Line style:</span>
            <select value={lineStyle} data-testid="fp-shape-line-style" onChange={(e) => setLineStyle(e.target.value)}>
              {LINE_STYLE_CHOICES.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            {canFill(shape.kind) && (
              <>
                <span>Fill:</span>
                <select value={fill} data-testid="fp-shape-fill" onChange={(e) => setFill(e.target.value as FillChoice)}>
                  {FILL_CHOICES.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </>
            )}
            <span>Layer:</span>
            <select value={layer} data-testid="fp-shape-layer" onChange={(e) => setLayer(e.target.value)}>
              {(layerChoices.some((l) => l.value === layer) ? layerChoices : [{ value: layer, label: layer }, ...layerChoices]).map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
                </option>
              ))}
            </select>
          </div>
          {errors.length > 0 && (
            <ul style={{ color: "var(--error, #e66)", margin: "8px 0 0", paddingLeft: 18 }} data-testid="fp-shape-errors">
              {errors.map((e) => (
                <li key={e}>{e}</li>
              ))}
            </ul>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" data-testid="fp-shape-ok" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
