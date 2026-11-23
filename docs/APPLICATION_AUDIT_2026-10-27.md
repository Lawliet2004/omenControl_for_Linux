# OMEN Control application audit

Date: 27 October 2026

This is the original audit baseline. Current implementation and evidence are in
`APPLICATION_AUDIT_IMPLEMENTATION.md`. The user subsequently specified English
only; translation proposals below are outside the accepted scope.

## Scope and verdict

This is a source and documentation audit of the Rust/GTK application, its service, existing tests, and saved desktop screenshots. It distinguishes code-confirmed behavior from proposed features and validation still required. The screenshots are historical captures, not a fresh visual test of every current screen. No hardware settings were changed. This checkout has no Git metadata, so a baseline diff was unavailable.

OMEN Control is a useful monitoring-first development application, not yet a complete or hardware-validated control suite. It already offers nine native pages, bounded sensor collection, capability explanations, a CLI, user-scoped game launch, diagnostics, tray integration and a compact overlay. Its conservative default is a strength. The most urgent work is making displayed monitoring data trustworthy and completing the game/profile lifecycle; hardware writes must remain gated by model-specific safety evidence.

Priority definitions: **P1** affects trust, availability or a major workflow; **P2** is a meaningful product or usability gap; **P3** is refinement. No issue in this audit is asserted to block every user, so none is classified P0. These priorities are not a hardware validation or security certification.

## Confirmed findings

| Priority | Finding and impact | Evidence | Recommended outcome |
| --- | --- | --- | --- |
| P1 | History can imply a continuous or synchronized timeline when readings are missing or sampled at different times. Each series stores only values, skips absent readings, and stretches independently across the full plot. A legend can show the last retained reading after a source disappears. | `gui/src/charts.rs:12`, `:127`, `:154`; `gui/src/pages.rs:139` | Preserve acquisition timestamps and missing samples; plot on one time axis with explicit gaps, age and source. Test outages and differently paced sources. The README's statement about sample acquisition times applies to underlying readings, not the present chart history. |
| P1 | If the polling worker stalls, the main window retains previous readings without aging them. Battery data can also remain visible after a snapshot omits the battery; fan fields can remain if both fan data and an error are absent. Users can mistake old information for live status. | `gui/src/main.rs:252`, `:273`; `gui/src/pages.rs:178`, `:867`, `:1257` | Refresh freshness independently of arriving snapshots, replace missing values with unavailable, and test disconnect, stall, recovery and stale capability behavior. The overlay already has separate freshness handling. |
| P1 | A successful game launch applies the bound profile but does not track the game or restore the previous profile on exit. A launcher request also does not prove a game started. The selected profile can remain active after play. | `crates/client/src/games.rs:192`; `docs/GAME_LAUNCHERS.md:56` | Make the current limitation and manual restore visible now. Add reliable lifecycle tracking, ownership and recovery rules for concurrent games, launcher failures, external profile changes and crashes before promising automatic restoration. |
| P1 | A local user able to reach the production socket can occupy all 16 connection slots indefinitely by sending data within each per-read timeout, denying service to other clients. This is an availability finding; it does not demonstrate bypass of write authorization. The bind comment also misstates production socket permissions. | `crates/daemon/src/server.rs:15`, `:24`, `:49`, `:77`, `:97` | Enforce bounded connection/session lifetime and fair per-user or request handling while preserving legitimate clients; add a saturation regression test and correct the comment. |
| P1 | Chart data is drawn in Cairo with small painted labels and color-distinguished series; no explicit accessible chart summary is apparent in the widget implementation. Users who cannot see or distinguish the lines may not obtain equivalent history. | `gui/src/charts.rs:45`, `:110`, `:127` | Provide an accessible textual summary or data table, keyboard access and line styles as well as color; verify with AT-SPI and large text. This is a source-level finding, not a complete screen-reader audit. |
| P2 | Navigation and content use a fixed-width sidebar and fixed card/column grids. Horizontal scrolling is disabled for pages. Narrow-window and large-text clipping is a credible risk, but was not directly reproduced in this audit. | `gui/src/main.rs:112`; `gui/src/pages.rs:278`, `:383`, `:403`, `:1554` | Use an adaptive libadwaita navigation split and reflow cards/rows; test small windows, large text, translated strings and keyboard traversal. |
| P2 | The nine navigation destinations form one flat list; icon names are declared but not rendered. The header presents a static application name and a prominent tray action while connection/source information is separated into the sidebar footer. The saved screens show little hierarchy between metric cards. | `gui/src/main.rs:93`, `:130`, `:153`, `:203`; `docs/screenshots/dashboard-sandboxed-service.png` | Group destinations around monitoring, controls, games and settings; use consistent symbolic icons with labels, a current-page title, and an actionable connection/health indicator. Move tray behavior into a secondary action or menu. |
| P2 | Cards show a label and a number with little context about age, provider or trend. Raw sensor lists are dense and lack discovery tools. | `gui/src/pages.rs:238`, `:272`, `:383`; `docs/screenshots/vitals.png` | Add compact age/source and clear stale/unavailable states, meaningful units and optional trends. Add sensor search, grouping and filtering, retaining access to raw identifiers. |
| P2 | Dark appearance is forced and some interface and chart colors are hard-coded. The result does not follow a light system preference and may not adapt well to high-contrast settings. Contrast and high-contrast behavior were not measured here. | `gui/src/main.rs:33`, `:499`; `gui/src/charts.rs:84` | Use semantic theme colors, honor system appearance or offer a preference, and verify contrast and legibility in light, dark and high-contrast modes. |
| P2 | Displayed game library requires an application restart to refresh, and several installation formats and custom roots are not discovered. | `docs/GAME_LAUNCHERS.md:63` | Add an explicit rescan with progress, search and clear source/error states; add user-configured locations and further launchers only with tested formats. |
| P2 | End-to-end keyboard/screen-reader accessibility, localization, real graphical authorization, physical global shortcut activation and full-screen overlay behavior have not been fully validated. | `docs/CONTINUATION_CHECKLIST.md:11`; `docs/TEST_REPORT.md` | Test actual GTK interaction and accessibility on supported desktops, translations and scaled text, graphical polkit, physical shortcuts and compositor behavior. Report outcomes per desktop instead of assuming cross-desktop support. |

## Feature inventory and remaining work

| Area | Current state | Remaining work / boundary |
| --- | --- | --- |
| Monitoring and history | CPU, GPU where exposed, fan RPM, battery, storage, network, system readings and charts; unavailable states and source information exist. | Correct chart timelines and stale handling first; add useful search, history ranges and optional export. Separate unavailable, stale and collecting states. |
| Profiles | Current state and advertised choices; standard profile writes require exact-target opt-in and authorization. One documented physical transaction passed. | Broader physical integration, battery transitions, graphical authorization and recovery validation. Game lifecycle restoration is software work still to build. |
| Fans | RPM and reported modes; Automatic/Maximum fixture paths and a separately guarded, exact-target experimental trial exist. | Live control is not validated. Physical settling, provider coverage, independent recovery, thermal policy, suspend/resume and approved target-specific testing remain release gates. The documented 8C30 target has no exposed manual PWM percentage/curve control. |
| Lighting and charging | Adapter and validation fixtures exist; unavailable reasons are surfaced. | No validated live writes; per-key lighting is unimplemented. Establish genuine interface, model compatibility and safe behavior before offering control. Interface absence alone does not prove absence of physical hardware. |
| Graphics | PCI, driver and runtime power state. | MUX/Optimus switching is not implemented; require a verified platform interface and safe reboot/recovery UX before considering it. |
| Game library | Native/Flatpak Steam, Heroic and Lutris discovery, custom commands, unprivileged launcher requests and early failure recovery. | Actual launch/lifetime verification, automatic restoration, refresh and broader formats. Records can be stale and a submitted request is not proof of execution. |
| Desktop integration | Tray, local shortcuts, opt-in portal shortcut and compact monitoring overlay. | Physical shortcut, desktop approval and compositor/full-screen behavior remain unverified. Topmost placement is desktop-dependent. |
| Settings and diagnostics | Versioned configuration, migrations, scoped backups/import preview, diagnostics export and existing native interaction tests. | Improve organization and discoverability; validate real desktop dialogs, graphical authorization, accessibility and privacy of exported details. |
| Packaging | Debian package and documented container/systemd VM lifecycle tests. | Validate clean graphical installation, physical suspend/reboot and target distributions; do not infer universal support from a single package or VM. |
| Localization | English interface. | Complete extraction, translations, locale-aware formatting, right-to-left and expanded-string layout tests. |

Detailed evidence and hardware restrictions are maintained in `docs/CONTINUATION_CHECKLIST.md`, `docs/HARDWARE_SAFETY.md`, `docs/TEST_REPORT.md`, and `docs/GAME_LAUNCHERS.md`.

## Design direction

Keep the application recognizably native to GTK/libadwaita: useful density, system typography and controls, restrained accent color and explicit state. Avoid a gaming-themed visual treatment that obscures readings or makes unavailable controls look active.

1. **Shell and top bar.** Use an adaptive split view with a collapsible sidebar. Show the current page title and a compact service/source status with a route to diagnostics. Put tray, overlay and settings actions in a predictable secondary menu. Show important errors near the affected task, with a direct recovery action where possible.
2. **Sidebar.** Group Monitor (Dashboard, Vitals), Controls (Performance, Fans, Lighting, Graphics, Battery), Library and Settings. Pair a consistent symbolic icon with each text label; keep selection, keyboard focus and status legible. Adapt groups if research shows a different task order is better.
3. **Dashboard widgets.** Make the first row answer “is the system reporting normally?” with current profile, connection and data freshness. Use a consistent card with label, prominent value and unit, concise source/age, optional trend, and explicit unavailable/stale states. Reserve warning color for meaningful state and use text or symbols as well as color. Let users choose which metrics appear only after the default hierarchy is sound.
4. **Charts and sensors.** Build accurate time-based charts with labeled time range, shared axes, data gaps, accessible summaries and selectable ranges. Group sensors by device and type; add search, sorting and plain-language descriptions while retaining raw identifiers for diagnosis.
5. **Control pages.** Present current state, available actions, exact reason for unavailable controls and the next safe action in that order. Distinguish monitoring, experimental tests and validated control. Use progressive disclosure for advanced settings and safety explanations; do not imply an unsupported action will work.
6. **Settings and recovery.** Break the long settings page into clearly titled preference groups. Make backup scope and diagnostics contents understandable before export/import, with visible completion and recovery messages. Provide a concise capability report with model, BIOS, source, last update and why features are disabled.

## Additional product opportunities

These are proposals, not claims of existing functionality:

- **Capability and health center:** one place for device identification, service connection, provider freshness, supported controls and actionable explanations.
- **Monitoring notifications:** opt-in thresholds, hysteresis and rate limits, with explicit distinction from thermal protection; no automatic hardware write.
- **Personalized monitoring:** pin useful sensors, select chart windows and export timestamped history, respecting bounded storage and privacy.
- **Game workflow:** rescan/search, clear launch receipt status, managed profile sessions and restoration with explicit ownership rules.
- **Onboarding and help:** a short first-run explanation of read-only mode, source availability and how to reach diagnostics; contextual explanations for unsupported capabilities.
- **Compatibility reporting:** a model/BIOS-specific evidence view and user-consented diagnostic export; distinguish observed, fixture-tested and physically validated support.

## Recommended delivery sequence

1. **Trust and service reliability:** timestamped chart history, missing/stale state handling, battery/fan disappearance tests, socket fairness and regression tests.
2. **Core experience:** in-app library refresh and clear game/profile status; design and implement game lifecycle and safe restoration with concurrency and failure tests.
3. **Adaptive shell and visual system:** navigation grouping, contextual header, responsive cards, semantic appearance, accessible charts and sensor search. Validate across window sizes, scaling, keyboard and AT-SPI.
4. **Hardware and desktop release gates:** model-specific controls only after documented independent recovery and physical validation; validate desktop auth, shortcuts, overlay, suspend/resume and packaging on declared targets.
5. **Enhancements:** optional alerts, customizable widgets, history export, onboarding and broader integrations based on user needs.

## Verification and limits

The audit used source inspection, existing project evidence and saved screenshot review. `cargo test --workspace --locked` passed 208 debug tests with the documented local GTK development prefix. No GUI workflow, release-build, package, VM or physical hardware tests were rerun for this audit. The charts, stale display and socket findings are reasoned from code; this audit did not reproduce them with interactive fault injection. Existing fixture/VM reports are useful evidence for their stated conditions, not proof of physical behavior. Full accessibility, contrast, responsiveness, real graphical authorization and hardware operation were not tested in this audit. No code or hardware controls were changed.
