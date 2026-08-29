use roadie_ddc::vcp::{InputSource, PowerMode};
use roadie_ddc::{Capabilities, Feature, Value};
use roadie_display::Risk;

use super::{
    Reach, parse_feature, parse_power, parse_value, render_capabilities, render_list,
    render_reading, render_refusal, render_write,
};
use crate::spoken::{assert_agrees, assert_listenable};

/// Every string this command can print, swept for the patterns that break a
/// screen reader.
///
/// This is the whole reason the rendering functions take values rather than
/// devices: output that needs a monitor to produce could never be checked on
/// a machine with no monitor, which is every machine this has ever run on.
#[test]
fn every_rendered_string_is_listenable() {
    let value = Value {
        current: 50,
        maximum: 100,
    };
    let strings = [
        render_list(&[]),
        render_list(&[("LG ULTRAFINE".to_owned(), Reach::Answers)]),
        render_list(&[
            ("LG ULTRAFINE".to_owned(), Reach::Answers),
            (
                "Dell U2720Q".to_owned(),
                Reach::Silent("permission denied.".to_owned()),
            ),
        ]),
        render_list(&[(
            "Dell U2720Q".to_owned(),
            Reach::Silent("it did not answer.".to_owned()),
        )]),
        render_reading("LG ULTRAFINE", Feature::Brightness, value),
        render_reading(
            "LG ULTRAFINE",
            Feature::InputSource,
            Value {
                current: 0x11,
                maximum: 0x12,
            },
        ),
        render_reading(
            "LG ULTRAFINE",
            Feature::PowerMode,
            Value {
                current: 0x01,
                maximum: 0x05,
            },
        ),
        render_reading(
            "LG ULTRAFINE",
            Feature::Other(0xE2),
            Value {
                current: 3,
                maximum: 10,
            },
        ),
        render_reading(
            "LG ULTRAFINE",
            Feature::Brightness,
            Value {
                current: 7,
                maximum: 0,
            },
        ),
        render_write(
            "LG ULTRAFINE",
            Feature::Brightness,
            80,
            Some(Value {
                current: 80,
                maximum: 100,
            }),
        ),
        render_write(
            "LG ULTRAFINE",
            Feature::Brightness,
            90,
            Some(Value {
                current: 75,
                maximum: 100,
            }),
        ),
        render_write("LG ULTRAFINE", Feature::Brightness, 80, None),
        render_refusal(Risk::PowerOff, "power", "off"),
        render_refusal(Risk::SaveSettings, "brightness", "80"),
        render_capabilities("LG ULTRAFINE", &Capabilities::default()),
    ];
    for text in &strings {
        assert_listenable(text, "roadie display");
        assert_agrees(text, "roadie display");
    }
}

#[test]
fn an_empty_list_says_what_would_have_to_be_true() {
    // The worst possible answer to "where is my monitor" is "no monitors
    // found" and nothing else, because it leaves someone with no next move.
    let text = render_list(&[]);
    assert!(text.contains("connected by a cable"), "{text}");
    assert!(
        text.contains("laptop's own screen never speaks DDC"),
        "the commonest confusion, said before it is asked: {text}"
    );
}

#[test]
fn one_monitor_takes_the_singular() {
    let text = render_list(&[("LG ULTRAFINE".to_owned(), Reach::Answers)]);
    assert!(text.starts_with("1 monitor attached.\n"), "{text}");
}

#[test]
fn more_than_one_monitor_takes_the_plural() {
    let text = render_list(&[
        ("LG ULTRAFINE".to_owned(), Reach::Answers),
        ("Dell U2720Q".to_owned(), Reach::Answers),
    ]);
    assert!(text.starts_with("2 monitors attached.\n"), "{text}");
}

#[test]
fn a_monitor_that_cannot_be_reached_is_listed_with_its_reason() {
    // Not omitted. A list that silently drops the monitor on the desk answers
    // "why is my screen missing" with nothing at all.
    let text = render_list(&[(
        "Dell U2720Q".to_owned(),
        Reach::Silent("permission denied. Add your user to the i2c group.".to_owned()),
    )]);
    assert!(text.contains("Dell U2720Q does not answer"), "{text}");
    assert!(text.contains("i2c group"), "{text}");
}

#[test]
fn a_list_where_nothing_answers_names_the_commonest_cause() {
    let text = render_list(&[(
        "Dell U2720Q".to_owned(),
        Reach::Silent("it did not answer.".to_owned()),
    )]);
    assert!(
        text.contains("DDC/CI switched off in the monitor's own menu"),
        "the first thing to check, and the one nobody thinks of: {text}"
    );
}

#[test]
fn a_reading_says_the_percentage_and_the_raw_pair() {
    // Both, because a maximum is not always 100 and is not always honest.
    // The raw pair is what someone reports when a panel misbehaves.
    let text = render_reading(
        "LG ULTRAFINE",
        Feature::Brightness,
        Value {
            current: 40,
            maximum: 80,
        },
    );
    assert_eq!(
        text,
        "LG ULTRAFINE brightness is 50 percent, or 40 out of a maximum of 80.\n"
    );
}

#[test]
fn a_reading_with_no_maximum_does_not_divide_by_it() {
    let text = render_reading(
        "LG ULTRAFINE",
        Feature::Brightness,
        Value {
            current: 7,
            maximum: 0,
        },
    );
    assert_eq!(
        text,
        "LG ULTRAFINE brightness is 7, and the monitor reports no maximum for it.\n"
    );
}

#[test]
fn an_input_is_named_rather_than_numbered() {
    let text = render_reading(
        "LG ULTRAFINE",
        Feature::InputSource,
        Value {
            current: 0x11,
            maximum: 0x12,
        },
    );
    assert_eq!(text, "LG ULTRAFINE input source is HDMI 1.\n");
}

#[test]
fn a_vendor_input_is_given_its_number_rather_than_a_wrong_name() {
    // Above 0x12 is vendor territory, and USB-C in particular has no standard
    // number. A wrong name is worse than a number.
    let text = render_reading(
        "LG ULTRAFINE",
        Feature::InputSource,
        Value {
            current: 0x1B,
            maximum: 0x1C,
        },
    );
    assert_eq!(text, "LG ULTRAFINE input source is value 27.\n");
}

#[test]
fn power_is_named_rather_than_numbered() {
    let text = render_reading(
        "LG ULTRAFINE",
        Feature::PowerMode,
        Value {
            current: 0x04,
            maximum: 0x05,
        },
    );
    assert_eq!(text, "LG ULTRAFINE power is screen off.\n");
}

#[test]
fn a_write_that_was_clamped_says_so() {
    // The failure this exists to catch. A monitor answers nothing after a
    // write, so "set to 90" with no read-back is a claim rather than a fact,
    // and panels that clamp below their reported maximum are common.
    let text = render_write(
        "LG ULTRAFINE",
        Feature::Brightness,
        90,
        Some(Value {
            current: 75,
            maximum: 100,
        }),
    );
    assert!(
        text.contains("was set to 90 but reads back as 75"),
        "{text}"
    );
    assert!(text.contains("clamp"), "{text}");
}

#[test]
fn a_write_that_landed_says_that_plainly() {
    let text = render_write(
        "LG ULTRAFINE",
        Feature::Brightness,
        80,
        Some(Value {
            current: 80,
            maximum: 100,
        }),
    );
    assert_eq!(
        text,
        "LG ULTRAFINE brightness set to 80, and reads back the same.\n"
    );
}

#[test]
fn a_write_that_could_not_be_confirmed_does_not_claim_it_landed() {
    let text = render_write("LG ULTRAFINE", Feature::Brightness, 80, None);
    assert!(text.contains("cannot be confirmed here"), "{text}");
}

#[test]
fn a_refusal_prints_a_command_that_survives_a_shell() {
    // A command this program prints is a command someone pastes. One the
    // shell splits is worse than no instruction, because the person argues
    // with it before doubting it.
    let text = render_refusal(Risk::PowerOff, "power mode", "off");
    assert!(
        text.contains("roadie display set 'power mode' off --yes"),
        "{text}"
    );
}

#[test]
fn a_refusal_names_the_physical_way_back() {
    let text = render_refusal(Risk::PowerOff, "power", "off");
    assert!(text.contains("power button on the monitor"), "{text}");
}

#[test]
fn a_feature_is_parsed_from_the_words_people_use() {
    for (typed, expected) in [
        ("brightness", Feature::Brightness),
        ("Brightness", Feature::Brightness),
        ("contrast", Feature::Contrast),
        ("input", Feature::InputSource),
        ("input source", Feature::InputSource),
        ("input-source", Feature::InputSource),
        ("power", Feature::PowerMode),
        ("power mode", Feature::PowerMode),
        ("colour preset", Feature::ColorPreset),
        // The name in the crate is spelled the British way; someone dictating
        // will get whichever their speech engine prefers.
        ("color", Feature::ColorPreset),
        ("mccs", Feature::McssVersion),
    ] {
        assert_eq!(
            parse_feature(typed).ok(),
            Some(expected),
            "{typed:?} should name {expected:?}"
        );
    }
}

#[test]
fn a_feature_code_is_accepted_in_hexadecimal() {
    // Vendor features have no names, and reading one is how someone finds out
    // what their panel's undocumented knob does.
    assert_eq!(parse_feature("0xe2").ok(), Some(Feature::Other(0xE2)));
    assert_eq!(parse_feature("0X10").ok(), Some(Feature::Brightness));
}

#[test]
fn an_unknown_feature_lists_the_ones_that_work() {
    let error = parse_feature("loudness").expect_err("there is no such setting");
    let message = error.to_string();
    assert!(message.contains("brightness"), "{message}");
    assert!(message.contains("0xe2"), "{message}");
    assert_listenable(&message, "roadie display");
}

#[test]
fn a_value_is_parsed_in_the_form_that_suits_its_feature() {
    let range = Some(Value {
        current: 50,
        maximum: 100,
    });
    // Inputs by name.
    assert_eq!(
        parse_value(Feature::InputSource, "hdmi2", range).ok(),
        Some(u16::from(InputSource::Hdmi2.code()))
    );
    // Power by name.
    assert_eq!(
        parse_value(Feature::PowerMode, "standby", range).ok(),
        Some(u16::from(PowerMode::Standby.code()))
    );
    // Mute both ways round.
    assert_eq!(parse_value(Feature::Mute, "mute", range).ok(), Some(1));
    assert_eq!(parse_value(Feature::Mute, "unmute", range).ok(), Some(2));
    // A plain number, and a percentage of the monitor's own maximum.
    assert_eq!(parse_value(Feature::Brightness, "40", range).ok(), Some(40));
    assert_eq!(
        parse_value(Feature::Brightness, "50%", range).ok(),
        Some(50)
    );
}

#[test]
fn a_percentage_is_of_the_monitors_own_maximum_not_of_a_hundred() {
    // The whole reason the current value is read before a write.
    let range = Some(Value {
        current: 0,
        maximum: 80,
    });
    assert_eq!(
        parse_value(Feature::Brightness, "50%", range).ok(),
        Some(40)
    );
}

#[test]
fn a_percentage_with_no_reading_behind_it_is_refused_rather_than_guessed() {
    let error = parse_value(Feature::Brightness, "50%", None)
        .expect_err("there is no maximum to take a percentage of");
    assert!(error.to_string().contains("did not answer"), "{error}");
}

#[test]
fn screen_off_and_off_are_kept_apart() {
    // The one mistake here that costs someone a walk to the monitor. Active
    // off blanks the panel and keeps it listening; off may stop it answering
    // at all.
    assert_eq!(parse_power("screen-off"), Some(PowerMode::ActiveOff));
    assert_eq!(parse_power("blank"), Some(PowerMode::ActiveOff));
    assert_eq!(parse_power("off"), Some(PowerMode::Off));
    assert_eq!(parse_power("nonsense"), None);
}

#[test]
fn capabilities_are_reported_with_what_had_to_be_forgiven() {
    // Monitors ship unbalanced capability strings. Recovering silently would
    // hide the reason a later command behaves oddly.
    let mut capabilities = Capabilities {
        model: Some("TESTPANEL".to_owned()),
        ..Capabilities::default()
    };
    capabilities
        .warnings
        .push("an unbalanced bracket was closed".to_owned());
    let text = render_capabilities("LG ULTRAFINE", &capabilities);
    assert!(text.contains("calls itself TESTPANEL"), "{text}");
    assert!(text.contains("had to be worked around"), "{text}");
    assert_listenable(&text, "roadie display");
}
