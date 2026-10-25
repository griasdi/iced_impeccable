//! `iced-impeccable install`: the skill, embedded in the binary, copied into
//! a git repository for Claude Code and omp, and the marketplace plugin
//! detected and removed through the agents' own CLIs.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

pub(crate) const PLUGIN_ID: &str = "iced-impeccable@iced-impeccable";
pub(crate) const SKILL_NAME: &str = "iced-impeccable";

macro_rules! skill_file {
    ($path:literal) => {
        (
            $path,
            include_str!(concat!("../plugin/skills/iced-impeccable/", $path)),
        )
    };
}

/// The skill files: path relative to the skill dir (`/`-separated) and
/// contents.
pub(crate) const SKILL: &[(&str, &str)] = &[
    skill_file!("SKILL.md"),
    skill_file!("reference/adapt.iced.md"),
    skill_file!("reference/audit.iced.md"),
    skill_file!("reference/design-md.iced.md"),
    skill_file!("reference/iced.md"),
    skill_file!("reference/variants.iced.md"),
    skill_file!("reference/verify.md"),
];

/// An agent the skill installs for.
pub(crate) struct Agent {
    /// Name on the command line and in output.
    pub name: &'static str,
    /// Its CLI, for `plugin list` and `plugin uninstall`.
    pub cli: &'static str,
    /// Project skills directory, relative to the repo root.
    pub skills_dir: &'static str,
}

pub(crate) static AGENTS: [Agent; 2] = [
    Agent {
        name: "claude",
        cli: "claude",
        skills_dir: ".claude/skills",
    },
    Agent {
        name: "omp",
        cli: "omp",
        skills_dir: ".omp/skills",
    },
];

/// The embedded `SKILL.md`.
pub(crate) fn skill_md() -> &'static str {
    SKILL[0].1
}

/// The repository root containing `start`: the first ancestor (inclusive)
/// with a `.git` entry, file or directory, so worktrees count.
pub(crate) fn repo_root(start: &Path) -> Result<PathBuf, String> {
    let path = fs::canonicalize(start).map_err(|e| format!("{}: {e}", start.display()))?;
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{} is not inside a git repository", start.display()))
}

/// The `version:` value of a skill's frontmatter (the block between the
/// first line `---` and the next one); `None` without frontmatter or value.
pub(crate) fn skill_version(skill_md: &str) -> Option<&str> {
    let mut lines = skill_md.lines();
    if lines.next()? != "---" {
        return None;
    }
    lines
        .take_while(|line| *line != "---")
        .find_map(|line| line.strip_prefix("version:"))
        .map(|value| value.trim().trim_matches(|c| c == '"' || c == '\''))
        .filter(|value| !value.is_empty())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Installed,
    Updated { from: String },
    UpToDate,
    Symlink,
}

/// A sibling of `dir` to build or park a copy in; never inside it.
fn staging(dir: &Path, tag: &str) -> PathBuf {
    dir.with_file_name(format!(".{SKILL_NAME}.{tag}-{}", std::process::id()))
}

/// Writes the skill into `dir` (`<root>/<skills_dir>/iced-impeccable`),
/// replacing a stale copy; a symlinked `dir` stays untouched. The new copy
/// is built next to `dir` and swapped in, so a failure leaves the old one.
pub(crate) fn write_skill(dir: &Path) -> io::Result<Outcome> {
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let new = staging(dir, "new");
            write_staged(&new)?;
            if let Err(e) = fs::rename(&new, dir) {
                let _ = fs::remove_dir_all(&new);
                return Err(e);
            }
            Ok(Outcome::Installed)
        }
        Err(e) => Err(e),
        Ok(meta) if meta.file_type().is_symlink() => Ok(Outcome::Symlink),
        Ok(_) => {
            let mut existing = BTreeMap::new();
            collect_files(dir, "", &mut existing)?;
            let same = existing.len() == SKILL.len()
                && SKILL.iter().all(|(path, contents)| {
                    existing
                        .get(*path)
                        .is_some_and(|file| fs::read(file).is_ok_and(|b| b == contents.as_bytes()))
                });
            if same {
                return Ok(Outcome::UpToDate);
            }
            let from = fs::read_to_string(dir.join("SKILL.md"))
                .ok()
                .and_then(|md| skill_version(&md).map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            let new = staging(dir, "new");
            let old = staging(dir, "old");
            write_staged(&new)?;
            let _ = fs::remove_dir_all(&old);
            if let Err(e) = fs::rename(dir, &old) {
                let _ = fs::remove_dir_all(&new);
                return Err(e);
            }
            if let Err(e) = fs::rename(&new, dir) {
                let _ = fs::rename(&old, dir);
                let _ = fs::remove_dir_all(&new);
                return Err(e);
            }
            let _ = fs::remove_dir_all(&old);
            Ok(Outcome::Updated { from })
        }
    }
}

/// Writes the skill into `new` after clearing a leftover; a failed write
/// removes the partial copy.
fn write_staged(new: &Path) -> io::Result<()> {
    let _ = fs::remove_dir_all(new);
    write_files(new).inspect_err(|_| {
        let _ = fs::remove_dir_all(new);
    })
}

fn write_files(dir: &Path) -> io::Result<()> {
    for (path, contents) in SKILL {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, contents)?;
    }
    Ok(())
}

/// Every file below `dir`, keyed by its `/`-joined path relative to the
/// walk's start.
fn collect_files(dir: &Path, prefix: &str, out: &mut BTreeMap<String, PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type()?.is_dir() {
            collect_files(&entry.path(), &rel, out)?;
        } else {
            out.insert(rel, entry.path());
        }
    }
    Ok(())
}

/// A marketplace install of the plugin.
pub(crate) struct Install {
    pub agent: &'static Agent,
    pub scope: String,
}

/// The scopes of `plugin list --json` output that load the plugin in
/// `root` (canonical), deduplicated, in listing order.
pub(crate) fn parse_installs(
    agent: &Agent,
    json: &str,
    root: &Path,
) -> Result<Vec<String>, String> {
    let unexpected = || format!("unexpected {} plugin list output", agent.cli);
    let value: Value = serde_json::from_str(json).map_err(|_| unexpected())?;
    let in_root = |path: Option<&Value>| {
        path.and_then(Value::as_str)
            .is_some_and(|p| fs::canonicalize(p).is_ok_and(|p| p == root))
    };
    let mut scopes: Vec<String> = Vec::new();
    let mut add = |scope: &str| {
        if !scopes.iter().any(|s| s == scope) {
            scopes.push(scope.to_owned());
        }
    };
    let ours = |item: &Value| item.get("id").and_then(Value::as_str) == Some(PLUGIN_ID);
    match agent.name {
        "claude" => {
            for item in value
                .as_array()
                .ok_or_else(unexpected)?
                .iter()
                .filter(|i| ours(i))
            {
                let scope = item
                    .get("scope")
                    .and_then(Value::as_str)
                    .ok_or_else(unexpected)?;
                if scope == "user" || in_root(item.get("projectPath")) {
                    add(scope);
                }
            }
        }
        "omp" => {
            let items = value
                .get("marketplace")
                .and_then(Value::as_array)
                .ok_or_else(unexpected)?;
            for item in items.iter().filter(|i| ours(i)) {
                let item_scope = item.get("scope").and_then(Value::as_str);
                let entries = item
                    .get("entries")
                    .and_then(Value::as_array)
                    .ok_or_else(unexpected)?;
                for entry in entries {
                    let scope = entry
                        .get("scope")
                        .and_then(Value::as_str)
                        .or(item_scope)
                        .ok_or_else(unexpected)?;
                    let path = entry.get("projectPath");
                    if scope == "user" || path.is_none() || in_root(path) {
                        add(scope);
                    }
                }
            }
        }
        _ => return Err(unexpected()),
    }
    Ok(scopes)
}

/// The plugin's install scopes relevant to `root`, or `None` when the
/// agent's CLI is not present.
pub(crate) fn list_installs(agent: &Agent, root: &Path) -> Result<Option<Vec<String>>, String> {
    let output = match Command::new(agent.cli)
        .args(["plugin", "list", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", agent.cli)),
    };
    if !output.status.success() {
        return Err(format!(
            "{} plugin list failed: {}",
            agent.cli,
            failure_text(&output)
        ));
    }
    let json = String::from_utf8_lossy(&output.stdout);
    parse_installs(agent, &json, root).map(Some)
}

/// The arguments that remove `install`.
pub(crate) fn uninstall_args(install: &Install) -> Vec<String> {
    let mut args = vec![
        "plugin".to_owned(),
        "uninstall".to_owned(),
        PLUGIN_ID.to_owned(),
    ];
    match install.agent.name {
        "omp" => args.push(format!("--scope={}", install.scope)),
        _ => args.extend(["--scope".to_owned(), install.scope.clone()]),
    }
    args
}

pub(crate) fn uninstall(install: &Install, root: &Path) -> Result<(), String> {
    let cli = install.agent.cli;
    let output = Command::new(cli)
        .args(uninstall_args(install))
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{cli}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{cli} plugin uninstall failed: {}",
            failure_text(&output)
        ));
    }
    Ok(())
}

/// Trimmed stderr, or stdout when stderr is empty.
fn failure_text(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        stderr
    };
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "iced-impeccable-install-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn embedded_skill_is_complete() {
        let mut on_disk = BTreeMap::new();
        collect_files(
            Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/plugin/skills/iced-impeccable"
            )),
            "",
            &mut on_disk,
        )
        .unwrap();
        let mut embedded: Vec<&str> = SKILL.iter().map(|(path, _)| *path).collect();
        embedded.sort_unstable();
        assert_eq!(
            on_disk.keys().map(String::as_str).collect::<Vec<_>>(),
            embedded
        );
    }

    #[test]
    fn skill_version_reads_only_the_frontmatter() {
        assert_eq!(
            skill_version("---\nname: x\nversion: 1.2.3\n---\n"),
            Some("1.2.3")
        );
        assert_eq!(
            skill_version("---\nversion: \"1.2.3\"\n---\n"),
            Some("1.2.3")
        );
        assert_eq!(skill_version("---\nversion: '1.2.3'\n---\n"), Some("1.2.3"));
        assert_eq!(skill_version("---\nname: x\n---\nversion: 1.2.3\n"), None);
        assert_eq!(skill_version("---\nversion:\n---\n"), None);
        assert_eq!(skill_version("name: x\nversion: 1.2.3\n"), None);
        assert_eq!(skill_version(""), None);
    }

    #[test]
    fn write_skill_installs_updates_and_skips_symlinks() {
        let base = temp_dir("write");
        let dir = base.join(SKILL_NAME);
        assert_eq!(write_skill(&dir).unwrap(), Outcome::Installed);
        for (path, contents) in SKILL {
            assert_eq!(fs::read_to_string(dir.join(path)).unwrap(), *contents);
        }
        assert_eq!(write_skill(&dir).unwrap(), Outcome::UpToDate);

        fs::write(dir.join("SKILL.md"), "---\nversion: 0.0.1\n---\n").unwrap();
        fs::write(dir.join("reference/old.md"), "stale").unwrap();
        assert_eq!(
            write_skill(&dir).unwrap(),
            Outcome::Updated {
                from: "0.0.1".to_owned()
            }
        );
        assert!(!dir.join("reference/old.md").exists());
        for (path, contents) in SKILL {
            assert_eq!(fs::read_to_string(dir.join(path)).unwrap(), *contents);
        }

        // A stray file alone is enough to update.
        fs::write(dir.join("extra.md"), "stale").unwrap();
        assert!(matches!(
            write_skill(&dir).unwrap(),
            Outcome::Updated { .. }
        ));
        assert!(!dir.join("extra.md").exists());
        let leftovers: Vec<_> = fs::read_dir(&base)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".iced-impeccable."))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        #[cfg(unix)]
        {
            let target = base.join("checkout");
            fs::create_dir_all(&target).unwrap();
            fs::write(target.join("SKILL.md"), "dev").unwrap();
            let link = base.join("linked");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert_eq!(write_skill(&link).unwrap(), Outcome::Symlink);
            assert_eq!(fs::read_to_string(target.join("SKILL.md")).unwrap(), "dev");
            assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
        }
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn parse_installs_keeps_scopes_that_load_in_the_repo() {
        let base = temp_dir("parse");
        let root = fs::canonicalize(&base).unwrap();
        let other = temp_dir("parse-other");
        let claude = format!(
            r#"[
                {{"id": "{PLUGIN_ID}", "scope": "local", "projectPath": "{other}"}},
                {{"id": "other@market", "scope": "user"}},
                {{"id": "{PLUGIN_ID}", "scope": "user"}},
                {{"id": "{PLUGIN_ID}", "scope": "project", "projectPath": "{root}"}},
                {{"id": "{PLUGIN_ID}", "scope": "user"}}
            ]"#,
            other = other.display(),
            root = root.display(),
        );
        assert_eq!(
            parse_installs(&AGENTS[0], &claude, &root).unwrap(),
            ["user", "project"]
        );

        let omp = format!(
            r#"{{"npm": [{{"id": "{PLUGIN_ID}"}}], "marketplace": [
                {{"id": "{PLUGIN_ID}", "scope": "user", "entries": [
                    {{"scope": "user", "installPath": "/x"}},
                    {{"scope": "project", "installPath": "/y"}},
                    {{"scope": "project", "projectPath": "{other}"}}
                ]}},
                {{"id": "other@market", "scope": "user", "entries": [{{"scope": "user"}}]}}
            ]}}"#,
            other = other.display(),
        );
        assert_eq!(
            parse_installs(&AGENTS[1], &omp, &root).unwrap(),
            ["user", "project"]
        );
        assert_eq!(
            parse_installs(&AGENTS[1], r#"{"npm": [], "marketplace": []}"#, &root).unwrap(),
            Vec::<String>::new()
        );

        assert!(parse_installs(&AGENTS[0], "not json", &root).is_err());
        assert!(parse_installs(&AGENTS[1], "[]", &root).is_err());
        fs::remove_dir_all(&base).unwrap();
        fs::remove_dir_all(&other).unwrap();
    }
}
