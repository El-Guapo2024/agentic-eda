// The library table dialogs (state/libraryTableDialogs.ts holds which is open): Manage Symbol Libraries, Manage Footprint Libraries, Configure Paths, New Library and Add Library.
import { ConfigurePathsDialog } from "./ConfigurePathsDialog";
import { LibraryFileDialog } from "./LibraryFileDialog";
import { LibraryTablesDialog } from "./LibraryTablesDialog";

export function LibraryTableDialogs() {
  return (
    <>
      <LibraryTablesDialog />
      <ConfigurePathsDialog />
      <LibraryFileDialog />
    </>
  );
}
