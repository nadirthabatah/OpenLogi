use super::*;
use crate::spoken::{assert_agrees, assert_listenable};

/// Everything this subcommand can print is read aloud, including the advice
/// that only appears when something has gone wrong — which is exactly the
/// text least likely to be looked at on screen.
#[test]
fn every_fixed_sentence_is_listenable() {
    assert_listenable(WHAT_IT_IS, "the description of a TourBox");
    assert_agrees(WHAT_IT_IS, "the description of a TourBox");
    for line in NOTHING_FOUND_ADVICE {
        assert_listenable(line, "the advice for finding no TourBox");
        assert_agrees(line, "the advice for finding no TourBox");
    }
    let silence = nothing_heard_advice();
    assert_listenable(&silence, "the advice for hearing nothing");
    assert_agrees(&silence, "the advice for hearing nothing");
}

/// The cable comes first on purpose. A charge-only cable was the actual
/// cause the first time this project met a TourBox, and it presents as a
/// controller that does not exist rather than as a cable that does not work.
#[test]
fn the_advice_names_the_cable_before_anything_else() {
    let first = NOTHING_FOUND_ADVICE
        .first()
        .expect("there is advice to give");
    assert!(first.contains("cable"), "the first thing said was: {first}");
}

/// A device that will not open is almost always another program holding it,
/// and the operating system's own error never says so.
#[test]
fn an_unopenable_port_names_the_program_that_is_probably_holding_it() {
    let error = SerialError::Open {
        path: "/dev/cu.usbmodem000000011".to_owned(),
        reason: "Resource busy".to_owned(),
    };
    let advice = unreachable_advice(&error);
    assert!(advice.contains("TourBox Console"), "{advice}");
    assert!(advice.contains("/dev/cu.usbmodem000000011"), "{advice}");
    assert_listenable(&advice, "the advice for a port that will not open");
}

/// Counts and verbs have to agree in both directions. One TourBox "is"
/// attached; two "are". This is the shape that shipped wrong once before on
/// this project, so it is pinned rather than trusted.
#[test]
fn the_count_of_attached_controllers_agrees_with_its_verb() {
    let one = format!("{} attached:", counted(1, "TourBox is", "TourBoxes are"));
    assert_eq!(one, "1 TourBox is attached:");
    assert_agrees(&one, "one attached controller");

    let two = format!("{} attached:", counted(2, "TourBox is", "TourBoxes are"));
    assert_eq!(two, "2 TourBoxes are attached:");
    assert_agrees(&two, "two attached controllers");
}

/// The quiet-timeout sentence is built from a number too, and a one-second
/// timeout must not say "1 seconds".
#[test]
fn the_quiet_timeout_agrees_with_its_noun() {
    assert_eq!(counted(1, "second", "seconds"), "1 second");
    assert_eq!(counted(30, "second", "seconds"), "30 seconds");
}
