//! The Specctra DSN / SES element tree and the way KiCad writes it: `pcbnew/specctra_import_export/specctra.h` (the classes and their `Format`) and the tail of
//! `specctra.cpp` (`PARSER::FormatContents`, `PLACE::Format`, `PADSTACK::Compare`, `IMAGE::Compare`).
//!
//! The tree is the C++ one with the owning pointers made `Option`s and the `ELEM_HOLDER` kid lists made typed vectors. `Format` is a line-for-line port: the same
//! `Print( nestLevel, .. )` calls in the same order, the same wrapping margins, the same quoting, so a file written here is the file KiCad writes.
//!
//! What is here is what `FromBOARD` makes and `FromSESSION` reads, plus what the parser needs to read a KiCad DSN back (the tests do, to hold the writer to a file
//! KiCad wrote). Constructs the two never use (`region`, `grid`, `class_class`, `topology`, `fromto`, `control`, `layer_noise_weight`) are parsed and dropped by
//! [`super::parse`].

use super::format::{format_double2str, g6, Out};

/// `POINT`: a point of the DSN coordinate system, or a distance from some origin.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }

    /// `POINT::FixNegativeZero`: no `-0` in the file.
    pub fn fixed(mut self) -> Pt {
        if self.x == 0.0 {
            self.x = 0.0;
        }
        if self.y == 0.0 {
            self.y = 0.0;
        }
        self
    }
}

impl std::ops::AddAssign for Pt {
    fn add_assign(&mut self, o: Pt) {
        self.x += o.x;
        self.y += o.y;
    }
}

/// `PROPERTY`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Property {
    pub name: String,
    pub value: String,
}

impl Property {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("({} {})\n", out.q(&self.name), out.q(&self.value)));
    }
}

/// The engineering unit of a `(unit ..)` or `(resolution ..)`: `T_inch`, `T_mil`, `T_cm`, `T_mm`, `T_um`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Inch,
    Mil,
    Cm,
    Mm,
    Um,
}

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Inch => "inch",
            Unit::Mil => "mil",
            Unit::Cm => "cm",
            Unit::Mm => "mm",
            Unit::Um => "um",
        }
    }
}

/// `UNIT_RES`: a `(unit ..)` or a `(resolution ..)`. `UNIT_RES::Default` (inch, 2540000) is what an element with none of its own and none in its parents has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitRes {
    pub is_resolution: bool,
    pub units: Unit,
    pub value: i32,
}

impl UnitRes {
    pub const DEFAULT: UnitRes = UnitRes { is_resolution: true, units: Unit::Inch, value: 2_540_000 };

    pub fn unit(units: Unit) -> UnitRes {
        UnitRes { is_resolution: false, units, value: 2_540_000 }
    }

    pub fn resolution(units: Unit, value: i32) -> UnitRes {
        UnitRes { is_resolution: true, units, value }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        if !self.is_resolution {
            out.print(nest, &format!("(unit {})\n", self.units.name()));
        } else {
            out.print(nest, &format!("(resolution {} {})\n", self.units.name(), self.value));
        }
    }
}

/// `PARSER`: the `(parser ..)` record.
#[derive(Debug, Clone, PartialEq)]
pub struct Parser {
    pub string_quote: char,
    pub space_in_quoted_tokens: bool,
    pub case_sensitive: bool,
    pub wires_include_testpoint: bool,
    pub routes_include_testpoint: bool,
    pub routes_include_guides: bool,
    pub routes_include_image_conductor: bool,
    pub via_rotate_first: bool,
    pub generated_by_freeroute: bool,
    /// Pairs of strings, one pair per `(constant ..)`.
    pub constants: Vec<String>,
    pub host_cad: String,
    pub host_version: String,
}

impl Parser {
    /// `PARSER::PARSER`: KiCad's own defaults, `host_version` being what the build calls itself.
    pub fn new(host_version: &str) -> Parser {
        Parser {
            string_quote: '"',
            space_in_quoted_tokens: false,
            case_sensitive: false,
            wires_include_testpoint: false,
            routes_include_testpoint: false,
            routes_include_guides: false,
            routes_include_image_conductor: false,
            via_rotate_first: true,
            generated_by_freeroute: false,
            constants: Vec::new(),
            host_cad: "KiCad's Pcbnew".to_string(),
            host_version: host_version.to_string(),
        }
    }

    /// `PARSER::FormatContents`.
    pub fn format_contents(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(string_quote {})\n", self.string_quote));
        out.print(nest, &format!("(space_in_quoted_tokens {})\n", if self.space_in_quoted_tokens { "on" } else { "off" }));
        out.print(nest, &format!("(host_cad \"{}\")\n", self.host_cad));
        out.print(nest, &format!("(host_version \"{}\")\n", self.host_version));
        for pair in self.constants.chunks(2) {
            let (s1, s2) = (&pair[0], pair.get(1).map(String::as_str).unwrap_or(""));
            out.print(nest, &format!("(constant {} {})\n", out.q(s1), out.q(s2)));
        }
        if self.routes_include_testpoint || self.routes_include_guides || self.routes_include_image_conductor {
            out.print(
                nest,
                &format!(
                    "(routes_include{}{}{})\n",
                    if self.routes_include_testpoint { " testpoint" } else { "" },
                    if self.routes_include_guides { " guides" } else { "" },
                    if self.routes_include_image_conductor { " image_conductor" } else { "" }
                ),
            );
        }
        if self.wires_include_testpoint {
            out.print(nest, "(wires_include testpoint)\n");
        }
        if !self.via_rotate_first {
            out.print(nest, "(via_rotate_first off)\n");
        }
        if self.case_sensitive {
            out.print(nest, "(case_sensitive on)\n");
        }
    }

    /// `ELEM::Format`: the wrapper around [`Parser::format_contents`].
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(parser\n");
        self.format_contents(out, nest + 1);
        out.print(nest, ")\n");
    }
}

/// `RECTANGLE`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rectangle {
    pub layer_id: String,
    pub point0: Pt,
    pub point1: Pt,
}

impl Rectangle {
    pub fn set_corners(&mut self, p0: Pt, p1: Pt) {
        self.point0 = p0.fixed();
        self.point1 = p1.fixed();
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        let newline = if nest != 0 { "\n" } else { "" };
        out.print(nest, &format!("(rect {} {} {} {} {}){}", out.q(&self.layer_id), g6(self.point0.x), g6(self.point0.y), g6(self.point1.x), g6(self.point1.y), newline));
    }
}

/// `RULE`: a `(rule ..)` or `(place_rule ..)`, its rules kept as the strings they were written in.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: &'static str,
    pub rules: Vec<String>,
}

impl Rule {
    pub fn new(name: &'static str) -> Rule {
        Rule { name, rules: Vec::new() }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("({}", self.name));
        let single_line;
        if self.rules.len() == 1 {
            single_line = true;
            out.print(0, &format!(" {})", self.rules[0]));
        } else {
            out.print(0, "\n");
            single_line = false;
            for rule in &self.rules {
                out.print(nest + 1, &format!("{rule}\n"));
            }
            out.print(nest, ")");
        }
        if nest != 0 || !single_line {
            out.print(0, "\n");
        }
    }
}

/// `PATH`: a `(path ..)` or a `(polygon ..)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PathElem {
    pub polygon: bool,
    pub layer_id: String,
    pub aperture_width: f64,
    pub points: Vec<Pt>,
    pub aperture_square: bool,
}

impl PathElem {
    pub fn new(polygon: bool) -> PathElem {
        PathElem { polygon, layer_id: String::new(), aperture_width: 0.0, points: Vec::new(), aperture_square: false }
    }

    pub fn name(&self) -> &'static str {
        if self.polygon {
            "polygon"
        } else {
            "path"
        }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        let newline = if nest != 0 { "\n" } else { "" };
        const RIGHTMARGIN: usize = 70;
        let mut per_line = out.print(nest, &format!("({} {} {}", self.name(), out.q(&self.layer_id), g6(self.aperture_width)));
        let wrap_nest = (nest + 1).max(6);
        for pt in &self.points {
            if per_line > RIGHTMARGIN {
                out.print(0, "\n");
                per_line = out.print(wrap_nest, "");
            } else {
                per_line += out.print(0, "  ");
            }
            per_line += out.print(0, &format!("{} {}", g6(pt.x), g6(pt.y)));
        }
        if self.aperture_square {
            out.print(0, "(aperture_type square)");
        }
        out.print(0, &format!("){newline}"));
    }
}

/// `CIRCLE`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Circle {
    pub layer_id: String,
    pub diameter: f64,
    pub vertex: Pt,
}

impl Circle {
    pub fn format(&self, out: &mut Out, nest: i32) {
        let newline = if nest != 0 { "\n" } else { "" };
        out.print(nest, &format!("(circle {} {}", out.q(&self.layer_id), g6(self.diameter)));
        if self.vertex.x != 0.0 || self.vertex.y != 0.0 {
            out.print(0, &format!(" {} {}){}", g6(self.vertex.x), g6(self.vertex.y), newline));
        } else {
            out.print(0, &format!("){newline}"));
        }
    }
}

/// `QARC`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Qarc {
    pub layer_id: String,
    pub aperture_width: f64,
    pub vertex: [Pt; 3],
}

impl Qarc {
    pub fn format(&self, out: &mut Out, nest: i32) {
        let newline = if nest != 0 { "\n" } else { "" };
        out.print(nest, &format!("(qarc {} {}", out.q(&self.layer_id), g6(self.aperture_width)));
        for pt in &self.vertex {
            out.print(0, &format!("  {} {}", g6(pt.x), g6(pt.y)));
        }
        out.print(0, &format!("){newline}"));
    }
}

/// The `<shape_descriptor>` a window, a shape, a keepout or a wire holds: a rectangle, a circle, a polygon or path, a quarter arc.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeElem {
    Rect(Rectangle),
    Circle(Circle),
    Qarc(Qarc),
    Path(PathElem),
}

impl ShapeElem {
    pub fn format(&self, out: &mut Out, nest: i32) {
        match self {
            ShapeElem::Rect(s) => s.format(out, nest),
            ShapeElem::Circle(s) => s.format(out, nest),
            ShapeElem::Qarc(s) => s.format(out, nest),
            ShapeElem::Path(s) => s.format(out, nest),
        }
    }

    /// The layer the shape is on.
    pub fn layer_id(&self) -> &str {
        match self {
            ShapeElem::Rect(s) => &s.layer_id,
            ShapeElem::Circle(s) => &s.layer_id,
            ShapeElem::Qarc(s) => &s.layer_id,
            ShapeElem::Path(s) => &s.layer_id,
        }
    }
}

/// `WINDOW`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Window {
    pub shape: Option<ShapeElem>,
}

impl Window {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(window ");
        if let Some(shape) = &self.shape {
            shape.format(out, 0);
        }
        out.print(0, ")\n");
    }
}

/// `KEEPOUT`, and `COPPER_PLANE` (a keepout of the type `plane`).
#[derive(Debug, Clone, PartialEq)]
pub struct Keepout {
    /// `keepout`, `place_keepout`, `via_keepout`, `wire_keepout`, `bend_keepout`, `elongate_keepout` or `plane`.
    pub kind: &'static str,
    pub name: String,
    pub sequence_number: i32,
    pub rules: Option<Rule>,
    pub place_rules: Option<Rule>,
    pub windows: Vec<Window>,
    pub shape: Option<ShapeElem>,
}

impl Keepout {
    pub fn new(kind: &'static str) -> Keepout {
        Keepout { kind, name: String::new(), sequence_number: -1, rules: None, place_rules: None, windows: Vec::new(), shape: None }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        let mut newline = "\n";
        out.print(nest, &format!("({}", self.kind));
        if !self.name.is_empty() {
            out.print(0, &format!(" {}", out.q(&self.name)));
        } else {
            // The zone with no name or net_code == 0.
            out.print(0, " \"\"");
        }
        if self.sequence_number != -1 {
            out.print(0, &format!(" (sequence_number {})", self.sequence_number));
        }
        if let Some(shape) = &self.shape {
            out.print(0, " ");
            shape.format(out, 0);
        }
        if let Some(rules) = &self.rules {
            out.print(0, newline);
            newline = "";
            rules.format(out, nest + 1);
        }
        if let Some(rules) = &self.place_rules {
            out.print(0, newline);
            newline = "";
            rules.format(out, nest + 1);
        }
        if !self.windows.is_empty() {
            out.print(0, newline);
            for window in &self.windows {
                window.format(out, nest + 1);
            }
            out.print(nest, ")\n");
        } else {
            out.print(0, ")\n");
        }
    }
}

/// `BOUNDARY`: `(boundary ..)` or `(place_boundary ..)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    pub place: bool,
    pub paths: Vec<PathElem>,
    pub rectangle: Option<Rectangle>,
}

impl Boundary {
    pub fn new(place: bool) -> Boundary {
        Boundary { place, paths: Vec::new(), rectangle: None }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("({}\n", if self.place { "place_boundary" } else { "boundary" }));
        if let Some(rect) = &self.rectangle {
            rect.format(out, nest + 1);
        } else {
            for path in &self.paths {
                path.format(out, nest + 1);
            }
        }
        out.print(nest, ")\n");
    }

    /// `BOUNDARY::GetCorners`: the x, y of every corner, in order.
    pub fn corners(&self) -> Vec<f64> {
        let mut v = Vec::new();
        if let Some(r) = &self.rectangle {
            v.extend([r.point0.x, r.point0.y, r.point0.x, r.point1.y, r.point1.x, r.point1.y, r.point1.x, r.point0.y]);
        } else {
            for path in &self.paths {
                for pt in &path.points {
                    v.extend([pt.x, pt.y]);
                }
            }
        }
        v
    }
}

/// `VIA` of a structure: the padstacks that may be used as vias, and the spares.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViaList {
    pub padstacks: Vec<String>,
    pub spares: Vec<String>,
}

impl ViaList {
    pub fn format(&self, out: &mut Out, nest: i32) {
        const RIGHTMARGIN: usize = 80;
        let mut per_line = out.print(nest, "(via");
        for padstack in &self.padstacks {
            if per_line > RIGHTMARGIN {
                out.print(0, "\n");
                per_line = out.print(nest + 1, "");
            }
            per_line += out.print(0, &format!(" {}", out.q(padstack)));
        }
        if !self.spares.is_empty() {
            out.print(0, "\n");
            per_line = out.print(nest + 1, "(spare");
            for spare in &self.spares {
                if per_line > RIGHTMARGIN {
                    out.print(0, "\n");
                    per_line = out.print(nest + 2, "");
                }
                per_line += out.print(0, &format!(" {}", out.q(spare)));
            }
            out.print(0, ")");
        }
        out.print(0, ")\n");
    }
}

/// `LAYER`: a `(layer ..)` of a structure.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// `signal`, `power`, `mixed` or `jumper`.
    pub layer_type: &'static str,
    pub direction: Option<String>,
    /// The words after `(cost`: a keyword, or a number (`cost_number`).
    pub cost: Option<String>,
    pub cost_type: Option<String>,
    pub rules: Option<Rule>,
    pub use_net: Vec<String>,
    pub properties: Vec<Property>,
}

impl Layer {
    pub fn new(name: &str) -> Layer {
        Layer { name: name.to_string(), layer_type: "signal", direction: None, cost: None, cost_type: None, rules: None, use_net: Vec::new(), properties: Vec::new() }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(layer {}\n", out.q(&self.name)));
        out.print(nest + 1, &format!("(type {})\n", self.layer_type));
        if !self.properties.is_empty() {
            out.print(nest + 1, "(property\n");
            for property in &self.properties {
                property.format(out, nest + 2);
            }
            out.print(nest + 1, ")\n");
        }
        if let Some(direction) = &self.direction {
            out.print(nest + 1, &format!("(direction {direction})\n"));
        }
        if let Some(rules) = &self.rules {
            rules.format(out, nest + 1);
        }
        if let Some(cost) = &self.cost {
            out.print(nest + 1, &format!("(cost {cost}"));
            if let Some(t) = &self.cost_type {
                out.print(0, &format!(" (type {t})"));
            }
            out.print(0, ")\n");
        }
        if !self.use_net.is_empty() {
            out.print(nest + 1, "(use_net");
            for net in &self.use_net {
                out.print(0, &format!(" {}", out.q(net)));
            }
            out.print(0, ")\n");
        }
        out.print(nest, ")\n");
    }
}

/// `STRUCTURE`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Structure {
    pub unit: Option<UnitRes>,
    pub layers: Vec<Layer>,
    pub boundary: Option<Boundary>,
    pub place_boundary: Option<Boundary>,
    pub via: Option<ViaList>,
    pub rules: Option<Rule>,
    pub place_rules: Option<Rule>,
    pub keepouts: Vec<Keepout>,
    pub planes: Vec<Keepout>,
}

impl Structure {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(structure\n");
        let n = nest + 1;
        if let Some(unit) = &self.unit {
            unit.format(out, n);
        }
        for layer in &self.layers {
            layer.format(out, n);
        }
        if let Some(b) = &self.boundary {
            b.format(out, n);
        }
        if let Some(b) = &self.place_boundary {
            b.format(out, n);
        }
        for plane in &self.planes {
            plane.format(out, n);
        }
        for keepout in &self.keepouts {
            keepout.format(out, n);
        }
        if let Some(via) = &self.via {
            via.format(out, n);
        }
        if let Some(rules) = &self.rules {
            rules.format(out, n);
        }
        if let Some(rules) = &self.place_rules {
            rules.format(out, n);
        }
        out.print(nest, ")\n");
    }
}

/// `STRUCTURE_OUT`: the `(structure_out ..)` of a session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructureOut {
    pub layers: Vec<Layer>,
    pub rules: Option<Rule>,
}

/// `T_front` or `T_back`: the side of a placed component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Front => "front",
            Side::Back => "back",
        }
    }
}

/// `PLACE`: a `(place ..)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub component_id: String,
    pub side: Side,
    pub rotation: f64,
    pub vertex: Option<Pt>,
    pub mirror: Option<String>,
    pub status: Option<String>,
    pub logical_part: String,
    pub place_rules: Option<Rule>,
    pub properties: Vec<Property>,
    pub lock_type: Option<String>,
    pub rules: Option<Rule>,
    pub has_region: bool,
    pub part_number: String,
}

impl Place {
    pub fn new() -> Place {
        Place {
            component_id: String::new(),
            side: Side::Front,
            rotation: 0.0,
            vertex: None,
            mirror: None,
            status: None,
            logical_part: String::new(),
            place_rules: None,
            properties: Vec::new(),
            lock_type: None,
            rules: None,
            has_region: false,
            part_number: String::new(),
        }
    }

    pub fn set_vertex(&mut self, v: Pt) {
        self.vertex = Some(v.fixed());
    }

    /// `PLACE::Format`.
    pub fn format(&self, out: &mut Out, nest: i32) {
        let use_multi_line = self.place_rules.is_some() || !self.properties.is_empty() || self.rules.is_some() || self.has_region;
        if use_multi_line {
            out.print(nest, &format!("(place {}\n", out.q(&self.component_id)));
            out.print(nest + 1, "");
        } else {
            out.print(nest, &format!("(place {}", out.q(&self.component_id)));
        }
        if let Some(v) = &self.vertex {
            out.print(0, &format!(" {} {}", format_double2str(v.x), format_double2str(v.y)));
            out.print(0, &format!(" {}", self.side.name()));
            out.print(0, &format!(" {}", format_double2str(self.rotation)));
        }
        let mut space = " ";
        if let Some(m) = &self.mirror {
            out.print(0, &format!("{space}(mirror {m})"));
            space = "";
        }
        if let Some(s) = &self.status {
            out.print(0, &format!("{space}(status {s})"));
            space = "";
        }
        if !self.logical_part.is_empty() {
            out.print(0, &format!("{space}(logical_part {})", out.q(&self.logical_part)));
            space = "";
        }
        if use_multi_line {
            out.print(0, "\n");
            if let Some(r) = &self.place_rules {
                r.format(out, nest + 1);
            }
            if !self.properties.is_empty() {
                out.print(nest + 1, "(property \n");
                for p in &self.properties {
                    p.format(out, nest + 2);
                }
                out.print(nest + 1, ")\n");
            }
            if let Some(t) = &self.lock_type {
                out.print(nest + 1, &format!("(lock_type {t})\n"));
            }
            if let Some(r) = &self.rules {
                r.format(out, nest + 1);
            }
            if !self.part_number.is_empty() {
                out.print(nest + 1, &format!("(PN {})\n", out.q(&self.part_number)));
            }
        } else {
            if let Some(t) = &self.lock_type {
                out.print(0, &format!("{space}(lock_type {t})"));
                space = "";
            }
            if !self.part_number.is_empty() {
                out.print(0, &format!("{space}(PN {})", out.q(&self.part_number)));
            }
        }
        out.print(0, ")\n");
    }
}

/// `COMPONENT`: the places of one image.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub image_id: String,
    pub places: Vec<Place>,
}

impl Component {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(component {}\n", out.q(&self.image_id)));
        for place in &self.places {
            place.format(out, nest + 1);
        }
        out.print(nest, ")\n");
    }
}

/// `PLACEMENT`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placement {
    pub unit: Option<UnitRes>,
    pub flip_style: Option<String>,
    pub components: Vec<Component>,
}

impl Placement {
    /// `PLACEMENT::LookupCOMPONENT`: the component of an image, added when there is none yet.
    pub fn lookup_component(&mut self, image_id: &str) -> &mut Component {
        if let Some(i) = self.components.iter().position(|c| c.image_id == image_id) {
            return &mut self.components[i];
        }
        self.components.push(Component { image_id: image_id.to_string(), places: Vec::new() });
        self.components.last_mut().expect("just pushed")
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(placement\n");
        let n = nest + 1;
        if let Some(u) = &self.unit {
            u.format(out, n);
        }
        if let Some(f) = &self.flip_style {
            out.print(n, &format!("(place_control (flip_style {f}))\n"));
        }
        for c in &self.components {
            c.format(out, n);
        }
        out.print(nest, ")\n");
    }
}

/// `SHAPE`: a `(shape ..)` of a padstack, or an `(outline ..)` of an image.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeEl {
    pub outline: bool,
    pub shape: Option<ShapeElem>,
    /// `(connect off)` is written; `on` is the default.
    pub connect_off: bool,
    pub windows: Vec<Window>,
}

impl ShapeEl {
    pub fn new(outline: bool) -> ShapeEl {
        ShapeEl { outline, shape: None, connect_off: false, windows: Vec::new() }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("({} ", if self.outline { "outline" } else { "shape" }));
        if let Some(s) = &self.shape {
            s.format(out, 0);
        }
        if self.connect_off {
            out.print(0, "(connect off)");
        }
        if !self.windows.is_empty() {
            out.print(0, "\n");
            for w in &self.windows {
                w.format(out, nest + 1);
            }
            out.print(nest, ")\n");
        } else {
            out.print(0, ")\n");
        }
    }
}

/// `PIN`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pin {
    pub padstack_id: String,
    pub rotation: f64,
    pub is_rotated: bool,
    pub pin_id: String,
    pub vertex: Pt,
    /// KiCad's net code of the pad this pin was made from.
    pub ki_net_code: i32,
}

impl Pin {
    pub fn set_rotation(&mut self, r: f64) {
        self.rotation = r;
        self.is_rotated = r != 0.0;
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        if self.is_rotated {
            out.print(nest, &format!("(pin {} (rotate {})", out.q(&self.padstack_id), g6(self.rotation)));
        } else {
            out.print(nest, &format!("(pin {}", out.q(&self.padstack_id)));
        }
        out.print(0, &format!(" {} {} {})\n", out.q(&self.pin_id), g6(self.vertex.x), g6(self.vertex.y)));
    }
}

/// `IMAGE`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    pub image_id: String,
    /// `(side front|back)` is written; `both` is the default.
    pub side: Option<Side>,
    pub unit: Option<UnitRes>,
    /// The outlines (`SHAPE` of the type `outline`).
    pub outlines: Vec<ShapeEl>,
    pub pins: Vec<Pin>,
    pub rules: Option<Rule>,
    pub place_rules: Option<Rule>,
    pub keepouts: Vec<Keepout>,
    /// How many images of this id came before (`m_duplicated`): `id::n`.
    pub duplicated: i32,
}

impl Image {
    pub fn image_id(&self) -> String {
        if self.duplicated != 0 {
            format!("{}::{}", self.image_id, self.duplicated)
        } else {
            self.image_id.clone()
        }
    }

    /// `IMAGE::FormatContents`.
    pub fn format_contents(&self, out: &mut Out, nest: i32) {
        if let Some(side) = self.side {
            out.print(0, &format!(" (side {})", side.name()));
        }
        out.print(0, "\n");
        if let Some(u) = &self.unit {
            u.format(out, nest);
        }
        for o in &self.outlines {
            o.format(out, nest);
        }
        for p in &self.pins {
            p.format(out, nest);
        }
        if let Some(r) = &self.rules {
            r.format(out, nest);
        }
        if let Some(r) = &self.place_rules {
            r.format(out, nest);
        }
        for k in &self.keepouts {
            k.format(out, nest);
        }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(image {}", out.q(&self.image_id())));
        self.format_contents(out, nest + 1);
        out.print(nest, ")\n");
    }

    /// `makeHash`: the contents with the white space, parentheses and quotes taken out. Two images with the same hash are the same image.
    pub fn hash(&self) -> String {
        let mut sf = Out::default();
        self.format_contents(&mut sf, 0);
        sf.stripped()
    }
}

/// `PADSTACK`: a pad's or a via's padstack.
#[derive(Debug, Clone, PartialEq)]
pub struct Padstack {
    pub padstack_id: String,
    pub unit: Option<UnitRes>,
    pub shapes: Vec<ShapeEl>,
    /// `rotate`: `on` is the default, `off` is written.
    pub rotate_off: bool,
    /// `absolute`: `off` is the default, `on` is written.
    pub absolute_on: bool,
    /// `attach`: `Some(false)` is `(attach off)`, `Some(true)` is `(attach on (use_via ..))`, `None` writes nothing.
    pub attach: Option<bool>,
    pub via_id: String,
    pub rules: Option<Rule>,
}

impl Padstack {
    /// `PADSTACK::PADSTACK`: `attach off`.
    pub fn new() -> Padstack {
        Padstack { padstack_id: String::new(), unit: None, shapes: Vec::new(), rotate_off: false, absolute_on: false, attach: Some(false), via_id: String::new(), rules: None }
    }

    /// `PADSTACK::FormatContents`.
    pub fn format_contents(&self, out: &mut Out, nest: i32) {
        if let Some(u) = &self.unit {
            u.format(out, nest);
        }
        for s in &self.shapes {
            s.format(out, nest);
        }
        out.print(nest, "");
        match self.attach {
            Some(false) => {
                out.print(0, "(attach off)");
            }
            Some(true) => {
                out.print(0, &format!("(attach on (use_via {}))", out.q(&self.via_id)));
            }
            None => {}
        }
        if self.rotate_off {
            out.print(0, "(rotate off)");
        }
        if self.absolute_on {
            out.print(0, "(absolute on)");
        }
        out.print(0, "\n");
        if let Some(r) = &self.rules {
            r.format(out, nest);
        }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(padstack {}\n", out.q(&self.padstack_id)));
        self.format_contents(out, nest + 1);
        out.print(nest, ")\n");
    }

    /// `makeHash`.
    pub fn hash(&self) -> String {
        let mut sf = Out::default();
        self.format_contents(&mut sf, 0);
        sf.stripped()
    }

    /// `PADSTACK::Compare`: by contents, then by name (a via's name holds its drill, which its shapes do not).
    pub fn compare_key(&self) -> (String, String) {
        (self.hash(), self.padstack_id.clone())
    }
}

impl Default for Padstack {
    fn default() -> Self {
        Padstack::new()
    }
}

/// `LIBRARY`: a `(library ..)`, or the `(library_out ..)` of a session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub unit: Option<UnitRes>,
    pub images: Vec<Image>,
    hashes: Vec<String>,
    /// All the padstacks except the vias.
    pub padstacks: Vec<Padstack>,
    pub vias: Vec<Padstack>,
}

impl Library {
    /// The hash of every image (`IMAGE::m_hash`), kept beside the images so a lookup formats nothing twice.
    fn image_hashes(&mut self) -> &Vec<String> {
        while self.hashes.len() > self.images.len() {
            self.hashes.pop();
        }
        for i in self.hashes.len()..self.images.len() {
            let h = self.images[i].hash();
            self.hashes.push(h);
        }
        &self.hashes
    }

    /// `LIBRARY::FindIMAGE`: the index of an image with the same contents, or `None` after numbering `image` among the images of its id.
    pub fn find_image(&mut self, image: &mut Image) -> Option<usize> {
        let hash = image.hash();
        if let Some(i) = self.image_hashes().iter().position(|h| *h == hash) {
            return Some(i);
        }
        // There is no match to the IMAGE contents, but now generate a unique name for it.
        let mut dups = 1;
        for other in &self.images {
            if other.image_id == image.image_id {
                image.duplicated = dups;
                dups += 1;
            }
        }
        None
    }

    /// `LIBRARY::LookupIMAGE`: the index of the image in the library, which is `image` itself, added, when none like it was there.
    pub fn lookup_image(&mut self, mut image: Image) -> usize {
        match self.find_image(&mut image) {
            Some(i) => i,
            None => {
                self.images.push(image);
                self.images.len() - 1
            }
        }
    }

    /// `LIBRARY::LookupVia`: the via padstack that is the same as `via` (same contents, same name), added when there is none.
    pub fn lookup_via(&mut self, via: Padstack) -> usize {
        let key = via.compare_key();
        if let Some(i) = self.vias.iter().position(|v| v.compare_key() == key) {
            return i;
        }
        self.vias.push(via);
        self.vias.len() - 1
    }

    /// `LIBRARY::FindPADSTACK`.
    pub fn find_padstack(&self, id: &str) -> Option<&Padstack> {
        self.padstacks.iter().find(|p| p.padstack_id == id)
    }

    pub fn format_contents(&self, out: &mut Out, nest: i32) {
        if let Some(u) = &self.unit {
            u.format(out, nest);
        }
        for i in &self.images {
            i.format(out, nest);
        }
        for p in &self.padstacks {
            p.format(out, nest);
        }
        for v in &self.vias {
            v.format(out, nest);
        }
    }

    pub fn format(&self, out: &mut Out, nest: i32, name: &str) {
        out.print(nest, &format!("({name}\n"));
        self.format_contents(out, nest + 1);
        out.print(nest, ")\n");
    }
}

/// `PIN_REF`: `component-pin`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PinRef {
    pub component_id: String,
    pub pin_id: String,
}

impl PinRef {
    /// `PIN_REF::FormatIt`: the number of bytes written.
    pub fn format_it(&self, out: &mut Out, nest: i32) -> usize {
        let newline = if nest != 0 { "\n" } else { "" };
        out.print(nest, &format!("{}-{}{}", out.q(&self.component_id), out.q(&self.pin_id), newline))
    }
}

/// `NET`: a net of the `(network ..)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Net {
    pub net_id: String,
    pub unassigned: bool,
    pub net_number: Option<i32>,
    /// `pins` (the default) or `order`.
    pub pins_order: bool,
    pub pins: Vec<PinRef>,
    pub rules: Option<Rule>,
}

impl Net {
    pub fn format(&self, out: &mut Out, nest: i32) {
        let mut space = " ";
        out.print(nest, &format!("(net {}", out.q(&self.net_id)));
        if self.unassigned {
            out.print(0, &format!("{space}(unassigned)"));
            space = "";
        }
        if let Some(n) = self.net_number {
            out.print(0, &format!("{space}(net_number {n})"));
        }
        out.print(0, "\n");
        if !self.pins.is_empty() {
            const RIGHTMARGIN: usize = 80;
            let mut per_line = out.print(nest + 1, if self.pins_order { "(order" } else { "(pins" });
            for pin in &self.pins {
                if per_line > RIGHTMARGIN {
                    out.print(0, "\n");
                    per_line = out.print(nest + 2, "");
                } else {
                    per_line += out.print(0, " ");
                }
                per_line += pin.format_it(out, 0);
            }
            out.print(0, ")\n");
        }
        if let Some(r) = &self.rules {
            r.format(out, nest + 1);
        }
        out.print(nest, ")\n");
    }
}

/// `CLASS`: a net class of the `(network ..)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Class {
    pub class_id: String,
    pub net_ids: Vec<String>,
    pub circuit: Vec<String>,
    pub rules: Option<Rule>,
}

impl Class {
    pub fn format(&self, out: &mut Out, nest: i32) {
        let mut per_line = out.print(nest, &format!("(class {}", out.q(&self.class_id)));
        const RIGHTMARGIN: usize = 72;
        for net_id in &self.net_ids {
            let mut space = " ";
            if per_line > RIGHTMARGIN {
                out.print(0, "\n");
                per_line = out.print(nest + 1, "");
                space = "";
            }
            // Allegro PCB Router (Specctra) doesn't like empty net names.
            if net_id.is_empty() {
                continue;
            }
            per_line += out.print(0, &format!("{space}{}", out.q(net_id)));
        }
        let mut new_line = false;
        if !self.circuit.is_empty() || self.rules.is_some() {
            out.print(0, "\n");
            new_line = true;
        }
        if !self.circuit.is_empty() {
            out.print(nest + 1, "(circuit\n");
            for c in &self.circuit {
                out.print(nest + 2, &format!("{c}\n"));
            }
            out.print(nest + 1, ")\n");
        }
        if let Some(r) = &self.rules {
            r.format(out, nest + 1);
        }
        out.print(if new_line { nest } else { 0 }, ")\n");
    }
}

/// `NETWORK`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub nets: Vec<Net>,
    pub classes: Vec<Class>,
}

impl Network {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(network\n");
        for n in &self.nets {
            n.format(out, nest + 1);
        }
        for c in &self.classes {
            c.format(out, nest + 1);
        }
        out.print(nest, ")\n");
    }
}

/// The `type` of a wire or a via: `fix`, `route`, `normal` or `protect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    Fix,
    Route,
    Normal,
    Protect,
}

impl WireType {
    pub fn name(self) -> &'static str {
        match self {
            WireType::Fix => "fix",
            WireType::Route => "route",
            WireType::Normal => "normal",
            WireType::Protect => "protect",
        }
    }
}

/// `WIRE`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wire {
    pub shape: Option<ShapeElem>,
    pub net_id: String,
    pub turret: i32,
    pub wire_type: Option<WireType>,
    pub attr: Option<String>,
    pub shield: String,
    pub windows: Vec<Window>,
    pub has_connect: bool,
    pub supply: bool,
}

impl Wire {
    pub fn new() -> Wire {
        Wire { turret: -1, ..Default::default() }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(wire ");
        if let Some(s) = &self.shape {
            s.format(out, 0);
        }
        if !self.net_id.is_empty() {
            out.print(0, &format!("(net {})", out.q(&self.net_id)));
        }
        if self.turret >= 0 {
            out.print(0, &format!("(turrent {})", self.turret));
        }
        if let Some(t) = self.wire_type {
            out.print(0, &format!("(type {})", t.name()));
        }
        if let Some(a) = &self.attr {
            out.print(0, &format!("(attr {a})"));
        }
        if !self.shield.is_empty() {
            out.print(0, &format!("(shield {})", out.q(&self.shield)));
        }
        if !self.windows.is_empty() {
            out.print(0, "\n");
            for w in &self.windows {
                w.format(out, nest + 1);
            }
        }
        if self.supply {
            out.print(0, "(supply)");
        }
        out.print(0, ")\n");
    }
}

/// `WIRE_VIA`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireVia {
    pub padstack_id: String,
    pub vertexes: Vec<Pt>,
    pub net_id: String,
    pub via_number: i32,
    pub via_type: Option<WireType>,
    pub attr: Option<String>,
    pub virtual_pin_name: String,
    pub contact_layers: Vec<String>,
    pub supply: bool,
}

impl WireVia {
    pub fn new() -> WireVia {
        WireVia { via_number: -1, ..Default::default() }
    }

    pub fn format(&self, out: &mut Out, nest: i32) {
        const RIGHTMARGIN: usize = 80;
        let mut per_line = out.print(nest, &format!("(via {}", out.q(&self.padstack_id)));
        for pt in &self.vertexes {
            if per_line > RIGHTMARGIN {
                out.print(0, "\n");
                per_line = out.print(nest + 1, "");
            } else {
                per_line += out.print(0, "  ");
            }
            per_line += out.print(0, &format!("{} {}", g6(pt.x), g6(pt.y)));
        }
        if !self.net_id.is_empty() || self.via_number != -1 || self.via_type.is_some() || self.attr.is_some() || self.supply {
            out.print(0, " ");
        }
        let wrap = |out: &mut Out, per_line: &mut usize| {
            if *per_line > RIGHTMARGIN {
                out.print(0, "\n");
                *per_line = out.print(nest + 1, "");
            }
        };
        if !self.net_id.is_empty() {
            wrap(out, &mut per_line);
            per_line += out.print(0, &format!("(net {})", out.q(&self.net_id)));
        }
        if self.via_number != -1 {
            wrap(out, &mut per_line);
            per_line += out.print(0, &format!("(via_number {})", self.via_number));
        }
        if let Some(t) = self.via_type {
            wrap(out, &mut per_line);
            per_line += out.print(0, &format!("(type {})", t.name()));
        }
        if let Some(a) = &self.attr {
            wrap(out, &mut per_line);
            if a == "virtual_pin" {
                per_line += out.print(0, &format!("(attr virtual_pin {})", out.q(&self.virtual_pin_name)));
            } else {
                per_line += out.print(0, &format!("(attr {a})"));
            }
        }
        if self.supply {
            wrap(out, &mut per_line);
            out.print(0, "(supply)");
        }
        if !self.contact_layers.is_empty() {
            out.print(0, "\n");
            out.print(nest + 1, "(contact\n");
            for layer in &self.contact_layers {
                out.print(nest + 2, &format!("{}\n", out.q(layer)));
            }
            out.print(nest + 1, "))\n");
        } else {
            out.print(0, ")\n");
        }
    }
}

/// `WIRING`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wiring {
    pub unit: Option<UnitRes>,
    pub wires: Vec<Wire>,
    pub wire_vias: Vec<WireVia>,
}

impl Wiring {
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, "(wiring\n");
        let n = nest + 1;
        if let Some(u) = &self.unit {
            u.format(out, n);
        }
        for w in &self.wires {
            w.format(out, n);
        }
        for v in &self.wire_vias {
            v.format(out, n);
        }
        out.print(nest, ")\n");
    }
}

/// `PCB`: the design file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pcb {
    pub pcbname: String,
    pub parser: Option<Parser>,
    pub resolution: Option<UnitRes>,
    pub unit: Option<UnitRes>,
    pub structure: Option<Structure>,
    pub placement: Option<Placement>,
    pub library: Option<Library>,
    pub network: Option<Network>,
    pub wiring: Option<Wiring>,
}

impl Pcb {
    /// `SPECCTRA_DB::MakePCB`: a PCB with every part in it empty.
    pub fn make(host_version: &str) -> Pcb {
        Pcb {
            pcbname: String::new(),
            parser: Some(Parser::new(host_version)),
            resolution: Some(UnitRes::resolution(Unit::Inch, 2_540_000)),
            unit: Some(UnitRes::unit(Unit::Inch)),
            structure: Some(Structure { boundary: Some(Boundary::new(false)), via: Some(ViaList::default()), rules: Some(Rule::new("rule")), ..Default::default() }),
            placement: Some(Placement::default()),
            library: Some(Library::default()),
            network: Some(Network::default()),
            wiring: Some(Wiring::default()),
        }
    }

    /// `PCB::Format`.
    pub fn format(&self, out: &mut Out, nest: i32) {
        out.print(nest, &format!("(pcb {}\n", out.q(&self.pcbname)));
        let n = nest + 1;
        if let Some(p) = &self.parser {
            p.format(out, n);
        }
        if let Some(r) = &self.resolution {
            r.format(out, n);
        }
        if let Some(u) = &self.unit {
            u.format(out, n);
        }
        if let Some(s) = &self.structure {
            s.format(out, n);
        }
        if let Some(p) = &self.placement {
            p.format(out, n);
        }
        if let Some(l) = &self.library {
            l.format(out, n, "library");
        }
        if let Some(net) = &self.network {
            net.format(out, n);
        }
        if let Some(w) = &self.wiring {
            w.format(out, n);
        }
        out.print(nest, ")\n");
    }

    /// The file's text, written the way `SPECCTRA_DB::ExportPCB` writes it (quote character `"`).
    pub fn to_text(&self) -> String {
        let mut out = Out::new(self.parser.as_ref().map(|p| p.string_quote).unwrap_or('"'));
        self.format(&mut out, 0);
        out.into_string()
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------- the session

/// `HISTORY`: the history of a session. Only read; the times are not kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    pub ancestors: Vec<String>,
    pub comments: Vec<String>,
}

/// `NET_OUT`: a net of the `(network_out ..)` of a session, with the routing the router made of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetOut {
    pub net_id: String,
    pub net_number: i32,
    pub rules: Option<Rule>,
    pub wires: Vec<Wire>,
    pub wire_vias: Vec<WireVia>,
}

/// `ROUTE`: the `(routes ..)` of a session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Route {
    pub resolution: Option<UnitRes>,
    pub parser: Option<Parser>,
    pub structure_out: Option<StructureOut>,
    pub library: Option<Library>,
    pub net_outs: Vec<NetOut>,
}

impl Route {
    /// `ROUTE::GetUnits`: its own resolution, else the default of an element with none (`UNIT_RES::Default`: its parent, the session, has none either).
    pub fn units(&self) -> UnitRes {
        self.resolution.unwrap_or(UnitRes::DEFAULT)
    }
}

/// `SESSION`: a session file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Session {
    pub session_id: String,
    pub base_design: String,
    pub history: Option<History>,
    pub structure: Option<Structure>,
    pub placement: Option<Placement>,
    pub was_is: Option<Vec<(PinRef, PinRef)>>,
    pub route: Option<Route>,
}
