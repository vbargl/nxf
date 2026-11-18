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
  # The group, and the user of the same name that runs the update.
  group = "nxf";
  home = "/var/lib/nxf-home";

  gitConfig = lib.optional (auto.gitCredentialsFile != null) {
    key = "credential.helper";
    value = "store --file=${auto.gitCredentialsFile}";
  };

  # The update may have started before a switch; use the running system's tools.
  tools = ''
    export PATH=${
      lib.makeBinPath [
        config.systemd.package
        pkgs.coreutils
      ]
    }:/run/current-system/sw/bin:$PATH
  '';

  # One hook as an executable in /etc/nxf/hooks, when the configuration has it.
  # Modules may each add lines, so every command must succeed.
  hook =
    name: script:
    lib.optionalAttrs (script != "") {
      "nxf/hooks/${name}".source = pkgs.writeShellScript "nxf-${name}-hook" ''
        set -e
        ${tools}
        ${script}
      '';
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
        `flake.lock` and one configuration under `nixosConfigurations`. Root
        owns it and the group `nxf` may write it, so every member of the group
        can work with nxf, and so can the user `nxf` that runs the update.
      '';
    };

    autoUpdate = {
      enable = lib.mkEnableOption ''
        a timer that updates the inputs of the machine flake with nxf, commits
        each update that builds, and switches to the result
      '';

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
        default = {
          name = "nxf";
          email = "nxf@${config.networking.hostName}";
        };
        defaultText = lib.literalExpression ''{ name = "nxf"; email = "nxf@''${config.networking.hostName}"; }'';
        description = ''
          The identity of the lockfile commits of the automatic update. With
          null, Git's own configuration decides. The commits that members of
          the group make by hand carry their own identity.
        '';
      };

      gitCredentialsFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/run/secrets/nxf-git-credentials";
        description = ''
          A file in the format of git-credential-store, one
          `https://user:token@host` line per host, for inputs fetched with Git
          over HTTPS. Only the user `nxf` must be able to read it.
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

      checkHook = lib.mkOption {
        type = lib.types.lines;
        default = "";
        example = ''
          test "$(curl --fail --silent https://example.org/rollout)" = open
        '';
        description = ''
          A shell script that is asked before an automatic update starts.
          When one of its commands fails, the run is skipped: it reports
          `skipped` with the last line the script printed, and nothing is
          fetched, built or activated. Updates run by hand do not ask it.
        '';
      };

      failureHook = lib.mkOption {
        type = lib.types.lines;
        default = "";
        example = ''
          curl --fail --silent --request POST --data @"$NXF_RESULTS" https://example.org/rollout/pause
        '';
        description = ''
          A shell script that runs after an automatic update failed: an
          input that did not build, a switch that failed, or a health check.
          It finds the results of the run as JSON in the file `$NXF_RESULTS`.
          When it fails itself, that is reported as the result `hook`.
        '';
      };

      hookTimeout = lib.mkOption {
        type = lib.types.ints.positive;
        default = 60;
        description = "Seconds the `checkHook` and the `failureHook` each have to end.";
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

      # Whoever is in this group works with nxf: they write the flake and the
      # state of the updater. Root owns both, always.
      users.groups.${group} = { };

      # The update runs as this user, not as root, with an SSH configuration and
      # Git credentials of its own. Only the switch is elevated, with sudo, and
      # that needs no password: nh calls sudo for it, and an unattended run
      # cannot answer. The group is trusted like root, because what it writes
      # is what the switch activates.
      users.users.${group} = {
        isSystemUser = true;
        inherit group home;
        createHome = true;
        description = "nxf automatic updates";
      };
      security.sudo.extraRules = [
        {
          groups = [ group ];
          commands = [
            {
              command = "ALL";
              options = [ "NOPASSWD" ];
            }
          ];
        }
      ];

      # Git refuses a checkout another user owns, and root owns this one.
      programs.git.enable = lib.mkDefault true;
      programs.git.config.safe.directory = [ cfg.flake ];

      systemd.tmpfiles.rules = [ "d /var/lib/nxf 2775 root ${group} -" ];

      # Only what nxf changes is handed over: the directory, the lockfile, the
      # repository and the state. The other files, such as secrets, keep their
      # owner and mode.
      systemd.services.nxf-permissions = {
        description = "Give the group ${group} the machine flake";
        wantedBy = [ "multi-user.target" ];
        after = [ "local-fs.target" ];
        path = [
          pkgs.coreutils
          pkgs.findutils
          pkgs.git
        ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          state=/var/lib/nxf
          if [ -d "$state" ]; then
            chown --recursive root:${group} "$state"
            chmod --recursive g+rwX "$state"
            find "$state" -type d -exec chmod g+s {} +
          fi
          flake=${lib.escapeShellArg cfg.flake}
          [ -d "$flake" ] || exit 0
          chown root:${group} "$flake"
          chmod 2775 "$flake"
          if [ -e "$flake/flake.lock" ]; then
            chown root:${group} "$flake/flake.lock"
            chmod 664 "$flake/flake.lock"
          fi
          if [ -d "$flake/.git" ]; then
            chown --recursive root:${group} "$flake/.git"
            chmod --recursive g+rwX "$flake/.git"
            find "$flake/.git" -type d -exec chmod g+s {} +
            # New objects and refs of any member stay usable by the others.
            git -C "$flake" config core.sharedRepository group
          fi
        '';
      };
    })

    (lib.mkIf auto.enable {
      environment.etc =
        lib.mapAttrs' (
          name: script:
          lib.nameValuePair "nxf/health.d/${name}" {
            source = pkgs.writeShellScript "nxf-health-${name}" ''
              ${tools}
              ${script}
            '';
          }
        ) auto.healthChecks
        // lib.optionalAttrs (auto.healthChecks != { }) {
          # nxf reads the times after the switch, so those of the new system count.
          "nxf/health.conf".text = ''
            timeout=${toString auto.healthTimeout}
            interval=${toString auto.healthInterval}
          '';
        }
        // hook "check" auto.checkHook
        // hook "failure" auto.failureHook;

      systemd.services.nxf-auto-update = {
        description = "Update the machine flake with nxf, build it and switch to it";
        # The switch this service performs must not stop or restart it.
        restartIfChanged = false;
        unitConfig.X-StopOnRemoval = false;
        wants = [
          "network-online.target"
          "nxf-permissions.service"
        ];
        after = [
          "network-online.target"
          "nxf-permissions.service"
        ];
        path = [
          cfg.package
          config.nix.package
          pkgs.git
          pkgs.openssh
          # sudo, with which nh switches.
          "/run/wrappers"
        ];
        environment = {
          HOME = home;
          NXF_FLAKE = cfg.flake;
          # Build records every member's `nxf status` can read.
          NXF_STATE = "/var/lib/nxf";
          NXF_HOOK_TIMEOUT = toString auto.hookTimeout;
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
          User = group;
          Group = group;
          # What the update writes stays writable by the group.
          UMask = "0002";
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
