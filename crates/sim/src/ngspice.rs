//! ngspice as a separate program: found the way the studio finds kicad-cli (`eda_kicad_engine::find_cli`: an environment variable, then
//! `PATH`), run in batch mode (`ngspice -b <deck>`) with what it prints -- standard output and standard error in the order it wrote them,
//! KiCad's console (`NGSPICE::cbSendChar` reports both, the "stdout "/"stderr " prefix stripped) -- and stopped by killing it.
//!
//! KiCad loads libngspice (`NGSPICE::init_dll`) and finds its code models and `spinit` beside its own install. A separately installed
//! ngspice finds `spinit` in the data directory it was built for, or in `SPICE_LIB_DIR`; an ngspice unpacked somewhere else (`~/tools/ngspice`
//! on the box) has its data in `share/ngspice` beside its `bin`, so a run sets `SPICE_LIB_DIR` there when it is not set and the directory
//! has `scripts/spinit`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The environment variable naming the ngspice binary.
pub const NGSPICE_ENV: &str = "EDA_NGSPICE";

/// What a machine without ngspice is told (KiCad's "Unable to load ngspice shared library. Please check your install." is about the library it
/// loads; this program runs the ngspice program).
pub const NGSPICE_MISSING: &str = "ngspice not found: the simulator runs the ngspice program (set EDA_NGSPICE to it, or put ngspice on PATH)";

/// `EDA_NGSPICE` (when it names a file), else `ngspice` on `PATH`.
pub fn find_ngspice() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(NGSPICE_ENV).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()) {
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join("ngspice")).find(|p| p.is_file())
}

/// `share/ngspice` beside the binary's `bin` (the binary followed through its links), when it has `scripts/spinit`.
pub fn spice_lib_dir(ngspice: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(ngspice).unwrap_or_else(|_| ngspice.to_path_buf());
    let dir = real.parent()?.parent()?.join("share").join("ngspice");
    dir.join("scripts").join("spinit").is_file().then_some(dir)
}

/// The run in progress, so another request can stop it. One run at a time: `SIMULATOR_FRAME::StartSimulation`'s "Another simulation is
/// already running." is answered from here.
#[derive(Default)]
pub struct RunSlot {
    pid: Mutex<Option<u32>>,
    stopped: AtomicBool,
}

/// `SIMULATOR_FRAME::StartSimulation`'s message when the simulator is busy.
pub const ALREADY_RUNNING: &str = "Another simulation is already running.";

impl RunSlot {
    pub fn is_running(&self) -> bool {
        self.pid.lock().map(|p| p.is_some()).unwrap_or(false)
    }

    /// Stop Simulation: the run's whole process group is killed (`NGSPICE::Stop` is `bg_halt`, which a batch run has no equivalent of; the vectors
    /// it made so far are lost with the process). Whether there was a run to stop.
    pub fn stop(&self) -> bool {
        let pid = self.pid.lock().ok().and_then(|p| *p);
        let Some(pid) = pid else { return false };
        self.stopped.store(true, Ordering::SeqCst);
        kill_group(pid);
        true
    }
}

fn kill_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg("-9").arg("--").arg(format!("-{pid}")).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutput {
    /// Everything ngspice printed, in order.
    pub console: String,
    pub success: bool,
    /// Stopped by [`RunSlot::stop`].
    pub stopped: bool,
    /// Killed for running longer than its limit.
    pub timed_out: bool,
}

/// Run `ngspice -b <deck>` in `cwd`, at most `limit`, registered in `slot` so that it can be stopped. Refused when `slot` has a run.
pub fn run_batch(ngspice: &Path, deck: &Path, cwd: &Path, slot: &RunSlot, limit: Duration) -> Result<RunOutput, String> {
    let mut cmd = Command::new(ngspice);
    cmd.arg("-b").arg(deck).current_dir(cwd).stdin(Stdio::null());
    if std::env::var_os("SPICE_LIB_DIR").is_none() {
        if let Some(dir) = spice_lib_dir(ngspice) {
            cmd.env("SPICE_LIB_DIR", dir);
        }
    }
    // One pipe for both streams, so the console has them in the order ngspice wrote them.
    let (mut reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    cmd.stdout(writer.try_clone().map_err(|e| e.to_string())?).stderr(writer);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = {
        let mut pid = slot.pid.lock().map_err(|_| "the simulator's lock is poisoned".to_string())?;
        if pid.is_some() {
            return Err(ALREADY_RUNNING.to_string());
        }
        let child = cmd.spawn().map_err(|e| format!("cannot run {}: {e}", ngspice.display()))?;
        *pid = Some(child.id());
        slot.stopped.store(false, Ordering::SeqCst);
        child
    };
    // The command (and its copies of the pipe's writing end) must go, or the read below never sees the end of the pipe.
    drop(cmd);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut reader, &mut bytes);
        let _ = tx.send(bytes);
    });
    let deadline = Instant::now() + limit;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                kill_group(child.id());
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break None,
        }
    };
    if let Ok(mut pid) = slot.pid.lock() {
        *pid = None;
    }
    let bytes = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    let stopped = slot.stopped.swap(false, Ordering::SeqCst);
    Ok(RunOutput { console: String::from_utf8_lossy(&bytes).into_owned(), success: status.is_some_and(|s| s.success()) && !stopped && !timed_out, stopped, timed_out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_directory_is_found_beside_the_binary() {
        let d = std::env::temp_dir().join(format!("eda-sim-ngspice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("usr/bin")).unwrap();
        std::fs::create_dir_all(d.join("usr/share/ngspice/scripts")).unwrap();
        std::fs::write(d.join("usr/bin/ngspice"), "").unwrap();
        assert_eq!(spice_lib_dir(&d.join("usr/bin/ngspice")), None, "no spinit yet");
        std::fs::write(d.join("usr/share/ngspice/scripts/spinit"), "").unwrap();
        assert!(spice_lib_dir(&d.join("usr/bin/ngspice")).unwrap().ends_with("usr/share/ngspice"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_run_is_stopped_by_killing_it_and_one_runs_at_a_time() {
        // A stand-in for ngspice that prints to both streams and then waits.
        let d = std::env::temp_dir().join(format!("eda-sim-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let fake = d.join("ngspice");
        std::fs::write(&fake, "#!/bin/sh\necho out\necho err 1>&2\nsleep 30\n").unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let slot = std::sync::Arc::new(RunSlot::default());
        let s2 = slot.clone();
        let (fake2, d2) = (fake.clone(), d.clone());
        let h = std::thread::spawn(move || run_batch(&fake2, Path::new("deck.cir"), &d2, &s2, Duration::from_secs(20)));
        let t0 = Instant::now();
        while !slot.is_running() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(run_batch(&fake, Path::new("deck.cir"), &d, &slot, Duration::from_secs(1)).unwrap_err(), ALREADY_RUNNING);
        std::thread::sleep(Duration::from_millis(200));
        assert!(slot.stop());
        let out = h.join().unwrap().unwrap();
        assert!(out.stopped && !out.success, "{out:?}");
        assert!(out.console.contains("out\n") && out.console.contains("err\n"), "{:?}", out.console);
        assert!(t0.elapsed() < Duration::from_secs(15));
        assert!(!slot.is_running() && !slot.stop());
        let _ = std::fs::remove_dir_all(&d);
    }
}
