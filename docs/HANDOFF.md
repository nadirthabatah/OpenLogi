# Handoff — where this fork stands

`AGENTS.md` is the standing contract: architecture, gate commands, subsystem
rules. This file is the other half — the state of *this fork's* work, the
things that cost real time to learn the first time, and what comes next.
Read it before picking anything up.

Last updated after PR #12. `master` is clean and everything below is merged.

---

## 1. Who this is for

Nadir Thabatah is blind. He works by dictation and screen reader, often from
an iPhone. That one fact decides more design here than any technical
constraint, and it is why several things that look like style rules are
actually correctness rules:

- **Every output is read aloud.** Box-drawing characters, bare symbols, and
  `(s)` plurals are unlistenable, so `roadie-cli/src/spoken.rs` enforces their
  absence and integration tests check the exact shipping strings. Counts and
  verbs have to agree — "the 1 device you cannot open **are** not among them"
  shipped once and is exactly the class `spoken::assert_agrees` now catches.
- **Colour is never the only signal.** Tested, not merely intended.
- **Padding is a bug, not cosmetics.** Monitors pad text with NUL, and
  `str::trim` does not remove it. A padded model name reaches a screen reader
  as a name followed by silence, with nothing on screen to explain it. That
  was a real defect found in `roadie-ddc`, and the same trim now guards both
  the capability parser and the EDID parser.
- **Never say a word twice.** `Edid::describe` resolves `GSM` to "LG" and then
  declines to prepend it to a name that already starts with it, because
  "LG LG ULTRAFINE" is worse aloud than on screen.
- **An unrecoverable action needs a name.** `PowerMode::Off` can stop a
  monitor answering DDC, leaving the bezel button as the only way back. For
  someone who cannot see the bezel that is not an inconvenience, so
  `is_recoverable` exists to be checked and anything unrecognised is assumed
  unrecoverable.

When in doubt about wording, read the sentence out loud.

## 2. Where the work is

- **This fork:** `github.com/nadirthabatah/OpenRoadie`, branch `master`.
- **Upstream:** AprilNEA's OpenLogi. Still the source of most of the HID++
  code. Its brand assets were deleted rather than renamed.
- **The project is OpenRoadie; the command is `roadie`.** Every subcommand
  follows: `roadie doctor`, `roadie devices`, `roadie via`, `roadie mcp`.

**A trap worth naming.** `nadirthabatah/Open-switch` is a *different*
repository — the original planning notes, docs only, no code. A session can be
started with that as its working directory while all the real work is in
OpenRoadie. Check `git remote -v` before believing a clean working tree.

## 3. What is built

Merged, in order:

| PR | What |
| --- | --- |
| 1–2 | MCP server over stdio, and the Elgato Stream Deck driver |
| 3 | Cross-vendor device survey — `roadie devices` says what it can configure on each |
| 4 | VIA/QMK keyboards and macro pads |
| 5 | Portable setup bundles — export a profile, import it on another machine |
| 6 | `roadie doctor` diagnostics |
| 7 | MCP parity with the CLI |
| 8 | The rename: 720 files, OpenLogi to OpenRoadie, `openlogi` to `roadie` |
| 9 | The logo, and the retirement of upstream's artwork |
| 10 | `roadie-ddc` — DDC/CI, MCCS and EDID as a pure crate |
| 11 | Repository URLs pointed at the post-rename name |

### Device categories that work today

Logitech HID++ mice and keyboards, Elgato Stream Decks, QMK/VIA keyboards and
macro pads, UVC webcams, Logitech standalone lights.

### Monitors are half-built

`roadie-ddc` is the pure protocol: packet framing, VCP features, capability
strings, EDID. 91 unit tests, no host I/O, holds the wasm portability claim.
**Nothing yet reaches an actual monitor** — that needs a host backend, and it
differs per platform:

| Platform | API | Notes |
| --- | --- | --- |
| macOS | `IOAVService` on Apple silicon, `IOI2CSendRequest` on Intel | Private framework. What MonitorControl and BetterDisplay use. Never works on the built-in display. |
| Windows | `dxva2.dll` | Higher level: it does the framing itself, so `roadie-ddc`'s packet layer is unused there |
| Linux | `/dev/i2c-*`, discovered through `/sys/class/drm/*/ddc/i2c-dev/` | The kernel already publishes each display's EDID at `/sys/class/drm/*/edid`, so displays can be named with no I2C access at all |

That table is why the backend trait belongs at the *VCP* level (`get`, `set`,
`capabilities`) rather than at the packet level — Windows never sees a packet.

## 4. Working agreements that cost time to learn

**`cargo run -p xtask -- ci` is the only gate that counts.** A hand-rolled
gate built from remembered commands once passed while missing MSRV,
cargo-deny and shell entirely, and excluded a crate CI does not exclude. The
repo has its own runner; use it. On Linux it reports 11 passed and 1 skipped;
the skip is the macOS test job and it is **not** a pass — say so in the PR.

Its prerequisites on a fresh container: `libxkbcommon-dev`,
`libxkbcommon-x11-dev`, `shellcheck`, `shfmt`, `cargo-deny`,
`gcc-mingw-w64-x86-64`, and the `x86_64-pc-windows-gnu` and
`wasm32-unknown-unknown` targets.

**Never push while a run is in progress on the same branch.** CI sets
`concurrency: cancel-in-progress: true`, keyed on the ref. Fourteen commits
once went unverified this way while the last completed run was fourteen
commits stale, and "the gate is green" was reported on the strength of it.
Batch locally, push once, wait. Different branches are safe — the key includes
the ref.

**Clippy reports as warnings; `-D warnings` makes them failures.** Grepping
output for `^error` hides every one of them. Check the exit code — and not
after a pipe, where `$?` is the last command's status, not clippy's.

**A test that agrees with the code proves nothing.** Wire-format tests spell
byte sequences out literally rather than computing them from the function
under test. `51 82 01 10 AC` is the documented DDC read-brightness sequence
and checks the framing from outside; the packed EDID codes `0x10AC` and
`0x1E6D` are the published PNP IDs for Dell and LG.

**Check the echo.** Neither HID++ VIA nor DDC has sequence numbers. A reply
read too early is the *previous* answer, and every reply after it is shifted
by one — brightness set from a contrast reading, with nothing reporting an
error. Both protocols echo what they are answering. Both are checked.

**Forgive malformed data from hardware, and record what you forgave.**
Monitors ship unbalanced capability strings and miscomputed EDID checksums. A
strict parser rejects working hardware, which is the wrong trade for a tool
whose job is the hardware someone already owns. Recover, and put what you
recovered from in a `warnings` field so `doctor` can print a specific sentence
instead of a vague one.

### Mutation testing is what actually finds the defects here

The method: change one thing so the code is deliberately wrong, run the tests,
and require a failure. A mutation that survives is a claim nothing checks.
Script it — apply, test, revert — and expect roughly a fifth to survive the
first pass.

Track record across the fork: five sweeps, and **every gap was in code that
had prose about it but no test.** The comments were consistently ahead of the
tests. The driver logic, where the consequences are worst and the care was
highest, came back clean each time.

Two cautions learned the hard way. `rustfmt` reflows source, so a
pattern-match mutation script silently finds nothing — always report
"pattern not found" separately from "survived". And distinguish a real gap
from an *equivalent* mutant: `roadie-ddc` has one where truncating an
over-long input name cannot produce a match, because no valid name is as long
as the buffer. That one is documented in the code rather than papered over
with a test that would not really bite.

## 5. The gap that only Nadir can close

**No code in this fork has ever touched a physical peripheral.** The container
has no USB. Everything is unit-tested and checked by CI on three platforms,
and none of that is the same as a device answering.

`docs/VERIFYING.md` exists to close it in one sitting: twelve ordered steps,
nothing destructive, undo commands throughout, in priority order. It is the
highest-value thing to do the next time hardware and time coincide.

## 6. What comes next: monitors, in build order

The last handoff left "which platform first" open. It is answered here, and the
answer is that it was the wrong question to block on. Everything except a panel
actually answering can be built and proven with no hardware attached and no
macOS host in the room, so the platform choice does not gate the work — it only
decides which backend gets *verified* first, and that is a hardware question
Nadir settles at his desk, not a design question a session settles for him.

So: the trait, the Linux backend and a mock land first, because those are the
parts a Linux container can prove. macOS is written behind the same trait and
compiled by CI's two macOS jobs. Windows is cross-compiled. Verification then
starts wherever Nadir happens to be sitting.

### 6.1 The seam is a new crate, `roadie-display`

`roadie-ddc` stays pure. It is on the wasm portability list in
`xtask/src/commands/ci/jobs/steps.rs`, and that list is deliberately a claim a
crate earns and then has to keep — adding an ioctl to it would quietly retire
the claim. Host I/O goes in a sibling crate, exactly as `roadie-hid` is the
host-facing sibling of `roadie-device`.

What gets created:

| File | What it carries |
| --- | --- |
| `crates/roadie-display/src/lib.rs` | The `VcpBackend` trait — `get`, `set`, `capabilities` — a `Display` handle carrying the EDID identity, and `enumerate` |
| `crates/roadie-display/src/linux.rs` | `/sys/class/drm` walk, then `/dev/i2c-*` through the `ddc/i2c-dev` symlink, `I2C_RDWR` |
| `crates/roadie-display/src/macos.rs` | `IOAVService` on Apple silicon, `IOI2CSendRequest` on Intel |
| `crates/roadie-display/src/windows.rs` | `dxva2`, which does its own framing |
| `crates/roadie-display/src/mock.rs` | A scripted display behind the same trait, so the CLI and the MCP tools have something to answer them with nothing plugged in |

The trait sits at the VCP level rather than the packet level because Windows
never sees a packet: `dxva2` takes a feature code and a value. On Linux and
macOS the same call goes through `roadie-ddc`'s framing. Putting the seam one
layer lower would give Windows a packet layer to route around.

Two properties the Linux backend gets for free and should not give up. The
kernel publishes each display's EDID at `/sys/class/drm/*/edid`, so a machine
where the I2C nodes are not readable still produces a named list rather than an
empty one — the failure says *which* screen it could not reach. And enumeration
that needs no I2C access means `roadie devices` can include monitors without
asking for a permission first.

Three obligations belong to the backend rather than to `roadie-ddc`, because
they are transport facts:

- **Timing floors.** Roughly 40 ms between requests and 50 ms after a write.
  They are floors, not targets, and the failure mode of rushing a panel is a
  garbled reply rather than a clean error.
- **Retries.** A garbled reply is a transport event, so the transport decides
  whether to ask again.
- **Nothing on Windows.** `dxva2` does framing, timing and retry itself. The
  backend there is thin on purpose, and that asymmetry is the whole reason the
  trait is where it is.

The echo check stays in `roadie-ddc`, where it already is. Neither DDC nor VIA
has sequence numbers, so a reply read too early is the previous answer and
every reply after it is shifted by one.

### 6.2 The safety gate is part of the design, not a later polish

`PowerMode::Off` can leave a monitor not answering DDC, with the bezel button
as the only way back, and `Save Current Settings` writes to memory with a
finite number of erase cycles. So `is_recoverable` is checked before any write
crosses it, the CLI refuses an unrecoverable one without an explicit flag and
names the physical button that will be needed, and saving to the monitor's own
memory is its own subcommand that is never appended to a set.

### 6.3 `roadie display`, not `roadie monitor`

The previous handoff called it `roadie monitor`. Reconsidered, because
`crates/roadie-cli/src/cmd/mcp/tools/monitor.rs` already exists and watches
physical input — "monitor" there is the verb. Two unrelated things under one
word is bad on screen and worse aloud, so the screen is `roadie display` and
the MCP module is `displays.rs`.

`crates/roadie-cli/src/cmd/display.rs` plus a `Display` arm in `Command`.
Subcommands: `list`, `capabilities`, `get`, `set`, `input`, and `brightness` as
the shorthand for the one control people actually reach for. Output goes
through `spoken.rs` like every other subcommand, and the integration tests
spell the shipping strings out rather than computing them.

### 6.4 MCP parity, because that is the standing rule

`crates/roadie-cli/src/cmd/mcp/tools/displays.rs`, registered in `tools.rs`:
`list_displays`, `get_display_setting`, `set_display_setting`. Every category
since PR 7 reaches the MCP server as well as the CLI, and a monitor is the
category most worth asking an assistant to adjust, since the alternative is an
on-screen menu driven by bezel buttons.

### 6.5 The survey has to know about screens

`roadie-catalog` gains a driver for DDC displays, so `roadie devices` includes
the monitor in the one list it promises. The crate is pure, so this is ordinary
tested work.

### 6.6 After monitors

Unchanged in order, with one note. Audio interfaces starting with the Focusrite
Scarlett family are per-vendor USB work with no standard underneath, so they
cost the most per device. Elgato Key Lights are HTTP over the local network,
well documented, and adjacent to the Stream Deck work already merged — which
makes them the better thing to reach for in a session with no hardware to hand.
Then headsets, MIDI pads, RGB on other brands, and other vendors' mice and
keyboards.

## 7. What a session can do with nobody at the desk

Nadir travels, and a session that stalls waiting for a monitor to be plugged in
wastes the time. The split is sharper than it looks, so it is written down.

Buildable, testable and gate-green with no hardware and no computer in front of
anyone:

- Everything in 6.1 through 6.5. The mock backend is what makes it true: the
  CLI, the MCP tools and the survey can all be driven end to end against a
  scripted display.
- The macOS FFI. It cannot be compiled in a Linux container, but CI's
  `tests (macos, arm64)` and `tests (macos, x86_64)` jobs compile it on a PR.
  CI is the macOS compiler for a travelling session; it is not a substitute for
  a run.
- The Windows backend, cross-compiled locally against `x86_64-pc-windows-gnu`
  and by the `clippy (windows)` job.
- A mutation sweep over whatever lands. The fork's record is that every gap
  found this way was in code that had prose about it and no test.
- The `docs/VERIFYING.md` steps for monitors, written ahead of the hardware so
  the first sitting is one ordered pass rather than an exploration.

Needs a panel answering, and nobody else can do it:

- The reply checksum seed. Requests and replies are checksummed differently,
  and a monitor that disagrees looks exactly like a monitor that is not there.
- The timing floors, on real panels rather than on the specification's minimum.
- Which input-source values a given monitor accepts. Above `0x12` is vendor
  territory and USB-C has no standard number at all.
- Whether each screen on the desk speaks DDC in the first place.
- Everything already in `docs/VERIFYING.md`, steps 1 through 9, none of which
  any hardware has ever run.

## 8. Brand

The logo is Nadir's own. `design/LICENSE` reserves it to him and states
explicitly that none of it derives from OpenLogi — upstream's icon set is
AprilNEA's and was deleted rather than renamed.

Three assets are cut from the single supplied file: the knob (app icon), the
letter R (alternate icon), and the full lockup (README headers, six
languages). Both marks are cut with circular masks rather than rectangles,
because the knob and the R touch in the source. The menu-bar glyphs are the
exception — drawn as geometry, since a photographic knob turns to porridge at
18 points and macOS flattens a template icon to one colour anyway.

`AppIcon::Prism` is now `AppIcon::Letter`, with a serde alias so configs
written before the change still parse. That alias is a schema contract, the
same as the `openlogi` alias on the default variant. Do not remove either.
