use crossterm::event::{KeyEvent, KeyModifiers};

pub(crate) fn resolved_text_char(key: &KeyEvent, character: char) -> char {
    if key.modifiers.contains(KeyModifiers::SHIFT) && character.is_ascii_lowercase() {
        character.to_ascii_uppercase()
    } else {
        character
    }
}

pub(crate) fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    text[..cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

pub(crate) fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor >= text.len() {
        return text.len();
    }

    cursor
        + text[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::{next_char_boundary, previous_char_boundary, resolved_text_char};

    #[test]
    fn uppercases_lowercase_ascii_chars_when_shift_is_present() {
        let key = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(resolved_text_char(&key, 'a'), 'A');
    }

    #[test]
    fn leaves_non_letters_unchanged() {
        let key = KeyEvent {
            code: KeyCode::Char('1'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(resolved_text_char(&key, '1'), '1');
    }

    #[test]
    fn previous_char_boundary_steps_back_one_character() {
        assert_eq!(previous_char_boundary("aßc", 3), 1);
    }

    #[test]
    fn next_char_boundary_steps_forward_one_character() {
        assert_eq!(next_char_boundary("aßc", 1), 3);
    }
}
