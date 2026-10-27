//! What the board's reference images (`PCB_REFERENCE_IMAGE`) and the schematic's images (`SCH_BITMAP`) need to know of the file they embed, without a decoder: its kind, its size in
//! pixels and its definition in pixels per inch, read from the header the way wxWidgets' image handlers do and `BITMAP_BASE::updatePPI` (common/bitmap_base.cpp) takes it.
//! No pixel is decoded here (no new dependency may decode one): the studio's browser decodes and draws the image, and a file the header of which is not understood is refused.
//!
//! The image is held as the base64 text a `(data ..)` token carries, so a file written back has the bytes it was read with (`BITMAP_BASE::SaveImageData` writes `m_imageData`
//! as it was). Also here: the base64 codec that needs, a content hash for the URL the browser fetches the pixels at, and a plain PNG writer for tests and placeholders.

use crate::ir::Nm;

/// The definition of an image whose file says none (`BITMAP_BASE`'s `m_ppi`).
pub const DEFAULT_PPI: u32 = 300;
/// `(scale s)` is held in millionths: 1.0 is this.
pub const SCALE_PPM: i64 = 1_000_000;

/// The kinds of file KiCad's image handlers are used for here: PNG and JPEG (what the schematic accepts a drop of) and the GIF and BMP that wxWidgets reads too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
}

impl ImageFormat {
    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Bmp => "image/bmp",
        }
    }
}

/// The size and definition an image file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageHeader {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    /// Pixels per inch (`BITMAP_BASE::GetPPI`): the file's, else [`DEFAULT_PPI`].
    pub ppi: u32,
    /// The definition KiCad took before file version 20260623 (`BITMAP_BASE::GetLegacyPPI`: a PNG's pixels per centimetre read as an integer, `GetOptionInt` truncating 37.8 to
    /// 37, then `KiROUND( dpiX * 2.54 )`); the same as `ppi` for every other image. `parsePCB_REFERENCE_IMAGE` scales an older file's image by `ppi / legacy_ppi` to keep the size
    /// it was drawn at.
    pub legacy_ppi: u32,
}

/// What reading a header gave: the header, a file that ends before it says, or a file that is not an image of a kind known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed {
    Header(ImageHeader),
    NeedMore,
    Bad,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as u32)
}

/// The kind of file the first bytes say (the magic numbers wxWidgets' handlers sniff).
pub fn sniff(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(ImageFormat::Png)
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if b.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if b.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

/// The header of the image file `b` (which may be only its first part).
///
/// A PNG's `pHYs` chunk is pixels per metre: wxWidgets makes it pixels per centimetre (`/ 100`) and `updatePPI` `KiROUND( dpiX * 2.54 )`. A JPEG's JFIF density is dots per
/// inch (unit 1) or per centimetre (unit 2, `KiROUND( dpiX * 2.54 )`). A GIF, a BMP, or a file that says none has [`DEFAULT_PPI`]; a resolution of one or less is no resolution.
pub fn parse(b: &[u8]) -> Parsed {
    let Some(format) = sniff(b) else { return if b.len() < 8 { Parsed::NeedMore } else { Parsed::Bad } };
    let done_legacy = |width: u32, height: u32, ppi: u32, legacy_ppi: u32| if width > 0 && height > 0 { Parsed::Header(ImageHeader { format, width, height, ppi, legacy_ppi }) } else { Parsed::Bad };
    let done = |width: u32, height: u32, ppi: u32| done_legacy(width, height, ppi, ppi);
    match format {
        ImageFormat::Png => {
            // signature, then chunks: length, type, data, crc; IHDR first
            if b.len() < 33 {
                return Parsed::NeedMore;
            }
            if &b[12..16] != b"IHDR" {
                return Parsed::Bad;
            }
            let (Some(w), Some(h)) = (be32(b, 16), be32(b, 20)) else { return Parsed::NeedMore };
            let mut ppi = DEFAULT_PPI;
            let mut legacy_ppi = DEFAULT_PPI;
            let mut at = 8usize;
            loop {
                let (Some(len), Some(kind)) = (be32(b, at), b.get(at + 4..at + 8)) else { return Parsed::NeedMore };
                if kind == b"pHYs" {
                    let (Some(per_metre), Some(unit)) = (be32(b, at + 8), b.get(at + 16)) else { return Parsed::NeedMore };
                    if *unit == 1 && per_metre as f64 / 100.0 > 1.0 {
                        ppi = (per_metre as f64 / 100.0 * 2.54).round() as u32;
                    }
                    // (`GetLegacyPPI`: `if( dpiX > 1 )` on the truncated integer, else `m_ppi`)
                    legacy_ppi = if *unit == 1 && per_metre / 100 > 1 { ((per_metre / 100) as f64 * 2.54).round() as u32 } else { ppi };
                    break;
                }
                if kind == b"IDAT" || kind == b"IEND" {
                    break;
                }
                at = at.saturating_add(12).saturating_add(len as usize);
                if at > b.len() {
                    return Parsed::NeedMore;
                }
            }
            done_legacy(w, h, ppi, legacy_ppi)
        }
        ImageFormat::Gif => match (le16(b, 6), le16(b, 8)) {
            (Some(w), Some(h)) => done(w, h, DEFAULT_PPI),
            _ => Parsed::NeedMore,
        },
        ImageFormat::Bmp => match (le32(b, 18), le32(b, 22)) {
            (Some(w), Some(h)) => done(w, (h as i32).unsigned_abs(), DEFAULT_PPI),
            _ => Parsed::NeedMore,
        },
        ImageFormat::Jpeg => {
            let mut ppi = DEFAULT_PPI;
            let mut at = 2usize;
            loop {
                let Some(&first) = b.get(at) else { return Parsed::NeedMore };
                if first != 0xff {
                    at += 1;
                    continue;
                }
                let Some(&marker) = b.get(at + 1) else { return Parsed::NeedMore };
                if marker == 0xff {
                    at += 1;
                    continue;
                }
                if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                    at += 2;
                    continue;
                }
                let Some(len) = be16(b, at + 2) else { return Parsed::NeedMore };
                if marker == 0xe0 && len >= 16 {
                    let Some(jfif) = b.get(at + 4..at + 9) else { return Parsed::NeedMore };
                    if jfif == b"JFIF\0" {
                        let (Some(&unit), Some(density)) = (b.get(at + 11), be16(b, at + 12)) else { return Parsed::NeedMore };
                        if density > 1 && unit == 1 {
                            ppi = density;
                        } else if density > 1 && unit == 2 {
                            ppi = (density as f64 * 2.54).round() as u32;
                        }
                    }
                }
                // a start of frame (not the table definitions in the same range)
                if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
                    let (Some(h), Some(w)) = (be16(b, at + 5), be16(b, at + 7)) else { return Parsed::NeedMore };
                    return done(w, h, ppi);
                }
                at += 2 + len as usize;
            }
        }
    }
}

/// The header of the image file `b`, or `None` when it is not one of a kind known.
pub fn header_of_bytes(b: &[u8]) -> Option<ImageHeader> {
    match parse(b) {
        Parsed::Header(h) => Some(h),
        _ => None,
    }
}

/// The header of the image whose base64 text is `data`. Only the start of the file is decoded, as much of it as the header needs.
pub fn header_of_base64(data: &str) -> Option<ImageHeader> {
    // A header is in the first few hundred bytes of a PNG and of most JPEGs; a JPEG with a big EXIF or colour profile segment has it further in.
    for prefix in [2_048usize, 32_768, 524_288] {
        if data.len() <= prefix {
            break;
        }
        match parse(&base64_decode_prefix(data, prefix)?) {
            Parsed::Header(h) => return Some(h),
            Parsed::Bad => return None,
            Parsed::NeedMore => {}
        }
    }
    header_of_bytes(&base64_decode(data)?)
}

/// `BITMAP_BASE::GetSize`: the pixels at their definition (an inch over `ppi` each) times the scale, in nanometres.
pub fn size_nm(h: &ImageHeader, scale_ppm: i64) -> (Nm, Nm) {
    let k = crate::units::NM_PER_INCH as f64 / h.ppi.max(1) as f64 * scale_ppm as f64 / SCALE_PPM as f64;
    (Nm::kiround(h.width as f64 * k), Nm::kiround(h.height as f64 * k))
}

// ------------------------------------------------------------------------------------------------------------------------------------ base64

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The standard base64 of `bytes`, padded, with no line breaks.
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

fn sextet(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32 + 26),
        b'0'..=b'9' => Some((c - b'0') as u32 + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// The bytes of the base64 `text`; white space between the characters is skipped (the file breaks the text into lines), a padding `=` ends it. `None` for any other character.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    base64_decode_prefix(text, usize::MAX)
}

/// The bytes of the first `limit` characters of the base64 `text` (rounded down to whole groups of four).
fn base64_decode_prefix(text: &str, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len().min(limit) / 4 * 3);
    let mut acc = 0u32;
    let mut n = 0u32;
    for (seen, c) in text.bytes().filter(|c| !c.is_ascii_whitespace()).enumerate() {
        if seen >= limit {
            break;
        }
        if c == b'=' {
            break;
        }
        acc = acc << 6 | sextet(c)?;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    match n {
        0 => {}
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => return None,
    }
    Some(out)
}

/// A short content hash of the image text (FNV-1a), the part of the URL that changes when the pixels do.
pub fn content_hash(data: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data.bytes().filter(|b| !b.is_ascii_whitespace()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The image text with its white space taken out: the form the IR holds (`(data ..)` of a file is broken into lines of 76 characters).
pub fn compact(data: &str) -> String {
    data.chars().filter(|c| !c.is_whitespace()).collect()
}

// ------------------------------------------------------------------------------------------------------------------------------------ a plain PNG

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    b << 16 | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let from = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[from..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A PNG of `width` x `height` pixels in one colour `rgb`, at `ppi` pixels per inch, its pixels stored uncompressed (a deflate stream of stored blocks). For the tests and for the
/// placeholder of an image whose data is gone: it is a valid file any reader takes, and the only one this crate can make without a codec.
pub fn solid_png(width: u32, height: u32, rgb: [u8; 3], ppi: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity((1 + width as usize * 3) * height as usize);
    for _ in 0..height {
        raw.push(0); // filter: none
        for _ in 0..width {
            raw.extend_from_slice(&rgb);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        z.push(if blocks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bits, RGB, deflate, no filter method, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    let per_metre = (ppi as f64 / 0.0254).round() as u32;
    let mut phys = Vec::new();
    phys.extend_from_slice(&per_metre.to_be_bytes());
    phys.extend_from_slice(&per_metre.to_be_bytes());
    phys.push(1);
    chunk(&mut out, b"pHYs", &phys);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_length_and_skips_the_line_breaks_of_a_file() {
        for n in 0..40usize {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(base64_decode(&text).unwrap(), bytes, "length {n}");
            let broken: String = text.as_bytes().chunks(7).map(|c| format!("\n\t\"{}\"", std::str::from_utf8(c).unwrap())).collect::<String>().replace('"', "");
            assert_eq!(base64_decode(&broken).unwrap(), bytes, "length {n} broken into lines");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
        assert!(base64_decode("TW*u").is_none());
    }

    #[test]
    fn a_png_gives_its_size_and_its_definition_and_the_default_when_it_says_none() {
        let png = solid_png(60, 40, [10, 200, 30], 254);
        let h = header_of_bytes(&png).unwrap();
        assert_eq!((h.format, h.width, h.height, h.ppi), (ImageFormat::Png, 60, 40, 254));
        // 60 px at 254 ppi are 6 mm; at scale 2 they are 12.
        assert_eq!(size_nm(&h, SCALE_PPM), (Nm::new(6_000_000), Nm::new(4_000_000)));
        assert_eq!(size_nm(&h, 2 * SCALE_PPM), (Nm::new(12_000_000), Nm::new(8_000_000)));
        // the same file without its pHYs chunk is a 300 ppi image
        let mut no_phys = png[..33].to_vec();
        let mut at = 33;
        while at + 12 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            if &png[at + 4..at + 8] != b"pHYs" {
                no_phys.extend_from_slice(&png[at..at + 12 + len]);
            }
            at += 12 + len;
        }
        assert_eq!(header_of_bytes(&no_phys).unwrap().ppi, DEFAULT_PPI);
        // 96 dpi is 3780 pixels per metre, 37.8 per centimetre: KiCad's 96, and before 20260623 37 x 2.54, 94
        let h96 = header_of_bytes(&solid_png(3, 3, [0, 0, 0], 96)).unwrap();
        assert_eq!((h96.ppi, h96.legacy_ppi), (96, 94));
        // 300 ppi is 11811 pixels per metre
        assert_eq!(header_of_bytes(&solid_png(3, 3, [0, 0, 0], 300)).unwrap().ppi, 300);
    }

    #[test]
    fn the_header_is_read_from_the_base64_of_a_whole_file_and_from_a_truncated_one() {
        let text = base64_encode(&solid_png(5, 7, [1, 2, 3], 96));
        assert_eq!(header_of_base64(&text).map(|h| (h.width, h.height, h.ppi)), Some((5, 7, 96)));
        assert!(header_of_base64("").is_none() && header_of_base64("AAAAAAAAAAAAAAAAAAAA").is_none());
        assert_eq!(parse(&solid_png(5, 7, [1, 2, 3], 96)[..20]), Parsed::NeedMore);
    }

    #[test]
    fn a_jpeg_gives_its_size_and_its_jfif_density() {
        // SOI, APP0 (JFIF, 1.1, dots per inch 72 x 72), SOF0 of 30 rows and 40 columns
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 1, 0, 72, 0, 72, 0, 0];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0, 17, 8, 0, 30, 0, 40, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        let h = header_of_bytes(&jpeg).unwrap();
        assert_eq!((h.format, h.width, h.height, h.ppi), (ImageFormat::Jpeg, 40, 30, 72));
        // dots per centimetre: 118 / cm is 300 ppi
        jpeg[13] = 2;
        jpeg[14] = 0;
        jpeg[15] = 118;
        assert_eq!(header_of_bytes(&jpeg).unwrap().ppi, 300);
        // a JFIF that says "aspect ratio only" (unit 0) is no resolution
        jpeg[13] = 0;
        assert_eq!(header_of_bytes(&jpeg).unwrap().ppi, DEFAULT_PPI);
    }

    #[test]
    fn a_gif_a_bmp_and_something_else() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[12, 0, 9, 0, 0, 0, 0]);
        assert_eq!(header_of_bytes(&gif).map(|h| (h.format, h.width, h.height, h.ppi)), Some((ImageFormat::Gif, 12, 9, DEFAULT_PPI)));
        let mut bmp = vec![0u8; 30];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&20u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-10i32).to_le_bytes());
        assert_eq!(header_of_bytes(&bmp).map(|h| (h.format, h.width, h.height)), Some((ImageFormat::Bmp, 20, 10)));
        assert!(header_of_bytes(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_none());
        assert_eq!(parse(b"<svg"), Parsed::NeedMore);
    }

    #[test]
    fn the_hash_follows_the_text_and_not_its_line_breaks() {
        let a = base64_encode(&solid_png(2, 2, [9, 9, 9], 300));
        let b = base64_encode(&solid_png(2, 2, [9, 9, 8], 300));
        assert_ne!(content_hash(&a), content_hash(&b));
        let broken = format!("{}\n{}", &a[..10], &a[10..]);
        assert_eq!(content_hash(&a), content_hash(&broken));
        assert_eq!(compact(&broken), a);
    }
}
