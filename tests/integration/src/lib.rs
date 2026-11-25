//! Cross-crate integration tests. These exercise the *real* wire path:
//! an NDJSON Unix-socket server dispatching to mock hardware through the
//! public crates, driven by the real IPC client — the same protocol the
//! daemon, CLI and GUI speak.

#[cfg(test)]
mod tests {
    use omen_core::capability::Capabilities;
    use omen_core::config::OmenConfig;
    use omen_core::fans::FanMode;
    use omen_core::OmenError;
    use omen_hardware::mock::{MockBackend, MockModel};
    use omen_hardware::HardwareBackend;
    use omen_ipc::protocol::{Op, Request, Response};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Dispatcher equivalent to the daemon's (the daemon's own unit tests cover
    /// its full op set; here we verify the wire + backend composition).
    struct ServiceShim {
        backend: Arc<MockBackend>,
        #[allow(dead_code)]
        config: std::sync::Mutex<OmenConfig>,
    }

    impl ServiceShim {
        fn handle(&self, op: Op) -> Response {
            match op {
                Op::Ping => Response::ok(1, "pong"),
                Op::GetCapabilities => Response::ok(1, self.backend.capabilities()),
                Op::GetSensors => match self.backend.read_sensors() {
                    Ok(s) => Response::ok(1, s),
                    Err(e) => Response::err(1, e),
                },
                Op::SetFanMode { mode } => match self.backend.set_fan_mode(mode) {
                    Ok(()) => Response::ok(1, "applied"),
                    Err(e) => Response::err(1, e),
                },
                Op::GetFanState => match self.backend.fan_state() {
                    Ok(s) => Response::ok(1, s),
                    Err(e) => Response::err(1, e),
                },
                Op::GetBattery => match self.backend.battery() {
                    Ok(b) => Response::ok(1, b),
                    Err(e) => Response::err(1, e),
                },
                other => Response::err(1, OmenError::NotImplemented(other.name().to_string())),
            }
        }
    }

    struct TestDaemon {
        dir: PathBuf,
        socket: PathBuf,
        client: omen_ipc::IpcClient,
    }

    impl Drop for TestDaemon {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn start_daemon(model: MockModel) -> TestDaemon {
        let base = std::env::temp_dir();
        let dir_name = format!(
            "omen-it-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        // Unix domain socket paths are capped by SUN_LEN (108 bytes). Fall back
        // to /tmp when the inherited TMPDIR is too deep to bind a socket.
        let root = if base.join(&dir_name).join("ctl.sock").as_os_str().len() < 100 {
            base
        } else {
            PathBuf::from("/tmp")
        };
        let dir = root.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("ctl.sock");
        let service = Arc::new(ServiceShim {
            backend: Arc::new(MockBackend::new(model)),
            config: std::sync::Mutex::new(OmenConfig::default()),
        });
        let listener = UnixListener::bind(&socket).unwrap();
        {
            let service = service.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let svc = service.clone();
                    std::thread::spawn(move || {
                        let Ok(mut writer) = stream.try_clone() else {
                            return;
                        };
                        let reader = BufReader::new(stream);
                        for line in reader.lines().map_while(Result::ok) {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let resp: Response = match serde_json::from_str::<Request>(&line) {
                                Ok(req) => {
                                    let mut r = svc.handle(req.op);
                                    r.id = req.id;
                                    r
                                }
                                Err(e) => {
                                    Response::err(0, OmenError::InvalidArgument(format!("{e}")))
                                }
                            };
                            let mut out = serde_json::to_string(&resp).unwrap();
                            out.push('\n');
                            if writer.write_all(out.as_bytes()).is_err() {
                                break;
                            }
                        }
                    });
                }
            });
        }
        TestDaemon {
            dir,
            socket: socket.clone(),
            client: omen_ipc::IpcClient::new(socket),
        }
    }

    #[test]
    fn wire_roundtrip_ping_capabilities_sensors() {
        let d = start_daemon(MockModel::VictusFb);
        let pong: String = d.client.call_decode(Op::Ping).unwrap();
        assert_eq!(pong, "pong");
        let caps: Capabilities = d.client.call_decode(Op::GetCapabilities).unwrap();
        assert_eq!(caps.machine.board_name, "8C30");
        assert!(!caps.fan_control.is_available());
        assert!(caps.fan_monitoring.is_available());
        let sensors: Vec<omen_core::sensors::SensorSample> =
            d.client.call_decode(Op::GetSensors).unwrap();
        assert!(sensors.iter().any(|s| s.value == Some(61.8)));
    }

    #[test]
    fn wire_fan_write_refused_for_unsupported_board() {
        let d = start_daemon(MockModel::VictusFb);
        let resp = d
            .client
            .call(Op::SetFanMode {
                mode: FanMode::Manual { duty_pct: 80 },
            })
            .unwrap();
        assert!(!resp.ok);
        assert!(matches!(resp.error.unwrap(), OmenError::Unsupported(_)));
    }

    #[test]
    fn wire_fan_write_applies_on_supported_board() {
        let d = start_daemon(MockModel::FanControlOmen);
        let resp = d
            .client
            .call(Op::SetFanMode {
                mode: FanMode::Manual { duty_pct: 65 },
            })
            .unwrap();
        assert!(resp.ok, "{resp:?}");
        let st: omen_core::fans::FanState = d.client.call_decode(Op::GetFanState).unwrap();
        assert!(
            matches!(st.mode, Some(FanMode::Manual { duty_pct }) if (60..=70).contains(&duty_pct))
        );
    }

    #[test]
    fn wire_battery_read() {
        let d = start_daemon(MockModel::VictusFb);
        let b: omen_core::power::BatteryInfo = d.client.call_decode(Op::GetBattery).unwrap();
        assert_eq!(b.capacity_pct, Some(100));
    }

    #[test]
    fn wire_unknown_op_reports_not_implemented() {
        let d = start_daemon(MockModel::VictusFb);
        let resp = d.client.call(Op::GetGames).unwrap();
        assert!(!resp.ok);
        assert!(matches!(resp.error.unwrap(), OmenError::NotImplemented(_)));
    }

    #[test]
    fn protocol_rejects_garbage_lines_cleanly() {
        let d = start_daemon(MockModel::VictusFb);
        let mut s = UnixStream::connect(&d.socket).unwrap();
        writeln!(s, "{{not json").unwrap();
        let reader = BufReader::new(s);
        let mut lines = reader.lines();
        let l = lines.next().unwrap().unwrap();
        let r: Response = serde_json::from_str(&l).unwrap();
        assert!(!r.ok);
        assert!(matches!(r.error.unwrap(), OmenError::InvalidArgument(_)));
    }
}
