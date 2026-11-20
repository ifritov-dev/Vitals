# Vitals

A macOS menu bar sensor monitor in the spirit of the GNOME Vitals extension:
one status slot showing the categories you picked, and a dropdown with live
readings from AppleSMC and the kernel counters. Written in Rust against AppKit
through the Objective-C runtime; no Swift, no storyboards, no nibs.

## Build

```sh
make app       # bundle into dist/Vitals.app
make install   # install to /Applications, replacing what is there
make verify    # self-test the built bundle
make uninstall
```

`cargo` and `rustc` are resolved from `~/.rustup/toolchains/stable-aarch64-apple-darwin/bin`
by the Makefile, so neither has to be on `PATH`.

## How it works

- **Sensors.** AppleSMC through IOKit for temperature, voltage, fans and the
  battery rails; `host_statistics64`, `host_processor_info`, `getloadavg`,
  `getifaddrs`, `statfs` and `IOPowerSources` for everything else. A full poll
  is 264 SMC keys; only the categories the menu bar shows are polled.
- **Threading.** Polling runs on a worker thread and hands finished snapshots
  to the main thread, so opening and moving through the menu never waits on
  the driver.
- **Menu.** Submenus are filled when they open, not with the dropdown. The
  value column is placed per row with a tab stop, so all values end on one edge
  and the menu stays as narrow as its widest row.
- **Icons.** Phosphor, bundled as a font in `Contents/Resources`. An icon font
  advances one fixed width per glyph, which is what keeps the names aligned.
- **Preferences** live in `NSUserDefaults` under `com.daniel.vitals`: shown
  categories, per-category sensor choice and the refresh interval.
- **Start at Login** is a LaunchAgent written to `~/Library/LaunchAgents`.

## Command line

Everything below runs against the real sensors without changing anything on
screen, which is how the UI is checked.

| Flag | What it does |
| --- | --- |
| `--selftest` | Decoder, formatter and preference assertions |
| `--dump [--samples N --interval S] [--out path]` | Every category and sensor with live values |
| `--dump-menu [--out path]` | Builds the real menu, prints its rows, column geometry and icon state |
| `--bench-menu` | Times the dropdown build, a submenu build, a live refresh and the three poll paths |
| `--render-rows <path.png>` | Draws the dropdown's rows offscreen for checking the layout |
| `--print-config` | Shown categories, interval and selections |
| `--set-selection <category> [--sensor <id>]` | Saves a choice without opening the menu |
| `--login-status`, `--set-login on\|off` | Reads or sets the login item |
| `--demo-open`, `--demo-close`, `--demo-actions` | Drives the menu from a timer, for automated captures |

## Licence

The icon font is Phosphor (MIT), in `packaging/Phosphor.ttf`.
