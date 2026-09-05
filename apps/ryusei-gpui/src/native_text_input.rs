//! Reusable Unicode-safe editing state for native GPUI input elements.
//!
//! The model owns text, selection and undo history. Views decide when to
//! commit the current value to a host transaction; platform input handlers can
//! feed composition text through `insert_text` without knowing that contract.

use std::ops::Range;

use gpui_kit::gpui;

const MAX_HISTORY: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKeyResult {
    Changed,
    Submit,
    Cancel,
    Ignored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InputSnapshot {
    text: String,
    selection: Range<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeTextInput {
    text: String,
    selection: Range<usize>,
    undo: Vec<InputSnapshot>,
    redo: Vec<InputSnapshot>,
    /// Character-index range of the current IME composition (marked text).
    /// The marked text is present in `text`; this range lets a later
    /// `set_marked_text`/`insert_text` call replace it instead of appending,
    /// which is what previously duplicated every composed keystroke.
    marked_range: Option<Range<usize>>,
}

impl NativeTextInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.chars().count();
        Self {
            text,
            selection: end..end,
            undo: Vec::new(),
            redo: Vec::new(),
            marked_range: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        let end = self.text.chars().count();
        self.selection = end..end;
        self.undo.clear();
        self.redo.clear();
        self.marked_range = None;
    }

    pub fn select_all(&mut self) {
        self.selection = 0..self.text.chars().count();
    }

    pub fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.push_undo();
        let start = self.byte_index(self.selection.start);
        let end = self.byte_index(self.selection.end);
        self.text.replace_range(start..end, text);
        let cursor = self.selection.start + text.chars().count();
        self.selection = cursor..cursor;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.selection.start != self.selection.end {
            return self.replace_selection("");
        }
        if self.selection.start == 0 {
            return false;
        }
        self.push_undo();
        let start_char = self.selection.start - 1;
        let start = self.byte_index(start_char);
        let end = self.byte_index(self.selection.start);
        self.text.replace_range(start..end, "");
        self.selection = start_char..start_char;
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.selection.start != self.selection.end {
            return self.replace_selection("");
        }
        let end_char = self.text.chars().count();
        if self.selection.end >= end_char {
            return false;
        }
        self.push_undo();
        let start = self.byte_index(self.selection.start);
        let end = self.byte_index(self.selection.start + 1);
        self.text.replace_range(start..end, "");
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.selection.start == 0 {
            return false;
        }
        let cursor = self.selection.start - 1;
        self.selection = cursor..cursor;
        true
    }

    pub fn move_right(&mut self) -> bool {
        let end = self.text.chars().count();
        if self.selection.end >= end {
            return false;
        }
        let cursor = self.selection.end + 1;
        self.selection = cursor..cursor;
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.push_redo();
        self.restore(previous);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.push_undo();
        self.restore(next);
        true
    }

    pub fn handle_key(&mut self, key: &str, key_char: Option<&str>) -> InputKeyResult {
        match key {
            "backspace" => self.backspace().then_some(InputKeyResult::Changed),
            "delete" => self.delete_forward().then_some(InputKeyResult::Changed),
            "left" => self.move_left().then_some(InputKeyResult::Changed),
            "right" => self.move_right().then_some(InputKeyResult::Changed),
            "enter" => Some(InputKeyResult::Submit),
            "escape" => Some(InputKeyResult::Cancel),
            _ => key_char
                .filter(|text| !text.is_empty())
                .and_then(|text| self.insert_text(text).then_some(InputKeyResult::Changed)),
        }
        .unwrap_or(InputKeyResult::Ignored)
    }

    pub fn utf16_selection(&self) -> Range<usize> {
        self.utf16_index(self.selection.start)..self.utf16_index(self.selection.end)
    }

    pub fn text_for_utf16_range(&self, range: Range<usize>) -> String {
        let start = self.character_index_for_utf16(range.start);
        let end = self.character_index_for_utf16(range.end);
        self.text
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    #[allow(dead_code)]
    pub fn replace_utf16_range(&mut self, range: Option<Range<usize>>, text: &str) {
        let range = range
            .map(|range| {
                self.character_index_for_utf16(range.start)
                    ..self.character_index_for_utf16(range.end)
            })
            .unwrap_or_else(|| self.selection.clone());
        self.selection = range;
        if text.is_empty() {
            self.replace_selection("");
        } else {
            self.insert_text(text);
        }
    }

    /// Replaces any existing composition with `text`, marking it for the next
    /// `set_marked_text` / `insert_text` update. `range` is UTF-16, as the
    /// platform bridge supplies.
    pub fn replace_marked_text(&mut self, range: Option<Range<usize>>, text: &str) {
        // Drop the previous composition first so repeated `set_marked_text`
        // calls replace it instead of appending a duplicate.
        if let Some(marked) = self.marked_range.take() {
            let start = self.byte_index(marked.start);
            let end = self.byte_index(marked.end);
            self.text.replace_range(start..end, "");
            self.selection = marked.start..marked.start;
        }
        let char_range = range
            .map(|range| {
                self.character_index_for_utf16(range.start)
                    ..self.character_index_for_utf16(range.end)
            })
            .unwrap_or_else(|| self.selection.clone());
        self.push_undo();
        let start = self.byte_index(char_range.start);
        let end = self.byte_index(char_range.end);
        self.text.replace_range(start..end, text);
        let cursor = char_range.start + text.chars().count();
        self.selection = cursor..cursor;
        self.marked_range = Some(char_range.start..cursor);
    }

    /// Commits the composition as real text (macOS `insertText:`).
    pub fn commit_marked_text(&mut self, range: Option<Range<usize>>, text: &str) {
        let marked = self.marked_range.take();
        let char_range = range
            .map(|range| {
                self.character_index_for_utf16(range.start)
                    ..self.character_index_for_utf16(range.end)
            })
            .or(marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        self.push_undo();
        let start = self.byte_index(char_range.start);
        let end = self.byte_index(char_range.end);
        self.text.replace_range(start..end, text);
        let cursor = char_range.start + text.chars().count();
        self.selection = cursor..cursor;
    }

    pub fn unmark_text(&mut self) {
        self.marked_range = None;
    }

    pub fn marked_text_utf16_range(&self) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.utf16_index(range.start)..self.utf16_index(range.end))
    }

    fn replace_selection(&mut self, text: &str) -> bool {
        if self.selection.start == self.selection.end && text.is_empty() {
            return false;
        }
        self.push_undo();
        let start = self.byte_index(self.selection.start);
        let end = self.byte_index(self.selection.end);
        self.text.replace_range(start..end, text);
        let cursor = self.selection.start + text.chars().count();
        self.selection = cursor..cursor;
        true
    }

    fn byte_index(&self, character_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(character_index)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len())
    }

    fn utf16_index(&self, character_index: usize) -> usize {
        self.text
            .chars()
            .take(character_index)
            .map(char::len_utf16)
            .sum()
    }

    fn character_index_for_utf16(&self, utf16_index: usize) -> usize {
        let mut consumed = 0;
        for (character_index, character) in self.text.chars().enumerate() {
            let next = consumed + character.len_utf16();
            if utf16_index < next {
                return character_index;
            }
            if utf16_index == next {
                return character_index + 1;
            }
            consumed = next;
        }
        self.text.chars().count()
    }

    fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
        }
    }

    fn restore(&mut self, snapshot: InputSnapshot) {
        self.text = snapshot.text;
        self.selection = snapshot.selection;
    }

    fn push_undo(&mut self) {
        if self.undo.len() == MAX_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    fn push_redo(&mut self) {
        if self.redo.len() == MAX_HISTORY {
            self.redo.remove(0);
        }
        self.redo.push(self.snapshot());
    }
}

/// A zero-visual element that attaches GPUI's platform input bridge to a
/// focused entity during paint. The surrounding view supplies the visual box.
pub struct NativeInputBinding<V: gpui::EntityInputHandler> {
    focus_handle: gpui::FocusHandle,
    view: gpui::Entity<V>,
}

impl<V: gpui::EntityInputHandler> NativeInputBinding<V> {
    pub fn new(focus_handle: gpui::FocusHandle, view: gpui::Entity<V>) -> Self {
        Self { focus_handle, view }
    }
}

impl<V: gpui::EntityInputHandler> gpui::Element for NativeInputBinding<V> {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        // Fill the surrounding input box so `bounds_for_range` reports the real
        // caret rectangle instead of a zero-sized one (which hid the cursor).
        let style = gpui::Style {
            size: gpui::Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        window.set_focus_handle(&self.focus_handle, cx);
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        window.handle_input(
            &self.focus_handle,
            gpui::ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}

impl<V: gpui::EntityInputHandler> gpui::IntoElement for NativeInputBinding<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{InputKeyResult, NativeTextInput};

    #[test]
    fn edits_unicode_by_character_not_utf8_byte() {
        let mut input = NativeTextInput::new("A围");
        assert!(input.backspace());
        assert_eq!(input.text(), "A");
        assert!(input.insert_text("棋"));
        assert_eq!(input.text(), "A棋");
    }

    #[test]
    fn selection_replaces_and_undoes() {
        let mut input = NativeTextInput::new("comment");
        input.select_all();
        assert!(input.insert_text("note"));
        assert_eq!(input.text(), "note");
        assert!(input.undo());
        assert_eq!(input.text(), "comment");
        assert!(input.redo());
        assert_eq!(input.text(), "note");
    }

    #[test]
    fn platform_utf16_ranges_replace_surrogate_pairs_safely() {
        let mut input = NativeTextInput::new("A😀围");
        // `😀` occupies UTF-16 indices 1..3, unlike its UTF-8 byte range.
        input.replace_utf16_range(Some(1..3), "棋");
        assert_eq!(input.text(), "A棋围");
        assert_eq!(input.text_for_utf16_range(1..2), "棋");
        assert_eq!(input.utf16_selection(), 2..2);
    }

    #[test]
    fn key_dispatch_preserves_submit_cancel_and_navigation() {
        let mut input = NativeTextInput::new("ab");
        assert_eq!(input.handle_key("left", None), InputKeyResult::Changed);
        assert_eq!(input.handle_key("x", Some("棋")), InputKeyResult::Changed);
        assert_eq!(input.text(), "a棋b");
        assert_eq!(input.handle_key("enter", None), InputKeyResult::Submit);
        assert_eq!(input.handle_key("escape", None), InputKeyResult::Cancel);
        assert_eq!(input.handle_key("tab", None), InputKeyResult::Ignored);
    }
}
