use roadie_focusrite::{Settings, Snapshot};

use super::{attached_line, describe_snapshot};
use crate::spoken::{assert_agrees, assert_listenable};

/// The count and the verb have to agree. This shipped wrong once — the
/// number was written into the phrase as well as supplied by `counted`, so
/// the line read "1 1 audio interface is attached" — which is exactly the
/// class of defect a listener notices and a reader skims past.
#[test]
fn the_count_of_interfaces_agrees_with_its_verb_and_says_the_number_once() {
    assert_eq!(attached_line(1), "1 audio interface is attached.\n");
    assert_eq!(attached_line(2), "2 audio interfaces are attached.\n");
    for count in [0, 1, 2, 5] {
        let said = attached_line(count);
        assert_listenable(&said, "roadie audio");
        assert_agrees(&said, "roadie audio");
    }
}

/// The Vocaster Two on this project's desk, as it actually read on
/// 2026-09-03: two inputs, gains 15 and 9, nothing muted, phantom off, and
/// still in mass storage mode.
fn desk_vocaster() -> Snapshot {
    Snapshot {
        model: "Vocaster Two",
        firmware: 1749,
        msd_mode: Some(true),
        inputs: vec![
            Settings {
                input: 1,
                gain: Some(15),
                muted: Some(false),
                phantom: Some(false),
            },
            Settings {
                input: 2,
                gain: Some(9),
                muted: Some(false),
                phantom: Some(false),
            },
        ],
    }
}

#[test]
fn an_interface_says_what_every_input_is_doing() {
    let said = describe_snapshot(&desk_vocaster());
    assert!(
        said.contains("Vocaster Two, firmware version 1749."),
        "{said}"
    );
    assert!(
        said.contains("Input 1: gain 15, not muted, phantom power off."),
        "{said}"
    );
    assert!(
        said.contains("Input 2: gain 9, not muted, phantom power off."),
        "{said}"
    );
    assert_listenable(&said, "roadie audio");
    assert_agrees(&said, "roadie audio");
}

/// Phantom power on has to be said in full — "48 volt", not "48V", which a
/// screen reader spells letter by letter.
#[test]
fn phantom_power_being_on_is_said_in_words_that_can_be_heard() {
    let mut snapshot = desk_vocaster();
    snapshot.inputs[0].phantom = Some(true);
    snapshot.inputs[0].muted = Some(true);
    let said = describe_snapshot(&snapshot);
    assert!(
        said.contains("Input 1: gain 15, muted, 48 volt phantom power on."),
        "{said}"
    );
    assert!(!said.contains("48V"), "{said}");
    assert_listenable(&said, "roadie audio");
    assert_agrees(&said, "roadie audio");
}

/// Mass storage mode is reported without being made to sound like a fault,
/// because on this hardware it is not one: everything works with it on.
#[test]
fn mass_storage_mode_is_explained_rather_than_alarmed_about() {
    let said = describe_snapshot(&desk_vocaster());
    assert!(said.contains("mass storage mode"), "{said}");
    assert!(said.contains("nothing needs doing about it"), "{said}");
    assert_listenable(&said, "roadie audio");

    let mut off = desk_vocaster();
    off.msd_mode = Some(false);
    assert!(
        !describe_snapshot(&off).contains("mass storage"),
        "an interface out of that mode should not be told about it"
    );
}

/// A model whose gain is a physical knob has no gain to report, and the line
/// must not say "gain None" or leave a dangling comma.
#[test]
fn an_input_with_no_software_controls_is_omitted_rather_than_half_said() {
    let snapshot = Snapshot {
        model: "Scarlett 18i8 3rd Gen",
        firmware: 1600,
        msd_mode: None,
        inputs: vec![
            Settings {
                input: 1,
                gain: None,
                muted: None,
                phantom: Some(true),
            },
            Settings {
                input: 2,
                gain: None,
                muted: None,
                phantom: None,
            },
        ],
    };
    let said = describe_snapshot(&snapshot);
    assert!(
        said.contains("Input 1: 48 volt phantom power on."),
        "{said}"
    );
    assert!(
        !said.contains("Input 2"),
        "an input with nothing to say is left out"
    );
    assert!(!said.contains("None"), "{said}");
    assert_listenable(&said, "roadie audio");
    assert_agrees(&said, "roadie audio");
}
