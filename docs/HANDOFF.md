# Handoff — where this fork stands

`AGENTS.md` is the standing contract: architecture, gate commands, subsystem
rules. This file is the other half — the state of *this fork's* work, the
things that cost real time to learn the first time, and what comes next.
Read it before picking anything up.

Last updated on the `claude/openroadie-handoff-m5hphl` branch, which carries the
monitor work described in section 6 and is not yet merged. Everything in
section 3 up to PR #12 is on `master`.

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

- **Never present a decision as a picker.** An interactive multiple-choice
  list is inaccessible: it has to be navigated rather than read, and the
  options do not simply arrive in the ear the way prose does. When an agent
  needs Nadir to choose something, the options go in plain sentences and he
  types the answer back. This applies to Claude Code's `AskUserQuestion` tool
  specifically — do not call it. Write the options out instead.

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
| 12 | This handoff |

### Device categories that work today

Logitech HID++ mice and keyboards, Elgato Stream Decks, QMK/VIA keyboards and
macro pads, UVC webcams, Logitech standalone lights, and — on the branch, not
yet on `master` — monitors over DDC/CI, Elgato Key Lights over the network, and
TourBox controllers over their serial port.

### TourBox, as of the branch

`roadie-tourbox` is the fourth device family on this branch and the first that
is neither HID nor network. A TourBox presents a **USB CDC serial port** and
streams **one byte per event**: the low six bits name the control, the high two
say what happened to it. For a button, bit 7 is release. For a wheel, bit 6 is
direction and bit 7 is "the turn has ended" — so a wheel uses all four
combinations and a button uses two. No framing, no length, no checksum, no
sequence number. A byte read is an event and a byte lost is an event lost.

That shape decides the crate. There is nothing to resynchronise to, so a byte
the build cannot explain is either a control from a model it has never met or a
corrupt byte, and *nothing in the encoding tells those apart* — hence a typed
error rather than a nearest-match, because a wrong guess here is a keystroke
nobody asked for. Buttons and wheels are separate types (`Button` and `Wheel`)
so that the combinations which really are impossible — a button that claims to
have turned — cannot be constructed and have to be rejected only at the one
place bytes come in.

It is one crate rather than two, like `roadie-keylight` and unlike
`roadie-ddc`: the host half is a serial port with no platform FFI in it, so a
`serial` feature says the same thing as a sibling crate would, at far less
ceremony. The protocol half holds the wasm portability claim.

**Three things are hardware-verified and one is not.** On 2026-08-31, against
a TourBox Elite on this desk: enumeration finds it by USB identity
(`c251:2005`, serial `00000001`), `roadie devices` files it as configurable,
and the port opens with nothing else holding it. What is *not* verified is the
control codes themselves — no button has yet been pressed with this build
listening. They are transcribed from published open-source drivers and pinned
by tests, which proves the code does what the crate claims and cannot prove the
claims match the device.

**Cross-checking three drivers was worth more than the mutation sweep.** The
codes were first transcribed from one project, then compared against two more
written independently, for different models, in different languages. That is
not hardware, but it is three witnesses, and it settled one open question and
found one outright defect that no test could have.

The open question was the knob's press byte. One source records `0x77`, which
would make the knob the only control setting a turn bit while being pressed.
Two others record `0x37` — and the first source's own *release* byte is `0xb7`,
which is `0x37` with the release bit set and therefore inconsistent with its own
press. So `0x77` is almost certainly a mis-transcription. The build implements
`0x37` and still **rejects `0x77` by name** rather than quietly accepting both,
so hardware can overturn it; the test is
`the_disputed_knob_byte_is_refused_rather_than_guessed_at`.

The defect was worse and is the reason this exercise earned its keep. **A wheel
reports the end of a turn**, and this build rejected those bytes as impossible.
A wheel sends a run of detents and then one more byte carrying the same control
code and direction with the high bit set — the same bit that means "released" on
a button, which is consistent rather than coincidental, since both mark the end
of something held. One driver names them the `_STOP` family; another prints one
from live hardware. Every turn of every wheel would have ended in a spurious
error at the moment the hand stopped. The type model was wrong too, and said so
out loud: it claimed "a wheel turns one way or the other and is never released",
which is exactly the kind of confident sentence that should be checked against a
second source. `TurnPhase` now carries it, and a wheel has no impossible
action at all — both of its high bits are meaningful and independent.

The general lesson, which is the one to carry: **the mutation sweep and the
cross-check find different things.** The sweep proves the tests bite on the
behaviour the code claims. It cannot see a claim that is wrong in the code and
the tests alike, because both were written from the same reading of the same
source. Only a second reading finds that, and here it took a third to break the
tie.

**Two findings worth keeping.** macOS publishes every serial device twice, as
`/dev/cu.NAME` and `/dev/tty.NAME`; listing both reported one controller as
two, and the `tty.` half is also the wrong one to hand anybody, because opening
it blocks waiting for a carrier a controller never asserts. And the 94-byte
setup message that the vendor's software sends on connect configures *haptics*,
not event reporting — a TourBox streams whether or not anything has ever talked
to it, which is why reading one needs no handshake and no write access.

Still unverified: every model other than the Elite, and the setup message,
which is written and has never been sent to a device.

### Monitors, as of the branch

`roadie-ddc` is the pure protocol: packet framing, VCP features, capability
strings, EDID. 91 unit tests, no host I/O, holds the wasm portability claim.

`roadie-display` is the host half: the `VcpBackend` seam, the `Ddc` adapter
carrying framing and timing and retries, backends for all three platforms, and
`mock::Panel`, a monitor made of software that answers *packets* so everything
above it can be driven with nothing plugged in. `roadie display` is the CLI,
`list_displays` / `read_display_settings` / `set_display_setting` are the MCP
tools, and `roadie devices` includes the screen.

**One monitor has now been reached, and it half-answered.** On 2026-08-30 the
macOS backend ran against Nadir's desk for the first time: the registry walk
found the panel, the `dlopen`ed private calls behaved, and the EDID read at
I²C `0x50` named it — an LG TV, which then never acknowledged DDC at `0x37`
(`IOAVServiceWriteI2C` 0xe0114000, LG TVs speak CEC instead). So the transport
and identification were verified and the reply checksum seed was not; that
needed a desktop monitor.

**The desktop monitor arrived, and the seed is verified.** On 2026-09-02 an
RTK HG560T34 on USB-C answered everything: brightness and contrast reads, a
full capability string (MCCS 2.2, seven switchable inputs), and a brightness
write that read back correctly and was restored. Replies came back, parsed,
and passed the checksum — which closes the one gap no mock could. A detail
worth keeping: macOS listed only the LG TV as an online display at the time,
and the registry walk still found and drove the HG560T34 — the DDC transport
does not care whether the window server is drawing to a panel. The backends differ per platform,
and the table below is what each one had to be written against:

| Platform | API | Notes |
| --- | --- | --- |
| macOS | `IOAVService` on Apple silicon | Private framework, resolved by `dlopen`. What MonitorControl and BetterDisplay use. Never works on the built-in display. Intel's `IOI2CSendRequest` path is deliberately **not** implemented — an Intel Mac is told so plainly rather than handed a second untestable path. |
| Windows | `dxva2.dll` | Higher level: it does the framing itself, so `roadie-ddc`'s packet layer is unused there |
| Linux | `/dev/i2c-*`, discovered through `/sys/class/drm/*/ddc/i2c-dev/` | The kernel already publishes each display's EDID at `/sys/class/drm/*/edid`, so displays can be named with no I2C access at all |

That table is why the backend trait belongs at the *VCP* level (`get`, `set`,
`capabilities`) rather than at the packet level — Windows never sees a packet.

The EDID is read differently on each: Linux from `/sys/class/drm/*/edid`, which
needs no permission at all; macOS off the EEPROM at I²C address `0x50`, on the
transport that already exists, so identification needs no second private
dependency; Windows not at all, which is a real gap — `dxva2` describes most
panels as "Generic PnP Monitor", so displays there are numbered rather than
named.

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

**macOS compiles here.** `cargo check` does not link, so
`--target aarch64-apple-darwin` type-checks a macOS-only file — signatures,
lifetimes, borrows and all — on a Linux container with no Mac anywhere. It
works for any crate whose macOS dependencies are pure Rust, which the `objc2`
family is, and it caught every mistake in `roadie-display`'s `IOAVService`
backend. `rustup target add aarch64-apple-darwin` and then clippy that target
alongside the Windows one. What it does not prove is anything about *running*:
no linking, no frameworks resolved, no private symbol confirmed to exist, and
no `dlsym`'d signature checked against the one Apple ships — a wrong one
type-checks perfectly and is undefined behaviour when called. The full note is
in `.claude/rules/cross-platform.md`.

**A new dependency can move the `gpui` git pin, and a plain `cargo check` is
enough to do it.** The manifest entry has no rev, so a re-resolve follows the
branch's HEAD. It does not look like a dependency problem: it is a compile
error deep inside `gpui_linux` about `ImageFormat::iter`, with a note about two
versions of `strum`. `git checkout Cargo.lock` does not fix it, because the
next `cargo check` moves it again. Let the lock pick up the genuinely new
packages, then `cargo update -p gpui --precise cc053a4a...`, then check that
`git diff Cargo.lock` shows only the new crates.

**Read the three surfaces side by side; the odd one out is usually a bug.**
Every device family is reachable three ways — the command line, the MCP
server, and the GUI through the agent — and they are written at different
times, so they drift. Four defects this session were found that way and none
of them by a failing test. A Key Light that stopped answering was listed by
the CLI and by MCP, both with a comment explaining why, and silently dropped
by the agent, so it vanished from the GUI. `roadie doctor` claimed "no
peripheral of any kind" while `roadie devices` had grown two families it did
not check. The technique is cheap: pick one question — what happens when the
device does not answer? — and ask it of all three.

**A mutation can survive because the data happens to be sorted.** Swapping
"take the newest applicable firmware table" for "take the last one listed"
changed nothing in `roadie-scarlett`, because the tables are listed oldest
first. That is a fragility rather than an equivalence: a table added out of
order would have selected the wrong layout silently. The fix is a test with
deliberately jumbled data, which is the only thing that can tell the two apart
— and separately an assertion that the real data *is* ordered, which the code
does not rely on but which catches the mistake.

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

Track record across the fork: six sweeps, and **every gap was in code that
had prose about it but no test.** The comments were consistently ahead of the
tests. The driver logic, where the consequences are worst and the care was
highest, came back clean each time.

The sixth sweep, over `roadie-display`, held to that exactly. Twenty-nine
mutations, twenty-seven killed on the first pass. One survivor was an
equivalent mutant — a redundant emptiness check in front of `Edid::parse`,
which rejects a short block anyway, so the guard was a branch that could not
fire; it was deleted rather than given a test that would not have bitten. The
other was real, and was the sort this method exists to find: the Linux
backend's central promise, that a display is never dropped for being
unreachable, had a paragraph of prose and no test, because enumeration read
`/sys/class/drm` directly and `/sys` cannot be written to. Making the root a
parameter turned five claims into tested ones, and the test written for it
immediately exposed a message that nested one error's text inside another's.

The seventh, over `roadie-keylight` and its front ends, found four survivors in
twenty-two and **not one of them was in the code** — all four were tests that
looked like they proved something and did not. The IPv4-preference test
compared against an address the filter already excluded, so any rule at all
would have passed it. The "too wide for the wire" test used 70000, which
truncates to 4464 and clamps to the same answer as the correct code; 65536 is
the value that tells them apart, because it truncates to zero and clamps to the
opposite end. The refusal of a call that changes nothing was reachable only
through the network, so it was extracted into a function that could be tested.
And `full_description` had never been checked for a network device at all. That
is a different failure mode from the earlier sweeps — prose ahead of tests —
and worth naming: **a test can be present, passing, and evidence of nothing.**

The eighth, over `roadie-tourbox`, ran eighteen and killed eighteen. That is
the first clean first pass on the project, and it is worth being suspicious of
rather than pleased about: the crate is small, its logic is one mask and two
lookups, and the tests were written from the same transcription the code was.
A sweep cannot find a shared misreading of the source material — only hardware
can, which is what section 5's outstanding button pass is for.

Three cautions learned the hard way. `rustfmt` reflows source, so a
pattern-match mutation script silently finds nothing — always report
"pattern not found" separately from "survived".

**Restore the file's timestamp too, not just its contents.** `mv backup.bak
file` puts the bytes back and takes the backup's *older* mtime with them, which
is older than the artifact cargo built from the mutated source. Cargo then
judges the build fresh and the next `cargo test` runs **the previous mutant's
binary against clean source**. This bit exactly once, on the `roadie-tourbox`
sweep: a test that had passed all day failed immediately after the sweep, with
the code visibly correct on disk. Read the other way round, the same trap turns
a stale pass into a reported survivor, which is the more expensive direction.
`touch` the file after restoring it. And distinguish a real gap
from an *equivalent* mutant: `roadie-ddc` has one where truncating an
over-long input name cannot produce a match, because no valid name is as long
as the buffer. That one is documented in the code rather than papered over
with a test that would not really bite.

## 5. The gap that only Nadir can close — and the first sitting

The first hardware sitting happened on 2026-08-30, on macOS at Nadir's desk.
What it verified, for the first time anywhere:

- **HID++ over a Bolt receiver**: an MX Mechanical and an MX Master 3S
  enumerate with battery, serials and feature tables, by direct enumeration.
- **The Stream Deck XL, end to end**: open, brightness, image writes, key
  decode, and the catalogue's key numbering — the physical top-left key is
  key 0, horizontal neighbours differ by 1 and vertical by 8, and press and
  release pair correctly. The visual half (labels upright, unclipped) still
  needs sighted eyes.
- **The macOS display transport and EDID identification** — and not the reply
  checksum seed; see section 3.
- **Discovery honesty**: with no Elgato light on the network, `roadie light
  list` and the OS's own multicast browser agree there is nothing, rather
  than inventing something.

The sitting also found what no test could: **two other programs eat this
desk's Stream Deck.** Elgato's own app holds it exclusively — opens fail,
which the open error now explains. Worse, Logitech's device manager
(`com.logi.cp-dev-mgr`, installed with Logi Options+) holds the deck's
*input* in seize mode: opens succeed, writes land, and every key report goes
to Logitech alone, so the deck looks write-only to anything else. No error
anywhere says so; an independent IOKit listener hearing silence is what gave
it away, and `launchctl bootout gui/<uid>/com.logi.cp-dev-mgr` is what frees
it. `streamdeck verify` now names both causes when it sees no key press.

**A second sitting, 2026-08-31, on the TourBox.** The controller that the
previous session recorded as "unenumerated at USB level, likely a cable issue"
was exactly that: it now enumerates, and it is a TourBox Elite. The cable
diagnosis was right and is worth remembering, because a charge-only USB-C cable
presents as a controller that does not exist rather than as a cable that does
not work — which is why `roadie tourbox` names the cable before anything else
when it finds nothing.

What that sitting verified is in section 3. What it has **not** verified is the
control codes: the outstanding step is one pass of `roadie tourbox listen` with
every button pressed and every wheel turned, which is the only thing that can
confirm the transcribed bytes and settle the knob dispute. The hardware for it
is on the desk, so this is the cheapest open verification on the project.

**A third sitting, 2026-09-02, with nobody at the desk.** Nadir attached new
hardware and asked for everything that could be verified without him. Two of
the four open hardware gaps closed:

- **The DDC monitor arrived and the checksum seed is verified** — the RTK
  HG560T34 details are in section 3. Input switching was deliberately not
  tested: changing the input of a monitor at an unattended desk takes the
  screen away from whoever comes back to it, and that choice belongs to the
  person sitting there.
- **The VIA board arrived, spoke protocol 12, and exposed a gap this build
  then closed.** A Kiiboom Cybrix 16 (`5343:0080`) answered the handshake and
  was refused: it speaks VIA protocol 12 and the build implemented only 9.
  The four commands this build sends are byte-identical from 9 through 12 —
  what changed in between was the lighting commands (unused here) and the
  quantum keycode numbering (deliberately not in the name table, which names
  only the era-stable HID-standard codes) — so 12 was added to the accepted
  set, with the transitional 10 and 11 still refused by name. After that, the
  first VIA hardware verification anywhere: handshake (protocol 12, six
  layers), a full keymap read, and a write of F24 confirmed by read-back and
  then undone. Protocol 9 remains transcription no board has confirmed.
- **The TourBox still enumerates and its port still opens** — the listener
  ran and honestly reported nothing pressed, because nobody was there to
  press. The button pass remains one minute of Nadir's hands.
- **The Scarlett Solo did not appear at all.** No Focusrite vendor id
  (`0x1235`) anywhere in the IO registry. A generic "USB AUDIO DEVICE"
  (`2f6e:4e02`, two in, two out) is attached and is not how any Focusrite
  presents itself — it is most likely the new monitor's own audio. The
  2026-08-30 TourBox lesson repeats: a charge-only or faulty USB cable
  presents as hardware that does not exist, not as a cable that does not
  work. Sections 6.7 through 6.9 stay hardware-unverified until the Scarlett
  enumerates — worth checking its cable and power before anything else.

Still needing hardware this desk has not shown: an Elgato light (the mired
direction, by eye), and the Scarlett above. The remaining gaps that need only
hands, not purchases: the TourBox button pass, and the Stream Deck's visual
half (sighted eyes).

`docs/VERIFYING.md` remains the ordered pass for whatever hardware appears
next, and this sitting held its shape: every failure it met was either a
permission, another program holding the device, or a genuine finding.

## 6. Monitors: what was built, and why it is shaped this way

**Sections 6.1 through 6.5 are done and on this branch.** They are kept in
full rather than reduced to a changelog line, because what they carry is the
reasoning, and the reasoning is what the next person needs when they change
one of these decisions. What comes after them is 6.6.

The last handoff left "which platform first" open. It was the wrong question
to block on. Everything except a panel
actually answering can be built and proven with no hardware attached and no
macOS host in the room, so the platform choice does not gate the work — it only
decides which backend gets *verified* first, and that is a hardware question
Nadir settles at his desk, not a design question a session settles for him.

So: the trait, the Linux backend and a mock landed first, because those are
the parts a Linux container can prove. macOS went in behind the same trait —
and, as it turned out, could be type-checked here after all; see section 4.
Windows is cross-compiled. Verification starts wherever Nadir happens to be
sitting, and `docs/VERIFYING.md` step 10 is the ordered pass for it.

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

### 6.6 Key Lights, which are done

Taken next rather than the audio interfaces, because they are HTTP over the
local network and well documented, so a session with no hardware could build
and prove them. `roadie-keylight` is one crate rather than two: unlike
`roadie-ddc`, whose host half is three platform APIs carrying unsafe FFI, a Key
Light's host half is an HTTP client with no platform code at all, so a feature
gate says the same thing at a fraction of the ceremony — the way `roadie-core`
already gates its filesystem reads. The protocol keeps the wasm portability
claim with `net` and `discovery` off.

Three decisions in it are worth keeping:

**They live under `roadie light`, not a command of their own.** The question
someone asks is "what lights do I have", and an answer covering only the ones
on USB would be worse for being confidently incomplete. So a Litra on USB and a
Key Light on Wi-Fi are one list, one selector and one set of verbs.

**Colour temperature is the part to get right.** The light does not take
Kelvin. It takes mireds — reciprocal megakelvin — and they run *backwards*: a
larger number is a warmer light. Everything else on this desk speaks Kelvin, so
the conversion sits at the boundary, rounds to nearest rather than truncating
(which halves the worst-case round-trip drift, 48 K to 24 K), and is tested
against the endpoints Elgato publishes rather than trusted.

**Discovery is not a convenience.** A light's address comes from a DHCP lease
and changes on its own, so an address in a config file is one that will be
wrong one morning. Multicast is how the light stays findable without anyone
maintaining that — and it is also why a guest network or a VPN can hide a light
that is working perfectly.

### 6.7 Scarlett, the protocol half

`roadie-scarlett` is the wire format and the device tables, with no I/O — the
shape `roadie-ddc` landed in before `roadie-display` followed it. A Scarlett's
audio is standard USB audio class and needs nothing from us; what needs a
protocol is everything around it.

**The licensing position, because it will be asked about.** Focusrite publishes
no specification. The authoritative description is the Linux kernel's
`mixer_scarlett2.c` by Geoffrey D. Bennett, which is GPL-2.0 while this project
is MIT/Apache-2.0. What was taken is facts — opcodes, byte offsets, field
widths, and which model uses which table. A register offset is a fact about a
piece of hardware in the way a pinout is; it is not authored expression. No
code, comment, structure or naming was reproduced. This is the same footing
`roadie-ddc` stands on with `ddcutil`, which is also GPL. Nadir approved this
explicitly. The crate documentation states it in full, deliberately, so that an
auditor finds it in the source rather than in a commit message.

Three things in it are worth keeping:

**The start-up exception nearly ate the sequence check.** Fetching a reply is a
separate transfer from sending the request, so a stale reply is a real, intact
answer to an older question and only the echoed sequence number tells them
apart. Start-up is the exception: the request carrying sequence 1 is answered
with 0. Phrased as a rule about the number, that exception collides *exactly*
with the failure the check exists to catch — every session's second request
carries 1, and the leftover reply from its first carries 0. The first version
had that bug and its own test caught it. Tied to the two commands that only
ever run at start-up, it cannot happen.

**Phantom power is one bit in a shared byte.** Writing the byte outright
switches 48 V off on every other input pair, silently, while the panel shows
only the pair that was asked for. So a bit-sized setting is planned as a
read-modify-write, and `apply_bit` exists as its own function to be tested
without a device.

**The gate is as narrow as the monitor one.** Only switching phantom power
*on* is risky: off is how somebody makes the interface safe again, and a
confirmation in front of the safe direction is an obstacle in the wrong place.
The acknowledgement names the pair it was given for, so agreeing once cannot be
spent on every input.

### 6.8 The Scarlett host layer forks by platform, and Linux is the odd one

Found while planning that layer, before writing any of it, and it changes what
it is. **On Linux the protocol crate is not the way in and should not be used.**

The kernel's `snd-usb-audio` claims the interface, and `mixer_scarlett2.c` is
part of it, so the vendor control endpoint already has an owner. Reaching it
from userspace over raw USB would mean detaching that driver — which stops the
audio, on an audio interface. Worse, the kernel is already doing the work: it
publishes every one of these settings as an ordinary ALSA mixer control, with
names built per model, such as `Line In 1 Phantom Power Switch` and
`Line In 1-2 Phantom Power Switch` where one phantom switch covers a pair.

So the honest shape is:

- **Linux** — read and write ALSA mixer controls. No USB, no protocol crate,
  nothing to reverse. The work is the name mapping per model, which is pure and
  testable, plus an ALSA binding.
- **macOS and Windows** — no kernel driver claims the control interface, so raw
  USB control transfers, which is exactly what `roadie-scarlett` is for.

That is not a loss for the protocol crate: it earns its keep on two of the three
platforms, and it is the only option there. But it does mean "write a USB
backend" is the wrong description of the next step, and that the Linux path is
much smaller and much safer than it looked.

This was **not verified against hardware or against a running kernel** — it is
read from the driver source and from how USB interface claiming works. Worth
confirming with `amixer -c` on a machine with a Scarlett attached before
building on it, which is a one-command check.

### 6.9 The Linux half of Scarlett is names, and they are built

`roadie-scarlett::alsa` turns a model plus an input number into the ALSA
control name the kernel publishes. That is the whole Linux mechanism: no USB,
no packets, no protocol crate — a host reads and writes names.

They look regular and are not, which is why they are generated from per-model
facts rather than formatted from a guess. The number in a name is the input as
a person counts it, from one, but *which* input a control belongs to is a
property of the model. A **Scarlett Solo 4th Gen has one phantom switch and it
is on input two.** A 2i2 3rd Gen has one that covers both inputs, so it is
named for a range — `Line In 1-2` — rather than for an input. An 18i20 3rd Gen
groups four inputs per switch. And the 4th generation turned "air" from a
switch into a choice, which changes the last word of the name.

Getting one wrong fails silently: the name matches no control, and the setting
simply appears not to exist. So there are tests pinning each of those four
shapes by name, plus two that hold the whole table together — every model names
as many controls as it claims to have, and no model names two controls the same
thing. The mutation sweep ran fifteen and killed fifteen.

Still unverified against hardware, and one `amixer -c` with a Scarlett attached
would settle both this and the fork above.

### 6.10 After that — this is the part still to do

The Scarlett host layers themselves: an ALSA binding on Linux over these names,
and USB control transfers on macOS and Windows over the packet layer. Then the
CLI and MCP surfaces. Then headsets, MIDI pads, RGB on other brands, and other
vendors' mice and keyboards.

## 7. What a session can do with nobody at the desk

Nadir travels, and a session that stalls waiting for a monitor to be plugged in
wastes the time. The split is sharper than it looks, so it is written down.

Buildable, testable and gate-green with no hardware and no computer in front of
anyone:

- Everything in 6.1 through 6.5, all of which is now done. The mock backend is
  what made it true: the CLI, the MCP tools and the survey were all driven
  against a scripted display.
- The macOS FFI, which turned out to be type-checkable here — see section 4.
  CI's two macOS jobs still compile it for real on a PR, and neither is a
  substitute for running it.
- The Windows backend, cross-compiled locally against `x86_64-pc-windows-gnu`
  and by the `clippy (windows)` job.
- A mutation sweep over whatever lands. The fork's record is that every gap
  found this way was in code that had prose about it and no test.
- The `docs/VERIFYING.md` steps for monitors, written ahead of the hardware so
  the first sitting is one ordered pass rather than an exploration.

Needed a panel answering, and the 2026-09-02 sitting settled most of it:

- The reply checksum seed: verified against the RTK HG560T34 — replies come
  back, parse, and pass the checksum.
- The timing floors held on one real panel with the default spacing; one
  panel is a data point, not a distribution.
- Which input-source values a given monitor accepts. Above `0x12` is vendor
  territory and USB-C has no standard number at all — and switching inputs
  on an unattended desk was deliberately not attempted.
- Whether each screen on the desk speaks DDC: answered for this desk. The LG
  TV does not (CEC instead), the HG560T34 does.

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
