// The router's own dialogs (actions/pcbRouterSweep.ts):
//
//   LayerPairDialog               SELECT_COPPER_LAYERS_PAIR_DIALOG (pcbnew/sel_layer.cpp): the top and bottom
//                                 layer of the pair, the "add to presets" button and the presets table.
//   DiffPairDimensionsDialog      DIALOG_PNS_DIFF_PAIR_DIMENSIONS (dialogs/dialog_pns_diff_pair_dimensions.cpp).
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { addLayerPair, hasSameLayers, layerPairName, removeLayerPair, setCurrentLayerPair, type LayerPairInfo, type LayerPairSettings } from "../kicad-port/layerPairs";
import type { CustomDiffPair } from "../kicad-port/pcbParityState";
import { DialogShell, LengthRow, useLengthField } from "./pcbDialogKit";

export function LayerPairDialog({ settings, copper, onOk }: { settings: LayerPairSettings; copper: readonly string[]; onOk: (settings: LayerPairSettings) => void }) {
  // `m_dialogPairSettings`: a local copy edited by the dialog, handed back on OK.
  const [s, setS] = useState(settings);
  const [selected, setSelected] = useState<number | null>(null);
  const setPair = (a: string, b: string) => setS((cur) => setCurrentLayerPair(cur, { a, b }));
  const cell: React.CSSProperties = { padding: "2px 6px", fontSize: 12 };
  return (
    <DialogShell
      title="Select Copper Layer Pair"
      width={520}
      onCancel={closeSweepDialog}
      onOk={() => {
        closeSweepDialog();
        onOk(s);
      }}
    >
      <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>
        <LayerColumn title="Top/Front layer:" copper={copper} value={s.current.a} onPick={(l) => setPair(l, s.current.b)} name="layer-pair-top" />
        <LayerColumn title="Bottom/Back layer:" copper={copper} value={s.current.b} onPick={(l) => setPair(s.current.a, l)} name="layer-pair-bottom" />
        <div style={{ alignSelf: "center" }}>
          <button
            title="Add the selected pair to the presets"
            aria-label="Add to presets"
            onClick={() => setS((cur) => addLayerPair(cur, { pair: cur.current, enabled: true, name: null }).settings)}
          >
            Add &rarr;
          </button>
        </div>
      </div>
      <fieldset style={{ marginTop: 10, border: "1px solid var(--border, #444)", padding: "4px 8px" }}>
        <legend style={{ fontSize: 12 }}>Copper Layer Pair Presets</legend>
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr style={{ textAlign: "left", fontSize: 11, opacity: 0.8 }}>
              <th style={cell}>Enabled</th>
              <th style={cell}>Layers</th>
              <th style={cell}>Label</th>
            </tr>
          </thead>
          <tbody>
            {s.pairs.length === 0 && (
              <tr>
                <td colSpan={3} style={{ ...cell, opacity: 0.6 }}>
                  No presets. Pick two layers and press Add.
                </td>
              </tr>
            )}
            {s.pairs.map((p, i) => (
              <tr
                key={`${p.pair.a}/${p.pair.b}`}
                aria-selected={selected === i}
                style={{ background: selected === i ? "var(--accent-soft, rgba(80,140,255,0.25))" : undefined, fontWeight: hasSameLayers(p.pair, s.current) ? 600 : 400 }}
                onClick={() => setSelected(i)}
                onDoubleClick={() => setS((cur) => setCurrentLayerPair(cur, p.pair))} // `onPairActivated`
              >
                <td style={cell}>
                  <input
                    type="checkbox"
                    checked={p.enabled}
                    aria-label={`Enable ${layerPairName(p.pair)}`}
                    onChange={(e) => setS((cur) => ({ ...cur, pairs: cur.pairs.map((q, j): LayerPairInfo => (j === i ? { ...q, enabled: e.target.checked } : q)) }))}
                  />
                </td>
                <td style={cell}>{layerPairName(p.pair)}</td>
                <td style={cell}>
                  <input
                    style={{ width: "100%" }}
                    value={p.name ?? ""}
                    aria-label={`Label of ${layerPairName(p.pair)}`}
                    onChange={(e) => setS((cur) => ({ ...cur, pairs: cur.pairs.map((q, j): LayerPairInfo => (j === i ? { ...q, name: e.target.value } : q)) }))}
                  />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ marginTop: 4 }}>
          <button
            disabled={selected === null || selected >= s.pairs.length}
            title="Delete the selected preset"
            onClick={() => {
              if (selected === null) return;
              const p = s.pairs[selected];
              if (p) setS((cur) => removeLayerPair(cur, p.pair).settings);
              setSelected(null);
            }}
          >
            Delete preset
          </button>
        </div>
      </fieldset>
    </DialogShell>
  );
}

function LayerColumn({ title, copper, value, onPick, name }: { title: string; copper: readonly string[]; value: string; onPick: (layer: string) => void; name: string }) {
  return (
    <div style={{ flex: 1 }}>
      <div style={{ fontSize: 12, marginBottom: 4 }}>{title}</div>
      <div style={{ border: "1px solid var(--border, #444)", padding: "2px 6px", maxHeight: 180, overflowY: "auto" }}>
        {copper.map((layer) => (
          <label key={layer} style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, padding: "1px 0" }}>
            <input type="radio" name={name} checked={value === layer} onChange={() => onPick(layer)} />
            {layer}
          </label>
        ))}
      </div>
    </div>
  );
}

export function DiffPairDimensionsDialog({ value, onOk }: { value: CustomDiffPair; onOk: (v: CustomDiffPair) => void }) {
  const width = useLengthField(value.widthUm);
  const gap = useLengthField(value.gapUm);
  const viaGap = useLengthField(value.viaGapUm);
  const [same, setSame] = useState(value.viaGapSameAsTrackGap);
  const w = width.parse();
  const g = gap.parse();
  const v = viaGap.parse();
  // TransferDataFromWindow: "Track gap must be greater than 0."
  const error = g === null || g <= 0 ? "Track gap must be greater than 0." : w === null || w <= 0 ? "Width must be greater than 0." : !same && (v === null || v < 0) ? "Via gap must be a length." : null;
  const submit = () => {
    if (error || w === null || g === null) return;
    closeSweepDialog();
    onOk({ widthUm: w, gapUm: g, viaGapUm: same || v === null ? g : v, viaGapSameAsTrackGap: same });
  };
  return (
    <DialogShell title="Differential Pair Dimensions" width={340} onCancel={closeSweepDialog} onOk={submit} okDisabled={!!error}>
      <LengthRow label="Width:" field={width} autoFocus onEnter={submit} />
      <LengthRow label="Track gap:" field={gap} onEnter={submit} />
      <LengthRow label="Via gap:" field={viaGap} onEnter={submit} disabled={same} />
      <label className="filter-row" style={{ marginTop: 4 }}>
        <input type="checkbox" checked={same} onChange={(e) => setSame(e.target.checked)} />
        Via gap same as track gap
      </label>
      {error && <div style={{ color: "#ef5b5b", fontSize: 11, marginTop: 6 }}>{error}</div>}
    </DialogShell>
  );
}
