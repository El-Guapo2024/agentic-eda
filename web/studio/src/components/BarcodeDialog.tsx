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

const mm = (um: number): string => String(Math.round(um) / 1e6);
const um = (s: string): number => Math.round(Number(s) * 1e6);

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
    width: mm(b.width_nm),
    height: mm(b.height_nm),
    textSize: mm(b.text_height_nm),
    angle: String(angle / 1000),
    showText: b.show_text ?? true,
    knockout: b.knockout ?? false,
    marginX: mm(b.margin_x_nm ?? 0),
    marginY: mm(b.margin_y_nm ?? 0),
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
    width_nm: nums.w,
    height_nm: nums.h,
    text: f.text,
    text_height_nm: nums.th,
    kind: f.kind,
    ecc: clampEcc(f.kind, f.ecc),
    show_text: f.showText,
    knockout: f.knockout,
    margin_x_nm: nums.mx,
    margin_y_nm: nums.my,
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

/** What Barcode Properties needs from the editor it is open in (`ShowBarcodePropertiesDialog` of the board editor and of the footprint editor). */
export interface BarcodeHost {
  /** A new value opens the form afresh on `barcode`. */
  openKey: unknown;
  barcode: CmdBarcode;
  /** The barcode's lock; null where the editor keeps none (no Locked row). */
  locked: boolean | null;
  layers: string[];
  layerName: (layer: string) => string;
  /** OK: the barcode (without its id) and the lock asked for, one undo step; null when taken, else why not. */
  submit: (barcode: CmdBarcode, locked: boolean) => Promise<string | null>;
  close: () => void;
}

/** The board's dialog: `pcb_item` `add_barcode` for the Add Barcode tool's barcode, `edit_barcode` (and the lock) for one the board has. */
export function PcbBarcodeDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { barcode: req } = usePcbItemDialogs();
  if (!req || !state.board) return null;
  const board = state.board;
  const host: BarcodeHost = {
    openKey: req,
    barcode: req.barcode,
    locked: req.id ? (board.locked ?? []).includes(req.id) : false,
    layers: boardLayerList(board.layers ?? [], board.board_rules?.layer_setup),
    layerName: (l) => layerDisplayName(board.board_rules?.layer_setup, l),
    submit: async (rest, locked) => {
      const cmds: Cmd[] = req.id ? [{ op: "pcb_item", verb: "edit_barcode", id: req.id, barcode: rest }] : [{ op: "pcb_item", verb: "add_barcode", barcode: rest }];
      if (req.id && locked !== (board.locked ?? []).includes(req.id)) cmds.push({ op: "set_locked", ids: [req.id], locked });
      const done = await applyCommands(api, dispatch, cmds);
      if (!done) return "The board refused the barcode.";
      if (!req.id && locked) {
        // a new barcode takes its lock after it is added: it is the one new item the selection now holds
        const created = [...api.getState().selection].filter((id) => api.getState().board?.drawings?.barcodes?.some((b) => b.id === id));
        if (created.length > 0) await api.cmd({ op: "set_locked", ids: created, locked: true });
      }
      return null;
    },
    close: () => closeBarcodeDialog(),
  };
  return <BarcodeForm host={host} />;
}

export function BarcodeForm({ host }: { host: BarcodeHost }) {
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<BarcodeReply | null>(null);
  const ref = useRef<HTMLInputElement>(null);
  // the last plot and the form it was made of (`previewKey`): OK validates the text as it is now, never a plot of what it was before the last keystroke
  const latest = useRef<{ key: string; reply: BarcodeReply | null }>({ key: "", reply: null });

  // the form starts from the barcode the dialog was opened on, and keeps what is typed through the state's refreshes
  useEffect(() => {
    setForm(formOf(host.barcode, host.locked ?? false));
    setError(null);
    setPreview(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host.openKey]);

  // `refreshPreview`: the barcode as it is now, after a pause in the typing; the plot takes a moment, so ask again while it runs
  const previewKey = form ? JSON.stringify([form.text, form.kind, form.ecc, form.layer, form.width, form.height, form.textSize, form.angle, form.showText, form.knockout, form.marginX, form.marginY]) : "";
  useEffect(() => {
    if (!form) return;
    const b = barcodeOf(form, null);
    if (typeof b === "string" || form.text.length === 0) {
      latest.current = { key: previewKey, reply: form.text.length === 0 ? { status: "ready", paths: [] } : null };
      setPreview(latest.current.reply);
      return;
    }
    let live = true;
    const run = async (): Promise<void> => {
      for (let attempt = 0; attempt < 100 && live; attempt++) {
        const reply = await previewBarcode(b);
        if (!live) return;
        if (reply.status !== "pending") {
          latest.current = { key: previewKey, reply };
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

  if (!form) return null;
  const set = <K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = host.close;

  const ok = async () => {
    const built = barcodeOf(form, null);
    if (typeof built === "string") return setError(built);
    // `TransferDataFromWindow`: a text that made no symbol is refused with the reason ("Barcode Error"); an empty text has none to make.
    if (form.text.length > 0) {
      // `TransferDataFromWindow` builds the barcode of the values on screen: a plot of the form before the last change (OK pressed within the preview's
      // pause after typing) is not that, and the barcode is asked for again (UI-LEFT P29)
      let reply = latest.current.key === previewKey ? latest.current.reply : null;
      for (let attempt = 0; attempt < 100 && (!reply || reply.status === "pending"); attempt++) {
        reply = await previewBarcode(built);
        if (reply.status === "pending") await new Promise((r) => setTimeout(r, 250));
      }
      if (reply?.status === "ready" && reply.paths.length === 0) return setError(`Barcode Error: ${barcodeProblem(built.kind ?? "qr", form.text)}`);
    }
    const { id: _id, ...rest } = built;
    const refused = await host.submit(rest, form.locked);
    if (refused) return setError(refused);
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
              {!host.layers.includes(form.layer) && <option value={form.layer}>{form.layer}</option>}
              {host.layers.map((l) => (
                <option key={l} value={l}>
                  {host.layerName(l)}
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
            {host.locked !== null && (
              <>
                <span>Locked</span>
                <label>
                  <input type="checkbox" checked={form.locked} onChange={(e) => set("locked", e.target.checked)} data-testid="pbc-locked" />
                </label>
              </>
            )}
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
