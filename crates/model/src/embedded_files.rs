//! Embedded files (GAPS.md items 24 and 30; `include/embedded_files.h`, `common/embedded_files.cpp`).
//!
//! KiCad 9 and later can keep files inside the files of a project: a board, a schematic, a library footprint or a library symbol holds a list of them -- 3D models, fonts,
//! datasheets, drawing sheets -- each stored as a zstd frame, base64-encoded, with a checksum, and a footprint's `(model "kicad-embed://name.step")` or a field's
//! `kicad-embed://name.pdf` names one. This module is the list as the design holds it; the codec (zstd, base64, the checksum) is `eda_kicad::embedded`.
//!
//! A file is kept **as it was read**: [`EmbeddedFile::data`] is the base64 text of the compressed frame (`EMBEDDED_FILE::compressedEncodedData`) and
//! [`EmbeddedFile::checksum`] the checksum the file wrote (`data_hash`), so a list nobody edited is written back byte for byte, and a file is decompressed only to be extracted
//! or looked at. A file with no data is a reference: a footprint on a board names the files it uses, and the board's own list holds the data
//! (`BOARD::FixupEmbeddedData`).
//!
//! The list is sorted by name -- KiCad's is a `std::map` -- and a name is a file name, never a path (`EMBEDDED_FILES::AddFile` files a file under `GetFullName()`).

use serde::{Deserialize, Serialize};

/// `FILEEXT::KiCadUriPrefix`: the scheme of the name a model, a datasheet or a worksheet gives for an embedded file is this followed by `://` and the file's name.
pub const URI_SCHEME: &str = "kicad-embed://";

/// `EMBEDDED_FILE::FILE_TYPE`: what the file is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedFileType {
    Font,
    Model,
    Worksheet,
    Datasheet,
    #[default]
    Other,
}

impl EmbeddedFileType {
    /// The word `(type ..)` carries (`EMBEDDED_FILES::WriteEmbeddedFiles`).
    pub fn word(self) -> &'static str {
        match self {
            EmbeddedFileType::Datasheet => "datasheet",
            EmbeddedFileType::Font => "font",
            EmbeddedFileType::Model => "model",
            EmbeddedFileType::Worksheet => "worksheet",
            EmbeddedFileType::Other => "other",
        }
    }

    /// The type a `(type ..)` word names (`EMBEDDED_FILES_PARSER::ParseEmbedded`); `None` for a word the parser refuses.
    pub fn from_word(word: &str) -> Option<EmbeddedFileType> {
        match word {
            "datasheet" => Some(EmbeddedFileType::Datasheet),
            "font" => Some(EmbeddedFileType::Font),
            "model" => Some(EmbeddedFileType::Model),
            "worksheet" => Some(EmbeddedFileType::Worksheet),
            "other" => Some(EmbeddedFileType::Other),
            _ => None,
        }
    }

    /// The type `EMBEDDED_FILES::AddFile` gives a file from its extension (compared in upper case): `STP`, `STPZ`, `STEP`, `WRL` and `WRZ` are models; `WOFF`, `WOFF2`, `TTF` and
    /// `OTF` fonts; `PDF` a datasheet; `KICAD_WKS` a worksheet; any other file is `other`.
    pub fn of_file_name(name: &str) -> EmbeddedFileType {
        let ext = extension_of(name).to_ascii_uppercase();
        match ext.as_str() {
            "STP" | "STPZ" | "STEP" | "WRL" | "WRZ" => EmbeddedFileType::Model,
            "WOFF" | "WOFF2" | "TTF" | "OTF" => EmbeddedFileType::Font,
            "PDF" => EmbeddedFileType::Datasheet,
            "KICAD_WKS" => EmbeddedFileType::Worksheet,
            _ => EmbeddedFileType::Other,
        }
    }

    /// What the Embedded Files panel offers to pick in the file chooser for a type; the words the studio's UI shows.
    pub fn label(self) -> &'static str {
        match self {
            EmbeddedFileType::Font => "Font",
            EmbeddedFileType::Model => "3D model",
            EmbeddedFileType::Worksheet => "Drawing sheet",
            EmbeddedFileType::Datasheet => "Datasheet",
            EmbeddedFileType::Other => "Other",
        }
    }
}

/// `wxFileName::GetFullName()`: the last component of a path, in either separator.
pub fn full_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `wxFileName::GetExt()`: what follows the last dot of the file name (nothing for a name with no dot, or whose only dot starts it? `wxFileName` takes `.hidden` as a name with
/// no extension).
pub fn extension_of(name: &str) -> &str {
    let name = full_name_of(name);
    match name.rfind('.') {
        Some(0) | None => "",
        Some(at) => &name[at + 1..],
    }
}

/// The name a `kicad-embed://name` link stands for; `None` for a path that is not one.
pub fn name_of_link(link: &str) -> Option<&str> {
    link.strip_prefix(URI_SCHEME)
}

/// One embedded file. See the module doc for what `data` and `checksum` hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedFile {
    /// The file's name, no directory (`(name "..")`).
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: EmbeddedFileType,
    /// The compressed, base64-encoded content (`compressedEncodedData`): a zstd frame that tells its content size, in the base64 alphabet with `=` padding, no line breaks.
    /// Empty for a reference whose data is in the list it points at.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data: String,
    /// `(checksum "..")` as the file wrote it (`data_hash`): 32 upper-case hexadecimal digits of the MurmurHash3 of the content, or 64 lower-case ones of its SHA-256 in a file of
    /// KiCad 9.0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
}

impl EmbeddedFile {
    /// `EMBEDDED_FILE::GetLink`: `kicad-embed://name`, the string a model, a datasheet or a worksheet names the file by.
    pub fn link(&self) -> String {
        format!("{URI_SCHEME}{}", self.name)
    }

    /// The reference to the file: its name, type and checksum, no data -- what a footprint on a board lists of the files it uses.
    pub fn as_reference(&self) -> EmbeddedFile {
        EmbeddedFile { name: self.name.clone(), kind: self.kind, data: String::new(), checksum: self.checksum.clone() }
    }
}

/// A list of embedded files and whether fonts are embedded (`EMBEDDED_FILES`): `(embedded_fonts yes|no)` and `(embedded_files ..)` of a board, a schematic, a footprint or
/// a symbol.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddedFiles {
    /// `EMBEDDED_FILES::GetAreFontsEmbedded`: the fonts the text uses are embedded when the file is saved, and the fonts that were embedded before are dropped when it is not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fonts: bool,
    /// Sorted by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<EmbeddedFile>,
}

impl EmbeddedFiles {
    /// No files and fonts not embedded: the state of a design that never had any (so a design without them reads as before).
    pub fn is_empty(&self) -> bool {
        !self.fonts && self.files.is_empty()
    }

    /// `EMBEDDED_FILES::IsEmpty`: no files (whatever the fonts flag says).
    pub fn has_no_files(&self) -> bool {
        self.files.is_empty()
    }

    /// Where the file of this name is, or where it would go: a list read from a hand-edited `design.json` may not be sorted, so the search does not rely on it.
    fn position(&self, name: &str) -> Result<usize, usize> {
        match self.files.iter().position(|f| f.name == name) {
            Some(at) => Ok(at),
            None => Err(self.files.partition_point(|f| f.name.as_str() < name)),
        }
    }

    /// `EMBEDDED_FILES::GetEmbeddedFile`: the file of exactly this name.
    pub fn get(&self, name: &str) -> Option<&EmbeddedFile> {
        self.position(name).ok().map(|i| &self.files[i])
    }

    /// `EMBEDDED_FILES::HasFile`: whether a file is filed under the file-name part of `name`.
    pub fn has_file(&self, name: &str) -> bool {
        self.position(full_name_of(name)).is_ok()
    }

    /// `EMBEDDED_FILES::AddFile( EMBEDDED_FILE* )`: the file added, unless one of that name is there already (`std::map::insert` keeps the first). `true` when it was added.
    pub fn insert(&mut self, file: EmbeddedFile) -> bool {
        match self.position(&file.name) {
            Ok(_) => false,
            Err(at) => {
                self.files.insert(at, file);
                true
            }
        }
    }

    /// `EMBEDDED_FILES::AddFile( .., aOverwrite = true )`: the file added, replacing the one of the same name. Returns the file it replaced.
    pub fn put(&mut self, file: EmbeddedFile) -> Option<EmbeddedFile> {
        match self.position(&file.name) {
            Ok(at) => Some(std::mem::replace(&mut self.files[at], file)),
            Err(at) => {
                self.files.insert(at, file);
                None
            }
        }
    }

    /// `EMBEDDED_FILES::RemoveFile`: the file of this name taken out.
    pub fn remove(&mut self, name: &str) -> Option<EmbeddedFile> {
        self.position(name).ok().map(|i| self.files.remove(i))
    }

    /// `EMBEDDED_FILES::ClearEmbeddedFonts`: every font taken out.
    pub fn clear_fonts(&mut self) -> usize {
        let before = self.files.len();
        self.files.retain(|f| f.kind != EmbeddedFileType::Font);
        before - self.files.len()
    }

    /// The files a writer writes (`PCB_IO_KICAD_SEXPR::FormatBoardToFormatter`, `SCH_IO_KICAD_SEXPR::Format`, the library savers): the fonts are dropped when fonts are not
    /// embedded (`ClearEmbeddedFonts` before the write), and `WriteEmbeddedFiles` skips a file with no data. `with_data` is false for a footprint on a board, which lists the
    /// files it uses and leaves their data to the board's list: such a list keeps the references that have no data.
    pub fn to_write(&self, with_data: bool) -> Vec<&EmbeddedFile> {
        self.files.iter().filter(|f| (self.fonts || f.kind != EmbeddedFileType::Font) && (!with_data || !f.data.is_empty())).collect()
    }

    /// The files of `self` with the data of the file of the same name in `pool` where `self`'s own has none (`BOARD::FixupEmbeddedData`).
    pub fn fixed_up_from(&self, pool: &EmbeddedFiles) -> EmbeddedFiles {
        let mut out = self.clone();
        for f in &mut out.files {
            if let Some(src) = pool.get(&f.name) {
                f.data = src.data.clone();
                f.checksum = src.checksum.clone();
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------------------------- the lists of a design

/// Whose list: the thing a design keeps embedded files for. On the wire `{"kind": "board"}`, `{"kind": "footprint", "reference": "U1"}`, ...
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EmbeddedOwner {
    /// The board: Board Setup > Board Data > Embedded Files.
    Board,
    /// The schematic: Schematic Setup > Schematic Data > Embedded Files.
    Schematic,
    /// A footprint on the board, by reference: the Embedded Files tab of its Footprint Properties.
    Footprint { reference: String },
    /// A footprint of the project's footprint library (the Footprint Editor), by name.
    LibraryFootprint { name: String },
    /// A symbol of the project's symbol library (the Symbol Editor), by `lib_id`.
    LibrarySymbol { lib_id: String },
}

impl EmbeddedOwner {
    /// The name the owner goes by in a message and in the journal.
    pub fn label(&self) -> String {
        match self {
            EmbeddedOwner::Board => "board".into(),
            EmbeddedOwner::Schematic => "schematic".into(),
            EmbeddedOwner::Footprint { reference } => format!("footprint {reference}"),
            EmbeddedOwner::LibraryFootprint { name } => format!("library footprint {name}"),
            EmbeddedOwner::LibrarySymbol { lib_id } => format!("library symbol {lib_id}"),
        }
    }
}

/// Why an owner has no list: what is missing from the design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoSuchOwner {
    pub check: &'static str,
    pub what: String,
    pub reason: &'static str,
}

/// The list `owner` has in `design`; an error when the owner is not there (no such footprint on the board or in the library).
pub fn list_of(design: &crate::ir::Design, owner: &EmbeddedOwner) -> Result<EmbeddedFiles, NoSuchOwner> {
    match owner {
        EmbeddedOwner::Board => Ok(design.drawings.as_ref().map(|d| d.embedded.clone()).unwrap_or_default()),
        EmbeddedOwner::Schematic => Ok(design.schematic.as_ref().map(|s| s.extras.embedded.clone()).unwrap_or_default()),
        EmbeddedOwner::Footprint { reference } => {
            if !design.placement.as_ref().is_some_and(|p| p.footprints.iter().any(|f| &f.id == reference)) {
                return Err(NoSuchOwner { check: "ops_not_placed", what: reference.clone(), reason: "no footprint with this reference is on the board" });
            }
            Ok(of_footprint(design, reference).cloned().unwrap_or_default())
        }
        EmbeddedOwner::LibraryFootprint { name } => {
            let lib = design.footprint_library.as_ref();
            if lib.and_then(|l| l.by_name(name)).is_none() {
                return Err(NoSuchOwner { check: "ops_unknown_footprint", what: name.clone(), reason: "this footprint is not in the project library; open it in the Footprint Editor first" });
            }
            Ok(lib.and_then(|l| l.embedded.get(name)).cloned().unwrap_or_default())
        }
        EmbeddedOwner::LibrarySymbol { lib_id } => {
            let lib = design.symbol_library.as_ref();
            if lib.and_then(|l| l.by_lib_id(lib_id)).is_none() {
                return Err(NoSuchOwner { check: "ops_unknown_symbol", what: lib_id.clone(), reason: "this symbol is not in the project library; open it in the Symbol Editor first" });
            }
            Ok(lib.and_then(|l| l.embedded.get(lib_id)).cloned().unwrap_or_default())
        }
    }
}

/// Every file with data a lookup by name can find, in the order KiCad's resolver would try them for a model nobody scoped: the board's own, then the footprints' (the order they
/// are placed in), then the footprints of the project's footprint library (the Footprint Editor's 3D view asks for their models by the same route).
///
/// `FILENAME_RESOLVER::ResolvePath` takes a stack of lists and the first that has the name wins; the 3D view's stack is the footprint's own list, then the board's
/// (`RENDER_3D_OPENGL::load3dModels`). A board file holds one list for the board and its footprints, a name taken once ([`board_file_list`]), so the board's list first finds
/// what the footprint's would.
pub fn lookup_files(design: &crate::ir::Design) -> Vec<&EmbeddedFile> {
    let mut out: Vec<&EmbeddedFile> = Vec::new();
    if let Some(d) = design.drawings.as_ref() {
        out.extend(d.embedded.files.iter().filter(|f| !f.data.is_empty()));
    }
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        if let Some(list) = of_footprint(design, &fp.id) {
            out.extend(list.files.iter().filter(|f| !f.data.is_empty()));
        }
    }
    if let Some(lib) = design.footprint_library.as_ref() {
        out.extend(lib.embedded.values().flat_map(|l| l.files.iter()).filter(|f| !f.data.is_empty()));
    }
    out
}

/// The files a footprint on the board lists as its own: what the Embedded Files tab of its Footprint Properties set, else what the board file listed inside the footprint.
pub fn of_footprint<'a>(design: &'a crate::ir::Design, id: &str) -> Option<&'a EmbeddedFiles> {
    let drawings = design.drawings.as_ref()?;
    drawings.footprint_edits.iter().find(|e| e.id == id).and_then(|e| e.embedded.as_ref()).or_else(|| drawings.footprint_extras.iter().find(|e| e.id == id).map(|e| &e.embedded))
}

/// The one list a board file holds for the board and its footprints (`PCB_IO_KICAD_SEXPR::format( const BOARD* )`: "consolidate the embedded models in footprints into a single map"):
/// the board's own files, then the files of the footprints in the order they are placed, a name taken by the first that has it. Only the files with data are in it, and the
/// fonts are dropped when the board does not embed them. Its `fonts` is the board's.
pub fn board_file_list(design: &crate::ir::Design) -> EmbeddedFiles {
    let own = design.drawings.as_ref().map(|d| d.embedded.clone()).unwrap_or_default();
    let mut out = EmbeddedFiles { fonts: own.fonts, files: own.to_write(true).into_iter().cloned().collect() };
    for fp in design.placement.iter().flat_map(|p| p.footprints.iter()) {
        if let Some(list) = of_footprint(design, &fp.id) {
            for f in list.files.iter().filter(|f| !f.data.is_empty()) {
                out.insert(f.clone());
            }
        }
    }
    out
}

/// The files a footprint on the board lists inside its `(footprint ..)`: its own that have data -- its own, else the board's (`BOARD::FixupEmbeddedData`) -- which the footprint's
/// block names without their data (`EMBEDDED_FILES::WriteEmbeddedFiles( .., aWriteData = false )`).
pub fn footprint_listed_files(design: &crate::ir::Design, id: &str) -> Vec<EmbeddedFile> {
    let Some(list) = of_footprint(design, id) else { return Vec::new() };
    let fixed = match design.drawings.as_ref() {
        Some(d) => list.fixed_up_from(&d.embedded),
        None => list.clone(),
    };
    fixed.to_write(true).into_iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, data: &str) -> EmbeddedFile {
        EmbeddedFile { name: name.into(), kind: EmbeddedFileType::of_file_name(name), data: data.into(), checksum: "AB".into() }
    }

    #[test]
    fn the_type_comes_from_the_extension_as_add_file_has_it() {
        for (name, kind) in [
            ("a.step", EmbeddedFileType::Model),
            ("A.STP", EmbeddedFileType::Model),
            ("a.stpZ", EmbeddedFileType::Model),
            ("a.wrl", EmbeddedFileType::Model),
            ("a.WRZ", EmbeddedFileType::Model),
            ("f.ttf", EmbeddedFileType::Font),
            ("f.WOFF2", EmbeddedFileType::Font),
            ("f.otf", EmbeddedFileType::Font),
            ("d.pdf", EmbeddedFileType::Datasheet),
            ("s.kicad_wks", EmbeddedFileType::Worksheet),
            ("x.txt", EmbeddedFileType::Other),
            ("noext", EmbeddedFileType::Other),
            (".step", EmbeddedFileType::Other),
            ("dir/with.dots/x.step", EmbeddedFileType::Model),
            ("a.step.gz", EmbeddedFileType::Other),
        ] {
            assert_eq!(EmbeddedFileType::of_file_name(name), kind, "{name}");
        }
        for t in [EmbeddedFileType::Font, EmbeddedFileType::Model, EmbeddedFileType::Worksheet, EmbeddedFileType::Datasheet, EmbeddedFileType::Other] {
            assert_eq!(EmbeddedFileType::from_word(t.word()), Some(t));
        }
        assert_eq!(EmbeddedFileType::from_word("picture"), None);
    }

    #[test]
    fn the_list_is_sorted_and_the_first_file_of_a_name_wins() {
        let mut l = EmbeddedFiles::default();
        assert!(l.is_empty());
        assert!(l.insert(file("b.step", "B")));
        assert!(l.insert(file("a.pdf", "A")));
        assert!(l.insert(file("c.ttf", "C")));
        assert!(!l.insert(file("b.step", "OTHER")), "std::map::insert keeps the first");
        assert_eq!(l.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a.pdf", "b.step", "c.ttf"]);
        assert_eq!(l.get("b.step").unwrap().data, "B");
        assert!(l.has_file("some/dir/b.step"), "HasFile looks at the file name part");
        assert!(!l.has_file("b.stp"));
        let old = l.put(file("b.step", "NEW")).unwrap();
        assert_eq!(old.data, "B");
        assert_eq!(l.get("b.step").unwrap().data, "NEW");
        assert_eq!(l.files.len(), 3);
        assert_eq!(l.remove("a.pdf").unwrap().data, "A");
        assert!(l.remove("a.pdf").is_none());
    }

    #[test]
    fn fonts_are_dropped_on_write_unless_embedded_and_empty_files_never_written() {
        let mut l = EmbeddedFiles::default();
        l.insert(file("f.ttf", "F"));
        l.insert(file("m.step", "M"));
        l.insert(file("empty.pdf", ""));
        assert_eq!(l.to_write(true).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["m.step"]);
        // a footprint on a board lists references too, and keeps the data-less ones
        assert_eq!(l.to_write(false).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["empty.pdf", "m.step"]);
        l.fonts = true;
        assert_eq!(l.to_write(true).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["f.ttf", "m.step"]);
        assert_eq!(l.clear_fonts(), 1);
        assert!(l.get("f.ttf").is_none());
    }

    #[test]
    fn a_reference_takes_its_data_from_the_pool() {
        let mut pool = EmbeddedFiles::default();
        pool.insert(file("m.step", "DATA"));
        let mut fp = EmbeddedFiles::default();
        fp.insert(file("m.step", "").as_reference());
        fp.insert(file("lonely.pdf", ""));
        let fixed = fp.fixed_up_from(&pool);
        assert_eq!(fixed.get("m.step").unwrap().data, "DATA");
        assert_eq!(fixed.get("lonely.pdf").unwrap().data, "");
        assert_eq!(fixed.get("m.step").unwrap().link(), "kicad-embed://m.step");
        assert_eq!(name_of_link("kicad-embed://m.step"), Some("m.step"));
        assert_eq!(name_of_link("m.step"), None);
    }

    #[test]
    fn serde_leaves_out_what_is_empty_and_reads_what_it_wrote() {
        assert_eq!(serde_json::to_string(&EmbeddedFiles::default()).unwrap(), "{}");
        let mut l = EmbeddedFiles { fonts: true, files: Vec::new() };
        l.insert(file("m.step", "DATA"));
        let text = serde_json::to_string(&l).unwrap();
        assert!(text.contains("\"type\":\"model\"") && text.contains("\"fonts\":true"), "{text}");
        let back: EmbeddedFiles = serde_json::from_str(&text).unwrap();
        assert_eq!(back, l);
    }
}
