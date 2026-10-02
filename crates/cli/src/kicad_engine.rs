//! A board directory's face of kicad-cli (docs/ARCHITECTURE.md, "Engines"):
//! load the board, hand its current `design.json` revision to
//! `eda_kicad_engine`, return the report as the JSON the studio and the CLI
//! print. Derived files go to `.kicad/` beside `design.json` (scratch,
//! rewritten on every run); exports go to `export/kicad/<kind>/`.
//!
//! DRC, ERC and every output (plots, Gerbers, drill, position, STEP,
//! netlist, BOM, ...) are kicad-cli's. Our own checks, the ones KiCad does
//! not have, are `eda-lint`'s (see [`lint`]).

use crate::board;
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where this board's derived KiCad files go.
fn work(dir: &Path) -> PathBuf {
    dir.join(".kicad")
}

/// The board's design and model, with the schematic the engine would derive
/// from the intent when none is stored yet -- so ERC, plots and netlists
/// work on a fresh board, same as `GET /api/schematic.svg`.
pub fn load_with_schematic(dir: &Path) -> Result<(Design, ConstraintModel), Vec<CheckResult>> {
    let (_, mut design, model) = board::load(dir)?;
    if design.schematic.is_none() {
        design = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?;
    }
    Ok((design, model))
}

/// `kicad-cli pcb drc` on the current design: `{ engine, violations,
/// unconnected_items, counts }`.
pub fn drc(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    Ok(eda_kicad_engine::drc(&design, &model, &work(dir))?.to_json())
}

/// `kicad-cli sch erc` on the current schematic, in the studio's ERC shape
/// (`check`, `severity`, `location` = the first item's id, `hint`) plus the
/// full mapped `items` list. A finding the schematic's own exclusion list
/// accepts reports as `excluded`.
pub fn erc(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let exclusions: Vec<(String, String)> = design.schematic.as_ref().map(|s| s.erc_exclusions.iter().map(|e| (e.check.clone(), e.location.clone())).collect()).unwrap_or_default();
    Ok(eda_kicad_engine::erc(&design, &model, &work(dir))?.to_json(&exclusions))
}

/// `kicad-cli pcb export <kind> [args...]`: `{ ok, engine, files }`.
pub fn export(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    eda_kicad_engine::export_pcb(&design, &model, &work(dir), dir, kind, args)
}

/// `kicad-cli sch export <kind> [args...]` (`netlist`, `bom`, `pdf`, `svg`,
/// `dxf`, `ps`, `png`): `{ ok, engine, files }`.
pub fn export_sch(dir: &Path, kind: &str, args: &[String]) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    eda_kicad_engine::export_sch(&design, &model, &work(dir), dir, kind, args)
}

/// Our own checks, the ones KiCad does not have: `{ pcb: { violations,
/// counts }, schematic: { violations, counts } }`. `pcb` is shaped like a
/// DRC report (`type`, `description`, `severity`, `items`, `fix`);
/// `schematic` like an ERC one (`check`, `severity`, `location`, `hint`).
/// Cheap and in-process, so the studio runs it on every refresh.
pub fn lint(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (design, model) = load_with_schematic(dir)?;
    let pcb = eda_lint::check_pcb(&design, &model);
    let pcb_counts = eda_lint::counts(&pcb);
    let mut sch_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let sch_checks = eda_lint::check_schematic(&design, &model);
    let schematic: Vec<Value> = sch_checks
        .iter()
        .filter(|c| !matches!(c.status, eda_model::CheckStatus::Pass))
        .map(|c| {
            *sch_counts.entry(c.check.as_str()).or_default() += 1;
            json!({
                "check": c.check,
                "severity": match c.status { eda_model::CheckStatus::Fail => "error", _ => "warning" },
                "location": c.location,
                "hint": c.hint,
            })
        })
        .collect();
    Ok(json!({
        "pcb": { "violations": pcb, "counts": pcb_counts },
        "schematic": { "violations": schematic, "counts": sch_counts },
    }))
}
