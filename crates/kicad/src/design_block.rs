//! Design blocks: a reusable piece of a design, kept in a `.kicad_blocks` library (`common/design_block.cpp`, `design_block_io.cpp`).
//!
//! A design block library is a **folder** named `<library>.kicad_blocks`; it holds one folder per block, `<name>.kicad_block`, and a block's folder holds
//!
//! * `<name>.kicad_sch` -- the schematic the block places (a sheet's file, or the contents of a selection saved as a sheet),
//! * `<name>.kicad_pcb` -- optionally, the layout the block places on a board (a board file, or the selection saved as one),
//! * `<name>.json` -- its metadata: `{"description": "...", "keywords": "...", "fields": {"Name": "value", ...}}`.
//!
//! Either of the two designs may be missing (a block saved from a board has no schematic until it is updated from one), and so may the metadata file (a
//! block with no description, keywords or fields).
//!
//! This is the pure half of the feature, ported from KiCad (read-only source, `8303b2ad`):
//!
//! * `DESIGN_BLOCK_IO::DesignBlockEnumerate`, `DesignBlockLoad`, `DesignBlockExists`, `DesignBlockSave`, `DesignBlockDelete`, `CreateLibrary`, `DeleteLibrary`
//!   and `IsLibraryWritable` (`common/design_block_io.cpp`) -- [`enumerate`], [`load`], [`exists`], [`save`], [`delete`], [`create_library`], [`delete_library`],
//!   [`is_writable`];
//! * `DESIGN_BLOCK` (`common/design_block.h`): the name, the description, the keywords and the ordered fields -- [`DesignBlock`];
//! * `DIALOG_DESIGN_BLOCK_PROPERTIES::TransferDataFromWindow` and `TransferDataFromGrid` -- [`name_problem`], [`clean_fields`];
//! * `DESIGN_BLOCK::GetSearchTerms` (the same terms `DESIGN_BLOCK_INFO::GetSearchTerms` gives) -- [`DesignBlock::search_terms`].
//!
//! A library is user data on disk: nothing here is called unless the person named the library (a row of a design block library table), and a block's
//! folder is the only thing written or removed.
//!
//! # Where this differs from KiCad
//!
//! * `DesignBlockEnumerate` names a block by the part of its folder's name before the **first** dot (`dirname.Before( '.' )`), so a block called `a.b` is listed as
//!   `a`, which is not a folder and cannot be loaded: such a block is invisible to KiCad. [`enumerate`] names it by the folder's name less `.kicad_block`, so a library
//!   written by another tool is read whole, and [`name_problem`] refuses a dot in the name of a block that is saved here, so that KiCad can read what the studio wrote.
//! * Nothing is written through a temporary file and a move of the *design* files as `wxCopyFile` does; the metadata is written the way `AtomicWriteFile` does
//!   (a temporary file, then a rename).

use eda_model::sheet_list::str_num_cmp;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};

/// `FILEEXT::KiCadDesignBlockLibPathExtension`: a library is a folder called `<name>.kicad_blocks`.
pub const LIBRARY_EXT: &str = "kicad_blocks";
/// `FILEEXT::KiCadDesignBlockPathExtension`: a block is a folder called `<name>.kicad_block`.
pub const BLOCK_EXT: &str = "kicad_block";
/// `FILEEXT::KiCadSchematicFileExtension`.
pub const SCHEMATIC_EXT: &str = "kicad_sch";
/// `FILEEXT::KiCadPcbFileExtension`.
pub const BOARD_EXT: &str = "kicad_pcb";
/// `FILEEXT::JsonFileExtension`: the metadata.
pub const METADATA_EXT: &str = "json";

/// A design block (`DESIGN_BLOCK`): what the metadata file says of it and where its two designs are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesignBlock {
    /// The block's name (`LIB_ID::GetLibItemName`): its folder is `<name>.kicad_block` and its files `<name>.<ext>`.
    pub name: String,
    pub description: String,
    pub keywords: String,
    /// The fields a placed block starts with, in the order the metadata file has them (`nlohmann::ordered_json`): the sheet's own fields when it is placed as a sheet.
    pub fields: Vec<(String, String)>,
    /// The schematic file (`GetSchematicFile`): in the library after a [`load`], or the file [`save`] copies in.
    pub schematic_file: Option<PathBuf>,
    /// The board file (`GetBoardFile`): likewise.
    pub board_file: Option<PathBuf>,
}

impl DesignBlock {
    /// `DESIGN_BLOCK::GetSearchTerms`: the library's nickname (weight 4), the name (8), `Lib:Name` (16), every word of the keywords (4), all of them as one text
    /// (1) and the description (1).
    pub fn search_terms(&self, nickname: &str) -> Vec<(String, u32)> {
        let mut terms = vec![(nickname.to_string(), 4), (self.name.clone(), 8), (format!("{nickname}:{}", self.name), 16)];
        terms.extend(self.keywords.split(' ').filter(|w| !w.is_empty()).map(|w| (w.to_string(), 4)));
        terms.push((self.keywords.clone(), 1));
        terms.push((self.description.clone(), 1));
        terms
    }

    /// The value of the field `name`, when the block has it.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

// ------------------------------------------------------------------------------------------------ paths

/// `<library>/<name>.kicad_block`.
pub fn block_folder(library: &Path, name: &str) -> PathBuf {
    library.join(format!("{name}.{BLOCK_EXT}"))
}

/// `<library>/<name>.kicad_block/<name>.<ext>`.
pub fn block_file(library: &Path, name: &str, ext: &str) -> PathBuf {
    block_folder(library, name).join(format!("{name}.{ext}"))
}

/// What `DesignBlockLoad` shows of a folder it names: the path with the trailing separator `wxFileName::GetPathSeparator()` leaves.
fn shown_folder(library: &Path, name: &str) -> String {
    format!("{}/", block_folder(library, name).display())
}

// ------------------------------------------------------------------------------------------------ names

/// Why `name` cannot name a block, in KiCad's words, or `None`. `DIALOG_DESIGN_BLOCK_PROPERTIES::TransferDataFromWindow`: a character a library item may not have
/// (`LIB_ID::FindIllegalLibraryNameChar`: a control character, a backslash or a colon) and `/`, "a path character which is illegal for design blocks and footprints". A
/// name cannot be empty (`DESIGN_BLOCK_IO::DesignBlockSave`: "Design block does not have a valid library ID.") and, here, holds no dot (see the module doc).
pub fn name_problem(name: &str) -> Option<String> {
    if let Some(ch) = name.chars().find(|&c| (c as u32) < 0x20 || c == '\\' || c == ':' || c == '/') {
        return Some(format!("Illegal character '{ch}' in name '{name}'."));
    }
    if name.is_empty() {
        return Some("Design block does not have a valid library ID.".to_string());
    }
    if name.contains('.') {
        return Some(format!("Illegal character '.' in name '{name}': KiCad lists a design block by the part of its folder's name before the first dot, so a dot would hide it from KiCad."));
    }
    None
}

/// `DIALOG_DESIGN_BLOCK_PROPERTIES::TransferDataFromGrid`: a field's name is stripped, its line breaks removed and its double spaces made single; two fields of one
/// name are refused ("Duplicate fields are not allowed."). The values are kept as typed. Returns the cleaned list.
pub fn clean_fields(fields: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, value) in fields {
        let name = name.trim().replace('\n', "").replace("  ", " ");
        if out.iter().any(|(n, _)| *n == name) {
            return Err("Duplicate fields are not allowed.".to_string());
        }
        out.push((name, value.clone()));
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ the metadata file

/// A JSON object read as the pairs it lists, in the order the file has them (`nlohmann::ordered_json`): `serde_json::Map` sorts its keys.
struct Ordered(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Ordered, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object of strings")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut out: Vec<(String, String)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, String>()? {
                    // `GetFields()[name] = value`: a name met twice keeps its first place and takes the later value.
                    match out.iter_mut().find(|(n, _)| *n == k) {
                        Some(slot) => slot.1 = v,
                        None => out.push((k, v)),
                    }
                }
                Ok(Ordered(out))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
struct Metadata {
    description: Option<String>,
    keywords: Option<String>,
    fields: Option<Ordered>,
}

/// The text `dbMetadata.dump( 0 )` gives for a block: `nlohmann::json` with an indent of 0 puts every member on a line of its own, flush left, and an empty object as
/// `{}`. The keys are in the order they were assigned: description, keywords, fields.
pub fn metadata_text(block: &DesignBlock) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("\"description\": {},\n", json_string(&block.description)));
    out.push_str(&format!("\"keywords\": {},\n", json_string(&block.keywords)));
    if block.fields.is_empty() {
        out.push_str("\"fields\": {}\n");
    } else {
        out.push_str("\"fields\": {\n");
        let last = block.fields.len() - 1;
        for (i, (name, value)) in block.fields.iter().enumerate() {
            out.push_str(&format!("{}: {}{}\n", json_string(name), json_string(value), if i == last { "" } else { "," }));
        }
        out.push_str("}\n");
    }
    out.push('}');
    out
}

/// `nlohmann::json`'s string: quotes and backslashes escaped, `\b \f \n \r \t` by letter, the other control characters as `\u00xx`; everything else, UTF-8 included, as it is.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ------------------------------------------------------------------------------------------------ reading

/// `DESIGN_BLOCK_IO::DesignBlockEnumerate`: the names of the blocks of the library folder `library`, in the order a tree lists them (names compared naturally,
/// without regard to case). An error when the folder cannot be opened.
pub fn enumerate(library: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(library).map_err(|_| format!("Design block '{}' does not exist.", library.display()))?;
    let suffix = format!(".{BLOCK_EXT}");
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            // `wxDIR_DIRS` without `wxDIR_HIDDEN`: a folder whose name starts with a dot is skipped.
            let stem = file.strip_suffix(&suffix)?;
            (!file.starts_with('.') && !stem.is_empty()).then(|| stem.to_string())
        })
        .collect();
    names.sort_by(|a, b| str_num_cmp(&a.to_lowercase(), &b.to_lowercase()).then_with(|| a.cmp(b)));
    Ok(names)
}

/// `DESIGN_BLOCK_IO::DesignBlockExists`.
pub fn exists(library: &Path, name: &str) -> bool {
    block_folder(library, name).is_dir()
}

/// `DESIGN_BLOCK_IO::DesignBlockLoad`: the block `name` of `library` -- its two files when they are there and its metadata when the metadata file is.
pub fn load(library: &Path, name: &str) -> Result<DesignBlock, String> {
    if !exists(library, name) {
        return Err(format!("Design block '{}' does not exist.", shown_folder(library, name)));
    }
    let mut block = DesignBlock { name: name.to_string(), ..Default::default() };
    let sch = block_file(library, name, SCHEMATIC_EXT);
    let pcb = block_file(library, name, BOARD_EXT);
    let meta = block_file(library, name, METADATA_EXT);
    if sch.is_file() {
        block.schematic_file = Some(sch);
    }
    if pcb.is_file() {
        block.board_file = Some(pcb);
    }
    if meta.is_file() {
        let unreadable = || format!("Design block metadata file '{}' could not be read.", meta.display());
        let text = std::fs::read_to_string(&meta).map_err(|_| unreadable())?;
        let parsed: Metadata = serde_json::from_str(&text).map_err(|_| unreadable())?;
        block.description = parsed.description.unwrap_or_default();
        block.keywords = parsed.keywords.unwrap_or_default();
        block.fields = parsed.fields.map(|o| o.0).unwrap_or_default();
    }
    Ok(block)
}

// ------------------------------------------------------------------------------------------------ writing

/// `DESIGN_BLOCK_IO::IsLibraryWritable` (common/design_block_io.cpp): `path.IsOk() && path.IsDirWritable()`, and `wxFileName::IsDirWritable` is
/// `access( dir, W_OK ) == 0`: whether *this user* may write the folder, not whether any write bit is set (r6 B6-1: a root-owned 0755 folder is read only to
/// everyone else).
///
/// Differs: std has no `access`, and no crate is added for it, so on Linux the process's effective uid, gid and supplementary groups are read from
/// `/proc/self/status` and the folder's mode bits are tested as the kernel does (owner bits for the owner, else group bits for a member, else the other bits;
/// root may always write). POSIX ACLs and a read-only mount are not seen. Elsewhere (no `/proc`) it falls back to "some write bit is set".
pub fn is_writable(library: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(library) else { return false };
    if !meta.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Some(who) = Credentials::of_this_process() {
            return who.may_write(meta.uid(), meta.gid(), meta.mode());
        }
    }
    !meta.permissions().readonly()
}

/// The effective uid, gid and supplementary groups of this process (`/proc/self/status`'s `Uid:`, `Gid:` and `Groups:` lines; the second number is the
/// effective one).
#[cfg(unix)]
#[derive(Debug, Clone)]
struct Credentials {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

#[cfg(unix)]
impl Credentials {
    fn of_this_process() -> Option<Credentials> {
        Self::parse(&std::fs::read_to_string("/proc/self/status").ok()?)
    }

    fn parse(status: &str) -> Option<Credentials> {
        let field = |key: &str| status.lines().find_map(|l| l.strip_prefix(key)).map(|rest| rest.split_whitespace().filter_map(|n| n.parse::<u32>().ok()).collect::<Vec<_>>());
        let uid = *field("Uid:")?.get(1)?;
        let gid = *field("Gid:")?.get(1)?;
        Some(Credentials { uid, gid, groups: field("Groups:").unwrap_or_default() })
    }

    /// `access( W_OK )`'s permission test on a file owned by `owner:group` with `mode`.
    fn may_write(&self, owner: u32, group: u32, mode: u32) -> bool {
        if self.uid == 0 {
            return true;
        }
        if self.uid == owner {
            return mode & 0o200 != 0;
        }
        if self.gid == group || self.groups.contains(&group) {
            return mode & 0o020 != 0;
        }
        mode & 0o002 != 0
    }
}

/// `DESIGN_BLOCK_IO::CreateLibrary`: the library folder. An existing path is refused ("Cannot overwrite library path").
pub fn create_library(library: &Path) -> Result<(), String> {
    if library.is_dir() {
        return Err(format!("Cannot overwrite library path '{}'.", library.display()));
    }
    std::fs::create_dir(library).map_err(|_| format!("Library path '{}' could not be created.\n\nMake sure you have write permissions and try again.", library.display()))
}

/// `DESIGN_BLOCK_IO::DeleteLibrary`: the library folder and the blocks in it. A folder with a file in it, or one with a folder that is not a block's, is refused:
/// a library holds nothing else and the person's other files are not ours to remove. `Ok(false)` when there is no such folder.
pub fn delete_library(library: &Path) -> Result<bool, String> {
    if !library.is_dir() {
        return Ok(false);
    }
    if !is_writable(library) {
        return Err(format!("Insufficient permissions to delete folder '{}'.", library.display()));
    }
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(library).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).collect();
    if entries.iter().any(|e| !e.path().is_dir()) {
        return Err(format!("Library folder '{}' has unexpected files.", library.display()));
    }
    // KiCad's own check wants each sub-folder to end in `.kicad_blocks`, which a library's block folders (`.kicad_block`) never do and so refuses every library that has
    // a block in it; the check that matters is that each is a block's folder.
    if let Some(odd) = entries.iter().find(|e| !e.file_name().to_string_lossy().ends_with(&format!(".{BLOCK_EXT}"))) {
        return Err(format!("Unexpected folder '{}' found in library path '{}'.", odd.path().display(), library.display()));
    }
    std::fs::remove_dir_all(library).map_err(|_| format!("Design block library '{}' cannot be deleted.", library.display()))?;
    Ok(true)
}

/// `DESIGN_BLOCK_IO::DesignBlockSave`: the block's folder in `library` made if it is not there, its schematic and its board copied in under the block's name (a file
/// already in place is not copied over itself: that is how a block's metadata alone is changed) and its metadata written. The block must have a name that can name a
/// folder, and a schematic or a board; the source files must exist.
pub fn save(library: &Path, block: &DesignBlock) -> Result<(), String> {
    if let Some(problem) = name_problem(&block.name) {
        return Err(problem);
    }
    if block.schematic_file.is_none() && block.board_file.is_none() {
        return Err("Design block does not have a schematic or board file.".to_string());
    }
    if let Some(sch) = block.schematic_file.as_ref().filter(|p| !p.is_file()) {
        return Err(format!("Schematic source file '{}' does not exist.", sch.display()));
    }
    if let Some(pcb) = block.board_file.as_ref().filter(|p| !p.is_file()) {
        return Err(format!("Board source file '{}' does not exist.", pcb.display()));
    }
    let folder = block_folder(library, &block.name);
    if !folder.is_dir() {
        std::fs::create_dir(&folder).map_err(|_| format!("Design block folder '{}/' could not be created.", folder.display()))?;
    }
    let same = |a: &Path, b: &Path| a == b || std::fs::canonicalize(a).ok().zip(std::fs::canonicalize(b).ok()).is_some_and(|(a, b)| a == b);
    if let Some(sch) = &block.schematic_file {
        let to = block_file(library, &block.name, SCHEMATIC_EXT);
        if !same(sch, &to) {
            std::fs::copy(sch, &to).map_err(|_| format!("Schematic file '{}' could not be saved as design block at '{}'.", sch.display(), to.display()))?;
        }
    }
    if let Some(pcb) = &block.board_file {
        let to = block_file(library, &block.name, BOARD_EXT);
        if !same(pcb, &to) {
            std::fs::copy(pcb, &to).map_err(|_| format!("Board file '{}' could not be saved as design block at '{}'.", pcb.display(), to.display()))?;
        }
    }
    let meta = block_file(library, &block.name, METADATA_EXT);
    let failed = || format!("Design block metadata file '{}' could not be saved.", meta.display());
    let tmp = meta.with_extension("json.tmp");
    std::fs::write(&tmp, metadata_text(block)).and_then(|()| std::fs::rename(&tmp, &meta)).map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        failed()
    })
}

/// `DESIGN_BLOCK_IO::DesignBlockDelete`: the block's whole folder.
pub fn delete(library: &Path, name: &str) -> Result<(), String> {
    let folder = block_folder(library, name);
    if !folder.is_dir() {
        return Err(format!("Design block '{name}.{BLOCK_EXT}' does not exist."));
    }
    std::fs::remove_dir_all(&folder).map_err(|_| format!("Design block folder '{}' could not be deleted.", folder.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r6 B6-1: `IsDirWritable` is `access( W_OK )` for this user, so a root-owned 0755 folder is read only to anyone else (the write bits say "writable").
    #[cfg(unix)]
    #[test]
    fn a_folder_owned_by_another_user_is_read_only() {
        let me = Credentials::of_this_process().expect("/proc/self/status");
        if me.uid != 0 {
            assert!(!is_writable(Path::new("/usr")), "/usr is root's");
        }
        let own = temp("writable");
        assert!(is_writable(&own));
        let user = Credentials { uid: 1000, gid: 1000, groups: vec![27] };
        assert!(!user.may_write(0, 0, 0o755));
        assert!(user.may_write(1000, 0, 0o755));
        assert!(!user.may_write(1000, 1000, 0o575), "the owner's bits decide for the owner");
        assert!(user.may_write(0, 27, 0o775), "a supplementary group");
        assert!(user.may_write(0, 0, 0o757));
        assert!(Credentials { uid: 0, gid: 0, groups: vec![] }.may_write(5, 5, 0o500));
        let parsed = Credentials::parse("Name:\tx\nUid:\t1\t2\t2\t2\nGid:\t3\t4\t4\t4\nGroups:\t4 9 \n").unwrap();
        assert_eq!((parsed.uid, parsed.gid, parsed.groups), (2, 4, vec![4, 9]));
        let _ = std::fs::remove_dir_all(&own);
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-design-block-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn block(name: &str) -> DesignBlock {
        DesignBlock {
            name: name.into(),
            description: "A buck converter".into(),
            keywords: "power buck 5V".into(),
            fields: vec![("Zeta".into(), "last".into()), ("Alpha".into(), "first".into()), ("Part".into(), "TPS62".into())],
            ..Default::default()
        }
    }

    #[test]
    fn the_metadata_is_written_the_way_nlohmann_dumps_it_and_read_back_in_order() {
        let text = metadata_text(&block("Buck"));
        assert_eq!(text, "{\n\"description\": \"A buck converter\",\n\"keywords\": \"power buck 5V\",\n\"fields\": {\n\"Zeta\": \"last\",\n\"Alpha\": \"first\",\n\"Part\": \"TPS62\"\n}\n}");
        let empty = DesignBlock { name: "E".into(), ..Default::default() };
        assert_eq!(metadata_text(&empty), "{\n\"description\": \"\",\n\"keywords\": \"\",\n\"fields\": {}\n}");
        // quotes, backslashes, line breaks and other control characters are escaped; UTF-8 is left as it is
        let odd = DesignBlock { name: "O".into(), description: "say \"hi\"\\\n\tcaf\u{e9} \u{1}".into(), ..Default::default() };
        assert_eq!(metadata_text(&odd), "{\n\"description\": \"say \\\"hi\\\"\\\\\\n\\tcaf\u{e9} \\u0001\",\n\"keywords\": \"\",\n\"fields\": {}\n}");
        let parsed: Metadata = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.fields.unwrap().0, block("Buck").fields, "the fields keep the file's order");
    }

    #[test]
    fn a_block_saved_is_a_block_loaded_with_its_files_and_metadata() {
        let root = temp("roundtrip");
        let lib = root.join("Mine.kicad_blocks");
        create_library(&lib).unwrap();
        assert!(create_library(&lib).unwrap_err().starts_with("Cannot overwrite library path"));
        assert_eq!(enumerate(&lib).unwrap(), Vec::<String>::new());
        let (sch, pcb) = (root.join("tmp1.kicad_sch"), root.join("tmp2.kicad_pcb"));
        std::fs::write(&sch, "(kicad_sch)").unwrap();
        std::fs::write(&pcb, "(kicad_pcb)").unwrap();
        let mut b = block("Buck");
        b.schematic_file = Some(sch);
        b.board_file = Some(pcb);
        save(&lib, &b).unwrap();
        assert!(lib.join("Buck.kicad_block").join("Buck.kicad_sch").is_file());
        assert!(lib.join("Buck.kicad_block").join("Buck.kicad_pcb").is_file());
        assert!(lib.join("Buck.kicad_block").join("Buck.json").is_file());
        assert!(!lib.join("Buck.kicad_block").join("Buck.json.tmp").exists());
        assert_eq!(enumerate(&lib).unwrap(), vec!["Buck"]);
        assert!(exists(&lib, "Buck") && !exists(&lib, "Boost"));
        let back = load(&lib, "Buck").unwrap();
        assert_eq!((back.description.as_str(), back.keywords.as_str()), ("A buck converter", "power buck 5V"));
        assert_eq!(back.fields, b.fields);
        assert_eq!(back.schematic_file, Some(block_file(&lib, "Buck", "kicad_sch")));
        assert_eq!(back.board_file, Some(block_file(&lib, "Buck", "kicad_pcb")));
        assert_eq!(back.field("Part"), Some("TPS62"));
        // saving the loaded block again changes its metadata alone: the files are in place
        let mut edited = back.clone();
        edited.keywords = "power".into();
        save(&lib, &edited).unwrap();
        assert_eq!(load(&lib, "Buck").unwrap().keywords, "power");
        assert_eq!(std::fs::read_to_string(block_file(&lib, "Buck", "kicad_sch")).unwrap(), "(kicad_sch)");
        // a rename is a save under the new name and a delete of the old
        let mut renamed = load(&lib, "Buck").unwrap();
        renamed.name = "Buck5V".into();
        save(&lib, &renamed).unwrap();
        delete(&lib, "Buck").unwrap();
        assert_eq!(enumerate(&lib).unwrap(), vec!["Buck5V"]);
        assert_eq!(std::fs::read_to_string(block_file(&lib, "Buck5V", "kicad_sch")).unwrap(), "(kicad_sch)");
        assert!(delete(&lib, "Buck").unwrap_err().contains("Buck.kicad_block' does not exist."));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_block_with_one_design_or_no_metadata_loads_and_a_bad_one_is_named() {
        let root = temp("partial");
        let lib = root.join("L.kicad_blocks");
        create_library(&lib).unwrap();
        // a block KiCad's tools left with only a layout and no metadata file
        std::fs::create_dir_all(lib.join("OnlyBoard.kicad_block")).unwrap();
        std::fs::write(lib.join("OnlyBoard.kicad_block").join("OnlyBoard.kicad_pcb"), "(kicad_pcb)").unwrap();
        let b = load(&lib, "OnlyBoard").unwrap();
        assert!(b.schematic_file.is_none() && b.board_file.is_some() && b.description.is_empty() && b.fields.is_empty());
        // metadata that is not the object KiCad reads
        std::fs::create_dir_all(lib.join("Bad.kicad_block")).unwrap();
        std::fs::write(lib.join("Bad.kicad_block").join("Bad.json"), "{\"description\": 3}").unwrap();
        assert!(load(&lib, "Bad").unwrap_err().contains("Bad.json' could not be read."));
        std::fs::write(lib.join("Bad.kicad_block").join("Bad.json"), "{\"fields\": {\"a\": \"1\", \"a\": \"2\", \"b\": \"3\"}}").unwrap();
        assert_eq!(load(&lib, "Bad").unwrap().fields, vec![("a".to_string(), "2".to_string()), ("b".to_string(), "3".to_string())], "a name met twice keeps its place and the later value");
        // folders that are not blocks are not listed: a file, a hidden folder, a folder of another kind
        std::fs::write(lib.join("README.kicad_block"), "x").unwrap();
        std::fs::create_dir_all(lib.join(".hidden.kicad_block")).unwrap();
        std::fs::create_dir_all(lib.join("Other.pretty")).unwrap();
        assert_eq!(enumerate(&lib).unwrap(), vec!["Bad", "OnlyBoard"]);
        assert!(enumerate(&root.join("none.kicad_blocks")).unwrap_err().ends_with("none.kicad_blocks' does not exist."));
        assert!(load(&lib, "Nope").unwrap_err().starts_with("Design block '") && load(&lib, "Nope").unwrap_err().ends_with("Nope.kicad_block/' does not exist."));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_refuses_what_kicad_refuses_and_a_name_that_would_hide_the_block() {
        let root = temp("refuse");
        let lib = root.join("L.kicad_blocks");
        create_library(&lib).unwrap();
        let src = root.join("a.kicad_sch");
        std::fs::write(&src, "(kicad_sch)").unwrap();
        let with = |name: &str| DesignBlock { name: name.into(), schematic_file: Some(src.clone()), ..Default::default() };
        assert_eq!(save(&lib, &with("")).unwrap_err(), "Design block does not have a valid library ID.");
        assert_eq!(save(&lib, &with("a/b")).unwrap_err(), "Illegal character '/' in name 'a/b'.");
        assert_eq!(save(&lib, &with("a:b")).unwrap_err(), "Illegal character ':' in name 'a:b'.");
        assert!(save(&lib, &with("a.b")).unwrap_err().starts_with("Illegal character '.' in name 'a.b'"));
        assert!(save(&lib, &with("..")).is_err());
        assert_eq!(save(&lib, &DesignBlock { name: "x".into(), ..Default::default() }).unwrap_err(), "Design block does not have a schematic or board file.");
        let ghost = DesignBlock { name: "x".into(), schematic_file: Some(root.join("ghost.kicad_sch")), ..Default::default() };
        assert!(save(&lib, &ghost).unwrap_err().starts_with("Schematic source file '"));
        let ghost = DesignBlock { name: "x".into(), board_file: Some(root.join("ghost.kicad_pcb")), ..Default::default() };
        assert!(save(&lib, &ghost).unwrap_err().starts_with("Board source file '"));
        assert_eq!(enumerate(&lib).unwrap(), Vec::<String>::new(), "nothing was written by a refused save");
        // an accepted name may hold spaces, dashes and non-ASCII letters
        save(&lib, &with("Caf\u{e9} 5-V")).unwrap();
        assert_eq!(enumerate(&lib).unwrap(), vec!["Caf\u{e9} 5-V"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fields_are_cleaned_as_the_grid_does_and_search_terms_are_the_trees() {
        let cleaned = clean_fields(&[("  Part \n Number  ".into(), "x".into()), ("Maker".into(), "y".into())]).unwrap();
        assert_eq!(cleaned[0].0, "Part  Number".replace("  ", " "));
        assert_eq!(cleaned.len(), 2);
        assert_eq!(clean_fields(&[("a".into(), "1".into()), (" a ".into(), "2".into())]).unwrap_err(), "Duplicate fields are not allowed.");
        let terms = block("Buck").search_terms("Mine");
        assert_eq!(terms[..3], [("Mine".to_string(), 4), ("Buck".to_string(), 8), ("Mine:Buck".to_string(), 16)]);
        assert_eq!(terms[3..6].iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), vec!["power", "buck", "5V"]);
        assert_eq!(terms[6], ("power buck 5V".to_string(), 1));
        assert_eq!(terms[7], ("A buck converter".to_string(), 1));
    }

    #[test]
    fn a_library_with_blocks_is_deleted_whole_and_one_with_anything_else_is_not() {
        let root = temp("deletelib");
        let lib = root.join("L.kicad_blocks");
        create_library(&lib).unwrap();
        std::fs::create_dir_all(lib.join("A.kicad_block")).unwrap();
        std::fs::write(lib.join("A.kicad_block").join("A.json"), "{}").unwrap();
        std::fs::write(lib.join("notes.txt"), "mine").unwrap();
        assert!(delete_library(&lib).unwrap_err().contains("has unexpected files"));
        std::fs::remove_file(lib.join("notes.txt")).unwrap();
        std::fs::create_dir_all(lib.join("Photos")).unwrap();
        assert!(delete_library(&lib).unwrap_err().contains("Unexpected folder"));
        std::fs::remove_dir_all(lib.join("Photos")).unwrap();
        assert!(delete_library(&lib).unwrap());
        assert!(!lib.exists());
        assert!(!delete_library(&lib).unwrap(), "no such folder is not an error");
        let _ = std::fs::remove_dir_all(&root);
    }
}
