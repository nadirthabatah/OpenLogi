use roadie_scarlett::config::ConfigItem;
use roadie_scarlett::device::{VENDOR_ID, find};
use roadie_scarlett::risk::{Acknowledged, Risk};

use super::Session;
use crate::ControlError;
use crate::mock::Panel;

/// A Vocaster Two: gain, mute and phantom, all written through the scratch
/// area. The firmware is the one the interface on this project's desk
/// actually reports.
const VOCASTER_TWO: u16 = 0x8217;

/// A Scarlett 18i8 3rd Gen: phantom power as one bit inside a shared byte,
/// which is the case the read-modify-write exists for.
const SCARLETT_18I8: u16 = 0x8214;

fn model(product_id: u16) -> &'static roadie_scarlett::device::Model {
    find(VENDOR_ID, product_id).expect("the model is in the table")
}

/// A session over a mock, plus a handle on what the mock holds.
fn session(
    product_id: u16,
    firmware: u32,
) -> (
    Session,
    std::sync::Arc<std::sync::Mutex<crate::mock::State>>,
) {
    let panel = Panel::new(model(product_id), firmware);
    let state = panel.state();
    let session = Session::with_transport(Box::new(panel), model(product_id))
        .expect("the handshake completes");
    (session, state)
}

fn memory(state: &std::sync::Mutex<crate::mock::State>, at: u16) -> u8 {
    state.lock().expect("not poisoned").memory[at as usize]
}

fn set_memory(state: &std::sync::Mutex<crate::mock::State>, at: u16, value: u8) {
    state.lock().expect("not poisoned").memory[at as usize] = value;
}

fn activations(state: &std::sync::Mutex<crate::mock::State>) -> Vec<u8> {
    state.lock().expect("not poisoned").activations.clone()
}

#[test]
fn the_handshake_reads_the_firmware_version() {
    // Not decoration: the version chooses which table the settings live in,
    // so a session that skipped it would address the oldest layout.
    let (session, _) = session(VOCASTER_TWO, 1749);
    assert_eq!(session.firmware(), 1749);
    assert_eq!(session.model().name, "Vocaster Two");
}

#[test]
fn a_gain_written_reads_back_as_itself() {
    let (mut session, _) = session(VOCASTER_TWO, 1749);
    session.set_gain(1, 42).expect("the write lands");
    assert_eq!(session.gain(1).expect("reads back"), 42);
}

/// The whole reason the buffered shape exists. The value and its index go to
/// the scratch area and the activation applies them; the setting's own
/// address is never written. A host that wrote the address instead would
/// change nothing and read back the old value.
#[test]
fn a_buffered_write_puts_the_value_and_index_in_the_scratch_area_then_activates() {
    let (mut session, state) = session(VOCASTER_TWO, 1749);
    let table = model(VOCASTER_TWO)
        .table_for(1749)
        .expect("the Vocaster has a table");
    let scratch = table.param_buf_addr;
    let gain = table
        .descriptor(ConfigItem::InputGain)
        .expect("the Vocaster has software gain");

    session.set_gain(2, 55).expect("the write lands");

    assert_eq!(
        memory(&state, scratch),
        55,
        "the value goes to the scratch area"
    );
    assert_eq!(
        memory(&state, scratch + 1),
        1,
        "the index goes one byte above it, counted from zero"
    );
    assert_eq!(
        activations(&state),
        vec![gain.activate],
        "and the activation is what applies them"
    );
}

/// The failure this crate exists to prevent, and the one that would be
/// silent on a desk: phantom power is one bit per input inside a shared
/// byte, so writing the byte outright switches it **off** on every other
/// input while the panel shows only the one that was asked for.
#[test]
fn switching_phantom_on_one_input_leaves_its_neighbours_alone() {
    let (mut session, state) = session(SCARLETT_18I8, 1600);
    let phantom = model(SCARLETT_18I8)
        .table_for(1600)
        .expect("the 18i8 has a table")
        .descriptor(ConfigItem::PhantomSwitch)
        .expect("the 18i8 has phantom power");
    assert!(
        !phantom.is_whole_bytes(),
        "this test is only meaningful where the setting is bit-sized"
    );

    // Input two already has 48 volts on it.
    set_memory(&state, phantom.offset, 0b0000_0010);

    session
        .set_phantom(
            1,
            true,
            Some(Acknowledged::of(Risk::PhantomPower { pair: 1 })),
        )
        .expect("acknowledged, so it goes through");

    assert_eq!(
        memory(&state, phantom.offset),
        0b0000_0011,
        "input one gained phantom power and input two kept it"
    );
    assert!(
        session.phantom(2).expect("reads back"),
        "input two is still on"
    );
    assert!(
        session.phantom(1).expect("reads back"),
        "and input one is now on"
    );
}

/// The other half of the same guarantee: switching one input off must not
/// take its neighbour with it.
#[test]
fn switching_phantom_off_on_one_input_leaves_its_neighbours_alone() {
    let (mut session, state) = session(SCARLETT_18I8, 1600);
    let phantom = model(SCARLETT_18I8)
        .table_for(1600)
        .expect("the 18i8 has a table")
        .descriptor(ConfigItem::PhantomSwitch)
        .expect("the 18i8 has phantom power");
    set_memory(&state, phantom.offset, 0b0000_0011);

    // Off needs no acknowledgement: it is how somebody makes the interface
    // safe again, and a confirmation in front of the safe direction is an
    // obstacle in the wrong place.
    session.set_phantom(1, false, None).expect("off is ungated");

    assert_eq!(memory(&state, phantom.offset), 0b0000_0010);
}

#[test]
fn switching_phantom_on_without_an_acknowledgement_is_refused() {
    let (mut session, state) = session(SCARLETT_18I8, 1600);
    let phantom = model(SCARLETT_18I8)
        .table_for(1600)
        .expect("the 18i8 has a table")
        .descriptor(ConfigItem::PhantomSwitch)
        .expect("the 18i8 has phantom power");

    let error = session
        .set_phantom(1, true, None)
        .expect_err("48 volts is gated");
    let said = error.to_string();
    // The error carries the sentence to put to a person, not a code: the
    // caller's next move is to say it out loud.
    assert!(said.contains("ribbon"), "{said}");
    assert!(said.contains("48 volt"), "{said}");
    assert_eq!(
        memory(&state, phantom.offset),
        0,
        "and nothing was written on the way to being refused"
    );
}

/// Agreeing to switch phantom power on for one input must not authorise it
/// on the next, which is how a host that asks once ends up sending 48 volts
/// down a cable nobody agreed to.
#[test]
fn an_acknowledgement_for_one_input_does_not_authorise_another() {
    let (mut session, _) = session(SCARLETT_18I8, 1600);
    let for_input_one = Acknowledged::of(Risk::PhantomPower { pair: 1 });
    session
        .set_phantom(2, true, Some(for_input_one))
        .expect_err("that acknowledgement was for input one");
    session
        .set_phantom(1, true, Some(for_input_one))
        .expect("this one matches");
}

#[test]
fn an_input_the_model_does_not_have_is_refused_with_the_count() {
    let (mut session, _) = session(VOCASTER_TWO, 1749);
    let error = session.gain(3).expect_err("a Vocaster Two has two inputs");
    let said = error.to_string();
    assert!(said.contains('3'), "{said}");
    assert!(said.contains('2'), "{said}");

    // Counting from one, the way the numbers are printed on the box.
    session.gain(0).expect_err("there is no input zero");
    session.gain(1).expect("input one exists");
}

/// A model that lacks a setting is told so by name, rather than having the
/// write land at address zero — which on these interfaces is a different
/// setting entirely.
#[test]
fn a_setting_the_model_does_not_have_is_named_rather_than_guessed_at() {
    // A Scarlett 18i8 3rd Gen has no software gain control; its gain is a
    // physical knob.
    let (mut session, _) = session(SCARLETT_18I8, 1600);
    let error = session.gain(1).expect_err("no software gain on this model");
    assert!(matches!(
        error,
        ControlError::NoSuchInput { .. } | ControlError::NoSuchSetting { .. }
    ));
}

#[test]
fn muting_an_input_lands_and_reads_back() {
    let (mut session, _) = session(VOCASTER_TWO, 1749);
    assert!(!session.muted(1).expect("reads"), "starts unmuted");
    session.set_muted(1, true).expect("the write lands");
    assert!(session.muted(1).expect("reads back"));
    session.set_muted(1, false).expect("and back again");
    assert!(!session.muted(1).expect("reads back"));
}

#[test]
fn mass_storage_mode_reads_and_can_be_switched_off() {
    let (mut session, state) = session(VOCASTER_TWO, 1749);
    let msd = model(VOCASTER_TWO)
        .table_for(1749)
        .expect("a table")
        .descriptor(ConfigItem::MsdSwitch)
        .expect("the Vocaster ships in Mass Storage mode");
    set_memory(&state, msd.offset, 1);

    assert!(session.msd_mode().expect("reads"));
    session.set_msd_mode(false).expect("the write lands");
    assert!(!session.msd_mode().expect("reads back"));
    assert!(
        activations(&state).contains(&msd.activate),
        "a write that needs activation has to get one, or the stored value \
         changes and the hardware does not"
    );
}

/// One call, one consistent answer. Asking setting by setting means a list
/// that can disagree with itself halfway down.
#[test]
fn a_snapshot_reports_every_input_the_model_has() {
    let (mut session, _) = session(VOCASTER_TWO, 1749);
    session.set_gain(1, 15).expect("write");
    session.set_gain(2, 9).expect("write");
    session.set_muted(2, true).expect("write");

    let snapshot = session.snapshot().expect("the interface answers");
    assert_eq!(snapshot.model, "Vocaster Two");
    assert_eq!(snapshot.firmware, 1749);
    assert_eq!(snapshot.inputs.len(), 2);
    assert_eq!(snapshot.inputs[0].gain, Some(15));
    assert_eq!(snapshot.inputs[1].gain, Some(9));
    assert_eq!(snapshot.inputs[0].muted, Some(false));
    assert_eq!(snapshot.inputs[1].muted, Some(true));
    assert_eq!(snapshot.inputs[0].phantom, Some(false));
}

/// A model with no software gain still produces a snapshot: the fields it
/// lacks are absent rather than the whole call failing.
#[test]
fn a_snapshot_of_a_model_without_gain_omits_it_rather_than_failing() {
    let (mut session, _) = session(SCARLETT_18I8, 1600);
    let snapshot = session.snapshot().expect("the interface answers");
    assert_eq!(snapshot.inputs.len(), 2, "two phantom-capable inputs");
    assert!(
        snapshot.inputs.iter().all(|input| input.gain.is_none()),
        "an 18i8's gain is a knob, not a setting"
    );
    assert!(snapshot.inputs.iter().all(|input| input.phantom.is_some()));
}
