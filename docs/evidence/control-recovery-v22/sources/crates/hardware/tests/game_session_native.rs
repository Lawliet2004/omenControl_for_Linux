//! Explicit native integration target. Uses a private PPD protocol fixture and
//! an ordinary user systemd scope; never connects to the system power service.
use omen_core::profile::ProfileKind;
use omen_hardware::game_scope::{GameScope, ScopeState};
use omen_hardware::profiles::holds::ProfileHold;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use zbus::blocking::{connection::Builder, Proxy};

const SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
const OBJECT: &str = "/org/freedesktop/UPower/PowerProfiles";

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[path = "support/ppd.rs"]
pub mod ppd;
use ppd::Fixture;

fn wait(mut test: impl FnMut() -> bool, message: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if test() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{message}");
}
fn hold(address: &str, profile: ProfileKind, reason: &str) -> ProfileHold {
    ProfileHold::at_address(address, SERVICE, profile, reason).unwrap()
}
fn manual(address: &str, profile: &str) {
    let connection = Builder::address(address).unwrap().build().unwrap();
    Proxy::new(&connection, SERVICE, OBJECT, SERVICE)
        .unwrap()
        .set_property("ActiveProfile", profile)
        .unwrap();
    connection.close().unwrap();
}
fn passed(checks: &mut Vec<String>, message: &str) {
    checks.push(message.into());
    println!("PASS {message}");
}

fn run() {
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "native tests must run as the desktop user"
    );
    let temporary = tempfile::tempdir().unwrap();
    let mut bus = ChildGuard(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                temporary.path().join("bus").display()
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
    let mut fixture = Fixture::start(address);
    let mut checks = Vec::new();
    let performance = hold(address, ProfileKind::Performance, "fixture performance");
    assert!(performance.active().unwrap());
    assert_eq!(fixture.profile(), "performance");
    let mut quiet = hold(address, ProfileKind::Quiet, "fixture quiet");
    assert_eq!(fixture.profile(), "power-saver");
    quiet.release().unwrap();
    assert_eq!(fixture.profile(), "performance");
    drop(performance);
    wait(
        || fixture.profile() == "balanced",
        "last disconnect did not restore baseline",
    );
    passed(
        &mut checks,
        "overlapping PPD holds arbitrate Quiet over Performance and release independently",
    );
    let first = hold(
        address,
        ProfileKind::Performance,
        "identical display reason",
    );
    let second = hold(
        address,
        ProfileKind::Performance,
        "identical display reason",
    );
    let cookie = *fixture.state.lock().unwrap().holds.keys().next().unwrap();
    fixture.state.lock().unwrap().holds.remove(&cookie);
    assert!(!first.active().unwrap());
    assert!(second.active().unwrap());
    drop(first);
    assert!(second.active().unwrap());
    drop(second);
    wait(
        || fixture.profile() == "balanced",
        "duplicate-reason fixture did not clean up",
    );
    passed(
        &mut checks,
        "holds with identical display text remain distinguishable after one is revoked",
    );
    let performance = hold(
        address,
        ProfileKind::Performance,
        "fixture manual cancellation",
    );
    manual(address, "performance");
    assert!(performance.active().unwrap());
    manual(address, "power-saver");
    assert!(!performance.active().unwrap());
    manual(address, "performance");
    assert!(!performance.active().unwrap());
    drop(performance);
    assert_eq!(fixture.profile(), "performance");
    manual(address, "balanced");
    passed(&mut checks,"provider same-value requests remain no-ops; a real manual/ABA change cancels the hold and old ownership cannot overwrite it");
    fixture.state.lock().unwrap().fail_reply = true;
    assert!(ProfileHold::at_address(
        address,
        SERVICE,
        ProfileKind::Performance,
        "fixture failed reply"
    )
    .is_err());
    wait(
        || fixture.profile() == "balanced",
        "lost reply left an orphan hold",
    );
    passed(
        &mut checks,
        "a failed reply after hold application closes the peer and leaves no orphan profile",
    );
    fixture.state.lock().unwrap().hide_holds = true;
    assert!(ProfileHold::at_address(
        address,
        SERVICE,
        ProfileKind::Performance,
        "unconfirmed hold"
    )
    .is_err());
    wait(
        || fixture.profile() == "balanced",
        "unconfirmed successful reply left an orphan hold",
    );
    fixture.state.lock().unwrap().hide_holds = false;
    passed(
        &mut checks,
        "a successful method reply without matching provider state is rejected and cleaned up",
    );
    let ready = temporary.path().join("holding");
    let mut helper = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["hold-child", address])
            .arg(&ready)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait(|| ready.exists(), "hold helper did not start");
    assert_eq!(fixture.profile(), "performance");
    helper.0.kill().unwrap();
    helper.0.wait().unwrap();
    wait(
        || fixture.profile() == "balanced",
        "killed holder left an orphan profile",
    );
    passed(
        &mut checks,
        "SIGKILL of a real holder process releases its private-bus hold without a recovery write",
    );
    let mut old = hold(
        address,
        ProfileKind::Performance,
        "fixture previous service",
    );
    fixture.close();
    let replacement = Fixture::start(address);
    let mut new = hold(
        address,
        ProfileKind::Performance,
        "fixture replacement service",
    );
    assert!(old.active().is_err());
    assert!(old.release().is_err());
    assert!(new.active().unwrap());
    assert_eq!(replacement.profile(), "performance");
    new.release().unwrap();
    assert_eq!(replacement.profile(), "balanced");
    drop(old);
    drop(new);
    passed(
        &mut checks,
        "service-owner pinning prevents a stale cookie from releasing a replacement PPD hold",
    );
    for profile in [ProfileKind::Balanced, ProfileKind::Cool] {
        assert!(ProfileHold::at_address(address, SERVICE, profile, "unsupported fixture").is_err());
    }
    passed(
        &mut checks,
        "unsupported hold profiles fail explicitly instead of falling back to unowned writes",
    );
    scope_checks(temporary.path(), &mut checks);
    drop(helper);
    drop(replacement);
    drop(fixture);
    drop(bus);
    temporary.close().unwrap();
    let output = std::env::var_os("OMEN_TEST_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/managed-session-evidence"));
    fs::create_dir_all(&output).unwrap();
    let evidence = serde_json::json!({"result":"passed","hardware_writes":false,"system_power_bus_used":false,
        "debug_assertions":cfg!(debug_assertions), "test_binary":std::env::current_exe().unwrap(),
        "recorded_unix":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        "profile_adapter":"real D-Bus requests to a private PPD protocol fixture","cgroup_backend":"real user systemd and kernel cgroup v2",
        "checks_passed":checks,"cleanup_complete":true});
    fs::write(
        output.join("game-session-native.json"),
        serde_json::to_string_pretty(&evidence).unwrap() + "\n",
    )
    .unwrap();
}

fn launch_scope(directory: &Path, unit: &str) -> ChildGuard {
    ChildGuard(
        Command::new("systemd-run")
            .args([
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--expand-environment=no",
            ])
            .arg(format!("--unit={unit}"))
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .arg("scope-child")
            .arg(directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn scope_checks(directory: &Path, checks: &mut Vec<String>) {
    let token = format!("{:032x}", std::process::id());
    let unit = format!("omen-game-{token}.scope");
    let mut runner = launch_scope(directory, &unit);
    let ready = directory.join("scope-pid");
    wait(
        || ready.exists(),
        "native scope helper did not start (a logged-in user systemd manager is required)",
    );
    let pid: u32 = fs::read_to_string(&ready).unwrap().parse().unwrap();
    let uid = unsafe { libc::geteuid() };
    let scope = GameScope::for_process(uid, pid, &token).unwrap();
    assert_eq!(scope.state().unwrap(), ScopeState::Populated);
    assert!(GameScope::for_process(uid + 1, pid, &token).is_err());
    assert!(GameScope::for_process(uid, pid, "ffffffffffffffffffffffffffffffff").is_err());
    passed(checks,"scope registration derives the live desktop UID and dedicated unit from procfs, rejecting another UID/token");
    fs::write(directory.join("scope-go"), "go").unwrap();
    wait(
        || {
            // systemd-run may exec the worker in place, so reap it when it is
            // our direct child. A zombie has exited even before it is reaped.
            let _ = runner.0.try_wait().unwrap();
            match fs::read_to_string(format!("/proc/{pid}/stat")) {
                Ok(stat) => stat.rsplit_once(')').is_some_and(|(_, tail)| {
                    matches!(tail.split_whitespace().next(), Some("Z" | "X"))
                }),
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            }
        },
        "scope wrapper did not exit",
    );
    assert_eq!(scope.state().unwrap(), ScopeState::Populated);
    passed(checks,"the pinned scope remains populated after the wrapper exits while a detached descendant is alive");
    wait(
        || scope.state().is_ok_and(|s| s == ScopeState::Empty),
        "scope did not become empty after the final descendant exited",
    );
    wait(
        || runner.0.try_wait().unwrap().is_some(),
        "systemd-run did not exit",
    );
    let path = Path::new("/sys/fs/cgroup").join(&scope.identity().relative_path);
    wait(|| !path.exists(), "collected fixture scope was not removed");
    passed(
        checks,
        "the original pinned scope reports empty only after its final descendant exits",
    );
    let second_directory = directory.join("second");
    fs::create_dir(&second_directory).unwrap();
    let mut second_runner = launch_scope(&second_directory, &unit);
    let second_ready = second_directory.join("scope-pid");
    wait(
        || second_ready.exists(),
        "replacement fixture scope did not start",
    );
    let second_pid = fs::read_to_string(second_ready).unwrap().parse().unwrap();
    let second = GameScope::for_process(uid, second_pid, &token).unwrap();
    assert_eq!(second.state().unwrap(), ScopeState::Populated);
    assert_ne!(scope.identity().inode, second.identity().inode);
    assert_eq!(scope.state().unwrap(), ScopeState::Empty);
    fs::write(second_directory.join("scope-go"), "go").unwrap();
    wait(
        || second.state().is_ok_and(|s| s == ScopeState::Empty),
        "replacement scope did not empty",
    );
    wait(
        || second_runner.0.try_wait().unwrap().is_some(),
        "replacement systemd-run did not exit",
    );
    wait(|| !path.exists(), "replacement scope was not collected");
    passed(checks, "reusing a collected unit name creates a different inode and cannot revive the old tracked scope");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("hold-child") => {
            let _hold = hold(&args[2], ProfileKind::Performance, "fixture killed holder");
            fs::write(&args[3], "ready").unwrap();
            loop {
                std::thread::park();
            }
        }
        Some("scope-child") => {
            let directory = Path::new(&args[2]);
            fs::write(directory.join("scope-pid"), std::process::id().to_string()).unwrap();
            wait(
                || directory.join("scope-go").exists(),
                "parent did not release scope helper",
            );
            // Double-forking launchers and ordinary detached descendants retain
            // kernel cgroup membership. No name matching or fixed exit timeout.
            // Intentionally orphan this short fixture: the controller waits for
            // its real scope to empty and systemd/init reaps the descendant.
            #[allow(clippy::zombie_processes)]
            Command::new("/bin/sleep")
                .arg("1.5")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
        }
        _ => run(),
    }
}
