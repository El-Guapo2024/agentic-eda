// Live libraries (PARITY-common.md section 34, a deliberate difference from KiCad): the stamp of the board's libraries that `GET /api/version` carries in its
// `X-Library-Stamp` header (crates/cli/src/library_live.rs) moves whenever a library table or a library file of the board's tables changed on disk. KiCad
// reads the tables when the project is opened and again only through Manage Libraries (25ec2335 `LIBRARY_MANAGER::LoadProjectTables`, `ReloadTables`).
// This is the page's side: the first stamp it is told is the one its answers are of, and a later, different one says they are stale.

/** The response header of `GET /api/version` that carries the stamp. */
export const LIBRARY_STAMP_HEADER = "X-Library-Stamp";

export interface LibraryStampWatch {
  /** One poll's stamp (`null`: the server said none). True, and `stale` was called, when it is another than the last one seen. */
  note(stamp: string | null): boolean;
  /** Forget the stamp seen (the page as it was loaded). */
  reset(): void;
}

/** Watch the stamps of the polls; `stale` drops what the page kept of the libraries. */
export function libraryStampWatch(stale: () => void): LibraryStampWatch {
  let last: string | null = null;
  return {
    note(stamp) {
      if (stamp === null || stamp === last) return false;
      const first = last === null;
      last = stamp;
      if (first) return false;
      stale();
      return true;
    },
    reset() {
      last = null;
    },
  };
}
