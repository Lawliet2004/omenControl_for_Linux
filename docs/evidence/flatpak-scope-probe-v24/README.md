# Revision 24 Flatpak boundary observations

Managed Flatpak integration remains unsupported. The final 40-check VM result
includes seven observations reproducing candidate-wrapper defects, not a passing
game-compatibility claim. See reports/docs/FLATPAK_SESSION_BOUNDARIES_2026-10-28.md.

The unchanged OMEN package and 132 product-source files retain revision 24
attribution. Failed runs and the earlier 37-check probe are retained separately.
Runtime fixtures use an isolated user-owned mock service/private PPD server; the
suite's existing installed-service checks have separate root-sandbox attribution.

inputs/upstream/flatpak-1.16.6.tar.gz is the unchanged official upstream source
archive. Its own copyright/license notices are retained inside it. The three
extracted reference files retain their original notices; upstream COPYING is
included alongside them. Tag/source hashes pin provenance, not a verified
upstream signature. Ubuntu packages were verified against signed distro indices.
The 135,546,880-byte offline APT repository remains in target/flatpak-scope-probe/
complete/complete-repository.tar with its full hash and 105 package entries in
inputs/repository-manifest.json; the large archive is not duplicated here.
