//! Elgato Key Lights, on the network — and, for the Neo, on USB too.
//!
//! A Key Light is reached over Wi-Fi: it runs a small HTTP server on port
//! 9123, answers `GET /elgato/lights` with its current state as JSON, and
//! takes the same shape back as a `PUT`. There is no authentication, no
//! pairing and no vendor cloud — which is unusually decent of Elgato, and is
//! why a third-party tool can drive one at all.
//!
//! The Key Light Neo adds a USB data port and speaks the *same* application
//! protocol over it, wrapped in HID report frames — see [`usb`]. The state
//! types in [`state`] serve both transports unchanged.
//!
//! # This crate is the protocol, not the network
//!
//! Everything here is pure: JSON in, typed state out, and the arithmetic
//! between what a person says and what the light accepts. The HTTP is behind
//! the `http` feature and the discovery behind `discovery`, so the protocol
//! itself keeps the wasm portability claim the way `roadie-ddc` does.
//!
//! That split is one crate here rather than two, unlike `roadie-ddc` and
//! `roadie-display`. The reason those are separate is that the host half is
//! three genuinely different platform APIs carrying unsafe FFI, and none of
//! that should be able to creep into the protocol. A Key Light's host half is
//! one HTTP client with no platform code in it at all, so a feature gate says
//! the same thing at a fraction of the ceremony — the way `roadie-core`
//! already gates its filesystem reads.
//!
//! # Colour temperature is the part to get right
//!
//! A Key Light does not take Kelvin. It takes **mireds** — reciprocal
//! megakelvin, `1_000_000 / kelvin` — and it takes them backwards: a *larger*
//! mired value is a *warmer*, lower-Kelvin light. Elgato's own app hides this
//! and shows Kelvin, everything else on this desk speaks Kelvin, and
//! `roadie light` speaks Kelvin. So the conversion lives here, at the
//! boundary, and is tested against the endpoints of the range rather than
//! trusted.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "discovery")]
#[cfg_attr(docsrs, doc(cfg(feature = "discovery")))]
pub mod discovery;
pub mod info;
#[cfg(feature = "net")]
#[cfg_attr(docsrs, doc(cfg(feature = "net")))]
pub mod net;
pub mod state;
pub mod usb;

#[cfg(feature = "discovery")]
pub use discovery::{DiscoveryError, discover};
pub use info::AccessoryInfo;
#[cfg(feature = "net")]
pub use net::{KeyLight, NetError};
pub use state::{Light, LightError, Lights, Range};

/// The port every Key Light serves on.
///
/// Fixed by the firmware, not configurable, and the same across every model
/// in the family.
pub const PORT: u16 = 9123;

/// The path carrying the light's current state, for both `GET` and `PUT`.
pub const LIGHTS_PATH: &str = "/elgato/lights";

/// The path carrying the light's identity and firmware.
pub const INFO_PATH: &str = "/elgato/accessory-info";

/// The mDNS service Key Lights announce themselves under.
pub const SERVICE: &str = "_elg._tcp.local.";
