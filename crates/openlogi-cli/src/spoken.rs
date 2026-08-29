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

/// An argument as it must be typed to survive a shell.
///
/// A command this program prints is a command someone copies. A layout called
/// "my deck" echoed bare produces `openlogi streamdeck apply my deck`, which
/// the shell splits into two arguments and the program then rejects — and the
/// person is left arguing with an instruction the program itself gave them.
///
/// Single quotes because they are literal in every POSIX shell; an argument
/// containing one has that one spliced, which is the standard way and survives
/// being pasted.
#[must_use]
pub fn shell_argument(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ',' | ':' | '=' | '@')
        });
    if plain {
        return argument.to_owned();
    }
    format!("'{}'", argument.replace('\'', r"'\''"))
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
    " that point ",
    " which run ",
    " which are ",
    " which were ",
    " which have ",
    " which point ",
    " were accepted",
    " were not ",
    " were found",
    " are not ",
    " have been ",
];

/// Clauses that a plural count in front of them makes ungrammatical.
///
/// The mirror of [`DISAGREEING`], and the one I wrote immediately after fixing
/// its cases: having got the verb to agree with the count, the *next* clause
/// in the same sentence still said "it". Both directions are the same mistake
/// — a sentence tested at one length and shipped at the other.
const DISAGREEING_PLURAL: &[&str] = &[
    " it points ",
    " it is ",
    " it was ",
    " it has ",
    " it does ",
    " which is ",
    " that is not among",
];

/// Fail if a count and the words after it disagree in number.
///
/// Checks both directions: a count of one followed by a plural verb, and a
/// count of more than one followed by a singular clause. A sentence is
/// normally written and tested at one of the two lengths, so whichever the
/// author did not try is the one that ships broken — and read aloud it does
/// not merely look untidy, it arrives as a sentence that does not parse.
///
/// # Panics
///
/// Whenever a line's count and its verbs disagree.
pub fn assert_agrees(text: &str, what: &str) {
    for line in text.lines() {
        let Some((count, rest)) = leading_count(line) else {
            continue;
        };
        let (patterns, how) = if count == 1 {
            (DISAGREEING, "a count of one")
        } else {
            (DISAGREEING_PLURAL, "a count of more than one")
        };
        for pattern in patterns {
            assert!(
                !rest.contains(pattern),
                "{what} says {pattern:?} after {how}, which reads as a broken \
                 sentence: {line}"
            );
        }
    }
}

/// The first count in a line, and everything after it.
///
/// Returns `None` for a line with no count in it, and skips a number that is
/// part of a longer one — "11 devices" is not the singular case.
fn leading_count(line: &str) -> Option<(u32, &str)> {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        // A number glued to a letter or a dot is a version, an id or a
        // measurement rather than a count of something.
        let is_a_count = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        if is_a_count
            && bytes.get(at) == Some(&b' ')
            && let Ok(count) = line[start..at].parse::<u32>()
        {
            return Some((count, &line[at..]));
        }
    }
    None
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
    use super::{assert_agrees, assert_listenable, counted, leading_count, shell_argument};

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

    /// Both directions of the mistake, each taken from output that shipped.
    #[test]
    fn a_count_that_disagrees_with_its_verbs_is_caught() {
        let broken = [
            "  1 action that run a program or type text were accepted:",
            "  the 1 device you cannot open are not among them",
            "  2 linked folders were not followed, because it points somewhere else",
        ];
        for line in broken {
            let caught = std::panic::catch_unwind(|| assert_agrees(line, "a probe"));
            assert!(caught.is_err(), "not caught: {line}");
        }
    }

    /// And the sentences that are fine have to stay fine, or the check gets
    /// switched off by whoever it wakes up at three in the morning.
    #[test]
    fn correct_sentences_are_left_alone() {
        let fine = [
            "  accepted 1 action that would run a program or type text:",
            "  the attached HID device can be opened",
            "  all 3 attached HID devices can be opened",
            "  1 device you cannot open is not among them",
            "  2 devices you cannot open are not among them",
            "  applied 11 keys from deck.toml",
            "  speaks VIA protocol 9, with 1 keymap layer",
            "  1 of 1 attached device can be configured by this build.",
        ];
        for line in fine {
            assert_agrees(line, "a probe");
        }
    }

    /// A number that is part of a word is not a count of anything.
    #[test]
    fn a_version_or_an_id_is_not_read_as_a_count() {
        assert_eq!(leading_count("no numbers here"), None);
        assert_eq!(leading_count("1 device"), Some((1, " device")));
        assert_eq!(leading_count("11 devices"), Some((11, " devices")));
        // "046d:4082" is an id; the digits glued to it must not read as one.
        assert_eq!(leading_count("MX Master 3S (046d:4082)"), None);
    }

    /// A command this program prints is a command someone copies. One that
    /// the shell then splits is worse than no instruction at all, because the
    /// person argues with it before doubting it.
    #[test]
    fn a_name_needing_quotes_gets_them() {
        assert_eq!(shell_argument("streaming"), "streaming");
        assert_eq!(shell_argument("my-deck.2"), "my-deck.2");
        assert_eq!(shell_argument("/home/me/deck.toml"), "/home/me/deck.toml");
        assert_eq!(shell_argument("my deck"), "'my deck'");
        assert_eq!(shell_argument(""), "''");
        assert_eq!(shell_argument("\u{65e5}\u{672c}"), "'\u{65e5}\u{672c}'");
    }

    /// The characters a shell would act on must not reach it unquoted.
    #[test]
    fn a_name_carrying_shell_syntax_is_made_inert() {
        for hostile in ["a;rm -rf ~", "a$(id)", "a`id`", "a|b", "a&b", "a>b", "a*b"] {
            let quoted = shell_argument(hostile);
            assert!(quoted.starts_with('\''), "{hostile:?} -> {quoted}");
            assert!(quoted.ends_with('\''), "{hostile:?} -> {quoted}");
        }
    }

    /// A quote inside the name is the one case simple quoting gets wrong.
    #[test]
    fn a_name_containing_a_quote_is_spliced_not_broken() {
        assert_eq!(shell_argument("it's"), r"'it'\''s'");
    }
}
