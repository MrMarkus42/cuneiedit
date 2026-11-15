//! The file types CuneiEdit can be registered for on Windows, in groups. The installer's
//! choices (`packaging/windows/file-types.iss`) are generated from this list; a test checks
//! that the file is up to date (run it with `CUNEIEDIT_WRITE_ISS=1` to rewrite it).
//!
//! The installer calls `CuneiEdit.exe --register-file-types <ext>…` after installing and
//! `--unregister-file-types` when uninstalling; the settings dialog changes the same
//! registration.

use crate::i18n::Texts;

pub struct Group {
    /// Identifier in the installer.
    pub id: &'static str,
    pub name: fn(&Texts) -> &'static str,
    /// Whether the group is chosen by default (scripts, web pages and images are not:
    /// their double click should keep running or showing them).
    pub recommended: bool,
    pub extensions: &'static [&'static str],
}

pub const GROUPS: &[Group] = &[
    Group {
        id: "text",
        name: |t| t.file_group_text,
        recommended: true,
        extensions: &[
            "txt", "text", "log", "nfo", "md", "markdown", "rst", "adoc", "tex",
        ],
    },
    Group {
        id: "config",
        name: |t| t.file_group_config,
        recommended: true,
        extensions: &[
            "ini",
            "cfg",
            "conf",
            "config",
            "toml",
            "yaml",
            "yml",
            "json",
            "jsonc",
            "properties",
            "editorconfig",
            "gitignore",
            "gitattributes",
            "env",
        ],
    },
    Group {
        id: "data",
        name: |t| t.file_group_data,
        recommended: true,
        extensions: &["csv", "tsv", "xml", "xsd", "xsl", "sql", "diff", "patch"],
    },
    Group {
        id: "code",
        name: |t| t.file_group_code,
        recommended: true,
        extensions: &[
            "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "cs", "java", "kt", "kts", "go", "rs",
            "swift", "php", "lua", "dart", "scala", "groovy", "gradle", "cmake", "asm", "pas",
            "fs", "hs", "ex", "exs", "erl", "zig", "jl", "r",
        ],
    },
    Group {
        id: "web",
        name: |t| t.file_group_web,
        recommended: true,
        extensions: &["css", "scss", "less", "jsx", "tsx", "mjs", "vue"],
    },
    Group {
        id: "pages",
        name: |t| t.file_group_pages,
        recommended: false,
        extensions: &["html", "htm", "svg", "ts"],
    },
    Group {
        id: "scripts",
        name: |t| t.file_group_scripts,
        recommended: false,
        extensions: &[
            "bat", "cmd", "ps1", "psm1", "vbs", "js", "py", "pyw", "rb", "pl", "sh", "reg",
        ],
    },
];

/// Every extension, in the order of [`GROUPS`].
pub fn all() -> Vec<&'static str> {
    GROUPS
        .iter()
        .flat_map(|g| g.extensions.iter().copied())
        .collect()
}

/// The extensions chosen by default.
pub fn recommended() -> Vec<&'static str> {
    GROUPS
        .iter()
        .filter(|g| g.recommended)
        .flat_map(|g| g.extensions.iter().copied())
        .collect()
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Register exactly these known extensions (unknown ones are ignored).
    Register(Vec<&'static str>),
    Unregister,
}

/// Reads `--register-file-types <ext>…` or `--unregister-file-types` (arguments after the
/// program name).
pub fn parse_args(args: &[String]) -> Option<Request> {
    match args.first().map(String::as_str) {
        Some("--register-file-types") => {
            let known = all();
            let mut chosen = Vec::new();
            for arg in &args[1..] {
                for word in arg.split_whitespace() {
                    let word = word.trim_start_matches('.').to_ascii_lowercase();
                    if let Some(&extension) = known.iter().find(|&&e| e == word)
                        && !chosen.contains(&extension)
                    {
                        chosen.push(extension);
                    }
                }
            }
            Some(Request::Register(chosen))
        }
        Some("--unregister-file-types") => Some(Request::Unregister),
        _ => None,
    }
}

/// Handles the installer's command line; returns the exit code if it was such a call.
pub fn run_from_args() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match parse_args(&args)? {
        Request::Register(chosen) => {
            let texts = crate::i18n::Language::from_system().texts();
            register(&chosen, texts.file_type_description)
        }
        Request::Unregister => unregister(),
    };
    Some(i32::from(result.is_err()))
}

/// The current registration, as shown in the settings.
#[derive(Clone, Debug)]
pub struct State {
    /// Per extension of [`all`]: registered for CuneiEdit.
    pub registered: Vec<bool>,
    /// Per extension of [`all`]: a double click opens CuneiEdit.
    pub default: Vec<bool>,
}

/// Whether this platform registers file types (Windows only).
pub const SUPPORTED: bool = cfg!(windows);

#[cfg(windows)]
pub fn state() -> State {
    use cuneiedit_windows::file_types;
    let all = all();
    let exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.canonicalize().ok());
    let default = all
        .iter()
        .map(|extension| {
            let program = file_types::default_program(extension)
                .and_then(|program| program.canonicalize().ok());
            exe.is_some() && program == exe
        })
        .collect();
    State {
        registered: file_types::registered(&all),
        default,
    }
}

/// Registers the extensions whose entry in `selected` (per extension of [`all`]) is true.
pub fn apply(selected: &[bool], texts: &Texts) -> std::io::Result<()> {
    let chosen: Vec<&str> = all()
        .into_iter()
        .zip(selected)
        .filter_map(|(extension, &on)| on.then_some(extension))
        .collect();
    register(&chosen, texts.file_type_description)
}

#[cfg(windows)]
fn register(chosen: &[&str], description: &str) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    cuneiedit_windows::file_types::register(&exe, &all(), chosen, description)
}

#[cfg(windows)]
fn unregister() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    cuneiedit_windows::file_types::unregister(&exe, &all())
}

#[cfg(windows)]
pub fn open_default_apps() -> std::io::Result<()> {
    cuneiedit_windows::file_types::open_default_apps()
}

#[cfg(not(windows))]
pub fn state() -> State {
    State {
        registered: vec![false; all().len()],
        default: vec![false; all().len()],
    }
}

#[cfg(not(windows))]
fn register(_chosen: &[&str], _description: &str) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(not(windows))]
fn unregister() -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(not(windows))]
pub fn open_default_apps() -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Inno Setup code for the installer's choices: one task per extension under its group, and
/// a function that lists the chosen extensions.
#[cfg(test)]
fn installer_script() -> String {
    use crate::i18n::Language;
    let mut out = String::from(
        "; Generated from crates/cuneiedit-app/src/file_types.rs – do not edit by hand.\n\n",
    );
    out.push_str("[CustomMessages]\n");
    for language in Language::ALL {
        let texts = language.texts();
        let code = match language {
            Language::English => "english",
            Language::German => "german",
        };
        out.push_str(&format!("{code}.FileTypes={}\n", texts.file_types_title));
        for group in GROUPS {
            out.push_str(&format!(
                "{code}.FileGroup_{}={}\n",
                group.id,
                (group.name)(texts)
            ));
        }
    }
    out.push_str("\n[Tasks]\n");
    out.push_str("Name: \"types\"; Description: \"{cm:FileTypes}\"\n");
    for group in GROUPS {
        let flags = if group.recommended {
            ""
        } else {
            "; Flags: unchecked"
        };
        out.push_str(&format!(
            "Name: \"types\\{0}\"; Description: \"{{cm:FileGroup_{0}}}\"{flags}\n",
            group.id
        ));
        for extension in group.extensions {
            out.push_str(&format!(
                "Name: \"types\\{}\\{}\"; Description: \".{extension}\"{flags}\n",
                group.id,
                task_name(extension)
            ));
        }
    }
    out.push_str(
        "\n[Code]\nfunction ChosenFileTypes(Param: String): String;\nbegin\n  Result := '';\n",
    );
    for group in GROUPS {
        for extension in group.extensions {
            out.push_str(&format!(
                "  if WizardIsTaskSelected('types\\{}\\{}') then Result := Result + ' {extension}';\n",
                group.id,
                task_name(extension)
            ));
        }
    }
    out.push_str("end;\n");
    out
}

/// Task names must be identifiers.
#[cfg(test)]
fn task_name(extension: &str) -> String {
    format!("ext_{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_unique_and_plain() {
        let all = all();
        for (index, extension) in all.iter().enumerate() {
            assert!(!all[..index].contains(extension), "{extension} twice");
            assert!(
                extension
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{extension}"
            );
        }
    }

    #[test]
    fn reads_the_installer_command_line() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&["a.txt"])), None);
        assert_eq!(parse_args(&[]), None);
        assert_eq!(
            parse_args(&args(&["--register-file-types", " txt .RS", "nope", "txt"])),
            Some(Request::Register(vec!["txt", "rs"]))
        );
        assert_eq!(
            parse_args(&args(&["--register-file-types"])),
            Some(Request::Register(Vec::new()))
        );
        assert_eq!(
            parse_args(&args(&["--unregister-file-types"])),
            Some(Request::Unregister)
        );
    }

    #[test]
    fn scripts_are_not_recommended() {
        let recommended = recommended();
        for extension in ["bat", "cmd", "ps1", "vbs", "js", "py", "reg", "html"] {
            assert!(!recommended.contains(&extension), "{extension}");
        }
        assert!(recommended.contains(&"txt"));
    }

    #[test]
    fn installer_script_is_up_to_date() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/windows/file-types.iss"
        );
        let script = installer_script();
        if std::env::var_os("CUNEIEDIT_WRITE_ISS").is_some() {
            std::fs::write(path, &script).expect("packaging/windows is writable");
        }
        let current = std::fs::read_to_string(path).unwrap_or_default();
        assert_eq!(
            current.replace("\r\n", "\n"),
            script,
            "run the tests with CUNEIEDIT_WRITE_ISS=1"
        );
    }
}
