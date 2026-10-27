//! VLM judge: a hosted vision model looks at a picture of each stage and
//! returns a rubric-versioned verdict. It only ever sees gate-clean
//! candidates (the caller's job); its defects become CheckResults so the
//! loop treats them like any other gate, and every verdict is logged as
//! preference data for a trained critic later.
//!
//! The picture is KiCad's: `kicad-cli sch export svg|png` of the derived
//! schematic, `kicad-cli pcb export svg|png` of the derived board
//! (`eda_kicad_engine::pictures`), so the judge sees what KiCad draws. The
//! hand-drawn pictures this crate and `eda-render` used to make
//! ([`render_stage_svg`], [`render_board_svg`]) are INACTIVE, kept for a
//! future all-Rust build behind `EDA_RUST_PICTURES=1`.
//!
//! No fallback: without `ANTHROPIC_API_KEY`, kicad-cli (or, for a kicad-cli
//! too old to plot PNG, `rsvg-convert`), or a parseable verdict the judge
//! FAILS (`judge_unavailable`), it never passes silently.

use base64::Engine;
use eda_model::footprint::{placed_courtyard, placed_pads};
use eda_model::ir::Design;
use eda_model::{CheckResult, CheckStatus, ConstraintModel};
use eda_model::units::{mm, Nm};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 2: the pictures are kicad-cli's plots (KiCad's symbols, colours and
/// drawing sheet), no longer the hand-drawn ones rubric 1 was written for.
pub const RUBRIC_VERSION: &str = "judge-rubric-2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Schematic,
    Placement,
    Routing,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Schematic => "schematic",
            Stage::Placement => "placement",
            Stage::Routing => "routing",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defect {
    pub check: String,
    pub severity: String,
    #[serde(default)]
    pub location: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub stage: Stage,
    pub rubric_version: String,
    pub model: String,
    /// 0..10, 10 = a senior EE would sign it off as drawn.
    pub score: f64,
    pub summary: String,
    pub defects: Vec<Defect>,
}

#[derive(Debug, Clone)]
pub struct JudgeOptions {
    pub model: String,
    pub api_key: Option<String>,
    /// Verdict score below this is a Fail on `judge_<stage>_score`.
    pub min_score: f64,
    pub max_tokens: u32,
}

impl Default for JudgeOptions {
    fn default() -> Self {
        JudgeOptions {
            model: std::env::var("EDA_JUDGE_MODEL").unwrap_or_else(|_| "claude-sonnet-5".into()),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty()),
            min_score: std::env::var("EDA_JUDGE_MIN_SCORE").ok().and_then(|v| v.parse().ok()).unwrap_or(7.0),
            max_tokens: 1500,
        }
    }
}

fn unavailable(hint: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail("judge_unavailable", "judge", hint)]
}

// ------------------------------------------------------------- pictures
/// The picture of a stage, as the files the judge looks at.
#[derive(Debug, Clone, Default)]
pub struct StagePicture {
    /// The SVG of every page (one for a board, one per sheet for a schematic, the root first).
    pub svgs: Vec<String>,
    /// The PNG of every page, in the same order: what the vision model is sent.
    pub pngs: Vec<Vec<u8>>,
}

/// Whether the inactive hand-drawn pictures are asked for (`EDA_RUST_PICTURES=1`) instead of kicad-cli's.
pub fn rust_pictures() -> bool {
    eda_render::rust_pictures()
}

fn no_picture(stage: Stage, e: Vec<CheckResult>) -> Vec<CheckResult> {
    let why: Vec<String> = e.iter().filter_map(|c| c.hint.clone()).collect();
    unavailable(format!("no {} picture to judge: the judge looks at KiCad's plot of the design and has no fallback: {}", stage.name(), why.join("; ")))
}

/// The stage as KiCad plots it (`eda_kicad_engine::pictures`): the schematic's pages by `kicad-cli sch export svg|png`, the board by
/// `kicad-cli pcb export svg|png` (placement without the tracks, routing with them). The derived files go into `<out>/.kicad`, named `name`.
/// A kicad-cli that plots no PNG (an older nightly) has its SVG rasterised by `rsvg-convert` instead, the board on KiCad's background.
pub fn kicad_stage_picture(stage: Stage, design: &Design, model: &ConstraintModel, out: &Path, name: &str) -> Result<StagePicture, Vec<CheckResult>> {
    use eda_kicad_engine::pictures::{board_plot, plots_png, schematic_plot, PictureFormat, PCB_BACKGROUND};
    let work = out.join(".kicad");
    let read = |p: &Path| std::fs::read(p).map_err(|e| unavailable(format!("could not read {}: {e}", p.display())));
    let mut pic = StagePicture::default();
    match stage {
        Stage::Schematic => {
            let pages = schematic_plot(design, model, &work, &work.join("judge-schematic-svg"), name, PictureFormat::Svg).map_err(|e| no_picture(stage, e))?;
            for p in &pages {
                pic.svgs.push(String::from_utf8_lossy(&read(p)?).into_owned());
            }
            if plots_png("sch") {
                for p in schematic_plot(design, model, &work, &work.join("judge-schematic-png"), name, PictureFormat::Png).map_err(|e| no_picture(stage, e))? {
                    pic.pngs.push(read(&p)?);
                }
            } else {
                for svg in &pic.svgs {
                    pic.pngs.push(svg_to_png(svg, 1600, None)?);
                }
            }
        }
        Stage::Placement | Stage::Routing => {
            let routing = stage == Stage::Routing;
            let svg = board_plot(design, model, &work, &work.join(format!("judge-{}.svg", stage.name())), name, PictureFormat::Svg, routing).map_err(|e| no_picture(stage, e))?;
            pic.svgs.push(String::from_utf8_lossy(&read(&svg)?).into_owned());
            if plots_png("pcb") {
                let png = board_plot(design, model, &work, &work.join(format!("judge-{}.png", stage.name())), name, PictureFormat::Png, routing).map_err(|e| no_picture(stage, e))?;
                pic.pngs.push(read(&png)?);
            } else {
                pic.pngs.push(svg_to_png(&pic.svgs[0], 1600, Some(PCB_BACKGROUND))?);
            }
        }
    }
    Ok(pic)
}

/// INACTIVE (2026-10-28): replaced by [`kicad_stage_picture`] (kicad-cli `sch export svg` / `pcb export svg`) per the user's port-vs-reuse rule;
/// kept for a future all-Rust build, and used only with `EDA_RUST_PICTURES=1`.
///
/// Render the stage the judge should look at. Schematic uses the real
/// renderer; placement/routing use a plain board view (outline, courtyards
/// with refdes, pads, tracks, vias) so the judge sees exactly our geometry.
pub fn render_stage_svg(stage: Stage, design: &Design, model: &ConstraintModel) -> Result<String, Vec<CheckResult>> {
    match stage {
        Stage::Schematic => eda_render::render_schematic(design, model),
        Stage::Placement | Stage::Routing => render_board_svg(design, model, stage == Stage::Routing),
    }
}

/// INACTIVE (2026-10-28): replaced by kicad-cli `pcb export svg` ([`kicad_stage_picture`]) per the user's port-vs-reuse rule; kept for a
/// future all-Rust build, and used only with `EDA_RUST_PICTURES=1`.
pub fn render_board_svg(design: &Design, model: &ConstraintModel, with_routing: bool) -> Result<String, Vec<CheckResult>> {
    let pl = design.placement.as_ref().ok_or_else(|| unavailable("design has no placement to render"))?;
    let (mut x0, mut y0, mut x1, mut y1) = (Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN);
    for p in &pl.outline {
        x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y);
    }
    if x0 == Nm::MAX {
        return Err(unavailable("placement has no outline"));
    }
    let m = mm(2.0);
    let (vx, vy, vw, vh) = (x0 - m, y0 - m, x1 - x0 + m * 2, y1 - y0 + m * 2);
    let mut s = String::new();
    s.push_str(&format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{vx} {vy} {vw} {vh}\" width=\"{}\" height=\"{}\">\n", vw / 20_000, vh / 20_000));
    s.push_str("<rect x=\"-1e12\" y=\"-1e12\" width=\"2e12\" height=\"2e12\" fill=\"#0d1b12\"/>\n");
    let pts: Vec<String> = pl.outline.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
    s.push_str(&format!("<polygon points=\"{}\" fill=\"#123d1f\" stroke=\"#f2d34b\" stroke-width=\"150000\"/>\n", pts.join(" ")));
    let fs = model.board.refdes_font(&pl.outline);
    for fp in &pl.footprints {
        let part = match model.part(&fp.id) { Some(p) => p, None => continue };
        if let Some((cx0, cy0, cx1, cy1)) = placed_courtyard(model, part, fp) {
            s.push_str(&format!("<rect x=\"{cx0}\" y=\"{cy0}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#c8c8c8\" stroke-width=\"60000\" stroke-dasharray=\"300000 200000\"/>\n", cx1 - cx0, cy1 - cy0));
            let bx = eda_model::footprint::refdes_box_side((cx0, cy0, cx1, cy1), &fp.id, fs, eda_model::footprint::outline_top(&pl.outline), fp.label);
            s.push_str(&format!("<text x=\"{}\" y=\"{}\" font-size=\"{fs}\" fill=\"#ffffff\" text-anchor=\"middle\" font-family=\"sans-serif\">{}</text>\n", (bx.0 + bx.2) / 2, eda_model::footprint::refdes_baseline(bx, fs), fp.id));
        }
        if let Some(pads) = placed_pads(model, part, fp) {
            for pad in pads {
                let (w, h) = pad.size;
                let fill = if pad.through_hole { "#d7b96a" } else { "#b87333" };
                s.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>\n", pad.center.x - w / 2, pad.center.y - h / 2));
            }
        }
    }
    if with_routing {
        if let Some(rt) = &design.routing {
            for t in &rt.tracks {
                let color = if t.layer.starts_with('F') { "#e0453a" } else { "#3f74d6" };
                let pts: Vec<String> = t.pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
                s.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" opacity=\"0.9\"/>\n", pts.join(" "), t.width));
            }
            for v in &rt.vias {
                s.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#9a9a9a\"/><circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#0d1b12\"/>\n", v.at.x, v.at.y, v.diameter / 2, v.at.x, v.at.y, v.drill / 2));
            }
        }
    }
    s.push_str("</svg>\n");
    Ok(s)
}

/// SVG → PNG via rsvg-convert (the same tool the dev review loop uses), on `background` (`-b`) when one is given: for the inactive
/// hand-drawn pictures, and for a kicad-cli too old to plot PNG itself.
pub fn svg_to_png(svg: &str, width_px: u32, background: Option<&str>) -> Result<Vec<u8>, Vec<CheckResult>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("rsvg-convert");
    cmd.args(["-w", &width_px.to_string()]);
    if let Some(bg) = background {
        cmd.args(["-b", bg]);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| unavailable(format!("rsvg-convert not runnable: {e}")))?;
    child.stdin.take().unwrap().write_all(svg.as_bytes()).map_err(|e| unavailable(e.to_string()))?;
    let out = child.wait_with_output().map_err(|e| unavailable(e.to_string()))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(unavailable(format!("rsvg-convert failed: {}", String::from_utf8_lossy(&out.stderr))));
    }
    Ok(out.stdout)
}

// ------------------------------------------------------------- rubrics
pub fn rubric(stage: Stage) -> &'static str {
    match stage {
        Stage::Schematic => "You are a senior electronics engineer reviewing a SCHEMATIC drawing for readability, as a reviewer who will sign it off. Judge only what is visible. Rubric (each item is a defect if violated): \
1. Every reference designator and value sits immediately next to its own symbol and is unambiguous. \
2. No text is drawn over a wire, another text, or a symbol. \
3. Wires are short and direct; no net wanders around the sheet or loops back; a two-pin connection should be near-straight. \
4. Power and ground nets use power flags / ground symbols, not long drawn wires; supply at top, ground at bottom. \
5. Signal flow left to right; inputs/connectors on the left, outputs on the right. \
6. Related parts are grouped (decoupling caps next to their IC pin, feedback networks next to the op-amp). \
7. Few wire crossings; no four-way junctions ambiguity; junction dots where wires join. \
8. Symbols are standard and recognisable; pin names readable.",
        Stage::Placement => "You are a senior PCB layout engineer reviewing a component PLACEMENT (KiCad's board view: outline, courtyards in magenta, silkscreen with refdes in yellow, pads; no traces yet). Rubric: \
1. Decoupling capacitors sit right next to the IC pins they serve. \
2. Connectors are at the board edge with their pins facing outward. \
3. Parts connected to each other are close; no part stranded far from everything it talks to. \
4. Even use of the board; no needless crowding in one corner while another is empty, unless intent demands it. \
5. Orientations are consistent (passives aligned in rows, same rotation where possible). \
6. Enough room between parts for routing and assembly; nothing crammed against the outline. \
7. Thermal/high-current parts have space and are not boxed in.",
        Stage::Routing => "You are a senior PCB layout engineer reviewing ROUTED copper in KiCad's colours (red = top copper, blue = bottom copper, round pads with a drilled hole on both = vias, magenta outlines = courtyards, yellow = silkscreen). Rubric: \
1. Traces are short and direct; no detours, staircases, or loops. \
2. Few vias; two-pin nets should rarely need a via. \
3. No trace runs between pads of a part or under a part without reason. \
4. No acid traps / acute angles; clean 90° or 45° corners. \
5. Traces do not hug the board edge. \
6. Layer usage sensible: one layer predominantly one direction on dense boards. \
7. Power/ground routing is wider / uses pours where appropriate.",
    }
}

fn prompt(stage: Stage, context: &str) -> String {
    format!(
        "{}\n\nContext about the design:\n{}\n\nReturn ONLY a JSON object, no prose, of the shape: {{\"score\": <0-10 number, 10 = sign-off ready>, \"summary\": \"<one sentence>\", \"defects\": [{{\"check\": \"<snake_case_short_name>\", \"severity\": \"fail\"|\"warn\", \"location\": \"<refdes/net/area>\", \"hint\": \"<what is wrong and how to fix>\"}}]}}. Use severity fail for anything a reviewer would send back; warn for nits. Be specific: name the refdes or net.",
        rubric(stage), context
    )
}

fn design_context(stage: Stage, design: &Design, model: &ConstraintModel) -> String {
    let parts: Vec<String> = model.parts.iter().map(|p| format!("{} ({}{})", p.reference, p.value.as_deref().unwrap_or(""), p.package.as_ref().map(|k| format!(", {k}")).unwrap_or_default())).collect();
    let nets: Vec<String> = model.nets.iter().map(|n| format!("{}[{}]", n.name, n.pins.len())).collect();
    let mut cx = format!("stage: {}\nparts: {}\nnets: {}", stage.name(), parts.join(", "), nets.join(", "));
    if let Some(r) = &design.routing {
        cx.push_str(&format!("\ntracks: {}, vias: {}", r.tracks.len(), r.vias.len()));
    }
    cx
}

// ------------------------------------------------------------- the call
/// Ask the vision model for a verdict on `png`.
pub fn judge_png(stage: Stage, png: &[u8], context: &str, o: &JudgeOptions) -> Result<Verdict, Vec<CheckResult>> {
    judge_pngs(stage, &[png.to_vec()], context, o)
}

/// Ask the vision model for a verdict on the pages `pngs` (a schematic's sheets, the root first), sent as one message.
pub fn judge_pngs(stage: Stage, pngs: &[Vec<u8>], context: &str, o: &JudgeOptions) -> Result<Verdict, Vec<CheckResult>> {
    let key = o.api_key.clone().ok_or_else(|| unavailable("ANTHROPIC_API_KEY not set; the judge has no fallback"))?;
    let mut content: Vec<serde_json::Value> = pngs
        .iter()
        .map(|png| serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png)}}))
        .collect();
    content.push(serde_json::json!({"type": "text", "text": prompt(stage, context)}));
    let body = serde_json::json!({
        "model": o.model,
        "max_tokens": o.max_tokens,
        "messages": [{
            "role": "user",
            "content": content
        }]
    });
    let resp = ureq::post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body);
    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let t = r.into_string().unwrap_or_default();
            return Err(unavailable(format!("API HTTP {code}: {}", t.chars().take(400).collect::<String>())));
        }
        Err(e) => return Err(unavailable(format!("API transport: {e}"))),
    };
    let v: serde_json::Value = resp.into_json().map_err(|e| unavailable(format!("API response not JSON: {e}")))?;
    let text = v["content"].as_array().and_then(|c| c.iter().find_map(|b| b["text"].as_str().map(str::to_string))).ok_or_else(|| unavailable("API response had no text block"))?;
    let json_str = extract_json(&text).ok_or_else(|| unavailable(format!("judge did not return JSON: {}", text.chars().take(300).collect::<String>())))?;
    #[derive(Deserialize)]
    struct Raw { score: f64, #[serde(default)] summary: String, #[serde(default)] defects: Vec<Defect> }
    let raw: Raw = serde_json::from_str(json_str).map_err(|e| unavailable(format!("judge JSON malformed: {e}")))?;
    Ok(Verdict { stage, rubric_version: RUBRIC_VERSION.into(), model: o.model.clone(), score: raw.score, summary: raw.summary, defects: raw.defects })
}

fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// The file of page `i` of a stage's picture: `judge_<stage>.<ext>` for the first (the root sheet, the board), `judge_<stage>_<n>.<ext>` for
/// page n = 2, 3, ... of a hierarchical schematic.
fn picture_file(out: &Path, stage: Stage, i: usize, ext: &str) -> std::path::PathBuf {
    if i == 0 {
        out.join(format!("judge_{}.{ext}", stage.name()))
    } else {
        out.join(format!("judge_{}_{}.{ext}", stage.name(), i + 1))
    }
}

/// Full path: picture → verdict → CheckResults. Also writes
/// `<out>/judge_<stage>.{svg,png,json}` (and `judge_<stage>_<n>.{svg,png}`
/// for a schematic's further sheets) so the review page can show what the
/// judge saw. The pictures are KiCad's ([`kicad_stage_picture`], the derived
/// files named `name`), or the inactive hand-drawn ones with
/// `EDA_RUST_PICTURES=1`.
pub fn judge_stage(stage: Stage, design: &Design, model: &ConstraintModel, out: &Path, name: &str, o: &JudgeOptions) -> Result<(Verdict, Vec<CheckResult>), Vec<CheckResult>> {
    let pic = if rust_pictures() {
        let svg = render_stage_svg(stage, design, model)?;
        let png = svg_to_png(&svg, 1600, None)?;
        StagePicture { svgs: vec![svg], pngs: vec![png] }
    } else {
        kicad_stage_picture(stage, design, model, out, name)?
    };
    // A page an earlier run wrote that this one has not (a sheet since removed) must not pass for this run's.
    for ext in ["svg", "png"] {
        let mut i = pic.svgs.len().max(pic.pngs.len());
        while picture_file(out, stage, i, ext).exists() {
            let _ = std::fs::remove_file(picture_file(out, stage, i, ext));
            i += 1;
        }
    }
    for (i, svg) in pic.svgs.iter().enumerate() {
        let _ = std::fs::write(picture_file(out, stage, i, "svg"), svg);
    }
    for (i, png) in pic.pngs.iter().enumerate() {
        let _ = std::fs::write(picture_file(out, stage, i, "png"), png);
    }
    let verdict = judge_pngs(stage, &pic.pngs, &design_context(stage, design, model), o)?;
    let _ = std::fs::write(out.join(format!("judge_{}.json", stage.name())), serde_json::to_string_pretty(&verdict).unwrap());
    Ok((verdict.clone(), verdict_to_checks(&verdict, o)))
}

pub fn verdict_to_checks(v: &Verdict, o: &JudgeOptions) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let score_check = format!("judge_{}_score", v.stage.name());
    if v.score < o.min_score {
        out.push(CheckResult::fail(&score_check, "judge", format!("score {:.1} < {:.1}: {}", v.score, o.min_score, v.summary)));
    } else {
        out.push(CheckResult { check: score_check, status: CheckStatus::Pass, location: None, hint: Some(format!("score {:.1}: {}", v.score, v.summary)), detail: None });
    }
    for d in &v.defects {
        let status = if d.severity.eq_ignore_ascii_case("fail") { CheckStatus::Fail } else { CheckStatus::Warn };
        let name = format!("judge_{}_{}", v.stage.name(), d.check.trim().to_ascii_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
        out.push(CheckResult { check: name, status, location: Some(d.location.clone()), hint: Some(d.hint.clone()), detail: None });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_json_from_prose() {
        assert_eq!(extract_json("sure: {\"a\":1} done"), Some("{\"a\":1}"));
        assert_eq!(extract_json("no json"), None);
    }
    #[test]
    fn verdict_maps_to_checks() {
        let v = Verdict { stage: Stage::Schematic, rubric_version: RUBRIC_VERSION.into(), model: "m".into(), score: 4.0, summary: "meh".into(),
            defects: vec![Defect { check: "Label Far".into(), severity: "fail".into(), location: "C1".into(), hint: "move".into() }] };
        let o = JudgeOptions { api_key: None, ..Default::default() };
        let c = verdict_to_checks(&v, &o);
        assert_eq!(c[0].check, "judge_schematic_score");
        assert_eq!(c[0].status, CheckStatus::Fail);
        assert_eq!(c[1].check, "judge_schematic_label_far");
    }
    #[test]
    fn no_key_fails_loudly() {
        let o = JudgeOptions { api_key: None, ..Default::default() };
        let e = judge_png(Stage::Schematic, b"png", "", &o).unwrap_err();
        assert_eq!(e[0].check, "judge_unavailable");
    }
}
