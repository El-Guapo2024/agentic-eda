// Assign Footprints... (`eeschema.EditorControl.assignFootprints` -> `SCH_EDIT_FRAME::OnOpenCvpcb`): CvPcb's job in one dialog -- the symbols on the left with
// the footprint each has, the libraries in the middle, the footprints on the right narrowed by `FOOTPRINT_FILTER` (`kicad-port/footprintFilter.ts`: the
// symbol's own footprint filters, its pin count, the library, the words typed). Double-click a footprint, or Assign, to give it to the selected symbols;
// OK applies every assignment as one undo step (`AssignFootprints`'s single "Assign Footprints" commit).
//
// CvPcb's footprint viewer (`CVPCB_ACTIONS::showFootprintViewer`, "View Selected Footprint" -> `DISPLAY_FOOTPRINTS_FRAME`, KiCad 25ec2335) is a window of its own,
// closed when CvPcb opens and opened by View Selected Footprint (UI-LEFT-2 LL7): here a window over the page, moved by its caption and closed by its button (a page
// cannot open a window of the operating system's). It shows the footprint picked in the footprint list, else the selected symbol's (`InitDisplay`), drawn with the
// Footprint Editor's painter, titled "Footprint: %s" with "Lib: %s",
// or `GetFootprint`'s message when it cannot be drawn. Automatically Assign Footprints (`CVPCB_ACTIONS::autoAssociate` -> `AutomaticFootprintMatching`) reads the project's
// footprint association files (Manage Footprint Association Files..., `DIALOG_CONFIG_EQUFILES`: EquFilesDialog.tsx) on the server and stages what it assigns with the
// dialog's other assignments; its messages are KiCad's ("%lu footprint/symbol equivalences found.", "Equivalence File Load Error", "CvPcb Warning"). CvPcb's hotkeys
// (`CVPCB_ACTIONS`, UI-LEFT-2 LL7): Tab and Shift+Tab move between the libraries, the symbols and the footprints (`ChangeFocus`), Enter assigns the selected footprint
// to the selected symbols and selects the next symbol without one (`Associate`, `gotoNextNA`), Delete takes the selected symbols' footprints away (`DeleteAssoc`),
// Ctrl+S applies the assignments to the schematic and stays open (`SaveAssociationsToSchematic`), the arrow keys move in the list that has the focus. Not ported:
// the viewer's 3D view (the Footprint Chooser has one).
import { useEffect, useMemo, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { fetchFootprintLibraryNames } from "../../api/client";
import { fetchAnyFootprint, fetchAnySymbol } from "../../api/libraryClient";
import { autoAssociate, fetchCvpcbFootprint, type CvpcbFootprint } from "../../api/cvpcbClient";
import type { Cmd } from "../../api/types";
import { filterFootprints, toCandidate, uniquePadCount, uniquePinCount, type FootprintCandidate } from "../../kicad-port/footprintFilter";
import { autoAssociateInput, changeFocus, cvpcbHotkey, nextUnassigned, viewerCaption, viewerFootprint, type CvpcbControl } from "../../kicad-port/cvpcbEqu";
import { expandStackedPinNotation } from "../../kicad-port/stackedPins";
import { FootprintChooserDialog } from "../FootprintChooserDialog";
import { FootprintPreview } from "../chooser/ChooserPreview";
import { EquFilesDialog } from "./EquFilesDialog";
import { SchDialogFrame } from "./SchDialogFrame";

interface Row {
  ref: string;
  value: string;
  footprint: string;
  libId: string;
}

const naturalRef = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true });

export function AssignFootprintsDialog({ onClose }: { onClose: () => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();

  const rows: Row[] = useMemo(() => {
    const seen = new Map<string, Row>();
    for (const s of state.schematic?.symbols ?? []) {
      if (!seen.has(s.id)) seen.set(s.id, { ref: s.id, value: s.value ?? "", footprint: s.footprint ?? "", libId: s.lib_id ?? "" });
    }
    return [...seen.values()].sort((a, b) => naturalRef(a.ref, b.ref));
  }, [state.schematic]);

  const [staged, setStaged] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);
  const [names, setNames] = useState<string[]>([]);
  const [padCounts, setPadCounts] = useState<Record<string, number>>({});
  const [library, setLibrary] = useState("");
  const [bySymbol, setBySymbol] = useState(true);
  const [byPins, setByPins] = useState(false);
  const [text, setText] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [symInfo, setSymInfo] = useState<{ filters: string[]; pins: number } | null>(null);
  // "From KiCad's libraries...": the Footprint Chooser over every installed library, narrowed by the first selected symbol like the list here.
  const [choosing, setChoosing] = useState(false);
  // "View Selected Footprint": the viewer window (`DISPLAY_FOOTPRINTS_FRAME`), not open when CvPcb opens; where it is on the page
  const [viewer, setViewer] = useState(false);
  const [viewerAt, setViewerAt] = useState<{ x: number; y: number }>(() => ({ x: Math.max(16, window.innerWidth - 440), y: 80 }));
  const viewerDrag = useRef<{ dx: number; dy: number } | null>(null);
  // the list with the keyboard (`GetFocusedControl`)
  const [focused, setFocused] = useState<CvpcbControl | null>(null);
  const listRefs = { library: useRef<HTMLDivElement>(null), component: useRef<HTMLDivElement>(null), footprint: useRef<HTMLDivElement>(null) };
  const [shownFp, setShownFp] = useState<{ id: string; reply: CvpcbFootprint | null } | null>(null);
  // Manage Footprint Association Files..., and what the last Automatically Assign Footprints said
  const [equOpen, setEquOpen] = useState(false);
  const equOpenRef = useRef(false);
  equOpenRef.current = equOpen;
  const [autoSays, setAutoSays] = useState<{ status: string; loadErrors: string | null; warnings: string | null } | null>(null);
  const [autoBusy, setAutoBusy] = useState(false);

  const current = (r: Row) => staged[r.ref] ?? r.footprint;

  useEffect(() => {
    let alive = true;
    fetchFootprintLibraryNames().then(
      (r) => alive && setNames(r.names),
      () => undefined
    );
    return () => {
      alive = false;
    };
  }, []);

  // The first selected symbol's own footprint filters and pin count (`GetFootprintFilters`, `GetPinCount`).
  const firstRow = rows.find((r) => r.ref === selected[0]);
  useEffect(() => {
    let alive = true;
    setSymInfo(null);
    if (!firstRow?.libId) return;
    fetchAnySymbol(firstRow.libId).then(
      ({ symbol }) => alive && setSymInfo({ filters: symbol.footprint_filters, pins: uniquePinCount(symbol.pins, (n) => expandStackedPinNotation(n).numbers) }),
      () => undefined
    );
    return () => {
      alive = false;
    };
  }, [firstRow?.libId]);

  // A pin-count filter needs every candidate's pad count, which is only known by reading the footprint: read them once, when the filter is turned on.
  useEffect(() => {
    if (!byPins) return;
    let alive = true;
    (async () => {
      for (const id of names) {
        if (!alive) return;
        if (padCounts[id] !== undefined) continue;
        try {
          const { footprint } = await fetchAnyFootprint(id);
          if (alive) setPadCounts((p) => ({ ...p, [id]: uniquePadCount(footprint.pads) }));
        } catch {
          /* a footprint that cannot be read has no pad count and never passes the filter */
        }
      }
    })();
    return () => {
      alive = false;
    };
    // padCounts is read through the functional updater below; re-running on every count would restart the loop
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [byPins, names]);

  const candidates: FootprintCandidate[] = useMemo(() => names.map((n) => toCandidate(n, padCounts[n] ?? null)), [names, padCounts]);
  const libraries = useMemo(() => [...new Set(candidates.map((c) => c.lib).filter((l) => l !== ""))].sort(), [candidates]);
  const shown = useMemo(
    () => filterFootprints(candidates, { symbolFilters: bySymbol && symInfo && symInfo.filters.length > 0 ? symInfo.filters : null, pinCount: byPins && symInfo ? symInfo.pins : null, library, text }),
    [candidates, bySymbol, byPins, symInfo, library, text]
  );

  const clickRow = (ref: string, e: React.MouseEvent) => {
    if (e.shiftKey && anchor.current) {
      const a = rows.findIndex((r) => r.ref === anchor.current);
      const b = rows.findIndex((r) => r.ref === ref);
      setSelected(rows.slice(Math.min(a, b), Math.max(a, b) + 1).map((r) => r.ref));
    } else if (e.ctrlKey || e.metaKey) {
      setSelected((s) => (s.includes(ref) ? s.filter((x) => x !== ref) : [...s, ref]));
      anchor.current = ref;
    } else {
      setSelected([ref]);
      anchor.current = ref;
    }
    // `OnSelectComponent`: the footprint list selects the symbol's footprint (`SetSelectedFootprint`) or nothing, and the viewer follows (`RefreshFootprintViewer`)
    const row = rows.find((r) => r.ref === ref);
    const fp = row ? current(row) : "";
    setPicked(fp && shown.some((c) => c.id === fp) ? fp : null);
  };

  const assign = (footprint: string) => {
    if (selected.length === 0) return;
    setStaged((s) => ({ ...s, ...Object.fromEntries(selected.map((r) => [r, footprint])) }));
  };

  // `DISPLAY_FOOTPRINTS_FRAME::InitDisplay`: the footprint picked in the list, else the selected symbol's
  const viewId = viewerFootprint(picked, firstRow ? current(firstRow) : null);
  useEffect(() => {
    if (!viewer) return;
    if (!viewId) {
      setShownFp(null);
      return;
    }
    let alive = true;
    fetchCvpcbFootprint(viewId).then(
      (reply) => alive && setShownFp({ id: viewId, reply }),
      (e) => alive && setShownFp({ id: viewId, reply: { footprint: null, error: e instanceof Error ? e.message : String(e) } })
    );
    return () => {
      alive = false;
    };
  }, [viewId, viewer]);
  const viewed = shownFp && shownFp.id === viewId ? shownFp.reply : null;
  const caption = viewerCaption(viewId, viewed?.footprint ? viewed.lib : undefined);

  /** `CVPCB_ASSOCIATION_TOOL::AutoAssociate` -> `AutomaticFootprintMatching`: the server reads the association files and says what to assign; it is staged here. */
  const runAutoAssociate = async () => {
    setAutoBusy(true);
    try {
      const r = await autoAssociate(autoAssociateInput(rows, staged));
      if (r.error) return setAutoSays({ status: "", loadErrors: r.error, warnings: null });
      if (r.assignments.length > 0) setStaged((s) => ({ ...s, ...Object.fromEntries(r.assignments.map((a) => [a.ref, a.footprint])) }));
      setAutoSays({ status: r.status, loadErrors: r.load_errors, warnings: r.warnings });
    } catch (e) {
      setAutoSays({ status: "", loadErrors: e instanceof Error ? e.message : String(e), warnings: null });
    } finally {
      setAutoBusy(false);
    }
  };

  /** `SetSelectedComponent`: one symbol selected, as a click selects it (the footprint list and the viewer follow). */
  const selectSymbol = (ref: string) => {
    setSelected([ref]);
    anchor.current = ref;
    const row = rows.find((r) => r.ref === ref);
    const fp = row ? current(row) : "";
    setPicked(fp && shown.some((c) => c.id === fp) ? fp : null);
    listRefs.component.current?.querySelector(`[data-cvpcb-symbol="${CSS.escape(ref)}"]`)?.scrollIntoView({ block: "nearest" });
  };

  /** CvPcb's hotkeys (the module doc); the arrow keys move in the list that has the focus. */
  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).closest("input, textarea, select")) return;
    const hk = cvpcbHotkey(e);
    if (hk === "focusRight" || hk === "focusLeft") {
      const to = changeFocus(focused, hk === "focusRight" ? "right" : "left");
      if (!to) return;
      e.preventDefault();
      listRefs[to].current?.focus();
    } else if (hk === "associate") {
      if (!picked || selected.length === 0) return;
      e.preventDefault();
      const after = { ...staged, ...Object.fromEntries(selected.map((r) => [r, picked])) };
      assign(picked);
      const next = nextUnassigned(rows, (ref) => after[ref] ?? rows.find((r) => r.ref === ref)?.footprint ?? "", selected);
      if (next) selectSymbol(next);
    } else if (hk === "deleteAssoc") {
      if (selected.length === 0) return;
      e.preventDefault();
      assign("");
    } else if (hk === "save") {
      e.preventDefault();
      void submit(false);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const d = e.key === "ArrowDown" ? 1 : -1;
      if (focused === "component" && rows.length > 0) {
        e.preventDefault();
        const at = rows.findIndex((r) => r.ref === selected[0]);
        selectSymbol(rows[Math.min(rows.length - 1, Math.max(0, at < 0 ? 0 : at + d))]!.ref);
      } else if (focused === "footprint" && shown.length > 0) {
        e.preventDefault();
        const at = shown.findIndex((c) => c.id === picked);
        const next = shown[Math.min(shown.length - 1, Math.max(0, at < 0 ? 0 : at + d))]!.id;
        setPicked(next);
        listRefs.footprint.current?.querySelector(`[data-cvpcb-fp="${CSS.escape(next)}"]`)?.scrollIntoView({ block: "nearest" });
      } else if (focused === "library") {
        e.preventDefault();
        const all = ["", ...libraries];
        const at = all.indexOf(library);
        setLibrary(all[Math.min(all.length - 1, Math.max(0, at + d))]!);
      }
    }
  };

  const changes = rows.filter((r) => staged[r.ref] !== undefined && staged[r.ref] !== r.footprint);

  /** OK, and Ctrl+S (`SaveAssociationsToSchematic`, which leaves CvPcb open: `close` false): every staged assignment as one undo step. */
  const submit = async (close = true) => {
    if (changes.length === 0) return close ? onClose() : undefined;
    const cmds: Cmd[] = changes.map((r) => ({ op: "edit_symbol_fields", id: r.ref, footprint: staged[r.ref]! }));
    if (await api.cmdBatch(cmds)) {
      dispatch({ type: "TOAST", message: `Assigned ${changes.length} footprint${changes.length === 1 ? "" : "s"}.`, kind: "info" });
      if (close) onClose();
      else setStaged({});
    }
  };

  const cell = { padding: "2px 6px", whiteSpace: "nowrap" as const, overflow: "hidden", textOverflow: "ellipsis" };
  const listBox = { border: "1px solid var(--chrome-border)", height: "44vh", overflow: "auto", fontSize: 12 };

  return (
    <SchDialogFrame
      title="Assign Footprints"
      width={1000}
      // Escape and a click outside close the Manage Footprint Association Files dialog first
      onClose={() => !equOpenRef.current && onClose()}
      footer={
        <button className="primary" onClick={() => void submit(true)}>
          {changes.length > 0 ? `Apply ${changes.length} assignment${changes.length === 1 ? "" : "s"}` : "OK"}
        </button>
      }
    >
      <div style={{ display: "flex", gap: 10, alignItems: "stretch" }} onKeyDown={onKeyDown} data-cvpcb-lists>
        <div style={{ flex: "1.3 1 0", minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Symbols ({rows.length})</div>
          <div style={{ ...listBox, outline: focused === "component" ? "1px solid var(--chrome-accent, #4a90d9)" : undefined }} tabIndex={0} ref={listRefs.component} onFocus={() => setFocused("component")} data-cvpcb-list="component">
            <table style={{ width: "100%", borderCollapse: "collapse", tableLayout: "fixed" }}>
              <colgroup>
                <col style={{ width: "18%" }} />
                <col style={{ width: "22%" }} />
                <col />
              </colgroup>
              <tbody>
                {rows.map((r) => {
                  const fp = current(r);
                  const isStaged = staged[r.ref] !== undefined && staged[r.ref] !== r.footprint;
                  return (
                    <tr key={r.ref} data-cvpcb-symbol={r.ref} className={selected.includes(r.ref) ? "armed" : undefined} style={{ cursor: "default", background: selected.includes(r.ref) ? "var(--chrome-selected-bg)" : undefined, color: selected.includes(r.ref) ? "var(--chrome-selected-text)" : undefined }} onClick={(e) => clickRow(r.ref, e)}>
                      <td style={cell}>{r.ref}</td>
                      <td style={cell}>{r.value}</td>
                      <td data-cvpcb-assigned style={{ ...cell, fontStyle: isStaged ? "italic" : undefined, fontWeight: isStaged ? 700 : undefined }} title={fp}>
                        {fp || <span style={{ opacity: 0.5 }}>(none)</span>}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
        <div style={{ flex: "0.7 1 0", minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Libraries</div>
          <div style={{ ...listBox, outline: focused === "library" ? "1px solid var(--chrome-accent, #4a90d9)" : undefined }} tabIndex={0} ref={listRefs.library} onFocus={() => setFocused("library")} data-cvpcb-list="library">
            {["", ...libraries].map((l) => (
              <div key={l || "(all)"} style={{ ...cell, cursor: "default", background: library === l ? "var(--chrome-selected-bg)" : undefined, color: library === l ? "var(--chrome-selected-text)" : undefined }} onClick={() => setLibrary(l)}>
                {l || "All libraries"}
              </div>
            ))}
          </div>
        </div>
        <div style={{ flex: "1.4 1 0", minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Footprints ({shown.length})</div>
          <div style={{ ...listBox, outline: focused === "footprint" ? "1px solid var(--chrome-accent, #4a90d9)" : undefined }} tabIndex={0} ref={listRefs.footprint} onFocus={() => setFocused("footprint")} data-cvpcb-list="footprint">
            {shown.map((c) => (
              <div key={c.id} data-cvpcb-fp={c.id} title={c.id} style={{ ...cell, cursor: "default", background: picked === c.id ? "var(--chrome-selected-bg)" : undefined, color: picked === c.id ? "var(--chrome-selected-text)" : undefined }} onClick={() => setPicked(c.id)} onDoubleClick={() => assign(c.id)}>
                {c.id}
                {c.padCount !== null && <small style={{ opacity: 0.6 }}> · {c.padCount} pads</small>}
              </div>
            ))}
            {shown.length === 0 && (
              <div className="panel-empty">{names.length === 0 ? "This project has no footprints yet. Open or draw one in the Footprint Editor, then assign it here." : "No footprint matches the filters."}</div>
            )}
          </div>
        </div>
      </div>
      {viewer && (
        // the footprint viewer (`DISPLAY_FOOTPRINTS_FRAME`): a window of its own, over the page, moved by its caption (`SetTitle( "Footprint: %s" )`)
        <div
          data-cvpcb-viewer={viewId}
          role="dialog"
          aria-label="Footprint Viewer"
          style={{ position: "fixed", left: viewerAt.x, top: viewerAt.y, width: 400, zIndex: 1000, background: "var(--chrome-bg)", border: "1px solid var(--chrome-border)", boxShadow: "0 6px 24px rgba(0,0,0,0.35)", display: "flex", flexDirection: "column" }}
          onClick={(e) => e.stopPropagation()}
        >
          <div
            style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 6, fontWeight: 600, fontSize: 11, padding: "4px 6px", cursor: "move", userSelect: "none", borderBottom: "1px solid var(--chrome-border)" }}
            onPointerDown={(e) => {
              if ((e.target as HTMLElement).closest("button")) return;
              viewerDrag.current = { dx: e.clientX - viewerAt.x, dy: e.clientY - viewerAt.y };
              e.currentTarget.setPointerCapture(e.pointerId);
            }}
            onPointerMove={(e) => {
              const d = viewerDrag.current;
              if (d) setViewerAt({ x: Math.max(0, Math.min(window.innerWidth - 60, e.clientX - d.dx)), y: Math.max(0, Math.min(window.innerHeight - 30, e.clientY - d.dy)) });
            }}
            onPointerUp={() => (viewerDrag.current = null)}
          >
            <span data-cvpcb-viewer-title style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={caption.title}>
              {caption.title || "Footprint"}
            </span>
            <span data-cvpcb-viewer-status style={{ fontWeight: 400, opacity: 0.8 }}>
              {caption.status}
            </span>
            <button onClick={() => setViewer(false)} title="Close" data-cvpcb-viewer-close style={{ padding: "0 6px" }}>
              {"×"}
            </button>
          </div>
          <FootprintPreview footprint={viewed?.footprint ?? null} height={300} status={!viewId ? "No footprint selected" : viewed ? (viewed.error ?? "") : "Loading…"} />
        </div>
      )}
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 14, marginTop: 8, fontSize: 12 }}>
        <label title="Show only the footprints the selected symbol's footprint filters allow">
          <input type="checkbox" checked={bySymbol} onChange={(e) => setBySymbol(e.target.checked)} /> Filter by the symbol's footprint filters
        </label>
        <label title={symInfo ? `The selected symbol has ${symInfo.pins} pins` : "Select a symbol first"}>
          <input type="checkbox" checked={byPins} onChange={(e) => setByPins(e.target.checked)} /> Filter by pin count{symInfo ? ` (${symInfo.pins})` : ""}
        </label>
        <input placeholder="Search footprints" value={text} onChange={(e) => setText(e.target.value)} style={{ flex: 1, minWidth: 140 }} />
        <button disabled={selected.length === 0} onClick={() => setChoosing(true)} title="Choose a footprint from KiCad's installed libraries (the Footprint Chooser)" data-assign-from-libraries>
          From KiCad's libraries…
        </button>
        <button disabled={!picked || selected.length === 0} onClick={() => picked && assign(picked)}>
          Assign
        </button>
        <button disabled={selected.length === 0} onClick={() => assign("")} title="Remove the footprint from the selected symbols (Delete)">
          Remove assignment
        </button>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 10, marginTop: 6, fontSize: 12 }}>
        <button disabled={autoBusy || rows.length === 0} onClick={() => void runAutoAssociate()} title="Assign footprints automatically using a footprint association file" data-cvpcb-auto>
          Automatically Assign Footprints
        </button>
        <button onClick={() => setEquOpen(true)} title="Edit the footprint association files list.  These files are used to automatically assign footprints based on symbol values." data-cvpcb-equ>
          Manage Footprint Association Files…
        </button>
        <label title="View Selected Footprint">
          <input type="checkbox" checked={viewer} onChange={(e) => setViewer(e.target.checked)} data-cvpcb-viewer-toggle /> View Selected Footprint
        </label>
        <span style={{ opacity: 0.6 }} title="CvPcb's hotkeys">
          Tab / Shift+Tab: next list · Enter: assign · Delete: remove · Ctrl+S: save to schematic
        </span>
        {autoSays?.status && (
          <span data-cvpcb-status style={{ opacity: 0.8 }}>
            {autoSays.status}
          </span>
        )}
      </div>
      {autoSays?.loadErrors && (
        <div role="alert" data-cvpcb-load-errors style={{ marginTop: 6, fontSize: 12, whiteSpace: "pre-wrap" }}>
          <b>Equivalence File Load Error</b>
          {"\n"}
          {autoSays.loadErrors}
        </div>
      )}
      {autoSays?.warnings && (
        <div role="alert" data-cvpcb-warnings style={{ marginTop: 6, fontSize: 12, whiteSpace: "pre-wrap" }}>
          <b>CvPcb Warning</b>
          {"\n"}
          {autoSays.warnings}
        </div>
      )}
      {equOpen && <EquFilesDialog onClose={() => setEquOpen(false)} />}
      {choosing && (
        <FootprintChooserDialog
          preselect={firstRow && current(firstRow).includes(":") ? current(firstRow) : null}
          pinCount={symInfo?.pins}
          fpFilters={symInfo?.filters}
          onCancel={() => setChoosing(false)}
          onChoose={(pick) => {
            setChoosing(false);
            if (pick.kind === "footprint") assign(pick.name);
          }}
        />
      )}
    </SchDialogFrame>
  );
}
