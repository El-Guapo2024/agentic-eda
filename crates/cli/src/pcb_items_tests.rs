//! The board items of GAPS.md item 24 (package P16) through a board directory: a text box is one undo step of the board and reaches the derived `.kicad_pcb`, and (the slow tier,
//! `EDA_SLOW_TESTS=1`) kicad-cli loads what this studio writes and plots it as it plots the same board after its own rewrite of the file.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_model::ir::Point;
use eda_model::pcb_items::TextBox;
use eda_ops::{pcb_items::PcbItemCmd, Cmd, Domain};
use std::path::Path;

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

fn add(dir: &Path, t: TextBox) {
    step(dir, Cmd::PcbItem(PcbItemCmd::AddTextBox { text_box: t }), false, "test").unwrap();
}

fn boxes(dir: &Path) -> Vec<TextBox> {
    load(dir).unwrap().1.drawings.map(|d| d.board_items.text_boxes).unwrap_or_default()
}

/// A text box as the Draw Text Box tool and its dialog make one on the silkscreen: three lines of text in a box 20 mm by 8 mm, a dashed border, margins of its own.
fn notes_box() -> TextBox {
    let mut t = TextBox::new("F.SilkS", Point { x: 2_000, y: 2_000 }, Point { x: 22_000, y: 10_000 }, 1_000, 120);
    t.text = "REV A\nBuilt by the studio, wrapped to the width of its box when the line is long".into();
    t.line_style = eda_model::pcb_items::LineStyle::Dash;
    t.margin_left_um = 900;
    t.margin_right_um = 1_100;
    t
}

/// A knockout box: the box is copper-free silkscreen ink with the text cut out of it, turned a quarter, mirrored on the back.
fn knockout_box() -> TextBox {
    let mut t = TextBox::new("B.SilkS", Point { x: 2_000, y: 14_000 }, Point { x: 18_000, y: 20_000 }, 1_200, 150);
    t.text = "TOP SECRET".into();
    t.knockout = true;
    t.border = false;
    t.halign = eda_model::ir::TextJustify::Center;
    t.valign = eda_model::ir::TextVAlign::Center;
    t.bold = true;
    t
}

#[test]
fn a_text_box_is_one_undo_step_of_the_board_and_survives_the_project_file() {
    let dir = scratch("p16_text_box");
    setup(&dir);
    assert!(boxes(&dir).is_empty());
    add(&dir, notes_box());
    let shown = boxes(&dir);
    assert_eq!(shown.len(), 1);
    assert!(shown[0].id.starts_with("tbx_"));
    // The project file is design.json: a fresh load has the box with everything the dialog set.
    let design: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap();
    assert_eq!(design["drawings"]["board_items"]["text_boxes"][0]["line_style"], "dash", "{design}");
    assert_eq!((shown[0].margin_left_um, shown[0].margin_right_um), (900, 1_100));

    // Move, edit and delete are verbs of the same history.
    let id = shown[0].id.clone();
    step(&dir, Cmd::MoveItems { ids: vec![id.clone()], dx: 1_000, dy: 500 }, false, "test").unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 3_000, y: 2_500 });
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 2_000, y: 2_000 }, "one undo takes the move back");
    let mut edited = boxes(&dir)[0].clone();
    edited.text = "REV B".into();
    step(&dir, Cmd::PcbItem(PcbItemCmd::EditTextBox { id: id.clone(), text_box: edited }), false, "test").unwrap();
    assert_eq!(boxes(&dir)[0].text, "REV B");
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteTextBox { id }), false, "test").unwrap();
    assert!(boxes(&dir).is_empty());
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(boxes(&dir).len(), 1, "undo brings the deleted box back");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(boxes(&dir).is_empty(), "and the whole history of the box goes back to nothing");
}

/// kicad-cli's `pcb export svg` of `layers` from the board file `board`, the same plot whoever wrote the file.
fn plot(cli: &Path, board: &Path, layers: &str, out: &Path) -> String {
    let done = eda_kicad_engine::cli_command(cli, None)
        .args(["pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet", "--page-size-mode", "2", "--layers", layers, "-o"])
        .arg(out)
        .arg(board)
        .output()
        .expect("kicad-cli runs");
    assert!(done.status.success() && out.exists(), "kicad-cli did not plot {}: {}{}", board.display(), String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
    std::fs::read_to_string(out).unwrap()
}

/// What the plot draws, without the lines that say when and by whom.
fn body(svg: &str) -> String {
    svg.lines().filter(|l| !l.contains("<title>") && !l.contains("<!--") && !l.contains("<desc>") && !l.contains("Created by") && !l.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// The text boxes this studio writes load in kicad-cli and plot as KiCad plots them: the plot of the derived board file and the plot of kicad-cli's own rewrite of it
/// (`pcb upgrade --force`, which reads the file with `PCB_IO_KICAD_SEXPR_PARSER` and writes it with `PCB_IO_KICAD_SEXPR`) are one and the same, so the parser took the box
/// as the writer meant it -- the text wrapped to the margins, the dashed border, the knockout with its text cut out. The rewrite still has both boxes, with the margins.
#[test]
fn kicad_cli_loads_the_text_boxes_this_studio_writes_and_plots_them_as_its_own_rewrite() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_text_box_cli");
    setup(&dir);
    add(&dir, notes_box());
    add(&dir, knockout_box());
    // The studio's own plot derives the board file (`.kicad/*.kicad_pcb`).
    let args: Vec<String> = ["--layers", "F.SilkS,B.SilkS"].iter().map(|s| s.to_string()).collect();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    let text = std::fs::read_to_string(&derived).unwrap();
    assert_eq!(text.matches("(gr_text_box").count(), 2, "{text}");

    let (ours, theirs) = (dir.join("ours.svg"), dir.join("theirs.svg"));
    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(upgraded.matches("(gr_text_box").count(), 2, "kicad-cli kept both boxes: {upgraded}");
    assert!(upgraded.contains("(margins 0.9 0.81 1.1 0.81)") || upgraded.contains("(margins 0.9 0.8100 1.1 0.8100)") || upgraded.contains("(margins 0.9 0.81"), "the margins: {upgraded}");

    // KiCad plots F.SilkS: the box's border (dashed: many short strokes) and the glyphs of the wrapped text; B.SilkS: the knockout
    let (a_front, b_front) = (plot(&cli, &derived, "F.SilkS", &ours), plot(&cli, &copy, "F.SilkS", &theirs));
    assert!(a_front.len() > 2_000, "a real plot: {} bytes", a_front.len());
    assert_eq!(body(&a_front), body(&b_front), "the same plot before and after kicad-cli's rewrite of the file");
    let (a_back, b_back) = (plot(&cli, &derived, "B.SilkS", &ours), plot(&cli, &copy, "B.SilkS", &theirs));
    assert_eq!(body(&a_back), body(&b_back), "the knockout plots the same too");
    assert_ne!(body(&a_front), body(&a_back), "the two layers plot different things");

    // Take the box away and the plot is empty of it: the strokes are the box's.
    let id = boxes(&dir)[0].id.clone();
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteTextBox { id }), false, "test").unwrap();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let without = plot(&cli, &derived, "F.SilkS", &dir.join("without.svg"));
    assert!(body(&without).len() + 500 < body(&a_front).len(), "the box is most of the plot: {} against {}", body(&without).len(), body(&a_front).len());
}

// ----------------------------------------------------------------------------------------------------------------------------- reference images

use eda_model::image_info::{base64_encode, solid_png};
use eda_model::pcb_items::RefImage;

fn state(dir: &Path) -> serde_json::Value {
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    crate::studio::state(dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap()
}

fn ref_images(dir: &Path) -> Vec<RefImage> {
    load(dir).unwrap().1.drawings.map(|d| d.board_items.images).unwrap_or_default()
}

/// The pixels of the image the tests place: 120 x 80 pixels at 254 pixels per inch, red.
fn red_png() -> Vec<u8> {
    solid_png(120, 80, [200, 30, 30], 254)
}

fn add_image(dir: &Path, layer: &str, at: Point, scale_ppm: i64) -> String {
    let mut image = RefImage::new(layer, at, &base64_encode(&red_png()));
    image.scale_ppm = scale_ppm;
    step(dir, Cmd::PcbItem(PcbItemCmd::AddRefImage { image }), false, "test").unwrap();
    ref_images(dir).last().unwrap().id.clone()
}

/// A reference image is one undo step of the board; the state describes it (place, size, pixels, definition, hash) without its pixels; `GET /api/pcb_image` gives the file;
/// the derived KiCad board has it as `(image ..)` with the data in lines of 76 characters.
#[test]
fn a_reference_image_is_one_undo_step_described_by_the_state_served_by_the_route_and_written_to_the_board() {
    let dir = scratch("p16_ref_image");
    setup(&dir);
    assert!(ref_images(&dir).is_empty());
    let id = add_image(&dir, "Cmts.User", Point { x: 30_000, y: 20_000 }, 500_000);
    assert!(id.starts_with("img_"));

    let s = state(&dir);
    let shown = &s["drawings"]["images"][0];
    assert_eq!((shown["id"].as_str(), shown["layer"].as_str(), shown["x"].as_i64(), shown["y"].as_i64()), (Some(id.as_str()), Some("Cmts.User"), Some(30_000), Some(20_000)));
    // 120 x 80 px at 254 ppi are 12 x 8 mm; at half scale 6 x 4
    assert_eq!((shown["w"].as_i64(), shown["h"].as_i64(), shown["scale_ppm"].as_i64(), shown["ppi"].as_u64()), (Some(6_000), Some(4_000), Some(500_000), Some(254)));
    assert_eq!(shown["px"], serde_json::json!([120, 80]));
    assert!(shown.get("data").is_none() && !s.to_string().contains(&base64_encode(&red_png())[..64]), "the pixels are not in the state");
    let hash = shown["hash"].as_str().unwrap().to_string();

    let reply = crate::image_api::pcb_image(&dir, &format!("/api/pcb_image?id={id}&h={hash}"));
    assert_eq!((reply.status, reply.kind, reply.body), ("200 OK", "image/png", red_png()));
    assert_eq!(crate::image_api::pcb_image(&dir, "/api/pcb_image?id=nope").status, "404 Not Found");

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-20", title: "t" }).unwrap();
    assert!(text.contains("(image\n\t\t(at 30 20) (layer \"Cmts.User\") (scale 0.5)"), "{text}");
    let data_lines = text.lines().filter(|l| l.trim_start().starts_with("\"iVBOR") || (l.trim_start().starts_with('"') && l.trim_end().ends_with('"') && l.len() > 70)).count();
    assert!(data_lines >= 2, "the data is in lines: {text}");

    // Edit and undo: the image comes back as it was; one undo per step.
    step(&dir, Cmd::PcbItem(PcbItemCmd::EditRefImage { id: id.clone(), layer: "Dwgs.User".into(), at: Point { x: 0, y: 0 }, scale_ppm: 2_000_000 }), false, "test").unwrap();
    assert_eq!((ref_images(&dir)[0].layer.as_str(), ref_images(&dir)[0].scale_ppm), ("Dwgs.User", 2_000_000));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!((ref_images(&dir)[0].layer.as_str(), ref_images(&dir)[0].scale_ppm), ("Cmts.User", 500_000));
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteRefImage { id: id.clone() }), false, "test").unwrap();
    assert!(ref_images(&dir).is_empty());
    assert_eq!(crate::image_api::pcb_image(&dir, &format!("/api/pcb_image?id={id}")).status, "404 Not Found");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(ref_images(&dir).len(), 1, "undo brings the deleted image back with its pixels");
    assert_eq!(ref_images(&dir)[0].data, base64_encode(&red_png()));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(ref_images(&dir).is_empty(), "and the whole history of the image goes back to nothing");
}

/// kicad-cli loads a board with the image this studio writes, keeps its pixels byte for byte in its own rewrite (`pcb upgrade --force`, which writes `m_imageData` as it read it),
/// takes the place and the scale as written, and plots nothing of it (a reference image is kept out of every plot).
#[test]
fn kicad_cli_loads_the_reference_image_this_studio_writes_keeps_its_pixels_and_does_not_plot_it() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_ref_image_cli");
    setup(&dir);
    add_image(&dir, "Cmts.User", Point { x: 30_000, y: 20_000 }, 500_000);
    add_image(&dir, "F.SilkS", Point { x: 60_000, y: 40_000 }, 1_000_000);
    let args: Vec<String> = ["--layers", "Cmts.User"].iter().map(|s| s.to_string()).collect();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    let text = std::fs::read_to_string(&derived).unwrap();
    assert_eq!(text.matches("(image").count(), 2, "{text}");

    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(upgraded.matches("(image").count(), 2, "kicad-cli kept both images: {upgraded}");
    let (back, _, notes) = eda_kicad::import_kicad_pcb(&upgraded).unwrap();
    assert!(!notes.unread_items.contains_key("image"));
    let mut images = back.drawings.unwrap().board_items.images;
    images.sort_by_key(|i| i.at.x);
    assert_eq!(images.len(), 2);
    let (first, second) = (&images[0], &images[1]);
    assert_eq!((first.layer.as_str(), first.at, first.scale_ppm), ("Cmts.User", Point { x: 30_000, y: 20_000 }, 500_000), "kicad-cli read the place, the layer and the scale as written");
    assert_eq!((second.layer.as_str(), second.at, second.scale_ppm), ("F.SilkS", Point { x: 60_000, y: 40_000 }, 1_000_000));
    assert_eq!(first.data, base64_encode(&red_png()), "the pixels are the file this studio embedded, byte for byte");
    assert_eq!(second.data, first.data);

    // Reference images are kept out of every plot.
    let svg = plot(&cli, &derived, "Cmts.User,F.SilkS", &dir.join("images.svg"));
    assert!(!svg.contains("<image") && !svg.contains("data:image"), "a reference image is not plotted");
}

// ----------------------------------------------------------------------------------------------------------------------------- the schematic's image

use eda_model::sch_extras::SchGraphicKind;
use eda_ops::sch_edit::SchCmd;

fn sch_images(dir: &Path) -> Vec<(String, Point, i64, String)> {
    let (_, design, _) = load(dir).unwrap();
    design
        .schematic
        .map(|s| {
            s.extras
                .graphics
                .iter()
                .filter_map(|g| match &g.shape {
                    SchGraphicKind::Image { at, scale_ppm, data } => Some((g.id.clone(), *at, *scale_ppm, data.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn add_sch_image(dir: &Path, at: Point, scale_ppm: i64) {
    let graphic = eda_model::sch_extras::SchGraphic::new(SchGraphicKind::Image { at, scale_ppm, data: base64_encode(&red_png()) });
    step(dir, Cmd::sch(SchCmd::AddGraphic { graphic }), false, "test").unwrap();
}

/// The schematic's image is a graphic of the sheet: one undo step, described by the state without its pixels, served by `GET /api/sch_image`, written as `(image ..)` and read
/// back by the importer.
#[test]
fn a_schematic_image_is_one_undo_step_described_by_the_state_served_by_the_route_and_written_to_the_sheet() {
    let dir = scratch("p16_sch_image");
    super::tests::setup_schematic(&dir);
    assert!(sch_images(&dir).is_empty());
    add_sch_image(&dir, Point { x: 50_800, y: 38_100 }, 1_000_000);
    let images = sch_images(&dir);
    assert_eq!(images.len(), 1);
    let id = images[0].0.clone();
    assert!(id.starts_with("img_"), "{id}");

    let j = crate::studio::schematic_json(&dir, "", crate::text_vars_json::clock_of("/api/schematic")).unwrap();
    let shape = j["graphics"].as_array().unwrap().iter().find(|g| g["id"] == id.as_str()).unwrap()["shape"].clone();
    assert_eq!((shape["type"].as_str(), shape["px"].clone(), shape["ppi"].as_u64()), (Some("image"), serde_json::json!([120, 80]), Some(254)));
    assert!(shape.get("data").is_none() && !j.to_string().contains(&base64_encode(&red_png())[..64]), "the pixels are not in the state");

    let reply = crate::image_api::sch_image(&dir, &format!("/api/sch_image?id={id}&h={}", shape["hash"].as_str().unwrap()));
    assert_eq!((reply.status, reply.kind, reply.body), ("200 OK", "image/png", red_png()));
    assert_eq!(crate::image_api::sch_image(&dir, "/api/sch_image?id=nope").status, "404 Not Found");

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_sch(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-20", title: "t" }).unwrap();
    assert!(text.contains("(image\n\t\t(at 50.8 38.1)\n\t\t(uuid"), "{text}");
    let (back, _, _) = eda_kicad::import_kicad_sch(&text).unwrap();
    let read = back.schematic.unwrap().extras.graphics.into_iter().find_map(|g| if let SchGraphicKind::Image { at, scale_ppm, data } = g.shape { Some((at, scale_ppm, data)) } else { None }).expect("the image reads back");
    assert_eq!(read, (Point { x: 50_800, y: 38_100 }, 1_000_000, base64_encode(&red_png())));

    // an edit that sends no pixels keeps them; one undo takes the edit back
    let edit = eda_model::sch_extras::SchGraphic::new(SchGraphicKind::Image { at: Point { x: 0, y: 0 }, scale_ppm: 2_000_000, data: String::new() });
    step(&dir, Cmd::sch(SchCmd::EditGraphic { id: id.clone(), graphic: edit }), false, "test").unwrap();
    assert_eq!((sch_images(&dir)[0].1, sch_images(&dir)[0].2, sch_images(&dir)[0].3.clone()), (Point { x: 0, y: 0 }, 2_000_000, base64_encode(&red_png())));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!((sch_images(&dir)[0].1, sch_images(&dir)[0].2), (Point { x: 50_800, y: 38_100 }, 1_000_000));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(sch_images(&dir).is_empty());
}

/// kicad-cli loads the sheet with the image this studio writes and plots it: the SVG has an `<image>` at the place and of the size the studio computes -- centred on the point,
/// 6 mm x 4 mm for a 120 x 80 pixel image of 254 pixels per inch at half scale (the plot encodes the pixels again, so its PNG is not the file's bytes; the size is what must agree).
#[test]
fn kicad_cli_plots_the_schematic_image_this_studio_writes() {
    if !slow() {
        return;
    }
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("kicad-cli not found; skipping");
        return;
    }
    let dir = scratch("p16_sch_image_cli");
    super::tests::setup_schematic(&dir);
    add_sch_image(&dir, Point { x: 100_000, y: 80_000 }, 500_000);
    let out = crate::kicad_engine::export_sch(&dir, "svg", &[]).expect("kicad-cli plots the schematic");
    let file = out["files"].as_array().and_then(|f| f.first()).and_then(|f| f.as_str()).unwrap_or_else(|| panic!("{out}")).to_string();
    let svg = std::fs::read_to_string(dir.join(&file)).unwrap();
    let at = svg.find("<image").expect("the plot has the image");
    let end = svg[at..].find("/>").map(|e| at + e).unwrap_or(svg.len());
    let tag = &svg[at..end];
    let attr = |name: &str| -> f64 {
        let key = format!("{name}=\"");
        let from = tag.find(&key).unwrap_or_else(|| panic!("{name} in {tag:.200}")) + key.len();
        tag[from..].split('"').next().unwrap().parse().unwrap()
    };
    assert!(tag.contains("data:image/png;base64,"));
    assert_eq!((attr("x"), attr("y"), attr("width"), attr("height")), (97.0, 78.0, 6.0, 4.0), "{tag:.200}");
    assert_eq!(svg.matches("<image").count(), 1);
}

// ----------------------------------------------------------------------------------------------------------------------------- points

use eda_model::pcb_items::PcbPoint;

fn points(dir: &Path) -> Vec<PcbPoint> {
    load(dir).unwrap().1.drawings.map(|d| d.board_items.points).unwrap_or_default()
}

fn add_point(dir: &Path, layer: &str, at: Point, size_um: i64) {
    let mut point = PcbPoint::new(layer, at);
    point.size_um = size_um;
    step(dir, Cmd::PcbItem(PcbItemCmd::AddPoint { point }), false, "test").unwrap();
}

/// A point is one undo step of the board; the state describes it; the derived board has `(point ..)` as `format( const PCB_POINT* )` writes it.
#[test]
fn a_point_is_one_undo_step_described_by_the_state_and_written_to_the_board() {
    let dir = scratch("p16_point");
    setup(&dir);
    assert!(points(&dir).is_empty());
    add_point(&dir, "F.SilkS", Point { x: 30_000, y: 20_000 }, 2_000);
    let id = points(&dir)[0].id.clone();
    assert!(id.starts_with("pnt_"));
    let s = state(&dir);
    let shown = &s["drawings"]["points"][0];
    assert_eq!((shown["id"].as_str(), shown["layer"].as_str(), shown["x"].as_i64(), shown["y"].as_i64(), shown["size"].as_i64()), (Some(id.as_str()), Some("F.SilkS"), Some(30_000), Some(20_000), Some(2_000)));

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "t" }).unwrap();
    assert!(text.contains("(point (at 30 20) (size 2) (layer \"F.SilkS\") (uuid \""), "{text}");
    assert!(!text.contains("(point (at 30 20) (size 2) (layer \"F.SilkS\") (locked"), "no lock in the file form");
    let (back, _, notes) = eda_kicad::import_kicad_pcb(&text).unwrap();
    assert!(!notes.unread_items.contains_key("point"), "the importer reads it");
    let read: Vec<_> = back.drawings.unwrap().board_items.points.into_iter().map(|p| (p.layer, p.at, p.size_um)).collect();
    assert_eq!(read, vec![("F.SilkS".to_string(), Point { x: 30_000, y: 20_000 }, 2_000)]);

    step(&dir, Cmd::PcbItem(PcbItemCmd::EditPoint { id: id.clone(), layer: "B.SilkS".into(), at: Point { x: 0, y: 0 }, size_um: 3_000 }), false, "test").unwrap();
    assert_eq!((points(&dir)[0].layer.as_str(), points(&dir)[0].size_um), ("B.SilkS", 3_000));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!((points(&dir)[0].layer.as_str(), points(&dir)[0].size_um), ("F.SilkS", 2_000), "one undo takes the edit back");
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeletePoint { id }), false, "test").unwrap();
    assert!(points(&dir).is_empty());
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(points(&dir).len(), 1, "undo brings it back");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(points(&dir).is_empty());
}

/// kicad-cli loads the board with its points, its `pcb upgrade --force` writes them back with the place, size and layer as written, and a point is in no plot (the SVG of the
/// layer is the same with and without it).
#[test]
fn kicad_cli_loads_the_points_this_studio_writes_keeps_them_and_does_not_plot_them() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_point_cli");
    setup(&dir);
    let args: Vec<String> = ["--layers", "F.SilkS"].iter().map(|s| s.to_string()).collect();
    let derived_of = |dir: &Path| std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let without = body(&plot(&cli, &derived_of(&dir), "F.SilkS", &dir.join("without.svg")));
    // inside the board's own extent: the plot's page is the board's extent, points included
    add_point(&dir, "F.SilkS", Point { x: 10_000, y: 12_000 }, 2_000);
    add_point(&dir, "B.Cu", Point { x: 5_000, y: 6_000 }, 1_000);
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = derived_of(&dir);
    let text = std::fs::read_to_string(&derived).unwrap();
    assert_eq!(text.matches("(point (at").count(), 2, "{text}");

    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(upgraded.matches("(point").count(), 2, "kicad-cli kept both points: {upgraded}");
    let (back, _, _) = eda_kicad::import_kicad_pcb(&upgraded).unwrap();
    let mut read: Vec<_> = back.drawings.unwrap().board_items.points.into_iter().map(|p| (p.layer, p.at, p.size_um)).collect();
    read.sort_by_key(|p| p.1.x);
    assert_eq!(read, vec![("B.Cu".to_string(), Point { x: 5_000, y: 6_000 }, 1_000), ("F.SilkS".to_string(), Point { x: 10_000, y: 12_000 }, 2_000)], "kicad-cli read the place, size and layer as written");
    let with = body(&plot(&cli, &derived, "F.SilkS", &dir.join("with.svg")));
    assert_eq!(with, without, "a point is in no plot");
}
