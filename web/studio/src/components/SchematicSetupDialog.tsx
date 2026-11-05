// "Schematic Setup..." (eeschema.EditorControl.schematicSetup -- eeschema/dialogs/dialog_schematic_setup.cpp): a tree of pages on the left and the page on
// the right, like Board Setup's, with every page of KiCad's treebook in its order (`DIALOG_SCHEMATIC_SETUP::DIALOG_SCHEMATIC_SETUP`):
//
//   General           Formatting            panel_setup_formatting.cpp (schematicSetup/FormattingPage.tsx): every row; the connection grid is ERC's
//                                           `endpoint_off_grid` grid, the junction dot size is drawn on the sheet
//                     Annotation            panel_eeschema_annotation_options.cpp (AnnotationPage.tsx): the unit notation, order and numbering
//                     Field Name Templates  panel_template_fieldnames.cpp (FieldNameTemplatesPage.tsx): the project's templates, Visible and URL
//                     BOM Presets           panel_bom_presets.cpp (BomPresetsPage.tsx): the project's saved BOM and BOM formatting presets
//   Electrical Rules  Violation Severity    common/dialogs/panel_setup_severities.cpp over eeschema/erc/erc_settings.cpp (ErcSeveritiesPage.tsx)
//                     Pin Conflicts Map     eeschema/dialogs/panel_setup_pinmap.cpp over eeschema/erc/erc_settings.cpp (below)
//   Project           Net Classes           common/dialogs/panel_setup_netclasses.cpp with `aIsEEschema` -- Board Setup's page (boardSetup/NetClassesPage.tsx,
//                                           `editor="sch"`) over the same table, showing the wire and bus thickness, colour and line style columns
//                     Bus Alias Definitions panel_setup_buses.cpp (BusAliasesPage.tsx)
//                     Text Variables        common/dialogs/panel_text_variables.cpp -- Board Setup's page (boardSetup/TextVariablesPage.tsx): one list for both
//   Schematic Data    Embedded Files        the shared `PANEL_EMBEDDED_FILES` (components/EmbeddedFilesPanel.tsx) on the schematic's list (the root sheet's)
//
// Each page's Apply is one undoable /api/cmd verb (or one `batch`), as Board Setup's are; the settings live in the derived project (`.kicad_pro`'s `schematic`
// object and `net_settings`) that kicad-cli reads. Not ported: "Import Settings from Another Project..." (`onAuxiliaryAction`; Board Setup has none either).
//
// The pin map grid is PANEL_SETUP_PINMAP's: the lower triangle of the 11 pin types ("NC is not included in the pin map as it generates errors
// separately"), clicking a cell cycles OK -> Warning -> Error -> OK (`changeErrorLevel`) and the verb writes both mirrored cells. "Reset to
// defaults" is `ResetPanel` -> `ERC_SETTINGS::ResetPinMap`. Every click is one undoable `set_erc_pin_map_cell` / `reset_erc_pin_map` /api/cmd.
// The map actually used by ERC is the design's own `schematic.erc_pin_map` (absent = KiCad's default) -- GET /api/sch/erc_pin_map reports it.
//
// A marker menu's "Edit violation severities..." opens this dialog on Violation Severity, "Edit pin-to-pin conflict map..." on the Pin Conflicts Map, "Edit
// connection grid spacing..." on Formatting and an undefined net class's "Edit Netclasses..." on Net Classes (`ShowSchematicSetupDialog( _( "Violation Severity" ) )`,
// `"Pin Conflicts Map"`, `"Formatting"`, `"Net Classes"`).
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchErcPinMap } from "../api/client";
import type { ErcPinMapReply } from "../api/types";
import { PINMAP_TYPE_COUNT, PIN_TYPE_LABELS, levelGlyph, levelTooltip, nextLevel, triangleCells } from "../kicad-port/ercPinMap";
import { getCheckerView, setErcView, type SchSetupPage } from "../state/checkerView";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { NetClassesPage } from "./boardSetup/NetClassesPage";
import { TextVariablesPage } from "./boardSetup/TextVariablesPage";
import { AnnotationPage } from "./schematicSetup/AnnotationPage";
import { BomPresetsPage } from "./schematicSetup/BomPresetsPage";
import { BusAliasesPage } from "./schematicSetup/BusAliasesPage";
import { ErcSeveritiesPage } from "./schematicSetup/ErcSeveritiesPage";
import { FieldNameTemplatesPage } from "./schematicSetup/FieldNameTemplatesPage";
import { FormattingPage } from "./schematicSetup/FormattingPage";
import { EmbeddedFilesPanel } from "./EmbeddedFilesPanel";
import "../styles/boardSetup.css";

type Page = SchSetupPage;

const SCHEMATIC_OWNER = { kind: "schematic" } as const;

/** KiCad's page tree (`DIALOG_SCHEMATIC_SETUP`'s treebook), with its page names, in its order. */
const TREE: ReadonlyArray<{ title: string; pages: ReadonlyArray<{ id: Page; label: string }> }> = [
  {
    title: "General",
    pages: [
      { id: "formatting", label: "Formatting" },
      { id: "annotation", label: "Annotation" },
      { id: "field_templates", label: "Field Name Templates" },
      { id: "bom_presets", label: "BOM Presets" },
    ],
  },
  {
    title: "Electrical Rules",
    pages: [
      { id: "severities", label: "Violation Severity" },
      { id: "pinmap", label: "Pin Conflicts Map" },
    ],
  },
  {
    title: "Project",
    pages: [
      { id: "netclasses", label: "Net Classes" },
      { id: "bus_aliases", label: "Bus Alias Definitions" },
      { id: "text_variables", label: "Text Variables" },
    ],
  },
  { title: "Schematic Data", pages: [{ id: "embedded_files", label: "Embedded Files" }] },
];
const PAGE_IDS: Page[] = TREE.flatMap((g) => g.pages.map((p) => p.id));
const PAGE_LABEL = Object.fromEntries(TREE.flatMap((g) => g.pages.map((p) => [p.id, p.label]))) as Record<Page, string>;

const LEVEL_COLORS = ["#2e9e57", "#d9a21b", "#d64545"];
const CELL = 26;
const LABEL_W = 130;

export function SchematicSetupDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.schDialog === "setup";
  const [page, setPage] = useState<Page>("formatting");
  const [map, setMap] = useState<ErcPinMapReply | null>(null);
  const [hover, setHover] = useState<{ row: number; col: number } | null>(null);
  // The pages that have edits not applied yet, marked in the tree; Close asks first.
  const [unapplied, setUnapplied] = useState<ReadonlySet<Page>>(new Set());
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

  const reload = useCallback(async () => {
    try {
      setMap(await fetchErcPinMap());
    } catch {
      setMap(null);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    setUnapplied(new Set());
    setAskDiscard(false);
    // A marker menu names the page it wants; without one the dialog opens on its first page, as KiCad's does.
    const wanted = getCheckerView().erc.setupPage;
    setPage(wanted ?? "formatting");
    if (wanted) setErcView({ setupPage: null });
  }, [open]);

  // The undo of a pin map click from outside the dialog shows up here too.
  useEffect(() => {
    if (open) void reload();
  }, [open, reload, state.version]);

  const leave = useCallback(() => dispatch({ type: "SET_SCH_DIALOG", dialog: null }), [dispatch]);
  const close = useCallback(() => {
    if (unapplied.size > 0) setAskDiscard(true);
    else leave();
  }, [unapplied, leave]);
  useEffect(() => {
    if (unapplied.size === 0) setAskDiscard(false);
  }, [unapplied]);

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
                    {unapplied.has(p.id) && <span className="bs-dot" title="Changes not applied yet" />}
                  </button>
                ))}
              </div>
            ))}
          </nav>

          <FormattingPage hidden={page !== "formatting"} onDirty={markers.formatting} />
          <AnnotationPage hidden={page !== "annotation"} onDirty={markers.annotation} />
          <FieldNameTemplatesPage hidden={page !== "field_templates"} onDirty={markers.field_templates} />
          <BomPresetsPage hidden={page !== "bom_presets"} onDirty={markers.bom_presets} />
          <ErcSeveritiesPage hidden={page !== "severities"} onDirty={markers.severities} />
          <NetClassesPage editor="sch" hidden={page !== "netclasses"} onDirty={markers.netclasses} />
          <BusAliasesPage hidden={page !== "bus_aliases"} onDirty={markers.bus_aliases} />
          <TextVariablesPage hidden={page !== "text_variables"} onDirty={markers.text_variables} />
          <EmbeddedFilesPanel owner={SCHEMATIC_OWNER} run={api.cmd} hidden={page !== "embedded_files"} version={state.version} />

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
              <span style={{ marginRight: "auto", fontSize: 11, color: "var(--chrome-warn)" }}>
                {PAGE_IDS.filter((id) => unapplied.has(id))
                  .map((id) => PAGE_LABEL[id])
                  .join(", ")}{" "}
                {unapplied.size === 1 ? "has" : "have"} changes that were not applied. Close and lose them?
              </span>
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
