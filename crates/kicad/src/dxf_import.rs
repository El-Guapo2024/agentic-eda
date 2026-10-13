//! A DXF reader: the shapes of an ASCII DXF file as [`crate::graphics_import::ImportedShape`]s, in millimetres, Y down.
//!
//! Ports `DXF_IMPORT_PLUGIN` (`common/import_gfx/dxf_import_plugin.{h,cpp}` at KiCad 8303b2ad) together with the part of dxflib that
//! plugin leans on. KiCad reads a DXF with dxflib (QCAD's library) and hears about each entity through a callback; the library is
//! not part of the KiCad source this was ported from, so the reading of a file into group codes and entities is written from the DXF
//! reference, and every callback below is the plugin's own function of that name.
//!
//! What it reads, as the plugin does:
//!
//! * `LINE`, `ARC`, `CIRCLE`, `ELLIPSE` (a full or partial ellipse is a run of Bezier curves, a circle-shaped one a circle or an arc),
//!   `POINT` (a small filled circle), `LWPOLYLINE` and `POLYLINE` with `VERTEX` and `SEQEND` (lines and arcs from the bulges; a closed one
//!   gets its closing segment), `SPLINE` (a B-spline cut into cubic Bezier curves), `TEXT` and `MTEXT`;
//! * `BLOCK` and `INSERT`: a block is read once and each INSERT puts a transformed copy of it in the drawing. A block that holds an
//!   INSERT of another block is expanded where the outer one is inserted (KiCad's plugin puts such a nested INSERT straight into the
//!   drawing, at the block's own coordinates, and leaves it out of the outer block; see PARITY-pcb.md);
//! * the units: `$INSUNITS` of the header, else the unit the caller chose; the line width of an entity from its lineweight (group 370),
//!   its layer's, or the caller's default; `LAYER` and `STYLE` of the tables for that and for a text's width factor;
//! * the world and object coordinate systems: an entity with an extrusion direction other than +Z is turned into the world's axes
//!   (the "arbitrary axis algorithm"), which is what makes a circle SolidWorks exported mirrored come out the right way round.
//!
//! What it does not: dimensions, leaders, hatches, solids, traces, 3D faces, images and construction lines are reported ("... not
//! currently supported.", `ON_UNSUPPORTED`) and left out, and every other entity (`ATTRIB`, `VIEWPORT`, `MLINE`, ...) is skipped
//! without a word, as in KiCad.
//!
//! The Y axis of a DXF points up and the board's down, so every point is mirrored on the way in (`mapY`); a DXF drawn in the first
//! quadrant lands above the origin, as it does in KiCad.

use crate::graphics_import::*;

/// "minimum bulge value before resorting to a line segment; the value 0.0218 is equivalent to about 5 degrees arc".
const MIN_BULGE: f64 = 0.0218;

// `DXF_IMPORT_LINEWEIGHT_*`: the magic lineweights of the DXF specification.
const LW_BY_LAYER: i32 = -1;
const LW_BY_LW_DEFAULT: i32 = -3;

/// The deepest chain of blocks inserting blocks that is followed before it is taken for a loop.
const MAX_BLOCK_DEPTH: usize = 16;

/// `DXF_IMPORT_UNITS`: the values are the ones of `$INSUNITS` in the DXF specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DxfUnits {
    /// "Unspecified": the plugin's scale is 1 (millimetres).
    #[default]
    Default,
    Inch,
    Feet,
    Mm,
    Cm,
    Meters,
    MicroInches,
    Mils,
    Yards,
    Angstroms,
    Nanometers,
    Microns,
    Decimeters,
}

impl DxfUnits {
    /// The units `$INSUNITS` names (`setVariableInt`); the ones KiCad has no scale for (miles, kilometres, ...) are the default.
    pub fn from_insunits(v: i32) -> DxfUnits {
        match v {
            1 => DxfUnits::Inch,
            2 => DxfUnits::Feet,
            4 => DxfUnits::Mm,
            5 => DxfUnits::Cm,
            6 => DxfUnits::Meters,
            8 => DxfUnits::MicroInches,
            9 => DxfUnits::Mils,
            10 => DxfUnits::Yards,
            11 => DxfUnits::Angstroms,
            12 => DxfUnits::Nanometers,
            13 => DxfUnits::Microns,
            14 => DxfUnits::Decimeters,
            _ => DxfUnits::Default,
        }
    }

    /// `getCurrentUnitScale`: millimetres per unit.
    pub fn scale(self) -> f64 {
        match self {
            DxfUnits::Inch => 25.4,
            DxfUnits::Feet => 304.8,
            DxfUnits::Mm | DxfUnits::Default => 1.0,
            DxfUnits::Cm => 10.0,
            DxfUnits::Meters => 1000.0,
            DxfUnits::MicroInches => 2.54e-5,
            DxfUnits::Mils => 0.0254,
            DxfUnits::Yards => 914.4,
            DxfUnits::Angstroms => 1.0e-7,
            DxfUnits::Nanometers => 1.0e-6,
            DxfUnits::Microns => 1.0e-3,
            DxfUnits::Decimeters => 100.0,
        }
    }

    /// The name a request or a dialog uses.
    pub fn name(self) -> &'static str {
        match self {
            DxfUnits::Default => "default",
            DxfUnits::Inch => "inch",
            DxfUnits::Feet => "feet",
            DxfUnits::Mm => "mm",
            DxfUnits::Cm => "cm",
            DxfUnits::Meters => "meters",
            DxfUnits::MicroInches => "microinches",
            DxfUnits::Mils => "mils",
            DxfUnits::Yards => "yards",
            DxfUnits::Angstroms => "angstroms",
            DxfUnits::Nanometers => "nanometers",
            DxfUnits::Microns => "microns",
            DxfUnits::Decimeters => "decimeters",
        }
    }

    /// The unit a name stands for (`name`'s spellings, and a few the dialog's labels suggest).
    pub fn from_name(s: &str) -> Option<DxfUnits> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "" | "default" | "unspecified" => DxfUnits::Default,
            "inch" | "inches" | "in" => DxfUnits::Inch,
            "feet" | "foot" | "ft" => DxfUnits::Feet,
            "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => DxfUnits::Mm,
            "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => DxfUnits::Cm,
            "m" | "meter" | "meters" | "metre" | "metres" => DxfUnits::Meters,
            "microinch" | "microinches" => DxfUnits::MicroInches,
            "mil" | "mils" => DxfUnits::Mils,
            "yard" | "yards" => DxfUnits::Yards,
            "angstrom" | "angstroms" => DxfUnits::Angstroms,
            "nm" | "nanometer" | "nanometers" => DxfUnits::Nanometers,
            "um" | "micron" | "microns" | "micrometer" | "micrometers" => DxfUnits::Microns,
            "dm" | "decimeter" | "decimeters" => DxfUnits::Decimeters,
            _ => return None,
        })
    }
}

/// What the caller sets on the plugin before it reads a file (`DXF_IMPORT_PLUGIN::SetUnit`, `SetDefaultLineWidthMM`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxfOptions {
    /// The unit of a file that has no `$INSUNITS` (the dialog's "DXF default units").
    pub units: DxfUnits,
    /// The width of a line that has none (the dialog's "DXF default line width"), mm.
    pub line_width_mm: f64,
}

impl Default for DxfOptions {
    fn default() -> DxfOptions {
        DxfOptions { units: DxfUnits::Mm, line_width_mm: 0.2 }
    }
}

// ------------------------------------------------------------------------------------------------------------------ the reader

/// A number the way dxflib reads one (`strtod`): the longest numeric prefix, and 0 when there is none.
fn parse_real(s: &str) -> f64 {
    let t = s.trim();
    if let Ok(v) = t.parse::<f64>() {
        if v.is_finite() {
            return v;
        }
    }
    // The longest prefix that is a number ("1.5abc" -> 1.5, "." -> 0).
    let mut best = 0.0;
    for i in 1..=t.len() {
        if !t.is_char_boundary(i) {
            continue;
        }
        if let Ok(v) = t[..i].parse::<f64>() {
            if v.is_finite() {
                best = v;
            }
        }
    }
    best
}

fn parse_int(s: &str) -> i32 {
    let t = s.trim();
    if let Ok(v) = t.parse::<i32>() {
        return v;
    }
    // `atoi`: the leading integer ("4.0" -> 4).
    let mut end = 0;
    for (i, c) in t.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    t[..end].parse::<i32>().unwrap_or(0)
}

/// A record of the file: the group 0 that opens it and the groups up to the next group 0.
struct Rec<'a> {
    name: &'a str,
    g: &'a [(i32, String)],
}

impl<'a> Rec<'a> {
    /// The value of the last group with this code (dxflib keeps a map from code to value for an entity).
    fn get(&self, code: i32) -> Option<&'a str> {
        self.g.iter().rev().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }

    fn real(&self, code: i32, default: f64) -> f64 {
        self.get(code).map_or(default, parse_real)
    }

    fn int(&self, code: i32, default: i32) -> i32 {
        self.get(code).map_or(default, parse_int)
    }

    fn string(&self, code: i32, default: &str) -> String {
        self.get(code).unwrap_or(default).to_string()
    }

    fn all(&self, code: i32) -> impl Iterator<Item = &'a str> + '_ {
        self.g.iter().filter(move |(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
}

/// The attributes dxflib hands the plugin with each entity.
#[derive(Debug, Clone, Default)]
struct Attr {
    layer: String,
    width: i32,
}

struct Layer {
    name: String,
    line_weight: i32,
}

struct Style {
    name: String,
    width_factor: f64,
}

/// What a block holds: shapes already mapped to millimetres, and the INSERTs of other blocks in it.
enum BlockItem {
    Shape(ImportedShape),
    /// An INSERT of another block: the matrix, and the mapped insertion point (the block's base point is taken off when it is expanded).
    Insert { block: String, m: Mat2, ins: V2 },
}

struct Block {
    name: String,
    /// The block's base point, in the file's units.
    base: (f64, f64),
    items: Vec<BlockItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Header,
    Tables,
    Blocks,
    Entities,
    Other,
}

/// `DXF2BRD_ENTITY_DATA`, the polyline being read.
#[derive(Default)]
struct PolylineState {
    /// 0 = no entity, 1 = first vertex next, 2 = in progress.
    status: u8,
    flags: i32,
    last: V2,
    start: V2,
    bulge: f64,
}

/// The plugin's state while it reads (`DXF_IMPORT_PLUGIN`'s members).
struct Plugin {
    units: DxfUnits,
    default_thickness: f64,
    layers: Vec<Layer>,
    blocks: Vec<Block>,
    styles: Vec<Style>,
    /// `m_currentBlock`.
    current_block: Option<usize>,
    /// `m_internalImporter`.
    main: Buffer,
    messages: Vec<String>,
    bbox: Option<BBox>,
    poly: PolylineState,
    mtext: String,
    attrs: Attr,
    extrusion: [f64; 3],
    /// The table the reader is in (`LAYER`, `STYLE`, ...).
    table: String,
    /// The blocks being expanded, innermost last: an INSERT of one of them is a loop.
    active: Vec<usize>,
    nesting_reported: bool,
}

type V3 = [f64; 3];

/// `getArbitraryAxis`: the rows of the matrix are the object coordinate system's X, Y and Z axes in the world.
fn arbitrary_axis(ext: [f64; 3]) -> [V3; 3] {
    let norm = |v: V3| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l == 0.0 {
            v
        } else {
            [v[0] / l, v[1] / l, v[2] / l]
        }
    };
    let cross = |a: V3, b: V3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let z = norm(ext);
    let x = if z[0].abs() < 1.0 / 64.0 && z[1].abs() < 1.0 / 64.0 { norm(cross([0.0, 1.0, 0.0], z)) } else { norm(cross([0.0, 0.0, 1.0], z)) };
    let y = norm(cross(z, x));
    [x, y, z]
}

/// `ocsToWcs`: the world coordinates of a point given in the entity's object coordinates (the transpose of the axis matrix).
fn ocs_to_wcs(axes: &[V3; 3], p: V3) -> V3 {
    [
        axes[0][0] * p[0] + axes[1][0] * p[1] + axes[2][0] * p[2],
        axes[0][1] * p[0] + axes[1][1] * p[1] + axes[2][1] * p[2],
        axes[0][2] * p[0] + axes[1][2] * p[1] + axes[2][2] * p[2],
    ]
}

/// `toNativeString`: a DXF text without its formatting codes. MTEXT's codes are dropped or turned into KiCad's text markup (stacked
/// fractions, overbars), `\P` is a new line, `\U+XXXX` a character, and `%%c`, `%%d` and `%%p` the diameter, degree and plus-minus signs.
pub fn to_native_string(data: &str) -> String {
    let chars: Vec<char> = data.chars().collect();
    let mut res = String::new();
    let mut braces: i32 = 0;
    let mut overbar_level: i32 = -1;
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '{' => braces += 1,
            '}' => {
                if overbar_level == braces {
                    res.push('}');
                    overbar_level = -1;
                }
                braces -= 1;
            }
            '^' => {
                i += 1;
                if i >= chars.len() {
                    break;
                }
                match chars[i] {
                    'I' => res.push('\t'),
                    'J' => res.push('\u{8}'),
                    ' ' => res.push('^'),
                    _ => {}
                }
            }
            '\\' => {
                i += 1;
                if i >= chars.len() {
                    break;
                }
                match chars[i] {
                    'P' | 'X' => res.push('\n'),
                    '~' => res.push('\u{A0}'),
                    'U' => {
                        i += 2;
                        let mut hex = String::new();
                        while hex.chars().count() < 4 && i < chars.len() {
                            hex.push(chars[i]);
                            i += 1;
                        }
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().filter(|v| *v != 0).and_then(char::from_u32) {
                            res.push(c);
                        }
                        i -= 1;
                    }
                    'S' => {
                        i += 1;
                        let mut stacked = String::new();
                        while i < chars.len() {
                            if chars[i] == ';' {
                                break;
                            }
                            stacked.push(chars[i]);
                            i += 1;
                        }
                        if let Some((before, after)) = stacked.split_once('#') {
                            res.push_str(&format!("^{{{before}}}/_{{{after}}}"));
                        } else {
                            res.push_str(&stacked.replace("^ ", "/"));
                        }
                    }
                    'O' => {
                        if overbar_level == -1 {
                            res.push_str("~{");
                            overbar_level = braces;
                        }
                    }
                    'o' => {
                        if overbar_level == braces {
                            res.push('}');
                            overbar_level = -1;
                        }
                    }
                    'L' | 'l' | 'K' | 'k' | 'N' => {}
                    'p' | 'Q' | 'H' | 'W' | 'F' | 'f' | 'A' | 'C' | 'c' | 'T' => {
                        while i < chars.len() {
                            if chars[i] == ';' {
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // An escaped character: the character itself.
                        i += 1;
                        if i >= chars.len() {
                            break;
                        }
                        res.push(chars[i]);
                    }
                }
            }
            c => res.push(c),
        }
        i += 1;
    }
    if overbar_level != -1 {
        res.push('}');
    }
    // %%c diameter (U+2205, "empty set"; the sign KiCad's font has), %%d degree, %%p plus-minus.
    let mut out = String::with_capacity(res.len());
    let b: Vec<char> = res.chars().collect();
    let mut k = 0;
    while k < b.len() {
        if b[k] == '%' && k + 2 < b.len() && b[k + 1] == '%' {
            match b[k + 2] {
                'c' | 'C' => {
                    out.push('\u{2205}');
                    k += 3;
                    continue;
                }
                'd' | 'D' => {
                    out.push('\u{B0}');
                    k += 3;
                    continue;
                }
                'p' | 'P' => {
                    out.push('\u{B1}');
                    k += 3;
                    continue;
                }
                _ => {}
            }
        }
        out.push(b[k]);
        k += 1;
    }
    out
}

/// The groups of a file: the pairs of lines, a code and a value. A binary DXF and a file that is not DXF at all are refused.
fn read_groups(text: &str, messages: &mut Vec<String>) -> Result<Vec<(i32, String)>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with("AutoCAD Binary DXF") {
        return Err("This is a binary DXF file; only ASCII DXF files can be imported. Save it as an ASCII DXF.".into());
    }
    if text.trim().is_empty() {
        return Err("The DXF file is empty.".into());
    }
    if text.lines().next().is_none_or(|l| l.trim().parse::<i32>().is_err()) {
        return Err("This is not a DXF file: it does not start with a group code.".into());
    }
    let mut lines = text.lines();
    let mut groups = Vec::new();
    let mut n = 0usize;
    while let Some(code_line) = lines.next() {
        n += 1;
        let code_line = code_line.trim();
        let Some(value) = lines.next() else { break };
        n += 1;
        match code_line.parse::<i32>() {
            Ok(code) => groups.push((code, value.trim().to_string())),
            Err(_) => {
                // A line that is not a group code: dxflib stops here with what it has.
                if groups.is_empty() {
                    return Err("This is not a DXF file: it does not start with a group code.".into());
                }
                messages.push(format!("The DXF file is malformed near line {n}; the rest is ignored."));
                break;
            }
        }
    }
    if groups.is_empty() {
        return Err("The DXF file is empty.".into());
    }
    Ok(groups)
}

impl Plugin {
    fn new(opts: &DxfOptions) -> Plugin {
        Plugin {
            units: opts.units,
            default_thickness: opts.line_width_mm,
            // "placeholder layer so we can fallback to something later"
            layers: vec![Layer { name: String::new(), line_weight: LW_BY_LW_DEFAULT }],
            blocks: Vec::new(),
            styles: Vec::new(),
            current_block: None,
            main: Buffer::default(),
            messages: Vec::new(),
            bbox: None,
            poly: PolylineState::default(),
            mtext: String::new(),
            attrs: Attr::default(),
            extrusion: [0.0, 0.0, 1.0],
            table: String::new(),
            active: Vec::new(),
            nesting_reported: false,
        }
    }

    fn report(&mut self, msg: &str) {
        self.messages.push(msg.to_string());
    }

    // -- coordinate conversions from the file to mm (the Y axis flipped)

    fn unit_scale(&self) -> f64 {
        self.units.scale()
    }

    fn map_x(&self, x: f64) -> f64 {
        x * self.unit_scale()
    }

    fn map_y(&self, y: f64) -> f64 {
        -(y * self.unit_scale())
    }

    fn map_dim(&self, v: f64) -> f64 {
        v * self.unit_scale()
    }

    fn map_pt(&self, p: V3) -> V2 {
        V2::new(self.map_x(p[0]), self.map_y(p[1]))
    }

    fn update_limits(&mut self, p: V2) {
        BBox::merge_point(&mut self.bbox, p);
    }

    fn emit(&mut self, shape: ImportedShape) {
        match self.current_block {
            Some(i) => self.blocks[i].items.push(BlockItem::Shape(shape)),
            None => self.main.add_shape(shape),
        }
    }

    fn layer(&self, name: &str) -> &Layer {
        if !name.is_empty() {
            if let Some(l) = self.layers.iter().find(|l| l.name == name) {
                return l;
            }
        }
        &self.layers[0]
    }

    /// `lineWeightToWidth`: a lineweight in hundredths of a millimetre, or the layer's, or the default.
    fn line_weight_to_width(&self, mut lw: i32, layer_name: &str) -> f64 {
        if lw == LW_BY_LAYER {
            lw = self.layer(layer_name).line_weight;
        }
        if lw >= 0 {
            lw as f64 / 100.0
        } else {
            self.default_thickness
        }
    }

    /// The width of the entity being read.
    fn width(&self) -> f64 {
        self.line_weight_to_width(self.attrs.width, &self.attrs.layer)
    }

    fn style(&self, name: &str) -> Option<&Style> {
        if name.is_empty() {
            return None;
        }
        self.styles.iter().find(|s| s.name == name)
    }

    // -- the file

    fn run(&mut self, groups: &[(i32, String)]) {
        let mut section = Section::None;
        let mut i = 0;
        while i < groups.len() {
            if groups[i].0 != 0 {
                // Anything before the first group 0 is a comment (999) or a stray.
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < groups.len() && groups[j].0 != 0 {
                j += 1;
            }
            let rec = Rec { name: groups[i].1.as_str(), g: &groups[i + 1..j] };
            if rec.name == "EOF" {
                break;
            }
            self.record(&rec, &mut section);
            i = j;
        }
    }

    fn record(&mut self, rec: &Rec, section: &mut Section) {
        match rec.name {
            "SECTION" => {
                *section = match rec.get(2).unwrap_or("") {
                    "HEADER" => Section::Header,
                    "TABLES" => Section::Tables,
                    "BLOCKS" => Section::Blocks,
                    "ENTITIES" => Section::Entities,
                    _ => Section::Other,
                };
                if *section == Section::Header {
                    self.header(rec);
                }
                return;
            }
            "ENDSEC" => {
                *section = Section::None;
                return;
            }
            _ => {}
        }
        match *section {
            Section::Tables => match rec.name {
                "TABLE" => self.table = rec.string(2, ""),
                "ENDTAB" => self.table.clear(),
                "LAYER" if self.table == "LAYER" => self.add_layer(rec),
                "STYLE" if self.table == "STYLE" => self.add_text_style(rec),
                _ => {}
            },
            Section::Blocks | Section::Entities => self.entity(rec),
            _ => {}
        }
    }

    /// `setVariableInt` for the variables the plugin reads.
    fn header(&mut self, rec: &Rec) {
        let mut key = "";
        for (code, value) in rec.g {
            if *code == 9 {
                key = value.as_str();
            } else if key == "$INSUNITS" && *code == 70 {
                // "Drawing units": the file's own unit replaces the caller's; the ones KiCad has no scale for reset it to the default.
                self.units = DxfUnits::from_insunits(parse_int(value));
            }
        }
    }

    fn add_layer(&mut self, rec: &Rec) {
        let name = rec.string(2, "");
        let mut lw = rec.int(370, LW_BY_LAYER);
        if lw == LW_BY_LAYER {
            lw = LW_BY_LW_DEFAULT;
        }
        self.layers.push(Layer { name, line_weight: lw });
    }

    fn add_text_style(&mut self, rec: &Rec) {
        let name = rec.string(2, "");
        self.styles.push(Style { name, width_factor: rec.real(41, 1.0) });
    }

    // -- entities

    fn entity(&mut self, rec: &Rec) {
        self.attrs = Attr { layer: rec.string(8, "0"), width: rec.int(370, LW_BY_LAYER) };
        self.extrusion = [rec.real(210, 0.0), rec.real(220, 0.0), rec.real(230, 1.0)];
        match rec.name {
            "BLOCK" => self.add_block(rec),
            "ENDBLK" => self.current_block = None,
            "LINE" => self.add_line(rec),
            "POINT" => self.add_point(rec),
            "CIRCLE" => {
                let c = [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)];
                self.add_circle(c, rec.real(40, 0.0));
            }
            "ARC" => {
                let c = [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)];
                self.add_arc(c, rec.real(40, 0.0), rec.real(50, 0.0), rec.real(51, 0.0));
            }
            "ELLIPSE" => self.add_ellipse(rec),
            "LWPOLYLINE" => self.add_lwpolyline(rec),
            "POLYLINE" => self.add_polyline(rec.int(70, 0)),
            "VERTEX" => self.add_vertex(rec),
            "SEQEND" => self.end_entity(),
            "SPLINE" => self.add_spline(rec),
            "TEXT" => self.add_text(rec),
            "MTEXT" => self.add_mtext(rec),
            "INSERT" => self.add_insert(rec),
            "XLINE" | "RAY" => self.report("DXF construction lines not currently supported."),
            "ARCALIGNEDTEXT" => self.report("DXF arc-aligned text not currently supported."),
            "DIMENSION" | "LEADER" => self.report("DXF dimensions not currently supported."),
            "HATCH" => self.report("DXF hatches not currently supported."),
            "TRACE" => self.report("DXF traces not currently supported."),
            "3DFACE" => self.report("DXF 3dfaces not currently supported."),
            "SOLID" => self.report("DXF solids not currently supported."),
            "IMAGE" => self.report("DXF images not currently supported."),
            _ => {}
        }
    }

    fn add_block(&mut self, rec: &Rec) {
        let name = rec.string(2, "");
        self.blocks.push(Block { name, base: (rec.real(10, 0.0), rec.real(20, 0.0)), items: Vec::new() });
        self.current_block = Some(self.blocks.len() - 1);
    }

    fn add_line(&mut self, rec: &Rec) {
        let w = self.width();
        let start = V2::new(self.map_x(rec.real(10, 0.0)), self.map_y(rec.real(20, 0.0)));
        let end = V2::new(self.map_x(rec.real(11, 0.0)), self.map_y(rec.real(21, 0.0)));
        self.emit(ImportedShape::Line { start, end, stroke: Stroke::width(w) });
        self.update_limits(start);
        self.update_limits(end);
    }

    /// `addPoint`: "we emulate points with filled circles".
    fn add_point(&mut self, rec: &Rec) {
        let axes = arbitrary_axis(self.extrusion);
        let c = ocs_to_wcs(&axes, [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)]);
        let center = self.map_pt(c);
        let line_width = 0.0001;
        let thickness = self.map_dim(rec.real(39, 0.0).max(0.01));
        self.emit(ImportedShape::Circle { center, radius: thickness, stroke: Stroke::width(line_width), filled: true, fill_color: None });
        let d = V2::new(thickness, thickness);
        self.update_limits(center + d);
        self.update_limits(center - d);
    }

    fn add_circle(&mut self, c: V3, radius: f64) {
        let axes = arbitrary_axis(self.extrusion);
        let center = self.map_pt(ocs_to_wcs(&axes, c));
        let w = self.width();
        let r = self.map_dim(radius);
        self.emit(ImportedShape::Circle { center, radius: r, stroke: Stroke::width(w), filled: false, fill_color: None });
        let d = V2::new(r, r);
        self.update_limits(center + d);
        self.update_limits(center - d);
    }

    /// `addArc`: the angles are degrees, counter-clockwise, in the entity's own coordinate system.
    fn add_arc(&mut self, c: V3, radius: f64, angle1: f64, angle2: f64) {
        let axes = arbitrary_axis(self.extrusion);
        let center_w = ocs_to_wcs(&axes, c);
        let center = self.map_pt(center_w);
        let mut start_angle = angle1;
        let mut end_angle = angle2;
        // `arbAxis.GetScale()` is the diagonal's first two entries: an OCS that is mirrored in one axis turns the arc round.
        if (axes[0][0] < 0.0) != (axes[1][1] < 0.0) {
            start_angle = 180.0 - start_angle;
            end_angle = 180.0 - end_angle;
            std::mem::swap(&mut start_angle, &mut end_angle);
        }
        let sp = rotate_point(V2::new(radius, 0.0), V2::new(0.0, 0.0), -start_angle);
        let arc_start = V2::new(self.map_x(sp.x + center_w[0]), self.map_y(sp.y + center_w[1]));
        // "calculate arc angle (arcs are CCW, and should be < 0 in Pcbnew)"
        let mut angle = -(end_angle - start_angle);
        if angle > 0.0 {
            angle -= 360.0;
        }
        let w = self.width();
        let r = self.map_dim(radius);
        self.add_arc_shape(center, arc_start, angle, Stroke::width(w));
        let d = V2::new(r, r);
        self.update_limits(center + d);
        self.update_limits(center - d);
    }

    /// An arc in the importer's sense. A sweep of a full turn is a circle, and one of nothing is nothing (KiCad adds an arc of three
    /// coincident points for either).
    fn add_arc_shape(&mut self, center: V2, start: V2, angle_deg: f64, stroke: Stroke) {
        if angle_deg.abs() >= 360.0 - 1e-9 {
            self.emit(ImportedShape::Circle { center, radius: (start - center).norm(), stroke, filled: false, fill_color: None });
        } else if angle_deg.abs() > 1e-12 && (start - center).norm() > 0.0 {
            self.emit(ImportedShape::Arc { center, start, angle_deg, stroke });
        }
    }

    /// `addEllipse`: a circle or an arc when the ratio is 1, else a run of Bezier curves (`TransformEllipseToBeziers`).
    fn add_ellipse(&mut self, rec: &Rec) {
        let axes = arbitrary_axis(self.extrusion);
        let cw = ocs_to_wcs(&axes, [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)]);
        let mw = ocs_to_wcs(&axes, [rec.real(11, 0.0), rec.real(21, 0.0), rec.real(31, 0.0)]);
        let ratio = rec.real(40, 1.0);
        let center = self.map_pt(cw);
        let major = self.map_pt(mw);
        // DXF elliptical arcs store their parameters in radians; the default is the whole ellipse.
        let start_deg = rec.real(41, 0.0).to_degrees();
        let mut end_deg = rec.real(42, std::f64::consts::TAU).to_degrees();
        if start_deg > end_deg {
            end_deg += 360.0;
        }
        let w = self.width();
        if ratio == 1.0 {
            let radius = major.norm();
            let stroke = Stroke::width(w);
            if start_deg == end_deg || (end_deg - start_deg - 360.0).abs() < 1e-9 {
                self.emit(ImportedShape::Circle { center, radius, stroke, filled: false, fill_color: None });
            } else {
                // Angles are relative to the major axis (its angle in the mirrored frame).
                let rot = major.y.atan2(major.x).to_degrees();
                let s = start_deg - rot;
                let e = end_deg - rot;
                let (a_s, a_e) = (s, e);
                let mut angle = -(a_e - a_s);
                if angle > 0.0 {
                    angle -= 360.0;
                }
                let sp = rotate_point(V2::new(radius, 0.0), V2::new(0.0, 0.0), -a_s);
                // `addArc` starts from the world point; the centre here is already mapped, so the start is mapped by the same signs.
                let arc_start = V2::new(center.x + sp.x, center.y - sp.y);
                self.add_arc_shape(center, arc_start, angle, stroke);
            }
            self.update_limits(center + V2::new(radius, radius));
            self.update_limits(center - V2::new(radius, radius));
            return;
        }
        let rotation_rad = major.y.atan2(major.x);
        let major_radius = major.norm();
        let minor_radius = major_radius * ratio;
        let beziers = ellipse_to_beziers(center, major_radius, minor_radius, rotation_rad, start_deg.to_radians(), end_deg.to_radians());
        for b in beziers {
            self.emit(ImportedShape::Spline { start: b[0], c1: b[1], c2: b[2], end: b[3], stroke: Stroke::width(w) });
        }
        // "Naive bounding"
        self.update_limits(center + major);
        self.update_limits(center - major);
    }

    // -- polylines

    fn add_polyline(&mut self, flags: i32) {
        self.poly = PolylineState { status: 1, flags, ..Default::default() };
    }

    fn add_lwpolyline(&mut self, rec: &Rec) {
        self.add_polyline(rec.int(70, 0));
        // The vertices: group 10 opens one, 20 its Y, 42 its bulge.
        struct V {
            x: f64,
            y: f64,
            bulge: f64,
        }
        let mut vs: Vec<V> = Vec::new();
        for (code, value) in rec.g {
            match *code {
                10 => vs.push(V { x: parse_real(value), y: 0.0, bulge: 0.0 }),
                20 => {
                    if let Some(v) = vs.last_mut() {
                        v.y = parse_real(value);
                    }
                }
                42 => {
                    if let Some(v) = vs.last_mut() {
                        v.bulge = parse_real(value);
                    }
                }
                _ => {}
            }
        }
        for v in vs {
            self.vertex(v.x, v.y, 0.0, v.bulge, 0.0, 0.0);
        }
        self.end_entity();
    }

    fn add_vertex(&mut self, rec: &Rec) {
        self.vertex(rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0), rec.real(42, 0.0), rec.real(40, 0.0), rec.real(41, 0.0));
    }

    /// `addVertex`.
    fn vertex(&mut self, x: f64, y: f64, z: f64, bulge: f64, start_width: f64, end_width: f64) {
        if self.poly.status == 0 {
            return;
        }
        let mut line_width = self.width();
        // "support for per-vertex-encoded linewidth (Cadence uses it)": linewidths are scaled by 100 in DXF
        if start_width > 0.0 {
            line_width = start_width / 100.0;
        } else if end_width > 0.0 {
            line_width = end_width / 100.0;
        }
        let axes = arbitrary_axis(self.extrusion);
        let v = ocs_to_wcs(&axes, [x, y, z]);
        if self.poly.status == 1 {
            self.poly.last = V2::new(self.map_x(v[0]), self.map_y(v[1]));
            self.poly.start = self.poly.last;
            self.poly.bulge = bulge;
            self.poly.status = 2;
            return;
        }
        let seg_end = V2::new(self.map_x(v[0]), self.map_y(v[1]));
        let last = self.poly.last;
        if self.poly.bulge.abs() < MIN_BULGE {
            self.insert_line(last, seg_end, line_width);
        } else {
            self.insert_arc(last, seg_end, self.poly.bulge, line_width);
        }
        self.poly.last = seg_end;
        self.poly.bulge = bulge;
    }

    /// `endEntity`: a closed polyline gets its closing segment.
    fn end_entity(&mut self) {
        if self.poly.status != 0 && self.poly.flags & 1 != 0 {
            let w = self.width();
            let (last, start, bulge) = (self.poly.last, self.poly.start, self.poly.bulge);
            if bulge.abs() < MIN_BULGE {
                self.insert_line(last, start, w);
            } else {
                self.insert_arc(last, start, bulge, w);
            }
        }
        self.poly = PolylineState::default();
    }

    fn insert_line(&mut self, a: V2, b: V2, width: f64) {
        self.emit(ImportedShape::Line { start: a, end: b, stroke: Stroke::width(width) });
        self.update_limits(a);
        self.update_limits(b);
    }

    /// `insertArc`: the arc of a polyline segment from its bulge (the tangent of a quarter of the arc's angle).
    fn insert_arc(&mut self, seg_start: V2, seg_end: V2, bulge: f64, width: f64) {
        // "ensure aBulge represents an angle from +/- ( 0 .. approx 359.8 deg )"
        let bulge = bulge.clamp(-2000.0, 2000.0);
        let ang = 4.0 * bulge.atan();
        // "reflect the Y values to put everything in a RHCS"
        let sp = V2::new(seg_start.x, -seg_start.y);
        let ep = V2::new(seg_end.x, -seg_end.y);
        let mut off_ang = (ep.y - sp.y).atan2(ep.x - sp.x);
        // "length of subtended segment = 1/2 distance between the 2 points"
        let d = 0.5 * ((sp.x - ep.x) * (sp.x - ep.x) + (sp.y - ep.y) * (sp.y - ep.y)).sqrt();
        if d == 0.0 {
            return;
        }
        let xm = (sp.x + ep.x) * 0.5;
        let ym = (sp.y + ep.y) * 0.5;
        let mut radius = d / (ang * 0.5).sin();
        if radius < 0.0 {
            radius = -radius;
        }
        // "calculate the height of the triangle with base d and hypotenuse r"
        let mut dh2 = radius * radius - d * d;
        if dh2 < 0.0 {
            dh2 = 0.0;
        }
        let h = dh2.sqrt();
        if ang < 0.0 {
            off_ang -= std::f64::consts::FRAC_PI_2;
        } else {
            off_ang += std::f64::consts::FRAC_PI_2;
        }
        // "for angles greater than 180 deg we need to flip the direction in which the arc center is found relative to the midpoint"
        if ang < -std::f64::consts::PI {
            off_ang += std::f64::consts::PI;
        } else if ang > std::f64::consts::PI {
            off_ang -= std::f64::consts::PI;
        }
        let cx = h * off_ang.cos() + xm;
        let cy = h * off_ang.sin() + ym;
        let center = V2::new(cx, -cy);
        let (arc_start, angle) = if ang < 0.0 { (V2::new(ep.x, -ep.y), ang.to_degrees()) } else { (V2::new(sp.x, -sp.y), -ang.to_degrees()) };
        self.add_arc_shape(center, arc_start, angle, Stroke::width(width));
        let r = V2::new(radius, radius);
        self.update_limits(center + r);
        self.update_limits(center - r);
    }

    // -- splines

    fn add_spline(&mut self, rec: &Rec) {
        let degree = rec.int(71, 3);
        let knots: Vec<f64> = rec.all(40).map(parse_real).collect();
        // Control points: group 10 opens one, 20 its Y.
        let mut ctrl: Vec<V2> = Vec::new();
        for (code, value) in rec.g {
            match *code {
                10 => ctrl.push(V2::new(parse_real(value), 0.0)),
                20 => {
                    if let Some(p) = ctrl.last_mut() {
                        p.y = parse_real(value);
                    }
                }
                _ => {}
            }
        }
        let width = self.width();
        self.insert_spline(degree, &ctrl, &knots, width);
    }

    /// `insertSpline`: the B-spline as cubic Bezier curves. Weights are ignored, as in KiCad. A spline of degree 1 or 2 is raised to
    /// degree 3 first (so a degree-1 spline comes out as lines, once the importer finds the curves flat); one of a higher degree is
    /// drawn as lines (KiCad takes the first four control points of each piece, which is not the piece).
    fn insert_spline(&mut self, degree: i32, ctrl: &[V2], knots: &[f64], width: f64) {
        if ctrl.len() < 2 {
            // "malformed spline"
            return;
        }
        let pieces = match bspline_to_beziers(degree, ctrl, knots) {
            Some(p) => p,
            None => {
                self.report("Invalid spline definition encountered");
                return;
            }
        };
        let u = self.unit_scale();
        let map = |p: V2| V2::new(p.x * u, -(p.y * u));
        let mut out: Vec<ImportedShape> = Vec::new();
        for piece in pieces {
            let pts: Vec<V2> = piece.iter().map(|p| map(*p)).collect();
            if pts.len() == 4 {
                out.push(ImportedShape::Spline { start: pts[0], c1: pts[1], c2: pts[2], end: pts[3], stroke: Stroke::width(width) });
            } else {
                // A piece of a higher degree: its points, sampled.
                const SAMPLES: usize = 16;
                let mut prev = pts[0];
                for i in 1..=SAMPLES {
                    let p = de_casteljau(&pts, i as f64 / SAMPLES as f64);
                    out.push(ImportedShape::Line { start: prev, end: p, stroke: Stroke::width(width) });
                    prev = p;
                }
            }
            for p in &pts {
                self.update_limits(*p);
            }
        }
        for s in out {
            self.emit(s);
        }
    }

    // -- text

    fn add_text(&mut self, rec: &Rec) {
        let axes = arbitrary_axis(self.extrusion);
        let ref_w = ocs_to_wcs(&axes, [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)]);
        let nan0 = |v: f64| if v.is_nan() { 0.0 } else { v };
        let sec_w = ocs_to_wcs(&axes, [nan0(rec.real(11, f64::NAN)), nan0(rec.real(21, f64::NAN)), nan0(rec.real(31, f64::NAN))]);
        let mut ref_point = self.map_pt(ref_w);
        let mut sec_point = self.map_pt(sec_w);
        let hj = rec.int(72, 0);
        let vj = rec.int(73, 0);
        if (vj != 0 || hj != 0 || hj == 4) && hj != 3 && hj != 5 {
            std::mem::swap(&mut ref_point, &mut sec_point);
        }
        let text = to_native_string(&rec.string(1, ""));
        let style_factor = self.style(&rec.string(7, "")).map(|s| s.width_factor);
        let text_height = self.map_dim(rec.real(40, 2.5));
        // "The 0.9 factor gives a better height/width base ratio with our font"
        let mut char_width = text_height * 0.9;
        if let Some(f) = style_factor {
            char_width *= f;
        }
        let text_width = char_width * text.chars().count() as f64;
        let thickness = text_height / 8.0;
        let v = match vj {
            2 => VJustify::Center,
            3 => VJustify::Top,
            _ => VJustify::Bottom,
        };
        let h = match hj {
            1 | 4 => HJustify::Center,
            2 => HJustify::Right,
            _ => HJustify::Left,
        };
        let angle_degree = rec.real(50, 0.0);
        self.emit_text(ref_point, text, text_height, char_width, thickness, angle_degree, h, v, text_width);
    }

    fn add_mtext(&mut self, rec: &Rec) {
        // "If the text string is greater than 250 characters, the string is divided into 250-character chunks, which appear in one or
        // more group 3 codes. If group 3 codes are used, the last group is a group 1."
        self.mtext.clear();
        for chunk in rec.all(3) {
            self.mtext.push_str(chunk);
        }
        self.mtext.push_str(rec.get(1).unwrap_or(""));
        let text = to_native_string(&std::mem::take(&mut self.mtext));
        let style_factor = self.style(&rec.string(7, "")).map(|s| s.width_factor);
        let text_height = self.map_dim(rec.real(40, 2.5));
        let mut char_width = text_height * 0.9;
        if let Some(f) = style_factor {
            char_width *= f;
        }
        let text_width = char_width * text.chars().count() as f64;
        let thickness = text_height / 8.0;
        let axes = arbitrary_axis(self.extrusion);
        let pos_w = ocs_to_wcs(&axes, [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)]);
        let textpos = self.map_pt(pos_w);
        let ap = rec.int(71, 1);
        let v = if ap <= 3 {
            VJustify::Top
        } else if ap <= 6 {
            VJustify::Center
        } else {
            VJustify::Bottom
        };
        let h = match ap % 3 {
            1 => HJustify::Left,
            2 => HJustify::Center,
            _ => HJustify::Right,
        };
        // dxflib: the text direction is the X direction vector (group 11, 21) when there is one, else the angle (group 50, degrees).
        let angle_degree = if rec.get(11).is_some() {
            let (x, y) = (rec.real(11, 0.0), rec.real(21, 0.0));
            if x.abs() < 1.0e-6 {
                if y > 0.0 {
                    90.0
                } else {
                    270.0
                }
            } else {
                (y / x).atan().to_degrees()
            }
        } else {
            rec.real(50, 0.0)
        };
        self.emit_text(textpos, text, text_height, char_width, thickness, angle_degree, h, v, text_width);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_text(&mut self, at: V2, text: String, height: f64, char_width: f64, thickness: f64, angle_deg: f64, h: HJustify, v: VJustify, text_width: f64) {
        // The corners of the text's box, for the picture's limits.
        let (lx, rx) = match h {
            HJustify::Left => (0.0, text_width),
            HJustify::Center => (-text_width / 2.0, text_width / 2.0),
            HJustify::Right => (-text_width, 0.0),
        };
        let (by, ty) = match v {
            VJustify::Bottom => (0.0, height),
            VJustify::Center => (-height / 2.0, height / 2.0),
            VJustify::Top => (-height, 0.0),
        };
        let (s, c) = angle_deg.to_radians().sin_cos();
        for (x, y) in [(lx, by), (rx, by), (lx, ty), (rx, ty)] {
            self.update_limits(V2::new(at.x + x * c - y * s, at.y + x * s + y * c));
        }
        self.emit(ImportedShape::Text { origin: at, text, height, width: char_width, thickness, orientation_deg: angle_deg, h, v });
    }

    // -- blocks

    /// `addInsert`: a copy of the block, turned, scaled and moved. Inside a block definition the INSERT is kept and expanded where
    /// that block is inserted.
    fn add_insert(&mut self, rec: &Rec) {
        let name = rec.string(2, "");
        let (m, ins) = self.insert_transform(rec);
        match self.current_block {
            Some(i) => {
                if !name.is_empty() {
                    self.blocks[i].items.push(BlockItem::Insert { block: name, m, ins });
                }
            }
            None => {
                // A block that is not defined is skipped, as in KiCad.
                if self.blocks.iter().any(|b| b.name == name) {
                    let t = ins - self.mapped_base(&name);
                    let mut shapes = Vec::new();
                    self.expand(&name, m, t, 0, &mut shapes);
                    for s in shapes {
                        self.main.add_shape(s);
                    }
                }
            }
        }
    }

    /// The block's base point, mapped.
    fn mapped_base(&self, name: &str) -> V2 {
        let base = self.blocks.iter().find(|b| b.name == name).map_or((0.0, 0.0), |b| b.base);
        V2::new(self.map_x(base.0), self.map_y(base.1))
    }

    /// The matrix of an INSERT, `(arbAxis * rot) * scale`, and its insertion point, mapped.
    fn insert_transform(&self, rec: &Rec) -> (Mat2, V2) {
        let axes = arbitrary_axis(self.extrusion);
        // DL_InsertData's angle is in degrees; the board's Y axis points down, so the turn is the other way.
        let rot = Mat2::rotation((-rec.real(50, 0.0)).to_radians());
        let scale = Mat2::scale(rec.real(41, 1.0), rec.real(42, 1.0));
        let arb = Mat2 { a: axes[0][0], b: axes[0][1], c: axes[1][0], d: axes[1][1] };
        let m = arb.mul(rot).mul(scale);
        let ins = ocs_to_wcs(&axes, [rec.real(10, 0.0), rec.real(20, 0.0), rec.real(30, 0.0)]);
        (m, V2::new(self.map_x(ins[0]), self.map_y(ins[1])))
    }

    /// The shapes of a block, transformed by `m` and `t` (`p -> m p + t`), with the blocks it inserts expanded in turn.
    fn expand(&mut self, name: &str, m: Mat2, t: V2, depth: usize, out: &mut Vec<ImportedShape>) {
        let Some(bi) = self.blocks.iter().position(|b| b.name == name) else { return };
        if depth > MAX_BLOCK_DEPTH || self.active.contains(&bi) {
            if !self.nesting_reported {
                self.nesting_reported = true;
                self.report("DXF blocks insert each other in a loop or too deeply; the inner ones are left out.");
            }
            return;
        }
        self.active.push(bi);
        // The items are taken out and put back: expanding a nested INSERT needs the other blocks while this one is read.
        let items = std::mem::take(&mut self.blocks[bi].items);
        for item in &items {
            match item {
                BlockItem::Shape(s) => {
                    let mut c = s.clone();
                    c.transform(m, t);
                    out.push(c);
                }
                BlockItem::Insert { block, m: mi, ins } => {
                    // The nested block's shapes: p -> Mi p + (ins - base) inside this block, so in the drawing M (Mi p + ti) + t.
                    let ti = *ins - self.mapped_base(block);
                    let total = m.mul(*mi);
                    let moved = m.apply(ti) + t;
                    self.expand(block, total, moved, depth + 1, out);
                }
            }
        }
        self.blocks[bi].items = items;
        self.active.pop();
    }
}

// ------------------------------------------------------------------------------------------------------------ curves

/// `TransformEllipseToBeziers`: an ellipse or elliptical arc as cubic Bezier curves, one per quarter turn at most. `rotation_rad` is the
/// angle of the major axis, the parameters are the ellipse's own (measured from the major axis, counter-clockwise).
fn ellipse_to_beziers(center: V2, major: f64, minor: f64, rotation_rad: f64, start_rad: f64, end_rad: f64) -> Vec<[V2; 4]> {
    let mut arc_angle = -(end_rad - start_rad);
    if arc_angle >= 0.0 {
        arc_angle -= std::f64::consts::TAU;
    }
    const MIN_BEZIERS_PER_CIRCLE: f64 = 4.0;
    let num = (arc_angle / (std::f64::consts::TAU / MIN_BEZIERS_PER_CIRCLE)).abs().ceil() as usize;
    if num == 0 {
        return Vec::new();
    }
    let inc = arc_angle / num as f64;
    // k = 4/3 tan(theta/4): the distance of a control point from its end point that makes the curve hug the circle.
    let k = (4.0 / 3.0) * (inc / 4.0).tan();
    let first = [V2::new(1.0, 0.0), V2::new(1.0, k), V2::new(inc.cos() + k * inc.sin(), inc.sin() - k * inc.cos()), V2::new(inc.cos(), inc.sin())];
    // `RotatePoint( p, angle )` in radians: x' = x cos + y sin, y' = y cos - x sin.
    let rot = |p: V2, a: f64| {
        let (s, c) = a.sin_cos();
        V2::new(p.x * c + p.y * s, p.y * c - p.x * s)
    };
    let tf = |p: V2, angle: f64| {
        // "Bring to the actual starting angle", scale to the radii, rotate to the ellipse's own coordinate system, move to its centre.
        let p = rot(p, -(angle - start_rad));
        let p = V2::new(p.x * major, p.y * minor);
        let p = rot(p, -rotation_rad);
        p + center
    };
    (0..num).map(|i| [tf(first[0], i as f64 * inc), tf(first[1], i as f64 * inc), tf(first[2], i as f64 * inc), tf(first[3], i as f64 * inc)]).collect()
}

/// A point of a Bezier curve of any degree.
fn de_casteljau(pts: &[V2], t: f64) -> V2 {
    let mut p: Vec<V2> = pts.to_vec();
    for r in 1..p.len() {
        for i in 0..p.len() - r {
            p[i] = p[i] * (1.0 - t) + p[i + 1] * t;
        }
    }
    p[0]
}

/// The point of a blossom (the polar form of the B-spline) with the given arguments, by de Boor's scheme on span `k`.
fn blossom(p: usize, k: usize, ctrl: &[V2], knots: &[f64], args: &[f64]) -> V2 {
    let mut d: Vec<V2> = (0..=p).map(|i| ctrl[k - p + i]).collect();
    for r in 1..=p {
        let t = args[r - 1];
        for i in (r..=p).rev() {
            let gi = k - p + i;
            let denom = knots[gi + p - r + 1] - knots[gi];
            let alpha = if denom == 0.0 { 0.0 } else { (t - knots[gi]) / denom };
            d[i] = d[i - 1] * (1.0 - alpha) + d[i] * alpha;
        }
    }
    d[p]
}

/// `tinyspline`'s `elevateDegree` and `toBeziers`, as the plugin uses them: a B-spline of `degree` through `ctrl` with `knots` as
/// the Bezier pieces of its domain, each raised to degree 3 when the spline's degree is lower. `None` for a definition tinyspline
/// refuses: a degree that is not below the number of control points, a knot vector of the wrong length, one that decreases, a knot
/// repeated more than the order, or a domain with nothing in it.
pub fn bspline_to_beziers(degree: i32, ctrl: &[V2], knots: &[f64]) -> Option<Vec<Vec<V2>>> {
    if degree < 1 {
        return None;
    }
    let p = degree as usize;
    let n = ctrl.len();
    if p >= n || knots.len() != n + p + 1 {
        return None;
    }
    if knots.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    // A knot repeated more than the order makes no sense.
    let mut run = 1;
    for w in knots.windows(2) {
        run = if w[1] == w[0] { run + 1 } else { 1 };
        if run > p + 1 {
            return None;
        }
    }
    if knots[n] <= knots[p] {
        return None;
    }
    let mut out = Vec::new();
    for k in p..n {
        let (a, b) = (knots[k], knots[k + 1]);
        if b <= a {
            continue;
        }
        let mut piece: Vec<V2> = (0..=p)
            .map(|j| {
                let args: Vec<f64> = (0..p).map(|i| if i < p - j { a } else { b }).collect();
                blossom(p, k, ctrl, knots, &args)
            })
            .collect();
        // Raise the degree of the piece to 3.
        while piece.len() < 4 {
            let deg = piece.len() - 1;
            let mut up = Vec::with_capacity(piece.len() + 1);
            for i in 0..=deg + 1 {
                let f = i as f64 / (deg + 1) as f64;
                let prev = if i > 0 { piece[i - 1] } else { V2::default() };
                let cur = if i <= deg { piece[i] } else { V2::default() };
                up.push(prev * f + cur * (1.0 - f));
            }
            piece = up;
        }
        out.push(piece);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------------------------------- entry

/// Reads an ASCII DXF file. `Err` is a file that cannot be read at all (binary, empty, not DXF); everything the plugin cannot handle is
/// a message in the result.
pub fn read_dxf(text: &str, opts: &DxfOptions) -> Result<Imported, String> {
    let mut messages = Vec::new();
    let groups = read_groups(text, &mut messages)?;
    let mut plugin = Plugin::new(opts);
    plugin.messages = messages;
    plugin.run(&groups);
    Ok(Imported { shapes: plugin.main.shapes, messages: plugin.messages, bbox: plugin.bbox })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF text from (record name, groups) pairs, with the section markers around.
    fn dxf(header: &[(i32, &str)], tables: &[(&str, Vec<(i32, &str)>)], blocks: &[(&str, Vec<(i32, &str)>)], entities: &[(&str, Vec<(i32, &str)>)]) -> String {
        let mut s = String::new();
        let mut put = |code: i32, v: &str| {
            s.push_str(&format!("{code}\n{v}\n"));
        };
        put(0, "SECTION");
        put(2, "HEADER");
        for (c, v) in header {
            put(*c, v);
        }
        put(0, "ENDSEC");
        if !tables.is_empty() {
            put(0, "SECTION");
            put(2, "TABLES");
            for (name, g) in tables {
                put(0, "TABLE");
                put(2, name);
                let entry = name.to_string();
                put(0, &entry);
                for (c, v) in g {
                    put(*c, v);
                }
                put(0, "ENDTAB");
            }
            put(0, "ENDSEC");
        }
        if !blocks.is_empty() {
            put(0, "SECTION");
            put(2, "BLOCKS");
            for (name, g) in blocks {
                put(0, name);
                for (c, v) in g {
                    put(*c, v);
                }
            }
            put(0, "ENDSEC");
        }
        put(0, "SECTION");
        put(2, "ENTITIES");
        for (name, g) in entities {
            put(0, name);
            for (c, v) in g {
                put(*c, v);
            }
        }
        put(0, "ENDSEC");
        put(0, "EOF");
        s
    }

    fn read(entities: &[(&str, Vec<(i32, &str)>)]) -> Imported {
        read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "4")], &[], &[], entities), &DxfOptions::default()).unwrap()
    }

    fn lines(i: &Imported) -> Vec<(V2, V2, f64)> {
        i.shapes
            .iter()
            .filter_map(|s| match s {
                ImportedShape::Line { start, end, stroke } => Some((*start, *end, stroke.width)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_line_is_mirrored_in_y_and_takes_the_default_width() {
        let i = read(&[("LINE", vec![(8, "0"), (10, "1"), (20, "2"), (30, "0"), (11, "11"), (21, "2"), (31, "0")])]);
        assert_eq!(lines(&i), vec![(V2::new(1.0, -2.0), V2::new(11.0, -2.0), 0.2)]);
        let b = i.bbox.unwrap();
        assert_eq!((b.min, b.max), (V2::new(1.0, -2.0), V2::new(11.0, -2.0)));
    }

    #[test]
    fn the_units_come_from_the_header_else_from_the_caller() {
        fn line(x: &str) -> Vec<(&'static str, Vec<(i32, &str)>)> {
            vec![("LINE", vec![(10, "0"), (20, "0"), (11, x), (21, "0")])]
        }
        let inch = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "1")], &[], &[], &line("2")), &DxfOptions::default()).unwrap();
        assert_eq!(lines(&inch)[0].1, V2::new(50.8, 0.0));
        // No $INSUNITS: the caller's unit.
        let none = read_dxf(&dxf(&[], &[], &[], &line("2")), &DxfOptions { units: DxfUnits::Inch, line_width_mm: 0.2 }).unwrap();
        assert_eq!(lines(&none)[0].1, V2::new(50.8, 0.0));
        // The file's own unit wins over the caller's.
        let both = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "4")], &[], &[], &line("2")), &DxfOptions { units: DxfUnits::Inch, line_width_mm: 0.2 }).unwrap();
        assert_eq!(lines(&both)[0].1, V2::new(2.0, 0.0));
        // "Unspecified" resets to millimetres (the plugin's own behaviour).
        let zero = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "0")], &[], &[], &line("2")), &DxfOptions { units: DxfUnits::Inch, line_width_mm: 0.2 }).unwrap();
        assert_eq!(lines(&zero)[0].1, V2::new(2.0, 0.0));
    }

    #[test]
    fn a_lineweight_is_hundredths_of_a_millimetre_and_by_layer_takes_the_layers() {
        let tables = [("LAYER", vec![(2, "OUT"), (70, "0"), (370, "50")])];
        let ents = [
            ("LINE", vec![(8, "OUT"), (10, "0"), (20, "0"), (11, "1"), (21, "0")]),
            ("LINE", vec![(8, "OUT"), (370, "25"), (10, "0"), (20, "0"), (11, "1"), (21, "0")]),
            ("LINE", vec![(8, "0"), (10, "0"), (20, "0"), (11, "1"), (21, "0")]),
        ];
        let i = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "4")], &tables, &[], &ents), &DxfOptions { units: DxfUnits::Mm, line_width_mm: 0.3 }).unwrap();
        let w: Vec<f64> = lines(&i).iter().map(|l| l.2).collect();
        assert_eq!(w, vec![0.5, 0.25, 0.3]);
    }

    #[test]
    fn an_arc_runs_counter_clockwise_and_comes_out_with_the_importers_negative_sweep() {
        let i = read(&[("ARC", vec![(10, "0"), (20, "0"), (30, "0"), (40, "10"), (50, "0"), (51, "90")])]);
        let ImportedShape::Arc { center, start, angle_deg, .. } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
        assert_eq!(center, V2::new(0.0, 0.0));
        assert!((start.x - 10.0).abs() < 1e-9 && start.y.abs() < 1e-9);
        assert!((angle_deg + 90.0).abs() < 1e-9);
        // The end of the sweep is the DXF point (0, 10), which is (0, -10) with the Y axis down.
        let (_, mid, end) = arc_points(center, start, angle_deg);
        assert!(end.x.abs() < 1e-9 && (end.y + 10.0).abs() < 1e-9, "{end:?}");
        assert!(mid.x > 0.0 && mid.y < 0.0);
    }

    #[test]
    fn an_arc_through_zero_degrees_sweeps_the_short_way_round() {
        // From 270 to 90 is half a turn through 0 degrees.
        let i = read(&[("ARC", vec![(10, "0"), (20, "0"), (40, "5"), (50, "270"), (51, "90")])]);
        let ImportedShape::Arc { angle_deg, .. } = i.shapes[0] else { panic!() };
        assert!((angle_deg + 180.0).abs() < 1e-9, "{angle_deg}");
    }

    #[test]
    fn a_circle_is_a_circle_and_a_point_a_small_filled_one() {
        let i = read(&[("CIRCLE", vec![(10, "3"), (20, "4"), (40, "2.5")]), ("POINT", vec![(10, "0"), (20, "0")])]);
        assert!(matches!(i.shapes[0], ImportedShape::Circle { center, radius, filled: false, .. } if center == V2::new(3.0, -4.0) && radius == 2.5));
        assert!(matches!(i.shapes[1], ImportedShape::Circle { radius, filled: true, .. } if (radius - 0.01).abs() < 1e-12));
    }

    #[test]
    fn a_closed_lwpolyline_gets_its_closing_segment() {
        let i = read(&[("LWPOLYLINE", vec![(90, "4"), (70, "1"), (10, "0"), (20, "0"), (10, "10"), (20, "0"), (10, "10"), (20, "5"), (10, "0"), (20, "5")])]);
        let l = lines(&i);
        assert_eq!(l.len(), 4, "{:?}", i.shapes);
        assert_eq!(l[3].0, V2::new(0.0, -5.0));
        assert_eq!(l[3].1, V2::new(0.0, 0.0));
    }

    #[test]
    fn an_open_lwpolyline_has_one_segment_fewer_than_vertices() {
        let i = read(&[("LWPOLYLINE", vec![(90, "3"), (70, "0"), (10, "0"), (20, "0"), (10, "10"), (20, "0"), (10, "10"), (20, "5")])]);
        assert_eq!(lines(&i).len(), 2);
    }

    #[test]
    fn a_bulge_of_one_is_a_half_circle() {
        // From (0,0) to (10,0) with bulge 1: a half circle of radius 5 centred at (5, 0).
        let i = read(&[("LWPOLYLINE", vec![(90, "2"), (70, "0"), (10, "0"), (20, "0"), (42, "1"), (10, "10"), (20, "0")])]);
        assert_eq!(i.shapes.len(), 1);
        let ImportedShape::Arc { center, start, angle_deg, .. } = i.shapes[0].clone() else { panic!("{:?}", i.shapes) };
        assert!((center.x - 5.0).abs() < 1e-9 && center.y.abs() < 1e-9, "{center:?}");
        assert!((angle_deg.abs() - 180.0).abs() < 1e-9, "{angle_deg}");
        let (s, _, e) = arc_points(center, start, angle_deg);
        // The sweep joins the two vertices (in either order, as an arc).
        let ends = [(s.x, s.y), (e.x, e.y)];
        assert!(ends.iter().any(|p| p.0.abs() < 1e-9 && p.1.abs() < 1e-9) && ends.iter().any(|p| (p.0 - 10.0).abs() < 1e-9 && p.1.abs() < 1e-9), "{ends:?}");
        // A counter-clockwise bulge (positive) bows to the right of the direction of travel in DXF's frame: below the chord, which is
        // above the chord (y negative) on the board once the Y axis is mirrored? The bow of bulge +1 from (0,0) to (10,0) is below
        // the line in DXF coordinates (y < 0), so above it (y > 0 in the board's) here.
        let (_, mid, _) = arc_points(center, start, angle_deg);
        assert!(mid.y > 0.0, "{mid:?}");
    }

    #[test]
    fn a_small_bulge_is_a_straight_segment() {
        let i = read(&[("LWPOLYLINE", vec![(90, "2"), (70, "0"), (10, "0"), (20, "0"), (42, "0.01"), (10, "10"), (20, "0")])]);
        assert_eq!(lines(&i).len(), 1);
    }

    #[test]
    fn a_polyline_with_vertices_and_a_seqend_is_the_same_as_an_lwpolyline() {
        let vs = |x: &'static str, y: &'static str| ("VERTEX", vec![(8, "0"), (10, x), (20, y), (30, "0")]);
        let i = read(&[("POLYLINE", vec![(8, "0"), (66, "1"), (70, "1")]), vs("0", "0"), vs("10", "0"), vs("10", "5"), ("SEQEND", vec![(8, "0")])]);
        assert_eq!(lines(&i).len(), 3, "two sides and the closing one: {:?}", i.shapes);
    }

    #[test]
    fn an_ellipse_is_a_run_of_bezier_curves_and_a_round_one_a_circle() {
        let i = read(&[("ELLIPSE", vec![(10, "0"), (20, "0"), (30, "0"), (11, "10"), (21, "0"), (31, "0"), (40, "0.5"), (41, "0"), (42, "6.283185307179586")])]);
        assert_eq!(i.shapes.len(), 4, "a quarter turn each");
        let ImportedShape::Spline { start, end, .. } = i.shapes[0] else { panic!() };
        // The curves leave the major axis' end point (10, 0) and the minor axis' end is (0, 5) up to the Y mirror.
        assert!((start.x - 10.0).abs() < 1e-9 && start.y.abs() < 1e-9, "{start:?}");
        assert!(end.x.abs() < 1e-9 && (end.y.abs() - 5.0).abs() < 1e-9, "{end:?}");
        let round = read(&[("ELLIPSE", vec![(10, "0"), (20, "0"), (11, "4"), (21, "0"), (40, "1"), (41, "0"), (42, "6.283185307179586")])]);
        assert!(matches!(round.shapes[..], [ImportedShape::Circle { radius, .. }] if (radius - 4.0).abs() < 1e-9), "{:?}", round.shapes);
    }

    #[test]
    fn a_spline_is_cut_into_cubic_curves() {
        // A cubic Bezier as a clamped B-spline.
        let i = read(&[("SPLINE", vec![(70, "8"), (71, "3"), (72, "8"), (73, "4"), (40, "0"), (40, "0"), (40, "0"), (40, "0"), (40, "1"), (40, "1"), (40, "1"), (40, "1"), (10, "0"), (20, "0"), (10, "0"), (20, "10"), (10, "10"), (20, "10"), (10, "10"), (20, "0")])]);
        assert_eq!(i.shapes.len(), 1, "{:?}", i.shapes);
        let ImportedShape::Spline { start, c1, c2, end, .. } = i.shapes[0] else { panic!() };
        assert_eq!((start, c1, c2, end), (V2::new(0.0, 0.0), V2::new(0.0, -10.0), V2::new(10.0, -10.0), V2::new(10.0, 0.0)));
    }

    #[test]
    fn a_degree_one_spline_comes_out_as_straight_curves_and_a_bad_one_is_reported() {
        let ok = read(&[("SPLINE", vec![(71, "1"), (72, "5"), (73, "3"), (40, "0"), (40, "0"), (40, "1"), (40, "2"), (40, "2"), (10, "0"), (20, "0"), (10, "10"), (20, "0"), (10, "10"), (20, "10")])]);
        assert_eq!(ok.shapes.len(), 2);
        let ImportedShape::Spline { start, c1, c2, end, .. } = ok.shapes[0] else { panic!() };
        assert_eq!((start, end), (V2::new(0.0, 0.0), V2::new(10.0, 0.0)));
        assert!((c1.x - 10.0 / 3.0).abs() < 1e-9 && (c2.x - 20.0 / 3.0).abs() < 1e-9);
        // Two knots too few.
        let bad = read(&[("SPLINE", vec![(71, "3"), (72, "3"), (73, "4"), (40, "0"), (40, "0"), (40, "1"), (10, "0"), (20, "0"), (10, "1"), (20, "1"), (10, "2"), (20, "0"), (10, "3"), (20, "1")])]);
        assert!(bad.shapes.is_empty());
        assert_eq!(bad.messages, vec!["Invalid spline definition encountered".to_string()]);
    }

    #[test]
    fn text_is_placed_at_its_justification_point_with_kicads_sizes() {
        let i = read(&[("TEXT", vec![(10, "5"), (20, "5"), (40, "2"), (1, "ABCD"), (50, "90")])]);
        let ImportedShape::Text { origin, text, height, width, thickness, orientation_deg, h, v } = i.shapes[0].clone() else { panic!() };
        assert_eq!((origin, text.as_str(), h, v), (V2::new(5.0, -5.0), "ABCD", HJustify::Left, VJustify::Bottom));
        assert!((height - 2.0).abs() < 1e-9 && (width - 1.8).abs() < 1e-9 && (thickness - 0.25).abs() < 1e-9 && (orientation_deg - 90.0).abs() < 1e-9);
        // Centred text: the alignment point (group 11) is where it goes.
        let c = read(&[("TEXT", vec![(10, "0"), (20, "0"), (11, "7"), (21, "8"), (40, "1"), (1, "x"), (72, "1"), (73, "2")])]);
        let ImportedShape::Text { origin, h, v, .. } = c.shapes[0].clone() else { panic!() };
        assert_eq!((origin, h, v), (V2::new(7.0, -8.0), HJustify::Center, VJustify::Center));
    }

    #[test]
    fn mtext_formatting_codes_are_dropped_and_its_chunks_joined() {
        assert_eq!(to_native_string("{\\fArial|b0;Hello}\\Pworld %%c10 %%d"), "Hello\nworld \u{2205}10 \u{B0}");
        assert_eq!(to_native_string("\\S1#2;"), "^{1}/_{2}");
        assert_eq!(to_native_string("\\Oover\\o"), "~{over}");
        let i = read(&[("MTEXT", vec![(10, "0"), (20, "0"), (40, "2"), (71, "5"), (3, "ab"), (1, "cd")])]);
        let ImportedShape::Text { text, h, v, .. } = i.shapes[0].clone() else { panic!() };
        assert_eq!((text.as_str(), h, v), ("abcd", HJustify::Center, VJustify::Center));
    }

    #[test]
    fn an_insert_places_a_turned_scaled_copy_of_a_block() {
        let blocks = [("BLOCK", vec![(2, "B"), (10, "0"), (20, "0")]), ("LINE", vec![(10, "1"), (20, "0"), (11, "2"), (21, "0")]), ("ENDBLK", vec![])];
        let ents = [("INSERT", vec![(2, "B"), (10, "10"), (20, "10"), (41, "2"), (42, "2"), (50, "90")])];
        let i = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "4")], &[], &blocks, &ents), &DxfOptions::default()).unwrap();
        // The block's line, turned a quarter counter-clockwise (DXF frame) and doubled: from (0, 2) to (0, 4); then at (10, 10).
        // In the board's frame: x 10, y -12 to -14.
        let l = lines(&i);
        assert_eq!(l.len(), 1, "{:?}", i.shapes);
        assert!((l[0].0.x - 10.0).abs() < 1e-9 && (l[0].0.y + 12.0).abs() < 1e-9, "{:?}", l[0]);
        assert!((l[0].1.x - 10.0).abs() < 1e-9 && (l[0].1.y + 14.0).abs() < 1e-9, "{:?}", l[0]);
    }

    #[test]
    fn nested_blocks_are_placed_where_the_outer_block_is_inserted() {
        let blocks = [
            ("BLOCK", vec![(2, "INNER"), (10, "0"), (20, "0")]),
            ("LINE", vec![(10, "0"), (20, "0"), (11, "1"), (21, "0")]),
            ("ENDBLK", vec![]),
            ("BLOCK", vec![(2, "OUTER"), (10, "0"), (20, "0")]),
            ("INSERT", vec![(2, "INNER"), (10, "5"), (20, "0")]),
            ("ENDBLK", vec![]),
        ];
        let ents = [("INSERT", vec![(2, "OUTER"), (10, "100"), (20, "0")]), ("INSERT", vec![(2, "OUTER"), (10, "0"), (20, "20")])];
        let i = read_dxf(&dxf(&[(9, "$INSUNITS"), (70, "4")], &[], &blocks, &ents), &DxfOptions::default()).unwrap();
        let l = lines(&i);
        // One line per outer insert, 5 to the right of each; nothing at the block's own place.
        assert_eq!(l.len(), 2, "{:?}", i.shapes);
        assert_eq!((l[0].0, l[0].1), (V2::new(105.0, 0.0), V2::new(106.0, 0.0)));
        assert_eq!((l[1].0, l[1].1), (V2::new(5.0, -20.0), V2::new(6.0, -20.0)));
    }

    #[test]
    fn blocks_that_insert_each_other_do_not_loop_forever() {
        let blocks = [("BLOCK", vec![(2, "A")]), ("INSERT", vec![(2, "A")]), ("LINE", vec![(10, "0"), (20, "0"), (11, "1"), (21, "0")]), ("ENDBLK", vec![])];
        let ents = [("INSERT", vec![(2, "A")])];
        let i = read_dxf(&dxf(&[], &[], &blocks, &ents), &DxfOptions::default()).unwrap();
        assert!(i.messages.iter().any(|m| m.contains("in a loop")), "{:?}", i.messages);
        assert_eq!(lines(&i).len(), 1, "the block's own line, once");
    }

    #[test]
    fn a_circle_in_an_extruded_plane_is_turned_into_the_world() {
        // Extrusion (0, 0, -1): the object's X points the other way, so a circle at x = 5 is at x = -5.
        let i = read(&[("CIRCLE", vec![(10, "5"), (20, "0"), (30, "0"), (40, "1"), (210, "0"), (220, "0"), (230, "-1")])]);
        assert!(matches!(i.shapes[0], ImportedShape::Circle { center, .. } if (center.x + 5.0).abs() < 1e-9));
    }

    #[test]
    fn what_cannot_be_imported_is_reported_and_binary_files_are_refused() {
        let i = read(&[("HATCH", vec![]), ("DIMENSION", vec![]), ("ATTRIB", vec![]), ("SOLID", vec![])]);
        assert_eq!(i.messages, vec!["DXF hatches not currently supported.", "DXF dimensions not currently supported.", "DXF solids not currently supported."]);
        assert_eq!(i.message_counts().len(), 3);
        assert!(read_dxf("AutoCAD Binary DXF\r\n\u{1a}\0", &DxfOptions::default()).is_err());
        assert!(read_dxf("hello", &DxfOptions::default()).is_err());
        assert!(read_dxf("", &DxfOptions::default()).is_err());
    }

    #[test]
    fn a_dxf_with_crlf_line_ends_and_padded_codes_reads_the_same() {
        let t = "  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nLINE\r\n 10\r\n1.5\r\n 20\r\n2.5\r\n 11\r\n3\r\n 21\r\n4\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
        let i = read_dxf(t, &DxfOptions::default()).unwrap();
        assert_eq!(lines(&i)[0].0, V2::new(1.5, -2.5));
    }
}
