//! A sheet file outside the root's folder (`../shared/amp.kicad_sch`): KiCad reads it where the placing file names it
//! (`SCH_IO_KICAD_SEXPR::loadHierarchy`: `fileName.MakeAbsolute( m_currentPath.top() )`) and saves a screen only where it read it. Our derived
//! schematic is a copy in its work folder, so every file of it must be written inside that folder, and never over the project's own sheet.

use std::path::{Path, PathBuf};

const ROOT: &str = r#"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "11111111-1111-1111-1111-111111111111") (paper "A4")
 (sheet (at 100 100) (size 20 20) (exclude_from_sim no) (in_bom yes) (on_board yes) (dnp no) (uuid "21111111-1111-1111-1111-111111111111")
  (property "Sheetname" "AMP" (at 100 99 0) (effects (font (size 1.27 1.27)) (justify left bottom)))
  (property "Sheetfile" "SHEETFILE" (at 100 121 0) (effects (font (size 1.27 1.27)) (justify left top)))
  (instances (project "root" (path "/11111111-1111-1111-1111-111111111111" (page "2")))))
 (sheet_instances (path "/" (page "1"))))
"#;

const AMP: &str = r#"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0") (uuid "41111111-1111-1111-1111-111111111111") (paper "A4")
 (text "${SHEETFILE} ${#}" (exclude_from_sim no) (at 50 50 0) (effects (font (size 1.27 1.27))) (uuid "51111111-1111-1111-1111-111111111111")))
"#;

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `root` (in `<base>/<root_dir>`) places `sheetfile`; the derived files go to `<base>/<work>`. Returns kicad-cli's kicadxml netlist of them.
fn run(base: &Path, root_dir: &str, sheetfile: &str, work: &str) -> Option<String> {
    let root_path = base.join(root_dir).join("root.kicad_sch");
    std::fs::create_dir_all(root_path.parent().unwrap()).unwrap();
    std::fs::write(&root_path, ROOT.replace("SHEETFILE", sheetfile)).unwrap();
    let amp = base.join(root_dir).join(sheetfile);
    std::fs::create_dir_all(amp.parent().unwrap()).unwrap();
    std::fs::write(&amp, AMP).unwrap();
    let (design, model, _) = eda_kicad::import_kicad_sch_tree(&root_path).expect("the tree reads");
    let work = base.join(work);
    let mut before = Vec::new();
    files_under(base, &mut before);
    let r = eda_kicad_engine::sim_netlists(&design, &model, &work, "board");
    let mut after = Vec::new();
    files_under(base, &mut after);
    for f in &after {
        assert!(before.contains(f) || f.starts_with(&work), "{} is written outside the work folder {}", f.display(), work.display());
    }
    assert_eq!(std::fs::read_to_string(&amp).unwrap(), AMP, "the project's own sheet is left alone");
    match r {
        Ok(n) => Some(n.xml),
        Err(e) if eda_kicad_engine::find_cli().is_none() => {
            eprintln!("kicad-cli not found ({e:?}); the netlist is not checked");
            None
        }
        Err(e) => panic!("the derived schematic: {e:?}"),
    }
}

#[test]
fn a_screen_above_the_roots_folder_is_written_inside_the_work_folder() {
    let base = std::env::temp_dir().join(format!("eda-sheet-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // one level up, the work folder beside the project
    let one = run(&base.join("one"), "src/proj", "../shared/amp.kicad_sch", "out/.kicad");
    // two levels up, the work folder as deep as the project: `work.join("../../shared2/amp.kicad_sch")` is the project's own file
    let two = run(&base.join("two"), "src/proj", "../../shared2/amp.kicad_sch", "out/.kicad");
    // an absolute name
    let abs = base.join("three/abs/amp.kicad_sch");
    let three = run(&base.join("three"), "src/proj", abs.to_str().unwrap(), "out/.kicad");
    for xml in [one, two, three].into_iter().flatten() {
        assert!(xml.contains("name=\"/AMP/\""), "kicad-cli reads the sheet in the derived file: {xml}");
    }
    let _ = std::fs::remove_dir_all(&base);
}
