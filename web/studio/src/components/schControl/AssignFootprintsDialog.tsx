// Assign Footprints... (`eeschema.EditorControl.assignFootprints` -> `SCH_EDIT_FRAME::OnOpenCvpcb`): CvPcb's job in one dialog -- the symbols on the left with
// the footprint each has, the libraries in the middle, the footprints on the right narrowed by `FOOTPRINT_FILTER` (`kicad-port/footprintFilter.ts`: the
// symbol's own footprint filters, its pin count, the library, the words typed). Double-click a footprint, or Assign, to give it to the selected symbols;
// OK applies every assignment as one undo step (`AssignFootprints`'s single "Assign Footprints" commit).
//
// CvPcb's footprint viewer (`CVPCB_ACTIONS::showFootprintViewer`, "View Selected Footprint" -> `DISPLAY_FOOTPRINTS_FRAME`, KiCad 25ec2335) is the pane under the lists:
// the footprint picked in the footprint list, else the selected symbol's (`InitDisplay`), drawn with the Footprint Editor's painter, titled "Footprint: %s" with "Lib: %s",
// or `GetFootprint`'s message when it cannot be drawn. Automatically Assign Footprints (`CVPCB_ACTIONS::autoAssociate` -> `AutomaticFootprintMatching`) reads the project's
// footprint association files (Manage Footprint Association Files..., `DIALOG_CONFIG_EQUFILES`: EquFilesDialog.tsx) on the server and stages what it assigns with the
// dialog's other assignments; its messages are KiCad's ("%lu footprint/symbol equivalences found.", "Equivalence File Load Error", "CvPcb Warning"). Not ported: the
// CvPcb hotkeys, and the viewer as a window of its own (it is a pane here; KiCad's 3D view of it is the Footprint Chooser's).
import { useEffect, useMemo, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { fetchFootprintLibraryNames } from "../../api/client";
import { fetchAnyFootprint, fetchAnySymbol } from "../../api/libraryClient";
import { autoAssociate, fetchCvpcbFootprint, type CvpcbFootprint } from "../../api/cvpcbClient";
import type { Cmd } from "../../api/types";
import { filterFootprints, toCandidate, uniquePadCount, uniquePinCount, type FootprintCandidate } from "../../kicad-port/footprintFilter";
import { autoAssociateInput, viewerCaption, viewerFootprint } from "../../kicad-port/cvpcbEqu";
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
  // "View Selected Footprint": the viewer pane, shown to begin with
  const [viewer, setViewer] = useState(true);
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

  const changes = rows.filter((r) => staged[r.ref] !== undefined && staged[r.ref] !== r.footprint);

  const submit = async () => {
    if (changes.length === 0) return onClose();
    const cmds: Cmd[] = changes.map((r) => ({ op: "edit_symbol_fields", id: r.ref, footprint: staged[r.ref]! }));
    if (await api.cmdBatch(cmds)) {
      dispatch({ type: "TOAST", message: `Assigned ${changes.length} footprint${changes.length === 1 ? "" : "s"}.`, kind: "info" });
      onClose();
    }
  };

  const cell = { padding: "2px 6px", whiteSpace: "nowrap" as const, overflow: "hidden", textOverflow: "ellipsis" };
  const listBox = { border: "1px solid var(--chrome-border)", height: "44vh", overflow: "auto", fontSize: 12 };

  return (
    <SchDialogFrame
      title="Assign Footprints"
      width={viewer ? 1280 : 1000}
      // Escape and a click outside close the Manage Footprint Association Files dialog first
      onClose={() => !equOpenRef.current && onClose()}
      footer={
        <button className="primary" onClick={() => void submit()}>
          {changes.length > 0 ? `Apply ${changes.length} assignment${changes.length === 1 ? "" : "s"}` : "OK"}
        </button>
      }
    >
      <div style={{ display: "flex", gap: 10, alignItems: "stretch" }}>
        <div style={{ flex: "1.3 1 0", minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Symbols ({rows.length})</div>
          <div style={listBox}>
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
          <div style={listBox}>
            {["", ...libraries].map((l) => (
              <div key={l || "(all)"} style={{ ...cell, cursor: "default", background: library === l ? "var(--chrome-selected-bg)" : undefined, color: library === l ? "var(--chrome-selected-text)" : undefined }} onClick={() => setLibrary(l)}>
                {l || "All libraries"}
              </div>
            ))}
          </div>
        </div>
        <div style={{ flex: "1.4 1 0", minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Footprints ({shown.length})</div>
          <div style={listBox}>
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
        {viewer && (
          // the footprint viewer (`DISPLAY_FOOTPRINTS_FRAME`): a pane beside the lists here, a window of its own in KiCad
          <div data-cvpcb-viewer={viewId} style={{ flex: "1.3 1 0", minWidth: 0, display: "flex", flexDirection: "column" }}>
            <div style={{ display: "flex", justifyContent: "space-between", gap: 6, fontWeight: 600, fontSize: 11, marginBottom: 4 }}>
              <span data-cvpcb-viewer-title style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={caption.title}>
                {caption.title || "Footprint"}
              </span>
              <span data-cvpcb-viewer-status style={{ fontWeight: 400, opacity: 0.8 }}>
                {caption.status}
              </span>
            </div>
            <div style={{ border: "1px solid var(--chrome-border)" }}>
              <FootprintPreview footprint={viewed?.footprint ?? null} height={300} status={!viewId ? "No footprint selected" : viewed ? (viewed.error ?? "") : "Loading…"} />
            </div>
          </div>
        )}
      </div>
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
