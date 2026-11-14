// "Export 3D View" (`DIALOG_EXPORT_3D_IMAGE`, 3d-viewer/dialogs/dialog_export_3d_image.cpp at KiCad 8303b2ad; the rules kicad-port/export3dImage.ts): Image Size
// (Width:, Height:, the unit, the aspect lock), Resolution (X / Y resolution, pixels/in or pixels/mm), the pixel size; OK renders the view at that size
// (`captureScreenshot`) and asks "3D Image File Name" (a `.jpg` / `.jpeg` name is JPEG, else PNG, ".png" added to a name of no extension), then saves it --
// the browser's download, where KiCad writes the file (`saveOrCopyImage`). UI-LEFT-3 C3-4.
import { useState } from "react";
import { closeExport3DImage, useExport3DImageOpen } from "../../state/viewer3dExport";
import { RESOLUTION_UNITS, SIZE_UNITS, exportFileOf, exportSize, initialExport3D, pixelSizeLabel, setHeight, setResolutionUnit, setSizeUnit, setWidth, toggleLock, type Export3DState } from "../../kicad-port/export3dImage";
import { saveBlob } from "../../api/libraryClient";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { Viewer3DApi } from "./Viewer3D";

export function Export3DImageDialog({ api }: { api: Viewer3DApi | null }) {
  const open = useExport3DImageOpen();
  if (!open || !api) return null;
  return <Body api={api} />;
}

function Body({ api }: { api: Viewer3DApi }) {
  const dispatch = useStudioDispatch();
  const boardName = useStudioState().board?.name ?? "board";
  const [s, setS] = useState<Export3DState>(() => {
    const c = api.viewSize();
    return initialExport3D(Math.max(1, c.width), Math.max(1, c.height));
  });
  const [naming, setNaming] = useState<{ width: number; height: number } | null>(null);
  const [name, setName] = useState(`${boardName}.png`);
  const num = (v: number, digits: number) => String(Number(v.toFixed(digits)));
  const digits = s.sizeUnit === "pixels" ? 0 : 2;

  const save = async () => {
    if (!naming) return;
    const f = exportFileOf(name.trim() || `${boardName}.png`);
    const blob = await api.exportImage(naming.width, naming.height, f.format);
    closeExport3DImage();
    if (blob) saveBlob(blob, f.name);
    else dispatch({ type: "TOAST", message: "Can't save file.", kind: "error" });
  };

  if (naming)
    return (
      <div className="dialog-backdrop" onClick={closeExport3DImage}>
        <div className="dialog" role="dialog" aria-label="3D Image File Name" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
          <div className="dialog-header">
            <span>3D Image File Name</span>
          </div>
          <div className="dialog-body">
            <input aria-label="File name" data-testid="export3d-file-name" value={name} onChange={(e) => setName(e.target.value)} style={{ width: "100%" }} autoFocus />
            <div style={{ fontSize: 11, opacity: 0.7, marginTop: 4 }}>JPEG file (*.jpg, *.jpeg) or PNG file (*.png)</div>
          </div>
          <div className="dialog-footer">
            <button onClick={closeExport3DImage}>Cancel</button>
            <button className="primary" data-testid="export3d-save" onClick={() => void save()}>
              Save
            </button>
          </div>
        </div>
      </div>
    );

  return (
    <div className="dialog-backdrop" onClick={closeExport3DImage}>
      <div className="dialog" role="dialog" aria-label="Export 3D View" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Export 3D View</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontWeight: 600, marginBottom: 4 }}>Image Size</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 110px 30px 90px", rowGap: 6, alignItems: "center" }}>
            <span>Width:</span>
            <input type="number" aria-label="Width:" data-testid="export3d-width" min={1} max={50000} value={num(s.width, digits)} onChange={(e) => setS(setWidth(s, Number(e.target.value)))} />
            <button aria-label={s.locked ? "Unlock aspect ratio" : "Lock aspect ratio"} title="Lock aspect ratio" data-testid="export3d-lock" onClick={() => setS(toggleLock(s))}>
              {s.locked ? "🔒" : "🔓"}
            </button>
            <select aria-label="Size units" data-testid="export3d-size-units" value={s.sizeUnit} onChange={(e) => setS(setSizeUnit(s, e.target.value as Export3DState["sizeUnit"]))}>
              {SIZE_UNITS.map(([u, label]) => (
                <option key={u} value={u}>
                  {label}
                </option>
              ))}
            </select>
            <span>Height:</span>
            <input type="number" aria-label="Height:" data-testid="export3d-height" min={1} max={50000} value={num(s.height, digits)} onChange={(e) => setS(setHeight(s, Number(e.target.value)))} />
            <span />
            <span />
          </div>
          <div data-testid="export3d-pixels" style={{ fontSize: 12, opacity: 0.8, margin: "6px 0" }}>
            {pixelSizeLabel(s)}
          </div>
          <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)" }} />
          <div style={{ fontWeight: 600, marginBottom: 4 }}>Resolution</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 110px 100px", rowGap: 6, alignItems: "center" }}>
            <span>X resolution:</span>
            <input type="number" aria-label="X resolution:" min={1} max={10000} value={num(s.xRes, 3)} onChange={(e) => setS({ ...s, xRes: Math.min(10000, Math.max(1, Number(e.target.value))) })} />
            <select aria-label="Resolution units" value={s.resUnit} onChange={(e) => setS(setResolutionUnit(s, e.target.value as Export3DState["resUnit"]))}>
              {RESOLUTION_UNITS.map(([u, label]) => (
                <option key={u} value={u}>
                  {label}
                </option>
              ))}
            </select>
            <span>Y resolution:</span>
            <input type="number" aria-label="Y resolution:" min={1} max={10000} value={num(s.yRes, 3)} onChange={(e) => setS({ ...s, yRes: Math.min(10000, Math.max(1, Number(e.target.value))) })} />
            <span />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={closeExport3DImage}>Cancel</button>
          <button className="primary" data-testid="export3d-ok" onClick={() => setNaming(exportSize(s))}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
