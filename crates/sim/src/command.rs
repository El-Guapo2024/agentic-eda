//! The analysis commands: what kind of analysis a command is (`SPICE_CIRCUIT_MODEL::CommandToSimType`), its names
//! (`SPICE_SIMULATOR::TypeToName`), which ones the schematic's text directives hold (`NETLIST_EXPORTER_SPICE::ReadDirectives`,
//! `SPICE_CIRCUIT_MODEL::GetSchTextSimCommand`) and the fields of a `.dc` and a `.noise` command
//! (`SPICE_CIRCUIT_MODEL::ParseDCCommand`, `ParseNoiseCommand`).

use crate::value::SpiceValue;

/// `SIM_TYPE` (`sim_types.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SimType {
    Unknown,
    Ac,
    Dc,
    Disto,
    Noise,
    Op,
    Pz,
    Sens,
    Tf,
    Tran,
    Sp,
    Fft,
}

impl SimType {
    /// `SPICE_SIMULATOR::TypeToName( aType, true )`: the short name, as the workbook's `analysis` and the tab titles carry it.
    pub fn short_name(self) -> &'static str {
        match self {
            SimType::Op => "OP",
            SimType::Ac => "AC",
            SimType::Dc => "DC",
            SimType::Tran => "TRAN",
            SimType::Disto => "DISTO",
            SimType::Noise => "NOISE",
            SimType::Pz => "PZ",
            SimType::Sens => "SENS",
            SimType::Tf => "TF",
            SimType::Sp => "SP",
            SimType::Fft => "FFT",
            SimType::Unknown => "Custom",
        }
    }

    /// `SPICE_SIMULATOR::TypeToName( aType, false )`.
    pub fn long_name(self) -> &'static str {
        match self {
            SimType::Op => "DC Operating Point",
            SimType::Ac => "Small-Signal Analysis",
            SimType::Dc => "DC Sweep Analysis",
            SimType::Tran => "Transient Analysis",
            SimType::Disto => "Small-Signal Distortion Analysis",
            SimType::Noise => "Noise Analysis",
            SimType::Pz => "Pole-Zero Analysis",
            SimType::Sens => "Sensitivity Analysis",
            SimType::Tf => "Transfer Function Analysis",
            SimType::Sp => "S-Parameter Analysis",
            SimType::Fft => "Frequency Content Analysis",
            SimType::Unknown => "Custom",
        }
    }

    /// `SIM_TAB::IsPlottable`.
    pub fn is_plottable(self) -> bool {
        matches!(self, SimType::Ac | SimType::Dc | SimType::Sp | SimType::Tran | SimType::Noise | SimType::Fft)
    }
}

/// `SPICE_CIRCUIT_MODEL::CommandToSimType`: by the command's first word, lower case and trimmed (on the right, as `wxString::Trim` does);
/// an FFT is a command that starts with `fft` or has a line that does.
pub fn command_to_sim_type(cmd: &str) -> SimType {
    let c = cmd.to_lowercase();
    let c = c.trim_end();
    if c == ".op" {
        SimType::Op
    } else if c.starts_with(".ac") {
        SimType::Ac
    } else if c.starts_with(".dc") {
        SimType::Dc
    } else if c.starts_with(".tran") {
        SimType::Tran
    } else if c.starts_with(".disto") {
        SimType::Disto
    } else if c.starts_with(".noise") {
        SimType::Noise
    } else if c.starts_with(".pz") {
        SimType::Pz
    } else if c.starts_with(".sens") {
        SimType::Sens
    } else if c.starts_with(".sp") {
        SimType::Sp
    } else if c.starts_with(".tf") {
        SimType::Tf
    } else if c.starts_with("fft") || c.contains("\nfft") {
        SimType::Fft
    } else {
        SimType::Unknown
    }
}

fn is_directive(line: &str, dir: &str) -> bool {
    line == dir || line.starts_with(&format!("{dir} "))
}

/// The dot commands `NETLIST_EXPORTER_SPICE::ReadDirectives` sends to SPICE.
const DIRECTIVES: &[&str] = &[
    ".AC", ".CONTROL", ".CSPARAM", ".DISTO", ".DC", ".ELSE", ".ELSEIF", ".END", ".ENDC", ".ENDIF", ".ENDS", ".FOUR", ".FUNC", ".GLOBAL", ".IC", ".IF", ".INCLUDE",
    ".LIB", ".MEAS", ".MODEL", ".NODESET", ".NOISE", ".OP", ".OPTIONS", ".PARAM", ".PLOT", ".PRINT", ".PROBE", ".PZ", ".SAVE", ".SENS", ".SP", ".SUBCKT",
    ".TEMP", ".TF", ".TITLE", ".TRAN", ".WIDTH",
];

/// Whether a schematic text is a directive (`NETLIST_EXPORTER_SPICE::ReadDirectives`): one of its lines, upper-cased, is one of the dot
/// commands above, or declares a mutual inductance (`K<name> L<name> L<name> <coupling>`). The whole text is then the directive.
pub fn text_is_directive(text: &str) -> bool {
    for raw in text.split(['\r', '\n']).filter(|l| !l.is_empty()) {
        let line = raw.to_uppercase();
        if line.starts_with('.') {
            if DIRECTIVES.iter().any(|d| is_directive(&line, d)) {
                return true;
            }
        } else if line.starts_with('K') {
            let mut t = line.split([' ', '\t']).filter(|s| !s.is_empty());
            if !t.next().is_some_and(|s| s.starts_with('K')) {
                continue;
            }
            if !t.next().is_some_and(|s| s.starts_with('L')) {
                continue;
            }
            if !t.next().is_some_and(|s| s.starts_with('L')) {
                continue;
            }
            // Coupling coefficient
            if t.next().is_none() {
                continue;
            }
            return true;
        }
    }
    false
}

/// `NETLIST_EXPORTER_SPICE::IsSimCommand` as `WriteDirectives` asks it of a whole directive: it starts (upper-cased) with an analysis.
pub fn is_sim_command(directive: &str) -> bool {
    if !directive.starts_with('.') {
        return false;
    }
    let c = directive.to_uppercase();
    [".AC", ".DC", ".TRAN", ".OP", ".DISTO", ".NOISE", ".PZ", ".SENS", ".TF"].iter().any(|d| is_directive(&c, d))
}

/// `SPICE_CIRCUIT_MODEL::GetSchTextSimCommand`: the directives that are analyses, each followed by "\r\n", the whole trimmed on the right.
pub fn sch_text_sim_command(directives: &[String]) -> String {
    let mut s = String::new();
    for d in directives.iter().filter(|d| is_sim_command(d)) {
        s.push_str(d);
        s.push_str("\r\n");
    }
    s.trim_end().to_string()
}

/// `SPICE_DC_PARAMS`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DcParams {
    pub source: String,
    pub start: SpiceValue,
    pub end: SpiceValue,
    pub increment: SpiceValue,
}

fn spice(t: Option<&str>) -> SpiceValue {
    t.and_then(SpiceValue::parse).unwrap_or_default()
}

/// `SPICE_CIRCUIT_MODEL::ParseDCCommand`: the first source and, when the command names one, the second.
pub fn parse_dc_command(cmd: &str) -> Option<(DcParams, Option<DcParams>)> {
    if !cmd.to_lowercase().starts_with(".dc") {
        return None;
    }
    let mut t = cmd.get(3..).unwrap_or("").split([' ', '\t']).filter(|s| !s.is_empty());
    let src1 = DcParams { source: t.next().unwrap_or("").to_string(), start: spice(t.next()), end: spice(t.next()), increment: spice(t.next()) };
    let src2 = t.next().map(|s| DcParams { source: s.to_string(), start: spice(t.next()), end: spice(t.next()), increment: spice(t.next()) });
    Some((src1, src2))
}

/// `SPICE_CIRCUIT_MODEL::ParseNoiseCommand`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoiseParams {
    pub output: String,
    pub reference: String,
    pub source: String,
    pub scale: String,
    pub points: SpiceValue,
    pub f_start: SpiceValue,
    pub f_stop: SpiceValue,
    pub save_all: bool,
}

pub fn parse_noise_command(cmd: &str) -> Option<NoiseParams> {
    if !cmd.to_lowercase().starts_with(".noise") {
        return None;
    }
    let rest = cmd.get(6..).unwrap_or("").trim_start();
    if !rest.to_lowercase().starts_with("v(") {
        return None;
    }
    let rest = &rest[2..];
    let (function, params) = rest.split_once(')').unwrap_or((rest, ""));
    let mut f = function.split([' ', ',', '\t']).filter(|s| !s.is_empty());
    let mut p = NoiseParams { output: f.next().unwrap_or("").into(), reference: f.next().unwrap_or("").into(), ..NoiseParams::default() };
    let mut t = params.split([' ', '\t']).filter(|s| !s.is_empty());
    let mut token = t.next();
    if let Some(s) = token {
        p.source = s.to_string();
        token = t.next();
    }
    if let Some(s) = token.filter(|s| matches!(s.to_lowercase().as_str(), "dec" | "oct" | "lin")) {
        p.scale = s.to_string();
        token = t.next();
    }
    if let Some(s) = token {
        p.points = spice(Some(s));
        token = t.next();
    }
    if let Some(s) = token {
        p.f_start = spice(Some(s));
        token = t.next();
    }
    if let Some(s) = token {
        p.f_stop = spice(Some(s));
        token = t.next();
    }
    p.save_all = token.is_some();
    Some(p)
}

/// `NGSPICE::GetXAxis`: the vector the plot's x axis is, among the plot's vectors (`vectors`, for a DC sweep the one ending "-sweep"; a raw
/// file calls it `v(v-sweep)` where libngspice's memory has `v-sweep`).
pub fn x_axis(sim: SimType, vectors: &[&str]) -> Option<String> {
    match sim {
        SimType::Ac | SimType::Sp | SimType::Noise | SimType::Fft => Some("frequency".into()),
        SimType::Dc => Some(vectors.iter().find(|v| { let l = v.to_lowercase(); l.ends_with("-sweep") || l.ends_with("-sweep)") }).map(|v| v.to_string()).unwrap_or_else(|| "sweep".into())),
        SimType::Tran => Some("time".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_typed_by_their_first_word() {
        assert_eq!(command_to_sim_type(".tran 10u 5m"), SimType::Tran);
        assert_eq!(command_to_sim_type(".OP  "), SimType::Op);
        assert_eq!(command_to_sim_type(".op 1"), SimType::Unknown, "only a bare .op");
        assert_eq!(command_to_sim_type(".ac dec 10 1 1Meg"), SimType::Ac);
        assert_eq!(command_to_sim_type("linearize v(out)\nfft v(out)"), SimType::Fft);
        assert_eq!(command_to_sim_type(".sp lin 10 1 1k"), SimType::Sp);
        assert_eq!(command_to_sim_type("* custom"), SimType::Unknown);
        assert!(SimType::Noise.is_plottable() && !SimType::Op.is_plottable());
        assert_eq!(SimType::Unknown.short_name(), "Custom");
    }

    #[test]
    fn directives_are_recognized_as_kicad_does() {
        assert!(text_is_directive(".tran 1u 1m"));
        assert!(text_is_directive("note\n.include \"x.lib\""));
        assert!(text_is_directive("K1 L1 L2 0.9"));
        assert!(!text_is_directive("K1 L1 L2"));
        assert!(!text_is_directive("R1 is 10k"));
        assert!(!text_is_directive(".transistor"));
        assert!(is_sim_command(".tran 1u 1m\n.save v(out)"));
        assert!(!is_sim_command(".include x"));
        let d = vec![".include x".to_string(), ".tran 1u 1m".to_string(), ".ac dec 10 1 1k".to_string()];
        assert_eq!(sch_text_sim_command(&d), ".tran 1u 1m\r\n.ac dec 10 1 1k");
    }

    #[test]
    fn dc_and_noise_commands_are_read_field_by_field() {
        let (a, b) = parse_dc_command(".dc V1 0 5 0.1 V2 0 1 0.5").unwrap();
        assert_eq!(a.source, "V1");
        assert_eq!(a.end.to_f64(), 5.0);
        let b = b.unwrap();
        assert_eq!(b.source, "V2");
        assert_eq!(b.increment.to_f64(), 0.5);
        assert!(parse_dc_command(".dc V1 0 5 1").unwrap().1.is_none());
        let n = parse_noise_command(".noise v(out,in) V1 dec 10 1 1Meg 1").unwrap();
        assert_eq!((n.output.as_str(), n.reference.as_str(), n.source.as_str(), n.scale.as_str()), ("out", "in", "V1", "dec"));
        assert_eq!(n.f_stop.to_f64(), 1e6);
        assert!(n.save_all);
        let m = parse_noise_command(".noise v(out) V1 lin 5 10 100").unwrap();
        assert_eq!(m.reference, "");
        assert!(!m.save_all);
        assert_eq!(x_axis(SimType::Dc, &["v(out)", "v-sweep"]).as_deref(), Some("v-sweep"));
    }
}
