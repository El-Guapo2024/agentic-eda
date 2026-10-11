//! A `.kicad_sch` file read to be put in a design as a sheet, or merged into one (`SCH_EDIT_FRAME::LoadSheetFromFile`).
//!
//! What the file is: its own screen, the screens of the sheets it places (by the file name each placement gives, as the file wrote it),
//! and the library symbols the symbols name -- everything a design needs to take the sheet in. The reader is `eda_kicad::load_sheet_file`;
//! `eda_ops::Cmd::ImportSheet` carries one, and the server fills it in from the path the user chose, so the verb itself reads no file
//! (a command has to replay and undo the same anywhere).

use crate::ir::{LibrarySymbol, SchematicSection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetFile {
    /// The file's own screen, in KiCad's frame (`imported_from_kicad` is set): the sheet symbol it was drawn on is not in it.
    pub root: SchematicSection,
    /// The screens of the sheets the file places, and of the sheets inside those, by the file the placement names.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub screens: BTreeMap<String, SchematicSection>,
    /// The definitions of the library symbols the file's symbols name, from the `(lib_symbols ...)` of each file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lib_symbols: Vec<LibrarySymbol>,
    /// What the reader could not carry across, one line each ("2 symbols had no library definition ... and were left out").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A command is compared by value; a section has no equality of its own -- two files are equal when they write the same JSON.
impl PartialEq for SheetFile {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_string(self).ok() == serde_json::to_string(other).ok()
    }
}

impl SheetFile {
    /// Every screen of the file with the file it is (`""` for the file's own).
    pub fn all_screens(&self) -> impl Iterator<Item = (&str, &SchematicSection)> {
        std::iter::once(("", &self.root)).chain(self.screens.iter().map(|(f, s)| (f.as_str(), s)))
    }

    /// The library symbols' nicknames the symbols and power symbols of the file name (`Device` of `Device:R`), each once, sorted.
    pub fn library_nicknames(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .all_screens()
            .flat_map(|(_, s)| s.symbols.iter().map(|x| x.lib_id.as_str()).chain(s.power_symbols.iter().map(|p| p.lib_id.as_str())))
            .filter_map(|id| id.split_once(':').map(|(lib, _)| lib.to_string()))
            .filter(|lib| !lib.is_empty())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Every reference (symbol and power symbol) the file places, each once, in the order of the screens.
    pub fn references(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, s) in self.all_screens() {
            for r in s.symbols.iter().map(|x| &x.id).chain(s.power_symbols.iter().map(|p| &p.id)) {
                if !out.contains(r) {
                    out.push(r.clone());
                }
            }
        }
        out
    }
}
