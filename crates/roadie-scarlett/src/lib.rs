//! The Focusrite Scarlett control protocol, with no host I/O in it.
//!
//! A Scarlett is a USB audio interface. The audio itself is standard USB
//! audio class and needs nothing from this crate; what needs a protocol is
//! everything *around* the audio — phantom power, input gain, pad, air, the
//! monitor mix — which Focusrite carries over a vendor-specific control
//! interface that no standard describes.
//!
//! This crate is the wire format and the device tables. It opens nothing and
//! talks to nothing, in the same shape as [`roadie_ddc`] for monitors: a host
//! layer that owns a USB handle can drive it, and this half stays testable and
//! portable.
//!
//! # Where these facts come from, and the licence question that raises
//!
//! Focusrite publishes no specification for this interface. The authoritative
//! description is the Linux kernel's `sound/usb/mixer_scarlett2.c`, written by
//! Geoffrey D. Bennett, which is GPL-2.0 — and OpenRoadie is MIT/Apache-2.0.
//!
//! What is taken from it here is **facts**: opcodes, byte offsets, field
//! widths, and which model uses which table. A register offset is a fact about
//! a piece of hardware in the same way a serial number is; it is not authored
//! expression, and it can no more be licensed than the pinout of a connector.
//! No code, comment, structure, or naming from that driver is reproduced —
//! this crate is organised around its own types and its own explanations, and
//! where the two would agree it is because the hardware left no choice.
//!
//! This is the same footing [`roadie_ddc`] stands on with `ddcutil`, which is
//! also GPL and is likewise credited for facts rather than copied for code.
//! The credit is deliberate and belongs in the source rather than in a commit
//! message: anybody auditing this file should be able to see where the numbers
//! came from without having to dig.
//!
//! [`roadie_ddc`]: https://docs.rs/roadie-ddc

#![forbid(unsafe_code)]

pub mod alsa;
pub mod config;
pub mod device;
pub mod packet;
pub mod risk;
mod tables;
pub mod transaction;

pub use config::{ConfigItem, ConfigSet, Descriptor};
pub use device::{Model, VENDOR_ID, find};
pub use packet::{
    INIT_1, INIT_2, INIT_2_RESPONSE_LEN, Packet, ProtocolError, Request, Response, Sequence,
    firmware_version,
};
pub use risk::{Acknowledged, Risk};
pub use transaction::{Plan, PlanError, plan_write};
