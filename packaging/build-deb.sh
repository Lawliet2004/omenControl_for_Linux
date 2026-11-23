#!/bin/bash
# Build the omen-linux .deb from a cargo release build.
# Usage: packaging/build-deb.sh [output-dir]
# Requires: cargo and dpkg-deb. Run from the repository root.
set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/target/debian}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
PKG="$(mktemp -d "$OUT/.omen-linux-stage.XXXXXX")"
chmod 755 "$PKG"
trap 'rm -rf -- "$PKG"' EXIT
UPSTREAM_VERSION="$(cd "$ROOT" && cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
VERSION="$(dpkg-parsechangelog -l"$ROOT/packaging/debian/changelog" -SVersion)"
case "$VERSION" in
  "$UPSTREAM_VERSION"-*) ;;
  *) echo "Debian changelog version does not match Cargo workspace version" >&2; exit 1 ;;
esac
ARCH="$(dpkg --print-architecture)"

echo "==> Building release binaries"
( cd "$ROOT" && cargo build --release --locked --workspace )

echo "==> Staging package tree at $PKG"
install -D -m755 "$ROOT/target/release/omen-daemon" "$PKG/usr/bin/omen-daemon"
install -D -m755 "$ROOT/target/release/omen-cli"     "$PKG/usr/bin/omen-cli"
install -D -m755 "$ROOT/target/release/omen-gui"     "$PKG/usr/bin/omen-gui"
# The sandbox helper needs a runtime-independent libc. A supplied sysroot is
# extracted tooling, not a host installation; / works with distro musl-dev.
python3 "$ROOT/packaging/build-flatpak-wrapper.py" --musl-sysroot "${OMEN_MUSL_SYSROOT:-/}" --output "$PKG/usr/libexec/omen-linux/omen-flatpak-game"
chmod 755 "$PKG/usr/libexec/omen-linux/omen-flatpak-game"
install -D -m644 "$ROOT/packaging/omen-flatpak-game.c" "$PKG/usr/share/doc/omen-linux/flatpak/omen-flatpak-game.c"
mv "$PKG/usr/libexec/omen-linux/omen-flatpak-game.json" "$PKG/usr/share/doc/omen-linux/flatpak/build-manifest.json"
install -D -m644 "${OMEN_MUSL_SYSROOT:-/}/usr/share/doc/musl/copyright" "$PKG/usr/share/doc/omen-linux/flatpak/musl-copyright"
install -D -m644 "$ROOT/packaging/build-flatpak-wrapper.py" "$PKG/usr/share/doc/omen-linux/flatpak/build-flatpak-wrapper.py"
install -D -m644 "$ROOT/docs/FLATPAK_SETUP.md" "$PKG/usr/share/doc/omen-linux/FLATPAK_SETUP.md"
install -D -m644 "$ROOT/docs/AUDIT_COMPLETION_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/AUDIT_COMPLETION_2026-10-28.md"
install -D -m644 "$ROOT/packaging/systemd/omen-linux.service" "$PKG/usr/lib/systemd/system/omen-linux.service"
install -D -m644 "$ROOT/packaging/systemd/omen-fan-recovery@.service" "$PKG/usr/lib/systemd/system/omen-fan-recovery@.service"
install -D -m644 "$ROOT/packaging/systemd/omen-nvidia-reader.service" "$PKG/usr/lib/systemd/system/omen-nvidia-reader.service"
install -D -m644 "$ROOT/packaging/systemd/omen-nvidia-reader.socket" "$PKG/usr/lib/systemd/system/omen-nvidia-reader.socket"
install -D -m644 "$ROOT/packaging/polkit/com.omenlinux.policy" "$PKG/usr/share/polkit-1/actions/com.omenlinux.policy"
install -D -m644 "$ROOT/packaging/com.omenlinux.Gui.desktop" "$PKG/usr/share/applications/com.omenlinux.Gui.desktop"
install -D -m644 "$ROOT/packaging/com.omenlinux.Gui.svg" "$PKG/usr/share/icons/hicolor/scalable/apps/com.omenlinux.Gui.svg"
install -D -m644 "$ROOT/docs/INSTALLATION.md"  "$PKG/usr/share/doc/omen-linux/INSTALLATION.md"
install -D -m644 "$ROOT/docs/HARDWARE_SAFETY.md" "$PKG/usr/share/doc/omen-linux/HARDWARE_SAFETY.md"
install -D -m644 "$ROOT/docs/HARDWARE_COMPATIBILITY.md" "$PKG/usr/share/doc/omen-linux/HARDWARE_COMPATIBILITY.md"
install -D -m644 "$ROOT/docs/ARCHITECTURE.md"  "$PKG/usr/share/doc/omen-linux/ARCHITECTURE.md"
install -D -m644 "$ROOT/docs/GAME_LAUNCHERS.md" "$PKG/usr/share/doc/omen-linux/GAME_LAUNCHERS.md"
install -D -m644 "$ROOT/docs/LAUNCHER_EXPANSION.md" "$PKG/usr/share/doc/omen-linux/LAUNCHER_EXPANSION.md"
install -D -m644 "$ROOT/docs/MANAGED_GAME_SESSIONS.md" "$PKG/usr/share/doc/omen-linux/MANAGED_GAME_SESSIONS.md"
install -D -m644 "$ROOT/docs/ACCESSIBILITY.md" "$PKG/usr/share/doc/omen-linux/ACCESSIBILITY.md"
install -D -m644 "$ROOT/docs/GPU_MONITORING.md" "$PKG/usr/share/doc/omen-linux/GPU_MONITORING.md"
install -D -m644 "$ROOT/docs/GLOBAL_SHORTCUTS.md" "$PKG/usr/share/doc/omen-linux/GLOBAL_SHORTCUTS.md"
install -D -m644 "$ROOT/docs/COARSE_FAN_VALIDATION_PLAN.md" "$PKG/usr/share/doc/omen-linux/COARSE_FAN_VALIDATION_PLAN.md"
install -D -m644 "$ROOT/docs/FAN_MODES.md" "$PKG/usr/share/doc/omen-linux/FAN_MODES.md"
install -D -m644 "$ROOT/docs/STATUS_OVERLAY.md" "$PKG/usr/share/doc/omen-linux/STATUS_OVERLAY.md"
install -D -m644 "$ROOT/docs/CONTINUATION_CHECKLIST.md" "$PKG/usr/share/doc/omen-linux/CONTINUATION_CHECKLIST.md"
install -D -m644 "$ROOT/docs/NETWORK_MONITORING.md" "$PKG/usr/share/doc/omen-linux/NETWORK_MONITORING.md"
install -D -m644 "$ROOT/docs/FAN_MONITORING.md" "$PKG/usr/share/doc/omen-linux/FAN_MONITORING.md"
install -D -m644 "$ROOT/docs/THERMAL_GUARD.md" "$PKG/usr/share/doc/omen-linux/THERMAL_GUARD.md"
install -D -m644 "$ROOT/docs/FAN_GUARDIAN.md" "$PKG/usr/share/doc/omen-linux/FAN_GUARDIAN.md"
install -D -m644 "$ROOT/docs/PACKAGE_VALIDATION.md" "$PKG/usr/share/doc/omen-linux/PACKAGE_VALIDATION.md"
install -D -m644 "$ROOT/docs/VM_VALIDATION.md" "$PKG/usr/share/doc/omen-linux/VM_VALIDATION.md"
install -D -m644 "$ROOT/docs/DESKTOP_VALIDATION_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/DESKTOP_VALIDATION_2026-10-28.md"
install -D -m644 "$ROOT/docs/CONTROL_PAGE_RECOVERY_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/CONTROL_PAGE_RECOVERY_2026-10-28.md"
install -D -m644 "$ROOT/docs/SOURCE_SWITCHING_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/SOURCE_SWITCHING_2026-10-28.md"
install -D -m644 "$ROOT/docs/SENSOR_BROWSER_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/SENSOR_BROWSER_2026-10-28.md"
install -D -m644 "$ROOT/docs/NOTIFICATION_WORKFLOW_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/NOTIFICATION_WORKFLOW_2026-10-28.md"
install -D -m644 "$ROOT/docs/FLATPAK_SESSION_BOUNDARIES_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/FLATPAK_SESSION_BOUNDARIES_2026-10-28.md"
install -D -m644 "$ROOT/docs/SESSION_SCOPE_MIGRATION_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/SESSION_SCOPE_MIGRATION_2026-10-28.md"
install -D -m644 "$ROOT/docs/REAL_GAME_VALIDATION_2026-10-28.md" "$PKG/usr/share/doc/omen-linux/REAL_GAME_VALIDATION_2026-10-28.md"
install -D -m644 "$ROOT/docs/APPLICATION_AUDIT_IMPLEMENTATION.md" "$PKG/usr/share/doc/omen-linux/APPLICATION_AUDIT_IMPLEMENTATION.md"
install -D -m644 "$ROOT/docs/PROFILE_VALIDATION_PLAN.md" "$PKG/usr/share/doc/omen-linux/PROFILE_VALIDATION_PLAN.md"
install -D -m644 "$ROOT/docs/evidence/live-profile-test.json" "$PKG/usr/share/doc/omen-linux/evidence/live-profile-test.json"
install -D -m644 "$ROOT/LICENSE" "$PKG/usr/share/doc/omen-linux/LICENSE"
install -D -m644 "$ROOT/docs/REVERSE_ENGINEERING_REPORT.md" "$PKG/usr/share/doc/omen-linux/REVERSE_ENGINEERING_REPORT.md"
install -D -m644 "$ROOT/docs/OMENCORE_REVIEW.md" "$PKG/usr/share/doc/omen-linux/OMENCORE_REVIEW.md"
install -D -m644 "$ROOT/docs/TEST_REPORT.md" "$PKG/usr/share/doc/omen-linux/TEST_REPORT.md"
install -D -m644 "$ROOT/docs/WINDOWS_ARTIFACTS.md" "$PKG/usr/share/doc/omen-linux/WINDOWS_ARTIFACTS.md"
install -D -m644 "$ROOT/docs/THIRD_PARTY.md" "$PKG/usr/share/doc/omen-linux/THIRD_PARTY.md"
install -d -m755 "$PKG/usr/share/doc/omen-linux/icon-fallback"
cp -R "$ROOT/gui/assets/." "$PKG/usr/share/doc/omen-linux/icon-fallback/"
python3 "$ROOT/packaging/generate-notices.py" "$PKG/usr/share/doc/omen-linux/DEPENDENCY_NOTICES.md"
gzip -9n "$PKG/usr/share/doc/omen-linux/DEPENDENCY_NOTICES.md"
install -D -m644 "$ROOT/README.md"             "$PKG/usr/share/doc/omen-linux/README.md"
install -D -m644 "$ROOT/packaging/debian/copyright" "$PKG/usr/share/doc/omen-linux/copyright"
install -D -m644 "$ROOT/packaging/debian/changelog" "$PKG/usr/share/doc/omen-linux/changelog.Debian"
gzip -9n "$PKG/usr/share/doc/omen-linux/changelog.Debian"
install -d -m755 "$PKG/etc/omen-linux"
echo "==> Writing control area (DEBIAN/)"
install -d "$PKG/DEBIAN"
SHLIBS="$(cd "$ROOT/packaging" && dpkg-shlibdeps --ignore-missing-info -O -e"$PKG/usr/bin/omen-gui" -e"$PKG/usr/bin/omen-daemon" -e"$PKG/usr/bin/omen-cli" ${OMEN_SHLIB_PATH:+-l"$OMEN_SHLIB_PATH"} | sed -n 's/^shlibs:Depends=//p')"
if [ -z "$SHLIBS" ]; then
  echo "Could not determine binary library dependencies" >&2
  exit 1
fi
sed -e "s/@ARCH@/$ARCH/" -e "s/@VERSION@/$VERSION/" -e "s/@SHLIBS@/$SHLIBS/" "$ROOT/packaging/debian/control.in" > "$PKG/DEBIAN/control"
install -m755 "$ROOT/packaging/debian/postinst" "$PKG/DEBIAN/postinst"
install -m755 "$ROOT/packaging/debian/prerm"    "$PKG/DEBIAN/prerm"
install -m755 "$ROOT/packaging/debian/postrm"   "$PKG/DEBIAN/postrm"

# md5sums over the payload only
( cd "$PKG" && find ./usr ./etc -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums )

echo "==> dpkg-deb"
mkdir -p "$OUT"
dpkg-deb --build --root-owner-group "$PKG" "$OUT/omen-linux_${VERSION}_${ARCH}.deb"
echo "==> Built $OUT/omen-linux_${VERSION}_${ARCH}.deb"
dpkg-deb --info "$OUT/omen-linux_${VERSION}_${ARCH}.deb"
