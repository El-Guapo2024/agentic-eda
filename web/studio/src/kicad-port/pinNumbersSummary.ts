// `PIN_NUMBERS` (common/pin_numbers.cpp, include/pin_numbers.h): the sorted set of pin / pad numbers behind the
// "Pad numbers: 1-8,A1" and "Duplicate pads: ..." lines of the Pad Table (`DIALOG_FP_EDIT_PAD_TABLE::updateSummary`)
// and the Pin Table. A number is split into alternating digit and non-digit runs ("A12" = "A", "12"); two numbers
// compare run by run, digit runs numerically and the others as text; and a run of numbers that follow each other by
// exactly one collapses to "first-last".

/** `PIN_NUMBERS::getNextSymbol`: the next run of `str` from `cursor.at` (a signed number with `v`/`.` allowed inside, or text up to the next digit). */
function nextSymbol(str: string, cursor: { at: number }): string {
  if (str.length <= cursor.at) return "";
  const begin = cursor.at;
  const isDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
  const c = str[cursor.at]!;
  if (isDigit(c) || ((c === "+" || c === "-") && cursor.at < str.length - 1 && isDigit(str[cursor.at + 1]))) {
    while (++cursor.at < str.length) {
      const d = str[cursor.at]!;
      if (isDigit(d) || d === "v" || d === "V" || d === ".") continue;
      break;
    }
  } else {
    while (++cursor.at < str.length) {
      if (isDigit(str[cursor.at])) break;
    }
  }
  return str.slice(begin, cursor.at);
}

/**
 * `PIN_NUMBERS::Compare`: -2 / 2 = the first is before / after the second with a gap; -1 / 1 = the first is the
 * immediate predecessor / successor (numbers one apart); 0 = equal. Text runs compare as `wxString::Cmp`
 * (so any two different texts come out as -1 / 1 -- KiCad's own behavior).
 */
export function comparePinNumbers(lhs: string, rhs: string): number {
  const c1 = { at: 0 };
  const c2 = { at: 0 };
  for (;;) {
    let s1 = nextSymbol(lhs, c1);
    let s2 = nextSymbol(rhs, c2);
    if (s1 === "" && s2 === "") return 0;
    if (s1 === "") return -2;
    if (s2 === "") return 2;
    const numeric1 = /[0-9]/.test(s1);
    const numeric2 = /[0-9]/.test(s2);
    if (numeric1) {
      if (!numeric2) return -2;
      s1 = s1.replace(/[vV]/, ".");
      s2 = s2.replace(/[vV]/, ".");
      const v1 = Number.parseFloat(s1);
      const v2 = Number.parseFloat(s2);
      const val1 = Number.isNaN(v1) ? 0 : v1;
      const val2 = Number.isNaN(v2) ? 0 : v2;
      if (val1 < val2) return val1 === val2 - 1 ? -1 : -2;
      if (val1 > val2) return val1 === val2 + 1 ? 1 : 2;
    } else {
      if (numeric2) return 2;
      const res = s1 < s2 ? -1 : s1 > s2 ? 1 : 0;
      if (res !== 0) return res;
    }
  }
}

/** `PIN_NUMBERS`: a `std::set` ordered by `Compare( a, b ) < 0`; an insert that finds an equivalent entry is a duplicate. */
export class PinNumbers {
  private pins: string[] = [];
  private duplicates: string[] = [];

  insert(v: string): void {
    // lower_bound: the first entry that is not less than `v`
    let lo = 0;
    let hi = this.pins.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (comparePinNumbers(this.pins[mid]!, v) < 0) lo = mid + 1;
      else hi = mid;
    }
    const at = this.pins[lo];
    if (at !== undefined && !(comparePinNumbers(v, at) < 0)) {
      if (!this.duplicates.some((d) => comparePinNumbers(d, v) === 0)) this.duplicates.push(v);
      this.duplicates.sort((a, b) => comparePinNumbers(a, b));
      return;
    }
    this.pins.splice(lo, 0, v);
  }

  get size(): number {
    return this.pins.length;
  }

  /** `GetSummary`: "1-3,5". */
  summary(): string {
    const a = this.pins;
    if (a.length === 0) return "";
    let ret = "";
    let rangeStart = 0;
    let idx = 0;
    for (;;) {
      const last = idx;
      idx++;
      const rc = idx < a.length ? comparePinNumbers(a[last]!, a[idx]!) : -2;
      if (rc === -1) continue; // adjacent
      ret += a[rangeStart]!;
      if (rangeStart !== last) ret += `-${a[last]!}`;
      if (idx >= a.length) break;
      rangeStart = idx;
      ret += ",";
    }
    return ret;
  }

  /** `GetDuplicates`: "2,5", or "none". */
  duplicatesText(): string {
    return this.duplicates.length === 0 ? "none" : this.duplicates.join(",");
  }
}

/** The two lines the Pad Table shows for a list of pad numbers (a pad with an empty number is not counted, `if( pad->GetNumber().Length() )`). */
export function padNumberSummary(numbers: Iterable<string>): { summary: string; duplicates: string } {
  const set = new PinNumbers();
  for (const n of numbers) if (n.length > 0) set.insert(n);
  return { summary: set.summary(), duplicates: set.duplicatesText() };
}
