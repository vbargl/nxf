# Evaluates the module as root and as a user, and checks the generated units.
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

  user = system {
    users.users.alice = {
      isNormalUser = true;
      home = "/home/alice";
    };
    services.nxf = {
      flake = "/home/alice/machine";
      autoUpdate = {
        enable = true;
        user = "alice";
        schedule = "hourly";
        randomizedDelay = "20min";
      };
    };
  };

  unit = config: name: config.systemd.units.${name}.text;
in
pkgs.runCommand "nxf-module-check"
  {
    rootService = unit root "nxf-auto-update.service";
    rootTimer = unit root "nxf-auto-update.timer";
    userService = unit user "nxf-auto-update.service";
    userTimer = unit user "nxf-auto-update.timer";
    healthCheck = root.environment.etc."nxf/health.d/10-units".source;
    healthSettings = root.environment.etc."nxf/health.conf".text;
    passAsFile = [
      "healthSettings"
      "rootService"
      "rootTimer"
      "userService"
      "userTimer"
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
    has "$rootServicePath" '"GIT_CONFIG_COUNT=2"'
    has "$rootServicePath" '"GIT_CONFIG_KEY_0=safe.directory"'
    has "$rootServicePath" '"GIT_CONFIG_VALUE_1=store --file=/run/secrets/git-credentials"'
    has "$rootServicePath" '"NIX_CONFIG=!include /run/secrets/nix.conf"'
    # The times are in /etc, which the switch replaces; the unit keeps running.
    lacks "$rootServicePath" 'NXF_HEALTH'
    has "$healthSettingsPath" 'timeout=600'
    has "$healthSettingsPath" 'interval=10'
    has "$rootServicePath" 'X-StopOnRemoval=false'
    has "$rootServicePath" 'X-RestartIfChanged=false'
    test -x "$healthCheck"
    has "$healthCheck" 'systemctl --failed --no-legend'
    lacks "$rootServicePath" '/run/wrappers'
    has "$rootTimerPath" 'OnCalendar=*-*-* 00/6:00:00'
    has "$rootTimerPath" 'RandomizedDelaySec=1h'
    has "$rootTimerPath" 'FixedRandomDelay=true'
    has "$rootTimerPath" 'Persistent=true'

    has "$userServicePath" 'User=alice'
    has "$userServicePath" '"HOME=/home/alice"'
    has "$userServicePath" '"NXF_FLAKE=/home/alice/machine"'
    has "$userServicePath" '/run/wrappers'
    lacks "$userServicePath" 'GIT_COMMITTER_EMAIL'
    lacks "$userServicePath" 'GIT_CONFIG_COUNT'
    lacks "$userServicePath" 'NIX_CONFIG'
    has "$userTimerPath" 'OnCalendar=hourly'
    has "$userTimerPath" 'RandomizedDelaySec=20min'

    touch $out
  ''
