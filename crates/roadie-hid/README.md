# roadie-hid

HID++ device discovery and control helpers for OpenRoadie.

This crate is the OpenRoadie-oriented HID layer built on top of
[`async-hid`](https://crates.io/crates/async-hid) and the workspace's vendored
[`roadie-hidpp`](https://crates.io/crates/openlogi-hidpp) protocol crate. It
owns device enumeration, receiver routing, shared-channel transport setup, and a
small set of typed operations used by the CLI, agent, and GUI.

Use this crate when OpenRoadie needs to talk to Logitech HID++ devices through the
host HID stack. Use `roadie-hidpp` directly when implementing protocol-level
feature support that is independent of OpenRoadie's discovery and transport
policy.

Public entry points include:

- `enumerate` for a one-shot inventory of receivers and paired devices.
- `list_pairing_receivers`, `run_pairing`, and `unpair` for receiver pairing.
- `get_dpi`, `set_dpi`, SmartShift, high-resolution wheel, thumbwheel, and
  reprogrammable-control helpers for supported HID++ features.
- `set_keyboard_color` / `set_keyboard_color_with` for solid keyboard RGB —
  preferring the typed `ColorLedEffects` (`0x8070`) wrapper and falling back to
  the `PerKeyLighting` (`0x8080`) stream.
- `dump_features` and `dump_reprog_controls` for diagnostics.

Protocol-level feature support lives in `roadie-hidpp`; this crate adds the
discovery, routing, fallback, and error-classification policy on top.
