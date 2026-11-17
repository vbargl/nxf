# Automatic updates of this machine flake with nxf.
#
# The options are those of nxf.nixosModules.default, which the flake must
# import next to this file. Each is shown with its default; uncomment a
# line to change it.
{ config, ... }:
{
  # Directory of the machine flake: a local Git repository with a
  # `flake.lock` and one configuration under `nixosConfigurations`. Root
  # owns it and the group `nxf` may write it, so every member of the group
  # can work with nxf, and so can the user `nxf` that runs the update.
  # services.nxf.flake = "/etc/nixos";

  services.nxf.autoUpdate = {
    enable = true;

    # When the update starts, as a systemd calendar expression.
    # schedule = "*-*-* 00/6:00:00";

    # The update starts up to this long after the schedule.
    # randomizedDelay = "1h";

    # Keep the same delay on every run. systemd derives it from the
    # machine ID, so machines spread out and each keeps its own slot.
    # fixedDelay = true;

    # Shell scripts that must all succeed after an automatic switch. They
    # are those of the new configuration, run in name order, and are
    # repeated until all pass or `healthTimeout` runs out. Then the
    # previous system is activated again and the revision is not tried
    # again until a newer one comes.
    # healthChecks = { };

    # Seconds the health checks have to pass.
    # healthTimeout = 300;

    # Seconds between two rounds of health checks.
    # healthInterval = 10;

    # The identity of the lockfile commits of the automatic update. With
    # null, Git's own configuration decides. The commits that members of
    # the group make by hand carry their own identity.
    # gitIdentity = { name = "nxf"; email = "nxf@${config.networking.hostName}"; };

    # A file in the format of git-credential-store, one
    # `https://user:token@host` line per host, for inputs fetched with Git
    # over HTTPS. Only the user `nxf` must be able to read it.
    # gitCredentialsFile = null;

    # A nix.conf fragment for the update only, such as
    # `access-tokens = github.com=<token>` for `github:` inputs or
    # `netrc-file = <path>` for tarball inputs.
    # nixConfigFile = null;

    # The update is stopped after this long.
    # timeout = "3h";
  };
}
