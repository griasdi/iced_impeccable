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

[Unreleased]: https://github.com/tuco86/iced_impeccable/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tuco86/iced_impeccable/releases/tag/v0.2.0
[0.1.1]: https://github.com/tuco86/iced_impeccable/releases/tag/v0.1.1
[0.1.0]: https://github.com/tuco86/iced_impeccable/releases/tag/v0.1.0
