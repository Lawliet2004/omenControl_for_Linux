use super::*;
use omen_core::sensors::{SensorKind, SensorSource};
use omen_ipc::{protocol::DaemonInfo, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Service {
    stop: Arc<AtomicBool>,
    flip: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Service {
    fn new(path: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flip = Arc::new(AtomicBool::new(false));
        let info = Arc::new(Mutex::new(DaemonInfo {
            protocol_version: omen_ipc::protocol::PROTOCOL_VERSION,
            daemon_version: "fixture".into(),
            machine: Default::default(),
            mock: true,
            started_unix: 1,
        }));
        let (done, change) = (stop.clone(), flip.clone());
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_millis(100)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let request: Request = serde_json::from_str(&line).unwrap();
                    let response = match request.op {
                        Op::GetInfo => Response::ok(request.id, info.lock().unwrap().clone()),
                        Op::GetCapabilities => Response::ok(request.id, fallback_caps()),
                        Op::GetSensors => {
                            if change.swap(false, Ordering::SeqCst) {
                                info.lock().unwrap().mock = false;
                            }
                            Response::ok(
                                request.id,
                                vec![SensorSample::numeric(
                                    "fixture/temp",
                                    "Fixture temperature",
                                    SensorKind::Temperature,
                                    SensorSource::Mock,
                                    47.0,
                                )],
                            )
                        }
                        Op::GetProfile => Response::ok(request.id, ProfileKind::Balanced),
                        _ => Response::err(
                            request.id,
                            OmenError::DaemonUnavailable("fixture unavailable".into()),
                        ),
                    };
                    let mut encoded = serde_json::to_vec(&response).unwrap();
                    encoded.push(b'\n');
                    if reader.get_mut().write_all(&encoded).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            stop,
            flip,
            thread: Some(thread),
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn late_service_reconnects_and_replacement_has_a_new_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("service");
    let mut poller = SourcePoller::at(IpcClient::new(&path), false);
    let absent = poller.poll();
    assert!(matches!(absent.route, DataSource::Offline));
    assert!(absent.origin.fixture.is_none());
    assert!(absent.sensors.is_empty());
    let original = Service::new(&path);
    let first = poller.poll();
    assert_eq!(first.origin.fixture, Some(true));
    assert_eq!(first.sensors.len(), 1);
    let second = poller.poll();
    let (DataSource::Daemon(a), DataSource::Daemon(b)) = (&first.route, &second.route) else {
        panic!("service route missing")
    };
    assert!(
        Arc::ptr_eq(a, b),
        "same service must retain the local telemetry cache"
    );
    std::fs::remove_file(&path).unwrap();
    let _replacement = Service::new(&path);
    let third = poller.poll();
    assert_ne!(first.origin.identity, third.origin.identity);
    assert_eq!(third.origin.fixture, Some(true));
    assert!(first.route.daemon().unwrap().call(Op::Ping).is_err());
    drop(original);
}

#[test]
fn metadata_changed_during_acquisition_discards_the_entire_frame() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("service");
    let service = Service::new(&path);
    let mut poller = SourcePoller::at(IpcClient::new(path), false);
    service.flip.store(true, Ordering::SeqCst);
    let mixed = poller.poll();
    assert!(mixed.origin.identity.is_none());
    assert!(mixed.origin.fixture.is_none());
    assert!(mixed.sensors.is_empty());
    assert!(mixed.profile.is_none());
    assert!(!mixed.capabilities.performance_profiles.is_available());
    assert!(matches!(mixed.route, DataSource::Offline));
    assert!(mixed.error.unwrap().contains("changed during"));
}

#[test]
fn source_tracker_retains_edits_during_outage_and_resets_on_replacement() {
    let origin = |key: &str| SourceOrigin {
        identity: Some(key.into()),
        ..Default::default()
    };
    let mut tracker = SourceTracker::default();
    assert!(!tracker.changed(&origin("first")));
    assert!(!tracker.changed(&SourceOrigin::default()));
    assert!(!tracker.changed(&origin("first")));
    assert!(tracker.changed(&origin("second")));
    assert!(!tracker.changed(&origin("second")));
}

#[test]
fn unbound_user_endpoint_never_establishes_physical_provenance() {
    let client = IpcClient::new("unused");
    let mut info = DaemonInfo {
        protocol_version: 1,
        daemon_version: "test".into(),
        machine: Default::default(),
        mock: false,
        started_unix: 1,
    };
    assert_eq!(SourceOrigin::service(&client, &info).fixture, None);
    assert_eq!(
        SourceOrigin::service(&client, &info).status,
        "Source unverified"
    );
    info.mock = true;
    assert_eq!(SourceOrigin::service(&client, &info).fixture, Some(true));
}
