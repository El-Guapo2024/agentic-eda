// "Schematic Setup..." (eeschema.EditorControl.schematicSetup -- eeschema/
// dialogs/dialog_schematic_setup.cpp) trimmed to the one page this app
// has a backing setting for: Electrical Rules > "Pin Conflicts Map"
// (eeschema/dialogs/panel_setup_pinmap.cpp over eeschema/erc/
// erc_settings.cpp). The real dialog's other pages (general options,
// formatting, annotation, field name templates, violation severities,
// net classes, text variables, ...) are not ported -- ERC *severity per
// check* in particular (panel_setup_severities.cpp) is a documented gap,
// see PARITY-sch.md -- so they are not shown as dead tabs.
//
// The grid is PANEL_SETUP_PINMAP's: the lower triangle of the 11 pin
// types ("NC is not included in the pin map as it generates errors
// separately"), clicking a cell cycles OK -> Warning -> Error -> OK
// (`changeErrorLevel`) and the verb writes both mirrored cells. "Reset to
// defaults" is `ResetPanel` -> `ERC_SETTINGS::ResetPinMap`. Every click is
// one undoable `set_erc_pin_map_cell` / `reset_erc_pin_map` /api/cmd.
// The map actually used by ERC is the design's own `schematic.erc_pin_map`
// (absent = KiCad's default) -- GET /api/sch/erc_pin_map reports it.
import { useCallback, useEffect, useState } from "react";
import { fetchErcPinMap } from "../api/client";
import type { ErcPinMapReply } from "../api/types";
import { PINMAP_TYPE_COUNT, PIN_TYPE_LABELS, levelGlyph, levelTooltip, nextLevel, triangleCells } from "../kicad-port/ercPinMap";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const LEVEL_COLORS = ["#2e9e57", "#d9a21b", "#d64545"];
const CELL = 26;
const LABEL_W = 130;

export function SchematicSetupDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.schDialog === "setup";
  const [map, setMap] = useState<ErcPinMapReply | null>(null);
  const [hover, setHover] = useState<{ row: number; col: number } | null>(null);

  const reload = useCallback(async () => {
    try {
      setMap(await fetchErcPinMap());
    } catch {
      setMap(null);
    }
  }, []);

  useEffect(() => {
    if (open) void reload();
  }, [open, reload]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_SCH_DIALOG", dialog: null });

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
        className="dialog"
        style={{ width: 640, maxHeight: "90vh" }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
      >
        <div className="dialog-header">
          <span>Schematic Setup</span>
          <span style={{ fontWeight: 400, fontSize: 11, color: "var(--chrome-text-dim)" }}>Electrical Rules &rsaquo; Pin Conflicts Map</span>
        </div>
        <div className="dialog-body">
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
        </div>
        <div className="dialog-footer">
          <button disabled={!map?.custom} title="Restore KiCad's default pin conflict map" onClick={() => void reset()}>
            Reset to Defaults
          </button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
