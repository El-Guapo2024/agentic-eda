//! `eda-cypress` — NVlabs Cypress (ISPD'25, DREAMPlace-based analytical
//! placer) as a workspace generator behind the same `Placer` interface as
//! `eda-place`.
//!
//! Today this drives a native CPU build of Cypress as a subprocess through
//! the Bookshelf bridge (`eda_interchange::bookshelf`): our IR out, its
//! `.gp.pl` back in, our gates judge the result. The subprocess boundary is
//! deliberate — "wrap, don't swallow" — until the placer math (electrostatic
//! density via FFT, weighted-average wirelength, Nesterov) is ported into
//! Rust, at which point this crate keeps its API and drops the Python.
//!
//! Locate the build with `CYPRESS_INSTALL` (default `~/ws/Cypress/install`)
//! and `CYPRESS_PYTHON` (default `python`, i.e. whatever env is active).

use eda_interchange::{from_bookshelf_pl, to_bookshelf};
use eda_model::ir::Design;
use eda_model::{CheckResult, ConstraintModel};
use eda_place::Placer;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bookshelf unit used for the bridge, µm.
pub const UNIT_UM: i64 = 100;

#[derive(Debug, Clone)]
pub struct CypressOptions {
    pub install: PathBuf,
    pub python: String,
    /// Scratch directory for Bookshelf files, config and results.
    pub work_dir: PathBuf,
    pub gpu: bool,
    pub bins: u32,
    pub target_density: f64,
    pub iterations: u32,
}

impl Default for CypressOptions {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        CypressOptions {
            install: std::env::var("CYPRESS_INSTALL").map(PathBuf::from).unwrap_or_else(|_| Path::new(&home).join("ws/Cypress/install")),
            python: std::env::var("CYPRESS_PYTHON").unwrap_or_else(|_| "python".into()),
            work_dir: std::env::temp_dir().join("eda-cypress"),
            gpu: false,
            bins: 64,
            target_density: 0.6,
            iterations: 1000,
        }
    }
}

impl CypressOptions {
    pub fn available(&self) -> bool {
        self.install.join("dreamplace/Placer.py").exists()
    }
}

/// DREAMPlace/Cypress config JSON (keys mirror test/simple.json and the
/// PCB tuner's defaults).
pub fn config_json(aux: &Path, result_dir: &Path, seed: u64, o: &CypressOptions) -> serde_json::Value {
    serde_json::json!({
        "aux_input": aux,
        "gpu": if o.gpu { 1 } else { 0 },
        "num_bins_x": o.bins, "num_bins_y": o.bins,
        "global_place_stages": [{
            "num_bins_x": o.bins, "num_bins_y": o.bins, "iteration": o.iterations,
            "learning_rate": 0.00025, "wirelength": "weighted_average", "optimizer": "nesterov",
            "Llambda_density_weight_iteration": 1, "Lsub_iteration": 1
        }],
        "target_density": o.target_density, "density_weight": 0.008, "gamma": 0.1318231577,
        "random_seed": seed, "scale_factor": 1.0, "ignore_net_degree": 100,
        "enable_fillers": 1, "gp_noise_ratio": 0.025, "global_place_flag": 1,
        "legalize_flag": 1, "detailed_place_flag": 0, "stop_overflow": 0.07,
        "dtype": "float32", "plot_flag": 0, "result_dir": result_dir
    })
}

fn io_fail(what: &str, e: impl std::fmt::Display) -> Vec<CheckResult> {
    vec![CheckResult::fail("cypress_io", what, e.to_string())]
}

/// Run Cypress on `design` (its placement/outline seeds the problem) and
/// return a design with Cypress's placement. Precondition failures and
/// subprocess errors come back as `CheckResult`s like every generator.
pub fn place_with_cypress(design: &Design, model: &ConstraintModel, seed: u64, o: &CypressOptions) -> Result<Design, Vec<CheckResult>> {
    if !o.available() {
        return Err(vec![CheckResult::fail("cypress_unavailable", o.install.display().to_string(), "no dreamplace/Placer.py there; build Cypress or set CYPRESS_INSTALL")]);
    }
    let name = "board";
    let bs = to_bookshelf(design, model, name, UNIT_UM)?;
    let dir = o.work_dir.join(format!("{}-{seed}", std::process::id()));
    let bs_dir = dir.join("bookshelf");
    std::fs::create_dir_all(&bs_dir).map_err(|e| io_fail("work dir", e))?;
    for (fname, content) in bs.files(name) {
        std::fs::write(bs_dir.join(fname), content).map_err(|e| io_fail("bookshelf", e))?;
    }
    let results = dir.join("results");
    let cfg = config_json(&bs_dir.join(format!("{name}.aux")), &results, seed, o);
    let cfg_path = dir.join("config.json");
    std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).map_err(|e| io_fail("config", e))?;

    let out = Command::new(&o.python)
        .arg("dreamplace/Placer.py")
        .arg(&cfg_path)
        .current_dir(&o.install)
        .env("PYTHONPATH", &o.install)
        .output()
        .map_err(|e| io_fail("spawn python", e))?;
    let pl_path = results.join(name).join(format!("{name}.gp.pl"));
    if !out.status.success() || !pl_path.exists() {
        let log = String::from_utf8_lossy(&out.stderr);
        let tail: String = log.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(vec![CheckResult::fail("cypress_failed", cfg_path.display().to_string(), format!("exit {:?}; log tail: {tail}", out.status.code()))]);
    }
    let pl = std::fs::read_to_string(&pl_path).map_err(|e| io_fail("read .pl", e))?;
    let mut placed = from_bookshelf_pl(&pl, design, model, UNIT_UM)?;
    placed.provenance.seed = seed;
    placed.provenance.engine_version = format!("cypress@{}", o.install.display());
    Ok(placed)
}

/// `Placer` adapter so the loop can pick Cypress like any other generator.
pub struct Cypress(pub CypressOptions);

impl Placer for Cypress {
    fn name(&self) -> &str {
        "cypress"
    }
    fn place(&self, design: &Design, model: &ConstraintModel, seed: u64) -> Result<Design, Vec<CheckResult>> {
        place_with_cypress(design, model, seed, &self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_has_the_pcb_keys() {
        let o = CypressOptions::default();
        let c = config_json(Path::new("/x/b.aux"), Path::new("/r"), 7, &o);
        assert_eq!(c["gpu"], 0);
        assert_eq!(c["random_seed"], 7);
        assert_eq!(c["global_place_stages"][0]["wirelength"], "weighted_average");
    }

    #[test]
    fn unavailable_install_is_a_clean_failure() {
        let o = CypressOptions { install: PathBuf::from("/definitely/not/here"), ..Default::default() };
        let d = Design { schema: 1, provenance: eda_model::ir::Provenance { engine_version: "t".into(), intent_hash: "h".into(), seed: 0, stage_hashes: vec![] }, schematic: None, placement: None, routing: None };
        let err = place_with_cypress(&d, &ConstraintModel::default(), 0, &o).unwrap_err();
        assert_eq!(err[0].check, "cypress_unavailable");
    }
}
