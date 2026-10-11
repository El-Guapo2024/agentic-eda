// Port of pcbnew/dialogs/dialog_global_edit_tracks_and_vias{,_base}.cpp --
// "Edit Track & Via Properties..." (pcbnew.GlobalEdit.editTracksAndVias,
// task item 2). Scope and filter checkboxes default unchecked, same literal
// fidelity CleanupTracksDialog.tsx already established for this port
// (nothing in the read dialog base ever calls SetValue(true) on them).
//
// The Scope box (Tracks; Vias with its tri-state box over the Through, Blind
// and Buried types) and the Filter Items box (net, net class, layer, track
// width, via diameter, selected items only) are kicad-port/globalEditTracksVias.ts;
// the net and the class are remembered for the session as `g_netFilter` and
// `g_netclassFilter` are, and the net starts on the highlighted net. A via's
// type is what its layers say, and its layer for the layer filter is the top
// of its span; this model has no microvia type, no padstack / annular-ring
// concept and no IPC-4761 protection feature, so the Microvias box and those
// two Action rows are not here (PARITY-pcb.md section 13). The Action side
// edits the layer (tracks), the track width and the via size, or sets every
// item to the values of its net class (`BoardRules::width_of` /
// `via_diameter_of` / `via_drill_of`).
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo, type LengthUnit } from "../state/units";
import { initialTracksViasFilter, matchingTracksAndVias, NOTHING_IN_SCOPE, trackViaChoices, viasBoxState, withAllVias, type TracksViasFilter, type TracksViasScope } from "../kicad-port/globalEditTracksVias";

type Action = "specified" | "net_class";

// `g_netclassFilter` / `g_netFilter`: "Globals to remember control settings during a session".
let remembered = { net: "", netclass: "" };

const GROUP_TITLE = { fontWeight: 600, fontSize: 11, marginBottom: 4 } as const;
const FILTER_ROW = { display: "flex", alignItems: "center", gap: 6, marginBottom: 4 } as const;

/** The number a length field shows: the value in the display unit, without the noise of a conversion. */
const shown = (um: number, unit: LengthUnit): string => String(Number(umTo(um, unit).toFixed(4)));

export function GlobalEditTracksAndViasDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.editTracksAndViasDialogOpen;

  const [scope, setScope] = useState<TracksViasScope>(NOTHING_IN_SCOPE);
  // null until a filter is touched: the defaults follow the highlighted net and the active layer of the moment the dialog opens.
  const [box, setBox] = useState<TracksViasFilter | null>(null);
  // What the two length filters show while they are being typed in (the filter itself holds µm).
  const [widthText, setWidthText] = useState<string | null>(null);
  const [sizeText, setSizeText] = useState<string | null>(null);

  const [action, setAction] = useState<Action>("specified");
  const [changeLayer, setChangeLayer] = useState(false);
  const [newLayer, setNewLayer] = useState("");
  const [changeTrackWidth, setChangeTrackWidth] = useState(false);
  const [trackWidth, setTrackWidth] = useState(0); // display units
  const [changeViaSize, setChangeViaSize] = useState(false);
  const [viaDiameter, setViaDiameter] = useState(0); // display units
  const [viaDrill, setViaDrill] = useState(0); // display units
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const unit: LengthUnit = state.units;
  const filter = box ?? initialTracksViasFilter(state.board, state.netHighlight, state.activeLayer, remembered);
  const choices = trackViaChoices(state.board);
  const layers = state.board?.layers ?? [];

  const close = () => {
    remembered = { net: filter.net, netclass: filter.netclass };
    setScope(NOTHING_IN_SCOPE);
    setBox(null);
    setWidthText(null);
    setSizeText(null);
    dispatch({ type: "SET_EDIT_TRACKS_AND_VIAS_DIALOG_OPEN", open: false });
  };

  const setFilter = (patch: Partial<TracksViasFilter>) => setBox({ ...filter, ...patch });
  const viasState = viasBoxState(scope);

  const { trackIds, viaIds } = matchingTracksAndVias(state.board, scope, filter, state.selection);
  const ids = [...trackIds, ...viaIds];

  const apply = async () => {
    if (ids.length === 0) return;
    setBusy(true);
    try {
      const ok = await api.cmd({
        op: "edit_tracks_and_vias",
        ids,
        track_width: action === "net_class" ? { kind: "net_class" } : changeTrackWidth ? { kind: "value", um: Math.round(umFrom(trackWidth, unit)) } : null,
        via_size: action === "net_class" ? { kind: "net_class" } : changeViaSize ? { kind: "value", diameter: Math.round(umFrom(viaDiameter, unit)), drill: Math.round(umFrom(viaDrill, unit)) } : null,
        layer: action === "specified" && changeLayer && newLayer ? newLayer : null,
      });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Edit Track & Via Properties</span>
        </div>
        <div className="dialog-body">
          <div style={GROUP_TITLE}>Scope</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" checked={scope.tracks} onChange={(e) => setScope({ ...scope, tracks: e.target.checked })} aria-label="Tracks" /> Tracks
          </label>
          <label className="filter-row" style={{ display: "block" }}>
            <input
              type="checkbox"
              checked={viasState === "on"}
              ref={(el) => {
                if (el) el.indeterminate = viasState === "mixed";
              }}
              onChange={(e) => setScope(withAllVias(scope, e.target.checked))}
              aria-label="Vias"
            />{" "}
            Vias
          </label>
          <div style={{ marginLeft: 22, marginBottom: 8 }}>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={scope.through} onChange={(e) => setScope({ ...scope, through: e.target.checked })} aria-label="Through vias" /> Through vias
            </label>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={scope.blind} onChange={(e) => setScope({ ...scope, blind: e.target.checked })} aria-label="Blind vias" /> Blind vias
            </label>
            <label className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={scope.buried} onChange={(e) => setScope({ ...scope, buried: e.target.checked })} aria-label="Buried vias" /> Buried vias
            </label>
          </div>

          <div style={GROUP_TITLE}>Filter Items</div>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filter.netOn} onChange={(e) => setFilter({ netOn: e.target.checked })} aria-label="Filter items by net" />
            Filter items by net:
            <select value={filter.net} onChange={(e) => setFilter({ net: e.target.value, netOn: true })} style={{ flex: 1 }} aria-label="Net filter">
              {choices.nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filter.netclassOn} onChange={(e) => setFilter({ netclassOn: e.target.checked })} aria-label="Filter items by net class" />
            Filter items by net class:
            <select value={filter.netclass} onChange={(e) => setFilter({ netclass: e.target.value, netclassOn: true })} style={{ flex: 1 }} aria-label="Net class filter">
              {choices.netclasses.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filter.layerOn} onChange={(e) => setFilter({ layerOn: e.target.checked })} aria-label="Filter items by layer" />
            Filter items by layer:
            <select value={filter.layer} onChange={(e) => setFilter({ layer: e.target.value, layerOn: true })} style={{ flex: 1 }} aria-label="Layer filter">
              {choices.layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filter.trackWidthOn} onChange={(e) => setFilter({ trackWidthOn: e.target.checked })} aria-label="Filter tracks by width" />
            Filter tracks by width:
            <input
              type="number"
              step="any"
              value={widthText ?? shown(filter.trackWidth, unit)}
              onChange={(e) => {
                setWidthText(e.target.value);
                setFilter({ trackWidth: Math.round(umFrom(Number(e.target.value), unit)), trackWidthOn: true });
              }}
              style={{ width: 80 }}
              aria-label="Track width filter"
            />
            {unit}
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filter.viaSizeOn} onChange={(e) => setFilter({ viaSizeOn: e.target.checked })} aria-label="Filter vias by diameter" />
            Filter vias by diameter:
            <input
              type="number"
              step="any"
              value={sizeText ?? shown(filter.viaSize, unit)}
              onChange={(e) => {
                setSizeText(e.target.value);
                setFilter({ viaSize: Math.round(umFrom(Number(e.target.value), unit)), viaSizeOn: true });
              }}
              style={{ width: 80 }}
              aria-label="Via diameter filter"
            />
            {unit}
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={filter.selectedOnly} onChange={(e) => setFilter({ selectedOnly: e.target.checked })} aria-label="Selected items only" /> Selected items only
          </label>

          <div style={GROUP_TITLE}>Action</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="radio" checked={action === "specified"} onChange={() => setAction("specified")} /> Set to specified values
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 6 }}>
            <input type="radio" checked={action === "net_class"} onChange={() => setAction("net_class")} /> Set to net class values
          </label>

          {action === "specified" && (
            <div className="kv-grid" style={{ gridTemplateColumns: "20px 110px 1fr", rowGap: 4 }}>
              <input type="checkbox" checked={changeLayer} onChange={(e) => setChangeLayer(e.target.checked)} />
              <span>Layer:</span>
              <select value={newLayer} onChange={(e) => setNewLayer(e.target.value)} disabled={!changeLayer}>
                <option value="">-- leave unchanged --</option>
                {layers.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>

              <input type="checkbox" checked={changeTrackWidth} onChange={(e) => setChangeTrackWidth(e.target.checked)} />
              <span>Track width:</span>
              <input type="number" value={trackWidth} disabled={!changeTrackWidth} onChange={(e) => setTrackWidth(Number(e.target.value))} />

              <input type="checkbox" checked={changeViaSize} onChange={(e) => setChangeViaSize(e.target.checked)} />
              <span>Via diameter:</span>
              <input type="number" value={viaDiameter} disabled={!changeViaSize} onChange={(e) => setViaDiameter(Number(e.target.value))} />

              <span />
              <span>Via drill:</span>
              <input type="number" value={viaDrill} disabled={!changeViaSize} onChange={(e) => setViaDrill(Number(e.target.value))} />
            </div>
          )}

          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 10 }}>
            {trackIds.length} track(s) and {viaIds.length} via(s) match the current scope and filters.
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button className="primary" onClick={apply} disabled={busy || ids.length === 0 || (action === "specified" && !changeLayer && !changeTrackWidth && !changeViaSize)}>
            Apply and Close
          </button>
        </div>
      </div>
    </div>
  );
}
