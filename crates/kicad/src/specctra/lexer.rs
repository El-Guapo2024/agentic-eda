//! `DSNLEXER` in the mode `SPECCTRA_DB` runs it in (`common/dsnlexer.cpp`, `SetSpecctraMode( true )`), reading a design or a session file a line at a time.
//!
//! A token is a parenthesis, a bar, a number (`isNumber`), a string in the file's quote character, the one-character quote definition after `string_quote`, the dash of a
//! `<pin_reference>` (`"U2"-"14"`), or a bare word, which is a keyword to the parser when it is one it knows and a symbol when it is not. The parser tells the two apart by the
//! word, and by the token: a quoted word is a string, never a keyword.
//!
//! Errors are KiCad's `PARSE_ERROR`s with the same words, followed by ` in 'file', line N, offset M.`: `Expecting '%s'`, `Expecting %s. Got '%s'` and
//! `Unexpected '%s'` put a token's `GetTokenString` -- already in single quotes -- in quotes again (`Expecting ''('' in ..`, as kicad-cli's own readers say it), and
//! `Unexpected( CurText() )` gives the word bare.

/// What a token is (`DSN_T`'s negative values, and a keyword or symbol for a bare word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    None,
    Left,
    Right,
    /// A bare word: a keyword of the grammar when it spells one, else an arbitrary symbol.
    Symbol,
    /// A word in the string quote (`DSN_STRING`).
    Str,
    Number,
    Dash,
    Bar,
    /// The character after `string_quote` (`DSN_QUOTE_DEF`).
    QuoteDef,
    /// The word `string_quote` (`DSN_STRING_QUOTE`).
    StringQuoteWord,
    Eof,
}

/// A `PARSE_ERROR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub problem: String,
    pub source: String,
    pub line: usize,
    pub offset: usize,
}

impl ParseError {
    /// `PARSE_ERROR::Problem()`: `"%s in '%s', line %d, offset %d."`.
    pub fn what(&self) -> String {
        format!("{} in '{}', line {}, offset {}.", self.problem, self.source, self.line, self.offset)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.what())
    }
}

impl std::error::Error for ParseError {}

pub type PResult<T> = Result<T, ParseError>;

/// The lexer: the text split into lines, the token being read and the one before it.
pub struct Lexer<'a> {
    source: String,
    lines: Vec<&'a [u8]>,
    /// The line being read (`LINE_READER::LineNumber()` is `line + 1` once a line was read).
    line: usize,
    started: bool,
    /// Offsets in the current line: where the next token starts (`next`) and where the line ends (`limit`).
    next: usize,
    limit: usize,
    cur_offset: usize,
    pub cur: Tok,
    pub prev: Tok,
    cur_text: String,
    string_delimiter: u8,
    pub space_in_quoted_tokens: bool,
}

impl<'a> Lexer<'a> {
    /// A lexer over `text`, which `source` names in the errors (`CurSource()`).
    pub fn new(text: &'a str, source: &str) -> Lexer<'a> {
        let bytes = text.as_bytes();
        let mut lines: Vec<&[u8]> = Vec::new();
        let mut start = 0;
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                lines.push(&bytes[start..=i]);
                start = i + 1;
            }
        }
        if start < bytes.len() {
            lines.push(&bytes[start..]);
        }
        Lexer {
            source: source.to_string(),
            lines,
            line: 0,
            started: false,
            next: 0,
            limit: 0,
            cur_offset: 0,
            cur: Tok::None,
            prev: Tok::None,
            cur_text: String::new(),
            // SetSpecctraMode( true ): quoted tokens may hold spaces, the quote is the double quote.
            string_delimiter: b'"',
            space_in_quoted_tokens: true,
        }
    }

    /// `CurText()`.
    pub fn text(&self) -> &str {
        &self.cur_text
    }

    /// `PrevTok()`.
    pub fn prev_tok(&self) -> Tok {
        self.prev
    }

    /// `SetStringDelimiter( c )`: the old one back.
    pub fn set_string_delimiter(&mut self, c: u8) -> u8 {
        std::mem::replace(&mut self.string_delimiter, c)
    }

    /// `CurLineNumber()`.
    pub fn line_number(&self) -> usize {
        if self.started {
            (self.line + 1).min(self.lines.len().max(1))
        } else {
            0
        }
    }

    fn err_at(&self, problem: impl Into<String>, offset: usize) -> ParseError {
        ParseError { problem: problem.into(), source: self.source.clone(), line: self.line_number(), offset }
    }

    fn err(&self, problem: impl Into<String>) -> ParseError {
        self.err_at(problem, self.cur_offset)
    }

    fn read_line(&mut self) -> usize {
        let idx = if self.started { self.line + 1 } else { 0 };
        if idx >= self.lines.len() {
            // `LINE_READER::ReadLine` at the end: nothing, and the line number stays.
            return 0;
        }
        self.started = true;
        self.line = idx;
        self.next = 0;
        self.limit = self.lines[idx].len();
        self.limit
    }

    fn is_space(c: u8) -> bool {
        matches!(c, b' ' | b'\n' | b'\r' | b'\t' | 0)
    }

    fn is_sep(c: u8) -> bool {
        Self::is_space(c) || c == b'(' || c == b')' || c == b'|'
    }

    fn is_string_terminator(&self, c: u8) -> bool {
        (!self.space_in_quoted_tokens && c == b' ') || c == self.string_delimiter
    }

    /// `isNumber`: an integer, a fixed point number, or a float with an exponent.
    fn is_number(s: &[u8]) -> bool {
        let mut i = 0;
        let n = s.len();
        let mut saw = false;
        if i < n && (s[i] == b'-' || s[i] == b'+') {
            i += 1;
        }
        while i < n && s[i].is_ascii_digit() {
            i += 1;
            saw = true;
        }
        if i < n && s[i] == b'.' {
            i += 1;
            while i < n && s[i].is_ascii_digit() {
                i += 1;
                saw = true;
            }
        }
        if saw && i < n && (s[i] == b'E' || s[i] == b'e') {
            i += 1;
            saw = false;
            if i < n && (s[i] == b'-' || s[i] == b'+') {
                i += 1;
            }
            while i < n && s[i].is_ascii_digit() {
                i += 1;
                saw = true;
            }
        }
        saw && i == n
    }

    /// `DSNLEXER::NextTok`.
    pub fn next_tok(&mut self) -> PResult<Tok> {
        self.prev = self.cur;
        if self.cur == Tok::Eof {
            return Ok(Tok::Eof);
        }
        let mut cur = self.next;
        let mut fresh_line = cur >= self.limit;
        loop {
            if fresh_line {
                // L_read
                let len = self.read_line();
                if len == 0 {
                    self.cur = Tok::Eof;
                    self.cur_offset = 0;
                    self.next = self.limit;
                    return Ok(Tok::Eof);
                }
                cur = 0;
                let line = self.lines[self.line];
                while cur < self.limit && Self::is_space(line[cur]) {
                    cur += 1;
                }
                // If the first non-blank character is #, this line is a comment.
                if cur < self.limit && line[cur] == b'#' {
                    fresh_line = true;
                    continue;
                }
            } else {
                let line = self.lines[self.line];
                while cur < self.limit && Self::is_space(line[cur]) {
                    cur += 1;
                }
            }
            if cur >= self.limit {
                fresh_line = true;
                continue;
            }
            break;
        }

        let line = self.lines[self.line];
        let mut head;
        let b = line[cur];
        if b == b'(' || b == b')' {
            self.cur_text = (b as char).to_string();
            self.cur = if b == b'(' { Tok::Left } else { Tok::Right };
            head = cur + 1;
            return Ok(self.finish(cur, head));
        }
        if b == b'|' {
            self.cur_text = "|".to_string();
            self.cur = Tok::Bar;
            head = cur + 1;
            return Ok(self.finish(cur, head));
        }

        // Specctra mode.
        // The dash out of a <pin_reference> like U2-14 or "U2"-"14": a dash with a non-space before it.
        if b == b'-' && cur > 0 && !Self::is_space(line[cur - 1]) {
            self.cur_text = "-".to_string();
            self.cur = Tok::Dash;
            head = cur + 1;
            return Ok(self.finish(cur, head));
        }

        // Switching the string_quote character.
        if self.prev == Tok::StringQuoteWord {
            let errtxt = "String delimiter must be a single character of ', \", or $";
            if !matches!(b, b'\'' | b'$' | b'"') {
                return Err(self.err_at(errtxt, cur));
            }
            self.cur_text = (b as char).to_string();
            head = cur + 1;
            if head < self.limit && !Self::is_sep(line[head]) {
                return Err(self.err_at(errtxt, cur));
            }
            self.cur = Tok::QuoteDef;
            return Ok(self.finish(cur, head));
        }

        // A string in the quote character.
        if b == self.string_delimiter {
            let start = cur + 1;
            head = start;
            while head < self.limit && !self.is_string_terminator(line[head]) {
                head += 1;
            }
            if head >= self.limit {
                return Err(self.err_at("Un-terminated delimited string", self.cur_offset));
            }
            self.cur_text = String::from_utf8_lossy(&line[start..head]).into_owned();
            head += 1;
            self.cur = Tok::Str;
            return Ok(self.finish(cur, head));
        }

        // A bare word.
        head = cur;
        while head < self.limit && !Self::is_sep(line[head]) {
            head += 1;
        }
        let word = &line[cur..head];
        self.cur_text = String::from_utf8_lossy(word).into_owned();
        self.cur = if Self::is_number(word) {
            Tok::Number
        } else if self.cur_text == "string_quote" {
            Tok::StringQuoteWord
        } else {
            Tok::Symbol
        };
        Ok(self.finish(cur, head))
    }

    fn finish(&mut self, cur: usize, head: usize) -> Tok {
        self.cur_offset = cur;
        self.next = head;
        self.cur
    }

    /// `IsSymbol( tok )`: a symbol, a string, or a word that is a keyword.
    pub fn is_symbol(tok: Tok) -> bool {
        matches!(tok, Tok::Symbol | Tok::Str | Tok::StringQuoteWord)
    }

    /// Whether the current token is the bare word `kw` (a keyword `T_xxx`).
    pub fn is_kw(&self, kw: &str) -> bool {
        self.cur == Tok::Symbol && self.cur_text == kw
    }

    // -------------------------------------------------------------------------------------------------------------------- the errors

    /// `GetTokenString( CurTok() )`: the current token as the errors name it -- a bare word that is one of the keywords (`specctra.keywords`) by its own spelling,
    /// any other by its kind (`'symbol'`).
    fn cur_token_string(&self) -> String {
        if self.cur == Tok::Symbol && is_keyword(&self.cur_text) {
            format!("'{}'", self.cur_text)
        } else {
            self.token_string(self.cur)
        }
    }

    fn token_string(&self, tok: Tok) -> String {
        let t = match tok {
            Tok::None => "NONE",
            Tok::StringQuoteWord => "string_quote",
            Tok::QuoteDef => "quoted text delimiter",
            Tok::Dash => "-",
            Tok::Symbol => "symbol",
            Tok::Number => "number",
            Tok::Right => ")",
            Tok::Left => "(",
            Tok::Str => "quoted string",
            Tok::Eof => "end of input",
            Tok::Bar => "|",
        };
        format!("'{t}'")
    }

    /// `Expecting( int aTok )`.
    pub fn expecting_tok(&self, tok: Tok) -> ParseError {
        self.err(format!("Expecting '{}'", self.token_string(tok)))
    }

    /// `Expecting( const char* )`: `Expecting a|b. Got ''x''`.
    pub fn expecting(&self, what: &str) -> ParseError {
        self.err(format!("Expecting {what}. Got '{}'", self.cur_token_string()))
    }

    /// `Expecting( T_xxx )` for a keyword: `Expecting ''kw''`.
    pub fn expecting_kw(&self, kw: &str) -> ParseError {
        self.err(format!("Expecting ''{kw}''"))
    }

    /// `Unexpected( const char* text )`.
    pub fn unexpected(&self, text: &str) -> ParseError {
        self.err(format!("Unexpected {text}"))
    }

    /// `Unexpected( T_xxx )` for a keyword: `Unexpected ''kw''`.
    pub fn unexpected_kw(&self, kw: &str) -> ParseError {
        self.err(format!("Unexpected ''{kw}''"))
    }

    /// `Unexpected( int aTok )`: the current token (a keyword by its name, another word as `'symbol'`), or the end of the input.
    pub fn unexpected_tok(&self, tok: Tok) -> ParseError {
        let what = if tok == self.cur { self.cur_token_string() } else { self.token_string(tok) };
        self.err(format!("Unexpected '{what}'"))
    }

    /// `NeedLEFT`.
    pub fn need_left(&mut self) -> PResult<()> {
        if self.next_tok()? != Tok::Left {
            return Err(self.expecting_tok(Tok::Left));
        }
        Ok(())
    }

    /// `NeedRIGHT`.
    pub fn need_right(&mut self) -> PResult<()> {
        if self.next_tok()? != Tok::Right {
            return Err(self.expecting_tok(Tok::Right));
        }
        Ok(())
    }

    /// `NeedSYMBOL`.
    pub fn need_symbol(&mut self) -> PResult<Tok> {
        let tok = self.next_tok()?;
        if !Self::is_symbol(tok) {
            return Err(self.expecting_tok(Tok::Symbol));
        }
        Ok(tok)
    }

    /// `NeedSYMBOLorNUMBER`.
    pub fn need_symbol_or_number(&mut self) -> PResult<Tok> {
        let tok = self.next_tok()?;
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.expecting("a symbol or number"));
        }
        Ok(tok)
    }

    /// `parseDouble`: the current token as a number.
    pub fn parse_double(&self) -> PResult<f64> {
        self.cur_text.trim().parse::<f64>().map_err(|_| self.err("Invalid floating point number"))
    }
}

/// The keywords of the Specctra lexer (`pcbnew/specctra_import_export/specctra.keywords`), sorted: a bare word spelled as one of them is that keyword's token
/// (`DSNLEXER::findToken`, an exact match), any other is `DSN_SYMBOL`.
const KEYWORDS: &[&str] = &[
    "absolute", "add_group", "add_pins", "added", "allow_antenna", "allow_redundant_wiring", "amp", "ancestor", "antipad", "aperture_type", "array", "attach",
    "attr", "average_pair_length", "back", "base_design", "bbv_ctr2ctr", "bend_keepout", "bond", "both", "bottom", "bottom_layer_sel", "boundary", "brickpat",
    "bundle", "bus", "bypass", "capacitance_resolution", "capacitor", "case_sensitive", "cct1", "cct1a", "center_center", "checking_trim_by_pin", "circ",
    "circle", "circuit", "class", "class_class", "classes", "clear", "clearance", "cluster", "cm", "color", "colors", "comment", "comp", "comp_edge_center",
    "comp_order", "component", "composite", "conductance_resolution", "conductor", "conflict", "connect", "constant", "contact", "control", "corner", "corners",
    "cost", "created_time", "cross", "crosstalk_model", "current_resolution", "deleted", "deleted_keepout", "delta", "diagonal", "direction", "directory",
    "discrete", "effective_via_length", "elongate_keepout", "exclude", "expose", "extra_image_directory", "family", "family_family", "family_family_spacing",
    "fanout", "farad", "file", "fit", "fix", "flip_style", "floor_plan", "footprint", "forbidden", "force_to_terminal_point", "forgotten", "free", "fromto",
    "front", "front_only", "gap", "gate", "gates", "generated_by_freeroute", "global", "grid", "group", "group_set", "guide", "hard", "height", "high",
    "history", "horizontal", "host_cad", "host_version", "image", "image_conductor", "image_image", "image_image_spacing", "image_outline_clearance",
    "image_set", "image_type", "inch", "include", "include_pins_in_crosstalk", "inductance_resolution", "insert", "instcnfg", "inter_layer_clearance", "jumper",
    "junction_type", "keepout", "kg", "kohm", "large", "large_large", "layer", "layer_depth", "layer_noise_weight", "layer_pair", "layer_rule", "length",
    "length_amplitude", "length_factor", "length_gap", "library", "library_out", "limit", "limit_bends", "limit_crossing", "limit_vias", "limit_way", "linear",
    "linear_interpolation", "load", "lock_type", "logical_part", "logical_part_mapping", "low", "match_fromto_delay", "match_fromto_length",
    "match_group_delay", "match_group_length", "match_net_delay", "match_net_length", "max_delay", "max_len", "max_length", "max_noise",
    "max_restricted_layer_length", "max_stagger", "max_stub", "max_total_delay", "max_total_length", "max_total_vias", "medium", "mhenry", "mho", "microvia",
    "mid_driven", "mil", "min_gap", "mirror", "mirror_first", "mixed", "mm", "negative_diagonal", "net", "net_number", "net_out", "net_pin_changes", "nets",
    "network", "network_out", "no", "noexpose", "noise_accumulation", "noise_calculation", "normal", "object_type", "off", "off_grid", "offset", "on", "open",
    "opposite_side", "order", "orthogonal", "outline", "overlap", "pad", "pad_pad", "padstack", "pair", "parallel", "parallel_noise", "parallel_segment",
    "parser", "part_library", "path", "pcb", "permit_orient", "permit_side", "physical", "physical_part_mapping", "piggyback", "pin", "pin_allow",
    "pin_cap_via", "pin_via_cap", "pin_width_taper", "pins", "pintype", "place", "place_boundary", "place_control", "place_keepout", "place_rule", "placement",
    "plan", "plane", "pn", "point", "polygon", "polyline_path", "position", "positive_diagonal", "power", "power_dissipation", "power_fanout", "prefix",
    "primary", "priority", "property", "protect", "qarc", "quarter", "radius", "ratio", "ratio_tolerance", "rect", "reduced", "region", "region_class",
    "region_class_class", "region_net", "relative_delay", "relative_group_delay", "relative_group_length", "relative_length", "reorder", "reroute_order_viols",
    "resistance_resolution", "resistor", "resolution", "restricted_layer_length_factor", "room", "rotate", "rotate_first", "round", "roundoff_rotation",
    "route", "route_to_fanout_only", "routes", "routes_include", "rule", "same_net_checking", "sample_window", "saturation_length", "sec", "secondary", "self",
    "sequence_number", "session", "set_color", "set_pattern", "shape", "shield", "shield_gap", "shield_loop", "shield_tie_down_interval", "shield_width",
    "side", "signal", "site", "small", "smd", "snap", "snap_angle", "soft", "source", "space_in_quoted_tokens", "spacing", "spare", "spiral_via", "square",
    "stack_via", "stack_via_depth", "standard", "starburst", "status", "structure", "structure_out", "subgate", "subgates", "substituted", "such", "suffix",
    "super_placement", "supply", "supply_pin", "swapping", "switch_window", "system", "tandem_noise", "tandem_segment", "tandem_shield_overhang", "term_only",
    "terminal", "terminator", "test", "test_points", "testpoint", "threshold", "time_length_factor", "time_resolution", "tjunction", "tolerance", "top",
    "topology", "total", "track_id", "turret", "type", "um", "unassigned", "unconnects", "unit", "up", "use_array", "use_layer", "use_net", "use_via", "value",
    "vertical", "via", "via_array_template", "via_at_smd", "via_keepout", "via_number", "via_rotate_first", "via_site", "via_size", "virtual_pin", "volt",
    "voltage_resolution", "was_is", "way", "weight", "width", "window", "wire", "wire_keepout", "wires", "wires_include", "wiring", "write_resolution",
    "x               # test cmake script with indent and comment", "xy", "y",
];

/// `findToken( word ) != DSN_SYMBOL`.
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.binary_search(&word).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<(Tok, String)> {
        let mut lx = Lexer::new(text, "t");
        let mut out = Vec::new();
        loop {
            let t = lx.next_tok().unwrap();
            out.push((t, lx.text().to_string()));
            if t == Tok::Eof {
                return out;
            }
        }
    }

    #[test]
    fn words_numbers_strings_and_parentheses() {
        let t = toks("(pcb \"my board\" (resolution um 10) 1.5e3 -2 U1-2 +12)\n");
        let kinds: Vec<Tok> = t.iter().map(|x| x.0).collect();
        assert_eq!(
            kinds,
            vec![Tok::Left, Tok::Symbol, Tok::Str, Tok::Left, Tok::Symbol, Tok::Symbol, Tok::Number, Tok::Right, Tok::Number, Tok::Number, Tok::Symbol, Tok::Number, Tok::Right, Tok::Eof]
        );
        assert_eq!(t[2].1, "my board", "a string loses its quotes and may hold spaces");
        assert_eq!(t[10].1, "U1-2", "a dash inside a word is part of it");
    }

    #[test]
    fn a_dash_after_a_quote_is_the_dash_of_a_pin_reference() {
        let t = toks("\"U12\"-\"14\" U2-14 a -1\n");
        let kinds: Vec<Tok> = t.iter().map(|x| x.0).collect();
        assert_eq!(kinds, vec![Tok::Str, Tok::Dash, Tok::Str, Tok::Symbol, Tok::Symbol, Tok::Number, Tok::Eof]);
        assert_eq!(t[0].1, "U12");
        assert_eq!(t[2].1, "14");
    }

    #[test]
    fn comment_lines_are_skipped_and_the_end_is_sticky() {
        let mut lx = Lexer::new("# a comment\n  # another\n(a)\n", "t");
        assert_eq!(lx.next_tok().unwrap(), Tok::Left);
        assert_eq!(lx.line_number(), 3);
        for _ in 0..3 {
            lx.next_tok().unwrap();
        }
        assert_eq!(lx.next_tok().unwrap(), Tok::Eof);
        assert_eq!(lx.next_tok().unwrap(), Tok::Eof);
    }

    #[test]
    fn string_quote_changes_the_delimiter() {
        let t = toks("(string_quote $)\n($a b$ \"x\")\n");
        // After string_quote the delimiter is a dollar; the double quote is then a character of a bare word.
        let mut lx = Lexer::new("(string_quote $)\n($a b$)\n", "t");
        let mut got = Vec::new();
        loop {
            let k = lx.next_tok().unwrap();
            if k == Tok::QuoteDef {
                lx.set_string_delimiter(lx.text().as_bytes()[0]);
            }
            got.push((k, lx.text().to_string()));
            if k == Tok::Eof {
                break;
            }
        }
        assert_eq!(got[2].0, Tok::QuoteDef);
        assert_eq!(got[5], (Tok::Str, "a b".to_string()));
        assert!(!t.is_empty());
    }

    #[test]
    fn an_unterminated_string_is_kicads_error() {
        let mut lx = Lexer::new("(a \"b\n", "board.ses");
        lx.next_tok().unwrap();
        lx.next_tok().unwrap();
        let e = lx.next_tok().unwrap_err();
        assert_eq!(e.problem, "Un-terminated delimited string");
        assert_eq!(e.source, "board.ses");
        assert_eq!(e.line, 1);
        assert!(e.what().starts_with("Un-terminated delimited string in 'board.ses', line 1, offset "));
    }

    #[test]
    fn a_bar_separates_words() {
        let t = toks("a|b\n");
        assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![Tok::Symbol, Tok::Bar, Tok::Symbol, Tok::Eof]);
    }
}
