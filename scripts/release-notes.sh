#!/usr/bin/env bash
# Release notes of one version: its CHANGELOG.md section without the heading.
# Fails when the heading (with its date), the link reference or the content
# is missing.
#
#   scripts/release-notes.sh X.Y.Z
set -euo pipefail

version=${1:?usage: scripts/release-notes.sh X.Y.Z}
cd "$(dirname "$0")/.."

fail() {
    echo "release-notes: $*" >&2
    exit 1
}

grep -qE "^## \[${version//./\\.}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}\$" CHANGELOG.md \
    || fail "CHANGELOG.md has no '## [$version] - YYYY-MM-DD' heading"
grep -qxF "[$version]: https://github.com/griasdi/iced_impeccable/releases/tag/v$version" CHANGELOG.md \
    || fail "CHANGELOG.md has no '[$version]: …/releases/tag/v$version' link"

notes=$(awk -v head="## [$version] - " '
    index($0, head) == 1 { found = 1; next }
    found && (/^## \[/ || /^\[[^]]+\]: /) { exit }
    found { print }
' CHANGELOG.md)
# Command substitution dropped the trailing blank lines; drop the leading ones.
notes=${notes#"${notes%%[!$'\n']*}"}
[[ -n $notes ]] || fail "the [$version] section of CHANGELOG.md is empty"
printf '%s\n' "$notes"
