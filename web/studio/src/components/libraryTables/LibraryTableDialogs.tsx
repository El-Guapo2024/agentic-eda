// The library table dialogs (state/libraryTableDialogs.ts holds which is open): Manage Symbol Libraries, Manage Footprint Libraries, Configure Paths, New Library and Add Library;
// and the library editors' Save Library As (state/librarySaveDialogs.ts), which writes a library and its table row.
import { SaveLibraryAsDialog } from "../library/SaveLibraryAsDialog";
import { ConfigurePathsDialog } from "./ConfigurePathsDialog";
import { LibraryFileDialog } from "./LibraryFileDialog";
import { LibraryTablesDialog } from "./LibraryTablesDialog";

export function LibraryTableDialogs() {
  return (
    <>
      <LibraryTablesDialog />
      <ConfigurePathsDialog />
      <LibraryFileDialog />
      <SaveLibraryAsDialog />
    </>
  );
}
