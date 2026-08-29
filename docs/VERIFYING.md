# Verifying on real hardware

Everything in this fork is unit-tested and checked by CI on macOS, Windows and
Linux. None of it has been run against physical peripherals, because the
environment it was written in has no USB at all. That is a real gap, and this
document is how to close it: an ordered list of commands, each with the answer
that means it worked.

Nothing here is destructive. Every step either reads state or makes a change
you can reverse, and every step that writes to a device says so and gives you
the command to undo it.

If you have limited time, do steps 0, 6 and 7. Step 0 clears the permission
problems that mask everything else, step 6 proves the hub sees your desk, and
step 7 exercises the code most likely to be wrong.

## What is checked without a device, and what is not

Two decoders here meet bytes a device sent: the Stream Deck key-report
decoder and the VIA response parser. Neither has ever met a device, so both
are checked against the assumption that a device sends *anything at all* —
220,000 generated reports between them, across every catalogued model and
every command, requiring a value or an error and never a crash.

That is not a substitute for hardware and is not offered as one. It rules
out one specific way the drivers could fail on your desk — a panic or a
misread when a report arrives truncated, out of order, or as noise from a
device being unplugged mid-transfer — which is the failure this project can
least afford, because it kills the process that was watching your keys.

The generators are deterministic, so a failure names bytes that reproduce
it rather than a run that happened once.

What still needs a device is everything about *meaning*: that the key you
press is the key we report, that the image we send lands the right way up,
and that the collection we chose is the one carrying the traffic. That is
what the steps below are for.

## 0. Build, then ask the program what is wrong

```sh
cargo build --release
./target/release/openlogi doctor
```

**Expect:** every check `OK` or `NOTE`, and the line "Nothing needs fixing."

**Do this first and do not skip it.** Almost every failure further down this
document is a permission rather than a defect, and every one of those shows up
here as a `FIX` line with the steps that resolve it. Working through those
steps before anything else will save you the whole rest of the list.

A `NOTE` about the background agent is expected and is not a problem — nothing
in this document needs the agent except step 4.

If `doctor` reports a problem it cannot explain, that itself is worth
reporting: its whole purpose is to turn "nothing found" into something you can
act on, and a case it handles badly is a defect in the most important command
here.

## 0b. Confirm the agent path

```sh
./target/release/openlogi list
```

**Expect:** your devices, each with slot, name, kind, online state and battery.
The first line says whether the inventory came from the running agent or from
direct enumeration — both are fine, but note which, because a permission
problem on macOS shows up as a *different* answer between them.

**If empty:** on macOS the process needs Input Monitoring; on Linux the udev
rules must be installed and your session must have been restarted since.

## 1. Reading device state

```sh
./target/release/openlogi diag features
./target/release/openlogi camera
```

**Expect:** a feature dump for the active device, and each camera control's
min, max, default and current value.

**Note:** `openlogi camera` lists Logitech cameras. The MCP `list_cameras` tool
deliberately does not filter by vendor — if you have an Obsbot, an Elgato or a
built-in camera, that tool should list it and this command should not. A
difference between the two is the expected result, not a bug.

## 2. The MCP server, by hand

You do not need an AI client to test this. The server speaks newline-delimited
JSON on stdin and stdout, so a shell pipeline is a complete test:

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_devices","arguments":{}}}' \
  | ./target/release/openlogi mcp
```

**Expect:** two JSON lines. The second contains your real devices, and each one
carries a `route` object.

Copy a `route` verbatim from that output into the next call — that is exactly
what an assistant does:

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_dpi","arguments":{"route":PASTE_HERE}}}' \
  | ./target/release/openlogi mcp
```

**Expect:** the mouse's current DPI and the list of values its sensor supports.

**This is the most important single check in this document.** It proves the
whole chain — client, server, IPC, agent, HID, device — on your actual
hardware.

## 3. Writing to a device (reversible)

Note the DPI you just read, then set it and read it back:

```sh
./target/release/openlogi diag dpi --target 1600
```

**Expect:** a read, a write, a read-back showing 1600, and a restore to the
original. The pointer speed should visibly change and change back.

## 4. Identifying a button by pressing it

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"watch_input","arguments":{"seconds":10}}}' \
  | ./target/release/openlogi mcp
```

Press a mouse button while it runs.

**Expect:** the button named, with `pressed` and `released` lines.

**If it reports nothing pressed:** the agent's input hook is not running, which
on macOS means Accessibility has not been granted to the *agent* (not to this
CLI). This is the check most likely to fail first on a fresh macOS install, and
the failure is a permission, not a bug.

## 5. Profiles, round trip

```sh
./target/release/openlogi profile export ~/my-setup.toml
./target/release/openlogi profile inspect ~/my-setup.toml
```

**Expect:** the file is written, and inspecting it reports your schema version,
your device count, and — for a profile you exported yourself — either no
actions that run a program, or the ones you configured, listed by location.

To prove the safety guard, put something dangerous in a copy and try to import
it:

```sh
cp ~/my-setup.toml /tmp/untrusted.toml
cat >> /tmp/untrusted.toml <<'EOF'

[keyboard.bindings]
F13 = { RunShellCommand = "echo this should never run" }
EOF
./target/release/openlogi profile import /tmp/untrusted.toml
echo "exit status: $?"
```

**Expect:** a refusal naming `keyboard.bindings.f13` and the command text, the
words "Nothing has been imported", and exit status 3. Your configuration is
untouched.

This step deliberately uses the single-file form, because the audit is what it
is checking. Carrying a whole setup between machines — configuration *and*
layouts — is step 9, and that is the one that matters most to you.

## 6. Everything plugged in

```sh
./target/release/openlogi devices
```

**Expect:** every peripheral on your desk, in one list, sorted into
"configurable now", "probably configurable", "wireless receivers" and
"detected, not configurable by this build". The closing line gives a total.

**This is the check with the widest reach and it needs your eyes, not the
program's.** Count what is physically plugged in and count what the list says.
Anything attached that is missing entirely is the most serious kind of bug this
project can have — worse than a device reported as unsupported, because an
unsupported device is at least visible. If something is missing, report it with
the output and what the device is.

A device under "probably configurable" is a VIA candidate that has not been
opened yet; step 8 confirms it.

Devices under "detected, not configurable" are working as intended. That list
is the roadmap, so the vendor and product ids printed there are worth sending
along whatever else you report.

## 7. Stream Deck

```sh
./target/release/openlogi streamdeck verify
```

**Expect:** every collection listed, the chosen one marked, the screens dimming
and restoring, then a prompt to press the top-left key and a `CORRECT` line.

A `MISMATCH` line is a genuine finding and worth reporting — it means the key
ordering in the catalogue is wrong for that model. So is "no key press seen",
which means the collection choice is wrong. Both are the questions this command
exists to answer.

Then a whole layout, which is how anyone will actually use it:

```sh
./target/release/openlogi streamdeck example desk-test
./target/release/openlogi streamdeck apply desk-test
./target/release/openlogi streamdeck layouts
```

**Expect:** key 0 reading `MUTE MIC` on a dark red ground, key 1 reading `REC`
in orange, and `desk-test` in the layout list. The text should fill each key
without spilling over its edges — a label that is clipped, upside down, or
mirrored is a rendering defect worth reporting with a photograph.

## 8. QMK and VIA macro pads

Only if you have a board running QMK with VIA enabled.

```sh
./target/release/openlogi via
```

**Expect:** the board named, with the VIA protocol revision it speaks and how
many keymap layers it holds.

**If it says "did not answer as a VIA device":** the firmware may not have VIA
enabled, or the HID collection this driver picks is the wrong one for that
board. Either way that line is the finding — report it with the board's name.
A board that goes completely silent is given two seconds and then reported the
same way, rather than left waiting.

**If it refuses the protocol revision:** that is the driver being careful
rather than a fault. VIA payload layouts have changed between revisions and
this build implements one. Report the number it names; it is exactly what is
needed to add support.

```sh
./target/release/openlogi via keymap 0
```

**Expect:** the keys of layer 0, by name — `A`, `F13`, `LEFT_CTRL` — with their
matrix row and column. The count at the end says how many positions were empty
or passed through.

**If it reports nothing assigned:** the matrix is probably larger than the
default read. Try `--rows 10 --columns 20`.

### The one write, and how to undo it

This is the only step in this document that changes something on a keyboard.
Pick a key you are willing to lose for a minute — a spare macro pad key, not
your only Enter.

```sh
./target/release/openlogi via get 0 <row> <column>
./target/release/openlogi via set 0 <row> <column> F13
```

**Expect:** the change reported as `WAS -> F13`, the words "Confirmed by
reading the position back", and then the exact command to undo it. Press the
key: it should now send F13.

Run that undo command and press the key again to confirm it is back.

**If the write reports a mismatch:** the keyboard accepted the command and
stored something else. That is a real defect and the most important one this
step can find — report it with both values from the message.

## 9. Carrying the whole setup to another machine

The promise this project is built around, and the only step that needs two
computers.

```sh
./target/release/openlogi profile export ~/my-setup
```

**Expect:** a folder, and a report naming `config.toml` and every saved layout.

Copy that whole folder to the second machine, install there, and:

```sh
./target/release/openlogi profile import ~/my-setup
./target/release/openlogi streamdeck layouts
./target/release/openlogi streamdeck apply desk-test
```

**Expect:** the import naming the layouts it restored, the layout list showing
them, and the deck looking the way it did on the first machine — icons
included.

**If a key that had a picture is now blank:** the icon did not travel. That is
a defect and worth reporting with the layout file, because a bundle that looks
complete and applies to blank keys is exactly the failure this is checked
against.

## What a failure here means

Failures in steps 0b, 1 and 4 are almost always permissions, not defects —
macOS in particular gates HID reads and input monitoring separately and per
binary. Step 0 exists to catch those before you reach them.

Failures in steps 2, 3 and 5 are defects worth reporting, with the command and
its full output.

Steps 6, 7 and 8 are explicitly unproven. They are the least verified code in
the fork — written against published protocols and thoroughly unit-tested
against scripted devices, but never run against a physical one — and their
output is written to be pasted straight into an issue.

If you have limited time, do steps 0, 6 and 7 in that order. Step 0 clears the
permissions that mask everything else, step 6 proves that the hub sees your
desk, and step 7 exercises the code most likely to be wrong.
