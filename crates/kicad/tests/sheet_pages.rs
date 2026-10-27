//! The page numbers of a hierarchy's sheets as KiCad reads and numbers them: a sheet whose file is missing keeps its place (r3 B3-M4), another
//! project's page for a placement is not this one's (r3 B3-M5), and a page is read with its whitespace out and an empty one as `#` (r3 B3-L1, B3-L2).

use eda_kicad::{export_kicad_sch_tree, import_kicad_sch_tree, ExportMeta};

const ROOT_UUID: &str = "11111111-1111-1111-1111-111111111111";

/// A root placing A (`a.kicad_sch`, at x 50) and B (`b.kicad_sch`, at x 150), each sheet with the `(instances ..)` given.
fn root(a_instances: &str, b_instances: &str) -> String {
    let sheet = |x: u32, uuid: &str, name: &str, inst: &str| {
        format!(
            "(sheet (at {x} 100) (size 20 20) (uuid \"{uuid}\")\n  (property \"Sheetname\" \"{name}\" (at {x} 99 0) (effects (font (size 1.27 1.27))))\n  (property \"Sheetfile\" \"{}.kicad_sch\" (at {x} 121 0) (effects (font (size 1.27 1.27))))\n  {inst})\n",
            name.to_lowercase()
        )
    };
    format!(
        "(kicad_sch (version 20250114) (generator \"eeschema\") (uuid \"{ROOT_UUID}\") (paper \"A4\")\n{}{}(sheet_instances (path \"/\" (page \"1\"))))\n",
        sheet(50, "21111111-1111-1111-1111-111111111111", "A", a_instances),
        sheet(150, "31111111-1111-1111-1111-111111111111", "B", b_instances)
    )
}

const SCREEN: &str = "(kicad_sch (version 20250114) (generator \"eeschema\") (uuid \"41111112-1111-1111-1111-111111111111\") (paper \"A4\"))\n";

/// The project in a scratch folder, with `b.kicad_sch` (and `a.kicad_sch` when `with_a`), read as a tree.
fn read(name: &str, root_text: &str, with_a: bool) -> (eda_model::ir::Design, eda_model::ConstraintModel) {
    let dir = std::env::temp_dir().join(format!("eda-sheet-pages-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("root.kicad_sch"), root_text).unwrap();
    std::fs::write(dir.join("b.kicad_sch"), SCREEN).unwrap();
    if with_a {
        std::fs::write(dir.join("a.kicad_sch"), SCREEN.replace("41111112", "51111112")).unwrap();
    }
    let (design, model, _) = import_kicad_sch_tree(&dir.join("root.kicad_sch")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (design, model)
}

/// The page each sheet of the derived root is written with, by name.
fn written_pages(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> Vec<(String, String)> {
    let files = export_kicad_sch_tree(design, model, &ExportMeta { date: "2026-10-27", title: "root" }, "root.kicad_sch").unwrap();
    let text = &files[0].1;
    ["A", "B"]
        .iter()
        .map(|name| {
            // the sheet's `(instances (project .. (path .. (page "N"))))` follows its fields
            let at = text.find(&format!("\"Sheetname\" \"{name}\"")).unwrap_or_else(|| panic!("sheet {name} in {text}"));
            let page = &text[at..][text[at..].find("(page \"").unwrap() + 7..];
            (name.to_string(), page[..page.find('"').unwrap()].to_string())
        })
        .collect()
}

#[test]
fn a_sheet_with_no_screen_keeps_its_place_in_the_page_numbers() {
    // `loadHierarchy` gives A an empty screen; `SetInitialPageNumbers` numbers it 2 and B 3
    let (design, model) = read("missing", &root("", ""), false);
    let mut pages = written_pages(&design, &model);
    pages.sort();
    assert_eq!(pages, vec![("A".to_string(), "2".to_string()), ("B".to_string(), "3".to_string())]);
}

#[test]
fn another_projects_page_for_the_same_placement_is_not_this_ones() {
    let a = format!("(instances (project \"root\" (path \"/{ROOT_UUID}\" (page \"3\"))) (project \"other\" (path \"/99999999-9999-9999-9999-999999999999\" (page \"9\"))))");
    let b = format!("(instances (project \"root\" (path \"/{ROOT_UUID}\" (page \"5\"))))");
    let (design, model) = read("other_project", &root(&a, &b), true);
    let mut pages = written_pages(&design, &model);
    pages.sort();
    assert_eq!(pages, vec![("A".to_string(), "3".to_string()), ("B".to_string(), "5".to_string())]);
    // and the other project's instance listed first does not win either
    let a = format!("(instances (project \"other\" (path \"/99999999-9999-9999-9999-999999999999\" (page \"9\"))) (project \"root\" (path \"/{ROOT_UUID}\" (page \"3\"))))");
    let (design, model) = read("other_project_first", &root(&a, &b), true);
    let a_page = written_pages(&design, &model).into_iter().find(|(n, _)| n == "A").unwrap().1;
    assert_eq!(a_page, "3");
}

#[test]
fn a_page_is_read_without_its_whitespace_and_an_empty_one_as_hash() {
    let a = format!("(instances (project \"root\" (path \"/{ROOT_UUID}\" (page \" 1 2\t\"))))");
    let b = format!("(instances (project \"root\" (path \"/{ROOT_UUID}\" (page \"\"))))");
    let (design, _) = read("whitespace", &root(&a, &b), true);
    let sheets = &design.schematic.as_ref().unwrap().sheets;
    let page = |name: &str| sheets.iter().find(|s| s.name == name).unwrap().page.clone();
    assert_eq!((page("A"), page("B")), ("12".to_string(), "#".to_string()));
}
