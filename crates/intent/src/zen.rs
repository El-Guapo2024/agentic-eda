//! Minimal Zener-like Starlark front-end.
//!
//! See the crate-level doc comment in `lib.rs` for why this hand-rolled
//! front-end exists instead of embedding `pcb-zen-core` directly.
//!
//! Supported DSL (a small, deliberately restricted subset of what real
//! Zener supports — enough to describe a flat schematic with modules):
//!
//! ```python
//! Component(
//!     "U1",
//!     mpn = "MCP1700T-3302E/TT",
//!     value = "3.3V LDO",
//!     package = "SOT-23",
//!     pins = [
//!         ("1", "GND", "ground"),
//!         ("2", "VOUT", "power"),
//!         ("3", "VIN", "power"),
//!     ],
//! )
//!
//! Net("VIN", ["U1.3", "C1.1"])
//!
//! Module("power_stage", ["U1", "C1", "C2"])
//! ```
//!
//! `PinKind` strings map case-insensitively to the model enum; anything
//! unrecognized becomes `signal` (kind is advisory, not an error).

use std::cell::RefCell;
use std::path::Path;

use eda_model::{CheckResult, Cluster, ConstraintModel, Net, Part, Pin, PinKind};
use starlark::environment::{GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::none::NoneType;
use starlark::values::ProvidesStaticType;

/// Accumulates parts/nets/clusters as the Starlark script executes.
#[derive(Default, ProvidesStaticType)]
struct Builder {
    parts: RefCell<Vec<Part>>,
    nets: RefCell<Vec<Net>>,
    clusters: RefCell<Vec<Cluster>>,
}

fn parse_pin_kind(s: &str) -> PinKind {
    match s.to_ascii_lowercase().as_str() {
        "power" | "pwr" | "vcc" | "vdd" => PinKind::Power,
        "ground" | "gnd" => PinKind::Ground,
        "passive" => PinKind::Passive,
        "nc" | "no_connect" | "noconnect" => PinKind::Nc,
        _ => PinKind::Signal,
    }
}

fn builder_from_eval<'a, 'v, 'e>(eval: &'a Evaluator<'v, '_, 'e>) -> anyhow::Result<&'a Builder> {
    eval.extra
        .and_then(|e: &'a dyn starlark::values::AnyLifetime<'e>| e.downcast_ref::<Builder>())
        .ok_or_else(|| anyhow::anyhow!("internal error: builder context missing"))
}

#[starlark::starlark_module]
fn zen_globals(gb: &mut GlobalsBuilder) {
    /// Declare a component/part.
    fn Component<'v>(
        reference: String,
        #[starlark(default = UnpackListOrTuple::default())] pins: UnpackListOrTuple<(
            String,
            String,
            String,
        )>,
        mpn: Option<String>,
        value: Option<String>,
        package: Option<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let b = builder_from_eval(eval)?;
        let pins = pins
            .into_iter()
            .map(|(number, name, kind)| Pin {
                number,
                name: if name.is_empty() { None } else { Some(name) },
                kind: parse_pin_kind(&kind),
            })
            .collect();
        b.parts.borrow_mut().push(Part {
            reference,
            mpn,
            lcsc: None,
            value,
            package,
            footprint: None,
            pins,
            body_um: None, symbol: None, datasheet: None,
            edge: None,
        });
        Ok(NoneType)
    }

    /// Declare a net and the pins ("REF.PIN") attached to it.
    fn Net<'v>(
        name: String,
        #[starlark(default = UnpackListOrTuple::default())] pins: UnpackListOrTuple<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let b = builder_from_eval(eval)?;
        b.nets.borrow_mut().push(Net {
            name,
            pins: pins.items,
        });
        Ok(NoneType)
    }

    /// Declare a module/cluster: an anchor reference plus its member refs.
    fn Module<'v>(
        anchor: String,
        #[starlark(default = UnpackListOrTuple::default())] members: UnpackListOrTuple<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        let b = builder_from_eval(eval)?;
        b.clusters.borrow_mut().push(Cluster {
            anchor,
            members: members.items,
            orientation: Default::default(),
        });
        Ok(NoneType)
    }
}

fn parse_error(path: &Path, err: impl std::fmt::Display) -> Vec<CheckResult> {
    vec![CheckResult::fail(
        "intent_parse_error",
        path.display().to_string(),
        err.to_string(),
    )]
}

/// Evaluate a `.zen`-style Starlark file into a [`ConstraintModel`].
///
/// Never panics: any evaluation failure (syntax error, undefined name,
/// runtime error in the script, missing file, etc.) comes back as
/// `Err(Vec<CheckResult>)` rather than unwinding.
pub fn import_zen(path: &Path) -> Result<ConstraintModel, Vec<CheckResult>> {
    let src = std::fs::read_to_string(path).map_err(|e| parse_error(path, e))?;

    let ast = AstModule::parse(&path.display().to_string(), src, &Dialect::Extended)
        .map_err(|e| parse_error(path, e))?;

    let globals = GlobalsBuilder::standard().with(zen_globals).build();

    type BuiltModel = (Vec<Part>, Vec<Net>, Vec<Cluster>);
    let eval_result: Result<BuiltModel, String> = Module::with_temp_heap(|module| {
        let builder = Builder::default();
        let eval_outcome = {
            let mut eval = Evaluator::new(&module);
            eval.extra = Some(&builder);
            let outcome = eval.eval_module(ast, &globals).map_err(|e| e.to_string());
            drop(eval);
            outcome
        };
        eval_outcome.map(|_| {
            (
                builder.parts.into_inner(),
                builder.nets.into_inner(),
                builder.clusters.into_inner(),
            )
        })
    });

    let (parts, nets, clusters) = match eval_result {
        Ok(v) => v,
        Err(msg) => {
            return Err(vec![CheckResult::fail(
                "intent_parse_error",
                path.display().to_string(),
                msg,
            )]);
        }
    };

    let model = ConstraintModel {
        parts,
        nets,
        clusters,
        placement_rules: Vec::new(),
        stackup: None,
        impedance_targets: Vec::new(),
        footprints: Vec::new(),
        symbols: Vec::new(),
        board: Default::default(),
        allow: Default::default(),
        solver: Default::default(),
    };

    // Structural sanity check: nets referencing pins on undeclared parts.
    let mut errors = Vec::new();
    for net in &model.nets {
        for pin_ref in &net.pins {
            let reference = pin_ref.split('.').next().unwrap_or(pin_ref);
            if model.part(reference).is_none() {
                errors.push(CheckResult::fail(
                    "intent_undefined_net",
                    format!("{}:{}", net.name, pin_ref),
                    format!(
                        "net '{}' references pin '{}' on undeclared part '{}'",
                        net.name, pin_ref, reference
                    ),
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(model)
}
