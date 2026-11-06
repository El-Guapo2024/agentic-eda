// What `SCINTILLA_TRICKS` (common/scintilla_tricks.cpp at KiCad 8303b2ad) and the Scintilla editor under it do to the custom rules editors
// besides completion (ruleCompletion.ts): the brace highlight (`onScintillaUpdateUI` over Scintilla's `BraceMatch`), Ctrl+/ (Cmd+/ on a Mac)
// commenting lines (`onCharHook`'s `isCtrlSlash` branch), Tab (`m_te->Tab()`: Scintilla's `Editor::Indent( true )` with the rules editor's
// `SetUseTabs( true )`, `SetTabWidth( 4 )`, `SetIndent( 4 )`, `SetTabIndents( true )`), and smart quotes and dashes typed or pasted made plain
// (`ConvertSmartQuotesAndDashes`, common/string_utils.cpp). components/RuleTextEditor.tsx applies them to its textarea.
//
// Positions are the string's (UTF-16); Scintilla's are UTF-8 bytes, which only matters outside ASCII.
//
// Pure: no React, no DOM.

/** The braces the rules editors match (`SCINTILLA_TRICKS( m_textEditor, wxT( "()" ), ... )`). */
export const RULE_BRACES = "()";

/** Scintilla's `BraceOpposite`. */
function braceOpposite(c: string | undefined): string | null {
  switch (c) {
    case "(":
      return ")";
    case ")":
      return "(";
    case "[":
      return "]";
    case "]":
      return "[";
    case "{":
      return "}";
    case "}":
      return "{";
    case "<":
      return ">";
    case ">":
      return "<";
    default:
      return null;
  }
}

/**
 * Scintilla's `Document::BraceMatch( position )`: the brace that closes (or opens) the one at `position`, counting nested pairs of the same
 * kind; -1 when it has none. Scintilla only counts characters of the brace's style; the rules editors have no lexer, so every character has
 * the same one and a parenthesis inside a string counts too.
 */
export function braceMatch(text: string, position: number): number {
  const chBrace = text[position];
  const chSeek = braceOpposite(chBrace);
  if (chSeek === null) return -1;
  const direction = chBrace === "(" || chBrace === "[" || chBrace === "{" || chBrace === "<" ? 1 : -1;
  let depth = 1;
  for (let p = position + direction; p >= 0 && p < text.length; p += direction) {
    const c = text[p];
    if (c === chBrace) depth++;
    if (c === chSeek) depth--;
    if (depth === 0) return p;
  }
  return -1;
}

/** What `onScintillaUpdateUI` lights: a brace and its match (`BraceHighlight`), a brace without one (`BraceBadLight`), or nothing. */
export type BraceLight = { kind: "match"; at: [number, number] } | { kind: "bad"; at: number } | null;

/**
 * `SCINTILLA_TRICKS::onScintillaUpdateUI`: the brace just before the caret, else the one just after it, with its match. (KiCad redoes this
 * only when the caret or the selection moved; here it follows the text too, so an edit that leaves the caret where it was -- Delete --
 * does not leave a stale light.)
 */
export function braceLight(text: string, caret: number, braces: string = RULE_BRACES): BraceLight {
  const isBrace = (c: string | undefined) => c !== undefined && braces.includes(c);
  let p1 = -1;
  if (caret > 0 && isBrace(text[caret - 1])) p1 = caret - 1;
  else if (isBrace(text[caret])) p1 = caret;
  if (p1 < 0) return null;
  const p2 = braceMatch(text, p1);
  return p2 === -1 ? { kind: "bad", at: p1 } : { kind: "match", at: [p1, p2] };
}

/** An edit of the text: the new text and its selection (`anchor` is where the selection started, `caret` where it ends). */
export interface TextEdit {
  text: string;
  anchor: number;
  caret: number;
}

/** The lines of a text: where each starts (a line ends with its `\n`, which belongs to it). */
function lineStarts(text: string): number[] {
  const out = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") out.push(i + 1);
  return out;
}

const lineOf = (starts: readonly number[], pos: number): number => {
  let line = 0;
  while (line + 1 < starts.length && starts[line + 1]! <= pos) line++;
  return line;
};

/** `PositionFromLine`: the start of a line, the end of the text past the last. */
const lineStart = (text: string, starts: readonly number[], line: number): number => (line < starts.length ? starts[line]! : text.length);

/** `LineEnd`: where a line's text ends (before its `\n`). */
const lineEnd = (text: string, starts: readonly number[], line: number): number => {
  const next = line + 1 < starts.length ? starts[line + 1]! - 1 : text.length;
  return Math.max(next, lineStart(text, starts, line));
};

/** `firstNonWhitespace( aLine, &count )`: the first character of the line that is not a space or a tab (its `\n` counts; `\r` past the end). */
function firstNonWhitespace(text: string, starts: readonly number[], line: number): { c: string; count: number } {
  const begin = lineStart(text, starts, line);
  const end = line + 1 < starts.length ? starts[line + 1]! : text.length; // `GetLineLength` includes the line end
  let count = 0;
  for (let i = begin; i < end; i++) {
    const c = text[i]!;
    if (c === " " || c === "\t") {
      count++;
      continue;
    }
    return { c, count };
  }
  return { c: "\r", count };
}

/**
 * Ctrl+/ (`isCtrlSlash`): when the first selected line does not start with `#`, a `#` goes at the start of every selected line; else the
 * first `#` of each selected line that starts with one goes. The selection then covers the lines whole, the last one's line end included.
 */
export function toggleComment(text: string, selStart: number, selEnd: number): TextEdit {
  let starts = lineStarts(text);
  const startLine = lineOf(starts, selStart);
  const endLine = lineOf(starts, selEnd);
  const comment = firstNonWhitespace(text, starts, startLine).c !== "#";
  let out = text;
  for (let ii = startLine; ii <= endLine; ++ii) {
    starts = lineStarts(out);
    const at = lineStart(out, starts, ii);
    if (comment) out = out.slice(0, at) + "#" + out.slice(at);
    else {
      const f = firstNonWhitespace(out, starts, ii);
      if (f.c === "#") out = out.slice(0, at + f.count) + out.slice(at + f.count + 1);
    }
  }
  starts = lineStarts(out);
  const anchor = lineStart(out, starts, startLine);
  const caret = endLine + 1 < starts.length ? starts[endLine + 1]! : out.length;
  return { text: out, anchor, caret };
}

/** The rules editor's `SetTabWidth( 4 )` and `SetIndent( 4 )`. */
const TAB_WIDTH = 4;
const INDENT = 4;

/** `GetColumn( pos )`: the column of a position, tabs going to the next multiple of the tab width. */
function columnOf(text: string, starts: readonly number[], pos: number): number {
  let col = 0;
  for (let i = lineStart(text, starts, lineOf(starts, pos)); i < pos; i++) col = text[i] === "\t" ? (Math.floor(col / TAB_WIDTH) + 1) * TAB_WIDTH : col + 1;
  return col;
}

/** `GetLineIndentation` and `GetLineIndentPosition`: the columns of a line's leading blanks and where they end. */
function indentation(text: string, starts: readonly number[], line: number): { columns: number; end: number } {
  let col = 0;
  let i = lineStart(text, starts, line);
  const end = lineEnd(text, starts, line);
  for (; i < end; i++) {
    if (text[i] === " ") col++;
    else if (text[i] === "\t") col = (Math.floor(col / TAB_WIDTH) + 1) * TAB_WIDTH;
    else break;
  }
  return { columns: col, end: i };
}

/** `SetLineIndentation`: the line's leading blanks replaced by tabs and then spaces making `columns` (`useTabs`); the position after them. */
function setLineIndentation(text: string, line: number, columns: number): { text: string; pos: number } {
  const starts = lineStarts(text);
  const begin = lineStart(text, starts, line);
  const old = indentation(text, starts, line);
  let indent = Math.max(0, columns);
  let blanks = "";
  while (indent >= TAB_WIDTH) {
    blanks += "\t";
    indent -= TAB_WIDTH;
  }
  blanks += " ".repeat(indent);
  return { text: text.slice(0, begin) + blanks + text.slice(old.end), pos: begin + blanks.length };
}

/**
 * Tab (`SCINTILLA_TRICKS::onCharHook` calls `m_te->Tab()`, with or without Shift): Scintilla's `Editor::Indent( true )`. Within one line the
 * selection goes, then a caret in the line's leading blanks indents the line to the next multiple of 4 columns (`tabIndents`), anywhere else
 * a tab is typed. Over several lines each one that is not empty is indented by 4 columns (not the last when the selection only reaches its
 * start) and the selection covers them.
 */
export function tabIndent(text: string, anchor: number, caret: number): TextEdit {
  let starts = lineStarts(text);
  const lineOfAnchor = lineOf(starts, anchor);
  const lineCurrentPos = lineOf(starts, caret);
  if (lineOfAnchor === lineCurrentPos) {
    const from = Math.min(anchor, caret);
    const to = Math.max(anchor, caret);
    let out = text.slice(0, from) + text.slice(to);
    starts = lineStarts(out);
    const ind = indentation(out, starts, lineCurrentPos);
    if (columnOf(out, starts, from) <= columnOf(out, starts, ind.end)) {
      const change = INDENT - (ind.columns % INDENT);
      const r = setLineIndentation(out, lineCurrentPos, ind.columns + change);
      return { text: r.text, anchor: r.pos, caret: r.pos };
    }
    out = out.slice(0, from) + "\t" + out.slice(from);
    return { text: out, anchor: from + 1, caret: from + 1 };
  }
  // Multiple lines selected so indent
  const anchorPosOnLine = anchor - lineStart(text, starts, lineOfAnchor);
  const currentPosPosOnLine = caret - lineStart(text, starts, lineCurrentPos);
  const lineTopSel = Math.min(lineOfAnchor, lineCurrentPos);
  let lineBottomSel = Math.max(lineOfAnchor, lineCurrentPos);
  if (lineStart(text, starts, lineBottomSel) === anchor || lineStart(text, starts, lineBottomSel) === caret) lineBottomSel--; // If not selecting any characters on a line, do not indent
  let out = text;
  for (let line = lineBottomSel; line >= lineTopSel; line--) {
    const s = lineStarts(out);
    if (lineStart(out, s, line) < lineEnd(out, s, line)) out = setLineIndentation(out, line, indentation(out, s, line).columns + INDENT).text;
  }
  starts = lineStarts(out);
  const ls = (line: number) => lineStart(out, starts, line);
  if (lineOfAnchor < lineCurrentPos) {
    return currentPosPosOnLine === 0 ? { text: out, caret: ls(lineCurrentPos), anchor: ls(lineOfAnchor) } : { text: out, caret: ls(lineCurrentPos + 1), anchor: ls(lineOfAnchor) };
  }
  return anchorPosOnLine === 0 ? { text: out, caret: ls(lineCurrentPos), anchor: ls(lineOfAnchor) } : { text: out, caret: ls(lineCurrentPos), anchor: ls(lineOfAnchor + 1) };
}

/** `ConvertSmartQuotesAndDashes`: ‘ ’ to ', “ ” to ", – — to - (what a keyboard's smart punctuation types). */
export function convertSmartQuotesAndDashes(text: string): string {
  return text.replace(/[‘’]/g, "'").replace(/[“”]/g, '"').replace(/[–—]/g, "-");
}

/** The one range an edit changed: what goes between the common start and the common end (to replace just that, keeping the undo history short). */
export function changedRange(before: string, after: string): { from: number; to: number; insert: string } {
  let from = 0;
  while (from < before.length && from < after.length && before[from] === after[from]) from++;
  let endB = before.length;
  let endA = after.length;
  while (endB > from && endA > from && before[endB - 1] === after[endA - 1]) {
    endB--;
    endA--;
  }
  return { from, to: endB, insert: after.slice(from, endA) };
}
