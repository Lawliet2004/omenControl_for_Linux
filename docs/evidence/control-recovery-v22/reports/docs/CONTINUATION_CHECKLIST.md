# Continuation requirements and evidence

Read-only recheck on 2026-10-26: Victus fb1002AX, board 8C30, BIOS F.10,
kernel 7.0.0-38-generic. Revision 10 archive matches its recorded 2,678,566
bytes and SHA-256 12ee20d102c7c129eb080564805bbaaf150d1e20c8cfb9623503e16bba2b7400.
No applicable AGENTS.md was found. The checkout has no Git metadata.

Revision 22 recovers controls from late capability/readback changes, preserves
edits, refreshes graphics inventory and discloses advanced curve editing. The
complete 0–110 °C curve schema range is preserved. Twelve native recovery/keyboard
workflows pass per build profile; twelve final scenes pass with 273 measured
active text-label contrast samples. Exact final package/VM attribution is in
`CONTROL_PAGE_RECOVERY_2026-10-28.md`. Automatic direct-to-daemon migration and
source/fixture-provenance refresh remain software requirements alongside the
existing physical and full accessibility gates.

Revision 21 now has one actual native Lutris/OpenTTD result: six game checks in
a separate 39-check fresh VM suite. Real launch/scope/hold lifetime, normal exit,
explicit release and service restart pass with a private provider; guest game
fullscreen transitions pass with software rendering. The game and free graphics
total about 33 MB. See `REAL_GAME_VALIDATION_2026-10-28.md`. Additional games,
other launchers/accounts, GPU/physical behavior and managed Flatpak remain open.

| Requirement | Existing implementation / fixture evidence | Physical or external evidence / remaining gate |
|---|---|---|
| Fan ownership and recovery | Revision 14: Automatic/Maximum UI actions, exact-target independent systemd writer, durable intent/restart recovery, 10-second Maximum ceiling, isolated NVIDIA temperature reader; 205 Rust tests per profile, 13 recovery scenarios and four native workflows per profile | Default monitoring stays read-only and all host services are inactive. Host writes, RPM settling, thermal response and suspend/recovery remain unverified; the exact-target first trial is documented in `COARSE_FAN_VALIDATION_PLAN.md`. This 8C30 target exposes no manual PWM percentage/curve control |
| Games and profile restoration | Revisions 16–18: actual-command wrappers, peer-bound daemon holds through real user-scope lifetime, final-descendant release, overlap/policy handling, interrupted history and separate private desktop launch reports; production-service/private-PPD integration checks | Revision 20 adds native Legendary, Steam Snap and account-scoped Steam shortcuts; pinned upstream Legendary executes offline synthetic games and its actual-command wrapper. Native Lutris/OpenTTD launch/exit/release/restart now passes in a private desktop VM; real store accounts/games, Steam/Proton, Heroic and additional Lutris execution, managed sandbox execution and complete job-control behavior remain unvalidated. Launcher submission alone does not establish game lifetime; reports have no profile authority |
| Keyboard and presentation | Revision 19: window-local pane/page/search keys, native control traversal, chart range/export/history, setup/recovery and network-copy workflows; generated Orca speech on private Xwayland | Other screen readers, audible synthesis, expanded/transient presentation states, additional themes/accents and physical focus/DPI remain open. A defined scene matrix is not full accessibility certification |
| Overlay and desktop integration | Revision 11: opt-in compact window, shared snapshots, stale/unavailable/source states, saved preference, native/tray/local/portal actions. 32 workflows in each debug/release × X11/Wayland variant; app-only captures inspected | Physical shortcut activation, full-screen/compositor placement/focus, effective physical DPI and full accessibility remain unverified. English-only interface is requested; additional translations are outside scope |
| Other hardware adapters | Fixture RGB/threshold adapters; real monitoring; explicit unsupported states | No verified live RGB, charging threshold, MUX, power-limit or traffic-policy interface on this target; no new writes authorized |
| Windows reference | Official SDK static analysis and pinned community source provenance | Main Gaming Hub package and compatible Windows behavioral reference missing. No repeated REA setup or engine installation needed for ordinary Rust work |
| Deployment and validation | Revision 15 package `0.1.0-15`: 19 isolated container checks and 30 systemd VM checks, with cleanup. Later revisions retain separate source/package attribution and static verification | Revision 20 adds 20 installed container lifecycle and 13 launcher-format checks with reused distribution dependencies; the unchanged package also passes 33 headless systemd VM checks, including managed sessions and two reboots. See `DEPLOYMENT_VALIDATION_2026-10-28.md`. Revision 21 adds 39 fresh Ubuntu/Plasma Wayland VM checks, including real KDE/PAM authorization and two reboots, plus missing-theme icon fallbacks; see `DESKTOP_VALIDATION_2026-10-28.md`. No host installation occurred. Physical input/suspend, broader distributions and full accessibility remain external gates |

This checklist tracks implementation separately from fixture tests, read-only
physical observations and authorized physical controls. The only completed
physical control transaction remains the earlier approved PPD switch/restore.

Revision 12 adds separate Automatic/Maximum capability and native actions with
mode-only fixture supervision. Live mode writes remain blocked. Independent
production recovery, exact-target thermal policy and approved physical validation
remain required; see `FAN_MODES.md`. Revision 11 evidence is preserved rather than
reattributed to the new binaries.

Revision 13 adds independently managed coarse-mode recovery and persistent intent
with an exact-target experimental trial path. Static packaging does not enable it.
The remaining physical gate is now a concrete five-second observation procedure
with an independent ten-second ceiling. Sustained control, physical recovery and
suspend/resume remain unvalidated. Manual/curve production work is separate.

Revision 14 adds a separate unprivileged NVIDIA telemetry reader so the isolated
controller can verify current GPU temperature without weakening its device
sandbox. The native Automatic/Maximum controls passed debug/release fixture
workflows; the package passed lifecycle and systemd VM validation. No physical
fan-control write was made.
