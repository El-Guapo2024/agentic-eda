// Live libraries (PARITY-common.md section 34, a deliberate difference from KiCad, which sees a table written by something else only when the project is
// opened again): the page's poll of `GET /api/version` carries the stamp of the board's libraries (kicad-port/libraryStamp.ts, crates/cli/src/library_live.rs),
// and a stamp other than the last one seen is "a library table or a library file changed on disk" -- an agent added a project library, the CLI wrote a
// footprint, another tab saved. What the page kept of the libraries is dropped (`librariesChanged`: the choosers' items and drawings, the trees' names and
// columns, the search index), and what is on screen asks again: the trees through `onInstalledLibrariesReset`, the choosers, CvPcb's list and the library
// editors' trees through [`useLibrariesGeneration`]. The item a library editor shows is not touched: its own watch asks KiCad's question first
// (`state/libraryWatch.ts`).
import { useEffect, useState } from "react";
import { onInstalledLibrariesReset } from "../api/libraryIndexClient";
import { librariesChanged } from "../api/libraryTablesClient";
import { libraryStampWatch } from "../kicad-port/libraryStamp";

const watch = libraryStampWatch(librariesChanged);

/** The library stamp of one poll's answer (`fetchVersionAndLibraries`): another one than the last drops what the page kept. Returns whether it did. */
export function noteLibraryStamp(stamp: string | null): boolean {
  return watch.note(stamp);
}

/** A count that moves on each time the page's library answers are dropped (`resetInstalledLibraries`): an effect that asked for them asks again. */
export function useLibrariesGeneration(): number {
  const [generation, setGeneration] = useState(0);
  useEffect(() => onInstalledLibrariesReset(() => setGeneration((g) => g + 1)), []);
  return generation;
}
