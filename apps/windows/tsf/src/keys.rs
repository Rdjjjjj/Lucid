//! 把虚拟键译成 Lucid 认识的文字。不读取键盘钩子，也不合成按键。

pub const DUPLICATE_WINDOW_MS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Insert(char),
    Backspace,
    Return,
    Escape,
    Ignore,
}

pub fn action_for_virtual_key(
    virtual_key: u32,
    character: Option<char>,
    control_down: bool,
) -> KeyAction {
    if control_down {
        return KeyAction::Ignore;
    }
    match virtual_key {
        0x08 => KeyAction::Backspace,
        0x0d => KeyAction::Return,
        0x1b => KeyAction::Escape,
        _ => match character {
            Some('\u{8}') | Some('\u{7f}') => KeyAction::Backspace,
            Some('\n') | Some('\r') => KeyAction::Return,
            Some('\u{1b}') => KeyAction::Escape,
            Some(character) if is_insertable(character) => KeyAction::Insert(character),
            _ => KeyAction::Ignore,
        },
    }
}

pub fn is_insertable(character: char) -> bool {
    let value = character as u32;
    value >= 0x20 && value != 0x7f && !(0xF700..=0xF8FF).contains(&value)
}

pub fn is_duplicate(
    previous: Option<(String, u32, u64)>,
    text: &str,
    virtual_key: u32,
    now_ms: u64,
) -> bool {
    previous.is_some_and(|(last_text, last_key, at)| {
        last_text == text
            && last_key == virtual_key
            && now_ms.saturating_sub(at) < DUPLICATE_WINDOW_MS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_navigation_are_not_inserted() {
        assert_eq!(
            action_for_virtual_key(0x41, Some('a'), true),
            KeyAction::Ignore
        );
        assert_eq!(action_for_virtual_key(0x25, None, false), KeyAction::Ignore);
        assert_eq!(
            action_for_virtual_key(0x41, Some('a'), false),
            KeyAction::Insert('a')
        );
    }

    #[test]
    fn editing_keys_keep_their_meaning() {
        assert_eq!(
            action_for_virtual_key(0x08, None, false),
            KeyAction::Backspace
        );
        assert_eq!(
            action_for_virtual_key(0x0d, Some('\r'), false),
            KeyAction::Return
        );
        assert_eq!(action_for_virtual_key(0x1b, None, false), KeyAction::Escape);
    }

    #[test]
    fn repeated_key_inside_thirty_milliseconds_is_duplicate() {
        let previous = Some(("a".to_owned(), 0x41, 1_000));
        assert!(is_duplicate(previous.clone(), "a", 0x41, 1_020));
        assert!(!is_duplicate(previous, "a", 0x41, 1_030));
    }
}
