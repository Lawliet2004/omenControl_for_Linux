# Nested Flatpak bridge development

Unreleased revision 26; automatic GUI setup is disabled and no package is delivered.
See reports/docs/FLATPAK_NESTED_BRIDGE_2026-10-28.md for exact scope and open work.

The passing VM run has 45 checks, including 12 Flatpak workflows and two reboots.
It uses finalized revision 25 as its base, a separately retained development CLI,
a native static sandbox wrapper, and a newly compiled root-service test helper.
The guest CLI is restored before reboot checks. The root service retains its
installed sandbox and normal polkit policy; its profile provider is private.

A failed test-assumption run and a preliminary capability run are separately
retained. Do not add their counts to the passing suite. No real Flatpak game,
Flathub launcher, GUI, physical or full job-control compatibility is claimed.

Artifacts include full project sources/license, existing Rust dependency notices,
static musl copyright and build manifest. Exact musl archives and signed-release
metadata are retained; the full signed index, offline repository and cloud image
remain in target/ at the hashes recorded in the input/run manifests. No host
package installation, service replacement or hardware write occurred.
