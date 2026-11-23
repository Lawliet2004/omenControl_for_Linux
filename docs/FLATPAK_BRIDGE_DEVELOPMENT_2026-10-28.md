# Flatpak bridge development

This report records the earlier sibling-scope prototype and its exact evidence.
The current unreleased implementation uses a nested cgroup and a native wrapper;
see [the subsequent milestone](FLATPAK_NESTED_BRIDGE_2026-10-28.md). The historical
results below do not establish preservation of per-app resource limits.

This is **unreleased revision 26 work**. The bridge works with a local Flatpak
fixture, but automatic setup remains disabled in the GUI. No revision 26 package
is delivered here. Revision 25 remains the latest finalized package.

## Implementation

The sandbox wrapper waits for host approval, then executes its original argument
vector in place. The host receives only the game ID, instance ID and caller PID;
it never receives or executes the game command. It verifies the instance,
namespace, UID and original process identity, with bounded lookup/reads and a
pinned process descriptor.

The user's systemd manager creates an OMEN scope containing the host helper and
existing sandbox caller through its PIDFDs property. There is no numeric-PID
attachment fallback. The root daemon authorizes its existing StartManagedGame
request from the host helper's actual socket peer. It accepts no new PID, UID,
cgroup-path or command authority. Approval requires shared scope membership and
a confirmed profile hold. The original sandbox caller then executes the game.
The daemon follows the scope independently after the host helper exits.

The inspected official [systemd scope implementation](https://github.com/systemd/systemd/blob/v259/src/core/dbus-scope.c)
and [serialization](https://github.com/systemd/systemd/blob/v259/src/shared/bus-unit-util.c)
define the descriptor property. The bridge uses the standard user bus. An
initial private-socket attempt hit a zbus reply-sender parsing panic; that failed
run is retained separately. No crate version/checksum changed: the client adds
a direct dependency on the already locked zbus 5.19.0.

## Verified results

The current source passes **277 Rust tests per debug/release profile**, strict
all-target/all-feature Clippy and formatting checks.

A fresh VM passes **43 checks**, including ten Flatpak boundary/handoff
observations and two reboots. The development CLI runs separately from the
installed binaries. The adopted live process retains its five namespace
identities, UID/GID/groups, effective capabilities, NoNewPrivs, seccomp mode/filter
count, environment hash, cwd and start identity. Its private Performance hold
survives host-helper exit and releases after actual sandbox exit.

A separate fresh VM passes **40 checks**, including seven real HostCommand
wrapper workflows and two reboots. The root service retains its installed
sandbox and normal polkit path, with a private PPD protocol provider:

1. The unchanged wrapper uses real flatpak-spawn HostCommand and authorized
   root-service registration.
2. Missing org.freedesktop.Flatpak permission prevents game execution and holds.
3. The wrapped sandbox command returns nonzero exit code 7 unchanged.
4. Empty, spaced, dollar and Unicode arguments, dynamic environment, cwd and
   syscall filtering survive. A detached child retains its profile after command
   exit; the hold ends while the launcher remains alive.
5. Explicit release ends only profile ownership; the command continues.
6. Service restart releases the provider peer and records Interrupted without
   stopping the command or reapplying a profile.
7. Normal polkit refusal prevents command execution and leaves no hold.

The wrapper suite overlays only the development CLI inside the guest, then
restores exact installed bytes before reboot/package lifecycle checks. These
are development overlays on finalized revision 25, not revision 26 package
validation. Flatpak 1.16.6 and the additional flatpak-xdg-utils 1.0.6-2 come from
signed Ubuntu indices. The latter archive is 21,610 bytes. The 106-archive
repository is 135,567,360 bytes, SHA-256
a2f28de31afbd63c5dcc57ae934fd1fb7d80db5d4e15b4f52b9bdbed6395ba57.

Positive fixture cases explicitly declare the host-command permission required
by the [official command reference](https://docs.flatpak.org/en/latest/flatpak-command-reference.html).
No real app or global permission override is changed. Failed transport, overlay
setup and stale-instance cleanup runs remain separately retained. Every VM
exits and removes its temporary disk, seed and keys; wrapper runs restore the
installed guest CLI and remove their fixtures.

## Required before enabling setup

- Validate terminating signals, cancellation, overlaps and further HostCommand
  failures with the real wrapper.
- Preserve the caller's ancestor slice and handle existing per-app scope
  resource policies. The proof covers the default app.slice fixture; custom
  scope resource-policy equivalence is not established.
- Exercise a real small game and supported Flatpak launcher. The local
  shell/runtime fixture is not a game, Flathub package or store compatibility
  result. Native Lutris/OpenTTD retains its separate revision 21 attribution.
- Add capability/permission checks, safe placement in existing app data,
  setup/removal guidance and accessible GUI integration. Discovered Flatpak
  entries remain disabled for automatic setup; a custom command that merely
  starts a Flatpak client is still not a valid actual-command wrapper.
- Build/validate the final package after integration. Broader distribution,
  desktop, complete accessibility and physical hardware gates remain open.

Per-run source snapshots, exact binary/script hashes and results are in
evidence/flatpak-bridge-development/validation-summary.json. No host package is
installed, no host power provider is contacted, and no physical control write
occurs. English-only presentation remains the accepted scope. The audit remains
active.
