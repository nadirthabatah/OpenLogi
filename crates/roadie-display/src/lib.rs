//! Reaching an actual monitor.
//!
//! [`roadie_ddc`] is the protocol with no host under it: packets in, packets
//! out. This crate is the other half — the part that knows about `/dev/i2c-3`,
//! about `IOAVService`, about `dxva2.dll` — and it exists as a separate crate
//! rather than a module of `roadie-ddc` for a reason worth stating plainly:
//! `roadie-ddc` is on CI's wasm portability list, and that list is a claim a
//! crate earns and then has to keep. An ioctl added to it would retire the
//! claim quietly. So the split here is the same one `roadie-hid` has from
//! `roadie-device`, for the same reason.
//!
//! # Why monitors are worth the trouble
//!
//! A monitor is the peripheral nobody thinks of as a peripheral, and it has
//! the best standard behind it of anything on the desk. Brightness is the
//! control people reach for most, and the alternative to software is a menu
//! driven by four unlabelled buttons on a bezel — which is an annoyance for
//! someone who can see it and an impossibility for someone who cannot.
//!
//! # The shape
//!
//! - [`DdcTransport`] carries bytes to an I²C address. Linux and macOS
//!   implement it.
//! - [`Ddc`] turns any transport into a [`VcpBackend`], adding framing,
//!   the timing floors, and the retry policy. Written once.
//! - [`VcpBackend`] is what everything above sees. Windows implements it
//!   directly, because `dxva2` does the framing itself.
//! - [`Display`] is one monitor: a backend, plus whatever the host could learn
//!   about what it *is*, plus the refusal that guards the two writes that
//!   cannot be undone.
//! - [`mock::Panel`] is a monitor made of software, so all of the above can be
//!   driven with nothing plugged in.
//!
//! # What has never been verified
//!
//! No code in this crate has met a physical monitor. The protocol comes from
//! the DDC/CI 1.1 and MCCS 2.2a specifications and from `ddcutil`'s reading of
//! them; the host calls come from the documented APIs and from what
//! MonitorControl and `ddcutil` do with them. That is a sound footing and it
//! is not verification. `docs/VERIFYING.md` carries the ordered steps that
//! would make it one.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod backend;
pub mod ddc;
pub mod mock;
pub mod risk;

pub use backend::{DdcTransport, DisplayError, VcpBackend};
pub use ddc::{Ddc, Pacing};
pub use risk::{Acknowledged, Risk};

use roadie_ddc::{Capabilities, Edid, Feature, Value};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// How this host addresses one display.
///
/// Opaque on purpose, and not a stable identity: it is an I²C bus name on
/// Linux, an IOKit service on macOS, an ordinal into the handles `dxva2` hands
/// out on Windows. None of those survive a reboot reliably, let alone a cable
/// swap, which is exactly why the *name* a person sees comes from the EDID
/// instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(String);

impl DisplayId {
    /// Wrap a host handle.
    #[must_use]
    pub fn new(handle: impl Into<String>) -> Self {
        Self(handle.into())
    }

    /// The handle, as the host spells it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DisplayId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One monitor, and the way to talk to it.
///
/// Holds the guard as well as the backend: [`Display::set`] refuses the writes
/// that cannot be taken back, and [`VcpBackend::set`] is the unguarded one
/// underneath. The guard is here, at the shared type, rather than in the CLI,
/// because a refusal implemented in one front end is a refusal the MCP server
/// does not have — and the MCP server is where a write arrives without anyone
/// having typed it.
pub struct Display {
    id: DisplayId,
    edid: Option<Edid>,
    backend: Box<dyn VcpBackend>,
}

impl std::fmt::Debug for Display {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Display")
            .field("id", &self.id)
            .field("edid", &self.edid)
            .finish_non_exhaustive()
    }
}

impl Display {
    /// Assemble a display from a host handle, what is known about it, and a
    /// way to talk to it.
    #[must_use]
    pub fn new(id: DisplayId, edid: Option<Edid>, backend: Box<dyn VcpBackend>) -> Self {
        Self { id, edid, backend }
    }

    /// How this host addresses it.
    #[must_use]
    pub fn id(&self) -> &DisplayId {
        &self.id
    }

    /// What the display says it is, when the host could read an EDID.
    #[must_use]
    pub fn edid(&self) -> Option<&Edid> {
        self.edid.as_ref()
    }

    /// The name to print or speak.
    ///
    /// The EDID's description when there is one, because "LG ULTRAFINE" tells
    /// someone which screen this is and `/dev/i2c-7` does not. The host handle
    /// only when there is no EDID at all, which on Linux means the kernel had
    /// nothing either.
    #[must_use]
    pub fn describe(&self) -> String {
        self.edid
            .as_ref()
            .map_or_else(|| self.id.0.clone(), Edid::describe)
    }

    /// Read one feature.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the monitor could not be reached, stayed
    /// silent, or answered that it does not have the feature.
    pub fn get(&mut self, feature: Feature) -> Result<Value, DisplayError> {
        self.backend.get(feature)
    }

    /// Write one feature, refusing what cannot be undone.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError::Refused`] carrying the [`Risk`] when the write
    /// is one of the two that cannot be taken back from the keyboard — the
    /// caller is expected to put [`Risk::spoken`] to whoever asked, and come
    /// back through [`Display::set_acknowledging`] if they still mean it.
    /// Otherwise returns [`DisplayError`] if the monitor could not be reached.
    pub fn set(&mut self, feature: Feature, value: u16) -> Result<(), DisplayError> {
        if let Some(risk) = Risk::of(feature, value) {
            return Err(DisplayError::Refused(risk));
        }
        self.backend.set(feature, value)
    }

    /// Write one feature, having put its risk to someone who said yes.
    ///
    /// The acknowledgement is checked against the risk this write actually
    /// carries, so one cannot be spent on another: agreeing to save settings
    /// does not authorise powering the panel off.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError::Refused`] if the acknowledgement names a
    /// different risk than this write carries, or [`DisplayError`] if the
    /// monitor could not be reached.
    pub fn set_acknowledging(
        &mut self,
        feature: Feature,
        value: u16,
        acknowledged: Acknowledged,
    ) -> Result<(), DisplayError> {
        match Risk::of(feature, value) {
            None => self.backend.set(feature, value),
            Some(risk) if risk == acknowledged.risk() => self.backend.set(feature, value),
            Some(risk) => Err(DisplayError::Refused(risk)),
        }
    }

    /// Read and parse the capability string.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError`] if the monitor could not be reached or sent a
    /// string that could not be parsed at all.
    pub fn capabilities(&mut self) -> Result<Capabilities, DisplayError> {
        self.backend.capabilities()
    }

    /// Commit the current settings to the monitor's own memory.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayError::Refused`] unless `acknowledged` names
    /// [`Risk::SaveSettings`], or [`DisplayError`] if the monitor could not be
    /// reached.
    pub fn save_settings(&mut self, acknowledged: Acknowledged) -> Result<(), DisplayError> {
        if acknowledged.risk() != Risk::SaveSettings {
            return Err(DisplayError::Refused(Risk::SaveSettings));
        }
        self.backend.save_settings()
    }
}

/// Every display this host can find.
///
/// Finding one is not the same as being able to control it. On Linux in
/// particular a display is named from the EDID the kernel already publishes,
/// which needs no permission at all, while talking to it needs access to an
/// I²C node that is usually group-owned — so an entry in this list with a
/// perfectly good name can still refuse every read. That is the honest shape
/// of the problem and the reason enumeration does not filter on reachability:
/// a list that silently omitted the monitor on the desk would answer the
/// question "why is my screen not here" with nothing.
///
/// # Errors
///
/// Returns [`DisplayError`] if the host's display list itself could not be
/// read. An individual display that cannot be opened is reported by the
/// operations on it, not by omission from this list.
pub fn enumerate() -> Result<Vec<Display>, DisplayError> {
    #[cfg(target_os = "linux")]
    {
        linux::enumerate()
    }
    #[cfg(target_os = "macos")]
    {
        macos::enumerate()
    }
    #[cfg(target_os = "windows")]
    {
        windows::enumerate()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(DisplayError::Unsupported {
            platform: std::env::consts::OS,
        })
    }
}

#[cfg(test)]
mod tests;
