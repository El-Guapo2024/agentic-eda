// `PAD_TOOL::EnumeratePads` (pcbnew/tools/pad_tool.cpp, `pcbnew.PadTool.enumeratePads`, "Renumber Pads...") -- the
// click-to-number tool of the footprint editor. A dialog (`DIALOG_ENUM_PADS`: prefix, start number, step) opens the
// tool; then every pad the cursor goes over takes the next number, a second click on a numbered pad takes its number
// back (and that number is the next one handed out, before any fresh one), a double click or leaving the tool
// commits the lot as one undo step, Escape reverts it.
//
// Pure state machine here; `components/footprint/FootprintCanvas.tsx` feeds it the pads under the cursor and
// `footprintEditorStore.tsx` sends the commit (`set_pad_numbers`).

/** `SEQUENTIAL_PAD_ENUMERATION_PARAMS` (`m_start_number`, `m_step`, `m_prefix`). */
export interface EnumerateParams {
  start: number;
  step: number;
  prefix: string;
}

export const DEFAULT_ENUMERATE_PARAMS: EnumerateParams = { start: 1, step: 1, prefix: "" };

export interface EnumerateAssigned {
  /** The sequence value this pad took (`oldNumbers[newNumber].first`). */
  value: number;
  /** The number the pad had before the tool touched it (`oldNumbers[newNumber].second`). */
  oldNumber: string;
  /** What it holds now (`prefix + value`). */
  number: string;
}

export interface EnumerateState {
  params: EnumerateParams;
  /** `seqPadNum`: the next fresh value. */
  seq: number;
  /** `storedPadNumbers`: values given back by a re-click, handed out first, oldest first. */
  stored: number[];
  /** The pads numbered so far (`pad->IsSelected()` in the C++), by pad id, in the order they were numbered. */
  assigned: Map<string, EnumerateAssigned>;
}

export function enumerateStart(params: EnumerateParams): EnumerateState {
  return { params, seq: params.start, stored: [], assigned: new Map() };
}

/** `constructPadNumber`: `wxString::Format( "%s%d", prefix, value )`. */
export function enumerateNumber(params: EnumerateParams, value: number): string {
  return `${params.prefix}${value}`;
}

/** The value the next pad will take: the oldest given-back one if there is one, else the next fresh one. */
export function enumerateNextValue(s: EnumerateState): number {
  return s.stored.length > 0 ? s.stored[0]! : s.seq;
}

/** The tool's popup: "Click on pad %s\nPress <esc> to cancel all; double-click to finish". */
export function enumeratePopupText(s: EnumerateState): string {
  return `Click on pad ${enumerateNumber(s.params, enumerateNextValue(s))} -- Esc cancels all, double-click finishes`;
}

/**
 * One pad the cursor is over: a pad not numbered yet takes the next number (`commit.Modify( pad ); ... pad->SetNumber( newNumber )`);
 * a numbered pad that is *clicked* (`evt->IsClick( BUT_LEFT )`, not merely dragged over) gives its number back and gets its old number
 * back (`storedPadNumbers.push_back( it->second.first ); pad->SetNumber( it->second.second )`). A drag over an already-numbered pad
 * does nothing. `padId`'s current number is `currentNumber` -- only used for the record of what it had before.
 */
export function enumerateHit(s: EnumerateState, padId: string, currentNumber: string, isClick: boolean): EnumerateState {
  const had = s.assigned.get(padId);
  if (!had) {
    const stored = [...s.stored];
    let seq = s.seq;
    let value: number;
    if (stored.length > 0) {
      value = stored.shift()!;
    } else {
      value = seq;
      seq += s.params.step;
    }
    const assigned = new Map(s.assigned);
    assigned.set(padId, { value, oldNumber: currentNumber, number: enumerateNumber(s.params, value) });
    return { ...s, seq, stored, assigned };
  }
  if (isClick) {
    const assigned = new Map(s.assigned);
    assigned.delete(padId);
    return { ...s, stored: [...s.stored, had.value], assigned };
  }
  return s;
}

/** The numbers a commit writes: every numbered pad's `[id, number]`, in the order they were numbered. */
export function enumerateCommit(s: EnumerateState): [string, string][] {
  return [...s.assigned.entries()].map(([id, a]) => [id, a.number]);
}

/** The number a pad shows while the tool runs (its new one once taken, else its own). */
export function enumerateShownNumber(s: EnumerateState, padId: string, own: string): string {
  return s.assigned.get(padId)?.number ?? own;
}

/**
 * `EnumeratePads`' "approximate the mouse move by a line" step: "wxWidgets deliver mouse move events not frequently enough,
 * resulting in skipping pads if the user moves cursor too fast. To solve it, create a line that approximates the mouse move and
 * search pads that are on the line" every 0.1 mm. `segments = distance / int( 0.1 * IU_PER_MM ) + 1`, `step = ( mouse - old ) / segments`
 * (integer division), test points `mouse - j * step` for `j < segments` -- from the cursor back towards where it was.
 */
export function sweepPoints(prev: { x: number; y: number } | null, cur: { x: number; y: number }, searchStepUm = 100): { x: number; y: number }[] {
  const from = prev ?? cur; // `isFirstPoint`: the first event has no previous position
  const distance = Math.hypot(cur.x - from.x, cur.y - from.y);
  const segments = Math.trunc(distance / searchStepUm) + 1;
  const stepX = Math.trunc((cur.x - from.x) / segments);
  const stepY = Math.trunc((cur.y - from.y) / segments);
  const out: { x: number; y: number }[] = [];
  for (let j = 0; j < segments; j++) out.push({ x: cur.x - j * stepX, y: cur.y - j * stepY });
  return out;
}

/**
 * The pads to number for one mouse event: the numberable pads under each test point of the sweep, in sweep order, with a pad
 * listed twice in a row only once (`selectedPads.unique()` removes consecutive duplicates only).
 */
export function padsUnderSweep<P extends { id?: string }>(pads: readonly P[], points: readonly { x: number; y: number }[], contains: (pad: P, x: number, y: number) => boolean): P[] {
  const hits: P[] = [];
  for (const pt of points) {
    for (const pad of pads) {
      if (!contains(pad, pt.x, pt.y)) continue;
      if (hits[hits.length - 1]?.id !== pad.id) hits.push(pad);
    }
  }
  return hits;
}
