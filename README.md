# Vitals

A native macOS menu bar sensor monitor in the spirit of the GNOME
[Vitals](https://github.com/corecoding/Vitals) extension. Swift and AppKit
only, no third-party dependencies.

## What it shows

Nine categories, each a row in the dropdown with its live aggregate value and
a chevron into its full sensor list:

| Category | Source |
|---|---|
| Temperature | SMC thermal diodes (`T*` keys) |
| Voltage | SMC voltage rails (`V*` keys) |
| Fan | SMC `F<N>Ac` actual RPM |
| Memory | `host_statistics64` + `hw.memsize` |
| Processor | `host_processor_info` per-core load |
| System | `getloadavg` |
| Network | `getifaddrs` byte counters, differenced |
| Storage | `volumeAvailableCapacityForImportantUsage` |
| Battery | `IOPowerSources` + SMC `B0AV`/`B0AC` |

Every category degrades gracefully: if it finds no usable sensor on this
hardware it shows **No data** instead of an invented number.

## Choosing what a menu bar slot shows

Click the slot, hover a category, and pick **Average** or any individual
sensor. The choice is stored per category and the category is added to the
menu bar automatically, so you never have to visit Settings to see it.

## Build

Requires Command Line Tools (no Xcode project involved):

```sh
make build     # swift build -c release
make app       # assemble, ad-hoc sign
make run       # open the app
make install   # copy to /Applications (refuses to overwrite)
make verify    # self-tests plus a sensor dump
```

The app is an `LSUIElement` accessory: no Dock icon, no root, no privileges
beyond reading IOKit.

## Design notes

- **The SMC wire format is 80 bytes and its layout must match the driver,**
  so every exchange happens in `Sources/CSMC/CSMC.c`, where the struct layout
  is guaranteed. Swift only sees decoded values. A Swift-side struct compiles
  to 76 bytes and the driver rejects it with `kIOReturnBadArgument`.
- **`sp78` is signed 7.8 fixed point** and `fpe2` is unsigned 14.2, so the
  two need different sign handling. `SMCDecoder` covers `flt`, `ioft`, `sp78`,
  `sp87`, `sp5a`, `sp96`, `sp4b`, `fpe2`, `fp1f`, `ui8`…`si32`, and the SMC
  pads type strings to four characters with spaces.
- The key list and each key's type are read once at startup; a refresh only
  re-reads values, which keeps a full pass around 30 ms.
- Sensor reads run on a background queue. The menu bar and open dropdown are
  updated from a `Timer` in `.common` run loop mode, which is what keeps a
  dropdown live while it is tracking.
- Menu columns are sized to the longest label in each menu rather than to a
  fixed tab stop, so a sensor list is as wide as its contents and no wider.

## Self-tests

`Vitals --selftest` asserts the fixed-point decoders, byte formatting and
preference round-tripping. It exists instead of XCTest because XCTest ships
with Xcode, which this machine does not have.

## Diagnostics

- `Vitals --dump` prints every sensor per category with its key, label and
  value. `--samples N --interval S` takes several timed samples.
- `Vitals --dump-menu` prints the live `NSMenu` tree (titles, checkmarks,
  submenu contents, menu widths and status item frames).
- `Vitals --print-config` prints the stored preferences.
