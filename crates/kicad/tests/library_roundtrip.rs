//! The library editors' round trip of KiCad's own libraries (P17 review): a sample of the installed footprints and symbols is read
//! by the editors' readers ([`eda_kicad::parse_library_footprint`], [`eda_kicad::parse_library_symbols`]), written back by their
//! writers ([`eda_kicad::export_kicad_mod_with`], [`eda_kicad::export_kicad_sym_library_with`]), and both the original and our copy are
//! re-saved by `kicad-cli fp upgrade` / `sym upgrade --force`: what KiCad writes for the two is compared node by node (uuids and the
//! file's version and generator aside, numbers to the micrometre of the editors' grid).
//!
//! Slow tier (`EDA_SLOW_TESTS=1`, and kicad-cli and the installed libraries must be there). `EDA_LIB_ROUNDTRIP_REPORT=1` prints
//! every difference; the test fails when an item's difference is not one of [`KNOWN`], the losses this port still has.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ----------------------------------------------------------------------------------------------------------------------
// a small s-expression reader, enough for what kicad-cli writes
// ----------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Sx {
    Atom(String),
    List(Vec<Sx>),
}

fn parse(text: &str) -> Vec<Sx> {
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut stack: Vec<Vec<Sx>> = vec![Vec::new()];
    while i < b.len() {
        let c = b[i];
        match c {
            '(' => {
                stack.push(Vec::new());
                i += 1;
            }
            ')' => {
                let l = stack.pop().unwrap();
                stack.last_mut().unwrap().push(Sx::List(l));
                i += 1;
            }
            '"' => {
                let mut s = String::from("\"");
                i += 1;
                while i < b.len() && b[i] != '"' {
                    if b[i] == '\\' && i + 1 < b.len() {
                        s.push(b[i]);
                        i += 1;
                    }
                    s.push(b[i]);
                    i += 1;
                }
                s.push('"');
                i += 1;
                stack.last_mut().unwrap().push(Sx::Atom(s));
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                let mut s = String::new();
                while i < b.len() && !b[i].is_whitespace() && b[i] != '(' && b[i] != ')' {
                    s.push(b[i]);
                    i += 1;
                }
                stack.last_mut().unwrap().push(Sx::Atom(s));
            }
        }
    }
    stack.pop().unwrap()
}

fn tag(l: &[Sx]) -> &str {
    match l.first() {
        Some(Sx::Atom(a)) => a.as_str(),
        _ => "",
    }
}

/// A number to the micrometre (the editors' grid is the micrometre: a KiCad length finer than it is rounded on the way in).
fn norm_atom(a: &str, coarse: bool) -> String {
    if coarse {
        if let Ok(v) = a.parse::<f64>() {
            if a.contains('.') || a.contains('e') {
                let r = (v * 1000.0).round() / 1000.0;
                return format!("{}", if r == 0.0 { 0.0 } else { r });
            }
        }
    }
    a.to_string()
}

/// Each node of `l` as one line `path: atoms`, the path being the tags from the item down (`pad/size`), into `out`. Skipped: the
/// uuids, the file's version and generator.
fn flatten(l: &[Sx], path: &str, coarse: bool, out: &mut Vec<(String, String)>) {
    let t = tag(l);
    if matches!(t, "uuid" | "tstamp" | "version" | "generator" | "generator_version") {
        return;
    }
    let here = if path.is_empty() { t.to_string() } else { format!("{path}/{t}") };
    let atoms: Vec<String> = l.iter().skip(1).filter_map(|c| if let Sx::Atom(a) = c { Some(norm_atom(a, coarse)) } else { None }).collect();
    // a pad, a property or a sub-symbol is named by its first atom, so its children say whose they are
    let named = matches!(t, "pad" | "property" | "symbol" | "pin" | "model" | "fp_text" | "text" | "name" | "number" | "alternate");
    let child_path = if named && !atoms.is_empty() { format!("{here}[{}]", atoms[0]) } else { here.clone() };
    out.push((here.clone(), format!("{child_path}: {}", atoms.join(" "))));
    for c in l.iter().skip(1) {
        if let Sx::List(cl) = c {
            flatten(cl, &child_path, coarse, out);
        }
    }
}

/// The differences between KiCad's write of the original item and of our copy: (path, "-" what only the original has / "+" what only
/// ours has).
fn diff(original: &[Sx], ours: &[Sx], coarse: bool) -> Vec<(String, String)> {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for c in original.iter().skip(2) {
        if let Sx::List(l) = c {
            flatten(l, "", coarse, &mut a);
        }
    }
    for c in ours.iter().skip(2) {
        if let Sx::List(l) = c {
            flatten(l, "", coarse, &mut b);
        }
    }
    let mut count: BTreeMap<&str, (i64, &str)> = BTreeMap::new();
    for (p, s) in &a {
        count.entry(s.as_str()).or_insert((0, p.as_str())).0 += 1;
    }
    for (p, s) in &b {
        count.entry(s.as_str()).or_insert((0, p.as_str())).0 -= 1;
    }
    let mut out = Vec::new();
    for (s, (n, p)) in count {
        if n > 0 {
            out.push((p.to_string(), format!("- {s}")));
        } else if n < 0 {
            out.push((p.to_string(), format!("+ {s}")));
        }
    }
    out
}

/// A path's category: its tags without the names (`pad/drill`, `symbol/symbol/pin/name`).
fn category(path: &str) -> String {
    let mut s = String::new();
    let mut depth = 0;
    for c in path.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    s
}

// ----------------------------------------------------------------------------------------------------------------------
// the sample
// ----------------------------------------------------------------------------------------------------------------------

fn sorted_dir(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext) && !p.file_name().unwrap().to_string_lossy().starts_with("._")).collect()).unwrap_or_default();
    v.sort();
    v
}

/// About `n` footprints: one from every few libraries, a different place in each, and a few of known shapes (custom pads, net ties,
/// slots, thermal vias, many fields).
fn footprint_sample(root: &Path, n: usize) -> Vec<PathBuf> {
    let libs = sorted_dir(root, "pretty");
    let mut out = Vec::new();
    let step = (libs.len() / n).max(1);
    for (k, lib) in libs.iter().enumerate().filter(|(k, _)| k % step == 0) {
        let fps = sorted_dir(lib, "kicad_mod");
        if !fps.is_empty() {
            out.push(fps[(k * 7) % fps.len()].clone());
        }
    }
    for extra in [
        "NetTie.pretty/NetTie-2_SMD_Pad0.5mm.kicad_mod",
        "Jumper.pretty/SolderJumper-2_P1.3mm_Bridged_RoundedPad1.0x1.5mm.kicad_mod",
        "Connector_USB.pretty/USB_C_Receptacle_HRO_TYPE-C-31-M-12.kicad_mod",
        "Package_DFN_QFN.pretty/QFN-16-1EP_3x3mm_P0.5mm_EP1.7x1.7mm_ThermalVias.kicad_mod",
        "MountingHole.pretty/MountingHole_3.2mm_M3_Pad_Via.kicad_mod",
        "TestPoint.pretty/TestPoint_Pad_D1.0mm.kicad_mod",
        "Package_BGA.pretty/BGA-64_9.0x9.0mm_Layout10x10_P0.8mm.kicad_mod",
        "Symbol.pretty/KiCad-Logo_5mm_SilkScreen.kicad_mod",
        "Connector_PinHeader_2.54mm.pretty/PinHeader_2x05_P2.54mm_Vertical.kicad_mod",
        "Capacitor_THT.pretty/CP_Radial_D5.0mm_P2.00mm.kicad_mod",
    ] {
        let p = root.join(extra);
        if p.exists() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// About `n` symbols by (library file, name): one from every few libraries, then derived and multi-unit ones until there are at least
/// a dozen of each.
fn symbol_sample(root: &Path, n: usize) -> Vec<(PathBuf, String)> {
    let libs = sorted_dir(root, "kicad_sym");
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let (mut derived, mut multi) = (0, 0);
    let step = (libs.len() / n).max(1);
    for (k, lib) in libs.iter().enumerate() {
        let text = std::fs::read_to_string(lib).unwrap_or_default();
        let sums = eda_kicad::scan_symbols(&text);
        if sums.is_empty() {
            continue;
        }
        if k % step == 0 {
            let s = &sums[(k * 13) % sums.len()];
            derived += s.extends.is_some() as usize;
            multi += (s.unit_count() > 1) as usize;
            out.push((lib.clone(), s.name.clone()));
        }
        if derived < 12 && k % 3 == 1 {
            if let Some(s) = sums.iter().filter(|s| s.extends.is_some()).nth(k % 5) {
                if !out.iter().any(|(l, n)| l == lib && *n == s.name) {
                    derived += 1;
                    multi += (s.unit_count() > 1) as usize;
                    out.push((lib.clone(), s.name.clone()));
                }
            }
        }
        if multi < 12 && k % 3 == 2 {
            if let Some(s) = sums.iter().filter(|s| s.unit_count() > 1 && s.extends.is_none()).nth(k % 4) {
                if !out.iter().any(|(l, n)| l == lib && *n == s.name) {
                    multi += 1;
                    out.push((lib.clone(), s.name.clone()));
                }
            }
        }
    }
    // symbols of known shapes: beziers, unit names, a private field, a power symbol, symbols excluded from the position files, a DeMorgan symbol
    for (lib, name) in [
        ("Auxiliary_Items", "Spring"),
        ("Device", "Thermal_Jumper"),
        ("Transistor_FET", "Q_Dual_NMOS_PMOS_G1S2G2D2S1D1"),
        ("FPGA_Altera_MAX10", "10M04SCE144"),
        ("power", "GND"),
        ("Graphic", "Annotation_Exclamation"),
        ("74xx", "74LS00"),
        ("Device", "R"),
    ] {
        let path = root.join(format!("{lib}.kicad_sym"));
        if path.exists() && !out.iter().any(|(l, n)| *l == path && n == name) {
            out.push((path, name.to_string()));
        }
    }
    eprintln!("symbol sample: {} symbols, {derived} derived, {multi} with more than one unit", out.len());
    out
}

/// The symbol `name` of a library file with every symbol it extends, as one library of KiCad's (the forms as the file has them).
fn symbol_library_of(text: &str, name: &str) -> (String, Vec<String>) {
    let sums = eda_kicad::scan_symbols(text);
    let mut chain = vec![name.to_string()];
    let mut at = name.to_string();
    while let Some(parent) = sums.iter().find(|s| s.name == at).and_then(|s| s.extends.clone()) {
        chain.push(parent.clone());
        at = parent;
    }
    // the file's own version: what `~` and other tokens mean depends on it
    let version = text.split("(version ").nth(1).and_then(|v| v.split(')').next()).unwrap_or("20241209");
    let mut lib = format!("(kicad_symbol_lib (version {version}) (generator \"kicad_symbol_editor\")\n");
    for n in chain.iter().rev() {
        if let Some(s) = sums.iter().find(|s| s.name == *n) {
            lib.push_str(eda_kicad::symbol_text(text, s));
            lib.push('\n');
        }
    }
    lib.push_str(")\n");
    (lib, chain)
}

// ----------------------------------------------------------------------------------------------------------------------
// kicad-cli
// ----------------------------------------------------------------------------------------------------------------------

fn find_kicad_cli() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn upgrade(cli: &Path, kind: &str, input: &Path, output: &Path) {
    let out = Command::new(cli).args([kind, "upgrade", "--force", "-o"]).arg(output).arg(input).output().expect("runs kicad-cli");
    assert!(out.status.success(), "kicad-cli {kind} upgrade {}: {}{}", input.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// Differences we know and keep, by category, with why. Everything else fails the test.
const KNOWN: &[(&str, &str)] = &[];

struct Tally {
    items: usize,
    exact: usize,
    /// items whose only differences are lengths finer than the micrometre
    sub_nm: usize,
    by_category: BTreeMap<String, (usize, Vec<String>)>,
}

impl Tally {
    fn new() -> Self {
        Tally { items: 0, exact: 0, sub_nm: 0, by_category: BTreeMap::new() }
    }

    fn add(&mut self, item: &str, original: &[Sx], ours: &[Sx], report: bool) {
        self.items += 1;
        let coarse = diff(original, ours, true);
        if coarse.is_empty() {
            if diff(original, ours, false).is_empty() {
                self.exact += 1;
            } else {
                self.sub_nm += 1;
            }
            return;
        }
        let mut cats: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (p, line) in &coarse {
            cats.entry(category(p)).or_default().push(line.clone());
        }
        for (c, lines) in cats {
            if report {
                for l in lines.iter().take(6) {
                    eprintln!("  {item}: {l}");
                }
            }
            let e = self.by_category.entry(c).or_insert((0, Vec::new()));
            e.0 += 1;
            if e.1.len() < 3 {
                e.1.push(format!("{item}: {}", lines[0]));
            }
        }
    }

    fn print(&self, what: &str) {
        eprintln!("{what}: {} items, {} identical, {} identical to the micrometre, {} differ", self.items, self.exact, self.sub_nm, self.items - self.exact - self.sub_nm);
        for (c, (n, ex)) in &self.by_category {
            eprintln!("  {n:>3} items  {c}");
            for e in ex {
                eprintln!("         e.g. {e}");
            }
        }
    }

    fn unknown(&self) -> Vec<String> {
        self.by_category.keys().filter(|c| !KNOWN.iter().any(|(k, _)| c.starts_with(k))).cloned().collect()
    }
}

/// `EDA_LIB_ROUNDTRIP_N` items of each kind (about), else `n`.
fn sample_size(n: usize) -> usize {
    std::env::var("EDA_LIB_ROUNDTRIP_N").ok().and_then(|v| v.parse().ok()).unwrap_or(n)
}

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda_lib_roundtrip_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn kicad_writes_the_installed_footprints_the_same_after_the_footprint_editors_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let root = eda_kicad::kicad_shared_dir("footprints");
    let sample = footprint_sample(&root, sample_size(60));
    if sample.is_empty() {
        eprintln!("skipped: no installed footprints under {}", root.display());
        return;
    }
    let report = std::env::var_os("EDA_LIB_ROUNDTRIP_REPORT").is_some();
    let dir = scratch("fp");
    let (orig, ours) = (dir.join("orig.pretty"), dir.join("ours.pretty"));
    std::fs::create_dir_all(&orig).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    let mut names = Vec::new();
    for (k, path) in sample.iter().enumerate() {
        // one file name per item: two libraries can have a footprint of the same name
        let name = format!("{k:03}_{}", path.file_stem().unwrap().to_string_lossy());
        let text = std::fs::read_to_string(path).unwrap();
        let parsed = eda_kicad::parse_library_footprint(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut fp = parsed.footprint;
        fp.name = path.file_stem().unwrap().to_string_lossy().to_string();
        std::fs::write(orig.join(format!("{name}.kicad_mod")), &text).unwrap();
        std::fs::write(ours.join(format!("{name}.kicad_mod")), eda_kicad::export_kicad_mod_with(&fp, &parsed.embedded)).unwrap();
        names.push((name, path.clone(), parsed.warnings));
    }
    upgrade(&cli, "fp", &orig, &dir.join("orig_up.pretty"));
    upgrade(&cli, "fp", &ours, &dir.join("ours_up.pretty"));
    let mut tally = Tally::new();
    for (name, path, warnings) in &names {
        let item = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        let a = std::fs::read_to_string(dir.join("orig_up.pretty").join(format!("{name}.kicad_mod"))).unwrap();
        let b = std::fs::read_to_string(dir.join("ours_up.pretty").join(format!("{name}.kicad_mod"))).unwrap_or_else(|_| panic!("kicad-cli did not keep our {item}"));
        let (Some(Sx::List(a)), Some(Sx::List(b))) = (parse(&a).into_iter().next(), parse(&b).into_iter().next()) else { panic!("{item}: not a footprint") };
        if report && !warnings.is_empty() {
            eprintln!("  {item}: reader warnings {warnings:?}");
        }
        tally.add(&item, &a, &b, report);
    }
    tally.print("footprints");
    let unknown = tally.unknown();
    assert!(unknown.is_empty(), "differences not in KNOWN: {unknown:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kicad_writes_the_installed_symbols_the_same_after_the_symbol_editors_round_trip() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let root = eda_kicad::kicad_shared_dir("symbols");
    let sample = symbol_sample(&root, sample_size(60));
    if sample.is_empty() {
        eprintln!("skipped: no installed symbols under {}", root.display());
        return;
    }
    let report = std::env::var_os("EDA_LIB_ROUNDTRIP_REPORT").is_some();
    let dir = scratch("sym");
    let (orig, ours) = (dir.join("orig"), dir.join("ours"));
    std::fs::create_dir_all(&orig).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    let mut items = Vec::new();
    for (k, (lib, name)) in sample.iter().enumerate() {
        let text = std::fs::read_to_string(lib).unwrap();
        let (one, chain) = symbol_library_of(&text, name);
        let parsed = eda_kicad::parse_library_symbols(&one).unwrap_or_else(|e| panic!("{}:{name}: {e}", lib.display()));
        let syms: Vec<&eda_model::ir::LibrarySymbol> = parsed.symbols.iter().collect();
        let file = format!("{k:03}.kicad_sym");
        std::fs::write(orig.join(&file), &one).unwrap();
        std::fs::write(ours.join(&file), eda_kicad::export_kicad_sym_library_with(&syms, &parsed.embedded)).unwrap();
        items.push((file, format!("{}:{name}", lib.file_stem().unwrap().to_string_lossy()), chain, parsed.warnings));
    }
    let mut tally = Tally::new();
    for (file, item, chain, warnings) in &items {
        upgrade(&cli, "sym", &orig.join(file), &dir.join(format!("o_{file}")));
        upgrade(&cli, "sym", &ours.join(file), &dir.join(format!("e_{file}")));
        let a = parse(&std::fs::read_to_string(dir.join(format!("o_{file}"))).unwrap());
        let b = parse(&std::fs::read_to_string(dir.join(format!("e_{file}"))).unwrap());
        let syms = |t: &[Sx]| -> BTreeMap<String, Vec<Sx>> {
            let Some(Sx::List(root)) = t.first() else { return BTreeMap::new() };
            root.iter()
                .filter_map(|c| if let Sx::List(l) = c { Some(l) } else { None })
                .filter(|l| tag(l) == "symbol")
                .filter_map(|l| if let Some(Sx::Atom(n)) = l.get(1) { Some((n.trim_matches('"').to_string(), l.clone())) } else { None })
                .collect()
        };
        let (sa, sb) = (syms(&a), syms(&b));
        if report && !warnings.is_empty() {
            eprintln!("  {item}: reader warnings {warnings:?}");
        }
        // the symbol itself; the symbols it extends are there to be extended
        let name = &chain[0];
        match (sa.get(name), sb.get(name)) {
            (Some(x), Some(y)) => tally.add(item, x, y, report),
            _ => panic!("{item}: missing in KiCad's write of {}", if sa.contains_key(name) { "ours" } else { "the original" }),
        }
    }
    tally.print("symbols");
    let unknown = tally.unknown();
    assert!(unknown.is_empty(), "differences not in KNOWN: {unknown:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
