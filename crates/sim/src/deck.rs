//! The deck one batch run of ngspice reads. It is a control block (`.control` ... `.endc`) and nothing else: it `source`s the simulator's
//! netlist and then asks of ngspice what KiCad asks of libngspice around a run, in KiCad's order:
//! - `NGSPICE::Attach`: the netlist loaded (`LoadNetlist`, here `source`), then the compatibility mode (`NGSPICE::GetSettingCommands`) --
//!   KiCad parses the netlist with `ngbehavior` unset and sets the mode again after it ("ngspice 46+ stopped substituting gnd with node 0
//!   inside subcircuits when a PSpice compatibility mode is active"), and so does this deck -- and `esave none` unless the tab saves all
//!   events (with its `echo`);
//! - `SIMULATOR_FRAME_UI::applyTuners` (`OnSimUpdate`): an `alter` per tuned value (`SPICE_GENERATOR_IDEAL::TunerCommand`,
//!   `SPICE_GENERATOR_SOURCE::TunerCommand`);
//! - `NGSPICE::Run`: `run` (KiCad's `bg_run`). An FFT tab (`SIMULATOR_FRAME::StartSimulation`) runs the transient analysis its signals come
//!   from and then its own lines (`linearize`, `fft`) on that plot;
//! - `SIMULATOR_FRAME_UI::OnSimRefresh( true )`: `let user<id> = ...` for every user-defined signal (`applyUserDefinedSignals`, with its
//!   `echo`), then `meas` for every measurement (`UpdateMeasurement`, with its `echo`), and the Fourier analyses asked
//!   (`SIMULATOR_FRAME_UI::DoFourier`);
//! - the plots written to raw files for [`crate::raw`]: the analysis's own plot, and for a noise analysis both of its plots (the second,
//!   the integrated noise, is the one the run leaves current; `OnSimRefresh` prints it and plots the first); then `quit`, or batch mode
//!   would run the circuit once more.
//!
//! The netlist is `source`d rather than being the deck itself: ngspice 42 in batch mode cannot read a netlist with `.probe p(<a voltage
//! source>)` in it (its power probe becomes lines it reads as commands, and the matrix is singular), which every netlist with "Save all
//! power dissipations" has, and it reads one it is told to `source` as libngspice reads KiCad's.
//!
//! Every line that comes from the studio is checked to be one line: a newline would start another command.

use crate::command::{command_to_sim_type, SimType};
use crate::value::format_g;

/// A tuned value (`TUNER_SLIDER`): the item and the value to alter it to.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuner {
    /// `SPICE_GENERATOR::ItemName`.
    pub item: String,
    pub value: f64,
}

/// What one run asks of ngspice after the netlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Controls {
    /// The tab's command; for an FFT, its own lines (`linearize ...`, `fft ...`), run after `run`.
    pub command: String,
    /// The tab's `GetSimOptions`.
    pub options: u32,
    /// `NGSPICE_SETTINGS::model_mode`.
    pub model_mode: u32,
    pub tuners: Vec<Tuner>,
    /// `(id, expression)`: the user-defined signals, the expressions with their net names quoted already (`quoteNetNames`).
    pub user_signals: Vec<(i64, String, String)>,
    /// `(row, text)`: the measurements, `text` what follows the result name in `meas <type> meas_result_<row> <text>`.
    pub measurements: Vec<(usize, String)>,
    /// `(fundamental, signal)`: `fourier <fundamental> <signal>`.
    pub fourier: Vec<(String, String)>,
}

/// The raw files a run writes, relative to where ngspice runs.
#[derive(Debug, Clone, PartialEq)]
pub struct RawFiles {
    pub main: String,
    /// The noise analysis's integrated-noise plot.
    pub second: Option<String>,
}

fn one_line(what: &str, s: &str) -> Result<(), String> {
    if s.contains('\n') || s.contains('\r') {
        return Err(format!("{what} must be one line: {s:?}"));
    }
    Ok(())
}

/// `NGSPICE::GetSettingCommands`.
pub fn setting_commands(model_mode: u32) -> Vec<&'static str> {
    match model_mode {
        1 => vec!["unset ngbehavior"],
        2 => vec!["set ngbehavior=psa"],
        3 => vec!["set ngbehavior=lta"],
        4 => vec!["set ngbehavior=ltpsa"],
        5 => vec!["set ngbehavior=hsa"],
        _ => vec![],
    }
}

/// `{:g}` as fmt formats a double (the tuner commands' `alter @{}={:g}`): six significant digits, `%g`.
fn fmt_g(x: f64) -> String {
    format_g(x, 6)
}

/// The deck for `controls`: `netlist` (the path ngspice is to `source`, relative to where it runs) and the raw files under `raw_stem`.
pub fn driver(controls: &Controls, netlist: &str, raw_stem: &str) -> Result<(String, RawFiles), String> {
    let sim = command_to_sim_type(&controls.command);
    one_line("the netlist's path", netlist)?;
    let mut c = String::from("* KiCad SPICE simulator\n.control\n");
    c.push_str(&format!("source {netlist}\n"));
    for s in setting_commands(controls.model_mode) {
        c.push_str(s);
        c.push('\n');
    }
    if controls.options & crate::netlist::OPTION_SAVE_ALL_EVENTS == 0 {
        c.push_str("echo Command: esave none\nesave none\n");
    }
    for t in &controls.tuners {
        one_line("a tuned item", &t.item)?;
        if t.item.is_empty() || !t.value.is_finite() {
            return Err(format!("cannot tune {:?} to {}", t.item, t.value));
        }
        c.push_str(&format!("alter @{}={}\n", t.item, fmt_g(t.value)));
    }
    c.push_str("run\n");
    // `OnSimRefresh` of a pole-zero analysis: "Simulation results:" and `print all` in the console.
    if sim == SimType::Pz {
        c.push_str("echo\necho Simulation results:\necho\nprint all\n");
    }
    if sim == SimType::Fft {
        for line in controls.command.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
            c.push_str(line);
            c.push('\n');
        }
    }
    for (id, shown, quoted) in &controls.user_signals {
        one_line("a user-defined signal", shown)?;
        one_line("a user-defined signal", quoted)?;
        c.push_str(&format!("echo let user{id} = {shown}\nlet user{id} = {quoted}\n"));
    }
    let type_name = sim.short_name();
    for (row, text) in &controls.measurements {
        one_line("a measurement", text)?;
        let cmd = format!("meas {type_name} meas_result_{row} {text}");
        c.push_str(&format!("echo {cmd}\n{cmd}\n"));
    }
    for (fundamental, signal) in &controls.fourier {
        one_line("a Fourier analysis", fundamental)?;
        one_line("a Fourier analysis", signal)?;
        c.push_str(&format!("fourier {fundamental} {signal}\n"));
    }
    let main = format!("{raw_stem}.raw");
    let mut second = None;
    c.push_str("set filetype=binary\n");
    if sim == SimType::Noise {
        let s = format!("{raw_stem}-2.raw");
        c.push_str(&format!("write {s}\nsetplot previous\n"));
        second = Some(s);
    }
    c.push_str(&format!("write {main}\nquit\n.endc\n.end\n"));
    Ok((c, RawFiles { main, second }))
}

/// `SIMULATOR_FRAME_UI::applyUserDefinedSignals`' `quoteNetNames`: every net name of `netnames` the expression has -- and the net-name
/// characters that run on after it -- put in double quotes, so that ngspice reads `/out` or `Net-_R1-Pad1_` as one node.
pub fn quote_net_names(expr: &str, netnames: &[String]) -> String {
    let chars: Vec<char> = expr.chars().collect();
    let mut mask = vec![false; chars.len()];
    let is_netname_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '+' | '-' | '~' | '.');
    for name in netnames.iter().filter(|n| !n.is_empty()) {
        let nc: Vec<char> = name.chars().collect();
        let mut pos = 0;
        while pos + nc.len() <= chars.len() {
            if chars[pos..pos + nc.len()] == nc[..] {
                for m in mask.iter_mut().skip(pos).take(nc.len()) {
                    *m = true;
                }
            }
            pos += 1;
        }
    }
    for i in 0..chars.len() {
        if !mask[i] || (i > 0 && mask[i - 1]) {
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() {
            if mask[j] {
                j += 1;
                continue;
            }
            if is_netname_char(chars[j]) {
                mask[j] = true;
                j += 1;
            } else {
                break;
            }
        }
    }
    let mut out = String::new();
    let mut start_quote = true;
    for (i, c) in chars.iter().enumerate() {
        if mask[i] && start_quote {
            out.push('"');
            start_quote = false;
        } else if !mask[i] && !start_quote {
            out.push('"');
            start_quote = true;
        }
        out.push(*c);
    }
    if !start_quote {
        out.push('"');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transient_run_tunes_runs_measures_and_writes() {
        let controls = Controls {
            command: ".tran 10u 5m".into(),
            options: crate::netlist::OPTION_DEFAULT_FLAGS,
            model_mode: 4,
            tuners: vec![Tuner { item: "R1".into(), value: 1500.0 }],
            user_signals: vec![(0, "V(out)*2".into(), "V(\"out\")*2".into())],
            measurements: vec![(0, "MAX V(out)".into())],
            fourier: vec![("1k".into(), "V(out)".into())],
        };
        let (c, raw) = driver(&controls, ".kicad/sim/run.cir", "sim").unwrap();
        assert_eq!(
            c,
            "* KiCad SPICE simulator\n.control\nsource .kicad/sim/run.cir\nset ngbehavior=ltpsa\nalter @R1=1500\nrun\necho let user0 = V(out)*2\nlet user0 = V(\"out\")*2\necho meas TRAN meas_result_0 MAX V(out)\nmeas TRAN meas_result_0 MAX V(out)\nfourier 1k V(out)\nset filetype=binary\nwrite sim.raw\nquit\n.endc\n.end\n"
        );
        assert_eq!(raw, RawFiles { main: "sim.raw".into(), second: None });
    }

    #[test]
    fn net_names_are_quoted_as_kicad_quotes_them() {
        let nets = vec!["/out".to_string(), "/in".to_string(), "Net-_R1-Pad1_".to_string()];
        assert_eq!(quote_net_names("V(/out)-V(/in)", &nets), "V(\"/out\")-V(\"/in\")");
        assert_eq!(quote_net_names("V(Net-_R1-Pad1_)*2", &nets), "V(\"Net-_R1-Pad1_\")*2");
        assert_eq!(quote_net_names("V(x)", &nets), "V(x)");
    }

    #[test]
    fn noise_writes_both_plots_and_events_can_go() {
        let controls = Controls { command: ".noise v(out) V1 dec 10 1 1Meg".into(), options: 0, model_mode: 0, ..Controls::default() };
        let (c, raw) = driver(&controls, "n.cir", "n").unwrap();
        assert!(c.contains("source n.cir\necho Command: esave none\nesave none\nrun\n"), "{c}");
        assert!(c.ends_with("write n-2.raw\nsetplot previous\nwrite n.raw\nquit\n.endc\n.end\n"), "{c}");
        assert_eq!(raw.second.as_deref(), Some("n-2.raw"));
    }

    #[test]
    fn an_fft_runs_its_lines_after_the_transient_and_lines_stay_lines() {
        let controls = Controls { command: "linearize v(out)\nfft v(out)".into(), options: crate::netlist::OPTION_DEFAULT_FLAGS, model_mode: 1, ..Controls::default() };
        let (c, _) = driver(&controls, "f.cir", "f").unwrap();
        assert!(c.contains("unset ngbehavior\nrun\nlinearize v(out)\nfft v(out)\n"), "{c}");
        let bad = Controls { measurements: vec![(0, "MAX V(out)\nshell rm -rf x".into())], ..controls };
        assert!(driver(&bad, "f.cir", "f").is_err());
        assert!(driver(&Controls::default(), "a\nshell x", "f").is_err());
    }
}
