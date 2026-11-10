//! User interface texts. Every text is defined once, with all languages side by side;
//! the compiler enforces that none is missing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::German];

    pub fn from_system() -> Self {
        match sys_locale::get_locale() {
            Some(locale) if locale.to_lowercase().starts_with("de") => Self::German,
            _ => Self::English,
        }
    }

    /// Name of the language in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }

    pub fn texts(self) -> &'static Texts {
        match self {
            Self::English => &EN,
            Self::German => &DE,
        }
    }
}

/// Replaces `{name}` placeholders in `template`.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

macro_rules! texts {
    ($($(#[$meta:meta])* $name:ident: $en:literal, $de:literal;)*) => {
        pub struct Texts {
            $($(#[$meta])* pub $name: &'static str,)*
        }

        const EN: Texts = Texts { $($name: $en,)* };
        const DE: Texts = Texts { $($name: $de,)* };

        /// Every text in English and German, to check placeholders.
        #[cfg(test)]
        const PAIRS: &[(&str, &str)] = &[$(($en, $de),)*];
    };
}

texts! {
    menu_file: "File", "Datei";
    new: "New", "Neu";
    new_window: "New Window", "Neues Fenster";
    open: "Open…", "Öffnen…";
    reveal_finder: "Show in Finder", "Im Finder zeigen";
    reveal_other: "Show in File Manager", "Im Dateimanager zeigen";
    reload: "Reload from Disk", "Von Festplatte neu laden";
    recent_files: "Recent Files", "Zuletzt geöffnet";
    open_all_recent: "Open All", "Alle öffnen";
    clear_recent: "Clear List", "Liste leeren";
    restore_closed: "Restore Recently Closed File", "Zuletzt geschlossene Datei wiederherstellen";
    save: "Save", "Speichern";
    save_as: "Save As…", "Speichern unter…";
    save_copy: "Save a Copy As…", "Kopie speichern unter…";
    save_all: "Save All", "Alle speichern";
    rename: "Rename…", "Umbenennen…";
    close: "Close", "Schließen";
    close_all: "Close All", "Alle schließen";
    close_more: "Close Multiple Documents", "Mehrere Dokumente schließen";
    close_others: "Close All but Active Document", "Alle außer dem aktiven schließen";
    close_left: "Close All to the Left", "Alle links davon schließen";
    close_right: "Close All to the Right", "Alle rechts davon schließen";
    close_unchanged: "Close All Unchanged", "Alle unveränderten schließen";
    move_to_trash: "Move to Trash…", "In den Papierkorb legen…";
    load_session: "Load Session…", "Sitzung laden…";
    save_session: "Save Session…", "Sitzung speichern…";
    open_folder: "Open Folder as Workspace…", "Ordner als Arbeitsbereich öffnen…";
    close_folder: "Close Workspace", "Arbeitsbereich schließen";
    export_html: "Export as HTML…", "Als HTML exportieren…";
    export_pdf: "Export as PDF…", "Als PDF exportieren…";
    print: "Print…", "Drucken…";
    quit: "Quit", "Beenden";

    menu_edit: "Edit", "Bearbeiten";
    undo: "Undo", "Rückgängig";
    redo: "Redo", "Wiederholen";
    cut: "Cut", "Ausschneiden";
    copy: "Copy", "Kopieren";
    paste: "Paste", "Einfügen";
    select_all: "Select All", "Alles auswählen";
    insert_date: "Insert Date and Time", "Datum und Uhrzeit einfügen";
    date_short: "Short", "Kurz";
    date_long: "Long", "Lang";
    date_custom: "Custom Format", "Eigenes Format";
    copy_to_clipboard: "Copy to Clipboard", "In die Zwischenablage kopieren";
    copy_full_path: "Full File Path", "Vollständiger Dateipfad";
    copy_file_name: "File Name", "Dateiname";
    copy_directory: "Directory Path", "Ordnerpfad";
    indent: "Indent", "Einzug";
    increase_indent: "Increase Line Indent", "Einzug vergrößern";
    decrease_indent: "Decrease Line Indent", "Einzug verkleinern";
    convert_case: "Convert Case", "Groß-/Kleinschreibung";
    uppercase: "UPPERCASE", "GROSSBUCHSTABEN";
    lowercase: "lowercase", "kleinbuchstaben";
    proper_case: "Proper Case", "Wörter Groß Schreiben";
    proper_blend: "Proper Case (Blend)", "Wörter groß (Großbuchstaben behalten)";
    sentence_case: "Sentence case", "Satzanfang groß";
    sentence_blend: "Sentence case (blend)", "Satzanfang groß (Großbuchstaben behalten)";
    invert_case: "iNVERT cASE", "uMKEHREN";
    random_case: "ranDOm CasE", "zUfäLLiG";
    line_operations: "Line Operations", "Zeilenoperationen";
    duplicate_line: "Duplicate Current Line", "Zeile duplizieren";
    delete_line: "Delete Current Line", "Zeile löschen";
    move_line_up: "Move Up Current Line", "Zeile nach oben";
    move_line_down: "Move Down Current Line", "Zeile nach unten";
    remove_duplicates: "Remove Duplicate Lines", "Doppelte Zeilen entfernen";
    remove_consecutive_duplicates: "Remove Consecutive Duplicate Lines", "Aufeinanderfolgende doppelte Zeilen entfernen";
    split_lines: "Split Lines at Edge Column", "Zeilen an der Randspalte umbrechen";
    join_lines: "Join Lines", "Zeilen verbinden";
    blank_above: "Insert Blank Line Above Current", "Leerzeile darüber einfügen";
    blank_below: "Insert Blank Line Below Current", "Leerzeile darunter einfügen";
    remove_empty: "Remove Empty Lines", "Leere Zeilen entfernen";
    remove_blank: "Remove Empty Lines (Containing Blank Characters)", "Leere Zeilen entfernen (auch mit Leerzeichen)";
    reverse_lines: "Reverse Line Order", "Zeilenreihenfolge umkehren";
    shuffle_lines: "Randomize Line Order", "Zeilen zufällig anordnen";
    sort_lines: "Sort Lines", "Zeilen sortieren";
    /// `{order}` is how lines are compared, `{direction}` ascending or descending.
    sort_item: "{order}, {direction}", "{order}, {direction}";
    ascending: "ascending", "aufsteigend";
    descending: "descending", "absteigend";
    sort_lexicographic: "Lexicographically", "Lexikografisch";
    sort_ignore_case: "Lexicographically ignoring case", "Lexikografisch ohne Groß-/Kleinschreibung";
    sort_locale: "In locale order", "Nach Sprachregeln";
    sort_length: "By length", "Nach Länge";
    sort_integer: "As integers", "Als Ganzzahlen";
    sort_decimal_comma: "As decimals (comma)", "Als Dezimalzahlen (Komma)";
    sort_decimal_dot: "As decimals (dot)", "Als Dezimalzahlen (Punkt)";
    /// `{line}` is a line number.
    sort_failed: "Line {line} does not start with a number.", "Zeile {line} beginnt nicht mit einer Zahl.";
    comment_menu: "Comment/Uncomment", "Kommentieren";
    toggle_comment: "Toggle Single Line Comment", "Zeilenkommentar ein/aus";
    line_comment: "Single Line Comment", "Zeilenkommentar";
    line_uncomment: "Single Line Uncomment", "Zeilenkommentar entfernen";
    block_comment: "Block Comment", "Blockkommentar";
    block_uncomment: "Block Uncomment", "Blockkommentar entfernen";
    blank_operations: "Blank Operations", "Leerraum";
    trim_trailing: "Trim Trailing Space", "Leerzeichen am Zeilenende entfernen";
    trim_leading: "Trim Leading Space", "Leerzeichen am Zeilenanfang entfernen";
    trim_both: "Trim Leading and Trailing Space", "Leerzeichen am Anfang und Ende entfernen";
    eol_to_space: "EOL to Space", "Zeilenumbrüche durch Leerzeichen ersetzen";
    trim_eol_to_space: "Trim Both and EOL to Space", "Kürzen und Zeilenumbrüche ersetzen";
    tabs_to_spaces: "TAB to Space", "Tabs in Leerzeichen";
    spaces_to_tabs_all: "Space to TAB (All)", "Leerzeichen in Tabs (alle)";
    spaces_to_tabs_leading: "Space to TAB (Leading)", "Leerzeichen in Tabs (am Zeilenanfang)";
    redact: "Redact Selection", "Auswahl schwärzen";
    read_only: "Read-Only", "Schreibgeschützt";
    complete: "Word Completion", "Wortvervollständigung";
    multi_editing: "Multi-Editing", "Mehrfachbearbeitung";
    add_cursor_above: "Add Cursor Above", "Cursor darüber hinzufügen";
    add_cursor_below: "Add Cursor Below", "Cursor darunter hinzufügen";
    multi_editing_hint: "Alt+drag selects columns, ⌘/Ctrl+click adds a cursor, Esc keeps one.", "Alt+Ziehen wählt Spalten, ⌘/Strg+Klick fügt einen Cursor hinzu, Esc lässt einen übrig.";
    column_editor: "Column Editor…", "Spalteneditor…";
    column_text: "Text to insert", "Einzufügender Text";
    column_numbers: "Numbers to insert", "Einzufügende Zahlen";
    column_initial: "Initial number", "Startwert";
    column_increment: "Increase by", "Erhöhen um";
    column_repeat: "Repeat", "Wiederholen";
    column_leading_zeros: "Leading zeros", "Führende Nullen";
    column_format: "Format", "Format";
    format_dec: "Dec", "Dez";
    format_hex: "Hex", "Hex";
    format_oct: "Oct", "Okt";
    format_bin: "Bin", "Bin";
    column_editor_help: "Inserts at every cursor, or without several cursors in every line from the cursor down, at the cursor's column.", "Fügt an jedem Cursor ein, ohne mehrere Cursor in jeder Zeile ab dem Cursor abwärts, in der Spalte des Cursors.";

    menu_search: "Search", "Suchen";
    find: "Find…", "Suchen…";
    replace: "Replace…", "Ersetzen…";
    find_next: "Find Next", "Weitersuchen";
    find_previous: "Find Previous", "Rückwärts suchen";
    select_find_next: "Select and Find Next", "Auswählen und weitersuchen";
    select_find_previous: "Select and Find Previous", "Auswählen und rückwärts suchen";
    find_all: "Find All in Current Document", "Alle im aktuellen Dokument finden";
    find_all_tabs: "Find All in All Open Documents", "Alle in allen offenen Dokumenten finden";
    find_in_files: "Find in Files…", "In Dateien suchen…";
    go_to: "Go To…", "Gehe zu…";
    go_line: "Line", "Zeile";
    go_offset: "Offset", "Position";
    find_label: "Find", "Suchen";
    replace_label: "Replace with", "Ersetzen durch";
    match_case: "Match case", "Groß-/Kleinschreibung beachten";
    whole_word: "Whole word", "Nur ganze Wörter";
    regex: "Regular expression", "Regulärer Ausdruck";
    extended: "Extended (\\n, \\r, \\t, \\0, \\x…)", "Erweitert (\\n, \\r, \\t, \\0, \\x…)";
    dot_newline: ". matches newline", ". findet Zeilenumbruch";
    in_selection: "In selection", "In Auswahl";
    wrap_around: "Wrap around", "Am Ende von vorne beginnen";
    replace_one: "Replace", "Ersetzen";
    replace_all: "Replace All", "Alle ersetzen";
    find_all_button: "Find All", "Alle finden";
    all_documents: "All Documents", "Alle Dokumente";
    /// `{current}` and `{total}` are match numbers.
    match_position: "{current} of {total}", "{current} von {total}";
    /// `{total}` is the number of matches.
    match_count: "{total} matches", "{total} Treffer";
    no_matches: "No matches", "Keine Treffer";
    /// `{count}` is the number of replacements.
    replaced: "{count} replaced", "{count} ersetzt";
    invalid_regex: "Invalid regular expression", "Ungültiger regulärer Ausdruck";
    /// `{max}` is the last line number.
    line_number: "Line number (1–{max})", "Zeilennummer (1–{max})";
    /// `{max}` is the last offset.
    offset_number: "Character offset (0–{max})", "Zeichenposition (0–{max})";
    go: "Go", "Gehe zu";
    matching_brace: "Go to Matching Brace", "Zur passenden Klammer";
    select_braces: "Select All Between Matching Braces", "Alles zwischen passenden Klammern auswählen";
    mark_menu: "Style All Occurrences of Token", "Alle Vorkommen hervorheben";
    /// `{n}` is a style number.
    mark_style: "Using Style {n}", "Mit Stil {n}";
    clear_style_menu: "Clear Style", "Hervorhebung entfernen";
    clear_all_styles: "Clear All Styles", "Alle Hervorhebungen entfernen";
    find_chars: "Find Characters in Range…", "Zeichen in Bereich suchen…";
    non_ascii: "Non-ASCII characters", "Nicht-ASCII-Zeichen";
    ascii: "ASCII characters (0–127)", "ASCII-Zeichen (0–127)";
    custom_range: "Custom range (decimal)", "Eigener Bereich (dezimal)";
    not_found: "Not found", "Nicht gefunden";
    results: "Search Results", "Suchergebnisse";
    next_result: "Next Search Result", "Nächstes Suchergebnis";
    previous_result: "Previous Search Result", "Vorheriges Suchergebnis";
    results_filter: "Filter results", "Ergebnisse filtern";
    next_mark: "Jump to Next Marked Text", "Zur nächsten Hervorhebung springen";
    previous_mark: "Jump to Previous Marked Text", "Zur vorherigen Hervorhebung springen";
    copy_marked: "Copy Marked Text", "Hervorgehobenen Text kopieren";
    next_change: "Go to Next Change", "Zur nächsten Änderung";
    previous_change: "Go to Previous Change", "Zur vorherigen Änderung";
    replace_all_docs: "In All Documents", "In allen Dokumenten";
    /// `{count}` and `{files}` are numbers.
    replaced_in_docs: "{count} replaced in {files} documents", "{count} ersetzt in {files} Dokumenten";
    /// `{hits}` and `{files}` are numbers.
    results_summary: "{hits} hits in {files} files", "{hits} Treffer in {files} Dateien";
    /// `{files}` is the number of files searched so far.
    searching: "Searching… {files} files", "Suche… {files} Dateien";
    directory: "Directory", "Ordner";
    browse: "Browse…", "Durchsuchen…";
    filters: "Filters", "Filter";
    subfolders: "In all subfolders", "In allen Unterordnern";
    hidden_folders: "In hidden files and folders", "In versteckten Dateien und Ordnern";
    replace_in_files: "Replace in Files", "In Dateien ersetzen";
    /// `{dir}` is a folder.
    confirm_replace_files: "Replace all matches in all matching files in “{dir}”? This cannot be undone.", "Alle Treffer in allen passenden Dateien in „{dir}“ ersetzen? Das kann nicht rückgängig gemacht werden.";
    /// `{count}` and `{files}` are numbers.
    replaced_in_files: "{count} replaced in {files} files", "{count} ersetzt in {files} Dateien";

    bookmarks: "Bookmarks", "Lesezeichen";
    toggle_bookmark: "Toggle Bookmark", "Lesezeichen setzen/entfernen";
    next_bookmark: "Next Bookmark", "Nächstes Lesezeichen";
    previous_bookmark: "Previous Bookmark", "Vorheriges Lesezeichen";
    clear_bookmarks: "Clear All Bookmarks", "Alle Lesezeichen entfernen";
    invert_bookmarks: "Inverse Bookmarks", "Lesezeichen umkehren";
    copy_bookmarked: "Copy Bookmarked Lines", "Zeilen mit Lesezeichen kopieren";
    cut_bookmarked: "Cut Bookmarked Lines", "Zeilen mit Lesezeichen ausschneiden";
    paste_bookmarked: "Replace Bookmarked Lines with Clipboard", "Zeilen mit Lesezeichen durch Zwischenablage ersetzen";
    remove_bookmarked: "Remove Bookmarked Lines", "Zeilen mit Lesezeichen löschen";
    remove_unbookmarked: "Remove Unbookmarked Lines", "Zeilen ohne Lesezeichen löschen";
    bookmark_matches: "Bookmark Lines with Matches", "Zeilen mit Treffern markieren";

    menu_compare: "Compare", "Vergleichen";
    compare: "Compare", "Vergleichen";
    set_compare_first: "Set as First to Compare", "Als ersten für Vergleich festlegen";
    compare_with_file: "Compare with File…", "Mit Datei vergleichen…";
    compare_with_saved: "Compare with Saved Version", "Mit gespeicherter Version vergleichen";
    end_compare: "End Comparison", "Vergleich beenden";
    next_difference: "Next Difference", "Nächster Unterschied";
    previous_difference: "Previous Difference", "Vorheriger Unterschied";
    ignore_whitespace: "Ignore whitespace", "Leerzeichen ignorieren";
    ignore_case: "Ignore case", "Groß-/Kleinschreibung ignorieren";
    identical: "The files are identical", "Die Dateien sind identisch";
    /// `{changed}`, `{added}` and `{removed}` are numbers of blocks.
    differences: "{changed} changed, {added} added, {removed} removed", "{changed} geändert, {added} hinzugefügt, {removed} entfernt";
    /// `{name}` is the file name.
    saved_version: "{name} (saved)", "{name} (gespeichert)";

    menu_format: "Encoding", "Kodierung";
    interpret_as: "Interpret As", "Interpretieren als";
    convert_to: "Convert To", "Konvertieren in";
    character_sets: "Character Sets", "Zeichensätze";
    line_ending: "Line Ending", "Zeilenende";
    script_western: "Western European", "Westeuropäisch";
    script_central: "Central European", "Mitteleuropäisch";
    script_baltic: "Baltic", "Baltisch";
    script_cyrillic: "Cyrillic", "Kyrillisch";
    script_greek: "Greek", "Griechisch";
    script_turkish: "Turkish", "Türkisch";
    script_hebrew: "Hebrew", "Hebräisch";
    script_arabic: "Arabic", "Arabisch";
    script_nordic: "Nordic", "Nordeuropäisch";
    script_vietnamese: "Vietnamese", "Vietnamesisch";
    script_thai: "Thai", "Thailändisch";
    script_chinese: "Chinese", "Chinesisch";
    script_japanese: "Japanese", "Japanisch";
    script_korean: "Korean", "Koreanisch";
    script_dos: "DOS (OEM)", "DOS (OEM)";

    menu_syntax: "Language", "Sprache";
    plain_text: "Normal text", "Normaler Text";
    user_languages: "User-Defined Languages…", "Benutzerdefinierte Sprachen…";
    udl_name: "Name", "Name";
    udl_extensions: "File extensions", "Dateiendungen";
    udl_keywords: "Keywords", "Schlüsselwörter";
    udl_types: "Types and constants", "Typen und Konstanten";
    udl_line_comment: "Line comment", "Zeilenkommentar";
    udl_block_comment: "Block comment (start, end)", "Blockkommentar (Anfang, Ende)";
    udl_quotes: "String quotes", "Anführungszeichen";
    udl_ignore_case: "Ignore case", "Groß-/Kleinschreibung ignorieren";
    udl_words_hint: "Separate words with spaces.", "Wörter durch Leerzeichen trennen.";
    udl_new: "New language", "Neue Sprache";
    add: "Add", "Hinzufügen";
    delete: "Delete", "Löschen";

    menu_view: "View", "Ansicht";
    always_on_top: "Always on Top", "Immer im Vordergrund";
    full_screen: "Full Screen", "Vollbild";
    distraction_free: "Distraction Free Mode", "Ablenkungsfreier Modus";
    post_it: "Post-it", "Post-it";
    folding_menu: "Folding", "Falten";
    fold_all: "Fold All", "Alle einklappen";
    unfold_all: "Unfold All", "Alle ausklappen";
    fold_current: "Fold Current Level", "Aktuelle Ebene einklappen";
    unfold_current: "Unfold Current Level", "Aktuelle Ebene ausklappen";
    hide_lines: "Hide Lines", "Zeilen ausblenden";
    folding: "Show Fold Markers", "Faltmarken anzeigen";
    smart_highlight: "Highlight Occurrences of Selected Word", "Vorkommen des markierten Worts hervorheben";
    document_map: "Document Map", "Dokumentkarte";
    file_tree: "Folder as Workspace", "Ordner als Arbeitsbereich";
    toolbar: "Toolbar", "Symbolleiste";
    no_folder: "No folder open.", "Kein Ordner geöffnet.";
    refresh: "Refresh", "Aktualisieren";
    move_to_other_view: "Move to Other View", "In andere Ansicht verschieben";
    close_other_view: "Close Other View", "Andere Ansicht schließen";
    move_to_new_window: "Move to New Window", "In neues Fenster verschieben";
    show_symbol: "Show Symbol", "Symbole anzeigen";
    show_whitespace: "Show Space and TAB", "Leerzeichen und Tabs anzeigen";
    show_line_endings: "Show End of Line", "Zeilenenden anzeigen";
    show_all_chars: "Show All Characters", "Alle Zeichen anzeigen";
    show_indent_guides: "Show Indent Guide", "Einrückungslinien anzeigen";
    show_wrap_symbol: "Show Wrap Symbol", "Umbruchsymbol anzeigen";
    edge_line: "Show Edge Line", "Randlinie anzeigen";
    word_wrap: "Word Wrap", "Zeilenumbruch";
    change_history: "Show Change History", "Änderungsverlauf anzeigen";
    zoom_in: "Zoom In", "Vergrößern";
    zoom_out: "Zoom Out", "Verkleinern";
    zoom_reset: "Restore Default Zoom", "Standardgröße";
    summary: "Summary…", "Zusammenfassung…";
    panels: "Panels", "Bereiche";
    function_list: "Function List", "Funktionsliste";
    document_list: "Document List", "Dokumentliste";
    clipboard_history: "Clipboard History", "Zwischenablage-Verlauf";
    character_panel: "Character Panel", "Zeichentabelle";
    tab_menu: "Tab", "Tab";
    next_tab: "Next Tab", "Nächster Tab";
    previous_tab: "Previous Tab", "Vorheriger Tab";
    move_tab_forward: "Move Tab Forward", "Tab nach rechts verschieben";
    move_tab_backward: "Move Tab Backward", "Tab nach links verschieben";
    pin_tab: "Pin Tab", "Tab anheften";
    tab_color: "Tab Color", "Tabfarbe";
    no_color: "None", "Keine";
    /// `{n}` is a color number.
    color_n: "Color {n}", "Farbe {n}";
    split_view: "Split View", "Geteilte Ansicht";
    monitoring: "Monitoring (tail -f)", "Überwachen (tail -f)";
    summary_path: "Full path", "Vollständiger Pfad";
    summary_modified: "Modified", "Geändert";
    summary_chars: "Characters (without line endings)", "Zeichen (ohne Zeilenenden)";
    summary_words: "Words", "Wörter";
    summary_lines: "Lines", "Zeilen";
    summary_bytes: "Document length (bytes)", "Dokumentlänge (Bytes)";
    summary_selection: "Selected characters", "Ausgewählte Zeichen";
    no_symbols: "No functions found", "Keine Funktionen gefunden";
    clipboard_empty: "Copied texts appear here. Click one to insert it.", "Kopierte Texte erscheinen hier. Klicke einen an, um ihn einzufügen.";
    char_value: "Value", "Wert";
    char_char: "Character", "Zeichen";

    menu_settings: "Settings", "Einstellungen";
    language: "User Interface Language", "Sprache der Oberfläche";
    theme: "Appearance", "Erscheinungsbild";
    theme_system: "System", "System";
    theme_light: "Light", "Hell";
    theme_dark: "Dark", "Dunkel";
    tab_width: "Tab Width", "Tabulatorbreite";
    indent_with_spaces: "Indent with Spaces", "Mit Leerzeichen einrücken";
    auto_close: "Auto-Close Brackets and Quotes", "Klammern und Anführungszeichen automatisch schließen";
    auto_complete: "Auto-Completion While Typing", "Automatisch vervollständigen beim Tippen";
    backup: "Remember Unsaved Changes on Quit", "Ungespeicherte Änderungen beim Beenden merken";
    ask_reload: "Ask Before Reloading Changed Files", "Vor dem Neuladen geänderter Dateien fragen";
    preferences: "All Settings…", "Alle Einstellungen…";
    date_format: "Custom date format", "Eigenes Datumsformat";
    date_format_help: "%Y year, %m month, %d day, %H hour, %M minute, %S second", "%Y Jahr, %m Monat, %d Tag, %H Stunde, %M Minute, %S Sekunde";
    invalid_date_format: "Invalid date format", "Ungültiges Datumsformat";
    edge_column: "Edge column", "Randspalte";
    new_documents: "New documents", "Neue Dokumente";
    tab_general: "General", "Allgemein";
    tab_editor: "Editor", "Editor";
    tab_backup: "Backup & Updates", "Sicherung & Updates";
    tab_shortcuts: "Shortcuts", "Tastenkürzel";
    color_theme: "Color theme", "Farbschema";
    theme_standard: "Standard (follows appearance)", "Standard (folgt dem Erscheinungsbild)";
    font: "Font", "Schriftart";
    font_builtin: "Built-in", "Eingebaut";
    choose_font: "Choose…", "Auswählen…";
    font_size: "Font size", "Schriftgröße";
    /// `{error}` is the reason.
    font_failed: "The font could not be loaded: {error}", "Die Schriftart konnte nicht geladen werden: {error}";
    font_too_large: "the file is larger than 50 MB", "die Datei ist größer als 50 MB";
    not_a_font: "not a TrueType or OpenType font", "keine TrueType- oder OpenType-Schrift";
    save_backup: "When saving", "Beim Speichern";
    backup_none: "No backup", "Keine Sicherung";
    backup_simple: "Simple (name.bak next to the file)", "Einfach (name.bak neben der Datei)";
    backup_verbose: "Verbose (copies with date in the settings folder)", "Ausführlich (Kopien mit Datum im Einstellungsordner)";
    press_keys: "Press keys…", "Tasten drücken…";
    remove: "Remove", "Entfernen";
    reset: "Reset", "Zurücksetzen";
    filter: "Filter", "Filtern";
    editor_shortcut_note: "Undo, clipboard and indent keys are part of the editor and cannot be changed.", "Rückgängig, Zwischenablage und Einrücken gehören zum Editor und lassen sich nicht ändern.";

    menu_macro: "Macro", "Makro";
    start_recording: "Start Recording", "Aufnahme starten";
    stop_recording: "Stop Recording", "Aufnahme beenden";
    play_macro: "Playback", "Abspielen";
    play_repeatedly: "Run a Macro Multiple Times…", "Makro mehrfach ausführen…";
    save_macro: "Save Current Recorded Macro…", "Aufgenommenes Makro speichern…";
    delete_macro: "Delete Saved Macro", "Gespeichertes Makro löschen";
    macro_name: "Name", "Name";
    run_times: "Run this many times", "So oft ausführen";
    until_end: "Run until the end of file", "Bis zum Dateiende ausführen";
    run: "Run", "Ausführen";
    recording: "Recording", "Aufnahme";

    menu_tools: "Tools", "Werkzeuge";
    checksum_selection: "Generate from Selection into Clipboard", "Aus Auswahl in die Zwischenablage";
    checksum_files: "Generate from Files…", "Aus Dateien erzeugen…";

    menu_help: "Help", "Hilfe";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    menu_window: "Window", "Fenster";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_settings: "Settings…", "Einstellungen…";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_services: "Services", "Dienste";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_hide: "Hide CuneiEdit", "CuneiEdit ausblenden";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_hide_others: "Hide Others", "Andere ausblenden";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_show_all: "Show All", "Alle einblenden";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_quit: "Quit CuneiEdit", "CuneiEdit beenden";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_minimize: "Minimize", "Im Dock ablegen";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_zoom: "Zoom", "Zoomen";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_fullscreen: "Enter Full Screen", "Vollbild";
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    mac_bring_all_to_front: "Bring All to Front", "Alle nach vorne bringen";
    about: "About CuneiEdit", "Über CuneiEdit";
    /// `{error}` is the reason.
    #[cfg_attr(not(windows), allow(dead_code))]
    print_failed: "Printing failed: {error}", "Drucken fehlgeschlagen: {error}";
    about_text: "A fast and simple text editor.", "Ein schneller und einfacher Texteditor.";
    check_for_updates: "Check for Updates…", "Nach Updates suchen…";
    updates_title: "Updates", "Updates";
    update_question: "Should CuneiEdit look for new versions? It then asks github.com once a day; apart from that request (with your IP address) nothing is sent. Every update is checked against CuneiEdit's signature before it is installed. You can change this later in the settings.", "Soll CuneiEdit nach neuen Versionen suchen? Es fragt dann einmal täglich bei github.com nach; außer dieser Anfrage (mit deiner IP-Adresse) wird nichts gesendet. Jedes Update wird vor der Installation gegen die Signatur von CuneiEdit geprüft. Du kannst das später in den Einstellungen ändern.";
    update_off: "Don't look for updates", "Nicht nach Updates suchen";
    update_notify: "Look daily and ask before installing", "Täglich suchen und vor dem Installieren fragen";
    update_automatic: "Look daily and install automatically", "Täglich suchen und automatisch installieren";
    update_checking: "Looking for a new version…", "Suche nach einer neuen Version …";
    /// `{version}` is the installed version.
    update_current: "CuneiEdit {version} is the latest version.", "CuneiEdit {version} ist die neueste Version.";
    /// `{version}` is the new version, `{current}` the installed one.
    update_available: "CuneiEdit {version} is available (installed: {current}).", "CuneiEdit {version} ist verfügbar (installiert: {current}).";
    update_notes: "What's new", "Neu";
    update_install: "Install", "Installieren";
    update_later: "Later", "Später";
    /// `{url}` is the release page.
    update_manual: "Download it from {url}", "Herunterladen unter {url}";
    /// `{version}` is the new version.
    update_installing: "Downloading and installing CuneiEdit {version}…", "CuneiEdit {version} wird heruntergeladen und installiert …";
    /// `{version}` is the new version.
    update_installed: "CuneiEdit {version} is installed and starts with the next launch.", "CuneiEdit {version} ist installiert und startet beim nächsten Programmstart.";
    update_restart: "Restart Now", "Jetzt neu starten";
    /// `{error}` is the reason.
    update_failed: "The update did not work: {error}", "Das Update hat nicht funktioniert: {error}";
    /// `{error}` is the message of the download tool.
    update_err_download: "no connection to GitHub ({error})", "keine Verbindung zu GitHub ({error})";
    update_err_signature: "the update is not signed by CuneiEdit and was rejected.", "das Update ist nicht von CuneiEdit signiert und wurde abgelehnt.";
    update_err_damaged: "the download is damaged.", "der Download ist beschädigt.";
    update_err_location: "this copy cannot be updated where it is (e.g. in a disk image). Move CuneiEdit to the Applications folder first.", "diese Kopie kann an ihrem Ort nicht aktualisiert werden (z. B. in einem Disk-Image). Verschiebe CuneiEdit zuerst in den Programme-Ordner.";
    update_err_platform: "there is no installable version for this system yet.", "für dieses System gibt es noch keine installierbare Version.";
    update_err_denied: "administrator rights were not given.", "Administratorrechte wurden nicht erteilt.";

    /// Title of a new, unsaved document; `{n}` is its number.
    untitled: "new {n}", "neu {n}";
    unsaved_title: "Unsaved Changes", "Ungespeicherte Änderungen";
    /// `{name}` is the document title.
    unsaved_question: "Do you want to save the changes to “{name}”?", "Möchtest du die Änderungen an „{name}“ speichern?";
    dont_save: "Don't Save", "Nicht speichern";
    cancel: "Cancel", "Abbrechen";
    ok: "OK", "OK";
    error: "Error", "Fehler";
    /// `{name}` is the file name, `{error}` the system error.
    open_failed: "“{name}” could not be opened: {error}", "„{name}“ konnte nicht geöffnet werden: {error}";
    /// `{name}` is the file name, `{error}` the system error.
    save_failed: "“{name}” could not be saved: {error}", "„{name}“ konnte nicht gespeichert werden: {error}";
    /// `{name}` is the file name, `{error}` the system error.
    rename_failed: "“{name}” could not be renamed: {error}", "„{name}“ konnte nicht umbenannt werden: {error}";
    /// `{name}` is the file name, `{error}` the system error.
    trash_failed: "“{name}” could not be moved to the Trash: {error}", "„{name}“ konnte nicht in den Papierkorb gelegt werden: {error}";
    /// `{char}`, `{line}`, `{column}` and `{encoding}` describe the character that cannot be saved.
    unencodable: "The character “{char}” in line {line}, column {column} cannot be saved as {encoding}. Choose a different encoding under Encoding → Convert To.", "Das Zeichen „{char}“ in Zeile {line}, Spalte {column} kann nicht als {encoding} gespeichert werden. Wähle unter Kodierung → Konvertieren in eine andere Kodierung.";
    new_name: "New name", "Neuer Name";
    /// `{name}` is the file name.
    trash_question: "Move “{name}” to the Trash?", "„{name}“ in den Papierkorb legen?";
    /// `{name}` is the file name.
    changed_on_disk: "“{name}” was changed by another program. Reload it?", "„{name}“ wurde von einem anderen Programm geändert. Neu laden?";
    undo_reload_hint: "Your unsaved changes can be restored with Undo.", "Deine ungespeicherten Änderungen lassen sich mit Rückgängig wiederherstellen.";
    /// `{name}` is the file name.
    deleted_on_disk: "“{name}” no longer exists. Keep it open?", "„{name}“ existiert nicht mehr. Geöffnet lassen?";
    keep: "Keep", "Behalten";
    reload_button: "Reload", "Neu laden";

    status_length: "length", "Länge";
    status_lines: "lines", "Zeilen";
    status_line: "Ln", "Z";
    status_column: "Col", "Sp";
    status_selection: "Sel", "Ausw";
    status_cursors: "cursors", "Cursor";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(
            fill("„{name}“: {error}", &[("name", "a.txt"), ("error", "nope")]),
            "„a.txt“: nope"
        );
    }

    #[test]
    fn languages_use_the_same_placeholders() {
        let placeholders = |text: &str| {
            let mut found: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned()))
                .collect();
            found.sort();
            found
        };
        for (en, de) in PAIRS {
            assert_eq!(placeholders(en), placeholders(de), "{en} / {de}");
            assert!(!en.is_empty() && !de.is_empty());
        }
    }
}
