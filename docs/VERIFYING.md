# Verifying on real hardware

Everything in this fork is unit-tested and checked by CI on macOS, Windows and
Linux. None of it has been run against physical peripherals, because the
environment it was written in has no USB at all. That is a real gap, and this
document is how to close it: an ordered list of commands, each with the answer
that means it worked.

Nothing here is destructive. Every step either reads state or makes a change
you can reverse, and the two that write to a device say so.

## 0. Build and confirm the agent is reachable

```sh
cargo build --release
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

**The real test is the one that matters to you:** export on one machine, carry
the file to another, import it there, and check your devices behave the same.
On a machine that has never run this app the import will say there was nothing
to back up — that is correct.

## 6. Stream Deck (arrives with the Stream Deck branch)

```sh
./target/release/openlogi streamdeck verify
```

**Expect:** every collection listed, the chosen one marked, the screens dimming
and restoring, then a prompt to press the top-left key and a `CORRECT` line.

A `MISMATCH` line is a genuine finding and worth reporting — it means the key
ordering in the catalogue is wrong for that model. So is "no key press seen",
which means the collection choice is wrong. Both are the questions this command
exists to answer.

## What a failure here means

Failures in steps 0, 1 and 4 are almost always permissions, not defects — macOS
in particular gates HID reads and input monitoring separately and per binary.

Failures in steps 2, 3 and 5 are defects worth reporting, with the command and
its full output.

Step 6 is explicitly unproven: it is the least verified code in the fork, and
its `verify` output is written to be pasted straight into an issue.
