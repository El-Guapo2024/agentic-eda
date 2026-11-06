//! CvPcb's footprint association files (`.equ`) and its Automatically Assign Footprints, the pure half: reading a file and matching the symbols that have no
//! footprint to the footprints the files name by value. Ported from KiCad 25ec2335's cvpcb/auto_associate.cpp (`GetQuotedText`,
//! `CVPCB_MAINFRAME::buildEquivalenceList`, `sortListbyCmpValue`, `CVPCB_MAINFRAME::AutomaticFootprintMatching`); the cleaned 8303b2ad tree has no `cvpcb/`.
//!
//! An association file is lines of two quoted texts, a symbol's value and a footprint's `Lib:Name` (`'FT232BL'  'QFP:LQFP-32_7x7mm_Pitch0.8mm'`); a line starting
//! with `#` and a blank line are skipped (`GetLine`), and so is a line without two quoted texts. A space in the value is read as `_`.
//!
//! The project lists its files (`PROJECT_FILE::m_EquivalenceFiles`, the project file's `cvpcb.equivalence_files`, each path with `${VAR}`s); where they are and
//! which footprints the libraries hold is the caller's (`crates/cli/src/cvpcb_api.rs`): this module is given each file's text, or why it could not be read, and
//! a test of whether a `Lib:Name` is a footprint of the libraries (`FOOTPRINT_LIST::GetFootprintInfo`).
//!
//! **Differs**: `std::sort` with `sortListbyCmpValue` leaves the order of two entries of one value to the sort; here they keep the order of the files and of
//! the lines (a stable sort), which is the order KiCad's sort gives a list of 16 or fewer (an insertion sort) and the order the person wrote. A line longer
//! than KiCad's 1023-byte buffer is read whole here (`fgets` would split it).

/// One association (`FOOTPRINT_EQUIVALENCE`): a symbol value and the footprint it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Equivalence {
    /// `m_ComponentValue`, with its spaces made `_`.
    pub value: String,
    /// `m_FootprintFPID`, as the file writes it.
    pub footprint: String,
}

/// `GetQuotedText`: the text between the first two `'` of `text` and what follows the second, or `None` (KiCad's empty string) when there are not two.
pub fn quoted_text(text: &str) -> Option<(&str, &str)> {
    let open = text.find('\'')?;
    let rest = &text[open + 1..];
    let close = rest.find('\'')?;
    Some((&rest[..close], &rest[close + 1..]))
}

/// The associations of one file's text, in the file's order (`buildEquivalenceList`'s loop over `GetLine`).
pub fn parse_equ(text: &str) -> Vec<Equivalence> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        // `GetLine`: lines starting with '#', '\n' or '\r' are skipped; `strtok( Line, "\n\r" )` cuts the line at its end
        if line.is_empty() || line.starts_with('#') || line.starts_with('\r') {
            continue;
        }
        let line = line.split('\r').next().unwrap_or("");
        let Some((value, rest)) = quoted_text(line) else { continue };
        if value.is_empty() {
            continue;
        }
        let Some((footprint, _)) = quoted_text(rest) else { continue };
        if footprint.is_empty() {
            continue;
        }
        out.push(Equivalence { value: value.replace(' ', "_"), footprint: footprint.to_string() });
    }
    out
}

/// One association file as the caller found it: its text, or the reason `buildEquivalenceList` gives for not reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquFile {
    Text(String),
    /// The path did not lead to a file: "Equivalence file '%s' could not be found." with the file's name (`fn.GetFullName()`).
    NotFound { name: String },
    /// The file is there and could not be opened: "Error opening equivalence file '%s'." with its full path.
    Unreadable { path: String },
}

/// `buildEquivalenceList`: every association of every file, in order, and the load errors (KiCad's messages, joined by a blank line as `aErrorMessages` is).
pub fn build_equivalence_list(files: &[EquFile]) -> (Vec<Equivalence>, Option<String>) {
    let mut list = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for f in files {
        match f {
            EquFile::Text(text) => list.extend(parse_equ(text)),
            EquFile::NotFound { name } => errors.push(format!("Equivalence file '{name}' could not be found.")),
            EquFile::Unreadable { path } => errors.push(format!("Error opening equivalence file '{path}'.")),
        }
    }
    (list, (!errors.is_empty()).then(|| errors.join("\n\n")))
}

/// A symbol of the netlist CvPcb works on (`COMPONENT`): its reference, its value, the footprint it has now (`GetFPID`, an assignment made in the dialog
/// included) and its library symbol's footprint filters (`GetFootprintFilters`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub filters: Vec<String>,
}

/// What Automatically Assign Footprints did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matching {
    /// The symbols given a footprint, in the netlist's order (`AssociateFootprint( CVPCB_ASSOCIATION( kk, fpid ) )`).
    pub assignments: Vec<(String, String)>,
    /// How many associations the files hold (the status text "%lu footprint/symbol equivalences found.").
    pub found: usize,
    /// "Equivalence File Load Error": the files that could not be read.
    pub load_errors: Option<String>,
    /// "CvPcb Warning": the associations whose footprint no library has.
    pub warnings: Option<String>,
}

/// `wxString::Matches`: `*` any run, `?` any one character, over the whole text, case and all.
pub fn wx_matches(text: &str, pattern: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// `wxString::CmpNoCase( b ) == 0`.
fn same_no_case(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_lowercase).eq(b.chars().flat_map(char::to_lowercase))
}

/// The footprint's name of a `Lib:Name` (`FOOTPRINT_INFO::GetFootprintName`).
fn item_name(fpid: &str) -> &str {
    fpid.split_once(':').map_or(fpid, |(_, n)| n)
}

/// `CVPCB_MAINFRAME::AutomaticFootprintMatching`. For each symbol with no footprint, in order:
///
/// 1. the associations of its value (`CmpNoCase`) are tried in the list's order (sorted by value, `sortListbyCmpValue`, descending): an association whose
///    footprint the libraries have and whose value no neighbour in the sorted list shares (`equ_is_unique`, compared case and all) is taken at once;
/// 2. of several for one value, the first whose footprint's name matches one of the symbol's footprint filters (`wxString::Matches`) is taken, or the first
///    with a footprint at all when the symbol has no filter; else the first with a footprint the libraries have (`fpid_candidate`);
/// 3. failing all, a symbol with exactly one filter that is itself a footprint of the libraries gets it ("obviously the last chance").
///
/// An association whose footprint no library has is a warning: "Component %s: footprint %s not found in any of the project footprint libraries."
pub fn automatic_matching(components: &[Component], files: &[EquFile], exists: impl Fn(&str) -> bool) -> Matching {
    let (mut list, load_errors) = build_equivalence_list(files);
    // `std::sort( ..., sortListbyCmpValue )`: `ref.m_ComponentValue.Cmp( test.m_ComponentValue ) > 0`, the greater value first (a stable sort here; see the module doc)
    list.sort_by(|a, b| b.value.cmp(&a.value));
    let mut out = Matching { found: list.len(), load_errors, ..Default::default() };
    let mut warnings: Vec<String> = Vec::new();
    for c in components {
        if !c.footprint.is_empty() {
            continue; // the component has already a footprint
        }
        let mut found = false;
        let mut candidate: Option<&str> = None;
        let mut chosen: Option<&str> = None;
        for (idx, e) in list.iter().enumerate() {
            if !same_no_case(&e.value, &c.value) {
                continue;
            }
            let fp = exists(&e.footprint);
            let unique = !(list.get(idx + 1).is_some_and(|n| n.value == e.value) || (idx > 0 && list[idx - 1].value == e.value));
            if fp && unique {
                chosen = Some(&e.footprint);
                found = true;
                break;
            }
            if fp && candidate.is_none() {
                candidate = Some(&e.footprint);
            }
            if fp {
                // "if no entries, do not filter"
                found = c.filters.is_empty() || c.filters.iter().any(|f| wx_matches(item_name(&e.footprint), f));
            } else {
                warnings.push(format!("Component {}: footprint {} not found in any of the project footprint libraries.", c.reference, e.footprint));
            }
            if found {
                chosen = Some(&e.footprint);
                break;
            }
        }
        if found {
            if let Some(fp) = chosen {
                out.assignments.push((c.reference.clone(), fp.to_string()));
            }
            continue;
        }
        if let Some(fp) = candidate {
            out.assignments.push((c.reference.clone(), fp.to_string()));
            continue;
        }
        // obviously the last chance: there's only one filter matching one footprint
        if let [only] = c.filters.as_slice() {
            if exists(only) {
                out.assignments.push((c.reference.clone(), only.clone()));
            }
        }
    }
    out.warnings = (!warnings.is_empty()).then(|| warnings.join("\n\n"));
    out
}

// ------------------------------------------------------------------------------------------------ the project's list

/// The board folder's file that holds the project's list of association files. KiCad keeps the list in the project file (`PROJECT_FILE::m_EquivalenceFiles`,
/// `cvpcb.equivalence_files`) and `DIALOG_CONFIG_EQUFILES::OnOkClick` saves the project at once (`SaveProject`), outside the undo list; the studio's derived
/// project file is written again on every run, so the list is kept here (`{"version": 1, "equivalence_files": [...]}`), as the folder's `path-variables.json` keeps
/// the project's path variables, and [`derive_into_project`] gives it to the derived project.
pub const CVPCB_FILE: &str = "cvpcb.json";

/// The project's list in the board folder `dir`, the paths as written, in order; empty when there is none.
pub fn equivalence_files(dir: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join(CVPCB_FILE)) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    v.get("equivalence_files").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Store the list (an empty one removes the file), through a temporary file and a rename.
pub fn write_equivalence_files(dir: &std::path::Path, files: &[String]) -> Result<(), String> {
    let path = dir.join(CVPCB_FILE);
    if files.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("could not remove {}: {e}", path.display())),
        };
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({ "version": 1, "equivalence_files": files })).unwrap_or_default() + "\n";
    let tmp = dir.join(format!("{CVPCB_FILE}.tmp"));
    std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write {}: {e}", path.display())
    })
}

/// The derived `.kicad_pro` (`pro`) of the board in `board_dir` gets the board's list as `cvpcb.equivalence_files`, each entry that is relative or under
/// `${KIPRJMOD}` written out whole: the derived project is in `<board>/.kicad`, and `wxExpandEnvVars` reads `KIPRJMOD` from the environment, which KiCad sets to
/// that folder. Returns whether anything was added.
pub fn derive_into_project(board_dir: &std::path::Path, pro: &mut serde_json::Value) -> bool {
    let files = equivalence_files(board_dir);
    if files.is_empty() || !pro.is_object() {
        return false;
    }
    let folder = board_dir.display().to_string();
    let written: Vec<String> = files
        .iter()
        .map(|f| {
            let f = f.replace("${KIPRJMOD}", &folder).replace("$(KIPRJMOD)", &folder);
            if f.starts_with('$') || std::path::Path::new(&f).is_absolute() {
                f
            } else {
                board_dir.join(&f).display().to_string()
            }
        })
        .collect();
    if !pro.get("cvpcb").is_some_and(serde_json::Value::is_object) {
        pro["cvpcb"] = serde_json::json!({});
    }
    pro["cvpcb"]["equivalence_files"] = serde_json::json!(written);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_kept_beside_the_board_and_the_derived_project_gets_it_written_out_whole() {
        let dir = std::env::temp_dir().join(format!("eda-cvpcb-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut pro = serde_json::json!({ "meta": { "version": 1 } });
        assert!(equivalence_files(&dir).is_empty() && !derive_into_project(&dir, &mut pro));
        let files: Vec<String> = ["${KIPRJMOD}/mine.equ", "rel/two.equ", "/abs/three.equ", "${KICAD10_FOOTPRINT_DIR}/x.equ"].map(String::from).to_vec();
        write_equivalence_files(&dir, &files).unwrap();
        assert_eq!(equivalence_files(&dir), files);
        assert!(derive_into_project(&dir, &mut pro));
        let d = dir.display().to_string();
        assert_eq!(pro["cvpcb"]["equivalence_files"], serde_json::json!([format!("{d}/mine.equ"), format!("{d}/rel/two.equ"), "/abs/three.equ", "${KICAD10_FOOTPRINT_DIR}/x.equ"]));
        assert_eq!(pro["meta"]["version"], 1);
        write_equivalence_files(&dir, &[]).unwrap();
        assert!(!dir.join(CVPCB_FILE).exists(), "an empty list leaves no file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const FILE: &str = "# footprint associations\n\
        'FT232BL'\t\t'Package_QFP:LQFP-32_7x7mm_P0.8mm'\n\
        \n\
        '10 k' 'Resistor_SMD:R_0603_1608Metric'\r\n\
        'LED'   'LED_SMD:LED_0603_1608Metric'\n\
        'LED'   'LED_THT:LED_D5.0mm'\n\
        'only one quote\n\
        'NoFootprint' ''\n\
        'GHOST' 'Nowhere:Nothing'\n";

    fn comp(r: &str, v: &str, fp: &str, filters: &[&str]) -> Component {
        Component { reference: r.into(), value: v.into(), footprint: fp.into(), filters: filters.iter().map(|s| s.to_string()).collect() }
    }

    fn libs(id: &str) -> bool {
        ["Package_QFP:LQFP-32_7x7mm_P0.8mm", "Resistor_SMD:R_0603_1608Metric", "LED_SMD:LED_0603_1608Metric", "LED_THT:LED_D5.0mm", "Crystal:Crystal_SMD_3225-4Pin_3.2x2.5mm"].contains(&id)
    }

    #[test]
    fn a_file_is_read_as_get_quoted_text_reads_it() {
        assert_eq!(quoted_text("  'a b'  'c'"), Some(("a b", "  'c'")));
        assert_eq!(quoted_text("no quotes"), None);
        assert_eq!(quoted_text("'open"), None);
        let list = parse_equ(FILE);
        let pairs: Vec<(&str, &str)> = list.iter().map(|e| (e.value.as_str(), e.footprint.as_str())).collect();
        assert_eq!(
            pairs,
            vec![
                ("FT232BL", "Package_QFP:LQFP-32_7x7mm_P0.8mm"),
                ("10_k", "Resistor_SMD:R_0603_1608Metric"),
                ("LED", "LED_SMD:LED_0603_1608Metric"),
                ("LED", "LED_THT:LED_D5.0mm"),
                ("GHOST", "Nowhere:Nothing"),
            ],
            "comments, blank lines, a line without two quoted texts and an empty footprint are skipped; a value's space is '_'; a \\r ends a line"
        );
    }

    #[test]
    fn the_files_that_cannot_be_read_are_reported_in_kicads_words() {
        let (list, errors) = build_equivalence_list(&[EquFile::NotFound { name: "mine.equ".into() }, EquFile::Text(FILE.into()), EquFile::Unreadable { path: "/p/locked.equ".into() }]);
        assert_eq!(list.len(), 5);
        assert_eq!(errors.as_deref(), Some("Equivalence file 'mine.equ' could not be found.\n\nError opening equivalence file '/p/locked.equ'."));
        assert_eq!(build_equivalence_list(&[]).1, None);
    }

    #[test]
    fn wx_matches_is_a_whole_text_wildcard_with_case() {
        assert!(wx_matches("R_0603_1608Metric", "R_*"));
        assert!(wx_matches("R_0603_1608Metric", "R_0603?1608*"));
        assert!(!wx_matches("R_0603_1608Metric", "r_*"));
        assert!(!wx_matches("R_0603_1608Metric", "0603"));
        assert!(wx_matches("anything", "*"));
    }

    #[test]
    fn a_unique_value_is_assigned_and_an_assigned_symbol_is_left_alone() {
        let comps = [comp("U1", "ft232bl", "", &[]), comp("U2", "FT232BL", "Package_QFP:Other", &[]), comp("R1", "10_k", "", &["R_*"]), comp("R2", "10 k", "", &[])];
        let m = automatic_matching(&comps, &[EquFile::Text(FILE.into())], libs);
        assert_eq!(m.found, 5);
        assert_eq!(
            m.assignments,
            vec![("U1".to_string(), "Package_QFP:LQFP-32_7x7mm_P0.8mm".to_string()), ("R1".to_string(), "Resistor_SMD:R_0603_1608Metric".to_string())],
            "the value is compared without case; a symbol with a footprint is skipped; the schematic's '10 k' is not the file's '10_k'"
        );
        assert_eq!(m.warnings, None);
    }

    #[test]
    fn an_ambiguous_value_is_settled_by_the_footprint_filters_else_the_first_candidate() {
        let files = [EquFile::Text(FILE.into())];
        // the filter keeps the THT LED
        let m = automatic_matching(&[comp("D1", "LED", "", &["LED*D5.0mm*"])], &files, libs);
        assert_eq!(m.assignments, vec![("D1".to_string(), "LED_THT:LED_D5.0mm".to_string())]);
        // no filter: the first of the value's entries (in the files' order) is taken
        let m = automatic_matching(&[comp("D2", "LED", "", &[])], &files, libs);
        assert_eq!(m.assignments, vec![("D2".to_string(), "LED_SMD:LED_0603_1608Metric".to_string())]);
        // a filter that keeps neither: the first candidate
        let m = automatic_matching(&[comp("D3", "LED", "", &["Diode_*"])], &files, libs);
        assert_eq!(m.assignments, vec![("D3".to_string(), "LED_SMD:LED_0603_1608Metric".to_string())]);
    }

    #[test]
    fn a_footprint_no_library_has_is_a_warning_and_one_filter_that_is_a_footprint_is_the_last_chance() {
        let files = [EquFile::Text(FILE.into())];
        let m = automatic_matching(&[comp("X1", "GHOST", "", &[]), comp("Y1", "16MHz", "", &["Crystal:Crystal_SMD_3225-4Pin_3.2x2.5mm"]), comp("Y2", "16MHz", "", &["Crystal*", "Other*"])], &files, libs);
        assert_eq!(m.assignments, vec![("Y1".to_string(), "Crystal:Crystal_SMD_3225-4Pin_3.2x2.5mm".to_string())]);
        assert_eq!(m.warnings.as_deref(), Some("Component X1: footprint Nowhere:Nothing not found in any of the project footprint libraries."));
    }

    #[test]
    fn the_list_is_sorted_by_value_greatest_first_and_two_files_of_one_value_are_ambiguous() {
        let a = EquFile::Text("'C1' 'LED_SMD:LED_0603_1608Metric'\n".into());
        let b = EquFile::Text("'C1' 'LED_THT:LED_D5.0mm'\n'B' 'Resistor_SMD:R_0603_1608Metric'\n".into());
        let m = automatic_matching(&[comp("D1", "c1", "", &["LED_D5*"]), comp("R1", "B", "", &[])], &[a, b], libs);
        assert_eq!(m.assignments, vec![("D1".to_string(), "LED_THT:LED_D5.0mm".to_string()), ("R1".to_string(), "Resistor_SMD:R_0603_1608Metric".to_string())]);
    }
}
