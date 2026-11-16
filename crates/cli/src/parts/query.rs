//! What the parts cache answers: search, show, fits, stats, check-board, status and plain SQL. Every answer is JSON (`eda parts` prints it, `--table`
//! as text; `GET /api/parts/*` sends it).
//!
//! A query names libraries by the board's nicknames: they are resolved through the board's tables (`library_tables::libs`) to paths, and each path's row is
//! compared with the library's shallow stamp first. A library whose row is not current is `stale` in the answer: the CLI reads it again before it answers
//! (`cmd`), the studio queues it for its indexer thread and answers with what it has. Nothing here runs kicad-cli.

use super::refresh::shallow_stamp;
use super::store::{LibRow, Store};
use super::kind_name;
use crate::library_index::{Kind, LibSource};
use crate::library_search::{natural_cmp, parse_query, score_item, Filter};
use eda_kicad::{FootprintSummary, SymbolSummary};
use rusqlite::{params_from_iter, types::Value as SqlValue};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One library of the board as a query sees it.
#[derive(Debug, Clone)]
pub(crate) struct Lib {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) row: Option<LibRow>,
    /// The row is there and its stamp is the library's.
    pub(crate) fresh: bool,
}

/// The libraries of `kind` the board's choosers list (hidden ones left out), in the trees' order, each with its row.
pub(crate) fn listed_libs(store: &Store, set: &dyn LibSource, kind: Kind) -> Vec<Lib> {
    set.listed(kind).iter().filter_map(|info| lib_of(store, set, kind, &info.name)).collect()
}

/// Library `name` of the board (a hidden one too: a placed part still resolves through it).
pub(crate) fn lib_of(store: &Store, set: &dyn LibSource, kind: Kind, name: &str) -> Option<Lib> {
    // a hidden library too: `LibSet::lib_path` finds every row
    let path = set.lib_path(kind, name).ok()?;
    let row = store.library(kind, &path);
    let fresh = row.as_ref().is_some_and(|r| r.stamp.is_some() && r.stamp == shallow_stamp(kind, &path));
    Some(Lib { name: name.to_string(), path, row, fresh })
}

pub(crate) fn stale_names(libs: &[Lib]) -> Vec<String> {
    libs.iter().filter(|l| !l.fresh).map(|l| l.name.clone()).collect()
}

fn sql_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// A word the trigram index can prefilter by: three characters or more, plain ASCII, none of the characters a regular expression, a wildcard, a relation or
/// `Lib:Name` gives a meaning (those words are scored, never prefiltered).
fn prefilter_word(word: &str) -> Option<String> {
    let w = word.to_lowercase();
    (w.chars().count() >= 3 && w.chars().all(|c| c.is_ascii_graphic() && !"*?/^$<>=:\"()[]{}|\\.+".contains(c))).then_some(w)
}

/// The search's options.
#[derive(Debug, Clone, Default)]
pub(crate) struct SearchOpts {
    pub(crate) query: String,
    /// Only this library (a nickname).
    pub(crate) lib: Option<String>,
    /// Symbols: exactly this many pins; footprints: this many numbered pads (the footprint chooser's "Filter by pin count").
    pub(crate) pins: Option<usize>,
    pub(crate) smd: bool,
    pub(crate) tht: bool,
    /// Footprints whose pitch is this, to the hundredth of a millimetre.
    pub(crate) pitch_mm: Option<f64>,
    /// Footprints whose 3D model is found.
    pub(crate) has_3d: bool,
    /// Footprint filter patterns (`SOIC*3.9x4.9mm*`), as a symbol's `ki_fp_filters` apply.
    pub(crate) fp_filters: Vec<String>,
    pub(crate) limit: usize,
}

/// One symbol row as the chooser's index keeps it.
pub(crate) fn symbol_summary_of(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<SymbolSummary> {
    let fields: String = r.get(at + 13)?;
    let numbers: String = r.get(at + 12)?;
    Ok(SymbolSummary {
        name: r.get(at)?,
        extends: r.get(at + 1)?,
        reference: r.get(at + 2)?,
        value: r.get(at + 3)?,
        footprint: r.get(at + 4)?,
        datasheet: r.get(at + 5)?,
        description: r.get(at + 6)?,
        keywords: r.get(at + 7)?,
        fp_filters: r.get(at + 8)?,
        power: r.get(at + 9)?,
        units: r.get(at + 10)?,
        alternate_body_style: r.get(at + 11)?,
        pin_numbers: if numbers.is_empty() { Vec::new() } else { numbers.split('\n').map(str::to_string).collect() },
        user_fields: serde_json::from_str(&fields).unwrap_or_default(),
        span: r.get::<_, i64>(at + 14)? as usize..r.get::<_, i64>(at + 15)? as usize,
    })
}

/// The columns [`symbol_summary_of`] reads, in its order.
pub(crate) const SYMBOL_COLUMNS: &str =
    "s.name, s.extends, s.reference, s.value, s.footprint, s.datasheet, s.description, s.keywords, s.fp_filters, s.power, s.units, s.alternate_body_style, s.pin_numbers, s.user_fields_json, s.span_start, s.span_end";

/// One footprint row's chooser summary and the rest of what a search shows of it.
#[derive(Debug, Clone, Default)]
pub(crate) struct FootprintRow {
    pub(crate) name: String,
    pub(crate) summary: FootprintSummary,
    pub(crate) attr: String,
    pub(crate) pitch_nm: i64,
    pub(crate) courtyard: Option<(i64, i64)>,
    pub(crate) model_found: bool,
    pub(crate) models: Vec<String>,
}

pub(crate) const FOOTPRINT_COLUMNS: &str = "f.name, f.description, f.tags, f.numbered_pads, f.pads, f.attr, f.pitch_nm, f.courtyard_w_nm, f.courtyard_h_nm, f.has_courtyard, f.model_found, f.model_paths_json";

pub(crate) fn footprint_row_of(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<FootprintRow> {
    let models: String = r.get(at + 11)?;
    Ok(FootprintRow {
        name: r.get(at)?,
        summary: FootprintSummary { description: r.get(at + 1)?, tags: r.get(at + 2)?, numbered_pads: r.get(at + 3)?, pads: r.get(at + 4)? },
        attr: r.get(at + 5)?,
        pitch_nm: r.get(at + 6)?,
        courtyard: r.get::<_, bool>(at + 9)?.then(|| Ok::<_, rusqlite::Error>((r.get(at + 7)?, r.get(at + 8)?))).transpose()?,
        model_found: r.get(at + 10)?,
        models: serde_json::from_str(&models).unwrap_or_default(),
    })
}

fn mm(nm: i64) -> f64 {
    (nm as f64 / 1e6 * 1e4).round() / 1e4
}

pub(crate) fn symbol_json(lib: &str, s: &SymbolSummary, score: Option<u32>) -> Value {
    let mut v = json!({
        "id": format!("{lib}:{}", s.name), "lib": lib, "name": s.name, "description": s.description, "keywords": s.keywords, "value": s.value,
        "reference": s.reference, "footprint": s.footprint, "datasheet": s.datasheet, "fp_filters": s.fp_filters, "pins": s.pin_count(), "units": s.unit_count(),
        "power": s.power, "extends": s.extends,
    });
    if !s.user_fields.is_empty() {
        v["fields"] = Value::Object(s.user_fields.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>());
    }
    if let Some(score) = score {
        v["score"] = json!(score);
    }
    v
}

pub(crate) fn footprint_json(lib: &str, f: &FootprintRow, score: Option<u32>) -> Value {
    let mut v = json!({
        "id": format!("{lib}:{}", f.name), "lib": lib, "name": f.name, "description": f.summary.description, "tags": f.summary.tags,
        "pads": f.summary.numbered_pads, "all_pads": f.summary.pads, "attr": f.attr, "pitch_mm": mm(f.pitch_nm),
        "courtyard_mm": f.courtyard.map(|(w, h)| json!([mm(w), mm(h)])), "models": f.models, "model_found": f.model_found,
    });
    if let Some(score) = score {
        v["score"] = json!(score);
    }
    v
}

/// The rows of the libraries `libs` (their row ids) whose terms might hold every prefilterable word of the query, as an SQL condition on `alias.id` and its
/// parameters: a word is in the item's indexed text (the trigram index of `fts`), or in the nickname of its library (a term too). `None`: no word to
/// prefilter by, every item is scored.
fn prefilter(words: &[String], libs: &[(usize, &Lib)], fts: &str, alias: &str) -> Option<(String, Vec<SqlValue>)> {
    if words.is_empty() {
        return None;
    }
    let mut sql = Vec::new();
    let mut args: Vec<SqlValue> = Vec::new();
    for w in words {
        let by_name: Vec<i64> = libs.iter().filter(|(_, l)| l.name.to_lowercase().contains(w.as_str())).filter_map(|(_, l)| l.row.as_ref().map(|r| r.id)).collect();
        args.push(SqlValue::Text(format!("\"{w}\"")));
        sql.push(format!("({alias}.id IN (SELECT rowid FROM {fts} WHERE {fts} MATCH ?{}) OR {alias}.library_id IN ({}))", args.len(), sql_list(&by_name)));
    }
    Some((sql.join(" AND "), args))
}

/// `eda parts search`, `GET /api/parts/search`: the items of the board's listed libraries that match the query, scored as KiCad's chooser scores them
/// (`library_search::score_item` over the chooser's terms: `LIB_SYMBOL::cacheSearchTerms` / `FOOTPRINT_INFO::GetSearchTerms`, with the Description column),
/// best first, ties in the trees' order -- the order `GET /api/library/search` gives -- narrowed by the options. Items KiCad's parser refuses are left out,
/// as the choosers leave them out.
pub(crate) fn search(store: &Store, set: &dyn LibSource, kind: Kind, opts: &SearchOpts) -> Value {
    let mut libs = listed_libs(store, set, kind);
    if let Some(only) = &opts.lib {
        libs.retain(|l| &l.name == only);
    }
    let matchers = parse_query(&opts.query);
    let words: Vec<String> = opts.query.split_whitespace().take(100).filter_map(prefilter_word).collect();
    let indexed: Vec<(usize, &Lib)> = libs.iter().enumerate().filter(|(_, l)| l.row.is_some()).collect();
    let by_id: HashMap<i64, usize> = indexed.iter().map(|(i, l)| (l.row.as_ref().expect("indexed").id, *i)).collect();
    let ids: Vec<i64> = by_id.keys().copied().collect();
    // (lib index, item, score, the item's json)
    let mut hits: Vec<(usize, String, u32, Value)> = Vec::new();
    match kind {
        Kind::Symbol => {
            let shown = Filter::default();
            let shown = shown.shown_columns();
            let (cond, args) = prefilter(&words, &indexed, "symbols_fts", "s").unwrap_or_else(|| ("1".into(), Vec::new()));
            let sql = format!("SELECT s.library_id, {SYMBOL_COLUMNS} FROM symbols s WHERE s.kicad_refused = 0 AND s.library_id IN ({}) AND {cond}", sql_list(&ids));
            if let Ok(mut st) = store.conn.prepare(&sql) {
                let rows = st.query_map(params_from_iter(args.iter()), |r| Ok((r.get::<_, i64>(0)?, symbol_summary_of(r, 1)?)));
                for (lid, s) in rows.into_iter().flatten().flatten() {
                    let li = by_id[&lid];
                    if opts.pins.is_some_and(|n| s.pin_count() != n) {
                        continue;
                    }
                    let lib = &libs[li].name;
                    let mut terms = crate::library_search::symbol_terms(lib, &s);
                    terms.extend(crate::library_search::column_terms(&s, &shown));
                    let score = score_item(&matchers, &terms);
                    if score > 0 {
                        hits.push((li, s.name.clone(), score, symbol_json(lib, &s, Some(score))));
                    }
                }
            }
        }
        Kind::Footprint => {
            let filter = Filter { pins: opts.pins, fp_filters: opts.fp_filters.iter().map(|p| p.to_lowercase()).collect(), ..Default::default() };
            let (cond, args) = prefilter(&words, &indexed, "footprints_fts", "f").unwrap_or_else(|| ("1".into(), Vec::new()));
            let sql = format!("SELECT f.library_id, {FOOTPRINT_COLUMNS} FROM footprints f WHERE f.kicad_refused = 0 AND f.library_id IN ({}) AND {cond}", sql_list(&ids));
            if let Ok(mut st) = store.conn.prepare(&sql) {
                let rows = st.query_map(params_from_iter(args.iter()), |r| Ok((r.get::<_, i64>(0)?, footprint_row_of(r, 1)?)));
                for (lid, f) in rows.into_iter().flatten().flatten() {
                    let li = by_id[&lid];
                    let lib = &libs[li].name;
                    if !filter.footprint_passes(lib, &f.name, f.summary.numbered_pads)
                        || (opts.smd && f.attr != "smd")
                        || (opts.tht && f.attr != "through_hole")
                        || (opts.has_3d && !f.model_found)
                        || opts.pitch_mm.is_some_and(|p| (f.pitch_nm - (p * 1e6).round() as i64).abs() > 5_000 || f.pitch_nm == 0)
                    {
                        continue;
                    }
                    let score = score_item(&matchers, &crate::library_search::footprint_terms(lib, &f.name, &f.summary));
                    if score > 0 {
                        hits.push((li, f.name.clone(), score, footprint_json(lib, &f, Some(score))));
                    }
                }
            }
        }
    }
    let matches = hits.len();
    hits.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then_with(|| natural_cmp(&a.1, &b.1)).then_with(|| a.1.cmp(&b.1)));
    let limit = if opts.limit == 0 { 50 } else { opts.limit };
    let truncated = hits.len() > limit;
    hits.truncate(limit);
    json!({
        "kind": kind_name(kind),
        "query": opts.query,
        "matches": matches,
        "truncated": truncated,
        "stale": stale_names(&libs),
        "unindexed": libs.iter().filter(|l| l.row.is_none()).map(|l| l.name.clone()).collect::<Vec<_>>(),
        "hits": hits.into_iter().map(|h| h.3).collect::<Vec<_>>(),
    })
}

/// A symbol of the board's libraries by `Lib:Name`: its row id and summary.
fn find_symbol(store: &Store, set: &dyn LibSource, id: &str) -> Result<(Lib, i64, SymbolSummary), String> {
    let (lib, name) = id.split_once(':').ok_or_else(|| format!("{id:?} is not a Lib:Name"))?;
    let l = lib_of(store, set, Kind::Symbol, lib).ok_or_else(|| format!("no symbol library {lib:?} in the board's tables"))?;
    let row = l.row.as_ref().ok_or_else(|| format!("the symbol library {lib:?} is not in the parts cache yet (eda parts refresh)"))?;
    let found = store.conn.query_row(&format!("SELECT s.id, {SYMBOL_COLUMNS} FROM symbols s WHERE s.library_id = ?1 AND s.name = ?2"), rusqlite::params![row.id, name], |r| {
        Ok((r.get::<_, i64>(0)?, symbol_summary_of(r, 1)?))
    });
    match found {
        Ok((sid, s)) => Ok((l, sid, s)),
        Err(_) => Err(format!("no symbol {id:?} in the parts cache")),
    }
}

fn find_footprint(store: &Store, set: &dyn LibSource, id: &str) -> Result<(Lib, i64, FootprintRow), String> {
    let (lib, name) = id.split_once(':').ok_or_else(|| format!("{id:?} is not a Lib:Name"))?;
    let l = lib_of(store, set, Kind::Footprint, lib).ok_or_else(|| format!("no footprint library {lib:?} in the board's tables"))?;
    let row = l.row.as_ref().ok_or_else(|| format!("the footprint library {lib:?} is not in the parts cache yet (eda parts refresh)"))?;
    let found = store.conn.query_row(&format!("SELECT f.id, {FOOTPRINT_COLUMNS} FROM footprints f WHERE f.library_id = ?1 AND f.name = ?2"), rusqlite::params![row.id, name], |r| {
        Ok((r.get::<_, i64>(0)?, footprint_row_of(r, 1)?))
    });
    match found {
        Ok((fid, f)) => Ok((l, fid, f)),
        Err(_) => Err(format!("no footprint {id:?} in the parts cache")),
    }
}

fn symbol_pins(store: &Store, sid: i64) -> Vec<Value> {
    let Ok(mut st) = store.conn.prepare_cached("SELECT unit, body_style, number, name, electrical_type, shape, hidden FROM symbol_pins WHERE symbol_id = ?1 ORDER BY rowid") else { return Vec::new() };
    st.query_map([sid], |r| {
        Ok(json!({ "unit": r.get::<_, i64>(0)?, "body_style": r.get::<_, i64>(1)?, "number": r.get::<_, String>(2)?, "name": r.get::<_, String>(3)?, "type": r.get::<_, String>(4)?, "shape": r.get::<_, String>(5)?, "hidden": r.get::<_, bool>(6)? }))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

fn footprint_pads(store: &Store, fid: i64) -> Vec<Value> {
    let Ok(mut st) = store.conn.prepare_cached("SELECT number, type, shape, x, y, rot, w, h, drill FROM footprint_pads WHERE footprint_id = ?1 ORDER BY rowid") else { return Vec::new() };
    st.query_map([fid], |r| {
        Ok(json!({
            "number": r.get::<_, String>(0)?, "type": r.get::<_, String>(1)?, "shape": r.get::<_, String>(2)?, "x_mm": mm(r.get(3)?), "y_mm": mm(r.get(4)?),
            "rot": r.get::<_, f64>(5)?, "w_mm": mm(r.get(6)?), "h_mm": mm(r.get(7)?), "drill_mm": mm(r.get(8)?),
        }))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// The pad numbers a schematic pin can land on: copper pads, not holes, with a number.
fn pad_numbers(store: &Store, fid: i64) -> HashSet<String> {
    let Ok(mut st) = store.conn.prepare_cached("SELECT number FROM footprint_pads WHERE footprint_id = ?1 AND type != 'np_thru_hole' AND number != ''") else { return HashSet::new() };
    st.query_map([fid], |r| r.get::<_, String>(0)).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default()
}

/// `eda parts show Lib:Name`, `GET /api/parts/show`: everything the cache holds of one symbol (its pins) or footprint (its pads), and with `definition` the
/// editable definition read from the library now -- cut out of its text by the span the cache keeps, once the library's stamp is checked to be the one the
/// span is for.
pub(crate) fn show(store: &Store, set: &dyn LibSource, kind: Kind, id: &str, definition: bool) -> Value {
    match kind {
        Kind::Symbol => match find_symbol(store, set, id) {
            Err(e) => json!({ "error": e }),
            Ok((lib, sid, s)) => {
                let mut v = symbol_json(&lib.name, &s, None);
                v["pin_numbers"] = json!(s.pin_numbers);
                v["alternate_body_style"] = json!(s.alternate_body_style);
                v["pin_list"] = json!(symbol_pins(store, sid));
                v["stale"] = json!(!lib.fresh);
                if definition {
                    v["definition"] = symbol_definition(store, &lib, &s).unwrap_or_else(|e| json!({ "error": e }));
                }
                v
            }
        },
        Kind::Footprint => match find_footprint(store, set, id) {
            Err(e) => json!({ "error": e }),
            Ok((lib, fid, f)) => {
                let mut v = footprint_json(&lib.name, &f, None);
                v["pad_list"] = json!(footprint_pads(store, fid));
                v["stale"] = json!(!lib.fresh);
                if definition {
                    let file = lib.path.join(format!("{}.kicad_mod", f.name));
                    v["definition"] = std::fs::read_to_string(&file)
                        .map_err(|e| format!("{}: {e}", file.display()))
                        .and_then(|t| eda_kicad::parse_library_footprint(&t).map_err(|e| format!("{}: {e}", file.display())))
                        .map(|p| json!(p.footprint))
                        .unwrap_or_else(|e| json!({ "error": e }));
                }
                v
            }
        },
    }
}

/// The symbol's definition, cut out of its library's text by the spans the cache keeps (its own and its bases').
fn symbol_definition(store: &Store, lib: &Lib, s: &SymbolSummary) -> Result<Value, String> {
    if !lib.fresh {
        return Err(format!("the library {} changed since it was indexed (eda parts refresh)", lib.name));
    }
    let row = lib.row.as_ref().ok_or("not indexed")?;
    let text = eda_kicad::read_symbol_library_text(&lib.path)?;
    // the chain of bases, as the summaries `parse_symbol` looks them up in
    let mut chain = vec![s.clone()];
    let mut at = s.extends.clone();
    while let Some(base) = at.take() {
        if chain.len() > 8 || chain.iter().any(|c| c.name == base) {
            break;
        }
        let found = store.conn.query_row(&format!("SELECT {SYMBOL_COLUMNS} FROM symbols s WHERE s.library_id = ?1 AND s.name = ?2"), rusqlite::params![row.id, base], |r| symbol_summary_of(r, 0));
        if let Ok(b) = found {
            at = b.extends.clone();
            chain.push(b);
        }
    }
    let mut sym = eda_kicad::parse_symbol_checked(&text, &chain, &s.name)?.ok_or("the symbol's text is not where the cache says")?;
    sym.lib_id = format!("{}:{}", lib.name, s.name);
    Ok(json!(sym))
}

/// The footprints `Filter` passes, in the order of the footprint selector (`library_search::footprint_select_in`): the listed libraries in the trees' order,
/// each one's footprints by name, at most `limit`.
fn select_footprints(store: &Store, libs: &[Lib], filter: &Filter, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    for l in libs {
        let Some(row) = &l.row else { continue };
        let Ok(mut st) = store.conn.prepare_cached("SELECT name, numbered_pads FROM footprints WHERE library_id = ?1 AND kicad_refused = 0") else { continue };
        let mut names: Vec<(String, u32)> = st.query_map([row.id], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default();
        names.sort_by(|a, b| natural_cmp(&a.0, &b.0).then_with(|| a.0.cmp(&b.0)));
        for (n, pads) in names {
            if filter.footprint_passes(&l.name, &n, pads) {
                out.push(format!("{}:{n}", l.name));
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out
}

/// `eda parts fits SYMBOL [--footprint FP]`, `GET /api/parts/fits`: the footprints the Symbol Chooser's footprint selector lists for the symbol (its pin count
/// of numbered pads and a name one of its footprint filters matches whole: `FOOTPRINT_SELECT_WIDGET` -> pcbnew's `filterFootprints`, the rule of
/// `GET /api/library/fp_select`), at most 400; with a footprint, whether that one fits and pin by pin how its pads meet the symbol's pins.
pub(crate) fn fits(store: &Store, sym_set: &dyn LibSource, fp_set: &dyn LibSource, symbol: &str, footprint: Option<&str>) -> Value {
    let (lib, _, s) = match find_symbol(store, sym_set, symbol) {
        Ok(found) => found,
        Err(e) => return json!({ "error": e }),
    };
    let filter = Filter::parse(&s.pin_count().to_string(), &s.fp_filters, "");
    let zero = s.fp_filters.trim().is_empty();
    let mut v = json!({ "symbol": format!("{}:{}", lib.name, s.name), "pins": s.pin_count(), "fp_filters": s.fp_filters, "default_footprint": s.footprint });
    let fp_libs = listed_libs(store, fp_set, Kind::Footprint);
    if let Some(fp) = footprint {
        match find_footprint(store, fp_set, fp) {
            Err(e) => v["error"] = json!(e),
            Ok((flib, fid, f)) => {
                let pads = pad_numbers(store, fid);
                let pins: HashSet<String> = s.pin_numbers.iter().cloned().collect();
                let mut missing: Vec<&String> = pins.iter().filter(|p| !pads.contains(*p)).collect();
                let mut unused: Vec<&String> = pads.iter().filter(|p| !pins.contains(*p)).collect();
                missing.sort_by(|a, b| natural_cmp(a, b));
                unused.sort_by(|a, b| natural_cmp(a, b));
                let count_ok = filter.pins.is_none_or(|n| n == f.summary.numbered_pads as usize);
                let filter_ok = Filter { pins: None, ..filter.clone() }.footprint_passes(&flib.name, &f.name, f.summary.numbered_pads);
                v["footprint"] = json!(format!("{}:{}", flib.name, f.name));
                v["pad_count"] = json!(f.summary.numbered_pads);
                v["pin_count_matches"] = json!(count_ok);
                v["filter_matches"] = json!(filter_ok);
                v["selector_lists_it"] = json!(count_ok && filter_ok && !(zero && filter.pins.is_none()));
                v["pins_without_pad"] = json!(missing);
                v["pads_without_pin"] = json!(unused);
                v["fits"] = json!(count_ok && filter_ok && missing.is_empty());
            }
        }
        v["stale"] = json!(stale_names(&fp_libs));
        return v;
    }
    let list = if zero && filter.pins.is_none() { Vec::new() } else { select_footprints(store, &fp_libs, &filter, crate::library_search::SELECT_LIMIT) };
    v["footprints"] = json!(list);
    v["stale"] = json!(stale_names(&fp_libs));
    v
}

fn count(store: &Store, sql: &str) -> i64 {
    store.conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

/// `eda parts stats [--by-lib]`, `GET /api/parts/stats`: the health of the libraries -- how many symbols, pins, footprints and pads, which footprints have no
/// courtyard or no 3D model found, what KiCad's parser refuses, which libraries load with errors, which verdicts are not known.
pub(crate) fn stats(store: &Store, by_lib: bool) -> Value {
    let size = std::fs::metadata(&store.path).map_or(0, |m| m.len()) + std::fs::metadata(store.path.with_extension("sqlite-wal")).map_or(0, |m| m.len());
    let mut v = json!({
        "db": store.path.display().to_string(),
        "db_bytes": size,
        "generation": store.generation(),
        "libraries": { "symbol": count(store, "SELECT count(*) FROM libraries WHERE kind = 'symbol'"), "footprint": count(store, "SELECT count(*) FROM libraries WHERE kind = 'footprint'") },
        "symbols": count(store, "SELECT count(*) FROM symbols"),
        "derived_symbols": count(store, "SELECT count(*) FROM symbols WHERE extends IS NOT NULL"),
        "power_symbols": count(store, "SELECT count(*) FROM symbols WHERE power"),
        "symbol_pins": count(store, "SELECT count(*) FROM symbol_pins"),
        "symbols_without_footprint_filters": count(store, "SELECT count(*) FROM symbols WHERE fp_filters = '' AND NOT power"),
        "footprints": count(store, "SELECT count(*) FROM footprints"),
        "footprint_pads": count(store, "SELECT count(*) FROM footprint_pads"),
        "footprints_without_courtyard": count(store, "SELECT count(*) FROM footprints WHERE NOT has_courtyard"),
        "footprints_without_model": count(store, "SELECT count(*) FROM footprints WHERE model_paths_json = '[]'"),
        "footprints_model_not_found": count(store, "SELECT count(*) FROM footprints WHERE model_paths_json != '[]' AND NOT model_found"),
        "footprints_reader_refuses": count(store, "SELECT count(*) FROM footprints WHERE reader_error IS NOT NULL"),
        "kicad_refused": { "symbols": count(store, "SELECT count(*) FROM symbols WHERE kicad_refused"), "footprints": count(store, "SELECT count(*) FROM footprints WHERE kicad_refused") },
        "verdicts": {
            "ok": count(store, "SELECT count(*) FROM libraries WHERE verdict_state = 'ok'"),
            "some_refused": count(store, "SELECT count(*) FROM libraries WHERE verdict_state = 'some_refused'"),
            "unknown": count(store, "SELECT count(*) FROM libraries WHERE verdict_state = 'unknown'"),
        },
    });
    let errors: Vec<Value> = store
        .libraries()
        .into_iter()
        .filter_map(|r| r.verdict_message.clone().or(r.load_error.clone()).map(|e| json!({ "kind": r.kind, "lib": r.nickname, "path": r.path, "error": e })))
        .collect();
    v["load_errors"] = json!(errors);
    if by_lib {
        let mut libs = Vec::new();
        for r in store.libraries() {
            let (table, extra) = if r.kind == "symbol" {
                ("symbols", "(SELECT count(*) FROM symbol_pins p JOIN symbols s ON s.id = p.symbol_id WHERE s.library_id = l.id)")
            } else {
                ("footprints", "(SELECT count(*) FROM footprints f WHERE f.library_id = l.id AND NOT f.has_courtyard)")
            };
            let (items, extra_n, refused): (i64, i64, i64) = store
                .conn
                .query_row(&format!("SELECT (SELECT count(*) FROM {table} WHERE library_id = l.id), {extra}, (SELECT count(*) FROM {table} WHERE library_id = l.id AND kicad_refused) FROM libraries l WHERE l.id = ?1"), [r.id], |x| {
                    Ok((x.get(0)?, x.get(1)?, x.get(2)?))
                })
                .unwrap_or((0, 0, 0));
            debug_assert_eq!(items, r.item_count);
            let mut row = json!({ "kind": r.kind, "lib": r.nickname, "path": r.path, "format": r.format, "items": r.item_count, "kicad_refused": refused, "verdict": r.verdict_state, "indexed_at": r.indexed_at });
            row[if r.kind == "symbol" { "pins" } else { "without_courtyard" }] = json!(extra_n);
            libs.push(row);
        }
        v["by_lib"] = json!(libs);
    }
    v
}

/// The board's design and parts as its intent names them -- `board::build_model`'s reading without its library resolution (which parses whole libraries: the
/// cache answers for them here).
fn board_parts(dir: &Path) -> Result<(eda_model::ir::Design, eda_model::ConstraintModel), String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let meta: crate::board::Meta = serde_json::from_str(&read(&dir.join("board.json"))?).map_err(|e| format!("board.json: {e}"))?;
    let design: eda_model::ir::Design = serde_json::from_str(&read(&dir.join("design.json"))?).map_err(|e| format!("design.json: {e}"))?;
    let intent = crate::board::intent_path(dir, &meta, &design);
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&read(Path::new(&intent))?).map_err(|e| format!("{intent}: {e}"))?;
    eda_model::fp_assign::apply_part_renames(&design, &mut model);
    Ok((design, model))
}

/// `eda parts check-board -C dir`, `GET /api/parts/check`: for every part of the board, whether its symbol and its footprint are in the board's libraries, whether
/// the symbol's pins all land on a pad of the footprint, and whether the footprint's 3D model is found. A part drawn with a generated symbol (`eda:..`) has no
/// library symbol to check.
pub(crate) fn check_board(store: &Store, dir: &Path, sym_set: &dyn LibSource, fp_set: &dyn LibSource) -> Value {
    let (design, model) = match board_parts(dir) {
        Ok(b) => b,
        Err(e) => return json!({ "error": e }),
    };
    let placed: HashMap<&str, &str> = design.schematic.as_ref().map(|s| s.symbols.iter().filter(|i| !i.lib_id.is_empty()).map(|i| (i.id.as_str(), i.lib_id.as_str())).collect()).unwrap_or_default();
    let (mut parts, mut problems) = (Vec::new(), 0usize);
    for p in &model.parts {
        let lib_id = placed.get(p.reference.as_str()).map(|s| s.to_string()).unwrap_or_else(|| eda_model::symbol::resolve_lib_id(p));
        let mut issues: Vec<String> = Vec::new();
        let generated = !lib_id.contains(':') || lib_id.starts_with("eda:") || lib_id.starts_with(eda_model::gensym::GENERATED_PREFIX);
        let sym = if generated { None } else { find_symbol(store, sym_set, &lib_id).map_err(|e| issues.push(e)).ok() };
        // the part's footprint as named (`Lib:Name`), else the KiCad footprint its package stands for (`kicad_footprint_of_part`, what the board places)
        let footprint = p.footprint.clone().filter(|f| f.contains(':')).or_else(|| eda_model::footprint::kicad_footprint_of_part(p).map(|f| f.id()));
        let fp = match footprint.as_deref().filter(|f| !f.is_empty()) {
            None => {
                issues.push("no footprint".into());
                None
            }
            Some(f) => find_footprint(store, fp_set, f).map_err(|e| issues.push(e)).ok(),
        };
        let mut row = json!({ "reference": p.reference, "symbol": lib_id, "footprint": footprint, "generated_symbol": generated });
        if let Some((_, fid, f)) = &fp {
            row["model_found"] = json!(f.model_found);
            if !f.models.is_empty() && !f.model_found {
                issues.push("3D model not found".into());
            }
            if f.models.is_empty() {
                issues.push("no 3D model".into());
            }
            let pads = pad_numbers(store, *fid);
            let pins: Vec<String> = match &sym {
                Some((_, _, s)) => s.pin_numbers.clone(),
                None => p.pins.iter().map(|pin| pin.number.clone()).collect(),
            };
            let mut missing: Vec<String> = pins.iter().filter(|n| !pads.contains(*n)).cloned().collect();
            missing.sort_by(|a, b| natural_cmp(a, b));
            missing.dedup();
            if !missing.is_empty() {
                issues.push(format!("pins without a pad: {}", missing.join(", ")));
            }
            row["pins_without_pad"] = json!(missing);
        }
        if !issues.is_empty() {
            problems += 1;
        }
        row["issues"] = json!(issues);
        parts.push(row);
    }
    let stale: Vec<String> = stale_names(&listed_libs(store, sym_set, Kind::Symbol)).into_iter().chain(stale_names(&listed_libs(store, fp_set, Kind::Footprint))).collect();
    json!({ "board": dir.display().to_string(), "parts": parts.len(), "with_issues": problems, "stale": stale, "checks": parts })
}

/// `eda parts status`, `GET /api/parts/status`: where the cache is, whether it is being built, and which of the board's libraries are not current.
pub(crate) fn status(store: &Store, sym_set: &dyn LibSource, fp_set: &dyn LibSource) -> Value {
    let sym = listed_libs(store, sym_set, Kind::Symbol);
    let fp = listed_libs(store, fp_set, Kind::Footprint);
    let stale: Vec<String> = sym.iter().chain(fp.iter()).filter(|l| !l.fresh).map(|l| format!("{}:{}", if sym.iter().any(|s| s.path == l.path) { "symbol" } else { "footprint" }, l.name)).collect();
    let unknown = store.libraries().iter().filter(|r| r.verdict_state == "unknown").count();
    json!({
        "db": store.path.display().to_string(),
        "schema_version": super::schema::SCHEMA_VERSION,
        "scanner_version": super::schema::SCANNER_VERSION,
        "generation": store.generation(),
        "building": store.lease_holder().map(|(owner, pid, age)| json!({ "owner": owner, "pid": pid, "heartbeat_age_s": age })),
        "libraries": { "symbol": sym.len(), "footprint": fp.len() },
        "indexed": { "symbol": sym.iter().filter(|l| l.fresh).count(), "footprint": fp.iter().filter(|l| l.fresh).count() },
        "stale": stale,
        "verdicts_unknown": unknown,
        "state": if stale.is_empty() { "ready" } else { "stale" },
    })
}

/// `eda parts sql "SELECT ..."`: the rows of a query on a read-only connection (`SQLITE_OPEN_READ_ONLY`: a statement that writes is refused by SQLite itself),
/// each an object of its columns; at most `limit` rows.
pub(crate) fn sql(store: &Store, query: &str, limit: usize) -> Value {
    let mut st = match store.conn.prepare(query) {
        Ok(st) => st,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    if !st.readonly() {
        return json!({ "error": "only statements that read are run here" });
    }
    let names: Vec<String> = st.column_names().into_iter().map(str::to_string).collect();
    let mut rows = match st.query([]) {
        Ok(r) => r,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let mut out = Vec::new();
    let mut truncated = false;
    loop {
        match rows.next() {
            Ok(Some(r)) => {
                if out.len() >= limit {
                    truncated = true;
                    break;
                }
                let mut obj = Map::new();
                for (i, n) in names.iter().enumerate() {
                    let v = match r.get_ref(i) {
                        Ok(rusqlite::types::ValueRef::Null) | Err(_) => Value::Null,
                        Ok(rusqlite::types::ValueRef::Integer(n)) => json!(n),
                        Ok(rusqlite::types::ValueRef::Real(f)) => json!(f),
                        Ok(rusqlite::types::ValueRef::Text(t)) => json!(String::from_utf8_lossy(t)),
                        Ok(rusqlite::types::ValueRef::Blob(b)) => json!(format!("<{} bytes>", b.len())),
                    };
                    obj.insert(n.clone(), v);
                }
                out.push(Value::Object(obj));
            }
            Ok(None) => break,
            Err(e) => return json!({ "error": e.to_string() }),
        }
    }
    json!({ "columns": names, "rows": out, "truncated": truncated })
}

#[cfg(test)]
mod tests {
    use super::super::refresh::tests::{fixture, temp};
    use super::super::refresh::{refresh_targets, ModelCtx, Target};
    use super::*;
    use crate::library_search::{footprint_select_in, index_all, search_in};

    /// A library root of a few of the installed libraries (links to them), with its targets: what the parity tests compare the choosers' answers on.
    /// `None` when KiCad's libraries are not installed.
    fn installed_subset(tag: &str) -> Option<(PathBuf, PathBuf, PathBuf, Vec<Target>)> {
        let (sym_src, fp_src) = (Kind::Symbol.root(), Kind::Footprint.root());
        if !sym_src.is_dir() || !fp_src.is_dir() {
            return None;
        }
        let dir = temp(tag);
        let (sym, fp) = (dir.join("symbols"), dir.join("footprints"));
        std::fs::create_dir_all(&sym).unwrap();
        std::fs::create_dir_all(&fp).unwrap();
        let mut targets = Vec::new();
        for name in ["Device", "Amplifier_Operational", "Regulator_Linear", "power", "Connector", "Diode", "Transistor_FET", "Interface_UART", "Timer", "MCU_ST_STM32F1", "LED", "Switch"] {
            let src = eda_kicad::find_symbol_library_file(&sym_src, name)?;
            let link = sym.join(src.file_name()?);
            std::os::unix::fs::symlink(&src, &link).ok()?;
            targets.push(Target { kind: Kind::Symbol, path: link, nickname: name.into(), format: "kicad" });
        }
        for name in ["Resistor_SMD", "Capacitor_SMD", "Package_SO", "Package_DIP", "Package_QFP", "Connector_PinHeader_2.54mm", "Diode_SMD", "LED_SMD", "Package_TO_SOT_SMD", "Crystal", "Resistor_THT", "Package_DFN_QFN"] {
            let src = fp_src.join(format!("{name}.pretty"));
            let link = fp.join(format!("{name}.pretty"));
            std::os::unix::fs::symlink(&src, &link).ok()?;
            targets.push(Target { kind: Kind::Footprint, path: link, nickname: name.into(), format: "kicad" });
        }
        Some((dir, sym, fp, targets))
    }

    /// `(lib, name, score)` of a `GET /api/library/search` answer, in its order (score, then library, then name, as it ranks them before it groups them).
    fn chooser_hits(v: &Value, libs: &[String]) -> Vec<(String, String, u64)> {
        let mut out: Vec<(String, String, u64)> = Vec::new();
        for l in v["libraries"].as_array().unwrap() {
            for it in l["items"].as_array().unwrap() {
                out.push((l["name"].as_str().unwrap().to_string(), it["name"].as_str().unwrap().to_string(), it["score"].as_u64().unwrap()));
            }
        }
        let pos = |l: &str| libs.iter().position(|x| x == l).unwrap();
        out.sort_by(|a, b| b.2.cmp(&a.2).then(pos(&a.0).cmp(&pos(&b.0))).then_with(|| natural_cmp(&a.1, &b.1)).then_with(|| a.1.cmp(&b.1)));
        out
    }

    #[test]
    fn the_cache_search_ranks_as_the_choosers_search_does() {
        let Some((dir, sym, fp, targets)) = installed_subset("parity-search") else {
            eprintln!("no installed KiCad libraries; skipping");
            return;
        };
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap();
        index_all(Kind::Symbol, &sym);
        index_all(Kind::Footprint, &fp);
        let queries = [
            "lm358", "opamp", "dual opamp", "resistor", "r_0603", "soic-8", "usb c", "regulator 3.3", "ldo", "0805", "dip-8", "led", "mosfet n", "*3.9x4.9*", "/^r_0[68]0/", "stm32", "crystal",
            "uart", "pinheader 1x04", "qfp-48", "schottky", "capacitor", "gnd", "vcc", "555", "ne555", "res", "c", "1x02", "device:r", "voltage>3", "AMS1117", "sot-23", "qfn",
        ];
        for kind in [Kind::Symbol, Kind::Footprint] {
            let root: &Path = if kind == Kind::Symbol { &sym } else { &fp };
            let libs: Vec<String> = root.listed(kind).iter().map(|l| l.name.clone()).collect();
            for q in queries {
                let theirs = search_in(kind, &root.to_path_buf(), q, &Filter::default(), 100_000);
                let theirs = chooser_hits(&theirs, &libs);
                let ours = search(&store, &root.to_path_buf(), kind, &SearchOpts { query: q.into(), limit: 100_000, ..Default::default() });
                assert_eq!(ours["stale"], json!([]), "{ours}");
                let ours: Vec<(String, String, u64)> =
                    ours["hits"].as_array().unwrap().iter().map(|h| (h["lib"].as_str().unwrap().to_string(), h["name"].as_str().unwrap().to_string(), h["score"].as_u64().unwrap())).collect();
                assert_eq!(ours.len(), theirs.len(), "{kind:?} {q:?}: {} vs {} hits", ours.len(), theirs.len());
                assert_eq!(ours, theirs, "{kind:?} {q:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fits_lists_what_the_symbol_choosers_footprint_selector_lists() {
        let Some((dir, sym, fp, targets)) = installed_subset("parity-fits") else {
            eprintln!("no installed KiCad libraries; skipping");
            return;
        };
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap();
        index_all(Kind::Footprint, &fp);
        let all: Vec<(String, String, usize, String)> = {
            let mut st = store.conn.prepare("SELECT l.nickname, s.name, s.pin_count, s.fp_filters FROM symbols s JOIN libraries l ON l.id = s.library_id ORDER BY s.id").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as usize, r.get(3)?))).unwrap().map(|r| r.unwrap()).collect()
        };
        let step = (all.len() / 200).max(1);
        let (mut compared, mut non_empty) = (0, 0);
        for (lib, name, pins, filters) in all.iter().step_by(step).take(200) {
            let filter = Filter::parse(&pins.to_string(), filters, "");
            let theirs = footprint_select_in(&fp, &filter, filters.trim().is_empty(), crate::library_search::SELECT_LIMIT);
            let ours = fits(&store, &sym, &fp, &format!("{lib}:{name}"), None);
            assert_eq!(ours["footprints"], theirs["footprints"], "{lib}:{name} ({pins} pins, {filters:?})");
            compared += 1;
            non_empty += usize::from(theirs["footprints"].as_array().is_some_and(|a| !a.is_empty()));
        }
        assert!(compared >= 150 && non_empty > 20, "{compared} compared, {non_empty} with footprints");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn show_fits_stats_and_sql_on_the_fixture() {
        let (dir, targets) = fixture("query");
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap();
        // the fixture's root lists `Pkg` (footprints) and `Lib`, `Dir` (symbols)
        let root = dir.clone();
        let lm = show(&store, &root, Kind::Symbol, "Lib:LM2904", true);
        assert_eq!((lm["pins"].clone(), lm["units"].clone(), lm["extends"].clone()), (json!(8), json!(3), json!("LM358")), "{lm}");
        let pin_list = lm["pin_list"].as_array().unwrap();
        assert_eq!(pin_list.len(), 8);
        assert!(pin_list.iter().any(|p| p["number"] == "8" && p["name"] == "V+" && p["type"] == "power_in"));
        assert_eq!(lm["definition"]["lib_id"], "Lib:LM2904", "the definition cut out by the cached spans: {}", lm["definition"]);
        assert_eq!(lm["definition"]["pins"].as_array().map(Vec::len), Some(8));
        let soic = show(&store, &root, Kind::Footprint, "Pkg:SOIC-8_3.9x4.9mm_P1.27mm", false);
        assert_eq!((soic["attr"].clone(), soic["pitch_mm"].clone(), soic["pads"].clone()), (json!("smd"), json!(1.27), json!(8)));
        assert_eq!(soic["courtyard_mm"], json!([7.4, 5.4]));
        assert_eq!(soic["pad_list"].as_array().unwrap().len(), 8);
        assert!(show(&store, &root, Kind::Symbol, "Lib:Nope", false)["error"].is_string());
        // fits: the selector's list, and one footprint pin by pin
        let f = fits(&store, &root, &root, "Lib:LM358", None);
        assert_eq!(f["footprints"], json!(["Pkg:DIP-8_W7.62mm", "Pkg:SOIC-8_3.9x4.9mm_P1.27mm"]), "8 pads and a filter each, by name; not the R_0603: {f}");
        let one = fits(&store, &root, &root, "Lib:R", Some("Pkg:R_0603_1608Metric"));
        assert_eq!((one["fits"].clone(), one["pin_count_matches"].clone(), one["filter_matches"].clone()), (json!(true), json!(true), json!(true)), "{one}");
        let bad = fits(&store, &root, &root, "Lib:R", Some("Pkg:SOIC-8_3.9x4.9mm_P1.27mm"));
        assert_eq!((bad["fits"].clone(), bad["pin_count_matches"].clone(), bad["pads_without_pin"].as_array().map(Vec::len)), (json!(false), json!(false), Some(6)));
        // stats and SQL
        let st = stats(&store, true);
        assert_eq!((st["symbols"].clone(), st["footprints"].clone(), st["footprint_pads"].clone()), (json!(5), json!(3), json!(18)), "{st}");
        assert_eq!(st["by_lib"].as_array().unwrap().len(), 3);
        let ro = Store::open_read_only(&store.path).unwrap();
        let rows = sql(&ro, "SELECT name, pitch_nm FROM footprints WHERE kicad_wildcard('dip*', name)", 10);
        assert_eq!(rows["rows"], json!([{ "name": "DIP-8_W7.62mm", "pitch_nm": 2_540_000 }]));
        assert!(sql(&ro, "DELETE FROM footprints", 10)["error"].is_string(), "a read-only cache");
        assert!(sql(&ro, "SELECT nope", 10)["error"].is_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_board_finds_each_parts_symbol_footprint_pins_and_model() {
        let (dir, targets) = fixture("check");
        let store = Store::open(&dir.join("p.sqlite")).unwrap();
        refresh_targets(&store, &targets, false, &ModelCtx::for_dir(&dir)).unwrap();
        let board = dir.join("board");
        std::fs::create_dir_all(&board).unwrap();
        let model: eda_model::ConstraintModel = serde_json::from_value(json!({ "parts": [
            { "reference": "U1", "symbol": "Lib:LM358", "footprint": "Pkg:SOIC-8_3.9x4.9mm_P1.27mm", "pins": [] },
            { "reference": "R1", "symbol": "Lib:R", "footprint": "Pkg:SOIC-8_3.9x4.9mm_P1.27mm", "pins": [] },
            { "reference": "R2", "symbol": "Lib:R", "footprint": "Pkg:R_0603_1608Metric", "pins": [] },
            { "reference": "X1", "symbol": "Lib:Gone", "footprint": "Nope:X", "pins": [] }
        ]}))
        .unwrap();
        let intent = board.join("intent.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        std::fs::write(board.join("board.json"), json!({ "intent": intent.display().to_string(), "snap_nm": 100_000, "spacing_nm": 300_000 }).to_string()).unwrap();
        std::fs::write(board.join("design.json"), r#"{"schema":2,"provenance":{"engine_version":"0","intent_hash":"x","seed":0,"stage_hashes":[]}}"#).unwrap();
        let v = check_board(&store, &board, &dir, &dir);
        let by = |r: &str| v["checks"].as_array().unwrap().iter().find(|c| c["reference"] == r).cloned().unwrap_or_else(|| panic!("{v}"));
        let u1 = by("U1");
        assert_eq!(u1["pins_without_pad"], json!([]), "{u1}");
        // the SOIC names KiCad's model: found where KiCad's 3D model library is installed
        let installed = crate::model3d_api::models_dir().join("Package_SO.3dshapes/SOIC-8_3.9x4.9mm_P1.27mm.step").is_file();
        assert_eq!(u1["model_found"], json!(installed), "{u1}");
        assert_eq!(u1["issues"].as_array().unwrap().iter().any(|i| i == "3D model not found"), !installed, "{u1}");
        assert_eq!(by("R1")["pins_without_pad"], json!([]), "pins 1 and 2 land on the SOIC's pads 1 and 2");
        assert_eq!(by("R2")["issues"], json!(["no 3D model"]));
        let x1 = by("X1")["issues"].clone();
        assert_eq!(x1.as_array().unwrap().len(), 2, "{x1}");
        assert_eq!(v["with_issues"], json!(if installed { 2 } else { 4 }), "R2 has no model, X1 nothing; U1 and R1 the SOIC's model: {v}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
