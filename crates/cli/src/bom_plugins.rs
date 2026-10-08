//! The legacy BOM generators (`eeschema.EditorControl.generateBOMLegacy`, `DIALOG_BOM`): the scripts KiCad ships in its plugins folder
//! (`bom_csv_grouped_by_value.py`, ...). A generator reads the intermediate XML netlist (`kicad-cli sch export python-bom`) and writes
//! the BOM next to it. This module only reads the scripts and builds the command line the way `BOM_GENERATOR_HANDLER`
//! (eeschema/bom_plugins.cpp at 8303b2ad) does -- running one is `sch_export_api::bom_legacy`'s job, behind the kicad-cli lane.
//!
//! Only the scripts of KiCad's own plugins folder are offered: the dialog's "Add generator" and its editable command line would let the browser
//! run any program on this machine, which this server never does for a request.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// One generator script found in the plugins folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// The script's file name (`bom_csv_grouped_by_value.py`): what a request names it by.
    pub file: String,
    /// `py` or `xsl`.
    pub kind: String,
    /// The text between `@package` and the end of the script's header comment (`BOM_GENERATOR_HANDLER::readHeader`): what the script says it does.
    pub info: String,
    /// The extension the header's command line gives the output (`"%O.csv"` -> `.csv`), empty when it names none.
    pub ext: String,
}

impl Plugin {
    /// The plugin's name in the dialog's list: the file without its extension (`BOM_GENERATOR_HANDLER::m_name`).
    pub fn name(&self) -> &str {
        self.file.rsplit_once('.').map_or(self.file.as_str(), |(n, _)| n)
    }

    /// The command line as the dialog shows it, with `%I` (the intermediate netlist) and `%O` (the output without its extension) left in.
    pub fn command_template(&self, script: &Path) -> String {
        match self.kind.as_str() {
            "xsl" => format!("xsltproc -o \"%O{}\" \"{}\" \"%I\"", self.ext, script.display()),
            _ => format!("python3 \"{}\" \"%I\" \"%O{}\"", script.display(), self.ext),
        }
    }

    /// The program and arguments that run the script on `xml`, writing `<out_base><ext>`: the command line above, without a shell.
    pub fn invocation(&self, script: &Path, python: &str, xml: &Path, out_base: &Path) -> (String, Vec<String>) {
        let out = format!("{}{}", out_base.display(), self.ext);
        match self.kind.as_str() {
            "xsl" => ("xsltproc".to_string(), vec!["-o".into(), out, script.display().to_string(), xml.display().to_string()]),
            _ => (python.to_string(), vec![script.display().to_string(), xml.display().to_string(), out]),
        }
    }
}

/// `BOM_GENERATOR_HANDLER::IsValidGenerator`: the extensions a generator may have on the platforms this server runs on.
pub fn is_generator_file(name: &str) -> bool {
    matches!(name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref(), Some("py") | Some("xsl"))
}

/// `readHeader`: the text between `@package` and `end`, without the blank lines that follow `@package`; empty when either is missing.
pub fn read_header(text: &str, end: &str) -> String {
    let Some(at) = text.find("@package") else { return String::new() };
    let start = at + "@package".len();
    let Some(len) = text[start..].find(end) else { return String::new() };
    let body = &text[start..start + len];
    body.trim_start_matches(|c: char| c < ' ').to_string()
}

/// `getOutputExtension`: the extension after `"%O` in the header, up to the closing quote (it includes the dot); empty when there is none.
pub fn output_extension(header: &str) -> String {
    const ARG: &str = "\"%O";
    let Some(at) = header.find(ARG) else { return String::new() };
    let rest = &header[at + ARG.len()..];
    rest.find('"').map_or_else(String::new, |end| rest[..end].to_string())
}

/// A generator script's header and output extension (`BOM_GENERATOR_HANDLER`'s constructor).
pub fn parse(file: &str, text: &str) -> Plugin {
    let kind = file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let info = match kind.as_str() {
        "xsl" => read_header(text, "-->"),
        _ => read_header(text, "\"\"\""),
    };
    let ext = output_extension(&info);
    Plugin { file: file.to_string(), kind, info, ext }
}

/// Every generator script in `dir`, by file name.
pub fn list(dir: &Path) -> Vec<(Plugin, PathBuf)> {
    let mut found: Vec<(Plugin, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            if !is_generator_file(&file) || !path.is_file() {
                return None;
            }
            // `kicad_netlist_reader.py` and `kicad_utils.py` are the generators' own helper modules, not generators: no `@package` header.
            let text = std::fs::read_to_string(&path).ok()?;
            let plugin = parse(&file, &text);
            (!plugin.info.is_empty()).then_some((plugin, path))
        })
        .collect();
    found.sort_by(|a, b| a.0.file.cmp(&b.0.file));
    found
}

/// `GET /api/sch/bom_plugins`: `{ok, dir, plugins: [{name, file, kind, info, command, ext}]}`, or `{ok: false, message}` when this machine's KiCad has no plugins folder.
pub fn listing() -> Value {
    let Some(dir) = eda_kicad_engine::bom_plugins_dir() else {
        return json!({ "ok": false, "message": "KiCad's plugins folder was not found (set EDA_KICAD_PLUGINS to the folder that holds bom_csv_grouped_by_value.py and its siblings)" });
    };
    let plugins: Vec<Value> = list(&dir)
        .into_iter()
        .map(|(p, path)| json!({ "name": p.name(), "file": p.file, "kind": p.kind, "info": p.info, "command": p.command_template(&path), "ext": p.ext }))
        .collect();
    json!({ "ok": true, "dir": dir.to_string_lossy(), "plugins": plugins })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PY: &str = "#\n# Example\n#\n\n\"\"\"\n    @package\n    Output: CSV (comma-separated)\n\n    Command line:\n    python \"pathToFile/bom_csv_grouped_by_value.py\" \"%I\" \"%O.csv\"\n\"\"\"\n\nimport sys\n";

    #[test]
    fn the_header_is_what_follows_package_up_to_the_closing_quotes() {
        let p = parse("bom_csv_grouped_by_value.py", PY);
        assert!(p.info.starts_with("    Output: CSV"), "{:?}", p.info);
        assert!(p.info.contains("Command line:"));
        assert!(!p.info.contains("\"\"\""));
        assert_eq!(p.ext, ".csv");
        assert_eq!(p.name(), "bom_csv_grouped_by_value");
    }

    #[test]
    fn a_script_without_a_header_has_none_and_no_extension() {
        let p = parse("kicad_utils.py", "import sys\n");
        assert!(p.info.is_empty());
        assert_eq!(p.ext, "");
        assert_eq!(read_header("@package\nnever closed", "\"\"\""), "");
    }

    #[test]
    fn an_xsl_header_ends_at_the_comment_close_and_runs_through_xsltproc() {
        let xsl = "<?xml version=\"1.0\"?>\n<!--\n    @package\n    Generate a HTML BOM list.\n    xsltproc -o \"%O.html\" \"/x/bom2html.xsl\" \"%I\"\n-->\n";
        let p = parse("bom2html.xsl", xsl);
        assert_eq!(p.kind, "xsl");
        assert_eq!(p.ext, ".html");
        let (program, args) = p.invocation(Path::new("/x/bom2html.xsl"), "python3", Path::new("/out/b.xml"), Path::new("/out/b"));
        assert_eq!(program, "xsltproc");
        assert_eq!(args, vec!["-o", "/out/b.html", "/x/bom2html.xsl", "/out/b.xml"]);
        assert_eq!(p.command_template(Path::new("/x/bom2html.xsl")), "xsltproc -o \"%O.html\" \"/x/bom2html.xsl\" \"%I\"");
    }

    #[test]
    fn a_python_generator_gets_the_netlist_and_the_output_name() {
        let p = parse("g.py", PY);
        let (program, args) = p.invocation(Path::new("/p/g.py"), "python3", Path::new("/out/b.xml"), Path::new("/out/b"));
        assert_eq!(program, "python3");
        assert_eq!(args, vec!["/p/g.py", "/out/b.xml", "/out/b.csv"]);
        assert_eq!(p.command_template(Path::new("/p/g.py")), "python3 \"/p/g.py\" \"%I\" \"%O.csv\"");
    }

    #[test]
    fn only_scripts_count_as_generators() {
        assert!(is_generator_file("a.py") && is_generator_file("B.XSL"));
        assert!(!is_generator_file("README-bom.txt") && !is_generator_file("noext"));
    }

    #[test]
    fn the_listing_skips_helper_modules_and_sorts_by_file() {
        let dir = std::env::temp_dir().join(format!("eda-bom-plugins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b_gen.py"), PY).unwrap();
        std::fs::write(dir.join("a_gen.py"), PY).unwrap();
        std::fs::write(dir.join("kicad_utils.py"), "import os\n").unwrap();
        std::fs::write(dir.join("README-bom.txt"), "hello").unwrap();
        let names: Vec<String> = list(&dir).into_iter().map(|(p, _)| p.file).collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names, vec!["a_gen.py", "b_gen.py"]);
    }
}
