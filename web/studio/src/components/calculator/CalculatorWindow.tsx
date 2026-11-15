// The Calculator Tools window (`PCB_CALCULATOR_FRAME`, pcb_calculator/pcb_calculator_frame.cpp at KiCad 25ec2335): title "Calculator Tools", the
// menus File (Close, Quit), Preferences (Preferences...) and Help (`AddStandardHelpMenu`), and a wxTreebook: the calculators listed in a tree at the
// left by group (components/calculator/pages.tsx), the chosen one at the right. KiCad's is a program of its own (pcb_calculator, started by
// `COMMON_CONTROL::Execute( showCalculatorTools )` from the PCB and Schematic Editors' Tools menu); here it is a modeless window over the editors,
// moved by its title bar and sized by its corner, like the simulator's (components/simulator/SimulatorWindow.tsx). It stays open across the editor
// tabs, as KiCad's frame stays open beside them.
//
// Every panel is mounted while the window is open and only the chosen one is shown, so a panel keeps what was typed into it when another is chosen
// (KiCad's panels all live in the treebook). The settings (`last_page` and each panel's) are state/pcbCalculator.ts's.
import { useEffect, useRef, useState } from "react";
import actionsData from "../../kicad/actions.json";
import type { ActionsFile } from "../../kicad/types";
import { useActionRunner } from "../../actions/useActionRunner";
import { openExternal } from "../../actions/commonSuiteActions";
import { helpUrl, languageOf } from "../../kicad-port/appLinks";
import { closePcbCalculator, updatePcbCalculatorSettings, usePcbCalculator } from "../../state/pcbCalculator";
import { CALC_PAGES, shownPageIndex } from "./pages";
import "../../styles/calculator.css";

const actionsByName = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a]));

type MenuEntry = { label: string; tip?: string; hotkey?: string; run: () => void } | null;

function Menus(props: { onRun: (name: string) => void }) {
  const [open, setOpen] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);
  const action = (name: string): MenuEntry => {
    const a = actionsByName.get(name);
    return { label: a?.label ?? name, tip: a?.tooltip ?? undefined, run: () => props.onRun(name) };
  };
  // `doReCreateMenuBar`. File: `AddClose( _( "Calculator Tools" ) )` and `AddQuit( .. )` (the one window closes either way). Preferences:
  // `ACTIONS::openPreferences` (the language list is not ported: the studio is in English). Help: `AddStandardHelpMenu`.
  const menus: { label: string; items: MenuEntry[] }[] = [
    {
      label: "File",
      items: [
        { label: "Close", tip: "Close Calculator Tools", hotkey: "Ctrl+W", run: closePcbCalculator },
        { label: "Quit", tip: "Quit Calculator Tools", hotkey: "Ctrl+Q", run: closePcbCalculator },
      ],
    },
    { label: "Preferences", items: [action("common.SuiteControl.openPreferences")] },
    {
      label: "Help",
      items: [
        // `COMMON_CONTROL::ShowHelp` with the frame's `help_name()`: "pcb_calculator".
        { ...action("common.SuiteControl.help")!, run: () => openExternal(helpUrl("pcb_calculator", languageOf(typeof navigator === "undefined" ? undefined : navigator.language))) },
        action("common.SuiteControl.gettingStarted"),
        action("common.SuiteControl.listHotKeys"),
        action("common.SuiteControl.getInvolved"),
        action("common.SuiteControl.donate"),
        action("common.SuiteControl.reportBug"),
        null,
        action("common.SuiteControl.about"),
      ],
    },
  ];
  return (
    <div className="menubar calc-menubar" ref={ref}>
      {menus.map((m, i) => (
        <div key={m.label} className={`menubar-item${open === i ? " open" : ""}`} onClick={() => setOpen(open === i ? null : i)} onMouseEnter={() => open !== null && setOpen(i)}>
          {m.label}
          {open === i && (
            <div className="menubar-dropdown" onClick={() => setOpen(null)}>
              {m.items.map((n, j) =>
                n === null ? (
                  <div key={j} className="menu-separator" role="separator" />
                ) : (
                  <div key={j} className="menu-node-item" role="menuitem" title={n.tip} onClick={n.run}>
                    <span>{n.label}</span>
                    {n.hotkey ? <span className="menu-node-hotkey">{n.hotkey}</span> : null}
                  </div>
                )
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

export function CalculatorWindow() {
  const ui = usePcbCalculator();
  const { run } = useActionRunner();
  const root = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const move = useRef<{ dx: number; dy: number } | null>(null);
  const selected = shownPageIndex(ui.settings.last_page);

  // Ctrl+W / Ctrl+Q while the window has the keyboard (File > Close / Quit).
  useEffect(() => {
    if (!ui.open) return;
    const onKey = (e: KeyboardEvent) => {
      if (!root.current?.contains(document.activeElement)) return;
      if ((e.ctrlKey || e.metaKey) && (e.key === "w" || e.key === "q")) {
        e.preventDefault();
        e.stopPropagation();
        closePcbCalculator();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [ui.open]);

  if (!ui.open) return null;
  const select = (i: number) => updatePcbCalculatorSettings((s) => void (s.last_page = shownPageIndex(i)));
  const style: React.CSSProperties = pos ? { left: pos.x, top: pos.y, right: "auto", bottom: "auto" } : {};
  return (
    <div ref={root} className="calc-window" role="dialog" aria-label="Calculator Tools" data-testid="calculator-window" style={style} tabIndex={-1}>
      <div
        className="calc-titlebar"
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
        <span className="calc-title">Calculator Tools</span>
        <button type="button" className="calc-close" aria-label="Close" data-testid="calculator-close" onPointerDown={(e) => e.stopPropagation()} onClick={() => closePcbCalculator()}>
          ×
        </button>
      </div>
      <Menus onRun={(name) => run(name)} />
      <div className="calc-body">
        <div className="calc-tree" role="tree" aria-label="Calculators" data-testid="calculator-tree">
          {CALC_PAGES.map((p, i) => (
            <div
              key={p.id}
              role="treeitem"
              aria-selected={i === selected}
              data-calc-page={p.id}
              className={`calc-tree-row${p.component ? " calc-tree-leaf" : " calc-tree-group"}${i === selected ? " selected" : ""}`}
              onClick={() => select(i)}
            >
              {p.component ? null : <span className="calc-tree-twisty">{"▾"}</span>}
              {p.label}
            </div>
          ))}
        </div>
        <div className="calc-page">
          {CALC_PAGES.map((p, i) => {
            const C = p.component;
            if (!C) return null;
            return (
              <div key={p.id} className="calc-page-slot" hidden={i !== selected} data-calc-slot={p.id}>
                <C />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
