// "Net Display Options" (`APPEARANCE_CONTROLS::createControls`): when the net and net class colours show (on every copper item, on the ratsnest only, nowhere)
// and which ratsnest lines show (all, those between layers that are on, none).
import { useAppearanceView } from "./useAppearanceView";
import { Pane, Radios } from "./shared";
import type { NetColorMode, RatsnestDisplay } from "../../../kicad-port/appearance";

export function NetDisplayOptions() {
  const { state, op } = useAppearanceView();
  const display: RatsnestDisplay = !state.showRatsnest ? "none" : state.bcx.ratsnestMode === "visible" ? "visible" : "all";
  return (
    <Pane title="Net Display Options">
      <div className="ap-sub" title="Choose when to show net and netclass colors">
        Net colors:
      </div>
      <Radios<NetColorMode>
        name="Net colors"
        value={state.appearance.netColorMode}
        onChange={(mode) => op({ op: "net_color_mode", mode })}
        options={[
          { value: "all", label: "All", title: "Net and netclass colors are shown on all copper items" },
          { value: "ratsnest", label: "Ratsnest", title: "Net and netclass colors are shown on the ratsnest only" },
          { value: "off", label: "None", title: "Net and netclass colors are not shown" },
        ]}
      />
      <div className="ap-sub" title="Choose which ratsnest lines to display">
        Ratsnest display:
      </div>
      <Radios<RatsnestDisplay>
        name="Ratsnest display"
        value={display}
        onChange={(mode) => op({ op: "ratsnest_display", mode })}
        options={[
          { value: "all", label: "All", title: "Show ratsnest lines to items on all layers" },
          { value: "visible", label: "Visible layers", title: "Show ratsnest lines to items on visible layers" },
          { value: "none", label: "None", title: "Hide all ratsnest lines" },
        ]}
      />
    </Pane>
  );
}
