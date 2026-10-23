// Import Sheet... and Draw Sheet from File -- `eeschema.InteractiveDrawing.importSheet` / `drawSheetFromFile` (`SCH_DRAWING_TOOLS::ImportSheet` / `DrawSheet`):
//   Import Sheet...        opens the "Choose Schematic" chooser (components/schControl/ImportSheetDialog.tsx); Open arms the placement tool.
//   ddAppendFile           a schematic dropped on the sheet (`SCH_EDIT_TOOL::DdAppendFile` runs Import Sheet with the file). A browser gives the page the text of every file of the drop and no
//                          folder, so the files are posted (`POST /api/sch/dropped_sheets`: the sheets a file places are found among them and the missing ones named) and the chooser opens on the
//                          file the server made of them, with `LoadSheetFromFile`'s checks and the options it remembers; Open arms the placement tool as for a file picked in it.
//   Draw Sheet from File   KiCad posts it with the file once the chooser is done (`new wxString( sheetFileName )`); the parameter here is that path, or an object
//                          `{ path, keepAnnotations? }`. It checks the file at once (`wxFileExists`: "File '%s' does not exist.") and arms the tool to draw the sheet.
//                          Without a parameter there is no file yet, so it opens the chooser, which then only places a sheet.
import type { Dispatch } from "react";
import { fetchSheetFileCheck } from "../api/hierarchyClient";
import { putDroppedSheets } from "../api/sheetDropClient";
import { droppedFilesOf, droppedSaid } from "../kicad-port/sheetDrop";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { SchControlAction } from "../state/schControlStore";
import { defaultImportOptions } from "../kicad-port/schImportSheet";

type Registry = Map<string, (param?: unknown) => void>;

export interface SchSheetFileContext {
  tab: string;
  state: StudioState;
  api: StudioApi;
  dispatch: Dispatch<Action>;
  controlDispatch: Dispatch<SchControlAction>;
}

/** The path and options a `drawSheetFromFile` event carries, or null when it carries no file. */
export function fileOfParam(param: unknown): { path: string; keepAnnotations: boolean } | null {
  if (typeof param === "string") return param.trim() ? { path: param.trim(), keepAnnotations: false } : null;
  if (param && typeof param === "object") {
    const o = param as { path?: unknown; keepAnnotations?: unknown };
    if (typeof o.path === "string" && o.path.trim()) return { path: o.path.trim(), keepAnnotations: o.keepAnnotations === true };
  }
  return null;
}

export function registerSchSheetFileActions(m: Registry, ctx: SchSheetFileContext): void {
  const { dispatch, controlDispatch } = ctx;
  const onSchematic = (fn: (param?: unknown) => void) => (param?: unknown) => {
    if (ctx.tab === "schematic") fn(param);
  };
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });

  m.set(
    "eeschema.InteractiveDrawing.importSheet",
    onSchematic(() => controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "import_sheet" } }))
  );

  m.set(
    "eeschema.EditorControl.ddAppendFile",
    onSchematic((param) => {
      const files = droppedFilesOf(param);
      if (files.length === 0) return;
      void putDroppedSheets(files).then(
        (r) => {
          if (!r.ok || !r.path) return toast(r.message ?? "The dropped schematics could not be read.", "error");
          const said = droppedSaid(r);
          if (said) toast(said);
          controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "import_sheet", initialPath: r.path } });
        },
        (e) => toast(e instanceof Error ? e.message : String(e), "error")
      );
    })
  );

  m.set(
    "eeschema.InteractiveDrawing.drawSheetFromFile",
    onSchematic((param) => {
      const file = fileOfParam(param);
      if (!file) return controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "import_sheet", forceSheet: true } });
      void fetchSheetFileCheck({ path: file.path, keep_annotations: file.keepAnnotations }).then(
        (check) => {
          if (!check.ok) return toast(check.message ?? `File '${file.path}' does not exist.`, "error");
          const error = check.findings.find((f) => f.severity === "error");
          if (error) return toast(error.text, "error");
          dispatch({ type: "SET_SCH_SHEET_IMPORT", plan: { ...defaultImportOptions(), asSheet: true, keepAnnotations: file.keepAnnotations, path: file.path } });
        },
        (e) => toast(e instanceof Error ? e.message : String(e), "error")
      );
    })
  );
}
