//! `DSNLEXER` in the mode `SPECCTRA_DB` runs it in (`common/dsnlexer.cpp`, `SetSpecctraMode( true )`), reading a design or a session file a line at a time.
//!
//! A token is a parenthesis, a bar, a number (`isNumber`), a string in the file's quote character, the one-character quote definition after `string_quote`, the dash of a
//! `<pin_reference>` (`"U2"-"14"`), or a bare word, which is a keyword to the parser when it is one it knows and a symbol when it is not. The parser tells the two apart by the
//! word, and by the token: a quoted word is a string, never a keyword.
//!
//! Errors are KiCad's `PARSE_ERROR`s with the same words: `Expecting 'x'`, `Expecting a|b. Got 'y'`, `Unexpected 'x'`, followed by ` in 'file', line N, offset M.`.

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
        self.err(format!("Expecting {}", self.token_string(tok)))
    }

    /// `Expecting( const char* )`: `Expecting a|b. Got 'x'`.
    pub fn expecting(&self, what: &str) -> ParseError {
        let got = if self.cur == Tok::Symbol { format!("'{}'", self.cur_text) } else { self.token_string(self.cur) };
        self.err(format!("Expecting {what}. Got {got}"))
    }

    /// `Expecting( T_xxx )` for a keyword.
    pub fn expecting_kw(&self, kw: &str) -> ParseError {
        self.err(format!("Expecting '{kw}'"))
    }

    /// `Unexpected( const char* text )`.
    pub fn unexpected(&self, text: &str) -> ParseError {
        self.err(format!("Unexpected {text}"))
    }

    /// `Unexpected( T_xxx )` (the keyword in quotes) and `Unexpected( int aTok )` for a token.
    pub fn unexpected_kw(&self, kw: &str) -> ParseError {
        self.err(format!("Unexpected '{kw}'"))
    }

    pub fn unexpected_tok(&self, tok: Tok) -> ParseError {
        self.err(format!("Unexpected {}", self.token_string(tok)))
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
