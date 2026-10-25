//! The subcommands of the standalone `iced-impeccable` binary, usable from
//! any other binary as well.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::image;
use crate::install::{self, AGENTS, Agent, Install, Outcome};
use crate::protocol;

const SHEET_MAX_WIDTH: u32 = 2560;

/// `ctl [--wait MS] [--keep-going] [--window N] <socket> <command...>`; returns the exit
/// code.
pub fn ctl(args: &[String]) -> i32 {
    crate::client::run(args)
}

/// The built-in commands with synopsis and summary, then the target, key
/// and coordinate lines. Commands an app registers are only listed by the
/// running app's `help`.
pub fn help() -> String {
    protocol::help_lines()
        .chain(protocol::help_footer())
        .collect::<Vec<_>>()
        .join("\n")
}

fn sheet_usage() -> &'static str {
    "usage: iced-impeccable sheet [--cols N] [--labels a,b,c] [--max-width PX] OUT.png IN.png..."
}

/// `sheet [--cols N] [--labels a,b,c] [--max-width PX] OUT.png IN.png...`:
/// lays screenshots out in one labelled grid. Prints `ok OUT WxH`; returns
/// the exit code.
pub fn sheet(args: &[String]) -> i32 {
    let mut cols = None;
    let mut labels: Option<Vec<String>> = None;
    let mut max_width = SHEET_MAX_WIDTH;
    let mut rest = args;
    loop {
        match rest {
            [flag, value, tail @ ..] if flag == "--cols" => {
                match value.parse::<usize>().ok().filter(|&n| n > 0) {
                    Some(n) => cols = Some(n),
                    None => {
                        return bad(
                            &format!("--cols {value:?} is not a positive integer"),
                            sheet_usage(),
                        );
                    }
                }
                rest = tail;
            }
            [flag, value, tail @ ..] if flag == "--labels" => {
                labels = Some(value.split(',').map(str::to_owned).collect());
                rest = tail;
            }
            [flag, value, tail @ ..] if flag == "--max-width" => {
                match value.parse::<u32>().ok().filter(|&n| n > 0) {
                    Some(n) => max_width = n,
                    None => {
                        return bad(
                            &format!("--max-width {value:?} is not a positive integer"),
                            sheet_usage(),
                        );
                    }
                }
                rest = tail;
            }
            _ => break,
        }
    }
    let [out, inputs @ ..] = rest else {
        return bad("missing OUT.png", sheet_usage());
    };
    if inputs.is_empty() {
        return bad("missing IN.png", sheet_usage());
    }
    let labels = match labels {
        Some(labels) if labels.len() != inputs.len() => {
            return bad(
                &format!("{} labels for {} images", labels.len(), inputs.len()),
                sheet_usage(),
            );
        }
        Some(labels) => labels,
        None => inputs
            .iter()
            .map(|p| {
                Path::new(p)
                    .file_stem()
                    .map_or_else(|| p.clone(), |s| s.to_string_lossy().into_owned())
            })
            .collect(),
    };
    let mut images = Vec::with_capacity(inputs.len());
    for (path, label) in inputs.iter().zip(labels) {
        match image::read_png(Path::new(path)) {
            Ok(frame) => images.push((label, frame)),
            Err(e) => {
                say!("err {path}: {e}");
                return 1;
            }
        }
    }
    let sheet = match image::sheet(&images, cols, max_width) {
        Ok(sheet) => sheet,
        Err(e) => {
            say!("err {e}");
            return 1;
        }
    };
    let out = PathBuf::from(out);
    if let Err(e) = image::write_png(&out, &sheet) {
        say!("err {}: {e}", out.display());
        return 1;
    }
    say!(
        "ok {} {}x{}",
        out.display(),
        sheet.size.width,
        sheet.size.height
    );
    0
}

fn install_usage() -> &'static str {
    "usage: iced-impeccable install [--agent claude,omp] [--yes] [DIR]"
}

/// `install [--agent claude,omp] [--yes] [DIR]`: copies the skill into the
/// git repository containing DIR (default `.`) for the chosen agents, after
/// removing a marketplace install of the plugin. Prints `ok`/`err` lines;
/// returns the exit code (0 done, 1 aborted or failed, 2 bad usage).
pub fn install(args: &[String]) -> i32 {
    let mut chosen: Option<Vec<&'static Agent>> = None;
    let mut yes = false;
    let mut dir: Option<&str> = None;
    let mut rest = args;
    loop {
        match rest {
            [flag, value, tail @ ..] if flag == "--agent" => {
                let mut list: Vec<&'static Agent> = Vec::new();
                for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    let Some(agent) = AGENTS.iter().find(|a| a.name == name) else {
                        return bad(
                            &format!("unknown agent {name:?} (claude, omp)"),
                            install_usage(),
                        );
                    };
                    if !list.iter().any(|a| std::ptr::eq(*a, agent)) {
                        list.push(agent);
                    }
                }
                if list.is_empty() {
                    return bad("--agent needs a name", install_usage());
                }
                chosen = Some(list);
                rest = tail;
            }
            [flag] if flag == "--agent" => return bad("--agent needs a name", install_usage()),
            [flag, tail @ ..] if flag == "--yes" || flag == "-y" => {
                yes = true;
                rest = tail;
            }
            [arg, ..] if arg.starts_with('-') => {
                return bad(&format!("unknown option {arg:?}"), install_usage());
            }
            [arg, tail @ ..] => {
                if dir.is_some() {
                    return bad("too many arguments", install_usage());
                }
                dir = Some(arg);
                rest = tail;
            }
            [] => break,
        }
    }
    let root = match install::repo_root(Path::new(dir.unwrap_or("."))) {
        Ok(root) => root,
        Err(e) => {
            say!("err {e}");
            return 1;
        }
    };

    // The marketplace plugin would load next to the repo copy.
    let mut detected = [false; AGENTS.len()];
    let mut installs = Vec::new();
    for (i, agent) in AGENTS.iter().enumerate() {
        match install::list_installs(agent, &root) {
            Err(e) => eprintln!("warning: {e}"),
            Ok(None) => {}
            Ok(Some(scopes)) => {
                detected[i] = true;
                installs.extend(scopes.into_iter().map(|scope| Install { agent, scope }));
            }
        }
    }
    let selected = match chosen {
        Some(list) => list,
        None if io::stdin().is_terminal() => {
            let mut ticked: Vec<bool> = AGENTS
                .iter()
                .zip(detected)
                .map(|(agent, detected)| {
                    let config = agent.skills_dir.split('/').next().unwrap_or_default();
                    detected || root.join(config).is_dir()
                })
                .collect();
            match select_agents(&root, &mut ticked) {
                Some(list) => list,
                None => return 1,
            }
        }
        None => return bad("no terminal: pass --agent claude,omp", install_usage()),
    };

    // omp loads `.claude/skills` as well as `.omp/skills` (its `claude` provider), so an omp plugin
    // collides with any copy; Claude Code loads only `.claude/skills`.
    let relevant: Vec<Install> = installs
        .into_iter()
        .filter(|i| i.agent.name == "omp" || selected.iter().any(|a| std::ptr::eq(*a, i.agent)))
        .collect();
    if !relevant.is_empty() && remove_plugin(&relevant, &root, yes).is_err() {
        return 1;
    }

    let version = install::skill_version(install::skill_md()).unwrap_or("unknown");
    let mut changed = false;
    for agent in selected {
        let rel = format!("{}/{}", agent.skills_dir, install::SKILL_NAME);
        let name = agent.name;
        match install::write_skill(&root.join(agent.skills_dir).join(install::SKILL_NAME)) {
            Ok(Outcome::Installed) => {
                changed = true;
                say!("ok {name} {rel} installed {version}");
            }
            Ok(Outcome::Updated { from }) => {
                changed = true;
                if from == version {
                    say!("ok {name} {rel} updated {version} (files changed)");
                } else {
                    say!("ok {name} {rel} updated {from} -> {version}");
                }
            }
            Ok(Outcome::UpToDate) => say!("ok {name} {rel} up to date {version}"),
            Ok(Outcome::Symlink) => say!("ok {name} {rel} skipped: symlink"),
            Err(e) => {
                say!("err {name} {rel}: {e}");
                return 1;
            }
        }
    }
    if changed {
        say!("restart the agent session to load the skill");
    }
    0
}

/// Asks (unless `yes`) and removes every install, then checks the agents
/// no longer list the plugin. Errors are printed.
fn remove_plugin(installs: &[Install], root: &Path, yes: bool) -> Result<(), ()> {
    say!("the iced-impeccable plugin is installed and would load next to the repo copy:");
    for install in installs {
        say!("  {} {}", install.agent.name, install.scope);
    }
    if !yes {
        if !io::stdin().is_terminal() {
            say!("err aborted: rerun with --yes to remove it");
            return Err(());
        }
        let answer = prompt("remove it? [y/N] ").unwrap_or_default();
        if !(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")) {
            say!("err aborted");
            return Err(());
        }
    }
    for install in installs {
        if let Err(e) = install::uninstall(install, root) {
            say!("err {e}");
            return Err(());
        }
        say!("removed {} {}", install.agent.name, install.scope);
    }
    for agent in AGENTS.iter() {
        if !installs.iter().any(|i| std::ptr::eq(i.agent, agent)) {
            continue;
        }
        match install::list_installs(agent, root) {
            Ok(Some(scopes)) if !scopes.is_empty() => {
                let scope = scopes[0].clone();
                let command = install::uninstall_args(&Install {
                    agent,
                    scope: scope.clone(),
                });
                say!(
                    "err {} still lists the plugin ({scope}); remove it with: {} {}",
                    agent.cli,
                    agent.cli,
                    command.join(" ")
                );
                return Err(());
            }
            Ok(_) => {}
            Err(e) => {
                say!("err {e}");
                return Err(());
            }
        }
    }
    Ok(())
}

/// The toggle prompt; `None` (printed) when the user quits or ticks
/// nothing.
fn select_agents(root: &Path, ticked: &mut [bool]) -> Option<Vec<&'static Agent>> {
    loop {
        say!(
            "install the iced-impeccable skill into {} for:",
            root.display()
        );
        for (i, (agent, on)) in AGENTS.iter().zip(ticked.iter()).enumerate() {
            say!(
                "  [{}] {} {:<8}{}/{}",
                if *on { 'x' } else { ' ' },
                i + 1,
                agent.name,
                agent.skills_dir,
                install::SKILL_NAME
            );
        }
        let Some(input) = prompt("number toggles, Enter installs, q quits: ") else {
            say!("err aborted");
            return None;
        };
        match input.as_str() {
            "" if ticked.contains(&true) => {
                return Some(
                    AGENTS
                        .iter()
                        .zip(ticked.iter())
                        .filter(|(_, on)| **on)
                        .map(|(agent, _)| agent)
                        .collect(),
                );
            }
            "" => {
                say!("err nothing selected");
                return None;
            }
            "q" => {
                say!("err aborted");
                return None;
            }
            _ => match input.parse::<usize>() {
                Ok(n) if (1..=ticked.len()).contains(&n) => ticked[n - 1] = !ticked[n - 1],
                _ => say!("unknown choice {input:?}"),
            },
        }
    }
}

/// Prints `question`, reads one trimmed line; `None` on EOF or error.
fn prompt(question: &str) -> Option<String> {
    print!("{question}");
    io::stdout().flush().ok()?;
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_owned()),
    }
}

fn bad(message: &str, usage: &str) -> i32 {
    eprintln!("{message}");
    eprintln!("{usage}");
    2
}
