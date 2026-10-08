// pcbnew.DRCTool.runDRC ("Design Rules Checker", Inspect menu). Read
// pcbnew/dialogs/dialog_drc.cpp and dialog_drc_base.cpp for the real dialog's shape: a
// top options row (refill zones / test schematic parity), a notebook with
// "Violations (%s)" / "Unconnected Items (%s)" / "Schematic Parity (%s)" /
// "Ignored Tests (%s)" pages, a "Show: All / Errors [n] / Warnings [n] /
// Exclusions [n]" row, a list of markers (RC_TREE_MODEL: kicad-port/rcItems.ts,
// components/RcList.tsx) with a right-click menu, then Close.
//
// The violations ARE kicad-cli's: GET /api/drc exports the current design
// to a derived .kicad_pcb, runs `kicad-cli pcb drc` on it, and points each
// reported item back at our own id (crates/cli/src/kicad_engine.rs). There
// is no other DRC engine and no engine switch. It takes seconds (about 4 s
// on a 30-part board), so it runs on demand -- when the dialog opens on a
// board kicad-cli has not judged yet, and on "Run DRC" -- with a visible
// running state, never on every change. The board stays editable during a
// run (the server answers edits meanwhile; closing this window leaves the run
// going), and a report stamped with a revision the board has since left is
// shown as out of date (kicad-port/checkRevision.ts). KiCad's own `type`
// names are shown directly as each violation's category.
//
// Exclusions: "Exclude this violation", "Exclude with comment...", "Exclude all ..."
// and Remove are undoable verbs (`add_drc_exclusions`), persisted in design.json and
// written into the derived project for kicad-cli (crates/kicad/src/drc_exclusions.rs);
// the list is patched in place, with no new run. Next / Previous / Exclude Marker
// (the Inspect menu's actions) step and act on the page that is up and the rows the
// Show boxes let through (state/checkerView.ts).
//
// "Schematic Parity" is `kicad-cli pcb drc --schematic-parity`: the derived schematic
// goes beside the derived board and the report's `schematic_parity` is the list.
// "Ignored Tests" is the report's `ignored_checks`: the checks whose severity is
// Ignore, which kicad-cli does not run; a right click changes a check's severity.
//
// "Lint" is a tab of its own: crates/lint, our own checks that KiCad does not have
// (placement quality, net-class track width). In-process and cheap, so it follows
// the board live while the dialog is open; it never mixes into kicad-cli's lists.
import { useEffect, useMemo, useState } from "react";
import type { DrcViolation } from "../api/types";
import { STALE_NOTICE, isStale } from "../kicad-port/checkRevision";
import { allShown, canExclude, countKinds, listedIndexes, markerPrefix, rcKind, rcMenu, setShowAll, type RcFilter } from "../kicad-port/rcItems";
import { askExclusionComment, setDrcView, useCheckerView, type DrcTab } from "../state/checkerView";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { drcListOf, drcSelectedOf, drcSettingSeverity, drcTitle, excludeDrc, excludeMarkerDrc, openSeveritySetup, restoreDrc, runMarkerMenu, selectDrc, setDrcSeverity, stepDrc, toMenuEntries } from "../actions/checkerOps";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { RcList, type RcRow } from "./RcList";

export function DrcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const view = useCheckerView().drc;
  const open = state.drcDialogOpen;
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const cctx = useMemo(() => ({ api, dispatch }), [api, dispatch]);

  const report = state.drc;
  const violations = report?.violations ?? [];
  const unconnected = report?.unconnected_items ?? [];
  const parityRun = report?.schematic_parity_run === true;
  const parity = report?.schematic_parity ?? [];
  const ignored = report?.ignored_checks ?? [];
  const lint = state.lint?.pcb.violations ?? [];
  const running = state.drcRunning;
  // `updateDisplayedCounts`: the badges count every marker of the report, whatever the Show boxes say (the parity page only once it has run).
  const counts = useMemo(() => countKinds(violations, unconnected, parityRun ? parity : []), [violations, unconnected, parity, parityRun]);
  // Out of date: the board's revision is no longer the one the report was computed on. A run in flight says "running" instead.
  const stale = report !== null && !running && isStale(state.drcVersion, state.version);

  // Opening the dialog on a board kicad-cli has not judged yet runs it (the
  // running state below shows meanwhile); a board it already judged keeps its
  // report until "Run DRC". `version === null` re-evaluates this once the
  // first /api/version answer is in.
  useEffect(() => {
    if (open && state.version !== null && !state.drcRunning && (state.drc === null || isStale(state.drcVersion, state.version))) void api.runDrc();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, state.version === null]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_DRC_OPEN", open: false });
  const setTab = (tab: DrcTab) => setDrcView({ tab });
  const setFilter = (filter: RcFilter) => setDrcView({ filter });

  const listedCount = (tab: DrcTab) => listedIndexes(drcListOf(report, tab), view.filter).length;
  const tabTitle = (tab: DrcTab, label: string) => {
    if (!report) return label;
    if (tab === "parity" && !parityRun) return `${label} (not run)`;
    if (tab === "ignored") return `${label} (${ignored.length})`;
    if (tab === "lint") return `${label} (${lint.length})`;
    return `${label} (${listedCount(tab)})`;
  };

  /** `OnDRCItemRClick`: the marker's menu, drawn at the pointer. */
  const openMenu = (tab: DrcTab, index: number, x: number, y: number) => {
    const v = drcListOf(report, tab)[index];
    if (!v) return;
    const spec = rcMenu({ domain: "drc", title: drcTitle(v.type), excluded: v.excluded === true, severity: drcSettingSeverity(state, v.type) });
    const entries = toMenuEntries(spec, (id) => void runMarkerMenu(cctx, { domain: "drc", tab, index }, id, askExclusionComment), canExclude(v));
    setMenu({ x, y, entries });
  };

  const toggle = (tab: DrcTab, index: number) => {
    const v = drcListOf(report, tab)[index];
    if (!v) return;
    void (v.excluded ? restoreDrc(cctx, [v]) : excludeDrc(cctx, [v]));
  };

  const rowsOf = (tab: DrcTab): RcRow[] => {
    const list = drcListOf(report, tab);
    const selected = drcSelectedOf(state, tab);
    return listedIndexes(list, view.filter).map((i) => {
      const v = list[i]!;
      const kind = rcKind(v);
      const aside = v.excluded && v.kicad_matched === false ? " Waived in this report; kicad-cli's own report still lists it (KiCad keys an exclusion by the marker's exact position, which its report does not give)." : "";
      return { index: i, kind, prefix: markerPrefix(kind, v.severity), message: v.description, items: v.items.map((it) => it.description), comment: v.comment ?? "", selected: selected === i, canToggle: canExclude(v), title: `${v.type}${aside}` };
    });
  };

  const lintRows: RcRow[] = lint.map((v: DrcViolation, i) => ({
    index: i,
    kind: rcKind(v),
    prefix: `${v.type.replace(/_/g, " ")}: `,
    message: v.description,
    items: v.items.map((it) => it.description),
    comment: "",
    selected: state.drcLintSelected === i,
    lint: true,
    canToggle: false,
    extra: v.fix ? `Fix: move ${v.fix.mover} toward ${v.fix.toward} (${v.fix.suggested_command})` : undefined,
  }));

  /** Lint findings select the way a violation does (items on the board, frame them) but keep their own highlight. */
  const jumpToLint = (index: number) => {
    const v = lint[index];
    if (!v) return;
    dispatch({ type: "SET_DRC_LINT_SELECTED", index });
    const refs = v.items.flatMap((it) => (it.id && it.id !== "outline" ? [it.id.split("#")[0]!.split(".")[0]!] : []));
    dispatch({ type: "SET_SELECTION", refs });
    dispatch({ type: "SET_HOT", refs });
    dispatch({ type: "SET_TAB", tab: "pcb" });
  };

  const tab = view.tab;
  const markerPage = tab === "violations" || tab === "unconnected" || tab === "parity";

  const ignoredMenu = (key: string, x: number, y: number) => {
    const current = drcSettingSeverity(state, key);
    const entries: MenuEntry[] = (["error", "warning", "ignore"] as const).map((sev) => ({
      label: sev === "error" ? "Error" : sev === "warning" ? "Warning" : "Ignore",
      checked: current === sev,
      onSelect: () => void setDrcSeverity(cctx, key, sev),
    }));
    setMenu({ x, y, entries });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Design Rules Checker</span>
          <span className="rc-badges" aria-label="Markers by kind">
            <span style={{ color: "var(--chrome-danger)" }}>{counts.errors} error(s)</span>
            <span style={{ color: "var(--chrome-warn)" }}>{counts.warnings} warning(s)</span>
            <span style={{ color: "var(--chrome-text-dim)" }}>{counts.exclusions} excluded</span>
          </span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div className="rc-toolbar">
            <button onClick={() => void api.runDrc()} disabled={running}>
              {running ? "Running…" : "Run DRC"}
            </button>
            {report?.engine && <span style={{ color: "var(--chrome-text-dim)" }}>{report.engine}</span>}
            {stale && (
              <span className="stale-notice" role="status" style={{ color: "var(--chrome-warn)" }}>
                {STALE_NOTICE}
              </span>
            )}
            <span style={{ flex: 1 }} />
            <button title="Previous Marker" aria-label="Previous Marker" disabled={!markerPage} onClick={() => stepDrc(cctx, "prev")}>
              ◀ Previous
            </button>
            <button title="Next Marker" aria-label="Next Marker" disabled={!markerPage} onClick={() => stepDrc(cctx, "next")}>
              Next ▶
            </button>
            <button title="Mark the selected violation as an exclusion" aria-label="Exclude Marker" disabled={tab !== "violations"} onClick={() => void excludeMarkerDrc(cctx)}>
              Exclude Marker
            </button>
          </div>
          {state.drcError && (
            <div className="run-error" role="alert">
              {state.drcError}
            </div>
          )}
          {running && (
            <div className="run-banner" role="status">
              <span className="bar" />
              <span>kicad-cli is checking the board (a few seconds). The board stays editable: close this window and carry on, and the result is marked out of date if the design changes.</span>
            </div>
          )}
          <div style={{ display: "flex", gap: 18, marginBottom: 10 }}>
            <label className="toggle" title="KiCad's own refill. Off by default: kicad-cli 10.99 skips its courtyard checks on a run that refills, so it judges the fills shown on screen instead.">
              <input type="checkbox" checked={state.drcRefillZones} onChange={(e) => dispatch({ type: "SET_DRC_REFILL", refill: e.target.checked })} />
              Refill all zones before performing DRC
            </label>
            <label className="toggle" title="kicad-cli --schematic-parity: compare the board with the schematic (missing, extra and duplicate footprints, pads on other nets, footprints other than the symbol's). Applies to the next run.">
              <input type="checkbox" checked={state.drcParity} onChange={(e) => dispatch({ type: "SET_DRC_PARITY", parity: e.target.checked })} />
              Test for parity between PCB and schematic
            </label>
          </div>

          <div className="dock-tabs" style={{ marginBottom: 10 }}>
            {(
              [
                ["violations", "Violations"],
                ["unconnected", "Unconnected Items"],
                ["parity", "Schematic Parity"],
                ["ignored", "Ignored Tests"],
                ["lint", "Lint"],
              ] as const
            ).map(([id, label]) => (
              <div key={id} className={`dock-tab${tab === id ? " active" : ""}`} onClick={() => setTab(id)} title={id === "lint" ? "Our own checks, the ones KiCad does not have" : undefined}>
                {tabTitle(id, label)}
              </div>
            ))}
          </div>

          <div style={{ opacity: running ? 0.5 : stale ? 0.6 : 1 }}>
            {markerPage && (
              <div className="rc-show">
                <span style={{ color: "var(--chrome-text-dim)" }}>Show:</span>
                <label className="toggle">
                  <input type="checkbox" checked={allShown(view.filter)} onChange={(e) => setFilter(setShowAll(e.target.checked))} />
                  All
                </label>
                <label className="toggle">
                  <input type="checkbox" checked={view.filter.errors} onChange={(e) => setFilter({ ...view.filter, errors: e.target.checked })} />
                  Errors ({counts.errors})
                </label>
                <label className="toggle">
                  <input type="checkbox" checked={view.filter.warnings} onChange={(e) => setFilter({ ...view.filter, warnings: e.target.checked })} />
                  Warnings ({counts.warnings})
                </label>
                <label className="toggle">
                  <input type="checkbox" checked={view.filter.exclusions} onChange={(e) => setFilter({ ...view.filter, exclusions: e.target.checked })} />
                  Exclusions ({counts.exclusions})
                </label>
              </div>
            )}

            {tab === "violations" && (
              <>
                {!report && <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to check the board."}</div>}
                {report && violations.length === 0 && <div className="panel-empty">No violations.</div>}
                {report && violations.length > 0 && listedCount("violations") === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                <RcList rows={rowsOf("violations")} onSelect={(i) => selectDrc(cctx, "violations", i, true)} onMenu={(i, x, y) => openMenu("violations", i, x, y)} onToggle={(i) => toggle("violations", i)} />
              </>
            )}

            {tab === "unconnected" && (
              <>
                {!report && <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to list unconnected items."}</div>}
                {report && unconnected.length === 0 && <div className="panel-empty">No unconnected items.</div>}
                {report && unconnected.length > 0 && listedCount("unconnected") === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                <RcList rows={rowsOf("unconnected")} onSelect={(i) => selectDrc(cctx, "unconnected", i, true)} onMenu={(i, x, y) => openMenu("unconnected", i, x, y)} onToggle={(i) => toggle("unconnected", i)} />
              </>
            )}

            {tab === "parity" && (
              <>
                {!report && <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to compare the board with the schematic."}</div>}
                {report && !parityRun && (
                  <div className="panel-empty" role="status">
                    {report.schematic_parity_error ? `The parity test could not run: ${report.schematic_parity_error}` : "Not run. Check “Test for parity between PCB and schematic” and run DRC again."}
                  </div>
                )}
                {report && parityRun && parity.length === 0 && <div className="panel-empty">The board matches the schematic.</div>}
                {report && parityRun && parity.length > 0 && listedCount("parity") === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                <RcList rows={parityRun ? rowsOf("parity") : []} onSelect={(i) => selectDrc(cctx, "parity", i, true)} onMenu={(i, x, y) => openMenu("parity", i, x, y)} onToggle={(i) => toggle("parity", i)} />
              </>
            )}

            {tab === "ignored" && (
              <>
                <div className="panel-empty" style={{ textAlign: "left", marginBottom: 6 }}>
                  Checks whose severity is Ignore: kicad-cli does not run them.{" "}
                  <a href="#" onClick={(e) => (e.preventDefault(), openSeveritySetup(cctx, "drc"))}>
                    Edit violation severities...
                  </a>{" "}
                  Right-click one to change its severity.
                </div>
                {!report && <div className="panel-empty">{running ? "Running DRC…" : "Run DRC to list the ignored tests."}</div>}
                {report && ignored.length === 0 && <div className="panel-empty">No test is ignored.</div>}
                <div role="list">
                  {ignored.map((c) => (
                    <div
                      key={c.key}
                      role="listitem"
                      className="rc-ignored-row"
                      title={c.key}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        ignoredMenu(c.key, e.clientX, e.clientY);
                      }}
                    >
                      {" • "}
                      {c.description}
                    </div>
                  ))}
                </div>
              </>
            )}

            {tab === "lint" && (
              <>
                <div className="panel-empty" style={{ textAlign: "left", marginBottom: 6 }}>
                  Our own checks (placement quality, net-class track width). KiCad has no equivalent; its rules are in Violations.
                </div>
                {!state.lint && <div className="panel-empty">Loading…</div>}
                {state.lint && lint.length === 0 && <div className="panel-empty">No findings.</div>}
                <RcList rows={lintRows} onSelect={jumpToLint} />
              </>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
      {/* The menu sits inside the backdrop, whose click closes the dialog: a click on an entry must stop here. */}
      {menu && (
        <div onClick={(e) => e.stopPropagation()}>
          <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />
        </div>
      )}
    </div>
  );
}
