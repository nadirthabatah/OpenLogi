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
openlogi streamdeck           # list attached Elgato Stream Decks
openlogi streamdeck verify    # check the Stream Deck driver against your hardware
```

## Stream Decks

`openlogi streamdeck` lists every Stream Deck collection the OS reports;
`brightness`, `reset` and `watch` drive an attached device.

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

Asset synchronization probes `assets.openlogi.org`, the versioned Cloudflare
Pages release alias, and the pinned jsDelivr npm release concurrently. The first
mirror with a valid catalog supplies every file for that synchronization run.
Set `OPENLOGI_ASSETS` or pass `openlogi assets sync --base <URL>` to use one
uniform asset origin instead of automatic mirror selection.
