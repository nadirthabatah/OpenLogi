//! The desk beyond HID++, on the agent's side of the socket.
//!
//! The GUI does no device I/O — that is the whole of its contract — so the
//! panels that show a monitor's brightness, a Key Light's colour, a Stream
//! Deck's keys or a preamp's gain ask the agent, and this is what answers.
//!
//! One child per family, because they share almost nothing but this file: a
//! monitor speaks I²C over the video cable, a Key Light speaks HTTP over the
//! network, a Focusrite speaks a vendor protocol on its own USB interface, a
//! TourBox speaks bytes down a serial port, and a Stream Deck and a VIA board
//! each speak their own HID dialect. What they do share is the shape of the
//! answer — enumerate, probe, and say why when it did not work — and
//! [`blocking`], which keeps all of it off the runtime serving the input hook.
//!
//! Nothing is cached between calls. A monitor unplugged and plugged back in
//! gets a new handle, a light's address comes from a DHCP lease, and a stale
//! handle that writes to the wrong device is a worse failure than an open()
//! per request — which is cheap next to the exchange that follows it.

mod audio;
mod controllers;
mod decks;
mod displays;
mod lights;
mod pads;

pub use audio::{list_audio_interfaces, set_audio_input};
pub use controllers::list_controllers;
pub use decks::{list_stream_decks, set_stream_deck};
pub use displays::{list_displays, read_display, set_display};
pub use lights::{list_network_lights, set_network_light};
pub use pads::list_macro_pads;

/// Run blocking device work off the runtime that is also serving the hook.
///
/// `lost` is what to answer if the blocking task panics or is cancelled. It is
/// a closure so that nothing is built for the case that almost never happens,
/// and it is a parameter rather than [`Default`] because the honest answer
/// differs: an empty list is a fine way to say "found none", but an empty
/// reading would be a lie about a monitor that was never asked.
///
/// Not every caller needs it. The Stream Deck and VIA host layers are already
/// async — they sit on `async-hid` — and wrapping those in `spawn_blocking`
/// would move a future onto a blocking thread to be polled there, which is
/// both pointless and a way to occupy that pool for the length of a USB
/// timeout. Those await directly; the ones on I²C, HTTP, `nusb` and
/// `serialport` come through here.
async fn blocking<T, F, L>(work: F, lost: L) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    L: FnOnce() -> T,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(answer) => answer,
        Err(_) => lost(),
    }
}
