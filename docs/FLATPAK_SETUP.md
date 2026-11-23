# Automatic profiles inside Flatpak games

OMEN wraps the actual game command inside its original Flatpak sandbox. It
creates a nested game cgroup beneath the launcher's app scope, retaining its
ancestor resource limits, environment, directory and argument vector. The
privileged service never launches the command or enters the sandbox.

In Game Library, select Performance or Quiet and open **Profile setup**:

1. **Install launcher helper** copies the packaged static helper into this
   launcher's private app data. **Check Flatpak setup** reads the effective
   host-command permission. It does not change permissions or launcher settings.
2. **Copy wrapper command** and put it around the actual game command in Steam
   Launch Options, Lutris Command prefix, or Heroic's wrapper fields. Keep
   required existing arguments. Steam needs its `%command%` placeholder.
3. Enable automatic restore. On the next launch, approve the ordinary profile
   authorization if asked. If canceled, denied, unsupported or unavailable, the
   wrapped command fails without starting the game. Review Game sessions and
   saved launch issues for the result.

Use the CLI for the same desktop-owned operations:

```sh
omen-cli game-flatpak-setup lutris/flatpak/1 --json
omen-cli game-flatpak-setup lutris/flatpak/1 --install
omen-cli game-flatpak-setup lutris/flatpak/1 --remove
```

The helper requires Linux socket peer process descriptors (Linux 6.5+), unified
cgroup v2 with a user-owned app group, a user systemd session, an enabled profile
provider and authorized OMEN service. Launch-time identity/delegation checks are
authoritative; a successful setup check does not establish game compatibility.
The launcher must include `flatpak-spawn` at `/usr/bin/flatpak-spawn` and allow
`org.freedesktop.Flatpak` on its session bus. That permission allows host command
execution and is broader than game profiles. Review it in the desktop's Flatpak
settings before enabling it. Restart the launcher after changing permissions.
OMEN neither grants nor revokes sandbox permissions.

The copied helper lives at
`~/.var/app/APP_ID/data/omen-linux/managed-v1/omen-flatpak-game`. Setup publishes
its executable and SHA-256 installation record together. Updates/removal refuse
changed executables, symlinks, unexpected files or directories writable by other
users. If files were edited, preserve them and recover the dedicated directory
manually before retrying. The app's other data and launcher config are retained.

**Use manual restore** disables automatic profiles for future launches and keeps
the wrapper in place as a pass-through while host-command permission and the
helper remain available. To play without those dependencies, remove the command
prefix from the launcher. **Remove launcher helper** disables all
managed game IDs for that Flatpak launcher and removes only the verified helper
and record. Remove the command prefix from its games before launching again.
An existing game continues running; its existing profile hold lasts until exit
or **Release profile hold**. Releasing a hold never terminates the game.

Known routes are Steam, Heroic and Lutris Flatpak. Each launcher/runtime/game
combination needs its own execution evidence. Native games, Snap, sandboxed
Legendary, physical controls and full accessibility have separate requirements.
Current verification and exact scope are in `APPLICATION_AUDIT_IMPLEMENTATION.md`.
