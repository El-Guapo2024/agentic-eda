//! KiCad's library tables: `sym-lib-table` and `fp-lib-table`, and the path variables their library paths are written with.
//!
//! A KiCad library is found through a table, never by its place on disk: a row says "this nickname is the file or folder at this URI", and a
//! `Lib:Name` id means "the item `Name` of the row nicknamed `Lib`". KiCad keeps two tables per kind -- the global one in its configuration
//! folder (`~/Library/Preferences/kicad/<version>/`) and one per project, in the project folder -- and a row of type `Table` chains another
//! table file in, which is how the global table carries the 222 symbol and 155 footprint libraries of KiCad's own install (one row, `KiCad`,
//! pointing at `SharedSupport/template/sym-lib-table`). The project's rows are read after the global ones and a nickname defined twice is
//! the later one.
//!
//! This module is the pure half of that, ported from KiCad (read-only source, `8303b2ad`):
//!
//!   * the file format -- `common/libraries/library_table_grammar.h` and `library_table_parser.cpp` (the reader), `library_table.cpp`
//!     `LIBRARY_TABLE::Format` (the writer: `(version 7)`, one `(lib ...)` per line, `(disabled)` and `(hidden)` after `descr`);
//!   * how the tables are put together -- `library_manager.cpp` `loadTables`, `loadNestedTables` (a nested table's path is relative to
//!     the table that names it; a table already visited is an error) and `Rows` (global first, then project, a nested table expanded in
//!     place, a disabled nested table left out, later rows of a nickname replacing earlier ones, a hidden nested table hides its rows);
//!   * the variables a path may hold -- `common/common.cpp` `KIwxExpandEnvVars` (`${NAME}`, `$(NAME)` and `$NAME`; the project's own
//!     variables first, then the environment; `KICAD*_SYMBOL_DIR` and the like fall back to whatever version of them is defined), and
//!     `common/env_vars.cpp` (the predefined ones), `common/settings/common_settings.cpp` `InitializeEnvironment` (their defaults);
//!   * how a path is written back as one -- `common/env_paths.cpp` `NormalizePath`;
//!   * the dialog's checks -- `common/lib_table_grid_tricks.cpp` `VerifyTable` (a row with no path is dropped, a nickname must be there
//!     and legal, no two alike), `library_manager.cpp` `CheckTableRow` and the library readers' own messages for a path that is not there.
//!
//! Nothing here touches KiCad's configuration: the global tables are read, never written ([`write_table`] is for a project's own).

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

// ------------------------------------------------------------------------------------------------ the format

/// The two kinds of library the studio reads. (KiCad has a third table, for design blocks.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableKind {
    Symbol,
    Footprint,
}

impl TableKind {
    /// Both kinds, symbols first.
    pub const ALL: [TableKind; 2] = [TableKind::Symbol, TableKind::Footprint];

    /// The table's own keyword, the head of the file (`LIBRARY_TABLE_GRAMMAR::KEYWORDS`).
    pub fn keyword(self) -> &'static str {
        match self {
            TableKind::Symbol => "sym_lib_table",
            TableKind::Footprint => "fp_lib_table",
        }
    }

    /// The file's name in a configuration or project folder (`FILEEXT::SymbolLibraryTableFileName`, `FootprintLibraryTableFileName`).
    pub fn file_name(self) -> &'static str {
        match self {
            TableKind::Symbol => "sym-lib-table",
            TableKind::Footprint => "fp-lib-table",
        }
    }

    pub fn parse(s: &str) -> Option<TableKind> {
        match s {
            "symbol" => Some(TableKind::Symbol),
            "footprint" => Some(TableKind::Footprint),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TableKind::Symbol => "symbol",
            TableKind::Footprint => "footprint",
        }
    }
}

/// The type of KiCad's own library format (`SCH_IO_MGR::ShowType( SCH_KICAD )`, `PCB_IO_MGR::ShowType( KICAD_SEXP )`): `.kicad_sym` files and `.pretty` folders.
pub const KICAD_TYPE: &str = "KiCad";
/// The type of a row that chains another table (`LIBRARY_TABLE_ROW::TABLE_TYPE_NAME`).
pub const NESTED_TYPE: &str = "Table";
/// The version `LIBRARY_TABLE::Format` writes.
pub const TABLE_VERSION: i64 = 7;

/// The library formats KiCad names in a row's type, symbol side (`SCH_IO_MGR::ShowType`) and footprint side (the `PCB_IO_MGR` plugin names). The studio reads
/// [`KICAD_TYPE`] and chains [`NESTED_TYPE`]; the others are listed, kept in the file and flagged as unread.
pub fn known_types(kind: TableKind) -> &'static [&'static str] {
    match kind {
        TableKind::Symbol => &["KiCad", "Legacy", "Altium", "CADSTAR Schematic Archive", "Database", "EAGLE", "EasyEDA (JLCEDA) Std", "EasyEDA (JLCEDA) Pro", "gEDA / Lepton EDA", "LTspice", "HTTP", "PADS Logic", "Table"],
        TableKind::Footprint => &["KiCad", "Legacy", "Allegro", "Altium Circuit Maker", "Altium Circuit Studio", "Altium Designer", "CADSTAR PCB Archive", "Eagle", "EasyEDA / JLCEDA Std", "EasyEDA / JLCEDA Pro", "Fabmaster", "GPCB", "P-Cad", "SolidWorks PCB", "IPC-2581", "ODB++", "PADS", "Sprint Layout", "Table"],
    }
}

/// One `(lib ...)` of a table (`LIBRARY_TABLE_ROW`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub nickname: String,
    pub uri: String,
    /// The library format: `KiCad`, `Legacy`, ..., or `Table` for a chained table.
    pub lib_type: String,
    pub options: String,
    pub description: String,
    pub disabled: bool,
    /// The "Show" box of the symbol table, turned off: the library is still found, but the trees and the chooser leave it out.
    pub hidden: bool,
}

impl Row {
    pub fn is_table(&self) -> bool {
        self.lib_type == NESTED_TYPE
    }
}

/// A parsed `sym-lib-table` or `fp-lib-table` (`LIBRARY_TABLE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub kind: TableKind,
    pub version: Option<i64>,
    pub rows: Vec<Row>,
}

/// Where a table file did not parse: KiCad's own wording, `Syntax error at line L, column C`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn err(&self, at: usize, what: &str) -> ParseError {
        let upto = &self.b[..at.min(self.b.len())];
        let line = 1 + upto.iter().filter(|&&c| c == b'\n').count();
        let column = 1 + upto.iter().rev().take_while(|&&c| c != b'\n').count();
        ParseError { line, column, message: format!("Syntax error at line {line}, column {column}: {what}") }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// `pegtl`'s `space`: blank, tab, newline, carriage return, form feed, vertical tab.
    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8, what: &str) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(self.i, what))
        }
    }

    /// `TOKEN`: anything up to a parenthesis or a blank.
    fn token(&mut self) -> &'a str {
        let start = self.i;
        while matches!(self.peek(), Some(c) if !matches!(c, b'(' | b')' | b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i]).unwrap_or("")
    }

    /// `PROPERTY_VALUE`: a quoted text, which ends at the next quote (the reader has no escapes), or a token.
    fn value(&mut self) -> Result<String, ParseError> {
        if self.peek() == Some(b'"') {
            let open = self.i;
            self.i += 1;
            let start = self.i;
            while matches!(self.peek(), Some(c) if c != b'"') {
                self.i += 1;
            }
            if self.peek().is_none() {
                return Err(self.err(open, "a quoted text is not closed"));
            }
            let text = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
            self.i += 1;
            Ok(text)
        } else {
            let at = self.i;
            let t = self.token();
            if t.is_empty() {
                return Err(self.err(at, "a value is expected"));
            }
            Ok(t.to_string())
        }
    }
}

impl Table {
    pub fn new(kind: TableKind) -> Table {
        Table { kind, version: Some(TABLE_VERSION), rows: Vec::new() }
    }

    /// `LIBRARY_TABLE_PARSER::ParseBuffer`. Laxer than KiCad's grammar in blanks only (before the closing parenthesis of a property, around the
    /// whole file); a keyword the grammar does not have, a text that is not closed or anything after the table is the same error.
    pub fn parse(text: &str) -> Result<Table, ParseError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut r = Reader { b: text.as_bytes(), i: 0 };
        r.skip_space();
        r.expect(b'(', "a library table starts with a parenthesis")?;
        let head_at = r.i;
        let head = r.token();
        let kind = match head {
            "sym_lib_table" => TableKind::Symbol,
            "fp_lib_table" => TableKind::Footprint,
            "design_block_lib_table" => return Err(r.err(head_at, "a design block library table is not read here")),
            _ => return Err(r.err(head_at, "expected sym_lib_table or fp_lib_table")),
        };
        let mut table = Table { kind, version: None, rows: Vec::new() };
        loop {
            r.skip_space();
            match r.peek() {
                Some(b')') => {
                    r.i += 1;
                    break;
                }
                Some(b'(') => {
                    r.i += 1;
                    r.skip_space();
                    let at = r.i;
                    match r.token() {
                        "version" => {
                            if !matches!(r.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                                return Err(r.err(r.i, "a version is expected"));
                            }
                            r.skip_space();
                            let v = r.value()?;
                            r.skip_space();
                            r.expect(b')', "expected a closing parenthesis")?;
                            table.version = v.trim().parse::<i64>().ok();
                        }
                        "lib" => table.rows.push(Self::row(&mut r)?),
                        _ => return Err(r.err(at, "expected version or lib")),
                    }
                }
                _ => return Err(r.err(r.i, "expected a lib or the end of the table")),
            }
        }
        r.skip_space();
        if r.peek().is_some() {
            return Err(r.err(r.i, "nothing may follow the table"));
        }
        Ok(table)
    }

    /// After `(lib`: the properties up to the closing parenthesis (`LIB_ROW`).
    fn row(r: &mut Reader) -> Result<Row, ParseError> {
        if !matches!(r.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            return Err(r.err(r.i, "a lib needs properties"));
        }
        let mut row = Row::default();
        let mut members = 0;
        loop {
            r.skip_space();
            match r.peek() {
                Some(b')') => {
                    if members == 0 {
                        return Err(r.err(r.i, "a lib needs properties"));
                    }
                    r.i += 1;
                    return Ok(row);
                }
                Some(b'(') => {
                    r.i += 1;
                    let at = r.i;
                    let key = r.token();
                    match key {
                        "hidden" | "disabled" => {
                            r.skip_space();
                            r.expect(b')', "expected a closing parenthesis")?;
                            if key == "hidden" {
                                row.hidden = true;
                            } else {
                                row.disabled = true;
                            }
                        }
                        "name" | "type" | "uri" | "options" | "descr" => {
                            if !matches!(r.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                                return Err(r.err(r.i, "a value is expected"));
                            }
                            r.skip_space();
                            let value = r.value()?;
                            r.skip_space();
                            r.expect(b')', "expected a closing parenthesis")?;
                            match key {
                                "name" => row.nickname = value,
                                "type" => row.lib_type = value,
                                "uri" => row.uri = value,
                                "options" => row.options = value,
                                _ => row.description = value,
                            }
                        }
                        _ => return Err(r.err(at, "expected name, type, uri, options, descr, hidden or disabled")),
                    }
                    members += 1;
                }
                _ => return Err(r.err(r.i, "expected a property of the lib")),
            }
        }
    }

    /// `LIBRARY_TABLE::Format` through the library-table prettifier: `(version 7)`, then a line to a `(lib ...)`, tab-indented. A value is written
    /// raw, as the reader (which has no escapes) takes it: a quote cannot be written, so it becomes an apostrophe, and a path never holds a backslash.
    pub fn format(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "'").replace(['\n', '\r'], " "));
        let mut out = format!("({}\n\t(version {TABLE_VERSION})\n", self.kind.keyword());
        for row in &self.rows {
            out.push_str(&format!(
                "\t(lib (name {}) (type {}) (uri {}) (options {}) (descr {})",
                quote(&row.nickname),
                quote(&row.lib_type),
                quote(&row.uri.replace('\\', "/")),
                quote(&row.options),
                quote(&row.description)
            ));
            if row.disabled {
                out.push_str(" (disabled)");
            }
            if row.hidden {
                out.push_str(" (hidden)");
            }
            out.push_str(")\n");
        }
        out.push_str(")\n");
        out
    }

    pub fn has_row(&self, nickname: &str) -> bool {
        self.rows.iter().any(|r| r.nickname == nickname)
    }

    pub fn row_named(&self, nickname: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.nickname == nickname)
    }
}

/// Write a project's table beside the project (`LIBRARY_TABLE::Save`'s job, without the backup file): through a temporary file and a rename, so a
/// crash leaves the old table whole. Never for a global table -- KiCad's configuration is the user's, not ours.
pub fn write_table(path: &Path, table: &Table) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, table.format()).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write {}: {e}", path.display())
    })
}

// ------------------------------------------------------------------------------------------------ nicknames

/// `LIB_ID::FindIllegalLibraryNameChar`: the first character a library nickname may not hold -- a control character, a backslash or a colon (the
/// colon is what separates the nickname from the item's name in `Lib:Name`).
pub fn illegal_nickname_char(nickname: &str) -> Option<char> {
    nickname.chars().find(|&c| (c as u32) < 0x20 || c == '\\' || c == ':')
}

/// `LIB_ID::FixIllegalChars( name, true )`: each illegal character becomes `_`; what a library file's name becomes as a nickname.
pub fn fix_nickname(name: &str) -> String {
    name.chars().map(|c| if (c as u32) < 0x20 || c == '\\' || c == ':' { '_' } else { c }).collect()
}

// ------------------------------------------------------------------------------------------------ variables

/// `wxString::Matches`: `*` is any run of characters, `?` any one.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// The names KiCad defines itself (`predefinedEnvVars` of `common/env_vars.cpp`) for a version of it: they cannot be renamed or removed in Configure Paths.
pub fn predefined_names(major: u32) -> Vec<String> {
    ["KIPRJMOD".to_string(), format!("KICAD{major}_SYMBOL_DIR"), format!("KICAD{major}_3DMODEL_DIR"), format!("KICAD{major}_FOOTPRINT_DIR"), format!("KICAD{major}_TEMPLATE_DIR"), "KICAD_USER_TEMPLATE_DIR".to_string(), "KICAD_PTEMPLATES".to_string(), format!("KICAD{major}_3RD_PARTY")].into()
}

/// `ENV_VAR::IsEnvVarImmutable`.
pub fn is_predefined(name: &str, major: u32) -> bool {
    predefined_names(major).iter().any(|n| n == name)
}

/// What `${NAME}` can mean: the project's own variables, then the environment (`KIwxExpandEnvVars` asks `PROJECT::TextVarResolver` first, then `wxGetEnv`).
#[derive(Clone, Debug, Default)]
pub struct Vars {
    /// The project's variables (text variables in KiCad; the studio's project path variables).
    pub text: BTreeMap<String, String>,
    /// What KiCad's process has in its environment: `KIPRJMOD`, the variables it defines (and the ones the user changed), the real environment.
    pub env: BTreeMap<String, String>,
    /// The names of the predefined variables, for the versioned fallback (`KICAD8_SYMBOL_DIR` in an old table reads `KICAD10_SYMBOL_DIR`).
    pub predefined: Vec<String>,
}

impl Vars {
    /// `ExpandEnvVarSubstitutions`: every `${NAME}`, `$(NAME)` and `$NAME` that has a value is replaced by it, and the result is expanded again while it still
    /// holds something to expand (a variable whose value names another), unless that comes back to a text already seen. A name nothing defines stays as written.
    pub fn expand(&self, text: &str) -> String {
        self.expand_with(text, None)
    }

    fn versioned(&self, pattern: &str, out: &mut String) -> bool {
        for name in &self.predefined {
            if glob(pattern, name) {
                if let Some(v) = self.env.get(name) {
                    out.push_str(v);
                    return true;
                }
            }
        }
        false
    }

    fn expand_with(&self, text: &str, seen: Option<&mut HashSet<String>>) -> String {
        let mut seen = seen;
        if let Some(set) = seen.as_deref_mut() {
            if !set.insert(text.to_string()) {
                return text.to_string();
            }
        }
        let c: Vec<char> = text.chars().collect();
        let len = c.len();
        let mut out = String::with_capacity(len);
        let mut n = 0usize;
        while n < len {
            let mut ch = c[n];
            match ch {
                '$' => {
                    // `Bracket_Normal` / `Bracket_Curly` hold the closing character; none for a bare `$NAME`.
                    let mut close: Option<char> = None;
                    if n + 1 < len {
                        match c[n + 1] {
                            '(' => {
                                close = Some(')');
                                n += 1;
                                ch = c[n];
                            }
                            '{' => {
                                close = Some('}');
                                n += 1;
                                ch = c[n];
                            }
                            _ => {}
                        }
                    }
                    let mut m = n + 1;
                    if m >= len {
                        break;
                    }
                    let mut after: Option<char> = Some(c[m]);
                    while let Some(a) = after {
                        if !(a.is_alphanumeric() || a == '_' || a == ':') {
                            break;
                        }
                        m += 1;
                        if m == len {
                            after = None;
                            break;
                        }
                        after = Some(c[m]);
                    }
                    let name: String = c[n + 1..m].iter().collect();
                    let mut expanded = false;
                    let wildcard = |p: &str| glob(p, &name);
                    if let Some(v) = self.text.get(&name) {
                        out.push_str(v);
                        expanded = true;
                    } else if let Some(v) = self.env.get(&name) {
                        out.push_str(v);
                        expanded = true;
                    } else if name.contains("KISYS3DMOD") || wildcard("KICAD*_3DMODEL_DIR") {
                        expanded = self.versioned("KICAD*_3DMODEL_DIR", &mut out);
                    } else if wildcard("KICAD*_SYMBOL_DIR") {
                        expanded = self.versioned("KICAD*_SYMBOL_DIR", &mut out);
                    } else if wildcard("KICAD*_FOOTPRINT_DIR") {
                        expanded = self.versioned("KICAD*_FOOTPRINT_DIR", &mut out);
                    } else if wildcard("KICAD*_3RD_PARTY") {
                        expanded = self.versioned("KICAD*_3RD_PARTY", &mut out);
                    } else {
                        // a variable nothing defines is left as it is
                        if close.is_some() {
                            out.push(c[n - 1]);
                        }
                        out.push(ch);
                        out.push_str(&name);
                    }
                    if !expanded && close.is_some() {
                        let versioned = name.contains("KISYS3DMOD") || wildcard("KICAD*_3DMODEL_DIR") || wildcard("KICAD*_SYMBOL_DIR") || wildcard("KICAD*_FOOTPRINT_DIR") || wildcard("KICAD*_3RD_PARTY");
                        if versioned {
                            out.push(c[n - 1]);
                            out.push(ch);
                            out.push_str(&name);
                        }
                    }
                    if let Some(close) = close {
                        // a missing closing bracket is KiCad's warning and no more
                        if m < len && after == Some(close) {
                            if !expanded {
                                out.push(close);
                            }
                            m += 1;
                        }
                    }
                    n = m;
                    continue;
                }
                '\\' if n + 1 < len && (c[n + 1] == '%' || c[n + 1] == '$') => {
                    n += 1;
                    out.push(c[n]);
                }
                _ => out.push(ch),
            }
            n += 1;
        }
        let first = out.find(['{', '(', '%']);
        let last = out.rfind(['}', ')', '%']);
        if let (Some(f), Some(l)) = (first, last) {
            if f != l {
                let mut local = HashSet::new();
                let set = match seen {
                    Some(s) => s,
                    None => &mut local,
                };
                return self.expand_with(&out, Some(set));
            }
        }
        out
    }

    /// `LIBRARY_MANAGER::ExpandURI`: the variables replaced, then the path made absolute (a relative one is relative to `base`, KiCad's working folder) and
    /// tidied (`.` and `..` resolved).
    pub fn expand_uri(&self, uri: &str, base: &Path) -> PathBuf {
        let expanded = self.expand(uri);
        absolute(Path::new(&expanded), base)
    }
}

/// `path` made absolute against `base` with `.` and `..` resolved by name (`wxFileName::MakeAbsolute`; nothing is read from disk, so a path that is not there still has a name).
pub fn absolute(path: &Path, base: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What `ENV_VAR_NAME_VALIDATOR` lets a name of a path variable be made of: capital letters, digits (not first) and `_`. Typing a lower-case letter gives the capital.
pub fn fix_variable_name(typed: &str) -> String {
    let mut out = String::new();
    for c in typed.chars() {
        let c = c.to_ascii_uppercase();
        if c == '_' || c.is_ascii_uppercase() || (c.is_ascii_digit() && !out.is_empty()) {
            out.push(c);
        }
    }
    out
}

/// Whether `name` is one a path variable may have.
pub fn is_variable_name(name: &str) -> bool {
    !name.is_empty() && fix_variable_name(name) == name
}

/// The variable names a text uses (`${NAME}` and `$(NAME)`), in order of first use -- what the dialogs' "Available path substitutions" and Configure Paths scan the tables for.
pub fn variables_used(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 1 < c.len() {
        if c[i] == '$' && (c[i + 1] == '{' || c[i + 1] == '(') {
            let close = if c[i + 1] == '{' { '}' } else { ')' };
            if let Some(end) = c[i + 2..].iter().position(|&x| x == close) {
                let name: String = c[i + 2..i + 2 + end].iter().collect();
                if !name.is_empty() && !found.contains(&name) {
                    found.push(name);
                }
                i += end + 3;
                continue;
            }
        }
        i += 1;
    }
    found
}

/// `NormalizePath`: a path written the way a table holds it. The variable whose folder holds the file deepest wins (`${KICAD10_SYMBOL_DIR}/Device.kicad_sym`), else
/// the project's folder (`${KIPRJMOD}/libs/Mine.kicad_sym`), else the path as it is. `vars` are name and folder pairs; a folder that is not there is not tried.
pub fn normalize_path(file: &Path, vars: &[(String, String)], project_dir: Option<&Path>) -> String {
    let Some(parent) = file.parent() else { return file.display().to_string() };
    let Some(file_name) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else { return file.display().to_string() };
    let dirs = |p: &Path| -> Vec<String> { p.components().filter_map(|c| if let Component::Normal(s) = c { Some(s.to_string_lossy().into_owned()) } else { None }).collect() };
    let inside = |folder: &Path| -> Option<Vec<String>> {
        if !folder.is_absolute() || !file.is_absolute() {
            return None;
        }
        let (a, b) = (dirs(folder), dirs(parent));
        if a.len() > b.len() || a.iter().zip(&b).any(|(x, y)| x != y) {
            return None;
        }
        Some(b[a.len()..].to_vec())
    };
    let mut best: Option<(usize, &str, Vec<String>)> = None;
    for (name, value) in vars {
        let folder = Path::new(value);
        if !folder.is_dir() {
            continue;
        }
        if let Some(rest) = inside(folder) {
            let depth = dirs(folder).len();
            if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
                best = Some((depth, name, rest));
            }
        }
    }
    let (name, rest) = match best {
        Some((_, name, rest)) => (name.to_string(), rest),
        None => match project_dir.and_then(|p| inside(p).map(|rest| ("KIPRJMOD".to_string(), rest))) {
            Some(found) => found,
            None => return file.display().to_string(),
        },
    };
    let mut out = format!("${{{name}}}/");
    for d in rest {
        out.push_str(&d);
        out.push('/');
    }
    out.push_str(&file_name);
    out
}

// ------------------------------------------------------------------------------------------------ KiCad's own environment

/// Where KiCad's installed data lives (`PATHS::GetStock*Path`), the defaults of its predefined variables.
#[derive(Clone, Debug)]
pub struct Stock {
    pub symbols: PathBuf,
    pub footprints: PathBuf,
    pub models: PathBuf,
    pub templates: PathBuf,
    pub design_blocks: PathBuf,
    pub user_templates: PathBuf,
    pub third_party: PathBuf,
}

impl Stock {
    /// The stock folders beside the symbol libraries (`SharedSupport/symbols` -> `SharedSupport/template`, `blocks`), the two library roots and the 3D model root as given,
    /// the user's own under `~/Documents/KiCad/<version>`.
    pub fn new(symbols: PathBuf, footprints: PathBuf, models: PathBuf, user_documents: &Path) -> Stock {
        let beside = |name: &str| symbols.parent().map_or_else(|| PathBuf::from(name), |p| p.join(name));
        Stock { templates: beside("template"), design_blocks: beside("blocks"), user_templates: user_documents.join("template"), third_party: user_documents.join("3rdparty"), symbols, footprints, models }
    }
}

/// One variable of KiCad's own environment (`ENV_VAR_ITEM`): what Configure Paths lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
    /// Defined in the real environment, which takes precedence over the table (the dialog greys the row).
    pub external: bool,
    /// Still the value KiCad gave it.
    pub default: bool,
}

/// KiCad's environment as its process holds it after `InitializeEnvironment` and the settings are read: the defaults of the predefined variables, replaced by the ones
/// in `kicad_common.json` (`environment.vars`), replaced by the real environment where that defines the same name.
pub fn global_env_vars(major: u32, stock: &Stock, configured: &BTreeMap<String, String>, real_env: &BTreeMap<String, String>) -> Vec<EnvVar> {
    let defaults: Vec<(String, String)> = vec![
        (format!("KICAD{major}_FOOTPRINT_DIR"), stock.footprints.display().to_string()),
        (format!("KICAD{major}_3DMODEL_DIR"), stock.models.display().to_string()),
        (format!("KICAD{major}_TEMPLATE_DIR"), stock.templates.display().to_string()),
        ("KICAD_USER_TEMPLATE_DIR".to_string(), stock.user_templates.display().to_string()),
        (format!("KICAD{major}_3RD_PARTY"), stock.third_party.display().to_string()),
        (format!("KICAD{major}_SYMBOL_DIR"), stock.symbols.display().to_string()),
        (format!("KICAD{major}_DESIGN_BLOCK_DIR"), stock.design_blocks.display().to_string()),
    ];
    let mut out: BTreeMap<String, EnvVar> = BTreeMap::new();
    for (name, default) in defaults {
        match real_env.get(&name).filter(|v| !v.is_empty()) {
            Some(v) => out.insert(name.clone(), EnvVar { name, value: v.clone(), external: true, default: *v == default }),
            None => {
                let value = configured.get(&name).cloned().unwrap_or_else(|| default.clone());
                let is_default = value == default;
                out.insert(name.clone(), EnvVar { name, value, external: false, default: is_default })
            }
        };
    }
    for (name, value) in configured {
        if out.contains_key(name) {
            continue;
        }
        match real_env.get(name).filter(|v| !v.is_empty()) {
            Some(v) => out.insert(name.clone(), EnvVar { name: name.clone(), value: v.clone(), external: true, default: false }),
            None => out.insert(name.clone(), EnvVar { name: name.clone(), value: value.clone(), external: false, default: false }),
        };
    }
    out.into_values().collect()
}

/// `environment.vars` of a `kicad_common.json`: the paths the user defined in Configure Paths. `null` or a missing section is none.
pub fn configured_env_vars(json_text: &str) -> BTreeMap<String, String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) else { return BTreeMap::new() };
    v.pointer("/environment/vars")
        .and_then(|vars| vars.as_object())
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default()
}

/// The major version in a KiCad configuration folder's name (`10.99` -> 10).
pub fn major_of(version_dir: &str) -> Option<u32> {
    version_dir.split('.').next()?.parse().ok()
}

/// The newest version folder (`10.99` over `9.0`, `7.0`) of KiCad's configuration root that holds a library table, or of `KICAD_CONFIG_HOME` itself when that is set.
pub fn discover_config_dir(root: &Path) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u32>, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let version: Vec<u32> = name.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<u32>>>()?;
            Some((version, e.path()))
        })
        .filter(|(_, p)| TableKind::ALL.iter().any(|k| p.join(k.file_name()).is_file()) || p.join("kicad_common.json").is_file())
        .collect();
    found.sort();
    found.pop().map(|(_, p)| p)
}

// ------------------------------------------------------------------------------------------------ loading

/// Which table a row came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Global,
    Project,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Project => "project",
        }
    }
}

/// A table read from a file, with the tables its `Table` rows chain in (`LIBRARY_MANAGER::loadTables`, `loadNestedTables`).
#[derive(Clone, Debug)]
pub struct LoadedTable {
    pub path: PathBuf,
    pub scope: Scope,
    pub table: Table,
    /// One per row, in order: whether it is usable, and a chained table's own rows.
    pub rows: Vec<LoadedRow>,
}

#[derive(Clone, Debug, Default)]
pub struct LoadedRow {
    pub ok: bool,
    pub error: String,
    /// The table a `Table` row names, when it could be read.
    pub child: Option<Box<LoadedTable>>,
}

/// The file a path names, tidied the way KiCad does before it compares tables (`WX_FILENAME::ResolvePossibleSymlinks`).
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl LoadedTable {
    /// Read `path` (and, below it, every table it chains). The error is KiCad's wording: the path does not exist, or the syntax error.
    pub fn load(path: &Path, kind: TableKind, scope: Scope, vars: &Vars) -> Result<LoadedTable, String> {
        let mut seen = HashSet::new();
        Self::load_inner(path, kind, scope, vars, &mut seen)
    }

    fn load_inner(path: &Path, kind: TableKind, scope: Scope, vars: &Vars, seen: &mut HashSet<PathBuf>) -> Result<LoadedTable, String> {
        let path = resolved(path);
        if !path.is_file() {
            return Err(format!("The library table path '{}' does not exist", path.display()));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let table = Table::parse(&text).map_err(|e| e.message)?;
        if table.kind != kind {
            return Err(format!("Library table '{}' has type {} but expected {}; skipping", path.display(), table.kind.keyword(), kind.keyword()));
        }
        seen.insert(path.clone());
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut rows = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            if !row.is_table() {
                rows.push(LoadedRow { ok: true, ..Default::default() });
                continue;
            }
            // the chained table's path is relative to the table that names it
            let file = resolved(&vars.expand_uri(&row.uri, &folder));
            if seen.contains(&file) {
                rows.push(LoadedRow { ok: false, error: "A reference to this library table already exists".into(), child: None });
                continue;
            }
            match Self::load_inner(&file, kind, scope, vars, seen) {
                Ok(child) => rows.push(LoadedRow { ok: true, error: String::new(), child: Some(Box::new(child)) }),
                Err(e) => rows.push(LoadedRow { ok: false, error: e, child: None }),
            }
        }
        Ok(LoadedTable { path, scope, table, rows })
    }
}

/// Why a library cannot be read, in KiCad's words (`LIBRARY_MANAGER_ADAPTER::createPlugin` and the library readers' own messages), or `None` when it can.
/// `path` is the row's URI with its variables expanded.
pub fn check_row(kind: TableKind, row: &Row, path: &Path) -> Option<String> {
    if row.is_table() {
        return (!path.is_file()).then(|| format!("Nested table '{}' not found.", row.uri));
    }
    if !known_types(kind).contains(&row.lib_type.as_str()) {
        return Some(format!("Unknown library type {} ", row.lib_type));
    }
    if row.lib_type != KICAD_TYPE {
        return Some(format!("The studio reads KiCad-format libraries only; a '{}' library is not read here.", row.lib_type));
    }
    match kind {
        TableKind::Symbol => {
            if path.is_dir() {
                Some(format!("'{}' is a folder of symbol files, which the studio does not read; add the .kicad_sym files themselves.", path.display()))
            } else if !path.is_file() {
                Some(format!("Library '{}' not found.", path.display()))
            } else {
                None
            }
        }
        TableKind::Footprint => (!path.is_dir()).then(|| format!("Footprint library '{}' not found.", path.display())),
    }
}

/// A library a table defines, resolved: where it is and whether it can be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lib {
    pub nickname: String,
    /// The path as the row writes it.
    pub uri: String,
    /// The file (symbols) or folder (footprints), variables expanded.
    pub path: PathBuf,
    pub lib_type: String,
    pub options: String,
    pub description: String,
    pub scope: Scope,
    pub disabled: bool,
    pub hidden: bool,
    /// `None` when the library can be read; else why not.
    pub error: Option<String>,
    /// The table file that holds the row.
    pub table: PathBuf,
}

impl Lib {
    /// Found, readable and switched on: what the trees, the choosers and the lookups use.
    pub fn is_available(&self) -> bool {
        !self.disabled && self.error.is_none()
    }

    /// Listed in the trees and choosers (`!Hidden()` as well).
    pub fn is_listed(&self) -> bool {
        self.is_available() && !self.hidden
    }
}

/// `LIBRARY_MANAGER::Rows( type, BOTH, false )`: the libraries of the tables in the order they are read -- each table's rows in turn, a chained table in the place of its
/// row, a disabled chained table skipped, a row of a hidden chained table hidden -- with a nickname defined again replaced by its later row (it keeps its first place).
pub fn effective_libs(kind: TableKind, tables: &[&LoadedTable], vars: &Vars) -> Vec<Lib> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: BTreeMap<String, Lib> = BTreeMap::new();
    fn walk(kind: TableKind, t: &LoadedTable, parent_hidden: bool, vars: &Vars, order: &mut Vec<String>, by_name: &mut BTreeMap<String, Lib>) {
        let folder = t.path.parent().map(Path::to_path_buf).unwrap_or_default();
        for (row, loaded) in t.table.rows.iter().zip(&t.rows) {
            if !loaded.ok {
                continue;
            }
            if row.is_table() {
                if row.disabled {
                    continue;
                }
                if let Some(child) = &loaded.child {
                    walk(kind, child, parent_hidden || row.hidden, vars, order, by_name);
                }
                continue;
            }
            let path = vars.expand_uri(&row.uri, &folder);
            let lib = Lib {
                nickname: row.nickname.clone(),
                uri: row.uri.clone(),
                error: check_row(kind, row, &path),
                path,
                lib_type: row.lib_type.clone(),
                options: row.options.clone(),
                description: row.description.clone(),
                scope: t.scope,
                disabled: row.disabled,
                hidden: row.hidden || parent_hidden,
                table: t.path.clone(),
            };
            if !by_name.contains_key(&lib.nickname) {
                order.push(lib.nickname.clone());
            }
            by_name.insert(lib.nickname.clone(), lib);
        }
    }
    for t in tables {
        walk(kind, t, false, vars, &mut order, &mut by_name);
    }
    order.into_iter().filter_map(|n| by_name.remove(&n)).collect()
}

// ------------------------------------------------------------------------------------------------ the dialog's checks

/// One thing `VerifyTable` refused, with the row it stopped at and the column to put the cursor in (`nickname`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableProblem {
    pub row: usize,
    pub column: &'static str,
    pub message: String,
}

/// `LIB_TABLE_GRID_TRICKS::VerifyTable`: what the dialog does when OK is pressed, on the rows as edited. A row with no path is dropped without a word; a row with no
/// nickname, or one with a character a nickname may not have, stops the dialog; so do two rows with one nickname (the later is the one reported). The trimmed values are
/// what is kept, and a table without a Show column (footprints) does not keep a hidden flag. Returns the cleaned rows, or the first problem.
pub fn verify_rows(rows: &[Row], supports_visibility: bool) -> Result<Vec<Row>, TableProblem> {
    let mut kept: Vec<Row> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let (nick, uri) = (row.nickname.trim().to_string(), row.uri.trim().to_string());
        if uri.is_empty() {
            continue;
        }
        if nick.is_empty() {
            return Err(TableProblem { row: index, column: "nickname", message: "Library must have a nickname.".into() });
        }
        if let Some(ch) = illegal_nickname_char(&nick) {
            return Err(TableProblem { row: index, column: "nickname", message: format!("Illegal character '{ch}' in nickname '{nick}'.") });
        }
        let mut row = row.clone();
        row.nickname = nick;
        row.uri = uri;
        if !supports_visibility {
            row.hidden = false;
        }
        kept.push(row);
    }
    for (i, a) in kept.iter().enumerate() {
        if let Some((j, _)) = kept.iter().enumerate().skip(i + 1).find(|(_, b)| b.nickname == a.nickname) {
            // the position reported is among the rows the person sees, whichever of them were dropped
            let shown = rows.iter().enumerate().filter(|(_, r)| !r.uri.trim().is_empty()).map(|(k, _)| k).nth(j).unwrap_or(j);
            return Err(TableProblem { row: shown, column: "nickname", message: format!("Multiple libraries cannot share the same nickname ('{}').", a.nickname) });
        }
    }
    Ok(kept)
}

/// A table file for the KiCad the derived project is given to: `${KIPRJMOD}` replaced by the project's folder (the derived project lives in a folder of its own, so the
/// variable would otherwise name that), and a relative path made absolute against the project's folder.
pub fn derived_for_kicad(table: &Table, project_dir: &Path) -> Table {
    let project = project_dir.display().to_string();
    let rows = table
        .rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            let mut uri = row.uri.replace("${KIPRJMOD}", &project).replace("$(KIPRJMOD)", &project);
            if !uri.contains("${") && !uri.contains("$(") && !uri.is_empty() && !Path::new(&uri).is_absolute() {
                uri = absolute(Path::new(&uri), project_dir).display().to_string();
            }
            row.uri = uri;
            row
        })
        .collect();
    Table { kind: table.kind, version: Some(TABLE_VERSION), rows }
}

// ------------------------------------------------------------------------------------------------ a project's own path variables

/// The file a project keeps its own path variables in, beside its library tables. KiCad keeps a project's variables in the project file (its text
/// variables, which `PROJECT::TextVarResolver` reads before the environment, so `${NAME}` works in a library path); the studio's derived project file is
/// rewritten on every run, so they live here and the derived file is given them.
pub const PATH_VARIABLES_FILE: &str = "path-variables.json";

/// The variables of the project in `dir`: `{"version": 1, "variables": {"NAME": "value"}}`. None when there is no such file, or it is not one.
pub fn read_project_variables(dir: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(dir.join(PATH_VARIABLES_FILE)) else { return BTreeMap::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return BTreeMap::new() };
    v.get("variables").and_then(|m| m.as_object()).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
}

/// Store the project's variables (an empty set removes the file). Through a temporary file and a rename.
pub fn write_project_variables(dir: &Path, vars: &BTreeMap<String, String>) -> Result<(), String> {
    let path = dir.join(PATH_VARIABLES_FILE);
    if vars.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("could not remove {}: {e}", path.display())),
        };
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "variables": vars })).unwrap_or_default() + "\n";
    let tmp = dir.join(format!("{PATH_VARIABLES_FILE}.tmp"));
    std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write {}: {e}", path.display())
    })
}

// ------------------------------------------------------------------------------------------------ the derived project

/// Give the derived KiCad project of a board the board's library setup, so kicad-cli reads the same libraries the studio does. `board_dir` is the board's folder (the
/// studio's project folder) and `work` the folder of the derived project, `<board>/.kicad`, whose `<stem>.kicad_pro` is `pro`:
///
///   * the board's `sym-lib-table` and `fp-lib-table` are written into `work` ([`derived_for_kicad`]: `${KIPRJMOD}` named the real folder); a table the board lacks, or that does
///     not parse, leaves none there. KiCad reads the global tables from its own configuration, as it does for any project;
///   * the board's path variables go into the project file as its text variables, which `PROJECT::TextVarResolver` reads before the environment, with `KIPRJMOD` itself named the
///     board's folder -- the derived project lives in a folder of its own, and a model path or a library path written with `${KIPRJMOD}` would otherwise lead into it.
///
/// A board with neither tables nor variables is left as it was: no table files, no `text_variables`. Returns whether there was anything to derive.
pub fn derive_project(board_dir: &Path, work: &Path, pro: &mut serde_json::Value) -> Result<bool, String> {
    let mut any = false;
    for kind in TableKind::ALL {
        let dst = work.join(kind.file_name());
        let table = std::fs::read_to_string(board_dir.join(kind.file_name())).ok().and_then(|t| Table::parse(&t).ok()).filter(|t| t.kind == kind);
        match table {
            Some(t) => {
                write_table(&dst, &derived_for_kicad(&t, board_dir))?;
                any = true;
            }
            None => {
                let _ = std::fs::remove_file(&dst);
            }
        }
    }
    // The design's Text Variables page (`export_kicad_pro_for` put it in `pro`) is the project's whole set once it has been used; the board folder's file is the seed
    // it started from and no longer says anything (`eda_model::rules::effective_text_variables`).
    let from_page = pro.get("text_variables").is_some_and(serde_json::Value::is_object);
    let vars = if from_page {
        pro.get("text_variables").and_then(|v| v.as_object()).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
    } else {
        read_project_variables(board_dir)
    };
    any |= from_page || !vars.is_empty();
    if any {
        let folder = board_dir.display().to_string();
        let mut text = pro.get("text_variables").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        text.insert("KIPRJMOD".into(), serde_json::json!(folder));
        for (name, value) in &vars {
            text.insert(name.clone(), serde_json::json!(value.replace("${KIPRJMOD}", &folder).replace("$(KIPRJMOD)", &folder)));
        }
        pro["text_variables"] = serde_json::Value::Object(text);
    }
    Ok(any)
}

// ------------------------------------------------------------------------------------------------ importing a KiCad project

/// What [`import_project_setup`] carried over.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportedSetup {
    /// The table files written into the board's folder.
    pub tables: Vec<&'static str>,
    /// How many of the project's variables became the board's path variables.
    pub variables: usize,
    /// What was left alone, and why: a table that does not parse, a file the board's folder already has.
    pub notes: Vec<String>,
}

/// A real KiCad project keeps its library tables beside the board and its own variables in the project file's `text_variables`; `eda import-kicad` brings both along, so the
/// libraries of the project still resolve from the new board folder. `${KIPRJMOD}` in them named the project's folder, so it is written out as `source_dir` (a path relative to
/// the project is made absolute against it, [`derived_for_kicad`]). A table or a variables file the board's folder already has is kept, and importing into the project's own
/// folder carries nothing: its setup already is the board's. Every text variable the project has comes along, not only the ones named like a path variable (Board Setup >
/// Text Variables edits them all, Configure Paths the ones it can list); only a name the Text Variables page would refuse is left behind, and `KIPRJMOD` always.
pub fn import_project_setup(source_dir: &Path, board_dir: &Path, project_file: Option<&str>) -> ImportedSetup {
    let mut done = ImportedSetup::default();
    let same = |a: &Path, b: &Path| std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(a, b)| a == b);
    if same(source_dir, board_dir) {
        return done;
    }
    for kind in TableKind::ALL {
        let name = kind.file_name();
        let Ok(text) = std::fs::read_to_string(source_dir.join(name)) else { continue };
        let table = match Table::parse(&text) {
            Ok(t) if t.kind == kind => t,
            Ok(_) => {
                done.notes.push(format!("{name}: not a {} library table, left behind", kind.name()));
                continue;
            }
            Err(e) => {
                done.notes.push(format!("{name}: {e}, left behind"));
                continue;
            }
        };
        if board_dir.join(name).exists() {
            done.notes.push(format!("{name}: the board's folder has one already, kept"));
            continue;
        }
        match write_table(&board_dir.join(name), &derived_for_kicad(&table, source_dir)) {
            Ok(()) => done.tables.push(name),
            Err(e) => done.notes.push(e),
        }
    }
    let taken = project_file.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).and_then(|v| v.get("text_variables")?.as_object().cloned()).unwrap_or_default();
    let folder = source_dir.display().to_string();
    let vars: BTreeMap<String, String> = taken
        .iter()
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.replace("${KIPRJMOD}", &folder).replace("$(KIPRJMOD)", &folder))))
        .filter(|(name, _)| eda_model::rules::text_variable_name_problem(name).is_none())
        .collect();
    if !vars.is_empty() {
        if board_dir.join(PATH_VARIABLES_FILE).exists() {
            done.notes.push(format!("{PATH_VARIABLES_FILE}: the board's folder has one already, kept"));
        } else {
            match write_project_variables(board_dir, &vars) {
                Ok(()) => done.variables = vars.len(),
                Err(e) => done.notes.push(e),
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    const KICAD_GLOBAL: &str = "(sym_lib_table\n\t(version 7)\n\t(lib (name \"KiCad\") (type \"Table\") (uri \"/Applications/KiCad/KiCad.app/Contents/SharedSupport/template/sym-lib-table\") (options \"\") (descr \"KiCad Default Libraries\"))\n)\n";

    fn row(nick: &str, uri: &str) -> Row {
        Row { nickname: nick.into(), uri: uri.into(), lib_type: KICAD_TYPE.into(), ..Default::default() }
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-table-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn importing_a_kicad_project_brings_its_tables_and_variables_with_the_folder_written_out() {
        let root = temp("import");
        let (project, board) = (root.join("project"), root.join("board"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&board).unwrap();
        std::fs::write(
            project.join("sym-lib-table"),
            "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/libs/Mine.kicad_sym\") (options \"\") (descr \"\"))\n\t(lib (name \"Theirs\") (type \"KiCad\") (uri \"${THEIRS}/T.kicad_sym\") (options \"\") (descr \"\") (hidden))\n)\n",
        )
        .unwrap();
        // a footprint table that is really a symbol table, and one that does not parse
        std::fs::write(project.join("fp-lib-table"), "(sym_lib_table (version 7))\n").unwrap();
        let pro = r#"{ "text_variables": { "THEIRS": "${KIPRJMOD}/theirs", "REVISION": "B", "Not a name": "x", "KIPRJMOD": "/nope", "COUNT": 3 } }"#;
        let done = import_project_setup(&project, &board, Some(pro));
        assert_eq!(done.tables, vec!["sym-lib-table"]);
        assert_eq!(done.notes.len(), 1, "{:?}", done.notes);
        assert!(done.notes[0].starts_with("fp-lib-table: not a footprint library table"), "{:?}", done.notes);
        let table = Table::parse(&std::fs::read_to_string(board.join("sym-lib-table")).unwrap()).unwrap();
        let folder = project.display().to_string();
        assert_eq!(table.rows[0].uri, format!("{folder}/libs/Mine.kicad_sym"), "the project's folder, not the board's");
        assert_eq!(table.rows[1].uri, "${THEIRS}/T.kicad_sym", "another variable stays a variable");
        assert!(table.rows[1].hidden, "the flags come along");
        assert!(!board.join("fp-lib-table").exists());
        // every text variable with a name the Text Variables page takes, never KIPRJMOD; the folder written out in a value
        let vars = read_project_variables(&board);
        assert_eq!(vars.keys().map(String::as_str).collect::<Vec<_>>(), vec!["Not a name", "REVISION", "THEIRS"]);
        assert_eq!(vars["THEIRS"], format!("{folder}/theirs"));
        assert_eq!(done.variables, 3);
        // a second import into the same board keeps what is there; the project's own folder carries nothing
        let again = import_project_setup(&project, &board, Some(pro));
        assert_eq!((again.tables.len(), again.variables, again.notes.len()), (0, 0, 3), "{:?}", again.notes);
        assert_eq!(import_project_setup(&project, &project, Some(pro)), ImportedSetup::default());
        // a project without tables or variables brings nothing
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(import_project_setup(&empty, &root.join("elsewhere"), None), ImportedSetup::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn kicads_own_global_table_parses_and_writes_back_byte_for_byte() {
        let t = Table::parse(KICAD_GLOBAL).unwrap();
        assert_eq!((t.kind, t.version, t.rows.len()), (TableKind::Symbol, Some(7), 1));
        assert_eq!((t.rows[0].nickname.as_str(), t.rows[0].lib_type.as_str(), t.rows[0].description.as_str()), ("KiCad", "Table", "KiCad Default Libraries"));
        assert!(t.rows[0].is_table() && !t.rows[0].disabled && !t.rows[0].hidden);
        assert_eq!(t.format(), KICAD_GLOBAL, "the writer is KiCad's: tab indents, a line to a lib, version 7");
    }

    #[test]
    fn a_table_round_trips_with_every_field_and_both_flags() {
        let mut t = Table::new(TableKind::Footprint);
        t.rows.push(Row { nickname: "Mine".into(), uri: "${KIPRJMOD}/libs/Mine.pretty".into(), lib_type: "KiCad".into(), options: "a=b|c".into(), description: "My parts (v2)".into(), disabled: true, hidden: true });
        t.rows.push(row("Other", "/abs/Other.pretty"));
        let text = t.format();
        assert!(text.starts_with("(fp_lib_table\n\t(version 7)\n\t(lib (name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/libs/Mine.pretty\") (options \"a=b|c\") (descr \"My parts (v2)\") (disabled) (hidden))\n"), "{text}");
        assert_eq!(Table::parse(&text).unwrap(), t);
        // and the second generation is the same text
        assert_eq!(Table::parse(&text).unwrap().format(), text);
    }

    #[test]
    fn the_reader_takes_what_kicad_wrote_in_older_forms() {
        // no version, bare tokens, properties in another order, flags first
        let t = Table::parse("(fp_lib_table (lib (name Mine)(type KiCad)(uri ${KIPRJMOD}/m.pretty)(options \"\")(descr \"\"))\n (lib (hidden) (disabled) (descr \"x\") (uri \"/a\") (name \"B\") (type \"Legacy\")))").unwrap();
        assert_eq!(t.version, None);
        assert_eq!(t.rows.len(), 2);
        assert_eq!((t.rows[0].nickname.as_str(), t.rows[0].uri.as_str()), ("Mine", "${KIPRJMOD}/m.pretty"));
        assert!(t.rows[1].hidden && t.rows[1].disabled);
        assert_eq!(t.rows[1].lib_type, "Legacy");
        // a quoted text holds blanks and parentheses
        let t = Table::parse("(sym_lib_table (version 7) (lib (name \"A B\") (uri \"/x (y)/z.kicad_sym\") (type \"KiCad\")))").unwrap();
        assert_eq!((t.rows[0].nickname.as_str(), t.rows[0].uri.as_str()), ("A B", "/x (y)/z.kicad_sym"));
        // an empty table
        assert_eq!(Table::parse("(sym_lib_table\n\t(version 7)\n)\n").unwrap().rows.len(), 0);
    }

    #[test]
    fn a_table_that_is_not_one_is_a_syntax_error_with_its_place() {
        for bad in ["", "(kicad_pcb)", "(sym_lib_table (lib))", "(sym_lib_table (lib (name \"A\") (foo \"x\")))", "(sym_lib_table (lib (name \"A\")", "(sym_lib_table (lib (name \"A)))", "(sym_lib_table) trailing", "(sym_lib_table (nope))", "(design_block_lib_table (version 7))"] {
            let e = Table::parse(bad).unwrap_err();
            assert!(e.message.starts_with("Syntax error at line 1, column "), "{bad:?}: {e:?}");
        }
        let e = Table::parse("(sym_lib_table\n\t(version 7)\n\t(lib (name \"A\") (bogus \"B\"))\n)").unwrap_err();
        assert_eq!((e.line, e.column), (3, 19), "the unknown keyword: {e:?}");
    }

    #[test]
    fn what_the_writer_cannot_say_it_changes_rather_than_write_a_table_kicad_cannot_read() {
        let mut t = Table::new(TableKind::Symbol);
        t.rows.push(Row { nickname: "N".into(), uri: "C:\\libs\\x.kicad_sym".into(), lib_type: "KiCad".into(), description: "say \"hi\"\nthere".into(), ..Default::default() });
        let back = Table::parse(&t.format()).unwrap();
        assert_eq!(back.rows[0].uri, "C:/libs/x.kicad_sym", "a path is written with forward slashes, like KiCad's");
        assert_eq!(back.rows[0].description, "say 'hi' there");
    }

    #[test]
    fn nicknames_follow_lib_id() {
        assert_eq!(illegal_nickname_char("Good_Name-1 (v2)"), None);
        assert_eq!(illegal_nickname_char("a:b"), Some(':'));
        assert_eq!(illegal_nickname_char("a\\b"), Some('\\'));
        assert_eq!(illegal_nickname_char("a\tb"), Some('\t'));
        assert_eq!(fix_nickname("my:lib\\v2"), "my_lib_v2");
        assert_eq!(fix_nickname("Plain.name"), "Plain.name");
    }

    fn vars(text: &[(&str, &str)], env: &[(&str, &str)]) -> Vars {
        Vars { text: text.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), env: env.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), predefined: predefined_names(10) }
    }

    #[test]
    fn variables_expand_in_all_three_spellings_and_unknown_ones_stay() {
        let v = vars(&[("MYLIBS", "/home/me/libs")], &[("KIPRJMOD", "/work/board"), ("KICAD10_SYMBOL_DIR", "/kicad/symbols"), ("EMPTY", "")]);
        assert_eq!(v.expand("${KIPRJMOD}/a.kicad_sym"), "/work/board/a.kicad_sym");
        assert_eq!(v.expand("$(KIPRJMOD)/a"), "/work/board/a");
        assert_eq!(v.expand("$KIPRJMOD/a"), "/work/board/a");
        assert_eq!(v.expand("${MYLIBS}/x/${KICAD10_SYMBOL_DIR}"), "/home/me/libs/x//kicad/symbols");
        assert_eq!(v.expand("${NOPE}/x"), "${NOPE}/x", "a variable nothing defines is left as written");
        assert_eq!(v.expand("$(NOPE)/x"), "$(NOPE)/x");
        assert_eq!(v.expand("a${EMPTY}b"), "ab", "a variable set to nothing is nothing");
        assert_eq!(v.expand("100\\$ and \\%"), "100$ and %", "a backslash suppresses a dollar");
        assert_eq!(v.expand("no variables"), "no variables");
        assert_eq!(v.expand("${KIPRJMOD"), "/work/board", "an unclosed bracket is a warning in KiCad and the variable is still replaced");
    }

    #[test]
    fn the_projects_own_variables_win_over_the_environment() {
        let v = vars(&[("KIPRJMOD", "/override"), ("X", "text")], &[("KIPRJMOD", "/work/board"), ("X", "env"), ("Y", "env")]);
        assert_eq!(v.expand("${KIPRJMOD}|${X}|${Y}"), "/override|text|env");
    }

    #[test]
    fn a_variable_may_name_another_and_a_loop_ends() {
        let v = vars(&[("A", "${B}/a"), ("B", "${C}/b"), ("C", "/c")], &[]);
        assert_eq!(v.expand("${A}/x"), "/c/b/a/x");
        let looped = vars(&[("A", "${B}"), ("B", "${A}")], &[]);
        let r = looped.expand("${A}");
        assert!(r == "${A}" || r == "${B}", "{r}");
    }

    #[test]
    fn an_old_versioned_variable_reads_the_one_that_is_defined() {
        let v = vars(&[], &[("KICAD10_SYMBOL_DIR", "/kicad/symbols"), ("KICAD10_FOOTPRINT_DIR", "/kicad/footprints"), ("KICAD10_3DMODEL_DIR", "/kicad/3d")]);
        assert_eq!(v.expand("${KICAD8_SYMBOL_DIR}/Device.kicad_sym"), "/kicad/symbols/Device.kicad_sym");
        assert_eq!(v.expand("${KICAD6_FOOTPRINT_DIR}/X.pretty"), "/kicad/footprints/X.pretty");
        assert_eq!(v.expand("${KISYS3DMOD}/X.3dshapes"), "/kicad/3d/X.3dshapes");
        assert_eq!(v.expand("${KICAD9_3DMODEL_DIR}/a"), "/kicad/3d/a");
        let none = vars(&[], &[]);
        assert_eq!(none.expand("${KICAD8_SYMBOL_DIR}/x"), "${KICAD8_SYMBOL_DIR}/x", "defined by no version: left as written");
    }

    #[test]
    fn a_uri_is_made_absolute_against_its_folder_and_tidied() {
        let v = vars(&[], &[("KIPRJMOD", "/work/board")]);
        assert_eq!(v.expand_uri("${KIPRJMOD}/libs/../libs/a.kicad_sym", Path::new("/elsewhere")), PathBuf::from("/work/board/libs/a.kicad_sym"));
        assert_eq!(v.expand_uri("libs/a.kicad_sym", Path::new("/proj")), PathBuf::from("/proj/libs/a.kicad_sym"));
        assert_eq!(v.expand_uri("../t/sym-lib-table", Path::new("/proj/sub")), PathBuf::from("/proj/t/sym-lib-table"));
        assert_eq!(v.expand_uri("/abs/./x", Path::new("/proj")), PathBuf::from("/abs/x"));
    }

    #[test]
    fn variable_names_are_capitals_digits_and_underscores() {
        assert_eq!(fix_variable_name("my_libs-2 v3"), "MY_LIBS2V3");
        assert_eq!(fix_variable_name("9lives"), "LIVES", "no digit first");
        assert_eq!(fix_variable_name("A9"), "A9");
        assert!(is_variable_name("KICAD10_SYMBOL_DIR") && is_variable_name("_X") && !is_variable_name("lower") && !is_variable_name("9X") && !is_variable_name("") && !is_variable_name("A B"));
    }

    #[test]
    fn variables_used_lists_each_name_once() {
        assert_eq!(variables_used("${A}/x/${B}/$(C)/${A}"), vec!["A", "B", "C"]);
        assert_eq!(variables_used("/plain/path"), Vec::<String>::new());
        assert_eq!(variables_used("${unclosed"), Vec::<String>::new());
    }

    #[test]
    fn a_path_is_written_with_the_deepest_variable_that_holds_it_else_the_project() {
        let root = temp("normalize");
        let (lib_dir, deep) = (root.join("libs"), root.join("libs").join("deep"));
        std::fs::create_dir_all(&deep).unwrap();
        let vars = vec![("SHALLOW".to_string(), root.display().to_string()), ("DEEP".to_string(), lib_dir.display().to_string()), ("GONE".to_string(), root.join("nowhere").display().to_string())];
        assert_eq!(normalize_path(&deep.join("x.kicad_sym"), &vars, None), "${DEEP}/deep/x.kicad_sym");
        assert_eq!(normalize_path(&lib_dir.join("y.kicad_sym"), &vars, None), "${DEEP}/y.kicad_sym", "straight in the folder of the variable");
        let project = root.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        assert_eq!(normalize_path(&project.join("libs").join("z.pretty"), &[], Some(&project)), "${KIPRJMOD}/libs/z.pretty");
        assert_eq!(normalize_path(Path::new("/somewhere/else/q.kicad_sym"), &vars, Some(&project)), "/somewhere/else/q.kicad_sym");
        // a variable wins over the project when it holds the file
        assert_eq!(normalize_path(&project.join("w.kicad_sym"), &[("P".to_string(), project.display().to_string())], Some(&project)), "${P}/w.kicad_sym");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn kicads_environment_is_the_defaults_then_the_config_then_the_real_environment() {
        let stock = Stock::new(PathBuf::from("/K/SharedSupport/symbols"), PathBuf::from("/K/SharedSupport/footprints"), PathBuf::from("/K/SharedSupport/3dmodels"), Path::new("/home/me/Documents/KiCad/10.99"));
        assert_eq!(stock.templates, PathBuf::from("/K/SharedSupport/template"));
        let configured: BTreeMap<String, String> = [("KICAD10_FOOTPRINT_DIR".to_string(), "/mine/fp".to_string()), ("MYLIBS".to_string(), "/mine/libs".to_string())].into();
        let real: BTreeMap<String, String> = [("KICAD10_3DMODEL_DIR".to_string(), "/ext/3d".to_string())].into();
        let vars = global_env_vars(10, &stock, &configured, &real);
        let get = |n: &str| vars.iter().find(|v| v.name == n).unwrap_or_else(|| panic!("no {n}"));
        assert_eq!((get("KICAD10_SYMBOL_DIR").value.as_str(), get("KICAD10_SYMBOL_DIR").default, get("KICAD10_SYMBOL_DIR").external), ("/K/SharedSupport/symbols", true, false));
        assert_eq!((get("KICAD10_FOOTPRINT_DIR").value.as_str(), get("KICAD10_FOOTPRINT_DIR").default), ("/mine/fp", false), "the configured value replaces the default");
        assert_eq!((get("KICAD10_3DMODEL_DIR").value.as_str(), get("KICAD10_3DMODEL_DIR").external), ("/ext/3d", true), "the real environment wins and is marked");
        assert_eq!(get("MYLIBS").value, "/mine/libs");
        assert_eq!(get("KICAD10_3RD_PARTY").value, "/home/me/Documents/KiCad/10.99/3rdparty");
        assert_eq!(get("KICAD10_TEMPLATE_DIR").value, "/K/SharedSupport/template");
        assert!(is_predefined("KIPRJMOD", 10) && is_predefined("KICAD10_SYMBOL_DIR", 10) && !is_predefined("MYLIBS", 10) && !is_predefined("KICAD9_SYMBOL_DIR", 10));
    }

    #[test]
    fn the_configured_variables_come_from_kicad_commons_environment_section() {
        assert!(configured_env_vars(r#"{"environment": {"vars": null}}"#).is_empty());
        assert!(configured_env_vars("not json").is_empty());
        assert!(configured_env_vars("{}").is_empty());
        let m = configured_env_vars(r#"{"environment": {"vars": {"MYLIBS": "/x", "N": 3}}}"#);
        assert_eq!(m.get("MYLIBS").map(String::as_str), Some("/x"));
        assert!(!m.contains_key("N"));
        assert_eq!((major_of("10.99"), major_of("9.0"), major_of("nightly")), (Some(10), Some(9), None));
    }

    #[test]
    fn the_newest_configuration_folder_with_a_table_is_found() {
        let root = temp("config");
        for v in ["7.0", "9.0", "10.99", "notes"] {
            std::fs::create_dir_all(root.join(v)).unwrap();
        }
        std::fs::write(root.join("9.0").join("sym-lib-table"), "x").unwrap();
        std::fs::write(root.join("7.0").join("fp-lib-table"), "x").unwrap();
        assert_eq!(discover_config_dir(&root), Some(root.join("9.0")), "10.99 has no table or settings");
        std::fs::write(root.join("10.99").join("kicad_common.json"), "{}").unwrap();
        assert_eq!(discover_config_dir(&root), Some(root.join("10.99")));
        assert_eq!(discover_config_dir(&root.join("missing")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stock table beside a symbol folder, a global table chaining it, and a project table: the shape of a real install.
    fn tables(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, Vars) {
        let root = temp(tag);
        let (share, config, project) = (root.join("share"), root.join("config"), root.join("project"));
        for d in [share.join("symbols"), share.join("template"), config.clone(), project.clone()] {
            std::fs::create_dir_all(&d).unwrap();
        }
        for lib in ["Device", "Amp"] {
            std::fs::write(share.join("symbols").join(format!("{lib}.kicad_sym")), "(kicad_symbol_lib)").unwrap();
        }
        std::fs::write(
            share.join("template").join("sym-lib-table"),
            "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Device\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Device.kicad_sym\") (options \"\") (descr \"Passives\"))\n\t(lib (name \"Amp\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Amp.kicad_sym\") (options \"\") (descr \"Amplifiers\"))\n\t(lib (name \"Gone\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Gone.kicad_sym\") (options \"\") (descr \"\"))\n)\n",
        )
        .unwrap();
        std::fs::write(config.join("sym-lib-table"), format!("(sym_lib_table\n\t(version 7)\n\t(lib (name \"KiCad\") (type \"Table\") (uri \"{}\") (options \"\") (descr \"KiCad Default Libraries\"))\n)\n", share.join("template").join("sym-lib-table").display())).unwrap();
        let v = Vars { text: BTreeMap::new(), env: [("KICAD10_SYMBOL_DIR".to_string(), share.join("symbols").display().to_string()), ("KIPRJMOD".to_string(), project.display().to_string())].into(), predefined: predefined_names(10) };
        (root, share, config, project, v)
    }

    #[test]
    fn a_global_table_chains_kicads_and_the_project_table_adds_to_it() {
        let (root, _share, config, project, v) = tables("effective");
        std::fs::create_dir_all(project.join("libs")).unwrap();
        std::fs::write(project.join("libs").join("Mine.kicad_sym"), "(kicad_symbol_lib)").unwrap();
        // the project redefines Device and adds Mine; one has a variable, one is a plain relative path
        let mut pt = Table::new(TableKind::Symbol);
        pt.rows.push(Row { description: "My Device".into(), ..row("Device", "${KIPRJMOD}/libs/Mine.kicad_sym") });
        pt.rows.push(row("Mine", "libs/Mine.kicad_sym"));
        write_table(&project.join("sym-lib-table"), &pt).unwrap();

        let global = LoadedTable::load(&config.join("sym-lib-table"), TableKind::Symbol, Scope::Global, &v).unwrap();
        assert!(global.rows[0].ok && global.rows[0].child.is_some());
        assert_eq!(global.rows[0].child.as_ref().unwrap().table.rows.len(), 3);
        let proj = LoadedTable::load(&project.join("sym-lib-table"), TableKind::Symbol, Scope::Project, &v).unwrap();
        let libs = effective_libs(TableKind::Symbol, &[&global, &proj], &v);
        let names: Vec<&str> = libs.iter().map(|l| l.nickname.as_str()).collect();
        assert_eq!(names, ["Device", "Amp", "Gone", "Mine"], "the order of reading; Device keeps its first place");
        let device = &libs[0];
        assert_eq!((device.scope, device.description.as_str()), (Scope::Project, "My Device"), "the project's row replaced the global one");
        assert_eq!(device.path, project.join("libs").join("Mine.kicad_sym"));
        assert_eq!(libs[1].scope, Scope::Global);
        assert!(libs[1].is_available() && libs[1].path.ends_with("symbols/Amp.kicad_sym"), "{:?}", libs[1].path);
        assert!(libs[2].error.as_deref().is_some_and(|e| e.starts_with("Library '") && e.ends_with("Gone.kicad_sym' not found.")), "{:?}", libs[2].error);
        assert!(!libs[2].is_available());
        assert!(libs[3].is_available(), "a relative path is relative to the table's folder: {:?}", libs[3]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn disabled_and_hidden_rows_and_chained_tables_follow_the_manager() {
        let (root, _share, config, project, v) = tables("flags");
        // hide the chained KiCad table: its libraries are found but not listed; then disable it: they are gone
        let mut g = Table::parse(&std::fs::read_to_string(config.join("sym-lib-table")).unwrap()).unwrap();
        g.rows[0].hidden = true;
        std::fs::write(config.join("sym-lib-table"), g.format()).unwrap();
        let global = LoadedTable::load(&config.join("sym-lib-table"), TableKind::Symbol, Scope::Global, &v).unwrap();
        let libs = effective_libs(TableKind::Symbol, &[&global], &v);
        assert!(libs.iter().all(|l| l.hidden) && libs.iter().any(|l| l.is_available() && !l.is_listed()));
        g.rows[0].disabled = true;
        std::fs::write(config.join("sym-lib-table"), g.format()).unwrap();
        let global = LoadedTable::load(&config.join("sym-lib-table"), TableKind::Symbol, Scope::Global, &v).unwrap();
        assert!(effective_libs(TableKind::Symbol, &[&global], &v).is_empty(), "a disabled chained table contributes nothing");
        // a disabled library of the project is still a row, but not available
        let mut pt = Table::new(TableKind::Symbol);
        pt.rows.push(Row { disabled: true, ..row("Off", "${KICAD10_SYMBOL_DIR}/Device.kicad_sym") });
        write_table(&project.join("sym-lib-table"), &pt).unwrap();
        let proj = LoadedTable::load(&project.join("sym-lib-table"), TableKind::Symbol, Scope::Project, &v).unwrap();
        let libs = effective_libs(TableKind::Symbol, &[&proj], &v);
        assert_eq!((libs.len(), libs[0].is_available(), libs[0].error.clone()), (1, false, None));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_nested_table_a_loop_and_a_wrong_kind_are_errors_of_their_row_or_table() {
        let (root, share, config, _project, v) = tables("errors");
        let own = config.join("sym-lib-table");
        // a table that chains itself and one that is not there
        std::fs::write(&own, format!("(sym_lib_table (version 7)\n (lib (name \"Self\") (type \"Table\") (uri \"{}\") (options \"\") (descr \"\"))\n (lib (name \"Nope\") (type \"Table\") (uri \"/no/such/table\") (options \"\") (descr \"\"))\n (lib (name \"Ok\") (type \"Table\") (uri \"{}\") (options \"\") (descr \"\")))", own.display(), share.join("template").join("sym-lib-table").display())).unwrap();
        let t = LoadedTable::load(&own, TableKind::Symbol, Scope::Global, &v).unwrap();
        assert_eq!(t.rows[0].error, "A reference to this library table already exists");
        assert!(t.rows[1].error.starts_with("The library table path '/no/such/table' does not exist"), "{}", t.rows[1].error);
        assert!(t.rows[2].ok);
        assert_eq!(effective_libs(TableKind::Symbol, &[&t], &v).len(), 3, "only the good chained table's rows");
        // a footprint table is not a symbol table
        let e = LoadedTable::load(&own, TableKind::Footprint, Scope::Global, &v).unwrap_err();
        assert!(e.contains("has type sym_lib_table but expected fp_lib_table"), "{e}");
        assert!(LoadedTable::load(&root.join("none"), TableKind::Symbol, Scope::Project, &v).unwrap_err().contains("does not exist"));
        std::fs::write(root.join("bad"), "(sym_lib_table (oops))").unwrap();
        assert!(LoadedTable::load(&root.join("bad"), TableKind::Symbol, Scope::Project, &v).unwrap_err().starts_with("Syntax error at line 1"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_row_says_why_its_library_cannot_be_read() {
        let root = temp("check");
        std::fs::write(root.join("a.kicad_sym"), "(kicad_symbol_lib)").unwrap();
        std::fs::create_dir_all(root.join("b.pretty")).unwrap();
        std::fs::create_dir_all(root.join("folder")).unwrap();
        let sym = |lib_type: &str| Row { lib_type: lib_type.into(), uri: "u".into(), nickname: "N".into(), ..Default::default() };
        assert_eq!(check_row(TableKind::Symbol, &sym("KiCad"), &root.join("a.kicad_sym")), None);
        assert_eq!(check_row(TableKind::Footprint, &sym("KiCad"), &root.join("b.pretty")), None);
        assert!(check_row(TableKind::Symbol, &sym("KiCad"), &root.join("zz.kicad_sym")).unwrap().ends_with("zz.kicad_sym' not found."));
        assert!(check_row(TableKind::Footprint, &sym("KiCad"), &root.join("zz.pretty")).unwrap().starts_with("Footprint library '"));
        assert!(check_row(TableKind::Symbol, &sym("KiCad"), &root.join("folder")).unwrap().contains("folder of symbol files"));
        assert!(check_row(TableKind::Footprint, &sym("KiCad"), &root.join("a.kicad_sym")).is_some(), "a footprint library is a folder");
        assert_eq!(check_row(TableKind::Symbol, &sym("Bogus"), &root).unwrap(), "Unknown library type Bogus ");
        assert!(check_row(TableKind::Symbol, &sym("Legacy"), &root).unwrap().contains("KiCad-format libraries only"));
        assert!(check_row(TableKind::Symbol, &sym("Table"), &root.join("none")).unwrap().starts_with("Nested table 'u' not found"));
        assert_eq!(check_row(TableKind::Symbol, &sym("Table"), &root.join("a.kicad_sym")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_drops_empty_paths_and_stops_at_a_bad_nickname_or_a_duplicate() {
        let ok = verify_rows(&[row(" A ", " /a.kicad_sym "), row("", ""), row("B", "/b.kicad_sym")], true).unwrap();
        assert_eq!(ok.iter().map(|r| (r.nickname.as_str(), r.uri.as_str())).collect::<Vec<_>>(), [("A", "/a.kicad_sym"), ("B", "/b.kicad_sym")], "trimmed, and the row with no path is gone without a word");
        let p = verify_rows(&[row("A", "/a"), row("", "/b")], true).unwrap_err();
        assert_eq!((p.row, p.column, p.message.as_str()), (1, "nickname", "Library must have a nickname."));
        let p = verify_rows(&[row("A:B", "/a")], true).unwrap_err();
        assert_eq!(p.message, "Illegal character ':' in nickname 'A:B'.");
        let p = verify_rows(&[row("A", "/a"), row("Z", ""), row("B", "/b"), row("A", "/c")], true).unwrap_err();
        assert_eq!((p.row, p.message.as_str()), (3, "Multiple libraries cannot share the same nickname ('A')."), "the later of the two, counted among the rows as shown");
        // a table without a Show column keeps no hidden flag
        let r = Row { hidden: true, ..row("A", "/a") };
        assert!(!verify_rows(std::slice::from_ref(&r), false).unwrap()[0].hidden);
        assert!(verify_rows(std::slice::from_ref(&r), true).unwrap()[0].hidden);
    }

    #[test]
    fn the_derived_project_gets_the_boards_tables_and_variables_and_a_board_without_them_gets_nothing() {
        let root = temp("derive");
        let (board, work) = (root.join("board"), root.join("board").join(".kicad"));
        std::fs::create_dir_all(&work).unwrap();
        let mut pro = serde_json::json!({ "erc": {} });
        assert_eq!(derive_project(&board, &work, &mut pro), Ok(false));
        assert_eq!(pro, serde_json::json!({ "erc": {} }), "nothing to derive, nothing added");
        assert!(!work.join("sym-lib-table").exists());
        // a table and a variable
        let mut t = Table::new(TableKind::Symbol);
        t.rows.push(row("Mine", "${KIPRJMOD}/libs/Mine.kicad_sym"));
        t.rows.push(row("Team", "${TEAM}/Team.kicad_sym"));
        write_table(&board.join("sym-lib-table"), &t).unwrap();
        write_project_variables(&board, &[("TEAM".to_string(), "${KIPRJMOD}/shared".to_string())].into()).unwrap();
        std::fs::write(work.join("fp-lib-table"), "stale").unwrap();
        assert_eq!(derive_project(&board, &work, &mut pro), Ok(true));
        let derived = Table::parse(&std::fs::read_to_string(work.join("sym-lib-table")).unwrap()).unwrap();
        assert_eq!(derived.rows[0].uri, format!("{}/libs/Mine.kicad_sym", board.display()), "the real project folder, not the derived one");
        assert_eq!(derived.rows[1].uri, "${TEAM}/Team.kicad_sym", "a project variable stays a variable: the project file defines it");
        assert!(!work.join("fp-lib-table").exists(), "no table of the board, none in the derived project");
        assert_eq!(pro["text_variables"]["KIPRJMOD"], board.display().to_string());
        assert_eq!(pro["text_variables"]["TEAM"], format!("{}/shared", board.display()));
        assert_eq!(pro["erc"], serde_json::json!({}));
        // the Text Variables page, once used, is the whole set: the file's TEAM is not added behind it, and `${KIPRJMOD}` in a value is the real folder
        let mut paged = serde_json::json!({ "text_variables": { "REV": "B", "LIBS": "${KIPRJMOD}/libs" } });
        assert_eq!(derive_project(&board, &work, &mut paged), Ok(true));
        assert_eq!(paged["text_variables"], serde_json::json!({ "KIPRJMOD": board.display().to_string(), "REV": "B", "LIBS": format!("{}/libs", board.display()) }));
        let mut emptied = serde_json::json!({ "text_variables": {} });
        assert_eq!(derive_project(&board, &work, &mut emptied), Ok(true));
        assert_eq!(emptied["text_variables"], serde_json::json!({ "KIPRJMOD": board.display().to_string() }), "a page with every variable deleted shadows the file");
        // a table that does not parse is left out rather than failing the export
        std::fs::write(board.join("sym-lib-table"), "(sym_lib_table (oops))").unwrap();
        write_project_variables(&board, &BTreeMap::new()).unwrap();
        assert_eq!(derive_project(&board, &work, &mut serde_json::json!({})), Ok(false));
        assert!(!work.join("sym-lib-table").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_table_the_derived_project_carries_names_the_real_project_folder() {
        let mut t = Table::new(TableKind::Footprint);
        t.rows.push(row("A", "${KIPRJMOD}/libs/A.pretty"));
        t.rows.push(row("B", "$(KIPRJMOD)/B.pretty"));
        t.rows.push(row("C", "rel/C.pretty"));
        t.rows.push(row("D", "${MYLIBS}/D.pretty"));
        t.rows.push(row("E", "/abs/E.pretty"));
        let d = derived_for_kicad(&t, Path::new("/work/board"));
        let uris: Vec<&str> = d.rows.iter().map(|r| r.uri.as_str()).collect();
        assert_eq!(uris, ["/work/board/libs/A.pretty", "/work/board/B.pretty", "/work/board/rel/C.pretty", "${MYLIBS}/D.pretty", "/abs/E.pretty"]);
        assert_eq!(d.rows[3].nickname, "D");
    }
}
