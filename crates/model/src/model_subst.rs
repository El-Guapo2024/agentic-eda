//! Replacing an obsolete VRML model reference with the STEP model that stands for it: what KiCad's Migrate 3D Models dialog (`pcbnew/dialogs/dialog_migrate_3d_models.cpp`) ranks and
//! what `MODEL_SUBSTITUTION` (`3d-viewer/3d_cache/model_substitution_helpers.cpp`) matches. No file system here: a catalog is a list of the STEP-family files somebody found
//! ([`CatalogEntry`]), and the functions rank and pick among them. The walk of the folders and the verb that rewrites the footprints are `eda_cli`'s
//! (`crates/cli/src/model3d_tab_api.rs`) and `eda_ops`'s (`Cmd::MigrateBoardModels`).
//!
//! A reference is "missing" when it names a `.wrl` or `.wrz` file that is not there. Its candidates are the catalog's files whose normalised stem is the same (`R_0603_1608Metric` is
//! `R-0603-1608Metric`, whatever the case and the extension) -- 1000 points when they sit in a folder of the same name as the reference's, 800 when not -- and those a few edits
//! away (a quarter of the stem, at least two: 500 less ten for each edit): the first fifteen by points, then by name.

/// The extensions KiCad takes for "current" replacements, in the order it prefers them (`stepExtensions`).
pub const STEP_EXTENSIONS: [&str; 7] = ["step", "stp", "stpz", "step.gz", "stp.gz", "iges", "igs"];

/// How many candidates a missing reference is offered (`kMaxCandidates`).
pub const MAX_CANDIDATES: usize = 15;

/// `IsWrlExtension`: the part after the last dot, in lower case, is `wrl` or `wrz`.
pub fn is_wrl(file_name: &str) -> bool {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    file_name.contains('.') && (ext == "wrl" || ext == "wrz")
}

/// `hasStepExtension`: the path ends in one of [`STEP_EXTENSIONS`] with a name before it.
pub fn has_step_extension(path: &str) -> bool {
    let lower = path.to_lowercase();
    STEP_EXTENSIONS.iter().any(|e| {
        let dotted = format!(".{e}");
        lower.len() > dotted.len() && lower.ends_with(&dotted)
    })
}

/// The last part of a path, either separator (`wxFileName::GetFullName`).
pub fn full_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `normalizeStem`: the file name without its 3D extension (`.step.gz`, `.stp.gz`, `.wrl`, `.wrz`, `.step`, `.stp`, `.stpz`, `.iges`, `.igs`, the first that fits, and only when a name is
/// left), in lower case, `-` and spaces turned into `_` -- so `R_0603_1608Metric` and `R-0603-1608Metric` are one stem.
pub fn normalize_stem(file_name: &str) -> String {
    let mut stem = file_name.to_string();
    for ext in [".step.gz", ".stp.gz", ".wrl", ".wrz", ".step", ".stp", ".stpz", ".iges", ".igs"] {
        if stem.chars().count() > ext.chars().count() && stem.to_lowercase().ends_with(ext) {
            let keep = stem.chars().count() - ext.chars().count();
            stem = stem.chars().take(keep).collect();
            break;
        }
    }
    stem.to_lowercase().replace(['-', ' '], "_")
}

/// `parentDirName`: the name of the folder the file is in -- `Resistor_SMD.3dshapes` for `${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603.wrl`; empty for a bare name.
pub fn parent_dir_name(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    if parts.len() < 2 {
        return String::new();
    }
    parts[parts.len() - 2].to_string()
}

/// The classic edit distance, insertions, deletions and substitutions (`levenshtein`), by characters.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// One STEP-family file somebody found: the path to write in the footprint, its normalised stem and the name of its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub path: String,
    pub stem: String,
    pub parent: String,
}

impl CatalogEntry {
    /// The entry of a file at `path` (`scanDirectory`: the stem of its full name, the folder it is in).
    pub fn of(path: &str) -> Self {
        CatalogEntry { path: path.to_string(), stem: normalize_stem(full_name(path)), parent: parent_dir_name(path) }
    }
}

/// One replacement offered for a missing reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    /// What the list shows: the file's name, a dash and its folder (`MATCH_CANDIDATE::m_display`).
    pub display: String,
    pub score: i32,
}

/// `rankCandidatesFor`: the catalog's files that could stand for `wrl`, best first -- an exact stem (1000 in a folder of the same name, else 800), or one within `max(2, stem length / 4)`
/// edits (500 less ten for each) -- the first [`MAX_CANDIDATES`], ties by display name without regard to case. The "keep existing reference" row the dialog adds at the end is the caller's.
pub fn rank_candidates(wrl: &str, catalog: &[CatalogEntry]) -> Vec<Candidate> {
    let wrl_stem = normalize_stem(full_name(wrl));
    let wrl_parent = parent_dir_name(wrl);
    let max_dist = (wrl_stem.chars().count() / 4).max(2);
    let wrl_len = wrl_stem.chars().count();
    let mut ranked: Vec<Candidate> = Vec::new();
    for entry in catalog {
        let score = if entry.stem == wrl_stem {
            if !wrl_parent.is_empty() && entry.parent.to_lowercase() == wrl_parent.to_lowercase() {
                1000
            } else {
                800
            }
        } else {
            // the distance is at least the difference of the lengths: the files that are too long or too short are not measured
            let len = entry.stem.chars().count();
            if len.abs_diff(wrl_len) > max_dist {
                continue;
            }
            let dist = levenshtein(&entry.stem, &wrl_stem);
            if dist <= max_dist {
                500 - dist as i32 * 10
            } else {
                0
            }
        };
        if score > 0 {
            ranked.push(Candidate { path: entry.path.clone(), display: format!("{}   \u{2014}   {}", full_name(&entry.path), entry.parent), score });
        }
    }
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.display.to_lowercase().cmp(&b.display.to_lowercase())));
    ranked.truncate(MAX_CANDIDATES);
    ranked
}

/// `STEP_CATALOG::FindMatchFor`: the file the catalog has under the same normalised stem, the one in a folder of the same name as the reference's when there is one, else the first. `None` for
/// a name that is not a `.wrl` or `.wrz`, and for a stem the catalog has no file under.
pub fn find_match<'a>(wrl: &str, catalog: &'a [CatalogEntry]) -> Option<&'a CatalogEntry> {
    if !is_wrl(wrl) {
        return None;
    }
    let stem = normalize_stem(full_name(wrl));
    if stem.is_empty() {
        return None;
    }
    let same: Vec<&CatalogEntry> = catalog.iter().filter(|e| e.stem == stem).collect();
    let parent = parent_dir_name(wrl);
    if !parent.is_empty() {
        if let Some(e) = same.iter().find(|e| e.parent.to_lowercase() == parent.to_lowercase()) {
            return Some(e);
        }
    }
    same.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(paths: &[&str]) -> Vec<CatalogEntry> {
        paths.iter().map(|p| CatalogEntry::of(p)).collect()
    }

    #[test]
    fn a_file_name_is_a_wrl_by_the_extension_after_its_last_dot() {
        for yes in ["a.wrl", "A.WRL", "dir/x.y.wrz", "${V}/R.Wrz"] {
            assert!(is_wrl(yes), "{yes}");
        }
        for no in ["a.step", "wrl", "a.wrl.step", "", "a."] {
            assert!(!is_wrl(no), "{no}");
        }
        assert!(has_step_extension("a/b.STEP") && has_step_extension("b.stp.gz") && has_step_extension("x.igs"));
        assert!(!has_step_extension(".step") && !has_step_extension("a.wrl") && !has_step_extension("step"));
    }

    #[test]
    fn a_stem_has_no_extension_no_case_no_dashes() {
        assert_eq!(normalize_stem("R_0603_1608Metric.wrl"), "r_0603_1608metric");
        assert_eq!(normalize_stem("R-0603 1608Metric.STEP"), "r_0603_1608metric");
        assert_eq!(normalize_stem("a.step.gz"), "a");
        assert_eq!(normalize_stem("a.b.step"), "a.b", "only the last extension goes");
        assert_eq!(normalize_stem(".wrl"), ".wrl", "a name that is only an extension stays");
        assert_eq!(normalize_stem("plain"), "plain");
        assert_eq!(full_name("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R.wrl"), "R.wrl");
        assert_eq!(full_name("C:\\models\\R.wrl"), "R.wrl");
        assert_eq!(parent_dir_name("${V}/Resistor_SMD.3dshapes/R.wrl"), "Resistor_SMD.3dshapes");
        assert_eq!(parent_dir_name("R.wrl"), "");
    }

    #[test]
    fn the_edit_distance_is_the_classic_one() {
        for (a, b, d) in [("", "", 0), ("abc", "", 3), ("", "ab", 2), ("kitten", "sitting", 3), ("flaw", "lawn", 2), ("r_0603", "r_0603", 0), ("r_0603", "r_0604", 1)] {
            assert_eq!(levenshtein(a, b), d, "{a} {b}");
            assert_eq!(levenshtein(b, a), d, "{b} {a}");
        }
    }

    #[test]
    fn candidates_are_the_same_stem_first_in_the_same_folder_then_the_near_ones() {
        let wrl = "${KICAD6_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl";
        let c = catalog(&[
            "${K}/Capacitor_SMD.3dshapes/R-0603-1608Metric.step",     // the same stem, another folder
            "${K}/Resistor_SMD.3dshapes/R_0603_1608Metric.step",      // the same stem, the same folder
            "${K}/Resistor_SMD.3dshapes/R_0603_1608metric.stp",       // the same, another extension
            "${K}/Resistor_SMD.3dshapes/R_0604_1608Metric.step",      // one edit
            "${K}/Resistor_SMD.3dshapes/R_0805_2012Metric.step",      // six: more than a quarter of 17 (four)
            "${K}/Resistor_SMD.3dshapes/R_0603_1608Metric_Pad.step",  // four edits: just within it
            "${K}/Resistor_SMD.3dshapes/R_0603_1608Metric_HandSolder.step",
            "${K}/Resistor_SMD.3dshapes/readme.txt",
        ]);
        let ranked = rank_candidates(wrl, &c);
        let shown: Vec<(&str, i32)> = ranked.iter().map(|r| (r.path.rsplit('/').next().unwrap(), r.score)).collect();
        assert_eq!(shown, [("R_0603_1608Metric.step", 1000), ("R_0603_1608metric.stp", 1000), ("R-0603-1608Metric.step", 800), ("R_0604_1608Metric.step", 490), ("R_0603_1608Metric_Pad.step", 460)]);
        assert_eq!(ranked[0].display, "R_0603_1608Metric.step   \u{2014}   Resistor_SMD.3dshapes");
        // the same name in a folder of the same name, whatever the case
        let upper = rank_candidates("x/RESISTOR_smd.3dshapes/R_0603_1608Metric.wrl", &c);
        assert_eq!(upper[0].score, 1000);
        // a reference with no folder has no folder to match
        assert_eq!(rank_candidates("R_0603_1608Metric.wrl", &c)[0].score, 800);
    }

    #[test]
    fn at_most_fifteen_candidates_by_points_then_by_name_and_a_distance_of_a_quarter_of_the_stem() {
        let many: Vec<String> = (0..30).map(|i| format!("${{K}}/A.3dshapes/part_{i:02}.step")).collect();
        let c: Vec<CatalogEntry> = many.iter().map(|p| CatalogEntry::of(p)).collect();
        let ranked = rank_candidates("old/A.3dshapes/part_00.wrl", &c);
        assert_eq!(ranked.len(), MAX_CANDIDATES);
        assert_eq!(ranked[0].score, 1000, "the exact one first");
        assert!(ranked.windows(2).all(|w| w[0].score >= w[1].score));
        // ties by name: the one-edit files part_01.. part_09 and part_10, part_20, part_30 are two edits or one
        let ones: Vec<&str> = ranked.iter().skip(1).filter(|r| r.score == 490).map(|r| full_name(&r.path)).collect();
        let mut sorted = ones.clone();
        sorted.sort();
        assert_eq!(ones, sorted, "ties are by name");
        // a short stem is allowed two edits, not a quarter of its length
        assert_eq!(rank_candidates("x/ab.wrl", &catalog(&["x/abcd.step", "x/abcde.step"])).iter().map(|r| r.score).collect::<Vec<_>>(), vec![480], "ab to abcd is two edits, to abcde three");
    }

    #[test]
    fn the_match_of_a_name_prefers_the_same_folder_and_a_non_wrl_has_none() {
        let c = catalog(&["${K}/Other.3dshapes/R_0603.step", "${K}/Resistor_SMD.3dshapes/R_0603.step", "${K}/Resistor_SMD.3dshapes/R_0805.step"]);
        assert_eq!(find_match("${V}/Resistor_SMD.3dshapes/R-0603.wrl", &c).unwrap().path, "${K}/Resistor_SMD.3dshapes/R_0603.step");
        assert_eq!(find_match("${V}/Elsewhere.3dshapes/R_0603.wrl", &c).unwrap().path, "${K}/Other.3dshapes/R_0603.step", "else the first");
        assert_eq!(find_match("R_0603.wrz", &c).unwrap().stem, "r_0603");
        assert!(find_match("R_0603.step", &c).is_none(), "only a WRL is migrated");
        assert!(find_match("R_9999.wrl", &c).is_none());
        assert!(find_match("R_0603.wrl", &[]).is_none());
    }
}
