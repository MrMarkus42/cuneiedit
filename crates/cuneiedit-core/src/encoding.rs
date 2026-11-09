use std::fmt;
use std::hash::{Hash, Hasher};

use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

const BOM_UTF8: &[u8] = &[0xEF, 0xBB, 0xBF];
const BOM_UTF16_LE: &[u8] = &[0xFF, 0xFE];
const BOM_UTF16_BE: &[u8] = &[0xFE, 0xFF];

/// Character encoding of a file on disk.
#[derive(Clone, Copy, Debug, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// UTF-16 without a byte order mark.
    Utf16LeNoBom,
    Utf16BeNoBom,
    Windows1252,
    /// Any other single- or multi-byte encoding supported by `encoding_rs`.
    Legacy(&'static encoding_rs::Encoding),
    /// A DOS code page that `encoding_rs` does not have.
    Oem(&'static OemCodePage),
}

/// A single-byte DOS code page: ASCII plus 128 characters for the bytes 0x80–0xFF.
#[derive(Debug)]
pub struct OemCodePage {
    name: &'static str,
    high: &'static str,
}

impl OemCodePage {
    fn table(&self) -> Vec<char> {
        self.high.chars().collect()
    }

    fn byte(table: &[char], ch: char) -> Option<u8> {
        if ch.is_ascii() {
            return Some(ch as u8);
        }
        let index = table.iter().position(|&c| c == ch)?;
        Some(0x80 + index as u8)
    }
}

pub static CP437: OemCodePage = OemCodePage {
    name: "IBM437",
    high: "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐\
           └┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}",
};

pub static CP850: OemCodePage = OemCodePage {
    name: "IBM850",
    high: "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐\
           └┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}",
};

/// Code page 850 with the euro sign instead of the dotless i.
pub static CP858: OemCodePage = OemCodePage {
    name: "IBM858",
    high: "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐\
           └┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈ€ÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}",
};

impl PartialEq for Encoding {
    fn eq(&self, other: &Self) -> bool {
        self.label() == other.label()
    }
}

impl Eq for Encoding {}

impl Hash for Encoding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.label().hash(state);
    }
}

/// Groups of legacy encodings, as shown in the encoding menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    WesternEuropean,
    CentralEuropean,
    Baltic,
    Cyrillic,
    Greek,
    Turkish,
    Hebrew,
    Arabic,
    NorthernEuropean,
    Vietnamese,
    Thai,
    Chinese,
    Japanese,
    Korean,
    Dos,
}

const GROUPS: &[(Script, &[Encoding])] = {
    use crate::Encoding::{Legacy, Oem};
    use encoding_rs::*;
    &[
        (
            Script::WesternEuropean,
            &[Legacy(ISO_8859_15), Legacy(ISO_8859_14), Legacy(MACINTOSH)],
        ),
        (
            Script::CentralEuropean,
            &[
                Legacy(WINDOWS_1250),
                Legacy(ISO_8859_2),
                Legacy(ISO_8859_16),
            ],
        ),
        (
            Script::Baltic,
            &[
                Legacy(WINDOWS_1257),
                Legacy(ISO_8859_4),
                Legacy(ISO_8859_13),
            ],
        ),
        (
            Script::Cyrillic,
            &[
                Legacy(WINDOWS_1251),
                Legacy(ISO_8859_5),
                Legacy(KOI8_R),
                Legacy(KOI8_U),
                Legacy(X_MAC_CYRILLIC),
            ],
        ),
        (Script::Greek, &[Legacy(WINDOWS_1253), Legacy(ISO_8859_7)]),
        (Script::Turkish, &[Legacy(WINDOWS_1254), Legacy(ISO_8859_3)]),
        (Script::Hebrew, &[Legacy(WINDOWS_1255), Legacy(ISO_8859_8)]),
        (Script::Arabic, &[Legacy(WINDOWS_1256), Legacy(ISO_8859_6)]),
        (Script::NorthernEuropean, &[Legacy(ISO_8859_10)]),
        (Script::Vietnamese, &[Legacy(WINDOWS_1258)]),
        (Script::Thai, &[Legacy(WINDOWS_874)]),
        (
            Script::Chinese,
            &[Legacy(GBK), Legacy(GB18030), Legacy(BIG5)],
        ),
        (
            Script::Japanese,
            &[Legacy(SHIFT_JIS), Legacy(EUC_JP), Legacy(ISO_2022_JP)],
        ),
        (Script::Korean, &[Legacy(EUC_KR)]),
        (
            Script::Dos,
            &[Oem(&CP437), Oem(&CP850), Oem(&CP858), Legacy(IBM866)],
        ),
    ]
};

impl Encoding {
    pub const ALL: [Self; 7] = [
        Self::Utf8,
        Self::Utf8Bom,
        Self::Utf16Le,
        Self::Utf16Be,
        Self::Utf16LeNoBom,
        Self::Utf16BeNoBom,
        Self::Windows1252,
    ];

    /// Further encodings, grouped by script.
    pub fn groups() -> impl Iterator<Item = (Script, impl Iterator<Item = Self>)> {
        GROUPS
            .iter()
            .map(|(script, encodings)| (*script, encodings.iter().copied()))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE BOM",
            Self::Utf16Be => "UTF-16 BE BOM",
            Self::Utf16LeNoBom => "UTF-16 LE",
            Self::Utf16BeNoBom => "UTF-16 BE",
            Self::Windows1252 => "Windows-1252",
            Self::Legacy(encoding) => encoding.name(),
            Self::Oem(page) => page.name,
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .chain(Self::groups().flat_map(|(_, encodings)| encodings))
            .find(|encoding| encoding.label() == label)
    }

    /// Decodes `bytes` in this encoding, without detection; a matching BOM is skipped.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 | Self::Utf8Bom => {
                decode(UTF_8, bytes.strip_prefix(BOM_UTF8).unwrap_or(bytes))
            }
            Self::Utf16Le => decode(UTF_16LE, bytes.strip_prefix(BOM_UTF16_LE).unwrap_or(bytes)),
            Self::Utf16Be => decode(UTF_16BE, bytes.strip_prefix(BOM_UTF16_BE).unwrap_or(bytes)),
            Self::Utf16LeNoBom => decode(UTF_16LE, bytes),
            Self::Utf16BeNoBom => decode(UTF_16BE, bytes),
            Self::Windows1252 => decode(WINDOWS_1252, bytes),
            Self::Legacy(encoding) => decode(encoding, bytes),
            Self::Oem(page) => {
                let table = page.table();
                bytes
                    .iter()
                    .map(|&b| {
                        if b < 0x80 {
                            b as char
                        } else {
                            table[usize::from(b - 0x80)]
                        }
                    })
                    .collect()
            }
        }
    }

    /// Detects the encoding of `bytes` and decodes them.
    ///
    /// A BOM wins; then UTF-16 is recognized by its zero bytes, valid UTF-8 is UTF-8 and
    /// everything else is read as Windows-1252, which maps every byte, so decoding never fails.
    pub fn detect_and_decode(bytes: Vec<u8>) -> (String, Self) {
        if let Some(rest) = bytes.strip_prefix(BOM_UTF8) {
            return (decode(UTF_8, rest), Self::Utf8Bom);
        }
        if let Some(rest) = bytes.strip_prefix(BOM_UTF16_LE) {
            return (decode(UTF_16LE, rest), Self::Utf16Le);
        }
        if let Some(rest) = bytes.strip_prefix(BOM_UTF16_BE) {
            return (decode(UTF_16BE, rest), Self::Utf16Be);
        }
        if let Some(encoding) = detect_utf16(&bytes) {
            return (encoding.decode(&bytes), encoding);
        }
        match String::from_utf8(bytes) {
            Ok(text) => (text, Self::Utf8),
            Err(err) => (decode(WINDOWS_1252, err.as_bytes()), Self::Windows1252),
        }
    }

    /// Encodes `text`, including a BOM where the encoding has one.
    pub fn encode(self, text: &str) -> Result<Vec<u8>, EncodeError> {
        Ok(match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf8Bom => [BOM_UTF8, text.as_bytes()].concat(),
            Self::Utf16Le => encode_utf16(text, BOM_UTF16_LE, u16::to_le_bytes),
            Self::Utf16Be => encode_utf16(text, BOM_UTF16_BE, u16::to_be_bytes),
            Self::Utf16LeNoBom => encode_utf16(text, &[], u16::to_le_bytes),
            Self::Utf16BeNoBom => encode_utf16(text, &[], u16::to_be_bytes),
            Self::Windows1252 => encode_legacy(WINDOWS_1252, text)?,
            Self::Legacy(encoding) => encode_legacy(encoding, text)?,
            Self::Oem(page) => {
                let table = page.table();
                let mut bytes = Vec::with_capacity(text.len());
                for (char_index, ch) in text.chars().enumerate() {
                    let byte =
                        OemCodePage::byte(&table, ch).ok_or(EncodeError { ch, char_index })?;
                    bytes.push(byte);
                }
                bytes
            }
        })
    }
}

/// UTF-16 without BOM, recognized by text whose every other byte is mostly zero (Latin
/// script). Binary data with zero code units is not taken for UTF-16.
fn detect_utf16(bytes: &[u8]) -> Option<Encoding> {
    const SAMPLE: usize = 4096;
    // Odd lengths or invalid code units would not survive saving unchanged.
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let sample = &bytes[..bytes.len().min(SAMPLE)];
    if sample.len() < 4 {
        return None;
    }
    let units = sample.len() / 2;
    let mut zero_even = 0;
    let mut zero_odd = 0;
    for pair in sample.as_chunks::<2>().0 {
        match (pair[0] == 0, pair[1] == 0) {
            (true, true) => return None,
            (true, false) => zero_even += 1,
            (false, true) => zero_odd += 1,
            (false, false) => {}
        }
    }
    let mostly = |count: usize| count * 10 >= units * 4;
    let rarely = |count: usize| count * 20 <= units;
    let (encoding, decoder) = if mostly(zero_odd) && rarely(zero_even) {
        (Encoding::Utf16LeNoBom, UTF_16LE)
    } else if mostly(zero_even) && rarely(zero_odd) {
        (Encoding::Utf16BeNoBom, UTF_16BE)
    } else {
        return None;
    };
    decoder
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|_| encoding)
}

fn encode_legacy(
    encoding: &'static encoding_rs::Encoding,
    text: &str,
) -> Result<Vec<u8>, EncodeError> {
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors {
        return Err(first_unmappable(encoding, text));
    }
    Ok(bytes.into_owned())
}

fn decode(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> String {
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

fn encode_utf16(text: &str, bom: &[u8], to_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bom.len() + text.len() * 2);
    out.extend_from_slice(bom);
    for unit in text.encode_utf16() {
        out.extend_from_slice(&to_bytes(unit));
    }
    out
}

fn first_unmappable(encoding: &'static encoding_rs::Encoding, text: &str) -> EncodeError {
    let mut buf = [0u8; 4];
    text.chars()
        .enumerate()
        .find(|(_, ch)| encoding.encode(ch.encode_utf8(&mut buf)).2)
        .map(|(char_index, ch)| EncodeError { ch, char_index })
        .expect("encoder reported an error, so an unmappable char exists")
}

/// A character that the target encoding cannot represent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodeError {
    pub ch: char,
    pub char_index: usize,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "character {:?} (U+{:04X}) at position {} cannot be encoded",
            self.ch, self.ch as u32, self.char_index
        )
    }
}

impl std::error::Error for EncodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_boms() {
        assert_eq!(
            Encoding::detect_and_decode(b"\xEF\xBB\xBFhi".to_vec()),
            ("hi".into(), Encoding::Utf8Bom)
        );
        assert_eq!(
            Encoding::detect_and_decode(b"\xFF\xFEh\0i\0".to_vec()),
            ("hi".into(), Encoding::Utf16Le)
        );
        assert_eq!(
            Encoding::detect_and_decode(b"\xFE\xFF\0h\0i".to_vec()),
            ("hi".into(), Encoding::Utf16Be)
        );
    }

    #[test]
    fn falls_back_to_windows_1252() {
        assert_eq!(
            Encoding::detect_and_decode("Grüße".as_bytes().to_vec()),
            ("Grüße".into(), Encoding::Utf8)
        );
        assert_eq!(
            Encoding::detect_and_decode(b"Gr\xFC\xDFe \x80".to_vec()),
            ("Grüße €".into(), Encoding::Windows1252)
        );
    }

    #[test]
    fn round_trips_every_encoding() {
        let text = "Grüße € – ok\n";
        for encoding in Encoding::ALL {
            let bytes = encoding.encode(text).unwrap();
            assert_eq!(
                Encoding::detect_and_decode(bytes),
                (text.into(), encoding),
                "{encoding:?}"
            );
        }
    }

    #[test]
    fn handles_legacy_encodings() {
        let cyrillic = Encoding::from_label("windows-1251").unwrap();
        let bytes = cyrillic.encode("Привет").unwrap();
        assert_eq!(bytes.len(), 6);
        assert_eq!(cyrillic.decode(&bytes), "Привет");
        assert_eq!(cyrillic.encode("ä").unwrap_err().ch, 'ä');
        let groups: usize = Encoding::groups().map(|(_, e)| e.count()).sum();
        assert_eq!(groups, 36);
        assert_eq!(
            Encoding::from_label("Shift_JIS").map(Encoding::label),
            Some("Shift_JIS")
        );
    }

    #[test]
    fn handles_dos_code_pages() {
        for page in [&CP437, &CP850, &CP858] {
            assert_eq!(page.table().len(), 128, "{}", page.name);
        }
        let dos = Encoding::from_label("IBM437").unwrap();
        assert_eq!(dos.decode(b"Gr\x81\xE1e \xC9\xCD"), "Grüße ╔═");
        assert_eq!(dos.encode("Grüße ╔═").unwrap(), b"Gr\x81\xE1e \xC9\xCD");
        assert_eq!(dos.encode("€").unwrap_err().ch, '€');
        assert_eq!(Encoding::Oem(&CP850).decode(b"\x9B\xD5"), "øı");
        assert_eq!(Encoding::Oem(&CP858).decode(b"\x9B\xD5"), "ø€");
        let all: Vec<u8> = (0..=255).collect();
        let text = Encoding::Oem(&CP850).decode(&all);
        assert_eq!(Encoding::Oem(&CP850).encode(&text).unwrap(), all);
    }

    #[test]
    fn detects_utf16_without_bom() {
        let text = "Grüße € – ok\n";
        for encoding in [Encoding::Utf16LeNoBom, Encoding::Utf16BeNoBom] {
            let bytes = encoding.encode(text).unwrap();
            assert_eq!(
                Encoding::detect_and_decode(bytes),
                (text.into(), encoding),
                "{encoding:?}"
            );
        }
        let binary = b"\x00\x00\x01\x02ab\xFFd".to_vec();
        assert_eq!(
            Encoding::detect_and_decode(binary).1,
            Encoding::Windows1252,
            "zero code units are not text"
        );
        for not_utf16 in [&b"\ny\0\x9ch"[..], b"a\0b\0\x00\xD8c\0"] {
            assert_eq!(
                Encoding::detect_and_decode(not_utf16.to_vec()).1,
                Encoding::Windows1252,
                "odd length or a lone surrogate would change when saved"
            );
        }
    }

    #[test]
    fn decodes_explicitly_and_skips_bom() {
        assert_eq!(Encoding::Utf8.decode(b"\xEF\xBB\xBFhi"), "hi");
        assert_eq!(Encoding::Windows1252.decode("ä".as_bytes()), "Ã¤");
        assert_eq!(Encoding::Utf16Le.decode(b"h\0i\0"), "hi");
    }

    #[test]
    fn reports_unmappable_char() {
        assert_eq!(
            Encoding::Windows1252.encode("ab✓"),
            Err(EncodeError {
                ch: '✓',
                char_index: 2
            })
        );
    }
}
