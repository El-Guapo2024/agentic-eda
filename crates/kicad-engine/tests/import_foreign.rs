//! `eda_kicad_engine::import_foreign_pcb` / `import_foreign_sch` (`kicad-cli pcb import`, `kicad-cli sch import`).
//!
//! The first half runs against a stand-in kicad-cli (a shell script) and always runs: how the command is built, where its files go, how its report and its printed lines
//! become findings, how a failure reads. The second half is the slow tier (`EDA_SLOW_TESTS=1`): the real kicad-cli on KiCad's QA files of other formats
//! (`qa/data/pcbnew/plugins/{altium,eagle,cadstar,pads,pcad,fabmaster,easyeda}`), each converted and read back by our reader, whose footprint and net counts must be those of
//! the `.kicad_pcb` kicad-cli wrote -- counted here from the file's text by a scan of its own, not by the reader under test.

use eda_kicad_engine::foreign::{self, parse_log, upload_name, Severity};
use eda_kicad_engine::{import_foreign_pcb, import_foreign_sch, ForeignOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The environment is the process's: one test sets it at a time.
static ENV: Mutex<()> = Mutex::new(());

struct Env {
    keys: Vec<&'static str>,
    _only_one: std::sync::MutexGuard<'static, ()>,
}

impl Env {
    fn take() -> Env {
        Env { keys: vec![], _only_one: ENV.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }
    }
    fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(key, value);
        self.keys.push(key);
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for key in &self.keys {
            std::env::remove_var(key);
        }
    }
}

/// A stand-in kicad-cli: answers `version`, otherwise runs `body`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = version ]; then echo 10.99.0-fake; exit 0; fi\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The first run of a new executable can take a second on macOS (the system scans it first).
    let _ = std::process::Command::new(&path).arg("version").output();
    path
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda_import_foreign_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What a kicad-cli `pcb import` / `sch import` does, as far as this module can tell: the options are read off the command line, the output is written where `-o` says with the
/// library kicad-cli makes beside it, the report where `--report-file` says, and the importers' lines go to stderr. `$FAKE_LOG` gets the command line and the settings folder.
const IMPORTING_CLI: &str = r#"scope=$1; out=""; report=""; map=""; fmt=""; prev=""; in=""
for a in "$@"; do
  case "$prev" in -o) out="$a";; --report-file) report="$a";; --layer-map) map="$a";; --format) fmt="$a";; esac
  prev="$a"; in="$a"
done
echo "$* | config=$KICAD_CONFIG_HOME" >> "$FAKE_LOG"
[ -n "$map" ] && cp "$map" "$FAKE_LOG.map"
if [ "$scope" = import ]; then
  # the project-level command: `import -o <stem> [--layer-map F] FILE` writes <stem>.kicad_pro (and the board beside it); it prints no JSON report
  if [ -n "$FAKE_NO_PROJECT" ]; then echo "Failed to import: the project could not be made"; exit 3; fi
  printf '{ "net_settings": { "classes": [ { "name": "Default", "track_width": 0.25 }, { "name": "highpower", "track_width": 0.3048 } ] } }\n' > "$out.kicad_pro"
  exit 0
fi
case "$in" in *unknown*) echo "No PCB importer recognizes the file format of '$in'"; exit 3;; *broken*) echo "Importing '$in' using Eagle format..."; echo "12:50:13 AM: Error: XML parsing error: 'syntax error' at line 1" >&2; echo "Failed to load board: Unable to read file '$in'"; exit 3;; esac
echo "Importing '$in' using Eagle format..."
if [ "$scope" = pcb ]; then
  printf '(kicad_pcb (version 20260901) (generator "pcbnew"))\n' > "$out"
  lib="$(dirname "$out")/$(basename "$out" .kicad_pcb)-import-fps.pretty"
  mkdir -p "$lib"; echo '(footprint "A")' > "$lib/A.kicad_mod"
  echo '(fp_lib_table)' > "$(dirname "$out")/fp-lib-table"
  mapped="auto"; [ -n "$map" ] && mapped="explicit"
  cat > "$report" <<JSON
{ "errors": ["Could not find image file 'C:\\\\x.bmp'"], "output_file": "$(basename "$out")", "source_file": "$(basename "$in")", "source_format": "${FAKE_FORMAT:-Eagle}",
  "statistics": { "footprints": 2, "tracks": 3, "vias": 1, "zones": 0 }, "warnings": ["KiCad design rules are different from Eagle ones."],
  "layer_mapping": { "Top": { "kicad_layer": "F.Cu", "method": "auto" }, "tNames": { "kicad_layer": "F.Fab", "method": "$mapped" }, "200bmp": { "kicad_layer": "UNDEFINED", "method": "auto" } } }
JSON
else
  printf '(kicad_sch (version 20260830) (generator "eeschema"))\n' > "$out"
  echo '(sym_lib_table)' > "$(dirname "$out")/sym-lib-table"
  cat > "$report" <<JSON
{ "errors": [], "output_file": "$(basename "$out")", "source_file": "$(basename "$in")", "source_format": "Eagle", "statistics": { "sheets": 1, "symbols": 36, "renamed_board_nets": 0 }, "warnings": [] }
JSON
fi
echo "12:33:13 AM: Error: No library symbol found for schematic symbol 'R1 10k'." >&2
echo "12:33:13 AM: Error: No library symbol found for schematic symbol 'R1 10k'." >&2
echo "12:33:14 AM: Warning: Font 'CADSTAR' not found; substituting 'KiCad Font'." >&2
echo "12:33:14 AM: Setting schematic symbol 'D1 GREEN' library identifier to 'eagle-eagle-import:LED'." >&2
echo "Successfully saved imported board to '$out'""#;

#[test]
fn a_board_import_runs_kicad_cli_in_the_callers_folder_with_none_of_the_users_settings_and_hands_back_the_file_the_report_and_the_findings() {
    let mut env = Env::take();
    let dir = scratch("board");
    let log = dir.join("calls.log");
    env.set("FAKE_LOG", &log);
    env.set("EDA_KICAD_CLI", script(&dir, "importing.sh", IMPORTING_CLI));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let input = foreign::stage_upload(&dir.join("work"), "../../My Board.brd", b"<eagle/>").unwrap();
    assert_eq!(input.file_name().unwrap(), "My Board.brd", "a name with folders in it is the file name alone");
    assert!(input.starts_with(dir.join("work")), "the upload stays in the run's folder");

    let got = import_foreign_pcb(&dir.join("work"), &input, &ForeignOptions::default()).expect("an import");
    assert_eq!(got.engine, "kicad-cli 10.99.0-fake");
    assert_eq!(got.text, "(kicad_pcb (version 20260901) (generator \"pcbnew\"))\n");
    assert_eq!(got.output.file_name().unwrap(), "My_Board.kicad_pcb", "the output is named for the input, with the characters of a file name every tool takes");
    assert!(got.output.starts_with(dir.join("work").join("pcb-out")), "{:?}", got.output);

    // The command line: the group and the verb, the report asked for as JSON, the output and the input; no --format for auto, no --layer-map without a map.
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 1, "one launch: {calls}");
    assert!(calls.starts_with("pcb import --report-format json --report-file "), "{calls}");
    assert!(calls.contains(" -o ") && calls.contains("My_Board.kicad_pcb") && calls.contains("My Board.brd"), "{calls}");
    assert!(!calls.contains("--format") && !calls.contains("--layer-map"), "{calls}");
    if std::env::var_os("EDA_KICAD_USER_CONFIG").is_none() {
        let home = dir.join("settings").join("empty");
        assert!(calls.contains(&format!("config={}", home.display())), "through cli_command, so none of the user's KiCad settings: {calls}");
    }

    // What kicad-cli wrote beside the board is listed, relative to the folder.
    assert_eq!(got.files, vec!["My_Board-import-fps.pretty/A.kicad_mod".to_string(), "fp-lib-table".to_string()]);

    // The report: the format kicad-cli recognised, the counts, where the layers went.
    assert_eq!(got.report.source_format, "Eagle");
    assert_eq!(got.report.statistics.get("footprints"), Some(&2));
    assert_eq!(got.report.statistics.get("tracks"), Some(&3));
    let layer = |name: &str| got.report.layers.iter().find(|l| l.source == name).unwrap_or_else(|| panic!("{name}")).clone();
    assert_eq!((layer("Top").kicad.as_deref(), layer("Top").explicit), (Some("F.Cu"), false));
    assert_eq!(layer("200bmp").kicad, None, "UNDEFINED is a layer left out");

    // The findings: the report's error and warning, then the importers' own lines, each message once with its count; an information line is not one.
    let findings = got.findings();
    let shown: Vec<(&str, &str, usize)> = findings.iter().map(|f| (f.severity.name(), f.message.as_str(), f.count)).collect();
    assert_eq!(
        shown,
        vec![
            ("error", "Could not find image file 'C:\\x.bmp'", 1),
            ("error", "No library symbol found for schematic symbol 'R1 10k'.", 2),
            ("warning", "KiCad design rules are different from Eagle ones.", 1),
            ("warning", "Font 'CADSTAR' not found; substituting 'KiCad Font'.", 1),
        ]
    );
    assert!(got.log.iter().any(|l| l.severity == Severity::Info && l.text.starts_with("Setting schematic symbol")), "kept in the log");
    let j = got.to_json();
    assert_eq!((j["errors"].as_u64(), j["warnings"].as_u64()), (Some(3), Some(2)), "counts are of messages said, not of distinct ones");
    assert_eq!(j["format"], "Eagle");
    assert_eq!(j["findings"][1]["count"], 2);
    let layers = j["layers"].as_array().unwrap();
    assert!(layers.contains(&serde_json::json!({ "source": "200bmp", "kicad": null, "explicit": false })) && layers.contains(&serde_json::json!({ "source": "Top", "kicad": "F.Cu", "explicit": false })), "{layers:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_format_and_a_layer_map_are_handed_to_kicad_cli_and_a_layer_it_names_is_explicit_in_the_report() {
    let mut env = Env::take();
    let dir = scratch("map");
    let log = dir.join("calls.log");
    env.set("FAKE_LOG", &log);
    env.set("EDA_KICAD_CLI", script(&dir, "importing.sh", IMPORTING_CLI));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let input = foreign::stage_upload(&dir.join("work"), "b.brd", b"x").unwrap();
    let mut opts = ForeignOptions { format: Some("eagle".into()), ..Default::default() };
    opts.layer_map.insert("tNames".into(), "F.Fab".into());
    opts.layer_map.insert("Unknown Layer".into(), "User.1".into());
    let got = import_foreign_pcb(&dir.join("work"), &input, &opts).expect("an import");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.starts_with("pcb import --format eagle --layer-map "), "{calls}");
    let map: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("calls.log.map")).unwrap()).unwrap();
    assert_eq!(map, serde_json::json!({ "Unknown Layer": "User.1", "tNames": "F.Fab" }), "a JSON object of source layer to KiCad layer, as the option wants");
    assert!(got.report.layers.iter().any(|l| l.source == "tNames" && l.explicit), "{:?}", got.report.layers);

    // `auto` is kicad-cli's own word for not saying; a name that is not one is refused before kicad-cli is started.
    let _ = std::fs::remove_file(&log);
    import_foreign_pcb(&dir.join("work"), &input, &ForeignOptions { format: Some("auto".into()), ..Default::default() }).unwrap();
    assert!(!std::fs::read_to_string(&log).unwrap().contains("--format"));
    let _ = std::fs::remove_file(&log);
    let e = import_foreign_pcb(&dir.join("work"), &input, &ForeignOptions { format: Some("eagle; rm -rf".into()), ..Default::default() }).unwrap_err();
    assert_eq!(e[0].check, "kicad_cli_import_format_name");
    assert!(!log.exists(), "kicad-cli was not started");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_project_kicad_cli_makes_of_a_board_is_read_when_asked_for_and_its_absence_is_said_and_not_an_error() {
    let mut env = Env::take();
    let dir = scratch("project");
    let log = dir.join("calls.log");
    env.set("FAKE_LOG", &log);
    env.set("EDA_KICAD_CLI", script(&dir, "importing.sh", IMPORTING_CLI));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let work = dir.join("work");
    let input = foreign::stage_upload(&work, "My Board.brd", b"x").unwrap();

    // Not asked for: one launch, no project (`pcb import` writes the board alone).
    let got = import_foreign_pcb(&work, &input, &ForeignOptions::default()).unwrap();
    assert!(got.project.is_none() && got.to_json()["has_project"] == false);
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);

    // Asked for: the importer runs again as the project-level command, in a folder of its own, and the `.kicad_pro` it wrote comes back as text.
    let _ = std::fs::remove_file(&log);
    let got = import_foreign_pcb(&work, &input, &ForeignOptions { project: true, ..Default::default() }).unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 2, "the board, then its project: {calls}");
    assert!(lines[0].starts_with("pcb import "), "{calls}");
    assert!(lines[1].starts_with("import -o ") && lines[1].contains("project/board ") && lines[1].contains("My Board.brd") && !lines[1].contains("--layer-map"), "{calls}");
    let pro: serde_json::Value = serde_json::from_str(got.project.as_deref().expect("the project")).unwrap();
    assert_eq!(pro["net_settings"]["classes"][1]["name"], "highpower");
    assert_eq!(got.to_json()["has_project"], true);
    assert_eq!(got.text, "(kicad_pcb (version 20260901) (generator \"pcbnew\"))\n", "the board is the one `pcb import` made, with its report");
    assert_eq!(got.report.source_format, "Eagle");
    assert!(!got.files.iter().any(|f| f.contains("project")), "the project run's files are not the board's: {:?}", got.files);

    // The layers the person moved are moved in the project's run as well, from the same map file.
    let _ = std::fs::remove_file(&log);
    let mut opts = ForeignOptions { project: true, ..Default::default() };
    opts.layer_map.insert("tNames".into(), "F.Fab".into());
    import_foreign_pcb(&work, &input, &opts).unwrap();
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.lines().nth(1).unwrap().contains("--layer-map "), "{calls}");

    // A schematic has no project of this kind: no second launch.
    let _ = std::fs::remove_file(&log);
    let sch = foreign::stage_upload(&work, "Power.SchDoc", b"x").unwrap();
    let got = import_foreign_sch(&work, &sch, &ForeignOptions { project: true, ..Default::default() }).unwrap();
    assert!(got.project.is_none());
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);

    // A kicad-cli that cannot make the project (an older one, a file the project-level command does not take): the board is imported, and the log says what is missing.
    env.set("FAKE_NO_PROJECT", "1");
    let got = import_foreign_pcb(&work, &input, &ForeignOptions { project: true, ..Default::default() }).expect("the board still comes");
    assert!(got.project.is_none());
    let said = got.log.iter().find(|l| l.text.contains("design rules and net classes")).expect("said");
    assert_eq!(said.severity, Severity::Info, "not an error of the import: the board is whole");
    assert!(said.text.contains("could not be made"), "{}", said.text);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_schematic_import_has_no_layer_map_and_makes_a_kicad_sch() {
    let mut env = Env::take();
    let dir = scratch("sch");
    let log = dir.join("calls.log");
    env.set("FAKE_LOG", &log);
    env.set("EDA_KICAD_CLI", script(&dir, "importing.sh", IMPORTING_CLI));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let input = foreign::stage_upload(&dir.join("work"), "Power.SchDoc", b"x").unwrap();
    let mut opts = ForeignOptions::default();
    opts.layer_map.insert("Top".into(), "F.Cu".into());
    let got = import_foreign_sch(&dir.join("work"), &input, &opts).expect("an import");
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.starts_with("sch import --report-format json "), "{calls}");
    assert!(!calls.contains("--layer-map"), "layers are a board's: {calls}");
    assert_eq!(got.text, "(kicad_sch (version 20260830) (generator \"eeschema\"))\n");
    assert_eq!(got.output.file_name().unwrap(), "Power.kicad_sch");
    assert!(got.output.starts_with(dir.join("work").join("sch-out")));
    assert_eq!(got.files, vec!["sym-lib-table".to_string()]);
    assert_eq!(got.report.statistics.get("symbols"), Some(&36));
    // Boards and schematics of one run do not share a folder: kicad-cli's library cache is written beside the output.
    import_foreign_pcb(&dir.join("work"), &foreign::stage_upload(&dir.join("work"), "b.brd", b"x").unwrap(), &ForeignOptions::default()).unwrap();
    assert!(dir.join("work").join("sch-out").join("Power.kicad_sch").exists() && dir.join("work").join("pcb-out").join("b.kicad_pcb").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_no_importer_recognises_and_one_that_fails_to_load_are_told_apart_and_said_in_kicad_clis_words() {
    let mut env = Env::take();
    let dir = scratch("fail");
    env.set("FAKE_LOG", dir.join("calls.log"));
    env.set("EDA_KICAD_CLI", script(&dir, "importing.sh", IMPORTING_CLI));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let work = dir.join("work");

    let unknown = foreign::stage_upload(&work, "unknown.xyz", b"x").unwrap();
    let e = import_foreign_pcb(&work, &unknown, &ForeignOptions::default()).unwrap_err();
    assert!(foreign::is_unrecognized(&e), "{e:?}");
    assert!(e[0].hint.as_deref().unwrap().contains("No PCB importer recognizes the file format"), "{e:?}");

    let broken = foreign::stage_upload(&work, "broken.brd", b"x").unwrap();
    let e = import_foreign_pcb(&work, &broken, &ForeignOptions::default()).unwrap_err();
    assert!(!foreign::is_unrecognized(&e), "a recognised file that does not load is not an unknown format: {e:?}");
    assert_eq!(e[0].check, "kicad_cli_import");
    let said = e[0].hint.clone().unwrap();
    assert!(said.contains("XML parsing error") && said.contains("Failed to load board"), "the importer's error and kicad-cli's last word: {said}");

    // No file, no run.
    let e = import_foreign_pcb(&work, &work.join("in").join("missing.brd"), &ForeignOptions::default()).unwrap_err();
    assert_eq!(e[0].check, "kicad_cli_import_input");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_kicad_cli_with_no_import_command_is_said_to_need_a_newer_one() {
    let mut env = Env::take();
    let dir = scratch("old");
    // kicad-cli 10.0 and before: `import` is not one of the commands, and the usage is what is printed.
    env.set("EDA_KICAD_CLI", script(&dir, "old.sh", "echo \"Failed to parse 'import', did you mean 'downgrade'\"; echo 'Usage: kicad-cli pcb [--help] {diff,downgrade,drc,export,render,upgrade}'; exit 1"));
    env.set("EDA_KICAD_CONFIG_HOME", dir.join("settings"));
    let input = foreign::stage_upload(&dir.join("work"), "b.brd", b"x").unwrap();
    let e = import_foreign_pcb(&dir.join("work"), &input, &ForeignOptions::default()).unwrap_err();
    assert_eq!(e[0].check, "kicad_cli_import_missing");
    assert!(e[0].hint.as_deref().unwrap().contains("has no `pcb import` command"), "{e:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_importers_lines_are_sorted_by_the_word_after_their_time_stamp() {
    let log = parse_log(
        "Importing '/x/a.sch' using EAGLE format...\n\
         12:33:13 AM: Error: No library symbol found for schematic symbol 'R1'.\n\
         9:05:00 PM: Warning: Layer 'x' could not be mapped\n\
         12:33:14 AM: Setting schematic symbol 'D1' library identifier to 'a:b'.\n\
         23:59:59: Error: a 24 hour clock has no AM or PM\n\
         Error: with no time stamp at all\n\
         \n\
         Successfully saved imported schematic to '/x/a.kicad_sch'\n\
         plain text\n",
    );
    let got: Vec<(Severity, &str)> = log.iter().map(|l| (l.severity, l.text.as_str())).collect();
    assert_eq!(
        got,
        vec![
            (Severity::Error, "No library symbol found for schematic symbol 'R1'."),
            (Severity::Warning, "Layer 'x' could not be mapped"),
            (Severity::Info, "Setting schematic symbol 'D1' library identifier to 'a:b'."),
            (Severity::Error, "a 24 hour clock has no AM or PM"),
            (Severity::Error, "with no time stamp at all"),
            (Severity::Info, "plain text"),
        ]
    );
}

#[test]
fn a_name_from_the_browser_is_a_file_name_and_nothing_else() {
    assert_eq!(upload_name("board.PcbDoc").as_deref(), Some("board.PcbDoc"));
    assert_eq!(upload_name("C:\\Users\\me\\Adafruit AHT20 & Co.brd").as_deref(), Some("Adafruit AHT20 & Co.brd"));
    assert_eq!(upload_name("../../etc/passwd").as_deref(), Some("passwd"));
    assert_eq!(upload_name("PCB_USBMETER-PD QC修改版.json").as_deref(), Some("PCB_USBMETER-PD QC修改版.json"));
    for bad in ["", "/", "..", ".hidden", "a/"] {
        assert_eq!(upload_name(bad), None, "{bad:?}");
    }
    assert!(foreign::stage_upload(&scratch("stage"), "..", b"x").is_err());
}

#[test]
fn the_importers_that_ask_for_a_layer_map_are_named() {
    for f in ["Altium Designer", "Eagle", "PADS", "CADSTAR PCB Archive", "Allegro", "SolidWorks PCB"] {
        assert!(foreign::maps_layers(f), "{f}");
    }
    for f in ["P-Cad", "Fabmaster", "EasyEDA / JLCEDA Std", ""] {
        assert!(!foreign::maps_layers(f), "{f}");
    }
}
