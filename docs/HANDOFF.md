# Handoff — where this fork stands

`AGENTS.md` is the standing contract: architecture, gate commands, subsystem
rules. This file is the other half — the state of *this fork's* work, the
things that cost real time to learn the first time, and what comes next.
Read it before picking anything up.

Last updated after PR #11. `master` is clean and everything below is merged.

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

## 6. What comes next

In recommended order. Reasoning included so it can be argued with rather than
just followed.

1. **A DDC host backend, so monitors actually work.** Highest value: one
   implementation reaches essentially every desk monitor regardless of brand,
   and brightness is the control people reach for most. **Open question: which
   platform first.** macOS is the best guess — this project is a Logitech
   Options+ alternative and its macOS path is the most developed — but that
   has not been confirmed, and it decides the order. Ask.
2. **`roadie monitor` CLI and MCP tools** on top of that backend.
3. **Audio interfaces**, starting with the Focusrite Scarlett family. Nadir
   asked about these directly. No cross-vendor standard exists, so this is
   per-vendor work — unlike monitors, cameras and macro pads, where a standard
   did the heavy lifting.
4. **Elgato Key Lights** — network-controlled, well documented, and adjacent
   to the Stream Deck work already merged.
5. **Headsets**, then **MIDI pads**, then **RGB on other brands**, then
   **other vendors' mice and keyboards**.

## 7. Brand

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
