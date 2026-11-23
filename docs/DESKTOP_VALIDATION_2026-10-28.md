# Revision 21 desktop validation

Revision 21 fixes missing header/sidebar icons on incomplete desktop themes and
passes **39 checks in a fresh Ubuntu/Plasma Wayland systemd VM**, including real
KDE/PAM authorization and two reboots. The exact tested candidate is
`target/icon-fallback-v21/candidate/omen-linux_0.1.0-21_amd64.deb`, SHA-256
`f4fc0a72b90416260755be5e475e9926d8bf0e48a729043b3534bba6b08eaab7`.
The final documentation package is compared with that candidate: runtime files,
maintainer scripts and control metadata must match. Its final fingerprint and
all source/evidence hashes are in `evidence/desktop-v21/validation-summary.json`.
Revision 20's package and historical evidence are preserved unchanged.

## Results and scope

| Coverage | Result |
| --- | --- |
| Installed service/package lifecycle | 17 checks, including effective sandbox permissions, stop/start/restart, failure recovery, enabled/disabled reboots, reinstall/mask/remove/purge |
| Independent coarse recovery | Nine checks in exact-path virtual fixtures; no physical fan control |
| Isolated NVIDIA reader | Four fixture checks, private endpoint and stale-data refusal |
| Managed sessions in installed sandbox | Three checks with production IPC/service behavior, real user scopes and private PPD; no store game or physical profile change |
| Real graphical authorization | Six checks through the installed GUI, KDE file portal, KDE polkit agent and PAM |
| Native GUI regressions | 31 checks on each X11 and Wayland backend, including nine observed native file selections per backend |
| Icon/theme presentation | Four scenes, 71 mapped icon observations and 142 measured active text-label contrast samples; no detected missing icons, horizontal label clipping, collapsed text allocations or failing measured contrast |
| Rust/source checks | 264 release Rust tests, formatting and strict all-target/all-feature Clippy |
| Package contents | Static ownership, linkage, checksums, desktop/policy/unit checks and 15 exact icon sources with notices |

The release GUI SHA-256 is
`212c0cb61a18bbcd07652d846e4e3e38fcf50d4afcda1ff0121b53797bb3b7ee`.
The CLI and daemon remain byte-identical to revision 20:
`f0e5c0b08cfec6f32ecb94e39ca65a990f46af4f4ee7bd0dd74c8c562484cf30`
and `219780aa03ea5bda0036b402f0231a13cbded8329b8cf1b2dd05e3bc6f4a12cb`.
Their earlier launcher/runtime results retain their original attribution.

## Real authorization workflow

The installed `/usr/bin/omen-gui` starts through desktop autostart in an active,
local SDDM Plasma Wayland session. The normal installed daemon handles requests.
The KDE file portal opens the fixture backup and the application displays its
policy preview. Every imported backup keeps battery automation disabled.

The six checks establish:

1. Canceling the application preview sends no service mutation.
2. Canceling the real KDE authentication dialog preserves settings and returns
   error feedback in the GUI.
3. An incorrect disposable-VM password cannot authorize the write.
4. The correct VM-only password authorizes the GUI through PAM and creates an
   observable temporary authorization for `com.omenlinux.admin`.
5. A second GUI import uses that retained authorization without a new dialog.
6. Natural expiry requires a fresh dialog; canceling it leaves settings intact.

The fresh run reports a 299-second authorization window. The cache is observed
absent on the first ten-second poll after its reported expiry. The clock is not
advanced to simulate expiry. No authorization-grant rule is installed for this
graphical stage; temporary rules used by earlier headless fixtures are removed
before it starts. The root harness places only its cache inspector in the
verified SDDM scope, then drops it to UID 1000, because polkit restricts cache
inspection to the caller's own session. The GUI already belongs to the real
desktop. Its authorization is not bypassed.

Guest-only libevdev/uinput sends keyboard input through the virtual compositor.
The generated password stays private and is never logged or placed in a command
argument. The prior account hash and service configuration are restored. QMP
captures show the real dialogs; inspected password fields are empty/masked.
The [polkit authority API](https://www.freedesktop.org/software/polkit/docs/latest/eggdbus-interface-org.freedesktop.PolicyKit1.Authority.html)
describes temporary authorizations, and the
[kernel uinput documentation](https://www.kernel.org/doc/html/latest/input/uinput.html)
describes the virtual input mechanism.

## Icon fallback correction

The minimal installed desktop selected `gnome`, an absent icon theme. Actual GTK
lookups returned `image-missing` for eight of the ten probed application symbols,
while SVG decoding itself succeeded. Adding a decoder dependency would not fix
that lookup failure.

The GUI now embeds 15 unchanged GNOME Adwaita 50.0 symbolic SVGs in a GResource.
They participate in hicolor fallback rather than replacing the selected theme.
The Performance lookup aliases Adwaita's performance-profile symbol to the
existing `speedometer-symbolic` name. Sources, upstream notices, LGPL text and
signed-archive/per-file hashes are retained under `gui/assets/` and included in
the installed package. The build uses `glib-compile-resources`; no runtime
download or desktop configuration change is needed.

The actual app reports its mapped icon resolutions in developer capture output.
The four scenes cover a missing theme in wide light and narrow dark Sans 20,
Adwaita light and Breeze dark. The selected theme remains unchanged. The missing
wide scene uses 13 bundled fallbacks; Adwaita and Breeze use their installed
icons where available, with one and two bundled fallbacks respectively. All four
captures were inspected together once. No further visual edit followed.
GTK documents this behavior in
[IconTheme resource paths](https://docs.gtk.org/gtk4/method.IconTheme.set_resource_path.html).

These measurements cover mapped icons and the collector's supported visible
text labels. They do not measure every input/icon contrast state, all themes,
hover/pressed states, physical DPI or complete accessibility conformance.

## Isolated environment and reproducibility

The fresh guest uses the signed Ubuntu 26.04.1 cloud image and existing extracted
QEMU 10.2.1 runtime, kernel `7.0.0-38-generic` and systemd
`259.5-0ubuntu3.4`. QEMU runs as the desktop user with two virtual CPUs, 2 GiB RAM
and a virtual GPU. It has no shared host folders or physical device passthrough.
Networking is restricted to the explicit localhost SSH forward.

Installation uses 610 verified Ubuntu main/universe archives from three signed
pockets. Their signed-index payload totals 308,505,686 bytes. The complete local
repository is 403,599,360 bytes, SHA-256
`6481c4c39b0f3f3fc1f7e6a21e3de5218eaa57f4d8d2dc680226decc8a2a940c`.
Signatures and package SHA-256 values are verified before use and APT verifies
them again. Date/signature checks remain enabled. The preparer accepts plain,
LZ4, XZ or gzip indexes, supports both components and resumes partial downloads
without accepting them before a complete hash match. Its main-only mode also
reproduced revision 20's original repository byte for byte.

`tests/systemd_vm.py` accepts `--graphical-authorization --memory-mib 2048`, both
`--repository-component` values and the repeated `--extra-package` arguments
recorded in the evidence command. `tests/README.md` explains the fixture. Public
signed indexes, dependency resolution, repository manifest, exact invocation,
raw results and captures are retained with the evidence. Reproduction must use
fresh signed indexes once their validity window expires.

The guest exits zero and removes its disk, seed and private SSH inputs. Earlier
debug guests also exited zero; their retained inputs were subsequently removed
and those cleanup records are preserved. The unknown pre-existing debug overlay,
original dependency container, running app/mock daemon and host authentication
agent were preserved. No host package/service installation or physical control
write occurred.

## Failures retained

The initial large archive transfer exceeded its timeout; resumable downloads
with a longer bound completed the same hash-verified input set. The first GUI
attempt exposed a buffered screenshot-acknowledgement deadlock and an inspector
still belonging to SSH. The second queried the session's control group from the
wrong interface. Subsequent reused-guest debugging corrected a malformed test
backup, reloaded SDDM before configuring autologin, used the KDE Name field
instead of its URL navigator, and handled Qt's capitalized action names.
These are harness corrections. The final fresh run passes without stdout
nudges or manual intervention. A prior reused revision 20 guest separately
passed all six real authorization checks; it is not counted as a fresh full run.

The first Wayland native regression stopped after 26 checks at a Recent-file
picker. The harness now observes the requested row's selected state before
acceptance and targets a showing dialog. Both backends then pass all 31 checks
and nine observed selections. The precise cause of the initial timing failure
is not established; its record remains available.

The initial package command also exposed a real build-script bug: a relative
output path broke shared-library inspection after changing directories. Revision
21 canonicalizes that output path; the same relative invocation then built the
validated candidate successfully.

## Remaining gates

This closes the bounded Ubuntu/Plasma virtual-desktop installation and real
authorization workflow. It does not validate physical keyboard/focus/display,
global shortcuts or overlays above full-screen games, other distributions or
assistive technologies, actual Steam/Proton/Heroic/Lutris/Epic games/accounts,
managed Flatpak execution, or physical HP fan/RGB/charging/MUX/suspend behavior.
The application audit remains active and presentation remains English-only.
