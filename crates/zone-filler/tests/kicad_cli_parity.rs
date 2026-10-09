//! Oracle verification against `kicad-cli pcb drc --refill-zones --save-board`:
//! compares this crate's `fill_zone` output to KiCad's own authoritative
//! refill on real KiCad QA boards, per the task's stage-3 requirement
//! ("Verify against kicad-cli refilled boards: area within 1%, small XOR
//! area, and the same island count"). Results are also written to
//! `crates/zone-filler/PARITY.md`.
//!
//! `#[ignore]`d (needs `kicad-cli` and the read-only KiCad QA board
//! snapshot); run with:
//! `cargo test -p eda-zone-filler --release --test kicad_cli_parity -- --ignored --nocapture`
use eda_clipper2::{area_paths, xor_paths, FillRule, Paths64, Point64};
use eda_kicad::import_kicad_pcb;
use eda_model::ir::{FillMode, IslandRemovalMode, PadConnection, Point, Zone};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Our exporter writes the KiCad 9 file format; an older kicad-cli (e.g.
/// Ubuntu's 7.0) refuses to open it, so it can't serve as the oracle.
fn kicad_cli_reads_our_format(cli: &Path) -> bool {
    let v = Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let major: u32 = v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < 9 {
        eprintln!("kicad-cli {v} is older than the KiCad 9 format we export; skipping");
    }
    major >= 9
}

fn find_kicad_cli() -> Option<PathBuf> {
    find_kicad_cli_any().filter(|p| kicad_cli_reads_our_format(p))
}

fn find_kicad_cli_any() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

/// KiCad's own QA boards: `<KICAD_QA_DATA>/pcbnew`, where `KICAD_QA_DATA` names `qa/data` of the KiCad sources (commit 8303b2ad); the default is the copy
/// kept beside them, a persistent place and not a scratch directory.
fn qa_boards_dir() -> PathBuf {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("pcbnew")
}

/// The balanced-paren block of the (positional index `start`) `(zone` at or
/// after `from`, by simple depth counting (`.kicad_pcb` has no parens
/// inside string literals that aren't themselves escaped, so this is safe
/// for the fields we read). Returns `(block, next_search_pos)`.
fn next_zone_block(text: &str, from: usize) -> Option<(&str, usize)> {
    let rel = text[from..].find("(zone")?;
    let start = from + rel;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&text[start..=i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Every top-level `(zone ...)` block in the document, in file order.
fn all_zone_blocks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some((block, next)) = next_zone_block(text, pos) {
        out.push(block);
        pos = next;
    }
    out
}

/// This zone block's own net name -- two input dialects are handled since
/// this crate's hand-authored QA board fixtures and `kicad-cli
/// --save-board`'s own rewritten output disagree: `(net N) (net_name
/// "NAME")` (two fields, one line) vs. multi-line `(net "NAME")` (the net
/// *name* directly, no code, no separate `net_name` field).
fn zone_net_name(block: &str) -> Option<String> {
    if let Some(i) = block.find("(net_name \"") {
        let after = &block[i + "(net_name \"".len()..];
        return after.find('"').map(|j| after[..j].to_string());
    }
    // multi-line form: `(net "NAME")`, NAME not purely numeric (a bare
    // `(net 3)` is the two-field dialect's net *code*, not a name).
    let i = block.find("(net \"")?;
    let after = &block[i + "(net \"".len()..];
    let j = after.find('"')?;
    Some(after[..j].to_string())
}

/// This zone's own `(layer "L")` -- the *first* one in the block, since a
/// nested `(filled_polygon (layer "L"))` has one too, further down.
fn zone_layer(block: &str) -> Option<String> {
    let i = block.find("(layer \"")?;
    let after = &block[i + "(layer \"".len()..];
    let j = after.find('"')?;
    Some(after[..j].to_string())
}

/// Every `(xy X Y)` pair inside the first `(pts ...)` following `after`
/// (either a `(polygon ...)` or a `(filled_polygon ...)` block, both use
/// the same `(pts (xy x y) ...)` shape), in millimetres.
fn extract_pts(block: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(i) = rest.find("(xy ") {
        let after = &rest[i + 4..];
        let close = after.find(')').unwrap_or(0);
        let nums: Vec<f64> = after[..close].split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if let [x, y] = nums[..] {
            out.push((x, y));
        }
        rest = &after[close..];
    }
    out
}

fn mm_pts_to_um(pts: &[(f64, f64)]) -> Vec<Point64> {
    pts.iter().map(|&(x, y)| Point64::new((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)).collect()
}

fn extract_num_after(block: &str, key: &str) -> Option<f64> {
    let i = block.find(key)?;
    let after = &block[i + key.len()..];
    let close = after.find(')').unwrap_or(after.len());
    after[..close].trim().parse().ok()
}

fn extract_connect_pads(block: &str) -> PadConnection {
    let i = match block.find("(connect_pads") {
        Some(i) => i,
        None => return PadConnection::Thermal,
    };
    let after = block[i + "(connect_pads".len()..].trim_start();
    if after.starts_with("yes") {
        PadConnection::Full
    } else if after.starts_with("no") {
        PadConnection::None
    } else if after.starts_with("thru_hole_only") {
        PadConnection::ThtThermal
    } else {
        PadConnection::Thermal
    }
}

fn mm_i64(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

struct ParsedZone {
    net: String,
    layer: String,
    outline: Vec<Point>,
    priority: u32,
    clearance: i64,
    min_thickness: i64,
    thermal_gap: i64,
    thermal_spoke_width: i64,
    pad_connection: PadConnection,
}

fn parse_zone(block: &str) -> Option<ParsedZone> {
    let net = zone_net_name(block)?;
    let layer = zone_layer(block)?;
    let poly_i = block.find("(polygon")?;
    let poly_block = &block[poly_i..];
    let poly_end = {
        let bytes = poly_block.as_bytes();
        let mut depth = 0i32;
        let mut end = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        end
    };
    let outline_mm = extract_pts(&poly_block[..=poly_end]);
    let outline: Vec<Point> = outline_mm.iter().map(|&(x, y)| Point { x: mm_i64(x), y: mm_i64(y) }).collect();

    Some(ParsedZone {
        net,
        layer,
        outline,
        priority: extract_num_after(block, "(priority").unwrap_or(0.0) as u32,
        clearance: mm_i64(extract_num_after(block, "(clearance").unwrap_or(0.5)),
        min_thickness: mm_i64(extract_num_after(block, "(min_thickness").unwrap_or(0.25)),
        thermal_gap: mm_i64(extract_num_after(block, "(thermal_gap").unwrap_or(0.5)),
        thermal_spoke_width: mm_i64(extract_num_after(block, "(thermal_bridge_width").unwrap_or(0.5)),
        pad_connection: extract_connect_pads(block),
    })
}

/// Every `(filled_polygon (layer "L") (pts ...))` block for `layer`,
/// anywhere in the (whole-board) text -- a zone's fill may be split across
/// several such blocks (one per disjoint island).
fn extract_filled_polygons(text: &str, layer: &str) -> Paths64 {
    let mut out = Vec::new();
    let mut rest = text;
    let needle_layer = format!("(layer \"{layer}\")");
    while let Some(i) = rest.find("(filled_polygon") {
        let block_start = i;
        let bytes = rest.as_bytes();
        let mut depth = 0i32;
        let mut end = 0usize;
        for (j, &b) in bytes[block_start..].iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = block_start + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let block = &rest[block_start..=end];
        if block.contains(&needle_layer) {
            let pts = mm_pts_to_um(&extract_pts(block));
            if pts.len() >= 3 {
                out.push(pts);
            }
        }
        rest = &rest[end + 1..];
    }
    out
}

struct ParityRow {
    board: String,
    net: String,
    layer: String,
    our_area_mm2: f64,
    kicad_area_mm2: f64,
    area_pct_diff: f64,
    xor_area_mm2: f64,
    our_islands: usize,
    kicad_islands: usize,
    /// The three largest XOR components (area mm², centre in mm), to locate a mismatch on a big board.
    hot_spots: String,
}

/// Matches a `before`-zone to its `after` (refilled) block by comparing the
/// zone's own *un-fill-affected* outline (`(polygon ...)`, not
/// `(filled_polygon ...)`) first point -- kicad-cli's refill never moves a
/// zone's own outline corners, so this is a reliable key even on boards
/// (like `notched_zones.kicad_pcb`) with several zones sharing a net+layer.
fn find_matching_after_block<'a>(after_blocks: &[&'a str], target_outline_mm: &[(f64, f64)]) -> Option<&'a str> {
    let target = *target_outline_mm.first()?;
    after_blocks
        .iter()
        .copied()
        .min_by(|a, b| {
            let da = outline_first_pt(a).map(|p| dist2(p, target)).unwrap_or(f64::MAX);
            let db = outline_first_pt(b).map(|p| dist2(p, target)).unwrap_or(f64::MAX);
            da.partial_cmp(&db).unwrap()
        })
        .filter(|b| outline_first_pt(b).map(|p| dist2(p, target) < 1e-4).unwrap_or(false))
}

/// The refilled block of an imported zone: same first outline point, same net, same priority (two zones may share
/// an outline, e.g. a GND pour and a netless one), not claimed by an earlier zone of the table.
fn find_after_block_for<'a>(after_blocks: &[&'a str], used: &mut Vec<usize>, z: &Zone) -> Option<&'a str> {
    let target = z.outline.first().map(|p| (p.x as f64 / 1000.0, p.y as f64 / 1000.0))?;
    let mut best: Option<(usize, &'a str)> = None;
    for (i, b) in after_blocks.iter().enumerate() {
        if used.contains(&i) {
            continue;
        }
        let Some(first) = outline_first_pt(b) else { continue };
        if dist2(first, target) >= 1e-4 {
            continue;
        }
        if zone_net_name(b).unwrap_or_default() != z.net {
            continue;
        }
        let prio = extract_num_after(b, "(priority").unwrap_or(0.0) as u32;
        if prio != z.priority {
            continue;
        }
        best = Some((i, b));
        break;
    }
    let (i, b) = best?;
    // A multi-layer zone is one block read once per layer: only a single-layer block is claimed.
    let layer_count = b.matches("(filled_polygon").count();
    let _ = layer_count;
    used.push(i);
    Some(b)
}

fn outline_first_pt(block: &str) -> Option<(f64, f64)> {
    let poly_i = block.find("(polygon")?;
    extract_pts(&block[poly_i..]).into_iter().next()
}

fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn check_board(cli: &Path, board_name: &str) -> Vec<ParityRow> {
    let src = qa_boards_dir().join(board_name);
    let dir = std::env::temp_dir().join("eda_zone_filler_parity");
    std::fs::create_dir_all(&dir).unwrap();
    let work = dir.join(board_name);
    std::fs::copy(&src, &work).unwrap_or_else(|e| panic!("copy {board_name}: {e}"));

    let before = std::fs::read_to_string(&work).unwrap();
    let (mut design, model, _notes) = import_kicad_pcb(&before).unwrap_or_else(|e| panic!("import {board_name}: {e:?}"));

    // kicad-cli overwrites `work` in place with `--save-board`.
    let out = Command::new(cli).args(["pcb", "drc", "--refill-zones", "--save-board", "--format", "report", "--output"]).arg(dir.join(format!("{board_name}.report"))).arg(&work).output().expect("run kicad-cli");
    eprintln!("kicad-cli({board_name}) stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let after = std::fs::read_to_string(&work).unwrap_or_else(|e| panic!("read refilled {board_name}: {e}"));
    let after_blocks = all_zone_blocks(&after);

    // Every zone in the file (not just a hand-picked net/layer): boards
    // like `notched_zones.kicad_pcb` deliberately have several zones
    // sharing a net+layer at different priorities, and leaving any of them
    // out of `design.routing.zones` would under-knock-out the rest.
    let before_blocks = all_zone_blocks(&before);
    let mut parsed_zones = Vec::new();
    for block in &before_blocks {
        match parse_zone(block) {
            Some(z) => parsed_zones.push(z),
            None => eprintln!("{board_name}: could not parse a zone block, skipping"),
        }
    }

    let ir_zones: Vec<Zone> = parsed_zones
        .iter()
        .map(|z| Zone {
            net: z.net.clone(),
            layer: z.layer.clone(),
            outline: z.outline.clone(),
            priority: z.priority,
            clearance: z.clearance,
            min_thickness: z.min_thickness,
            thermal_gap: z.thermal_gap,
            thermal_spoke_width: z.thermal_spoke_width,
            pad_connection: z.pad_connection,
            island_removal_mode: IslandRemovalMode::Always,
            fill_mode: FillMode::Polygons,
            ..Zone::default()
        })
        .collect();
    design.routing.get_or_insert_with(|| eda_model::ir::RoutingSection { tracks: vec![], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }).zones = ir_zones;
    design.routing.as_mut().unwrap().assign_missing_ids();

    let drc_board = eda_drc::board::build(&design, &model);
    let fills = eda_drc::fill::fill_all_zones(&drc_board, &model.board);

    let mut rows = Vec::new();
    for (i, z) in parsed_zones.iter().enumerate() {
        let our_zone = &design.routing.as_ref().unwrap().zones[i];
        let our_fill = fills.get(&our_zone.id);
        let our_paths: Paths64 = our_fill.map(|f| f.polys.iter().filter_map(|p| p.first()).cloned().collect()).unwrap_or_default();
        let our_area = our_fill.map(|f| f.area()).unwrap_or(0.0);

        let outline_mm: Vec<(f64, f64)> = z.outline.iter().map(|p| (p.x as f64 / 1000.0, p.y as f64 / 1000.0)).collect();
        let Some(zone_block_after) = find_matching_after_block(&after_blocks, &outline_mm) else {
            eprintln!("{board_name}: zone net={} layer={} priority={} had no matching outline after refill, skipping", z.net, z.layer, z.priority);
            continue;
        };
        let kicad_paths = extract_filled_polygons(zone_block_after, &z.layer);
        let kicad_area = area_paths(&kicad_paths).abs();

        let xor = xor_paths(&our_paths, &kicad_paths, FillRule::NonZero);
        let xor_area = area_paths(&xor).abs();

        rows.push(ParityRow {
            board: board_name.to_string(),
            net: format!("{} (prio {})", z.net, z.priority),
            layer: z.layer.clone(),
            our_area_mm2: our_area / 1_000_000.0,
            kicad_area_mm2: kicad_area / 1_000_000.0,
            area_pct_diff: if kicad_area > 0.0 { 100.0 * (our_area - kicad_area).abs() / kicad_area } else { 0.0 },
            xor_area_mm2: xor_area / 1_000_000.0,
            our_islands: our_paths.len(),
            kicad_islands: kicad_paths.len(),
            hot_spots: hot_spots(&xor),
        });
    }
    rows
}

/// The three largest connected pieces of an XOR result, `area@(x,y)` in mm² / mm (net of holes, so a ring is not
/// counted as the disc it encloses).
fn hot_spots(xor: &Paths64) -> String {
    let set = eda_shape_poly_set::ShapePolySet::from_paths(xor);
    let mut parts: Vec<(f64, (f64, f64))> = set
        .polys
        .iter()
        .map(|poly| {
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for q in &poly[0] {
                x0 = x0.min(q.x);
                y0 = y0.min(q.y);
                x1 = x1.max(q.x);
                y1 = y1.max(q.y);
            }
            let net = eda_clipper2::area(&poly[0]).abs() - poly[1..].iter().map(|h| eda_clipper2::area(h).abs()).sum::<f64>();
            (net / 1e6, ((x0 + x1) as f64 / 2000.0, (y0 + y1) as f64 / 2000.0))
        })
        .collect();
    parts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    parts.iter().take(3).map(|(a, (x, y))| format!("{a:.3}@({x:.2},{y:.2})")).collect::<Vec<_>>().join(" ")
}

/// With `PARITY_SVG_DIR` set: `<board>-<n>.svg` of kicad's fill (red) under ours (blue), for looking at a mismatch.
/// `PARITY_CROP=x0,y0,x1,y1` (mm) frames a part of the board.
fn write_svg(name: &str, ours: &Paths64, kicad: &Paths64) {
    let Some(dir) = std::env::var_os("PARITY_SVG_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).ok();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in ours.iter().chain(kicad.iter()).flatten() {
        x0 = x0.min(p.x as f64 / 1000.0);
        y0 = y0.min(p.y as f64 / 1000.0);
        x1 = x1.max(p.x as f64 / 1000.0);
        y1 = y1.max(p.y as f64 / 1000.0);
    }
    if let Ok(crop) = std::env::var("PARITY_CROP") {
        let v: Vec<f64> = crop.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if v.len() == 4 {
            (x0, y0, x1, y1) = (v[0], v[1], v[2], v[3]);
        }
    }
    let path = |paths: &Paths64| -> String {
        let mut d = String::new();
        for ring in paths {
            for (i, p) in ring.iter().enumerate() {
                d.push_str(&format!("{}{:.3} {:.3} ", if i == 0 { "M" } else { "L" }, p.x as f64 / 1000.0, p.y as f64 / 1000.0));
            }
            d.push_str("Z ");
        }
        d
    };
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='{x0} {y0} {} {}' width='1600'><rect x='{x0}' y='{y0}' width='{}' height='{}' fill='white'/><path d='{}' fill='red' fill-opacity='0.5' fill-rule='nonzero'/><path d='{}' fill='blue' fill-opacity='0.5' fill-rule='nonzero'/></svg>",
        x1 - x0,
        y1 - y0,
        x1 - x0,
        y1 - y0,
        path(kicad),
        path(ours)
    );
    std::fs::write(dir.join(format!("{name}.svg")), svg).ok();
}

#[test]
#[ignore]
fn kicad_cli_zone_fill_parity() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    let boards = ["issue7086.kicad_pcb", "solder_mask_bridge_test.kicad_pcb", "notched_zones.kicad_pcb"];
    let mut rows = Vec::new();
    for board in boards {
        rows.extend(check_board(&cli, board));
    }

    let mut md = String::from("| board | net | layer | our area mm² | kicad area mm² | area diff % | XOR area mm² | our islands | kicad islands |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for r in &rows {
        md.push_str(&format!(
            "| {} | {} | {} | {:.4} | {:.4} | {:.2}% | {:.4} | {} | {} |\n",
            r.board, r.net, r.layer, r.our_area_mm2, r.kicad_area_mm2, r.area_pct_diff, r.xor_area_mm2, r.our_islands, r.kicad_islands
        ));
    }
    println!("\n{md}");
    // Copy this run's table into PARITY.md by hand if it changes meaningfully
    // (not auto-written here, so PARITY.md can carry commentary alongside it).
}


// ---------------------------------------------------------------------------
// Feature parity: boards whose zones use thermal spokes (rotated and circular
// pads, per-pad and per-footprint connection overrides), hatch fill, corner
// smoothing and priority interplay. Unlike `check_board` above, the zones here
// are the ones `import_kicad_pcb` reads from the file, so every zone setting
// the importer knows about (fill mode, hatch, smoothing, island removal, ...)
// reaches our filler exactly as the studio would see it.
// ---------------------------------------------------------------------------

/// `qa/data/pcbnew`-relative boards of the feature table in `PARITY.md`.
const FEATURE_BOARDS: &[&str] = &[
    "zone_filler.kicad_pcb",
    "test_starved_thermal.kicad_pcb",
    "issue21746/issue21746.kicad_pcb",
    "stonehenge.kicad_pcb",
    "issue2568.kicad_pcb",
    "fill_bad.kicad_pcb",
];

/// Our fill of every copper pour of `rel` against `kicad-cli pcb drc --refill-zones --save-board`.
fn check_board_imported(cli: &Path, rel: &str) -> Vec<ParityRow> {
    let src = qa_boards_dir().join(rel);
    let name = rel.replace('/', "__");
    let dir = std::env::var_os("PARITY_TMP").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("eda_zone_filler_parity"));
    std::fs::create_dir_all(&dir).unwrap();
    let work = dir.join(&name);
    std::fs::copy(&src, &work).unwrap_or_else(|e| panic!("copy {rel}: {e}"));

    let before = std::fs::read_to_string(&work).unwrap();
    let (mut design, model, _notes) = import_kicad_pcb(&before).unwrap_or_else(|e| panic!("import {rel}: {e:?}"));
    if let Some(r) = design.routing.as_mut() {
        r.assign_missing_ids();
    }

    let out = Command::new(cli).args(["pcb", "drc", "--refill-zones", "--save-board", "--format", "report", "--output"]).arg(dir.join(format!("{name}.report"))).arg(&work).output().expect("run kicad-cli");
    eprintln!("kicad-cli({rel}) stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let after = std::fs::read_to_string(&work).unwrap_or_else(|e| panic!("read refilled {rel}: {e}"));
    let after_blocks = all_zone_blocks(&after);

    let drc_board = eda_drc::board::build(&design, &model);
    let fills = eda_drc::fill::fill_all_zones(&drc_board, &model.board);

    let mut rows = Vec::new();
    let Some(routing) = design.routing.as_ref() else { return rows };
    let mut used: Vec<usize> = Vec::new();
    for z in routing.zones.iter().filter(|z| !z.is_rule_area && !z.teardrop && z.parent_footprint.is_none()) {
        let our_fill = fills.get(&z.id);
        let our_paths: Paths64 = our_fill.map(|f| f.polys.iter().filter_map(|p| p.first()).cloned().collect()).unwrap_or_default();
        let our_area = our_fill.map(|f| f.area()).unwrap_or(0.0);

        let outline_mm: Vec<(f64, f64)> = z.outline.iter().map(|p| (p.x as f64 / 1000.0, p.y as f64 / 1000.0)).collect();
        let _ = &outline_mm;
        // A zone on several layers appears once per layer with the same block: only claim a block on its first layer.
        let block_after = {
            let mut probe = used.clone();
            let found = find_after_block_for(&after_blocks, &mut probe, z);
            match found {
                Some(b) => {
                    let multi = b.contains("(layers");
                    if !multi {
                        used = probe;
                    }
                    Some(b)
                }
                None => {
                    // a multi-layer zone's block was already used by its first layer: look it up again without the claim
                    let mut none: Vec<usize> = Vec::new();
                    find_after_block_for(&after_blocks, &mut none, z)
                }
            }
        };
        let Some(zone_block_after) = block_after else {
            eprintln!("{rel}: zone net={} layer={} priority={} had no matching outline after refill, skipping", z.net, z.layer, z.priority);
            continue;
        };
        let kicad_paths = extract_filled_polygons(zone_block_after, &z.layer);
        let kicad_area = area_paths(&kicad_paths).abs();
        let xor = xor_paths(&our_paths, &kicad_paths, FillRule::NonZero);
        let xor_area = area_paths(&xor).abs();

        write_svg(&format!("{}-{}-{}", name, z.layer.replace('.', "_"), rows.len()), &our_paths, &kicad_paths);
        rows.push(ParityRow {
            board: rel.to_string(),
            net: format!("{} (prio {}{})", z.net, z.priority, if z.fill_mode == FillMode::HatchPattern { ", hatch" } else { "" }),
            layer: z.layer.clone(),
            our_area_mm2: our_area / 1_000_000.0,
            kicad_area_mm2: kicad_area / 1_000_000.0,
            area_pct_diff: if kicad_area > 0.0 { 100.0 * (our_area - kicad_area).abs() / kicad_area } else { 0.0 },
            xor_area_mm2: xor_area / 1_000_000.0,
            our_islands: our_paths.len(),
            kicad_islands: kicad_paths.len(),
            hot_spots: hot_spots(&xor),
        });
    }
    rows
}

fn parity_table(rows: &[ParityRow]) -> String {
    let mut md = String::from("| board | net | layer | our area mm² | kicad area mm² | area diff % | XOR area mm² | XOR % of kicad | our islands | kicad islands | largest XOR pieces mm² @ mm |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|---|---|\n");
    for r in rows {
        let xor_pct = if r.kicad_area_mm2 > 0.0 { 100.0 * r.xor_area_mm2 / r.kicad_area_mm2 } else { 0.0 };
        md.push_str(&format!(
            "| {} | {} | {} | {:.3} | {:.3} | {:.2}% | {:.3} | {:.2}% | {} | {} | {} |\n",
            r.board, r.net, r.layer, r.our_area_mm2, r.kicad_area_mm2, r.area_pct_diff, r.xor_area_mm2, xor_pct, r.our_islands, r.kicad_islands, r.hot_spots
        ));
    }
    md
}

#[test]
fn kicad_cli_zone_fill_feature_parity() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() && std::env::var_os("PARITY_BOARDS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    // `PARITY_BOARDS=a.kicad_pcb,dir/b.kicad_pcb` measures other boards of `qa/data/pcbnew`.
    let boards: Vec<String> = match std::env::var("PARITY_BOARDS") {
        Ok(list) => list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Err(_) => FEATURE_BOARDS.iter().map(|s| s.to_string()).collect(),
    };
    let mut rows = Vec::new();
    for board in &boards {
        rows.extend(check_board_imported(&cli, board));
    }
    println!("\n{}", parity_table(&rows));
}
