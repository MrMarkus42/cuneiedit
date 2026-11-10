# CuneiEdit – rules for contributors

CuneiEdit is a cross-platform text editor in Rust (egui/eframe) with the feature set of a
full programmer's editor. Primary platform is macOS; Windows and Linux follow.

## Clean-room rule (licensing)

- **Never copy, translate or closely paraphrase code or texts from other editors or GPL
  projects.** Other editors may only serve as a reference for *features and behavior*.
- Never name other editors or their plugins in code, docs, commit messages or release notes.
- Implement everything from scratch or with dependencies whose license is allowed in `deny.toml`.

## Architecture

| Crate | Responsibility | May depend on |
|---|---|---|
| `crates/cuneiedit-core` | Document model: rope buffer, encodings (incl. own DOS code page tables), line endings, undo (several edits can form one step: `Document::replace_many`), search (`search.rs`), syntax highlighting incl. user-defined languages (`highlight.rs`), code folding ranges (`fold.rs`), line operations (`ops.rs`), case/whitespace/bracket/tag helpers (`transform.rs`), file comparison and change history (`diff.rs`), symbols/completion/call tips/result lines (`code.rs`), find in files, folder listing and path completion (`files.rs`), HTML/PDF export (`export.rs`) with a TrueType reader for embedding fonts (`font.rs`), checksums (`checksum.rs`) | no UI crates |
| `crates/cuneiedit-editor` | egui editor widget: layout of visible lines, word wrap, cursor, selection, multiple cursors and column selection, folding, input, syntax colors and `Palette`, match highlights, smart highlighting, whitespace/indent guides/edge line, bookmarks, gutter marks, brackets and tags, auto-close, macro recording, spacer and folded rows (`rows.rs`), read-only mode | core, egui |
| `crates/cuneiedit-macos` | macOS only: files opened from Finder/Dock and quit requests, by adding two methods to winit's application delegate at runtime; the print dialog (PDFKit) | objc2 (`unsafe` allowed) |
| `crates/cuneiedit-windows` | Windows only: print dialog and printing with GDI; the named pipe for single instance (`instance.rs`) | windows-sys (`unsafe` allowed) |
| `crates/cuneiedit-update` | Updates: signed manifest (`manifest.rs`), check and download via the system's `curl`, installation per platform (`install.rs`); the release tool `cuneiedit-release` (key generation, manifest, signing; not shipped) | ed25519-compact, sha2 |
| `crates/cuneiedit-app` | Window (`app/`, see its module doc), find bar (`find.rs`), comparison state (`compare.rs`), settings (`settings.rs`), session and backups (`session.rs`), single instance (`instance.rs`), trash and "show in Finder" (`system.rs`), color themes (`themes.rs`), saved macros (`macros.rs`), user languages (`languages.rs`), UI texts (`i18n.rs`), app icon (`app_icon.rs`; `icon_files.rs` writes PNG and Windows `.res` files for `build.rs` and the example) | everything |

- In `app/`, every command is a `Command` variant with label and default shortcut in
  `command.rs`; menus are built from these, `run.rs` executes them. New feature = variant +
  label + `run` arm (+ `enabled`/`check` in `menu.rs` if needed; + `CUSTOMIZABLE` if users
  should be able to give it a shortcut). Shortcuts in effect come from `keymap.rs`
  (defaults plus the user's changes in `settings.ini`).

- Logic that does not need a UI belongs in `cuneiedit-core` and gets unit tests there.
- Positions are **char indices** into the rope; the buffer always uses `\n`. The on-disk line
  ending and encoding are applied only when saving.
- The editor lays out **only visible lines**. Never do per-frame work proportional to file size.
- Syntax highlighting is an own line-based lexer (`highlight.rs`, one `Spec` per language) with
  cached line start states, invalidated via `Document::first_change_since`. New language =
  new `Spec` entry plus a test.
- Every line operation in `ops.rs` is exactly one undo step.
- Several cursors edit through `EditorState::replace_each`, which uses
  `Document::replace_many`: one undo step, linked edits in the history.
- Folding: `fold.rs` finds blocks by unclosed brackets or deeper indentation, cheaply per
  visible line; folds are `(header line, hidden lines)` in the editor state and enter the
  `RowMap` as hidden lines. Folds follow edits like bookmarks.
- Comparison (`diff.rs`) is Myers' O(ND) diff in linear space with an edit-distance budget;
  a property test checks minimality against an LCS reference. The editor aligns both sides
  with spacer rows (`RowMap` maps lines to visual rows – reuse it for word wrap).

## UI texts

- All user-visible text lives in `crates/cuneiedit-app/src/i18n.rs`, defined once per text with
  English and German side by side in the `texts!` macro. No hard-coded UI strings elsewhere.
  Unused texts cause a compiler warning; a test checks that placeholders match.
- Placeholders use `{name}` and are filled with `i18n::fill`.

## Code rules

- `unsafe` is forbidden (`unsafe_code = "forbid"` in the workspace). Approved exceptions,
  each with its own lints and a `// SAFETY:` comment on every block: `crates/cuneiedit-macos`
  (2026-11-06) for the Objective-C calls macOS requires, and `crates/cuneiedit-windows`
  (2026-11-09) for the Win32 print dialog and GDI printing (`windows-sys`, Microsoft's raw
  bindings, already used by winit), and since 2026-11-10 the single-instance named pipe.
- `cargo fmt`, `cargo clippy --all-targets` (no warnings) and `cargo test` must pass.
- Keep comments short; only explain what the code cannot.

## Security rules

- The app makes **no network connections**, has **no telemetry**, runs **no external
  processes** and loads **no code at runtime**. Any change to this needs explicit approval.
  Approved exception (`system.rs`): "Show in Finder" runs the fixed program `/usr/bin/open -R`
  with the file as one argument, no shell (Windows: `explorer.exe /select,`, Linux: `xdg-open`
  on the folder).
  Approved exception (2026-11-09, `cuneiedit-update`): updates. Only if the user chose it (asked
  on the first start, changeable in the settings) or via "Check for Updates…". HTTPS only, to
  `github.com` and its redirects, through the system's `curl` (fixed path, `--disable` so no
  `.curlrc`, `=https` also for redirects, size and time limits). Nothing is installed unless
  `update.txt` is signed with a key in `PUBLIC_KEYS`, its version is newer and the file matches
  its size and SHA-256. Installing runs fixed system programs with paths as arguments, never a
  shell string: macOS `ditto` (unpack), `osascript` with a constant script for the admin
  password if the folder is not writable, `open -n` to restart; Windows renames the running
  `.exe` (removed on the next start) or, in protected folders, runs the installed `.exe`
  elevated (UAC) via PowerShell `Start-Process -Verb RunAs` as a helper that only replaces
  itself and re-checks the hash. Linux only shows the release page.
  Approved exception (2026-11-10, `instance.rs`): single instance. A second start hands its
  file paths to the running instance and exits; nothing else is accepted. The channel is
  local only and limited to the user: a Unix domain socket in a folder with mode 0700 (Linux:
  `$XDG_RUNTIME_DIR/cuneiedit/`, otherwise `instance/` in the config directory), or on
  Windows a named pipe whose access list allows only the user's SID, that rejects remote
  clients and whose server process the client checks to run as the same user. Messages are
  limited to 1 MB and 1000 absolute paths and parsed defensively (tested); the paths are
  opened like files dropped on the window.
- Before adding a dependency, check: maintenance and popularity, license, `unsafe` usage,
  `build.rs` and proc macros (they run at build time). Prefer the standard library or a few
  lines of own code over small crates.
- Only crates.io is allowed as source (no git dependencies).
- Always commit `Cargo.lock`. Update dependencies deliberately, never as a side effect.
- After every dependency change run:

  ```sh
  cargo deny check   # licenses, sources, advisories, bans
  cargo audit        # RustSec vulnerability database
  ```

  Every ignored advisory in `deny.toml` needs a reason and is re-checked on updates.
- Files are read and written only on explicit user action (open, save, drag and drop, find/
  replace in files after confirmation, move to trash after confirmation, export, listing a
  folder the user opened as workspace, path completion on ⌃Space). Exceptions, all in the
  platform config directory (macOS: `~/Library/Application Support/CuneiEdit`), written
  atomically and parsed defensively: `settings.ini`, `session.ini`, `macros.ini`,
  `languages.ini` and `backup/tab-<n>.txt` (unsaved text; folder mode 0700; names are
  validated so `session.ini` cannot point elsewhere). Also: the font file chosen in the
  settings is read at start (checked for a font header, at most 50 MB); with "backup when
  saving", the old file is copied to `name.bak` next to it or into `saved/` (mode 0700) in
  the config directory before it is overwritten.
- Saved macros contain only keyboard input, copy, cut and paste – never code.
- Printing embeds only fonts the editor has already loaded; `font.rs` parses them with checked
  reads, so a damaged font is skipped instead of crashing (tested with truncated and garbage
  data).
- File contents are untrusted: the lexer looks ahead at most `MAX_SPAN` chars for spans and
  lexes huge lines only in a window, so crafted lines cannot make it quadratic (tested).
- Find in files runs on a background thread, does not follow symlinked folders and skips
  binary files and files over 64 MB. The clipboard history is kept in memory only.
- Approved dependencies beyond the basics: RustCrypto `md-5`/`sha1`/`sha2` (checksums),
  `jiff` (local date and time, without default features), `muda` (native macOS menu bar,
  macOS only, without GTK), `objc2`/`objc2-app-kit`/`objc2-foundation`/`objc2-pdf-kit` (in
  `cuneiedit-macos`; the first three are already used by winit), `ed25519-compact` (update
  signatures; pure Rust, no dependencies, without default features).
- **Every release is signed.** Releases are only made by pushing a tag `v<version>` (equal to
  the version in `Cargo.toml`); `.github/workflows/release.yml` builds macOS and Windows,
  writes `update.txt` and signs it with the secret `CUNEIEDIT_UPDATE_KEY`, and fails without it.
  Never create or edit releases by hand. The secret key is kept offline by the owner; losing
  it means old versions can no longer update. To change keys, add the new public key to
  `PUBLIC_KEYS` one release before signing with it.
- GitHub Actions are pinned by commit SHA (version as comment; Dependabot updates them);
  checkouts do not keep credentials.
- Parsers of untrusted data are fuzzed (`fuzz/`, cargo-fuzz with libFuzzer and
  AddressSanitizer): opening a file (`open`), highlighting and export (`highlight`), font
  files (`font`), update information (`manifest`). CI fuzzes each target for 2 minutes per
  push and 20 minutes weekly. Every parser for new untrusted input gets a fuzz target.
- The macOS app is signed with the hardened runtime. With the repository secrets
  `APPLE_CERTIFICATE_P12` (Developer ID Application certificate as base64 `.p12`),
  `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGN_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID` and
  `APPLE_APP_PASSWORD` (app-specific password), releases are signed with Developer ID and
  notarized; without them the app is signed ad hoc.
- The app runs on its own winit event loop (`main.rs`, `eframe::create_native`) so the macOS
  handlers are installed before launch events arrive.

## Commands

```sh
cargo run -p cuneiedit-app -- [files…]   # start CuneiEdit
cargo test                             # all tests
cargo clippy --all-targets
cargo fmt --all
(cd fuzz && cargo +nightly fuzz run open -- -max_total_time=60)   # fuzz one target (nightly)
./packaging/macos/build-dmg.sh         # .app bundle and .dmg in target/dmg
git tag -a v0.3.0 -m "Release notes" && git push origin v0.3.0   # signed release via CI
```

- Open `cuneiedit.code-workspace` in VS Code: recommended extensions, clippy on save, tasks for
  running (also with a throwaway test profile), "Check all (before committing)", building the
  DMG and fuzzing, and a debug configuration.

- Icons are drawn in code, no image files: toolbar icons in `app/toolbar.rs` (16 × 16 grid),
  the app icon in `app_icon.rs` (window icon at runtime; `examples/app_icon.rs` writes the
  PNGs for the `.icns` when packaging; on Windows `build.rs` embeds it into the `.exe` as a
  resource, which Explorer and the taskbar show). The packaging script runs Apple's build tools
  (`iconutil`, `codesign`, `hdiutil`) at build time only.

## Decisions

- **Goal:** an own, complete product (not a port of another editor); few dependencies, easy to maintain.
- **Name:** CuneiEdit, from Latin *cunei* (wedges, as in cuneiform); the icon shows three
  wedges. The crates.io names and the `.com`/`.org` domains were free on 2026-11-09;
  trademarks are not yet checked. Bundle id `io.github.mrmarkus42.cuneiedit`.
- **License:** MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Keep all dependencies permissive.
- **GUI:** egui/eframe 0.37 with the `glow` renderer and an own editor widget (egui's `TextEdit`
  does not scale). The core stays UI-independent so the GUI could be replaced.
- **Platforms:** macOS first; the code must stay portable to Windows and Linux.
- **Priorities:** syntax highlighting and the search menu; plugins are lowest priority.
- **Highlighting:** own lexer instead of `syntect`/tree-sitter – no dependencies, no grammar
  licenses, fast enough (see Measured). Revisit only if folding needs real parse trees.
- **Settings:** own `key=value` file instead of serde/ron, to avoid dependencies.
- **Languages:** discussions in German; code, comments and commit messages in English.

## Known gaps

- The native macOS menu bar (`app/mac_menu.rs`) has only the usual menus with the main
  commands; the complete menus are in the window. Its shortcuts take the keys before egui,
  so edit items are passed on as input events. A quit request from the Dock or system is
  answered with "cancel" and then runs CuneiEdit's own quit (AppleScript reports error -128;
  a logout is interrupted once).
- Search scans a full copy of the document; fine up to ~100 MB, a streaming search would scale further.
- Long-line windowing assumes monospace widths for the window offset.
- Comparison does not detect moved blocks (dedicated compare tools can) and is not restored with the session.
- Move to trash renames into `~/.Trash`: Finder's "Put Back" does not work, other volumes fail;
  not available on Windows (needs the Shell API, i.e. `unsafe` or a crate – decision open).
- No right-to-left text: egui has no bidirectional layout; revisit if egui gains it.
- Printing renders a PDF that embeds the editor's TrueType fonts whole (no subsetting, ~300 KB
  per font; CFF/OpenType-only fonts are skipped). Characters none of them has print as a box.
  On macOS it goes to the system print dialog (PDFKit). On Windows the system print dialog
  chooses printer, pages and copies, and the text is printed with GDI (Consolas, syntax
  colors, same page layout as the PDF via `export::pages`); not yet tested on many printers.
  On Linux the PDF is saved.
- Lines over 20 000 chars are highlighted only around the shown columns, starting in a neutral
  state there, and wrap at a fixed width instead of at words.
- With word wrap on, a change of the window width re-wraps every line (~4 s when all 6 million
  lines of a 560 MB file wrap), and typing then costs ~0.1 s per keystroke.
- Macros record keyboard input only, not menu commands. Mark styles are re-searched after
  edits instead of moving with the text.
- Change history is computed by hashing all lines per edit; disabled above 100 000 lines or
  10 million chars.
- Folding is heuristic (brackets in comments count; markup is folded by indentation). Folds
  and the second view are not restored with the session.
- With several cursors, each keystroke is its own undo step; auto-close is off.
- User languages are leaked once per definition (a few KB) to keep specs `'static`.
- Windows: all in one process (`app/windows.rs`). The main window is eframe's root
  viewport, the others are immediate viewports; per-window state is declared in the
  `app_state!` macro in `app/mod.rs` and swapped into `CuneiEditApp` while a window is shown,
  so all code works on "the current window". Closing the main window while others are open
  moves the first other window into it. Window positions are not saved in the session, and
  tabs cannot be dragged between windows ("Move to New Window" only).
- The Windows `.exe` icon resource and the named pipe were only checked by building for
  Windows; the single instance and the windows were tested on macOS.
- Not notarized yet (needs the Apple Developer Program and the secrets above): Gatekeeper warns
  on the first start. Settings and session files (`cuneiedit-app`) are not fuzzed yet.

## Measured

- 65 MB / 1 million lines plus a 5 MB single-line file open together: ~260 MB RSS, 0 % idle CPU
  (release build, Apple Silicon).
- Highlighting 51 MB / 1.9 million lines of Rust: 0.59 s for a full pass (spread over frames
  with a budget of 50 000 lines); re-highlighting the visible lines after an edit: ~30 µs.
- Comparing two files with 10 000 / 100 000 / 1 000 000 lines and one change per 1000 lines:
  9 ms / 50 ms / 440 ms. Above 20 000 lines, edits are re-compared after a 0.3 s typing pause.
- Idle with two files open after waves 1 and 2: ~110 MB RSS, 0.4 % CPU (disk check every 2 s).
- 560 MB / 6 million lines of Rust: opens in ~0.7 s, ~1 GB footprint, 0 % idle CPU, ~18 ms per
  keystroke with word wrap on. A 72 MB single-line JSON file: ~20 ms per keystroke (lexing
  the whole line took ~230 ms per frame before huge lines were windowed).

## Roadmap

The feature checklist (menu by menu, with status and waves) is
`docs/features.md` – update it whenever a feature is added.

1. ~~Foundation: workspace, document model, window, tabs, open/save, encodings, line endings~~
2. ~~Editor widget: long lines, context menu, scrolling past the end, whitespace display,
   word wrap~~; IME polish
3. ~~Search menu: find/replace with regex, mark all, count, go to line, find in files~~
4. ~~Syntax highlighting for 60 languages, syntax menu, light/dark themes~~
5. ~~Comfort: session restore with backups, recent files, settings, zoom, line operations,
   case conversion, comments, bookmarks, split view~~
6. Advanced: ~~compare files, macros, auto-completion, function list, checksums, code folding,
   column mode / multi-cursor~~; moved-block detection in compare
   (waves 1–3 of `docs/features.md` are done except RTL and the Windows trash)
7. Distribution: ~~icon, `.app` bundle and `.dmg` (ad hoc signed, arm64), hardened runtime,
   notarization in the release workflow~~; Apple Developer ID (to enable notarization),
   universal binary (Intel), Windows installer and Linux packages

## Current state and next steps

Keep this section up to date at the end of every working session, so work can continue
seamlessly from here.

**State (2026-11-09):** version 0.3.0 (beta) is the first release under the name CuneiEdit
(DMG, macOS ZIP, Windows EXE and ZIP, signed `update.txt`): all features of
`docs/features.md` waves 1–3, 60 languages, Unicode printing, Windows print dialog,
large-file performance, signed updates, fuzzing, hardened runtime. CI is green on macOS,
Windows and Linux. Earlier releases were removed; there is no update path from them.
Private vulnerability reporting is enabled on GitHub; `SECURITY.md` describes it.
Since 0.3.0 (not released yet): Windows `.exe` icon, single instance (files from Explorer,
Finder or the command line go to the running process), several windows ("New Window",
"Move to New Window"; restored with the session).

**Next steps, in order:**

1. Test updates for real with a small v0.3.1: on macOS (in `/Applications` as admin and as a
   standard user, and in `~/Applications`) and on a real Windows PC (user folder and
   `C:\Program Files` with UAC). Also try CuneiEdit on Windows in general (typing, saving,
   printing with the new print dialog on a real printer) and check the `.exe` icon in
   Explorer and the taskbar, double clicks on files while CuneiEdit runs, and several
   windows (new window, move a tab, close the main window, quit with unsaved documents).
2. Look at the things never checked visually: print dialogs, new highlighting colors, update
   dialog and first-start question.
3. Check the trademark before announcing CuneiEdit widely.
4. Apple Developer Program: create the Developer ID certificate and set the six `APPLE_*`
   secrets (see Security rules) – then releases are notarized automatically. Optionally a
   Windows code-signing certificate (SmartScreen).
5. Later: fuzz settings and session parsing, Intel/universal macOS build, Windows installer,
   Linux packages, screenshots for the README, moved-block detection in compare.

**Working conventions:** discuss in German, write code, comments, commits and docs in
English. Docs and code never mention who or what wrote them. Ask before pushing, before new
dependencies and before anything that weakens the security rules; run `cargo fmt`,
`cargo clippy --all-targets` (zero warnings), `cargo test`, `cargo deny check` and
`cargo audit` before every commit. Never run test instances of the app with the real
profile: start them with `HOME=<temp dir>` so `settings.ini` and `session.ini` stay
untouched. The release signing key is kept by the owner outside the repository.
