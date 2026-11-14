// Mounts the schematic control dialogs once, for `App.tsx`: whichever dialog `state.dialog` names (`schControlStore.tsx`).
import { useEffect } from "react";
import { rescueOnOpen } from "../../api/rescueClient";
import { markRescueOnLoad } from "../../state/rescueOnLoad";
import { useStudioState } from "../../state/store";
import { useSchControlDispatch, useSchControlState } from "../../state/schControlStore";
import { AssignFootprintsDialog } from "./AssignFootprintsDialog";
import { BusSyntaxHelpDialog } from "./BusSyntaxHelpDialog";
import { ExportSymbolsDialog } from "./ExportSymbolsDialog";
import { ImportSheetDialog } from "./ImportSheetDialog";
import { IncrementAnnotationsDialog } from "./IncrementAnnotationsDialog";
import { LegacyBomDialog } from "./LegacyBomDialog";
import { LibLinksDialog } from "./LibLinksDialog";
import { PageNumberDialog } from "./PageNumberDialog";
import { RemapSymbolsDialog } from "./RemapSymbolsDialog";
import { RescueSymbolsDialog } from "./RescueSymbolsDialog";
import { SymbolCheckerDialog } from "./SymbolCheckerDialog";
import { SymbolDiffDialog } from "./SymbolDiffDialog";
import { DesignVariantDialogs } from "../DesignVariantDialogs";

let askedOnOpen = false;

export function SchControlHost() {
  const { dialog } = useSchControlState();
  const dispatch = useSchControlDispatch();
  const close = () => dispatch({ type: "CLOSE_DIALOG" });
  // A KiCad 5 project `eda import-kicad` brought in with no cache library, opened now: Rescue Symbols, not on demand, unless "Never show Rescue Symbols tool"
  // (`OpenProjectFiles`: `if( ( !cfg || !cfg->m_RescueNeverShow ) && !cacheExists ) RescueSymbolLibTableProject( false )`; UI-LEFT-3 S3-16). The browser's
  // import runs it at once (actions/importActions.ts).
  const neverShow = useStudioState().prefs.schRescueNeverShow;
  useEffect(() => {
    // once per page, as the studio opens the board (the server spends its mark: a second mount asks nothing)
    if (askedOnOpen) return;
    askedOnOpen = true;
    void rescueOnOpen()
      .then((pending) => {
        if (!pending || neverShow) return;
        markRescueOnLoad();
        dispatch({ type: "OPEN_DIALOG", dialog: { kind: "rescue_symbols" } });
      })
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <>
      {/* The Net Navigator is a pane of the schematic's left column (components/panels/SchematicDock.tsx), not mounted here. */}
      {dialog?.kind === "bus_syntax" && <BusSyntaxHelpDialog onClose={close} />}
      {dialog?.kind === "increment_annotations" && <IncrementAnnotationsDialog onClose={close} />}
      {dialog?.kind === "page_number" && <PageNumberDialog path={dialog.path} onClose={close} />}
      {dialog?.kind === "library_links" && <LibLinksDialog onClose={close} />}
      {dialog?.kind === "rescue_symbols" && <RescueSymbolsDialog onClose={close} />}
      {dialog?.kind === "remap_symbols" && <RemapSymbolsDialog onClose={close} />}
      {dialog?.kind === "assign_footprints" && <AssignFootprintsDialog onClose={close} />}
      {dialog?.kind === "export_symbols" && <ExportSymbolsDialog onClose={close} />}
      {dialog?.kind === "legacy_bom" && <LegacyBomDialog onClose={close} />}
      {dialog?.kind === "symbol_check" && <SymbolCheckerDialog onClose={close} />}
      {dialog?.kind === "import_sheet" && <ImportSheetDialog forceSheet={dialog.forceSheet} initialPath={dialog.initialPath} onClose={close} />}
      {dialog?.kind === "symbol_diff" && <SymbolDiffDialog reference={dialog.ref} onClose={close} />}
      {dialog?.kind === "variant" && <DesignVariantDialogs />}
    </>
  );
}
