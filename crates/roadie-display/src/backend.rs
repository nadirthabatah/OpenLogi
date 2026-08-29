//! The seam between the DDC protocol and the machine it runs on.
//!
//! Two traits, at two levels, because the three platforms do not divide the
//! work the same way.
//!
//! [`DdcTransport`] is the low one: bytes to an address and bytes back. Linux
//! and macOS both implement it, because on both of them the host hands a raw
//! I²C payload to the display and the framing, checksums, timing and retries
//! are ours. [`crate::ddc::Ddc`] turns any such transport into a
//! [`VcpBackend`], so that logic is written and tested exactly once.
//!
//! [`VcpBackend`] is the high one, and it is the trait the rest of OpenRoadie
//! sees: a feature code in, a value out. Windows implements it directly.
//! `dxva2` takes a VCP code and a value, does its own framing, its own timing
//! and its own retries, and never exposes a packet — so a seam one layer lower
//! would give the Windows backend a packet layer to route around, and the
//! shared code above it nothing to do.
//!
//! That asymmetry is the whole argument for where the seam is. It is written
//! down here because it is not visible from either end alone.
//!
//! Neither trait requires `Send`. A monitor is talked to from wherever the
//! command runs, one message at a time, and nothing in the tree moves a
//! display between threads. Requiring it would buy nothing and cost a great
//! deal: the macOS backend holds a CoreFoundation handle, so the bound could
//! only be met by asserting it, and an `unsafe impl Send` written for a bound
//! nobody needs is the worst kind of unsafe there is. Add it the day something
//! actually needs to move a display, with that thing as the reason.

use roadie_ddc::capabilities::CapabilitiesError;
use roadie_ddc::packet::{Frame, ProtocolError};
use roadie_ddc::{Capabilities, Feature, Value};

use crate::risk::Risk;

/// A raw DDC/CI transport: one request out, one reply back.
///
/// Implementors do addressing and nothing else. They do not frame, checksum,
/// wait, retry, or interpret — [`crate::ddc::Ddc`] does all of that on top,
/// which is why a new platform that exposes an I²C bus is a small file.
///
/// The one thing a transport does owe is honesty about short reads: a monitor
/// that has nothing to say still clocks out bytes, and reporting them as a
/// successful read of zero is what lets the layer above recognise a null
/// message and wait rather than declare the display broken.
pub trait DdcTransport {
    /// A name for this display suitable for an error message, and for a person
    /// to hear. Ideally the panel's own name; the bus otherwise.
    fn name(&self) -> String;

    /// Send one request to the display's DDC/CI address.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError::Transport`] if the host refused the write.
    fn send(&mut self, frame: &Frame) -> Result<(), DisplayError>;

    /// Read a reply into `buffer`, returning how many bytes arrived.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError::Transport`] if the host refused the read.
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, DisplayError>;
}

/// Everything OpenRoadie asks of a display, whoever carries the bytes.
///
/// Deliberately small. Every control a monitor has is a VCP feature, so the
/// interesting surface is three verbs and a list, not one method per knob.
pub trait VcpBackend {
    /// A name for this display suitable for an error message, and for a person
    /// to hear.
    fn name(&self) -> String;

    /// Read one feature's current and maximum value.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the display could not be reached, did not
    /// answer, or answered that it does not have the feature.
    fn get(&mut self, feature: Feature) -> Result<Value, DisplayError>;

    /// Write one feature.
    ///
    /// A monitor sends nothing back after a write, so a caller that needs to
    /// know it landed reads the feature again — and should, since panels that
    /// report a maximum they then clamp below are common.
    ///
    /// This is the unguarded write. [`crate::Display::set`] is the one to call
    /// from anywhere a person is waiting, because it refuses the two writes
    /// that cannot be taken back from the keyboard.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the display could not be reached.
    fn set(&mut self, feature: Feature, value: u16) -> Result<(), DisplayError>;

    /// Read and parse the monitor's capability string.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the display could not be reached or the
    /// string could not be parsed at all. A string that merely has something
    /// wrong with it parses anyway and records what it recovered from in
    /// [`Capabilities::warnings`].
    fn capabilities(&mut self) -> Result<Capabilities, DisplayError>;

    /// Commit the current settings to the monitor's own memory.
    ///
    /// Deliberate only; see [`Risk::SaveSettings`].
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the display could not be reached.
    fn save_settings(&mut self) -> Result<(), DisplayError>;
}

/// Why talking to a display did not work.
///
/// Split by what a person can do about it rather than by which layer raised
/// it: a permission problem, a monitor that will not answer, a monitor that
/// answered something impossible, and a refusal that came from us.
#[derive(Debug, thiserror::Error)]
pub enum DisplayError {
    /// The host would not open the display's control channel.
    ///
    /// On Linux this is the usual one, and it is a group membership rather
    /// than a fault: the I²C nodes belong to `i2c`, and a user who is not in
    /// that group gets a permission error from a monitor that is working
    /// perfectly.
    #[error("cannot reach the display on {path}: {reason}")]
    Access {
        /// The host handle that could not be opened.
        path: String,
        /// What the host said about it.
        reason: String,
    },

    /// The transfer itself failed.
    ///
    /// `retryable` is the backend's judgement, not the caller's: only the
    /// backend knows whether the host said "busy, ask again" or "no". A bus
    /// that was busy and a display that did not acknowledge its address are
    /// both worth another attempt; a permission error never will be.
    #[error("the transfer to {name} failed: {reason}")]
    Transport {
        /// The display the transfer was aimed at.
        name: String,
        /// What the host said about it.
        reason: String,
        /// Whether asking again could plausibly succeed.
        retryable: bool,
    },

    /// The monitor answered, but not in a way the protocol allows.
    #[error("{name} answered in a way that could not be read: {source}")]
    Protocol {
        /// The display that answered.
        name: String,
        /// The protocol fault.
        #[source]
        source: ProtocolError,
    },

    /// The monitor never produced a usable reply, across every attempt.
    ///
    /// The commonest cause is not a broken monitor: it is DDC/CI switched off
    /// in the monitor's own menu, where it often ships off.
    #[error("{name} did not answer after {attempts} attempts; the last problem was: {last}")]
    Silent {
        /// The display that stayed silent.
        name: String,
        /// How many attempts were made.
        attempts: u8,
        /// The problem the final attempt hit.
        last: Box<DisplayError>,
    },

    /// The capability string could not be parsed at all.
    #[error("{name} sent a capability string that could not be read: {source}")]
    Capabilities {
        /// The display that sent it.
        name: String,
        /// The parse failure.
        #[source]
        source: CapabilitiesError,
    },

    /// The display was found, and its control channel could not be opened.
    ///
    /// Distinct from [`Self::Access`] because it names the *display* rather
    /// than the path: "LG ULTRAFINE cannot be reached" is the sentence someone
    /// can act on, and the panel's own name is the thing they can match
    /// against a screen on the desk.
    #[error("{name} cannot be reached: {reason}")]
    Unopened {
        /// The display, by whatever name it could be given.
        name: String,
        /// Why its control channel could not be opened.
        reason: String,
    },

    /// We refused the write, because it cannot be undone from the keyboard.
    ///
    /// Not a failure of the display or the host. See [`Risk`].
    #[error("{0}")]
    Refused(Risk),

    /// This build has no way to reach a monitor on this platform.
    #[error("this build cannot reach monitors on {platform}")]
    Unsupported {
        /// The platform, as `std::env::consts::OS` names it.
        platform: &'static str,
    },
}

/// Box a backend for [`crate::Display`].
///
/// A free function rather than a method so a backend type never has to know
/// it is going to be boxed.
pub fn boxed<B: VcpBackend + 'static>(backend: B) -> Box<dyn VcpBackend> {
    Box::new(backend)
}

/// A display that was found but cannot be talked to, and the reason.
///
/// Enumeration keeps these rather than dropping them. A monitor missing from
/// the list answers "why is my screen not here" with nothing; a monitor in the
/// list that says "permission denied, the I2C devices belong to the i2c group"
/// answers it completely. Every operation returns the same stored reason, so
/// the explanation does not depend on which one a person happened to try.
#[derive(Debug)]
pub struct Unreachable {
    name: String,
    reason: String,
}

impl Unreachable {
    /// Box a display that could not be opened, keeping `reason` to explain it.
    ///
    /// `reason` is a bare sentence rather than a [`DisplayError`], deliberately.
    /// Wrapping one error's rendered text inside another produces "cannot reach
    /// the display on X: cannot reach the display on Y: ...", which is unhelpful
    /// on screen and genuinely hard to follow read aloud.
    #[must_use]
    pub fn boxed(name: String, reason: String) -> Box<dyn VcpBackend> {
        Box::new(Self { name, reason })
    }

    /// The stored refusal, shaped as the error for any operation.
    ///
    /// Every operation gives the same answer on purpose: the one a person
    /// happens to try first is the one they will report, so it must not decide
    /// how much they are told.
    fn refuse<T>(&self) -> Result<T, DisplayError> {
        Err(DisplayError::Unopened {
            name: self.name.clone(),
            reason: self.reason.clone(),
        })
    }
}

impl VcpBackend for Unreachable {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn get(&mut self, _feature: Feature) -> Result<Value, DisplayError> {
        self.refuse()
    }

    fn set(&mut self, _feature: Feature, _value: u16) -> Result<(), DisplayError> {
        self.refuse()
    }

    fn capabilities(&mut self) -> Result<Capabilities, DisplayError> {
        self.refuse()
    }

    fn save_settings(&mut self) -> Result<(), DisplayError> {
        self.refuse()
    }
}
