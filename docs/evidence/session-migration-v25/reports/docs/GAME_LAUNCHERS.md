# Game discovery and launch boundaries

OMEN reads installed-game records as the desktop user. It does not install
launchers, sign in to stores, download games, edit store data, or run games from
the privileged daemon. Records may be stale or describe damaged/missing files.
A successful launch receipt means a request was submitted; it leaves
`game_execution_verified` false. Native Steam/Proton, Heroic and managed Flatpak
compatibility remain unvalidated. One native Lutris/OpenTTD game is exercised in
[the separate revision 21 result](REAL_GAME_VALIDATION_2026-10-28.md). A real
Flatpak scope/namespace probe now reproduces lifetime and command-status defects;
it does not establish game compatibility. See
[the revision 24 investigation](FLATPAK_SESSION_BOUNDARIES_2026-10-28.md).
Revision 25 now records scope migration as Interrupted; see
[the fix and validation](SESSION_SCOPE_MIGRATION_2026-10-28.md).
The pinned Legendary CLI is separately exercised with synthetic
offline games; this does not establish real Epic game/account compatibility.

## Discovery

| Source | Native location | Flatpak location relative to home | Records |
| --- | --- | --- | --- |
| Steam | `~/.steam/steam`, `~/.steam/root`, `$XDG_DATA_HOME/Steam` (default `~/.local/share/Steam`) | `.var/app/com.valvesoftware.Steam/.local/share/Steam` | `steamapps/appmanifest_*.acf`, additional libraries in `steamapps/libraryfolders.vdf` |
| Steam Snap | `~/snap/steam/common/.local/share/Steam` | — | Steam manifests, library roots and account-scoped binary shortcuts |
| Legendary | `$LEGENDARY_CONFIG_PATH`, else `$XDG_CONFIG_HOME/legendary` (default `~/.config/legendary`) | — | `installed.json` |
| Heroic | `$XDG_CONFIG_HOME/heroic` (default `~/.config/heroic`) | `.var/app/com.heroicgameslauncher.hgl/config/heroic` | Epic `legendaryConfig/legendary/installed.json`; GOG `gog_store/installed.json`; Amazon `nile_config/nile/installed.json` |
| Lutris | `$XDG_DATA_HOME/lutris/pga.db` (default `~/.local/share/lutris/pga.db`) | `.var/app/net.lutris.Lutris/data/lutris/pga.db` | SQLite `games` rows with `installed = 1` and positive numeric ID |

Steam unions its roots, deduplicates canonical paths within an installation
type, supports legacy numbered path values and modern numbered objects, and
remaps the Flatpak sandbox's default Steam path to its host location. A bounded
KeyValues reader handles braces, quoted escapes, Unicode and comments. Manifest
filename/content IDs must match. When `StateFlags` is present, the installed bit
(4) must be set. A manifest without the field remains an inventory entry.

Heroic includes Epic base games, GOG installed entries and Amazon installed IDs;
it does not treat account ownership alone as installation. Epic DLC-only records
are excluded. GOG names currently display `GOG <product ID>` because its installed
records contain no title. Amazon titles come from its optional local library
record, falling back to the product ID. No credentials or authentication files
are read.

Lutris uses a read-only SQLite connection with a 100 ms busy timeout and a
10,000-row result limit. It observes committed WAL rows, never creates a missing
database, and does not migrate or update game records. Invalid files/schemas are
skipped. SQLite may use the launcher's shared-memory locking infrastructure.
JSON/text/binary KeyValues input is limited to 8 MiB per file; KeyValues nesting/node counts
are limited. Missing/corrupt stores are skipped independently.

## Launch routing

| Example ID | Executable and separate arguments |
| --- | --- |
| `steam/620` | `steam`, `steam://rungameid/620` |
| `steam/flatpak/620` | `flatpak`, `run`, `com.valvesoftware.Steam`, `steam://rungameid/620` |
| `steam/snap/620` | `snap`, `run`, `steam`, `steam://rungameid/620` |
| `steam/shortcut/7/4045620583` | `steam`, `steam://rungameid/17375808096043008000` |
| `legendary/Quail` | `legendary`, `launch`, `--`, `Quail`; selected `LEGENDARY_CONFIG_PATH` |
| `heroic/legendary/Quail` | `heroic`, `heroic://launch/legendary/Quail` |
| `heroic/flatpak/gog/123` | `flatpak`, `run`, `com.heroicgameslauncher.hgl`, `heroic://launch/gog/123` |
| `heroic/nile/amzn1.adg.fixture` | `heroic`, `heroic://launch/nile/amzn1.adg.fixture` |
| `lutris/9` | `lutris`, `lutris:rungameid/9` |
| `lutris/flatpak/9` | `flatpak`, `run`, `net.lutris.Lutris`, `lutris:rungameid/9` |

The executable must resolve in PATH during preparation, before profile mutation.
Numeric IDs and allowed Heroic runner/URI components are validated. Store launch
text never becomes a shell command. Only an explicit user-authored Custom entry
uses `/bin/sh -c`. Native and Flatpak IDs remain distinct even when their product
IDs match. All routes refuse effective UID 0.

## Library interaction

The library uses a native GTK model and virtualized rows, so it creates controls
for the displayed portion rather than one widget tree for every game. Search
covers the complete scanned library. An empty search result keeps the search and
rescan controls available. Expanded Profile setup belongs to the game identity
and survives filtering/recycling. Unchanged rescan results retain their native
controls; removed entries and changed bindings are updated. Failed scans keep
previous results with an error. Profile choices follow current capabilities and
freshness; automatic presentation updates do not rewrite saved bindings.

A release fixture with 10,000 custom entries verifies search for the final entry,
its actual unprivileged launch, stale/recovery presentation, expanded setup and
removal after rescan on X11 and Wayland. This is a native interaction/performance
check, not real store compatibility. Results and a controlled revision 16
comparison are in `evidence/library-virtualized/validation-summary.json`.
The approach follows the [GTK list-widget model and recycling contract](https://docs.gtk.org/gtk4/section-list-widget.html).

## Profile modes and game lifetime

Manual mode is the default. Its 300 ms initial launcher check detects spawn and
immediate nonzero failures. A bound profile uses an authorized daemon-owned
submission transaction; concurrent submissions serialize and intervening daemon
profile/policy changes revoke ownership. Failure recovery verifies ownership and
current state, never a blind write. Transactions expire after thirty seconds;
daemon restart loses that authority. Manual mode does not track game exit. A
later launcher failure or GUI crash can leave its applied profile selected; review
and restore it manually. External same-value/ABA writes cannot be identified by
this submission mechanism.

Opt-in automatic mode uses an actual-command wrapper and daemon-owned PPD hold.
Custom commands launched here receive the wrapper automatically. Native store
entries show manual wrapper setup instructions; URI submission alone does not
apply an automatic profile or register lifetime. The wrapper must execute the
real game command in a dedicated user scope. Requested and effective profiles
can differ when provider holds overlap. Final descendant exit releases only the
owned hold; manual changes, explicit release and policy changes preserve later
choices. Restart records interrupted sessions without reapplying profile values.

See [managed sessions](MANAGED_GAME_SESSIONS.md) for setup, CLI commands, provider
semantics, authorization and remaining validation. The native Lutris/OpenTTD
result is limited to its stated version and conditions. Managed Flatpak has
boundary-probe evidence, but no supported game integration. A dispatcher to an existing
process outside the scope is not lifetime evidence. Both modes keep all game
execution unprivileged; the daemon never executes commands.

Late failures now appear in Game Library's **Launch issues**, including failures
that a managed worker reports before it can register a daemon session. The
desktop history remains available after reopening. `omen-cli game-launch-issues
--json` reads the same saved reports; `game-launch-issues-clear` explicitly resets
them. Reports store closed reasons and exit status, without command arguments or
captured output. A launch-process report does not prove game lifetime and never
authorizes profile restoration; see [managed sessions](MANAGED_GAME_SESSIONS.md)
for storage, reset and observer limitations.

## Exclusions and evidence

The library's **Library locations** section adds directories for the supported
Steam, Heroic, Lutris and Legendary record formats, with an explicit supported
installation route. Steam offers Native, Flatpak and Snap; Heroic/Lutris offer
Native/Flatpak and Legendary uses its selected native configuration. Locations are user-scoped, included in user backups, and limited to 32
absolute paths. Steam locations contain `steamapps`; Heroic locations contain
its store directories; Lutris locations contain `pga.db`; Legendary locations
contain `installed.json`. Missing and malformed
records remain skipped. Default locations are scanned first, and duplicate IDs
use their first discovered record. Legendary receives its selected directory. Extra Steam, Heroic and Lutris
directories do not change the launcher's active configuration: a nonstandard
Heroic/Lutris database may require the launcher to be configured separately. Discovery is an inventory, not proof
that the selected launcher can execute a record. Tests cover explicit roots,
native/Flatpak IDs, missing records and unchanged Lutris database contents.
Discovery is capped at 10,000 entries, and the displayed count is not a complete
inventory guarantee.

Binary Steam shortcuts are read only for the account marked MostRecent in that
installation's `config/loginusers.vdf`. IDs include the account and unsigned
stored shortcut ID. Launch preparation rechecks that selection; it does not sign
in, force an account or verify a running Steam client's account. Modern records
must contain a valid `appid`; missing/ambiguous account metadata and unsupported
binary types are skipped independently of app manifests. No shortcut executable
or launch-option text is reconstructed into a shell command.

Legendary routes retain their discovered configuration directory. Versions
0.20.34 or later in the 0.20/0.21 series are admitted by a bounded `--version`
probe before profile mutation; older/unknown series fail with an update or Custom
command instruction. Default locations win duplicate IDs and the selected root
is visible in the row/JSON inventory. Additional locations do not switch the
active configuration of Steam, Heroic or Lutris.

Heroic sideload/Zoom/third-party records, Legendary sandbox installations and
AppImages without a matching PATH launcher remain outside automatic routing.
See [expanded launcher support](LAUNCHER_EXPANSION.md) for source identities,
configuration behavior, verification and remaining runtime limits.
Case-insensitive filesystem path searches and
complete KeyValues conditional syntax are not implemented. Explicit Custom
commands remain available. The game library provides an asynchronous rescan and name/ID search. A failed
scan leaves the previous results visible and reports the error. Unreadable or
invalid individual store records are skipped, so the count is not a completeness
guarantee. Each launch resolves the current records again.

The implementation follows pinned primary sources, not inferred launcher shell
strings: [Heroic protocol handling](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/509a34feea8541b73c928e289f2cf262d432db4a/src/backend/protocol.ts),
[Heroic store readers](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/tree/509a34feea8541b73c928e289f2cf262d432db4a/src/backend/storeManagers),
[Lutris schema](https://github.com/lutris/lutris/blob/1f32fe0e46e2a5d92fad9db2846c7cb7becd20c3/lutris/database/schema.py),
[Lutris command handling](https://github.com/lutris/lutris/blob/1f32fe0e46e2a5d92fad9db2846c7cb7becd20c3/lutris/gui/application.py),
and [Protontricks Steam library handling](https://github.com/Matoking/protontricks/blob/23f19063999bb016fc8e1be048b76e4c5e4c78dc/src/protontricks/steam.py).
The Flathub manifests identify the
[Steam](https://github.com/flathub/com.valvesoftware.Steam),
[Heroic](https://github.com/flathub/com.heroicgameslauncher.hgl) and
[Lutris](https://github.com/flathub/net.lutris.Lutris) application IDs.
Reviewed files, commits and SHA-256 fingerprints are retained in
`evidence/launcher-sources.json`. Rust fixtures and 16 real-process fixture
scenarios validate parsing/routing/recovery. They do not establish compatibility
with every launcher version or prove actual game execution.
