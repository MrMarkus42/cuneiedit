use std::borrow::Cow;

/// Line terminator used when a document is written to disk.
///
/// In memory, documents always use `\n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl Default for LineEnding {
    fn default() -> Self {
        if cfg!(windows) { Self::CrLf } else { Self::Lf }
    }
}

impl LineEnding {
    pub const ALL: [Self; 3] = [Self::Lf, Self::CrLf, Self::Cr];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CR LF",
            Self::Cr => "CR",
        }
    }

    /// Returns the most frequent line ending in `text`, or the platform default if there is none.
    pub fn detect(text: &str) -> Self {
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let mut bytes = text.bytes().peekable();
        while let Some(b) = bytes.next() {
            match b {
                b'\r' if bytes.peek() == Some(&b'\n') => {
                    bytes.next();
                    crlf += 1;
                }
                b'\r' => cr += 1,
                b'\n' => lf += 1,
                _ => {}
            }
        }
        if lf == 0 && crlf == 0 && cr == 0 {
            Self::default()
        } else if crlf >= lf && crlf >= cr {
            Self::CrLf
        } else if lf >= cr {
            Self::Lf
        } else {
            Self::Cr
        }
    }

    /// Converts every `\r\n` and `\r` in `text` to `\n`.
    pub fn normalize(text: &str) -> Cow<'_, str> {
        if !text.contains('\r') {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            } else {
                out.push(ch);
            }
        }
        Cow::Owned(out)
    }

    /// Converts the `\n` line endings of `text` to this line ending.
    pub fn apply(self, text: &str) -> String {
        match self {
            Self::Lf => text.to_owned(),
            _ => text.replace('\n', self.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_majority() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\n"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\nb\nc\r\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\rb\r"), LineEnding::Cr);
        assert_eq!(LineEnding::detect("abc"), LineEnding::default());
    }

    #[test]
    fn normalizes_and_applies() {
        assert_eq!(LineEnding::normalize("a\r\nb\rc\n"), "a\nb\nc\n");
        assert_eq!(LineEnding::CrLf.apply("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(LineEnding::Cr.apply("a\nb"), "a\rb");
    }
}
