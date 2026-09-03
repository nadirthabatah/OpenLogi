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
step 7 exercises the code most likely to be wrong. Step 10, the monitors, is
one command and settles more per second spent than anything else here.

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
./target/release/roadie doctor
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
./target/release/roadie list
```

**Expect:** your devices, each with slot, name, kind, online state and battery.
The first line says whether the inventory came from the running agent or from
direct enumeration — both are fine, but note which, because a permission
problem on macOS shows up as a *different* answer between them.

**If empty:** on macOS the process needs Input Monitoring; on Linux the udev
rules must be installed and your session must have been restarted since.

## 1. Reading device state

```sh
./target/release/roadie diag features
./target/release/roadie camera
```

**Expect:** a feature dump for the active device, and each camera control's
min, max, default and current value.

**Note:** `roadie camera` lists Logitech cameras. The MCP `list_cameras` tool
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
  | ./target/release/roadie mcp
```

**Expect:** two JSON lines. The second contains your real devices, and each one
carries a `route` object.

Copy a `route` verbatim from that output into the next call — that is exactly
what an assistant does:

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_dpi","arguments":{"route":PASTE_HERE}}}' \
  | ./target/release/roadie mcp
```

**Expect:** the mouse's current DPI and the list of values its sensor supports.

**This is the most important single check in this document.** It proves the
whole chain — client, server, IPC, agent, HID, device — on your actual
hardware.

## 3. Writing to a device (reversible)

Note the DPI you just read, then set it and read it back:

```sh
./target/release/roadie diag dpi --target 1600
```

**Expect:** a read, a write, a read-back showing 1600, and a restore to the
original. The pointer speed should visibly change and change back.

## 4. Identifying a button by pressing it

```sh
printf '%s\n%s\n' \
  '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"watch_input","arguments":{"seconds":10}}}' \
  | ./target/release/roadie mcp
```

Press a mouse button while it runs.

**Expect:** the button named, with `pressed` and `released` lines.

**If it reports nothing pressed:** the agent's input hook is not running, which
on macOS means Accessibility has not been granted to the *agent* (not to this
CLI). This is the check most likely to fail first on a fresh macOS install, and
the failure is a permission, not a bug.

## 5. Profiles, round trip

```sh
./target/release/roadie profile export ~/my-setup.toml
./target/release/roadie profile inspect ~/my-setup.toml
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
./target/release/roadie profile import /tmp/untrusted.toml
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
./target/release/roadie devices
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
./target/release/roadie streamdeck verify
```

**Expect:** every collection listed, the chosen one marked, the screens dimming
and restoring, then a prompt to press the top-left key and a `CORRECT` line.

A `MISMATCH` line is a genuine finding and worth reporting — it means the key
ordering in the catalogue is wrong for that model. So is "no key press seen",
which means the collection choice is wrong. Both are the questions this command
exists to answer.

Then a whole layout, which is how anyone will actually use it:

```sh
./target/release/roadie streamdeck example desk-test
./target/release/roadie streamdeck apply desk-test
./target/release/roadie streamdeck layouts
```

**Expect:** key 0 reading `MUTE MIC` on a dark red ground, key 1 reading `REC`
in orange, and `desk-test` in the layout list. The text should fill each key
without spilling over its edges — a label that is clipped, upside down, or
mirrored is a rendering defect worth reporting with a photograph.

## 8. QMK and VIA macro pads

Only if you have a board running QMK with VIA enabled.

```sh
./target/release/roadie via
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
this build implements two, 9 and 12. Report the number it names; it is
exactly what is needed to add support.

```sh
./target/release/roadie via keymap 0
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
./target/release/roadie via get 0 <row> <column>
./target/release/roadie via set 0 <row> <column> F13
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
./target/release/roadie profile export ~/my-setup
```

**Expect:** a folder, and a report naming `config.toml` and every saved layout.

Copy that whole folder to the second machine, install there, and:

```sh
./target/release/roadie profile import ~/my-setup
./target/release/roadie streamdeck layouts
./target/release/roadie streamdeck apply desk-test
```

**Expect:** the import naming the layouts it restored, the layout list showing
them, and the deck looking the way it did on the first machine — icons
included.

**If a key that had a picture is now blank:** the icon did not travel. That is
a defect and worth reporting with the layout file, because a bundle that looks
complete and applies to blank keys is exactly the failure this is checked
against.

## 10. Monitors

The newest code here, and the least proven: no monitor has ever answered any
of it. The protocol comes from the DDC/CI 1.1 and MCCS 2.2a specifications and
from `ddcutil`'s reading of them, and the host calls from the documented APIs
plus what MonitorControl does with them. That is a sound footing and it is not
verification.

Your laptop's built-in screen will not appear and is not meant to. It has no
DDC channel at all; its brightness is a system setting rather than a monitor
one. This needs an external monitor on a cable.

```sh
./target/release/roadie display list
```

**Expect:** each external monitor named from its own EDID — "LG ULTRAFINE"
rather than a port number — and the words "answers and can be controlled".

**If a monitor is named but does not answer:** the name came from the EDID and
the control channel is separate, so this is the useful half working. On Linux
the reason will say the `i2c` group; add yourself to it, log out and back in.
Otherwise the commonest cause by far is DDC/CI switched off in the monitor's
own menu, where many ship it off. Turn it on and try again.

**If nothing is listed at all on an Apple silicon Mac:** that is a finding
worth reporting. The registry walk looks for `DCPAVServiceProxy` entries whose
`Location` is `External`, and a Mac that names them differently is exactly what
this step exists to discover.

**On an Intel Mac it will say so plainly.** That path uses a different
mechanism and is deliberately not implemented rather than guessed at.

### Reading, which changes nothing

```sh
./target/release/roadie display get brightness
./target/release/roadie display capabilities
```

**Expect:** a brightness as a percentage *and* as the monitor's own numbers,
and a capability list naming the model, the settings it has and the inputs it
can switch between.

**Three things here are worth more than the rest of this document**, because
they are the parts that could not be checked without a panel:

1. **A reading that comes back at all** confirms the reply checksum seed.
   Requests and replies are checksummed differently, and a host using the wrong
   one fails every reply identically — which looks exactly like a monitor that
   is not answering. If reads work, that seed is right.
2. **Readings that are consistently wrong by one** would mean the timing
   floors are too short: DDC has no sequence numbers, so a reply read too early
   is the answer to the previous question. Ask for brightness twice and then
   contrast; if the contrast reading equals the brightness one, say so. The
   echo check should catch this and report it rather than believing it, so this
   confirms the check works.
3. **The capability string** is what the input names in the next step come
   from. If it mentions settings this build does not name, that list is the
   feature request.

### One write, and how to undo it

```sh
./target/release/roadie display get brightness
./target/release/roadie display brightness 40
```

**Expect:** "set to 40, and reads back the same", or a message saying the
monitor clamped it lower. Either is a pass; the second is a real property of
some panels and worth reporting with the model name.

Put it back with `roadie display brightness <the percentage you started with>`.

### The input switch, which is the one to be careful with

Switching a monitor to an input with nothing plugged into it leaves you with a
dark screen and the bezel buttons as the way back. Do this one only with a
second machine on the other input, or not at all.

```sh
./target/release/roadie display capabilities
./target/release/roadie display set input hdmi2
```

**Expect:** the monitor to switch. Above value `0x12` is vendor territory and
USB-C in particular has no standard number, so if your monitor's USB-C input
is what you want, the capability list is where its number will be — and which
number works on which panel is exactly the thing nobody has written down.

### The refusal, which should never actually fire

```sh
./target/release/roadie display set power off
```

**Expect:** it refuses, tells you the monitor may stop answering the computer
entirely, and says the way back would be the power button on the monitor
itself. **Do not pass `--yes`.** The point of running this is to confirm the
refusal happens, and a monitor that stops answering DDC is a monitor only its
bezel can recover.

## 11. Elgato Key Lights

Only if you have one. A Key Light Neo can be reached two ways — plugged in
over a USB data cable, or over Wi-Fi — and every other light in the family is
Wi-Fi only. The USB path is the simpler one to verify and the one that needs
nothing set up: plug the Neo into a data cable (not a charge-only one) and it
answers, no app and no network involved. The Wi-Fi path adds what can go
wrong on a network: the light has to be powered on, on the same network as
this computer, and the network has to carry multicast — which some guest
networks and some VPNs deliberately do not.

```sh
./target/release/roadie light list
```

**Expect:** each light named — a Neo on USB says "on USB" so it is not
confused with the same light on Wi-Fi — with whether it is on, its brightness
as a percentage, and its colour temperature in kelvin.

**If nothing is found and the light is on:** for a Neo on USB, check the cable
is a data cable and the port carries data — the same trap the TourBox fell
into, where a charge-only cable or a power-only socket makes the light invisible
rather than merely uncontrolled. For a Wi-Fi light, the most likely cause is
multicast rather than the light: try again on the same Wi-Fi with any VPN off.
A light on a different subnet from this computer will not be found and cannot
be, which is a property of multicast rather than a defect here.

**On a Neo over USB, brightness above what its power source allows is
refused, not clamped.** Plugged into a computer's USB port it caps around 40
percent, and asking for more answers with the ceiling named rather than a bare
error — that is the firmware's own behaviour, and the sentence tells you the
number to ask for instead. A mains supply lifts the ceiling.

**If a light is found and then says it did not answer:** that is the useful
half working. Discovery and reachability are separate questions and a light
that went to sleep between the two is the ordinary case; the address it prints
is what to ping.

### The three writes, all reversible

Note what the light is at now, from the list above, so you can put it back.

```sh
./target/release/roadie light brightness --percent 40
./target/release/roadie light temperature --kelvin 4000
./target/release/roadie light off
./target/release/roadie light on
```

**Expect:** each command to report the light's state *after* the change, read
back from the light itself rather than echoed from the request.

**Two things worth checking by eye or by hand**, because they are the ones no
test here could settle:

1. **That 4000 kelvin looks like 4000 kelvin.** The light does not take kelvin;
   it takes mireds, which run backwards — a larger number is a warmer light.
   The conversion is tested against its own arithmetic, and what it has never
   been tested against is a lamp. If warm and cold come out swapped, that is
   the finding, and it is a one-line fix.
2. **That brightness 3 is the dimmest it goes and not off.** The floor is the
   light's own, not a choice here, and off is a separate setting. A light that
   goes dark at 3 percent would mean the two have been conflated somewhere.

Put it back with the values you noted.

### Two lights, which is where the naming matters

If you have a key and a fill light:

```sh
./target/release/roadie light list
./target/release/roadie light brightness --device fill --percent 20
```

**Expect:** the one you named to change, and the other to be untouched.
Without `--device`, expect a refusal that lists both names rather than a guess.

**If both are called the same thing** — which happens, since Elgato names them
after the model — the refusal will list the same name twice and the address is
how you tell them apart. That is worth reporting: it means the name someone
sees is not the one the app set, and it is a genuine gap.

## 12. TourBox controllers

Only if you have one. This is the first device here reached over a serial port
rather than HID or the network, and the first whose control codes were
transcribed from other people's reverse engineering rather than from a
published specification. That makes this step unusually valuable: it is the
only thing that can confirm them.

```sh
./target/release/roadie tourbox list
```

**Expect:** the model, the serial port it is on, its serial number, and a count
of buttons and wheels. Exactly one entry per controller.

**If it says two are attached and you have one:** that is a defect worth
reporting. macOS names every serial device twice and this build drops the
duplicate; two entries means the rule missed a case.

**If nothing is found:** check the cable before anything else. A charge-only
USB-C cable carries power but no data, so the controller lights up and never
reaches the computer at all — indistinguishable from a dead controller until
you swap it. This was the actual cause the first time this project met one. A
TourBox Neo will also not be found even when it is working, because it connects
through a general-purpose serial adapter whose USB identity is shared with many
unrelated devices; name its port with `--port` instead.

### Pressing every control, which is the whole point of this step

```sh
./target/release/roadie tourbox listen
```

Then press every control once, pausing about a second between each, and listen
to what each one is called. A useful order, because it groups the ones most
likely to be confused with each other:

1. The three wheels, turned both ways: the **knob**, the **scroll wheel**, the
   flat **dial**.
2. The same three pressed inward. Pressing a wheel is a *different control*
   from turning it and carries its own code, which is the single most likely
   thing to have been transcribed wrong.
3. The four-way pad: **up, down, left, right**.
4. **Tall, short, side, top**.
5. **C1**, **C2**, and the centre **tour** button.

**Expect:** each press named, and each release named separately. Fourteen
buttons, so twenty-eight button events, plus the wheels.

A turn reads as a run of "turned clockwise" lines. The published drivers
document one more byte after the last detent, which this build would read
aloud as "stopped turning clockwise" — and the Elite on this project's desk
sent none in a 450-event pass on 2026-09-02. So on an Elite, no stop line is
the expected outcome; a model that does send one will be named, and that
would be worth reporting as a model difference.

**The one to watch is the knob press.** Two published sources disagree about
the byte it sends, and this build implements one of them and refuses the other
by name rather than guessing between them. So there are two good outcomes and
neither is a failure:

- It says **knob press** and **knob released**. The transcription was right.
- It says **something this build does not recognise**, naming a byte. The other
  source was right, and that sentence is the fix: the byte it names is the
  correct code, and `the_disputed_knob_byte_is_refused_rather_than_guessed_at`
  in `roadie-tourbox` is the test to change.

**If a control is named as a different control**, that is a straightforward
transcription error and the report should say which physical control you
pressed and what it was called.

**If nothing arrives at all but the port opened**, check the build before
blaming anything else: an Elite says nothing until it is sent the unlock
command, and three sessions of listeners on this project were silent for
exactly that reason before the handshake was added on 2026-09-02. On a build
that sends the unlock, another program holding the controller's input is the
remaining cause — TourBox Console is the likely one. That is the same failure
the Stream Deck had on this desk, where Logitech's device manager held the
deck's input in seize mode and every key report went to it alone with no
error anywhere.

## 13. Focusrite audio interfaces

Only if you have a Scarlett or a Vocaster. The audio itself needs nothing from
this: it is standard USB audio and the operating system already handles it.
What `roadie audio` reaches is everything around it, over a separate control
channel, and recording is never interrupted by any of it.

```sh
./target/release/roadie audio status
```

**Expect:** the model, its firmware version, and one line per input saying the
preamp gain, whether it is muted, and whether 48 volt phantom power is on.

**If it says the interface is in mass storage mode:** that is how it leaves
the factory and it is not a fault. It presents a small disk of registration
files. Everything here works with it on, which was confirmed on a Vocaster
Two, so nothing needs doing about it.

**If nothing is found:** check the cable carries data rather than only power,
and check the socket does too. That trap has now hidden three different
devices on this project's desk.

**If it says it could not claim the control interface:** something else is
holding it. The audio side is unaffected — this is specifically the control
channel, and the message says so.

### The reversible writes

```sh
./target/release/roadie audio gain 1 25
./target/release/roadie audio mute 1 on
./target/release/roadie audio mute 1 off
```

**Expect:** each command to report what the interface reads back afterwards
rather than echoing the request, and the gain change to name the exact command
that undoes it.

**Worth checking by ear**, because it is the one thing no test here can
settle: with a microphone plugged in and monitoring, a gain change should be
*audible*. A value that stores and reads back correctly proves the address is
consistent; only hearing it proves the address is the right one.

### The one write that asks first

```sh
./target/release/roadie audio phantom 1 on
```

**Expect:** it refuses, and reads out what 48 volt phantom power can damage —
a ribbon or older passive microphone — then names the exact command with
`--yes` that goes ahead. Nothing changes until you type that.

That refusal is the whole safety design and it is deliberately narrow.
Switching phantom power *off* asks nothing, because that is how you make the
interface safe again. And an assistant driving the MCP server can switch it
off but **cannot** switch it on at all: it gets the sentence to read to you
and the command for you to type, because what 48 volts can damage is at the
end of a cable no software can see.

**Before typing the `--yes` form, unplug anything you are not sure about.**

## What a failure here means

Failures in steps 0b, 1 and 4 are almost always permissions, not defects —
macOS in particular gates HID reads and input monitoring separately and per
binary. Step 0 exists to catch those before you reach them.

Failures in steps 2, 3 and 5 are defects worth reporting, with the command and
its full output. Begin any report with `roadie doctor` — its first line names
the build and the platform, which are the first two things anyone reading a
report has to ask for. `roadie doctor --json` carries the same two fields for
anything that parses output rather than reading it.

Steps 6, 7, 8, 10 and 11 are explicitly unproven. They are the least verified code
in the fork — written against published protocols and thoroughly unit-tested
against scripted devices, but never run against a physical one — and their
output is written to be pasted straight into an issue.

Step 10 is the least proven of those, and the one where a single reading
settles the most: a monitor that answers `roadie display get brightness` at all
has confirmed the reply checksum seed, which nothing without hardware could.
Step 11 has one question of the same kind — whether warm and cold come out the
way round they should — and it is answered by looking at the lamp.

If you have limited time, do steps 0, 6 and 7 in that order. Step 0 clears the
permissions that mask everything else, step 6 proves that the hub sees your
desk, and step 7 exercises the code most likely to be wrong. Step 10 is the
next one after those, and the cheapest: one command, and it either answers or
it does not.
