//! Component classes: the project's rules that give footprints a class, and the classes a board file gives its footprints itself.
//!
//! A footprint's component class (`FOOTPRINT::GetComponentClass()`, what a DRC rule reads as `A.Component_Class` and `A.hasComponentClass('X')`) is made of
//!
//! - the classes its file lists (`(component_classes (class "A") (class "B"))` in the footprint: `FOOTPRINT::GetStaticComponentClass`, kept per footprint in
//!   `FootprintExtra::component_classes`), which the schematic's `Component Class` field puts there on Update PCB from Schematic, and
//! - the classes the project's assignment rules give it (`COMPONENT_CLASS_SETTINGS`, the project file's top-level `component_class_settings`: each rule a class name, an
//!   all/any operator and conditions on the footprint's reference, library link, side, rotation, a field, a custom expression or its sheet), and, when the project asks,
//!   one per sheet the footprint is on.
//!
//! This module is the project half: [`ComponentClassSettings`] as `BoardRules::component_classes`, read from and written to the `.kicad_pro` by `eda_kicad`
//! (`merge_project_setup_pages`, `export_kicad_pro`), so the derived project says to kicad-cli what the original did. The Board Setup page that edits it is not built.

use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}

/// What a condition of an assignment rule looks at (`COMPONENT_CLASS_ASSIGNMENT_DATA::CONDITION_TYPE`; the project file names them in capitals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    /// The footprint's reference, as a wildcard pattern (`C?`, `U5*`).
    Reference,
    /// The footprint's library link, as a wildcard pattern.
    Footprint,
    /// `Front` or `Back`.
    Side,
    /// The footprint's rotation in degrees.
    Rotation,
    /// A footprint field: the name is `primary`, the value pattern `secondary`.
    FootprintField,
    /// A DRC expression (`A.intersectsArea('RULE_1')`) in `primary`.
    Custom,
    /// The footprint's sheet, as a wildcard pattern.
    SheetName,
}

impl ConditionKind {
    /// The name the project file gives the kind (`GetConditionName`).
    pub fn file_name(self) -> &'static str {
        match self {
            ConditionKind::Reference => "REFERENCE",
            ConditionKind::Footprint => "FOOTPRINT",
            ConditionKind::Side => "SIDE",
            ConditionKind::Rotation => "ROTATION",
            ConditionKind::FootprintField => "FOOTPRINT_FIELD",
            ConditionKind::Custom => "CUSTOM",
            ConditionKind::SheetName => "SHEET_NAME",
        }
    }

    /// The kind a project file's name stands for (`GetConditionType`); a name KiCad does not know is its `REFERENCE` (the first kind).
    pub fn from_file_name(name: &str) -> ConditionKind {
        match name {
            "FOOTPRINT" => ConditionKind::Footprint,
            "SIDE" => ConditionKind::Side,
            "ROTATION" => ConditionKind::Rotation,
            "FOOTPRINT_FIELD" => ConditionKind::FootprintField,
            "CUSTOM" => ConditionKind::Custom,
            "SHEET_NAME" => ConditionKind::SheetName,
            _ => ConditionKind::Reference,
        }
    }
}

/// One condition of an assignment rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassCondition {
    pub kind: ConditionKind,
    /// What the condition matches (a pattern, a side, a number, an expression, or a field's name).
    #[serde(default)]
    pub primary: String,
    /// A field condition's value pattern; empty for the other kinds.
    #[serde(default)]
    pub secondary: String,
}

/// One assignment rule: the footprints that meet `conditions` get the class `class`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassAssignment {
    pub class: String,
    /// `"conditions_operator": "ALL"` (every condition must hold; the default) or `"ANY"`.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub all: bool,
    #[serde(default)]
    pub conditions: Vec<ClassCondition>,
}

fn d_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// The project's component class settings (`COMPONENT_CLASS_SETTINGS`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentClassSettings {
    /// `sheet_component_classes.enabled`: every footprint also gets a class for each sheet it is on.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sheet_classes: bool,
    /// The assignment rules, in the order the project lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<ClassAssignment>,
}

impl ComponentClassSettings {
    pub fn is_default(&self) -> bool {
        !self.sheet_classes && self.assignments.is_empty()
    }

    /// The settings the project's `component_class_settings` JSON object holds (`COMPONENT_CLASS_SETTINGS`' two parameters, `sheet_component_classes` and
    /// `assignments`; `loadAssignment` for each rule: a rule without its class or operator is not one, a condition's name is the part before the first `-`).
    pub fn from_project_json(json: &serde_json::Value) -> ComponentClassSettings {
        let mut out = ComponentClassSettings::default();
        if let Some(enabled) = json.get("sheet_component_classes").and_then(|s| s.get("enabled")).and_then(|v| v.as_bool()) {
            out.sheet_classes = enabled;
        }
        for rule in json.get("assignments").and_then(|a| a.as_array()).into_iter().flatten() {
            let (Some(class), Some(operator)) = (rule.get("component_class").and_then(|c| c.as_str()), rule.get("conditions_operator").and_then(|c| c.as_str())) else { continue };
            let mut conditions = Vec::new();
            if let Some(map) = rule.get("conditions").and_then(|c| c.as_object()) {
                for (name, data) in map {
                    let text = |key: &str| data.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    conditions.push(ClassCondition { kind: ConditionKind::from_file_name(name.split('-').next().unwrap_or("")), primary: text("primary"), secondary: text("secondary") });
                }
            }
            out.assignments.push(ClassAssignment { class: class.to_string(), all: operator == "ALL", conditions });
        }
        out
    }

    /// The project's `component_class_settings` JSON object (`COMPONENT_CLASS_SETTINGS::saveAssignment`): a condition's `primary` and `secondary` only when they say
    /// something, and a second condition of one kind under `NAME-1`, a third under `NAME-2`.
    pub fn to_project_json(&self) -> serde_json::Value {
        let assignments: Vec<serde_json::Value> = self
            .assignments
            .iter()
            .map(|a| {
                let mut conditions = serde_json::Map::new();
                for c in &a.conditions {
                    let mut entry = serde_json::Map::new();
                    if !c.primary.is_empty() {
                        entry.insert("primary".into(), c.primary.clone().into());
                    }
                    if !c.secondary.is_empty() {
                        entry.insert("secondary".into(), c.secondary.clone().into());
                    }
                    let base = c.kind.file_name();
                    let mut name = base.to_string();
                    let mut suffix = 1;
                    while conditions.contains_key(&name) {
                        name = format!("{base}-{suffix}");
                        suffix += 1;
                    }
                    conditions.insert(name, entry.into());
                }
                serde_json::json!({ "component_class": a.class, "conditions": conditions, "conditions_operator": if a.all { "ALL" } else { "ANY" } })
            })
            .collect();
        serde_json::json!({ "assignments": assignments, "meta": { "version": 0 }, "sheet_component_classes": { "enabled": self.sheet_classes } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The assignments of KiCad's own `component_classes.kicad_pro` (trimmed): read, written and read again, they are the same, and a condition of a kind that is there
    /// twice keeps both.
    #[test]
    fn the_projects_assignments_are_read_and_written_as_kicad_writes_them() {
        let json = serde_json::json!({
            "assignments": [
                { "component_class": "CAP_1", "conditions": { "FOOTPRINT": { "primary": "Capacitor_SMD:C_0805_2012Metric" }, "FOOTPRINT_FIELD": { "primary": "MY_FIELD", "secondary": "MY_VALUE" }, "REFERENCE": { "primary": "C1" }, "REFERENCE-1": { "primary": "C2" }, "SIDE": { "primary": "Front" } }, "conditions_operator": "ALL" },
                { "component_class": "ANY", "conditions": { "CUSTOM": { "primary": "A.intersectsArea('RULE_1')" }, "SHEET_NAME": {} }, "conditions_operator": "ANY" }
            ],
            "meta": { "version": 0 },
            "sheet_component_classes": { "enabled": true }
        });
        let settings = ComponentClassSettings::from_project_json(&json);
        assert!(settings.sheet_classes);
        assert_eq!(settings.assignments.len(), 2);
        let first = &settings.assignments[0];
        assert_eq!((first.class.as_str(), first.all, first.conditions.len()), ("CAP_1", true, 5));
        assert!(first.conditions.contains(&ClassCondition { kind: ConditionKind::FootprintField, primary: "MY_FIELD".into(), secondary: "MY_VALUE".into() }));
        assert_eq!(first.conditions.iter().filter(|c| c.kind == ConditionKind::Reference).count(), 2, "REFERENCE and REFERENCE-1 are two conditions");
        let second = &settings.assignments[1];
        assert!(!second.all);
        assert_eq!(second.conditions.iter().map(|c| c.kind).collect::<Vec<_>>(), vec![ConditionKind::Custom, ConditionKind::SheetName]);
        // Written, the object is KiCad's: capitals, an empty condition has no `primary`, the second reference is `REFERENCE-1`.
        let written = settings.to_project_json();
        assert_eq!(written["assignments"][0]["conditions_operator"], "ALL");
        assert_eq!(written["assignments"][0]["conditions"]["REFERENCE-1"], serde_json::json!({ "primary": "C2" }));
        assert_eq!(written["assignments"][1]["conditions"]["SHEET_NAME"], serde_json::json!({}));
        assert_eq!(written["sheet_component_classes"]["enabled"], true);
        assert_eq!(written["meta"]["version"], 0);
        assert_eq!(ComponentClassSettings::from_project_json(&written), settings);
        // Nothing set is the default, and a rule with no class or operator is not a rule.
        assert!(ComponentClassSettings::from_project_json(&serde_json::json!({ "assignments": [{ "conditions": {} }] })).is_default());
        assert!(ComponentClassSettings::default().is_default());
    }
}
