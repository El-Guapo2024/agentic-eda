// The dialogs of the board items GAPS.md item 24 (package P16) added. Text Box Properties (`DIALOG_TEXTBOX_PROPERTIES`, pcbnew/dialogs/dialog_textbox_properties.cpp at KiCad 8303b2ad) is here:
// opened after the Draw Text Box tool's second click -- the box is added by OK and gone with Cancel -- or on a box that is there (E, a double click, Properties in the context menu). It
// has the look of the Symbol Editor's Text Box Properties (components/symbol/SymbolTextBoxDialog.tsx), with the layer, the glyph width and height, the pen (automatic or not), the
// orientation (which turns the box about its position, `PCB_TEXTBOX::SetTextAngle`), the border and its style, the margins, knockout and the lock.
import { useEffect, useRef, useState } from "react";
import { greyscalePng } from "../actions/imageFiles";
import { applyCommands } from "../actions/pcbSweepKit";
import type { BoardRefImage, BoardTextBox, TextBoxHAlign, TextBoxVAlign } from "../api/pcbItemTypes";
import type { SchLineStyle } from "../api/schEditTypes";
import type { Cmd } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { autoPenUm, clampPenUm, escapeQuoted, grownToFit, textBoxToCmd, TEXT_MAX_SIZE_UM, TEXT_MIN_SIZE_UM, unescapeQuoted, withTextAngle } from "../kicad-port/pcbTextBox";
import { checkImageScale, imageSizeUm, pcbImageUrl, scaleForHeightUm, scaleForWidthUm, scalePpmOf, scaleText } from "../kicad-port/refImage";
import { closeRefImageDialog, closeTextBoxDialog, usePcbItemDialogs } from "../state/pcbItemDialogs";
import { ImageEditorPanel } from "./ImageEditorPanel";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { measureStrokeText } from "./text/strokeFont";

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const H_ALIGNS: TextBoxHAlign[] = ["left", "center", "right"];
const V_ALIGNS: TextBoxVAlign[] = ["top", "center", "bottom"];
/** `lineTypeNames`: the border styles of the dialog's combo box. */
const BORDER_STYLES: Array<{ value: SchLineStyle; label: string }> = [
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-Dot" },
  { value: "dash_dot_dot", label: "Dash-Dot-Dot" },
];

const mm = (um: number): string => String(Math.round(um) / 1000);
const um = (s: string): number => Math.round(Number(s) * 1000);

interface Form {
  text: string;
  layer: string;
  width: string;
  height: string;
  thickness: string;
  autoThickness: boolean;
  bold: boolean;
  italic: boolean;
  h: TextBoxHAlign;
  v: TextBoxVAlign;
  mirror: boolean;
  /** Degrees. */
  angle: string;
  border: boolean;
  borderWidth: string;
  borderStyle: SchLineStyle;
  margins: [string, string, string, string];
  knockout: boolean;
  locked: boolean;
}

/** `TransferDataToWindow`. */
function formOf(b: BoardTextBox, locked: boolean): Form {
  const angle = b.angle > 180_000 ? b.angle - 360_000 : b.angle; // `m_orientation.SetAngleValue( orientation.Normalize180() )`
  return {
    text: unescapeQuoted(b.text),
    layer: b.layer,
    width: mm(b.width),
    height: mm(b.size),
    thickness: mm(b.thickness),
    autoThickness: b.thickness_auto,
    bold: b.bold,
    italic: b.italic,
    h: b.halign,
    v: b.valign,
    mirror: b.mirror,
    angle: String(angle / 1000),
    border: b.border,
    borderWidth: mm(b.border ? b.stroke_width : 0),
    borderStyle: b.line_style === "default" ? "solid" : b.line_style,
    margins: [mm(b.margins[0]), mm(b.margins[1]), mm(b.margins[2]), mm(b.margins[3])],
    knockout: b.knockout,
    locked,
  };
}

export function PcbItemDialogs() {
  return (
    <>
      <PcbTextBoxDialog />
      <PcbRefImageDialog />
    </>
  );
}

export function PcbTextBoxDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { textBox: req } = usePcbItemDialogs();
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);

  // the form starts from the box the dialog was opened on, and keeps what is typed through the state's refreshes
  useEffect(() => {
    if (!req) {
      setForm(null);
      return;
    }
    setForm(formOf(req.box, req.id ? (state.board?.locked ?? []).includes(req.id) : false));
    setError(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !form || !state.board) return null;
  const board = state.board;
  const set = <K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = () => closeTextBoxDialog();

  const ok = async () => {
    // `m_textWidth.Validate( minSize, maxSize )`
    const [w, h] = [um(form.width), um(form.height)];
    for (const [label, v] of [["width", w], ["height", h]] as const) {
      if (!Number.isFinite(v) || v < TEXT_MIN_SIZE_UM || v > TEXT_MAX_SIZE_UM) return setError(`The text ${label} must be between 0.001 mm and 250 mm.`);
    }
    const prev = req.box;
    let pen = form.autoThickness ? 0 : um(form.thickness);
    if (!Number.isFinite(pen) || pen < 0) return setError("The text thickness must not be negative.");
    if (pen > 0) {
      const max = clampPenUm(pen, Math.min(w, h));
      if (pen > max) {
        dispatch({ type: "TOAST", message: "The text thickness is too large for the text size. It will be clamped.", kind: "info" });
        pen = max;
      }
    }
    const margins = form.margins.map(um);
    if (margins.some((m) => !Number.isFinite(m) || m < 0)) return setError("The margins must not be negative.");
    const borderWidth = um(form.borderWidth);
    if (form.border && (!Number.isFinite(borderWidth) || borderWidth < 0)) return setError("The border width must not be negative.");
    const angle = Math.round(Number(form.angle) * 1000);
    if (!Number.isFinite(angle)) return setError("The orientation must be a number of degrees.");
    let box: BoardTextBox = {
      ...prev,
      text: escapeQuoted(form.text),
      layer: form.layer,
      size: h,
      width: w,
      thickness: pen > 0 ? pen : autoPenUm(Math.min(w, h), form.bold),
      thickness_auto: pen === 0,
      bold: form.bold,
      italic: form.italic,
      halign: form.h,
      valign: form.v,
      mirror: form.mirror,
      border: form.border,
      stroke_width: form.border ? borderWidth : prev.stroke_width,
      line_style: form.borderStyle,
      margins: [margins[0]!, margins[1]!, margins[2]!, margins[3]!],
      knockout: form.knockout,
    };
    // `SetTextAngle`: the box turns about its position
    box = withTextAngle(box, angle);
    // the box is as big as its text needs (`GetMinSize`)
    box = grownToFit(box, measureStrokeText);
    const cmd = textBoxToCmd(box);
    const cmds: Cmd[] = req.id ? [{ op: "pcb_item", verb: "edit_text_box", id: req.id, text_box: cmd }] : [{ op: "pcb_item", verb: "add_text_box", text_box: cmd }];
    if (req.id && form.locked !== (board.locked ?? []).includes(req.id)) cmds.push({ op: "set_locked", ids: [req.id], locked: form.locked });
    const done = await applyCommands(api, dispatch, cmds);
    if (!done) return setError("The board refused the text box.");
    if (!req.id && form.locked) {
      // a new box takes its lock after it is added: it is the one new item the selection now holds
      const created = [...api.getState().selection].filter((id) => api.getState().board?.drawings?.text_boxes?.some((t) => t.id === id));
      if (created.length > 0) await api.cmd({ op: "set_locked", ids: created, locked: true });
    }
    close();
  };

  const radio = (name: string, testid: string, checked: boolean, label: string, onChange: () => void) => (
    <label style={{ marginRight: 10 }}>
      <input type="radio" name={name} checked={checked} onChange={onChange} data-testid={testid} /> {label}
    </label>
  );
  const num = (key: "width" | "height" | "thickness" | "borderWidth" | "angle", testid: string, extra: Partial<React.InputHTMLAttributes<HTMLInputElement>> = {}) => (
    <input type="number" step="0.01" value={form[key]} onChange={(e) => set(key, e.target.value)} style={{ width: 80 }} data-testid={testid} {...extra} />
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Text Box Properties" data-testid="pcb-text-box-properties">
        <div className="dialog-header">
          <span>Text Box Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <label style={{ display: "block", marginBottom: 4 }}>Text</label>
          <textarea
            ref={ref}
            value={form.text}
            rows={4}
            style={{ width: "100%", boxSizing: "border-box" }}
            data-testid="ptb-text"
            onChange={(e) => set("text", e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void ok();
              if (e.key === "Escape") close();
            }}
          />
          <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr", marginTop: 10 }}>
            <span>Layer</span>
            <select value={form.layer} onChange={(e) => set("layer", e.target.value)} data-testid="ptb-layer">
              {boardLayerList(board.layers ?? [], board.board_rules?.layer_setup).map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(board.board_rules?.layer_setup, l)}
                </option>
              ))}
            </select>
            <span>Text width (mm)</span>
            {num("width", "ptb-width")}
            <span>Text height (mm)</span>
            {num("height", "ptb-height")}
            <span>Text thickness (mm)</span>
            <span>
              {num("thickness", "ptb-thickness", { disabled: form.autoThickness })}
              <label style={{ marginLeft: 12 }}>
                <input type="checkbox" checked={form.autoThickness} onChange={(e) => set("autoThickness", e.target.checked)} data-testid="ptb-auto-thickness" /> Auto
              </label>
            </span>
            <span>Style</span>
            <span>
              <label>
                <input type="checkbox" checked={form.bold} onChange={(e) => set("bold", e.target.checked)} data-testid="ptb-bold" /> Bold
              </label>
              <label style={{ marginLeft: 12 }}>
                <input type="checkbox" checked={form.italic} onChange={(e) => set("italic", e.target.checked)} data-testid="ptb-italic" /> Italic
              </label>
              <label style={{ marginLeft: 12 }}>
                <input type="checkbox" checked={form.mirror} onChange={(e) => set("mirror", e.target.checked)} data-testid="ptb-mirror" /> Mirrored
              </label>
            </span>
            <span>Horizontal alignment</span>
            <span>{H_ALIGNS.map((a) => radio("ptb-h", `ptb-h-${a}`, form.h === a, cap(a), () => set("h", a)))}</span>
            <span>Vertical alignment</span>
            <span>{V_ALIGNS.map((a) => radio("ptb-v", `ptb-v-${a}`, form.v === a, cap(a), () => set("v", a)))}</span>
            <span>Orientation (deg)</span>
            {num("angle", "ptb-angle", { step: 1 })}
            <span>Border</span>
            <span>
              <input type="checkbox" checked={form.border} onChange={(e) => set("border", e.target.checked)} data-testid="ptb-border" aria-label="Border" />
              <span style={{ marginLeft: 8, opacity: form.border ? 1 : 0.5 }}>
                Width: {num("borderWidth", "ptb-border-width", { disabled: !form.border, min: 0 })} mm{" "}
                <select value={form.borderStyle} disabled={!form.border} onChange={(e) => set("borderStyle", e.target.value as SchLineStyle)} data-testid="ptb-border-style">
                  {BORDER_STYLES.map((s) => (
                    <option key={s.value} value={s.value}>
                      {s.label}
                    </option>
                  ))}
                </select>
              </span>
            </span>
            <span>Margins (mm)</span>
            <span>
              {(["Left", "Top", "Right", "Bottom"] as const).map((side, i) => (
                <label key={side} style={{ marginRight: 8 }}>
                  {side}{" "}
                  <input
                    type="number"
                    step="0.01"
                    min={0}
                    value={form.margins[i]}
                    style={{ width: 62 }}
                    data-testid={`ptb-margin-${side.toLowerCase()}`}
                    onChange={(e) => set("margins", form.margins.map((m, j) => (j === i ? e.target.value : m)) as Form["margins"])}
                  />
                </label>
              ))}
            </span>
            <span>Knockout</span>
            <label>
              <input type="checkbox" checked={form.knockout} onChange={(e) => set("knockout", e.target.checked)} data-testid="ptb-knockout" /> Fill the box and cut the text out of it
            </label>
            <span>Locked</span>
            <label>
              <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="ptb-locked" />
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="ptb-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="ptb-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="ptb-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------------------------------------------------------------- Reference Image Properties

interface RefImageForm {
  x: string;
  y: string;
  layer: string;
  locked: boolean;
  scale: string;
  width: string;
  height: string;
}

function refImageFormOf(i: BoardRefImage, locked: boolean): RefImageForm {
  return { x: mm(i.x), y: mm(i.y), layer: i.layer, locked, scale: scaleText(i.scale_ppm), width: mm(i.w), height: mm(i.h) };
}

/**
 * `DIALOG_REFERENCE_IMAGE_PROPERTIES` (pcbnew/dialogs/dialog_reference_image_properties.cpp): the position, the layer, the lock, the size and, in the panel the schematic's Image Properties
 * shares, the scale and Convert to Greyscale. Width, height and scale move together (`onWidthChanged`, `onHeightChanged`, `onScaleChanged`); OK checks the scale (`CheckValues`) and applies
 * everything as one step, Cancel leaves the image as it was.
 */
export function PcbRefImageDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { refImage: req } = usePcbItemDialogs();
  const [form, setForm] = useState<RefImageForm | null>(null);
  const [grey, setGrey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const image = req ? (state.board?.drawings?.images ?? []).find((i) => i.id === req.id) : undefined;

  useEffect(() => {
    if (!req || !image) {
      setForm(null);
      setGrey(null);
      return;
    }
    setForm(refImageFormOf(image, (state.board?.locked ?? []).includes(image.id)));
    setGrey(null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !image || !form || !state.board) return null;
  const board = state.board;
  const set = <K extends keyof RefImageForm>(key: K, value: RefImageForm[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = () => closeRefImageDialog();
  const sizeAt = (scalePpm: number): [number, number] => imageSizeUm(image.px, image.ppi, scalePpm);
  /** The scale was typed: the size follows (`onScaleChanged`). */
  const onScale = (text: string) => {
    const ppm = scalePpmOf(text);
    setForm((f) => {
      if (!f) return f;
      if (!(ppm > 0)) return { ...f, scale: text };
      const [w, h] = sizeAt(ppm);
      return { ...f, scale: text, width: mm(w), height: mm(h) };
    });
    setError(null);
  };
  /** The width or the height was typed: the scale follows, and the other side (`onWidthChanged`, `onHeightChanged`). */
  const onSide = (side: "width" | "height", text: string) => {
    const want = um(text);
    setForm((f) => {
      if (!f) return f;
      if (!(want > 0)) return { ...f, [side]: text };
      const ppm = side === "width" ? scaleForWidthUm(image.px, image.ppi, want) : scaleForHeightUm(image.px, image.ppi, want);
      const [w, h] = sizeAt(ppm);
      return { ...f, scale: scaleText(ppm), width: side === "width" ? text : mm(w), height: side === "height" ? text : mm(h) };
    });
    setError(null);
  };
  const greyscale = async () => {
    setBusy(true);
    try {
      setGrey(await greyscalePng(pcbImageUrl(image.id, image.hash), image.ppi));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const ok = async () => {
    const [x, y] = [um(form.x), um(form.y)];
    if (!Number.isFinite(x) || !Number.isFinite(y)) return setError("The position must be numbers.");
    const scalePpm = scalePpmOf(form.scale);
    const check = checkImageScale(image.px, scalePpm);
    if (!check.ok) return setError(check.error);
    if (check.confirm && !window.confirm(check.confirm)) return;
    const cmds: Cmd[] = [];
    if (x !== image.x || y !== image.y || form.layer !== image.layer || scalePpm !== image.scale_ppm) {
      cmds.push({ op: "pcb_item", verb: "edit_ref_image", id: image.id, layer: form.layer, at: { x, y }, scale_ppm: scalePpm });
    }
    if (grey) cmds.push({ op: "pcb_item", verb: "set_ref_image_data", id: image.id, data: grey });
    if (form.locked !== (board.locked ?? []).includes(image.id)) cmds.push({ op: "set_locked", ids: [image.id], locked: form.locked });
    if (cmds.length > 0 && !(await applyCommands(api, dispatch, cmds, [], null, { keep: true }))) return setError("The board refused the image.");
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Reference Image Properties" data-testid="pcb-ref-image-properties">
        <div className="dialog-header">
          <span>Reference Image Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
            <span>Position X (mm)</span>
            <input type="number" step="0.01" value={form.x} onChange={(e) => set("x", e.target.value)} style={{ width: 100 }} data-testid="pri-x" />
            <span>Position Y (mm)</span>
            <input type="number" step="0.01" value={form.y} onChange={(e) => set("y", e.target.value)} style={{ width: 100 }} data-testid="pri-y" />
            <span>Width (mm)</span>
            <input type="number" step="0.01" min="0" value={form.width} onChange={(e) => onSide("width", e.target.value)} style={{ width: 100 }} data-testid="pri-width" />
            <span>Height (mm)</span>
            <input type="number" step="0.01" min="0" value={form.height} onChange={(e) => onSide("height", e.target.value)} style={{ width: 100 }} data-testid="pri-height" />
            <span>Layer</span>
            <select value={form.layer} onChange={(e) => set("layer", e.target.value)} data-testid="pri-layer">
              {boardLayerList(board.layers ?? [], board.board_rules?.layer_setup).map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(board.board_rules?.layer_setup, l)}
                </option>
              ))}
            </select>
            <span>Locked</span>
            <label>
              <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="pri-locked" />
            </label>
          </div>
          <div style={{ marginTop: 10 }}>
            <ImageEditorPanel previewUrl={grey ? `data:image/png;base64,${grey}` : pcbImageUrl(image.id, image.hash)} ppi={image.ppi} scale={form.scale} onScale={onScale} onGreyscale={() => void greyscale()} busy={busy} testid="pri" />
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="pri-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="pri-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="pri-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
