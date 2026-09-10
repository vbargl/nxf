#!/usr/bin/env bash
# Black-box test suite for nxf, run against the nxf-vm-test incus VM using
# the scratch flake at /mnt/test-profiles (see its flake.nix for what each
# profile exercises). Runs nxf exactly as a real user would - as the
# unprivileged vbargl user, under a real systemd --user session (lingering
# enabled below) so systemd-unit assertions check actual enable/start/
# restart/stop behaviour, not just that nxf gracefully skips when there's no
# session - and asserts on real command output, not just exit codes.
set -u

VM=nxf-vm-test
PROFILES_DIR=/mnt/test-profiles
HOME_DIR=/home/vbargl

# Real D-Bus user session bus, via lingering (see below) rather than an
# actual interactive login - lets systemd-unit tests exercise real
# `systemctl --user` enable/restart/disable, not just nxf's no-session skip
# path (see systemd.go's hasUserSession).
EXEC_BASE=(incus exec "$VM" --user 1000 --group 100 \
  --env "HOME=$HOME_DIR" --env "PATH=/run/current-system/sw/bin" \
  --env "XDG_RUNTIME_DIR=/run/user/1000" \
  --env "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus")

nxf() {
  local extra=()
  case "${1-} ${2-}" in
    "profile add"|"profile remove"|"profile upgrade"|"profile rollback"|"profile clean"|"os switch"|"os test"|"os boot"|"os rollback"|"os clean")
      extra+=(--approve)
      ;;
  esac
  "${EXEC_BASE[@]}" --cwd "$PROFILES_DIR" -- nxf "$@" "${extra[@]}"
}

vm() {
  "${EXEC_BASE[@]}" -- "$@"
}

PASS=0
FAIL=0
FINDINGS=()

pass() { PASS=$((PASS+1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { FAIL=$((FAIL+1)); printf '  \033[31mFAIL\033[0m %s\n' "$1"; }
section() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
finding() { FINDINGS+=("$1"); printf '  \033[33mFINDING\033[0m %s\n' "$1"; }

assert_contains() {
  local haystack=$1 needle=$2 desc=$3
  if [[ "$haystack" == *"$needle"* ]]; then pass "$desc"; else
    fail "$desc (expected to find: $needle)"; printf '%s\n' "$haystack" | sed 's/^/    | /'
  fi
}

assert_not_contains() {
  local haystack=$1 needle=$2 desc=$3
  if [[ "$haystack" != *"$needle"* ]]; then pass "$desc"; else
    fail "$desc (did not expect to find: $needle)"; printf '%s\n' "$haystack" | sed 's/^/    | /'
  fi
}

assert_rc() {
  local rc=$1 want=$2 desc=$3
  if [[ "$rc" == "$want" ]]; then pass "$desc"; else
    fail "$desc (rc=$rc, want $want)"
  fi
}

assert_exists() {
  local desc=$1; shift
  if vm test "$@" 2>/dev/null; then pass "$desc"; else fail "$desc"; fi
}

assert_not_exists() {
  local desc=$1; shift
  if ! vm test "$@" 2>/dev/null; then pass "$desc"; else fail "$desc"; fi
}

# ---- make sure vbargl has a real systemd --user session (idempotent) ------
incus exec "$VM" --env PATH=/run/current-system/sw/bin -- loginctl enable-linger vbargl >/dev/null 2>&1
for i in $(seq 1 15); do
  incus exec "$VM" --env PATH=/run/current-system/sw/bin -- test -S /run/user/1000/bus 2>/dev/null && break
  sleep 1
done

# ---- make sure the os-tests scratch flake is mounted (idempotent) ---------
if ! incus config device show nxf-vm-test 2>/dev/null | grep -q '^os-tests:'; then
  incus config device add nxf-vm-test os-tests disk \
    source=/home/vbargl/personal/nix/nxf-os-tests path=/mnt/os-tests >/dev/null
fi

# ---- reset to a known-clean baseline (best-effort, ignores errors) --------
section "reset baseline"
for p in pkgs-only with-unit with-units with-config empty; do
  vm --user 1000 --group 100 --env "HOME=$HOME_DIR" --env "PATH=/run/current-system/sw/bin" \
    nix profile remove "profile-$p" >/dev/null 2>&1
  vm --user 1000 --group 100 --env "HOME=$HOME_DIR" --env "PATH=/run/current-system/sw/bin" \
    nix profile remove "$p" >/dev/null 2>&1
done
vm rm -rf "$HOME_DIR/.local/state/nxf" "$HOME_DIR/.config/systemd/user" "$HOME_DIR/.config/nxf-demo" >/dev/null 2>&1
out=$(nxf profile list 2>&1); rc=$?
assert_rc "$rc" 0 "baseline: profile list exits 0"
assert_contains "$out" "no nxf-aware profiles applied" "baseline: no profiles applied"

# ---- pkgs-only: plain packages, first-ever add shows full diff -----------
section "pkgs-only: add"
out=$(nxf profile add "$PROFILES_DIR#pkgs-only" 2>&1); rc=$?
assert_rc "$rc" 0 "add pkgs-only exits 0"
assert_contains "$out" "cowsay" "add pkgs-only: nvd diff lists cowsay"
assert_contains "$out" "lolcat" "add pkgs-only: nvd diff lists lolcat"
assert_contains "$out" "Closure size:" "add pkgs-only: nvd prints closure size line"
out=$(nxf profile list 2>&1)
assert_contains "$out" "pkgs-only" "profile list shows pkgs-only"
assert_exists "cowsay binary landed in profile" -x "$HOME_DIR/.nix-profile/bin/cowsay"

section "pkgs-only: remove"
out=$(nxf profile remove pkgs-only 2>&1); rc=$?
assert_rc "$rc" 0 "remove pkgs-only (by short name) exits 0"
assert_contains "$out" "cowsay" "remove pkgs-only: nvd diff lists cowsay as removed"
out=$(nxf profile list 2>&1)
assert_contains "$out" "no nxf-aware profiles applied" "profile list empty after remove"
assert_not_exists "cowsay binary gone after remove" -e "$HOME_DIR/.nix-profile/bin/cowsay"

# ---- with-unit: single systemd unit, real systemd --user session ---------
section "with-unit: add"
out=$(nxf profile add "$PROFILES_DIR#with-unit" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-unit exits 0"
assert_not_contains "$out" "no systemd user session available" "add with-unit: real session present, no skip warning"
assert_contains "$out" "enabling nxf-with-unit-greeter.service" "add with-unit: reports enabling the unit"
out=$(nxf profile list 2>&1)
assert_contains "$out" "with-unit" "profile list shows with-unit"
out=$(nxf profile list -v 2>&1)
assert_contains "$out" "greeter.service" "profile list -v shows greeter unit"
assert_exists "unit symlink installed" -L "$HOME_DIR/.config/systemd/user/nxf-with-unit-greeter.service"
assert_exists "applied state recorded" -f "$HOME_DIR/.local/state/nxf/applied/with-unit.json"
active=$(vm systemctl --user is-active nxf-with-unit-greeter.service 2>&1)
if [[ "$active" == "active" ]]; then pass "unit genuinely active under real systemd --user"; else
  fail "unit genuinely active under real systemd --user (was: $active)"
fi
sleep 6
loglines=$(vm cat /tmp/nxf-test-greeter.log 2>/dev/null | wc -l)
if [[ "${loglines:-0}" -ge 1 ]]; then pass "unit is really doing work (heartbeat log growing)"; else
  fail "unit is really doing work (heartbeat log growing)"
fi

section "with-unit: re-add after the unit's own store path changes triggers a real restart"
pid1=$(vm systemctl --user show -p MainPID --value nxf-with-unit-greeter.service 2>&1)
FLAKE_FILE=/home/vbargl/personal/nix/nxf-profile-tests/flake.nix
sed -i 's/hello from \${name}"/hello from ${name} v2"/' "$FLAKE_FILE"
out=$(nxf profile add "$PROFILES_DIR#with-unit" 2>&1); rc=$?
sed -i 's/hello from \${name} v2"/hello from ${name}"/' "$FLAKE_FILE"
assert_rc "$rc" 0 "re-add with-unit after content change exits 0"
assert_contains "$out" "restarting nxf-with-unit-greeter.service (store path changed)" "re-add: nxf takes the restart branch, not enable"
pid2=$(vm systemctl --user show -p MainPID --value nxf-with-unit-greeter.service 2>&1)
if [[ "$pid1" != "$pid2" ]]; then pass "unit actually restarted (PID changed: $pid1 -> $pid2)"; else
  fail "unit actually restarted (PID unchanged: $pid1)"
fi
sleep 6
assert_contains "$(vm tail -3 /tmp/nxf-test-greeter.log 2>&1)" "v2" "restarted unit is running the NEW content"

section "with-unit: remove by short name (regression check)"
out=$(nxf profile remove with-unit 2>&1); rc=$?
assert_rc "$rc" 0 "remove with-unit (by short name) exits 0"
assert_not_contains "$out" "does not match any packages" "remove with-unit: nix actually found the element"
assert_contains "$out" "profile \"with-unit\" removed, tearing down" "remove with-unit: sync reports teardown"
assert_not_exists "unit symlink removed" -e "$HOME_DIR/.config/systemd/user/nxf-with-unit-greeter.service"
assert_not_exists "applied state cleared" -e "$HOME_DIR/.local/state/nxf/applied/with-unit.json"
active=$(vm systemctl --user is-active nxf-with-unit-greeter.service 2>&1)
if [[ "$active" != "active" ]]; then pass "unit genuinely stopped after remove"; else
  fail "unit genuinely stopped after remove (was: $active)"
fi
out=$(nxf profile list 2>&1)
assert_contains "$out" "no nxf-aware profiles applied" "profile list empty after remove"

# ---- with-units: multiple units on one profile, real systemd session -----
section "with-units: add/remove tears down both units"
out=$(nxf profile add "$PROFILES_DIR#with-units" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-units exits 0"
assert_exists "unit #1 symlink installed" -L "$HOME_DIR/.config/systemd/user/nxf-with-units-first.service"
assert_exists "unit #2 symlink installed" -L "$HOME_DIR/.config/systemd/user/nxf-with-units-second.service"
active=$(vm systemctl --user is-active nxf-with-units-first.service nxf-with-units-second.service 2>&1)
if [[ "$active" == $'active\nactive' ]]; then pass "both units genuinely active"; else
  fail "both units genuinely active (was: $active)"
fi
out=$(nxf profile remove with-units 2>&1); rc=$?
assert_rc "$rc" 0 "remove with-units exits 0"
assert_not_exists "unit #1 symlink removed" -e "$HOME_DIR/.config/systemd/user/nxf-with-units-first.service"
assert_not_exists "unit #2 symlink removed" -e "$HOME_DIR/.config/systemd/user/nxf-with-units-second.service"

# ---- no-session degradation still works (root / no D-Bus bus at all) ------
# Separate from the real-session coverage above: nxf must still degrade
# gracefully - warn and skip, not fail - when there's genuinely no session
# (root, bare containers, etc). Runs as root with no XDG_RUNTIME_DIR/
# DBUS_SESSION_BUS_ADDRESS at all, unlike EXEC_BASE above.
section "with-unit: no-session degradation (as root, no bus)"
out=$(incus exec "$VM" --env PATH=/run/current-system/sw/bin --cwd "$PROFILES_DIR" -- nxf profile add --approve "$PROFILES_DIR#with-unit" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-unit as root (no session) still exits 0"
assert_contains "$out" "no systemd user session available" "add with-unit as root: warns instead of failing"
incus exec "$VM" --env PATH=/run/current-system/sw/bin --cwd "$PROFILES_DIR" -- nxf profile remove --approve with-unit >/dev/null 2>&1

# ---- with-config: activate hook lands a config file -----------------------
section "with-config: activate hook"
out=$(nxf profile add "$PROFILES_DIR#with-config" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-config exits 0"
assert_contains "$out" 'running activation script for "with-config"' "add with-config: activation script ran"
cfg=$(vm cat "$HOME_DIR/.config/nxf-demo/config.toml" 2>&1)
assert_contains "$cfg" "hello from a static profile config file" "with-config: config file content correct"

section "with-config: idempotent re-add (same store path)"
out=$(nxf profile add "$PROFILES_DIR#with-config" 2>&1); rc=$?
assert_rc "$rc" 0 "re-add with-config exits 0"
if [[ "$out" == *"No changes."* || "$out" == *"delta +0"* ]]; then
  pass "re-add with-config: no net package change"
else
  fail "re-add with-config: no net package change"; echo "$out"
fi
assert_not_contains "$out" 'running activation script for "with-config"' "re-add with-config: activation script NOT re-run (path unchanged)"
after=$(vm nix profile list 2>&1)
assert_not_contains "$after" "with-config-1" "re-add does not create a disambiguated duplicate element"

section "with-config: remove (activate hook has no teardown - config file expected to remain)"
out=$(nxf profile remove with-config 2>&1); rc=$?
assert_rc "$rc" 0 "remove with-config exits 0"
if vm test -e "$HOME_DIR/.config/nxf-demo/config.toml" 2>/dev/null; then
  finding "removing a profile whose only effect is an activate hook (with-config) leaves its config file behind - there's no deactivate hook, so ~/.config/nxf-demo/config.toml survives after remove. May be intentional (matches the manifest.go doc comment: \"no deactivate hook in the manifest today\"), but worth confirming that's the intended UX."
fi

# ---- empty: no packages, activate-only profile ----------------------------
section "empty: activate-only profile with no packages"
out=$(nxf profile add "$PROFILES_DIR#empty" 2>&1); rc=$?
assert_rc "$rc" 0 "add empty exits 0"
assert_contains "$out" "nxf test: empty profile activated" "add empty: activate script's own stderr line appears"
out=$(nxf profile list 2>&1)
assert_contains "$out" "empty" "profile list shows empty profile"
out=$(nxf profile remove empty 2>&1); rc=$?
assert_rc "$rc" 0 "remove empty exits 0"

# ---- multi-profile coexistence --------------------------------------------
# Every non-empty profile in this flake ships cowsay, so any two of
# {pkgs-only, with-unit, with-units, with-config} collide on cowsay's own
# file paths (a real, expected `nix profile` priority conflict, not an nxf
# bug - see the standalone reproduction). "empty" is the only profile with no
# packages, so it's the only partner that coexists with another without a
# --priority workaround.
section "multi-profile: two profiles applied at once"
nxf profile add "$PROFILES_DIR#empty" >/dev/null 2>&1
out=$(nxf profile add "$PROFILES_DIR#with-unit" 2>&1); rc=$?
assert_rc "$rc" 0 "add with-unit on top of empty exits 0"
out=$(nxf profile list 2>&1)
assert_contains "$out" "empty" "list shows empty alongside with-unit"
assert_contains "$out" "with-unit" "list shows with-unit alongside empty"

section "multi-profile: removing one leaves the other's unit intact"
out=$(nxf profile remove empty 2>&1); rc=$?
assert_rc "$rc" 0 "remove empty exits 0"
assert_exists "with-unit's unit symlink survives sibling removal" -L "$HOME_DIR/.config/systemd/user/nxf-with-unit-greeter.service"
out=$(nxf profile list 2>&1)
assert_not_contains "$out" "empty" "list no longer shows empty"
assert_contains "$out" "with-unit" "list still shows with-unit"
nxf profile remove with-unit >/dev/null 2>&1

# ---- confirmed real bug this run found and fixed: `add` is now idempotent -
section "idempotency: re-adding an already-installed profile doesn't duplicate it"
nxf profile add "$PROFILES_DIR#pkgs-only" >/dev/null 2>&1
before=$(vm nix profile list 2>&1)
out=$(nxf profile add "$PROFILES_DIR#pkgs-only" 2>&1); rc=$?
assert_rc "$rc" 0 "re-add pkgs-only exits 0"
after=$(vm nix profile list 2>&1)
assert_not_contains "$after" "pkgs-only-1" "re-add does not create a disambiguated duplicate element"
n=$(printf '%s' "$after" | grep -c '^Name:.*pkgs-only')
if [[ "$n" == "1" ]]; then pass "exactly one pkgs-only element after re-add"; else
  fail "exactly one pkgs-only element after re-add (found $n)"; echo "$after"
fi
out=$(nxf profile remove pkgs-only 2>&1); rc=$?
assert_rc "$rc" 0 "remove pkgs-only after re-add exits 0"
assert_not_exists "cowsay fully gone after single remove (no leftover duplicate)" -e "$HOME_DIR/.nix-profile/bin/cowsay"

# ---- profile build: builds without installing -----------------------------
section "profile build: does not touch the live profile"
before=$(nxf profile list 2>&1)
out=$(nxf profile build "$PROFILES_DIR#pkgs-only" 2>&1); rc=$?
assert_rc "$rc" 0 "profile build exits 0"
after=$(nxf profile list 2>&1)
if [[ "$before" == "$after" ]]; then pass "profile build: live profile unchanged"; else
  fail "profile build: live profile changed"; echo "before: $before"; echo "after: $after"
fi

# ---- NXF_NO_NOM: escape hatch ----------------------------------------------
section "NXF_NO_NOM=1: build proceeds without nom"
out=$("${EXEC_BASE[@]}" --env NXF_NO_NOM=1 --cwd "$PROFILES_DIR" -- nxf profile add --approve "$PROFILES_DIR#pkgs-only" 2>&1); rc=$?
assert_rc "$rc" 0 "NXF_NO_NOM=1 add exits 0"
assert_contains "$out" "cowsay" "NXF_NO_NOM=1 add: nvd diff still runs"
nxf profile remove pkgs-only >/dev/null 2>&1

# ---- known-gap probe: removing a name that was never added ----------------
section "edge case: remove a profile name that was never added"
out=$(nxf profile remove never-added 2>&1); rc=$?
assert_rc "$rc" 1 "remove never-added reports failure"
assert_contains "$out" "not installed" "remove never-added: error says it is not installed"

# ---- nxf profile sync (standalone, not just implicitly via add/remove) ----
section "profile sync: reconciles drift left by a plain (non-nxf) nix profile remove"
nxf profile add "$PROFILES_DIR#with-unit" >/dev/null 2>&1
"${EXEC_BASE[@]}" -- nix profile remove "profile-with-unit" >/dev/null 2>&1
"${EXEC_BASE[@]}" -- nix profile remove "with-unit" >/dev/null 2>&1
assert_exists "unit symlink still present right after the bypass removal (sync hasn't run yet)" \
  -e "$HOME_DIR/.config/systemd/user/nxf-with-unit-greeter.service"
out=$(nxf profile sync 2>&1); rc=$?
assert_rc "$rc" 0 "profile sync exits 0"
assert_contains "$out" 'profile "with-unit" removed, tearing down' "sync notices the externally-removed profile"
assert_not_exists "sync tears down the orphaned unit symlink" \
  -e "$HOME_DIR/.config/systemd/user/nxf-with-unit-greeter.service"

# ---- nxf os build/test/switch/boot -----------------------------------------
# Uses the scratch flake at /home/vbargl/personal/nix/nxf-os-tests (mounted
# into the VM at /mnt/os-tests) instead of the real ash-twin/saber hosts:
# those either fail to build for reasons unrelated to nxf (ash-twin: a real,
# pre-existing gap - hosts/ash-twin never sets nxf.boot.loader, so nixpkgs'
# default-enabled grub fails its own device assertion) or are too big to
# evaluate in this VM's RAM (saber OOM-killed `nix` outright). gen1/gen2/gen3
# share nxf-vm-test's own base module (incus-virtual-machine.nix), so
# nixos-rebuild can safely switch/test/boot them *inside* this disposable VM.
#
# These test hosts deliberately don't carry the nxf package (avoids an
# impure absolute-path callPackage back to the real repo), so switching into
# one removes `nxf` from PATH - every step below uses the ORIGINAL system
# generation's nxf directly by absolute path, and the very last step
# restores that original generation regardless of pass/fail so the VM is
# left usable for any later `nxf profile` testing.
section "nxf os: build/test/switch/boot across small synthetic NixOS generations"
OS_TESTS_DIR=/mnt/os-tests
ORIG_NXF=/nix/var/nix/profiles/system-1-link/sw/bin/nxf
os_nxf() {
  "${EXEC_BASE[@]}" --cwd "$OS_TESTS_DIR" -- "$ORIG_NXF" "$@" --approve
}
os_root_nxf() {
  incus exec "$VM" --env "PATH=/run/current-system/sw/bin" --cwd "$OS_TESTS_DIR" -- "$ORIG_NXF" "$@" --approve
}
restore_original_generation() {
  incus exec "$VM" -- /nix/var/nix/profiles/system-1-link/bin/switch-to-configuration switch >/dev/null 2>&1
}
trap restore_original_generation EXIT

out=$(os_nxf os build gen1 2>&1); rc=$?
assert_rc "$rc" 0 "os build gen1 (unprivileged) exits 0"
assert_contains "$out" "Closure size:" "os build gen1: nvd diff against /run/current-system printed"

out=$(os_root_nxf os test gen1 2>&1); rc=$?
assert_rc "$rc" 0 "os test gen1 (root) exits 0"
after=$(vm readlink /run/current-system)
assert_contains "$after" "nxf-os-test" "os test gen1: /run/current-system now points at gen1's build"
assert_exists "os test gen1: cowsay landed system-wide" -x "/run/current-system/sw/bin/cowsay"

out=$(os_root_nxf os switch gen2 2>&1); rc=$?
assert_rc "$rc" 0 "os switch gen2 (root) exits 0"
assert_contains "$out" "lolcat" "os switch gen2: nvd diff lists lolcat as added"
assert_contains "$out" "the following new units were started" "os switch gen2: nixos-rebuild started the new unit"
sleep 6
active=$(vm systemctl is-active nxf-os-test-heartbeat 2>&1)
if [[ "$active" == "active" ]]; then pass "os switch gen2: heartbeat unit actually running"; else
  fail "os switch gen2: heartbeat unit actually running (was: $active)"
fi

out=$(os_root_nxf os switch gen3 2>&1); rc=$?
assert_rc "$rc" 0 "os switch gen3 (root) exits 0"
assert_contains "$out" "Removed packages:" "os switch gen3: nvd diff shows removed packages (shrinking switch)"
active=$(vm systemctl is-active nxf-os-test-heartbeat 2>&1)
if [[ "$active" != "active" ]]; then pass "os switch gen3: heartbeat unit stopped"; else
  fail "os switch gen3: heartbeat unit stopped (was: $active)"
fi

before=$(vm readlink /run/current-system)
out=$(os_root_nxf os boot gen1 2>&1); rc=$?
assert_rc "$rc" 0 "os boot gen1 (root) exits 0"
after=$(vm readlink /run/current-system)
if [[ "$before" == "$after" ]]; then pass "os boot gen1: does not touch /run/current-system (only sets the boot default)"; else
  fail "os boot gen1: does not touch /run/current-system"; echo "before: $before"; echo "after: $after"
fi

restore_original_generation
trap - EXIT
if [[ "$(vm which nxf 2>&1)" == "/run/current-system/sw/bin/nxf" ]]; then
  pass "original generation restored: nxf back on PATH"
else
  fail "original generation restored: nxf back on PATH"
fi

# ---- summary ---------------------------------------------------------------
section "summary"
echo "PASS=$PASS FAIL=$FAIL"
if [[ ${#FINDINGS[@]} -gt 0 ]]; then
  echo "Findings (not test failures, but worth a look):"
  for f in "${FINDINGS[@]}"; do echo "  - $f"; done
fi
[[ $FAIL -eq 0 ]]
