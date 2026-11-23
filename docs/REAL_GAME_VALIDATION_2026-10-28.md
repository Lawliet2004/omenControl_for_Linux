# Native Lutris/OpenTTD validation — 28 October 2026

The unchanged revision **0.1.0-21** package passes **39 checks** in a fresh
Ubuntu 26.04/Plasma Wayland VM, including **six real-game checks**. Three
launches use the signed distro OpenTTD game, free OpenGFX graphics and native
Lutris Linux runner. The installed game/data/graphics total **32,194 KiB**
(about **33 MB**), within the user's requested 2 GB limit. The application
remains English only.

This validates one native game's launcher, actual process lifetime and managed
profile behavior. Profile changes use a private PPD fixture; no physical profile
write or host package/service installation occurred. The original desktop/PAM
39-check suite remains separate: this run repeats its 33 non-graphical checks
and substitutes six game checks for its six authorization workflows.

## Exact inputs

| Input | Version / fingerprint |
| --- | --- |
| OMEN package | `omen-linux_0.1.0-21_amd64.deb`, 3,645,506 bytes |
| Package SHA-256 | `25964019083530e3c3e567dca7edd13cb4eebe1ce0ad2eea8aacc7f301ae3edf` |
| Lutris | `0.5.22-1`, native distro package |
| OpenTTD and data | `15.1-1` |
| OpenGFX | `7.1-3` |
| Plasma workspace | `4:6.6.6-0ubuntu0.1` |
| Signed Ubuntu cloud image SHA-256 | `5645d0318ad21f643ebb1ac4567514620e9893380b07b0201fda163baa5b7eb5` |
| Signed local repository | 658 verified archives; 374,321,066 bytes of package content |
| Repository tar | 471,347,200 bytes; SHA-256 `e484db343b75a7a357eac5e7d3e050b4c4a76acccd537258861f62a955e7334e` |

The repository preparer now accepts signed Ubuntu **multiverse**, which contains
Lutris, alongside main/universe. All three pockets' InRelease signatures,
uncompressed indexes and complete archive sizes/SHA-256 hashes are verified.
Guest APT repeats verification without disabling signature/date checks. Of the
658 archives, 611 came from verified caches and 47 downloads added 64,531,424
bytes. No host APT source was changed. The guest network was restricted to
explicit localhost SSH forwarding; attempted online Lutris migration requests
failed and are retained in its logs.

`dpkg --verify` reports no changed files in the four game/launcher packages.
The VM result records executable/runner hashes, versions, real game PIDs, UID
1000, arguments, window properties and matching user scope identities. The
installed CLI, daemon and GUI package bytes are unchanged from revision 21.

## Workflow and observed results

Ordinary user configuration was created through the installed Lutris API, using
the same `/usr` resource prefix as its installed entry point. Its game entry
uses `/usr/games/openttd`, a small new world, windowed 960 × 600 video, and
disabled sound/music. Lutris's normal **Command prefix** is:

```text
/usr/bin/omen-cli game-run lutris/1 --
```

The runner and launcher code are unmodified. Runtime downloads are disabled
through existing Lutris environment/configuration options. The installed OMEN
CLI submits `lutris:rungameid/1`; Lutris supplies the actual executable/arguments
to that prefix, following its
[command preparation](https://github.com/lutris/lutris/blob/master/lutris/runner_interpreter.py).
The game's [command options](https://wiki.openttd.org/en/Manual/Command%20line),
[hotkeys](https://wiki.openttd.org/en/Manual/Hotkeys) and
[console quit command](https://wiki.openttd.org/en/Manual/Console%20Commands)
provide the genuine window/input/exit path.

| Check | Observed result |
| --- | --- |
| Discovery and size | Installed OMEN reads the actual Lutris database and discovers `lutris/1` / OpenTTD. Game/data/graphics are below 2 GB. |
| Actual launch and hold | Production URI routing reaches Lutris's normal prefix. `/usr/games/openttd` runs as UID 1000 inside its matching `omen-game-TOKEN.scope`, maps an OpenTTD window and holds Performance through the private provider. A launch receipt remains explicitly unverified until those observations. |
| Fullscreen transition | Guest-only keyboard input enters and exits fullscreen on Plasma Wayland with an Xwayland/SDL game. The real window state changes and the profile hold stays active. |
| Normal exit | The game's own console `quit` ends its process/scope. The hold releases, Balanced returns, session history becomes Ended and no saved launch issue remains. The GUI's later poll moves the session into recent history. |
| Explicit release | Releasing the profile through the installed desktop-user CLI returns Balanced while the same real game PID keeps running. Normal quit does not reapply the hold. |
| Service restart | Restart releases the provider peer, records Interrupted and leaves the actual game running. Normal quit then succeeds without reapplying a profile. |

Each of the three launches must respond to its real screenshot hotkey before
recovery/exit input. Native screenshot path, size and SHA-256 observations are
recorded; their image bytes were inside the removed guest. Three separately
retained QEMU captures show the windowed game, fullscreen game and post-exit
library. These final captures were inspected together once. The game renders
its world but displays a **no compatible GPU / hardware acceleration disabled**
notice: the disposable framebuffer has no passed-through GPU/Vulkan driver.
That notice is preserved and does not establish GPU compatibility. Lutris also
logs absent optional Battle.net/protobuf and i386/Vulkan components; the native
64-bit game still executes the tested paths.

The game stage replaces only the installed service's ExecStart with the existing
native integration helper, using production IPC/service/session code and the
fixture backend/private PPD. Its installed systemd sandbox remains unchanged,
including strict system mounts and read-only cgroups. A guest-only polkit rule
grants the normal control action to ubuntu; this stage does not test graphical
authorization or grant the admin action. The original revision 21 KDE/PAM
evidence remains in `DESKTOP_VALIDATION_2026-10-28.md`.

## Failures, cleanup and limits

Four earlier runs are retained as failures, with exact game-test source hashes:

1. `game-vm`: inspected user-manager environment before Plasma exported it.
2. `game-vm-retry`: setup API lacked the installed launcher's resource prefix.
3. `game-vm-ready`: the native OMEN window took focus after game launch;
   Xwayland's root active-window property retained the game. The test now also
   requires the client's focused state immediately before input.
4. `game-vm-final`: five checks passed, but console quit did not complete in the
   restart case. Input readiness had not been observed before commands; the exact
   lost-input cause was not established. The final test requires an actual native
   screenshot response on every launch and passes all six checks.

The final confirmation completed at **06:48:18 UTC**, with 39 checks, two genuine
guest reboots, QEMU exit 0 and removal of its disk, seed and private key. All four
failed VMs and the dependency-resolution VM also exited 0 and removed their
private inputs. The original host GUI, mock daemon, KDE authorization agent,
Docker container and unrelated old stopped VM remain preserved.

This result does not validate Steam/Proton, Heroic, real store accounts, managed
Flatpak execution, controllers, GPU acceleration, performance, prolonged play,
the OMEN fullscreen overlay/global shortcuts, physical input or physical profile
recovery. It establishes one actual native Lutris game path. The full audit stays
open in `APPLICATION_AUDIT_IMPLEMENTATION.md`.

Machine-readable results, commands, signed indexes, source snapshots, retained
failures and hashes: `evidence/real-game-v21/validation-summary.json`. This report
is an addendum; the preserved package's bundled documentation predates it.
