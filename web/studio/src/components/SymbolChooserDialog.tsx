// `A` (sch_drawing_tools.cpp PlaceSymbol -> `SelectSymbolFromLibrary` -> SYMBOL_CHOOSER_FRAME over PANEL_SYMBOL_CHOOSER, eeschema/symbol_chooser_frame.cpp,
// eeschema/widgets/panel_symbol_chooser.cpp): KiCad's Symbol Chooser. A tree -- "-- Recently Used --", "-- Already Placed --", the project's own libraries
// and every library KiCad has installed (223 of them, 220 MB: opened one at a time, searched on the server) -- with a search over names, descriptions and
// keywords; beside it the selected symbol's drawing (one unit at a time), its default footprint and its description.
//
// Place Symbol (A, `PlaceSymbol` with `m_symbolHistoryList`) opens it at its first click (actions/schPlaceActions.ts `beginItem`). OK puts the chosen symbol in
// the hand (`new SCH_SYMBOL( *libSymbol, .., sel, cursorPos )`, `addSymbol`, `annotate()`): it rides the cursor, numbered, until the click that places it with
// `add_symbol` (actions/schPlaceActions.ts `carrySymbol`). The server keeps the definition of an installed symbol with the schematic in that same command
// (crates/cli/src/library_place.rs), so it draws, exports and passes ERC. "Place repeated copies" (`m_keepSymbol`, off) and "Place all units" (`m_useUnits`,
// on) are KiCad's two boxes: the next copy or the next unit rides after each click. Cancel places nothing, and the tool stays armed.
//
// `P` (Place Power: `PlaceSymbol` with `filter.FilterPowerSymbols( true )` and `m_powerHistoryList`) opens the same chooser on power symbols only: titled
// "Choose Power Symbol" (`DIALOG_SYMBOL_CHOOSER`), the tree filtered to `aNode.m_IsPower` (a library with none is not shown), its own Recently Used, the
// placed power symbols as Already Placed, no footprint (`m_show_footprints = false`), and "Place repeated copies" (`m_keepSymbol`). OK puts the chosen
// power symbol in the hand (actions/schPlaceActions.ts): it rides the cursor until the click that places it with `add_power_symbol` (with its
// library definition, crates/cli/src/library_place.rs), the net its Value. Cancel places nothing and Place Power stays armed.
//
// The search takes KiCad's four kinds of word (`EDA_COMBINED_MATCHER`, CTX_LIBITEM): text, `*` / `?` wildcards, a `/regular expression/` (or `^...$`) and a
// relational word (`resistance<10k` against a `resistance=4.7k` of the symbol's keywords or description) -- kicad-port/libChooser.ts and the server's
// crates/cli/src/library_search.rs. Under the symbol's drawing is the footprint selector (`FOOTPRINT_SELECT_WIDGET`, `populateFootprintSelector`): "[Default]"
// and the symbol's default footprint (or "No default footprint"), then the footprints of every library with the symbol's pin count and a name its footprint
// filters match (pcbnew's `filterFootprints`, at most 400; none when the symbol has no filters and no pins); the one chosen is drawn below
// (`onFootprintSelected` -> `showFootprint`) and is the footprint placed (`m_fp_override`). The "Choose..." button beside it, which opens the Footprint
// Chooser, is the studio's. A library that loaded with errors (a broken file of a `.kicad_symdir` folder) is named under the search box, as KiCad's status
// bar names it (`GetLibraryLoadErrors`).
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  fetchFootprintSelection,
  fetchProjectFootprintDrawing,
  fetchFootprintDetails,
  fetchProjectSymbolDrawing,
  fetchProjectSymbols,
  fetchSymbolDetails,
  type ProjectSymbol,
  type SymbolDetails,
} from "../api/libraryChooserClient";
import { fetchInstalledLibraries } from "../api/libraryIndexClient";
import type { LibraryFootprint, LibrarySymbol } from "../api/types";
import { chosenOf, type ChooserItem, type ChooserRow } from "../kicad-port/libChooser";
import { unitLetter } from "../kicad-port/unitLetter";
import { loadErrorDetails, loadErrorSummary } from "../kicad-port/libraryLoadErrors";
import { useLibraryLoadErrors } from "../state/libraryLoadErrors";
import { recentItems, rememberChosen } from "../state/chooserRecent";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { carryItem, carrySymbol } from "../actions/schPlaceActions";
import { pendingChooserAt } from "../state/schCarryStore";
import { ChooserDetails, Linkified } from "./chooser/ChooserDetails";
import { FootprintPreview, SymbolPreview } from "./chooser/ChooserPreview";
import { LibChooserTree } from "./chooser/LibChooserTree";
import { chooserFocusGuard, useChooser } from "./chooser/useChooser";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import "../styles/chooser.css";

const SYMBOL_FILTER = { excludePower: true } as const;
const POWER_FILTER = { onlyPower: true } as const;
const notPower = (item: ChooserItem) => !item.power;
const isPower = (item: ChooserItem) => !!item.power;
const anySymbol = () => true;

interface Loaded {
  id: string;
  info: SymbolDetails | null;
  symbol: LibrarySymbol | null;
  error: string | null;
}

export function SymbolChooserDialog() {
  const state = useStudioState();
  // Place Power's chooser: opened by its first click (`schPowerPending` is where the click was)
  if (state.schPowerPending) return <SymbolChooser power at={state.schPowerPending.at} />;
  return state.symbolChooserOpen ? <SymbolChooser power={false} /> : null;
}

/** What a dialog that only picks a symbol asks of the chooser: the id to open on, and where the choice goes. */
export interface SymbolPick {
  preselect?: string | null;
  onChoose: (libId: string) => void;
  onCancel: () => void;
}

/**
 * `SYMBOL_CHOOSER_FRAME` (`FRAME_SYMBOL_CHOOSER`, eeschema/symbol_chooser_frame.cpp) as another dialog opens it to pick a library id (`ShowModal( &newName )`:
 * Change Symbols' browse buttons, `launchNewIdSymbolBrowser` / `launchMatchIdSymbolBrowser`): every library of the tables, power symbols included (no filter:
 * `nullptr`), no "Already Placed" (`dummyAlreadyPlaced`), no footprint pane (`aShowFootprints = false`), opened on the id the field holds (`SetPreselect`); OK
 * gives the chosen symbol's `Lib:Name`.
 */
export function SymbolPickerDialog(props: SymbolPick) {
  return <SymbolChooser power={false} pick={props} />;
}

function SymbolChooser({ power, at, pick }: { power: boolean; at?: [number, number]; pick?: SymbolPick }) {
  const showFootprints = useStudioState().prefs.schFootprintPreview;
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const close = () => (pick ? pick.onCancel() : dispatch(power ? { type: "SET_SCH_POWER_PENDING", pending: null } : { type: "SET_SYMBOL_CHOOSER_OPEN", open: false }));
  /** "Place repeated copies" (`m_keepSymbol`, unchecked as the dialog is made). */
  const [keep, setKeep] = useState(false);
  /** "Place all units" (`m_useUnits->SetValue( true )`): "Sequentially place all units of the symbol." */
  const [allUnits, setAllUnits] = useState(true);

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

  const recent = recentItems(power ? "power" : "symbol");
  const wanted = pick ? anySymbol : power ? isPower : notPower;
  const placed = useMemo(() => (pick ? [] : project.filter((p) => p.placed && wanted(p))), [project, wanted, pick]);
  // The project's libraries are the ones KiCad does not have; the stand-in table only where KiCad's libraries are not installed.
  const own = useMemo(() => project.filter((p) => wanted(p) && !installedNames.has(p.lib) && (p.source !== "builtin" || installedNames.size === 0)), [project, installedNames, wanted]);
  const chooser = useChooser({ kind: "symbol", active: true, recent, placed, project: own, filter: pick ? undefined : power ? POWER_FILTER : SYMBOL_FILTER, keep: wanted, preselect: (pick?.preselect || null) ?? recent[0]?.id ?? null, treeKey: power ? "power" : "symbols" });

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
  const defaultFootprint = info?.footprint ?? chosen?.item.footprint ?? "";
  const footprintName = footprintOverride ?? defaultFootprint;
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

  // The footprint selector (`populateFootprintSelector`): the symbol's pin count (`GetGraphicalPins( 0, 1 )`; here its distinct pin numbers, the same unless two
  // pins share one) and its footprint filters; disabled while the symbol is not loaded (`m_fp_sel_ctrl->Disable()`).
  const selPins = info?.pins ?? (shown?.symbol ? chosen?.item.pins ?? 0 : 0);
  const selFilters = useMemo(() => (info ? (info.fp_filters ? info.fp_filters.split(/\s+/).filter((f) => f !== "") : []) : (shown?.symbol?.footprint_filters ?? [])), [info, shown?.symbol]);
  const selReady = !!shown && !shown.error && !power && !pick;
  const selKey = selReady ? `${id}\u0001${selPins}\u0001${selFilters.join(" ")}` : "";
  const [selection, setSelection] = useState<{ key: string; footprints: string[]; indexing: boolean } | null>(null);
  useEffect(() => {
    if (!selKey) {
      setSelection(null);
      return;
    }
    const ctl = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    const ask = () =>
      fetchFootprintSelection(selPins, selFilters, ctl.signal)
        .then((r) => {
          if (ctl.signal.aborted) return;
          setSelection({ key: selKey, footprints: r.footprints, indexing: r.status === "indexing" });
          // `filterFootprints` waits for the libraries to load: the server says it is still reading them, so ask again
          if (r.status === "indexing") timer = setTimeout(ask, 500);
        })
        .catch(() => !ctl.signal.aborted && setSelection({ key: selKey, footprints: [], indexing: false }));
    void ask();
    return () => {
      ctl.abort();
      if (timer) clearTimeout(timer);
    };
    // selPins and selFilters are in selKey
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selKey]);
  const selList = selection && selection.key === selKey ? selection : null;
  const loadErrors = useLibraryLoadErrors();

  const place = useCallback(
    async (row: ChooserRow | undefined) => {
      const c = chosenOf(row);
      if (!c) {
        if (row?.kind === "group") chooser.toggleGroup(row.group.lib);
        return;
      }
      const lib = c.item.id.slice(0, Math.max(0, c.item.id.indexOf(":")));
      if (pick) {
        // `SYMBOL_CHOOSER_FRAME::OnOK`: the chosen symbol's id goes back to the dialog that asked (`DismissModal( true, libId.Format() )`)
        rememberChosen("symbol", c.item);
        pick.onChoose(c.item.id);
        return;
      }
      const details = shown && shown.id === c.item.id ? shown.info : installedNames.has(lib) ? await fetchSymbolDetails(c.item.id).catch(() => null) : null;
      const reference = details?.reference || c.item.reference || "";
      rememberChosen(power ? "power" : "symbol", { ...c.item, reference, value: details?.value ?? c.item.value, footprint: details?.footprint ?? c.item.footprint, units: details?.units ?? c.item.units, pins: details?.pins ?? c.item.pins, description: details?.description ?? c.item.description });
      if (power) {
        // `new SCH_SYMBOL( *libSymbol, ... )`: the power symbol rides the cursor; the net it names is its Value (`SCH_PIN::GetDefaultNetName` of a power pin)
        close();
        carryItem({ api }, "sch_power", { kind: "power", libId: c.item.id, net: details?.value || c.item.value || c.item.name, rotMillideg: 0 }, at ?? [0, 0], [], keep);
        return;
      }
      close();
      // `new SCH_SYMBOL( *libSymbol, &m_frame->GetCurrentSheet(), sel, cursorPos, .. )`: the unit chosen (`sel.Unit`), the library's Value and default Footprint (a new
      // `SCH_SYMBOL` copies the fields of its `LIB_SYMBOL`), numbered (`annotate()`), in the hand where the first click was
      carrySymbol(
        { api },
        {
          libId: c.item.id,
          referencePrefix: reference,
          unit: Math.min(Math.max(c.unit > 0 ? c.unit : unitPick, 1), units),
          unitCount: units,
          value: details?.value || undefined,
          footprint: footprintOverride ?? details?.footprint ?? "",
          placeAllUnits: allUnits,
        },
        pendingChooserAt() ?? [0, 0],
        keep
      );
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [chooser, shown, installedNames, unitPick, units, footprintOverride, dispatch, power, keep, allUnits, at, pick]
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
  const hint = loadError ?? s.error ?? (s.searching ? (s.indexing ? (s.total ? `Searching… ${s.indexed} of ${s.total} libraries read` : "Searching…") : `${s.matches} match${s.matches === 1 ? "" : "es"}${s.truncated ? " (showing the best)" : ""}`) : "");
  const sym = shown?.symbol ?? null;
  const pinCount = info?.pins ?? chosen?.item.pins ?? 0;

  return (
    <>
      <div className="dialog-backdrop" data-symbol-chooser data-symbol-picker={pick ? "" : undefined} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
        <div className="dialog chooser-dialog" onKeyDown={onKeyDown} {...chooserFocusGuard} role="dialog" aria-label="Symbol Chooser">
          <div className="dialog-header">
            <span>
              {power ? "Choose Power Symbol" : "Symbol Chooser"}
              {chooser.libraries > 0 ? ` (${chooser.libraries} libraries)` : ""}
            </span>
          </div>
          <div className="dialog-body chooser-body">
            <div className="chooser-left">
              <div className="chooser-search">
                <input id="library-tree-search" data-chooser-search autoFocus placeholder="Search symbols (name, description, keywords)" value={chooser.query} onChange={(e) => chooser.setQuery(e.target.value)} />
                <div className="chooser-hint" data-chooser-status data-chooser-answered={s.answered ?? undefined}>
                  {hint}
                </div>
                {loadErrors.length > 0 && (
                  <div className="chooser-hint" style={{ color: "var(--chrome-warn)" }} title={loadErrorDetails(loadErrors)} data-chooser-load-errors>
                    {"⚠ "}
                    {loadErrorSummary(loadErrors)}
                  </div>
                )}
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
              {/* `PANEL_SYMBOL_CHOOSER`'s `m_show_footprints`: "Show footprint previews in Symbol Chooser" (`EESCHEMA_SETTINGS::m_Appearance.footprint_preview`,
                  Preferences > Schematic Editor > Editing Options); off, the footprint selector and preview are not there and the details take the pane */}
              {id && !power && !pick && showFootprints && (
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
                  <select
                    data-footprint-selector
                    aria-label="Footprint"
                    style={{ width: "100%", fontSize: 12 }}
                    disabled={!selReady}
                    value={footprintOverride ?? defaultFootprint}
                    title={selList?.indexing ? "Reading the footprint libraries…" : undefined}
                    onChange={(e) => setFootprintOverride(e.target.value === defaultFootprint ? null : e.target.value)}
                  >
                    <option value={defaultFootprint}>{defaultFootprint === "" ? "No default footprint" : `[Default] ${defaultFootprint}`}</option>
                    {footprintOverride !== null && footprintOverride !== defaultFootprint && !(selList?.footprints ?? []).includes(footprintOverride) && <option value={footprintOverride}>{footprintOverride}</option>}
                    {(selList?.footprints ?? [])
                      .filter((f) => f !== defaultFootprint)
                      .map((f) => (
                        <option key={f} value={f}>
                          {f}
                        </option>
                      ))}
                  </select>
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
            {pick ? (
              <span style={{ marginRight: "auto" }} />
            ) : (
              <>
                <label title="Keep the symbol selected for subsequent clicks.">
                  <input type="checkbox" checked={keep} onChange={(e) => setKeep(e.target.checked)} data-chooser-keep /> Place repeated copies
                </label>
                <label style={{ marginRight: "auto", marginLeft: 30 }} title="Sequentially place all units of the symbol.">
                  <input type="checkbox" checked={allUnits} onChange={(e) => setAllUnits(e.target.checked)} data-chooser-all-units /> Place all units
                </label>
              </>
            )}
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
