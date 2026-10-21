// `PANEL_IMAGE_EDITOR` (common/dialogs/panel_image_editor.cpp at KiCad 8303b2ad): the part of Image Properties both editors share -- a preview of the image, its definition in pixels per
// inch, the scale it is shown at and the Convert to Greyscale button. The dialogs own the numbers and the working copy of the pixels; this only lays them out.
interface Props {
  /** Where the preview comes from: the image on the server, or the working copy a greyscale conversion made. */
  previewUrl: string;
  ppi: number;
  /** The scale as typed (`m_textCtrlScale`). */
  scale: string;
  onScale: (text: string) => void;
  onGreyscale: () => void;
  busy?: boolean;
  /** Prefix of the data-testids: `<prefix>-scale`, `<prefix>-ppi`, `<prefix>-greyscale`, `<prefix>-preview`. */
  testid: string;
}

export function ImageEditorPanel({ previewUrl, ppi, scale, onScale, onGreyscale, busy, testid }: Props) {
  return (
    <div style={{ display: "grid", gap: 8 }}>
      <div style={{ border: "1px solid var(--chrome-border, #888)", background: "var(--chrome-bg-sunken, #1118)", padding: 6, textAlign: "center" }}>
        <img src={previewUrl} alt="Preview of the image" style={{ maxWidth: "100%", maxHeight: 220, imageRendering: "auto" }} data-testid={`${testid}-preview`} />
      </div>
      <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
        <span>Image PPI</span>
        <span data-testid={`${testid}-ppi`}>{ppi}</span>
        <span>Scale</span>
        <input type="number" step="0.01" min="0" value={scale} onChange={(e) => onScale(e.target.value)} style={{ width: 100 }} data-testid={`${testid}-scale`} />
      </div>
      <div>
        <button type="button" onClick={onGreyscale} disabled={busy} data-testid={`${testid}-greyscale`}>
          Convert to Greyscale
        </button>
      </div>
    </div>
  );
}
