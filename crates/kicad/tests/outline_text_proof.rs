//! UI-LEFT-3 C3-3, C3-8 (slow tier, `EDA_SLOW_TESTS=1`): a board text in an outline face as the server lays it out (`eda_model::text_shape`, `OUTLINE_FONT`'s
//! glyphs read from the font file) is the one kicad-cli plots. kicad-cli plots an outline text from its render cache (`BRDITEMS_PLOTTER::PlotText`: one filled
//! region per glyph, its holes fractured in, and each overbar a line at the pen), so each glyph's region is matched to the glyph polygon nearest it: their
//! boxes agree within 0.012 mm (FreeType's hinting at 358 pixels to the em, which is not ported, moves a point by up to half a pixel there), their areas
//! within 0.5 %, and an overbar's ends (the cursor's sum of HarfBuzz's advances) to the nanometre. Kerned pairs, a tab, a subscript, a turned and a mirrored
//! text, a fake bold (IPAGothic has no bold: `FT_Outline_Embolden`) and a fake italic (this machine's DejaVu Sans has no oblique: the 12 degree slant).

use eda_model::text_shape::{shape, TextShape};
use std::path::PathBuf;

fn kicad_cli() -> PathBuf {
    std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"))
}

fn area(p: &[[i64; 2]]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i][0] as f64 * p[(i + 1) % n][1] as f64 - p[(i + 1) % n][0] as f64 * p[i][1] as f64).sum::<f64>().abs() / 2.0
}

fn bbox(p: &[[i64; 2]]) -> [i64; 4] {
    let xs = p.iter().map(|q| q[0]);
    let ys = p.iter().map(|q| q[1]);
    [xs.clone().min().unwrap(), ys.clone().min().unwrap(), xs.max().unwrap(), ys.max().unwrap()]
}

/// The regions (`G36` .. `G37`) and the drawn lines (`D02` then `D01`) of a Gerber file, nanometres, y down as the board has it.
fn gerber(path: &std::path::Path) -> (Vec<Vec<[i64; 2]>>, Vec<[[i64; 2]; 2]>) {
    let text = std::fs::read_to_string(path).unwrap();
    let (mut regions, mut lines) = (Vec::new(), Vec::new());
    let mut cur: Option<Vec<[i64; 2]>> = None;
    let mut at = [0i64, 0];
    for l in text.lines() {
        if l.starts_with("G36") {
            cur = Some(Vec::new());
        } else if l.starts_with("G37") {
            regions.extend(cur.take());
        } else if let Some(rest) = l.strip_prefix('X') {
            let (x, rest) = rest.split_once('Y').unwrap();
            let y: String = rest.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
            let p = [x.parse::<i64>().unwrap(), -y.parse::<i64>().unwrap()];
            match &mut cur {
                Some(r) => r.push(p),
                None if l.ends_with("D01*") => lines.push([at, p]),
                None => {}
            }
            at = p;
        }
    }
    (regions, lines)
}

#[test]
fn outline_texts_are_the_glyphs_kicad_cli_plots() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    if !std::path::Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").exists() {
        eprintln!("skipped: no DejaVu Sans");
        return;
    }
    // (text, at x, y, angle, face, bold, italic, size w h, justify, mirror)
    let texts: Vec<TextShape> = vec![
        TextShape { text: "AVo~{xy}g\tW_{2}".into(), pos: [100_000_000, 50_000_000], size: [1_500_000, 2_000_000], thickness: 300_000, angle: 30.0, h_align: -1, v_align: 1, line_spacing: 1.0, face: Some("DejaVu Sans".into()), ..Default::default() },
        TextShape { text: "To Wa".into(), pos: [100_000_000, 70_000_000], size: [2_000_000, 2_000_000], thickness: 300_000, angle: 0.0, h_align: 0, v_align: 0, line_spacing: 1.0, face: Some("Liberation Serif".into()), ..Default::default() },
        TextShape { text: "Hb".into(), pos: [130_000_000, 50_000_000], size: [2_000_000, 2_000_000], thickness: 300_000, bold: true, h_align: 1, v_align: -1, line_spacing: 1.0, face: Some("IPAGothic".into()), ..Default::default() },
        TextShape { text: "Wy".into(), pos: [130_000_000, 70_000_000], size: [2_000_000, 2_000_000], thickness: 300_000, italic: true, mirror: true, angle: 90.0, h_align: 0, v_align: 0, line_spacing: 1.0, face: Some("DejaVu Sans".into()), ..Default::default() },
    ];
    let mut board = String::from("(kicad_pcb (version 20240108) (generator \"pcbnew\") (generator_version \"8.0\")\n  (general (thickness 1.6) (legacy_teardrops no))\n  (paper \"A4\")\n  (layers (0 \"F.Cu\" signal) (31 \"B.Cu\" signal) (37 \"F.SilkS\" user \"F.Silkscreen\") (44 \"Edge.Cuts\" user))\n  (setup (pad_to_mask_clearance 0))\n  (net 0 \"\")\n");
    let mm = |v: i64| format!("{}", v as f64 / 1e6);
    for t in &texts {
        let h = match t.h_align {
            -1 => " left",
            1 => " right",
            _ => "",
        };
        let v = match t.v_align {
            -1 => " top",
            1 => " bottom",
            _ => "",
        };
        let flags = format!("{}{}", if t.bold { " (bold yes)" } else { "" }, if t.italic { " (italic yes)" } else { "" });
        board.push_str(&format!(
            "  (gr_text \"{}\" (at {} {} {}) (layer \"F.SilkS\")\n    (effects (font (face \"{}\") (size {} {}) (thickness {}){flags}) (justify{h}{v}{})))\n",
            t.text.replace('\t', "\\t"),
            mm(t.pos[0]),
            mm(t.pos[1]),
            t.angle,
            t.face.as_deref().unwrap(),
            mm(t.size[1]),
            mm(t.size[0]),
            mm(t.thickness),
            if t.mirror { " mirror" } else { "" }
        ));
    }
    board.push_str(")\n");
    let dir = std::env::temp_dir().join(format!("eda-outline-text-proof-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.kicad_pcb");
    std::fs::write(&path, &board).unwrap();
    let out = dir.join("out");
    let o = std::process::Command::new(kicad_cli()).args(["pcb", "export", "gerbers", "--layers", "F.SilkS", "--no-protel-ext", "-o"]).arg(&out).arg(&path).output().expect("kicad-cli runs");
    assert!(o.status.success(), "kicad-cli pcb export gerbers: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let gbr = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).find(|p| p.to_string_lossy().contains("SilkS") || p.to_string_lossy().contains("Silkscreen")).expect("the silkscreen's Gerber");
    let (regions, lines) = gerber(&gbr);
    let mut polys = Vec::new();
    let mut bars = Vec::new();
    for t in &texts {
        let s = shape(t).expect("a shape");
        assert!(s.outline, "{} is in an outline font", t.text);
        polys.extend(s.polys);
        bars.extend(s.segments);
    }
    assert_eq!(regions.len(), polys.len(), "a region per glyph");
    let mut worst = (0i64, 0.0f64);
    for r in &regions {
        let b = bbox(r);
        let p = polys.iter().min_by_key(|p| bbox(&p.outline).iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum::<i64>()).unwrap();
        let ob = bbox(&p.outline);
        let d = ob.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).max().unwrap();
        let ours = area(&p.outline) - p.holes.iter().map(|h| area(h)).sum::<f64>();
        let rel = (ours - area(r)).abs() / area(r);
        worst = (worst.0.max(d), worst.1.max(rel));
        assert!(d <= 12_000, "a glyph's box off by {d} nm: ours {ob:?}, kicad-cli's {b:?}");
        assert!(rel < 0.005, "a glyph's area off by {:.3} %", rel * 100.0);
    }
    eprintln!("{} glyphs: boxes within {} nm, areas within {:.3} %", regions.len(), worst.0, worst.1 * 100.0);
    assert_eq!(bars.len(), 1);
    assert!(lines.iter().any(|l| *l == bars[0] || *l == [bars[0][1], bars[0][0]]), "the overbar to the nanometre: ours {:?}, kicad-cli's {lines:?}", bars[0]);
    let _ = std::fs::remove_dir_all(&dir);
}
