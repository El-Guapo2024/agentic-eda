// pcbnew.EditorControl.boardSetup ("Board Setup...") -- port of
// pcbnew/dialogs/dialog_board_setup.cpp, which is really a tree of
// ~15 panel_setup_*.cpp pages. Pages below map onto what this app's
// constraint model (crates/model/src/lib.rs `BoardRules`) actually holds;
// a page with no IR backing at all (vias/zones-only panels already
// covered by their own dialogs, tuning patterns, custom DRC-rule text) is
// left out entirely rather than faked. Teardrops (task item 4) now has
// real IR backing (`RoutingSection.teardrop_settings`) and is a genuinely
// editable page, like Track Widths & Vias.
//
// A hard split runs through every page here: `BoardRules` (net classes,
// hole/clearance/text defaults, stackup) lives on the *intent*-derived
// `ConstraintModel` this app loads read-only (`crates/cli/src/board.rs`'s
// `load`) -- there is no `Cmd` that can change it, because doing so needs
// a second edit/undo path into the intent file this session did not build
// (GAPS.md #10 sizes that "L", same as the custom-rule-language page would
// be). `RoutingSection.track_width_presets`/`via_presets`/
// `teardrop_settings` live on the editable `design.json` IR, so those
// pages are genuinely editable; every other page is a read-only mirror,
// same "no command exists for this field yet" convention this app's
// other dialogs already use.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, umFrom, umTo } from "../state/units";
import type { TeardropSettings, Um } from "../api/types";

type Page = "classes" | "tracks_vias" | "teardrops" | "rules" | "text_graphics" | "stackup";
const PAGES: { id: Page; label: string }[] = [
  { id: "classes", label: "Net Classes" },
  { id: "tracks_vias", label: "Track Widths & Vias" },
  { id: "teardrops", label: "Teardrops" },
  { id: "rules", label: "Design Rules" },
  { id: "text_graphics", label: "Text & Graphics" },
  { id: "stackup", label: "Layer Stackup" },
];

const DEFAULT_TEARDROP_SETTINGS: TeardropSettings = {
  enabled: false,
  target_vias: true,
  target_pth_pads: true,
  target_smd_pads: true,
  best_length_ratio: 0.5,
  best_width_ratio: 1.0,
  max_len_um: 1000,
  max_width_um: 2000,
  width_to_size_filter_ratio: 0.9,
};

export function BoardSetupDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.boardSetupDialogOpen;
  const [page, setPage] = useState<Page>("classes");
  const units = state.units;
  const rules = state.board?.board_rules;
  const routing = state.board?.routing;

  const [widths, setWidths] = useState<number[]>([]);
  const [vias, setVias] = useState<{ diameter: number; drill: number }[]>([]);
  const [td, setTd] = useState<TeardropSettings>(DEFAULT_TEARDROP_SETTINGS);
  const [tdBusy, setTdBusy] = useState(false);
  const [tdMessage, setTdMessage] = useState<string | null>(null);

  // (Re)seed the editable lists from the board every time the dialog
  // opens -- not on every render, so mid-edit keystrokes survive the
  // ~700ms /api/state poll landing in between them.
  useEffect(() => {
    if (!open) return;
    setWidths(routing?.track_width_presets ?? []);
    setVias(routing?.via_presets ?? []);
    setTd(routing?.teardrop_settings ?? DEFAULT_TEARDROP_SETTINGS);
    setTdMessage(null);
    if (state.boardSetupInitialPage) {
      setPage(state.boardSetupInitialPage as Page);
      dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: null });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: false });

  const applyTrackVia = async () => {
    await api.cmd({ op: "set_track_width_presets", widths });
    await api.cmd({ op: "set_via_presets", presets: vias });
  };

  const setTdField = <K extends keyof TeardropSettings>(key: K, value: TeardropSettings[K]) => setTd((s) => ({ ...s, [key]: value }));

  const applyTeardropSettings = async () => {
    setTdBusy(true);
    setTdMessage(null);
    try {
      const ok = await api.cmd({ op: "set_teardrop_settings", settings: td });
      if (!ok) setTdMessage("Could not save -- check the ratios are between 0 and 1.");
    } finally {
      setTdBusy(false);
    }
  };

  const addAllTeardrops = async () => {
    setTdBusy(true);
    setTdMessage(null);
    try {
      await api.cmd({ op: "set_teardrop_settings", settings: td }); // save first, so "Add All" reflects the fields on screen
      const ok = await api.cmd({ op: "add_all_teardrops" });
      setTdMessage(ok ? "Teardrops added." : "Could not add teardrops.");
    } finally {
      setTdBusy(false);
    }
  };

  const removeAllTeardrops = async () => {
    setTdBusy(true);
    setTdMessage(null);
    try {
      const ok = await api.cmd({ op: "remove_all_teardrops" });
      setTdMessage(ok ? "All teardrops removed." : "Could not remove teardrops.");
    } finally {
      setTdBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640, maxHeight: "80vh" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Board Setup...</span>
        </div>
        <div className="editor-tabs" style={{ padding: "4px 12px 0" }}>
          {PAGES.map((p) => (
            <button key={p.id} className={`editor-tab${page === p.id ? " active" : ""}`} onClick={() => setPage(p.id)}>
              {p.label}
            </button>
          ))}
        </div>
        <div className="dialog-body" style={{ overflowY: "auto" }}>
          {page === "classes" && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>
                Read-only: net classes live on the board's intent/constraints file, which this app has no edit command for yet (dialog_copper_zones.cpp's net-class picker and panel_setup_rules.cpp's
                pattern-assignment editor are the same gap -- see GAPS.md #10).
              </p>
              <table className="setup-table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Nets</th>
                    <th>Track Width</th>
                    <th>Clearance</th>
                    <th>Via Size</th>
                    <th>Via Drill</th>
                    <th>Priority</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td>Default</td>
                    <td>everything else</td>
                    <td>{formatLength(rules?.track_width ?? 200, units)}</td>
                    <td>{formatLength(rules?.clearance ?? 200, units)}</td>
                    <td>{formatLength(rules?.via_diameter ?? 600, units)}</td>
                    <td>{formatLength(rules?.via_drill ?? 300, units)}</td>
                    <td>0</td>
                  </tr>
                  {(rules?.net_classes ?? []).map((c) => (
                    <tr key={c.name}>
                      <td>{c.name}</td>
                      <td>{c.nets.join(", ")}</td>
                      <td>{c.track_width != null ? formatLength(c.track_width, units) : "—"}</td>
                      <td>{c.clearance != null ? formatLength(c.clearance, units) : "—"}</td>
                      <td>{c.via_diameter != null ? formatLength(c.via_diameter, units) : "—"}</td>
                      <td>{c.via_drill != null ? formatLength(c.via_drill, units) : "—"}</td>
                      <td>{c.priority}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}

          {page === "tracks_vias" && (
            <>
              <p style={{ fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)" }}>Track Widths (W / Shift+W cycle these, plus the board default above)</p>
              {widths.map((w, i) => (
                <div key={i} className="filter-row" style={{ gap: 8 }}>
                  <input
                    type="number"
                    step="any"
                    value={umTo(w, units)}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (!Number.isFinite(n)) return;
                      setWidths((ws) => ws.map((cur, j) => (j === i ? Math.round(umFrom(n, units)) : cur)));
                    }}
                    style={{ width: 100 }}
                  />
                  <span>{units}</span>
                  <button onClick={() => setWidths((ws) => ws.filter((_, j) => j !== i))}>Remove</button>
                </div>
              ))}
              <button onClick={() => setWidths((ws) => [...ws, (rules?.track_width ?? 250) as Um])}>Add Track Width</button>

              <p style={{ fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 14 }}>Via Sizes (the via-size-cycle hotkey, plus the board default above)</p>
              {vias.map((v, i) => (
                <div key={i} className="filter-row" style={{ gap: 8 }}>
                  <span>Dia.</span>
                  <input
                    type="number"
                    step="any"
                    value={umTo(v.diameter, units)}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (!Number.isFinite(n)) return;
                      setVias((vs) => vs.map((cur, j) => (j === i ? { ...cur, diameter: Math.round(umFrom(n, units)) } : cur)));
                    }}
                    style={{ width: 90 }}
                  />
                  <span>Drill</span>
                  <input
                    type="number"
                    step="any"
                    value={umTo(v.drill, units)}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (!Number.isFinite(n)) return;
                      setVias((vs) => vs.map((cur, j) => (j === i ? { ...cur, drill: Math.round(umFrom(n, units)) } : cur)));
                    }}
                    style={{ width: 90 }}
                  />
                  <span>{units}</span>
                  <button onClick={() => setVias((vs) => vs.filter((_, j) => j !== i))}>Remove</button>
                </div>
              ))}
              <button onClick={() => setVias((vs) => [...vs, { diameter: (rules?.via_diameter ?? 600) as Um, drill: (rules?.via_drill ?? 300) as Um }])}>Add Via Size</button>

              <div style={{ marginTop: 14 }}>
                <button className="primary" onClick={applyTrackVia}>
                  Apply
                </button>
              </div>
            </>
          )}

          {page === "teardrops" && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>
                `pcbnew/teardrop/*` (task item 4). Round anchors only -- a via, or a round through-hole/SMD pad; a rectangular or round-rect pad never gets one (see `eda_connectivity::teardrop`'s own
                doc for the full scope: straight edges only, single track segment only, no track-to-track teardrops).
              </p>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={td.enabled} onChange={(e) => setTdField("enabled", e.target.checked)} /> Enable teardrops
              </label>

              <p style={{ fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 10 }}>Apply to</p>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={td.target_vias} onChange={(e) => setTdField("target_vias", e.target.checked)} /> Vias
              </label>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={td.target_pth_pads} onChange={(e) => setTdField("target_pth_pads", e.target.checked)} /> Through-hole pads (round only)
              </label>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={td.target_smd_pads} onChange={(e) => setTdField("target_smd_pads", e.target.checked)} /> SMD pads (round only)
              </label>

              <p style={{ fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 10 }}>Shape</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "220px 1fr" }}>
                <span>Best length ratio (of anchor size)</span>
                <input type="number" step="0.05" min={0} max={1} value={td.best_length_ratio} onChange={(e) => setTdField("best_length_ratio", Number(e.target.value))} style={{ width: 90 }} />
                <span>Best width ratio (of anchor size)</span>
                <input type="number" step="0.05" min={0} max={1} value={td.best_width_ratio} onChange={(e) => setTdField("best_width_ratio", Number(e.target.value))} style={{ width: 90 }} />
                <span>Maximum length</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    value={umTo(td.max_len_um, units)}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) setTdField("max_len_um", Math.round(umFrom(n, units)));
                    }}
                    style={{ width: 90 }}
                  />{" "}
                  {units}
                </span>
                <span>Maximum width</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    value={umTo(td.max_width_um, units)}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) setTdField("max_width_um", Math.round(umFrom(n, units)));
                    }}
                    style={{ width: 90 }}
                  />{" "}
                  {units}
                </span>
                <span>Width-to-size filter ratio</span>
                <input
                  type="number"
                  step="0.05"
                  min={0}
                  max={1}
                  value={td.width_to_size_filter_ratio}
                  onChange={(e) => setTdField("width_to_size_filter_ratio", Number(e.target.value))}
                  style={{ width: 90 }}
                />
              </div>

              <div style={{ marginTop: 14, display: "flex", gap: 8, alignItems: "center" }}>
                <button className="primary" disabled={tdBusy} onClick={applyTeardropSettings}>
                  Apply Settings
                </button>
                <button disabled={tdBusy} onClick={addAllTeardrops}>
                  Add All Teardrops
                </button>
                <button disabled={tdBusy} onClick={removeAllTeardrops}>
                  Remove All Teardrops
                </button>
              </div>
              {tdMessage && <p style={{ fontSize: 11, marginTop: 8 }}>{tdMessage}</p>}
            </>
          )}

          {page === "rules" && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>
                Read-only. This app's DRC engine has no custom rule language (`panel_setup_rules.cpp`'s expression-based constraints) -- see GAPS.md #10. The board-wide defaults it does check against:
              </p>
              <div className="kv-grid" style={{ gridTemplateColumns: "220px 1fr" }}>
                <span>Minimum clearance</span>
                <span>{formatLength(rules?.clearance ?? 200, units)}</span>
                <span>Minimum track width</span>
                <span>{formatLength(rules?.track_width ?? 200, units)}</span>
                <span>Minimum via annular ring</span>
                <span>{formatLength(rules?.annular_width_min_um ?? 100, units)}</span>
                <span>Minimum hole to hole</span>
                <span>{formatLength(rules?.hole_to_hole_min_um ?? 250, units)}</span>
                <span>Minimum hole clearance</span>
                <span>{formatLength(rules?.hole_clearance_um ?? 250, units)}</span>
                <span>Minimum silk clearance</span>
                <span>{formatLength(rules?.silk_clearance_um ?? 0, units)}</span>
              </div>
            </>
          )}

          {page === "text_graphics" && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>Read-only (`panel_setup_text_and_graphics.cpp`'s defaults) -- same model-split reasoning as the other pages here.</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "220px 1fr" }}>
                <span>Reference designator size</span>
                <span>{rules?.refdes_font_um != null ? formatLength(rules.refdes_font_um, units) : "auto (1/40 of the shorter board side)"}</span>
                <span>Minimum silk text height</span>
                <span>{formatLength(rules?.min_silk_text_height_um ?? 800, units)}</span>
                <span>Minimum silk text thickness</span>
                <span>{formatLength(rules?.min_silk_text_thickness_um ?? 80, units)}</span>
              </div>
            </>
          )}

          {page === "stackup" && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>Read-only (`panel_setup_layers.cpp`'s stackup table) -- same model-split reasoning as the other pages here.</p>
              {rules?.stackup && rules.stackup.layers.length > 0 ? (
                <table className="setup-table">
                  <thead>
                    <tr>
                      <th>Layer</th>
                      <th>Material</th>
                      <th>Thickness</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rules.stackup.layers.map((l, i) => (
                      <tr key={i}>
                        <td>{l.name}</td>
                        <td>{l.material ?? "—"}</td>
                        <td>{l.thickness_mm != null ? `${l.thickness_mm} mm` : "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : (
                <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>No stackup defined for this board -- copper layers: {(state.board?.layers ?? []).join(", ") || "none"}.</p>
              )}
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
