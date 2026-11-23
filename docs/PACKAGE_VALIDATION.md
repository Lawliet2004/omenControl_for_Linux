# Package lifecycle validation

## Revision 24 sensor browser

Revision 24 groups sensors by reported device/provider and type, adds native
filters/comparable sorting, stable reading identities and explicit copy snapshots.
It passes 276 Rust tests per profile, formatting and strict Clippy. Nine browser
workflows pass per profile; both X11 and Wayland pass 31 regressions, and 13
keyboard/Orca workflows pass with the final release GUI. Five confirmation scenes
pass with 190 measured active text-label contrasts. A fresh installed VM passes
38 checks, including the current Cargo-selected helper, five default-source
workflows and two reboots, with cleanup. CLI/daemon bytes match revision 23.
No new KDE/PAM or store-game result is claimed for revision 24. See
`SENSOR_BROWSER_2026-10-28.md` and
`evidence/sensor-browser-v24/validation-summary.json` for exact identities,
retained failures and limits.


Revision 23 verification note: the initial session commands selected a cached
helper. A supplemental run now passes 20 native checks and 38 fresh installed-VM
checks with the current helper and unchanged final package, including two reboots
and cleanup. See `evidence/source-switch-v23-current-helper/` and the correction in
`SOURCE_SWITCHING_2026-10-28.md`. Published evidence and the final package remain
unchanged.


## Revision 23 source recovery

Revision 23 adds verified source reconnection and provenance, immutable action
routes and source-specific editor/history recovery. The source passes 271 Rust
tests per profile, formatting and strict all-target/all-feature Clippy. Six
source workflows and twelve control/keyboard workflows pass per profile; both
native backends pass 31 settings/health/network regressions. Six main-window
source scenes pass with 185 measured active text-label contrasts; six companion
overlay captures were reviewed. The fresh installed VM passes 38 checks,
including five default-source workflows and two reboots. Six real KDE/PAM checks
pass on the identical candidate in an earlier run with a later observer failure;
that entire run is retained as failed. All own VM inputs are removed. Exact
identities and limits are in `SOURCE_SWITCHING_2026-10-28.md` and
`evidence/source-switch-v23/`. Earlier evidence stays attributed
to its tested revision.


## Revision 22 installed controls

The final revision 22 candidate passes 39 fresh desktop VM checks, including
six actual KDE/PAM workflows, natural authorization expiry and two reboots, with
QEMU exit 0 and private-input removal. Its control-page source also has twelve
native recovery/keyboard workflows per build profile and 31 existing regressions
per native backend. The final package is compared with that exact candidate via
`--vm-reference`; runtime files, maintainer scripts and control metadata must
match. See `CONTROL_PAGE_RECOVERY_2026-10-28.md` and
`evidence/control-recovery-v22/validation-summary.json`. An earlier candidate's
separate result predates the curve-range correction. No container or real-game
result is silently inherited from revision 21.

## Revision 21 installed desktop

The revision 21 candidate passes 39 fresh Ubuntu/Plasma Wayland systemd VM
checks, including installation/lifecycle/reboots and real KDE/PAM authorization.
Its static verification also checks 15 exact bundled icon sources and notices.
The final documentation package is compared with that exact candidate using
`--vm-reference`; only documentation/checksums may differ. Runtime files,
maintainer scripts and control metadata must match. See
[desktop validation](DESKTOP_VALIDATION_2026-10-28.md) and
`evidence/desktop-v21/validation-summary.json`. No new container result is
attributed to revision 21; the older container results below retain their own
package identities. Physical and cross-distribution gates remain open.

Revision 14 passed 19 container install, upgrade, authorization and removal checks.
Revision 13's 19-check result and its package fingerprint remain archived under
`evidence/revision13/`. The package is still an experimental development snapshot;
these checks do not prove physical fan behavior.

## Revision 20 offline dependency reuse

Revision 20 passes 20 lifecycle checks and 13 installed launcher-format checks as
UID 1000. The test used a new, isolated container cloned from an existing Ubuntu
26.04 dependency environment. A non-pausing snapshot preserved the original
container's identity, running state, PID and start time. Only the new test
container's old OMEN package/configuration was purged. Distribution dependencies
were reused; the candidate was installed with `dpkg --install`, followed by
`apt-get --no-download check`. No archive update or dependency download was used.
This establishes installed runtime/linkage, policy and lifecycle behavior in that
environment, rather than a pristine dependency-resolution run.

The extra launcher checks execute installed CLI/daemon binaries on private
records and controlled executables. They do not invoke real Steam/Snap clients or
an Epic account. The container and the temporary dependency snapshot image were
removed; no host package/service was installed. Exact identities, versions and
cleanup are in `evidence/launcher-expansion/package-lifecycle.json`,
`installed-game-formats.json` and `dependency-snapshot.json`. The unchanged revision 20
package subsequently passed 33 headless systemd VM checks; see
`DEPLOYMENT_VALIDATION_2026-10-28.md`. Graphical authentication and complete
graphical installation remain open.
The final package's documentation-only changes are checked against the exact
lifecycle-tested candidate with the equivalence verifier described below.

The first offline APT attempt rejected the local package pathname; the harness
now uses dpkg for the local artifact. A second attempt supplied revision 10 as a
baseline, which already had helper state and could not test migration from a
pre-helper package. The passing run uses revision 1; the harness now rejects an
ineligible baseline before creating a container. Both failed containers were
removed and their failure records are retained with revision 20 evidence.

## Reproduce

Use an existing local Ubuntu image, the current package, and the pre-helper
`0.1.0-1` package for migration coverage:

```bash
python3 tests/package_lifecycle.py \
  target/debian/omen-linux_0.1.0-14_amd64.deb \
  --baseline target/debian/omen-linux_0.1.0-1_amd64.deb
```

The harness pins the local base image by ID, copies the packages and test payload
into a new container, and installs distribution dependencies using HTTPS apt.
The minimal image receives the host's public CA bundle for TLS verification;
Ubuntu archive signatures remain checked. Only the required `main` component is
enabled. No image is pulled automatically. It uses the local Docker socket from
the host to manage the container; that socket is never passed into the container.
Tests require network access to Ubuntu repositories and enough disk space for
GTK/runtime dependencies.

The container has separate namespaces, no explicit host mounts/device passthrough,
no SYS_ADMIN capability, no-new-privileges, and a read-only sysfs mount. Its normal
Docker-provided kernel views can expose read-only host information. It receives
no host D-Bus address/socket. The test payload checks these constraints before
changing package/service state. The container is removed in the harness's final
cleanup, including on test failure. The explicit debug option `--keep-failed`
instead records a failed container's ID for inspection and manual removal.
No package is installed on the host.

## Coverage and interpretation

The payload checks these behaviors using actual `apt`, `dpkg`, distribution
helpers, installed binaries, production IPC, `pkcheck`, D-Bus and polkit:

* Dependency resolution and native library linkage on the base distribution.
* Initial private system configuration and offline systemd enablement.
* Reinstall preserving enabled, disabled and administrator-masked states.
* Actual version increase from `0.1.0-1`, which had no helper state, preserving a
  disabled service and existing system configuration.
* Removal/reinstallation retaining configuration and disabled state; purge
  removing package settings/helper links while preserving desktop settings.
* Private production-daemon socket startup/shutdown, with mock mode absent.
* Unprivileged reads and denied mutations when authorization is unavailable.
* Installed polkit actions denying a non-session user without an auth agent.
* A temporary container-only control grant reaching the daemon's refusal to
  launch games, while still being unable to change admin settings.
* A temporary admin grant allowing the same UID to save **disabled** automation
  settings; removal of that grant revoking later requests.
* Bus loss retaining read access while failing closed for mutations.

A temporary `systemctl` recorder exercises the maintscripts' running-manager
branch with `policy-rc.d` returning 101. It verifies that a denied restart is
not executed. This recorder is not a systemd service manager and cannot prove
actual process restarts or sandbox behavior. The production daemon is launched
directly for the IPC tests and stopped by the test harness. No hardware control
operation is requested; control-action authorization is tested using the
daemon's always-refused game-launch API. Service-policy writes leave battery
automation disabled. No fake hardware result is described as a physical test.

The recorded run passed **19 checks** on stock Ubuntu 26.04, including UID 1000
network counter/rate reads without authorization grants. Results, tested package fingerprints, base image digest, dependency versions,
individual checks and cleanup state are in `evidence/package-lifecycle.json`.
Setup, command and daemon/bus/polkit logs are retained alongside it. Consult that
result's status rather than treating this test description as a passing result.

The final package can contain updated documentation after the lifecycle run.
`tests/package.py --lifecycle-reference TESTED.deb FINAL.deb` verifies that every
non-documentation payload entry and its permissions, all maintainer scripts,
and control metadata match the exact successfully tested artifact. Only package
documentation and its checksum list may differ. Both fingerprints and changed
documentation paths are recorded in `evidence/package-validation.json`.

## Packaging behavior

Revision `0.1.0-1` used direct `systemctl enable --now` during configuration and
stop/disable during upgrade. It did not enable the unit in an offline container,
and a running-system upgrade would override an administrator's disabled choice.
The baseline observation is in `evidence/package-lifecycle-before.log`.

Revision `0.1.0-2` uses Debian helpers to track enablement, preserve administrator
choices and honor `policy-rc.d`. Its migration first records existing links when
upgrading an old package that had no helper state. It creates a missing config
with mode 0600, retains existing configuration across upgrade/remove, and
removes the known system config/backup on purge. Unknown files and desktop
settings are retained. The package version comes from the Debian changelog and
must match the Cargo upstream version.

The offline migration results do not prove the old `0.1.0-1` runtime hook preserves
an enabled service. A real-manager experiment found that it disables the unit
before the new package can read the prior choice. A first upgrade from that
development revision may need explicit re-enabling; the new postinst reports
the exception. See `VM_VALIDATION.md` and `vm-legacy-running-upgrade.log`.
Revision `0.1.0-3` also binds sysfs read-only and protects kernel tunables;
the container suite cannot validate those mount restrictions.

These choices follow the distribution's
[deb-systemd-helper lifecycle](https://manpages.debian.org/unstable/init-system-helpers/deb-systemd-helper.1p.en.html)
and [deb-systemd-invoke service policy](https://manpages.debian.org/unstable/init-system-helpers/deb-systemd-invoke.1p.en.html).
The local Ubuntu power-profiles-daemon maintscripts supplied an additional
dh_installsystemd reference; no helper implementation is bundled.

## Remaining validation

This container suite does not test a real systemd PID 1, automatic boot/start/
restart, unit sandbox permissions, or reboot/shutdown recovery; the separate
VM suite is described in `VM_VALIDATION.md`. Logind active/inactive
session classification, graphical admin prompts/cancellation, cached
authentication expiry, upgrade failure/rollback, and a full Kubuntu desktop
install/remove cycle remain unverified by this container suite. It also cannot
verify suspend/resume, physical sensors, thermal behavior or supported hardware
controls. Existing live hardware gates remain in force.

## Revision 12 rerun

The coarse-mode runtime archive passed all 19 container lifecycle checks, including
package installation, upgrade/removal and authorization boundaries. The disposable
container was removed. Its exact hash and final documentation-only equivalence
are recorded in `evidence/validation-summary.json`. No host install or physical
control was performed.
