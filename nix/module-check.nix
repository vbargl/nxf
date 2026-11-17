# Evaluates the module with its defaults and with the options set, and checks
# the generated units, the group and the permissions of the flake.
{
  self,
  nixpkgs,
  pkgs,
}:

let
  system =
    module:
    (nixpkgs.lib.nixosSystem {
      inherit (pkgs.stdenv.hostPlatform) system;
      modules = [
        self.nixosModules.default
        module
        {
          networking.hostName = "machine";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
          system.stateVersion = "26.05";
        }
      ];
    }).config;

  root = system {
    services.nxf.autoUpdate = {
      enable = true;
      gitCredentialsFile = "/run/secrets/git-credentials";
      nixConfigFile = "/run/secrets/nix.conf";
      healthChecks."10-units" = "test -z \"$(systemctl --failed --no-legend)\"";
      healthTimeout = 600;
    };
  };

  plain = system {
    services.nxf = {
      flake = "/srv/machine";
      autoUpdate = {
        enable = true;
        schedule = "hourly";
        randomizedDelay = "20min";
        gitIdentity = null;
      };
    };
  };

  # The command alone, with no timer.
  command = system { services.nxf.enable = true; };

  unit = config: name: config.systemd.units.${name}.text;
in
pkgs.runCommand "nxf-module-check"
  {
    rootService = unit root "nxf-auto-update.service";
    rootTimer = unit root "nxf-auto-update.timer";
    plainService = unit plain "nxf-auto-update.service";
    plainTimer = unit plain "nxf-auto-update.timer";
    permissions = root.systemd.services.nxf-permissions.script;
    plainPermissions = plain.systemd.services.nxf-permissions.script;
    permissionsUnit = unit root "nxf-permissions.service";
    gitconfig = root.environment.etc.gitconfig.source;
    plainGitconfig = plain.environment.etc.gitconfig.source;
    commandGitconfig = command.environment.etc.gitconfig.source;
    tmpfiles = builtins.concatStringsSep "\n" root.systemd.tmpfiles.rules;
    rootGroup = builtins.toJSON (builtins.hasAttr "nxf" root.users.groups);
    commandGroup = builtins.toJSON (builtins.hasAttr "nxf" command.users.groups);
    commandHasTimer = builtins.toJSON (builtins.hasAttr "nxf-auto-update" command.systemd.timers);
    healthCheck = root.environment.etc."nxf/health.d/10-units".source;
    passAsFile = [
      "rootService"
      "rootTimer"
      "plainService"
      "plainTimer"
      "permissions"
      "plainPermissions"
      "permissionsUnit"
      "tmpfiles"
    ];
  }
  ''
    has() { grep --quiet --fixed-strings -- "$2" "$1" || { echo "missing in $1: $2"; cat "$1"; exit 1; }; }
    lacks() { ! grep --quiet --fixed-strings -- "$2" "$1" || { echo "unexpected in $1: $2"; cat "$1"; exit 1; }; }

    has "$rootServicePath" 'User=root'
    has "$rootServicePath" 'bin/nxf auto-update'
    has "$rootServicePath" '"NXF_FLAKE=/etc/nixos"'
    has "$rootServicePath" '"NXF_STATE=/var/lib/nxf"'
    has "$rootServicePath" '"GIT_COMMITTER_EMAIL=nxf@machine"'
    has "$rootServicePath" '"GIT_CONFIG_COUNT=1"'
    has "$rootServicePath" '"GIT_CONFIG_KEY_0=credential.helper"'
    has "$rootServicePath" '"GIT_CONFIG_VALUE_0=store --file=/run/secrets/git-credentials"'
    has "$rootServicePath" '"NIX_CONFIG=!include /run/secrets/nix.conf"'
    has "$rootServicePath" '"NXF_HEALTH_TIMEOUT=600"'
    has "$rootServicePath" '"NXF_HEALTH_INTERVAL=10"'
    has "$rootServicePath" 'X-StopOnRemoval=false'
    has "$rootServicePath" 'X-RestartIfChanged=false'
    test -x "$healthCheck"
    has "$healthCheck" 'systemctl --failed --no-legend'
    # Root runs it whatever the machine, and the group keeps what it writes.
    has "$rootServicePath" 'Group=nxf'
    has "$rootServicePath" 'UMask=0002'
    has "$rootServicePath" 'StateDirectoryMode=2775'
    has "$tmpfilesPath" 'd /var/lib/nxf 2775 root nxf -'
    has "$permissionsUnitPath" 'Type=oneshot'
    has "$permissionsPath" 'chown root:nxf "$flake"'
    has "$permissionsPath" 'chmod 2775 "$flake"'
    has "$permissionsPath" 'core.sharedRepository group'
    has "$permissionsPath" "flake=/etc/nixos"
    test "$rootGroup" = true
    test "$commandGroup" = true
    test "$commandHasTimer" = false
    # Git trusts the checkout root owns, for every member.
    has "$gitconfig" 'directory = "/etc/nixos"'
    has "$commandGitconfig" 'directory = "/etc/nixos"'
    has "$rootTimerPath" 'OnCalendar=*-*-* 00/6:00:00'
    has "$rootTimerPath" 'RandomizedDelaySec=1h'
    has "$rootTimerPath" 'FixedRandomDelay=true'
    has "$rootTimerPath" 'Persistent=true'

    has "$plainServicePath" 'User=root'
    has "$plainServicePath" '"HOME=/root"'
    has "$plainServicePath" '"NXF_FLAKE=/srv/machine"'
    lacks "$plainServicePath" 'GIT_COMMITTER_EMAIL'
    lacks "$plainServicePath" 'GIT_CONFIG_COUNT'
    lacks "$plainServicePath" 'NIX_CONFIG'
    lacks "$plainServicePath" '/run/wrappers'
    has "$plainPermissionsPath" "flake=/srv/machine"
    has "$plainGitconfig" 'directory = "/srv/machine"'
    has "$plainTimerPath" 'OnCalendar=hourly'
    has "$plainTimerPath" 'RandomizedDelaySec=20min'

    touch $out
  ''
