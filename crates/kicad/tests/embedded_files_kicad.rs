//! Embedded files against the files KiCad wrote (GAPS.md items 24 and 30): the two boards of KiCad's QA corpus that embed drawing sheets (`issue22102.kicad_pcb`,
//! `connect/connect.kicad_pcb`) and the footprint that embeds a STEP model (`Espressif ESP32-WROOM-32.kicad_mod`). They are read, each file is decompressed and its checksum
//! checked, and the files are written back: the `(embedded_files ..)` block comes out character for character as KiCad printed it.
//!
//! The corpus is `qa/data` of the KiCad sources (`KICAD_SRC_DIR`, else the checkout the other tests use); the tests say so and pass when it is not there.

use eda_kicad::embedded::{base64_decode, checksum_of, compress, decode, sizes};
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use std::path::PathBuf;

fn qa(rel: &str) -> Option<PathBuf> {
    let root = std::env::var("KICAD_SRC_DIR").unwrap_or_else(|_| "/Users/juanantonioluera/ws/kicad-src-8303b2ad".into());
    let p = PathBuf::from(root).join("qa/data").join(rel);
    if p.is_file() {
        Some(p)
    } else {
        eprintln!("skipped: {} is not there (set KICAD_SRC_DIR to the KiCad sources)", p.display());
        None
    }
}

/// The text of the `(embedded_files ..)` block that is a child of the file's root: from its line to the line of its closing parenthesis.
fn block_of(text: &str) -> &str {
    let at = text.find("\n\t(embedded_files").expect("an embedded_files block") + 1;
    let end = text[at..].find("\n\t)\n").expect("the end of the block") + at + "\n\t)".len();
    &text[at..end]
}

fn board_files(rel: &str) -> Option<(String, eda_model::ir::Design, eda_model::ConstraintModel, eda_kicad::ImportNotes)> {
    let path = qa(rel)?;
    let text = std::fs::read_to_string(path).unwrap();
    let (design, model, notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("{e:?}"));
    Some((text, design, model, notes))
}

#[test]
fn the_drawing_sheets_of_kicads_boards_decode_and_match_their_checksums() {
    for (rel, names) in [("pcbnew/issue22102.kicad_pcb", vec!["SchematicTemplate.kicad_wks", "SchematicTemplateEmpty.kicad_wks"]), ("pcbnew/connect/connect.kicad_pcb", vec!["V2_A4_Empty.kicad_wks"])] {
        let Some((_, design, _, notes)) = board_files(rel) else { return };
        let list = &design.drawings.as_ref().expect("the board has drawings").embedded;
        assert_eq!(list.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), names, "{rel}");
        assert!(notes.embedded_problems.is_empty(), "{:?}", notes.embedded_problems);
        assert!(!notes.unread_items.contains_key("embedded_files"), "the block is read now: {:?}", notes.unread_items);
        for f in &list.files {
            assert_eq!(f.kind, eda_model::embedded_files::EmbeddedFileType::Worksheet);
            if f.name == "SchematicTemplate.kicad_wks" {
                // `kicad-cli pcb upgrade --force` on the same board (10.99.0) writes this checksum for the sheet: the MurmurHash3 with KiCad's seed.
                assert_eq!(f.checksum, "6003EC72C5978E83C182073E67278FCF");
                assert_eq!(decode(f).unwrap().checksum, "C484281A569A08003B5882311AF84B05");
            }
            let d = decode(f).unwrap_or_else(|e| panic!("{}: {e}", f.name));
            assert!(d.bytes.starts_with(b"(kicad_wks"), "{}: {:?}", f.name, String::from_utf8_lossy(&d.bytes[..20]));
            // These boards were saved by a KiCad 10 build whose MurmurHash3 had the bug in its last block (`addDataV1`): the checksum in the file is that one, and
            // `DecompressAndDecode` takes it and keeps the right one from then on -- which is what `kicad-cli pcb upgrade` writes (see the slow test).
            assert!(d.legacy_checksum, "{}: {}", f.name, f.checksum);
            assert_ne!(f.checksum, d.checksum);
            assert_eq!(d.checksum, checksum_of(&d.bytes));
            let sz = sizes(f);
            assert_eq!(sz.content, Some(d.bytes.len() as u64), "{}: the frame header tells the size", f.name);
            assert_eq!(sz.stored as usize, base64_decode(&f.data).unwrap().len());
            // The same content compressed as KiCad does, at level 15 (the zstd of this build and KiCad's may differ in what they pick, so only the content is the proof).
            let again = compress(&d.bytes).unwrap();
            assert_eq!(zstd_content(&again), d.bytes.len() as u64);
        }
    }
}

fn zstd_content(frame: &[u8]) -> u64 {
    zstd::zstd_safe::get_frame_content_size(frame).unwrap().unwrap()
}

#[test]
fn a_board_writes_its_embedded_files_back_as_kicad_printed_them() {
    for rel in ["pcbnew/issue22102.kicad_pcb", "pcbnew/connect/connect.kicad_pcb"] {
        let Some((original, design, model, _)) = board_files(rel) else { return };
        let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap_or_else(|e| panic!("{e:?}"));
        // character for character: the data, the checksums, the layout, down to the line breaks after 76 characters
        assert_eq!(block_of(&written), block_of(&original), "{rel}: the embedded_files block");
        // and read again it is the same list
        let (again, _, notes) = import_kicad_pcb(&written).unwrap();
        assert!(notes.embedded_problems.is_empty());
        assert_eq!(again.drawings.unwrap().embedded, design.drawings.unwrap().embedded, "{rel}");
    }
}

#[test]
fn an_unchanged_board_without_embedded_files_still_writes_none() {
    let Some((_, design, model, _)) = board_files("pcbnew/footprints_load_save.kicad_pcb") else { return };
    assert!(design.drawings.as_ref().is_none_or(|d| d.embedded.is_empty()));
    let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(!written.contains("embedded_files") && !written.contains("embedded_fonts"));
}

const ESP32: &str = "pcbnew/plugins/altium/pcblib/Espressif ESP32-WROOM-32.pretty/Espressif ESP32-WROOM-32.kicad_mod";

#[test]
fn a_footprint_that_embeds_a_step_model_is_read_and_written_as_kicad_has_it() {
    let Some(path) = qa(ESP32) else { return };
    let original = std::fs::read_to_string(path).unwrap();
    let parsed = eda_kicad::parse_library_footprint(&original).unwrap();
    assert_eq!(parsed.embedded.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Espressif - ESP32-WROOM-32.step"]);
    let f = &parsed.embedded.files[0];
    assert_eq!(f.kind, eda_model::embedded_files::EmbeddedFileType::Model);
    let d = decode(f).unwrap();
    assert!(d.bytes.starts_with(b"ISO-10303-21;"), "a STEP file: {:?}", String::from_utf8_lossy(&d.bytes[..16]));
    assert!(d.bytes.len() > 100_000, "{} bytes", d.bytes.len());
    assert_eq!(sizes(f).content, Some(d.bytes.len() as u64));
    // Compressed again at level 15 the content makes the very frame KiCad stored: `CompressAndEncode`'s `ZSTD_compress( .., 15 )` and the crate's one-shot call are the same
    // thing at the zstd release this lock pins (1.5.7). A different release may pick other bytes for the same content; the size would stay within a few percent.
    let again = compress(&d.bytes).unwrap();
    assert_eq!(eda_kicad::embedded::base64_encode(&again), f.data, "KiCad's frame is {} bytes, ours {}", base64_decode(&f.data).unwrap().len(), again.len());
    // The model line names it: `kicad-embed://..`
    assert!(parsed.footprint.models3d.iter().any(|m| m.path == f.link()), "{:?}", parsed.footprint.models3d.iter().map(|m| &m.path).collect::<Vec<_>>());
    assert!(parsed.warnings.is_empty() || parsed.warnings.iter().all(|w| !w.contains("embedded")), "{:?}", parsed.warnings);

    let written = eda_kicad::export_kicad_mod_with(&parsed.footprint, &parsed.embedded);
    assert_eq!(block_of(&written), block_of(&original), "the block as KiCad printed it");
    let again = eda_kicad::parse_library_footprint(&written).unwrap();
    assert_eq!(again.embedded, parsed.embedded);
    // The footprint with no files writes none.
    assert!(!eda_kicad::export_kicad_mod(&parsed.footprint).contains("embedded_files"));
}

fn sample_files() -> eda_model::embedded_files::EmbeddedFiles {
    let mut list = eda_model::embedded_files::EmbeddedFiles::default();
    let model: Vec<u8> = (0..3000u32).flat_map(|i| i.to_le_bytes()).collect();
    list.insert(eda_kicad::embedded::encode_new("part.step", &model).unwrap());
    list.insert(eda_kicad::embedded::encode_new("datasheet.pdf", b"%PDF-1.4 a datasheet").unwrap());
    list
}

#[test]
fn a_schematic_writes_the_files_it_embeds_in_its_own_file_and_reads_them_back() {
    let Some(path) = qa("pcbnew/custom_fields.kicad_sch") else { return };
    let text = std::fs::read_to_string(path).unwrap();
    let (mut design, model, _) = eda_kicad::import_kicad_sch(&text).unwrap();
    assert!(design.schematic.as_ref().unwrap().extras.embedded.is_empty());
    let plain = eda_kicad::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(!plain.contains("embedded_files") && plain.contains("(embedded_fonts no)"), "nothing embedded, as before");

    let list = sample_files();
    design.schematic.as_mut().unwrap().extras.embedded = list.clone();
    let written = eda_kicad::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(written.contains("\t(embedded_fonts no)\n\t(embedded_files\n\t\t(file\n\t\t\t(name \"datasheet.pdf\")\n\t\t\t(type datasheet)\n\t\t\t(data |"), "{}", &written[written.len().saturating_sub(900)..]);
    let (back, _, _) = eda_kicad::import_kicad_sch(&written).unwrap();
    assert_eq!(back.schematic.unwrap().extras.embedded, list);

    // fonts: the flag is the schematic's, and the fonts it holds are only written while it is on
    design.schematic.as_mut().unwrap().extras.embedded.fonts = true;
    design.schematic.as_mut().unwrap().extras.embedded.insert(eda_kicad::embedded::encode_new("Some Font.ttf", b"\0\x01\0\0 a font").unwrap());
    let with_fonts = eda_kicad::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(with_fonts.contains("(embedded_fonts yes)") && with_fonts.contains("Some Font.ttf"));
    design.schematic.as_mut().unwrap().extras.embedded.fonts = false;
    let without = eda_kicad::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
    assert!(without.contains("(embedded_fonts no)") && !without.contains("Some Font.ttf") && without.contains("part.step"), "ClearEmbeddedFonts before the save");
}

#[test]
fn a_symbol_library_writes_the_files_a_symbol_embeds_and_reads_them_back() {
    let text = r#"(kicad_symbol_lib (version 20231120) (generator "t")
        (symbol "R" (pin_numbers (hide yes)) (pin_names (offset 0) (hide no)) (in_bom yes) (on_board yes)
            (property "Reference" "R" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")))
        )
        (symbol "C" (pin_numbers (hide yes)) (pin_names (offset 0) (hide no)) (in_bom yes) (on_board yes)
            (property "Reference" "C" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "C_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")))
        )
    )"#;
    let parsed = eda_kicad::parse_library_symbols(text).unwrap();
    assert!(parsed.embedded.is_empty());
    let list = sample_files();
    let mut embedded = std::collections::BTreeMap::new();
    embedded.insert(parsed.symbols[1].lib_id.clone(), list.clone());
    let syms: Vec<&eda_model::ir::LibrarySymbol> = parsed.symbols.iter().collect();
    let written = eda_kicad::export_kicad_sym_library_with(&syms, &embedded);
    let again = eda_kicad::parse_library_symbols(&written).unwrap();
    assert_eq!(again.embedded.len(), 1, "only the symbol that has files");
    assert_eq!(again.embedded.get("C"), Some(&list));
    assert_eq!(again.symbols.len(), 2);
    assert_eq!(eda_kicad::export_kicad_sym_library(&syms).matches("embedded").count(), 0);
}

#[test]
fn a_footprint_on_a_board_lists_its_files_without_data_and_the_boards_list_has_the_data() {
    let Some((_, mut design, model, _)) = board_files("pcbnew/footprints_load_save.kicad_pcb") else { return };
    let id = design.placement.as_ref().unwrap().footprints[0].id.clone();
    let list = sample_files();
    // the footprint uses part.step (its own data) and names a file only the board holds
    let mut mine = eda_model::embedded_files::EmbeddedFiles::default();
    mine.insert(list.get("part.step").unwrap().clone());
    mine.insert(list.get("datasheet.pdf").unwrap().as_reference());
    let drawings = design.drawings.get_or_insert_with(Default::default);
    drawings.footprint_extras.iter_mut().find(|e| e.id == id).unwrap().embedded = mine;
    drawings.embedded.insert(list.get("datasheet.pdf").unwrap().clone());
    let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();

    // the board's block has both files with data, once each
    let board_block = block_of(&written);
    assert_eq!(board_block.matches("(data |").count(), 2, "{board_block}");
    // the footprint's own block names both and has no data
    let fp_at = written.find("\t\t(embedded_files").expect("the footprint's block");
    let fp_block = &written[fp_at..fp_at + written[fp_at..].find("\n\t\t)\n").unwrap()];
    assert!(fp_block.contains("(name \"part.step\")") && fp_block.contains("(name \"datasheet.pdf\")") && !fp_block.contains("(data"), "{fp_block}");

    let (back, _, notes) = import_kicad_pcb(&written).unwrap();
    assert!(notes.embedded_problems.is_empty());
    let dr = back.drawings.unwrap();
    assert_eq!(dr.embedded.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["datasheet.pdf", "part.step"], "the board's block is the one list");
    let listed = &dr.footprint_extras.iter().find(|e| e.id == id).unwrap().embedded;
    assert_eq!(listed.files.iter().map(|f| (f.name.as_str(), f.data.is_empty())).collect::<Vec<_>>(), [("datasheet.pdf", true), ("part.step", true)]);
    assert_eq!(listed.get("part.step").unwrap().checksum, list.get("part.step").unwrap().checksum);
}

/// Contents of every kind a person embeds: nothing, one byte, text, every byte value, a file whose frame is many lines long; under names with spaces, dots and non-ASCII letters.
fn odd_files() -> Vec<(String, Vec<u8>)> {
    let mut seed = 0x2545_F491_u32;
    let noise: Vec<u8> = (0..300_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 24) as u8
        })
        .collect();
    vec![
        ("empty.txt".into(), Vec::new()),
        ("one.bin".into(), vec![0]),
        ("Notes on the part.PDF".into(), b"%PDF-1.7\n\x00\xff text".to_vec()),
        ("all bytes.dat".into(), (0..=255u8).cycle().take(4096).collect()),
        ("Müller résumé.step".into(), noise),
        ("a.b.c.kicad_wks".into(), b"(kicad_wks (version 20231118))".to_vec()),
    ]
}

fn odd_list() -> eda_model::embedded_files::EmbeddedFiles {
    let mut list = eda_model::embedded_files::EmbeddedFiles::default();
    for (name, content) in odd_files() {
        assert!(list.insert(eda_kicad::embedded::encode_new(&name, &content).unwrap()));
    }
    list
}

/// Every file of `list` decodes to the bytes it was made of.
fn assert_contents(list: &eda_model::embedded_files::EmbeddedFiles, what: &str) {
    for (name, content) in odd_files() {
        let f = list.get(&name).unwrap_or_else(|| panic!("{what}: {name} is read back"));
        assert_eq!(decode(f).unwrap_or_else(|e| panic!("{what}: {name}: {e}")).bytes, content, "{what}: {name}, byte for byte");
    }
    assert_eq!(list.files.len(), odd_files().len(), "{what}");
}

#[test]
fn every_writer_and_reader_carries_a_file_byte_for_byte() {
    let list = odd_list();
    assert_contents(&list, "encoded");
    // the board
    if let Some((_, mut design, model, _)) = board_files("pcbnew/footprints_load_save.kicad_pcb") {
        design.drawings.get_or_insert_with(Default::default).embedded = list.clone();
        let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        let (back, _, notes) = import_kicad_pcb(&written).unwrap();
        assert!(notes.embedded_problems.is_empty(), "{:?}", notes.embedded_problems);
        let again = back.drawings.unwrap().embedded;
        assert_eq!(again, list, "the board's list, data and checksums as they were");
        assert_contents(&again, "board");
        // and once more: what was read is written the same
        let mut d2 = design.clone();
        d2.drawings.as_mut().unwrap().embedded = again;
        assert_eq!(block_of(&export_kicad_pcb(&d2, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap()), block_of(&written));
    }
    // the schematic
    if let Some(path) = qa("pcbnew/custom_fields.kicad_sch") {
        let (mut design, model, _) = eda_kicad::import_kicad_sch(&std::fs::read_to_string(path).unwrap()).unwrap();
        design.schematic.as_mut().unwrap().extras.embedded = list.clone();
        let written = eda_kicad::export_kicad_sch(&design, &model, &ExportMeta { date: "2026-01-01", title: "t" }).unwrap();
        let (back, _, _) = eda_kicad::import_kicad_sch(&written).unwrap();
        assert_contents(&back.schematic.unwrap().extras.embedded, "schematic");
    }
    // a library footprint
    if let Some(path) = qa(ESP32) {
        let parsed = eda_kicad::parse_library_footprint(&std::fs::read_to_string(path).unwrap()).unwrap();
        let written = eda_kicad::export_kicad_mod_with(&parsed.footprint, &list);
        let again = eda_kicad::parse_library_footprint(&written).unwrap();
        assert_eq!(again.embedded, list);
        assert_contents(&again.embedded, "footprint");
    }
    // a library symbol
    let text = r#"(kicad_symbol_lib (version 20231120) (generator "t")
        (symbol "R" (pin_numbers (hide yes)) (pin_names (offset 0) (hide no)) (in_bom yes) (on_board yes)
            (property "Reference" "R" (at 0 0 0) (effects (font (size 1.27 1.27))))
            (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1")))
        )
    )"#;
    let parsed = eda_kicad::parse_library_symbols(text).unwrap();
    let embedded: std::collections::BTreeMap<String, eda_model::embedded_files::EmbeddedFiles> = [("R".to_string(), list.clone())].into();
    let syms: Vec<&eda_model::ir::LibrarySymbol> = parsed.symbols.iter().collect();
    let again = eda_kicad::parse_library_symbols(&eda_kicad::export_kicad_sym_library_with(&syms, &embedded)).unwrap();
    assert_contents(again.embedded.get("R").unwrap(), "symbol");
}

// ------------------------------------------------------------------------------------------------------------------ the slow tier (kicad-cli)

fn kicad_cli() -> Option<PathBuf> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    for c in ["/usr/local/bin/kicad-cli", "/usr/bin/kicad-cli", "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"] {
        if std::path::Path::new(c).is_file() {
            return Some(c.into());
        }
    }
    let out = std::process::Command::new("which").arg("kicad-cli").output().ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        eprintln!("skipped: no kicad-cli");
        None
    } else {
        Some(p.into())
    }
}

/// `kicad-cli pcb export <format>` of `board` into `out`; kicad-cli's own report, which names a model it could not add.
fn export_3d(cli: &std::path::Path, format: &str, board: &std::path::Path, out: &std::path::Path) -> String {
    let o = std::process::Command::new(cli).args(["pcb", "export", format, "-f", "-o"]).arg(out).arg(board).output().expect("runs kicad-cli");
    let report = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success() && out.is_file(), "kicad-cli pcb export {format}: {report}");
    report
}

/// How many solids a STEP file holds.
fn solids(step: &std::path::Path) -> usize {
    let text = std::fs::read_to_string(step).unwrap();
    text.matches("MANIFOLD_SOLID_BREP(").count()
}

/// How many meshes a GLB holds: the `meshes` of the glTF JSON chunk (the 12-byte header, then the chunk's length and type, then the JSON).
fn glb_meshes(glb: &std::path::Path) -> usize {
    let b = std::fs::read(glb).unwrap();
    assert_eq!(&b[..4], b"glTF");
    let len = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
    let json: serde_json::Value = serde_json::from_slice(&b[20..20 + len]).unwrap();
    json["meshes"].as_array().map_or(0, |m| m.len())
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): a board whose footprint names its STEP model `kicad-embed://..`, the model kept in the board's own `(embedded_files ..)` as this
/// studio writes it, is exported by `kicad-cli pcb export step` and `glb` with the model in it -- as many solids and meshes as the same board with the model named by its path on
/// disk, and more than the board with no model at all (`EXPORTER_STEP::buildFootprint3DShapes`: `ResolvePath` with the footprint's and the board's embedded files).
#[test]
fn kicad_cli_exports_a_board_with_an_embedded_step_model_with_the_model_in_it() {
    let Some(cli) = kicad_cli() else { return };
    let Some(fp_path) = qa(ESP32) else { return };
    let Some((_, mut design, mut model, _)) = board_files("pcbnew/footprints_load_save.kicad_pcb") else { return };
    // the STEP model: the one KiCad's own QA footprint embeds
    let parsed = eda_kicad::parse_library_footprint(&std::fs::read_to_string(fp_path).unwrap()).unwrap();
    let step_file = parsed.embedded.files[0].clone();
    let step_bytes = decode(&step_file).unwrap().bytes;

    let work = std::env::temp_dir().join(format!("eda_embedded_export_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let on_disk = work.join("model.step");
    std::fs::write(&on_disk, &step_bytes).unwrap();

    let write = |design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, name: &str| -> PathBuf {
        let p = work.join(format!("{name}.kicad_pcb"));
        std::fs::write(&p, export_kicad_pcb(design, model, &ExportMeta { date: "2026-10-23", title: name }).unwrap()).unwrap();
        p
    };
    for fp in &mut model.footprints {
        fp.models3d.clear();
    }
    let bare = write(&design, &model, "bare");
    let first = model.footprints[0].name.clone();
    let users = model.parts.iter().filter(|p| model.footprint_of(p).is_some_and(|f| f.name == first)).count();
    assert!(users >= 1, "the footprint {first} is on the board");
    model.footprints[0].models3d = vec![eda_model::footprint::Model3d::identity(on_disk.to_str().unwrap())];
    let by_path = write(&design, &model, "by_path");
    model.footprints[0].models3d = vec![eda_model::footprint::Model3d::identity(&step_file.link())];
    design.drawings.get_or_insert_with(Default::default).embedded.insert(step_file.clone());
    let embedded = write(&design, &model, "embedded");
    // the board file holds the model, as KiCad writes it
    let text = std::fs::read_to_string(&embedded).unwrap();
    assert!(text.contains(&format!("(model \"{}\"", step_file.link())) && block_of(&text).contains(&step_file.checksum));

    let step_of = |b: &PathBuf| {
        let out = b.with_extension("step");
        let report = export_3d(&cli, "step", b, &out);
        (solids(&out), report)
    };
    let (none_solids, _) = step_of(&bare);
    let (path_solids, _) = step_of(&by_path);
    let (embedded_solids, report) = step_of(&embedded);
    assert!(path_solids > none_solids, "the model by its path adds solids: {none_solids} -> {path_solids}");
    assert_eq!(embedded_solids, path_solids, "the embedded model is exported as the one on disk is: {report}");

    let glb_of = |b: &PathBuf| {
        let out = b.with_extension("glb");
        export_3d(&cli, "glb", b, &out);
        glb_meshes(&out)
    };
    let (none, by_path_meshes, embedded_meshes) = (glb_of(&bare), glb_of(&by_path), glb_of(&embedded));
    assert!(by_path_meshes > none, "the model by its path adds meshes: {none} -> {by_path_meshes}");
    assert_eq!(embedded_meshes, by_path_meshes, "the GLB has the embedded model too");
    eprintln!("{users} footprint(s) with the model; STEP solids {none_solids} / {path_solids} / {embedded_solids}, GLB meshes {none} / {by_path_meshes} / {embedded_meshes} (none / by path / embedded)");
    let _ = std::fs::remove_dir_all(&work);
}
