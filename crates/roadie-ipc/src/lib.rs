//! The agent↔GUI IPC contract.
//!
//! The tarpc service definition and the wire types it carries are re-exported
//! at this crate's root; [`transport`] is the cross-platform local-socket
//! transport that carries them. This is a leaf crate — it depends on
//! `roadie-core` and nothing else internal to the workspace — so the GUI (a
//! pure IPC client) can pull in the wire contract without linking
//! `roadie-hid`/`hidpp`/`async-hid`. The agent-side runtime that answers
//! these RPCs (hook runtime, device I/O, the Actions Ring's session state, …)
//! stays in `roadie-agent-core`, which depends on this crate rather than
//! the other way around.

pub mod client;
pub mod desk;
mod ipc;
pub mod transport;

pub use ipc::*;
