//! Exporting a document with syntax colors, as HTML or as a printable PDF.
//!
//! The PDF embeds the given TrueType fonts (the editor's), so any character they cover is
//! printed; without fonts it falls back to the built-in Courier with Windows-1252.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use encoding_rs::WINDOWS_1252;

use crate::Document;
use crate::font::TrueType;
use crate::highlight::{Language, LineState, Token, TokenKind, highlight_line};

/// Text color for a token kind (`None` is plain text), as RGB.
pub type Colors<'a> = &'a dyn Fn(Option<TokenKind>) -> [u8; 3];

/// Chars of a line with their color kind.
type ColoredLine = Vec<(char, Option<TokenKind>)>;

/// Lines of the document, each with the color kind of every char.
fn colored_lines(doc: &Document, language: Language) -> Vec<ColoredLine> {
    let mut state = LineState::Normal;
    let mut lines = Vec::with_capacity(doc.text().len_lines());
    for slice in doc.text().lines() {
        let text: String = slice.chars().filter(|&c| c != '\n').collect();
        let mut tokens: Vec<Token> = Vec::new();
        state = highlight_line(language, &text, state, Some(&mut tokens));
        let mut kinds = vec![None; text.chars().count()];
        let len = kinds.len();
        for token in tokens {
            for kind in &mut kinds[token.start.min(len)..token.end.min(len)] {
                *kind = Some(token.kind);
            }
        }
        lines.push(text.chars().zip(kinds).collect());
    }
    lines
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn escape_html(c: char, out: &mut String) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        c => out.push(c),
    }
}

/// A standalone HTML page showing the document with syntax colors.
pub fn html(
    doc: &Document,
    language: Language,
    title: &str,
    colors: Colors<'_>,
    background: [u8; 3],
) -> String {
    let mut out =
        String::from("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>");
    title.chars().for_each(|c| escape_html(c, &mut out));
    let _ = write!(
        out,
        "</title>\n<style>body {{ background: {}; color: {}; }} \
         pre {{ font-family: ui-monospace, Menlo, Consolas, monospace; }}</style>\n\
         </head>\n<body>\n<pre>",
        hex(background),
        hex(colors(None)),
    );
    for (index, line) in colored_lines(doc, language).iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut current = None;
        for &(c, kind) in line {
            if kind != current {
                if current.is_some() {
                    out.push_str("</span>");
                }
                if kind.is_some() {
                    let _ = write!(out, "<span style=\"color: {}\">", hex(colors(kind)));
                }
                current = kind;
            }
            escape_html(c, &mut out);
        }
        if current.is_some() {
            out.push_str("</span>");
        }
    }
    out.push_str("</pre>\n</body>\n</html>\n");
    out
}

const PAGE_WIDTH: f32 = 595.0;
const PAGE_HEIGHT: f32 = 842.0;
const MARGIN: f32 = 50.0;
const FONT_SIZE: f32 = 9.0;
const FOOTER_SIZE: f32 = 8.0;
const LINE_HEIGHT: f32 = 11.0;
/// Width of a Courier character in units of the font size.
const COURIER_WIDTH: f32 = 0.6;

/// Fonts of a PDF: the given TrueType fonts in order of preference (each char uses the
/// first font that has it), or the built-in Courier if there are none.
struct PdfFonts<'a> {
    fonts: &'a [TrueType],
    cache: HashMap<char, (usize, u16)>,
    /// Per font the glyphs used, with the char they show (for copying text from the PDF).
    used: Vec<BTreeMap<u16, char>>,
}

impl<'a> PdfFonts<'a> {
    fn new(fonts: &'a [TrueType]) -> Self {
        Self {
            fonts,
            cache: HashMap::new(),
            used: vec![BTreeMap::new(); fonts.len()],
        }
    }

    /// Width of a char in units of the font size.
    fn char_width(&self) -> f32 {
        let Some(font) = self.fonts.first() else {
            return COURIER_WIDTH;
        };
        ['M', ' ']
            .iter()
            .find_map(|&c| font.glyph(c))
            .map_or(COURIER_WIDTH, |glyph| {
                f32::from(font.advance(glyph)) / f32::from(font.units_per_em())
            })
    }

    /// Font and glyph for `c`; chars no font has show the missing-glyph box of the first.
    fn glyph(&mut self, c: char) -> (usize, u16) {
        let fonts = self.fonts;
        let found = *self.cache.entry(c).or_insert_with(|| {
            fonts
                .iter()
                .enumerate()
                .find_map(|(index, font)| font.glyph(c).map(|glyph| (index, glyph)))
                .unwrap_or((0, 0))
        });
        self.used[found.0].entry(found.1).or_insert(c);
        found
    }

    /// Appends operators showing `text` at `size`; `current` is the font in effect.
    fn show(
        &mut self,
        text: &[char],
        size: f32,
        current: &mut Option<(usize, f32)>,
        out: &mut Vec<u8>,
    ) {
        let mut select = |font: usize, out: &mut Vec<u8>| {
            if *current != Some((font, size)) {
                out.extend_from_slice(format!("/F{font} {size} Tf\n").as_bytes());
                *current = Some((font, size));
            }
        };
        if self.fonts.is_empty() {
            select(0, out);
            courier_string(text, out);
            return;
        }
        let glyphs: Vec<(usize, u16)> = text.iter().map(|&c| self.glyph(c)).collect();
        for run in glyphs.chunk_by(|a, b| a.0 == b.0) {
            select(run[0].0, out);
            out.push(b'<');
            for (_, glyph) in run {
                out.extend_from_slice(format!("{glyph:04X}").as_bytes());
            }
            out.extend_from_slice(b"> Tj\n");
        }
    }
}

/// `text` as a Windows-1252 string for Courier; other characters become `?`.
fn courier_string(text: &[char], out: &mut Vec<u8>) {
    out.push(b'(');
    let mut buf = [0u8; 4];
    for &c in text {
        let (bytes, _, unmappable) = WINDOWS_1252.encode(c.encode_utf8(&mut buf));
        let byte = if unmappable || bytes.len() != 1 {
            b'?'
        } else {
            bytes[0]
        };
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out.extend_from_slice(b") Tj\n");
}

/// One page's text as a PDF content stream.
fn page_content(
    rows: &[ColoredLine],
    footer: &str,
    colors: Colors<'_>,
    fonts: &mut PdfFonts<'_>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let top = PAGE_HEIGHT - MARGIN - FONT_SIZE;
    out.extend_from_slice(format!("BT\n{LINE_HEIGHT} TL\n{MARGIN} {top} Td\n").as_bytes());
    let mut font = None;
    let mut color = None;
    let set_color = |kind: Option<TokenKind>, out: &mut Vec<u8>| {
        let [r, g, b] = colors(kind).map(|v| f32::from(v) / 255.0);
        out.extend_from_slice(format!("{r:.3} {g:.3} {b:.3} rg\n").as_bytes());
    };
    set_color(None, &mut out);
    for row in rows {
        for run in row.chunk_by(|a, b| a.1 == b.1) {
            let kind = run[0].1;
            if Some(kind) != color {
                set_color(kind, &mut out);
                color = Some(kind);
            }
            let text: Vec<char> = run.iter().map(|&(c, _)| c).collect();
            fonts.show(&text, FONT_SIZE, &mut font, &mut out);
        }
        out.extend_from_slice(b"T*\n");
    }
    out.extend_from_slice(b"ET\nBT\n");
    set_color(None, &mut out);
    out.extend_from_slice(format!("{MARGIN} {} Td\n", MARGIN / 2.0).as_bytes());
    let footer: Vec<char> = footer.chars().collect();
    let mut font = None;
    fonts.show(&footer, FOOTER_SIZE, &mut font, &mut out);
    out.extend_from_slice(b"ET\n");
    out
}

fn stream(dict: &str, content: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", content.len()).into_bytes();
    out.extend_from_slice(content);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Maps glyphs back to text, so text can be searched and copied in the PDF.
fn to_unicode(used: &BTreeMap<u16, char>) -> Vec<u8> {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = used.iter().filter(|(glyph, _)| **glyph != 0).collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (glyph, c) in chunk {
            let mut units = [0u16; 2];
            let hex: String = c
                .encode_utf16(&mut units)
                .iter()
                .map(|unit| format!("{unit:04X}"))
                .collect();
            let _ = writeln!(out, "<{glyph:04X}> <{hex}>");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

/// PDF objects embedding `font` as `/F{index}`, numbered from `first`; returns the
/// objects, the first being the font dictionary.
fn font_objects(
    font: &TrueType,
    index: usize,
    used: &BTreeMap<u16, char>,
    first: usize,
) -> Vec<Vec<u8>> {
    let scale = |v: i32| v * 1000 / i32::from(font.units_per_em());
    let name = format!("CuneiEditFont{index}");
    let widths: String = used
        .keys()
        .map(|&glyph| format!("{glyph} [{}]", scale(i32::from(font.advance(glyph)))))
        .collect::<Vec<_>>()
        .join(" ");
    let [x_min, y_min, x_max, y_max] = font.bbox().map(|v| scale(i32::from(v)));
    let ascent = scale(i32::from(font.ascent()));
    let descent = scale(i32::from(font.descent()));
    vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H \
             /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
            first + 1,
            first + 4
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor {} 0 R /CIDToGIDMap /Identity /W [{widths}] >>",
            first + 2
        )
        .into_bytes(),
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 4 \
             /FontBBox [{x_min} {y_min} {x_max} {y_max}] /ItalicAngle 0 /Ascent {ascent} \
             /Descent {descent} /CapHeight {ascent} /StemV 80 /FontFile2 {} 0 R >>",
            first + 3
        )
        .into_bytes(),
        stream(&format!("/Length1 {}", font.data().len()), font.data()),
        stream("", &to_unicode(used)),
    ]
}

/// Lines with tabs expanded and control characters dropped, wrapped every `columns` chars.
fn wrapped_rows(
    doc: &Document,
    language: Language,
    tab_width: usize,
    columns: usize,
) -> Vec<ColoredLine> {
    let tab_width = tab_width.max(1);
    let columns = columns.max(1);
    let mut rows: Vec<ColoredLine> = Vec::new();
    for line in colored_lines(doc, language) {
        let mut row = Vec::new();
        for (c, kind) in line {
            if c == '\t' {
                let spaces = tab_width - row.len() % tab_width;
                row.extend(std::iter::repeat_n((' ', kind), spaces));
            } else if !c.is_control() {
                row.push((c, kind));
            }
        }
        if row.is_empty() {
            rows.push(row);
            continue;
        }
        rows.extend(row.chunks(columns).map(<[_]>::to_vec));
    }
    rows
}

/// One printed row: runs of text with their color kind (`None` is plain text).
pub type PrintRow = Vec<(String, Option<TokenKind>)>;

/// The document laid out for a printer: pages of at most `rows_per_page` rows, each at most
/// `columns` chars wide. There is always at least one page.
pub fn pages(
    doc: &Document,
    language: Language,
    tab_width: usize,
    columns: usize,
    rows_per_page: usize,
) -> Vec<Vec<PrintRow>> {
    let rows: Vec<PrintRow> = wrapped_rows(doc, language, tab_width, columns)
        .iter()
        .map(|row| {
            row.chunk_by(|a, b| a.1 == b.1)
                .map(|run| (run.iter().map(|&(c, _)| c).collect(), run[0].1))
                .collect()
        })
        .collect();
    if rows.is_empty() {
        return vec![Vec::new()];
    }
    rows.chunks(rows_per_page.max(1))
        .map(<[_]>::to_vec)
        .collect()
}

/// The document as A4 pages, with long lines wrapped and page numbers below `title`.
/// Text uses the first of `fonts` that has a character (missing ones show as a box), or
/// Courier with Windows-1252 if `fonts` is empty.
pub fn pdf(
    doc: &Document,
    language: Language,
    title: &str,
    tab_width: usize,
    colors: Colors<'_>,
    fonts: &[TrueType],
) -> Vec<u8> {
    let mut pdf_fonts = PdfFonts::new(fonts);
    let columns =
        (((PAGE_WIDTH - 2.0 * MARGIN) / (FONT_SIZE * pdf_fonts.char_width())) as usize).max(1);
    let rows_per_page = ((PAGE_HEIGHT - 2.0 * MARGIN) / LINE_HEIGHT) as usize;
    let rows = wrapped_rows(doc, language, tab_width, columns);
    let pages: Vec<&[ColoredLine]> = if rows.is_empty() {
        vec![&[]]
    } else {
        rows.chunks(rows_per_page).collect()
    };
    let contents: Vec<Vec<u8>> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let footer = format!("{title} \u{2013} {}/{}", index + 1, pages.len());
            page_content(page, &footer, colors, &mut pdf_fonts)
        })
        .collect();

    // Objects: 1 catalog, 2 page tree, then the fonts, then a content stream and a page
    // per page.
    let mut objects: Vec<Vec<u8>> = vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(), Vec::new()];
    let mut font_refs = String::new();
    if fonts.is_empty() {
        let _ = write!(font_refs, "/F0 {} 0 R", objects.len() + 1);
        objects.push(
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        );
    }
    for (index, (font, used)) in fonts.iter().zip(&pdf_fonts.used).enumerate() {
        if used.is_empty() {
            continue;
        }
        let first = objects.len() + 1;
        let _ = write!(font_refs, " /F{index} {first} 0 R");
        objects.extend(font_objects(font, index, used, first));
    }
    let mut kids = Vec::new();
    for content in contents {
        objects.push(stream("", &content));
        let content_ref = objects.len();
        kids.push(format!("{} 0 R", objects.len() + 1));
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                 /Resources << /Font << {font_refs} >> >> /Contents {content_ref} 0 R >>"
            )
            .into_bytes(),
        );
    }
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        kids.len()
    )
    .into_bytes();

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    let _ = write!(
        table,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(table.as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(kind: Option<TokenKind>) -> [u8; 3] {
        match kind {
            Some(TokenKind::Keyword) => [0, 0, 255],
            _ => [0, 0, 0],
        }
    }

    fn doc(text: &str) -> Document {
        let mut doc = Document::new();
        doc.replace(0..0, text);
        doc
    }

    #[test]
    fn exports_html_with_colors() {
        let rust = Language::from_name("Rust").unwrap();
        let html = html(
            &doc("fn a<T>() {}\n\"&\""),
            rust,
            "a & b",
            &colors,
            [255; 3],
        );
        assert!(html.contains("<title>a &amp; b</title>"));
        assert!(html.contains(
            "<span style=\"color: #0000ff\">fn</span> \
             <span style=\"color: #000000\">a</span>&lt;T&gt;() {}\n"
        ));
        assert!(html.contains("&quot;&amp;&quot;"));
    }

    #[test]
    fn lays_out_pages_for_printers() {
        let rust = Language::from_name("Rust").unwrap();
        let laid_out = pages(&doc("fn a() {}\n\tbbbbbb\n\nc"), rust, 4, 6, 3);
        let text = |row: &PrintRow| row.iter().map(|(t, _)| t.as_str()).collect::<String>();
        let rows: Vec<Vec<String>> = laid_out
            .iter()
            .map(|p| p.iter().map(text).collect())
            .collect();
        assert_eq!(
            rows,
            vec![vec!["fn a()", " {}", "    bb"], vec!["bbbb", "", "c"]]
        );
        assert_eq!(
            laid_out[0][0][0],
            ("fn".to_owned(), Some(TokenKind::Keyword))
        );
        assert_eq!(pages(&doc(""), rust, 4, 0, 0).len(), 1, "always one page");
    }

    #[test]
    fn exports_pdf_pages() {
        let text = format!("let x = (a);\n{}\n\tÜ✓", "y".repeat(200));
        let rust = Language::from_name("Rust").unwrap();
        let pdf = pdf(&doc(&text), rust, "t", 4, &colors, &[]);
        let content = String::from_utf8_lossy(&pdf);
        assert!(content.starts_with("%PDF-1.4"));
        assert!(content.contains("0.000 0.000 1.000 rg\n/F0 9 Tf\n(let) Tj"));
        assert!(content.contains(" x = \\(a\\);) Tj"));
        assert!(
            pdf.windows(4).any(|w| w == b" \xDC?)"),
            "Windows-1252, unknown as ?"
        );
        assert!(content.contains("(t \u{fffd} 1/1) Tj"), "footer");
        assert_valid_xref(&pdf);
        let rows = content.matches("T*").count();
        assert_eq!(rows, 1 + 3 + 1, "the long line wraps into three rows");
    }

    fn assert_valid_xref(pdf: &[u8]) {
        let content = String::from_utf8_lossy(pdf);
        let start = content.rfind("startxref\n").unwrap() + "startxref\n".len();
        let xref: usize = content[start..].lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref..].starts_with(b"xref"), "offsets are in bytes");
        let entries = pdf[xref..]
            .split(|&b| b == b'\n')
            .skip(3)
            .take_while(|line| line.ends_with(b" n "));
        for (number, line) in entries.enumerate() {
            let offset: usize = std::str::from_utf8(&line[..10]).unwrap().parse().unwrap();
            let header = format!("{} 0 obj", number + 1);
            assert!(pdf[offset..].starts_with(header.as_bytes()), "{header}");
        }
    }

    #[test]
    fn embeds_fonts_for_unicode() {
        let font = TrueType::parse(&crate::font::tests::test_font(), 0).unwrap();
        let text = "A€ß\n😀";
        let pdf = pdf(&doc(text), Language::PLAIN_TEXT, "A", 4, &colors, &[font]);
        let content = String::from_utf8_lossy(&pdf);
        assert_valid_xref(&pdf);
        assert!(
            content.contains("/F0 9 Tf\n<000100020000> Tj"),
            "ß is missing: box"
        );
        assert!(content.contains("<0003> Tj"));
        assert!(content.contains("/Subtype /CIDFontType2"));
        assert!(content.contains("/W [0 [500] 1 [600] 2 [700] 3 [1000]]"));
        assert!(content.contains("<0002> <20AC>"), "ToUnicode");
        assert!(content.contains("<0003> <D83DDE00>"), "surrogate pair");
        assert!(!content.contains("Courier"));
        assert_eq!(content.matches("/FontFile2").count(), 1);
    }
}
