//! The one lane kicad-cli's work runs in, off the studio's request loop
//! (docs/ARCHITECTURE.md, "Engines"; `studio::offload` is the way in).
//!
//! A kicad-cli run takes seconds (a DRC about 4 s, an export more). The
//! studio's loop answers every other request itself, one at a time, so a run
//! done inline made edits, the `/api/version` poll that drives live
//! co-editing and the 3D view wait for it. Those requests now run here, each
//! on its own thread; only they wait on kicad-cli.
//!
//! The lane keeps kicad-cli bounded: **one run at a time**, whatever the
//! kind. Every run exports the design to the same scratch directory
//! (`<board>/.kicad/`: one `board.kicad_pcb`, one project file), so two runs
//! side by side would rewrite files kicad-cli is reading, and a DRC, an ERC
//! and a STEP export at once is what a 16 GB machine does not need. The 3D
//! view's GLB build has its own slot (`GlbJob`) and its own temp directory.
//!
//! A run that never ends would hold the lane for good, so `eda_kicad_engine`
//! kills one that outlives its limit (120 s for a DRC or ERC, 300 s for an
//! export) and returns an error; the lane then goes on with the next request.
//!
//! A request for what is already running joins it instead of starting
//! another run: same request (route and body) on the same design revision
//! (the stamp `/api/version` serves). Anything else waits for its turn, and
//! starts when the lane frees -- on the revision it finds then.
//!
//! Every reply carries `revision`, the stamp of the design the run started
//! from, so the studio can tell a result from the board that has moved on
//! since (`web/studio/src/kicad-port/checkRevision.ts`). The stamp is read
//! before the design is loaded: an edit that lands in between makes the
//! result look older than it is, never newer.

use serde_json::{json, Value};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// One run, as the requests that joined it see it.
struct Flight {
    what: String,
    revision: String,
    reply: Mutex<Option<Value>>,
    done: Condvar,
}

impl Flight {
    fn wait(&self) -> Value {
        let mut reply = relock(&self.reply);
        loop {
            if let Some(v) = reply.as_ref() {
                return v.clone();
            }
            reply = self.done.wait(reply).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// A panic elsewhere must not turn the lane into a wall of poisoned locks.
fn relock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct Lane {
    /// The run in progress, if any.
    current: Mutex<Option<Arc<Flight>>>,
    /// Signalled when `current` frees.
    changed: Condvar,
}

impl Lane {
    /// Run `work` as the lane's one job and return its reply with the
    /// `revision` it ran on, or, when the same `what` is already running on
    /// the same revision, wait for that run and return its reply. `what` names
    /// the request (route and body); `revision` is the design's current stamp,
    /// asked again whenever the lane frees. Blocks the calling thread for as
    /// long as the lane is busy and then for the run itself.
    pub fn run(&self, what: &str, revision: impl Fn() -> String, work: impl FnOnce() -> Value) -> Value {
        let mut work = Some(work);
        loop {
            let now = revision();
            let mut current = relock(&self.current);
            match current.as_ref() {
                Some(flight) if flight.what == what && flight.revision == now => {
                    let flight = flight.clone();
                    drop(current);
                    return flight.wait();
                }
                // Busy with another kind, or with this one on an older revision.
                Some(_) => drop(self.changed.wait(current).unwrap_or_else(PoisonError::into_inner)),
                None => {
                    let flight = Arc::new(Flight { what: what.to_string(), revision: now.clone(), reply: Mutex::new(None), done: Condvar::new() });
                    *current = Some(flight.clone());
                    drop(current);
                    let work = work.take().expect("a request starts its run once");
                    // A job that panics must still answer, or everything joined to it waits forever.
                    let mut reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| json!({ "error": "the kicad-cli job crashed" }));
                    if let Some(fields) = reply.as_object_mut() {
                        fields.insert("revision".into(), json!(now));
                    }
                    *relock(&flight.reply) = Some(reply.clone());
                    flight.done.notify_all();
                    *relock(&self.current) = None;
                    self.changed.notify_all();
                    return reply;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::time::Duration;

    fn rev(s: &'static str) -> impl Fn() -> String {
        move || s.to_string()
    }

    #[test]
    fn the_same_request_on_the_same_revision_joins_the_running_job() {
        let lane = Arc::new(Lane::default());
        let runs = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(std::sync::Barrier::new(6));
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let (lane, runs, start) = (lane.clone(), runs.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    lane.run("drc", rev("r1"), || {
                        runs.fetch_add(1, SeqCst);
                        std::thread::sleep(Duration::from_millis(500));
                        json!({ "violations": [] })
                    })
                })
            })
            .collect();
        let replies: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(runs.load(SeqCst), 1, "six requests, one run");
        assert!(replies.iter().all(|r| *r == json!({ "violations": [], "revision": "r1" })), "everyone gets the run's reply, stamped with its revision: {replies:?}");
    }

    #[test]
    fn different_requests_run_one_at_a_time() {
        let lane = Arc::new(Lane::default());
        let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let threads: Vec<_> = ["drc", "erc", "stats", "export"]
            .into_iter()
            .map(|what| {
                let (lane, now, most) = (lane.clone(), now.clone(), most.clone());
                std::thread::spawn(move || {
                    lane.run(what, rev("r1"), || {
                        most.fetch_max(now.fetch_add(1, SeqCst) + 1, SeqCst);
                        std::thread::sleep(Duration::from_millis(80));
                        now.fetch_sub(1, SeqCst);
                        json!({ "what": what })
                    })
                })
            })
            .collect();
        let replies: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(most.load(SeqCst), 1, "never two kicad-cli runs at once");
        let mut got: Vec<&str> = replies.iter().map(|r| r["what"].as_str().unwrap()).collect();
        got.sort();
        assert_eq!(got, ["drc", "erc", "export", "stats"], "and every request is answered");
    }

    #[test]
    fn a_request_on_a_newer_revision_waits_for_the_run_then_runs_again() {
        let lane = Arc::new(Lane::default());
        let revision = Arc::new(Mutex::new("r1".to_string()));
        let runs = Arc::new(AtomicUsize::new(0));
        let first = {
            let (lane, revision, runs) = (lane.clone(), revision.clone(), runs.clone());
            std::thread::spawn(move || {
                lane.run("drc", || relock(&revision).clone(), || {
                    runs.fetch_add(1, SeqCst);
                    std::thread::sleep(Duration::from_millis(300));
                    json!({ "n": 1 })
                })
            })
        };
        while runs.load(SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        // The board changes while the first run is going: a new request is not that run's.
        *relock(&revision) = "r2".to_string();
        let second = {
            let (lane, revision, runs) = (lane.clone(), revision.clone(), runs.clone());
            std::thread::spawn(move || {
                lane.run("drc", || relock(&revision).clone(), || {
                    runs.fetch_add(1, SeqCst);
                    json!({ "n": 2 })
                })
            })
        };
        assert_eq!(first.join().unwrap(), json!({ "n": 1, "revision": "r1" }), "the first run keeps the revision it started on");
        assert_eq!(second.join().unwrap(), json!({ "n": 2, "revision": "r2" }));
        assert_eq!(runs.load(SeqCst), 2);
    }

    /// The routes in studio.rs that start kicad-cli must go through `offload`; one answered
    /// inline holds up the whole serve loop again (edits, /api/version, the 3D view) for as
    /// long as kicad-cli runs. A new route that calls into the engine, the fab or the schematic
    /// output backends and fails this: wrap it in `offload(...)` like its neighbours.
    #[test]
    fn every_studio_route_that_starts_kicad_cli_goes_through_offload() {
        let studio = include_str!("studio.rs");
        let production = studio.split("#[cfg(test)]").next().unwrap();
        let starts_kicad_cli = ["kicad_engine::drc(", "kicad_engine::erc(", "kicad_engine::board_stats", "kicad_engine::check_rules", "kicad_engine::export", "fab_api::", "sch_output_api::", "board_output_api::", "sch_export_api::"];
        for (n, line) in production.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("use ") {
                continue;
            }
            assert!(
                !starts_kicad_cli.iter().any(|call| line.contains(call)) || line.contains("offload("),
                "studio.rs:{}: this starts kicad-cli on the request loop; run it through `offload(...)` (crates/cli/src/kicad_lane.rs): {}",
                n + 1,
                code
            );
        }
    }

    #[test]
    fn a_job_that_panics_answers_everyone_and_frees_the_lane() {
        let lane = Arc::new(Lane::default());
        let joined = {
            let lane = lane.clone();
            std::thread::spawn(move || lane.run("drc", rev("r1"), || panic!("kicad-cli blew up")))
        };
        let reply = joined.join().unwrap();
        assert_eq!(reply["error"], "the kicad-cli job crashed");
        assert_eq!(reply["revision"], "r1");
        assert_eq!(lane.run("drc", rev("r1"), || json!({ "ok": true })), json!({ "ok": true, "revision": "r1" }), "the lane works again");
    }
}
