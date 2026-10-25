# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `iced-impeccable install [--agent claude,omp] [--yes] [DIR]` copies the
  skill, embedded in the binary, into the git repository containing `DIR`
  (`.claude/skills/iced-impeccable/`, `.omp/skills/iced-impeccable/`). It
  first offers to remove a marketplace install of the plugin, asks for the
  agents with the detected ones preselected, updates a stale copy and leaves
  a symlinked skill directory alone.
- The crate package ships the skill files (`plugin/skills/`).
- `hover [--nth N] TARGET` moves the pointer to the centre of a widget, for
  hover states and tooltips: `ok X Y`.
- `type -e TEXT` and `clip-set -e TEXT` read `\n` (Enter in `type`, newline in
  `clip-set`), `\t` (Tab) and `\\`; other escapes are errors.
- Target `=TEXT` is exact text even when it starts with `#`, `~` or `=`.
- `screenshot --no-pointer` renders as if the pointer were outside the
  window: no hover state.
- `wait-idle [--timeout MS] [MS]`: the time `wait-idle` gives up after is
  settable (default 10000 ms, the old fixed value).

### Fixed

- `install` now selects the agents before it removes anything, so a run
  without a terminal and without `--agent` exits 2 with the plugin intact,
  and it removes only the installs that collide with the chosen agents (an
  omp plugin always, since omp also loads `.claude/skills`; a Claude Code
  plugin only when `claude` is chosen).
- `install` writes the new skill copy next to the old one and swaps it in, so
  a failed update leaves the installed skill untouched. A same-version update
  reports `updated VERSION (files changed)`, and the installed version is
  read from the frontmatter only.
- `ctl`, `sheet`, `install` and `help` end with exit status 141 instead of
  panicking when stdout is a closed pipe (`ctl … | head`).
- A host no longer deletes a file that is not a socket at its `--control`
  path, and refuses to start (`another host is listening`, exit 1) when a
  live host owns the socket; only a stale socket file is replaced.
- A client can no longer exhaust the host with numbers or lines: waits and
  `--timeout` are at most an hour (`ctl --wait` too), `move` and `drag` times
  a minute, `drag` steps 1 to 1000, window sizes 1 to 16384, scales 0.25 to 8.
  A window may not pass 8192 physical pixels on a side (`--size` with
  `--scale`, `resize` and `scale` reply `err too large: …`), and a zoomed
  screenshot at most 64 million pixels (`err zoom too large: WxH`).
- A request line over 64 KiB or that is not UTF-8 gets an `err` reply instead
  of being cut and parsed, and a reply over 16 MiB becomes an `err` too.
- `wait-idle` with a quiet time of nine seconds or more no longer always
  times out: the timeout now counts from the end of the quiet time.
- `windows` escapes the whole title like `tree` does (backslashes and control
  characters, not only quotes).
- The host drops the app's state at exit, so its `Drop` runs (a watchdog ends
  the process after 5 s if it hangs).
- `resize` replies with the clamped size in OS-logical units, as `help`
  says, not the interface size.
- `record` reads ffmpeg's stderr while it runs (a chatty ffmpeg can no longer
  block it) and keeps only the first 64 KiB. An app that resizes the window
  during a recording ends it with `err window resized during recording`
  and the path of the video up to that point.
- Keys carry physical key codes: letters, digits, punctuation and named keys
  (`key a`, `key enter`, `type` text, `key code:KeyA`) report the US-layout
  `physical_key`, so shortcuts that match physical keys fire. `code:KeyA`
  and `code:ArrowLeft` also carry the key and text, other codes only the code.
- `keydown` modifiers stay held: later `key`, `type`, `click` with modifiers
  and `keyup` of another key report them, and `keyup` leaves the rest down
  instead of clearing all modifiers. A held Ctrl, Alt or Super suppresses the
  text of a key press, like on a keyboard.
- `find`, `tap` and `hover` without `--nth` ignore matches that are scrolled
  out of view when exactly one match is visible, instead of reporting an
  ambiguity.
- Contact sheets shrink each image to the cell width with an area average,
  instead of halving every image until the widest fits: images of different
  sizes (a 1x and a 2x capture) now come out at the same width, and a
  narrower image keeps its size.
- `wait-idle` waits for the app's running tasks: `ok idle` and `ok animating`
  are held back while a task started within the last 5 s has not ended (a
  `Task::perform` that sleeps, a request), so a command that starts one is
  no longer answered before its result arrives. A redraw or relayout request
  from the app no longer counts as activity.
- Input commands settle on activity only: a heartbeat (a clock tick, a
  frame message) no longer keeps them waiting out the full 250 ms.
- Window actions: `monitor_size` reports the window's size, `GainFocus`,
  `set_min_size` and `set_max_size` work (a window is clamped to a new limit
  and the app hears the resize), `window::raw_id` returns the id, and an
  action the host does not implement is logged by name.
- Windows take part in keyboard focus: a window the host opens gets
  `Focused`, the one that had focus `Unfocused`, and closing the focused
  window focuses the newest remaining one.

### Changed

- `type` text that starts with the word `-e` is now the escape flag; write
  `type -e '-e ...'` to type it.
- The backends and the installer are cargo features: `wgpu`, `tiny-skia`,
  `x11`, `wayland` and `cli`, all on by default. With
  `default-features = false` a library user gets only the renderers it names
  (`features = ["wgpu", "tiny-skia"]`), no `serde_json` and no installer;
  `x11`/`wayland` then come from the app's own `iced`; the crate itself
  enables only iced's `advanced` and `thread-pool` (a futures executor,
  without which iced does not compile). Without `--backend`
  the host picks `wgpu` when that feature is on, else `tiny-skia`. The
  `iced-impeccable` binary requires `cli`. A host that finds no renderer says
  it may not be compiled in.
- The skill is aligned with impeccable 4.7: native-equivalence rule, command
  overrides for `critique`, `optimize`, `craft`, `doctor` and bare
  `/impeccable`, per-widget status lists, a corrected modal recipe, and
  documented `hover`, `=TEXT`, `type -e`, `--no-pointer`, `wait-idle
  --timeout` and `record`.

## [0.2.0] - 2026-10-11

### Added

- `Remote` and `run` accept an `iced::Daemon` as well as an
  `iced::Application` (new trait `Runnable`). The host keeps every window the
  program opens: `window::open`, `close`, `resize`, `maximize` and the window
  queries act on the given window, and an application exits with its last
  window, a daemon only through `iced::exit`.
- `windows` command: `ok N`, then `INDEX WxH scale S title="TITLE"` per open
  window. `info` ends with `windows N`.
- A leading `@N ` addresses window N (1-based, in open order); without it a
  command acts on the oldest open window. `ctl --window N` adds `@N ` to every
  line without one.
- `--clipboard TEXT` host flag: the clipboard's content at start.

### Changed

- `Remote<P>` is generic over the program (`Remote<A: Runnable>`) instead of
  `Application<P>`'s inner program.

## [0.1.1] - 2026-10-02

### Added

- `drop PATH` command: delivers `FileHovered` then `FileDropped` for `PATH`;
  `ctl` resolves a relative path against its own working directory.
- `Remote::windowed` and `Remote::headless` setup hooks: run code only before
  the windowed app or only in a `--headless` process.

## [0.1.0] - 2026-10-02

### Added

- `Remote` builder and `run` for any iced 0.14 `Application`: `<bin> --headless
  --control <socket>` runs the app offscreen with a hardware renderer (wgpu,
  or tiny-skia via `--backend`), `<bin> ctl <socket> ...` drives it, anything
  else runs the windowed app. `heartbeat` marks periodic messages, `command`
  registers app-specific commands; `is_headless` and `app_args` for app code.
- Host flags `--size`, `--scale`, `--appearance dark|light`, `--backend`; the
  app's fonts, default font, theme, style, scale factor and window settings
  (`min_size`, `max_size`, `exit_on_close_request`) are honoured.
- Control protocol with multi-line replies: pointer and keyboard input (key
  aliases such as `left`, `esc`, `pgdn`), `tap` / `find` / `find-all` on
  targets (`#ID`, `~SUBSTRING`, exact text, `--nth N`) with errors that name
  similar texts, ambiguity and visibility, `tree` widget inventory, `focused`,
  `wait-for` / `wait-gone` / `wait-idle` (`ok idle`, `ok animating`),
  `screenshot` with `--annotate`, `--crop` and `--zoom`, `record` via ffmpeg,
  `resize`, `scale`, `appearance`, clipboard, `quit` that respects close
  requests, and `help`.
- Transport over a Unix socket, on Windows a named pipe.
- `iced-impeccable` binary: `ctl` with connect retry (`--wait`) and stdin batch
  mode (`-`, `--keep-going`), `sheet` contact sheets, `help`.
- Demo app (`examples/demo.rs`) exercising every protocol path.
- `iced-impeccable` skill plugin, a companion to the `impeccable` design
  skill: iced platform reference, headless verification, audit, adapt,
  variants and DESIGN.md mapping.

[Unreleased]: https://github.com/griasdi/iced_impeccable/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/griasdi/iced_impeccable/releases/tag/v0.2.0
[0.1.1]: https://github.com/griasdi/iced_impeccable/releases/tag/v0.1.1
[0.1.0]: https://github.com/griasdi/iced_impeccable/releases/tag/v0.1.0
