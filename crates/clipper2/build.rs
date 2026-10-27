//! Builds KiCad's vendored Clipper2 2.0.1 (`third_party/clipper2`, copied
//! from `kicad-src-25ec2335/thirdparty/clipper2`) and the C ABI shim
//! (`cpp/shim.cpp`) into a static library, as KiCad's
//! `thirdparty/clipper2/CMakeLists.txt` does: the engine, offset and
//! rectclip sources, `USINGZ` defined, C++17.
//!
//! `cc` picks the system C++ compiler: g++ (or clang++) on Linux, and Apple
//! clang on macOS, which needs only the Xcode Command Line Tools
//! (`xcode-select --install`). `cc` also links the matching C++ runtime
//! (libstdc++ on Linux, libc++ on macOS).
//!
//! The C++ is always built at `-O2` or more, even in a debug profile.

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/clipper2");
    let srcs = ["clipper.engine.cpp", "clipper.offset.cpp", "clipper.rectclip.cpp"];
    let mut b = cc::Build::new();
    b.cpp(true).std("c++17").define("USINGZ", None).include(root.join("include")).warnings(false).file("cpp/shim.cpp");
    for s in srcs {
        b.file(root.join("src").join(s));
    }
    // Clipper2 is the hot path of every zone, outline and router hull: build
    // it optimised even in a debug profile (as a distro's KiCad does).
    if matches!(std::env::var("OPT_LEVEL").as_deref(), Ok("0") | Ok("1")) {
        b.opt_level(2);
    }
    b.compile("clipper2");
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed={}", root.display());
}
