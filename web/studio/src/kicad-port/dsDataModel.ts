// The drawing sheet (`.kicad_wks`): KiCad's frame, zone references and title block as a list of items laid out against the page's corners, and
// the draw items one page makes of them. A port of common/drawing_sheet at commit 8303b2ad:
//
//   drawing_sheet_parser.cpp   DRAWING_SHEET_PARSER (Parse, parseHeader, parseSetup, parseGraphic, parseText, parsePolygon, parsePolyOutline,
//                              parseBitmap, readPngdata, readOption, parseCoordinate, parseInt, parseDouble), convertLegacyVariableRefs,
//                              DS_DATA_MODEL::SetPageLayout / LoadDrawingSheet's error handling
//   ds_data_model.cpp          DS_DATA_MODEL (its defaults, SetupDrawEnvironment)
//   ds_data_item.cpp           DS_DATA_ITEM::SyncDrawItems / GetStartPos / GetEndPos / IsInsidePage / GetPenSizeIU, DS_DATA_ITEM_POLYGONS
//                              (SyncDrawItems, GetCornerPosition, SetBoundingBox, IsInsidePage), DS_DATA_ITEM_TEXT (SyncDrawItems, IncrementLabel,
//                              ReplaceAntiSlashSequence, SetConstrainedTextSize), DS_DATA_ITEM_BITMAP (SyncDrawItems, GetPPI)
//   ds_draw_item.cpp           DS_DRAW_ITEM_LIST::BuildDrawItemsList (the page-1 options)
//   ds_painter.cpp             DS_DRAW_ITEM_LIST::BuildFullText (the sheet's own variables, then the title block's)
//   common/title_block.cpp     TITLE_BLOCK::TextVarResolver
//   common/common.cpp          ExpandTextVars (nested `${..}`, escaped `\${..}`, an unknown variable left as written)
//   common/dsnlexer.cpp        the quoted-string escapes and the number/symbol split the parser's tokens rely on
//
// Units: the model is in mm, as the file is; the draw items are in nm (the studio's internal unit, `m_WSunits2Iu` = 1e6), rounded as `KiROUND`.
//
// Differs:
//   - `${KICAD_VERSION}` is the caller's text (the studio passes "KiCad E.D.A. eda studio"); KiCad writes "KiCad E.D.A. " and its version.
//   - `@{..}` expressions are left as written (KiCad evaluates them with EXPRESSION_EVALUATOR); a variable the sheet, the title block and the
//     caller's project resolver do not know is left as `${NAME}`, as KiCad leaves it.
//   - `(font (face ..))` is kept but every text is drawn in the stroke font (the studio has no outline fonts).
//   - A text's box for `maxlen` / `maxheight` (`SetConstrainedTextSize`, which asks `EDA_TEXT::GetTextBox`) is measured with the caller's
//     `measure` (the stroke font's advance), the box widened by the pen as `schTextBox` does.
//   - A bitmap keeps its PNG bytes (base64); its size comes from the PNG's `pHYs` resolution (`BITMAP_BASE::updatePPI`, 300 PPI without one)
//     and `(scale ..)`. Legacy `(pngdata (data "89 50 ..") ..)` hex lines are read too.
//
// Pure: no React, no DOM.

import { DEFAULT_DRAWING_SHEET } from "./dsDefaultSheet";

// ------------------------------------------------------------------------------------------------------------------------------------- model

/** `CORNER_ANCHOR`: which page corner (inside the margins) a coordinate is measured from, the direction going toward the opposite corner. */
export type DsAnchor = "rb" | "rt" | "lb" | "lt";

/** `POINT_COORD`: mm from a corner; `RB_CORNER` when the file names none. */
export interface DsPoint {
  x: number;
  y: number;
  anchor: DsAnchor;
}

/** `PAGE_OPTION`: `ALL_PAGES`, `FIRST_PAGE_ONLY` (`(option page1only)`) or `SUBSEQUENT_PAGES` (`(option notonpage1)`). */
export type DsPageOption = "all" | "first" | "subsequent";

export type DsHAlign = "left" | "center" | "right";
export type DsVAlign = "top" | "center" | "bottom";

/** A `COLOR4D`, each channel 0..1. */
export interface DsColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

interface DsItemBase {
  /** `m_Name`. */
  name: string;
  /** `m_Info` (`(comment ..)`). */
  info: string;
  pageOption: DsPageOption;
  pos: DsPoint;
  end: DsPoint;
  /** `m_LineWidth`, mm; 0 takes the model's default. */
  lineWidth: number;
  /** `m_RepeatCount`, 1..100. */
  repeat: number;
  /** `m_IncrementVector`, mm. */
  incr: { x: number; y: number };
  /** `m_IncrementLabel`. */
  incrLabel: number;
}

export interface DsSegmentItem extends DsItemBase {
  type: "segment";
}

export interface DsRectItem extends DsItemBase {
  type: "rect";
}

export interface DsTextItem extends DsItemBase {
  type: "text";
  /** `m_TextBase`: the text as written, its legacy `%X` formats already turned into variables. */
  textBase: string;
  hJustify: DsHAlign;
  vJustify: DsVAlign;
  italic: boolean;
  bold: boolean;
  /** `(font (face ..))`, or null for KiCad's own font. */
  face: string | null;
  /** `m_TextSize`, mm; 0 takes the model's default. */
  size: { x: number; y: number };
  /** `m_TextColor`, null for `COLOR4D::UNSPECIFIED` (the sheet's colour). */
  color: DsColor | null;
  /** `m_Orient`, degrees. */
  orient: number;
  /** `m_BoundingBoxSize` (`maxlen`, `maxheight`), mm; 0 is no limit. */
  maxLen: number;
  maxHeight: number;
}

export interface DsPolygonsItem extends DsItemBase {
  type: "poly";
  /** `m_Corners`, mm, relative to `pos`. */
  corners: { x: number; y: number }[];
  /** `m_polyIndexEnd`: the last corner of each outline. */
  polyEnds: number[];
  /** `m_Orient`, degrees. */
  orient: number;
  /** `SetBoundingBox`: the turned corners' box. */
  min: { x: number; y: number };
  max: { x: number; y: number };
}

export interface DsBitmapItem extends DsItemBase {
  type: "bitmap";
  /** The PNG (base64), or null when the item has no readable image (`DS_DATA_ITEM_BITMAP::SyncDrawItems` then draws nothing). */
  png: string | null;
  /** The image's own pixel size and resolution (`BITMAP_BASE::GetPPI`, 300 without a `pHYs` chunk). */
  pixels: { w: number; h: number };
  imagePpi: number;
  /** `BITMAP_BASE::GetScale` (`(scale ..)`). */
  scale: number;
}

export type DsItem = DsSegmentItem | DsRectItem | DsTextItem | DsPolygonsItem | DsBitmapItem;

/** `DS_DATA_MODEL`: the setup's defaults, the margins and the items in file order. */
export interface DsModel {
  /** `m_fileFormatVersionAtLoad`: the `(version ..)`, 0 for a `page_layout` file. */
  version: number;
  /** `m_DefaultLineWidth`, mm (0 until the setup says). */
  defaultLineWidth: number;
  /** `m_DefaultTextSize`, mm (`TB_DEFAULT_TEXTSIZE`, 1.5). */
  defaultTextSize: { x: number; y: number };
  /** `m_DefaultTextThickness`, mm (0 until the setup says). */
  defaultTextThickness: number;
  leftMargin: number;
  rightMargin: number;
  topMargin: number;
  bottomMargin: number;
  /** `AllowVoidList`: a well-formed file with a setup may have no items. */
  allowVoidList: boolean;
  items: DsItem[];
}

/** `TB_DEFAULT_TEXTSIZE`. */
export const TB_DEFAULT_TEXTSIZE = 1.5;
/** `SEXPR_WORKSHEET_FILE_VERSION`: the newest format this reader knows (ds_file_versions.h). */
export const SEXPR_WORKSHEET_FILE_VERSION = 20231118;

/** `DS_DATA_MODEL::DS_DATA_MODEL()`. */
export function emptyModel(): DsModel {
  return {
    version: 0,
    defaultLineWidth: 0,
    defaultTextSize: { x: TB_DEFAULT_TEXTSIZE, y: TB_DEFAULT_TEXTSIZE },
    defaultTextThickness: 0,
    leftMargin: 10,
    rightMargin: 10,
    topMargin: 10,
    bottomMargin: 10,
    allowVoidList: false,
    items: [],
  };
}

function baseItem(): DsItemBase {
  return { name: "", info: "", pageOption: "all", pos: { x: 0, y: 0, anchor: "rb" }, end: { x: 0, y: 0, anchor: "rb" }, lineWidth: 0, repeat: 1, incr: { x: 0, y: 0 }, incrLabel: 1 };
}

// ------------------------------------------------------------------------------------------------------------------------------------- lexer

type TokKind = "left" | "right" | "symbol" | "string" | "number" | "eof";

interface Tok {
  kind: TokKind;
  text: string;
  line: number;
  col: number;
}

/** DSNLEXER's number test: an optional sign, digits with at most one point, an optional exponent. */
const NUMBER = /^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/;

/** The quoted-string escapes of `DSNLEXER::NextTok` (non-specctra mode). */
function unescape(body: string): string {
  let out = "";
  for (let i = 0; i < body.length; i++) {
    const c = body[i]!;
    if (c !== "\\") {
      out += c;
      continue;
    }
    const n = body[++i];
    if (n === undefined) break;
    switch (n) {
      case '"':
      case "\\":
        out += n;
        break;
      case "a":
        out += "\x07";
        break;
      case "b":
        out += "\x08";
        break;
      case "f":
        out += "\x0c";
        break;
      case "n":
        out += "\n";
        break;
      case "r":
        out += "\r";
        break;
      case "t":
        out += "\t";
        break;
      case "v":
        out += "\x0b";
        break;
      case "x": {
        const m = /^[0-9a-fA-F]{1,2}/.exec(body.slice(i + 1));
        if (m) {
          out += String.fromCharCode(parseInt(m[0], 16));
          i += m[0].length;
        } else out += "x";
        break;
      }
      default: {
        const m = /^[0-7]{1,3}/.exec(body.slice(i));
        if (m) {
          out += String.fromCharCode(parseInt(m[0], 8));
          i += m[0].length - 1;
        } else {
          out += "\\";
          i--;
        }
      }
    }
  }
  return out;
}

/** The file's tokens; a line whose first non-blank character is `#` is a comment (DSNLEXER's `commentsAreTokens` off). */
function tokenize(text: string): Tok[] | { error: string; line: number; col: number } {
  const toks: Tok[] = [];
  let i = 0;
  let line = 1;
  let lineStart = 0;
  let atLineStart = true;
  while (i < text.length) {
    const c = text[i]!;
    if (c === "\n") {
      line++;
      i++;
      lineStart = i;
      atLineStart = true;
      continue;
    }
    if (c === " " || c === "\t" || c === "\r" || c === "\f" || c === "\v") {
      i++;
      continue;
    }
    if (c === "#" && atLineStart) {
      while (i < text.length && text[i] !== "\n") i++;
      continue;
    }
    atLineStart = false;
    const col = i - lineStart + 1;
    if (c === "(" || c === ")") {
      toks.push({ kind: c === "(" ? "left" : "right", text: c, line, col });
      i++;
      continue;
    }
    if (c === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') {
        if (text[j] === "\\") j++;
        if (text[j] === "\n") line++;
        j++;
      }
      if (j >= text.length) return { error: "Unterminated delimited string", line, col };
      toks.push({ kind: "string", text: unescape(text.slice(i + 1, j)), line, col });
      i = j + 1;
      continue;
    }
    let j = i;
    while (j < text.length && !/[\s()"]/.test(text[j]!)) j++;
    const word = text.slice(i, j);
    toks.push({ kind: NUMBER.test(word) ? "number" : "symbol", text: word, line, col });
    i = j;
  }
  toks.push({ kind: "eof", text: "", line, col: i - lineStart + 1 });
  return toks;
}

// ------------------------------------------------------------------------------------------------------------------------------------- parser

class ParseError extends Error {
  constructor(
    message: string,
    readonly line: number,
    readonly col: number,
  ) {
    super(message);
  }
}

/** `convertLegacyVariableRefs`: the `%X` formats of a file older than the variables, as `${..}` (`%%` is a percent sign). */
export function convertLegacyVariableRefs(textbase: string): string {
  let msg = "";
  const comment: Record<string, string> = { "0": "${COMMENT1}", "1": "${COMMENT2}", "2": "${COMMENT3}", "3": "${COMMENT4}", "4": "${COMMENT5}", "5": "${COMMENT6}", "6": "${COMMENT7}", "7": "${COMMENT8}", "8": "${COMMENT9}" };
  const simple: Record<string, string> = {
    "%": "%",
    D: "${ISSUE_DATE}",
    R: "${REVISION}",
    K: "${KICAD_VERSION}",
    Z: "${PAPER}",
    S: "${#}",
    N: "${##}",
    F: "${FILENAME}",
    L: "${LAYER}",
    P: "${SHEETPATH}",
    Y: "${COMPANY}",
    T: "${TITLE}",
  };
  for (let ii = 0; ii < textbase.length; ii++) {
    if (textbase[ii] !== "%") {
      msg += textbase[ii];
      continue;
    }
    if (++ii >= textbase.length) break;
    const format = textbase[ii]!;
    if (format === "C") {
      const n = textbase[++ii];
      if (n !== undefined && comment[n]) msg += comment[n];
    } else if (simple[format] !== undefined) {
      msg += simple[format];
    }
  }
  return msg;
}

/** `ConvertToNewOverbarNotation` (common/string_utils.cpp): `~text~` overbars of files before 20210606 as `~{text}`. */
export function convertToNewOverbarNotation(old: string): string {
  if (old === "~") return old;
  let out = "";
  let inOverbar = false;
  for (let i = 0; i < old.length; i++) {
    const ch = old[i]!;
    if (ch === "~") {
      const next = old[i + 1];
      if (next === "~") {
        if (old[i + 2] === "{") {
          out += "~~{}";
          continue;
        }
        out += "~";
        i++;
        continue;
      } else if (next === "{") {
        return old;
      } else {
        out += inOverbar ? "}" : "~{";
        inOverbar = !inOverbar;
        continue;
      }
    } else if ((ch === " " || ch === "}" || ch === ")") && inOverbar) {
      out += "}";
      inOverbar = false;
    }
    out += ch;
  }
  if (inOverbar) out += "}";
  return out;
}

/** What reading a `.kicad_wks` gives: the model (with every item read before an error, as the C++ appends as it goes) and the error, if any. */
export interface DsParseResult {
  model: DsModel;
  /** `IO_ERROR::What()`-style text: "Unexpected foo in 'source', line 3, offset 7.", or the future-format message. */
  error: string | null;
}

/** The FUTURE_FORMAT_ERROR text (common/exceptions.cpp `FUTURE_FORMAT_ERROR::init`). */
function futureFormatMessage(version: number, generatorVersion: string): string {
  return generatorVersion
    ? `KiCad was unable to open this file because it was created with a more recent version than the one you are running.\n\nTo open it you will need to upgrade KiCad to version ${generatorVersion} or later (file format dated ${version} or later).`
    : `KiCad was unable to open this file because it was created with a more recent version than the one you are running.\n\nTo open it you will need to upgrade KiCad to a version dated ${version} or later.`;
}

class Parser {
  private i = 0;
  private requiredVersion = 0;
  private generatorVersion = "";

  constructor(
    private readonly toks: Tok[],
    private readonly source: string,
  ) {}

  private get cur(): Tok {
    return this.toks[Math.max(0, this.i - 1)]!;
  }

  private next(): Tok {
    const t = this.toks[this.i] ?? this.toks[this.toks.length - 1]!;
    if (this.i < this.toks.length) this.i++;
    return t;
  }

  private fail(problem: string): never {
    const t = this.cur;
    throw new ParseError(`${problem} in '${this.source}', line ${t.line}, offset ${t.col}.`, t.line, t.col);
  }

  /** `DSNLEXER::Unexpected( CurText() )`. */
  private unexpected(): never {
    this.fail(`Unexpected ${this.cur.text}`);
  }

  private needLeft(): void {
    if (this.next().kind !== "left") this.fail("Expecting '('");
  }

  private needRight(): void {
    if (this.next().kind !== "right") this.fail("Expecting ')'");
  }

  /** `NeedSYMBOLorNUMBER`: a symbol, a keyword, a quoted string or a number. */
  private needSymbolOrNumber(): string {
    const t = this.next();
    if (t.kind === "symbol" || t.kind === "string" || t.kind === "number") return t.text;
    this.fail("Expecting a symbol or number");
  }

  /** `parseInt`: a number token, read as `atoi` reads it. */
  private int(): number {
    const t = this.next();
    if (t.kind !== "number") this.fail("Expecting 'number'");
    const v = parseInt(t.text, 10);
    return Number.isFinite(v) ? v : 0;
  }

  private intIn(min: number, max: number): number {
    return Math.min(max, Math.max(min, this.int()));
  }

  private double(): number {
    const t = this.next();
    if (t.kind !== "number") this.fail("Expecting 'number'");
    return Number(t.text);
  }

  /** The keyword a token is, when it is one (a quoted string never is). */
  private static kw(t: Tok): string | null {
    return t.kind === "symbol" ? t.text : null;
  }

  parse(model: DsModel): void {
    this.needLeft();
    const head = this.next();
    this.parseHeader(head);
    model.version = this.requiredVersion;
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      switch (Parser.kw(t)) {
        case "generator":
          this.needSymbolOrNumber();
          this.needRight();
          break;
        case "generator_version":
          this.generatorVersion = this.next().text;
          this.needRight();
          break;
        case "setup":
          if (this.requiredVersion > SEXPR_WORKSHEET_FILE_VERSION) throw new ParseError(futureFormatMessage(this.requiredVersion, this.generatorVersion), t.line, t.col);
          this.parseSetup(model);
          break;
        case "line":
        case "rect": {
          const item = { ...baseItem(), type: Parser.kw(t) === "line" ? "segment" : "rect" } as DsSegmentItem | DsRectItem;
          this.parseGraphic(item);
          model.items.push(item);
          break;
        }
        case "polygon": {
          const item: DsPolygonsItem = { ...baseItem(), type: "poly", corners: [], polyEnds: [], orient: 0, min: { x: 0, y: 0 }, max: { x: 0, y: 0 } };
          this.parsePolygon(item);
          model.items.push(item);
          break;
        }
        case "bitmap": {
          const item: DsBitmapItem = { ...baseItem(), type: "bitmap", png: null, pixels: { w: 0, h: 0 }, imagePpi: 300, scale: 1 };
          this.parseBitmap(item);
          // "Drop invalid bitmaps"
          if (item.png) model.items.push(item);
          break;
        }
        case "tbtext": {
          const text = convertLegacyVariableRefs(this.needSymbolOrNumber());
          const item: DsTextItem = { ...baseItem(), type: "text", textBase: text, hJustify: "left", vJustify: "center", italic: false, bold: false, face: null, size: { x: 0, y: 0 }, color: null, orient: 0, maxLen: 0, maxHeight: 0 };
          this.parseText(item);
          model.items.push(item);
          break;
        }
        default:
          this.unexpected();
      }
    }
  }

  /** `parseHeader`: `kicad_wks` / `drawing_sheet` carry `(version N)`; the older `page_layout` files have none (version 0). */
  private parseHeader(head: Tok): void {
    const kw = Parser.kw(head);
    if (kw === "kicad_wks" || kw === "drawing_sheet") {
      this.needLeft();
      const t = this.next();
      if (Parser.kw(t) === "version") {
        this.requiredVersion = this.int();
        this.needRight();
      } else {
        this.fail("Expecting 'version'");
      }
    } else {
      this.requiredVersion = 0;
    }
  }

  private parseSetup(model: DsModel): void {
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") continue;
      switch (Parser.kw(t)) {
        case "linewidth":
          model.defaultLineWidth = this.double();
          this.needRight();
          break;
        case "textsize":
          model.defaultTextSize = { x: this.double(), y: this.double() };
          this.needRight();
          break;
        case "textlinewidth":
          model.defaultTextThickness = this.double();
          this.needRight();
          break;
        case "left_margin":
          model.leftMargin = this.double();
          this.needRight();
          break;
        case "right_margin":
          model.rightMargin = this.double();
          this.needRight();
          break;
        case "top_margin":
          model.topMargin = this.double();
          this.needRight();
          break;
        case "bottom_margin":
          model.bottomMargin = this.double();
          this.needRight();
          break;
        default:
          this.unexpected();
      }
    }
    // "The file is well-formed. If it has no further items, then that's the way the user wants it."
    model.allowVoidList = true;
  }

  private readOption(item: DsItemBase): void {
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      switch (Parser.kw(t)) {
        case "page1only":
          item.pageOption = "first";
          break;
        case "notonpage1":
          item.pageOption = "subsequent";
          break;
        default:
          this.unexpected();
      }
    }
  }

  private parseCoordinate(p: DsPoint): void {
    p.x = this.double();
    p.y = this.double();
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      switch (Parser.kw(t)) {
        case "ltcorner":
          p.anchor = "lt";
          break;
        case "lbcorner":
          p.anchor = "lb";
          break;
        case "rbcorner":
          p.anchor = "rb";
          break;
        case "rtcorner":
          p.anchor = "rt";
          break;
        default:
          this.unexpected();
      }
    }
  }

  /** The keys every item kind reads alike; true when `kw` was one of them. */
  private common(kw: string | null, item: DsItemBase): boolean {
    switch (kw) {
      case "comment":
        item.info = this.needSymbolOrNumber();
        this.needRight();
        return true;
      case "option":
        this.readOption(item);
        return true;
      case "name":
        item.name = this.needSymbolOrNumber();
        this.needRight();
        return true;
      case "repeat":
        item.repeat = this.intIn(1, 100);
        this.needRight();
        return true;
      case "incrx":
        item.incr.x = this.double();
        this.needRight();
        return true;
      case "incry":
        item.incr.y = this.double();
        this.needRight();
        return true;
      case "linewidth":
        item.lineWidth = this.double();
        this.needRight();
        return true;
      default:
        return false;
    }
  }

  private parseGraphic(item: DsSegmentItem | DsRectItem): void {
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") {
        t = this.next();
      } else if (Parser.kw(t) !== "end") {
        // "due to a old bug in kicad, the token T_end can be found without T_LEFT in a very few .wks files"
        this.unexpected();
      }
      const kw = Parser.kw(t);
      if (this.common(kw, item)) continue;
      switch (kw) {
        case "start":
          this.parseCoordinate(item.pos);
          break;
        case "end":
          this.parseCoordinate(item.end);
          break;
        default:
          this.unexpected();
      }
    }
  }

  private parseText(item: DsTextItem): void {
    if (this.requiredVersion < 20210606) item.textBase = convertToNewOverbarNotation(item.textBase);
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      const kw = Parser.kw(t);
      if (kw !== "linewidth" && this.common(kw, item)) continue;
      switch (kw) {
        case "pos":
          this.parseCoordinate(item.pos);
          break;
        case "incrlabel":
          item.incrLabel = this.intIn(-2147483648, 2147483647);
          this.needRight();
          break;
        case "maxlen":
          item.maxLen = this.double();
          this.needRight();
          break;
        case "maxheight":
          item.maxHeight = this.double();
          this.needRight();
          break;
        case "font":
          item.color = null;
          for (let f = this.next(); f.kind !== "right" && f.kind !== "eof"; f = this.next()) {
            if (f.kind === "left") continue;
            switch (Parser.kw(f)) {
              case "face":
                item.face = this.next().text;
                this.needRight();
                break;
              case "bold":
                item.bold = true;
                break;
              case "italic":
                item.italic = true;
                break;
              case "size":
                item.size = { x: this.double(), y: this.double() };
                this.needRight();
                break;
              case "color":
                item.color = { r: this.intIn(0, 255) / 255, g: this.intIn(0, 255) / 255, b: this.intIn(0, 255) / 255, a: Math.min(1, Math.max(0, this.double())) };
                this.needRight();
                break;
              case "linewidth":
                item.lineWidth = this.double();
                this.needRight();
                break;
              default:
                this.unexpected();
            }
          }
          if (item.face === "") item.face = null;
          break;
        case "justify":
          for (let j = this.next(); j.kind !== "right" && j.kind !== "eof"; j = this.next()) {
            switch (Parser.kw(j)) {
              case "center":
                item.hJustify = "center";
                item.vJustify = "center";
                break;
              case "left":
                item.hJustify = "left";
                break;
              case "right":
                item.hJustify = "right";
                break;
              case "top":
                item.vJustify = "top";
                break;
              case "bottom":
                item.vJustify = "bottom";
                break;
              default:
                this.unexpected();
            }
          }
          break;
        case "rotate":
          item.orient = this.double();
          this.needRight();
          break;
        default:
          this.unexpected();
      }
    }
  }

  private parsePolygon(item: DsPolygonsItem): void {
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      const kw = Parser.kw(t);
      if (this.common(kw, item)) continue;
      switch (kw) {
        case "pos":
          this.parseCoordinate(item.pos);
          break;
        case "pts":
          this.parsePolyOutline(item);
          // `CloseContour`
          item.polyEnds.push(item.corners.length - 1);
          break;
        case "rotate":
          item.orient = this.double();
          this.needRight();
          break;
        default:
          this.unexpected();
      }
    }
    setPolygonsBoundingBox(item);
  }

  private parsePolyOutline(item: DsPolygonsItem): void {
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      if (Parser.kw(t) !== "xy") this.unexpected();
      item.corners.push({ x: this.double(), y: this.double() });
      this.needRight();
    }
  }

  private parseBitmap(item: DsBitmapItem): void {
    let bytes: Uint8Array | null = null;
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      const kw = Parser.kw(t);
      if (kw !== "comment" && kw !== "option" && kw !== "name" && kw !== "repeat" && kw !== "incrx" && kw !== "incry" && kw !== "linewidth") {
        switch (kw) {
          case "pos":
            this.parseCoordinate(item.pos);
            break;
          case "scale":
            item.scale = this.double();
            this.needRight();
            break;
          case "data": {
            let data = "";
            for (let d = this.next(); d.kind !== "right"; d = this.next()) {
              if (d.kind !== "symbol" && d.kind !== "string") this.fail(`Expecting base64 image data. Got '${d.text}'`);
              data += d.text;
            }
            bytes = base64Bytes(data);
            if (!bytes || !pngInfo(bytes)) this.fail("Failed to read image data.");
            break;
          }
          case "pngdata":
            bytes = this.readPngdata();
            break;
          default:
            this.unexpected();
        }
        continue;
      }
      this.common(kw, item);
    }
    const info = bytes ? pngInfo(bytes) : null;
    if (bytes && info) {
      item.png = bytesBase64(bytes);
      item.pixels = { w: info.width, h: info.height };
      item.imagePpi = info.ppi;
    }
  }

  /** `readPngdata`: the legacy hex lines (`(data "89 50 4E 47 ..")`), read as `BITMAP_BASE::LoadLegacyData` does. */
  private readPngdata(): Uint8Array | null {
    const bytes: number[] = [];
    for (let t = this.next(); t.kind !== "right" && t.kind !== "eof"; t = this.next()) {
      if (t.kind === "left") t = this.next();
      if (Parser.kw(t) !== "data") this.unexpected();
      for (const hex of this.needSymbolOrNumber().trim().split(/\s+/)) {
        const v = parseInt(hex, 16);
        if (Number.isFinite(v)) bytes.push(v & 0xff);
      }
      this.needRight();
    }
    const out = new Uint8Array(bytes);
    return pngInfo(out) ? out : null;
  }
}

/** `DS_DATA_ITEM_POLYGONS::SetBoundingBox`: the box of the corners turned by the item's angle. */
function setPolygonsBoundingBox(item: DsPolygonsItem): void {
  if (item.corners.length === 0) {
    item.min = { x: 0, y: 0 };
    item.max = { x: 0, y: 0 };
    return;
  }
  // `VECTOR2I pos = m_Corners[ii]` -- the corners are cast to integers (mm, truncated: VECTOR2's casting constructor) before they are turned by the
  // integer `RotatePoint`, which rounds (`KiROUND`) off the right angles
  const turned = item.corners.map((c) => rotatePoint(Math.trunc(c.x), Math.trunc(c.y), item.orient));
  const xs = turned.map((p) => kiRound(p[0]));
  const ys = turned.map((p) => kiRound(p[1]));
  item.min = { x: Math.min(...xs), y: Math.min(...ys) };
  item.max = { x: Math.max(...xs), y: Math.max(...ys) };
}

/** `RotatePoint( double*, double*, EDA_ANGLE )` (trigo.cpp): positive is counter-clockwise on a y-down page. */
export function rotatePoint(x: number, y: number, angleDeg: number): [number, number] {
  const a = (((angleDeg % 360) + 360) % 360);
  if (a === 0) return [x, y];
  if (a === 90) return [y, -x];
  if (a === 180) return [-x, -y];
  if (a === 270) return [-y, x];
  const r = (angleDeg * Math.PI) / 180;
  const s = Math.sin(r);
  const c = Math.cos(r);
  return [x * c + y * s, y * c - x * s];
}

// ------------------------------------------------------------------------------------------------------------------------------------- PNG

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function base64Bytes(text: string): Uint8Array | null {
  const clean = text.replace(/[^A-Za-z0-9+/]/g, "");
  const out: number[] = [];
  let buf = 0;
  let bits = 0;
  for (const ch of clean) {
    buf = (buf << 6) | B64.indexOf(ch);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push((buf >> bits) & 0xff);
    }
  }
  return out.length > 0 ? new Uint8Array(out) : null;
}

function bytesBase64(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i]! << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    out += B64[(n >> 18) & 63]! + B64[(n >> 12) & 63]! + (i + 1 < bytes.length ? B64[(n >> 6) & 63]! : "=") + (i + 2 < bytes.length ? B64[n & 63]! : "=");
  }
  return out;
}

/** A PNG's size and resolution: the IHDR's width and height, and the `pHYs` chunk's pixels per metre as PPI (wx reads it as per centimetre,
 *  `BITMAP_BASE::updatePPI` turns that into `KiROUND( dpi * 2.54 )`); 300 without one. Null when the bytes are no PNG. */
export function pngInfo(bytes: Uint8Array): { width: number; height: number; ppi: number } | null {
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (bytes.length < 33 || sig.some((b, i) => bytes[i] !== b)) return null;
  const u32 = (o: number) => ((bytes[o]! << 24) >>> 0) + (bytes[o + 1]! << 16) + (bytes[o + 2]! << 8) + bytes[o + 3]!;
  const width = u32(16);
  const height = u32(20);
  let ppi = 300;
  for (let o = 8; o + 8 <= bytes.length; ) {
    const len = u32(o);
    const type = String.fromCharCode(bytes[o + 4]!, bytes[o + 5]!, bytes[o + 6]!, bytes[o + 7]!);
    if (type === "pHYs" && len >= 9 && o + 17 <= bytes.length) {
      const ppm = u32(o + 8);
      const unit = bytes[o + 16];
      if (unit === 1) {
        const perCm = Math.trunc(ppm / 100);
        if (perCm > 1) ppi = Math.round(perCm * 2.54);
      }
    }
    if (type === "IEND") break;
    o += 12 + len;
  }
  return { width, height, ppi };
}

// ------------------------------------------------------------------------------------------------------------------------------------- reading

/**
 * Reads a `.kicad_wks` text (`DS_DATA_MODEL::LoadDrawingSheet` / `SetPageLayout`): the items the file has, in order. On a parse error the items read before it
 * stay and `error` says what went wrong (`IO_ERROR::What()`); a sheet left with no items and no setup is drawn as KiCad's default
 * ([`effectiveModel`]).
 */
export function parseDrawingSheet(text: string, source = "Sexpr_string"): DsParseResult {
  const model = emptyModel();
  const toks = tokenize(text);
  if (!Array.isArray(toks)) return { model, error: `${toks.error} in '${source}', line ${toks.line}, offset ${toks.col}.` };
  try {
    new Parser(toks, source).parse(model);
    return { model, error: null };
  } catch (e) {
    if (e instanceof ParseError) return { model, error: e.message };
    throw e;
  }
}

let defaultModelCache: DsModel | null = null;

/** `DS_DATA_MODEL::SetDefaultLayout`: KiCad's default sheet, read once. */
export function defaultModel(): DsModel {
  if (!defaultModelCache) defaultModelCache = parseDrawingSheet(DEFAULT_DRAWING_SHEET, "default page").model;
  return defaultModelCache;
}

/** `BuildDrawItemsList`'s "Build the basic layout shape, if the layout list is empty": a model with no items that does not allow it is the default sheet. */
export function effectiveModel(model: DsModel | null | undefined): DsModel {
  if (!model || (model.items.length === 0 && !model.allowVoidList)) return defaultModel();
  return model;
}

// ------------------------------------------------------------------------------------------------------------------------------------- text variables

/** `ExpandTextVars` (common/common.cpp): `${TOKEN}` replaced by what `resolve` says (null: left as written), nested `${..}` expanded first, an escaped
 *  `\${..}` kept as written (KiCad keeps it behind an escape marker until the text is shown, which then shows `${..}`); `@{..}` is copied with its
 *  variables expanded but not evaluated. */
export function expandTextVars(source: string, resolve: (token: string) => string | null, depth = 0): string {
  const MAX_DEPTH = 10; // ADVANCED_CFG::m_ResolveTextRecursionDepth's default
  let out = "";
  for (let i = 0; i < source.length; i++) {
    const c = source[i]!;
    if (c === "\\" && (source[i + 1] === "$" || source[i + 1] === "@") && source[i + 2] === "{") {
      // copy `${...}` through its matching brace, without the backslash
      out += source[i + 1]! + "{";
      let braces = 1;
      let j = i + 3;
      for (; j < source.length && braces > 0; j++) {
        if (source[j] === "{") braces++;
        else if (source[j] === "}") braces--;
        out += source[j];
      }
      i = j - 1;
      continue;
    }
    if ((c === "$" || c === "@") && source[i + 1] === "{") {
      let token = "";
      let braces = 1;
      let j = i + 2;
      for (; j < source.length; j++) {
        const d = source[j]!;
        if (d === "{") {
          braces++;
          token += d;
        } else if (d === "}") {
          braces--;
          if (braces === 0) break;
          token += d;
        } else token += d;
      }
      i = j;
      if (token === "") continue;
      if (token.includes("${") || token.includes("@{")) {
        if (depth < MAX_DEPTH) token = expandTextVars(token, resolve, depth + 1);
      }
      if (c === "@") {
        out += `@{${token}}`;
        continue;
      }
      const v = resolve(token);
      out += v === null ? `\${${token}}` : v;
      continue;
    }
    out += c;
  }
  return out;
}

/** What a page's title block says (`TITLE_BLOCK`). */
export interface DsTitleBlock {
  title: string;
  date: string;
  rev: string;
  company: string;
  /** Comment 1 first; up to nine. */
  comments: readonly string[];
}

/** What one page of the drawing sheet is built for (`DS_PROXY_VIEW_ITEM::buildDrawList`'s setters and the page). */
export interface DsPageContext {
  /** The paper, nm (`PAGE_INFO::GetSizeMils`). */
  widthNm: number;
  heightNm: number;
  /** `PAGE_INFO::GetTypeAsString`: "A4", "USLetter", "User". */
  paperName: string;
  titleBlock: DsTitleBlock;
  /** `${FILENAME}` is its name, `${FILEPATH}` all of it. */
  fileName: string;
  sheetName: string;
  sheetPath: string;
  /** `${#}`: the page number as the page shows it. */
  pageNumber: string;
  /** `${##}`. */
  sheetCount: number;
  /** `SetIsFirstPage`: what `page1only` / `notonpage1` items go by (the board is always its first page). */
  isFirstPage: boolean;
  /** `${LAYER}` (`RENDER_SETTINGS::GetLayerName`). */
  sheetLayer?: string;
  /** `${VARIANT}` / `${VARIANT_DESC}`. */
  variantName?: string;
  variantDesc?: string;
  /** `${KICAD_VERSION}`. */
  kicadVersion: string;
  /** `DS_DRAW_ITEM_LIST::m_penSize` (`RENDER_SETTINGS::GetDrawingSheetLineWidth`, 100000 IU): the width of an item that has none, nm. */
  defaultPenNm?: number;
  /** The project's own variables and the `m_properties` (`PROJECT::TextVarResolver`); null for one it does not know. */
  projectVar?: (name: string) => string | null;
}

/** `CURRENT_DATE` (`TITLE_BLOCK::GetCurrentDate`): today, ISO. */
function currentDate(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** `TITLE_BLOCK::TextVarResolver`: the title block's fields; null for a token it does not have. */
export function titleBlockVar(tb: DsTitleBlock, token: string): string | null {
  switch (token) {
    case "ISSUE_DATE":
      return tb.date;
    case "CURRENT_DATE":
      return currentDate();
    case "REVISION":
      return tb.rev;
    case "TITLE":
      return tb.title;
    case "COMPANY":
      return tb.company;
  }
  if (token.length === 8 && token.startsWith("COMMENT")) {
    const c = token[7]!;
    if (c >= "1" && c <= "9") return tb.comments[c.charCodeAt(0) - 49] ?? "";
  }
  return null;
}

/** `wxFileName::GetFullName`: the last component of a path. */
function fullName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

/** `DS_DRAW_ITEM_LIST::BuildFullText`: the sheet's own variables, then the title block's (whose values may hold variables of their own), then the project's. */
export function buildFullText(textbase: string, ctx: DsPageContext): string {
  const resolver = (withTitleBlock: boolean) => (token: string): string | null => {
    let v: string | null = null;
    switch (token) {
      case "KICAD_VERSION":
        v = ctx.kicadVersion;
        break;
      case "#":
        v = ctx.pageNumber;
        break;
      case "##":
        v = String(ctx.sheetCount);
        break;
      case "SHEETNAME":
        v = ctx.sheetName;
        break;
      case "SHEETPATH":
        v = ctx.sheetPath;
        break;
      case "FILENAME":
        v = fullName(ctx.fileName);
        break;
      case "FILEPATH":
        return ctx.fileName;
      case "PAPER":
        v = ctx.paperName;
        break;
      case "LAYER":
        v = ctx.sheetLayer ?? "";
        break;
      case "VARIANT":
        v = ctx.variantName ?? "";
        break;
      case "VARIANT_DESC":
        v = ctx.variantDesc ?? "";
        break;
    }
    if (v === null && withTitleBlock) {
      const tb = titleBlockVar(ctx.titleBlock, token);
      // the title block's own value may hold variables: the worksheet resolver again, without the title block
      if (tb !== null) return expandTextVars(tb, resolver(false));
    }
    if (v !== null) return ctx.projectVar ? expandTextVars(v, (t) => ctx.projectVar!(t)) : v;
    return ctx.projectVar?.(token) ?? null;
  };
  return expandTextVars(textbase, resolver(true));
}

// ------------------------------------------------------------------------------------------------------------------------------------- draw items

/** A point, nm. */
export type DsXY = [number, number];

export interface DsDrawLine {
  kind: "line";
  start: DsXY;
  end: DsXY;
  widthNm: number;
}

export interface DsDrawRect {
  kind: "rect";
  start: DsXY;
  end: DsXY;
  widthNm: number;
}

export interface DsDrawPoly {
  kind: "poly";
  pos: DsXY;
  /** The outlines, nm, each closed. */
  outlines: DsXY[][];
  widthNm: number;
}

export interface DsDrawText {
  kind: "text";
  text: string;
  pos: DsXY;
  /** The text's width and height, nm (`m_ConstrainedTextSize`). */
  size: DsXY;
  /** `SetTextThickness`: the pen, nm. */
  penNm: number;
  hJustify: DsHAlign;
  vJustify: DsVAlign;
  angleDeg: number;
  italic: boolean;
  bold: boolean;
  multiline: boolean;
  color: DsColor | null;
}

export interface DsDrawBitmap {
  kind: "bitmap";
  /** The image's centre, nm. */
  pos: DsXY;
  /** Its drawn size, nm (`BITMAP_BASE::GetSize`: the pixels times `GetScalingFactor`). */
  size: DsXY;
  png: string;
}

export type DsDrawItem = DsDrawLine | DsDrawRect | DsDrawPoly | DsDrawText | DsDrawBitmap;

/** `KiROUND`: half away from zero. */
function kiRound(v: number): number {
  return v < 0 ? -Math.round(-v) : Math.round(v);
}

/** mm to nm (`m_WSunits2Iu`). */
const IU = 1_000_000;

/** The page's two corners inside the margins, mm (`SetupDrawEnvironment`'s `m_LT_Corner`, `m_RB_Corner`). */
interface Corners {
  lt: { x: number; y: number };
  rb: { x: number; y: number };
}

function corners(model: DsModel, ctx: DsPageContext): Corners {
  return { lt: { x: model.leftMargin, y: model.topMargin }, rb: { x: ctx.widthNm / IU - model.rightMargin, y: ctx.heightNm / IU - model.bottomMargin } };
}

/** `DS_DATA_ITEM::GetStartPos` / `GetEndPos`: the `ii`-th repeat of a point, mm on the page. */
function pointAt(p: DsPoint, incr: { x: number; y: number }, ii: number, c: Corners): { x: number; y: number } {
  const x = p.x + incr.x * ii;
  const y = p.y + incr.y * ii;
  switch (p.anchor) {
    case "rb":
      return { x: c.rb.x - x, y: c.rb.y - y };
    case "rt":
      return { x: c.rb.x - x, y: c.lt.y + y };
    case "lb":
      return { x: c.lt.x + x, y: c.rb.y - y };
    case "lt":
      return { x: c.lt.x + x, y: c.lt.y + y };
  }
}

const toIU = (p: { x: number; y: number }): DsXY => [kiRound(p.x * IU), kiRound(p.y * IU)];

/** `DS_DATA_ITEM::IsInsidePage`: both points within the margins. */
function insidePage(item: DsItem, ii: number, c: Corners): boolean {
  if (item.type === "poly") {
    const s = pointAt(item.pos, item.incr, ii, c);
    if (c.lt.x > s.x + item.min.x || c.lt.y > s.y + item.min.y) return false;
    if (c.rb.x < s.x + item.max.x || c.rb.y < s.y + item.max.y) return false;
    return true;
  }
  for (const p of [pointAt(item.pos, item.incr, ii, c), pointAt(item.end, item.incr, ii, c)]) {
    if (c.rb.x < p.x || c.lt.x > p.x) return false;
    if (c.rb.y < p.y || c.lt.y > p.y) return false;
  }
  return true;
}

/** `GetPenSizeForBold`, `GetPenSizeForNormal` (gr_text.cpp). */
export const penSizeForBold = (size: number): number => kiRound(size / 5);
export const penSizeForNormal = (size: number): number => kiRound(size / 8);

/** `EDA_TEXT::GetEffectiveTextPenWidth` with no default (the DS painter's `GetDefaultPenWidth` is 0): a pen of 1 IU or less is the size's, and
 *  every pen is clamped to a quarter of the smaller side (`ClampTextPenSize`). */
export function effectiveTextPen(t: Pick<DsDrawText, "penNm" | "size" | "bold">): number {
  let pen = t.penNm;
  if (pen <= 1) pen = t.bold ? penSizeForBold(t.size[0]) : penSizeForNormal(t.size[0]);
  return Math.min(pen, kiRound(Math.min(Math.abs(t.size[0]), Math.abs(t.size[1])) * 0.25));
}

/** How wide a line of text is set at `sizeX` x `sizeY` (the stroke font's advance), in the units of the sizes. */
export type DsMeasure = (text: string, sizeX: number, sizeY: number) => number;

/** `STROKE_FONT::GetInterline`: the glyph height times `METRICS::m_InterlinePitch` (1.68) and the legacy factor (0.9583), rounded. */
export function interline(sizeY: number): number {
  return Math.round(sizeY * 1.68 * 0.9583);
}

/** `DS_DATA_ITEM_TEXT::ReplaceAntiSlashSequence`: `\n` in the text is a line break and `\\` a backslash; true when the text has more than one line. */
export function replaceAntiSlashSequence(text: string): { text: string; multiline: boolean } {
  let out = "";
  let multiline = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === "\n") {
      multiline = true;
      out += c;
    } else if (c === "\\") {
      const n = text[i + 1];
      if (n === undefined) {
        out += c;
        break;
      }
      if (n === "\\") {
        out += "\\";
        i++;
      } else if (n === "n") {
        multiline = true;
        out += "\n";
        i++;
      } else out += c;
    } else out += c;
  }
  return { text: out, multiline };
}

/** `DS_DATA_ITEM_TEXT::IncrementLabel`: the base text with its last character stepped by `incr` (a digit becomes the number, anything else the character so many on). */
export function incrementLabel(textBase: string, incr: number): string {
  if (textBase.length === 0) return textBase;
  const last = textBase[textBase.length - 1]!;
  const head = textBase.slice(0, -1);
  if (last >= "0" && last <= "9") return head + String(incr + last.charCodeAt(0) - 48);
  return head + String.fromCharCode(incr + last.charCodeAt(0));
}

/** `DS_DATA_ITEM_TEXT::SetConstrainedTextSize`: the text's size (the model's default where the item has none), shrunk to fit `maxlen` / `maxheight`. */
function constrainedTextSize(item: DsTextItem, fullText: string, model: DsModel, measure: DsMeasure): { x: number; y: number } {
  const size = { x: item.size.x || model.defaultTextSize.x, y: item.size.y || model.defaultTextSize.y };
  if (item.maxLen > 0 || item.maxHeight > 0) {
    // `EDA_TEXT::GetTextBox` of a dummy with no pen: the pen is the size's, the box the widest line plus the pen's reach, `0.17` taller than the ink
    const lines = fullText.split("\n");
    const pen = item.bold ? penSizeForBold(size.x) : penSizeForNormal(size.x);
    const k = Math.round(pen * 1.5 * 1000) / 1000;
    const w = Math.max(...lines.map((l) => measure(l, size.x, size.y))) + 2 * k;
    const ext = size.y + 2 * k;
    const h = ext + Math.round(ext * 0.17 * 1000) / 1000 + (lines.length - 1) * (size.y * 1.68 * 0.9583);
    const bw = Math.round(w);
    const bh = Math.round(h);
    if (item.maxLen > 0 && bw > item.maxLen) size.x *= item.maxLen / bw;
    if (item.maxHeight > 0 && bh > item.maxHeight) size.y *= item.maxHeight / bh;
  }
  return size;
}

/** A rough advance for a model built without a font at hand: 0.9 of the width per character (the stroke font's mean). The painter passes the real one. */
export const roughMeasure: DsMeasure = (text, sizeX) => text.length * sizeX * 0.9;

/**
 * `DS_DRAW_ITEM_LIST::BuildDrawItemsList`: the draw items of one page -- every item the page-1 option lets on this page, each repeat that stays inside
 * the margins (the first always), texts with their variables expanded (`SyncDrawItems`).
 */
export function buildDrawItems(model: DsModel | null | undefined, ctx: DsPageContext, measure: DsMeasure = roughMeasure): DsDrawItem[] {
  const m = effectiveModel(model);
  const c = corners(m, ctx);
  const defaultPen = ctx.defaultPenNm ?? 100_000;
  const out: DsDrawItem[] = [];
  for (const item of m.items) {
    if (item.pageOption === "first" && !ctx.isFirstPage) continue;
    if (item.pageOption === "subsequent" && ctx.isFirstPage) continue;
    switch (item.type) {
      case "segment":
      case "rect": {
        let pen = kiRound((item.lineWidth || m.defaultLineWidth) * IU);
        if (pen === 0) pen = defaultPen;
        for (let j = 0; j < item.repeat; j++) {
          if (j > 0 && !insidePage(item, j, c)) continue;
          out.push({ kind: item.type === "segment" ? "line" : "rect", start: toIU(pointAt(item.pos, item.incr, j, c)), end: toIU(pointAt(item.end, item.incr, j, c)), widthNm: pen });
        }
        break;
      }
      case "poly": {
        // `DS_DATA_ITEM_POLYGONS::GetPenSizeIU`: its own width only
        const pen = kiRound(item.lineWidth * IU);
        for (let j = 0; j < item.repeat; j++) {
          if (j > 0 && !insidePage(item, j, c)) continue;
          const start = pointAt(item.pos, item.incr, j, c);
          const outlines: DsXY[][] = [];
          for (let k = 0; k < item.polyEnds.length; k++) {
            const first = k === 0 ? 0 : item.polyEnds[k - 1]! + 1;
            const last = item.polyEnds[k]!;
            const outline: DsXY[] = [];
            for (let n = first; n <= last; n++) {
              const corner = item.corners[n]!;
              const [rx, ry] = rotatePoint(corner.x, corner.y, item.orient);
              // `GetCornerPositionIU`: `int( pos.x )`, truncated
              outline.push([Math.trunc((rx + start.x) * IU), Math.trunc((ry + start.y) * IU)]);
            }
            outlines.push(outline);
          }
          out.push({ kind: "poly", pos: toIU(start), outlines, widthNm: pen });
        }
        break;
      }
      case "text": {
        const built = replaceAntiSlashSequence(buildFullText(item.textBase, ctx));
        let fullText = built.text;
        const multiline = built.multiline;
        let pen = kiRound((item.lineWidth || m.defaultTextThickness) * IU);
        if (pen === 0) pen = defaultPen;
        const sizeMm = constrainedTextSize(item, fullText, m, measure);
        const size: DsXY = [kiRound(sizeMm.x * IU), kiRound(sizeMm.y * IU)];
        if (item.bold) pen = penSizeForBold(Math.min(size[0], size[1]));
        for (let j = 0; j < item.repeat; j++) {
          if (j > 0 && !insidePage(item, j, c)) continue;
          out.push({ kind: "text", text: fullText, pos: toIU(pointAt(item.pos, item.incr, j, c)), size, penNm: pen, hJustify: item.hJustify, vJustify: item.vJustify, angleDeg: item.orient, italic: item.italic, bold: item.bold, multiline, color: item.color });
          // "Increment label for the next text (has no meaning for multiline texts)"
          if (item.repeat > 1 && !multiline) fullText = incrementLabel(item.textBase, (j + 1) * item.incrLabel);
        }
        break;
      }
      case "bitmap": {
        if (!item.png) break;
        // `pix_size_iu = GetMilsToIUfactor() * 1000 / PPI`, the image's PPI divided by its scale (`DS_DATA_ITEM_BITMAP::GetPPI`) giving the drawn size
        const pixNm = 25_400_000 / item.imagePpi;
        const size: DsXY = [kiRound(item.pixels.w * pixNm * item.scale), kiRound(item.pixels.h * pixNm * item.scale)];
        for (let j = 0; j < item.repeat; j++) {
          if (j > 0 && !insidePage(item, j, c)) continue;
          out.push({ kind: "bitmap", pos: toIU(pointAt(item.pos, item.incr, j, c)), size, png: item.png });
        }
        break;
      }
    }
  }
  return out;
}

// ------------------------------------------------------------------------------------------------------------------------------------- text layout

/** One line of a drawing-sheet text: its string and where its baseline starts, nm, in the text's own axes from its position (`FONT::getLinePositions`). */
export interface DsTextLine {
  text: string;
  x: number;
  y: number;
}

/**
 * `FONT::getLinePositions` for a stroke text of separate width and height: the first line's top is `size.y * 1.17` high and every further line
 * `interline` more; the block is lifted by its whole height for a bottom justification and half of it for a centre one; each line is justified on its
 * own, and the stroke font's legacy offsets (`pen / 1.52` along, `pen * 0.052` up) applied.
 */
export function textLines(t: Pick<DsDrawText, "text" | "size" | "hJustify" | "vJustify" | "multiline" | "penNm" | "bold">, measure: DsMeasure): DsTextLine[] {
  const pen = effectiveTextPen(t);
  const strings = t.multiline ? t.text.split("\n") : [t.text];
  const [sx, sy] = t.size;
  const step = interline(sy);
  const height = Math.trunc(sy * 1.17) + (strings.length - 1) * step;
  const offX = Math.trunc(pen / 1.52);
  let offY = Math.trunc(sy - pen * 0.052);
  if (t.vJustify === "center") offY -= Math.trunc(height / 2);
  else if (t.vJustify === "bottom") offY -= height;
  return strings.map((text, i) => {
    const advance = Math.trunc(measure(text, sx, sy));
    const x = t.hJustify === "left" ? offX : t.hJustify === "center" ? -Math.trunc(advance / 2) : -(advance + offX);
    return { text, x, y: offY + i * step };
  });
}
