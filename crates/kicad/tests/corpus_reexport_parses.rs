//! Every board of KiCad's QA corpus, imported and exported, gives a file that parses -- and that imports again as a board with the same footprints, tracks, vias and zones.
//!
//! The re-export keeps tokens it does not read as the text they were (a via's `(padstack ..)`, a pad's `(teardrops ..)`, a footprint's `(descr ..)`, the board's text boxes and
//! reference images), and writing a token back means quoting it as the lexer will take it again: this test is the net under that. It needs no kicad-cli (the DRC of the
//! re-exports is `parity_roundtrip.rs`'s) and no network; it needs KiCad's QA boards (`KICAD_QA_DATA`, or `KICAD_SRC_DIR`/qa/data, or the copy beside the sources this port
//! was read from) and runs the whole corpus, so it is a slow test: `EDA_SLOW_TESTS=1 cargo test -p eda-kicad --test corpus_reexport_parses -- --nocapture`.
//!
//! `EDA_CORPUS_REEXPORT_OUT=<dir>` also keeps the exports there, one file per board (its path in the corpus with `/` turned into `__`), for kicad-cli to load: the real parser
//! is the judge of a token kept as text, and `kicad-cli pcb upgrade --force` on each is a one-line loop.

use std::path::{Path, PathBuf};

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};

fn qa_root() -> Option<PathBuf> {
    let from_env = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")));
    from_env.into_iter().chain([PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data")]).find(|p| p.exists())
}

fn boards(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            boards(&p, out);
        } else if p.extension().is_some_and(|x| x == "kicad_pcb") {
            out.push(p);
        }
    }
}

#[test]
fn every_board_of_the_corpus_exports_a_file_that_imports_again() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(root) = qa_root() else {
        eprintln!("skipped: KiCad's QA boards are not here (KICAD_QA_DATA)");
        return;
    };
    let mut files = Vec::new();
    boards(&root, &mut files);
    files.sort();
    let keep = std::env::var_os("EDA_CORPUS_REEXPORT_OUT").map(PathBuf::from).inspect(|dir| std::fs::create_dir_all(dir).expect("directory for the exports"));
    let mut failures = Vec::new();
    let (mut checked, mut skipped) = (0, 0);
    for file in &files {
        let name = file.strip_prefix(&root).unwrap_or(file).display().to_string();
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        // A board this importer refuses (a file that is not a board, a footprint without a place) is the corpus-wide import measure's business, not this test's.
        let (design, model) = match import_kicad_pcb(&text) {
            Ok((design, model, _)) => (design, model),
            Err(e) => {
                eprintln!("skipped {name}: not imported: {}", e.iter().map(|c| format!("{} {}", c.check, c.hint.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("; "));
                skipped += 1;
                continue;
            }
        };
        let out = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }))) {
            Ok(Ok(out)) => out,
            // The export refuses what it cannot make a board of (a pour on a net that is not in the netlist): the same as before this test existed.
            Ok(Err(e)) => {
                eprintln!("skipped {name}: not exported: {}", e.iter().map(|c| format!("{} {}", c.check, c.hint.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("; "));
                skipped += 1;
                continue;
            }
            Err(_) => {
                failures.push(format!("{name}: the export panicked"));
                continue;
            }
        };
        checked += 1;
        if let Some(dir) = &keep {
            std::fs::write(dir.join(name.replace('/', "__")), &out).expect("write an export");
        }
        match import_kicad_pcb(&out) {
            Err(e) => failures.push(format!("{name}: the export does not import again: {e:?}")),
            Ok((back, _, _)) => {
                let counts = |d: &eda_model::ir::Design| {
                    (
                        d.placement.as_ref().map_or(0, |p| p.footprints.len()),
                        d.routing.as_ref().map_or((0, 0, 0), |r| (r.tracks.len(), r.vias.len(), r.zones.len())),
                        d.drawings.as_ref().map_or((0, 0, 0), |x| (x.texts.len(), x.barcodes.len(), x.raw_items.len())),
                    )
                };
                if counts(&design) != counts(&back) {
                    failures.push(format!("{name}: {:?} before, {:?} after", counts(&design), counts(&back)));
                }
            }
        }
    }
    eprintln!("{checked} boards exported and imported again, {skipped} skipped, {} failed", failures.len());
    assert!(checked > 100, "the corpus is here and few boards were checked: {checked}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
