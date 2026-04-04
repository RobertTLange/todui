use crossterm::event::{KeyEvent, KeyModifiers};

pub(crate) fn resolved_text_char(key: &KeyEvent, character: char) -> char {
    if key.modifiers.contains(KeyModifiers::SHIFT) && character.is_ascii_lowercase() {
        character.to_ascii_uppercase()
    } else {
        character
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::resolved_text_char;

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
}
