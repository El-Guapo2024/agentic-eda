// The library table actions: Manage Symbol Libraries, Manage Footprint Libraries, Configure Paths (`COMMON_CONTROL::ShowLibraryTable` and `ConfigurePaths`,
// common/tool/common_control.cpp -- they run in every editor frame, each frame's menus offer the ones that belong) and the library editors' New Library and Add Library
// (`SYMBOL_EDITOR_CONTROL::AddLibrary`, `PCB_CONTROL::AddLibrary`). The dialogs are components/libraryTables/*, which libraries they edit is `crates/cli/src/library_tables.rs`:
// the board's own tables; KiCad's global ones are read only. `registerCommonActions` (commonActions.ts) calls `registerLibraryTableActions` once while the registry is built.
import { openConfigurePaths, openLibraryFile, openLibraryTables } from "../state/libraryTableDialogs";
import type { ActionHandler, CommonActionContext } from "./commonActions";

export function registerLibraryTableActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  const tab = ctx.tab;
  if (tab !== "pcb" && tab !== "schematic" && tab !== "symbol" && tab !== "footprint") return;

  // ACTIONS::showSymbolLibTable / showFootprintLibTable -- COMMON_CONTROL::ShowLibraryTable: the editor of the library tables of that kind (`DIALOG_SCH_LIBRARY_TABLE`,
  // `DIALOG_PCB_LIBRARY_TABLE`), from whichever frame asks.
  m.set("common.SuiteControl.showSymbolLibTable", () => openLibraryTables("symbol"));
  m.set("common.SuiteControl.showFootprintLibTable", () => openLibraryTables("footprint"));

  // ACTIONS::configurePaths -- COMMON_CONTROL::ConfigurePaths: `DIALOG_CONFIGURE_PATHS`.
  m.set("common.SuiteControl.configurePaths", () => openConfigurePaths());

  // ACTIONS::newLibrary / addLibrary -- SYMBOL_EDITOR_CONTROL::AddLibrary in the Symbol Editor, PCB_CONTROL::AddLibrary in the Footprint Editor (and the board editor, whose
  // menus do not offer it): New Library makes an empty library file or folder and a row in the library table, Add Library puts one that exists in the table.
  if (tab === "symbol") {
    m.set("common.Control.newLibrary", () => openLibraryFile("symbol", true));
    m.set("common.Control.addLibrary", () => openLibraryFile("symbol", false));
  } else if (tab === "footprint") {
    m.set("common.Control.newLibrary", () => openLibraryFile("footprint", true));
    m.set("common.Control.addLibrary", () => openLibraryFile("footprint", false));
  }
}
