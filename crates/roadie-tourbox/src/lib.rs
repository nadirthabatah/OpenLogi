//! The TourBox controller protocol, and the serial port it arrives on.
//!
//! A TourBox is a left-hand controller: knobs, a dial, a scroll wheel and a
//! dozen buttons, meant to sit under the hand that is not holding the mouse.
//! It is the first device family OpenRoadie speaks that is neither HID nor
//! network — it presents a USB CDC serial port and streams one byte per
//! event.
//!
//! Three pieces:
//!
//! - [`model`] — which TourBox is attached, and what controls it carries.
//! - [`event`] — the one-byte event encoding, in both directions.
//! - [`serial`] — the host half: finding the port and reading events off it.
//!
//! # The protocol in one paragraph
//!
//! Every event is a single byte. The low six bits name the control and the
//! high two bits say what happened to it: for a button, pressed or released;
//! for a wheel, which way it turned. There is no framing, no length, no
//! checksum and no sequence number, so a byte read is an event and a byte
//! lost is an event lost. That is also why this crate hands back a typed
//! error for a byte it does not recognise rather than a default — a wrong
//! guess here is a keystroke the user did not ask for.
//!
//! # Verification status
//!
//! The control codes are transcribed from the published behaviour of
//! existing open-source TourBox drivers and are covered by unit tests that
//! pin the encoding. **They have not yet been confirmed against physical
//! hardware.** The tests prove this code does what the crate claims; they
//! cannot prove the claims match a real device.
//!
//! The codes were transcribed from one driver and then cross-checked
//! against two more written independently, for different models, in
//! different languages. That is not hardware, but it is three witnesses, and
//! it settled two questions a single source had left open.
//!
//! **The knob's press byte.** One source records `0x77`, which would make
//! the knob the only control that sets a turn bit while being pressed. Two
//! others record `0x37`, and the first source's own *release* byte is
//! `0xb7` — which is `0x37` with the release bit set, and inconsistent with
//! its own press. So `0x77` is very likely a mis-transcription of `0x37`.
//! This crate implements `0x37` and rejects `0x77` by name rather than
//! quietly accepting both, so hardware can still overturn it.
//!
//! **Wheels report the end of a turn**, and an earlier version of this crate
//! rejected those bytes as impossible. A wheel sends a run of detents and
//! then one more byte with the high bit set, marking that it has come to
//! rest — see [`event::TurnPhase`]. Without that, every turn of every wheel
//! would have ended in a spurious error at the moment the hand stopped. No
//! single source stated it; comparing three is what found it.
//!
//! Still to settle on hardware: whether the transcription is right at all,
//! every model other than the Elite, and [`event::SETUP_MESSAGE`], which
//! configures haptics and has never been sent to a device.
//!
//! Nothing in [`model`] or [`event`] performs I/O.

#![deny(missing_docs)]
#![deny(rustdoc::bare_urls)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod event;
pub mod model;
#[cfg(feature = "serial")]
#[cfg_attr(docsrs, doc(cfg(feature = "serial")))]
pub mod serial;

pub use event::{Button, ButtonAction, Event, Turn, TurnPhase, Wheel, decode};
pub use model::{Model, identify};
#[cfg(feature = "serial")]
pub use serial::{SerialError, TourBox, ports};

use thiserror::Error;

/// Why a TourBox event byte could not be decoded.
///
/// Both variants are a refusal to guess. The protocol carries no checksum
/// and no framing, so a byte this crate does not understand is either a
/// control a newer model added or a byte that arrived corrupt, and there is
/// nothing in the encoding that tells those apart. Turning either into a
/// nearby known control would deliver a keystroke nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// The low six bits name no control this crate knows.
    #[error("no control on a TourBox is numbered {control:#04x} (whole byte {byte:#04x})")]
    UnknownControl {
        /// The low six bits of the offending byte.
        control: u8,
        /// The byte as it arrived, for a bug report.
        byte: u8,
    },
    /// A control that arrived carrying an action it cannot perform.
    ///
    /// A button that reports a turn, or a wheel that reports a release. The
    /// two halves of the byte are individually valid and the combination is
    /// not, which is the signature of a corrupt byte rather than of a
    /// control this build has not heard of.
    ///
    /// The message names the control *as an action* — "pressing the knob"
    /// rather than "the knob" — because three of these controls are both a
    /// button and a wheel. "The knob cannot report a turn" would be plainly
    /// false; what cannot happen is one byte meaning both at once.
    #[error(
        "byte {byte:#04x} names {control}, but its action bits say {action}, and no control does both"
    )]
    ImpossibleAction {
        /// The control the low bits named, phrased as an action.
        control: &'static str,
        /// What the high bits claimed happened to it.
        action: &'static str,
        /// The byte as it arrived, for a bug report.
        byte: u8,
    },
}
