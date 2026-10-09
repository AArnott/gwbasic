//! Editor inputs. The window maps physical keys onto this enum; tests send it directly.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Char(u8),
    Up,
    Down,
    Left,
    Right,
    WordLeft,
    WordRight,
    Backspace,
    Delete,
    Insert,
    Tab,
    Escape,
    CtrlEnd,
    End,
    Home,
    CtrlHome,
    Enter,
    CtrlEnter,
    /// Ctrl+Break. Stops a running program.
    Break,
    /// Ctrl+C. At the editor and at `INPUT` this abandons the line. A running
    /// program ignores it; GW-BASIC does not treat Ctrl+C as Break.
    CtrlC,
    Bell,
    Pause,
    CtrlT,
    Function(u8),
    PrintScreen,
    ToggleEcho,
}

/// Ctrl+letter editor chords from the GW-BASIC User's Guide, chapter 4.
pub fn ctrl_letter(c: char) -> Option<Input> {
    match c.to_ascii_lowercase() {
        'b' => Some(Input::WordLeft),
        'c' => Some(Input::CtrlC),
        'e' => Some(Input::CtrlEnd),
        'f' => Some(Input::WordRight),
        'g' => Some(Input::Bell),
        'h' => Some(Input::Backspace),
        'j' => Some(Input::CtrlEnter),
        'k' => Some(Input::Home),
        'l' => Some(Input::CtrlHome),
        'm' => Some(Input::Enter),
        'n' => Some(Input::End),
        'r' => Some(Input::Insert),
        's' => Some(Input::Pause),
        't' => Some(Input::CtrlT),
        '6' => Some(Input::Up),
        '-' => Some(Input::Down),
        ']' => Some(Input::Left),
        '\\' => Some(Input::Right),
        '[' => Some(Input::Escape),
        _ => None,
    }
}

pub fn char_to_byte(c: char) -> Option<u8> {
    if c == '\u{2190}' {
        Some(0x1B)
    } else if c >= ' ' && (c as u32) < 128 {
        Some(c as u8)
    } else if (c as u32) < 256 && c >= ' ' {
        Some(c as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_chords_match_the_manual() {
        assert_eq!(ctrl_letter('C'), Some(Input::CtrlC));
        assert_eq!(ctrl_letter('m'), Some(Input::Enter));
        assert_eq!(ctrl_letter('['), Some(Input::Escape));
        assert_eq!(ctrl_letter('6'), Some(Input::Up));
        assert_eq!(ctrl_letter('a'), None);
    }
}
