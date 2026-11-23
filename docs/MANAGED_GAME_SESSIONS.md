# Managed game sessions

Revision 26 packages the static Flatpak wrapper and adds per-launcher installation,
permission checks, safe update/removal and opt-in automatic mode in Profile setup.
The wrapper preserves the original sandbox and app scope's aggregate limits.
The frozen candidate passes 48 private VM checks, including fifteen Flatpak
lifecycle workflows and two reboots. Follow [Flatpak setup](FLATPAK_SETUP.md);
[the implementation report](AUDIT_COMPLETION_2026-10-28.md) separates local fixture,
real launcher and physical evidence.

The earlier revision 26 development milestone passed 45 VM checks before GUI
setup and package integration were delivered. Its separate historical evidence
is retained in [the nested bridge development report](FLATPAK_NESTED_BRIDGE_2026-10-28.md).

Revision 25 retains the original registered process identity during observation.
A live peer moving outside its tracked scope now releases the hold and records
Interrupted, rather than Ended; its game may still be running. See
[scope migration recovery](SESSION_SCOPE_MIGRATION_2026-10-28.md). Automatic
Flatpak setup was unavailable at that milestone.

Revision 16 connects automatic profile ownership to an actual-command wrapper,
daemon supervision, per-game setup and native session history. Manual mode is
still the default. This implements wrapped process-tree lifetime; it does not
prove that every launcher dispatches a game into that tree. Real Steam,
Heroic and Flatpak managed execution remain unvalidated. Revision 21 now has
one actual native Lutris/OpenTTD result, described below; other Lutris games
and store accounts remain separate compatibility requirements.

A real Flatpak 1.16.6 scope/namespace probe on the unchanged revision 24 package
now reproduces premature profile release after scope migration. Namespace entry
can retain detached descendants in the OMEN scope, but the tested `flatpak enter`
command loses exit-code/signal semantics. Automatic restoration for Flatpak
remains unsupported. This also applies to custom commands that start a Flatpak
client. Such a command is not currently rejected before registration; it can acquire and
then prematurely release a hold. Use manual restoration. See
[the runtime investigation](FLATPAK_SESSION_BOUNDARIES_2026-10-28.md).

## Setup and use

In Games, choose Performance or Quiet for an entry, open **Profile setup**, then
enable automatic restore. These user preferences are included in user backups.
Balanced and Cool cannot use provider holds. Changing the mode affects future
launches; **Release profile hold** ends an existing hold without stopping its game.

- Custom commands launched here are wrapped automatically. Use the actual game
  command, rather than a request to an already-running launcher.
- Native Steam setup displays a quoted `omen-cli game-run GAME_ID -- %command%`
  Launch Options prefix. Keep required existing game arguments.
- Native Lutris/Heroic setup displays an actual-command prefix. The launcher's
  prefix/wrapper interface must supply its real executable and separate arguments.
  Configuring those fields is manual; OMEN does not modify launcher databases.
- Automatic setup is unavailable for discovered Flatpak entries. Host-spawning a
  Flatpak client does not establish valid executable paths, environment or lifetime.

The CLI accepts an actual argument vector, for example:

```bash
omen-cli game-run custom/example -- /absolute/path/to/game argument
omen-cli game-sessions --json
omen-cli game-session-release SESSION_TOKEN
omen-cli game-launch-issues --json
omen-cli game-launch-issues-clear
```

`game-run` passes through to the command when automatic mode is disabled. With
that mode enabled, failure to acquire an authorized, verified hold stops the
wrapped command and reports an error. Launching a store URI does not pre-apply
an automatic profile: the configured wrapper must register when the real command
starts. A launch receipt remains unverified. The GUI shows cached per-user
sessions, requested profile, fresh effective profile, hold state, details and
recent ended/interrupted history. Unavailable status disables release controls.

## Ownership and process lifetime

The desktop-user wrapper creates a dedicated `omen-game-<random token>.scope`
through `systemd-run --user --scope --collect --expand-environment=no`. It then
requests the hold and execs the unchanged command arguments. The daemon never
receives or executes game commands. Effective UID 0 is refused by the wrapper.

Revision 18 retains a small observer process outside that scope. It observes
scope-creation failures and the wrapped command's exit, preserves its numeric
status or terminating signal, and forwards directly addressed INT, TERM, HUP,
QUIT, USR1 and USR2 signals to its unreaped child. Command arguments, credentials,
working directory and ordinary environment are inherited; the internal report
correlation variable is removed before actual execution. The scope description
is generic, rather than a copy of the command arguments. There is an additional
observer PID: killing or stopping that observer alone does not terminate the
game or revoke the daemon's independently tracked profile hold. Full launcher
and job-control compatibility still requires real-runtime validation.

The production socket binds registration to SO_PEERCRED and SO_PEERPIDFD, checks
that the original peer is still alive after authorization, and derives the
scope from that peer's procfs identity. It accepts no client-supplied PID, UID or
cgroup path. Missing strong peer identity, cgroup v2 or a dedicated user scope is
an explicit failure. Successful repeated registration for the same UID, token,
game, profile and pinned scope is idempotent.

The scope directory is pinned on the real cgroup v2 filesystem. Recursive
populated state includes detached descendants. Recreating a collected unit name
cannot revive the old identity. Every observation opens `cgroup.events` through
the pinned directory: a native test reproduced stale kernfs contents when reusing
one open event descriptor with `pread(0)`. Generic observation failures end profile
ownership with interrupted status rather than guessing game exit.

Each session owns a dedicated persistent PPD bus peer. Hold acquisition verifies
provider state and pins its unique service owner. Dropping that peer releases
only its hold; the application never writes a remembered profile value. Manual
daemon profile changes and battery policy take ownership after releasing OMEN's
managed holds. Canceled/lost holds never automatically rearm. Known conflicting
power managers and existing exact board/BIOS opt-in restrictions still apply.

The daemon watches scopes independently of the GUI and wrapper lifetime. Workers
perform remote provider checks separately from cached status reads and thermal
workers. Admission is bounded to four active sessions per UID, sixteen globally
and thirty-two retained records. Failed exec releases promptly; final descendant
exit releases automatically. GUI termination does not end a running session.
Daemon death releases PPD peers, so it does not preserve game profile ownership.

## Desktop launch issues

Game Library shows **Launch issues** for pre-registration wrapper failures and
launch processes that fail after the initial receipt. These reports also load
after reopening the app. A live failure updates the app's feedback bar. Reports
contain a game ID, random report ID, optional related scope token, timestamp,
stage and a closed reason code or numeric exit status. They contain no command,
arguments, captured output, arbitrary backend error text, provider cookie or
reusable profile authority. Detailed worker errors take precedence over a later
generic exit report with the same report ID. Store URI submission and a separately
invoked actual-command wrapper can legitimately produce separate reports.

Up to 64 reports are retained in `launch-issues/issues.json` beside the user's
configuration, in a private 0700 directory with private 0600 files. Writers use
a bounded lock and atomic replacement; readers do not wait for the writer lock.
Invalid data is preserved until **Clear launch issues** explicitly resets it.
If saving fails, the current GUI retains a bounded live report where available;
an independently invoked CLI worker's unsaved report cannot survive its exit.
Permission problems must be repaired before saved reports can be cleared.
The two CLI commands above inspect and clear saved reports without hardware
discovery or a daemon connection. They refuse effective UID 0.

Reports describe launch processes, not verified game lifetime. A nonzero launcher
exit can coexist with a running descendant or a request already delivered to an
existing store client. Review Game sessions and the current profile separately.
Clearing reports does not change game preferences, daemon history or profiles.
In manual mode, late failures remain an instruction to review and restore the
profile manually; no late blind restore is added.

## Restart and recovery

`/etc/omen-linux/managed-game-sessions.json` is a service-owned, private 0600
journal with a version, UID, boot/scope identity and display status. Updates are
atomic and synchronized. It contains no PPD cookie or reusable write authority.
On restart, prior active records become Interrupted and no profile is reapplied.
Invalid/unreadable journals are preserved, reported, and block new managed holds.
An administrator must review and repair that file before starting new sessions.

## Provider behavior and ownership

The [PPD API](https://upower.pages.freedesktop.org/power-profiles-daemon/gdbus-org.freedesktop.UPower.PowerProfiles.html)
provides HoldProfile/ReleaseProfile and reports ActiveProfileHolds. The reviewed
[implementation](https://gitlab.freedesktop.org/upower/power-profiles-daemon/-/blob/1a151ccd8bb42c050960f65ca3dee31effde6b36/src/power-profiles-daemon.c)
has SHA-256 `d60de153c1ff6dfd7db1ec0a163eec5b152791c7dd6ae4d62d432f2079079dea`.
The private protocol fixture follows these details:

- Holds support Performance and Quiet (`power-saver`), not Balanced or Cool.
  Unsupported targets must remain explicit; no compare-and-write substitute
  may be advertised as equivalent ownership.
- Quiet takes priority over Performance when holds overlap. Requested profile
  and effective profile therefore need separate presentation.
- A manual change to a different active profile cancels holds. A request for
  the already-active value is a provider no-op and does not cancel them. An
  observed A-to-B-to-A change still leaves an old hold canceled.
- The provider removes holds when their unique bus caller disappears and
  selects the remaining hold or its last manually selected profile.
- ReleaseProfile uses a cookie. A replacement service can reuse that cookie;
  the adapter must address the original unique service owner.
- ActiveProfileHolds does not include cookies. A random suffix distinguishes
  leases even when their human-readable reason is identical.

The [kernel cgroup documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html#un-populated-notification)
defines populated state recursively and permits removal only after live members
and child groups are gone. [systemd-run scopes](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html)
preserve the caller's execution environment. The wrapper disables systemd argument expansion and preserves the original
argument vector.

## Verification and remaining requirements

The optional native integration targets exercise real desktop-user systemd scopes
and production socket/service paths with a private PPD protocol server. They do
not use the system power bus or perform physical control writes. Checks cover
exact arguments/UID, detached descendants, overlap/priority, per-user quotas,
ordinary-scope refusal, failed exec, explicit release, manual same-value and
external A-to-B-to-A changes, battery policy, daemon SIGKILL/restart, private
journal integrity, authorization failure and corrupt recovery data. Lower-level
checks also cover failed/unconfirmed replies, provider replacement, duplicate
hold descriptions and collected scope-name reuse.

```bash
cargo test --workspace --locked
cargo test --release --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p omen-cli --features native-session-tests --test managed-games-native --locked
cargo test -p omen-hardware --features native-session-tests --test game-session-native --locked
# Repeat the two native targets with --release for release verification.
```

Native targets explicitly require a logged-in user systemd manager, cgroup v2
and a kernel with SO_PEERPIDFD. They are opt-in targets, not silently skipped
workspace tests or alternate production control paths. Native GTK checks cover
setup, actual custom wrapper launch, release, preference changes during play,
GUI termination and wide/light plus narrow/dark layouts. Exact binaries and
results are recorded separately from the preserved revision 15 package.

At the revision 16 milestone, remaining work included verified real launcher/runtime execution, managed
Flatpak wrappers, launcher migration outside the dedicated scope, multiple
desktop/kernel versions, graphical authorization and complete keyboard/screen-reader
testing. A failure before daemon registration can currently appear only on the
wrapper's stderr; the GUI launch receipt does not establish startup success or
provide a durable session record for that case. Direct GUI feedback for those
late startup failures remains an experience gap. Installed-sandbox verification
remains unverified for revision 16 because dependency downloads stalled before
application checks; the prepared test uses a private provider fixture.
A game that deliberately dispatches execution to an existing process outside its
scope is not tracked by this mechanism. PPD/kernel I/O failure is not a guarantee
of physical profile recovery. No new physical profile transaction has been run.
The full application audit remains open in `APPLICATION_AUDIT_IMPLEMENTATION.md`.

## Installed revision 20 sandbox

The exact revision 20 package now has three additional checks under its installed
systemd sandbox in a fresh signed Ubuntu VM. A normal production-polkit grant
allows the desktop wrapper's real peer pidfd/user cgroup; exit releases its private
PPD hold and persists a root-owned private journal. Restart leaves the game
command running and recovers Interrupted history without reapplying authority.
The production server/service implementation uses a fixture backend/private PPD
for these checks; unit sandbox properties remain unchanged. This is not real
store or physical profile validation. See `DEPLOYMENT_VALIDATION_2026-10-28.md`.
Revision 21 repeats those three installed-sandbox checks in its own 39-check
fresh VM suite and adds actual KDE/PAM GUI authorization. The CLI/daemon remain
byte-identical to revision 20. See `DESKTOP_VALIDATION_2026-10-28.md`; real games,
managed Flatpak and physical profile recovery remain open.

## Actual native Lutris/OpenTTD

The unchanged final revision 21 package passes six actual native-game checks in
a separate 39-check Ubuntu/Plasma Wayland VM suite. Native Lutris 0.5.22-1
receives OMEN's normal launch URI and supplies OpenTTD 15.1-1 to its configured
actual-command prefix. The real game's PID/window/user scope and private PPD hold
are observed; normal quit releases the hold, explicit release preserves play,
and daemon restart leaves the game running with Interrupted history. Guest
fullscreen transitions pass through Xwayland/SDL, and each launch must produce
its own native screenshot before recovery input. OpenTTD/data/OpenGFX total
32,194 KiB. The virtual display uses software rendering and reports missing
GPU acceleration. This does not establish physical profile recovery or other
store/runtime compatibility. Exact hashes, retained failures and cleanup are in
`REAL_GAME_VALIDATION_2026-10-28.md` and
`evidence/real-game-v21/validation-summary.json`.
