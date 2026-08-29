//! The Elgato Stream Deck wire protocol, as pure data transformations.
//!
//! This is the second device family OpenRoadie speaks, and the first that is
//! not Logitech. It is laid out to mirror the split that already works for
//! HID++: this crate is the protocol and knows no host — the counterpart of
//! `roadie-device` — while opening HID nodes, scheduling writes, and
//! turning key events into actions belong to a host layer above it.
//!
//! Three pieces:
//!
//! - [`model`] — which Stream Deck is attached, and its keys, screens and
//!   layout. Adding a model that speaks a known generation is a table entry
//!   and no code.
//! - [`report`] — control reports out (brightness, reset) and key events in.
//! - [`image`] — framing an encoded key image into output packets.
//!
//! # Verification status
//!
//! Every byte layout here is written from the published behavior of the
//! protocol as implemented by existing open-source Stream Deck libraries,
//! and is covered by unit tests that pin the encoding. **None of it has been
//! run against physical hardware yet.** The tests prove the code does what
//! this crate claims; they cannot prove the claims match a real device. Two
//! things to confirm first on hardware: the per-row key mirroring on the
//! original Stream Deck ([`model::KeyOrder::RightToLeftRows`]), and the
//! gen 1 feature-report prefixes.
//!
//! Nothing here talks to the network, and nothing here retains state beyond
//! the values a caller hands it.

#![deny(missing_docs)]
#![deny(rustdoc::bare_urls)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod font;
pub mod image;
pub mod model;
#[cfg(feature = "render")]
pub mod render;
pub mod report;

use thiserror::Error;

/// Why a Stream Deck exchange could not be encoded or decoded.
///
/// Every variant is a refusal to guess. A short report, an unexpected report
/// ID, or a generation whose framing is not implemented could each be
/// papered over with a default, and each default would silently produce
/// wrong key events or a corrupt image on a real device.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// A key index that this model does not have.
    #[error("key {index} is out of range for a device with {count} keys")]
    KeyOutOfRange {
        /// The offending index.
        index: u16,
        /// How many keys the model actually has.
        count: u16,
    },
    /// A brightness above 100 percent.
    #[error("brightness {percent} is above 100 percent")]
    BrightnessOutOfRange {
        /// The offending percentage.
        percent: u8,
    },
    /// An input report that ended before the data it should carry.
    #[error("report is {found} bytes, expected at least {expected}")]
    ShortReport {
        /// Bytes the decode needed.
        expected: usize,
        /// Bytes actually present.
        found: usize,
    },
    /// An input report addressed to a report ID this decode does not handle.
    #[error("report ID {report_id:#04x} does not carry key state")]
    UnexpectedReport {
        /// The report ID that arrived.
        report_id: u8,
    },
    /// Image upload attempted against a model whose keys have no screens.
    #[error("the {model} has no key screens")]
    ScreenlessModel {
        /// Marketing name of the model.
        model: &'static str,
    },
    /// A key image could not be encoded.
    #[error("could not encode the key image: {detail}")]
    ImageEncoding {
        /// What the encoder reported.
        detail: String,
    },
    /// Image framing attempted for a generation this crate does not encode.
    #[error("key image framing for the {model} is not implemented yet")]
    ImageFramingUnsupported {
        /// Marketing name of the model.
        model: &'static str,
    },
    /// An image needing more packets than the wire's page counter can express.
    #[error("image is {bytes} bytes, more than the {max} the page counter can address")]
    ImageTooLarge {
        /// Size of the offending image.
        bytes: usize,
        /// Largest image the framing can address.
        max: usize,
    },
}
