// pcbnew.EditorControl.boardSetup ("Board Setup...") -- port of pcbnew/dialogs/dialog_board_setup.cpp, which is a tree of panel_setup_*.cpp
// pages: the page tree on the left (Board Stackup, Text & Graphics, Design Rules, with KiCad's own page names) and the page on the right.
//
// Every page edits. The rule pages (Physical Stackup, Solder Mask/Paste, Defaults, Constraints, Net Classes, Custom Rules, Violation
// Severity -- components/boardSetup/*) edit a copy of what the board says and send the whole page on Apply as one undoable command
// (`set_stackup`, `set_mask_paste`, ... crates/ops/src/board_setup.rs). The backend keeps the page in design.json (`drawings.rules`) and
// `board::load` lays it over the board's description, so the router, DRC, the gates and the KiCad project kicad-cli reads all follow it, and
// Undo takes it back. Pre-defined Sizes, Teardrops and Dimensions edit the design directly (`set_track_width_presets` ...), as before.
//
// Not ported: Board Editor Layers (the board's layers are its copper layers; the technical layers are fixed), Zone Hatch Offsets, Formatting,
// Text Variables, Length-tuning Patterns, Tuning Profiles, Component Classes and Embedded Files -- nothing in the model holds them yet.
import { useCallback, useEffect, useMemo, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo } from "../state/units";
import type { DimensionSettings, DimensionTextPosition, DimensionUnits, DimensionUnitsFormat, TeardropSettings, Um } from "../api/types";
import { ConstraintsPage } from "./boardSetup/ConstraintsPage";
import { CustomRulesPage } from "./boardSetup/CustomRulesPage";
import { MaskPastePage } from "./boardSetup/MaskPastePage";
import { NetClassesPage } from "./boardSetup/NetClassesPage";
import { SeveritiesPage } from "./boardSetup/SeveritiesPage";
import { StackupPage } from "./boardSetup/StackupPage";
import { TextGraphicsPage } from "./boardSetup/TextGraphicsPage";
import "../styles/boardSetup.css";

export type Page = "stackup" | "mask_paste" | "text_graphics" | "dimensions" | "constraints" | "tracks_vias" | "teardrops" | "classes" | "custom_rules" | "severities";

/** KiCad's page tree (`DIALOG_BOARD_SETUP`'s treebook), with its page names; the pages that are not here have nothing in the model to edit. */
const TREE: ReadonlyArray<{ title: string; pages: ReadonlyArray<{ id: Page; label: string }> }> = [
  {
    title: "Board Stackup",
    pages: [
      { id: "stackup", label: "Physical Stackup" },
      { id: "mask_paste", label: "Solder Mask/Paste" },
    ],
  },
  {
    title: "Text & Graphics",
    pages: [
      { id: "text_graphics", label: "Defaults" },
      { id: "dimensions", label: "Dimensions" },
    ],
  },
  {
    title: "Design Rules",
    pages: [
      { id: "constraints", label: "Constraints" },
      { id: "tracks_vias", label: "Pre-defined Sizes" },
      { id: "teardrops", label: "Teardrops" },
      { id: "classes", label: "Net Classes" },
      { id: "custom_rules", label: "Custom Rules" },
      { id: "severities", label: "Violation Severity" },
    ],
  },
];
const PAGE_IDS: Page[] = TREE.flatMap((g) => g.pages.map((p) => p.id));
const PAGE_LABEL = Object.fromEntries(TREE.flatMap((g) => g.pages.map((p) => [p.id, p.label]))) as Record<Page, string>;

// `BOARD_DESIGN_SETTINGS`'s own real defaults (task item 7) -- see
// `eda_model::ir::DimensionSettings`'s own `impl Default` doc.
const DEFAULT_DIMENSION_SETTINGS: DimensionSettings = {
  units: "automatic",
  units_format: "no_suffix",
  precision: 4,
  suppress_trailing_zeros: true,
  text_position: "outside",
  keep_text_aligned: true,
  text_size_um: 1000,
  stroke_width: 200,
  arrow_length: 1270,
  extension_offset: 500,
  extension_height: 586,
};

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
  const drawings = state.board?.drawings;

  const [widths, setWidths] = useState<number[]>([]);
  const [vias, setVias] = useState<{ diameter: number; drill: number }[]>([]);
  const [td, setTd] = useState<TeardropSettings>(DEFAULT_TEARDROP_SETTINGS);
  const [tdBusy, setTdBusy] = useState(false);
  const [tdMessage, setTdMessage] = useState<string | null>(null);
  const [dimSettings, setDimSettings] = useState<DimensionSettings>(DEFAULT_DIMENSION_SETTINGS);
  const [dimBusy, setDimBusy] = useState(false);
  const [dimMessage, setDimMessage] = useState<string | null>(null);
  // The pages that have edits not applied yet (the rule pages say so themselves), marked in the tree and asked about on Close.
  const [unapplied, setUnapplied] = useState<ReadonlySet<Page>>(new Set());
  // Close was asked for while some page has such edits: the footer asks whether to drop them.
  const [askDiscard, setAskDiscard] = useState(false);
  const markers = useMemo(
    () =>
      Object.fromEntries(
        PAGE_IDS.map((id) => [
          id,
          (dirty: boolean) =>
            setUnapplied((prev) => {
              if (prev.has(id) === dirty) return prev;
              const next = new Set(prev);
              if (dirty) next.add(id);
              else next.delete(id);
              return next;
            }),
        ])
      ) as Record<Page, (dirty: boolean) => void>,
    []
  );

  // (Re)seed the editable lists from the board every time the dialog
  // opens -- not on every render, so mid-edit keystrokes survive the
  // ~700ms /api/state poll landing in between them.
  useEffect(() => {
    if (!open) return;
    setWidths(routing?.track_width_presets ?? []);
    setVias(routing?.via_presets ?? []);
    setTd(routing?.teardrop_settings ?? DEFAULT_TEARDROP_SETTINGS);
    setTdMessage(null);
    setDimSettings(drawings?.dimension_settings ?? DEFAULT_DIMENSION_SETTINGS);
    setDimMessage(null);
    setUnapplied(new Set());
    setAskDiscard(false);
    if (state.boardSetupInitialPage) {
      const wanted = state.boardSetupInitialPage as Page;
      if (PAGE_IDS.includes(wanted)) setPage(wanted);
      dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: null });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Close asks first when a rule page has edits that were never applied (they would be lost with the page); the question is in the footer,
  // not a native dialog, so a window that cannot show one still closes.
  const leave = useCallback(() => dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: false }), [dispatch]);
  const close = useCallback(() => {
    if (unapplied.size > 0) setAskDiscard(true);
    else leave();
  }, [unapplied, leave]);

  useEffect(() => {
    if (unapplied.size === 0) setAskDiscard(false);
  }, [unapplied]);

  // Escape closes the dialog (and is the dialog's: the canvas' own Escape must not also run); with the question asked it answers "keep editing".
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      // A select in the middle of a choice keeps its own Escape.
      if ((e.target as HTMLElement | null)?.tagName === "SELECT") return;
      e.stopPropagation();
      e.preventDefault();
      if (askDiscard) setAskDiscard(false);
      else close();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, close, askDiscard]);

  if (!open) return null;

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

  const setDimField = <K extends keyof DimensionSettings>(key: K, value: DimensionSettings[K]) => setDimSettings((s) => ({ ...s, [key]: value }));

  // `BOARD_DESIGN_SETTINGS::m_Dimension*` (task item 7): applied to new
  // dimensions from this point on only, never retroactively -- see
  // `DimensionSettings`'s own doc.
  const applyDimensionSettings = async () => {
    setDimBusy(true);
    setDimMessage(null);
    try {
      const ok = await api.cmd({ op: "set_dimension_settings", settings: dimSettings });
      setDimMessage(ok ? "Saved. Applies to new dimensions from now on." : "Could not save.");
    } finally {
      setDimBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog bs-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Board Setup</span>
        </div>
        <div className="dialog-body bs-body">
          <nav className="bs-nav" aria-label="Board Setup pages">
            {TREE.map((group) => (
              <div key={group.title}>
                <div className="bs-nav-group">{group.title}</div>
                {group.pages.map((p) => (
                  <button key={p.id} className={`bs-nav-item${page === p.id ? " active" : ""}`} aria-current={page === p.id ? "page" : undefined} onClick={() => setPage(p.id)}>
                    <span>{p.label}</span>
                    {unapplied.has(p.id) && <span className="bs-dot" title="Changes not applied yet" />}
                  </button>
                ))}
              </div>
            ))}
          </nav>

          <StackupPage hidden={page !== "stackup"} onDirty={markers.stackup} />
          <MaskPastePage hidden={page !== "mask_paste"} onDirty={markers.mask_paste} />
          <TextGraphicsPage hidden={page !== "text_graphics"} onDirty={markers.text_graphics} />
          <ConstraintsPage hidden={page !== "constraints"} onDirty={markers.constraints} />
          <NetClassesPage hidden={page !== "classes"} onDirty={markers.classes} />
          <CustomRulesPage hidden={page !== "custom_rules"} onDirty={markers.custom_rules} />
          <SeveritiesPage hidden={page !== "severities"} onDirty={markers.severities} />

          <section className="bs-page" hidden={page !== "tracks_vias"} aria-label="Pre-defined Sizes">
            <h3 className="bs-title">Pre-defined Sizes</h3>
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
          </section>

          <section className="bs-page" hidden={page !== "teardrops"} aria-label="Teardrops">
            <h3 className="bs-title">Teardrops</h3>
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
          </section>

          <section className="bs-page" hidden={page !== "dimensions"} aria-label="Dimensions">
            <h3 className="bs-title">Dimensions</h3>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>
                `panel_setup_dimensions.cpp` (task item 7): defaults a newly drawn dimension starts from (`StyleFromSettings`). Changing these never touches a dimension already on the board -- edit it directly (its own Properties dialog) instead.
              </p>
              <div className="kv-grid" style={{ gridTemplateColumns: "220px 1fr" }}>
                <span>Units</span>
                <select value={dimSettings.units} onChange={(e) => setDimField("units", e.target.value as DimensionUnits)}>
                  <option value="automatic">Automatic</option>
                  <option value="mm">Millimeters</option>
                  <option value="mil">Mils</option>
                  <option value="inch">Inches</option>
                </select>
                <span>Units format</span>
                <select value={dimSettings.units_format} onChange={(e) => setDimField("units_format", e.target.value as DimensionUnitsFormat)}>
                  <option value="no_suffix">1234.0</option>
                  <option value="bare_suffix">1234.0 mm</option>
                  <option value="paren_suffix">1234.0 (mm)</option>
                </select>
                <span>Precision (decimal places)</span>
                <input type="number" min={0} max={5} step={1} value={dimSettings.precision} onChange={(e) => setDimField("precision", Math.max(0, Math.min(5, Math.round(Number(e.target.value)))))} style={{ width: 90 }} />
                <span>Text position</span>
                <select value={dimSettings.text_position} onChange={(e) => setDimField("text_position", e.target.value as DimensionTextPosition)}>
                  <option value="outside">Outside</option>
                  <option value="inline">Inline</option>
                </select>
              </div>
              <label className="filter-row" style={{ display: "block", marginTop: 6 }}>
                <input type="checkbox" checked={dimSettings.suppress_trailing_zeros} onChange={(e) => setDimField("suppress_trailing_zeros", e.target.checked)} /> Suppress trailing zeros
              </label>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={dimSettings.keep_text_aligned} onChange={(e) => setDimField("keep_text_aligned", e.target.checked)} /> Keep text aligned with dimension
              </label>

              <p style={{ fontWeight: 600, fontSize: 11, color: "var(--chrome-text-dim)", marginTop: 10 }}>Style</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "220px 1fr" }}>
                <span>Text size</span>
                <span>
                  <input type="number" step="any" value={umTo(dimSettings.text_size_um, units)} onChange={(e) => setDimField("text_size_um", Math.round(umFrom(Number(e.target.value), units)))} style={{ width: 90 }} /> {units}
                </span>
                <span>Line thickness</span>
                <span>
                  <input type="number" step="any" value={umTo(dimSettings.stroke_width, units)} onChange={(e) => setDimField("stroke_width", Math.round(umFrom(Number(e.target.value), units)))} style={{ width: 90 }} /> {units}
                </span>
                <span>Arrow length</span>
                <span>
                  <input type="number" step="any" value={umTo(dimSettings.arrow_length, units)} onChange={(e) => setDimField("arrow_length", Math.round(umFrom(Number(e.target.value), units)))} style={{ width: 90 }} /> {units}
                </span>
                <span>Extension line offset</span>
                <span>
                  <input type="number" step="any" value={umTo(dimSettings.extension_offset, units)} onChange={(e) => setDimField("extension_offset", Math.round(umFrom(Number(e.target.value), units)))} style={{ width: 90 }} /> {units}
                </span>
                <span>Extension past crossbar</span>
                <span>
                  <input type="number" step="any" value={umTo(dimSettings.extension_height, units)} onChange={(e) => setDimField("extension_height", Math.round(umFrom(Number(e.target.value), units)))} style={{ width: 90 }} /> {units}
                </span>
              </div>

              <div style={{ marginTop: 14 }}>
                <button className="primary" disabled={dimBusy} onClick={applyDimensionSettings}>
                  Apply Settings
                </button>
              </div>
              {dimMessage && <p style={{ fontSize: 11, marginTop: 8 }}>{dimMessage}</p>}
          </section>
        </div>
        <div className="dialog-footer">
          {askDiscard && unapplied.size > 0 ? (
            <>
              <span className="bs-error" role="alert" style={{ marginRight: "auto", alignSelf: "center" }}>
                Not applied yet: {[...unapplied].map((id) => PAGE_LABEL[id]).join(", ")}. Discard these changes?
              </span>
              <button onClick={() => setAskDiscard(false)}>Keep editing</button>
              <button className="primary" onClick={leave}>
                Discard and close
              </button>
            </>
          ) : (
            <button className="primary" onClick={close}>
              Close
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
