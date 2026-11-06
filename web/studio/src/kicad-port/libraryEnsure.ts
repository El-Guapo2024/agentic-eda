// Opening a library item (`Lib:Name`) the project library does not hold yet -- a symbol or footprint of KiCad's installed libraries, or one the board's model
// resolved -- first copies it in, as one undoable `put_library_*` (api/libraryClient.ts): the open verbs know only the project library, the model and the built-in
// table, and start a BLANK item for any other name. So the copy must not fail silently: a read error taken for "unknown", or a put the server refused and nobody
// looked at, opened a library's symbol (or footprint) blank, and a save then wrote the blank one over it (task-u1-fpedit found it for footprints, 23f4a206).
//
// The rule (KiCad never opens an empty item for a library item it failed to read: `SYMBOL_EDIT_FRAME::LoadSymbol` shows "Error loading symbol %s from library '%s'." and returns false): null when the item is
// in the project library now, or when the server says nothing knows the name (the open verb then starts a blank one, as New would); else why it could not be copied
// in, after one more try (a request that lost a race with another command). Pure: the reads and the put are the caller's (no DOM, compiled by `npm run test:unit`).

export interface EnsureDeps<T> {
  /** The item by name, from the project library or anything it loads; throws when it cannot be read. */
  fetchAny: (name: string) => Promise<{ item: T; inProject: boolean }>;
  /** The undoable put into the project library; the reply says whether the server took it. */
  put: (item: T) => Promise<{ ok: boolean; message?: string }>;
}

/** `null`: in the project library now, or unknown everywhere; a message: why it could not be copied in (the caller opens nothing and says it). */
export async function ensureInProject<T>(name: string, deps: EnsureDeps<T>, unknown: RegExp, attempts = 2): Promise<string | null> {
  if (!name.includes(":")) return null; // a bare name is the project library's own
  let problem: string | null = null;
  for (let attempt = 0; attempt < attempts; attempt++) {
    let found: { item: T; inProject: boolean };
    try {
      found = await deps.fetchAny(name);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (unknown.test(message)) return null;
      problem = message;
      continue;
    }
    if (found.inProject) return null;
    const reply = await deps.put(found.item);
    if (reply.ok) return null;
    problem = reply.message ?? "the project library refused it";
  }
  return problem;
}

/** `library_api::symbol_any`'s answer for a `lib_id` nothing knows. */
export const UNKNOWN_SYMBOL = /no symbol with this lib_id/;

/** What the Symbol Editor says when it opens nothing: `LoadSymbol`'s "Error loading symbol %s from library '%s'." with the reason as `DisplayErrorMessage`'s extended text. */
export function symbolNotLoaded(libId: string, problem: string): string {
  const at = libId.indexOf(":");
  const [lib, name] = at < 0 ? ["", libId] : [libId.slice(0, at), libId.slice(at + 1)];
  return `Error loading symbol ${name} from library '${lib}'.\n${problem}`;
}
