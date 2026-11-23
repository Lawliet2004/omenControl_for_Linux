# First live profile test — approved and passed

This procedure is prepared for **8C30 / F.10 / kernel 7.0.0-38-generic**. Current
read-only discovery found PPD active, offering performance, balanced and power-saver;
platform_profile sysfs is absent. The actual initial profile must be read immediately
before testing. Approval for REA setup does not authorize hardware writes.

Before any write:

1. Finish unit/process tests and package inspection; verify discovery still matches.
2. Use a fresh temporary configuration with battery automation disabled. Confirm
   no TLP/tuned/auto-cpufreq process, PPD hold, or conflicting control service.
3. Record `powerprofilesctl get`, PPD's advertised choices and holds, AC/battery,
   board/BIOS/kernel and current daemon configuration.
4. Obtain explicit approval for this concrete test: request **Balanced** once,
   verify readback, then restore the exact original profile and verify it.
   No fan, RGB, charging, undervolting, firmware or graphics writes are included.

After approval, run the daemon with `--enable-profiles-for 8C30:F.10` under its
appropriate installed authorization boundary and isolated configuration. Send
`SetProfile(Balanced)` through production IPC. A test supervisor must restore the
original profile in a finally/trap path, including timeouts and failures; record
both requested and read-back states. Stop the test daemon and verify recovery.
If polkit or privilege prerequisites are unavailable, stop and document that
blocker; do not bypass authorization or relabel an unauthenticated CLI call as
validation of the daemon's authorized path.

One successful transaction does not validate thermal performance, other modes,
BIOS revisions, suspend/resume, power-source transitions, or automatic per-game
policy. Those remain separate checks. The approved transaction has now passed;
results are recorded below.

## Prepared supervisor

`scripts/profile_test.py` records the original state **before** mutation, checks
exact reference identity, requires live-vs-mock agreement, checks AC and the
backend capability, sets Balanced via production IPC, verifies against PPD's
independent D-Bus property, then attempts restoration in a finally path. It saves
readback and any recovery error. Its fixture mode is exercised by `tests/smoke.py`.

After separate approval and administrator setup:

```bash
python3 scripts/profile_test.py --socket /run/omen-linux/ctl.sock \
  --output docs/evidence/live-profile-test.json --approve-balanced-restore
```

The argument is an explicit operator confirmation; it is not authorization for
this chat to run the command before user approval. The daemon must already be
configured for the exact opt-in, with a fresh configuration disabling battery
automation. The default installed daemon remains read-only. `sudo -n` was unavailable; the approved administrator setup succeeded through
standard pkexec authentication. No authorization bypass was used.

## Recorded result

The user explicitly approved this test. The reviewed polkit action file was
installed root:root 0644 at `/usr/share/polkit-1/actions/com.omenlinux.policy`. A
real, unprivileged daemon with `--enable-profiles-for 8C30:F.10` ran with an
isolated private directory and fresh configuration disabling battery automation.
The production authorization path accepted the UID 1000 active-session client.

* Original: **Performance**, confirmed independently against PPD.
* Requested: **Balanced**, confirmed through daemon and independent PPD reads.
* Restored: **Performance**, confirmed through both reads and again after daemon
  exit. The daemon exited 0 and removed its socket.

The machine was on AC, PPD advertised all three standard profiles, no hold was
active, and no known competing power manager was detected. Evidence is in
`evidence/live-profile-{preflight,test,run}.json` and `live-profile-daemon.log`.
PPD uses amd_pstate here; no firmware `platform_profile` interface exists. This
validates one Linux CPU power-policy transaction, not Windows thermal semantics
or physical performance. No fan/RGB/charging/graphics/firmware control was tested.
The action file remains installed; neither a systemd service nor the .deb was
installed. Future tests require review of their own scope and remaining gates.
