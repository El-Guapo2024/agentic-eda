// Barcode Properties (`DIALOG_BARCODE_PROPERTIES`, pcbnew/dialogs/dialog_barcode_properties.cpp at KiCad 8303b2ad), GAPS.md item 24, package P16 step 4: opened after the Add Barcode tool's
// click -- the barcode is added by OK and gone with Cancel -- or on a barcode that is there (E, a double click, Properties in the menu). It has the text, the kind (Code 39, Code 128, Data Matrix,
// QR Code, Micro QR Code), the error correction of the two QR kinds (Micro QR has no High), the layer, the position, the size of the symbol, the size of the text (while it is shown), the
// orientation, the knockout with its margins and the lock, and a preview of the barcode as kicad-cli plots it (`refreshPreview`). OK refuses a text that cannot be encoded as the kind
// the dialog has ("Barcode Error"), as KiCad does when zint made nothing of it.
import { useEffect, useRef, useState } from "react";
import { applyCommands } from "../actions/pcbSweepKit";
import type { BarcodeEcc, BarcodeKind, CmdBarcode } from "../api/pcbItemTypes";
import type { Cmd } from "../api/types";
import { BARCODE_KINDS, barcodeProblem, clampEcc, eccChoices, hasErrorCorrection, type BarcodePath } from "../kicad-port/barcodeShapes";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { closeBarcodeDialog, usePcbItemDialogs } from "../state/pcbItemDialogs";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { previewBarcode, type BarcodeReply } from "./canvas/barcodeCache";

const mm = (um: number): string => String(Math.round(um) / 1000);
const um = (s: string): number => Math.round(Number(s) * 1000);

interface Form {
  text: string;
  kind: BarcodeKind;
  ecc: BarcodeEcc;
  layer: string;
  x: string;
  y: string;
  width: string;
  height: string;
  textSize: string;
  /** Degrees. */
  angle: string;
  showText: boolean;
  knockout: boolean;
  marginX: string;
  marginY: string;
  locked: boolean;
}

/** `TransferDataToWindow`. */
function formOf(b: CmdBarcode, locked: boolean): Form {
  const angle = (b.angle ?? 0) > 180_000 ? (b.angle ?? 0) - 360_000 : (b.angle ?? 0);
  return {
    text: b.text,
    kind: b.kind ?? "qr",
    ecc: b.ecc ?? "L",
    layer: b.layer,
    x: mm(b.at.x),
    y: mm(b.at.y),
    width: mm(b.width_um),
    height: mm(b.height_um),
    textSize: mm(b.text_height_um),
    angle: String(angle / 1000),
    showText: b.show_text ?? true,
    knockout: b.knockout ?? false,
    marginX: mm(b.margin_x_um ?? 0),
    marginY: mm(b.margin_y_um ?? 0),
    locked,
  };
}

/** `transferDataToBarcode`: the barcode the form describes, or why it is not one. */
function barcodeOf(f: Form, id: string | null): CmdBarcode | string {
  const nums = { x: um(f.x), y: um(f.y), w: um(f.width), h: um(f.height), th: um(f.textSize), mx: um(f.marginX), my: um(f.marginY) };
  if (Object.values(nums).some((n) => !Number.isFinite(n))) return "Every size and position must be a number.";
  if (nums.w <= 0 || nums.h <= 0) return "The size of the barcode must be greater than zero.";
  if (nums.th <= 0) return "The text size must be greater than zero.";
  if (nums.mx < 0 || nums.my < 0) return "The margins must not be negative.";
  const turn = Number(f.angle);
  if (!Number.isFinite(turn)) return "The orientation must be a number of degrees.";
  const angle = ((Math.round(turn * 1000) % 360_000) + 360_000) % 360_000;
  const barcode: CmdBarcode = {
    layer: f.layer,
    at: { x: nums.x, y: nums.y },
    angle,
    width_um: nums.w,
    height_um: nums.h,
    text: f.text,
    text_height_um: nums.th,
    kind: f.kind,
    ecc: clampEcc(f.kind, f.ecc),
    show_text: f.showText,
    knockout: f.knockout,
    margin_x_um: nums.mx,
    margin_y_um: nums.my,
  };
  return id ? { ...barcode, id } : barcode;
}

/** The preview: the polygons of the plot, filled even-odd in the ink's colour, fitted to the dialog's box. */
function PreviewSvg({ paths }: { paths: BarcodePath[] }) {
  const pts = paths.flatMap((p) => p.flatMap((ring) => ring));
  if (pts.length === 0) return null;
  const [x0, x1] = [Math.min(...pts.map((p) => p[0])), Math.max(...pts.map((p) => p[0]))];
  const [y0, y1] = [Math.min(...pts.map((p) => p[1])), Math.max(...pts.map((p) => p[1]))];
  const pad = Math.max(x1 - x0, y1 - y0) * 0.05;
  return (
    <svg viewBox={`${x0 - pad} ${y0 - pad} ${x1 - x0 + 2 * pad} ${y1 - y0 + 2 * pad}`} width="100%" height="100%" data-testid="pbc-preview-svg" data-paths={paths.length}>
      {paths.map((path, i) => (
        <path key={i} d={path.map((ring) => `M ${ring.map((p) => `${p[0]} ${p[1]}`).join(" L ")} Z`).join(" ")} fill="currentColor" fillRule="evenodd" />
      ))}
    </svg>
  );
}

export function PcbBarcodeDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { barcode: req } = usePcbItemDialogs();
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<BarcodeReply | null>(null);
  const ref = useRef<HTMLInputElement>(null);
  const latest = useRef<BarcodeReply | null>(null);

  // the form starts from the barcode the dialog was opened on, and keeps what is typed through the state's refreshes
  useEffect(() => {
    if (!req) {
      setForm(null);
      setPreview(null);
      return;
    }
    setForm(formOf(req.barcode, req.id ? (state.board?.locked ?? []).includes(req.id) : false));
    setError(null);
    setPreview(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  // `refreshPreview`: the barcode as it is now, after a pause in the typing; the plot takes a moment, so ask again while it runs
  const previewKey = form ? JSON.stringify([form.text, form.kind, form.ecc, form.layer, form.width, form.height, form.textSize, form.angle, form.showText, form.knockout, form.marginX, form.marginY]) : "";
  useEffect(() => {
    if (!form) return;
    const b = barcodeOf(form, null);
    if (typeof b === "string" || form.text.length === 0) {
      latest.current = form.text.length === 0 ? { status: "ready", paths: [] } : null;
      setPreview(latest.current);
      return;
    }
    let live = true;
    const run = async (): Promise<void> => {
      for (let attempt = 0; attempt < 100 && live; attempt++) {
        const reply = await previewBarcode(b);
        if (!live) return;
        if (reply.status !== "pending") {
          latest.current = reply;
          setPreview(reply);
          return;
        }
        await new Promise((r) => setTimeout(r, 250));
      }
    };
    const t = setTimeout(() => void run(), 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewKey]);

  if (!req || !form || !state.board) return null;
  const board = state.board;
  const set = <K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = () => closeBarcodeDialog();

  const ok = async () => {
    const built = barcodeOf(form, req.id);
    if (typeof built === "string") return setError(built);
    // `TransferDataFromWindow`: a text that made no symbol is refused with the reason ("Barcode Error"); an empty text has none to make.
    if (form.text.length > 0) {
      let reply = latest.current;
      for (let attempt = 0; attempt < 100 && (!reply || reply.status === "pending"); attempt++) {
        reply = await previewBarcode(built);
        if (reply.status === "pending") await new Promise((r) => setTimeout(r, 250));
      }
      if (reply?.status === "ready" && reply.paths.length === 0) return setError(`Barcode Error: ${barcodeProblem(built.kind ?? "qr", form.text)}`);
    }
    const { id: _id, ...rest } = built;
    const cmds: Cmd[] = req.id ? [{ op: "pcb_item", verb: "edit_barcode", id: req.id, barcode: rest }] : [{ op: "pcb_item", verb: "add_barcode", barcode: rest }];
    if (req.id && form.locked !== (board.locked ?? []).includes(req.id)) cmds.push({ op: "set_locked", ids: [req.id], locked: form.locked });
    const done = await applyCommands(api, dispatch, cmds);
    if (!done) return setError("The board refused the barcode.");
    if (!req.id && form.locked) {
      // a new barcode takes its lock after it is added: it is the one new item the selection now holds
      const created = [...api.getState().selection].filter((id) => api.getState().board?.drawings?.barcodes?.some((b) => b.id === id));
      if (created.length > 0) await api.cmd({ op: "set_locked", ids: created, locked: true });
    }
    close();
  };

  const text = (key: "x" | "y" | "width" | "height" | "textSize" | "marginX" | "marginY" | "angle", testid: string, extra: Partial<React.InputHTMLAttributes<HTMLInputElement>> = {}) => (
    <input type="number" step={key === "angle" ? 1 : 0.01} value={form[key]} onChange={(e) => set(key, e.target.value)} style={{ width: 90 }} data-testid={testid} {...extra} />
  );
  const eccOn = hasErrorCorrection(form.kind);
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Barcode Properties" data-testid="pcb-barcode-properties">
        <div className="dialog-header">
          <span>Barcode Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <label style={{ display: "block", marginBottom: 4 }}>Text</label>
          <input
            ref={ref}
            value={form.text}
            style={{ width: "100%", boxSizing: "border-box" }}
            data-testid="pbc-text"
            onChange={(e) => set("text", e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void ok();
              if (e.key === "Escape") close();
            }}
          />
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr", marginTop: 10 }}>
            <span>Barcode type</span>
            <select
              value={form.kind}
              onChange={(e) => {
                const kind = e.target.value as BarcodeKind;
                // `OnUpdateUI`: Micro QR has no High level
                setForm((f) => (f ? { ...f, kind, ecc: clampEcc(kind, f.ecc) } : f));
                setError(null);
              }}
              data-testid="pbc-kind"
            >
              {BARCODE_KINDS.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.label}
                </option>
              ))}
            </select>
            <span>Error correction</span>
            <select value={form.ecc} disabled={!eccOn} onChange={(e) => set("ecc", e.target.value as BarcodeEcc)} data-testid="pbc-ecc">
              {eccChoices(form.kind).map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
            <span>Layer</span>
            <select value={form.layer} onChange={(e) => set("layer", e.target.value)} data-testid="pbc-layer">
              {boardLayerList(board.layers ?? [], board.board_rules?.layer_setup).map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(board.board_rules?.layer_setup, l)}
                </option>
              ))}
            </select>
            <span>Position X, Y (mm)</span>
            <span>
              {text("x", "pbc-x")} {text("y", "pbc-y")}
            </span>
            <span>Width, Height (mm)</span>
            <span>
              {text("width", "pbc-width", { min: 0.01 })} {text("height", "pbc-height", { min: 0.01 })}
            </span>
            <span>Orientation (deg)</span>
            {text("angle", "pbc-angle")}
            <span>Show text</span>
            <span>
              <input type="checkbox" checked={form.showText} onChange={(e) => set("showText", e.target.checked)} data-testid="pbc-show-text" aria-label="Show text" />
              <span style={{ marginLeft: 12, opacity: form.showText ? 1 : 0.5 }}>Text size (mm): {text("textSize", "pbc-text-size", { disabled: !form.showText, min: 0.01 })}</span>
            </span>
            <span>Knockout</span>
            <span>
              <input type="checkbox" checked={form.knockout} onChange={(e) => set("knockout", e.target.checked)} data-testid="pbc-knockout" aria-label="Knockout" />
              <span style={{ marginLeft: 12, opacity: form.knockout ? 1 : 0.5 }}>
                Margin X, Y (mm): {text("marginX", "pbc-margin-x", { disabled: !form.knockout, min: 0 })} {text("marginY", "pbc-margin-y", { disabled: !form.knockout, min: 0 })}
              </span>
            </span>
            <span>Locked</span>
            <label>
              <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="pbc-locked" />
            </label>
          </div>
          <div style={{ marginTop: 10, height: 190, border: "1px solid var(--chrome-border, #888)", borderRadius: 4, display: "flex", alignItems: "center", justifyContent: "center", padding: 6, boxSizing: "border-box" }} data-testid="pbc-preview">
            {preview?.status === "ready" && preview.paths.length > 0 && <PreviewSvg paths={preview.paths} />}
            {preview?.status === "ready" && preview.paths.length === 0 && form.text.length > 0 && <span style={{ opacity: 0.7 }}>No barcode: this text cannot be encoded as this kind of code.</span>}
            {preview?.status === "failed" && <span style={{ opacity: 0.7 }}>{preview.error}</span>}
            {!preview && form.text.length > 0 && <span style={{ opacity: 0.7 }}>Drawing the barcode...</span>}
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="pbc-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="pbc-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="pbc-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
