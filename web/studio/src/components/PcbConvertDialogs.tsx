// The dialogs of "Create from Selection" (actions/pcbConvertSweep.ts):
//
//   ConversionSettingsDialog   CONVERT_SETTINGS_DIALOG (pcbnew/tools/convert_tool.cpp): the strategy (copy the first
//                              object's line width / centerlines / bounding hull with its gap and width) and "Delete
//                              source objects after conversion"; the options a given conversion does not use are hidden.
//   OutsetItemsDialog          DIALOG_OUTSET_ITEMS (pcbnew/dialogs/dialog_outset_items.cpp).
//   CreateTracksDialog         the layer prompt of "Create Tracks from Selection" plus the net this model's tracks need.
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import type { ConvertSettings, ConvertStrategy, OutsetParams } from "../kicad-port/pcbConvert";
import { DialogShell, LengthRow, lengthText, useLengthField } from "./pcbDialogKit";

export function ConversionSettingsDialog({
  settings,
  showCopyLineWidth,
  showCenterline,
  showHull,
  onOk,
}: {
  settings: ConvertSettings;
  showCopyLineWidth: boolean;
  showCenterline: boolean;
  showHull: boolean;
  onOk: (s: ConvertSettings) => void;
}) {
  const available: ConvertStrategy[] = [...(showCopyLineWidth ? (["copy_linewidth"] as const) : []), ...(showCenterline ? (["centerline"] as const) : []), ...(showHull ? (["bounding_hull"] as const) : [])];
  const [strategy, setStrategy] = useState<ConvertStrategy>(available.includes(settings.strategy) ? settings.strategy : (available[0] ?? settings.strategy));
  const gap = useLengthField(settings.gapUm);
  const width = useLengthField(settings.widthUm);
  const [del, setDel] = useState(settings.deleteOriginals);
  const g = gap.parse();
  const w = width.parse();
  const hull = strategy === "bounding_hull";
  const ok = !hull || (g !== null && w !== null);
  const submit = () => {
    if (!ok) return;
    closeSweepDialog();
    onOk({ strategy, gapUm: hull ? (g ?? 0) : settings.gapUm, widthUm: hull ? (w ?? 0) : settings.widthUm, deleteOriginals: del });
  };
  const radio = (value: ConvertStrategy, label: string) => (
    <label className="filter-row" key={value}>
      <input type="radio" name="convert-strategy" checked={strategy === value} onChange={() => setStrategy(value)} />
      {label}
    </label>
  );
  return (
    <DialogShell title="Conversion Settings" width={380} onCancel={closeSweepDialog} onOk={submit} okDisabled={!ok}>
      {showCopyLineWidth && radio("copy_linewidth", "Copy line width of first object")}
      {showCenterline && radio("centerline", "Use centerlines")}
      {showHull && radio("bounding_hull", "Create bounding hull")}
      {showHull && (
        <div style={{ marginLeft: 22 }}>
          <LengthRow label="Gap:" field={gap} disabled={!hull} onEnter={submit} />
          <LengthRow label="Line width:" field={width} disabled={!hull} onEnter={submit} />
        </div>
      )}
      <label className="filter-row" style={{ marginTop: 8 }}>
        <input type="checkbox" checked={del} onChange={(e) => setDel(e.target.checked)} />
        Delete source objects after conversion
      </label>
    </DialogShell>
  );
}

/** `s_outsetPresetValue` / `s_presetLineWidths` / `s_presetGridRounding`, in micrometres. */
const OUTSET_PRESETS_UM = [100, 110, 150, 250, 260, 500, 1000, 2000];
const LINE_WIDTH_PRESETS_UM = [50, 100, 120, 150, 200];

export function OutsetItemsDialog({ params, layers, layerDefaultWidth, onOk }: { params: OutsetParams; layers: readonly string[]; layerDefaultWidth: (layer: string) => number; onOk: (p: OutsetParams) => void }) {
  const outset = useLengthField(params.outsetUm);
  const lineWidth = useLengthField(params.lineWidthUm);
  const grid = useLengthField(params.gridRoundingUm ?? 10);
  const [roundCorners, setRoundCorners] = useState(params.roundCorners);
  const [roundToGrid, setRoundToGrid] = useState(params.gridRoundingUm !== null);
  const [copyLayers, setCopyLayers] = useState(params.useSourceLayers);
  const [layer, setLayer] = useState(params.layer);
  const [copyWidths, setCopyWidths] = useState(params.useSourceWidths);
  const [del, setDel] = useState(params.deleteSourceItems);
  const o = outset.parse();
  const w = lineWidth.parse();
  const g = grid.parse();
  // TransferDataFromWindow: "Line width must be a positive value."
  const error = o === null ? "Enter the outset." : w === null || w <= 0 ? "Line width must be a positive value." : roundToGrid && (g === null || g <= 0) ? "Enter the grid size." : null;
  const submit = () => {
    if (error || o === null || w === null) return;
    closeSweepDialog();
    onOk({ outsetUm: o, roundCorners, useSourceLayers: copyLayers, useSourceWidths: copyWidths, layer, lineWidthUm: w, gridRoundingUm: roundToGrid ? g : null, deleteSourceItems: del });
  };
  const presets = (values: number[], set: (text: string) => void, units: "mm" | "mil" | "in") => (
    <select value="" aria-label="Presets" onChange={(e) => e.target.value && set(e.target.value)} style={{ width: 48 }}>
      <option value="">...</option>
      {values.map((v) => (
        <option key={v} value={lengthText(v, units)}>
          {lengthText(v, units)} {units}
        </option>
      ))}
    </select>
  );
  return (
    <DialogShell title="Outset Items" width={420} onCancel={closeSweepDialog} onOk={submit} okDisabled={!!error}>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <LengthRow label="Outset:" field={outset} autoFocus onEnter={submit} />
        {presets(OUTSET_PRESETS_UM, outset.setText, outset.units)}
      </div>
      <label className="filter-row" title="This is only possible for rectangular outsets.">
        <input type="checkbox" checked={roundToGrid} onChange={(e) => setRoundToGrid(e.target.checked)} />
        Round outwards to grid multiples (when possible)
      </label>
      <div style={{ marginLeft: 22 }}>
        <LengthRow label="Grid size:" field={grid} disabled={!roundToGrid} onEnter={submit} />
      </div>
      <label className="filter-row">
        <input type="checkbox" checked={roundCorners} onChange={(e) => setRoundCorners(e.target.checked)} />
        Round corners (when possible)
      </label>
      <div style={{ borderTop: "1px solid var(--border, #444)", margin: "8px 0" }} />
      <label className="filter-row">
        <input type="checkbox" checked={copyLayers} onChange={(e) => setCopyLayers(e.target.checked)} />
        Copy item layers
      </label>
      <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginLeft: 22, marginBottom: 6, opacity: copyLayers ? 0.5 : 1 }}>
        <span style={{ minWidth: 90 }}>Layer:</span>
        <select value={layer} disabled={copyLayers} onChange={(e) => setLayer(e.target.value)} style={{ flex: 1 }}>
          {layers.map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
      </label>
      <label className="filter-row" title="This is not possible for items like pads, which will still use the value below.">
        <input type="checkbox" checked={copyWidths} onChange={(e) => setCopyWidths(e.target.checked)} />
        Copy item widths (if possible)
      </label>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <LengthRow label="Line width:" field={lineWidth} onEnter={submit} />
        {presets(LINE_WIDTH_PRESETS_UM, lineWidth.setText, lineWidth.units)}
        <button
          title="Use the board's default line width for the layer"
          onClick={() => {
            lineWidth.setText(lengthText(layerDefaultWidth(copyLayers ? (layers[0] ?? layer) : layer), lineWidth.units));
          }}
        >
          Layer Default
        </button>
      </div>
      <label className="filter-row" style={{ marginTop: 6 }} title="Items that cannot be outset are never deleted.">
        <input type="checkbox" checked={del} onChange={(e) => setDel(e.target.checked)} />
        Delete source items after outset
      </label>
      {error && <div style={{ color: "#ef5b5b", fontSize: 11, marginTop: 6 }}>{error}</div>}
    </DialogShell>
  );
}

export function CreateTracksDialog({
  nets,
  copperLayers,
  layer,
  net,
  deleteOriginals,
  onOk,
}: {
  nets: readonly string[];
  copperLayers: readonly string[];
  layer: string;
  net: string;
  deleteOriginals: boolean;
  onOk: (v: { layer: string; net: string; deleteOriginals: boolean }) => void;
}) {
  const [l, setL] = useState(layer);
  const [n, setN] = useState(net || nets[0] || "");
  const [del, setDel] = useState(deleteOriginals);
  const ok = !!n && !!l;
  const submit = () => {
    if (!ok) return;
    closeSweepDialog();
    onOk({ layer: l, net: n, deleteOriginals: del });
  };
  return (
    <DialogShell title="Create Tracks from Selection" width={380} onCancel={closeSweepDialog} onOk={submit} okDisabled={!ok}>
      <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 6 }}>
        <span style={{ minWidth: 90 }}>Copper layer:</span>
        <select value={l} onChange={(e) => setL(e.target.value)} style={{ flex: 1 }}>
          {copperLayers.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginBottom: 6 }} title="Every track in this board model belongs to a net.">
        <span style={{ minWidth: 90 }}>Net:</span>
        <select value={n} onChange={(e) => setN(e.target.value)} style={{ flex: 1 }}>
          {nets.map((x) => (
            <option key={x} value={x}>
              {x}
            </option>
          ))}
        </select>
      </label>
      <label className="filter-row">
        <input type="checkbox" checked={del} onChange={(e) => setDel(e.target.checked)} />
        Delete source objects after conversion
      </label>
    </DialogShell>
  );
}
