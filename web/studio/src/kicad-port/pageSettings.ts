// Page Settings (`common.Control.pageSettings`): the paper formats, the size a page resolves to, the checks the dialog makes and the
// edit it sends. Ported from common/page_info.cpp (`PAGE_INFO::standardPageSizes`, `SetType`, `SetPortrait`) and
// common/dialogs/dialog_page_settings.cpp (`GetPageLayoutInfoFromDialog`, `TransferDataFromWindow`), commit 8303b2ad. The same table is in
// crates/model/src/page.rs, which validates and writes it; this is the dialog's and the drawing sheet's copy.
//
// Sizes are µm, KiCad's mils converted exactly (`MMsize( 297, 210 )` is 297 x 210 mm; the inch formats are whole mils).

/** A paper format of `PAGE_INFO::standardPageSizes`: its name, the description the dialog lists and its landscape size. */
export interface PageFormat {
  name: string;
  label: string;
  width: number;
  height: number;
}

export const PAGE_FORMATS: readonly PageFormat[] = [
  { name: "A5", label: "A5 148 x 210mm", width: 210_000, height: 148_000 },
  { name: "A4", label: "A4 210 x 297mm", width: 297_000, height: 210_000 },
  { name: "A3", label: "A3 297 x 420mm", width: 420_000, height: 297_000 },
  { name: "A2", label: "A2 420 x 594mm", width: 594_000, height: 420_000 },
  { name: "A1", label: "A1 594 x 841mm", width: 841_000, height: 594_000 },
  { name: "A0", label: "A0 841 x 1189mm", width: 1_189_000, height: 841_000 },
  { name: "A", label: "A 8.5 x 11in", width: 279_400, height: 215_900 },
  { name: "B", label: "B 11 x 17in", width: 431_800, height: 279_400 },
  { name: "C", label: "C 17 x 22in", width: 558_800, height: 431_800 },
  { name: "D", label: "D 22 x 34in", width: 863_600, height: 558_800 },
  { name: "E", label: "E 34 x 44in", width: 1_117_600, height: 863_600 },
  { name: "USLetter", label: "US Letter 8.5 x 11in", width: 279_400, height: 215_900 },
  { name: "USLegal", label: "US Legal 8.5 x 14in", width: 355_600, height: 215_900 },
  { name: "USLedger", label: "US Ledger 11 x 17in", width: 431_800, height: 279_400 },
];

/** `PAGE_SIZE_TYPE::User`, listed last as "User (Custom)". */
export const USER_PAPER = "User";
export const USER_PAPER_LABEL = "User (Custom)";

/** `MIN_PAGE_SIZE_MILS` (1000 mils), `MAX_PAGE_SIZE_PCBNEW_MILS` (48000) and `MAX_PAGE_SIZE_EESCHEMA_MILS` (120000). */
export const MIN_PAGE_SIZE_UM = 25_400;
export const MAX_PAGE_SIZE_PCBNEW_UM = 1_219_200;
export const MAX_PAGE_SIZE_EESCHEMA_UM = 3_048_000;

/** A document's paper as the dialog edits it (the state JSON's `PageInfo` without the resolved size). */
export interface Paper {
  paper: string;
  portrait: boolean;
  /** A User paper's width and height, µm. */
  userSize: [number, number] | null;
}

/** A new document's paper: A4 landscape (`PAGE_INFO`'s default). */
export const DEFAULT_PAPER: Paper = { paper: "A4", portrait: false, userSize: null };

/** `PAGE_INFO::s_user_width` / `s_user_height`: the custom size before one was chosen, 17 x 11 in. */
export const DEFAULT_USER_SIZE_UM: readonly [number, number] = [431_800, 279_400];

/** The paper of a state JSON `page` (null/absent: the default). */
export function paperOf(page: { paper: string; portrait?: boolean; user_size_um?: [number, number] | null } | null | undefined): Paper {
  if (!page) return DEFAULT_PAPER;
  return { paper: page.paper, portrait: !!page.portrait, userSize: page.user_size_um ?? null };
}

/** The width and height of a paper in its orientation (`PAGE_INFO::GetSizeMils`); an unknown name falls back to A4 landscape. */
export function paperSizeUm(p: Paper): { width: number; height: number } {
  if (p.paper === USER_PAPER) {
    return p.userSize ? { width: p.userSize[0], height: p.userSize[1] } : { width: PAGE_FORMATS[1]!.width, height: PAGE_FORMATS[1]!.height };
  }
  const f = PAGE_FORMATS.find((x) => x.name === p.paper) ?? PAGE_FORMATS[1]!;
  return p.portrait ? { width: f.height, height: f.width } : { width: f.width, height: f.height };
}

/** The label the dialog's Paper list shows for a format name. */
export function paperLabel(name: string): string {
  return name === USER_PAPER ? USER_PAPER_LABEL : (PAGE_FORMATS.find((f) => f.name === name)?.label ?? name);
}

/** `%Z` of the drawing sheet: the paper's name ("A4", "USLetter", "User"). */
export function paperName(p: Paper): string {
  return p.paper;
}

/**
 * The paper the dialog leaves when `paper` is chosen with `portrait` and the custom size: a standard format has no user size, a user size
 * has no orientation of its own (`OnPaperSizeChoice` disables the other control; `GetPageLayoutInfoFromDialog` reads it back from the size).
 */
export function normalizePaper(p: Paper): Paper {
  if (p.paper === USER_PAPER) return { paper: USER_PAPER, portrait: false, userSize: p.userSize ?? [DEFAULT_USER_SIZE_UM[0], DEFAULT_USER_SIZE_UM[1]] };
  return { paper: p.paper, portrait: p.portrait, userSize: null };
}

/** True when the paper is A4 landscape, the default, which is not stored. */
export function isDefaultPaper(p: Paper): boolean {
  return p.paper === DEFAULT_PAPER.paper && !p.portrait && p.userSize === null;
}

/** The two papers are the same page. */
export function samePaper(a: Paper, b: Paper): boolean {
  const x = normalizePaper(a);
  const y = normalizePaper(b);
  return x.paper === y.paper && x.portrait === y.portrait && x.userSize?.[0] === y.userSize?.[0] && x.userSize?.[1] === y.userSize?.[1];
}

/**
 * The check `TransferDataFromWindow` makes on a User size (`m_customSizeX.Validate( MIN_PAGE_SIZE_MILS, m_maxPageSizeMils.x, MILS )`): a message
 * when the width or height is outside the limits of the editor (`maxUm`), else null.
 */
export function userSizeError(width: number, height: number, maxUm: number): string | null {
  if (!Number.isFinite(width) || !Number.isFinite(height)) return "The paper width and height must be numbers.";
  for (const [what, v] of [["width", width], ["height", height]] as const) {
    if (v < MIN_PAGE_SIZE_UM || v > maxUm) return `The paper ${what} must be between ${MIN_PAGE_SIZE_UM / 1000} mm and ${maxUm / 1000} mm.`;
  }
  return null;
}

/** KiCad's nine title block comments. */
export const COMMENT_COUNT = 9;

export interface TitleBlockFields {
  title: string;
  date: string;
  rev: string;
  company: string;
  /** Always `COMMENT_COUNT` entries, comment 1 first. */
  comments: string[];
}

/** A title block as the dialog edits it: every field present, nine comments. */
export function titleBlockFields(tb: { title?: string; date?: string; rev?: string; company?: string; comments?: string[] } | null | undefined): TitleBlockFields {
  const comments = Array.from({ length: COMMENT_COUNT }, (_, i) => tb?.comments?.[i] ?? "");
  return { title: tb?.title ?? "", date: tb?.date ?? "", rev: tb?.rev ?? "", company: tb?.company ?? "", comments };
}

/** The title block `set_*_page` is sent: the fields as typed, trailing empty comments dropped (the model keeps no `comment 9` of ""). */
export function titleBlockToSend(tb: TitleBlockFields): { title: string; date: string; rev: string; company: string; comments: string[] } {
  const comments = [...tb.comments];
  while (comments.length > 0 && comments[comments.length - 1] === "") comments.pop();
  return { title: tb.title, date: tb.date, rev: tb.rev, company: tb.company, comments };
}

/** The paper as `set_board_page` / `set_schematic_page` take it. */
export function paperToCmd(p: Paper): { paper: string; portrait?: boolean; user_size_um?: [number, number] } {
  const n = normalizePaper(p);
  if (n.paper === USER_PAPER) return { paper: USER_PAPER, user_size_um: n.userSize! };
  return n.portrait ? { paper: n.paper, portrait: true } : { paper: n.paper };
}

/** `DIALOG_PAGES_SETTINGS::OnDateApplyClick`: a picked date in the `FormatISODate()` form, "2026-10-07". */
export function isoDate(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
