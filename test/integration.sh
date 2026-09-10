#!/usr/bin/env bash
# Black-box tests that GitHub Actions can run: real `nix profile` against a
# throwaway profile (NXF_PROFILE is test-only), no VM, no systemd user session. Full systemd/os
# coverage stays in test/vm-test.sh (local incus).
set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TEST_FLAKE=$ROOT/test

if [[ -z "${NXF:-}" ]]; then
  NXF=$(nix build --no-link --print-out-paths "$ROOT#nxf")/bin/nxf
fi

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

export HOME=$WORKDIR/home
export XDG_STATE_HOME=$WORKDIR/state
export XDG_CONFIG_HOME=$WORKDIR/config
export XDG_DATA_HOME=$WORKDIR/data
export NXF_PROFILE=$WORKDIR/profile
export NXF_NO_NOM=1
# Isolate from the caller's systemd --user session (if any) so this suite
# exercises the documented no-session path, which is also what CI has.
unset DBUS_SESSION_BUS_ADDRESS
unset XDG_RUNTIME_DIR
mkdir -p "$HOME" "$XDG_STATE_HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { FAIL=$((FAIL + 1)); printf '  \033[31mFAIL\033[0m %s\n' "$1"; }
section() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }

assert_rc() {
  local rc=$1 want=$2 desc=$3
  if [[ "$rc" == "$want" ]]; then pass "$desc"; else fail "$desc (rc=$rc, want $want)"; fi
}

assert_contains() {
  local haystack=$1 needle=$2 desc=$3
  if [[ "$haystack" == *"$needle"* ]]; then pass "$desc"; else
    fail "$desc (expected to find: $needle)"
    printf '%s\n' "$haystack" | sed 's/^/    | /'
  fi
}

assert_not_contains() {
  local haystack=$1 needle=$2 desc=$3
  if [[ "$haystack" != *"$needle"* ]]; then pass "$desc"; else
    fail "$desc (did not expect: $needle)"
  fi
}

assert_exists() {
  local desc=$1; shift
  if test "$@"; then pass "$desc"; else fail "$desc"; fi
}

assert_not_exists() {
  local desc=$1; shift
  if ! test "$@"; then pass "$desc"; else fail "$desc"; fi
}

nxf() { "$NXF" "$@"; }

section "baseline"
out=$(nxf profile list 2>&1); rc=$?
assert_rc "$rc" 0 "profile list on empty profile exits 0"
assert_contains "$out" "no nxf-aware profiles applied" "empty list message"

out=$(nxf profile remove never-added --approve 2>&1); rc=$?
assert_rc "$rc" 1 "remove never-added fails"
assert_contains "$out" "not installed" "remove never-added says not installed"

out=$(nxf profile add "$TEST_FLAKE#../oops" --approve 2>&1); rc=$?
assert_rc "$rc" 1 "invalid profile name is rejected"

section "pkgs-only"
out=$(nxf profile add --approve "$TEST_FLAKE#pkgs-only" 2>&1); rc=$?
assert_rc "$rc" 0 "add pkgs-only exits 0"
assert_contains "$out" "evaluate / build" "add prints evaluate/build phase"
assert_contains "$out" "plan" "add prints plan phase"
assert_contains "$out" "activate" "add prints activate phase"
assert_exists "hello landed in the throwaway profile" -x "$NXF_PROFILE/bin/hello"
out=$(nxf profile list 2>&1)
assert_contains "$out" "pkgs-only" "list shows pkgs-only"

section "dry-run"
out=$(nxf profile add --dry-run "$TEST_FLAKE#gui.daily" 2>&1); rc=$?
assert_rc "$rc" 0 "dry-run add exits 0"
assert_contains "$out" "Dry run: not applying" "dry-run does not apply"
out=$(nxf profile list 2>&1)
assert_not_contains "$out" "gui.daily" "dry-run did not install gui.daily"

section "dotted name"
out=$(nxf profile add --approve "$TEST_FLAKE#gui.daily" 2>&1); rc=$?
assert_rc "$rc" 0 "add gui.daily exits 0"
out=$(nxf profile list 2>&1)
assert_contains "$out" "gui.daily" "list shows gui.daily (not nix's 'daily')"
nixlist=$(nix profile list --json --profile "$NXF_PROFILE" 2>/dev/null || true)
assert_contains "$nixlist" '"gui.daily"' "nix profile list element key is gui.daily"
assert_not_contains "$nixlist" '"daily"' "nix profile list has no colliding 'daily' key"
out=$(nxf profile remove --approve gui.daily 2>&1); rc=$?
assert_rc "$rc" 0 "remove gui.daily by nxf name exits 0"
out=$(nxf profile list 2>&1)
assert_not_contains "$out" "gui.daily" "gui.daily gone after remove"

section "with-unit (no session: files still installed)"
out=$(nxf profile add --approve "$TEST_FLAKE#with-unit" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-unit exits 0"
assert_contains "$out" "no systemd user session available" "warns instead of failing without a session"
assert_exists "unit file installed" -e "$XDG_CONFIG_HOME/systemd/user/nxf-with-unit-greeter.service"
out=$(nxf profile list -v 2>&1)
assert_contains "$out" "greeter.service" "verbose list shows greeter.service"
out=$(nxf profile remove --approve with-unit 2>&1); rc=$?
assert_rc "$rc" 0 "remove with-unit exits 0"
assert_not_exists "unit file removed" -e "$XDG_CONFIG_HOME/systemd/user/nxf-with-unit-greeter.service"

section "activate / deactivate"
out=$(nxf profile add --approve "$TEST_FLAKE#with-hooks" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-hooks exits 0"
assert_contains "$(cat "$HOME/hooks.log" 2>/dev/null)" "activate with-hooks" "activate hook ran"
out=$(nxf profile remove --approve with-hooks 2>&1); rc=$?
assert_rc "$rc" 0 "remove with-hooks exits 0"
assert_contains "$(cat "$HOME/hooks.log" 2>/dev/null)" "deactivate with-hooks" "deactivate hook ran"

section "cleanup leftover pkgs-only"
nxf profile remove --approve pkgs-only >/dev/null 2>&1 || true
out=$(nxf profile list 2>&1)
assert_contains "$out" "no nxf-aware profiles applied" "profile empty at end"

section "summary"
echo "PASS=$PASS FAIL=$FAIL"
[[ $FAIL -eq 0 ]]
