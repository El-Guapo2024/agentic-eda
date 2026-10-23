// The embedded files' actions (GAPS.md items 24 and 30, package P26). Called from `registerPcbEditSweep`.
//
//   common.Embed.embededFile            "Embedded Files" (`ACTIONS::embeddedFiles`, "Manage embedded files"): the Embedded Files page of the editor's setup -- Board Setup >
//                                       Board Data > Embedded Files on the board, Schematic Setup > Schematic Data > Embedded Files on the schematic. KiCad's `EMBED_TOOL::AddFile`
//                                       takes the file as the event's parameter; a person picks it here with the page's Add Files.
//   common.Embed.removeFile             `EMBED_TOOL::RemoveFile` / the panel's Remove: the rows selected in the Embedded Files panel that is showing (state/embeddedPanel.ts)
//   common.Embed.extractFile            the panel's Export (`onExportFiles`): the selected rows, or all, downloaded
//   pcbnew.EditorControl.collect3DModels  Tools > Collect and Embed 3D Models (`PCB_CONTROL::CollectAndEmbed3DModels`): every model of a footprint on the board that names a file
//                                       is embedded in the board and named by `kicad-embed://`, one undo step ("Embed 3D Models"), then the info bar's
//                                       "%d 3D model(s) successfully embedded." Nothing to embed is no step and no message, as in KiCad.
import { fetchCollectPlan } from "../api/embeddedClient";
import { COLLECT_MODELS_CMD, collectedMessage } from "../kicad-port/embeddedFiles";
import { setErcView } from "../state/checkerView";
import { activeEmbeddedPanel } from "../state/embeddedPanel";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

const NO_PANEL = "Open the Embedded Files panel first (Board Setup > Board Data > Embedded Files, Schematic Setup > Schematic Data > Embedded Files, or Footprint Properties).";

export function registerEmbeddedActions(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch, api } = ctx;
  const { toast, pcbOnly } = createSweepHelpers(ctx);

  m.set("common.Embed.embededFile", () => {
    if (state.tab === "pcb") {
      dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "embedded_files" });
      dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
    } else if (state.tab === "schematic") {
      setErcView({ setupPage: "embedded_files" });
      dispatch({ type: "SET_SCH_DIALOG", dialog: "setup" });
    } else {
      toast("The embedded files of a library footprint are on the Embedded Files tab of its Footprint Properties.", "info");
    }
  });
  m.set("common.Embed.removeFile", () => {
    const panel = activeEmbeddedPanel();
    if (panel) panel.remove();
    else toast(NO_PANEL, "info");
  });
  m.set("common.Embed.extractFile", () => {
    const panel = activeEmbeddedPanel();
    if (panel) panel.extract();
    else toast(NO_PANEL, "info");
  });

  m.set(
    "pcbnew.EditorControl.collect3DModels",
    pcbOnly(() => {
      void (async () => {
        let count = 0;
        try {
          count = (await fetchCollectPlan()).count;
        } catch (e) {
          toast(e instanceof Error ? e.message : String(e), "error");
          return;
        }
        if (count === 0) return;
        if (await api.cmd(COLLECT_MODELS_CMD)) toast(collectedMessage(count), "info");
      })();
    })
  );
}
