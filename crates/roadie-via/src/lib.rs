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
//! `via_command_id` enum and the reference VIA implementation. For a long
//! time this project had none of the hardware, and that caveat lived here.
//! On 2026-09-02 a protocol 12 board — a Kiiboom Cybrix 16 — answered this
//! code for the first time: the handshake, a full keymap read across its
//! matrix, and a write confirmed by read-back and then undone. That verifies
//! the framing, the report size, and the four command ids against one real
//! board on one protocol revision; protocol 9 remains transcription, still
//! unmet by hardware.
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
