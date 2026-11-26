<h1 align="center">Vitals</h1>

A macOS menu bar monitor for the numbers a Mac will tell you if you ask: CPU,
memory, temperatures, fan speeds, battery, network and disk. Native AppKit, no
web view, no Electron, one dependency (`libc`) and about 6,000 lines of Rust.

<div align="center">
  <img width="458" height="332" alt="image" src="https://github.com/user-attachments/assets/d01a4626-388e-4a74-b025-9c310a9dc2d8" />
</div>

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
  core for the CPU, the boot volume for storage.
- **Choosing a sensor** — picking a sensor shows that sensor in the menu bar;
  picking the row that is already active removes the category from the bar.
- **Refresh Interval** — 1 to 10 seconds, 5 by default. Only the categories on
  the menu bar are polled, and the dropdown follows the same interval while it
  is open; the rest keep their last reading. Two things stretch it on their
  own: two minutes without input slow the bar to 20 seconds, and a display
  that is asleep or a screen that is locked stops the refresh loop completely,
  which restarts with a fresh reading the moment the bar is back on screen.
- **Fixed Status Width** — keeps the menu bar item at the width the categories
  on it need, so a reading that grows ("9°" to "100°") widens it once instead
  of shifting the whole bar on every refresh. Turn it off to let the item size
  itself to whatever it currently says.
- **Row toggles keep the menu open** — choosing a sensor, an interval or either
  switch re-opens the dropdown where it was, so several changes can be made
  without clicking the status item again.
- **Start at Login** — a LaunchAgent in `~/Library/LaunchAgents`. macOS
  sometimes deletes that file on its own (background item management prunes
  agents it has registered too often), so the setting is stored in
  `NSUserDefaults` and the agent is rebuilt at every launch.
- **Quit Vitals** — `⌘Q` while the menu is open.

Readings update in place while the menu is open, and a sensor whose key stops
answering falls back to *No data* after three misses rather than showing a
stale number forever.

## What it costs

A menu bar app that polls sensors is only worth running if it is nearly free,
so the expensive parts are scheduled rather than ticked:

- **A redraw is the pricey part, not the reading.** Writing a new title to the
  status item runs through WindowServer and FrontBoard and costs tens of
  millions of instructions — an order of magnitude more than a poll of every
  sensor. The title is therefore only written when its text actually changes,
  and never while the bar is off screen.
- **Nothing runs behind a dark display.** The refresh loop stops on display
  sleep, screen lock and fast user switching, and resumes with a full poll on
  wake. Measured on an M1 Pro: exactly zero wakeups, zero instructions and
  zero energy in that state.
- **An average samples a slice.** A category showing its average reads an
  eighth of its sensors per refresh (about 30 driver calls instead of 240 for
  temperature) and averages that slice against the values the rest of the
  family still holds, so the average still covers every sensor. Opening the
  dropdown polls everything fresh.
- **Sleeping is free.** The app holds no power assertions and does not fight
  App Nap, `caffeinate` or the system sleep timer. One-shot timers carry a
  tolerance so their wakeup can be folded into whatever else the machine is
  doing.

## Updates

Vitals updates itself from the releases published on GitHub. Every launch
checks the latest release once, and the dropdown carries a row above **Quit
Vitals** that reads *Check for Updates*. When that release is newer than the
running build the row becomes *Update to vX.Y.Z*; choosing it downloads the DMG
from that release, verifies and mounts the image, copies the bundle inside over
the copy in `/Applications`, and relaunches the new one. The check is a bounded
`curl` call to the GitHub API, because a binary with one dependency has no TLS
stack of its own to use.

Three limits are worth knowing before you rely on it:

- **It only updates an installed copy.** If `/Applications/Vitals.app` is not
  there the row refuses and says so, rather than installing a second app you
  would not open; a build run out of `dist/` is not something to replace.
- **The download is ad-hoc signed, not notarised.** Gatekeeper trusts it the
  same way it trusts the DMG you install by hand, so the quarantine flag the
  download carries is cleared after the swap and the new copy starts without a
  second confirmation.
- **A check needs the network.** A failed check, like a rate-limited one, is
  reported in the menu and leaves the installed app untouched.

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
| `--check-updates` | Current version and whether a newer release exists (network) |
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
| Storage | `statfs` on the boot volume |
| Battery | IOPS power source description |

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

## Contributing

Issues that come with `--dump` output are the ones that get fixed fastest. For
patches, [CONTRIBUTING.md](CONTRIBUTING.md) covers the setup, the checks a
change has to pass (`make verify` above all), and the ground rules that keep the
app at one dependency.

## Releases

`.github/workflows/release.yml` runs on demand from the **Actions** tab:
*Release* → *Run workflow*. It stamps the version you enter into `Cargo.toml`
and `Info.plist`, builds the arm64 bundle, signs it ad-hoc, packages
`dist/Vitals-<version>.dmg`, verifies the bundle with `--selftest`, and
publishes a GitHub release tagged `v<version>` with the DMG attached.

That published release is also what the running app consumes: installing the
update is a copy of the DMG's bundle, so a version only reaches installed copies
once it has a release.

## Licence

GPL-3.0-or-later — see [LICENSE](LICENSE). Copyright © 2026 ifritov-dev.

The bundled icon font is [Phosphor](https://phosphoricons.com) (MIT), which is
included in `packaging/Phosphor.ttf` and keeps its own licence.
