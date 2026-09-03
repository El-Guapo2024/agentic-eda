//! `eda-intent` — turns LLM-authored design intent into a [`eda_model::ConstraintModel`],
//! and lints that model before any geometry is derived.
//!
//! # Importer: which path was taken, and why
//!
//! The task brief asked for a `.zen` (Zener) importer built on `pcb-zen-core`
//! if practical, else a hand-rolled Starlark front-end.
//!
//! **We took the fallback path** (`crate::zen`, built on the plain
//! `starlark` crate). Investigation of the upstream repo
//! (github.com/diodeinc/pcb) found:
//!
//! - `pcb-zen-core` is **not published on crates.io** (verified: the
//!   crates.io API returns 404 for it), so it can only be pulled as a git
//!   dependency.
//! - The repo has **no tags/releases** to pin a git dependency to — only a
//!   moving `main` branch, which is exactly the kind of unpinned dependency
//!   we were told to avoid.
//! - It's one crate in a ~40-crate Cargo workspace and pulls in a
//!   **forked `starlark`/`starlark_map`/`starlark_syntax`** (not the
//!   crates.io versions) plus `pcb-sch`, `rayon`, `walkdir`, etc. — roughly
//!   27k lines in `pcb-zen-core` alone, wired tightly into the rest of the
//!   `pcb` toolchain (module resolution, `pcb.toml` workspaces, a package
//!   registry client). Embedding it as a library dependency of a small
//!   crate would mean vendoring a large, actively-changing, semver-free
//!   dependency graph for a fraction of its surface area.
//!
//! Given "impractical to embed" was an explicit escape hatch in the brief,
//! we used it. `crate::zen` instead defines a small, deliberately narrow
//! Starlark DSL (`Component(...)`, `Net(...)`, `Module(...)`) evaluated with
//! the plain `starlark = "0.14"` crate from crates.io, with native global
//! functions that append directly into a `ConstraintModel`-shaped builder.
//! It is *not* wire-compatible with real Zener/`.zen` files from the `pcb`
//! toolchain — it is a same-shaped subset good enough for an LLM (or us) to
//! author schematic intent in a Starlark-like language, per the project's
//! stated approach ("LLM authors only schematic intent").
//!
//! # Public API
//!
//! - [`import_zen`] — evaluate a `.zen`-style file into a `ConstraintModel`,
//!   or a `Vec<CheckResult>` on any failure (never panics).
//! - [`lint::lint`] — T1.5 electrical lint over a `ConstraintModel`, before
//!   any geometry exists.

pub mod lint;
mod zen;
mod zen_cli;

pub use zen::import_zen;
pub use zen_cli::import_zen_cli;
