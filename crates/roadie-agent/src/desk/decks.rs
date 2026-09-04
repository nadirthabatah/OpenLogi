//! Stream Decks, reached over HID.
//!
//! Async rather than blocking: the host layer sits on `async-hid`, so this
//! awaits directly instead of going through [`super::blocking`].
//!
//! # Opening is the question worth asking
//!
//! Enumerating a Stream Deck only reads HID descriptors, so one is almost
//! always *listed*. Whether it can be driven is a different question, and on
//! this platform it is usually answered "no" by another program: Elgato's own
//! Stream Deck app holds the device exclusively, and Logitech's device manager
//! has been seen taking its input out from under a driver that had it open.
//! So the list opens each deck and reports what happened, the same way the
//! monitor list runs a DDC probe.

use roadie_hid::streamdeck::{self, Attached, Session};
use roadie_ipc::desk::{StreamDeckChange, StreamDeckFailure, StreamDeckSummary};
use roadie_streamdeck::model::ELGATO_VENDOR_ID;
use roadie_streamdeck::report::Brightness;

/// Every Stream Deck attached, and whether each one opened.
pub async fn list_stream_decks() -> Vec<StreamDeckSummary> {
    let Ok(collections) = streamdeck::attached().await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    // One physical deck presents several HID collections; `preferred` picks the
    // one that actually carries the protocol, so a single device is listed once
    // rather than three times under the same name.
    for deck in streamdeck::preferred(&collections) {
        let opened = Session::open(deck).await;
        found.push(summarize(deck, opened.err().map(|error| error.to_string())));
    }
    found
}

/// One deck's summary, given whatever went wrong opening it.
fn summarize(deck: &Attached, why: Option<String>) -> StreamDeckSummary {
    StreamDeckSummary {
        id: identify(deck),
        name: deck.name.clone(),
        model: deck.model.name.to_owned(),
        keys: deck.model.key_count(),
        dials: deck.model.dials,
        reachable: why.is_none(),
        unreachable_reason: why,
    }
}

/// How one deck is addressed across calls.
///
/// The serial number when there is one, because it survives a replug and
/// distinguishes two decks of the same model. Without one there is nothing
/// stable to say beyond which model it is — which is still enough to find it
/// again when it is the only one of its kind, and is honest about not being
/// enough when it is not.
fn identify(deck: &Attached) -> String {
    deck.serial_number.clone().unwrap_or_else(|| {
        format!(
            "{ELGATO_VENDOR_ID:04x}:{product:04x}",
            product = deck.model.product_id
        )
    })
}

/// Change one deck, answering with what it then looks like.
pub async fn set_stream_deck(
    id: String,
    change: StreamDeckChange,
) -> Result<StreamDeckSummary, StreamDeckFailure> {
    if change.is_empty() {
        return Err(StreamDeckFailure::NothingToDo);
    }
    // The brightness is validated before the device is opened, so a value the
    // hardware has no defined behaviour for never reaches it.
    let brightness = change
        .brightness_percent
        .map(Brightness::new)
        .transpose()
        .map_err(|error| StreamDeckFailure::Refused(error.to_string()))?;

    let collections = streamdeck::attached()
        .await
        .map_err(|error| StreamDeckFailure::Unreachable(error.to_string()))?;
    let deck = streamdeck::preferred(&collections)
        .into_iter()
        .find(|deck| identify(deck) == id)
        .ok_or(StreamDeckFailure::NotFound)?;

    let mut session = Session::open(deck)
        .await
        .map_err(|error| StreamDeckFailure::Unreachable(error.to_string()))?;
    if let Some(brightness) = brightness {
        session
            .set_brightness(brightness)
            .await
            .map_err(|error| StreamDeckFailure::Unreachable(error.to_string()))?;
    }
    if change.reset {
        session
            .reset()
            .await
            .map_err(|error| StreamDeckFailure::Unreachable(error.to_string()))?;
    }
    // Reached the device and came back, so it is reachable by definition. There
    // is no read-back to offer: see `StreamDeckChange` for why brightness is
    // the one write on this wire whose result cannot be confirmed.
    Ok(summarize(deck, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_change_asking_for_nothing_never_opens_a_device() {
        let answer = set_stream_deck("whatever".into(), StreamDeckChange::default()).await;
        assert_eq!(answer, Err(StreamDeckFailure::NothingToDo));
    }

    #[tokio::test]
    async fn a_brightness_the_hardware_has_no_behaviour_for_is_refused_before_opening() {
        // Above 100 is undefined on the device, and the newtype is what keeps
        // it from arriving. Checked ahead of enumeration so the refusal does
        // not depend on any hardware being present to hear it.
        let answer = set_stream_deck(
            "whatever".into(),
            StreamDeckChange {
                brightness_percent: Some(101),
                reset: false,
            },
        )
        .await;
        assert!(
            matches!(answer, Err(StreamDeckFailure::Refused(_))),
            "{answer:?}"
        );
    }
}
