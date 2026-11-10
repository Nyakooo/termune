use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
pub enum InputField {
    Login(usize),
    AccessCode,
    Search,
    PlaylistName,
}

#[derive(Clone, Copy, Default)]
pub struct Caret {
    position: Option<usize>,
    pub viewport: usize,
}

impl Caret {
    pub fn position(self, text: &str) -> usize {
        self.position.unwrap_or(text.len())
    }

    pub(super) fn place(&mut self, text: &str, offset: usize) {
        // Edits can merge adjacent graphemes, so advance to the next valid boundary.
        self.position = text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index >= offset).then_some(index));
    }

    pub(super) fn insert(&mut self, text: &mut String, value: &str) {
        let position = self.position(text);
        text.insert_str(position, value);
        self.place(text, position + value.len());
    }

    pub(super) fn key(&mut self, text: &mut String, key: KeyEvent) {
        let position = self.position(text);
        let previous = text[..position]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index);
        let next = position + text[position..].graphemes(true).next().map_or(0, str::len);
        match key.code {
            KeyCode::Left => self.place(text, previous),
            KeyCode::Right => self.place(text, next),
            KeyCode::Home => self.place(text, 0),
            KeyCode::End => self.position = None,
            KeyCode::Backspace => {
                text.replace_range(previous..position, "");
                self.place(text, previous);
            }
            KeyCode::Delete => {
                text.replace_range(position..next, "");
                self.place(text, position);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                text.clear();
                *self = Self::default();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                self.insert(text, c.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }
}
