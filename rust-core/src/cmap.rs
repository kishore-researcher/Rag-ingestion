//! Minimal `/ToUnicode` CMap parser.
//!
//! Subset and CID fonts encode text as glyph indices, not Unicode, so the
//! raw string bytes in a content stream are meaningless on their own
//! (`,0 , + , . ?? F 5 *`). The font's `/ToUnicode` stream maps each source
//! code to its Unicode text. Only the two constructs that carry mappings
//! are implemented: `beginbfchar` and `beginbfrange` (sequential and array
//! forms). `codespacerange`, `usecmap`, and `cidrange` are ignored; see the
//! README for the consequences.

use std::collections::HashMap;

/// A hostile CMap could declare `<0000> <FFFFFFFF> <0000>`; cap one range.
const MAX_RANGE_ENTRIES: u32 = 0x10000;
/// Cap on total entries retained per font.
const MAX_TOTAL_ENTRIES: usize = 262_144;

#[derive(Debug, Default)]
pub struct ToUnicodeMap {
    map: HashMap<u32, String>,
}

impl ToUnicodeMap {
    pub fn lookup(&self, code: u32) -> Option<&str> {
        self.map.get(&code).map(|s| s.as_str())
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
    fn insert(&mut self, code: u32, text: String) {
        if self.map.len() < MAX_TOTAL_ENTRIES {
            self.map.insert(code, text);
        }
    }
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = hex.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut digits = digits;
    if digits.len() % 2 == 1 {
        digits.push(b'0');
    }
    let val = |b: u8| (b as char).to_digit(16).unwrap() as u8;
    Some(digits.chunks(2).map(|p| (val(p[0]) << 4) | val(p[1])).collect())
}

fn bytes_to_code(bytes: &[u8]) -> u32 {
    bytes.iter().take(4).fold(0u32, |acc, b| (acc << 8) | *b as u32)
}

fn utf16be_units(bytes: &[u8]) -> Vec<u16> {
    bytes.chunks(2).map(|c| if c.len() == 2 { ((c[0] as u16) << 8) | c[1] as u16 } else { c[0] as u16 }).collect()
}

fn utf16be_to_string(bytes: &[u8]) -> String {
    String::from_utf16_lossy(&utf16be_units(bytes))
}

/// Destination for entry `offset` of a sequential range: the last UTF-16
/// unit is incremented by `offset`.
fn utf16be_to_string_offset(bytes: &[u8], offset: u32) -> String {
    let mut units = utf16be_units(bytes);
    if let Some(last) = units.last_mut() {
        *last = last.wrapping_add(offset as u16);
    }
    String::from_utf16_lossy(&units)
}

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Hex(&'a str),
    ArrayOpen,
    ArrayClose,
    Word(&'a str),
}

fn tokenize(data: &str) -> Vec<Tok<'_>> {
    let b = data.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_whitespace() => i += 1,
            b'%' => {
                while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                    i += 1;
                }
            }
            b'<' if b.get(i + 1) == Some(&b'<') => i += 2, // dict open: ignore
            b'>' if b.get(i + 1) == Some(&b'>') => i += 2,
            b'<' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && b[j] != b'>' {
                    j += 1;
                }
                out.push(Tok::Hex(&data[start..j.min(b.len())]));
                i = j + 1;
            }
            b'[' => {
                out.push(Tok::ArrayOpen);
                i += 1;
            }
            b']' => {
                out.push(Tok::ArrayClose);
                i += 1;
            }
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'<' | b'>' | b'[' | b']' | b'%') {
                    i += 1;
                }
                if i == start {
                    i += 1;
                } else {
                    out.push(Tok::Word(&data[start..i]));
                }
            }
        }
    }
    out
}

/// Parses a decoded `/ToUnicode` stream. Returns `None` when no mapping
/// could be extracted, so callers fall back to their existing behaviour.
pub fn parse_tounicode_cmap(data: &[u8]) -> Option<ToUnicodeMap> {
    let text = String::from_utf8_lossy(data);
    let toks = tokenize(&text);
    let mut map = ToUnicodeMap::default();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Word("beginbfchar") => {
                i += 1;
                while i + 1 < toks.len() {
                    if let (Tok::Hex(src), Tok::Hex(dst)) = (&toks[i], &toks[i + 1]) {
                        if let (Some(s), Some(d)) = (hex_to_bytes(src), hex_to_bytes(dst)) {
                            map.insert(bytes_to_code(&s), utf16be_to_string(&d));
                        }
                        i += 2;
                    } else {
                        break;
                    }
                }
            }
            Tok::Word("beginbfrange") => {
                i += 1;
                while i + 2 < toks.len() {
                    let (Tok::Hex(lo), Tok::Hex(hi)) = (&toks[i], &toks[i + 1]) else { break };
                    let (lo_b, hi_b) = (hex_to_bytes(lo), hex_to_bytes(hi));
                    match &toks[i + 2] {
                        Tok::Hex(dst) => {
                            if let (Some(l), Some(h), Some(d)) = (lo_b, hi_b, hex_to_bytes(dst)) {
                                let (lo_c, hi_c) = (bytes_to_code(&l), bytes_to_code(&h));
                                let count = hi_c.saturating_sub(lo_c).min(MAX_RANGE_ENTRIES - 1);
                                for off in 0..=count {
                                    map.insert(lo_c + off, utf16be_to_string_offset(&d, off));
                                }
                            }
                            i += 3;
                        }
                        Tok::ArrayOpen => {
                            let mut j = i + 3;
                            let mut off = 0u32;
                            let lo_c = lo_b.as_deref().map(bytes_to_code);
                            while j < toks.len() && toks[j] != Tok::ArrayClose {
                                if let (Tok::Hex(d), Some(l)) = (&toks[j], lo_c) {
                                    if let Some(db) = hex_to_bytes(d) {
                                        if off < MAX_RANGE_ENTRIES {
                                            map.insert(l + off, utf16be_to_string(&db));
                                        }
                                    }
                                    off += 1;
                                }
                                j += 1;
                            }
                            i = j + 1;
                        }
                        _ => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    if map.map.is_empty() { None } else { Some(map) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> ToUnicodeMap {
        parse_tounicode_cmap(s.as_bytes()).expect("map")
    }

    #[test]
    fn bfchar_maps_codes() {
        let m = p("1 beginbfchar\n<0003> <0020>\n<0024> <0041>\nendbfchar");
        assert_eq!(m.lookup(3), Some(" "));
        assert_eq!(m.lookup(0x24), Some("A"));
        assert_eq!(m.lookup(5), None);
    }

    #[test]
    fn bfrange_sequential() {
        let m = p("1 beginbfrange\n<0010> <0012> <0030>\nendbfrange");
        assert_eq!(m.lookup(0x10), Some("0"));
        assert_eq!(m.lookup(0x11), Some("1"));
        assert_eq!(m.lookup(0x12), Some("2"));
        assert_eq!(m.lookup(0x13), None);
    }

    #[test]
    fn bfrange_array_form() {
        let m = p("1 beginbfrange\n<0001> <0003> [<0041> <0042> <0043>]\nendbfrange");
        assert_eq!(m.lookup(1), Some("A"));
        assert_eq!(m.lookup(2), Some("B"));
        assert_eq!(m.lookup(3), Some("C"));
    }

    #[test]
    fn ligature_maps_to_multiple_chars() {
        let m = p("1 beginbfchar\n<0005> <00660069>\nendbfchar");
        assert_eq!(m.lookup(5), Some("fi"));
    }

    #[test]
    fn surrogate_pair_destination() {
        let m = p("1 beginbfchar\n<0007> <D83DDE00>\nendbfchar");
        assert_eq!(m.lookup(7), Some("\u{1F600}"));
    }

    #[test]
    fn single_byte_source_codes() {
        let m = p("1 beginbfchar\n<41> <0058>\nendbfchar");
        assert_eq!(m.lookup(0x41), Some("X"));
    }

    #[test]
    fn real_world_preamble_comments_and_codespace() {
        let src = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
            /CMapName /Adobe-Identity-UCS def\n% a comment <0001> <0002>\n\
            1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
            2 beginbfchar\n<0003> <0020>\n<0024> <0041>\nendbfchar\n\
            1 beginbfrange\n<0025> <0027> <0042>\nendbfrange\nendcmap";
        let m = p(src);
        assert_eq!(m.len(), 5);
        assert_eq!(m.lookup(0x27), Some("D"));
    }

    #[test]
    fn spaced_and_lowercase_hex() {
        let m = p("1 beginbfchar\n<00 0a> <00 6a>\nendbfchar");
        assert_eq!(m.lookup(0x0a), Some("j"));
    }

    #[test]
    fn empty_or_garbage_is_none() {
        assert!(parse_tounicode_cmap(b"").is_none());
        assert!(parse_tounicode_cmap(b"not a cmap at all \x00\xff").is_none());
    }

    #[test]
    fn malformed_entries_do_not_poison_good_ones() {
        let m = p("3 beginbfchar\n<zz> <0041>\n<0001> <0042>\n<0002> <0043>\nendbfchar");
        assert_eq!(m.lookup(1), Some("B"));
        assert_eq!(m.lookup(2), Some("C"));
    }

    #[test]
    fn truncated_stream_does_not_panic() {
        let _ = parse_tounicode_cmap(b"1 beginbfrange\n<0001> <0003> [<0041> <00");
        let _ = parse_tounicode_cmap(b"1 beginbfchar\n<0001>");
        let _ = parse_tounicode_cmap(b"1 beginbfrange\n<0001> <0003>");
    }

    #[test]
    fn oversized_range_is_capped() {
        let m = p("1 beginbfrange\n<00000000> <FFFFFFFF> <0041>\nendbfrange");
        assert!(m.len() <= MAX_RANGE_ENTRIES as usize);
    }
}
