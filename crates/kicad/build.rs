//! Builds KiCad's nanosvg (`third_party/nanosvg`, copied from
//! `kicad-src-25ec2335/thirdparty/nanosvg`) and its C ABI shim
//! (`cpp/nanosvg_shim.cpp`) for the SVG import (`src/svg_import/nanosvg.rs`).
//!
//! KiCad links nanosvg with Fontconfig, FreeType and HarfBuzz to draw `text`
//! elements. Those are not on every machine the workspace builds on, so the
//! build puts `cpp/nofont` first on the include path: stand-in headers for
//! the three libraries and a `nanosvg_text.h` whose text functions do
//! nothing (the real one stays in `third_party/nanosvg/text`). nanosvg.cpp is
//! compiled unchanged.
//!
//! `cc` uses the system C++ compiler (g++ or clang++ on Linux; Apple clang on
//! macOS, from the Xcode Command Line Tools) at C++17.

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/nanosvg");
    let mut b = cc::Build::new();
    b.cpp(true).std("c++17").include("cpp/nofont").include(&root).warnings(false).file(root.join("nanosvg.cpp")).file("cpp/nanosvg_shim.cpp");
    if matches!(std::env::var("OPT_LEVEL").as_deref(), Ok("0") | Ok("1")) {
        b.opt_level(2);
    }
    b.compile("kicad_nanosvg");
    println!("cargo:rerun-if-changed=cpp");
    println!("cargo:rerun-if-changed={}", root.display());
}
