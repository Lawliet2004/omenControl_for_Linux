# Unreleased Flatpak bridge development

This is development code, not a revision 26 package or supported Flatpak GUI setup.
See reports/docs/FLATPAK_BRIDGE_DEVELOPMENT_2026-10-28.md for remaining requirements.

The core suite has 43 checks and the separate HostCommand suite 40 checks. Each
uses finalized revision 25 as the base, with a separately retained development
CLI. The HostCommand fixture overlays/restores only the guest CLI before reboot
checks; there is no host installation. Both providers are private fixtures.
The wrapper suite uses the installed root-service sandbox and normal polkit.

Failed transport/setup/cleanup runs and exact per-run source snapshots are
retained separately. Do not combine them into a passing run. No real game,
Flathub launcher, GUI, graphical authorization or physical validation is claimed.
The tested CLI is in artifacts/, with project license and dependency notices.
Large signed repository/cloud image inputs remain in target/ with exact hashes
in the run and repository manifests. Earlier published artifacts are unchanged.
