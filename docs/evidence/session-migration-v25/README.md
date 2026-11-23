# Revision 25 scope migration recovery

The source fixes the original registered peer leaving its tracked scope. It
releases its profile hold and records Interrupted while the process remains
alive. This is not a managed Flatpak implementation or game-compatibility claim.

The candidate has 40 installed VM checks and two reboots. The final package's
runtime payload, maintainer scripts and control metadata are identical to that
candidate, with six finalized document payloads. See the package comparison and
reports/docs/SESSION_SCOPE_MIGRATION_2026-10-28.md for exact attribution.

Debug/release each pass 276 Rust, 21 native managed-session and 12 lower-level
scope/provider checks. The release native run uses the ordinary packaged CLI.
The current helper was selected from Cargo artifact JSON and retained at
target/session-migration-v25/artifacts/managed-games-native-release, with its
hash/source identity in runs/candidate-manifest.json. Large compiled helpers and
packages are retained there rather than duplicated in this evidence directory.

The seven Flatpak observations use the same pinned Ubuntu packages and explicit
local fixture as evidence/flatpak-scope-probe-v24. Upstream source/archive and
signature/package provenance remain in that immutable publication. This bundle
copies provenance manifests and records its exact summary hash. No host install,
system power-provider call or physical control write occurs. No new GUI, real
graphical authorization, real game, full accessibility or physical validation
is claimed. Earlier packages and evidence retain their attribution.
