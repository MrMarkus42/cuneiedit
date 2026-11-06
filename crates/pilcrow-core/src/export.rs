//! Exporting a document with syntax colors, as HTML or as a printable PDF.
//!
//! The PDF uses the built-in Courier font with Windows-1252 encoding, so no font has to be
//! embedded; characters outside that encoding are printed as `?`.

use std::fmt::Write as _;

use encoding_rs::WINDOWS_1252;

use crate::Document;
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
const LINE_HEIGHT: f32 = 11.0;
/// Width of a Courier character in units of the font size.
const COURIER_WIDTH: f32 = 0.6;

fn pdf_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'(');
    let mut buf = [0u8; 4];
    for c in text.chars() {
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
fn page_content(rows: &[ColoredLine], footer: &str, colors: Colors<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    let top = PAGE_HEIGHT - MARGIN - FONT_SIZE;
    out.extend_from_slice(
        format!("BT\n/F1 {FONT_SIZE} Tf\n{LINE_HEIGHT} TL\n{MARGIN} {top} Td\n").as_bytes(),
    );
    let mut color = None;
    let set_color = |kind: Option<TokenKind>, out: &mut Vec<u8>| {
        let [r, g, b] = colors(kind).map(|v| f32::from(v) / 255.0);
        out.extend_from_slice(format!("{r:.3} {g:.3} {b:.3} rg\n").as_bytes());
    };
    set_color(None, &mut out);
    for row in rows {
        let mut start = 0;
        while start < row.len() {
            let kind = row[start].1;
            let end = row[start..]
                .iter()
                .position(|&(_, k)| k != kind)
                .map_or(row.len(), |offset| start + offset);
            if Some(kind) != color {
                set_color(kind, &mut out);
                color = Some(kind);
            }
            let text: String = row[start..end].iter().map(|&(c, _)| c).collect();
            pdf_string(&text, &mut out);
            start = end;
        }
        out.extend_from_slice(b"T*\n");
    }
    out.extend_from_slice(b"ET\nBT\n");
    set_color(None, &mut out);
    out.extend_from_slice(format!("/F1 8 Tf\n{MARGIN} {} Td\n", MARGIN / 2.0).as_bytes());
    pdf_string(footer, &mut out);
    out.extend_from_slice(b"ET\n");
    out
}

/// The document as A4 pages, with long lines wrapped and page numbers below `title`.
pub fn pdf(
    doc: &Document,
    language: Language,
    title: &str,
    tab_width: usize,
    colors: Colors<'_>,
) -> Vec<u8> {
    let columns = ((PAGE_WIDTH - 2.0 * MARGIN) / (FONT_SIZE * COURIER_WIDTH)) as usize;
    let rows_per_page = ((PAGE_HEIGHT - 2.0 * MARGIN) / LINE_HEIGHT) as usize;
    let tab_width = tab_width.max(1);
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
    let pages: Vec<&[ColoredLine]> = if rows.is_empty() {
        vec![&[]]
    } else {
        rows.chunks(rows_per_page).collect()
    };

    // Objects: 1 catalog, 2 page tree, 3 font, then a content stream and a page per page.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 5 + 2 * i))
        .collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_vec(),
    );
    for (index, page) in pages.iter().enumerate() {
        let footer = format!("{title} \u{2013} {}/{}", index + 1, pages.len());
        let content = page_content(page, &footer, colors);
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(&content);
        stream.extend_from_slice(b"endstream");
        objects.push(stream);
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                4 + 2 * index
            )
            .into_bytes(),
        );
    }

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
        assert!(html.contains("<span style=\"color: #0000ff\">fn</span> a&lt;T&gt;() {}\n"));
        assert!(html.contains("&quot;&amp;&quot;"));
    }

    #[test]
    fn exports_pdf_pages() {
        let text = format!("let x = (a);\n{}\n\tÜ✓", "y".repeat(200));
        let rust = Language::from_name("Rust").unwrap();
        let pdf = pdf(&doc(&text), rust, "t", 4, &colors);
        let content = String::from_utf8_lossy(&pdf);
        assert!(content.starts_with("%PDF-1.4"));
        assert!(content.contains("0.000 0.000 1.000 rg\n(let) Tj"));
        assert!(content.contains(" x = \\(a\\);) Tj"));
        assert!(
            pdf.windows(4).any(|w| w == b" \xDC?)"),
            "Windows-1252, unknown as ?"
        );
        assert!(content.contains("(t \u{fffd} 1/1) Tj"), "footer");
        let start = content.rfind("startxref\n").unwrap() + "startxref\n".len();
        let xref: usize = content[start..].lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref..].starts_with(b"xref"), "offsets are in bytes");
        let rows = content.matches("T*").count();
        assert_eq!(rows, 1 + 3 + 1, "the long line wraps into three rows");
    }
}
