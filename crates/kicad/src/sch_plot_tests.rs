//! Tests for `sch_plot.rs` (included there with `#[path]` so they can see its private helpers).

use super::*;
use eda_engine::{derive_schematic, EngineOptions};
use eda_model::ir::{NetLabel, SheetInstance, SheetPin};
use eda_model::{Net, Part, Pin, PinKind};

fn part(reference: &str, value: &str, pins: Vec<Pin>) -> Part {
    Part { reference: reference.into(), mpn: None, lcsc: None, value: Some(value.into()), package: None, footprint: Some("Foo:Bar".into()), pins, body_um: None, symbol: None, datasheet: None, edge: None }
}
fn pin(number: &str, name: &str, kind: PinKind) -> Pin {
    Pin { number: number.into(), name: Some(name.into()), kind }
}

/// U1 (LDO) + two caps, nets with known connectivity.
pub(crate) fn ldo_model() -> ConstraintModel {
    ConstraintModel {
        parts: vec![
            part("U1", "AMS1117", vec![pin("1", "VIN", PinKind::Power), pin("2", "GND", PinKind::Ground), pin("3", "EN", PinKind::Signal), pin("4", "VOUT", PinKind::Power), pin("5", "NC", PinKind::Nc)]),
            part("CIN", "10uF", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]),
            part("COUT", "22uF", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Ground)]),
        ],
        nets: vec![
            Net { name: "VIN".into(), pins: vec!["U1.1".into(), "CIN.1".into(), "U1.3".into()] },
            Net { name: "VOUT".into(), pins: vec!["U1.4".into(), "COUT.1".into()] },
            Net { name: "GND".into(), pins: vec!["U1.2".into(), "CIN.2".into(), "COUT.2".into()] },
        ],
        ..Default::default()
    }
}

/// The studio's own resolution rule, with a plain box standing in for
/// `synthesize_generic_symbol` (pins down the left edge).
pub(crate) fn resolver(model: &ConstraintModel) -> impl Fn(&SymbolInstance) -> LibSymbol + '_ {
    move |s: &SymbolInstance| {
        let real = if s.lib_id.is_empty() || eda_model::is_synthetic_lib_id(&s.lib_id) { None } else { model.symbol_of(&s.lib_id) };
        real.unwrap_or_else(|| {
            let part = model.part(&s.id);
            let pins: Vec<eda_model::LibPin> = part
                .map(|p| {
                    p.pins
                        .iter()
                        .enumerate()
                        .map(|(i, q)| eda_model::LibPin {
                            number: q.number.clone(),
                            name: q.name.clone().unwrap_or_default(),
                            electrical_type: "passive".into(),
                            shape: "line".into(),
                            at: SPoint::new(0.0, -(i as f64 + 1.0) * 2.54),
                            angle_deg: 0.0,
                            length_mm: 2.54,
                            unit: 1,
                        })
                        .collect()
                })
                .unwrap_or_default();
            LibSymbol {
                lib_id: format!("eda:{}", s.id),
                graphics: vec![SymbolGraphic::Rectangle { unit: 1, start: SPoint::new(2.54, 0.0), end: SPoint::new(12.7, -10.16), stroke_mm: 0.254, filled: false }],
                pins,
                power: false,
                in_bom: true,
                on_board: true,
                datasheet: String::new(),
                description: String::new(),
                reference_prefix: "U".into(),
                unit_count: 1,
            }
        })
    }
}

fn meta() -> PlotMeta {
    PlotMeta { iso_date: "2026-10-02T12:34:56".into(), tool: "agentic-eda 0.1".into(), project: "ldo".into(), fallback_title: "LDO test".into(), fallback_date: "2026-10-02".into() }
}

/// Single sheet: derived schematic of the LDO model.
pub(crate) fn single() -> (Design, ConstraintModel) {
    let model = ldo_model();
    let design = derive_schematic(&model, &EngineOptions::new(1, "plot")).unwrap();
    (design, model)
}

/// Two sheets: a root with one placement of `child`, which holds R1.
pub(crate) fn hierarchical() -> (Design, ConstraintModel) {
    let model = ConstraintModel { parts: vec![part("R1", "10k", vec![pin("1", "1", PinKind::Passive), pin("2", "2", PinKind::Passive)])], nets: vec![Net { name: "SIG".into(), pins: vec!["R1.1".into()] }], ..Default::default() };
    let mut child_design = derive_schematic(&model, &EngineOptions::new(1, "plot_child")).unwrap();
    let child_sch = child_design.schematic.as_mut().unwrap();
    let r1_at = child_sch.symbols[0].at;
    child_sch.labels.push(NetLabel { id: String::new(), net: "SIG".into(), at: Point { x: r1_at.x, y: r1_at.y - 3810 }, kind: LabelKind::Hierarchical { shape: eda_model::ir::LabelShape::Passive } });
    let root = SchematicSection {
        symbols: vec![],
        wires: vec![],
        labels: vec![],
        texts: vec![],
        power_symbols: vec![],
        no_connects: vec![],
        bus_entries: vec![],
        erc_exclusions: vec![],
        erc_pin_map: None,
        user_fields: Default::default(),
        title_block: None,
        sheets: vec![SheetInstance {
            id: "sheet1".into(),
            name: "power".into(),
            file: "child.kicad_sch".into(),
            at: Point { x: 20_000, y: 20_000 },
            size: (30_000, 20_000),
            pins: vec![SheetPin { id: String::new(), name: "SIG".into(), shape: eda_model::ir::LabelShape::Passive, at: Point { x: 20_000, y: 30_000 } }],
        }],
        instance_overrides: vec![], junctions: vec![], lines: vec![],
        imported_from_kicad: false,
    };
    let mut screens = BTreeMap::new();
    screens.insert("child.kicad_sch".to_string(), child_sch.clone());
    (Design { sheet_contents: Some(screens), schematic: Some(root), ..child_design }, model)
}

/// A tag-balance check good enough for SVG/XML written by these plotters.
pub(crate) fn well_formed(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let end = s[i..].find('>').ok_or("unterminated tag")? + i;
        let tag = &s[i + 1..end];
        if tag.starts_with('?') || tag.starts_with('!') {
            // prolog / doctype
        } else if let Some(name) = tag.strip_prefix('/') {
            let open = stack.pop().ok_or_else(|| format!("stray </{name}>"))?;
            if open != name.trim() {
                return Err(format!("</{name}> closes <{open}>"));
            }
        } else if !tag.ends_with('/') {
            let name = tag.split_whitespace().next().unwrap_or("").to_string();
            stack.push(name);
        }
        i = end + 1;
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(format!("unclosed: {stack:?}"))
    }
}

#[test]
fn svg_is_well_formed_and_has_the_expected_elements() {
    let (design, model) = single();
    let r = resolver(&model);
    let files = plot_schematic(&design, &model, PlotFormat::Svg, &SchPlotOpts::default(), &meta(), &r).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, "ldo.svg");
    let svg = String::from_utf8(files[0].bytes.clone()).unwrap();
    well_formed(&svg).unwrap_or_else(|e| panic!("{e}\n{svg}"));
    assert!(svg.starts_with("<?xml version=\"1.0\" standalone=\"no\"?>"));
    assert!(svg.contains("width=\"297.0000mm\" height=\"210.0000mm\" viewBox=\"0.0000 0.0000 297.0000 210.0000\""));
    assert!(svg.contains("<desc>Image generated by Eeschema-SVG </desc>"));
    // page background rect (LAYER_SCHEMATIC_BACKGROUND), drawing sheet frame and title block text
    assert!(svg.contains("<rect x=\"0.000000\" y=\"0.000000\" width=\"297.000000\" height=\"210.000000\""));
    assert!(svg.contains(">Title: LDO test</text>"));
    assert!(svg.contains(">Sheet: /</text>"));
    assert!(svg.contains(">Id: 1/1</text>"));
    assert!(svg.contains(">File: ldo.kicad_sch</text>"));
    // symbols: reference + value text as hidden <text> plus stroked glyph groups
    assert!(svg.contains(">U1</text>") && svg.contains(">CIN</text>"));
    assert!(svg.contains("<g class=\"stroked-text\"><desc>AMS1117</desc>"));
    // wires in LAYER_WIRE green, the symbol body in LAYER_DEVICE
    assert!(svg.contains("stroke:#009600"), "wire colour");
    assert!(svg.contains("stroke:#840000"), "device colour");
    // KiCad's per-segment path form
    assert!(svg.contains("<path d=\"M"));
}

#[test]
fn svg_black_and_white_drops_colour_and_background() {
    let (design, model) = single();
    let r = resolver(&model);
    let opts = SchPlotOpts { black_and_white: true, ..Default::default() };
    let files = plot_schematic(&design, &model, PlotFormat::Svg, &opts, &meta(), &r).unwrap();
    let svg = String::from_utf8(files[0].bytes.clone()).unwrap();
    well_formed(&svg).unwrap();
    assert!(!svg.contains("stroke:#009600") && !svg.contains("stroke:#840000"));
    assert!(!svg.contains("<rect x=\"0.000000\" y=\"0.000000\" width=\"297.000000\""), "no background rect in B&W");
    assert!(svg.contains("stroke:#000000"));
}

#[test]
fn plot_drawing_sheet_option_removes_the_frame() {
    let (design, model) = single();
    let r = resolver(&model);
    let opts = SchPlotOpts { plot_drawing_sheet: false, ..Default::default() };
    let svg = String::from_utf8(plot_schematic(&design, &model, PlotFormat::Svg, &opts, &meta(), &r).unwrap().remove(0).bytes).unwrap();
    assert!(!svg.contains("Title: LDO test"));
    let with = String::from_utf8(plot_schematic(&design, &model, PlotFormat::Svg, &SchPlotOpts::default(), &meta(), &r).unwrap().remove(0).bytes).unwrap();
    assert!(with.len() > svg.len());
}

#[test]
fn hierarchy_gives_one_svg_per_sheet_with_kicad_names() {
    let (design, model) = hierarchical();
    let r = resolver(&model);
    let files = plot_schematic(&design, &model, PlotFormat::Svg, &SchPlotOpts::default(), &meta(), &r).unwrap();
    assert_eq!(files.iter().map(|f| f.filename.as_str()).collect::<Vec<_>>(), ["ldo.svg", "ldo-power.svg"]);
    let child = String::from_utf8(files[1].bytes.clone()).unwrap();
    assert!(child.contains(">Sheet: /power/</text>"), "{child}");
    assert!(child.contains(">Id: 2/2</text>"));
    // the root draws the sheet box and its pin
    let root = String::from_utf8(files[0].bytes.clone()).unwrap();
    assert!(root.contains("<desc>power</desc>") && root.contains("<desc>SIG</desc>"));
    // plot_all = false plots only the chosen sheet
    let opts = SchPlotOpts { plot_all: false, current_sheet: vec!["sheet1".into()], ..Default::default() };
    let one = plot_schematic(&design, &model, PlotFormat::Svg, &opts, &meta(), &r).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].filename, "ldo-power.svg");
}

/// Parse the PDF back: header, `startxref`, xref table, and that every
/// entry points at its own `N 0 obj`.
pub(crate) fn check_pdf(bytes: &[u8]) -> (usize, usize) {
    assert!(bytes.starts_with(b"%PDF-1.5\n%\x80\x81\x82\x83\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(bytes).to_string();
    let sx = text.rfind("startxref\n").expect("startxref");
    let off: usize = text[sx + 10..].lines().next().unwrap().parse().unwrap();
    assert_eq!(&bytes[off..off + 5], b"xref\n", "startxref points at the xref table");
    let header = bytes[off + 5..].split(|b| *b == b'\n').next().unwrap();
    let n: usize = std::str::from_utf8(header).unwrap().strip_prefix("0 ").unwrap().parse().unwrap();
    // Entries are fixed-width (20 bytes each, "\n"-terminated): read them by offset.
    let table_start = off + 5 + header.len() + 1;
    assert_eq!(&bytes[table_start..table_start + 20], b"0000000000 65535 f \n");
    for i in 1..n {
        let e = &bytes[table_start + 20 * i..table_start + 20 * (i + 1)];
        let entry = std::str::from_utf8(e).unwrap();
        assert_eq!(entry.len(), 20);
        assert!(entry.ends_with(" 00000 n \n"), "entry {i}: {entry:?}");
        let o: usize = entry[..10].parse().unwrap();
        let want = format!("{i} 0 obj\n");
        assert_eq!(&bytes[o..o + want.len()], want.as_bytes(), "object {i} is at its xref offset");
    }
    assert!(text[sx..].contains(&format!("/Size {n} ")) || text[off..].contains(&format!("/Size {n} ")));
    let kids = text.matches("/Type /Page\n").count();
    let count: usize = text.split("/Type /Pages\n/Kids [\n").nth(1).unwrap().split("/Count ").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
    (kids, count)
}

#[test]
fn pdf_has_valid_xref_and_one_page_for_one_sheet() {
    let (design, model) = single();
    let r = resolver(&model);
    let files = plot_schematic(&design, &model, PlotFormat::Pdf, &SchPlotOpts::default(), &meta(), &r).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, "ldo.pdf");
    let (kids, count) = check_pdf(&files[0].bytes);
    assert_eq!((kids, count), (1, 1));
    let text = String::from_utf8_lossy(&files[0].bytes);
    assert!(text.contains("/MediaBox [0 0 841.89 595.276]"), "A4 landscape in points");
    assert!(text.contains("0.0072 0 0 0.0072 0 0 cm 1 J 1 j"));
    assert!(text.contains("/Producer (KiCad PDF)") && text.contains("/Creator (Eeschema-PDF)") && text.contains("/Title (LDO test)"));
    assert!(text.contains("/Type /Catalog"));
}

#[test]
fn pdf_has_one_page_per_sheet_with_bookmarks() {
    let (design, model) = hierarchical();
    let r = resolver(&model);
    let files = plot_schematic(&design, &model, PlotFormat::Pdf, &SchPlotOpts::default(), &meta(), &r).unwrap();
    let (kids, count) = check_pdf(&files[0].bytes);
    assert_eq!((kids, count), (2, 2));
    let text = String::from_utf8_lossy(&files[0].bytes);
    // outline: the root page, and the child's bookmark parented under it (`/Count -1` on the root node)
    assert!(text.contains("/Count -1\n/First"), "child bookmark nested under the root page");
    assert!(text.contains("/Title (Page 1)") && text.contains("/Title (power \\(Page 2\\))"));
}

#[test]
fn page_size_a_scales_the_viewport() {
    let (design, model) = single();
    let r = resolver(&model);
    let opts = SchPlotOpts { page_size_select: PageSizeSelect::A, ..Default::default() };
    let svg = String::from_utf8(plot_schematic(&design, &model, PlotFormat::Svg, &opts, &meta(), &r).unwrap().remove(0).bytes).unwrap();
    // US "A" is 11 x 8.5 in = 279.4 x 215.9 mm
    assert!(svg.contains("width=\"279.4000mm\" height=\"215.9000mm\""), "{}", &svg[..400]);
}

#[test]
fn junctions_are_derived_from_wire_geometry() {
    let (design, _) = single();
    let sch = design.schematic.unwrap();
    // A T: one wire ends on the interior of another.
    let mut sch2 = sch.clone();
    sch2.wires = vec![
        eda_model::ir::Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }], bus: false },
        eda_model::ir::Wire { id: String::new(), net: "N".into(), pins: vec![], pts: vec![Point { x: 5_000, y: 0 }, Point { x: 5_000, y: 5_000 }], bus: false },
    ];
    sch2.labels.clear();
    sch2.power_symbols.clear();
    assert_eq!(junction_points(&sch2), vec![Point { x: 5_000, y: 0 }]);
}

#[test]
fn drawing_sheet_default_description_parses_and_lays_out() {
    let m = parse_drawing_sheet(DEFAULT_DRAWING_SHEET);
    assert_eq!((m.left, m.right, m.top, m.bottom), (10.0, 10.0, 10.0, 10.0));
    assert_eq!(m.items.iter().filter(|i| i.kind == DsKind::Text).count(), 17);
    assert_eq!(increment_label("A", 2), "C");
    assert_eq!(increment_label("1", 5), "6");
    // A4: 6 columns, 4 rows of zone references (the 30/100 repeats clip to the page).
    let v = DsVars { kicad_version: "v".into(), page: "1".into(), count: 1, sheet_path: "/".into(), file_name: "f".into(), paper: "A4".into(), title: "T".into(), company: "C".into(), rev: "2".into(), date: "d".into(), comments: vec![] };
    let mut p = SvgPlotter::new("d");
    p.set_color_mode(true);
    p.set_viewport(Pt::default(), IUS_PER_DECIMIL, 1.0, false);
    p.start_plot("1");
    plot_drawing_sheet(&mut p, &PageInfo::A4, &v, Color::BLACK);
    let svg = p.end_plot();
    for label in ["<desc>1</desc>", "<desc>6</desc>", "<desc>A</desc>", "<desc>D</desc>"] {
        assert!(svg.contains(label), "{label}");
    }
    assert!(!svg.contains("<desc>7</desc>") && !svg.contains("<desc>E</desc>"), "labels beyond the page are clipped");
    assert!(svg.contains("<desc>Rev: 2</desc>") && svg.contains("<desc>Title: T</desc>"));
}
