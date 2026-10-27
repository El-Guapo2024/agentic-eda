//! Generates the prost types of KiCad's IPC API into `crates/kicad-engine/src/api/gen/`.
//! See `Cargo.toml` for how to run it. The well-known types (`google.protobuf.Any`, `Empty`, `FieldMask`, ...) are
//! compiled too, so the runtime needs `prost` alone (no `prost-types`).
use std::path::PathBuf;

fn main() {
    let proto_dir = PathBuf::from(std::env::var("KICAD_API_PROTO_DIR").unwrap_or_else(|_| {
        format!("{}/src/kicad-src-25ec2335/api/proto", std::env::var("HOME").unwrap())
    }));
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/api/gen");
    std::fs::create_dir_all(&out).unwrap();
    let mut protos: Vec<PathBuf> = Vec::new();
    let mut stack = vec![proto_dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "proto") {
                protos.push(p);
            }
        }
    }
    protos.sort();
    let mut cfg = prost_build::Config::new();
    cfg.out_dir(&out);
    cfg.compile_well_known_types();
    cfg.disable_comments(["."]);
    // `prost::Name` on every message, with the type URL KiCad packs into `google.protobuf.Any`
    // (`type.googleapis.com/kiapi.common.commands.Ping`; `api_handler.cpp::Handle` reads the name after the slash).
    cfg.enable_type_names();
    cfg.type_name_domain(["."], "type.googleapis.com");
    cfg.compile_protos(&protos, &[proto_dir]).unwrap();
    for e in std::fs::read_dir(&out).unwrap() {
        println!("{}", e.unwrap().path().display());
    }
}
