//! DDC/CI and MCCS, the protocol every desk monitor already speaks.
//!
//! A monitor is the one peripheral on the desk that nobody thinks of as a
//! peripheral, and it is also the one with the best standard behind it. The
//! display data channel has carried a two-way control protocol since 1998:
//! ask a monitor for its brightness and it answers, tell it to switch to
//! HDMI 2 and it switches. Dell, LG, Samsung, ASUS, BenQ, Gigabyte, and most
//! of the panels sold as "monitor" rather than "TV" implement enough of it to
//! be useful, and they implement the *same* enough of it that one
//! implementation reaches all of them. That is the same bargain UVC gives us
//! for webcams and VIA gives us for macro pads, and it is why monitors were
//! the right category to add next.
//!
//! Everything here is pure. Packets in, packets out; capability strings in,
//! parsed features out. No I²C, no ioctls, no platform code. Host transport
//! belongs to the layer above, and it differs per platform in a way none of
//! this logic should have to know about:
//!
//! | Platform | How the bytes get to the monitor |
//! | --- | --- |
//! | Linux | `/dev/i2c-*`, the DDC line the driver exposes |
//! | Windows | `dxva2.dll`, the monitor-configuration API |
//! | macOS | `IOAVService` on Apple silicon, `IOI2CSendRequest` on Intel |
//!
//! # What this is drawn from, and what that means
//!
//! The framing, the checksum rule, the opcodes, and the timing minimums come
//! from the DDC/CI 1.1 and MCCS 2.2a specifications and from `ddcutil`'s
//! implementation of them — not from watching a real monitor answer, because
//! this project has none of the hardware. That is a sound footing, since it is
//! the contract every DDC monitor is built to, but it is not verification, and
//! saying so here beats letting the omission be inferred.
//!
//! Three things in particular deserve a real panel in front of them before
//! anyone trusts them, and [`docs/VERIFYING.md`] carries them as ordered
//! steps:
//!
//! 1. The reply checksum seed. Requests and replies are checksummed
//!    differently — see [`packet`] — and a monitor that disagrees will look
//!    like a monitor that is not answering at all.
//! 2. The timing minimums. They are floors, not targets; some panels want
//!    considerably more, and the failure mode of rushing one is a garbled
//!    reply rather than a clean error.
//! 3. Which input-source values a given monitor actually accepts. Values above
//!    `0x12` are vendor territory, and USB-C in particular has no standard
//!    number. [`vcp::InputSource`] carries the standard ones and passes
//!    everything else through unchanged rather than guessing.
//!
//! # Writing to a monitor is not free
//!
//! Reading is safe. Writing is mostly safe. Two writes are not, and both are
//! called out where they are defined rather than left for a caller to
//! discover:
//!
//! - [`vcp::PowerMode::Off`] can put a monitor into a state where it stops
//!   answering DDC entirely, so the only way back is the physical button. On a
//!   desk that is an annoyance. For someone who cannot see the button, it is
//!   worse, so [`vcp::PowerMode::is_recoverable`] exists and the layer above
//!   is expected to ask before crossing it.
//! - `Save Current Settings` writes to the monitor's own non-volatile memory,
//!   which has a finite number of erase cycles. It is a deliberate act, never
//!   something to append to every set.
//!
//! [`docs/VERIFYING.md`]: https://github.com/nadirthabatah/OpenRoadie/blob/master/docs/VERIFYING.md

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod capabilities;
pub mod edid;
pub mod packet;
pub mod vcp;

pub use capabilities::{Capabilities, CapabilitiesError};
pub use edid::{Edid, EdidError};
pub use packet::{I2C_ADDRESS, ProtocolError, Reply, Request};
pub use vcp::{Feature, InputSource, PowerMode, Value};
