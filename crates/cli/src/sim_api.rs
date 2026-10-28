//! `/api/sim/*`: the server side of the studio's SPICE simulator (`web/studio/src/components/simulator/`, package P21).
//!
//! The circuit is kicad-cli's (`eda_kicad_engine::sim_netlists`: the SPICE and the XML netlist of the derived schematic) and the simulation
//! is ngspice's, a separate program run in batch mode (`eda_sim::ngspice`); this module joins them the way `SIMULATOR_FRAME` does:
//! - `POST /api/sim/circuit` `{command?, options?}`: what `SPICE_CIRCUIT_MODEL` knows after `ReadSchematicAndLibraries` -- the nets, the
//!   items (name, device, pins, currents, tuner), the schematic's directives and its analysis (`GetSchTextSimCommand`), each symbol pin's net
//!   -- and, for a command, the netlist Show SPICE Netlist shows (`SIMULATOR_CONTROL::ShowNetlist`, `GetNetlist`) with its messages;
//! - `POST /api/sim/run` `{command, options, tuners, user_signals, measurements, fourier, fft_tran}`: `SIMULATOR_FRAME::StartSimulation`
//!   to `OnSimRefresh( true )` in one batch run of ngspice, its console and its plots back (`raw`), the vectors as numbers;
//! - `POST /api/sim/stop`: Stop Simulation, the run killed (`eda_sim::ngspice::RunSlot::stop`);
//! - `POST /api/sim/workbook/parse` `{text}` and `/api/sim/workbook/text` `{workbook}`: a `.wbk` read, and written as KiCad writes it
//!   (`eda_sim::workbook`). Saving it into the design is the `set_simulator` command (`crates/ops`).
//!
//! kicad-cli runs in the studio's kicad-cli lane like every other kicad-cli job; ngspice runs after it, outside the lane, so a long
//! transient does not hold up a DRC, and one simulation runs at a time ("Another simulation is already running.").

use crate::kicad_engine;
use eda_model::ir::Design;
use eda_sim::command::{command_to_sim_type, sch_text_sim_command, SimType};
use eda_sim::deck::{driver, quote_net_names, Controls, Tuner};
use eda_sim::netlist::{circuit, circuit_nets, read_directives, simulator_netlist, spice_net_name, split_netlist, subckt_ports, CircuitItem};
use eda_sim::ngspice::{find_ngspice, run_batch, RunSlot, NGSPICE_MISSING};
use eda_sim::raw::{parse_raw, RawPlot};
use eda_sim::workbook::{parse_workbook, Workbook};
use eda_sim::xml::parse_xml_netlist;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

/// The one simulation the studio runs at a time.
pub fn slot() -> &'static RunSlot {
    static SLOT: OnceLock<RunSlot> = OnceLock::new();
    SLOT.get_or_init(RunSlot::default)
}

/// How long a run may take before it is killed: `EDA_SIM_TIMEOUT_SECS`, else ten minutes (a batch run cannot be asked how far it is).
fn run_limit() -> Duration {
    std::env::var("EDA_SIM_TIMEOUT_SECS").ok().and_then(|s| s.trim().parse::<f64>().ok()).filter(|s| s.is_finite() && *s > 0.0).map(Duration::from_secs_f64).unwrap_or(Duration::from_secs(600))
}

/// Every text and text box of every sheet, root first then the sheets in the order `BuildSheetList` visits them (`ReadDirectives` reads
/// `SCH_TEXT` and `SCH_TEXTBOX`).
fn sheet_texts(design: &Design) -> Vec<String> {
    fn visit(design: &Design, sch: &eda_model::ir::SchematicSection, out: &mut Vec<String>, depth: usize) {
        out.extend(sch.texts.iter().map(|t| t.content.clone()));
        for g in &sch.extras.graphics {
            if let eda_model::sch_extras::SchGraphicKind::TextBox { text, .. } = &g.shape {
                out.push(text.clone());
            }
        }
        if depth > 32 {
            return;
        }
        for child in &sch.sheets {
            if let Some(content) = design.sheet_contents.as_ref().and_then(|c| c.get(&child.file)) {
                visit(design, content, out, depth + 1);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(root) = design.schematic.as_ref() {
        visit(design, root, &mut out, 0);
    }
    out
}

/// `ResolveFile` of a library path for `writeInclude`: the path as it is when it is a file, else against the board's folder, else against
/// the derived project's.
fn resolver(dir: &Path) -> impl Fn(&str) -> Option<String> {
    let (board, work) = (dir.to_path_buf(), dir.join(".kicad"));
    move |p: &str| {
        let path = Path::new(p);
        let candidates: Vec<PathBuf> = if path.is_absolute() { vec![path.to_path_buf()] } else { vec![board.join(path), work.join(path)] };
        candidates.into_iter().find(|c| c.is_file()).map(|c| std::fs::canonicalize(&c).unwrap_or(c).to_string_lossy().into_owned())
    }
}

/// The circuit as the simulator sees it: kicad-cli's netlist cut into its parts, its items and the schematic's directives.
struct Circuit {
    design: Design,
    spice: String,
    directives: Vec<String>,
    items: Vec<CircuitItem>,
    /// `"REF.PIN"` -> the SPICE net on it.
    pin_nets: BTreeMap<String, String>,
    messages: Vec<String>,
}

fn read_circuit(dir: &Path) -> Result<Circuit, String> {
    let (design, nl) = kicad_engine::sim_netlists(dir).map_err(|e| crate::board::reasons(&e))?;
    let directives = read_directives(sheet_texts(&design).iter().map(String::as_str));
    let parts = split_netlist(&nl.spice, &directives);
    // The ports of the subcircuits of the libraries the netlist includes (a subcircuit's current names are per port).
    let resolve = resolver(dir);
    let mut libs = String::new();
    for l in &parts.head {
        if let Some(rest) = l.trim().strip_prefix(".include") {
            if let Some(text) = resolve(rest.trim().trim_matches('"')).and_then(|p| std::fs::read_to_string(p).ok()) {
                libs.push_str(&text);
                libs.push('\n');
            }
        }
    }
    let xml = parse_xml_netlist(&nl.xml);
    let items = circuit(&parts, &xml, &subckt_ports(&libs));
    let mut pin_nets = BTreeMap::new();
    for n in &xml.nets {
        for (r, p) in &n.nodes {
            pin_nets.insert(format!("{r}.{p}"), spice_net_name(&n.name));
        }
    }
    Ok(Circuit { design, spice: nl.spice, directives: parts.directives.clone(), items, pin_nets, messages: nl.messages })
}

fn pin_counts(items: &[CircuitItem]) -> BTreeMap<String, usize> {
    items.iter().map(|i| (i.name.clone(), i.pin_count)).collect()
}

fn item_json(i: &CircuitItem) -> Value {
    json!({
        "ref": i.reference,
        "name": i.name,
        "device": i.device.to_string(),
        "nodes": i.nodes,
        "pin_count": i.pin_count,
        "pin_index": i.pin_index,
        "currents": i.currents,
        "tuner": i.tuner.as_ref().map(|(p, v)| json!({ "param": p, "value": v })),
        "sim_device": i.sim_device,
        "sim_type": i.sim_type,
    })
}

fn options_of(req: &Value) -> u32 {
    req.get("options").and_then(Value::as_u64).map(|o| o as u32).unwrap_or(eda_sim::netlist::OPTION_DEFAULT_FLAGS)
}

/// `POST /api/sim/circuit`.
pub fn circuit_route(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let c = match read_circuit(dir) {
        Ok(c) => c,
        Err(e) => return err(e),
    };
    let nets = circuit_nets(&c.items);
    let sch_command = sch_text_sim_command(&c.directives);
    let mut reply = json!({
        "ok": true,
        "nets": nets,
        "items": c.items.iter().map(item_json).collect::<Vec<_>>(),
        "pin_nets": c.pin_nets,
        "directives": c.directives,
        "sch_sim_command": sch_command,
        "messages": c.messages,
        "ngspice": find_ngspice().is_some(),
        "project": kicad_engine::project_name_of(dir),
        "settings": {
            "ngspice": c.design.schematic.as_ref().and_then(|s| s.extras.ngspice.as_ref()),
            "workbook": c.design.schematic.as_ref().and_then(|s| s.extras.sim_workbook.as_ref()),
        },
    });
    if let Some(command) = req.get("command").and_then(Value::as_str) {
        let n = simulator_netlist(&c.spice, &c.directives, command, options_of(&req), &pin_counts(&c.items), &resolver(dir));
        reply["netlist"] = json!(n.text);
        let mut messages = c.messages.clone();
        messages.extend(n.messages);
        reply["messages"] = json!(messages);
    }
    reply
}

/// `NGSPICE_SETTINGS::model_mode` of the design (KiCad's default when it has none).
fn model_mode(design: &Design) -> u32 {
    design.schematic.as_ref().and_then(|s| s.extras.ngspice.as_ref()).map_or(eda_model::sch_extras::NgspiceSettings::LT_PSPICE, |n| n.model_mode)
}

fn plot_json(p: &RawPlot) -> Value {
    json!({
        "plot_name": p.plot_name,
        "complex": p.complex,
        "points": p.points,
        "vectors": p.vectors.iter().map(|v| {
            let mut o = json!({ "name": v.name, "kind": v.kind, "re": v.real });
            if let Some(im) = &v.imag {
                o["im"] = json!(im);
            }
            o
        }).collect::<Vec<_>>(),
    })
}

/// What a run needs from the netlist step: the deck and where its raw files go.
pub(crate) struct Prepared {
    netlist: String,
    deck_path: PathBuf,
    raw: eda_sim::deck::RawFiles,
    messages: Vec<String>,
    sim_type: SimType,
}

/// The netlist step of `POST /api/sim/run`, on the kicad-cli lane: the circuit read, the netlist and its control block written.
pub(crate) fn prepare(dir: &Path, req: &Value) -> Result<Prepared, String> {
    let command = req.get("command").and_then(Value::as_str).unwrap_or("").to_string();
    let options = options_of(req);
    let sim_type = command_to_sim_type(&command);
    let c = read_circuit(dir)?;
    // An FFT runs on the transient analysis of a TRAN tab (`SIMULATOR_FRAME::StartSimulation`: "You must run a TRAN simulation first; its
    // results will be used for the fast Fourier transform."): the netlist is that tab's.
    let (netlist_command, netlist_options) = if sim_type == SimType::Fft {
        let tran = req.get("fft_tran").filter(|t| t.is_object()).ok_or("You must run a TRAN simulation first; its results will be used for the fast Fourier transform.")?;
        (tran.get("command").and_then(Value::as_str).unwrap_or("").to_string(), options_of(tran))
    } else {
        (command.clone(), options)
    };
    let n = simulator_netlist(&c.spice, &c.directives, &netlist_command, netlist_options, &pin_counts(&c.items), &resolver(dir));
    let mut messages = c.messages.clone();
    messages.extend(n.messages.iter().cloned());
    // `applyTuners`: the item each tuned symbol is, and the reasons a tuner is not applied.
    let mut tuners = Vec::new();
    let mut tune_errors = Vec::new();
    for t in req.get("tuners").and_then(Value::as_array).into_iter().flatten() {
        let r = t.get("ref").and_then(Value::as_str).unwrap_or("");
        let value = t.get("value").and_then(Value::as_f64).unwrap_or(f64::NAN);
        match c.items.iter().find(|i| i.reference == r) {
            Some(i) if i.tuner.is_some() => tuners.push(Tuner { item: i.name.clone(), value }),
            Some(_) => tune_errors.push(format!("{r} is not tunable")),
            None => tune_errors.push(format!("{r} not found")),
        }
    }
    if !tune_errors.is_empty() {
        messages.push(format!("Could not apply tuned value(s):\n{}", tune_errors.join("\n")));
    }
    // `applyUserDefinedSignals`: the net names of the voltages the signals list has (`m_netnames`) are quoted.
    let netnames: Vec<String> = circuit_nets(&c.items).into_iter().filter(|n| n != "GND" && n != "0" && !n.starts_with("unconnected-_")).collect();
    let user_signals: Vec<(i64, String, String)> = req
        .get("user_signals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let id = s.get("id").and_then(Value::as_i64)?;
            let expr = s.get("expr").and_then(Value::as_str)?.to_string();
            Some((id, expr.clone(), quote_net_names(&expr, &netnames)))
        })
        .collect();
    let measurements: Vec<(usize, String)> = req
        .get("measurements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m.get("row").and_then(Value::as_u64)? as usize, m.get("text").and_then(Value::as_str)?.to_string())))
        .collect();
    let fourier: Vec<(String, String)> = req
        .get("fourier")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f.get("fundamental").and_then(Value::as_str)?.to_string(), f.get("signal").and_then(Value::as_str)?.to_string())))
        .collect();
    let controls = Controls { command: command.clone(), options, model_mode: model_mode(&c.design), tuners, user_signals, measurements, fourier };
    let sim_dir = dir.join(".kicad").join("sim");
    std::fs::create_dir_all(&sim_dir).map_err(|e| e.to_string())?;
    for f in ["run.raw", "run-2.raw", "run.cir", "run-deck.cir"] {
        let _ = std::fs::remove_file(sim_dir.join(f));
    }
    // ngspice runs in the board's folder (a library named relative to the project is found there); its raw files go beside the deck.
    std::fs::write(sim_dir.join("run.cir"), &n.text).map_err(|e| e.to_string())?;
    let (deck, raw) = driver(&controls, ".kicad/sim/run.cir", ".kicad/sim/run")?;
    let deck_path = sim_dir.join("run-deck.cir");
    std::fs::write(&deck_path, deck).map_err(|e| e.to_string())?;
    Ok(Prepared { netlist: n.text, deck_path, raw, messages, sim_type })
}

/// `POST /api/sim/run`, after the lane: ngspice, and its plots read back. `prepared` is what [`prepare`] made on the lane.
pub(crate) fn simulate(dir: &Path, prepared: Result<Prepared, String>) -> Value {
    let p = match prepared {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let Some(ngspice) = find_ngspice() else { return json!({ "ok": false, "message": NGSPICE_MISSING, "netlist": p.netlist, "messages": p.messages }) };
    let out = match run_batch(&ngspice, &p.deck_path, dir, slot(), run_limit()) {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "message": e, "netlist": p.netlist, "messages": p.messages }),
    };
    let read = |rel: &str| -> Result<Vec<RawPlot>, String> {
        let bytes = std::fs::read(dir.join(rel)).map_err(|_| format!("ngspice wrote no results ({rel})"))?;
        parse_raw(&bytes)
    };
    let mut reply = json!({
        "ok": out.success,
        "stopped": out.stopped,
        "timed_out": out.timed_out,
        "console": out.console,
        "netlist": p.netlist,
        "messages": p.messages,
        "sim_type": p.sim_type.short_name(),
    });
    if out.stopped {
        reply["message"] = json!("Simulation stopped.");
        return reply;
    }
    if out.timed_out {
        reply["message"] = json!(format!("ngspice did not finish within {} s and was killed (set EDA_SIM_TIMEOUT_SECS to give it more time)", run_limit().as_secs()));
        return reply;
    }
    match read(&p.raw.main) {
        Ok(plots) => reply["plots"] = Value::Array(plots.iter().map(plot_json).collect()),
        Err(e) => {
            reply["ok"] = json!(false);
            reply["message"] = json!(e);
        }
    }
    if let Some(second) = &p.raw.second {
        if let Ok(plots) = read(second) {
            reply["second_plots"] = Value::Array(plots.iter().map(plot_json).collect());
        }
    }
    reply
}

/// `POST /api/sim/run` run in full on the calling thread (the tests; the studio's route splits it, `studio::offload_sim_run`).
#[cfg(test)]
pub fn run_blocking(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if slot().is_running() {
        return err(eda_sim::ngspice::ALREADY_RUNNING);
    }
    simulate(dir, prepare(dir, &req))
}

/// `POST /api/sim/stop`.
pub fn stop_route() -> Value {
    json!({ "ok": true, "stopped": slot().stop() })
}

/// `POST /api/sim/workbook/parse` `{text}`: the workbook in a `.wbk` file's text, as KiCad's JSON (`{ok, workbook}`), or KiCad's "Unable to
/// load or parse file %s" reason.
pub fn workbook_parse_route(body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = req.get("text").and_then(Value::as_str).unwrap_or("");
    match parse_workbook(text) {
        Ok(wb) => json!({ "ok": true, "workbook": wb.to_value() }),
        Err(e) => err(e),
    }
}

/// `POST /api/sim/workbook/text` `{workbook}`: the bytes `SaveWorkbook` writes for it (`{ok, text}`).
pub fn workbook_text_route(body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match Workbook::from_value(req.get("workbook").unwrap_or(&Value::Null)) {
        Ok(wb) => json!({ "ok": true, "text": wb.to_kicad_text() }),
        Err(e) => err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board made from `yaml` the way `eda board new` makes one.
    fn board(name: &str, yaml: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-sim-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let intent = dir.join(format!("{name}.yaml"));
        std::fs::write(&intent, yaml).unwrap();
        let out = dir.join("board");
        let args: Vec<String> = ["new", intent.to_str().unwrap(), "-o", out.to_str().unwrap()].iter().map(|s| s.to_string()).collect();
        crate::board::run(&args, |model| {
            let empty = Design {
                schema: eda_model::ir::SCHEMA_VERSION,
                provenance: eda_model::ir::Provenance { engine_version: "t".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
                schematic: None,
                nets: None,
                placement: None,
                routing: None,
                drawings: None,
                footprint_library: None,
                sheet_contents: None,
                bus_aliases: vec![],
                symbol_library: None,
            };
            crate::seed_outline(&empty, model)
        })
        .unwrap();
        out
    }

    const RC: &str = "parts:\n  - { reference: V1, value: VSIN, symbol: \"Simulation_SPICE:VSIN\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R1, value: 1k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: C1, value: 1u, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\nnets:\n  - { name: IN, pins: [V1.1, R1.1] }\n  - { name: OUT, pins: [R1.2, C1.1] }\n  - { name: GND, pins: [V1.2, C1.2] }\n";

    /// A non-inverting amplifier of KiCad's built-in op-amp (`Simulation_SPICE:OPAMP`, `kicad_builtin_opamp`) with a gain of 11 on a
    /// tenth of a volt from a divider of its 12 V supply, and an RC charging from the supply beside it.
    const OPAMP: &str = "parts:\n  - { reference: V1, value: \"12\", symbol: \"Simulation_SPICE:VDC\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - reference: U1\n    value: OPAMP\n    symbol: \"Simulation_SPICE:OPAMP\"\n    pins: [{ number: \"1\", kind: signal }, { number: \"2\", kind: signal }, { number: \"3\", kind: power }, { number: \"4\", kind: power }, { number: \"5\", kind: signal }]\n  - { reference: R1, value: 10k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R2, value: 100k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R3, value: 119k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R4, value: 1k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: R5, value: 1k, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\n  - { reference: C1, value: 1u, package: \"0603\", pins: [{ number: \"1\", kind: passive }, { number: \"2\", kind: passive }] }\nnets:\n  - { name: VCC, pins: [V1.1, U1.3, R3.1, R5.1] }\n  - { name: GND, pins: [V1.2, U1.4, R4.2, R1.2, C1.2] }\n  - { name: IN, pins: [R3.2, R4.1, U1.1] }\n  - { name: FB, pins: [U1.2, R2.1, R1.1] }\n  - { name: OUT, pins: [U1.5, R2.2] }\n  - { name: RC, pins: [R5.2, C1.1] }\n";

    fn slow() -> bool {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return false;
        }
        if find_ngspice().is_none() {
            eprintln!("skipped: {NGSPICE_MISSING}");
            return false;
        }
        true
    }

    fn run(dir: &Path, req: Value) -> Value {
        let r = run_blocking(dir, req.to_string().as_bytes());
        assert_eq!(r["ok"], true, "{}", serde_json::to_string_pretty(&r).unwrap());
        r
    }

    fn vector<'a>(reply: &'a Value, name: &str) -> Vec<f64> {
        let plot = &reply["plots"][0];
        let v = plot["vectors"].as_array().unwrap().iter().find(|v| v["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).unwrap_or_else(|| panic!("no {name} in {:?}", plot["vectors"].as_array().unwrap().iter().map(|v| v["name"].clone()).collect::<Vec<_>>()));
        v["re"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    fn imag(reply: &Value, name: &str) -> Vec<f64> {
        let v = reply["plots"][0]["vectors"].as_array().unwrap().iter().find(|v| v["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).unwrap();
        v["im"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    /// `y` at `x` by linear interpolation (`CURSOR::doSetCoordX`).
    fn at(xs: &[f64], ys: &[f64], x: f64) -> f64 {
        let i = xs.partition_point(|v| *v <= x).clamp(1, xs.len() - 1);
        ys[i - 1] + (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]) * (x - xs[i - 1])
    }

    /// Slow tier (kicad-cli and ngspice): an RC filter and an op-amp amplifier simulated end to end -- kicad-cli's netlist, KiCad's
    /// simulator netlist, ngspice in batch mode, its raw file read -- against the voltages worked out by hand.
    #[test]
    fn an_rc_filter_and_an_op_amp_simulate_to_the_hand_computed_values() {
        if !slow() {
            return;
        }
        let opts = eda_sim::netlist::OPTION_DEFAULT_FLAGS;

        // The op-amp: the operating point. The divider gives 12 V * 1k / 120k = 0.1 V. The model (`kicad_builtin_opamp`) has an offset of
        // 10 mV on its + input, a gain A = 20k and 10 ohm of output resistance into the 110k of the feedback divider, so
        // out = k A (0.1 - 0.01) / (1 + k A / 11), k = 110k / (110k + 10).
        let b = board("opamp", OPAMP);
        let circuit = circuit_route(&b, json!({ "command": ".op", "options": opts }).to_string().as_bytes());
        assert_eq!(circuit["ok"], true, "{circuit}");
        let items = circuit["items"].as_array().unwrap();
        let u1 = items.iter().find(|i| i["ref"] == "U1").unwrap();
        assert_eq!(u1["name"], "XU1");
        assert_eq!(u1["currents"][4], "I(XU1:out)");
        assert_eq!(circuit["pin_nets"]["U1.5"], "/OUT");
        assert!(items.iter().find(|i| i["ref"] == "R5").unwrap()["tuner"]["value"] == "1k");
        let netlist = circuit["netlist"].as_str().unwrap();
        assert!(netlist.contains(".save all\n.probe alli\n") && netlist.contains(".op\n") && netlist.ends_with(".end\n"), "{netlist}");

        let op = run(&b, json!({ "command": ".op", "options": opts }));
        let k = 110_000.0 / 110_010.0;
        let a = 20_000.0;
        let want_out = k * a * 0.09 / (1.0 + k * a / 11.0);
        let out = vector(&op, "v(/out)")[0];
        assert!((out - want_out).abs() < 1e-4 * want_out, "V(/OUT) = {out}, by hand {want_out}");
        assert!((vector(&op, "v(/in)")[0] - 0.1).abs() < 1e-9);
        assert!(op["console"].as_str().unwrap().contains("Circuit: KiCad schematic"), "{}", op["console"]);

        // The RC beside it charging from 12 V (".tran 10u 5m 0 10u uic", what the Transient page writes for "use initial conditions"):
        // 12 (1 - e^-1) at t = RC = 1 ms. A tuned R5 of 2k halves the rate: 12 (1 - e^-0.5).
        let tran = run(&b, json!({ "command": ".tran 10u 5m 0 10u uic", "options": opts, "measurements": [{ "row": 0, "text": "MAX V(/RC)" }], "user_signals": [{ "id": 0, "expr": "V(/RC)*2" }] }));
        let (t, rc) = (vector(&tran, "time"), vector(&tran, "v(/rc)"));
        let want = 12.0 * (1.0 - (-1.0f64).exp());
        let got = at(&t, &rc, 1e-3);
        assert!((got - want).abs() < 0.005 * want, "V(RC) at 1 ms = {got}, by hand {want}");
        let max = vector(&tran, "meas_result_0")[0];
        let want_max = 12.0 * (1.0 - (-5.0f64).exp());
        assert!((max - want_max).abs() < 0.005 * want_max, "MAX = {max}, by hand {want_max}");
        let user = vector(&tran, "user0");
        assert!((at(&t, &user, 1e-3) - 2.0 * got).abs() < 1e-9, "user0 is twice the signal");
        let tuned = run(&b, json!({ "command": ".tran 10u 5m 0 10u uic", "options": opts, "tuners": [{ "ref": "R5", "value": 2000.0 }] }));
        let got2 = at(&vector(&tuned, "time"), &vector(&tuned, "v(/rc)"), 1e-3);
        let want2 = 12.0 * (1.0 - (-0.5f64).exp());
        assert!((got2 - want2).abs() < 0.005 * want2, "tuned V(RC) at 1 ms = {got2}, by hand {want2}");

        // The RC filter: R 1k, C 1u, driven by VSIN's "ac=1". |H| = 1 / sqrt(1 + (2 pi f R C)^2), the phase -atan(2 pi f R C).
        let a_board = board("rc", RC);
        let ac = run(&a_board, json!({ "command": ".ac dec 10 1 100k", "options": opts }));
        let f = vector(&ac, "frequency");
        let (re, im) = (vector(&ac, "v(/out)"), imag(&ac, "v(/out)"));
        let i = f.iter().position(|x| (x - 1000.0).abs() < 1.0).expect("1 kHz is a point of the sweep");
        let w = 2.0 * std::f64::consts::PI * f[i] * 1e-3;
        let gain = re[i].hypot(im[i]);
        assert!((gain - 1.0 / (1.0 + w * w).sqrt()).abs() < 1e-6, "|H(1k)| = {gain}");
        assert!((im[i].atan2(re[i]) + w.atan()).abs() < 1e-6, "phase");
        // the -3 dB point
        let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e-3);
        let gains: Vec<f64> = re.iter().zip(&im).map(|(r, i)| 20.0 * r.hypot(*i).log10()).collect();
        assert!((at(&f, &gains, fc) + 3.0103).abs() < 0.05, "{} dB at fc", at(&f, &gains, fc));

        // A noise analysis writes both of its plots.
        let noise = run(&a_board, json!({ "command": ".noise v(/OUT) V1 dec 10 1 10k", "options": opts }));
        assert!(noise["plots"][0]["vectors"].as_array().unwrap().iter().any(|v| v["name"] == "onoise_spectrum"), "{}", noise["plots"][0]["plot_name"]);
        assert!(noise["second_plots"][0]["vectors"].as_array().unwrap().iter().any(|v| v["name"].as_str().is_some_and(|n| n.contains("onoise_total"))));

        // A DC sweep of the supply: the divider's output follows it, a 120th of it.
        let dc = run(&b, json!({ "command": ".dc V1 0 12 1", "options": opts }));
        let sweep = vector(&dc, "v(v-sweep)");
        assert_eq!(sweep.len(), 13);
        assert!((vector(&dc, "v(/in)")[12] - 0.1).abs() < 1e-9);
    }

    #[test]
    fn the_workbook_routes_read_and_write_kicads_text() {
        let text = "{\n  \"custom_cursors\": 2,\n  \"tabs\": [],\n  \"user_defined_signals\": [],\n  \"version\": 7\n}\n";
        let parsed = workbook_parse_route(json!({ "text": text }).to_string().as_bytes());
        assert_eq!(parsed["ok"], true);
        let back = workbook_text_route(json!({ "workbook": parsed["workbook"] }).to_string().as_bytes());
        assert_eq!(back["text"], text);
        assert_eq!(workbook_parse_route(br#"{"text": "{ nope"}"#)["ok"], false);
    }

    /// The Simulation Model dialog's write (`DIALOG_SIM_MODEL::TransferDataFromWindow` for a library model, `web/studio`'s SimModelDialog): U1
    /// made a subcircuit of a library in the project folder -- `Sim.Library` relative, `Sim.Name`, `Sim.Device` "SUBCKT" and `Sim.Pins` -- and
    /// kicad-cli's netlist and ngspice find it there. The library's op-amp is a VCVS of gain 1e5, so V(OUT) is very nearly 11 * 0.1 V.
    #[test]
    fn a_library_subcircuit_the_model_dialog_picks_simulates() {
        if !slow() {
            return;
        }
        let opts = eda_sim::netlist::OPTION_DEFAULT_FLAGS;
        let b = board("libamp", OPAMP);
        std::fs::create_dir_all(b.join("models")).unwrap();
        std::fs::write(b.join("models/amp.lib"), "* a test op-amp\n.subckt MYAMP inp inn vcc vee out\nE1 out 0 inp inn 1e5\nR1 vcc vee 1Meg\n.ends MYAMP\n").unwrap();
        // the dialog opens on the library symbol's fields (the board keeps no schematic of its own yet: the derived one is read)
        let u1 = crate::sim_model_api::symbol_route(&b, json!({ "ref": "U1" }).to_string().as_bytes());
        assert_eq!((u1["fields"]["Sim.Name"].as_str(), u1["pins"].as_array().map(Vec::len)), (Some("kicad_builtin_opamp"), Some(5)), "{u1}");
        let lib = crate::sim_model_api::library_route(&b, json!({ "path": "models/amp.lib" }).to_string().as_bytes());
        assert_eq!(lib["models"][0]["pins"], json!(["inp", "inn", "vcc", "vee", "out"]), "{lib}");
        let edit = |field: &str, value: &str| eda_ops::fields_table::FieldEdit { id: "U1".into(), field: field.into(), value: value.into() };
        let cmd = eda_ops::Cmd::SetSymbolFields {
            edits: vec![edit("Sim.Library", "models/amp.lib"), edit("Sim.Name", "MYAMP"), edit("Sim.Device", "SUBCKT"), edit("Sim.Pins", "1=inp 2=inn 3=vcc 4=vee 5=out")],
            add_fields: vec![],
            rename_fields: vec![],
            remove_fields: vec![],
        };
        crate::board::step(&b, cmd, false, "test").unwrap();
        let op = run(&b, json!({ "command": ".op", "options": opts }));
        // V(IN) is 12 V * 1k / 120k = 0.1 V; the feedback divider takes a 1/11 of V(OUT) back: V(OUT) = A V(IN) / (1 + A / 11)
        let a = 1e5;
        let want_out = a * 0.1 / (1.0 + a / 11.0);
        let out = vector(&op, "v(/out)")[0];
        assert!((out - want_out).abs() < 1e-6, "V(/OUT) = {out}, by hand {want_out}; netlist {}", op["netlist"]);
        assert!(op["netlist"].as_str().unwrap().contains("XU1 /IN /FB VCC GND /OUT MYAMP"), "{}", op["netlist"]);
    }
}
