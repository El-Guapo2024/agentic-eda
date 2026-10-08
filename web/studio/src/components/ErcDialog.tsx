// eeschema.InspectionTool.runERC ("Electrical Rules Checker", Inspect
// menu). Read eeschema/dialogs/dialog_erc_base.cpp and dialog_erc.cpp for the real
// dialog's shape: a "Run ERC" button, a notebook with "Violations (%s)" and
// "Ignored Tests (%s)" pages, an Errors/Warnings/Exclusions "Show" row, a flat
// RC_TREE_MODEL list (kicad-port/rcItems.ts, components/RcList.tsx; one block per
// marker, in sheet order -- not grouped under a severity/sheet header), a right-click
// menu on each marker, and a cross-probe that pans/selects the offending item on click.
// Cloned from DrcDialog.tsx's own structure.
//
// The violations ARE kicad-cli's: GET /api/erc exports the current schematic
// to a derived .kicad_sch, runs `kicad-cli sch erc` on it (with the design's
// own ERC pin map and per-check severities in the project file), and points each reported item back
// at our own id (crates/cli/src/kicad_engine.rs). There is no other ERC
// engine and no engine switch. It takes seconds, so it runs on demand --
// when the dialog opens on a schematic kicad-cli has not judged yet, and on
// "Run ERC" -- with a visible running state, never on every change. The
// schematic stays editable during a run (the server answers edits meanwhile;
// closing this window leaves the run going), and a report stamped with a
// revision the design has since left is shown as out of date
// (kicad-port/checkRevision.ts).
//
// Each violation's `location` is our id for its first item (a symbol, a
// pin "REF.PIN", a power symbol, a wire, a label, a no-connect, a text), not
// a point: `ercMarkerPosition` (components/schematic/ercMarkerPosition.ts,
// also used by SchematicView.tsx's own canvas markers) resolves it back to a
// point/refs here instead of reading one straight off the violation the way
// DrcDialog's rows can.
//
// "Lint" is a tab of its own: crates/lint's schematic readability checks
// (grid, wire length and overlap, label placement, sheet density), which
// KiCad's ERC does not have. In-process and cheap, so it follows the
// schematic live while the dialog is open; it never mixes into kicad-cli's
// list.
//
// Exclusions are dialog_erc.cpp's: "Exclude this violation" / Remove (the row's button,
// the marker's menu, Exclude Marker) persist to `design.schematic.erc_exclusions` via
// `add_erc_exclusion`/`delete_erc_exclusion`; the report on screen is patched in place
// (no new kicad-cli run for a waiver). "Change severity" and "Ignore all" in the menu write the
// schematic's per-check severities (`set_erc_severities`, Schematic Setup > Violation
// Severity), which go to kicad-cli in the derived project. "Ignored Tests" lists the
// report's `ignored_checks`.
import { useEffect, useMemo, useState } from "react";
import type { ErcViolation } from "../api/types";
import { STALE_NOTICE, isStale } from "../kicad-port/checkRevision";
import { allShown, countKinds, ercKey, listedIndexes, markerPrefix, rcKind, rcMenu, setShowAll, type RcFilter } from "../kicad-port/rcItems";
import { setErcView, useCheckerView, type ErcTab } from "../state/checkerView";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { askExclusionComment } from "../state/checkerView";
import { ercSettingSeverity, ercTitle, excludeMarkerErc, frameErc, isPinMapCheck, openSeveritySetup, runMarkerMenu, selectErc, setErcExcluded, setErcSeverity, stepErc, toMenuEntries } from "../actions/checkerOps";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { RcList, type RcRow } from "./RcList";

/** The finding's own line: kicad-cli's description, without the item list `hint` appends to it ("Pin not connected: Symbol U1 Pin 2 [...]"). */
function ercMessage(v: ErcViolation): string {
  const descs = (v.items ?? []).map((i) => i.description).filter((d) => d !== "");
  const tail = descs.length > 0 ? `: ${descs.join("; ")}` : "";
  const hint = v.hint ?? "";
  return tail !== "" && hint.endsWith(tail) ? hint.slice(0, -tail.length) : hint || v.check.replace(/_/g, " ");
}

export function ErcDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const view = useCheckerView().erc;
  const open = state.ercDialogOpen;
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const cctx = useMemo(() => ({ api, dispatch }), [api, dispatch]);

  const report = state.erc;
  const violations = report?.violations ?? [];
  const ignored = report?.ignored_checks ?? [];
  const lint = state.lint?.schematic.violations ?? [];
  const running = state.ercRunning;
  const counts = useMemo(() => countKinds(violations), [violations]);
  // Out of date: the design's revision is no longer the one the report was computed on. A run in flight says "running" instead.
  const stale = report !== null && !running && isStale(state.ercVersion, state.version);

  // Opening the dialog on a schematic kicad-cli has not judged yet runs it
  // (the running state below shows meanwhile); one it already judged keeps
  // its report until "Run ERC".
  useEffect(() => {
    if (open && state.version !== null && !state.ercRunning && (state.erc === null || isStale(state.ercVersion, state.version))) void api.runErc();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, state.version === null]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_ERC_DIALOG_OPEN", open: false });
  const tab = view.tab;
  const setTab = (t: ErcTab) => setErcView({ tab: t });
  const setFilter = (filter: RcFilter) => setErcView({ filter });
  const listed = listedIndexes(violations, view.filter);

  /** `OnERCItemRClick`: the finding's menu, drawn at the pointer. */
  const openMenu = (index: number, x: number, y: number) => {
    const v = violations[index];
    if (!v) return;
    const spec = rcMenu({ domain: "erc", title: ercTitle(v.check), excluded: v.severity === "excluded", severity: ercSettingSeverity(v), pinMap: isPinMapCheck(v.check), comments: false });
    setMenu({ x, y, entries: toMenuEntries(spec, (id) => void runMarkerMenu(cctx, { domain: "erc", index }, id, askExclusionComment), ercKey(v) !== null) });
  };

  const toggle = (index: number) => {
    const v = violations[index];
    if (v) void setErcExcluded(cctx, [v], v.severity !== "excluded");
  };

  const rows: RcRow[] = listed.map((i) => {
    const v = violations[i]!;
    const kind = rcKind(v);
    return {
      index: i,
      kind,
      prefix: markerPrefix(kind, ercSettingSeverity(v)),
      message: ercMessage(v),
      items: (v.items ?? []).map((it) => it.description).filter((d) => d !== ""),
      comment: "",
      selected: state.ercSelected === i,
      canToggle: ercKey(v) !== null,
      title: v.check,
    };
  });

  const lintRows: RcRow[] = lint.map((v, i) => ({
    index: i,
    kind: rcKind(v),
    prefix: `${v.check.replace(/_/g, " ")}: `,
    message: v.hint ?? v.location ?? "",
    items: v.location ? [v.location] : [],
    comment: "",
    selected: state.ercLintSelected === i,
    lint: true,
    canToggle: false,
  }));

  /** Lint findings select the way a finding does (frame the point they resolve to) but keep their own highlight. */
  const jumpToLint = (index: number) => {
    dispatch({ type: "SET_ERC_LINT_SELECTED", index });
    frameErc(cctx, lint[index], true);
  };

  const ignoredMenu = (key: string, x: number, y: number) => {
    const current = ignored.some((c) => c.key === key) ? "ignore" : "warning";
    const entries: MenuEntry[] = (["error", "warning", "ignore"] as const).map((sev) => ({
      label: sev === "error" ? "Error" : sev === "warning" ? "Warning" : "Ignore",
      checked: current === sev,
      onSelect: () => void setErcSeverity(cctx, key, sev),
    }));
    setMenu({ x, y, entries });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Electrical Rules Checker</span>
          <span className="rc-badges" aria-label="Markers by kind">
            <span style={{ color: "var(--chrome-danger)" }}>{counts.errors} error(s)</span>
            <span style={{ color: "var(--chrome-warn)" }}>{counts.warnings} warning(s)</span>
            <span style={{ color: "var(--chrome-text-dim)" }}>{counts.exclusions} excluded</span>
          </span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div className="rc-toolbar">
            <button onClick={() => void api.runErc()} disabled={running}>
              {running ? "Running…" : "Run ERC"}
            </button>
            {report?.engine && <span style={{ color: "var(--chrome-text-dim)" }}>{report.engine}</span>}
            {stale && (
              <span className="stale-notice" role="status" style={{ color: "var(--chrome-warn)" }}>
                {STALE_NOTICE}
              </span>
            )}
            <span style={{ flex: 1 }} />
            <button title="Previous Marker" aria-label="Previous Marker" disabled={tab === "lint"} onClick={() => stepErc(cctx, "prev")}>
              ◀ Previous
            </button>
            <button title="Next Marker" aria-label="Next Marker" disabled={tab === "lint"} onClick={() => stepErc(cctx, "next")}>
              Next ▶
            </button>
            <button title="Mark the selected violation as an exclusion" aria-label="Exclude Marker" disabled={tab !== "erc"} onClick={() => void excludeMarkerErc(cctx)}>
              Exclude Marker
            </button>
          </div>
          {state.ercError && (
            <div className="run-error" role="alert">
              {state.ercError}
            </div>
          )}
          {running && (
            <div className="run-banner" role="status">
              <span className="bar" />
              <span>kicad-cli is checking the schematic (a few seconds). The schematic stays editable: close this window and carry on, and the result is marked out of date if the design changes.</span>
            </div>
          )}

          <div className="dock-tabs" style={{ marginBottom: 10 }}>
            <div className={`dock-tab${tab === "erc" ? " active" : ""}`} onClick={() => setTab("erc")}>
              Violations{report ? ` (${listed.length})` : ""}
            </div>
            <div className={`dock-tab${tab === "ignored" ? " active" : ""}`} onClick={() => setTab("ignored")}>
              Ignored Tests{report ? ` (${ignored.length})` : ""}
            </div>
            <div className={`dock-tab${tab === "lint" ? " active" : ""}`} onClick={() => setTab("lint")} title="Our own readability checks, the ones KiCad does not have">
              Lint ({lint.length})
            </div>
          </div>

          <div style={{ opacity: running ? 0.5 : stale ? 0.6 : 1 }}>
            {tab === "erc" && (
              <>
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

                {!report && <div className="panel-empty">{running ? "Running ERC…" : "Run ERC to check the schematic."}</div>}
                {report && violations.length === 0 && <div className="panel-empty">No violations.</div>}
                {report && violations.length > 0 && listed.length === 0 && <div className="panel-empty">Nothing matches the current filter.</div>}
                <RcList rows={rows} onSelect={(i) => selectErc(cctx, i, true)} onMenu={openMenu} onToggle={toggle} />
              </>
            )}

            {tab === "ignored" && (
              <>
                <div className="panel-empty" style={{ textAlign: "left", marginBottom: 6 }}>
                  Checks whose severity is Ignore: kicad-cli does not run them.{" "}
                  <a href="#" onClick={(e) => (e.preventDefault(), openSeveritySetup(cctx, "erc"))}>
                    Edit violation severities...
                  </a>{" "}
                  Right-click one to change its severity.
                </div>
                {!report && <div className="panel-empty">{running ? "Running ERC…" : "Run ERC to list the ignored tests."}</div>}
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
                  Our own readability checks (grid, wire length and overlap, label placement, sheet density). KiCad's ERC has no equivalent; its rules are in Violations.
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
