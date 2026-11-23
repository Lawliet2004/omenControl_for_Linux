# Revision 20 deployment addendum

On 28 October 2026, the unchanged `0.1.0-20` package passed **33 headless systemd
VM checks**, including two guest reboots. The tested package SHA-256 is
`62a8d612dceab7395ad01283abf620b88daf04c381c42f8a073d46ec98353cdb`.
No application or packaging source changed for this validation. The existing
package and revision 20 evidence remain immutable; bundled documentation predates
this addendum. Current results are in
`evidence/deployment-v20/validation-summary.json`.

| Coverage | Checks | Evidence established |
| --- | --- | --- |
| Installed service and package | 17 | Production socket/UID reads, real network counters, effective capabilities/seccomp, actual mount permissions, polkit denial/grant/revocation, stop/start/restart, SIGKILL recovery, enabled/disabled boots, reinstall/mask/remove/purge |
| Independent coarse recovery | 9 | Separate writer cgroup, exact fixture attribute access, controller/writer death, lease expiry, stale ownership refusal and pending-intent removal protection |
| Isolated NVIDIA telemetry | 4 | Static socket activation, capability-free reader, private endpoint, fixture attribution, freshness refusal and recovery without widening device access |
| Managed game sessions | 3 | Authorized desktop wrapper with real peer pidfd/user cgroup, final-exit hold release and private journal, service restart preserving the command and interrupted history |

The normal installed daemon runs first. Managed-session checks then use the
production server/service implementation with a fixture backend and private PPD,
under the installed unit's unchanged sandbox. Only its executable is temporarily
overridden. This proves that the sandbox permits the required peer/cgroup
observation; it does not validate real store games or a physical profile change.
All fixture overrides, rules and helper state are removed afterward.

## Signed offline installation

The guest boots from the existing signed Ubuntu 26.04.1 cloud image, verified
again before launch with the Ubuntu cloud-image keyring. It uses kernel
`7.0.0-38-generic`, systemd `259.5-0ubuntu3.4` and QEMU `10.2.1`.
The desktop user runs QEMU without device passthrough or shared host folders.
`restrict=on` isolates the user-mode network except for the explicit SSH forward
bound to `127.0.0.1`. Package installation uses only a guest-local file repository.

The repository contains three Ubuntu archive-signed InRelease files and matching
main/amd64 package indexes. Their signatures and uncompressed index SHA-256 values
are verified on the host; native APT verifies them again in the guest. Package
archives are checked against SHA-256 values from those signed indexes. The weak
MD5 values printed by `apt-get --print-uris` are not used as the trust source.
No signature/date check is disabled and no `trusted=yes` source is introduced.

A separate disposable guest resolved the actual dependency set without
installing the app. Its status is `resolved`, with application tests explicitly
false. Of the 95 required archives, 66 were already verified in local caches and
29 more, totaling 16,961,004 bytes, were downloaded through alternate registered
Ubuntu mirrors. The 66 cache hits include a newly verified GTK preflight archive;
including that preflight, this run fetched 30 full archives totaling 20,512,990 bytes. All 95 final archives passed strong checksum verification. The resulting
53,473,280-byte repository has SHA-256
`8aeeeb8cb5bef30b8086256fc95a5b0dd2e6cd9af9139fb8c4f33e89863fdefd`.
The checked-in preparation script reproduced that exact archive from the retained
inputs and cache. It never installs host packages.

These inputs are a dated snapshot. Refresh the signed indexes when their validity
window expires; do not bypass APT freshness checks. Mirror registration alone is
not the integrity guarantee: archive signatures and package hashes provide it.
See [Ubuntu's mirror registry](https://launchpad.net/ubuntu/+archivemirrors),
[NoCloud configuration](https://docs.cloud-init.io/en/latest/reference/datasources/nocloud.html)
and [network configuration](https://docs.cloud-init.io/en/latest/reference/network-config-format-v2.html).
The QEMU network option is documented in the locally retained runtime's
`system/invocation.html` and the [QEMU manual](https://www.qemu.org/docs/master/system/invocation.html).

## Reproduce

Use the existing verified VM runtime/image described in `VM_VALIDATION.md`.
The archived index inputs below are public signed repository metadata.

```bash
python3 scripts/prepare-offline-vm-repository.py \
  --indices docs/evidence/deployment-v20/indices \
  --destination target/offline-vm/bootstrap

OMEN_TEST_EVIDENCE_DIR=target/offline-vm/resolution python3 tests/systemd_vm.py \
  target/debian/omen-linux_0.1.0-20_amd64.deb \
  --offline-repository target/offline-vm/bootstrap/metadata-repository.tar \
  --resolve-only

python3 scripts/prepare-offline-vm-repository.py \
  --indices docs/evidence/deployment-v20/indices \
  --destination target/offline-vm/complete \
  --resolution target/offline-vm/resolution/vm-dependency-resolution.txt \
  --cache target/deployment-v20/offline-repository \
  --mirror https://mirror.pilotfiber.com/ubuntu/ \
  --mirror https://mirror.xtom.com.hk/ubuntu/

OMEN_TEST_EVIDENCE_DIR=target/offline-vm/validation python3 tests/systemd_vm.py \
  target/debian/omen-linux_0.1.0-20_amd64.deb \
  --offline-repository target/offline-vm/complete/complete-repository.tar \
  --managed-game-helper target/release/deps/managed_games_native-50cfcaebb87f067b
```

The helper executable name is build-dependent. Build/discover the release
`native-session-tests` target as described in `MANAGED_GAME_SESSIONS.md`, then
restore the normal release build before testing its CLI. No helper is included
in the installed production package.

The first bootstrap requested optional command-not-found metadata that this
binary-only repository does not contain; the source now explicitly selects the
Packages target. A second bootstrap reported an interface-rename warning; the
seed now matches the virtual NIC without renaming it. Making this isolated
interface optional for boot and ignoring unavailable external DNS/NTP avoids
waiting for network access that QEMU deliberately blocks. Neither failed bootstrap
ran application tests. Both exited with cleanup, and their records are retained.

## Limits and cleanup

The final guest shut down with QEMU exit code zero. Its overlay, cloud-init seed,
private SSH key and host-key file were removed. The source image, pre-existing
debug overlay, original dependency container and unrelated running app processes
were preserved. No host package/service was installed and no physical control
write was performed. Repository/cache artifacts remain under `target/` for reuse.

This closes revision 20's headless installed-service VM gate. It does not close
graphical polkit interaction, a complete graphical desktop/distribution install,
physical shortcuts/focus/full-screen placement, real store/runtime compatibility,
complete accessibility, or HP fan/RGB/charging/graphics/suspend validation. The
full application audit remains active.
