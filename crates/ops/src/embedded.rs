//! The verbs of the embedded files (GAPS.md items 24 and 30): the Embedded Files panel (`PANEL_EMBEDDED_FILES`, `common/dialogs/panel_embedded_files.cpp`) in Board Setup, Schematic
//! Setup and Footprint Properties, the tool actions behind it (`common.Embed.embededFile`, `common.Embed.removeFile`, `common/tool/embed_tool.cpp`) and Collect and Embed 3D Models
//! (`PCB_CONTROL::CollectAndEmbed3DModels`, `pcbnew/tools/pcb_control.cpp`).
//!
//! They are one variant of [`crate::Cmd`], `{"op": "embedded", "verb": "add", "owner": {"kind": "board"}, "name": "part.step", "data": "<base64 of the file>"}`. The panel edits a copy
//! of the list and applies it on OK; here every edit is a verb of its own, one undo step, which is what the other setup pages of the studio do for their edits that are not a table of
//! rows (the panel's own Apply is the click).
//!
//! A list belongs to its [`EmbeddedOwner`]: the board (`DrawingsSection::embedded`), the schematic (the root sheet's `SchExtras::embedded`), a footprint on the board
//! (`FootprintEdit::embedded` over the list its board file gave it), a footprint or a symbol of the project libraries (`FootprintLibrarySection::embedded`,
//! `SymbolLibrarySection::embedded`). The files are kept as KiCad keeps them (`eda_kicad::embedded`): the content a person adds is compressed here, once, and stored as the
//! compressed text.

use super::Board;
use eda_model::embedded_files::{full_name_of, name_of_link, EmbeddedFile, EmbeddedFileType, EmbeddedFiles};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The largest file the Add verb takes: the studio's server reads at most 64 MiB of a request, and the content comes as base64 in it.
pub const MAX_FILE_BYTES: usize = 40 * 1024 * 1024;

/// Whose list a verb edits: see [`eda_model::embedded_files::EmbeddedOwner`].
pub use eda_model::embedded_files::EmbeddedOwner;

fn owner_domain(owner: &EmbeddedOwner) -> super::Domain {
    match owner {
        EmbeddedOwner::Board | EmbeddedOwner::Footprint { .. } => super::Domain::Pcb,
        EmbeddedOwner::Schematic => super::Domain::Schematic,
        EmbeddedOwner::LibraryFootprint { .. } => super::Domain::FootprintEditor,
        EmbeddedOwner::LibrarySymbol { .. } => super::Domain::SymbolEditor,
    }
}

/// One 3D model Collect and Embed 3D Models found: the path the footprints name it by, and the files it embeds -- the model first, then (for a VRML model) the STEP beside it, which
/// the OCCT exporter wants. The studio's server reads them (`crates/cli/src/embedded_collect.rs`); the verb reads no file, so that it replays and undoes the same elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectedModel {
    pub path: String,
    pub files: Vec<EmbeddedFile>,
}

/// One edit of an embedded files list. On the wire `{"op": "embedded", "verb": "<verb>", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum EmbeddedCmd {
    /// The panel's Add button and `common.Embed.embededFile` (`PANEL_EMBEDDED_FILES::AddEmbeddedFile`, `EMBEDDED_FILES::AddFile`): the file `name` with this content (`data`, base64 of
    /// the bytes), compressed and checksummed here. A file of that name that is there with other content is refused unless `replace` (the panel asks "Replace" or "Reuse Existing");
    /// one with the same content is refused as nothing to change. The type comes from the name's extension unless `kind` says.
    Add {
        owner: EmbeddedOwner,
        name: String,
        data: String,
        #[serde(default)]
        replace: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<EmbeddedFileType>,
    },
    /// The panel's Remove button and `common.Embed.removeFile` (`PANEL_EMBEDDED_FILES::RemoveEmbeddedFile`): the files by name -- or by link, `kicad-embed://name` -- taken out. For the
    /// board, `nested` takes them out of the footprints' lists too ("Deleted embedded files are also referenced in some footprints. Delete from footprints as well?").
    Remove {
        owner: EmbeddedOwner,
        names: Vec<String>,
        #[serde(default)]
        nested: bool,
    },
    /// The panel's "Embed fonts" box: whether the fonts the text uses are embedded when the file is saved. The fonts already in the list go (`onFontEmbedClick`).
    SetFonts { owner: EmbeddedOwner, embed: bool },
    /// The whole list replaced -- what a footprint or a symbol brings in with it when it is imported from a file.
    SetList { owner: EmbeddedOwner, list: EmbeddedFiles },
    /// `pcbnew.EditorControl.collect3DModels`: the models the footprints of the board use are embedded in the board, and the footprints name them by `kicad-embed://`. The studio's
    /// server fills `models` in from the files it finds; a command that carries none embeds nothing.
    CollectModels {
        #[serde(default)]
        models: Vec<CollectedModel>,
    },
}

impl EmbeddedCmd {
    /// Which editor the step belongs to, for the undo scope.
    pub fn domain(&self) -> super::Domain {
        match self {
            EmbeddedCmd::Add { owner, .. } | EmbeddedCmd::Remove { owner, .. } | EmbeddedCmd::SetFonts { owner, .. } | EmbeddedCmd::SetList { owner, .. } => owner_domain(owner),
            EmbeddedCmd::CollectModels { .. } => super::Domain::Pcb,
        }
    }

    /// The ids the command names, for the journal and the activity feed.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            EmbeddedCmd::Add { owner, name, .. } => owner_ids(owner).into_iter().chain([name.as_str()]).collect(),
            EmbeddedCmd::Remove { owner, names, .. } => owner_ids(owner).into_iter().chain(names.iter().map(String::as_str)).collect(),
            EmbeddedCmd::SetFonts { owner, .. } | EmbeddedCmd::SetList { owner, .. } => owner_ids(owner),
            EmbeddedCmd::CollectModels { models } => models.iter().map(|m| m.path.as_str()).collect(),
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        match self {
            EmbeddedCmd::Add { owner, name, data, replace, .. } => format!("embed add {} {name:?} ({} bytes of data){}", owner.label(), data.len() / 4 * 3, if *replace { " --replace" } else { "" }),
            EmbeddedCmd::Remove { owner, names, nested } => format!("embed remove {} {}{}", owner.label(), names.join(","), if *nested { " --nested" } else { "" }),
            EmbeddedCmd::SetFonts { owner, embed } => format!("embed fonts {} {}", owner.label(), if *embed { "on" } else { "off" }),
            EmbeddedCmd::SetList { owner, list } => format!("embed set {} ({} files)", owner.label(), list.files.len()),
            EmbeddedCmd::CollectModels { models } => format!("embed collect-3d-models ({} models)", models.len()),
        }
    }

    /// The kind of step it is, for the journal's summary line.
    pub fn kind(&self) -> &'static str {
        "embedded-files"
    }
}

fn owner_ids(owner: &EmbeddedOwner) -> Vec<&str> {
    match owner {
        EmbeddedOwner::Board | EmbeddedOwner::Schematic => Vec::new(),
        EmbeddedOwner::Footprint { reference } => vec![reference.as_str()],
        EmbeddedOwner::LibraryFootprint { name } => vec![name.as_str()],
        EmbeddedOwner::LibrarySymbol { lib_id } => vec![lib_id.as_str()],
    }
}

fn refuse(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

fn nothing_to_change(what: &str, msg: &str) -> Vec<CheckResult> {
    refuse("ops_nothing_to_change", what, msg)
}

/// A file named as the panel names it: the name, or the link `kicad-embed://name` (`RemoveEmbeddedFile` takes the prefix off).
fn plain_name(name: &str) -> &str {
    name_of_link(name).unwrap_or(name)
}

impl Board<'_> {
    /// The list `owner` has now.
    pub(crate) fn embedded_now(&self, owner: &EmbeddedOwner) -> Result<EmbeddedFiles, Vec<CheckResult>> {
        eda_model::embedded_files::list_of(&self.design, owner).map_err(|e| refuse(e.check, &e.what, e.reason))
    }

    /// Keep `list` as `owner`'s: an empty one leaves no entry at all, and a footprint's list that is the one its board file gave it is no edit.
    pub(crate) fn store_embedded(&mut self, owner: &EmbeddedOwner, list: EmbeddedFiles) -> Result<(), Vec<CheckResult>> {
        match owner {
            EmbeddedOwner::Board => {
                self.drawings_mut().embedded = list;
            }
            EmbeddedOwner::Schematic => {
                let Some(sch) = self.design.schematic.as_mut() else { return Err(refuse("ops_no_schematic", "schematic", "the design has no schematic to embed files in")) };
                sch.extras.embedded = list;
            }
            EmbeddedOwner::Footprint { reference } => {
                let own = self.design.drawings.as_ref().and_then(|d| d.footprint_extras.iter().find(|e| &e.id == reference)).map(|e| e.embedded.clone()).unwrap_or_default();
                self.edit_slot(reference).embedded = (list != own).then_some(list);
                self.tidy_edit(reference);
            }
            EmbeddedOwner::LibraryFootprint { name } => {
                let lib = self.footprint_library_mut();
                if list.is_empty() {
                    lib.embedded.remove(name);
                } else {
                    lib.embedded.insert(name.clone(), list);
                }
            }
            EmbeddedOwner::LibrarySymbol { lib_id } => {
                let lib = self.symbol_library_mut();
                if list.is_empty() {
                    lib.embedded.remove(lib_id);
                } else {
                    lib.embedded.insert(lib_id.clone(), list);
                }
            }
        }
        Ok(())
    }

    /// The list of `owner` after `change`, refused when it comes to the list it was.
    fn change_embedded(&mut self, owner: &EmbeddedOwner, what: &str, change: impl FnOnce(&mut EmbeddedFiles) -> Result<(), Vec<CheckResult>>) -> Result<(), Vec<CheckResult>> {
        let was = self.embedded_now(owner)?;
        let mut list = was.clone();
        change(&mut list)?;
        if list == was {
            return Err(nothing_to_change(what, "the embedded files are as they were"));
        }
        self.store_embedded(owner, list)
    }

    /// Apply one [`EmbeddedCmd`].
    pub(crate) fn apply_embedded(&mut self, cmd: &EmbeddedCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            EmbeddedCmd::Add { owner, name, data, replace, kind } => self.add_embedded(owner, name, data, *replace, *kind),
            EmbeddedCmd::Remove { owner, names, nested } => self.remove_embedded(owner, names, *nested),
            EmbeddedCmd::SetFonts { owner, embed } => self.set_embedded_fonts(owner, *embed),
            EmbeddedCmd::SetList { owner, list } => {
                let what = owner.label();
                let mut sorted = EmbeddedFiles { fonts: list.fonts, files: Vec::new() };
                for f in &list.files {
                    sorted.insert(f.clone());
                }
                self.change_embedded(owner, &what, |l| {
                    *l = sorted;
                    Ok(())
                })
            }
            EmbeddedCmd::CollectModels { models } => self.collect_models(models),
        }
    }

    /// `PANEL_EMBEDDED_FILES::AddEmbeddedFile` and `EMBEDDED_FILES::AddFile( .., aOverwrite = true )`.
    fn add_embedded(&mut self, owner: &EmbeddedOwner, name: &str, data: &str, replace: bool, kind: Option<EmbeddedFileType>) -> Result<(), Vec<CheckResult>> {
        let name = full_name_of(name.trim());
        if name.is_empty() {
            return Err(refuse("ops_bad_embedded_name", name, "the file has no name"));
        }
        if name.contains('"') || name.chars().any(char::is_control) {
            return Err(refuse("ops_bad_embedded_name", name, "a file name has no quotes or control characters in it"));
        }
        let bytes = eda_kicad::embedded::base64_decode(data.trim()).map_err(|_| refuse("ops_bad_embedded_data", name, "the content is not base64"))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(refuse("ops_embedded_too_large", name, format!("{name} is {} bytes; the largest file that can be embedded is {MAX_FILE_BYTES}", bytes.len())));
        }
        let mut file = eda_kicad::embedded::encode_new(name, &bytes).map_err(|e| refuse("ops_bad_embedded_data", name, e.to_string()))?;
        if let Some(kind) = kind {
            file.kind = kind;
        }
        self.change_embedded(owner, name, |list| {
            if let Some(old) = list.get(name) {
                let same = old.checksum == file.checksum || eda_kicad::embedded::decode(old).is_ok_and(|d| d.checksum == file.checksum);
                if same {
                    return Err(nothing_to_change(name, "this file is already embedded"));
                }
                if !replace {
                    return Err(refuse("ops_embedded_exists", name, format!("A file named '{name}' is already embedded, but the file on disk has different content.")));
                }
            }
            list.put(file);
            Ok(())
        })
    }

    /// `PANEL_EMBEDDED_FILES::RemoveEmbeddedFile`, and `TransferDataFromWindow`'s removal of the files from the footprints' lists too.
    fn remove_embedded(&mut self, owner: &EmbeddedOwner, names: &[String], nested: bool) -> Result<(), Vec<CheckResult>> {
        if names.is_empty() {
            return Err(nothing_to_change("embedded files", "no file is named"));
        }
        let names: Vec<&str> = names.iter().map(|n| plain_name(n)).collect();
        let what = owner.label();
        // A footprint's list that only names a file the board holds: the board's list is the one to take it out of (Board Setup lists those, and does not remove them from here).
        if let Some(missing) = names.iter().find(|n| self.embedded_now(owner).map_or(false, |l| l.get(n).is_none())) {
            return Err(refuse("ops_unknown_embedded_file", missing, format!("no file named '{missing}' is embedded in the {what}")));
        }
        self.change_embedded(owner, &what, |list| {
            for n in &names {
                list.remove(n);
            }
            Ok(())
        })?;
        if nested && matches!(owner, EmbeddedOwner::Board) {
            let refs: Vec<String> = self.design.placement.iter().flat_map(|p| p.footprints.iter().map(|f| f.id.clone())).collect();
            for r in refs {
                let mut list = eda_model::embedded_files::of_footprint(&self.design, &r).cloned().unwrap_or_default();
                let before = list.files.len();
                for n in &names {
                    list.remove(n);
                }
                if list.files.len() != before {
                    self.store_embedded(&EmbeddedOwner::Footprint { reference: r }, list)?;
                }
            }
        }
        Ok(())
    }

    /// The "Embed fonts" box (`onFontEmbedClick` and `TransferDataFromWindow`).
    fn set_embedded_fonts(&mut self, owner: &EmbeddedOwner, embed: bool) -> Result<(), Vec<CheckResult>> {
        let what = owner.label();
        self.change_embedded(owner, &what, |list| {
            if list.fonts != embed {
                list.fonts = embed;
                // The click removes the fonts that were in the list, and adds the ones the text uses -- the studio has no outline font to add.
                list.clear_fonts();
            }
            Ok(())
        })
    }

    /// `PCB_CONTROL::CollectAndEmbed3DModels`, given the files the server found.
    fn collect_models(&mut self, models: &[CollectedModel]) -> Result<(), Vec<CheckResult>> {
        if models.is_empty() {
            return Err(nothing_to_change("3D models", "no 3D model of a footprint on the board is a file that can be embedded"));
        }
        let mut renames: BTreeMap<String, String> = BTreeMap::new();
        {
            let board = &mut self.drawings_mut().embedded;
            for m in models {
                let Some(main) = m.files.first() else { continue };
                for f in &m.files {
                    // `AddFile( fname, false )`: a file of that name that is there stays, whatever it holds.
                    board.insert(f.clone());
                }
                let link = board.get(&main.name).map(EmbeddedFile::link).unwrap_or_else(|| main.link());
                renames.insert(m.path.clone(), link);
            }
        }
        self.migrate_board_models(&renames)
    }

    /// `FootprintEdit`s and library entries that go with a name that changes: the embedded files of a library footprint follow its rename.
    pub(crate) fn move_library_footprint_embedded(&mut self, from: &str, to: &str) {
        if let Some(lib) = self.design.footprint_library.as_mut() {
            if let Some(list) = lib.embedded.remove(from) {
                lib.embedded.insert(to.to_string(), list);
            }
        }
    }

    /// The same for a library symbol.
    pub(crate) fn move_library_symbol_embedded(&mut self, from: &str, to: &str) {
        if let Some(lib) = self.design.symbol_library.as_mut() {
            if let Some(list) = lib.embedded.remove(from) {
                lib.embedded.insert(to.to_string(), list);
            }
        }
    }
}
