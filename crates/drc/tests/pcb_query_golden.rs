//! Find by Properties' Query on the backend (`eda_drc::query`) against the golden answers of the INACTIVE TypeScript copy of the language
//! (`tests/fixtures/pcb_query_golden.json`, written by `web/studio/src/kicad-port/pcbQuery.test.ts`, which checks the TypeScript side against the
//! same file): the same request, so the same `ok`, the same compiler messages and the same matching ids, for every expression but the ones in
//! [`KNOWN`], where the Rust side is KiCad's and the TypeScript copy was not.

use serde_json::Value;

/// Where the answers differ on purpose: `memberOfSheet` is ported in Rust (a footprint with no sheet path is on the root sheet, `/`), and the TypeScript
/// copy said false for every sheet.
const KNOWN: &[&str] = &["A.memberOfSheet('/')"];

#[test]
fn the_backend_answers_every_golden_query_as_the_typescript_copy_did() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pcb_query_golden.json");
    let golden: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let request = golden["request"].clone();
    let cases = golden["cases"].as_array().unwrap();
    let mut differ = Vec::new();
    for case in cases {
        let mut req = request.clone();
        req["expression"] = case["expression"].clone();
        let answer = eda_drc::query::handle(req.to_string().as_bytes());
        let pick = |v: &Value| (v["ok"].clone(), v["errors"].clone(), v["ids"].clone());
        let expression = case["expression"].as_str().unwrap_or("");
        if KNOWN.contains(&expression) {
            assert_ne!(pick(&answer), pick(case), "{expression}: listed as different, and alike now: take it off the list");
            continue;
        }
        if pick(&answer) != pick(case) {
            differ.push(format!("{}\n  rust {} {} {}\n  ts   {} {} {}", case["expression"], answer["ok"], answer["errors"], answer["ids"], case["ok"], case["errors"], case["ids"]));
        }
    }
    assert!(differ.is_empty(), "{} of {} queries differ:\n{}", differ.len(), cases.len(), differ.join("\n"));
    eprintln!("{} queries: {} alike, {} known to differ", cases.len(), cases.len() - KNOWN.len(), KNOWN.len());
}
