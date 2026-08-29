# roadie-device / roadie-hid — the HID++ device layer

This guide covers the two-crate seam; it lives here because `roadie-device`
owns the layer. `crates/roadie-hid/AGENTS.md` points back here.

- The HID++ layer is split at `roadie_device::backend::HidBackend`.
  `roadie-device` holds everything that knows the protocol and nothing about
  a host — enumeration policy, the probe, the write layer, sessions, pairing —
  and is handed a backend. `roadie-hid` is the backend for this machine
  (`async-hid`, the Windows composite channel, Input Monitoring, the on-disk
  probe cache) plus `host`, which supplies it to the entry points so the
  public API still reads `set_dpi(route, dpi)`. A change that makes
  `roadie-device` depend on a host breaks CI's `wasm (portable crates)` job,
  which is the point of that job.

- `roadie-hidpp` (lib name `hidpp`, 0BSD) is a **hard fork**, not a tracked vendor
  copy — read `crates/roadie-hidpp/AGENTS.md` before touching that crate. Its own
  rules (protocol facts from official specs, typed wire values end to end) live there
  now, not here, to keep this file to the `roadie-hid` side only.
- Device "kind" flows through four incompatible vocabularies (Bolt pairing register,
  feature `0x0005` `DeviceType` — defined in `roadie-hidpp` — the assets-registry
  string, and `roadie_core::device::DeviceKind`) — the same small integers mean
  different things in each. Never cross them by raw value; convert at the boundary.
  `kind` is identity-only; capability decisions come from the feature table.
- Enumeration runs on a poll with cache/ledger grace logic so sleeping or briefly
  unreachable devices keep their identity and panels. Changes to probing must keep the
  "replay last-good inventory through transient failures" behavior intact — run the
  inventory/watcher tests and think about the partial-failure paths, not just clean
  enumeration.
