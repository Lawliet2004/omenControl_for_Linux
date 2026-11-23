# Systemd VM validation

## Revision 22 control-page addendum

Revision 22 retains separate VM identities for its earlier candidate and final
curve-range-corrected candidate. The final documentation package must match the
final tested candidate's runtime/control payload. Exact checks, package hashes,
signed repository, graphical authorization/reboots and cleanup are recorded in
`CONTROL_PAGE_RECOVERY_2026-10-28.md` and
`evidence/control-recovery-v22/validation-summary.json`. Earlier revision 21
results below remain attributed to their original package.

## Revision 21 real-game addendum

The unchanged final revision 21 package passes a separate 39-check fresh VM run:
33 installed-service/recovery checks plus six actual native Lutris/OpenTTD checks.
Three real game launches cover the command prefix, user-scope/profile lifetime,
normal exit, explicit release and daemon restart, with guest fullscreen changes
and native screenshot input responses. The profile provider is private and the
framebuffer uses software video. Two guest reboots pass; all own VM disks/seeds/
keys are removed. See `REAL_GAME_VALIDATION_2026-10-28.md` and
`evidence/real-game-v21/validation-summary.json` for exact package/index hashes,
commands, retained failed attempts and limits. Use `tests/systemd_vm.py --real-game`
with the native-session helper and the signed main/universe/multiverse repository;
the full guest package list and invocation are retained in that evidence.

## Revision 21 desktop addendum

Revision 21's candidate passes 39 checks in a fresh signed Ubuntu VM, including
real Plasma Wayland/KDE portal/PAM authorization, natural approval expiry,
installed managed-session sandbox behavior and two reboots. QEMU exits zero and
removes its private inputs. The final documentation package is compared for
identical runtime/control payload. See
[desktop validation](DESKTOP_VALIDATION_2026-10-28.md) and
`evidence/desktop-v21/validation-summary.json`. Physical hardware/input, other
distributions and full-screen behavior remain unvalidated. Earlier results below
retain their original attribution.

Revision 14 passed 30 checks, including the independently managed fan recovery
writer and NVIDIA reader, across two real guest reboots. The VM does not validate
physical HP fan response or a graphical desktop. The host package and host service
remain uninstalled. Revision 13's 17-check result is preserved under
`evidence/revision13/`.

## Revision 20 addendum

The unchanged final revision 20 package now passes 33 headless systemd VM checks,
including the three installed-sandbox managed-session workflows and two guest
reboots. The guest installs genuine distro dependencies from a locally staged,
signature-checked repository with restricted networking. Exact identities,
commands, initial bootstrap failures and cleanup are in
[the deployment addendum](DEPLOYMENT_VALIDATION_2026-10-28.md) and
`evidence/deployment-v20/validation-summary.json`. Older results below retain their
original attribution. Graphical and physical release gates remain open.

## Inputs and isolation

The Ubuntu 26.04 amd64 cloud image is downloaded from the
[official release directory](https://cloud-images.ubuntu.com/releases/resolute/release/).
Its SHA-256 checksum is verified against the signed manifest using the system's
Ubuntu cloud-image keyring. The recorded image is 859090944 bytes with SHA-256
`5645d0318ad21f643ebb1ac4567514620e9893380b07b0201fda163baa5b7eb5`;
the signer fingerprint is `D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81`.
`evidence/vm-image.json` and `vm-image-signature.log` retain the verification.

`scripts/prepare-vm-runtime.py` resolves Ubuntu's QEMU dependencies using apt's
existing package metadata, downloads archives over HTTPS, verifies their apt
checksums and extracts them into `target/vm-runtime`. It never installs host
packages or executes package maintainer scripts. Package versions and archive
fingerprints are in `evidence/vm-runtime-packages.json`.

QEMU runs as UID 1000 using the existing KVM device permission, two virtual CPUs,
1 GiB RAM and a disposable qcow2 overlay. It has a host-side seccomp sandbox,
no device passthrough, no shared host folders, and user-mode networking with SSH
forwarded to localhost only. A temporary key authorizes the VM's default user;
password login is disabled. The guest's boot ID is checked against the host's,
and the payload requires systemd PID 1 plus QEMU/KVM identity before proceeding.
The seed, overlay and private key are removed after normal completion/failure.
The explicit debug option retains failed-run files after stopping the VM.

## Reproduce

On the Ubuntu development host, with apt metadata, curl, genisoimage, SSH and
the Ubuntu cloud-image keyring already available:

```bash
python3 scripts/prepare-vm-runtime.py
mkdir -p target/vm-images
cd target/vm-images
curl -fLO https://cloud-images.ubuntu.com/releases/resolute/release/SHA256SUMS
curl -fLO https://cloud-images.ubuntu.com/releases/resolute/release/SHA256SUMS.gpg
gpgv --keyring /usr/share/keyrings/ubuntu-cloudimage-keyring.gpg SHA256SUMS.gpg SHA256SUMS
curl -fLO https://cloud-images.ubuntu.com/releases/resolute/release/ubuntu-26.04-server-cloudimg-amd64.img
cd ../..
python3 tests/systemd_vm.py target/debian/omen-linux_0.1.0-14_amd64.deb \
  --apt-mirror https://mirrors.edge.kernel.org/ubuntu
```

The harness verifies the signed image checksum again before boot. This method
follows [cloud-init NoCloud](https://docs.cloud-init.io/en/latest/reference/datasources/nocloud.html)
and [QEMU invocation](https://www.qemu.org/docs/master/system/invocation.html)
documentation. Its apt setup changes only the disposable guest, uses HTTPS and
archive signature checks, and installs the actual package with distro libraries.

## Checks and discovered defects

The suite covers service startup and UID 1000 reads; effective capabilities,
NoNewPrivileges and seccomp; actual read-only/writable mount paths, hidden home
and private tmp; production admin authorization with temporary VM-only grants;
stop/start/restart; guest-only SIGKILL/on-failure restart; enabled and disabled
boot behavior; disabled/masked/enabled reinstalls; removal, reinstall and purge.
No hardware-control operation is requested. Admin saves keep automation disabled.

The original unit's `ReadOnlyPaths=/sys` left sysfs writable on the tested
systemd 259.5/kernel 7.0 image. Adding `ProtectKernelTunables` protected proc
tunables but did not correct that sysfs view. An explicit `BindReadOnlyPaths=/sys`
produced the required read-only view while keeping monitoring paths readable.
Revision `0.1.0-3` includes that bind and kernel-tunable protection. The observed
before/after evidence is retained in `vm-before-kernel-protection.json`,
`vm-kernel-protection-first-probe.json` and `vm-bind-ready-probe.log`.

The verifier checks the service's actual `/proc/PID/root` view with `statvfs`,
independently of unit configuration, and cross-checks namespace mount data.
Covered lower mounts may remain writable in mountinfo; they do not determine
the visible path's access mode. See the
[Linux proc-root documentation](https://man7.org/linux/man-pages/man5/proc_pid_root.5.html)
and [nsenter root/namespace options](https://man7.org/linux/man-pages/man1/nsenter.1.html).

A separate live-manager experiment confirmed a legacy limitation: upgrading
an enabled/running `0.1.0-1` stops/disables it in that old package's own prerm.
The new postinst cannot reconstruct the administrator's prior choice. It keeps
the resulting state and reports a manual re-enable command. Offline container
migration tests do not exercise that old runtime hook. The exact before/after
states and commands are in `evidence/vm-legacy-running-upgrade.log`.

## Results and limits

Revision `0.1.0-13` passed **17 checks**, including two guest reboots, on kernel
7.0.0-38-generic and systemd 259.5. The guest shut down with QEMU exit code 0;
its disk, seed and private key were removed. Canonical's archive endpoint failed
during a preceding attempt, so the successful run used the Linux Kernel
Archives' Ubuntu mirror over HTTPS, retaining Ubuntu archive-signature checks.
No host apt sources were changed.

Consult `evidence/vm-systemd.json` for the run's status, individual checks, package
fingerprint, kernel/systemd versions, runtime namespace data and cleanup state.
`vm-systemd-run.log`, per-boot service journals and serial output retain runtime
evidence. Final documentation-only repackaging is checked with
`tests/package.py --vm-reference TESTED.deb FINAL.deb`; the non-documentation
payload, permissions, maintainer scripts and control metadata must match.

This suite does not establish sensor accuracy, fan crash recovery, suspend/resume,
thermal stability, firmware behavior, or profile switching on an HP machine.
Graphical authentication prompts/cancellation/cache expiry, full KDE/Wayland/X11
installation, other distributions and upgrade-failure rollback remain separate
validation gates. The earlier approved physical profile test was not repeated.

An earlier revision 9 bootstrap attempt failed guest DNS resolution before
package installation. It exited and removed its private inputs; the failed
record is `evidence/vm-systemd-revision9-bootstrap-dns-failure.json`. A fresh
retry and the final runtime build both passed. That setup failure is not counted
as an application test pass; current package/binary identity is in the final
VM record and package equivalence check.

## Revision 12 rerun

The Automatic/Maximum runtime archive passed all 17 checks with two guest reboots,
QEMU exit 0 and removal of its disposable disk, seed and key. Exact archive hash
and final documentation-only equivalence are in `evidence/validation-summary.json`.
This deployment test does not validate a physical HP fan or independent production
fan recovery; live writes remain disabled.
