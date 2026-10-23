// The SPICE Simulator window (`SIMULATOR_FRAME`, `SIMULATOR_FRAME_UI_BASE`; eeschema/sim/simulator_frame.cpp, simulator_frame_ui_base.cpp,
// toolbars_simulator_frame.cpp, commit 8303b2ad): its menus (File, View, Simulation), its toolbar, the analysis tabs with their plots, the console
// under them and the side panel (signals, cursors, measurements, tuners). KiCad's is a frame of its own; here it is a window over the schematic
// editor that keeps the schematic usable beside it (Probe and Tune click on the sheet). It is modeless, can be moved by its title bar and sized by
// its corner.
import { useEffect, useRef, useState } from "react";
import actionsData from "../../kicad/actions.json";
import type { ActionsFile, MenuNode } from "../../kicad/types";
import { MenuNodeView } from "../MenuBar";
import { ActionIcon } from "../Toolbar";
import { useActionRunner } from "../../actions/useActionRunner";
import { clearInfobar, closeSimulator, closeTab, frameTitleOf, selectTab, setSimFocused, useSimUi } from "../../state/simulator";
import { currentTab } from "../../kicad-port/sim/simFrame";
import { SimPlot } from "./SimPlot";
import { SimSidePanel } from "./SimSidePanel";
import { SimDialogs } from "./SimDialogs";
import "../../styles/simulator.css";

const actionsByName = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a]));

const item = (action: string, label?: string): MenuNode => ({ type: "item", action, ...(label ? { label } : {}) }) as MenuNode;
const sep: MenuNode = { type: "separator" } as MenuNode;
/** File > Close (`AddClose( _( "Simulator" ) )`): no action of its own, the frame's wxID_CLOSE. */
const CLOSE = { type: "item", action: "" } as MenuNode;

/** `SIMULATOR_FRAME::doReCreateMenuBar`. (Preferences and Help are the schematic editor's.) */
const MENUS: { label: string; items: MenuNode[] }[] = [
  {
    label: "File",
    items: [
      item("eeschema.Simulation.newAnalysisTab"),
      sep,
      item("eeschema.Simulation.openWorkbook"),
      item("eeschema.Simulation.saveWorkbook"),
      item("eeschema.Simulation.saveWorkbookAs"),
      sep,
      item("eeschema.Simulator.exportPNG"),
      item("eeschema.Simulator.exportCSV"),
      sep,
      item("eeschema.Simulator.exportToClipboard"),
      item("eeschema.Simulator.exportPlotToSchematic"),
      sep,
      CLOSE,
    ],
  },
  {
    label: "View",
    items: [
      item("eeschema.Simulator.toggleSimSidePanel"),
      item("eeschema.Simulator.toggleSimConsole"),
      sep,
      item("common.Control.undoZoom"),
      item("common.Control.redoZoom"),
      sep,
      item("common.Control.zoomInCenter"),
      item("common.Control.zoomOutCenter"),
      item("common.Control.zoomInHorizontally"),
      item("common.Control.zoomOutHorizontally"),
      item("common.Control.zoomInVertically"),
      item("common.Control.zoomOutVertically"),
      item("common.Control.zoomFitScreen"),
      sep,
      item("common.Control.toggleGrid"),
      item("eeschema.Simulator.toggleLegend"),
      item("eeschema.Simulator.toggleDottedSecondary"),
      item("eeschema.Simulator.toggleDarkModePlots"),
    ],
  },
  {
    label: "Simulation",
    items: [
      item("eeschema.Simulation.newAnalysisTab"),
      item("eeschema.Simulation.simAnalysisProperties"),
      item("eeschema.Simulation.runSimulation"),
      sep,
      item("eeschema.Simulation.probe"),
      item("eeschema.Simulation.tune"),
      sep,
      item("eeschema.Simulation.editUserDefinedSignals"),
      item("eeschema.Simulation.showNetlist"),
    ],
  },
];

/** `SIMULATOR_TOOLBAR_SETTINGS::DefaultToolbarConfig`, TOP_MAIN. The zoom buttons are KiCad's common zoom actions on the plot. */
const TOOLBAR: (string | null)[] = [
  "eeschema.Simulation.openWorkbook",
  "eeschema.Simulation.saveWorkbook",
  null,
  "eeschema.Simulation.newAnalysisTab",
  "eeschema.Simulation.simAnalysisProperties",
  null,
  "eeschema.Simulation.runSimulation",
  "eeschema.Simulation.stopSimulation",
  null,
  "common.Control.zoomInCenter",
  "common.Control.zoomOutCenter",
  "common.Control.zoomInHorizontally",
  "common.Control.zoomOutHorizontally",
  "common.Control.zoomInVertically",
  "common.Control.zoomOutVertically",
  "common.Control.zoomFitScreen",
  null,
  "eeschema.Simulation.probe",
  "eeschema.Simulation.tune",
  null,
  "eeschema.Simulation.editUserDefinedSignals",
  "eeschema.Simulation.showNetlist",
];


function Menus() {
  const [open, setOpen] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);
  return (
    <div className="menubar sim-menubar" ref={ref}>
      {MENUS.map((m, i) => (
        <div key={m.label} className={`menubar-item${open === i ? " open" : ""}`} onClick={() => setOpen(open === i ? null : i)} onMouseEnter={() => open !== null && setOpen(i)}>
          {m.label}
          {open === i && (
            <div className="menubar-dropdown" onClick={() => setOpen(null)}>
              {m.items.map((n, j) =>
                n === CLOSE ? (
                  <div key={j} className="menu-node-item" role="menuitem" onClick={() => void closeSimulator()}>
                    <span>Close Simulator</span>
                  </div>
                ) : (
                  <MenuNodeView key={j} node={n} />
                )
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function SimToolbar() {
  const { run, isEnabled } = useActionRunner();
  return (
    <div className="sim-toolbar" role="toolbar" aria-label="Simulator">
      {TOOLBAR.map((name, i) => {
        if (!name) return <span key={i} className="sim-toolbar-sep" />;
        const a = actionsByName.get(name);
        const enabled = isEnabled(name);
        return (
          <button key={name} type="button" className="sim-tool" title={a?.label ?? name} aria-label={a?.label ?? name} disabled={!enabled} onClick={() => run(name)}>
            <ActionIcon iconName={a?.icon ?? null} size={20} />
          </button>
        );
      })}
    </div>
  );
}

export function SimulatorWindow() {
  const ui = useSimUi();
  const root = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const move = useRef<{ dx: number; dy: number } | null>(null);

  // The window has the keyboard once something in it is clicked, and loses it to a click elsewhere (KiCad's frame being the active one).
  useEffect(() => {
    if (!ui.open) return;
    const onDoc = (e: PointerEvent) => {
      const t = e.target as Element | null;
      const inside = (!!root.current && root.current.contains(t)) || !!t?.closest?.(".sim-dialog");
      setSimFocused(inside);
    };
    document.addEventListener("pointerdown", onDoc, true);
    return () => document.removeEventListener("pointerdown", onDoc, true);
  }, [ui.open]);

  if (!ui.open) return <SimDialogs />;
  const f = ui.frame;
  const tab = currentTab(f);
  const style: React.CSSProperties = pos ? { left: pos.x, top: pos.y, right: "auto", bottom: "auto" } : {};
  return (
    <>
      <div ref={root} className={`sim-window${ui.focused ? " focused" : ""}`} role="dialog" aria-label="SPICE Simulator" style={style}>
        <div
          className="sim-titlebar"
          onPointerDown={(e) => {
            const r = root.current!.getBoundingClientRect();
            move.current = { dx: e.clientX - r.left, dy: e.clientY - r.top };
            try {
              (e.target as Element).setPointerCapture(e.pointerId);
            } catch {
              // a pointer the page did not see go down
            }
          }}
          onPointerMove={(e) => move.current && setPos({ x: Math.max(0, e.clientX - move.current.dx), y: Math.max(0, e.clientY - move.current.dy) })}
          onPointerUp={() => (move.current = null)}
        >
          <span className="sim-title">{frameTitleOf(ui)}</span>
          <button type="button" className="sim-close" aria-label="Close" onPointerDown={(e) => e.stopPropagation()} onClick={() => void closeSimulator()}>
            ×
          </button>
        </div>
        {ui.infobar && (
          <div className="sim-infobar" role="status">
            {ui.infobar}
            <button type="button" aria-label="Dismiss" onClick={() => clearInfobar()}>×</button>
          </div>
        )}
        <Menus />
        <SimToolbar />
        <div className="sim-body">
          <div className="sim-left">
            <div className="sim-tabs" role="tablist">
              {f.tabs.map((t, i) => (
                <div key={t.id} role="tab" aria-selected={i === f.current} className={`sim-tab${i === f.current ? " active" : ""}`} onClick={() => selectTab(i)}>
                  {t.title}
                  <button type="button" className="sim-tab-close" aria-label={`Close ${t.title}`} onClick={(e) => {
                    e.stopPropagation();
                    closeTab(i);
                  }}>
                    ×
                  </button>
                </div>
              ))}
            </div>
            <div className="sim-plot-area">
              {tab?.plot ? (
                <SimPlot key={tab.id} tabId={tab.id} plot={tab.plot} />
              ) : tab ? (
                // `SIM_NOPLOT_TAB`
                <div className="sim-noplot">Simulation provided no plots. Please refer to console window for results.</div>
              ) : null}
            </div>
            {f.consoleShown && (
              <textarea className="sim-console" readOnly aria-label="Simulation console" value={f.console} ref={(el) => {
                if (el) el.scrollTop = el.scrollHeight;
              }} />
            )}
          </div>
          {f.sidePanelShown && <SimSidePanel />}
        </div>
      </div>
      <SimDialogs />
    </>
  );
}

