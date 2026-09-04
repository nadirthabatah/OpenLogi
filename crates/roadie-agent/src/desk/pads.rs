//! VIA-speaking keyboards and macro pads, reached over HID.
//!
//! Async rather than blocking, for the same reason as the Stream Deck: the
//! host layer sits on `async-hid`.
//!
//! These boards carry no model table. VIA firmware answers for itself how many
//! layers it has and what every key does, so the only identity here is what
//! the board and the USB descriptors say.

use roadie_hid::via::{self, Session};
use roadie_ipc::desk::MacroPadSummary;

/// What opening a board settled about it.
///
/// Its own type so [`summarize`] is a total function over the two outcomes and
/// can be tested without a `Attached`, which carries a live device handle and
/// cannot be built in a test.
enum Handshake {
    /// It answered, with the protocol revision and layer count it gave.
    Spoke { protocol: u16, layers: u8 },
    /// It did not, with what went wrong.
    Silent(String),
}

/// Every VIA board attached, and what its handshake said.
pub async fn list_macro_pads() -> Vec<MacroPadSummary> {
    let Ok(boards) = via::attached().await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for board in &boards {
        // Matching the VIA usage page is a guess until something answers on it:
        // the page is conventional, not reserved, and a board can carry it and
        // speak nothing. Opening is what turns that guess into a fact, and it
        // is also where an unsupported protocol revision is refused.
        let handshake = match Session::open(board).await {
            Ok(session) => Handshake::Spoke {
                protocol: session.protocol(),
                layers: session.layers(),
            },
            Err(error) => Handshake::Silent(error.to_string()),
        };
        found.push(summarize(
            &identify(
                board.serial_number.as_deref(),
                board.vendor_id,
                board.product_id,
            ),
            &board.name,
            board.vendor_id,
            board.product_id,
            handshake,
        ));
    }
    found
}

/// One board's summary.
///
/// A board that did not answer keeps `protocol` and `layers` at zero, where
/// they mean nothing — `reachable` is what a reader consults first, the same
/// rule the light and deck summaries follow.
fn summarize(
    id: &str,
    name: &str,
    vendor_id: u16,
    product_id: u16,
    handshake: Handshake,
) -> MacroPadSummary {
    let (protocol, layers, unreachable_reason) = match handshake {
        Handshake::Spoke { protocol, layers } => (protocol, layers, None),
        Handshake::Silent(why) => (0, 0, Some(why)),
    };
    MacroPadSummary {
        id: id.to_owned(),
        name: name.to_owned(),
        vendor_id,
        product_id,
        protocol,
        layers,
        reachable: unreachable_reason.is_none(),
        unreachable_reason,
    }
}

/// How one board is addressed across calls.
///
/// The serial number when there is one; otherwise its USB identity, which is
/// enough to tell two different boards apart and not enough to tell two of the
/// same model apart. Saying so in the shape of the string is better than
/// pretending otherwise.
fn identify(serial_number: Option<&str>, vendor_id: u16, product_id: u16) -> String {
    serial_number.map_or_else(
        || format!("{vendor_id:04x}:{product_id:04x}"),
        ToOwned::to_owned,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_with_a_serial_is_named_by_it() {
        // A serial is the only thing that tells two of the same model apart.
        assert_eq!(identify(Some("KB0001"), 0x5343, 0x0080), "KB0001");
    }

    #[test]
    fn a_board_without_a_serial_falls_back_to_its_usb_identity() {
        assert_eq!(identify(None, 0x5343, 0x0080), "5343:0080");
    }

    #[test]
    fn a_board_that_answered_reports_what_it_said() {
        let summary = summarize(
            "5343:0080",
            "SmartCloud",
            0x5343,
            0x0080,
            Handshake::Spoke {
                protocol: 12,
                layers: 6,
            },
        );
        assert!(summary.reachable);
        assert_eq!(summary.protocol, 12);
        assert_eq!(summary.layers, 6);
        assert!(summary.unreachable_reason.is_none());
    }

    #[test]
    fn a_board_that_did_not_answer_reports_nothing_it_did_not_learn() {
        // Zero is not a protocol revision and not a layer count. It is what
        // `reachable: false` means, and the fields are only reached through it.
        let summary = summarize(
            "5343:0080",
            "SmartCloud",
            0x5343,
            0x0080,
            Handshake::Silent("did not answer as a VIA device".to_owned()),
        );
        assert!(!summary.reachable);
        assert_eq!(summary.protocol, 0);
        assert_eq!(summary.layers, 0);
        assert_eq!(
            summary.unreachable_reason.as_deref(),
            Some("did not answer as a VIA device")
        );
    }
}
