//! KiCad 10's line endings (file version 20260818, "Line ending shapes"): the arrowhead, open arrowhead, circle or square at the start or the end of an open graphic shape
//! -- a line, an arc or a curve (`LINE_ENDING`, `include/line_ending.h`, `common/line_ending.cpp` of KiCad 25ec2335; the 8303b2ad source the rest of the port reads predates them).
//!
//! A board shape's endings are kept with its other looks, by the shape's id ([`crate::pcb_items::ShapeStyle`]), and a footprint graphic's beside it
//! ([`crate::fp_art::FpShape`]). The file has them as `(start_shape arrow (length 3) (width 3) (stroke (width 0.6) (type default)))` and `(end_shape ..)` after the
//! shape's `(stroke ..)` (`LINE_ENDING::Format`); a style of `none` is not written, so a [`LineEnding`] of [`LineEndingStyle::None`] is what an absent one is.
//!
//! The geometry (the size an automatic ending takes, how far it shortens the line, its outline) is the canvas's: `web/studio/src/kicad-port/lineEnding.ts`. The
//! clearance and the zone knock-out of an ending on copper are kicad-cli's.

use serde::{Deserialize, Serialize};

use crate::ir::Nm;
use crate::pcb_items::LineStyle;

/// `LINE_ENDING_STYLE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEndingStyle {
    #[default]
    None,
    Arrow,
    Circle,
    Square,
    ArrowOpen,
}

impl LineEndingStyle {
    /// `LINE_ENDING::StyleToToken`.
    pub fn token(self) -> &'static str {
        match self {
            LineEndingStyle::None => "none",
            LineEndingStyle::Arrow => "arrow",
            LineEndingStyle::Circle => "circle",
            LineEndingStyle::Square => "square",
            LineEndingStyle::ArrowOpen => "arrow_open",
        }
    }

    /// `PCB_IO_KICAD_SEXPR_PARSER::parseLineEnding`'s first token; `None` for a word the parser expects none of ("arrow, circle, square, arrow_open, or none").
    pub fn from_token(t: &str) -> Option<LineEndingStyle> {
        Some(match t {
            "none" => LineEndingStyle::None,
            "arrow" => LineEndingStyle::Arrow,
            "circle" => LineEndingStyle::Circle,
            "square" => LineEndingStyle::Square,
            "arrow_open" => LineEndingStyle::ArrowOpen,
            _ => return None,
        })
    }
}

fn is_zero(v: &Nm) -> bool {
    *v == 0
}

/// One `LINE_ENDING`: its style, its size along the line (`length_nm`) and across it (`width_nm`), 0 being automatic (`DEFAULT_RATIO_LENGTH` and `DEFAULT_RATIO_WIDTH`
/// times the line's width, or the other size when one is set: `GetEffectiveSize`), and the width of its outline (`stroke_width_nm`; 0 is a sharp, filled shape) with
/// the outline's line type, which the file carries and nothing draws.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineEnding {
    #[serde(default)]
    pub style: LineEndingStyle,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "length_um", deserialize_with = "crate::units::de")]
    pub length_nm: Nm,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "stroke_width_um", deserialize_with = "crate::units::de")]
    pub stroke_width_nm: Nm,
    #[serde(default, skip_serializing_if = "LineStyle::is_default")]
    pub stroke_style: LineStyle,
}

impl LineEnding {
    pub fn new(style: LineEndingStyle) -> LineEnding {
        LineEnding { style, ..LineEnding::default() }
    }

    /// `LINE_ENDING()`: no ending.
    pub fn is_none(&self) -> bool {
        self.style == LineEndingStyle::None
    }

    /// `LINE_ENDING::SetLength`, `SetWidth`, `SetStrokeWidth` as `DIALOG_SHAPE_PROPERTIES::TransferDataFromWindow` calls them: a negative size is 0 (`std::max( 0, .. )`).
    pub fn clamped(mut self) -> LineEnding {
        self.length_nm = self.length_nm.max(0);
        self.width_nm = self.width_nm.max(0);
        self.stroke_width_nm = self.stroke_width_nm.max(0);
        self
    }

    /// `LINE_ENDING::Format( aOut, pcbIUScale, aToken )`: `(<token> <style> [(length ..)] [(width ..)] [(stroke (width ..) (type ..))])`, or nothing for no ending.
    /// `mm` formats nanometres as KiCad's `FormatInternalUnits` does.
    pub fn format(&self, token: &str, mm: impl Fn(Nm) -> String) -> String {
        if self.is_none() {
            return String::new();
        }
        let mut out = format!("({token} {}", self.style.token());
        if self.length_nm > 0 {
            out.push_str(&format!(" (length {})", mm(self.length_nm)));
        }
        if self.width_nm > 0 {
            out.push_str(&format!(" (width {})", mm(self.width_nm)));
        }
        if self.stroke_width_nm > 0 {
            out.push_str(&format!(" (stroke (width {}) (type {}))", mm(self.stroke_width_nm), self.stroke_style.token()));
        }
        out.push(')');
        out
    }
}

/// The two endings of one shape, as the shape dialog and the Properties panel edit them together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineEndings {
    #[serde(default, skip_serializing_if = "LineEnding::is_none")]
    pub start: LineEnding,
    #[serde(default, skip_serializing_if = "LineEnding::is_none")]
    pub end: LineEnding,
}

impl LineEndings {
    pub fn is_none(&self) -> bool {
        self.start.is_none() && self.end.is_none()
    }

    /// Both endings as the file writes them after the shape's stroke: `start_shape` then `end_shape`, each preceded by a space when there is one.
    pub fn format(&self, mm: impl Fn(Nm) -> String) -> String {
        let mut out = String::new();
        for (e, token) in [(&self.start, "start_shape"), (&self.end, "end_shape")] {
            let s = e.format(token, &mm);
            if !s.is_empty() {
                out.push(' ');
                out.push_str(&s);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm(v: Nm) -> String {
        let s = format!("{:.6}", v as f64 / 1000.0);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }

    #[test]
    fn an_ending_is_written_as_line_ending_format_writes_it() {
        assert_eq!(LineEnding::new(LineEndingStyle::None).format("start_shape", mm), "", "no ending, no token");
        assert_eq!(LineEnding::new(LineEndingStyle::Arrow).format("start_shape", mm), "(start_shape arrow)", "an automatic size says none");
        let e = LineEnding { style: LineEndingStyle::ArrowOpen, length_nm: 3000, width_nm: 3000, stroke_width_nm: 600, stroke_style: LineStyle::Default };
        assert_eq!(e.format("end_shape", mm), "(end_shape arrow_open (length 3) (width 3) (stroke (width 0.6) (type default)))");
        let both = LineEndings { start: LineEnding { width_nm: 1500, ..LineEnding::new(LineEndingStyle::Square) }, end: LineEnding::new(LineEndingStyle::Circle) };
        assert_eq!(both.format(mm), " (start_shape square (width 1.5)) (end_shape circle)");
    }

    #[test]
    fn every_style_reads_back_from_its_token() {
        for s in [LineEndingStyle::None, LineEndingStyle::Arrow, LineEndingStyle::Circle, LineEndingStyle::Square, LineEndingStyle::ArrowOpen] {
            assert_eq!(LineEndingStyle::from_token(s.token()), Some(s));
        }
        assert_eq!(LineEndingStyle::from_token("diamond"), None);
    }
}
