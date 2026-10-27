//! The `kicad-cli api-server` process this program keeps, and the requests it sends it.
//!
//! `kicad-cli api-server --socket <path>` (`kicad/cli/command_api_server.cpp::API_SERVER_COMMAND::doPerform`) is KiCad's IPC API with no GUI: a
//! `KICAD_API_SERVER` with the common handler, which opens one project at a time and, for an open board, registers `API_HANDLER_PCB` over a
//! `HEADLESS_PCB_CONTEXT` (`pcbnew/pcbnew.cpp::IFACE::handleOpenPcb`). Each request is an `ApiRequest` whose `google.protobuf.Any` carries the
//! command; the reply is an `ApiResponse` with a status and the answer (`common/api/api_server.cpp::KICAD_API_SERVER::onApiRequest`).
//!
//! The lane ([`with_server`]) starts the server on first use and keeps it: a request finds it up (one `Ping` when it has been idle), or restarts it
//! when the process died or stopped answering, and runs again once on the new one. One request runs at a time (the lane's lock): the server holds
//! one project, so a use that changes the board opens it, works and closes everything again ([`Client::open_board`], [`Client::close_all`]); export
//! jobs leave theirs open for the next job on the same file ([`Client::open_board_kept`]).
//!
//! The process ends with this program. It runs under a small `sh` that reads its stdin, a pipe from here, and kills its process group when the pipe
//! closes: on [`shutdown`] (the CLI's end, the studio's) and on any exit of this program, a crash or a `kill -9` included, since the kernel closes
//! the pipe then too. A kicad-cli without `api-server` (the Mac's 10.99.0-5334, any KiCad 9) or `EDA_KICAD_API=0` leaves the lane closed, and
//! every caller has its per-run kicad-cli path (or our own code) to fall back on: [`ApiError::falls_back`].

use super::proto::google::protobuf::Any;
use super::proto::kiapi::common::commands::{CloseAllDocuments, GetVersion, GetVersionResponse, OpenDocument, OpenDocumentResponse, Ping};
use super::proto::kiapi::common::types::{DocumentSpecifier, DocumentType};
use super::proto::kiapi::common::{ApiRequest, ApiRequestHeader, ApiResponse, ApiStatusCode};
use super::sp::ReqSocket;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Set to `0`, `off` or `no`, the lane stays closed: every caller takes its per-run kicad-cli path, as on a kicad-cli without `api-server`.
pub const ENV: &str = "EDA_KICAD_API";

/// How long a started server may take to answer its first `Ping`. It loads no board at start, so it is up in about a second here; a first start
/// after an install writes KiCad's default settings first.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a `Ping` may take: the server answers it from its event loop, so a server that does not is stuck.
const PING_TIMEOUT: Duration = Duration::from_secs(10);

/// A server idle for longer than this is pinged before it is used.
const IDLE_CHECK: Duration = Duration::from_secs(2);

/// After a start that failed, the lane stays closed this long before it tries again (a broken kicad-cli is not started on every request).
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Why a request through the lane did not work.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The lane is closed: no kicad-cli, one without `api-server`, [`ENV`] off, or a server that would not start (the reason).
    Unavailable(String),
    /// The socket failed under the request (the server died or hung): the lane restarts it.
    Transport(String),
    /// KiCad answered with an error status (`ApiResponseStatus`): its code and its words.
    Status { code: ApiStatusCode, message: String },
    /// The reply could not be read as the message the command answers with.
    Decode(String),
}

impl ApiError {
    /// Whether the caller should do the job its other way (per-run kicad-cli, or our own code): the lane is closed, or this server does not know the
    /// command (`AS_UNHANDLED`: no handler, such as `ImportSpecctraSession` on a KiCad before 11.0; `AS_UNIMPLEMENTED`), or it failed under the
    /// request twice. A `Status` with KiCad's own refusal (`AS_BAD_REQUEST`: the file is wrong) is an answer, not a reason to ask elsewhere.
    pub fn falls_back(&self) -> bool {
        match self {
            ApiError::Unavailable(_) | ApiError::Transport(_) | ApiError::Decode(_) => true,
            ApiError::Status { code, .. } => matches!(code, ApiStatusCode::AsUnhandled | ApiStatusCode::AsUnimplemented | ApiStatusCode::AsNotReady | ApiStatusCode::AsBusy),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unavailable(why) => write!(f, "kicad-cli api-server is not available: {why}"),
            ApiError::Transport(why) => write!(f, "kicad-cli api-server stopped answering: {why}"),
            ApiError::Status { code, message } => write!(f, "{}{}", code.as_str_name(), if message.is_empty() { String::new() } else { format!(": {message}") }),
            ApiError::Decode(why) => write!(f, "kicad-cli api-server answered something unreadable: {why}"),
        }
    }
}

/// One running `kicad-cli api-server` and the connection to it.
pub struct Server {
    /// The `sh` that runs it (the leader of its process group).
    child: Child,
    /// The pipe whose end ends it.
    stdin: Option<ChildStdin>,
    socket: ReqSocket,
    /// The folder of the socket and the server's stderr (`server.log`), removed with it.
    dir: PathBuf,
    cli: PathBuf,
    last_used: Instant,
    client_name: String,
    /// The board left open for the next job ([`Client::open_board_kept`]): its path, the hash of its file and its project's, and its specifier.
    kept: Option<(PathBuf, u64, DocumentSpecifier)>,
}

fn socket_dir() -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    // A Unix socket's path is at most 104 bytes on macOS (108 on Linux): a short name in the temporary folder.
    std::env::temp_dir().join(format!("eda-kapi-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)))
}

/// The socket folders of programs that ended without [`shutdown`] (a `kill -9`: the server went with the pipe, its folder stayed), removed.
fn sweep_stale_dirs() {
    let me = std::process::id();
    for e in std::fs::read_dir(std::env::temp_dir()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix("eda-kapi-").and_then(|r| r.split('-').next()).and_then(|p| p.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let alive = Command::new("kill").args(["-0", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if !alive {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

impl Server {
    /// Start `kicad-cli api-server` at `cli` with this program's KiCad settings folder (`super::super::cli_command`) and wait for it to answer a `Ping`.
    pub fn start(cli: &Path) -> Result<Server, String> {
        sweep_stale_dirs();
        let dir = socket_dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let sock = dir.join("api.sock");
        let log = dir.join("server.log");
        // The settings folder and whatever else a per-run kicad-cli gets, carried over to the shell that runs this one.
        let template = crate::cli_command(cli, None);
        let mut cmd = Command::new("sh");
        for (k, v) in template.get_envs() {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        // `read` returns when the pipe from here closes (this program ended or let go of the server); `kill 0` then ends the whole group: the
        // shell, kicad-cli's own launcher script and the server, which shuts down on SIGTERM (`apiServerSignalHandler`).
        cmd.args(["-c", "\"$1\" api-server --socket \"$2\" >/dev/null 2>>\"$3\" </dev/null & read _; kill -TERM 0 2>/dev/null", "sh"]).arg(cli).arg(&sock).arg(&log);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().map_err(|e| format!("cannot run {}: {e}", cli.display()))?;
        let stdin = child.stdin.take();
        let client_name = format!("eda-{}", std::process::id());
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last = String::from("no socket yet");
        let socket = loop {
            if let Ok(Some(status)) = child.try_wait() {
                let said = std::fs::read_to_string(&log).unwrap_or_default();
                let _ = std::fs::remove_dir_all(&dir);
                return Err(format!("kicad-cli api-server exited ({status}){}", if said.trim().is_empty() { String::new() } else { format!(": {}", said.trim()) }));
            }
            if Instant::now() >= deadline {
                let mut stdin = stdin;
                stop_group(&mut child, &mut stdin, &dir);
                return Err(format!("kicad-cli api-server did not answer within {} s ({last})", START_TIMEOUT.as_secs()));
            }
            if sock.exists() {
                match ReqSocket::connect(&sock, PING_TIMEOUT) {
                    Ok(mut s) => match call_on::<Ping, ()>(&mut s, &client_name, &Ping {}, PING_TIMEOUT) {
                        Ok(()) => break s,
                        Err(e) => last = e.to_string(),
                    },
                    Err(e) => last = e.to_string(),
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Ok(Server { child, stdin, socket, dir, cli: cli.to_path_buf(), last_used: Instant::now(), client_name, kept: None })
    }

    /// Whether the process is still there and answers a `Ping`.
    pub fn healthy(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None)) && self.call::<Ping, ()>(&Ping {}, PING_TIMEOUT).is_ok()
    }

    /// The kicad-cli this server is.
    pub fn cli(&self) -> &Path {
        &self.cli
    }

    /// The process id of the `sh` that runs the server (the leader of its process group): for a test that kills it.
    pub fn group_id(&self) -> u32 {
        self.child.id()
    }

    /// Send `req` and read the reply as `R`, waiting up to `timeout`.
    pub fn call<Q: prost::Message + prost::Name, R: prost::Message + Default>(&mut self, req: &Q, timeout: Duration) -> Result<R, ApiError> {
        self.last_used = Instant::now();
        call_on(&mut self.socket, &self.client_name, req, timeout)
    }
}

/// End a server: close the pipe (the shell kills its group), wait for the group to go, and kill what is left of it after five seconds.
fn stop_group(child: &mut Child, stdin: &mut Option<ChildStdin>, dir: &Path) {
    drop(stdin.take());
    let deadline = Instant::now() + Duration::from_secs(5);
    let pgid = child.id();
    let group_alive = || Command::new("kill").args(["-0", "--", &format!("-{pgid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    // The shell is reaped as it goes (a zombie leader would keep the group alive to `kill -0`); the server is init's to reap by then.
    while Instant::now() < deadline && (!matches!(child.try_wait(), Ok(Some(_))) || group_alive()) {
        std::thread::sleep(Duration::from_millis(20));
    }
    if group_alive() {
        let _ = Command::new("kill").args(["-9", "--", &format!("-{pgid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(dir);
}

impl Drop for Server {
    fn drop(&mut self) {
        stop_group(&mut self.child, &mut self.stdin, &self.dir);
    }
}

/// One request on a socket: the `ApiRequest` of `req` (`client_name` names this program in KiCad's log and its commits), and its `ApiResponse` read as
/// `R` when the status is `AS_OK`.
fn call_on<Q: prost::Message + prost::Name, R: prost::Message + Default>(socket: &mut ReqSocket, client_name: &str, req: &Q, timeout: Duration) -> Result<R, ApiError> {
    let request = ApiRequest {
        header: Some(ApiRequestHeader { kicad_token: String::new(), client_name: client_name.to_string() }),
        message: Some(Any { type_url: Q::type_url(), value: req.encode_to_vec() }),
    };
    let bytes = socket.request(&prost::Message::encode_to_vec(&request), timeout).map_err(|e| ApiError::Transport(format!("{}: {e}", Q::full_name())))?;
    let response = <ApiResponse as prost::Message>::decode(bytes.as_slice()).map_err(|e| ApiError::Decode(e.to_string()))?;
    let status = response.status.unwrap_or_default();
    let code = ApiStatusCode::try_from(status.status).unwrap_or(ApiStatusCode::AsUnknown);
    if code != ApiStatusCode::AsOk {
        return Err(ApiError::Status { code, message: status.error_message });
    }
    let value = response.message.map(|a| a.value).unwrap_or_default();
    R::decode(value.as_slice()).map_err(|e| ApiError::Decode(format!("{}: {e}", Q::full_name())))
}

/// What a lane user gets: the server, and the commands every use needs.
pub struct Client<'a> {
    server: &'a mut Server,
}

impl Client<'_> {
    /// [`Server::call`] with `timeout`.
    pub fn call<Q: prost::Message + prost::Name, R: prost::Message + Default>(&mut self, req: &Q, timeout: Duration) -> Result<R, ApiError> {
        self.server.call(req, timeout)
    }

    /// `GetVersion`: the server's KiCad (`full_version`, e.g. "10.99.0-...").
    pub fn version(&mut self) -> Result<String, ApiError> {
        let v: GetVersionResponse = self.call(&GetVersion {}, PING_TIMEOUT)?;
        Ok(v.version.map(|v| v.full_version).unwrap_or_default())
    }

    /// `CloseAllDocuments { force }`: the server holds nothing (the project is unloaded, unsaved changes dropped).
    pub fn close_all(&mut self) -> Result<(), ApiError> {
        self.server.kept = None;
        self.call::<_, ()>(&CloseAllDocuments { force: true }, crate::limit(crate::EXPORT_TIMEOUT))
    }

    /// The board file `pcb` open for export jobs, left open afterwards: a board already open from the same file, with the same bytes in it and in its
    /// project, is used as it is. A job loads the board once more for itself and keeps it while the document stays open (`PCBNEW_JOBS_HANDLER::getBoard`
    /// caches `m_cliBoard` until `IFACE::closeBoard` clears it), so the jobs of one revision of the design after the first pay neither load.
    /// Only for commands that do not change the board: anything else opens with [`Client::open_board`], which closes it first.
    pub fn open_board_kept(&mut self, pcb: &Path) -> Result<DocumentSpecifier, ApiError> {
        let path = std::fs::canonicalize(pcb).unwrap_or_else(|_| pcb.to_path_buf());
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::fs::read(&path).unwrap_or_default().hash(&mut h);
            std::fs::read(path.with_extension("kicad_pro")).unwrap_or_default().hash(&mut h);
            h.finish()
        };
        if let Some((p, h, doc)) = &self.server.kept {
            if *p == path && *h == hash {
                return Ok(doc.clone());
            }
        }
        let doc = self.open(&path, DocumentType::DoctypePcb)?;
        self.server.kept = Some((path, hash, doc.clone()));
        Ok(doc)
    }

    /// `OpenDocument { DOCTYPE_PCB, path }` after closing whatever was open (one project at a time): the board loads as `kicad-cli pcb` loads it, with its
    /// project beside it, and the answer is the specifier the board's commands take.
    pub fn open_board(&mut self, pcb: &Path) -> Result<DocumentSpecifier, ApiError> {
        self.open(pcb, DocumentType::DoctypePcb)
    }

    /// `OpenDocument { DOCTYPE_SCHEMATIC, path }`, as [`Client::open_board`].
    pub fn open_schematic(&mut self, sch: &Path) -> Result<DocumentSpecifier, ApiError> {
        self.open(sch, DocumentType::DoctypeSchematic)
    }

    fn open(&mut self, path: &Path, kind: DocumentType) -> Result<DocumentSpecifier, ApiError> {
        self.close_all()?;
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let r: OpenDocumentResponse = self.call(&OpenDocument { r#type: kind as i32, path: path.to_string_lossy().into_owned() }, crate::limit(crate::EXPORT_TIMEOUT))?;
        r.document.ok_or_else(|| ApiError::Decode("OpenDocument answered no document".into()))
    }
}

/// The lane's state: the server, or why there is none.
#[derive(Default)]
struct Lane {
    server: Option<Server>,
    /// The last start that failed: when, and why (the lane stays closed for [`RETRY_AFTER`]).
    failed: Option<(Instant, String)>,
    /// Servers started by this program: the first, and every restart.
    starts: u32,
}

fn lane() -> &'static Mutex<Lane> {
    static LANE: OnceLock<Mutex<Lane>> = OnceLock::new();
    LANE.get_or_init(Default::default)
}

/// Whether [`ENV`] closes the lane.
fn turned_off() -> bool {
    std::env::var(ENV).is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "off" | "no" | "false"))
}

/// Whether kicad-cli at `cli` has the `api-server` command: its top-level `--help` lists it (one run per binary per run of this program).
pub fn cli_has_api_server(cli: &Path) -> bool {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<PathBuf, bool>>> = OnceLock::new();
    crate::per_cli(&CACHE, cli, || {
        let mut cmd = crate::cli_command(cli, None);
        cmd.arg("--help");
        crate::output_within(cmd, crate::limit(crate::VERSION_TIMEOUT))
            .map(|o| crate::parse_export_kinds(&format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).iter().any(|k| k == "api-server"))
            .unwrap_or(false)
    })
}

/// Why the lane is closed on this machine, or `None` when it can be used (it may still fail to start: [`with_server`] says so).
pub fn unavailable() -> Option<String> {
    if turned_off() {
        return Some(format!("{ENV} is off"));
    }
    let Some(cli) = crate::find_cli() else { return Some("kicad-cli not found".into()) };
    if !cli_has_api_server(&cli) {
        return Some(format!("kicad-cli {} has no api-server command", crate::cli_version(&cli)));
    }
    None
}

/// Run `f` on the lane's server, starting it first if need be. The server is pinged when it has been idle, and restarted when its process is gone,
/// it does not answer, or `f` fails on its socket ([`ApiError::Transport`]); `f` then runs once more on the new server, so it must be a whole job
/// (open, work, close). An [`ApiError::Unavailable`] says the lane is closed; the caller falls back ([`ApiError::falls_back`]).
pub fn with_server<T>(mut f: impl FnMut(&mut Client) -> Result<T, ApiError>) -> Result<T, ApiError> {
    if let Some(why) = unavailable() {
        return Err(ApiError::Unavailable(why));
    }
    let cli = crate::find_cli().ok_or_else(|| ApiError::Unavailable("kicad-cli not found".into()))?;
    let mut lane = lane().lock().unwrap_or_else(|p| p.into_inner());
    for attempt in 0..2 {
        // A server of another kicad-cli (`EDA_KICAD_CLI` changed), one that died, or one that stopped answering is replaced.
        let keep = match lane.server.as_mut() {
            Some(s) if s.cli() != cli => false,
            Some(s) if s.last_used.elapsed() >= IDLE_CHECK || attempt > 0 => s.healthy(),
            Some(_) => true,
            None => false,
        };
        if !keep {
            lane.server = None;
            if let Some((when, why)) = &lane.failed {
                if when.elapsed() < RETRY_AFTER {
                    return Err(ApiError::Unavailable(why.clone()));
                }
            }
            match Server::start(&cli) {
                Ok(s) => {
                    lane.server = Some(s);
                    lane.starts += 1;
                    lane.failed = None;
                }
                Err(why) => {
                    lane.failed = Some((Instant::now(), why.clone()));
                    return Err(ApiError::Unavailable(why));
                }
            }
        }
        let server = lane.server.as_mut().expect("started above");
        match f(&mut Client { server }) {
            Err(ApiError::Transport(_)) if attempt == 0 => {
                // The server died or hung under the job: a new one, and the job again.
                lane.server = None;
            }
            other => return other,
        }
    }
    Err(ApiError::Transport("kicad-cli api-server failed twice".into()))
}

/// How many servers this program has started (the first and every restart): for the tests and the studio's status.
pub fn starts() -> u32 {
    lane().lock().map(|l| l.starts).unwrap_or(0)
}

/// The process group id of the running server, if one runs: for a test that kills it.
pub fn running_group_id() -> Option<u32> {
    lane().lock().ok().and_then(|l| l.server.as_ref().map(Server::group_id))
}

/// Stop the server, if one runs. The CLI calls this at its end and the studio when it stops; a program that ends without it stops the server too
/// (the pipe closes), so this only makes it prompt.
pub fn shutdown() {
    if let Ok(mut l) = lane().lock() {
        l.server = None;
    }
}
