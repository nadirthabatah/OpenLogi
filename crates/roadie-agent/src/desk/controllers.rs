//! TourBox controllers, reached over a serial port.
//!
//! Identity only. A TourBox has nothing to read and nothing to write: it
//! streams a byte per event and stores no settings, so what its buttons and
//! wheels *do* is this app's configuration rather than anything on the device.
//!
//! That is why there is no `set_controller` beside this. Enumerating is also
//! deliberately all it does — opening a TourBox writes an unlock message and a
//! haptics setup to it, which is a real change to a device somebody may be
//! using, and far too much to do merely because a panel was opened.

use roadie_ipc::desk::ControllerSummary;
use roadie_tourbox::serial::Port;

use super::blocking;

/// Every TourBox on a serial port.
pub async fn list_controllers() -> Vec<ControllerSummary> {
    blocking(
        || {
            roadie_tourbox::ports()
                .unwrap_or_default()
                .iter()
                .map(summarize)
                .collect()
        },
        Vec::new,
    )
    .await
}

/// One controller's summary.
///
/// No `reachable` field, unlike every other summary here, because nothing was
/// tried: enumeration found a serial port whose USB identity is a TourBox, and
/// this reports exactly that. A field that was always `true` would claim a
/// probe that never happened.
fn summarize(port: &Port) -> ControllerSummary {
    ControllerSummary {
        id: port.path.clone(),
        name: port.model.name.to_owned(),
        buttons: count(port.model.buttons.len()),
        wheels: count(port.model.wheels.len()),
        haptics: port.model.haptics,
        serial_number: port.serial_number.clone(),
    }
}

/// A control count, narrowed for the wire.
///
/// Saturating rather than wrapping: these come from a static table with tens
/// of entries, so the ceiling is unreachable — but a count that wrapped to
/// zero would be a worse answer than one that is merely very large.
fn count(len: usize) -> u16 {
    u16::try_from(len).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_count_saturates_rather_than_wrapping() {
        assert_eq!(count(14), 14);
        assert_eq!(count(usize::MAX), u16::MAX);
    }

    #[tokio::test]
    async fn listing_answers_even_where_there_is_no_serial_port_at_all() {
        // The list is a fact about the desk, not a request that can fail: a
        // machine with no ports has no controllers, which is an answer rather
        // than an error. Runs on every platform and on a bare CI host.
        let found = list_controllers().await;
        for controller in &found {
            assert!(!controller.id.is_empty(), "a port with no path");
        }
    }
}
