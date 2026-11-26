# Contributing to Vitals

Vitals is a small app on purpose: one dependency, no web view, and every sensor
read through the same IOKit and `mach` calls that `powermetrics` and Activity
Monitor use. Patches that keep it that way are welcome.

## Before you write code

Open an issue first for anything larger than a bug fix. New sensor categories,
new menu bar behaviour, or a new dependency usually need a decision on scope
before anyone writes the patch, because the app's selling point is what it costs
to run.

Bug reports are most useful with the readings that were wrong. Include:

- macOS version and the Mac model (`Apple M1 Pro`, `M3 Max`, and so on)
- what the menu bar showed and what the truth was (`powermetrics`, `pmset -g`,
  Activity Monitor)
- the relevant slice of `/Applications/Vitals.app/Contents/MacOS/Vitals --dump`

Issues in the AppleSMC key names are the usual cause of a category showing
nothing, so the dump saves a round trip.

## Ground rules

- **Apple Silicon, macOS 11 or newer.** The Objective-C layer calls
  `objc_msgSend` directly, which only works with the arm64 calling convention.
  Intel support is not a small change and is not wanted.
- **No new crates without a reason in the PR.** `libc` is the only dependency.
  Anything that pulls a tree of crates with it needs to justify itself against
  the one-dependency promise, and a TLS stack pulled in to make one HTTP call is
  exactly the trade this app refuses.
- **The UI thread stays the UI thread.** Every timer, message and menu callback
  runs on the main run loop, and sensor reads happen on one worker thread that
  hands finished snapshots back over a channel. Blocking work on the main loop,
  or reading sensors from it, will be sent back.
- **Cheap by default.** A redraw costs more than a poll, so a change that
  writes the status item title or adds a wakeup needs to say what it costs when
  the bar is hidden or the display is asleep.
- **Match the surrounding code.** Comments explain why a thing is the way it is,
  not what the line does. Prose style in docs and comments is plain and
  specific.

## Getting set up

Requirements are macOS 11 or newer, Xcode command line tools, and a Rust
toolchain from `rustup`.

```sh
git clone https://github.com/ifritov-dev/vitals.git
cd vitals
make run          # builds dist/Vitals.app and launches it
make verify       # self-test plus a sensor dump
```

`cargo build` alone gives you a bare binary with no `Info.plist` and no icon
font, so use `make` for anything you want to look at on screen.

## Checking your change

`make verify` is the check that matters. It runs `--selftest`, which asserts the
SMC decoders, the formatters, the battery maths and the release parser against
known bytes, and then dumps every live sensor. Nothing runs on a pull request
yet: the build and release workflows are manual only, so run `make verify`
yourself and say so in the PR.

For anything that touches layout or a reading:

| What changed | How to show it |
| --- | --- |
| A sensor decoder | A `--dump` before and after, or a new assertion in `src/selftest.rs` |
| The dropdown, submenu or columns | `--dump-menu` output, or `--render-rows out.png` |
| The menu bar title | A screenshot of the bar, plus what the category read |
| Update or release handling | `--check-updates` output against a real release |

Run `cargo fmt` before you commit. Keep `dist/` and `target/` out of the
commit; both are ignored already.

## Commits

Write commits in the imperative, in the voice the history already uses:
`Fix inflated memory usage reading`, `Remove the status bar tooltip`, `Stop
paying for a menu bar nobody is looking at`. One logical change per commit, and
the body says why when the reason is not obvious from the diff.

## Opening a pull request

1. Fork the repository and branch from `main`, or branch in place if you have
   push access. Nothing else needs a special branch name.
2. Keep the pull request to one subject. If you fixed a bug and also tidied the
   file around it, send the tidy separately.
3. Target `main` and fill in the template. The sections ask what changed, why,
   and how you checked it, and the checklist takes a minute.
4. Push additional commits to your branch rather than force pushing, so review
   comments keep their place.

A pull request is ready when it builds, `make verify` passes, and the change
does what the description says it does. Version bumps and releases are done by
the maintainer through the *Release* workflow, so leave `Cargo.toml` version
alone.

## Licence

Vitals is GPL-3.0-or-later. By sending a pull request you agree that your
contribution is licensed under the same terms, and that you wrote it or have the
right to submit it. Do not paste code from a project under a different licence
into this one.
