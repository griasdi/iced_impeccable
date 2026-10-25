#!/usr/bin/env bash
# End-to-end smoke test: runs the demo headless and drives it through `ctl`,
# checking the first reply line of every command.
#
#   scripts/smoke.sh [tiny-skia|wgpu]    default tiny-skia: needs no GPU
#
# Runs under Git Bash on Windows too, where the control channel is a named
# pipe instead of a socket file.
set -euo pipefail

backend=${1:-tiny-skia}
cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}
cargo build -q --example demo --example multiwin --bin iced-impeccable

dir=$(mktemp -d)
if [[ ${OS:-} == Windows_NT ]]; then
    sock=iced-impeccable-smoke-$$
    sock2=iced-impeccable-smoke-multiwin-$$
else
    sock=$dir/demo.sock
    sock2=$dir/multiwin.sock
fi
"$target/debug/examples/demo" --headless --control "$sock" --size 1100x720 --clipboard 'from start' \
    --backend "$backend" 2>"$dir/host.log" &
host=$!
host2=
trap 'kill "$host" $host2 2>/dev/null || true; rm -rf "$dir"' EXIT

failures=0
# expect PATTERN COMMAND...: the reply's first line must match PATTERN.
expect() {
    local pattern=$1
    shift
    local reply
    reply=$("$target/debug/iced-impeccable" ctl "$sock" "$@" || true)
    if grep -qE -- "$pattern" <<<"${reply%%$'\n'*}"; then
        echo "ok    $*"
    else
        echo "FAIL  $*"
        echo "      expected /$pattern/, got: ${reply%%$'\n'*}"
        failures=$((failures + 1))
    fi
}

expect "^ok pid [0-9]+ app demo size 1100x720 scale 1 appearance dark backend " info
# A second host on the same socket must refuse to start and leave the first alone.
if [[ ${OS:-} != Windows_NT ]]; then
    "$target/debug/examples/demo" --headless --control "$sock" --backend "$backend" 2>"$dir/dup.log" &
    dup=$!
    for _ in $(seq 50); do
        kill -0 "$dup" 2>/dev/null || break
        sleep 0.1
    done
    if kill -0 "$dup" 2>/dev/null; then
        kill "$dup" 2>/dev/null || true
        wait "$dup" 2>/dev/null || true
        echo "FAIL  second host on a live socket kept running"
        failures=$((failures + 1))
    else
        dup_status=0
        wait "$dup" || dup_status=$?
        if [[ $dup_status == 1 ]] && grep -q 'another host is listening' "$dir/dup.log"; then
            echo "ok    second host refused"
        else
            echo "FAIL  second host: status $dup_status, log: $(cat "$dir/dup.log")"
            failures=$((failures + 1))
        fi
    fi
    expect "^ok pid" info
fi
expect "^ok from start$" clip
expect "^ok [0-9]+$" tree
[[ $("$target/debug/iced-impeccable" ctl "$sock" tree) == *'hidden text="Row 60"'* ]] \
    || { echo "FAIL  tree lists Row 60 as hidden"; failures=$((failures + 1)); }
expect "^err ambiguous: 2 matches \(use --nth\)" find Save
expect "^ok [0-9]+ [0-9]+$" tap '#name'
expect "^ok$" type -e 'Ada\n'
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" wait-for Saved Ada
expect "^ok [0-9]+ [0-9]+$" tap --nth 2 Save
expect "^ok [0-9]+ [0-9]+$" hover '#name'
# `=` is exact text, `#` an id: the demo has a text "#rows" and a scrollable id "rows".
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" find '=#rows'
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" find '#rows'
expect "^err not visible: " find Row 60
expect '^err not found: Sav; similar: "Save"' find Sav
expect "^ok [0-9]+ [0-9]+$" tap '#settings'
expect "^ok idle$" wait-idle
expect "^ok [0-9]+ [0-9]+$" tap Copy
expect "^ok Ada$" clip
expect "^ok [0-9]+ [0-9]+$" tap Spinner
expect "^ok animating$" wait-idle
expect "^err timeout$" wait-idle --timeout 300
expect "^ok [0-9]+ [0-9]+$" tap Spinner
expect "^ok$" key left
expect '^err unknown key "nokey"; keys: ' key nokey
expect "^ok$" reset
expect "^ok$" wait-gone Saved Ada
# A shortcut matched by physical key, and modifiers held across commands.
expect "^ok$" key ctrl+k
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" wait-for Hotkey
expect "^ok$" keydown ctrl+a
expect "^ok$" keydown shift+b
expect "^ok$" keyup shift+b
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" wait-for 'Mods ctrl'
expect "^ok$" keyup ctrl+a
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" wait-for 'Mods none'
expect "^ok$" clip-set -e 'a\nb'
expect "^ok a$" clip
expect "^ok$" drop "$dir/notes.txt"
expect "^ok [0-9]+ [0-9]+ [0-9]+ [0-9]+$" wait-for Dropped notes.txt
expect '^err unknown command "palette" \(see help\)$' palette
expect "^ok$" appearance light
expect "^ok .*/light.png 1100x720 scale 1$" screenshot --annotate "$dir/light.png"
expect "^ok .*/np.png 1100x720 scale 1$" screenshot --no-pointer "$dir/np.png"
expect "^ok .*/zoom.png 800x320 scale 1$" screenshot --crop 0 0 200 80 --zoom 4 "$dir/zoom.png"
expect "^ok 1000 700$" resize 1000 700
expect "^ok 1100 720$" resize 1100 720
expect "^err too large: physical 16384x16384 exceeds 8192$" resize 16384 16384
if command -v ffmpeg >/dev/null; then
    expect "^ok recording " record "$dir/v.mp4"
    expect "^ok .*v.mp4 [0-9]+ frames" record-stop
else
    expect "^err ffmpeg" record "$dir/v.mp4"
fi
expect "^ok$" scale 2
expect "^ok .*/hidpi.png 2200x1440 scale 2$" screenshot "$dir/hidpi.png"
expect "^ok 1100 720 2$" size
expect "^ok$" quit

wait "$host" || { echo "FAIL  host exit status $?"; failures=$((failures + 1)); }
grep -q 'demo: state dropped' "$dir/host.log" \
    || { echo "FAIL  host dropped the app state at exit"; failures=$((failures + 1)); }
sheet=$("$target/debug/iced-impeccable" sheet "$dir/sheet.png" "$dir/light.png" "$dir/hidpi.png" || true)
[[ $sheet =~ ^ok\ .*/sheet.png\ 2560x874$ ]] && echo "ok    sheet" \
    || { echo "FAIL  sheet: $sheet"; failures=$((failures + 1)); }

# Skill install into a repo, with an empty PATH so no agent CLI (and no
# installed plugin of the developer) is touched.
mkdir -p "$dir/repo/.git"
skill_install() { PATH= "$target/debug/iced-impeccable" install --agent claude,omp "$@" 2>&1 || true; }
out=$(skill_install "$dir/repo")
[[ $out == *'ok claude .claude/skills/iced-impeccable installed '* \
    && $out == *'ok omp .omp/skills/iced-impeccable installed '* ]] && echo "ok    install" \
    || { echo "FAIL  install: $out"; failures=$((failures + 1)); }
out=$(skill_install "$dir/repo")
[[ $out == *'ok claude .claude/skills/iced-impeccable up to date '* \
    && $out == *'ok omp .omp/skills/iced-impeccable up to date '* ]] && echo "ok    install again" \
    || { echo "FAIL  install again: $out"; failures=$((failures + 1)); }
if out=$(PATH= "$target/debug/iced-impeccable" install --agent claude "$dir" 2>&1); then
    echo "FAIL  install outside a repo succeeded: $out"; failures=$((failures + 1))
elif [[ $out == 'err '*' is not inside a git repository' ]]; then
    echo "ok    install outside a repo"
else
    echo "FAIL  install outside a repo: $out"; failures=$((failures + 1))
fi
if [[ ${OS:-} != Windows_NT ]]; then
    # Stub agent CLIs that list the plugin until it is uninstalled: the agent
    # selection comes before any removal, and only relevant installs go.
    mkdir -p "$dir/bin" "$dir/repo2/.git"
    for cli in claude omp; do
        cat >"$dir/bin/$cli" <<'STUB'
#!/usr/bin/env bash
cli=$(basename "$0")
case "$1 $2" in
"plugin list")
    if [[ -e $STUB_DIR/$cli.removed ]]; then
        [[ $cli == claude ]] && echo '[]' || echo '{"npm":[],"marketplace":[]}'
    elif [[ $cli == claude ]]; then
        echo '[{"id":"iced-impeccable@iced-impeccable","scope":"user"}]'
    else
        echo '{"npm":[],"marketplace":[{"id":"iced-impeccable@iced-impeccable","scope":"project","entries":[{"scope":"project"}]}]}'
    fi
    ;;
"plugin uninstall")
    echo "$cli $*" >>"$STUB_DIR/uninstalls"
    touch "$STUB_DIR/$cli.removed"
    ;;
esac
STUB
        chmod +x "$dir/bin/$cli"
    done
    stub_install() {
        STUB_DIR=$dir PATH="$dir/bin:/usr/bin:/bin" "$target/debug/iced-impeccable" install "$@" \
            </dev/null 2>&1 && code=0 || code=$?
    }
    out=$(stub_install --yes "$dir/repo2"; echo "exit $code")
    [[ $out == *'no terminal: pass --agent claude,omp'* && $out == *'exit 2' && ! -e $dir/uninstalls ]] \
        && echo "ok    install without agents removes nothing" \
        || { echo "FAIL  install without agents: $out"; failures=$((failures + 1)); }
    out=$(stub_install --agent omp --yes "$dir/repo2"; echo "exit $code")
    [[ $out == *'ok omp .omp/skills/iced-impeccable installed'* && $out == *'exit 0' \
        && $(<"$dir/uninstalls") == 'omp plugin uninstall iced-impeccable@iced-impeccable --scope=project' ]] \
        && echo "ok    install removes the omp plugin only" \
        || { echo "FAIL  install --agent omp: $out"; failures=$((failures + 1)); }
    out=$(stub_install --agent claude --yes "$dir/repo2"; echo "exit $code")
    [[ $out == *'ok claude .claude/skills/iced-impeccable installed'* && $out == *'exit 0' \
        && $(<"$dir/uninstalls") == 'omp plugin uninstall iced-impeccable@iced-impeccable --scope=project
claude plugin uninstall iced-impeccable@iced-impeccable --scope user' ]] \
        && echo "ok    install removes the claude plugin for claude" \
        || { echo "FAIL  install --agent claude: $out"; failures=$((failures + 1)); }
fi
[[ ! -e $sock ]] || { echo "FAIL  socket left behind"; failures=$((failures + 1)); }

# A daemon with two windows: `windows` lists them, `@N` addresses one.
"$target/debug/examples/multiwin" --headless --control "$sock2" --size 640x480 \
    --backend "$backend" 2>>"$dir/host.log" &
host2=$!
sock=$sock2
expect "^ok 1$" windows
expect "^ok" tap 'Open second'
expect "^ok 2$" windows
expect "^ok" @2 find second
expect "^ok .* 320x240" @2 screenshot "$dir/second.png"
expect "^ok" @2 tap Close
expect "^ok 1$" windows
expect "^err no window 2" @2 tree
expect "^ok" quit
wait "$host2" || { echo "FAIL  multiwin exit status $?"; failures=$((failures + 1)); }
host2=

if ((failures > 0)); then
    echo "$failures failure(s); host log:"
    cat "$dir/host.log"
    exit 1
fi
echo "smoke ($backend): all passed"
