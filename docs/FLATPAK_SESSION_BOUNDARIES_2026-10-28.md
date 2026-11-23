# Flatpak managed-session boundary investigation

Date: 28 October 2026. This is new runtime evidence for the unchanged revision
24 application. **Managed Flatpak integration is still unavailable.** Passing
observation checks below reproduce failures of candidate wrapping approaches;
they are not a claim that automatic restoration works for Flatpak games.

## Findings

| Experiment | Observed result | Product implication |
|---|---|---|
| Run a real Flatpak app inside an OMEN-shaped user scope | Flatpak moves the sandbox to `app-flatpak-…scope`; the original scope becomes inactive | Host-wrapping `flatpak run` does not retain the process tree that OMEN supervises |
| Installed revision 24 CLI wraps that launch with a private profile provider | The session becomes Ended and the profile returns to Balanced while the sandbox remains alive | A custom command can bypass the discovered-library setup restriction; the session describes scope emptiness, not verified game exit |
| Enter the actual command PID's sandbox from a new OMEN scope | Literal arguments, including empty, quoted, dollar and Unicode values, its dynamic environment and working directory survive; the entered command remains in the OMEN scope | Namespace entry is a possible foundation, with additional identity, permission and execution work still required |
| The entered command exits with code 7 | `flatpak enter` returns 0; ordinary `flatpak run` returns 7 for the same command | Directly using `enter` would hide a failed command |
| The entered command terminates itself with SIGTERM | `flatpak enter` returns numeric code 15 | Terminating-signal semantics are lost |
| An entered command starts a detached six-second child and exits | The installed wrapper returns 0, but the real child remains in the OMEN scope; the private Performance hold persists until final child exit, then releases | Scope supervision can work after namespace entry, but the status defects still prevent support |

The source agrees with these observations: Flatpak creates its own transient
scope, and its `enter` implementation passes a raw wait status to `exit`.
The inspected version is **1.16.6**, pinned to commit
`e761a8885453c217a931281092a641ebbdd0a0c6`:
[scope creation](https://github.com/flatpak/flatpak/blob/e761a8885453c217a931281092a641ebbdd0a0c6/common/flatpak-run.c),
[namespace entry](https://github.com/flatpak/flatpak/blob/e761a8885453c217a931281092a641ebbdd0a0c6/app/flatpak-builtins-enter.c).
These observations are version-specific; newer Flatpak releases were not tested.

## Validation and attribution

The final fresh headless Ubuntu VM passes **40 checks**: the existing 33 installed
service/recovery/session/reboot checks and seven Flatpak boundary observations.
It performs two real reboots. The stage removes its fixture, app/runtime,
remote, newly created user Flatpak directories and helper. The harness stops
its QEMU process and removes its temporary disk, seed and SSH keys.

The VM installs Flatpak 1.16.6 and bubblewrap 0.11.1 from signed Ubuntu indices.
The offline repository contains 105 archives, is 135,546,880 bytes and has
SHA-256 `db5a0f709c2957d9d9115cacbfe7f2d608b20725085003448e33a170b90edea7`.
The minimal runtime is built from those guest OS binaries using the strategy
of Flatpak's own test runtime recipe. Its local fixture repository intentionally
has no signature; it is not a Flathub app or a real game. Build export disables
the build-time sandbox only; actual execution uses Flatpak's sandbox. No global
AppArmor or user-namespace restriction is disabled.

The profile experiment uses the installed ordinary CLI with production session/
socket code, a user-owned mock service and a private PPD protocol server. It
does not validate that experiment inside the production root service sandbox.
The suite separately runs the existing three installed-service session checks
with the current release helper, SHA-256
`f5e322fdb994d9e9bc0f209b7c67c8159a4f33560f52ba659a3fb6a73013a6ad`.

The final package remains `omen-linux_0.1.0-24_amd64.deb`, 3,694,524 bytes,
SHA-256 `1a7eb732fa5c672c2e920396ccdf163b718bb008008bc4cedd69ac7f8bb226df`.
All 132 product-source hashes match the previous revision 24 publication. Only
the VM harness, a new probe and documentation change; no package revision or new
Rust build is claimed. The signed Ubuntu archive provenance is distinct from
the unsigned upstream Git tag/source download. Relevant upstream files match
the pinned commit byte for byte; their hashes and archive are retained.

Three unsuccessful preparation/expanded-probe runs are retained: an app that
had not been finalized, a copied dynamic loader missing executable permission,
and a test scope that expanded a literal dollar argument. The third overall
run passed the initial four-observation probe and all 37 suite checks; the fifth
passes the expanded seven-observation probe. Each run used a fresh VM and
completed VM cleanup. They are not combined into one passing run.

## Work required before automatic Flatpak setup

1. Implement a desktop-user bridge around the launcher's actual game command.
   Pin and verify the caller's Flatpak instance, UID, process start identity and
   namespace descriptors. Do not add caller-supplied PID/cgroup authority to the
   root daemon or treat a launcher URI as game lifetime.
2. Preserve the byte-level argument vector, the actual caller's environment and
   working directory, numeric exit status and terminating signals. A launcher
   instance PID may have different environment/cwd from its game command. The
   unmodified `flatpak enter` shortcut fails the status requirements above.
3. Define setup and recovery for a sandbox-to-host bridge. Official
   [Flatpak command documentation](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
   states that `flatpak-spawn --host` requires access to
   `org.freedesktop.Flatpak`. Detect required existing permission and explain it;
   do not silently grant broad host access. Host and sandbox paths must both be
   valid in the respective execution contexts.
4. Verify pre-execution refusal, missing permissions, stale/reused caller
   identity, exact arguments/environment/cwd, signal forwarding, detached final
   descendants, overlapping holds, explicit release, and service/launcher death.
   Improve migrated-scope status so it cannot imply verified game exit.
5. Exercise the finished bridge with a real small Flatpak game and supported
   launcher versions before enabling automatic setup. Native Lutris/OpenTTD's
   earlier approximately 33 MB real-game result remains separately attributed
   to revision 21 in `REAL_GAME_VALIDATION_2026-10-28.md`.

Evidence is published in `evidence/flatpak-scope-probe-v24/validation-summary.json`,
including exact per-run inputs, results, command logs and source provenance.
No host installation, host power-provider call or physical hardware control
write occurred. Existing packages and evidence remain unchanged. English-only
presentation remains the requested scope; broader launcher, desktop, physical
hardware and full accessibility requirements remain open.
