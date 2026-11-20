# Vitals

A native macOS menu bar sensor monitor in the spirit of the GNOME
[Vitals](https://github.com/corecoding/Vitals) extension. Written in Rust with
no third-party crates beyond `libc`.

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
| Storage | `statfs` on `/` |
| Battery | `IOPowerSources` + SMC `B0AV`/`B0AC` |

Every category degrades gracefully: if it finds no usable sensor on this
hardware it shows **No data** instead of an invented number.

## Choosing what a menu bar slot shows

Click the slot, hover a category and pick **Average** or any individual
sensor. The choice is stored per category and the category is added to the
menu bar automatically, so you never have to visit a settings window.

Everything is configured from the menu itself: **Menu Bar** toggles which
categories have a slot, and there are toggles for icons, the single compact
slot, and the refresh interval (1–10 s).

## Build

```sh
make build     # cargo build --release
make app       # assemble and ad-hoc sign dist/Vitals.app
make run       # open the app
make install   # copy to /Applications (refuses to overwrite)
make verify    # self-tests plus a sensor dump
```

The app is an `LSUIElement` accessory: no Dock icon, no root, no privileges
beyond reading IOKit.

## Design notes

- **No Objective-C binding crate.** AppKit is reached through hand-written
  `objc_msgSend` transmutes in `src/objc.rs`; the app needs about a dozen
  classes and this keeps the build dependency-free. The cost is that every
  message signature has to be right — a wrong arity shows up as
  `Rust cannot catch foreign exceptions`, because it throws an ObjC exception.
- **The SMC wire format is an 80-byte struct that must match the driver.**
  `SmcKeyData` is `#[repr(C)]` over exactly the C fields and is asserted to be
  80 bytes at open; a layout mismatch makes every call return
  `kIOReturnBadArgument`.
- **Reading a key is two round trips:** KEYINFO for the type and size, then
  READ_BYTES with that size prefilled. One call comes back with SMC result 132.
- **`sp78` is signed 7.8 fixed point** and `fpe2` is unsigned 14.2, so the two
  need different sign handling. `decode` covers `flt`, `ioft`, `sp78`, `sp87`,
  `sp5a`, `sp96`, `sp4b`, `fpe2`, `fp1f`, `ui8`…`si32`, and trims type strings
  because the SMC pads them to four characters with spaces.
- The key list and each key's type are read once at startup; a refresh only
  re-reads values.
- The dropdown is a real `NSMenu` attached to each status item, so AppKit
  anchors it under the slot that was clicked. A ticker `NSTimer` added in
  `NSRunLoopCommonModes` rewrites only the value columns while a menu tracks,
  which keeps an open dropdown live.
- Menu columns are sized to the longest label in each menu rather than to a
  fixed tab stop, so a sensor list is as wide as its contents.

## Self-tests

`Vitals --selftest` asserts the fixed-point decoders, byte formatting and the
stored preferences. It exists instead of `cargo test`'s default harness because
the interesting logic sits in the same binary that has to be run against live
hardware anyway.

## Diagnostics

- `Vitals --dump` prints every sensor per category with its key, label and
  value. `--samples N --interval S` takes several timed samples.
- `Vitals --dump-menu` builds the real dropdown and prints its tree — titles,
  checkmarks, submenu contents and menu widths — without creating any status
  item, so it can be checked without touching the screen.
- `Vitals --print-config` prints the stored preferences.
