# Usage (CLI)

The `openlogi` command-line tool. For install and configuration, see the
[README](../README.md).

```sh
openlogi list                 # paired devices: slot, codename, kind, online, battery
openlogi assets sync          # pre-fetch device renders from the fastest available mirror
openlogi diag features        # dump every HID++ feature the active device reports
openlogi diag controls        # dump reprogrammable controls and capability flags
openlogi diag dpi             # read → write → read-back → restore DPI (smoke test)
openlogi diag smartshift      # toggle SmartShift and restore (smoke test)
openlogi diag lighting ff0000 # solid colour for a wired RGB keyboard (any RRGGBB hex)
openlogi mcp                  # serve the agent to an AI assistant over MCP (see below)
```

Running `openlogi` with no subcommand defaults to `list`. Set
`OPENLOGI_LOG=debug` for verbose tracing in the CLI, GUI, or agent.

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

The camera tools reach the device directly rather than through the agent, the
same way `openlogi camera` does — UVC controls are a host-exposed class
standard, not agent-owned state, so on macOS the grant that matters is the
CLI's own. Enumeration there is deliberately **not** filtered by vendor: the
same UVC registers answer on an Elgato, an Obsbot or a built-in camera, so
restricting the list to one manufacturer would hide devices that are in fact
controllable.

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
