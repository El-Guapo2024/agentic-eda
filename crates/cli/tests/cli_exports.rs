//! File > Export > IDFv3... and GenCAD... through the CLI (`eda board export --kicad idf|gencad [-- kicad-cli args]`, the command
//! `/api/fab/idf` and `/api/fab/gencad` run with the dialogs' arguments): kicad-cli's exporters, on the derived board of a placed and routed
//! design. The slow tier (`EDA_SLOW_TESTS=1`, the real kicad-cli) reads back enough of each file to hold it to the board: IDF's outline
//! (`.BOARD_OUTLINE`) and parts (`.PLACEMENT`, the `.emp`'s `.ELECTRICAL` outlines), GenCAD's outline (`$BOARD`), parts (`$COMPONENTS`) and
//! nets (`$SIGNALS`). IDF has no nets: an IDFv3 board file holds mechanical data only. The fast tier holds the refusal of a kicad-cli that
//! has no IDF exporter.
//!
//! The board is `examples/opamp_filter.yaml` placed and routed by `eda pipeline`, in a folder made by `eda board new` (the examples are never
//! written to).

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn eda(args: &[&str], env: &[(&str, &Path)]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_eda"));
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run eda");
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn ok(args: &[&str]) -> String {
    let (good, log) = eda(args, &[]);
    assert!(good, "eda {}:\n{log}", args.join(" "));
    log
}

fn slow() -> bool {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return false;
    }
    true
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_cli_exports_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn example() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/opamp_filter.yaml")
}

/// A board folder whose design is the placed and routed opamp filter.
fn routed_board(tag: &str) -> PathBuf {
    let root = scratch(tag);
    let p = root.join("pipeline");
    ok(&["pipeline", example().to_str().unwrap(), "-o", p.to_str().unwrap(), "--seed", "0"]);
    let b = root.join("board");
    ok(&["board", "new", example().to_str().unwrap(), "-o", b.to_str().unwrap()]);
    std::fs::copy(p.join("design.json"), b.join("design.json")).unwrap();
    b
}

/// `eda board export --kicad <kind> -C <dir> -- <args>`: the files it wrote, relative to `dir`.
fn export(dir: &Path, kind: &str, args: &[&str]) -> Vec<String> {
    let mut all = vec!["board", "export", "--kicad", kind, "-C", dir.to_str().unwrap(), "--"];
    all.extend_from_slice(args);
    let log = ok(&all);
    let v: Value = serde_json::from_str(&log[log.find('{').expect("a JSON reply")..]).unwrap_or_else(|e| panic!("{e}: {log}"));
    v["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect()
}

/// The design's outline (mm) and its parts' positions (mm), by reference.
struct Board {
    outline: Vec<(f64, f64)>,
    parts: BTreeMap<String, (f64, f64)>,
}

fn board(dir: &Path) -> Board {
    let d: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap();
    let mm = |v: &Value| v.as_f64().unwrap() / 1000.0;
    let outline = d["placement"]["outline"].as_array().unwrap().iter().map(|p| (mm(&p["x"]), mm(&p["y"]))).collect();
    let parts = d["placement"]["footprints"].as_array().unwrap().iter().map(|f| (f["id"].as_str().unwrap().to_string(), (mm(&f["at"]["x"]), mm(&f["at"]["y"])))).collect();
    Board { outline, parts }
}

fn bbox(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)))
}

fn near(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
}

/// The intent's nets: name -> { "REF.PIN" }.
fn intent_nets() -> BTreeMap<String, BTreeSet<String>> {
    let y: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(example()).unwrap()).unwrap();
    y["nets"].as_sequence().unwrap().iter().map(|n| (n["name"].as_str().unwrap().to_string(), n["pins"].as_sequence().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect())).collect()
}

// ---------------------------------------------------------------------------------------------------------------- IDF

/// What a `.emn` says: its units, the board outline's points, and each placed part's (reference, x, y, side).
struct Emn {
    units: String,
    thickness: f64,
    outline: Vec<(f64, f64)>,
    placement: BTreeMap<String, (f64, f64, String)>,
}

/// The lines of `text` between `.<section>` and `.END_<section>` (the first line's owner word kept off).
fn idf_section<'a>(text: &'a str, section: &str) -> Vec<&'a str> {
    text.lines().skip_while(|l| !l.starts_with(&format!(".{section}"))).skip(1).take_while(|l| !l.starts_with(&format!(".END_{section}"))).filter(|l| !l.trim().is_empty()).collect()
}

fn parse_emn(text: &str) -> Emn {
    let header = idf_section(text, "HEADER");
    assert!(header[0].starts_with("BOARD_FILE 3.0 "), "an IDFv3 board file: {}", header[0]);
    let units = header[1].split_whitespace().last().unwrap().to_string();
    let outline = idf_section(text, "BOARD_OUTLINE");
    let thickness: f64 = outline[0].trim().parse().unwrap();
    // `loop_index x y angle`
    let outline = outline[1..].iter().map(|l| l.split_whitespace().collect::<Vec<_>>()).map(|w| (w[1].parse().unwrap(), w[2].parse().unwrap())).collect();
    // Two lines a part: `"package" "part number" REF`, then `x y z rotation SIDE status`.
    let lines = idf_section(text, "PLACEMENT");
    let placement = lines
        .chunks(2)
        .map(|c| {
            let refdes = c[0].split_whitespace().last().unwrap().to_string();
            let w: Vec<&str> = c[1].split_whitespace().collect();
            (refdes, (w[0].parse().unwrap(), w[1].parse().unwrap(), w[4].to_string()))
        })
        .collect();
    Emn { units, thickness, outline, placement }
}

/// Fast: a kicad-cli whose `pcb export` has no `idf` (an older one) is refused with the reason before anything is run, and the same
/// kicad-cli's GenCAD is not refused for it.
#[test]
fn a_kicad_cli_without_an_idf_exporter_is_refused_with_the_reason() {
    let root = scratch("no_idf");
    let b = root.join("board");
    ok(&["board", "new", example().to_str().unwrap(), "-o", b.to_str().unwrap()]);
    let cli = root.join("old-kicad-cli");
    std::fs::write(&cli, "#!/bin/sh\nif [ \"$1\" = version ]; then echo 10.99.0-old; exit 0; fi\nif [ \"$3\" = --help ]; then echo 'Usage: pcb export [--help] {drill,gencad,gerbers,step}'; exit 0; fi\necho \"$@\" >> \"$0.ran\"\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (good, log) = eda(&["board", "export", "--kicad", "idf", "-C", b.to_str().unwrap()], &[("EDA_KICAD_CLI", &cli)]);
    assert!(!good, "{log}");
    assert!(log.contains("This kicad-cli (10.99.0-old) has no IDF exporter; KiCad 10.99 nightly or later has one."), "{log}");
    assert!(!root.join("old-kicad-cli.ran").exists(), "nothing but the probe ran");
    let (_, log) = eda(&["board", "export", "--kicad", "gencad", "-C", b.to_str().unwrap()], &[("EDA_KICAD_CLI", &cli)]);
    assert!(!log.contains("has no GenCAD exporter"), "{log}");
    assert!(std::fs::read_to_string(root.join("old-kicad-cli.ran")).unwrap().contains("pcb export gencad"), "the GenCAD export was run");
}

/// The dialog's defaults (`--units mm --origin center`): the board file has the board's outline about the centre of its bounding box
/// (`ExportIDF` without a reference point), the y axis up, every placed part where it is on the board; the library file an outline for
/// each part's footprint and value.
#[test]
fn idf_has_the_boards_outline_and_parts_about_its_centre() {
    if !slow() {
        return;
    }
    let b = routed_board("idf");
    let files = export(&b, "idf", &["--units", "mm", "--origin", "center"]);
    assert_eq!(files, ["export/kicad/idf/opamp_filter.emn", "export/kicad/idf/opamp_filter.emp"], "the board and its library");
    let emn = parse_emn(&std::fs::read_to_string(b.join(&files[0])).unwrap());
    let want = board(&b);
    let (x0, y0, x1, y1) = bbox(&want.outline);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    assert_eq!(emn.units, "MM");
    near(emn.thickness, 1.6, 1e-6, "the board's thickness");
    assert_eq!(emn.outline.first(), emn.outline.last(), "a closed outline");
    let (a, b2, c, d) = bbox(&emn.outline);
    for (got, want, what) in [(a, x0 - cx, "left"), (c, x1 - cx, "right"), (b2, -(y1 - cy), "bottom"), (d, -(y0 - cy), "top")] {
        near(got, want, 1e-4, what);
    }
    assert_eq!(emn.placement.keys().collect::<Vec<_>>(), want.parts.keys().collect::<Vec<_>>(), "every part placed");
    for (r, &(x, y)) in &want.parts {
        let (ix, iy, side) = &emn.placement[r];
        near(*ix, x - cx, 1e-4, &format!("{r} x"));
        near(*iy, -(y - cy), 1e-4, &format!("{r} y"));
        assert_eq!(side, "TOP", "{r}");
    }
    let emp = std::fs::read_to_string(b.join(&files[1])).unwrap();
    assert!(emp.starts_with(".HEADER\nLIBRARY_FILE 3.0 "), "{emp}");
    let outlines: BTreeSet<String> = emp.lines().filter(|l| l.starts_with('"')).map(|l| l.split_whitespace().take(2).collect::<Vec<_>>().join(" ")).collect();
    for l in idf_section(&std::fs::read_to_string(b.join(&files[0])).unwrap(), "PLACEMENT").into_iter().filter(|l| l.starts_with('"')) {
        let key = l.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        assert!(outlines.contains(&key), "an .ELECTRICAL outline for {key}: {outlines:?}");
    }
}

/// "Set board reference point:" with Mils (and with Millimeters), with the arguments `idf_args` builds for it: the outline is about the point the dialog was given
/// (here the board's top left corner), in mils. This holds kicad-cli to the reading of `--user-origin-x/-y` `idf_args` compensates for
/// (1/25400 mm with `--units mils`): a kicad-cli that reads them otherwise fails here.
#[test]
fn idf_takes_the_dialogs_reference_point_in_mils() {
    if !slow() {
        return;
    }
    let b = routed_board("idf_ref");
    let want = board(&b);
    let (x0, y0, x1, y1) = bbox(&want.outline);
    for (units, per_mm, tol) in [("mils", 1.0 / 0.0254, 0.11), ("mm", 1.0, 1e-4)] {
        let per_unit = if units == "mils" { 25_400.0 } else { 1_000_000.0 };
        let (ux, uy) = (format!("--user-origin-x={}", (x0 * per_unit).round() as i64), format!("--user-origin-y={}", (y0 * per_unit).round() as i64));
        let files = export(&b, "idf", &["--units", units, "--origin", "user", &ux, &uy]);
        let emn = parse_emn(&std::fs::read_to_string(b.join(&files[0])).unwrap());
        assert_eq!(emn.units, if units == "mils" { "THOU" } else { "MM" });
        let (a, b2, c, d) = bbox(&emn.outline);
        for (got, want, what) in [(a, 0.0, "left"), (c, (x1 - x0) * per_mm, "right"), (d, 0.0, "top"), (b2, -(y1 - y0) * per_mm, "bottom")] {
            near(got, want, tol, &format!("{units} {what}"));
        }
        let (r, &(x, y)) = want.parts.iter().next().unwrap();
        let (ix, iy, _) = emn.placement[r];
        near(ix, (x - x0) * per_mm, tol, &format!("{units} {r} x"));
        near(iy, -(y - y0) * per_mm, tol, &format!("{units} {r} y"));
    }
}

// ------------------------------------------------------------------------------------------------------------- GenCAD

/// What a `.cad` says: its units, the `$BOARD` lines' end points, each `$COMPONENTS` part's place, and each `$SIGNALS` net's nodes.
struct Cad {
    units: String,
    board: Vec<(f64, f64)>,
    components: BTreeMap<String, (f64, f64)>,
    signals: BTreeMap<String, BTreeSet<String>>,
}

fn cad_section<'a>(text: &'a str, section: &str) -> Vec<&'a str> {
    text.lines().skip_while(|l| *l != format!("${section}")).skip(1).take_while(|l| *l != format!("$END{section}")).collect()
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn parse_cad(text: &str) -> Cad {
    let header = cad_section(text, "HEADER");
    assert!(header.contains(&"GENCAD 1.4"), "{header:?}");
    let units = header.iter().find_map(|l| l.strip_prefix("UNITS ")).unwrap().to_string();
    let nums = |l: &str| l.split_whitespace().skip(1).map(|w| w.parse::<f64>().unwrap()).collect::<Vec<_>>();
    let board: Vec<(f64, f64)> = cad_section(text, "BOARD")
        .iter()
        .filter(|l| l.starts_with("LINE "))
        .flat_map(|l| {
            let n = nums(l);
            [(n[0], n[1]), (n[2], n[3])]
        })
        .collect();
    let mut components = BTreeMap::new();
    let mut at: Option<String> = None;
    for l in cad_section(text, "COMPONENTS") {
        if let Some(name) = l.strip_prefix("COMPONENT ") {
            at = Some(unquote(name));
        } else if let (Some(name), true) = (&at, l.starts_with("PLACE ")) {
            let n = nums(l);
            components.insert(name.clone(), (n[0], n[1]));
        }
    }
    let mut signals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut net = String::new();
    for l in cad_section(text, "SIGNALS") {
        if let Some(name) = l.strip_prefix("SIGNAL ") {
            net = unquote(name);
            signals.entry(net.clone()).or_default();
        } else if let Some(node) = l.strip_prefix("NODE ") {
            let w: Vec<String> = node.split_whitespace().map(unquote).collect();
            signals.get_mut(&net).unwrap().insert(format!("{}.{}", w[0], w[1]));
        }
    }
    Cad { units, board, components, signals }
}

/// The dialog's defaults (no option checked): the board's outline, every part at its place and every net of the intent with its pins, in
/// inches, the y axis up, from the board's own origin (`SetPlotOffet( 0, 0 )` without "Use drill/place file origin as origin").
#[test]
fn gencad_has_the_boards_outline_parts_and_nets() {
    if !slow() {
        return;
    }
    let b = routed_board("gencad");
    let files = export(&b, "gencad", &[]);
    assert_eq!(files, ["export/kicad/gencad/opamp_filter.cad"]);
    let cad = parse_cad(&std::fs::read_to_string(b.join(&files[0])).unwrap());
    assert_eq!(cad.units, "INCH");
    let want = board(&b);
    let inch = |mm: f64| mm / 25.4;
    let (x0, y0, x1, y1) = bbox(&want.outline);
    let (a, b2, c, d) = bbox(&cad.board);
    for (got, want, what) in [(a, inch(x0), "left"), (c, inch(x1), "right"), (b2, -inch(y1), "bottom"), (d, -inch(y0), "top")] {
        near(got, want, 1e-6, what);
    }
    assert_eq!(cad.components.keys().collect::<Vec<_>>(), want.parts.keys().collect::<Vec<_>>(), "every part a component");
    for (r, &(x, y)) in &want.parts {
        let (gx, gy) = cad.components[r];
        near(gx, inch(x), 1e-6, &format!("{r} x"));
        near(gy, -inch(y), 1e-6, &format!("{r} y"));
    }
    let nets = intent_nets();
    assert!(!nets.is_empty());
    for (name, pins) in &nets {
        assert_eq!(cad.signals.get(name), Some(pins), "the net {name} and its pins");
    }
}

/// Every option of the dialog is a flag kicad-cli takes: "Flip bottom footprint padstacks", "Generate unique pin names", "Generate a new
/// shape for each footprint instance", "Use drill/place file origin as origin", "Save the origin coordinates in the file". Each instance's own
/// shape is named after its reference; the nets stay.
#[test]
fn gencad_takes_every_option_of_the_dialog() {
    if !slow() {
        return;
    }
    let b = routed_board("gencad_opts");
    let files = export(&b, "gencad", &["--flip-bottom-pads", "--unique-pins", "--unique-footprints", "--use-drill-origin", "--store-origin-coord"]);
    let text = std::fs::read_to_string(b.join(&files[0])).unwrap();
    let cad = parse_cad(&text);
    assert_eq!(cad.components.len(), board(&b).parts.len());
    let shapes: BTreeSet<String> = cad_section(&text, "SHAPES").iter().filter_map(|l| l.strip_prefix("SHAPE ")).map(unquote).collect();
    assert_eq!(shapes.len(), cad.components.len(), "a shape for each footprint instance: {shapes:?}");
    assert_eq!(cad.signals.len(), intent_nets().len(), "{:?}", cad.signals.keys());
}
