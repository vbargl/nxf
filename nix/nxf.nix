# Automatic updates of this machine flake with nxf.
#
# The options are those of nxf.nixosModules.default, which the flake must
# import next to this file. Each is shown with its default; uncomment a
# line to change it.
{ config, ... }:
{
  # Directory of the machine flake: a local Git repository with a
  # `flake.lock` and one configuration under `nixosConfigurations`. The
  # user `nxf`, which runs the update, owns it, and the group `nxf` may
  # write it, so every member of the group can work with nxf.
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

    # A shell script that is asked before an automatic update starts.
    # When one of its commands fails, the run is skipped: it reports
    # `skipped` with the last line the script printed, and nothing is
    # fetched, built or activated. Updates run by hand do not ask it.
    # checkHook = "";

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

    # A shell script that runs after an automatic update failed: an
    # input that did not build, a switch that failed, or a health check.
    # It finds the results of the run as JSON in the file `$NXF_RESULTS`.
    # When it fails itself, that is reported as the result `hook`.
    # failureHook = "";

    # Seconds the `checkHook` and the `failureHook` each have to end.
    # hookTimeout = 60;

    # The identity of the lockfile commits of the automatic update. The
    # e-mail is empty by default: the commits stay on the machine and
    # nobody writes to their author. With null, Git's own configuration
    # decides. The commits that members of the group make by hand carry
    # their own identity.
    # gitIdentity = { name = "nxf"; email = ""; };

    # A file in the format of git-credential-store, one
    # `https://user:token@host` line per host, for inputs fetched with Git
    # over HTTPS. It may be anywhere outside the Nix store; the user
    # `nxf` must be able to read it, and nobody else should.
    # gitCredentialsFile = null;

    # A nix.conf fragment for the update only, such as
    # `access-tokens = github.com=<token>` for `github:` inputs or
    # `netrc-file = <path>` for tarball inputs. Like `gitCredentialsFile`,
    # it may be anywhere the user `nxf` can read.
    # nixConfigFile = null;

    # What the automatic update does with an input that has no entry in
    # `inputs`. With `off` it leaves the input alone, which is then
    # updated by hand. With `flag` it tries, and an update that cannot be
    # fetched or does not build is only recorded: the run goes on and
    # does not fail, and `nxf status` shows the record. With `panic` such
    # an update ends the run: no later input is tried, nothing is
    # switched to, and the run fails.
    # strategy = "panic";

    # The strategy of single inputs of the machine flake, by name. nxf
    # reads the strategies from the configuration in the checkout, so a
    # commit changes them, without a switch.
    # inputs = { };

    # The update is stopped after this long.
    # timeout = "3h";
  };
}
