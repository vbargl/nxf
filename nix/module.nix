self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.nxf;
  auto = cfg.autoUpdate;
  asRoot = auto.user == "root";

  gitConfig =
    # Root may work on a checkout another user owns.
    lib.optional asRoot {
      key = "safe.directory";
      value = cfg.flake;
    }
    ++ lib.optional (auto.gitCredentialsFile != null) {
      key = "credential.helper";
      value = "store --file=${auto.gitCredentialsFile}";
    };

  # Git reads configuration from numbered environment variables.
  gitConfigEnvironment = lib.listToAttrs (
    lib.concatLists (
      lib.imap0 (index: entry: [
        (lib.nameValuePair "GIT_CONFIG_KEY_${toString index}" entry.key)
        (lib.nameValuePair "GIT_CONFIG_VALUE_${toString index}" entry.value)
      ]) gitConfig
    )
  );
in
{
  options.services.nxf = {
    enable = lib.mkEnableOption "the nxf command for the machine's local flake";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.nxf;
      defaultText = lib.literalExpression "nxf.packages.\${system}.nxf";
      description = "The nxf package.";
    };

    flake = lib.mkOption {
      type = lib.types.str;
      default = "/etc/nixos";
      description = ''
        Directory of the machine flake: a local Git repository with a
        `flake.lock` and one configuration under `nixosConfigurations`.
      '';
    };

    autoUpdate = {
      enable = lib.mkEnableOption ''
        a timer that updates the inputs of the machine flake with nxf, commits
        each update that builds, and switches to the result
      '';

      user = lib.mkOption {
        type = lib.types.str;
        default = "root";
        example = "alice";
        description = ''
          The user that runs the update and must be able to write the flake
          directory. Root needs nothing else. Another user works as if they
          ran nxf themselves: with their own SSH configuration and Git
          identity, and with sudo without a password, which nh uses to switch.
        '';
      };

      schedule = lib.mkOption {
        type = lib.types.str;
        default = "*-*-* 00/6:00:00";
        example = "hourly";
        description = "When the update starts, as a systemd calendar expression.";
      };

      randomizedDelay = lib.mkOption {
        type = lib.types.str;
        default = "1h";
        description = "The update starts up to this long after the schedule.";
      };

      fixedDelay = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Keep the same delay on every run. systemd derives it from the
          machine ID, so machines spread out and each keeps its own slot.
        '';
      };

      gitIdentity = lib.mkOption {
        type = lib.types.nullOr (
          lib.types.submodule {
            options = {
              name = lib.mkOption { type = lib.types.str; };
              email = lib.mkOption { type = lib.types.str; };
            };
          }
        );
        default =
          if asRoot then
            {
              name = "nxf";
              email = "nxf@${config.networking.hostName}";
            }
          else
            null;
        defaultText = lib.literalExpression ''{ name = "nxf"; email = "nxf@''${config.networking.hostName}"; }'';
        description = ''
          The identity of the lockfile commits. With null, Git's own
          configuration decides, as for commits the user makes by hand; null
          is the default when `user` is not root.
        '';
      };

      gitCredentialsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/run/secrets/nxf-git-credentials";
        description = ''
          A file in the format of git-credential-store, one
          `https://user:token@host` line per host, for inputs fetched with Git
          over HTTPS. Only the update user must be able to read it.
        '';
      };

      nixConfigFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/run/secrets/nxf-nix.conf";
        description = ''
          A nix.conf fragment for the update only, such as
          `access-tokens = github.com=<token>` for `github:` inputs or
          `netrc-file = <path>` for tarball inputs.
        '';
      };

      healthChecks = lib.mkOption {
        type = lib.types.attrsOf lib.types.lines;
        default = { };
        example = lib.literalExpression ''
          {
            "10-units" = "test -z \"$(systemctl --failed --no-legend)\"";
            "20-k3s" = "systemctl is-active --quiet k3s";
          }
        '';
        description = ''
          Shell scripts that must all succeed after an automatic switch. They
          are those of the new configuration, run in name order, and are
          repeated until all pass or `healthTimeout` runs out. Then the
          previous system is activated again and the revision is not tried
          again until a newer one comes.
        '';
      };

      healthTimeout = lib.mkOption {
        type = lib.types.ints.positive;
        default = 300;
        description = "Seconds the health checks have to pass.";
      };

      healthInterval = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 10;
        description = "Seconds between two rounds of health checks.";
      };

      timeout = lib.mkOption {
        type = lib.types.str;
        default = "3h";
        description = "The update is stopped after this long.";
      };
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.enable || auto.enable) {
      environment.systemPackages = [ cfg.package ];
      environment.variables.NXF_FLAKE = cfg.flake;
    })

    (lib.mkIf auto.enable {
      environment.etc = lib.mapAttrs' (
        name: script:
        lib.nameValuePair "nxf/health.d/${name}" {
          source = pkgs.writeShellScript "nxf-health-${name}" ''
            # The update started before the switch; use the new system's tools.
            export PATH=${
              lib.makeBinPath [
                config.systemd.package
                pkgs.coreutils
              ]
            }:/run/current-system/sw/bin:$PATH
            ${script}
          '';
        }
      ) auto.healthChecks;

      systemd.services.nxf-auto-update = {
        description = "Update the machine flake with nxf, build it and switch to it";
        # The switch this service performs must not stop or restart it.
        restartIfChanged = false;
        unitConfig.X-StopOnRemoval = false;
        wants = [ "network-online.target" ];
        after = [ "network-online.target" ];
        path = [
          cfg.package
          config.nix.package
          pkgs.git
          pkgs.openssh
        ]
        # sudo, with which nh switches for an unprivileged user.
        ++ lib.optional (!asRoot) "/run/wrappers";
        environment = {
          HOME = config.users.users.${auto.user}.home;
          NXF_FLAKE = cfg.flake;
          # Build records every user's `nxf status` can read.
          NXF_STATE = "/var/lib/nxf";
          NXF_HEALTH_TIMEOUT = toString auto.healthTimeout;
          NXF_HEALTH_INTERVAL = toString auto.healthInterval;
          # Unattended: never ask, never hang.
          GIT_TERMINAL_PROMPT = "0";
          GIT_SSH_COMMAND = "${pkgs.openssh}/bin/ssh -o BatchMode=yes -o ConnectTimeout=30";
        }
        // lib.optionalAttrs (gitConfig != [ ]) (
          gitConfigEnvironment // { GIT_CONFIG_COUNT = toString (lib.length gitConfig); }
        )
        // lib.optionalAttrs (auto.gitIdentity != null) {
          GIT_AUTHOR_NAME = auto.gitIdentity.name;
          GIT_AUTHOR_EMAIL = auto.gitIdentity.email;
          GIT_COMMITTER_NAME = auto.gitIdentity.name;
          GIT_COMMITTER_EMAIL = auto.gitIdentity.email;
        }
        // lib.optionalAttrs (auto.nixConfigFile != null) {
          NIX_CONFIG = "!include ${auto.nixConfigFile}";
        };
        serviceConfig = {
          Type = "oneshot";
          User = auto.user;
          StateDirectory = "nxf";
          StateDirectoryMode = "0755";
          ExecStart = "${lib.getExe cfg.package} auto-update";
          TimeoutStartSec = auto.timeout;
          Nice = 10;
          IOSchedulingClass = "idle";
        };
      };

      systemd.timers.nxf-auto-update = {
        wantedBy = [ "timers.target" ];
        timerConfig = {
          OnCalendar = auto.schedule;
          RandomizedDelaySec = auto.randomizedDelay;
          FixedRandomDelay = auto.fixedDelay;
          Persistent = true;
        };
      };
    })
  ];
}
