use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{Design, LabelKind, Point, SchematicSection};
use eda_model::modules::infer_modules;
use eda_model::{resolve_lib_id, ConstraintModel, PinKind};

use super::kit::{cap, label_rect, paper_named, power_rect, sheet_pin_rect, text_w, Paper, Placed, Rect, G, SHEET_FILE_FONT, SHEET_NAME_FONT};
use super::{derive_hierarchy, Keep};
use crate::nets::{trace_nets, ScreenIn};
use crate::{derive_schematic_modules, EngineOptions};

fn model_of(file: &str) -> ConstraintModel {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(file);
    serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn opts() -> EngineOptions {
    EngineOptions::new(1, "test")
}

fn mcu30() -> (ConstraintModel, Design) {
    let model = model_of("mcu_board_30plus.yaml");
    let d = derive_schematic_modules(&model, &opts()).expect("derives");
    (model, d)
}

/// Every screen of a design as (file, section); the root has an empty file name.
fn screens(d: &Design) -> Vec<(String, &SchematicSection)> {
    let mut v = vec![(String::new(), d.schematic.as_ref().unwrap())];
    if let Some(c) = &d.sheet_contents {
        v.extend(c.iter().map(|(f, s)| (f.clone(), s)));
    }
    v
}

fn pins_of(model: &ConstraintModel, sch: &SchematicSection) -> BTreeMap<String, Point> {
    let mut out = BTreeMap::new();
    for sym in &sch.symbols {
        let Some(part) = model.part(&sym.id) else { continue };
        let resolved = model.real_symbol_of(&sym.lib_id, part);
        for (number, at) in crate::placed::pin_points(sym, part, resolved.as_ref()) {
            out.insert(format!("{}.{number}", sym.id), at);
        }
    }
    out
}

/// The nets a design's drawn sheets imply.
pub(super) fn traced(model: &ConstraintModel, d: &Design) -> BTreeMap<String, Vec<String>> {
    let mut all: Vec<(String, SchematicSection)> = screens(d).into_iter().map(|(f, s)| (f, s.clone())).collect();
    let pins: Vec<BTreeMap<String, Point>> = all.iter().map(|(_, s)| pins_of(model, s)).collect();
    let mut view: Vec<ScreenIn> = Vec::new();
    for ((file, sch), p) in all.iter_mut().zip(pins) {
        view.push(ScreenIn { sch, file: file.clone(), pins: p });
    }
    trace_nets(&mut view).into_iter().filter(|n| n.pins.len() >= 2).map(|n| (n.name, n.pins)).collect()
}

fn drawn_nets(model: &ConstraintModel) -> BTreeMap<String, Vec<String>> {
    model
        .nets
        .iter()
        .filter_map(|n| {
            let mut pins: Vec<String> = n
                .pins
                .iter()
                .filter(|p| {
                    let (r, num) = p.split_once('.').unwrap_or((p.as_str(), ""));
                    model.part(r).and_then(|part| part.pins.iter().find(|x| x.number == num)).is_some_and(|x| x.kind != PinKind::Nc)
                })
                .cloned()
                .collect();
            pins.sort();
            (pins.len() >= 2).then(|| (n.name.clone(), pins))
        })
        .collect()
}

/// A drawn item's rectangle and who it belongs to (items of one owner may touch).
struct Drawn {
    owner: String,
    what: String,
    rect: Rect,
}

/// Rebuild every rectangle of a sheet from the data alone, by the same rules the layout used.
fn drawn(model: &ConstraintModel, sch: &SchematicSection) -> Vec<Drawn> {
    let mut out = Vec::new();
    for s in &sch.symbols {
        let part = model.part(&s.id).unwrap();
        let resolved = model.real_symbol_of(&s.lib_id, part);
        let package = part.package.clone().unwrap_or_default();
        let mut p = Placed::new(part, &s.lib_id, resolved, &s.value, &package);
        p.flip = s.rot == 180_000;
        if p.flip {
            p.x = s.at.x - p.w();
            p.y = s.at.y - p.h();
        } else {
            p.x = s.at.x;
            p.y = s.at.y;
        }
        out.push(Drawn { owner: s.id.clone(), what: format!("symbol {}", s.id), rect: p.keepout() });
    }
    for ps in &sch.power_symbols {
        if ps.lib_id == "power:PWR_FLAG" {
            continue;
        }
        let ground = ps.net.to_ascii_uppercase().starts_with("GND");
        let up = (!ground) == (ps.rot == 0);
        let owner = ps.pin.split('.').next().unwrap_or("").to_string();
        out.push(Drawn { owner, what: format!("power {} {}", ps.id, ps.net), rect: power_rect(ps.at, &ps.net, up) });
    }
    for l in &sch.labels {
        // the way the label's wire leaves it
        let w = sch.wires.iter().find(|w| w.pts.last() == Some(&l.at) || w.pts.first() == Some(&l.at));
        let (owner, dir) = match w {
            Some(w) => {
                let (end, prev) = if w.pts.last() == Some(&l.at) { (w.pts[w.pts.len() - 1], w.pts[w.pts.len() - 2]) } else { (w.pts[0], w.pts[1]) };
                (w.pins.first().map(|p| p.split('.').next().unwrap_or("").to_string()).unwrap_or_default(), ((end.x - prev.x).signum(), (end.y - prev.y).signum()))
            }
            None => (String::new(), (1, 0)),
        };
        let hier = matches!(l.kind, LabelKind::Hierarchical { .. });
        out.push(Drawn { owner: if owner.is_empty() { format!("label {} {:?}", l.net, l.at) } else { owner }, what: format!("label {} at {:?}", l.net, l.at), rect: label_rect(l.at, dir, &l.net, hier) });
    }
    for s in &sch.sheets {
        let b = Rect::new(s.at.x, s.at.y, s.at.x + s.size.0, s.at.y + s.size.1);
        out.push(Drawn { owner: format!("sheet {}", s.name), what: format!("sheet {}", s.name), rect: b });
        out.push(Drawn { owner: format!("sheet {}", s.name), what: format!("name {}", s.name), rect: Rect::new(s.at.x, s.at.y - 400 - cap(SHEET_NAME_FONT), s.at.x + text_w(SHEET_NAME_FONT, &s.name), s.at.y - 400) });
        let base = s.at.y + s.size.1 + 400 + SHEET_FILE_FONT;
        out.push(Drawn { owner: format!("sheet {}", s.name), what: format!("file {}", s.file), rect: Rect::new(s.at.x, base - cap(SHEET_FILE_FONT), s.at.x + text_w(SHEET_FILE_FONT, &s.file), base) });
        for p in &s.pins {
            out.push(Drawn { owner: format!("sheet {}", s.name), what: format!("pin {}", p.name), rect: sheet_pin_rect(p.at, p.at.x == s.at.x, &p.name) });
        }
    }
    out
}

fn paper_of(sch: &SchematicSection) -> Paper {
    paper_named(sch.title_block.as_ref().map(|t| t.paper.as_str()).filter(|p| !p.is_empty()).unwrap_or("A4"))
}

/// Everything of every sheet lies inside the frame's drawing area and nothing of one part overlaps another's.
pub(super) fn assert_tidy(model: &ConstraintModel, d: &Design, what: &str) {
    for (file, sch) in screens(d) {
        let usable = paper_of(sch).usable();
        let items = drawn(model, sch);
        for it in &items {
            assert!(usable.contains(&it.rect), "{what} {file:?}: {} {:?} is outside the drawing area {:?}", it.what, it.rect, usable);
        }
        for w in &sch.wires {
            for p in &w.pts {
                assert!(usable.contains(&Rect { x0: p.x, y0: p.y, x1: p.x, y1: p.y }), "{what} {file:?}: wire point {p:?} outside {usable:?}");
                assert_eq!(p.x % G, 0, "{what} {file:?}: wire point {p:?} is off the grid");
                assert_eq!(p.y % G, 0, "{what} {file:?}: wire point {p:?} is off the grid");
            }
        }
        for (i, a) in items.iter().enumerate() {
            for b in &items[i + 1..] {
                if a.owner == b.owner {
                    continue;
                }
                assert!(!a.rect.overlaps(&b.rect), "{what} {file:?}: {} {:?} overlaps {} {:?}", a.what, a.rect, b.what, b.rect);
            }
        }
        for s in &sch.symbols {
            assert_eq!(s.at.x % G, 0, "{what} {file:?}: {} is off the grid", s.id);
            assert_eq!(s.at.y % G, 0, "{what} {file:?}: {} is off the grid", s.id);
        }
    }
}

#[test]
fn mcu30_becomes_a_root_and_three_module_sheets() {
    let (model, d) = mcu30();
    let root = d.schematic.as_ref().unwrap();
    assert!(root.symbols.is_empty(), "the root holds sheet symbols only");
    let names: Vec<&str> = root.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["MCU (U1)", "LED channels (D1\u{2013}D8)", "Header (J1)"]);
    let contents = d.sheet_contents.as_ref().unwrap();
    assert_eq!(contents.len(), 3);
    // Every part is on exactly one sheet.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for sec in contents.values() {
        for s in &sec.symbols {
            *seen.entry(s.id.clone()).or_default() += 1;
        }
    }
    assert_eq!(seen.len(), 30);
    assert!(seen.values().all(|&n| n == 1));
    assert_eq!(seen.keys().cloned().collect::<BTreeSet<_>>(), model.parts.iter().map(|p| p.reference.clone()).collect::<BTreeSet<_>>());
    // The nets that cross: PA0..PA7 between the MCU and the channels, RESET and SWD between the MCU and the header.
    let pins_of_sheet = |name: &str| -> BTreeSet<String> { root.sheets.iter().find(|s| s.name.starts_with(name)).unwrap().pins.iter().map(|p| p.name.clone()).collect() };
    let want_mcu: BTreeSet<String> = ["PA0", "PA1", "PA2", "PA3", "PA4", "PA5", "PA6", "PA7", "RESET", "SWD"].iter().map(|s| s.to_string()).collect();
    assert_eq!(pins_of_sheet("MCU"), want_mcu);
    assert_eq!(pins_of_sheet("LED"), (0..8).map(|i| format!("PA{i}")).collect::<BTreeSet<_>>());
    assert_eq!(pins_of_sheet("Header"), ["RESET", "SWD"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>());
    // Power is never a sheet pin.
    assert!(root.sheets.iter().all(|s| s.pins.iter().all(|p| p.name != "VDD" && p.name != "GND")));
    // Each sheet pin has its hierarchical label inside.
    for sh in &root.sheets {
        let child = &contents[&sh.file];
        for p in &sh.pins {
            assert!(child.labels.iter().any(|l| l.net == p.name && matches!(l.kind, LabelKind::Hierarchical { .. })), "{} has no hierarchical label {}", sh.file, p.name);
        }
    }
}

#[test]
fn mcu30_module_sheets_trace_back_to_the_intent_nets() {
    let (model, d) = mcu30();
    assert_eq!(traced(&model, &d), drawn_nets(&model));
}

#[test]
fn mcu30_sheets_are_inside_the_frame_and_nothing_overlaps() {
    let (model, d) = mcu30();
    assert_tidy(&model, &d, "mcu30");
}

#[test]
fn every_example_with_modules_traces_back_and_is_tidy() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
    files.extend(std::fs::read_dir(dir.join("ladder")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")));
    files.sort();
    let (mut hierarchical, mut single) = (0, 0);
    for f in files {
        let Ok(model) = serde_yaml::from_str::<ConstraintModel>(&std::fs::read_to_string(&f).unwrap()) else { continue };
        let modules = infer_modules(&model, &[]);
        let d = derive_schematic_modules(&model, &opts()).unwrap_or_else(|e| panic!("{}: {e:?}", f.display()));
        let label = f.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(traced(&model, &d), drawn_nets(&model), "{label}: the sheets do not trace back to the intent's nets");
        assert_tidy(&model, &d, &label);
        if modules.len() >= 2 {
            hierarchical += 1;
            assert_eq!(d.sheet_contents.as_ref().map(|c| c.len()), Some(modules.len()), "{label}");
        } else {
            single += 1;
            assert!(d.sheet_contents.is_none() && d.schematic.as_ref().unwrap().sheets.is_empty(), "{label}: one module stays one sheet");
        }
    }
    assert!(hierarchical >= 3, "only {hierarchical} examples are hierarchical");
    assert!(single >= 2, "only {single} examples are a single sheet");
}

/// The flat derivation still exists (`derive_schematic`): its first row used to sit on the frame line, off the top of the page.
#[test]
fn the_flat_derivation_sits_inside_the_frame_on_a_paper_that_holds_it() {
    for file in ["mcu_board_30plus.yaml", "ldo.yaml", "dense_small_outline.yaml", "ladder/l4_control_hub.yaml"] {
        let model = model_of(file);
        let d = crate::derive_schematic(&model, &opts()).unwrap();
        let sch = d.schematic.as_ref().unwrap();
        let usable = paper_of(sch).usable();
        let rects = drawn(&model, sch);
        assert!(!rects.is_empty(), "{file}");
        for it in &rects {
            assert!(usable.contains(&it.rect), "{file}: {} {:?} is outside the drawing area {:?}", it.what, it.rect, usable);
        }
        for w in &sch.wires {
            for p in &w.pts {
                assert!(usable.contains(&Rect { x0: p.x, y0: p.y, x1: p.x, y1: p.y }), "{file}: wire point {p:?}");
            }
        }
        // The grid is kept: every symbol and every wire point is still a multiple of 1.27 mm.
        assert!(sch.symbols.iter().all(|s| s.at.x % G == 0 && s.at.y % G == 0), "{file}");
    }
}

#[test]
fn derivation_is_deterministic() {
    let (_, a) = mcu30();
    let (_, b) = mcu30();
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
}

#[test]
fn power_symbol_ids_are_unique_across_the_whole_design() {
    let (_, d) = mcu30();
    let mut ids = BTreeSet::new();
    for (_, s) in screens(&d) {
        for p in &s.power_symbols {
            assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
        }
    }
    assert!(ids.iter().any(|i| i.starts_with("#PWR")));
}

#[test]
fn a_part_keeps_its_fields_when_the_sheets_are_derived_again() {
    let model = model_of("mcu_board_30plus.yaml");
    let modules = infer_modules(&model, &[]);
    let mut keep = Keep::default();
    let mut r1 = eda_model::ir::SymbolInstance { id: "R1".into(), at: Point { x: 0, y: 0 }, rot: 0, mirrored: false, mirror_y: false, lib_id: resolve_lib_id(model.part("R1").unwrap()), unit: 1, value: "470".into(), footprint: "Resistor_SMD:R_0603".into(), datasheet: "http://x".into() };
    keep.symbols.insert("R1".into(), r1.clone());
    r1.value = "ignored".into();
    let d = derive_hierarchy(&model, &opts(), &modules, &keep).unwrap();
    let child = d.sheet_contents.as_ref().unwrap().values().find(|s| s.symbols.iter().any(|x| x.id == "R1")).unwrap();
    let s = child.symbols.iter().find(|x| x.id == "R1").unwrap();
    assert_eq!((s.value.as_str(), s.footprint.as_str(), s.datasheet.as_str()), ("470", "Resistor_SMD:R_0603", "http://x"));
}
