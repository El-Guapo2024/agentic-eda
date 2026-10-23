// `DIALOG_IMAGE_PROPERTIES` (eeschema/dialogs/dialog_image_properties.cpp at KiCad 8303b2ad): the position of a schematic image and, in the panel the board's Reference Image Properties shares
// (components/ImageEditorPanel.tsx), its scale and Convert to Greyscale. OK checks the scale (`PANEL_IMAGE_EDITOR::CheckValues`) and sends the change as one `edit_graphic`, which leaves the
// pixels alone unless they were converted; Cancel leaves the image as it was. Opened by Properties (`E`, a double-click) on an image.
import { useState } from "react";
import { greyscalePng } from "../actions/imageFiles";
import { checkImageScale, DEFAULT_PPI, SCALE_PPM, scalePpmOf, scaleText, schImageUrl } from "../kicad-port/refImage";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { ImageEditorPanel } from "./ImageEditorPanel";
import { SchDialogShell } from "./SchDialogShell";

const mm = (um: number): string => String(Math.round(um) / 1e6);
const um = (s: string): number => Math.round(Number(s) * 1e6);

export function SchImageDialog({ id }: { id: string }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const graphic = state.schematic?.graphics?.find((g) => g.id === id);
  const shape = graphic?.shape.type === "image" ? graphic.shape : null;
  const [x, setX] = useState(() => mm(shape?.at.x ?? 0));
  const [y, setY] = useState(() => mm(shape?.at.y ?? 0));
  const [scale, setScale] = useState(() => scaleText(shape?.scale_ppm ?? SCALE_PPM));
  const [grey, setGrey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!graphic || !shape) return null;
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const url = schImageUrl(graphic.id, shape.hash ?? "", state.currentSheetPath.join("/"));
  const px = shape.px ?? [0, 0];

  const greyscale = async () => {
    setBusy(true);
    try {
      setGrey(await greyscalePng(url, shape.ppi ?? DEFAULT_PPI));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const ok = async () => {
    const [nx, ny] = [um(x), um(y)];
    if (!Number.isFinite(nx) || !Number.isFinite(ny)) return setError("The position must be numbers.");
    const scalePpm = scalePpmOf(scale);
    const check = checkImageScale(px, scalePpm);
    if (!check.ok) return setError(check.error);
    if (check.confirm && !window.confirm(check.confirm)) return;
    const changed = nx !== shape.at.x || ny !== shape.at.y || scalePpm !== (shape.scale_ppm ?? SCALE_PPM) || grey !== null;
    if (changed) {
      // no `data` keeps the pixels; converted ones replace them
      const next = { ...graphic, shape: { type: "image" as const, at: { x: nx, y: ny }, scale_ppm: scalePpm, ...(grey ? { data: grey } : {}) } };
      const { id: _id, ...input } = next;
      if (!(await api.cmd({ op: "sch_edit", verb: "edit_graphic", id: graphic.id, graphic: input }))) return setError("The sheet refused the image.");
    }
    close();
  };

  return (
    <SchDialogShell title="Image Properties" width={460} onCancel={close} onOk={() => void ok()} okLabel="OK">
      <div data-testid="sch-image-properties">
        <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
          <span>Position X (mm)</span>
          <input type="number" step="0.01" value={x} onChange={(e) => (setX(e.target.value), setError(null))} style={{ width: 100 }} data-testid="sim-x" />
          <span>Position Y (mm)</span>
          <input type="number" step="0.01" value={y} onChange={(e) => (setY(e.target.value), setError(null))} style={{ width: 100 }} data-testid="sim-y" />
        </div>
        <div style={{ marginTop: 10 }}>
          <ImageEditorPanel
            previewUrl={grey ? `data:image/png;base64,${grey}` : url}
            ppi={shape.ppi ?? DEFAULT_PPI}
            scale={scale}
            onScale={(t) => (setScale(t), setError(null))}
            onGreyscale={() => void greyscale()}
            busy={busy}
            testid="sim"
          />
        </div>
        {error && (
          <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="sim-error">
            {error}
          </p>
        )}
      </div>
    </SchDialogShell>
  );
}
