//! Everything the user can trigger from menus, shortcuts and context menus, with its
//! label and keyboard shortcut in one place.

use std::borrow::Cow;

use eframe::egui::{Key, KeyboardShortcut, Modifiers, ThemePreference};
use pilcrow_core::checksum::Algorithm;
use pilcrow_core::highlight::Language as Syntax;
use pilcrow_core::ops::{CommentMode, SortOrder};
use pilcrow_core::transform::{Blank, Case};
use pilcrow_core::{Encoding, LineEnding};

use crate::i18n::{Language, Texts, fill};
use crate::settings::Flag;

/// Edits applied to the selected lines (or the selection) of the active document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineOp {
    Duplicate,
    Delete,
    MoveUp,
    MoveDown,
    RemoveDuplicates,
    RemoveConsecutiveDuplicates,
    Split,
    Join,
    BlankAbove,
    BlankBelow,
    RemoveEmpty,
    RemoveBlank,
    Reverse,
    Shuffle,
    Sort(SortOrder, bool),
    Indent,
    Unindent,
    TrimTrailing,
    Case(Case),
    Blank(Blank),
    Comment(CommentMode),
    BlockComment(bool),
    Redact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathPart {
    Full,
    Name,
    Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStyle {
    Short,
    Long,
    Custom,
}

/// Commands without an index act on the active tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    New,
    Open,
    OpenRecent(usize),
    OpenAllRecent,
    ClearRecent,
    RestoreClosed,
    Reload,
    Save,
    SaveAs,
    SaveCopy,
    SaveAll,
    Rename,
    Close,
    CloseTab(usize),
    CloseAll,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseUnchanged,
    MoveToTrash,
    Reveal,
    LoadSession,
    SaveSession,
    OpenFolder,
    CloseFolder,
    ExportHtml,
    ExportPdf,
    Print,
    Quit,

    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    InsertDate(DateStyle),
    CopyPath(PathPart),
    Line(LineOp),
    ToggleReadOnly,
    Complete,
    AddCursorAbove,
    AddCursorBelow,
    ColumnEditor,

    OpenFind,
    OpenReplace,
    FindNext,
    FindPrevious,
    SelectFindNext,
    SelectFindPrevious,
    FindAll,
    FindAllInTabs,
    FindInFiles,
    GoTo,
    MatchingBrace,
    SelectBetweenBraces,
    Mark(usize),
    ClearMark(usize),
    ClearAllMarks,
    FindCharsInRange,
    ToggleResults,
    NextResult,
    PreviousResult,
    NextMark,
    PreviousMark,
    CopyMarked,
    NextChange,
    PreviousChange,
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    ClearBookmarks,
    InvertBookmarks,
    CopyBookmarked,
    CutBookmarked,
    PasteToBookmarked,
    RemoveBookmarked,
    RemoveUnbookmarked,
    BookmarkMatches,

    Toggle(Flag),
    ShowAllCharacters,
    AlwaysOnTop,
    FullScreen,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Summary,
    Activate(usize),
    NextTab,
    PreviousTab,
    MoveTabForward,
    MoveTabBackward,
    /// Moves the tab at the first index to the second, e.g. by dragging.
    MoveTab(usize, usize),
    ToggleSplit,
    ToggleMonitoring,
    TogglePin,
    SetTabColor(Option<usize>),
    MoveToOtherView,
    CloseOtherView,
    FoldAll,
    UnfoldAll,
    FoldCurrent,
    UnfoldCurrent,
    HideLines,
    DistractionFree,
    PostIt,

    Interpret(Encoding),
    Convert(Encoding),
    SetLineEnding(LineEnding),
    SetSyntax(Syntax),
    UserLanguages,

    SetCompareFirst,
    Compare,
    CompareWithFile,
    CompareWithSaved,
    EndCompare,
    NextDifference,
    PreviousDifference,
    ToggleIgnoreWhitespace,
    ToggleIgnoreCase,

    SetLanguage(Language),
    SetTheme(ThemePreference),
    /// Index into the built-in color themes; `None` follows the appearance.
    SetColorTheme(Option<usize>),
    SetTabWidth(usize),
    Preferences,

    StartRecording,
    StopRecording,
    PlayMacro,
    PlayMacroRepeatedly,
    SaveMacro,
    PlaySavedMacro(usize),
    DeleteMacro(usize),

    ChecksumSelection(Algorithm),
    ChecksumFiles(Algorithm),
    About,
}

const CMD: Modifiers = Modifiers::COMMAND;
const SHIFT: Modifiers = Modifiers::SHIFT;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);
const CMD_ALT_SHIFT: Modifiers = CMD_ALT.plus(Modifiers::SHIFT);
const ALT_SHIFT: Modifiers = Modifiers::ALT.plus(Modifiers::SHIFT);
const CTRL: Modifiers = Modifiers::CTRL;
const CTRL_SHIFT: Modifiers = Modifiers::CTRL.plus(Modifiers::SHIFT);
const NONE: Modifiers = Modifiers::NONE;

const fn key(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

const MAC: bool = cfg!(target_os = "macos");
/// ⌘H hides the app on macOS.
const REPLACE: KeyboardShortcut = if MAC {
    key(CMD_ALT, Key::F)
} else {
    key(CMD, Key::H)
};
const FOLD_CURRENT: KeyboardShortcut = if MAC {
    key(CMD_ALT, Key::OpenBracket)
} else {
    key(CMD_ALT, Key::F)
};
const UNFOLD_CURRENT: KeyboardShortcut = if MAC {
    key(CMD_ALT, Key::CloseBracket)
} else {
    key(CMD_ALT_SHIFT, Key::F)
};

/// Default keyboard shortcuts. Shortcuts with more modifiers come first, because a shortcut
/// also matches while extra modifiers are held. The first entry of a command is shown
/// in menus.
pub const SHORTCUTS: &[(KeyboardShortcut, Command)] = &[
    (UNFOLD_CURRENT, Command::UnfoldCurrent),
    (key(CMD_ALT_SHIFT, Key::Num0), Command::UnfoldAll),
    (FOLD_CURRENT, Command::FoldCurrent),
    (REPLACE, Command::OpenReplace),
    (key(CMD_ALT, Key::Num0), Command::FoldAll),
    (key(CMD_ALT, Key::S), Command::SaveAll),
    (key(CMD_ALT, Key::C), Command::Compare),
    (key(CMD_SHIFT, Key::S), Command::SaveAs),
    (key(CMD_SHIFT, Key::W), Command::CloseAll),
    (key(CMD_SHIFT, Key::T), Command::RestoreClosed),
    (key(CMD_SHIFT, Key::G), Command::FindPrevious),
    (key(CMD_SHIFT, Key::F), Command::FindInFiles),
    (key(CMD_SHIFT, Key::F3), Command::SelectFindPrevious),
    (key(CMD_SHIFT, Key::B), Command::SelectBetweenBraces),
    (key(CMD_SHIFT, Key::K), Command::Line(LineOp::Delete)),
    (
        key(CMD_SHIFT, Key::U),
        Command::Line(LineOp::Case(Case::Upper)),
    ),
    (key(CMD_SHIFT, Key::R), Command::StartRecording),
    (key(CMD_SHIFT, Key::P), Command::PlayMacro),
    (key(CMD_SHIFT, Key::D), Command::DistractionFree),
    (key(CTRL_SHIFT, Key::Tab), Command::PreviousTab),
    (key(ALT_SHIFT, Key::ArrowUp), Command::AddCursorAbove),
    (key(ALT_SHIFT, Key::ArrowDown), Command::AddCursorBelow),
    (key(CMD, Key::N), Command::New),
    (key(CMD, Key::O), Command::Open),
    (key(CMD, Key::S), Command::Save),
    (key(CMD, Key::W), Command::Close),
    (key(CMD, Key::Q), Command::Quit),
    (key(CMD, Key::P), Command::Print),
    (key(CMD, Key::F), Command::OpenFind),
    (key(CMD, Key::Comma), Command::Preferences),
    (key(CMD, Key::G), Command::FindNext),
    (key(CMD, Key::F3), Command::SelectFindNext),
    (key(CMD, Key::L), Command::GoTo),
    (key(CMD, Key::B), Command::MatchingBrace),
    (key(CMD, Key::D), Command::Line(LineOp::Duplicate)),
    (key(CMD, Key::J), Command::Line(LineOp::Join)),
    (key(CMD, Key::U), Command::Line(LineOp::Case(Case::Lower))),
    (
        key(CMD, Key::Slash),
        Command::Line(LineOp::Comment(CommentMode::Toggle)),
    ),
    (key(CMD, Key::Plus), Command::ZoomIn),
    (key(CMD, Key::Equals), Command::ZoomIn),
    (key(CMD, Key::Minus), Command::ZoomOut),
    (key(CMD, Key::Num0), Command::ZoomReset),
    (key(CMD, Key::F2), Command::ToggleBookmark),
    (key(CMD, Key::PageDown), Command::NextTab),
    (key(CMD, Key::PageUp), Command::PreviousTab),
    (key(CTRL, Key::Tab), Command::NextTab),
    (key(CTRL, Key::Space), Command::Complete),
    (
        key(Modifiers::ALT, Key::ArrowUp),
        Command::Line(LineOp::MoveUp),
    ),
    (
        key(Modifiers::ALT, Key::ArrowDown),
        Command::Line(LineOp::MoveDown),
    ),
    (key(SHIFT, Key::F3), Command::FindPrevious),
    (key(SHIFT, Key::F2), Command::PreviousBookmark),
    (key(SHIFT, Key::F4), Command::PreviousResult),
    (key(SHIFT, Key::F8), Command::PreviousDifference),
    (key(NONE, Key::F3), Command::FindNext),
    (key(NONE, Key::F2), Command::NextBookmark),
    (key(NONE, Key::F4), Command::NextResult),
    (key(NONE, Key::F7), Command::ToggleResults),
    (key(NONE, Key::F8), Command::NextDifference),
    (key(NONE, Key::F11), Command::FullScreen),
    (key(NONE, Key::F12), Command::PostIt),
];

/// Commands without a default shortcut that can get one in the settings.
pub const CUSTOMIZABLE: &[Command] = &[
    Command::Reload,
    Command::SaveCopy,
    Command::Rename,
    Command::CloseOthers,
    Command::CloseUnchanged,
    Command::Reveal,
    Command::OpenFolder,
    Command::ExportHtml,
    Command::ExportPdf,
    Command::InsertDate(DateStyle::Short),
    Command::InsertDate(DateStyle::Long),
    Command::InsertDate(DateStyle::Custom),
    Command::CopyPath(PathPart::Full),
    Command::Line(LineOp::Sort(SortOrder::Lexicographic, false)),
    Command::Line(LineOp::Sort(SortOrder::Lexicographic, true)),
    Command::Line(LineOp::RemoveDuplicates),
    Command::Line(LineOp::RemoveEmpty),
    Command::Line(LineOp::TrimTrailing),
    Command::Line(LineOp::Case(Case::Proper)),
    Command::Line(LineOp::BlankAbove),
    Command::Line(LineOp::BlankBelow),
    Command::Line(LineOp::Comment(CommentMode::Comment)),
    Command::Line(LineOp::Comment(CommentMode::Uncomment)),
    Command::Line(LineOp::BlockComment(true)),
    Command::Line(LineOp::BlockComment(false)),
    Command::Line(LineOp::Split),
    Command::Line(LineOp::Blank(Blank::TabsToSpaces)),
    Command::ToggleReadOnly,
    Command::ColumnEditor,
    Command::FindAll,
    Command::FindAllInTabs,
    Command::ClearAllMarks,
    Command::NextMark,
    Command::PreviousMark,
    Command::CopyMarked,
    Command::NextChange,
    Command::PreviousChange,
    Command::ClearBookmarks,
    Command::Toggle(Flag::WordWrap),
    Command::Toggle(Flag::ShowWhitespace),
    Command::Toggle(Flag::IndentGuides),
    Command::Toggle(Flag::DocumentMap),
    Command::Toggle(Flag::FileTree),
    Command::Toggle(Flag::Toolbar),
    Command::Toggle(Flag::FunctionList),
    Command::Toggle(Flag::DocumentList),
    Command::ShowAllCharacters,
    Command::AlwaysOnTop,
    Command::HideLines,
    Command::ToggleSplit,
    Command::MoveToOtherView,
    Command::Summary,
    Command::SetCompareFirst,
    Command::CompareWithSaved,
    Command::EndCompare,
    Command::PlayMacroRepeatedly,
    Command::SaveMacro,
    Command::UserLanguages,
];

/// Handled by the editor widget; listed for the menu labels only.
const EDITOR_SHORTCUTS: &[(KeyboardShortcut, Command)] = &[
    (key(CMD, Key::Z), Command::Undo),
    (key(CMD_SHIFT, Key::Z), Command::Redo),
    (key(CMD, Key::X), Command::Cut),
    (key(CMD, Key::C), Command::Copy),
    (key(CMD, Key::V), Command::Paste),
    (key(CMD, Key::A), Command::SelectAll),
    (key(NONE, Key::Tab), Command::Line(LineOp::Indent)),
    (key(SHIFT, Key::Tab), Command::Line(LineOp::Unindent)),
];

/// Cmd+1 … Cmd+9 activate a tab.
pub const TAB_KEYS: [Key; 9] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

impl Command {
    /// Shortcut of a command the editor widget handles itself; these cannot be changed.
    pub fn editor_shortcut(self) -> Option<KeyboardShortcut> {
        EDITOR_SHORTCUTS
            .iter()
            .find(|(_, c)| *c == self)
            .map(|(shortcut, _)| *shortcut)
    }

    /// Whether the command changes the text, so it is unavailable in read-only documents.
    pub fn edits(self) -> bool {
        matches!(
            self,
            Self::Undo
                | Self::Redo
                | Self::Cut
                | Self::Paste
                | Self::InsertDate(_)
                | Self::Line(_)
                | Self::Complete
                | Self::ColumnEditor
                | Self::CutBookmarked
                | Self::PasteToBookmarked
                | Self::RemoveBookmarked
                | Self::RemoveUnbookmarked
                | Self::PlayMacro
                | Self::PlayMacroRepeatedly
                | Self::PlaySavedMacro(_)
        )
    }

    /// Menu label; empty for commands that never appear as a plain menu item.
    pub fn label(self, t: &Texts) -> Cow<'static, str> {
        let text: &'static str = match self {
            Self::New => t.new,
            Self::Open => t.open,
            Self::OpenAllRecent => t.open_all_recent,
            Self::ClearRecent => t.clear_recent,
            Self::RestoreClosed => t.restore_closed,
            Self::Reload => t.reload,
            Self::Save => t.save,
            Self::SaveAs => t.save_as,
            Self::SaveCopy => t.save_copy,
            Self::SaveAll => t.save_all,
            Self::Rename => t.rename,
            Self::Close | Self::CloseTab(_) => t.close,
            Self::CloseAll => t.close_all,
            Self::CloseOthers => t.close_others,
            Self::CloseLeft => t.close_left,
            Self::CloseRight => t.close_right,
            Self::CloseUnchanged => t.close_unchanged,
            Self::MoveToTrash => t.move_to_trash,
            Self::Reveal if cfg!(target_os = "macos") => t.reveal_finder,
            Self::Reveal => t.reveal_other,
            Self::LoadSession => t.load_session,
            Self::SaveSession => t.save_session,
            Self::OpenFolder => t.open_folder,
            Self::CloseFolder => t.close_folder,
            Self::ExportHtml => t.export_html,
            Self::ExportPdf => t.export_pdf,
            Self::Print => t.print,
            Self::Quit => t.quit,
            Self::Undo => t.undo,
            Self::Redo => t.redo,
            Self::Cut => t.cut,
            Self::Copy => t.copy,
            Self::Paste => t.paste,
            Self::SelectAll => t.select_all,
            Self::InsertDate(DateStyle::Short) => t.date_short,
            Self::InsertDate(DateStyle::Long) => t.date_long,
            Self::InsertDate(DateStyle::Custom) => t.date_custom,
            Self::CopyPath(PathPart::Full) => t.copy_full_path,
            Self::CopyPath(PathPart::Name) => t.copy_file_name,
            Self::CopyPath(PathPart::Directory) => t.copy_directory,
            Self::Line(op) => return op.label(t),
            Self::ToggleReadOnly => t.read_only,
            Self::Complete => t.complete,
            Self::AddCursorAbove => t.add_cursor_above,
            Self::AddCursorBelow => t.add_cursor_below,
            Self::ColumnEditor => t.column_editor,
            Self::OpenFind => t.find,
            Self::OpenReplace => t.replace,
            Self::FindNext => t.find_next,
            Self::FindPrevious => t.find_previous,
            Self::SelectFindNext => t.select_find_next,
            Self::SelectFindPrevious => t.select_find_previous,
            Self::FindAll => t.find_all,
            Self::FindAllInTabs => t.find_all_tabs,
            Self::FindInFiles => t.find_in_files,
            Self::GoTo => t.go_to,
            Self::MatchingBrace => t.matching_brace,
            Self::SelectBetweenBraces => t.select_braces,
            Self::Mark(n) | Self::ClearMark(n) => {
                return fill(t.mark_style, &[("n", &(n + 1).to_string())]).into();
            }
            Self::ClearAllMarks => t.clear_all_styles,
            Self::FindCharsInRange => t.find_chars,
            Self::ToggleResults => t.results,
            Self::NextResult => t.next_result,
            Self::PreviousResult => t.previous_result,
            Self::NextMark => t.next_mark,
            Self::PreviousMark => t.previous_mark,
            Self::CopyMarked => t.copy_marked,
            Self::NextChange => t.next_change,
            Self::PreviousChange => t.previous_change,
            Self::ToggleBookmark => t.toggle_bookmark,
            Self::NextBookmark => t.next_bookmark,
            Self::PreviousBookmark => t.previous_bookmark,
            Self::ClearBookmarks => t.clear_bookmarks,
            Self::InvertBookmarks => t.invert_bookmarks,
            Self::CopyBookmarked => t.copy_bookmarked,
            Self::CutBookmarked => t.cut_bookmarked,
            Self::PasteToBookmarked => t.paste_bookmarked,
            Self::RemoveBookmarked => t.remove_bookmarked,
            Self::RemoveUnbookmarked => t.remove_unbookmarked,
            Self::BookmarkMatches => t.bookmark_matches,
            Self::Toggle(flag) => flag_label(flag, t),
            Self::ShowAllCharacters => t.show_all_chars,
            Self::AlwaysOnTop => t.always_on_top,
            Self::FullScreen => t.full_screen,
            Self::ZoomIn => t.zoom_in,
            Self::ZoomOut => t.zoom_out,
            Self::ZoomReset => t.zoom_reset,
            Self::Summary => t.summary,
            Self::NextTab => t.next_tab,
            Self::PreviousTab => t.previous_tab,
            Self::MoveTabForward => t.move_tab_forward,
            Self::MoveTabBackward => t.move_tab_backward,
            Self::ToggleSplit => t.split_view,
            Self::ToggleMonitoring => t.monitoring,
            Self::TogglePin => t.pin_tab,
            Self::SetTabColor(None) => t.no_color,
            Self::SetTabColor(Some(n)) => {
                return fill(t.color_n, &[("n", &(n + 1).to_string())]).into();
            }
            Self::MoveToOtherView => t.move_to_other_view,
            Self::CloseOtherView => t.close_other_view,
            Self::FoldAll => t.fold_all,
            Self::UnfoldAll => t.unfold_all,
            Self::FoldCurrent => t.fold_current,
            Self::UnfoldCurrent => t.unfold_current,
            Self::HideLines => t.hide_lines,
            Self::DistractionFree => t.distraction_free,
            Self::PostIt => t.post_it,
            Self::UserLanguages => t.user_languages,
            Self::Interpret(encoding) | Self::Convert(encoding) => encoding.label(),
            Self::SetLineEnding(line_ending) => line_ending.label(),
            Self::SetCompareFirst => t.set_compare_first,
            Self::Compare => t.compare,
            Self::CompareWithFile => t.compare_with_file,
            Self::CompareWithSaved => t.compare_with_saved,
            Self::EndCompare => t.end_compare,
            Self::NextDifference => t.next_difference,
            Self::PreviousDifference => t.previous_difference,
            Self::ToggleIgnoreWhitespace => t.ignore_whitespace,
            Self::ToggleIgnoreCase => t.ignore_case,
            Self::SetLanguage(language) => language.native_name(),
            Self::SetTheme(ThemePreference::System) => t.theme_system,
            Self::SetTheme(ThemePreference::Light) => t.theme_light,
            Self::SetTheme(ThemePreference::Dark) => t.theme_dark,
            Self::SetColorTheme(None) => t.theme_standard,
            Self::SetColorTheme(Some(index)) => crate::themes::THEMES
                .get(index)
                .map_or("", |theme| theme.name),
            Self::SetTabWidth(width) => return width.to_string().into(),
            Self::Preferences => t.preferences,
            Self::StartRecording => t.start_recording,
            Self::StopRecording => t.stop_recording,
            Self::PlayMacro => t.play_macro,
            Self::PlayMacroRepeatedly => t.play_repeatedly,
            Self::SaveMacro => t.save_macro,
            Self::ChecksumSelection(_) => t.checksum_selection,
            Self::ChecksumFiles(_) => t.checksum_files,
            Self::About => t.about,
            Self::OpenRecent(_)
            | Self::Activate(_)
            | Self::SetSyntax(_)
            | Self::MoveTab(..)
            | Self::PlaySavedMacro(_)
            | Self::DeleteMacro(_) => "",
        };
        text.into()
    }
}

impl LineOp {
    fn label(self, t: &Texts) -> Cow<'static, str> {
        let text = match self {
            Self::Duplicate => t.duplicate_line,
            Self::Delete => t.delete_line,
            Self::MoveUp => t.move_line_up,
            Self::MoveDown => t.move_line_down,
            Self::RemoveDuplicates => t.remove_duplicates,
            Self::RemoveConsecutiveDuplicates => t.remove_consecutive_duplicates,
            Self::Split => t.split_lines,
            Self::Join => t.join_lines,
            Self::BlankAbove => t.blank_above,
            Self::BlankBelow => t.blank_below,
            Self::RemoveEmpty => t.remove_empty,
            Self::RemoveBlank => t.remove_blank,
            Self::Reverse => t.reverse_lines,
            Self::Shuffle => t.shuffle_lines,
            Self::Sort(order, descending) => {
                let order = match order {
                    SortOrder::Lexicographic => t.sort_lexicographic,
                    SortOrder::IgnoreCase => t.sort_ignore_case,
                    SortOrder::Locale => t.sort_locale,
                    SortOrder::Length => t.sort_length,
                    SortOrder::Integer => t.sort_integer,
                    SortOrder::DecimalComma => t.sort_decimal_comma,
                    SortOrder::DecimalDot => t.sort_decimal_dot,
                };
                let direction = if descending {
                    t.descending
                } else {
                    t.ascending
                };
                return fill(t.sort_item, &[("order", order), ("direction", direction)]).into();
            }
            Self::Indent => t.increase_indent,
            Self::Unindent => t.decrease_indent,
            Self::TrimTrailing => t.trim_trailing,
            Self::Case(case) => match case {
                Case::Upper => t.uppercase,
                Case::Lower => t.lowercase,
                Case::Proper => t.proper_case,
                Case::ProperBlend => t.proper_blend,
                Case::Sentence => t.sentence_case,
                Case::SentenceBlend => t.sentence_blend,
                Case::Invert => t.invert_case,
                Case::Random(_) => t.random_case,
            },
            Self::Blank(blank) => match blank {
                Blank::TrimLeading => t.trim_leading,
                Blank::TrimBoth => t.trim_both,
                Blank::EolToSpace => t.eol_to_space,
                Blank::TrimAndEolToSpace => t.trim_eol_to_space,
                Blank::TabsToSpaces => t.tabs_to_spaces,
                Blank::SpacesToTabs(false) => t.spaces_to_tabs_all,
                Blank::SpacesToTabs(true) => t.spaces_to_tabs_leading,
            },
            Self::Comment(CommentMode::Toggle) => t.toggle_comment,
            Self::Comment(CommentMode::Comment) => t.line_comment,
            Self::Comment(CommentMode::Uncomment) => t.line_uncomment,
            Self::BlockComment(true) => t.block_comment,
            Self::BlockComment(false) => t.block_uncomment,
            Self::Redact => t.redact,
        };
        text.into()
    }
}

pub fn flag_label(flag: Flag, t: &Texts) -> &'static str {
    match flag {
        Flag::IndentWithSpaces => t.indent_with_spaces,
        Flag::ShowWhitespace => t.show_whitespace,
        Flag::ShowLineEndings => t.show_line_endings,
        Flag::ShowWrapSymbol => t.show_wrap_symbol,
        Flag::IndentGuides => t.show_indent_guides,
        Flag::WordWrap => t.word_wrap,
        Flag::AutoClose => t.auto_close,
        Flag::EdgeLine => t.edge_line,
        Flag::Backup => t.backup,
        Flag::ChangeHistory => t.change_history,
        Flag::AutoComplete => t.auto_complete,
        Flag::AskBeforeReload => t.ask_reload,
        Flag::FunctionList => t.function_list,
        Flag::DocumentList => t.document_list,
        Flag::ClipboardHistory => t.clipboard_history,
        Flag::CharacterPanel => t.character_panel,
        Flag::SmartHighlight => t.smart_highlight,
        Flag::Folding => t.folding,
        Flag::DocumentMap => t.document_map,
        Flag::FileTree => t.file_tree,
        Flag::Toolbar => t.toolbar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique() {
        let all: Vec<_> = SHORTCUTS.iter().chain(EDITOR_SHORTCUTS).collect();
        for (i, (a, _)) in all.iter().enumerate() {
            for (b, _) in &all[i + 1..] {
                assert_ne!(a, b, "{a:?} is assigned twice");
            }
        }
    }

    #[test]
    fn modifier_supersets_come_first() {
        // A shortcut must not be shadowed by an earlier one with fewer modifiers.
        for (i, (later, _)) in SHORTCUTS.iter().enumerate() {
            for (earlier, _) in &SHORTCUTS[..i] {
                let shadows = earlier.logical_key == later.logical_key
                    && later.modifiers.contains(earlier.modifiers)
                    && later.modifiers != earlier.modifiers;
                assert!(!shadows, "{earlier:?} shadows {later:?}");
            }
        }
    }

    #[test]
    fn every_menu_command_has_a_label() {
        let t = Language::English.texts();
        let commands = SHORTCUTS.iter().chain(EDITOR_SHORTCUTS).map(|(_, c)| c);
        for command in commands.chain(CUSTOMIZABLE) {
            assert!(!command.label(t).is_empty(), "{command:?}");
        }
    }

    #[test]
    fn customizable_commands_have_no_default_shortcut() {
        for command in CUSTOMIZABLE {
            assert!(
                !SHORTCUTS.iter().any(|(_, c)| c == command),
                "{command:?} is listed twice"
            );
        }
    }
}
