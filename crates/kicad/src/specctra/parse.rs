//! The recursive descent parser of `SPECCTRA_DB` (`pcbnew/specctra_import_export/specctra.cpp`): `doPCB` for a design file and `doSESSION` for a session, one `do..`
//! function for each of KiCad's, taking the same tokens in the same order and failing with the same words where they fail.
//!
//! What a session or a design holds that this port has no use for (`control`, `region`, `grid`, `class_class`, `layer_noise_weight`, the topology of a net) is read for its
//! grammar and dropped. `SPECCTRA_DB::LoadPCB` and `LoadSESSION` are [`load_pcb`] and [`load_session`].

use super::lexer::{Lexer, PResult, ParseError, Tok};
use super::model::*;

/// `atoi`: the integer at the start of `s`, 0 when there is none.
fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    if end < b.len() && (b[end] == b'-' || b[end] == b'+') {
        end += 1;
    }
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    t[..end].parse::<i64>().unwrap_or(0).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `SPECCTRA_DB::LoadPCB`: a design file's text. `source` is the name the errors give.
pub fn load_pcb(text: &str, source: &str) -> PResult<Pcb> {
    let mut p = DsnParser::new(text, source);
    if p.lx.next_tok()? != Tok::Left {
        return Err(p.lx.expecting_tok(Tok::Left));
    }
    if p.lx.next_tok()? != Tok::Symbol || p.lx.text() != "pcb" {
        return Err(p.lx.expecting_kw("pcb"));
    }
    let mut pcb = Pcb::default();
    p.do_pcb(&mut pcb)?;
    Ok(pcb)
}

/// `SPECCTRA_DB::LoadSESSION`: a session file's text.
pub fn load_session(text: &str, source: &str) -> PResult<Session> {
    let mut p = DsnParser::new(text, source);
    if p.lx.next_tok()? != Tok::Left {
        return Err(p.lx.expecting_tok(Tok::Left));
    }
    if p.lx.next_tok()? != Tok::Symbol || p.lx.text() != "session" {
        return Err(p.lx.expecting_kw("session"));
    }
    let mut session = Session::default();
    p.do_session(&mut session)?;
    Ok(session)
}

struct DsnParser<'a> {
    lx: Lexer<'a>,
    /// `m_quote_char`: what `doRULE` and the circuit strings put back round a quoted word.
    quote_char: char,
}

type R = PResult<()>;

impl<'a> DsnParser<'a> {
    fn new(text: &'a str, source: &str) -> Self {
        DsnParser { lx: Lexer::new(text, source), quote_char: '"' }
    }

    fn next(&mut self) -> PResult<Tok> {
        self.lx.next_tok()
    }

    /// The bare word the current token is, if it is one (a keyword of the grammar to the caller).
    fn word(&self) -> Option<String> {
        (self.lx.cur == Tok::Symbol).then(|| self.lx.text().to_string())
    }

    fn cur_text(&self) -> String {
        self.lx.text().to_string()
    }

    fn is_symbol(tok: Tok) -> bool {
        Lexer::is_symbol(tok)
    }

    /// `Unexpected( CurText() )`.
    fn unexpected_text(&self) -> ParseError {
        self.lx.unexpected(self.lx.text())
    }

    fn need_left_tok(&self, tok: Tok) -> R {
        if tok != Tok::Left {
            return Err(self.lx.expecting_tok(Tok::Left));
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------------------------------------- doPCB

    fn do_pcb(&mut self, growth: &mut Pcb) -> R {
        self.lx.need_symbol()?;
        growth.pcbname = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                break;
            }
            self.need_left_tok(tok)?;
            self.next()?;
            let w = self.word();
            match w.as_deref() {
                Some("parser") => {
                    if growth.parser.is_some() {
                        return Err(self.lx.unexpected_kw("parser"));
                    }
                    let mut p = Parser::new("");
                    self.do_parser(&mut p)?;
                    growth.parser = Some(p);
                }
                Some("unit") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("resolution") => {
                    if growth.resolution.is_some() {
                        return Err(self.lx.unexpected_kw("resolution"));
                    }
                    let mut u = UnitRes::resolution(Unit::Inch, 2_540_000);
                    self.do_resolution(&mut u)?;
                    growth.resolution = Some(u);
                }
                Some("structure") => {
                    if growth.structure.is_some() {
                        return Err(self.lx.unexpected_kw("structure"));
                    }
                    let mut s = Structure::default();
                    self.do_structure(&mut s)?;
                    growth.structure = Some(s);
                }
                Some("placement") => {
                    if growth.placement.is_some() {
                        return Err(self.lx.unexpected_kw("placement"));
                    }
                    let mut s = Placement::default();
                    self.do_placement(&mut s)?;
                    growth.placement = Some(s);
                }
                Some("library") => {
                    if growth.library.is_some() {
                        return Err(self.lx.unexpected_kw("library"));
                    }
                    let mut s = Library::default();
                    self.do_library(&mut s)?;
                    growth.library = Some(s);
                }
                Some("network") => {
                    if growth.network.is_some() {
                        return Err(self.lx.unexpected_kw("network"));
                    }
                    let mut s = Network::default();
                    self.do_network(&mut s)?;
                    growth.network = Some(s);
                }
                Some("wiring") => {
                    if growth.wiring.is_some() {
                        return Err(self.lx.unexpected_kw("wiring"));
                    }
                    let mut s = Wiring::default();
                    self.do_wiring(&mut s)?;
                    growth.wiring = Some(s);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
        let tok = self.next()?;
        if tok != Tok::Eof {
            return Err(self.lx.expecting_tok(Tok::Eof));
        }
        Ok(())
    }

    fn on_off(&mut self) -> PResult<bool> {
        let tok = self.next()?;
        let w = self.word();
        if tok != Tok::Symbol || (w.as_deref() != Some("on") && w.as_deref() != Some("off")) {
            return Err(self.lx.expecting("on|off"));
        }
        Ok(w.as_deref() == Some("on"))
    }

    fn do_parser(&mut self, growth: &mut Parser) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            let tok = self.next()?;
            if tok == Tok::StringQuoteWord {
                let tok = self.next()?;
                if tok != Tok::QuoteDef {
                    return Err(self.lx.expecting_tok(Tok::QuoteDef));
                }
                let c = self.lx.text().as_bytes()[0];
                self.lx.set_string_delimiter(c);
                growth.string_quote = c as char;
                self.quote_char = c as char;
                self.lx.need_right()?;
                continue;
            }
            match self.word().as_deref() {
                Some("space_in_quoted_tokens") => {
                    let on = self.on_off()?;
                    self.lx.space_in_quoted_tokens = on;
                    growth.space_in_quoted_tokens = on;
                    self.lx.need_right()?;
                }
                Some("host_cad") => {
                    self.lx.need_symbol()?;
                    growth.host_cad = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("host_version") => {
                    self.lx.need_symbol_or_number()?;
                    growth.host_version = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("constant") => {
                    self.lx.need_symbol_or_number()?;
                    let c1 = self.cur_text();
                    self.lx.need_symbol_or_number()?;
                    let c2 = self.cur_text();
                    self.lx.need_right()?;
                    growth.constants.push(c1);
                    growth.constants.push(c2);
                }
                Some("write_resolution") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    if tok != Tok::Symbol {
                        return Err(self.lx.expecting_tok(Tok::Symbol));
                    }
                    let tok = self.next()?;
                    if tok != Tok::Number {
                        return Err(self.lx.expecting_tok(Tok::Number));
                    }
                },
                Some("routes_include") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    match self.word().as_deref() {
                        Some("testpoint") => growth.routes_include_testpoint = true,
                        Some("guide") => growth.routes_include_guides = true,
                        Some("image_conductor") => growth.routes_include_image_conductor = true,
                        _ => return Err(self.lx.expecting("testpoint|guides|image_conductor")),
                    }
                },
                Some("wires_include") => {
                    let tok = self.next()?;
                    if tok != Tok::Symbol || self.lx.text() != "testpoint" {
                        return Err(self.lx.expecting_kw("testpoint"));
                    }
                    growth.routes_include_testpoint = true;
                    self.lx.need_right()?;
                }
                Some("case_sensitive") => {
                    growth.case_sensitive = self.on_off()?;
                    self.lx.need_right()?;
                }
                Some("via_rotate_first") => {
                    growth.via_rotate_first = self.on_off()?;
                    self.lx.need_right()?;
                }
                Some("generated_by_freeroute") => {
                    growth.generated_by_freeroute = true;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_resolution(&mut self, growth: &mut UnitRes) -> R {
        self.next()?;
        growth.units = match self.lx.text().to_lowercase().as_str() {
            "inch" => Unit::Inch,
            "mil" => Unit::Mil,
            "cm" => Unit::Cm,
            "mm" => Unit::Mm,
            "um" => Unit::Um,
            _ => return Err(self.lx.expecting("inch|mil|cm|mm|um")),
        };
        let tok = self.next()?;
        if tok != Tok::Number {
            return Err(self.lx.expecting_tok(Tok::Number));
        }
        growth.value = atoi(self.lx.text());
        self.lx.need_right()
    }

    fn do_unit(&mut self, growth: &mut UnitRes) -> R {
        self.next()?;
        growth.units = match (self.lx.cur, self.lx.text()) {
            (Tok::Symbol, "inch") => Unit::Inch,
            (Tok::Symbol, "mil") => Unit::Mil,
            (Tok::Symbol, "cm") => Unit::Cm,
            (Tok::Symbol, "mm") => Unit::Mm,
            (Tok::Symbol, "um") => Unit::Um,
            _ => return Err(self.lx.expecting("inch|mil|cm|mm|um")),
        };
        self.lx.need_right()
    }

    // ---------------------------------------------------------------------------------------------------------------------- structure

    fn do_structure(&mut self, growth: &mut Structure) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            let w = self.word();
            match w.as_deref() {
                Some("unit") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("resolution") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("resolution"));
                    }
                    let mut u = UnitRes::resolution(Unit::Inch, 2_540_000);
                    self.do_resolution(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("layer_noise_weight") => self.do_layer_noise_weight()?,
                Some(k @ ("place_boundary" | "boundary")) => {
                    let as_place = k == "place_boundary" || growth.boundary.is_some();
                    if as_place {
                        if growth.place_boundary.is_some() {
                            return Err(self.lx.unexpected_kw(k));
                        }
                        let mut b = Boundary::new(true);
                        self.do_boundary(&mut b)?;
                        growth.place_boundary = Some(b);
                    } else {
                        let mut b = Boundary::new(false);
                        self.do_boundary(&mut b)?;
                        growth.boundary = Some(b);
                    }
                }
                Some("plane") => {
                    let mut plane = Keepout::new("plane");
                    self.do_keepout(&mut plane)?;
                    growth.planes.push(plane);
                }
                Some("region") => self.do_region()?,
                Some("snap_angle") => {
                    self.lx.need_symbol()?;
                    self.lx.need_right()?;
                }
                Some("via") => {
                    if growth.via.is_some() {
                        return Err(self.lx.unexpected_kw("via"));
                    }
                    let mut v = ViaList::default();
                    self.do_via(&mut v)?;
                    growth.via = Some(v);
                }
                Some("control") => self.do_control()?,
                Some("layer") => {
                    let mut layer = Layer::new("");
                    self.do_layer(&mut layer)?;
                    growth.layers.push(layer);
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("place_rule") => {
                    if growth.place_rules.is_some() {
                        return Err(self.lx.unexpected_kw("place_rule"));
                    }
                    let mut r = Rule::new("place_rule");
                    self.do_rule(&mut r)?;
                    growth.place_rules = Some(r);
                }
                Some(k @ ("keepout" | "place_keepout" | "via_keepout" | "wire_keepout" | "bend_keepout" | "elongate_keepout")) => {
                    let kind = keepout_kind(k);
                    let mut ko = Keepout::new(kind);
                    self.do_keepout(&mut ko)?;
                    growth.keepouts.push(ko);
                }
                Some("grid") => self.do_grid()?,
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_structure_out(&mut self, growth: &mut StructureOut) -> R {
        let mut tok = self.next()?;
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("layer") => {
                    let mut layer = Layer::new("");
                    self.do_layer(&mut layer)?;
                    growth.layers.push(layer);
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    fn do_layer_noise_weight(&mut self) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            if self.next()? != Tok::Symbol || self.lx.text() != "layer_pair" {
                return Err(self.lx.expecting_kw("layer_pair"));
            }
            // doSPECCTRA_LAYER_PAIR
            self.lx.need_symbol()?;
            self.lx.need_symbol()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            self.lx.parse_double()?;
            self.lx.need_right()?;
        }
    }

    fn do_keepout(&mut self, growth: &mut Keepout) -> R {
        let mut tok = self.next()?;
        if Self::is_symbol(tok) {
            growth.name = self.cur_text();
            tok = self.next()?;
        }
        if tok != Tok::Left {
            return Err(self.lx.expecting_tok(Tok::Left));
        }
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            let w = self.word();
            match w.as_deref() {
                Some("sequence_number") => {
                    if self.next()? != Tok::Number {
                        return Err(self.lx.expecting_tok(Tok::Number));
                    }
                    growth.sequence_number = atoi(self.lx.text());
                    self.lx.need_right()?;
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("place_rule") => {
                    if growth.place_rules.is_some() {
                        return Err(self.lx.unexpected_kw("place_rule"));
                    }
                    let mut r = Rule::new("place_rule");
                    self.do_rule(&mut r)?;
                    growth.place_rules = Some(r);
                }
                Some(k @ ("rect" | "circle" | "polyline_path" | "path" | "polygon" | "qarc")) => {
                    if growth.shape.is_some() {
                        return Err(self.lx.unexpected_kw(if k == "polyline_path" { "path" } else { k }));
                    }
                    growth.shape = Some(self.do_shape_elem(k)?);
                }
                Some("window") => {
                    let mut window = Window::default();
                    self.do_window(&mut window)?;
                    growth.windows.push(window);
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    /// The `rect`, `circle`, `path`, `polygon` or `qarc` the current token (already read) starts.
    fn do_shape_elem(&mut self, kind: &str) -> PResult<ShapeElem> {
        Ok(match kind {
            "rect" => {
                let mut r = Rectangle::default();
                self.do_rectangle(&mut r)?;
                ShapeElem::Rect(r)
            }
            "circle" => {
                let mut c = Circle::default();
                self.do_circle(&mut c)?;
                ShapeElem::Circle(c)
            }
            "qarc" => {
                let mut q = Qarc::default();
                self.do_qarc(&mut q)?;
                ShapeElem::Qarc(q)
            }
            "polygon" => {
                let mut p = PathElem::new(true);
                self.do_path(&mut p)?;
                ShapeElem::Path(p)
            }
            _ => {
                let mut p = PathElem::new(false);
                self.do_path(&mut p)?;
                ShapeElem::Path(p)
            }
        })
    }

    fn do_window(&mut self, growth: &mut Window) -> R {
        let mut tok = self.next()?;
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some(k @ ("rect" | "circle" | "polyline_path" | "path" | "polygon" | "qarc")) => {
                    if growth.shape.is_some() {
                        return Err(self.lx.unexpected_kw(if k == "polyline_path" { "path" } else { k }));
                    }
                    growth.shape = Some(self.do_shape_elem(k)?);
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    fn do_boundary(&mut self, growth: &mut Boundary) -> R {
        let mut tok = self.next()?;
        if tok != Tok::Left {
            return Err(self.lx.expecting_tok(Tok::Left));
        }
        tok = self.next()?;
        if tok == Tok::Symbol && self.lx.text() == "rect" {
            if !growth.paths.is_empty() {
                return Err(self.lx.unexpected("rect when path already encountered"));
            }
            let mut r = Rectangle::default();
            self.do_rectangle(&mut r)?;
            growth.rectangle = Some(r);
            self.lx.need_right()?;
        } else if tok == Tok::Symbol && self.lx.text() == "path" {
            if growth.rectangle.is_some() {
                return Err(self.lx.unexpected("path when rect already encountered"));
            }
            loop {
                if !(self.lx.cur == Tok::Symbol && self.lx.text() == "path") {
                    return Err(self.lx.expecting_kw("path"));
                }
                let mut path = PathElem::new(false);
                self.do_path(&mut path)?;
                growth.paths.push(path);
                tok = self.next()?;
                if tok == Tok::Right {
                    break;
                }
                self.need_left_tok(tok)?;
                self.next()?;
            }
        } else {
            return Err(self.lx.expecting("rect|path"));
        }
        Ok(())
    }

    fn do_path(&mut self, growth: &mut PathElem) -> R {
        let mut tok = self.next()?;
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.lx.expecting("layer_id"));
        }
        growth.layer_id = self.cur_text();
        if self.next()? != Tok::Number {
            return Err(self.lx.expecting("aperture_width"));
        }
        growth.aperture_width = self.lx.parse_double()?;
        tok = self.next()?;
        loop {
            if tok != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            let x = self.lx.parse_double()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            let y = self.lx.parse_double()?;
            growth.points.push(Pt::new(x, y));
            tok = self.next()?;
            if tok == Tok::Right || tok == Tok::Left {
                break;
            }
        }
        if tok == Tok::Left {
            if self.next()? != Tok::Symbol || self.lx.text() != "aperture_type" {
                return Err(self.lx.expecting_kw("aperture_type"));
            }
            self.next()?;
            let w = self.word();
            if w.as_deref() != Some("round") && w.as_deref() != Some("square") {
                return Err(self.lx.expecting("round|square"));
            }
            growth.aperture_square = w.as_deref() == Some("square");
            self.lx.need_right()?;
        }
        Ok(())
    }

    fn do_rectangle(&mut self, growth: &mut Rectangle) -> R {
        self.lx.need_symbol()?;
        growth.layer_id = self.cur_text();
        let mut v = [0.0f64; 4];
        for slot in v.iter_mut() {
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            *slot = self.lx.parse_double()?;
        }
        growth.point0 = Pt::new(v[0], v[1]);
        growth.point1 = Pt::new(v[2], v[3]);
        self.lx.need_right()
    }

    fn do_circle(&mut self, growth: &mut Circle) -> R {
        self.lx.need_symbol_or_number()?;
        growth.layer_id = self.cur_text();
        if self.next()? != Tok::Number {
            return Err(self.lx.expecting_tok(Tok::Number));
        }
        growth.diameter = self.lx.parse_double()?;
        let mut tok = self.next()?;
        if tok == Tok::Number {
            growth.vertex.x = self.lx.parse_double()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            growth.vertex.y = self.lx.parse_double()?;
            tok = self.next()?;
        }
        if tok != Tok::Right {
            return Err(self.lx.expecting_tok(Tok::Right));
        }
        Ok(())
    }

    fn do_qarc(&mut self, growth: &mut Qarc) -> R {
        self.lx.need_symbol()?;
        growth.layer_id = self.cur_text();
        if self.next()? != Tok::Number {
            return Err(self.lx.expecting_tok(Tok::Number));
        }
        growth.aperture_width = self.lx.parse_double()?;
        for i in 0..3 {
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            growth.vertex[i].x = self.lx.parse_double()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            growth.vertex[i].y = self.lx.parse_double()?;
        }
        self.lx.need_right()
    }

    fn do_via(&mut self, growth: &mut ViaList) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            if tok == Tok::Left {
                if self.next()? != Tok::Symbol || self.lx.text() != "spare" {
                    return Err(self.lx.expecting_kw("spare"));
                }
                loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    if !Self::is_symbol(tok) {
                        return Err(self.lx.expecting_tok(Tok::Symbol));
                    }
                    growth.spares.push(self.cur_text());
                }
            } else if Self::is_symbol(tok) {
                growth.padstacks.push(self.cur_text());
            } else {
                return Err(self.unexpected_text());
            }
        }
    }

    fn do_control(&mut self) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("via_at_smd") => {
                    self.on_off()?;
                    self.lx.need_right()?;
                }
                Some(
                    "off_grid" | "route_to_fanout_only" | "force_to_terminal_point" | "same_net_checking" | "checking_trim_by_pin" | "noise_calculation" | "noise_accumulation" | "include_pins_in_crosstalk"
                    | "bbv_ctr2ctr" | "average_pair_length" | "crosstalk_model" | "roundoff_rotation" | "microvia" | "reroute_order_viols",
                ) => {
                    // doTOKPROP: one keyword, and the closing parenthesis.
                    self.next()?;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_properties(&mut self, growth: &mut Vec<Property>) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.lx.need_symbol_or_number()?;
            let name = self.cur_text();
            self.lx.need_symbol_or_number()?;
            let value = self.cur_text();
            growth.push(Property { name, value });
            self.lx.need_right()?;
        }
    }

    fn do_layer(&mut self, growth: &mut Layer) -> R {
        let tok = self.next()?;
        if !Self::is_symbol(tok) {
            return Err(self.lx.expecting_tok(Tok::Symbol));
        }
        growth.name = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("type") => {
                    self.next()?;
                    growth.layer_type = match self.word().as_deref() {
                        Some("signal") => "signal",
                        Some("power") => "power",
                        Some("mixed") => "mixed",
                        Some("jumper") => "jumper",
                        _ => return Err(self.lx.expecting("signal|power|mixed|jumper")),
                    };
                    if self.next()? != Tok::Right {
                        return Err(self.lx.expecting_tok(Tok::Right));
                    }
                }
                Some("rule") => {
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("property") => self.do_properties(&mut growth.properties)?,
                Some("direction") => {
                    self.next()?;
                    let text = self.cur_text();
                    let dir = match self.word().as_deref() {
                        Some(d @ ("horizontal" | "vertical" | "orthogonal" | "positive_diagonal" | "negative_diagonal" | "diagonal" | "off")) => d.to_string(),
                        // The example in the spec shows an abbreviation of the "horizontal" keyword.
                        _ if text == "hori" => "horizontal".to_string(),
                        _ if text == "vert" => "vertical".to_string(),
                        _ => return Err(self.lx.expecting("horizontal|vertical|orthogonal|positive_diagonal|negative_diagonal|diagonal|off")),
                    };
                    growth.direction = Some(dir);
                    if self.next()? != Tok::Right {
                        return Err(self.lx.expecting_tok(Tok::Right));
                    }
                }
                Some("cost") => {
                    let mut tok = self.next()?;
                    match (tok, self.word().as_deref()) {
                        (Tok::Symbol, Some(c @ ("forbidden" | "high" | "medium" | "low" | "free"))) => growth.cost = Some(c.to_string()),
                        (Tok::Number, _) => growth.cost = Some(atoi(self.lx.text()).to_string()),
                        _ => return Err(self.lx.expecting("forbidden|high|medium|low|free|<positive_integer>|-1")),
                    }
                    tok = self.next()?;
                    if tok == Tok::Left {
                        if self.next()? != Tok::Symbol || self.lx.text() != "type" {
                            return Err(self.unexpected_text());
                        }
                        self.next()?;
                        match self.word().as_deref() {
                            Some(c @ ("length" | "way")) => growth.cost_type = Some(c.to_string()),
                            _ => return Err(self.lx.expecting("length|way")),
                        }
                        if self.next()? != Tok::Right {
                            return Err(self.lx.expecting_tok(Tok::Right));
                        }
                        tok = self.next()?;
                    }
                    if tok != Tok::Right {
                        return Err(self.lx.expecting_tok(Tok::Right));
                    }
                }
                Some("use_net") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    if !Self::is_symbol(tok) {
                        return Err(self.lx.expecting_tok(Tok::Symbol));
                    }
                    growth.use_net.push(self.cur_text());
                },
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    /// `doRULE`: the rules as the text they were written in, one string for each parenthesised rule.
    fn do_rule(&mut self, growth: &mut Rule) -> R {
        let mut builder = String::new();
        let mut nesting = 1;
        let mut tok = Tok::None;
        while nesting != 0 && tok != Tok::Eof {
            tok = self.next()?;
            if tok == Tok::Left {
                nesting += 1;
            } else if tok == Tok::Right {
                nesting -= 1;
            }
            if nesting >= 1 {
                if self.lx.prev_tok() != Tok::Left && tok != Tok::Right && (tok != Tok::Left || nesting > 2) {
                    builder.push(' ');
                }
                if tok == Tok::Str {
                    builder.push(self.quote_char);
                }
                builder.push_str(self.lx.text());
                if tok == Tok::Str {
                    builder.push(self.quote_char);
                }
            }
            // When the nested rule is closed with a `)` and we are back down to nesting 1 (inside the <rule_descriptor> but outside the last rule), save the rule.
            if nesting == 1 {
                growth.rules.push(std::mem::take(&mut builder));
            }
        }
        if tok == Tok::Eof {
            return Err(self.lx.unexpected_tok(Tok::Eof));
        }
        Ok(())
    }

    fn do_region(&mut self) -> R {
        let mut tok = self.next()?;
        if Self::is_symbol(tok) {
            tok = self.next()?;
        }
        let mut has_rule = false;
        let mut has_rect = false;
        let mut has_polygon = false;
        loop {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("rect") => {
                    if has_rect {
                        return Err(self.lx.unexpected_kw("rect"));
                    }
                    has_rect = true;
                    let mut r = Rectangle::default();
                    self.do_rectangle(&mut r)?;
                }
                Some("polygon") => {
                    if has_polygon {
                        return Err(self.lx.unexpected_kw("polygon"));
                    }
                    has_polygon = true;
                    let mut p = PathElem::new(true);
                    self.do_path(&mut p)?;
                }
                Some("region_net" | "region_class") => {
                    self.lx.need_symbol()?;
                    self.lx.need_right()?;
                }
                Some("region_class_class") => self.do_class_class(true)?,
                Some("rule") => {
                    if has_rule {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    has_rule = true;
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
            if tok == Tok::Right {
                if !has_rule {
                    return Err(self.lx.expecting_kw("rule"));
                }
                return Ok(());
            }
        }
    }

    fn do_class_class(&mut self, region: bool) -> R {
        let tok = self.next()?;
        if tok != Tok::Left {
            return Err(self.lx.expecting_tok(Tok::Left));
        }
        let mut has_classes = false;
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            match self.word().as_deref() {
                Some("classes") => {
                    if has_classes {
                        return Err(self.lx.unexpected_kw("classes"));
                    }
                    has_classes = true;
                    // doCLASSES
                    let tok = self.next()?;
                    if !Self::is_symbol(tok) {
                        return Err(self.lx.expecting("class_id"));
                    }
                    loop {
                        let tok = self.next()?;
                        if !Self::is_symbol(tok) {
                            return Err(self.lx.expecting("class_id"));
                        }
                        if self.next()? == Tok::Right {
                            break;
                        }
                    }
                }
                Some("rule") => {
                    if region {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                }
                Some("layer_rule") => {
                    if region {
                        return Err(self.lx.unexpected_kw("layer_rule"));
                    }
                    self.do_layer_rule()?;
                }
                _ => return Err(self.lx.unexpected_tok(tok)),
            }
        }
    }

    fn do_layer_rule(&mut self) -> R {
        self.lx.need_symbol()?;
        let mut tok;
        loop {
            tok = self.next()?;
            if !Self::is_symbol(tok) {
                break;
            }
        }
        if tok != Tok::Left {
            return Err(self.lx.expecting_tok(Tok::Left));
        }
        if self.next()? != Tok::Symbol || self.lx.text() != "rule" {
            return Err(self.lx.expecting_kw("rule"));
        }
        let mut r = Rule::new("rule");
        self.do_rule(&mut r)?;
        self.lx.need_right()
    }

    fn do_grid(&mut self) -> R {
        let mut tok = self.next()?;
        let w = self.word();
        let grid_type = match w.as_deref() {
            Some(t @ ("via" | "wire" | "via_keepout" | "snap" | "place")) => t.to_string(),
            _ => return Err(self.lx.unexpected_tok(tok)),
        };
        if self.next()? != Tok::Number {
            return Err(self.lx.expecting_tok(Tok::Number));
        }
        self.lx.parse_double()?;
        tok = self.next()?;
        if tok == Tok::Left {
            loop {
                tok = self.next()?;
                if tok == Tok::Right {
                    break;
                }
                match self.word().as_deref() {
                    Some("direction") => {
                        if grid_type == "place" {
                            return Err(self.lx.unexpected_kw("direction"));
                        }
                        self.next()?;
                        if !matches!(self.word().as_deref(), Some("x") | Some("y")) {
                            return Err(self.unexpected_text());
                        }
                        if self.next()? != Tok::Right {
                            return Err(self.lx.expecting_tok(Tok::Right));
                        }
                    }
                    Some("offset") => {
                        if grid_type == "place" {
                            return Err(self.lx.unexpected_kw("offset"));
                        }
                        if self.next()? != Tok::Number {
                            return Err(self.lx.expecting_tok(Tok::Number));
                        }
                        self.lx.parse_double()?;
                        if self.next()? != Tok::Right {
                            return Err(self.lx.expecting_tok(Tok::Right));
                        }
                    }
                    Some("image_type") => {
                        if grid_type != "place" {
                            return Err(self.lx.unexpected_kw("image_type"));
                        }
                        self.next()?;
                        if !matches!(self.word().as_deref(), Some("smd") | Some("pin")) {
                            return Err(self.unexpected_text());
                        }
                        if self.next()? != Tok::Right {
                            return Err(self.lx.expecting_tok(Tok::Right));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------------------------------------- placement

    fn do_place(&mut self, growth: &mut Place) -> R {
        let mut tok = self.next()?;
        if !Self::is_symbol(tok) {
            return Err(self.lx.expecting("component_id"));
        }
        growth.component_id = self.cur_text();
        tok = self.next()?;
        if tok == Tok::Number {
            let x = self.lx.parse_double()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting_tok(Tok::Number));
            }
            let y = self.lx.parse_double()?;
            growth.set_vertex(Pt::new(x, y));
            self.next()?;
            growth.side = match self.word().as_deref() {
                Some("front") => Side::Front,
                Some("back") => Side::Back,
                _ => return Err(self.lx.expecting("front|back")),
            };
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting("rotation"));
            }
            growth.rotation = self.lx.parse_double()?;
        }
        loop {
            tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("mirror") => {
                    self.next()?;
                    match self.word().as_deref() {
                        Some(m @ ("x" | "y" | "xy" | "off")) => growth.mirror = Some(m.to_string()),
                        _ => return Err(self.lx.expecting("x|y|xy|off")),
                    }
                }
                Some("status") => {
                    self.next()?;
                    match self.word().as_deref() {
                        Some(m @ ("added" | "deleted" | "substituted")) => growth.status = Some(m.to_string()),
                        _ => return Err(self.lx.expecting("added|deleted|substituted")),
                    }
                }
                Some("logical_part") => {
                    if !growth.logical_part.is_empty() {
                        return Err(self.lx.unexpected_kw("logical_part"));
                    }
                    let tok = self.next()?;
                    if !Self::is_symbol(tok) {
                        return Err(self.lx.expecting("logical_part_id"));
                    }
                    growth.logical_part = self.cur_text();
                }
                Some("place_rule") => {
                    if growth.place_rules.is_some() {
                        return Err(self.lx.unexpected_kw("place_rule"));
                    }
                    let mut r = Rule::new("place_rule");
                    self.do_rule(&mut r)?;
                    growth.place_rules = Some(r);
                }
                Some("property") => {
                    if !growth.properties.is_empty() {
                        return Err(self.lx.unexpected_kw("property"));
                    }
                    self.do_properties(&mut growth.properties)?;
                }
                Some("lock_type") => {
                    self.next()?;
                    match self.word().as_deref() {
                        Some(m @ ("position" | "gate" | "subgate" | "pin")) => growth.lock_type = Some(m.to_string()),
                        _ => return Err(self.lx.expecting("position|gate|subgate|pin")),
                    }
                }
                Some("rule") => {
                    if growth.rules.is_some() || growth.has_region {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("region") => {
                    if growth.rules.is_some() || growth.has_region {
                        return Err(self.lx.unexpected_kw("region"));
                    }
                    growth.has_region = true;
                    self.do_region()?;
                }
                // The keyword is `pn` in the lexer's table and KiCad writes `PN`; both are taken here.
                Some("pn" | "PN") => {
                    if !growth.part_number.is_empty() {
                        return Err(self.lx.unexpected_kw("pn"));
                    }
                    self.lx.need_symbol_or_number()?;
                    growth.part_number = self.cur_text();
                    self.lx.need_right()?;
                }
                _ => {
                    let t = self.lx.cur;
                    return Err(self.lx.unexpected_tok(t));
                }
            }
        }
    }

    fn do_component(&mut self, growth: &mut Component) -> R {
        let mut tok = self.next()?;
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.lx.expecting("image_id"));
        }
        growth.image_id = self.cur_text();
        loop {
            tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("place") => {
                    let mut place = Place::new();
                    self.do_place(&mut place)?;
                    growth.places.push(place);
                }
                _ => {
                    let t = self.lx.cur;
                    return Err(self.lx.unexpected_tok(t));
                }
            }
        }
    }

    fn do_placement(&mut self, growth: &mut Placement) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            if tok == Tok::Eof {
                return Err(self.lx.unexpected_tok(Tok::Eof));
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("unit") => {
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("resolution") => {
                    let mut u = UnitRes::resolution(Unit::Inch, 2_540_000);
                    self.do_resolution(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("place_control") => {
                    self.lx.need_right()?;
                    self.next()?;
                    if self.lx.cur != Tok::Symbol || self.lx.text() != "flip_style" {
                        return Err(self.lx.expecting_kw("flip_style"));
                    }
                    self.next()?;
                    match self.word().as_deref() {
                        Some(f @ ("mirror_first" | "rotate_first")) => growth.flip_style = Some(f.to_string()),
                        _ => return Err(self.lx.expecting("mirror_first|rotate_first")),
                    }
                    self.lx.need_right()?;
                    self.lx.need_right()?;
                }
                Some("component") => {
                    let mut c = Component { image_id: String::new(), places: Vec::new() };
                    self.do_component(&mut c)?;
                    growth.components.push(c);
                }
                _ => {
                    let t = self.lx.cur;
                    return Err(self.lx.unexpected_tok(t));
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- library

    fn do_padstack(&mut self, growth: &mut Padstack) -> R {
        let tok = self.next()?;
        // The padstack id may be a number.
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.lx.expecting("m_padstack_id"));
        }
        growth.padstack_id = self.cur_text();
        let mut has_unit = false;
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("unit") => {
                    if has_unit {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    has_unit = true;
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("rotate") => {
                    growth.rotate_off = !self.on_off()?;
                    self.lx.need_right()?;
                }
                Some("absolute") => {
                    growth.absolute_on = self.on_off()?;
                    self.lx.need_right()?;
                }
                Some("shape") => {
                    let mut shape = ShapeEl::new(false);
                    self.do_shape(&mut shape)?;
                    growth.shapes.push(shape);
                }
                Some("attach") => {
                    self.next()?;
                    let w = self.word();
                    if w.as_deref() != Some("off") && w.as_deref() != Some("on") {
                        return Err(self.lx.expecting("off|on"));
                    }
                    growth.attach = Some(w.as_deref() == Some("on"));
                    let tok = self.next()?;
                    if tok == Tok::Left {
                        if self.next()? != Tok::Symbol || self.lx.text() != "use_via" {
                            return Err(self.lx.expecting_kw("use_via"));
                        }
                        self.lx.need_symbol()?;
                        growth.via_id = self.cur_text();
                        self.lx.need_right()?;
                        self.lx.need_right()?;
                    }
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_shape(&mut self, growth: &mut ShapeEl) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            let mut kind = self.word();
            // The example in the spec uses "circ" instead of "circle".
            if kind.as_deref() == Some("circ") {
                kind = Some("circle".to_string());
            }
            match kind.as_deref() {
                Some(k @ ("rect" | "circle" | "polyline_path" | "path" | "polygon" | "qarc")) => {
                    if growth.shape.is_some() {
                        return Err(self.lx.unexpected_kw(if k == "polyline_path" { "path" } else { k }));
                    }
                    growth.shape = Some(self.do_shape_elem(k)?);
                }
                Some("connect") => {
                    self.next()?;
                    let w = self.word();
                    if w.as_deref() != Some("on") && w.as_deref() != Some("off") {
                        return Err(self.lx.expecting("on|off"));
                    }
                    growth.connect_off = w.as_deref() == Some("off");
                    self.lx.need_right()?;
                }
                Some("window") => {
                    let mut window = Window::default();
                    self.do_window(&mut window)?;
                    growth.windows.push(window);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_image(&mut self, growth: &mut Image) -> R {
        let tok = self.next()?;
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.lx.expecting("image_id"));
        }
        growth.image_id = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("unit") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("side") => {
                    self.next()?;
                    growth.side = match self.word().as_deref() {
                        Some("front") => Some(Side::Front),
                        Some("back") => Some(Side::Back),
                        Some("both") => None,
                        _ => return Err(self.lx.expecting("front|back|both")),
                    };
                    self.lx.need_right()?;
                }
                Some("outline") => {
                    let mut outline = ShapeEl::new(true);
                    self.do_shape(&mut outline)?;
                    growth.outlines.push(outline);
                }
                Some("pin") => {
                    let mut pin = Pin::default();
                    self.do_pin(&mut pin)?;
                    growth.pins.push(pin);
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("place_rule") => {
                    if growth.place_rules.is_some() {
                        return Err(self.lx.unexpected_kw("place_rule"));
                    }
                    let mut r = Rule::new("place_rule");
                    self.do_rule(&mut r)?;
                    growth.place_rules = Some(r);
                }
                Some(k @ ("keepout" | "place_keepout" | "via_keepout" | "wire_keepout" | "bend_keepout" | "elongate_keepout")) => {
                    let mut ko = Keepout::new(keepout_kind(k));
                    self.do_keepout(&mut ko)?;
                    growth.keepouts.push(ko);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_pin(&mut self, growth: &mut Pin) -> R {
        let tok = self.next()?;
        // A padstack id may be a number.
        if !Self::is_symbol(tok) && tok != Tok::Number {
            return Err(self.lx.expecting("m_padstack_id"));
        }
        growth.padstack_id = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            if tok == Tok::Left {
                self.next()?;
                if self.lx.cur != Tok::Symbol || self.lx.text() != "rotate" {
                    return Err(self.lx.expecting_kw("rotate"));
                }
                if self.next()? != Tok::Number {
                    return Err(self.lx.expecting_tok(Tok::Number));
                }
                let r = self.lx.parse_double()?;
                growth.set_rotation(r);
                self.lx.need_right()?;
            } else {
                if !Self::is_symbol(tok) && tok != Tok::Number {
                    return Err(self.lx.expecting("pin_id"));
                }
                growth.pin_id = self.cur_text();
                if self.next()? != Tok::Number {
                    return Err(self.lx.expecting_tok(Tok::Number));
                }
                growth.vertex.x = self.lx.parse_double()?;
                if self.next()? != Tok::Number {
                    return Err(self.lx.expecting_tok(Tok::Number));
                }
                growth.vertex.y = self.lx.parse_double()?;
            }
        }
    }

    fn do_library(&mut self, growth: &mut Library) -> R {
        let mut has_unit = false;
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("unit") => {
                    if has_unit {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    has_unit = true;
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("padstack") => {
                    let mut p = Padstack::new();
                    self.do_padstack(&mut p)?;
                    growth.padstacks.push(p);
                }
                Some("image") => {
                    let mut i = Image::default();
                    self.do_image(&mut i)?;
                    growth.images.push(i);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- network

    /// `readCOMPnPIN`: a pin reference split at its dash; the first token has been read.
    fn read_comp_n_pin(&mut self) -> PResult<PinRef> {
        let pin_def = "<pin_reference>::=<component_id>-<pin_id>";
        if !Self::is_symbol(self.lx.cur) {
            return Err(self.lx.expecting(pin_def));
        }
        if self.lx.cur != Tok::Str {
            // A12-14, no wrapping quotes: a single token to split.
            let text = self.cur_text();
            let Some((c, p)) = text.split_once('-') else { return Err(self.lx.expecting(pin_def)) };
            Ok(PinRef { component_id: c.to_string(), pin_id: p.to_string() })
        } else {
            // "U12"-"14" or "U12"-14, three tokens in either case.
            let component_id = self.cur_text();
            let tok = self.next()?;
            if tok != Tok::Dash {
                return Err(self.lx.expecting(pin_def));
            }
            self.next()?;
            Ok(PinRef { component_id, pin_id: self.cur_text() })
        }
    }

    fn do_net(&mut self, growth: &mut Net) -> R {
        let tok = self.next()?;
        if !Self::is_symbol(tok) {
            return Err(self.lx.expecting("net_id"));
        }
        growth.net_id = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            let w = self.word();
            match w.as_deref() {
                Some("unassigned") => {
                    growth.unassigned = true;
                    self.lx.need_right()?;
                }
                Some("net_number") => {
                    if self.next()? != Tok::Number {
                        return Err(self.lx.expecting_tok(Tok::Number));
                    }
                    growth.net_number = Some(atoi(self.lx.text()));
                    self.lx.need_right()?;
                }
                Some(k @ ("pins" | "order" | "expose" | "noexpose" | "source" | "load" | "terminator")) => {
                    let mut refs = Vec::new();
                    loop {
                        let tok = self.next()?;
                        if tok == Tok::Right {
                            break;
                        }
                        refs.push(self.read_comp_n_pin()?);
                    }
                    if k == "pins" || k == "order" {
                        growth.pins_order = k == "order";
                        growth.pins.extend(refs);
                    }
                }
                Some("comp_order") => {
                    self.do_comp_order()?;
                }
                Some("type") => {
                    self.next()?;
                    if !matches!(self.word().as_deref(), Some("fix") | Some("normal")) {
                        return Err(self.lx.expecting("fix|normal"));
                    }
                    self.lx.need_right()?;
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("layer_rule") => self.do_layer_rule()?,
                Some("fromto") => self.do_fromto()?,
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_comp_order(&mut self) -> R {
        loop {
            let tok = self.next()?;
            if !Self::is_symbol(tok) {
                if tok != Tok::Right {
                    return Err(self.lx.expecting_tok(Tok::Right));
                }
                return Ok(());
            }
        }
    }

    fn do_fromto(&mut self) -> R {
        // The first two grammar items are single tokens: the string delimiter is off while they are read.
        let old = self.lx.set_string_delimiter(0);
        let tok = self.next()?;
        if !Self::is_symbol(tok) {
            self.lx.set_string_delimiter(old);
            return Err(self.lx.expecting_tok(Tok::Symbol));
        }
        let tok = self.next()?;
        if !Self::is_symbol(tok) {
            self.lx.set_string_delimiter(old);
            return Err(self.lx.expecting_tok(Tok::Symbol));
        }
        self.lx.set_string_delimiter(old);
        let mut has_net = false;
        let mut has_rule = false;
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("type") => {
                    self.next()?;
                    if !matches!(self.word().as_deref(), Some("fix") | Some("normal") | Some("soft")) {
                        return Err(self.lx.expecting("fix|normal|soft"));
                    }
                    self.lx.need_right()?;
                }
                Some("rule") => {
                    if has_rule {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    has_rule = true;
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                }
                Some("layer_rule") => self.do_layer_rule()?,
                Some("net") => {
                    if has_net {
                        return Err(self.lx.unexpected_kw("net"));
                    }
                    has_net = true;
                    self.lx.need_symbol()?;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_topology(&mut self) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("fromto") => self.do_fromto()?,
                Some("comp_order") => self.do_comp_order()?,
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_class(&mut self, growth: &mut Class) -> R {
        self.lx.need_symbol()?;
        growth.class_id = self.cur_text();
        // The net ids; <composite_name_list>s are not supported.
        let mut tok;
        loop {
            tok = self.next()?;
            if !Self::is_symbol(tok) {
                break;
            }
            growth.net_ids.push(self.cur_text());
        }
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("layer_rule") => self.do_layer_rule()?,
                Some("topology") => self.do_topology()?,
                Some("circuit") => {
                    // The circuit descriptors are kept as strings.
                    let mut builder = String::new();
                    let mut nesting = 1;
                    let mut t = Tok::None;
                    let mut first = true;
                    while nesting != 0 && t != Tok::Eof {
                        t = self.next()?;
                        if t == Tok::Left {
                            nesting += 1;
                        } else if t == Tok::Right {
                            nesting -= 1;
                        }
                        if nesting >= 1 {
                            let previous = self.lx.prev_tok();
                            // `previousTok != T_circuit`: the keyword that opened the descriptor is the token before the first one.
                            if previous != Tok::Left && !first && t != Tok::Right {
                                builder.push(' ');
                            }
                            first = false;
                            if t == Tok::Str {
                                builder.push(self.quote_char);
                            }
                            builder.push_str(self.lx.text());
                            if t == Tok::Str {
                                builder.push(self.quote_char);
                            }
                        }
                        if nesting == 0 {
                            growth.circuit.push(builder.clone());
                            break;
                        }
                    }
                    if t == Tok::Eof {
                        return Err(self.lx.unexpected_tok(Tok::Eof));
                    }
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    fn do_network(&mut self, growth: &mut Network) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("net") => {
                    let mut n = Net::default();
                    self.do_net(&mut n)?;
                    growth.nets.push(n);
                }
                Some("class") => {
                    let mut c = Class::default();
                    self.do_class(&mut c)?;
                    growth.classes.push(c);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- wiring

    fn do_wire(&mut self, growth: &mut Wire) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some(k @ ("rect" | "circle" | "polyline_path" | "path" | "polygon" | "qarc")) => {
                    if growth.shape.is_some() {
                        return Err(self.lx.unexpected_kw(if k == "polyline_path" { "path" } else { k }));
                    }
                    growth.shape = Some(self.do_shape_elem(k)?);
                }
                Some("net") => {
                    self.lx.need_symbol_or_number()?;
                    growth.net_id = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("turret") => {
                    if self.next()? != Tok::Number {
                        return Err(self.lx.expecting_tok(Tok::Number));
                    }
                    growth.turret = atoi(self.lx.text());
                    self.lx.need_right()?;
                }
                Some("type") => {
                    self.next()?;
                    growth.wire_type = Some(match self.word().as_deref() {
                        Some("fix") => WireType::Fix,
                        Some("route") => WireType::Route,
                        Some("normal") => WireType::Normal,
                        Some("protect") => WireType::Protect,
                        _ => return Err(self.lx.expecting("fix|route|normal|protect")),
                    });
                    self.lx.need_right()?;
                }
                Some("attr") => {
                    self.next()?;
                    match self.word().as_deref() {
                        Some(a @ ("test" | "fanout" | "bus" | "jumper")) => growth.attr = Some(a.to_string()),
                        _ => return Err(self.lx.expecting("test|fanout|bus|jumper")),
                    }
                    self.lx.need_right()?;
                }
                Some("shield") => {
                    self.lx.need_symbol()?;
                    growth.shield = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("window") => {
                    let mut window = Window::default();
                    self.do_window(&mut window)?;
                    growth.windows.push(window);
                }
                Some("connect") => {
                    if growth.has_connect {
                        return Err(self.lx.unexpected_kw("connect"));
                    }
                    growth.has_connect = true;
                    self.do_connect()?;
                }
                Some("supply") => {
                    growth.supply = true;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_connect(&mut self) -> R {
        let mut tok = self.next()?;
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("terminal") => {
                    // Since the terminal information is not used, it is tossed.
                    loop {
                        let t = self.next()?;
                        if t == Tok::Right || t == Tok::Eof {
                            break;
                        }
                    }
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    fn do_wire_via(&mut self, growth: &mut WireVia) -> R {
        self.lx.need_symbol()?;
        growth.padstack_id = self.cur_text();
        let mut tok;
        loop {
            tok = self.next()?;
            if tok != Tok::Number {
                break;
            }
            let x = self.lx.parse_double()?;
            if self.next()? != Tok::Number {
                return Err(self.lx.expecting("vertex.y"));
            }
            let y = self.lx.parse_double()?;
            growth.vertexes.push(Pt::new(x, y));
        }
        while tok != Tok::Right {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("net") => {
                    self.lx.need_symbol()?;
                    growth.net_id = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("via_number") => {
                    if self.next()? != Tok::Number {
                        return Err(self.lx.expecting("<via#>"));
                    }
                    growth.via_number = atoi(self.lx.text());
                    self.lx.need_right()?;
                }
                Some("type") => {
                    self.next()?;
                    growth.via_type = Some(match self.word().as_deref() {
                        Some("fix") => WireType::Fix,
                        Some("route") => WireType::Route,
                        Some("normal") => WireType::Normal,
                        Some("protect") => WireType::Protect,
                        _ => return Err(self.lx.expecting("fix|route|normal|protect")),
                    });
                    self.lx.need_right()?;
                }
                Some("attr") => {
                    self.next()?;
                    match self.word().as_deref() {
                        Some(a @ ("test" | "fanout" | "jumper" | "virtual_pin")) => {
                            growth.attr = Some(a.to_string());
                            if a == "virtual_pin" {
                                self.lx.need_symbol()?;
                                growth.virtual_pin_name = self.cur_text();
                            }
                        }
                        _ => return Err(self.lx.expecting("test|fanout|jumper|virtual_pin")),
                    }
                    self.lx.need_right()?;
                }
                Some("contact") => {
                    self.lx.need_symbol()?;
                    let mut t = Tok::Symbol;
                    while Self::is_symbol(t) {
                        growth.contact_layers.push(self.cur_text());
                        t = self.next()?;
                    }
                    if t != Tok::Right {
                        return Err(self.lx.expecting_tok(Tok::Right));
                    }
                }
                Some("supply") => {
                    growth.supply = true;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
        }
        Ok(())
    }

    fn do_wiring(&mut self, growth: &mut Wiring) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("unit") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("unit"));
                    }
                    let mut u = UnitRes::unit(Unit::Inch);
                    self.do_unit(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("resolution") => {
                    if growth.unit.is_some() {
                        return Err(self.lx.unexpected_kw("resolution"));
                    }
                    let mut u = UnitRes::resolution(Unit::Inch, 2_540_000);
                    self.do_resolution(&mut u)?;
                    growth.unit = Some(u);
                }
                Some("wire") => {
                    let mut w = Wire::new();
                    self.do_wire(&mut w)?;
                    growth.wires.push(w);
                }
                Some("via") => {
                    let mut v = WireVia::new();
                    self.do_wire_via(&mut v)?;
                    growth.wire_vias.push(v);
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    // ---------------------------------------------------------------------------------------------------------------------- session

    /// `readTIME`: `month day hour : minute : second year` or `month day hour:minute:second year`. The time itself is not kept.
    fn read_time(&mut self) -> R {
        let time_toks = "<month> <day> <hour> : <minute> : <second> <year> or <month> <day> <hour>:<minute>:<second> <year>";
        self.lx.need_symbol()?;
        let mut tok = self.next()?;
        if tok != Tok::Number {
            return Err(self.lx.expecting(time_toks));
        }
        tok = self.next()?;
        if tok == Tok::Number {
            for _ in 0..2 {
                self.lx.need_symbol()?;
                if self.lx.text() != ":" {
                    return Err(self.lx.expecting(time_toks));
                }
                tok = self.next()?;
                if tok != Tok::Number {
                    return Err(self.lx.expecting(time_toks));
                }
            }
        } else if tok == Tok::Symbol {
            if self.lx.text().split(':').count() != 3 {
                return Err(self.lx.expecting(time_toks));
            }
        }
        tok = self.next()?;
        if tok != Tok::Number {
            return Err(self.lx.expecting(time_toks));
        }
        Ok(())
    }

    fn do_ancestor(&mut self, growth: &mut History) -> R {
        self.lx.need_symbol()?;
        growth.ancestors.push(self.cur_text());
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("created_time") => {
                    self.read_time()?;
                    self.lx.need_right()?;
                }
                Some("comment") => {
                    self.lx.need_symbol()?;
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_history(&mut self, growth: &mut History) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("ancestor") => self.do_ancestor(growth)?,
                Some("self") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    self.need_left_tok(tok)?;
                    self.next()?;
                    match self.word().as_deref() {
                        Some("created_time") => {
                            self.read_time()?;
                            self.lx.need_right()?;
                        }
                        Some("comment") => {
                            self.lx.need_symbol()?;
                            growth.comments.push(self.cur_text());
                            self.lx.need_right()?;
                        }
                        _ => return Err(self.unexpected_text()),
                    }
                },
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_session(&mut self, growth: &mut Session) -> R {
        // The path can be defined by multiple tokens if there are spaces in it (e.g. by TopoR).
        self.lx.need_symbol()?;
        let mut full_path = self.cur_text();
        let mut tok;
        loop {
            tok = self.next()?;
            if tok == Tok::Left {
                break;
            }
            if tok == Tok::Eof {
                return Err(self.lx.expecting_tok(Tok::Left));
            }
            full_path.push(' ');
            full_path.push_str(self.lx.text());
        }
        growth.session_id = full_path;
        loop {
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("base_design") => {
                    self.lx.need_symbol()?;
                    growth.base_design = self.cur_text();
                    self.lx.need_right()?;
                }
                Some("history") => {
                    if growth.history.is_some() {
                        return Err(self.lx.unexpected_kw("history"));
                    }
                    let mut h = History::default();
                    self.do_history(&mut h)?;
                    growth.history = Some(h);
                }
                Some("structure") => {
                    if growth.structure.is_some() {
                        return Err(self.lx.unexpected_kw("structure"));
                    }
                    let mut s = Structure::default();
                    self.do_structure(&mut s)?;
                    growth.structure = Some(s);
                }
                Some("placement") => {
                    if growth.placement.is_some() {
                        return Err(self.lx.unexpected_kw("placement"));
                    }
                    let mut p = Placement::default();
                    self.do_placement(&mut p)?;
                    growth.placement = Some(p);
                }
                Some("was_is") => {
                    if growth.was_is.is_some() {
                        return Err(self.lx.unexpected_kw("was_is"));
                    }
                    let mut pairs = Vec::new();
                    self.do_was_is(&mut pairs)?;
                    growth.was_is = Some(pairs);
                }
                Some("routes") => {
                    if growth.route.is_some() {
                        return Err(self.lx.unexpected_kw("routes"));
                    }
                    let mut r = Route::default();
                    self.do_route(&mut r)?;
                    growth.route = Some(r);
                }
                _ => return Err(self.unexpected_text()),
            }
            tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
        }
    }

    fn do_was_is(&mut self, growth: &mut Vec<(PinRef, PinRef)>) -> R {
        // None of the pins is ok too.
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("pins") => {
                    self.lx.need_symbol()?;
                    let was = self.read_comp_n_pin()?;
                    self.lx.need_symbol()?;
                    let is = self.read_comp_n_pin()?;
                    growth.push((was, is));
                    self.lx.need_right()?;
                }
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_route(&mut self, growth: &mut Route) -> R {
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("resolution") => {
                    if growth.resolution.is_some() {
                        return Err(self.lx.unexpected_kw("resolution"));
                    }
                    let mut u = UnitRes::resolution(Unit::Inch, 2_540_000);
                    self.do_resolution(&mut u)?;
                    growth.resolution = Some(u);
                }
                Some("parser") => {
                    // Electra 2.9.1 emits two (parser ) elements in a row: the second replaces the first.
                    let mut p = Parser::new("");
                    self.do_parser(&mut p)?;
                    growth.parser = Some(p);
                }
                Some("structure_out") => {
                    if growth.structure_out.is_some() {
                        return Err(self.lx.unexpected_kw("structure_out"));
                    }
                    let mut s = StructureOut::default();
                    self.do_structure_out(&mut s)?;
                    growth.structure_out = Some(s);
                }
                Some("library_out") => {
                    if growth.library.is_some() {
                        return Err(self.lx.unexpected_kw("library_out"));
                    }
                    let mut l = Library::default();
                    self.do_library(&mut l)?;
                    growth.library = Some(l);
                }
                Some("network_out") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    self.need_left_tok(tok)?;
                    let tok = self.next()?;
                    // It is class NET_OUT, but token `net` in Freerouting; Allegro PCB Router (Specctra) uses a capitalized "Net".
                    if !(tok == Tok::Symbol && (self.lx.text() == "net" || self.lx.text() == "Net")) {
                        return Err(self.unexpected_text());
                    }
                    let mut n = NetOut { net_number: -1, ..Default::default() };
                    self.do_net_out(&mut n)?;
                    growth.net_outs.push(n);
                },
                Some("test_points") => loop {
                    let tok = self.next()?;
                    if tok == Tok::Right {
                        break;
                    }
                    // Not supported.
                    return Err(self.unexpected_text());
                },
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_net_out(&mut self, growth: &mut NetOut) -> R {
        self.lx.need_symbol_or_number()?;
        growth.net_id = self.cur_text();
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            self.need_left_tok(tok)?;
            self.next()?;
            match self.word().as_deref() {
                Some("net_number") => {
                    let tok = self.next()?;
                    if tok != Tok::Number {
                        return Err(self.lx.expecting_tok(Tok::Number));
                    }
                    growth.net_number = atoi(self.lx.text());
                    self.lx.need_right()?;
                }
                Some("rule") => {
                    if growth.rules.is_some() {
                        return Err(self.lx.unexpected_kw("rule"));
                    }
                    let mut r = Rule::new("rule");
                    self.do_rule(&mut r)?;
                    growth.rules = Some(r);
                }
                Some("wire") => {
                    let mut w = Wire::new();
                    self.do_wire(&mut w)?;
                    growth.wires.push(w);
                }
                Some("via") => {
                    let mut v = WireVia::new();
                    self.do_wire_via(&mut v)?;
                    growth.wire_vias.push(v);
                }
                Some("supply_pin") => self.do_supply_pin()?,
                _ => return Err(self.unexpected_text()),
            }
        }
    }

    fn do_supply_pin(&mut self) -> R {
        self.lx.need_symbol()?;
        loop {
            let tok = self.next()?;
            if tok == Tok::Right {
                return Ok(());
            }
            if Self::is_symbol(tok) {
                self.read_comp_n_pin()?;
            } else if tok == Tok::Left {
                let tok = self.next()?;
                if !(tok == Tok::Symbol && self.lx.text() == "net") {
                    return Err(self.lx.expecting_kw("net"));
                }
                self.lx.need_right()?;
            } else {
                return Err(self.unexpected_text());
            }
        }
    }
}

fn keepout_kind(k: &str) -> &'static str {
    match k {
        "place_keepout" => "place_keepout",
        "via_keepout" => "via_keepout",
        "wire_keepout" => "wire_keepout",
        "bend_keepout" => "bend_keepout",
        "elongate_keepout" => "elongate_keepout",
        _ => "keepout",
    }
}
