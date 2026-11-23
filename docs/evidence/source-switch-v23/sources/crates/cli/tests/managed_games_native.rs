//! Real CLI + production Unix transport/daemon service, private PPD hardware
//! fixture and user systemd scopes. No host power bus or hardware writes.
#[path = "../../hardware/tests/support/ppd.rs"]
pub mod ppd;
use omen_core::games::{GameSessionReport, SessionPhase};
use omen_core::profile::ProfileKind;
use omen_core::{OmenError, Result};
use omen_hardware::{
    mock::{MockBackend, MockModel},
    HardwareBackend,
};
use omen_ipc::{IpcClient, Op};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zbus::blocking::{connection::Builder, Proxy};
use zbus::zvariant::OwnedValue;

const CLI: &str = env!("CARGO_BIN_EXE_omen-cli");
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct PpdBackend {
    inner: MockBackend,
    address: String,
}
impl PpdBackend {
    fn ppd<T>(&self, call: impl FnOnce(Proxy<'_>) -> Result<T>) -> Result<T> {
        let conn = Builder::address(self.address.as_str())
            .map_err(|e| OmenError::Io(e.to_string()))?
            .method_timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| OmenError::Io(e.to_string()))?;
        let proxy = Proxy::new(&conn, ppd::SERVICE, ppd::OBJECT, ppd::SERVICE)
            .map_err(|e| OmenError::Io(e.to_string()))?;
        call(proxy)
    }
}
impl HardwareBackend for PpdBackend {
    fn detect(&self) -> omen_core::machine::MachineInfo {
        self.inner.detect()
    }
    fn capabilities(&self) -> omen_core::capability::Capabilities {
        self.inner.capabilities()
    }
    fn graphics(&self) -> omen_hardware::gpu::GraphicsStatus {
        self.inner.graphics()
    }
    fn read_sensors(&self) -> Result<Vec<omen_core::sensors::SensorSample>> {
        self.inner.read_sensors()
    }
    fn network(&self) -> Result<omen_core::network::NetworkSnapshot> {
        self.inner.network()
    }
    fn fan_state(&self) -> Result<omen_core::fans::FanState> {
        self.inner.fan_state()
    }
    fn set_fan_mode(&self, _: omen_core::fans::FanMode) -> Result<()> {
        Err(OmenError::Unsupported(
            "unrelated fixture control disabled".into(),
        ))
    }
    fn profile(&self) -> Result<ProfileKind> {
        self.ppd(|p| {
            let value: String = p
                .get_property("ActiveProfile")
                .map_err(|e| OmenError::Io(e.to_string()))?;
            match value.as_str() {
                "performance" => Ok(ProfileKind::Performance),
                "power-saver" => Ok(ProfileKind::Quiet),
                "balanced" => Ok(ProfileKind::Balanced),
                _ => Err(OmenError::Io("invalid fixture profile".into())),
            }
        })
    }
    fn set_profile(&self, p: ProfileKind) -> Result<()> {
        self.ppd(|proxy| {
            let holds: Vec<HashMap<String, OwnedValue>> = proxy
                .get_property("ActiveProfileHolds")
                .map_err(|e| OmenError::Io(e.to_string()))?;
            if !holds.is_empty() {
                return Err(OmenError::Unsupported(
                    "another application still holds a profile".into(),
                ));
            }
            proxy
                .set_property("ActiveProfile", p.ppd_name().unwrap())
                .map_err(|e| OmenError::Io(e.to_string()))
        })
    }
    fn hold_profile(
        &self,
        p: ProfileKind,
        reason: &str,
    ) -> Result<omen_hardware::profiles::holds::ProfileHold> {
        omen_hardware::profiles::holds::ProfileHold::at_address(
            &self.address,
            ppd::SERVICE,
            p,
            reason,
        )
    }
    fn lighting_state(&self) -> Result<omen_core::lighting::LightingState> {
        self.inner.lighting_state()
    }
    fn set_lighting(&self, _: &omen_core::lighting::LightingState) -> Result<()> {
        Err(OmenError::Unsupported("fixture control disabled".into()))
    }
    fn battery(&self) -> Result<omen_core::power::BatteryInfo> {
        self.inner.battery()
    }
    fn ac(&self) -> Result<omen_core::power::AcInfo> {
        self.inner.ac()
    }
    fn set_charge_threshold(&self, _: u8) -> Result<()> {
        Err(OmenError::Unsupported("fixture control disabled".into()))
    }
    fn charge_threshold(&self) -> Result<Option<u8>> {
        self.inner.charge_threshold()
    }
    fn detect_games(&self) -> Vec<omen_core::games::Game> {
        Vec::new()
    }
}

fn wait(mut predicate: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{message}");
}
fn report(socket: &Path) -> GameSessionReport {
    IpcClient::new(socket)
        .call_decode(Op::GetGameSessions)
        .unwrap()
}
fn invoke(socket: &Path, op: Op) -> Result<String> {
    IpcClient::new(socket).call_decode(op)
}
fn server(directory: &Path, address: &str, mock: bool) -> (Process, PathBuf, PathBuf) {
    fs::create_dir_all(directory).unwrap();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("ctl.sock");
    let journal = directory.join("sessions.json");
    let log = fs::File::create(directory.join("service.log")).unwrap();
    let mut child = Process(
        Command::new(std::env::current_exe().unwrap())
            .arg("fixture-service")
            .arg(&socket)
            .arg(&journal)
            .arg(address)
            .arg(if mock { "mock" } else { "authorized" })
            .env("DBUS_SYSTEM_BUS_ADDRESS", address)
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "READY");
    (child, socket, journal)
}
fn game(socket: &Path, id: &str, marker: &Path, millis: u64, detach: bool) -> Process {
    let log = fs::File::create(marker.with_extension("log")).unwrap();
    Process(
        Command::new(CLI)
            .arg("--socket")
            .arg(socket)
            .args(["game-run", id, "--"])
            .arg(std::env::current_exe().unwrap())
            .arg("game-child")
            .arg(marker)
            .arg(millis.to_string())
            .arg(if detach { "detach" } else { "wait" })
            .args(["literal $HOME ${NOT_EXPANDED}", "two words", "--flag=α"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap(),
    )
}
fn finish(child: &mut Process) {
    wait(
        || child.0.try_wait().unwrap().is_some(),
        "game command did not finish",
    );
}
fn end(socket: &Path, id: &str) {
    wait(
        || {
            report(socket)
                .sessions
                .iter()
                .filter(|s| s.game_id == id)
                .all(|s| !matches!(s.phase, SessionPhase::Starting | SessionPhase::Tracking))
        },
        "session did not end",
    );
}
fn manual(address: &str, profile: &str) {
    let conn = Builder::address(address).unwrap().build().unwrap();
    Proxy::new(&conn, ppd::SERVICE, ppd::OBJECT, ppd::SERVICE)
        .unwrap()
        .set_property("ActiveProfile", profile)
        .unwrap();
}
fn pass(checks: &mut Vec<String>, text: &str) {
    checks.push(text.into());
    println!("PASS {text}");
}

fn run() {
    assert_ne!(unsafe { libc::geteuid() }, 0);
    let directory = tempfile::Builder::new()
        .prefix("omen-game-integration-")
        .tempdir_in(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target"),
        )
        .unwrap();
    let kept = directory.keep();
    let root = kept.as_path();
    eprintln!("Fixture directory (removed on success): {}", root.display());
    let user = root.join("user");
    let system = root.join("system");
    fs::create_dir(&user).unwrap();
    fs::create_dir(&system).unwrap();
    std::env::set_var("OMEN_CONFIG_DIR", &user);
    std::env::set_var("OMEN_SYSTEM_CONFIG_DIR", &system);
    std::env::set_var("OMEN_HOME", root.join("home"));
    let mut config = omen_core::config::OmenConfig::default();
    for id in ["custom/session", "custom/quiet"] {
        config.games.per_game_profiles.insert(
            id.into(),
            if id.ends_with("quiet") {
                ProfileKind::Quiet
            } else {
                ProfileKind::Performance
            },
        );
        config.games.managed_game_ids.insert(id.into());
    }
    config.save_to(&user.join("config.toml")).unwrap();
    let original_user = fs::read(user.join("config.toml")).unwrap();
    let mut bus = Process(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                root.join("bus").display()
            ))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut address = String::new();
    BufReader::new(bus.0.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim();
    let fixture = ppd::Fixture::start(address);
    let (mut daemon, socket, journal) = server(&root.join("service"), address, true);
    let mut checks = Vec::new();
    assert!(report(&socket).mock);
    let marker = root.join("normal");
    let mut child = game(&socket, "custom/session", &marker, 350, false);
    wait(|| marker.exists(), "normal game did not execute");
    assert_eq!(fixture.profile(), "performance");
    let args: serde_json::Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    assert_eq!(args["uid"], unsafe { libc::geteuid() });
    assert_eq!(
        args["args"],
        serde_json::json!(["literal $HOME ${NOT_EXPANDED}", "two words", "--flag=α"])
    );
    assert!(args["request_env"].is_null());
    finish(&mut child);
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "normal exit did not release profile",
    );
    pass(&mut checks,"real game-run executes exact arguments as the desktop UID and automatically releases its profile after exit");
    let request = "aa111111111111111111111111111111";
    let status = Command::new(CLI)
        .arg("--socket")
        .arg(&socket)
        .args([
            "game-run",
            "custom/session",
            "--",
            "/bin/sh",
            "-c",
            "exit 37",
        ])
        .env(omen_client::launch_issues::REQUEST_ENV, request)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(37));
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "nonzero exit leaked a hold",
    );
    let history = omen_client::launch_issues::History::for_config(&user.join("config.toml"));
    let issue = history
        .read()
        .unwrap()
        .into_iter()
        .find(|i| i.id == request)
        .unwrap();
    assert_eq!(
        issue.reason,
        omen_client::launch_issues::Reason::ExitCode(37)
    );
    pass(&mut checks,"nonzero actual-command status is preserved, its profile is released, and a private desktop report survives exit");
    let marker = root.join("signal");
    let mut child = game(&socket, "custom/session", &marker, 20_000, false);
    wait(|| marker.exists(), "signal fixture did not execute");
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    finish(&mut child);
    assert_eq!(child.0.wait().unwrap().signal(), Some(libc::SIGTERM));
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "signalled command leaked a hold",
    );
    pass(&mut checks,"SIGTERM directed at the outside-scope observer reaches the actual command and preserves the terminating signal");
    let mut ignored = Command::new(CLI);
    ignored
        .arg("--socket")
        .arg(&socket)
        .args([
            "game-run",
            "custom/session",
            "--",
            "/bin/sh",
            "-c",
            "exit 38",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        ignored.pre_exec(|| {
            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
            Ok(())
        });
    }
    assert_eq!(ignored.status().unwrap().code(), Some(38));
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "ignored SIGCHLD fixture leaked a hold",
    );
    pass(
        &mut checks,
        "an inherited ignored SIGCHLD disposition does not lose the actual command status",
    );
    let marker = root.join("scope-refused");
    let status = Command::new(CLI)
        .arg("--socket")
        .arg(&socket)
        .args(["game-run", "custom/session", "--", "/usr/bin/touch"])
        .arg(&marker)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", root.join("no-user-bus").display()),
        )
        .env("XDG_RUNTIME_DIR", root.join("no-runtime"))
        .env(
            omen_client::launch_issues::REQUEST_ENV,
            "bb111111111111111111111111111111",
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!marker.exists());
    assert!(history
        .read()
        .unwrap()
        .iter()
        .any(|i| i.id == "bb111111111111111111111111111111"));
    assert_eq!(fixture.profile(), "balanced");
    pass(&mut checks,"failure before worker registration is saved without executing the game or acquiring a profile");
    let marker = root.join("detached");
    let mut detached = game(&socket, "custom/session", &marker, 1600, true);
    wait(|| marker.exists(), "detached game did not execute");
    finish(&mut detached);
    assert_eq!(fixture.profile(), "performance");
    assert!(report(&socket)
        .sessions
        .iter()
        .any(|s| s.game_id == "custom/session" && s.profile_held));
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "detached descendant left an orphan profile",
    );
    pass(
        &mut checks,
        "wrapper exit does not release the profile while its detached descendant is alive",
    );
    let mut performance = game(
        &socket,
        "custom/session",
        &root.join("overlap-p"),
        850,
        false,
    );
    wait(
        || root.join("overlap-p").exists(),
        "overlap performance failed",
    );
    let mut quiet = game(
        &socket,
        "custom/quiet",
        &root.join("overlap-q"),
        1650,
        false,
    );
    wait(|| root.join("overlap-q").exists(), "overlap quiet failed");
    assert_eq!(fixture.profile(), "power-saver");
    finish(&mut performance);
    end(&socket, "custom/session");
    assert_eq!(fixture.profile(), "power-saver");
    finish(&mut quiet);
    end(&socket, "custom/quiet");
    wait(
        || fixture.profile() == "balanced",
        "overlap baseline was not restored",
    );
    pass(&mut checks,"overlapping sessions release independently and use provider priority without a last-launch restore stack");
    let mut child = game(&socket, "custom/session", &root.join("manual"), 1200, false);
    wait(|| root.join("manual").exists(), "manual fixture failed");
    invoke(
        &socket,
        Op::SetProfile {
            profile: ProfileKind::Performance,
        },
    )
    .unwrap();
    assert!(!report(&socket).sessions.iter().any(|s| s.profile_held));
    finish(&mut child);
    end(&socket, "custom/session");
    assert_eq!(fixture.profile(), "performance");
    manual(address, "balanced");
    pass(
        &mut checks,
        "an explicit same-value daemon profile choice revokes owned holds and survives game exit",
    );
    let mut child = game(
        &socket,
        "custom/session",
        &root.join("external"),
        2700,
        false,
    );
    wait(
        || root.join("external").exists(),
        "external-change fixture failed",
    );
    manual(address, "power-saver");
    manual(address, "performance");
    wait(
        || !report(&socket).sessions.iter().any(|s| s.profile_held),
        "external cancellation not reflected",
    );
    finish(&mut child);
    end(&socket, "custom/session");
    assert_eq!(fixture.profile(), "performance");
    manual(address, "balanced");
    pass(
        &mut checks,
        "external A-to-B-to-A selection cancels ownership and is never overwritten after exit",
    );
    let refused = IpcClient::new(&socket)
        .call(Op::StartManagedGame {
            token: "0123456789abcdef0123456789abcdef".into(),
            game_id: "custom/session".into(),
            profile: ProfileKind::Performance,
        })
        .unwrap();
    assert!(!refused.ok);
    assert_eq!(fixture.profile(), "balanced");
    pass(
        &mut checks,
        "production peer binding refuses managed registration from an ordinary application scope",
    );
    // An inherited connection retains the original connector's credentials.
    // Keep a descendant in its valid scope and replay its otherwise-idempotent
    // registration after the original connector exits. The pidfd must reject it.
    let inherited = root.join("inherited-peer.json");
    let script = r#"import json,os,re,socket,sys,time
token=re.search(r'omen-game-([0-9a-f]{32})[.]scope',open('/proc/self/cgroup').read()).group(1)
s=socket.socket(socket.AF_UNIX);s.settimeout(5);s.connect(sys.argv[1]);reader=s.makefile('rb')
s.sendall(json.dumps({'v':1,'id':1,'op':{'op':'get_game_sessions'}}).encode()+b'\n')
assert json.loads(reader.readline())['ok']
original=os.getpid()
if os.fork(): os._exit(0)
while os.getppid()==original: time.sleep(.01)
s.sendall(json.dumps({'v':1,'id':2,'op':{'op':'start_managed_game','args':{'token':token,'game_id':'custom/session','profile':'performance'}}}).encode()+b'\n')
reply=json.loads(reader.readline());open(sys.argv[2],'w').write(json.dumps(reply))
"#;
    let mut inherited_process = Process(
        Command::new(CLI)
            .arg("--socket")
            .arg(&socket)
            .args([
                "game-run",
                "custom/session",
                "--",
                "/usr/bin/python3",
                "-c",
                script,
            ])
            .arg(&socket)
            .arg(&inherited)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait(
        || inherited.exists(),
        "inherited-peer fixture did not reply",
    );
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(&inherited).unwrap()).unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(
        reply
            .to_string()
            .contains("original socket peer has exited"),
        "{reply}"
    );
    finish(&mut inherited_process);
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "inherited-peer fixture leaked a hold",
    );
    pass(&mut checks, "a live descendant cannot replay registration through an inherited socket after its original peer exits");
    let missing = root.join("missing-exec");
    let status = Command::new(CLI)
        .arg("--socket")
        .arg(&socket)
        .args(["game-run", "custom/session", "--"])
        .arg(&missing)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "failed exec leaked hold",
    );
    pass(
        &mut checks,
        "failed actual-command exec releases its hold and returns an error",
    );
    let mut children = Vec::new();
    for n in 0..4 {
        let marker = root.join(format!("quota-{n}"));
        children.push(game(&socket, "custom/session", &marker, 1800, false));
        wait(|| marker.exists(), "quota fixture failed");
    }
    let fifth = root.join("quota-refused");
    let mut extra = game(&socket, "custom/session", &fifth, 30, false);
    finish(&mut extra);
    assert!(!fifth.exists());
    assert_eq!(
        report(&socket)
            .sessions
            .iter()
            .filter(|s| s.profile_held)
            .count(),
        4
    );
    for child in &mut children {
        finish(child);
    }
    end(&socket, "custom/session");
    wait(
        || fixture.profile() == "balanced",
        "quota fixtures did not release",
    );
    pass(&mut checks,"four concurrent user sessions are admitted and a fifth fails before executing or acquiring a hold");
    let mut child = game(
        &socket,
        "custom/session",
        &root.join("release"),
        1200,
        false,
    );
    wait(|| root.join("release").exists(), "release fixture failed");
    let token = report(&socket)
        .sessions
        .iter()
        .find(|s| s.profile_held)
        .unwrap()
        .token
        .clone();
    invoke(&socket, Op::ReleaseGameSession { token }).unwrap();
    wait(
        || fixture.profile() == "balanced",
        "explicit release failed",
    );
    assert!(child.0.try_wait().unwrap().is_none());
    finish(&mut child);
    end(&socket, "custom/session");
    pass(
        &mut checks,
        "explicit release drops the owned profile without terminating the game",
    );
    let mut child = game(&socket, "custom/session", &root.join("policy"), 1500, false);
    wait(|| root.join("policy").exists(), "policy fixture failed");
    let ipc = IpcClient::new(&socket);
    let mut settings: omen_core::settings::ServiceSettings =
        ipc.call_decode(Op::GetServiceSettings).unwrap();
    settings.power.battery_aware = true;
    ipc.call(Op::SetServiceSettings {
        settings: settings.clone(),
    })
    .unwrap()
    .decode::<String>()
    .unwrap();
    wait(
        || fixture.profile() == "balanced",
        "policy transition did not release hold",
    );
    finish(&mut child);
    end(&socket, "custom/session");
    settings.power.battery_aware = false;
    ipc.call(Op::SetServiceSettings { settings })
        .unwrap()
        .decode::<String>()
        .unwrap();
    pass(
        &mut checks,
        "battery policy takes ownership only after managed holds are released",
    );
    let mut child = game(&socket, "custom/session", &root.join("crash"), 3200, false);
    wait(|| root.join("crash").exists(), "crash fixture failed");
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    wait(
        || fixture.profile() == "balanced",
        "daemon SIGKILL did not release PPD peer",
    );
    let (replacement, replacement_socket, _) = server(&root.join("service"), address, true);
    let recovered = report(&replacement_socket);
    assert!(recovered
        .sessions
        .iter()
        .any(|s| s.phase == SessionPhase::Interrupted));
    assert!(!recovered.sessions.iter().any(|s| s.profile_held));
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(fixture.profile(), "balanced");
    finish(&mut child);
    pass(&mut checks,"daemon SIGKILL releases profiles; restart recovers interrupted history without reapplying old authority");
    assert_eq!(
        fs::metadata(&journal).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!fs::read_to_string(&journal).unwrap().contains("cookie"));
    assert_eq!(fs::read(user.join("config.toml")).unwrap(), original_user);
    pass(&mut checks,"the durable private journal contains recovery information without cookies and leaves desktop settings unchanged");
    let (denied, denied_socket, _) = server(&root.join("denied"), address, false);
    let denied_marker = root.join("denied-game");
    let mut child = game(&denied_socket, "custom/session", &denied_marker, 10, false);
    finish(&mut child);
    assert!(!denied_marker.exists());
    assert_eq!(fixture.profile(), "balanced");
    pass(&mut checks,"production authorization fails closed on a private bus without polkit and no game command runs");
    assert!(history
        .read()
        .unwrap()
        .iter()
        .any(|i| i.reason == omen_client::launch_issues::Reason::Authorization));
    let bytes = fs::read_to_string(user.join("launch-issues/issues.json")).unwrap();
    for forbidden in [
        "literal $HOME",
        "two words",
        "--flag",
        "cookie",
        "private-command",
    ] {
        assert!(!bytes.contains(forbidden));
    }
    assert_eq!(
        fs::metadata(user.join("launch-issues"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(user.join("launch-issues/issues.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    pass(&mut checks,"registration denial is reported with a closed reason, no command or output, and desktop-private permissions");
    drop(denied);
    let bad = root.join("corrupt");
    fs::create_dir(&bad).unwrap();
    fs::write(bad.join("sessions.json"), "invalid journal").unwrap();
    fs::set_permissions(bad.join("sessions.json"), fs::Permissions::from_mode(0o600)).unwrap();
    let (corrupt, corrupt_socket, _) = server(&bad, address, true);
    let marker = root.join("corrupt-game");
    let mut child = game(&corrupt_socket, "custom/session", &marker, 10, false);
    finish(&mut child);
    assert!(!marker.exists());
    assert_eq!(
        fs::read_to_string(bad.join("sessions.json")).unwrap(),
        "invalid journal"
    );
    assert!(report(&corrupt_socket).recovery_error.is_some());
    pass(&mut checks,"an unreadable recovery journal is preserved and blocks new profile ownership before execution");
    drop(corrupt);
    drop(replacement);
    drop(daemon);
    drop(fixture);
    drop(bus);
    fs::remove_dir_all(root).unwrap();
    let out = std::env::var_os("OMEN_TEST_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/managed-game-integration"));
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("managed-games-native.json"),serde_json::to_string_pretty(&serde_json::json!({"result":"passed","fixture_only":true,"physical_hardware_writes":false,"system_power_bus_used":false,"debug_assertions":cfg!(debug_assertions),"cli":CLI,"checks_passed":checks,"cleanup_complete":true})).unwrap()+"\n").unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("ppd-service") => {
            let _fixture = ppd::Fixture::start(&args[2]);
            println!("READY");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        Some("fixture-service") => {
            let backend = Arc::new(PpdBackend {
                inner: MockBackend::new(MockModel::VictusFb),
                address: args[4].clone(),
            });
            let config = omen_core::config::OmenConfig::load_from(
                &omen_core::config::OmenConfig::daemon_path(),
            )
            .unwrap();
            let service = omen_daemon::service::HardwareService::new_with_game_journal(
                backend,
                args[5] == "mock",
                config,
                Some(PathBuf::from(&args[3])),
            );
            let server =
                omen_daemon::server::Server::bind(Path::new(&args[2]), service.clone()).unwrap();
            println!("READY");
            std::io::stdout().flush().unwrap();
            server.run().unwrap();
            service.restore_safe_state();
        }
        Some("game-child") => {
            let millis: u64 = args[3].parse().unwrap();
            fs::write(&args[2],serde_json::to_vec(&serde_json::json!({"uid":unsafe{libc::geteuid()},"pid":std::process::id(),"args":&args[5..],"request_env":std::env::var_os(omen_client::launch_issues::REQUEST_ENV)})).unwrap()).unwrap();
            if args[4] == "detach" {
                // Deliberately detach the fixture; its parent test waits for the
                // daemon to observe the final descendant and collected scope.
                #[allow(clippy::zombie_processes)]
                Command::new("/bin/sleep")
                    .arg(format!("{}", millis as f64 / 1000.0))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
            } else {
                std::thread::sleep(Duration::from_millis(millis));
            }
        }
        _ => run(),
    }
}
