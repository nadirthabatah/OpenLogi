//! QMK keycodes, and the names people actually use for them.
//!
//! A keymap read back as `0x0068` tells nobody anything. Read back as `F13` it
//! is a key you can reason about, say out loud, and write down. This module is
//! the difference between a diagnostic dump and a tool someone can use without
//! looking at a keyboard.
//!
//! # What is here and what is deliberately not
//!
//! The values below are the USB HID keyboard usage page, which QMK's basic
//! keycodes are numerically identical to. That is a published standard, so
//! these are transcription rather than guesswork.
//!
//! Media and consumer keys, layer-switching codes, and QMK's quantum keycodes
//! are **not** here. They are real and worth adding, but their numbering is
//! QMK's own rather than a standard's, and a wrong entry in this table would
//! rename a key that is not what it claims — or, worse, be written back. An
//! unnamed keycode renders as its number, which is honest; a misnamed one is
//! not. Adding them is a matter of checking against firmware, not of guessing
//! harder.

/// Keycode meaning "no key": the position does nothing.
pub const NONE: u16 = 0x0000;

/// Keycode meaning "fall through to the layer below".
pub const TRANSPARENT: u16 = 0x0001;

/// Named keycodes, as `(keycode, name)`.
///
/// Written out rather than computed from ranges. The ranges *are* contiguous
/// — the letters, the digits, the function keys — but a table you can read
/// down and check against a keyboard is worth more here than four clever loops
/// whose off-by-one would rename every key after it.
const NAMED: &[(u16, &str)] = &[
    (NONE, "NO"),
    (TRANSPARENT, "TRANSPARENT"),
    (0x0004, "A"),
    (0x0005, "B"),
    (0x0006, "C"),
    (0x0007, "D"),
    (0x0008, "E"),
    (0x0009, "F"),
    (0x000a, "G"),
    (0x000b, "H"),
    (0x000c, "I"),
    (0x000d, "J"),
    (0x000e, "K"),
    (0x000f, "L"),
    (0x0010, "M"),
    (0x0011, "N"),
    (0x0012, "O"),
    (0x0013, "P"),
    (0x0014, "Q"),
    (0x0015, "R"),
    (0x0016, "S"),
    (0x0017, "T"),
    (0x0018, "U"),
    (0x0019, "V"),
    (0x001a, "W"),
    (0x001b, "X"),
    (0x001c, "Y"),
    (0x001d, "Z"),
    (0x001e, "1"),
    (0x001f, "2"),
    (0x0020, "3"),
    (0x0021, "4"),
    (0x0022, "5"),
    (0x0023, "6"),
    (0x0024, "7"),
    (0x0025, "8"),
    (0x0026, "9"),
    (0x0027, "0"),
    (0x0028, "ENTER"),
    (0x0029, "ESCAPE"),
    (0x002a, "BACKSPACE"),
    (0x002b, "TAB"),
    (0x002c, "SPACE"),
    (0x002d, "MINUS"),
    (0x002e, "EQUAL"),
    (0x002f, "LEFT_BRACKET"),
    (0x0030, "RIGHT_BRACKET"),
    (0x0031, "BACKSLASH"),
    (0x0033, "SEMICOLON"),
    (0x0034, "QUOTE"),
    (0x0035, "GRAVE"),
    (0x0036, "COMMA"),
    (0x0037, "DOT"),
    (0x0038, "SLASH"),
    (0x0039, "CAPS_LOCK"),
    (0x003a, "F1"),
    (0x003b, "F2"),
    (0x003c, "F3"),
    (0x003d, "F4"),
    (0x003e, "F5"),
    (0x003f, "F6"),
    (0x0040, "F7"),
    (0x0041, "F8"),
    (0x0042, "F9"),
    (0x0043, "F10"),
    (0x0044, "F11"),
    (0x0045, "F12"),
    (0x0046, "PRINT_SCREEN"),
    (0x0047, "SCROLL_LOCK"),
    (0x0048, "PAUSE"),
    (0x0049, "INSERT"),
    (0x004a, "HOME"),
    (0x004b, "PAGE_UP"),
    (0x004c, "DELETE"),
    (0x004d, "END"),
    (0x004e, "PAGE_DOWN"),
    (0x004f, "RIGHT"),
    (0x0050, "LEFT"),
    (0x0051, "DOWN"),
    (0x0052, "UP"),
    (0x0068, "F13"),
    (0x0069, "F14"),
    (0x006a, "F15"),
    (0x006b, "F16"),
    (0x006c, "F17"),
    (0x006d, "F18"),
    (0x006e, "F19"),
    (0x006f, "F20"),
    (0x0070, "F21"),
    (0x0071, "F22"),
    (0x0072, "F23"),
    (0x0073, "F24"),
    (0x00e0, "LEFT_CTRL"),
    (0x00e1, "LEFT_SHIFT"),
    (0x00e2, "LEFT_ALT"),
    (0x00e3, "LEFT_GUI"),
    (0x00e4, "RIGHT_CTRL"),
    (0x00e5, "RIGHT_SHIFT"),
    (0x00e6, "RIGHT_ALT"),
    (0x00e7, "RIGHT_GUI"),
];

/// The name of a keycode, when this build knows one.
#[must_use]
pub fn name(keycode: u16) -> Option<&'static str> {
    NAMED
        .iter()
        .find_map(|&(code, name)| (code == keycode).then_some(name))
}

/// How a keycode is written for a person: its name, or its number.
///
/// Never fails, and never lies by omission — an unnamed keycode renders as the
/// hex a firmware reference can be checked against, which is a usable answer
/// rather than a shrug.
#[must_use]
pub fn describe(keycode: u16) -> String {
    name(keycode).map_or_else(|| format!("{keycode:#06x}"), str::to_owned)
}

/// The keycode for a name, matched case-insensitively and with an optional
/// `KC_` prefix, so all of `F13`, `f13` and `KC_F13` work.
///
/// Being forgiving here is not mere convenience. Names arrive dictated, typed
/// from memory, or pasted out of QMK documentation that writes them one way
/// while VIA writes them another; refusing `KC_F13` because the table stores
/// `F13` would be a puzzle with no clue attached.
#[must_use]
pub fn parse(name: &str) -> Option<u16> {
    let wanted = name.trim();
    let wanted = wanted
        .strip_prefix("KC_")
        .or_else(|| wanted.strip_prefix("kc_"))
        .unwrap_or(wanted);
    NAMED
        .iter()
        .find_map(|&(code, known)| known.eq_ignore_ascii_case(wanted).then_some(code))
}

#[cfg(test)]
mod tests {
    use super::{NAMED, NONE, TRANSPARENT, describe, name, parse};

    #[test]
    fn the_letters_land_where_hid_puts_them() {
        assert_eq!(name(0x0004), Some("A"));
        assert_eq!(name(0x001d), Some("Z"));
    }

    /// The gap between F12 and F13 is the one place this table is not
    /// contiguous, and it is exactly where a computed range would go wrong.
    #[test]
    fn the_function_keys_span_the_gap_correctly() {
        assert_eq!(name(0x0045), Some("F12"));
        assert_eq!(name(0x0068), Some("F13"));
        assert_eq!(name(0x0073), Some("F24"));
        assert_eq!(name(0x0046), Some("PRINT_SCREEN"), "not F13");
    }

    #[test]
    fn a_keycode_with_no_name_is_still_described_usefully() {
        assert_eq!(name(0x00ff), None);
        assert_eq!(describe(0x00ff), "0x00ff");
    }

    #[test]
    fn a_named_keycode_is_described_by_its_name() {
        assert_eq!(describe(0x0004), "A");
        assert_eq!(describe(NONE), "NO");
        assert_eq!(describe(TRANSPARENT), "TRANSPARENT");
    }

    /// Names arrive dictated, typed from memory, or pasted from documentation
    /// that spells them differently. Refusing a spelling that means the same
    /// key is a puzzle with no clue attached.
    #[test]
    fn a_name_parses_however_it_is_spelled() {
        for spelling in ["F13", "f13", "KC_F13", "kc_f13", "  F13  "] {
            assert_eq!(parse(spelling), Some(0x0068), "{spelling} did not parse");
        }
    }

    #[test]
    fn an_unknown_name_is_refused_rather_than_guessed() {
        assert_eq!(parse("SUPERKEY"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("KC_"), None);
    }

    #[test]
    fn every_name_round_trips() {
        for &(code, known) in NAMED {
            assert_eq!(parse(known), Some(code), "{known} did not round trip");
            assert_eq!(name(code), Some(known));
        }
    }

    /// Two entries sharing a keycode or a name would make the lookups pick by
    /// table order, which is not a rule anyone could predict.
    #[test]
    fn the_table_has_no_duplicates() {
        for (index, &(code, known)) in NAMED.iter().enumerate() {
            for &(other_code, other_name) in &NAMED[..index] {
                assert_ne!(code, other_code, "{known} and {other_name} share a keycode");
                assert!(
                    !known.eq_ignore_ascii_case(other_name),
                    "{known} appears twice"
                );
            }
        }
    }
}
