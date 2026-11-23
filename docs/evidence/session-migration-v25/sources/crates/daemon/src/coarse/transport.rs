use super::actuator::Identity;
use super::{credentials, invalid, Reply, Request, SOCKET, TARGET};
use omen_core::fans::{FanMode, FanSupervisorStatus};
use omen_core::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream, limit: usize) -> Result<T> {
    let deadline = Instant::now() + Duration::from_millis(800);
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| invalid("recovery frame deadline exceeded"))?;
        stream.set_read_timeout(Some(remaining.min(Duration::from_millis(200))))?;
        let mut b = [0];
        if stream.read(&mut b)? == 0 {
            return Err(invalid("recovery channel closed"));
        }
        if b[0] == b'\n' {
            return serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()));
        }
        bytes.push(b[0]);
        if bytes.len() > limit {
            return Err(invalid("oversized recovery frame"));
        }
    }
}
pub(super) fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    Ok(())
}
#[derive(Default)]
struct State {
    token: Option<String>,
    seq: u64,
    status: Option<FanSupervisorStatus>,
    identity: Option<Identity>,
}
pub struct Client {
    path: PathBuf,
    fixture: bool,
    instance: String,
    state: Mutex<State>,
}
impl Client {
    pub fn fixture(path: PathBuf) -> Self {
        Self {
            path,
            fixture: true,
            instance: String::new(),
            state: Mutex::new(State::default()),
        }
    }
    pub fn live(instance: String) -> Self {
        Self {
            path: PathBuf::from(SOCKET),
            fixture: false,
            instance,
            state: Mutex::new(State::default()),
        }
    }
    fn exchange(&self, request: &Request, state: &mut State) -> Result<()> {
        let mut stream = UnixStream::connect(&self.path)?;
        let peer = credentials(&stream, self.fixture, false)?;
        stream.set_write_timeout(Some(Duration::from_millis(800)))?;
        if !self.fixture
            && (!peer
                .cgroup
                .trim()
                .ends_with(&format!("/omen-fan-recovery@{}.service", self.instance))
                || peer.cgroup == std::fs::read_to_string("/proc/self/cgroup")?)
        {
            return Err(invalid(
                "recovery writer is not in its separate expected service",
            ));
        }
        send(&mut stream, request)?;
        let reply: Reply = read_frame(&mut stream, 4096)?;
        if reply.seq != request.seq
            || reply.fixture != self.fixture
            || reply.status.pid != Some(peer.pid as u32)
        {
            return Err(invalid("recovery reply identity or fixture/live mismatch"));
        }
        if !self.fixture
            && (reply.identity.target != TARGET
                || reply.identity.path
                    != Path::new("/sys/devices/platform/hp-wmi/hwmon").join(&self.instance))
        {
            return Err(invalid("recovery writer has a different live target"));
        }
        if state
            .identity
            .as_ref()
            .is_some_and(|i| i != &reply.identity)
        {
            return Err(invalid(
                "recovery actuator changed; restart and review required",
            ));
        }
        state.identity = Some(reply.identity);
        state.status = Some(reply.status);
        if !reply.ok {
            return Err(invalid(
                reply
                    .error
                    .unwrap_or_else(|| "recovery command refused".into()),
            ));
        }
        match request.op.as_str() {
            "arm" => {
                state.token = Some(
                    reply
                        .token
                        .ok_or_else(|| invalid("missing recovery lease token"))?,
                );
                state.seq = 0;
            }
            "status" => {}
            _ => state.seq = request.seq,
        }
        Ok(())
    }
    fn call(&self, op: &str) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // Status handshake verifies fixture/live and actuator identity BEFORE arm.
        if state.identity.is_none() && op != "status" {
            self.exchange(
                &Request {
                    op: "status".into(),
                    token: None,
                    seq: 0,
                },
                &mut state,
            )?;
        }
        let seq = if matches!(op, "status" | "arm") {
            0
        } else {
            state
                .seq
                .checked_add(1)
                .ok_or_else(|| invalid("sequence exhausted"))?
        };
        let request = Request {
            op: op.into(),
            token: state.token.clone(),
            seq,
        };
        self.exchange(&request, &mut state)
    }
    pub fn is_fixture(&self) -> bool {
        self.fixture
    }
    pub fn begin(&self) -> Result<()> {
        self.call("arm")
    }
    pub fn apply(&self, mode: FanMode) -> Result<()> {
        match mode {
            FanMode::Auto => {
                self.begin()?;
                self.call("auto")
            }
            FanMode::Max => self.call("max"),
            _ => Err(invalid(
                "independent coarse writer refuses manual duty and curves",
            )),
        }
    }
    pub fn pulse(&self) -> Result<()> {
        self.call("pulse")
    }
    pub fn check(&self) -> Result<FanSupervisorStatus> {
        self.call("status")?;
        self.status()
            .ok_or_else(|| invalid("missing recovery status"))
    }
    pub fn status(&self) -> Option<FanSupervisorStatus> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
}

/// Closed fixture probe, authenticated as this executable. It requires the
/// server's fixture handshake and cannot operate a live recovery writer.
pub fn fixture_probe(path: &Path, op: &str) -> Result<()> {
    let client = Client::fixture(path.into());
    client.check()?;
    if op == "status" {
        return Ok(());
    }
    client.begin()?;
    if op == "auto" {
        return client.apply(FanMode::Auto);
    }
    client.apply(FanMode::Max)?;
    println!("maximum");
    std::io::stdout().flush()?;
    match op {
        "hold" => loop {
            std::thread::sleep(Duration::from_millis(500));
            client.pulse()?;
        },
        "max" => Ok(()),
        "replay" | "manual" => {
            let mut state = client.state.lock().unwrap();
            let request = Request {
                op: if op == "manual" { "manual" } else { "pulse" }.into(),
                token: state.token.clone(),
                seq: if op == "manual" {
                    state.seq + 1
                } else {
                    state.seq
                },
            };
            if client.exchange(&request, &mut state).is_ok() {
                return Err(invalid("invalid fixture command was accepted"));
            }
            Ok(())
        }
        "malformed" => {
            let mut stream = UnixStream::connect(path)?;
            stream.write_all(b"{broken\n")?;
            std::thread::sleep(Duration::from_millis(150));
            Ok(())
        }
        "late" => {
            std::thread::sleep(Duration::from_millis(4400));
            if client.pulse().is_ok() {
                return Err(invalid("expired lease renewed"));
            }
            Ok(())
        }
        _ => Err(invalid("unknown fixture probe")),
    }
}
