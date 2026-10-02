//! Offline companion to `parity_erc.rs`: re-runs *our* `check_erc` on
//! KiCad's QA schematics and compares per-type counts with the kicad-cli
//! counts recorded in `docs/parity/raw/erc.json` (and with the "ours"
//! counts recorded there at the time, to see what changed since). For
//! environments with the KiCad source tree but no kicad-cli.
//!
//!   EDA_KICAD_QA_BOARDS=<kicad>/qa/data \
//!     cargo test -p eda-kicad --test parity_erc_cached -- --ignored --nocapture

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eda_kicad::{check_erc, import_kicad_sch_tree};
use eda_model::CheckStatus;

/// Checks that resolve symbols/footprints against KiCad's installed
/// libraries (`sym-lib-table`/`fp-lib-table`): without those libraries
/// every lookup fails, so these are environment-dependent and left out of
/// the comparison unless `PARITY_WITH_LIBS=1`.
const LIBRARY_DEPENDENT: &[&str] = &["lib_symbol_issues", "lib_symbol_mismatch", "footprint_link_issues"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

#[test]
#[ignore]
fn parity_erc_cached_counts() {
    let Some(qa) = std::env::var_os("EDA_KICAD_QA_BOARDS").map(PathBuf::from) else {
        println!("EDA_KICAD_QA_BOARDS not set; skipping");
        return;
    };
    let with_libs = std::env::var("PARITY_WITH_LIBS").as_deref() == Ok("1");
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo_root().join("docs/parity/raw/erc.json")).unwrap()).unwrap();
    // type -> (kicad, recorded ours, ours now)
    let mut totals: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut boards = Vec::new();
    for b in raw["boards"].as_array().unwrap() {
        if b["source"] != "qa" || !b["error"].is_null() {
            continue;
        }
        let name = b["board"].as_str().unwrap();
        let sch = [qa.join(name), qa.join("pcbnew").join(name), qa.join("eeschema").join(name)].into_iter().find(|p| p.exists());
        let Some(sch) = sch else {
            println!("{name}: not found");
            continue;
        };
        let now = match std::panic::catch_unwind(|| {
            let (design, model, _) = import_kicad_sch_tree(&sch).expect("import");
            check_erc(&design, &model)
        }) {
            Ok(r) => r,
            Err(_) => {
                println!("{name}: panicked");
                continue;
            }
        };
        let mut ours: BTreeMap<String, usize> = BTreeMap::new();
        for r in &now {
            if r.status == CheckStatus::Pass || r.check.starts_with("schematic_") || (!with_libs && LIBRARY_DEPENDENT.contains(&r.check.as_str())) {
                continue;
            }
            *ours.entry(r.check.clone()).or_insert(0) += 1;
        }
        let mut diff_now = 0;
        let mut diff_then = 0;
        let mut detail = Vec::new();
        let mut keys: Vec<String> = b["types"].as_object().unwrap().keys().cloned().collect();
        keys.extend(ours.keys().cloned());
        keys.sort();
        keys.dedup();
        for t in keys {
            if !with_libs && LIBRARY_DEPENDENT.contains(&t.as_str()) {
                continue;
            }
            let k = b["types"][&t]["kicad"].as_u64().unwrap_or(0) as usize;
            let then = b["types"][&t]["ours"].as_u64().unwrap_or(0) as usize;
            let n = ours.get(&t).copied().unwrap_or(0);
            let e = totals.entry(t.clone()).or_default();
            e.0 += k;
            e.1 += then;
            e.2 += n;
            diff_now += k.abs_diff(n);
            diff_then += k.abs_diff(then);
            if then != n {
                detail.push(format!("{t} kicad {k}: {then}->{n}"));
            }
        }
        boards.push((diff_then as i64 - diff_now as i64, name.to_string(), detail.join(", ")));
    }
    println!("\n{:<28} {:>7} {:>9} {:>7}", "type", "kicad", "recorded", "now");
    let (mut a, mut b, mut c, mut dthen, mut dnow) = (0, 0, 0, 0, 0);
    for (t, (k, then, now)) in &totals {
        println!("{t:<28} {k:>7} {then:>9} {now:>7}");
        a += k;
        b += then;
        c += now;
        dthen += k.abs_diff(*then);
        dnow += k.abs_diff(*now);
    }
    println!("{:<28} {a:>7} {b:>9} {c:>7}   |diff| recorded {dthen} -> now {dnow}", "TOTAL");
    boards.sort_by(|x, y| y.0.abs().cmp(&x.0.abs()));
    println!("\nboards whose counts changed (improvement in |diff|, details):");
    for (gain, n, d) in boards.iter().filter(|b| !b.2.is_empty()).take(25) {
        println!("{gain:>+5}  {n}: {d}");
    }
}
