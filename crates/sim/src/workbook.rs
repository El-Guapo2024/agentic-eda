//! The simulator's workbook (`.wbk`), read and written as KiCad does:
//! - written by `SIMULATOR_FRAME_UI::SaveWorkbook` (`SaveCursorToWorkbook`): version 7 JSON, `nlohmann::json` dumped with `std::setw( 2 )` and
//!   a newline after it. nlohmann keeps an object's keys in a `std::map`, so they come out sorted; [`to_kicad_text`] writes the same bytes
//!   (two-space indent, `": "`, nlohmann's escapes and its shortest round-trip doubles, `1.0` for a whole double, `1e-05` below 1e-4);
//! - read by `SIMULATOR_FRAME_UI::LoadWorkbook`: a file whose first line is `version N` or a number is a workbook of before version 7
//!   (`loadLegacyWorkbook`, `legacy_workbook.cpp`), anything else JSON (`loadJsonWorkbook`, comments allowed as nlohmann's
//!   `ignore_comments`).
//!
//! The studio's simulator panel holds the workbook as the JSON object KiCad writes ([`Workbook::to_value`], [`Workbook::from_value`]), so what
//! it sends to be saved and what it is given when it opens a file are KiCad's own keys.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// A cursor of a trace (`"cursor<n>"`): where it is and how its values are written.
#[derive(Debug, Clone, PartialEq)]
pub struct WbCursor {
    pub position: f64,
    pub x_format: String,
    pub y_format: String,
}

/// One trace of a plot tab.
#[derive(Debug, Clone, PartialEq)]
pub struct WbTrace {
    /// `SIM_TRACE_TYPE` bits.
    pub trace_type: i64,
    /// The signal as the signals list shows it (`findSignalName`: a user-defined signal's expression, not its `user<n>` vector).
    pub signal: String,
    /// `COLOR4D::ToCSSString`: `rgb(r, g, b)`.
    pub color: Option<String>,
    /// By cursor number (1, 2, then the custom cursors 3 and up).
    pub cursors: BTreeMap<i64, WbCursor>,
    /// `"cursorD"`: the difference row's formats.
    pub cursor_d: Option<(String, String)>,
}

/// The plot settings of a tab that has a plot (`SIM_PLOT_TAB`).
#[derive(Debug, Clone, PartialEq)]
pub struct WbPlot {
    pub traces: Vec<WbTrace>,
    /// `(expr, format)`.
    pub measurements: Vec<(String, String)>,
    pub dotted_secondary: bool,
    pub show_grid: bool,
    pub fixed_y1: Option<(f64, f64)>,
    pub fixed_y2: Option<(f64, f64)>,
    pub fixed_y3: Option<(f64, f64)>,
    /// Shown, at this position.
    pub legend: Option<(i64, i64)>,
    /// `(left, right, top, bottom)`.
    pub margins: Option<(i64, i64, i64, i64)>,
}

/// One analysis tab.
#[derive(Debug, Clone, PartialEq)]
pub struct WbTab {
    /// `SPICE_SIMULATOR::TypeToName( type, true )` (written; the type is the command's when read).
    pub analysis: String,
    /// The command, then `.kicad adjustpaths`, `.save all`, `.probe alli`, `.probe allp`, `.kicad esavenone` for its options.
    pub commands: Vec<String>,
    pub plot: Option<WbPlot>,
}

/// The workbook.
#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    pub version: i64,
    pub tabs: Vec<WbTab>,
    pub user_defined_signals: Vec<String>,
    /// `m_customCursorsCnt - 1`: 2 for the two cursors every workbook has.
    pub custom_cursors: i64,
    pub last_sch_text_sim_command: Option<String>,
}

impl Default for Workbook {
    fn default() -> Self {
        Workbook { version: 7, tabs: Vec::new(), user_defined_signals: Vec::new(), custom_cursors: 2, last_sch_text_sim_command: None }
    }
}

/// A tab's command and options as `commands` (`SaveWorkbook`).
pub fn commands_of(command: &str, options: u32) -> Vec<String> {
    use crate::netlist::*;
    let mut v = vec![command.to_string()];
    if options & OPTION_ADJUST_INCLUDE_PATHS != 0 {
        v.push(".kicad adjustpaths".into());
    }
    if options & OPTION_SAVE_ALL_VOLTAGES != 0 {
        v.push(".save all".into());
    }
    if options & OPTION_SAVE_ALL_CURRENTS != 0 {
        v.push(".probe alli".into());
    }
    if options & OPTION_SAVE_ALL_DISSIPATIONS != 0 {
        v.push(".probe allp".into());
    }
    if options & OPTION_SAVE_ALL_EVENTS == 0 {
        v.push(".kicad esavenone".into());
    }
    v
}

/// `loadJsonWorkbook`: a tab's command and options from `commands` (each command line trimmed on the right and the lines joined as they are).
pub fn command_and_options(commands: &[String]) -> (String, u32) {
    use crate::netlist::*;
    let mut options = OPTION_ADJUST_PASSIVE_VALS | OPTION_SAVE_ALL_EVENTS;
    let mut command = String::new();
    for c in commands {
        match c.as_str() {
            ".kicad adjustpaths" => options |= OPTION_ADJUST_INCLUDE_PATHS,
            ".save all" => options |= OPTION_SAVE_ALL_VOLTAGES,
            ".probe alli" => options |= OPTION_SAVE_ALL_CURRENTS,
            ".probe allp" => options |= OPTION_SAVE_ALL_DISSIPATIONS,
            ".kicad esavenone" => options &= !OPTION_SAVE_ALL_EVENTS,
            _ => command.push_str(c.trim_end()),
        }
    }
    (command, options)
}

fn f(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn i(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|x| x as i64))
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn min_max(v: &Value) -> Option<(f64, f64)> {
    Some((f(&v["min"])?, f(&v["max"])?))
}

impl Workbook {
    /// The workbook from KiCad's JSON (`loadJsonWorkbook`: a key that is not there is not read; a value of the wrong type is an error, as
    /// nlohmann's `type_error` is).
    pub fn from_value(js: &Value) -> Result<Workbook, String> {
        let bad = |what: &str| format!("Json type error reading the workbook: {what}");
        let mut wb = Workbook { version: js.get("version").and_then(i).unwrap_or(7), ..Workbook::default() };
        let tabs = js.get("tabs").and_then(Value::as_array).ok_or_else(|| bad("tabs"))?;
        for tab in tabs {
            let commands: Vec<String> = tab.get("commands").and_then(Value::as_array).ok_or_else(|| bad("commands"))?.iter().map(|c| s(c).ok_or_else(|| bad("a command"))).collect::<Result<_, _>>()?;
            let (command, _) = command_and_options(&commands);
            let sim = crate::command::command_to_sim_type(&command);
            let plot = if sim.is_plottable() {
                let mut p = WbPlot { traces: Vec::new(), measurements: Vec::new(), dotted_secondary: false, show_grid: true, fixed_y1: None, fixed_y2: None, fixed_y3: None, legend: None, margins: None };
                if let Some(traces) = tab.get("traces").and_then(Value::as_array) {
                    for t in traces {
                        let mut tr = WbTrace { trace_type: t.get("trace_type").and_then(i).ok_or_else(|| bad("trace_type"))?, signal: t.get("signal").and_then(s).ok_or_else(|| bad("signal"))?, color: t.get("color").and_then(s), cursors: BTreeMap::new(), cursor_d: None };
                        if let Some(d) = t.get("cursorD") {
                            tr.cursor_d = Some((d.get("x_format").and_then(s).unwrap_or_default(), d.get("y_format").and_then(s).unwrap_or_default()));
                        }
                        if let Some(o) = t.as_object() {
                            for (k, c) in o {
                                if let Some(n) = k.strip_prefix("cursor").and_then(|n| n.parse::<i64>().ok()) {
                                    tr.cursors.insert(n, WbCursor { position: c.get("position").and_then(f).ok_or_else(|| bad("a cursor's position"))?, x_format: c.get("x_format").and_then(s).unwrap_or_default(), y_format: c.get("y_format").and_then(s).unwrap_or_default() });
                                }
                            }
                        }
                        p.traces.push(tr);
                    }
                }
                if let Some(ms) = tab.get("measurements").and_then(Value::as_array) {
                    for m in ms {
                        p.measurements.push((m.get("expr").and_then(s).ok_or_else(|| bad("expr"))?, m.get("format").and_then(s).ok_or_else(|| bad("format"))?));
                    }
                }
                p.dotted_secondary = tab.get("dottedSecondary").map(|v| v.as_bool().ok_or_else(|| bad("dottedSecondary"))).transpose()?.unwrap_or(false);
                p.show_grid = tab.get("showGrid").map(|v| v.as_bool().ok_or_else(|| bad("showGrid"))).transpose()?.unwrap_or(true);
                p.fixed_y1 = tab.get("fixedY1scale").and_then(min_max);
                p.fixed_y2 = tab.get("fixedY2scale").and_then(min_max);
                p.fixed_y3 = tab.get("fixedY3scale").and_then(min_max);
                p.legend = tab.get("legend").and_then(|l| Some((i(&l["x"])?, i(&l["y"])?)));
                p.margins = tab.get("margins").and_then(|m| Some((i(&m["left"])?, i(&m["right"])?, i(&m["top"])?, i(&m["bottom"])?)));
                Some(p)
            } else {
                None
            };
            wb.tabs.push(WbTab { analysis: sim.short_name().to_string(), commands, plot });
        }
        if let Some(sigs) = js.get("user_defined_signals").and_then(Value::as_array) {
            wb.user_defined_signals = sigs.iter().map(|v| s(v).ok_or_else(|| bad("a user-defined signal"))).collect::<Result<_, _>>()?;
        }
        wb.custom_cursors = js.get("custom_cursors").and_then(i).unwrap_or(2);
        wb.last_sch_text_sim_command = js.get("last_sch_text_sim_command").and_then(s);
        Ok(wb)
    }

    /// KiCad's JSON of the workbook (`SaveWorkbook`), as a value.
    pub fn to_value(&self) -> Value {
        let mut tabs = Vec::new();
        for t in &self.tabs {
            let mut tab = json!({ "analysis": t.analysis, "commands": t.commands });
            if let Some(p) = &t.plot {
                let traces: Vec<Value> = p
                    .traces
                    .iter()
                    .map(|tr| {
                        let mut o = Map::new();
                        o.insert("trace_type".into(), json!(tr.trace_type));
                        o.insert("signal".into(), json!(tr.signal));
                        if let Some(c) = &tr.color {
                            o.insert("color".into(), json!(c));
                        }
                        for (n, c) in &tr.cursors {
                            o.insert(format!("cursor{n}"), json!({ "position": c.position, "x_format": c.x_format, "y_format": c.y_format }));
                        }
                        if let Some((x, y)) = &tr.cursor_d {
                            o.insert("cursorD".into(), json!({ "x_format": x, "y_format": y }));
                        }
                        Value::Object(o)
                    })
                    .collect();
                tab["traces"] = Value::Array(traces);
                tab["measurements"] = Value::Array(p.measurements.iter().map(|(e, fm)| json!({ "expr": e, "format": fm })).collect());
                tab["dottedSecondary"] = json!(p.dotted_secondary);
                tab["showGrid"] = json!(p.show_grid);
                for (key, y) in [("fixedY1scale", p.fixed_y1), ("fixedY2scale", p.fixed_y2), ("fixedY3scale", p.fixed_y3)] {
                    if let Some((min, max)) = y {
                        tab[key] = json!({ "min": min, "max": max });
                    }
                }
                if let Some((x, y)) = p.legend {
                    tab["legend"] = json!({ "x": x, "y": y });
                }
                if let Some((l, r, t, b)) = p.margins {
                    tab["margins"] = json!({ "left": l, "right": r, "top": t, "bottom": b });
                }
            }
            tabs.push(tab);
        }
        let mut js = json!({ "version": 7, "tabs": tabs, "user_defined_signals": self.user_defined_signals, "custom_cursors": self.custom_cursors });
        if let Some(c) = &self.last_sch_text_sim_command {
            js["last_sch_text_sim_command"] = json!(c);
        }
        js
    }

    /// The bytes `SaveWorkbook` writes.
    pub fn to_kicad_text(&self) -> String {
        let mut out = NJson::from_workbook(self).dump(2);
        out.push('\n');
        out
    }
}

/// A JSON value as nlohmann holds one: a whole number and a double are two kinds, and objects keep their keys sorted.
#[derive(Debug, Clone, PartialEq)]
pub enum NJson {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<NJson>),
    Obj(BTreeMap<String, NJson>),
}

impl NJson {
    fn obj(pairs: Vec<(&str, NJson)>) -> NJson {
        NJson::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn strs(v: &[String]) -> NJson {
        NJson::Arr(v.iter().map(|s| NJson::Str(s.clone())).collect())
    }

    /// The workbook with KiCad's types: the cursors' positions and the fixed scales are doubles, everything else counted is an integer.
    pub fn from_workbook(wb: &Workbook) -> NJson {
        let mut tabs = Vec::new();
        for t in &wb.tabs {
            let mut tab = vec![("analysis", NJson::Str(t.analysis.clone())), ("commands", NJson::strs(&t.commands))];
            let mut extra: Vec<(String, NJson)> = Vec::new();
            if let Some(p) = &t.plot {
                let traces = p
                    .traces
                    .iter()
                    .map(|tr| {
                        let mut o: BTreeMap<String, NJson> = BTreeMap::new();
                        o.insert("trace_type".into(), NJson::Int(tr.trace_type));
                        o.insert("signal".into(), NJson::Str(tr.signal.clone()));
                        if let Some(c) = &tr.color {
                            o.insert("color".into(), NJson::Str(c.clone()));
                        }
                        for (n, c) in &tr.cursors {
                            o.insert(format!("cursor{n}"), NJson::obj(vec![("position", NJson::Float(c.position)), ("x_format", NJson::Str(c.x_format.clone())), ("y_format", NJson::Str(c.y_format.clone()))]));
                        }
                        if let Some((x, y)) = &tr.cursor_d {
                            o.insert("cursorD".into(), NJson::obj(vec![("x_format", NJson::Str(x.clone())), ("y_format", NJson::Str(y.clone()))]));
                        }
                        NJson::Obj(o)
                    })
                    .collect();
                extra.push(("traces".into(), NJson::Arr(traces)));
                extra.push(("measurements".into(), NJson::Arr(p.measurements.iter().map(|(e, fm)| NJson::obj(vec![("expr", NJson::Str(e.clone())), ("format", NJson::Str(fm.clone()))])).collect())));
                extra.push(("dottedSecondary".into(), NJson::Bool(p.dotted_secondary)));
                extra.push(("showGrid".into(), NJson::Bool(p.show_grid)));
                for (key, y) in [("fixedY1scale", p.fixed_y1), ("fixedY2scale", p.fixed_y2), ("fixedY3scale", p.fixed_y3)] {
                    if let Some((min, max)) = y {
                        extra.push((key.into(), NJson::obj(vec![("min", NJson::Float(min)), ("max", NJson::Float(max))])));
                    }
                }
                if let Some((x, y)) = p.legend {
                    extra.push(("legend".into(), NJson::obj(vec![("x", NJson::Int(x)), ("y", NJson::Int(y))])));
                }
                if let Some((l, r, tp, b)) = p.margins {
                    extra.push(("margins".into(), NJson::obj(vec![("left", NJson::Int(l)), ("right", NJson::Int(r)), ("top", NJson::Int(tp)), ("bottom", NJson::Int(b))])));
                }
            }
            let mut o: BTreeMap<String, NJson> = tab.drain(..).map(|(k, v)| (k.to_string(), v)).collect();
            o.extend(extra);
            tabs.push(NJson::Obj(o));
        }
        let mut root = vec![("version", NJson::Int(7)), ("tabs", NJson::Arr(tabs)), ("user_defined_signals", NJson::strs(&wb.user_defined_signals)), ("custom_cursors", NJson::Int(wb.custom_cursors))];
        if let Some(c) = &wb.last_sch_text_sim_command {
            root.push(("last_sch_text_sim_command", NJson::Str(c.clone())));
        }
        NJson::obj(root)
    }

    /// `nlohmann::json::dump( indent )`.
    pub fn dump(&self, indent: usize) -> String {
        let mut out = String::new();
        self.write(&mut out, indent, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize, level: usize) {
        match self {
            NJson::Null => out.push_str("null"),
            NJson::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            NJson::Int(n) => out.push_str(&n.to_string()),
            NJson::Float(x) => out.push_str(&nlohmann_double(*x)),
            NJson::Str(s) => write_string(out, s),
            NJson::Arr(a) => {
                if a.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (k, v) in a.iter().enumerate() {
                    out.push_str(&" ".repeat(indent * (level + 1)));
                    v.write(out, indent, level + 1);
                    if k + 1 < a.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(indent * level));
                out.push(']');
            }
            NJson::Obj(o) => {
                if o.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (k, (key, v)) in o.iter().enumerate() {
                    out.push_str(&" ".repeat(indent * (level + 1)));
                    write_string(out, key);
                    out.push_str(": ");
                    v.write(out, indent, level + 1);
                    if k + 1 < o.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&" ".repeat(indent * level));
                out.push('}');
            }
        }
    }
}

/// nlohmann's `serializer::dump_escaped` with `ensure_ascii` off: `"`, `\`, the five named control characters and `\u00xx` for the others.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// nlohmann's `to_chars` of a double (`format_buffer` with `min_exp = -4`, `max_exp = 15`): the shortest digits that read back as the same
/// double, as `1234.0`, `12.34`, `0.001234` or `1.234e-05`. A non-finite double is `null`.
pub fn nlohmann_double(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let k = digits.len() as i32;
    // value = 0.d1d2...dk * 10^n
    let n = e + 1;
    let mut s = String::new();
    if x < 0.0 {
        s.push('-');
    }
    if k <= n && n <= 15 {
        s.push_str(&digits);
        s.push_str(&"0".repeat((n - k) as usize));
        s.push_str(".0");
    } else if 0 < n && n <= 15 {
        s.push_str(&digits[..n as usize]);
        s.push('.');
        s.push_str(&digits[n as usize..]);
    } else if -4 < n && n <= 0 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-n) as usize));
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        if k > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        let ee = n - 1;
        s.push('e');
        s.push(if ee < 0 { '-' } else { '+' });
        let a = ee.abs();
        if a < 10 {
            s.push('0');
        }
        s.push_str(&a.to_string());
    }
    s
}

/// The text with `//` and `/* */` comments outside strings taken out (nlohmann's `ignore_comments`).
fn strip_comments(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `SIMULATOR_FRAME_UI::LoadWorkbook`: the workbook in a file's text, JSON or of before version 7.
pub fn parse_workbook(text: &str) -> Result<Workbook, String> {
    let first = text.lines().next().unwrap_or("");
    if first.starts_with("version ") || first.trim().parse::<i64>().is_ok() {
        return parse_legacy(text);
    }
    let js: Value = serde_json::from_str(&strip_comments(text)).map_err(|e| format!("Json parse error reading the workbook: {e}"))?;
    Workbook::from_value(&js)
}

/// `SIMULATOR_FRAME_UI::loadLegacyWorkbook` (`legacy_workbook.cpp`) and its `parseTraceParams`: the line-by-line workbook of versions 1 to 6,
/// as the JSON one it is shown as. Its errors are KiCad's, "Error loading workbook: line %d: %s.".
pub fn parse_legacy(text: &str) -> Result<Workbook, String> {
    use crate::netlist::*;
    let lines: Vec<&str> = text.lines().collect();
    let mut at = 0usize;
    let next = |at: &mut usize| -> Option<String> {
        let l = lines.get(*at).map(|s| s.trim_end_matches('\r').to_string());
        *at += 1;
        l
    };
    let err = |line: usize, msg: &str| format!("Error loading workbook: line {}: {msg}.", line);
    let first = next(&mut at).unwrap_or_default();
    let mut version: i64 = 1;
    let count_line = if let Some(v) = first.strip_prefix("version ") {
        version = v.trim().parse().map_err(|_| err(at, "expecting version"))?;
        next(&mut at).unwrap_or_default()
    } else {
        first
    };
    let tab_count: i64 = count_line.trim().parse().map_err(|_| err(at, "expecting simulation tab count"))?;
    let mut wb = Workbook::default();
    for _ in 0..tab_count {
        let _sim_type: i64 = next(&mut at).unwrap_or_default().trim().parse().map_err(|_| err(at, "expecting simulation tab type"))?;
        // UnescapeString of the command line: "{return}" is a new line in KiCad's escapes.
        let command = next(&mut at).unwrap_or_default().replace("{return}", "\n");
        let mut options = OPTION_DEFAULT_FLAGS;
        if version >= 2 {
            options &= !(OPTION_ADJUST_INCLUDE_PATHS | OPTION_SAVE_ALL_VOLTAGES | OPTION_SAVE_ALL_CURRENTS);
        }
        if version >= 3 {
            options &= !OPTION_SAVE_ALL_DISSIPATIONS;
        }
        let mut sim_command = String::new();
        for line in command.split(['\r', '\n']).filter(|l| !l.is_empty()) {
            if line.starts_with(".kicad adjustpaths") {
                options |= OPTION_ADJUST_INCLUDE_PATHS;
            } else if line.starts_with(".save all") {
                options |= OPTION_SAVE_ALL_VOLTAGES;
            } else if line.starts_with(".probe alli") {
                options |= OPTION_SAVE_ALL_CURRENTS;
            } else if line.starts_with(".probe allp") {
                options |= OPTION_SAVE_ALL_DISSIPATIONS;
            } else {
                sim_command.push_str(line);
                sim_command.push('\n');
            }
        }
        let sim = crate::command::command_to_sim_type(&sim_command);
        let traces_count: i64 = next(&mut at).unwrap_or_default().trim().parse().map_err(|_| err(at, "expecting trace count"))?;
        let mut plot = sim.is_plottable().then(|| WbPlot { traces: Vec::new(), measurements: Vec::new(), dotted_secondary: false, show_grid: true, fixed_y1: None, fixed_y2: None, fixed_y3: None, legend: None, margins: None });
        for _ in 0..traces_count {
            let trace_type: i64 = next(&mut at).unwrap_or_default().trim().parse().map_err(|_| err(at, "expecting trace type"))?;
            let name = next(&mut at).unwrap_or_default();
            if name.is_empty() {
                return Err(err(at, "expecting trace name"));
            }
            let param = next(&mut at).unwrap_or_default();
            if param.is_empty() {
                return Err(err(at, "expecting trace color"));
            }
            let Some(p) = plot.as_mut() else { continue };
            if trace_type == 0 && name == "$LEGEND" {
                let c: Vec<i64> = param.split(' ').filter_map(|s| s.parse().ok()).collect();
                p.legend = Some((c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0)));
                continue;
            }
            let mut tr = WbTrace { trace_type, signal: name, color: None, cursors: BTreeMap::new(), cursor_d: None };
            if version >= 4 {
                for item in param.split('|') {
                    let parts: Vec<&str> = item.split(':').collect();
                    if item.starts_with("rgb") {
                        tr.color = Some(item.to_string());
                    } else if (item.starts_with("cursor1") || item.starts_with("cursor2")) && parts.len() == 3 {
                        let id = if item.starts_with("cursor1") { 1 } else { 2 };
                        let x = parts[0].split_once('=').and_then(|(_, v)| v.parse().ok()).unwrap_or(0.0);
                        tr.cursors.insert(id, WbCursor { position: x, x_format: parts[1].into(), y_format: parts[2].into() });
                    } else if item.starts_with("cursorD") && parts.len() == 3 {
                        tr.cursor_d = Some((parts[1].into(), parts[2].into()));
                    } else if item == "dottedSecondary" {
                        p.dotted_secondary = true;
                    } else if item == "hideGrid" {
                        p.show_grid = false;
                    }
                }
            }
            p.traces.push(tr);
        }
        if version > 4 {
            let n: i64 = next(&mut at).unwrap_or_default().trim().parse().map_err(|_| err(at, "expecting measurement count"))?;
            for _ in 0..n {
                let m = next(&mut at).unwrap_or_default();
                if m.is_empty() {
                    return Err(err(at, "expecting measurement definition"));
                }
                let fm = next(&mut at).unwrap_or_default();
                if fm.is_empty() {
                    return Err(err(at, "expecting measurement format definition"));
                }
                if let Some(p) = plot.as_mut() {
                    p.measurements.push((m, fm));
                }
            }
        }
        wb.tabs.push(WbTab { analysis: sim.short_name().into(), commands: commands_of(sim_command.trim_end_matches('\n'), options), plot });
    }
    if let Some(n) = next(&mut at).and_then(|l| l.trim().parse::<i64>().ok()) {
        for _ in 0..n {
            wb.user_defined_signals.push(next(&mut at).unwrap_or_default());
        }
    }
    if version >= 5 && !wb.tabs.is_empty() {
        wb.last_sch_text_sim_command = next(&mut at);
    }
    Ok(wb)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workbook as KiCad 10 writes one (`SaveWorkbook`), byte for byte: two tabs, a trace with both cursors, a measurement, a fixed scale.
    const KICAD_WBK: &str = r#"{
  "custom_cursors": 2,
  "last_sch_text_sim_command": ".tran 10u 5m",
  "tabs": [
    {
      "analysis": "TRAN",
      "commands": [
        ".tran 10u 5m",
        ".kicad adjustpaths",
        ".save all",
        ".probe alli",
        ".probe allp"
      ],
      "dottedSecondary": true,
      "fixedY1scale": {
        "max": 5.0,
        "min": -0.5
      },
      "margins": {
        "bottom": 45,
        "left": 70,
        "right": 70,
        "top": 30
      },
      "measurements": [
        {
          "expr": "MAX V(/out)",
          "format": "3~V"
        }
      ],
      "showGrid": true,
      "traces": [
        {
          "color": "rgb(228, 26, 28)",
          "cursor1": {
            "position": 0.001,
            "x_format": "3~s",
            "y_format": "3~V"
          },
          "cursor2": {
            "position": 1e-05,
            "x_format": "3~s",
            "y_format": "3~V"
          },
          "cursorD": {
            "x_format": "3~s",
            "y_format": "3~V"
          },
          "signal": "V(/out)",
          "trace_type": 257
        }
      ]
    },
    {
      "analysis": "OP",
      "commands": [
        ".op",
        ".kicad esavenone"
      ]
    }
  ],
  "user_defined_signals": [
    "V(/out)-V(/in)"
  ],
  "version": 7
}
"#;

    #[test]
    fn a_kicad_workbook_reads_and_writes_back_byte_for_byte() {
        let wb = parse_workbook(KICAD_WBK).unwrap();
        assert_eq!(wb.tabs.len(), 2);
        let p = wb.tabs[0].plot.as_ref().unwrap();
        assert_eq!(p.traces[0].cursors[&2].position, 1e-5);
        assert_eq!(p.fixed_y1, Some((-0.5, 5.0)));
        assert!(wb.tabs[1].plot.is_none());
        assert_eq!(wb.to_kicad_text(), KICAD_WBK);
        // and through the panel's JSON
        assert_eq!(Workbook::from_value(&wb.to_value()).unwrap(), wb);
    }

    #[test]
    fn doubles_are_written_as_nlohmann_writes_them() {
        for (x, s) in [(1.0, "1.0"), (0.5, "0.5"), (0.001, "0.001"), (0.0001, "0.0001"), (1e-5, "1e-05"), (123.456, "123.456"), (1e20, "1e+20"), (1e15, "1e+15"), (1e14, "100000000000000.0"), (-2.5e-7, "-2.5e-07"), (0.0, "0.0"), (1.5e300, "1.5e+300")] {
            assert_eq!(nlohmann_double(x), s, "{x}");
        }
        let mut out = String::new();
        write_string(&mut out, "a\"b\\c\nd\u{1}é");
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn options_and_commands_go_both_ways() {
        use crate::netlist::*;
        let c = commands_of(".op", OPTION_DEFAULT_FLAGS);
        assert_eq!(c, vec![".op", ".kicad adjustpaths", ".save all", ".probe alli", ".probe allp"]);
        assert_eq!(command_and_options(&c), (".op".to_string(), OPTION_DEFAULT_FLAGS));
        let (cmd, opts) = command_and_options(&[".ac dec 10 1 1k  ".into(), ".kicad esavenone".into()]);
        assert_eq!(cmd, ".ac dec 10 1 1k");
        assert_eq!(opts, OPTION_ADJUST_PASSIVE_VALS);
    }

    #[test]
    fn comments_are_allowed_and_bad_files_are_errors() {
        let text = "// a workbook\n{ \"version\": 7, /* none */ \"tabs\": [], \"user_defined_signals\": [\"a//b\"] }";
        let wb = parse_workbook(text).unwrap();
        assert_eq!(wb.user_defined_signals, vec!["a//b"]);
        assert!(parse_workbook("{ not json").is_err());
        assert!(parse_workbook("{\"tabs\": [{\"commands\": 3}]}").is_err());
    }

    #[test]
    fn a_legacy_workbook_is_read() {
        let text = "version 5\n1\n9\n.tran 1u 1m{return}.save all\n2\n1\nV(out)\nrgb(228, 26, 28)|cursor1=0.0005:3~s:3~V|dottedSecondary\n0\n$LEGEND\n10 20\n1\nMAX V(out)\n3~V\n1\nV(out)*2\n.tran 1u 1m\n";
        let wb = parse_workbook(text).unwrap();
        let t = &wb.tabs[0];
        assert_eq!(t.commands, vec![".tran 1u 1m", ".save all"], "version 3 and later say every option they have");
        let p = t.plot.as_ref().unwrap();
        assert_eq!(p.traces.len(), 1);
        assert_eq!(p.traces[0].color.as_deref(), Some("rgb(228, 26, 28)"));
        assert_eq!(p.traces[0].cursors[&1].position, 0.0005);
        assert!(p.dotted_secondary);
        assert_eq!(p.legend, Some((10, 20)));
        assert_eq!(p.measurements, vec![("MAX V(out)".to_string(), "3~V".to_string())]);
        assert_eq!(wb.user_defined_signals, vec!["V(out)*2"]);
        assert_eq!(wb.last_sch_text_sim_command.as_deref(), Some(".tran 1u 1m"));
        assert!(parse_workbook("version 5\nx\n").unwrap_err().starts_with("Error loading workbook: line 2: expecting simulation tab count"));
    }
}
