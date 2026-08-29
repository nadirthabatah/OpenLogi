---
paths:
  - "crates/roadie-hook/**"
  - "crates/roadie-inject/**"
  - "crates/roadie-hid/**"
  - "crates/roadie-display/**"
---

# Platform / cfg-gated code — macOS-green is a trap

macOS-green proves **nothing** about `#[cfg(target_os = "linux")]` /
`windows` code. Recent agent failures that only showed up on CI Linux:

- Shadowing a crate-level constant with a local `const` of a different type
  (e.g. `LOGITECH_VENDOR_ID: u16` next to `use crate::LOGITECH_VENDOR_ID`
  which is `u32`) — E0255 / E0308, **only compiles on Linux**.
- Importing a name that only exists on another OS, or redefining one that
  master already exports from `lib.rs`.

When the diff touches any of:

- `crates/roadie-hook/src/linux.rs` / `windows.rs`
- `crates/roadie-inject/src/inject/linux.rs` / `windows.rs`
- `crates/roadie-agent/src/autostart/linux.rs` / `windows.rs`
- `crates/roadie-camera/src/capture_linux.rs`, `capture_windows.rs`,
  `com_windows.rs`, `uvc_windows.rs`, `uvc_linux.rs`, `linux.rs`
- `crates/roadie-display/src/linux.rs` / `macos.rs` / `windows.rs`
- `crates/roadie-hid/src/channel/transport.rs` (has `#[cfg]` branches)
- any `#[cfg(target_os = …)]` block, in any crate

you MUST either:

1. Cross-check with devenv when available:
   `devenv tasks run roadie:check-windows` (also
   `cargo xtask ci clippy-windows`), or
2. Manually re-read every changed cfg-gated file against **current master** for:
   - name collisions with existing `pub use` / `pub const` items
   - type mismatches (`u16` vs `u32`, `Option` arity, new enum fields)
   - call sites that gained args on master (e.g. `with_runtime`, `build_device_list`,
     `dispatch_action`) but the PR still uses the old signature

Do not claim "cross-platform green" without CI (or a local cross-lint) having
actually run those targets. `RUSTFLAGS=-D warnings` is global in CI — plain
warnings fail there too.

There is no Linux equivalent of the Windows task, and it cannot be complete if
there ever is: `roadie-camera`'s Linux backend needs kernel headers
(`v4l2-sys` wants `linux/videodev2.h`), so it does not cross-compile from macOS
at all. For a Linux-only change outside camera, rustup's
`aarch64-unknown-linux-musl` target covers the rest — but leave out `roadie`,
`roadie-cli` and `roadie-assets`, whose `ureq → ring` dependency needs a
cross C toolchain:

```sh
cargo clippy --target aarch64-unknown-linux-musl \
  -p roadie-hook -p roadie-inject -p roadie-hid -p roadie-hidpp \
  -p roadie-core -p roadie-agent -p roadie-agent-core -p roadie-ipc \
  -p roadie-permissions --all-targets -- -D warnings
```

Everything that recipe skips — camera on Linux above all — is CI's alone to
catch.

## macOS can be type-checked from Linux, and usually should be

`cargo check` does not link, so a macOS-only file can be compiled — types,
signatures, lifetimes, borrows and all — on a Linux container with no Mac
anywhere:

```sh
rustup target add aarch64-apple-darwin
cargo clippy -p <crate> --target aarch64-apple-darwin --all-targets -- -D warnings
```

This works for any crate whose macOS dependencies are pure Rust, which the
`objc2` family is. It caught every mistake in `roadie-display`'s `IOAVService`
backend without a Mac in the room, and it is the difference between writing
native FFI blind and writing it against a compiler.

What it does **not** prove is anything about running: no linking, no
frameworks resolved, no private symbol confirmed to exist at runtime, and no
signature checked against the one Apple actually ships — a `dlsym`'d function
whose declared signature is wrong type-checks perfectly and is undefined
behaviour when called. Those still need the CI macOS jobs and, in the end, a
Mac.
