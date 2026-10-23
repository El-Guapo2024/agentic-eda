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
    let mut t = TextBox::new("F.SilkS", Point { x: 2_000_000, y: 2_000_000 }, Point { x: 22_000_000, y: 10_000_000 }, 1_000_000, 120_000);
    t.text = "REV A\nBuilt by the studio, wrapped to the width of its box when the line is long".into();
    t.line_style = eda_model::pcb_items::LineStyle::Dash;
    t.margin_left_nm = 900_000;
    t.margin_right_nm = 1_100_000;
    t
}

/// A knockout box: the box is copper-free silkscreen ink with the text cut out of it, turned a quarter, mirrored on the back.
fn knockout_box() -> TextBox {
    let mut t = TextBox::new("B.SilkS", Point { x: 2_000_000, y: 14_000_000 }, Point { x: 18_000_000, y: 20_000_000 }, 1_200_000, 150_000);
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
    assert_eq!((shown[0].margin_left_nm, shown[0].margin_right_nm), (900_000, 1_100_000));

    // Move, edit and delete are verbs of the same history.
    let id = shown[0].id.clone();
    step(&dir, Cmd::MoveItems { ids: vec![id.clone()], dx: 1_000_000, dy: 500_000 }, false, "test").unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 3_000_000, y: 2_500_000 });
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(boxes(&dir)[0].start, Point { x: 2_000_000, y: 2_000_000 }, "one undo takes the move back");
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
    let id = add_image(&dir, "Cmts.User", Point { x: 30_000_000, y: 20_000_000 }, 500_000);
    assert!(id.starts_with("img_"));

    let s = state(&dir);
    let shown = &s["drawings"]["images"][0];
    assert_eq!((shown["id"].as_str(), shown["layer"].as_str(), shown["x"].as_i64(), shown["y"].as_i64()), (Some(id.as_str()), Some("Cmts.User"), Some(30_000_000), Some(20_000_000)));
    // 120 x 80 px at 254 ppi are 12 x 8 mm; at half scale 6 x 4
    assert_eq!((shown["w"].as_i64(), shown["h"].as_i64(), shown["scale_ppm"].as_i64(), shown["ppi"].as_u64()), (Some(6_000_000), Some(4_000_000), Some(500_000), Some(254)));
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
    add_image(&dir, "Cmts.User", Point { x: 30_000_000, y: 20_000_000 }, 500_000);
    add_image(&dir, "F.SilkS", Point { x: 60_000_000, y: 40_000_000 }, 1_000_000);
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
    assert_eq!((first.layer.as_str(), first.at, first.scale_ppm), ("Cmts.User", Point { x: 30_000_000, y: 20_000_000 }, 500_000), "kicad-cli read the place, the layer and the scale as written");
    assert_eq!((second.layer.as_str(), second.at, second.scale_ppm), ("F.SilkS", Point { x: 60_000_000, y: 40_000_000 }, 1_000_000));
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
    add_sch_image(&dir, Point { x: 50_800_000, y: 38_100_000 }, 1_000_000);
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
    assert_eq!(read, (Point { x: 50_800_000, y: 38_100_000 }, 1_000_000, base64_encode(&red_png())));

    // an edit that sends no pixels keeps them; one undo takes the edit back
    let edit = eda_model::sch_extras::SchGraphic::new(SchGraphicKind::Image { at: Point { x: 0, y: 0 }, scale_ppm: 2_000_000, data: String::new() });
    step(&dir, Cmd::sch(SchCmd::EditGraphic { id: id.clone(), graphic: edit }), false, "test").unwrap();
    assert_eq!((sch_images(&dir)[0].1, sch_images(&dir)[0].2, sch_images(&dir)[0].3.clone()), (Point { x: 0, y: 0 }, 2_000_000, base64_encode(&red_png())));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert_eq!((sch_images(&dir)[0].1, sch_images(&dir)[0].2), (Point { x: 50_800_000, y: 38_100_000 }, 1_000_000));
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
    add_sch_image(&dir, Point { x: 100_000_000, y: 80_000_000 }, 500_000);
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

fn add_point(dir: &Path, layer: &str, at: Point, size_nm: i64) {
    let mut point = PcbPoint::new(layer, at);
    point.size_nm = size_nm;
    step(dir, Cmd::PcbItem(PcbItemCmd::AddPoint { point }), false, "test").unwrap();
}

/// A point is one undo step of the board; the state describes it; the derived board has `(point ..)` as `format( const PCB_POINT* )` writes it.
#[test]
fn a_point_is_one_undo_step_described_by_the_state_and_written_to_the_board() {
    let dir = scratch("p16_point");
    setup(&dir);
    assert!(points(&dir).is_empty());
    add_point(&dir, "F.SilkS", Point { x: 30_000_000, y: 20_000_000 }, 2_000_000);
    let id = points(&dir)[0].id.clone();
    assert!(id.starts_with("pnt_"));
    let s = state(&dir);
    let shown = &s["drawings"]["points"][0];
    assert_eq!((shown["id"].as_str(), shown["layer"].as_str(), shown["x"].as_i64(), shown["y"].as_i64(), shown["size"].as_i64()), (Some(id.as_str()), Some("F.SilkS"), Some(30_000_000), Some(20_000_000), Some(2_000_000)));

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "t" }).unwrap();
    assert!(text.contains("(point (at 30 20) (size 2) (layer \"F.SilkS\") (uuid \""), "{text}");
    assert!(!text.contains("(point (at 30 20) (size 2) (layer \"F.SilkS\") (locked"), "no lock in the file form");
    let (back, _, notes) = eda_kicad::import_kicad_pcb(&text).unwrap();
    assert!(!notes.unread_items.contains_key("point"), "the importer reads it");
    let read: Vec<_> = back.drawings.unwrap().board_items.points.into_iter().map(|p| (p.layer, p.at, p.size_nm)).collect();
    assert_eq!(read, vec![("F.SilkS".to_string(), Point { x: 30_000_000, y: 20_000_000 }, 2_000_000)]);

    step(&dir, Cmd::PcbItem(PcbItemCmd::EditPoint { id: id.clone(), layer: "B.SilkS".into(), at: Point { x: 0, y: 0 }, size_nm: 3_000_000 }), false, "test").unwrap();
    assert_eq!((points(&dir)[0].layer.as_str(), points(&dir)[0].size_nm), ("B.SilkS", 3_000_000));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!((points(&dir)[0].layer.as_str(), points(&dir)[0].size_nm), ("F.SilkS", 2_000_000), "one undo takes the edit back");
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
    add_point(&dir, "F.SilkS", Point { x: 10_000_000, y: 12_000_000 }, 2_000_000);
    add_point(&dir, "B.Cu", Point { x: 5_000_000, y: 6_000_000 }, 1_000_000);
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
    let mut read: Vec<_> = back.drawings.unwrap().board_items.points.into_iter().map(|p| (p.layer, p.at, p.size_nm)).collect();
    read.sort_by_key(|p| p.1.x);
    assert_eq!(read, vec![("B.Cu".to_string(), Point { x: 5_000_000, y: 6_000_000 }, 1_000_000), ("F.SilkS".to_string(), Point { x: 10_000_000, y: 12_000_000 }, 2_000_000)], "kicad-cli read the place, size and layer as written");
    let with = body(&plot(&cli, &derived, "F.SilkS", &dir.join("with.svg")));
    assert_eq!(with, without, "a point is in no plot");
}

// ----------------------------------------------------------------------------------------------------------------------------- hatch fills

use eda_model::ir::Shape;
use eda_model::pcb_items::{HatchKind, ShapeFill};

fn shapes(dir: &Path) -> Vec<Shape> {
    load(dir).unwrap().1.drawings.map(|d| d.shapes).unwrap_or_default()
}

fn add_shape(dir: &Path, shape: Shape) -> String {
    step(dir, Cmd::AddShape { shape }, false, "test").unwrap();
    shapes(dir).last().unwrap().id().to_string()
}

fn set_fill(dir: &Path, id: &str, fill: ShapeFill) {
    step(dir, Cmd::PcbItem(PcbItemCmd::SetShapeStyle { ids: vec![id.to_string()], fill: Some(fill), start_ending: None, end_ending: None }), false, "test").unwrap();
}

fn rect_shape(layer: &str, (x0, y0): (i64, i64), (x1, y1): (i64, i64)) -> Shape {
    Shape::Rect { id: String::new(), layer: layer.into(), stroke_width: 200_000, filled: false, start: Point { x: x0, y: y0 }, end: Point { x: x1, y: y1 } }
}

/// A vertical line 0.6 mm wide at `x` from `y0` to `y1`.
fn bar(layer: &str, x: i64, y0: i64, y1: i64) -> Shape {
    Shape::Segment { id: String::new(), layer: layer.into(), stroke_width: 600_000, filled: false, start: Point { x, y: y0 }, end: Point { x, y: y1 } }
}

/// The state's description of the shape `id`.
fn shape_json(dir: &Path, id: &str) -> serde_json::Value {
    state(dir)["drawings"]["shapes"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone()
}

/// A hatch is one undo step of the board; the state says how the shape is hatched and carries the lines `EDA_SHAPE::UpdateHatching` makes of it from the board as it is, so a shape put
/// over it cuts them at once (KiCad's `rehatchShapes` after every commit) and taking that shape away gives them back; the derived board has `(fill hatch)`.
#[test]
fn a_hatch_is_one_undo_step_described_by_the_state_with_its_lines_and_written_to_the_board() {
    let dir = scratch("p16_hatch");
    setup(&dir);
    // Away from the two footprints: their Value fields are on F.Fab and cut a hatching they are over (the slow test below has the square over one).
    let id = add_shape(&dir, rect_shape("F.Fab", (1_000_000, 11_000_000), (9_000_000, 19_000_000)));
    assert!(shape_json(&dir, &id).get("fill").is_none() && shape_json(&dir, &id).get("hatch_lines").is_none(), "an unfilled shape says nothing of hatching");

    set_fill(&dir, &id, ShapeFill::Hatch);
    let s = shape_json(&dir, &id);
    assert_eq!((s["fill"].as_str(), s["filled"].as_bool(), s["hatch_width"].as_i64()), (Some("hatch"), Some(false), Some(200_000)));
    let lines: Vec<[i64; 4]> = s["hatch_lines"].as_array().unwrap().iter().map(|l| [l[0].as_i64().unwrap(), l[1].as_i64().unwrap(), l[2].as_i64().unwrap(), l[3].as_i64().unwrap()]).collect();
    // 8 mm square, 0.2 mm line: a line every 2 mm along x + y, three of them 2 mm apart in x (the corners meet at x + y = 2 and 18 mm): x + y = 4, 6, ..., 16 mm.
    assert_eq!(lines.len(), 7, "{lines:?}");
    assert!(lines.iter().all(|l| l[0] + l[1] == l[2] + l[3] && (l[0] + l[1]) % 2_000_000 == 0));
    let alone = lines.len();

    // A line across the shape on its layer: the lines it crosses are cut in two, with no step of ours between.
    let cutter = add_shape(&dir, bar("F.Fab", 5_000_000, 10_000_000, 20_000_000));
    let cut = shape_json(&dir, &id)["hatch_lines"].as_array().unwrap().len();
    assert!(cut > alone, "{cut} against {alone}");
    step(&dir, Cmd::DeleteShape { id: cutter }, false, "test").unwrap();
    assert_eq!(shape_json(&dir, &id)["hatch_lines"].as_array().unwrap().len(), alone, "and taking the bar away gives the lines back");
    // One on another layer cuts nothing.
    add_shape(&dir, bar("F.SilkS", 5_000_000, 10_000_000, 20_000_000));
    assert_eq!(shape_json(&dir, &id)["hatch_lines"].as_array().unwrap().len(), alone);

    // The three hatches: a reverse hatch has the lines the other way, a cross-hatch both.
    set_fill(&dir, &id, ShapeFill::ReverseHatch);
    let rev = shape_json(&dir, &id);
    assert_eq!(rev["fill"], "reverse_hatch");
    assert!(rev["hatch_lines"].as_array().unwrap().iter().all(|l| l[1].as_i64().unwrap() - l[0].as_i64().unwrap() == l[3].as_i64().unwrap() - l[2].as_i64().unwrap()));
    set_fill(&dir, &id, ShapeFill::CrossHatch);
    assert_eq!(shape_json(&dir, &id)["hatch_lines"].as_array().unwrap().len(), 2 * alone);

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "t" }).unwrap();
    assert_eq!(text.matches("(fill cross_hatch)").count(), 1, "{text}");
    let (back, _, _) = eda_kicad::import_kicad_pcb(&text).unwrap();
    assert_eq!(back.drawings.unwrap().board_items.shape_styles.iter().map(|s| s.hatch).collect::<Vec<_>>(), vec![Some(HatchKind::CrossHatch)]);

    // Each fill is one undo step.
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(shape_json(&dir, &id)["fill"], "reverse_hatch");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(shape_json(&dir, &id)["fill"], "hatch");
    set_fill(&dir, &id, ShapeFill::Solid);
    let solid = shape_json(&dir, &id);
    assert_eq!((solid["filled"].as_bool(), solid.get("fill").is_none(), solid.get("hatch_lines").is_none()), (Some(true), true, true), "a solid fill is not a hatch");
}

// ---- the polygons of the plot

/// The polygons kicad-cli's SVG plot has for hatching: the `<path .. fill-rule:evenodd;" d="M x,y x,y .. Z">` that are filled and have no stroke, one per sub-path, in millimetres.
fn plotted_polygons(svg: &str) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    for path in svg.split("<path ").skip(1) {
        let head = path.split("d=\"").next().unwrap_or("");
        if !head.contains("stroke:none") || !head.contains("fill-rule:evenodd") {
            continue;
        }
        let d = path.split("d=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
        for sub in d.split('M').filter(|s| !s.trim().is_empty()) {
            let pts: Vec<(f64, f64)> = sub
                .split(|c: char| c.is_whitespace() || c == 'Z' || c == 'L')
                .filter(|t| t.contains(','))
                .filter_map(|t| t.split_once(',').and_then(|(x, y)| Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))))
                .collect();
            if pts.len() >= 3 {
                out.push(pts);
            }
        }
    }
    out
}

/// Whether every vertex of each polygon has one of the other within `tol_mm`, and the counts of vertices agree.
fn same_polygon(a: &[(f64, f64)], b: &[(f64, f64)], tol_mm: f64) -> bool {
    let near = |p: &(f64, f64), set: &[(f64, f64)]| set.iter().any(|q| (p.0 - q.0).abs() <= tol_mm && (p.1 - q.1).abs() <= tol_mm);
    a.len() == b.len() && a.iter().all(|p| near(p, b)) && b.iter().all(|q| near(q, a))
}

/// The polygons of `theirs` that none of `mine` is (each of mine answers one only).
fn unmatched(mine: &[Vec<(f64, f64)>], theirs: &[Vec<(f64, f64)>], tol_mm: f64) -> Vec<usize> {
    let mut used = vec![false; mine.len()];
    let mut missing = Vec::new();
    for (i, t) in theirs.iter().enumerate() {
        match (0..mine.len()).find(|&j| !used[j] && same_polygon(&mine[j], t, tol_mm)) {
            Some(j) => used[j] = true,
            None => missing.push(i),
        }
    }
    missing
}

fn area_mm2(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].0 * poly[(i + 1) % n].1 - poly[(i + 1) % n].0 * poly[i].1).sum::<f64>().abs() / 2.0
}

/// The hatching kicad-cli plots for a hatch, a reverse hatch and a cross-hatch of a rectangle, a circle and a polygon -- with a bar of the same layer across the rectangle -- is the
/// hatching this studio works out: a hatch and a reverse hatch plot the same polygons (a stadium per line, to the micrometre), a cross-hatch the same area of lattice. kicad-cli's own
/// rewrite of the file keeps the three fills.
#[test]
fn kicad_cli_plots_the_hatching_this_studio_works_out() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_hatch_cli");
    setup(&dir);
    let rect = add_shape(&dir, rect_shape("F.Fab", (1_000_000, 1_000_000), (9_000_000, 9_000_000)));
    set_fill(&dir, &rect, ShapeFill::Hatch);
    add_shape(&dir, bar("F.Fab", 5_000_000, 0, 12_000_000));
    let circle = add_shape(&dir, Shape::Circle { id: String::new(), layer: "F.Fab".into(), stroke_width: 200_000, filled: false, center: Point { x: 15_000_000, y: 5_000_000 }, end: Point { x: 18_500_000, y: 5_000_000 } });
    set_fill(&dir, &circle, ShapeFill::ReverseHatch);
    let tri = add_shape(&dir, Shape::Polygon { id: String::new(), layer: "F.Fab".into(), stroke_width: 200_000, filled: false, pts: vec![Point { x: 11_000_000, y: 11_000_000 }, Point { x: 19_000_000, y: 11_000_000 }, Point { x: 15_000_000, y: 19_000_000 }] });
    set_fill(&dir, &tri, ShapeFill::CrossHatch);

    let args: Vec<String> = ["--layers", "F.Fab"].iter().map(|s| s.to_string()).collect();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    let text = std::fs::read_to_string(&derived).unwrap();
    assert_eq!((text.matches("(fill hatch)").count(), text.matches("(fill reverse_hatch)").count(), text.matches("(fill cross_hatch)").count()), (1, 1, 1), "{text}");

    let theirs = plotted_polygons(&plot(&cli, &derived, "F.Fab", &dir.join("hatch.svg")));
    let (_, design, model) = load(&dir).unwrap();
    let all = design.drawings.clone().unwrap();
    let hatched = |id: &str| (all.shapes.iter().find(|s| s.id() == id).unwrap(), all.board_items.hatch_of(id).unwrap());

    // The three hatchings, each polygon of ours against one of kicad-cli's: a stadium per line for the hatch and the reverse hatch.
    let to_mm = |poly: &eda_shape_poly_set::LineChain| -> Vec<(f64, f64)> { poly.iter().map(|p| (p.x as f64 / 1000.0, p.y as f64 / 1000.0)).collect() };
    let stadiums: Vec<Vec<(f64, f64)>> = [&rect, &circle].iter().flat_map(|id| { let (s, h) = hatched(id); eda_drc::hatch::hatch_polygons(&design, Some(&model), s, h).iter().map(to_mm).collect::<Vec<_>>() }).collect();
    assert!(stadiums.len() > 10, "{} lines", stadiums.len());
    let (tri_shape, tri_kind) = hatched(&tri);
    let lattice: Vec<Vec<(f64, f64)>> = eda_drc::hatch::hatch_polygons(&design, Some(&model), tri_shape, tri_kind).iter().map(to_mm).collect();
    assert!(!lattice.is_empty());
    // kicad-cli's polygons that are ours stadiums: those with the 12 vertices of a stadium of eight segments; the lattice is the rest.
    let (their_stadiums, their_lattice): (Vec<_>, Vec<_>) = theirs.iter().cloned().partition(|p| p.len() == 12);
    let missing = unmatched(&stadiums, &their_stadiums, 0.002);
    let centres = |polys: &[Vec<(f64, f64)>]| -> Vec<(i64, i64, usize)> {
        let mut c: Vec<_> = polys.iter().map(|p| ((p.iter().map(|q| q.0).sum::<f64>() / p.len() as f64 * 1000.0).round() as i64, (p.iter().map(|q| q.1).sum::<f64>() / p.len() as f64 * 1000.0).round() as i64, p.len())).collect();
        c.sort();
        c
    };
    assert!(
        missing.is_empty(),
        "{} of kicad-cli's {} hatch polygons are none of our {}: first {:?}\nours   {:?}\ntheirs {:?}",
        missing.len(),
        their_stadiums.len(),
        stadiums.len(),
        missing.first().map(|&i| &their_stadiums[i]),
        centres(&stadiums),
        centres(&their_stadiums)
    );
    assert_eq!(their_stadiums.len(), stadiums.len(), "the same number of lines, the bar's cuts included");
    let (ours_area, theirs_area): (f64, f64) = (lattice.iter().map(|p| area_mm2(p)).sum(), their_lattice.iter().map(|p| area_mm2(p)).sum());
    assert!(ours_area > 1.0 && (ours_area - theirs_area).abs() / theirs_area < 0.01, "the cross-hatch lattice: {ours_area} mm2 here, {theirs_area} mm2 in kicad-cli's plot");

    // kicad-cli reads the file and writes the fills back.
    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!((upgraded.matches("(fill hatch)").count(), upgraded.matches("(fill reverse_hatch)").count(), upgraded.matches("(fill cross_hatch)").count()), (1, 1, 1), "{upgraded}");
}

// ----------------------------------------------------------------------------------------------------------------------------- barcodes

use eda_model::barcode::{Barcode, BarcodeKind, EccLevel};

fn barcodes(dir: &Path) -> Vec<Barcode> {
    load(dir).unwrap().1.drawings.map(|d| d.barcodes).unwrap_or_default()
}

fn qr(layer: &str, at: Point, text: &str) -> Barcode {
    Barcode {
        id: String::new(),
        at,
        angle: 0,
        layer: layer.into(),
        width_nm: 8_000_000,
        height_nm: 8_000_000,
        text: text.into(),
        text_height_nm: 1_000_000,
        kind: BarcodeKind::Qr,
        ecc: EccLevel::L,
        show_text: true,
        knockout: false,
        margin_x_nm: 0,
        margin_y_nm: 0,
    }
}

fn add_barcode(dir: &Path, b: Barcode) {
    step(dir, Cmd::PcbItem(PcbItemCmd::AddBarcode { barcode: b }), false, "test").unwrap();
}

/// A barcode is one undo step of the board; the state describes it, its shape is asked for by id (an empty text has no symbol, so that answer needs no kicad-cli), and the derived board has
/// `(barcode ..)` as `format( const PCB_BARCODE* )` writes it.
#[test]
fn a_barcode_is_one_undo_step_described_by_the_state_and_written_to_the_board() {
    let dir = scratch("p16_barcode");
    setup(&dir);
    assert!(barcodes(&dir).is_empty());
    add_barcode(&dir, qr("F.SilkS", Point { x: 10_000_000, y: 12_000_000 }, "HELLO"));
    let id = barcodes(&dir)[0].id.clone();
    assert!(id.starts_with("bar_"));
    let s = state(&dir);
    let shown = &s["drawings"]["barcodes"][0];
    assert_eq!(
        (shown["id"].as_str(), shown["layer"].as_str(), shown["x"].as_i64(), shown["y"].as_i64(), shown["w"].as_i64(), shown["h"].as_i64(), shown["text"].as_str(), shown["kind"].as_str(), shown["ecc"].as_str(), shown["show_text"].as_bool()),
        (Some(id.as_str()), Some("F.SilkS"), Some(10_000_000), Some(12_000_000), Some(8_000_000), Some(8_000_000), Some("HELLO"), Some("qr"), Some("L"), Some(true))
    );

    let (_, design, model) = load(&dir).unwrap();
    let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-21", title: "t" }).unwrap();
    assert!(text.contains("(barcode (at 10 12 0) (layer \"F.SilkS\") (size 8 8) (text \"HELLO\") (text_height 1) (type qr) (ecc_level L) (hide no) (knockout no)"), "{text}");
    let (back, _, notes) = eda_kicad::import_kicad_pcb(&text).unwrap();
    assert!(!notes.unread_items.contains_key("barcode"), "the importer reads it");
    assert_eq!(back.drawings.unwrap().barcodes.len(), 1);

    let mut edited = barcodes(&dir)[0].clone();
    edited.text = "WORLD".into();
    edited.angle = 90_000;
    step(&dir, Cmd::PcbItem(PcbItemCmd::EditBarcode { id: id.clone(), barcode: edited }), false, "test").unwrap();
    assert_eq!((barcodes(&dir)[0].text.as_str(), barcodes(&dir)[0].angle), ("WORLD", 90_000));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!((barcodes(&dir)[0].text.as_str(), barcodes(&dir)[0].angle), ("HELLO", 0), "one undo takes the edit back");
    step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteBarcode { id }), false, "test").unwrap();
    assert!(barcodes(&dir).is_empty());
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(barcodes(&dir).len(), 1, "undo brings it back");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(barcodes(&dir).is_empty());
}

/// The shape routes: an unknown barcode and a bad request are refused and a barcode with no text has no symbol to plot (so no plot runs and no kicad-cli is needed).
#[test]
fn the_barcode_shape_routes_answer_without_a_plot_for_what_has_nothing_to_plot() {
    let dir = scratch("p16_barcode_route");
    setup(&dir);
    let unknown = crate::barcode_api::shape_route(&dir, "/api/barcode_shape?id=bar_nope");
    assert_eq!(unknown.status, "404 Not Found");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&unknown.body).unwrap()["status"], "failed");
    add_barcode(&dir, qr("F.SilkS", Point { x: 5_000_000, y: 5_000_000 }, ""));
    let id = barcodes(&dir)[0].id.clone();
    let empty = crate::barcode_api::shape_route(&dir, &format!("/api/barcode_shape?id={id}"));
    assert_eq!(empty.status, "200 OK");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&empty.body).unwrap(), serde_json::json!({ "status": "ready", "paths": [] }));
    assert_eq!(crate::barcode_api::preview_route(&dir, b"not json").status, "400 Bad Request");
    assert_eq!(crate::barcode_api::preview_route(&dir, br#"{"barcode": {"layer": "F.SilkS"}}"#).status, "400 Bad Request");
    let preview = crate::barcode_api::preview_route(&dir, br#"{"barcode": {"layer": "F.SilkS", "at": {"x": 0, "y": 0}, "width_nm": 5000, "height_nm": 5000, "text": "", "text_height_nm": 1000}}"#);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&preview.body).unwrap(), serde_json::json!({ "status": "ready", "paths": [] }));
}

/// The shape this studio serves for a barcode is what kicad-cli plots of it: for QR, Micro QR, Data Matrix and Code 128 barcodes, a turned one, one on a back layer (mirrored), one with a
/// knockout and margins, and a text one kind cannot encode, the polygons the board's own plot has -- at the place the barcode is on the board -- are the polygons of the shape (plotted
/// with the barcode at the origin and moved here), vertex for vertex to 2 micrometres. kicad-cli's `pcb upgrade --force` keeps the barcodes with their parameters.
#[test]
fn kicad_cli_plots_the_barcode_shape_this_studio_serves() {
    if !slow() {
        return;
    }
    let Some(cli) = eda_kicad_engine::find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let dir = scratch("p16_barcode_cli");
    setup(&dir);
    let at = Point { x: 10_000_000, y: 12_000_000 };
    let case = |layer: &str, kind: BarcodeKind, text: &str, w: i64, h: i64, mutate: &dyn Fn(&mut Barcode)| {
        let mut b = qr(layer, at, text);
        b.kind = kind;
        b.width_nm = w;
        b.height_nm = h;
        mutate(&mut b);
        b
    };
    let cases = vec![
        ("qr", case("F.SilkS", BarcodeKind::Qr, "HELLO", 8_000_000, 8_000_000, &|_| {})),
        ("qr turned, level H", case("F.SilkS", BarcodeKind::Qr, "TURNED 90", 8_000_000, 8_000_000, &|b| {
            b.angle = 90_000;
            b.ecc = EccLevel::H;
        })),
        ("micro qr", case("F.SilkS", BarcodeKind::MicroQr, "MQ1", 6_000_000, 6_000_000, &|b| b.ecc = EccLevel::M)),
        ("data matrix", case("F.SilkS", BarcodeKind::DataMatrix, "DM-2026", 7_000_000, 7_000_000, &|_| {})),
        ("code 128 with its text", case("F.SilkS", BarcodeKind::Code128, "AB-123", 14_000_000, 5_000_000, &|b| b.text_height_nm = 1_200_000)),
        ("qr on the back (mirrored)", case("B.SilkS", BarcodeKind::Qr, "BACK", 8_000_000, 8_000_000, &|_| {})),
        ("qr knocked out of the silkscreen", case("F.SilkS", BarcodeKind::Qr, "KO", 8_000_000, 8_000_000, &|b| {
            b.knockout = true;
            b.margin_x_nm = 1_000_000;
            b.margin_y_nm = 600_000;
        })),
        ("code 39 with a text it cannot take", case("F.SilkS", BarcodeKind::Code39, "h\u{e9}llo", 14_000_000, 5_000_000, &|_| {})),
    ];
    let derived_of = |dir: &Path| std::fs::read_dir(dir.join(".kicad")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "kicad_pcb")).expect("the derived board file");
    for (name, barcode) in cases {
        add_barcode(&dir, barcode.clone());
        let args: Vec<String> = ["--layers", &barcode.layer].iter().map(|s| s.to_string()).collect();
        crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
        let derived = derived_of(&dir);
        let svg = plot(&cli, &derived, &barcode.layer, &dir.join("theirs.svg"));
        let theirs: Vec<Vec<Vec<[i64; 2]>>> = crate::barcode_api::filled_paths(&svg);
        let ours = crate::barcode_api::shape_blocking(&barcode, &barcode.text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let moved: Vec<Vec<Vec<[i64; 2]>>> = ours.paths.iter().map(|path| path.iter().map(|ring| ring.iter().map(|p| [p[0] + at.x, p[1] + at.y]).collect()).collect()).collect();
        assert_eq!(moved.len(), theirs.len(), "{name}: the same number of polygons ({} served, {} plotted)", moved.len(), theirs.len());
        for (i, (a, b)) in moved.iter().zip(&theirs).enumerate() {
            assert_eq!(a.len(), b.len(), "{name}: polygon {i} has the same contours");
            for (ra, rb) in a.iter().zip(b) {
                assert_eq!(ra.len(), rb.len(), "{name}: polygon {i}");
                assert!(ra.iter().zip(rb).all(|(p, q)| (p[0] - q[0]).abs() <= 2000 && (p[1] - q[1]).abs() <= 2000), "{name}: polygon {i} differs: {ra:?} against {rb:?}");
            }
        }
        if name == "code 39 with a text it cannot take" {
            assert!(theirs.is_empty(), "zint makes nothing of it, and so does kicad-cli's plot: {} polygons", theirs.len());
        } else {
            assert!(!theirs.is_empty(), "{name}: a plot with polygons in it");
        }
        // the extent: the barcode's own size, centred where it is (the text under it, and a knockout's margin, are outside it)
        if name == "qr" {
            let (xs, ys): (Vec<i64>, Vec<i64>) = (theirs.iter().flatten().flatten().map(|p| p[0]).collect(), theirs.iter().flatten().flatten().map(|p| p[1]).collect());
            assert_eq!((xs.iter().min().copied(), xs.iter().max().copied()), (Some(at.x - 4_000_000), Some(at.x + 4_000_000)), "8 mm wide, centred on x");
            assert!(ys.iter().min().copied() == Some(at.y - 4_000_000) && ys.iter().max().unwrap() > &(at.y + 4_000_000), "8 mm tall from the top, with its text under it");
        }
        let id = barcodes(&dir)[0].id.clone();
        step(&dir, Cmd::PcbItem(PcbItemCmd::DeleteBarcode { id }), false, "test").unwrap();
    }

    // kicad-cli reads the barcodes and writes them back with what they were given
    add_barcode(&dir, case("F.SilkS", BarcodeKind::Qr, "KEPT", 8_000_000, 8_000_000, &|b| b.ecc = EccLevel::Q));
    add_barcode(&dir, case("F.SilkS", BarcodeKind::Code128, "KEPT-TOO", 14_000_000, 5_000_000, &|b| b.at = Point { x: 4_000_000, y: 3_000_000 }));
    let args: Vec<String> = ["--layers", "F.SilkS"].iter().map(|s| s.to_string()).collect();
    crate::kicad_engine::export(&dir, "svg", &args).expect("the studio's plot of the board");
    let derived = derived_of(&dir);
    let copy = dir.join("upgraded.kicad_pcb");
    std::fs::copy(&derived, &copy).unwrap();
    let rewrite = eda_kicad_engine::cli_command(&cli, None).args(["pcb", "upgrade", "--force"]).arg(&copy).output().expect("kicad-cli runs");
    assert!(rewrite.status.success(), "pcb upgrade: {}{}", String::from_utf8_lossy(&rewrite.stdout), String::from_utf8_lossy(&rewrite.stderr));
    let upgraded = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(upgraded.matches("(barcode").count(), 2, "kicad-cli kept both barcodes: {upgraded}");
    let (back, _, _) = eda_kicad::import_kicad_pcb(&upgraded).unwrap();
    let mut read: Vec<_> = back.drawings.unwrap().barcodes.into_iter().map(|b| (b.text, b.kind, b.ecc, b.at, b.width_nm)).collect();
    read.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(read[0].0, "KEPT");
    assert_eq!((read[0].1, read[0].2, read[0].3, read[0].4), (BarcodeKind::Qr, EccLevel::Q, at, 8_000_000));
    assert_eq!((read[1].0.as_str(), read[1].1), ("KEPT-TOO", BarcodeKind::Code128));
}

// ----------------------------------------------------------------------------------------------------------------------------- KiCad 10: line endings, grid items

/// A line's endings are one undo step, described by the state (`start_ending`, `end_ending`) and written to the board after the stroke.
#[test]
fn a_lines_endings_are_one_undo_step_described_by_the_state_and_written_to_the_board() {
    use eda_model::line_ending::{LineEnding, LineEndingStyle};
    let dir = scratch("k10_endings");
    setup(&dir);
    let id = add_shape(&dir, bar("F.SilkS", 5_000_000, 10_000_000, 20_000_000));
    let arrow = LineEnding { style: LineEndingStyle::Arrow, length_nm: 2_000_000, ..Default::default() };
    step(&dir, Cmd::PcbItem(PcbItemCmd::SetShapeStyle { ids: vec![id.clone()], fill: None, start_ending: Some(arrow), end_ending: Some(LineEnding::new(LineEndingStyle::ArrowOpen)) }), false, "test").unwrap();
    let s = shape_json(&dir, &id);
    assert_eq!(s["start_ending"], serde_json::json!({ "style": "arrow", "length": 2_000_000, "width": 0, "stroke_width": 0 }));
    assert_eq!(s["end_ending"]["style"], "arrow_open");
    let pcb = std::fs::read_to_string(dir.join(".kicad").join(format!("{}.kicad_pcb", dir.file_name().unwrap().to_str().unwrap()))).unwrap_or_default();
    if !pcb.is_empty() {
        assert!(pcb.contains("(start_shape arrow (length 2)) (end_shape arrow_open)"), "{pcb}");
    }
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(shape_json(&dir, &id).get("start_ending").is_none(), "the undo takes them away");
}

/// A grid item is one undo step of the board and the state lists it.
#[test]
fn a_grid_item_is_one_undo_step_and_the_state_lists_it() {
    use eda_model::grid_item::GridItem;
    let dir = scratch("k10_grid");
    setup(&dir);
    let g = GridItem { angle: 30.0, tick_interval: 5000, ..GridItem::new(Point { x: 20_000_000, y: 15_000_000 }) };
    step(&dir, Cmd::PcbItem(PcbItemCmd::AddGridItem { grid_item: g }), false, "test").unwrap();
    let grids = state(&dir)["drawings"]["grid_items"].clone();
    assert_eq!(grids.as_array().unwrap().len(), 1);
    let j = &grids[0];
    assert_eq!((j["kind"].as_str(), j["x"].as_i64(), j["extent"][0].as_i64(), j["angle"].as_f64(), j["tick_interval"].as_i64(), j["affects"]["cursor"].as_bool()), (Some("cartesian"), Some(20_000_000), Some(5_000_000), Some(30.0), Some(5000), Some(true)));
    assert!(j["id"].as_str().unwrap().starts_with("grd_"));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(state(&dir)["drawings"]["grid_items"].as_array().map_or(0, |a| a.len()), 0);
}

/// A command's drill-chart rebuild sees the board the command left (`OnModify`'s `RefreshGeneratedTables` after the commit): a new board's footprints are in its chart in
/// the import's own step, not in the next command's.
#[test]
fn a_replaced_boards_drill_chart_lists_its_footprints_holes_in_the_same_step() {
    let qa = std::path::PathBuf::from(std::env::var_os("KICAD10_QA_DATA").unwrap_or_else(|| "/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data".into()));
    let Ok(text) = std::fs::read_to_string(qa.join("pcbnew/api_kitchen_sink.kicad_pcb")) else {
        eprintln!("skipped: needs KiCad 25ec2335's api_kitchen_sink.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    let dir = scratch("drill_chart_replace");
    setup(&dir);
    step(&dir, Cmd::ReplaceBoard { text, name: "api_kitchen_sink.kicad_pcb".into(), keep_kicad_layer_names: false, project: None }, false, "test").unwrap();
    let dr = load(&dir).unwrap().1.drawings.unwrap();
    let chart = dr.board_items.drill_charts.first().expect("the kitchen sink's chart");
    let t = dr.tables.iter().find(|t| t.id == chart.table_id).unwrap();
    let texts: Vec<&str> = t.cells.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["SYM", "Drill (mm)", "", "0.30", "", "1.14", "", "1.52"], "the pads' rows, as kicad-cli's upgrade has them");
}

/// Fill All regenerates a via stitching its fill made dirty (`ZONE_FILLER_TOOL::regenerateDirtyGenerators`, `PCB_VIA_STITCH::OnZoneFillChanged`), then refills round the
/// vias (`refillAroundGenerators`): `stitch_fp_rulearea`'s stitching emptied, a fill brings KiCad's 73 vias back. Slow tier (kicad-cli fills).
#[test]
fn fill_all_regenerates_a_via_stitching_over_the_new_fill() {
    if !slow() {
        return;
    }
    let qa = std::path::PathBuf::from(std::env::var_os("KICAD10_QA_DATA").unwrap_or_else(|| "/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data".into()));
    let Ok(text) = std::fs::read_to_string(qa.join("pcbnew/stitch_fp_rulearea.kicad_pcb")) else {
        eprintln!("skipped: needs KiCad 25ec2335's stitch_fp_rulearea.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    if eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: needs kicad-cli");
        return;
    }
    let dir = scratch("stitch_fill");
    setup(&dir);
    step(&dir, Cmd::ReplaceBoard { text, name: "stitch_fp_rulearea.kicad_pcb".into(), keep_kicad_layer_names: false, project: None }, false, "test").unwrap();
    let design = load(&dir).unwrap().1;
    let v = design.drawings.as_ref().unwrap().board_items.via_stitches[0].clone();
    let kicad: std::collections::BTreeSet<(i64, i64)> = design.routing.as_ref().unwrap().vias.iter().filter(|x| v.member_ids.contains(&x.id)).map(|x| (x.at.x, x.at.y)).collect();
    assert_eq!(kicad.len(), 73);
    step(&dir, Cmd::PcbItem(PcbItemCmd::SetViaStitch { id: Some(v.id.clone()), stitch: v.clone(), vias: Some(Vec::new()) }), false, "test").unwrap();
    assert!(load(&dir).unwrap().1.drawings.unwrap().board_items.via_stitches[0].member_ids.is_empty());
    let done = crate::fill_api::refill(&dir, None);
    assert_eq!(done["ok"], true, "{done}");
    let after = load(&dir).unwrap().1;
    let v = &after.drawings.as_ref().unwrap().board_items.via_stitches[0];
    let ours: std::collections::BTreeSet<(i64, i64)> = after.routing.as_ref().unwrap().vias.iter().filter(|x| v.member_ids.contains(&x.id)).map(|x| (x.at.x, x.at.y)).collect();
    assert_eq!(ours, kicad, "the fill put the stitching's vias back where KiCad's Update put them");
}
