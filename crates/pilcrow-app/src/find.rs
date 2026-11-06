use std::ops::Range;

use eframe::egui::{self, Key, TextEdit, Ui};
use pilcrow_core::{
    Document, Search, SearchError, SearchQuery, match_index, next_match, prev_match,
};
use pilcrow_editor::EditorState;

use crate::i18n::{Texts, fill};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindAction {
    /// The query changed: jump to the first match from the selection start.
    Changed,
    Next,
    Previous,
    Replace,
    ReplaceAll,
    ReplaceAllInTabs,
    FindAll,
    FindAllInTabs,
    Close,
}

/// Matches of one query in one document revision.
struct Cache {
    tab: u64,
    revision: u64,
    query: SearchQuery,
    scope: Option<Range<usize>>,
    search: Result<Option<Search>, SearchError>,
    matches: Vec<Range<usize>>,
}

/// Find/replace bar shared by all tabs.
pub struct FindBar {
    pub open: bool,
    replace: bool,
    query: SearchQuery,
    replacement: String,
    wrap: bool,
    /// "In selection": the char range searched, fixed when the option was turned on.
    scope: Option<Range<usize>>,
    focus_field: bool,
    status: Option<String>,
    cache: Option<Cache>,
}

impl Default for FindBar {
    fn default() -> Self {
        Self {
            open: false,
            replace: false,
            query: SearchQuery::default(),
            replacement: String::new(),
            wrap: true,
            scope: None,
            focus_field: false,
            status: None,
            cache: None,
        }
    }
}

impl FindBar {
    /// Opens the bar, using a single-line selection as the new pattern.
    pub fn show(&mut self, replace: bool, selected: &str) {
        self.open = true;
        self.replace = replace;
        self.focus_field = true;
        self.status = None;
        self.set_pattern(selected);
    }

    fn set_pattern(&mut self, text: &str) {
        if !text.is_empty() && !text.contains('\n') {
            self.query.pattern = text.to_owned();
        }
    }

    pub fn has_pattern(&self) -> bool {
        !self.query.pattern.is_empty()
    }

    pub fn query(&self) -> &SearchQuery {
        &self.query
    }

    pub fn query_mut(&mut self) -> &mut SearchQuery {
        &mut self.query
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    pub fn replacement_mut(&mut self) -> &mut String {
        &mut self.replacement
    }

    /// Searches literally for `text` (e.g. the selection) and selects the next match.
    pub fn select_and_find(
        &mut self,
        text: &str,
        forward: bool,
        tab: u64,
        doc: &Document,
        editor: &mut EditorState,
    ) {
        if text.is_empty() || text.contains('\n') {
            return;
        }
        self.query = SearchQuery {
            pattern: text.to_owned(),
            match_case: self.query.match_case,
            ..SearchQuery::default()
        };
        self.scope = None;
        let selection = editor.selection();
        let wrap = self.wrap;
        if forward {
            self.select(tab, doc, editor, |m| next_match(m, selection.end, wrap));
        } else {
            self.select(tab, doc, editor, |m| prev_match(m, selection.start, wrap));
        }
    }

    /// Matches in `doc`, recomputed only when the document or query changed.
    pub fn matches(&mut self, tab: u64, doc: &Document) -> &[Range<usize>] {
        let current = self.cache.as_ref().is_some_and(|cache| {
            cache.tab == tab
                && cache.revision == doc.revision()
                && cache.query == self.query
                && cache.scope == self.scope
        });
        if !current {
            let search = Search::new(&self.query);
            let mut matches = match &search {
                Ok(Some(search)) => search.find_all(doc),
                _ => Vec::new(),
            };
            if let Some(scope) = &self.scope {
                matches.retain(|m| m.start >= scope.start && m.end <= scope.end);
            }
            self.cache = Some(Cache {
                tab,
                revision: doc.revision(),
                query: self.query.clone(),
                scope: self.scope.clone(),
                search,
                matches,
            });
        }
        &self.cache.as_ref().expect("cache was just filled").matches
    }

    /// The compiled query, if it is valid and not empty.
    pub fn search(&self) -> Option<&Search> {
        self.cache.as_ref()?.search.as_ref().ok()?.as_ref()
    }

    /// Replaces every match in all of `doc`; returns how many.
    pub fn replace_all_in(&self, doc: &mut Document) -> usize {
        match Search::new(&self.query) {
            Ok(Some(search)) => search.replace_all(doc, &self.replacement, None),
            _ => 0,
        }
    }

    pub fn set_status(&mut self, status: String) {
        self.status = Some(status);
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        texts: &Texts,
        selection: &Range<usize>,
    ) -> Option<FindAction> {
        let mut action = None;
        let shift = ui.input(|i| i.modifiers.shift);
        let mut set = |a| action = Some(a);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let field = ui.add(
                TextEdit::singleline(&mut self.query.pattern)
                    .id_salt("pilcrow-find")
                    .hint_text(texts.find_label)
                    .desired_width(280.0),
            );
            if std::mem::take(&mut self.focus_field) {
                field.request_focus();
            }
            if field.changed() {
                set(FindAction::Changed);
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                set(if shift {
                    FindAction::Previous
                } else {
                    FindAction::Next
                });
                field.request_focus();
            }

            let toggles = [
                (&mut self.query.match_case, "Aa", texts.match_case),
                (&mut self.query.whole_word, "\\b", texts.whole_word),
                (&mut self.query.regex, ".*", texts.regex),
                (&mut self.query.extended, "\\n", texts.extended),
            ];
            for (value, label, hint) in toggles {
                if ui.toggle_value(value, label).on_hover_text(hint).changed() {
                    set(FindAction::Changed);
                }
            }
            if self.query.regex
                && ui
                    .toggle_value(&mut self.query.dot_matches_newline, ".↵")
                    .on_hover_text(texts.dot_newline)
                    .changed()
            {
                set(FindAction::Changed);
            }
            let mut in_selection = self.scope.is_some();
            if ui
                .toggle_value(&mut in_selection, "[…]")
                .on_hover_text(texts.in_selection)
                .changed()
            {
                self.scope = (in_selection && !selection.is_empty()).then(|| selection.clone());
            }
            ui.toggle_value(&mut self.wrap, "⟲")
                .on_hover_text(texts.wrap_around);
            if ui.button("↑").on_hover_text(texts.find_previous).clicked() {
                set(FindAction::Previous);
            }
            if ui.button("↓").on_hover_text(texts.find_next).clicked() {
                set(FindAction::Next);
            }
            self.status_label(ui, texts, selection);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").clicked() {
                    set(FindAction::Close);
                }
                if ui
                    .button(texts.all_documents)
                    .on_hover_text(texts.find_all_tabs)
                    .clicked()
                {
                    set(FindAction::FindAllInTabs);
                }
                if ui
                    .button(texts.find_all_button)
                    .on_hover_text(texts.find_all)
                    .clicked()
                {
                    set(FindAction::FindAll);
                }
            });
        });
        if self.replace {
            ui.horizontal(|ui| {
                let field = ui.add(
                    TextEdit::singleline(&mut self.replacement)
                        .id_salt("pilcrow-replace")
                        .hint_text(texts.replace_label)
                        .desired_width(280.0),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    set(FindAction::Replace);
                    field.request_focus();
                }
                if ui.button(texts.replace_one).clicked() {
                    set(FindAction::Replace);
                }
                if ui.button(texts.replace_all).clicked() {
                    set(FindAction::ReplaceAll);
                }
                if ui.button(texts.replace_all_docs).clicked() {
                    set(FindAction::ReplaceAllInTabs);
                }
            });
        }
        ui.add_space(2.0);
        action
    }

    fn status_label(&self, ui: &mut Ui, texts: &Texts, selection: &Range<usize>) {
        if let Some(status) = &self.status {
            ui.label(status);
            return;
        }
        let Some(cache) = &self.cache else {
            return;
        };
        match &cache.search {
            Err(err) => {
                ui.colored_label(ui.visuals().error_fg_color, texts.invalid_regex)
                    .on_hover_text(err.to_string());
            }
            Ok(None) => {}
            Ok(Some(_)) if cache.matches.is_empty() => {
                ui.colored_label(ui.visuals().warn_fg_color, texts.no_matches);
            }
            Ok(Some(_)) => {
                let total = cache.matches.len().to_string();
                let text = match match_index(&cache.matches, selection) {
                    Some(index) => fill(
                        texts.match_position,
                        &[("current", &(index + 1).to_string()), ("total", &total)],
                    ),
                    None => fill(texts.match_count, &[("total", &total)]),
                };
                ui.label(text);
            }
        }
    }

    /// Runs an action on one document. `FindAll*` are handled by the caller.
    pub fn run(
        &mut self,
        action: FindAction,
        tab: u64,
        doc: &mut Document,
        editor: &mut EditorState,
        texts: &Texts,
    ) {
        self.status = None;
        let selection = editor.selection();
        let wrap = self.wrap;
        match action {
            FindAction::Close => {
                self.open = false;
                self.scope = None;
            }
            FindAction::FindAll | FindAction::FindAllInTabs | FindAction::ReplaceAllInTabs => {}
            FindAction::Changed => {
                self.select(tab, doc, editor, |m| next_match(m, selection.start, wrap));
            }
            FindAction::Next => {
                self.select(tab, doc, editor, |m| next_match(m, selection.end, wrap));
            }
            FindAction::Previous => {
                self.select(tab, doc, editor, |m| prev_match(m, selection.start, wrap));
            }
            FindAction::Replace => {
                let is_match = match_index(self.matches(tab, doc), &selection).is_some();
                if let Some(search) = self.search().filter(|_| is_match)
                    && let Some(after) = search.replace_at(doc, selection, &self.replacement)
                {
                    editor.set_cursor(after);
                }
                let pos = editor.selection().end;
                self.select(tab, doc, editor, |m| next_match(m, pos, wrap));
            }
            FindAction::ReplaceAll => {
                self.matches(tab, doc);
                if let Some(search) = self.search() {
                    let count = search.replace_all(doc, &self.replacement, self.scope.clone());
                    editor.set_cursor(selection.start.min(doc.text().len_chars()));
                    self.status = Some(fill(texts.replaced, &[("count", &count.to_string())]));
                    // The replacements changed the text the scope referred to.
                    self.scope = None;
                }
            }
        }
    }

    fn select(
        &mut self,
        tab: u64,
        doc: &Document,
        editor: &mut EditorState,
        pick: impl FnOnce(&[Range<usize>]) -> Option<usize>,
    ) {
        let matches = self.matches(tab, doc);
        if let Some(found) = pick(matches).map(|index| matches[index].clone()) {
            editor.set_selection(found.start, found.end);
        }
    }
}
