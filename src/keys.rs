//! Key specifications of the control protocol (`key ctrl+s`, `key code:Backquote`)
//! and the keyboard events a platform reports for them.

use iced::Event;
use iced::keyboard::key::{Code, Named, NativeCode, Physical};
use iced::keyboard::{self, Key, Modifiers};

/// A key press as a platform reports it: `key` without the modifiers,
/// `modified_key` with Shift applied, the physical key, and the text it
/// types, if any.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Keystroke {
    pub(crate) key: Key,
    pub(crate) modified_key: Key,
    pub(crate) physical: Physical,
    pub(crate) modifiers: Modifiers,
    pub(crate) text: Option<char>,
}

/// Named keys, canonical names first, then aliases.
const KEYS: &[(&str, Named)] = &[
    ("enter", Named::Enter),
    ("escape", Named::Escape),
    ("tab", Named::Tab),
    ("backspace", Named::Backspace),
    ("delete", Named::Delete),
    ("insert", Named::Insert),
    ("space", Named::Space),
    ("arrowup", Named::ArrowUp),
    ("arrowdown", Named::ArrowDown),
    ("arrowleft", Named::ArrowLeft),
    ("arrowright", Named::ArrowRight),
    ("home", Named::Home),
    ("end", Named::End),
    ("pageup", Named::PageUp),
    ("pagedown", Named::PageDown),
    ("f1", Named::F1),
    ("f2", Named::F2),
    ("f3", Named::F3),
    ("f4", Named::F4),
    ("f5", Named::F5),
    ("f6", Named::F6),
    ("f7", Named::F7),
    ("f8", Named::F8),
    ("f9", Named::F9),
    ("f10", Named::F10),
    ("f11", Named::F11),
    ("f12", Named::F12),
    ("up", Named::ArrowUp),
    ("down", Named::ArrowDown),
    ("left", Named::ArrowLeft),
    ("right", Named::ArrowRight),
    ("esc", Named::Escape),
    ("return", Named::Enter),
    ("del", Named::Delete),
    ("pgup", Named::PageUp),
    ("pgdown", Named::PageDown),
    ("pgdn", Named::PageDown),
];

/// The physical key each named key in [`KEYS`] comes from.
const NAMED_CODES: &[(Named, Code)] = &[
    (Named::Enter, Code::Enter),
    (Named::Escape, Code::Escape),
    (Named::Tab, Code::Tab),
    (Named::Backspace, Code::Backspace),
    (Named::Delete, Code::Delete),
    (Named::Insert, Code::Insert),
    (Named::Space, Code::Space),
    (Named::ArrowUp, Code::ArrowUp),
    (Named::ArrowDown, Code::ArrowDown),
    (Named::ArrowLeft, Code::ArrowLeft),
    (Named::ArrowRight, Code::ArrowRight),
    (Named::Home, Code::Home),
    (Named::End, Code::End),
    (Named::PageUp, Code::PageUp),
    (Named::PageDown, Code::PageDown),
    (Named::F1, Code::F1),
    (Named::F2, Code::F2),
    (Named::F3, Code::F3),
    (Named::F4, Code::F4),
    (Named::F5, Code::F5),
    (Named::F6, Code::F6),
    (Named::F7, Code::F7),
    (Named::F8, Code::F8),
    (Named::F9, Code::F9),
    (Named::F10, Code::F10),
    (Named::F11, Code::F11),
    (Named::F12, Code::F12),
];

/// `a`..`z`, in alphabet order.
const LETTERS: [Code; 26] = [
    Code::KeyA,
    Code::KeyB,
    Code::KeyC,
    Code::KeyD,
    Code::KeyE,
    Code::KeyF,
    Code::KeyG,
    Code::KeyH,
    Code::KeyI,
    Code::KeyJ,
    Code::KeyK,
    Code::KeyL,
    Code::KeyM,
    Code::KeyN,
    Code::KeyO,
    Code::KeyP,
    Code::KeyQ,
    Code::KeyR,
    Code::KeyS,
    Code::KeyT,
    Code::KeyU,
    Code::KeyV,
    Code::KeyW,
    Code::KeyX,
    Code::KeyY,
    Code::KeyZ,
];

/// `0`..`9`.
const DIGITS: [Code; 10] = [
    Code::Digit0,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
];

/// What Shift types on each digit key, in the order of [`DIGITS`].
const SHIFTED_DIGITS: &str = ")!@#$%^&*(";

/// Unshifted character, shifted character and key of a US layout's punctuation keys.
const PUNCTUATION: &[(char, char, Code)] = &[
    ('`', '~', Code::Backquote),
    ('-', '_', Code::Minus),
    ('=', '+', Code::Equal),
    ('[', '{', Code::BracketLeft),
    (']', '}', Code::BracketRight),
    ('\\', '|', Code::Backslash),
    (';', ':', Code::Semicolon),
    ('\'', '"', Code::Quote),
    (',', '<', Code::Comma),
    ('.', '>', Code::Period),
    ('/', '?', Code::Slash),
];

fn named_code(named: Named) -> Option<Code> {
    NAMED_CODES
        .iter()
        .find(|(n, _)| *n == named)
        .map(|(_, c)| *c)
}

fn code_named(code: Code) -> Option<Named> {
    NAMED_CODES
        .iter()
        .find(|(_, c)| *c == code)
        .map(|(n, _)| *n)
}

/// The key a US layout types `c` on, with or without Shift.
fn us_code(c: char) -> Option<Code> {
    if c.is_ascii_alphabetic() {
        return Some(LETTERS[usize::from(c.to_ascii_lowercase() as u8 - b'a')]);
    }
    if let Some(digit) = c.to_digit(10) {
        return Some(DIGITS[digit as usize]);
    }
    if let Some(digit) = SHIFTED_DIGITS.find(c) {
        return Some(DIGITS[digit]);
    }
    PUNCTUATION
        .iter()
        .find(|(plain, shifted, _)| c == *plain || c == *shifted)
        .map(|(_, _, code)| *code)
}

/// The character a letter or digit key types without Shift.
fn code_char(code: Code) -> Option<char> {
    if let Some(i) = LETTERS.iter().position(|c| *c == code) {
        return Some(char::from(b'a' + i as u8));
    }
    DIGITS
        .iter()
        .position(|c| *c == code)
        .map(|i| char::from(b'0' + i as u8))
}

/// Every key name `key` accepts, comma separated, for help and errors.
pub(crate) fn key_names() -> String {
    let mut names: Vec<&str> = KEYS.iter().map(|(name, _)| *name).collect();
    names.push("code:<Name>");
    names.join(", ")
}

impl Keystroke {
    /// `[ctrl+][shift+][alt+][super+]KEY`, modifiers in any order. `KEY` is
    /// one character, a key name, or `code:<Name>` for a physical key such as
    /// `code:Backquote`.
    pub(crate) fn parse(spec: &str) -> Result<Self, String> {
        let mut modifiers = Modifiers::empty();
        let mut rest = spec;
        'prefixes: loop {
            for (prefix, flag) in [
                ("ctrl+", Modifiers::CTRL),
                ("shift+", Modifiers::SHIFT),
                ("alt+", Modifiers::ALT),
                ("super+", Modifiers::LOGO),
            ] {
                if rest.len() > prefix.len()
                    && rest.is_char_boundary(prefix.len())
                    && rest[..prefix.len()].eq_ignore_ascii_case(prefix)
                {
                    modifiers |= flag;
                    rest = &rest[prefix.len()..];
                    continue 'prefixes;
                }
            }
            break;
        }
        if let Some(name) = rest.strip_prefix("code:") {
            let code = physical_code(name).ok_or_else(|| format!("unknown key code {name:?}"))?;
            return Ok(Self::from_code(code, modifiers));
        }
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(Self::character(c, modifiers)),
            _ => {
                let named = named_key(rest)
                    .ok_or_else(|| format!("unknown key {rest:?}; keys: {}", key_names()))?;
                Ok(Self::named(named, modifiers))
            }
        }
    }

    /// The press of a physical key: named keys and letters or digits carry
    /// their key and text as well, any other key only its code.
    fn from_code(code: Code, modifiers: Modifiers) -> Self {
        if let Some(named) = code_named(code) {
            return Self::named(named, modifiers);
        }
        if let Some(c) = code_char(code) {
            return Self::character(c, modifiers);
        }
        Self {
            key: Key::Unidentified,
            modified_key: Key::Unidentified,
            physical: Physical::Code(code),
            modifiers,
            text: None,
        }
    }

    /// A character key. Shift is applied the way a US layout applies it to
    /// letters; other characters are taken as given. The physical key is the
    /// one a US layout types the character on, if any.
    pub(crate) fn character(c: char, modifiers: Modifiers) -> Self {
        if c == ' ' {
            return Self::named(Named::Space, modifiers);
        }
        let (bare, shifted) = if modifiers.shift() {
            (c.to_ascii_lowercase(), c.to_ascii_uppercase())
        } else {
            (c, c)
        };
        Self {
            key: Key::Character(bare.to_string().into()),
            modified_key: Key::Character(shifted.to_string().into()),
            physical: us_code(c).map_or(
                Physical::Unidentified(NativeCode::Unidentified),
                Physical::Code,
            ),
            modifiers,
            text: types_text(modifiers).then_some(shifted),
        }
    }

    pub(crate) fn named(named: Named, modifiers: Modifiers) -> Self {
        Self {
            key: Key::Named(named),
            modified_key: Key::Named(named),
            physical: named_code(named).map_or(
                Physical::Unidentified(NativeCode::Unidentified),
                Physical::Code,
            ),
            modifiers,
            // Space is the one named key that types something.
            text: (named == Named::Space && types_text(modifiers)).then_some(' '),
        }
    }
}

/// Ctrl, Alt and Super turn a key into a shortcut rather than text.
fn types_text(modifiers: Modifiers) -> bool {
    !(modifiers.control() || modifiers.alt() || modifiers.logo())
}

fn named_key(name: &str) -> Option<Named> {
    KEYS.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, key)| *key)
}

/// A physical key by its `Code` debug name.
fn physical_code(name: &str) -> Option<Code> {
    macro_rules! codes {
        ($($code:ident),* $(,)?) => {
            [$((stringify!($code), Code::$code)),*]
        };
    }
    let table = codes![
        Backquote,
        Backslash,
        BracketLeft,
        BracketRight,
        Comma,
        Equal,
        Minus,
        Period,
        Quote,
        Semicolon,
        Slash,
        CapsLock,
        Space,
        Tab,
        Enter,
        Escape,
        Backspace,
        Delete,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        SuperLeft,
        SuperRight,
        Insert,
        Home,
        End,
        PageUp,
        PageDown,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
    ];
    table.into_iter().find(|(n, _)| *n == name).map(|(_, c)| c)
}

/// The modifier change and press a keyboard reports when a key goes down
/// while the modifiers `held` are already down.
pub(crate) fn key_down_events(keystroke: &Keystroke, held: Modifiers) -> [Event; 2] {
    let modifiers = keystroke.modifiers | held;
    [
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keystroke.key.clone(),
            modified_key: keystroke.modified_key.clone(),
            physical_key: keystroke.physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: keystroke
                .text
                .filter(|_| types_text(modifiers))
                .map(|c| c.to_string().into()),
            repeat: false,
        }),
    ]
}

/// The release and modifier change a keyboard reports when a key goes up,
/// leaving the modifiers `remaining` down.
pub(crate) fn key_up_events(keystroke: &Keystroke, remaining: Modifiers) -> [Event; 2] {
    [
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: keystroke.key.clone(),
            modified_key: keystroke.modified_key.clone(),
            physical_key: keystroke.physical,
            location: keyboard::Location::Standard,
            modifiers: keystroke.modifiers,
        }),
        Event::Keyboard(keyboard::Event::ModifiersChanged(remaining)),
    ]
}

/// A key press and release, bracketed by the modifier changes a keyboard
/// reports around them, while the modifiers `held` are down.
pub(crate) fn key_events(keystroke: &Keystroke, held: Modifiers) -> [Event; 4] {
    let [modifiers, press] = key_down_events(keystroke, held);
    let [release, cleared] = key_up_events(keystroke, held);
    [modifiers, press, release, cleared]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystroke_modifiers_in_any_order() {
        let a = Keystroke::parse("ctrl+shift+k").unwrap();
        let b = Keystroke::parse("Shift+CTRL+k").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.modifiers, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(a.key, Key::Character("k".into()));
        assert_eq!(a.modified_key, Key::Character("K".into()));
        assert_eq!(a.text, None, "a shortcut types nothing");
    }

    #[test]
    fn plain_character_types_text() {
        assert_eq!(Keystroke::parse("a").unwrap().text, Some('a'));
        assert_eq!(Keystroke::parse("shift+a").unwrap().text, Some('A'));
    }

    #[test]
    fn named_keys_and_space() {
        let k = Keystroke::parse("shift+enter").unwrap();
        assert_eq!(k.key, Key::Named(Named::Enter));
        assert_eq!(k.text, None);
        let k = Keystroke::parse("ctrl+space").unwrap();
        assert_eq!(k.key, Key::Named(Named::Space));
        assert_eq!(k.text, None);
        assert_eq!(Keystroke::parse("space").unwrap().text, Some(' '));
    }

    #[test]
    fn aliases_resolve_to_their_named_key() {
        for (alias, named) in [
            ("left", Named::ArrowLeft),
            ("right", Named::ArrowRight),
            ("up", Named::ArrowUp),
            ("down", Named::ArrowDown),
            ("esc", Named::Escape),
            ("return", Named::Enter),
            ("del", Named::Delete),
            ("pgup", Named::PageUp),
            ("pgdown", Named::PageDown),
            ("pgdn", Named::PageDown),
            ("insert", Named::Insert),
        ] {
            let k = Keystroke::parse(&format!("shift+{alias}")).unwrap();
            assert_eq!(k.key, Key::Named(named), "{alias}");
            assert_eq!(k.modifiers, Modifiers::SHIFT, "{alias}");
        }
    }

    #[test]
    fn unknown_key_lists_the_known_names() {
        let e = Keystroke::parse("nokey").unwrap_err();
        assert!(
            e.starts_with("unknown key \"nokey\"; keys: enter, escape"),
            "{e}"
        );
        assert!(e.contains("left") && e.contains("code:<Name>"), "{e}");
    }

    #[test]
    fn physical_code_keys() {
        let k = Keystroke::parse("code:Backquote").unwrap();
        assert_eq!(k.physical, Physical::Code(Code::Backquote));
        assert_eq!(k.text, None);
        assert!(Keystroke::parse("code:Nope").is_err());
    }

    #[test]
    fn characters_carry_their_us_layout_key() {
        let code = |s: &str| Keystroke::parse(s).unwrap().physical;
        for spec in ["a", "A", "shift+a"] {
            assert_eq!(code(spec), Physical::Code(Code::KeyA), "{spec}");
        }
        assert_eq!(code("!"), Physical::Code(Code::Digit1));
        assert_eq!(code("1"), Physical::Code(Code::Digit1));
        assert_eq!(code(")"), Physical::Code(Code::Digit0));
        assert_eq!(code("?"), Physical::Code(Code::Slash));
        assert_eq!(code("/"), Physical::Code(Code::Slash));
        assert_eq!(code("~"), Physical::Code(Code::Backquote));
        assert_eq!(code("\\"), Physical::Code(Code::Backslash));
        assert_eq!(code("é"), Physical::Unidentified(NativeCode::Unidentified));
    }

    #[test]
    fn named_keys_carry_their_code() {
        for (spec, expected) in [
            ("enter", Code::Enter),
            ("pgdn", Code::PageDown),
            ("f12", Code::F12),
            ("space", Code::Space),
        ] {
            assert_eq!(
                Keystroke::parse(spec).unwrap().physical,
                Physical::Code(expected),
                "{spec}"
            );
        }
    }

    #[test]
    fn code_specs_resolve_to_keys() {
        let k = Keystroke::parse("code:KeyA").unwrap();
        assert_eq!(k.key, Key::Character("a".into()));
        assert_eq!(k.text, Some('a'));
        assert_eq!(k.physical, Physical::Code(Code::KeyA));
        let k = Keystroke::parse("shift+code:KeyA").unwrap();
        assert_eq!(k.key, Key::Character("a".into()));
        assert_eq!(k.modified_key, Key::Character("A".into()));
        assert_eq!(k.text, Some('A'));
        assert_eq!(Keystroke::parse("ctrl+code:KeyA").unwrap().text, None);
        let k = Keystroke::parse("code:ArrowLeft").unwrap();
        assert_eq!(k.key, Key::Named(Named::ArrowLeft));
        assert_eq!(k.physical, Physical::Code(Code::ArrowLeft));
        let k = Keystroke::parse("code:Backquote").unwrap();
        assert_eq!(k.key, Key::Unidentified);
    }

    #[test]
    fn named_codes_round_trip_and_cover_every_key() {
        for (named, code) in NAMED_CODES {
            assert_eq!(named_code(*named), Some(*code));
            assert_eq!(code_named(*code), Some(*named));
        }
        for (name, named) in KEYS {
            assert!(named_code(*named).is_some(), "{name} has no code");
        }
    }

    #[test]
    fn held_modifiers_join_presses_and_stay_after_releases() {
        let ctrl_a = Keystroke::parse("ctrl+a").unwrap();
        let [changed, press] = key_down_events(&Keystroke::parse("b").unwrap(), Modifiers::CTRL);
        assert_eq!(
            changed,
            Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::CTRL))
        );
        let Event::Keyboard(keyboard::Event::KeyPressed {
            modifiers, text, ..
        }) = press
        else {
            panic!("not a press");
        };
        assert_eq!((modifiers, text), (Modifiers::CTRL, None));

        let [release, remaining] = key_up_events(&ctrl_a, Modifiers::SHIFT);
        let Event::Keyboard(keyboard::Event::KeyReleased { modifiers, .. }) = release else {
            panic!("not a release");
        };
        assert_eq!(modifiers, Modifiers::CTRL);
        assert_eq!(
            remaining,
            Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT))
        );
    }
}
