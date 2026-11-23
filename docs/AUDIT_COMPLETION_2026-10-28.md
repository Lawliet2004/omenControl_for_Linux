# Audit implementation follow-through

Revision 26 adds the remaining locally implementable Flatpak setup and package
integration, installed Heroic sideload records, and further native accessibility
and process-lifetime coverage. It does not turn fixture evidence into physical
hardware support or claim compatibility with accounts/games that were not run.

## Product changes

- Game Library Profile setup installs, checks and removes a launcher-scoped
  static helper. The same operations are in `omen-cli game-flatpak-setup`.
  Native launcher configuration remains user controlled; the UI provides the
  exact actual-command prefix and permission/restart/recovery guidance.
- Effective host-command permission is inspected off GTK's main thread. A
  failed check keeps automatic mode unavailable even after switching to manual
  mode. Launch-time sandbox identity, kernel, cgroup and authorization checks
  remain authoritative. A setup check is not game execution evidence.
- Helpers and SHA-256 installation records are published as one directory.
  Copied commands retain the sandbox-visible home path when the host home is a
  symlink; bounded verification refuses unexpected directory contents.
  Updates and removal preserve modified files, symlinks and unrelated data.
  Shared removal disables that launcher's automatic bindings through the same
  serialized settings transaction as other preferences. It preserves manual
  profile choices and active sessions.
- Debian packaging builds the static PIE helper from an explicit musl sysroot,
  accepts both an installed system musl toolchain and an extracted musl sysroot,
  checks dynamic dependencies and stack permissions, and ships its source,
  build manifest and complete musl notice. The service still never executes
  games or receives a PID/command/path authority from this workflow.
- Heroic sideload discovery requires the pinned store's `runner=sideload`,
  explicit installed flag, valid ID and nonempty title. Native/Flatpak records
  remain separate and use Heroic's own URI protocol. Executable metadata never
  becomes an OMEN shell command.
- Native and Flatpak wrapper command views inherit the configured text size;
  their monospace font no longer stays small when the application font grows.
  Developer captures can use a bounded 4–30 second staging delay. UI reports
  include read-only command text geometry and actual computed font size.

## Verification

Evidence is published under `docs/evidence/audit-v26/`. Its index records exact
binary/package identities and keeps each VM run separately attributed. Raw
preparation caches remain under `target/audit-completion/`.

- Debug and release: 283 Rust tests each. Strict all-target/all-feature Clippy
  passes. User-settings shared removal has a scoped preservation regression.
- Nine CLI setup/recovery workflows cover installation, updates, permission loss,
  modified content, unexpected files/links, a symlinked home, pre-registration
  handoff reporting and launcher-scoped removal.
- Seven actual keyboard setup workflows pass per debug/release, including repeated
  native row rebindings. The existing
  thirteen keyboard/Orca workflows and twelve control recovery workflows pass.
- Five expanded Flatpak setup/permission/recovery scenes pass geometry and
  active-text contrast checks; actual command-font scaling is checked in points
  after conversion from GTK's computed pixel units. Five additional native launcher/location scenes include large wrapper text.
  Twelve additional control scenes pass high-contrast geometry/contrast checks with the software renderer.
- Fifteen launcher-format checks pass per debug/release profile, including
  exact native and Flatpak Heroic sideload URI routing without executing
  executable metadata; the installed package repeats all fifteen.
- Twenty-one native managed-session checks pass. Two further private-PPD checks
  verify inherited descriptors, foreground PTY identity, process-group stop/
  resume, retained ownership while stopped and release after normal exit.
- A fresh installed sandbox VM passes 48 checks including fifteen Flatpak
  workflows, overlapping holds, wrapper death during authorization, inherited
  descriptors, original resource limits, two reboots and complete cleanup.
  It uses local Flatpak fixtures, not a store game. The final rerun uses the
  frozen candidate; its CLI and daemon match the tested package.

A separate fresh Ubuntu/Plasma Wayland VM passes 50 checks on the preceding
packaged candidate (before the final setup-row callback lifetime fix): six real KDE/PAM authorization workflows including natural
cache expiry, five source-recovery workflows, six actual native Lutris/OpenTTD
workflows, installed-service/lifecycle/isolation checks and two reboots. The
fixture preparation installs its desktop dependencies from signed local APT
indexes. The frozen candidate also passes twenty lifecycle and fifteen installed format
checks in a clean Ubuntu container with networking disabled and signed local
package indexes. Neither environment uses a host power provider.

Real signed Flathub Lutris/OpenTTD validation is being completed separately.
The earlier native Lutris/OpenTTD result remains separately attributed. No
real Steam/Proton, Epic/Legendary account, Heroic game, Snap execution, graphics
acceleration or full job-control conformance is implied by these checks.

## Original remaining-work items

| Tracker item | Implemented or verified here | External work still required |
| --- | --- | --- |
| Managed games / Flatpak | Private helper setup, package integration, permission/readiness gating, recovery/removal, overlaps and additional process semantics | Real launcher/game matrix, store accounts, additional controller/cgroup-namespace variants |
| Keyboard / screen readers / presentation | Further keyboard setup/recovery, inherited text sizing, expanded permission/error/high-contrast scenes and existing Orca regressions | Additional assistive technology, audible synthesis, physical focus/DPI and complete accessibility certification |
| Retained rows / control states | Existing twelve control recovery workflows and expanded high-contrast scenes | Physical network selection/focus and representative hardware performance measurements |
| Added launcher formats | Installed Heroic sideload inventory and native/Flatpak protocol routing, pinned source provenance | Real store/account execution; Zoom and arbitrary third-party routing need a verified upstream interface |
| Desktop and distribution integration | Current Ubuntu VM/package checks; per-desktop behavior remains explicit | Physical shortcut activation, physical polkit input, compositor/fullscreen placement, physical suspend and other distributions |
| Source recovery | Existing implementation retained; regression suite remains available | Physical desktop/distribution coverage |

The pinned Heroic protocol runner enum includes sideload and excludes Zoom.
Guessing a Zoom URI would not establish compatibility; it remains outside
automatic routing. Source fingerprints are in the evidence publication.

## Hardware gates

Live fan/RGB/charge support requires suitable physical interfaces and measured
model/BIOS-specific settling, thermal coverage, suspend/resume and independent
crash recovery. Per-key lighting and MUX switching require verified interfaces
that are absent from the reference target's demonstrated capabilities. Traffic
priority requires a separate designed and authorized policy interface. These
remain open; device-specific writes are still disabled. No host package was
installed and no physical control write was performed for this work.
