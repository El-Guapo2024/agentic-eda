// The Footprint Chooser (`FOOTPRINT_CHOOSER_FRAME` over `PANEL_FOOTPRINT_CHOOSER`, pcbnew/footprint_chooser_frame.cpp): a tree of KiCad's installed
// footprint libraries with a search box, the footprint's drawing and its description beside it, and, when it is opened for a symbol, the two filters of
// `FOOTPRINT_CHOOSER_FRAME::filterFootprint` -- "Filter by pin count" and "Apply footprint filters" (the symbol's `ki_fp_filters`).
//
// The preview pane has KiCad's two switches (`FOOTPRINT_CHOOSER_FRAME`: "Show/hide footprint view panel" and "Show/hide 3D view panel", at least one of them on): the footprint's drawing, and
// its 3D view -- the footprint with the models its file lists on it, placed the way KiCad places them (components/footprint/ModelPreview3D.tsx, the preview of the 3D Models tab).
// "Show 3D viewer in own window" (`m_show3DViewer`, unchecked as the frame is made; `onExternalViewer3DEnable`, `Show3DViewerFrame`, `Update3DView`): while it is
// ticked and the 3D view is on, a window of its own (`EDA_3D_VIEWER_FRAME`, titled "3D Viewer — Lib:Name") shows the selected footprint in 3D and follows
// the selection (`updateViews`); unticking it closes that window, and so does its own close box. Here it is a window inside the page, moved by its title
// bar and sized by its corner, over the chooser.
//
// Two places use it: Place Footprint (`BOARD_EDITOR_CONTROL::PlaceFootprint`: a footprint of its own on the board -- a mounting hole, a fiducial -- beside
// the unplaced parts of the schematic) and the Footprint field of a symbol (`SelectFootprintFromLibrary`, assigning a footprint). The dialog only
// chooses; what is done with the choice is the caller's.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchFootprintDetails, fetchProjectFootprintDrawing, type FootprintDetails } from "../api/libraryChooserClient";
import { fetchPreviewPart } from "../api/model3dClient";
import type { LibraryFootprint, Part } from "../api/types";
import { rowsOfLibrary } from "../kicad-port/fpModels";
import { chosenOf, footprintFilterPatterns, passesFootprintFilters, urlIn, type ChooserItem, type ChooserRow } from "../kicad-port/libChooser";
import { recentItems, rememberChosen } from "../state/chooserRecent";
import { ChooserDetails, Linkified } from "./chooser/ChooserDetails";
import { FootprintPreview } from "./chooser/ChooserPreview";
import { ModelPreview3D } from "./footprint/ModelPreview3D";
import { LibChooserTree } from "./chooser/LibChooserTree";
import { chooserFocusGuard, useChooser } from "./chooser/useChooser";
import "../styles/chooser.css";

export type FootprintPick = { kind: "footprint"; name: string } | { kind: "unplaced"; ref: string };

export interface FootprintChooserProps {
  title?: string;
  /** `Lib:Name` to open on: the symbol's present footprint. */
  preselect?: string | null;
  /** Opened for a symbol: its pins (distinct numbers) offer "Filter by pin count". */
  pinCount?: number;
  /** Opened for a symbol: its `ki_fp_filters` offer "Apply footprint filters". */
  fpFilters?: readonly string[];
  /** Place Footprint: the schematic's parts that have no footprint on the board yet, a group of their own. */
  unplaced?: readonly { ref: string; label: string }[];
  onChoose: (pick: FootprintPick) => void;
  onCancel: () => void;
}

const FILTERS_KEY = "eda-studio.footprint-chooser-filters";
const VIEWS_KEY = "eda-studio.footprint-chooser-views";

/** `m_showFpMode` / `m_show3DMode`: which of the two previews are shown, kept between uses (the footprint's drawing starts on, the 3D view off, as in KiCad). */
function savedViews(): { fp: boolean; view3d: boolean } {
  try {
    const raw = JSON.parse(localStorage.getItem(VIEWS_KEY) ?? "{}") as { fp?: boolean; view3d?: boolean };
    const view3d = raw.view3d === true;
    return { fp: raw.fp !== false || !view3d, view3d };
  } catch {
    return { fp: true, view3d: false };
  }
}

function saveViews(v: { fp: boolean; view3d: boolean }): void {
  try {
    localStorage.setItem(VIEWS_KEY, JSON.stringify(v));
  } catch {
    /* it is a convenience */
  }
}

/** `m_FootprintChooser.filter_on_pin_count` / `use_fp_filters`, kept between uses (both start off, as in KiCad). */
function savedFilters(): { pins: boolean; patterns: boolean } {
  try {
    const raw = JSON.parse(localStorage.getItem(FILTERS_KEY) ?? "{}") as { pins?: boolean; patterns?: boolean };
    return { pins: raw.pins === true, patterns: raw.patterns === true };
  } catch {
    return { pins: false, patterns: false };
  }
}

function saveFilters(f: { pins: boolean; patterns: boolean }): void {
  try {
    localStorage.setItem(FILTERS_KEY, JSON.stringify(f));
  } catch {
    /* it is a convenience */
  }
}

export function FootprintChooserDialog({ title = "Footprint Chooser", preselect = null, pinCount, fpFilters, unplaced, onChoose, onCancel }: FootprintChooserProps) {
  const initial = useMemo(savedFilters, []);
  const [views, setViews] = useState(savedViews);
  const toggleView = (which: "fp" | "view3d") => {
    // `on3DviewReq` / `onFpViewReq`: a view is switched off only when the other one is on
    const next = { ...views, [which]: !views[which] };
    if (!next.fp && !next.view3d) return;
    setViews(next);
    saveViews(next);
    // `on3DviewReq` turning the 3D view on: the own window opens when its box is ticked, and closes when it is not
    if (which === "view3d" && next.view3d) setOwn3dOpen(own3dRef.current);
  };
  const own3dRef = useRef(false);
  const [byPins, setByPins] = useState(initial.pins);
  const [byPatterns, setByPatterns] = useState(initial.patterns);
  const patterns = useMemo(() => footprintFilterPatterns(fpFilters), [fpFilters]);
  const pinsOn = byPins && !!pinCount && pinCount > 0;
  const patternsOn = byPatterns && patterns.length > 0;

  const filter = useMemo(() => ({ pins: pinsOn ? pinCount : undefined, fpFilters: patternsOn ? patterns : undefined }), [pinsOn, pinCount, patternsOn, patterns]);
  const keep = useCallback(
    (item: ChooserItem) => {
      if (item.id.startsWith("ref:")) return true;
      if (pinsOn && item.pins !== undefined && item.pins !== pinCount) return false;
      return !patternsOn || passesFootprintFilters(patterns, item.lib, item.name);
    },
    [pinsOn, pinCount, patternsOn, patterns]
  );
  const extra = useMemo(
    () => (unplaced && unplaced.length > 0 ? { label: "-- Unplaced --", items: unplaced.map((u): ChooserItem => ({ id: `ref:${u.ref}`, lib: "", name: u.ref, description: u.label })) } : undefined),
    [unplaced]
  );
  const recent = recentItems("footprint");
  // The footprint to open on: the one asked for, else the one last used (`SetPreselectNode( historyInfos[0] )`).
  const chooser = useChooser({ kind: "footprint", active: true, recent, extra, filter, keep, preselect: preselect ?? recent[0]?.id ?? null });

  // The selected footprint's drawing and description, loaded for this one only.
  const chosen = chosenOf(chooser.selectedRow);
  const selectedId = chosen && !chosen.item.id.startsWith("ref:") ? chosen.item.id : null;
  const [details, setDetails] = useState<{ id: string; info: FootprintDetails | null; drawing: LibraryFootprint | null; error: string | null } | null>(null);
  useEffect(() => {
    if (!selectedId) {
      setDetails(null);
      return;
    }
    let cancelled = false;
    fetchFootprintDetails(selectedId)
      .then((info) => !cancelled && setDetails({ id: selectedId, info, drawing: info.footprint, error: null }))
      .catch(() =>
        fetchProjectFootprintDrawing(selectedId)
          .then((drawing) => !cancelled && setDetails({ id: selectedId, info: null, drawing, error: null }))
          .catch((e) => !cancelled && setDetails({ id: selectedId, info: null, drawing: null, error: e instanceof Error ? e.message : String(e) }))
      );
    return () => {
      cancelled = true;
    };
  }, [selectedId]);
  const shown = details && details.id === selectedId ? details : null;
  // The 3D view of the selected footprint: its own geometry under its models, the way the tab of Footprint Properties shows them.
  const [preview3d, setPreview3d] = useState<{ id: string; part: Part | null } | null>(null);
  useEffect(() => {
    if (!views.view3d || !selectedId) {
      setPreview3d(null);
      return;
    }
    let cancelled = false;
    void fetchPreviewPart({ footprint: selectedId }).then((part) => !cancelled && setPreview3d({ id: selectedId, part }));
    return () => {
      cancelled = true;
    };
  }, [views.view3d, selectedId]);
  const models3d = useMemo(() => (shown?.drawing ? rowsOfLibrary(shown.drawing) : []), [shown?.drawing]);
  // "Show 3D viewer in own window": the box, and whether the window is up (`Get3DViewerFrame()`): ticking the box opens it when the 3D view is on (`if(
  // m_grButton3DView->IsChecked() ) Show3DViewerFrame()`), turning the 3D view on with the box ticked opens it too (`on3DviewReq`), and unticking it closes it.
  const [own3d, setOwn3d] = useState(false);
  const [own3dOpen, setOwn3dOpen] = useState(false);
  const external3d = own3dOpen && own3d;
  // the window's 3D view reads the selected footprint as the pane's does
  const [external, setExternal] = useState<{ id: string; part: Part | null } | null>(null);
  useEffect(() => {
    if (!external3d || !selectedId) {
      setExternal(null);
      return;
    }
    let cancelled = false;
    void fetchPreviewPart({ footprint: selectedId }).then((part) => !cancelled && setExternal({ id: selectedId, part }));
    return () => {
      cancelled = true;
    };
  }, [external3d, selectedId]);

  const choose = useCallback(
    (row: ChooserRow | undefined) => {
      const c = chosenOf(row);
      if (!c) {
        if (row?.kind === "group") chooser.toggleGroup(row.group.lib);
        return;
      }
      if (c.item.id.startsWith("ref:")) return onChoose({ kind: "unplaced", ref: c.item.name });
      rememberChosen("footprint", { ...c.item, pins: details?.id === c.item.id ? (details.info?.numbered_pads ?? c.item.pins) : c.item.pins });
      onChoose({ kind: "footprint", name: c.item.id });
    },
    [chooser, details, onChoose]
  );

  const root = useRef<HTMLDivElement>(null);
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      chooser.move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(chooser.selectedRow);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      // First Escape clears the search, the second closes (`PANEL_FOOTPRINT_CHOOSER::OnChar`).
      if (chooser.query !== "") chooser.setQuery("");
      else onCancel();
    }
  };

  const s = chooser.status;
  const hint = s.error ? `Error: ${s.error}` : s.searching ? (s.indexing ? (s.total ? `Searching… ${s.indexed} of ${s.total} libraries read` : "Searching…") : `${s.matches} match${s.matches === 1 ? "" : "es"}${s.truncated ? " (showing the best)" : ""}`) : "";
  const info = shown?.info;
  const doc = info && urlIn(info.description);

  return (
    // Rendered inside the dialog that opened it (Symbol Properties, Assign Footprints): a click or a key in here is not that dialog's.
    <div className="dialog-backdrop" data-footprint-chooser onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <div className="dialog chooser-dialog" ref={root} onKeyDown={onKeyDown} {...chooserFocusGuard} role="dialog" aria-label={title}>
        <div className="dialog-header">
          <span>
            {title}
            {chooser.libraries > 0 ? ` (${chooser.libraries} libraries)` : ""}
          </span>
        </div>
        <div className="dialog-body chooser-body">
          <div className="chooser-left">
            <div className="chooser-search">
              <input id="library-tree-search" data-chooser-search autoFocus placeholder="Search footprints (name, description, keywords)" value={chooser.query} onChange={(e) => chooser.setQuery(e.target.value)} />
              <div className="chooser-hint" data-chooser-status data-chooser-answered={s.answered ?? undefined}>
                {hint}
              </div>
            </div>
            {(pinCount ?? 0) > 0 || patterns.length > 0 ? (
              <div className="chooser-footprint-filters">
                {(pinCount ?? 0) > 0 && (
                  <label>
                    <input
                      type="checkbox"
                      checked={byPins}
                      onChange={(e) => {
                        setByPins(e.target.checked);
                        saveFilters({ pins: e.target.checked, patterns: byPatterns });
                      }}
                    />
                    Filter by pin count ({pinCount})
                  </label>
                )}
                {patterns.length > 0 && (
                  <label>
                    <input
                      type="checkbox"
                      checked={byPatterns}
                      onChange={(e) => {
                        setByPatterns(e.target.checked);
                        saveFilters({ pins: byPins, patterns: e.target.checked });
                      }}
                    />
                    Apply footprint filters ({patterns.join(" ")})
                  </label>
                )}
              </div>
            ) : null}
            <LibChooserTree
              kind="footprint"
              rows={chooser.rows}
              selectedKey={chooser.selectedKey}
              loading={chooser.loading}
              onSelect={chooser.select}
              onChoose={choose}
              onToggleGroup={chooser.toggleGroup}
              onToggleItem={chooser.toggleItem}
              emptyText={s.searching ? (s.indexing ? "Searching…" : "No footprint matches.") : chooser.libraries === 0 ? "No footprint library is installed." : "Nothing to show."}
            />
          </div>
          <div className="chooser-right">
            <div style={{ display: "flex", gap: 4, justifyContent: "flex-end", fontSize: 11 }} data-chooser-views>
              <button
                title="Show/hide 3D view panel"
                aria-pressed={views.view3d}
                style={{ fontWeight: views.view3d ? 700 : 400, borderBottom: views.view3d ? "2px solid var(--chrome-accent, #4a90e2)" : "2px solid transparent" }}
                onClick={() => toggleView("view3d")}
                data-chooser-view-3d
              >
                3D
              </button>
              <button
                title="Show/hide footprint view panel"
                aria-pressed={views.fp}
                style={{ fontWeight: views.fp ? 700 : 400, borderBottom: views.fp ? "2px solid var(--chrome-accent, #4a90e2)" : "2px solid transparent" }}
                onClick={() => toggleView("fp")}
                data-chooser-view-fp
              >
                Footprint
              </button>
            </div>
            {views.fp && <FootprintPreview footprint={shown?.drawing ?? null} status={!selectedId ? (chosen ? "Unplaced part from the schematic" : "No footprint selected") : shown?.error ? shown.error : shown ? "" : "Loading…"} />}
            {views.view3d &&
              (selectedId ? (
                <ModelPreview3D part={preview3d && preview3d.id === selectedId ? preview3d.part : null} rows={models3d} height={230} testId="chooser-preview-3d" />
              ) : (
                <div style={{ height: 230, display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid var(--chrome-border)", fontSize: 12, color: "var(--chrome-text-dim)" }} data-chooser-3d-empty>
                  No footprint selected
                </div>
              ))}
            {chosen && chosen.item.id.startsWith("ref:") ? (
              <ChooserDetails name={chosen.item.name} description={chosen.item.description} rows={[]} />
            ) : selectedId && chosen ? (
              <ChooserDetails
                name={chosen.item.name}
                description={info?.description ?? chosen.item.description}
                rows={[
                  ...(info?.tags ? [{ name: "Keywords", value: info.tags }] : []),
                  ...(doc ? [{ name: "Documentation", value: <Linkified text={doc} max={72} /> }] : []),
                  ...(info ? [{ name: "Pads", value: `${info.pads}${info.numbered_pads !== info.pads ? ` (${info.numbered_pads} numbered)` : ""}` }] : []),
                ]}
              />
            ) : null}
          </div>
        </div>
        <div className="dialog-footer">
          <label style={{ marginRight: "auto" }}>
            <input
              type="checkbox"
              checked={own3d}
              onChange={(e) => {
                const on = e.target.checked;
                own3dRef.current = on;
                setOwn3d(on);
                // `onExternalViewer3DEnable`: ticked, the window opens if the 3D view is on; unticked, it closes
                setOwn3dOpen(on && views.view3d);
              }}
              data-chooser-own-3d
            />{" "}
            Show 3D viewer in own window
          </label>
          <button onClick={onCancel} data-chooser-cancel>
            Cancel
          </button>
          <button className="primary" disabled={!chosen} onClick={() => choose(chooser.selectedRow)} data-chooser-ok>
            OK
          </button>
        </div>
      </div>
      {external3d && <Viewer3DWindow id={selectedId} part={external && external.id === selectedId ? external.part : null} rows={models3d} onClose={() => setOwn3dOpen(false)} />}
    </div>
  );
}

/** `EDA_3D_VIEWER_FRAME` as the Footprint Chooser opens it: "3D Viewer — Lib:Name" (`FOOTPRINT_CHOOSER_FRAME::Update3DView`), a window moved by its title bar and
 *  sized by its corner, closed by its close box. */
function Viewer3DWindow({ id, part, rows, onClose }: { id: string | null; part: Part | null; rows: ReturnType<typeof rowsOfLibrary>; onClose: () => void }) {
  const [pos, setPos] = useState(() => ({ x: Math.max(16, window.innerWidth - 680), y: 60 }));
  const [size, setSize] = useState({ w: 600, h: 460 });
  const drag = useRef<{ kind: "move" | "size"; x: number; y: number; from: { x: number; y: number } } | null>(null);
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (d.kind === "move") setPos({ x: Math.max(0, d.from.x + dx), y: Math.max(0, d.from.y + dy) });
    else setSize({ w: Math.max(320, d.from.x + dx), h: Math.max(240, d.from.y + dy) });
  };
  const start = (kind: "move" | "size") => (e: React.PointerEvent) => {
    e.stopPropagation();
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
    drag.current = { kind, x: e.clientX, y: e.clientY, from: kind === "move" ? pos : { x: size.w, y: size.h } };
  };
  return (
    <div
      className="dialog"
      role="dialog"
      aria-label="3D Viewer"
      data-chooser-3d-window
      style={{ position: "fixed", left: pos.x, top: pos.y, width: size.w, zIndex: 1100, display: "flex", flexDirection: "column", boxShadow: "0 4px 18px rgba(0,0,0,0.45)" }}
      onClick={(e) => e.stopPropagation()}
      onPointerMove={onPointerMove}
      onPointerUp={() => (drag.current = null)}
    >
      <div className="dialog-header" style={{ cursor: "move", display: "flex", alignItems: "center" }} onPointerDown={start("move")}>
        <span style={{ flex: 1 }} data-chooser-3d-window-title>{`3D Viewer \u2014 ${id ?? ""}`}</span>
        <button aria-label="Close 3D Viewer" onPointerDown={(e) => e.stopPropagation()} onClick={onClose} data-chooser-3d-window-close>
          {"×"}
        </button>
      </div>
      <div style={{ padding: 6 }}>
        {id ? (
          <ModelPreview3D part={part} rows={rows} height={size.h - 60} testId="chooser-3d-window" />
        ) : (
          <div style={{ height: size.h - 60, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 12, color: "var(--chrome-text-dim)" }}>No footprint selected</div>
        )}
      </div>
      <div style={{ position: "absolute", right: 0, bottom: 0, width: 14, height: 14, cursor: "nwse-resize" }} onPointerDown={start("size")} aria-hidden />
    </div>
  );
}
