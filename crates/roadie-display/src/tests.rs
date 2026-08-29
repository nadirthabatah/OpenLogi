use roadie_ddc::Feature;

use super::{Acknowledged, Ddc, Display, DisplayError, DisplayId, Pacing, Risk, backend};
use crate::mock::Panel;

/// A display backed by a scripted panel, with no waiting.
fn display() -> Display {
    let panel = Panel::new("test panel")
        .with_feature(0x10, 50, 100)
        .with_feature(0xD6, 0x01, 0x05);
    Display::new(
        DisplayId::new("card0-DP-1"),
        None,
        backend::boxed(Ddc::with_pacing(panel, Pacing::NONE)),
    )
}

#[test]
fn an_ordinary_write_goes_straight_through() {
    let mut display = display();
    display
        .set(Feature::Brightness, 80)
        .expect("brightness is reversible and needs no confirming");
}

#[test]
fn powering_off_is_refused_without_a_confirmation() {
    let mut display = display();
    let error = display
        .set(Feature::PowerMode, 0x05)
        .expect_err("the one write that may not be undoable");
    let DisplayError::Refused(risk) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(risk, Risk::PowerOff);
}

#[test]
fn powering_off_goes_through_once_it_has_been_acknowledged() {
    let mut display = display();
    display
        .set_acknowledging(Feature::PowerMode, 0x05, Acknowledged::of(Risk::PowerOff))
        .expect("acknowledged, so it proceeds");
}

#[test]
fn an_acknowledgement_cannot_be_spent_on_a_different_risk() {
    // Agreeing to save settings is not agreeing to switch the screen off. If
    // one confirmation could authorise the other, a front end that asked about
    // the cheap one would be able to perform the expensive one.
    let mut display = display();
    let error = display
        .set_acknowledging(
            Feature::PowerMode,
            0x05,
            Acknowledged::of(Risk::SaveSettings),
        )
        .expect_err("the wrong risk was acknowledged");
    let DisplayError::Refused(risk) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(risk, Risk::PowerOff);
}

#[test]
fn a_recoverable_power_state_needs_no_confirmation() {
    // Active off blanks the panel and keeps it listening, which is the one to
    // reach for. It must not be dragged into the confirmation flow with the
    // genuinely dangerous value.
    let mut display = display();
    display
        .set(Feature::PowerMode, 0x04)
        .expect("active off is recoverable");
}

#[test]
fn saving_needs_its_own_acknowledgement() {
    let mut display = display();
    let error = display
        .save_settings(Acknowledged::of(Risk::PowerOff))
        .expect_err("that acknowledgement was about something else");
    assert!(matches!(error, DisplayError::Refused(Risk::SaveSettings)));

    display
        .save_settings(Acknowledged::of(Risk::SaveSettings))
        .expect("acknowledged, so it proceeds");
}

#[test]
fn a_display_with_no_edid_is_named_by_its_connector() {
    // Not the pretty answer, but an honest one: a connector name is still
    // something a person can match against a cable.
    assert_eq!(display().describe(), "card0-DP-1");
}

#[test]
fn a_display_with_an_edid_is_named_by_the_panel() {
    // A minimal but valid EDID: the header, LG's PNP code 0x1E6D packed into
    // the manufacturer field, and a descriptor holding the model name.
    let mut block = [0_u8; 128];
    block[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
    block[8] = 0x1E;
    block[9] = 0x6D;
    block[18] = 1;
    block[19] = 4;
    // Descriptor at 0x36: tag 0xFC is the display's name.
    block[54..59].copy_from_slice(&[0x00, 0x00, 0x00, 0xFC, 0x00]);
    block[59..72].copy_from_slice(b"ULTRAFINE\n   ");
    let sum = block[..127]
        .iter()
        .fold(0_u8, |acc, byte| acc.wrapping_add(*byte));
    block[127] = sum.wrapping_neg();

    let edid = roadie_ddc::Edid::parse(&block).expect("a well-formed EDID");
    let display = Display::new(
        DisplayId::new("card0-DP-1"),
        Some(edid),
        backend::boxed(Ddc::with_pacing(Panel::new("test panel"), Pacing::NONE)),
    );
    assert_eq!(display.describe(), "LG ULTRAFINE");
}

#[test]
fn a_display_that_cannot_be_opened_explains_itself_on_every_operation() {
    // The reason must not depend on which operation someone happened to try
    // first, because the one they try first is the one they will report.
    let mut display = Display::new(
        DisplayId::new("card0-DP-1"),
        None,
        backend::Unreachable::boxed(
            "LG ULTRAFINE".to_owned(),
            &DisplayError::Access {
                path: "/dev/i2c-7".to_owned(),
                reason: "permission denied".to_owned(),
            },
        ),
    );

    for message in [
        display.get(Feature::Brightness).map(|_| ()).unwrap_err(),
        display.set(Feature::Brightness, 50).unwrap_err(),
        display.capabilities().map(|_| ()).unwrap_err(),
    ] {
        assert!(
            message.to_string().contains("permission denied"),
            "every operation says the same why: {message}"
        );
    }
}

/// A backend that fails the test if anything reaches it.
///
/// The property being checked is an ordering one: the guard has to refuse
/// *before* the write goes out, not report an error after it. For a save that
/// is the whole point — the monitor's memory has a finite number of erase
/// cycles, so a guard that lets the request through and complains afterwards
/// has already spent one. A backend that cannot be touched is the only way to
/// state that without reaching back through a boxed trait object.
#[derive(Debug)]
struct Tripwire;

impl crate::VcpBackend for Tripwire {
    fn name(&self) -> String {
        "tripwire".to_owned()
    }

    fn get(&mut self, _feature: Feature) -> Result<roadie_ddc::Value, DisplayError> {
        panic!("a read reached the monitor");
    }

    fn set(&mut self, _feature: Feature, _value: u16) -> Result<(), DisplayError> {
        panic!("a refused write reached the monitor");
    }

    fn capabilities(&mut self) -> Result<roadie_ddc::Capabilities, DisplayError> {
        panic!("a capability read reached the monitor");
    }

    fn save_settings(&mut self) -> Result<(), DisplayError> {
        panic!("a refused save reached the monitor");
    }
}

#[test]
fn a_refusal_happens_before_anything_goes_out() {
    let mut display = Display::new(DisplayId::new("card0-DP-1"), None, backend::boxed(Tripwire));

    display
        .set(Feature::PowerMode, 0x05)
        .expect_err("powering off needs an acknowledgement");
    display
        .set_acknowledging(
            Feature::PowerMode,
            0x05,
            Acknowledged::of(Risk::SaveSettings),
        )
        .expect_err("that acknowledgement was about something else");
    display
        .save_settings(Acknowledged::of(Risk::PowerOff))
        .expect_err("that acknowledgement was about something else");
}
