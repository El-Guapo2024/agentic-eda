// The board editor's bottom dock: the panes `PCB_EDIT_FRAME` docks under the canvas (pcb_edit_frame.cpp: the Net Inspector and the Search pane, both
// `.Bottom()` at layer 0, so they sit right under the canvas, between the vertical toolbars, side by side in the order they are added). Each is a captioned
// pane with a close button (`CloseButton( true )`) that its action shows again (`ToggleNetInspector`, `ToggleSearch`); the dock is as tall as the person drags
// its top edge (`aui.search_panel_height`, `net_inspector_width`: kept here as one height in the browser, with the dock layout).
import { useCallback, useEffect, useRef, useState } from "react";
import { paneVisible, PCB_BOTTOM_PANES } from "../../kicad-port/dockLayout";
import { useDockLayout } from "../../state/dockLayoutStore";
import { DockPanel } from "./Dock";
import { NetInspectorPanel } from "./NetInspectorPanel";
import { SearchPanel } from "./SearchPanel";

const HEIGHT_KEY = "eda-studio.bottom-dock-height.v1";
/** `BestSize( 300, 200 )` of the Net Inspector; the pane never goes under its `MinSize` height of 60 plus the caption. */
const DEFAULT_HEIGHT = 220;
const MIN_HEIGHT = 90;

function storedHeight(): number {
  try {
    const v = Number(window.localStorage.getItem(HEIGHT_KEY));
    return Number.isFinite(v) && v >= MIN_HEIGHT ? v : DEFAULT_HEIGHT;
  } catch {
    return DEFAULT_HEIGHT;
  }
}

export function BottomDock() {
  const layout = useDockLayout();
  const [height, setHeight] = useState(storedHeight);
  const drag = useRef<{ y: number; h: number } | null>(null);
  const shown = PCB_BOTTOM_PANES.filter((id) => paneVisible(layout, id));

  const onMove = useCallback((e: PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const max = Math.max(MIN_HEIGHT, window.innerHeight - 200);
    setHeight(Math.round(Math.min(max, Math.max(MIN_HEIGHT, d.h + (d.y - e.clientY)))));
  }, []);
  const onUp = useCallback(() => {
    drag.current = null;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
  }, [onMove]);
  useEffect(() => {
    try {
      window.localStorage.setItem(HEIGHT_KEY, String(height));
    } catch {
      /* not kept: the height still applies for this session */
    }
  }, [height]);
  useEffect(() => onUp, [onUp]);

  if (shown.length === 0) return null;
  const allFolded = shown.every((id) => layout.folded[id]);
  return (
    <div className="bottom-dock" data-testid="bottom-dock" style={{ height: allFolded ? undefined : height }}>
      {!allFolded && (
        <div
          className="bottom-dock-sash"
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize the bottom panes"
          onPointerDown={(e) => {
            e.preventDefault();
            drag.current = { y: e.clientY, h: height };
            window.addEventListener("pointermove", onMove);
            window.addEventListener("pointerup", onUp);
          }}
        />
      )}
      <div className="bottom-dock-panes">
        {shown.map((id) => (
          <div key={id} className="bottom-dock-pane" data-bottom-pane={id}>
            <DockPanel id={id} title={id === "netInspector" ? "Net Inspector" : "Search"}>
              {id === "netInspector" ? <NetInspectorPanel /> : <SearchPanel />}
            </DockPanel>
          </div>
        ))}
      </div>
    </div>
  );
}
