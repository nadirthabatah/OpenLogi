//! Writing output that is as good to listen to as it is to look at.
//!
//! This project's accessibility rule is that the command line is a first-class
//! interface rather than a fallback: for someone who cannot see the screen it
//! is often the only one. That rule is easy to state and easy to erode, because
//! nobody sets out to make output unlistenable — they write `{n} device(s)`
//! because it is shorter, and a screen reader says "device open paren s close
//! paren" for the rest of the program's life.
//!
//! So the two halves of the rule live here: a helper that makes the right thing
//! the easy thing, and, in tests, a checker that sweeps rendered output for the
//! patterns that break it. The checker matters more than it looks. Output that
//! needs hardware to produce cannot be swept by running the program on a
//! machine with none, so the sweep has to be reachable from unit tests that
//! synthesize the text instead.

/// A count with the right noun for it.
///
/// `counted(1, "device", "devices")` is `"1 device"`. The alternative most
/// often reached for, `"{n} device(s)"`, reads fine and is spoken as "device
/// open paren s close paren" — every time, in every line, to the people who
/// most rely on this interface.
#[must_use]
pub fn counted(how_many: usize, one: &str, many: &str) -> String {
    if how_many == 1 {
        format!("{how_many} {one}")
    } else {
        format!("{how_many} {many}")
    }
}

/// Patterns that make terminal output worse to hear than to read.
pub const UNLISTENABLE: &[(&str, &str)] = &[
    (
        "(s)",
        "a screen reader says \"open paren s close paren\"; use spoken::counted",
    ),
    (
        "\u{2500}",
        "box-drawing characters are read out one per character",
    ),
    ("\u{2502}", "box-drawing characters"),
    ("\u{250c}", "box-drawing characters"),
    ("\u{2514}", "box-drawing characters"),
    (
        "\u{2588}",
        "block characters are read out one per character",
    ),
    (
        "\u{2713}",
        "a tick alone carries the meaning; say the word too",
    ),
    ("\u{2714}", "a tick alone carries the meaning"),
    ("\u{2717}", "a cross alone carries the meaning"),
    ("\u{2718}", "a cross alone carries the meaning"),
    ("\u{274c}", "a cross alone carries the meaning"),
    ("\u{2705}", "a tick alone carries the meaning"),
];

/// Verbs that a singular count in front of them makes ungrammatical.
///
/// `counted` gets the noun right and stops there, so "1 action that run a
/// program ... were accepted" is what a count wedged into the middle of a
/// sentence gives — correct-looking in the plural case the author tested, wrong
/// in the singular one they did not. Read aloud it is worse than on the page,
/// because nothing is scannable: the sentence simply arrives broken.
///
/// The fix is always to put the verb before the count, or to fold it into the
/// noun that `counted` chooses. These patterns are what a singular count
/// followed by a plural verb looks like.
const DISAGREEING: &[&str] = &[
    " that run ",
    " that type ",
    " that are ",
    " that were ",
    " that have ",
    " were accepted",
    " are not ",
    " have been ",
    " were found",
];

/// Fail if a singular count is followed by a plural verb.
///
/// Only the "1 " case can be wrong this way, so that is all this looks for.
///
/// # Panics
///
/// Whenever a line starts a count at one and then uses a plural verb.
pub fn assert_agrees(text: &str, what: &str) {
    for line in text.lines() {
        let Some(at) = line.find("1 ") else {
            continue;
        };
        // "11 devices" and "21 devices" are not the singular case.
        if line[..at].ends_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let rest = &line[at..];
        for pattern in DISAGREEING {
            assert!(
                !rest.contains(pattern),
                "{what} says {pattern:?} after a count of one, which reads as \
                 a broken sentence: {line}"
            );
        }
    }
}

/// Fail if `text` carries anything hostile to a screen reader.
///
/// `what` names what produced it, so a failure says which output to fix.
///
/// # Panics
///
/// Whenever `text` carries such a pattern. Panicking is the point: this is a
/// test assertion, and the message names the pattern, why it matters, and the
/// output it came from.
pub fn assert_listenable(text: &str, what: &str) {
    for (pattern, why) in UNLISTENABLE {
        assert!(
            !text.contains(pattern),
            "{what} contains {pattern:?} — {why}\n{text}"
        );
    }
    for line in text.lines() {
        let trimmed = line.trim();
        assert!(
            trimmed.len() < 8
                || !trimmed
                    .chars()
                    .all(|c| matches!(c, '=' | '-' | '*' | '_' | '~' | '#')),
            "{what} draws a rule, {line:?}, which is heard as repeated punctuation or \
             as nothing at all\n{text}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{assert_listenable, counted};

    #[test]
    fn one_takes_the_singular_and_everything_else_the_plural() {
        assert_eq!(counted(1, "device", "devices"), "1 device");
        assert_eq!(counted(0, "device", "devices"), "0 devices");
        assert_eq!(counted(2, "device", "devices"), "2 devices");
        assert_eq!(counted(11, "layout", "layouts"), "11 layouts");
    }

    #[test]
    fn ordinary_prose_passes() {
        assert_listenable(
            "3 devices found — 1 of them can be configured.\nRun openlogi doctor.",
            "a test string",
        );
    }

    /// The checker has to actually catch things, or it is decoration that
    /// makes everyone feel the rule is being kept.
    #[test]
    #[should_panic(expected = "open paren s close paren")]
    fn a_parenthesised_plural_is_caught() {
        assert_listenable("found 1 device(s)", "a test string");
    }

    #[test]
    #[should_panic(expected = "draws a rule")]
    fn a_drawn_rule_is_caught() {
        assert_listenable("Heading\n========\nbody", "a test string");
    }

    #[test]
    #[should_panic(expected = "box-drawing")]
    fn a_box_drawing_character_is_caught() {
        assert_listenable("\u{250c}\u{2500}\u{2500} Devices", "a test string");
    }

    #[test]
    #[should_panic(expected = "carries the meaning")]
    fn a_bare_tick_is_caught() {
        assert_listenable("\u{2713} permissions", "a test string");
    }
}
