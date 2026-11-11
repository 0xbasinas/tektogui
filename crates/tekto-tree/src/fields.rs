//! Text-field editing: keyboard text, IME, selection, and the field-key reducer.
use super::*;

impl Tree {
    pub(crate) fn clear_field_preedit(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        if node.kind != NodeKind::TextField {
            return;
        }
        node.preedit.clear();
        node.preedit_cursor = None;
    }

    /// Printable keyboard text for the focused text field.
    ///
    /// This is the desktop `KeyboardInput` path. Returns `Ok(false)` when
    /// focus is not a text field, `text` has no printable characters, or a
    /// preedit is showing. A preedit is left as it is (not committed, not
    /// cleared). [`Tree::insert_text`] is the agent path and does drop a preedit.
    pub fn type_keyboard_text(&mut self, text: &str) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self
            .get(id)
            .is_some_and(|n| n.kind != NodeKind::TextField || n.disabled)
        {
            return Ok(false);
        }
        if self.get(id).is_some_and(|n| !n.preedit.is_empty()) {
            return Ok(false);
        }
        let multiline = self.get(id).is_some_and(|node| node.multiline);
        let plain: String = text
            .chars()
            .filter(|c| !c.is_control() || (multiline && *c == '\n'))
            .collect();
        if plain.is_empty() {
            return Ok(false);
        }
        for ch in plain.chars() {
            self.apply_on_field(id, FieldKey::Insert(ch))?;
        }
        Ok(true)
    }

    /// Insert plain text at the caret of a text field and focus it.
    ///
    /// Control characters are skipped. An in-progress preedit is dropped
    /// (not committed). Composition goes through [`Tree::apply_ime`].
    pub fn insert_text(&mut self, id: NodeId, text: &str) -> Result<(), TreeError> {
        self.layout_dirty = true;
        self.ensure_field(id)?;
        self.set_focus(id)?;
        if let Some(node) = self.nodes.get_mut(&id) {
            node.preedit.clear();
            node.preedit_cursor = None;
        }
        let multiline = self.get(id).is_some_and(|node| node.multiline);
        for ch in text
            .chars()
            .filter(|c| !c.is_control() || (multiline && *c == '\n'))
        {
            self.apply_on_field(id, FieldKey::Insert(ch))?;
        }
        Ok(())
    }

    /// One key aimed at the focused text field.
    ///
    /// Returns `Ok(false)` when nothing is focused or focus is not a text field.
    pub fn apply_field_key(&mut self, key: FieldKey) -> Result<bool, TreeError> {
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self
            .get(id)
            .is_some_and(|n| n.kind != NodeKind::TextField || n.disabled)
        {
            return Ok(false);
        }
        // The input method owns the keys while a composition is on screen.
        if self.get(id).is_some_and(|n| !n.preedit.is_empty()) {
            return Ok(false);
        }
        self.apply_on_field(id, key)?;
        Ok(true)
    }

    /// Replace the focused field's committed value. Used by the Android overlay.
    ///
    /// Control characters are dropped. A preedit is cleared and not committed.
    /// Returns `Ok(false)` when focus is not a text field or nothing changed.
    pub fn set_focused_field_value(&mut self, text: &str, caret: usize) -> Result<bool, TreeError> {
        self.layout_dirty = true;
        self.reset_caret_blink();
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self
            .get(id)
            .is_none_or(|n| n.kind != NodeKind::TextField || n.disabled)
        {
            return Ok(false);
        }
        let multiline = self.get(id).is_some_and(|node| node.multiline);
        let plain: String = text
            .chars()
            .filter(|c| !c.is_control() || (multiline && *c == '\n'))
            .collect();
        let caret = caret.min(plain.chars().count());
        let node = self.nodes.get_mut(&id).expect("field");
        if node.text.as_deref().unwrap_or("") == plain
            && node.caret == caret
            && node.preedit.is_empty()
            && node.preedit_cursor.is_none()
            && node.selection_anchor.is_none()
        {
            return Ok(false);
        }
        node.preedit.clear();
        node.preedit_cursor = None;
        node.text = Some(plain);
        node.caret = caret;
        node.vertical_goal_x = None;
        node.selection_anchor = None;
        node.selection_x = None;
        self.emit(TreeEvent::Changed(id));
        Ok(true)
    }

    /// Apply one IME event to the focused text field.
    ///
    /// Returns `Ok(false)` when nothing is focused, focus is not a text field,
    /// or the event does not change the field (`Enabled`, or a repeated preedit).
    /// `Preedit` replaces the composition string. `Commit` inserts into the
    /// committed value and clears the preedit. `Disabled` drops an unfinished
    /// preedit without inserting it.
    pub fn apply_ime(&mut self, event: ImeEvent) -> Result<bool, TreeError> {
        self.layout_dirty = true;
        self.reset_caret_blink();
        let Some(id) = self.focus else {
            return Ok(false);
        };
        if self
            .get(id)
            .is_some_and(|n| n.kind != NodeKind::TextField || n.disabled)
        {
            return Ok(false);
        }
        self.ensure_field(id)?;
        match event {
            ImeEvent::Enabled => Ok(false),
            ImeEvent::Disabled => {
                let node = self.nodes.get_mut(&id).expect("field");
                if node.preedit.is_empty() && node.preedit_cursor.is_none() {
                    return Ok(false);
                }
                node.preedit.clear();
                node.preedit_cursor = None;
                Ok(true)
            }
            ImeEvent::Preedit { text, cursor } => {
                let cursor = sanitize_preedit_cursor(&text, cursor);
                let node = self.nodes.get_mut(&id).expect("field");
                node.vertical_goal_x = None;
                if node.preedit == text && node.preedit_cursor == cursor {
                    return Ok(false);
                }
                node.preedit = text;
                node.preedit_cursor = cursor;
                Ok(true)
            }
            ImeEvent::Commit(text) => {
                let node = self.nodes.get_mut(&id).expect("field");
                let had_preedit = !node.preedit.is_empty() || node.preedit_cursor.is_some();
                node.preedit.clear();
                node.preedit_cursor = None;
                let multiline = node.multiline;
                let plain: String = text
                    .chars()
                    .filter(|c| !c.is_control() || (multiline && *c == '\n'))
                    .collect();
                if plain.is_empty() {
                    return Ok(had_preedit);
                }
                for ch in plain.chars() {
                    self.apply_on_field(id, FieldKey::Insert(ch))?;
                }
                Ok(true)
            }
        }
    }

    fn ensure_field(&self, id: NodeId) -> Result<(), TreeError> {
        match self.nodes.get(&id) {
            Some(node) if node.disabled => Err(TreeError {
                message: format!("node {id} is disabled"),
            }),
            Some(node) if node.kind == NodeKind::TextField => Ok(()),
            Some(_) => Err(TreeError {
                message: format!("node {id} is not a text field"),
            }),
            None => Err(TreeError {
                message: format!("unknown node {id}"),
            }),
        }
    }

    fn apply_on_field(&mut self, id: NodeId, key: FieldKey) -> Result<(), TreeError> {
        self.layout_dirty = true;
        self.reset_caret_blink();
        let node = self.nodes.get_mut(&id).ok_or_else(|| TreeError {
            message: format!("unknown node {id}"),
        })?;
        if node.kind != NodeKind::TextField {
            return Err(TreeError {
                message: format!("node {id} is not a text field"),
            });
        }
        let current = node.text.clone().unwrap_or_default();
        node.vertical_goal_x = None;
        let mut chars: Vec<char> = current.chars().collect();
        let boundaries = grapheme_boundaries(&current);
        let snap = |at: usize| {
            boundaries
                .iter()
                .copied()
                .take_while(|b| *b <= at)
                .last()
                .unwrap_or(0)
        };
        let mut caret = snap(node.caret.min(chars.len()));
        node.caret = caret;
        node.selection_anchor = node.selection_anchor.map(snap);
        let selection = node.selection();
        let extending = matches!(
            key,
            FieldKey::SelectLeft
                | FieldKey::SelectRight
                | FieldKey::SelectHome
                | FieldKey::SelectEnd
                | FieldKey::SelectAll
        );
        if extending && node.selection_anchor.is_none() {
            node.selection_anchor = Some(caret);
        }
        let previous = |at: usize| {
            boundaries
                .iter()
                .copied()
                .take_while(|b| *b < at)
                .last()
                .unwrap_or(0)
        };
        let next = |at: usize| {
            boundaries
                .iter()
                .copied()
                .find(|b| *b > at)
                .unwrap_or(chars.len())
        };
        match key {
            FieldKey::Insert(ch) => {
                if ch.is_control() && !(node.multiline && ch == '\n') {
                    return Ok(());
                }
                if let Some((start, end)) = selection {
                    chars.drain(start..end);
                    caret = start;
                }
                chars.insert(caret, ch);
                caret += 1;
            }
            FieldKey::Backspace => {
                let (start, end) = selection.unwrap_or((previous(caret), caret));
                if start < end {
                    chars.drain(start..end);
                    caret = start;
                }
            }
            FieldKey::Delete => {
                let (start, end) = selection.unwrap_or((caret, next(caret)));
                if start < end {
                    chars.drain(start..end);
                    caret = start;
                }
            }
            FieldKey::Left => caret = selection.map_or_else(|| previous(caret), |(start, _)| start),
            FieldKey::Right => caret = selection.map_or_else(|| next(caret), |(_, end)| end),
            FieldKey::Home | FieldKey::SelectHome => {
                caret = if node.multiline {
                    chars[..caret]
                        .iter()
                        .rposition(|ch| *ch == '\n')
                        .map_or(0, |index| index + 1)
                } else {
                    0
                }
            }
            FieldKey::End | FieldKey::SelectEnd => {
                caret = if node.multiline {
                    chars[caret..]
                        .iter()
                        .position(|ch| *ch == '\n')
                        .map_or(chars.len(), |index| caret + index)
                } else {
                    chars.len()
                }
            }
            FieldKey::SelectLeft => caret = previous(caret),
            FieldKey::SelectRight => caret = next(caret),
            FieldKey::SelectAll => {
                node.selection_anchor = Some(0);
                caret = chars.len();
            }
        }
        if !extending {
            node.selection_anchor = None;
        }
        node.selection_x = None;
        node.text = Some(chars.into_iter().collect());
        node.caret = caret;
        if current != node.text.as_deref().unwrap_or("") {
            self.emit(TreeEvent::Changed(id));
        }
        Ok(())
    }

    /// Set a selection using Unicode scalar indices, snapped to grapheme boundaries.
    pub fn set_field_selection(
        &mut self,
        id: NodeId,
        anchor: usize,
        focus: usize,
    ) -> Result<(), TreeError> {
        self.ensure_field(id)?;
        self.set_focus(id)?;
        let node = self.nodes.get_mut(&id).expect("field");
        if !node.preedit.is_empty() {
            return Err(TreeError {
                message: "cannot select during IME composition".into(),
            });
        }
        let boundaries = grapheme_boundaries(node.text.as_deref().unwrap_or(""));
        let snap = |index| {
            boundaries
                .iter()
                .copied()
                .take_while(|value| *value <= index)
                .last()
                .unwrap_or(0)
        };
        node.selection_anchor = Some(snap(anchor));
        node.vertical_goal_x = None;
        node.caret = snap(focus);
        node.selection_x = None;
        Ok(())
    }

    /// Return the selected committed text, without changing the clipboard.
    pub fn selected_text(&self, id: NodeId) -> Result<String, TreeError> {
        self.ensure_field(id)?;
        let node = &self.nodes[&id];
        let Some((start, end)) = node.selection() else {
            return Ok(String::new());
        };
        Ok(node
            .text
            .as_deref()
            .unwrap_or("")
            .chars()
            .skip(start)
            .take(end - start)
            .collect())
    }
}
