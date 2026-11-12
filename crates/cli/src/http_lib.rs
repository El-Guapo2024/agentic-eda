//! HTTP symbol libraries (UI-LEFT-2 LL5): a row of the symbol library table of type `HTTP` names a `.kicad_httplib` settings file, and KiCad reads the library
//! from the REST server it names (`SCH_IO_HTTP_LIB`, eeschema/sch_io/http_lib/sch_io_http_lib.cpp; `HTTP_LIB_CONNECTION`, common/http_lib/http_lib_connection.cpp;
//! `HTTP_LIB_SETTINGS`, common/http_lib/http_lib_settings.cpp; KiCad 8303b2ad). Ported here, on the server (a page cannot reach another server, and the
//! studio's readers take a library as a file):
//!
//!   * the settings (`ensureSettings`): `source.type` (`REST_API`, else "has invalid library type."), `source.api_version` (needed; `v1` is the only one:
//!     "uses API version %s, but KiCad requires version %s."), `source.root_url` (needed; a `/` added, then the API version and a `/`), `source.token`,
//!     `source.timeout_parts_seconds` (30) and `source.timeout_categories_seconds` (600), with KiCad's messages;
//!   * the connection (`validateHttpLibraryEndpoints`, `syncCategories`, `SelectAll`, `SelectOne`): every request with `Accept: application/json` and
//!     `Authorization: Token <token>`, redirects followed; the root must answer 200 with non-empty `categories` and `parts`; `categories.json` lists the
//!     categories (`id`, `name`, `description`), `parts/category/<id>.json` a category's parts (`id`, `name` -- the id when none, illegal characters as `_`
//!     (`LIB_ID::FixIllegalChars( name, false )`) --, `description`, `keywords`, `footprint_filters`), `parts/<id>.json` one part with its `symbolIdStr`,
//!     the `exclude_from_*` flags (`boolFromString`) and its `fields` (`value`, `visible`); an answer that is not 200 is "API responded with error code: %d:
//!     %s" (`httpErrorCodeDescription`);
//!   * each part's symbol (`loadSymbolFromPart`): the symbol `symbolIdStr` names, read through the other libraries of the tables (flat), renamed after the
//!     part, else an empty symbol of that name ("return metadata only"); the flags; each field -- Footprint (its first `;`-separated entry, the rest footprint
//!     filters), Description, Value, Datasheet, Reference, Keywords, else the symbol's field of that name or a new user field -- with its visibility; then the
//!     part's description, keywords and footprint filters.
//!
//! The library is then a `.kicad_sym` file the studio writes into its library cache (as `crate::library_convert` keeps the converted libraries) and every
//! reader reads as any other: listed, searched, opened, placed. It is fetched again when the settings file changes or after `timeout_categories_seconds`
//! (KiCad's category cache; a part is fetched with its category here, so the parts' shorter time is not kept apart). A library that cannot be read keeps
//! KiCad's message as its row's status. **Differs**: KiCad lists each category as a library of its own in the trees ("Nickname - Category",
//! `GetSubLibraryNames`); here the parts are one library under the row's nickname, as their `Nickname:Part` ids are in KiCad too. Database libraries
//! (`SCH_IO_DATABASE`) need an ODBC driver, a dependency the studio does not take: they stay listed and not read.

use eda_kicad::library_table as lt;
use eda_model::ir::LibrarySymbol;
use eda_model::sym_fields::LibrarySymbolField;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime};

/// `HTTP_LIB_SETTINGS::m_api_version`: the only API version KiCad speaks.
const SUPPORTED_API_VERSION: &str = "v1";

/// The settings of a `.kicad_httplib` file (`HTTP_LIB_SOURCE`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Source {
    /// The root URL with the API version: `<root_url>/<api_version>/`.
    pub(crate) root_url: String,
    pub(crate) token: String,
    pub(crate) timeout_parts: u64,
    pub(crate) timeout_categories: u64,
}

/// `SCH_IO_HTTP_LIB::ensureSettings`' `tryLoad`: the settings file read, or KiCad's message.
pub(crate) fn read_settings(path: &Path) -> Result<Source, String> {
    let shown = path.display();
    let missing = || format!("HTTP library settings file {shown} missing or invalid.");
    let text = std::fs::read_to_string(path).map_err(|_| missing())?;
    let json: Value = serde_json::from_str(&text).map_err(|_| missing())?;
    let src = json.get("source").cloned().unwrap_or(Value::Null);
    let s = |k: &str| src.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str, d: u64| src.get(k).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))).unwrap_or(d);
    let api_version = s("api_version");
    if api_version.is_empty() {
        return Err(format!("HTTP library settings file {shown} is missing the API version number."));
    }
    if api_version != SUPPORTED_API_VERSION {
        return Err(format!("HTTP library settings file {shown} uses API version {api_version}, but KiCad requires version {SUPPORTED_API_VERSION}."));
    }
    let mut root_url = s("root_url");
    if root_url.is_empty() {
        return Err(format!("HTTP library settings file {shown} is missing the root URL."));
    }
    if s("type") != "REST_API" {
        return Err(format!("HTTP library settings file {shown} has invalid library type."));
    }
    if !root_url.ends_with('/') {
        root_url.push('/');
    }
    root_url.push_str(&api_version);
    root_url.push('/');
    Ok(Source { root_url, token: s("token"), timeout_parts: n("timeout_parts_seconds", 30), timeout_categories: n("timeout_categories_seconds", 600) })
}

/// `httpErrorCodeDescription`.
fn http_code_description(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy (Deprecated)",
        306 => "Unused",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required (Experimental)",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early (Experimental)",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "Unknown",
    }
}

/// One request (`createCurlEasyObject` + `Perform` + `checkServerResponse`): the body as JSON, or the error KiCad appends to `m_lastError`.
fn get_json(src: &Source, url: &str) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new().redirects(8).timeout(Duration::from_secs(30)).build();
    let resp = agent.get(url).set("Accept", "application/json").set("Authorization", &format!("Token {}", src.token)).call();
    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(format!("API responded with error code: {code}: {}\n", http_code_description(code))),
        Err(ureq::Error::Transport(t)) => return Err(format!("Error: {t}\n")),
    };
    if resp.status() != 200 {
        return Err(format!("API responded with error code: {}: {}\n", resp.status(), http_code_description(resp.status())));
    }
    let body = resp.into_string().map_err(|e| format!("Error: {e}\n"))?;
    if body.is_empty() {
        return Err("KiCad received an empty response!\n".to_string());
    }
    serde_json::from_str(&body).map_err(|e| format!("Error: {e}\nAPI Response:  {body}\n"))
}

/// `HTTP_LIB_CATEGORY`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Category {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
}

/// `HTTP_LIB_PART`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Part {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) symbol_id: String,
    pub(crate) exclude_from_bom: bool,
    pub(crate) exclude_from_board: bool,
    pub(crate) exclude_from_sim: bool,
    pub(crate) desc: String,
    pub(crate) keywords: String,
    pub(crate) fp_filters: Vec<String>,
    /// (name, value, visible), in the answer's order.
    pub(crate) fields: Vec<(String, String, bool)>,
}

/// A JSON value as the text `std::string` takes it: a string as it is, anything else written out (nlohmann's `get<std::string>` refuses those; KiCad's part
/// then fails, the studio keeps the text).
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `boolFromString`: "true", "yes", "y", "1" (any case) are true, "false", "no", "n", "0" false, anything else the default.
pub(crate) fn bool_from_string(v: &str, default: bool) -> bool {
    let low = v.to_lowercase();
    if low.is_empty() {
        return default;
    }
    if ["true", "yes", "y", "1"].contains(&low.as_str()) {
        return true;
    }
    if ["false", "no", "n", "0"].contains(&low.as_str()) {
        return false;
    }
    default
}

/// `LIB_ID::FixIllegalChars( name, false )`: `:`, tab, line feed, carriage return, `\`, `<`, `>` and `"` become `_`.
fn fix_item_name(name: &str) -> String {
    name.chars().map(|c| if matches!(c, ':' | '\t' | '\n' | '\r' | '\\' | '<' | '>' | '"') { '_' } else { c }).collect()
}

/// `setPartIdNameAndMetadata`.
fn part_metadata(v: &Value) -> Part {
    let id = text_of(v.get("id").unwrap_or(&Value::Null));
    let name = v.get("name").map(text_of).unwrap_or_else(|| id.clone());
    let fp_filters = match v.get("footprint_filters") {
        Some(Value::Array(a)) => a.iter().map(text_of).collect(),
        Some(other) => vec![text_of(other)],
        None => Vec::new(),
    };
    Part { name: fix_item_name(&name), id, desc: v.get("description").map(text_of).unwrap_or_default(), keywords: v.get("keywords").map(text_of).unwrap_or_default(), fp_filters, ..Default::default() }
}

/// `SelectOne`'s reading of `parts/<id>.json`.
fn part_details(v: &Value) -> Part {
    let mut p = part_metadata(v);
    p.symbol_id = v.get("symbolIdStr").map(text_of).unwrap_or_default();
    let flag = |k: &str| v.get(k).map(|x| bool_from_string(&text_of(x), false)).unwrap_or(false);
    p.exclude_from_bom = flag("exclude_from_bom");
    p.exclude_from_board = flag("exclude_from_board");
    p.exclude_from_sim = flag("exclude_from_sim");
    if let Some(Value::Object(fields)) = v.get("fields") {
        for (k, props) in fields {
            let value = props.get("value").map(text_of).unwrap_or_default();
            let visible = props.get("visible").map(|x| bool_from_string(&text_of(x), true)).unwrap_or(true);
            p.fields.push((k.clone(), value, visible));
        }
    }
    p
}

/// What the server answers for the library: its categories, and each category's parts in full.
pub(crate) fn fetch(src: &Source) -> Result<Vec<(Category, Vec<Part>)>, String> {
    // `validateHttpLibraryEndpoints`
    let root = get_json(src, &src.root_url)?;
    // `!response.at( "categories" ).empty()`: nlohmann's `empty()` is true for null and an empty array or object only (a string, even "", is not empty)
    let has = |k: &str| root.get(k).is_some_and(|v| !(v.is_null() || v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(serde_json::Map::is_empty)));
    if !(has("categories") && has("parts")) {
        return Err(format!("Could not connect to {}. Errors: ", src.root_url));
    }
    // `syncCategories`
    let cats = get_json(src, &format!("{}categories.json", src.root_url))?;
    let mut out = Vec::new();
    for c in cats.as_array().cloned().unwrap_or_default() {
        let category = Category { id: text_of(c.get("id").unwrap_or(&Value::Null)), name: text_of(c.get("name").unwrap_or(&Value::Null)), description: c.get("description").map(text_of).unwrap_or_default() };
        // `SelectAll`, then `SelectOne` of each part (`LoadSymbol`)
        let list = get_json(src, &format!("{}parts/category/{}.json", src.root_url, category.id)).map_err(|e| format!("Error retrieving data from HTTP library {}: {e}", category.name))?;
        let mut parts = Vec::new();
        for item in list.as_array().cloned().unwrap_or_default() {
            let meta = part_metadata(&item);
            let full = get_json(src, &format!("{}parts/{}.json", src.root_url, meta.id)).map(|v| part_details(&v)).unwrap_or(meta);
            parts.push(full);
        }
        out.push((category, parts));
    }
    Ok(out)
}

/// `SCH_IO_HTTP_LIB::loadSymbolFromPart`: the part as a library symbol `nickname:name`. `base` reads the symbol a `symbolIdStr` names (flat), from the other
/// libraries of the tables.
pub(crate) fn symbol_from_part(nickname: &str, part: &Part, base: &dyn Fn(&str) -> Option<LibrarySymbol>) -> LibrarySymbol {
    let lib_id = format!("{nickname}:{}", part.name);
    let mut sym = match (!part.symbol_id.is_empty()).then(|| base(&part.symbol_id)).flatten() {
        Some(mut s) => {
            s.lib_id = lib_id.clone();
            s.extends = None;
            s
        }
        None => LibrarySymbol::new_empty(lib_id.clone()),
    };
    sym.in_bom = !part.exclude_from_bom;
    sym.on_board = !part.exclude_from_board;
    sym.exclude_from_sim = part.exclude_from_sim;
    let mut fields = sym.all_fields();
    let mut fp_filters: Vec<String> = Vec::new();
    let set = |fields: &mut Vec<LibrarySymbolField>, name: &str, text: &str, visible: bool| match fields.iter_mut().find(|f| f.name == name) {
        Some(f) => {
            f.text = text.to_string();
            f.hidden = !visible;
        }
        None => {
            let mut f = LibrarySymbolField::named(name, text);
            f.hidden = !visible;
            fields.push(f);
        }
    };
    let mut keywords: Option<String> = None;
    for (name, value, visible) in &part.fields {
        match name.to_lowercase().as_str() {
            "footprint" => {
                let tokens: Vec<String> = value.split([';', '\t', '\r', '\n']).filter(|t| !t.is_empty()).map(str::to_string).collect();
                if let Some(first) = tokens.first() {
                    set(&mut fields, "Footprint", first, *visible);
                } else if let Some(f) = fields.iter_mut().find(|f| f.name == "Footprint") {
                    f.hidden = !visible;
                }
                fp_filters.extend(tokens);
            }
            "description" => set(&mut fields, "Description", value, *visible),
            "value" => set(&mut fields, "Value", value, *visible),
            "datasheet" => set(&mut fields, "Datasheet", value, *visible),
            "reference" => set(&mut fields, "Reference", value, *visible),
            "keywords" => keywords = Some(value.clone()),
            _ => set(&mut fields, name, value, *visible),
        }
    }
    sym.set_fields(fields);
    if let Some(k) = keywords {
        sym.keywords = k;
    }
    // `SetDescription( aPart.desc )`, `SetKeyWords( aPart.keywords )`: the part's own, over the fields'
    sym.description = part.desc.clone();
    if let Some(f) = sym.fields.iter_mut().find(|f| f.name == "Description") {
        f.text = part.desc.clone();
    }
    sym.keywords = part.keywords.clone();
    fp_filters.extend(part.fp_filters.iter().cloned());
    sym.footprint_filters = fp_filters;
    sym.published = true;
    sym
}

type Memo = Mutex<HashMap<PathBuf, (String, SystemTime, Result<PathBuf, String>)>>;

fn memo() -> &'static Memo {
    static M: OnceLock<Memo> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// How long a failure is remembered before the server is asked again.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// The `.kicad_sym` the HTTP library of `nickname` (its settings file at `settings`) is read as, fetched when the cache has none of the settings' present
/// state younger than `timeout_categories_seconds`. `base` reads a part's `symbolIdStr`. `Err` is KiCad's message.
pub(crate) fn readable(nickname: &str, settings: &Path, base: &dyn Fn(&str) -> Option<LibrarySymbol>) -> Result<PathBuf, String> {
    readable_with(nickname, settings, &crate::library_convert::cache_dir(), base, &fetch)
}

pub(crate) fn readable_with(nickname: &str, settings: &Path, cache: &Path, base: &dyn Fn(&str) -> Option<LibrarySymbol>, fetch: &dyn Fn(&Source) -> Result<Vec<(Category, Vec<Part>)>, String>) -> Result<PathBuf, String> {
    let src = read_settings(settings)?;
    let state = std::fs::read_to_string(settings).unwrap_or_default();
    let key = settings.to_path_buf();
    let now = SystemTime::now();
    if let Some((s, at, answer)) = memo().lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
        let age = now.duration_since(*at).unwrap_or_default();
        let fresh = match answer {
            Ok(p) => p.is_file() && age < Duration::from_secs(src.timeout_categories),
            Err(_) => age < RETRY_AFTER,
        };
        if *s == state && fresh {
            return answer.clone();
        }
    }
    let answer = (|| {
        let lib = fetch(&src)?;
        let symbols: Vec<LibrarySymbol> = lib.iter().flat_map(|(_, parts)| parts.iter().map(|p| symbol_from_part(nickname, p, base))).collect();
        let refs: Vec<&LibrarySymbol> = symbols.iter().collect();
        let text = eda_kicad::export_kicad_sym_library(&refs);
        std::fs::create_dir_all(cache).map_err(|e| format!("Cannot create the library cache {}: {e}", cache.display()))?;
        let name = format!("http-{}-{}.kicad_sym", lt::fix_nickname(nickname).replace([' ', '/'], "_"), &blake3::hash(settings.display().to_string().as_bytes()).to_hex()[..16]);
        let target = cache.join(name);
        let tmp = target.with_extension("kicad_sym.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(target)
    })();
    memo().lock().unwrap_or_else(PoisonError::into_inner).insert(key, (state, now, answer.clone()));
    answer
}

/// What the row of an HTTP library says in the table dialog: the last failure, while it is remembered.
pub(crate) fn status(settings: &Path) -> Option<String> {
    match memo().lock().unwrap_or_else(PoisonError::into_inner).get(settings) {
        Some((_, _, Err(e))) => Some(e.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda-httplib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A REST server of KiCad's HTTP library API on a local port, answering a few requests from a table of paths; the token it was sent is checked.
    fn serve(routes: Vec<(&'static str, &'static str)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    continue;
                }
                let mut auth = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if line.to_lowercase().starts_with("authorization:") {
                        auth = line.trim().to_string();
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
                let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, b)) if auth.ends_with("Token secret") => ("200 OK", b.to_string()),
                    Some(_) => ("401 Unauthorized", String::new()),
                    None => ("404 Not Found", String::new()),
                };
                let mut out = stream;
                let _ = write!(out, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        format!("http://127.0.0.1:{port}/api")
    }

    #[test]
    fn the_settings_are_read_with_kicads_messages() {
        let dir = temp("settings");
        let f = dir.join("W5.kicad_httplib");
        let write = |s: &str| std::fs::write(&f, s).unwrap();
        write(r#"{"meta":{"version":1.0},"name":"W5","source":{"type":"REST_API","api_version":"v1","root_url":"http://x/api","token":"t"}}"#);
        assert_eq!(read_settings(&f).unwrap(), Source { root_url: "http://x/api/v1/".into(), token: "t".into(), timeout_parts: 30, timeout_categories: 600 });
        write(r#"{"source":{"type":"REST_API","root_url":"http://x/api"}}"#);
        assert!(read_settings(&f).unwrap_err().ends_with("is missing the API version number."));
        write(r#"{"source":{"type":"REST_API","api_version":"v2","root_url":"http://x/"}}"#);
        assert!(read_settings(&f).unwrap_err().ends_with("uses API version v2, but KiCad requires version v1."));
        write(r#"{"source":{"type":"REST_API","api_version":"v1"}}"#);
        assert!(read_settings(&f).unwrap_err().ends_with("is missing the root URL."));
        write(r#"{"source":{"type":"SQL","api_version":"v1","root_url":"http://x/"}}"#);
        assert!(read_settings(&f).unwrap_err().ends_with("has invalid library type."));
        assert!(read_settings(&dir.join("none.kicad_httplib")).unwrap_err().ends_with("missing or invalid."));
        assert!(bool_from_string("Yes", false) && !bool_from_string("n", true) && bool_from_string("maybe", true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole of a read over a local REST server: the categories, the parts in full, each part the symbol its `symbolIdStr` names renamed after it with its
    /// fields, flags and filters, an unknown symbol an empty one; written as a library every reader reads; a refused token is KiCad's error.
    #[test]
    fn an_http_library_is_read_from_its_rest_server_into_a_symbol_library() {
        let url = serve(vec![
            ("/api/v1/", r#"{"categories":"","parts":""}"#),
            ("/api/v1/categories.json", r#"[{"id":"16","name":"Resistors","description":"Fixed resistors"}]"#),
            ("/api/v1/parts/category/16.json", r#"[{"id":"1","name":"RC0603 10k","description":"10k 1%"},{"id":"2","name":"Odd:Part"}]"#),
            (
                "/api/v1/parts/1.json",
                r#"{"id":"1","name":"RC0603 10k","symbolIdStr":"Device:R","exclude_from_bom":"False","exclude_from_board":"no","exclude_from_sim":"yes","description":"10k 1% 0603","keywords":"res 10k","fields":{"footprint":{"value":"Resistor_SMD:R_0603;R_*"},"value":{"value":"10k"},"MPN":{"value":"RC0603FR-0710KL","visible":"False"}}}"#,
            ),
            ("/api/v1/parts/2.json", r#"{"id":"2","name":"Odd:Part","symbolIdStr":"Nowhere:X","fields":{}}"#),
        ]);
        // the settings' URL is the server's root without the version (`root_url += "/" + api_version + "/"`), and the tests' server checks the token
        let dir = temp("read");
        let settings = dir.join("W5.kicad_httplib");
        let write = |token: &str| std::fs::write(&settings, format!(r#"{{"source":{{"type":"REST_API","api_version":"v1","root_url":"{url}","token":"{token}","timeout_categories_seconds":600}}}}"#)).unwrap();
        write("secret");
        let mut device = LibrarySymbol::new_empty("Device:R");
        device.reference_prefix = "R".into();
        device.description = "Resistor".into();
        let base = |id: &str| (id == "Device:R").then(|| device.clone());
        let path = readable_with("W5", &settings, &dir.join("cache"), &base, &fetch).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lib = eda_kicad::parse_library_symbols(&text).unwrap();
        let names: Vec<&str> = lib.symbols.iter().map(|s| s.lib_id.as_str()).collect();
        assert!(names.iter().any(|n| n.ends_with("RC0603 10k")) && names.iter().any(|n| n.ends_with("Odd_Part")), "{names:?}");
        let r = lib.symbols.iter().find(|s| s.lib_id.ends_with("RC0603 10k")).unwrap();
        assert_eq!(r.reference_prefix, "R", "the symbol symbolIdStr names");
        assert_eq!(r.description, "10k 1% 0603", "the part's description over the symbol's");
        assert_eq!(r.keywords, "res 10k");
        assert!(r.in_bom && r.on_board && r.exclude_from_sim);
        assert_eq!(r.field("Footprint").unwrap().text, "Resistor_SMD:R_0603");
        assert_eq!(r.field("Value").unwrap().text, "10k");
        let mpn = r.field("MPN").unwrap();
        assert!(mpn.text == "RC0603FR-0710KL" && mpn.hidden, "a user field with its visibility");
        assert_eq!(r.footprint_filters, ["Resistor_SMD:R_0603", "R_*"]);
        let odd = lib.symbols.iter().find(|s| s.lib_id.ends_with("Odd_Part")).unwrap();
        assert!(odd.pins.is_empty() && odd.graphics.is_empty(), "a symbol no library has: metadata only");
        // a refused token: KiCad's message, remembered as the row's status
        let settings2 = dir.join("W6.kicad_httplib");
        std::fs::write(&settings2, format!(r#"{{"source":{{"type":"REST_API","api_version":"v1","root_url":"{url}","token":"wrong"}}}}"#)).unwrap();
        let err = readable_with("W6", &settings2, &dir.join("cache"), &base, &fetch).unwrap_err();
        assert!(err.contains("API responded with error code: 401: Unauthorized"), "{err}");
        assert_eq!(status(&settings2).as_deref(), Some(err.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
