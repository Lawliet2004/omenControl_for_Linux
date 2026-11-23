# Third-party attribution

Application: GPL-3.0-or-later, `LICENSE`. Rust dependencies carry their own licenses;
`Cargo.lock` pins the resolved versions. No HP proprietary code/assets/binaries
are included or redistributed.

The GUI embeds symbolic fallback icons from GNOME Project's Adwaita Icon Theme
50.0. A selected desktop theme retains priority; the embedded hicolor resources
cover missing icons. `gui/assets/PROVENANCE.json` records the exact signed Ubuntu
archive and per-icon hashes. The Performance fallback uses Adwaita's
`power-profile-performance-symbolic` under the existing `speedometer-symbolic`
lookup name. The artwork is otherwise unchanged. Upstream attribution/license
notices, LGPL v3 text and the SVG sources are retained under `gui/assets/` and
installed in `icon-fallback/` beside this document.

The GUI uses ksni (resolved version 0.3.7, Unlicense),
https://github.com/iovxw/ksni, for the StatusNotifierItem and D-Bus menu protocols.
The package includes `DEPENDENCY_NOTICES.md.gz`, generated from the actual locked
crate distributions, preserving their license and notice files, including build
and test dependencies. `packaging/generate-notices.py` fails if a resolved crate
has no notice file rather than silently omitting it.

The installed project REA skill (`.agents/skills/reverse-engineer-anything`) is
MIT-licensed under its own `LICENSE`, from morluto/rea. It is not part of the
application's runtime payload. Analysis engines were not installed.

The factual board IDs in `crates/hardware/src/board_tables.rs` come from Linux
`drivers/platform/x86/hp/hp-wmi.c` (SPDX GPL-2.0-or-later); exact reviewed revision
and file hash are in `evidence/sources.json`. No kernel implementation is linked
into the Rust application; sysfs provides the runtime boundary.

OMEN Space, OpenOMEN, OmenMon and OmenLinux/omen-rgb-keyboard were reviewed at the
commits in `evidence/sources.json` under their GPL-3 licenses. Their hardware
claims are attributed and separated from this application's validation. Their
implementations were not copied into this project; no out-of-tree driver is
bundled or automatically installed.

OmenCore's canonical source, theantipopau/omencore, was established and reviewed
at commit 840f03d58ddd4c3a709780b6d635828d9ef3d87c under its MIT license
(copyright 2024–2026 TheAntiPopAU). `OMENCORE_REVIEW.md` records source and issue
scope; `evidence/omencore-sources.json` preserves the reviewed license hash and
file provenance. No OmenCore implementation or assets are copied or bundled.

Debian maintainer scripts follow the init-system-helpers lifecycle model and
the dh_installsystemd patterns shipped with Ubuntu's power-profiles-daemon.
They invoke the distribution's `deb-systemd-helper` and `deb-systemd-invoke`;
the helper implementations are not bundled. The relevant Debian manuals are
linked in `PACKAGE_VALIDATION.md`.
