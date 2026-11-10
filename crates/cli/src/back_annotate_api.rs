//! `POST /api/back_annotate`: Update Schematic from PCB's nets (`DIALOG_UPDATE_FROM_PCB::updateData`, the dry run of `BACK_ANNOTATE` with "Net names", "Prefer symbol
//! unit swaps over label swaps" and "Prefer symbol pin swaps over label swaps"; KiCad 25ec2335 `eeschema/tools/backannotate.cpp`).
//!
//! The body is `eda_ops::back_annotate::BackAnnotateOptions` (`{ net_names, prefer_unit_swaps, prefer_pin_swaps, allow_pin_swaps }`, the last the schematic
//! preference "Allow unconstrained pin swaps"); the answer is `{ ok, lines: [{ severity, text }], cmds: [..] }`: the report and the verbs that do it, which the
//! dialog sends with its footprint changes as one batch (one undo step of both editors). The board's nets are the net list in force; the schematic's are what
//! the drawing traces to (`board::trace_kicad_names`) -- the stored schematic, else the one the studio shows for a board with none (which the batch's first
//! sheet edit stores, so the labels the verbs name are the ones shown).

use crate::board;
use eda_ops::back_annotate::{plan_net_back_annotation, BackAnnotateOptions, NetBackAnnotation};
use serde_json::{json, Value};
use std::path::Path;

/// The plan for the board at `dir` with `opts`.
pub(crate) fn plan(dir: &Path, opts: &BackAnnotateOptions) -> Result<NetBackAnnotation, String> {
    let (_, mut design, model) = board::load(dir).map_err(|e| board::reasons(&e))?;
    let board_nets = design.nets.clone().unwrap_or_else(|| model.nets.clone());
    if design.schematic.is_none() {
        let derived = board::derived_schematic(&design, &model).map_err(|e| board::reasons(&e))?;
        design.schematic = derived.schematic;
        design.sheet_contents = derived.sheet_contents;
    }
    let drawn = board::trace_kicad_names(&mut design.clone(), &model);
    Ok(plan_net_back_annotation(&design, &model, &board_nets, &drawn, opts))
}

pub fn back_annotate(dir: &Path, body: &[u8]) -> Value {
    let opts: BackAnnotateOptions = if body.is_empty() { BackAnnotateOptions::default() } else {
        match serde_json::from_slice(body) {
            Ok(o) => o,
            Err(e) => return json!({ "ok": false, "message": format!("the request is not the dialog's options: {e}") }),
        }
    };
    match plan(dir, &opts) {
        Ok(p) => json!({ "ok": true, "lines": p.lines, "cmds": p.cmds }),
        Err(message) => json!({ "ok": false, "message": message }),
    }
}

