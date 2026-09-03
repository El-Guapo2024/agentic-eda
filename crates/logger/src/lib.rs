//! `eda-logger` — the append-only JSONL run log every loop writes, plus the
//! escalation policy that reads it back.
//!
//! One line per event. Events are self-describing (`kind`), carry the run
//! id, stage, iteration and validity tier, and embed the full `CheckResult`
//! list for a candidate so a later pass (or a critic being trained) can
//! rebuild the scorecard without re-running anything. Candidate identity is
//! the blake3 of `Design::canonical_bytes()` so byte-identical retries are
//! visible as such.

use eda_model::ir::Stage;
use eda_model::{CheckResult, CheckStatus};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

/// Validity tiers, cheapest first. A candidate short-circuits at the first
/// tier that fails; everything after is not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Intent parses / evaluates.
    Schema,
    /// Model structurally sane (nets resolve, parts exist).
    Semantic,
    /// Electrical lint on the model, pre-geometry.
    Lint,
    /// Our own geometry gates on the derived design.
    Geometry,
    /// Second-opinion external checks (kicad-cli ERC/DRC).
    External,
    /// Hosted / learned critic.
    Critic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    RunStarted { run_id: String, ts: String, intent_path: String, intent_hash: String, engine_version: String },
    Candidate {
        run_id: String,
        ts: String,
        stage: Stage,
        iteration: u32,
        seed: u64,
        candidate_hash: String,
        tier_reached: Tier,
        checks: Vec<CheckResult>,
        #[serde(default)]
        metrics: serde_json::Value,
    },
    Escalated { run_id: String, ts: String, stage: Stage, iteration: u32, reason: String },
    Frozen { run_id: String, ts: String, stage: Stage, candidate_hash: String },
    Note { run_id: String, ts: String, text: String },
}

impl Event {
    pub fn run_id(&self) -> &str {
        match self {
            Event::RunStarted { run_id, .. }
            | Event::Candidate { run_id, .. }
            | Event::Escalated { run_id, .. }
            | Event::Frozen { run_id, .. }
            | Event::Note { run_id, .. } => run_id,
        }
    }
}

/// RFC 3339 UTC timestamp without pulling in a date crate.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

pub fn candidate_hash(design: &eda_model::ir::Design) -> String {
    match design.canonical_bytes() {
        Ok(b) => blake3::hash(&b).to_hex().to_string(),
        Err(_) => "unhashable".into(),
    }
}

/// Append-only writer. Flushes on every event so a crash mid-loop loses
/// nothing already decided.
pub struct RunLog {
    file: std::fs::File,
    pub run_id: String,
}

impl RunLog {
    pub fn open(path: &Path, run_id: impl Into<String>) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(RunLog { file, run_id: run_id.into() })
    }

    pub fn write(&mut self, event: &Event) -> std::io::Result<()> {
        let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }

    pub fn candidate(
        &mut self,
        stage: Stage,
        iteration: u32,
        seed: u64,
        design: &eda_model::ir::Design,
        tier_reached: Tier,
        checks: &[CheckResult],
        metrics: serde_json::Value,
    ) -> std::io::Result<()> {
        self.write(&Event::Candidate {
            run_id: self.run_id.clone(),
            ts: now_rfc3339(),
            stage,
            iteration,
            seed,
            candidate_hash: candidate_hash(design),
            tier_reached,
            checks: checks.to_vec(),
            metrics,
        })
    }

    pub fn note(&mut self, text: impl Into<String>) -> std::io::Result<()> {
        self.write(&Event::Note { run_id: self.run_id.clone(), ts: now_rfc3339(), text: text.into() })
    }
}

/// Read every event of a JSONL log (optionally only one run).
pub fn read_log(path: &Path, run_id: Option<&str>) -> std::io::Result<Vec<Event>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let ev: Event = serde_json::from_str(line).map_err(std::io::Error::other)?;
        if run_id.is_none_or(|r| ev.run_id() == r) {
            out.push(ev);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- policy

/// When a loop stops retrying and hands the problem up (to a stronger
/// author, or a human).
#[derive(Debug, Clone)]
pub struct EscalationPolicy {
    /// Escalate after this many candidates in a stage.
    pub max_iterations: u32,
    /// Escalate when the same check name fails on this many consecutive
    /// candidates.
    pub same_check_fails: u32,
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        EscalationPolicy { max_iterations: 5, same_check_fails: 2 }
    }
}

impl EscalationPolicy {
    /// Looks at the candidates of `stage` in `events`, in order. Returns the
    /// reason to escalate, or `None` to keep iterating.
    pub fn should_escalate(&self, events: &[Event], stage: Stage) -> Option<String> {
        let cands: Vec<&Event> = events.iter().filter(|e| matches!(e, Event::Candidate { stage: s, .. } if *s == stage)).collect();
        if cands.len() as u32 >= self.max_iterations {
            return Some(format!("{} candidates in stage {:?} without a clean one", cands.len(), stage));
        }
        // Consecutive same-check failures on the tail.
        let fail_sets: Vec<Vec<&str>> = cands
            .iter()
            .map(|e| match e {
                Event::Candidate { checks, .. } => checks.iter().filter(|c| c.status == CheckStatus::Fail).map(|c| c.check.as_str()).collect(),
                _ => vec![],
            })
            .collect();
        if fail_sets.len() as u32 >= self.same_check_fails {
            let tail = &fail_sets[fail_sets.len() - self.same_check_fails as usize..];
            for name in &tail[0] {
                if tail.iter().all(|set| set.contains(name)) {
                    return Some(format!("check `{name}` failed on {} consecutive candidates", self.same_check_fails));
                }
            }
        }
        None
    }
}

/// True when the candidate is clean: no `Fail` at any tier.
pub fn is_clean(checks: &[CheckResult]) -> bool {
    checks.iter().all(|c| c.status != CheckStatus::Fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(stage: Stage, i: u32, fails: &[&str]) -> Event {
        Event::Candidate {
            run_id: "r".into(),
            ts: now_rfc3339(),
            stage,
            iteration: i,
            seed: 0,
            candidate_hash: "h".into(),
            tier_reached: Tier::Geometry,
            checks: fails.iter().map(|f| CheckResult::fail(f, "x", "y")).collect(),
            metrics: serde_json::Value::Null,
        }
    }

    #[test]
    fn escalates_on_repeated_check() {
        let p = EscalationPolicy::default();
        let ev = vec![cand(Stage::Schematic, 0, &["a", "b"]), cand(Stage::Schematic, 1, &["b"])];
        assert!(p.should_escalate(&ev, Stage::Schematic).unwrap().contains("`b`"));
        let ev = vec![cand(Stage::Schematic, 0, &["a"]), cand(Stage::Schematic, 1, &["b"])];
        assert!(p.should_escalate(&ev, Stage::Schematic).is_none());
    }

    #[test]
    fn escalates_on_iteration_budget() {
        let p = EscalationPolicy { max_iterations: 3, same_check_fails: 99 };
        let ev: Vec<Event> = (0..3).map(|i| cand(Stage::Placement, i, &[&format!("c{i}")])).collect();
        assert!(p.should_escalate(&ev, Stage::Placement).is_some());
        assert!(p.should_escalate(&ev, Stage::Schematic).is_none());
    }

    #[test]
    fn round_trips_through_file() {
        let dir = std::env::temp_dir().join(format!("eda-logger-{}", std::process::id()));
        let path = dir.join("run.jsonl");
        let mut log = RunLog::open(&path, "run1").unwrap();
        log.write(&Event::RunStarted { run_id: "run1".into(), ts: now_rfc3339(), intent_path: "x.zen".into(), intent_hash: "abc".into(), engine_version: "0".into() }).unwrap();
        log.write(&cand(Stage::Schematic, 0, &["schematic_offgrid"])).unwrap();
        log.note("hello").unwrap();
        let back = read_log(&path, None).unwrap();
        assert_eq!(back.len(), 3);
        assert!(matches!(back[1], Event::Candidate { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn timestamp_shape() {
        let t = now_rfc3339();
        assert_eq!(t.len(), 20, "{t}");
        assert!(t.ends_with('Z'));
        assert!(t.starts_with("20"));
    }
}
