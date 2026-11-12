//! An IBIS file (`.ibs`) read the way KiCad reads it for the Simulation Model dialog: `SIM_LIBRARY_IBIS::ReadFile` builds a `KIBIS` of the
//! file, which runs `IbisParser::ParseFile` over the whole file, checks every section as the parser leaves it, and is refused
//! ("Invalid IBIS file '%s'") when anything failed (`KIBIS::m_valid`). What the dialog then lists of it are its components, each one's pins
//! (number and signal name), the models a pin can take, and its differential pins.
//!
//! Ported from `eeschema/sim/kibis/ibis_parser.cpp` / `ibis_parser.h` (KiCad 8303b2ad), the whole parser:
//! - `ParseFile`: the file a line at a time (`getNextLine`: a line ends at a newline, at a NUL and at the comment character, `|` or the one
//!   `[Comment Char]` sets, `changeCommentChar`; a line of 2048 characters is "Line exceeds maximum length."), each one given to
//!   `onNewLine`, the parse stopping at the first line that fails ("Error on line %d") or at `[End]` (a file that ends without `[End]` is
//!   not an error, and its last section is never checked); then every `[Add Submodel]` must name a `[Submodel]` of the file
//!   ("%s submodel not found.");
//! - `onNewLine` / `getKeyword`: a keyword is a `[...]` that starts the line, its spaces read as underscores, compared without case
//!   (`compareIbisWord`), given to the parse function of the context (`parseHeader`, `parseComponent`, `parseModelSelector`, `parseModel`,
//!   `parseSubmodel`, `parsePackageModel`, `parsePackageModelModelData`, `parseAlgorithmicModel`); any other line continues the keyword before
//!   it (`m_continue`: `readString`, `readPackage`, `readPin`, `readPinMapping`, `readDiffPin`, `readSeriesPinMapping`, `readModelSelector`,
//!   `readModel`, `readSubmodel`, `readIVtableEntry`, `readVTtableEntry`, `readWaveform`, `readRamp`, `readModelSpec`,
//!   `readReceiverThresholds`, `readAlgorithmicModel`, `readAddSubmodel`, `readSubmodelSpec`, `readPackageModelPins`, `readMatrixData`), else
//!   "Missing keyword.";
//! - `changeContext`: a keyword the context does not know checks the section being left (`IbisHeader::Check`, `IbisComponent::Check`,
//!   `IbisModel::Check`, `IbisSubmodel::Check`, `IbisModelSelector::Check`, `IbisPackageModel::Check`) and starts `[Component]`,
//!   `[Model Selector]`, `[Model]`, `[Submodel]`, `[Define Package Model]`, `[End Package Model]` or `[End]`; anything else is "Unknown keyword
//!   in %s context: %s". A failed check ends the parse there (the context becomes `END`), the keyword not looked at;
//! - the readers of values: `skipWhitespaces`, `checkEndofLine`, `isLineEmptyFromCursor`, `readWord`, `readString`, `storeString`,
//!   `readTableLine`, `parseDouble` (`std::stod` and a scale letter: `T G M k m u n p f`), `readDouble`, `readInt`, `parseDvdt`, `readDvdt`,
//!   `readRampdvdt`, `readNumericSubparam`, `readTypMinMaxValue`, `readTypMinMaxValueSubparam`, `readMatrixType`, `readMatrixBandwidth`,
//!   `readMatrixRow`, `readMatrixPinIndex`, `readMatrixBandedOrFull`, `readMatrixSparse`;
//! - the checks with their messages word for word: `IBIS_MATRIX::Check`, `TypMinMaxValue::Check`, `IbisComponentPackage::Check`,
//!   `IbisComponentPin::Check`, `IbisComponent::Check`, `IbisModelSelector::Check`, `IVtable::Check`, `dvdtTypMinMax::Check`,
//!   `IbisRamp::Check`, `IbisModel::Check`, `IbisSubmodel::Check`, `IbisHeader::Check`, `IbisPackageModel::Check`.
//!
//! And from `eeschema/sim/kibis/kibis.cpp`, what decides `m_valid` and what the dialog lists: `KIBIS::KIBIS` (valid when `ParseFile` and
//! `KIBIS_FILE::Init` succeed and every `KIBIS_MODEL` and `KIBIS_COMPONENT` is valid -- which they always are in 8303b2ad, so it is the
//! parse's status), `KIBIS_COMPONENT::KIBIS_COMPONENT` (the dummy first row of `[Pin]` left out), `KIBIS_PIN::KIBIS_PIN` (a pin's models are
//! its model selector's entries, or its model name, each one that is a `[Model]` of the file) and the `[Diff Pin]` pairing (a pin is
//! differential when a row names it and its inverse pin is a pin of the component, `isDiffPin`).
//!
//! The messages are what `IBIS_BASE::Report` hands the dialog's reporter, in order and with the severity KiCad gives (a `Report` without one
//! is `RPT_SEVERITY_INFO`). Translations are off in KiCad here (`#define _( x ) x`), so the texts are the English ones. Note that a message
//! of severity ERROR does not always fail the parse (an invalid `[Ramp]`, a row of an I-V table whose typ current cannot be read): the file
//! is then valid, but the dialog, which refuses a library on any error, still refuses it.
//!
//! Where the C++ throws: `std::vector::at`, `std::string::at` and `std::string::substr` throw `std::out_of_range` in a few places
//! (`readPin` on a row with too few columns or a `[Pin]` keyword line without its R_pin/L_pin/C_pin heads, `parseDvdt` on a dV/dt value with
//! no `/`, `IbisHeader::Check` on a `[File Name]` shorter than four characters, a missing one included). Nothing in the dialog's path catches
//! it, so KiCad ends in `OnExceptionInMainLoop` / `PGM_BASE::HandleException` ("Unhandled exception class: %s  what: %s") and stops. Here the
//! parse stops there too, that line is the last message (as an error, so the dialog refuses the file), `thrown` holds the exception's
//! `what()` (libstdc++'s text) and no `KIBIS` is built.
//!
//! Not ported: the SPICE of a pin's model (`KIBIS_PIN::getKuKdOneWaveform`, `KIBIS_MODEL::generateSquareWave`,
//! `SPICE_GENERATOR_IBIS::IbisDevice`, `IVtable::Spice`, `IVtable::InterpolatedI`), which kicad-cli writes in the netlist; the series-element
//! keywords of KiCad 25ec2335 (`[R Series]`, `[On]`, `[Off]`, `[Series MOSFET]`, ... and their checks), a feature added after 8303b2ad and
//! not a fix, so in 8303b2ad's parser (as here) they are "Unknown keyword in MODEL context"; KiCad 25ec2335's other change to the parser
//! (`readWaveform` deleting a waveform made in a wrong context) is a leak fix with nothing to port; and `wxLogMessage` lines, which go to
//! KiCad's log, not to the reporter. `IBIS_PARSER_CONTINUE::VT_TABLE` is never set in KiCad (`readVTtableEntry` sets `IV_TABLE`, which
//! `readWaveform` overrides), so it is left out.

use std::collections::BTreeMap;

/// `SEVERITY` as `IBIS_BASE::Report` hands it over (`RPT_SEVERITY_INFO` when the call names none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Action,
    Warning,
    Error,
}

/// `nan( NAN_NA )`: a value given as `NA` (a NaN whose payload is 1).
pub fn na() -> f64 {
    f64::from_bits(0x7ff8_0000_0000_0001)
}

/// `nan( NAN_INVALID )`: a value that could not be read (a NaN whose payload is 0).
fn nan_invalid() -> f64 {
    f64::from_bits(0x7ff8_0000_0000_0000)
}

/// `isNumberNA`: the bits of `nan( NAN_NA )` (`memcmp`), so any other NaN is an invalid value.
pub fn is_number_na(x: f64) -> bool {
    x.to_bits() == na().to_bits()
}

type Rep = Vec<(Severity, String)>;

fn report(rep: &mut Rep, msg: impl Into<String>, severity: Severity) {
    rep.push((severity, msg.into()));
}

/// A `std::out_of_range` the C++ throws, with libstdc++'s `what()`.
#[derive(Debug)]
struct Thrown(String);

type R = Result<bool, Thrown>;

/// `std::vector::at`.
fn vec_at(v: &[String], n: usize) -> Result<&String, Thrown> {
    v.get(n).ok_or_else(|| Thrown(format!("vector::_M_range_check: __n (which is {n}) >= this->size() (which is {})", v.len())))
}

/// C's `isspace` in the C locale (`LOCALE_IO`).
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `IbisParser::compareIbisWord`: equal, without case.
fn compare_ibis_word(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

/// `IBIS_CORNER`: the index of `TypMinMaxValue::value`.
pub const TYP: usize = 0;
pub const MIN: usize = 1;
pub const MAX: usize = 2;

/// `TypMinMaxValue`: typ, min, max, each `NA` until read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypMinMaxValue {
    pub value: [f64; 3],
}

impl Default for TypMinMaxValue {
    fn default() -> Self {
        TypMinMaxValue { value: [na(); 3] }
    }
}

impl TypMinMaxValue {
    /// `TypMinMaxValue::isNA`: the typ value is `NA`.
    pub fn is_na(&self) -> bool {
        is_number_na(self.value[TYP])
    }

    /// `TypMinMaxValue::Check`: no corner is an invalid NaN (`NA` is fine).
    pub fn check(&self) -> bool {
        self.value.iter().all(|v| !(v.is_nan() && !is_number_na(*v)))
    }

    /// `TypMinMaxValue::Add`.
    pub fn add(&mut self, other: &TypMinMaxValue) {
        if !other.is_na() {
            self.value[TYP] += other.value[TYP];
            for c in [MIN, MAX] {
                if is_number_na(self.value[c]) {
                    self.value[c] += other.value[TYP];
                } else {
                    self.value[c] += other.value[c];
                }
            }
        }
    }
}

/// `IBIS_MATRIX_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisMatrixType {
    #[default]
    Undefined,
    Banded,
    Sparse,
    Full,
}

/// `IBIS_MATRIX`.
#[derive(Debug, Clone, PartialEq)]
pub struct IbisMatrix {
    pub kind: IbisMatrixType,
    pub rows: i32,
    pub cols: i32,
    pub data: Vec<f64>,
}

impl Default for IbisMatrix {
    fn default() -> Self {
        IbisMatrix { kind: IbisMatrixType::Undefined, rows: -1, cols: -1, data: Vec::new() }
    }
}

impl IbisMatrix {
    /// `IBIS_MATRIX::Check`.
    fn check(&self, rep: &mut Rep) -> bool {
        if self.rows < 1 || self.cols < 1 {
            report(rep, "Dimension of matrices should be >= 1.", Severity::Error);
            return false;
        }
        if self.data.iter().any(|d| d.is_nan()) {
            report(rep, "There are NaN elements in a matrix.", Severity::Error);
            return false;
        }
        true
    }
}

/// `IbisHeader`.
#[derive(Debug, Clone, PartialEq)]
pub struct IbisHeader {
    pub ibis_version: f64,
    pub file_revision: f64,
    pub file_name: String,
    pub source: String,
    pub date: String,
    pub notes: String,
    pub disclaimer: String,
    pub copyright: String,
}

impl Default for IbisHeader {
    fn default() -> Self {
        IbisHeader {
            ibis_version: -1.0,
            file_revision: -1.0,
            file_name: String::new(),
            source: String::new(),
            date: String::new(),
            notes: String::new(),
            disclaimer: String::new(),
            copyright: String::new(),
        }
    }
}

/// `IBIS_MAX_VERSION`: "Up to v7.0, IBIS is fully backward compatible".
const IBIS_MAX_VERSION: f64 = 7.0;

impl IbisHeader {
    /// `IbisHeader::Check`. Its `m_fileName.substr( m_fileName.length() - 4 )` throws for a name shorter than four characters.
    fn check(&self, rep: &mut Rep) -> R {
        let mut status = true;
        let message = "Checking Header...";
        let fail = |status: &mut bool, rep: &mut Rep, msg: &str| {
            if *status {
                report(rep, message, Severity::Action);
            }
            report(rep, msg, Severity::Error);
            *status = false;
        };
        if self.ibis_version == -1.0 {
            fail(&mut status, rep, "Missing [IBIS Ver]");
        }
        if self.ibis_version > IBIS_MAX_VERSION {
            fail(&mut status, rep, "The parser does not handle this IBIS version");
        }
        if self.file_revision == -1.0 {
            fail(&mut status, rep, "Missing [File Rev]");
        }
        if self.file_name.is_empty() {
            fail(&mut status, rep, "Missing [File Name]");
        }
        let name = self.file_name.as_bytes();
        if name.len() < 4 {
            return Err(Thrown(format!(
                "basic_string::substr: __pos (which is {}) > this->size() (which is {})",
                (name.len() as u64).wrapping_sub(4),
                name.len()
            )));
        }
        let ext = &name[name.len() - 4..];
        if !(ext == b".ibs" || ext == b".pkg" || ext == b".ebd" || ext == b".ims") {
            fail(&mut status, rep, "Invalid file extension in [File Name]");
        }
        Ok(status)
    }
}

/// `IbisComponentPackage`: `[Package]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisComponentPackage {
    pub r_pkg: TypMinMaxValue,
    pub l_pkg: TypMinMaxValue,
    pub c_pkg: TypMinMaxValue,
}

impl IbisComponentPackage {
    /// `IbisComponentPackage::Check`: the three typ values are given.
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        for (v, msg) in [
            (&self.r_pkg, "Missing or invalid R_pkg value."),
            (&self.l_pkg, "Missing or invalid L_pkg value."),
            (&self.c_pkg, "Missing or invalid C_pkg value."),
        ] {
            if !v.check() || v.is_na() {
                report(rep, msg, Severity::Error);
                status = false;
            }
        }
        status
    }
}

/// `IbisComponentPin`: a row of `[Pin]` (the first one, the column heads, is a `dummy`).
#[derive(Debug, Clone, PartialEq)]
pub struct IbisComponentPin {
    pub pin_name: String,
    pub signal_name: String,
    pub model_name: String,
    pub r_pin: f64,
    pub l_pin: f64,
    pub c_pin: f64,
    pub r_col: i32,
    pub l_col: i32,
    pub c_col: i32,
    pub dummy: bool,
}

impl Default for IbisComponentPin {
    fn default() -> Self {
        IbisComponentPin {
            pin_name: String::new(),
            signal_name: String::new(),
            model_name: String::new(),
            r_pin: na(),
            l_pin: na(),
            c_pin: na(),
            r_col: 0,
            l_col: 0,
            c_col: 0,
            dummy: false,
        }
    }
}

impl IbisComponentPin {
    /// `IbisComponentPin::Check`.
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        if !self.dummy {
            let message = format!("Checking pin {}", self.pin_name);
            let invalid = |v: f64| v.is_nan() && !is_number_na(v);
            for (bad, msg) in [
                (self.pin_name.is_empty(), "Pin name cannot be empty."),
                (self.signal_name.is_empty(), "Signal name cannot be empty."),
                (self.model_name.is_empty(), "Model name cannot be empty."),
                (invalid(self.r_pin), "Rpin is not valid."),
                (invalid(self.l_pin), "Lpin is not valid."),
                (invalid(self.c_pin), "Cpin is not valid."),
            ] {
                if bad {
                    if status {
                        report(rep, message.clone(), Severity::Action);
                    }
                    report(rep, msg, Severity::Error);
                    status = false;
                }
            }
        }
        status
    }
}

/// `IbisComponentPinMapping`: a row of `[Pin Mapping]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisComponentPinMapping {
    pub pin_name: String,
    pub pd_ref: String,
    pub pu_ref: String,
    pub gnd_clamp_ref: String,
    pub power_clamp_ref: String,
    pub ext_ref: String,
}

/// `IbisDiffPinEntry`: a row of `[Diff Pin]` (the parser reads its two pins only; `vdiff` and `tdelay` keep their defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct IbisDiffPinEntry {
    pub pin_a: String,
    pub pin_b: String,
    pub vdiff: f64,
    pub tdelay: TypMinMaxValue,
}

impl Default for IbisDiffPinEntry {
    fn default() -> Self {
        IbisDiffPinEntry { pin_a: String::new(), pin_b: String::new(), vdiff: 0.2, tdelay: TypMinMaxValue::default() }
    }
}

/// `IbisComponentSeriesPinMapping`: a row of `[Series Pin Mapping]` (IBIS 4.1+).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisComponentSeriesPinMapping {
    pub pin1: String,
    pub pin2: String,
    pub model_name: String,
    pub function_table_group: String,
}

/// KiCad's `IbisComponent` as the parser fills it (the `[Component]` section; [`IbisComponent`] is what the dialog lists of it).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisParsedComponent {
    pub name: String,
    pub manufacturer: String,
    pub package: IbisComponentPackage,
    /// `m_pins`, the dummy first row included.
    pub pins: Vec<IbisComponentPin>,
    pub pin_mappings: Vec<IbisComponentPinMapping>,
    pub package_model: String,
    pub bus_label: String,
    pub die_supply_pads: String,
    /// `m_diffPin.m_entries`.
    pub diff_pins: Vec<IbisDiffPinEntry>,
    pub series_pin_mappings: Vec<IbisComponentSeriesPinMapping>,
}

impl IbisParsedComponent {
    /// `IbisComponent::Check`.
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        let message = format!("Checking component {}", self.name);
        let fail = |status: &mut bool, rep: &mut Rep, msg: &str| {
            if *status {
                report(rep, message.clone(), Severity::Action);
            }
            report(rep, msg, Severity::Error);
            *status = false;
        };
        if self.name.is_empty() {
            fail(&mut status, rep, "Component: name cannot be empty.");
        }
        if self.manufacturer.is_empty() {
            fail(&mut status, rep, "Component: manufacturer cannot be empty.");
        }
        if !self.package.check(rep) {
            fail(&mut status, rep, "Component: invalid package.");
        }
        if self.pins.is_empty() {
            fail(&mut status, rep, "Component: no pin");
        }
        for pin in &self.pins {
            status &= pin.check(rep);
        }
        status
    }
}

/// `IbisModelSelectorEntry`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisModelSelectorEntry {
    pub model_name: String,
    /// The rest of the row, as `readString` keeps it (its leading spaces included).
    pub model_description: String,
}

/// `IbisModelSelector`: `[Model Selector]` (`Check` is always true).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisModelSelector {
    pub name: String,
    pub models: Vec<IbisModelSelectorEntry>,
}

/// `IVtableEntry`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IvTableEntry {
    pub v: f64,
    pub i: TypMinMaxValue,
}

impl Default for IvTableEntry {
    fn default() -> Self {
        IvTableEntry { v: 0.0, i: TypMinMaxValue::default() }
    }
}

/// `IVtable`: `[Pulldown]`, `[Pullup]`, `[GND Clamp]`, ...
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IvTable {
    pub entries: Vec<IvTableEntry>,
}

impl IvTable {
    /// `IVtable::Check` (the monotonicity KiCad leaves as a TODO is not checked).
    fn check(&self, rep: &mut Rep) -> bool {
        for entry in &self.entries {
            if entry.v.is_nan() {
                report(rep, "There is an invalid voltage in an IV table", Severity::Error);
                return false;
            }
            if !entry.i.check() {
                report(rep, "There is an invalid current in an IV table", Severity::Error);
                return false;
            }
        }
        true
    }
}

/// `VTtableEntry`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VtTableEntry {
    pub t: f64,
    pub v: TypMinMaxValue,
}

impl Default for VtTableEntry {
    fn default() -> Self {
        VtTableEntry { t: 0.0, v: TypMinMaxValue::default() }
    }
}

/// `VTtable`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VtTable {
    pub entries: Vec<VtTableEntry>,
}

/// `IBIS_MODEL_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisModelType {
    #[default]
    Undefined,
    InputStd,
    Output,
    Io,
    ThreeState,
    OpenDrain,
    IoOpenDrain,
    OpenSink,
    IoOpenSink,
    OpenSource,
    IoOpenSource,
    InputEcl,
    OutputEcl,
    IoEcl,
    ThreeStateEcl,
    Terminator,
    Series,
    SeriesSwitch,
}

/// `IBIS_MODEL_ENABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisModelEnable {
    #[default]
    Undefined,
    ActiveHigh,
    ActiveLow,
}

/// `IBIS_MODEL_POLARITY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisModelPolarity {
    #[default]
    Undefined,
    Inverting,
    NonInverting,
}

/// `dvdt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dvdt {
    pub dv: f64,
    pub dt: f64,
}

impl Default for Dvdt {
    fn default() -> Self {
        Dvdt { dv: 1.0, dt: 1.0 }
    }
}

/// `dvdtTypMinMax`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DvdtTypMinMax {
    pub value: [Dvdt; 3],
}

impl DvdtTypMinMax {
    /// `dvdtTypMinMax::Check`: the typ value is a number (not even `NA`), min is not an invalid NaN. (KiCad tests the MIN corner twice and
    /// never MAX; kept.)
    pub fn check(&self) -> bool {
        let invalid = |v: f64| v.is_nan() && !is_number_na(v);
        let mut status = true;
        if self.value[TYP].dv.is_nan() || self.value[TYP].dt.is_nan() {
            status = false;
        }
        if invalid(self.value[MIN].dv) || invalid(self.value[MIN].dt) {
            status = false;
        }
        status
    }
}

/// `IbisRamp`: `[Ramp]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IbisRamp {
    pub falling: DvdtTypMinMax,
    pub rising: DvdtTypMinMax,
    /// "The R_load subparameter is optional if the default 50 ohm load is used".
    pub r_load: f64,
}

impl Default for IbisRamp {
    fn default() -> Self {
        IbisRamp { falling: DvdtTypMinMax::default(), rising: DvdtTypMinMax::default(), r_load: 50.0 }
    }
}

impl IbisRamp {
    /// `IbisRamp::Check`.
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        if self.r_load.is_nan() {
            status = false;
            report(rep, "Invalid R_load.", Severity::Error);
        }
        if !self.falling.check() {
            report(rep, "Invalid falling dV/dt.", Severity::Error);
            status = false;
        }
        if !self.rising.check() {
            report(rep, "Invalid rising dV/dt.", Severity::Error);
            status = false;
        }
        status
    }
}

/// `IBIS_WAVEFORM_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisWaveformType {
    #[default]
    Rising,
    Falling,
}

/// `IbisWaveform`: `[Rising Waveform]` / `[Falling Waveform]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisWaveform {
    pub table: VtTable,
    pub kind: IbisWaveformType,
    pub r_dut: f64,
    pub c_dut: f64,
    pub l_dut: f64,
    pub r_fixture: f64,
    pub c_fixture: f64,
    pub l_fixture: f64,
    pub v_fixture: f64,
    pub v_fixture_min: f64,
    pub v_fixture_max: f64,
}

/// `IBIS_SUBMODEL_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubmodelMode {
    #[default]
    All,
    Driving,
    NonDriving,
}

/// `IbisSubmodelMode`: a row of `[Add Submodel]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisSubmodelMode {
    pub name: String,
    pub mode: SubmodelMode,
}

/// `IbisModel`: `[Model]`.
#[derive(Debug, Clone, PartialEq)]
pub struct IbisModel {
    pub name: String,
    pub kind: IbisModelType,
    pub vinl: f64,
    pub vinh: f64,
    pub vref: f64,
    pub rref: f64,
    pub cref: f64,
    pub vmeas: f64,
    pub enable: IbisModelEnable,
    pub polarity: IbisModelPolarity,
    pub c_comp: TypMinMaxValue,
    pub c_comp_gnd_clamp: TypMinMaxValue,
    pub c_comp_power_clamp: TypMinMaxValue,
    pub c_comp_pullup: TypMinMaxValue,
    pub c_comp_pulldown: TypMinMaxValue,
    pub voltage_range: TypMinMaxValue,
    pub temperature_range: TypMinMaxValue,
    pub pullup_reference: TypMinMaxValue,
    pub pulldown_reference: TypMinMaxValue,
    pub gnd_clamp_reference: TypMinMaxValue,
    pub power_clamp_reference: TypMinMaxValue,
    pub rgnd: TypMinMaxValue,
    pub rpower: TypMinMaxValue,
    pub rac: TypMinMaxValue,
    pub cac: TypMinMaxValue,
    pub gnd_clamp: IvTable,
    pub power_clamp: IvTable,
    pub pullup: IvTable,
    pub pulldown: IvTable,
    pub isso_pu: IvTable,
    pub isso_pd: IvTable,
    pub composite_current: IvTable,
    pub rising_waveforms: Vec<IbisWaveform>,
    pub falling_waveforms: Vec<IbisWaveform>,
    pub ramp: IbisRamp,
    pub submodels: Vec<IbisSubmodelMode>,
}

impl Default for IbisModel {
    /// The class's defaults: "the default values of Vinl = 0.8 V and Vinh = 2.0 V are assumed".
    fn default() -> Self {
        IbisModel {
            name: String::new(),
            kind: IbisModelType::Undefined,
            vinl: 0.8,
            vinh: 2.0,
            vref: 0.0,
            rref: 0.0,
            cref: 0.0,
            vmeas: 0.0,
            enable: IbisModelEnable::Undefined,
            polarity: IbisModelPolarity::Undefined,
            c_comp: TypMinMaxValue::default(),
            c_comp_gnd_clamp: TypMinMaxValue::default(),
            c_comp_power_clamp: TypMinMaxValue::default(),
            c_comp_pullup: TypMinMaxValue::default(),
            c_comp_pulldown: TypMinMaxValue::default(),
            voltage_range: TypMinMaxValue::default(),
            temperature_range: TypMinMaxValue::default(),
            pullup_reference: TypMinMaxValue::default(),
            pulldown_reference: TypMinMaxValue::default(),
            gnd_clamp_reference: TypMinMaxValue::default(),
            power_clamp_reference: TypMinMaxValue::default(),
            rgnd: TypMinMaxValue::default(),
            rpower: TypMinMaxValue::default(),
            rac: TypMinMaxValue::default(),
            cac: TypMinMaxValue::default(),
            gnd_clamp: IvTable::default(),
            power_clamp: IvTable::default(),
            pullup: IvTable::default(),
            pulldown: IvTable::default(),
            isso_pu: IvTable::default(),
            isso_pd: IvTable::default(),
            composite_current: IvTable::default(),
            rising_waveforms: Vec::new(),
            falling_waveforms: Vec::new(),
            ramp: IbisRamp::default(),
            submodels: Vec::new(),
        }
    }
}

impl IbisModel {
    /// `IbisModel::Check`. An invalid `[Ramp]` is reported but leaves the status as it is (as in KiCad).
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        if self.name.is_empty() {
            report(rep, "Model name cannot be empty", Severity::Error);
            status = false;
        }
        let message = format!("Checking model {}", self.name);
        let invalid = |v: f64| v.is_nan() && !is_number_na(v);
        let fail = |status: &mut bool, rep: &mut Rep, msg: &str| {
            if *status {
                report(rep, message.clone(), Severity::Action);
            }
            report(rep, msg, Severity::Error);
            *status = false;
        };
        if self.kind == IbisModelType::Undefined {
            fail(&mut status, rep, "Undefined model type.");
        }
        for (v, msg) in [
            (self.vinh, "Invalid Vinh value."),
            (self.vinl, "Invalid Vinl value."),
            (self.rref, "Invalid R_ref value."),
            (self.cref, "Invalid C_ref value."),
            (self.vref, "Invalid V_ref value."),
            (self.vmeas, "Invalid V_meas value."),
        ] {
            if invalid(v) {
                fail(&mut status, rep, msg);
            }
        }
        for (v, msg) in [
            (&self.c_comp, "Missing or invalid C_comp value."),
            (&self.c_comp_gnd_clamp, "Missing or invalid C_comp_gnd_clamp value."),
            (&self.c_comp_power_clamp, "Missing or invalid C_comp_power_clamp value."),
            (&self.c_comp_pullup, "Missing or invalid C_comp_pullup value."),
            (&self.c_comp_pulldown, "Missing or invalid C_comp_pulldown value."),
        ] {
            if !v.check() {
                fail(&mut status, rep, msg);
            }
        }
        if self.c_comp.is_na()
            && self.c_comp_gnd_clamp.is_na()
            && self.c_comp_power_clamp.is_na()
            && self.c_comp_pullup.is_na()
            && self.c_comp_pulldown.is_na()
        {
            fail(&mut status, rep, "No C_comp values are available.");
        }
        if !self.temperature_range.check() || self.temperature_range.is_na() {
            fail(&mut status, rep, "Missing or invalid Temperature Range value.");
        }
        for (v, msg) in [
            (&self.voltage_range, "Missing or invalid Voltage Range value."),
            (&self.pullup_reference, "Missing or invalid Pullup Reference value."),
            (&self.pulldown_reference, "Missing or invalid Pulldown Reference value."),
            (&self.gnd_clamp_reference, "Missing or invalid GND Clamp Reference value."),
            (&self.power_clamp_reference, "Missing or invalid POWER Clamp Reference value."),
        ] {
            if !v.check() {
                fail(&mut status, rep, msg);
            }
        }
        if self.voltage_range.is_na()
            && (self.pullup_reference.is_na()
                || self.pulldown_reference.is_na()
                || self.gnd_clamp_reference.is_na()
                || self.power_clamp_reference.is_na())
        {
            fail(&mut status, rep, "Voltage Range or Reference is missing.");
        }
        for (t, msg) in [
            (&self.pulldown, "Invalid Pulldown table."),
            (&self.pullup, "Invalid Pullup table."),
            (&self.isso_pd, "Invalid ISSO_PD table."),
            (&self.isso_pu, "Invalid ISSO_PU table."),
            (&self.composite_current, "Invalid Composite Current table."),
            (&self.power_clamp, "Invalid POWER Clamp table."),
            (&self.gnd_clamp, "Invalid GND Clamp table."),
        ] {
            if !t.check(rep) {
                report(rep, msg, Severity::Error);
                status = false;
            }
        }
        let no_ramp = matches!(
            self.kind,
            IbisModelType::InputEcl | IbisModelType::InputStd | IbisModelType::Terminator | IbisModelType::Series | IbisModelType::SeriesSwitch
        );
        if !no_ramp && !self.ramp.check(rep) {
            report(rep, "Invalid Ramp specification.", Severity::Error);
        }
        status
    }
}

/// `IBIS_SUBMODEL_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IbisSubmodelType {
    #[default]
    Undefined,
    DynamicClamp,
    BusHold,
    FallBack,
}

/// `IbisSubmodel`: `[Submodel]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisSubmodel {
    pub name: String,
    pub kind: IbisSubmodelType,
    pub v_trigger_r: TypMinMaxValue,
    pub v_trigger_f: TypMinMaxValue,
    pub off_delay: TypMinMaxValue,
    pub pullup: IvTable,
    pub pulldown: IvTable,
    pub gnd_clamp: IvTable,
    pub power_clamp: IvTable,
    pub gnd_pulse: IvTable,
    pub power_pulse: IvTable,
    pub ramp: IbisRamp,
    pub rising_waveforms: Vec<IbisWaveform>,
    pub falling_waveforms: Vec<IbisWaveform>,
}

impl IbisSubmodel {
    /// `IbisSubmodel::Check`: only a static dynamic clamp is supported. (A submodel with no type is "The submodel has no type." and then
    /// fails without another message: KiCad's `default:` branch is a `wxLogMessage`.)
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        if self.name.is_empty() {
            report(rep, "Submodel name cannot be empty.", Severity::Error);
            status = false;
        }
        let message = format!("Checking submodel {}", self.name);
        let fail = |status: &mut bool, rep: &mut Rep, msg: &str| {
            if *status {
                report(rep, message.clone(), Severity::Action);
            }
            report(rep, msg, Severity::Error);
            *status = false;
        };
        if self.kind == IbisSubmodelType::Undefined {
            fail(&mut status, rep, "The submodel has no type.");
        }
        for (v, msg) in [
            (&self.v_trigger_r, "Missing or invalid V_trigger_r value."),
            (&self.v_trigger_f, "Missing or invalid V_trigger_f value."),
            (&self.off_delay, "Missing or invalid Off_delay value."),
        ] {
            if !v.check() {
                fail(&mut status, rep, msg);
            }
        }
        for (t, msg) in [
            (&self.pullup, "Invalid Pullup table."),
            (&self.pulldown, "Invalid Pulldown table."),
            (&self.gnd_clamp, "Invalid GND Clamp table."),
            (&self.power_clamp, "Invalid POWER Clamp table."),
            (&self.gnd_pulse, "Invalid GND Pulse table."),
            (&self.power_pulse, "Invalid POWER Pulse table."),
        ] {
            if !t.check(rep) {
                report(rep, msg, Severity::Error);
                status = false;
            }
        }
        if !self.ramp.check(rep) {
            report(rep, "Invalid Ramp specification.", Severity::Error);
            status = false;
        }
        if self.kind != IbisSubmodelType::DynamicClamp {
            let what = match self.kind {
                IbisSubmodelType::BusHold => "Bus Hold",
                IbisSubmodelType::FallBack => "Fall Back",
                _ => return false,
            };
            report(rep, format!("{what} submodels are not yet supported."), Severity::Error);
            status = false;
        } else if !self.v_trigger_r.is_na() || !self.v_trigger_f.is_na() {
            report(rep, "Triggered mode dynamic clamp submodels are not yet supported.", Severity::Error);
            status = false;
        }
        status
    }
}

/// `IbisPackageModel`: `[Define Package Model]`. Its three matrices always exist (`changeContext` makes them), so KiCad's "... matrix is
/// nullptr." cannot happen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisPackageModel {
    pub name: String,
    pub manufacturer: String,
    pub oem: String,
    pub description: String,
    pub number_of_pins: i32,
    /// `[Pin Numbers]`: each pin's index.
    pub pins: BTreeMap<String, i32>,
    pub resistance_matrix: IbisMatrix,
    pub capacitance_matrix: IbisMatrix,
    pub inductance_matrix: IbisMatrix,
}

impl IbisPackageModel {
    /// `IbisPackageModel::Check` (the resistance matrix too, though KiCad's comment says it is not required).
    fn check(&self, rep: &mut Rep) -> bool {
        let mut status = true;
        if self.name.is_empty() {
            report(rep, "Package model name cannot be empty.", Severity::Error);
            status = false;
        }
        let message = format!("Checking package model {}", self.name);
        let fail = |status: &mut bool, rep: &mut Rep, msg: &str| {
            if *status {
                report(rep, message.clone(), Severity::Action);
            }
            report(rep, msg, Severity::Error);
            *status = false;
        };
        if self.manufacturer.is_empty() {
            fail(&mut status, rep, "Manufacturer cannot be empty.");
        }
        if self.oem.is_empty() {
            fail(&mut status, rep, "OEM cannot be empty.");
        }
        if self.number_of_pins < 0 {
            fail(&mut status, rep, "Negative number of pins.");
        }
        if self.pins.len() as i32 != self.number_of_pins {
            fail(&mut status, rep, "Number of pins does not match [Pin Numbers] size.");
        }
        if !self.resistance_matrix.check(rep) {
            fail(&mut status, rep, "Resistance matrix is incorrect.");
        }
        if self.capacitance_matrix.kind == IbisMatrixType::Undefined {
            fail(&mut status, rep, "Capacitance matrix is undefined.");
        }
        if !self.capacitance_matrix.check(rep) {
            fail(&mut status, rep, "Capacitance matrix is incorrect.");
        }
        if self.inductance_matrix.kind == IbisMatrixType::Undefined {
            fail(&mut status, rep, "Inductance matrix is undefined.");
        }
        if !self.inductance_matrix.check(rep) {
            fail(&mut status, rep, "Inductance matrix is incorrect.");
        }
        status
    }
}

/// One `[Component]` as the dialog lists it (`KIBIS_COMPONENT`, and `SIM_LIBRARY_IBIS::InitModel`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisComponent {
    pub name: String,
    /// `m_ibisPins`: (pin number, signal name), in the file's order, the dummy first row left out.
    pub pins: Vec<(String, String)>,
    /// Each pin's models (`KIBIS_PIN::m_models`' names), in the order of `pins`.
    pub pin_models: Vec<Vec<String>>,
    /// The pins a `[Diff Pin]` row pairs (`SIM_LIBRARY_IBIS::m_diffPins`), in the order of `pins`.
    pub diff_pins: Vec<String>,
}

/// What KiCad makes of one file: the dialog's lists, the reporter's messages, `KIBIS::m_valid`, and the parser's `IbisFile`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IbisFile {
    /// `KIBIS::m_components` (none when the parser threw).
    pub components: Vec<IbisComponent>,
    /// `[Model]` names (`KIBIS::m_models`).
    pub models: Vec<String>,
    /// What was reported, in order.
    pub messages: Vec<(Severity, String)>,
    /// `KIBIS::m_valid`.
    pub valid: bool,
    /// The `what()` of the `std::out_of_range` the C++ throws, when it does (see the module's doc).
    pub thrown: Option<String>,
    pub header: IbisHeader,
    /// `IbisFile::m_components`.
    pub ibis_components: Vec<IbisParsedComponent>,
    pub model_selectors: Vec<IbisModelSelector>,
    /// `IbisFile::m_models`.
    pub ibis_models: Vec<IbisModel>,
    pub package_models: Vec<IbisPackageModel>,
    pub submodels: BTreeMap<String, IbisSubmodel>,
}

/// `IBIS_PARSER_CONTINUE` (`VT_TABLE`, which nothing sets, and `COMPONENT_DIESUPPLYPADS`, which nothing reads, left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cont {
    None,
    String,
    ComponentPackage,
    ComponentPinMapping,
    ComponentDiffPin,
    ComponentSeriesPinMapping,
    ComponentPin,
    Matrix,
    ModelSelector,
    Model,
    ModelSpec,
    Submodel,
    SubmodelSpec,
    RxThresholds,
    AlgorithmicModel,
    AddSubmodel,
    IvTable,
    Ramp,
    Waveform,
    PackageModelPins,
}

/// `IBIS_PARSER_CONTEXT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Header,
    Component,
    ModelSelector,
    Model,
    Submodel,
    PackageModel,
    PackageModelModelData,
    AlgorithmicModel,
    End,
}

/// What `m_continuingString` points at.
#[derive(Debug, Clone, Copy)]
enum StrRef {
    FileName,
    Source,
    Notes,
    Disclaimer,
    Copyright,
    Date,
    ComponentName(usize),
    ComponentManufacturer(usize),
    ComponentPackageModel(usize),
    PackageModelManufacturer(usize),
    PackageModelOem(usize),
    PackageModelDescription(usize),
}

/// The owner of a table: `m_currentModel` or `m_currentSubmodel`.
#[derive(Debug, Clone)]
enum Owner {
    Model(usize),
    Submodel(String),
}

/// What `m_currentIVtable` points at.
#[derive(Debug, Clone)]
enum IvRef {
    GndClamp(Owner),
    PowerClamp(Owner),
    Pullup(Owner),
    Pulldown(Owner),
    IssoPd(usize),
    IssoPu(usize),
    CompositeCurrent(usize),
    GndPulse(String),
    PowerPulse(String),
}

/// What `m_currentWaveform` points at.
#[derive(Debug, Clone)]
struct WfRef {
    owner: Owner,
    rising: bool,
    index: usize,
}

/// Which matrix of `m_currentPackageModel` `m_currentMatrix` is.
#[derive(Debug, Clone, Copy)]
enum MatRef {
    Resistance(usize),
    Capacitance(usize),
    Inductance(usize),
}

/// KiCad's `IbisFile`: what the parser fills.
#[derive(Default)]
struct Data {
    header: IbisHeader,
    components: Vec<IbisParsedComponent>,
    model_selectors: Vec<IbisModelSelector>,
    models: Vec<IbisModel>,
    package_models: Vec<IbisPackageModel>,
    submodels: BTreeMap<String, IbisSubmodel>,
}

impl Data {
    fn string(&mut self, r: StrRef) -> &mut String {
        match r {
            StrRef::FileName => &mut self.header.file_name,
            StrRef::Source => &mut self.header.source,
            StrRef::Notes => &mut self.header.notes,
            StrRef::Disclaimer => &mut self.header.disclaimer,
            StrRef::Copyright => &mut self.header.copyright,
            StrRef::Date => &mut self.header.date,
            StrRef::ComponentName(i) => &mut self.components[i].name,
            StrRef::ComponentManufacturer(i) => &mut self.components[i].manufacturer,
            StrRef::ComponentPackageModel(i) => &mut self.components[i].package_model,
            StrRef::PackageModelManufacturer(i) => &mut self.package_models[i].manufacturer,
            StrRef::PackageModelOem(i) => &mut self.package_models[i].oem,
            StrRef::PackageModelDescription(i) => &mut self.package_models[i].description,
        }
    }

    fn submodel(&mut self, name: &str) -> &mut IbisSubmodel {
        self.submodels.get_mut(name).expect("the current submodel is in the map")
    }

    fn iv(&mut self, r: &IvRef) -> &mut IvTable {
        match r {
            IvRef::GndClamp(Owner::Model(i)) => &mut self.models[*i].gnd_clamp,
            IvRef::GndClamp(Owner::Submodel(n)) => &mut self.submodel(n).gnd_clamp,
            IvRef::PowerClamp(Owner::Model(i)) => &mut self.models[*i].power_clamp,
            IvRef::PowerClamp(Owner::Submodel(n)) => &mut self.submodel(n).power_clamp,
            IvRef::Pullup(Owner::Model(i)) => &mut self.models[*i].pullup,
            IvRef::Pullup(Owner::Submodel(n)) => &mut self.submodel(n).pullup,
            IvRef::Pulldown(Owner::Model(i)) => &mut self.models[*i].pulldown,
            IvRef::Pulldown(Owner::Submodel(n)) => &mut self.submodel(n).pulldown,
            IvRef::IssoPd(i) => &mut self.models[*i].isso_pd,
            IvRef::IssoPu(i) => &mut self.models[*i].isso_pu,
            IvRef::CompositeCurrent(i) => &mut self.models[*i].composite_current,
            IvRef::GndPulse(n) => &mut self.submodel(n).gnd_pulse,
            IvRef::PowerPulse(n) => &mut self.submodel(n).power_pulse,
        }
    }

    fn waveforms(&mut self, owner: &Owner, rising: bool) -> &mut Vec<IbisWaveform> {
        match (owner, rising) {
            (Owner::Model(i), true) => &mut self.models[*i].rising_waveforms,
            (Owner::Model(i), false) => &mut self.models[*i].falling_waveforms,
            (Owner::Submodel(n), true) => &mut self.submodel(n).rising_waveforms,
            (Owner::Submodel(n), false) => &mut self.submodel(n).falling_waveforms,
        }
    }

    fn waveform(&mut self, r: &WfRef) -> &mut IbisWaveform {
        &mut self.waveforms(&r.owner, r.rising)[r.index]
    }

    fn matrix(&mut self, r: MatRef) -> &mut IbisMatrix {
        match r {
            MatRef::Resistance(i) => &mut self.package_models[i].resistance_matrix,
            MatRef::Capacitance(i) => &mut self.package_models[i].capacitance_matrix,
            MatRef::Inductance(i) => &mut self.package_models[i].inductance_matrix,
        }
    }
}

/// `IBIS_MAX_LINE_LENGTH`: "official limit is 1024".
const IBIS_MAX_LINE_LENGTH: usize = 2048;

/// The line state of `IbisParser` (`m_buffer`, `m_lineOffset`, `m_lineIndex`, `m_lineLength`, `m_commentChar`) and its reporter, with the
/// readers that only need those.
struct Lexer {
    /// `m_buffer`: the file and a NUL.
    buf: Vec<u8>,
    line_counter: i64,
    comment_char: u8,
    buffer_index: usize,
    line_offset: usize,
    line_index: usize,
    line_length: usize,
    rep: Rep,
}

impl Lexer {
    /// `m_buffer[m_lineOffset + aIndex]` (a NUL past the end, where the C++ would read beyond the buffer).
    fn ch(&self, index: usize) -> u8 {
        self.buf.get(self.line_offset + index).copied().unwrap_or(0)
    }

    fn report(&mut self, msg: impl Into<String>, severity: Severity) {
        report(&mut self.rep, msg, severity);
    }

    /// `IbisParser::getNextLine`.
    fn get_next_line(&mut self) -> bool {
        self.line_counter += 1;
        let tmp_index = self.buffer_index;
        self.line_offset = self.buffer_index;
        if self.buffer_index >= self.buf.len() {
            return false;
        }
        let buf = &self.buf;
        let at = |i: usize| buf.get(i).copied().unwrap_or(0);
        let mut bi = self.buffer_index;
        let mut c = at(bi);
        bi += 1;
        let mut i = 1;
        while c != self.comment_char && c != 0 && c != b'\n' && i < IBIS_MAX_LINE_LENGTH {
            c = at(bi);
            bi += 1;
            i += 1;
        }
        if i == IBIS_MAX_LINE_LENGTH {
            self.buffer_index = bi;
            self.report("Line exceeds maximum length.", Severity::Error);
            return false;
        }
        self.line_length = bi - tmp_index - 1;
        self.line_index = 0;
        if c == self.comment_char {
            while c != 0 && c != b'\n' {
                c = at(bi);
                bi += 1;
            }
        }
        self.buffer_index = bi;
        true
    }

    /// `IbisParser::skipWhitespaces`.
    fn skip_whitespaces(&mut self) {
        while is_space(self.ch(self.line_index)) && self.line_index < self.line_length {
            self.line_index += 1;
        }
    }

    /// `IbisParser::checkEndofLine`.
    fn check_end_of_line(&mut self) -> bool {
        self.skip_whitespaces();
        if self.line_index < self.line_length {
            self.report("A line did not end properly.", Severity::Error);
            return false;
        }
        true
    }

    /// `IbisParser::isLineEmptyFromCursor`.
    fn is_line_empty_from_cursor(&self) -> bool {
        let mut cursor = self.line_index;
        while is_space(self.ch(cursor)) && cursor < self.line_length {
            cursor += 1;
        }
        cursor >= self.line_length
    }

    /// `IbisParser::getKeyword`: the `[...]` that starts the line, its spaces made underscores; "" when the line does not start with `[`.
    fn get_keyword(&mut self) -> String {
        if self.ch(self.line_index) != b'[' {
            return String::new();
        }
        self.line_index += 1;
        let mut keyword = Vec::new();
        let mut c = self.ch(self.line_index);
        self.line_index += 1;
        while c != b']' && self.line_index < self.line_length {
            keyword.push(if c == b' ' { b'_' } else { c });
            c = self.ch(self.line_index);
            self.line_index += 1;
        }
        String::from_utf8_lossy(&keyword).into_owned()
    }

    /// `IbisParser::changeCommentChar`: `[Comment Char] #_char`. It reads past the line's end, which may stop at the old comment character
    /// (an invalid character is reported, and still taken).
    fn change_comment_char(&mut self) -> bool {
        self.skip_whitespaces();
        let mut c = self.ch(self.line_index);
        self.line_index += 1;
        let d = c;
        if !b"!\"#$%&'()*,:;<>?@\\^`{|}~".contains(&c) {
            self.report("New comment character is invalid.", Severity::Error);
        }
        c = self.ch(self.line_index);
        self.line_index += 1;
        let mut str_char = Vec::new();
        while !is_space(c) && c != 0 && c != b'\n' {
            str_char.push(c);
            c = self.ch(self.line_index);
            self.line_index += 1;
        }
        if str_char != b"_char" {
            self.report("Invalid syntax. Should be |_char or &_char, etc...", Severity::Error);
            return false;
        }
        while is_space(c) && c != 0 && c != b'\n' && c != d {
            c = self.ch(self.line_index);
            self.line_index += 1;
        }
        if !is_space(c) && c != d {
            self.report("No extra argument was expected", Severity::Error);
            return false;
        }
        self.comment_char = d;
        true
    }

    /// `IbisParser::readWord`: "" when there is none.
    fn read_word(&mut self) -> String {
        self.skip_whitespaces();
        let start = self.line_index;
        while !is_space(self.ch(self.line_index)) && self.line_index < self.line_length {
            self.line_index += 1;
        }
        let a = (self.line_offset + start).min(self.buf.len());
        let b = (self.line_offset + self.line_index).min(self.buf.len()).max(a);
        String::from_utf8_lossy(&self.buf[a..b]).into_owned()
    }

    /// `IbisParser::readString`: the rest of the line added to `dest`, the trailing spaces of the whole removed (KiCad's loop, which keeps
    /// one character of a string that is all spaces).
    fn read_string(&mut self, dest: &mut String) -> bool {
        let mut d = std::mem::take(dest).into_bytes();
        while self.line_index < self.line_length {
            d.push(self.ch(self.line_index));
            self.line_index += 1;
        }
        let len = d.len();
        if len >= 1 {
            let mut c = d[len - 1];
            let mut i = 0;
            while is_space(c) && i < len {
                c = d[len - 1 - i];
                i += 1;
            }
            d.truncate(len - i + 1);
        }
        *dest = String::from_utf8_lossy(&d).into_owned();
        true
    }

    /// `IbisParser::readTableLine`: the words of the rest of the line.
    fn read_table_line(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while self.line_index < self.line_length {
            let mut s = Vec::new();
            while !is_space(self.ch(self.line_index)) && self.line_index < self.line_length {
                s.push(self.ch(self.line_index));
                self.line_index += 1;
            }
            if !s.is_empty() {
                out.push(String::from_utf8_lossy(&s).into_owned());
            }
            while is_space(self.ch(self.line_index)) && self.line_index < self.line_length {
                self.line_index += 1;
            }
        }
        out
    }

    /// `IbisParser::parseDouble`: `NA`, else `std::stod` (an invalid NaN and false when it throws) and a scale letter right after the number
    /// (any other letter ignored: "In some files, "vinh = 3.0V""). `aAllowModifiers` is not looked at by KiCad, nor here.
    fn parse_double(&mut self, dest: &mut f64, s: &str) -> bool {
        self.skip_whitespaces();
        let mut status = true;
        let mut result;
        if s == "NA" {
            result = na();
        } else {
            match stod(s.as_bytes()) {
                Some((v, size)) => {
                    result = v;
                    if size < s.len() {
                        match s.as_bytes()[size] {
                            b'T' => result *= 1e12,
                            b'G' => result *= 1e9,
                            b'M' => result *= 1e6,
                            b'k' => result *= 1e3,
                            b'm' => result *= 1e-3,
                            b'u' => result *= 1e-6,
                            b'n' => result *= 1e-9,
                            b'p' => result *= 1e-12,
                            b'f' => result *= 1e-15,
                            _ => {}
                        }
                    }
                }
                None => {
                    result = nan_invalid();
                    status = false;
                }
            }
        }
        *dest = result;
        status
    }

    /// `IbisParser::parseDvdt`: `NA`, or `dv/dt`. Its `aString.at( i )` throws when there is no `/`.
    fn parse_dvdt(&mut self, dest: &mut Dvdt, s: &str) -> R {
        if s == "NA" {
            dest.dv = na();
            dest.dt = na();
            return Ok(true);
        }
        let b = s.as_bytes();
        let mut i = 1;
        while i < b.len() {
            if b[i] == b'/' {
                break;
            }
            i += 1;
        }
        if i >= b.len() {
            return Err(Thrown(format!("basic_string::at: __n (which is {i}) >= this->size() (which is {})", b.len())));
        }
        let s1 = String::from_utf8_lossy(&b[..i]).into_owned();
        let s2 = String::from_utf8_lossy(&b[i + 1..]).into_owned();
        Ok(self.parse_double(&mut dest.dv, &s1) && self.parse_double(&mut dest.dt, &s2))
    }

    /// `IbisParser::readDvdt`.
    fn read_dvdt(&mut self, dest: &mut Dvdt) -> R {
        let s = self.read_word();
        if s.is_empty() {
            self.report("A dV/dt value is missing.", Severity::Warning);
            return Ok(false);
        }
        if !self.parse_dvdt(dest, &s)? {
            self.report(format!("{s} is not a valid dV/dt value."), Severity::Warning);
            return Ok(false);
        }
        Ok(true)
    }

    /// `IbisParser::readRampdvdt`.
    fn read_ramp_dvdt(&mut self, dest: &mut DvdtTypMinMax) -> R {
        let mut status = true;
        for corner in [TYP, MIN, MAX] {
            status &= self.read_dvdt(&mut dest.value[corner])?;
        }
        Ok(status)
    }

    /// `IbisParser::readDouble`.
    fn read_double(&mut self, dest: &mut f64) -> bool {
        let s = self.read_word();
        if s.is_empty() {
            self.report("Failed to read a word.", Severity::Warning);
            return false;
        }
        if !self.parse_double(dest, &s) {
            self.report("Failed to read a double.", Severity::Warning);
            return false;
        }
        true
    }

    /// `IbisParser::readInt` (`std::stoi`; a NaN made an `int` is `INT_MIN` on x86).
    fn read_int(&mut self, dest: &mut i32) -> bool {
        let s = self.read_word();
        if s.is_empty() {
            self.report("Failed to read a word.", Severity::Warning);
            return false;
        }
        let mut status = true;
        let (result, size) = match stoi(s.as_bytes()) {
            Some(v) => v,
            None => {
                if s != "NA" {
                    status = false;
                }
                (i32::MIN, 0)
            }
        };
        if size != s.len() {
            status = false;
            self.report("Number is not an integer", Severity::Warning);
        }
        *dest = result;
        status
    }

    /// `IbisParser::readNumericSubparam`: `name = value` at the start of the line.
    fn read_numeric_subparam(&mut self, name: &str, dest: &mut f64) -> bool {
        if name.len() >= self.line_length {
            return false;
        }
        let old_index = self.line_index;
        self.line_index = 0;
        let mut param = Vec::new();
        for _ in 0..name.len() {
            param.push(self.ch(self.line_index));
            self.line_index += 1;
        }
        if param != name.as_bytes() {
            self.line_index = old_index;
            return false;
        }
        self.skip_whitespaces();
        let mut status = self.ch(self.line_index) == b'=';
        self.line_index += 1;
        if status {
            self.skip_whitespaces();
            status &= self.read_double(dest);
        }
        if !status {
            self.line_index = old_index;
        }
        status
    }

    /// `IbisParser::readTypMinMaxValue`: typ, then min and max if there (each one missing is a warning, "Failed to read a word.").
    fn read_typ_min_max_value(&mut self, dest: &mut TypMinMaxValue) -> bool {
        self.skip_whitespaces();
        if !self.read_double(&mut dest.value[TYP]) {
            self.report("Typ-Min-Max Values requires at least Typ.", Severity::Error);
            return false;
        }
        self.read_double(&mut dest.value[MIN]);
        self.read_double(&mut dest.value[MAX]);
        true
    }

    /// `IbisParser::readTypMinMaxValueSubparam`: `name typ min max` at the start of the line, a space right after the name. Its value read,
    /// it is true even when the value was not.
    fn read_typ_min_max_value_subparam(&mut self, name: &str, dest: &mut TypMinMaxValue) -> bool {
        self.line_index = 0;
        if name.len() >= self.line_length {
            return false;
        }
        let mut param = Vec::new();
        for _ in 0..name.len() {
            param.push(self.ch(self.line_index));
            self.line_index += 1;
        }
        if self.ch(self.line_index) == b' ' && param == name.as_bytes() {
            self.read_typ_min_max_value(dest);
            true
        } else {
            false
        }
    }
}

/// `std::stod` in the C locale: the value and how many characters it took, or `None` where it throws (no number, or out of range).
fn stod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let sign = |v: f64| if neg { -v } else { v };
    let rest = &s[i..];
    let starts = |w: &[u8]| rest.len() >= w.len() && rest[..w.len()].eq_ignore_ascii_case(w);
    if starts(b"infinity") {
        return Some((sign(f64::INFINITY), i + 8));
    }
    if starts(b"inf") {
        return Some((sign(f64::INFINITY), i + 3));
    }
    if starts(b"nan") {
        let mut j = i + 3;
        if s.get(j) == Some(&b'(') {
            let mut k = j + 1;
            while k < s.len() && (s[k].is_ascii_alphanumeric() || s[k] == b'_') {
                k += 1;
            }
            if s.get(k) == Some(&b')') {
                j = k + 1;
            }
        }
        return Some((sign(nan_invalid()), j));
    }
    if rest.len() > 2 && rest[0] == b'0' && (rest[1] | 0x20) == b'x' {
        // a hexadecimal float, `0x1.8p3`
        let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as f64;
        let mut j = i + 2;
        let mut mant = 0.0f64;
        let mut digits = 0;
        let mut exp: i64 = 0;
        while j < s.len() && s[j].is_ascii_hexdigit() {
            mant = mant * 16.0 + hex(s[j]);
            digits += 1;
            j += 1;
        }
        if j < s.len() && s[j] == b'.' {
            let mut k = j + 1;
            while k < s.len() && s[k].is_ascii_hexdigit() {
                mant = mant * 16.0 + hex(s[k]);
                exp -= 4;
                digits += 1;
                k += 1;
            }
            if digits > 0 {
                j = k;
            }
        }
        if digits == 0 {
            // "0x" with no digit: the number is the "0"
            return Some((sign(0.0), i + 1));
        }
        if j < s.len() && (s[j] | 0x20) == b'p' {
            let mut k = j + 1;
            let mut eneg = false;
            if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
                eneg = s[k] == b'-';
                k += 1;
            }
            if k < s.len() && s[k].is_ascii_digit() {
                let mut e: i64 = 0;
                while k < s.len() && s[k].is_ascii_digit() {
                    e = (e * 10 + (s[k] - b'0') as i64).min(100_000);
                    k += 1;
                }
                exp += if eneg { -e } else { e };
                j = k;
            }
        }
        let v = mant * 2f64.powi(exp.clamp(-100_000, 100_000) as i32);
        if v.is_infinite() || (mant != 0.0 && v.abs() < f64::MIN_POSITIVE) {
            return None;
        }
        return Some((sign(v), j));
    }
    let start = i;
    let mut j = i;
    let mut digits = 0;
    let mut nonzero = false;
    while j < s.len() && s[j].is_ascii_digit() {
        nonzero |= s[j] != b'0';
        digits += 1;
        j += 1;
    }
    if j < s.len() && s[j] == b'.' {
        let mut k = j + 1;
        while k < s.len() && s[k].is_ascii_digit() {
            nonzero |= s[k] != b'0';
            digits += 1;
            k += 1;
        }
        if digits > 0 {
            j = k;
        }
    }
    if digits == 0 {
        return None;
    }
    if j < s.len() && (s[j] | 0x20) == b'e' {
        let mut k = j + 1;
        if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
            k += 1;
        }
        if k < s.len() && s[k].is_ascii_digit() {
            while k < s.len() && s[k].is_ascii_digit() {
                k += 1;
            }
            j = k;
        }
    }
    // Rust's parser wants a digit on both sides of the point
    let mut text = String::from("0");
    text.push_str(std::str::from_utf8(&s[start..j]).ok()?);
    let text = text.replace(".e", ".0e").replace(".E", ".0E");
    let text = if text.ends_with('.') { format!("{text}0") } else { text };
    let v: f64 = text.parse().ok()?;
    // `ERANGE`: an overflow, or a number too small for a normal double
    if v.is_infinite() || (nonzero && v.abs() < f64::MIN_POSITIVE) {
        return None;
    }
    Some((sign(v), j))
}

/// `std::stoi` (base 10): the value and how many characters it took, or `None` where it throws.
fn stoi(s: &[u8]) -> Option<(i32, usize)> {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let start = i;
    let mut v: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        v = (v * 10 + (s[i] - b'0') as i64).min(i64::from(u32::MAX) + 1);
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if neg { -v } else { v };
    i32::try_from(v).ok().map(|v| (v, i))
}

/// `IbisParser`: the line state, the file it fills, and its `m_current*` pointers (as indices).
struct Parser {
    lx: Lexer,
    f: Data,
    cur_component: Option<usize>,
    cur_model_selector: Option<usize>,
    cur_model: Option<usize>,
    cur_submodel: Option<String>,
    cur_package_model: Option<usize>,
    cur_matrix: Option<MatRef>,
    cur_matrix_row: i32,
    cur_matrix_col: i32,
    cur_iv: Option<IvRef>,
    cur_waveform: Option<WfRef>,
    continuing: Option<StrRef>,
    cont: Cont,
    ctx: Ctx,
}

impl Parser {
    fn new(text: &str) -> Self {
        let mut buf = text.as_bytes().to_vec();
        buf.push(0);
        Parser {
            lx: Lexer { buf, line_counter: 0, comment_char: b'|', buffer_index: 0, line_offset: 0, line_index: 0, line_length: 0, rep: Vec::new() },
            f: Data::default(),
            cur_component: None,
            cur_model_selector: None,
            cur_model: None,
            cur_submodel: None,
            cur_package_model: None,
            cur_matrix: None,
            cur_matrix_row: 0,
            cur_matrix_col: 0,
            cur_iv: None,
            cur_waveform: None,
            continuing: None,
            cont: Cont::None,
            ctx: Ctx::Header,
        }
    }

    fn cm(&self) -> usize {
        self.cur_model.expect("in a [Model]")
    }

    fn cc(&self) -> usize {
        self.cur_component.expect("in a [Component]")
    }

    fn cpm(&self) -> usize {
        self.cur_package_model.expect("in a [Define Package Model]")
    }

    fn cs(&self) -> String {
        self.cur_submodel.clone().expect("in a [Submodel]")
    }

    /// `IbisParser::ParseFile` (`m_parrot` off, as `KIBIS` sets it).
    fn parse_file(&mut self) -> R {
        let size = self.lx.buf.len();
        self.lx.line_counter = 0;
        self.lx.buffer_index = 0;
        let mut status = true;
        while self.lx.buffer_index < size && status {
            if !self.lx.get_next_line() {
                self.lx.report("Unexpected end of file. Missing [END] ?", Severity::Error);
                status = false;
            }
            if status && !self.on_new_line()? {
                let n = self.lx.line_counter;
                self.lx.report(format!("Error on line {n}"), Severity::Error);
                status = false;
            }
            if self.ctx == Ctx::End {
                break;
            }
        }
        if status {
            'done: for model in &self.f.models {
                for sub in &model.submodels {
                    if !self.f.submodels.contains_key(&sub.name) {
                        report(&mut self.lx.rep, format!("{} submodel not found.", sub.name), Severity::Error);
                        status = false;
                        break 'done;
                    }
                }
            }
        }
        Ok(status)
    }

    /// `IbisParser::onNewLine`.
    fn on_new_line(&mut self) -> R {
        let keyword = self.lx.get_keyword();
        if !keyword.is_empty() {
            self.cont = Cont::None;
            return match self.ctx {
                Ctx::Header => self.parse_header(&keyword),
                Ctx::Component => self.parse_component(&keyword),
                // `parseModelSelector`
                Ctx::ModelSelector => self.change_context(&keyword),
                Ctx::Model => self.parse_model(&keyword),
                Ctx::Submodel => self.parse_submodel(&keyword),
                Ctx::PackageModel => self.parse_package_model(&keyword),
                Ctx::PackageModelModelData => self.parse_package_model_model_data(&keyword),
                Ctx::AlgorithmicModel => self.parse_algorithmic_model(&keyword),
                Ctx::End => {
                    self.lx.report("Internal error: Bad parser context.", Severity::Error);
                    Ok(false)
                }
            };
        }
        self.lx.skip_whitespaces();
        if self.lx.line_index == self.lx.line_length {
            // "That was an empty line"
            return Ok(true);
        }
        // "No new keyword ? Then it is the continuation of the previous one !"
        Ok(match self.cont {
            Cont::String => {
                self.lx.skip_whitespaces();
                let r = self.continuing.expect("a string to continue");
                let s = self.f.string(r);
                s.push('\n');
                self.lx.read_string(s)
            }
            Cont::ComponentPackage => self.read_package(),
            Cont::ComponentPin => self.read_pin()?,
            Cont::ComponentPinMapping => self.read_pin_mapping(),
            Cont::ComponentDiffPin => self.read_diff_pin(),
            Cont::ComponentSeriesPinMapping => self.read_series_pin_mapping(),
            Cont::ModelSelector => self.read_model_selector(),
            Cont::Model => self.read_model(),
            Cont::Submodel => self.read_submodel(),
            Cont::IvTable => {
                let r = self.cur_iv.clone().expect("an IV table");
                self.read_iv_table_entry(r)
            }
            Cont::Waveform => {
                let r = self.cur_waveform.clone().expect("a waveform");
                let kind = self.f.waveform(&r).kind;
                self.read_waveform(Some(r), kind)
            }
            Cont::Ramp => self.read_ramp()?,
            // `readModelSpec`, `readReceiverThresholds`, `readAlgorithmicModel`: "TODO"
            Cont::ModelSpec | Cont::RxThresholds | Cont::AlgorithmicModel => true,
            Cont::AddSubmodel => self.read_add_submodel(),
            Cont::SubmodelSpec => self.read_submodel_spec(),
            Cont::PackageModelPins => self.read_package_model_pins(),
            Cont::Matrix => self.read_matrix_data(),
            Cont::None => {
                self.lx.report("Missing keyword.", Severity::Error);
                false
            }
        })
    }

    /// `IbisParser::storeString`.
    fn store_string(&mut self, r: StrRef, multiline: bool) -> bool {
        self.lx.skip_whitespaces();
        let mut status = self.lx.read_string(self.f.string(r));
        self.cont = if multiline { Cont::String } else { Cont::None };
        self.continuing = Some(r);
        status &= self.lx.check_end_of_line();
        status
    }

    /// `storeString( aDest, false )` for a name read before its section is added (`IbisModelSelector MS; storeString( MS.m_name, false )`).
    fn store_name(&mut self) -> (String, bool) {
        self.lx.skip_whitespaces();
        let mut name = String::new();
        let mut status = self.lx.read_string(&mut name);
        self.cont = Cont::None;
        status &= self.lx.check_end_of_line();
        (name, status)
    }

    /// `IbisParser::changeContext`.
    fn change_context(&mut self, keyword: &str) -> R {
        let mut status = true;
        let rep = &mut self.lx.rep;
        match self.ctx {
            Ctx::Header => status &= self.f.header.check(rep)?,
            Ctx::Component => status &= self.f.components[self.cur_component.expect("in a [Component]")].check(rep),
            Ctx::Model => status &= self.f.models[self.cur_model.expect("in a [Model]")].check(rep),
            Ctx::Submodel => {
                let name = self.cur_submodel.clone().expect("in a [Submodel]");
                status &= self.f.submodels[&name].check(rep);
            }
            // `IbisModelSelector::Check`
            Ctx::ModelSelector => {}
            Ctx::PackageModel => status &= self.f.package_models[self.cur_package_model.expect("in a package model")].check(rep),
            Ctx::End => {
                report(rep, "Cannot change context after [END]", Severity::Info);
                status = false;
            }
            Ctx::PackageModelModelData | Ctx::AlgorithmicModel => report(rep, "Changing context from an undefined context", Severity::Info),
        }

        if !compare_ibis_word(keyword, "End") && status {
            if compare_ibis_word(keyword, "Component") {
                self.f.components.push(IbisParsedComponent::default());
                let i = self.f.components.len() - 1;
                self.cur_component = Some(i);
                status &= self.store_string(StrRef::ComponentName(i), false);
                self.ctx = Ctx::Component;
            } else if compare_ibis_word(keyword, "Model_Selector") {
                let (name, ok) = self.store_name();
                status &= ok;
                self.f.model_selectors.push(IbisModelSelector { name, models: Vec::new() });
                self.cur_model_selector = Some(self.f.model_selectors.len() - 1);
                self.ctx = Ctx::ModelSelector;
                self.cont = Cont::ModelSelector;
            } else if compare_ibis_word(keyword, "Model") {
                let mut model = IbisModel::default();
                model.temperature_range.value[MIN] = 0.0;
                model.temperature_range.value[TYP] = 50.0;
                model.temperature_range.value[MAX] = 100.0;
                let (name, ok) = self.store_name();
                status &= ok;
                model.name = name;
                self.f.models.push(model);
                self.cur_model = Some(self.f.models.len() - 1);
                self.ctx = Ctx::Model;
                self.cont = Cont::Model;
            } else if compare_ibis_word(keyword, "Submodel") {
                let (name, ok) = self.store_name();
                status &= ok;
                if self.f.submodels.contains_key(&name) {
                    self.lx.report(format!("Submodel name is not unique: {name}"), Severity::Error);
                    status = false;
                } else {
                    self.f.submodels.insert(name.clone(), IbisSubmodel { name: name.clone(), ..Default::default() });
                }
                self.cur_submodel = Some(name);
                self.ctx = Ctx::Submodel;
                self.cont = Cont::Submodel;
            } else if compare_ibis_word(keyword, "Define_Package_Model") {
                let (name, ok) = self.store_name();
                status &= ok;
                self.f.package_models.push(IbisPackageModel { name, ..Default::default() });
                self.cur_package_model = Some(self.f.package_models.len() - 1);
                self.ctx = Ctx::PackageModel;
            } else if compare_ibis_word(keyword, "End_Package_Model") {
                // "else .pkg file, we just go back to header, to get the [END] keyword"
                self.ctx = if self.cur_component.is_some() { Ctx::Component } else { Ctx::Header };
                self.cont = Cont::None;
            } else {
                status = false;
                let context = match self.ctx {
                    Ctx::Header => "HEADER",
                    Ctx::Component => "COMPONENT",
                    Ctx::ModelSelector => "MODEL_SELECTOR",
                    Ctx::Model => "MODEL",
                    Ctx::PackageModel => "PACKAGE_MODEL",
                    Ctx::PackageModelModelData => "PACKAGE_MODEL_MODEL_DATA",
                    _ => "???",
                };
                self.lx.report(format!("Unknown keyword in {context} context: {keyword}"), Severity::Error);
            }
        } else {
            self.ctx = Ctx::End;
        }
        Ok(status)
    }

    /// `IbisParser::parseHeader`.
    fn parse_header(&mut self, kw: &str) -> R {
        let mut status = true;
        if compare_ibis_word(kw, "IBIS_Ver") {
            status &= self.lx.read_double(&mut self.f.header.ibis_version);
        } else if compare_ibis_word(kw, "Comment_char") {
            let ok = self.lx.change_comment_char();
            if ok {
                self.cont = Cont::None;
            }
            status &= ok;
        } else if compare_ibis_word(kw, "File_Name") {
            status &= self.store_string(StrRef::FileName, false);
        } else if compare_ibis_word(kw, "File_Rev") {
            status &= self.lx.read_double(&mut self.f.header.file_revision);
        } else if compare_ibis_word(kw, "Source") {
            status &= self.store_string(StrRef::Source, true);
        } else if compare_ibis_word(kw, "Notes") {
            status &= self.store_string(StrRef::Notes, true);
        } else if compare_ibis_word(kw, "Disclaimer") {
            status &= self.store_string(StrRef::Disclaimer, true);
        } else if compare_ibis_word(kw, "Copyright") {
            status &= self.store_string(StrRef::Copyright, true);
        } else if compare_ibis_word(kw, "Date") {
            status &= self.store_string(StrRef::Date, false);
        } else if !self.change_context(kw)? {
            status = false;
        }
        Ok(status)
    }

    /// `IbisParser::parseComponent` (`[Die Supply Pads]` is "Not supported yet" in KiCad: an unknown keyword).
    fn parse_component(&mut self, kw: &str) -> R {
        let mut status = true;
        let cc = self.cc();
        if compare_ibis_word(kw, "Manufacturer") {
            status &= self.store_string(StrRef::ComponentManufacturer(cc), true);
        } else if compare_ibis_word(kw, "Package") {
            status &= self.read_package();
        } else if compare_ibis_word(kw, "Pin") {
            status &= self.read_pin()?;
        } else if compare_ibis_word(kw, "Pin_Mapping") {
            status &= self.read_pin_mapping();
        } else if compare_ibis_word(kw, "Diff_Pin") {
            status &= self.read_diff_pin();
        } else if compare_ibis_word(kw, "Series_Pin_Mapping") {
            status &= self.read_series_pin_mapping();
        } else if compare_ibis_word(kw, "Package_Model") {
            status &= self.store_string(StrRef::ComponentPackageModel(cc), true);
        } else if !self.change_context(kw)? {
            status = false;
        }
        Ok(status)
    }

    /// `readTypMinMaxValue` into a value of the current model.
    fn model_value(&mut self, which: fn(&mut IbisModel) -> &mut TypMinMaxValue) -> bool {
        let cm = self.cm();
        let Parser { lx, f, .. } = self;
        lx.read_typ_min_max_value(which(&mut f.models[cm]))
    }

    /// `IbisParser::parseModel`.
    fn parse_model(&mut self, kw: &str) -> R {
        let cm = self.cm();
        let owner = Owner::Model(cm);
        Ok(if compare_ibis_word(kw, "Voltage_Range") {
            self.model_value(|m| &mut m.voltage_range)
        } else if compare_ibis_word(kw, "Temperature_Range") {
            self.model_value(|m| &mut m.temperature_range)
        } else if compare_ibis_word(kw, "GND_Clamp") {
            self.read_iv_table_entry(IvRef::GndClamp(owner))
        } else if compare_ibis_word(kw, "POWER_Clamp") {
            self.read_iv_table_entry(IvRef::PowerClamp(owner))
        } else if compare_ibis_word(kw, "Pulldown") {
            self.read_iv_table_entry(IvRef::Pulldown(owner))
        } else if compare_ibis_word(kw, "Pullup") {
            self.read_iv_table_entry(IvRef::Pullup(owner))
        } else if compare_ibis_word(kw, "ISSO_PD") {
            self.read_iv_table_entry(IvRef::IssoPd(cm))
        } else if compare_ibis_word(kw, "ISSO_PU") {
            self.read_iv_table_entry(IvRef::IssoPu(cm))
        } else if compare_ibis_word(kw, "Composite_Current") {
            self.read_iv_table_entry(IvRef::CompositeCurrent(cm))
        } else if compare_ibis_word(kw, "Rising_Waveform") {
            self.read_waveform(None, IbisWaveformType::Rising)
        } else if compare_ibis_word(kw, "Falling_Waveform") {
            self.read_waveform(None, IbisWaveformType::Falling)
        } else if compare_ibis_word(kw, "Ramp") {
            self.read_ramp()?
        } else if compare_ibis_word(kw, "Model_Spec") {
            // `readModelSpec`
            self.cont = Cont::ModelSpec;
            true
        } else if compare_ibis_word(kw, "Receiver_Thresholds") {
            // `readReceiverThresholds`
            self.cont = Cont::RxThresholds;
            true
        } else if compare_ibis_word(kw, "Pullup_Reference") {
            self.model_value(|m| &mut m.pullup_reference)
        } else if compare_ibis_word(kw, "Pulldown_Reference") {
            self.model_value(|m| &mut m.pulldown_reference)
        } else if compare_ibis_word(kw, "POWER_Clamp_Reference") {
            self.model_value(|m| &mut m.power_clamp_reference)
        } else if compare_ibis_word(kw, "GND_Clamp_Reference") {
            self.model_value(|m| &mut m.gnd_clamp_reference)
        } else if compare_ibis_word(kw, "Rac") {
            self.model_value(|m| &mut m.rac)
        } else if compare_ibis_word(kw, "Cac") {
            self.model_value(|m| &mut m.cac)
        } else if compare_ibis_word(kw, "Rpower") {
            self.model_value(|m| &mut m.rpower)
        } else if compare_ibis_word(kw, "Rgnd") {
            self.model_value(|m| &mut m.rgnd)
        } else if compare_ibis_word(kw, "Algorithmic_Model") {
            self.ctx = Ctx::AlgorithmicModel;
            self.cont = Cont::AlgorithmicModel;
            true
        } else if compare_ibis_word(kw, "Add_Submodel") {
            self.read_add_submodel()
        } else {
            self.change_context(kw)?
        })
    }

    /// `IbisParser::parseSubmodel`.
    fn parse_submodel(&mut self, kw: &str) -> R {
        let cs = self.cs();
        let owner = Owner::Submodel(cs.clone());
        Ok(if compare_ibis_word(kw, "Submodel_Spec") {
            self.read_submodel_spec()
        } else if compare_ibis_word(kw, "Pullup") {
            self.read_iv_table_entry(IvRef::Pullup(owner))
        } else if compare_ibis_word(kw, "Pulldown") {
            self.read_iv_table_entry(IvRef::Pulldown(owner))
        } else if compare_ibis_word(kw, "GND_Clamp") {
            self.read_iv_table_entry(IvRef::GndClamp(owner))
        } else if compare_ibis_word(kw, "POWER_Clamp") {
            self.read_iv_table_entry(IvRef::PowerClamp(owner))
        } else if compare_ibis_word(kw, "GND_Pulse") {
            self.read_iv_table_entry(IvRef::GndPulse(cs))
        } else if compare_ibis_word(kw, "POWER_Pulse") {
            self.read_iv_table_entry(IvRef::PowerPulse(cs))
        } else if compare_ibis_word(kw, "Rising_Waveform") {
            self.read_waveform(None, IbisWaveformType::Rising)
        } else if compare_ibis_word(kw, "Falling_Waveform") {
            self.read_waveform(None, IbisWaveformType::Falling)
        } else if compare_ibis_word(kw, "Ramp") {
            self.read_ramp()?
        } else {
            self.change_context(kw)?
        })
    }

    /// `IbisParser::parsePackageModel`.
    fn parse_package_model(&mut self, kw: &str) -> R {
        let mut status = true;
        let pm = self.cpm();
        if compare_ibis_word(kw, "Manufacturer") {
            status &= self.store_string(StrRef::PackageModelManufacturer(pm), false);
        } else if compare_ibis_word(kw, "OEM") {
            status &= self.store_string(StrRef::PackageModelOem(pm), false);
        } else if compare_ibis_word(kw, "Description") {
            status &= self.store_string(StrRef::PackageModelDescription(pm), false);
        } else if compare_ibis_word(kw, "Number_of_Pins") {
            status &= self.lx.read_int(&mut self.f.package_models[pm].number_of_pins);
        } else if compare_ibis_word(kw, "Pin_Numbers") {
            status &= self.read_package_model_pins();
        } else if compare_ibis_word(kw, "Model_Data") {
            self.ctx = Ctx::PackageModelModelData;
            self.cont = Cont::None;
        } else if !self.change_context(kw)? {
            status = false;
        }
        Ok(status)
    }

    /// `IbisParser::parsePackageModelModelData`.
    fn parse_package_model_model_data(&mut self, kw: &str) -> R {
        let mut status = true;
        let pm = self.cpm();
        if compare_ibis_word(kw, "Resistance_Matrix") {
            status = self.read_matrix_type(MatRef::Resistance(pm));
        } else if compare_ibis_word(kw, "Capacitance_Matrix") {
            status = self.read_matrix_type(MatRef::Capacitance(pm));
        } else if compare_ibis_word(kw, "Inductance_Matrix") {
            status = self.read_matrix_type(MatRef::Inductance(pm));
        } else if compare_ibis_word(kw, "Bandwidth") {
            status = self.read_matrix_bandwidth();
        } else if compare_ibis_word(kw, "Row") {
            status = self.read_matrix_row();
        } else if compare_ibis_word(kw, "End_Model_Data") {
            self.ctx = Ctx::PackageModel;
            self.cont = Cont::None;
        } else if !self.change_context(kw)? {
            status = false;
        }
        Ok(status)
    }

    /// `IbisParser::parseAlgorithmicModel`: everything up to `[End Algorithmic Model]` is skipped.
    fn parse_algorithmic_model(&mut self, kw: &str) -> R {
        if compare_ibis_word(kw, "End_Algorithmic_Model") {
            self.ctx = Ctx::Model;
            self.cont = Cont::None;
            Ok(true)
        } else {
            self.change_context(kw)
        }
    }

    /// `IbisParser::readPackage`: `R_pkg|L_pkg|C_pkg typ min max` (on the keyword's own line, KiCad wants one more word).
    fn read_package(&mut self) -> bool {
        let mut status = true;
        let cc = self.cc();
        let fields = self.lx.read_table_line();
        let extra = if self.cont == Cont::None { 1 } else { 0 };
        if fields.len() == 4 + extra {
            let Parser { lx, f, .. } = self;
            let package = &mut f.components[cc].package;
            let value = match fields[0].as_str() {
                "R_pkg" => &mut package.r_pkg,
                "L_pkg" => &mut package.l_pkg,
                "C_pkg" => &mut package.c_pkg,
                _ => {
                    lx.report("Invalid field in [Package]", Severity::Info);
                    return false;
                }
            };
            status &= lx.parse_double(&mut value.value[TYP], &fields[1]);
            // "Min / max values are optional, so don't update the status"
            lx.parse_double(&mut value.value[MIN], &fields[2]);
            lx.parse_double(&mut value.value[MAX], &fields[3]);
        } else if !fields.is_empty() {
            self.lx.report("A [Package] line requires exactly 4 elements.", Severity::Error);
            status = false;
        }
        self.cont = Cont::ComponentPackage;
        status
    }

    /// `IbisParser::readPin`: the whole line (the keyword's included), its first one the column heads.
    fn read_pin(&mut self) -> R {
        let mut status = true;
        let cc = self.cc();
        self.lx.line_index = 0;
        let fields = self.lx.read_table_line();
        let mut pin = IbisComponentPin::default();
        let continuing = self.cont == Cont::ComponentPin;
        let last = self.f.components[cc].pins.last().map(|p| (p.r_col, p.l_col, p.c_col)).unwrap_or((0, 0, 0));
        if fields.len() == 3 {
            if continuing {
                pin.pin_name = fields[0].clone();
                pin.signal_name = fields[1].clone();
                pin.model_name = fields[2].clone();
                (pin.r_col, pin.l_col, pin.c_col) = last;
            } else {
                pin.dummy = true;
            }
        } else if continuing {
            pin.pin_name = vec_at(&fields, 0)?.clone();
            pin.signal_name = vec_at(&fields, 1)?.clone();
            pin.model_name = vec_at(&fields, 2)?.clone();
            (pin.r_col, pin.l_col, pin.c_col) = last;
            if pin.r_col == 0 || pin.l_col == 0 || pin.c_col == 0 {
                self.lx.report("Invalid pin entry: 6 values from a table with only 3.", Severity::Error);
                status = false;
            } else {
                let mut ok = self.lx.parse_double(&mut pin.r_pin, vec_at(&fields, pin.r_col as usize)?);
                ok = ok && self.lx.parse_double(&mut pin.l_pin, vec_at(&fields, pin.l_col as usize)?);
                ok = ok && self.lx.parse_double(&mut pin.c_pin, vec_at(&fields, pin.c_col as usize)?);
                if !ok {
                    self.lx.report("Can't read a R, L or C value for a pin.", Severity::Error);
                    status = false;
                }
            }
        } else {
            for i in 3..6 {
                match vec_at(&fields, i)?.as_str() {
                    "R_pin" => pin.r_col = i as i32,
                    "L_pin" => pin.l_col = i as i32,
                    "C_pin" => pin.c_col = i as i32,
                    _ => {
                        self.lx.report("Invalid field name in [Pin]", Severity::Error);
                        status = false;
                    }
                }
            }
            if pin.r_col == 0 || pin.l_col == 0 || pin.c_col == 0 {
                self.lx.report("Missing argument in [Pin]", Severity::Error);
                status = false;
            }
            pin.dummy = true;
        }
        self.f.components[cc].pins.push(pin);
        self.cont = Cont::ComponentPin;
        Ok(status)
    }

    /// `IbisParser::readPinMapping`.
    fn read_pin_mapping(&mut self) -> bool {
        let mut status = true;
        let fields = self.lx.read_table_line();
        if self.cont == Cont::None {
            // "No info on first line"
            self.cont = Cont::ComponentPinMapping;
        } else if !fields.is_empty() {
            let mut mapping = IbisComponentPinMapping::default();
            if fields.len() > 6 || fields.len() < 3 {
                self.lx.report("Wrong number of columns for pin mapping.", Severity::Error);
                status = false;
            } else {
                mapping.pin_name = fields[0].clone();
                mapping.pd_ref = fields[1].clone();
                mapping.pu_ref = fields[2].clone();
                mapping.gnd_clamp_ref = fields.get(3).cloned().unwrap_or_default();
                mapping.power_clamp_ref = fields.get(4).cloned().unwrap_or_default();
                mapping.ext_ref = fields.get(5).cloned().unwrap_or_default();
            }
            let cc = self.cc();
            self.f.components[cc].pin_mappings.push(mapping);
        }
        status
    }

    /// `IbisParser::readDiffPin`: `pin inv_pin ...` (the rest of the row is not read).
    fn read_diff_pin(&mut self) -> bool {
        let mut status = true;
        self.lx.line_index = 0;
        if self.cont == Cont::None {
            self.cont = Cont::ComponentDiffPin;
        } else {
            let mut entry = IbisDiffPinEntry { pin_a: self.lx.read_word(), ..Default::default() };
            if entry.pin_a.is_empty() {
                self.lx.report("Incorrect diff pin name", Severity::Error);
                status = false;
            }
            entry.pin_b = self.lx.read_word();
            if entry.pin_b.is_empty() {
                self.lx.report("Incorrect inv_pin name", Severity::Error);
                status = false;
            }
            if status {
                let cc = self.cc();
                self.f.components[cc].diff_pins.push(entry);
            }
        }
        status
    }

    /// `IbisParser::readSeriesPinMapping`: `pin_1 pin_2 model_name [function_table_group]`.
    fn read_series_pin_mapping(&mut self) -> bool {
        let fields = self.lx.read_table_line();
        if self.cont == Cont::None {
            self.cont = Cont::ComponentSeriesPinMapping;
        } else if !fields.is_empty() {
            if fields.len() >= 3 {
                let mapping = IbisComponentSeriesPinMapping {
                    pin1: fields[0].clone(),
                    pin2: fields[1].clone(),
                    model_name: fields[2].clone(),
                    function_table_group: fields.get(3).cloned().unwrap_or_default(),
                };
                let cc = self.cc();
                self.f.components[cc].series_pin_mappings.push(mapping);
            } else {
                self.lx.report("Wrong number of columns for series pin mapping.", Severity::Warning);
            }
        }
        true
    }

    /// `IbisParser::readModelSelector`: `model_name description`.
    fn read_model_selector(&mut self) -> bool {
        let model_name = self.lx.read_word();
        if model_name.is_empty() {
            return false;
        }
        let mut entry = IbisModelSelectorEntry { model_name, model_description: String::new() };
        let status = self.lx.read_string(&mut entry.model_description);
        let ms = self.cur_model_selector.expect("in a [Model Selector]");
        self.f.model_selectors[ms].models.push(entry);
        status
    }

    /// `IbisParser::readModel`: a subparameter of `[Model]` (`Model_type`, `Enable`, `Polarity`, `Vinl = ..`, ..., `C_comp typ min max`, ...).
    /// The `name = value` ones and `C_comp...` must start the line (`readNumericSubparam` reads from its first character).
    fn read_model(&mut self) -> bool {
        let mut status = true;
        let subparam = self.lx.read_word();
        if subparam.is_empty() {
            return status;
        }
        if self.cont != Cont::Model {
            self.lx.report("Continued reading a model that did not begin. ( internal error )", Severity::Error);
            return false;
        }
        let cm = self.cm();
        // `subparam.substr( 0, n )`
        let sb = subparam.as_bytes();
        let prefix = |n: usize| &sb[..sb.len().min(n)];
        if prefix(10) == b"Model_type" {
            let w = self.lx.read_word();
            if w.is_empty() {
                self.lx.report("Internal Error while reading model_type", Severity::Error);
                status = false;
            } else {
                use IbisModelType as T;
                let kind = [
                    ("Input", T::InputStd),
                    ("Output", T::Output),
                    ("I/O", T::Io),
                    ("3-state", T::ThreeState),
                    ("Open_drain", T::OpenDrain),
                    ("I/O_Open_drain", T::IoOpenDrain),
                    ("Open_sink", T::OpenSink),
                    ("I/O_open_sink", T::IoOpenSink),
                    ("Open_source", T::OpenSource),
                    ("I/O_open_source", T::IoOpenSource),
                    ("Input_ECL", T::InputEcl),
                    ("Output_ECL", T::OutputEcl),
                    ("I/O_ECL", T::IoEcl),
                    ("3-state_ECL", T::ThreeStateEcl),
                    ("Terminator", T::Terminator),
                    ("Series", T::Series),
                    ("Series_switch", T::SeriesSwitch),
                ]
                .into_iter()
                .find(|(n, _)| compare_ibis_word(&w, n));
                match kind {
                    Some((_, k)) => self.f.models[cm].kind = k,
                    None => {
                        self.lx.report(format!("Unknown Model_type {w}"), Severity::Error);
                        status = false;
                    }
                }
            }
        } else if prefix(7) == b"Enable" {
            let w = self.lx.read_word();
            if w.is_empty() {
                self.lx.report("Internal Error while reading Enable", Severity::Error);
                status = false;
            } else if w == "Active-High" {
                self.f.models[cm].enable = IbisModelEnable::ActiveHigh;
            } else if w == "Active-Low" {
                self.f.models[cm].enable = IbisModelEnable::ActiveLow;
            } else {
                self.lx.report(format!("Unknown Enable: {w}"), Severity::Error);
                status = false;
            }
        } else if prefix(9) == b"Polarity" {
            let w = self.lx.read_word();
            if w.is_empty() {
                self.lx.report("Internal Error while reading Enable", Severity::Error);
                status = false;
            } else if w == "Inverting" {
                self.f.models[cm].polarity = IbisModelPolarity::Inverting;
            } else if w == "Non-Inverting" {
                self.f.models[cm].polarity = IbisModelPolarity::NonInverting;
            } else {
                self.lx.report(format!("Unknown polarity {w}"), Severity::Error);
                status = false;
            }
        } else {
            let Parser { lx, f, .. } = self;
            let m = &mut f.models[cm];
            let found = lx.read_numeric_subparam("Vinl", &mut m.vinl)
                || lx.read_numeric_subparam("Vinh", &mut m.vinh)
                || lx.read_numeric_subparam("Vref", &mut m.vref)
                || lx.read_numeric_subparam("Rref", &mut m.rref)
                || lx.read_numeric_subparam("Cref", &mut m.cref)
                || lx.read_numeric_subparam("Vmeas", &mut m.vmeas)
                || lx.read_typ_min_max_value_subparam("C_comp", &mut m.c_comp)
                || lx.read_typ_min_max_value_subparam("C_comp_gnd_clamp", &mut m.c_comp_gnd_clamp)
                || lx.read_typ_min_max_value_subparam("C_comp_power_clamp", &mut m.c_comp_power_clamp)
                || lx.read_typ_min_max_value_subparam("C_comp_pullup", &mut m.c_comp_pullup)
                || lx.read_typ_min_max_value_subparam("C_comp_pulldown", &mut m.c_comp_pulldown);
            if !found {
                status = false;
            }
        }
        self.cont = Cont::Model;
        status
    }

    /// `IbisParser::readSubmodel`: `Submodel_type Dynamic_clamp|Bus_hold|Fall_back`.
    fn read_submodel(&mut self) -> bool {
        let mut status = true;
        let subparam = self.lx.read_word();
        if subparam.is_empty() {
            return status;
        }
        if subparam == "Submodel_type" {
            let w = self.lx.read_word();
            let cs = self.cs();
            let kind = match w.as_str() {
                "" => {
                    self.lx.report("The Submodel_type value is missing.", Severity::Error);
                    return false;
                }
                "Dynamic_clamp" => IbisSubmodelType::DynamicClamp,
                "Bus_hold" => IbisSubmodelType::BusHold,
                "Fall_back" => IbisSubmodelType::FallBack,
                _ => {
                    self.lx.report(format!("{w} is not a recognised submodel type."), Severity::Error);
                    return false;
                }
            };
            self.f.submodel(&cs).kind = kind;
        } else {
            self.lx.report(format!("{subparam} is not a recognised Submodel subparameter."), Severity::Error);
            status = false;
        }
        status
    }

    /// `IbisParser::readSubmodelSpec`: `Off_delay|V_trigger_r|V_trigger_f typ min max`.
    fn read_submodel_spec(&mut self) -> bool {
        let mut status = true;
        self.cont = Cont::SubmodelSpec;
        self.lx.skip_whitespaces();
        let subparam = self.lx.read_word();
        if !subparam.is_empty() {
            let cs = self.cs();
            let Parser { lx, f, .. } = self;
            let sub = f.submodel(&cs);
            match subparam.as_str() {
                "Off_delay" => status = lx.read_typ_min_max_value(&mut sub.off_delay),
                "V_trigger_r" => status = lx.read_typ_min_max_value(&mut sub.v_trigger_r),
                "V_trigger_f" => status = lx.read_typ_min_max_value(&mut sub.v_trigger_f),
                _ => {
                    lx.report(format!("{subparam} is not a recognised Submodel Spec subparameter."), Severity::Error);
                    status = false;
                }
            }
        }
        status
    }

    /// `IbisParser::readAddSubmodel`: `submodel_name All|Driving|Non-Driving`.
    fn read_add_submodel(&mut self) -> bool {
        self.cont = Cont::AddSubmodel;
        self.lx.skip_whitespaces();
        if self.lx.line_index < self.lx.line_length {
            let name = self.lx.read_word();
            if name.is_empty() {
                self.lx.report("The submodel name is missing.", Severity::Info);
                return false;
            }
            let s = self.lx.read_word();
            if s.is_empty() {
                self.lx.report("The submodel mode is missing.", Severity::Info);
                return false;
            }
            let mode = match s.as_str() {
                "All" => SubmodelMode::All,
                "Driving" => SubmodelMode::Driving,
                "Non-Driving" => SubmodelMode::NonDriving,
                _ => {
                    self.lx.report(format!("{s} is not a recognised submodel mode."), Severity::Warning);
                    return false;
                }
            };
            let cm = self.cm();
            self.f.models[cm].submodels.push(IbisSubmodelMode { name, mode });
        }
        true
    }

    /// `IbisParser::readRamp`: `R_load = ..`, `dV/dt_r typ min max`, `dV/dt_f typ min max`.
    fn read_ramp(&mut self) -> R {
        let mut status = true;
        self.cont = Cont::Ramp;
        let Parser { lx, f, ctx, cur_model, cur_submodel, .. } = self;
        let ramp = match ctx {
            Ctx::Model => &mut f.models[cur_model.expect("in a [Model]")].ramp,
            Ctx::Submodel => &mut f.submodel(cur_submodel.as_deref().expect("in a [Submodel]")).ramp,
            // `wxLogMessage( "Invalid context for ramp: %i." )`
            _ => return Ok(false),
        };
        if !lx.read_numeric_subparam("R_load", &mut ramp.r_load) {
            let s = lx.read_word();
            if !s.is_empty() {
                if s == "dV/dt_r" {
                    status &= lx.read_ramp_dvdt(&mut ramp.rising)?;
                } else if s == "dV/dt_f" {
                    status &= lx.read_ramp_dvdt(&mut ramp.falling)?;
                } else {
                    lx.report("Invalid ramp data", Severity::Error);
                    status = false;
                }
            }
        }
        Ok(status)
    }

    /// `IbisParser::readIVtableEntry`: `V I_typ I_min I_max`; a row whose typ current cannot be read is left out without failing the line.
    fn read_iv_table_entry(&mut self, r: IvRef) -> bool {
        let mut status = true;
        self.lx.skip_whitespaces();
        let mut entry = IvTableEntry::default();
        if self.lx.line_index < self.lx.line_length {
            status &= self.lx.read_double(&mut entry.v);
            if status && self.lx.read_typ_min_max_value(&mut entry.i) {
                self.f.iv(&r).entries.push(entry);
            }
        }
        self.cont = Cont::IvTable;
        self.cur_iv = Some(r);
        status
    }

    /// `IbisParser::readVTtableEntry`: `t V_typ V_min V_max` (it sets `m_continue` to `IV_TABLE`, which `readWaveform` then overrides).
    fn read_vt_table_entry(&mut self, r: WfRef) -> bool {
        let mut status = true;
        self.lx.skip_whitespaces();
        let mut entry = VtTableEntry::default();
        if self.lx.line_index < self.lx.line_length {
            status &= self.lx.read_double(&mut entry.t);
            status &= self.lx.read_typ_min_max_value(&mut entry.v);
        }
        self.cont = Cont::IvTable;
        if status {
            self.f.waveform(&r).table.entries.push(entry);
        }
        status
    }

    /// `IbisParser::readWaveform`: a new waveform on the keyword's line, then its `R_fixture = ..` (...) subparameters and its V-t rows.
    /// (KiCad stores `R_dut`, `L_dut`, `C_dut` in the fixture's fields; kept. A row on the keyword's own line goes to the waveform read
    /// before, `m_currentWaveform`, as in KiCad -- or to this one when there was none, where KiCad would crash.)
    fn read_waveform(&mut self, dest: Option<WfRef>, kind: IbisWaveformType) -> bool {
        let mut status = true;
        let wf = if self.cont != Cont::Waveform {
            let owner = match self.ctx {
                Ctx::Model => Owner::Model(self.cm()),
                Ctx::Submodel => Owner::Submodel(self.cs()),
                // `wxLogMessage( "Invalid context for waveform: %i" )`
                _ => return false,
            };
            let rising = kind == IbisWaveformType::Rising;
            let list = self.f.waveforms(&owner, rising);
            list.push(IbisWaveform { kind, ..Default::default() });
            let index = list.len() - 1;
            WfRef { owner, rising, index }
        } else {
            match dest {
                Some(d) => d,
                None => {
                    self.lx.report("Internal error detected, a waveform should exist", Severity::Error);
                    return false;
                }
            }
        };
        if status && !self.lx.is_line_empty_from_cursor() {
            let found = {
                let Parser { lx, f, .. } = self;
                let w = f.waveform(&wf);
                lx.read_numeric_subparam("R_fixture", &mut w.r_fixture)
                    || lx.read_numeric_subparam("L_fixture", &mut w.l_fixture)
                    || lx.read_numeric_subparam("C_fixture", &mut w.c_fixture)
                    || lx.read_numeric_subparam("V_fixture", &mut w.v_fixture)
                    || lx.read_numeric_subparam("V_fixture_min", &mut w.v_fixture_min)
                    || lx.read_numeric_subparam("V_fixture_max", &mut w.v_fixture_max)
                    || lx.read_numeric_subparam("R_dut", &mut w.r_fixture)
                    || lx.read_numeric_subparam("L_dut", &mut w.l_fixture)
                    || lx.read_numeric_subparam("C_dut", &mut w.c_fixture)
            };
            // "The line is not a subparameter, then let's try to read a VT table entry"
            if !found {
                let target = self.cur_waveform.clone().unwrap_or_else(|| wf.clone());
                if !self.read_vt_table_entry(target) {
                    status = false;
                }
            }
        }
        self.cur_waveform = Some(wf);
        self.cont = Cont::Waveform;
        status
    }

    /// `IbisParser::readPackageModelPins`: a pin name a line, numbered from 0.
    fn read_package_model_pins(&mut self) -> bool {
        self.cont = Cont::PackageModelPins;
        let s = self.lx.read_word();
        if !s.is_empty() {
            let pm = self.cpm();
            let pins = &mut self.f.package_models[pm].pins;
            // `m_pins[str] = m_pins.size()`: the size before the insertion (C++17)
            let n = pins.len() as i32;
            pins.insert(s, n);
        }
        true
    }

    /// `IbisParser::readMatrixPinIndex`.
    fn read_matrix_pin_index(&mut self, dest: &mut i32) -> bool {
        *dest = 0;
        let s = self.lx.read_word();
        if s.is_empty() {
            self.lx.report("Missing pin name/number for matrix row.", Severity::Info);
            return false;
        }
        let pm = self.cpm();
        match self.f.package_models[pm].pins.get(&s) {
            Some(i) => {
                *dest = *i;
                true
            }
            None => {
                self.lx.report("Invalid pin name/number in matrix row.", Severity::Info);
                false
            }
        }
    }

    /// `IbisParser::readMatrixType`: `Banded_matrix|Full_matrix|Sparse_matrix` (an unknown type is reported as itself, a warning: KiCad
    /// builds "%s is not a recognised matrix type." and reports `str`).
    fn read_matrix_type(&mut self, r: MatRef) -> bool {
        self.cur_matrix = Some(r);
        {
            let m = self.f.matrix(r);
            m.kind = IbisMatrixType::Undefined;
            m.rows = -1;
            m.cols = -1;
        }
        self.cur_matrix_row = -1;
        self.cur_matrix_col = -1;
        self.cont = Cont::None;
        let s = self.lx.read_word();
        if s.is_empty() {
            self.lx.report("The matrix type is missing.", Severity::Error);
            return false;
        }
        let kind = if compare_ibis_word(&s, "Banded_matrix") {
            IbisMatrixType::Banded
        } else if compare_ibis_word(&s, "Full_matrix") {
            IbisMatrixType::Full
        } else if compare_ibis_word(&s, "Sparse_matrix") {
            IbisMatrixType::Sparse
        } else {
            self.lx.report(s, Severity::Warning);
            return false;
        };
        let pins = self.f.package_models[self.cpm()].number_of_pins;
        let m = self.f.matrix(r);
        m.kind = kind;
        if kind != IbisMatrixType::Banded {
            m.rows = pins;
            m.cols = pins;
            m.data.resize((pins as i64 * pins as i64).max(0) as usize, 0.0);
        }
        true
    }

    /// `IbisParser::readMatrixBandwidth`: `[Bandwidth] n` of a banded matrix.
    fn read_matrix_bandwidth(&mut self) -> bool {
        let Some(r) = self.cur_matrix.filter(|r| {
            let m = self.f.matrix(*r);
            m.kind == IbisMatrixType::Banded && m.cols <= 0
        }) else {
            self.lx.report("Unexpected [Bandwidth] keyword.", Severity::Error);
            return false;
        };
        let mut bandwidth = 0;
        let status = self.lx.read_int(&mut bandwidth);
        if status {
            if bandwidth < 0 {
                self.lx.report("Matrix bandwidth must be >= 0.", Severity::Error);
                return false;
            }
            let pins = self.f.package_models[self.cpm()].number_of_pins;
            let m = self.f.matrix(r);
            m.rows = pins;
            m.cols = 1 + bandwidth;
            m.data.resize((pins as i64 * (1 + bandwidth as i64)).max(0) as usize, 0.0);
        }
        status
    }

    /// `IbisParser::readMatrixRow`: `[Row] pin`.
    fn read_matrix_row(&mut self) -> bool {
        let Some(r) = self.cur_matrix.filter(|r| self.f.matrix(*r).rows >= 0) else {
            self.lx.report("Unexpected [Row] keyword.", Severity::Error);
            return false;
        };
        let mut index = 0;
        let status = self.read_matrix_pin_index(&mut index);
        if status {
            self.cur_matrix_row = index;
            self.cur_matrix_col = if self.f.matrix(r).kind == IbisMatrixType::Full { index } else { 0 };
            self.cont = Cont::Matrix;
        }
        status
    }

    /// `IbisParser::readMatrixBandedOrFull`: a row's values, from the diagonal on.
    fn read_matrix_banded_or_full(&mut self, r: MatRef) -> bool {
        let values = self.lx.read_table_line();
        for v in &values {
            let (cols, len) = {
                let m = self.f.matrix(r);
                (m.cols, m.data.len())
            };
            if self.cur_matrix_col >= cols {
                self.lx.report("Too much data for this matrix row.", Severity::Error);
                return false;
            }
            let index = self.cur_matrix_row as i64 * cols as i64 + self.cur_matrix_col as i64;
            if index < 0 || index >= len as i64 {
                self.lx.report("Element location is outside the matrix.", Severity::Error);
                return false;
            }
            let Parser { lx, f, .. } = self;
            if !lx.parse_double(&mut f.matrix(r).data[index as usize], v) {
                lx.report("Failed to read a matrix element.", Severity::Error);
                return false;
            }
            self.cur_matrix_col += 1;
        }
        true
    }

    /// `IbisParser::readMatrixSparse`: `pin value`.
    fn read_matrix_sparse(&mut self, r: MatRef) -> bool {
        let mut index = 0;
        if !self.read_matrix_pin_index(&mut index) {
            self.lx.report("Failed to read a matrix pin name.", Severity::Error);
            return false;
        }
        let mut value = 0.0;
        if !self.lx.read_double(&mut value) {
            self.lx.report("Failed to read a matrix element.", Severity::Error);
            return false;
        }
        let row = self.cur_matrix_row as i64;
        let m = self.f.matrix(r);
        let index = index as i64 + row * m.cols as i64;
        if index < 0 || index >= m.data.len() as i64 {
            self.lx.report("Element location is outside the matrix.", Severity::Error);
            return false;
        }
        m.data[index as usize] = value;
        true
    }

    /// `IbisParser::readMatrixData`.
    fn read_matrix_data(&mut self) -> bool {
        let Some(r) = self.cur_matrix else {
            // `wxLogMessage( "Matrix pointer is null" )`
            return false;
        };
        match self.f.matrix(r).kind {
            IbisMatrixType::Banded | IbisMatrixType::Full => self.read_matrix_banded_or_full(r),
            IbisMatrixType::Sparse => self.read_matrix_sparse(r),
            // `wxLogMessage( "Invalid matrix type: %i" )`
            IbisMatrixType::Undefined => false,
        }
    }
}

/// `KIBIS::KIBIS` over `IbisParser::ParseFile` of the file's text: the reporter's messages, `m_valid`, the dialog's lists
/// (`KIBIS_COMPONENT`, `KIBIS_PIN`, the `[Diff Pin]` pairing) and the parser's `IbisFile`.
pub fn read_ibis(text: &str) -> IbisFile {
    let mut p = Parser::new(text);
    let result = p.parse_file();
    let Parser { lx, f, .. } = p;
    let mut out = IbisFile {
        messages: lx.rep,
        header: f.header,
        ibis_components: f.components,
        model_selectors: f.model_selectors,
        ibis_models: f.models,
        package_models: f.package_models,
        submodels: f.submodels,
        ..Default::default()
    };
    let status = match result {
        Ok(status) => status,
        Err(Thrown(what)) => {
            // `PGM_BASE::HandleException`, and KiCad stops
            out.messages.push((Severity::Error, format!("Unhandled exception class: St12out_of_range  what: {what}")));
            out.thrown = Some(what);
            return out;
        }
    };
    // `KIBIS_FILE::Init` and each `KIBIS_MODEL` are always valid
    out.models = out.ibis_models.iter().map(|m| m.name.clone()).collect();
    for comp in &out.ibis_components {
        // `KIBIS_COMPONENT::KIBIS_COMPONENT`: the dummy rows left out; each `KIBIS_PIN` valid
        let pins: Vec<&IbisComponentPin> = comp.pins.iter().filter(|p| !p.dummy).collect();
        let mut c = IbisComponent { name: comp.name.clone(), ..Default::default() };
        for pin in &pins {
            c.pins.push((pin.pin_name.clone(), pin.signal_name.clone()));
            // `KIBIS_PIN::KIBIS_PIN`: the first selector of that name, else the model name; every `[Model]` of each name
            let wanted: Vec<String> = match out.model_selectors.iter().find(|s| s.name == pin.model_name) {
                Some(s) => s.models.iter().map(|e| e.model_name.clone()).collect(),
                None => vec![pin.model_name.clone()],
            };
            c.pin_models.push(wanted.iter().flat_map(|w| out.models.iter().filter(move |m| *m == w).cloned()).collect());
        }
        // `KIBIS::KIBIS`: `GetPin` (the first pin of that number) of both pins of a row, each then the other's `m_complementaryPin`
        let mut diff = vec![false; pins.len()];
        for entry in &comp.diff_pins {
            let a = pins.iter().position(|p| p.pin_name == entry.pin_a);
            let b = pins.iter().position(|p| p.pin_name == entry.pin_b);
            if let (Some(a), Some(b)) = (a, b) {
                diff[a] = true;
                diff[b] = true;
            }
        }
        c.diff_pins = pins.iter().zip(&diff).filter(|(_, d)| **d).map(|(p, _)| p.pin_name.clone()).collect();
        out.components.push(c);
    }
    out.valid = status;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test file of the lists, as KiCad wants it: a `[File Rev]`, a complete `[Package]`, and each model's C_comp and voltage range.
    const IBS: &str = "[IBIS Ver]   5.1\n[Comment Char] #_char\n# a comment | not one\n[File Name]  t.ibs\n[File Rev] 1.0\n[Component]  ACME 1\n[Manufacturer] ACME\n[Package]\nR_pkg 1 1 1\nL_pkg 1n NA NA\nC_pkg 1p NA NA\n[Pin]  signal_name  model_name  R_pin L_pin C_pin\n1  OUT  driver  0.1 1n 1p # the output\n2  GND  GND  NA NA NA\n3  IOP  io_sel NA NA NA\n4  ION  io_sel NA NA NA\n[Diff Pin] inv_pin vdiff tdelay_typ tdelay_min tdelay_max\n3 4 0.1 0 NA NA\n[Model Selector] io_sel\nio33 3.3 V IO\nio18 1.8 V IO\n[Model] driver\nModel_type Output\nC_comp 1p NA NA\n[Voltage Range] 3.3 3 3.6\n[Model] io33\nModel_type I/O\nC_comp 1p NA NA\n[Voltage Range] 3.3 3 3.6\n[Model] io18\nModel_type I/O\nC_comp 1p NA NA\n[Voltage Range] 1.8 1.7 1.9\n[End]\n[Component] after the end\n";

    fn errors(f: &IbisFile) -> Vec<&str> {
        f.messages.iter().filter(|(s, _)| *s == Severity::Error).map(|(_, m)| m.as_str()).collect()
    }

    fn msgs(list: &[(Severity, &str)]) -> Vec<(Severity, String)> {
        list.iter().map(|(s, m)| (*s, m.to_string())).collect()
    }

    use Severity::{Action, Error, Warning};

    #[test]
    fn components_pins_models_and_differential_pins_as_kibis_lists_them() {
        let f = read_ibis(IBS);
        assert!(f.valid, "{:?}", f.messages);
        assert!(f.messages.is_empty(), "{:?}", f.messages);
        assert_eq!(f.models, ["driver", "io33", "io18"]);
        assert_eq!(f.components.len(), 1, "nothing after [End]");
        let c = &f.components[0];
        assert_eq!(c.name, "ACME 1");
        assert_eq!(c.pins, [("1".into(), "OUT".into()), ("2".into(), "GND".into()), ("3".into(), "IOP".into()), ("4".into(), "ION".into())]);
        assert_eq!(c.pin_models, [vec!["driver".to_string()], vec![], vec!["io33".into(), "io18".into()], vec!["io33".into(), "io18".into()]], "GND is no [Model]; a selector's entries");
        assert_eq!(c.diff_pins, ["3", "4"]);
        let p = &f.ibis_components[0];
        assert_eq!((p.pins[1].r_pin, p.pins[1].l_pin, p.pins[1].c_pin), (0.1, 1e-9, 1e-12), "the pin's R, L, C, `1n` read as 1e-9");
        assert!(p.pins[0].dummy && (p.pins[0].r_col, p.pins[0].l_col, p.pins[0].c_col) == (3, 4, 5));
        assert_eq!(f.model_selectors[0].models[0].model_description, " 3.3 V IO", "readString keeps the spaces before it");
    }

    #[test]
    fn a_pin_keyword_without_its_column_heads_throws_as_in_kicad() {
        // KiCad's `readPin` reads columns 3 to 5 of the `[Pin]` line itself (`fields.at( i )`): with the heads on the next line it throws
        let f = read_ibis("[IBIS Ver] 5.1\n[File Name] x.ibs\n[File Rev] 1\n[Component] X\n[Manufacturer] M\n[Pin]\nsignal_name model_name\n7 A m\n[Model] m\n");
        assert!(!f.valid);
        assert_eq!(f.thrown.as_deref(), Some("vector::_M_range_check: __n (which is 3) >= this->size() (which is 1)"));
        assert_eq!(
            f.messages,
            msgs(&[(Error, "Unhandled exception class: St12out_of_range  what: vector::_M_range_check: __n (which is 3) >= this->size() (which is 1)")])
        );
        assert!(f.components.is_empty(), "no KIBIS");
        // with the heads on the [Pin] line, a three-column table
        let f = read_ibis("[IBIS Ver] 5.1\n[File Name] x.ibs\n[File Rev] 1\n[Component] X\n[Manufacturer] M\n[Package]\nR_pkg 1 NA NA\nL_pkg 1n NA NA\nC_pkg 1p NA NA\n[Pin] signal_name model_name\n7 A m\n[Model] m\nModel_type Input\nC_comp 1p 1p 1p\n[Voltage Range] 5 5 5\n[End]\n");
        assert!(f.valid, "{:?}", f.messages);
        assert_eq!(f.components[0].pins, [("7".to_string(), "A".to_string())]);
        assert_eq!(f.components[0].pin_models, [vec!["m".to_string()]]);
    }

    const VALID: &str = "[IBIS Ver] 5.1
[File Name] small.ibs
[File Rev] 1.0
[Date] 12/11/2026
[Source] From a test,
         over two lines.
[Component] SMALL
[Manufacturer] ACME
[Package]
| variable typ min max
R_pkg 0.1 0.05 0.2
L_pkg 2n 1n 4n
C_pkg 1p 0.5p 2p
[Pin] signal_name model_name R_pin L_pin C_pin
1 OUT out 50m 1n 0.25p
2 GND GND NA NA NA
[Model] out
Model_type Output
Polarity Non-Inverting
Enable Active-High
Vinl = 0.8V
Vinh = 2.0V
C_comp 1.0pF 0.5pF 2.0pF
[Voltage Range] 3.3V 3.0V 3.6V
[Pulldown]
| V I(typ) I(min) I(max)
-3.3 -0.5 -0.25 -1
0 0 0 0
3.3 0.5 0.25 1
[Pullup]
-3.3 0.5 0.25 1
0 0 0 0
3.3 -0.5 -0.25 -1
[GND Clamp]
-3.3 -1 -1 -1
0 0 0 0
[POWER Clamp]
-3.3 1 1 1
0 0 0 0
[Ramp]
R_load = 50
dV/dt_r 1.0/0.5n 0.5/0.25n 2/1n
dV/dt_f 1.0/0.5n 0.5/0.25n 2/1n
[End]
";

    #[test]
    fn a_small_valid_file_is_valid_with_every_table_read() {
        let f = read_ibis(VALID);
        assert!(f.valid, "{:?}", f.messages);
        assert!(f.messages.is_empty(), "{:?}", f.messages);
        assert_eq!(f.header.ibis_version, 5.1);
        assert_eq!(f.header.source, "From a test,\nover two lines.");
        assert_eq!(f.ibis_components[0].package.l_pkg.value, [2e-9, 1e-9, 4e-9]);
        let m = &f.ibis_models[0];
        assert_eq!((m.kind, m.polarity, m.enable), (IbisModelType::Output, IbisModelPolarity::NonInverting, IbisModelEnable::ActiveHigh));
        assert_eq!((m.vinl, m.vinh), (0.8, 2.0));
        assert_eq!(m.c_comp.value, [1e-12, 0.5e-12, 2e-12]);
        assert_eq!(m.voltage_range.value, [3.3, 3.0, 3.6]);
        assert_eq!(m.temperature_range.value, [50.0, 0.0, 100.0], "the default of changeContext");
        assert_eq!(m.pulldown.entries.len(), 3);
        assert_eq!(m.pulldown.entries[2].v, 3.3);
        assert_eq!(m.pulldown.entries[2].i.value, [0.5, 0.25, 1.0]);
        assert_eq!((m.pullup.entries.len(), m.gnd_clamp.entries.len(), m.power_clamp.entries.len()), (3, 2, 2));
        assert_eq!(m.ramp.r_load, 50.0);
        assert_eq!(m.ramp.rising.value[TYP], Dvdt { dv: 1.0, dt: 0.5e-9 });
        assert_eq!(m.ramp.falling.value[MAX], Dvdt { dv: 2.0, dt: 1e-9 });
        assert_eq!(f.components[0].pin_models, [vec!["out".to_string()], vec![]]);
    }

    /// `VALID` with `from` replaced by `to`.
    fn with(from: &str, to: &str) -> IbisFile {
        assert!(VALID.contains(from), "{from}");
        read_ibis(&VALID.replacen(from, to, 1))
    }

    #[test]
    fn a_missing_ibis_ver_is_reported_when_the_header_is_left() {
        let f = with("[IBIS Ver] 5.1\n", "");
        assert!(!f.valid);
        assert_eq!(f.messages, msgs(&[(Action, "Checking Header..."), (Error, "Missing [IBIS Ver]"), (Error, "Error on line 6")]));
        assert!(f.components.is_empty(), "the parse stopped at [Component]");
    }

    #[test]
    fn a_component_without_a_manufacturer() {
        let f = with("[Manufacturer] ACME\n", "");
        assert!(!f.valid);
        assert_eq!(f.messages, msgs(&[(Action, "Checking component SMALL"), (Error, "Component: manufacturer cannot be empty."), (Error, "Error on line 16")]));
    }

    #[test]
    fn a_model_without_a_model_type() {
        let f = with("Model_type Output\n", "");
        assert!(!f.valid);
        assert_eq!(f.messages, msgs(&[(Action, "Checking model out"), (Error, "Undefined model type."), (Error, "Error on line 43")]));
    }

    #[test]
    fn a_model_without_c_comp_or_voltage_range() {
        let f = read_ibis(&VALID.replace("C_comp 1.0pF 0.5pF 2.0pF\n", "").replace("[Voltage Range] 3.3V 3.0V 3.6V\n", ""));
        assert_eq!(
            f.messages,
            msgs(&[(Action, "Checking model out"), (Error, "No C_comp values are available."), (Error, "Voltage Range or Reference is missing."), (Error, "Error on line 42")])
        );
    }

    #[test]
    fn an_iv_row_with_a_bad_voltage_fails_its_line() {
        let f = with("0 0 0 0\n3.3 0.5", "0 0 0 0\nabc 0.5");
        assert!(!f.valid);
        assert_eq!(f.messages, msgs(&[(Warning, "Failed to read a double."), (Error, "Error on line 29")]));
    }

    #[test]
    fn an_iv_row_with_a_bad_current_fails_the_table_when_the_model_is_left() {
        let f = with("3.3 0.5 0.25 1", "3.3 0.5 4x0m 1");
        assert!(f.valid, "\"4x0m\" is 4 (stod), its x ignored: {:?}", f.messages);
        let f = with("3.3 0.5 0.25 1", "3.3 0.5 abc 1");
        assert!(!f.valid);
        assert_eq!(
            f.messages,
            msgs(&[
                (Warning, "Failed to read a double."),
                (Error, "There is an invalid current in an IV table"),
                (Error, "Invalid Pulldown table."),
                (Error, "Error on line 44"),
            ])
        );
        // a typ current that cannot be read: the row is left out, and neither the line nor the file fails (the dialog still refuses it)
        let f = with("3.3 0.5 0.25 1", "3.3 abc 0.25 1");
        assert!(f.valid);
        assert_eq!(f.ibis_models[0].pulldown.entries.len(), 2);
        assert_eq!(f.messages, msgs(&[(Warning, "Failed to read a double."), (Error, "Typ-Min-Max Values requires at least Typ.")]));
    }

    #[test]
    fn a_typ_only_row_warns_for_its_missing_min_and_max() {
        let f = with("0 0 0 0\n3.3 0.5 0.25 1", "0 0 0 0\n3.3 0.5");
        assert!(f.valid);
        assert_eq!(f.messages, msgs(&[(Warning, "Failed to read a word."), (Warning, "Failed to read a word.")]));
    }

    #[test]
    fn unknown_keywords_and_lines_without_one() {
        let f = with("[Model] out\n", "[Bogus Thing]\n[Model] out\n");
        assert_eq!(f.messages, msgs(&[(Error, "Unknown keyword in COMPONENT context: Bogus_Thing"), (Error, "Error on line 17")]));
        // before any pin, the component's check fails first, and the keyword is not looked at
        let f = with("[Pin] signal_name", "[Bogus Thing]\n[Pin] signal_name");
        assert_eq!(f.messages, msgs(&[(Action, "Checking component SMALL"), (Error, "Component: no pin"), (Error, "Error on line 14")]));
        let f = with("[Date] 12/11/2026\n", "[Date] 12/11/2026\nstray words\n");
        assert_eq!(f.messages, msgs(&[(Error, "Missing keyword."), (Error, "Error on line 5")]));
        // a Series keyword of KiCad 25ec2335 is not known to 8303b2ad
        let f = with("[Ramp]\n", "[R Series] 1 NA NA\n[Ramp]\n");
        assert_eq!(f.messages, msgs(&[(Error, "Unknown keyword in MODEL context: R_Series"), (Error, "Error on line 40")]));
        // a model's subparameter must start its line
        let f = with("Vinl = 0.8V", "  Vinl = 0.8V");
        assert_eq!(f.messages, msgs(&[(Error, "Error on line 21")]));
    }

    #[test]
    fn an_invalid_ramp_is_reported_but_leaves_the_file_valid() {
        let f = with("dV/dt_r 1.0/0.5n", "dV/dt_r NA");
        assert!(f.valid, "IbisModel::Check does not fail on the ramp");
        assert_eq!(f.messages, msgs(&[(Error, "Invalid rising dV/dt."), (Error, "Invalid Ramp specification.")]));
        // a dV/dt with no "/": `aString.at( i )` throws
        let f = with("dV/dt_r 1.0/0.5n", "dV/dt_r 1.0");
        assert_eq!(f.thrown.as_deref(), Some("basic_string::at: __n (which is 3) >= this->size() (which is 3)"));
        assert!(!f.valid);
    }

    #[test]
    fn a_missing_file_name_throws_after_its_messages() {
        let f = with("[File Name] small.ibs\n", "");
        assert_eq!(
            f.messages,
            msgs(&[
                (Action, "Checking Header..."),
                (Error, "Missing [File Name]"),
                (Error, "Unhandled exception class: St12out_of_range  what: basic_string::substr: __pos (which is 18446744073709551612) > this->size() (which is 0)"),
            ])
        );
        let f = with("[File Name] small.ibs", "[File Name] small.txt");
        assert_eq!(f.messages, msgs(&[(Action, "Checking Header..."), (Error, "Invalid file extension in [File Name]"), (Error, "Error on line 7")]));
    }

    #[test]
    fn without_end_the_last_section_is_not_checked() {
        let f = read_ibis(&VALID.replace("Model_type Output\n", "").replace("[End]\n", ""));
        assert!(f.valid, "{:?}", f.messages);
        // a header alone is a valid file with no component
        let f = read_ibis("[IBIS Ver] 5.1\n");
        assert!(f.valid && f.components.is_empty() && f.messages.is_empty());
    }

    #[test]
    fn stod_reads_as_the_c_library_does() {
        assert_eq!(stod(b"1.5e3x"), Some((1500.0, 5)));
        assert_eq!(stod(b"5."), Some((5.0, 2)));
        assert_eq!(stod(b".5V"), Some((0.5, 2)));
        assert_eq!(stod(b"-2e"), Some((-2.0, 2)));
        assert_eq!(stod(b"0x10"), Some((16.0, 4)));
        assert_eq!(stod(b"abc"), None);
        assert_eq!(stod(b"1e999"), None, "ERANGE");
        assert_eq!(stod(b"inf").map(|v| v.1), Some(3));
        assert_eq!(stoi(b"12a"), Some((12, 2)));
        assert_eq!(stoi(b"99999999999"), None);
    }

    #[test]
    fn package_models_submodels_waveforms_and_algorithmic_models() {
        let text = "[IBIS Ver] 5.1
[File Name] pk.ibs
[File Rev] 1
[Component] C
[Manufacturer] M
[Package Model] PKG
[Package]
R_pkg 1 NA NA
L_pkg 1n NA NA
C_pkg 1p NA NA
[Pin] signal_name model_name R_pin L_pin C_pin
A1 IO io
A2 GND GND
[Model] io
Model_type Input
C_comp 1p NA NA
[Voltage Range] 3.3 NA NA
[Algorithmic Model]
anything at all
[End Algorithmic Model]
[Add Submodel]
odt Non-Driving
[Rising Waveform]
R_fixture = 50
V_fixture = 0
V_fixture_min = 0.5
0 0 0 0
1n 3.3 3.3 3.3
[Submodel] odt
Submodel_type Dynamic_clamp
[GND Clamp]
0 0 0 0
[Define Package Model] PKG
[Manufacturer] M
[OEM] O
[Number of Pins] 2
[Pin Numbers]
A1
A2
[Model Data]
[Resistance Matrix] Banded_matrix
[Bandwidth] 1
[Row] A1
0.5 0.25
[Row] A2
0.5
[Inductance Matrix] Full_matrix
[Row] A1
1n 2n
[Row] A2
1n
[Capacitance Matrix] Sparse_matrix
[Row] A1
A1 1p
A2 2p
[Row] A2
A2 1p
[End Model Data]
[End Package Model]
[End]
";
        let f = read_ibis(text);
        assert!(f.valid, "{:?}", f.messages);
        assert!(f.messages.is_empty(), "{:?}", f.messages);
        let m = &f.ibis_models[0];
        assert_eq!(m.submodels, [IbisSubmodelMode { name: "odt".into(), mode: SubmodelMode::NonDriving }]);
        let w = &m.rising_waveforms[0];
        assert_eq!((w.r_fixture, w.v_fixture, w.v_fixture_min, w.table.entries.len()), (50.0, 0.0, 0.5, 2));
        assert_eq!(f.submodels["odt"].kind, IbisSubmodelType::DynamicClamp);
        assert_eq!(f.submodels["odt"].gnd_clamp.entries.len(), 1);
        assert_eq!(f.ibis_components[0].package_model, "PKG");
        let p = &f.package_models[0];
        assert_eq!(p.pins.get("A2"), Some(&1));
        assert_eq!((p.resistance_matrix.rows, p.resistance_matrix.cols), (2, 2));
        assert_eq!(p.resistance_matrix.data, [0.5, 0.25, 0.5, 0.0]);
        assert_eq!(p.inductance_matrix.data, [1e-9, 2e-9, 0.0, 1e-9]);
        assert_eq!(p.capacitance_matrix.data, [1e-12, 2e-12, 0.0, 1e-12]);
        // a submodel nobody defines
        let f = read_ibis(&text.replace("odt Non-Driving", "nope All"));
        assert_eq!(errors(&f), ["nope submodel not found."]);
        assert!(!f.valid);
        // a package model without OEM
        let f = read_ibis(&text.replace("[OEM] O\n", ""));
        assert_eq!(f.messages[..2], msgs(&[(Action, "Checking package model PKG"), (Error, "OEM cannot be empty.")])[..]);
    }

    /// The IBIS file of the studio's browser check (web/studio/e2e/sim-model-io.ibs) is one KiCad loads.
    #[test]
    fn the_studio_check_file_is_valid() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/e2e/sim-model-io.ibs");
        let f = read_ibis(&std::fs::read_to_string(path).unwrap());
        assert!(f.valid && f.messages.is_empty(), "{:?}", f.messages);
        assert_eq!(f.components[0].pins.len(), 4);
        assert_eq!(f.components[0].diff_pins, ["3", "4"]);
        assert_eq!(f.models, ["drv", "io33", "io18"]);
    }

    /// KiCad's own QA files (`qa/data/eeschema/*.ibs`), which its tests load as valid: `KICAD_SRC_DIR` names a KiCad source tree (skipped
    /// when it is not set).
    #[test]
    fn kicad_qa_ibis_files_are_valid() {
        let Ok(src) = std::env::var("KICAD_SRC_DIR") else { return };
        let dir = std::path::Path::new(&src).join("qa/data/eeschema");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "ibs") {
                let text = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
                let f = read_ibis(&text);
                assert!(f.valid, "{}: {:?}", p.display(), f.messages);
                assert_eq!(errors(&f), Vec::<&str>::new(), "{}", p.display());
                assert!(!f.components.is_empty(), "{}", p.display());
                n += 1;
            }
        }
        assert!(n >= 3, "the QA files in {}", dir.display());
    }
}
