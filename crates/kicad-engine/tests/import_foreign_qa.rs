//! Other EDA formats in, against the real kicad-cli, on KiCad's own QA files (`qa/data/pcbnew/plugins/{altium,eagle,cadstar,pads,pcad,fabmaster,easyeda}`).
//!
//! The oracle is the `.kicad_pcb` kicad-cli wrote. For every file of the list:
//!   1. `kicad-cli pcb import` converts it (`import_foreign_pcb`, through `cli_command`), and says in its report what it made;
//!   2. our reader (`import_kicad_pcb`) reads the result;
//!   3. the footprints and the nets of what the reader made are those of the file -- counted here from the file's text by a scan of its own (parentheses and strings, nothing of
//!      the reader under test) -- and the footprints are also the report's `statistics.footprints`, kicad-cli's own count. Tracks and vias are compared the same way.
//! The schematic case is the QA files that have one: Eagle's `.sch` and the Altium sheets, converted by `import_foreign_sch` and read by `load_sheet_file`, whose symbols are the
//! placed symbols of the file.
//!
//! Slow tier (about three seconds a file, and the reader in a debug build): `EDA_SLOW_TESTS=1`. `EDA_IMPORT_FILES=a,b` (substrings of the paths below) runs fewer;
//! `KICAD_QA_DATA` moves the QA data.

use eda_kicad::{import_kicad_pcb, load_sheet_file};
use eda_kicad_engine::foreign::Severity;
use eda_kicad_engine::{find_cli, import_foreign_pcb, import_foreign_sch, with_scratch, ForeignOptions};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn qa_dir() -> PathBuf {
    std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"))
}

fn plugins_dir() -> PathBuf {
    qa_dir().join("pcbnew").join("plugins")
}

/// The boards of KiCad's QA data in the formats kicad-cli converts, as paths under `qa/data/pcbnew/plugins`. Left out, because kicad-cli does not turn them into a board:
/// `cadstar/lib/footprint-with-thermal-pad.cpa` (a footprint library), `easyedapro/ProProject_*` (a project of several boards, which KiCad asks the person to choose from; kicad-cli
/// stops with a `bad_function_call`), `2581/` (IPC-2581 has no importer in this build) and the `fakeboard.*` stubs (thirty bytes).
fn boards() -> Vec<String> {
    let mut list: Vec<String> = [
        "altium/HiFive/HiFive1.B01.PcbDoc",
        "altium/eDP_adapter_dvt1_source/eDP_adapter_dvt1.PcbDoc",
        "eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.brd",
        "eagle/test_eagle_23016/test_eagle.brd",
        "cadstar/route_offset/minimal_route_offset_curved_track.cpa",
        "pcad/pcad_4layer_glyph_test_ascii.PCB",
        "fabmaster/cds2f_only_2_comp.txt",
        "fabmaster/cds2f_14066-20316FBRD.txt",
        "easyeda/PCB_PCB_ESP32-PICO-D4 smart watch_2023-09-02.json",
        "easyeda/PCB_USBMETER-PD QC修改版_2023-09-02.json",
        "easyeda/BackupProjects_ESP32-PICO-D4_smart_watch.zip",
        "easyedapro/Scanning Tunneling Microscope OpenSTM ControlBoard.zip",
    ]
    .map(String::from)
    .to_vec();
    // PADS: every ASCII export and every binary board of the folder.
    let mut pads = Vec::new();
    walk(&plugins_dir().join("pads"), &mut pads);
    pads.retain(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("asc" | "pcb")));
    pads.sort();
    list.extend(pads.iter().map(|p| p.strip_prefix(plugins_dir()).unwrap().to_string_lossy().into_owned()));
    if let Ok(only) = std::env::var("EDA_IMPORT_FILES") {
        let wanted: Vec<&str> = only.split(',').filter(|s| !s.is_empty()).collect();
        list.retain(|f| wanted.iter().any(|w| f.contains(w)));
    }
    list
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// One `(tag ...)` of a KiCad file, found by a scan of its text: how deep it is (the root form is 1), its tag, and its first quoted argument.
struct Form {
    depth: usize,
    tag: String,
    name: Option<String>,
}

/// Every form of `text`, by a scan that knows parentheses and double-quoted strings (with `\"`) and nothing else.
fn scan(text: &str) -> Vec<Form> {
    let b = text.as_bytes();
    let mut forms = Vec::new();
    let (mut i, mut depth) = (0, 0usize);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                let start = i + 1;
                let mut j = start;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'(' && b[j] != b')' {
                    j += 1;
                }
                let tag = text[start..j].to_string();
                // The first quoted string before the next parenthesis: `(net "GND")`, `(net 3 "GND")`, `(footprint "Lib:Name" ...)`.
                let mut k = j;
                let mut name = None;
                while k < b.len() && b[k] != b'(' && b[k] != b')' {
                    if b[k] == b'"' {
                        let s = k + 1;
                        let mut e = s;
                        while e < b.len() && b[e] != b'"' {
                            e += if b[e] == b'\\' { 2 } else { 1 };
                        }
                        name = Some(text[s..e.min(b.len())].to_string());
                        break;
                    }
                    k += 1;
                }
                forms.push(Form { depth, tag, name });
                i = j;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ => i += 1,
        }
    }
    forms
}

/// What a `.kicad_pcb` holds, counted from its text: footprints, distinct non-empty net names, tracks (segments and arcs), vias and zones, all children of the root form.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Counts {
    footprints: usize,
    nets: usize,
    tracks: usize,
    vias: usize,
}

fn counts_of(text: &str) -> Counts {
    let forms = scan(text);
    let at_root = |tag: &str| forms.iter().filter(|f| f.depth == 2 && f.tag == tag).count();
    // KiCad 10 names a net where it is used (`(net "GND")`, on pads, tracks, vias and zones); before that a table `(net 1 "GND")` did, and items said the number.
    let nets: BTreeSet<&str> = forms.iter().filter(|f| f.tag == "net" && f.depth >= 2).filter_map(|f| f.name.as_deref()).filter(|n| !n.is_empty()).collect();
    Counts { footprints: at_root("footprint"), nets: nets.len(), tracks: at_root("segment") + at_root("arc"), vias: at_root("via") }
}

#[test]
fn the_scan_counts_what_a_board_file_holds() {
    let text = r#"(kicad_pcb (version 20260901)
  (footprint "L:A" (at 1 2) (pad "1" smd rect (net "GND")) (pad "2" smd rect (net "A(1)")) (fp_text user "(net \"X\")"))
  (footprint "L:B" (pad "1" smd rect (net "GND")))
  (segment (start 0 0) (end 1 1) (net "VCC")) (arc (start 0 0) (mid 1 1) (end 2 0) (net "VCC"))
  (via (at 0 0) (net "")) (zone (net "GND") (polygon (pts))))"#;
    assert_eq!(counts_of(text), Counts { footprints: 2, nets: 3, tracks: 2, vias: 1 }, "GND, VCC and A(1): a net in a text or an empty name is none");
    let legacy = "(kicad_pcb (net 0 \"\") (net 1 \"GND\") (footprint \"L:A\" (pad \"1\" smd rect (net 1 \"GND\"))) (segment (net 1)))";
    assert_eq!(counts_of(legacy), Counts { footprints: 1, nets: 1, tracks: 1, vias: 0 });
}

/// Whether the slow tier is on and there is a kicad-cli to run it with.
fn slow_and_ready() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return false;
    }
    if find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return false;
    }
    true
}

#[test]
fn kicad_cli_converts_every_qa_board_of_another_format_and_our_reader_reads_what_it_made() {
    if !slow_and_ready() {
        return;
    }
    let root = plugins_dir();
    let mut table = String::from("| file | format | footprints | nets | tracks | vias | findings |\n|---|---|---:|---:|---:|---:|---:|\n");
    let mut bad: Vec<String> = Vec::new();
    let files = boards();
    assert!(!files.is_empty());
    for rel in &files {
        let input = root.join(rel);
        assert!(input.is_file(), "{rel}: no such QA file ({})", input.display());
        with_scratch(|work| {
            let started = std::time::Instant::now();
            let got = match import_foreign_pcb(work, &input, &ForeignOptions::default()) {
                Ok(g) => g,
                Err(e) => {
                    bad.push(format!("{rel}: kicad-cli could not convert it: {e:?}"));
                    return;
                }
            };
            let took = started.elapsed().as_secs_f32();
            // The oracle: the file kicad-cli made, counted from its text.
            let oracle = counts_of(&got.text);
            // Our reader on it.
            let (design, model, _notes) = match import_kicad_pcb(&got.text) {
                Ok(r) => r,
                Err(e) => {
                    bad.push(format!("{rel}: our reader could not read the converted board: {e:?}"));
                    return;
                }
            };
            let ours = Counts {
                footprints: design.placement.as_ref().map_or(0, |p| p.footprints.len()),
                nets: model.nets.len(),
                tracks: design.routing.as_ref().map_or(0, |r| r.tracks.len()),
                vias: design.routing.as_ref().map_or(0, |r| r.vias.len()),
            };
            let said = |k: &str| got.report.statistics.get(k).copied().unwrap_or(-1);
            let line = format!(
                "| {rel} | {} | {} / {} / {} | {} / {} | {} / {} / {} | {} / {} / {} | {} ({took:.1} s) |\n",
                got.report.source_format, ours.footprints, oracle.footprints, said("footprints"), ours.nets, oracle.nets, ours.tracks, oracle.tracks, said("tracks"), ours.vias, oracle.vias, said("vias"), got.findings().len()
            );
            table.push_str(&line);
            if ours.footprints != oracle.footprints {
                bad.push(format!("{rel}: footprints: our reader made {}, the converted file has {}", ours.footprints, oracle.footprints));
            }
            if said("footprints") != oracle.footprints as i64 {
                bad.push(format!("{rel}: kicad-cli's report says {} footprints, its file has {}", said("footprints"), oracle.footprints));
            }
            if ours.nets != oracle.nets {
                bad.push(format!("{rel}: nets: our reader made {}, the converted file names {}", ours.nets, oracle.nets));
            }
            if ours.tracks != oracle.tracks || ours.vias != oracle.vias {
                bad.push(format!("{rel}: copper: our reader made {} tracks and {} vias, the converted file has {} and {}", ours.tracks, ours.vias, oracle.tracks, oracle.vias));
            }
            // A board with nothing in it would pass every comparison above: the report must have counted something, unless the file is one of the empty QA cases.
            assert!(!got.report.source_format.is_empty(), "{rel}: no format in the report");
        });
    }
    eprintln!("{table}");
    assert!(bad.is_empty(), "\n{}\n\n{table}", bad.join("\n"));
}

#[test]
fn a_layer_map_moves_a_layer_and_the_report_says_so() {
    if !slow_and_ready() {
        return;
    }
    let input = plugins_dir().join("eagle/test_eagle_23016/test_eagle.brd");
    with_scratch(|work| {
        let first = import_foreign_pcb(work, &input, &ForeignOptions::default()).expect("automatic mapping");
        let tnames = first.report.layers.iter().find(|l| l.source == "tNames").expect("Eagle's tNames layer").clone();
        assert_eq!((tnames.kicad.as_deref(), tnames.explicit), (Some("F.SilkS"), false), "the importer's guess");
        // The person puts it on F.Fab instead; a second run in a folder of its own.
        let mut opts = ForeignOptions::default();
        opts.layer_map.insert("tNames".into(), "F.Fab".into());
        opts.layer_map.insert("nonexistent".into(), "F.Cu".into());
        let second = import_foreign_pcb(&work.join("again"), &input, &opts).expect("explicit mapping");
        let moved = second.report.layers.iter().find(|l| l.source == "tNames").unwrap();
        assert_eq!((moved.kicad.as_deref(), moved.explicit), (Some("F.Fab"), true));
        let untouched = second.report.layers.iter().find(|l| l.source == "Top").unwrap();
        assert_eq!((untouched.kicad.as_deref(), untouched.explicit), (Some("F.Cu"), false), "a layer the map does not name keeps the guess");
        let findings = second.findings();
        assert!(findings.iter().any(|f| f.severity == Severity::Warning && f.message.contains("'nonexistent'") && f.message.contains("does not match any source layer")), "{findings:?}");
        assert_ne!(first.text, second.text, "the board itself differs: the reference texts moved layers");
    });
}

/// The placed symbols of a `.kicad_sch`: the `(symbol (lib_id ..))` forms of the screen (children of the root); the definitions inside `(lib_symbols ..)` are one level down.
fn placed_symbols(text: &str) -> usize {
    scan(text).iter().filter(|f| f.depth == 2 && f.tag == "symbol").count()
}

#[test]
fn kicad_cli_converts_the_qa_schematics_and_our_reader_reads_the_symbols_it_placed() {
    if !slow_and_ready() {
        return;
    }
    let root = qa_dir();
    // The schematics of the QA data in the formats kicad-cli converts: Eagle's two, and the two sheets of the Altium project (an Altium project file itself is not a schematic to
    // kicad-cli; `PrjPcb` is how a project lists its sheets). The gEDA ones of `eeschema/io/geda` have no importer in kicad-cli's `sch import` (its --format names no gEDA).
    let mut files = vec![
        "pcbnew/plugins/eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.sch",
        "eeschema/io/eagle/eagle-import-testfile.sch",
        "pcbnew/plugins/altium/eDP_adapter_dvt1_source/power.SchDoc",
        "pcbnew/plugins/altium/eDP_adapter_dvt1_source/it6251_core.SchDoc",
    ];
    if let Ok(only) = std::env::var("EDA_IMPORT_FILES") {
        let wanted: Vec<&str> = only.split(',').filter(|s| !s.is_empty()).collect();
        files.retain(|f| wanted.iter().any(|w| f.contains(w)));
    }
    let mut bad = Vec::new();
    for rel in files {
        with_scratch(|work| {
            let got = match import_foreign_sch(work, &root.join(rel), &ForeignOptions::default()) {
                Ok(g) => g,
                Err(e) => {
                    bad.push(format!("{rel}: kicad-cli could not convert it: {e:?}"));
                    return;
                }
            };
            let placed = placed_symbols(&got.text);
            let file = match load_sheet_file(&got.output) {
                Ok(f) => f,
                Err(e) => {
                    bad.push(format!("{rel}: our reader could not read the converted schematic: {e:?}"));
                    return;
                }
            };
            let read: usize = file.all_screens().map(|(_, s)| s.symbols.len() + s.power_symbols.len()).sum();
            // The reader drops a symbol whose library symbol the file lacks, and says so in the file's notes.
            let dropped: usize = file.notes.iter().filter_map(|n| n.split_whitespace().next()?.parse::<usize>().ok()).sum();
            eprintln!("{rel}: {} placed, {read} read, {dropped} left out, report: {:?}", placed, got.report.statistics);
            if read + dropped != placed {
                bad.push(format!("{rel}: the converted file places {placed} symbols; our reader made {read} and left out {dropped}: {:?}", file.notes));
            }
            if got.report.statistics.get("symbols").copied() != Some(placed as i64) {
                bad.push(format!("{rel}: kicad-cli's report says {:?} symbols, its file places {placed}", got.report.statistics.get("symbols")));
            }
        });
    }
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}
