// `A` (sch_drawing_tools.cpp PlaceSymbol -> `SelectSymbolFromLibrary` -> SYMBOL_CHOOSER_FRAME over PANEL_SYMBOL_CHOOSER, eeschema/symbol_chooser_frame.cpp,
// eeschema/widgets/panel_symbol_chooser.cpp): KiCad's Symbol Chooser. A tree -- "-- Recently Used --", "-- Already Placed --", the project's own libraries
// and every library KiCad has installed (223 of them, 220 MB: opened one at a time, searched on the server) -- with a search over names, descriptions and
// keywords; beside it the selected symbol's drawing (one unit at a time), its default footprint and its description.
//
// Confirming does not place anything itself: it arms `state.armedSymbol` and the `sch_place_symbol` tool, the same two-step ("choose, then click to place")
// every other eeschema placement tool in this app uses, and SchematicView.tsx's onPointerDown is what sends `add_symbol` when the sheet is clicked. The server
// keeps the definition of an installed symbol with the schematic in that same command (crates/cli/src/library_place.rs), so it draws, exports and passes ERC.
//
// Not ported: the regular-expression and relational search words (a word is searched as text, with `*` and `?`), the Footprint selector's list of footprints
// (the Choose button opens the Footprint Chooser instead), and power symbols, which `P` places.
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchProjectFootprintDrawing, fetchFootprintDetails, fetchProjectSymbolDrawing, fetchProjectSymbols, fetchSymbolDetails, type ProjectSymbol, type SymbolDetails } from "../api/libraryChooserClient";
import { fetchInstalledLibraries } from "../api/libraryIndexClient";
import type { LibraryFootprint, LibrarySymbol } from "../api/types";
import { chosenOf, type ChooserItem, type ChooserRow } from "../kicad-port/libChooser";
import { unitLetter } from "../kicad-port/unitLetter";
import { recentItems, rememberChosen } from "../state/chooserRecent";
import { useStudioDispatch, useStudioState } from "../state/store";
import { ChooserDetails, Linkified } from "./chooser/ChooserDetails";
import { FootprintPreview, SymbolPreview } from "./chooser/ChooserPreview";
import { LibChooserTree } from "./chooser/LibChooserTree";
import { chooserFocusGuard, useChooser } from "./chooser/useChooser";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import "../styles/chooser.css";

const SYMBOL_FILTER = { excludePower: true } as const;
const notPower = (item: ChooserItem) => !item.power;

interface Loaded {
  id: string;
  info: SymbolDetails | null;
  symbol: LibrarySymbol | null;
  error: string | null;
}

export function SymbolChooserDialog() {
  const open = useStudioState().symbolChooserOpen;
  return open ? <SymbolChooser /> : null;
}

function SymbolChooser() {
  const dispatch = useStudioDispatch();
  const close = () => dispatch({ type: "SET_SYMBOL_CHOOSER_OPEN", open: false });

  // The project's own symbols and the ones the design uses, and the names of the installed libraries (a symbol of those is read from the library file).
  const [project, setProject] = useState<ProjectSymbol[]>([]);
  const [installedNames, setInstalledNames] = useState<ReadonlySet<string>>(new Set());
  const [loadError, setLoadError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    fetchProjectSymbols()
      .then((p) => !cancelled && setProject(p))
      .catch((e) => !cancelled && setLoadError(e instanceof Error ? e.message : String(e)));
    fetchInstalledLibraries("symbol")
      .then((libs) => !cancelled && setInstalledNames(new Set(libs.map((l) => l.name))))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const recent = recentItems("symbol");
  const placed = useMemo(() => project.filter((p) => p.placed && !p.power), [project]);
  // The project's libraries are the ones KiCad does not have; the stand-in table only where KiCad's libraries are not installed.
  const own = useMemo(() => project.filter((p) => !p.power && !installedNames.has(p.lib) && (p.source !== "builtin" || installedNames.size === 0)), [project, installedNames]);
  const chooser = useChooser({ kind: "symbol", active: true, recent, placed, project: own, filter: SYMBOL_FILTER, keep: notPower, preselect: recent[0]?.id ?? null });

  const chosen = chosenOf(chooser.selectedRow);
  const id = chosen?.item.id ?? null;
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  useEffect(() => {
    if (!id) {
      setLoaded(null);
      return;
    }
    let cancelled = false;
    const lib = id.slice(0, Math.max(0, id.indexOf(":")));
    const fallback = () =>
      fetchProjectSymbolDrawing(id)
        .then((symbol) => !cancelled && setLoaded({ id, info: null, symbol, error: null }))
        .catch((e) => !cancelled && setLoaded({ id, info: null, symbol: null, error: e instanceof Error ? e.message : String(e) }));
    if (installedNames.has(lib)) {
      fetchSymbolDetails(id)
        .then((info) => !cancelled && setLoaded({ id, info, symbol: info.symbol, error: null }))
        .catch(() => void fallback());
    } else void fallback();
    return () => {
      cancelled = true;
    };
  }, [id, installedNames]);
  const shown = loaded && loaded.id === id ? loaded : null;
  const info = shown?.info ?? null;
  const units = Math.max(1, info?.units ?? shown?.symbol?.unit_count ?? chosen?.item.units ?? 1);

  // Which unit the symbol is placed as: the unit row chosen, else the letter picked beside the preview.
  const [unitPick, setUnitPick] = useState(1);
  const [footprintOverride, setFootprintOverride] = useState<string | null>(null);
  const [pickingFootprint, setPickingFootprint] = useState(false);
  useEffect(() => {
    setUnitPick(1);
    setFootprintOverride(null);
  }, [id]);
  const unit = Math.min(Math.max(chosen && chosen.unit > 0 ? chosen.unit : unitPick, 1), units);

  // The default footprint, and its drawing (`PANEL_SYMBOL_CHOOSER::showFootprintFor`).
  const footprintName = footprintOverride ?? info?.footprint ?? chosen?.item.footprint ?? "";
  const [footprint, setFootprint] = useState<{ name: string; drawing: LibraryFootprint | null; error: string | null } | null>(null);
  useEffect(() => {
    if (!footprintName) {
      setFootprint(null);
      return;
    }
    let cancelled = false;
    fetchFootprintDetails(footprintName)
      .then((d) => !cancelled && setFootprint({ name: footprintName, drawing: d.footprint, error: null }))
      .catch(() =>
        fetchProjectFootprintDrawing(footprintName)
          .then((drawing) => !cancelled && setFootprint({ name: footprintName, drawing, error: null }))
          .catch(() => !cancelled && setFootprint({ name: footprintName, drawing: null, error: "Invalid footprint specified" }))
      );
    return () => {
      cancelled = true;
    };
  }, [footprintName]);
  const fpShown = footprint && footprint.name === footprintName ? footprint : null;

  const place = useCallback(
    async (row: ChooserRow | undefined) => {
      const c = chosenOf(row);
      if (!c) {
        if (row?.kind === "group") chooser.toggleGroup(row.group.lib);
        return;
      }
      const lib = c.item.id.slice(0, Math.max(0, c.item.id.indexOf(":")));
      const details = shown && shown.id === c.item.id ? shown.info : installedNames.has(lib) ? await fetchSymbolDetails(c.item.id).catch(() => null) : null;
      const reference = details?.reference || c.item.reference || "";
      rememberChosen("symbol", { ...c.item, reference, value: details?.value ?? c.item.value, footprint: details?.footprint ?? c.item.footprint, units: details?.units ?? c.item.units, pins: details?.pins ?? c.item.pins, description: details?.description ?? c.item.description });
      dispatch({
        type: "SET_ARMED_SYMBOL",
        symbol: {
          libId: c.item.id,
          referencePrefix: reference,
          unit: Math.min(Math.max(c.unit > 0 ? c.unit : unitPick, 1), units),
          // An installed symbol starts with its library's Value and default Footprint (a new `SCH_SYMBOL` copies the fields of its `LIB_SYMBOL`).
          value: details?.value || undefined,
          footprint: footprintOverride ?? details?.footprint ?? "",
        },
      });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_place_symbol" });
      close();
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [chooser, shown, installedNames, unitPick, units, footprintOverride, dispatch]
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      chooser.move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void place(chooser.selectedRow);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      // First Escape clears the search, the second closes (`PANEL_SYMBOL_CHOOSER::OnChar`).
      if (chooser.query !== "") chooser.setQuery("");
      else close();
    }
  };

  const s = chooser.status;
  const hint = loadError ?? s.error ?? (s.searching ? (s.indexing ? `Searching… ${s.indexed} of ${s.total || "…"} libraries read` : `${s.matches} match${s.matches === 1 ? "" : "es"}${s.truncated ? " (showing the best)" : ""}`) : "");
  const sym = shown?.symbol ?? null;
  const pinCount = info?.pins ?? chosen?.item.pins ?? 0;

  return (
    <>
      <div className="dialog-backdrop" data-symbol-chooser onKeyDown={(e) => e.stopPropagation()}>
        <div className="dialog chooser-dialog" onKeyDown={onKeyDown} {...chooserFocusGuard} role="dialog" aria-label="Symbol Chooser">
          <div className="dialog-header">
            <span>
              Symbol Chooser
              {chooser.libraries > 0 ? ` (${chooser.libraries} libraries)` : ""}
            </span>
          </div>
          <div className="dialog-body chooser-body">
            <div className="chooser-left">
              <div className="chooser-search">
                <input id="library-tree-search" data-chooser-search autoFocus placeholder="Search symbols (name, description, keywords)" value={chooser.query} onChange={(e) => chooser.setQuery(e.target.value)} />
                <div className="chooser-hint" data-chooser-status>
                  {hint}
                </div>
              </div>
              <LibChooserTree
                kind="symbol"
                rows={chooser.rows}
                selectedKey={chooser.selectedKey}
                loading={chooser.loading}
                onSelect={chooser.select}
                onChoose={(row) => void place(row)}
                onToggleGroup={chooser.toggleGroup}
                onToggleItem={chooser.toggleItem}
                emptyText={s.searching ? (s.indexing ? "Searching…" : "No symbol matches.") : "Nothing to show."}
              />
            </div>
            <div className="chooser-right">
              <SymbolPreview symbol={sym} unit={unit} bodyStyle={1} status={!id ? "No symbol selected" : shown?.error ? shown.error : shown ? "" : "Loading…"} />
              {units > 1 && (
                <div className="chooser-units">
                  <span style={{ opacity: 0.8 }}>Unit</span>
                  {Array.from({ length: units }, (_, i) => i + 1).map((u) => (
                    <button key={u} className={u === unit ? "primary" : undefined} onClick={() => setUnitPick(u)} data-unit={u}>
                      {unitLetter(u)}
                    </button>
                  ))}
                </div>
              )}
              {id && (
                <>
                  <div style={{ display: "flex", gap: 6, alignItems: "center", fontSize: 12 }}>
                    <span style={{ fontWeight: 600 }}>Footprint</span>
                    <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={footprintName} data-default-footprint>
                      {footprintName || "No footprint specified"}
                    </span>
                    <button onClick={() => setPickingFootprint(true)} title="Choose another footprint for this symbol" data-choose-footprint>
                      Choose…
                    </button>
                  </div>
                  <FootprintPreview footprint={fpShown?.drawing ?? null} height={150} status={!footprintName ? "No footprint specified" : fpShown?.error ? fpShown.error : fpShown ? "" : "Loading…"} />
                </>
              )}
              {chosen && (
                <ChooserDetails
                  name={chosen.item.name}
                  derivedFrom={info?.extends ?? null}
                  description={info?.description ?? chosen.item.description}
                  rows={[
                    ...(info?.keywords ? [{ name: "Keywords", value: info.keywords }] : []),
                    ...(info?.reference || chosen.item.reference ? [{ name: "Reference", value: info?.reference || chosen.item.reference || "" }] : []),
                    ...(footprintName ? [{ name: "Footprint", value: footprintName }] : []),
                    ...(info?.datasheet && info.datasheet !== "~" ? [{ name: "Datasheet", value: <Linkified text={info.datasheet} max={75} /> }] : []),
                    { name: "Units", value: String(units) },
                    ...(pinCount > 0 ? [{ name: "Pins", value: String(pinCount) }] : []),
                  ]}
                />
              )}
            </div>
          </div>
          <div className="dialog-footer">
            <button onClick={close} data-chooser-cancel>
              Cancel
            </button>
            <button className="primary" disabled={!chosen} onClick={() => void place(chooser.selectedRow)} data-chooser-ok>
              OK
            </button>
          </div>
        </div>
      </div>
      {pickingFootprint && (
        <FootprintChooserDialog
          title="Footprint Chooser"
          preselect={footprintName || null}
          pinCount={pinCount}
          fpFilters={info?.fp_filters ? info.fp_filters.split(/\s+/) : shown?.symbol?.footprint_filters}
          onCancel={() => setPickingFootprint(false)}
          onChoose={(pick) => {
            setPickingFootprint(false);
            if (pick.kind === "footprint") setFootprintOverride(pick.name);
          }}
        />
      )}
    </>
  );
}
