//! The VIA protocol, as QMK firmware speaks it.
//!
//! VIA is the reason "support every macro pad" is a tractable goal rather than
//! a per-vendor slog. Hundreds of keyboards and macro pads run QMK with VIA
//! enabled, and every one of them answers the same short raw-HID commands: read
//! the keymap, write a keycode, read and write macros. One implementation
//! reaches all of them, which is the same bargain UVC gives us for cameras.
//!
//! Everything here is pure: bytes in, bytes out. Framing, parsing, and keycode
//! naming are testable without a keyboard, and they are what most of the risk
//! lives in. Host I/O belongs to `roadie-hid`.
//!
//! # What this is drawn from, and what that means
//!
//! The command numbers and payload shapes come from QMK's own
//! `via_command_id` enum and the reference VIA implementation, not from
//! observing a device — this project has none of the hardware. That is a
//! reasonable footing (it is the contract every VIA keyboard is built to) but
//! it is not verification, and it is recorded here rather than left implied.
//! Two things in particular are worth checking against a real device before
//! trusting them: that a given board reports the protocol version this crate
//! expects, and that its report size matches [`REPORT_LEN`].
//!
//! A wrong keycode written to a keyboard is not a cosmetic bug — it can take a
//! key away from someone mid-use — so [`Command::SetKeycode`] is deliberately
//! the only writing command implemented so far, and the CLI reads back what it
//! wrote.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod command;
pub mod identity;
pub mod keycode;

pub use command::{Command, ProtocolError, Response};
pub use identity::{REPORT_LEN, USAGE_ID, USAGE_PAGE, is_via_collection};
