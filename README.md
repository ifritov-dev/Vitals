# Vitals

A macOS menu bar monitor for the numbers a Mac will tell you if you ask: CPU,
memory, temperatures, fan speeds, battery, network and disk. Native AppKit, no
web view, no Electron, one dependency (`libc`) and about 4,500 lines of Rust.

Every sensor is read from the same places `powermetrics`, `pmset` and
Activity Monitor read them — the AppleSMC IOKit driver, `mach` host statistics,
`sysctl`, `getifaddrs`, `statfs` and the IOPS power source description — with
the Objective-C bindings written by hand rather than pulled in as a crate.

> Apple Silicon only. The Objective-C interop relies on `objc_msgSend`'s arm64
> calling convention, so the bundle is built for `aarch64-apple-darwin`.

## Install

### From a release

Download the `.dmg` from [Releases](https://github.com/ifritov-dev/vitals/releases),
open it and drag **Vitals** to *Applications*.

The build is signed ad-hoc, not notarised, so on first launch macOS may refuse
it. Either right-click the app and choose **Open**, or clear the quarantine
flag once:

```sh
xattr -dr com.apple.quarantine /Applications/Vitals.app
```

### From source

```sh
git clone https://github.com/ifritov-dev/vitals.git
cd vitals
make install      # builds, signs and copies to /Applications
make run          # or: build and launch from dist/ without installing
```

Requirements: macOS 11 or newer, Xcode command line tools, and a Rust
toolchain (`rustup`). `make` targets:

| Target | What it does |
| --- | --- |
| `make build` | Release binary in `target/release/vitals` |
| `make app` | Signed `dist/Vitals.app`, version stamped from `Cargo.toml` |
| `make run` | `make app` and launch it |
| `make install` | Copies the bundle to `/Applications`, removing stray copies |
| `make dmg` | `dist/Vitals-<version>.dmg` with an *Applications* shortcut |
| `make verify` | Self-test plus a sensor dump |
| `make uninstall` | Removes `/Applications/Vitals.app` |
| `make clean` | `cargo clean` and `rm -rf dist` |

## Using it

Vitals lives in the menu bar as a single status item — no Dock icon, no window
(`LSUIElement` is set). The item shows one `key=value;` slot per category you
have put there, and clicking it opens the dropdown.

- **Rows** — each category shows its average and opens a submenu of every
  sensor it found: individual SMC keys for temperature, voltage and fans, per
  core for the CPU, per volume for storage.
- **Choosing a sensor** — picking a sensor shows that sensor in the menu bar;
  picking the row that is already active removes the category from the bar.
- **Refresh Interval** — 1 to 10 seconds, 5 by default. Only the categories on
  the menu bar are polled; the rest keep their last reading.
- **Start at Login** — a LaunchAgent in `~/Library/LaunchAgents`. macOS
  sometimes deletes that file on its own (background item management prunes
  agents it has registered too often), so the setting is stored in
  `NSUserDefaults` and the agent is rebuilt at every launch.
- **Quit Vitals** — `⌘Q` while the menu is open.

Readings update in place while the menu is open, and a sensor whose key stops
answering falls back to *No data* after three misses rather than showing a
stale number forever.

## Command line

The binary carries the whole diagnostic surface, so the UI can be checked
without photographing the screen. Everything below reads the real sensors and
changes nothing on screen. The two `--set-*` rows are the exception: they write
persistent state, and both refuse to run unless the executable is the installed
app bundle, because that is the preference domain the app reads.

| Flag | What it does |
| --- | --- |
| `--selftest` | Decoder, formatter, model and preference assertions |
| `--dump [--samples N --interval S] [--out path]` | Every category and sensor with live values |
| `--dump-menu [--out path]` | Builds the real menu, prints rows, column geometry and submenus |
| `--bench-menu` | Times the dropdown build, a submenu build, a live refresh and the poll paths |
| `--render-rows <path.png>` | Draws the dropdown's rows offscreen for checking the layout |
| `--print-config` | Shown categories, interval, login item and selections |
| `--set-selection <category> [--sensor <id>]` | Saves a choice without opening the menu (writes prefs, bundle only) |
| `--login-status` | Preference, agent file and launchd job state |
| `--set-login on\|off` | Enables or disables the login item (writes prefs, bundle only) |
| `--demo-open`, `--demo-close`, `--demo-actions` | Drives the menu from a timer, for automated captures |

```console
$ /Applications/Vitals.app/Contents/MacOS/Vitals --dump --samples 1
# Vitals sensor dump
# host: Apple M3 Pro · macOS 14.6
# SMC: AppleSMC opened unprivileged · 4312 keys total, 189 polled per refresh

# sample 1
## Temperature  —  average 46.7 °C  ·  61 sensor(s)
   TC0P  CPU Proximity           46.7 °C
   …
```

## Data sources

| Category | Source |
| --- | --- |
| Temperature, Voltage, Fan | AppleSMC via `IOConnectCallStructMethod`, decoded from the `sp78`/`fpe2`/`flt`/`si*` data types |
| Memory, Processor, System | `host_statistics64`, `host_processor_info`, `sysctl` |
| Network | `getifaddrs`, per-interface 32-bit counters diffed with wrap-around |
| Storage | `statfs` per mounted volume |
| Battery | IOPS power source description, with the SMC keys as a fallback |

## Layout

```
src/
  main.rs        argument dispatch, headless commands, startup reconcile
  ui.rs          status item, dropdown, submenu building, @objc callbacks
  objc.rs        the hand-written Objective-C runtime layer
  model.rs       categories, readings, formatting
  store.rs       one snapshot of every category, SMC connection lifecycle
  poller.rs      the worker thread that reads sensors off the main thread
  smc.rs         AppleSMC wire protocol and decoders
  sensors/       smc categories, battery, kernel counters
  login.rs       LaunchAgent login item
  prefs.rs       NSUserDefaults-backed preferences
  diag.rs        text renders for the diagnostics flags
  icons.rs       Phosphor icon font
  selftest.rs    assertions behind --selftest
```

The app is single-threaded by construction: every timer, message and menu
callback runs on the main run loop, and the only other thread reads sensors and
hands finished snapshots over a channel.

## Development

```sh
make verify                                  # self-test + dump
make app && dist/Vitals.app/Contents/MacOS/Vitals --dump-menu
```

`cargo build` alone produces a bare binary without `Info.plist` or the icon
font, so the app's own resources only exist in the bundle that `make app`
builds.

## Releases

`.github/workflows/release.yml` runs on demand from the **Actions** tab:
*Release* → *Run workflow*. It stamps the version you enter into `Cargo.toml`
and `Info.plist`, builds the arm64 bundle, signs it ad-hoc, packages
`dist/Vitals-<version>.dmg`, verifies the bundle with `--selftest`, and
publishes a GitHub release tagged `v<version>` with the DMG attached.

## Licence

GPL-3.0-or-later — see [LICENSE](LICENSE). Copyright © 2026 ifritov-dev.

The bundled icon font is [Phosphor](https://phosphoricons.com) (MIT), which is
included in `packaging/Phosphor.ttf` and keeps its own licence.
