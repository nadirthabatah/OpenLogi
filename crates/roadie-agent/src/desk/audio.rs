//! Focusrite audio interfaces, reached over their own vendor USB interface.
//!
//! Blocking: the transport is `nusb`'s synchronous half, so everything here
//! goes through [`super::blocking`].
//!
//! The control interface is vendor-specific and has no exclusive owner, so
//! claiming it disturbs nothing — the audio interfaces belong to the system's
//! audio daemon and are never touched. Reading and writing settings while
//! somebody is recording is safe by construction rather than by luck.

use roadie_focusrite::session::Session;
use roadie_focusrite::transport::{self, Attached};
use roadie_ipc::desk::{AudioFailure, AudioInputChange, AudioInputSettings, AudioInterfaceSummary};
use roadie_scarlett::risk::{Acknowledged, Risk};

use super::blocking;

/// Every Focusrite interface attached, each with a full snapshot.
pub async fn list_audio_interfaces() -> Vec<AudioInterfaceSummary> {
    blocking(
        || {
            let Ok(entries) = transport::attached() else {
                return Vec::new();
            };
            entries
                .into_iter()
                // An entry that failed to resolve is a Focusrite this build has
                // no tables for. It is dropped rather than listed: there is
                // nothing to show for it and nothing that could be changed, and
                // an unusable row in a panel of working ones only puzzles.
                .flatten()
                .map(|interface| match Session::open(&interface) {
                    Ok(mut session) => match session.snapshot() {
                        Ok(snapshot) => summarize(&interface, &snapshot),
                        Err(error) => unreachable(&interface, &error.to_string()),
                    },
                    Err(error) => unreachable(&interface, &error.to_string()),
                })
                .collect()
        },
        Vec::new,
    )
    .await
}

/// One interface's summary, from a snapshot already taken.
fn summarize(
    interface: &Attached,
    snapshot: &roadie_focusrite::session::Snapshot,
) -> AudioInterfaceSummary {
    AudioInterfaceSummary {
        id: identify(interface),
        name: snapshot.model.to_owned(),
        firmware: snapshot.firmware,
        mass_storage: snapshot.msd_mode,
        inputs: snapshot
            .inputs
            .iter()
            .map(|settings| AudioInputSettings {
                input: settings.input,
                gain: settings.gain,
                muted: settings.muted,
                phantom: settings.phantom,
            })
            .collect(),
        reachable: true,
        unreachable_reason: None,
    }
}

/// An interface that enumerated and then would not answer.
///
/// Listed rather than dropped, for the same reason a silent monitor is: the
/// commonest cause is another program holding the control interface, and that
/// is something a person can act on.
fn unreachable(interface: &Attached, why: &str) -> AudioInterfaceSummary {
    AudioInterfaceSummary {
        id: identify(interface),
        name: interface.name.to_owned(),
        firmware: 0,
        mass_storage: None,
        inputs: Vec::new(),
        reachable: false,
        unreachable_reason: Some(why.to_owned()),
    }
}

/// How one interface is addressed across calls.
fn identify(interface: &Attached) -> String {
    interface
        .serial_number
        .clone()
        .unwrap_or_else(|| interface.name.to_owned())
}

/// Change one input, answering with the whole interface as it then reads.
pub async fn set_audio_input(
    id: String,
    input: u16,
    change: AudioInputChange,
) -> Result<AudioInterfaceSummary, AudioFailure> {
    if change.is_empty() {
        return Err(AudioFailure::NothingToDo);
    }
    // The risk is derived here, from this input and this direction, before any
    // device is opened — so a refusal costs no round trip and does not depend
    // on the hardware being reachable to be correct.
    let phantom_risk = change.phantom.and_then(|on| Risk::of_phantom(input, on));
    if let Some(risk) = phantom_risk.filter(|_| !change.phantom_acknowledged) {
        return Err(AudioFailure::NeedsAcknowledgement(risk.spoken()));
    }

    blocking(
        move || {
            let interface = find(&id)?;
            let mut session =
                Session::open(&interface).map_err(|error| refused(&error.to_string()))?;

            if let Some(value) = change.gain {
                session
                    .set_gain(input, value)
                    .map_err(|error| refused(&error.to_string()))?;
            }
            if let Some(muted) = change.muted {
                session
                    .set_muted(input, muted)
                    .map_err(|error| refused(&error.to_string()))?;
            }
            if let Some(on) = change.phantom {
                // Built here, beside the write, from the risk this very input
                // carries — which is the whole point of the type. The flag that
                // crossed the wire answered a question; it could not itself be
                // the proof, or a confirmation of one input would authorise the
                // next.
                let acknowledged = phantom_risk.map(Acknowledged::of);
                session
                    .set_phantom(input, on, acknowledged)
                    .map_err(|error| refused(&error.to_string()))?;
            }

            // Read the whole interface back rather than echoing the request.
            // Phantom power is switched per *pair*, so changing it on one input
            // changes what its neighbour reports — and the gain these boxes
            // settle on is not always the one they were handed.
            let snapshot = session
                .snapshot()
                .map_err(|error| AudioFailure::Unreachable(error.to_string()))?;
            Ok(summarize(&interface, &snapshot))
        },
        || {
            Err(AudioFailure::Unreachable(
                "the agent stopped talking to that interface unexpectedly.".to_owned(),
            ))
        },
    )
    .await
}

/// The interface with that serial, or why not.
fn find(id: &str) -> Result<Attached, AudioFailure> {
    let entries =
        transport::attached().map_err(|error| AudioFailure::Unreachable(error.to_string()))?;
    entries
        .into_iter()
        .flatten()
        .find(|interface| identify(interface) == id)
        .ok_or(AudioFailure::NotFound)
}

/// A write the interface would not take.
///
/// Refused rather than unreachable: the device answered, and what it said was
/// no — usually because the model has no such control on that input.
fn refused(why: &str) -> AudioFailure {
    AudioFailure::Refused(why.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_change_asking_for_nothing_never_opens_a_device() {
        let answer = set_audio_input("whatever".into(), 1, AudioInputChange::default()).await;
        assert_eq!(answer, Err(AudioFailure::NothingToDo));
    }

    #[tokio::test]
    async fn switching_phantom_power_on_unacknowledged_is_refused_before_opening() {
        // The sentence comes back so the caller can show the very words the
        // command line reads out, rather than inventing its own.
        let answer = set_audio_input(
            "whatever".into(),
            1,
            AudioInputChange {
                phantom: Some(true),
                ..AudioInputChange::default()
            },
        )
        .await;
        let Err(AudioFailure::NeedsAcknowledgement(said)) = answer else {
            panic!("expected an acknowledgement to be demanded, got {answer:?}");
        };
        assert!(said.contains("48 volt"), "{said}");
        assert!(said.contains("ribbon"), "{said}");
    }

    #[tokio::test]
    async fn switching_phantom_power_off_asks_nothing() {
        // Off is how somebody makes the interface safe again; putting a
        // confirmation in front of it would be an obstacle before the safe
        // direction. It gets as far as looking for the device, which is what
        // NotFound proves here.
        let answer = set_audio_input(
            "no such interface".into(),
            1,
            AudioInputChange {
                phantom: Some(false),
                ..AudioInputChange::default()
            },
        )
        .await;
        assert_eq!(answer, Err(AudioFailure::NotFound));
    }
}
