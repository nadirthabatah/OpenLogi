//! What this build knows about a peripheral it finds plugged in.
//!
//! The promise this crate exists to keep is one list: plug anything in, and be
//! told what it is and what can be done with it — whoever made it. Vendor
//! software answers that question for one brand and stays silent about the
//! rest of the desk. Answering it for everything at once means the answer has
//! to be *honest about not knowing*, which is why [`Support`] has a variant
//! for "detected, and nothing here drives it" rather than quietly omitting the
//! device. A peripheral this build cannot configure is still a peripheral you
//! own, and hiding it would tell you nothing about why your hub looks empty.
//!
//! Everything here is pure: identities in, a verdict out. No I/O, no host
//! calls, no OS. Enumeration belongs to `roadie-hid` and `roadie-camera`;
//! this crate only decides what the enumerated things *are*, which is the part
//! worth testing without hardware — and the part that must keep working on a
//! machine with none of the hardware attached.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod hidpp;
pub mod identity;
pub mod support;

pub use identity::{IdSource, Identity};
pub use support::{Driver, Peripheral, Support};
