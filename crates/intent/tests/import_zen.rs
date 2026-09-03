use std::path::PathBuf;

use eda_model::PinKind;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn ldo_and_cap_import_successfully() {
    let model = eda_intent::import_zen(&fixture("ldo_cap.zen")).expect("should import cleanly");

    assert_eq!(model.parts.len(), 2);
    let u1 = model.part("U1").expect("U1 present");
    assert_eq!(u1.mpn.as_deref(), Some("MCP1700T-3302E/TT"));
    assert_eq!(u1.pins.len(), 3);
    assert!(u1.pins.iter().any(|p| p.kind == PinKind::Ground));
    assert!(u1.pins.iter().any(|p| p.kind == PinKind::Power));

    assert_eq!(model.nets.len(), 3);
    let vout = model.nets_matching("VOUT");
    assert_eq!(vout.len(), 1);
    assert!(vout[0].pins.contains(&"U1.2".to_string()));
    assert!(vout[0].pins.contains(&"C1.1".to_string()));
}

#[test]
fn broken_script_returns_check_results_not_panic() {
    let result = eda_intent::import_zen(&fixture("broken.zen"));
    let errs = result.expect_err("broken script must not import");
    assert!(!errs.is_empty());
    assert_eq!(errs[0].check, "intent_parse_error");
    assert_eq!(errs[0].status, eda_model::CheckStatus::Fail);
    assert!(errs[0].hint.is_some());
}

#[test]
fn undefined_net_reference_returns_check_result() {
    let dir = tempdir();
    let path = dir.join("undefined_net.zen");
    std::fs::write(
        &path,
        r#"
Component("U1", pins = [("1", "VCC", "power")])
Net("BAD", ["U1.1", "U99.5"])
"#,
    )
    .unwrap();

    let result = eda_intent::import_zen(&path);
    let errs = result.expect_err("net referencing undeclared part must fail");
    assert!(errs.iter().any(|e| e.check == "intent_undefined_net"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn multi_module_design_imports_with_clusters() {
    let model =
        eda_intent::import_zen(&fixture("multi_module.zen")).expect("should import cleanly");

    assert_eq!(model.parts.len(), 4);
    assert_eq!(model.nets.len(), 4);
    assert_eq!(model.clusters.len(), 2);

    let power = model
        .clusters
        .iter()
        .find(|c| c.anchor == "U1")
        .expect("power module present");
    assert!(power.members.contains(&"C1".to_string()));

    let mcu = model
        .clusters
        .iter()
        .find(|c| c.anchor == "U2")
        .expect("mcu module present");
    assert!(mcu.members.contains(&"C2".to_string()));
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda-intent-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
