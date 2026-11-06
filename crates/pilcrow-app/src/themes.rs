//! Built-in color themes for the text area.

use eframe::egui::Color32;
use pilcrow_editor::Palette;

pub struct Theme {
    pub name: &'static str,
    pub palette: Palette,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "Paper",
        palette: Palette {
            background: rgb(0xFFFFFF),
            text: rgb(0x000000),
            gutter: rgb(0xF3F3F3),
            line_number: rgb(0x8A8A8A),
            current_line: rgb(0xF2F5FA),
            selection: rgb(0xB3D7FF),
            cursor: rgb(0x000000),
            keyword: rgb(0x0000C0),
            ty: rgb(0x007070),
            string: rgb(0xA31515),
            number: rgb(0x098658),
            comment: rgb(0x6A737D),
            meta: rgb(0x795E26),
        },
    },
    Theme {
        name: "Solarized Light",
        palette: Palette {
            background: rgb(0xFDF6E3),
            text: rgb(0x586E75),
            gutter: rgb(0xEEE8D5),
            line_number: rgb(0x93A1A1),
            current_line: rgb(0xEEE8D5),
            selection: rgb(0xDDD6C1),
            cursor: rgb(0x073642),
            keyword: rgb(0x859900),
            ty: rgb(0xB58900),
            string: rgb(0x2AA198),
            number: rgb(0xD33682),
            comment: rgb(0x93A1A1),
            meta: rgb(0xCB4B16),
        },
    },
    Theme {
        name: "Solarized Dark",
        palette: Palette {
            background: rgb(0x002B36),
            text: rgb(0x839496),
            gutter: rgb(0x073642),
            line_number: rgb(0x586E75),
            current_line: rgb(0x073642),
            selection: rgb(0x0E4B5A),
            cursor: rgb(0x93A1A1),
            keyword: rgb(0x859900),
            ty: rgb(0xB58900),
            string: rgb(0x2AA198),
            number: rgb(0xD33682),
            comment: rgb(0x586E75),
            meta: rgb(0xCB4B16),
        },
    },
    Theme {
        name: "Monokai",
        palette: Palette {
            background: rgb(0x272822),
            text: rgb(0xF8F8F2),
            gutter: rgb(0x2F3029),
            line_number: rgb(0x90908A),
            current_line: rgb(0x3E3D32),
            selection: rgb(0x49483E),
            cursor: rgb(0xF8F8F0),
            keyword: rgb(0xF92672),
            ty: rgb(0x66D9EF),
            string: rgb(0xE6DB74),
            number: rgb(0xAE81FF),
            comment: rgb(0x75715E),
            meta: rgb(0xA6E22E),
        },
    },
    Theme {
        name: "Nord",
        palette: Palette {
            background: rgb(0x2E3440),
            text: rgb(0xD8DEE9),
            gutter: rgb(0x3B4252),
            line_number: rgb(0x616E88),
            current_line: rgb(0x3B4252),
            selection: rgb(0x434C5E),
            cursor: rgb(0xD8DEE9),
            keyword: rgb(0x81A1C1),
            ty: rgb(0x8FBCBB),
            string: rgb(0xA3BE8C),
            number: rgb(0xB48EAD),
            comment: rgb(0x616E88),
            meta: rgb(0x88C0D0),
        },
    },
];

/// The palette of the theme called `name`; `None` (follow the appearance) for unknown names.
pub fn palette(name: &str) -> Option<Palette> {
    THEMES
        .iter()
        .find(|theme| theme.name == name)
        .map(|theme| theme.palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_themes_by_name() {
        assert!(palette("Nord").is_some());
        assert!(palette("").is_none());
        let names: Vec<_> = THEMES.iter().map(|t| t.name).collect();
        for name in &names {
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1);
        }
    }
}
