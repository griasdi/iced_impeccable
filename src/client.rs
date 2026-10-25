//! `ctl [--wait MS] [--keep-going] [--window N] <socket> <command...>`: one
//! command line to a headless host, its reply on stdout; `-` instead of the
//! command reads a batch from stdin.

use std::io::{BufRead, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::channel;
use crate::protocol::{SCREENSHOT_OPTIONS, split_options};

/// How long `ctl` waits for a host that is not listening yet.
const CONNECT_WAIT: Duration = Duration::from_millis(5000);
const CONNECT_RETRY: Duration = Duration::from_millis(50);

fn usage() -> String {
    let bin = std::env::args_os()
        .next()
        .map(PathBuf::from)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "iced-impeccable".to_owned());
    format!(
        "usage: {bin} ctl [--wait MS] [--keep-going] [--window N] <socket> <command> [args...] | -"
    )
}

/// Exits 0 on `ok`, 1 on `err` or when the host cannot be reached, 2 on a
/// bad command line.
pub(crate) fn run(args: &[String]) -> i32 {
    let mut wait = CONNECT_WAIT;
    let mut keep_going = false;
    let mut window = None;
    let mut args = args;
    loop {
        match args {
            [flag, ms, rest @ ..] if flag == "--wait" => {
                let wait_ms = match crate::protocol::millis(Some(ms)) {
                    Ok(ms) => ms,
                    Err(e) => {
                        eprintln!("--wait: {e}");
                        eprintln!("{}", usage());
                        return 2;
                    }
                };
                wait = wait_ms;
                args = rest;
            }
            [flag, n, rest @ ..] if flag == "--window" => {
                match n.parse::<usize>() {
                    Ok(n) if n >= 1 => window = Some(n),
                    _ => {
                        eprintln!("--window {n:?} is not a window number (1, 2, ...)");
                        eprintln!("{}", usage());
                        return 2;
                    }
                }
                args = rest;
            }
            [flag, rest @ ..] if flag == "--keep-going" => {
                keep_going = true;
                args = rest;
            }
            _ => break,
        }
    }
    let [socket, words @ ..] = args else {
        eprintln!("{}", usage());
        return 2;
    };
    let socket = Path::new(socket);
    match words {
        [] => {
            eprintln!("{}", usage());
            2
        }
        [dash] if dash == "-" => batch(socket, wait, keep_going, window),
        _ => {
            if send(socket, &select(&words.join(" "), window), wait) {
                0
            } else {
                1
            }
        }
    }
}

/// Runs stdin line by line, one connection each, stopping at the first
/// `err` unless `keep_going`.
fn batch(socket: &Path, wait: Duration, keep_going: bool, window: Option<usize>) -> i32 {
    let mut input = String::new();
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) => {
                input.push_str(&line);
                input.push('\n');
            }
            Err(e) => {
                eprintln!("stdin: {e}");
                return 2;
            }
        }
    }
    let mut failed = false;
    for (n, line) in batch_lines(&input) {
        if !send(socket, &select(line, window), wait) {
            eprintln!("line {n}: {line}");
            if !keep_going {
                return 1;
            }
            failed = true;
        }
    }
    i32::from(failed)
}

/// The commands of a batch with their 1-based line numbers: blank lines and
/// `#` comments skipped.
fn batch_lines(input: &str) -> impl Iterator<Item = (usize, &str)> {
    input
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
}

/// Sends one command, prints the reply; true when it starts with `ok`.
fn send(socket: &Path, line: &str, wait: Duration) -> bool {
    let line = match absolute_paths(line) {
        Ok(line) => line,
        Err(e) => {
            say!("err cwd: {e}");
            return false;
        }
    };
    let reply = connect(socket, wait).and_then(|connection| connection.request(&line));
    match reply {
        Ok(reply) => {
            say!("{reply}");
            let first = reply.lines().next().unwrap_or("");
            first == "ok" || first.starts_with("ok ")
        }
        Err(e) => {
            say!("err {}: {e}", socket.display());
            false
        }
    }
}

/// Connects, retrying while the host has not created its socket or pipe yet.
fn connect(socket: &Path, wait: Duration) -> std::io::Result<channel::Connection> {
    let deadline = Instant::now() + wait;
    loop {
        match channel::connect(socket) {
            Err(e)
                if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::ConnectionRefused)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(CONNECT_RETRY);
            }
            result => return result,
        }
    }
}

/// `line` addressed to `--window N` unless it names a window itself.
fn select(line: &str, window: Option<usize>) -> String {
    match window {
        Some(n) if !line.starts_with('@') => format!("@{n} {line}"),
        _ => line.to_owned(),
    }
}

/// A relative `screenshot`, `record` or `drop` path resolved against this
/// process's working directory, not the host's. A leading `@N` stays.
fn absolute_paths(line: &str) -> std::io::Result<String> {
    if line.starts_with('@') {
        let (selector, command) = line.split_once(' ').unwrap_or((line, ""));
        return Ok(format!(
            "{selector} {}",
            absolute_paths(command.trim_start())?
        ));
    }
    let (name, rest) = line.split_once(' ').unwrap_or((line, ""));
    let (prefix, path) = match name {
        "record" | "drop" => ("", rest.trim_start()),
        "screenshot" => match split_options(rest, SCREENSHOT_OPTIONS) {
            Ok((_, path)) => (&rest[..rest.len() - path.len()], path),
            Err(_) => return Ok(line.to_owned()),
        },
        _ => return Ok(line.to_owned()),
    };
    if path.is_empty() || Path::new(path).is_absolute() {
        return Ok(line.to_owned());
    }
    let absolute = std::env::current_dir()?.join(path);
    Ok(format!(
        "{name} {}{}",
        prefix.trim_start(),
        absolute.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_skips_blank_lines_and_comments() {
        let input = "info\n\n# a comment\n  tap Save  \n\t\nquit\n";
        let lines: Vec<_> = batch_lines(input).collect();
        assert_eq!(lines, [(1, "info"), (4, "tap Save"), (6, "quit")]);
    }

    #[test]
    fn relative_screenshot_paths_become_absolute_after_options() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_paths("screenshot --crop 0 0 10 10 --zoom 2 out/a.png").unwrap(),
            format!(
                "screenshot --crop 0 0 10 10 --zoom 2 {}",
                cwd.join("out/a.png").display()
            )
        );
        assert_eq!(
            absolute_paths("record v.mp4").unwrap(),
            format!("record {}", cwd.join("v.mp4").display())
        );
        assert_eq!(
            absolute_paths("drop f.txt").unwrap(),
            format!("drop {}", cwd.join("f.txt").display())
        );
        // Absolute on this platform (`/tmp/..` is only root-relative on Windows).
        let absolute = format!("screenshot {}", cwd.join("x.png").display());
        assert_eq!(absolute_paths(&absolute).unwrap(), absolute);
        assert_eq!(absolute_paths("tap Save").unwrap(), "tap Save");
    }

    #[test]
    fn a_window_selector_stays_in_front_of_the_absolute_path() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute_paths("@2 screenshot out.png").unwrap(),
            format!("@2 screenshot {}", cwd.join("out.png").display())
        );
    }

    #[test]
    fn the_window_flag_addresses_lines_without_their_own_selector() {
        assert_eq!(select("tree", Some(2)), "@2 tree");
        assert_eq!(select("@3 tree", Some(2)), "@3 tree");
        assert_eq!(select("tree", None), "tree");
    }
}
