// The dialogs of the board items GAPS.md item 24 (package P16) added (Barcode Properties is components/BarcodeDialog.tsx). Text Box Properties (`DIALOG_TEXTBOX_PROPERTIES`, pcbnew/dialogs/dialog_textbox_properties.cpp at KiCad 8303b2ad) is here:
// opened after the Draw Text Box tool's second click -- the box is added by OK and gone with Cancel -- or on a box that is there (E, a double click, Properties in the context menu). It
// has the look of the Symbol Editor's Text Box Properties (components/symbol/SymbolTextBoxDialog.tsx), with the layer, the glyph width and height, the pen (automatic or not), the
// orientation (which turns the box about its position, `PCB_TEXTBOX::SetTextAngle`), the border and its style, the margins, knockout and the lock.
import { useEffect, useRef, useState } from "react";
import { greyscalePng } from "../actions/imageFiles";
import { applyCommands } from "../actions/pcbSweepKit";
import type { BoardRefImage, BoardTextBox, CmdTextBox, TextBoxHAlign, TextBoxVAlign } from "../api/pcbItemTypes";
import type { SchLineStyle } from "../api/schEditTypes";
import type { Cmd } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { autoPenNm, clampPenNm, escapeQuoted, grownToFit, textBoxToCmd, TEXT_MAX_SIZE_NM, TEXT_MIN_SIZE_NM, unescapeQuoted, withTextAngle } from "../kicad-port/pcbTextBox";
import { checkImageScale, imageSizeNm, pcbImageUrl, scaleForHeightNm, scaleForWidthNm, scalePpmOf, scaleText } from "../kicad-port/refImage";
import { closeRefImageDialog, closeTextBoxDialog, usePcbItemDialogs } from "../state/pcbItemDialogs";
import { ImageEditorPanel } from "./ImageEditorPanel";
import { PcbBarcodeDialog } from "./BarcodeDialog";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { measureStrokeText } from "./text/strokeFont";
import { IDENTITY_TRANSFORMS, pcbTransformsOf } from "../state/originTransforms";
import { fromDisplay, toDisplay, type CoordType, type OriginTransforms } from "../kicad-port/originTransforms";

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

const mm = (um: number): string => String(Math.round(um) / 1e6);
const um = (s: string): number => Math.round(Number(s) * 1e6);

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
      <PcbBarcodeDialog />
    </>
  );
}

/**
 * What Text Box Properties needs from the editor it is open in (the board's, below, or the Footprint Editor's, components/footprint/FpItemDialogs.tsx): `DIALOG_TEXTBOX_PROPERTIES`
 * is one dialog for both frames (`ShowTextBoxPropertiesDialog`).
 */
export interface TextBoxHost {
  /** A new value opens the form afresh on `box`. */
  openKey: unknown;
  box: BoardTextBox;
  /** The box's lock; null where the editor keeps none (no Locked row). */
  locked: boolean | null;
  layers: string[];
  layerName: (layer: string) => string;
  toast: (message: string) => void;
  /** OK: the box as the verbs take it and the lock asked for, one undo step; null when it was taken, else why not. */
  submit: (box: CmdTextBox, locked: boolean) => Promise<string | null>;
  close: () => void;
}

/** The board's dialog: `pcb_item` `add_text_box` for the Draw Text Box tool's box, `edit_text_box` (and the lock) for one the board has. */
export function PcbTextBoxDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { textBox: req } = usePcbItemDialogs();
  if (!req || !state.board) return null;
  const board = state.board;
  const host: TextBoxHost = {
    openKey: req,
    box: req.box,
    locked: req.id ? (board.locked ?? []).includes(req.id) : false,
    layers: boardLayerList(board.layers ?? [], board.board_rules?.layer_setup),
    layerName: (l) => layerDisplayName(board.board_rules?.layer_setup, l),
    toast: (message) => dispatch({ type: "TOAST", message, kind: "info" }),
    submit: async (cmd, locked) => {
      const cmds: Cmd[] = req.id ? [{ op: "pcb_item", verb: "edit_text_box", id: req.id, text_box: cmd }] : [{ op: "pcb_item", verb: "add_text_box", text_box: cmd }];
      if (req.id && locked !== (board.locked ?? []).includes(req.id)) cmds.push({ op: "set_locked", ids: [req.id], locked });
      const done = await applyCommands(api, dispatch, cmds);
      if (!done) return "The board refused the text box.";
      if (!req.id && locked) {
        // a new box takes its lock after it is added: it is the one new item the selection now holds
        const created = [...api.getState().selection].filter((id) => api.getState().board?.drawings?.text_boxes?.some((t) => t.id === id));
        if (created.length > 0) await api.cmd({ op: "set_locked", ids: created, locked: true });
      }
      return null;
    },
    close: () => closeTextBoxDialog(),
  };
  return <TextBoxForm host={host} />;
}

export function TextBoxForm({ host }: { host: TextBoxHost }) {
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);

  // the form starts from the box the dialog was opened on, and keeps what is typed through the state's refreshes
  useEffect(() => {
    setForm(formOf(host.box, host.locked ?? false));
    setError(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host.openKey]);

  if (!form) return null;
  const set = <K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = host.close;

  const ok = async () => {
    // `m_textWidth.Validate( minSize, maxSize )`
    const [w, h] = [um(form.width), um(form.height)];
    for (const [label, v] of [["width", w], ["height", h]] as const) {
      if (!Number.isFinite(v) || v < TEXT_MIN_SIZE_NM || v > TEXT_MAX_SIZE_NM) return setError(`The text ${label} must be between 0.001 mm and 250 mm.`);
    }
    const prev = host.box;
    let pen = form.autoThickness ? 0 : um(form.thickness);
    if (!Number.isFinite(pen) || pen < 0) return setError("The text thickness must not be negative.");
    if (pen > 0) {
      const max = clampPenNm(pen, Math.min(w, h));
      if (pen > max) {
        host.toast("The text thickness is too large for the text size. It will be clamped.");
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
      thickness: pen > 0 ? pen : autoPenNm(Math.min(w, h), form.bold),
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
    const refused = await host.submit(textBoxToCmd(box), form.locked);
    if (refused) return setError(refused);
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
              {!host.layers.includes(form.layer) && <option value={form.layer}>{form.layer}</option>}
              {host.layers.map((l) => (
                <option key={l} value={l}>
                  {host.layerName(l)}
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
            {host.locked !== null && (
              <>
                <span>Locked</span>
                <label>
                  <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="ptb-locked" />
                </label>
              </>
            )}
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

/**
 * The four length fields are `ABS_X_COORD` / `ABS_Y_COORD` in KiCad (`m_posX`, `m_posY`, and -- as KiCad 8303b2ad and 25ec2335 set them -- `m_width` and `m_height`
 * too), so the board editor's Origins & Axes (UI-LEFT-2 PP9) turns all four: the width and the height show less the origin's X and Y, as KiCad's do, and are read
 * back the same way, so the scale they set is right.
 */
function refImageFormOf(i: BoardRefImage, locked: boolean, xf: OriginTransforms): RefImageForm {
  return { x: mm(toDisplay(xf, i.x, "abs_x")), y: mm(toDisplay(xf, i.y, "abs_y")), layer: i.layer, locked, scale: scaleText(i.scale_ppm), width: mm(toDisplay(xf, i.w, "abs_x")), height: mm(toDisplay(xf, i.h, "abs_y")) };
}

/**
 * `DIALOG_REFERENCE_IMAGE_PROPERTIES` (pcbnew/dialogs/dialog_reference_image_properties.cpp): the position, the layer, the lock, the size and, in the panel the schematic's Image Properties
 * shares, the scale and Convert to Greyscale. Width, height and scale move together (`onWidthChanged`, `onHeightChanged`, `onScaleChanged`); OK checks the scale (`CheckValues`) and applies
 * everything as one step, Cancel leaves the image as it was.
 */
/** What Reference Image Properties needs from the editor it is open in (`ShowReferenceImagePropertiesDialog` of the board editor and of the footprint editor). */
export interface RefImageHost {
  /** A new value opens the form afresh on `image`. */
  openKey: unknown;
  image: BoardRefImage;
  /** The image's pixels, for the preview and Convert to Greyscale. */
  previewUrl: string;
  /** The image's lock; null where the editor keeps none (no Locked row). */
  locked: boolean | null;
  /** The frame's origin transforms (the board editor's Origins & Axes); absent: none. */
  xf?: OriginTransforms;
  layers: string[];
  layerName: (layer: string) => string;
  /** OK: the position, the layer, the scale, the greyscale pixels (null: unchanged) and the lock, one undo step; null when taken, else why not. */
  submit: (edit: { layer: string; at: { x: number; y: number }; scalePpm: number; grey: string | null; locked: boolean }) => Promise<string | null>;
  close: () => void;
}

/** The board's dialog: `edit_ref_image`, `set_ref_image_data` and the lock, on an image the board has. */
export function PcbRefImageDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { refImage: req } = usePcbItemDialogs();
  const image = req ? (state.board?.drawings?.images ?? []).find((i) => i.id === req.id) : undefined;
  if (!req || !image || !state.board) return null;
  const board = state.board;
  const host: RefImageHost = {
    openKey: req,
    image,
    previewUrl: pcbImageUrl(image.id, image.hash),
    locked: (board.locked ?? []).includes(image.id),
    layers: boardLayerList(board.layers ?? [], board.board_rules?.layer_setup),
    layerName: (l) => layerDisplayName(board.board_rules?.layer_setup, l),
    xf: pcbTransformsOf(state.prefs, board),
    submit: async ({ layer, at, scalePpm, grey, locked }) => {
      const cmds: Cmd[] = [];
      if (at.x !== image.x || at.y !== image.y || layer !== image.layer || scalePpm !== image.scale_ppm) {
        cmds.push({ op: "pcb_item", verb: "edit_ref_image", id: image.id, layer, at, scale_ppm: scalePpm });
      }
      if (grey) cmds.push({ op: "pcb_item", verb: "set_ref_image_data", id: image.id, data: grey });
      if (locked !== (board.locked ?? []).includes(image.id)) cmds.push({ op: "set_locked", ids: [image.id], locked });
      if (cmds.length > 0 && !(await applyCommands(api, dispatch, cmds, [], null, { keep: true }))) return "The board refused the image.";
      return null;
    },
    close: () => closeRefImageDialog(),
  };
  return <RefImagePropertiesForm host={host} />;
}

export function RefImagePropertiesForm({ host }: { host: RefImageHost }) {
  const image = host.image;
  const xf = host.xf ?? IDENTITY_TRANSFORMS;
  /** `ChangeDoubleValue` of a field of `type`, and `GetDoubleValue` back (mm text <-> nm). */
  const shownMm = (nm: number, type: CoordType) => mm(toDisplay(xf, nm, type));
  const takenNm = (text: string, type: CoordType) => Math.round(fromDisplay(xf, um(text), type));
  const [form, setForm] = useState<RefImageForm | null>(null);
  const [grey, setGrey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setForm(refImageFormOf(image, host.locked ?? false, xf));
    setGrey(null);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host.openKey]);

  if (!form) return null;
  const set = <K extends keyof RefImageForm>(key: K, value: RefImageForm[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = host.close;
  const sizeAt = (scalePpm: number): [number, number] => imageSizeNm(image.px, image.ppi, scalePpm);
  /** The scale was typed: the size follows (`onScaleChanged`). */
  const onScale = (text: string) => {
    const ppm = scalePpmOf(text);
    setForm((f) => {
      if (!f) return f;
      if (!(ppm > 0)) return { ...f, scale: text };
      const [w, h] = sizeAt(ppm);
      return { ...f, scale: text, width: shownMm(w, "abs_x"), height: shownMm(h, "abs_y") };
    });
    setError(null);
  };
  /** The width or the height was typed: the scale follows, and the other side (`onWidthChanged`, `onHeightChanged`). */
  const onSide = (side: "width" | "height", text: string) => {
    // `m_width.GetDoubleValue()`: what was typed, back through the field's transform
    const want = takenNm(text, side === "width" ? "abs_x" : "abs_y");
    setForm((f) => {
      if (!f) return f;
      if (!(want > 0) || text.trim() === "" || !Number.isFinite(Number(text))) return { ...f, [side]: text };
      const ppm = side === "width" ? scaleForWidthNm(image.px, image.ppi, want) : scaleForHeightNm(image.px, image.ppi, want);
      const [w, h] = sizeAt(ppm);
      return { ...f, scale: scaleText(ppm), width: side === "width" ? text : shownMm(w, "abs_x"), height: side === "height" ? text : shownMm(h, "abs_y") };
    });
    setError(null);
  };
  const greyscale = async () => {
    setBusy(true);
    try {
      setGrey(await greyscalePng(host.previewUrl, image.ppi));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const ok = async () => {
    const [x, y] = [takenNm(form.x, "abs_x"), takenNm(form.y, "abs_y")];
    if (!Number.isFinite(x) || !Number.isFinite(y)) return setError("The position must be numbers.");
    const scalePpm = scalePpmOf(form.scale);
    const check = checkImageScale(image.px, scalePpm);
    if (!check.ok) return setError(check.error);
    if (check.confirm && !window.confirm(check.confirm)) return;
    const refused = await host.submit({ layer: form.layer, at: { x, y }, scalePpm, grey, locked: form.locked });
    if (refused) return setError(refused);
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
              {!host.layers.includes(form.layer) && <option value={form.layer}>{form.layer}</option>}
              {host.layers.map((l) => (
                <option key={l} value={l}>
                  {host.layerName(l)}
                </option>
              ))}
            </select>
            {host.locked !== null && (
              <>
                <span>Locked</span>
                <label>
                  <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="pri-locked" />
                </label>
              </>
            )}
          </div>
          <div style={{ marginTop: 10 }}>
            <ImageEditorPanel previewUrl={grey ? `data:image/png;base64,${grey}` : host.previewUrl} ppi={image.ppi} scale={form.scale} onScale={onScale} onGreyscale={() => void greyscale()} busy={busy} testid="pri" />
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
