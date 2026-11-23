# Managed-session scope migration recovery

Revision 25 retains the registering process's pinned proc directory and original
start identity throughout scope supervision. If that live peer moves outside
the dedicated game scope, observation stops, its PPD hold releases, and the
session becomes **Interrupted**. Its detail explains that a game may still be
running. Normal completion says that the tracked scope is empty.

This addresses the misleading Ended result reproduced with real Flatpak 1.16.6
in `FLATPAK_SESSION_BOUNDARIES_2026-10-28.md`. It does not implement a managed
Flatpak bridge or enable automatic setup for sandboxed launchers.

The pinned process directory cannot become a replacement process when its PID
is reused. Membership checks accept the original scope and its nested groups,
with a full path-component boundary. A dead or zombie registering peer does not
end a populated scope: detached descendants retain existing scope supervision.
Read/access/identity failures interrupt observation rather than prove exit.
The [kernel proc documentation](https://docs.kernel.org/filesystems/proc.html)
describes the pinned-descriptor behavior. No new client-supplied PID, UID or
cgroup authority is accepted by the root service.

## Validation

- Debug and release each pass 276 Rust tests, 21 native CLI/service/session
  checks, and 12 lower-level profile/scope checks. Strict all-target/all-feature
  Clippy and formatting checks pass.
- The added native case first acquires a private Performance hold, then moves
  the registered live process into another real user scope. It confirms that
  the process remains alive, the profile returns to Balanced, and Interrupted
  with no held profile is durably recorded. Ordinary exit, detached descendants,
  overlap, signal forwarding, peer replay refusal and scope-name reuse retain
  their existing checks.
- A fresh installed Ubuntu VM passes 40 checks: 33 service/recovery/session/
  reboot checks plus seven Flatpak boundary observations. The installed CLI
  now records Interrupted while the real Flatpak sandbox remains alive. The
  ordinary entry/exit controls and detached namespace-entry prototype retain
  their earlier results. Two real reboots complete; the VM exits and its
  disk, seed, keys, guest fixtures and helper are removed.

The release helper was selected from current Cargo artifact JSON and retained
at `target/session-migration-v25/artifacts/managed-games-native-release`, SHA-256
`9c9fb81d31b58d42e923c926a3ac5e05b98bddde1db8cbd56e38f65ee83444e9`.
Its source hash is
`f7a36940ec9a4d0abc9d302f6acedd73b07bc510ef85aac507c449e4a16dc0a9`.
The release session run uses the ordinary CLI packaged in the candidate, SHA-256
`fabbc592f8bf9666010bd7d5d1244ae45b799b6aab3c15226d25eec63ae67963`.
The installed VM's three original managed checks exercise the production root
service sandbox and polkit with a private PPD provider. The additional Flatpak
observations use a private user-owned mock service and the installed CLI. These
are separate conditions; the latter are not a root-sandbox compatibility claim.

The VM-tested candidate is 3,696,180 bytes, SHA-256
`919adc5f763632511cb6081c457983e12109ff753d64dd40d175cdfba7fcb0bd`.
The final `omen-linux_0.1.0-25_amd64.deb` keeps the tested runtime payload,
maintainer scripts and control metadata; only packaged documents are finalized.
Exact final package bytes/hash and the archive comparison are in
`evidence/session-migration-v25/validation-summary.json`.
Earlier packages and evidence retain their original attribution.

## Remaining work

The observation covers the registering peer while it is alive. It cannot
reconstruct arbitrary descendants that leave the scope after that peer exits,
or establish game lifetime from a launcher dispatch. A sandbox bridge must
preserve the actual command's identity, arguments, environment, working directory,
exit code, signals and final descendants before automatic Flatpak setup can be
enabled. The tested Flatpak 1.16.6 `enter` status defects are still present.
Custom commands that start Flatpak are still not refused before registration;
use manual restoration for that unsupported integration.

No new UI, screen-reader, graphical authorization, real store-game or physical
hardware validation is claimed here. These runs use private providers and
ordinary mock files; no host installation, system power-provider access or
physical control write occurs. English-only presentation remains the requested
scope. The broader application audit remains active.
