// "Schematic Setup..." (eeschema.EditorControl.schematicSetup -- eeschema/
// dialogs/dialog_schematic_setup.cpp): a tree of pages on the left and the
// page on the right, like Board Setup's. The pages this app has a backing
// setting for are the two of Electrical Rules:
//
//   Violation Severity   common/dialogs/panel_setup_severities.cpp over eeschema/erc/erc_settings.cpp --
//                        the severity of each ERC check (schematicSetup/ErcSeveritiesPage.tsx), written
//                        to the derived project's `erc.rule_severities` for kicad-cli
//   Pin Conflicts Map    eeschema/dialogs/panel_setup_pinmap.cpp over eeschema/erc/erc_settings.cpp
//
// The real dialog's other pages (general options, formatting, annotation,
// field name templates, net classes, text variables, ...) are not ported
// -- see PARITY-sch.md -- so they are not shown as dead tabs.
//
// The pin map grid is PANEL_SETUP_PINMAP's: the lower triangle of the 11 pin
// types ("NC is not included in the pin map as it generates errors
// separately"), clicking a cell cycles OK -> Warning -> Error -> OK
// (`changeErrorLevel`) and the verb writes both mirrored cells. "Reset to
// defaults" is `ResetPanel` -> `ERC_SETTINGS::ResetPinMap`. Every click is
// one undoable `set_erc_pin_map_cell` / `reset_erc_pin_map` /api/cmd.
// The map actually used by ERC is the design's own `schematic.erc_pin_map`
// (absent = KiCad's default) -- GET /api/sch/erc_pin_map reports it.
//
// A marker menu's "Edit violation severities..." opens this dialog on the
// first page, "Edit pin-to-pin conflict map..." on the second
// (`ShowSchematicSetupDialog( _( "Violation Severity" ) )` / `"Pin Conflicts Map"`).
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchErcPinMap } from "../api/client";
import type { ErcPinMapReply } from "../api/types";
import { PINMAP_TYPE_COUNT, PIN_TYPE_LABELS, levelGlyph, levelTooltip, nextLevel, triangleCells } from "../kicad-port/ercPinMap";
import { getCheckerView, setErcView } from "../state/checkerView";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { ErcSeveritiesPage } from "./schematicSetup/ErcSeveritiesPage";
import "../styles/boardSetup.css";

type Page = "severities" | "pinmap";

/** KiCad's page tree (`DIALOG_SCHEMATIC_SETUP`'s treebook), with its page names; the pages that are not here have nothing in the model to edit. */
const TREE: ReadonlyArray<{ title: string; pages: ReadonlyArray<{ id: Page; label: string }> }> = [
  {
    title: "Electrical Rules",
    pages: [
      { id: "severities", label: "Violation Severity" },
      { id: "pinmap", label: "Pin Conflicts Map" },
    ],
  },
];

const LEVEL_COLORS = ["#2e9e57", "#d9a21b", "#d64545"];
const CELL = 26;
const LABEL_W = 130;

export function SchematicSetupDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.schDialog === "setup";
  const [page, setPage] = useState<Page>("severities");
  const [map, setMap] = useState<ErcPinMapReply | null>(null);
  const [hover, setHover] = useState<{ row: number; col: number } | null>(null);
  // The severity page has edits not applied yet; Close asks first.
  const [unapplied, setUnapplied] = useState(false);
  const [askDiscard, setAskDiscard] = useState(false);

  const reload = useCallback(async () => {
    try {
      setMap(await fetchErcPinMap());
    } catch {
      setMap(null);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    setUnapplied(false);
    setAskDiscard(false);
    // A marker menu names the page it wants.
    const wanted = getCheckerView().erc.setupPage;
    if (wanted) {
      setPage(wanted);
      setErcView({ setupPage: null });
    }
  }, [open]);

  // The undo of a pin map click from outside the dialog shows up here too.
  useEffect(() => {
    if (open) void reload();
  }, [open, reload, state.version]);

  const leave = useCallback(() => dispatch({ type: "SET_SCH_DIALOG", dialog: null }), [dispatch]);
  const close = useCallback(() => {
    if (unapplied) setAskDiscard(true);
    else leave();
  }, [unapplied, leave]);
  const dirty = useMemo(() => (unapplied ? ["severities" as Page] : []), [unapplied]);

  if (!open) return null;

  const click = async (row: number, col: number, level: number) => {
    // `changeErrorLevel`: ( level + 1 ) % 3, mirrored by the backend verb.
    await api.cmd({ op: "set_erc_pin_map_cell", a: row, b: col, level: nextLevel(level) });
    await reload();
  };

  const reset = async () => {
    await api.cmd({ op: "reset_erc_pin_map" });
    await reload();
  };

  const cells = map ? triangleCells(map.matrix) : [];
  const hovered = hover && map ? (map.matrix[hover.row]?.[hover.col] ?? 0) : null;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div
        className="dialog bs-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key !== "Escape") return;
          if (askDiscard) setAskDiscard(false);
          else close();
        }}
      >
        <div className="dialog-header">
          <span>Schematic Setup</span>
        </div>
        <div className="dialog-body bs-body">
          <nav className="bs-nav" aria-label="Schematic Setup pages">
            {TREE.map((group) => (
              <div key={group.title}>
                <div className="bs-nav-group">{group.title}</div>
                {group.pages.map((p) => (
                  <button key={p.id} className={`bs-nav-item${page === p.id ? " active" : ""}`} aria-current={page === p.id ? "page" : undefined} onClick={() => setPage(p.id)}>
                    <span>{p.label}</span>
                    {dirty.includes(p.id) && <span className="bs-dot" title="Changes not applied yet" />}
                  </button>
                ))}
              </div>
            ))}
          </nav>

          <ErcSeveritiesPage hidden={page !== "severities"} onDirty={setUnapplied} />

          <section className="bs-page" hidden={page !== "pinmap"} aria-label="Pin Conflicts Map">
            <h3 className="bs-title">Pin Conflicts Map</h3>
            <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 0 }}>
              Click a square to change what ERC reports when the two pin types meet on one net: no error, a warning, or an error. Changes are symmetric and saved with the design.
            </p>
            {!map && <div className="panel-empty">Loading pin map…</div>}
            {map && (
              <div style={{ position: "relative", marginLeft: LABEL_W, marginTop: 96, width: PINMAP_TYPE_COUNT * CELL, height: PINMAP_TYPE_COUNT * CELL + 4 }}>
                {/* Column headings: one rotated label above each diagonal cell, like the panel's CommentERC_V + "|" callout. */}
                {PIN_TYPE_LABELS.slice(0, PINMAP_TYPE_COUNT).map((label, i) => (
                  <div
                    key={`col-${label}`}
                    style={{ position: "absolute", left: i * CELL + CELL / 2, top: i * CELL - 2, transform: "translateY(-100%) rotate(-45deg)", transformOrigin: "0 100%", whiteSpace: "nowrap", fontSize: 10, color: "var(--chrome-text-dim)" }}
                  >
                    {label}
                  </div>
                ))}
                {PIN_TYPE_LABELS.slice(0, PINMAP_TYPE_COUNT).map((label, i) => (
                  <div key={`row-${label}`} style={{ position: "absolute", left: -LABEL_W, top: i * CELL, height: CELL, lineHeight: `${CELL}px`, width: LABEL_W - 8, textAlign: "right", fontSize: 11 }}>
                    {label}
                  </div>
                ))}
                {cells.map((c) => (
                  <button
                    key={`${c.row}-${c.col}`}
                    title={`${PIN_TYPE_LABELS[c.row]} / ${PIN_TYPE_LABELS[c.col]}: ${levelTooltip(c.level)}`}
                    aria-label={`${PIN_TYPE_LABELS[c.row]} and ${PIN_TYPE_LABELS[c.col]}: ${levelTooltip(c.level)}`}
                    onClick={() => void click(c.row, c.col, c.level)}
                    onMouseEnter={() => setHover({ row: c.row, col: c.col })}
                    onMouseLeave={() => setHover(null)}
                    style={{
                      position: "absolute",
                      left: c.col * CELL,
                      top: c.row * CELL,
                      width: CELL - 2,
                      height: CELL - 2,
                      padding: 0,
                      borderRadius: 3,
                      border: hover && hover.row === c.row && hover.col === c.col ? "2px solid var(--chrome-accent)" : "1px solid var(--chrome-border)",
                      background: LEVEL_COLORS[c.level],
                      color: "white",
                      fontWeight: 700,
                      fontSize: 12,
                      cursor: "pointer",
                    }}
                  >
                    {levelGlyph(c.level)}
                  </button>
                ))}
              </div>
            )}
            <div style={{ marginTop: 14, minHeight: 16, fontSize: 11, color: "var(--chrome-text-dim)" }}>
              {hover && hovered !== null ? `${PIN_TYPE_LABELS[hover.row]} / ${PIN_TYPE_LABELS[hover.col]}: ${levelTooltip(hovered)}` : map?.custom ? "This design uses a customized pin map." : "Using KiCad's default pin map."}
            </div>
            <div className="bs-actions">
              <button disabled={!map?.custom} title="Restore KiCad's default pin conflict map" onClick={() => void reset()}>
                Reset to Defaults
              </button>
            </div>
          </section>
        </div>
        <div className="dialog-footer">
          {askDiscard && (
            <>
              <span style={{ marginRight: "auto", fontSize: 11, color: "var(--chrome-warn)" }}>Violation Severity has changes that were not applied. Close and lose them?</span>
              <button onClick={() => setAskDiscard(false)}>Keep editing</button>
              <button className="primary" onClick={leave}>
                Discard and close
              </button>
            </>
          )}
          {!askDiscard && (
            <button className="primary" onClick={close}>
              Close
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
