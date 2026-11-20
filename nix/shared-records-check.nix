# Boots a machine and checks that the members of the group nxf share and
# replace the build records, and that a user outside it keeps their own.
{ self, pkgs }:
let
  system = pkgs.runCommand "nxf-test-system" { } "mkdir $out";
  # Only nh is mocked. Nix archives real local inputs and registers real GC roots.
  nh = pkgs.writeShellScriptBin "nh" ''
    ln -sfn ${system} "''${@: -1}"
  '';
in
pkgs.testers.runNixOSTest {
  name = "nxf-shared-records";
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    services.nxf = {
      enable = true;
      flake = "/srv/machine";
    };
    users.users = {
      alice = {
        isNormalUser = true;
        extraGroups = [ "nxf" ];
      };
      bob = {
        isNormalUser = true;
        extraGroups = [ "nxf" ];
      };
      outsider.isNormalUser = true;
    };
    environment.systemPackages = [ nh ];
    nix.settings.experimental-features = [
      "nix-command"
      "flakes"
    ];
  };
  testScript = ''
    import shlex

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("mkdir -p /srv/input /srv/machine /var/cache/nxf")
    machine.succeed("chown nxf:nxf /var/cache/nxf")
    machine.succeed("echo source > /srv/input/value")
    machine.succeed("git -C /srv/input init -q && git -C /srv/input add . && git -C /srv/input -c user.name=test -c user.email=test@example.invalid commit -qm input")
    machine.succeed("""cat > /srv/machine/flake.nix <<'EOF'
    {
      inputs.source = { url = "git+file:///srv/input"; flake = false; };
      outputs = _: { nixosConfigurations.default = {}; };
    }
    EOF""")
    machine.succeed("git -C /srv/machine init -q && git -C /srv/machine add .")
    machine.succeed("nix flake lock /srv/machine")
    machine.succeed("git -C /srv/machine add . && git -C /srv/machine -c user.name=test -c user.email=test@example.invalid commit -qm fixture")
    machine.succeed("systemctl restart nxf-permissions.service")
    # tmpfiles must correct an existing directory, including one root made 0755.
    machine.succeed("chmod 0755 /var/lib/nxf/systems && systemd-tmpfiles --create")
    assert machine.succeed("stat -c '%a %U %G' /var/lib/nxf/systems").strip() == "2775 nxf nxf"
    tree = machine.succeed("git -C /srv/machine rev-parse 'HEAD^{tree}'").strip()
    record = "/var/lib/nxf/systems/" + tree

    def build(user, state=None):
        env = "NXF_FLAKE=/srv/machine NXF_SYSTEM=${system} NO_COLOR=1"
        if state:
            env += " NXF_STATE=" + state + " HOME=/var/cache/nxf"
        return machine.succeed("sudo -u " + user + " env " + env + " nxf build 2>&1")

    for user in ["alice", "bob", "nxf"]:
        build(user, "/var/lib/nxf" if user == "nxf" else None)
        assert machine.succeed("stat -c '%U %G' " + record).strip() == user + " nxf"
        assert machine.succeed("readlink " + record).strip() == "${system}"

    # Root's ordinary umask does not prevent another member replacing its link.
    machine.succeed("umask 022; NXF_FLAKE=/srv/machine NXF_SYSTEM=${system} nxf build")
    assert machine.succeed("stat -c '%U %G' " + record).strip() == "root nxf"
    build("alice")
    assert machine.succeed("stat -c '%U %G' " + record).strip() == "alice nxf"

    result = build("outsider")
    assert "shared build records unavailable" in result
    machine.succeed("test -L /home/outsider/.local/state/nxf/systems/" + tree)
    assert machine.succeed("stat -c %U " + record).strip() == "alice"
    # The sources remain per-user; their roots protect the store for everyone.
    source = machine.succeed("readlink -f /home/alice/.local/state/nxf/sources/input").strip()
    assert source.startswith("/nix/store/")
    machine.succeed("nix-collect-garbage")
    machine.succeed("test -f " + shlex.quote(source + "/value"))
    machine.succeed("sudo -u nxf test -r " + shlex.quote(source + "/value"))
  '';
}
