# Pilcrow – rules for contributors

Pilcrow is a cross-platform text editor in Rust (egui/eframe), inspired by the feature set of
Notepad++. Primary platform is macOS; Windows and Linux follow.

## Clean-room rule (licensing)

- **Never copy, translate or closely paraphrase code from Notepad++, Scintilla or other GPL projects.**
  Notepad++ is only a reference for *features and behavior* (menus, dialogs, changelog).
- Implement everything from scratch or with dependencies whose license is allowed in `deny.toml`.

## Architecture

| Crate | Responsibility | May depend on |
|---|---|---|
| `crates/pilcrow-core` | Document model: rope buffer, encodings (incl. own DOS code page tables), line endings, undo (several edits can form one step: `Document::replace_many`), search (`search.rs`), syntax highlighting incl. user-defined languages (`highlight.rs`), code folding ranges (`fold.rs`), line operations (`ops.rs`), case/whitespace/bracket/tag helpers (`transform.rs`), file comparison and change history (`diff.rs`), symbols/completion/call tips/result lines (`code.rs`), find in files, folder listing and path completion (`files.rs`), HTML/PDF export (`export.rs`), checksums (`checksum.rs`) | no UI crates |
| `crates/pilcrow-editor` | egui editor widget: layout of visible lines, word wrap, cursor, selection, multiple cursors and column selection, folding, input, syntax colors and `Palette`, match highlights, smart highlighting, whitespace/indent guides/edge line, bookmarks, gutter marks, brackets and tags, auto-close, macro recording, spacer and folded rows (`rows.rs`), read-only mode | core, egui |
| `crates/pilcrow-macos` | macOS only: files opened from Finder/Dock and quit requests, by adding two methods to winit's application delegate at runtime | objc2 (the only crate with `unsafe`) |
| `crates/pilcrow-app` | Window (`app/`, see its module doc), find bar (`find.rs`), comparison state (`compare.rs`), settings (`settings.rs`), session and backups (`session.rs`), trash and "show in Finder" (`system.rs`), color themes (`themes.rs`), saved macros (`macros.rs`), user languages (`languages.rs`), UI texts (`i18n.rs`), app icon (`app_icon.rs`) | everything |

- In `app/`, every command is a `Command` variant with label and default shortcut in
  `command.rs`; menus are built from these, `run.rs` executes them. New feature = variant +
  label + `run` arm (+ `enabled`/`check` in `menu.rs` if needed; + `CUSTOMIZABLE` if users
  should be able to give it a shortcut). Shortcuts in effect come from `keymap.rs`
  (defaults plus the user's changes in `settings.ini`).

- Logic that does not need a UI belongs in `pilcrow-core` and gets unit tests there.
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

- All user-visible text lives in `crates/pilcrow-app/src/i18n.rs`, defined once per text with
  English and German side by side in the `texts!` macro. No hard-coded UI strings elsewhere.
  Unused texts cause a compiler warning; a test checks that placeholders match.
- Placeholders use `{name}` and are filled with `i18n::fill`.

## Code rules

- `unsafe` is forbidden (`unsafe_code = "forbid"` in the workspace). Approved exception
  (2026-11-06): `crates/pilcrow-macos`, which has its own lints, keeps `unsafe` to the
  Objective-C calls macOS requires; every block has a `// SAFETY:` comment.
- `cargo fmt`, `cargo clippy --all-targets` (no warnings) and `cargo test` must pass.
- Keep comments short; only explain what the code cannot.

## Security rules

- The app makes **no network connections**, has **no telemetry**, runs **no external
  processes** and loads **no code at runtime**. Any change to this needs explicit approval.
  Approved exception (`system.rs`): "Show in Finder" runs the fixed program `/usr/bin/open -R`
  with the file as one argument, no shell (Windows: `explorer.exe /select,`, Linux: `xdg-open`
  on the folder).
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
  platform config directory (macOS: `~/Library/Application Support/Pilcrow`), written
  atomically and parsed defensively: `settings.ini`, `session.ini`, `macros.ini`,
  `languages.ini` and `backup/tab-<n>.txt` (unsaved text; folder mode 0700; names are
  validated so `session.ini` cannot point elsewhere). Also: the font file chosen in the
  settings is read at start (checked for a font header, at most 50 MB); with "backup when
  saving", the old file is copied to `name.bak` next to it or into `saved/` (mode 0700) in
  the config directory before it is overwritten.
- Saved macros contain only keyboard input, copy, cut and paste – never code.
- Find in files runs on a background thread, does not follow symlinked folders and skips
  binary files and files over 64 MB. The clipboard history is kept in memory only.
- Approved dependencies beyond the basics: RustCrypto `md-5`/`sha1`/`sha2` (checksums),
  `jiff` (local date and time, without default features), `muda` (native macOS menu bar,
  macOS only, without GTK), `objc2`/`objc2-app-kit`/`objc2-foundation`/`objc2-pdf-kit` (in
  `pilcrow-macos`; the first three are already used by winit).
- The app runs on its own winit event loop (`main.rs`, `eframe::create_native`) so the macOS
  handlers are installed before launch events arrive.

## Commands

```sh
cargo run -p pilcrow-app -- [files…]   # start Pilcrow
cargo test                             # all tests
cargo clippy --all-targets
cargo fmt --all
./packaging/macos/build-dmg.sh         # .app bundle and .dmg in target/dmg
```

- Icons are drawn in code, no image files: toolbar icons in `app/toolbar.rs` (16 × 16 grid),
  the app icon in `app_icon.rs` (window icon at runtime; `examples/app_icon.rs` writes the
  PNGs for the `.icns` when packaging). The packaging script runs Apple's build tools
  (`iconutil`, `codesign`, `hdiutil`) at build time only.

## Decisions

- **Goal:** an own, complete product (not a Notepad++ port); few dependencies, easy to maintain.
- **Name:** Pilcrow (¶). Availability of crates.io name, domain and trademark not yet checked.
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
  answered with "cancel" and then runs Pilcrow's own quit (AppleScript reports error -128;
  a logout is interrupted once).
- Search scans a full copy of the document; fine up to ~100 MB, a streaming search would scale further.
- Long-line windowing assumes monospace widths for the window offset.
- Comparison does not detect moved blocks (ComparePlus can) and is not restored with the session.
- Move to trash renames into `~/.Trash`: Finder's "Put Back" does not work, other volumes fail;
  not available on Windows (needs the Shell API, i.e. `unsafe` or a crate – decision open).
- No right-to-left text: egui has no bidirectional layout; revisit if egui gains it.
- Printing renders a PDF (built-in Courier, Windows-1252 only; other characters print as
  `?`). On macOS it goes to the system print dialog (PDFKit); elsewhere the PDF is saved.
- Macros record keyboard input only, not menu commands. Mark styles are re-searched after
  edits instead of moving with the text.
- Change history is computed by hashing all lines per edit; disabled above 100 000 lines.
- Folding is heuristic (brackets in comments count; markup is folded by indentation). Folds
  and the second view are not restored with the session.
- With several cursors, each keystroke is its own undo step; auto-close is off.
- User languages are leaked once per definition (a few KB) to keep specs `'static`.
- CI workflow has not run yet; GitHub Actions are pinned by tag, not by commit SHA.

## Measured

- 65 MB / 1 million lines plus a 5 MB single-line file open together: ~260 MB RSS, 0 % idle CPU
  (release build, Apple Silicon).
- Highlighting 51 MB / 1.9 million lines of Rust: 0.59 s for a full pass (spread over frames
  with a budget of 50 000 lines); re-highlighting the visible lines after an edit: ~30 µs.
- Comparing two files with 10 000 / 100 000 / 1 000 000 lines and one change per 1000 lines:
  9 ms / 50 ms / 440 ms. Above 20 000 lines, edits are re-compared after a 0.3 s typing pause.
- Idle with two files open after waves 1 and 2: ~110 MB RSS, 0.4 % CPU (disk check every 2 s).

## Roadmap

The feature checklist against Notepad++ (menu by menu, with status and waves) is
`docs/feature-parity.md` – update it whenever a feature is added.

1. ~~Foundation: workspace, document model, window, tabs, open/save, encodings, line endings~~
2. ~~Editor widget: long lines, context menu, scrolling past the end, whitespace display,
   word wrap~~; IME polish
3. ~~Search menu: find/replace with regex, mark all, count, go to line, find in files~~
4. ~~Syntax highlighting for 30 languages, syntax menu, light/dark themes~~
5. ~~Comfort: session restore with backups, recent files, settings, zoom, line operations,
   case conversion, comments, bookmarks, split view~~
6. Advanced: ~~compare files, macros, auto-completion, function list, checksums, code folding,
   column mode / multi-cursor~~; moved-block detection in compare
   (waves 1–3 of `docs/feature-parity.md` are done except RTL and the Windows trash)
7. Distribution: ~~icon, `.app` bundle and `.dmg` (ad hoc signed, arm64)~~; Developer ID
   signing and notarization, universal binary (Intel), Windows and Linux packages
