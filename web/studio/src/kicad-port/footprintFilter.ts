// `FOOTPRINT_FILTER` (common/footprint_filter.cpp at 8303b2ad): the lists CvPcb and the footprint chooser narrow a library's footprints with --
// by the symbol's own footprint filters (`ki_fp_filters`, anchored wildcards on the name, case-insensitive, with an optional `library:` part), by the
// number of pins, by library, and by words typed in the search box. Used by `eeschema.EditorControl.assignFootprints` (the assignment dialog).

export interface FootprintCandidate {
  /** `Library:Name`, or just the name. */
  id: string;
  /** The library nickname ("" for a bare name) and the name. */
  lib: string;
  name: string;
  /** `GetUniquePadCount`: how many different pad numbers it has, or null while that has not been read yet. */
  padCount: number | null;
}

export interface FootprintFilters {
  /** The symbol's footprint filters (`FilterByFootprintFilters`); null/undefined = not filtering by them. An empty list matches everything. */
  symbolFilters?: readonly string[] | null;
  /** `FilterByPinCount`; null/undefined = not filtering. */
  pinCount?: number | null;
  /** `FilterByLibrary`; "" = every library. */
  library?: string;
  /** `FilterByTextPattern`: words, all of which must be found. */
  text?: string;
}

export function splitFootprintId(id: string): { lib: string; name: string } {
  const at = id.indexOf(":");
  return at < 0 ? { lib: "", name: id } : { lib: id.slice(0, at), name: id.slice(at + 1) };
}

export function toCandidate(id: string, padCount: number | null = null): FootprintCandidate {
  return { id, ...splitFootprintId(id), padCount };
}

/** `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `*` is any run of characters, `?` one character, the pattern must match the whole text. */
export function wildcardAnchoredMatch(pattern: string, text: string): boolean {
  const re = new RegExp("^" + pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$", "s");
  return re.test(text);
}

/**
 * `FootprintFilterMatch`: a footprint passes when any of the symbol's filters matches its name (lower-case); a filter that holds a ':' is matched
 * against `library:name` instead. No filters at all passes everything.
 */
export function footprintFilterMatch(c: FootprintCandidate, filters: readonly string[]): boolean {
  if (filters.length === 0) return true;
  return filters.some((f) => {
    const pattern = f.toLowerCase();
    const name = (pattern.includes(":") ? `${c.lib.toLowerCase()}:` : "") + c.name.toLowerCase();
    return wildcardAnchoredMatch(pattern, name);
  });
}

/** The search terms of a footprint: its library and name, which is what the free-text box looks in. */
function searchTerms(c: FootprintCandidate): string {
  return `${c.lib} ${c.name}`.toLowerCase();
}

/** `FOOTPRINT_FILTER` over a list: the candidates that pass every filter that is on, in the list's order. A pin-count filter excludes a candidate whose pad count is not known. */
export function filterFootprints(list: readonly FootprintCandidate[], f: FootprintFilters): FootprintCandidate[] {
  const words = (f.text ?? "").toLowerCase().split(/\s+/).filter((w) => w !== "");
  return list.filter((c) => {
    if (f.pinCount != null && !(f.pinCount >= 0 && c.padCount === f.pinCount)) return false;
    if (f.library && c.lib !== f.library) return false;
    if (f.symbolFilters && !footprintFilterMatch(c, f.symbolFilters)) return false;
    if (words.length > 0) {
      const terms = searchTerms(c);
      if (!words.every((w) => terms.includes(w))) return false;
    }
    return true;
  });
}

/** `FOOTPRINT::GetUniquePadCount( DO_NOT_INCLUDE_NPTH )`: how many different pad numbers a footprint has -- a pad with no number and a non-plated hole do not count. */
export function uniquePadCount(pads: ReadonlyArray<{ number: string; kind: string }>): number {
  return new Set(pads.filter((p) => p.number !== "" && p.kind !== "non_plated_hole").map((p) => p.number)).size;
}

/** The pin count CvPcb filters by for a symbol: its different pin numbers, each stacked pin number counted once per pin it stands for. */
export function uniquePinCount(pins: ReadonlyArray<{ number: string }>, expand: (n: string) => string[]): number {
  return new Set(pins.flatMap((p) => expand(p.number))).size;
}
