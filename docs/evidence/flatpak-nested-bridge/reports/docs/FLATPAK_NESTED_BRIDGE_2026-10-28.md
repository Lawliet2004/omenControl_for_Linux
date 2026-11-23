# Flatpak game groups beneath the original app scope

Unreleased revision 26 now keeps the game beneath its existing Flatpak cgroup.
The development build passes 45 installed VM checks, including 12 Flatpak
workflows and two reboots. Debug and release each pass 278 Rust tests; strict
Clippy and formatting checks pass. Revision 25 remains the latest finalized
package. Automatic Flatpak setup is still disabled in the GUI.

## What changed

The earlier sibling-scope prototype could move a game outside limits applied
specifically to its original Flatpak scope. Copying the limits into a sibling
would create a separate allowance and would not preserve aggregate enforcement.
The new bridge creates a user-owned `omen-game-TOKEN.scope` cgroup directly below
the caller's original group. This is a nested kernel cgroup, not another systemd
unit. It keeps all existing ancestors; the bridge changes no controller,
delegation setting, parent policy or Flatpak permission. The kernel applies
parent limits to descendants, as described in the
[cgroup v2 documentation](https://www.kernel.org/doc/html/v6.8/admin-guide/cgroup-v2.html).

A small static musl executable replaces the shell wrapper. It creates a private
Unix packet socket before forking `flatpak-spawn --host --watch-bus`, forwarding
one endpoint through `--forward-fd`. The host receives the game ID, instance ID
and socket descriptor. It receives no game command, environment or caller PID.
Socket credentials and `SO_PEERPIDFD` pin the original creator across PID
namespaces. The host also verifies its UID, Flatpak instance metadata, separate
PID namespace and current cgroup. Unsupported process-descriptor kernels fail
closed; there is no numeric-PID or process-scan fallback.

The desktop-user helper pins the actual cgroup v2 directory, creates a random
child and sends only its writable `cgroup.procs` descriptor back. The wrapper
checks the filesystem, descriptor and random group name, writes zero to move
itself, and confirms its new membership before acknowledging. The helper then
moves itself through the same descriptor. Parent memory, CPU, task and other
hierarchical policies continue to apply to both processes.

The root daemon uses its existing authorized `StartManagedGame` request. Its
cgroup observer accepts a dedicated game child below a Flatpak app scope and
still derives identity from the actual pinned IPC peer. There is no new
privileged PID, UID, path or command API, and its cgroup mount stays read-only.
Only after a verified hold does the wrapper execute its original argument vector
in place. It preserves its environment, directory and inherited signal settings.
Automatic-off mode executes without creating a group or requesting a hold.

The helper trims only empty groups bearing an exact OMEN token beneath its
pinned user-owned parent; active groups are bounded. Application-scope collection
removes the hierarchy after its processes exit. No persistent watcher or user
service is installed. The `flatpak-spawn` options are documented by
[Flatpak](https://docs.flatpak.org/en/latest/flatpak-command-reference.html#flatpak-spawn).

## Exact validation

`target/flatpak-nested-bridge/wrapper-attempt-2/runtime/vm-systemd.json` records:

- 45 passing checks, including 12 Flatpak workflows, real service sandbox and
  normal polkit, two reboot transitions, guest-only CLI restoration and cleanup.
- A local Flatpak fixture running the native static wrapper through real
  HostCommand and a signed distro `flatpak-spawn`, with a private PPD provider.
- Actual sandbox membership beneath the original app scope while its
  `memory.max=268435456`, `cpu.max=50000 100000` and `pids.max=64` remain unchanged.
  That parent exposes no controllers to the child; child controller files are
  consequently absent. This is valid hierarchical enforcement. No controller
  was enabled to make the test pass. This checks placement and policy retention,
  not a workload stress measurement of every controller or BPF policy.
- Exit 7 preserved, paired direct/wrapped SIGTERM status 143, inherited ignored
  SIGCHLD, literal/empty/Unicode arguments, dynamic environment and directory,
  NoNewPrivs and seccomp filtering, automatic-off mode, detached-child lifetime,
  explicit release, restart interruption and normal authorization denial.
- The launcher remains alive after the final game child exits and releases the
  hold. Releasing a hold or restarting the service leaves the game running.

The first wrapper attempt passed exit, SIGTERM and ignored-SIGCHLD cases, then
failed because its test incorrectly expected child controller files to exist.
The corrected test observes the actual hierarchy and unchanged parent limits.
Both attempts restore the guest CLI and clean their fixtures; the failed run is
retained separately and is not counted as a passing suite. A separate preliminary
41-check run proves unprivileged child creation/migration under those limits,
using the earlier prototype; it does not validate the new native wrapper.

Exact retained development artifacts:

| Artifact | SHA-256 |
| --- | --- |
| CLI | `70a59e6ccdc37cc3dd907e31af7a9e81054cc5178ec143e0da06038fe2a9f562` |
| Native sandbox wrapper, 82,536 bytes | `f11b3d68396a8b30d331cf39499b66a74bb15260e41b7c220287e519f964c853` |
| Root-service/private-provider test helper | `151c0c32947632d88dea295ef2a7bf29fcced95fc8721cf4a3358984ed27028a` |
| Finalized revision 25 base package | `c84e5e60bc2e0b7254cf0430e68030c0e2681ff13913c39336621fe7068f5e54` |

The root fixture executable contains the new observer; the installed revision 25
daemon has not silently acquired this code. Its installed service sandbox and
production polkit policy surround the new fixture service. The separate CLI
overlay is restored byte-for-byte before reboot/package lifecycle checks.

`packaging/build-flatpak-wrapper.py` builds from an explicitly supplied musl
sysroot, rejects existing outputs and checks for static linking and a
non-executable stack. A second build reproduces the tested wrapper byte-for-byte.
The musl packages are verified against signed Ubuntu indices, extracted locally,
and never installed on the host. Their full copyright notice accompanies the
published wrapper. The package builder does not yet include this development
helper. The client no longer needs the prototype's direct zbus dependency.

The separate publication at `evidence/flatpak-nested-bridge/` retains sources,
exact binaries, build/VM logs, signed-input provenance, failed-run details and
checksums. Earlier evidence publications and packages remain unchanged.

## Remaining integration work

This uses a local fixture, not a real game or a Flathub launcher. Native
Lutris/OpenTTD remains the separate real-game result; it cannot validate Flatpak.
Still needed: a small real Flatpak game and launcher path, approval cancellation
and wrapper death during preparation, inherited file descriptors and broader
job-control behavior, overlapping sandbox sessions, and user-facing setup,
permission guidance, removal and recovery. Further controller policies and
cgroup-namespace variants need compatibility coverage. No physical hardware,
GUI presentation, screen-reader or broader distribution validation is claimed.
