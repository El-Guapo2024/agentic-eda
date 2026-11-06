//! The fonts a board's texts use, embedded on save as KiCad embeds them (UI-LEFT P26; `eda_kicad_engine::fonts`), held to kicad-cli itself.
//!
//! KiCad's QA board `issue14549.kicad_pcb` sets six board texts in "Calibri" bold, a face this machine lacks: `FONTCONFIG::FindFont` substitutes, and `BOARD::EmbedFonts` embeds
//! the file fontconfig put in its place. With `(embedded_fonts yes)`:
//!   1. the board as this studio writes it without the fonts, saved again by `kicad-cli pcb upgrade --force` (`PCB_IO_KICAD_SEXPR::FormatBoardToFormatter`: `EmbedFonts()`),
//!      comes back with the font files KiCad embeds -- the same names, types and checksums `fonts::with_board_fonts` embeds;
//!   2. the board as the studio derives it (`export_pcb`: the fonts embedded) is saved by kicad-cli with its block unchanged: KiCad adds nothing (`AddFile( .., false )`);
//!   3. with the box off, the studio writes no font, and kicad-cli's save clears the fonts a file held (`ClearEmbeddedFonts`).
//!
//! Slow tier (kicad-cli): `EDA_SLOW_TESTS=1`; the QA data from `KICAD_QA_DATA`.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_kicad_engine::find_cli;
use eda_kicad_engine::fonts::{board_font_uses, text_fonts, with_board_fonts};
use eda_model::embedded_files::{EmbeddedFileType, EmbeddedFiles};
use std::path::{Path, PathBuf};

fn qa_board(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|s| PathBuf::from(s).join("qa/data")))?;
    let p = root.join("pcbnew").join(name);
    p.is_file().then_some(p)
}

fn upgrade(cli: &Path, file: &Path) {
    let o = std::process::Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(file).output().expect("runs kicad-cli");
    assert!(o.status.success(), "kicad-cli pcb upgrade --force: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// The board list a `.kicad_pcb` holds, as (name, type, checksum) of each file.
fn listed(text: &str) -> Vec<(String, EmbeddedFileType, String)> {
    let (design, _, _) = import_kicad_pcb(text).expect("reads the board");
    design.drawings.map(|d| d.embedded).unwrap_or_default().files.into_iter().map(|f| (f.name, f.kind, f.checksum)).collect()
}

#[test]
fn kicad_cli_embeds_the_same_fonts_for_the_boards_texts() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let Some(path) = qa_board("issue14549.kicad_pcb") else {
        eprintln!("skipped: no KICAD_QA_DATA");
        return;
    };
    let (mut design, model, _) = import_kicad_pcb(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let uses = board_font_uses(&design);
    assert!(uses.iter().any(|u| u.face == "Calibri" && u.bold), "the board's texts are in Calibri bold: {uses:?}");
    let (fonts, skipped) = text_fonts(&uses);
    assert!(!fonts.is_empty(), "fontconfig matches a file for Calibri ({skipped:?})");
    design.drawings.get_or_insert_with(Default::default).embedded = EmbeddedFiles { fonts: true, files: Vec::new() };

    let work = std::env::temp_dir().join(format!("eda_embedded_fonts_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let meta = ExportMeta { date: "2026-11-06", title: "fonts" };

    // 1. KiCad embeds on save what `with_board_fonts` embeds
    let plain = export_kicad_pcb(&design, &model, &meta).unwrap();
    assert!(listed(&plain).is_empty(), "the writer adds no font of its own");
    let board = work.join("plain.kicad_pcb");
    std::fs::write(&board, &plain).unwrap();
    upgrade(&cli, &board);
    let theirs = listed(&std::fs::read_to_string(&board).unwrap());
    let with = with_board_fonts(&design).into_owned();
    let ours: Vec<(String, EmbeddedFileType, String)> = with.drawings.as_ref().unwrap().embedded.files.iter().map(|f| (f.name.clone(), f.kind, f.checksum.clone())).collect();
    assert!(!theirs.is_empty(), "kicad-cli embedded the fonts");
    eprintln!("kicad-cli embedded {:?}", theirs.iter().map(|(n, _, c)| format!("{n} {c}")).collect::<Vec<_>>());
    assert_eq!(ours, theirs, "the same font files, typed font, the same content");
    assert!(ours.iter().all(|(_, k, _)| *k == EmbeddedFileType::Font));

    // 2. the derived board (`export_pcb` writes it through the fonts step) keeps its block through KiCad's save
    let root = work.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let derived_dir = work.join(".kicad");
    eda_kicad_engine::export_pcb(&design, &model, &derived_dir, &root, "fonts", "svg", &["--layers".to_string(), "F.SilkS".to_string()]).expect("kicad-cli exports the derived board");
    let derived = std::fs::read_to_string(derived_dir.join("fonts.kicad_pcb")).unwrap();
    assert_eq!(listed(&derived), ours, "the derived board embeds the fonts");
    let resaved = work.join("derived.kicad_pcb");
    std::fs::write(&resaved, &derived).unwrap();
    upgrade(&cli, &resaved);
    assert_eq!(listed(&std::fs::read_to_string(&resaved).unwrap()), ours, "KiCad adds nothing and keeps the files");

    // 3. the box off: no font written, and kicad-cli clears a font the file held
    let mut off = with.clone();
    off.drawings.as_mut().unwrap().embedded.fonts = false;
    let text = export_kicad_pcb(&off, &model, &meta).unwrap();
    assert!(listed(&text).is_empty(), "`ClearEmbeddedFonts`: the writer drops them");
    let held = derived.replace("(embedded_fonts yes)", "(embedded_fonts no)");
    let cleared = work.join("cleared.kicad_pcb");
    std::fs::write(&cleared, &held).unwrap();
    upgrade(&cli, &cleared);
    assert!(listed(&std::fs::read_to_string(&cleared).unwrap()).is_empty(), "kicad-cli clears them too");
    let _ = std::fs::remove_dir_all(&work);
}
