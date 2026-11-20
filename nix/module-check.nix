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
      git.credentialsFile = "/run/secrets/git-credentials";
      nixConfigFile = "/run/secrets/nix.conf";
      health.checks."10-units" = "test -z \"$(systemctl --failed --no-legend)\"";
      health.timeout = 600;
      hooks.check = "test -e /run/rollout-open";
      hooks.failure = "cp \"$NXF_RESULTS\" /run/rollout-paused";
      hooks.timeout = 30;
      strategy = "flag";
      inputs.work = "off";
    };
  };

  plain = system {
    services.nxf = {
      flake = "/srv/machine";
      autoUpdate = {
        enable = true;
        schedule = "hourly";
        randomizedDelay = 1200;
        fixedDelay = false;
        updateTimeout = 2700;
        git.identity = null;
      };
    };
  };

  # The command alone, with no timer.
  command = system { services.nxf.enable = true; };

  # Whether the types of the options take a configuration.
  accepts =
    module: (builtins.tryEval (builtins.deepSeq (system module).services.nxf.autoUpdate true)).success;

  update = autoUpdate: {
    services.nxf = {
      autoUpdate = {
        enable = true;
      }
      // autoUpdate;
    };
  };

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
    rootUser = root.users.users.nxf.group;
    # What nxf reads from the configuration in the checkout.
    rootStrategies = builtins.toJSON {
      inherit (root.services.nxf.autoUpdate) strategy inputs;
    };
    plainStrategies = builtins.toJSON {
      inherit (plain.services.nxf.autoUpdate) strategy inputs;
    };
    rootHome = root.users.users.nxf.home;
    rootCreateHome = builtins.toJSON root.users.users.nxf.createHome;
    rootSudo = builtins.toJSON (
      builtins.any (
        rule: rule.groups == [ "nxf" ] && (builtins.head rule.commands).options == [ "NOPASSWD" ]
      ) root.security.sudo.extraRules
    );
    commandSudo = builtins.toJSON (
      builtins.any (rule: rule.groups == [ "nxf" ]) command.security.sudo.extraRules
    );
    durationsAccepted = builtins.toJSON (
      accepts (update {
        randomizedDelay = 0;
        updateTimeout = 60;
      })
    );
    systemdStringRefused = builtins.toJSON (
      accepts (update {
        randomizedDelay = "20min";
      })
    );
    zeroTimeoutRefused = builtins.toJSON (
      accepts (update {
        health.timeout = 0;
      })
    );
    commandHasTimer = builtins.toJSON (builtins.hasAttr "nxf-auto-update" command.systemd.timers);
    healthCheck = root.environment.etc."nxf/health.d/10-units".source;
    healthSettings = root.environment.etc."nxf/health.conf".text;
    checkHook = root.environment.etc."nxf/hooks/check".source;
    failureHook = root.environment.etc."nxf/hooks/failure".source;
    plainEtc = builtins.concatStringsSep " " (builtins.attrNames plain.environment.etc);
    passAsFile = [
      "healthSettings"
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

    has "$rootServicePath" 'User=nxf'
    has "$rootServicePath" 'bin/nxf auto-update'
    has "$rootServicePath" '"NXF_FLAKE=/etc/nixos"'
    has "$rootServicePath" '"NXF_STATE=/var/lib/nxf"'
    has "$rootServicePath" '"GIT_COMMITTER_NAME=nxf"'
    # No e-mail by default: the variable is there and empty.
    has "$rootServicePath" '"GIT_COMMITTER_EMAIL="'
    has "$rootServicePath" '"GIT_CONFIG_COUNT=1"'
    has "$rootServicePath" '"GIT_CONFIG_KEY_0=credential.helper"'
    has "$rootServicePath" '"GIT_CONFIG_VALUE_0=store --file=/run/secrets/git-credentials"'
    has "$rootServicePath" '"NIX_CONFIG=!include /run/secrets/nix.conf"'
    # The times are in /etc, which the switch replaces; the unit keeps running.
    lacks "$rootServicePath" 'NXF_HEALTH'
    has "$healthSettingsPath" 'timeout=600'
    has "$healthSettingsPath" 'interval=10'
    has "$rootServicePath" 'X-StopOnRemoval=false'
    has "$rootServicePath" 'X-RestartIfChanged=false'
    test -x "$healthCheck"
    # Lines that modules add to one check must each succeed.
    has "$healthCheck" 'set -e'
    test -x "$checkHook"
    has "$checkHook" 'set -e'
    has "$checkHook" 'test -e /run/rollout-open'
    test -x "$failureHook"
    has "$failureHook" 'cp "$NXF_RESULTS" /run/rollout-paused'
    has "$rootServicePath" '"NXF_HOOK_TIMEOUT=30"'
    has "$plainServicePath" '"NXF_HOOK_TIMEOUT=60"'
    case " $plainEtc " in *" nxf/"*) echo "unexpected scripts: $plainEtc"; exit 1 ;; esac
    has "$healthCheck" 'systemctl --failed --no-legend'
    # The group keeps what the update writes, and only the switch is elevated.
    has "$rootServicePath" 'Group=nxf'
    has "$rootServicePath" 'UMask=0002'
    # The user has no home; what tools write there goes to the cache.
    test "$rootHome" = /var/empty
    test "$rootCreateHome" = false
    has "$rootServicePath" '"HOME=/var/cache/nxf"'
    has "$rootServicePath" '"XDG_CACHE_HOME=/var/cache/nxf"'
    has "$rootServicePath" 'CacheDirectory=nxf'
    has "$rootServicePath" 'CacheDirectoryMode=0750'
    has "$rootServicePath" '/run/wrappers'
    has "$rootServicePath" 'nxf-permissions.service'
    lacks "$rootServicePath" 'StateDirectory'
    has "$permissionsPath" 'chown --recursive nxf:nxf "$state"'
    test "$rootUser" = nxf
    test "$rootStrategies" = '{"inputs":{"work":"off"},"strategy":"flag"}'
    test "$plainStrategies" = '{"inputs":{},"strategy":"report"}'
    test "$rootSudo" = true
    test "$commandSudo" = true
    has "$tmpfilesPath" 'd /var/lib/nxf 2775 nxf nxf -'
    has "$tmpfilesPath" 'd /var/lib/nxf/systems 2775 nxf nxf -'
    has "$permissionsUnitPath" 'Type=oneshot'
    has "$permissionsPath" 'chown nxf:nxf "$flake"'
    has "$permissionsPath" 'chmod 2775 "$flake"'
    has "$permissionsPath" 'core.sharedRepository group'
    # Git rewrites its configuration as root, so the owner is set after it.
    line() { grep --line-number --fixed-strings -- "$1" "$permissionsPath" | cut -d: -f1; }
    test "$(line 'core.sharedRepository group')" -lt "$(line 'chown --recursive nxf:nxf "$flake/.git"')"
    has "$permissionsPath" "flake=/etc/nixos"
    test "$rootGroup" = true
    test "$commandGroup" = true
    test "$commandHasTimer" = false
    # Git trusts the checkout the user nxf owns, for root and every member.
    has "$gitconfig" 'directory = "/etc/nixos"'
    has "$commandGitconfig" 'directory = "/etc/nixos"'
    has "$rootTimerPath" 'OnCalendar=*-*-* 00/6:00:00'
    has "$rootTimerPath" 'RandomizedDelaySec=3600s'
    has "$rootTimerPath" 'FixedRandomDelay=true'
    has "$rootTimerPath" 'Persistent=true'
    # A oneshot is starting for as long as it runs, so this ends the update.
    has "$rootServicePath" 'Type=oneshot'
    has "$rootServicePath" 'TimeoutStartSec=10800s'

    has "$plainServicePath" 'User=nxf'
    has "$plainServicePath" '"NXF_FLAKE=/srv/machine"'
    lacks "$plainServicePath" 'GIT_COMMITTER_EMAIL'
    lacks "$plainServicePath" 'GIT_CONFIG_COUNT'
    lacks "$plainServicePath" 'NIX_CONFIG'
    has "$plainPermissionsPath" "flake=/srv/machine"
    has "$plainGitconfig" 'directory = "/srv/machine"'
    has "$plainTimerPath" 'OnCalendar=hourly'
    has "$plainTimerPath" 'RandomizedDelaySec=1200s'
    has "$plainTimerPath" 'FixedRandomDelay=false'
    has "$plainServicePath" 'TimeoutStartSec=2700s'

    # Every duration is a number of seconds.
    test "$durationsAccepted" = true
    test "$systemdStringRefused" = false
    test "$zeroTimeoutRefused" = false

    touch $out
  ''
