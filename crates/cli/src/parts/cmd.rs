//! `eda parts ...`: the parts cache from the command line (`docs/PARTS-CACHE.md`). Every answer is JSON on stdout (`--table` prints it as text); errors go to
//! stderr with a failing exit code.
//!
//! The board's tables name the libraries (`-C <board dir>`, the current folder by default: KiCad's global tables, plus the board's own when there is one).
//! A query first compares each library's stamp and reads the ones that changed (or were never read) into the cache, then answers from the cache; the
//! first query on a machine builds it all. Only `refresh --verify` and `verify` run kicad-cli.

use super::query::{self, SearchOpts};
use super::refresh::{self, Target};
use super::store::Store;
use crate::library_index::Kind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: eda parts <command> [-C <board dir>] [--table]
  refresh [--kind symbol|footprint] [--deep] [--verify]   bring the cache up to date (deep: every file; verify: ask kicad-cli for KiCad's verdicts)
  status                                                  where the cache is, what is stale, who is building
  search QUERY [--kind symbol|footprint] [--lib L] [--pins N] [--smd|--tht] [--pitch MM] [--has-3d] [--fp-filter PAT].. [-n N]
                                                          KiCad's chooser search over every library, best first
  show Lib:Name [--kind symbol|footprint] [--definition]  one symbol (pins) or footprint (pads); --definition reads the item itself
  fits SYMBOL [--footprint Lib:Name]                      the footprints the Symbol Chooser's selector lists; or whether one fits, pin by pin
  stats [--by-lib]                                        counts and health: courtyards, 3D models, KiCad's refusals, load errors
  check-board                                             the board's parts: symbol and footprint found, pins on pads, 3D model found
  sql \"SELECT ...\" [-n N]                                 any read-only query (tables: docs/PARTS-CACHE.md)
  verify [--kind symbol|footprint]                        ask kicad-cli for KiCad's parser's verdict on every library not yet checked
The cache is $EDA_PARTS_DB, else ~/.cache/eda-studio/parts.sqlite.";

#[derive(Default)]
struct Args {
    dir: Option<PathBuf>,
    table: bool,
    kind: Option<Kind>,
    deep: bool,
    verify: bool,
    by_lib: bool,
    definition: bool,
    footprint: Option<String>,
    limit: Option<usize>,
    search: SearchOpts,
    positional: Vec<String>,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    let value = |it: &mut std::slice::Iter<'_, String>, flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-C" => a.dir = Some(PathBuf::from(value(&mut it, "-C")?)),
            "--table" => a.table = true,
            "--json" => a.table = false,
            "--kind" => a.kind = Some(Kind::parse(&value(&mut it, "--kind")?).ok_or("--kind is symbol or footprint")?),
            "--deep" => a.deep = true,
            "--verify" => a.verify = true,
            "--by-lib" => a.by_lib = true,
            "--definition" => a.definition = true,
            "--footprint" => a.footprint = Some(value(&mut it, "--footprint")?),
            "-n" | "--limit" => a.limit = Some(value(&mut it, "-n")?.parse().map_err(|_| "-n takes a number")?),
            "--lib" => a.search.lib = Some(value(&mut it, "--lib")?),
            "--pins" => a.search.pins = Some(value(&mut it, "--pins")?.parse().map_err(|_| "--pins takes a number")?),
            "--smd" => a.search.smd = true,
            "--tht" => a.search.tht = true,
            "--pitch" => a.search.pitch_mm = Some(value(&mut it, "--pitch")?.parse().map_err(|_| "--pitch takes millimetres")?),
            "--has-3d" => a.search.has_3d = true,
            "--fp-filter" => a.search.fp_filters.extend(value(&mut it, "--fp-filter")?.split_whitespace().map(str::to_string)),
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}")),
            s => a.positional.push(s.to_string()),
        }
    }
    Ok(a)
}

/// The cache for reading; built first when there is none.
fn reader(db: &Path, dir: &Path) -> Result<Store, String> {
    if let Ok(s) = Store::open_read_only(db) {
        return Ok(s);
    }
    eprintln!("parts: building the parts cache at {} (once; later queries read only what changed)", db.display());
    let store = Store::open(db)?;
    refresh::refresh(&store, dir, &[Kind::Symbol, Kind::Footprint], false)?;
    drop(store);
    Store::open_read_only(db)
}

/// Read again the libraries of `kinds` whose rows are not current, before a query answers from them.
fn catch_up(db: &Path, dir: &Path, kinds: &[Kind]) -> Result<(), String> {
    let ro = reader(db, dir)?;
    let targets: Vec<Target> = refresh::board_targets(dir, kinds);
    let stale: Vec<Target> = targets.into_iter().filter(|t| ro.library(t.kind, &t.path).and_then(|r| r.stamp) != refresh::shallow_stamp(t.kind, &t.path)).collect();
    if stale.is_empty() {
        return Ok(());
    }
    drop(ro);
    let store = Store::open(db)?;
    refresh::refresh_targets(&store, &stale, false, &refresh::ModelCtx::for_dir(dir))?;
    Ok(())
}

pub(crate) fn run(argv: &[String]) -> ExitCode {
    let Some(command) = argv.first() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if matches!(command.as_str(), "-h" | "--help" | "help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let args = match parse(&argv[1..]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match answer(command, &args) {
        Ok(v) => {
            if args.table {
                print!("{}", table(command, &v));
            } else {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            }
            if v.get("error").is_some() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn answer(command: &str, a: &Args) -> Result<Value, String> {
    let dir = a.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let db = super::db_path();
    let kinds: Vec<Kind> = a.kind.map_or_else(|| vec![Kind::Symbol, Kind::Footprint], |k| vec![k]);
    let sets = || (crate::library_tables::libs(&dir, Kind::Symbol), crate::library_tables::libs(&dir, Kind::Footprint));
    let first = || a.positional.first().cloned().ok_or_else(|| format!("{command} needs an argument\n{USAGE}"));
    match command {
        "refresh" => {
            let store = Store::open(&db)?;
            let report = refresh::refresh(&store, &dir, &kinds, a.deep)?;
            let mut v = report.json();
            v["db"] = json!(db.display().to_string());
            if a.verify {
                v["verify"] = verify(&store, &dir, &kinds);
            }
            Ok(v)
        }
        "verify" => {
            let store = Store::open(&db)?;
            refresh::refresh(&store, &dir, &kinds, false)?;
            Ok(verify(&store, &dir, &kinds))
        }
        "status" => {
            let store = Store::open_read_only(&db).or_else(|_| Store::open(&db))?;
            let (s, f) = sets();
            Ok(query::status(&store, &*s, &*f))
        }
        "search" => {
            let kind = a.kind.unwrap_or(Kind::Symbol);
            catch_up(&db, &dir, &[kind])?;
            let store = Store::open_read_only(&db)?;
            let set = crate::library_tables::libs(&dir, kind);
            let mut opts = a.search.clone();
            opts.query = a.positional.join(" ");
            opts.limit = a.limit.unwrap_or(50);
            Ok(query::search(&store, &*set, kind, &opts))
        }
        "show" => {
            let id = first()?;
            catch_up(&db, &dir, &kinds)?;
            let store = Store::open_read_only(&db)?;
            let (s, f) = sets();
            Ok(match a.kind {
                Some(Kind::Footprint) => query::show(&store, &*f, Kind::Footprint, &id, a.definition),
                Some(Kind::Symbol) => query::show(&store, &*s, Kind::Symbol, &id, a.definition),
                // a symbol by that name, else a footprint
                None => {
                    let v = query::show(&store, &*s, Kind::Symbol, &id, a.definition);
                    if v.get("error").is_some() {
                        let fp = query::show(&store, &*f, Kind::Footprint, &id, a.definition);
                        if fp.get("error").is_none() {
                            return Ok(fp);
                        }
                    }
                    v
                }
            })
        }
        "fits" => {
            let symbol = first()?;
            catch_up(&db, &dir, &[Kind::Symbol, Kind::Footprint])?;
            let store = Store::open_read_only(&db)?;
            let (s, f) = sets();
            Ok(query::fits(&store, &*s, &*f, &symbol, a.footprint.as_deref()))
        }
        "stats" => {
            let store = reader(&db, &dir)?;
            Ok(query::stats(&store, a.by_lib))
        }
        "check-board" => {
            catch_up(&db, &dir, &[Kind::Symbol, Kind::Footprint])?;
            let store = Store::open_read_only(&db)?;
            let (s, f) = sets();
            Ok(query::check_board(&store, &dir, &*s, &*f))
        }
        "sql" => {
            let q = a.positional.join(" ");
            if q.trim().is_empty() {
                return Err(format!("sql needs a query\n{USAGE}"));
            }
            let store = reader(&db, &dir)?;
            Ok(query::sql(&store, &q, a.limit.unwrap_or(1000)))
        }
        other => Err(format!("unknown command {other:?}\n{USAGE}")),
    }
}

/// KiCad's parser's verdict on the board's libraries, kicad-cli asked for each one not known (`library_verdict`): one at a time, a line on stderr for each.
fn verify(store: &Store, dir: &Path, kinds: &[Kind]) -> Value {
    let targets = refresh::board_targets(dir, kinds);
    let started = std::time::Instant::now();
    let (asked, answered) = refresh::verify_targets(store, &targets, true, |t| eprintln!("parts: verifying {} {}", super::kind_name(t.kind), t.nickname));
    let refused: i64 = store.conn.query_row("SELECT (SELECT count(*) FROM symbols WHERE kicad_refused) + (SELECT count(*) FROM footprints WHERE kicad_refused)", [], |r| r.get(0)).unwrap_or(0);
    json!({ "libraries": targets.len(), "asked": asked, "answered": answered, "items_refused": refused, "ms": started.elapsed().as_millis() as u64 })
}

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `--table`: the answer as lines of text.
fn table(command: &str, v: &Value) -> String {
    let mut out = String::new();
    if let Some(e) = v.get("error") {
        return format!("error: {}\n", s(e));
    }
    match command {
        "search" => {
            for h in v["hits"].as_array().into_iter().flatten() {
                let extra = if h.get("pitch_mm").is_some() {
                    format!("pads {:>3} {:<12} pitch {:<6} 3D {}", s(&h["pads"]), s(&h["attr"]), s(&h["pitch_mm"]), if h["model_found"] == json!(true) { "yes" } else { "no" })
                } else {
                    format!("pins {:>3} units {}", s(&h["pins"]), s(&h["units"]))
                };
                out.push_str(&format!("{:>5}  {:<50} {}  {}\n", s(&h["score"]), s(&h["id"]), extra, s(&h["description"])));
            }
            out.push_str(&format!("{} matches{}\n", s(&v["matches"]), if v["truncated"] == json!(true) { " (best shown)" } else { "" }));
        }
        "fits" => {
            if let Some(list) = v["footprints"].as_array() {
                for f in list {
                    out.push_str(&format!("{}\n", s(f)));
                }
                out.push_str(&format!("{} footprints for {} ({} pins, filters {:?})\n", list.len(), s(&v["symbol"]), s(&v["pins"]), s(&v["fp_filters"])));
            } else {
                for k in ["symbol", "footprint", "pins", "pad_count", "pin_count_matches", "filter_matches", "pins_without_pad", "pads_without_pin", "fits"] {
                    out.push_str(&format!("{k:<18} {}\n", s(&v[k])));
                }
            }
        }
        "check-board" => {
            for c in v["checks"].as_array().into_iter().flatten() {
                let issues: Vec<String> = c["issues"].as_array().into_iter().flatten().map(s).collect();
                out.push_str(&format!("{:<8} {:<40} {:<50} {}\n", s(&c["reference"]), s(&c["symbol"]), s(&c["footprint"]), if issues.is_empty() { "ok".into() } else { issues.join("; ") }));
            }
            out.push_str(&format!("{} parts, {} with issues\n", s(&v["parts"]), s(&v["with_issues"])));
        }
        "sql" => {
            let cols: Vec<String> = v["columns"].as_array().into_iter().flatten().map(s).collect();
            out.push_str(&format!("{}\n", cols.join("\t")));
            for r in v["rows"].as_array().into_iter().flatten() {
                out.push_str(&format!("{}\n", cols.iter().map(|c| s(&r[c])).collect::<Vec<_>>().join("\t")));
            }
        }
        _ => {
            if let Some(obj) = v.as_object() {
                for (k, x) in obj {
                    match x {
                        Value::Array(a) if a.iter().all(|e| e.is_object()) && !a.is_empty() => {
                            out.push_str(&format!("{k}:\n"));
                            for e in a {
                                out.push_str(&format!("  {}\n", e));
                            }
                        }
                        _ => out.push_str(&format!("{k:<28} {}\n", s(x))),
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_options_are_read_and_a_wrong_one_is_refused() {
        let a = parse(&args("lm358 dual --kind footprint --pins 8 --smd --pitch 1.27 --has-3d --fp-filter SOIC* --lib Package_SO -n 5 -C /tmp --table")).unwrap();
        assert_eq!(a.positional, ["lm358", "dual"]);
        assert_eq!((a.kind, a.search.pins, a.search.smd, a.search.pitch_mm, a.search.has_3d), (Some(Kind::Footprint), Some(8), true, Some(1.27), true));
        assert_eq!((a.search.fp_filters.clone(), a.search.lib.clone(), a.limit, a.table), (vec!["SOIC*".to_string()], Some("Package_SO".to_string()), Some(5), true));
        assert_eq!(a.dir, Some(PathBuf::from("/tmp")));
        assert!(parse(&args("--kind gizmo")).is_err());
        assert!(parse(&args("--pins")).is_err());
        assert!(parse(&args("--frobnicate")).is_err());
    }

    #[test]
    fn a_table_is_lines_of_text() {
        let v = json!({ "hits": [{ "score": 81, "id": "Amplifier_Operational:LM358", "pins": 8, "units": 3, "description": "Dual op-amp" }], "matches": 1, "truncated": false });
        let t = table("search", &v);
        assert!(t.contains("Amplifier_Operational:LM358") && t.contains("pins   8") && t.ends_with("1 matches\n"), "{t}");
        assert_eq!(table("status", &json!({ "error": "nope" })), "error: nope\n");
        let rows = table("sql", &json!({ "columns": ["a", "b"], "rows": [{ "a": 1, "b": "x" }] }));
        assert_eq!(rows, "a\tb\n1\tx\n");
    }
}
