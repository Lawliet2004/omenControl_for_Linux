# Additional launcher formats

Revision 20 adds standalone native Legendary, Steam Snap, and modern binary
Steam shortcuts. Discovery and execution stay in the desktop user process.
The privileged daemon continues to refuse game inventory and game execution.
`omen-cli games --json` returns the same desktop inventory without connecting to
the hardware service or performing hardware discovery.

## Steam installations and shortcuts

Steam Snap uses `~/snap/steam/common/.local/share/Steam` and submits a single
`steam://rungameid/<id>` argument through `snap run steam`. Native, Flatpak and
Snap IDs remain distinct. Native aliases that resolve inside Snap's common
directory are classified as Snap. Canonical aliases within the same route are
deduplicated. Additional Steam locations explicitly select their installation.

For shortcuts, OMEN reads `config/loginusers.vdf` and requires exactly one account
marked `MostRecent`. It then reads only that account's
`userdata/<account-id>/config/shortcuts.vdf`. It does not read passwords or tokens,
sign in, force an account, or verify which account an already running Steam
client is using. Launch preparation rechecks the recent-account selection before
applying a profile; stale shortcut IDs fail preparation.

The binary reader supports bounded object/string/integer records and known
fixed-width scalar types. Files are limited to 8 MiB, 32 nesting levels and
100,000 nodes. Invalid UTF-8, truncated/unterminated records, ambiguous critical
keys, unsupported types and trailing data are rejected. Unreadable files and
named pipes cannot block a scan. A failed shortcut file does not remove valid
app manifests from that installation.

Modern shortcuts must supply their unsigned stored `appid`, at least
`0x80001388`. OMEN constructs the 64-bit shortcut game ID using the documented
Steam game-ID layout; it does not guess an ID from an executable/name checksum.
Shortcut executable paths and launch options never become shell commands.
Example: `steam/shortcut/7/4045620583` submits
`steam://rungameid/17375808096043008000`. The inventory retains the originating
Steam directory, and the ID retains its account component. These local IDs are
not authentication credentials.

## Legendary configuration and runtime

Default discovery uses `$LEGENDARY_CONFIG_PATH`, then
`$XDG_CONFIG_HOME/legendary`, then `~/.config/legendary`. Explicit library
locations select a configuration directory containing `installed.json`.
Inventory entries retain that canonical directory in `library_root`; the UI
shows it and the JSON inventory exposes it. Default locations are scanned first,
so duplicate IDs retain their first discovered record. This is not a simultaneous
multi-account selector.

Valid installed base games use `legendary/<app-name>`. DLC-only records are
excluded; executable-bearing installed entries remain eligible. A safe app name
is supplied literally after `launch --`. Preparation resolves `legendary` in
PATH, verifies the selected directory and sets `LEGENDARY_CONFIG_PATH` for both
the version probe and launch.

Legendary 0.20.33 ignores this configuration environment variable. Before any
profile mutation, a bounded three-second `--version` probe therefore admits only
stable 0.20.34+ within the 0.20 series or stable 0.21.x. Unknown series,
prereleases and unrecognized output fail with an update or explicit Custom
command instruction. This conservative check prevents a selected catalog from
silently launching through an unrelated configuration. It does not guarantee
every admitted version's game compatibility.

Manual mode retains submission transactions and explicit profile restoration.
Opt-in managed native Legendary uses a wrapper configured in that selected
Legendary configuration. The actual game command runs inside a dedicated user
scope; the desktop observer stays outside it. A URI/launcher receipt alone never
establishes game lifetime. See [managed sessions](MANAGED_GAME_SESSIONS.md).

## Verification and limits

The format suite exercises literal arguments, native/Flatpak/Snap routes,
configuration selection, account changes, malformed records, missing runtimes,
early failure recovery and rejection of an older Legendary before profile
mutation. Private native keyboard workflows cover search/setup, the real folder
chooser, persistence, rescan, launch and location removal. The folder test checks
the actual entered path before accepting GTK completion. The location selector
shows folder guidance for its selected launcher; removal actions stay compact
beside long wrapped paths and large text.

An optional integration runs the pinned, unmodified upstream Legendary 0.21.1
CLI against synthetic offline installed games. It verifies actual game execution
from two selected configurations and the configured OMEN wrapper's live private
PPD hold and release through real user scopes. A process-local socket/DNS guard
rejects and records attempted network access. This fixture does not establish a
real Epic account, game, DRM, Wine/Proton or online compatibility. Test-only Python
dependencies and distribution metadata are isolated under `target/`; nothing is
installed into the host's Python environment.

Snap and Steam shortcut routing tests use controlled executables, not installed
Steam/Snap clients. Native Steam/Proton, Heroic and Lutris game execution remain
unvalidated. Automatic wrapping for Steam Snap and Flatpak is disabled with an
explanation; saved unsupported preferences can be turned off. Heroic sideload,
Zoom and other third-party formats, sandboxed Legendary and AppImages without a
matching PATH launcher remain outside automatic routing. Explicit Custom commands
are available and are the only route that invokes a shell.

Pinned primary-source identities and hashes are recorded in
`evidence/launcher-expansion/launcher-sources.json` and
`legendary-version-compatibility.json`: Legendary's CLI/storage code,
Canonical's Steam Snap recipe, and Valve's binary KeyValues/game-ID definitions.
The final validation summary records exact binaries, package and test results.
