//! The verbs of the embedded files (GAPS.md items 24 and 30): the Embedded Files panel (`PANEL_EMBEDDED_FILES`, `common/dialogs/panel_embedded_files.cpp`) in Board Setup, Schematic
//! Setup and Footprint Properties, the tool actions behind it (`common.Embed.embededFile`, `common.Embed.removeFile`, `common/tool/embed_tool.cpp`) and Collect and Embed 3D Models
//! (`PCB_CONTROL::CollectAndEmbed3DModels`, `pcbnew/tools/pcb_control.cpp`).
//!
//! They are one variant of [`crate::Cmd`], `{"op": "embedded", "verb": "add", "owner": {"kind": "board"}, "name": "part.step", "data": "<base64 of the file>"}`. The panel edits a copy
//! of the list (`m_localFiles`) and applies it on OK (`PANEL_EMBEDDED_FILES::TransferDataFromWindow`): that is [`EmbeddedCmd::Apply`], one undo step for everything the panel did.
//! The single edits (`add`, `remove`, `set_fonts`) stay for the CLI, the tool actions and the journal of older steps.
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

/// A file the panel's Add Files took in (`PANEL_EMBEDDED_FILES::AddEmbeddedFile`): its name, its content (base64 of the bytes) and the type, when it is not the extension's. The
/// panel has asked its conflict question already: a file added under a name the list has replaces it ("Replace"; "Reuse Existing" adds nothing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedFile {
    pub name: String,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<EmbeddedFileType>,
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
    /// The panel's OK (`PANEL_EMBEDDED_FILES::TransferDataFromWindow`): the panel edited a copy of the list (`m_localFiles`) and this writes it back, one undo step. In the order
    /// the copy came to be: when the "Embed fonts" box was clicked (`refresh_fonts`, `onFontEmbedClick`) the fonts the list held go, then the files the panel removed
    /// (`removed`, by name or `kicad-embed://` link), then the files Add Files took in (`added`, each replacing a file of its name), then -- the box ticked -- the fonts the
    /// text uses (`font_files`: `onFontEmbedClick`'s `AddFile( font->GetFileName(), true )`, typed `FONT`); last, the box's state (`SetAreFontsEmbedded`). On the board,
    /// `nested` takes the removed files out of the footprints' lists too (the answer to "Deleted embedded files are also referenced in some footprints. Delete from
    /// footprints as well?").
    ///
    /// `text_fonts` names the rows of fonts the click took in that the panel still lists, and `font_files` is their content: the studio's server reads it in when the
    /// command comes with none (`crates/cli/src/embedded_api.rs` `with_text_fonts`), so the command replays and undoes the same on a machine without the fonts.
    Apply {
        owner: EmbeddedOwner,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        removed: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        added: Vec<AddedFile>,
        fonts: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        refresh_fonts: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        text_fonts: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_files: Option<Vec<EmbeddedFile>>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        nested: bool,
    },
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
            EmbeddedCmd::Add { owner, .. } | EmbeddedCmd::Remove { owner, .. } | EmbeddedCmd::SetFonts { owner, .. } | EmbeddedCmd::SetList { owner, .. } | EmbeddedCmd::Apply { owner, .. } => owner_domain(owner),
            EmbeddedCmd::CollectModels { .. } => super::Domain::Pcb,
        }
    }

    /// The ids the command names, for the journal and the activity feed.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            EmbeddedCmd::Add { owner, name, .. } => owner_ids(owner).into_iter().chain([name.as_str()]).collect(),
            EmbeddedCmd::Remove { owner, names, .. } => owner_ids(owner).into_iter().chain(names.iter().map(String::as_str)).collect(),
            EmbeddedCmd::SetFonts { owner, .. } | EmbeddedCmd::SetList { owner, .. } => owner_ids(owner),
            EmbeddedCmd::Apply { owner, removed, added, .. } => owner_ids(owner).into_iter().chain(removed.iter().map(String::as_str)).chain(added.iter().map(|a| a.name.as_str())).collect(),
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
            EmbeddedCmd::Apply { owner, removed, added, fonts, refresh_fonts, text_fonts, nested, .. } => {
                let mut parts: Vec<String> = Vec::new();
                if !removed.is_empty() {
                    parts.push(format!("-{}", removed.join(",")));
                }
                if !added.is_empty() {
                    parts.push(format!("+{}", added.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(",")));
                }
                if *refresh_fonts {
                    parts.push(format!("fonts {}", if text_fonts.is_empty() { "cleared".to_string() } else { text_fonts.join(",") }));
                }
                format!("embed apply {} {}fonts {}{}", owner.label(), parts.iter().map(|p| format!("{p} ")).collect::<String>(), if *fonts { "on" } else { "off" }, if *nested { " --nested" } else { "" })
            }
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
            EmbeddedCmd::Apply { owner, removed, added, fonts, refresh_fonts, font_files, nested, .. } => self.apply_panel(owner, removed, added, *fonts, *refresh_fonts, font_files.as_deref().unwrap_or_default(), *nested),
            EmbeddedCmd::CollectModels { models } => self.collect_models(models),
        }
    }

    /// The content of a file Add Files took in, kept as KiCad keeps it (`EMBEDDED_FILES::AddFile`: read, `CompressAndEncode`), refused as `AddEmbeddedFile` refuses it.
    fn new_file(name: &str, data: &str, kind: Option<EmbeddedFileType>) -> Result<EmbeddedFile, Vec<CheckResult>> {
        let name = full_name_of(name.trim());
        if name.is_empty() {
            return Err(refuse("ops_bad_embedded_name", name, "the file has no name"));
        }
        if name.contains('"') || name.chars().any(char::is_control) {
            return Err(refuse("ops_bad_embedded_name", name, "a file name has no quotes or control characters in it"));
        }
        let bytes = eda_kicad::embedded::base64_decode(data.trim()).map_err(|_| refuse("ops_bad_embedded_data", name, "the content is not base64"))?;
        // `EMBEDDED_FILES::AddFile` returns no file for an empty one (`if( !efile->decompressedData.data() ) return nullptr;`: an empty vector has no data), and
        // `AddEmbeddedFile` says so.
        if bytes.is_empty() {
            return Err(refuse("ops_bad_embedded_data", name, format!("Failed to add file '{name}'.")));
        }
        if bytes.len() > MAX_FILE_BYTES {
            return Err(refuse("ops_embedded_too_large", name, format!("{name} is {} bytes; the largest file that can be embedded is {MAX_FILE_BYTES}", bytes.len())));
        }
        let mut file = eda_kicad::embedded::encode_new(name, &bytes).map_err(|e| refuse("ops_bad_embedded_data", name, e.to_string()))?;
        if let Some(kind) = kind {
            file.kind = kind;
        }
        Ok(file)
    }

    /// [`EmbeddedCmd::Apply`]: `PANEL_EMBEDDED_FILES::TransferDataFromWindow` with the copy the panel made.
    #[allow(clippy::too_many_arguments)]
    fn apply_panel(&mut self, owner: &EmbeddedOwner, removed: &[String], added: &[AddedFile], fonts: bool, refresh_fonts: bool, font_files: &[EmbeddedFile], nested: bool) -> Result<(), Vec<CheckResult>> {
        let what = owner.label();
        let was = self.embedded_now(owner)?;
        let mut list = was.clone();
        // `onFontEmbedClick`: "the fonts the list held go" -- every row of type FONT, `m_localFiles->RemoveFile( name )`
        if refresh_fonts {
            list.clear_fonts();
        }
        let removed: Vec<&str> = removed.iter().map(|n| plain_name(n)).collect();
        for n in &removed {
            // a font the click above dropped is gone already; anything else the panel removed has to be there (the panel showed the list as it is now)
            if list.remove(n).is_none() && !(refresh_fonts && was.get(n).is_some_and(|f| f.kind == EmbeddedFileType::Font)) {
                return Err(refuse("ops_unknown_embedded_file", n, format!("no file named '{n}' is embedded in the {what}")));
            }
        }
        for a in added {
            list.put(Self::new_file(&a.name, &a.data, a.kind)?);
        }
        if refresh_fonts && fonts {
            for f in font_files {
                // `m_localFiles->AddFile( font->GetFileName(), true )`, and `EmbedFonts`' `file->type = FONT`
                list.put(EmbeddedFile { kind: EmbeddedFileType::Font, ..f.clone() });
            }
        }
        // `m_files->SetAreFontsEmbedded( m_cbEmbedFonts->IsChecked() )`
        list.fonts = fonts;
        // `RunOnNestedEmbeddedFiles`: the footprints of the board that list a file the board's copy no longer has, when the answer was yes
        let mut nested_lists: Vec<(String, EmbeddedFiles)> = Vec::new();
        if nested && matches!(owner, EmbeddedOwner::Board) {
            let gone: Vec<&str> = was.files.iter().map(|f| f.name.as_str()).filter(|n| list.get(n).is_none()).collect();
            for fp in self.design.placement.iter().flat_map(|p| p.footprints.iter()) {
                let mut l = eda_model::embedded_files::of_footprint(&self.design, &fp.id).cloned().unwrap_or_default();
                let before = l.files.len();
                for n in &gone {
                    l.remove(n);
                }
                if l.files.len() != before {
                    nested_lists.push((fp.id.clone(), l));
                }
            }
        }
        if list == was && nested_lists.is_empty() {
            return Err(nothing_to_change(&what, "the embedded files are as they were"));
        }
        if list != was {
            self.store_embedded(owner, list)?;
        }
        for (reference, l) in nested_lists {
            self.store_embedded(&EmbeddedOwner::Footprint { reference }, l)?;
        }
        Ok(())
    }

    /// `PANEL_EMBEDDED_FILES::AddEmbeddedFile` and `EMBEDDED_FILES::AddFile( .., aOverwrite = true )`.
    fn add_embedded(&mut self, owner: &EmbeddedOwner, name: &str, data: &str, replace: bool, kind: Option<EmbeddedFileType>) -> Result<(), Vec<CheckResult>> {
        let file = Self::new_file(name, data, kind)?;
        let name = file.name.clone();
        let name = name.as_str();
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
