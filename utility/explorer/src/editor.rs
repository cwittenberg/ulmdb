//! A byte-preserving value editor. The cursor always sits on a UTF-8 boundary.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt::Write;
use unicode_width::UnicodeWidthStr;

pub const MAX_VALUE_BYTES: usize = 65_536;
const MAX_INPUT_BYTES: usize = MAX_VALUE_BYTES * 3;

pub struct Editor {
    pub text: String,
    pub cursor: usize,
    pub hex: bool,
    pub error: Option<String>,
    pub scroll: u16,
    pub horizontal: u16,
    preferred_column: Option<usize>,
}

impl Editor {
    pub fn new(value: &[u8], prefer_hex: bool) -> Self {
        let plain = std::str::from_utf8(value)
            .ok()
            .filter(|text| text.chars().all(plain_character));
        let hex = prefer_hex || plain.is_none();
        Self {
            text: if hex {
                encode_hex(value)
            } else {
                plain.unwrap_or_default().to_owned()
            },
            cursor: 0,
            hex,
            error: None,
            scroll: 0,
            horizontal: 0,
            preferred_column: None,
        }
    }

    /// Decode without changing editor state. Empty values are valid.
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        if !self.hex {
            if self.text.len() > MAX_VALUE_BYTES {
                return Err("Value exceeds the 64 KiB editing limit.".to_owned());
            }
            if !self.text.chars().all(plain_character) {
                return Err("Text contains hidden control characters. Use Hex mode.".to_owned());
            }
            return Ok(self.text.as_bytes().to_vec());
        }
        let mut bytes = Vec::new();
        let mut high = None;
        for character in self.text.chars() {
            if character.is_ascii_whitespace() {
                continue;
            }
            let digit = character
                .to_digit(16)
                .filter(|_| character.is_ascii())
                .ok_or_else(|| "Hex mode accepts hexadecimal digits and spaces only.".to_owned())?
                as u8;
            if let Some(first) = high.take() {
                if bytes.len() == MAX_VALUE_BYTES {
                    return Err("Value exceeds the 64 KiB editing limit.".to_owned());
                }
                bytes.push((first << 4) | digit);
            } else {
                high = Some(digit);
            }
        }
        if high.is_some() {
            return Err("Incomplete hex byte. Enter two digits for each byte.".to_owned());
        }
        Ok(bytes)
    }

    /// Ctrl+S and Esc belong to the app, not to the text buffer.
    pub fn key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.normalize_cursor();
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('j' | 'J' | 'm' | 'M') if control => self.insert("\n"),
            KeyCode::Char('u' | 'U') if control => {
                self.text.clear();
                self.cursor = 0;
                self.scroll = 0;
                self.horizontal = 0;
                self.edited();
            }
            KeyCode::F(2) => self.toggle_format(),
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if plain_character(character) && character != '\n' {
                    let mut encoded = [0; 4];
                    self.insert(character.encode_utf8(&mut encoded));
                } else {
                    self.error =
                        Some("Hidden control characters must be entered in Hex mode.".to_owned());
                }
            }
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Tab => self.insert("    "),
            KeyCode::Backspace => {
                if self.cursor != 0 {
                    let previous = previous_boundary(&self.text, self.cursor);
                    self.text.drain(previous..self.cursor);
                    self.cursor = previous;
                    self.edited();
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.text.len() {
                    let next = next_boundary(&self.text, self.cursor);
                    self.text.drain(self.cursor..next);
                    self.edited();
                }
            }
            KeyCode::Left => {
                self.cursor = previous_boundary(&self.text, self.cursor);
                self.preferred_column = None;
            }
            KeyCode::Right => {
                self.cursor = next_boundary(&self.text, self.cursor);
                self.preferred_column = None;
            }
            KeyCode::Up => self.vertical(false),
            KeyCode::Down => self.vertical(true),
            KeyCode::PageUp | KeyCode::PageDown => {
                for _ in 0..10 {
                    self.vertical(key.code == KeyCode::PageDown);
                }
            }
            KeyCode::Home => {
                self.cursor = if control { 0 } else { self.line_start() };
                self.preferred_column = None;
            }
            KeyCode::End => {
                self.cursor = if control {
                    self.text.len()
                } else {
                    self.line_end()
                };
                self.preferred_column = None;
            }
            _ => {}
        }
    }

    /// Accept a whole bracketed paste, preserving newlines and Unicode bytes.
    /// Reject unsafe text atomically so hidden bytes never enter the buffer.
    pub fn paste(&mut self, text: &str) {
        self.normalize_cursor();
        if !text.chars().all(plain_character) {
            self.error = Some(
                "Paste contains hidden control characters. Enter those bytes in Hex mode."
                    .to_owned(),
            );
            return;
        }
        self.insert(text);
    }

    /// Zero-based line and terminal display column, rather than byte column.
    pub fn cursor_position(&self) -> (usize, usize) {
        let cursor = boundary_at_or_before(&self.text, self.cursor);
        let before = &self.text[..cursor];
        let row = before.bytes().filter(|byte| *byte == b'\n').count();
        let line = before.rsplit_once('\n').map_or(before, |(_, line)| line);
        (row, line.width())
    }

    fn insert(&mut self, text: &str) {
        if self.text.len().saturating_add(text.len()) > MAX_INPUT_BYTES {
            self.error =
                Some("Editor input is full. The decoded value may be at most 64 KiB.".to_owned());
            return;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.edited();
    }

    fn edited(&mut self) {
        self.error = None;
        self.preferred_column = None;
    }

    fn toggle_format(&mut self) {
        let bytes = match self.bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let text = if self.hex {
            match std::str::from_utf8(&bytes) {
                Ok(text) if text.chars().all(plain_character) => text.to_owned(),
                _ => {
                    self.error = Some(
                        "This value contains binary or hidden control bytes. Keep Hex mode."
                            .to_owned(),
                    );
                    return;
                }
            }
        } else {
            encode_hex(&bytes)
        };
        self.text = text;
        self.hex = !self.hex;
        self.cursor = 0;
        self.scroll = 0;
        self.horizontal = 0;
        self.edited();
    }

    fn normalize_cursor(&mut self) {
        self.cursor = boundary_at_or_before(&self.text, self.cursor);
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |index| self.cursor + index)
    }

    fn vertical(&mut self, down: bool) {
        let (_, column) = self.cursor_position();
        let goal = *self.preferred_column.get_or_insert(column);
        let (start, end) = if down {
            let end = self.line_end();
            if end == self.text.len() {
                return;
            }
            let start = end + 1;
            let end = self.text[start..]
                .find('\n')
                .map_or(self.text.len(), |offset| start + offset);
            (start, end)
        } else {
            let start = self.line_start();
            if start == 0 {
                return;
            }
            let end = start - 1;
            let start = self.text[..end].rfind('\n').map_or(0, |offset| offset + 1);
            (start, end)
        };
        self.cursor = start + offset_at_column(&self.text[start..end], goal);
    }
}

fn plain_character(character: char) -> bool {
    character == '\n'
        || (!character.is_control()
            && !matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}

fn boundary_at_or_before(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map_or(0, |(offset, _)| offset)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .chars()
        .next()
        .map_or(cursor, |character| cursor + character.len_utf8())
}

fn offset_at_column(line: &str, goal: usize) -> usize {
    if line.width() <= goal {
        return line.len();
    }
    // Prefix widths handle combining characters and emoji sequences. Binary
    // search avoids quadratic rescans on a long, unwrapped text value.
    let boundaries: Vec<_> = line.char_indices().map(|(offset, _)| offset).collect();
    let index = boundaries.partition_point(|&offset| line[..offset].width() <= goal);
    boundaries[index.saturating_sub(1)]
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(3));
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            text.push(if index % 16 == 0 { '\n' } else { ' ' });
        }
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(editor: &mut Editor, code: KeyCode) {
        editor.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(editor.text.is_char_boundary(editor.cursor));
    }

    #[test]
    fn empty_and_multiline_values_roundtrip_without_loss() {
        for value in [
            b"".as_slice(),
            b"first\nsecond\n",
            "hello 🌱\n世界".as_bytes(),
        ] {
            let mut editor = Editor::new(value, false);
            assert!(!editor.hex);
            assert_eq!(editor.bytes().unwrap(), value);
            key(&mut editor, KeyCode::F(2));
            assert!(editor.hex);
            assert_eq!(editor.bytes().unwrap(), value);
            key(&mut editor, KeyCode::F(2));
            assert!(!editor.hex);
            assert_eq!(editor.bytes().unwrap(), value);
        }
    }

    #[test]
    fn binary_and_hidden_controls_use_hex_and_cannot_be_lossily_toggled() {
        for value in [
            b"\xff\0\x1b[2J".as_slice(),
            b"a\tb",
            b"a\rb",
            "a\u{202e}b".as_bytes(),
        ] {
            let mut editor = Editor::new(value, false);
            assert!(editor.hex);
            assert_eq!(editor.bytes().unwrap(), value);
            let text = editor.text.clone();
            key(&mut editor, KeyCode::F(2));
            assert!(editor.hex);
            assert!(editor.error.is_some());
            assert_eq!(editor.text, text);
            assert_eq!(editor.bytes().unwrap(), value);
        }
    }

    #[test]
    fn edits_remove_complete_unicode_characters_and_report_display_columns() {
        let mut editor = Editor::new("a界🌱".as_bytes(), false);
        key(&mut editor, KeyCode::End);
        assert_eq!(editor.cursor_position(), (0, 5));
        key(&mut editor, KeyCode::Backspace);
        assert_eq!(editor.text, "a界");
        key(&mut editor, KeyCode::Left);
        assert_eq!(editor.cursor_position(), (0, 1));
        key(&mut editor, KeyCode::Delete);
        assert_eq!(editor.text, "a");
        key(&mut editor, KeyCode::Char('é'));
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Char('🌱'));
        assert_eq!(editor.text, "aé\n🌱");
        assert_eq!(editor.cursor_position(), (1, 2));
        assert_eq!(editor.bytes().unwrap(), "aé\n🌱".as_bytes());
    }

    #[test]
    fn vertical_movement_keeps_the_preferred_display_column() {
        let mut editor = Editor::new("abcd\nx\n界界z\n".as_bytes(), false);
        editor.cursor = 3;
        key(&mut editor, KeyCode::Down);
        assert_eq!(editor.cursor_position(), (1, 1));
        key(&mut editor, KeyCode::Down);
        assert_eq!(editor.cursor_position(), (2, 2));
        key(&mut editor, KeyCode::Up);
        assert_eq!(editor.cursor_position(), (1, 1));
        key(&mut editor, KeyCode::Up);
        assert_eq!(editor.cursor_position(), (0, 3));
        key(&mut editor, KeyCode::Home);
        assert_eq!(editor.cursor_position(), (0, 0));
        editor.key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
        assert_eq!(editor.cursor_position(), (3, 0));
    }

    #[test]
    fn combining_marks_and_emoji_sequences_have_terminal_display_widths() {
        let mut editor = Editor::new("e\u{301}👩‍💻\n界x".as_bytes(), false);
        key(&mut editor, KeyCode::End);
        assert_eq!(editor.cursor_position(), (0, 3));
        key(&mut editor, KeyCode::Down);
        assert_eq!(editor.cursor_position(), (1, 3));
        key(&mut editor, KeyCode::Up);
        assert_eq!(editor.cursor_position(), (0, 3));
    }

    #[test]
    fn hex_accepts_whitespace_and_rejects_incomplete_or_invalid_bytes() {
        let mut editor = Editor::new(&[], true);
        editor.text = "00 ff\n41 42".to_owned();
        assert_eq!(editor.bytes().unwrap(), [0, 255, 65, 66]);
        for malformed in ["f", "gg", "00 zz", "aé", "０１"] {
            editor.text = malformed.to_owned();
            assert!(editor.bytes().is_err());
            key(&mut editor, KeyCode::F(2));
            assert!(editor.hex);
            assert_eq!(editor.text, malformed);
            assert!(editor.error.is_some());
        }
    }

    #[test]
    fn limits_are_checked_and_clear_allows_a_new_empty_value() {
        let mut editor = Editor::new(&vec![b'a'; MAX_VALUE_BYTES], false);
        assert_eq!(editor.bytes().unwrap().len(), MAX_VALUE_BYTES);
        key(&mut editor, KeyCode::Char('b'));
        assert!(editor.bytes().is_err());
        editor.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(editor.text.is_empty());
        assert_eq!(editor.cursor, 0);
        assert!(editor.bytes().unwrap().is_empty());
        assert!(editor.error.is_none());

        editor.text = "a".repeat(MAX_INPUT_BYTES);
        key(&mut editor, KeyCode::Char('z'));
        assert_eq!(editor.text.len(), MAX_INPUT_BYTES);
        assert!(editor.error.is_some());

        let oversized_hex = Editor::new(&vec![255; MAX_VALUE_BYTES + 1], true);
        assert!(oversized_hex.bytes().is_err());
    }

    #[test]
    fn nonediting_shortcuts_release_events_and_controls_do_not_insert_bytes() {
        let mut editor = Editor::new(b"value", false);
        editor.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        key(&mut editor, KeyCode::Esc);
        key(&mut editor, KeyCode::Char('\x1b'));
        assert_eq!(editor.bytes().unwrap(), b"value");
        editor.key(KeyEvent::new_with_kind(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(editor.bytes().unwrap(), b"value");
        key(&mut editor, KeyCode::Tab);
        assert_eq!(editor.text, "    value");
    }

    #[test]
    fn multiline_paste_preserves_exact_unicode_and_inserts_at_the_cursor() {
        let mut editor = Editor::new("前後".as_bytes(), false);
        key(&mut editor, KeyCode::Right);
        editor.paste("first\n世界 🌱\nlast");
        assert_eq!(editor.text, "前first\n世界 🌱\nlast後");
        assert_eq!(
            editor.bytes().unwrap(),
            "前first\n世界 🌱\nlast後".as_bytes()
        );
        assert_eq!(editor.cursor_position(), (2, 4));
        assert!(editor.text.is_char_boundary(editor.cursor));
        assert_eq!(&editor.text[editor.cursor..], "後");
    }

    #[test]
    fn unsafe_or_oversized_paste_leaves_the_original_text_and_cursor() {
        let mut editor = Editor::new(b"before after", false);
        editor.cursor = 6;
        for unsafe_text in [
            "ok\x1b[2Jhidden",
            "ok\tindent",
            "ok\rreturn",
            "ok\u{202e}bidi",
            "ok\u{2066}isolate",
        ] {
            editor.paste(unsafe_text);
            assert_eq!(editor.text, "before after");
            assert_eq!(editor.cursor, 6);
            assert!(editor.error.is_some());
        }
        editor.paste(&"a".repeat(MAX_INPUT_BYTES));
        assert_eq!(editor.text, "before after");
        assert_eq!(editor.cursor, 6);
        assert!(editor.error.is_some());
        editor.paste("\n🌱\n");
        assert_eq!(editor.text, "before\n🌱\n after");
        assert!(editor.error.is_none());
    }

    #[test]
    fn control_j_and_control_m_insert_newlines() {
        let mut editor = Editor::new(b"tail", false);
        for character in ['j', 'm'] {
            editor.key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL,
            ));
        }
        assert_eq!(editor.bytes().unwrap(), b"\n\ntail");
        assert_eq!(editor.cursor_position(), (2, 0));
    }
}
