/// One reversible change: at char index `at`, `removed` was replaced by `inserted`.
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    pub id: u64,
    pub at: usize,
    pub removed: String,
    pub inserted: String,
    /// Undone and redone together with the edit before it.
    pub linked: bool,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    next_id: u64,
    sealed: bool,
}

impl History {
    pub fn top_id(&self) -> Option<u64> {
        self.undo.last().map(|edit| edit.id)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Prevents the next edit from being merged into the previous one.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Records an applied edit. Continued typing or deleting merges into the previous edit,
    /// unless that edit is `protected` (it marks the saved state).
    pub fn record(&mut self, at: usize, removed: String, inserted: String, protected: Option<u64>) {
        self.redo.clear();
        let sealed = std::mem::take(&mut self.sealed);
        if !sealed
            && let Some(last) = self.undo.last_mut()
            && Some(last.id) != protected
            && try_merge(last, at, &removed, &inserted)
        {
            return;
        }
        self.push(at, removed, inserted, false);
    }

    /// Records an applied edit that belongs to the same undo step as the previous one.
    pub fn record_linked(&mut self, at: usize, removed: String, inserted: String) {
        self.redo.clear();
        self.push(at, removed, inserted, true);
    }

    fn push(&mut self, at: usize, removed: String, inserted: String, linked: bool) {
        let id = self.next_id;
        self.next_id += 1;
        self.undo.push(Edit {
            id,
            at,
            removed,
            inserted,
            linked,
        });
    }

    /// The edits of the last undo step, most recent first.
    pub fn undo(&mut self) -> Vec<Edit> {
        let mut edits = Vec::new();
        while let Some(edit) = self.undo.pop() {
            let linked = edit.linked;
            self.redo.push(edit.clone());
            edits.push(edit);
            if !linked {
                break;
            }
        }
        self.sealed = true;
        edits
    }

    /// The edits of the last undone step, in their original order.
    pub fn redo(&mut self) -> Vec<Edit> {
        let mut edits = Vec::new();
        while let Some(edit) = self.redo.pop() {
            self.undo.push(edit.clone());
            edits.push(edit);
            if !self.redo.last().is_some_and(|next| next.linked) {
                break;
            }
        }
        self.sealed = true;
        edits
    }
}

fn try_merge(last: &mut Edit, at: usize, removed: &str, inserted: &str) -> bool {
    let typing = removed.is_empty()
        && last.removed.is_empty()
        && is_mergeable_char(inserted)
        && at == last.at + last.inserted.chars().count()
        && !(ends_with_whitespace(&last.inserted) && !inserted.starts_with(char::is_whitespace));
    if typing {
        last.inserted.push_str(inserted);
        return true;
    }

    if inserted.is_empty() && last.inserted.is_empty() && is_mergeable_char(removed) {
        if at + 1 == last.at {
            last.removed.insert_str(0, removed);
            last.at = at;
            return true;
        }
        if at == last.at {
            last.removed.push_str(removed);
            return true;
        }
    }
    false
}

fn is_mergeable_char(text: &str) -> bool {
    let mut chars = text.chars();
    matches!((chars.next(), chars.next()), (Some(ch), None) if ch != '\n')
}

fn ends_with_whitespace(text: &str) -> bool {
    text.ends_with(char::is_whitespace)
}
