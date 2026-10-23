//! Embedded files, the codec and the file text (`common/embedded_files.cpp`, `include/embedded_files.h`, `common/embedded_files_parser.cpp`).
//!
//! A file KiCad embeds is stored as `(file (name "x.step") (type model) (data |<base64>|) (checksum "<hex>"))`: the content compressed as one zstd frame at level 15
//! (`EMBEDDED_FILES::CompressAndEncode`), the frame in base64, and a checksum of the *content* -- the 128-bit MurmurHash3 (x64) of it with the seed `0xABBA2345` (`MMH3_HASH`),
//! 32 upper-case hexadecimal digits. Files saved by KiCad 9.0 have a SHA-256 of the content there (64 digits), and files saved by the first KiCad 10 builds a MurmurHash3 with
//! a bug in its last block (`addDataV1`); `DecompressAndDecode` takes all three.
//!
//! The design keeps a file as it was read ([`EmbeddedFile::data`] is the base64 text, [`EmbeddedFile::checksum`] the checksum the file had); this module turns those into the bytes
//! ([`decode`]) and the bytes into those ([`encode_new`]), reads the `(embedded_files ..)` block of a board, a schematic, a footprint or a symbol ([`read_owner`]) and writes it
//! as KiCad writes it ([`write_block`]: `WriteEmbeddedFiles`, then the pretty-printer's layout).
//!
//! `zstd` (the C library through the `zstd` crate) is the one dependency added for this. `ZSTD_compress` writes the content size into the frame header, and KiCad refuses a
//! frame that does not tell it (`ZSTD_getFrameContentSize`), so [`compress`] is `compress2` with the content size flag on, which is what the crate's one-shot call does.

use crate::sexpr::{self, Sexpr};
use eda_model::embedded_files::{EmbeddedFile, EmbeddedFileType, EmbeddedFiles};
use std::fmt::Write as _;

/// `EMBEDDED_FILES::Seed()`.
pub const SEED: u32 = 0xABBA_2345;
/// The level `CompressAndEncode` compresses at.
pub const ZSTD_LEVEL: i32 = 15;
/// `DecompressAndDecode`: "Limit to 1GB".
pub const MAX_CONTENT_BYTES: u64 = 1_000_000_000;
/// `WriteEmbeddedFiles`' `MIME_BASE64_LENGTH`: the characters of a line of the file's `(data ..)`.
const LINE: usize = 76;

/// Why a file's content cannot be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// The file has no data (a reference to a file the board holds, or a KiCad 9.0.0 file with an empty `(data)`).
    NoData,
    /// The data is not base64 (`wxBase64Decode` failed).
    Base64,
    /// The data is not a zstd frame that tells its content size, or the content is larger than KiCad reads (`ZSTD_getFrameContentSize`).
    Frame(String),
    /// The frame does not decompress (`ZSTD_decompress` failed).
    Decompress(String),
    /// The content is not what the checksum says (`RETURN_CODE::CHECKSUM_ERROR`).
    Checksum,
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedError::NoData => write!(f, "the file has no data"),
            EmbedError::Base64 => write!(f, "the data is not base64"),
            EmbedError::Frame(why) => write!(f, "the data is not a compressed frame KiCad reads ({why})"),
            EmbedError::Decompress(why) => write!(f, "the data does not decompress ({why})"),
            EmbedError::Checksum => write!(f, "checksum error: the content is not what the checksum says"),
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------ base64

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `wxBase64Encode`: the standard alphabet, `=` padded, no line breaks.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `wxBase64Decode` in strict mode: any character outside the alphabet (white space too) and any `=` before the end fails the whole text.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, EmbedError> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(EmbedError::Base64);
    }
    let value = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (g, quad) in bytes.chunks(4).enumerate() {
        let last = g + 1 == groups;
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err(EmbedError::Base64);
        }
        let mut n = 0u32;
        for (k, &c) in quad.iter().enumerate() {
            let v = if k >= 4 - pad { 0 } else { value(c).ok_or(EmbedError::Base64)? };
            n = n << 6 | v;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------------------------ checksums

const C1: u64 = 0x87c3_7b91_1142_53d5;
const C2: u64 = 0x4cf5_ad43_2745_937f;

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^= k >> 33;
    k
}

/// `MMH3_HASH`'s state: `h1`, `h2` and the length counted so far (a 32-bit counter, as in `mmh3_hash.h`).
struct Mmh3 {
    h1: u64,
    h2: u64,
    len: u32,
}

impl Mmh3 {
    fn new(seed: u32) -> Mmh3 {
        Mmh3 { h1: seed as u64, h2: seed as u64, len: 0 }
    }

    fn block(&mut self, b: &[u8; 16]) {
        let mut k1 = u64::from_le_bytes(b[..8].try_into().unwrap());
        let mut k2 = u64::from_le_bytes(b[8..].try_into().unwrap());
        k1 = k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
        self.h1 ^= k1;
        self.h1 = self.h1.rotate_left(27).wrapping_add(self.h2).wrapping_mul(5).wrapping_add(0x52dc_e729);
        k2 = k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
        self.h2 ^= k2;
        self.h2 = self.h2.rotate_left(31).wrapping_add(self.h1).wrapping_mul(5).wrapping_add(0x3849_5ab5);
    }

    /// `hashTail`: the last `len & 15` bytes of the 16-byte buffer, zero padded.
    fn tail(&mut self, buf: &[u8; 16]) {
        let (mut k1, mut k2) = (0u64, 0u64);
        let n = (self.len & 15) as usize;
        for i in (8..n).rev() {
            k2 ^= (buf[i] as u64) << ((i - 8) * 8);
        }
        if n > 8 {
            k2 = k2.wrapping_mul(C2).rotate_left(33).wrapping_mul(C1);
            self.h2 ^= k2;
        }
        for i in (0..n.min(8)).rev() {
            k1 ^= (buf[i] as u64) << (i * 8);
        }
        if n > 0 {
            k1 = k1.wrapping_mul(C1).rotate_left(31).wrapping_mul(C2);
            self.h1 ^= k1;
        }
    }

    fn finish(mut self, buf: &[u8; 16]) -> (u64, u64) {
        self.tail(buf);
        self.h1 ^= self.len as u64;
        self.h2 ^= self.len as u64;
        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);
        self.h1 = fmix64(self.h1);
        self.h2 = fmix64(self.h2);
        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);
        (self.h1, self.h2)
    }
}

/// `MMH3_HASH::addData` and `digest`: MurmurHash3_x64_128 of `data`, as `(Value64[0], Value64[1])`.
pub fn mmh3_x64_128(data: &[u8], seed: u32) -> (u64, u64) {
    let mut h = Mmh3::new(seed);
    let mut buf = [0u8; 16];
    let mut rest = data;
    while rest.len() >= 16 {
        buf.copy_from_slice(&rest[..16]);
        h.block(&buf);
        rest = &rest[16..];
        h.len = h.len.wrapping_add(16);
    }
    buf = [0u8; 16];
    buf[..rest.len()].copy_from_slice(rest);
    h.len = h.len.wrapping_add(rest.len() as u32);
    h.finish(&buf)
}

/// `MMH3_HASH::addDataV1`, the hash of the first KiCad 10 files: the tail is rounded up to a multiple of 4 bytes (a whole 4 more when it already is one), the padding counted in the
/// length, so a tail of 12 or more bytes is never hashed at all.
pub fn mmh3_v1_x64_128(data: &[u8], seed: u32) -> (u64, u64) {
    let mut h = Mmh3::new(seed);
    let mut buf = [0u8; 16];
    let mut rest = data;
    while rest.len() >= 16 {
        buf.copy_from_slice(&rest[..16]);
        h.block(&buf);
        rest = &rest[16..];
        h.len = h.len.wrapping_add(16);
    }
    buf = [0u8; 16];
    if !rest.is_empty() {
        buf[..rest.len()].copy_from_slice(rest);
        let padding = 4 - (rest.len() + 4) % 4;
        h.len = h.len.wrapping_add((rest.len() + padding) as u32);
    }
    h.finish(&buf)
}

/// `HASH_128::ToString`: the two halves in upper-case hexadecimal, 16 digits each.
fn hash_string(h: (u64, u64)) -> String {
    format!("{:016X}{:016X}", h.0, h.1)
}

/// The checksum KiCad writes for a file with this content (`EMBEDDED_FILE::Validate`): 32 upper-case hexadecimal digits.
pub fn checksum_of(content: &[u8]) -> String {
    hash_string(mmh3_x64_128(content, SEED))
}

/// SHA-256 as 64 lower-case hexadecimal digits (`picosha2::hash256_hex_string`), the checksum of a file saved by KiCad 9.0.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7,
        0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in padded.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    h.iter().fold(String::new(), |mut s, v| {
        let _ = write!(s, "{v:08x}");
        s
    })
}

// ------------------------------------------------------------------------------------------------------------------ compress and decompress

/// What a file holds, once decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub bytes: Vec<u8>,
    /// The checksum KiCad keeps for the file after it has read it (`data_hash = new_hash`): the current MurmurHash3 of the content, whatever the file said.
    pub checksum: String,
    /// The checksum in the file is not the current kind (SHA-256 or the first MurmurHash3): a save by KiCad writes the current one.
    pub legacy_checksum: bool,
}

/// `EMBEDDED_FILES::CompressAndEncode`'s compression: one zstd frame at level 15, content size in its header.
pub fn compress(content: &[u8]) -> Result<Vec<u8>, EmbedError> {
    zstd::bulk::compress(content, ZSTD_LEVEL).map_err(|e| EmbedError::Frame(e.to_string()))
}

/// A new embedded file (`EMBEDDED_FILES::AddFile` from a file on disk): `name` filed under its file name, the type from its extension, the content compressed, encoded and checksummed.
pub fn encode_new(name: &str, content: &[u8]) -> Result<EmbeddedFile, EmbedError> {
    let name = eda_model::embedded_files::full_name_of(name);
    Ok(EmbeddedFile { name: name.to_string(), kind: EmbeddedFileType::of_file_name(name), data: base64_encode(&compress(content)?), checksum: checksum_of(content) })
}

/// The content size the zstd frame at the start of `frame` tells (`ZSTD_getFrameContentSize`).
fn frame_content_size(frame: &[u8]) -> Result<u64, EmbedError> {
    match zstd::zstd_safe::get_frame_content_size(frame) {
        Ok(Some(n)) => Ok(n),
        Ok(None) => Err(EmbedError::Frame("the frame does not tell its content size".into())),
        Err(_) => Err(EmbedError::Frame("not a zstd frame".into())),
    }
}

/// `EMBEDDED_FILES::DecompressAndDecode`: the base64 undone, the frame decompressed (its content at most 1 GB), and the checksum checked -- SHA-256 when the file's is 64 digits long,
/// else the MurmurHash3, else the first build's.
pub fn decode(file: &EmbeddedFile) -> Result<Decoded, EmbedError> {
    if file.data.is_empty() {
        return Err(EmbedError::NoData);
    }
    let frame = base64_decode(&file.data)?;
    let size = frame_content_size(&frame)?;
    if size > MAX_CONTENT_BYTES {
        return Err(EmbedError::Frame(format!("{size} bytes is more than KiCad reads")));
    }
    let bytes = zstd::bulk::decompress(&frame, size as usize).map_err(|e| EmbedError::Decompress(e.to_string()))?;
    let current = checksum_of(&bytes);
    let mut legacy = false;
    if file.checksum.len() == 64 {
        if !file.checksum.eq_ignore_ascii_case(&sha256_hex(&bytes)) {
            return Err(EmbedError::Checksum);
        }
        legacy = true;
    } else if file.checksum != current {
        if file.checksum != hash_string(mmh3_v1_x64_128(&bytes, SEED)) {
            return Err(EmbedError::Checksum);
        }
        legacy = true;
    }
    Ok(Decoded { bytes, checksum: current, legacy_checksum: legacy })
}

/// What can be told of a file without decompressing it: the size its content will have and the size it is stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    pub content: Option<u64>,
    pub stored: u64,
}

/// The size of the file's content (from the frame's header: only the start of the data is decoded) and of the frame (from the length of the base64 text).
pub fn sizes(file: &EmbeddedFile) -> Sizes {
    let text = file.data.trim_end_matches('=');
    let stored = (text.len() * 3 / 4) as u64;
    // 32 base64 characters are 24 bytes: a zstd frame header is at most 18.
    let head = file.data.get(..file.data.len().min(32) / 4 * 4).unwrap_or("");
    let content = base64_decode(head).ok().and_then(|b| frame_content_size(&b).ok());
    Sizes { content, stored }
}

/// `GetTemporaryFileName`'s file name for an embedded file: `kicad_embedded_<checksum>.<extension of the name>`; the folder is the caller's. The checksum is the current one of the
/// content (KiCad names the cache file by the `data_hash` it has after it has read the file).
pub fn cache_file_name(name: &str, checksum: &str) -> String {
    let ext = eda_model::embedded_files::extension_of(name);
    if ext.is_empty() {
        format!("kicad_embedded_{checksum}")
    } else {
        format!("kicad_embedded_{checksum}.{ext}")
    }
}

/// The file's content written under `dir` (`EMBEDDED_FILES::GetTemporaryFileName`: created once, found again by the name its checksum gives), and its path. The name is made from the
/// checksum the file has, so that finding the file again costs no decompression; the content is checked when it is written, and written whole or not at all.
pub fn materialize(dir: &std::path::Path, file: &EmbeddedFile) -> Result<std::path::PathBuf, String> {
    let stored = if file.checksum.is_empty() { checksum_of(file.data.as_bytes()) } else { file.checksum.clone() };
    let path = dir.join(cache_file_name(&file.name, &stored));
    if path.is_file() {
        return Ok(path);
    }
    let decoded = decode(file).map_err(|e| format!("{}: {e}", file.name))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let part = dir.join(format!(".{}.{}.part", std::process::id(), path.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    std::fs::write(&part, &decoded.bytes).map_err(|e| format!("{}: {e}", part.display()))?;
    std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

// ------------------------------------------------------------------------------------------------------------------ reading

/// `EMBEDDED_FILES_PARSER::ParseEmbedded`: the files of one `(embedded_files ..)` block, as read, the first of a name kept. A `(file ..)` without a name, with a type the parser does not
/// know or with something else in it is not refused, as it is in KiCad -- which stops reading the block there -- but noted in `problems` and read as far as it goes.
fn read_block(block: &[Sexpr], into: &mut EmbeddedFiles, problems: &mut Vec<String>) {
    for file in block.iter().skip(1).filter_map(Sexpr::as_list) {
        if sexpr::tag(file) != Some("file") {
            problems.push(format!("embedded_files: expected (file ..), found ({})", sexpr::tag(file).unwrap_or("")));
            continue;
        }
        let mut name: Option<String> = None;
        let mut kind = EmbeddedFileType::Other;
        let mut data = String::new();
        let mut checksum = String::new();
        for item in file.iter().skip(1).filter_map(Sexpr::as_list) {
            match sexpr::tag(item) {
                Some("name") => name = sexpr::txt(item, 1).map(str::to_string),
                Some("type") => match sexpr::txt(item, 1).map(EmbeddedFileType::from_word) {
                    Some(Some(t)) => kind = t,
                    _ => problems.push(format!("embedded file {:?}: the type is not datasheet, font, model, worksheet or other", name.as_deref().unwrap_or(""))),
                },
                // The data is `|` + the lines of base64 + `|`: every atom after the tag, the bars taken off (the alphabet has none).
                Some("data") => data = item.iter().skip(1).filter_map(Sexpr::text).flat_map(str::chars).filter(|c| *c != '|' && !c.is_whitespace()).collect(),
                Some("checksum") => checksum = sexpr::txt(item, 1).unwrap_or("").to_string(),
                other => problems.push(format!("embedded file {:?}: expecting checksum, data or name, found {}", name.as_deref().unwrap_or(""), other.unwrap_or("?"))),
            }
        }
        match name {
            Some(name) if !name.is_empty() => {
                into.insert(EmbeddedFile { name, kind, data, checksum });
            }
            _ => problems.push("embedded_files: a file without a name".to_string()),
        }
    }
}

/// The embedded files of the board, schematic, footprint or symbol `owner` (its list `(item ..)`s): `(embedded_fonts yes|no)` and every `(embedded_files ..)` it has.
pub fn read_owner(owner: &[Sexpr], problems: &mut Vec<String>) -> EmbeddedFiles {
    let mut out = EmbeddedFiles::default();
    for item in owner.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("embedded_fonts") => out.fonts = matches!(sexpr::txt(item, 1), Some("yes") | Some("true")),
            Some("embedded_files") => read_block(item, &mut out, problems),
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------------------------ writing

/// `WriteEmbeddedFiles` as the board, schematic and library writers print it, in the pretty-printer's layout: `(embedded_files` at `depth` tabs, each file in it. `files` are the files
/// to write ([`EmbeddedFiles::to_write`]); `with_data` is false for a footprint on a board, which lists the files it uses and leaves their data to the board's list. The text ends with
/// a line break. A file with no data is skipped, whatever `with_data` says: `WriteEmbeddedFiles` skips an empty file.
pub fn write_block(out: &mut String, files: &[&EmbeddedFile], depth: usize, with_data: bool) {
    let t = "\t".repeat(depth);
    let _ = writeln!(out, "{t}(embedded_files");
    for f in files.iter().filter(|f| !f.data.is_empty()) {
        let _ = writeln!(out, "{t}\t(file");
        let _ = writeln!(out, "{t}\t\t(name {})", crate::sexpr_str(&f.name));
        let _ = writeln!(out, "{t}\t\t(type {})", f.kind.word());
        if with_data {
            let chunks: Vec<&str> = f.data.as_bytes().chunks(LINE).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
            if chunks.len() == 1 {
                let _ = writeln!(out, "{t}\t\t(data |{}|)", chunks[0]);
            } else {
                for (i, c) in chunks.iter().enumerate() {
                    let first = i == 0;
                    let last = i + 1 == chunks.len();
                    match (first, last) {
                        (true, _) => {
                            let _ = writeln!(out, "{t}\t\t(data |{c}");
                        }
                        (false, false) => {
                            let _ = writeln!(out, "{t}\t\t\t{c}");
                        }
                        (false, true) => {
                            let _ = writeln!(out, "{t}\t\t\t{c}|");
                        }
                    }
                }
                let _ = writeln!(out, "{t}\t\t)");
            }
        }
        let _ = writeln!(out, "{t}\t\t(checksum {})", crate::sexpr_str(&f.checksum));
        let _ = writeln!(out, "{t}\t)");
    }
    let _ = writeln!(out, "{t})");
}

/// An owner's `(embedded_fonts ..)` and `(embedded_files ..)` as its writer prints them at `depth` tabs, with the data: nothing at all when it has no files and its fonts are not
/// embedded. A list with only the fonts flag on writes the flag. `always_fonts` writes `(embedded_fonts no)` too, as the schematic and the library writers of KiCad do for every owner.
pub fn write_owner(out: &mut String, list: &EmbeddedFiles, depth: usize, always_fonts: bool) {
    let t = "\t".repeat(depth);
    if list.fonts || always_fonts {
        let _ = writeln!(out, "{t}(embedded_fonts {})", if list.fonts { "yes" } else { "no" });
    }
    let files = list.to_write(true);
    if !files.is_empty() {
        write_block(out, &files, depth, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_the_standard_alphabet_with_padding() {
        for (plain, coded) in [("", ""), ("M", "TQ=="), ("Ma", "TWE="), ("Man", "TWFu"), ("Many hands", "TWFueSBoYW5kcw==")] {
            assert_eq!(base64_encode(plain.as_bytes()), coded);
            assert_eq!(base64_decode(coded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)).unwrap(), all);
        for bad in ["TQ=", "T Q==", "TQ=A", "T===", "====", "TWFu\n"] {
            assert_eq!(base64_decode(bad), Err(EmbedError::Base64), "{bad:?}");
        }
    }

    #[test]
    fn the_hashes_are_the_published_ones() {
        // SHA-256 test vectors (FIPS 180-2).
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(&[b'a'; 1000]), sha256_hex(&vec![b'a'; 1000]));
        // MurmurHash3_x64_128: the empty input with seed 0 is all zeros; "hello" with seed 0 is the value smhasher prints.
        assert_eq!(mmh3_x64_128(b"", 0), (0, 0));
        assert_eq!(hash_string(mmh3_x64_128(b"hello", 0)), "CBD8A7B341BD9B025B1E906A48AE1D19");
        assert_eq!(hash_string(mmh3_x64_128(b"The quick brown fox jumps over the lazy dog", 0)), "E34BBC7BBC071B6C7A433CA9C49A9347");
    }

    #[test]
    fn the_first_builds_hash_differs_only_when_the_last_block_is_short() {
        let sixteen = [7u8; 16];
        assert_eq!(mmh3_v1_x64_128(&sixteen, SEED), mmh3_x64_128(&sixteen, SEED), "no tail, no difference");
        let seventeen = [7u8; 17];
        assert_ne!(mmh3_v1_x64_128(&seventeen, SEED), mmh3_x64_128(&seventeen, SEED));
        // a tail of 12 or more bytes is not hashed, only counted
        let a: Vec<u8> = (0..28).collect();
        let b: Vec<u8> = (0..16).chain(100..112).collect();
        assert_eq!(mmh3_v1_x64_128(&a, SEED), mmh3_v1_x64_128(&b, SEED));
    }

    #[test]
    fn a_new_file_is_a_level_15_frame_that_tells_its_size_and_decodes_back() {
        let content: Vec<u8> = (0..5000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let f = encode_new("some/dir/Model One.STEP", &content).unwrap();
        assert_eq!(f.name, "Model One.STEP");
        assert_eq!(f.kind, EmbeddedFileType::Model);
        assert!(f.data.starts_with("KLUv/"), "a zstd frame: {}", &f.data[..8]);
        assert_eq!(f.checksum.len(), 32);
        assert!(f.checksum.chars().all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c)));
        let sz = sizes(&f);
        assert_eq!(sz.content, Some(content.len() as u64));
        assert!(sz.stored < content.len() as u64);
        let d = decode(&f).unwrap();
        assert_eq!(d.bytes, content);
        assert_eq!(d.checksum, f.checksum);
        assert!(!d.legacy_checksum);
        // an empty file is a frame too
        let empty = encode_new("e.txt", b"").unwrap();
        assert_eq!(decode(&empty).unwrap().bytes, Vec::<u8>::new());
        assert_eq!(empty.checksum, checksum_of(b""));
        assert_eq!(empty.checksum.len(), 32);
    }

    #[test]
    fn decode_takes_the_three_checksums_and_refuses_the_rest() {
        let content = b"ISO-10303-21; a model that is not a multiple of sixteen bytes".to_vec();
        let f = encode_new("m.step", &content).unwrap();
        let sha = EmbeddedFile { checksum: sha256_hex(&content), ..f.clone() };
        let d = decode(&sha).unwrap();
        assert!(d.legacy_checksum && d.checksum == f.checksum);
        let v1 = EmbeddedFile { checksum: hash_string(mmh3_v1_x64_128(&content, SEED)), ..f.clone() };
        assert!(decode(&v1).unwrap().legacy_checksum);
        let wrong = EmbeddedFile { checksum: "0".repeat(32), ..f.clone() };
        assert_eq!(decode(&wrong), Err(EmbedError::Checksum));
        let wrong_sha = EmbeddedFile { checksum: "0".repeat(64), ..f.clone() };
        assert_eq!(decode(&wrong_sha), Err(EmbedError::Checksum));
        assert_eq!(decode(&EmbeddedFile { data: String::new(), ..f.clone() }), Err(EmbedError::NoData));
        assert_eq!(decode(&EmbeddedFile { data: "!!!!".into(), ..f.clone() }), Err(EmbedError::Base64));
        assert!(matches!(decode(&EmbeddedFile { data: base64_encode(b"plain text, no frame"), ..f.clone() }), Err(EmbedError::Frame(_))));
        // a frame that does not tell its content size is refused, as `ZSTD_getFrameContentSize` does
        let mut enc = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
        std::io::Write::write_all(&mut enc, &content).unwrap();
        let streamed = enc.finish().unwrap();
        assert!(matches!(decode(&EmbeddedFile { data: base64_encode(&streamed), ..f }), Err(EmbedError::Frame(_))));
    }

    #[test]
    fn the_block_reads_back_as_written_and_is_laid_out_as_kicad_prints_it() {
        let big: Vec<u8> = (0..4000u32).map(|i| (i * 7 % 256) as u8).collect();
        let mut list = EmbeddedFiles::default();
        list.insert(encode_new("big.step", &big).unwrap());
        list.insert(encode_new("small.txt", b"hi").unwrap());
        list.insert(encode_new("a.kicad_wks", &vec![b'x'; 500]).unwrap());
        let mut text = String::from("(kicad_pcb\n");
        write_owner(&mut text, &list, 1, true);
        text.push_str(")\n");
        // layout: the names in order, the data's first line after `(data |`, the rest at four tabs, `|` ends the last line, `)` alone after it
        assert!(text.contains("\t(embedded_fonts no)\n\t(embedded_files\n\t\t(file\n\t\t\t(name \"a.kicad_wks\")\n\t\t\t(type worksheet)\n\t\t\t(data |"), "{text}");
        assert!(text.find("big.step").unwrap() < text.find("small.txt").unwrap());
        let big_at = text.find("(name \"big.step\")").unwrap();
        let big_text = &text[big_at..];
        let lines: Vec<&str> = big_text.lines().collect();
        assert!(lines[2].starts_with("\t\t\t(data |") && lines[2].len() == "\t\t\t(data |".len() + LINE, "{}", lines[2]);
        assert!(lines[3].starts_with("\t\t\t\t") && lines[3].trim().len() == LINE);
        let end = lines.iter().position(|l| l.trim_end().ends_with('|') && !l.contains("(data")).unwrap();
        assert_eq!(lines[end + 1], "\t\t\t)");
        assert!(lines[end + 2].starts_with("\t\t\t(checksum \""));
        assert!(text.contains("(data |") && text.contains("\t\t\t(data |") && text.contains("|)"), "a one-line data stays on its line");
        let tree = sexpr::parse(&text).unwrap();
        let mut problems = Vec::new();
        let back = read_owner(tree.as_list().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.files, list.files);
        assert!(!back.fonts);
        for f in &back.files {
            assert!(decode(f).is_ok(), "{}", f.name);
        }
        assert_eq!(decode(back.get("big.step").unwrap()).unwrap().bytes, big);
    }

    #[test]
    fn references_are_written_without_data_and_a_data_less_file_not_at_all() {
        let mut list = EmbeddedFiles::default();
        list.insert(encode_new("m.step", b"model").unwrap());
        list.insert(EmbeddedFile { name: "ghost.pdf".into(), kind: EmbeddedFileType::Datasheet, data: String::new(), checksum: "AB".into() });
        let files = list.to_write(false);
        let mut text = String::new();
        write_block(&mut text, &files, 2, false);
        assert!(text.contains("(name \"m.step\")") && text.contains("(checksum \"") && !text.contains("(data"), "{text}");
        assert!(!text.contains("ghost.pdf"), "WriteEmbeddedFiles skips a file that has no data");
        let mut none = String::new();
        write_owner(&mut none, &EmbeddedFiles::default(), 1, false);
        assert_eq!(none, "", "nothing to say");
    }

    #[test]
    fn the_reader_keeps_going_past_what_kicad_would_stop_at() {
        let text = r#"(kicad_pcb (embedded_fonts yes) (embedded_files
            (file (name "a.step") (type model) (data |AAAA
              BBBB| ) (checksum "00"))
            (file (type model) (data |CC==|))
            (file (name "b.bin") (type picture) (surprise 1))
            (file (name "a.step") (type other))
          ))"#;
        let tree = sexpr::parse(text).unwrap();
        let mut problems = Vec::new();
        let list = read_owner(tree.as_list().unwrap(), &mut problems);
        assert!(list.fonts);
        assert_eq!(list.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a.step", "b.bin"]);
        assert_eq!(list.get("a.step").unwrap().data, "AAAABBBB");
        assert_eq!(list.get("a.step").unwrap().kind, EmbeddedFileType::Model, "the first of a name wins");
        assert_eq!(list.get("b.bin").unwrap().kind, EmbeddedFileType::Other);
        assert_eq!(problems.len(), 3, "{problems:?}");
    }
}
