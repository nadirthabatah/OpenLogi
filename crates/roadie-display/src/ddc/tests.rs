use std::time::{Duration, Instant};

use roadie_ddc::packet::{ProtocolError, Request};
use roadie_ddc::{Feature, Value};

use super::{Ddc, Pacer, Pacing, Retry, reply_len, retry_for};
use crate::backend::{DisplayError, VcpBackend};
use crate::mock::{Fault, Panel};

/// A panel wired up with no waiting, so the exchange logic can be tested
/// without the tests spending their runtime asleep. The waiting itself is
/// tested separately, against [`Pacer`], with literal durations.
fn unpaced(panel: Panel) -> Ddc<Panel> {
    Ddc::with_pacing(panel, Pacing::NONE)
}

#[test]
fn a_fresh_pacer_owes_nothing() {
    assert_eq!(Pacer::default().wait_at(Instant::now()), Duration::ZERO);
}

#[test]
fn a_pacer_owes_the_remainder_of_the_gap() {
    let start = Instant::now();
    let mut pacer = Pacer::default();
    pacer.finished(start, Duration::from_millis(50));

    // Nothing has elapsed: the whole gap is still owed.
    assert_eq!(pacer.wait_at(start), Duration::from_millis(50));
    // Thirty of the fifty have gone by.
    assert_eq!(
        pacer.wait_at(start + Duration::from_millis(30)),
        Duration::from_millis(20)
    );
}

#[test]
fn a_pacer_owes_nothing_once_the_gap_has_passed() {
    let start = Instant::now();
    let mut pacer = Pacer::default();
    pacer.finished(start, Duration::from_millis(50));

    // Exactly spent, and well past. Neither may come back as a negative wait,
    // which is what `saturating_sub` is there for: the usual case is a caller
    // that did other work between two reads and owes nothing at all.
    assert_eq!(
        pacer.wait_at(start + Duration::from_millis(50)),
        Duration::ZERO
    );
    assert_eq!(
        pacer.wait_at(start + Duration::from_secs(9)),
        Duration::ZERO
    );
}

#[test]
fn the_specified_pacing_is_the_specification_minimums() {
    // Floors from DDC/CI 1.1, written out so a later edit to them is a visible
    // change rather than a silent one.
    assert_eq!(Pacing::SPECIFIED.reply, Duration::from_millis(40));
    assert_eq!(Pacing::SPECIFIED.between, Duration::from_millis(50));
    assert_eq!(Pacing::SPECIFIED.after_write, Duration::from_millis(50));
}

#[test]
fn replies_are_read_at_the_size_of_the_answer() {
    // Framing plus an eight-byte feature payload.
    assert_eq!(reply_len(Request::Get(Feature::Brightness)), 11);
    // Framing, opcode, a two-byte offset, and the thirty-two byte cap.
    assert_eq!(reply_len(Request::Capabilities { offset: 0 }), 38);
    // Nothing answers a write, so nothing is read after one.
    assert_eq!(
        reply_len(Request::Set {
            feature: Feature::Brightness,
            value: 40
        }),
        0
    );
    assert_eq!(reply_len(Request::SaveSettings), 0);
}

#[test]
fn timing_faults_are_retried_and_answers_are_not() {
    let protocol = |source| DisplayError::Protocol {
        name: "test panel".to_owned(),
        source,
    };

    // Every one of these is the bus being read at the wrong moment.
    for source in [
        ProtocolError::Null,
        ProtocolError::Checksum {
            expected: 1,
            found: 2,
        },
        ProtocolError::WrongFeature {
            asked: 0x10,
            answered: 0x12,
        },
        ProtocolError::WrongOffset {
            asked: 0,
            answered: 32,
        },
        ProtocolError::TooShort { len: 1 },
    ] {
        assert_eq!(
            retry_for(&protocol(source)),
            Retry::Again,
            "{source:?} is a timing fault and asking again is what fixes it"
        );
    }

    // These are answers. The monitor said its piece and will say it again.
    for source in [
        ProtocolError::Unsupported { feature: 0x62 },
        ProtocolError::Failed { result: 0x02 },
        ProtocolError::NotAnswered,
    ] {
        assert_eq!(
            retry_for(&protocol(source)),
            Retry::Stop,
            "{source:?} is the monitor's answer, not a race"
        );
    }
}

#[test]
fn a_transport_decides_whether_its_own_failure_is_worth_retrying() {
    let transport = |retryable| DisplayError::Transport {
        name: "test panel".to_owned(),
        reason: "busy".to_owned(),
        retryable,
    };
    assert_eq!(retry_for(&transport(true)), Retry::Again);
    assert_eq!(retry_for(&transport(false)), Retry::Stop);
}

#[test]
fn a_refusal_is_never_retried() {
    let refused = DisplayError::Refused(crate::Risk::PowerOff);
    assert_eq!(retry_for(&refused), Retry::Stop);
}

#[test]
fn reading_a_feature_returns_what_the_panel_holds() {
    let mut ddc = unpaced(Panel::new("test panel").with_feature(0x10, 50, 100));
    let value = ddc.get(Feature::Brightness).expect("the panel answers");
    assert_eq!(
        value,
        Value {
            current: 50,
            maximum: 100
        }
    );
}

#[test]
fn writing_a_feature_reaches_the_panel() {
    let mut ddc = unpaced(Panel::new("test panel").with_feature(0x10, 50, 100));
    ddc.set(Feature::Brightness, 80).expect("the panel accepts");
    // Read it back the way a caller should: a monitor answers nothing after a
    // write, and panels that clamp below the maximum they report are common.
    assert_eq!(
        ddc.get(Feature::Brightness).expect("the panel answers"),
        Value {
            current: 80,
            maximum: 100
        }
    );
}

#[test]
fn a_hesitating_panel_is_asked_again() {
    // Two null messages and then an answer: a monitor waking from standby.
    let mut ddc = unpaced(
        Panel::new("test panel")
            .with_feature(0x10, 50, 100)
            .with_fault(Fault::NotReady)
            .with_fault(Fault::NotReady),
    );
    assert_eq!(
        ddc.get(Feature::Brightness)
            .expect("the third attempt lands"),
        Value {
            current: 50,
            maximum: 100
        }
    );
}

#[test]
fn a_stale_reply_is_discarded_rather_than_believed() {
    // The failure this crate exists to prevent. Without the echo check the
    // contrast reading would come back as the brightness that was asked for
    // just before it, and nothing anywhere would report a problem.
    let mut ddc = unpaced(
        Panel::new("test panel")
            .with_feature(0x10, 50, 100)
            .with_feature(0x12, 70, 100),
    );
    ddc.get(Feature::Brightness).expect("the panel answers");

    ddc.transport_mut().queue(Fault::Stale);
    assert_eq!(
        ddc.get(Feature::Contrast)
            .expect("the retry gets the right answer"),
        Value {
            current: 70,
            maximum: 100
        }
    );
}

#[test]
fn a_panel_that_never_answers_is_reported_silent() {
    let mut ddc = unpaced(
        Panel::new("test panel")
            .with_feature(0x10, 50, 100)
            .with_fault(Fault::NotReady)
            .with_fault(Fault::NotReady)
            .with_fault(Fault::NotReady),
    );
    let error = ddc
        .get(Feature::Brightness)
        .expect_err("three hesitations is not a hesitation");
    let DisplayError::Silent { attempts, .. } = error else {
        panic!("expected a silent display, got {error:?}");
    };
    assert_eq!(attempts, 3);
}

#[test]
fn an_unsupported_feature_is_not_asked_about_three_times() {
    // The monitor answered. Asking again would waste a hundred milliseconds
    // to be told the same thing, and on a list of features that adds up.
    let mut ddc = unpaced(Panel::new("test panel"));
    let error = ddc
        .get(Feature::Volume)
        .expect_err("the panel has no speakers");
    assert!(matches!(
        error,
        DisplayError::Protocol {
            source: ProtocolError::Unsupported { feature: 0x62 },
            ..
        }
    ));
    assert_eq!(
        ddc.transport().seen().len(),
        1,
        "an answer is not retried, so exactly one request went out"
    );
}

#[test]
fn a_capability_string_is_reassembled_from_its_fragments() {
    let text = "(prot(monitor)type(lcd)model(TESTPANEL)cmds(01 02 03 0C E3 F3)vcp(10 12 60(11 12) D6(01 04))mccs_ver(2.2))";
    assert!(
        text.len() > 64,
        "the string has to span at least three fragments for this to prove anything"
    );

    let mut ddc = unpaced(Panel::new("test panel").with_capabilities(text));
    let capabilities = ddc.capabilities().expect("the panel answers");

    assert_eq!(capabilities.model.as_deref(), Some("TESTPANEL"));
    assert_eq!(capabilities.mccs_version.as_deref(), Some("2.2"));
    assert!(capabilities.supports(Feature::Brightness));
    assert!(capabilities.supports(Feature::InputSource));
    assert!(
        !capabilities.supports(Feature::Volume),
        "a feature outside the string is a feature the panel does not have"
    );
    assert!(
        capabilities.warnings.is_empty(),
        "a well-formed string has nothing to forgive: {:?}",
        capabilities.warnings
    );
}

#[test]
fn a_capability_string_that_never_ends_is_cut_off_and_said_so() {
    // A monitor that never sends an empty fragment never ends the loop. The
    // cap is what stops it, and the warning is what stops the truncation being
    // silent — a capability list that is quietly short is worse than one that
    // says it is short.
    let text = format!("(prot(monitor)vcp(10){})", "0".repeat(9000));
    let mut ddc = unpaced(Panel::new("test panel").with_capabilities(&text));
    let capabilities = ddc.capabilities().expect("what did arrive still parses");

    assert!(
        capabilities
            .warnings
            .iter()
            .any(|warning| warning.contains("not read")),
        "the truncation has to be reported: {:?}",
        capabilities.warnings
    );
}
