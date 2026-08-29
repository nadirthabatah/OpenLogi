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

The tools exposed are `list_devices`, `read_dpi`, `set_dpi`, and
`reload_config`. Start from `list_devices`: its output carries, for every
device, the route object the other tools expect back verbatim, so routes are
never assembled by hand. Everything runs through the same agent IPC the GUI and
the rest of this CLI use — the server holds no device handles of its own — and
a tool call made while no agent is running reports that instead of failing, so
the assistant can say what to start.

Asset synchronization probes `assets.openlogi.org`, the versioned Cloudflare
Pages release alias, and the pinned jsDelivr npm release concurrently. The first
mirror with a valid catalog supplies every file for that synchronization run.
Set `OPENLOGI_ASSETS` or pass `openlogi assets sync --base <URL>` to use one
uniform asset origin instead of automatic mirror selection.
