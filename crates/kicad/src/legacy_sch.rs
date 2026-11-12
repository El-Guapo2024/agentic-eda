//! KiCad 5 (and older) legacy schematics: a `.sch` and the sheets it places, read as KiCad 8 / 10 reads them and written as the `.kicad_sch` files KiCad saves
//! them as, so the studio's own reader ([`crate::import_kicad_sch_tree_with`], [`crate::load_sheet_file`]) brings them in.
//!
//! Ported from KiCad 8303b2ad:
//!
//! * the reader -- `eeschema/sch_io/kicad_legacy/sch_io_kicad_legacy.cpp` (`LoadSchematicFile`, `loadHierarchy`, `loadFile`, `LoadContent`, `loadHeader`,
//!   `loadPageSettings`, `loadSheet`, `loadBitmap`, `loadJunction`, `loadNoConnect`, `loadWire`, `loadBusEntry`, `loadText`, `loadSymbol`, `loadBusAlias`,
//!   `CanReadSchematicFile`) and its helpers `sch_io_kicad_legacy_helpers.cpp` (`strCompare`, `parseInt`, `parseHex`, `parseDouble`, `parseChar`,
//!   `parseUnquotedString`, `parseQuotedString`, `is_eol`);
//! * what the editor does to a legacy schematic as it opens it (`files-io.cpp::SCH_EDIT_FRAME::OpenProjectFiles`, the `SCH_LEGACY` branch): the bus-to-bus
//!   entries made bus wires, the symbols linked to their library symbols (`SCH_SCREEN::UpdateSymbolLinks`: the symbol library table's library, else the
//!   project's cache library under `Lib_Name`), the duplicate time stamps replaced (`SCH_SCREENS::ReplaceDuplicateTimeStamps`), a legacy power symbol's
//!   Value made its hidden pin's name (`SCH_SCREEN::FixLegacyPowerSymbolMismatches`), and the missing symbol instances added
//!   (`SCH_SHEET_PATH::CheckForMissingSymbolInstances`);
//! * what the save writes (`sch_io_kicad_sexpr.cpp`: `saveSymbol`, `saveField`, `saveSheet`, `saveText`, `saveLine`, `saveBusEntry`, `saveJunction`,
//!   `saveNoConnect`, `saveBitmap`; `files-io.cpp::SaveProject`'s sheet file names made `.kicad_sch`), with the symbol orientation as
//!   `SCH_SYMBOL::GetOrientation` finds it, a sheet's name and file fields placed by `SCH_SHEET::AutoplaceFields`, and a sheet pin put on the sheet's edge
//!   by `SCH_SHEET_PIN::ConstrainOnEdge`;
//! * the library symbol each placed symbol is drawn from, flattened (`LIB_SYMBOL::Flatten`) and named as the schematic names it
//!   (`GetSchSymbolLibraryName`), from a library in KiCad's own format.
//!
//! **Differs**: the libraries themselves (`.lib`, `-cache.lib`) are not read here: KiCad's own legacy library reader runs in kicad-cli
//! (`kicad-cli sym upgrade`, `SCH_IO_MGR::ConvertLibrary`), and the caller hands each library symbol in as the KiCad-format text it made
//! ([`LibraryLookup`]). The new items' identifiers, which KiCad draws at random (`KIID()`), are drawn from a hash of the files' paths, so the same project is
//! read the same way twice. The junctions KiCad adds where a wire's end lands on another's middle (`SCHEMATIC::FixupJunctionsAfterImport`) are not added.

use crate::sexpr::{self, Sexpr};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Schematic internal units: 100 nm (`schIUScale`); `MilsToIU( 1 )` is 254.
const IU_PER_MIL: i64 = 254;

fn mils(v: i64) -> i64 {
    v * IU_PER_MIL
}

/// `DEFAULT_LINE_WIDTH_MILS`.
const DEFAULT_LINE_WIDTH_MILS: i64 = 6;

/// `MAX_UNIT_COUNT_PER_PACKAGE`.
const MAX_UNIT_COUNT_PER_PACKAGE: i64 = 676;

/// The file version the `.kicad_sch` files are written at: 8303b2ad's `SEXPR_SCHEMATIC_FILE_VERSION`, what KiCad saves a legacy schematic it opened as. From
/// 20250318 a `~` is the text `~` and not an empty one, as the legacy reader and the libraries kicad-cli converted have it (an unnamed legacy pin is `~`).
const WRITTEN_VERSION: i64 = 20260326;

// ------------------------------------------------------------------------------------------------ the helpers (sch_io_kicad_legacy_helpers.cpp)

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// `is_eol`.
fn is_eol(s: &str) -> bool {
    matches!(s.as_bytes().first(), None | Some(b'\n') | Some(b'\r'))
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

/// `strCompare`: `token` at the start of `line`, any case, followed by white space or the end; the rest from the next token on.
fn str_compare<'a>(token: &str, line: &'a str) -> Option<&'a str> {
    let n = token.len();
    let b = line.as_bytes();
    if b.len() < n || !line.is_char_boundary(n) || !line[..n].eq_ignore_ascii_case(token) {
        return None;
    }
    match b.get(n) {
        None => Some(&line[n..]),
        Some(&c) if is_space(c) => Some(skip_ws(&line[n..])),
        _ => None,
    }
}

/// A parse error, as `SCH_PARSE_ERROR` words it: the text, and the line it was found on.
fn err(what: &str, line_no: usize) -> String {
    format!("{what} at line {line_no}")
}

/// `parseInt` (`strtol( .., 10 )`): white space, a sign and digits; no digit is 0 and leaves the line where it was. The white space after is skipped.
fn parse_int(line: &str, line_no: usize) -> Result<(i64, &str), String> {
    if line.is_empty() {
        return Err(err("unexpected end of line", line_no));
    }
    let s = skip_ws(line);
    let b = s.as_bytes();
    let mut i = 0;
    let neg = matches!(b.first(), Some(b'-'));
    if matches!(b.first(), Some(b'-') | Some(b'+')) {
        i = 1;
    }
    let start = i;
    let mut v: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = v.checked_mul(10).and_then(|v| v.checked_add((b[i] - b'0') as i64)).ok_or_else(|| err("invalid integer value", line_no))?;
        i += 1;
    }
    if i == start {
        return Ok((0, skip_ws(line)));
    }
    Ok((if neg { -v } else { v }, skip_ws(&s[i..])))
}

/// `parseHex` (`strtoull( .., 16 )`).
fn parse_hex(line: &str, line_no: usize) -> Result<(u32, &str), String> {
    if line.is_empty() {
        return Err(err("unexpected end of line", line_no));
    }
    let s = skip_ws(line);
    let s2 = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    let n = s2.bytes().take_while(|c| c.is_ascii_hexdigit()).count();
    if n == 0 {
        return Ok((0, skip_ws(line)));
    }
    let v = u64::from_str_radix(&s2[..n], 16).map_err(|_| err("invalid hexadecimal number", line_no))?;
    Ok((v as u32, skip_ws(&s2[n..])))
}

/// `parseDouble`.
fn parse_double(line: &str, line_no: usize) -> Result<(f64, &str), String> {
    if line.is_empty() {
        return Err(err("unexpected end of line", line_no));
    }
    let s = skip_ws(line);
    let n = s.bytes().take_while(|c| c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'+' | b'e' | b'E')).count();
    let v: f64 = s[..n].parse().map_err(|_| err("invalid floating point number", line_no))?;
    Ok((v, skip_ws(&s[n..])))
}

/// `parseChar`: one character followed by white space.
fn parse_char(line: &str, line_no: usize) -> Result<(char, &str), String> {
    let s = skip_ws(line);
    let mut it = s.chars();
    let Some(c) = it.next() else { return Err(err("unexpected end of line", line_no)) };
    let rest = it.as_str();
    if !rest.as_bytes().first().is_some_and(|&b| is_space(b)) {
        return Err(err("expected single character token", line_no));
    }
    Ok((c, skip_ws(rest)))
}

/// `parseUnquotedString`.
fn parse_unquoted(line: &str, line_no: usize, can_be_empty: bool) -> Result<(String, &str), String> {
    let s = skip_ws(line);
    if s.is_empty() {
        return if can_be_empty { Ok((String::new(), s)) } else { Err(err("unexpected end of line", line_no)) };
    }
    let n = s.bytes().take_while(|&c| !is_space(c)).count();
    Ok((s[..n].to_string(), skip_ws(&s[n..])))
}

/// `parseQuotedString`: the text between the quotes, `\"` and `\\` unescaped (any other backslash kept); only spaces are skipped after it.
fn parse_quoted(line: &str, line_no: usize, can_be_empty: bool) -> Result<(String, &str), String> {
    let s = skip_ws(line);
    if s.is_empty() {
        return if can_be_empty { Ok((String::new(), s)) } else { Err(err("unexpected end of line", line_no)) };
    }
    let Some(body) = s.strip_prefix('"') else { return Err(err("expecting opening quote", line_no)) };
    let mut out = String::new();
    let mut it = body.char_indices();
    let mut end = body.len();
    let mut closed = false;
    while let Some((i, c)) = it.next() {
        if c == '\\' {
            let Some((_, n)) = it.next() else { return Err(err("unexpected end of line", line_no)) };
            if n != '"' && n != '\\' {
                out.push('\\');
            }
            out.push(n);
        } else if c == '"' {
            end = i;
            closed = true;
            break;
        } else {
            out.push(c);
        }
    }
    if out.is_empty() && !can_be_empty {
        return Err(err("expected quoted string", line_no));
    }
    let rest = if closed { &body[end + 1..] } else { &body[end..] };
    Ok((out, rest.trim_start_matches(' ')))
}

// ------------------------------------------------------------------------------------------------ what a legacy screen holds

/// `TRANSFORM` (x1, y1, x2, y2), KiCad 6's sense (the file's y1 and y2 negated as `loadSymbol` reads them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform(pub i32, pub i32, pub i32, pub i32);

impl Transform {
    const IDENTITY: Transform = Transform(1, 0, 0, 1);

    fn apply(&self, p: (i64, i64)) -> (i64, i64) {
        (self.0 as i64 * p.0 + self.1 as i64 * p.1, self.2 as i64 * p.0 + self.3 as i64 * p.1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HJustify {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VJustify {
    Top,
    #[default]
    Center,
    Bottom,
}

/// A symbol's field as `loadSymbol` reads it (`SetTextPos`: in the symbol's own frame, around its position).
#[derive(Debug, Clone, Default)]
pub struct Field {
    /// 0 Reference, 1 Value, 2 Footprint, 3 Datasheet, 4.. user.
    pub legacy_id: i64,
    pub name: String,
    pub text: String,
    pub text_pos: (i64, i64),
    pub size: i64,
    pub vertical: bool,
    pub visible: bool,
    pub h: HJustify,
    pub v: VJustify,
    pub italic: bool,
    pub bold: bool,
}

/// One `SCH_SYMBOL_INSTANCE`: a sheet path (without the root sheet's identifier, which `loadSymbol` puts in front: the tree's is drawn when it is
/// written), the reference and the unit there.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub path: Vec<String>,
    pub reference: String,
    pub unit: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Symbol {
    /// The library nickname (empty before file version 4, and in a symbol never remapped).
    pub nickname: String,
    pub item: String,
    pub prefix: String,
    pub unit: i64,
    pub body_style: i64,
    pub uuid: String,
    pub pos: (i64, i64),
    pub fields: Vec<Field>,
    pub transform: Option<Transform>,
    pub instances: Vec<Instance>,
}

impl Symbol {
    /// `LIB_ID::Format`.
    pub fn lib_id(&self) -> String {
        if self.nickname.is_empty() {
            self.item.clone()
        } else {
            format!("{}:{}", self.nickname, self.item)
        }
    }

    fn field(&self, id: i64) -> Option<&Field> {
        self.fields.iter().find(|f| f.legacy_id == id && id < 4)
    }

    fn field_mut(&mut self, id: i64) -> &mut Field {
        if let Some(i) = self.fields.iter().position(|f| f.legacy_id == id) {
            return &mut self.fields[i];
        }
        let name = ["Reference", "Value", "Footprint", "Datasheet"][id as usize].to_string();
        self.fields.push(Field { legacy_id: id, name, text_pos: self.pos, visible: true, ..Default::default() });
        self.fields.last_mut().expect("pushed")
    }

    fn reference(&self) -> String {
        self.field(0).map(|f| f.text.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelShape {
    Input,
    Output,
    Bidi,
    Tristate,
    Unspecified,
}

impl LabelShape {
    fn token(self) -> &'static str {
        match self {
            LabelShape::Input => "input",
            LabelShape::Output => "output",
            LabelShape::Bidi => "bidirectional",
            LabelShape::Tristate => "tri_state",
            LabelShape::Unspecified => "passive",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SheetPin {
    pub text: String,
    pub shape: LabelShape,
    pub side: Side,
    pub pos: (i64, i64),
    pub size: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub pos: (i64, i64),
    pub size: (i64, i64),
    pub uuid: String,
    pub name: String,
    pub name_size: i64,
    pub file: String,
    pub file_size: i64,
    pub pins: Vec<SheetPin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineLayer {
    Wire,
    Bus,
    Notes,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub layer: LineLayer,
    pub start: (i64, i64),
    pub end: (i64, i64),
    pub width: i64,
    /// `solid`, `dash`, `dash_dot`, `dot`; `None` is the default.
    pub style: Option<&'static str>,
    pub color: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Notes,
    Label,
    Hier,
    Global,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub kind: TextKind,
    pub pos: (i64, i64),
    /// `SPIN_STYLE` (0 left, 1 up, 2 right, 3 bottom), after `loadText`'s swap for local labels and texts.
    pub spin: i64,
    pub size: i64,
    pub shape: LabelShape,
    pub italic: bool,
    pub bold: bool,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub pos: (i64, i64),
    pub scale: f64,
    pub png: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Symbol(Symbol),
    Sheet(Sheet),
    Bitmap(Bitmap),
    Junction((i64, i64)),
    NoConnect((i64, i64)),
    Line(Line),
    /// `SCH_BUS_WIRE_ENTRY` (`bus_bus` false) or `SCH_BUS_BUS_ENTRY`: the position and the size.
    BusEntry { bus_bus: bool, pos: (i64, i64), size: (i64, i64) },
    Text(Text),
}

/// The page: its type (`PAGE_SIZE_TYPE`'s name as KiCad spells it), and for `User` its size in mils; portrait.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub kind: String,
    pub width_mils: i64,
    pub height_mils: i64,
    pub portrait: bool,
}

/// One legacy schematic file as `loadFile` reads it.
#[derive(Debug, Clone, Default)]
pub struct Screen {
    pub version: i64,
    pub page: Option<Page>,
    /// `TITLE_BLOCK`: title, date, rev, company, comments 1..9.
    pub title: Vec<(String, String)>,
    pub items: Vec<Item>,
    pub bus_aliases: Vec<(String, Vec<String>)>,
}

/// `SCH_IO_KICAD_LEGACY::CanReadSchematicFile`: the first line says "EESchema Schematic File Version" (any case).
pub fn is_legacy_schematic(text: &str) -> bool {
    let first = text.lines().next().unwrap_or("").trim_start_matches('\u{feff}');
    str_compare("Eeschema", first).and_then(|r| str_compare("Schematic", r)).and_then(|r| str_compare("File", r)).and_then(|r| str_compare("Version", r)).is_some()
}

/// The lines of a file, each with its end of line (`LINE_READER::ReadLine`), numbered from 1.
struct Reader<'a> {
    lines: Vec<&'a str>,
    next: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Reader { lines: text.split_inclusive('\n').collect(), next: 0 }
    }

    fn read_line(&mut self) -> Option<&'a str> {
        let l = self.lines.get(self.next).copied();
        if l.is_some() {
            self.next += 1;
        }
        l
    }

    fn line_no(&self) -> usize {
        self.next
    }
}

/// `SCH_IO_KICAD_LEGACY::loadFile`: the header, the content and, from version 2 on, `$EndSCHEMATC`.
pub fn parse_screen(text: &str) -> Result<Screen, String> {
    let mut r = Reader::new(text);
    let mut screen = Screen::default();
    // loadHeader
    let first = r.read_line().unwrap_or("");
    let first = first.trim_start_matches('\u{feff}');
    let after = str_compare("Eeschema", first).and_then(|l| str_compare("Schematic", l)).and_then(|l| str_compare("File", l)).and_then(|l| str_compare("Version", l));
    let Some(after) = after else { return Err("does not appear to be an Eeschema file.".to_string()) };
    screen.version = parse_int(after, 1)?.0;
    let mut header_done = false;
    while let Some(line) = r.read_line() {
        if str_compare("EELAYER END", line.trim_start_matches(' ')).is_some() {
            header_done = true;
            break;
        }
    }
    if !header_done {
        return Err("Missing 'EELAYER END'".to_string());
    }
    // LoadContent
    let mut ended = false;
    while let Some(raw) = r.read_line() {
        let line = raw.trim_start_matches(' ');
        if line.starts_with(['\n', '\r']) || line.is_empty() {
            // `LoadContent` raises "unrecognized token" for a blank line; legacy files written by KiCad have none
            continue;
        }
        if str_compare("$Descr", line).is_some() {
            load_page_settings(&mut r, line, &mut screen)?;
        } else if str_compare("$Comp", line).is_some() {
            let sym = load_symbol(&mut r, screen.version)?;
            screen.items.push(Item::Symbol(sym));
        } else if str_compare("$Sheet", line).is_some() {
            screen.items.push(Item::Sheet(load_sheet(&mut r)?));
        } else if str_compare("$Bitmap", line).is_some() {
            if let Some(b) = load_bitmap(&mut r)? {
                screen.items.push(Item::Bitmap(b));
            }
        } else if let Some(rest) = str_compare("Connection", line) {
            screen.items.push(Item::Junction(load_point(rest, r.line_no())?));
        } else if let Some(rest) = str_compare("NoConn", line) {
            screen.items.push(Item::NoConnect(load_point(rest, r.line_no())?));
        } else if let Some(rest) = str_compare("Wire", line) {
            screen.items.push(Item::Line(load_wire(&mut r, rest)?));
        } else if let Some(rest) = str_compare("Entry", line) {
            screen.items.push(load_bus_entry(&mut r, rest)?);
        } else if let Some(rest) = str_compare("Text", line) {
            screen.items.push(Item::Text(load_text(&mut r, rest, screen.version)?));
        } else if let Some(rest) = str_compare("BusAlias", line) {
            screen.bus_aliases.push(load_bus_alias(rest, r.line_no())?);
        } else if str_compare("Kmarq", line).is_some() {
            continue; // "Ignore legacy (until 2009) ERC marker entry"
        } else if str_compare("$EndSCHEMATC", line).is_some() {
            ended = true;
            break;
        } else {
            return Err(err("unrecognized token", r.line_no()));
        }
    }
    // "Unfortunately schematic files prior to version 2 are not terminated with $EndSCHEMATC"
    if screen.version > 1 && !ended {
        return Err("'$EndSCHEMATC' not found".to_string());
    }
    Ok(screen)
}

/// `loadJunction` / `loadNoConnect`: a name (`~`) and the position.
fn load_point(rest: &str, n: usize) -> Result<(i64, i64), String> {
    let (_, rest) = parse_unquoted(rest, n, false)?;
    let (x, rest) = parse_int(rest, n)?;
    let (y, _) = parse_int(rest, n)?;
    Ok((mils(x), mils(y)))
}

/// `PAGE_INFO::SetType( wxString )`: the page type names, any case (`magic_enum::case_insensitive`), as KiCad spells them.
const PAGE_TYPES: &[&str] = &["A5", "A4", "A3", "A2", "A1", "A0", "A", "B", "C", "D", "E", "GERBER", "USLetter", "USLegal", "USLedger", "User"];

/// `loadPageSettings`.
fn load_page_settings(r: &mut Reader, line: &str, screen: &mut Screen) -> Result<(), String> {
    let n = r.line_no();
    let rest = str_compare("$Descr", line).unwrap_or("");
    let (kind, rest) = parse_unquoted(rest, n, false)?;
    let Some(kind) = PAGE_TYPES.iter().find(|t| t.eq_ignore_ascii_case(&kind)) else { return Err(err("invalid page size", n)) };
    let (w, rest) = parse_int(rest, n)?;
    let (h, rest) = parse_int(rest, n)?;
    let mut page = Page { kind: kind.to_string(), ..Default::default() };
    if page.kind == "User" {
        page.width_mils = w;
        page.height_mils = h;
    } else {
        let (orientation, _) = parse_unquoted(rest, n, true)?;
        page.portrait = orientation == "portrait";
    }
    screen.page = Some(page);
    let keys: [(&str, &str); 13] = [
        ("Title", "title"),
        ("Date", "date"),
        ("Rev", "rev"),
        ("Comp", "company"),
        ("Comment1", "1"),
        ("Comment2", "2"),
        ("Comment3", "3"),
        ("Comment4", "4"),
        ("Comment5", "5"),
        ("Comment6", "6"),
        ("Comment7", "7"),
        ("Comment8", "8"),
        ("Comment9", "9"),
    ];
    loop {
        let Some(line) = r.read_line() else { return Err(err("unexpected end of file", r.line_no())) };
        let n = r.line_no();
        if str_compare("Sheet", line).is_some() {
            continue; // the virtual page number and count, which the s-expression file does not keep
        }
        if str_compare("$EndDescr", line).is_some() {
            return Ok(());
        }
        for (tok, key) in keys {
            if let Some(rest) = str_compare(tok, line) {
                let (text, _) = parse_quoted(rest, n, true)?;
                screen.title.push((key.to_string(), text));
                break;
            }
        }
    }
}

/// `loadSheet`.
fn load_sheet(r: &mut Reader) -> Result<Sheet, String> {
    let mut sheet = Sheet { name_size: mils(60), file_size: mils(60), ..Default::default() };
    while let Some(line) = r.read_line() {
        let n = r.line_no();
        if let Some(rest) = str_compare("S", line) {
            let (x, rest) = parse_int(rest, n)?;
            let (y, rest) = parse_int(rest, n)?;
            let (w, rest) = parse_int(rest, n)?;
            let (h, _) = parse_int(rest, n)?;
            sheet.pos = (mils(x), mils(y));
            sheet.size = (mils(w), mils(h));
        } else if let Some(rest) = str_compare("U", line) {
            let (text, _) = parse_unquoted(rest, n, false)?;
            if text != "00000000" {
                sheet.uuid = kiid_of(&text);
            }
        } else if let Some(rest) = line.strip_prefix('F') {
            let (id, rest) = parse_int(rest, n)?;
            if id == 0 || id == 1 {
                let (text, rest) = parse_quoted(rest, n, false)?;
                let (size, _) = parse_int(rest, n)?;
                if id == 0 {
                    (sheet.name, sheet.name_size) = (text, mils(size));
                } else {
                    (sheet.file, sheet.file_size) = (text, mils(size));
                }
            } else {
                let (text, rest) = parse_quoted(rest, n, true)?;
                let text = crate::overbar::convert_to_new_overbar_notation(&text);
                let (shape, rest) = parse_char(rest, n)?;
                let shape = match shape {
                    'I' => LabelShape::Input,
                    'O' => LabelShape::Output,
                    'B' => LabelShape::Bidi,
                    'T' => LabelShape::Tristate,
                    'U' => LabelShape::Unspecified,
                    _ => return Err(err("invalid sheet pin type", n)),
                };
                let (side, rest) = parse_char(rest, n)?;
                let side = match side {
                    'R' => Side::Right,
                    'T' => Side::Top,
                    'B' => Side::Bottom,
                    'L' => Side::Left,
                    _ => return Err(err("invalid sheet pin side", n)),
                };
                let (x, rest) = parse_int(rest, n)?;
                let (y, rest) = parse_int(rest, n)?;
                let (size, _) = parse_int(rest, n)?;
                sheet.pins.push(SheetPin { text, shape, side, pos: (mils(x), mils(y)), size: mils(size) });
            }
        } else if str_compare("$EndSheet", line).is_some() {
            // `SCH_SHEET_PIN::SetPosition`, which the pin's position was set by: `ConstrainOnEdge( pos, true )`, the side its nearest edge
            let (pos, size) = (sheet.pos, sheet.size);
            for pin in sheet.pins.iter_mut() {
                constrain_on_edge(pin, pos, size);
            }
            return Ok(sheet);
        }
    }
    Err("missing '$EndSheet`".to_string())
}

/// `SCH_SHEET_PIN::ConstrainOnEdge( aPos, true )`: the side is the edge nearest the position (`SHAPE_LINE_CHAIN::NearestSegment`: top, right, bottom,
/// left, the first of equal distances), and the pin is put on it, clamped to the sheet.
fn constrain_on_edge(pin: &mut SheetPin, pos: (i64, i64), size: (i64, i64)) {
    let (l, r, t, b) = (pos.0, pos.0 + size.0, pos.1, pos.1 + size.1);
    let p = pin.pos;
    let segs = [((l, t), (r, t), Side::Top), ((r, t), (r, b), Side::Right), ((r, b), (l, b), Side::Bottom), ((l, b), (l, t), Side::Left)];
    let mut best = i64::MAX;
    for (a, c, side) in segs {
        let d = seg_distance(a, c, p);
        if d < best {
            best = d;
            pin.side = side;
        }
    }
    let mut at = p;
    match pin.side {
        Side::Left => at.0 = l,
        Side::Right => at.0 = r,
        Side::Top => at.1 = t,
        Side::Bottom => at.1 = b,
    }
    match pin.side {
        Side::Left | Side::Right => at.1 = at.1.clamp(t.min(b), b.max(t)),
        Side::Top | Side::Bottom => at.0 = at.0.clamp(l.min(r), r.max(l)),
    }
    pin.pos = at;
}

/// `SEG::Distance( VECTOR2I )`: rounded to the nearest unit.
fn seg_distance(a: (i64, i64), b: (i64, i64), p: (i64, i64)) -> i64 {
    let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) as f64 * dx + (p.1 - a.1) as f64 * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 as f64 + t * dx, a.1 as f64 + t * dy);
    ((p.0 as f64 - cx).hypot(p.1 as f64 - cy)).round() as i64
}

/// `loadBitmap`: the position, the scale and the PNG (hexadecimal bytes); the scale made the PNG's own (`* PPI / 300`, "Legacy file formats assumed
/// 300 image PPI at load"). `None` for an image with no data.
fn load_bitmap(r: &mut Reader) -> Result<Option<Bitmap>, String> {
    let mut bitmap = Bitmap { pos: (0, 0), scale: 1.0, png: Vec::new() };
    while let Some(line) = r.read_line() {
        let n = r.line_no();
        if let Some(rest) = str_compare("Pos", line) {
            let (x, rest) = parse_int(rest, n)?;
            let (y, _) = parse_int(rest, n)?;
            bitmap.pos = (mils(x), mils(y));
        } else if let Some(rest) = str_compare("Scale", line) {
            let (s, _) = parse_double(rest, n)?;
            // "Prevent scalefactor values that cannot be displayed."
            bitmap.scale = if s.is_normal() { s } else { 1.0 };
        } else if str_compare("Data", line).is_some() {
            loop {
                let Some(line) = r.read_line() else { return Err(err("Unexpected end of file", r.line_no())) };
                if str_compare("EndData", line).is_some() {
                    bitmap.scale *= png_ppi(&bitmap.png) / 300.0;
                    break;
                }
                // "each byte = 2 hexadecimal digits and a space between 2 bytes"; a stray "$EndBitmap" of old files is discarded
                let mut s = line;
                while !s.is_empty() && !is_space(s.as_bytes()[0]) && !s.starts_with('$') {
                    let digits: String = s.chars().take(2).collect();
                    let v = u8::from_str_radix(digits.trim(), 16).map_err(|_| "invalid PNG data".to_string())?;
                    bitmap.png.push(v);
                    s = s.get(3..).unwrap_or("");
                }
            }
        } else if str_compare("$EndBitmap", line).is_some() {
            return Ok((!bitmap.png.is_empty()).then_some(bitmap));
        }
    }
    Err("unexpected end of file".to_string())
}

/// `BITMAP_BASE::GetPPI` of a PNG: its `pHYs` resolution in dots per inch, else 300.
fn png_ppi(png: &[u8]) -> f64 {
    let mut i = 8;
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        let kind = &png[i + 4..i + 8];
        if kind == b"pHYs" && i + 8 + 9 <= png.len() && png[i + 16] == 1 {
            let ppm = u32::from_be_bytes([png[i + 8], png[i + 9], png[i + 10], png[i + 11]]) as f64;
            if ppm > 0.0 {
                return (ppm * 0.0254).round();
            }
        }
        if kind == b"IDAT" || kind == b"IEND" {
            break;
        }
        i += 12 + len;
    }
    300.0
}

/// `loadWire`.
fn load_wire(r: &mut Reader, rest: &str) -> Result<Line, String> {
    let n = r.line_no();
    let (layer, rest) = if let Some(x) = str_compare("Wire", rest) {
        (LineLayer::Wire, x)
    } else if let Some(x) = str_compare("Bus", rest) {
        (LineLayer::Bus, x)
    } else if let Some(x) = str_compare("Notes", rest) {
        (LineLayer::Notes, x)
    } else {
        return Err(err("invalid line type", n));
    };
    let Some(mut rest) = str_compare("Line", rest) else { return Err(err("invalid wire definition", n)) };
    // "The default graphical line style was Dashed."
    let mut line = Line { layer, start: (0, 0), end: (0, 0), width: 0, style: (layer == LineLayer::Notes).then_some("dash"), color: None };
    while !is_eol(rest) {
        let (buf, r2) = parse_unquoted(rest, n, false)?;
        rest = r2;
        if buf == ")" {
            continue;
        } else if buf == "width" {
            let (w, r2) = parse_int(rest, n)?;
            rest = r2;
            line.width = mils(w);
        } else if buf == "style" {
            let (s, r2) = parse_unquoted(rest, n, false)?;
            rest = r2;
            line.style = match s.as_str() {
                "solid" => Some("solid"),
                "dashed" => Some("dash"),
                "dash_dot" => Some("dash_dot"),
                "dotted" => Some("dot"),
                _ => line.style,
            };
        } else if let Some((keyword, prm)) = buf.rsplit_once('(') {
            if keyword == "rgb" || keyword == "rgba" {
                let mut color = [0i64, 0, 0, 255];
                let mut ii = 0;
                if !prm.is_empty() {
                    color[0] = prm.trim_end_matches(',').parse().unwrap_or(0);
                    ii = 1;
                }
                let count = if keyword == "rgba" { 4 } else { 3 };
                while ii < count && !is_eol(rest) {
                    let (v, r2) = parse_int(rest, n)?;
                    color[ii] = v;
                    rest = r2.strip_prefix([',', ' ']).unwrap_or(r2);
                    ii += 1;
                }
                line.color = Some([color[0] as f64 / 255.0, color[1] as f64 / 255.0, color[2] as f64 / 255.0, color[3] as f64 / 255.0]);
            }
        }
    }
    let Some(coords) = r.read_line() else { return Err(err("unexpected end of file", r.line_no())) };
    let n = r.line_no();
    let (x1, c) = parse_int(coords, n)?;
    let (y1, c) = parse_int(c, n)?;
    let (x2, c) = parse_int(c, n)?;
    let (y2, _) = parse_int(c, n)?;
    line.start = (mils(x1), mils(y1));
    line.end = (mils(x2), mils(y2));
    Ok(line)
}

/// `loadBusEntry`.
fn load_bus_entry(r: &mut Reader, rest: &str) -> Result<Item, String> {
    let n = r.line_no();
    let bus_bus = if let Some(x) = str_compare("Wire", rest) {
        if str_compare("Line", x).is_none() {
            return Err(err("invalid bus entry definition expected 'Line'", n));
        }
        false
    } else if let Some(x) = str_compare("Bus", rest) {
        if str_compare("Bus", x).is_none() {
            return Err(err("invalid bus entry definition expected 'Bus'", n));
        }
        true
    } else {
        return Err(err("invalid bus entry type", n));
    };
    let Some(coords) = r.read_line() else { return Err(err("unexpected end of file", r.line_no())) };
    let n = r.line_no();
    let (x1, c) = parse_int(coords, n)?;
    let (y1, c) = parse_int(c, n)?;
    let (x2, c) = parse_int(c, n)?;
    let (y2, _) = parse_int(c, n)?;
    Ok(Item::BusEntry { bus_bus, pos: (mils(x1), mils(y1)), size: (mils(x2) - mils(x1), mils(y2) - mils(y1)) })
}

/// `loadText`.
fn load_text(r: &mut Reader, rest: &str, version: i64) -> Result<Text, String> {
    let n = r.line_no();
    let (kind, rest) = if let Some(x) = str_compare("Notes", rest) {
        (TextKind::Notes, x)
    } else if let Some(x) = str_compare("Label", rest) {
        (TextKind::Label, x)
    } else if let Some(x) = str_compare("HLabel", rest) {
        (TextKind::Hier, x)
    } else if let Some(x) = str_compare("GLabel", rest) {
        // "Prior to version 2, the SCH_GLOBALLABEL object did not exist."
        (if version == 1 { TextKind::Hier } else { TextKind::Global }, x)
    } else {
        return Err(err("unknown Text type", n));
    };
    let (x, rest) = parse_int(rest, n)?;
    let (y, rest) = parse_int(rest, n)?;
    let (mut spin, rest) = parse_int(rest, n)?;
    // "we store the orientation of hierarchical and global labels using a different int encoding than that for local labels"
    if kind != TextKind::Global && kind != TextKind::Hier {
        spin = match spin {
            0 => 2,
            2 => 0,
            s => s,
        };
    }
    let (size, mut rest) = parse_int(rest, n)?;
    let size = mils(size);
    let mut shape = LabelShape::Input;
    if kind == TextKind::Hier || kind == TextKind::Global {
        let names = [("Input", LabelShape::Input), ("Output", LabelShape::Output), ("BiDi", LabelShape::Bidi), ("3State", LabelShape::Tristate), ("UnSpc", LabelShape::Unspecified)];
        let Some((s, r2)) = names.iter().find_map(|(name, s)| str_compare(name, rest).map(|r2| (*s, r2))) else { return Err(err("invalid label type", n)) };
        shape = s;
        rest = r2;
    }
    let (mut italic, mut pen) = (false, 0);
    // "The following tokens do not exist in version 1 schematic files, and not always in version 2 for HLabels and GLabels"
    if version > 1 {
        if version > 2 || rest.as_bytes().first().is_some_and(|&c| c >= b' ') {
            if let Some(r2) = str_compare("Italic", rest) {
                italic = true;
                rest = r2;
            } else if let Some(r2) = str_compare("~", rest) {
                rest = r2;
            } else {
                return Err(err("expected 'Italics' or '~'", n));
            }
        }
        if rest.as_bytes().first().is_some_and(|&c| c >= b' ') {
            pen = parse_int(rest, n)?.0;
        }
    }
    let Some(body) = r.read_line() else { return Err(err("unexpected end of file", r.line_no())) };
    // `strtok( tmp, "\r\n" )`: the text up to the first end of line; "\n" written as two characters is a new line
    let body = body.split(['\r', '\n']).next().unwrap_or("");
    let text = crate::overbar::convert_to_new_overbar_notation(&body.replace("\\n", "\n"));
    Ok(Text { kind, pos: (mils(x), mils(y)), spin, size, shape, italic, bold: pen != 0, text })
}

/// `loadSymbol`.
fn load_symbol(r: &mut Reader, version: i64) -> Result<Symbol, String> {
    let mut sym = Symbol { unit: 1, body_style: 1, ..Default::default() };
    while let Some(line) = r.read_line() {
        let n = r.line_no();
        if let Some(rest) = str_compare("L", line) {
            let tokens: Vec<&str> = rest.split([' ', '\t', '\r', '\n']).filter(|t| !t.is_empty()).collect();
            if tokens.len() < 2 {
                return Err(err("invalid symbol library definition", n));
            }
            let lib_name = tokens[0].replace('~', " ");
            // "Prior to schematic version 4, library IDs did not have a library nickname"
            if version > 3 {
                match lib_name.split_once(':') {
                    Some((nick, item)) => {
                        sym.nickname = nick.to_string();
                        sym.item = fix_illegal_chars(item);
                    }
                    None => sym.item = fix_illegal_chars(&lib_name),
                }
            } else {
                sym.item = lib_name;
            }
            let reference = tokens[1].replace('~', " ");
            let mut prefix = reference.as_str();
            while let Some(last) = prefix.chars().last() {
                if !(last.is_ascii_digit() || last == '?') {
                    break;
                }
                prefix = &prefix[..prefix.len() - 1];
            }
            let prefix = prefix.trim();
            sym.prefix = if prefix.is_empty() { "U".to_string() } else { prefix.to_string() };
        } else if let Some(rest) = str_compare("U", line) {
            // "This fixes a potentially buggy files caused by unit being set to zero"
            let (unit, rest) = parse_int(rest, n)?;
            sym.unit = if unit == 0 { 1 } else { unit };
            let (body, rest) = parse_int(rest, n)?;
            sym.body_style = if body == 0 { 1 } else { body };
            let (text, _) = parse_unquoted(rest, n, false)?;
            if text != "00000000" {
                sym.uuid = kiid_of(&text);
            }
        } else if let Some(rest) = str_compare("P", line) {
            let (x, rest) = parse_int(rest, n)?;
            let (y, _) = parse_int(rest, n)?;
            sym.pos = (mils(x), mils(y));
        } else if let Some(rest) = str_compare("AR", line) {
            let attr = |rest: &'_ str, key: &str| -> Result<(), String> {
                if rest.len() < key.len() || !rest[..key.len()].eq_ignore_ascii_case(key) {
                    return Err(err(&format!("missing '{key}' token"), n));
                }
                Ok(())
            };
            attr(rest, "Path=")?;
            let (path, rest) = parse_quoted(&rest[5..], n, false)?;
            // "AR path excludes root sheet, but includes symbol.  Drop the symbol ID"; the root's identifier is put in front when the tree is written
            let mut ids: Vec<String> = path.split('/').filter(|c| !c.is_empty()).map(kiid_of).collect();
            ids.pop();
            attr(rest, "Ref=")?;
            let (reference, rest) = parse_quoted(&rest[4..], n, false)?;
            attr(rest, "Part=")?;
            let (unit, _) = parse_quoted(&rest[5..], n, false)?;
            let unit: i64 = unit.trim().parse().map_err(|_| err("expected integer value", n))?;
            if !(0..=MAX_UNIT_COUNT_PER_PACKAGE).contains(&unit) {
                return Err(err("unit value out of range", n));
            }
            // `AddHierarchicalReference`: one per path, a later one replacing an earlier
            sym.instances.retain(|i| i.path != ids);
            sym.instances.push(Instance { path: ids, reference: reference.clone(), unit });
            sym.field_mut(0).text = reference;
        } else if let Some(rest) = str_compare("F", line) {
            let (id, rest) = parse_int(rest, n)?;
            let (text, rest) = parse_quoted(rest, n, true)?;
            let (orient, rest) = parse_char(rest, n)?;
            let (x, rest) = parse_int(rest, n)?;
            let (y, rest) = parse_int(rest, n)?;
            // "Y got inverted in symbol coordinates"
            let pos = (mils(x), -(mils(y) - sym.pos.1) + sym.pos.1);
            let (size, rest) = parse_int(rest, n)?;
            let (attributes, mut rest) = parse_hex(rest, n)?;
            let mut f = Field { legacy_id: id, text_pos: pos, size: mils(size), visible: attributes == 0, ..Default::default() };
            let mut name = String::new();
            if version > 1 {
                let (h, r2) = parse_char(rest, n)?;
                let (attrs, r2) = parse_unquoted(r2, n, false)?;
                let (nm, _) = parse_quoted(r2, n, true)?;
                rest = "";
                name = nm;
                f.h = match h {
                    'L' => HJustify::Left,
                    'R' => HJustify::Right,
                    'C' => HJustify::Center,
                    _ => return Err(err("symbol field text horizontal justification must be L, R, or C", n)),
                };
                let a: Vec<char> = attrs.chars().collect();
                f.v = match a.first() {
                    Some('T') => VJustify::Top,
                    Some('B') => VJustify::Bottom,
                    Some('C') => VJustify::Center,
                    _ => return Err(err("symbol field text vertical justification must be B, T, or C", n)),
                };
                if a.len() > 1 {
                    if a.len() != 3 {
                        return Err(err("symbol field text attributes must be 3 characters wide", n));
                    }
                    match a[1] {
                        'I' => f.italic = true,
                        'N' => {}
                        _ => return Err(err("symbol field text italics indicator must be I or N", n)),
                    }
                    match a[2] {
                        'B' => f.bold = true,
                        'N' => {}
                        _ => return Err(err("symbol field text bold indicator must be B or N", n)),
                    }
                }
            }
            let _ = rest;
            f.text = text;
            f.vertical = match orient {
                'H' => false,
                'V' => true,
                _ => return Err(err("symbol field orientation must be H or V", n)),
            };
            if name.is_empty() {
                name = if (0..4).contains(&id) { ["Reference", "Value", "Footprint", "Datasheet"][id as usize].to_string() } else { format!("Field{id}") };
            }
            f.name = name;
            if (0..4).contains(&id) {
                // a mandatory field is the symbol's own; its name stays the canonical one (`saveField`: `GetCanonicalName`)
                f.name = ["Reference", "Value", "Footprint", "Datasheet"][id as usize].to_string();
                *sym.field_mut(id) = f;
            } else {
                sym.fields.push(f);
            }
        } else if str_compare("$EndComp", line).is_some() {
            return Ok(sym);
        } else {
            // "two lines that begin with a tab or spaces": the redundant position, then the transform
            let (_, rest) = parse_int(line, n)?;
            let (_, rest) = parse_int(rest, n)?;
            let _ = parse_int(rest, n)?;
            let Some(tline) = r.read_line() else { return Err(err("invalid symbol line", r.line_no())) };
            let n = r.line_no();
            let (x1, t) = parse_int(tline, n)?;
            let (y1, t) = parse_int(t, n)?;
            let (x2, t) = parse_int(t, n)?;
            let (y2, _) = parse_int(t, n)?;
            for (v, what) in [(x1, "X1"), (y1, "Y1"), (x2, "X2"), (y2, "Y2")] {
                if !(-1..=1).contains(&v) {
                    return Err(err(&format!("invalid symbol {what} transform value"), n));
                }
            }
            sym.transform = Some(Transform(x1 as i32, -y1 as i32, x2 as i32, -y2 as i32));
        }
    }
    Err("invalid symbol line".to_string())
}

/// `loadBusAlias`.
fn load_bus_alias(rest: &str, n: usize) -> Result<(String, Vec<String>), String> {
    let (name, mut rest) = parse_unquoted(rest, n, false)?;
    let mut members = Vec::new();
    while !rest.is_empty() {
        let (m, r2) = parse_unquoted(rest, n, true)?;
        if m.is_empty() {
            break;
        }
        members.push(m);
        rest = r2;
    }
    Ok((name, members))
}

/// `LIB_ID::FixIllegalChars( .., false )`: `:`, tab, new lines, `\`, `<`, `>` and `"` made `_`.
fn fix_illegal_chars(name: &str) -> String {
    name.chars().map(|c| if matches!(c, ':' | '\t' | '\n' | '\r' | '\\' | '<' | '>' | '"') { '_' } else { c }).collect()
}

/// `KIID( wxString )` of a legacy time stamp (eight hexadecimal digits): `00000000-0000-0000-0000-0000XXXXXXXX`, in lower case. Anything else is kept.
fn kiid_of(text: &str) -> String {
    let t = text.trim();
    if t.len() == 8 && t.bytes().all(|c| c.is_ascii_hexdigit()) {
        return format!("00000000-0000-0000-0000-0000{}", t.to_ascii_lowercase());
    }
    t.to_ascii_lowercase()
}

// ------------------------------------------------------------------------------------------------ the hierarchy (loadHierarchy)

/// The legacy schematic of a root file and the sheets it places, as `LoadSchematicFile` loads it.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    /// The screens by their absolute file (`.sch`), the root's first.
    pub screens: Vec<(PathBuf, Screen)>,
    /// The sub-sheets that could not be read (`m_error`: "the entire schematic could not be loaded").
    pub errors: Vec<String>,
}

/// `loadHierarchy`: the root file `root`, then each sheet's file (its name with the extension made `sch`, relative to the folder of the file that places
/// it), a file reached twice being one screen. `read` reads a file (the disk, or a set of files that arrived as text).
pub fn load_tree(root: &Path, read: &dyn Fn(&Path) -> Result<String, String>) -> Result<Tree, String> {
    let mut tree = Tree::default();
    let text = read(root).map_err(|e| format!("Error loading schematic '{}'.\n{e}", root.display()))?;
    let screen = parse_screen(&text).map_err(|e| format!("Error loading schematic '{}'.\n{e}", root.display()))?;
    tree.screens.push((normal(root), screen));
    let mut i = 0;
    while i < tree.screens.len() {
        let dir = tree.screens[i].0.parent().map(Path::to_path_buf).unwrap_or_default();
        let files: Vec<String> = tree.screens[i].1.items.iter().filter_map(|it| if let Item::Sheet(s) = it { Some(s.file.clone()) } else { None }).collect();
        for f in files {
            let path = normal(&dir.join(Path::new(&f.replace('\\', "/")).with_extension("sch")));
            if tree.screens.iter().any(|(p, _)| *p == path) {
                continue;
            }
            match read(&path).and_then(|t| parse_screen(&t)) {
                Ok(s) => tree.screens.push((path, s)),
                Err(e) => tree.errors.push(format!("{}: {e}", path.display())),
            }
        }
        i += 1;
    }
    Ok(tree)
}

/// A path with `.` and `..` worked out.
fn normal(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ the library symbols

/// The library symbol of a placed symbol, from its library identifier (`LIB_ID::Format`): KiCad's `UpdateSymbolLinks` asks the symbol library table
/// first, then the project's cache library under `Lib_Name`. The answer is the `(symbol ..)` text of a library in KiCad's own format (flattened here)
/// and where it came from, said as the load reporter says it.
pub type LibraryLookup<'a> = &'a dyn Fn(&str) -> Option<LookedUp>;

#[derive(Debug, Clone)]
pub struct LookedUp {
    /// The text of the whole `.kicad_sym` the symbol is in (a derived symbol's parent is found in it).
    pub library_text: std::sync::Arc<String>,
    /// The symbol's name in that library.
    pub name: String,
    /// It came from the cache library (`"Falling back to cache to set symbol '%s:%s' link '%s'."`).
    pub from_cache: bool,
}

/// The symbol `name` of a `.kicad_sym` text, flattened (`LIB_SYMBOL::Flatten`: the root of its `extends` chain, each derived symbol's mandatory fields
/// that are not empty over it, its other fields replacing those of the same name or added, its keywords and footprint filters when it has some) and
/// named `lib_name` (`SCH_SCREEN::UpdateSymbolLinks`: `m_libSymbols.insert( { GetSchSymbolLibraryName(), .. } )`). `None` when the library has no such symbol.
pub fn flattened_symbol(library_text: &str, name: &str, lib_name: &str) -> Option<Sexpr> {
    let tree = sexpr::parse(library_text).ok()?;
    let root = tree.as_list()?;
    let by_name: BTreeMap<&str, &[Sexpr]> = sexpr::find_all(root, "symbol").filter_map(|s| Some((sexpr::txt(s, 1)?, s))).collect();
    let mut chain: Vec<&[Sexpr]> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut cur = *by_name.get(name)?;
    loop {
        chain.push(cur);
        let Some(parent) = sexpr::find(cur, "extends").and_then(|e| sexpr::txt(e, 1)) else { break };
        if !seen.insert(parent) {
            break;
        }
        match by_name.get(parent) {
            Some(p) => cur = p,
            None => break,
        }
    }
    let mut out: Vec<Sexpr> = chain.last()?.to_vec();
    const MANDATORY: [&str; 5] = ["Reference", "Value", "Footprint", "Datasheet", "Description"];
    let prop_name = |n: &Sexpr| -> Option<String> { n.as_list().filter(|l| sexpr::tag(l) == Some("property")).and_then(|l| sexpr::txt(l, 1)).map(str::to_string) };
    let prop_text = |n: &Sexpr| -> String { n.as_list().and_then(|l| sexpr::txt(l, 2)).unwrap_or("").to_string() };
    for derived in chain.iter().rev().skip(1) {
        for item in derived.iter() {
            let Some(pname) = prop_name(item) else { continue };
            let text = prop_text(item);
            let keep_empty = !MANDATORY.contains(&pname.as_str()) && pname != "ki_keywords" && pname != "ki_fp_filters";
            if text.is_empty() && !keep_empty {
                continue;
            }
            match out.iter().position(|n| prop_name(n).as_deref() == Some(pname.as_str())) {
                Some(i) => out[i] = item.clone(),
                None => {
                    let at = out.iter().rposition(|n| prop_name(n).is_some()).map(|i| i + 1).unwrap_or(2);
                    out.insert(at, item.clone());
                }
            }
        }
    }
    out.retain(|n| !n.as_list().is_some_and(|l| sexpr::tag(l) == Some("extends")));
    if out.len() > 1 {
        out[1] = sexpr::quoted_atom(lib_name);
    }
    Some(Sexpr::List(out))
}

/// Whether a flattened library symbol is a global power symbol, and its first pin (is a power input, is hidden, its name): `FixLegacyPowerSymbolMismatches`.
fn first_pin_and_power(sym: &[Sexpr]) -> (bool, Option<(bool, bool, String)>) {
    let global_power = sexpr::find(sym, "power").is_some_and(|p| sexpr::txt(p, 1).is_none_or(|k| k == "global"));
    // `GetAllLibPins()[0]`: the first pin of the drawing, in the order the units are written
    let mut first: Option<(bool, bool, String)> = None;
    'units: for unit in sexpr::find_all(sym, "symbol") {
        for pin in sexpr::find_all(unit, "pin") {
            let power_in = sexpr::txt(pin, 1) == Some("power_in");
            let hidden = pin.iter().any(|a| a.text() == Some("hide")) || sexpr::find(pin, "hide").is_some_and(|h| sexpr::txt(h, 1) != Some("no"));
            let name = sexpr::find(pin, "name").and_then(|n| sexpr::txt(n, 1)).unwrap_or("").to_string();
            first = Some((power_in, hidden, name));
            break 'units;
        }
    }
    (global_power, first)
}

// ------------------------------------------------------------------------------------------------ writing the `.kicad_sch` files

/// What [`write_tree`] made.
#[derive(Debug, Clone, Default)]
pub struct Written {
    /// The root `.kicad_sch` written.
    pub root: PathBuf,
    /// Every file written, the root's first.
    pub files: Vec<PathBuf>,
    /// What the load reporter says (`UpdateSymbolLinks`'s messages, the sub-sheets that could not be read).
    pub messages: Vec<String>,
    /// `SCH_SCREENS::HasNoFullyDefinedLibIds`: no symbol names a library (a project that wants Remap Symbols).
    pub no_fully_defined_lib_ids: bool,
    /// Symbols with no library symbol.
    pub unlinked: usize,
}

/// A text quoted as `OUTPUTFORMATTER::Quotew` quotes it.
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `EDA_UNIT_UTILS::FormatInternalUnits( schIUScale, .. )`: millimetres, no trailing zeros.
fn mm(iu: i64) -> String {
    let s = format!("{:.4}", iu as f64 / 10_000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn xy(p: (i64, i64)) -> String {
    format!("{} {}", mm(p.0), mm(p.1))
}

fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `EDA_TEXT::Format`'s `(effects ..)`.
fn effects(size: i64, bold: bool, italic: bool, h: HJustify, v: VJustify, hide: bool) -> String {
    let mut font = format!("(font (size {} {})", mm(size), mm(size));
    if bold {
        font.push_str(&format!(" (thickness {}) (bold yes)", mm((size as f64 / 5.0).round() as i64)));
    }
    if italic {
        font.push_str(" (italic yes)");
    }
    font.push(')');
    let mut justify: Vec<&str> = Vec::new();
    match h {
        HJustify::Left => justify.push("left"),
        HJustify::Right => justify.push("right"),
        HJustify::Center => {}
    }
    match v {
        VJustify::Top => justify.push("top"),
        VJustify::Bottom => justify.push("bottom"),
        VJustify::Center => {}
    }
    let mut out = format!("(effects {font}");
    if !justify.is_empty() {
        out.push_str(&format!(" (justify {})", justify.join(" ")));
    }
    if hide {
        out.push_str(" (hide yes)");
    }
    out.push(')');
    out
}

/// The identifiers KiCad draws at random, drawn from the files' paths instead (the module doc's **Differs**), as version 4 UUIDs.
struct Ids {
    seed: blake3::Hash,
    n: u64,
}

impl Ids {
    fn next(&mut self) -> String {
        self.n += 1;
        let mut h = blake3::Hasher::new();
        h.update(self.seed.as_bytes());
        h.update(&self.n.to_le_bytes());
        let b = h.finalize();
        let mut b: [u8; 16] = b.as_bytes()[..16].try_into().expect("16 bytes");
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
    }
}

/// `SCH_SYMBOL::GetOrientation`: the transform matched against `SetOrientation` of each of KiCad's twelve choices in turn, on one copy of the symbol
/// (so the incremental `SYM_MIRROR_Y` applies to what the previous choice left). Returns (angle, mirror x, mirror y).
fn orientation(t: Transform) -> (i32, bool, bool) {
    // SYM_ORIENT_0 .. SYM_ORIENT_270 as (rotations counterclockwise from 0) and the mirrors
    #[derive(Clone, Copy)]
    enum Step {
        Orient(i32),
        MirrorX,
        MirrorY,
    }
    fn apply(m: &mut Transform, step: Step) {
        let temp = match step {
            Step::Orient(_) => unreachable!(),
            Step::MirrorX => Transform(1, 0, 0, -1),
            Step::MirrorY => Transform(-1, 0, 0, 1),
        };
        incremental(m, temp);
    }
    fn incremental(m: &mut Transform, temp: Transform) {
        let n = Transform(m.0 * temp.0 + m.2 * temp.1, m.1 * temp.0 + m.3 * temp.1, m.0 * temp.2 + m.2 * temp.3, m.1 * temp.2 + m.3 * temp.3);
        *m = n;
    }
    fn orient(m: &mut Transform, quarter: i32) {
        *m = Transform::IDENTITY;
        match quarter {
            1 => incremental(m, Transform(0, 1, -1, 0)),
            2 => {
                incremental(m, Transform(0, 1, -1, 0));
                incremental(m, Transform(0, 1, -1, 0));
            }
            3 => incremental(m, Transform(0, -1, 1, 0)),
            _ => {}
        }
    }
    // (orientation steps, angle, mirror x, mirror y) of `rotate_values`
    let choices: [(&[Step], i32, bool, bool); 12] = [
        (&[Step::Orient(0)], 0, false, false),
        (&[Step::Orient(1)], 90, false, false),
        (&[Step::Orient(2)], 180, false, false),
        (&[Step::Orient(3)], 270, false, false),
        (&[Step::Orient(0), Step::MirrorX], 0, true, false),
        (&[Step::Orient(1), Step::MirrorX], 90, true, false),
        (&[Step::Orient(3), Step::MirrorX], 270, true, false),
        (&[Step::MirrorY], 0, false, true),
        (&[Step::Orient(0), Step::MirrorY], 0, false, true),
        (&[Step::Orient(1), Step::MirrorY], 90, false, true),
        (&[Step::Orient(2), Step::MirrorY], 180, false, true),
        (&[Step::Orient(3), Step::MirrorY], 270, false, true),
    ];
    let mut temp = t;
    for (steps, angle, mx, my) in choices {
        for s in steps {
            match *s {
                Step::Orient(q) => orient(&mut temp, q),
                other => apply(&mut temp, other),
            }
        }
        if temp == t {
            return (angle, mx, my);
        }
    }
    (0, false, false)
}

/// Write the tree as `.kicad_sch` files under `out_dir` (each screen at its path from the root's folder, the extension `.kicad_sch`), the symbols
/// linked through `lookup`. `project` is the project's name (the root file's), which the symbol instances are filed under.
pub fn write_tree(tree: &Tree, out_dir: &Path, project: &str, lookup: LibraryLookup<'_>) -> Result<Written, String> {
    let Some((root_path, _)) = tree.screens.first() else { return Err("no schematic".into()) };
    let root_dir = root_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut seed = blake3::Hasher::new();
    for (p, _) in &tree.screens {
        seed.update(p.to_string_lossy().as_bytes());
    }
    let mut ids = Ids { seed: seed.finalize(), n: 0 };
    let mut written = Written { messages: tree.errors.iter().map(|e| format!("The entire schematic could not be loaded.  Errors occurred attempting to load hierarchical sheets. {e}")).collect(), ..Default::default() };

    // the screens, their items given identifiers where the file has none, and the duplicates replaced (`ReplaceDuplicateTimeStamps`, over every screen)
    let mut screens: Vec<(PathBuf, Screen)> = tree.screens.clone();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (_, s) in screens.iter_mut() {
        for it in s.items.iter_mut() {
            let id = match it {
                Item::Symbol(sym) => &mut sym.uuid,
                Item::Sheet(sh) => &mut sh.uuid,
                _ => continue,
            };
            if id.is_empty() || !seen.insert(id.clone()) {
                *id = ids.next();
                seen.insert(id.clone());
            }
        }
    }
    // the root sheet's identifier is its screen's (`loadHierarchy`: "if( aSheet == m_rootSheet ) m_Uuid = GetScreen()->GetUuid()")
    let root_uuid = ids.next();
    let key_of = |p: &Path| -> PathBuf { p.strip_prefix(&root_dir).map(Path::to_path_buf).unwrap_or_else(|_| p.to_path_buf()).with_extension("kicad_sch") };

    // the sheet paths each screen is shown on (`SCH_SHEET_LIST`), from the root down
    let index_of = |p: &Path| screens.iter().position(|(sp, _)| sp == p);
    let mut paths_of: Vec<Vec<Vec<String>>> = vec![Vec::new(); screens.len()];
    let mut stack: Vec<(usize, Vec<String>)> = vec![(0, vec![root_uuid.clone()])];
    let mut guard = 0;
    while let Some((i, path)) = stack.pop() {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        paths_of[i].push(path.clone());
        let dir = screens[i].0.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut children = Vec::new();
        for it in &screens[i].1.items {
            if let Item::Sheet(sh) = it {
                let file = normal(&dir.join(Path::new(&sh.file.replace('\\', "/")).with_extension("sch")));
                if let Some(j) = index_of(&file) {
                    if path.contains(&sh.uuid) {
                        continue; // a sheet that places itself
                    }
                    let mut p = path.clone();
                    p.push(sh.uuid.clone());
                    children.push((j, p));
                }
            }
        }
        for c in children.into_iter().rev() {
            stack.push(c);
        }
    }

    let mut any_nickname = false;
    for (i, (path, screen)) in screens.iter().enumerate() {
        let mut body = String::new();
        // the library symbols this screen's symbols are drawn from
        let mut lib_symbols: BTreeMap<String, Sexpr> = BTreeMap::new();
        let mut linked: BTreeMap<String, Option<Vec<Sexpr>>> = BTreeMap::new();
        for it in &screen.items {
            let Item::Symbol(sym) = it else { continue };
            if !sym.nickname.is_empty() {
                any_nickname = true;
            }
            let id = sym.lib_id();
            if linked.contains_key(&id) {
                continue;
            }
            let found = if sym.item.is_empty() {
                written.messages.push(format!("Schematic symbol reference '{id}' library identifier is not valid. Unable to link library symbol."));
                None
            } else {
                lookup(&id).and_then(|l| {
                    if l.from_cache {
                        written.messages.push(format!("Falling back to cache to set symbol '{}:{}' link '{}'.", sym.reference(), sym.field(1).map(|f| f.text.as_str()).unwrap_or(""), id.replace(':', "_")));
                    }
                    flattened_symbol(&l.library_text, &l.name, &id)
                })
            };
            if let Some(s) = &found {
                lib_symbols.insert(id.clone(), s.clone());
            } else {
                written.messages.push(format!("No library symbol found for schematic symbol '{} {}'.", sym.reference(), sym.field(1).map(|f| f.text.as_str()).unwrap_or("")));
            }
            linked.insert(id, found.and_then(|s| s.as_list().map(<[Sexpr]>::to_vec)));
        }
        if !lib_symbols.is_empty() {
            body.push_str("  (lib_symbols\n");
            for s in lib_symbols.values() {
                body.push_str("    ");
                body.push_str(&sexpr::to_text(s));
                body.push('\n');
            }
            body.push_str("  )\n");
        }
        let shared = paths_of[i].len() > 1;
        for it in &screen.items {
            match it {
                Item::Junction(p) => body.push_str(&format!("  (junction (at {}) (diameter 0) (color 0 0 0 0) (uuid {}))\n", xy(*p), q(&ids.next()))),
                Item::NoConnect(p) => body.push_str(&format!("  (no_connect (at {}) (uuid {}))\n", xy(*p), q(&ids.next()))),
                Item::BusEntry { bus_bus: false, pos, size } => body.push_str(&format!("  (bus_entry (at {}) (size {}) (stroke (width 0) (type default)) (uuid {}))\n", xy(*pos), xy(*size), q(&ids.next()))),
                // "Convert any legacy bus-bus entries to just be bus wires"
                Item::BusEntry { bus_bus: true, pos, size } => body.push_str(&format!("  (bus (pts (xy {}) (xy {})) (stroke (width 0) (type default)) (uuid {}))\n", xy(*pos), xy((pos.0 + size.0, pos.1 + size.1)), q(&ids.next()))),
                Item::Line(l) => {
                    let tag = match l.layer {
                        LineLayer::Wire => "wire",
                        LineLayer::Bus => "bus",
                        LineLayer::Notes => "polyline",
                    };
                    let color = l.color.map(|c| format!(" (color {} {} {} {})", (c[0] * 255.0).round(), (c[1] * 255.0).round(), (c[2] * 255.0).round(), num(c[3]))).unwrap_or_default();
                    body.push_str(&format!("  ({tag} (pts (xy {}) (xy {})) (stroke (width {}) (type {}){color}) (uuid {}))\n", xy(l.start), xy(l.end), mm(l.width), l.style.unwrap_or("default"), q(&ids.next())));
                }
                Item::Text(t) => body.push_str(&text_sexpr(t, &mut ids)),
                Item::Bitmap(b) => {
                    let data = crate::embedded::base64_encode(&b.png);
                    let chunks: Vec<String> = data.as_bytes().chunks(76).map(|c| q(std::str::from_utf8(c).unwrap_or(""))).collect();
                    body.push_str(&format!("  (image (at {}) (scale {}) (uuid {}) (data {}))\n", xy(b.pos), num(b.scale), q(&ids.next()), chunks.join(" ")));
                }
                Item::Symbol(sym) => body.push_str(&symbol_sexpr(sym, linked.get(&sym.lib_id()).and_then(|l| l.as_deref()), &paths_of[i], shared, project, &mut ids)),
                Item::Sheet(sh) => body.push_str(&sheet_sexpr(sh, &mut ids)),
            }
        }
        for (name, members) in &screen.bus_aliases {
            body.push_str(&format!("  (bus_alias {} (members {}))\n", q(name), members.iter().map(|m| q(m)).collect::<Vec<_>>().join(" ")));
        }
        let screen_uuid = if i == 0 { root_uuid.clone() } else { ids.next() };
        let mut text = format!("(kicad_sch (version {WRITTEN_VERSION}) (generator \"eeschema\") (generator_version \"10.0\")\n  (uuid {})\n", q(&screen_uuid));
        if let Some(page) = &screen.page {
            if page.kind == "User" {
                text.push_str(&format!("  (paper \"User\" {} {})\n", num(page.width_mils as f64 * 25.4 / 1000.0), num(page.height_mils as f64 * 25.4 / 1000.0)));
            } else {
                text.push_str(&format!("  (paper {}{})\n", q(&page.kind), if page.portrait { " portrait" } else { "" }));
            }
        }
        let tb: Vec<String> = screen
            .title
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| if k.len() == 1 { format!("(comment {k} {})", q(v)) } else { format!("({k} {})", q(v)) })
            .collect();
        if !tb.is_empty() {
            text.push_str(&format!("  (title_block {})\n", tb.join(" ")));
        }
        text.push_str(&body);
        if i == 0 {
            // the root's own sheet instance (`(sheet_instances (path "/" (page "1")))`)
            text.push_str("  (sheet_instances (path \"/\" (page \"1\")))\n");
        }
        text.push_str(")\n");
        let out = out_dir.join(key_of(path));
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
        written.files.push(out);
    }
    written.root = written.files.first().cloned().unwrap_or_default();
    written.no_fully_defined_lib_ids = !any_nickname && screens.iter().any(|(_, s)| s.items.iter().any(|it| matches!(it, Item::Symbol(_))));
    written.unlinked = written.messages.iter().filter(|m| m.starts_with("No library symbol found")).count();
    Ok(written)
}

/// `saveText` of a text or a label, its angle and justification from the spin style (`SCH_LABEL_BASE::SetSpinStyle`, a global and a hierarchical
/// label centred vertically; `loadText`'s own switch for a text).
fn text_sexpr(t: &Text, ids: &mut Ids) -> String {
    let (angle, h) = match t.spin {
        1 => (90, HJustify::Left),
        0 => (0, HJustify::Right),
        3 => (90, HJustify::Right),
        _ => (0, HJustify::Left),
    };
    let v = if matches!(t.kind, TextKind::Global | TextKind::Hier) { VJustify::Center } else { VJustify::Bottom };
    let eff = effects(t.size, t.bold, t.italic, h, v, false);
    let uuid = q(&ids.next());
    match t.kind {
        TextKind::Notes => format!("  (text {} (at {} {angle}) {eff} (uuid {uuid}))\n", q(&t.text), xy(t.pos)),
        TextKind::Label => format!("  (label {} (at {} {angle}) {eff} (uuid {uuid}))\n", q(&t.text), xy(t.pos)),
        TextKind::Hier => format!("  (hierarchical_label {} (shape {}) (at {} {angle}) {eff} (uuid {uuid}))\n", q(&t.text), t.shape.token(), xy(t.pos)),
        TextKind::Global => {
            // the Intersheet References field every global label has (`SCH_GLOBALLABEL::SCH_GLOBALLABEL`): hidden, at the label
            let field = format!("(property \"Intersheetrefs\" \"${{INTERSHEET_REFS}}\" (at {} 0) (hide yes) {})", xy(t.pos), effects(t.size, false, false, HJustify::Left, VJustify::Center, true));
            format!("  (global_label {} (shape {}) (at {} {angle}) {eff} (uuid {uuid}) {field})\n", q(&t.text), t.shape.token(), xy(t.pos))
        }
    }
}

/// `saveSymbol`: the orientation, the fields at their place on the sheet (`SCH_FIELD::GetPosition`, the symbol's transform applied), the instances of each
/// sheet path the screen is shown on (`CheckForMissingSymbolInstances` for a path the file gives none), and the Value of a legacy power symbol made its
/// hidden power pin's name (`FixLegacyPowerSymbolMismatches`).
fn symbol_sexpr(sym: &Symbol, lib: Option<&[Sexpr]>, paths: &[Vec<String>], shared: bool, project: &str, ids: &mut Ids) -> String {
    let t = sym.transform.unwrap_or(Transform::IDENTITY);
    let (angle, mx, my) = orientation(t);
    let mut s = format!("  (symbol (lib_id {}) (at {} {angle})", q(&sym.lib_id()), xy(sym.pos));
    if mx || my {
        s.push_str(&format!(" (mirror {})", if mx { "x" } else { "y" }));
    }
    // `CheckForMissingSymbolInstances`: a path the file gives no instance for takes the reference field and the unit (a screen shown once, as a legacy
    // file keeps them), else the first instance the symbol has
    let insts: Vec<(&Vec<String>, String, i64)> = paths
        .iter()
        .map(|p| match sym.instances.iter().find(|i| i.path[..] == p[1..]) {
            Some(i) => (p, i.reference.clone(), i.unit),
            None if !shared => (p, sym.reference(), sym.unit),
            None => match sym.instances.first() {
                Some(i) => (p, i.reference.clone(), i.unit),
                None => (p, sym.reference(), sym.unit),
            },
        })
        .collect();
    // "The symbol unit is always set to the ordianal instance": the first path that shows the screen
    let ordinal = insts.first().map(|(_, r, u)| (r.clone(), *u));
    let ordinal_unit = ordinal.as_ref().map(|o| o.1).unwrap_or(1);
    s.push_str(&format!(" (unit {ordinal_unit}) (body_style {}) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid {})", sym.body_style, q(&sym.uuid)));
    let mut fields = sym.fields.clone();
    for id in 0..4 {
        if !fields.iter().any(|f| f.legacy_id == id) {
            fields.push(Field { legacy_id: id, name: ["Reference", "Value", "Footprint", "Datasheet"][id as usize].to_string(), text_pos: sym.pos, size: mils(50), visible: id < 2, ..Default::default() });
        }
    }
    // `FixLegacyPowerSymbolMismatches`
    if let Some(lib) = lib {
        if let (true, Some((true, true, name))) = first_pin_and_power(lib) {
            if let Some(v) = fields.iter_mut().find(|f| f.legacy_id == 1) {
                v.text = name;
            }
        }
    }
    let mut ordered: Vec<&Field> = fields.iter().filter(|f| f.legacy_id < 4).collect();
    ordered.sort_by_key(|f| f.legacy_id);
    ordered.extend(fields.iter().filter(|f| f.legacy_id >= 4));
    for f in ordered {
        let rel = (f.text_pos.0 - sym.pos.0, f.text_pos.1 - sym.pos.1);
        let p = t.apply(rel);
        let at = (p.0 + sym.pos.0, p.1 + sym.pos.1);
        let mut text = f.text.clone();
        if f.legacy_id == 0 {
            // "The instance fields are always set to the default instance"
            if let Some((r, _)) = &ordinal {
                text = r.clone();
            }
        }
        s.push_str(&format!(
            " (property {} {} (at {} {}){} {})",
            q(&f.name),
            q(&text),
            xy(at),
            if f.vertical { 90 } else { 0 },
            if f.visible { "" } else { " (hide yes)" },
            effects(f.size, f.bold, f.italic, f.h, f.v, false)
        ));
    }
    // the Description field every symbol has since KiCad 8
    s.push_str(&format!(" (property \"Description\" \"\" (at {} 0) (hide yes) {})", xy(sym.pos), effects(mils(50), false, false, HJustify::Center, VJustify::Center, false)));
    let insts: Vec<String> = insts.iter().map(|(p, r, u)| format!("(path {} (reference {}) (unit {u}))", q(&format!("/{}", p.join("/"))), q(r))).collect();
    if !insts.is_empty() {
        s.push_str(&format!(" (instances (project {} {}))", q(project), insts.join(" ")));
    }
    let _ = ids;
    s.push_str(")\n");
    s
}

/// `saveSheet`: the name and file fields where `SCH_SHEET::AutoplaceFields` puts them (`loadSheet` runs it), the file made `.kicad_sch`, the pins on
/// their edge.
fn sheet_sexpr(sh: &Sheet, ids: &mut Ids) -> String {
    let file = Path::new(&sh.file.replace('\\', "/")).with_extension("kicad_sch").to_string_lossy().into_owned();
    // `GetPenWidth()` of a sheet with no schematic yet: `MilsToIU( DEFAULT_LINE_WIDTH_MILS )`
    let border = ((mils(DEFAULT_LINE_WIDTH_MILS) as f64 / 2.0).round() as i64) + 4;
    let (lr, tb) = sh.pins.iter().fold((0, 0), |(lr, tb), p| match p.side {
        Side::Left | Side::Right => (lr + 1, tb),
        _ => (lr, tb + 1),
    });
    let vertical = tb > 0 && lr == 0;
    let name_margin = border + (sh.name_size as f64 * 0.5).round() as i64;
    let file_margin = border + (sh.file_size as f64 * 0.4).round() as i64;
    let (name_at, file_at, angle) = if vertical {
        ((sh.pos.0 - name_margin, sh.pos.1 + sh.size.1), (sh.pos.0 + sh.size.0 + file_margin, sh.pos.1 + sh.size.1), 90)
    } else {
        ((sh.pos.0, sh.pos.1 - name_margin), (sh.pos.0, sh.pos.1 + sh.size.1 + file_margin), 0)
    };
    let mut s = format!(
        "  (sheet (at {}) (size {}) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (fields_autoplaced yes) (stroke (width 0) (type solid)) (fill (color 0 0 0 0.0000)) (uuid {})",
        xy(sh.pos),
        xy(sh.size),
        q(&sh.uuid)
    );
    s.push_str(&format!(" (property \"Sheetname\" {} (at {} {angle}) {})", q(&sh.name), xy(name_at), effects(sh.name_size, false, false, HJustify::Left, VJustify::Bottom, false)));
    s.push_str(&format!(" (property \"Sheetfile\" {} (at {} {angle}) {})", q(&file), xy(file_at), effects(sh.file_size, false, false, HJustify::Left, VJustify::Top, false)));
    for p in &sh.pins {
        // `SetSide`'s spin style and `getSheetPinAngle`
        let (angle, h) = match p.side {
            Side::Left => (180, HJustify::Left),
            Side::Right => (0, HJustify::Right),
            Side::Top => (90, HJustify::Right),
            Side::Bottom => (270, HJustify::Left),
        };
        s.push_str(&format!(" (pin {} {} (at {} {angle}) (uuid {}) {})", q(&p.text), p.shape.token(), xy(p.pos), q(&ids.next()), effects(p.size, false, false, h, VJustify::Center, false)));
    }
    s.push_str(")\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_says_whether_a_file_is_a_legacy_schematic() {
        assert!(is_legacy_schematic("EESchema Schematic File Version 4\nLIBS:x\n"));
        assert!(is_legacy_schematic("Eeschema Schematic File Version 2\n"));
        assert!(!is_legacy_schematic("(kicad_sch (version 20250114))"));
        assert!(!is_legacy_schematic("<?xml version=\"1.0\"?>\n<eagle>"));
    }

    #[test]
    fn the_parse_helpers_read_as_kicads_do() {
        assert_eq!(parse_int("  -12 rest", 1).unwrap(), (-12, "rest"));
        assert_eq!(parse_int("x", 1).unwrap(), (0, "x"), "no digit is 0 and stays put, as strtol");
        assert_eq!(parse_hex("0001 C", 1).unwrap(), (1, "C"));
        assert_eq!(parse_quoted("\"a\\\"b\\\\c\\d\" next", 1, false).unwrap(), ("a\"b\\c\\d".to_string(), "next"));
        assert!(parse_char("H\n", 1).is_ok() && parse_char("HV ", 1).is_err());
        assert_eq!(str_compare("Wire", "Wire Wire Line\n"), Some("Wire Line\n"));
        assert_eq!(str_compare("Wire", "Wires"), None);
        assert_eq!(kiid_of("4B3A13A4"), "00000000-0000-0000-0000-00004b3a13a4");
    }

    #[test]
    fn a_symbols_orientation_is_the_first_of_kicads_choices_its_transform_matches() {
        // the file's `1 0 0 -1` (KiCad 6's y negated: identity)
        assert_eq!(orientation(Transform(1, 0, 0, 1)), (0, false, false));
        assert_eq!(orientation(Transform(0, 1, -1, 0)), (90, false, false));
        assert_eq!(orientation(Transform(-1, 0, 0, -1)), (180, false, false));
        assert_eq!(orientation(Transform(0, -1, 1, 0)), (270, false, false));
        assert_eq!(orientation(Transform(1, 0, 0, -1)), (0, true, false));
        assert_eq!(orientation(Transform(-1, 0, 0, 1)), (0, false, true));
    }

    const SAMPLE: &str = "EESchema Schematic File Version 4
LIBS:power
EELAYER 30 0
EELAYER END
$Descr A4 11693 8268
encoding utf-8
Sheet 1 2
Title \"Demo\"
Date \"\"
Rev \"1\"
Comp \"\"
Comment1 \"first\"
$EndDescr
$Comp
L Device:R R1
U 1 1 5A000001
P 2000 1500
F 0 \"R1\" V 2080 1500 50  0000 C CNN
F 1 \"10k\" V 2000 1500 50  0000 C CNN
F 2 \"\" V 1930 1500 50  0001 C CNN
F 3 \"~\" H 2000 1500 50  0001 C CNN
F 4 \"X\" H 2000 1500 50  0001 C CNN \"MPN\"
	1    2000 1500
	1    0    0    -1
$EndComp
Wire Wire Line
	2000 1350 2000 1200
Text GLabel 2000 1200 1    50   Input ~ 0
IN
Text Label 2000 1700 0    50   ~ 0
~OUT~
Connection ~ 2000 1650
NoConn ~ 2500 1500
Entry Bus Bus
	3000 3000 3100 3100
$Sheet
S 4000 1000 1000 800
U 5A000002
F0 \"Sub\" 50
F1 \"sub.sch\" 50
F2 \"A\" I L 4010 1200 50
$EndSheet
$EndSCHEMATC
";

    #[test]
    fn a_legacy_file_is_read_as_loadfile_reads_it() {
        let s = parse_screen(SAMPLE).unwrap();
        assert_eq!(s.version, 4);
        assert_eq!(s.page.as_ref().unwrap().kind, "A4");
        assert!(s.title.contains(&("title".to_string(), "Demo".to_string())) && s.title.contains(&("1".to_string(), "first".to_string())));
        let Item::Symbol(sym) = &s.items[0] else { panic!() };
        assert_eq!((sym.lib_id().as_str(), sym.prefix.as_str(), sym.uuid.as_str()), ("Device:R", "R", "00000000-0000-0000-0000-00005a000001"));
        assert_eq!(sym.transform, Some(Transform(1, 0, 0, 1)));
        assert_eq!(sym.fields.iter().find(|f| f.legacy_id == 4).unwrap().name, "MPN");
        assert!(sym.fields.iter().find(|f| f.legacy_id == 0).unwrap().vertical);
        let labels: Vec<&Text> = s.items.iter().filter_map(|i| if let Item::Text(t) = i { Some(t) } else { None }).collect();
        assert_eq!((labels[0].kind, labels[0].spin, labels[0].shape), (TextKind::Global, 1, LabelShape::Input));
        assert_eq!((labels[1].kind, labels[1].spin, labels[1].text.as_str()), (TextKind::Label, 2, "~{OUT}"), "a local label's 0 is the global numbering's 2, and the overbar is converted");
        let Some(Item::Sheet(sh)) = s.items.iter().find(|i| matches!(i, Item::Sheet(_))) else { panic!() };
        assert_eq!((sh.name.as_str(), sh.file.as_str(), sh.pins[0].side, sh.pins[0].pos), ("Sub", "sub.sch", Side::Left, (mils(4000), mils(1200))), "the pin is put on the edge it is nearest");
        assert!(s.items.iter().any(|i| matches!(i, Item::BusEntry { bus_bus: true, .. })));
    }

    #[test]
    fn the_tree_is_written_as_kicad_sch_files_the_reader_takes() {
        let dir = std::env::temp_dir().join(format!("eda-legacy-sch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("demo.sch"), SAMPLE).unwrap();
        std::fs::write(dir.join("sub.sch"), "EESchema Schematic File Version 4\nEELAYER 30 0\nEELAYER END\n$Descr A4 11693 8268\n$EndDescr\nText HLabel 1000 1000 2    50   Input ~ 0\nA\n$EndSCHEMATC\n").unwrap();
        let tree = load_tree(&dir.join("demo.sch"), &|p| std::fs::read_to_string(p).map_err(|e| e.to_string())).unwrap();
        assert_eq!(tree.screens.len(), 2);
        let lib = std::sync::Arc::new("(kicad_symbol_lib (symbol \"R\" (property \"Reference\" \"R\") (property \"Value\" \"R\") (symbol \"R_1_1\" (pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\")) (pin passive line (at 0 -3.81 90) (length 1.27) (name \"~\") (number \"2\")))))".to_string());
        let out = dir.join("out");
        let w = write_tree(&tree, &out, "demo", &|id| (id == "Device:R").then(|| LookedUp { library_text: lib.clone(), name: "R".into(), from_cache: false })).unwrap();
        assert_eq!(w.files.len(), 2);
        assert!(!w.no_fully_defined_lib_ids);
        let text = std::fs::read_to_string(&w.root).unwrap();
        assert!(text.contains("(symbol \"Device:R\""), "the library symbol is named by the schematic's id: {text}");
        assert!(text.contains("\"sub.kicad_sch\""), "the sheet's file is the new one");
        assert!(out.join("sub.kicad_sch").is_file());
        let (design, _, notes) = crate::import_kicad_sch_tree(&w.root).unwrap();
        assert_eq!(notes.unresolved_symbols, 0);
        let sch = design.schematic.unwrap();
        assert_eq!(sch.symbols.len(), 1);
        assert_eq!(sch.sheets.len(), 1);
        assert_eq!(design.sheet_contents.map(|c| c.len()), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_derived_symbol_is_flattened_onto_its_parent() {
        let lib = "(kicad_symbol_lib (symbol \"+3V3\" (power) (property \"Reference\" \"#PWR\") (property \"Value\" \"+3V3\") (symbol \"+3V3_0_1\" (pin power_in line (at 0 0 90) (length 0) (hide yes) (name \"+3V3\") (number \"1\")))) (symbol \"+3.3V\" (extends \"+3V3\") (property \"Value\" \"+3.3V\") (property \"Reference\" \"\")))";
        let s = flattened_symbol(lib, "+3.3V", "+3.3V").unwrap();
        let t = sexpr::to_text(&s);
        assert!(t.starts_with("(symbol \"+3.3V\" (power)"), "{t}");
        assert!(t.contains("(property \"Value\" \"+3.3V\")") && t.contains("(property \"Reference\" \"#PWR\")"), "an empty mandatory field of the derived one keeps the parent's: {t}");
        assert!(!t.contains("extends"));
        let (power, pin) = first_pin_and_power(s.as_list().unwrap());
        assert!(power);
        assert_eq!(pin, Some((true, true, "+3V3".to_string())));
    }
}
