//! The crate, the skill and the plugin manifests carry one version
//! (RELEASING.md step 2). The manifests are not in the published package, so
//! they are only checked where they exist.

use std::path::Path;

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).ok()
}

#[test]
fn skill_and_manifests_match_the_crate_version() {
    let cargo = env!("CARGO_PKG_VERSION");

    let skill =
        read("plugin/skills/iced-impeccable/SKILL.md").expect("the skill ships with the crate");
    let version = skill
        .lines()
        .skip(1)
        .take_while(|line| *line != "---")
        .find_map(|line| line.strip_prefix("version:"))
        .map(|v| v.trim().trim_matches(['"', '\'']));
    assert_eq!(version, Some(cargo), "SKILL.md frontmatter version");

    if let Some(json) = read("plugin/.claude-plugin/plugin.json") {
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(json["version"].as_str(), Some(cargo), "plugin.json version");
    }

    if let Some(json) = read(".claude-plugin/marketplace.json") {
        let json: serde_json::Value = serde_json::from_str(&json).unwrap();
        let plugin = json["plugins"]
            .as_array()
            .and_then(|plugins| {
                plugins
                    .iter()
                    .find(|p| p["name"].as_str() == Some("iced-impeccable"))
            })
            .expect("marketplace.json lists iced-impeccable");
        assert_eq!(
            plugin["version"].as_str(),
            Some(cargo),
            "marketplace.json version"
        );
    }
}
