// The two lists under the tabs (`m_cbLayerPresets`, `m_cbViewports`): "Presets" -- the built-in layer presets, the person's own after a separator, then
// "Save preset..." and "Delete preset..." -- and "Viewports", the saved views of the board with "Save viewport..." and "Delete viewport...".
// Picking a preset shows its layers (`onLayerPresetChanged`); picking a viewport zooms to it (`VIEW::SetViewport`). The quick switcher on Ctrl+Tab / Alt+Tab is not
// ported: the browser keeps those keys.
import { useState } from "react";
import { useAppearanceView } from "./useAppearanceView";
import { MessageDialog, NameDialog, PickDialog } from "./shared";
import { allPresets, currentViewport, presetList, savePresetOutcome, viewForViewport } from "../../../kicad-port/layerPresets";
import { MAX_SCALE, MIN_SCALE } from "../../../kicad-port/view";

// The values of the two lists' options: a preset or viewport is `p:<name>` / `v:<name>` (a name is anything the person typed), the two commands are `cmd:save` and `cmd:delete`,
// and the blank entry is "".
const SAVE = "cmd:save";
const DELETE = "cmd:delete";

type Dlg = null | { kind: "save_preset" } | { kind: "overwrite"; name: string } | { kind: "refused"; message: string } | { kind: "delete_preset" } | { kind: "save_viewport" } | { kind: "delete_viewport" };

export function PresetBar() {
  const { state, dispatch, nets, op } = useAppearanceView();
  const [dlg, setDlg] = useState<Dlg>(null);
  const [viewport, setViewport] = useState("");
  const copper = nets.copper;
  const a = state.appearance;
  const list = presetList(copper, a.presets);
  const everyone = allPresets(copper, a.presets);
  const mine = a.presets.map((p) => p.name);

  const canvasSize = () => {
    const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
    return rect && rect.width > 0 && rect.height > 0 ? { w: rect.width, h: rect.height } : null;
  };

  const savePreset = (name: string) => {
    const outcome = savePresetOutcome(name, everyone);
    if (outcome.kind === "new") op({ op: "save_preset", name });
    else if (outcome.kind === "overwrite") {
      setDlg({ kind: "overwrite", name });
      return;
    } else if (outcome.kind === "refused") {
      setDlg({ kind: "refused", message: outcome.message });
      return;
    }
    setDlg(null);
  };

  return (
    <div className="ap-footer">
      <div className="ap-combo-row">
        <label htmlFor="ap-presets">Presets:</label>
        <select
          id="ap-presets"
          className="ap-combo"
          title="Save and restore layer visibility combinations."
          value={a.activePreset === "" ? "" : `p:${a.activePreset}`}
          onChange={(e) => {
            const v = e.target.value;
            if (v === SAVE) setDlg({ kind: "save_preset" });
            else if (v === DELETE) setDlg({ kind: "delete_preset" });
            else if (v.startsWith("p:")) op({ op: "select_preset", name: v.slice(2) });
          }}
        >
          {list.map((e, i) =>
            e.kind === "preset" ? (
              <option key={e.preset.name} value={`p:${e.preset.name}`}>
                {e.preset.name}
              </option>
            ) : e.kind === "separator" ? (
              // The blank entry the list sits on when what is shown matches no preset is the separator before the two commands.
              <option key={`s${i}`} value="" disabled>
                ---
              </option>
            ) : e.kind === "save" ? (
              <option key="save" value={SAVE}>
                Save preset...
              </option>
            ) : (
              <option key="delete" value={DELETE}>
                Delete preset...
              </option>
            )
          )}
        </select>
      </div>
      <div className="ap-combo-row">
        <label htmlFor="ap-viewports">Viewports:</label>
        <select
          id="ap-viewports"
          className="ap-combo"
          title="Save and restore view location and zoom."
          value={viewport === "" ? "" : `v:${viewport}`}
          onChange={(e) => {
            const v = e.target.value;
            if (v === SAVE) setDlg({ kind: "save_viewport" });
            else if (v === DELETE) setDlg({ kind: "delete_viewport" });
            else if (v.startsWith("v:")) {
              const name = v.slice(2);
              const vp = a.viewports.find((x) => x.name === name);
              const size = canvasSize();
              if (!vp || !size) return;
              setViewport(name);
              const next = viewForViewport(vp, size.w, size.h);
              dispatch({ type: "SET_VIEW", view: { ...next, scale: Math.max(MIN_SCALE, Math.min(MAX_SCALE, next.scale)) } });
            }
          }}
        >
          {a.viewports.map((v) => (
            <option key={v.name} value={`v:${v.name}`}>
              {v.name}
            </option>
          ))}
          <option value="" disabled>
            ---
          </option>
          <option value={SAVE}>Save viewport...</option>
          <option value={DELETE}>Delete viewport...</option>
        </select>
      </div>

      {dlg?.kind === "save_preset" && <NameDialog title="Save Layer Preset" label="Layer preset name:" initial={mine.includes(a.activePreset) ? a.activePreset : ""} onCancel={() => setDlg(null)} onOk={savePreset} />}
      {dlg?.kind === "overwrite" && (
        <MessageDialog
          title="Save Layer Preset"
          message="Overwrite existing preset?"
          onCancel={() => setDlg(null)}
          onOk={() => {
            op({ op: "save_preset", name: dlg.name });
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "refused" && <MessageDialog title="Error" message={dlg.message} onCancel={() => setDlg(null)} onOk={() => setDlg(null)} />}
      {dlg?.kind === "delete_preset" && (
        <PickDialog
          title="Delete Preset"
          header="Presets"
          label="Select preset:"
          items={mine}
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            op({ op: "delete_preset", name });
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "save_viewport" && (
        <NameDialog
          title="Save Viewport"
          label="Viewport name:"
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            const size = canvasSize();
            if (size) {
              op({ op: "save_viewport", name: name.trim(), rect: currentViewport(state.view, size.w, size.h) });
              setViewport(name.trim());
            }
            setDlg(null);
          }}
        />
      )}
      {dlg?.kind === "delete_viewport" && (
        <PickDialog
          title="Delete Viewport"
          header="Viewports"
          label="Select viewport:"
          items={a.viewports.map((v) => v.name)}
          onCancel={() => setDlg(null)}
          onOk={(name) => {
            op({ op: "delete_viewport", name });
            if (viewport === name) setViewport("");
            setDlg(null);
          }}
        />
      )}
    </div>
  );
}
