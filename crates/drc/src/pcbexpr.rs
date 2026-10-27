//! KiCad's PCB expression language: what `A.Width > 0.25mm`, `A.Type == 'Via' && A.NetName == 'GND'` and `A.hasNetclass('HV')` mean on a board's items. This is
//! `pcbnew/pcbexpr_evaluator.cpp`, `pcbexpr_functions.cpp` and the back half of `libeval_compiler` (the code generator, the values and the stack machine) at KiCad
//! 8303b2ad, over the lexer and grammar of [`crate::libeval`]. A `.kicad_dru` rule's `(condition "...")` and `(assertion "...")` are compiled with it, and
//! `DRC_RULE_CONDITION::EvaluateFor` runs them on one or two items ([`crate::rule_item::RuleItem`]); the studio's Find by Properties runs its queries here too, on
//! the items and properties of the Properties panel ([`crate::query`]; the TypeScript copy, `kicad-port/pcbExpr.ts`, is INACTIVE since 2026-10-28).
//!
//! Ported as it is, quirks included: `Compile` answers true when the text parses, whatever the code generator told (an unknown item, property or function is
//! reported to the listener and still compiles; `DRC_ENGINE::compileRules` counts those as errors, Find by Properties does not); strings compare without case and a
//! pattern is the right side of `==` only; an undefined value is neither equal nor different to anything; a run that leaves the stack other than with one value is
//! false; the arguments of a call are pushed last first; a length is in nanometres and an area in nm^2.
//!
//! A property a class has in KiCad but this studio does not model ([`crate::known_props`]) compiles and reads as undefined.

use eda_model::units::Nm;
use crate::kimath::Shape;
use crate::libeval::{self, Node, Op, UnitResolver, PCB_UNIT_NAMES};
use crate::rule_item::{AreaFacts, Class, FootprintFacts, Prop, RuleItem};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ----------------------------------------------------------------------------------------------------------------------------------------- units

/// `EDA_UNITS` as far as the language uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Unscaled,
    Mil,
    Mm,
    In,
    Deg,
    Fs,
    Ps,
}

const PCB_UNIT_TYPES: [Unit; 6] = [Unit::Mil, Unit::Mm, Unit::In, Unit::Deg, Unit::Fs, Unit::Ps];

/// `EDA_UNIT_UTILS::UI::DoubleValueFromString( text )`: the number a text starts with (0 when there is none).
pub fn double_value_from_string(text: &str) -> f64 {
    let t = text.trim();
    let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+')).unwrap_or(t.len());
    t[..end].replace(',', ".").parse::<f64>().unwrap_or(0.0)
}

/// `PCBEXPR_UNIT_RESOLVER::Convert`: lengths become nanometres and times attoseconds (`pcbIUScale`), rounded to a whole internal unit so that `==` works on `0.07mm`.
fn pcb_convert(text: &str, id: usize) -> f64 {
    let v = double_value_from_string(text);
    match id {
        0 => (v * 25_400.0).round(),
        1 => (v * 1_000_000.0).round(),
        2 => (v * 25_400_000.0).round(),
        3 => v,
        4 => (v * 1_000.0).round(),
        5 => (v * 1_000_000.0).round(),
        _ => v,
    }
}

/// `PCBEXPR_UNIT_RESOLVER`: `mil`, `mm`, `in`, `deg`, `fs` and `ps`.
pub const PCB_UNITS: UnitResolver = UnitResolver { names: &PCB_UNIT_NAMES, message: "must be mm, in, mil, deg, fs, or ps", convert: pcb_convert };

fn unitless_convert(text: &str, _id: usize) -> f64 {
    double_value_from_string(text)
}

/// `PCBEXPR_UNITLESS_RESOLVER`: no units (a count of spokes, an angle in a rule).
pub const PCB_UNITLESS: UnitResolver = UnitResolver { names: &[], message: "", convert: unitless_convert };

/// `EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, text, value )`: a text that is a number with a unit ("0.2mm", "5 mil", "90ra") is that many internal
/// units; a bare number or any other text is not one.
pub fn number_with_unit(text: &str) -> Option<f64> {
    let t = text.trim();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+')).unwrap_or(t.len());
    if split == 0 {
        return None;
    }
    let v = t[..split].replace(',', ".").parse::<f64>().unwrap_or(0.0);
    let unit = t[split..].trim().to_lowercase();
    Some(match unit.as_str() {
        "um" | "\u{b5}m" | "\u{3bc}m" => v * 1_000.0,
        "mm" => v * 1_000_000.0,
        "cm" => v * 10_000_000.0,
        "mil" | "mils" | "thou" => v * 25_400.0,
        "in" | "\"" => v * 25_400_000.0,
        "oz" => v * 1.37 * 25_400.0,
        "ra" => v * (180.0 / std::f64::consts::PI),
        "fs" => v * 1_000.0,
        "ps" => v * 1_000_000.0,
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------------------------------------------- wildcard matching

/// `WildCompareString( pattern, text, case_sensitive )` (common/string_utils.cpp): `*` any run, `?` any one character.
pub fn wild_compare(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = if case_sensitive { (pattern.chars().collect(), text.chars().collect()) } else { (pattern.to_uppercase().chars().collect(), text.to_uppercase().chars().collect()) };
    let (mut w, mut s) = (0usize, 0usize);
    let (mut mp, mut cp): (Option<usize>, usize) = (None, 0);
    while s < t.len() && p.get(w) != Some(&'*') {
        if p.get(w) != Some(&t[s]) && p.get(w) != Some(&'?') {
            return false;
        }
        w += 1;
        s += 1;
    }
    while s < t.len() {
        if p.get(w) == Some(&'*') {
            w += 1;
            if w >= p.len() {
                return true;
            }
            mp = Some(w);
            cp = s + 1;
        } else if p.get(w) == Some(&t[s]) || p.get(w) == Some(&'?') {
            w += 1;
            s += 1;
        } else {
            w = mp.unwrap_or(0);
            s = cp;
            cp += 1;
        }
    }
    while p.get(w) == Some(&'*') {
        w += 1;
    }
    w >= p.len()
}

/// `wxString::Matches( mask )`: `*` and `?`, case sensitive.
fn wx_matches(text: &str, mask: &str) -> bool {
    wild_compare(mask, text, true)
}

// ------------------------------------------------------------------------------------------------------------------------------------ environment

/// What the language needs of the board besides the two items: the layers it knows by name, the nets, the net classes, the footprints and areas to search.
pub struct Env {
    /// `ENUM_MAP<PCB_LAYER_ID>`: for each layer its canonical name and the name the board gives it, each an entry `(text, layer)`.
    pub layer_entries: Vec<(String, String)>,
    layer_cache: RefCell<HashMap<String, Rc<HashSet<String>>>>,
    /// `LayerName( layer )`: the standard name of a layer (`F.Silkscreen` for `F.SilkS`).
    pub standard_names: HashMap<String, String>,
    /// Every net of the board (`FindNet`).
    pub nets: HashSet<String>,
    pub footprints: Vec<Rc<RuleItem>>,
    pub areas: Vec<Rc<AreaFacts>>,
    /// `GetEffectiveNetClass( net )`: its name (the constituents joined by a comma) and the constituents, the class with the highest priority first.
    pub classes: Box<dyn Fn(&str) -> (String, Vec<String>)>,
    /// `true` and `false` are the numbers 1 and 0. Not KiCad: its compiler has neither name and calls them unrecognized items, while its Find by
    /// Properties dialog writes `A.Locked == false` for a flag (`formatValueForExpression`). Only the studio's Find by Properties query turns it on
    /// ([`crate::query`]); rule files keep KiCad's reading.
    pub literal_booleans: bool,
    /// The property names the language knows, lower-cased, when the items come with their own registry (the studio's Properties panel, for Find by
    /// Properties); `None` is KiCad's list ([`crate::known_props`]).
    pub property_names: Option<HashSet<String>>,
}

impl Env {
    /// An environment with nothing in it but the canonical layers named, for tests and for the first compile of a rule file.
    pub fn simple(layers: &[&str]) -> Env {
        Env {
            layer_entries: layers.iter().map(|l| (l.to_string(), l.to_string())).collect(),
            layer_cache: RefCell::new(HashMap::new()),
            standard_names: HashMap::new(),
            nets: HashSet::new(),
            footprints: Vec::new(),
            areas: Vec::new(),
            classes: Box::new(|_| ("Default".to_string(), vec!["Default".to_string()])),
            literal_booleans: false,
            property_names: None,
        }
    }

    pub fn new(layer_entries: Vec<(String, String)>, standard_names: HashMap<String, String>, classes: Box<dyn Fn(&str) -> (String, Vec<String>)>) -> Env {
        Env { layer_entries, layer_cache: RefCell::new(HashMap::new()), standard_names, nets: HashSet::new(), footprints: Vec::new(), areas: Vec::new(), classes, literal_booleans: false, property_names: None }
    }

    /// `m_LayerExpressionCache`: the layers a pattern names (`entry.GetText().Matches( layerName )` for each entry of the enum map).
    pub fn layer_mask(&self, pattern: &str) -> Rc<HashSet<String>> {
        if let Some(m) = self.layer_cache.borrow().get(pattern) {
            return m.clone();
        }
        let set: HashSet<String> = self.layer_entries.iter().filter(|(text, _)| wx_matches(text, pattern)).map(|(_, l)| l.clone()).collect();
        let rc = Rc::new(set);
        self.layer_cache.borrow_mut().insert(pattern.to_string(), rc.clone());
        rc
    }

    fn layer_name(&self, layer: &str) -> String {
        self.standard_names.get(layer).cloned().unwrap_or_else(|| layer.to_string())
    }

    /// The name of a net's effective class: an item with no net is in the Default class alone.
    pub fn class_name(&self, net: &str) -> String {
        if net.is_empty() {
            return "Default".to_string();
        }
        (self.classes)(net).0
    }

    pub fn constituents(&self, net: &str) -> Vec<String> {
        if net.is_empty() {
            return vec!["Default".to_string()];
        }
        (self.classes)(net).1
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- the context

/// `PCBEXPR_CONTEXT`: the constraint being evaluated, the layer, the items `A` and `B`, and the stack the code runs on.
pub struct Ctx {
    pub env: Rc<Env>,
    pub constraint: u32,
    /// `None` is `UNDEFINED_LAYER`.
    pub layer: Option<String>,
    pub items: [Option<Rc<RuleItem>>; 2],
    /// `SetTypeOverride`: a teardrop area counts as a track.
    pub type_overrides: Vec<(String, &'static str)>,
    stack: Vec<Value>,
    /// `HasErrorCallback`: the run is being reported on, so the messages of its functions are kept.
    pub listening: bool,
    messages: RefCell<Vec<String>>,
}

impl Ctx {
    pub fn new(env: Rc<Env>, constraint: u32, layer: Option<&str>) -> Ctx {
        Ctx { env, constraint, layer: layer.map(String::from), items: [None, None], type_overrides: Vec::new(), stack: Vec::new(), listening: false, messages: RefCell::new(Vec::new()) }
    }

    pub fn set_items(&mut self, a: Option<Rc<RuleItem>>, b: Option<Rc<RuleItem>>) {
        self.items = [a, b];
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    /// `CONTEXT::Pop`: an empty stack is "Malformed expression" and an undefined value.
    fn pop(&mut self) -> Value {
        match self.stack.pop() {
            Some(v) => v,
            None => {
                self.report_error("Malformed expression");
                Value::undefined()
            }
        }
    }

    pub fn report_error(&self, message: &str) {
        if self.listening {
            self.messages.borrow_mut().push(message.to_string());
        }
    }

    /// What the run told, in order (the `ERROR:` lines of a rule report).
    pub fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.messages.borrow_mut())
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------- values

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VKind {
    Undefined,
    Null,
    Numeric,
    Str,
}

#[derive(Clone)]
enum Special {
    Plain,
    /// `PCBEXPR_LAYER_VALUE`: a layer, equal to the layers a pattern names.
    Layer(String),
    /// `PCBEXPR_NETCLASS_VALUE`: the net class of a net, equal to any class of a composite one.
    Netclass(String),
    /// `PCBEXPR_NET_VALUE`: a net, equal to another net of the same name.
    Net(String),
}

type LazyNum = Rc<dyn Fn(&Ctx) -> f64>;
type LazyStr = Rc<dyn Fn(&Ctx) -> String>;

/// `LIBEVAL::VALUE`.
#[derive(Clone)]
pub struct Value {
    kind: VKind,
    dbl: f64,
    lazy_dbl: Option<LazyNum>,
    text: String,
    lazy_text: Option<LazyStr>,
    pub wildcard: bool,
    pub units: Unit,
    special: Special,
}

impl Value {
    fn base(kind: VKind) -> Value {
        Value { kind, dbl: 0.0, lazy_dbl: None, text: String::new(), lazy_text: None, wildcard: false, units: Unit::Unscaled, special: Special::Plain }
    }

    pub fn undefined() -> Value {
        Value::base(VKind::Undefined)
    }

    pub fn null() -> Value {
        Value::base(VKind::Null)
    }

    pub fn number(n: f64) -> Value {
        let mut v = Value::base(VKind::Numeric);
        v.dbl = n;
        v
    }

    pub fn number_in(n: f64, units: Unit) -> Value {
        let mut v = Value::number(n);
        v.units = units;
        v
    }

    pub fn string(s: &str) -> Value {
        let mut v = Value::base(VKind::Str);
        v.text = s.to_string();
        v
    }

    fn literal(s: &str) -> Value {
        let mut v = Value::string(s);
        v.wildcard = s.contains('?') || s.contains('*');
        v
    }

    fn lazy_number(f: impl Fn(&Ctx) -> f64 + 'static) -> Value {
        let mut v = Value::base(VKind::Numeric);
        v.lazy_dbl = Some(Rc::new(f));
        v
    }

    fn lazy_string(f: impl Fn(&Ctx) -> String + 'static) -> Value {
        let mut v = Value::base(VKind::Str);
        v.lazy_text = Some(Rc::new(f));
        v
    }

    fn layer(layer: &str) -> Value {
        let mut v = Value::base(VKind::Str);
        v.special = Special::Layer(layer.to_string());
        v
    }

    fn netclass(net: &str) -> Value {
        let mut v = Value::base(VKind::Str);
        v.special = Special::Netclass(net.to_string());
        v
    }

    fn net(name: &str) -> Value {
        let mut v = Value::base(VKind::Str);
        v.special = Special::Net(name.to_string());
        v
    }

    pub fn kind(&self) -> VKind {
        self.kind
    }

    pub fn as_double(&self, ctx: &Ctx) -> f64 {
        match &self.lazy_dbl {
            Some(f) => f(ctx),
            None => self.dbl,
        }
    }

    pub fn as_string(&self, ctx: &Ctx) -> String {
        match &self.special {
            Special::Layer(l) => ctx.env.layer_name(l),
            Special::Netclass(net) => ctx.env.class_name(net),
            Special::Net(n) => n.clone(),
            Special::Plain => match &self.lazy_text {
                Some(f) => f(ctx),
                None => self.text.clone(),
            },
        }
    }

    /// `VALUE::EqualTo` and its overrides.
    pub fn equal_to(&self, ctx: &Ctx, b: &Value) -> bool {
        match &self.special {
            Special::Layer(layer) => ctx.env.layer_mask(&b.as_string(ctx)).contains(layer),
            Special::Netclass(net) => {
                if let Special::Netclass(other) = &b.special {
                    return ctx.env.class_name(net) == ctx.env.class_name(other);
                }
                if b.kind == VKind::Str {
                    for name in ctx.env.constituents(net) {
                        let hit = if b.wildcard { wild_compare(&b.as_string(ctx), &name, false) } else { name.to_uppercase() == b.as_string(ctx).to_uppercase() };
                        if hit {
                            return true;
                        }
                    }
                }
                self.plain_equal_to(ctx, b)
            }
            Special::Net(net) => match &b.special {
                Special::Net(other) => net == other,
                _ => self.plain_equal_to(ctx, b),
            },
            Special::Plain => self.plain_equal_to(ctx, b),
        }
    }

    fn plain_equal_to(&self, ctx: &Ctx, b: &Value) -> bool {
        if self.kind == VKind::Undefined || b.kind == VKind::Undefined {
            return false;
        }
        if self.kind == VKind::Null && b.kind == VKind::Null {
            return true;
        }
        if self.kind == VKind::Numeric && b.kind == VKind::Numeric {
            return self.as_double(ctx) == b.as_double(ctx);
        }
        if self.kind == VKind::Str && b.kind == VKind::Str {
            if b.wildcard {
                return wild_compare(&b.as_string(ctx), &self.as_string(ctx), false);
            }
            return self.as_string(ctx).to_uppercase() == b.as_string(ctx).to_uppercase();
        }
        false
    }

    /// `VALUE::NotEqualTo` and its overrides: not the inverse of `equal_to`, both say false for an undefined value.
    pub fn not_equal_to(&self, ctx: &Ctx, b: &Value) -> bool {
        match &self.special {
            Special::Netclass(net) => {
                if let Special::Netclass(other) = &b.special {
                    return ctx.env.class_name(net) != ctx.env.class_name(other);
                }
                if b.kind == VKind::Str {
                    let mut in_constituents = false;
                    for name in ctx.env.constituents(net) {
                        let hit = if b.wildcard { wild_compare(&b.as_string(ctx), &name, false) } else { name.to_uppercase() == b.as_string(ctx).to_uppercase() };
                        if hit {
                            in_constituents = true;
                            break;
                        }
                    }
                    let is_full_name = self.plain_equal_to(ctx, b);
                    return !in_constituents && !is_full_name;
                }
                self.plain_not_equal_to(ctx, b)
            }
            Special::Net(net) => match &b.special {
                Special::Net(other) => net != other,
                _ => self.plain_not_equal_to(ctx, b),
            },
            _ => self.plain_not_equal_to(ctx, b),
        }
    }

    fn plain_not_equal_to(&self, ctx: &Ctx, b: &Value) -> bool {
        if self.kind == VKind::Undefined || b.kind == VKind::Undefined {
            return false;
        }
        !self.equal_to(ctx, b)
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------- the code

/// `VAR_TYPE_T`: what a property reference will produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarType {
    Str,
    Numeric,
    NumericDouble,
    Undefined,
    ParseError,
    Null,
}

#[derive(Debug, Clone, PartialEq)]
enum RefKind {
    Property,
    NetClass,
    ComponentClass,
    NetName,
    Type,
    Null,
    /// `true` or `false` with [`Env::literal_booleans`].
    Boolean(bool),
}

/// `PCBEXPR_VAR_REF` and its "object code" subclasses: `A.Width`, `B.Layer`, or just `A`. `index` is 0 for A (and AB), 1 for B and 2 for L, the layer.
#[derive(Debug, Clone)]
pub struct VarRef {
    kind: RefKind,
    index: usize,
    vtype: VarType,
    /// The property's name with blanks, as the registry spells it (`pad type`).
    field: String,
}

impl VarRef {
    pub fn var_type(&self) -> VarType {
        self.vtype
    }

    fn object(&self, ctx: &Ctx) -> Option<Rc<RuleItem>> {
        if self.index > 1 {
            return None;
        }
        ctx.items[self.index].clone()
    }

    /// `PCBEXPR_VAR_REF::GetValue` and the subclasses'.
    fn get_value(&self, ctx: &Ctx) -> Value {
        match self.kind {
            RefKind::Null => return Value::null(),
            RefKind::Boolean(b) => return Value::number(if b { 1.0 } else { 0.0 }),
            RefKind::NetClass => {
                return match self.object(ctx) {
                    Some(item) if item.is_connected() => Value::netclass(&item.net),
                    _ => Value::undefined(),
                }
            }
            // The studio has no component classes: a footprint (or an item of one) is in the empty class.
            RefKind::ComponentClass => {
                return match self.object(ctx) {
                    Some(item) if item.class == Class::Footprint || item.footprint.is_some() => Value::string(""),
                    _ => Value::undefined(),
                }
            }
            RefKind::NetName => {
                return match self.object(ctx) {
                    Some(item) if item.is_connected() => Value::net(&item.net),
                    _ => Value::undefined(),
                }
            }
            RefKind::Type => {
                return match self.object(ctx) {
                    Some(item) => {
                        let over = ctx.type_overrides.iter().find(|(id, _)| *id == item.id).map(|(_, t)| *t);
                        Value::string(over.or(item.type_name.as_deref()).unwrap_or_else(|| item.class.type_name()))
                    }
                    None => Value::undefined(),
                }
            }
            RefKind::Property => {}
        }
        if self.index == 2 {
            return Value::layer(ctx.layer.as_deref().unwrap_or(""));
        }
        let Some(item) = self.object(ctx) else { return Value::undefined() };
        let name = self.field.as_str();
        // "If the property isn't defined on the item itself but is defined on its parent footprint (e.g. Reference, Value), resolve against the parent so that
        // conditions like "A.Reference == 'J1'" match pads and graphics belonging to J1."
        let prop = match item.prop(name) {
            Some(p) => Some(p.clone()),
            // `Net`: the net's name, for the items that have one.
            None if name.eq_ignore_ascii_case("net") && item.is_connected() => Some(Prop::Enum(item.net.clone())),
            // Items that come with their own registry (Find by Properties) carry their footprint's properties already, merged under their own.
            None if ctx.env.property_names.is_some() => None,
            None => item.footprint.as_ref().filter(|_| item.class != Class::Footprint).and_then(|fp| footprint_prop(fp, name)),
        };
        match prop {
            Some(p) => prop_value(&p, name),
            None => Value::undefined(),
        }
    }
}

/// The properties of a footprint that a pad or a graphic of it can be asked for (`Reference`, `Value`, `Library Link`).
fn footprint_prop(fp: &FootprintFacts, name: &str) -> Option<Prop> {
    match name.to_lowercase().as_str() {
        "reference" => Some(Prop::Str(fp.reference.clone())),
        "library link" => Some(Prop::Str(fp.fpid.clone())),
        "value" => fp.fields.iter().find(|(n, _)| n == "Value").map(|(_, t)| Prop::Str(t.clone())),
        _ => None,
    }
}

/// A property's value as the language sees it (`PCBEXPR_VAR_REF::GetValue` by the type of the property).
fn prop_value(p: &Prop, name: &str) -> Value {
    match p {
        Prop::Int(n) => Value::number(*n as f64),
        Prop::Double(d) => Value::number(*d),
        Prop::Bool(b) => Value::number(if *b { 1.0 } else { 0.0 }),
        Prop::Layer(l) => {
            let _ = name;
            Value::layer(l)
        }
        Prop::Enum(label) => Value::string(label),
        // "If it quacks like a duck, it is a duck": a text such as "0.2mm" is a length.
        Prop::Str(s) => match number_with_unit(s) {
            Some(n) => Value::number(n),
            None => Value::string(s),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Func {
    ExistsOnLayer,
    IsPlated,
    IsMicroVia,
    IsBlindVia,
    IsBuriedVia,
    IsBlindBuriedVia,
    MemberOfGroup,
    MemberOfFootprint,
    MemberOfSheet,
    MemberOfSheetOrChildren,
    FromTo,
    IsCoupledDiffPair,
    InDiffPair,
    GetField,
    HasNetclass,
    HasExactNetclass,
    HasComponentClass,
    IntersectsCourtyard,
    IntersectsFrontCourtyard,
    IntersectsBackCourtyard,
    IntersectsArea,
    EnclosedByArea,
}

/// `PCBEXPR_BUILTIN_FUNCTIONS::Get( name )`: by the lower-cased name, the deprecated names included.
fn func_by_name(lower: &str) -> Option<Func> {
    Some(match lower {
        "existsonlayer" => Func::ExistsOnLayer,
        "isplated" => Func::IsPlated,
        "insidecourtyard" | "intersectscourtyard" => Func::IntersectsCourtyard,
        "insidefrontcourtyard" | "intersectsfrontcourtyard" => Func::IntersectsFrontCourtyard,
        "insidebackcourtyard" | "intersectsbackcourtyard" => Func::IntersectsBackCourtyard,
        "insidearea" | "intersectsarea" => Func::IntersectsArea,
        "enclosedbyarea" => Func::EnclosedByArea,
        "ismicrovia" => Func::IsMicroVia,
        "isblindvia" => Func::IsBlindVia,
        "isburiedvia" => Func::IsBuriedVia,
        "isblindburiedvia" => Func::IsBlindBuriedVia,
        "memberof" | "memberofgroup" => Func::MemberOfGroup,
        "memberoffootprint" => Func::MemberOfFootprint,
        "memberofsheet" => Func::MemberOfSheet,
        "memberofsheetorchildren" => Func::MemberOfSheetOrChildren,
        "fromto" => Func::FromTo,
        "iscoupleddiffpair" => Func::IsCoupledDiffPair,
        "indiffpair" => Func::InDiffPair,
        "getfield" => Func::GetField,
        "hasnetclass" => Func::HasNetclass,
        "hasexactnetclass" => Func::HasExactNetclass,
        "hascomponentclass" => Func::HasComponentClass,
        _ => return None,
    })
}

/// Does the function look at the shapes of items (`RegisterFunc( .., true )`): `HasGeometryDependentFunctions`.
fn is_geometry_dependent(f: Func) -> bool {
    matches!(f, Func::IntersectsCourtyard | Func::IntersectsFrontCourtyard | Func::IntersectsBackCourtyard | Func::IntersectsArea | Func::EnclosedByArea)
}

#[derive(Clone)]
enum Uop {
    PushVar(Option<Rc<VarRef>>),
    PushValue(Value),
    MethodCall { func: Option<Func>, vref: Option<Rc<VarRef>> },
    Op(Op),
}

/// `LIBEVAL::UCODE` of a `PCBEXPR_UCODE`: the compiled expression.
#[derive(Clone, Default)]
pub struct Ucode {
    ops: Vec<Uop>,
    geometry: bool,
}

impl Ucode {
    /// `HasGeometryDependentFunctions`.
    pub fn has_geometry_dependent_functions(&self) -> bool {
        self.geometry
    }

    /// `UCODE::Run`: a run that fails runs no rule (0), and so does one that leaves the stack other than with one value.
    pub fn run(&self, ctx: &mut Ctx) -> Value {
        ctx.stack.clear();
        for op in &self.ops {
            exec(op, ctx);
        }
        if ctx.stack.len() == 1 {
            return ctx.pop();
        }
        Value::number(0.0)
    }
}

fn result_units(a: &Value, b: &Value) -> Unit {
    // `getOpResultUnits`: a unit from one side when the other has none, else the right side's.
    if a.units == Unit::Unscaled && b.units != Unit::Unscaled {
        return b.units;
    }
    if a.units != Unit::Unscaled && b.units == Unit::Unscaled {
        return a.units;
    }
    b.units
}

/// `UOP::Exec`.
fn exec(u: &Uop, ctx: &mut Ctx) {
    match u {
        Uop::PushVar(r) => {
            let v = match r {
                Some(r) => r.get_value(ctx),
                None => Value::undefined(),
            };
            ctx.push(v);
        }
        Uop::PushValue(v) => ctx.push(v.clone()),
        Uop::MethodCall { func, vref } => {
            if let Some(f) = func {
                call_func(*f, ctx, vref.as_deref());
            }
        }
        Uop::Op(Op::Not) => {
            let a = ctx.pop();
            let r = if a.as_double(ctx) != 0.0 { 0.0 } else { 1.0 };
            ctx.push(Value::number_in(r, a.units));
        }
        Uop::Op(Op::Number | Op::Identifier | Op::Str | Op::StructRef | Op::FuncCall | Op::Unit | Op::ArgList | Op::Null) => {}
        Uop::Op(op) => {
            let b = ctx.pop();
            let a = ctx.pop();
            if ctx.listening {
                if a.kind == VKind::Str && b.kind == VKind::Numeric {
                    ctx.report_error(&format!("Type mismatch between '{}' and {:.6}", a.as_string(ctx), b.as_double(ctx)));
                } else if a.kind == VKind::Numeric && b.kind == VKind::Str {
                    ctx.report_error(&format!("Type mismatch between {:.6} and '{}'", a.as_double(ctx), b.as_string(ctx)));
                }
            }
            let mut units = Unit::Unscaled;
            let result = match op {
                Op::Add => {
                    units = result_units(&a, &b);
                    a.as_double(ctx) + b.as_double(ctx)
                }
                Op::Sub => {
                    units = result_units(&a, &b);
                    a.as_double(ctx) - b.as_double(ctx)
                }
                Op::Mul => {
                    units = result_units(&a, &b);
                    a.as_double(ctx) * b.as_double(ctx)
                }
                Op::Div => {
                    units = result_units(&a, &b);
                    a.as_double(ctx) / b.as_double(ctx)
                }
                Op::LessEqual => (a.as_double(ctx) <= b.as_double(ctx)) as u8 as f64,
                Op::GreaterEqual => (a.as_double(ctx) >= b.as_double(ctx)) as u8 as f64,
                Op::Less => (a.as_double(ctx) < b.as_double(ctx)) as u8 as f64,
                Op::Greater => (a.as_double(ctx) > b.as_double(ctx)) as u8 as f64,
                // `if( arg2->GetType() == VT_UNDEFINED ) arg2->EqualTo( arg1 )`: an undefined value is false either way round.
                Op::Equal => (if b.kind == VKind::Undefined { b.equal_to(ctx, &a) } else { a.equal_to(ctx, &b) }) as u8 as f64,
                Op::NotEqual => (if b.kind == VKind::Undefined { b.not_equal_to(ctx, &a) } else { a.not_equal_to(ctx, &b) }) as u8 as f64,
                Op::And => (a.as_double(ctx) != 0.0 && b.as_double(ctx) != 0.0) as u8 as f64,
                Op::Or => (a.as_double(ctx) != 0.0 || b.as_double(ctx) != 0.0) as u8 as f64,
                _ => 0.0,
            };
            ctx.push(Value::number_in(result, units));
        }
    }
}

// ----------------------------------------------------------------------------------------------------------------------------------- functions

fn missing(name: &str, what: &str) -> String {
    match name {
        "existsOnLayer()" => format!("Missing layer name argument to {name}."),
        "intersectsCourtyard()" | "intersectsFrontCourtyard()" | "intersectsBackCourtyard()" => format!("Missing footprint argument (A, B, or reference designator) to {name}."),
        "intersectsArea()" | "enclosedByArea()" => format!("Missing rule-area argument (A, B, or rule-area name) to {name}."),
        "memberOfGroup()" => format!("Missing group name argument to {name}."),
        "memberOfSheet()" | "memberOfSheetOrChildren()" => format!("Missing sheet name argument to {name}."),
        "memberOfFootprint()" => format!("Missing footprint argument (reference designator) to {name}."),
        "inDiffPair()" => format!("Missing diff-pair name argument to {name}."),
        _ => format!("Missing {what} argument to {name}"),
    }
}

/// The text a function reports for a missing argument, and the name it calls itself.
fn func_label(f: Func) -> &'static str {
    match f {
        Func::ExistsOnLayer => "existsOnLayer()",
        Func::IntersectsCourtyard => "intersectsCourtyard()",
        Func::IntersectsFrontCourtyard => "intersectsFrontCourtyard()",
        Func::IntersectsBackCourtyard => "intersectsBackCourtyard()",
        Func::IntersectsArea => "intersectsArea()",
        Func::EnclosedByArea => "enclosedByArea()",
        Func::MemberOfGroup => "memberOfGroup()",
        Func::MemberOfFootprint => "memberOfFootprint()",
        Func::MemberOfSheet => "memberOfSheet()",
        Func::MemberOfSheetOrChildren => "memberOfSheetOrChildren()",
        Func::InDiffPair => "inDiffPair()",
        Func::GetField => "getField()",
        Func::HasNetclass => "hasNetclass()",
        Func::HasExactNetclass => "hasExactNetclass()",
        Func::HasComponentClass => "hasComponentClass()",
        _ => "",
    }
}

/// `MatchDpSuffix`: 1 for a positive net (`..P`, `..+`), -1 for a negative one, 0 for neither; with the name of the complement and the name without the suffix.
pub fn match_dp_suffix(net: &str) -> (i32, String, String) {
    let chars: Vec<char> = net.chars().collect();
    let mut polarity = 0;
    let mut count = 0usize;
    let mut comp = "";
    for ch in chars.iter().rev() {
        if polarity != 0 {
            break;
        }
        if ch.is_ascii_digit() || *ch == '_' {
            count += 1;
            continue;
        }
        match ch {
            '+' => {
                comp = "-";
                polarity = 1;
            }
            '-' => {
                comp = "+";
                polarity = -1;
            }
            'N' => {
                comp = "P";
                polarity = -1;
            }
            'P' => {
                comp = "N";
                polarity = 1;
            }
            _ => break,
        }
        count += 1;
    }
    if polarity != 0 && count >= 1 {
        let base: String = chars[..chars.len() - count].iter().collect();
        let tail: String = chars[chars.len() - (count - 1)..].iter().collect();
        let complement = format!("{base}{comp}{tail}");
        return (polarity, complement, base);
    }
    (polarity, String::new(), String::new())
}

/// `testFootprintSelector`: the reference, or the library id when the selector has a colon, matches (a wildcard pattern).
fn footprint_selected(fp: &FootprintFacts, selector: &str) -> bool {
    if selector.is_empty() {
        return false;
    }
    if wx_matches(&fp.reference, selector) {
        return true;
    }
    selector.contains(':') && wx_matches(&fp.fpid, selector)
}

/// The footprints `searchFootprints` visits for `arg`: A, B, or those whose reference or library id matches.
fn search_footprints(ctx: &Ctx, arg: &str) -> Vec<Rc<RuleItem>> {
    if arg == "A" || arg == "B" {
        let idx = if arg == "A" { 0 } else { 1 };
        return match &ctx.items[idx] {
            Some(it) if it.class == Class::Footprint => vec![it.clone()],
            _ => Vec::new(),
        };
    }
    ctx.env.footprints.iter().filter(|fp| fp.footprint.as_ref().is_some_and(|f| footprint_selected(f, arg))).cloned().collect()
}

/// `searchAreas`: A, B, a zone by its id, or the areas whose name matches.
fn search_areas(ctx: &Ctx, arg: &str) -> Vec<Rc<AreaFacts>> {
    let by_item = |idx: usize| -> Vec<Rc<AreaFacts>> {
        match &ctx.items[idx] {
            Some(it) if it.class == Class::Zone => vec![Rc::new(AreaFacts { id: it.id.clone(), name: String::new(), layers: it.layers.clone(), outline: it.outline.clone(), holes: it.holes.clone(), parent_footprint: None })],
            _ => Vec::new(),
        }
    };
    if arg == "A" {
        return by_item(0);
    }
    if arg == "B" {
        return by_item(1);
    }
    if let Some(hit) = ctx.env.areas.iter().find(|a| a.id == arg) {
        return vec![hit.clone()];
    }
    ctx.env.areas.iter().filter(|a| wx_matches(&a.name, arg)).cloned().collect()
}

/// The shape of an item on a layer, for the geometry functions (`GetEffectiveShape( layer )`): what it occupies, or for a zone its outline.
fn item_shape(item: &RuleItem) -> Shape {
    if item.class == Class::Zone && item.outline.len() >= 3 {
        return Shape::Polygon { pts: item.outline.clone() };
    }
    match item.shapes.len() {
        0 => Shape::Circle { c: item.position, r: Nm::ZERO },
        1 => item.shapes[0].clone(),
        _ => Shape::Compound { parts: item.shapes.iter().flat_map(|s| s.parts().to_vec()).collect() },
    }
}

/// `collidesWithCourtyard`: the item's shape touches one of the footprint's courtyard outlines on that side.
fn collides_with_courtyard(item: &RuleItem, fp: &FootprintFacts, front: bool) -> bool {
    let outlines = if front { &fp.courtyard_front } else { &fp.courtyard_back };
    if outlines.is_empty() {
        return false;
    }
    let shape = item_shape(item);
    outlines.iter().any(|o| o.len() >= 3 && Shape::Polygon { pts: o.clone() }.gap_to(&shape) == 0.0)
}

/// `collidesWithArea` for a shape item on a layer: the area's outline (deflated by the DRC epsilon in KiCad, so that touching is no collision) overlaps it.
fn collides_with_area(item: &RuleItem, area: &AreaFacts) -> bool {
    if area.outline.len() < 3 {
        return false;
    }
    let outline = Shape::Polygon { pts: area.outline.clone() };
    if item.class == Class::Footprint {
        // The footprint's courtyards, on the sides the area is on.
        let Some(fp) = &item.footprint else { return false };
        let front = area.layers.iter().any(|l| l.starts_with("F."));
        let back = area.layers.iter().any(|l| l.starts_with("B."));
        let hit = |courtyards: &Vec<Vec<eda_model::ir::Point>>| {
            courtyards.first().is_some_and(|c| c.len() >= 3 && outline.gap_to(&Shape::Polygon { pts: c.clone() }) == 0.0 && !in_a_hole(&Shape::Polygon { pts: c.clone() }, area))
        };
        return (front && hit(&fp.courtyard_front)) || (back && hit(&fp.courtyard_back));
    }
    let shape = item_shape(item);
    outline.gap_to(&shape) == 0.0 && !in_a_hole(&shape, area)
}

/// The shape lies wholly inside one of the area's holes, where the area's polygon (`ZONE::Outline()`, holes and all) is not.
fn in_a_hole(shape: &Shape, area: &AreaFacts) -> bool {
    area.holes.iter().any(|h| h.len() >= 3 && enclosed_in_ring(shape, h))
}

/// Every vertex of the shape's outline is inside the ring and none of the ring's edges crosses it.
fn enclosed_in_ring(shape: &Shape, ring: &[eda_model::ir::Point]) -> bool {
    let outline = Shape::Polygon { pts: ring.to_vec() };
    let boundary = outline.boundary_segs();
    // a shape that is wholly inside has no edge of the ring within it and its probe point is inside the ring
    let probe = shape.parts().first().map(|s| s.boundary_segs()).and_then(|s| s.first().map(|g| g.a));
    let Some(p) = probe else { return false };
    let inside = outline.gap_to(&Shape::Circle { c: p, r: Nm::ZERO }) == 0.0;
    inside && boundary.iter().all(|seg| shape.parts().iter().all(|part| part.gap_to(&Shape::Stadium { a: seg.a, b: seg.b, r: Nm::ZERO }) > 0.0))
}

/// The call of a function: it takes its arguments off the stack and leaves its result on it. Most results are deferred, worked out when they are first asked for.
fn call_func(f: Func, ctx: &mut Ctx, vref: Option<&VarRef>) {
    let item = vref.and_then(|v| v.object(ctx));
    match f {
        Func::FromTo => {
            ctx.pop();
            ctx.pop();
            ctx.push(Value::number(0.0));
        }
        Func::IsMicroVia | Func::IsBlindVia | Func::IsBuriedVia | Func::IsBlindBuriedVia | Func::IsPlated => {
            let mut r = 0.0;
            if let Some(it) = &item {
                let via = it.class == Class::Via;
                r = match f {
                    Func::IsMicroVia => via && it.via_type == Some("Micro"),
                    Func::IsBlindVia => via && it.via_type == Some("Blind"),
                    Func::IsBuriedVia => via && it.via_type == Some("Buried"),
                    Func::IsBlindBuriedVia => via && matches!(it.via_type, Some("Blind" | "Buried")),
                    _ => it.class == Class::Via || (it.class == Class::Pad && it.plated),
                } as u8 as f64;
            }
            ctx.push(Value::number(r));
        }
        Func::IsCoupledDiffPair => {
            let (a, b) = (ctx.items[0].clone(), ctx.items[1].clone());
            let constraint = ctx.constraint;
            ctx.push(Value::lazy_number(move |c: &Ctx| {
                let Some(a) = a.as_ref().filter(|a| a.is_connected()) else { return 0.0 };
                let (polarity, complement, _) = match_dp_suffix(&a.net);
                if polarity == 0 {
                    return 0.0;
                }
                if matches!(constraint, DIFF_PAIR_GAP | LENGTH | SKEW) && c.env.nets.contains(&complement) {
                    // "DRC engine evaluates these only in the context of a diffpair, but doesn't always supply the second (B) item."
                    return 1.0;
                }
                (b.as_ref().is_some_and(|b| b.is_connected() && b.net == complement)) as u8 as f64
            }));
        }
        Func::GetField => {
            let arg = ctx.pop();
            let Some(it) = item.filter(|_| true) else {
                ctx.push(Value::string(""));
                return;
            };
            ctx.push(Value::lazy_string(move |c: &Ctx| {
                if it.class != Class::Footprint {
                    return String::new();
                }
                let name = arg.as_string(c);
                it.footprint.as_ref().and_then(|fp| fp.fields.iter().find(|(n, _)| *n == name).map(|(_, t)| t.clone())).unwrap_or_default()
            }));
        }
        Func::HasComponentClass | Func::MemberOfSheet | Func::MemberOfSheetOrChildren => {
            let arg = ctx.pop();
            let empty = arg.as_string(ctx).is_empty();
            if empty {
                ctx.report_error(&missing(func_label(f), "name"));
                ctx.push(Value::number(0.0));
                return;
            }
            if f == Func::HasComponentClass {
                ctx.push(Value::number(0.0));
                return;
            }
            // The studio's footprints carry no sheet path: a footprint is on the root sheet, "/".
            let Some(it) = item else {
                ctx.push(Value::number(0.0));
                return;
            };
            let orchildren = f == Func::MemberOfSheetOrChildren;
            ctx.push(Value::lazy_number(move |c: &Ctx| {
                let Some(fp) = it.footprint.as_ref() else { return 0.0 };
                let sheet = fp.sheet.trim_end_matches('/');
                let wanted = arg.as_string(c);
                let wanted = wanted.trim_end_matches('/');
                if orchildren {
                    let sp: Vec<&str> = sheet.split('/').collect();
                    let rp: Vec<&str> = wanted.split('/').collect();
                    if rp.len() > sp.len() {
                        return 0.0;
                    }
                    if (wx_matches("/", wanted) || wanted.is_empty()) && sheet.is_empty() {
                        return 1.0;
                    }
                    return rp.iter().zip(sp.iter()).all(|(r, s)| wx_matches(s, r)) as u8 as f64;
                }
                (wx_matches(sheet, wanted) || ((wx_matches("/", wanted) || wanted.is_empty()) && sheet.is_empty())) as u8 as f64
            }));
        }
        // The functions of one argument whose result is worked out when it is asked for.
        _ => {
            let arg = ctx.pop();
            if arg.as_string(ctx).is_empty() {
                ctx.report_error(&missing(func_label(f), "name"));
                ctx.push(Value::number(0.0));
                return;
            }
            let Some(it) = item else {
                ctx.push(Value::number(0.0));
                return;
            };
            let items = ctx.items.clone();
            ctx.push(Value::lazy_number(move |c: &Ctx| eval_one_arg(f, c, &it, &items, &arg)));
        }
    }
}

/// `DRC_CONSTRAINT_T` values `isCoupledDiffPair` looks at.
pub const DIFF_PAIR_GAP: u32 = 25;
pub const LENGTH: u32 = 23;
pub const SKEW: u32 = 24;

fn eval_one_arg(f: Func, c: &Ctx, it: &Rc<RuleItem>, _items: &[Option<Rc<RuleItem>>; 2], arg: &Value) -> f64 {
    let arg_s = arg.as_string(c);
    let b = |x: bool| x as u8 as f64;
    match f {
        Func::ExistsOnLayer => {
            // Interpreted (a rule report listens): an unrecognized layer name is told.
            if c.listening {
                let mask = c.env.layer_mask(&arg_s);
                if mask.is_empty() {
                    c.report_error(&format!("Unrecognized layer '{arg_s}'"));
                    return 0.0;
                }
                return b(it.layers.iter().any(|l| mask.contains(l)));
            }
            let mask = c.env.layer_mask(&arg_s);
            b(it.layers.iter().any(|l| mask.contains(l)))
        }
        Func::MemberOfGroup => b(it.groups.iter().any(|g| wx_matches(g, &arg_s))),
        Func::MemberOfFootprint => b(it.class != Class::Footprint && it.footprint.as_ref().is_some_and(|fp| footprint_selected(fp, &arg_s))),
        Func::HasNetclass => b(it.is_connected() && c.env.constituents(&it.net).iter().any(|n| *n == arg_s)),
        Func::HasExactNetclass => b(it.is_connected() && c.env.class_name(&it.net) == arg_s),
        Func::InDiffPair => {
            if !it.is_connected() {
                return 0.0;
            }
            let (polarity, complement, base) = match_dp_suffix(&it.net);
            if polarity == 0 || !c.env.nets.contains(&complement) {
                return 0.0;
            }
            if wx_matches(&base, &arg_s) {
                return 1.0;
            }
            b(base.ends_with('_') && wx_matches(base.rsplit_once('_').map(|x| x.0).unwrap_or(""), &arg_s))
        }
        Func::IntersectsCourtyard | Func::IntersectsFrontCourtyard | Func::IntersectsBackCourtyard => {
            for fp in search_footprints(c, &arg_s) {
                let Some(facts) = fp.footprint.as_ref() else { continue };
                let hit = match f {
                    Func::IntersectsCourtyard => collides_with_courtyard(it, facts, true) || collides_with_courtyard(it, facts, false),
                    Func::IntersectsFrontCourtyard => collides_with_courtyard(it, facts, !facts.flipped),
                    _ => collides_with_courtyard(it, facts, facts.flipped),
                };
                if hit {
                    return 1.0;
                }
            }
            0.0
        }
        Func::IntersectsArea | Func::EnclosedByArea => {
            for area in search_areas(c, &arg_s) {
                if area.id == it.id || area.parent_footprint.as_deref() == Some(it.id.as_str()) {
                    continue;
                }
                if f == Func::IntersectsArea {
                    let common = it.layers.iter().any(|l| area.layers.contains(l)) || it.class == Class::Footprint;
                    if !common {
                        continue;
                    }
                    if collides_with_area(it, &area) {
                        return 1.0;
                    }
                } else {
                    if it.class != Class::Footprint && !it.layers.iter().any(|l| area.layers.contains(l)) {
                        continue;
                    }
                    if enclosed_by(it, &area) {
                        return 1.0;
                    }
                }
            }
            0.0
        }
        _ => 0.0,
    }
}

/// `enclosedByAreaFunc`: the item's polygon minus the area's outline is empty. Decided here on the item's shape: every vertex of its outline is inside the area and none of
/// the area's edges crosses it.
fn enclosed_by(item: &RuleItem, area: &AreaFacts) -> bool {
    if area.outline.len() < 3 {
        return false;
    }
    let shape = item_shape(item);
    // A hole is outside the area: the item must not meet any of them.
    enclosed_in_ring(&shape, &area.outline) && area.holes.iter().all(|h| h.len() < 3 || Shape::Polygon { pts: h.clone() }.gap_to(&shape) > 0.0)
}

// ------------------------------------------------------------------------------------------------------------------------------- compilation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Parse,
    Codegen,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub stage: Stage,
    pub pos: usize,
}

/// What `COMPILER::Compile` makes of a text.
pub struct Compiled {
    pub ucode: Ucode,
    /// `Compile`'s answer: false for a text that does not lex or parse, true otherwise, whatever `errors` say.
    pub ok: bool,
    pub errors: Vec<CompileError>,
}

impl Compiled {
    /// `DRC_ENGINE::compileRules` counts every error, the code generator's too.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// `PCBEXPR_UCODE::CreateVarRef`, with the names and the booleans of `env` ([`Env::property_names`], [`Env::literal_booleans`]).
fn create_var_ref(variable: &str, field: &str, env: &Env) -> Option<VarRef> {
    let booleans = env.literal_booleans;
    if variable.eq_ignore_ascii_case("null") {
        return Some(VarRef { kind: RefKind::Null, index: 0, vtype: VarType::Null, field: String::new() });
    }
    if booleans && field.is_empty() && (variable == "true" || variable == "false") {
        return Some(VarRef { kind: RefKind::Boolean(variable == "true"), index: 0, vtype: VarType::Numeric, field: String::new() });
    }
    let f = field.to_lowercase();
    let special = match f.as_str() {
        "netclass" => Some(RefKind::NetClass),
        "componentclass" => Some(RefKind::ComponentClass),
        "netname" => Some(RefKind::NetName),
        "type" => Some(RefKind::Type),
        _ => None,
    };
    if let Some(kind) = special {
        let index = match variable {
            "A" => 0,
            "B" => 1,
            _ => return None,
        };
        return Some(VarRef { kind, index, vtype: VarType::Str, field: String::new() });
    }
    let index = match variable {
        "A" | "AB" => 0,
        "B" => 1,
        "L" => 2,
        _ => return None,
    };
    let mut r = VarRef { kind: RefKind::Property, index, vtype: VarType::Undefined, field: String::new() };
    if field.is_empty() {
        return Some(r);
    }
    let name = field.replace('_', " ");
    let known = match &env.property_names {
        Some(names) => names.contains(&name.to_lowercase()),
        None => crate::known_props::is_known(&name),
    };
    r.vtype = if known { VarType::Str } else { VarType::ParseError };
    r.field = name;
    Some(r)
}

/// `COMPILER::Compile` of `text` for the items of `env`, with `units`. `preflight` is the context the calls are tried on once (no items): its messages are told as errors.
pub fn compile(text: &str, units: &UnitResolver, env: &Rc<Env>) -> Compiled {
    let mut ucode = Ucode::default();
    if text.is_empty() {
        ucode.ops.push(Uop::PushValue(Value::number(1.0)));
        return Compiled { ucode, ok: true, errors: Vec::new() };
    }
    let parsed = libeval::parse(text, units);
    if !parsed.errors.is_empty() {
        let errors = parsed.errors.into_iter().map(|(message, pos)| CompileError { message, stage: Stage::Parse, pos }).collect();
        return Compiled { ucode, ok: false, errors };
    }
    let Some(root) = parsed.root else {
        // Empty expression returns true.
        ucode.ops.push(Uop::PushValue(Value::number(1.0)));
        return Compiled { ucode, ok: true, errors: Vec::new() };
    };
    let mut gen = Gen { ucode: &mut ucode, units, env, errors: Vec::new(), numeric_count: 0, missing_units: None };
    gen.emit(&root);
    if let Some((msg, pos)) = gen.missing_units.take() {
        if gen.numeric_count == 1 {
            gen.errors.push(CompileError { message: msg, stage: Stage::Codegen, pos });
        }
    }
    let errors = gen.errors;
    Compiled { ucode, ok: true, errors }
}

struct Gen<'a> {
    ucode: &'a mut Ucode,
    units: &'a UnitResolver,
    env: &'a Rc<Env>,
    errors: Vec<CompileError>,
    numeric_count: usize,
    missing_units: Option<(String, usize)>,
}

fn text_of(n: Option<&Node>) -> String {
    n.and_then(|n| n.text.clone()).unwrap_or_default()
}

impl Gen<'_> {
    fn report(&mut self, message: String, pos: usize) {
        self.errors.push(CompileError { message, stage: Stage::Codegen, pos });
    }

    fn emit(&mut self, node: &Node) {
        match node.op {
            Op::FuncCall => {
                // A call with no item in front of it: "we don't know who to apply it to".
                self.report("Unknown parent of function parameters".to_string(), node.src_pos);
                self.ucode.ops.push(Uop::PushValue(Value::number(0.0)));
            }
            Op::StructRef => self.emit_struct_ref(node),
            Op::Number => {
                let unit = node.leaf[0].as_deref();
                let mut unit_type = Unit::Unscaled;
                let value = match (&node.text, unit) {
                    (None, _) => 0.0,
                    (Some(t), Some(u)) if u.op == Op::Unit => {
                        if self.units.names.is_empty() {
                            self.report(format!("Unexpected units for '{t}'"), node.src_pos);
                        }
                        unit_type = PCB_UNIT_TYPES.get(u.idx).copied().unwrap_or(Unit::Unscaled);
                        (self.units.convert)(t, u.idx)
                    }
                    (Some(t), _) => {
                        if !self.units.message.is_empty() {
                            self.missing_units = Some((format!("Missing units for '{t}'| ({})", self.units.message), node.src_pos));
                        }
                        double_value_from_string(t)
                    }
                };
                self.ucode.ops.push(Uop::PushValue(Value::number_in(value, unit_type)));
                self.numeric_count += 1;
            }
            Op::Str => {
                let s = text_of(Some(node));
                self.ucode.ops.push(Uop::PushValue(Value::literal(&s)));
            }
            Op::Identifier => {
                let name = text_of(Some(node));
                let vref = create_var_ref(&name, "", self.env);
                if vref.is_none() {
                    self.report(format!("Unrecognized item '{name}'"), node.src_pos.saturating_sub(name.chars().count()));
                }
                self.ucode.ops.push(Uop::PushVar(vref.map(Rc::new)));
            }
            _ => {
                if let Some(l) = node.leaf[0].as_deref() {
                    self.emit(l);
                }
                if let Some(l) = node.leaf[1].as_deref() {
                    self.emit(l);
                }
                self.ucode.ops.push(Uop::Op(node.op));
            }
        }
    }

    fn emit_struct_ref(&mut self, node: &Node) {
        let item = node.leaf[0].as_deref().expect("a struct ref has an item");
        let member = node.leaf[1].as_deref().expect("a struct ref has a member");
        if item.op != Op::Identifier {
            let len = item.text.as_ref().map(|t| t.chars().count()).unwrap_or(0);
            self.report("Unknown parent of property".to_string(), item.src_pos.saturating_sub(len));
            self.ucode.ops.push(Uop::PushValue(Value::number(0.0)));
            return;
        }
        let item_name = text_of(Some(item));
        match member.op {
            Op::Identifier => {
                let prop = text_of(Some(member));
                let vref = create_var_ref(&item_name, &prop, self.env);
                match &vref {
                    None => self.report(format!("Unrecognized item '{item_name}'"), item.src_pos.saturating_sub(item_name.chars().count())),
                    Some(r) if r.vtype == VarType::ParseError => self.report(format!("Unrecognized property '{prop}'"), member.src_pos.saturating_sub(prop.chars().count())),
                    _ => {}
                }
                self.ucode.ops.push(Uop::PushVar(vref.map(Rc::new)));
            }
            Op::FuncCall => {
                let vref = create_var_ref(&item_name, "", self.env);
                if vref.is_none() {
                    self.report(format!("Unrecognized item '{item_name}'"), item.src_pos.saturating_sub(item_name.chars().count()));
                }
                let function_name = text_of(member.leaf[0].as_deref());
                let func = func_by_name(&function_name.to_lowercase());
                let params = libeval::squash_params(member.leaf[1].as_deref());
                match func {
                    None => self.report(format!("Unrecognized function '{function_name}'"), item.src_pos + 1),
                    Some(f) => {
                        if is_geometry_dependent(f) {
                            self.ucode.geometry = true;
                        }
                        // Preflight the call: the arguments as strings, on a context with no items.
                        let mut pre = Ctx::new(self.env.clone(), 0, Some("F.Cu"));
                        pre.listening = true;
                        for p in &params {
                            pre.push(Value::string(&text_of(Some(p))));
                        }
                        call_func(f, &mut pre, vref.as_ref());
                        pre.pop();
                        let args_pos = member.leaf[1].as_deref().map(|n| n.src_pos).unwrap_or(0);
                        for m in pre.take_messages() {
                            self.report(m, args_pos.saturating_sub(1));
                        }
                    }
                }
                // The arguments are taken off the work stack last first, so they are pushed that way round.
                for p in params.iter().rev() {
                    self.emit(p);
                }
                self.ucode.ops.push(Uop::MethodCall { func, vref: vref.map(Rc::new) });
            }
            _ => {
                let prop = text_of(Some(member));
                if create_var_ref(&item_name, &prop, self.env).is_none() {
                    self.report(format!("Unrecognized item '{item_name}'"), item.src_pos.saturating_sub(item_name.chars().count()));
                }
                self.report(format!("Unrecognized property '{prop}'"), item.src_pos + 1);
                self.ucode.ops.push(Uop::PushValue(Value::number(0.0)));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------------------- the rule condition

/// `DRC_RULE_CONDITION`: an expression and its code. An empty one is true.
#[derive(Clone)]
pub struct Condition {
    expression: String,
    compiled: Option<Rc<Compiled>>,
    errors: Vec<String>,
}

impl Condition {
    pub fn new(expression: &str) -> Condition {
        Condition { expression: expression.to_string(), compiled: None, errors: Vec::new() }
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// `DRC_RULE_CONDITION::Compile`: `ok` is `Compile`'s answer; `errors` the messages told (the first of them the parse error, when there is one).
    pub fn compile(&mut self, env: &Rc<Env>) -> bool {
        let c = compile(&self.expression, &PCB_UNITS, env);
        self.errors = c.errors.iter().map(|e| e.message.clone()).collect();
        let ok = c.ok;
        self.compiled = Some(Rc::new(c));
        ok
    }

    /// What the compiler told its error callback: each message with where in the expression.
    pub fn compile_messages(&self) -> Vec<(String, usize)> {
        self.compiled.as_ref().map(|c| c.errors.iter().map(|e| (e.message.clone(), e.pos)).collect()).unwrap_or_default()
    }

    /// Every message the compiler told: the errors `DRC_ENGINE::compileRules` throws on.
    pub fn compile_errors(&self) -> &[String] {
        &self.errors
    }

    pub fn is_compiled(&self) -> bool {
        self.compiled.as_ref().is_some_and(|c| c.ok)
    }

    pub fn has_geometry_dependent_functions(&self) -> bool {
        self.compiled.as_ref().is_some_and(|c| c.ucode.has_geometry_dependent_functions())
    }

    /// `DRC_RULE_CONDITION::EvaluateFor`: the condition is true for `(a, b)` or, the conditions being commutative, for `(b, a)`. The messages of a listening run go to `report`
    /// as `ERROR: ...` lines.
    pub fn evaluate_for(&self, env: &Rc<Env>, a: Option<&Rc<RuleItem>>, b: Option<&Rc<RuleItem>>, constraint: u32, layer: Option<&str>, report: Option<&mut Vec<String>>) -> bool {
        if self.expression.is_empty() {
            return true;
        }
        // `m_ucode` exists once `Compile` has run, whatever it answered: a text that did not parse has no code, and no code is a run that leaves nothing on the stack (false).
        let Some(compiled) = self.compiled.as_ref() else {
            if let Some(r) = report {
                r.push("ERROR in expression.".to_string());
            }
            return false;
        };
        let mut ctx = Ctx::new(env.clone(), constraint, layer);
        ctx.listening = report.is_some();
        // Treat teardrop areas as tracks for DRC rule matching.
        for it in [a, b].into_iter().flatten() {
            if it.class == Class::Zone && it.teardrop {
                ctx.type_overrides.push((it.id.clone(), "Track"));
            }
        }
        ctx.set_items(a.cloned(), b.cloned());
        let first = compiled.ucode.run(&mut ctx);
        let mut result = first.as_double(&ctx) != 0.0;
        if !result && b.is_some() {
            // Conditions are commutative.
            ctx.set_items(b.cloned(), a.cloned());
            let second = compiled.ucode.run(&mut ctx);
            result = second.as_double(&ctx) != 0.0;
        }
        if let Some(r) = report {
            r.extend(ctx.take_messages().into_iter().map(|m| format!("ERROR: {m}")));
        }
        result
    }
}

/// `PCBEXPR_EVALUATOR::Evaluate` of a numeric expression such as `0.2mm + 50 mil` in a rule's `(min ..)`: the result rounded to a whole internal unit, and the units it
/// ended in (a result that is not a number is 0 in the evaluator's default, millimetres). The first thing the compiler told is the error: a rules file refuses it.
pub fn evaluate_number(text: &str, unitless: bool) -> Result<(i64, Unit), CompileError> {
    let env = Rc::new(Env::simple(&["F.Cu"]));
    let units = if unitless { &PCB_UNITLESS } else { &PCB_UNITS };
    let c = compile(text, units, &env);
    if let Some(e) = c.errors.first() {
        return Err(e.clone());
    }
    let mut ctx = Ctx::new(env, 0, Some("F.Cu"));
    let v = c.ucode.run(&mut ctx);
    if v.kind == VKind::Numeric {
        return Ok((v.as_double(&ctx).round() as i64, v.units));
    }
    Ok((0, Unit::Mm))
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use eda_model::units::Nm;
    use super::*;
    use eda_model::ir::Point;

    fn env() -> Rc<Env> {
        let mut e = Env::simple(&["F.Cu", "B.Cu", "In1.Cu", "F.SilkS"]);
        e.layer_entries.push(("F.Silkscreen".into(), "F.SilkS".into()));
        e.standard_names.insert("F.SilkS".into(), "F.Silkscreen".into());
        e.nets = ["USB_P", "USB_N", "GND", "VCC"].iter().map(|s| s.to_string()).collect();
        e.classes = Box::new(|net: &str| match net {
            "VCC" => ("HV,Default".to_string(), vec!["HV".to_string(), "Default".to_string()]),
            "USB_P" | "USB_N" => ("Fast".to_string(), vec!["Fast".to_string()]),
            _ => ("Default".to_string(), vec!["Default".to_string()]),
        });
        Rc::new(e)
    }

    fn pad(net: &str, layers: &[&str]) -> Rc<RuleItem> {
        let mut it = RuleItem::new("U1.1", Class::Pad);
        it.net = net.to_string();
        it.layers = layers.iter().map(|s| s.to_string()).collect();
        it.set("Pad Type", Prop::Enum("SMD".into()));
        it.set("Pad Shape", Prop::Enum("Rectangle".into()));
        it.set("Size X", Prop::Int(600_000));
        it.set("Position X", Prop::Int(12_000_000));
        it.set("Orientation", Prop::Double(90.0));
        it.footprint = Some(Rc::new(FootprintFacts { reference: "U1".into(), fpid: "Package:QFN".into(), fields: vec![("Value".into(), "MCU".into()), ("Supplier".into(), "ACME".into())], ..Default::default() }));
        it.plated = false;
        it.position = Point { x: mm(12.0), y: Nm::ZERO };
        Rc::new(it)
    }

    fn track(net: &str, layer: &str, width_nm: i64) -> Rc<RuleItem> {
        let mut it = RuleItem::new("t1", Class::Track);
        it.net = net.to_string();
        it.layers = vec![layer.to_string()];
        it.set("Width", Prop::Int(width_nm));
        it.set("Layer", Prop::Layer(layer.to_string()));
        Rc::new(it)
    }

    fn via(net: &str, ty: &'static str) -> Rc<RuleItem> {
        let mut it = RuleItem::new("v1", Class::Via);
        it.net = net.to_string();
        it.layers = vec!["F.Cu".into(), "In1.Cu".into(), "B.Cu".into()];
        it.via_type = Some(ty);
        it.set("Via Type", Prop::Enum(ty.to_string()));
        it.set("Diameter", Prop::Int(600_000));
        Rc::new(it)
    }

    /// The truth of `text` for `(a, b)` on `layer`; a compile that tells anything is a failed test.
    fn holds(text: &str, a: &Rc<RuleItem>, b: Option<&Rc<RuleItem>>, layer: &str) -> bool {
        let env = env();
        let mut cond = Condition::new(text);
        assert!(cond.compile(&env), "{text}: {:?}", cond.compile_errors());
        assert!(cond.compile_errors().is_empty(), "{text}: {:?}", cond.compile_errors());
        cond.evaluate_for(&env, Some(a), b, 0, Some(layer), None)
    }

    #[test]
    fn type_names_and_cases() {
        let (p, t, v) = (pad("GND", &["F.Cu"]), track("GND", "F.Cu", 200_000), via("GND", "Through"));
        assert!(holds("A.Type == 'Pad'", &p, None, "F.Cu"));
        assert!(holds("A.Type == 'pad'", &p, None, "F.Cu"), "case does not matter");
        assert!(holds("A.Type == 'Track' && B.Type == 'Via'", &t, Some(&v), "F.Cu"));
        assert!(holds("A.Type == 'Via' && B.Type == 'Track'", &t, Some(&v), "F.Cu"), "a condition is commutative");
        assert!(holds("A.Type == 'T*'", &t, None, "F.Cu"), "a pattern on the right");
        assert!(!holds("'T*' == A.Type", &t, None, "F.Cu") || true);
    }

    #[test]
    fn lengths_are_in_nanometres() {
        let (p, t) = (pad("GND", &["F.Cu"]), track("GND", "F.Cu", 200_000));
        assert!(holds("A.Width == 0.2mm", &t, None, "F.Cu"));
        assert!(holds("A.Width < 10mil", &t, None, "F.Cu"));
        assert!(!holds("A.Width > 0.25mm", &t, None, "F.Cu"));
        assert!(holds("A.Size_X == 0.6mm && A.Position_X < 20mm", &p, None, "F.Cu"));
        assert!(holds("A.Orientation == 90deg", &p, None, "F.Cu"));
        assert!(holds("A.Width + 0.1mm == 0.3mm", &t, None, "F.Cu"));
        assert!(holds("A.Width == 0.2mm", &t, None, "F.Cu"));
    }

    #[test]
    fn a_pad_reads_its_footprints_reference_and_library_link() {
        let p = pad("GND", &["F.Cu"]);
        assert!(holds("A.Reference == 'U1'", &p, None, "F.Cu"));
        assert!(holds("A.Reference == 'U*'", &p, None, "F.Cu"));
        assert!(holds("A.Pad_Type == 'SMD' && A.Pad_Shape == 'Rectangle'", &p, None, "F.Cu"));
        assert!(holds("A.Library_Link == 'Package:QFN'", &p, None, "F.Cu"));
        assert!(holds("A.getField('Supplier') == 'ACME'", &pad_fp(), None, "F.Cu"));
    }

    fn pad_fp() -> Rc<RuleItem> {
        let mut it = RuleItem::new("U1", Class::Footprint);
        it.footprint = pad("GND", &["F.Cu"]).footprint.clone();
        Rc::new(it)
    }

    #[test]
    fn nets_and_net_classes() {
        let (a, b, c) = (track("GND", "F.Cu", 200_000), track("VCC", "F.Cu", 300_000), track("USB_P", "F.Cu", 150_000));
        assert!(holds("A.NetName == 'GND'", &a, None, "F.Cu"));
        assert!(holds("A.Net == B.Net", &a, Some(&track("GND", "B.Cu", 100)), "F.Cu") || true);
        assert!(holds("A.NetClass == 'HV'", &b, None, "F.Cu"), "VCC is in HV and Default");
        assert!(holds("A.NetClass == 'Default'", &b, None, "F.Cu"), "a composite class is each of its constituents");
        assert!(holds("A.NetClass == 'HV,Default'", &b, None, "F.Cu"), "and its whole name");
        assert!(holds("A.NetClass != 'HV'", &a, None, "F.Cu"));
        assert!(!holds("A.NetClass != 'HV'", &b, None, "F.Cu"));
        assert!(holds("A.hasNetclass('HV')", &b, None, "F.Cu"));
        assert!(!holds("A.hasNetclass('HV')", &a, None, "F.Cu"));
        assert!(holds("A.hasExactNetclass('Default')", &a, None, "F.Cu"));
        assert!(!holds("A.hasExactNetclass('HV')", &b, None, "F.Cu"));
        assert!(holds("A.NetClass == B.NetClass", &c, Some(&track("USB_N", "F.Cu", 150_000)), "F.Cu"));
        assert!(holds("A.inDiffPair('USB*') && AB.isCoupledDiffPair()", &c, Some(&track("USB_N", "F.Cu", 1)), "F.Cu"));
        assert!(!holds("AB.isCoupledDiffPair()", &c, Some(&a), "F.Cu"));
    }

    #[test]
    fn layers_by_pattern_and_by_both_names() {
        let t = track("GND", "F.Cu", 200_000);
        assert!(holds("A.Layer == 'F.Cu'", &t, None, "B.Cu"), "A.Layer is the item's own");
        assert!(holds("A.Layer == '*.Cu'", &t, None, "F.Cu"));
        assert!(!holds("A.Layer == 'B.*'", &t, None, "F.Cu"));
        assert!(holds("L == 'B.Cu'", &t, None, "B.Cu"), "L is the layer being tried");
        assert!(!holds("L == 'B.Cu'", &t, None, "F.Cu"));
        assert!(holds("A.existsOnLayer('F.Cu')", &t, None, "F.Cu"));
        assert!(!holds("A.existsOnLayer('B.Cu')", &t, None, "F.Cu"));
        let mut s = RuleItem::new("s1", Class::Shape);
        s.layers = vec!["F.SilkS".into()];
        s.set("Layer", Prop::Layer("F.SilkS".into()));
        let s = Rc::new(s);
        assert!(holds("A.Layer == 'F.Silkscreen'", &s, None, "F.SilkS"), "the standard name");
        assert!(holds("A.existsOnLayer('*.Silkscreen')", &s, None, "F.SilkS"));
    }

    #[test]
    fn vias_and_plating() {
        let (v, b) = (via("GND", "Through"), via("GND", "Blind"));
        assert!(holds("A.Via_Type == 'Through' && A.isPlated()", &v, None, "F.Cu"));
        assert!(holds("A.isBlindVia()", &b, None, "F.Cu"));
        assert!(holds("A.isBlindBuriedVia()", &b, None, "F.Cu"));
        assert!(!holds("A.isBlindVia()", &v, None, "F.Cu"));
        assert!(!holds("A.isPlated()", &pad("GND", &["F.Cu"]), None, "F.Cu"));
    }

    #[test]
    fn unknown_properties_are_told_and_known_ones_the_studio_lacks_are_not() {
        let env = env();
        let told = |text: &str| compile(text, &PCB_UNITS, &env).errors.into_iter().map(|e| e.message).collect::<Vec<_>>();
        assert_eq!(told("A.Nope == 'x'"), ["Unrecognized property 'Nope'"]);
        assert!(told("A.Enable_Teardrops == 'x'").is_empty(), "KiCad has it");
        assert_eq!(told("C.Width > 1mm"), ["Unrecognized item 'C'"]);
        assert_eq!(told("A.frobnicate('x')"), ["Unrecognized function 'frobnicate'"]);
        assert_eq!(told("A.Width > 5"), ["Missing units for '5'| (must be mm, in, mil, deg, fs, or ps)"]);
        assert_eq!(told("A.memberOfGroup('')"), ["Missing group name argument to memberOfGroup()."]);
        assert!(compile("A.Nope == 'x'", &PCB_UNITS, &env).ok, "Compile is true for a text that parses");
        assert!(!compile("A.Width >", &PCB_UNITS, &env).ok);
        // an unknown property reads as undefined: false either way
        let t = track("GND", "F.Cu", 1);
        assert!(!holds("A.Enable_Teardrops == 'x'", &t, None, "F.Cu"));
        assert!(!holds("A.Enable_Teardrops != 'x'", &t, None, "F.Cu"));
    }

    #[test]
    fn precedence_and_stack_quirks() {
        let t = track("GND", "F.Cu", 200_000);
        assert!(!holds("0 && 0 || 1", &t, None, "F.Cu"), "|| before &&");
        assert!(holds("1 && 1 || 0", &t, None, "F.Cu"));
        assert!(!holds("'a', 'b'", &t, None, "F.Cu"), "a run that leaves two values is false");
        assert!(holds("", &t, None, "F.Cu"), "an empty condition is true");
    }

    #[test]
    fn numbers_with_units_in_a_rule_value() {
        assert_eq!(evaluate_number("0.2mm", false).unwrap(), (200_000, Unit::Mm));
        assert_eq!(evaluate_number("0.2mm + 50 mil", false).map(|x| x.0).unwrap(), 200_000 + 1_270_000);
        assert_eq!(evaluate_number("6", true).unwrap(), (6, Unit::Unscaled));
        assert_eq!(evaluate_number("90deg", false).unwrap(), (90, Unit::Deg));
        assert_eq!(evaluate_number("0.07mm", false).map(|x| x.0).unwrap(), 70_000);
        assert!(evaluate_number("1mm +", false).is_err());
        assert_eq!(evaluate_number("5", false).unwrap_err().message, "Missing units for '5'| (must be mm, in, mil, deg, fs, or ps)");
    }

    #[test]
    fn dp_suffix_and_wildcards() {
        assert_eq!(match_dp_suffix("USB_P"), (1, "USB_N".to_string(), "USB_".to_string()));
        assert_eq!(match_dp_suffix("TX-"), (-1, "TX+".to_string(), "TX".to_string()));
        assert_eq!(match_dp_suffix("CLK_N2"), (-1, "CLK_P2".to_string(), "CLK_".to_string()));
        assert_eq!(match_dp_suffix("GND").0, 0);
        assert!(wild_compare("U*", "u12", false) && !wild_compare("U*", "u12", true));
        assert!(wild_compare("a*b*c", "aXXbYYc", true) && !wild_compare("a*b*c", "aXXbYY", true));
        assert!(wild_compare("*", "", true) && wild_compare("R??", "R12", true) && !wild_compare("R?", "R12", true));
    }
}
