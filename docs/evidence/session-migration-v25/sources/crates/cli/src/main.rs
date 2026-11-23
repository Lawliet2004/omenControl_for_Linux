//! `omen-cli` — command-line interface. Talks to the daemon when present;
//! falls back to direct read-only access when it is not (writes then fail
//! with a clear "daemon required" error, never by touching hardware as root).

use clap::{Parser, Subcommand};
use omen_core::capability::Capabilities;
use omen_core::fans::{FanMode, FanState};
use omen_core::OmenError;
use omen_hardware::backend::SysfsBackend;
use omen_ipc::protocol::Op;
use omen_ipc::IpcClient;

#[derive(Parser)]
#[command(
    name = "omen-cli",
    version,
    about = "Control and monitor HP OMEN/Victus hardware on Linux",
    disable_help_subcommand = true
)]
struct Cli {
    /// Daemon socket (default /run/omen-linux/ctl.sock)
    #[arg(long, global = true)]
    socket: Option<String>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Wrap the actual game command in a tracked user scope. Never pass a URI launcher here.
    GameRun {
        id: String,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    #[command(hide = true)]
    GameWorker {
        id: String,
        #[arg(long)]
        token: String,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Read this desktop user's launch issues; no daemon is required.
    GameLaunchIssues {
        #[arg(long)]
        json: bool,
    },
    /// Clear only desktop launch reports, without changing games or profiles.
    GameLaunchIssuesClear,
    /// List your tracked process trees and profile holds.
    GameSessions {
        #[arg(long)]
        json: bool,
    },
    /// Release your session's profile hold without stopping the game.
    GameSessionRelease { token: String },
    /// Submit a game launch as the desktop user; recover its profile on early failure.
    GameLaunch {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Inspect, export or import explicitly scoped settings backups.
    Settings {
        #[command(subcommand)]
        action: SettingsCmd,
    },
    /// Show detected machine and capability report.
    Detect,
    /// Dump all current sensor readings.
    Sensors,
    /// Per-interface network rates and counters (read-only).
    Network {
        #[arg(long)]
        json: bool,
        /// Time between two acquisitions. Rates use actual sample timing.
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(250..=10000))]
        interval_ms: u64,
    },
    /// Fan state.
    Fan,
    /// Set fan mode: auto | max | manual <PCT> | curve.
    FanSet { mode: String, duty: Option<u8> },
    /// Performance profile choices.
    Profiles,
    /// Show or set the active profile.
    Profile { set: Option<String> },
    /// Keyboard lighting state.
    Lighting,
    /// Battery and AC status.
    Battery,
    /// GPU / hybrid graphics status.
    Graphics,
    /// Detected games.
    Games {
        #[arg(long)]
        json: bool,
    },
    /// Full diagnostics JSON.
    Diagnostics,
    /// Check whether the daemon is reachable.
    Status,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ScopeArg {
    User,
    Service,
}
impl From<ScopeArg> for omen_core::settings::SettingsScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::User => Self::User,
            ScopeArg::Service => Self::Service,
        }
    }
}
#[derive(Subcommand)]
enum SettingsCmd {
    Export {
        #[arg(long, value_enum, default_value = "user")]
        scope: ScopeArg,
        path: std::path::PathBuf,
    },
    /// Show the scoped, validated contents without applying anything.
    Inspect {
        #[arg(long, value_enum, default_value = "user")]
        scope: ScopeArg,
        path: std::path::PathBuf,
    },
    Import {
        #[arg(long, value_enum, default_value = "user")]
        scope: ScopeArg,
        /// Required for service policies, which may activate automation.
        #[arg(long)]
        apply_service_policy: bool,
        path: std::path::PathBuf,
    },
}

struct ReadClient {
    ipc: IpcClient,
    monitoring: omen_client::monitoring::MonitoringClient,
    direct: Option<SysfsBackend>,
}

impl ReadClient {
    fn socket_path(&self) -> &std::path::Path {
        self.ipc.socket_path()
    }
    fn call(&self, op: Op) -> omen_core::Result<omen_ipc::Response> {
        use omen_hardware::HardwareBackend;
        if matches!(op, Op::GetGames) {
            let games =
                omen_client::games::DesktopGames::new(None, omen_core::config::OmenConfig::path())
                    .games()?;
            return Ok(omen_ipc::Response::ok(0, games));
        }
        let Some(b) = &self.direct else {
            return self.monitoring.call(op);
        };
        let response = match op {
            Op::GetCapabilities => omen_ipc::Response::ok(0, b.capabilities()),
            Op::GetSensors => omen_ipc::Response::ok(0, b.read_sensors()?),
            Op::GetNetwork => omen_ipc::Response::ok(0, b.network()?),
            Op::GetFanState => omen_ipc::Response::ok(0, b.fan_state()?),
            Op::GetProfileChoices => omen_ipc::Response::ok(0, b.capabilities().profile_choices),
            Op::GetProfile => omen_ipc::Response::ok(0, b.profile()?),
            Op::GetLighting => omen_ipc::Response::ok(0, b.lighting_state()?),
            Op::GetBattery => omen_ipc::Response::ok(0, b.battery()?),
            Op::GetAc => omen_ipc::Response::ok(0, b.ac()?),
            Op::GetGraphics => {
                omen_ipc::Response::ok(0, omen_hardware::gpu::graphics_status(b.root()))
            }
            Op::ExportDiagnostics => omen_ipc::Response::ok(
                0,
                serde_json::json!({
                    "mode": "direct read-only", "mock": false, "machine": b.detect(),
                    "capabilities": b.capabilities(), "sensors": b.read_sensors()?,
                    "network": b.network().ok(),
                    "battery": b.battery().ok(), "ac": b.ac().ok(),
                    "graphics": omen_hardware::gpu::graphics_status(b.root()),
                }),
            ),
            _ => {
                return Err(OmenError::DaemonUnavailable(
                    "this operation requires the authorized daemon".into(),
                ))
            }
        };
        Ok(response)
    }
}

fn client(socket: Option<&String>) -> ReadClient {
    let ipc = match socket {
        Some(s) => IpcClient::new(s),
        None => IpcClient::default_path(),
    };
    let direct = if ipc.is_daemon_alive() {
        None
    } else {
        Some(SysfsBackend::new())
    };
    let monitoring = omen_client::monitoring::MonitoringClient::new(ipc.clone());
    ReadClient {
        ipc,
        direct,
        monitoring,
    }
}

fn main() {
    // Rust ignores SIGPIPE by default; restore the default so piping into
    // `head` terminates quietly instead of panicking on broken stdout.
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    unsafe {
        signal(13 /* SIGPIPE */, 0 /* SIG_DFL */)
    };
    let cli = Cli::parse();
    let code = run(cli);
    std::process::exit(code);
}

fn run(cli: Cli) -> i32 {
    match &cli.command {
        Cmd::GameRun { id, command } | Cmd::GameWorker { id, command, .. } => {
            let token = if let Cmd::GameWorker { token, .. } = &cli.command {
                Some(token.as_str())
            } else {
                None
            };
            return match omen_client::game_sessions::run(id, command, cli.socket.as_deref(), token)
            {
                Ok(status) => omen_client::game_sessions::exit_code(status),
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        Cmd::Games { json } => {
            let games =
                omen_client::games::DesktopGames::new(None, omen_core::config::OmenConfig::path());
            return match games.games() {
                Ok(games) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&games).unwrap());
                    } else {
                        if games.is_empty() {
                            println!(
                                "no Steam, Heroic, Lutris, Legendary or custom games were found"
                            );
                        }
                        for game in games {
                            println!(
                                "{:<32} {:<10} {}",
                                game.name,
                                format!("{:?}", game.source).to_lowercase(),
                                game.id
                            );
                        }
                    }
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        Cmd::GameLaunchIssues { json } => {
            let history = omen_client::launch_issues::History::for_config(
                &omen_core::config::OmenConfig::path(),
            );
            return match history.read() {
                Ok(issues) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&issues).unwrap());
                    } else {
                        for issue in issues.iter().rev() {
                            println!(
                                "{} · {} · {}\n{}",
                                issue.game_id,
                                issue.stage.label(),
                                issue.id,
                                issue.reason.description()
                            );
                        }
                    }
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        Cmd::GameLaunchIssuesClear => {
            return match omen_client::launch_issues::History::for_config(
                &omen_core::config::OmenConfig::path(),
            )
            .clear()
            {
                Ok(()) => {
                    println!(
                        "Saved desktop launch issues cleared. Games and profiles were not changed."
                    );
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        Cmd::GameSessions { json } => {
            let client = cli
                .socket
                .as_ref()
                .map(IpcClient::new)
                .unwrap_or_else(IpcClient::default_path);
            return match client
                .call_decode::<omen_core::games::GameSessionReport>(Op::GetGameSessions)
            {
                Ok(report) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&report).unwrap());
                    } else {
                        if let Some(error) = report.recovery_error {
                            eprintln!("Recovery journal: {error}");
                        }
                        for session in report.sessions {
                            println!(
                                "{} · {:?} · requested {} · hold {}\n{}\n{}",
                                session.game_id,
                                session.phase,
                                session.requested_profile,
                                if session.profile_held {
                                    "active"
                                } else {
                                    "released"
                                },
                                session.token,
                                session.detail
                            );
                        }
                    }
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        Cmd::GameSessionRelease { token } => {
            let client = cli
                .socket
                .as_ref()
                .map(IpcClient::new)
                .unwrap_or_else(IpcClient::default_path);
            return match client
                .call(Op::ReleaseGameSession {
                    token: token.clone(),
                })
                .and_then(|r| r.decode::<String>())
            {
                Ok(message) => {
                    println!("{message}");
                    0
                }
                Err(error) => {
                    eprintln!("{error}");
                    1
                }
            };
        }
        _ => {}
    }
    if let Cmd::GameLaunch { id, json } = &cli.command {
        let daemon = cli
            .socket
            .as_ref()
            .map(IpcClient::new)
            .unwrap_or_else(IpcClient::default_path);
        let games = omen_client::games::DesktopGames::new(
            Some(daemon),
            omen_core::config::OmenConfig::path(),
        );
        return match games.launch_game(id) {
            Ok(launch) => {
                if *json {
                    println!("{}", serde_json::json!({"ok": true, "launch": launch}));
                } else {
                    println!("{}", launch.message());
                }
                0
            }
            Err(error) => {
                if *json {
                    println!("{}", serde_json::json!({"ok": false, "error": error}));
                } else {
                    eprintln!("{error}");
                }
                1
            }
        };
    }
    if let Cmd::Settings { action } = &cli.command {
        return run_settings(cli.socket.as_deref(), action);
    }
    let c = client(cli.socket.as_ref());
    match cli.command {
        Cmd::Games { .. }
        | Cmd::Settings { .. }
        | Cmd::GameLaunch { .. }
        | Cmd::GameRun { .. }
        | Cmd::GameWorker { .. }
        | Cmd::GameSessions { .. }
        | Cmd::GameLaunchIssues { .. }
        | Cmd::GameLaunchIssuesClear
        | Cmd::GameSessionRelease { .. } => {
            unreachable!("handled before hardware discovery")
        }
        Cmd::Status => match c.call(Op::Ping) {
            Ok(r) if r.ok => {
                let info: omen_ipc::protocol::DaemonInfo = c
                    .call(Op::GetInfo)
                    .and_then(|r| r.decode())
                    .unwrap_or(omen_ipc::protocol::DaemonInfo {
                        protocol_version: 0,
                        daemon_version: "?".into(),
                        machine: Default::default(),
                        mock: false,
                        started_unix: 0,
                    });
                println!(
                    "daemon: reachable (v{}, protocol {}, pid-started {})",
                    info.daemon_version, info.protocol_version, info.started_unix
                );
                0
            }
            _ => {
                println!("daemon: not reachable at {}", c.socket_path().display());
                1
            }
        },
        Cmd::Detect => with_daemon_or_readonly(&c, |c| {
            let caps: Capabilities = c.call(Op::GetCapabilities)?.decode()?;
            print_caps(&caps);
            Ok(())
        }),
        Cmd::Network { json, interval_ms } => with_daemon_or_readonly(&c, |c| {
            let _: omen_core::network::NetworkSnapshot = c.call(Op::GetNetwork)?.decode()?;
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
            let report: omen_core::network::NetworkSnapshot = c.call(Op::GetNetwork)?.decode()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                println!(
                    "Network source: {:?}; per-interface counters, no summed total",
                    report.source
                );
                println!(
                    "{:<16} {:<16} {:>16} {:>16}",
                    "Interface", "Link", "Receive", "Send"
                );
                for interface in &report.interfaces {
                    println!(
                        "{:<16} {:<16} {:>16} {:>16}",
                        interface.name,
                        interface.link_state.as_deref().unwrap_or("unknown"),
                        omen_core::network::format_rate(
                            interface.rx_bytes,
                            interface.rx_bytes_per_second
                        ),
                        omen_core::network::format_rate(
                            interface.tx_bytes,
                            interface.tx_bytes_per_second
                        )
                    );
                }
                if report.omitted_interfaces > 0 || report.unreadable_interfaces > 0 {
                    println!(
                        "Omitted: {}; unreadable: {}",
                        report.omitted_interfaces, report.unreadable_interfaces
                    );
                }
            }
            Ok(())
        }),
        Cmd::Sensors => with_daemon_or_readonly(&c, |c| {
            let sensors: Vec<omen_core::sensors::SensorSample> =
                c.call(Op::GetSensors)?.decode()?;
            for s in &sensors {
                match (s.value, &s.text) {
                    (Some(v), _) => println!("{:<38} {:>10.2} {}", s.label, v, s.kind.unit()),
                    (None, Some(t)) => println!("{:<38} {:>10} {}", s.label, t, s.source),
                    _ => println!("{:<38} {:>10}", s.label, "-"),
                }
            }
            Ok(())
        }),
        Cmd::Fan => with_daemon_or_readonly(&c, |c| {
            let st: FanState = c.call(Op::GetFanState)?.decode()?;
            println!("{}", serde_json::to_string_pretty(&st).unwrap());
            Ok(())
        }),
        Cmd::FanSet { mode, duty } => {
            let m = match parse_fan_mode(&mode, duty) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            match c.call(Op::SetFanMode { mode: m }) {
                Ok(r) if r.ok => {
                    println!("fan mode applied: {mode}");
                    0
                }
                Ok(r) => {
                    eprintln!(
                        "error: {}",
                        r.error.unwrap_or(OmenError::Internal("?".into()))
                    );
                    1
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Cmd::Profiles => with_daemon_or_readonly(&c, |c| {
            let choices: Vec<omen_core::profile::ProfileKind> =
                c.call(Op::GetProfileChoices)?.decode()?;
            let current: omen_core::profile::ProfileKind = c.call(Op::GetProfile)?.decode()?;
            for ch in &choices {
                let marker = if *ch == current { "*" } else { " " };
                println!("{marker} {ch}");
            }
            Ok(())
        }),
        Cmd::Profile { set } => match set {
            None => with_daemon_or_readonly(&c, |c| {
                let p: omen_core::profile::ProfileKind = c.call(Op::GetProfile)?.decode()?;
                println!("{p}");
                Ok(())
            }),
            Some(name) => {
                let p = match parse_profile(&name) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("error: {e}");
                        return 1;
                    }
                };
                match c.call(Op::SetProfile { profile: p }) {
                    Ok(r) if r.ok => {
                        println!("profile set: {p}");
                        0
                    }
                    Ok(r) => {
                        eprintln!(
                            "error: {}",
                            r.error.unwrap_or(OmenError::Internal("?".into()))
                        );
                        1
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        1
                    }
                }
            }
        },
        Cmd::Lighting => with_daemon_or_readonly(&c, |c| match c
            .call(Op::GetLighting)?
            .decode::<omen_core::lighting::LightingState>(
        ) {
            Ok(st) => {
                println!("{}", serde_json::to_string_pretty(&st).unwrap());
                Ok(())
            }
            Err(e) => Err(e),
        }),
        Cmd::Battery => with_daemon_or_readonly(&c, |c| {
            let b: omen_core::power::BatteryInfo = c.call(Op::GetBattery)?.decode()?;
            let ac: omen_core::power::AcInfo = c.call(Op::GetAc)?.decode()?;
            println!("AC online: {}", ac.online);
            println!("{}", serde_json::to_string_pretty(&b).unwrap());
            Ok(())
        }),
        Cmd::Graphics => with_daemon_or_readonly(&c, |c| {
            let g: omen_hardware::gpu::GraphicsStatus = c.call(Op::GetGraphics)?.decode()?;
            for gpu in &g.gpus {
                println!(
                    "{:<14} {:<8} driver={:?} runtime={:?}{}",
                    gpu.pci_address,
                    gpu.vendor,
                    gpu.driver,
                    gpu.runtime_status,
                    if gpu.discrete { " (discrete)" } else { "" }
                );
            }
            println!(
                "MUX: {}",
                if g.mux_available {
                    "available"
                } else {
                    "not available"
                }
            );
            Ok(())
        }),
        Cmd::Diagnostics => with_daemon_or_readonly(&c, |c| {
            let v: serde_json::Value = c.call(Op::ExportDiagnostics)?.decode()?;
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
            Ok(())
        }),
    }
}

fn run_settings(socket: Option<&str>, action: &SettingsCmd) -> i32 {
    use omen_core::settings::SettingsScope;
    use omen_ipc::settings::SettingsController;
    let daemon = socket
        .map(IpcClient::new)
        .unwrap_or_else(IpcClient::default_path);
    let controller = SettingsController::new(Some(daemon), omen_core::config::OmenConfig::path());
    let result = (|| -> omen_core::Result<()> {
        match action {
            SettingsCmd::Export { scope, path } => {
                controller.export_file(path, (*scope).into())?;
                println!("Exported {}", path.display());
            }
            SettingsCmd::Inspect { scope, path } => {
                println!(
                    "{}",
                    SettingsController::read_file(path, (*scope).into())?.export_json()?
                );
            }
            SettingsCmd::Import {
                scope,
                path,
                apply_service_policy,
            } => {
                let scope = (*scope).into();
                if scope == SettingsScope::Service && !apply_service_policy {
                    return Err(OmenError::InvalidArgument("service policies may activate automation; inspect the backup first, then explicitly pass --apply-service-policy".into()));
                }
                controller.import_file(path, scope)?;
                println!(
                    "Settings saved{}",
                    if scope == SettingsScope::User {
                        "; restart the GUI to refresh views and desktop preferences"
                    } else {
                        ""
                    }
                );
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

/// Read-only commands work without the daemon via direct sysfs access;
/// when the daemon IS reachable, prefer it (single source of truth).
fn with_daemon_or_readonly(
    c: &ReadClient,
    f: impl FnOnce(&ReadClient) -> omen_core::Result<()>,
) -> i32 {
    if c.direct.is_some() {
        eprintln!("note: daemon absent; direct read-only access");
    }
    match f(c) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn print_caps(caps: &Capabilities) {
    let m = &caps.machine;
    println!("Machine   : {} {}", m.sys_vendor, m.product_name);
    println!("Family    : {}  Board: {}", m.product_family, m.board_name);
    println!(
        "BIOS      : {} ({})  Kernel: {}",
        m.bios_version, m.bios_vendor, m.kernel_version
    );
    println!("Class     : {:?}", m.class);
    println!();
    println!("{:<26} {:<12} Detail", "Capability", "Status");
    for (name, cap) in [
        ("Fan monitoring", &caps.fan_monitoring),
        ("Fan manual control", &caps.fan_control),
        ("Fan curves", &caps.fan_curve),
        ("Performance profiles", &caps.performance_profiles),
        ("Lighting zones", &caps.lighting_zones),
        ("Keyboard backlight", &caps.keyboard_backlight),
        ("Graphics MUX", &caps.graphics_mux),
        ("Charge threshold", &caps.charge_threshold),
        ("Battery-aware", &caps.battery_aware_profiles),
        ("NVIDIA telemetry", &caps.nvidia_telemetry),
        ("AMD telemetry", &caps.amd_telemetry),
    ] {
        let status = match cap {
            omen_core::capability::Capability::Available { .. } => "available",
            omen_core::capability::Capability::ReadOnly { .. } => "read-only",
            omen_core::capability::Capability::Unavailable { .. } => "unavailable",
        };
        println!("{:<26} {:<12} {}", name, status, cap.detail());
    }
}

fn parse_fan_mode(mode: &str, duty: Option<u8>) -> omen_core::Result<FanMode> {
    match mode {
        "auto" => Ok(FanMode::Auto),
        "max" => Ok(FanMode::Max),
        "manual" => {
            let d = duty.ok_or_else(|| {
                OmenError::InvalidArgument(
                    "manual mode requires a duty percentage: fan-set manual 60".into(),
                )
            })?;
            Ok(FanMode::Manual { duty_pct: d })
        }
        "curve" => Ok(FanMode::Curve),
        other => Err(OmenError::InvalidArgument(format!(
            "unknown fan mode '{other}' (auto|max|manual|curve)"
        ))),
    }
}

fn parse_profile(name: &str) -> omen_core::Result<omen_core::profile::ProfileKind> {
    use omen_core::profile::ProfileKind;
    match name.to_lowercase().as_str() {
        "quiet" | "power-saver" => Ok(ProfileKind::Quiet),
        "balanced" => Ok(ProfileKind::Balanced),
        "cool" => Ok(ProfileKind::Cool),
        "performance" => Ok(ProfileKind::Performance),
        other => Err(OmenError::InvalidArgument(format!(
            "unknown profile '{other}' (quiet|balanced|cool|performance)"
        ))),
    }
}
