//! Find by Properties' Query tab on the backend: `DIALOG_FIND_BY_PROPERTIES::selectMatchingFromQuery` and `onCheckSyntaxClick`
//! (`pcbnew/dialogs/dialog_find_by_properties.cpp`) with the one expression language of the workspace ([`crate::pcbexpr`] over [`crate::libeval`]), so the
//! studio keeps no second copy of it (the TypeScript one, `web/studio/src/kicad-port/pcbExpr.ts` and `libeval.ts`, is INACTIVE since 2026-10-28).
//!
//! The items come from the studio, as its Properties panel sees them (`web/studio/src/kicad-port/pcbQuery.ts` builds the request): each item's class,
//! layers, net, the properties the panel has for it (available ones only; a pad or field also carries its footprint's, under its own), its footprint
//! (reference, library id, fields), its groups (innermost first), and the via and plating facts the functions ask. The board's facts are the layer names
//! the language knows, the standard names, the nets and each net's class. The panel's property model stays where the panel is; only the language moved.
//!
//! An item matches when the expression is not zero on any layer it is on (`L` is that layer, `A` the item, there is no `B`), as in
//! `selectMatchingFromQuery`. `true` and `false` are 1 and 0 ([`crate::pcbexpr::Env::literal_booleans`]): the queries Create from Selection Tab writes
//! use them. `memberOfSheet`, `hasComponentClass` and `A.ComponentClass` are KiCad's: the studio sends no sheet path and no classes, and the server
//! fills each footprint's from the board's rule board (`GetSheetname`, `GetComponentClass`: [`handle_with`], R6-D3); without them a footprint is on the
//! root sheet, `/`, with no class. The functions that need shapes (`intersectsCourtyard`, `intersectsArea`, `enclosedByArea` and their old names) find no courtyard or area
//! here and say false, as the TypeScript copy did.
//!
//! Request: `{ "expression": "...", "items": [...], "env": {...} }` (see [`Request`]). Answer: `{ "ok": bool, "errors": [..], "ids": [..] | null }`:
//! `ok` is `Compile`'s answer (the text lexes and parses), `errors` every message the compiler told, and `ids` the matches in the order of the items
//! (null when it does not compile).

use crate::pcbexpr::{compile, Ctx, Env, PCB_UNITS};
use crate::rule_item::{Class, FootprintFacts, Prop, RuleItem};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// One property of an item: its name as the registry spells it and its value by the kind of the property.
#[derive(Debug, Deserialize)]
pub struct QueryProp {
    pub name: String,
    /// `int`, `double`, `bool`, `str`, `enum` (the choice's label), `net` or `layer`.
    pub kind: String,
    pub value: Value,
}

#[derive(Debug, Deserialize, Default)]
pub struct QueryFootprint {
    pub reference: String,
    pub fpid: String,
    #[serde(default)]
    pub fields: Vec<(String, String)>,
    /// `FOOTPRINT::GetSheetname()` and the names of `GetComponentClass()`'s classes. The studio sends neither; the server fills them from the board's rule
    /// board ([`run_with`], R6-D3), as `selectMatchingFromQuery` reads the board's real footprints.
    #[serde(default)]
    pub sheet: Option<String>,
    #[serde(default)]
    pub classes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct QueryItem {
    pub id: String,
    /// The studio's class (`FOOTPRINT`, `PAD`, `PCB_TRACK`, ...).
    pub class: String,
    /// What `A.Type` says (`Footprint`, `Track`, `Graphic`, `Text Box`, ...).
    pub type_name: String,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub net: String,
    #[serde(default)]
    pub props: Vec<QueryProp>,
    /// A footprint's own facts, or the footprint of a pad or field.
    #[serde(default)]
    pub footprint: Option<QueryFootprint>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub plated: bool,
    /// `Through`, `Blind`, `Buried` or `Micro` for a via.
    #[serde(default)]
    pub via_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct QueryEnv {
    /// `ENUM_MAP<PCB_LAYER_ID>`: `[text, layer]` for each name a layer is known by.
    #[serde(default)]
    pub layer_entries: Vec<(String, String)>,
    /// `LayerName( layer )` where it is not the layer's own name.
    #[serde(default)]
    pub standard_names: HashMap<String, String>,
    #[serde(default)]
    pub nets: Vec<String>,
    /// For each net, its effective class's name and its constituents.
    #[serde(default)]
    pub classes: HashMap<String, (String, Vec<String>)>,
    /// Every property name the studio's registry has for a board item (`PCBEXPR_UCODE::CreateVarRef` asks the property manager, whose properties
    /// these are): a name not among them is "Unrecognized property".
    #[serde(default)]
    pub properties: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Request {
    pub expression: String,
    #[serde(default)]
    pub items: Vec<QueryItem>,
    #[serde(default)]
    pub env: QueryEnv,
}

/// The rule engine's class of a studio class: what decides "connected" and "a footprint" for the language.
fn class_of(studio: &str) -> Class {
    match studio {
        "FOOTPRINT" => Class::Footprint,
        "PAD" => Class::Pad,
        "PCB_TRACK" => Class::Track,
        "PCB_ARC" => Class::Arc,
        "PCB_VIA" => Class::Via,
        "ZONE" => Class::Zone,
        "PCB_FIELD" => Class::Field,
        "PCB_SHAPE" => Class::Shape,
        "PCB_GROUP" => Class::Group,
        "PCB_DIM_LEADER" => Class::Leader,
        s if s.starts_with("PCB_DIM") => Class::Dimension,
        // Texts, text boxes, tables, barcodes, images, points: not connected, not footprints.
        _ => Class::Text,
    }
}

fn prop_of(p: &QueryProp) -> Option<Prop> {
    let v = &p.value;
    Some(match p.kind.as_str() {
        "int" | "double" => Prop::Double(v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))?),
        "bool" => Prop::Bool(v.as_bool().unwrap_or_else(|| v.as_f64().is_some_and(|n| n != 0.0))),
        "layer" => Prop::Layer(v.as_str()?.to_string()),
        "enum" | "net" => Prop::Enum(match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
        _ => Prop::Str(match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }),
    })
}

fn rule_item(q: &QueryItem, facts: &dyn Fn(&str) -> Option<(String, Vec<String>)>) -> RuleItem {
    let mut it = RuleItem::new(q.id.clone(), class_of(&q.class));
    it.type_name = Some(q.type_name.clone());
    it.layers = q.layers.clone();
    it.layer = q.layers.first().cloned();
    it.net = q.net.clone();
    for p in &q.props {
        if let Some(v) = prop_of(p) {
            it.set(&p.name, v);
        }
    }
    it.footprint = q.footprint.as_ref().map(|f| {
        // the footprint's sheet and classes: the request's, else the board's (`memberOfSheet`, `hasComponentClass`, `A.ComponentClass`)
        let board = (f.sheet.is_none() || f.classes.is_none()).then(|| facts(&f.reference)).flatten().unwrap_or_default();
        let sheet = f.sheet.clone().unwrap_or(board.0);
        let classes = f.classes.clone().unwrap_or(board.1);
        Rc::new(FootprintFacts { reference: f.reference.clone(), fpid: f.fpid.clone(), fields: f.fields.clone(), sheet, classes, ..Default::default() })
    });
    it.groups = q.groups.clone();
    it.plated = q.plated;
    it.via_type = match q.via_type.as_deref() {
        Some("Through") => Some("Through"),
        Some("Blind") => Some("Blind"),
        Some("Buried") => Some("Buried"),
        Some("Micro") => Some("Micro"),
        _ => None,
    };
    it
}

fn env_of(e: &QueryEnv) -> Env {
    let classes = e.classes.clone();
    let mut env = Env::new(
        e.layer_entries.clone(),
        e.standard_names.clone(),
        Box::new(move |net: &str| classes.get(net).cloned().unwrap_or_else(|| ("Default".to_string(), vec!["Default".to_string()]))),
    );
    env.nets = e.nets.iter().cloned().collect::<HashSet<_>>();
    env.literal_booleans = true;
    env.property_names = e.properties.as_ref().map(|p| p.iter().map(|n| n.to_lowercase()).collect());
    env
}

/// The answer to one request.
pub fn run(req: &Request) -> Value {
    run_with(req, &|_| None)
}

/// [`run`] with `facts`: a footprint's sheet and component classes by its reference (the board's, `eda_drc::rule_board::RuleBoard`), for the footprints the
/// request sends without them.
pub fn run_with(req: &Request, facts: &dyn Fn(&str) -> Option<(String, Vec<String>)>) -> Value {
    let env = Rc::new(env_of(&req.env));
    let compiled = compile(&req.expression, &PCB_UNITS, &env);
    let errors: Vec<String> = compiled.errors.iter().map(|e| e.message.clone()).collect();
    if !compiled.ok {
        return json!({ "ok": false, "errors": errors, "ids": Value::Null });
    }
    let mut ids = Vec::new();
    for q in &req.items {
        let item = Rc::new(rule_item(q, facts));
        for layer in &q.layers {
            let mut ctx = Ctx::new(env.clone(), 0, Some(layer));
            ctx.set_items(Some(item.clone()), None);
            if compiled.ucode.run(&mut ctx).as_double(&ctx) != 0.0 {
                ids.push(q.id.clone());
                break;
            }
        }
    }
    json!({ "ok": true, "errors": errors, "ids": ids })
}

/// `POST /api/pcb/query`'s body to its answer.
pub fn handle(body: &[u8]) -> Value {
    handle_with(body, &|_| None)
}

/// [`handle`] with the board's footprint facts ([`run_with`]).
pub fn handle_with(body: &[u8], facts: &dyn Fn(&str) -> Option<(String, Vec<String>)>) -> Value {
    match serde_json::from_slice::<Request>(body) {
        Ok(req) => run_with(&req, facts),
        Err(e) => json!({ "ok": false, "errors": [format!("bad request: {e}")], "ids": Value::Null }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R6-D3: a footprint's sheet and component classes reach the language (`memberOfSheet`, `hasComponentClass`, `A.ComponentClass`), from the board's
    /// facts when the request has none, for the footprint and for a pad of it.
    #[test]
    fn a_footprints_sheet_and_classes_reach_the_query() {
        let body = |expression: &str| {
            json!({
                "expression": expression,
                "items": [
                    { "id": "R5", "class": "FOOTPRINT", "type_name": "Footprint", "layers": ["F.Cu"], "footprint": { "reference": "R5", "fpid": "R:0603" } },
                    { "id": "R5.1", "class": "PAD", "type_name": "Pad", "layers": ["F.Cu"], "footprint": { "reference": "R5", "fpid": "R:0603" } },
                    { "id": "R6", "class": "FOOTPRINT", "type_name": "Footprint", "layers": ["F.Cu"], "footprint": { "reference": "R6", "fpid": "R:0603", "sheet": "/LV/", "classes": [] } },
                ],
            })
            .to_string()
        };
        let facts = |r: &str| (r == "R5" || r == "R6").then(|| ("/HV/".to_string(), vec!["PWR".to_string()]));
        let ids = |expression: &str| handle_with(body(expression).as_bytes(), &facts)["ids"].clone();
        assert_eq!(ids("A.memberOfSheet('/HV/')"), json!(["R5", "R5.1"]), "the request's own sheet wins: R6 is on /LV/");
        assert_eq!(ids("A.hasComponentClass('PWR')"), json!(["R5", "R5.1"]));
        assert_eq!(ids("A.ComponentClass == 'PWR'"), json!(["R5", "R5.1"]));
        assert_eq!(handle(body("A.hasComponentClass('PWR')").as_bytes())["ids"], json!([]), "without the board's facts, no class");
    }
}
