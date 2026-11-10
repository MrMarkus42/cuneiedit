# CuneiEdit

**A fast, simple and secure text editor for macOS, Windows and Linux – written in Rust.**

[![CI](https://github.com/MrMarkus42/cuneiedit/actions/workflows/ci.yml/badge.svg)](https://github.com/MrMarkus42/cuneiedit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/MrMarkus42/cuneiedit?include_prereleases)](https://github.com/MrMarkus42/cuneiedit/releases)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

CuneiEdit has everything you expect from a full-featured programmer's editor – tabs, sessions,
powerful search, macros, compare, code folding, multiple cursors – in a native app that starts
instantly, stays responsive with files of hundreds of megabytes and never runs anything it
opens.

The name comes from Latin *cunei*, “wedges” – the marks of cuneiform, the oldest known
writing; the icon shows three of them.

> **Status: beta.** CuneiEdit is feature-rich and well tested, but young. Please report
> anything odd in the [issues](https://github.com/MrMarkus42/cuneiedit/issues).

## Contents

- [Why CuneiEdit](#why-cuneiedit)
- [Features](#features)
- [Install](#install)
- [Updates and privacy](#updates-and-privacy)
- [Security](#security)
- [Build from source](#build-from-source)
- [Contributing](#contributing)
- [License](#license)

## Why CuneiEdit

- **Fast** – starts in a moment, opens a 560 MB file with 6 million lines in under a second
  and keeps typing at a few milliseconds per key; single lines of many megabytes (minified
  files) stay smooth.
- **Simple** – everything in one window with the menus you expect; sensible defaults, no
  setup.
- **Secure** – written in memory-safe Rust; opened files are only ever displayed, never
  executed; no telemetry; updates are optional and cryptographically signed; the parsers for
  files, fonts and update information are fuzzed continuously.
- **Yours** – remembers every open document including unsaved changes, even after a crash.

## Features

**Files and sessions**
- Tabs: pin, color, reorder by dragging, many ways to close, reopen closed tabs
- Several windows; files opened from Explorer, Finder or the command line go to the last used
  window
- Open, save, save all, save a copy, rename, reload, move to trash, show in Finder/Explorer
- Restores the last session **including unsaved changes**; named session files
- Notices files changed or deleted by other programs; follow mode (`tail -f`)
- Encodings: UTF-8 (with or without BOM), UTF-16 LE/BE (with or without BOM), Windows-1252, DOS
  code pages 437/850/858/866 and 32 more – detection, "interpret as" and "convert to";
  line endings LF, CR LF, CR
- A folder as workspace with a file tree; optional backup copy on every save

**Editing**
- Multiple cursors and column selection with a column editor
- Code folding and hidden lines, matching brackets and markup tags
- Word, path and call-tip completion; auto-close of brackets and quotes
- Line operations: duplicate, delete, move, split, join, reverse, shuffle, sort (7 orders),
  remove duplicates and empty lines; 8 case conversions; whitespace tools; line and block
  comments; insert date and time
- Macros: record, play, repeat until the end, save by name
- Change history in the margin, bookmarks, five highlight styles

**Search**
- Find and replace with match case, whole word, regular expressions and escape sequences, in
  the selection or all open documents
- Find and replace in files with filters and a results panel
- Go to line or offset, to the matching bracket, to the next change

**Viewing**
- Syntax highlighting for **60 languages** – functions, escape sequences, string
  interpolation, regex literals, JavaScript and CSS inside HTML – plus your own languages
- Light and dark appearance, five color themes, your own font, zoom
- Word wrap, indent guides, edge line, visible whitespace and control characters
- Split view, second document side by side, document map, function list, document list
- Full screen, distraction-free mode, post-it, always on top

**Tools**
- Compare two files side by side with aligned lines and changed characters, or compare with
  the saved version
- Print with the system print dialog (macOS and Windows) and export to PDF or HTML with
  syntax colors; the PDF embeds the editor's fonts, so Unicode prints as shown
- MD5, SHA-1, SHA-256 and SHA-512 checksums; clipboard history; character panel; statistics

**Everywhere**
- Settings dialog including keyboard shortcuts; toolbar
- English and German user interface, following the system language
- macOS: native menu bar, "Open With" and double click in Finder, files dropped on the Dock

## Install

Download the latest version from the
[releases page](https://github.com/MrMarkus42/cuneiedit/releases/latest).

### macOS (Apple Silicon, macOS 11 or later)

1. Download `CuneiEdit-<version>.dmg` and open it.
2. Drag **CuneiEdit** into **Applications**.
3. The first time, open it with **right click → Open** and confirm. (CuneiEdit is not yet
   notarized by Apple, so macOS asks once.) If macOS still refuses, open **System Settings →
   Privacy & Security** and click **Open Anyway**.

Run CuneiEdit from Applications (or `~/Applications`), not from the disk image – only then can
it update itself.

### Windows (64-bit, Windows 10 or later)

Either download `CuneiEdit-<version>-windows-x64.zip`, unpack it to a folder of your choice and
start `CuneiEdit.exe`, or download `CuneiEdit-<version>-windows-x64.exe` and start it directly.
Keep it in a fixed place, e.g. `%LOCALAPPDATA%\Programs\CuneiEdit` (not in Downloads): updates
replace the executable where it is, so "Open with" and default programs keep working, whatever
the file is called.

Windows SmartScreen may warn about an unknown publisher (the executable is not code-signed
yet): click **More info → Run anyway**. CuneiEdit can live in a user folder or in
`C:\Program Files`; updates in protected folders ask for administrator rights (UAC).

### Linux

There are no packages yet; [build from source](#build-from-source). CuneiEdit tells you when a
new version is available.

## Updates and privacy

On the first start CuneiEdit asks whether it may look for new versions. You can change your
answer at any time under **All Settings → Backup & Updates**:

| Setting | What happens |
|---|---|
| Don't look for updates | CuneiEdit never connects to the network. |
| Look daily and ask before installing | Once a day CuneiEdit asks GitHub for the latest version and offers to install it. |
| Look daily and install automatically | The new version is installed in the background and starts with the next launch. |

**Help → Check for Updates…** (on macOS also in the CuneiEdit menu) checks right away.

The only connection is an HTTPS request to `github.com` for the release information; apart
from the request itself (and thus your IP address), nothing is sent. Every update is checked
against CuneiEdit's Ed25519 signature, its version and the size and SHA-256 of the download
before anything is installed. There is no telemetry, no account and no tracking.

## Security

- Written in Rust; `unsafe` code is forbidden except for two small, documented modules for
  the macOS and Windows system calls.
- Documents are only displayed and edited – nothing in a file is executed, no plugins, no
  scripts, no macros inside files.
- Parsers of untrusted input (text files in any encoding, syntax highlighting and export, font
  files, update information) are fuzzed on every change; the lexer is linear-time by design,
  so crafted files cannot freeze the editor.
- Dependencies are checked for known vulnerabilities and licenses on every change and weekly.
- Releases are built and signed by CI only; the macOS app uses the hardened runtime.

Found a vulnerability? Please report it privately – see [SECURITY.md](SECURITY.md).

## Build from source

Requirements: a current stable [Rust toolchain](https://rustup.rs). On Linux also the
development packages for X11/Wayland and GTK, e.g. on Debian/Ubuntu:

```sh
sudo apt-get install libxkbcommon-dev libwayland-dev libgtk-3-dev
```

Then:

```sh
git clone https://github.com/MrMarkus42/cuneiedit.git
cd cuneiedit
cargo run --release -p cuneiedit-app -- [files…]   # start CuneiEdit
cargo test                                       # run the tests
./packaging/macos/build-dmg.sh                   # macOS: CuneiEdit.app and a .dmg in target/dmg
```

The executable is `target/release/cuneiedit` (`cuneiedit.exe` on Windows).

## Contributing

Ideas, bug reports and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) describes
the architecture, the code and security rules, how releases are made, the current state and
the roadmap; [docs/features.md](docs/features.md) lists every feature menu by menu.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in CuneiEdit by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
