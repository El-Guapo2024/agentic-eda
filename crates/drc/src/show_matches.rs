//! The DRC Rule Editor's "Show Matches" (`DIALOG_DRC_RULE_EDITOR::highlightMatchingItems` over `DRC_ENGINE::GetItemsMatchingRule`, KiCad 8303b2ad
//! `pcbnew/drc/rule_editor/dialog_drc_rule_editor.cpp` and `pcbnew/drc/drc_engine.cpp`): the items of the board a rule's condition selects, for the
//! dialog to select, frame and brighten.
//!
//! The rule is the text the open panel generates (`GetGeneratedRule`), parsed after `(version 1)` with no reporter: a text that does not parse, or a
//! condition that does not compile, is an error (`-1`, "Show Matches (error)"); a text that holds no rule matches nothing. Each constraint of the rule
//! looks in its own domain of items (`getShowMatchDomainSpec`: copper items for a clearance, footprints for a courtyard clearance, holes for a hole size,
//! ...); a condition that names `B.` is tried on pairs of items (`ruleMatchesPair`), any other on items alone (`ruleMatchesUnary`), on each layer the item is
//! on that the rule's layer condition takes (`getShowMatchLayers`).
//!
//! The board's items are the rule board's ([`RuleBoard::show_match_items`]); what is missing there (a footprint's own fields and graphics, dimensions) is
//! missing here. The count is of KiCad's items: each track segment counts, though the studio selects the track.

use crate::constraints::{CType, Rule};
use crate::dru;
use crate::pcbexpr::Env;
use crate::rule_board::RuleBoard;
use crate::rule_item::{Class, RuleItem};
use std::collections::BTreeSet;
use std::rc::Rc;

/// `SHOWMATCH_DOMAIN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    AllItems,
    CopperItems,
    EdgeItems,
    Footprints,
    HoleItems,
    MaskExpansionItems,
    MaskItems,
    Pads,
    PadsAndVias,
    PasteItems,
    RoutingItems,
    SilkItems,
    SilkTargetItems,
    TextItems,
    Vias,
}

/// `SHOWMATCH_DOMAIN_SPEC`: where a constraint's first and second items come from, and whether the second domain's items are also tried alone.
struct DomainSpec {
    primary: Domain,
    secondary: Option<Domain>,
    secondary_unary: bool,
}

const fn spec(primary: Domain) -> DomainSpec {
    DomainSpec { primary, secondary: None, secondary_unary: false }
}

/// `getShowMatchDomainSpec`.
fn domain_spec(c: CType) -> DomainSpec {
    use Domain::*;
    match c {
        CType::Clearance => spec(CopperItems),
        CType::EdgeClearance => DomainSpec { primary: CopperItems, secondary: Some(EdgeItems), secondary_unary: true },
        CType::HoleClearance => DomainSpec { primary: HoleItems, secondary: Some(AllItems), secondary_unary: false },
        CType::HoleToHole => spec(HoleItems),
        CType::CourtyardClearance => spec(Footprints),
        CType::PhysicalClearance | CType::PhysicalHoleClearance | CType::Creepage => spec(AllItems),
        CType::SilkClearance => DomainSpec { primary: SilkItems, secondary: Some(SilkTargetItems), secondary_unary: false },
        CType::SolderMaskSliver => spec(MaskItems),
        CType::TrackWidth | CType::TrackAngle | CType::TrackSegmentLength | CType::ConnectionWidth | CType::DiffPairGap | CType::MaxUncoupled | CType::Length | CType::Skew => spec(RoutingItems),
        CType::ViaDiameter | CType::ViaCount => spec(Vias),
        CType::HoleSize => spec(HoleItems),
        CType::AnnularWidth => spec(PadsAndVias),
        CType::MinResolvedSpokes => spec(Pads),
        CType::TextHeight | CType::TextThickness => spec(TextItems),
        CType::SolderMaskExpansion => spec(MaskExpansionItems),
        CType::SolderPasteAbsMargin | CType::SolderPasteRelMargin => spec(PasteItems),
        _ => spec(AllItems),
    }
}

fn on(it: &RuleItem, layer: &str) -> bool {
    it.layers.iter().any(|l| l == layer)
}

fn on_copper(it: &RuleItem) -> bool {
    it.layers.iter().any(|l| l.ends_with(".Cu"))
}

/// `matchesShowMatchDomain` (groups, generators and nets, `isShowMatchSkippable`, are not among the rule board's items).
fn in_domain(it: &RuleItem, d: Domain) -> bool {
    use Domain::*;
    match d {
        AllItems => true,
        CopperItems => on_copper(it),
        EdgeItems => on(it, "Edge.Cuts") || on(it, "Margin") || (it.class == Class::Pad && it.pad_attribute == "NPTH" && it.has_hole),
        Footprints => it.class == Class::Footprint,
        HoleItems => it.has_hole,
        MaskExpansionItems => matches!(it.class, Class::Pad | Class::Track | Class::Arc | Class::Via | Class::Shape | Class::Zone),
        MaskItems => on(it, "F.Mask") || on(it, "B.Mask"),
        Pads => it.class == Class::Pad,
        PadsAndVias => matches!(it.class, Class::Pad | Class::Via),
        PasteItems => on(it, "F.Paste") || on(it, "B.Paste"),
        RoutingItems => matches!(it.class, Class::Track | Class::Arc | Class::Via | Class::Pad),
        SilkItems => on(it, "F.SilkS") || on(it, "B.SilkS"),
        SilkTargetItems => {
            ["F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab", "Edge.Cuts", "Margin"].iter().any(|l| on(it, l)) || on_copper(it)
        }
        TextItems => matches!(it.class, Class::Field | Class::Text | Class::Dimension | Class::Leader),
        Vias => it.class == Class::Via,
    }
}

/// `aRule.m_LayerCondition.test( layer )`; no layer (`UNDEFINED_LAYER`) is always taken.
fn rule_takes(rule: &Rule, layer: Option<&str>) -> bool {
    match (layer, &rule.layers) {
        (None, _) | (_, None) => true,
        (Some(l), Some(set)) => set.contains(l),
    }
}

/// One candidate: the item and the layers `getShowMatchLayers` gives it (empty is `UNDEFINED_LAYER`).
struct Candidate<'a> {
    item: &'a Rc<RuleItem>,
    layers: &'a [String],
}

/// `ruleMatchesUnary`.
fn matches_unary(env: &Rc<Env>, rule: &Rule, c: &Candidate, ctype: CType) -> bool {
    let eval = |layer: Option<&str>| rule.condition.as_ref().is_none_or(|cond| cond.evaluate_for(env, Some(c.item), None, ctype as u32, layer, None));
    let mut tested = false;
    let layers: Vec<Option<&str>> = if c.layers.is_empty() { vec![None] } else { c.layers.iter().map(|l| Some(l.as_str())).collect() };
    for layer in layers {
        if layer.is_some() && !rule_takes(rule, layer) {
            continue;
        }
        tested = true;
        if eval(layer) {
            return true;
        }
    }
    // `!testedLayer && aItem->GetLayerSet().none()`
    !tested && c.item.layers.is_empty() && eval(None)
}

/// `getShowMatchPairLayers`: the edge clearance tries A's layers, the courtyard clearance no layer, the others A's and B's, each once, as the rule takes them.
fn pair_layers<'a>(rule: &Rule, a: &'a Candidate, b: &'a Candidate, ctype: CType) -> Vec<Option<&'a str>> {
    let mut out: Vec<Option<&str>> = Vec::new();
    let mut add = |layer: Option<&'a str>| {
        if layer.is_some() && !rule_takes(rule, layer) {
            return;
        }
        if !out.contains(&layer) {
            out.push(layer);
        }
    };
    let layers_of = |c: &'a Candidate| -> Vec<Option<&'a str>> { if c.layers.is_empty() { vec![None] } else { c.layers.iter().map(|l| Some(l.as_str())).collect() } };
    match ctype {
        CType::EdgeClearance => layers_of(a).into_iter().for_each(&mut add),
        CType::CourtyardClearance => add(None),
        _ => {
            for l in layers_of(a).into_iter().chain(layers_of(b)) {
                add(l);
            }
        }
    }
    if out.is_empty() {
        out.push(None);
    }
    out
}

/// `ruleMatchesPair`.
fn matches_pair(env: &Rc<Env>, rule: &Rule, a: &Candidate, b: &Candidate, ctype: CType) -> bool {
    pair_layers(rule, a, b, ctype).into_iter().any(|layer| rule.condition.as_ref().is_none_or(|cond| cond.evaluate_for(env, Some(a.item), Some(b.item), ctype as u32, layer, None)))
}

/// `DRC_ENGINE::GetItemsMatchingRule`: the indices (into `items`) of the items `rule` matches, each once, in the order of `items`.
fn items_matching_rule(env: &Rc<Env>, rule: &Rule, items: &[(String, Rc<RuleItem>, Vec<String>)]) -> Vec<usize> {
    let condition = rule.condition.as_ref().map(|c| c.expression().to_string()).unwrap_or_default();
    let pairwise = condition.contains("B.");
    let mut matched: BTreeSet<usize> = BTreeSet::new();
    for constraint in rule.constraints.iter().filter(|c| c.ctype != CType::Null) {
        let ctype = constraint.ctype;
        let spec = domain_spec(ctype);
        let collect = |d: Domain| -> Vec<usize> { (0..items.len()).filter(|&i| in_domain(&items[i].1, d)).collect() };
        let primary = collect(spec.primary);
        let secondary = spec.secondary.map(collect).unwrap_or_default();
        let cand = |i: usize| Candidate { item: &items[i].1, layers: &items[i].2 };
        if pairwise {
            if secondary.is_empty() {
                for (k, &i) in primary.iter().enumerate() {
                    for &j in &primary[k + 1..] {
                        if matches_pair(env, rule, &cand(i), &cand(j), ctype) {
                            matched.insert(i);
                            matched.insert(j);
                        }
                    }
                }
            } else {
                for &i in &primary {
                    for &j in &secondary {
                        if i != j && matches_pair(env, rule, &cand(i), &cand(j), ctype) {
                            matched.insert(i);
                            matched.insert(j);
                        }
                    }
                }
            }
        } else {
            matched.extend(primary.iter().copied().filter(|&i| matches_unary(env, rule, &cand(i), ctype)));
            if spec.secondary_unary {
                matched.extend(secondary.iter().copied().filter(|&i| matches_unary(env, rule, &cand(i), ctype)));
            }
        }
    }
    matched.into_iter().collect()
}

/// What Show Matches found: the number of items (KiCad's count, each track segment one) and the ids the studio selects them by, each once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matches {
    pub count: usize,
    pub ids: Vec<String>,
}

/// `DIALOG_DRC_RULE_EDITOR::highlightMatchingItems` for the panel's generated rule text: `Err(())` where KiCad returns -1 (the text does not parse, or the
/// rule's condition does not compile), no match for an empty text or a text with no rule, else what `GetItemsMatchingRule` found on the board.
pub fn show_matches(board: &RuleBoard, rule_text: &str) -> Result<Matches, ()> {
    let none = Matches { count: 0, ids: Vec::new() };
    if rule_text.is_empty() {
        return Ok(none);
    }
    let env = Rc::new(board.env_for_rules());
    // `parser.Parse( rules, nullptr )`: no reporter, the first error is thrown; the condition is compiled after (`m_Condition->Compile( nullptr )`).
    let parsed = dru::parse_rules(&format!("(version 1)\n{rule_text}"), &env).map_err(|_| ())?;
    let Some(rule) = parsed.rules.into_iter().next() else { return Ok(none) };
    let mut rule = rule;
    if let Some(cond) = rule.condition.as_mut() {
        if !cond.expression().is_empty() && !cond.compile(&env) {
            return Err(());
        }
    }
    let items = board.show_match_items();
    let hits = items_matching_rule(&env, &rule, &items);
    let mut ids: Vec<String> = Vec::new();
    for &i in &hits {
        if !ids.contains(&items[i].0) {
            ids.push(items[i].0.clone());
        }
    }
    Ok(Matches { count: hits.len(), ids })
}
