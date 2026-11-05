//! What the Simulation Model dialog lists of an IBIS file (`.ibs`): its components, each one's pins (number and signal name), the models a pin
//! can take, and its differential pins -- `SIM_LIBRARY_IBIS::ReadFile` / `InitModel` over `KIBIS`, read the way `IbisParser` reads the lines.
//!
//! Ported from `eeschema/sim/kibis/ibis_parser.cpp` (KiCad 8303b2ad), the parts that give those lists:
//! - `getNextLine`: a line ends at a newline, and at the comment character (`|`, or the one `[Comment Char]` sets: `changeCommentChar`);
//! - `getKeyword`: a keyword is a `[...]` that starts the line, its spaces read as underscores, compared without case (`compareIbisWord`);
//! - `changeContext`: `[Component]` (its name the rest of the line, `storeString`), `[Model Selector]`, `[Model]`, `[End]`;
//! - `readPin`: the `[Pin]` table, its first line the column heads (a "dummy" pin, which `KIBIS_COMPONENT` leaves out), then
//!   `pin_name signal_name model_name [R L C]`; `readDiffPin` (`pin inv_pin ..`, its first line skipped too); `readModelSelector`
//!   (`model_name description`);
//! - `KIBIS_PIN::KIBIS_PIN`: a pin's models are its model selector's entries, or its model name, each one that is a `[Model]` of the file;
//!   `KIBIS::KIBIS`: a pin is differential when a `[Diff Pin]` row names it and its inverse pin is a pin of the component.
//!
//! Not ported (the rest of KIBIS, about 4,800 lines): the model tables (I-V, V-t, ramps, packages, submodels) and their checks, so a file
//! KiCad refuses as invalid ("Invalid IBIS file '%s'") may be listed here; the SPICE of a pin's model, which kicad-cli writes in the netlist.

/// One `[Component]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisComponent {
    pub name: String,
    /// `m_ibisPins`: (pin number, signal name), in the file's order, the dummy first row left out.
    pub pins: Vec<(String, String)>,
    /// Each pin's models (`KIBIS_PIN::m_models`' names), in the order of `pins`.
    pub pin_models: Vec<Vec<String>>,
    /// The pins a `[Diff Pin]` row pairs (`SIM_LIBRARY_IBIS::m_diffPins`).
    pub diff_pins: Vec<String>,
}

/// The lists of one file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisFile {
    pub components: Vec<IbisComponent>,
    /// `[Model]` names.
    pub models: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Continue {
    None,
    Pin,
    DiffPin,
    ModelSelector,
}

/// Read the lists of an IBIS file's text.
pub fn read_ibis(text: &str) -> IbisFile {
    let mut comment = '|';
    let mut components: Vec<IbisComponent> = Vec::new();
    // per component: the raw pin rows (pin, signal, model) and diff rows (pin, inverse)
    let mut pin_rows: Vec<Vec<(String, String, String)>> = Vec::new();
    let mut diff_rows: Vec<Vec<(String, String)>> = Vec::new();
    let mut selectors: Vec<(String, Vec<String>)> = Vec::new();
    let mut models: Vec<String> = Vec::new();
    let mut cont = Continue::None;
    let mut pin_header_seen = false;
    let mut diff_header_seen = false;

    for raw in text.split('\n') {
        // `getNextLine`: the line up to the comment character
        let line: String = raw.trim_end_matches('\r').chars().take_while(|c| *c != comment).collect();
        if let Some(rest) = line.strip_prefix('[') {
            // `getKeyword`
            let close = rest.find(']').unwrap_or(rest.len());
            let keyword = rest[..close].replace(' ', "_").to_ascii_lowercase();
            let after = rest.get(close + 1..).unwrap_or("").trim();
            cont = Continue::None;
            match keyword.as_str() {
                "comment_char" => {
                    // `changeCommentChar`: "|_char"
                    if let Some(c) = after.chars().next() {
                        comment = c;
                    }
                }
                "component" => {
                    components.push(IbisComponent { name: after.to_string(), ..Default::default() });
                    pin_rows.push(Vec::new());
                    diff_rows.push(Vec::new());
                }
                "pin" => {
                    cont = Continue::Pin;
                    pin_header_seen = false;
                    // `readPin` on the keyword's own line: its column heads, a dummy pin
                    if !after.is_empty() {
                        pin_header_seen = true;
                    }
                }
                "diff_pin" => {
                    cont = Continue::DiffPin;
                    diff_header_seen = !after.is_empty();
                }
                "model_selector" => {
                    selectors.push((after.to_string(), Vec::new()));
                    cont = Continue::ModelSelector;
                }
                "model" => models.push(after.to_string()),
                "end" => break,
                _ => {}
            }
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        match cont {
            Continue::Pin => {
                if !pin_header_seen {
                    pin_header_seen = true;
                    continue;
                }
                if fields.len() >= 3 {
                    if let Some(rows) = pin_rows.last_mut() {
                        rows.push((fields[0].to_string(), fields[1].to_string(), fields[2].to_string()));
                    }
                }
            }
            Continue::DiffPin => {
                if !diff_header_seen {
                    diff_header_seen = true;
                    continue;
                }
                if fields.len() >= 2 {
                    if let Some(rows) = diff_rows.last_mut() {
                        rows.push((fields[0].to_string(), fields[1].to_string()));
                    }
                }
            }
            Continue::ModelSelector => {
                if let Some((_, entries)) = selectors.last_mut() {
                    entries.push(fields[0].to_string());
                }
            }
            Continue::None => {}
        }
    }

    for (k, comp) in components.iter_mut().enumerate() {
        for (pin, signal, model) in &pin_rows[k] {
            comp.pins.push((pin.clone(), signal.clone()));
            // `KIBIS_PIN::KIBIS_PIN`: the selector of that name, else the model itself; only `[Model]`s of the file
            let wanted: Vec<String> = match selectors.iter().find(|(n, _)| n == model) {
                Some((_, entries)) => entries.clone(),
                None => vec![model.clone()],
            };
            let mut list = Vec::new();
            for w in &wanted {
                for m in &models {
                    if m == w {
                        list.push(m.clone());
                    }
                }
            }
            comp.pin_models.push(list);
        }
        for (a, b) in &diff_rows[k] {
            let has = |p: &String| comp.pins.iter().any(|(n, _)| n == p);
            if has(a) && has(b) {
                for p in [a, b] {
                    if !comp.diff_pins.contains(p) {
                        comp.diff_pins.push(p.clone());
                    }
                }
            }
        }
    }
    IbisFile { components, models }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IBS: &str = "[IBIS Ver]   5.1\n[Comment Char] #_char\n# a comment | not one\n[File Name]  t.ibs\n[Component]  ACME 1\n[Manufacturer] ACME\n[Package]\nR_pkg 1 1 1\n[Pin]  signal_name  model_name  R_pin L_pin C_pin\n1  OUT  driver  0.1 1n 1p # the output\n2  GND  GND  NA NA NA\n3  IOP  io_sel NA NA NA\n4  ION  io_sel NA NA NA\n[Diff Pin] inv_pin vdiff tdelay_typ tdelay_min tdelay_max\n3 4 0.1 0 NA NA\n[Model Selector] io_sel\nio33 3.3 V IO\nio18 1.8 V IO\n[Model] driver\nModel_type Output\n[Model] io33\n[Model] io18\n[End]\n[Component] after the end\n";

    #[test]
    fn components_pins_models_and_differential_pins_as_kibis_lists_them() {
        let f = read_ibis(IBS);
        assert_eq!(f.models, ["driver", "io33", "io18"]);
        assert_eq!(f.components.len(), 1, "nothing after [End]");
        let c = &f.components[0];
        assert_eq!(c.name, "ACME 1");
        assert_eq!(c.pins, [("1".into(), "OUT".into()), ("2".into(), "GND".into()), ("3".into(), "IOP".into()), ("4".into(), "ION".into())]);
        assert_eq!(c.pin_models, [vec!["driver".to_string()], vec![], vec!["io33".into(), "io18".into()], vec!["io33".into(), "io18".into()]], "GND is no [Model]; a selector's entries");
        assert_eq!(c.diff_pins, ["3", "4"]);
    }

    #[test]
    fn a_pin_table_whose_heads_are_on_the_next_line_skips_that_line() {
        let f = read_ibis("[Component] X\n[Pin]\nsignal_name model_name\n7 A m\n[Model] m\n");
        assert_eq!(f.components[0].pins, [("7".to_string(), "A".to_string())]);
        assert_eq!(f.components[0].pin_models, [vec!["m".to_string()]]);
    }
}
