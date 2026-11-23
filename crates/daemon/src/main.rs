//! `omen-daemon` — the privileged control daemon.
//!
//! Safety contract (see docs/HARDWARE_SAFETY.md):
//! * starts in read-only discovery mode,
//! * performs no hardware writes on startup,
//! * keeps live fan/RGB/charge writes disabled pending hardware validation,
//! * uses a separate writer with pinned ordinary files for fixture fan leases.
//!   Real hardware recovery after SIGKILL or suspend/resume is unvalidated.

use omen_daemon::{coarse, fan_guard, nvidia_reader, server, service, TERMINATE};

use omen_core::config::OmenConfig;
use omen_hardware::backend::SysfsBackend;
use omen_hardware::mock::{MockBackend, MockModel};
use service::HardwareService;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--internal-fan-guard") {
        if let Err(error) = fan_guard::child_entry() {
            eprintln!("fan guardian: {error}");
            std::process::exit(1);
        }
        return;
    }
    let argument = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    if args.get(1).map(String::as_str) == Some("--coarse-recovery")
        || args.get(1).map(String::as_str) == Some("--coarse-recovery-fixture")
    {
        let instance = argument("--instance").map(String::as_str).unwrap_or("");
        let fixture = if args[1] == "--coarse-recovery-fixture" {
            args.get(2).map(std::path::Path::new)
        } else {
            None
        };
        if (args[1] == "--coarse-recovery-fixture" && fixture.is_none())
            || coarse::run(instance, fixture)
                .map_err(|e| eprintln!("coarse recovery: {e}"))
                .is_err()
        {
            std::process::exit(2);
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("--coarse-probe-fixture") {
        let result = match (args.get(2), args.get(3)) {
            (Some(path), Some(op)) => coarse::fixture_probe(std::path::Path::new(path), op),
            _ => Err(omen_core::OmenError::InvalidArgument(
                "fixture probe requires socket and operation".into(),
            )),
        };
        if let Err(e) = result {
            eprintln!("fixture probe: {e}");
            std::process::exit(2);
        }
        return;
    }
    let coarse_target = argument("--experimental-coarse-fans-for");
    let coarse_fixture = argument("--mock-recovery");
    if args.get(1).map(String::as_str) == Some("--nvidia-reader")
        || args.get(1).map(String::as_str) == Some("--nvidia-reader-fixture")
    {
        let fixture = if args[1] == "--nvidia-reader-fixture" {
            args.get(2).map(std::path::Path::new)
        } else {
            None
        };
        if (args[1] == "--nvidia-reader-fixture" && fixture.is_none())
            || nvidia_reader::run(fixture)
                .map_err(|e| eprintln!("NVIDIA reader: {e}"))
                .is_err()
        {
            std::process::exit(2);
        }
        return;
    }
    if matches!(
        args.get(1).map(String::as_str),
        Some("--nvidia-reader-probe-fixture" | "--nvidia-reader-probe")
    ) {
        match omen_hardware::nvidia_bridge::read(args[1] == "--nvidia-reader-probe-fixture") {
            Ok(s) => println!("{}", serde_json::to_string(&s).unwrap()),
            Err(e) => {
                eprintln!("NVIDIA reader: {e}");
                std::process::exit(2);
            }
        }
        return;
    }
    let mock_reader = args.iter().any(|a| a == "--mock-nvidia-reader");
    let mock = args.iter().any(|a| a == "--mock");
    let mock_model = if args.iter().any(|a| a == "--mock-omen") {
        Some(MockModel::FanControlOmen)
    } else if mock {
        Some(MockModel::VictusFb)
    } else {
        None
    };
    // Any mock model implies mock mode (temp socket, authorization bypass).
    let mock = mock || mock_model.is_some();
    if (mock_reader && !mock)
        || (coarse_fixture.is_some() && !mock)
        || (coarse_target.is_some() && mock)
        || (args.iter().any(|a| a == "--mock-recovery") && coarse_fixture.is_none())
        || (args.iter().any(|a| a == "--experimental-coarse-fans-for") && coarse_target.is_none())
    {
        eprintln!(
            "coarse writer options require an explicit live target or a separate mock endpoint"
        );
        std::process::exit(2);
    }
    env_logger_init();

    log::info!(
        "omen-daemon {} starting (mock={mock})",
        env!("CARGO_PKG_VERSION")
    );

    let profile_target = args
        .iter()
        .position(|a| a == "--enable-profiles-for")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.split_once(':').map(|(a, b)| (a.to_owned(), b.to_owned())));
    if args.iter().any(|a| a == "--enable-profiles-for") && !matches!(profile_target, Some(Some(_)))
    {
        eprintln!(
            "--enable-profiles-for requires BOARD:BIOS; this is an explicit hardware-write opt-in"
        );
        std::process::exit(2);
    }
    let config_path = OmenConfig::daemon_path();
    let config = match OmenConfig::load_from(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!(
                "config load from {} failed ({e}); using safe defaults",
                config_path.display()
            );
            OmenConfig::default()
        }
    };

    let service = match mock_model {
        Some(model) => {
            let mut backend = MockBackend::new(model);
            if mock_reader {
                backend.inner.enable_isolated_nvidia(true);
            }
            let backend = Arc::new(backend);
            let supervised = if let Some(path) = coarse_fixture {
                fan_guard::SupervisedBackend::coarse(
                    backend.clone(),
                    coarse::Client::fixture(if path == "auto" {
                        backend
                            .inner
                            .root()
                            .join("run/omen-fan-recovery/control.sock")
                    } else {
                        path.into()
                    }),
                )
            } else {
                fan_guard::SupervisedBackend::fixture(backend.clone(), backend.inner.root())
            };
            HardwareService::new(Arc::new(supervised), true, config)
        }
        None => {
            let mut backend = SysfsBackend::new();
            if let Some(Some((board, bios))) = profile_target {
                if let Err(e) = backend.enable_profiles_for(board, bios) {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            }
            if let Some(target) = coarse_target {
                let Some((machine, instance)) = target.rsplit_once(':') else {
                    eprintln!("coarse target requires BOARD:BIOS:KERNEL:hwmonN");
                    std::process::exit(2);
                };
                if machine != coarse::TARGET
                    || coarse::machine_target(backend.root()) != coarse::TARGET
                    || !coarse::instance_valid(instance)
                {
                    eprintln!("unverified coarse fan target");
                    std::process::exit(2);
                }
                backend.enable_isolated_nvidia(false);
                let supervised = fan_guard::SupervisedBackend::coarse(
                    Arc::new(backend),
                    coarse::Client::live(instance.into()),
                );
                HardwareService::new(Arc::new(supervised), false, config)
            } else {
                HardwareService::new(Arc::new(backend), false, config)
            }
        }
    };

    // Signal/panic hooks only set an atomic flag; recovery runs in ordinary
    // thread context, never while executing a signal handler or panic hook.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("daemon panic: {info}");
        TERMINATE.store(true, std::sync::atomic::Ordering::SeqCst);
    }));

    service.apply_startup_policy();

    let socket = if mock {
        std::env::temp_dir().join(format!("omen-mock-{}.sock", std::process::id()))
    } else {
        omen_ipc::default_socket_path()
    };
    let server = match server::Server::bind(&socket, service.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("cannot bind {}: {e}", socket.display());
            service.restore_safe_state();
            std::process::exit(1);
        }
    };
    log::info!("listening on {}", server.socket_path().display());
    if mock {
        // Print for test harnesses.
        println!("{}", server.socket_path().display());
    }

    unsafe {
        libc::signal(
            libc::SIGTERM,
            handle_term as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, handle_term as *const () as libc::sighandler_t);
    }
    {
        let service = service.clone();
        let socket = socket.clone();
        std::thread::Builder::new()
            .name("fan-policy".into())
            .spawn(move || loop {
                if TERMINATE.load(std::sync::atomic::Ordering::SeqCst)
                    || service.shutdown_requested()
                {
                    service.restore_safe_state();
                    let _ = std::fs::remove_file(&socket);
                    return;
                }
                service.fan_policy_tick();
                std::thread::sleep(std::time::Duration::from_secs(1));
            })
            .expect("fan policy worker");
    }
    {
        let service = service.clone();
        std::thread::Builder::new()
            .name("power-policy".into())
            .spawn(move || {
                while !TERMINATE.load(std::sync::atomic::Ordering::SeqCst)
                    && !service.shutdown_requested()
                {
                    service.power_policy_tick();
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
            })
            .expect("power policy worker");
    }

    if let Err(e) = server.run() {
        log::error!("accept loop failed: {e}");
        service.restore_safe_state();
        std::process::exit(1);
    }
    service.restore_safe_state();
    let _ = std::fs::remove_file(&socket);
}

extern "C" fn handle_term(_sig: i32) {
    TERMINATE.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn env_logger_init() {
    // Minimal logger to stderr (journald captures it under systemd).
    struct StderrLog;
    impl log::Log for StderrLog {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Info
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                eprintln!("[{}] {}", record.level(), record.args());
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: StderrLog = StderrLog;
    let level = std::env::var("OMEN_LOG")
        .ok()
        .and_then(|v| v.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Info);
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(level);
}
