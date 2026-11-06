# Pilcrow ¶

A fast, simple and secure text editor for macOS, Windows and Linux, written in Rust.

## Features (0.1)

- Tabs (pin, color, drag to reorder, close variants, restore closed), open/save/save all/save
  a copy, rename, reload, move to trash, show in Finder, drag and drop, files from the command
  line; a folder as workspace (file tree); optional backup copy when saving
- Remembers open files **including unsaved changes** between sessions; manual session files
- Detects files changed or deleted by other programs; monitoring mode (`tail -f`)
- Encodings: UTF-8, UTF-8 BOM, UTF-16 LE/BE (with or without BOM), Windows-1252, DOS code
  pages 437/850/858/866 and 32 more character sets – detection, "interpret as" and
  "convert to"; line endings LF, CR LF, CR
- Line numbers, word wrap, indent guides, edge line, whitespace and control characters,
  matching brackets and markup tags, highlighting of the selected word, auto-close of
  brackets and quotes, word, path and call-tip completion, change history, code folding and
  hidden lines, multiple cursors and column selection with a column editor, split view and a
  second document side by side, document map, read-only mode, zoom, full screen, distraction
  free mode, post-it, always on top
- Find and replace: match case, whole word, regular expressions, extended escapes,
  in selection, wrap around; find all or replace all in a document or all open documents;
  find and replace in files (filters, subfolders) with a filterable results panel (F4);
  go to line or offset
- Five highlight styles (jump to and copy marked text), bookmarks (incl. cut/copy/replace
  bookmarked lines), find characters in a range, select and find next, go to matching
  bracket, go to next change
- Line operations: duplicate, delete, move, split, join, reverse, shuffle, insert blank lines,
  remove (consecutive) duplicates and empty lines, sort in 7 orders; 8 case conversions;
  whitespace operations; line and block comments; redact selection; insert date and time
- Syntax highlighting for 30 languages and formats plus user-defined languages; light and
  dark appearance and five color themes, own font; function list
- Printing (macOS print dialog) and export as PDF or HTML, with syntax colors
- Compare two files side by side: aligned lines, changed characters, navigation, options,
  compare with the saved version
- Macros (record, play, repeat, save by name), MD5/SHA-1/SHA-256/SHA-512 checksums, document
  list, clipboard history, character panel, summary
- Settings dialog including keyboard shortcuts; toolbar with its own icons
- macOS: native menu bar with the usual menus, "Open With" and double click in Finder, files
  dropped on the Dock icon
- User interface in English and German (follows the system language)
- No network access, no telemetry

## Build

Requires a current stable Rust toolchain.

```sh
cargo run -p pilcrow-app -- [files…]
cargo test
./packaging/macos/build-dmg.sh   # Pilcrow.app and target/dmg/Pilcrow-<version>.dmg
```

The disk image is signed ad hoc only and not notarized: on first start, open the app with
right click → Open. It contains an Apple Silicon (arm64) build.

See [CONTRIBUTING.md](CONTRIBUTING.md) for architecture, rules and roadmap.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
